// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use crate::script::{
    direct_page::{DirectInlinePageScriptRequest, DirectPageScriptHost},
    host_typings::{HostTypeSurfaceCatalogV1, HostTypeSurfaceV1},
    http_resource_authorizer::{
        sha256_integrity, HttpOutOfProcessPageScriptSourceAuthorizer, HttpScriptIntegrityManifest,
        HttpScriptResourceLimits, HttpScriptResourceOriginRule, HttpScriptResourcePolicy,
    },
    inline_runner::{DirectPageInlineExecutor, DirectPageScriptExecutionReport},
    javascript::JavaScriptPageExecutor,
    page_source_authorizer::{
        AuthorizedPageScriptGraph, PageScriptSourceAuthorizationError, PageScriptSourceAuthorizer,
        PageScriptSourceRequest,
    },
};
use blueice_bluets::{
    AuthorizedModule, AuthorizedModuleLoader, AuthorizedModuleResolution, CompilerOptions,
    LANGUAGE_VERSION,
};
use std::net::TcpListener;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

fn temp_frame_dir(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "blueice-session-test-{label}-{}",
        std::process::id()
    ))
}

fn client_pair() -> (UnixStream, UnixStream) {
    UnixStream::pair().unwrap()
}

/// A monotonic counter alongside the PID, so every call is unique
/// regardless of how many concurrent tests (each running on its own
/// thread, in this one test binary process) call it -- same
/// discipline `blueice-mcp-server`'s own `unique_socket_path` uses,
/// necessary here because many gatekeeper-behavior tests below each
/// need their own independent fake listener. `_label` exists purely
/// so call sites read self-documenting (`clearing_gatekeeper("foo-
/// test")`) -- deliberately *not* included in the actual path: a
/// Unix domain socket path is capped at ~100 bytes total
/// (`sockaddr_un::sun_path`, tighter on macOS than Linux), and this
/// module's already-long, already-temp-dir-prefixed test names
/// would blow that budget immediately if concatenated in.
fn unique_gatekeeper_socket_path(_label: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("bl-gk-{}-{n}.sock", std::process::id()))
}

/// Spins up a background listener that behaves exactly like `ai-
/// gatekeeper`'s own trivial minimal-slice stub (always clears),
/// bound to a fresh socket path unique to this call. Every existing
/// test below that navigates needs *some* gatekeeper behind the
/// path it gives `run_session` -- not because gating itself is
/// under test there (see the dedicated gatekeeper-behavior tests
/// further down for that), but because a genuinely unreachable
/// gatekeeper fails closed, which would turn those tests'
/// pre-existing "navigation always succeeds" assertions false. This
/// keeps every one of those assertions unmodified.
fn clearing_gatekeeper(label: &str) -> PathBuf {
    let path = unique_gatekeeper_socket_path(label);
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

/// Performs the `protocol_version` handshake `run_session` now
/// requires as the very first message on a fresh connection --
/// every test below drives `run_session` over a brand-new
/// connection, so every one of them needs this before its own
/// message(s), the same way a real client (`frontend`, `blueice-
/// mcp-server`) would via `blueice_ipc::client_handshake`.
fn handshake(client: &mut UnixStream) {
    blueice_ipc::client_handshake(client).unwrap();
}

/// Every test below that predates multi-tab (Phase 16) sets up its
/// fixture content on "the" page, the same single-tab shape it
/// always had -- this is just `tabs.default_tab()` resolved to its
/// `Page`, so those tests don't need to change beyond `Page::new`
/// becoming `TabManager::new`.
fn default_page(tabs: &mut TabManager) -> &mut Page {
    let default = tabs.default_tab();
    tabs.get_mut(default).unwrap()
}

fn admitted_direct_page_host(tabs: &TabManager, tab_id: TabId) -> DirectPageScriptHost {
    let profiles = HostTypeSurfaceCatalogV1::new([HostTypeSurfaceV1::new(
        LANGUAGE_VERSION,
        "session-page-v1",
        "session-empty-v1",
        Vec::new(),
    )])
    .unwrap();
    let artifact = profiles.generate("session-empty-v1").unwrap();
    let mut host = DirectPageScriptHost::new(profiles);
    assert_eq!(
        host.execute_inline(
            tabs,
            DirectInlinePageScriptRequest {
                tab_id,
                ordinal: 0,
                compiler_options: CompilerOptions::default(),
                feature_profile: "session-empty-v1".to_string(),
                supplied_manifest: &artifact.manifest,
                supplied_declaration_source: &artifact.declaration_source,
                supplied_runtime_bindings: &artifact.runtime_bindings,
            },
        )
        .unwrap(),
        blueice_bluejs::Value::Number(42.0)
    );
    host
}

fn inline_page_executor() -> DirectPageInlineExecutor {
    let profiles = HostTypeSurfaceCatalogV1::new([HostTypeSurfaceV1::new(
        LANGUAGE_VERSION,
        "session-inline-runner-v1",
        "session-inline-runner-empty-v1",
        Vec::new(),
    )])
    .unwrap();
    DirectPageInlineExecutor::new(
        profiles,
        "session-inline-runner-empty-v1",
        CompilerOptions::default(),
    )
    .unwrap()
}

struct SessionExternalGraphAuthorizer;

impl PageScriptSourceAuthorizer for SessionExternalGraphAuthorizer {
    fn authorize(
        &mut self,
        request: &PageScriptSourceRequest,
    ) -> Result<AuthorizedPageScriptGraph, PageScriptSourceAuthorizationError> {
        if request.declared_src != "/assets/main.ts" {
            return Err(PageScriptSourceAuthorizationError::new(
                "the test authorizer permits only its expected declaration",
            ));
        }
        let entry = "https://example.test/assets/main.ts";
        let dependency = "https://example.test/assets/value.ts";
        let loader = AuthorizedModuleLoader::new(
            [
                AuthorizedModule::new(
                    entry,
                    "import { value } from './value.ts'; export const answer: number = value + 1; answer;",
                ),
                AuthorizedModule::new(dependency, "export const value: number = 41;"),
            ],
            [AuthorizedModuleResolution::new(entry, "./value.ts", dependency)],
        )
        .map_err(|error| PageScriptSourceAuthorizationError::new(error.to_string()))?;
        AuthorizedPageScriptGraph::new(entry, loader, "session-external-policy-v1")
            .map_err(|error| PageScriptSourceAuthorizationError::new(error.to_string()))
    }
}

fn external_graph_page_executor() -> DirectPageInlineExecutor {
    let profiles = HostTypeSurfaceCatalogV1::new([HostTypeSurfaceV1::new(
        LANGUAGE_VERSION,
        "session-external-runner-v1",
        "session-external-runner-empty-v1",
        Vec::new(),
    )])
    .unwrap();
    DirectPageInlineExecutor::with_external_source_authorizer(
        profiles,
        "session-external-runner-empty-v1",
        CompilerOptions::default(),
        SessionExternalGraphAuthorizer,
    )
    .unwrap()
}

fn http_external_graph_page_executor(
    authorizer: HttpOutOfProcessPageScriptSourceAuthorizer,
) -> DirectPageInlineExecutor {
    let profiles = HostTypeSurfaceCatalogV1::new([HostTypeSurfaceV1::new(
        LANGUAGE_VERSION,
        "session-http-external-runner-v1",
        "session-http-external-runner-empty-v1",
        Vec::new(),
    )])
    .unwrap();
    DirectPageInlineExecutor::with_external_source_authorizer(
        profiles,
        "session-http-external-runner-empty-v1",
        CompilerOptions::default(),
        authorizer,
    )
    .unwrap()
}

#[test]
fn inline_blue_ts_scripts_execute_after_a_real_session_navigation() {
    let dir = temp_frame_dir("inline-blue-ts-page-pipeline");
    std::fs::create_dir_all(&dir).unwrap();
    let gatekeeper = clearing_gatekeeper("inline-blue-ts-page-pipeline");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 1024];
        let _ = std::io::Read::read(&mut stream, &mut request);
        let body = concat!(
            "<script type=\"application/x-blueice-typescript\">42;</script>",
            "<script type=\"application/x-blueice-typescript-module\">43;</script>"
        );
        std::io::Write::write_all(
            &mut stream,
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .unwrap();
    });

    let (mut client, mut server) = client_pair();
    let dir_for_thread = dir.clone();
    let (result_sender, result_receiver) = mpsc::sync_channel(1);
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0;
        let mut executor = inline_page_executor();
        run_session_with_script_requests_and_inline_page_executor(
            &mut tabs,
            &mut server,
            &dir_for_thread,
            &mut generation,
            &gatekeeper,
            None,
            Some(&mut executor),
        )
        .unwrap();
        result_sender
            .send((executor.debug_record_count(), executor.drain_reports()))
            .unwrap();
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: format!("http://{addr}"),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Navigated { .. }
    ));
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();

    handle.join().unwrap();
    let (debug_record_count, reports) = result_receiver.recv().unwrap();
    assert_eq!(debug_record_count, 2);
    assert!(matches!(
        reports.first(),
        Some(DirectPageScriptExecutionReport::Executed { ordinal: 0, .. })
    ));
    assert!(matches!(
        reports.last(),
        Some(DirectPageScriptExecutionReport::Executed { ordinal: 1, .. })
    ));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn script_reports_query_does_not_enable_the_default_session_executor() {
    let dir = temp_frame_dir("inline-blue-ts-reports-disabled");
    let (mut client, mut server) = client_pair();
    let dir_for_thread = dir.clone();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0;
        run_session(
            &mut tabs,
            &mut server,
            &dir_for_thread,
            &mut generation,
            std::path::Path::new("/not-used-without-navigation"),
        )
        .unwrap();
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetBlueTsScriptReports).unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Error {
            message: "inline BlueTS execution is not enabled".to_string(),
        }
    );
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetBlueJsScriptReports).unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Error {
            message: "inline JavaScript execution is not enabled".to_string(),
        }
    );
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();

    handle.join().unwrap();
}

#[test]
fn inline_javascript_scripts_execute_after_a_real_session_navigation() {
    let dir = temp_frame_dir("inline-javascript-page-pipeline");
    std::fs::create_dir_all(&dir).unwrap();
    let gatekeeper = clearing_gatekeeper("inline-javascript-page-pipeline");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 1024];
        let _ = std::io::Read::read(&mut stream, &mut request);
        let body = concat!(
            "<script>const classicAnswer = 42;</script>",
            "<script type=\"module\">export const moduleAnswer = 43;</script>",
            "<script>const = malformed;</script>"
        );
        std::io::Write::write_all(
            &mut stream,
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .unwrap();
    });

    let (mut client, mut server) = client_pair();
    let dir_for_thread = dir.clone();
    let gatekeeper_for_thread = gatekeeper.clone();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0;
        let mut executor = JavaScriptPageExecutor::default();
        run_session_with_script_and_debugger_requests_and_inline_javascript_executor(
            &mut tabs,
            &mut server,
            &dir_for_thread,
            &mut generation,
            &gatekeeper_for_thread,
            CoreSessionRequests::default(),
            Some(&mut executor),
        )
        .unwrap();
    });
    handshake(&mut client);
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: format!("http://{address}"),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Navigated { .. }
    ));
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetBlueJsScriptReports).unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::BlueJsScriptReports(vec![
            blueice_ipc::BlueJsScriptExecutionReport {
                tab_id: 1,
                document_generation: 1,
                ordinal: 0,
                kind: blueice_ipc::BlueJsScriptKind::Classic,
                outcome: blueice_ipc::BlueJsScriptExecutionOutcome::Executed,
            },
            blueice_ipc::BlueJsScriptExecutionReport {
                tab_id: 1,
                document_generation: 1,
                ordinal: 1,
                kind: blueice_ipc::BlueJsScriptKind::Module,
                outcome: blueice_ipc::BlueJsScriptExecutionOutcome::Executed,
            },
            blueice_ipc::BlueJsScriptExecutionReport {
                tab_id: 1,
                document_generation: 1,
                ordinal: 2,
                kind: blueice_ipc::BlueJsScriptKind::Classic,
                outcome: blueice_ipc::BlueJsScriptExecutionOutcome::Rejected {
                    category: "JavaScript parsing rejected the page script".to_string(),
                },
            },
        ])
    );
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();

    handle.join().unwrap();
    let _ = std::fs::remove_file(gatekeeper);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn inline_javascript_reports_remain_isolated_by_tab_after_real_session_navigation() {
    // The report drain is a tab-addressed observation boundary. Drive two
    // ordinary JavaScript documents through the live session and prove that
    // draining tab one cannot disclose or discard tab two's outcome.
    let dir = temp_frame_dir("inline-javascript-tab-isolation");
    std::fs::create_dir_all(&dir).unwrap();
    let gatekeeper = clearing_gatekeeper("inline-javascript-tab-isolation");

    let first_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let first_address = first_listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = first_listener.accept().unwrap();
        let mut request = [0u8; 1024];
        let _ = std::io::Read::read(&mut stream, &mut request);
        let body = "<main>first JavaScript tab</main><script>41 + 1;</script>";
        std::io::Write::write_all(
            &mut stream,
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .unwrap();
    });

    let second_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let second_address = second_listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = second_listener.accept().unwrap();
        let mut request = [0u8; 1024];
        let _ = std::io::Read::read(&mut stream, &mut request);
        let body = "<main>second JavaScript tab</main><script>40 + 3;</script>";
        std::io::Write::write_all(
            &mut stream,
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .unwrap();
    });

    let (mut client, mut server) = client_pair();
    let dir_for_thread = dir.clone();
    let gatekeeper_for_thread = gatekeeper.clone();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0;
        let mut executor = JavaScriptPageExecutor::default();
        run_session_with_script_and_debugger_requests_and_inline_javascript_executor(
            &mut tabs,
            &mut server,
            &dir_for_thread,
            &mut generation,
            &gatekeeper_for_thread,
            CoreSessionRequests::default(),
            Some(&mut executor),
        )
        .unwrap();
    });

    handshake(&mut client);
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: format!("http://{first_address}"),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Navigated { .. }
    ));
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::OpenTab {
            url: Some(format!("http://{second_address}")),
        },
    )
    .unwrap();
    let second_tab = match blueice_ipc::read_server_message(&mut client).unwrap() {
        ServerMessage::TabOpened { tab_id, .. } => tab_id,
        other => panic!("expected TabOpened, got {other:?}"),
    };
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetBlueJsScriptReports).unwrap();
    let (reply_tab, _, first_reports) =
        blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert_eq!(reply_tab, Some(1));
    assert_eq!(
        first_reports,
        ServerMessage::BlueJsScriptReports(vec![blueice_ipc::BlueJsScriptExecutionReport {
            tab_id: 1,
            document_generation: 1,
            ordinal: 0,
            kind: blueice_ipc::BlueJsScriptKind::Classic,
            outcome: blueice_ipc::BlueJsScriptExecutionOutcome::Executed,
        }])
    );

    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(second_tab),
        None,
        &ClientMessage::GetBlueJsScriptReports,
    )
    .unwrap();
    let (reply_tab, _, second_reports) =
        blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert_eq!(reply_tab, Some(second_tab));
    assert_eq!(
        second_reports,
        ServerMessage::BlueJsScriptReports(vec![blueice_ipc::BlueJsScriptExecutionReport {
            tab_id: second_tab,
            document_generation: 1,
            ordinal: 0,
            kind: blueice_ipc::BlueJsScriptKind::Classic,
            outcome: blueice_ipc::BlueJsScriptExecutionOutcome::Executed,
        }])
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    handle.join().unwrap();
    let _ = std::fs::remove_file(gatekeeper);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn authorized_external_blue_ts_module_graph_executes_after_a_real_session_navigation() {
    let dir = temp_frame_dir("external-blue-ts-page-pipeline");
    std::fs::create_dir_all(&dir).unwrap();
    let gatekeeper = clearing_gatekeeper("external-blue-ts-page-pipeline");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 1024];
        let _ = std::io::Read::read(&mut stream, &mut request);
        let body = "<script type=\"application/x-blueice-typescript-module\" src=\"/assets/main.ts\"></script>";
        std::io::Write::write_all(
            &mut stream,
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .unwrap();
    });

    let (mut client, mut server) = client_pair();
    let dir_for_thread = dir.clone();
    let (result_sender, result_receiver) = mpsc::sync_channel(1);
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0;
        let mut executor = external_graph_page_executor();
        run_session_with_script_requests_and_inline_page_executor(
            &mut tabs,
            &mut server,
            &dir_for_thread,
            &mut generation,
            &gatekeeper,
            None,
            Some(&mut executor),
        )
        .unwrap();
        result_sender
            .send((executor.debug_record_count(), executor.drain_reports()))
            .unwrap();
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: format!("http://{addr}"),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Navigated { .. }
    ));
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();

    handle.join().unwrap();
    let (debug_record_count, reports) = result_receiver.recv().unwrap();
    assert_eq!(debug_record_count, 2);
    assert!(matches!(
        reports.as_slice(),
        [DirectPageScriptExecutionReport::Executed {
            ordinal: 0,
            kind: crate::script::direct_page::DirectPageScriptKind::Module,
            ..
        }]
    ));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn http_manifest_authorizes_an_external_bluets_graph_after_a_real_session_navigation() {
    let dir = temp_frame_dir("http-authorized-external-blue-ts-page-pipeline");
    std::fs::create_dir_all(&dir).unwrap();
    let gatekeeper = clearing_gatekeeper("http-authorized-external-blue-ts-page-pipeline");
    let source = "export const answer: number = 42;";
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let entry = format!("http://{address}/assets/main.ts");
    let policy = HttpScriptResourcePolicy::new(
        HttpScriptResourceOriginRule::same_document_origin(),
        HttpScriptIntegrityManifest::new([(entry, sha256_integrity(source.as_bytes()))]).unwrap(),
        HttpScriptResourceLimits::default(),
    )
    .unwrap();
    let server = thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 1024];
            let request_len = std::io::Read::read(&mut stream, &mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..request_len]);
            let (content_type, body) = if request.starts_with("GET /assets/main.ts ") {
                ("application/typescript", source)
            } else {
                (
                    "text/html",
                    "<script type=\"application/x-blueice-typescript-module\" src=\"/assets/main.ts\"></script>",
                )
            };
            std::io::Write::write_all(
                &mut stream,
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
        }
    });

    let (mut client, mut server_stream) = client_pair();
    let dir_for_thread = dir.clone();
    let (result_sender, result_receiver) = mpsc::sync_channel(1);
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0;
        let mut executor = http_external_graph_page_executor(
            HttpOutOfProcessPageScriptSourceAuthorizer::new(policy),
        );
        run_session_with_script_requests_and_inline_page_executor(
            &mut tabs,
            &mut server_stream,
            &dir_for_thread,
            &mut generation,
            &gatekeeper,
            None,
            Some(&mut executor),
        )
        .unwrap();
        result_sender
            .send((executor.debug_record_count(), executor.drain_reports()))
            .unwrap();
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: format!("http://{address}/app/index.html"),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Navigated { .. }
    ));
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();

    handle.join().unwrap();
    server.join().unwrap();
    let (debug_record_count, reports) = result_receiver.recv().unwrap();
    assert_eq!(debug_record_count, 1);
    assert!(matches!(
        reports.as_slice(),
        [DirectPageScriptExecutionReport::Executed {
            ordinal: 0,
            kind: crate::script::direct_page::DirectPageScriptKind::Module,
            ..
        }]
    ));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn direct_page_host_is_invalidated_by_a_session_document_replacement() {
    let dir = temp_frame_dir("direct-page-lifecycle");
    std::fs::create_dir_all(&dir).unwrap();
    let (mut client, mut server) = client_pair();
    let client_task = thread::spawn(move || {
        handshake(&mut client);
        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::Navigate {
                url: "about:blank".to_string(),
            },
        )
        .unwrap();
        assert!(matches!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::Navigated { .. }
        ));
        assert!(matches!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::FrameReady { .. }
        ));
        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    });

    let mut tabs = TabManager::new(320.0, 200.0);
    let tab_id = tabs.default_tab();
    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<main id=\"app\"></main><script type=\"application/x-blueice-typescript\">42;</script>",
        Some("https://example.test/app/index.html".to_string()),
    );
    let mut direct_page_host = admitted_direct_page_host(&tabs, tab_id);
    assert_eq!(direct_page_host.debug_record_count(), 1);
    let mut generation = 0;
    let unused_gatekeeper = unique_gatekeeper_socket_path("direct-page-lifecycle");
    run_session_with_script_requests_and_direct_page_host(
        &mut tabs,
        &mut server,
        &dir,
        &mut generation,
        &unused_gatekeeper,
        None,
        Some(&mut direct_page_host),
    )
    .unwrap();
    client_task.join().unwrap();

    assert_eq!(direct_page_host.debug_record_count(), 0);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn synchronous_navigation_paths_supersede_pending_navigation_sequences() {
    let dir = temp_frame_dir("sync-navigation-supersedes");
    std::fs::create_dir_all(&dir).unwrap();
    let mut page = Page::new(320.0, 200.0);
    let mut stream = Vec::new();
    let mut generation = 0;
    let tab = TabId::from_u64(7);
    let mut pending_nav_seq = HashMap::from([(tab, 41)]);
    let (completion_tx, _completion_rx) = mpsc::channel();
    let unused_gatekeeper = unique_gatekeeper_socket_path("sync-navigation-supersedes");

    begin_gated_navigation(
        &mut page,
        &mut stream,
        &dir,
        &mut generation,
        Some(tab.as_u64()),
        None,
        tab,
        "about:blank".to_string(),
        PendingKind::Navigate,
        &mut pending_nav_seq,
        &completion_tx,
        &unused_gatekeeper,
    )
    .unwrap();
    assert_eq!(pending_nav_seq[&tab], 42);

    begin_gated_navigation(
        &mut page,
        &mut stream,
        &dir,
        &mut generation,
        Some(tab.as_u64()),
        None,
        tab,
        "file:///not-allowed".to_string(),
        PendingKind::Navigate,
        &mut pending_nav_seq,
        &completion_tx,
        &unused_gatekeeper,
    )
    .unwrap();
    assert_eq!(pending_nav_seq[&tab], 43);

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn resize_then_shutdown_produces_one_frame_and_then_ends_the_session() {
    let dir = temp_frame_dir("resize");
    let gatekeeper = clearing_gatekeeper("resize");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str("<p>hi</p>", None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Resize {
            width: 100,
            height: 50,
        },
    )
    .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(
        reply,
        ServerMessage::FrameReady {
            generation: 1,
            width: 100,
            height: 50,
            ..
        }
    ));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn navigate_replies_with_navigated_then_a_frame_reflecting_the_new_page() {
    let dir = temp_frame_dir("navigate");
    let gatekeeper = clearing_gatekeeper("navigate");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = std::io::Read::read(&mut stream, &mut buf);
        let body = "<p>fetched page</p>";
        std::io::Write::write_all(
            &mut stream,
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .unwrap();
    });

    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    let url = format!("http://{addr}");
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Navigate { url: url.clone() })
        .unwrap();
    let navigated = blueice_ipc::read_server_message(&mut client).unwrap();
    assert_eq!(navigated, ServerMessage::Navigated { url });
    let frame = blueice_ipc::read_server_message(&mut client).unwrap();
    let shm_path = match frame {
        ServerMessage::FrameReady {
            shm_path,
            generation: 1,
            ..
        } => shm_path,
        other => panic!("expected FrameReady, got {other:?}"),
    };
    assert!(
        shm::map_frame(std::path::Path::new(&shm_path)).is_ok(),
        "the frame-plane file must actually exist and be mappable"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn navigate_to_an_unreachable_host_replies_with_error_not_a_frame() {
    let dir = temp_frame_dir("navigate-error");
    let gatekeeper = clearing_gatekeeper("navigate-error");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "not-a-valid-url".to_string(),
        },
    )
    .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(reply, ServerMessage::Error { .. }));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn click_on_a_link_navigates_and_a_click_elsewhere_produces_no_reply() {
    let dir = temp_frame_dir("click");
    let gatekeeper = clearing_gatekeeper("click");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = std::io::Read::read(&mut stream, &mut buf);
        let body = "<p>landed</p>";
        std::io::Write::write_all(
            &mut stream,
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .unwrap();
    });
    let url = format!("http://{addr}");

    let (mut client, mut server) = client_pair();
    let dir_for_thread = dir.clone();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str(&format!(r#"<a href="{url}">go</a>"#), None);
        let mut generation = 0u64;
        run_session(
            &mut tabs,
            &mut server,
            &dir_for_thread,
            &mut generation,
            &gatekeeper,
        )
        .unwrap();
    });
    handshake(&mut client);

    // clicking the link navigates: expect Navigated then FrameReady
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Click { x: 2.0, y: 2.0 })
        .unwrap();
    let navigated = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(navigated, ServerMessage::Navigated { .. }));
    let frame = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(frame, ServerMessage::FrameReady { .. }));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn set_visible_produces_no_reply_and_the_session_keeps_running() {
    let dir = temp_frame_dir("visible");
    let gatekeeper = clearing_gatekeeper("visible");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Chrome(blueice_ipc::ChromeCommand::SetVisible(false)),
    )
    .unwrap();
    // proven by the fact that a subsequent message still gets a
    // normal reply -- Chrome(SetVisible) didn't wedge or end the session.
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Resize {
            width: 10,
            height: 10,
        },
    )
    .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(reply, ServerMessage::FrameReady { .. }));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn get_representation_shares_the_current_generation_across_every_send_frame_call_site() {
    // `get_representation_shares_the_current_generation_with_the_last_frame`
    // below proves the "same render pass" invariant for `Resize`
    // alone; this extends the same proof to `Scroll`, `Highlight`,
    // and a non-navigating `ActOn` (`Focus`) -- the other distinct
    // `send_frame` call sites in `run_session` (`Click`/`ActOn`'s
    // Click variant only ever reach `send_frame` via the same
    // navigate path `Navigate` itself already exercises, so they add
    // no new coverage here). `send_frame` is a single choke point
    // every one of these routes through, so this is expected to
    // hold structurally -- but the invariant is central enough to
    // this project's premise to prove per call site, not infer from
    // one example.
    let dir = temp_frame_dir("representation-generation-all-sites");
    let gatekeeper = clearing_gatekeeper("representation-generation-all-sites");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str(r#"<input type="text">"#, None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(snap) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    let input_id = snap.nodes[0].id;

    let assert_matching_generation = |client: &mut UnixStream, send: ClientMessage| {
        blueice_ipc::write_client_message(client, &send).unwrap();
        let frame = blueice_ipc::read_server_message(client).unwrap();
        let ServerMessage::FrameReady {
            generation: frame_generation,
            ..
        } = frame
        else {
            panic!("expected FrameReady, got {frame:?}")
        };

        blueice_ipc::write_client_message(client, &ClientMessage::GetRepresentation).unwrap();
        let reply = blueice_ipc::read_server_message(client).unwrap();
        let ServerMessage::Representation(snapshot) = reply else {
            panic!("expected Representation, got {reply:?}")
        };
        assert_eq!(snapshot.generation, frame_generation);
    };

    assert_matching_generation(&mut client, ClientMessage::Scroll { delta_y: 10.0 });
    assert_matching_generation(&mut client, ClientMessage::Highlight { id: Some(input_id) });
    assert_matching_generation(
        &mut client,
        ClientMessage::ActOn {
            id: input_id,
            action: NodeAction::Focus,
        },
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn get_representation_shares_the_current_generation_with_the_last_frame() {
    // the concrete, checkable "same render pass" proof
    // `phase-5-ai-representation-output/PLAN.md` asks for: a
    // Representation and the FrameReady sent alongside a prior
    // state change carry the identical generation number.
    let dir = temp_frame_dir("representation-generation");
    let gatekeeper = clearing_gatekeeper("representation-generation");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str(r#"<a href="/x">Go</a>"#, None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Resize {
            width: 100,
            height: 50,
        },
    )
    .unwrap();
    let frame = blueice_ipc::read_server_message(&mut client).unwrap();
    let ServerMessage::FrameReady {
        generation: frame_generation,
        ..
    } = frame
    else {
        panic!("expected FrameReady, got {frame:?}")
    };

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    let ServerMessage::Representation(snapshot) = reply else {
        panic!("expected Representation, got {reply:?}")
    };
    assert_eq!(snapshot.generation, frame_generation);
    assert!(snapshot
        .nodes
        .iter()
        .any(|n| n.name.as_deref() == Some("Go")));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn get_dom_returns_the_full_tree_unfiltered_by_the_ai_representation() {
    let dir = temp_frame_dir("get-dom");
    let gatekeeper = clearing_gatekeeper("get-dom");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs)
            .load_html_str(r#"<div style="background-color: red;">x</div>"#, None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetDom).unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    let ServerMessage::Dom(dump) = reply else {
        panic!("expected Dom, got {reply:?}")
    };
    assert!(dump.contains("<div>"), "a bare div has no AI-representation role but must still appear in the full DOM dump: {dump}");

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[path = "tests/navigation.rs"]
mod navigation;
#[path = "tests/session_actions.rs"]
mod session_actions;
