// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Static declarations retain the owner's runtime and source-work boundaries.

use blueice_bluets::{
    compile, AuthorizedModule, AuthorizedModuleLoader, AuthorizedModuleResolution, CompilerOptions,
    DiagnosticCode, MapLoader, ModuleSource, RuntimePolicy,
};

const MAIN: &str = "memory:///main.ts";
const GLOBALS: &str = "memory:///globals.d.ts";
const SOURCE: &str =
    "/// <reference path=\"./globals.d.ts\" />\nexport const answer: GlobalItem = { value: 42 };";
const DECLARATION: &str = "interface GlobalItem { value: number; }";

fn path_loader(authorized: bool) -> AuthorizedModuleLoader {
    AuthorizedModuleLoader::new(
        [
            AuthorizedModule::new(MAIN, SOURCE),
            AuthorizedModule::new(GLOBALS, DECLARATION),
        ],
        authorized.then(|| AuthorizedModuleResolution::new(MAIN, "./globals.d.ts", GLOBALS)),
    )
    .unwrap()
}

#[test]
fn reference_path_needs_an_exact_owner_edge_even_when_its_source_is_present() {
    let refused = compile(MAIN, &path_loader(false), CompilerOptions::default());
    assert!(refused.has_errors());
    assert!(refused.output.is_none());
    assert!(!refused.project.modules.contains_key(GLOBALS));
    let allowed = compile(MAIN, &path_loader(true), CompilerOptions::default());
    assert!(!allowed.has_errors(), "{:?}", allowed.diagnostics);
    assert_eq!(allowed.project.source(GLOBALS), Some(DECLARATION));
    assert_eq!(
        allowed.project.resolved_module(MAIN, "./globals.d.ts"),
        None
    );
}

#[test]
fn references_obey_module_edge_depth_and_total_source_limits() {
    assert!(!compile(MAIN, &path_loader(true), CompilerOptions::default()).has_errors());
    for boundary in ["modules", "edges", "depth", "bytes"] {
        let mut options = CompilerOptions::default();
        match boundary {
            "modules" => options.limits.max_modules = 1,
            "edges" => options.limits.max_module_edges = 0,
            "depth" => options.limits.max_module_depth = 0,
            "bytes" => options.limits.max_total_source_bytes = SOURCE.len() + DECLARATION.len() - 1,
            _ => unreachable!(),
        }
        let result = compile(MAIN, &path_loader(true), options);
        assert!(result.has_errors(), "reference bypassed {boundary} limit");
        assert!(result.output.is_none());
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == DiagnosticCode::ResourceLimit),
            "{:?}",
            result.diagnostics
        );
    }
}

fn named(source: &str, policy: RuntimePolicy) -> blueice_bluets::Compilation {
    compile(
        MAIN,
        &MapLoader::from([ModuleSource::new(MAIN, source)]),
        CompilerOptions {
            runtime_policy: policy,
            ambient_declaration_modules: vec![ModuleSource::new(
                "memory:///named.d.ts",
                "declare module 'owned' { export const answer: number; }",
            )],
            ..CompilerOptions::default()
        },
    )
}

#[test]
fn ambient_value_type_queries_supply_no_runtime_resolution() {
    let result = named(
        "import type { answer } from 'owned'; export const result: typeof answer = 42;",
        RuntimePolicy::Checked,
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    assert_eq!(result.project.resolved_module(MAIN, "owned"), None);
    assert_eq!(
        result
            .project
            .resolved_module_with_mode(MAIN, "owned", None),
        None
    );
    assert!(!result.project.modules.keys().any(|id| id.contains('\0')));
    let output = result.output.unwrap();
    assert!(!output.artifacts[MAIN].javascript.contains("owned"));
}

#[test]
fn ambient_values_cannot_authorize_executable_imports_or_reexports() {
    for policy in [RuntimePolicy::Checked, RuntimePolicy::TranspileOnly] {
        for source in [
            "import { answer } from 'owned'; export const result = answer;",
            "export { answer } from 'owned';",
            "export * from 'owned';",
        ] {
            let result = named(source, policy);
            assert!(
                result.has_errors(),
                "declaration authorized {source} under {policy:?}"
            );
            assert!(result.output.is_none());
            assert!(
                result
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.code == DiagnosticCode::InvalidDeclarationFile),
                "{:?}",
                result.diagnostics
            );
            assert_eq!(result.project.resolved_module(MAIN, "owned"), None);
        }
    }
}
