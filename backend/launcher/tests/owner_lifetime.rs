// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(unix)]

use blueice_ipc::{client_handshake, write_client_message, ClientMessage};
use blueice_launcher::control::{
    read_control_reply, write_control_request, ControlReply, ControlRequest,
};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

struct Rig {
    child: Child,
    directory: PathBuf,
}

impl Rig {
    fn start(owner_pipe: bool) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        // Short on Darwin too, independently of the test runner's long TMPDIR.
        let directory = PathBuf::from(format!(
            "/tmp/bi-owner-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        // Match the native bundle's sibling layout. On macOS, use fresh local
        // signatures just as the frontend build does, rather than depending on
        // the loader's treatment of linker-signed Cargo artifacts in place.
        let binaries = directory.join("bin");
        std::fs::create_dir(&binaries).unwrap();
        let override_binary = std::env::var_os("BLUEICE_TEST_LAUNCHER_EXE").map(PathBuf::from);
        let built = PathBuf::from(env!("CARGO_BIN_EXE_blueice-launcher"));
        for name in ["blueice-launcher", "blueice-core", "blueice-ai-gatekeeper"] {
            if override_binary.is_some() {
                break;
            }
            let target = binaries.join(name);
            std::fs::copy(built.parent().unwrap().join(name), &target).unwrap();
            #[cfg(target_os = "macos")]
            assert!(Command::new("/usr/bin/codesign")
                .args(["--force", "--sign", "-"])
                .arg(&target)
                .status()
                .unwrap()
                .success());
        }
        let mut command =
            Command::new(override_binary.unwrap_or_else(|| binaries.join("blueice-launcher")));
        command
            .args(["--socket"])
            .arg(directory.join("browser.sock"))
            .arg("--control-socket")
            .arg(directory.join("control.sock"))
            .arg("--frame-dir")
            .arg(directory.join("frames"))
            .args(["--width", "320", "--height", "200"])
            .env("TMPDIR", &directory)
            .env("XDG_RUNTIME_DIR", &directory)
            .stdin(Stdio::piped())
            .process_group(0);
        if owner_pipe {
            command.arg("--exit-on-stdin-eof");
        }
        Self {
            child: command.spawn().unwrap(),
            directory,
        }
    }

    fn connect(&mut self, name: &str) -> UnixStream {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Ok(stream) = UnixStream::connect(self.directory.join(name)) {
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                return stream;
            }
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "launcher exited before listening"
            );
            assert!(Instant::now() < deadline, "launcher never listened");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_for_clean_exit(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success(), "launcher failed: {status}");
                break;
            }
            assert!(
                Instant::now() < deadline,
                "owner EOF did not stop the launcher"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!self.directory.join("browser.sock").exists());
        assert!(!self.directory.join("control.sock").exists());
        assert!(!self.directory.join("frames").exists());
        assert!(
            std::fs::read_dir(self.directory.join("blueice"))
                .unwrap()
                .next()
                .is_none(),
            "gatekeeper socket was not cleaned up"
        );
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            // This group was created atomically for this test child alone.
            unsafe {
                libc::kill(-(self.child.id() as libc::pid_t), libc::SIGKILL);
            }
            let _ = self.child.wait();
        }
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn owner_eof_stops_real_services_and_cleans_their_paths() {
    let mut rig = Rig::start(true);
    let mut control = rig.connect("control.sock");
    write_control_request(&mut control, &ControlRequest::Status).unwrap();
    let ControlReply::Status(status) = read_control_reply(&mut control).unwrap() else {
        panic!("expected status");
    };
    let core = status.core_pid.unwrap() as libc::pid_t;
    let mut browser = rig.connect("browser.sock");
    client_handshake(&mut browser).unwrap();
    write_client_message(
        &mut browser,
        &ClientMessage::Navigate {
            url: "about:credits".into(),
        },
    )
    .unwrap();
    // Pending broadcasts may precede the lifetime monitor's Hello response.
    drop(rig.child.stdin.take());
    rig.wait_for_clean_exit();
    assert_eq!(
        unsafe { libc::kill(core, 0) },
        -1,
        "owned core was not reaped"
    );
}

#[test]
fn owner_eof_before_any_browser_connects_still_cleans_up() {
    let mut rig = Rig::start(true);
    drop(rig.child.stdin.take());
    rig.wait_for_clean_exit();
}

#[test]
fn stdin_eof_without_opt_in_preserves_the_existing_broker_lifetime() {
    let mut rig = Rig::start(false);
    drop(rig.child.stdin.take());
    let mut browser = rig.connect("browser.sock");
    client_handshake(&mut browser).unwrap();
    assert!(rig.child.try_wait().unwrap().is_none());
    write_client_message(&mut browser, &ClientMessage::Shutdown).unwrap();
    rig.wait_for_clean_exit();
}
