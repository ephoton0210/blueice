// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

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
        ($name:expr, $arguments:expr) => {{
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

    let unobserved_project = compiler_tool!(
        "bluetsc_describe_project",
        serde_json::json!({ "project_id": 1 })
    );
    assert!(matches!(
        compiler_tool_reply(&unobserved_project),
        blueice_ipc::compiler::CompilerReply::Error {
            code: blueice_ipc::compiler::CompilerErrorCode::UnobservedProject,
            ..
        }
    ));
    let inventory_result = compiler_tool!("bluetsc_list_projects", serde_json::json!({}));
    assert_eq!(inventory_result.is_error, Some(false));
    assert_source_free_compiler_tool_result(&inventory_result);
    let blueice_ipc::compiler::CompilerReply::Projects(inventory) =
        compiler_tool_reply(&inventory_result)
    else {
        panic!("sealed core catalog must return an opaque project inventory")
    };
    assert_eq!(
        inventory.projects,
        vec![blueice_ipc::compiler::CompilerProject { id: 1 }]
    );

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

    let work_set_result = compiler_tool!(
        "bluetsc_list_work_set",
        serde_json::json!({
            "project_id": 1,
            "generation": check.generation.sequence,
            "kind": "parsed",
            "limit": 1,
        })
    );
    assert_eq!(work_set_result.is_error, Some(false));
    assert_source_free_compiler_tool_result(&work_set_result);
    let blueice_ipc::compiler::CompilerReply::WorkSetPage(work_set_page) =
        compiler_tool_reply(&work_set_result)
    else {
        panic!("the sealed core fixture must expose a bounded parsed-module page")
    };
    assert_eq!(work_set_page.generation, check.generation);
    assert_eq!(
        work_set_page.kind,
        blueice_ipc::compiler::CompilerWorkSetKind::Parsed
    );
    assert_eq!(
        work_set_page.entries,
        vec!["project:///core-fixture/main.ts".to_string()]
    );
    assert!(work_set_page.next_cursor.is_none());
    assert!(!work_set_page.truncated);

    let unobserved_work_set = compiler_tool!(
        "bluetsc_list_work_set",
        serde_json::json!({
            "project_id": 1,
            "generation": check.generation.sequence + 1,
            "kind": "parsed",
        })
    );
    assert_eq!(unobserved_work_set.is_error, Some(true));
    assert!(matches!(
        compiler_tool_reply(&unobserved_work_set),
        blueice_ipc::compiler::CompilerReply::Error {
            code: blueice_ipc::compiler::CompilerErrorCode::StaleGeneration,
            ..
        }
    ));
    let wrong_work_set_session = client
        .call_tool(
            CallToolRequestParams::new("bluetsc_list_work_set").with_arguments(
                serde_json::json!({
                    "session_id": "f".repeat(64),
                    "project_id": 1,
                    "generation": check.generation.sequence,
                    "kind": "parsed",
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .expect("wrong work-set session must receive a structured result");
    assert_eq!(wrong_work_set_session.is_error, Some(true));

    let diagnostic_page = compiler_tool!(
        "bluetsc_list_diagnostics",
        serde_json::json!({
            "project_id": 1,
            "generation": check.generation.sequence,
            "limit": 1,
        })
    );
    assert_eq!(diagnostic_page.is_error, Some(false));
    assert_source_free_compiler_tool_result(&diagnostic_page);
    let blueice_ipc::compiler::CompilerReply::DiagnosticPage(diagnostic_page) =
        compiler_tool_reply(&diagnostic_page)
    else {
        panic!("the checked core fixture must expose an exact diagnostic-page reply")
    };
    assert_eq!(diagnostic_page.generation, check.generation);
    assert!(diagnostic_page.entries.is_empty());
    assert!(diagnostic_page.next_cursor.is_none());

    let unobserved_diagnostic_generation = compiler_tool!(
        "bluetsc_list_diagnostics",
        serde_json::json!({
            "project_id": 1,
            "generation": check.generation.sequence + 1,
            "limit": 1,
        })
    );
    assert_eq!(unobserved_diagnostic_generation.is_error, Some(true));
    assert_source_free_compiler_tool_result(&unobserved_diagnostic_generation);
    assert!(matches!(
        compiler_tool_reply(&unobserved_diagnostic_generation),
        blueice_ipc::compiler::CompilerReply::Error {
            code: blueice_ipc::compiler::CompilerErrorCode::StaleGeneration,
            ..
        }
    ));

    let wrong_diagnostic_session = client
        .call_tool(
            CallToolRequestParams::new("bluetsc_list_diagnostics").with_arguments(
                serde_json::json!({
                    "session_id": "f".repeat(64),
                    "project_id": 1,
                    "generation": check.generation.sequence,
                    "limit": 1,
                })
                .as_object()
                .expect("MCP diagnostic-page arguments must be an object")
                .clone(),
            ),
        )
        .await
        .expect("mismatched diagnostic session must receive a structured MCP result");
    assert_eq!(wrong_diagnostic_session.is_error, Some(true));
    let wrong_diagnostic_session_text = wrong_diagnostic_session.content[0]
        .as_text()
        .expect("mismatched diagnostic session result must be text")
        .text
        .as_str();
    assert!(wrong_diagnostic_session_text.contains("does not belong to this MCP adapter"));
    assert!(!wrong_diagnostic_session_text.contains("coreRegisteredAnswer"));

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

    // Generation evidence alone is not dereference authority. Before this
    // receipt has received an inventory page, every metadata category must
    // reject a numerically guessed ID in the MCP adapter without passing it
    // to the real core process.
    for (tool, arguments) in [
        (
            "debug_get_provenance",
            serde_json::json!({
                "project_id": 1,
                "generation": check.generation.sequence,
                "source_id": 0,
            }),
        ),
        (
            "debug_get_type",
            serde_json::json!({
                "project_id": 1,
                "generation": check.generation.sequence,
                "id": 0,
            }),
        ),
        (
            "debug_get_symbol",
            serde_json::json!({
                "project_id": 1,
                "generation": check.generation.sequence,
                "id": 0,
            }),
        ),
        (
            "debug_get_symbol_location",
            serde_json::json!({
                "project_id": 1,
                "generation": check.generation.sequence,
                "id": 0,
                "source_id": 0,
            }),
        ),
        (
            "debug_get_contract",
            serde_json::json!({
                "project_id": 1,
                "generation": check.generation.sequence,
                "id": 0,
            }),
        ),
        (
            "debug_get_contract_location",
            serde_json::json!({
                "project_id": 1,
                "generation": check.generation.sequence,
                "id": 0,
                "source_id": 0,
            }),
        ),
        (
            "debug_validate_contract",
            serde_json::json!({
                "project_id": 1,
                "generation": check.generation.sequence,
                "id": 0,
                "value": { "enabled": true },
            }),
        ),
    ] {
        let result = compiler_tool!(tool, arguments);
        assert_eq!(result.is_error, Some(true));
        assert_source_free_compiler_tool_result(&result);
        let blueice_ipc::compiler::CompilerReply::Error { code, message } =
            compiler_tool_reply(&result)
        else {
            panic!("unobserved {tool} ID must fail before compiler IPC")
        };
        assert_eq!(
            code,
            blueice_ipc::compiler::CompilerErrorCode::UnobservedMetadata
        );
        assert!(message.contains("not observed in an inventory page"));
    }

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
        let blueice_ipc::compiler::CompilerReply::StaticProvenance(provenance) =
            compiler_tool_reply(&result)
        else {
            panic!("source provenance query must return a static provenance record")
        };
        assert!(
            provenance.content_hash.starts_with("bts-sha256:"),
            "public MCP provenance must use the labeled SHA-256 format"
        );
        assert_eq!(
            provenance.content_hash.len(),
            "bts-sha256:".len() + 64,
            "public MCP provenance digest must contain exactly 32 SHA-256 bytes"
        );
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
    let mut saw_local_symbol = false;
    let mut saw_exported_symbol = false;
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
        match symbol.name.as_str() {
            "CoreFixtureSettings" => {
                assert!(!symbol.exported);
                saw_local_symbol = true;
            }
            "coreFixtureSettings" | "coreRegisteredAnswer" => {
                assert!(symbol.exported);
                saw_exported_symbol = true;
            }
            _ => {}
        }
        symbol_source_ids.push(symbol.source_id);
        if let Some(contract_id) = symbol.contract_id {
            symbol_contract_ids.push(contract_id);
        }
    }
    assert!(saw_local_symbol && saw_exported_symbol);
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

    let symbol_location_result = compiler_tool!(
        "debug_get_symbol_location",
        serde_json::json!({
            "project_id": 1,
            "generation": check.generation.sequence,
            "id": symbol_ids[0],
            "source_id": symbol_source_ids[0],
        })
    );
    assert_eq!(symbol_location_result.is_error, Some(false));
    assert_source_free_compiler_tool_result(&symbol_location_result);
    let blueice_ipc::compiler::CompilerReply::StaticSymbolLocation(symbol_location) =
        compiler_tool_reply(&symbol_location_result)
    else {
        panic!("inventoried symbol/source pair must resolve to a location")
    };
    assert!(symbol_location.is_well_formed());
    assert_eq!(symbol_location.symbol_id, symbol_ids[0]);
    assert_eq!(symbol_location.source_id, symbol_source_ids[0]);
    let unobserved_source_result = compiler_tool!(
        "debug_get_symbol_location",
        serde_json::json!({
            "project_id": 1,
            "generation": check.generation.sequence,
            "id": symbol_ids[0],
            "source_id": u32::MAX,
        })
    );
    assert_eq!(unobserved_source_result.is_error, Some(true));
    assert!(matches!(
        compiler_tool_reply(&unobserved_source_result),
        blueice_ipc::compiler::CompilerReply::Error {
            code: blueice_ipc::compiler::CompilerErrorCode::UnobservedMetadata,
            ..
        }
    ));

    let mut contract_source_ids = Vec::new();
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
        let blueice_ipc::compiler::CompilerReply::StaticContract(contract) =
            compiler_tool_reply(&result)
        else {
            panic!("inventoried contract ID must resolve")
        };
        contract_source_ids.push(contract.source_id);
    }
    let contract_location_result = compiler_tool!(
        "debug_get_contract_location",
        serde_json::json!({
            "project_id": 1,
            "generation": check.generation.sequence,
            "id": contract_ids[0],
            "source_id": contract_source_ids[0],
        })
    );
    assert_eq!(contract_location_result.is_error, Some(false));
    assert_source_free_compiler_tool_result(&contract_location_result);
    let blueice_ipc::compiler::CompilerReply::StaticContractLocation(contract_location) =
        compiler_tool_reply(&contract_location_result)
    else {
        panic!("inventoried contract/source pair must resolve to a location")
    };
    assert!(contract_location.is_well_formed());
    assert_eq!(contract_location.contract_id, contract_ids[0]);
    assert_eq!(contract_location.source_id, contract_source_ids[0]);
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
    for (tool, id, source_id) in [
        (
            "debug_get_symbol_location",
            symbol_ids[0],
            symbol_source_ids[0],
        ),
        (
            "debug_get_contract_location",
            contract_ids[0],
            contract_source_ids[0],
        ),
    ] {
        let stale_location = compiler_tool!(
            tool,
            serde_json::json!({
                "project_id": 1,
                "generation": check.generation.sequence,
                "id": id,
                "source_id": source_id,
            })
        );
        assert_eq!(stale_location.is_error, Some(true));
        assert_source_free_compiler_tool_result(&stale_location);
        assert!(matches!(
            compiler_tool_reply(&stale_location),
            blueice_ipc::compiler::CompilerReply::Error {
                code: blueice_ipc::compiler::CompilerErrorCode::StaleGeneration,
                ..
            }
        ));
    }
    let stale_work_set = compiler_tool!(
        "bluetsc_list_work_set",
        serde_json::json!({
            "project_id": 1,
            "generation": check.generation.sequence,
            "kind": "parsed",
        })
    );
    assert_eq!(stale_work_set.is_error, Some(true));
    assert!(matches!(
        compiler_tool_reply(&stale_work_set),
        blueice_ipc::compiler::CompilerReply::Error {
            code: blueice_ipc::compiler::CompilerErrorCode::StaleGeneration,
            ..
        }
    ));
    let stale_diagnostics = compiler_tool!(
        "bluetsc_list_diagnostics",
        serde_json::json!({
            "project_id": 1,
            "generation": check.generation.sequence,
            "limit": 1,
        })
    );
    assert_eq!(stale_diagnostics.is_error, Some(true));
    assert_source_free_compiler_tool_result(&stale_diagnostics);
    assert!(matches!(
        compiler_tool_reply(&stale_diagnostics),
        blueice_ipc::compiler::CompilerReply::Error {
            code: blueice_ipc::compiler::CompilerErrorCode::StaleGeneration,
            ..
        }
    ));
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
