// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#[cfg(unix)]
use super::super::http_resource_authorizer::{
    sha256_integrity, HttpOutOfProcessPageScriptSourceAuthorizer, HttpScriptIntegrityManifest,
    HttpScriptResourceLimits, HttpScriptResourceOriginRule, HttpScriptResourcePolicy,
};
use super::*;
use crate::script::host_typings::{
    core_script_host_type_catalog, HostTypeSurfaceV1, CORE_SCRIPT_DOCUMENT_CONTEXT_PROFILE_V1,
    CORE_SCRIPT_DOCUMENT_TEXT_PROFILE_V1,
};
use crate::script::page_source_authorizer::{
    AuthorizedPageScriptGraph, PageScriptSourceAuthorizationError,
};
use crate::Page;
use blueice_bluets::{
    AuthorizedModule, AuthorizedModuleLoader, AuthorizedModuleResolution, LANGUAGE_VERSION,
};
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn strict_runtime_boundary_reports_keep_three_distinct_source_free_categories() {
    use super::super::contracts::StrictRuntimeBoundaryDiagnostic;

    let private_id = "dom.private-page-binding".to_string();
    let categories = [
        report_error_message(DirectPageScriptError::StrictRuntimeBoundary(
            StrictRuntimeBoundaryDiagnostic::MissingContract {
                stable_binding_id: private_id.clone(),
            },
        )),
        report_error_message(DirectPageScriptError::StrictRuntimeBoundary(
            StrictRuntimeBoundaryDiagnostic::UnreifiableType {
                stable_binding_id: private_id.clone(),
            },
        )),
        report_error_message(DirectPageScriptError::StrictRuntimeBoundary(
            StrictRuntimeBoundaryDiagnostic::UncheckedBoundary {
                stable_binding_id: private_id.clone(),
            },
        )),
    ];
    assert_eq!(
        categories.len(),
        categories
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
    );
    assert!(categories
        .iter()
        .all(|category| !category.contains(&private_id)));
}

fn profiles() -> HostTypeSurfaceCatalogV1 {
    HostTypeSurfaceCatalogV1::new([HostTypeSurfaceV1::new(
        LANGUAGE_VERSION,
        "inline-runner-v1",
        "inline-runner-empty-v1",
        Vec::new(),
    )])
    .unwrap()
}

fn executor() -> DirectPageInlineExecutor {
    DirectPageInlineExecutor::new(
        profiles(),
        "inline-runner-empty-v1",
        CompilerOptions::default(),
    )
    .unwrap()
}

struct StaticExternalAuthorizer {
    requests: Rc<RefCell<Vec<PageScriptSourceRequest>>>,
}

impl PageScriptSourceAuthorizer for StaticExternalAuthorizer {
    fn authorize(
        &mut self,
        request: &PageScriptSourceRequest,
    ) -> Result<
        super::super::page_source_authorizer::AuthorizedPageScriptGraph,
        PageScriptSourceAuthorizationError,
    > {
        self.requests.borrow_mut().push(request.clone());
        let entry = "https://example.test/assets/main.ts";
        let value = "https://example.test/assets/value.ts";
        let loader = AuthorizedModuleLoader::new(
                [
                    AuthorizedModule::new(
                        entry,
                        "import { value } from './value.ts'; export const answer: number = value + 1; answer;",
                    ),
                    AuthorizedModule::new(value, "export const value: number = 41;"),
                ],
                [AuthorizedModuleResolution::new(entry, "./value.ts", value)],
            )
            .unwrap();
        AuthorizedPageScriptGraph::new(entry, loader, "external-policy-v1")
            .map_err(|error| PageScriptSourceAuthorizationError::new(error.to_string()))
    }
}

struct TrackingSourceAuthorizer {
    retained: Rc<RefCell<std::collections::BTreeSet<(TabId, u64)>>>,
    authorizations: Rc<RefCell<usize>>,
}

impl PageScriptSourceAuthorizer for TrackingSourceAuthorizer {
    fn authorize(
        &mut self,
        request: &PageScriptSourceRequest,
    ) -> Result<AuthorizedPageScriptGraph, PageScriptSourceAuthorizationError> {
        self.retained
            .borrow_mut()
            .insert((request.tab_id, request.document_generation));
        *self.authorizations.borrow_mut() += 1;
        Err(PageScriptSourceAuthorizationError::new("fixture rejection"))
    }

    fn release_document(&mut self, tab_id: TabId, document_generation: u64) {
        self.retained
            .borrow_mut()
            .remove(&(tab_id, document_generation));
    }

    fn retained_source_payload_bytes(
        &self,
        tab_id: TabId,
        document_generation: u64,
    ) -> Option<usize> {
        Some(
            usize::from(
                self.retained
                    .borrow()
                    .contains(&(tab_id, document_generation)),
            ) * 17,
        )
    }
}

#[test]
fn direct_executor_releases_external_source_on_navigation_and_close() {
    let retained = Rc::new(RefCell::new(std::collections::BTreeSet::new()));
    let authorizations = Rc::new(RefCell::new(0));
    let mut tabs = TabManager::new(320.0, 200.0);
    let tab_id = tabs.default_tab();
    let html =
        "<script type=\"application/x-blueice-typescript-module\" src=\"/main.ts\"></script>";
    tabs.get_mut(tab_id)
        .unwrap()
        .load_html_str(html, Some("https://example.test/first.html".to_string()));
    let mut executor = DirectPageInlineExecutor::with_external_source_authorizer(
        profiles(),
        "inline-runner-empty-v1",
        CompilerOptions::default(),
        TrackingSourceAuthorizer {
            retained: Rc::clone(&retained),
            authorizations: Rc::clone(&authorizations),
        },
    )
    .unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor.retained_external_source_payload_bytes(tab_id, 1),
        Some(17)
    );
    assert_eq!(
        executor
            .retained_page_cache_payload_bytes(tab_id, 1)
            .unwrap(),
        Some(17)
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(*authorizations.borrow(), 1);
    tabs.get_mut(tab_id)
        .unwrap()
        .load_html_str(html, Some("https://example.test/second.html".to_string()));
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor.retained_external_source_payload_bytes(tab_id, 1),
        None
    );
    assert_eq!(
        executor.retained_external_source_payload_bytes(tab_id, 2),
        Some(17)
    );
    assert_eq!(
        executor
            .retained_page_cache_payload_bytes(tab_id, 2)
            .unwrap(),
        Some(17)
    );
    assert_eq!(*authorizations.borrow(), 2);
    assert!(!retained.borrow().contains(&(tab_id, 1)));
    assert!(tabs.close_tab(tab_id));
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor.retained_external_source_payload_bytes(tab_id, 2),
        None
    );
    assert_eq!(
        executor
            .retained_page_cache_payload_bytes(tab_id, 2)
            .unwrap(),
        None
    );
    assert!(retained.borrow().is_empty());
}

#[test]
fn direct_live_cache_total_includes_debug_metadata_and_releases_it() {
    let mut tabs = TabManager::new(320.0, 200.0);
    let tab_id = tabs.default_tab();
    tabs.get_mut(tab_id).unwrap().load_html_str(
            "<script type=\"application/x-blueice-typescript\">const answer: number = 42; answer;</script>",
            Some("https://example.test/first.html".to_string()),
        );
    let mut executor = executor();
    executor.synchronize_and_execute(&tabs).unwrap();
    let metadata = executor
        .host
        .retained_debug_payload_bytes(tab_id)
        .unwrap()
        .unwrap();
    assert!(metadata > 0);
    assert_eq!(
        executor
            .retained_page_cache_payload_bytes(tab_id, 1)
            .unwrap(),
        Some(metadata)
    );

    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<main>replacement</main>",
        Some("https://example.test/second.html".to_string()),
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor
            .retained_page_cache_payload_bytes(tab_id, 1)
            .unwrap(),
        None
    );
    assert_eq!(
        executor
            .retained_page_cache_payload_bytes(tab_id, 2)
            .unwrap(),
        Some(0)
    );
    assert!(tabs.close_tab(tab_id));
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor
            .retained_page_cache_payload_bytes(tab_id, 2)
            .unwrap(),
        None
    );
}

#[test]
fn executes_opted_in_inline_classic_and_module_declarations_once_in_order() {
    let mut tabs = TabManager::new(320.0, 200.0);
    let tab_id = tabs.default_tab();
    tabs.get_mut(tab_id).unwrap().load_html_str(
        concat!(
            "<script type=\"application/x-blueice-typescript\">42;</script>",
            "<script type=\"application/x-blueice-typescript-module\">43;</script>"
        ),
        Some("https://example.test/app/index.html".to_string()),
    );
    let mut executor = executor();
    executor.synchronize_and_execute(&tabs).unwrap();

    assert_eq!(executor.debug_record_count(), 2);
    assert_eq!(executor.realm_stats(tab_id).unwrap().program_count, 2);
    assert_eq!(
        executor.reports(),
        &VecDeque::from([
            DirectPageScriptExecutionReport::Executed {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 0,
                kind: DirectPageScriptKind::Classic,
                policy: RuntimePolicy::Checked,
                source_position: None,
            },
            DirectPageScriptExecutionReport::Executed {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 1,
                kind: DirectPageScriptKind::Module,
                policy: RuntimePolicy::Checked,
                source_position: None,
            },
        ])
    );

    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(executor.reports().len(), 2, "the document runs only once");
}

#[test]
fn reports_owner_policy_and_verified_inline_position_without_source() {
    let source = "const broken: number = \"wrong\";";
    let mut tabs = TabManager::new(320.0, 200.0);
    let tab_id = tabs.default_tab();
    tabs.get_mut(tab_id).unwrap().load_html_str(
            &format!(
                "<script type=\"application/x-blueice-typescript\">{source}</script><script type=\"application/x-blueice-typescript\" src=\"https://secret.test/private.ts\"></script>"
            ),
            Some("https://example.test/page.html".to_string()),
        );
    let mut executor = executor();
    executor.synchronize_and_execute(&tabs).unwrap();
    let reports = executor.drain_reports();
    let [DirectPageScriptExecutionReport::Rejected {
        policy: RuntimePolicy::Checked,
        source_position: Some(position),
        message,
        ..
    }, DirectPageScriptExecutionReport::Rejected {
        policy: RuntimePolicy::Checked,
        source_position: None,
        ..
    }] = reports.as_slice()
    else {
        panic!("expected checked inline rejection and source-free external denial")
    };
    assert!(position.start < position.end);
    assert!(position.end as usize <= source.len());
    assert_eq!(message, "BlueTS compilation rejected the page script");
    assert!(!format!("{reports:?}").contains("secret.test"));
    assert!(!format!("{reports:?}").contains("wrong"));

    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<script type=\"application/x-blueice-typescript\">42;</script>",
        Some("https://example.test/strict.html".to_string()),
    );
    let mut strict = DirectPageInlineExecutor::new(
        profiles(),
        "inline-runner-empty-v1",
        CompilerOptions {
            runtime_policy: RuntimePolicy::StrictRuntime,
            ..CompilerOptions::default()
        },
    )
    .unwrap();
    strict.synchronize_and_execute(&tabs).unwrap();
    assert!(matches!(
        strict.reports().front(),
        Some(DirectPageScriptExecutionReport::Executed {
            policy: RuntimePolicy::StrictRuntime,
            source_position: None,
            ..
        })
    ));
}

#[test]
fn source_position_requires_matching_module_and_valid_utf8_boundaries() {
    let source = "aéz";
    let module = "core-inline-tab-1-document-1-ordinal-0";
    let error = |candidate: &str, start, end| {
        DirectPageScriptError::Bridge(BridgeError::UnsupportedRuntimeTarget {
            span: blueice_bluets::SourceSpan::new(candidate, start, end),
            message: "private diagnostic".into(),
        })
    };
    assert_eq!(
        source_position_from_error(&error(module, 1, 3), module, source),
        Some(InlineBlueTsSourcePosition { start: 1, end: 3 })
    );
    for (candidate, start, end) in [
        ("other-module", 1, 3),
        (module, 2, 3),
        (module, 3, 5),
        (module, 1, 1),
    ] {
        assert_eq!(
            source_position_from_error(&error(candidate, start, end), module, source),
            None
        );
    }
}

#[test]
fn draining_one_tab_reports_does_not_expose_or_discard_another_tabs_reports() {
    let mut executor = executor();
    executor.reports = VecDeque::from([
        DirectPageScriptExecutionReport::Executed {
            tab_id: 1,
            document_generation: 3,
            ordinal: 0,
            kind: DirectPageScriptKind::Classic,
            policy: RuntimePolicy::Checked,
            source_position: None,
        },
        DirectPageScriptExecutionReport::Rejected {
            tab_id: 2,
            document_generation: 4,
            ordinal: 1,
            kind: DirectPageScriptKind::Module,
            policy: RuntimePolicy::Checked,
            source_position: None,
            message: "BlueTS compilation rejected the page script".to_string(),
        },
    ]);

    assert_eq!(
        executor.drain_reports_for_tab(TabId::from_u64(1)),
        vec![DirectPageScriptExecutionReport::Executed {
            tab_id: 1,
            document_generation: 3,
            ordinal: 0,
            kind: DirectPageScriptKind::Classic,
            policy: RuntimePolicy::Checked,
            source_position: None,
        }]
    );
    assert_eq!(
        executor.reports(),
        &VecDeque::from([DirectPageScriptExecutionReport::Rejected {
            tab_id: 2,
            document_generation: 4,
            ordinal: 1,
            kind: DirectPageScriptKind::Module,
            policy: RuntimePolicy::Checked,
            source_position: None,
            message: "BlueTS compilation rejected the page script".to_string(),
        }])
    );
}

#[test]
fn configured_realm_limits_reject_inline_code_without_retaining_programs() {
    let mut tabs = TabManager::new(320.0, 200.0);
    let tab_id = tabs.default_tab();
    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<script type=\"application/x-blueice-typescript\">42;</script>",
        Some("https://example.test/app/index.html".to_string()),
    );
    let realms = DirectPageRealmOwner::new(
        blueice_bluejs::BlueJsPageRuntimeConfig {
            max_bytecode_bytes_per_realm: 1,
            ..blueice_bluejs::BlueJsPageRuntimeConfig::default()
        },
        blueice_bluets_bluejs::DirectDebugRetentionLimits::default(),
    )
    .unwrap();
    let mut executor = DirectPageInlineExecutor::with_realm_owner(
        profiles(),
        "inline-runner-empty-v1",
        CompilerOptions::default(),
        realms,
    )
    .unwrap();

    executor.synchronize_and_execute(&tabs).unwrap();

    assert_eq!(
        executor.reports(),
        &VecDeque::from([DirectPageScriptExecutionReport::Rejected {
            tab_id: tab_id.as_u64(),
            document_generation: 1,
            ordinal: 0,
            kind: DirectPageScriptKind::Classic,
            policy: RuntimePolicy::Checked,
            source_position: None,
            message: "BlueJS page execution failed".to_string(),
        }])
    );
    assert_eq!(executor.debug_record_count(), 0);
    let stats = executor.realm_stats(tab_id).unwrap();
    assert_eq!(stats.program_count, 0);
    assert_eq!(stats.bytecode_bytes, 0);
}

#[test]
fn inline_executor_runs_the_document_text_profile_through_page_lifecycle() {
    let mut tabs = TabManager::new(320.0, 200.0);
    let tab_id = tabs.default_tab();
    tabs.get_mut(tab_id).unwrap().load_html_str(
        concat!(
            "<main>current document</main>",
            "<script type=\"application/x-blueice-typescript\">",
            "const text: string = blueiceDocumentText(); text;",
            "</script>"
        ),
        Some("https://example.test/app/index.html".to_string()),
    );
    let mut executor = DirectPageInlineExecutor::new(
        core_script_host_type_catalog(),
        CORE_SCRIPT_DOCUMENT_TEXT_PROFILE_V1,
        CompilerOptions::default(),
    )
    .unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor.reports(),
        &VecDeque::from([DirectPageScriptExecutionReport::Executed {
            tab_id: tab_id.as_u64(),
            document_generation: 1,
            ordinal: 0,
            kind: DirectPageScriptKind::Classic,
            policy: RuntimePolicy::Checked,
            source_position: None,
        }])
    );
}

#[test]
fn inline_executor_redacts_a_host_binding_contract_rejection() {
    let mut tabs = TabManager::new(320.0, 200.0);
    let tab_id = tabs.default_tab();
    tabs.get_mut(tab_id).unwrap().load_html_str(
        concat!(
            "<main>page-controlled oversized document text</main>",
            "<script type=\"application/x-blueice-typescript\">",
            "blueiceDocumentText();",
            "</script>"
        ),
        Some("https://example.test/app/index.html".to_string()),
    );
    let profiles = core_script_host_type_catalog();
    let host = DirectPageScriptHost::with_realm_owner_and_contract_limits(
        profiles.clone(),
        DirectPageRealmOwner::default(),
        crate::script::contracts::CoreScriptBindingContractLimits {
            document_text: blueice_bluets::ValidationLimits {
                max_string_bytes: 8,
                ..blueice_bluets::ValidationLimits::default()
            },
            ..crate::script::contracts::CoreScriptBindingContractLimits::default()
        },
    );
    let mut executor = DirectPageInlineExecutor::new_with_host(
        profiles,
        CORE_SCRIPT_DOCUMENT_TEXT_PROFILE_V1,
        CompilerOptions::default(),
        None,
        host,
    )
    .unwrap();

    executor.synchronize_and_execute(&tabs).unwrap();

    assert_eq!(
        executor.reports(),
        &VecDeque::from([DirectPageScriptExecutionReport::Rejected {
            tab_id: tab_id.as_u64(),
            document_generation: 1,
            ordinal: 0,
            kind: DirectPageScriptKind::Classic,
            policy: RuntimePolicy::Checked,
            source_position: None,
            message: "host binding contract rejected the page script".to_string(),
        }])
    );
    assert_eq!(executor.debug_record_count(), 0);
    let stats = executor.realm_stats(tab_id).unwrap();
    assert_eq!(stats.program_count, 0);
    assert_eq!(stats.bytecode_bytes, 0);
}

#[test]
fn inline_executor_runs_the_document_context_profile_through_page_lifecycle() {
    let mut tabs = TabManager::new(320.0, 200.0);
    let tab_id = tabs.default_tab();
    tabs.get_mut(tab_id).unwrap().load_html_str(
        concat!(
            "<main>current context</main>",
            "<script type=\"application/x-blueice-typescript\">",
            "const origin: string = blueiceDocumentOrigin(); ",
            "const text: string = blueiceDocumentText(); origin + text;",
            "</script>"
        ),
        Some("https://example.test/app/index.html".to_string()),
    );
    let mut executor = DirectPageInlineExecutor::new(
        core_script_host_type_catalog(),
        CORE_SCRIPT_DOCUMENT_CONTEXT_PROFILE_V1,
        CompilerOptions::default(),
    )
    .unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();

    assert_eq!(
        executor.reports(),
        &VecDeque::from([DirectPageScriptExecutionReport::Executed {
            tab_id: tab_id.as_u64(),
            document_generation: 1,
            ordinal: 0,
            kind: DirectPageScriptKind::Classic,
            policy: RuntimePolicy::Checked,
            source_position: None,
        }])
    );
}

#[test]
fn rejects_external_source_without_reflecting_its_page_controlled_url() {
    let mut tabs = TabManager::new(320.0, 200.0);
    let tab_id = tabs.default_tab();
    tabs.get_mut(tab_id).unwrap().load_html_str(
            "<script type=\"application/x-blueice-typescript\" src=\"https://untrusted.test/a.ts\"></script>",
            Some("https://example.test/app/index.html".to_string()),
        );
    let mut executor = executor();
    executor.synchronize_and_execute(&tabs).unwrap();

    assert_eq!(executor.debug_record_count(), 0);
    let Some(DirectPageScriptExecutionReport::Rejected { message, .. }) =
        executor.reports().front()
    else {
        panic!("the external declaration must produce one rejection")
    };
    assert_eq!(
        message,
        "external BlueTS declarations require an authorized loader"
    );
    assert!(!message.contains("untrusted"));
}

#[cfg(unix)]
#[test]
fn http_authorized_direct_route_rejects_cross_origin_src_without_reflection() {
    let entry = "https://example.test/assets/main.ts";
    let policy = HttpScriptResourcePolicy::new(
        HttpScriptResourceOriginRule::same_document_origin(),
        HttpScriptIntegrityManifest::new([(
            entry.to_string(),
            sha256_integrity(b"export const answer: number = 42;"),
        )])
        .unwrap(),
        HttpScriptResourceLimits::default(),
    )
    .unwrap();
    let mut tabs = TabManager::new(320.0, 200.0);
    let tab_id = tabs.default_tab();
    tabs.get_mut(tab_id).unwrap().load_html_str(
            "<script type=\"application/x-blueice-typescript-module\" src=\"https://attacker.test/secret.ts\"></script>",
            Some("https://example.test/app/index.html".to_string()),
        );
    let mut executor = DirectPageInlineExecutor::with_external_source_authorizer(
        profiles(),
        "inline-runner-empty-v1",
        CompilerOptions::default(),
        HttpOutOfProcessPageScriptSourceAuthorizer::new(policy),
    )
    .unwrap();

    executor.synchronize_and_execute(&tabs).unwrap();

    assert_eq!(executor.debug_record_count(), 0);
    let Some(DirectPageScriptExecutionReport::Rejected { message, .. }) =
        executor.reports().front()
    else {
        panic!("the cross-origin declaration must produce one rejection")
    };
    assert_eq!(
        message,
        "external BlueTS source authorization rejected the page script"
    );
    assert!(!message.contains("attacker"));
}

#[test]
fn executes_an_external_module_only_from_a_core_authorized_closed_graph() {
    let mut tabs = TabManager::new(320.0, 200.0);
    let tab_id = tabs.default_tab();
    tabs.get_mut(tab_id).unwrap().load_html_str(
            "<script type=\"application/x-blueice-typescript-module\" src=\"/assets/main.ts\"></script>",
            Some("https://example.test/app/index.html".to_string()),
        );
    let requests = Rc::new(RefCell::new(Vec::new()));
    let mut executor = DirectPageInlineExecutor::with_external_source_authorizer(
        profiles(),
        "inline-runner-empty-v1",
        CompilerOptions::default(),
        StaticExternalAuthorizer {
            requests: Rc::clone(&requests),
        },
    )
    .unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();

    assert_eq!(executor.debug_record_count(), 2);
    assert!(matches!(
        executor.reports().front(),
        Some(DirectPageScriptExecutionReport::Executed {
            ordinal: 0,
            kind: DirectPageScriptKind::Module,
            ..
        })
    ));
    assert_eq!(
        requests.borrow().as_slice(),
        &[PageScriptSourceRequest {
            tab_id,
            document_generation: 1,
            ordinal: 0,
            kind: DirectPageScriptKind::Module,
            document_url: "https://example.test/app/index.html".to_string(),
            declared_src: "/assets/main.ts".to_string(),
        }]
    );
}

#[test]
fn does_not_reflect_a_thrown_page_value_into_an_execution_report() {
    let mut tabs = TabManager::new(320.0, 200.0);
    let tab_id = tabs.default_tab();
    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<script type=\"application/x-blueice-typescript\">throw \"page secret\";</script>",
        Some("https://example.test/app/index.html".to_string()),
    );
    let mut executor = executor();
    executor.synchronize_and_execute(&tabs).unwrap();

    let Some(DirectPageScriptExecutionReport::Rejected { message, .. }) =
        executor.reports().front()
    else {
        panic!("the throwing declaration must produce one rejection")
    };
    assert_eq!(message, "BlueTS lowering rejected the page script");
    assert!(!message.contains("page secret"));
}

#[test]
fn a_document_replacement_executes_its_new_declarations_and_discards_old_metadata() {
    let mut tabs = TabManager::new(320.0, 200.0);
    let tab_id = tabs.default_tab();
    let page: &mut Page = tabs.get_mut(tab_id).unwrap();
    page.load_html_str(
        "<script type=\"application/x-blueice-typescript\">42;</script>",
        Some("https://example.test/app/first.html".to_string()),
    );
    let mut executor = executor();
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(executor.debug_record_count(), 1);

    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<script type=\"application/x-blueice-typescript\">43;</script>",
        Some("https://example.test/app/next.html".to_string()),
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(executor.debug_record_count(), 1);
    assert_eq!(executor.reports().len(), 2);
}
