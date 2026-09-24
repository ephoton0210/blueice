// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Explicit graphical-session smoke test. Unlike the ordinary headless
//! launcher suite, this starts the exact sibling `blueice-frontend` window.
//! The launcher refuses to become ready unless that child has completed a
//! private-pipe Inspect after connecting to the shared core and creating its
//! real native window. Killing the exact child must then stop the broker and
//! retire its process-local optional grants. It is ignored
//! in headless CI and can be run in a real desktop session after building
//! the launcher, frontend, core, BlueJS, and extension-host binaries.

use blueice_launcher::control::{
    read_control_reply, write_control_request, ControlReply, ControlRequest,
};
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
#[ignore = "requires a graphical session and built sibling frontend/core/BlueJS/extension-host binaries"]
fn launcher_owned_native_window_inspects_and_its_loss_stops_the_broker() {
    let launcher_bin = PathBuf::from(env!("CARGO_BIN_EXE_blueice-launcher"));
    let frontend_bin = launcher_bin.with_file_name("blueice-frontend");
    assert!(frontend_bin.exists(), "build blueice-frontend beside the launcher first");
    assert!(launcher_bin.with_file_name("blueice-extension-host").exists(),
        "build blueice-extension-host beside the launcher first");
    let root = std::env::temp_dir().join(format!(
        "btw-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() % 10_000
    ));
    std::fs::create_dir(&root).unwrap();
    let root = TestRoot(root);
    let rendezvous = root.0.join("core.sock");
    let control = root.0.join("control.sock");
    let frames = root.0.join("frames");
    let manifest = root.0.join("extension.json");
    std::fs::write(&manifest,
        r#"{"name":"Native inspection proof","version":"1","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"declared":["dom:read"],"optional":["storage"]}}"#,
    ).unwrap();
    std::fs::write(root.0.join("extension.wasm"),
        wat::parse_str(r#"(module (func (export "blueice_start")))"#).unwrap(),
    ).unwrap();
    let launcher = Command::new(launcher_bin)
        .args([
            "--socket", rendezvous.to_str().unwrap(),
            "--control-socket", control.to_str().unwrap(),
            "--frame-dir", frames.to_str().unwrap(),
            "--extension-manifest", manifest.to_str().unwrap(),
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
    let mut inspector = UnixStream::connect(&control).unwrap();
    write_control_request(&mut inspector, &ControlRequest::InspectExtensionPermissions).unwrap();
    assert!(matches!(read_control_reply(&mut inspector).unwrap(),
        ControlReply::ExtensionPermissions { core_generation: 0, installed: Some(ref package) }
            if package.name == "Native inspection proof"
                && package.optional.len() == 1
                && package.optional[0].capability == "storage"
                && !package.optional[0].granted
    ));

    let mut client = UnixStream::connect(&rendezvous).unwrap();
    blueice_ipc::client_handshake(&mut client).unwrap();
    let mut processes = sysinfo::System::new_all();
    processes.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let frontend = processes.processes().values()
        .find(|process| {
            process.parent() == Some(sysinfo::Pid::from_u32(launcher.0.id()))
                && process.exe() == Some(frontend_bin.as_path())
        })
        .expect("the live launcher must own the exact sibling frontend child");
    assert_eq!(frontend.kill_with(sysinfo::Signal::Kill), Some(true));
    let deadline = Instant::now() + Duration::from_secs(5);
    while launcher.0.try_wait().unwrap().is_none() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    if launcher.0.try_wait().unwrap().is_none() {
        panic!("launcher did not fail closed after its trusted native window died");
    }
    assert!(!rendezvous.exists());
    assert!(!control.exists());
    assert!(!frames.exists());
}
