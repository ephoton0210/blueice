// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use blueice_bluets::{
    AuthorizedModule, AuthorizedModuleResolution, DiagnosticCode, RuntimePolicy, SourceSpan,
};

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
            source_map: true,
            declaration: true,
            resolver_fingerprint: "registered-project-resolver-v1".to_string(),
            runtime_policy: RuntimePolicy::Checked,
            ..CompilerOptions::default()
        },
    }
}

#[test]
fn retained_diagnostics_keep_authorized_utf16_locations_across_pages() {
    let registration = registration("const marker = '😀';\r\nconst invalid: number = 'wrong';");
    let unavailable = Diagnostic::error(
        DiagnosticCode::ModuleNotFound,
        SourceSpan::new("project:///app/not-authorized.ts", 0, 1),
        "unavailable source",
    );
    assert_eq!(
        diagnostic_locations(&registration.loader, &[unavailable]),
        [None]
    );
    let mut service = RegisteredProjectCompilerService::default();
    let id = service.register(registration).unwrap();
    let check = service.check(id).unwrap();
    assert!(check.has_errors);
    let index = check
        .retained_diagnostics
        .iter()
        .position(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch)
        .expect("the typed assignment must produce a mismatch diagnostic");
    let location = check.retained_diagnostic_locations[index].unwrap();
    assert_eq!(location.start.line, 1);
    assert_eq!(location.end.line, 1);
    assert!(location.start.column_utf16 < location.end.column_utf16);
    let page = service
        .diagnostic_inventory(check.generation, None, 32)
        .unwrap();
    assert_eq!(page.entries.len(), page.locations.len());
    assert_eq!(page.locations[index], Some(location));
}

#[test]
fn registered_projects_pin_authorized_inputs_and_expose_generation_bound_metadata() {
    let mut service = RegisteredProjectCompilerService::default();
    let id = service
        .register(registration("export const value: number = answer;"))
        .unwrap();
    assert_eq!(
        service.identity(id).unwrap(),
        RegisteredProjectIdentity {
            id,
            canonical_project_root: "project:///app".to_string(),
            canonical_config_root: "project:///app/blue-ts.json".to_string(),
            canonical_output_root: "project:///dist".to_string(),
            entry_module: ENTRY.to_string(),
        }
    );

    let first = service.check(id).unwrap();
    assert!(!first.has_errors, "{:#?}", first.diagnostics.entries);
    assert!(!first.cache_hit);
    assert!(first.artifact_fingerprint.is_some());
    let info = first.static_debug_info.as_ref().unwrap();
    let static_type = service
        .static_type(first.generation, info.types[0].id)
        .unwrap();
    let symbol = service
        .static_symbol(first.generation, info.symbols[0].id)
        .unwrap();
    assert_eq!(static_type.id, info.types[0].id);
    assert_eq!(symbol.id, info.symbols[0].id);
    assert!(service.static_debug_info(first.generation).is_ok());

    let second = service.check(id).unwrap();
    assert!(second.cache_hit);
    assert!(matches!(
        service.static_debug_info(first.generation),
        Err(CompilerServiceError::StaleGeneration { .. })
    ));
}

#[test]
fn diagnostic_pages_are_one_shot_generation_bound_and_source_text_free() {
    let mut service = RegisteredProjectCompilerService::new(CompilerServiceLimits {
        max_retained_diagnostics: 4,
        max_diagnostics: 1,
        max_diagnostic_cursors: 4,
        ..CompilerServiceLimits::default()
    });
    let id = service
        .register(registration(
            "const first: number = 'one'; \
             const second: number = 'two'; \
             const third: number = 'three';",
        ))
        .unwrap();
    let check = service.check(id).unwrap();
    assert!(check.has_errors);
    assert_eq!(check.diagnostics.entries.len(), 1);
    assert!(check.diagnostics.truncated);

    let first = service
        .diagnostic_inventory(check.generation, None, 1)
        .unwrap();
    assert_eq!(first.entries.len(), 1);
    assert!(
        !format!("{:?}", first.entries).contains("const first"),
        "a diagnostic page carries range/prose only, never project source text"
    );
    let cursor = first
        .next_cursor
        .expect("three distinct type failures require a continuation cursor");
    let second = service
        .diagnostic_inventory(check.generation, Some(cursor), 1)
        .unwrap();
    assert_eq!(second.entries.len(), 1);
    assert!(matches!(
        service.diagnostic_inventory(check.generation, Some(cursor), 1),
        Err(CompilerServiceError::InvalidDiagnosticCursor { .. })
    ));

    let later = service.check(id).unwrap();
    assert!(matches!(
        service.diagnostic_inventory(check.generation, second.next_cursor, 1),
        Err(CompilerServiceError::StaleGeneration { .. })
    ));
    assert!(matches!(
        service.diagnostic_inventory(later.generation, second.next_cursor, 1),
        Err(CompilerServiceError::InvalidDiagnosticCursor { .. })
    ));
}

#[test]
fn work_set_pages_are_kind_bound_one_shot_and_generation_bound() {
    let mut service = RegisteredProjectCompilerService::new(CompilerServiceLimits {
        max_work_set_cursors: 4,
        ..CompilerServiceLimits::default()
    });
    let id = service
        .register(registration("export const value: number = answer;"))
        .unwrap();
    let check = service.check(id).unwrap();
    assert_eq!(check.parsed_modules.len(), 2);
    let first = service
        .work_set_inventory(check.generation, WorkSetInventoryKind::Parsed, None, 1)
        .unwrap();
    assert_eq!(first.entries.len(), 1);
    assert!(!first.truncated);
    assert!(!first.entries[0].contains("export const"));
    let cursor = first.next_cursor.unwrap();
    assert!(matches!(
        service.work_set_inventory(
            check.generation,
            WorkSetInventoryKind::Rechecked,
            Some(cursor),
            1,
        ),
        Err(CompilerServiceError::InvalidWorkSetCursor { .. })
    ));
    let second = service
        .work_set_inventory(
            check.generation,
            WorkSetInventoryKind::Parsed,
            Some(cursor),
            1,
        )
        .unwrap();
    assert_eq!(second.entries.len(), 1);
    assert!(second.next_cursor.is_none());
    assert_ne!(first.entries, second.entries);
    assert!(matches!(
        service.work_set_inventory(
            check.generation,
            WorkSetInventoryKind::Parsed,
            Some(cursor),
            1,
        ),
        Err(CompilerServiceError::InvalidWorkSetCursor { .. })
    ));
    let later = service.check(id).unwrap();
    assert!(matches!(
        service.work_set_inventory(check.generation, WorkSetInventoryKind::Parsed, None, 1),
        Err(CompilerServiceError::StaleGeneration { .. })
    ));
    assert_eq!(
        service
            .work_set_inventory(
                later.generation,
                WorkSetInventoryKind::ReusedParsed,
                None,
                8
            )
            .unwrap()
            .entries
            .len(),
        2
    );
}

#[test]
fn work_set_retention_has_a_combined_utf8_byte_cap() {
    let mut service = RegisteredProjectCompilerService::new(CompilerServiceLimits {
        max_retained_work_set_bytes: ENTRY.len(),
        ..CompilerServiceLimits::default()
    });
    let id = service
        .register(registration("export const value: number = answer;"))
        .unwrap();
    let check = service.check(id).unwrap();
    assert_eq!(check.parsed_modules.len(), 2);
    let page = service
        .work_set_inventory(check.generation, WorkSetInventoryKind::Parsed, None, 8)
        .unwrap();
    assert_eq!(page.entries, vec![ENTRY.to_string()]);
    assert!(page.truncated);
    assert!(page.next_cursor.is_none());
    let later_set = service
        .work_set_inventory(check.generation, WorkSetInventoryKind::Rechecked, None, 8)
        .unwrap();
    assert!(later_set.entries.is_empty());
    assert!(later_set.truncated);
}

#[test]
fn retained_contracts_and_provenance_are_exact_generation_bound() {
    let mut service = RegisteredProjectCompilerService::default();
    let id = service
        .register(registration(
            "interface Settings { enabled: boolean; } \
             export const settings: Settings = { enabled: true };",
        ))
        .unwrap();
    let check = service.check(id).unwrap();
    let info = check.static_debug_info.as_ref().unwrap();
    let contract = info
        .contracts
        .iter()
        .find(|contract| contract.name == "Settings")
        .expect("a non-generic local interface must be reifiable");
    let provenance = service
        .static_provenance(check.generation, contract.source)
        .unwrap();
    assert_eq!(provenance.id, contract.source);
    assert_ne!(provenance.content_hash, "Settings");
    assert!(service
        .validate_static_contract(
            check.generation,
            contract.id,
            &ContractValue::Object(BTreeMap::from([(
                "enabled".to_string(),
                ContractValue::Boolean(true),
            )])),
        )
        .unwrap()
        .is_ok());
    let invalid = service
        .validate_static_contract(
            check.generation,
            contract.id,
            &ContractValue::Object(BTreeMap::from([(
                "enabled".to_string(),
                ContractValue::String("not-a-boolean".to_string()),
            )])),
        )
        .unwrap()
        .unwrap_err();
    assert_eq!(invalid.path, "$.enabled");

    let later = service.check(id).unwrap();
    assert_ne!(check.generation, later.generation);
    assert!(matches!(
        service.static_contract(check.generation, contract.id),
        Err(CompilerServiceError::StaleGeneration { .. })
    ));
}

#[test]
fn static_metadata_inventory_is_opaque_one_shot_and_generation_bound() {
    let mut service = RegisteredProjectCompilerService::default();
    let id = service
        .register(registration(
            "interface Settings { enabled: boolean; } \
             export const settings: Settings = { enabled: true };",
        ))
        .unwrap();
    let check = service.check(id).unwrap();
    let expected_symbol_ids = check
        .static_debug_info
        .as_ref()
        .unwrap()
        .symbols
        .iter()
        .map(|symbol| symbol.id.0)
        .collect::<Vec<_>>();

    for (kind, expected_count) in [
        (
            StaticMetadataInventoryKind::Sources,
            check.static_debug_info.as_ref().unwrap().sources.len(),
        ),
        (
            StaticMetadataInventoryKind::Types,
            check.static_debug_info.as_ref().unwrap().types.len(),
        ),
        (
            StaticMetadataInventoryKind::Symbols,
            check.static_debug_info.as_ref().unwrap().symbols.len(),
        ),
        (
            StaticMetadataInventoryKind::Contracts,
            check.static_debug_info.as_ref().unwrap().contracts.len(),
        ),
    ] {
        let page = service
            .static_metadata_inventory(check.generation, kind, None, 128)
            .unwrap();
        assert_eq!(page.ids.len(), expected_count);
        assert!(page.next_cursor.is_none());
    }

    let first = service
        .static_metadata_inventory(
            check.generation,
            StaticMetadataInventoryKind::Symbols,
            None,
            1,
        )
        .unwrap();
    assert_eq!(first.ids.len(), 1);
    let cursor = first
        .next_cursor
        .expect("multiple symbols must require a continuation cursor");

    // A cursor cannot be retargeted to another collection, and a failed
    // retarget does not consume the valid symbols continuation.
    assert!(matches!(
        service.static_metadata_inventory(
            check.generation,
            StaticMetadataInventoryKind::Types,
            Some(cursor),
            1,
        ),
        Err(CompilerServiceError::InvalidStaticMetadataCursor { .. })
    ));
    let second = service
        .static_metadata_inventory(
            check.generation,
            StaticMetadataInventoryKind::Symbols,
            Some(cursor),
            1,
        )
        .unwrap();
    assert_eq!(second.ids.len(), 1);
    assert!(matches!(
        service.static_metadata_inventory(
            check.generation,
            StaticMetadataInventoryKind::Symbols,
            Some(cursor),
            1,
        ),
        Err(CompilerServiceError::InvalidStaticMetadataCursor { .. })
    ));

    let mut discovered = first.ids;
    discovered.extend(second.ids);
    let mut cursor = second.next_cursor;
    while let Some(next) = cursor {
        let page = service
            .static_metadata_inventory(
                check.generation,
                StaticMetadataInventoryKind::Symbols,
                Some(next),
                1,
            )
            .unwrap();
        discovered.extend(page.ids);
        cursor = page.next_cursor;
    }
    assert_eq!(discovered, expected_symbol_ids);

    let stale_cursor = service
        .static_metadata_inventory(
            check.generation,
            StaticMetadataInventoryKind::Symbols,
            None,
            1,
        )
        .unwrap()
        .next_cursor
        .expect("fixture keeps multiple symbol IDs");
    let later = service.check(id).unwrap();
    assert_ne!(check.generation, later.generation);
    assert!(matches!(
        service.static_metadata_inventory(
            check.generation,
            StaticMetadataInventoryKind::Symbols,
            Some(stale_cursor),
            1,
        ),
        Err(CompilerServiceError::StaleGeneration { .. })
    ));
    assert!(matches!(
        service.static_metadata_inventory(
            later.generation,
            StaticMetadataInventoryKind::Symbols,
            Some(stale_cursor),
            1,
        ),
        Err(CompilerServiceError::InvalidStaticMetadataCursor { .. })
    ));
}

#[test]
fn build_keeps_artifacts_in_memory_and_preserves_no_emit_on_error() {
    let mut service = RegisteredProjectCompilerService::default();
    let valid = service
        .register(registration("export const value: number = answer;"))
        .unwrap();
    let build = service.build(valid).unwrap();
    let output = build.output.expect("valid build must return artifacts");
    assert_eq!(output.artifacts.len(), 2);
    assert!(build.check.artifact_fingerprint.is_some());

    let invalid = service
        .register(RegisteredProjectRegistration {
            canonical_project_root: "project:///invalid".to_string(),
            canonical_config_root: "project:///invalid/blue-ts.json".to_string(),
            canonical_output_root: "project:///invalid-dist".to_string(),
            ..registration("export const value: number = 'wrong';")
        })
        .unwrap();
    let failed_build = service.build(invalid).unwrap();
    assert!(failed_build.check.has_errors);
    assert!(failed_build.output.is_none());
    assert!(failed_build.check.artifact_fingerprint.is_none());
}

#[test]
fn registration_and_build_response_limits_fail_closed() {
    let mut service = RegisteredProjectCompilerService::new(CompilerServiceLimits {
        max_projects: 1,
        max_build_artifact_bytes: 1,
        ..CompilerServiceLimits::default()
    });
    let id = service
        .register(registration("export const value: number = answer;"))
        .unwrap();
    assert!(matches!(
        service.register(RegisteredProjectRegistration {
            canonical_project_root: "project:///second".to_string(),
            canonical_config_root: "project:///second/blue-ts.json".to_string(),
            canonical_output_root: "project:///second-dist".to_string(),
            ..registration("export const value: number = answer;")
        }),
        Err(CompilerServiceError::ProjectLimit { limit: 1 })
    ));
    assert!(matches!(
        service.build(id),
        Err(CompilerServiceError::BuildOutputLimit {
            resource: "bytes",
            limit: 1,
        })
    ));
}

#[test]
fn registration_rejects_an_entry_outside_the_closed_module_graph() {
    let mut service = RegisteredProjectCompilerService::default();
    let mut registration = registration("export const value: number = answer;");
    registration.entry_module = "project:///app/unapproved.ts".to_string();
    assert!(matches!(
        service.register(registration),
        Err(CompilerServiceError::EntryModuleNotAuthorized { .. })
    ));
}
