// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use blueice_ipc::compiler_catalog::{
    write_compiler_catalog, CompilerCatalogBootstrap, CompilerCatalogModule,
    CompilerCatalogOptions, CompilerCatalogProject, COMPILER_CATALOG_BOOTSTRAP_VERSION,
};
use blueice_ipc::compiler_output::{CompilerOutputErrorCode, CompilerOutputReply};

fn output_reply(result: &rmcp::model::CallToolResult) -> CompilerOutputReply {
    let text = &result.content[0].as_text().unwrap().text;
    let marker = format!("{}\n", blueice_mcp_server::UNTRUSTED_CONTENT_MARKER);
    let (_, json) = text.split_once(&marker).unwrap();
    let value: serde_json::Value = serde_json::from_str(json).unwrap();
    serde_json::from_value(value["reply"].clone()).unwrap()
}

#[tokio::test]
async fn real_mcp_build_requires_the_independent_owner_output_receipt() {
    let directory = std::env::temp_dir().join(format!(
        "blueice-mcp-output-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let directory = directory.canonicalize().unwrap();
    let project = directory.join("project");
    let output_root = directory.join("output");
    std::fs::create_dir(&project).unwrap();
    std::fs::create_dir(&output_root).unwrap();
    let config = project.join("blue-ts.json");
    let entry = project.join("main.ts");
    std::fs::write(&config, "{}").unwrap();
    std::fs::write(&entry, "export const answer: number = 42;").unwrap();
    let catalog = CompilerCatalogBootstrap {
        version: COMPILER_CATALOG_BOOTSTRAP_VERSION,
        projects: vec![CompilerCatalogProject {
            canonical_project_root: project.to_str().unwrap().into(),
            canonical_config_root: config.to_str().unwrap().into(),
            canonical_output_root: output_root.to_str().unwrap().into(),
            entry_module: entry.to_str().unwrap().into(),
            modules: vec![CompilerCatalogModule {
                canonical_id: entry.to_str().unwrap().into(),
                text: "export const answer: number = 42;".into(),
            }],
            expose_to_compiler_ipc: true,
            grant_output_write: true,
            resolutions: Vec::new(),
            options: CompilerCatalogOptions::default(),
        }],
    };
    let core_socket = unique_socket_path("output-core");
    let query_socket = unique_socket_path("output-query");
    let output_socket = unique_socket_path("output-write");
    let frames = directory.join("frames");
    let mut core = Command::new(sibling_core_binary())
        .args([
            "--socket",
            core_socket.to_str().unwrap(),
            "--compiler-socket",
            query_socket.to_str().unwrap(),
            "--compiler-output-socket",
            output_socket.to_str().unwrap(),
            "--compiler-catalog-stdin",
            "--frame-dir",
            frames.to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    write_compiler_catalog(&mut core.stdin.take().unwrap(), &catalog).unwrap();
    assert!(wait_for_socket(&core_socket));
    assert!(wait_for_socket(&query_socket));
    assert!(wait_for_socket(&output_socket));
    let server = BlueIceMcpServer::connect_with_core_compiler_and_output_sockets(
        &core_socket,
        &query_socket,
        &output_socket,
    )
    .unwrap();
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        server
            .serve(server_transport)
            .await
            .unwrap()
            .waiting()
            .await
            .unwrap();
    });
    let client = CompilerMcpClient.serve(client_transport).await.unwrap();
    let tools = client.list_tools(None).await.unwrap();
    assert!(tools.tools.iter().any(|tool| tool.name == "bluetsc_build"));
    let output_capabilities = client
        .call_tool(CallToolRequestParams::new("bluetsc_output_capabilities"))
        .await
        .unwrap();
    let output_capabilities: serde_json::Value =
        serde_json::from_str(&output_capabilities.content[0].as_text().unwrap().text).unwrap();
    let output_session = output_capabilities["output_session"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(output_session.starts_with("ow-"));
    let query_session = compiler_session_id(
        &client
            .call_tool(CallToolRequestParams::new("bluetsc_session_capabilities"))
            .await
            .unwrap(),
    );
    let call = |name: &str, arguments: serde_json::Value| {
        CallToolRequestParams::new(name.to_string())
            .with_arguments(arguments.as_object().unwrap().clone())
    };
    let query_inventory = client
        .call_tool(call(
            "bluetsc_list_projects",
            serde_json::json!({ "session_id": query_session }),
        ))
        .await
        .unwrap();
    assert_eq!(query_inventory.is_error, Some(false));
    let check = client
        .call_tool(call(
            "bluetsc_check",
            serde_json::json!({ "session_id": query_session, "project_id": 1 }),
        ))
        .await
        .unwrap();
    let blueice_ipc::compiler::CompilerReply::Check(check) = compiler_tool_reply(&check) else {
        panic!("query check must succeed before build")
    };
    let before_inventory = client
        .call_tool(call(
            "bluetsc_build",
            serde_json::json!({ "output_session_id": output_session, "project_id": 1 }),
        ))
        .await
        .unwrap();
    assert!(matches!(
        output_reply(&before_inventory),
        CompilerOutputReply::Error {
            code: CompilerOutputErrorCode::UnobservedProject,
            ..
        }
    ));
    let wrong_receipt = client
        .call_tool(call(
            "bluetsc_list_output_projects",
            serde_json::json!({ "output_session_id": query_session }),
        ))
        .await
        .unwrap();
    assert_eq!(wrong_receipt.is_error, Some(true));
    let output_inventory = client
        .call_tool(call(
            "bluetsc_list_output_projects",
            serde_json::json!({ "output_session_id": output_session }),
        ))
        .await
        .unwrap();
    let CompilerOutputReply::Projects(inventory) = output_reply(&output_inventory) else {
        panic!("output owner must inventory its granted project")
    };
    assert_eq!(inventory.projects.len(), 1);
    let query_receipt_build = client
        .call_tool(call(
            "bluetsc_build",
            serde_json::json!({
                "output_session_id": query_session,
                "project_id": inventory.projects[0].id,
            }),
        ))
        .await
        .unwrap();
    assert_eq!(query_receipt_build.is_error, Some(true));
    assert_eq!(std::fs::read_dir(&output_root).unwrap().count(), 0);
    let build = client
        .call_tool(call(
            "bluetsc_build",
            serde_json::json!({
                "output_session_id": output_session,
                "project_id": inventory.projects[0].id,
            }),
        ))
        .await
        .unwrap();
    let CompilerOutputReply::Build(result) = output_reply(&build) else {
        panic!("owner-granted MCP build must publish")
    };
    assert!(result.published);
    assert_eq!(
        result.generation.sequence,
        check.generation.sequence + 1,
        "the rejected query-receipt build must not advance the core generation"
    );
    assert_eq!(std::fs::read_dir(&output_root).unwrap().count(), 1);
    assert!(!build.content[0]
        .as_text()
        .unwrap()
        .text
        .contains(output_root.to_str().unwrap()));
    let stale = client
        .call_tool(call(
            "bluetsc_list_diagnostics",
            serde_json::json!({
                "session_id": query_session,
                "project_id": 1,
                "generation": check.generation.sequence,
            }),
        ))
        .await
        .unwrap();
    assert!(matches!(
        compiler_tool_reply(&stale),
        blueice_ipc::compiler::CompilerReply::Error {
            code: blueice_ipc::compiler::CompilerErrorCode::StaleGeneration,
            ..
        }
    ));
    client.cancel().await.unwrap();
    server_task.await.unwrap();
    assert!(core.wait().unwrap().success());
    std::fs::remove_dir_all(directory).unwrap();
}
