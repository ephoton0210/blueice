// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A private launcher directory whose `blueice-core` is a shell wrapper around
//! the real one, so a test can make chosen invocations misbehave or swap the
//! binary underneath a running launcher. The launcher finds `core` beside its
//! own executable, so the launcher, the real core, and its siblings are placed
//! (hard-linked) into one directory per test.

#![allow(dead_code)] // each test binary uses a different part of the rig

use blueice_ipc::{
    client_handshake, read_server_message, write_client_message, ClientMessage, ServerMessage,
};
use blueice_launcher::control::{
    read_control_reply, write_control_request, ControlReply, ControlRequest,
};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

pub fn unique_dir(label: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("l-retry-{label}-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Hard-links `from` to `to` (so `current_exe` resolves inside the private
/// directory), copying only if the two are on different filesystems.
pub fn place(from: &Path, to: &Path) {
    if std::fs::hard_link(from, to).is_err() {
        std::fs::copy(from, to).unwrap();
    }
}

pub fn wait_for(path: &Path) -> bool {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    false
}

pub struct Rig {
    pub dir: PathBuf,
    pub child: Child,
    pub rendezvous: PathBuf,
    pub control: PathBuf,
}

impl Rig {
    /// `core_script` is the body run in place of `blueice-core`; it can read
    /// the invocation count in `$n` and must eventually `exec "$REAL" "$@"`.
    pub fn start(label: &str, core_script: &str) -> Rig {
        Self::start_with(label, core_script, &[])
    }

    /// [`Self::start`] with extra launcher arguments.
    pub fn start_with(label: &str, core_script: &str, extra_args: &[&str]) -> Rig {
        let dir = unique_dir(label);
        let built = PathBuf::from(env!("CARGO_BIN_EXE_blueice-launcher"));
        let built_dir = built.parent().unwrap();
        place(&built, &dir.join("blueice-launcher"));
        for sibling in ["bluejs", "blueice-ai-gatekeeper"] {
            place(&built_dir.join(sibling), &dir.join(sibling));
        }
        place(
            &built_dir.join("blueice-core"),
            &dir.join("blueice-core-real"),
        );
        let script = format!(
            "#!/bin/sh\nDIR=\"$(dirname \"$0\")\"\nREAL=\"$DIR/blueice-core-real\"\n\
             n=$(cat \"$DIR/count\" 2>/dev/null || echo 0)\nn=$((n+1))\necho $n > \"$DIR/count\"\n{core_script}\n"
        );
        let core = dir.join("blueice-core");
        std::fs::write(&core, script).unwrap();
        std::fs::set_permissions(&core, std::fs::Permissions::from_mode(0o755)).unwrap();

        let rendezvous = dir.join("rv.sock");
        let control = dir.join("ctl.sock");
        let child = Command::new(dir.join("blueice-launcher"))
            .args(["--socket", rendezvous.to_str().unwrap()])
            .args(["--control-socket", control.to_str().unwrap()])
            .args(["--width", "320", "--height", "200"])
            .args(["--frame-dir", dir.join("frames").to_str().unwrap()])
            .args(extra_args)
            .spawn()
            .expect("failed to start the launcher");
        assert!(wait_for(&rendezvous), "the launcher never listened");
        assert!(
            wait_for(&control),
            "the launcher never opened its control socket"
        );
        Rig {
            dir,
            child,
            rendezvous,
            control,
        }
    }

    pub fn invocations(&self) -> u32 {
        std::fs::read_to_string(self.dir.join("count"))
            .unwrap()
            .trim()
            .parse()
            .unwrap()
    }

    pub fn cutover(&self) -> ControlReply {
        let mut control = UnixStream::connect(&self.control).unwrap();
        write_control_request(&mut control, &ControlRequest::Cutover).unwrap();
        read_control_reply(&mut control).unwrap()
    }

    pub fn client(&self) -> UnixStream {
        let mut stream = UnixStream::connect(&self.rendezvous).unwrap();
        client_handshake(&mut stream).unwrap();
        stream
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        if let Ok(mut stream) = UnixStream::connect(&self.rendezvous) {
            let _ = client_handshake(&mut stream);
            let _ = write_client_message(&mut stream, &ClientMessage::Shutdown);
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline && self.child.try_wait().ok().flatten().is_none() {
                thread::sleep(Duration::from_millis(50));
            }
        }
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub fn open_credits(stream: &mut UnixStream) {
    write_client_message(
        stream,
        &ClientMessage::Navigate {
            url: "about:credits".into(),
        },
    )
    .unwrap();
    while !matches!(
        read_server_message(stream).unwrap(),
        ServerMessage::FrameReady { .. }
    ) {}
}

pub fn url_now(stream: &mut UnixStream) -> Option<String> {
    write_client_message(stream, &ClientMessage::GetRepresentation).unwrap();
    loop {
        if let ServerMessage::Representation(snapshot) = read_server_message(stream).unwrap() {
            return snapshot.url;
        }
    }
}
