// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Exercises the actual compiled `blueice-core` binary as a real
//! subprocess -- `main`'s own argument parsing, socket binding,
//! accept, and shutdown/cleanup wiring, none of which
//! `src/bin/blueice-core.rs`'s unit tests touch (those cover
//! `parse_args` in isolation; `blueice_engine::session`'s own tests
//! cover the message loop over an in-process pipe). This is the
//! "e2e test through the real public interface" the project's
//! Definition of Done asks for applied to a binary rather than a
//! library: `Command::new(env!("CARGO_BIN_EXE_blueice-core"))`, not a
//! function call, is the real public interface here. Manually running
//! this exact binary under a real windowed frontend (WSLg) additionally
//! confirmed the end-to-end pixels look right; this test covers the
//! process-lifecycle contract repeatably in CI, which a manual run
//! can't.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

fn unique_socket_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "blueice-core-binary-test-{label}-{}.sock",
        std::process::id()
    ))
}

/// Extension listeners are intentionally bound only below a directory the
/// current user owns. Keep these test paths short for Darwin's Unix-domain
/// socket limit while exercising that same production invariant.
fn unique_private_extension_socket_path(label: &str) -> PathBuf {
    blueice_ipc::local_socket::default_socket_dir()
        .join(format!("core-ext-{label}-{}.sock", std::process::id()))
}

/// A gated `Navigate`/`OpenTab{url}` sent to the real subprocess needs
/// *some* `ai-gatekeeper` behind the `--gatekeeper-socket` path it's
/// given -- a genuinely unreachable one fails closed
/// (`phase-7-local-ai/PLAN.md`'s "Wiring design"), which would turn
/// this file's pre-existing "navigation always succeeds" assertions
/// false. Spins up a real listener running the actual minimal-slice
/// stub logic (`blueice_ai_gatekeeper::handle_one_check`, always
/// clears) bound to a fresh path unique to this call, mirroring
/// `blueice_engine::session`'s own test-module helper of the same
/// name/purpose.
fn clearing_gatekeeper(label: &str) -> PathBuf {
    let path = unique_socket_path(label);
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).unwrap();
    thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else { break };
            let _ = blueice_ai_gatekeeper::handle_one_check(&mut stream);
        }
    });
    path
}

fn wait_for(path: &std::path::Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    false
}

/// These tests start real core processes with short readiness deadlines.
/// Launching every case at once can saturate a development machine and turn
/// an otherwise healthy core startup into a spurious timeout. The guard is
/// process-local: a probe child running this test binary has its own lock.
fn core_process_test_guard() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn sibling_bluejs_binary() -> PathBuf {
    let core = PathBuf::from(env!("CARGO_BIN_EXE_blueice-core"));
    core.parent().unwrap().join("bluejs")
}

fn core_extension_host_probe_script(root: &Path, test_name: &str) -> PathBuf {
    fn shell_quote(value: &str) -> String {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    }

    let script = root.join("extension-host-probe.sh");
    let test_binary = std::env::current_exe().unwrap();
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nBLUEICE_TEST_EXTENSION_SOCKET=\"$2\"\nBLUEICE_TEST_EXTENSION_MANIFEST=\"$4\"\nexport BLUEICE_TEST_EXTENSION_SOCKET BLUEICE_TEST_EXTENSION_MANIFEST\nexec {} --exact {} --nocapture\n",
            shell_quote(test_binary.to_str().unwrap()),
            shell_quote(test_name)
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    script
}

/// `blueice-core` owns the child lifecycle and invokes a host executable with
/// `--connect`/`--manifest`. This test-only process is deliberately tiny: it
/// verifies that public invocation contract and its runtime-start sequencing
/// without relying on a sibling package's already-built binary. The real
/// `blueice-extension-host --connect` WASM path is tested in that package's
/// own binary integration test.
#[test]
fn extension_host_probe_child_authenticates_to_core() {
    let _guard = core_process_test_guard();
    let Ok(socket) = std::env::var("BLUEICE_TEST_EXTENSION_SOCKET") else {
        return;
    };
    let manifest = std::env::var("BLUEICE_TEST_EXTENSION_MANIFEST").unwrap();
    let authentication = std::env::var("BLUEICE_EXTENSION_AUTH_TOKEN").unwrap();
    let installed = blueice_extension_host::load_installed_extension(&manifest).unwrap();
    let capability_versions = installed
        .manifest()
        .capabilities()
        .declared()
        .iter()
        .map(|capability| (capability.clone(), 1))
        .collect();
    let mut stream = UnixStream::connect(socket).unwrap();
    blueice_ipc::extension::write_extension_request(
        &mut stream,
        &blueice_ipc::extension::ExtensionRequest::HelloAuthenticated {
            extension_id: installed.extension_id().to_string(),
            capability_versions,
            authentication,
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::extension::read_extension_reply(&mut stream).unwrap(),
        blueice_ipc::extension::ExtensionReply::HelloAck { .. }
    ));
    blueice_ipc::extension::write_extension_request(
        &mut stream,
        &blueice_ipc::extension::ExtensionRequest::RuntimeReady,
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::extension::read_extension_reply(&mut stream).unwrap(),
        blueice_ipc::extension::ExtensionReply::RuntimeStart
    );
    blueice_ipc::extension::write_extension_request(
        &mut stream,
        &blueice_ipc::extension::ExtensionRequest::NextRuntimeEvent,
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::extension::read_extension_reply(&mut stream).unwrap(),
        blueice_ipc::extension::ExtensionReply::RuntimeEventStreamClosed
    );
}

/// This test-only host is the peer of the compiled-core parent-pipe test. It
/// authenticates exactly like the installed child but has a separate private
/// test socket for asking it to attempt optional storage and one-shot DOM
/// reads without giving the extension any permission-control authority.
#[test]
fn extension_host_probe_child_observes_optional_and_ephemeral_grants() {
    let _guard = core_process_test_guard();
    let Ok(probe_socket) = std::env::var("BLUEICE_TEST_PROBE_SOCKET") else {
        return;
    };
    use blueice_ipc::extension::{
        read_extension_reply, write_extension_request, ExtensionReply, ExtensionRequest,
    };
    use std::collections::BTreeMap;

    let socket = std::env::var("BLUEICE_TEST_EXTENSION_SOCKET").unwrap();
    let manifest = std::env::var("BLUEICE_TEST_EXTENSION_MANIFEST").unwrap();
    let authentication = std::env::var("BLUEICE_EXTENSION_AUTH_TOKEN").unwrap();
    let installed = blueice_extension_host::load_installed_extension(manifest).unwrap();
    let mut extension = UnixStream::connect(socket).unwrap();
    write_extension_request(&mut extension, &ExtensionRequest::HelloAuthenticated {
        extension_id: installed.extension_id().to_string(),
        capability_versions: BTreeMap::from([
            ("storage".to_string(), 1), ("dom:read".to_string(), 3),
        ]),
        authentication,
    }).unwrap();
    assert!(matches!(read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::HelloAck { unsupported_capabilities } if unsupported_capabilities.is_empty()));
    write_extension_request(&mut extension, &ExtensionRequest::RuntimeReady).unwrap();
    assert_eq!(read_extension_reply(&mut extension).unwrap(), ExtensionReply::RuntimeStart);

    let mut probe = UnixStream::connect(probe_socket).unwrap();
    loop {
        let mut command = [0_u8; 1];
        if probe.read_exact(&mut command).is_err() || command[0] == b'q' {
            break;
        }
        let result = match command[0] {
            b'e' => {
                let mut trusted_ticket = None;
                for _ in 0..4 {
                    write_extension_request(&mut extension, &ExtensionRequest::NextRuntimeEvent).unwrap();
                    match read_extension_reply(&mut extension).unwrap() {
                        ExtensionReply::RuntimeEvent(
                            blueice_ipc::extension::ExtensionRuntimeEvent::TrustedEphemeralDomRead {
                                tab_id: 1, document_epoch: 2, ticket,
                            }
                        ) => {
                            trusted_ticket = Some(ticket);
                            break;
                        }
                        ExtensionReply::RuntimeEvent(
                            blueice_ipc::extension::ExtensionRuntimeEvent::NavigationCommitted { tab_id: 1 }
                        ) => {}
                        other => panic!("unexpected event while waiting for a trusted DOM read: {other:?}"),
                    }
                }
                let ticket = trusted_ticket.expect("core must deliver its parent-armed event");
                assert_eq!(ticket.len(), 64);
                probe.write_all(b"E").unwrap();
                probe.write_all(ticket.as_bytes()).unwrap();
                continue;
            }
            b'g' => {
                write_extension_request(&mut extension, &ExtensionRequest::StorageGet { key: "sample".into() }).unwrap();
                match read_extension_reply(&mut extension).unwrap() {
                    ExtensionReply::StorageGetResult { value: None } => b'A',
                    ExtensionReply::CapabilityDenied { capability, .. } if capability == "storage" => b'D',
                    other => panic!("unexpected optional storage result: {other:?}"),
                }
            }
            b'r' | b's' | b'v' | b'w' => {
                let request = match command[0] {
                    b'r' | b's' => {
                        let mut bytes = [0_u8; 64];
                        probe.read_exact(&mut bytes).unwrap();
                        ExtensionRequest::DomReadTabEphemeral {
                            tab_id: if command[0] == b'r' { 1 } else { 2 },
                            ticket: String::from_utf8(bytes.to_vec()).unwrap(),
                        }
                    }
                    b'v' => ExtensionRequest::DomReadTab { tab_id: 1 },
                    _ => ExtensionRequest::DomRead,
                };
                write_extension_request(&mut extension, &request).unwrap();
                match read_extension_reply(&mut extension).unwrap() {
                    ExtensionReply::DomReadResult { value } => {
                        let snapshot: blueice_ipc::AiSnapshot = serde_json::from_str(&value).unwrap();
                        assert_eq!(snapshot.tab_id, 1);
                        b'A'
                    }
                    ExtensionReply::CapabilityDenied { capability, .. } if capability == "dom:read" => b'D',
                    other => panic!("unexpected ephemeral DOM read result: {other:?}"),
                }
            }
            other => panic!("unexpected probe command: {other}"),
        };
        probe.write_all(&[result]).unwrap();
    }
}

/// Authenticated child for the real-process published-effect revocation test.
/// Only the parent test's private probe socket tells it when to attempt the
/// two optional actions; the extension cannot grant itself either capability.
#[test]
fn extension_host_probe_child_publishes_optional_effects() {
    let _guard = core_process_test_guard();
    let Ok(probe_socket) = std::env::var("BLUEICE_TEST_PROBE_SOCKET") else {
        return;
    };
    use blueice_ipc::extension::{
        read_extension_reply, write_extension_request, ExtensionReply, ExtensionRequest,
    };
    use std::collections::BTreeMap;

    let socket = std::env::var("BLUEICE_TEST_EXTENSION_SOCKET").unwrap();
    let manifest = std::env::var("BLUEICE_TEST_EXTENSION_MANIFEST").unwrap();
    let authentication = std::env::var("BLUEICE_EXTENSION_AUTH_TOKEN").unwrap();
    let rule_url = std::env::var("BLUEICE_TEST_RULE_URL").unwrap();
    let installed = blueice_extension_host::load_installed_extension(manifest).unwrap();
    let mut extension = UnixStream::connect(socket).unwrap();
    write_extension_request(&mut extension, &ExtensionRequest::HelloAuthenticated {
        extension_id: installed.extension_id().to_string(),
        capability_versions: BTreeMap::from([
            ("ui:inject".to_string(), 3),
            ("network:intercept".to_string(), 2),
        ]),
        authentication,
    }).unwrap();
    assert!(matches!(read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::HelloAck { unsupported_capabilities } if unsupported_capabilities.is_empty()));
    write_extension_request(&mut extension, &ExtensionRequest::RuntimeReady).unwrap();
    assert_eq!(read_extension_reply(&mut extension).unwrap(), ExtensionReply::RuntimeStart);

    let mut probe = UnixStream::connect(probe_socket).unwrap();
    probe.write_all(b"S").unwrap();
    loop {
        let mut command = [0_u8; 1];
        if probe.read_exact(&mut command).is_err() || command[0] == b'q' {
            break;
        }
        assert!(matches!(command[0], b'p' | b'd'));
        let published = command[0] == b'p';
        for (request, capability, allowed_reply) in [
            (
                ExtensionRequest::SetToolbarButton { label: "Optional Notes".into() },
                "ui:inject",
                ExtensionReply::UiInjectAck,
            ),
            (
                ExtensionRequest::RegisterNetworkBlockUrl { url: rule_url.clone() },
                "network:intercept",
                ExtensionReply::NetworkInterceptAck,
            ),
        ] {
            write_extension_request(&mut extension, &request).unwrap();
            let reply = read_extension_reply(&mut extension).unwrap();
            if published {
                assert_eq!(reply, allowed_reply, "a granted optional action must publish");
            } else {
                assert!(matches!(reply, ExtensionReply::CapabilityDenied { capability: actual, .. } if actual == capability),
                    "an ungranted optional action must be denied as {capability}");
            }
        }
        probe.write_all(if published { b"P" } else { b"D" }).unwrap();
    }
}

#[test]
fn extension_host_probe_child_publishes_toolbar_and_handles_activation() {
    let _guard = core_process_test_guard();
    use blueice_ipc::extension::{
        read_extension_reply, write_extension_request, ExtensionReply, ExtensionRequest,
        ExtensionRuntimeEvent,
    };
    use std::collections::BTreeMap;

    let Ok(socket) = std::env::var("BLUEICE_TEST_EXTENSION_SOCKET") else {
        return;
    };
    let manifest = std::env::var("BLUEICE_TEST_EXTENSION_MANIFEST").unwrap();
    let authentication = std::env::var("BLUEICE_EXTENSION_AUTH_TOKEN").unwrap();
    let installed = blueice_extension_host::load_installed_extension(manifest).unwrap();
    let mut stream = UnixStream::connect(socket).unwrap();
    write_extension_request(
        &mut stream,
        &ExtensionRequest::HelloAuthenticated {
            extension_id: installed.extension_id().to_string(),
            capability_versions: BTreeMap::from([("ui:inject".to_string(), 3)]),
            authentication,
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut stream).unwrap(),
        ExtensionReply::HelloAck {
            unsupported_capabilities: BTreeMap::new(),
        }
    );
    write_extension_request(&mut stream, &ExtensionRequest::RuntimeReady).unwrap();
    assert_eq!(read_extension_reply(&mut stream).unwrap(), ExtensionReply::RuntimeStart);
    write_extension_request(
        &mut stream,
        &ExtensionRequest::SetToolbarButton {
            label: "Enter password".to_string(),
        },
    )
    .unwrap();
    assert!(matches!(
        read_extension_reply(&mut stream).unwrap(),
        ExtensionReply::GatekeeperBlocked { category, .. }
            if category == "extension-toolbar-social-engineering"
    ));
    write_extension_request(
        &mut stream,
        &ExtensionRequest::SetToolbarButton {
            label: "Notes".to_string(),
        },
    )
    .unwrap();
    assert_eq!(read_extension_reply(&mut stream).unwrap(), ExtensionReply::UiInjectAck);
    write_extension_request(&mut stream, &ExtensionRequest::NextRuntimeEvent).unwrap();
    assert_eq!(
        read_extension_reply(&mut stream).unwrap(),
        ExtensionReply::RuntimeEvent(ExtensionRuntimeEvent::ToolbarActivated { tab_id: 1, grant_generation: 0 })
    );
    write_extension_request(
        &mut stream,
        &ExtensionRequest::SetToolbarButton {
            label: "Clicked".to_string(),
        },
    )
    .unwrap();
    assert_eq!(read_extension_reply(&mut stream).unwrap(), ExtensionReply::UiInjectAck);
    write_extension_request(
        &mut stream,
        &ExtensionRequest::ShowPopup {
            tab_id: 1,
            title: "Notes".to_string(),
            body: "Saved locally".to_string(),
        },
    )
    .unwrap();
    assert_eq!(read_extension_reply(&mut stream).unwrap(), ExtensionReply::UiInjectAck);
    write_extension_request(&mut stream, &ExtensionRequest::ClearPopup).unwrap();
    assert_eq!(read_extension_reply(&mut stream).unwrap(), ExtensionReply::UiInjectAck);
    write_extension_request(
        &mut stream,
        &ExtensionRequest::ShowPopupAction {
            tab_id: 1,
            title: "Notes".to_string(),
            body: "Ready to open".to_string(),
            action_label: "Open notes".to_string(),
        },
    ).unwrap();
    assert_eq!(read_extension_reply(&mut stream).unwrap(), ExtensionReply::UiInjectAck);
    write_extension_request(&mut stream, &ExtensionRequest::NextRuntimeEvent).unwrap();
    assert_eq!(
        read_extension_reply(&mut stream).unwrap(),
        ExtensionReply::RuntimeEvent(ExtensionRuntimeEvent::PopupActionActivated { tab_id: 1, grant_generation: 0 })
    );
    write_extension_request(&mut stream, &ExtensionRequest::SetToolbarButton {
        label: "Actioned".to_string(),
    }).unwrap();
    assert_eq!(read_extension_reply(&mut stream).unwrap(), ExtensionReply::UiInjectAck);
    write_extension_request(&mut stream, &ExtensionRequest::ClearToolbarButton).unwrap();
    assert_eq!(read_extension_reply(&mut stream).unwrap(), ExtensionReply::UiInjectAck);
}

fn extension_manifest_package(
    label: &str,
    declared_capabilities: &[&str],
) -> (PathBuf, PathBuf, String) {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let ordinal = NEXT.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "blueice-core-extension-package-{label}-{}-{ordinal}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let manifest = root.join("extension.json");
    let declared_capabilities = declared_capabilities
        .iter()
        .map(|capability| format!("\"{capability}\""))
        .collect::<Vec<_>>()
        .join(",");
    std::fs::write(
        &manifest,
        format!(
            r#"{{"name":"Core bridge test","version":"1.0.0","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{{"declared":[{declared_capabilities}]}}}}"#
        ),
    )
    .unwrap();
    std::fs::write(root.join("extension.wasm"), b"\0asm\x01\0\0\0").unwrap();
    let extension_id = blueice_extension_host::load_installed_extension(&manifest)
        .unwrap()
        .extension_id()
        .to_string();
    (root, manifest, extension_id)
}

#[test]
fn missing_socket_flag_exits_with_failure_and_no_socket_is_created() {
    let _guard = core_process_test_guard();
    let output = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .output()
        .expect("failed to run blueice-core");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--socket"));
}

#[test]
fn an_invalid_installed_extension_never_publishes_core_or_extension_sockets() {
    let _guard = core_process_test_guard();
    let core_socket = unique_socket_path("invalid-extension-core");
    let extension_socket = unique_socket_path("invalid-extension-protocol");
    let root = std::env::temp_dir().join(format!(
        "blueice-core-invalid-extension-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let manifest = root.join("extension.json");
    std::fs::write(&manifest, "{not valid JSON").unwrap();
    let _ = std::fs::remove_file(&core_socket);
    let _ = std::fs::remove_file(&extension_socket);

    let output = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            core_socket.to_str().unwrap(),
            "--extension-socket",
            extension_socket.to_str().unwrap(),
            "--extension-manifest",
            manifest.to_str().unwrap(),
        ])
        .output()
        .expect("failed to run core with a bad installed extension");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("could not install extension"));
    assert!(!core_socket.exists());
    assert!(!extension_socket.exists());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn real_subprocess_serves_navigate_resize_and_shutdown_over_a_real_socket() {
    let _guard = core_process_test_guard();
    let socket_path = unique_socket_path("full-session");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-binary-test-frames-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_dir_all(&frame_dir);
    let gatekeeper_path = clearing_gatekeeper("fs-gk"); // short: Unix socket paths are capped at ~100 bytes total

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let body = "<p>from a real subprocess</p>";
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
    });

    let mut child = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            socket_path.to_str().unwrap(),
            "--width",
            "300",
            "--height",
            "150",
            "--frame-dir",
            frame_dir.to_str().unwrap(),
            "--gatekeeper-socket",
            gatekeeper_path.to_str().unwrap(),
        ])
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn blueice-core");

    assert!(
        wait_for(&socket_path, Duration::from_secs(5)),
        "blueice-core never created its socket"
    );
    let mut stream =
        UnixStream::connect(&socket_path).expect("failed to connect to the real subprocess");
    blueice_ipc::client_handshake(&mut stream)
        .expect("the real subprocess must complete the protocol_version handshake");

    let url = format!("http://{addr}");
    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::Navigate { url: url.clone() },
    )
    .unwrap();
    let navigated = blueice_ipc::read_server_message(&mut stream).unwrap();
    assert_eq!(navigated, blueice_ipc::ServerMessage::Navigated { url });

    let frame = blueice_ipc::read_server_message(&mut stream).unwrap();
    let (shm_path, width, height) = match frame {
        blueice_ipc::ServerMessage::FrameReady {
            shm_path,
            width,
            height,
            generation: 1,
        } => (shm_path, width, height),
        other => panic!("expected the first FrameReady, got {other:?}"),
    };
    assert_eq!((width, height), (300, 150));
    let mapped = blueice_ipc::shm::map_frame(std::path::Path::new(&shm_path))
        .expect("the real subprocess's frame file must be mappable");
    assert_eq!(
        mapped.len() as u32,
        width * height * 4,
        "RGBA8 frame bytes must match the requested viewport size"
    );

    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::Resize {
            width: 100,
            height: 80,
        },
    )
    .unwrap();
    let resized = blueice_ipc::read_server_message(&mut stream).unwrap();
    assert!(matches!(
        resized,
        blueice_ipc::ServerMessage::FrameReady {
            width: 100,
            height: 80,
            generation: 2,
            ..
        }
    ));

    blueice_ipc::write_client_message(&mut stream, &blueice_ipc::ClientMessage::Shutdown).unwrap();
    let status = child
        .wait()
        .expect("failed to wait for blueice-core to exit");
    assert!(
        status.success(),
        "blueice-core must exit cleanly after Shutdown"
    );

    assert!(
        !socket_path.exists(),
        "blueice-core must remove its own socket file on exit"
    );
    assert!(
        !frame_dir.exists(),
        "blueice-core must remove its own frame directory on exit"
    );
}

#[test]
fn installed_extension_reads_a_real_core_owned_representation_over_private_sockets() {
    let _guard = core_process_test_guard();
    use blueice_ipc::extension::{
        read_extension_reply, write_extension_request, ExtensionReply, ExtensionRequest,
    };
    use std::collections::BTreeMap;

    let core_socket = unique_socket_path("extension-core");
    let extension_socket = unique_private_extension_socket_path("read");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-extension-frames-{}",
        std::process::id()
    ));
    let (package_root, manifest, extension_id) =
        extension_manifest_package("real-read", &["dom:read"]);
    let _ = std::fs::remove_file(&core_socket);
    let _ = std::fs::remove_file(&extension_socket);
    let _ = std::fs::remove_dir_all(&frame_dir);

    let mut core = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            core_socket.to_str().unwrap(),
            "--extension-socket",
            extension_socket.to_str().unwrap(),
            "--extension-manifest",
            manifest.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
        ])
        .spawn()
        .expect("failed to spawn core with an installed extension");

    assert!(wait_for(&core_socket, Duration::from_secs(5)));
    assert!(wait_for(&extension_socket, Duration::from_secs(5)));
    let mut frontend = UnixStream::connect(&core_socket).unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();
    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::Navigate {
            url: "about:credits".to_string(),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::Navigated { .. }
    ));
    assert!(matches!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));

    let mut extension = UnixStream::connect(&extension_socket).unwrap();
    write_extension_request(
        &mut extension,
        &ExtensionRequest::Hello {
            extension_id,
            capability_versions: BTreeMap::from([("dom:read".to_string(), 1)]),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::HelloAck {
            unsupported_capabilities: BTreeMap::new(),
        }
    );
    write_extension_request(&mut extension, &ExtensionRequest::DomRead).unwrap();
    let snapshot = match read_extension_reply(&mut extension).unwrap() {
        ExtensionReply::DomReadResult { value } => {
            serde_json::from_str::<blueice_ipc::AiSnapshot>(&value).unwrap()
        }
        other => panic!("expected a core-backed DomReadResult, got {other:?}"),
    };
    assert_eq!(snapshot.tab_id, 1);
    assert_eq!(snapshot.url.as_deref(), Some("about:credits"));
    assert!(
        !snapshot.nodes.is_empty(),
        "the extension must receive the navigated core page, not an empty initial snapshot"
    );

    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown)
        .unwrap();
    assert!(core.wait().unwrap().success());
    assert!(!core_socket.exists());
    assert!(!extension_socket.exists());
    assert!(!frame_dir.exists());
    let _ = std::fs::remove_dir_all(package_root);
}

#[test]
fn installed_extension_origin_scopes_follow_the_live_page_across_navigation() {
    let _guard = core_process_test_guard();
    use blueice_ipc::extension::{
        read_extension_reply, write_extension_request, ExtensionReply, ExtensionRequest,
    };
    use std::collections::BTreeMap;

    let core_socket = unique_socket_path("eosc");
    let extension_socket = unique_private_extension_socket_path("eosc");
    let gatekeeper_socket = clearing_gatekeeper("eosg");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-extension-origin-frames-{}",
        std::process::id()
    ));
    let (package_root, manifest, _) = extension_manifest_package(
        "origin-scope",
        &["dom:read", "dom:write", "network:observe"],
    );
    let allowed_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let allowed_addr = allowed_listener.local_addr().unwrap();
    let blocked_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let blocked_addr = blocked_listener.local_addr().unwrap();
    std::fs::write(
        &manifest,
        format!(
            r#"{{"name":"Core bridge test","version":"1.0.0","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{{"declared":["dom:read","dom:write","network:observe"]}},"capability_origins":{{"dom:read":["http://{allowed_addr}"],"dom:write":["http://{allowed_addr}"],"network:observe":["http://{allowed_addr}"]}}}}"#
        ),
    )
    .unwrap();
    let extension_id = blueice_extension_host::load_installed_extension(&manifest)
        .unwrap()
        .extension_id()
        .to_string();
    for (listener, body) in [
        (allowed_listener, "<h1 id=\"title\">Allowed</h1>"),
        (blocked_listener, "<h1 id=\"title\">Blocked</h1>"),
    ] {
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request);
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .unwrap();
        });
    }
    let _ = std::fs::remove_file(&core_socket);
    let _ = std::fs::remove_file(&extension_socket);
    let _ = std::fs::remove_dir_all(&frame_dir);
    let mut core = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            core_socket.to_str().unwrap(),
            "--extension-socket",
            extension_socket.to_str().unwrap(),
            "--extension-manifest",
            manifest.to_str().unwrap(),
            "--gatekeeper-socket",
            gatekeeper_socket.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
        ])
        .spawn()
        .expect("failed to spawn core with an origin-scoped extension");
    assert!(wait_for(&core_socket, Duration::from_secs(5)));
    assert!(wait_for(&extension_socket, Duration::from_secs(5)));
    let mut frontend = UnixStream::connect(&core_socket).unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();
    let mut extension = UnixStream::connect(&extension_socket).unwrap();
    write_extension_request(
        &mut extension,
        &ExtensionRequest::Hello {
            extension_id,
            capability_versions: BTreeMap::from([
                ("dom:read".to_string(), 2),
                ("dom:write".to_string(), 9),
                ("network:observe".to_string(), 2),
            ]),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::HelloAck {
            unsupported_capabilities: BTreeMap::new(),
        }
    );

    let navigate_and_heading = |frontend: &mut UnixStream, url: String| {
        blueice_ipc::write_client_message(frontend, &blueice_ipc::ClientMessage::Navigate { url })
            .unwrap();
        assert!(matches!(
            blueice_ipc::read_server_message(frontend).unwrap(),
            blueice_ipc::ServerMessage::Navigated { .. }
        ));
        assert!(matches!(
            blueice_ipc::read_server_message(frontend).unwrap(),
            blueice_ipc::ServerMessage::FrameReady { .. }
        ));
        blueice_ipc::write_client_message(frontend, &blueice_ipc::ClientMessage::GetRepresentation)
            .unwrap();
        let blueice_ipc::ServerMessage::Representation(snapshot) =
            blueice_ipc::read_server_message(frontend).unwrap()
        else {
            panic!("expected a page representation after navigation")
        };
        snapshot
            .nodes
            .iter()
            .find(|node| matches!(node.role, blueice_ipc::Role::Heading { .. }))
            .expect("the fetched page must expose its heading")
            .id
    };

    let allowed_heading = navigate_and_heading(&mut frontend, format!("http://{allowed_addr}/page"));
    write_extension_request(&mut extension, &ExtensionRequest::DomReadTab { tab_id: 1 }).unwrap();
    assert!(matches!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::DomReadResult { .. }
    ));
    write_extension_request(
        &mut extension,
        &ExtensionRequest::ReadNetworkResponse { tab_id: 1 },
    )
    .unwrap();
    assert!(matches!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::NetworkResponseResult { response: Some(_) }
    ));
    write_extension_request(
        &mut extension,
        &ExtensionRequest::SetVisibleLeafText {
            tab_id: 1,
            node_id: allowed_heading,
            value: "Allowed update".to_string(),
        },
    )
    .unwrap();
    assert_eq!(read_extension_reply(&mut extension).unwrap(), ExtensionReply::DomWriteAck);
    let (_, _, frame) = blueice_ipc::read_server_message_with_ids(&mut frontend).unwrap();
    assert!(matches!(frame, blueice_ipc::ServerMessage::FrameReady { .. }));

    let blocked_heading = navigate_and_heading(&mut frontend, format!("http://{blocked_addr}/page"));
    for (request, capability) in [
        (ExtensionRequest::DomReadTab { tab_id: 1 }, "dom:read"),
        (ExtensionRequest::ReadNetworkResponse { tab_id: 1 }, "network:observe"),
        (ExtensionRequest::ReadNetworkTrace { tab_id: 1 }, "network:observe"),
        (
            ExtensionRequest::SetVisibleLeafText {
                tab_id: 1,
                node_id: blocked_heading,
                value: "Must not change".to_string(),
            },
            "dom:write",
        ),
        (
            ExtensionRequest::SetVisibleTextContent {
                tab_id: 1,
                node_id: blocked_heading,
                value: "Must not change".to_string(),
            },
            "dom:write",
        ),
    ] {
        write_extension_request(&mut extension, &request).unwrap();
        assert!(matches!(
            read_extension_reply(&mut extension).unwrap(),
            ExtensionReply::OperationUnavailable { capability: denied, reason }
                if denied == capability && reason.contains("origin")
        ));
    }
    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::GetRepresentation,
    )
    .unwrap();
    let blueice_ipc::ServerMessage::Representation(snapshot) =
        blueice_ipc::read_server_message(&mut frontend).unwrap()
    else {
        panic!("the rejected write must not publish a frame or change the page")
    };
    assert!(snapshot.nodes.iter().any(|node| {
        matches!(node.role, blueice_ipc::Role::Heading { .. })
            && node.name.as_deref() == Some("Blocked")
    }));

    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown)
        .unwrap();
    assert!(core.wait().unwrap().success());
    assert!(!core_socket.exists());
    assert!(!extension_socket.exists());
    let _ = std::fs::remove_file(gatekeeper_socket);
    let _ = std::fs::remove_dir_all(package_root);
}

#[test]
fn installed_extension_v2_observes_only_a_committed_redirect_trace() {
    let _guard = core_process_test_guard();
    use blueice_ipc::extension::{
        read_extension_reply, write_extension_request, ExtensionReply, ExtensionRequest,
    };
    use std::collections::BTreeMap;

    let core_socket = unique_socket_path("ext-network-trace");
    let extension_socket = unique_private_extension_socket_path("trace");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-extension-trace-frames-{}", std::process::id()
    ));
    let (package_root, manifest, extension_id) =
        extension_manifest_package("network-trace", &["network:observe"]);
    let gatekeeper_socket = clearing_gatekeeper("ent");
    let _ = std::fs::remove_file(&core_socket);
    let _ = std::fs::remove_file(&extension_socket);
    let _ = std::fs::remove_dir_all(&frame_dir);

    let final_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let final_url = format!("http://{}/final", final_listener.local_addr().unwrap());
    let final_server = thread::spawn(move || {
        let (mut stream, _) = final_listener.accept().unwrap();
        let mut request = [0u8; 1024];
        let _ = stream.read(&mut request);
        stream.write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nSet-Cookie: private=never-expose\r\nContent-Length: 12\r\nConnection: close\r\n\r\n<p>final</p>",
        ).unwrap();
    });
    let redirect_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let request_url = format!("http://{}/start", redirect_listener.local_addr().unwrap());
    let redirect_server = thread::spawn({
        let final_url = final_url.clone();
        move || {
            let (mut stream, _) = redirect_listener.accept().unwrap();
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request);
            stream.write_all(format!(
                "HTTP/1.1 302 Found\r\nLocation: {final_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            ).as_bytes()).unwrap();
        }
    });

    let mut core = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket", core_socket.to_str().unwrap(),
            "--extension-socket", extension_socket.to_str().unwrap(),
            "--extension-manifest", manifest.to_str().unwrap(),
            "--gatekeeper-socket", gatekeeper_socket.to_str().unwrap(),
            "--frame-dir", frame_dir.to_str().unwrap(),
        ])
        .spawn()
        .expect("failed to spawn core for committed network trace test");
    assert!(wait_for(&core_socket, Duration::from_secs(5)));
    assert!(wait_for(&extension_socket, Duration::from_secs(5)));
    let mut frontend = UnixStream::connect(&core_socket).unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();
    let mut extension = UnixStream::connect(&extension_socket).unwrap();
    write_extension_request(&mut extension, &ExtensionRequest::Hello {
        extension_id,
        capability_versions: BTreeMap::from([("network:observe".to_string(), 2)]),
    }).unwrap();
    assert_eq!(read_extension_reply(&mut extension).unwrap(), ExtensionReply::HelloAck {
        unsupported_capabilities: BTreeMap::new(),
    });
    write_extension_request(&mut extension, &ExtensionRequest::ReadNetworkTrace { tab_id: 1 }).unwrap();
    assert_eq!(read_extension_reply(&mut extension).unwrap(), ExtensionReply::NetworkTraceResult { trace: None });

    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Navigate {
        url: request_url.clone(),
    }).unwrap();
    assert_eq!(blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::Navigated { url: final_url.clone() });
    assert!(matches!(blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { .. }));
    write_extension_request(&mut extension, &ExtensionRequest::ReadNetworkTrace { tab_id: 1 }).unwrap();
    let ExtensionReply::NetworkTraceResult { trace: Some(trace) } =
        read_extension_reply(&mut extension).unwrap() else {
            panic!("the committed page must expose its reviewed redirect trace")
        };
    assert_eq!(trace.request_url, request_url);
    assert_eq!(trace.redirects, vec![blueice_ipc::extension::NetworkRedirectInfo {
        request_url,
        status: 302,
        target_url: final_url.clone(),
    }]);
    assert_eq!(trace.response.final_url, final_url);
    assert_eq!(trace.response.status, 200);
    assert_eq!(trace.response.content_type.as_deref(), Some("text/html"));
    assert!(!format!("{trace:?}").contains("private=never-expose"));
    write_extension_request(&mut extension, &ExtensionRequest::ReadNetworkResponse { tab_id: 1 }).unwrap();
    assert_eq!(read_extension_reply(&mut extension).unwrap(), ExtensionReply::NetworkResponseResult {
        response: Some(trace.response),
    });

    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown).unwrap();
    assert!(core.wait().unwrap().success());
    redirect_server.join().unwrap();
    final_server.join().unwrap();
    assert!(!core_socket.exists());
    assert!(!extension_socket.exists());
    let _ = std::fs::remove_file(gatekeeper_socket);
    let _ = std::fs::remove_dir_all(package_root);
}

#[test]
fn installed_extension_v3_network_rule_clear_restores_navigation_after_a_redirect_block() {
    let _guard = core_process_test_guard();
    use blueice_ipc::extension::{
        read_extension_reply, write_extension_request, ExtensionReply, ExtensionRequest,
    };
    use std::collections::BTreeMap;
    use std::io::ErrorKind;

    let core_socket = unique_socket_path("ext-network-rule");
    let extension_socket = unique_private_extension_socket_path("network-rule");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-extension-network-rule-frames-{}",
        std::process::id()
    ));
    let (package_root, manifest, extension_id) =
        extension_manifest_package("exact-network-rule", &["network:intercept"]);
    let gatekeeper_socket = clearing_gatekeeper("enrg");
    let _ = std::fs::remove_file(&core_socket);
    let _ = std::fs::remove_file(&extension_socket);
    let _ = std::fs::remove_dir_all(&frame_dir);

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/private", listener.local_addr().unwrap());
    let redirect_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let redirect_url = format!("http://{}/before", redirect_listener.local_addr().unwrap());
    let redirect_server = thread::spawn({
        let url = url.clone();
        move || {
            let (mut stream, _) = redirect_listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 302 Found\r\nLocation: {url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    )
                    .as_bytes(),
                )
                .unwrap();
        }
    });

    let mut core = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            core_socket.to_str().unwrap(),
            "--extension-socket",
            extension_socket.to_str().unwrap(),
            "--extension-manifest",
            manifest.to_str().unwrap(),
            "--gatekeeper-socket",
            gatekeeper_socket.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
        ])
        .spawn()
        .expect("failed to spawn core with a v3 network-rule extension");

    assert!(wait_for(&core_socket, Duration::from_secs(5)));
    assert!(wait_for(&extension_socket, Duration::from_secs(5)));
    let mut frontend = UnixStream::connect(&core_socket).unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();
    let mut extension = UnixStream::connect(&extension_socket).unwrap();
    write_extension_request(
        &mut extension,
        &ExtensionRequest::Hello {
            extension_id,
            capability_versions: BTreeMap::from([("network:intercept".to_string(), 3)]),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::HelloAck {
            unsupported_capabilities: BTreeMap::new(),
        }
    );
    write_extension_request(
        &mut extension,
        &ExtensionRequest::RegisterNetworkBlockUrl { url: url.clone() },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::NetworkInterceptAck
    );

    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::Navigate { url: redirect_url },
    )
    .unwrap();
    match blueice_ipc::read_server_message(&mut frontend).unwrap() {
        blueice_ipc::ServerMessage::Error { message } => {
            assert!(message.contains("declarative extension rule"));
            assert!(message.contains(&url));
        }
        other => panic!("matching network rule must reject the navigation, got {other:?}"),
    }
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        ErrorKind::WouldBlock,
        "the extension rule must prevent core from opening an HTTP connection"
    );
    redirect_server.join().unwrap();

    // Clearing is a version-3 operation that can only remove rules from this
    // extension connection. It needs no new gatekeeper action review because
    // it reduces, rather than adds, privileged network policy.
    write_extension_request(&mut extension, &ExtensionRequest::ClearNetworkBlockUrls).unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::NetworkInterceptAck
    );
    listener.set_nonblocking(false).unwrap();
    let target_server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 19\r\nConnection: close\r\n\r\n<p>now allowed</p>\n",
            )
            .unwrap();
    });
    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::Navigate { url: url.clone() },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::Navigated { url }
    );
    target_server.join().unwrap();

    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown)
        .unwrap();
    assert!(core.wait().unwrap().success());
    assert!(!core_socket.exists());
    assert!(!extension_socket.exists());
    assert!(!frame_dir.exists());
    let _ = std::fs::remove_dir_all(package_root);
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn installed_extension_v4_host_rule_blocks_redirect_before_target_connection() {
    let _guard = core_process_test_guard();
    use blueice_ipc::extension::{
        read_extension_reply, write_extension_request, ExtensionReply, ExtensionRequest,
    };
    use std::collections::BTreeMap;
    use std::io::ErrorKind;

    let core_socket = unique_socket_path("ext-host-rule");
    let extension_socket = unique_private_extension_socket_path("host-rule");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-extension-host-rule-frames-{}", std::process::id()
    ));
    let (package_root, manifest, extension_id) =
        extension_manifest_package("host-network-rule", &["network:intercept"]);
    let gatekeeper_socket = clearing_gatekeeper("ehrg");
    let _ = std::fs::remove_file(&core_socket);
    let _ = std::fs::remove_file(&extension_socket);
    let _ = std::fs::remove_dir_all(&frame_dir);

    let target_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    target_listener.set_nonblocking(true).unwrap();
    let blocked_url = format!("http://localhost:{}/private", target_listener.local_addr().unwrap().port());
    let redirect_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let redirect_url = format!("http://{}/before", redirect_listener.local_addr().unwrap());
    let redirect_server = thread::spawn({
        let blocked_url = blocked_url.clone();
        move || {
            let (mut stream, _) = redirect_listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            stream.write_all(format!(
                "HTTP/1.1 302 Found\r\nLocation: {blocked_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            ).as_bytes()).unwrap();
        }
    });

    let mut core = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket", core_socket.to_str().unwrap(),
            "--extension-socket", extension_socket.to_str().unwrap(),
            "--extension-manifest", manifest.to_str().unwrap(),
            "--gatekeeper-socket", gatekeeper_socket.to_str().unwrap(),
            "--frame-dir", frame_dir.to_str().unwrap(),
        ])
        .spawn()
        .expect("failed to spawn core with a v4 host-rule extension");
    assert!(wait_for(&core_socket, Duration::from_secs(5)));
    assert!(wait_for(&extension_socket, Duration::from_secs(5)));
    let mut frontend = UnixStream::connect(&core_socket).unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();
    let mut extension = UnixStream::connect(&extension_socket).unwrap();
    write_extension_request(&mut extension, &ExtensionRequest::Hello {
        extension_id,
        capability_versions: BTreeMap::from([("network:intercept".to_string(), 4)]),
    }).unwrap();
    assert_eq!(read_extension_reply(&mut extension).unwrap(), ExtensionReply::HelloAck {
        unsupported_capabilities: BTreeMap::new(),
    });
    write_extension_request(&mut extension, &ExtensionRequest::RegisterNetworkBlockHost {
        host: "LOCALHOST.".to_string(),
    }).unwrap();
    assert_eq!(read_extension_reply(&mut extension).unwrap(), ExtensionReply::NetworkInterceptAck);

    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Navigate {
        url: redirect_url,
    }).unwrap();
    match blueice_ipc::read_server_message(&mut frontend).unwrap() {
        blueice_ipc::ServerMessage::Error { message } => {
            assert!(message.contains("declarative extension rule"));
            assert!(message.contains(&blocked_url));
        }
        other => panic!("the v4 host rule must reject the redirect target, got {other:?}"),
    }
    redirect_server.join().unwrap();
    assert_eq!(target_listener.accept().unwrap_err().kind(), ErrorKind::WouldBlock);

    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown).unwrap();
    assert!(core.wait().unwrap().success());
    assert!(!core_socket.exists());
    assert!(!extension_socket.exists());
    assert!(!frame_dir.exists());
    let _ = std::fs::remove_dir_all(package_root);
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn scoped_network_intercept_rule_affects_only_its_exact_origin_and_redirect_targets() {
    let _guard = core_process_test_guard();
    use blueice_ipc::extension::{
        read_extension_reply, write_extension_request, ExtensionReply, ExtensionRequest,
    };
    use std::collections::BTreeMap;
    use std::io::ErrorKind;

    let core_socket = unique_socket_path("eisc");
    let extension_socket = unique_private_extension_socket_path("eisc");
    let gatekeeper_socket = clearing_gatekeeper("eisg");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-intercept-scope-frames-{}",
        std::process::id()
    ));
    let (package_root, manifest, _) =
        extension_manifest_package("intercept-scope", &["network:intercept"]);
    let target_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    target_listener.set_nonblocking(true).unwrap();
    let target_url = format!("http://{}/private", target_listener.local_addr().unwrap());
    let target_origin = format!("http://{}", target_listener.local_addr().unwrap());
    std::fs::write(
        &manifest,
        format!(
            r#"{{"name":"Core bridge test","version":"1.0.0","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{{"declared":["network:intercept"]}},"capability_origins":{{"network:intercept":["{target_origin}"]}}}}"#
        ),
    )
    .unwrap();
    let extension_id = blueice_extension_host::load_installed_extension(&manifest)
        .unwrap()
        .extension_id()
        .to_string();
    let open_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let open_origin = format!("http://{}", open_listener.local_addr().unwrap());
    let open_server = thread::spawn({
        let target_url = target_url.clone();
        move || {
            for response in [
                "HTTP/1.1 200 OK\r\nContent-Length: 13\r\nConnection: close\r\n\r\n<h1>Open</h1>",
                "",
            ] {
                let (mut stream, _) = open_listener.accept().unwrap();
                let mut request = [0u8; 1024];
                let _ = stream.read(&mut request);
                if response.is_empty() {
                    stream
                        .write_all(
                            format!(
                                "HTTP/1.1 302 Found\r\nLocation: {target_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            )
                            .as_bytes(),
                        )
                        .unwrap();
                } else {
                    stream.write_all(response.as_bytes()).unwrap();
                }
            }
        }
    });
    let _ = std::fs::remove_file(&core_socket);
    let _ = std::fs::remove_file(&extension_socket);
    let _ = std::fs::remove_dir_all(&frame_dir);
    let mut core = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            core_socket.to_str().unwrap(),
            "--extension-socket",
            extension_socket.to_str().unwrap(),
            "--extension-manifest",
            manifest.to_str().unwrap(),
            "--gatekeeper-socket",
            gatekeeper_socket.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
        ])
        .spawn()
        .expect("failed to spawn core with a scoped network rule");
    assert!(wait_for(&core_socket, Duration::from_secs(5)));
    assert!(wait_for(&extension_socket, Duration::from_secs(5)));
    let mut frontend = UnixStream::connect(&core_socket).unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();
    let mut extension = UnixStream::connect(&extension_socket).unwrap();
    write_extension_request(
        &mut extension,
        &ExtensionRequest::Hello {
            extension_id,
            capability_versions: BTreeMap::from([("network:intercept".to_string(), 4)]),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::HelloAck {
            unsupported_capabilities: BTreeMap::new(),
        }
    );
    write_extension_request(
        &mut extension,
        &ExtensionRequest::RegisterNetworkBlockHost {
            host: "127.0.0.1".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::NetworkInterceptAck
    );

    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::Navigate {
            url: format!("{open_origin}/safe"),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::Navigated { .. }
    ));
    assert!(matches!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));

    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::Navigate {
            url: format!("{open_origin}/redirect"),
        },
    )
    .unwrap();
    match blueice_ipc::read_server_message(&mut frontend).unwrap() {
        blueice_ipc::ServerMessage::Error { message } => {
            assert!(message.contains("declarative extension rule"));
            assert!(message.contains(&target_url));
        }
        other => panic!("the in-scope redirect target must be blocked, got {other:?}"),
    }
    open_server.join().unwrap();
    assert_eq!(target_listener.accept().unwrap_err().kind(), ErrorKind::WouldBlock);

    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown)
        .unwrap();
    assert!(core.wait().unwrap().success());
    assert!(!core_socket.exists());
    assert!(!extension_socket.exists());
    let _ = std::fs::remove_file(gatekeeper_socket);
    let _ = std::fs::remove_dir_all(package_root);
}

#[test]
fn installed_extension_v5_path_prefix_blocks_redirect_before_target_connection() {
    let _guard = core_process_test_guard();
    use blueice_ipc::extension::{
        read_extension_reply, write_extension_request, ExtensionReply, ExtensionRequest,
    };
    use std::collections::BTreeMap;
    use std::io::ErrorKind;

    let core_socket = unique_socket_path("epr");
    let extension_socket = unique_private_extension_socket_path("pr");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-extension-path-prefix-frames-{}", std::process::id()
    ));
    let (package_root, manifest, extension_id) =
        extension_manifest_package("path-prefix-network-rule", &["network:intercept"]);
    let gatekeeper_socket = clearing_gatekeeper("eprg");
    let _ = std::fs::remove_file(&core_socket);
    let _ = std::fs::remove_file(&extension_socket);
    let _ = std::fs::remove_dir_all(&frame_dir);

    let target_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    target_listener.set_nonblocking(true).unwrap();
    let blocked_url = format!(
        "http://localhost:{}/private/report?download=1",
        target_listener.local_addr().unwrap().port()
    );
    let redirect_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let redirect_url = format!("http://{}/before", redirect_listener.local_addr().unwrap());
    let redirect_server = thread::spawn({
        let blocked_url = blocked_url.clone();
        move || {
            let (mut stream, _) = redirect_listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            stream.write_all(format!(
                "HTTP/1.1 302 Found\r\nLocation: {blocked_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            ).as_bytes()).unwrap();
        }
    });

    let mut core = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket", core_socket.to_str().unwrap(),
            "--extension-socket", extension_socket.to_str().unwrap(),
            "--extension-manifest", manifest.to_str().unwrap(),
            "--gatekeeper-socket", gatekeeper_socket.to_str().unwrap(),
            "--frame-dir", frame_dir.to_str().unwrap(),
        ])
        .spawn()
        .expect("failed to spawn core with a v5 path-prefix extension");
    assert!(wait_for(&core_socket, Duration::from_secs(5)));
    assert!(wait_for(&extension_socket, Duration::from_secs(5)));
    let mut frontend = UnixStream::connect(&core_socket).unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();
    let mut extension = UnixStream::connect(&extension_socket).unwrap();
    write_extension_request(&mut extension, &ExtensionRequest::Hello {
        extension_id,
        capability_versions: BTreeMap::from([("network:intercept".to_string(), 5)]),
    }).unwrap();
    assert_eq!(read_extension_reply(&mut extension).unwrap(), ExtensionReply::HelloAck {
        unsupported_capabilities: BTreeMap::new(),
    });
    write_extension_request(&mut extension, &ExtensionRequest::RegisterNetworkBlockPathPrefix {
        host: "LOCALHOST.".into(), path_prefix: "/private".into(),
    }).unwrap();
    assert_eq!(read_extension_reply(&mut extension).unwrap(), ExtensionReply::NetworkInterceptAck);

    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Navigate {
        url: redirect_url,
    }).unwrap();
    match blueice_ipc::read_server_message(&mut frontend).unwrap() {
        blueice_ipc::ServerMessage::Error { message } => {
            assert!(message.contains("declarative extension rule"));
            assert!(message.contains(&blocked_url));
        }
        other => panic!("the v5 path-prefix rule must reject the redirect, got {other:?}"),
    }
    redirect_server.join().unwrap();
    assert_eq!(target_listener.accept().unwrap_err().kind(), ErrorKind::WouldBlock);

    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown).unwrap();
    assert!(core.wait().unwrap().success());
    assert!(!core_socket.exists());
    assert!(!extension_socket.exists());
    assert!(!frame_dir.exists());
    let _ = std::fs::remove_dir_all(package_root);
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn installed_extension_v6_redirects_one_same_origin_navigation_before_fetch_and_clears_it() {
    let _guard = core_process_test_guard();
    use blueice_ipc::extension::{
        read_extension_reply, write_extension_request, ExtensionReply, ExtensionRequest,
    };
    use std::collections::BTreeMap;

    let core_socket = unique_socket_path("er6");
    let extension_socket = unique_private_extension_socket_path("r6");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-extension-redirect-frames-{}", std::process::id()
    ));
    let (package_root, manifest, extension_id) =
        extension_manifest_package("same-origin-redirect", &["network:intercept"]);
    let gatekeeper_socket = clearing_gatekeeper("r6g");
    let _ = std::fs::remove_file(&core_socket);
    let _ = std::fs::remove_file(&extension_socket);
    let _ = std::fs::remove_dir_all(&frame_dir);

    let web_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", web_listener.local_addr().unwrap());
    let source_url = format!("{origin}/old");
    let target_url = format!("{origin}/new");
    let web_server = thread::spawn(move || {
        for (path, body) in [
            ("/new", "<h1>Rewritten</h1>"),
            ("/old", "<h1>Original</h1>"),
        ] {
            let (mut stream, _) = web_listener.accept().unwrap();
            let mut request = [0u8; 1024];
            let len = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..len]);
            assert!(request.starts_with(&format!("GET {path} ")), "{request}");
            stream.write_all(format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len(),
            ).as_bytes()).unwrap();
        }
    });

    let mut core = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket", core_socket.to_str().unwrap(),
            "--extension-socket", extension_socket.to_str().unwrap(),
            "--extension-manifest", manifest.to_str().unwrap(),
            "--gatekeeper-socket", gatekeeper_socket.to_str().unwrap(),
            "--frame-dir", frame_dir.to_str().unwrap(),
        ])
        .spawn()
        .expect("failed to spawn core with a v6 redirect extension");
    assert!(wait_for(&core_socket, Duration::from_secs(5)));
    assert!(wait_for(&extension_socket, Duration::from_secs(5)));
    let mut frontend = UnixStream::connect(&core_socket).unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();
    let mut extension = UnixStream::connect(&extension_socket).unwrap();
    write_extension_request(&mut extension, &ExtensionRequest::Hello {
        extension_id,
        capability_versions: BTreeMap::from([("network:intercept".to_string(), 6)]),
    }).unwrap();
    assert_eq!(read_extension_reply(&mut extension).unwrap(), ExtensionReply::HelloAck {
        unsupported_capabilities: BTreeMap::new(),
    });
    write_extension_request(&mut extension, &ExtensionRequest::RegisterNetworkRedirectUrl {
        source_url: source_url.clone(), target_url: target_url.clone(),
    }).unwrap();
    assert_eq!(read_extension_reply(&mut extension).unwrap(), ExtensionReply::NetworkInterceptAck);

    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Navigate {
        url: source_url.clone(),
    }).unwrap();
    assert!(matches!(blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::Navigated { url, .. } if url == target_url));
    assert!(matches!(blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { .. }));

    write_extension_request(&mut extension, &ExtensionRequest::ClearNetworkBlockUrls).unwrap();
    assert_eq!(read_extension_reply(&mut extension).unwrap(), ExtensionReply::NetworkInterceptAck);
    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Navigate {
        url: source_url.clone(),
    }).unwrap();
    assert!(matches!(blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::Navigated { url, .. } if url == source_url));
    assert!(matches!(blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { .. }));

    web_server.join().unwrap();
    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown).unwrap();
    assert!(core.wait().unwrap().success());
    assert!(!core_socket.exists());
    assert!(!extension_socket.exists());
    assert!(!frame_dir.exists());
    let _ = std::fs::remove_dir_all(package_root);
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn installed_extension_storage_v1_v2_v3_keep_their_separate_lifetimes() {
    let _guard = core_process_test_guard();
    use blueice_ipc::extension::{
        read_extension_reply, write_extension_request, ExtensionReply, ExtensionRequest,
    };
    use std::collections::BTreeMap;

    let core_socket = unique_socket_path("ext-storage");
    let extension_socket = unique_private_extension_socket_path("storage");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-extension-storage-frames-{}",
        std::process::id()
    ));
    let data_dir = std::env::temp_dir().join(format!(
        "blueice-core-extension-storage-data-{}",
        std::process::id()
    ));
    let (package_root, manifest, extension_id) =
        extension_manifest_package("storage", &["storage"]);
    let _ = std::fs::remove_file(&core_socket);
    let _ = std::fs::remove_file(&extension_socket);
    let _ = std::fs::remove_dir_all(&frame_dir);
    let _ = std::fs::remove_dir_all(&data_dir);

    let mut core = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            core_socket.to_str().unwrap(),
            "--extension-socket",
            extension_socket.to_str().unwrap(),
            "--extension-manifest",
            manifest.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
        ])
        .env("XDG_DATA_HOME", &data_dir)
        .spawn()
        .expect("failed to spawn core with a storage extension");

    assert!(wait_for(&core_socket, Duration::from_secs(5)));
    assert!(wait_for(&extension_socket, Duration::from_secs(5)));
    let mut frontend = UnixStream::connect(&core_socket).unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();
    let hello = || ExtensionRequest::Hello {
        extension_id: extension_id.clone(),
        capability_versions: BTreeMap::from([("storage".to_string(), 1)]),
    };
    let hello_v2 = || ExtensionRequest::Hello {
        extension_id: extension_id.clone(),
        capability_versions: BTreeMap::from([("storage".to_string(), 2)]),
    };
    let hello_v3 = || ExtensionRequest::Hello {
        extension_id: extension_id.clone(),
        capability_versions: BTreeMap::from([("storage".to_string(), 3)]),
    };

    let mut first_connection = UnixStream::connect(&extension_socket).unwrap();
    write_extension_request(&mut first_connection, &hello()).unwrap();
    assert_eq!(
        read_extension_reply(&mut first_connection).unwrap(),
        ExtensionReply::HelloAck {
            unsupported_capabilities: BTreeMap::new(),
        }
    );
    write_extension_request(
        &mut first_connection,
        &ExtensionRequest::StorageSet {
            key: "task-state".to_string(),
            value: "complete".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut first_connection).unwrap(),
        ExtensionReply::StorageSetAck
    );
    drop(first_connection);

    let mut reconnected = UnixStream::connect(&extension_socket).unwrap();
    write_extension_request(&mut reconnected, &hello()).unwrap();
    assert_eq!(
        read_extension_reply(&mut reconnected).unwrap(),
        ExtensionReply::HelloAck {
            unsupported_capabilities: BTreeMap::new(),
        }
    );
    write_extension_request(
        &mut reconnected,
        &ExtensionRequest::StorageGet {
            key: "task-state".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut reconnected).unwrap(),
        ExtensionReply::StorageGetResult {
            value: Some("complete".to_string()),
        }
    );
    write_extension_request(
        &mut reconnected,
        &ExtensionRequest::StorageRemove {
            key: "task-state".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut reconnected).unwrap(),
        ExtensionReply::StorageRemoveAck { removed: true }
    );
    drop(reconnected);

    let mut durable = UnixStream::connect(&extension_socket).unwrap();
    write_extension_request(&mut durable, &hello_v2()).unwrap();
    assert_eq!(
        read_extension_reply(&mut durable).unwrap(),
        ExtensionReply::HelloAck {
            unsupported_capabilities: BTreeMap::new(),
        }
    );
    write_extension_request(
        &mut durable,
        &ExtensionRequest::DurableStorageSet {
            key: "task-state".to_string(),
            value: "persisted".to_string(),
        },
    )
    .unwrap();
    assert_eq!(read_extension_reply(&mut durable).unwrap(), ExtensionReply::StorageSetAck);
    write_extension_request(
        &mut durable,
        &ExtensionRequest::StorageSet {
            key: "task-state".to_string(),
            value: "temporary".to_string(),
        },
    )
    .unwrap();
    assert_eq!(read_extension_reply(&mut durable).unwrap(), ExtensionReply::StorageSetAck);
    write_extension_request(
        &mut durable,
        &ExtensionRequest::StorageSet {
            key: "ephemeral-only".to_string(),
            value: "not-durable".to_string(),
        },
    ).unwrap();
    assert_eq!(read_extension_reply(&mut durable).unwrap(), ExtensionReply::StorageSetAck);
    write_extension_request(&mut durable, &ExtensionRequest::DurableStorageListKeys).unwrap();
    assert!(matches!(read_extension_reply(&mut durable).unwrap(),
        ExtensionReply::CapabilityDenied { capability, .. } if capability == "storage"));
    write_extension_request(&mut durable, &hello_v3()).unwrap();
    assert_eq!(read_extension_reply(&mut durable).unwrap(),
        ExtensionReply::HelloAck { unsupported_capabilities: BTreeMap::new() });
    write_extension_request(&mut durable, &ExtensionRequest::DurableStorageListKeys).unwrap();
    assert_eq!(read_extension_reply(&mut durable).unwrap(),
        ExtensionReply::StorageKeysResult { keys: vec!["task-state".to_string()] });
    drop(durable);

    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown)
        .unwrap();
    assert!(core.wait().unwrap().success());
    assert!(!core_socket.exists());
    assert!(!extension_socket.exists());
    assert!(!frame_dir.exists());

    let mut restarted_core = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            core_socket.to_str().unwrap(),
            "--extension-socket",
            extension_socket.to_str().unwrap(),
            "--extension-manifest",
            manifest.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
        ])
        .env("XDG_DATA_HOME", &data_dir)
        .spawn()
        .expect("failed to restart core with the same storage extension");
    assert!(wait_for(&core_socket, Duration::from_secs(5)));
    assert!(wait_for(&extension_socket, Duration::from_secs(5)));
    let mut restarted_frontend = UnixStream::connect(&core_socket).unwrap();
    blueice_ipc::client_handshake(&mut restarted_frontend).unwrap();
    let mut restarted_extension = UnixStream::connect(&extension_socket).unwrap();
    write_extension_request(&mut restarted_extension, &hello_v3()).unwrap();
    assert_eq!(
        read_extension_reply(&mut restarted_extension).unwrap(),
        ExtensionReply::HelloAck {
            unsupported_capabilities: BTreeMap::new(),
        }
    );
    write_extension_request(
        &mut restarted_extension,
        &ExtensionRequest::DurableStorageGet { key: "task-state".to_string() },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut restarted_extension).unwrap(),
        ExtensionReply::StorageGetResult { value: Some("persisted".to_string()) }
    );
    write_extension_request(&mut restarted_extension, &ExtensionRequest::DurableStorageListKeys)
        .unwrap();
    assert_eq!(read_extension_reply(&mut restarted_extension).unwrap(),
        ExtensionReply::StorageKeysResult { keys: vec!["task-state".to_string()] });
    write_extension_request(
        &mut restarted_extension,
        &ExtensionRequest::StorageGet { key: "task-state".to_string() },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut restarted_extension).unwrap(),
        ExtensionReply::StorageGetResult { value: None }
    );
    blueice_ipc::write_client_message(&mut restarted_frontend, &blueice_ipc::ClientMessage::Shutdown)
        .unwrap();
    assert!(restarted_core.wait().unwrap().success());
    let _ = std::fs::remove_dir_all(package_root);
    let _ = std::fs::remove_dir_all(data_dir);
}

#[test]
fn optional_and_ephemeral_manifest_entries_cannot_be_self_granted_over_core_ipc() {
    let _guard = core_process_test_guard();
    use blueice_ipc::extension::{
        read_extension_reply, write_extension_request, ExtensionReply, ExtensionRequest,
    };
    use std::collections::BTreeMap;

    let core_socket = unique_socket_path("opt-tier");
    let extension_socket = unique_private_extension_socket_path("opt");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-ungranted-frames-{}",
        std::process::id()
    ));
    let data_dir = std::env::temp_dir().join(format!(
        "blueice-core-ungranted-data-{}",
        std::process::id()
    ));
    let (package_root, manifest, _) = extension_manifest_package("ungranted", &[]);
    std::fs::write(
        &manifest,
        r#"{"name":"Ungrantable tiers","version":"1.0.0","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"optional":["storage"],"runtime_ephemeral":["dom:read"]}}"#,
    )
    .unwrap();
    let extension_id = blueice_extension_host::load_installed_extension(&manifest)
        .unwrap()
        .extension_id()
        .to_string();
    let _ = std::fs::remove_file(&core_socket);
    let _ = std::fs::remove_file(&extension_socket);
    let _ = std::fs::remove_dir_all(&frame_dir);
    let _ = std::fs::remove_dir_all(&data_dir);

    let mut core = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            core_socket.to_str().unwrap(),
            "--extension-socket",
            extension_socket.to_str().unwrap(),
            "--extension-manifest",
            manifest.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
        ])
        .env("XDG_DATA_HOME", &data_dir)
        .spawn()
        .expect("failed to spawn core with an optional-only extension");
    assert!(wait_for(&core_socket, Duration::from_secs(5)));
    assert!(wait_for(&extension_socket, Duration::from_secs(5)));
    let mut frontend = UnixStream::connect(&core_socket).unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();
    let mut extension = UnixStream::connect(&extension_socket).unwrap();
    write_extension_request(
        &mut extension,
        &ExtensionRequest::Hello {
            extension_id,
            capability_versions: BTreeMap::from([
                ("storage".to_string(), 3),
                ("dom:read".to_string(), 2),
            ]),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::HelloAck {
            unsupported_capabilities: BTreeMap::new(),
        }
    );
    for (request, expected_capability) in [
        (ExtensionRequest::DurableStorageListKeys, "storage"),
        (
            ExtensionRequest::DurableStorageSet {
                key: "key".into(),
                value: "never-persist".into(),
            },
            "storage",
        ),
        (
            ExtensionRequest::StorageSet {
                key: "key".into(),
                value: "never-store".into(),
            },
            "storage",
        ),
        (ExtensionRequest::DomReadTab { tab_id: 1 }, "dom:read"),
    ] {
        write_extension_request(&mut extension, &request).unwrap();
        match read_extension_reply(&mut extension).unwrap() {
            ExtensionReply::CapabilityDenied { capability, .. } => {
                assert_eq!(capability, expected_capability)
            }
            other => panic!("{expected_capability} must remain ungranted, got {other:?}"),
        }
    }
    assert!(!data_dir.exists(), "denied durable writes must not create a data directory");
    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown)
        .unwrap();
    assert!(core.wait().unwrap().success());
    let _ = std::fs::remove_dir_all(package_root);
}

#[test]
fn private_parent_pipe_grants_optional_and_consumes_ephemeral_once_in_a_real_core() {
    let _guard = core_process_test_guard();
    use blueice_ipc::permission_control::{
        read_permission_control_reply, write_permission_control_request,
        PermissionControlReply, PermissionControlRequest,
    };

    let core_socket = unique_socket_path("perm-core");
    let extension_socket = unique_private_extension_socket_path("perm");
    let probe_socket = unique_socket_path("perm-probe");
    let frame_dir = std::env::temp_dir().join(format!("blueice-permission-frames-{}", std::process::id()));
    let (package_root, manifest, _) = extension_manifest_package("permission-parent", &[]);
    std::fs::write(&manifest, r#"{"name":"Optional storage","version":"1","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"optional":["storage"],"runtime_ephemeral":["dom:read"]}}"#).unwrap();
    let extension_id = blueice_extension_host::load_installed_extension(&manifest).unwrap().extension_id().to_string();
    let host = core_extension_host_probe_script(&package_root, "extension_host_probe_child_observes_optional_and_ephemeral_grants");
    let _ = std::fs::remove_file(&core_socket);
    let _ = std::fs::remove_file(&extension_socket);
    let _ = std::fs::remove_file(&probe_socket);
    let _ = std::fs::remove_dir_all(&frame_dir);
    let probe_listener = UnixListener::bind(&probe_socket).unwrap();
    probe_listener.set_nonblocking(true).unwrap();

    let mut core = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket", core_socket.to_str().unwrap(),
            "--extension-socket", extension_socket.to_str().unwrap(),
            "--extension-manifest", manifest.to_str().unwrap(),
            "--extension-host", host.to_str().unwrap(),
            "--permission-control-stdio",
            "--frame-dir", frame_dir.to_str().unwrap(),
        ])
        .env("BLUEICE_TEST_PROBE_SOCKET", &probe_socket)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn core with a private parent permission pipe");
    assert!(wait_for(&core_socket, Duration::from_secs(5)));
    let mut frontend = UnixStream::connect(&core_socket).unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut probe = loop {
        match probe_listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) => panic!("authenticated host never reached its private probe socket: {error}"),
        }
    };
    probe.set_nonblocking(false).unwrap();
    probe.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let mut control_in = core.stdin.take().unwrap();
    let mut control_out = core.stdout.take().unwrap();
    let probe_get = |probe: &mut UnixStream, command: u8, ticket: Option<&str>| {
        probe.write_all(&[command]).unwrap();
        if matches!(command, b'r' | b's') {
            let ticket = ticket.expect("an ephemeral read probe needs a bearer token");
            assert_eq!(ticket.len(), 64);
            probe.write_all(ticket.as_bytes()).unwrap();
        }
        let mut result = [0_u8; 1];
        probe.read_exact(&mut result).unwrap();
        result[0]
    };
    assert_eq!(probe_get(&mut probe, b'g', None), b'D', "optional declaration must not grant itself");
    let guessed_ticket = "0".repeat(64);
    assert_eq!(probe_get(&mut probe, b'r', Some(&guessed_ticket)), b'D',
        "an extension cannot guess an unarmed ephemeral token");

    write_permission_control_request(&mut control_in, &PermissionControlRequest::Inspect).unwrap();
    let state = read_permission_control_reply(&mut control_out).unwrap();
    let PermissionControlReply::State { extension_id: observed_id, optional, .. } = state else {
        panic!("expected the installed permission state, got {state:?}")
    };
    assert_eq!(observed_id, extension_id);
    assert_eq!(optional.len(), 1, "runtime-ephemeral declarations are not optional grants");
    assert_eq!(optional[0].capability, "storage");
    assert!(!optional[0].granted);

    let inspect_document = |input: &mut std::process::ChildStdin, output: &mut std::process::ChildStdout| {
        write_permission_control_request(input, &PermissionControlRequest::InspectDocument { tab_id: 1 }).unwrap();
        read_permission_control_reply(output).unwrap()
    };
    assert_eq!(inspect_document(&mut control_in, &mut control_out), PermissionControlReply::Document {
        tab_id: 1, document_epoch: 0, url: None,
    });
    for epoch in 1..=2 {
        blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Navigate {
            url: "about:credits".into(),
        }).unwrap();
        assert_eq!(blueice_ipc::read_server_message(&mut frontend).unwrap(),
            blueice_ipc::ServerMessage::Navigated { url: "about:credits".into() });
        assert!(matches!(blueice_ipc::read_server_message(&mut frontend).unwrap(),
            blueice_ipc::ServerMessage::FrameReady { .. }));
        assert_eq!(inspect_document(&mut control_in, &mut control_out), PermissionControlReply::Document {
            tab_id: 1, document_epoch: epoch, url: Some("about:credits".into()),
        }, "same-URL document replacement must invalidate the previous identity");
    }
    write_permission_control_request(&mut control_in, &PermissionControlRequest::InspectDocument {
        tab_id: u64::MAX,
    }).unwrap();
    assert!(matches!(read_permission_control_reply(&mut control_out).unwrap(),
        PermissionControlReply::Rejected { .. }));
    write_permission_control_request(&mut control_in, &PermissionControlRequest::ArmEphemeral {
        capability: "dom:read".into(), tab_id: 1, document_epoch: 1,
    }).unwrap();
    assert!(matches!(read_permission_control_reply(&mut control_out).unwrap(),
        PermissionControlReply::Rejected { .. }),
        "a stale same-URL document identity cannot arm a one-shot lease");
    write_permission_control_request(&mut control_in, &PermissionControlRequest::ArmEphemeral {
        capability: "dom:read".into(), tab_id: 1, document_epoch: 2,
    }).unwrap();
    let PermissionControlReply::EphemeralArmed {
        capability, tab_id, document_epoch, ticket,
    } = read_permission_control_reply(&mut control_out).unwrap() else {
        panic!("the private pipe should arm the live document");
    };
    assert_eq!((capability.as_str(), tab_id, document_epoch), ("dom:read", 1, 2));
    assert_eq!(ticket.len(), 64);
    assert_ne!(ticket, guessed_ticket);
    probe.write_all(b"e").unwrap();
    let mut event_marker = [0_u8; 1];
    probe.read_exact(&mut event_marker).unwrap();
    assert_eq!(event_marker, *b"E");
    let mut event_ticket = [0_u8; 64];
    probe.read_exact(&mut event_ticket).unwrap();
    assert_eq!(event_ticket.as_slice(), ticket.as_bytes(),
        "the authenticated host must receive the exact core-parent-armed ticket");
    assert_eq!(probe_get(&mut probe, b'w', None), b'D',
        "legacy implicit-tab read must not consume a document-bound lease");
    assert_eq!(probe_get(&mut probe, b'v', None), b'D',
        "ordinary v2 explicit-tab read must not consume a document-bound lease");
    assert_eq!(probe_get(&mut probe, b'r', Some(&guessed_ticket)), b'D',
        "a guessed token must not consume the valid lease");
    assert_eq!(probe_get(&mut probe, b's', Some(&ticket)), b'D',
        "another tab must not consume the lease");
    assert_eq!(probe_get(&mut probe, b'r', Some(&ticket)), b'A',
        "the authenticated host may read the exact document once");
    assert_eq!(probe_get(&mut probe, b'r', Some(&ticket)), b'D',
        "a second read must not reuse the consumed ticket");
    write_permission_control_request(&mut control_in, &PermissionControlRequest::ArmEphemeral {
        capability: "dom:read".into(), tab_id: 1, document_epoch: 2,
    }).unwrap();
    let PermissionControlReply::EphemeralArmed { ticket: next_ticket, .. } =
        read_permission_control_reply(&mut control_out).unwrap() else {
            panic!("a second private arming should replace the spent lease");
        };
    assert_ne!(ticket, next_ticket);
    assert_eq!(probe_get(&mut probe, b'r', Some(&ticket)), b'D',
        "an old token cannot borrow a newer arming");
    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Navigate {
        url: "about:credits".into(),
    }).unwrap();
    assert!(matches!(blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::Navigated { .. }));
    assert!(matches!(blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { .. }));
    assert_eq!(probe_get(&mut probe, b'r', Some(&next_ticket)), b'D',
        "same-URL navigation must expire an unspent lease");

    write_permission_control_request(&mut control_in, &PermissionControlRequest::Grant { capability: "dom:read".into() }).unwrap();
    assert!(matches!(read_permission_control_reply(&mut control_out).unwrap(), PermissionControlReply::Rejected { .. }));
    assert_eq!(probe_get(&mut probe, b'g', None), b'D');
    write_permission_control_request(&mut control_in, &PermissionControlRequest::Grant { capability: "storage".into() }).unwrap();
    assert_eq!(read_permission_control_reply(&mut control_out).unwrap(), PermissionControlReply::Updated {
        capability: "storage".into(), granted: true, changed: true,
    });
    assert_eq!(probe_get(&mut probe, b'g', None), b'A', "the same authenticated host connection must gain the optional capability");

    write_permission_control_request(&mut control_in, &PermissionControlRequest::Revoke { capability: "storage".into() }).unwrap();
    assert_eq!(read_permission_control_reply(&mut control_out).unwrap(), PermissionControlReply::Updated {
        capability: "storage".into(), granted: false, changed: true,
    });
    assert_eq!(probe_get(&mut probe, b'g', None), b'D', "the completed revoke must deny the existing host connection");
    write_permission_control_request(&mut control_in, &PermissionControlRequest::Grant { capability: "storage".into() }).unwrap();
    assert!(matches!(read_permission_control_reply(&mut control_out).unwrap(), PermissionControlReply::Updated { granted: true, .. }));
    assert_eq!(probe_get(&mut probe, b'g', None), b'A');
    drop(control_in);
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if probe_get(&mut probe, b'g', None) == b'D' {
            break;
        }
        assert!(Instant::now() < deadline, "parent pipe EOF must revoke its optional grants");
        thread::sleep(Duration::from_millis(20));
    }

    probe.write_all(b"q").unwrap();
    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown).unwrap();
    assert!(core.wait().unwrap().success());
    assert!(!core_socket.exists());
    assert!(!extension_socket.exists());
    let _ = std::fs::remove_file(probe_socket);
    let _ = std::fs::remove_dir_all(package_root);
}

#[test]
fn private_parent_revoke_removes_published_ui_and_network_rules_before_acknowledgement() {
    let _guard = core_process_test_guard();
    use blueice_ipc::permission_control::{
        read_permission_control_reply, write_permission_control_request,
        PermissionControlReply, PermissionControlRequest,
    };

    let core_socket = unique_socket_path("perm-effects-core");
    let extension_socket = unique_private_extension_socket_path("perm-effects");
    let probe_socket = unique_socket_path("perm-effects-probe");
    let gatekeeper_socket = clearing_gatekeeper("perm-effects-gk");
    let frame_dir = std::env::temp_dir().join(format!("blueice-permission-effects-frames-{}", std::process::id()));
    let (package_root, manifest, _) = extension_manifest_package("permission-effects", &[]);
    std::fs::write(&manifest, r#"{"name":"Optional effects","version":"1","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"optional":["ui:inject","network:intercept"]}}"#).unwrap();
    let host = core_extension_host_probe_script(&package_root, "extension_host_probe_child_publishes_optional_effects");
    let _ = std::fs::remove_file(&core_socket);
    let _ = std::fs::remove_file(&extension_socket);
    let _ = std::fs::remove_file(&probe_socket);
    let _ = std::fs::remove_dir_all(&frame_dir);
    let probe_listener = UnixListener::bind(&probe_socket).unwrap();
    probe_listener.set_nonblocking(true).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/optional", listener.local_addr().unwrap());

    let mut core = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket", core_socket.to_str().unwrap(),
            "--extension-socket", extension_socket.to_str().unwrap(),
            "--extension-manifest", manifest.to_str().unwrap(),
            "--extension-host", host.to_str().unwrap(),
            "--permission-control-stdio",
            "--gatekeeper-socket", gatekeeper_socket.to_str().unwrap(),
            "--frame-dir", frame_dir.to_str().unwrap(),
        ])
        .env("BLUEICE_TEST_PROBE_SOCKET", &probe_socket)
        .env("BLUEICE_TEST_RULE_URL", &url)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn core with optional published effects");
    assert!(wait_for(&core_socket, Duration::from_secs(15)));
    let mut frontend = UnixStream::connect(&core_socket).unwrap();
    frontend.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut probe = loop {
        match probe_listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) => panic!("authenticated optional-effects host never reached its probe: {error}"),
        }
    };
    probe.set_nonblocking(false).unwrap();
    probe.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut ready = [0_u8; 1];
    probe.read_exact(&mut ready).unwrap();
    assert_eq!(ready[0], b'S', "the authenticated host must enter its probe loop");
    let probe_actions = |probe: &mut UnixStream, command: u8| {
        probe.write_all(&[command]).unwrap();
        let mut answer = [0_u8; 1];
        probe.read_exact(&mut answer).unwrap();
        answer[0]
    };
    assert_eq!(probe_actions(&mut probe, b'd'), b'D', "optional declarations are not grants");
    let mut control_in = core.stdin.take().unwrap();
    let mut control_out = core.stdout.take().unwrap();
    for capability in ["ui:inject", "network:intercept"] {
        write_permission_control_request(&mut control_in, &PermissionControlRequest::Grant {
            capability: capability.into(),
        }).unwrap();
        assert_eq!(read_permission_control_reply(&mut control_out).unwrap(), PermissionControlReply::Updated {
            capability: capability.into(), granted: true, changed: true,
        });
    }
    assert_eq!(probe_actions(&mut probe, b'p'), b'P');
    assert_eq!(blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::ExtensionToolbar { label: Some("Optional Notes".into()) });

    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Navigate {
        url: url.clone(),
    }).unwrap();
    assert!(matches!(blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::Error { message } if message.contains("declarative extension rule")));
    assert_eq!(listener.accept().unwrap_err().kind(), std::io::ErrorKind::WouldBlock,
        "the published rule must block before opening a connection");

    for capability in ["ui:inject", "network:intercept"] {
        write_permission_control_request(&mut control_in, &PermissionControlRequest::Revoke {
            capability: capability.into(),
        }).unwrap();
        assert_eq!(read_permission_control_reply(&mut control_out).unwrap(), PermissionControlReply::Updated {
            capability: capability.into(), granted: false, changed: true,
        });
        if capability == "ui:inject" {
            assert_eq!(blueice_ipc::read_server_message(&mut frontend).unwrap(),
                blueice_ipc::ServerMessage::ExtensionToolbar { label: None },
                "the native toolbar must be removed before revoke completes");
        }
    }
    assert_eq!(probe_actions(&mut probe, b'd'), b'D', "the same host cannot republish either effect");

    let target_server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(20));
                }
                Err(error) => panic!("navigation did not reconnect after completed revoke: {error}"),
            }
        };
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 16\r\nConnection: close\r\n\r\n<p>now open</p>\n").unwrap();
    });
    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Navigate {
        url: url.clone(),
    }).unwrap();
    assert_eq!(blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::Navigated { url });
    target_server.join().unwrap();

    probe.write_all(b"q").unwrap();
    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown).unwrap();
    assert!(core.wait().unwrap().success());
    assert!(!core_socket.exists());
    assert!(!extension_socket.exists());
    let _ = std::fs::remove_file(probe_socket);
    let _ = std::fs::remove_file(gatekeeper_socket);
    let _ = std::fs::remove_dir_all(package_root);
}

#[test]
fn core_waits_for_its_spawned_extension_host_and_rejects_a_bearer_claim_peer() {
    let _guard = core_process_test_guard();
    use blueice_ipc::extension::{read_extension_reply, write_extension_request, ExtensionRequest};
    use std::collections::BTreeMap;

    // Darwin's Unix-domain socket path budget is small beneath its long
    // per-user temporary root; the PID in `unique_socket_path` keeps these
    // concise labels independent.
    let core_socket = unique_socket_path("aec");
    let extension_socket = unique_private_extension_socket_path("auth");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-authenticated-extension-frames-{}",
        std::process::id()
    ));
    let (package_root, manifest, extension_id) =
        extension_manifest_package("authenticated-host", &["dom:read"]);
    let extension_host = core_extension_host_probe_script(&package_root, "extension_host_probe_child_authenticates_to_core");
    let _ = std::fs::remove_file(&core_socket);
    let _ = std::fs::remove_file(&extension_socket);
    let _ = std::fs::remove_dir_all(&frame_dir);

    let mut core = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            core_socket.to_str().unwrap(),
            "--extension-socket",
            extension_socket.to_str().unwrap(),
            "--extension-manifest",
            manifest.to_str().unwrap(),
            "--extension-host",
            extension_host.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
        ])
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn core with its authenticated extension host");

    // The core socket is its public readiness signal. Its existence proves
    // that the child host completed the token handshake first; merely binding
    // the extension listener is insufficient in this mode.
    if !wait_for(&core_socket, Duration::from_secs(15)) {
        let output = core
            .wait_with_output()
            .expect("failed to collect core startup diagnostics");
        panic!(
            "core never published its frontend socket: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert!(wait_for(&extension_socket, Duration::from_secs(5)));
    assert_eq!(
        std::fs::metadata(&extension_socket)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600,
        "the core-owned extension listener must be private before any peer connects"
    );

    let mut bearer_claim_peer = UnixStream::connect(&extension_socket).unwrap();
    bearer_claim_peer
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    write_extension_request(
        &mut bearer_claim_peer,
        &ExtensionRequest::Hello {
            extension_id,
            capability_versions: BTreeMap::from([("dom:read".to_string(), 1)]),
        },
    )
    .unwrap();
    assert!(
        read_extension_reply(&mut bearer_claim_peer).is_err(),
        "the hash-derived manifest identity alone must not receive HelloAck in core-spawned mode"
    );

    let mut frontend = UnixStream::connect(&core_socket).unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();
    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown)
        .unwrap();
    assert!(core.wait().unwrap().success());
    assert!(!core_socket.exists());
    assert!(!extension_socket.exists());
    assert!(!frame_dir.exists());
    let _ = std::fs::remove_dir_all(package_root);
}

#[test]
fn core_spawned_extension_toolbar_reaches_client_and_activation_reaches_host() {
    let _guard = core_process_test_guard();
    let core_socket = unique_socket_path("uit");
    let extension_socket = unique_private_extension_socket_path("ui");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-ui-extension-frames-{}",
        std::process::id()
    ));
    let (package_root, manifest, _) = extension_manifest_package("native-ui", &["ui:inject"]);
    let gatekeeper_socket = clearing_gatekeeper("ui-gk");
    let extension_host = core_extension_host_probe_script(
        &package_root,
        "extension_host_probe_child_publishes_toolbar_and_handles_activation",
    );
    let _ = std::fs::remove_file(&core_socket);
    let _ = std::fs::remove_file(&extension_socket);
    let _ = std::fs::remove_dir_all(&frame_dir);
    let mut core = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            core_socket.to_str().unwrap(),
            "--extension-socket",
            extension_socket.to_str().unwrap(),
            "--extension-manifest",
            manifest.to_str().unwrap(),
            "--extension-host",
            extension_host.to_str().unwrap(),
            "--gatekeeper-socket",
            gatekeeper_socket.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
        ])
        .spawn()
        .expect("failed to spawn core with its native UI extension host");
    assert!(wait_for(&core_socket, Duration::from_secs(15)));
    let mut frontend = UnixStream::connect(&core_socket).unwrap();
    frontend.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::ExtensionToolbar {
            label: Some("Notes".to_string()),
        }
    );
    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::ActivateExtensionToolbar,
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::ExtensionToolbar {
            label: Some("Clicked".to_string()),
        }
    );
    assert_eq!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::ExtensionPopup {
            popup: Some(blueice_ipc::ExtensionPopup {
                id: 1,
                tab_id: 1,
                title: "Notes".to_string(),
                body: "Saved locally".to_string(),
                action_label: None,
            }),
        }
    );
    assert_eq!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::ExtensionPopup { popup: None }
    );
    assert_eq!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::ExtensionPopup {
            popup: Some(blueice_ipc::ExtensionPopup {
                id: 2,
                tab_id: 1,
                title: "Notes".to_string(),
                body: "Ready to open".to_string(),
                action_label: Some("Open notes".to_string()),
            }),
        }
    );
    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::ActivateExtensionPopupAction { popup_id: 1 },
    ).unwrap();
    assert!(matches!(blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::Error { message }
            if message.contains("no matching live extension popup action")));
    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::ActivateExtensionPopupAction { popup_id: 2 },
    ).unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::ExtensionPopup { popup: None }
    );
    assert_eq!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::ExtensionToolbar {
            label: Some("Actioned".to_string()),
        }
    );
    assert_eq!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::ExtensionToolbar { label: None }
    );
    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown)
        .unwrap();
    assert!(core.wait().unwrap().success());
    assert!(!core_socket.exists());
    assert!(!extension_socket.exists());
    assert!(!frame_dir.exists());
    let _ = std::fs::remove_file(gatekeeper_socket);
    let _ = std::fs::remove_dir_all(package_root);
}

#[test]
fn installed_extension_v9_writes_controls_and_reviewed_visible_text_after_gatekeeper_review() {
    let _guard = core_process_test_guard();
    use blueice_ipc::extension::{
        read_extension_reply, write_extension_request, ExtensionReply, ExtensionRequest,
    };
    use std::collections::BTreeMap;

    // macOS leaves little room below its long per-user temporary root; the
    // PID in `unique_socket_path` still keeps these concise leaves unique.
    let core_socket = unique_socket_path("ev2c");
    let extension_socket = unique_private_extension_socket_path("write");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-extension-v5-frames-{}",
        std::process::id()
    ));
    let (package_root, manifest, extension_id) =
        extension_manifest_package("real-write", &["dom:read", "dom:write"]);
    let gatekeeper_socket = clearing_gatekeeper("ev2g");
    let _ = std::fs::remove_file(&core_socket);
    let _ = std::fs::remove_file(&extension_socket);
    let _ = std::fs::remove_dir_all(&frame_dir);

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let body = r#"<h1 id="headline">Before</h1><p id="formatted">Before <strong>bold <em>and italic</em></strong></p><p id="linked">Before <a href="/next">link</a></p><label for="shared">Shared value</label><input id="shared" type="text" value="before"><label for="agree">Agree</label><input id="agree" type="checkbox"><label for="notes">Notes</label><textarea id="notes">before</textarea><label for="volume">Volume</label><input id="volume" type="range" min="0" max="10" step="2" value="0"><label for="first-priority">First priority</label><input id="first-priority" type="radio" name="priority" checked><label for="second-priority">Second priority</label><input id="second-priority" type="radio" name="priority"><label for="urgency">Urgency</label><select id="urgency"><option id="first-option" selected>First option</option><option id="second-option">Second option</option></select>"#;
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
    });

    let mut core = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            core_socket.to_str().unwrap(),
            "--extension-socket",
            extension_socket.to_str().unwrap(),
            "--extension-manifest",
            manifest.to_str().unwrap(),
            "--gatekeeper-socket",
            gatekeeper_socket.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
        ])
        .spawn()
        .expect("failed to spawn core with a v6 installed extension");

    assert!(wait_for(&core_socket, Duration::from_secs(5)));
    assert!(wait_for(&extension_socket, Duration::from_secs(5)));
    let mut frontend = UnixStream::connect(&core_socket).unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();
    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::Navigate {
            url: format!("http://{addr}"),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::Navigated { .. }
    ));
    assert!(matches!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));
    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::GetRepresentation,
    )
    .unwrap();
    let (
        input_id,
        checkbox_id,
        textarea_id,
        first_radio_id,
        second_radio_id,
        first_option_id,
        second_option_id,
        range_id,
        headline_id,
        formatted_id,
        linked_id,
    ) = match blueice_ipc::read_server_message(&mut frontend).unwrap() {
        blueice_ipc::ServerMessage::Representation(snapshot) => {
            let input_id = snapshot
                .nodes
                .iter()
                .find(|node| matches!(node.role, blueice_ipc::Role::TextBox))
                .expect("the navigated form must expose its text input")
                .id;
            let checkbox_id = snapshot
                .nodes
                .iter()
                .find(|node| matches!(node.role, blueice_ipc::Role::CheckBox))
                .expect("the navigated form must expose its checkbox")
                .id;
            let textarea_id = snapshot
                .nodes
                .iter()
                .find(|node| {
                    matches!(node.role, blueice_ipc::Role::TextBox)
                        && node.name.as_deref() == Some("Notes")
                })
                .expect("the navigated form must expose its textarea")
                .id;
            let first_radio_id = snapshot
                .nodes
                .iter()
                .find(|node| node.name.as_deref() == Some("First priority"))
                .expect("the navigated form must expose its first radio")
                .id;
            let second_radio_id = snapshot
                .nodes
                .iter()
                .find(|node| node.name.as_deref() == Some("Second priority"))
                .expect("the navigated form must expose its second radio")
                .id;
            let first_option_id = snapshot
                .nodes
                .iter()
                .find(|node| {
                    matches!(node.role, blueice_ipc::Role::Option)
                        && node.name.as_deref() == Some("First option")
                })
                .expect("the navigated form must expose its first select option")
                .id;
            let second_option_id = snapshot
                .nodes
                .iter()
                .find(|node| {
                    matches!(node.role, blueice_ipc::Role::Option)
                        && node.name.as_deref() == Some("Second option")
                })
                .expect("the navigated form must expose its second select option")
                .id;
            let range_id = snapshot
                .nodes
                .iter()
                .find(|node| {
                    matches!(node.role, blueice_ipc::Role::Slider)
                        && node.name.as_deref() == Some("Volume")
                })
                .expect("the navigated form must expose its integer range input")
                .id;
            let headline_id = snapshot.nodes.iter().find(|node| matches!(node.role, blueice_ipc::Role::Heading { .. }) && node.name.as_deref() == Some("Before")).expect("the page must expose its heading").id;
            let formatted_id = snapshot.nodes.iter().find(|node| matches!(node.role, blueice_ipc::Role::Paragraph) && node.name.as_deref().is_some_and(|name| name.starts_with("Before bold"))).expect("the page must expose its formatted paragraph").id;
            let linked_id = snapshot.nodes.iter().filter(|node| matches!(node.role, blueice_ipc::Role::Paragraph)).last().expect("the page must expose its linked paragraph").id;
            (
                input_id,
                checkbox_id,
                textarea_id,
                first_radio_id,
                second_radio_id,
                first_option_id,
                second_option_id,
                range_id,
                headline_id,
                formatted_id,
                linked_id,
            )
        }
        other => panic!("expected the input representation, got {other:?}"),
    };

    let mut extension = UnixStream::connect(&extension_socket).unwrap();
    write_extension_request(
        &mut extension,
        &ExtensionRequest::Hello {
            extension_id,
            capability_versions: BTreeMap::from([
                ("dom:read".to_string(), 2),
                ("dom:write".to_string(), 9),
            ]),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::HelloAck {
            unsupported_capabilities: BTreeMap::new(),
        }
    );
    write_extension_request(
        &mut extension,
        &ExtensionRequest::SetTextInputValue {
            tab_id: 1,
            node_id: input_id,
            value: "from extension v2".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::DomWriteAck
    );
    let (reply_tab, request_id, frame) =
        blueice_ipc::read_server_message_with_ids(&mut frontend).unwrap();
    assert_eq!(reply_tab, Some(1));
    assert_eq!(request_id, None);
    assert!(matches!(
        frame,
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));

    write_extension_request(&mut extension, &ExtensionRequest::SetVisibleLeafText {
        tab_id: 1, node_id: headline_id, value: "After".to_string(),
    }).unwrap();
    assert_eq!(read_extension_reply(&mut extension).unwrap(), ExtensionReply::DomWriteAck);
    let (reply_tab, request_id, frame) = blueice_ipc::read_server_message_with_ids(&mut frontend).unwrap();
    assert_eq!(reply_tab, Some(1));
    assert_eq!(request_id, None);
    assert!(matches!(frame, blueice_ipc::ServerMessage::FrameReady { .. }));

    write_extension_request(&mut extension, &ExtensionRequest::SetVisibleTextContent {
        tab_id: 1, node_id: formatted_id, value: "After format".to_string(),
    }).unwrap();
    assert_eq!(read_extension_reply(&mut extension).unwrap(), ExtensionReply::DomWriteAck);
    let (reply_tab, request_id, frame) =
        blueice_ipc::read_server_message_with_ids(&mut frontend).unwrap();
    assert_eq!(reply_tab, Some(1));
    assert_eq!(request_id, None);
    assert!(matches!(frame, blueice_ipc::ServerMessage::FrameReady { .. }));

    write_extension_request(&mut extension, &ExtensionRequest::SetVisibleTextContent {
        tab_id: 1, node_id: linked_id, value: "Must not remove link".to_string(),
    }).unwrap();
    assert!(matches!(read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::OperationUnavailable { capability, reason }
            if capability == "dom:write" && reason.contains("inline formatting")));
    write_extension_request(&mut extension, &ExtensionRequest::SetVisibleTextContent {
        tab_id: 1, node_id: formatted_id, value: "Enter your password".to_string(),
    }).unwrap();
    assert!(matches!(read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::GatekeeperBlocked { capability, category, .. }
            if capability == "dom:write" && category == "extension-visible-text-social-engineering"));

    write_extension_request(
        &mut extension,
        &ExtensionRequest::SetRangeInputValue {
            tab_id: 1,
            node_id: range_id,
            value: 6,
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::DomWriteAck
    );
    let (reply_tab, request_id, frame) =
        blueice_ipc::read_server_message_with_ids(&mut frontend).unwrap();
    assert_eq!(reply_tab, Some(1));
    assert_eq!(request_id, None);
    assert!(matches!(
        frame,
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));

    write_extension_request(
        &mut extension,
        &ExtensionRequest::SelectOption {
            tab_id: 1,
            node_id: second_option_id,
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::DomWriteAck
    );
    let (reply_tab, request_id, frame) =
        blueice_ipc::read_server_message_with_ids(&mut frontend).unwrap();
    assert_eq!(reply_tab, Some(1));
    assert_eq!(request_id, None);
    assert!(matches!(
        frame,
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));

    write_extension_request(
        &mut extension,
        &ExtensionRequest::SetRadioChecked {
            tab_id: 1,
            node_id: second_radio_id,
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::DomWriteAck
    );
    let (reply_tab, request_id, frame) =
        blueice_ipc::read_server_message_with_ids(&mut frontend).unwrap();
    assert_eq!(reply_tab, Some(1));
    assert_eq!(request_id, None);
    assert!(matches!(
        frame,
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));

    write_extension_request(
        &mut extension,
        &ExtensionRequest::SetCheckboxChecked {
            tab_id: 1,
            node_id: checkbox_id,
            checked: true,
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::DomWriteAck
    );
    let (reply_tab, request_id, frame) =
        blueice_ipc::read_server_message_with_ids(&mut frontend).unwrap();
    assert_eq!(reply_tab, Some(1));
    assert_eq!(request_id, None);
    assert!(matches!(
        frame,
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));

    write_extension_request(
        &mut extension,
        &ExtensionRequest::SetTextareaValue {
            tab_id: 1,
            node_id: textarea_id,
            value: "from extension v4\nwith detail".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::DomWriteAck
    );
    let (reply_tab, request_id, frame) =
        blueice_ipc::read_server_message_with_ids(&mut frontend).unwrap();
    assert_eq!(reply_tab, Some(1));
    assert_eq!(request_id, None);
    assert!(matches!(
        frame,
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));

    write_extension_request(&mut extension, &ExtensionRequest::DomReadTab { tab_id: 1 }).unwrap();
    let snapshot = match read_extension_reply(&mut extension).unwrap() {
        ExtensionReply::DomReadResult { value } => {
            serde_json::from_str::<blueice_ipc::AiSnapshot>(&value).unwrap()
        }
        other => panic!("expected the written core snapshot, got {other:?}"),
    };
    assert_eq!(snapshot.tab_id, 1);
    assert_eq!(
        snapshot
            .nodes
            .iter()
            .find(|node| node.id == input_id)
            .and_then(|node| node.state.value.as_deref()),
        Some("from extension v2")
    );
    assert_eq!(
        snapshot
            .nodes
            .iter()
            .find(|node| node.id == checkbox_id)
            .and_then(|node| node.state.checked),
        Some(true)
    );
    assert_eq!(
        snapshot
            .nodes
            .iter()
            .find(|node| node.id == first_option_id)
            .map(|node| node.state.selected),
        Some(false)
    );
    assert_eq!(
        snapshot
            .nodes
            .iter()
            .find(|node| node.id == second_option_id)
            .map(|node| node.state.selected),
        Some(true)
    );
    assert_eq!(
        snapshot
            .nodes
            .iter()
            .find(|node| node.id == textarea_id)
            .and_then(|node| node.state.value.as_deref()),
        Some("from extension v4 with detail")
    );
    assert_eq!(
        snapshot
            .nodes
            .iter()
            .find(|node| node.id == range_id)
            .and_then(|node| node.state.value.as_deref()),
        Some("6")
    );
    assert_eq!(snapshot.nodes.iter().find(|node| node.id == headline_id).and_then(|node| node.name.as_deref()), Some("After"));
    assert_eq!(snapshot.nodes.iter().find(|node| node.id == formatted_id).and_then(|node| node.name.as_deref()), Some("After format"));
    assert_eq!(snapshot.nodes.iter().find(|node| node.id == linked_id).and_then(|node| node.name.as_deref()), Some("Before"));
    assert_eq!(
        snapshot
            .nodes
            .iter()
            .find(|node| node.id == first_radio_id)
            .and_then(|node| node.state.checked),
        Some(false)
    );
    assert_eq!(
        snapshot
            .nodes
            .iter()
            .find(|node| node.id == second_radio_id)
            .and_then(|node| node.state.checked),
        Some(true)
    );

    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown)
        .unwrap();
    assert!(core.wait().unwrap().success());
    assert!(!core_socket.exists());
    assert!(!extension_socket.exists());
    assert!(!frame_dir.exists());
    let _ = std::fs::remove_dir_all(package_root);
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn subprocess_navigation_runs_bluejs_before_its_first_frame() {
    let _guard = core_process_test_guard();
    let socket_path = unique_socket_path("bluejs-session");
    let script_path = unique_socket_path("bluejs-script");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-binary-script-frames-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_file(&script_path);
    let _ = std::fs::remove_dir_all(&frame_dir);
    let gatekeeper_path = clearing_gatekeeper("js-gk");

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let body = r#"<div id="target"></div><script>
            let target = document.querySelector('#target');
            let message = document.createElement('p');
            message.textContent = 'automatic bluejs turn';
            target.appendChild(message);
        </script>"#;
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
    });

    let mut core = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            socket_path.to_str().unwrap(),
            "--script-socket",
            script_path.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
            "--gatekeeper-socket",
            gatekeeper_path.to_str().unwrap(),
        ])
        .spawn()
        .expect("failed to spawn blueice-core with script socket");
    assert!(wait_for(&socket_path, Duration::from_secs(5)));

    let mut bluejs = Command::new(sibling_bluejs_binary())
        .args(["--script-socket", script_path.to_str().unwrap()])
        .spawn()
        .expect("failed to spawn bluejs sibling binary");
    let mut stream = UnixStream::connect(&socket_path).unwrap();
    blueice_ipc::client_handshake(&mut stream).unwrap();
    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::Navigate {
            url: format!("http://{addr}"),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::Navigated { .. }
    ));
    assert!(matches!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { generation: 1, .. }
    ));
    blueice_ipc::write_client_message(&mut stream, &blueice_ipc::ClientMessage::GetDom).unwrap();
    let dom = match blueice_ipc::read_server_message(&mut stream).unwrap() {
        blueice_ipc::ServerMessage::Dom(dom) => dom,
        reply => panic!("expected DOM after scripted navigation, got {reply:?}"),
    };
    assert!(dom.contains("automatic bluejs turn"));

    blueice_ipc::write_client_message(&mut stream, &blueice_ipc::ClientMessage::Shutdown).unwrap();
    assert!(core.wait().unwrap().success());
    assert!(bluejs.wait().unwrap().success());
    assert!(!socket_path.exists());
    assert!(!script_path.exists());
}

#[test]
fn real_subprocess_serves_two_independently_addressed_tabs_without_cross_contamination() {
    let _guard = core_process_test_guard();
    // `phase-16-multi-tab-and-tab-groups/PLAN.md`'s minimal-first-slice
    // proof, one layer up from `session.rs`'s own in-process tests: the
    // real compiled `blueice-core` binary, driven over a real socket,
    // must keep two tabs' navigation and representation fully
    // independent.
    let socket_path = unique_socket_path("multi-tab");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-binary-test-frames-multi-tab-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_dir_all(&frame_dir);
    let gatekeeper_path = clearing_gatekeeper("mt-gk"); // short: Unix socket paths are capped at ~100 bytes total

    let listener_one = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr_one = listener_one.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener_one.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let body = "<p>first tab content</p>";
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
    });
    let listener_two = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr_two = listener_two.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener_two.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let body = "<p>second tab content</p>";
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
    });

    let mut child = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            socket_path.to_str().unwrap(),
            "--width",
            "300",
            "--height",
            "150",
            "--frame-dir",
            frame_dir.to_str().unwrap(),
            "--gatekeeper-socket",
            gatekeeper_path.to_str().unwrap(),
        ])
        .spawn()
        .expect("failed to spawn blueice-core");

    assert!(
        wait_for(&socket_path, Duration::from_secs(5)),
        "blueice-core never created its socket"
    );
    let mut stream =
        UnixStream::connect(&socket_path).expect("failed to connect to the real subprocess");
    blueice_ipc::client_handshake(&mut stream)
        .expect("the real subprocess must complete the protocol_version handshake");

    // Navigate the default (first) tab.
    let url_one = format!("http://{addr_one}");
    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::Navigate {
            url: url_one.clone(),
        },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::Navigated { url: url_one }
    );
    assert!(matches!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));

    // Open a second tab navigated straight to a different URL.
    let url_two = format!("http://{addr_two}");
    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::OpenTab {
            url: Some(url_two.clone()),
        },
    )
    .unwrap();
    let (tab_two, tab_two_url) = match blueice_ipc::read_server_message(&mut stream).unwrap() {
        blueice_ipc::ServerMessage::TabOpened { tab_id, url } => (tab_id, url),
        other => panic!("expected TabOpened, got {other:?}"),
    };
    assert_eq!(tab_two_url, Some(url_two));
    assert!(matches!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));

    // The default tab's own representation must still show only its
    // own content -- opening and navigating a second tab must not have
    // touched it.
    blueice_ipc::write_client_message(&mut stream, &blueice_ipc::ClientMessage::GetRepresentation)
        .unwrap();
    let default_tab_snapshot = match blueice_ipc::read_server_message(&mut stream).unwrap() {
        blueice_ipc::ServerMessage::Representation(snapshot) => snapshot,
        other => panic!("expected Representation, got {other:?}"),
    };
    assert!(default_tab_snapshot
        .nodes
        .iter()
        .any(|n| n.name.as_deref() == Some("first tab content")));
    assert!(!default_tab_snapshot
        .nodes
        .iter()
        .any(|n| n.name.as_deref() == Some("second tab content")));

    // The second tab's representation, addressed explicitly, must show
    // only *its* content.
    blueice_ipc::write_client_message_with_ids(
        &mut stream,
        Some(tab_two),
        None,
        &blueice_ipc::ClientMessage::GetRepresentation,
    )
    .unwrap();
    let (reply_tab, _, reply) = blueice_ipc::read_server_message_with_ids(&mut stream).unwrap();
    assert_eq!(reply_tab, Some(tab_two));
    let tab_two_snapshot = match reply {
        blueice_ipc::ServerMessage::Representation(snapshot) => snapshot,
        other => panic!("expected Representation, got {other:?}"),
    };
    assert_eq!(tab_two_snapshot.tab_id, tab_two);
    assert!(tab_two_snapshot
        .nodes
        .iter()
        .any(|n| n.name.as_deref() == Some("second tab content")));
    assert!(!tab_two_snapshot
        .nodes
        .iter()
        .any(|n| n.name.as_deref() == Some("first tab content")));

    blueice_ipc::write_client_message(&mut stream, &blueice_ipc::ClientMessage::Shutdown).unwrap();
    let status = child
        .wait()
        .expect("failed to wait for blueice-core to exit");
    assert!(
        status.success(),
        "blueice-core must exit cleanly after Shutdown"
    );

    assert!(!socket_path.exists());
    assert!(!frame_dir.exists());
}

#[test]
fn a_client_disconnecting_without_shutdown_still_lets_the_subprocess_exit_cleanly() {
    let _guard = core_process_test_guard();
    let socket_path = unique_socket_path("disconnect");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-binary-test-frames-disconnect-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_dir_all(&frame_dir);

    let mut child = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            socket_path.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
        ])
        .spawn()
        .expect("failed to spawn blueice-core");

    assert!(wait_for(&socket_path, Duration::from_secs(5)));
    let stream = UnixStream::connect(&socket_path).unwrap();
    drop(stream); // disconnect without ever sending Shutdown

    let status = child.wait_timeout_or_kill();
    assert!(
        status.success(),
        "blueice-core must exit cleanly when its one client just disconnects"
    );
}

trait WaitTimeoutOrKill {
    fn wait_timeout_or_kill(&mut self) -> std::process::ExitStatus;
}

/// The downloads-page subprocess test starts two independent binaries. Keep
/// their cleanup in a guard so an assertion failure cannot leave either one
/// running or leave its private runtime tree behind for the next test.
struct DownloadsPageCleanup {
    core: Child,
    downloads_socket: PathBuf,
    root: PathBuf,
}

impl Drop for DownloadsPageCleanup {
    fn drop(&mut self) {
        if let Ok(mut raw) = UnixStream::connect(&self.downloads_socket) {
            let _ = blueice_ipc::downloads::write_downloads_request(
                &mut raw,
                Some(1),
                &blueice_ipc::downloads::DownloadsRequest::Hello {
                    protocol_version: blueice_ipc::downloads::DOWNLOADS_PROTOCOL_VERSION,
                },
            );
            let _ = blueice_ipc::downloads::read_downloads_reply(&mut raw);
            let _ = blueice_ipc::downloads::write_downloads_request(
                &mut raw,
                Some(2),
                &blueice_ipc::downloads::DownloadsRequest::Shutdown,
            );
            let _ = blueice_ipc::downloads::read_downloads_reply(&mut raw);
        }
        if self.core.try_wait().ok().flatten().is_none() {
            let _ = self.core.kill();
        }
        let _ = self.core.wait();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl WaitTimeoutOrKill for std::process::Child {
    fn wait_timeout_or_kill(&mut self) -> std::process::ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.try_wait().expect("failed to poll child status") {
                return status;
            }
            if Instant::now() >= deadline {
                let _ = self.kill();
                panic!("blueice-core did not exit within the timeout");
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}

/// `about:downloads`, end to end through two real processes: the compiled
/// `blueice-core` renders the page, and -- because nothing is listening on
/// the downloads socket it is pointed at -- opening the page makes `core`
/// start the real `blueice-downloads` on that socket, after which the open
/// page updates itself without being reloaded.
#[test]
fn opening_about_downloads_starts_the_downloads_process_and_the_open_page_follows_it() {
    let _guard = core_process_test_guard();
    use blueice_ipc::downloads::{
        read_downloads_reply, write_downloads_request, DownloadsClient, DownloadsRequest,
        DOWNLOADS_PROTOCOL_VERSION,
    };
    use blueice_ipc::{ClientMessage, ServerMessage};

    // Private directories for everything the two processes would otherwise
    // put in the user's home or runtime dir.
    let root = std::env::temp_dir().join(format!("bc-dl-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (runtime, data, downloads) = (root.join("run"), root.join("data"), root.join("dl"));
    for dir in [&runtime, &data, &downloads] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let core_socket = root.join("core.sock");
    let downloads_socket = root.join("dl.sock");

    let core = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .arg("--socket")
        .arg(&core_socket)
        .arg("--downloads-socket")
        .arg(&downloads_socket)
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("XDG_DATA_HOME", &data)
        .env("BLUEICE_DOWNLOAD_DIR", &downloads)
        .stdin(Stdio::null())
        .spawn()
        .expect("spawn blueice-core");
    let mut cleanup = DownloadsPageCleanup {
        core,
        downloads_socket: downloads_socket.clone(),
        root: root.clone(),
    };
    assert!(
        wait_for(&core_socket, Duration::from_secs(5)),
        "core never bound its socket"
    );
    let mut client = UnixStream::connect(&core_socket).unwrap();
    // Starting the separate downloads binary can take longer after the
    // compiled-core suite has exercised several real processes. Keep this
    // deadline above that cold-start interval while still bounding a hang.
    client
        .set_read_timeout(Some(Duration::from_secs(45)))
        .unwrap();
    blueice_ipc::client_handshake(&mut client).unwrap();

    let dom = |client: &mut UnixStream| -> String {
        blueice_ipc::write_client_message(client, &ClientMessage::GetDom).unwrap();
        loop {
            match blueice_ipc::read_server_message(client).unwrap() {
                ServerMessage::Dom(text) => return text,
                ServerMessage::FrameReady { .. } => {}
                other => panic!("unexpected {other:?}"),
            }
        }
    };

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "about:downloads".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Navigated {
            url: "about:downloads".to_string()
        }
    );
    assert!(
        dom(&mut client).contains("The downloads service is not running"),
        "nothing was listening yet"
    );

    // Opening the page started the real downloads process, and the page notices.
    assert!(
        wait_for(&downloads_socket, Duration::from_secs(15)),
        "opening the page never started blueice-downloads"
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let text = dom(&mut client);
        if text.contains("No downloads yet.") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the open page never picked up the downloads service: {text}"
        );
        thread::sleep(Duration::from_millis(100));
    }

    // A transfer started by anyone appears in the open page. There is no
    // gatekeeper in this test, so it is blocked (fail-closed) -- which is
    // itself something the page should say.
    let mut downloads =
        DownloadsClient::connect(UnixStream::connect(&downloads_socket).unwrap()).unwrap();
    downloads
        .start("http://127.0.0.1:1/never-fetched.bin", None, false)
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let text = dom(&mut client);
        if text.contains("Blocked") && text.contains("Blocked by the safety gatekeeper") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the open page never showed the new transfer: {text}"
        );
        thread::sleep(Duration::from_millis(100));
    }

    // Tidy up: the downloads process was started on the page's behalf and
    // outlives it by design, so stop it explicitly.
    drop(downloads);
    let mut raw = UnixStream::connect(&downloads_socket).unwrap();
    write_downloads_request(
        &mut raw,
        Some(1),
        &DownloadsRequest::Hello {
            protocol_version: DOWNLOADS_PROTOCOL_VERSION,
        },
    )
    .unwrap();
    read_downloads_reply(&mut raw).unwrap();
    write_downloads_request(&mut raw, Some(2), &DownloadsRequest::Shutdown).unwrap();
    read_downloads_reply(&mut raw).unwrap();
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let _ = cleanup.core.wait();
}
