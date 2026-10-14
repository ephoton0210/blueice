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

#[test]
fn known_unavailable_library_selectors_keep_a_precise_frontend_refusal() {
    for name in ["dom", "es5"] {
        let source = format!("/// <reference lib=\"{name}\" />\nexport const answer: number = 42;");
        let result = compile(
            MAIN,
            &MapLoader::from([ModuleSource::new(MAIN, source)]),
            CompilerOptions::default(),
        );
        assert!(result.has_errors());
        assert!(result.output.is_none());
        assert!(
            result.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == DiagnosticCode::UnsupportedSyntax
                    && diagnostic.typescript.is_none()
                    && diagnostic.message.contains(name)
            }),
            "a known pinned library is unavailable, rather than absent: {:?}",
            result.diagnostics
        );
    }
}

#[test]
fn an_identical_owner_declaration_referenced_by_path_is_collected_once() {
    let options = |source: &str| CompilerOptions {
        ambient_declaration_modules: vec![ModuleSource::new(GLOBALS, source)],
        ..CompilerOptions::default()
    };
    let loader = MapLoader::from([
        ModuleSource::new(MAIN, SOURCE),
        ModuleSource::new(GLOBALS, DECLARATION),
    ]);
    let result = compile(MAIN, &loader, options(DECLARATION));
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    assert_eq!(result.project.source(GLOBALS), Some(DECLARATION));
    assert_eq!(result.project.resolved_module(MAIN, "./globals.d.ts"), None);
    let different = compile(
        MAIN,
        &loader,
        options("interface GlobalItem { value: string; }"),
    );
    assert!(different.has_errors());
    assert!(different.output.is_none());
    assert!(different
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == DiagnosticCode::InvalidDeclarationFile));
    let imported = MapLoader::from([
        ModuleSource::new(
            MAIN,
            "import type {} from './globals.d.ts'; export const answer: number = 42;",
        ),
        ModuleSource::new(GLOBALS, DECLARATION),
    ]);
    let duplicate = compile(MAIN, &imported, options(DECLARATION));
    assert!(duplicate.has_errors());
    assert!(duplicate.output.is_none());
    assert!(duplicate
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == DiagnosticCode::InvalidDeclarationFile));
}

#[test]
fn project_roots_and_path_references_share_the_same_owned_declaration() {
    let root = std::env::temp_dir().join(format!(
        "bluets-ambient-reference-roots-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("main.ts"), SOURCE).unwrap();
    std::fs::write(root.join("globals.d.ts"), DECLARATION).unwrap();
    let config = root.join("tsconfig.json");
    std::fs::write(&config, serde_json::json!({
        "compilerOptions": {"target":"ES2022","module":"ES2022","moduleResolution":"node10","strict":true},
        "files":["main.ts","globals.d.ts"]
    }).to_string()).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .arg("--project")
        .arg(config)
        .args(["--noEmit", "--diagnostics-json"])
        .output()
        .unwrap();
    std::fs::remove_dir_all(&root).unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
