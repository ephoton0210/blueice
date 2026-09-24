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
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const MANUAL_UI_WAT: &str = r#"(module
    (import "blueice" "runtime_event_kind" (func $kind (result i32)))
    (import "blueice" "set_toolbar_button_utf8" (func $toolbar (param i32 i32) (result i32)))
    (memory (export "memory") 1)
    (data (i32.const 0) "Consent Probe")
    (func (export "blueice_start")
        call $kind
        i32.const 1
        i32.eq
        if
            i32.const 0
            i32.const 13
            call $toolbar
            drop
        end))"#;

const MANUAL_EPHEMERAL_WAT: &str = r#"(module
    (import "blueice" "runtime_event_kind" (func $kind (result i32)))
    (import "blueice" "runtime_event_tab_id" (func $tab (result i64)))
    (import "blueice" "dom_read_ephemeral_utf8" (func $read (param i64 i32 i32) (result i32)))
    (import "blueice" "set_toolbar_button_utf8" (func $toolbar (param i32 i32) (result i32)))
    (memory (export "memory") 2)
    (data (i32.const 0) "Read Ready")
    (data (i32.const 32) "Read Succeeded")
    (func (export "blueice_start")
        call $kind
        i32.const 0
        i32.eq
        if
            i32.const 0
            i32.const 10
            call $toolbar
            i32.const 0
            i32.ne
            if unreachable end
        end
        call $kind
        i32.const 4
        i32.eq
        if
            call $tab
            i32.const 128
            i32.const 65536
            call $read
            i32.const 0
            i32.le_s
            if unreachable end
            call $tab
            i32.const 128
            i32.const 65536
            call $read
            i32.const -1
            i32.ne
            if unreachable end
            i32.const 32
            i32.const 14
            call $toolbar
            i32.const 0
            i32.ne
            if unreachable end
        end))"#;

#[test]
fn manual_consent_fixture_compiles_to_a_real_wasm_module() {
    let bytes = wat::parse_str(MANUAL_UI_WAT).unwrap();
    assert_eq!(&bytes[..4], b"\0asm");
    let ephemeral = wat::parse_str(MANUAL_EPHEMERAL_WAT).unwrap();
    assert_eq!(&ephemeral[..4], b"\0asm");
}

struct TestLauncher(Child, PathBuf);

impl Drop for TestLauncher {
    fn drop(&mut self) {
        // A manual timeout must not leave the trusted window or a granted
        // core orphaned. Lose the exact child authority first so the broker
        // follows its normal fail-closed teardown path.
        let mut processes = sysinfo::System::new_all();
        processes.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        if let Some(frontend) = processes.processes().values().find(|process| {
            process.parent() == Some(sysinfo::Pid::from_u32(self.0.id()))
                && process.exe() == Some(self.1.as_path())
        }) {
            let _ = frontend.kill_with(sysinfo::Signal::Kill);
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if self.0.try_wait().ok().flatten().is_some() {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
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

struct TestProcess(Child);

impl Drop for TestProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_for_path(path: &PathBuf, deadline: Instant) {
    while Instant::now() < deadline {
        if path.exists() {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for {}", path.display());
}

/// The headless companion to the human proof. This uses core's private
/// parent pipe directly, so it validates the real installed-WASM effect
/// lifecycle but deliberately cannot count as native-window consent.
#[test]
fn real_installed_wasm_toolbar_follows_private_optional_grant_and_revoke() {
    use blueice_ipc::permission_control::{
        read_permission_control_reply, write_permission_control_request, PermissionControlReply,
        PermissionControlRequest,
    };
    use std::process::Stdio;

    let launcher_bin = PathBuf::from(env!("CARGO_BIN_EXE_blueice-launcher"));
    let core_bin = launcher_bin.with_file_name("blueice-core");
    let host_bin = launcher_bin.with_file_name("blueice-extension-host");
    let gatekeeper_bin = launcher_bin.with_file_name("blueice-ai-gatekeeper");
    for binary in [&core_bin, &host_bin, &gatekeeper_bin] {
        assert!(
            binary.exists(),
            "build {} beside the launcher first",
            binary.display()
        );
    }
    let root = std::env::temp_dir().join(format!(
        "btw-wasm-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let root = TestRoot(root);
    let core_socket = root.0.join("core.sock");
    let extension_socket = root.0.join("extension.sock");
    let gatekeeper_socket = root.0.join("gatekeeper.sock");
    let frames = root.0.join("frames");
    let manifest = root.0.join("extension.json");
    std::fs::write(&manifest,
        r#"{"name":"Manual consent proof","version":"1","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"optional":["ui:inject"]}}"#,
    ).unwrap();
    std::fs::write(
        root.0.join("extension.wasm"),
        wat::parse_str(MANUAL_UI_WAT).unwrap(),
    )
    .unwrap();

    let gatekeeper = Command::new(gatekeeper_bin)
        .args([
            "--socket",
            gatekeeper_socket.to_str().unwrap(),
            "--settings",
            root.0.join("gatekeeper-settings.json").to_str().unwrap(),
        ])
        .spawn()
        .unwrap();
    let _gatekeeper = TestProcess(gatekeeper);
    wait_for_path(&gatekeeper_socket, Instant::now() + Duration::from_secs(5));
    let core = Command::new(core_bin)
        .args([
            "--socket",
            core_socket.to_str().unwrap(),
            "--extension-socket",
            extension_socket.to_str().unwrap(),
            "--extension-manifest",
            manifest.to_str().unwrap(),
            "--extension-host",
            host_bin.to_str().unwrap(),
            "--permission-control-stdio",
            "--gatekeeper-socket",
            gatekeeper_socket.to_str().unwrap(),
            "--frame-dir",
            frames.to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut core = TestProcess(core);
    wait_for_path(&core_socket, Instant::now() + Duration::from_secs(15));
    let mut parent_input = core.0.stdin.take().unwrap();
    let mut parent_output = core.0.stdout.take().unwrap();
    write_permission_control_request(&mut parent_input, &PermissionControlRequest::Inspect)
        .unwrap();
    assert!(
        matches!(read_permission_control_reply(&mut parent_output).unwrap(),
        PermissionControlReply::State { optional, .. }
            if optional.len() == 1 && !optional[0].granted)
    );

    let mut client = UnixStream::connect(&core_socket).unwrap();
    blueice_ipc::client_handshake(&mut client).unwrap();
    let mut reader = client.try_clone().unwrap();
    let (message_tx, message_rx) = mpsc::channel();
    thread::spawn(move || {
        while let Ok(message) = blueice_ipc::read_server_message(&mut reader) {
            if message_tx.send(message).is_err() {
                break;
            }
        }
    });

    write_permission_control_request(
        &mut parent_input,
        &PermissionControlRequest::Grant {
            capability: "ui:inject".into(),
        },
    )
    .unwrap();
    assert_eq!(
        read_permission_control_reply(&mut parent_output).unwrap(),
        PermissionControlReply::Updated {
            capability: "ui:inject".into(),
            granted: true,
            changed: true
        }
    );

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/optional-ui", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(20));
                }
                Err(error) => panic!("optional UI fixture was never fetched: {error}"),
            }
        };
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 15\r\nConnection: close\r\n\r\n<p>Consent</p>\n").unwrap();
    });
    blueice_ipc::write_client_message(&mut client, &blueice_ipc::ClientMessage::Navigate { url })
        .unwrap();
    await_toolbar(
        &message_rx,
        Some("Consent Probe"),
        Instant::now() + Duration::from_secs(15),
    );
    server.join().unwrap();

    write_permission_control_request(
        &mut parent_input,
        &PermissionControlRequest::Revoke {
            capability: "ui:inject".into(),
        },
    )
    .unwrap();
    assert_eq!(
        read_permission_control_reply(&mut parent_output).unwrap(),
        PermissionControlReply::Updated {
            capability: "ui:inject".into(),
            granted: false,
            changed: true
        }
    );
    await_toolbar(&message_rx, None, Instant::now() + Duration::from_secs(5));
    write_permission_control_request(&mut parent_input, &PermissionControlRequest::Inspect)
        .unwrap();
    assert!(
        matches!(read_permission_control_reply(&mut parent_output).unwrap(),
        PermissionControlReply::State { optional, .. }
            if optional.len() == 1 && !optional[0].granted)
    );
    blueice_ipc::write_client_message(&mut client, &blueice_ipc::ClientMessage::Shutdown).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while core.0.try_wait().unwrap().is_none() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        core.0.try_wait().unwrap().is_some(),
        "core did not exit after Shutdown"
    );
}

/// Headless companion to the person-driven F8 proof below. It arms through
/// core's private parent pipe, so this proves the real core/host/WASM effect
/// and second-read denial, but deliberately does not prove a human gesture.
#[test]
fn real_installed_wasm_consumes_a_private_one_shot_dom_read_once() {
    use blueice_ipc::permission_control::{
        read_permission_control_reply, write_permission_control_request,
        PermissionControlReply, PermissionControlRequest,
    };
    use std::process::Stdio;

    let launcher_bin = PathBuf::from(env!("CARGO_BIN_EXE_blueice-launcher"));
    let core_bin = launcher_bin.with_file_name("blueice-core");
    let host_bin = launcher_bin.with_file_name("blueice-extension-host");
    let gatekeeper_bin = launcher_bin.with_file_name("blueice-ai-gatekeeper");
    for binary in [&core_bin, &host_bin, &gatekeeper_bin] {
        assert!(binary.exists(), "build {} beside the launcher first", binary.display());
    }
    let root = std::env::temp_dir().join(format!(
        "bte-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .unwrap().as_nanos(),
    ));
    std::fs::create_dir(&root).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let root = TestRoot(root);
    let core_socket = root.0.join("core.sock");
    let extension_socket = root.0.join("extension.sock");
    let gatekeeper_socket = root.0.join("gatekeeper.sock");
    let frames = root.0.join("frames");
    let manifest = root.0.join("extension.json");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let url = format!("{origin}/one-shot-proof");
    std::fs::write(&manifest, format!(
        r#"{{"name":"Native one-shot proof","version":"1","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{{"declared":["ui:inject"],"runtime_ephemeral":["dom:read"]}},"capability_origins":{{"dom:read":["{origin}"]}}}}"#,
    )).unwrap();
    std::fs::write(root.0.join("extension.wasm"),
        wat::parse_str(MANUAL_EPHEMERAL_WAT).unwrap()).unwrap();
    let gatekeeper = Command::new(gatekeeper_bin)
        .args(["--socket", gatekeeper_socket.to_str().unwrap(),
            "--settings", root.0.join("gatekeeper-settings.json").to_str().unwrap()])
        .spawn().unwrap();
    let _gatekeeper = TestProcess(gatekeeper);
    wait_for_path(&gatekeeper_socket, Instant::now() + Duration::from_secs(5));
    let core = Command::new(core_bin)
        .args([
            "--socket", core_socket.to_str().unwrap(),
            "--extension-socket", extension_socket.to_str().unwrap(),
            "--extension-manifest", manifest.to_str().unwrap(),
            "--extension-host", host_bin.to_str().unwrap(),
            "--permission-control-stdio",
            "--gatekeeper-socket", gatekeeper_socket.to_str().unwrap(),
            "--frame-dir", frames.to_str().unwrap(),
        ])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    let mut core = TestProcess(core);
    wait_for_path(&core_socket, Instant::now() + Duration::from_secs(15));
    let mut parent_input = core.0.stdin.take().unwrap();
    let mut parent_output = core.0.stdout.take().unwrap();
    write_permission_control_request(&mut parent_input, &PermissionControlRequest::Inspect).unwrap();
    assert!(matches!(read_permission_control_reply(&mut parent_output).unwrap(),
        PermissionControlReply::State { optional, runtime_ephemeral, .. }
            if optional.is_empty() && runtime_ephemeral.len() == 1
                && runtime_ephemeral[0].capability == "dom:read"
                && runtime_ephemeral[0].origins == [origin.clone()]));
    let mut client = UnixStream::connect(&core_socket).unwrap();
    blueice_ipc::client_handshake(&mut client).unwrap();
    let mut reader = client.try_clone().unwrap();
    let (message_tx, message_rx) = mpsc::channel();
    thread::spawn(move || {
        while let Ok(message) = blueice_ipc::read_server_message(&mut reader) {
            if message_tx.send(message).is_err() { break; }
        }
    });
    blueice_ipc::write_client_message(&mut client, &blueice_ipc::ClientMessage::GetExtensionToolbar).unwrap();
    await_toolbar(&message_rx, Some("Read Ready"), Instant::now() + Duration::from_secs(5));
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
                    && Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
                Err(error) => panic!("one-shot fixture was not fetched: {error}"),
            }
        };
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        let body = b"<h1>One-shot proof</h1>";
        stream.write_all(format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len(),
        ).as_bytes()).unwrap();
        stream.write_all(body).unwrap();
    });
    blueice_ipc::write_client_message(&mut client,
        &blueice_ipc::ClientMessage::Navigate { url: url.clone() }).unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut frame_ready = false;
    while !frame_ready && Instant::now() < deadline {
        let message = message_rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("the fixture page did not render");
        frame_ready = matches!(message, blueice_ipc::ServerMessage::FrameReady { .. });
    }
    assert!(frame_ready);
    server.join().unwrap();
    write_permission_control_request(&mut parent_input,
        &PermissionControlRequest::InspectDocument { tab_id: 1 }).unwrap();
    let PermissionControlReply::Document { tab_id: 1, document_epoch, url: Some(live_url) } =
        read_permission_control_reply(&mut parent_output).unwrap() else {
        panic!("core did not expose the committed document identity")
    };
    assert_eq!(live_url, url);
    write_permission_control_request(&mut parent_input,
        &PermissionControlRequest::ArmEphemeral {
            capability: "dom:read".into(), tab_id: 1, document_epoch,
        }).unwrap();
    assert!(matches!(read_permission_control_reply(&mut parent_output).unwrap(),
        PermissionControlReply::EphemeralArmed {
            capability, tab_id: 1, document_epoch: armed_epoch, ..
        } if capability == "dom:read" && armed_epoch == document_epoch));
    await_toolbar(&message_rx, Some("Read Succeeded"), Instant::now() + Duration::from_secs(5));
    blueice_ipc::write_client_message(&mut client, &blueice_ipc::ClientMessage::Shutdown).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while core.0.try_wait().unwrap().is_none() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(core.0.try_wait().unwrap().is_some());
}

#[test]
#[ignore = "requires a graphical session and built sibling frontend/core/BlueJS/extension-host binaries"]
fn launcher_owned_native_window_inspects_and_its_loss_stops_the_broker() {
    let launcher_bin = PathBuf::from(env!("CARGO_BIN_EXE_blueice-launcher"));
    let frontend_bin = launcher_bin.with_file_name("blueice-frontend");
    assert!(
        frontend_bin.exists(),
        "build blueice-frontend beside the launcher first"
    );
    assert!(
        launcher_bin
            .with_file_name("blueice-extension-host")
            .exists(),
        "build blueice-extension-host beside the launcher first"
    );
    let root = std::env::temp_dir().join(format!(
        "btw-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            % 10_000
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
    std::fs::write(
        root.0.join("extension.wasm"),
        wat::parse_str(r#"(module (func (export "blueice_start")))"#).unwrap(),
    )
    .unwrap();
    let launcher = Command::new(launcher_bin)
        .args([
            "--socket",
            rendezvous.to_str().unwrap(),
            "--control-socket",
            control.to_str().unwrap(),
            "--frame-dir",
            frames.to_str().unwrap(),
            "--extension-manifest",
            manifest.to_str().unwrap(),
            "--trusted-frontend",
        ])
        .spawn()
        .expect("failed to spawn the graphical trusted-window launcher");
    let mut launcher = TestLauncher(launcher, frontend_bin.clone());
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
    assert!(
        rendezvous.exists(),
        "launcher never bound its shared core socket"
    );
    // Allow the broker's ten-second child-inspection deadline to expire.
    // Remaining alive afterward proves the private Inspect exchange
    // finished after window creation; a mere socket bind does not.
    thread::sleep(Duration::from_secs(11));
    assert!(
        launcher.0.try_wait().unwrap().is_none(),
        "launcher did not survive the trusted child-inspection deadline"
    );
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
    let frontend = processes
        .processes()
        .values()
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

fn inspected_ui_grant(control: &PathBuf) -> bool {
    let mut inspector = UnixStream::connect(control).unwrap();
    inspector
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    write_control_request(&mut inspector, &ControlRequest::InspectExtensionPermissions).unwrap();
    let ControlReply::ExtensionPermissions {
        installed: Some(package),
        ..
    } = read_control_reply(&mut inspector).unwrap()
    else {
        panic!("the native permission proof lost its installed package");
    };
    assert_eq!(package.name, "Manual consent proof");
    assert_eq!(package.optional.len(), 1);
    assert_eq!(package.optional[0].capability, "ui:inject");
    package.optional[0].granted
}

fn await_ui_grant(control: &PathBuf, expected: bool, deadline: Instant) {
    while Instant::now() < deadline {
        if inspected_ui_grant(control) == expected {
            return;
        }
        thread::sleep(Duration::from_millis(100));
    }
    panic!(
        "timed out waiting for the human to {} ui:inject in the native F8 panel",
        if expected { "grant" } else { "revoke" }
    );
}

fn await_toolbar(
    messages: &mpsc::Receiver<blueice_ipc::ServerMessage>,
    expected: Option<&str>,
    deadline: Instant,
) {
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let message = messages
            .recv_timeout(remaining)
            .expect("the shared core did not publish the expected native toolbar state");
        if let blueice_ipc::ServerMessage::ExtensionToolbar { label } = message {
            if label.as_deref() == expected {
                return;
            }
        }
    }
    panic!("timed out waiting for the native toolbar state {expected:?}");
}

/// Run ONLY with a person at the graphical desktop:
///
/// cargo test -p blueice-launcher --test trusted_native_window \
///   manual_native_grant_and_revoke_retire_published_toolbar -- --ignored --nocapture
///
/// This test never sends Grant/Revoke itself. The only authority is the
/// launcher-owned F8 panel operated by the person; this process uses the
/// operator socket solely for read-only inspection and an ordinary frontend
/// connection solely to trigger a navigation and observe publication.
#[test]
#[ignore = "requires a person to use the native F8 permission panel"]
fn manual_native_grant_and_revoke_retire_published_toolbar() {
    let launcher_bin = PathBuf::from(env!("CARGO_BIN_EXE_blueice-launcher"));
    for sibling in [
        "blueice-frontend",
        "blueice-extension-host",
        "blueice-core",
        "blueice-ai-gatekeeper",
        "bluejs",
    ] {
        assert!(
            launcher_bin.with_file_name(sibling).exists(),
            "build {sibling} beside the launcher first"
        );
    }
    let root = std::env::temp_dir().join(format!(
        "btw-manual-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    let root = TestRoot(root);
    let rendezvous = root.0.join("core.sock");
    let control = root.0.join("control.sock");
    let frames = root.0.join("frames");
    let manifest = root.0.join("extension.json");
    std::fs::write(&manifest,
        r#"{"name":"Manual consent proof","version":"1","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"optional":["ui:inject"]}}"#,
    ).unwrap();
    std::fs::write(
        root.0.join("extension.wasm"),
        wat::parse_str(MANUAL_UI_WAT).unwrap(),
    )
    .unwrap();
    let launcher = Command::new(&launcher_bin)
        .args([
            "--socket",
            rendezvous.to_str().unwrap(),
            "--control-socket",
            control.to_str().unwrap(),
            "--frame-dir",
            frames.to_str().unwrap(),
            "--extension-manifest",
            manifest.to_str().unwrap(),
            "--trusted-frontend",
        ])
        .spawn()
        .expect("failed to spawn the graphical trusted-window launcher");
    let mut launcher = TestLauncher(launcher, launcher_bin.with_file_name("blueice-frontend"));
    let ready_deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < ready_deadline {
        assert!(
            launcher.0.try_wait().unwrap().is_none(),
            "launcher exited before its trusted window was ready"
        );
        if rendezvous.exists() && control.exists() {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    assert!(rendezvous.exists() && control.exists());
    thread::sleep(Duration::from_secs(11));
    assert!(
        launcher.0.try_wait().unwrap().is_none(),
        "native child did not pass the private Inspect deadline"
    );
    assert!(
        !inspected_ui_grant(&control),
        "optional UI must start denied"
    );

    let mut client = UnixStream::connect(&rendezvous).unwrap();
    blueice_ipc::client_handshake(&mut client).unwrap();
    let mut reader = client.try_clone().unwrap();
    let (message_tx, message_rx) = mpsc::channel();
    thread::spawn(move || {
        while let Ok(message) = blueice_ipc::read_server_message(&mut reader) {
            if message_tx.send(message).is_err() {
                break;
            }
        }
    });

    eprintln!("In the launcher-owned window: press F8, select ui:inject, Review, then Confirm GRANT (60 seconds).");
    await_ui_grant(&control, true, Instant::now() + Duration::from_secs(60));

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/manual-consent", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(20));
                }
                Err(error) => panic!("consent proof did not fetch its local fixture: {error}"),
            }
        };
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 15\r\nConnection: close\r\n\r\n<p>Consent</p>\n").unwrap();
    });
    blueice_ipc::write_client_message(&mut client, &blueice_ipc::ClientMessage::Navigate { url })
        .unwrap();
    await_toolbar(
        &message_rx,
        Some("Consent Probe"),
        Instant::now() + Duration::from_secs(15),
    );
    server.join().unwrap();

    eprintln!("The toolbar is published. In the same native window: press F8, Review, then Confirm REVOKE (60 seconds).");
    await_ui_grant(&control, false, Instant::now() + Duration::from_secs(60));
    await_toolbar(&message_rx, None, Instant::now() + Duration::from_secs(5));

    let frontend_bin = launcher_bin.with_file_name("blueice-frontend");
    let mut processes = sysinfo::System::new_all();
    processes.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let frontend = processes
        .processes()
        .values()
        .find(|process| {
            process.parent() == Some(sysinfo::Pid::from_u32(launcher.0.id()))
                && process.exe() == Some(frontend_bin.as_path())
        })
        .expect("the live launcher must own the exact sibling frontend child");
    assert_eq!(frontend.kill_with(sysinfo::Signal::Kill), Some(true));
    let exit_deadline = Instant::now() + Duration::from_secs(5);
    while launcher.0.try_wait().unwrap().is_none() && Instant::now() < exit_deadline {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        launcher.0.try_wait().unwrap().is_some(),
        "the broker must stop when its native authority is lost"
    );
    assert!(!rendezvous.exists());
    assert!(!control.exists());
    assert!(!frames.exists());
}

/// Run only with a person at the graphical desktop. The test process never
/// writes the private trusted-window pipe or calls `ArmEphemeral`: a real
/// F8-panel pointer review and distinct confirm must drive the one-shot read.
///
/// First rebuild all sibling binaries; `cargo test` alone does not refresh
/// `target/debug/blueice-frontend`, and a stale window has no one-shot button:
///   cargo build -p blueice-launcher -p blueice-frontend-reference \
///     -p blueice-engine -p blueice-ai-gatekeeper \
///     -p blueice-extension-host -p blueice-bluejs --bins
/// Then, from `backend/`:
/// cargo test -p blueice-launcher --test trusted_native_window \
///   manual_native_one_shot_dom_read_reaches_installed_wasm -- --ignored --nocapture
#[test]
#[ignore = "requires a person to review and confirm the native one-shot DOM read"]
fn manual_native_one_shot_dom_read_reaches_installed_wasm() {
    let launcher_bin = PathBuf::from(env!("CARGO_BIN_EXE_blueice-launcher"));
    for sibling in [
        "blueice-frontend", "blueice-extension-host", "blueice-core",
        "blueice-ai-gatekeeper", "bluejs",
    ] {
        assert!(launcher_bin.with_file_name(sibling).exists(),
            "build {sibling} beside the launcher first");
    }
    let root = std::env::temp_dir().join(format!(
        "btw-e-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .unwrap().as_nanos(),
    ));
    std::fs::create_dir(&root).unwrap();
    let root = TestRoot(root);
    let rendezvous = root.0.join("core.sock");
    let control = root.0.join("control.sock");
    let frames = root.0.join("frames");
    let manifest = root.0.join("extension.json");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let url = format!("{origin}/one-shot-proof");
    std::fs::write(&manifest, format!(
        r#"{{"name":"Native one-shot proof","version":"1","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{{"declared":["ui:inject"],"runtime_ephemeral":["dom:read"]}},"capability_origins":{{"dom:read":["{origin}"]}}}}"#,
    )).unwrap();
    std::fs::write(root.0.join("extension.wasm"),
        wat::parse_str(MANUAL_EPHEMERAL_WAT).unwrap()).unwrap();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
                    && Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
                Err(error) => panic!("one-shot fixture was not fetched: {error}"),
            }
        };
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        let body = b"<h1>One-shot proof</h1>";
        stream.write_all(format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len(),
        ).as_bytes()).unwrap();
        stream.write_all(body).unwrap();
    });
    let launcher = Command::new(&launcher_bin)
        .args([
            "--socket", rendezvous.to_str().unwrap(),
            "--control-socket", control.to_str().unwrap(),
            "--frame-dir", frames.to_str().unwrap(),
            "--extension-manifest", manifest.to_str().unwrap(),
            "--trusted-frontend",
        ])
        .spawn()
        .expect("failed to spawn the graphical trusted-window launcher");
    let mut launcher = TestLauncher(launcher, launcher_bin.with_file_name("blueice-frontend"));
    wait_for_path(&rendezvous, Instant::now() + Duration::from_secs(15));
    wait_for_path(&control, Instant::now() + Duration::from_secs(15));
    thread::sleep(Duration::from_secs(11));
    assert!(launcher.0.try_wait().unwrap().is_none(),
        "the native window did not pass its private inspection deadline");
    let mut inspector = UnixStream::connect(&control).unwrap();
    write_control_request(&mut inspector, &ControlRequest::InspectExtensionPermissions).unwrap();
    assert!(matches!(read_control_reply(&mut inspector).unwrap(),
        ControlReply::ExtensionPermissions { installed: Some(ref package), .. }
            if package.optional.is_empty()
                && package.runtime_ephemeral.len() == 1
                && package.runtime_ephemeral[0].capability == "dom:read"
                && package.runtime_ephemeral[0].origins == [origin.clone()]));

    let mut client = UnixStream::connect(&rendezvous).unwrap();
    blueice_ipc::client_handshake(&mut client).unwrap();
    let mut reader = client.try_clone().unwrap();
    let (message_tx, message_rx) = mpsc::channel();
    thread::spawn(move || {
        while let Ok(message) = blueice_ipc::read_server_message(&mut reader) {
            if message_tx.send(message).is_err() { break; }
        }
    });
    blueice_ipc::write_client_message(&mut client, &blueice_ipc::ClientMessage::GetExtensionToolbar).unwrap();
    await_toolbar(&message_rx, Some("Read Ready"), Instant::now() + Duration::from_secs(5));
    blueice_ipc::write_client_message(&mut client,
        &blueice_ipc::ClientMessage::Navigate { url: url.clone() }).unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut navigated = false;
    let mut frame_ready = false;
    while !(navigated && frame_ready) && Instant::now() < deadline {
        let message = message_rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("the selected tab did not render the one-shot fixture");
        match message {
            blueice_ipc::ServerMessage::Navigated { url: observed } if observed == url => navigated = true,
            blueice_ipc::ServerMessage::FrameReady { .. } => frame_ready = true,
            _ => {}
        }
    }
    assert!(navigated && frame_ready);
    server.join().unwrap();
    eprintln!("In the launcher-owned window, press F8; click REVIEW ONE-SHOT DOM READ for the selected fixture tab, inspect the URL/scope, then click CONFIRM ONE READ (90 seconds).");
    await_toolbar(&message_rx, Some("Read Succeeded"), Instant::now() + Duration::from_secs(90));
    eprintln!("The installed WASM read the exact document once and its second read was denied.");
    blueice_ipc::write_client_message(&mut client, &blueice_ipc::ClientMessage::Shutdown).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while launcher.0.try_wait().unwrap().is_none() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(launcher.0.try_wait().unwrap().is_some());
    assert!(!rendezvous.exists() && !control.exists() && !frames.exists());
}
