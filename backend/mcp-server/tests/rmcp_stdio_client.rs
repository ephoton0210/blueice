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
