// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(unix)]

//! Integration test for [`blueice_mcp_server::CoreProcess`] -- spawns
//! the *real* `blueice-core` binary (expected to sit next to this test
//! binary in the workspace's shared `target/` dir, same assumption
//! `blueice-frontend-reference` already makes) and drives it over the
//! real Unix socket, the same "drive the real protocol with a test
//! client" strategy `blueice_engine::session`'s own tests use one
//! layer down. Headless and display-free, unlike
//! `frontend-reference`'s own GUI integration -- there is no reason
//! this can't run in CI.

use blueice_launcher::control::{
    read_control_reply, write_control_request, ControlReply, ControlRequest,
};
use blueice_launcher::{run_broker, CoreLaunchOptions, SpawnedCore};
use blueice_mcp_server::{BlueIceMcpServer, CoreProcess, OpenTabOutcome};
use rmcp::model::{CallToolRequestParams, ClientInfo};
use rmcp::{ClientHandler, ServiceExt};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Default)]
struct CompilerMcpClient;

impl ClientHandler for CompilerMcpClient {
    fn get_info(&self) -> ClientInfo {
        ClientInfo::default()
    }
}

fn compiler_tool_reply(
    result: &rmcp::model::CallToolResult,
) -> blueice_ipc::compiler::CompilerReply {
    let text = result
        .content
        .first()
        .and_then(|content| content.as_text())
        .expect("compiler tool must return one text result")
        .text
        .as_str();
    let marker = format!("{}\n", blueice_mcp_server::UNTRUSTED_CONTENT_MARKER);
    let (_, reply) = text
        .split_once(&marker)
        .expect("compiler result must delimit untrusted metadata before JSON");
    let envelope: serde_json::Value =
        serde_json::from_str(reply).expect("compiler tool result must retain the session envelope");
    serde_json::from_value(
        envelope
            .get("reply")
            .cloned()
            .expect("compiler tool result must include its source-free reply"),
    )
    .expect("compiler tool result must retain the IPC reply shape")
}

fn compiler_session_id(result: &rmcp::model::CallToolResult) -> String {
    assert_eq!(result.is_error, Some(false));
    let text = result
        .content
        .first()
        .and_then(|content| content.as_text())
        .expect("compiler session capability must return one text result")
        .text
        .as_str();
    let value: serde_json::Value =
        serde_json::from_str(text).expect("compiler session capability must return JSON");
    assert_eq!(value.get("available"), Some(&serde_json::Value::Bool(true)));
    let session = value
        .get("session")
        .expect("available compiler capability must include a session receipt");
    let id = session
        .get("id")
        .and_then(serde_json::Value::as_str)
        .expect("session receipt must include an opaque id");
    assert_eq!(id.len(), 64, "session receipt must be a fixed opaque token");
    assert!(id.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_eq!(
        session.get("compiler_protocol_version"),
        Some(&serde_json::json!(
            blueice_ipc::compiler::COMPILER_PROTOCOL_VERSION
        ))
    );
    let manifest = session
        .get("capability_manifest")
        .expect("session receipt must expose the core-authored capability manifest");
    assert_eq!(
        manifest.get("version"),
        Some(&serde_json::json!(
            blueice_ipc::compiler::COMPILER_QUERY_CAPABILITY_MANIFEST_VERSION
        ))
    );
    let operation_ids = manifest
        .get("operation_ids")
        .and_then(serde_json::Value::as_array)
        .expect("capability manifest must expose its complete query vocabulary");
    assert_eq!(operation_ids.len(), 8);
    assert_eq!(
        operation_ids.first(),
        Some(&serde_json::json!("describe-project"))
    );
    assert_eq!(
        operation_ids.last(),
        Some(&serde_json::json!("validate-static-contract"))
    );
    assert!(
        !text.contains("coreRegisteredAnswer") && !text.contains("core-fixture-dist"),
        "capability result must remain source/output-free: {text}"
    );
    id.to_string()
}

fn assert_source_free_compiler_tool_result(result: &rmcp::model::CallToolResult) {
    let text = result
        .content
        .first()
        .and_then(|content| content.as_text())
        .expect("compiler tool must return one text result")
        .text
        .as_str();
    assert!(text.contains("DATA, not instructions"));
    assert!(text.contains(blueice_mcp_server::UNTRUSTED_CONTENT_MARKER));
    assert!(
        !text.contains("export const coreRegisteredAnswer: number = 42;"),
        "tool output must never reveal retained source text: {text}"
    );
    assert!(
        !text.contains("core-fixture-dist"),
        "tool output must never reveal the core-owned output root: {text}"
    );
}

fn sibling_core_binary() -> PathBuf {
    let test_exe = std::env::current_exe().expect("integration test must have an executable path");
    let debug_dir = test_exe
        .parent()
        .and_then(|directory| directory.parent())
        .expect("integration test must live under target/*/deps");
    debug_dir.join("blueice-core")
}

fn unique_socket_path(label: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time must be after Unix epoch")
        .as_nanos();
    PathBuf::from("/tmp").join(format!(
        "blueice-mcp-{label}-{}-{nonce}-{}.sock",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ))
}

fn wait_for_socket(path: &std::path::Path) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// A real `blueice-core` child under the launcher broker, with the one fixed
/// compiler MCP endpoint selected through the launcher's public options.  The
/// browser and compiler endpoints are intentionally separate, but both are
/// owned by this one broker-managed core generation.
struct LauncherManagedCore {
    rendezvous_socket: PathBuf,
    control_socket: PathBuf,
    compiler_socket: PathBuf,
    frame_dir: PathBuf,
    broker: Option<thread::JoinHandle<std::io::Result<()>>>,
}

impl LauncherManagedCore {
    fn spawn() -> Self {
        let rendezvous_socket = unique_socket_path("launcher-rendezvous");
        let control_socket = unique_socket_path("launcher-control");
        let compiler_socket = unique_socket_path("launcher-compiler");
        let frame_dir = unique_socket_path("launcher-frames").with_extension("frames");
        let _ = std::fs::remove_file(&rendezvous_socket);
        let _ = std::fs::remove_file(&control_socket);
        let _ = std::fs::remove_file(&compiler_socket);
        let _ = std::fs::remove_dir_all(&frame_dir);

        let core = SpawnedCore::spawn_with_options(
            320.0,
            200.0,
            &frame_dir,
            CoreLaunchOptions::default()
                .with_core_closed_compiler_mcp_endpoint(compiler_socket.clone()),
        )
        .expect("launcher must start a core with its fixed compiler profile");
        assert!(
            wait_for_socket(&compiler_socket),
            "launcher-selected core compiler endpoint must be live"
        );
        let listener = UnixListener::bind(&rendezvous_socket)
            .expect("launcher broker must bind its frontend rendezvous socket");
        let control_listener = UnixListener::bind(&control_socket)
            .expect("launcher broker must bind its control socket");
        let broker =
            thread::spawn(move || run_broker(listener, control_listener, core, 320.0, 200.0));

        Self {
            rendezvous_socket,
            control_socket,
            compiler_socket,
            frame_dir,
            broker: Some(broker),
        }
    }

    fn shutdown(&mut self) {
        let Some(broker) = self.broker.take() else {
            return;
        };
        if let Ok(mut browser) = std::os::unix::net::UnixStream::connect(&self.rendezvous_socket) {
            if blueice_ipc::client_handshake(&mut browser).is_ok() {
                let _ = blueice_ipc::write_client_message(
                    &mut browser,
                    &blueice_ipc::ClientMessage::Shutdown,
                );
            }
        }
        broker
            .join()
            .expect("launcher broker thread must not panic")
            .expect("launcher broker must end normally after core shutdown");
    }

    fn cutover(&self) {
        let mut control = std::os::unix::net::UnixStream::connect(&self.control_socket)
            .expect("launcher control endpoint must accept a cutover request");
        write_control_request(&mut control, &ControlRequest::Cutover)
            .expect("cutover request must serialize");
        assert!(matches!(
            read_control_reply(&mut control).expect("launcher must reply to a cutover request"),
            ControlReply::CutoverDone { .. }
        ));
    }
}

impl Drop for LauncherManagedCore {
    fn drop(&mut self) {
        self.shutdown();
        let _ = std::fs::remove_file(&self.rendezvous_socket);
        let _ = std::fs::remove_file(&self.control_socket);
        let _ = std::fs::remove_file(&self.compiler_socket);
        let _ = std::fs::remove_dir_all(&self.frame_dir);
    }
}

#[test]
fn spawn_connects_navigates_and_cleans_up_on_drop() {
    let core =
        CoreProcess::spawn(320, 200).expect("blueice-core must spawn and accept a connection");

    let outcome = {
        let mut conn = core.conn.lock().unwrap();
        conn.navigate("about:blank", None)
            .expect("navigate must round-trip over the real socket")
    };
    assert_eq!(outcome.error, None);
    assert_eq!(outcome.snapshot.url.as_deref(), Some("about:blank"));

    drop(core); // Drop's shutdown + child reap + socket cleanup must not panic or hang
}

#[test]
fn open_tab_list_tabs_and_close_tab_round_trip_over_a_real_core() {
    // The end-to-end proof `phase-16-multi-tab-and-tab-groups/PLAN.md`'s
    // MCP layer needs: `blueice-mcp-server`'s own tab tools driving a
    // real, separately-compiled `blueice-core` subprocess, not just the
    // fake-responder unit tests in `lib.rs`.
    let core =
        CoreProcess::spawn(320, 200).expect("blueice-core must spawn and accept a connection");
    let mut conn = core.conn.lock().unwrap();

    let initial = conn.list_tabs().expect("list_tabs must round-trip");
    assert_eq!(initial.len(), 1, "a fresh core starts with exactly one tab");
    let default_tab = initial[0].id;

    let opened = conn.open_tab(None).expect("open_tab must round-trip");
    let OpenTabOutcome::Opened {
        tab_id: new_tab,
        url,
    } = opened
    else {
        panic!("expected Opened, got {opened:?}")
    };
    assert_eq!(url, None);
    assert_ne!(new_tab, default_tab);

    let after_open = conn.list_tabs().unwrap();
    assert_eq!(
        after_open.iter().map(|t| t.id).collect::<Vec<_>>(),
        vec![default_tab, new_tab]
    );

    // The default tab must be completely unaffected by the new one existing.
    let default_outcome = conn
        .navigate("about:blank", Some(default_tab))
        .expect("navigate on the default tab must still work");
    assert_eq!(default_outcome.snapshot.tab_id, default_tab);

    let closed = conn.close_tab(new_tab).expect("close_tab must round-trip");
    assert_eq!(closed, blueice_mcp_server::CloseTabOutcome::Closed);

    let after_close = conn.list_tabs().unwrap();
    assert_eq!(
        after_close.iter().map(|t| t.id).collect::<Vec<_>>(),
        vec![default_tab]
    );
}

#[tokio::test]
async fn compiler_mcp_tools_page_exact_metadata_from_one_real_core_process() {
    // This drives the complete public MCP route -- MCP request JSON,
    // `BlueIceMcpServer`, `CompilerConnection`, compiler IPC and core session
    // owner -- against a single real core.  In particular, the browser and
    // compiler connections must target that same core, rather than allowing a
    // fallback browser core to become unrelated to the sealed compiler
    // catalog.
    let socket_path = unique_socket_path("frontend");
    let compiler_socket_path = unique_socket_path("compiler");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-mcp-compiler-frames-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_file(&compiler_socket_path);
    let _ = std::fs::remove_dir_all(&frame_dir);

    let mut core = Command::new(sibling_core_binary())
        .args([
            "--socket",
            socket_path.to_str().unwrap(),
            "--compiler-socket",
            compiler_socket_path.to_str().unwrap(),
            "--compiler-project-profile",
            "core-closed-fixture-v1",
            "--frame-dir",
            frame_dir.to_str().unwrap(),
        ])
        .stderr(Stdio::piped())
        .spawn()
        .expect("blueice-core must start for the MCP compiler regression");
    if !wait_for_socket(&socket_path) {
        let output = core
            .wait_with_output()
            .expect("core must report why its compiler fixture did not start");
        panic!(
            "blueice-core did not create its frontend socket: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    if !wait_for_socket(&compiler_socket_path) {
        let output = core
            .wait_with_output()
            .expect("core must report why its compiler listener did not start");
        panic!(
            "blueice-core did not create its compiler socket: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let server = BlueIceMcpServer::connect_with_core_and_compiler_sockets(
        &socket_path,
        &compiler_socket_path,
    )
    .expect("MCP server must attach both adapters to the existing core");
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        server
            .serve(server_transport)
            .await
            .expect("MCP server must bind its in-memory transport")
            .waiting()
            .await
            .expect("MCP service must finish cleanly after client cancellation");
    });
    let client = CompilerMcpClient
        .serve(client_transport)
        .await
        .expect("MCP client must negotiate the in-memory transport");

    let compiler_session = compiler_session_id(
        &client
            .call_tool(CallToolRequestParams::new("bluetsc_session_capabilities"))
            .await
            .expect("MCP compiler session capability must round-trip"),
    );

    macro_rules! compiler_tool {
        ($name:literal, $arguments:expr) => {{
            let mut arguments = $arguments;
            arguments["session_id"] = serde_json::Value::String(compiler_session.clone());
            client
                .call_tool(
                    CallToolRequestParams::new($name).with_arguments(
                        arguments
                            .as_object()
                            .expect("MCP compiler arguments must be an object")
                            .clone(),
                    ),
                )
                .await
                .expect("MCP compiler tool must round-trip")
        }};
    }

    let project_result = compiler_tool!(
        "bluetsc_describe_project",
        serde_json::json!({ "project_id": 1 })
    );
    assert_eq!(project_result.is_error, Some(false));
    assert_source_free_compiler_tool_result(&project_result);
    let blueice_ipc::compiler::CompilerReply::Project(project) =
        compiler_tool_reply(&project_result)
    else {
        panic!("the sealed core profile must return its source-free project identity")
    };
    assert_eq!(project.project.id, 1);
    assert_eq!(project.entry_module, "project:///core-fixture/main.ts");

    let wrong_project_session = client
        .call_tool(
            CallToolRequestParams::new("bluetsc_describe_project").with_arguments(
                serde_json::json!({
                    "session_id": "f".repeat(64),
                    "project_id": 1,
                })
                .as_object()
                .expect("MCP project description arguments must be an object")
                .clone(),
            ),
        )
        .await
        .expect("mismatched project session must receive a structured MCP result");
    assert_eq!(wrong_project_session.is_error, Some(true));
    let wrong_project_session_text = wrong_project_session.content[0]
        .as_text()
        .expect("mismatched project session result must be text")
        .text
        .as_str();
    assert!(wrong_project_session_text.contains("does not belong to this MCP adapter"));
    assert!(!wrong_project_session_text.contains("coreRegisteredAnswer"));

    let check_result = compiler_tool!("bluetsc_check", serde_json::json!({ "project_id": 1 }));
    assert_eq!(check_result.is_error, Some(false));
    assert_source_free_compiler_tool_result(&check_result);
    let blueice_ipc::compiler::CompilerReply::Check(check) = compiler_tool_reply(&check_result)
    else {
        panic!("the sealed core profile must return an exact check generation")
    };
    let summary = check
        .static_metadata
        .as_ref()
        .expect("successful check must retain source-free static metadata");

    let wrong_session = client
        .call_tool(
            CallToolRequestParams::new("debug_list_static_metadata").with_arguments(
                serde_json::json!({
                    "session_id": "f".repeat(64),
                    "project_id": 1,
                    "generation": check.generation.sequence,
                    "kind": "symbols",
                    "limit": 1,
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .expect("mismatched compiler session must receive a structured MCP result");
    assert_eq!(wrong_session.is_error, Some(true));
    let wrong_session_text = wrong_session.content[0]
        .as_text()
        .expect("mismatched session result must be text")
        .text
        .as_str();
    assert!(wrong_session_text.contains("does not belong to this MCP adapter"));
    assert!(!wrong_session_text.contains("coreRegisteredAnswer"));

    let unobserved_generation = compiler_tool!(
        "debug_list_static_metadata",
        serde_json::json!({
            "project_id": 1,
            "generation": check.generation.sequence + 1,
            "kind": "symbols",
            "limit": 1,
        })
    );
    assert_eq!(unobserved_generation.is_error, Some(true));
    assert_source_free_compiler_tool_result(&unobserved_generation);
    let blueice_ipc::compiler::CompilerReply::Error { code, message } =
        compiler_tool_reply(&unobserved_generation)
    else {
        panic!("unobserved session generation must fail before compiler IPC")
    };
    assert_eq!(
        code,
        blueice_ipc::compiler::CompilerErrorCode::StaleGeneration
    );
    assert!(message.contains("not observed by this MCP session"));

    let mut source_ids = Vec::new();
    let mut type_ids = Vec::new();
    let mut symbol_ids = Vec::new();
    let mut contract_ids = Vec::new();
    for (kind_name, kind, expected_count) in [
        (
            "sources",
            blueice_ipc::compiler::CompilerStaticMetadataKind::Sources,
            summary.source_count,
        ),
        (
            "types",
            blueice_ipc::compiler::CompilerStaticMetadataKind::Types,
            summary.type_count,
        ),
        (
            "symbols",
            blueice_ipc::compiler::CompilerStaticMetadataKind::Symbols,
            summary.symbol_count,
        ),
        (
            "contracts",
            blueice_ipc::compiler::CompilerStaticMetadataKind::Contracts,
            summary.contract_count,
        ),
    ] {
        let mut ids = Vec::new();
        let mut cursors = std::collections::BTreeSet::new();
        let mut cursor = None;
        loop {
            let page_result = compiler_tool!(
                "debug_list_static_metadata",
                serde_json::json!({
                    "project_id": 1,
                    "generation": check.generation.sequence,
                    "kind": kind_name,
                    "cursor": cursor.map(|cursor: blueice_ipc::compiler::CompilerStaticMetadataCursor| {
                        serde_json::json!({ "id": cursor.id })
                    }),
                    "limit": 1,
                })
            );
            assert_eq!(page_result.is_error, Some(false));
            assert_source_free_compiler_tool_result(&page_result);
            let blueice_ipc::compiler::CompilerReply::StaticMetadataPage(page) =
                compiler_tool_reply(&page_result)
            else {
                panic!("{kind_name} inventory must return a page")
            };
            assert_eq!(page.generation, check.generation);
            assert_eq!(page.kind, kind);
            assert!(
                page.ids.len() <= 1,
                "core must enforce the MCP page bound for {kind_name}"
            );
            assert!(!page.ids.is_empty(), "inventory page must advance");
            ids.extend(page.ids);
            cursor = page.next_cursor;
            if let Some(cursor) = cursor {
                assert!(
                    cursors.insert(cursor.id),
                    "core must mint a fresh opaque continuation cursor"
                );
            } else {
                break;
            }
        }
        assert_eq!(u32::try_from(ids.len()).unwrap(), expected_count);
        match kind {
            blueice_ipc::compiler::CompilerStaticMetadataKind::Sources => source_ids = ids,
            blueice_ipc::compiler::CompilerStaticMetadataKind::Types => type_ids = ids,
            blueice_ipc::compiler::CompilerStaticMetadataKind::Symbols => symbol_ids = ids,
            blueice_ipc::compiler::CompilerStaticMetadataKind::Contracts => contract_ids = ids,
        }
    }

    // Each inventory ID must feed its existing exact-generation query; no
    // ordinal is guessed and no metadata list becomes a source-read API.
    for source_id in &source_ids {
        let result = compiler_tool!(
            "debug_get_provenance",
            serde_json::json!({
                "project_id": 1,
                "generation": check.generation.sequence,
                "source_id": source_id,
            })
        );
        assert_eq!(result.is_error, Some(false));
        assert_source_free_compiler_tool_result(&result);
        assert!(matches!(
            compiler_tool_reply(&result),
            blueice_ipc::compiler::CompilerReply::StaticProvenance(_)
        ));
    }
    for type_id in &type_ids {
        let result = compiler_tool!(
            "debug_get_type",
            serde_json::json!({
                "project_id": 1,
                "generation": check.generation.sequence,
                "id": type_id,
            })
        );
        assert_eq!(result.is_error, Some(false));
        assert_source_free_compiler_tool_result(&result);
        assert!(matches!(
            compiler_tool_reply(&result),
            blueice_ipc::compiler::CompilerReply::StaticType(_)
        ));
    }

    let mut symbol_source_ids = Vec::new();
    let mut symbol_contract_ids = Vec::new();
    for symbol_id in &symbol_ids {
        let result = compiler_tool!(
            "debug_get_symbol",
            serde_json::json!({
                "project_id": 1,
                "generation": check.generation.sequence,
                "id": symbol_id,
            })
        );
        assert_eq!(result.is_error, Some(false));
        assert_source_free_compiler_tool_result(&result);
        let blueice_ipc::compiler::CompilerReply::StaticSymbol(symbol) =
            compiler_tool_reply(&result)
        else {
            panic!("discovered symbol ID must resolve through its exact query")
        };
        symbol_source_ids.push(symbol.source_id);
        if let Some(contract_id) = symbol.contract_id {
            symbol_contract_ids.push(contract_id);
        }
    }
    assert!(symbol_source_ids.iter().all(|id| source_ids.contains(id)));
    assert!(
        !symbol_contract_ids.is_empty(),
        "core fixture must retain a local reifiable contract"
    );
    assert!(
        symbol_contract_ids
            .iter()
            .all(|id| contract_ids.contains(id)),
        "symbol contract references must come from the exact inventory"
    );

    for contract_id in &contract_ids {
        let result = compiler_tool!(
            "debug_get_contract",
            serde_json::json!({
                "project_id": 1,
                "generation": check.generation.sequence,
                "id": contract_id,
            })
        );
        assert_eq!(result.is_error, Some(false));
        assert_source_free_compiler_tool_result(&result);
        assert!(matches!(
            compiler_tool_reply(&result),
            blueice_ipc::compiler::CompilerReply::StaticContract(_)
        ));
    }
    let validation_result = compiler_tool!(
        "debug_validate_contract",
        serde_json::json!({
            "project_id": 1,
            "generation": check.generation.sequence,
            "id": symbol_contract_ids[0],
            "value": { "enabled": true },
        })
    );
    assert_eq!(validation_result.is_error, Some(false));
    assert_source_free_compiler_tool_result(&validation_result);
    assert!(matches!(
        compiler_tool_reply(&validation_result),
        blueice_ipc::compiler::CompilerReply::ContractValidation(
            blueice_ipc::compiler::CompilerContractValidation { valid: true, .. }
        )
    ));

    // Cross-kind use fails without consuming the genuine symbols cursor; a
    // subsequent valid use consumes it, and replay then fails.
    let first_symbols_result = compiler_tool!(
        "debug_list_static_metadata",
        serde_json::json!({
            "project_id": 1,
            "generation": check.generation.sequence,
            "kind": "symbols",
            "limit": 1,
        })
    );
    let blueice_ipc::compiler::CompilerReply::StaticMetadataPage(first_symbols) =
        compiler_tool_reply(&first_symbols_result)
    else {
        panic!("symbols inventory must start a bounded page")
    };
    let symbols_cursor = first_symbols
        .next_cursor
        .expect("fixture must require a symbols continuation cursor");
    let cross_kind_result = compiler_tool!(
        "debug_list_static_metadata",
        serde_json::json!({
            "project_id": 1,
            "generation": check.generation.sequence,
            "kind": "types",
            "cursor": { "id": symbols_cursor.id },
            "limit": 1,
        })
    );
    assert_eq!(cross_kind_result.is_error, Some(true));
    assert_source_free_compiler_tool_result(&cross_kind_result);
    assert!(matches!(
        compiler_tool_reply(&cross_kind_result),
        blueice_ipc::compiler::CompilerReply::Error {
            code: blueice_ipc::compiler::CompilerErrorCode::InvalidMetadataCursor,
            ..
        }
    ));
    let valid_continuation_result = compiler_tool!(
        "debug_list_static_metadata",
        serde_json::json!({
            "project_id": 1,
            "generation": check.generation.sequence,
            "kind": "symbols",
            "cursor": { "id": symbols_cursor.id },
            "limit": 1,
        })
    );
    assert_eq!(valid_continuation_result.is_error, Some(false));
    let replay_result = compiler_tool!(
        "debug_list_static_metadata",
        serde_json::json!({
            "project_id": 1,
            "generation": check.generation.sequence,
            "kind": "symbols",
            "cursor": { "id": symbols_cursor.id },
            "limit": 1,
        })
    );
    assert_eq!(replay_result.is_error, Some(true));
    assert_source_free_compiler_tool_result(&replay_result);
    assert!(matches!(
        compiler_tool_reply(&replay_result),
        blueice_ipc::compiler::CompilerReply::Error {
            code: blueice_ipc::compiler::CompilerErrorCode::InvalidMetadataCursor,
            ..
        }
    ));

    let stale_page_result = compiler_tool!(
        "debug_list_static_metadata",
        serde_json::json!({
            "project_id": 1,
            "generation": check.generation.sequence,
            "kind": "symbols",
            "limit": 1,
        })
    );
    let blueice_ipc::compiler::CompilerReply::StaticMetadataPage(stale_page) =
        compiler_tool_reply(&stale_page_result)
    else {
        panic!("fresh symbols page must produce a cursor before a later check")
    };
    let stale_cursor = stale_page
        .next_cursor
        .expect("fixture must retain enough symbols for stale-cursor coverage");
    let later_check_result =
        compiler_tool!("bluetsc_check", serde_json::json!({ "project_id": 1 }));
    let blueice_ipc::compiler::CompilerReply::Check(later_check) =
        compiler_tool_reply(&later_check_result)
    else {
        panic!("later core check must produce a new generation")
    };
    assert_ne!(later_check.generation, check.generation);
    let stale_result = compiler_tool!(
        "debug_list_static_metadata",
        serde_json::json!({
            "project_id": 1,
            "generation": check.generation.sequence,
            "kind": "symbols",
            "cursor": { "id": stale_cursor.id },
            "limit": 1,
        })
    );
    assert_eq!(stale_result.is_error, Some(true));
    assert_source_free_compiler_tool_result(&stale_result);
    assert!(matches!(
        compiler_tool_reply(&stale_result),
        blueice_ipc::compiler::CompilerReply::Error {
            code: blueice_ipc::compiler::CompilerErrorCode::StaleGeneration,
            ..
        }
    ));

    client.cancel().await.unwrap();
    server_task.await.unwrap();

    // This direct core mode owns one serial browser-control connection; the
    // MCP server closing that connection ends this test child cleanly.
    assert!(core.wait().unwrap().success());
    assert!(!compiler_socket_path.exists());
    assert!(!frame_dir.exists());
}

#[tokio::test]
async fn launcher_managed_core_keeps_mcp_browser_and_fixed_compiler_adapters_paired() {
    // The direct-core test above proves the query protocol.  This regression
    // proves the deployment shape: the launcher selects exactly its compiled
    // in profile and MCP attaches its browser plus compiler adapters to the
    // one same launcher-managed core, without a fallback process or project
    // registration route.
    let mut launcher = LauncherManagedCore::spawn();
    let server = BlueIceMcpServer::connect_with_core_and_compiler_sockets(
        &launcher.rendezvous_socket,
        &launcher.compiler_socket,
    )
    .expect("MCP must pair both endpoints owned by the launcher core");
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        server
            .serve(server_transport)
            .await
            .expect("MCP server must bind its in-memory transport")
            .waiting()
            .await
            .expect("MCP service must finish cleanly after client cancellation");
    });
    let client = CompilerMcpClient
        .serve(client_transport)
        .await
        .expect("MCP client must negotiate the in-memory transport");

    let compiler_session = compiler_session_id(
        &client
            .call_tool(CallToolRequestParams::new("bluetsc_session_capabilities"))
            .await
            .expect("paired MCP compiler session capability must round-trip"),
    );

    let navigate = client
        .call_tool(
            CallToolRequestParams::new("navigate").with_arguments(
                serde_json::json!({ "url": "about:blank" })
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .expect("browser adapter must reach the launcher-managed core");
    assert_eq!(navigate.is_error, Some(false));

    let check = client
        .call_tool(
            CallToolRequestParams::new("bluetsc_check").with_arguments(
                serde_json::json!({ "session_id": compiler_session, "project_id": 1 })
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .expect("compiler adapter must reach the paired core-owned profile");
    assert_eq!(check.is_error, Some(false));
    assert_source_free_compiler_tool_result(&check);
    assert!(matches!(
        compiler_tool_reply(&check),
        blueice_ipc::compiler::CompilerReply::Check(_)
    ));

    client.cancel().await.unwrap();
    server_task.await.unwrap();
    launcher.shutdown();
    assert!(
        !launcher.compiler_socket.exists(),
        "launcher shutdown must clean the compiler endpoint after paired MCP disconnects"
    );
}

#[tokio::test]
async fn launcher_cutover_keeps_mcp_compiler_connections_generation_pinned() {
    // This is deliberately a real paired adapter path rather than a raw
    // compiler client: browser MCP remains attached to the broker through a
    // cutover, while its separately accepted compiler stream is pinned to
    // v1.  It must fail closed after v1 ends, never acquire v2's catalog.
    let mut launcher = LauncherManagedCore::spawn();
    let server = BlueIceMcpServer::connect_with_core_and_compiler_sockets(
        &launcher.rendezvous_socket,
        &launcher.compiler_socket,
    )
    .expect("MCP must pair both v1 endpoints before cutover");
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        server
            .serve(server_transport)
            .await
            .expect("v1 MCP server must bind its in-memory transport")
            .waiting()
            .await
            .expect("v1 MCP service must finish after client cancellation");
    });
    let client = CompilerMcpClient
        .serve(client_transport)
        .await
        .expect("v1 MCP client must negotiate the in-memory transport");

    let v1_session = compiler_session_id(
        &client
            .call_tool(CallToolRequestParams::new("bluetsc_session_capabilities"))
            .await
            .expect("v1 MCP compiler session capability must round-trip"),
    );

    let v1_check_result = client
        .call_tool(
            CallToolRequestParams::new("bluetsc_check").with_arguments(
                serde_json::json!({ "session_id": v1_session, "project_id": 1 })
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .expect("v1 compiler check must round-trip before cutover");
    let blueice_ipc::compiler::CompilerReply::Check(v1_check) =
        compiler_tool_reply(&v1_check_result)
    else {
        panic!("v1 must expose its sealed check generation")
    };
    let first_symbols = client
        .call_tool(
            CallToolRequestParams::new("debug_list_static_metadata").with_arguments(
                serde_json::json!({
                    "session_id": v1_session,
                    "project_id": 1,
                    "generation": v1_check.generation.sequence,
                    "kind": "symbols",
                    "limit": 1,
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .expect("v1 metadata page must round-trip before cutover");
    let blueice_ipc::compiler::CompilerReply::StaticMetadataPage(v1_symbols) =
        compiler_tool_reply(&first_symbols)
    else {
        panic!("v1 must mint a bounded symbols continuation")
    };
    let v1_cursor = v1_symbols
        .next_cursor
        .expect("fixed profile must have enough symbols for a cursor");

    launcher.cutover();

    let old_cursor = client
        .call_tool(
            CallToolRequestParams::new("debug_list_static_metadata").with_arguments(
                serde_json::json!({
                    "session_id": v1_session,
                    "project_id": 1,
                    "generation": v1_check.generation.sequence,
                    "kind": "symbols",
                    "cursor": { "id": v1_cursor.id },
                    "limit": 1,
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await;
    if let Ok(result) = old_cursor {
        assert_eq!(
            result.is_error,
            Some(true),
            "a compiler request on the pre-cutover MCP connection must fail closed"
        );
    }
    client.cancel().await.unwrap();
    server_task.await.unwrap();

    // A new paired adapter sees the freshly committed v2.  Replaying the
    // cursor through this *new* connection still fails: cursor state belongs
    // to v1 and is neither copied nor reinterpreted by the public relay.
    let server = BlueIceMcpServer::connect_with_core_and_compiler_sockets(
        &launcher.rendezvous_socket,
        &launcher.compiler_socket,
    )
    .expect("a newly accepted paired MCP adapter must reach v2");
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        server
            .serve(server_transport)
            .await
            .expect("v2 MCP server must bind its in-memory transport")
            .waiting()
            .await
            .expect("v2 MCP service must finish after client cancellation");
    });
    let client = CompilerMcpClient
        .serve(client_transport)
        .await
        .expect("v2 MCP client must negotiate the in-memory transport");
    let v2_session = compiler_session_id(
        &client
            .call_tool(CallToolRequestParams::new("bluetsc_session_capabilities"))
            .await
            .expect("v2 MCP compiler session capability must round-trip"),
    );
    assert_ne!(
        v1_session, v2_session,
        "a newly accepted relay connection must receive a fresh MCP session receipt"
    );
    let v2_check_result = client
        .call_tool(
            CallToolRequestParams::new("bluetsc_check").with_arguments(
                serde_json::json!({ "session_id": v2_session, "project_id": 1 })
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .expect("new paired compiler adapter must reach v2");
    let blueice_ipc::compiler::CompilerReply::Check(v2_check) =
        compiler_tool_reply(&v2_check_result)
    else {
        panic!("v2 must expose its sealed check generation")
    };
    let replayed_cursor = client
        .call_tool(
            CallToolRequestParams::new("debug_list_static_metadata").with_arguments(
                serde_json::json!({
                    "session_id": v2_session,
                    "project_id": 1,
                    "generation": v2_check.generation.sequence,
                    "kind": "symbols",
                    "cursor": { "id": v1_cursor.id },
                    "limit": 1,
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .expect("v2 must reject an old opaque cursor as a structured reply");
    assert_eq!(replayed_cursor.is_error, Some(true));
    assert_source_free_compiler_tool_result(&replayed_cursor);
    assert!(matches!(
        compiler_tool_reply(&replayed_cursor),
        blueice_ipc::compiler::CompilerReply::Error {
            code: blueice_ipc::compiler::CompilerErrorCode::InvalidMetadataCursor,
            ..
        }
    ));

    client.cancel().await.unwrap();
    server_task.await.unwrap();
    launcher.shutdown();
    assert!(
        !launcher.compiler_socket.exists(),
        "stable public compiler socket must be removed on final launcher shutdown"
    );
}
