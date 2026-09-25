// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! End-to-end stdio compatibility with an actual MCP client implementation.
//!
//! `mcp_downloads.rs` deliberately keeps a small raw JSON-RPC client for
//! wire-level edge cases. This test instead uses rmcp's `TokioChildProcess`,
//! the standard stdio client transport, against the compiled binary. That
//! proves the public client/server lifecycle, including initialization and
//! clean EOF shutdown, instead of merely proving that two local framing
//! implementations agree.

#[path = "../../net/tests/common/mod.rs"]
mod common;

use common::TempDir;
use rmcp::{model::CallToolRequestParams, transport::TokioChildProcess, ServiceExt};
use serde_json::json;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Stdio;

fn command(
    binary: &Path,
    runtime: &TempDir,
    data: &TempDir,
    downloads: &TempDir,
) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(binary);
    command
        .env("XDG_RUNTIME_DIR", runtime.path())
        .env("XDG_DATA_HOME", data.path())
        .env("BLUEICE_DOWNLOAD_DIR", downloads.path())
        .stderr(Stdio::null());
    command
}

fn isolated_server_copy(dir: &TempDir) -> io::Result<std::path::PathBuf> {
    let copied = dir.join("blueice-mcp-server");
    std::fs::copy(env!("CARGO_BIN_EXE_blueice-mcp-server"), &copied)?;
    let mut permissions = std::fs::metadata(&copied)?.permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&copied, permissions)?;
    Ok(copied)
}

#[tokio::test]
async fn an_official_mcp_client_lists_tools_without_a_sibling_core_binary() {
    // The copy has no adjacent `blueice-core`. If the adapter tried to create
    // a browser before the first browser tool, its process would exit before
    // the official client can even complete `initialize`/`tools/list`.
    let bin_dir = TempDir::new();
    let binary = isolated_server_copy(&bin_dir).expect("copy the MCP binary");
    let runtime = TempDir::new();
    let data = TempDir::new();
    let downloads = TempDir::new();
    let transport = TokioChildProcess::new(command(&binary, &runtime, &data, &downloads))
        .expect("start the isolated MCP server");
    let client = ().serve(transport).await.expect("initialize through rmcp");

    let tools = client
        .list_all_tools()
        .await
        .expect("list tools through rmcp");
    let names: Vec<_> = tools.iter().map(|tool| tool.name.as_ref()).collect();
    assert!(
        names.contains(&"navigate"),
        "missing browser tools: {names:?}"
    );
    assert!(
        names.contains(&"download_file"),
        "missing download tools: {names:?}"
    );
    for tool in [
        "set_translation_language",
        "show_translation",
        "summarize_page",
        "organize_page",
        "get_assistant_settings",
        "propose_assistant_settings",
        "assistant_settings_proposal_status",
        "blueice_status",
    ] {
        assert!(
            names.contains(&tool),
            "missing Phase 7 translation tool {tool}: {names:?}"
        );
    }
    assert!(
        names.contains(&"remove_sftp_password"),
        "missing Phase 11 credential removal: {names:?}"
    );
    assert!(
        names.contains(&"bluejs_run") && names.contains(&"bluejs_analyze"),
        "missing BlueJS tools: {names:?}"
    );

    let arguments = serde_json::from_value(json!({ "code": "console.log('hello'); 2 + 3;" }))
        .expect("bluejs_run arguments are a JSON object");
    let result = client
        .call_tool(CallToolRequestParams::new("bluejs_run").with_arguments(arguments))
        .await
        .expect("run BlueJS through rmcp");
    assert_ne!(result.is_error, Some(true), "{result:?}");
    let result = serde_json::to_value(result).expect("tool result serializes");
    let text = result["content"][0]["text"]
        .as_str()
        .expect("bluejs output is text");
    assert!(text.contains("\"completion\": \"5\""), "{text}");
    assert!(text.contains("\"hello\""), "{text}");

    let arguments = serde_json::from_value(json!({ "code": "document.getElementById('target');" }))
        .expect("bluejs_analyze arguments are a JSON object");
    let result = client
        .call_tool(CallToolRequestParams::new("bluejs_analyze").with_arguments(arguments))
        .await
        .expect("analyze BlueJS through rmcp");
    assert_ne!(result.is_error, Some(true), "{result:?}");
    let result = serde_json::to_value(result).expect("tool result serializes");
    let text = result["content"][0]["text"]
        .as_str()
        .expect("bluejs analysis is text");
    assert!(text.contains("dom_read"), "{text}");

    client.cancel().await.expect("close the stdio MCP session");
}

#[tokio::test]
async fn an_official_mcp_client_drives_a_real_blueice_core() {
    let runtime = TempDir::new();
    let data = TempDir::new();
    let downloads = TempDir::new();
    let transport = TokioChildProcess::new(command(
        Path::new(env!("CARGO_BIN_EXE_blueice-mcp-server")),
        &runtime,
        &data,
        &downloads,
    ))
    .expect("start the MCP server");
    let client = ().serve(transport).await.expect("initialize through rmcp");

    let arguments = serde_json::from_value(json!({ "url": "about:credits" }))
        .expect("navigate arguments are a JSON object");
    let result = client
        .call_tool(CallToolRequestParams::new("navigate").with_arguments(arguments))
        .await
        .expect("navigate through rmcp");
    assert_ne!(result.is_error, Some(true), "{result:?}");
    let result = serde_json::to_value(result).expect("tool result serializes");
    let text = result["content"][0]["text"]
        .as_str()
        .expect("navigate result begins with text");
    assert!(text.contains("UNTRUSTED PAGE CONTENT"), "{text}");
    assert!(text.contains("about:credits"), "{text}");

    client.cancel().await.expect("close the stdio MCP session");
}

#[tokio::test]
async fn translation_tools_report_state_and_refuse_without_an_assistant() {
    let runtime = TempDir::new();
    let data = TempDir::new();
    let downloads = TempDir::new();
    let transport = TokioChildProcess::new(command(
        Path::new(env!("CARGO_BIN_EXE_blueice-mcp-server")),
        &runtime,
        &data,
        &downloads,
    ))
    .expect("start the MCP server");
    let client = ().serve(transport).await.expect("initialize through rmcp");

    let arguments = serde_json::from_value(json!({ "url": "about:credits" })).unwrap();
    client
        .call_tool(CallToolRequestParams::new("navigate").with_arguments(arguments))
        .await
        .expect("navigate through rmcp");

    // A page nobody translated has nothing to toggle, but the call is fine.
    let arguments = serde_json::from_value(json!({ "shown": false })).unwrap();
    let result = client
        .call_tool(CallToolRequestParams::new("show_translation").with_arguments(arguments))
        .await
        .expect("show_translation through rmcp");
    assert_ne!(result.is_error, Some(true), "{result:?}");
    let result = serde_json::to_value(result).unwrap();
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("UNTRUSTED PAGE CONTENT"), "{text}");
    assert!(text.contains("\"available\": false"), "{text}");
    assert!(text.contains("\"shown\": false"), "{text}");

    // The MCP client's private core has no assistant, and says so.
    let arguments = serde_json::from_value(json!({ "target_language": "zh-TW" })).unwrap();
    let result = client
        .call_tool(CallToolRequestParams::new("set_translation_language").with_arguments(arguments))
        .await
        .expect("set_translation_language through rmcp");
    assert_eq!(result.is_error, Some(true), "{result:?}");
    let result = serde_json::to_value(result).unwrap();
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("unavailable"), "{text}");

    // Summaries need an assistant too, and an unavailable one is an error
    // result naming the problem rather than an empty success.
    let result = client
        .call_tool(CallToolRequestParams::new("summarize_page"))
        .await
        .expect("summarize_page through rmcp");
    assert_eq!(result.is_error, Some(true), "{result:?}");
    let result = serde_json::to_value(result).unwrap();
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("unavailable"), "{text}");

    client.cancel().await.expect("close the stdio MCP session");
}

/// A fake launcher control socket that answers each request in turn from
/// `replies`, and reports every request it was asked.
fn fake_control_socket(
    replies: Vec<blueice_launcher::control::ControlReply>,
) -> (
    std::path::PathBuf,
    std::thread::JoinHandle<Vec<blueice_launcher::control::ControlRequest>>,
) {
    use blueice_launcher::control::{read_control_request, write_control_reply};
    let path = std::env::temp_dir().join(format!("mcp-rmcp-ctl-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
    let worker = std::thread::spawn(move || {
        let mut seen = Vec::new();
        for reply in replies {
            let (mut stream, _) = listener.accept().unwrap();
            seen.push(read_control_request(&mut stream).unwrap());
            write_control_reply(&mut stream, &reply).unwrap();
        }
        seen
    });
    (path, worker)
}

#[tokio::test]
async fn settings_proposals_are_proposals_only_and_blocked_ones_say_why() {
    use blueice_assistant_settings::AssistantSettings;
    use blueice_launcher::control::{ControlReply, ControlRequest};
    let (control, launcher) = fake_control_socket(vec![
        ControlReply::AssistantSettingsInForce {
            settings: Box::new(AssistantSettings::default()),
        },
        ControlReply::AssistantProposalAccepted {
            id: 7,
            digest: "d".into(),
            diff: vec!["Priority (nice): 10 -> 12".into()],
        },
        ControlReply::AssistantProposalBlocked {
            violations: vec!["the proposal changes nothing".into()],
        },
        ControlReply::AssistantProposalStatus {
            status: "pending".into(),
        },
    ]);
    let runtime = TempDir::new();
    let data = TempDir::new();
    let downloads = TempDir::new();
    let mut cmd = command(
        Path::new(env!("CARGO_BIN_EXE_blueice-mcp-server")),
        &runtime,
        &data,
        &downloads,
    );
    cmd.arg("--launcher-control-socket").arg(&control);
    let client = ().serve(TokioChildProcess::new(cmd).expect("start")).await.expect("initialize");

    let call = |name: &'static str, args: serde_json::Value| {
        let client = &client;
        async move {
            let arguments = args.as_object().cloned();
            let mut request = CallToolRequestParams::new(name);
            if let Some(arguments) = arguments {
                request = request.with_arguments(arguments);
            }
            let result = client.call_tool(request).await.expect(name);
            let error = result.is_error == Some(true);
            let value = serde_json::to_value(result).unwrap();
            (
                error,
                value["content"][0]["text"].as_str().unwrap().to_string(),
            )
        }
    };

    let (error, text) = call("get_assistant_settings", json!({})).await;
    assert!(!error);
    assert!(text.contains("\"backend\": \"none\""), "{text}");

    let proposal = json!({"settings": {"backend": "none", "idle_timeout_secs": 600, "nice": 12}});
    let (error, text) = call("propose_assistant_settings", proposal.clone()).await;
    assert!(!error, "{text}");
    assert!(text.contains("NOTHING HAS CHANGED"), "{text}");
    assert!(text.contains("cannot approve it yourself"), "{text}");

    let (error, text) = call("propose_assistant_settings", proposal).await;
    assert!(error, "a blocked proposal is an error result");
    assert!(text.contains("person was not asked"), "{text}");
    assert!(text.contains("changes nothing"), "{text}");

    let (error, text) = call("assistant_settings_proposal_status", json!({"id": 7})).await;
    assert!(!error);
    assert!(text.contains("pending"), "{text}");

    // An unknown backend word never reaches the launcher at all.
    let (error, text) = call(
        "propose_assistant_settings",
        json!({"settings": {"backend": "quantum", "idle_timeout_secs": 600, "nice": 12}}),
    )
    .await;
    assert!(error);
    assert!(text.contains("quantum"), "{text}");

    client.cancel().await.expect("close");
    let seen = launcher.join().unwrap();
    assert!(matches!(seen[0], ControlRequest::InspectAssistantSettings));
    assert!(matches!(
        seen[1],
        ControlRequest::ProposeAssistantSettings { .. }
    ));
    assert!(matches!(
        seen[3],
        ControlRequest::AssistantProposalStatus { id: 7 }
    ));
    assert_eq!(seen.len(), 4, "the bad-backend proposal was never sent");
    let _ = std::fs::remove_file(control);
}

#[tokio::test]
async fn blueice_status_reports_the_launchers_state_in_plain_lines() {
    use blueice_launcher::control::{
        AssistantStatus, ControlReply, ControlRequest, LauncherStatus, PendingProposalStatus,
    };
    let (control, launcher) = fake_control_socket(vec![ControlReply::Status(Box::new(
        LauncherStatus {
            launcher_pid: 111,
            core_generation: 2,
            core_pid: Some(222),
            assistant: Some(AssistantStatus {
                backend: "loopback".into(),
                resident_pid: None,
                spawn_count: 3,
            }),
            pending_proposal: Some(PendingProposalStatus {
                id: 9,
                seconds_left: 41,
            }),
        },
    ))]);
    let runtime = TempDir::new();
    let data = TempDir::new();
    let downloads = TempDir::new();
    let mut cmd = command(
        Path::new(env!("CARGO_BIN_EXE_blueice-mcp-server")),
        &runtime,
        &data,
        &downloads,
    );
    cmd.arg("--launcher-control-socket").arg(&control);
    let client = ().serve(TokioChildProcess::new(cmd).expect("start")).await.expect("initialize");
    let result = client
        .call_tool(CallToolRequestParams::new("blueice_status"))
        .await
        .expect("blueice_status");
    let value = serde_json::to_value(result).unwrap();
    let text = value["content"][0]["text"].as_str().unwrap();
    for expected in [
        "launcher pid: 111",
        "core: pid 222, generation 2",
        "backend loopback, pid not running, started 3 time(s)",
        "#9 waiting for the person (41s left)",
    ] {
        assert!(text.contains(expected), "missing {expected:?}: {text}");
    }
    client.cancel().await.expect("close");
    assert_eq!(launcher.join().unwrap(), vec![ControlRequest::Status]);
    let _ = std::fs::remove_file(control);
}

#[tokio::test]
async fn an_unreachable_launcher_is_a_clear_error() {
    let runtime = TempDir::new();
    let data = TempDir::new();
    let downloads = TempDir::new();
    let mut cmd = command(
        Path::new(env!("CARGO_BIN_EXE_blueice-mcp-server")),
        &runtime,
        &data,
        &downloads,
    );
    cmd.arg("--launcher-control-socket")
        .arg("/nonexistent/control.sock");
    let client = ().serve(TokioChildProcess::new(cmd).expect("start")).await.expect("initialize");
    let outcome = client
        .call_tool(CallToolRequestParams::new("get_assistant_settings"))
        .await;
    let message = format!("{outcome:?}");
    assert!(message.contains("control socket"), "{message}");
    client.cancel().await.expect("close");
}
