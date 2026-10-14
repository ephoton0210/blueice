// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(unix)]

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
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::{Duration, Instant};
#[path = "core_binary/compiler_debugger.rs"]
mod compiler_debugger;
#[path = "core_binary/compiler_output.rs"]
mod compiler_output;
#[path = "core_binary/core_session.rs"]
mod core_session;
#[path = "core_binary/multi_tab.rs"]
mod multi_tab;
#[path = "core_binary/page_execution.rs"]
mod page_execution;
#[path = "core_binary/root_control.rs"]
mod root_control;

fn spawn_private_bluejs_host(label: &str) -> (PathBuf, String, thread::JoinHandle<()>) {
    // The default macOS temporary directory can exceed the Unix-domain socket
    // pathname limit once this test's descriptive filename is appended.
    let path =
        PathBuf::from("/tmp").join(format!("blueice-oop-{label}-{}.sock", std::process::id()));
    let token = "0123456789abcdef0123456789abcdef".to_string();
    let listener = blueice_launcher::bluejs_host::bind_bluejs_host_socket(&path)
        .expect("test child host must bind its private socket");
    let child_token = token.clone();
    let child = thread::spawn(move || {
        let mut host = blueice_launcher::bluejs_host::BlueJsChildHost::default();
        blueice_launcher::bluejs_host::serve_bluejs_host_listener(listener, child_token, &mut host)
            .expect("test child host must serve its private protocol");
    });
    (path, token, child)
}

fn shutdown_private_bluejs_host(path: &std::path::Path, token: &str) {
    let mut stream =
        UnixStream::connect(path).expect("must connect to test child host for shutdown");
    blueice_ipc::page_host::write_page_host_request(
        &mut stream,
        &blueice_ipc::page_host::PageHostRequest::Hello {
            protocol_version: blueice_ipc::page_host::PAGE_HOST_PROTOCOL_VERSION,
            session_token: token.to_string(),
        },
    )
    .expect("must send child-host shutdown handshake");
    assert!(matches!(
        blueice_ipc::page_host::read_page_host_reply(&mut stream)
            .expect("child host must acknowledge shutdown handshake"),
        blueice_ipc::page_host::PageHostReply::HelloAck { .. }
    ));
    blueice_ipc::page_host::write_page_host_request(
        &mut stream,
        &blueice_ipc::page_host::PageHostRequest::Shutdown,
    )
    .expect("must request child-host shutdown");
    assert_eq!(
        blueice_ipc::page_host::read_page_host_reply(&mut stream)
            .expect("child host must acknowledge shutdown"),
        blueice_ipc::page_host::PageHostReply::ShutdownAck
    );
}

fn unique_socket_path(label: &str) -> PathBuf {
    // macOS may supply a long TMPDIR; keep the basename short enough for
    // sockaddr_un while preserving the test label and process uniqueness.
    std::env::temp_dir().join(format!("bicb-{label}-{}.sock", std::process::id()))
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
    write_extension_request(
        &mut extension,
        &ExtensionRequest::HelloAuthenticated {
            extension_id: installed.extension_id().to_string(),
            capability_versions: BTreeMap::from([
                ("storage".to_string(), 1),
                ("dom:read".to_string(), 3),
            ]),
            authentication,
        },
    )
    .unwrap();
    assert!(matches!(read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::HelloAck { unsupported_capabilities } if unsupported_capabilities.is_empty()));
    write_extension_request(&mut extension, &ExtensionRequest::RuntimeReady).unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::RuntimeStart
    );

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
                    write_extension_request(&mut extension, &ExtensionRequest::NextRuntimeEvent)
                        .unwrap();
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
                write_extension_request(
                    &mut extension,
                    &ExtensionRequest::StorageGet {
                        key: "sample".into(),
                    },
                )
                .unwrap();
                match read_extension_reply(&mut extension).unwrap() {
                    ExtensionReply::StorageGetResult { value: None } => b'A',
                    ExtensionReply::CapabilityDenied { capability, .. }
                        if capability == "storage" =>
                    {
                        b'D'
                    }
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
                        let snapshot: blueice_ipc::AiSnapshot =
                            serde_json::from_str(&value).unwrap();
                        assert_eq!(snapshot.tab_id, 1);
                        b'A'
                    }
                    ExtensionReply::CapabilityDenied { capability, .. }
                        if capability == "dom:read" =>
                    {
                        b'D'
                    }
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
    write_extension_request(
        &mut extension,
        &ExtensionRequest::HelloAuthenticated {
            extension_id: installed.extension_id().to_string(),
            capability_versions: BTreeMap::from([
                ("ui:inject".to_string(), 3),
                ("network:intercept".to_string(), 2),
            ]),
            authentication,
        },
    )
    .unwrap();
    assert!(matches!(read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::HelloAck { unsupported_capabilities } if unsupported_capabilities.is_empty()));
    write_extension_request(&mut extension, &ExtensionRequest::RuntimeReady).unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::RuntimeStart
    );

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
                ExtensionRequest::SetToolbarButton {
                    label: "Optional Notes".into(),
                },
                "ui:inject",
                ExtensionReply::UiInjectAck,
            ),
            (
                ExtensionRequest::RegisterNetworkBlockUrl {
                    url: rule_url.clone(),
                },
                "network:intercept",
                ExtensionReply::NetworkInterceptAck,
            ),
        ] {
            write_extension_request(&mut extension, &request).unwrap();
            let reply = read_extension_reply(&mut extension).unwrap();
            if published {
                assert_eq!(
                    reply, allowed_reply,
                    "a granted optional action must publish"
                );
            } else {
                assert!(
                    matches!(reply, ExtensionReply::CapabilityDenied { capability: actual, .. } if actual == capability),
                    "an ungranted optional action must be denied as {capability}"
                );
            }
        }
        probe
            .write_all(if published { b"P" } else { b"D" })
            .unwrap();
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
    assert_eq!(
        read_extension_reply(&mut stream).unwrap(),
        ExtensionReply::RuntimeStart
    );
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
    assert_eq!(
        read_extension_reply(&mut stream).unwrap(),
        ExtensionReply::UiInjectAck
    );
    write_extension_request(&mut stream, &ExtensionRequest::NextRuntimeEvent).unwrap();
    assert_eq!(
        read_extension_reply(&mut stream).unwrap(),
        ExtensionReply::RuntimeEvent(ExtensionRuntimeEvent::ToolbarActivated {
            tab_id: 1,
            grant_generation: 0
        })
    );
    write_extension_request(
        &mut stream,
        &ExtensionRequest::SetToolbarButton {
            label: "Clicked".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut stream).unwrap(),
        ExtensionReply::UiInjectAck
    );
    write_extension_request(
        &mut stream,
        &ExtensionRequest::ShowPopup {
            tab_id: 1,
            title: "Notes".to_string(),
            body: "Saved locally".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut stream).unwrap(),
        ExtensionReply::UiInjectAck
    );
    write_extension_request(&mut stream, &ExtensionRequest::ClearPopup).unwrap();
    assert_eq!(
        read_extension_reply(&mut stream).unwrap(),
        ExtensionReply::UiInjectAck
    );
    write_extension_request(
        &mut stream,
        &ExtensionRequest::ShowPopupAction {
            tab_id: 1,
            title: "Notes".to_string(),
            body: "Ready to open".to_string(),
            action_label: "Open notes".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut stream).unwrap(),
        ExtensionReply::UiInjectAck
    );
    write_extension_request(&mut stream, &ExtensionRequest::NextRuntimeEvent).unwrap();
    assert_eq!(
        read_extension_reply(&mut stream).unwrap(),
        ExtensionReply::RuntimeEvent(ExtensionRuntimeEvent::PopupActionActivated {
            tab_id: 1,
            grant_generation: 0
        })
    );
    write_extension_request(
        &mut stream,
        &ExtensionRequest::SetToolbarButton {
            label: "Actioned".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut stream).unwrap(),
        ExtensionReply::UiInjectAck
    );
    write_extension_request(&mut stream, &ExtensionRequest::ClearToolbarButton).unwrap();
    assert_eq!(
        read_extension_reply(&mut stream).unwrap(),
        ExtensionReply::UiInjectAck
    );
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

fn connect_with_retry(path: &std::path::Path, timeout: Duration) -> std::io::Result<UnixStream> {
    let deadline = Instant::now() + timeout;
    loop {
        match UnixStream::connect(path) {
            Ok(stream) => return Ok(stream),
            Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Err(error) => return Err(error),
        }
    }
}

fn serve_html_once(body: &'static str) -> (std::net::SocketAddr, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 1024];
        let _ = stream.read(&mut request);
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
    });
    (address, server)
}

fn read_tab_dom(frontend: &mut UnixStream, tab_id: u64, request_id: u64) -> String {
    blueice_ipc::write_client_message_with_ids(
        frontend,
        Some(tab_id),
        Some(request_id),
        &blueice_ipc::ClientMessage::GetDom,
    )
    .unwrap();
    loop {
        let (reply_tab, reply_id, reply) =
            blueice_ipc::read_server_message_with_ids(frontend).unwrap();
        if reply_id != Some(request_id) {
            assert!(matches!(
                reply,
                blueice_ipc::ServerMessage::FrameReady { .. }
            ));
            continue;
        }
        assert_eq!(reply_tab, Some(tab_id));
        let blueice_ipc::ServerMessage::Dom(dom) = reply else {
            panic!("expected a DOM dump, got {reply:?}");
        };
        return dom;
    }
}

fn script_exchange(
    script: &mut UnixStream,
    next_call_id: &mut u64,
    request: blueice_ipc::script::ScriptRequest,
) -> blueice_ipc::script::ScriptReply {
    use blueice_ipc::script::{ScriptReply, ScriptRequest};
    let Some(target) = request.document_target() else {
        assert!(matches!(request, ScriptRequest::Hello { .. }));
        blueice_ipc::script::write_script_request(script, &request).unwrap();
        return blueice_ipc::script::read_script_reply(script).unwrap();
    };
    let request_id = *next_call_id;
    *next_call_id = next_call_id.checked_add(1).unwrap();
    blueice_ipc::script::write_script_request(
        script,
        &ScriptRequest::Call {
            request_id,
            request: Box::new(request),
        },
    )
    .unwrap();
    match blueice_ipc::script::read_script_reply(script).unwrap() {
        ScriptReply::CallResult {
            request_id: actual_id,
            target: actual_target,
            reply,
        } if actual_id == request_id && actual_target == target => *reply,
        reply => panic!("script reply must echo call ID and document target: {reply:?}"),
    }
}

fn navigate_default_tab(frontend: &mut UnixStream, url: String) {
    blueice_ipc::write_client_message(
        frontend,
        &blueice_ipc::ClientMessage::Navigate { url: url.clone() },
    )
    .unwrap();
    loop {
        let reply = blueice_ipc::read_server_message(frontend)
            .unwrap_or_else(|error| panic!("navigation to {url} failed: {error}"));
        match reply {
            blueice_ipc::ServerMessage::Navigated { url: navigated } => {
                assert_eq!(navigated, url);
                break;
            }
            blueice_ipc::ServerMessage::FrameReady { .. } => {}
            other => panic!("unexpected navigation reply: {other:?}"),
        }
    }
    assert!(matches!(
        blueice_ipc::read_server_message(frontend).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));
}

/// Sends one complete native-debugger request/response pair over the real
/// socket and asserts that the raw public reply has not reflected this
/// fixture's page-controlled secret, a VM/completion representation, or a
/// known BlueJS opcode before decoding it. The protocol intentionally permits
/// an opaque instruction *offset*; that is not bytecode disclosure.
fn debugger_request(
    stream: &mut UnixStream,
    request: &blueice_ipc::debugger::DebuggerRequest,
    page_secret: &str,
) -> blueice_ipc::debugger::DebuggerReply {
    blueice_ipc::debugger::write_debugger_request(stream, request)
        .expect("must write debugger request to real core socket");
    let mut length = [0u8; 4];
    stream
        .read_exact(&mut length)
        .expect("real core must write a debugger reply length");
    let mut payload = vec![0u8; u32::from_le_bytes(length) as usize];
    stream
        .read_exact(&mut payload)
        .expect("real core must write a complete debugger reply payload");
    let rendered =
        std::str::from_utf8(&payload).expect("debugger reply framing must contain UTF-8 JSON");
    assert!(
        !rendered.contains(page_secret),
        "debugger reply must not disclose page source or its completion value: {rendered}"
    );
    assert!(
        !rendered.contains("Value(")
            && !rendered.contains("StoreBinding")
            && !rendered.contains("\"value\"")
            && !rendered.contains("\"completion\"")
            && !rendered.contains("\"opcode\""),
        "debugger reply must not disclose a VM value, completion payload, or bytecode opcode: {rendered}"
    );
    serde_json::from_slice(&payload).expect("real core must return a debugger reply JSON shape")
}

fn wait_for_debugger_state(
    stream: &mut UnixStream,
    program: blueice_ipc::debugger::DebuggerProgram,
    state: blueice_ipc::debugger::DebuggerExecutionState,
    page_secret: &str,
) {
    let expected = blueice_ipc::debugger::DebuggerReply::ExecutionState { program, state };
    let mut observed = None;
    for _ in 0..20 {
        let reply = debugger_request(
            stream,
            &blueice_ipc::debugger::DebuggerRequest::GetExecutionState { program },
            page_secret,
        );
        if reply == expected {
            return;
        }
        observed = Some(reply);
        thread::sleep(Duration::from_millis(50));
    }
    panic!("expected debugger state {expected:?}, last observed {observed:?}");
}

#[path = "core_binary/extensions.rs"]
mod extensions;

#[path = "core_binary/extension_dom.rs"]
mod extension_dom;

#[path = "core_binary/extension_dom_2.rs"]
mod extension_dom_2;

#[path = "core_binary/extension_network.rs"]
mod extension_network;

#[path = "core_binary/permissions.rs"]
mod permissions;

#[path = "core_binary/extension_ui.rs"]
mod extension_ui;

#[path = "core_binary/downloads.rs"]
mod downloads;
