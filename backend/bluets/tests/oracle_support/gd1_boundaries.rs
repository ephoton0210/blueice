// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Declaration owner controls are distinct from native language agreement.
use super::*;
use blueice_bluets::{
    compile, AuthorizedModule, AuthorizedModuleLoader, CheckingOptions, CompilerOptions,
    DiagnosticCode, EcmaTarget, IncrementalCompiler, MapLoader, ModuleSource,
};

fn policy() -> CompilerOptions {
    CompilerOptions {
        target: EcmaTarget::Es2022,
        declaration: true,
        checking: Some(CheckingOptions::default()),
        ..CompilerOptions::default()
    }
}
fn sources(base: &str) -> MapLoader {
    MapLoader::from([
        ModuleSource::new("memory:///base.ts",base),
        ModuleSource::new("memory:///main.ts","import {sample} from './base.ts'; export const value = sample; export const result:number = value.value;")
    ])
}

#[test]
fn charged_declaration_work() {
    let loader = sources(
        "export interface Shape<T> {value:T;} export const sample:Shape<number> = {value:42};",
    );
    let normal = compile("memory:///main.ts", &loader, policy());
    assert!(normal.diagnostics.is_empty(), "{:?}", normal.diagnostics);
    for limit in ["expansion", "depth", "tokens", "bytes"] {
        let mut options = policy();
        match limit {
            "expansion" => options.limits.max_type_expansions = 0,
            "depth" => options.limits.parser.max_type_depth = 0,
            "tokens" => options.limits.parser.max_tokens = 10,
            "bytes" => options.limits.max_total_source_bytes = 10,
            _ => unreachable!(),
        }
        let result = compile("memory:///main.ts", &loader, options);
        assert!(result.output.is_none(), "{limit}");
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::ResourceLimit),
            "{limit}: {:?}",
            result.diagnostics
        );
    }
}

#[test]
fn declarations_do_not_authorize_runtime_values() {
    let valid = MapLoader::from([
        ModuleSource::new(
            "memory:///types.d.ts",
            "export interface Shape {value:number;} export declare const hidden:number;",
        ),
        ModuleSource::new(
            "memory:///main.ts",
            "import type {Shape} from './types.d.ts'; export const value:Shape = {value:42};",
        ),
    ]);
    let result = compile("memory:///main.ts", &valid, policy());
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert!(!result.output.unwrap().artifacts["memory:///main.ts"]
        .javascript
        .contains("./types.d.ts"));
    let invalid = MapLoader::from([
        ModuleSource::new(
            "memory:///types.d.ts",
            "export declare const hidden:number;",
        ),
        ModuleSource::new(
            "memory:///main.ts",
            "import {hidden} from './types.d.ts'; export const value = hidden;",
        ),
    ]);
    let result = compile("memory:///main.ts", &invalid, policy());
    assert!(result.output.is_none());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::InvalidDeclarationFile),
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn repeat_build_symbol_and_import_stability() {
    let base="export namespace Types { export interface Shape {value:number;} } export const sample:Types.Shape = {value:42};";
    let mut compiler = IncrementalCompiler::new();
    let first = compiler.compile("memory:///main.ts", &sources(base), policy());
    assert!(
        first.compilation.diagnostics.is_empty(),
        "{:?}",
        first.compilation.diagnostics
    );
    let original = first.compilation.output.unwrap();
    let symbols = first.compilation.checked.unwrap().modules["memory:///main.ts"]
        .symbols
        .clone();
    let second = compiler.compile("memory:///main.ts", &sources(base), policy());
    assert!(second.cache_hit);
    assert_eq!(second.compilation.output.as_ref(), Some(&original));
    assert_eq!(
        second.compilation.checked.unwrap().modules["memory:///main.ts"].symbols,
        symbols
    );
    let edited = format!("{base}\n// source-only change preserves nominal symbol ownership\n");
    let next = compiler.compile("memory:///main.ts", &sources(&edited), policy());
    let fresh = compile("memory:///main.ts", &sources(&edited), policy());
    assert!(!next.cache_hit);
    assert_eq!(next.compilation.diagnostics, fresh.diagnostics);
    assert_eq!(next.compilation.output, fresh.output);
    assert_eq!(
        next.compilation.checked.unwrap().modules["memory:///main.ts"].symbols,
        symbols
    );
    let output = next.compilation.output.unwrap();
    assert_eq!(
        output.artifacts["memory:///main.ts"].declaration,
        original.artifacts["memory:///main.ts"].declaration
    );
    let temporary = Temporary::new("repeat");
    for row in input()["cases"].as_array().unwrap().iter().filter(|r| {
        r["accepts"] == true
            && matches!(
                r["form"].as_str(),
                Some(
                    "overload-imported-alias"
                        | "fresh-reexported-alias"
                        | "external-public-namespace"
                        | "merged-class-interface"
                )
            )
    }) {
        let root = temporary.project(row);
        let first = command(&root);
        assert!(
            first.status.success(),
            "{}: {}",
            row["id"],
            String::from_utf8_lossy(&first.stderr)
        );
        let before = outputs(&root);
        let second = command(&root);
        assert!(second.status.success());
        assert_eq!(second.stdout, first.stdout);
        assert_eq!(outputs(&root), before);
    }
}

#[test]
fn failed_declarations_keep_publication_atomic() {
    let temporary = Temporary::new("atomic");
    let input = input();
    for row in input["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["declarationError"] == true && r["runtimeModule"] == "CommonJS")
    {
        let root = temporary.project(row);
        fs::write(root.join("main.ts"), "export const result:number = 42;").unwrap();
        let valid = command(&root);
        assert!(
            valid.status.success(),
            "{}",
            String::from_utf8_lossy(&valid.stderr)
        );
        let before = outputs(&root);
        assert!(!before.is_empty());
        fs::write(
            root.join("main.ts"),
            row["files"]["main.ts"].as_str().unwrap(),
        )
        .unwrap();
        let failed = command(&root);
        assert!(!failed.status.success(), "{}", row["id"]);
        assert_eq!(outputs(&root), before, "{}", row["id"]);
    }
}

#[test]
fn canonical_source_confinement() {
    for specifier in ["../outside.ts", "https://foreign.invalid/private.ts"] {
        let loader = AuthorizedModuleLoader::new(
            [
                AuthorizedModule::new(
                    "owned:///main.ts",
                    format!("import {{sample}} from '{specifier}'; export const value = sample;"),
                ),
                AuthorizedModule::new("owned:///ungranted.ts", "export const sample = {value:42};"),
            ],
            [],
        )
        .unwrap();
        let result = compile("owned:///main.ts", &loader, policy());
        assert!(result.output.is_none());
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::ModuleNotFound),
            "{:?}",
            result.diagnostics
        );
        assert!(!result.project.modules.contains_key("owned:///ungranted.ts"));
    }
}
