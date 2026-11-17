// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Owner-selected programs survive the real launcher/core/IPC/MCP route.

use super::*;
use blueice_ipc::compiler::{CompilerErrorCode, CompilerReply};
use serde_json::json;

fn project(name: &str, exposed: bool) -> CompilerCatalogProject {
    let root = format!("project:///{name}");
    let entry = format!("{root}/main.ts");
    let consumer = format!("{root}/consumer.ts");
    CompilerCatalogProject {
        canonical_project_root: root.clone(),
        canonical_config_root: format!("{root}/tsconfig.json"),
        canonical_output_root: format!("project:///{name}-output"),
        entry_module: entry.clone(),
        program_roots: Some(vec![entry.clone(), consumer.clone()]),
        modules: vec![
            CompilerCatalogModule {
                canonical_id: entry,
                text: "const value: number = 1;".into(),
            },
            CompilerCatalogModule {
                canonical_id: consumer,
                text: "const result: number = value;".into(),
            },
            CompilerCatalogModule {
                canonical_id: format!("{root}/unselected.ts"),
                text: "const value: string = 'poison';".into(),
            },
        ],
        expose_to_compiler_ipc: exposed,
        grant_output_write: false,
        resolutions: Vec::new(),
        options: CompilerCatalogOptions {
            declaration: true,
            ..CompilerCatalogOptions::default()
        },
    }
}

#[tokio::test]
async fn sealed_program_roots_remain_complete_and_generation_bound_through_mcp_cutover() {
    let mut launcher = LauncherManagedCore::spawn_with_catalog(Some(CompilerCatalogBootstrap {
        version: COMPILER_CATALOG_BOOTSTRAP_VERSION,
        projects: vec![project("mcp-program", true), project("mcp-hidden", false)],
    }));
    let mut previous_session = None;
    for round in 0..2 {
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
        if let Some(previous) = &previous_session {
            assert_ne!(previous, &session);
            let replay = client
                .call_tool(
                    CallToolRequestParams::new("bluetsc_list_projects").with_arguments(
                        json!({"session_id":previous}).as_object().unwrap().clone(),
                    ),
                )
                .await
                .unwrap();
            assert_eq!(replay.is_error, Some(true));
        }
        macro_rules! tool {
            ($name:expr, $arguments:expr) => {{
                let mut arguments = $arguments;
                arguments["session_id"] = json!(session);
                let result = client
                    .call_tool(
                        CallToolRequestParams::new($name)
                            .with_arguments(arguments.as_object().unwrap().clone()),
                    )
                    .await
                    .unwrap();
                assert_source_free_compiler_tool_result(&result);
                let text = &result.content[0].as_text().unwrap().text;
                for forbidden in [
                    "const value: number = 1;",
                    "const result: number = value;",
                    "poison",
                    "unselected.ts",
                    "mcp-hidden",
                    "mcp-program-output",
                ] {
                    assert!(!text.contains(forbidden), "sealed content leaked: {text}");
                }
                result
            }};
        }
        let inventory = tool!("bluetsc_list_projects", json!({}));
        let CompilerReply::Projects(inventory) = compiler_tool_reply(&inventory) else {
            panic!("MCP must inventory only owner-exposed programs");
        };
        assert_eq!(
            inventory.projects,
            vec![blueice_ipc::compiler::CompilerProject { id: 1 }]
        );
        let hidden = tool!("bluetsc_check", json!({"project_id":2}));
        assert!(matches!(
            compiler_tool_reply(&hidden),
            CompilerReply::Error {
                code: CompilerErrorCode::UnobservedProject,
                ..
            }
        ));
        for (field, value) in [
            ("roots", json!(["project:///outside.ts"])),
            ("program_roots", json!(["project:///outside.ts"])),
            ("selected_roots", json!(["project:///outside.ts"])),
            ("entry_module", json!("project:///outside.ts")),
            ("sources", json!({"outside.ts":"const injected = 1;"})),
            ("resolver_edges", json!([])),
            ("options", json!({"noEmit":false})),
            ("write_capability", json!(true)),
        ] {
            let mut arguments = json!({"session_id":session,"project_id":1});
            arguments[field] = value;
            let result = client
                .call_tool(
                    CallToolRequestParams::new("bluetsc_check")
                        .with_arguments(arguments.as_object().unwrap().clone()),
                )
                .await;
            assert!(
                match result {
                    Err(_) => true,
                    Ok(result) => result.is_error == Some(true),
                },
                "MCP accepted caller-supplied {field}"
            );
        }
        let result = tool!("bluetsc_check", json!({"project_id":1}));
        let CompilerReply::Check(cold) = compiler_tool_reply(&result) else {
            panic!("the complete selected program must produce one generation");
        };
        assert!(!cold.has_errors, "{:?}", cold.diagnostics);
        assert_eq!(cold.generation.sequence, 1, "refusals reached the compiler");
        assert!(!cold.cache_hit);
        assert_eq!(cold.static_metadata.as_ref().unwrap().source_count, 2);
        assert_eq!(
            cold.artifact_fingerprint.as_deref(),
            Some(cold.project_fingerprint.as_str())
        );
        let result = tool!(
            "bluetsc_list_work_set",
            json!({"project_id":1,
            "generation":cold.generation.sequence,"kind":"parsed","limit":10})
        );
        let CompilerReply::WorkSetPage(parsed) = compiler_tool_reply(&result) else {
            panic!("MCP must retain the selected-program work set");
        };
        assert_eq!(
            parsed.entries,
            vec![
                "project:///mcp-program/consumer.ts",
                "project:///mcp-program/main.ts"
            ]
        );
        let first = tool!(
            "debug_list_static_metadata",
            json!({"project_id":1,
            "generation":cold.generation.sequence,"kind":"sources","limit":1})
        );
        let CompilerReply::StaticMetadataPage(first) = compiler_tool_reply(&first) else {
            panic!("source inventory must be bounded and generation-bound");
        };
        assert_eq!(first.ids.len(), 1);
        let cursor = first.next_cursor.unwrap();
        let second = tool!(
            "debug_list_static_metadata",
            json!({"project_id":1,
            "generation":cold.generation.sequence,"kind":"sources","limit":1,"cursor":{"id":cursor.id}})
        );
        let CompilerReply::StaticMetadataPage(second) = compiler_tool_reply(&second) else {
            panic!("source cursor must complete this same program generation");
        };
        assert_eq!(second.ids.len(), 1);
        assert!(second.next_cursor.is_none());
        let source_ids = [first.ids[0], second.ids[0]];
        assert_ne!(source_ids[0], source_ids[1]);
        let mut owners = Vec::new();
        for source_id in source_ids {
            let result = tool!(
                "debug_get_provenance",
                json!({"project_id":1,
                "generation":cold.generation.sequence,"source_id":source_id})
            );
            let CompilerReply::StaticProvenance(source) = compiler_tool_reply(&result) else {
                panic!("inventoried source must keep its exact owner");
            };
            assert_eq!(source.generation, cold.generation);
            assert!(source.content_hash.starts_with("bts-sha256:"));
            owners.push(source.module);
        }
        owners.sort();
        assert_eq!(owners, parsed.entries);
        let pending = tool!(
            "debug_list_static_metadata",
            json!({"project_id":1,"generation":cold.generation.sequence,"kind":"sources","limit":1})
        );
        let CompilerReply::StaticMetadataPage(pending) = compiler_tool_reply(&pending) else {
            panic!("an unconsumed cursor must remain bound to the cold generation");
        };
        let pending_cursor = pending.next_cursor.unwrap();
        let result = tool!("bluetsc_check", json!({"project_id":1}));
        let CompilerReply::Check(warm) = compiler_tool_reply(&result) else {
            panic!("the immutable selected program must support warm checking");
        };
        assert!(warm.cache_hit);
        assert_eq!(warm.generation.sequence, 2);
        assert_eq!(warm.project_fingerprint, cold.project_fingerprint);
        assert_eq!(warm.static_metadata.as_ref().unwrap().source_count, 2);
        for (name, arguments) in [
            ("bluetsc_list_work_set", json!({"kind":"parsed"})),
            ("debug_get_provenance", json!({"source_id":source_ids[0]})),
            (
                "debug_list_static_metadata",
                json!({"kind":"sources","cursor":{"id":pending_cursor.id}}),
            ),
        ] {
            let mut arguments = arguments;
            arguments["project_id"] = json!(1);
            arguments["generation"] = json!(cold.generation.sequence);
            let result = tool!(name, arguments);
            assert!(matches!(
                compiler_tool_reply(&result),
                CompilerReply::Error {
                    code: CompilerErrorCode::StaleGeneration,
                    ..
                }
            ));
        }
        let tools = client.list_tools(None).await.unwrap();
        for name in [
            "bluetsc_output_capabilities",
            "bluetsc_list_output_projects",
            "bluetsc_build",
        ] {
            assert!(tools.tools.iter().all(|tool| tool.name != name));
        }
        let build = client
            .call_tool(
                CallToolRequestParams::new("bluetsc_build").with_arguments(
                    json!({"output_session_id":session,"project_id":1})
                        .as_object()
                        .unwrap()
                        .clone(),
                ),
            )
            .await;
        assert!(match build {
            Err(_) => true,
            Ok(result) => result.is_error == Some(true),
        });
        previous_session = Some(session);
        client.cancel().await.unwrap();
        server_task.await.unwrap();
        if round == 0 {
            launcher.cutover();
        }
    }
    launcher.shutdown();
}
