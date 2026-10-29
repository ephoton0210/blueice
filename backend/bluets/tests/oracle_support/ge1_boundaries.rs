// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Lowering retains canonical authority, finite work and artifact identity.
use super::*;
use blueice_bluets::{
    compile, AuthorizedModule, AuthorizedModuleLoader, CheckingOptions, CompilerOptions,
    DiagnosticCode, EcmaTarget, IncrementalCompiler, MapLoader, ModuleSource,
};
use std::fs;

fn policy() -> CompilerOptions {
    CompilerOptions {
        target: EcmaTarget::Es5,
        libraries: Some(vec![EcmaTarget::Es2022]),
        checking: Some(CheckingOptions::default()),
        declaration: true,
        downlevel_iteration: true,
        ..CompilerOptions::default()
    }
}
fn row(form: &str) -> Value {
    input()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| {
            r["form"] == form
                && r["accepts"] == true
                && r["options"]["target"] == "ES5"
                && r["runtimeModule"] == "CommonJS"
        })
        .unwrap()
        .clone()
}
fn sources(source: &str) -> MapLoader {
    MapLoader::from([ModuleSource::new("memory:///main.ts", source)])
}

#[test]
fn canonical_source_and_import_authority() {
    for specifier in ["../outside.ts", "https://foreign.invalid/private.ts"] {
        let loader=AuthorizedModuleLoader::new([
            AuthorizedModule::new("owned:///main.ts",format!("import {{Base}} from '{specifier}'; export class Derived extends Base {{}}")),
            AuthorizedModule::new("owned:///ungranted.ts","export class Base {}"),
        ],[]).unwrap();
        let result = compile("owned:///main.ts", &loader, policy());
        assert!(result.output.is_none());
        assert!(result
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::ModuleNotFound));
        assert!(!result.project.modules.contains_key("owned:///ungranted.ts"));
    }
    let temp = Temporary::new("owner-output");
    let mut case = row("opaque-generator-es5");
    case["files"]["main.ts"] = serde_json::json!("export const result:number=42;");
    let outside = temp.0.join("outside");
    fs::create_dir(&outside).unwrap();
    let sentinel = outside.join("owner.txt");
    fs::write(&sentinel, "preserve").unwrap();
    case["options"]["outDir"] = serde_json::json!("../outside");
    let root = temp.project(&case);
    assert!(!command(&root).status.success());
    assert_eq!(fs::read_to_string(&sentinel).unwrap(), "preserve");
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
    #[cfg(unix)]
    {
        let temp = Temporary::new("owner-symlink");
        case["options"]["outDir"] = serde_json::json!("out");
        let root = temp.project(&case);
        std::os::unix::fs::symlink(&outside, root.join("out")).unwrap();
        assert!(!command(&root).status.success());
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
    }
}
#[test]
fn failed_emission_remains_atomic() {
    let temp = Temporary::new("atomic");
    for case in input()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["accepts"] == false)
    {
        let root = temp.project(case);
        fs::write(root.join("main.ts"), "export const result:number=42;").unwrap();
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
            case["files"]["main.ts"].as_str().unwrap(),
        )
        .unwrap();
        assert!(!command(&root).status.success(), "{}", case["id"]);
        assert_eq!(outputs(&root), before, "{}", case["id"]);
    }
}
#[test]
fn lowering_and_parser_budgets() {
    let row = row("binary-yield-order");
    let source = row["files"]["main.ts"].as_str().unwrap();
    let normal = compile("memory:///main.ts", &sources(source), policy());
    assert!(normal.diagnostics.is_empty(), "{:?}", normal.diagnostics);
    for limit in ["tokens", "bytes", "depth", "segments"] {
        let mut options = policy();
        match limit {
            "tokens" => options.limits.parser.max_tokens = 10,
            "bytes" => options.limits.max_total_source_bytes = 10,
            "depth" => options.limits.parser.max_type_depth = 0,
            "segments" => {
                options.source_map = true;
                options.limits.max_source_map_segments = 0;
            }
            _ => unreachable!(),
        }
        let result = compile("memory:///main.ts", &sources(source), options);
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
fn helper_collision_and_cache_provenance() {
    let row = row("helper-name-collisions");
    let source = row["files"]["main.ts"].as_str().unwrap();
    let mut compiler = IncrementalCompiler::new();
    let first = compiler.compile("memory:///main.ts", &sources(source), policy());
    assert!(
        first.compilation.diagnostics.is_empty(),
        "{:?}",
        first.compilation.diagnostics
    );
    let output = first.compilation.output.unwrap();
    let repeated = compiler.compile("memory:///main.ts", &sources(source), policy());
    assert!(repeated.cache_hit);
    assert_eq!(repeated.compilation.output.as_ref(), Some(&output));
    for target in [EcmaTarget::Es2015, EcmaTarget::Es2017] {
        let mut options = policy();
        options.target = target;
        let changed = compiler.compile("memory:///main.ts", &sources(source), options.clone());
        assert!(!changed.cache_hit);
        assert!(
            changed.compilation.diagnostics.is_empty(),
            "{:?}",
            changed.compilation.diagnostics
        );
        assert_eq!(
            changed.compilation.output,
            compile("memory:///main.ts", &sources(source), options).output
        );
    }
    let text = &output.artifacts["memory:///main.ts"].javascript;
    assert!(text.contains("__blueice_target_generator_0"));
    assert!(text.contains("__blueice_target_async_0"));
    let temp = Temporary::new("disk-cache");
    let mut case = row;
    case["options"]["composite"] = serde_json::json!(true);
    let root = temp.project(&case);
    let build = || {
        Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .args(["--build", "--verbose"])
            .arg(root.join("tsconfig.json"))
            .output()
            .unwrap()
    };
    let first = build();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let path = root.join("tsconfig.bluetsc.tsbuildinfo");
    let mut state: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(state["version"], 24);
    state["version"] = serde_json::json!(23);
    fs::write(&path, serde_json::to_vec(&state).unwrap()).unwrap();
    let rebuilt = build();
    assert!(rebuilt.status.success());
    assert!(String::from_utf8_lossy(&rebuilt.stdout).contains("Building project"));
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(&path).unwrap()).unwrap()["version"],
        24
    );
    let warm = build();
    assert!(warm.status.success());
    assert!(String::from_utf8_lossy(&warm.stdout).contains("up to date"));
}
#[test]
fn retained_runtime_and_helper_policy() {
    let source =
        "interface Host {new():{value:number};} export const result:number=new Host().value;";
    for target in [EcmaTarget::Es5, EcmaTarget::Es2015, EcmaTarget::Es2022] {
        let mut options = policy();
        options.target = target;
        let result = compile("memory:///main.ts", &sources(source), options);
        assert!(result.output.is_none());
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.typescript.as_ref().is_some_and(|t| t.code == 2693)),
            "{:?}",
            result.diagnostics
        );
    }
    let loader = MapLoader::from([
        ModuleSource::new(
            "memory:///types.d.ts",
            "export declare class Host{value:number;}",
        ),
        ModuleSource::new(
            "memory:///main.ts",
            "import {Host} from './types.d.ts'; export const result:number=new Host().value;",
        ),
    ]);
    let result = compile("memory:///main.ts", &loader, policy());
    assert!(result.output.is_none());
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.code == DiagnosticCode::InvalidDeclarationFile));
}
#[test]
fn iterator_finally_and_repeated_build_stability() {
    let temp = Temporary::new("repeat");
    for form in [
        "finally-overrides-return",
        "delegate-external-return-finally",
        "binding-iterator-finally",
        "async-generator-exception-template",
    ] {
        let case = row(form);
        let root = temp.project(&case);
        let first = command(&root);
        assert!(
            first.status.success(),
            "{}: {}",
            form,
            String::from_utf8_lossy(&first.stderr)
        );
        let before = outputs(&root);
        let second = command(&root);
        assert!(second.status.success());
        assert_eq!(outputs(&root), before);
        assert_eq!(first.stdout, second.stdout);
    }
}
