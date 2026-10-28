// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Explicit owner boundaries are separate from native agreements.
use super::*;
use blueice_bluets::{
    compile, CheckingOptions, CompilerOptions, DiagnosticCode, EcmaTarget, MapLoader, ModuleSource,
};

fn row(form: &str) -> Value {
    input()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| {
            row["form"] == form
                && row["accepts"] == true
                && row["runtimeModule"] == "CommonJS"
                && row["nativeRuntimeDefect"] == false
        })
        .unwrap()
        .clone()
}
fn policy() -> CompilerOptions {
    CompilerOptions {
        target: EcmaTarget::Es2022,
        checking: Some(CheckingOptions::default()),
        declaration: true,
        ..CompilerOptions::default()
    }
}

#[test]
fn class_type_work_is_charged() {
    let source = row("merged-generic-parent")["files"]["main.ts"]
        .as_str()
        .unwrap()
        .to_owned();
    let loader = MapLoader::from([ModuleSource::new("memory:///main.ts", &source)]);
    let result = compile("memory:///main.ts", &loader, policy());
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
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
                .any(|diagnostic| diagnostic.code == DiagnosticCode::ResourceLimit),
            "{limit}: {:?}",
            result.diagnostics
        );
    }
}

#[test]
fn constructor_types_do_not_install_runtime_globals() {
    let loader = MapLoader::from([ModuleSource::new(
        "memory:///main.ts",
        "interface Host { new(): { value:number }; } export const result = new Host().value;",
    )]);
    let result = compile("memory:///main.ts", &loader, policy());
    assert!(result.output.is_none());
    assert!(
        result.diagnostics.iter().any(|diagnostic| diagnostic
            .typescript
            .as_ref()
            .is_some_and(|counterpart| counterpart.code == 2693)),
        "{:?}",
        result.diagnostics
    );
    let loader=MapLoader::from([
        ModuleSource::new("memory:///base.ts","export class Base { value:number=1; } export class Other { other:string='value'; } export function choose(Base:typeof Other):typeof Base { return Base; }"),
        ModuleSource::new("memory:///main.ts","import {choose,Other} from './base.ts'; export class Box extends choose(Other) {} export const result:string = new Box().other;")
    ]);
    let result = compile("memory:///main.ts", &loader, policy());
    assert!(
        result.diagnostics.is_empty(),
        "lexical constructor query: {:?}",
        result.diagnostics
    );
}

#[test]
fn stateful_class_expression_effects_are_per_evaluation() {
    let temporary = Temporary::new("effects");
    for form in [
        "await-key-private",
        "await-key-static",
        "yield-key-private",
        "yield-key-static",
    ] {
        let case = row(form);
        let root = temporary.project(&case);
        let result = command(&root);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let file = root.join("out/main.js");
        let script=format!("Promise.resolve().then(()=>require({})).then(async m=>process.stdout.write(JSON.stringify(await m.result)));",serde_json::to_string(&file.to_string_lossy()).unwrap());
        for _ in 0..2 {
            let run = Command::new("node").args(["-e", &script]).output().unwrap();
            assert!(
                run.status.success(),
                "{}",
                String::from_utf8_lossy(&run.stderr)
            );
            assert_eq!(run.stdout, b"422");
        }
    }
    let mut case = row("heritage-call");
    case["files"]["main.ts"]=json!("let calls = 0; const Box_base = 10; class Base { value:number = 21; } function choose():typeof Base { calls++; return Base; } export class Box extends choose() {} export const result:number = new Box().value + new Box().value + calls + Box_base;");
    let root = temporary.project(&case);
    let result = command(&root);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let script = format!(
        "process.stdout.write(JSON.stringify(require({}).result));",
        serde_json::to_string(&root.join("out/main.js").to_string_lossy()).unwrap()
    );
    let run = Command::new("node").args(["-e", &script]).output().unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(run.stdout, b"53");
}

#[test]
fn getter_and_member_errors_retain_source_origins() {
    let source="export class Box { get value() { return absent; } } export const result = new Box().value;";
    let loader = MapLoader::from([ModuleSource::new("memory:///main.ts", source)]);
    let result = compile("memory:///main.ts", &loader, policy());
    assert!(result.output.is_none());
    let diagnostic = result
        .diagnostics
        .iter()
        .filter_map(|d| d.typescript.as_ref())
        .find(|d| d.code == 2304)
        .unwrap_or_else(|| panic!("{:?}", result.diagnostics));
    assert_eq!(
        &source[diagnostic.span.start..diagnostic.span.end],
        "absent"
    );
    assert!(!result
        .diagnostics
        .iter()
        .any(|d| d.message.contains("cannot infer getter")));
}

#[test]
fn native_branch_super_runtime_defect_is_refused() {
    let temporary = Temporary::new("unsafe");
    let input = input();
    for row in input["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["nativeRuntimeDefect"] == true)
    {
        let root = temporary.project(row);
        let result = command(&root);
        assert!(!result.status.success());
        assert!(outputs(&root).is_empty());
        let diagnostics = String::from_utf8_lossy(&result.stderr)
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert!(diagnostics
            .iter()
            .any(|d| !d["noTypeScriptCounterpart"].is_null()));
    }
}
