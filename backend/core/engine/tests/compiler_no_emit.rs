// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Owner-selected static compilation publishes metadata without output authority.

use blueice_bluets::{
    AuthorizedModule, AuthorizedModuleLoader, AuthorizedModuleResolution, CompilerOptions,
};
use blueice_engine::compiler_service::{
    RegisteredProjectCompilerService, RegisteredProjectRegistration, WorkSetInventoryKind,
};

#[test]
fn static_owner_programs_retain_generation_metadata_without_executable_artifacts() {
    let main = "project:///static/main.ts";
    let consumer = "project:///static/consumer.ts";
    let types = "project:///static/types.d.ts";
    let poison = "project:///static/unselected.ts";
    let mut service = RegisteredProjectCompilerService::default();
    let project = service
        .register_program(
            RegisteredProjectRegistration {
                canonical_project_root: "project:///static".into(),
                canonical_config_root: "project:///static/tsconfig.json".into(),
                canonical_output_root: "project:///static-output".into(),
                entry_module: main.into(),
                loader: AuthorizedModuleLoader::new(
                    [
                        AuthorizedModule::new(main, "import './types';"),
                        AuthorizedModule::new(consumer, "const item: Item = { value: 1 };"),
                        AuthorizedModule::new(
                            types,
                            "export {}; declare global { interface Item { value: number; } }",
                        ),
                        AuthorizedModule::new(poison, "interface Item { value: string; }"),
                    ],
                    [AuthorizedModuleResolution::new(main, "./types", types)],
                )
                .unwrap(),
                compiler_options: CompilerOptions {
                    no_emit: true,
                    declaration: true,
                    declaration_map: true,
                    source_map: true,
                    ..CompilerOptions::default()
                },
            },
            vec![main.into(), consumer.into()],
        )
        .unwrap();
    let checked = service.check(project).unwrap();
    assert!(!checked.has_errors, "{:?}", checked.diagnostics);
    assert!(checked.artifact_fingerprint.is_none());
    let metadata = service.static_debug_info(checked.generation).unwrap();
    assert_eq!(metadata.sources.len(), 3);
    assert!(metadata
        .sources
        .iter()
        .all(|source| source.module != poison));
    let built = service.build(project).unwrap();
    assert!(!built.check.has_errors);
    assert!(built.check.cache_hit);
    assert!(built.output.is_none());
    assert!(built.check.artifact_fingerprint.is_none());
    assert_eq!(built.check.project_fingerprint, checked.project_fingerprint);
    assert!(service.static_debug_info(checked.generation).is_err());
    assert_eq!(
        service
            .static_debug_info(built.check.generation)
            .unwrap()
            .sources
            .len(),
        3
    );
    let reused = service
        .work_set_inventory(
            built.check.generation,
            WorkSetInventoryKind::ReusedChecked,
            None,
            10,
        )
        .unwrap();
    assert_eq!(reused.entries.len(), 3);
}
