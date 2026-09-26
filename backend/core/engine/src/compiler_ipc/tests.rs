// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use blueice_bluets::{
    AuthorizedModule, AuthorizedModuleLoader, AuthorizedModuleResolution, CompilerOptions,
    RuntimePolicy,
};

mod inventory_tests {
    include!("inventory_tests.rs");
}

const ENTRY: &str = "project:///app/main.ts";
const DEPENDENCY: &str = "project:///app/math.ts";

fn registration(source: &str) -> RegisteredProjectRegistration {
    RegisteredProjectRegistration {
        canonical_project_root: "project:///app".to_string(),
        canonical_config_root: "project:///app/blue-ts.json".to_string(),
        canonical_output_root: "project:///dist".to_string(),
        entry_module: ENTRY.to_string(),
        loader: AuthorizedModuleLoader::new(
            [
                AuthorizedModule::new(
                    ENTRY,
                    format!("import {{ answer }} from './math'; {source}"),
                ),
                AuthorizedModule::new(DEPENDENCY, "export const answer: number = 42;"),
            ],
            [AuthorizedModuleResolution::new(ENTRY, "./math", DEPENDENCY)],
        )
        .unwrap(),
        compiler_options: CompilerOptions {
            resolver_fingerprint: "registered-project-resolver-v1".to_string(),
            runtime_policy: RuntimePolicy::Checked,
            ..CompilerOptions::default()
        },
    }
}

fn adapter() -> (CompilerServiceIpcAdapter, CompilerProject) {
    let mut adapter = CompilerServiceIpcAdapter::default();
    let project = adapter
        .register_core_project(registration("export const value: number = answer;"))
        .unwrap();
    (adapter, project)
}

fn inventory_on_stream(
    adapter: &mut CompilerServiceIpcAdapter,
    stream: &str,
    project: CompilerProject,
) {
    let CompilerReply::Projects(inventory) =
        adapter.handle_session_request(stream, CompilerRequest::ListProjects)
    else {
        panic!("accepted stream must receive sealed project inventory")
    };
    assert!(inventory.is_well_formed());
    assert!(inventory.projects.contains(&project));
}

#[test]
fn sealed_project_inventory_is_bounded_and_stream_local() {
    let (mut adapter, first) = adapter();
    let mut second_registration = registration("export const next: number = answer;");
    second_registration.canonical_project_root = "project:///next".to_string();
    second_registration.canonical_config_root = "project:///next/blue-ts.json".to_string();
    let second = adapter.register_core_project(second_registration).unwrap();
    let first_stream = "a".repeat(CompilerSessionAttestation::ID_LENGTH);
    let second_stream = "b".repeat(CompilerSessionAttestation::ID_LENGTH);
    for (stream, project) in [(&first_stream, first), (&second_stream, second)] {
        assert!(matches!(
            adapter.handle_session_request(stream, CompilerRequest::DescribeProject { project }),
            CompilerReply::Error {
                code: CompilerErrorCode::UnobservedProject,
                ..
            }
        ));
    }
    let CompilerReply::Projects(inventory) =
        adapter.handle_session_request(&first_stream, CompilerRequest::ListProjects)
    else {
        panic!("sealed project inventory must be available")
    };
    assert_eq!(inventory.projects, vec![first, second]);
    let mut later_registration = registration("export const later: number = answer;");
    later_registration.canonical_project_root = "project:///later".to_string();
    later_registration.canonical_config_root = "project:///later/blue-ts.json".to_string();
    let later = adapter.register_core_project(later_registration).unwrap();
    assert!(matches!(
        adapter.handle_session_request(
            &first_stream,
            CompilerRequest::DescribeProject { project: later }
        ),
        CompilerReply::Error {
            code: CompilerErrorCode::UnobservedProject,
            ..
        }
    ));
    assert!(matches!(
        adapter.handle_session_request(
            &first_stream,
            CompilerRequest::DescribeProject { project: first }
        ),
        CompilerReply::Project(_)
    ));
    assert!(matches!(
        adapter.handle_session_request(&second_stream, CompilerRequest::Check { project: first }),
        CompilerReply::Error {
            code: CompilerErrorCode::UnobservedProject,
            ..
        }
    ));
    assert!(matches!(
        adapter.handle_session_request(
            &first_stream,
            CompilerRequest::Check {
                project: CompilerProject { id: u64::MAX }
            }
        ),
        CompilerReply::Error {
            code: CompilerErrorCode::UnobservedProject,
            ..
        }
    ));
    adapter.end_session(&first_stream);
    assert!(matches!(
        adapter.handle_session_request(&first_stream, CompilerRequest::Check { project: first }),
        CompilerReply::Error {
            code: CompilerErrorCode::UnobservedProject,
            ..
        }
    ));
}

#[test]
fn opaque_project_queries_return_generation_bound_source_free_metadata() {
    let (mut adapter, project) = adapter();
    assert_eq!(
        adapter.handle(CompilerRequest::DescribeProject { project }),
        CompilerReply::Project(CompilerProjectIdentity {
            project,
            entry_module: ENTRY.to_string(),
        })
    );

    let CompilerReply::Check(check) = adapter.handle(CompilerRequest::Check { project }) else {
        panic!("registered project must check through adapter")
    };
    assert!(!check.has_errors, "{check:#?}");
    assert!(!check.parsed_modules.truncated);
    assert!(check
        .parsed_modules
        .entries
        .iter()
        .any(|module| module == ENTRY));
    assert!(check.artifact_fingerprint.is_some());
    let metadata = check.static_metadata.as_ref().unwrap();
    assert_eq!(metadata.source_count, 2);
    assert!(metadata.type_count > 0);
    assert!(metadata.symbol_count > 0);

    let CompilerReply::StaticType(static_type) = adapter.handle(CompilerRequest::GetStaticType {
        generation: check.generation,
        type_id: 0,
    }) else {
        panic!("checked type must be generation-addressable")
    };
    assert_eq!(static_type.generation, check.generation);
    // The first deterministic type belongs to the imported binding. Its
    // checked static shape is intentionally `unknown` until a richer
    // import type surface is implemented; this still proves a precise,
    // generation-bound type lookup rather than a runtime-value query.
    assert_eq!(static_type.display, "unknown");

    let CompilerReply::StaticSymbol(symbol) = adapter.handle(CompilerRequest::GetStaticSymbol {
        generation: check.generation,
        symbol_id: 0,
    }) else {
        panic!("checked symbol must be generation-addressable")
    };
    assert_eq!(symbol.generation, check.generation);
    assert_eq!(symbol.kind, CompilerSymbolKind::Import);
    assert!(!symbol.exported);
    assert_eq!(symbol.module, ENTRY);
    assert_ne!(symbol.module, "import { answer } from './math';");
    let CompilerReply::StaticSymbol(exported) = adapter.handle(CompilerRequest::GetStaticSymbol {
        generation: check.generation,
        symbol_id: 1,
    }) else {
        panic!("exported declaration must be generation-addressable")
    };
    assert_eq!(exported.name, "value");
    assert!(exported.exported);
}

#[test]
fn declaration_locations_require_exact_generation_and_source_ownership() {
    let mut adapter = CompilerServiceIpcAdapter::default();
    let project = adapter
        .register_core_project(registration(
            "export interface Shape { value: number; }\r\n/* 🚀 */ const value: number = answer;",
        ))
        .unwrap();
    let CompilerReply::Check(check) = adapter.handle(CompilerRequest::Check { project }) else {
        panic!("registered project must check")
    };
    assert!(!check.has_errors, "{check:#?}");
    let symbol_count = check.static_metadata.as_ref().unwrap().symbol_count;
    let symbols: Vec<_> = (0..symbol_count)
        .filter_map(|symbol_id| {
            match adapter.handle(CompilerRequest::GetStaticSymbol {
                generation: check.generation,
                symbol_id,
            }) {
                CompilerReply::StaticSymbol(symbol) => Some(symbol),
                _ => None,
            }
        })
        .collect();
    let variable = symbols
        .iter()
        .find(|symbol| symbol.name == "value")
        .unwrap();
    let interface = symbols
        .iter()
        .find(|symbol| symbol.name == "Shape")
        .unwrap();
    let contract_id = interface.contract_id.expect("interface is reifiable");
    let CompilerReply::StaticSymbolLocation(location) =
        adapter.handle(CompilerRequest::GetStaticSymbolLocation {
            generation: check.generation,
            symbol_id: variable.id,
            source_id: variable.source_id,
        })
    else {
        panic!("symbol location must be available")
    };
    assert!(location.is_well_formed());
    assert_eq!(location.coordinates.start_line, 1);
    assert_eq!(location.coordinates.start_column_utf16, 9);
    assert!(!format!("{location:?}").contains("value"));
    let CompilerReply::StaticContractLocation(contract_location) =
        adapter.handle(CompilerRequest::GetStaticContractLocation {
            generation: check.generation,
            contract_id,
            source_id: interface.source_id,
        })
    else {
        panic!("contract location must be available")
    };
    assert!(contract_location.is_well_formed());
    assert_eq!(contract_location.coordinates.start_line, 0);
    assert!(!format!("{contract_location:?}").contains("Shape"));
    assert!(matches!(
        adapter.handle(CompilerRequest::GetStaticSymbolLocation {
            generation: check.generation,
            symbol_id: variable.id,
            source_id: variable.source_id.wrapping_add(1),
        }),
        CompilerReply::Error {
            code: CompilerErrorCode::InvalidLocationTarget,
            ..
        }
    ));
    assert!(matches!(
        adapter.handle(CompilerRequest::GetStaticContractLocation {
            generation: check.generation,
            contract_id,
            source_id: interface.source_id.wrapping_add(1),
        }),
        CompilerReply::Error {
            code: CompilerErrorCode::InvalidLocationTarget,
            ..
        }
    ));
    let CompilerReply::Check(next) = adapter.handle(CompilerRequest::Check { project }) else {
        panic!("second check must succeed")
    };
    assert_ne!(next.generation, check.generation);
    assert!(matches!(
        adapter.handle(CompilerRequest::GetStaticSymbolLocation {
            generation: check.generation,
            symbol_id: variable.id,
            source_id: variable.source_id,
        }),
        CompilerReply::Error {
            code: CompilerErrorCode::StaleGeneration,
            ..
        }
    ));
}

#[test]
fn later_check_invalidates_an_old_generation_without_retargeting() {
    let (mut adapter, project) = adapter();
    let CompilerReply::Check(first) = adapter.handle(CompilerRequest::Check { project }) else {
        panic!("first check must succeed")
    };
    let CompilerReply::Check(second) = adapter.handle(CompilerRequest::Check { project }) else {
        panic!("second check must succeed")
    };
    assert_ne!(first.generation, second.generation);
    assert!(matches!(
        adapter.handle(CompilerRequest::GetStaticType {
            generation: first.generation,
            type_id: 0,
        }),
        CompilerReply::Error {
            code: CompilerErrorCode::StaleGeneration,
            ..
        }
    ));
}

#[test]
fn adapter_rejects_over_budget_or_non_finite_contract_snapshots() {
    let mut adapter = CompilerServiceIpcAdapter::default();
    let project = adapter
        .register_core_project(registration(
            "type Name = string; export const name: Name = 'blueice';",
        ))
        .unwrap();
    let CompilerReply::Check(check) = adapter.handle(CompilerRequest::Check { project }) else {
        panic!("registered project must check")
    };
    let CompilerReply::StaticSymbol(symbol) = adapter.handle(CompilerRequest::GetStaticSymbol {
        generation: check.generation,
        symbol_id: 1,
    }) else {
        panic!("type alias symbol must be retained")
    };
    let contract_id = symbol.contract_id.unwrap();
    let oversized = CompilerContractValue::String("x".repeat(256 * 1_024 + 1));
    assert!(matches!(
        adapter.handle(CompilerRequest::ValidateStaticContract {
            generation: check.generation,
            contract_id,
            value: oversized,
        }),
        CompilerReply::Error {
            code: CompilerErrorCode::InvalidContractValue,
            ..
        }
    ));
    assert!(matches!(
        adapter.handle(CompilerRequest::ValidateStaticContract {
            generation: check.generation,
            contract_id,
            value: CompilerContractValue::Number("NaN".to_string()),
        }),
        CompilerReply::Error {
            code: CompilerErrorCode::InvalidContractValue,
            ..
        }
    ));
}

#[test]
fn malformed_or_unknown_handles_never_create_or_select_a_project() {
    let (mut adapter, _) = adapter();
    assert!(matches!(
        adapter.handle(CompilerRequest::Check {
            project: CompilerProject { id: 0 },
        }),
        CompilerReply::Error {
            code: CompilerErrorCode::InvalidProject,
            ..
        }
    ));
    assert!(matches!(
        adapter.handle(CompilerRequest::DescribeProject {
            project: CompilerProject { id: 999 },
        }),
        CompilerReply::Error {
            code: CompilerErrorCode::InvalidProject,
            ..
        }
    ));
    assert!(matches!(
        adapter.handle(CompilerRequest::GetStaticSymbol {
            generation: CompilerGeneration {
                project: CompilerProject { id: 1 },
                sequence: 0,
            },
            symbol_id: 0,
        }),
        CompilerReply::Error {
            code: CompilerErrorCode::StaleGeneration,
            ..
        }
    ));
}

#[test]
fn adapter_refuses_an_over_budget_field_without_exposing_partial_data() {
    let mut adapter = CompilerServiceIpcAdapter::new(
        RegisteredProjectCompilerService::default(),
        CompilerServiceIpcLimits {
            max_field_bytes: 4,
            ..CompilerServiceIpcLimits::default()
        },
    )
    .unwrap();
    let project = adapter
        .register_core_project(registration("export const value: number = answer;"))
        .unwrap();
    assert!(matches!(
        adapter.handle(CompilerRequest::DescribeProject { project }),
        CompilerReply::Error {
            code: CompilerErrorCode::ResourceLimit,
            ..
        }
    ));
    assert!(matches!(
        adapter.handle(CompilerRequest::Check { project }),
        CompilerReply::Error {
            code: CompilerErrorCode::ResourceLimit,
            ..
        }
    ));
}

#[test]
fn check_marks_adapter_capped_work_sets_and_diagnostics_as_truncated() {
    let mut adapter = CompilerServiceIpcAdapter::new(
        RegisteredProjectCompilerService::default(),
        CompilerServiceIpcLimits {
            max_modules_per_set: 1,
            max_diagnostics: 0,
            ..CompilerServiceIpcLimits::default()
        },
    )
    .unwrap();
    let project = adapter
        .register_core_project(registration("export const value: number = 'wrong';"))
        .unwrap();
    let CompilerReply::Check(check) = adapter.handle(CompilerRequest::Check { project }) else {
        panic!("registered project must return a bounded check reply")
    };
    assert!(check.has_errors);
    assert_eq!(check.parsed_modules.entries.len(), 1);
    assert!(check.parsed_modules.truncated);
    assert!(check.rechecked_modules.truncated);
    assert!(check.diagnostics.entries.is_empty());
    assert!(check.diagnostics.truncated);
}

#[test]
fn adapter_pages_diagnostics_with_one_shot_generation_bound_cursors() {
    let mut adapter = CompilerServiceIpcAdapter::new(
        RegisteredProjectCompilerService::new(CompilerServiceLimits {
            max_retained_diagnostics: 4,
            max_diagnostics: 1,
            ..CompilerServiceLimits::default()
        }),
        CompilerServiceIpcLimits {
            max_diagnostics: 0,
            max_diagnostic_page_entries: 1,
            ..CompilerServiceIpcLimits::default()
        },
    )
    .unwrap();
    let project = adapter
        .register_core_project(registration(
            "const marker = '😀';\r\nconst first: number = 'one'; \
             const second: number = 'two'; \
             const third: number = 'three';",
        ))
        .unwrap();
    let CompilerReply::Check(check) = adapter.handle(CompilerRequest::Check { project }) else {
        panic!("registered invalid project must return a bounded check reply")
    };
    assert!(check.has_errors);
    assert!(check.diagnostics.entries.is_empty());
    assert!(check.diagnostics.truncated);
    let CompilerReply::DiagnosticPage(first) = adapter.handle(CompilerRequest::ListDiagnostics {
        generation: check.generation,
        cursor: None,
        limit: Some(1),
    }) else {
        panic!("first diagnostic page must be returned for the exact check generation")
    };
    assert_eq!(first.generation, check.generation);
    assert_eq!(first.entries.len(), 1);
    let coordinates = first.entries[0]
        .coordinates
        .expect("an authorized diagnostic must retain original source coordinates");
    assert_eq!(coordinates.start_line, 1);
    assert_eq!(coordinates.end_line, 1);
    assert!(coordinates
        .is_well_formed_for_diagnostic_range(first.entries[0].start, first.entries[0].end,));
    assert!(
        !format!("{first:?}").contains("const first"),
        "paged diagnostic output must not contain the retained project source"
    );
    let cursor = first
        .next_cursor
        .expect("fixture must require a continuation cursor");
    assert!(matches!(
        adapter.handle(CompilerRequest::ListDiagnostics {
            generation: check.generation,
            cursor: Some(cursor),
            limit: Some(0),
        }),
        CompilerReply::Error {
            code: CompilerErrorCode::InvalidDiagnosticPage,
            ..
        }
    ));
    let CompilerReply::DiagnosticPage(_) = adapter.handle(CompilerRequest::ListDiagnostics {
        generation: check.generation,
        cursor: Some(cursor),
        limit: Some(1),
    }) else {
        panic!("a rejected page-limit request must not consume its cursor")
    };
    assert!(matches!(
        adapter.handle(CompilerRequest::ListDiagnostics {
            generation: check.generation,
            cursor: Some(cursor),
            limit: Some(1),
        }),
        CompilerReply::Error {
            code: CompilerErrorCode::InvalidDiagnosticCursor,
            ..
        }
    ));
    let CompilerReply::Check(later) = adapter.handle(CompilerRequest::Check { project }) else {
        panic!("later check must create a successor generation")
    };
    assert!(matches!(
        adapter.handle(CompilerRequest::ListDiagnostics {
            generation: check.generation,
            cursor: None,
            limit: Some(1),
        }),
        CompilerReply::Error {
            code: CompilerErrorCode::StaleGeneration,
            ..
        }
    ));
    assert_ne!(later.generation, check.generation);
}

#[test]
fn immediate_check_diagnostics_include_authorized_utf16_positions() {
    let mut adapter = CompilerServiceIpcAdapter::default();
    let project = adapter
        .register_core_project(registration(
            "const marker = '😀';\r\nconst invalid: number = 'wrong';",
        ))
        .unwrap();
    let CompilerReply::Check(check) = adapter.handle(CompilerRequest::Check { project }) else {
        panic!("the invalid closed project must return a check result")
    };
    let diagnostic = check
        .diagnostics
        .entries
        .iter()
        .find(|entry| entry.code == "BTS3003")
        .expect("a static type mismatch must remain observable");
    let coordinates = diagnostic.coordinates.unwrap();
    assert_eq!(coordinates.start_line, 1);
    assert_eq!(coordinates.end_line, 1);
    assert!(coordinates.is_well_formed_for_diagnostic_range(diagnostic.start, diagnostic.end,));
    assert!(!format!("{check:?}").contains("const marker"));
}

#[test]
fn diagnostic_cursors_are_bound_to_the_receiving_compiler_stream() {
    let mut adapter = CompilerServiceIpcAdapter::new(
        RegisteredProjectCompilerService::new(CompilerServiceLimits {
            max_diagnostic_cursors: 1,
            ..CompilerServiceLimits::default()
        }),
        CompilerServiceIpcLimits {
            max_diagnostic_page_entries: 1,
            ..CompilerServiceIpcLimits::default()
        },
    )
    .unwrap();
    let project = adapter
        .register_core_project(registration(
            "const first: number = 'one'; \
             const second: number = 'two'; \
             const third: number = 'three';",
        ))
        .unwrap();
    let first_stream = "a".repeat(CompilerSessionAttestation::ID_LENGTH);
    let second_stream = "b".repeat(CompilerSessionAttestation::ID_LENGTH);
    inventory_on_stream(&mut adapter, &first_stream, project);
    inventory_on_stream(&mut adapter, &second_stream, project);
    let CompilerReply::Check(check) =
        adapter.handle_session_request(&first_stream, CompilerRequest::Check { project })
    else {
        panic!("the invalid fixture must yield a bounded check")
    };
    let CompilerReply::DiagnosticPage(first) = adapter.handle_session_request(
        &first_stream,
        CompilerRequest::ListDiagnostics {
            generation: check.generation,
            cursor: None,
            limit: Some(1),
        },
    ) else {
        panic!("the first stream must receive a diagnostic page")
    };
    let cursor = first
        .next_cursor
        .expect("three diagnostics need continuation");
    let continuation = CompilerRequest::ListDiagnostics {
        generation: check.generation,
        cursor: Some(cursor),
        limit: Some(1),
    };
    assert!(matches!(
        adapter.handle_session_request(&second_stream, continuation.clone()),
        CompilerReply::Error {
            code: CompilerErrorCode::InvalidDiagnosticCursor,
            ..
        }
    ));
    assert!(matches!(
        adapter.handle_session_request(
            &first_stream,
            CompilerRequest::ListDiagnostics {
                generation: check.generation,
                cursor: Some(cursor),
                limit: Some(0),
            },
        ),
        CompilerReply::Error {
            code: CompilerErrorCode::InvalidDiagnosticPage,
            ..
        }
    ));
    adapter.end_session(&first_stream);
    inventory_on_stream(&mut adapter, &first_stream, project);
    assert!(matches!(
        adapter.handle_session_request(&second_stream, continuation),
        CompilerReply::Error {
            code: CompilerErrorCode::InvalidDiagnosticCursor,
            ..
        }
    ));
    let CompilerReply::DiagnosticPage(second) = adapter.handle_session_request(
        &second_stream,
        CompilerRequest::ListDiagnostics {
            generation: check.generation,
            cursor: None,
            limit: Some(1),
        },
    ) else {
        panic!("disconnect must release the sole diagnostic cursor slot")
    };
    let second_cursor = second.next_cursor.expect("a fresh stream can paginate");
    assert_ne!(second_cursor, cursor);
    let CompilerReply::Check(later) =
        adapter.handle_session_request(&first_stream, CompilerRequest::Check { project })
    else {
        panic!("a later check must advance the project generation")
    };
    assert_ne!(later.generation, check.generation);
    assert!(matches!(
        adapter.handle_session_request(
            &second_stream,
            CompilerRequest::ListDiagnostics {
                generation: check.generation,
                cursor: Some(second_cursor),
                limit: Some(1),
            },
        ),
        CompilerReply::Error {
            code: CompilerErrorCode::InvalidDiagnosticCursor,
            ..
        }
    ));
}

#[test]
fn work_set_cursors_are_stream_kind_and_generation_bound() {
    let mut adapter = CompilerServiceIpcAdapter::new(
        RegisteredProjectCompilerService::new(CompilerServiceLimits {
            max_work_set_cursors: 1,
            ..CompilerServiceLimits::default()
        }),
        CompilerServiceIpcLimits {
            max_modules_per_set: 1,
            max_work_set_page_entries: 1,
            ..CompilerServiceIpcLimits::default()
        },
    )
    .unwrap();
    let project = adapter
        .register_core_project(registration("export const value: number = answer;"))
        .unwrap();
    let first_stream = "a".repeat(CompilerSessionAttestation::ID_LENGTH);
    let second_stream = "b".repeat(CompilerSessionAttestation::ID_LENGTH);
    inventory_on_stream(&mut adapter, &first_stream, project);
    inventory_on_stream(&mut adapter, &second_stream, project);
    let CompilerReply::Check(check) =
        adapter.handle_session_request(&first_stream, CompilerRequest::Check { project })
    else {
        panic!("fixture must check under the first stream")
    };
    assert!(check.parsed_modules.truncated);
    let CompilerReply::WorkSetPage(first) = adapter.handle_session_request(
        &first_stream,
        CompilerRequest::ListWorkSet {
            generation: check.generation,
            kind: CompilerWorkSetKind::Parsed,
            cursor: None,
            limit: Some(1),
        },
    ) else {
        panic!("first work-set page must be available")
    };
    assert_eq!(first.entries.len(), 1);
    let cursor = first.next_cursor.expect("two modules need continuation");
    let continuation = CompilerRequest::ListWorkSet {
        generation: check.generation,
        kind: CompilerWorkSetKind::Parsed,
        cursor: Some(cursor),
        limit: Some(1),
    };
    for rejected in [
        adapter.handle_session_request(&second_stream, continuation.clone()),
        adapter.handle_session_request(
            &first_stream,
            CompilerRequest::ListWorkSet {
                generation: check.generation,
                kind: CompilerWorkSetKind::Rechecked,
                cursor: Some(cursor),
                limit: Some(1),
            },
        ),
    ] {
        assert!(matches!(
            rejected,
            CompilerReply::Error {
                code: CompilerErrorCode::InvalidWorkSetCursor,
                ..
            }
        ));
    }
    assert!(matches!(
        adapter.handle_session_request(
            &first_stream,
            CompilerRequest::ListWorkSet {
                generation: check.generation,
                kind: CompilerWorkSetKind::Parsed,
                cursor: Some(cursor),
                limit: Some(0),
            },
        ),
        CompilerReply::Error {
            code: CompilerErrorCode::InvalidWorkSetPage,
            ..
        }
    ));
    let CompilerReply::WorkSetPage(second) =
        adapter.handle_session_request(&first_stream, continuation.clone())
    else {
        panic!("owning stream must consume its cursor once")
    };
    assert!(second.next_cursor.is_none());
    assert_ne!(first.entries, second.entries);
    assert!(matches!(
        adapter.handle_session_request(&first_stream, continuation),
        CompilerReply::Error {
            code: CompilerErrorCode::InvalidWorkSetCursor,
            ..
        }
    ));
    let CompilerReply::Check(later) =
        adapter.handle_session_request(&first_stream, CompilerRequest::Check { project })
    else {
        panic!("successor generation must check")
    };
    assert_ne!(later.generation, check.generation);
    assert!(matches!(
        adapter.handle_session_request(
            &first_stream,
            CompilerRequest::ListWorkSet {
                generation: check.generation,
                kind: CompilerWorkSetKind::Parsed,
                cursor: None,
                limit: Some(1),
            }
        ),
        CompilerReply::Error {
            code: CompilerErrorCode::StaleGeneration,
            ..
        }
    ));
}

#[test]
fn abandoned_work_set_cursor_slots_are_released_on_disconnect() {
    let mut adapter = CompilerServiceIpcAdapter::new(
        RegisteredProjectCompilerService::new(CompilerServiceLimits {
            max_work_set_cursors: 1,
            ..CompilerServiceLimits::default()
        }),
        CompilerServiceIpcLimits {
            max_work_set_page_entries: 1,
            ..CompilerServiceIpcLimits::default()
        },
    )
    .unwrap();
    let project = adapter
        .register_core_project(registration("export const value: number = answer;"))
        .unwrap();
    let first_stream = "a".repeat(CompilerSessionAttestation::ID_LENGTH);
    let second_stream = "b".repeat(CompilerSessionAttestation::ID_LENGTH);
    inventory_on_stream(&mut adapter, &first_stream, project);
    inventory_on_stream(&mut adapter, &second_stream, project);
    let CompilerReply::Check(check) =
        adapter.handle_session_request(&first_stream, CompilerRequest::Check { project })
    else {
        panic!("registered fixture must check")
    };
    let first_request = CompilerRequest::ListWorkSet {
        generation: check.generation,
        kind: CompilerWorkSetKind::Parsed,
        cursor: None,
        limit: Some(1),
    };
    let CompilerReply::WorkSetPage(first) =
        adapter.handle_session_request(&first_stream, first_request.clone())
    else {
        panic!("the first stream must own the sole cursor slot")
    };
    let cursor = first.next_cursor.unwrap();
    assert!(matches!(
        adapter.handle_session_request(&second_stream, first_request.clone()),
        CompilerReply::Error {
            code: CompilerErrorCode::ResourceLimit,
            ..
        }
    ));
    adapter.end_session(&first_stream);
    let CompilerReply::WorkSetPage(second) =
        adapter.handle_session_request(&second_stream, first_request)
    else {
        panic!("disconnect must release the sole work-set cursor slot")
    };
    assert_ne!(second.next_cursor, Some(cursor));
}

#[test]
fn rejected_diagnostic_wire_page_does_not_leak_a_core_cursor_slot() {
    let mut adapter = CompilerServiceIpcAdapter::new(
        RegisteredProjectCompilerService::new(CompilerServiceLimits {
            max_diagnostic_cursors: 1,
            ..CompilerServiceLimits::default()
        }),
        CompilerServiceIpcLimits {
            max_field_bytes: 4,
            max_diagnostic_page_entries: 1,
            ..CompilerServiceIpcLimits::default()
        },
    )
    .unwrap();
    let project = adapter
        .register_core_project(registration(
            "const first: number = 'one'; \
             const second: number = 'two'; \
             const third: number = 'three';",
        ))
        .unwrap();
    assert_eq!(
        adapter.handle(CompilerRequest::Check { project }),
        response_limit_reply(),
        "the tiny wire-field budget must not disclose a check generation"
    );
    // An untrusted raw IPC peer may guess a generation number even after
    // the check reply was rejected. Both attempts must fail at the wire
    // budget, not because the first undisclosed page exhausted the sole
    // service cursor slot.
    let generation = CompilerGeneration {
        project,
        sequence: 1,
    };
    for _ in 0..2 {
        assert_eq!(
            adapter.handle(CompilerRequest::ListDiagnostics {
                generation,
                cursor: None,
                limit: Some(1),
            }),
            response_limit_reply(),
        );
    }
}

#[test]
fn queued_requests_are_applied_only_by_the_adapter_owner() {
    let (mut adapter, project) = adapter();
    inventory_on_stream(
        &mut adapter,
        &"a".repeat(CompilerSessionAttestation::ID_LENGTH),
        project,
    );
    let (sender, receiver) = compiler_service_ipc_request_channel();
    assert!(sender
        .bind_session(CompilerSessionAttestation {
            id: "caller-chosen-short-token".to_string(),
        })
        .is_err());
    let bound = sender
        .bind_session(CompilerSessionAttestation {
            id: "a".repeat(CompilerSessionAttestation::ID_LENGTH),
        })
        .unwrap();
    let worker = std::thread::spawn(move || bound.request(CompilerRequest::Check { project }));
    while receiver.dispatch_pending(&mut adapter) == 0 {
        std::thread::yield_now();
    }
    assert!(matches!(
        worker.join().unwrap().unwrap(),
        CompilerReply::Check(_)
    ));
}

#[test]
fn startup_catalog_seals_registration_before_the_session_receives_queries() {
    let mut catalog = CoreCompilerProjectCatalog::default();
    let project = catalog
        .register_startup_project(registration("export const value: number = answer;"))
        .unwrap();
    assert_eq!(catalog.registered_project_count(), 1);

    // `seal` consumes the only object that exposes registration. The
    // resulting service exposes only this bounded query dispatch API.
    let mut session = catalog.seal();
    assert_eq!(session.registered_project_count(), 1);
    inventory_on_stream(
        &mut session.adapter,
        &"b".repeat(CompilerSessionAttestation::ID_LENGTH),
        project,
    );
    let (sender, receiver) = compiler_service_ipc_request_channel();
    let bound = sender
        .bind_session(CompilerSessionAttestation {
            id: "b".repeat(CompilerSessionAttestation::ID_LENGTH),
        })
        .unwrap();
    let worker =
        std::thread::spawn(move || bound.request(CompilerRequest::DescribeProject { project }));
    while session.dispatch_pending(&receiver) == 0 {
        std::thread::yield_now();
    }
    assert!(matches!(
        worker.join().unwrap().unwrap(),
        CompilerReply::Project(CompilerProjectIdentity { project: returned, .. })
            if returned == project
    ));
}

#[test]
fn private_startup_project_is_never_inventoried_or_queryable() {
    let mut catalog = CoreCompilerProjectCatalog::default();
    let visible = catalog
        .register_startup_project(registration("export const shown: number = answer;"))
        .unwrap();
    let mut private_registration = registration("export const hidden: number = answer;");
    private_registration.canonical_project_root = "project:///private".to_string();
    private_registration.canonical_config_root = "project:///private/blue-ts.json".to_string();
    private_registration.canonical_output_root = "project:///private-dist".to_string();
    let private = catalog
        .register_startup_project_private(private_registration)
        .unwrap();
    assert_eq!(catalog.registered_project_count(), 2);
    let mut session = catalog.seal();
    let stream = "c".repeat(CompilerSessionAttestation::ID_LENGTH);
    let CompilerReply::Projects(inventory) = session
        .adapter
        .handle_session_request(&stream, CompilerRequest::ListProjects)
    else {
        panic!("accepted stream must receive a project inventory")
    };
    assert_eq!(inventory.projects, vec![visible]);
    for request in [
        CompilerRequest::DescribeProject { project: private },
        CompilerRequest::Check { project: private },
    ] {
        assert!(matches!(
            session.adapter.handle_session_request(&stream, request),
            CompilerReply::Error {
                code: CompilerErrorCode::UnobservedProject,
                ..
            }
        ));
    }
}

#[test]
fn pre_registered_service_projects_remain_private_until_explicitly_exposed() {
    let mut service = RegisteredProjectCompilerService::default();
    let private = project_to_wire(
        service
            .register(registration("export const hidden: number = answer;"))
            .unwrap(),
    );
    let mut adapter =
        CompilerServiceIpcAdapter::new(service, CompilerServiceIpcLimits::default()).unwrap();
    let stream = "d".repeat(CompilerSessionAttestation::ID_LENGTH);
    for reply in [
        adapter.handle(CompilerRequest::ListProjects),
        adapter.handle_session_request(&stream, CompilerRequest::ListProjects),
    ] {
        let CompilerReply::Projects(inventory) = reply else {
            panic!("a private-only service must return an empty inventory")
        };
        assert!(inventory.projects.is_empty());
    }
    for request in [
        CompilerRequest::DescribeProject { project: private },
        CompilerRequest::Check { project: private },
    ] {
        assert!(matches!(
            adapter.handle(request.clone()),
            CompilerReply::Error {
                code: CompilerErrorCode::UnobservedProject,
                ..
            }
        ));
        assert!(matches!(
            adapter.handle_session_request(&stream, request),
            CompilerReply::Error {
                code: CompilerErrorCode::UnobservedProject,
                ..
            }
        ));
    }

    let mut exposed_registration = registration("export const shown: number = answer;");
    exposed_registration.canonical_project_root = "project:///shown".to_string();
    exposed_registration.canonical_config_root = "project:///shown/blue-ts.json".to_string();
    exposed_registration.canonical_output_root = "project:///shown-dist".to_string();
    let exposed = adapter.register_core_project(exposed_registration).unwrap();
    let CompilerReply::Projects(inventory) =
        adapter.handle_session_request(&stream, CompilerRequest::ListProjects)
    else {
        panic!("accepted stream must receive an inventory")
    };
    assert_eq!(inventory.projects, vec![exposed]);
    assert!(matches!(
        adapter.handle_session_request(&stream, CompilerRequest::Check { project: private }),
        CompilerReply::Error {
            code: CompilerErrorCode::UnobservedProject,
            ..
        }
    ));
}

#[test]
fn pre_registered_catalog_projects_are_counted_but_never_exposed() {
    let mut service = RegisteredProjectCompilerService::default();
    service
        .register(registration("export const hidden: number = answer;"))
        .unwrap();
    let catalog =
        CoreCompilerProjectCatalog::new(service, CompilerServiceIpcLimits::default()).unwrap();
    assert_eq!(catalog.registered_project_count(), 1);
    let mut session = catalog.seal();
    assert_eq!(session.registered_project_count(), 1);
    let stream = "e".repeat(CompilerSessionAttestation::ID_LENGTH);
    let CompilerReply::Projects(inventory) = session
        .adapter
        .handle_session_request(&stream, CompilerRequest::ListProjects)
    else {
        panic!("private-only catalog must still return a valid inventory")
    };
    assert!(inventory.projects.is_empty());
}

#[test]
fn invalid_ipc_limits_fail_before_an_adapter_is_available() {
    assert_eq!(
        CompilerServiceIpcAdapter::new(
            RegisteredProjectCompilerService::default(),
            CompilerServiceIpcLimits {
                max_stream_cursor_receipts: 0,
                ..CompilerServiceIpcLimits::default()
            },
        )
        .unwrap_err(),
        CompilerServiceIpcConfigurationError::ZeroStreamCursorReceipts
    );
    assert_eq!(
        CompilerServiceIpcAdapter::new(
            RegisteredProjectCompilerService::default(),
            CompilerServiceIpcLimits {
                max_response_bytes: 0,
                ..CompilerServiceIpcLimits::default()
            },
        )
        .unwrap_err(),
        CompilerServiceIpcConfigurationError::ZeroResponseBytes
    );
    assert_eq!(
        CompilerServiceIpcAdapter::new(
            RegisteredProjectCompilerService::default(),
            CompilerServiceIpcLimits {
                max_response_bytes: blueice_ipc::compiler::MAX_COMPILER_MESSAGE_BYTES + 1,
                ..CompilerServiceIpcLimits::default()
            },
        )
        .unwrap_err(),
        CompilerServiceIpcConfigurationError::ResponseExceedsTransportLimit
    );
    assert_eq!(
        CompilerServiceIpcAdapter::new(
            RegisteredProjectCompilerService::default(),
            CompilerServiceIpcLimits {
                max_diagnostic_page_entries: 0,
                ..CompilerServiceIpcLimits::default()
            },
        )
        .unwrap_err(),
        CompilerServiceIpcConfigurationError::ZeroDiagnosticPageEntries
    );
    assert_eq!(
        CompilerServiceIpcAdapter::new(
            RegisteredProjectCompilerService::default(),
            CompilerServiceIpcLimits {
                max_work_set_page_entries: 0,
                ..CompilerServiceIpcLimits::default()
            },
        )
        .unwrap_err(),
        CompilerServiceIpcConfigurationError::ZeroWorkSetPageEntries
    );
    assert_eq!(
        CompilerServiceIpcAdapter::new(
            RegisteredProjectCompilerService::default(),
            CompilerServiceIpcLimits {
                max_static_metadata_page_entries: 0,
                ..CompilerServiceIpcLimits::default()
            },
        )
        .unwrap_err(),
        CompilerServiceIpcConfigurationError::ZeroStaticMetadataPageEntries
    );
}
