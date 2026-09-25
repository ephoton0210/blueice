// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! End-to-end proof of `phase-8-live-core-hotswap/PLAN.md`'s bounded cutover
//! retry through the real compiled launcher. The launcher finds `core` beside
//! its own executable, so each test builds a private directory holding a
//! launcher and a `blueice-core` that is a shell wrapper around the real one,
//! and the wrapper misbehaves on chosen invocations (the first is v1, the
//! next ones are cutover attempts).

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

fn unique_dir(label: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("l-retry-{label}-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Hard-links `from` to `to` (so `current_exe` resolves inside the private
/// directory), copying only if the two are on different filesystems.
fn place(from: &Path, to: &Path) {
    if std::fs::hard_link(from, to).is_err() {
        std::fs::copy(from, to).unwrap();
    }
}

fn wait_for(path: &Path) -> bool {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    false
}

struct Rig {
    dir: PathBuf,
    child: Child,
    rendezvous: PathBuf,
    control: PathBuf,
}

impl Rig {
    /// `core_script` is the body run in place of `blueice-core`; it can read
    /// the invocation count in `$n` and must eventually `exec "$REAL" "$@"`.
    fn start(label: &str, core_script: &str) -> Rig {
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

    fn invocations(&self) -> u32 {
        std::fs::read_to_string(self.dir.join("count"))
            .unwrap()
            .trim()
            .parse()
            .unwrap()
    }

    fn cutover(&self) -> ControlReply {
        let mut control = UnixStream::connect(&self.control).unwrap();
        write_control_request(&mut control, &ControlRequest::Cutover).unwrap();
        read_control_reply(&mut control).unwrap()
    }

    fn client(&self) -> UnixStream {
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

fn open_credits(stream: &mut UnixStream) {
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

fn url_now(stream: &mut UnixStream) -> Option<String> {
    write_client_message(stream, &ClientMessage::GetRepresentation).unwrap();
    loop {
        if let ServerMessage::Representation(snapshot) = read_server_message(stream).unwrap() {
            return snapshot.url;
        }
    }
}

#[test]
fn a_cutover_whose_first_v2_fails_to_start_succeeds_on_a_retry() {
    // Invocation 1 is v1; invocation 2 (the first cutover attempt's v2) dies at
    // once; invocation 3 (the retry) is healthy.
    let rig = Rig::start(
        "transient",
        "if [ \"$n\" = \"2\" ]; then exit 1; fi\nexec \"$REAL\" \"$@\"",
    );
    let mut client = rig.client();
    open_credits(&mut client);

    let started = Instant::now();
    match rig.cutover() {
        ControlReply::CutoverDone { tabs_migrated } => assert_eq!(tabs_migrated, 1),
        other => panic!("expected the retry to succeed, got {other:?}"),
    }
    assert_eq!(
        rig.invocations(),
        3,
        "v1, the failed attempt, and the retry"
    );
    // The failed start was noticed at once, not after the 5 s startup wait.
    assert!(
        started.elapsed() < Duration::from_secs(15),
        "{:?}",
        started.elapsed()
    );

    // The same, still-open client connection now talks to v2 and sees the tab.
    assert_eq!(url_now(&mut client).as_deref(), Some("about:credits"));
}

#[test]
fn a_cutover_that_never_succeeds_gives_up_after_three_attempts_and_v1_keeps_serving() {
    let rig = Rig::start(
        "permanent",
        "if [ \"$n\" -ge 2 ]; then exit 1; fi\nexec \"$REAL\" \"$@\"",
    );
    let mut client = rig.client();
    open_credits(&mut client);

    match rig.cutover() {
        ControlReply::CutoverFailed { reason } => {
            assert!(reason.contains("gave up after 3 attempts"), "{reason}");
            assert!(reason.contains("failed to spawn v2"), "{reason}");
        }
        other => panic!("expected CutoverFailed, got {other:?}"),
    }
    assert_eq!(rig.invocations(), 4, "v1 and exactly three attempts");

    // v1 was never touched: the client still works, and a later cutover is
    // accepted (not stuck busy).
    assert_eq!(url_now(&mut client).as_deref(), Some("about:credits"));
    assert!(matches!(rig.cutover(), ControlReply::CutoverFailed { .. }));
}
