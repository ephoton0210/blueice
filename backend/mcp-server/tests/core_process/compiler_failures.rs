// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

fn project(name: &str, source: &str) -> CompilerCatalogProject {
    let root = format!("project:///{name}");
    let entry = format!("{root}/main.ts");
    CompilerCatalogProject {
        canonical_project_root: root.clone(),
        canonical_config_root: format!("{root}/blue-ts.json"),
        canonical_output_root: format!("project:///{name}-dist"),
        entry_module: entry.clone(),
        modules: vec![CompilerCatalogModule {
            canonical_id: entry,
            text: source.into(),
        }],
        expose_to_compiler_ipc: true,
        grant_output_write: false,
        resolutions: Vec::new(),
        options: CompilerCatalogOptions::default(),
    }
}

#[tokio::test]
async fn real_mcp_client_inspects_diagnostic_type_and_contract_failure() {
    let catalog = CompilerCatalogBootstrap {
        version: COMPILER_CATALOG_BOOTSTRAP_VERSION,
        projects: vec![
            project("diagnostic", "export const broken: number = 'wrong';"),
            project(
                "contract",
                "interface Settings { enabled: boolean; } \
                 export const settings: Settings = { enabled: true };",
            ),
        ],
    };
    let mut launcher = LauncherManagedCore::spawn_with_catalog(Some(catalog));
    let server = BlueIceMcpServer::connect_with_core_and_compiler_sockets(
        &launcher.rendezvous_socket,
        &launcher.compiler_socket,
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
    let session = compiler_session_id(
        &client
            .call_tool(CallToolRequestParams::new("bluetsc_session_capabilities"))
            .await
            .unwrap(),
    );
    let request = |name: &str, mut arguments: serde_json::Value| {
        arguments["session_id"] = serde_json::Value::String(session.clone());
        CallToolRequestParams::new(name.to_string())
            .with_arguments(arguments.as_object().unwrap().clone())
    };
    let inventory = client
        .call_tool(request("bluetsc_list_projects", serde_json::json!({})))
        .await
        .unwrap();
    let blueice_ipc::compiler::CompilerReply::Projects(inventory) = compiler_tool_reply(&inventory)
    else {
        panic!("the owner catalog must expose two opaque projects")
    };
    assert_eq!(inventory.projects.len(), 2);
    let mut diagnostic_id = None;
    let mut contract_id = None;
    for project in inventory.projects {
        let description = client
            .call_tool(request(
                "bluetsc_describe_project",
                serde_json::json!({ "project_id": project.id }),
            ))
            .await
            .unwrap();
        let blueice_ipc::compiler::CompilerReply::Project(identity) =
            compiler_tool_reply(&description)
        else {
            panic!("an inventoried project must have an exact description")
        };
        if identity.entry_module.contains("/diagnostic/") {
            diagnostic_id = Some(project.id);
        } else if identity.entry_module.contains("/contract/") {
            contract_id = Some(project.id);
        }
    }
    let diagnostic_id = diagnostic_id.unwrap();
    let contract_id = contract_id.unwrap();

    let diagnostic_check = client
        .call_tool(request(
            "bluetsc_check",
            serde_json::json!({ "project_id": diagnostic_id }),
        ))
        .await
        .unwrap();
    let blueice_ipc::compiler::CompilerReply::Check(diagnostic_check) =
        compiler_tool_reply(&diagnostic_check)
    else {
        panic!("the invalid project must return a check")
    };
    assert!(diagnostic_check.has_errors);
    let diagnostics = client
        .call_tool(request(
            "bluetsc_list_diagnostics",
            serde_json::json!({
                "project_id": diagnostic_id,
                "generation": diagnostic_check.generation.sequence,
                "limit": 4,
            }),
        ))
        .await
        .unwrap();
    assert_source_free_compiler_tool_result(&diagnostics);
    assert!(!diagnostics.content[0]
        .as_text()
        .unwrap()
        .text
        .contains("export const broken"));
    let blueice_ipc::compiler::CompilerReply::DiagnosticPage(page) =
        compiler_tool_reply(&diagnostics)
    else {
        panic!("the invalid project must expose its retained diagnostic")
    };
    assert_eq!(page.generation, diagnostic_check.generation);
    assert!(!page.entries.is_empty());
    assert!(page.entries.iter().any(|entry| {
        entry.severity == blueice_ipc::compiler::CompilerDiagnosticSeverity::Error
    }));

    let contract_check = client
        .call_tool(request(
            "bluetsc_check",
            serde_json::json!({ "project_id": contract_id }),
        ))
        .await
        .unwrap();
    let blueice_ipc::compiler::CompilerReply::Check(contract_check) =
        compiler_tool_reply(&contract_check)
    else {
        panic!("the valid project must return a check")
    };
    assert!(!contract_check.has_errors);
    let generation = contract_check.generation.sequence;
    let type_inventory = client
        .call_tool(request(
            "debug_list_static_metadata",
            serde_json::json!({
                "project_id": contract_id,
                "generation": generation,
                "kind": "types",
            }),
        ))
        .await
        .unwrap();
    let blueice_ipc::compiler::CompilerReply::StaticMetadataPage(type_page) =
        compiler_tool_reply(&type_inventory)
    else {
        panic!("the valid project must inventory static types")
    };
    let type_id = *type_page.ids.first().unwrap();
    let type_result = client
        .call_tool(request(
            "debug_get_type",
            serde_json::json!({
                "project_id": contract_id,
                "generation": generation,
                "id": type_id,
            }),
        ))
        .await
        .unwrap();
    assert_source_free_compiler_tool_result(&type_result);
    assert!(!type_result.content[0]
        .as_text()
        .unwrap()
        .text
        .contains("interface Settings"));
    let blueice_ipc::compiler::CompilerReply::StaticType(static_type) =
        compiler_tool_reply(&type_result)
    else {
        panic!("the inventoried static type must be inspectable")
    };
    assert!(!static_type.display.is_empty());

    let contract_inventory = client
        .call_tool(request(
            "debug_list_static_metadata",
            serde_json::json!({
                "project_id": contract_id,
                "generation": generation,
                "kind": "contracts",
            }),
        ))
        .await
        .unwrap();
    let blueice_ipc::compiler::CompilerReply::StaticMetadataPage(contract_page) =
        compiler_tool_reply(&contract_inventory)
    else {
        panic!("the valid project must inventory static contracts")
    };
    let contract_handle = *contract_page.ids.first().unwrap();
    let invalid_value = client
        .call_tool(request(
            "debug_validate_contract",
            serde_json::json!({
                "project_id": contract_id,
                "generation": generation,
                "id": contract_handle,
                "value": { "enabled": "wrong" },
            }),
        ))
        .await
        .unwrap();
    assert_source_free_compiler_tool_result(&invalid_value);
    assert!(!invalid_value.content[0]
        .as_text()
        .unwrap()
        .text
        .contains("interface Settings"));
    let blueice_ipc::compiler::CompilerReply::ContractValidation(validation) =
        compiler_tool_reply(&invalid_value)
    else {
        panic!("a contract mismatch must return a validation result")
    };
    assert!(!validation.valid);
    assert!(validation.failure.is_some());

    client.cancel().await.unwrap();
    server_task.await.unwrap();
    launcher.shutdown();
}
