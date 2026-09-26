// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

fn loaded_tabs(html: &str, url: &str) -> (TabManager, TabId) {
    let mut tabs = TabManager::new(320.0, 200.0);
    let tab_id = tabs.default_tab();
    tabs.get_mut(tab_id)
        .unwrap()
        .load_html_str(html, Some(url.to_string()));
    (tabs, tab_id)
}

fn reports(
    executor: &mut JavaScriptPageExecutor,
    tab_id: TabId,
) -> Vec<JavaScriptPageExecutionReport> {
    executor.drain_reports_for_tab(tab_id)
}

#[test]
fn executes_inline_classic_and_module_declarations_in_one_realm() {
    let (tabs, tab_id) = loaded_tabs(
        concat!(
            "<script>const answer = 40 + 2; answer;</script>",
            "<script type=\"module\">export const moduleAnswer = 43;</script>"
        ),
        "https://example.test/app/index.html",
    );
    let mut executor = JavaScriptPageExecutor::default();

    executor.synchronize_and_execute(&tabs).unwrap();

    assert_eq!(
        reports(&mut executor, tab_id),
        vec![
            JavaScriptPageExecutionReport::Executed {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 0,
                kind: BlueJsPageScriptKind::Classic,
            },
            JavaScriptPageExecutionReport::Executed {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 1,
                kind: BlueJsPageScriptKind::Module,
            },
        ]
    );
    assert_eq!(executor.realm_stats(tab_id).unwrap().program_count, 2);
}

#[test]
fn rejected_declaration_does_not_block_a_later_script() {
    let (tabs, tab_id) = loaded_tabs(
        concat!(
            "<script>const = syntaxError;</script>",
            "<script>41 + 1;</script>"
        ),
        "https://example.test/app/index.html",
    );
    let mut executor = JavaScriptPageExecutor::default();

    executor.synchronize_and_execute(&tabs).unwrap();

    let reports = reports(&mut executor, tab_id);
    assert!(matches!(
        reports.as_slice(),
        [
            JavaScriptPageExecutionReport::Rejected {
                category: "JavaScript parsing rejected the page script",
                ..
            },
            JavaScriptPageExecutionReport::Executed { .. }
        ]
    ));
}

#[test]
fn copied_document_text_binding_executes_and_rejects_arguments() {
    let (tabs, tab_id) = loaded_tabs(
        concat!(
            "<main>current document</main>",
            "<script>const text = blueiceDocumentText(); text;</script>",
            "<script>blueiceDocumentText(1);</script>"
        ),
        "https://example.test/app/index.html",
    );
    let mut executor = JavaScriptPageExecutor::default();

    executor.synchronize_and_execute(&tabs).unwrap();

    assert!(matches!(
        reports(&mut executor, tab_id).as_slice(),
        [
            JavaScriptPageExecutionReport::Executed {
                kind: BlueJsPageScriptKind::Classic,
                ..
            },
            JavaScriptPageExecutionReport::Rejected {
                category: "BlueJS page execution failed",
                ..
            }
        ]
    ));
}

#[test]
fn copied_document_origin_binding_executes_and_rejects_arguments() {
    let (tabs, tab_id) = loaded_tabs(
        concat!(
            "<main>current document</main>",
            "<script>if (blueiceDocumentOrigin() !== ",
            "'https://example.test') { throw 'unexpected origin'; }</script>",
            "<script>blueiceDocumentOrigin(1);</script>"
        ),
        "https://example.test/app/index.html",
    );
    let mut executor = JavaScriptPageExecutor::default();

    executor.synchronize_and_execute(&tabs).unwrap();

    assert!(matches!(
        reports(&mut executor, tab_id).as_slice(),
        [
            JavaScriptPageExecutionReport::Executed {
                kind: BlueJsPageScriptKind::Classic,
                ..
            },
            JavaScriptPageExecutionReport::Rejected {
                category: "BlueJS page execution failed",
                ..
            }
        ]
    ));
}

#[test]
fn document_text_contract_rejects_before_javascript_program_admission() {
    let oversized = "x".repeat(1_048_577);
    let (tabs, tab_id) = loaded_tabs(
        &format!("<main>{oversized}</main><script>blueiceDocumentText();</script>"),
        "https://example.test/app/index.html",
    );
    let mut executor = JavaScriptPageExecutor::default();

    executor.synchronize_and_execute(&tabs).unwrap();

    assert!(matches!(
        reports(&mut executor, tab_id).as_slice(),
        [JavaScriptPageExecutionReport::Rejected {
            category: "host binding contract rejected the page script",
            ..
        }]
    ));
    assert!(executor.realm_stats(tab_id).is_err());
}

#[test]
fn document_origin_contract_rejects_before_javascript_program_admission() {
    let (tabs, tab_id) = loaded_tabs(
        "<script>blueiceDocumentOrigin();</script>",
        "https://example.test/app/index.html",
    );
    let mut config = JavaScriptPageExecutorConfig::default();
    config
        .binding_contract_limits
        .document_origin
        .max_string_bytes = 1;
    let mut executor = JavaScriptPageExecutor::with_config(config).unwrap();

    executor.synchronize_and_execute(&tabs).unwrap();

    assert!(matches!(
        reports(&mut executor, tab_id).as_slice(),
        [JavaScriptPageExecutionReport::Rejected {
            category: "host binding contract rejected the page script",
            ..
        }]
    ));
    assert!(executor.realm_stats(tab_id).is_err());
}

#[test]
fn external_declaration_fails_closed_without_an_authorizer() {
    let (tabs, tab_id) = loaded_tabs(
        "<script src=\"/assets/app.js\"></script>",
        "https://example.test/app/index.html",
    );
    let mut executor = JavaScriptPageExecutor::default();

    executor.synchronize_and_execute(&tabs).unwrap();

    assert!(matches!(
        reports(&mut executor, tab_id).as_slice(),
        [JavaScriptPageExecutionReport::Rejected {
            category: "external JavaScript declarations require an authorized loader",
            ..
        }]
    ));
}

#[test]
fn navigation_replaces_the_realm_and_releases_old_programs() {
    let (mut tabs, tab_id) = loaded_tabs(
        "<script>const first = 1;</script>",
        "https://example.test/first.html",
    );
    let mut executor = JavaScriptPageExecutor::default();
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(executor.realm_stats(tab_id).unwrap().program_count, 1);
    let _ = reports(&mut executor, tab_id);

    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<script>const second = 2;</script>",
        Some("https://example.test/second.html".to_string()),
    );
    executor.synchronize_and_execute(&tabs).unwrap();

    assert_eq!(executor.realm_stats(tab_id).unwrap().program_count, 1);
    assert!(matches!(
        reports(&mut executor, tab_id).as_slice(),
        [JavaScriptPageExecutionReport::Executed {
            document_generation: 2,
            ..
        }]
    ));
}

#[test]
fn debugger_locations_are_opaque_exact_and_released_on_navigation() {
    let (mut tabs, tab_id) = loaded_tabs(
        "<script>const answer = 40 + 2; answer;</script>",
        "https://example.test/first.html",
    );
    let mut executor = JavaScriptPageExecutor::default();

    executor.synchronize_and_execute(&tabs).unwrap();
    let programs = executor.debugger_programs(tab_id, 1).unwrap();
    assert_eq!(programs.len(), 1);
    let program = programs[0];
    let safe_points = executor
        .debugger_safe_points(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
        )
        .unwrap();
    let safe_point = *safe_points
        .first()
        .expect("a compiled classic script has a safe point");
    executor
        .validate_debugger_safe_point(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
            safe_point.code_unit_ordinal,
            safe_point.bytecode_offset,
        )
        .unwrap();
    assert_eq!(
        executor.validate_debugger_safe_point(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
            safe_point.code_unit_ordinal,
            u32::MAX,
        ),
        Err(JavaScriptPageDebuggerError::InvalidSafePoint)
    );

    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<script>const successor = 43;</script>",
        Some("https://example.test/second.html".to_string()),
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor.debugger_programs(tab_id, 1),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
    let successor = executor.debugger_programs(tab_id, 2).unwrap();
    assert_eq!(successor.len(), 1);
    assert_ne!(successor[0].program_handle, program.program_handle);
    assert_eq!(
        executor.debugger_safe_points(
            tab_id,
            2,
            program.program_handle,
            program.program_generation,
        ),
        Err(JavaScriptPageDebuggerError::UnknownProgram)
    );
}

#[test]
fn debugger_safe_point_inventory_enforces_the_core_selected_reply_limit() {
    let (tabs, tab_id) = loaded_tabs(
        "<script>const answer = 40 + 2; answer;</script>",
        "https://example.test/limited-debugger.html",
    );
    let mut executor = JavaScriptPageExecutor::with_config(JavaScriptPageExecutorConfig {
        max_debugger_safe_points_per_program: 1,
        ..JavaScriptPageExecutorConfig::default()
    })
    .unwrap();

    executor.synchronize_and_execute(&tabs).unwrap();
    let program = executor.debugger_programs(tab_id, 1).unwrap()[0];
    assert_eq!(
        executor.debugger_safe_points(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
        ),
        Err(JavaScriptPageDebuggerError::ResourceLimit)
    );
}

#[test]
fn source_budget_rejects_before_parser_or_program_admission() {
    let (tabs, tab_id) = loaded_tabs(
        "<script>const answer = 42;</script>",
        "https://example.test/app/index.html",
    );
    let mut executor = JavaScriptPageExecutor::with_config(JavaScriptPageExecutorConfig {
        max_source_bytes_per_module: 1,
        ..JavaScriptPageExecutorConfig::default()
    })
    .unwrap();

    executor.synchronize_and_execute(&tabs).unwrap();

    assert!(matches!(
        reports(&mut executor, tab_id).as_slice(),
        [JavaScriptPageExecutionReport::Rejected {
            category: "JavaScript source exceeds configured policy",
            ..
        }]
    ));
    assert_eq!(executor.realm_stats(tab_id).unwrap().program_count, 0);
}

#[test]
fn bytecode_budget_rejects_without_retaining_a_partial_program() {
    let (tabs, tab_id) = loaded_tabs(
        "<script>const answer = 40 + 2; answer;</script>",
        "https://example.test/app/index.html",
    );
    let mut executor = JavaScriptPageExecutor::with_config(JavaScriptPageExecutorConfig {
        runtime: BlueJsPageRuntimeConfig {
            max_bytecode_bytes_per_realm: 1,
            ..BlueJsPageRuntimeConfig::default()
        },
        ..JavaScriptPageExecutorConfig::default()
    })
    .unwrap();

    executor.synchronize_and_execute(&tabs).unwrap();

    assert!(matches!(
        reports(&mut executor, tab_id).as_slice(),
        [JavaScriptPageExecutionReport::Rejected {
            category: "JavaScript page resource policy rejected the page script",
            ..
        }]
    ));
    let stats = executor.realm_stats(tab_id).unwrap();
    assert_eq!(stats.program_count, 0);
    assert_eq!(stats.bytecode_bytes, 0);
}

struct FixedAuthorizer {
    graph: AuthorizedJavaScriptModuleGraph,
}

impl JavaScriptPageSourceAuthorizer for FixedAuthorizer {
    fn authorize(
        &mut self,
        _request: &JavaScriptPageSourceRequest,
    ) -> Result<AuthorizedJavaScriptModuleGraph, JavaScriptPageSourceAuthorizationError> {
        Ok(self.graph.clone())
    }
}

#[test]
fn external_module_uses_authorized_canonical_resolution_records() {
    let entry = AuthorizedJavaScriptModule::new(
        "blueice://authorized/main.js",
        "import { value } from './dep.js'; value;",
    )
    .unwrap();
    let dependency =
        AuthorizedJavaScriptModule::new("blueice://authorized/dep.js", "export const value = 42;")
            .unwrap();
    let graph = AuthorizedJavaScriptModuleGraph::new(
        "blueice://authorized/main.js",
        [entry, dependency],
        [AuthorizedJavaScriptResolution::new(
            "blueice://authorized/main.js",
            "./dep.js",
            "blueice://authorized/dep.js",
        )
        .unwrap()],
        "test-authorized-javascript-resolver-v1",
    )
    .unwrap();
    let (tabs, tab_id) = loaded_tabs(
        "<script type=\"module\" src=\"/assets/main.js\"></script>",
        "https://example.test/app/index.html",
    );
    let mut executor = JavaScriptPageExecutor::with_external_source_authorizer(
        JavaScriptPageExecutorConfig::default(),
        FixedAuthorizer { graph },
    )
    .unwrap();

    executor.synchronize_and_execute(&tabs).unwrap();

    assert!(matches!(
        reports(&mut executor, tab_id).as_slice(),
        [JavaScriptPageExecutionReport::Executed {
            kind: BlueJsPageScriptKind::Module,
            ..
        }]
    ));
    assert_eq!(executor.realm_stats(tab_id).unwrap().program_count, 2);
}

#[test]
fn missing_static_resolution_rejects_without_admitting_any_graph_program() {
    let entry = AuthorizedJavaScriptModule::new(
        "blueice://authorized/main.js",
        "import { value } from './dep.js'; value;",
    )
    .unwrap();
    let dependency =
        AuthorizedJavaScriptModule::new("blueice://authorized/dep.js", "export const value = 42;")
            .unwrap();
    let graph = AuthorizedJavaScriptModuleGraph::new(
        "blueice://authorized/main.js",
        [entry, dependency],
        [],
        "test-authorized-javascript-resolver-v1",
    )
    .unwrap();
    let (tabs, tab_id) = loaded_tabs(
        "<script type=\"module\" src=\"/assets/main.js\"></script>",
        "https://example.test/app/index.html",
    );
    let mut executor = JavaScriptPageExecutor::with_external_source_authorizer(
        JavaScriptPageExecutorConfig::default(),
        FixedAuthorizer { graph },
    )
    .unwrap();

    executor.synchronize_and_execute(&tabs).unwrap();

    assert!(matches!(
        reports(&mut executor, tab_id).as_slice(),
        [JavaScriptPageExecutionReport::Rejected {
            category: "authorized JavaScript graph is missing a static resolution",
            ..
        }]
    ));
    assert_eq!(executor.realm_stats(tab_id).unwrap().program_count, 0);
}
