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

use blueice_mcp_server::{CompilerConnection, CoreProcess, OpenTabOutcome};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn sibling_core_binary() -> PathBuf {
    let test_exe = std::env::current_exe().expect("integration test must have an executable path");
    let debug_dir = test_exe
        .parent()
        .and_then(|directory| directory.parent())
        .expect("integration test must live under target/*/deps");
    debug_dir.join("blueice-core")
}

fn unique_socket_path(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time must be after Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "blueice-mcp-{label}-{}-{nonce}.sock",
        std::process::id()
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

#[test]
fn compiler_connection_queries_contract_and_provenance_from_a_real_core_process() {
    // This is the MCP-side e2e counterpart to core's compiler listener test:
    // use the public `CompilerConnection`, not a direct adapter, against a
    // core process that owns and seals the compiled-in catalog before binding
    // its private metadata socket.
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
    assert!(wait_for_socket(&socket_path));
    assert!(wait_for_socket(&compiler_socket_path));

    let mut frontend = UnixStream::connect(&socket_path).unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();
    let mut compiler = CompilerConnection::new(UnixStream::connect(&compiler_socket_path).unwrap());
    compiler.handshake().unwrap();
    let blueice_ipc::compiler::CompilerReply::Check(check) = compiler.check(1).unwrap() else {
        panic!("the sealed core profile must return a check generation")
    };
    let blueice_ipc::compiler::CompilerReply::StaticSymbol(symbol) = compiler
        .static_symbol(1, check.generation.sequence, 0)
        .unwrap()
    else {
        panic!("the retained local interface must expose an opaque contract ID")
    };
    let contract_id = symbol.contract_id.unwrap();
    assert!(matches!(
        compiler
            .static_provenance(1, check.generation.sequence, symbol.source_id)
            .unwrap(),
        blueice_ipc::compiler::CompilerReply::StaticProvenance(_)
    ));
    assert!(matches!(
        compiler
            .static_contract(1, check.generation.sequence, contract_id)
            .unwrap(),
        blueice_ipc::compiler::CompilerReply::StaticContract(_)
    ));
    assert!(matches!(
        compiler
            .validate_static_contract(
                1,
                check.generation.sequence,
                contract_id,
                blueice_ipc::compiler::CompilerContractValue::Object(
                    std::collections::BTreeMap::from([(
                        "enabled".to_string(),
                        blueice_ipc::compiler::CompilerContractValue::Boolean(true),
                    )]),
                ),
            )
            .unwrap(),
        blueice_ipc::compiler::CompilerReply::ContractValidation(
            blueice_ipc::compiler::CompilerContractValidation { valid: true, .. }
        )
    ));

    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown)
        .unwrap();
    assert!(core.wait().unwrap().success());
    assert!(!compiler_socket_path.exists());
    assert!(!frame_dir.exists());
}
