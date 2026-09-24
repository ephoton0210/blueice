// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Explicit graphical-session smoke test. Unlike the ordinary headless
//! launcher suite, this starts the exact sibling `blueice-frontend` window.
//! The launcher refuses to become ready unless that child has completed a
//! private-pipe Inspect after connecting to the shared core and creating its
//! real native window. It is ignored
//! in headless CI and can be run in a real desktop session after building
//! both binaries.

use blueice_ipc::{write_client_message, ClientMessage};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

struct TestLauncher(Child);

impl Drop for TestLauncher {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct TestRoot(PathBuf);

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
#[ignore = "requires a graphical session and a built sibling blueice-frontend"]
fn launcher_owned_native_window_completes_its_private_read_only_inspection() {
    let launcher_bin = PathBuf::from(env!("CARGO_BIN_EXE_blueice-launcher"));
    let frontend_bin = launcher_bin.with_file_name("blueice-frontend");
    assert!(frontend_bin.exists(), "build blueice-frontend beside the launcher first");
    let root = std::env::temp_dir().join(format!(
        "btw-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() % 10_000
    ));
    std::fs::create_dir(&root).unwrap();
    let root = TestRoot(root);
    let rendezvous = root.0.join("core.sock");
    let control = root.0.join("control.sock");
    let frames = root.0.join("frames");
    let launcher = Command::new(launcher_bin)
        .args([
            "--socket", rendezvous.to_str().unwrap(),
            "--control-socket", control.to_str().unwrap(),
            "--frame-dir", frames.to_str().unwrap(),
            "--trusted-frontend",
        ])
        .spawn()
        .expect("failed to spawn the graphical trusted-window launcher");
    let mut launcher = TestLauncher(launcher);
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if let Some(status) = launcher.0.try_wait().unwrap() {
            panic!("launcher exited before its trusted child inspection: {status}");
        }
        if rendezvous.exists() {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    assert!(rendezvous.exists(), "launcher never bound its shared core socket");
    // Allow the broker's ten-second child-inspection deadline to expire.
    // Remaining alive afterward proves the private Inspect exchange
    // finished after window creation; a mere socket bind does not.
    thread::sleep(Duration::from_secs(11));
    assert!(launcher.0.try_wait().unwrap().is_none(),
        "launcher did not survive the trusted child-inspection deadline");

    let mut client = UnixStream::connect(&rendezvous).unwrap();
    blueice_ipc::client_handshake(&mut client).unwrap();
    write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while launcher.0.try_wait().unwrap().is_none() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    if launcher.0.try_wait().unwrap().is_none() {
        panic!("launcher did not exit after the shared core shutdown");
    }
    assert!(!rendezvous.exists());
    assert!(!control.exists());
    assert!(!frames.exists());
}
