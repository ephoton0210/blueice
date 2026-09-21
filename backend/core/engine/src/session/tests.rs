// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use crate::script::{
    direct_page::{DirectInlinePageScriptRequest, DirectPageScriptHost},
    host_typings::{HostTypeSurfaceCatalogV1, HostTypeSurfaceV1},
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

#[test]
fn act_on_click_navigates_the_same_way_a_coordinate_click_does() {
    let dir = temp_frame_dir("act-on-click");
    let gatekeeper = clearing_gatekeeper("act-on-click");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = std::io::Read::read(&mut stream, &mut buf);
        let body = "<p>landed via id</p>";
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
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str(&format!(r#"<a href="{url}">go</a>"#), None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(snapshot) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    let link_id = snapshot
        .nodes
        .iter()
        .find(|n| n.name.as_deref() == Some("go"))
        .unwrap()
        .id;

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::ActOn {
            id: link_id,
            action: NodeAction::Click,
        },
    )
    .unwrap();
    let navigated = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(navigated, ServerMessage::Navigated { .. }));
    let frame = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(frame, ServerMessage::FrameReady { .. }));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn act_on_focus_is_reflected_in_the_next_representation() {
    let dir = temp_frame_dir("act-on-focus");
    let gatekeeper = clearing_gatekeeper("act-on-focus");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs)
            .load_html_str(r#"<input id="name" type="text" placeholder="Name">"#, None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(before) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    let input_id = before.nodes[0].id;
    assert!(!before.nodes[0].state.focused);

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::ActOn {
            id: input_id,
            action: NodeAction::Focus,
        },
    )
    .unwrap();
    let frame = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(
        matches!(frame, ServerMessage::FrameReady { .. }),
        "Focus is a state change and still gets a FrameReady, per session.rs's own docs"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(after) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    assert!(after.nodes[0].state.focused);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn act_on_an_unknown_id_is_a_harmless_no_op() {
    let dir = temp_frame_dir("act-on-unknown");
    let gatekeeper = clearing_gatekeeper("act-on-unknown");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str(r#"<a href="/x">go</a>"#, None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    // an unknown id with Click: same "no reply at all" contract as
    // a coordinate click that lands on nothing.
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::ActOn {
            id: 999_999,
            action: NodeAction::Click,
        },
    )
    .unwrap();
    // proven by the fact that the next message still gets a normal
    // reply -- the unknown id didn't wedge or end the session.
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
fn a_stale_id_from_before_a_navigation_is_a_harmless_no_op_after_it() {
    // Unlike `act_on_an_unknown_id_is_a_harmless_no_op` (a
    // never-allocated id), this id is real -- it existed in the
    // document *before* the navigation below. Regression: NodeId
    // allocation used to restart at 0 for every freshly-parsed
    // document, so this same numeric id could be reused by an
    // unrelated node in the post-navigation document, and ActOn
    // would silently act on that unrelated node instead of safely
    // no-op'ing.
    let dir = temp_frame_dir("stale-id-across-navigation");
    let gatekeeper = clearing_gatekeeper("stale-id-across-navigation");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str(r#"<a href="/x">go</a>"#, None);
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
    let stale_id = snap.nodes[0].id;

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "about:blank".to_string(),
        },
    )
    .unwrap();
    let navigated = blueice_ipc::read_server_message(&mut client).unwrap();
    assert_eq!(
        navigated,
        ServerMessage::Navigated {
            url: "about:blank".to_string()
        }
    );
    let _frame = blueice_ipc::read_server_message(&mut client).unwrap();

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::ActOn {
            id: stale_id,
            action: NodeAction::Click,
        },
    )
    .unwrap();
    // proven the same way as the never-allocated-id case: the next
    // message still gets a normal reply, so the stale id neither
    // wedged the session nor triggered a misdirected action.
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
fn highlight_adds_an_outline_to_the_next_frame_and_clearing_it_removes_it() {
    let dir = temp_frame_dir("highlight");
    let gatekeeper = clearing_gatekeeper("highlight");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str(r#"<a href="/x">go</a>"#, None);
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
    let link_id = snap.nodes[0].id;

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Highlight { id: Some(link_id) })
        .unwrap();
    let frame = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(frame, ServerMessage::FrameReady { .. }));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn hover_updates_state_silently_with_no_reply() {
    let dir = temp_frame_dir("hover");
    let gatekeeper = clearing_gatekeeper("hover");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str(r#"<a href="/x">go</a>"#, None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Hover { x: 2.0, y: 2.0 })
        .unwrap();
    // proven the same way SetVisible/Chrome is: the next message
    // still gets a normal reply, so Hover didn't wedge the session.
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(snap) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    assert!(
        snap.nodes[0].state.hovered,
        "the hovered state must be visible via GetRepresentation"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn chrome_set_visible_does_not_change_engine_render_state() {
    // `phase-5-ai-representation-output/PLAN.md`'s "verify engine
    // state is unchanged across a hide/show cycle" checklist item,
    // made explicit and checkable rather than left implicit in
    // `Chrome`'s no-op handling: a full hide-then-show round trip
    // must leave the representation (and therefore the DOM/styles/
    // fragment tree it's derived from) byte-for-byte identical, and
    // must not cause a new frame to be rendered.
    let dir = temp_frame_dir("chrome-no-restart");
    let gatekeeper = clearing_gatekeeper("chrome-no-restart");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str("<p>hi</p>", None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(before) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Chrome(blueice_ipc::ChromeCommand::SetVisible(false)),
    )
    .unwrap();
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Chrome(blueice_ipc::ChromeCommand::SetVisible(true)),
    )
    .unwrap();

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(after) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };

    assert_eq!(
        before.nodes, after.nodes,
        "a hide/show cycle must not change the engine's render-pass state"
    );
    assert_eq!(
        before.generation, after.generation,
        "no frame is re-rendered just from a visibility toggle"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn disconnecting_without_shutdown_ends_the_session_cleanly() {
    let dir = temp_frame_dir("disconnect");
    let gatekeeper = unique_gatekeeper_socket_path("disconnect"); // never dialed
    let (client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper)
    });
    drop(client);
    assert!(handle.join().unwrap().is_ok());
}

#[test]
fn a_first_message_that_is_not_hello_is_rejected_and_ends_the_session() {
    let dir = temp_frame_dir("handshake-not-hello-first");
    let gatekeeper = unique_gatekeeper_socket_path("handshake-not-hello-first"); // never dialed
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper)
    });

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(
        matches!(reply, ServerMessage::Error { .. }),
        "expected an Error reply, got {reply:?}"
    );

    assert!(
        handle.join().unwrap().is_ok(),
        "the session must end cleanly, not hang, after rejecting the handshake"
    );
}

#[test]
fn an_unsupported_protocol_version_is_rejected_and_ends_the_session() {
    let dir = temp_frame_dir("handshake-bad-version");
    let gatekeeper = unique_gatekeeper_socket_path("handshake-bad-version"); // never dialed
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper)
    });

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Hello {
            protocol_version: blueice_ipc::PROTOCOL_VERSION + 1,
        },
    )
    .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(
        matches!(reply, ServerMessage::Error { .. }),
        "expected an Error reply, got {reply:?}"
    );

    assert!(
        handle.join().unwrap().is_ok(),
        "the session must end cleanly, not hang, after rejecting an unsupported version"
    );
}

#[test]
fn a_hello_seen_again_after_the_handshake_is_answered_without_ending_the_session() {
    // The broker-multiplexing scenario `run_session`'s own docs
    // describe: a second external client's handshake, forwarded
    // into the one already-past-its-own-handshake shared
    // connection, must not be treated as a protocol violation.
    let dir = temp_frame_dir("late-hello");
    let gatekeeper = clearing_gatekeeper("late-hello");
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
        &ClientMessage::Hello {
            protocol_version: blueice_ipc::PROTOCOL_VERSION,
        },
    )
    .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert_eq!(
        reply,
        ServerMessage::Hello {
            protocol_version: blueice_ipc::PROTOCOL_VERSION
        }
    );

    // proven the same way other no-special-effect messages are:
    // the session is still alive and answers normally afterward.
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn every_reply_to_a_message_echoes_back_its_request_id() {
    let dir = temp_frame_dir("request-id-echo");
    let gatekeeper = clearing_gatekeeper("request-id-echo");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str("<p>hi</p>", None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message_with_id(
        &mut client,
        Some(99),
        &ClientMessage::GetRepresentation,
    )
    .unwrap();
    let (request_id, reply) = blueice_ipc::read_server_message_with_id(&mut client).unwrap();
    assert_eq!(request_id, Some(99));
    assert!(matches!(reply, ServerMessage::Representation(_)));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_unknown_client_variant_is_ignored_and_the_session_keeps_running() {
    let dir = temp_frame_dir("unknown-variant");
    let gatekeeper = clearing_gatekeeper("unknown-variant");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Unknown).unwrap();
    // proven the same way other no-reply messages are: the next
    // message still gets a normal reply, so Unknown didn't wedge
    // or end the session.
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
fn open_tab_creates_a_second_tab_visible_in_list_tabs() {
    let dir = temp_frame_dir("open-tab-list");
    let gatekeeper = clearing_gatekeeper("open-tab-list");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
    let ServerMessage::Tabs(before) = blueice_ipc::read_server_message(&mut client).unwrap() else {
        panic!("expected Tabs")
    };
    assert_eq!(
        before.len(),
        1,
        "a fresh core starts with exactly one tab, same as before Phase 16"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::OpenTab { url: None }).unwrap();
    let ServerMessage::TabOpened {
        tab_id: new_id,
        url,
    } = blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected TabOpened")
    };
    assert_eq!(url, None);
    assert_ne!(new_id, before[0].id);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
    let ServerMessage::Tabs(after) = blueice_ipc::read_server_message(&mut client).unwrap() else {
        panic!("expected Tabs")
    };
    assert_eq!(
        after.iter().map(|t| t.id).collect::<Vec<_>>(),
        vec![before[0].id, new_id]
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn open_tab_with_a_url_navigates_it_and_sends_a_frame() {
    let dir = temp_frame_dir("open-tab-with-url");
    let gatekeeper = clearing_gatekeeper("open-tab-with-url");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = std::io::Read::read(&mut stream, &mut buf);
        let body = "<p>opened via url</p>";
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
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::OpenTab {
            url: Some(url.clone()),
        },
    )
    .unwrap();
    let ServerMessage::TabOpened {
        tab_id: new_id,
        url: opened_url,
    } = blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected TabOpened")
    };
    assert_eq!(opened_url, Some(url));
    let frame = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(
        matches!(frame, ServerMessage::FrameReady { .. }),
        "expected FrameReady, got {frame:?}"
    );

    // The new tab's content must actually be addressable afterward.
    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(new_id),
        None,
        &ClientMessage::GetRepresentation,
    )
    .unwrap();
    let (reply_tab, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert_eq!(reply_tab, Some(new_id));
    let ServerMessage::Representation(snapshot) = reply else {
        panic!("expected Representation, got {reply:?}")
    };
    assert_eq!(snapshot.tab_id, new_id);
    assert!(snapshot
        .nodes
        .iter()
        .any(|n| n.name.as_deref() == Some("opened via url")));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn open_tab_with_a_failing_url_replies_error_not_tab_opened() {
    let dir = temp_frame_dir("open-tab-failing-url");
    let gatekeeper = clearing_gatekeeper("open-tab-failing-url");
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
        &ClientMessage::OpenTab {
            url: Some("not-a-valid-url".to_string()),
        },
    )
    .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(
        matches!(reply, ServerMessage::Error { .. }),
        "expected Error, got {reply:?}"
    );

    // The session must still be alive and taking new commands
    // afterward -- proven the same way every other no-crash case
    // in this file is.
    blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Tabs(_)
    ));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_action_addressed_to_one_tab_never_affects_another_tabs_state() {
    let dir = temp_frame_dir("tab-isolation");
    let gatekeeper = clearing_gatekeeper("tab-isolation");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str("<p>tab one</p>", None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
    let ServerMessage::Tabs(initial) = blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Tabs")
    };
    let tab_one = initial[0].id;

    blueice_ipc::write_client_message(&mut client, &ClientMessage::OpenTab { url: None }).unwrap();
    let ServerMessage::TabOpened {
        tab_id: tab_two, ..
    } = blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected TabOpened")
    };

    // Scroll only tab_two.
    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(tab_two),
        None,
        &ClientMessage::Scroll { delta_y: 500.0 },
    )
    .unwrap();
    let frame = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(frame, ServerMessage::FrameReady { .. }));

    // tab_one's representation must be completely unaffected --
    // still showing its own content, scroll untouched.
    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(tab_one),
        None,
        &ClientMessage::GetRepresentation,
    )
    .unwrap();
    let (_, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    let ServerMessage::Representation(snap) = reply else {
        panic!("expected Representation, got {reply:?}")
    };
    assert_eq!(snap.tab_id, tab_one);
    assert_eq!(
        snap.scroll_y, 0.0,
        "scrolling tab_two must not move tab_one's scroll position"
    );
    assert!(snap
        .nodes
        .iter()
        .any(|n| n.name.as_deref() == Some("tab one")));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_gated_navigation_addressed_to_one_tab_never_affects_another_tabs_state() {
    // Extends `an_action_addressed_to_one_tab_never_affects_another_
    // tabs_state` (which only covers `Scroll`) to a gated `Navigate`
    // specifically, now that navigation is asynchronous: `tab_two`
    // fully navigating must leave `tab_one`'s content, generation
    // relationship, and addressability completely untouched.
    let dir = temp_frame_dir("tab-isolation-gated-navigate");
    let gatekeeper = clearing_gatekeeper("tab-isolation-gated-navigate");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = std::io::Read::read(&mut stream, &mut buf);
        let body = "<p>tab two content</p>";
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
        default_page(&mut tabs).load_html_str("<p>tab one</p>", None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
    let ServerMessage::Tabs(initial) = blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Tabs")
    };
    let tab_one = initial[0].id;

    blueice_ipc::write_client_message(&mut client, &ClientMessage::OpenTab { url: None }).unwrap();
    let ServerMessage::TabOpened {
        tab_id: tab_two, ..
    } = blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected TabOpened")
    };

    let url = format!("http://{addr}");
    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(tab_two),
        None,
        &ClientMessage::Navigate { url: url.clone() },
    )
    .unwrap();
    let (reply_tab, _, navigated) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert_eq!(reply_tab, Some(tab_two));
    assert_eq!(navigated, ServerMessage::Navigated { url });
    let (_, _, frame) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert!(matches!(frame, ServerMessage::FrameReady { .. }));

    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(tab_one),
        None,
        &ClientMessage::GetRepresentation,
    )
    .unwrap();
    let (_, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    let ServerMessage::Representation(snap) = reply else {
        panic!("expected Representation, got {reply:?}")
    };
    assert_eq!(snap.tab_id, tab_one);
    assert!(
        snap.nodes
            .iter()
            .any(|n| n.name.as_deref() == Some("tab one")),
        "tab one's content must be untouched by tab two's gated navigation"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_message_addressed_to_an_unknown_tab_replies_error_not_a_silent_no_op() {
    let dir = temp_frame_dir("unknown-tab-error");
    let gatekeeper = clearing_gatekeeper("unknown-tab-error");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(999_999),
        None,
        &ClientMessage::GetRepresentation,
    )
    .unwrap();
    let (reply_tab, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert_eq!(
        reply_tab,
        Some(999_999),
        "the reply should still echo back which (nonexistent) tab was addressed"
    );
    assert!(
        matches!(reply, ServerMessage::Error { .. }),
        "expected Error, got {reply:?}"
    );

    // The session must survive an unknown-tab error, same as every
    // other error case in this file.
    blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Tabs(_)
    ));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn close_tab_removes_it_and_a_later_message_to_it_becomes_an_error() {
    let dir = temp_frame_dir("close-tab");
    let gatekeeper = clearing_gatekeeper("close-tab");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::OpenTab { url: None }).unwrap();
    let ServerMessage::TabOpened { tab_id: new_id, .. } =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected TabOpened")
    };

    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(new_id),
        None,
        &ClientMessage::CloseTab,
    )
    .unwrap();
    let (reply_tab, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert_eq!(reply_tab, Some(new_id));
    assert_eq!(reply, ServerMessage::TabClosed { tab_id: new_id });

    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(new_id),
        None,
        &ClientMessage::GetRepresentation,
    )
    .unwrap();
    let (_, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert!(
        matches!(reply, ServerMessage::Error { .. }),
        "a closed tab's id must no longer resolve, expected Error, got {reply:?}"
    );

    // Closing again is a harmless-but-reported "unknown tab" error,
    // not a panic or a second TabClosed.
    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(new_id),
        None,
        &ClientMessage::CloseTab,
    )
    .unwrap();
    let (_, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert!(matches!(reply, ServerMessage::Error { .. }));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_reply_to_an_untagged_request_still_echoes_the_resolved_default_tab_id() {
    // The load-bearing property that makes broadcast-shared,
    // multi-tab connections work at all: a request that left
    // `tab_id` implicit still gets a reply that self-discloses the
    // *concrete* tab it resolved to, not `None` -- otherwise a
    // second client sharing the connection via `blueice-launcher`'s
    // broker could never tell which tab an untagged client's
    // broadcasted reply was actually about.
    let dir = temp_frame_dir("echo-resolved-default-tab");
    let gatekeeper = clearing_gatekeeper("echo-resolved-default-tab");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
    let ServerMessage::Tabs(tabs) = blueice_ipc::read_server_message(&mut client).unwrap() else {
        panic!("expected Tabs")
    };
    let default_tab_id = tabs[0].id;

    // Sent with no tab_id at all -- the envelope-level default.
    blueice_ipc::write_client_message_with_id(&mut client, None, &ClientMessage::GetRepresentation)
        .unwrap();
    let (reply_tab, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert_eq!(
        reply_tab,
        Some(default_tab_id),
        "the reply must echo the resolved tab, not None"
    );
    assert!(matches!(reply, ServerMessage::Representation(_)));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

// -- Gatekeeper-specific behavior --------------------------------

#[test]
fn content_stage_rejection_blocks_navigation_and_leaves_the_page_unchanged() {
    let dir = temp_frame_dir("content-stage-block");
    let gatekeeper_path = unique_gatekeeper_socket_path("content-stage-block");
    let _ = std::fs::remove_file(&gatekeeper_path);
    let listener = UnixListener::bind(&gatekeeper_path).unwrap();
    thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else { break };
            let Ok(req) = blueice_ipc::gatekeeper::read_gatekeeper_request(&mut stream) else {
                continue;
            };
            let reply = match req {
                blueice_ipc::gatekeeper::GatekeeperRequest::CheckUrl { .. } => {
                    blueice_ipc::gatekeeper::GatekeeperReply::Cleared
                }
                blueice_ipc::gatekeeper::GatekeeperRequest::CheckContent { .. } => {
                    blueice_ipc::gatekeeper::GatekeeperReply::Rejected {
                        reason: "hidden instruction-shaped text".to_string(),
                        category: "prompt-injection".to_string(),
                    }
                }
            };
            let _ = blueice_ipc::gatekeeper::write_gatekeeper_reply(&mut stream, &reply);
        }
    });

    let http = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = http.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = http.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = std::io::Read::read(&mut stream, &mut buf);
        let body = "<p>malicious page</p>";
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
        run_session(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper_path,
        )
        .unwrap();
        dir
    });
    handshake(&mut client);

    let url = format!("http://{addr}");
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Navigate { url: url.clone() })
        .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert_eq!(
        reply,
        ServerMessage::GatekeeperBlocked {
            reason: "hidden instruction-shaped text".to_string(),
            category: "prompt-injection".to_string(),
            url: url.clone()
        }
    );

    // The page must not have changed: a follow-up GetRepresentation
    // shows no trace of the blocked page's content (no `FrameReady`
    // was ever produced for it either, since the only reply so far
    // was the GatekeeperBlocked above).
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(snap) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    assert!(!snap
        .nodes
        .iter()
        .any(|n| n.name.as_deref() == Some("malicious page")));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn navigation_fails_closed_when_the_gatekeeper_is_unreachable() {
    let dir = temp_frame_dir("gatekeeper-unreachable");
    let gatekeeper_path = unique_gatekeeper_socket_path("gatekeeper-unreachable"); // nothing listens here
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper_path,
        )
        .unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "http://example.invalid/".to_string(),
        },
    )
    .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(
        matches!(reply, ServerMessage::GatekeeperBlocked { .. }),
        "an unreachable gatekeeper must fail closed, got {reply:?}"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn navigation_fails_closed_when_the_gatekeeper_accepts_then_drops_the_connection() {
    let dir = temp_frame_dir("gatekeeper-drops-connection");
    let gatekeeper_path = unique_gatekeeper_socket_path("gatekeeper-drops-connection");
    let _ = std::fs::remove_file(&gatekeeper_path);
    let listener = UnixListener::bind(&gatekeeper_path).unwrap();
    thread::spawn(move || {
        for incoming in listener.incoming() {
            drop(incoming); // accept, then immediately disconnect -- no reply ever sent
        }
    });

    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper_path,
        )
        .unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "http://example.invalid/".to_string(),
        },
    )
    .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(
        matches!(reply, ServerMessage::GatekeeperBlocked { .. }),
        "a gatekeeper that drops the connection must fail closed, got {reply:?}"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_stalled_gatekeeper_check_for_one_tab_does_not_block_a_reply_to_another_tab() {
    // The single most important proof of the property this whole
    // mechanism exists for: a slow/stuck gatekeeper review for one
    // tab must never stall the one shared connection other tabs
    // (or clients sharing it via `blueice-launcher`'s broker) are
    // also using.
    let dir = temp_frame_dir("non-blocking-concurrency");
    let gatekeeper_path = unique_gatekeeper_socket_path("non-blocking-concurrency");
    let _ = std::fs::remove_file(&gatekeeper_path);
    let listener = UnixListener::bind(&gatekeeper_path).unwrap();
    thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else { break };
            thread::spawn(move || {
                if let Ok(req) = blueice_ipc::gatekeeper::read_gatekeeper_request(&mut stream) {
                    if matches!(&req, blueice_ipc::gatekeeper::GatekeeperRequest::CheckUrl { url } if url.contains("slow-tab"))
                    {
                        thread::sleep(Duration::from_millis(300));
                    }
                    let _ = blueice_ipc::gatekeeper::write_gatekeeper_reply(
                        &mut stream,
                        &blueice_ipc::gatekeeper::GatekeeperReply::Cleared,
                    );
                }
            });
        }
    });

    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper_path,
        )
        .unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::OpenTab { url: None }).unwrap();
    let ServerMessage::TabOpened { tab_id: tab_b, .. } =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected TabOpened")
    };

    // Kick off the default tab's navigation, whose gatekeeper check
    // stalls for 300ms -- fire-and-forget, its own reply isn't
    // waited on here.
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "http://127.0.0.1:1/slow-tab".to_string(),
        },
    )
    .unwrap();

    // Immediately address tab_b with an unrelated message.
    let start = Instant::now();
    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(tab_b),
        None,
        &ClientMessage::GetRepresentation,
    )
    .unwrap();
    let (reply_tab, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert_eq!(reply_tab, Some(tab_b));
    assert!(matches!(reply, ServerMessage::Representation(_)));
    assert!(start.elapsed() < Duration::from_millis(150), "tab_b's reply must arrive well before tab_a's stalled gatekeeper check resolves, took {:?}", start.elapsed());

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_second_navigation_supersedes_a_still_pending_first_one() {
    let dir = temp_frame_dir("supersede");
    let gatekeeper_path = unique_gatekeeper_socket_path("supersede");
    let _ = std::fs::remove_file(&gatekeeper_path);
    let listener = UnixListener::bind(&gatekeeper_path).unwrap();
    thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else { break };
            thread::spawn(move || {
                if let Ok(req) = blueice_ipc::gatekeeper::read_gatekeeper_request(&mut stream) {
                    if matches!(&req, blueice_ipc::gatekeeper::GatekeeperRequest::CheckUrl { url } if url.contains("first"))
                    {
                        thread::sleep(Duration::from_millis(300));
                    }
                    let _ = blueice_ipc::gatekeeper::write_gatekeeper_reply(
                        &mut stream,
                        &blueice_ipc::gatekeeper::GatekeeperReply::Cleared,
                    );
                }
            });
        }
    });

    let second_http = TcpListener::bind("127.0.0.1:0").unwrap();
    let second_addr = second_http.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = second_http.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = std::io::Read::read(&mut stream, &mut buf);
        let body = "<p>second page</p>";
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
        run_session(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper_path,
        )
        .unwrap();
        dir
    });
    handshake(&mut client);

    // First navigation: stalls 300ms on its own CheckUrl stage, and
    // even once cleared points nowhere reachable -- must never
    // become visible.
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "http://127.0.0.1:1/first-slow".to_string(),
        },
    )
    .unwrap();
    // Second navigation to the same (default) tab, sent immediately
    // after, well before the first's gatekeeper check resolves.
    let second_url = format!("http://{second_addr}");
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: second_url.clone(),
        },
    )
    .unwrap();

    let navigated = blueice_ipc::read_server_message(&mut client).unwrap();
    assert_eq!(navigated, ServerMessage::Navigated { url: second_url });
    let frame = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(frame, ServerMessage::FrameReady { .. }));

    // No further reply ever arrives for the stale first navigation,
    // even after waiting past its stall -- proven the same way
    // every other "harmless no-op" case in this file is: the next
    // real message still gets exactly one, normal reply.
    thread::sleep(Duration::from_millis(400));
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(snap) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    assert!(snap
        .nodes
        .iter()
        .any(|n| n.name.as_deref() == Some("second page")));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn open_tab_with_a_url_the_gatekeeper_blocks_replies_gatekeeper_blocked_not_tab_opened() {
    // `OpenTab{url: Some(_)}` goes through the same gated path
    // `Navigate` does (`PendingKind::OpenTab`) -- this is the
    // `OpenTab`-specific proof that a blocked outcome there reports
    // `GatekeeperBlocked`, not a bare `TabOpened`/`Error`, and that
    // no orphaned-but-blank tab id is leaked into a reply shape a
    // caller wouldn't expect.
    let dir = temp_frame_dir("open-tab-gatekeeper-blocked");
    let gatekeeper_path = unique_gatekeeper_socket_path("open-tab-gatekeeper-blocked");
    let _ = std::fs::remove_file(&gatekeeper_path);
    let listener = UnixListener::bind(&gatekeeper_path).unwrap();
    thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else { break };
            let Ok(_req) = blueice_ipc::gatekeeper::read_gatekeeper_request(&mut stream) else {
                continue;
            };
            let _ = blueice_ipc::gatekeeper::write_gatekeeper_reply(
                &mut stream,
                &blueice_ipc::gatekeeper::GatekeeperReply::Rejected {
                    reason: "known-bad domain".to_string(),
                    category: "blocklist".to_string(),
                },
            );
        }
    });

    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper_path,
        )
        .unwrap();
        dir
    });
    handshake(&mut client);

    let url = "http://example.invalid/".to_string();
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::OpenTab {
            url: Some(url.clone()),
        },
    )
    .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert_eq!(
        reply,
        ServerMessage::GatekeeperBlocked {
            reason: "known-bad domain".to_string(),
            category: "blocklist".to_string(),
            url
        }
    );

    // The session must still be alive afterward, same as every
    // other error/blocked case in this file -- and `ListTabs` must
    // still show the new (blank) tab `OpenTab` always creates,
    // per `ServerMessage::TabOpened`'s own documented limitation.
    blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
    let ServerMessage::Tabs(tabs) = blueice_ipc::read_server_message(&mut client).unwrap() else {
        panic!("expected Tabs")
    };
    assert_eq!(
        tabs.len(),
        2,
        "OpenTab always creates the tab, even though its requested navigation was blocked"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_non_navigating_message_to_a_tab_with_a_pending_navigation_applies_immediately() {
    // `phase-7-local-ai/PLAN.md`'s "Wiring design" is explicit that
    // this must work the way a real browser reflows/scrolls a
    // still-displayed old page while a new one loads: `Resize`
    // addressed to a tab whose gated navigation hasn't resolved yet
    // must apply immediately against that tab's *current*
    // (pre-navigation) `Page` state, not queue up behind it.
    //
    // ROOT CAUSE OF A ONCE-OBSERVED FLAKE (fixed here): this test
    // used to have the fake gatekeeper below stall for a fixed 5s
    // and then assert the `Resize` reply arrived in under 500ms --
    // i.e. it proved "immediate" by racing a *nearby fixed deadline*
    // (500ms) against the *other*, "queued" outcome's own fixed
    // deadline (5000ms). `Resize`'s dispatch (see the `ClientMessage
    // ::Resize` arm in `run_session` above) is not itself racy: it
    // never consults `pending_nav_seq`/the completion channel at
    // all, so its reply is always synchronously produced on the very
    // next loop iteration after the client's write lands in the
    // (already-buffered, in-kernel, `UnixStream::pair`) socket. But
    // "always produced immediately" is not the same as "always
    // *observed* within 500 wall-clock ms" -- under a fully loaded
    // test binary (many sibling `session::tests::*` cases, several
    // of which spawn their own OS threads and sleep), ordinary OS
    // scheduler latency in getting this test's own server thread its
    // next timeslice can occasionally eat into that margin, which is
    // exactly the kind of "sensitive to parallel test execution"
    // failure that was observed once in a full-workspace run. No
    // amount of widening that margin fixes this *for real* -- it
    // only shrinks the failure probability, which is precisely what
    // this test must not settle for (see `TEST_PLAN.md`'s Definition
    // of Done). The actual fix removes the race instead of narrowing
    // it: the fake gatekeeper below now blocks forever (never
    // replies), so the "queued" alternative can *never* resolve
    // during this test's lifetime, at any wall-clock distance -- the
    // `Resize` reply's mere arrival (bounded only by a generous,
    // not-tuned-against-anything timeout that exists purely so a
    // genuine regression fails promptly instead of hanging the
    // suite) is now itself the whole proof, with no nearby deadline
    // on either side of the comparison left to lose a race against.
    let dir = temp_frame_dir("resize-during-pending-nav");
    let gatekeeper_path = unique_gatekeeper_socket_path("resize-during-pending-nav");
    let _ = std::fs::remove_file(&gatekeeper_path);
    let listener = UnixListener::bind(&gatekeeper_path).unwrap();
    thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else { break };
            thread::spawn(move || {
                if let Ok(_req) = blueice_ipc::gatekeeper::read_gatekeeper_request(&mut stream) {
                    // Never replies, so the navigation this test
                    // kicks off can never resolve during the test's
                    // lifetime -- not merely "probably still pending
                    // after N seconds" (see the long comment above
                    // this test for why that distinction is the
                    // actual fix, not a tightened/loosened timeout).
                    // `thread::park` can wake spuriously, hence the
                    // loop; this thread simply leaks, parked, for
                    // the rest of the test binary's life once this
                    // test ends, same as any other test double here
                    // that outlives its own test.
                    loop {
                        thread::park();
                    }
                }
            });
        }
    });

    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str("<p>still the old page</p>", None);
        let mut generation = 0u64;
        run_session(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper_path,
        )
        .unwrap();
        dir
    });
    handshake(&mut client);

    // A generous, not-a-race-margin timeout: it exists only so a
    // genuine regression (an actual wait behind the now-permanently-
    // pending navigation) fails this test promptly instead of
    // hanging the whole suite, not to bound how fast the correct
    // path must be.
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "http://127.0.0.1:1/never-resolves".to_string(),
        },
    )
    .unwrap();

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Resize {
            width: 111,
            height: 222,
        },
    )
    .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).expect(
        "Resize must not be queued behind a pending navigation that, by construction \
             above, can now never complete -- a read timeout here means it was",
    );
    assert!(
        matches!(
            reply,
            ServerMessage::FrameReady {
                width: 111,
                height: 222,
                ..
            }
        ),
        "expected an immediate FrameReady for the resize, got {reply:?}"
    );

    // The old page's content is still what's shown -- the pending
    // navigation never actually applied.
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(snap) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    assert!(snap
        .nodes
        .iter()
        .any(|n| n.name.as_deref() == Some("still the old page")));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}
