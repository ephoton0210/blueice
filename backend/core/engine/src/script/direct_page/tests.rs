// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::super::host_typings::{
    core_script_host_type_catalog, HostTypeSurfaceV1, CORE_SCRIPT_DOCUMENT_CONTEXT_PROFILE_V1,
    CORE_SCRIPT_DOCUMENT_TEXT_PROFILE_V1, CORE_SCRIPT_EMPTY_PROFILE_V1,
};
use super::*;
use blueice_bluets::{AuthorizedModule, AuthorizedModuleResolution};

fn catalog() -> HostTypeSurfaceCatalogV1 {
    HostTypeSurfaceCatalogV1::new([HostTypeSurfaceV1::new(
        LANGUAGE_VERSION,
        "test-page-v1",
        "test-empty-v1",
        Vec::new(),
    )])
    .unwrap()
}

fn loaded_tabs() -> (TabManager, TabId) {
    let mut tabs = TabManager::new(320.0, 200.0);
    let tab_id = tabs.default_tab();
    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<main id=\"app\"></main>",
        Some("https://example.test/app/index.html".to_string()),
    );
    (tabs, tab_id)
}

fn request<'a>(
    tab_id: TabId,
    loader: &'a AuthorizedModuleLoader,
    artifact: &'a GeneratedHostTypingsV1,
) -> DirectPageScriptRequest<'a> {
    DirectPageScriptRequest {
        tab_id,
        kind: DirectPageScriptKind::Classic,
        entry: "page:///app/main.ts".to_string(),
        loader,
        compiler_options: CompilerOptions::default(),
        feature_profile: "test-empty-v1".to_string(),
        supplied_manifest: &artifact.manifest,
        supplied_declaration_source: &artifact.declaration_source,
        supplied_runtime_bindings: &artifact.runtime_bindings,
    }
}

#[test]
fn admits_only_a_verified_profile_and_closed_graph_into_its_open_realm() {
    let profiles = catalog();
    let artifact = profiles.generate("test-empty-v1").unwrap();
    let loader = AuthorizedModuleLoader::new(
        [AuthorizedModule::new(
            "page:///app/main.ts",
            "const answer: number = 40 + 2; answer;",
        )],
        [],
    )
    .unwrap();
    let (tabs, tab_id) = loaded_tabs();
    let mut host = DirectPageScriptHost::new(profiles);
    assert_eq!(
        host.execute(&tabs, request(tab_id, &loader, &artifact))
            .unwrap(),
        blueice_bluejs::Value::Number(42.0)
    );
    assert_eq!(host.debug_record_count(), 1);
    let stats = host.realm_stats(tab_id).unwrap();
    assert_eq!(stats.tab_id, tab_id.as_u64());
    assert_eq!(stats.program_count, 1);
    assert!(stats.bytecode_bytes > 0);
}

#[test]
fn configured_bytecode_limit_rejects_before_direct_page_execution() {
    let profiles = catalog();
    let artifact = profiles.generate("test-empty-v1").unwrap();
    let loader = AuthorizedModuleLoader::new(
        [AuthorizedModule::new(
            "page:///app/main.ts",
            "const answer: number = 42; answer;",
        )],
        [],
    )
    .unwrap();
    let (tabs, tab_id) = loaded_tabs();
    let realms = DirectPageRealmOwner::new(
        blueice_bluejs::BlueJsPageRuntimeConfig {
            max_bytecode_bytes_per_realm: 1,
            ..blueice_bluejs::BlueJsPageRuntimeConfig::default()
        },
        blueice_bluets_bluejs::DirectDebugRetentionLimits::default(),
    )
    .unwrap();
    let mut host = DirectPageScriptHost::with_realm_owner(profiles, realms);

    assert!(matches!(
        host.execute(&tabs, request(tab_id, &loader, &artifact)),
        Err(DirectPageScriptError::Bridge(BridgeError::PageRuntime(
            blueice_bluejs::BlueJsPageRuntimeError::BytecodeLimit {
                tab_id: failed_tab,
                limit: 1,
            }
        ))) if failed_tab == tab_id.as_u64()
    ));
    assert_eq!(host.debug_record_count(), 0);
    let stats = host.realm_stats(tab_id).unwrap();
    assert_eq!(stats.program_count, 0);
    assert_eq!(stats.bytecode_bytes, 0);
}

#[test]
fn document_text_profile_executes_against_the_current_document_only() {
    let profiles = core_script_host_type_catalog();
    let artifact = profiles
        .generate(CORE_SCRIPT_DOCUMENT_TEXT_PROFILE_V1)
        .unwrap();
    let loader = AuthorizedModuleLoader::new(
        [AuthorizedModule::new(
            "page:///app/main.ts",
            "const text: string = blueiceDocumentText(); text;",
        )],
        [],
    )
    .unwrap();
    let (mut tabs, tab_id) = loaded_tabs();
    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<main>first <strong>document</strong></main>",
        Some("https://example.test/app/index.html".to_string()),
    );
    let mut host = DirectPageScriptHost::new(profiles);
    assert_eq!(
        host.execute(
            &tabs,
            DirectPageScriptRequest {
                tab_id,
                kind: DirectPageScriptKind::Classic,
                entry: "page:///app/main.ts".to_string(),
                loader: &loader,
                compiler_options: CompilerOptions::default(),
                feature_profile: CORE_SCRIPT_DOCUMENT_TEXT_PROFILE_V1.to_string(),
                supplied_manifest: &artifact.manifest,
                supplied_declaration_source: &artifact.declaration_source,
                supplied_runtime_bindings: &artifact.runtime_bindings,
            },
        )
        .unwrap(),
        blueice_bluejs::Value::String("first document".into())
    );

    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<main>replacement document</main>",
        Some("https://example.test/app/next.html".to_string()),
    );
    assert_eq!(
        host.execute(
            &tabs,
            DirectPageScriptRequest {
                tab_id,
                kind: DirectPageScriptKind::Classic,
                entry: "page:///app/main.ts".to_string(),
                loader: &loader,
                compiler_options: CompilerOptions::default(),
                feature_profile: CORE_SCRIPT_DOCUMENT_TEXT_PROFILE_V1.to_string(),
                supplied_manifest: &artifact.manifest,
                supplied_declaration_source: &artifact.declaration_source,
                supplied_runtime_bindings: &artifact.runtime_bindings,
            },
        )
        .unwrap(),
        blueice_bluejs::Value::String("replacement document".into())
    );
}

#[test]
fn document_text_contract_rejects_an_oversized_snapshot_before_admission() {
    let profiles = core_script_host_type_catalog();
    let artifact = profiles
        .generate(CORE_SCRIPT_DOCUMENT_TEXT_PROFILE_V1)
        .unwrap();
    let loader = AuthorizedModuleLoader::new(
        [AuthorizedModule::new(
            "page:///app/main.ts",
            "blueiceDocumentText();",
        )],
        [],
    )
    .unwrap();
    let (mut tabs, tab_id) = loaded_tabs();
    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<main>oversized document snapshot</main>",
        Some("https://example.test/app/index.html".to_string()),
    );
    let mut host = DirectPageScriptHost::with_realm_owner_and_contract_limits(
        profiles,
        DirectPageRealmOwner::default(),
        CoreScriptBindingContractLimits {
            document_text: blueice_bluets::ValidationLimits {
                max_string_bytes: 8,
                ..blueice_bluets::ValidationLimits::default()
            },
            ..CoreScriptBindingContractLimits::default()
        },
    );

    assert!(matches!(
        host.execute(
            &tabs,
            DirectPageScriptRequest {
                tab_id,
                kind: DirectPageScriptKind::Classic,
                entry: "page:///app/main.ts".to_string(),
                loader: &loader,
                compiler_options: CompilerOptions::default(),
                feature_profile: CORE_SCRIPT_DOCUMENT_TEXT_PROFILE_V1.to_string(),
                supplied_manifest: &artifact.manifest,
                supplied_declaration_source: &artifact.declaration_source,
                supplied_runtime_bindings: &artifact.runtime_bindings,
            },
        ),
        Err(DirectPageScriptError::BindingContractViolation {
            stable_binding_id: "dom.document-text",
            contract_id: CORE_SCRIPT_DOCUMENT_TEXT_RESULT_CONTRACT_V1,
            ..
        })
    ));
    assert_eq!(host.debug_record_count(), 0);
    let stats = host.realm_stats(tab_id).unwrap();
    assert_eq!(stats.program_count, 0);
    assert_eq!(stats.bytecode_bytes, 0);
}

#[test]
fn direct_page_validation_work_is_charged_to_the_initiating_tab_and_generation() {
    let profiles = core_script_host_type_catalog();
    let artifact = profiles
        .generate(CORE_SCRIPT_DOCUMENT_CONTEXT_PROFILE_V1)
        .unwrap();
    let loader = AuthorizedModuleLoader::new(
        [AuthorizedModule::new(
            "page:///app/main.ts",
            "blueiceDocumentText();",
        )],
        [],
    )
    .unwrap();
    let (mut tabs, first_tab) = loaded_tabs();
    tabs.get_mut(first_tab).unwrap().load_html_str(
        "<main>ok</main>",
        Some("https://example.test/first.html".to_string()),
    );
    let second_tab = tabs.open_tab();
    tabs.get_mut(second_tab).unwrap().load_html_str(
        "<main>long</main>",
        Some("https://example.test/second.html".to_string()),
    );
    let mut limits = CoreScriptBindingContractLimits::default();
    limits.document_text.max_string_bytes = 3;
    let mut host = DirectPageScriptHost::with_realm_owner_and_contract_limits(
        profiles,
        DirectPageRealmOwner::default(),
        limits,
    );
    let execute = |host: &mut DirectPageScriptHost, tabs: &TabManager, tab_id| {
        host.execute(
            tabs,
            DirectPageScriptRequest {
                tab_id,
                kind: DirectPageScriptKind::Classic,
                entry: "page:///app/main.ts".to_string(),
                loader: &loader,
                compiler_options: CompilerOptions::default(),
                feature_profile: CORE_SCRIPT_DOCUMENT_CONTEXT_PROFILE_V1.to_string(),
                supplied_manifest: &artifact.manifest,
                supplied_declaration_source: &artifact.declaration_source,
                supplied_runtime_bindings: &artifact.runtime_bindings,
            },
        )
    };
    assert_eq!(
        execute(&mut host, &tabs, first_tab).unwrap(),
        blueice_bluejs::Value::String("ok".into())
    );
    assert!(matches!(
        execute(&mut host, &tabs, second_tab),
        Err(DirectPageScriptError::BindingContractViolation { .. })
    ));

    let origin_bytes = "https://example.test".len() as u64;
    let first = host.validation_usage(first_tab).unwrap();
    assert_eq!(first.document_generation, 2);
    assert_eq!(first.attempts, 2);
    assert_eq!(first.visited_nodes, 2);
    assert_eq!(first.copied_value_bytes, 2 + origin_bytes);
    let second = host.validation_usage(second_tab).unwrap();
    assert_eq!(second.document_generation, 1);
    assert_eq!(second.attempts, 1);
    assert_eq!(second.visited_nodes, 1);
    assert_eq!(second.copied_value_bytes, 4);
    assert_eq!(host.validation_usage(first_tab), Some(first));

    tabs.get_mut(first_tab).unwrap().load_html_str(
        "<main>new</main>",
        Some("https://example.test/replacement.html".to_string()),
    );
    host.synchronize_tab(&tabs, first_tab).unwrap();
    assert_eq!(host.validation_usage(first_tab), None);
    assert_eq!(host.validation_usage(second_tab), Some(second));
    host.close_page(second_tab);
    assert_eq!(host.validation_usage(second_tab), None);
}

#[test]
fn direct_page_retained_debug_payload_is_charged_to_each_live_tab() {
    let profiles = catalog();
    let artifact = profiles.generate("test-empty-v1").unwrap();
    let short_loader = AuthorizedModuleLoader::new(
        [AuthorizedModule::new(
            "page:///app/main.ts",
            "interface ShortShape { value: string; } const shortName: string = 'one'; shortName;",
        )],
        [],
    )
    .unwrap();
    let long_loader = AuthorizedModuleLoader::new(
        [AuthorizedModule::new(
            "page:///app/main.ts",
            "interface MuchLongerRetainedContractShape { value: string; } const muchLongerRetainedSymbolName: string = 'two'; muchLongerRetainedSymbolName;",
        )],
        [],
    )
    .unwrap();
    let (mut tabs, first_tab) = loaded_tabs();
    let second_tab = tabs.open_tab();
    tabs.get_mut(second_tab).unwrap().load_html_str(
        "<main>second</main>",
        Some("https://example.test/second.html".to_string()),
    );
    let mut host = DirectPageScriptHost::new(profiles);
    assert_eq!(host.retained_debug_payload_bytes(first_tab).unwrap(), None);
    let execute = |host: &mut DirectPageScriptHost,
                   tabs: &TabManager,
                   tab_id,
                   loader: &AuthorizedModuleLoader| {
        host.execute(
            tabs,
            DirectPageScriptRequest {
                tab_id,
                kind: DirectPageScriptKind::Classic,
                entry: "page:///app/main.ts".to_string(),
                loader,
                compiler_options: CompilerOptions::default(),
                feature_profile: "test-empty-v1".to_string(),
                supplied_manifest: &artifact.manifest,
                supplied_declaration_source: &artifact.declaration_source,
                supplied_runtime_bindings: &artifact.runtime_bindings,
            },
        )
    };
    assert_eq!(
        execute(&mut host, &tabs, first_tab, &short_loader).unwrap(),
        blueice_bluejs::Value::String("one".into())
    );
    assert_eq!(
        execute(&mut host, &tabs, second_tab, &long_loader).unwrap(),
        blueice_bluejs::Value::String("two".into())
    );
    let first_bytes = host
        .retained_debug_payload_bytes(first_tab)
        .unwrap()
        .unwrap();
    let second_bytes = host
        .retained_debug_payload_bytes(second_tab)
        .unwrap()
        .unwrap();
    assert!(first_bytes > 0);
    assert!(
        second_bytes > first_bytes,
        "longer retained names must cost more"
    );
    assert_eq!(
        host.retained_debug_payload_bytes(first_tab).unwrap(),
        Some(first_bytes)
    );

    tabs.get_mut(first_tab).unwrap().load_html_str(
        "<main>replacement</main>",
        Some("https://example.test/replacement.html".to_string()),
    );
    host.synchronize_tab(&tabs, first_tab).unwrap();
    assert_eq!(
        host.retained_debug_payload_bytes(first_tab).unwrap(),
        Some(0)
    );
    assert_eq!(
        host.retained_debug_payload_bytes(second_tab).unwrap(),
        Some(second_bytes)
    );
    host.close_page(second_tab);
    assert_eq!(host.retained_debug_payload_bytes(second_tab).unwrap(), None);
}

#[test]
fn strict_runtime_rejects_missing_unreifiable_and_unchecked_live_boundaries_separately() {
    let profiles = core_script_host_type_catalog();
    let artifact = profiles
        .generate(CORE_SCRIPT_DOCUMENT_TEXT_PROFILE_V1)
        .unwrap();
    let loader = AuthorizedModuleLoader::new(
        [AuthorizedModule::new(
            "page:///app/main.ts",
            "blueiceDocumentText();",
        )],
        [],
    )
    .unwrap();
    let (tabs, tab_id) = loaded_tabs();
    for case in 0..3 {
        let mut host = DirectPageScriptHost::new(profiles.clone());
        let text_record = host
            .boundary_records
            .iter()
            .position(|record| record.stable_binding_id == "dom.document-text")
            .unwrap();
        match case {
            0 => {
                host.boundary_records.remove(text_record);
            }
            1 => host.boundary_records[text_record].declared_type = blueice_bluets::Type::Any,
            2 => {
                host.boundary_records[text_record].validation =
                    super::super::contracts::HostBindingBoundaryValidation::Unchecked;
            }
            _ => unreachable!(),
        }
        let error = host
            .execute(
                &tabs,
                DirectPageScriptRequest {
                    tab_id,
                    kind: DirectPageScriptKind::Classic,
                    entry: "page:///app/main.ts".to_string(),
                    loader: &loader,
                    compiler_options: CompilerOptions {
                        runtime_policy: RuntimePolicy::StrictRuntime,
                        ..CompilerOptions::default()
                    },
                    feature_profile: CORE_SCRIPT_DOCUMENT_TEXT_PROFILE_V1.to_string(),
                    supplied_manifest: &artifact.manifest,
                    supplied_declaration_source: &artifact.declaration_source,
                    supplied_runtime_bindings: &artifact.runtime_bindings,
                },
            )
            .unwrap_err();
        let DirectPageScriptError::StrictRuntimeBoundary(diagnostic) = error else {
            panic!("strict-runtime must return a typed boundary diagnostic: {error:?}")
        };
        let binding_id = match (case, diagnostic) {
            (0, StrictRuntimeBoundaryDiagnostic::MissingContract { stable_binding_id })
            | (1, StrictRuntimeBoundaryDiagnostic::UnreifiableType { stable_binding_id })
            | (2, StrictRuntimeBoundaryDiagnostic::UncheckedBoundary { stable_binding_id }) => {
                stable_binding_id
            }
            (_, other) => panic!("strict-runtime chose the wrong refusal: {other:?}"),
        };
        assert_eq!(binding_id, "dom.document-text");
        assert_eq!(host.debug_record_count(), 0);
        assert_eq!(
            host.realm_stats(tab_id)
                .map(|stats| stats.program_count)
                .unwrap_or(0),
            0
        );
    }
}

#[test]
fn strict_runtime_admits_reviewed_snapshot_contracts_and_validates_before_capture() {
    let profiles = core_script_host_type_catalog();
    let artifact = profiles
        .generate(CORE_SCRIPT_DOCUMENT_CONTEXT_PROFILE_V1)
        .unwrap();
    let loader = AuthorizedModuleLoader::new(
        [AuthorizedModule::new(
            "page:///app/main.ts",
            "blueiceDocumentOrigin() + ':' + blueiceDocumentText();",
        )],
        [],
    )
    .unwrap();
    let (mut tabs, tab_id) = loaded_tabs();
    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<main>reviewed content</main>",
        Some("https://example.test/app/index.html".to_string()),
    );
    let request = || DirectPageScriptRequest {
        tab_id,
        kind: DirectPageScriptKind::Classic,
        entry: "page:///app/main.ts".to_string(),
        loader: &loader,
        compiler_options: CompilerOptions {
            runtime_policy: RuntimePolicy::StrictRuntime,
            ..CompilerOptions::default()
        },
        feature_profile: CORE_SCRIPT_DOCUMENT_CONTEXT_PROFILE_V1.to_string(),
        supplied_manifest: &artifact.manifest,
        supplied_declaration_source: &artifact.declaration_source,
        supplied_runtime_bindings: &artifact.runtime_bindings,
    };

    let mut reviewed = DirectPageScriptHost::new(profiles.clone());
    assert_eq!(
        reviewed.execute(&tabs, request()).unwrap(),
        blueice_bluejs::Value::String("https://example.test:reviewed content".into())
    );
    assert_eq!(reviewed.debug_record_count(), 1);

    let mut mismatched = DirectPageScriptHost::new(profiles.clone());
    mismatched
        .boundary_records
        .iter_mut()
        .find(|record| record.stable_binding_id == "dom.document-text")
        .unwrap()
        .declared_type = blueice_bluets::Type::Number;
    assert!(matches!(
        mismatched.execute(&tabs, request()),
        Err(DirectPageScriptError::StrictRuntimeBoundary(
            StrictRuntimeBoundaryDiagnostic::MissingContract { stable_binding_id }
        )) if stable_binding_id == "dom.document-text"
    ));
    assert_eq!(mismatched.debug_record_count(), 0);

    let mut mismatched_budget = DirectPageScriptHost::new(profiles.clone());
    mismatched_budget
        .boundary_records
        .iter_mut()
        .find(|record| record.stable_binding_id == "dom.document-text")
        .unwrap()
        .validation_limits
        .max_string_bytes += 1;
    assert!(matches!(
        mismatched_budget.execute(&tabs, request()),
        Err(DirectPageScriptError::StrictRuntimeBoundary(
            StrictRuntimeBoundaryDiagnostic::MissingContract { stable_binding_id }
        )) if stable_binding_id == "dom.document-text"
    ));
    assert_eq!(mismatched_budget.debug_record_count(), 0);

    let mut tight_limits = CoreScriptBindingContractLimits::default();
    tight_limits.document_text.max_string_bytes = 8;
    let mut bounded = DirectPageScriptHost::with_realm_owner_and_contract_limits(
        profiles,
        DirectPageRealmOwner::default(),
        tight_limits,
    );
    assert!(matches!(
        bounded.execute(&tabs, request()),
        Err(DirectPageScriptError::BindingContractViolation {
            stable_binding_id: "dom.document-text",
            contract_id: CORE_SCRIPT_DOCUMENT_TEXT_RESULT_CONTRACT_V1,
            ..
        })
    ));
    assert_eq!(bounded.debug_record_count(), 0);
    assert_eq!(bounded.realm_stats(tab_id).unwrap().program_count, 0);
    assert!(!bounded.bound_profiles.contains_key(&tab_id));
    // BlueJS rejects duplicate host globals. Successful same-name registration
    // in this exact live realm proves the rejected snapshot never installed
    // its callback or copied its value into the VM.
    bounded
        .realms
        .configure_realm_bindings(tab_id.as_u64(), |bindings| {
            bindings.install_global_function(
                "blueiceDocumentText",
                0,
                |_args: &[blueice_bluejs::HostValue]| {
                    Ok(blueice_bluejs::HostValue::String("safe".into()))
                },
            )
        })
        .expect("the rejected snapshot must not have installed its host global");
}

#[test]
fn document_context_profile_exposes_only_canonical_origin_and_text_snapshot() {
    let profiles = core_script_host_type_catalog();
    let artifact = profiles
        .generate(CORE_SCRIPT_DOCUMENT_CONTEXT_PROFILE_V1)
        .unwrap();
    let loader = AuthorizedModuleLoader::new(
        [AuthorizedModule::new(
            "page:///app/main.ts",
            concat!(
                "const text: string = blueiceDocumentText(); ",
                "const origin: string = blueiceDocumentOrigin(); ",
                "origin + ':' + text;"
            ),
        )],
        [],
    )
    .unwrap();
    let (mut tabs, tab_id) = loaded_tabs();
    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<main>context document</main>",
        Some("https://EXAMPLE.test:443/app/index.html?private=value#part".to_string()),
    );
    let mut host = DirectPageScriptHost::new(profiles);

    assert_eq!(
        host.execute(
            &tabs,
            DirectPageScriptRequest {
                tab_id,
                kind: DirectPageScriptKind::Classic,
                entry: "page:///app/main.ts".to_string(),
                loader: &loader,
                compiler_options: CompilerOptions::default(),
                feature_profile: CORE_SCRIPT_DOCUMENT_CONTEXT_PROFILE_V1.to_string(),
                supplied_manifest: &artifact.manifest,
                supplied_declaration_source: &artifact.declaration_source,
                supplied_runtime_bindings: &artifact.runtime_bindings,
            },
        )
        .unwrap(),
        blueice_bluejs::Value::String("https://example.test:context document".into())
    );

    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<main>replacement context</main>",
        Some("https://other.test/new/path?private=next".to_string()),
    );
    assert_eq!(
        host.execute(
            &tabs,
            DirectPageScriptRequest {
                tab_id,
                kind: DirectPageScriptKind::Classic,
                entry: "page:///app/main.ts".to_string(),
                loader: &loader,
                compiler_options: CompilerOptions::default(),
                feature_profile: CORE_SCRIPT_DOCUMENT_CONTEXT_PROFILE_V1.to_string(),
                supplied_manifest: &artifact.manifest,
                supplied_declaration_source: &artifact.declaration_source,
                supplied_runtime_bindings: &artifact.runtime_bindings,
            },
        )
        .unwrap(),
        blueice_bluejs::Value::String("https://other.test:replacement context".into())
    );
}

#[test]
fn non_empty_profiles_require_a_matching_runtime_installer() {
    let profiles = HostTypeSurfaceCatalogV1::new([HostTypeSurfaceV1::new(
        LANGUAGE_VERSION,
        "test-page-v1",
        "uninstalled-binding-v1",
        vec![super::super::host_typings::HostTypeBindingV1::new(
            "test.uninstalled",
            "declare function unavailable(): number;",
            super::super::host_typings::HostBindingRoleV1::Value,
            "global.unavailable",
            "test",
            "test",
            "test-page-v1",
        )],
    )])
    .unwrap();
    let artifact = profiles.generate("uninstalled-binding-v1").unwrap();
    let loader = AuthorizedModuleLoader::new(
        [AuthorizedModule::new(
            "page:///app/main.ts",
            "unavailable();",
        )],
        [],
    )
    .unwrap();
    let (tabs, tab_id) = loaded_tabs();
    let mut host = DirectPageScriptHost::new(profiles);
    assert!(matches!(
        host.execute(
            &tabs,
            DirectPageScriptRequest {
                tab_id,
                kind: DirectPageScriptKind::Classic,
                entry: "page:///app/main.ts".to_string(),
                loader: &loader,
                compiler_options: CompilerOptions::default(),
                feature_profile: "uninstalled-binding-v1".to_string(),
                supplied_manifest: &artifact.manifest,
                supplied_declaration_source: &artifact.declaration_source,
                supplied_runtime_bindings: &artifact.runtime_bindings,
            },
        ),
        Err(DirectPageScriptError::RuntimeBindingProfileUnavailable)
    ));
}

#[test]
fn document_text_profile_rejects_arguments_during_page_profile_checking() {
    let profiles = core_script_host_type_catalog();
    let artifact = profiles
        .generate(CORE_SCRIPT_DOCUMENT_TEXT_PROFILE_V1)
        .unwrap();
    let loader = AuthorizedModuleLoader::new(
        [AuthorizedModule::new(
            "page:///app/main.ts",
            "blueiceDocumentText(1);",
        )],
        [],
    )
    .unwrap();
    let (tabs, tab_id) = loaded_tabs();
    let mut host = DirectPageScriptHost::new(profiles);
    assert!(matches!(
        host.execute(
            &tabs,
            DirectPageScriptRequest {
                tab_id,
                kind: DirectPageScriptKind::Classic,
                entry: "page:///app/main.ts".to_string(),
                loader: &loader,
                compiler_options: CompilerOptions::default(),
                feature_profile: CORE_SCRIPT_DOCUMENT_TEXT_PROFILE_V1.to_string(),
                supplied_manifest: &artifact.manifest,
                supplied_declaration_source: &artifact.declaration_source,
                supplied_runtime_bindings: &artifact.runtime_bindings,
            },
        ),
        Err(DirectPageScriptError::Bridge(BridgeError::BlueTs(diagnostics)))
            if diagnostics.iter().any(|diagnostic| {
                diagnostic.code == blueice_bluets::DiagnosticCode::TypeMismatch
            })
    ));
}

#[test]
fn empty_profile_rejects_document_context_globals_before_vm_admission() {
    let profiles = core_script_host_type_catalog();
    let artifact = profiles.generate(CORE_SCRIPT_EMPTY_PROFILE_V1).unwrap();
    let (tabs, tab_id) = loaded_tabs();
    let mut host = DirectPageScriptHost::new(profiles);

    for global in ["blueiceDocumentText", "blueiceDocumentOrigin"] {
        let loader = AuthorizedModuleLoader::new(
            [AuthorizedModule::new(
                "page:///app/main.ts",
                format!("{global}();"),
            )],
            [],
        )
        .unwrap();
        let expected_message = format!("function {global} is not declared by this page profile");
        assert!(matches!(
            host.execute(
                &tabs,
                DirectPageScriptRequest {
                    tab_id,
                    kind: DirectPageScriptKind::Classic,
                    entry: "page:///app/main.ts".to_string(),
                    loader: &loader,
                    compiler_options: CompilerOptions::default(),
                    feature_profile: CORE_SCRIPT_EMPTY_PROFILE_V1.to_string(),
                    supplied_manifest: &artifact.manifest,
                    supplied_declaration_source: &artifact.declaration_source,
                    supplied_runtime_bindings: &artifact.runtime_bindings,
                },
            ),
            Err(DirectPageScriptError::Bridge(BridgeError::BlueTs(diagnostics)))
                if diagnostics.iter().any(|diagnostic| {
                    diagnostic.code == blueice_bluets::DiagnosticCode::UnknownName
                        && diagnostic.message == expected_message
                })
        ));
    }
    assert_eq!(host.debug_record_count(), 0);
}

#[test]
fn a_page_realm_cannot_switch_binding_profiles_before_navigation() {
    let profiles = core_script_host_type_catalog();
    let empty = profiles.generate(CORE_SCRIPT_EMPTY_PROFILE_V1).unwrap();
    let document_text = profiles
        .generate(CORE_SCRIPT_DOCUMENT_TEXT_PROFILE_V1)
        .unwrap();
    let (mut tabs, tab_id) = loaded_tabs();
    let mut host = DirectPageScriptHost::new(profiles);
    host.synchronize_tab(&tabs, tab_id).unwrap();
    host.configure_profile_bindings(&tabs, tab_id, CORE_SCRIPT_EMPTY_PROFILE_V1, &empty)
        .unwrap();
    assert!(matches!(
        host.configure_profile_bindings(
            &tabs,
            tab_id,
            CORE_SCRIPT_DOCUMENT_TEXT_PROFILE_V1,
            &document_text,
        ),
        Err(DirectPageScriptError::BindingProfileAlreadySelected)
    ));

    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<main>replacement</main>",
        Some("https://example.test/app/next.html".to_string()),
    );
    host.synchronize_tab(&tabs, tab_id).unwrap();
    host.configure_profile_bindings(
        &tabs,
        tab_id,
        CORE_SCRIPT_DOCUMENT_TEXT_PROFILE_V1,
        &document_text,
    )
    .unwrap();
}

#[test]
fn rejects_callers_attempt_to_bypass_the_verified_profile() {
    let profiles = catalog();
    let artifact = profiles.generate("test-empty-v1").unwrap();
    let loader =
        AuthorizedModuleLoader::new([AuthorizedModule::new("page:///app/main.ts", "1;")], [])
            .unwrap();
    let (tabs, tab_id) = loaded_tabs();
    let mut host = DirectPageScriptHost::new(profiles);
    let mut request = request(tab_id, &loader, &artifact);
    request
        .compiler_options
        .ambient_declaration_modules
        .push(blueice_bluets::ModuleSource::new(
            "page:///untrusted.d.ts",
            "declare const unsafe: any;",
        ));
    assert!(matches!(
        host.execute(&tabs, request),
        Err(DirectPageScriptError::CallerSuppliedAmbientDeclarations)
    ));
    assert_eq!(host.debug_record_count(), 0);
}

#[test]
fn admits_a_verified_authorized_module_graph_into_the_same_page_realm() {
    let profiles = catalog();
    let artifact = profiles.generate("test-empty-v1").unwrap();
    let loader = AuthorizedModuleLoader::new(
        [
            AuthorizedModule::new(
                "page:///app/main.ts",
                "import { value } from './value'; export const answer: number = value + 1; answer;",
            ),
            AuthorizedModule::new("page:///app/value.ts", "export const value: number = 41;"),
        ],
        [AuthorizedModuleResolution::new(
            "page:///app/main.ts",
            "./value",
            "page:///app/value.ts",
        )],
    )
    .unwrap();
    let (tabs, tab_id) = loaded_tabs();
    let mut host = DirectPageScriptHost::new(profiles);
    let mut request = request(tab_id, &loader, &artifact);
    request.kind = DirectPageScriptKind::Module;
    assert_eq!(
        host.execute(&tabs, request).unwrap(),
        blueice_bluejs::Value::Number(42.0)
    );
    assert_eq!(host.debug_record_count(), 2);
}

#[test]
fn same_origin_document_replacement_recreates_the_page_realm() {
    let profiles = catalog();
    let artifact = profiles.generate("test-empty-v1").unwrap();
    let first = AuthorizedModuleLoader::new(
        [AuthorizedModule::new("page:///app/main.ts", "40 + 2;")],
        [],
    )
    .unwrap();
    let second = AuthorizedModuleLoader::new(
        [AuthorizedModule::new("page:///app/main.ts", "40 + 3;")],
        [],
    )
    .unwrap();
    let (mut tabs, tab_id) = loaded_tabs();
    let mut host = DirectPageScriptHost::new(profiles);

    assert_eq!(
        host.execute(&tabs, request(tab_id, &first, &artifact))
            .unwrap(),
        blueice_bluejs::Value::Number(42.0)
    );
    assert_eq!(host.debug_record_count(), 1);

    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<main id=\"next\"></main>",
        Some("https://example.test/app/next.html".to_string()),
    );
    assert_eq!(
        host.execute(&tabs, request(tab_id, &second, &artifact))
            .unwrap(),
        blueice_bluejs::Value::Number(43.0)
    );
    assert_eq!(
        host.debug_record_count(),
        1,
        "a same-origin replacement must prune the preceding realm metadata"
    );
    assert_eq!(host.realm_stats(tab_id).unwrap().program_count, 1);
}

#[test]
fn navigation_releases_a_realm_bytecode_charge_before_the_next_script_admission() {
    let profiles = catalog();
    let artifact = profiles.generate("test-empty-v1").unwrap();
    let loader =
        AuthorizedModuleLoader::new([AuthorizedModule::new("page:///app/main.ts", "42;")], [])
            .unwrap();
    let (mut tabs, tab_id) = loaded_tabs();
    let mut host = DirectPageScriptHost::new(profiles);
    host.execute(&tabs, request(tab_id, &loader, &artifact))
        .unwrap();
    assert!(host.realm_stats(tab_id).unwrap().bytecode_bytes > 0);

    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<main id=\"next\"></main>",
        Some("https://example.test/app/next.html".to_string()),
    );
    host.synchronize_tab(&tabs, tab_id).unwrap();
    let stats = host.realm_stats(tab_id).unwrap();
    assert_eq!(stats.program_count, 0);
    assert_eq!(stats.bytecode_bytes, 0);
    assert_eq!(host.debug_record_count(), 0);
}

#[test]
fn synchronizing_tabs_prunes_a_closed_tabs_realm() {
    let profiles = catalog();
    let artifact = profiles.generate("test-empty-v1").unwrap();
    let loader =
        AuthorizedModuleLoader::new([AuthorizedModule::new("page:///app/main.ts", "42;")], [])
            .unwrap();
    let (mut tabs, tab_id) = loaded_tabs();
    let mut host = DirectPageScriptHost::new(profiles);
    host.execute(&tabs, request(tab_id, &loader, &artifact))
        .unwrap();
    assert_eq!(host.debug_record_count(), 1);

    assert!(tabs.close_tab(tab_id));
    host.synchronize_tabs(&tabs).unwrap();
    assert_eq!(host.debug_record_count(), 0);
    assert!(matches!(
        host.synchronize_tab(&tabs, tab_id),
        Err(DirectPageScriptError::UnknownTab { .. })
    ));
}

#[test]
fn synchronizing_tabs_does_not_allocate_a_realm_for_an_unadmitted_page() {
    let (tabs, _) = loaded_tabs();
    let mut host = DirectPageScriptHost::new(catalog());
    host.synchronize_tabs(&tabs).unwrap();
    assert_eq!(host.debug_record_count(), 0);
}

#[test]
fn blank_or_builtin_pages_do_not_gain_a_caller_selected_origin() {
    let profiles = catalog();
    let artifact = profiles.generate("test-empty-v1").unwrap();
    let loader =
        AuthorizedModuleLoader::new([AuthorizedModule::new("page:///app/main.ts", "42;")], [])
            .unwrap();
    let mut tabs = TabManager::new(320.0, 200.0);
    let tab_id = tabs.default_tab();
    let mut host = DirectPageScriptHost::new(profiles);
    assert!(matches!(
        host.execute(&tabs, request(tab_id, &loader, &artifact)),
        Err(DirectPageScriptError::PageHasNoUrl { .. })
    ));

    tabs.get_mut(tab_id)
        .unwrap()
        .load_html_str("", Some("about:blank".to_string()));
    assert!(matches!(
        host.execute(&tabs, request(tab_id, &loader, &artifact)),
        Err(DirectPageScriptError::InvalidPageUrl { .. })
    ));
    assert_eq!(host.debug_record_count(), 0);
}

#[test]
fn executes_an_inline_document_declaration_as_a_closed_single_module() {
    let profiles = catalog();
    let artifact = profiles.generate("test-empty-v1").unwrap();
    let mut tabs = TabManager::new(320.0, 200.0);
    let tab_id = tabs.default_tab();
    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<script type=\"application/x-blueice-typescript\">const answer: number = 40 + 2; answer;</script>",
        Some("https://example.test/app/index.html".to_string()),
    );
    let mut host = DirectPageScriptHost::new(profiles);

    assert_eq!(
        host.execute_inline(
            &tabs,
            DirectInlinePageScriptRequest {
                tab_id,
                ordinal: 0,
                compiler_options: CompilerOptions::default(),
                feature_profile: "test-empty-v1".to_string(),
                supplied_manifest: &artifact.manifest,
                supplied_declaration_source: &artifact.declaration_source,
                supplied_runtime_bindings: &artifact.runtime_bindings,
            },
        )
        .unwrap(),
        blueice_bluejs::Value::Number(42.0)
    );
    assert_eq!(host.debug_record_count(), 1);
}

#[test]
fn executes_an_inline_module_declaration_without_a_second_resolver() {
    let profiles = catalog();
    let artifact = profiles.generate("test-empty-v1").unwrap();
    let mut tabs = TabManager::new(320.0, 200.0);
    let tab_id = tabs.default_tab();
    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<script type=\"application/x-blueice-typescript-module\">export const answer: number = 40 + 2; answer;</script>",
        Some("https://example.test/app/index.html".to_string()),
    );
    let mut host = DirectPageScriptHost::new(profiles);

    assert_eq!(
        host.execute_inline(
            &tabs,
            DirectInlinePageScriptRequest {
                tab_id,
                ordinal: 0,
                compiler_options: CompilerOptions::default(),
                feature_profile: "test-empty-v1".to_string(),
                supplied_manifest: &artifact.manifest,
                supplied_declaration_source: &artifact.declaration_source,
                supplied_runtime_bindings: &artifact.runtime_bindings,
            },
        )
        .unwrap(),
        blueice_bluejs::Value::Number(42.0)
    );
    assert_eq!(host.debug_record_count(), 1);
}

#[test]
fn inline_execution_rejects_external_source_without_loading_it() {
    let profiles = catalog();
    let artifact = profiles.generate("test-empty-v1").unwrap();
    let mut tabs = TabManager::new(320.0, 200.0);
    let tab_id = tabs.default_tab();
    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<script type=\"application/x-blueice-typescript\" src=\"/app.ts\"></script>",
        Some("https://example.test/app/index.html".to_string()),
    );
    let mut host = DirectPageScriptHost::new(profiles);

    assert!(matches!(
        host.execute_inline(
            &tabs,
            DirectInlinePageScriptRequest {
                tab_id,
                ordinal: 0,
                compiler_options: CompilerOptions::default(),
                feature_profile: "test-empty-v1".to_string(),
                supplied_manifest: &artifact.manifest,
                supplied_declaration_source: &artifact.declaration_source,
                supplied_runtime_bindings: &artifact.runtime_bindings,
            },
        ),
        Err(DirectPageScriptError::ExternalScriptRequiresLoader { .. })
    ));
    assert_eq!(host.debug_record_count(), 0);
}

#[test]
fn rejected_external_declaration_still_retires_the_prior_document_realm() {
    let profiles = catalog();
    let artifact = profiles.generate("test-empty-v1").unwrap();
    let mut tabs = TabManager::new(320.0, 200.0);
    let tab_id = tabs.default_tab();
    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<script type=\"application/x-blueice-typescript\">42;</script>",
        Some("https://example.test/app/first.html".to_string()),
    );
    let mut host = DirectPageScriptHost::new(profiles);
    host.execute_inline(
        &tabs,
        DirectInlinePageScriptRequest {
            tab_id,
            ordinal: 0,
            compiler_options: CompilerOptions::default(),
            feature_profile: "test-empty-v1".to_string(),
            supplied_manifest: &artifact.manifest,
            supplied_declaration_source: &artifact.declaration_source,
            supplied_runtime_bindings: &artifact.runtime_bindings,
        },
    )
    .unwrap();
    assert_eq!(host.debug_record_count(), 1);

    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<script type=\"application/x-blueice-typescript\" src=\"/second.ts\"></script>",
        Some("https://example.test/app/second.html".to_string()),
    );
    assert!(matches!(
        host.execute_inline(
            &tabs,
            DirectInlinePageScriptRequest {
                tab_id,
                ordinal: 0,
                compiler_options: CompilerOptions::default(),
                feature_profile: "test-empty-v1".to_string(),
                supplied_manifest: &artifact.manifest,
                supplied_declaration_source: &artifact.declaration_source,
                supplied_runtime_bindings: &artifact.runtime_bindings,
            },
        ),
        Err(DirectPageScriptError::ExternalScriptRequiresLoader { .. })
    ));
    assert_eq!(host.debug_record_count(), 0);
}
