// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Owner controls are separate from G-T10's native comparison denominator.
use super::*;
use blueice_bluets::{
    AuthorizedModule, AuthorizedModuleLoader, AuthorizedModuleResolution, IncrementalCompiler,
};

const ENTRY: &str =
    "export type Value = import('./dep.js').Item; export const value: Value = {n: 1};";

fn sources(dependency: Option<&str>) -> FixtureLoader {
    let mut files = BTreeMap::from([("entry.ts".into(), ENTRY.into())]);
    if let Some(source) = dependency {
        files.insert("dep.ts".into(), source.into());
    }
    FixtureLoader(files)
}

#[test]
fn canonical_owner_edges_never_fall_back_to_paths_or_network() {
    for specifier in ["../outside.ts", "https://foreign.invalid/types.d.ts"] {
        let loader = AuthorizedModuleLoader::new(
            [
                AuthorizedModule::new(
                    "owned:///entry.ts",
                    format!("export type Value = import('{specifier}').Item;"),
                ),
                AuthorizedModule::new(
                    "owned:///ungranted.d.ts",
                    "export interface Item { n: number; }",
                ),
            ],
            [],
        )
        .unwrap();
        let result = compile("owned:///entry.ts", &loader, options(&json!({})));
        assert!(result.output.is_none());
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::ModuleNotFound),
            "{:?}",
            result.diagnostics
        );
        assert!(!result
            .project
            .modules
            .contains_key("owned:///ungranted.d.ts"));
    }
    let loader = AuthorizedModuleLoader::new(
        [
            AuthorizedModule::new(
                "owned:///entry.ts",
                "export type Value = import('schema').Item; export const value: Value = {n: 1};",
            ),
            AuthorizedModule::new(
                "owned:///canonical/schema.d.ts",
                "export interface Item { n: number; }",
            ),
        ],
        [AuthorizedModuleResolution::new(
            "owned:///entry.ts",
            "schema",
            "owned:///canonical/schema.d.ts",
        )],
    )
    .unwrap();
    let result = compile("owned:///entry.ts", &loader, options(&json!({})));
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert_eq!(
        result
            .project
            .resolved_module("owned:///entry.ts", "schema"),
        Some("owned:///canonical/schema.d.ts")
    );
    assert!(result
        .output
        .unwrap()
        .artifacts
        .keys()
        .all(|id| !id.ends_with(".d.ts")));
}

#[test]
fn declaration_queries_never_supply_executable_values() {
    for checking in [None, Some(CheckingOptions::default())] {
        let mut policy = options(&json!({}));
        policy.checking = checking;
        let mut files = BTreeMap::from([(
            "dep.d.ts".into(),
            "export declare const hidden: number;".into(),
        )]);
        files.insert(
            "entry.ts".into(),
            "export type Value = typeof import('./dep.js').hidden; export const value: Value = 1;"
                .into(),
        );
        let result = compile(
            "memory:///entry.ts",
            &FixtureLoader(files.clone()),
            policy.clone(),
        );
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert!(!result.output.unwrap().artifacts["memory:///entry.ts"]
            .javascript
            .contains("./dep.js"));
        files.insert(
            "entry.ts".into(),
            "import {hidden} from './dep.js'; export const value = hidden;".into(),
        );
        let result = compile("memory:///entry.ts", &FixtureLoader(files), policy);
        assert!(result.output.is_none());
        assert!(result
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::InvalidDeclarationFile));
    }
}

#[test]
fn type_edges_share_existing_module_edge_depth_byte_and_expansion_limits() {
    let loader = sources(Some("export interface Item { n: number; }"));
    let normal = compile("memory:///entry.ts", &loader, options(&json!({})));
    assert!(normal.diagnostics.is_empty(), "{:?}", normal.diagnostics);
    for limit in ["module", "edge", "depth", "bytes", "expansion"] {
        let mut policy = options(&json!({}));
        match limit {
            "module" => policy.limits.max_modules = 1,
            "edge" => policy.limits.max_module_edges = 0,
            "depth" => policy.limits.max_module_depth = 0,
            "bytes" => policy.limits.max_total_source_bytes = ENTRY.len(),
            "expansion" => policy.limits.max_type_expansions = 0,
            _ => unreachable!(),
        }
        let result = compile("memory:///entry.ts", &loader, policy);
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
fn incremental_type_dependencies_recheck_changed_missing_and_restored_sources() {
    let initial = sources(Some("export interface Item { n: number; }"));
    let mut compiler = IncrementalCompiler::new();
    let policy = options(&json!({}));
    let first = compiler.compile("memory:///entry.ts", &initial, policy.clone());
    assert!(
        first.compilation.diagnostics.is_empty(),
        "{:?}",
        first.compilation.diagnostics
    );
    let fingerprint = first.compilation.output.unwrap().fingerprint;
    for dependency in [
        Some("export interface Item { n: string; }"),
        None,
        Some("export interface Item { n: number; }"),
    ] {
        let loader = sources(dependency);
        let fresh = compile("memory:///entry.ts", &loader, policy.clone());
        let next = compiler.compile("memory:///entry.ts", &loader, policy.clone());
        assert_eq!(next.compilation.diagnostics, fresh.diagnostics);
        assert_eq!(next.compilation.output, fresh.output);
        if dependency.is_some_and(|text| text.contains("string")) {
            assert!(!next.cache_hit);
            assert!(next.rechecked_modules.contains("memory:///entry.ts"));
            assert!(next.compilation.output.is_none());
            assert!(next
                .compilation
                .diagnostics
                .iter()
                .any(|d| d.typescript.as_ref().is_some_and(|t| t.code == 2322)));
        } else if dependency.is_none() {
            assert!(!next.cache_hit);
            assert!(next.compilation.output.is_none());
            assert!(next
                .compilation
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::ModuleNotFound));
        } else {
            // Errors leave the last successful cache intact. The restored
            // byte-identical graph can reuse it after loading/verifying inputs.
            assert!(next.cache_hit);
            assert_eq!(next.compilation.output.unwrap().fingerprint, fingerprint);
        }
    }
}

#[test]
fn a_type_dependency_is_loaded_and_fingerprinted_but_not_executed_by_import() {
    let loader = sources(Some(
        "console.log('must-not-run'); export interface Item { n: number; }",
    ));
    for module in ["ESNext", "CommonJS"] {
        let result = compile(
            "memory:///entry.ts",
            &loader,
            options(&json!({"options":{"module":module}})),
        );
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert!(result.project.modules.contains_key("memory:///dep.ts"));
        let output = result.output.unwrap();
        assert!(output.artifacts["memory:///dep.ts"]
            .javascript
            .contains("must-not-run"));
        assert!(!output.artifacts["memory:///entry.ts"]
            .javascript
            .contains("./dep.js"));
    }
}
