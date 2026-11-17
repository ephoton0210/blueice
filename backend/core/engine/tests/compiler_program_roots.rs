// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Owner-selected roots are immutable and every result belongs to one generation.

use blueice_bluets::{AuthorizedModule, AuthorizedModuleLoader, CompilerOptions};
use blueice_engine::compiler_service::{
    CompilerServiceLimits, RegisteredProjectCompilerService, RegisteredProjectRegistration,
    WorkSetInventoryKind,
};

const MAIN: &str = "project:///program/main.ts";
const CONSUMER: &str = "project:///program/consumer.ts";
const POISON: &str = "project:///program/unselected.ts";

fn registration() -> RegisteredProjectRegistration {
    RegisteredProjectRegistration {
        canonical_project_root: "project:///program".into(),
        canonical_config_root: "project:///program/tsconfig.json".into(),
        canonical_output_root: "project:///program-output".into(),
        entry_module: MAIN.into(),
        loader: AuthorizedModuleLoader::new(
            [
                AuthorizedModule::new(MAIN, "const value: number = 1;"),
                AuthorizedModule::new(CONSUMER, "const result: number = value;"),
                AuthorizedModule::new(
                    POISON,
                    "const value: string = 'outside the selected program';",
                ),
            ],
            [],
        )
        .unwrap(),
        compiler_options: CompilerOptions {
            declaration: true,
            ..CompilerOptions::default()
        },
    }
}

#[test]
fn roots_and_authorized_inputs_are_copied_into_an_immutable_registration() {
    let mut registration = registration();
    let mut roots = vec![MAIN.to_string(), CONSUMER.to_string()];
    let mut service = RegisteredProjectCompilerService::default();
    let project = service
        .register_program(registration.clone(), roots.clone())
        .unwrap();
    roots.push(POISON.into());
    registration.entry_module = POISON.into();
    registration.compiler_options.declaration = false;
    let build = service.build(project).unwrap();
    assert!(!build.check.has_errors, "{:?}", build.check.diagnostics);
    assert_eq!(service.identity(project).unwrap().entry_module, MAIN);
    let output = build.output.unwrap();
    assert_eq!(output.artifacts.len(), 2);
    assert_eq!(
        output.artifacts[MAIN].declaration.as_deref(),
        Some("declare const value: number;\n")
    );
    assert_eq!(
        output.artifacts[CONSUMER].declaration.as_deref(),
        Some("declare const result: number;\n")
    );
    assert!(!output.artifacts.contains_key(POISON));
    let info = service.static_debug_info(build.check.generation).unwrap();
    assert_eq!(info.sources.len(), 2);
    assert!(info.sources.iter().all(|source| source.module != POISON));
}

#[test]
fn invalid_root_selections_refuse_before_consuming_a_registration_or_id() {
    for roots in [
        vec![],
        vec![String::new()],
        vec![MAIN.into(), MAIN.into()],
        vec![MAIN.into(), "project:///program/not-authorized.ts".into()],
        vec![MAIN.into(), "project:///outside.ts".into()],
        vec![MAIN.into(), "project:///program/bad\0.ts".into()],
        vec![CONSUMER.into(), MAIN.into()],
    ] {
        let mut service = RegisteredProjectCompilerService::default();
        assert!(
            service
                .register_program(registration(), roots.clone())
                .is_err(),
            "{roots:?}"
        );
        assert_eq!(service.registered_project_ids().count(), 0);
        assert_eq!(service.register(registration()).unwrap().as_u64(), 1);
    }
    let mut registration = registration();
    registration.compiler_options.limits.max_modules = 1;
    let mut service = RegisteredProjectCompilerService::default();
    assert!(service
        .register_program(registration, vec![MAIN.into(), CONSUMER.into()])
        .is_err());
    assert_eq!(service.registered_project_ids().count(), 0);
}

#[test]
fn check_build_metadata_and_work_sets_bind_one_complete_program_generation() {
    let mut service = RegisteredProjectCompilerService::default();
    let project = service
        .register_program(registration(), vec![MAIN.into(), CONSUMER.into()])
        .unwrap();
    let first = service.check(project).unwrap();
    assert!(!first.has_errors);
    let cold = service
        .work_set_inventory(first.generation, WorkSetInventoryKind::Parsed, None, 10)
        .unwrap();
    assert_eq!(cold.entries, vec![CONSUMER.to_string(), MAIN.to_string()]);
    let build = service.build(project).unwrap();
    assert_eq!(first.generation.sequence(), 1);
    assert_eq!(build.check.generation.sequence(), 2);
    assert!(build.check.cache_hit);
    assert_eq!(first.project_fingerprint, build.check.project_fingerprint);
    assert_eq!(
        build.output.as_ref().unwrap().fingerprint,
        build.check.project_fingerprint
    );
    assert_eq!(
        build.check.artifact_fingerprint.as_deref(),
        Some(build.check.project_fingerprint.as_str())
    );
    assert!(service.static_debug_info(first.generation).is_err());
    assert!(service
        .diagnostic_inventory(first.generation, None, 1)
        .is_err());
    assert!(service
        .work_set_inventory(first.generation, WorkSetInventoryKind::Parsed, None, 1)
        .is_err());
    let warm = service
        .work_set_inventory(
            build.check.generation,
            WorkSetInventoryKind::ReusedChecked,
            None,
            10,
        )
        .unwrap();
    assert_eq!(warm.entries, cold.entries);
    let parsed = service
        .work_set_inventory(
            build.check.generation,
            WorkSetInventoryKind::Parsed,
            None,
            10,
        )
        .unwrap();
    assert!(parsed.entries.is_empty());
}

#[test]
fn a_multi_root_program_obeys_shared_compiler_and_output_budgets() {
    let mut service = RegisteredProjectCompilerService::new(CompilerServiceLimits {
        max_build_artifacts: 1,
        ..CompilerServiceLimits::default()
    });
    let project = service
        .register_program(registration(), vec![MAIN.into(), CONSUMER.into()])
        .unwrap();
    assert!(service.build(project).is_err());
    let mut registration = registration();
    registration.compiler_options.limits.max_total_source_bytes = 1;
    let mut service = RegisteredProjectCompilerService::default();
    let project = service
        .register_program(registration, vec![MAIN.into(), CONSUMER.into()])
        .unwrap();
    let build = service.build(project).unwrap();
    assert!(build.check.has_errors);
    assert!(build.output.is_none());
    assert!(build.check.static_debug_info.is_none());
}
