// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Decorator metadata retains owner-selected inputs and publication boundaries.

use super::*;
use blueice_bluets::{
    compile, CheckingOptions, CompilerOptions, DiagnosticCode, EcmaTarget, IncrementalCompiler,
    MapLoader, ModuleKind, ModuleLoader, ModuleSource, RuntimePolicy,
};
use serde_json::{json, Value};
use std::cell::Cell;
use std::fs;

const ENTRY: &str = "memory:///main.ts";
const TYPES: &str = "memory:///types.ts";
const ALTERNATE: &str = "memory:///types-alternate.ts";

fn row(form: &str, module: &str, isolated: bool) -> Value {
    input()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| {
            row["form"] == form
                && row["runtimeModule"] == module
                && row["options"]["target"] == "ES2022"
                && row["options"]["useDefineForClassFields"] == true
                && if isolated {
                    row["options"]["isolatedModules"] == true
                        && row["options"]["emitDecoratorMetadata"] == true
                } else {
                    row["family"] == "metadata"
                }
        })
        .unwrap()
        .clone()
}

fn policy() -> CompilerOptions {
    CompilerOptions {
        target: EcmaTarget::Es2022,
        module_kind: ModuleKind::CommonJs,
        libraries: Some(vec![EcmaTarget::Es2022]),
        experimental_decorators: true,
        emit_decorator_metadata: true,
        declaration: true,
        checking: Some(CheckingOptions::default()),
        ..CompilerOptions::default()
    }
}

struct Owner {
    sources: MapLoader,
    alternate: Cell<bool>,
}
impl Owner {
    fn new(source: &str, provider: &str) -> Self {
        Self {
            sources: MapLoader::from([
                ModuleSource::new(ENTRY, source),
                ModuleSource::new(TYPES, provider),
                ModuleSource::new(ALTERNATE, "export interface Model { value: number; }"),
            ]),
            alternate: Cell::new(false),
        }
    }
    fn standard() -> Self {
        let row = row("imported-class", "CommonJS", false);
        Self::new(
            row["files"]["main.ts"].as_str().unwrap(),
            row["files"]["types.ts"].as_str().unwrap(),
        )
    }
}
impl ModuleLoader for Owner {
    fn load(&self, id: &str) -> Result<ModuleSource, String> {
        self.sources.load(id)
    }
    fn resolve(&self, _: &str, specifier: &str) -> Result<String, String> {
        if specifier == "./types.js" {
            Ok(if self.alternate.get() {
                ALTERNATE
            } else {
                TYPES
            }
            .into())
        } else {
            Err("metadata has no grant to resolve another source".into())
        }
    }
}

fn success(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn canonical_source_import_and_metadata_authority() {
    let owner = Owner::standard();
    let allowed = compile(ENTRY, &owner, policy());
    assert!(!allowed.has_errors(), "{:?}", allowed.diagnostics);
    let foreign = compile("memory:///ungranted.ts", &owner, policy());
    assert!(foreign.output.is_none());
    assert!(foreign
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == DiagnosticCode::ModuleNotFound));
    let temp = Temporary::new("metadata-authority");
    let root = temp.project(&row("imported-class", "CommonJS", false));
    success(&command(&root));
    let before = outputs(&root);
    #[cfg(unix)]
    {
        let outside = temp.0.join("outside");
        fs::create_dir_all(&outside).unwrap();
        let secret = outside.join("secret.ts");
        fs::write(&secret, "export class Model { value: number = 42; }").unwrap();
        fs::remove_file(root.join("types.ts")).unwrap();
        std::os::unix::fs::symlink(&secret, root.join("types.ts")).unwrap();
        let refused = command(&root);
        assert!(!refused.status.success());
        assert!(!String::from_utf8_lossy(&refused.stdout).contains("secret.ts"));
        assert_eq!(outputs(&root), before);
        assert_eq!(
            fs::read_to_string(secret).unwrap(),
            "export class Model { value: number = 42; }"
        );
    }
}

#[test]
fn failed_decorator_and_metadata_publication_is_atomic() {
    let temp = Temporary::new("metadata-atomic");
    let mut case = row("imported-interface", "ESNext", true);
    case["options"]["emitDecoratorMetadata"] = json!(false);
    let root = temp.project(&case);
    success(&command(&root));
    let before = outputs(&root);
    case["options"]["emitDecoratorMetadata"] = json!(true);
    fs::write(
        root.join("tsconfig.json"),
        serde_json::to_string_pretty(
            &json!({"compilerOptions":case["options"],"files":[case["entry"]]}),
        )
        .unwrap(),
    )
    .unwrap();
    let refused = command(&root);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("1272"));
    assert_eq!(outputs(&root), before);
}

#[test]
fn decorator_and_metadata_work_are_bounded() {
    let owner = Owner::standard();
    let normal = compile(ENTRY, &owner, policy());
    assert!(!normal.has_errors(), "{:?}", normal.diagnostics);
    for bound in ["edges", "tokens", "bytes", "depth", "segments"] {
        let mut options = policy();
        match bound {
            "edges" => options.limits.max_module_edges = 0,
            "tokens" => options.limits.parser.max_tokens = 10,
            "bytes" => options.limits.max_total_source_bytes = 10,
            "depth" => options.limits.parser.max_type_depth = 0,
            "segments" => {
                options.source_map = true;
                options.limits.max_source_map_segments = 0;
            }
            _ => unreachable!(),
        }
        let result = compile(ENTRY, &owner, options);
        assert!(result.output.is_none(), "{bound}");
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == DiagnosticCode::ResourceLimit),
            "{bound}: {:?}",
            result.diagnostics
        );
    }
    // Exact computed keys cannot relax an actual readonly declaration.
    let source = "class C { static readonly 0x20: number = 1; } C[0x20] = 2;";
    let result = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        policy(),
    );
    assert!(result.output.is_none());
    assert!(
        result.diagnostics.iter().any(|diagnostic| diagnostic
            .typescript
            .as_ref()
            .is_some_and(|ts| ts.code == 2540)),
        "{:?}",
        result.diagnostics
    );
    // Native indexed-never writes remain distinct from dotted property access.
    // A known numeric key must retain the G-T4 readonly provenance controls.
    for (statement, code) in [
        ("target[0] = value;", None),
        ("target['a'] = value;", None),
        ("target[0] = 1;", Some(2322)),
        ("target['a'] = 1;", Some(2322)),
        ("target.a = value;", Some(2339)),
    ] {
        let source = format!("function write(target: never, value: never): void {{ {statement} }}");
        let result = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            policy(),
        );
        assert_eq!(
            result.output.is_some(),
            code.is_none(),
            "{statement}: {:?}",
            result.diagnostics
        );
        if let Some(code) = code {
            assert!(
                result.diagnostics.iter().any(|diagnostic| diagnostic
                    .typescript
                    .as_ref()
                    .is_some_and(|ts| ts.code == code)),
                "{statement}: {:?}",
                result.diagnostics
            );
        }
    }
}

#[test]
fn helper_source_version_and_cache_identity_remain_bound() {
    let owner = Owner::standard();
    let mut compiler = IncrementalCompiler::new();
    let first = compiler.compile(ENTRY, &owner, policy());
    assert!(
        !first.compilation.has_errors(),
        "{:?}",
        first.compilation.diagnostics
    );
    let repeated = compiler.compile(ENTRY, &owner, policy());
    assert!(repeated.cache_hit);
    assert_eq!(first.compilation.output, repeated.compilation.output);
    let case = row("imported-class", "CommonJS", false);
    let provider = case["files"]["types.ts"].as_str().unwrap().to_string()
        + "\n// owner-revised source identity\n";
    let changed = Owner::new(case["files"]["main.ts"].as_str().unwrap(), &provider);
    let updated = compiler.compile(ENTRY, &changed, policy());
    assert!(!updated.cache_hit);
    assert!(
        !updated.compilation.has_errors(),
        "{:?}",
        updated.compilation.diagnostics
    );
    assert_ne!(
        first.compilation.project_fingerprint,
        updated.compilation.project_fingerprint
    );
    assert_eq!(
        updated.compilation.output,
        compile(ENTRY, &changed, policy()).output
    );
    let mut options = policy();
    options.emit_decorator_metadata = false;
    let switched = compiler.compile(ENTRY, &changed, options.clone());
    assert!(!switched.cache_hit);
    assert!(
        !switched.compilation.has_errors(),
        "{:?}",
        switched.compilation.diagnostics
    );
    assert_eq!(
        switched.compilation.output,
        compile(ENTRY, &changed, options).output
    );
}

#[test]
fn metadata_and_resolved_graph_changes_invalidate_unchanged_source() {
    let owner = Owner::standard();
    let mut compiler = IncrementalCompiler::new();
    let first = compiler.compile(ENTRY, &owner, policy());
    assert!(
        !first.compilation.has_errors(),
        "{:?}",
        first.compilation.diagnostics
    );
    owner.alternate.set(true);
    let changed = compiler.compile(ENTRY, &owner, policy());
    assert!(!changed.cache_hit);
    assert!(
        !changed.compilation.has_errors(),
        "{:?}",
        changed.compilation.diagnostics
    );
    assert!(changed.reused_parsed_modules.contains(ENTRY));
    assert!(changed.rechecked_modules.contains(ENTRY));
    assert_ne!(
        first.compilation.project_fingerprint,
        changed.compilation.project_fingerprint
    );
    assert_eq!(
        changed.compilation.output,
        compile(ENTRY, &owner, policy()).output
    );
    let javascript = &changed.compilation.output.as_ref().unwrap().artifacts[ENTRY].javascript;
    assert!(!javascript.contains("require("));
    assert!(javascript.contains("__bluetsMetadata(\"design:type\", Object)"));
}

#[test]
fn runtime_map_and_default_denied_policy_are_retained() {
    let case = row("import-type-class", "CommonJS", false);
    let owner = Owner::new(
        case["files"]["main.ts"].as_str().unwrap(),
        case["files"]["types.ts"].as_str().unwrap(),
    );
    let mut options = policy();
    options.source_map = true;
    let result = compile(ENTRY, &owner, options.clone());
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let artifact = &result.output.as_ref().unwrap().artifacts[ENTRY];
    assert!(!artifact.javascript.contains("require("));
    assert!(artifact
        .javascript
        .contains("__bluetsMetadata(\"design:type\", Function)"));
    assert!(artifact.source_map.is_some());
    options.runtime_policy = RuntimePolicy::StrictRuntime;
    let strict = compile(ENTRY, &owner, options);
    assert!(!strict.has_errors(), "{:?}", strict.diagnostics);
    assert!(!strict.output.as_ref().unwrap().artifacts[ENTRY]
        .javascript
        .contains("require("));
    let host =
        "interface Host {new(): {value: number};} export const result: number = new Host().value;";
    let denied = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, host)]),
        policy(),
    );
    assert!(denied.output.is_none());
    assert!(denied.diagnostics.iter().any(|diagnostic| diagnostic
        .typescript
        .as_ref()
        .is_some_and(|ts| ts.code == 2693)));
    let temp = Temporary::new("metadata-output");
    let mut case = row("imported-class", "CommonJS", false);
    let outside = temp.0.join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("owner.txt"), "preserve").unwrap();
    case["options"]["outDir"] = json!("../outside");
    let root = temp.project(&case);
    assert!(!command(&root).status.success());
    assert_eq!(
        fs::read_to_string(outside.join("owner.txt")).unwrap(),
        "preserve"
    );
}
