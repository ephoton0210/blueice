// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public program selection, Native semantics, and real incremental generations.

use blueice_bluets::{
    compile, compile_program, AuthorizedModuleLoader, CheckingOptions, Compilation,
    CompilerOptions, DiagnosticCode, EcmaTarget, IncrementalCompiler, ModuleKind, ModuleLoader,
    ModuleSource,
};
use serde_json::{json, Value};
use std::cell::Cell;
use std::collections::BTreeMap;

const PREFIX: &str = "program:///";
mod program_roots_support;

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/program_roots/library-contract.json")).unwrap()
}

fn roots(value: &Value) -> Vec<String> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|name| format!("{PREFIX}{}", name.as_str().unwrap()))
        .collect()
}

fn loader(files: &Value) -> AuthorizedModuleLoader {
    program_roots_support::loader(files)
}

fn options(profile: &Value) -> CompilerOptions {
    CompilerOptions {
        target: EcmaTarget::parse(profile["compilerOptions"]["target"].as_str().unwrap()).unwrap(),
        module_kind: match profile["compilerOptions"]["module"].as_str().unwrap() {
            "CommonJS" => ModuleKind::CommonJs,
            "ESNext" => ModuleKind::Esm,
            other => panic!("unexpected authored module profile {other}"),
        },
        libraries: Some(vec![EcmaTarget::Es5]),
        checking: Some(CheckingOptions::default()),
        declaration: true,
        ..CompilerOptions::default()
    }
}

fn diagnostics(compilation: &Compilation) -> Value {
    let rows = compilation
        .diagnostics
        .iter()
        .map(|row| {
            let counterpart = row
                .typescript
                .as_ref()
                .expect("complete Native counterpart");
            let position = |position: Option<&blueice_bluets::TypeScriptPosition>| {
                position.map(|p| json!({"line":p.line,"column":p.column,"length":p.length}))
            };
            json!({
                "code":counterpart.code,"message":counterpart.message,
                "file":counterpart.span.module.strip_prefix(PREFIX).unwrap(),
                "position":position(counterpart.position.as_ref()),
                "relatedInformation":counterpart.related_information.iter().map(|related| json!({
                    "code":related.code,"message":related.message,
                    "file":related.span.module.strip_prefix(PREFIX).unwrap(),
                    "position":position(related.position.as_ref()),"relatedInformation":[]
                })).collect::<Vec<_>>()
            })
        })
        .collect::<Vec<_>>();
    json!(rows)
}

fn assert_native(id: &str, compilation: &Compilation, expected: &Value) {
    assert_eq!(!compilation.has_errors(), expected["accepts"], "{id}");
    assert_eq!(diagnostics(compilation), expected["diagnostics"], "{id}");
    let declarations = compilation
        .output
        .iter()
        .flat_map(|output| &output.artifacts)
        .filter_map(|(module, artifact)| {
            artifact.declaration.as_ref().map(|text| {
                let name = module
                    .strip_prefix(PREFIX)
                    .unwrap()
                    .strip_suffix(".ts")
                    .unwrap();
                (format!("{name}.d.ts"), text.clone())
            })
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(json!(declarations), expected["declarations"], "{id}");
    if compilation.has_errors() {
        assert!(
            compilation.output.is_none(),
            "{id}: rejected input emitted artifacts"
        );
    }
}

#[test]
fn selected_programs_match_complete_native_diagnostics_and_declarations() {
    let data = fixture();
    for row in data["primary"].as_array().unwrap() {
        let compilation = compile_program(
            &roots(&row["roots"]),
            &loader(&row["files"]),
            options(&row["profile"]),
        );
        assert_native(row["id"].as_str().unwrap(), &compilation, &row["expected"]);
    }
}

#[test]
fn actual_incremental_programs_match_native_and_fresh_generations() {
    let data = fixture();
    for sequence in data["lifecycle"].as_array().unwrap() {
        let mut compiler = IncrementalCompiler::new();
        for state in sequence["states"].as_array().unwrap() {
            if state["clean"] == true {
                compiler = IncrementalCompiler::new();
            }
            let roots = roots(&state["roots"]);
            let loader = loader(&state["files"]);
            let options = options(&sequence["profile"]);
            let cold = compile_program(&roots, &loader, options.clone());
            let warm = compiler.compile_program(&roots, &loader, options);
            let id = format!(
                "{}/{}",
                sequence["id"].as_str().unwrap(),
                state["id"].as_str().unwrap()
            );
            assert_native(&id, &warm.compilation, &state["expected"]);
            assert_eq!(
                warm.compilation.project_fingerprint, cold.project_fingerprint,
                "{id}"
            );
            assert_eq!(warm.compilation.diagnostics, cold.diagnostics, "{id}");
            assert_eq!(warm.compilation.output, cold.output, "{id}");
            assert_eq!(warm.compilation.debug_info, cold.debug_info, "{id}");
            if state["id"].as_str().unwrap().starts_with("warm") && !cold.has_errors() {
                assert!(
                    warm.cache_hit,
                    "{id}: unchanged accepted state must reuse its actual cache"
                );
                assert!(warm.parsed_modules.is_empty(), "{id}");
                assert!(warm.rechecked_modules.is_empty(), "{id}");
            }
            let current = warm
                .compilation
                .project
                .modules
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            let mut expected = state["expected"]["modules"]
                .as_array()
                .unwrap()
                .iter()
                .map(|name| format!("{PREFIX}{}", name.as_str().unwrap()))
                .collect::<Vec<_>>();
            expected.sort();
            assert_eq!(
                current, expected,
                "{id}: unselected or removed module entered this generation"
            );
        }
    }
}

struct RefuseLoader(Cell<usize>);
impl ModuleLoader for RefuseLoader {
    fn load(&self, _: &str) -> Result<ModuleSource, String> {
        self.0.set(self.0.get() + 1);
        Err("the root preflight must refuse before source loading".into())
    }
    fn resolve(&self, _: &str, _: &str) -> Result<String, String> {
        panic!("invalid program roots acquired resolver authority")
    }
}

#[test]
fn malformed_duplicate_and_over_budget_roots_refuse_before_loading() {
    for roots in [
        vec![],
        vec![String::new()],
        vec!["program:///bad\0.ts".into()],
        vec!["program:///main.ts".into(), "program:///main.ts".into()],
    ] {
        let loader = RefuseLoader(Cell::new(0));
        let result = compile_program(&roots, &loader, CompilerOptions::default());
        assert!(result.has_errors(), "{roots:?}");
        assert!(result.output.is_none());
        assert_eq!(loader.0.get(), 0, "{roots:?}");
    }
    let loader = RefuseLoader(Cell::new(0));
    let mut options = CompilerOptions::default();
    options.limits.max_modules = 1;
    let result = compile_program(
        &["program:///a.ts".into(), "program:///b.ts".into()],
        &loader,
        options,
    );
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.code == DiagnosticCode::ResourceLimit));
    assert!(result.output.is_none());
    assert_eq!(loader.0.get(), 0);
}

#[test]
fn single_entry_compatibility_and_ordered_root_identity_are_explicit() {
    let loader = loader(&json!({"a.ts":"export const a = 1;", "b.ts":"export const b = 2;"}));
    let options = CompilerOptions::default();
    let entry = format!("{PREFIX}a.ts");
    let legacy = compile(&entry, &loader, options.clone());
    let one = compile_program(std::slice::from_ref(&entry), &loader, options.clone());
    assert_eq!(legacy.project_fingerprint, one.project_fingerprint);
    assert_eq!(legacy.diagnostics, one.diagnostics);
    assert_eq!(legacy.output, one.output);
    let roots = vec![entry, format!("{PREFIX}b.ts")];
    let mut compiler = IncrementalCompiler::new();
    let original = compiler.compile_program(&roots, &loader, options.clone());
    let reverse = vec![roots[1].clone(), roots[0].clone()];
    let reordered = compiler.compile_program(&reverse, &loader, options.clone());
    assert!(!original.compilation.has_errors());
    assert!(!reordered.compilation.has_errors());
    assert!(!reordered.cache_hit);
    assert_ne!(
        original.compilation.project_fingerprint,
        reordered.compilation.project_fingerprint
    );
    assert!(
        compiler
            .compile_program(&reverse, &loader, options)
            .cache_hit
    );
}

#[test]
fn root_failures_preserve_the_last_successful_cache_and_grant_no_runtime() {
    let selected = vec![format!("{PREFIX}main.ts")];
    let loader = loader(
        &json!({"main.ts":"declare function acquireAuthority(): never; acquireAuthority();"}),
    );
    let mut compiler = IncrementalCompiler::new();
    let initial = compiler.compile_program(&selected, &loader, CompilerOptions::default());
    assert!(
        !initial.compilation.has_errors(),
        "{:?}",
        initial.compilation.diagnostics
    );
    assert!(initial.compilation.output.unwrap().artifacts[&selected[0]]
        .javascript
        .contains("acquireAuthority()"));
    let failed = compiler.compile_program(
        &[format!("{PREFIX}missing.ts")],
        &loader,
        CompilerOptions::default(),
    );
    assert!(failed.compilation.has_errors());
    assert!(failed.compilation.output.is_none());
    assert!(
        compiler
            .compile_program(&selected, &loader, CompilerOptions::default())
            .cache_hit
    );
}
