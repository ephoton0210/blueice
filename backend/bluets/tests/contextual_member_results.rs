// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluets::{
    compile, compile_program, CompilerOptions, DiagnosticCode, MapLoader, ModuleLoader,
    ModuleSource,
};
use std::{cell::RefCell, process::Command};

const MAIN: &str = "memory:///main.ts";

fn source() -> String {
    let input: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/contextual_member_results/cases.json"
    ))
    .unwrap();
    input["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "member-variable")
        .unwrap()["source"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn contextual_member_results_keep_the_type_expansion_limit() {
    let loader = MapLoader::from([ModuleSource::new(MAIN, source())]);
    let mut options = CompilerOptions::default();
    options.limits.max_type_expansions = 0;
    let result = compile(MAIN, &loader, options);
    assert!(result.has_errors());
    assert!(result.output.is_none());
    assert!(result
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == DiagnosticCode::ResourceLimit));
}

#[test]
fn contextual_member_results_keep_the_total_source_budget() {
    let source = source();
    let mut options = CompilerOptions::default();
    options.limits.max_total_source_bytes = source.len() - 1;
    let loader = MapLoader::from([ModuleSource::new(MAIN, source)]);
    let result = compile(MAIN, &loader, options);
    assert!(result.output.is_none());
    assert!(result
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == DiagnosticCode::ResourceLimit));
}

#[test]
fn contextual_member_results_preserve_static_no_emit_authority() {
    let loader = MapLoader::from([ModuleSource::new(MAIN, source())]);
    let result = compile(
        MAIN,
        &loader,
        CompilerOptions {
            no_emit: true,
            ..CompilerOptions::default()
        },
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    assert!(result.output.is_none());
}

#[test]
fn contextual_member_results_do_not_load_an_unselected_owner_source() {
    struct ObservedLoader {
        loader: MapLoader,
        loads: RefCell<Vec<String>>,
    }
    impl ModuleLoader for ObservedLoader {
        fn load(&self, id: &str) -> Result<ModuleSource, String> {
            self.loads.borrow_mut().push(id.to_owned());
            self.loader.load(id)
        }
    }
    let outside = "memory:///outside.ts";
    let loader = ObservedLoader {
        loader: MapLoader::from([
            ModuleSource::new(MAIN, source()),
            ModuleSource::new(outside, "declare const factory: { make<T>(): string };\n"),
        ]),
        loads: RefCell::new(Vec::new()),
    };
    let result = compile_program(&[MAIN.to_owned()], &loader, CompilerOptions::default());
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    assert!(!loader.loads.borrow().iter().any(|id| id == outside));
    assert!(!result.project.modules.contains_key(outside));
}

#[test]
fn constrained_assertion_budget_exhaustion_refuses_without_artifacts() {
    let source = "export function make<T extends string>(): T { return undefined as T; }\n";
    for limit in [0, 1, 2, 64] {
        let mut options = CompilerOptions::default();
        options.limits.max_type_expansions = limit;
        let result = compile(
            MAIN,
            &MapLoader::from([ModuleSource::new(MAIN, source)]),
            options,
        );
        assert!(
            result.has_errors(),
            "budget {limit} must not mask an invalid cast"
        );
        assert!(result.output.is_none());
        assert!(result.diagnostics.iter().any(|diagnostic| {
            if limit < 2 {
                diagnostic.code == DiagnosticCode::ResourceLimit
            } else {
                diagnostic
                    .typescript
                    .as_ref()
                    .is_some_and(|row| row.code == 2352)
            }
        }));
    }
}

#[test]
fn constrained_assertions_preserve_widened_literal_and_union_overlap() {
    let compiler = std::env::var_os("BLUEICE_CONTEXTUAL_MEMBER_COMPILER")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_bluetsc").into());
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools/verify_assertion_overlap.cjs");
    let output = Command::new("node")
        .arg(script)
        .arg(compiler)
        .arg(include_str!(
            "fixtures/contextual_member_results/ASSERTION_OVERLAP_FROZEN.json"
        ))
        .arg(include_str!(
            "fixtures/contextual_member_results/ASSERTION_ALIAS_FROZEN.json"
        ))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "requires pinned TypeScript and Acorn and the complete frozen contextual-member matrix"]
fn contextual_member_results_match_the_complete_native_contract() {
    let compiler = std::env::var_os("BLUEICE_CONTEXTUAL_MEMBER_COMPILER")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_bluetsc").into());
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../development/browser_core/phase-18-bluets/tools/verify_contextual_member_results.cjs",
    );
    let output = Command::new("node")
        .arg(script)
        .arg(compiler)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
