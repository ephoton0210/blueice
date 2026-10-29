// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Module/helper composition retains owner authority and bounded work.
use super::*;
use blueice_bluets::{
    compile, CompilerOptions, DiagnosticCode, EcmaTarget, IncrementalCompiler, MapLoader,
    ModuleKind, ModuleLoader, ModuleSource,
};
use std::cell::Cell;
use std::fs;

const ENTRY: &str = "memory:///main.ts";
const PROVIDER: &str = "memory:///node_modules/tslib/tslib.d.ts";
const SOURCE: &str =
    "export async function compute():Promise<number>{return await Promise.resolve(42);}";
struct Owner {
    sources: MapLoader,
    esm: Cell<bool>,
}
impl Owner {
    fn new(provider: &str) -> Self {
        Self {
            sources: MapLoader::from([
                ModuleSource::new(ENTRY, SOURCE),
                ModuleSource::new(PROVIDER, provider),
            ]),
            esm: Cell::new(false),
        }
    }
}
impl ModuleLoader for Owner {
    fn load(&self, id: &str) -> Result<ModuleSource, String> {
        self.sources.load(id)
    }
    fn resolve(&self, _: &str, specifier: &str) -> Result<String, String> {
        if specifier == "tslib" {
            Ok(PROVIDER.into())
        } else {
            Err("owner grants only its pinned helper provider".into())
        }
    }
    fn implied_module_kind(&self, _: &str) -> Result<ModuleKind, String> {
        Ok(if self.esm.get() {
            ModuleKind::Esm
        } else {
            ModuleKind::CommonJs
        })
    }
}
fn provider() -> &'static str {
    input()["sharedFiles"]["node_modules/tslib/tslib.d.ts"]
        .as_str()
        .unwrap()
}
fn policy() -> CompilerOptions {
    CompilerOptions {
        target: EcmaTarget::Es2015,
        libraries: Some(vec![EcmaTarget::Es2022]),
        import_helpers: true,
        declaration: true,
        ..CompilerOptions::default()
    }
}
fn row() -> Value {
    input()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "async-arrow-finally-cjs-es2015-i1-n0")
        .unwrap()
        .clone()
}
fn successful(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn canonical_source_import_and_helper_provider_authority() {
    let temp = Temporary::new("authority");
    let root = temp.project(&row());
    successful(&command(&root));
    let outside = temp.0.join("outside");
    fs::create_dir_all(&outside).unwrap();
    let secret = outside.join("secret.d.ts");
    fs::write(&secret, provider()).unwrap();
    let before = outputs(&root);
    #[cfg(unix)]
    {
        let file = root.join("node_modules/tslib/tslib.d.ts");
        fs::remove_file(&file).unwrap();
        std::os::unix::fs::symlink(&secret, &file).unwrap();
        let refused = command(&root);
        assert!(!refused.status.success());
        assert!(!String::from_utf8_lossy(&refused.stdout).contains("secret.d.ts"));
        assert_eq!(outputs(&root), before);
    }
    assert_eq!(fs::read_to_string(secret).unwrap(), provider());
    let owner = Owner::new(provider());
    let foreign = compile("memory:///ungranted.ts", &owner, policy());
    assert!(foreign.output.is_none());
    assert!(foreign
        .diagnostics
        .iter()
        .any(|d| d.code == DiagnosticCode::ModuleNotFound));
}

#[test]
fn failed_helper_and_module_publication_is_atomic() {
    let temp = Temporary::new("atomic");
    for case in input()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["accepts"] == false || r["nativeRuntimeDefect"] == true)
    {
        let root = temp.project(case);
        let entry = root.join(case["entry"].as_str().unwrap());
        fs::write(&entry, "export const result:number=42;\n").unwrap();
        successful(&command(&root));
        let before = outputs(&root);
        assert!(!before.is_empty());
        fs::write(
            entry,
            case["files"][case["entry"].as_str().unwrap()]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert!(!command(&root).status.success(), "{}", case["id"]);
        assert_eq!(outputs(&root), before, "{}", case["id"]);
    }
}

#[test]
fn helper_selection_and_lowering_work_are_bounded() {
    let owner = Owner::new(provider());
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
                .any(|d| d.code == DiagnosticCode::ResourceLimit),
            "{bound}: {:?}",
            result.diagnostics
        );
    }
}

#[test]
fn helper_source_version_and_cache_identity_remain_bound() {
    let owner = Owner::new(provider());
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
    let changed = Owner::new(&(provider().to_owned() + "\n// owner-revised provider bytes\n"));
    let update = compiler.compile(ENTRY, &changed, policy());
    assert!(!update.cache_hit);
    assert!(
        !update.compilation.has_errors(),
        "{:?}",
        update.compilation.diagnostics
    );
    assert_ne!(
        first.compilation.project_fingerprint,
        update.compilation.project_fingerprint
    );
    for (imports, omit) in [(false, false), (false, true), (true, true)] {
        let mut options = policy();
        options.import_helpers = imports;
        options.no_emit_helpers = omit;
        let result = compiler.compile(ENTRY, &owner, options.clone());
        assert!(!result.cache_hit);
        assert!(
            !result.compilation.has_errors(),
            "{:?}",
            result.compilation.diagnostics
        );
        assert_eq!(
            result.compilation.output,
            compile(ENTRY, &owner, options).output
        );
    }
}

#[test]
fn node_format_and_provider_changes_invalidate_unchanged_source() {
    let owner = Owner::new(provider());
    let mut compiler = IncrementalCompiler::new();
    let mut options = policy();
    options.module_kind = ModuleKind::NodeNext;
    let first = compiler.compile(ENTRY, &owner, options.clone());
    assert!(
        !first.compilation.has_errors(),
        "{:?}",
        first.compilation.diagnostics
    );
    owner.esm.set(true);
    let changed = compiler.compile(ENTRY, &owner, options.clone());
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
        compile(ENTRY, &owner, options).output
    );
}

#[test]
fn runtime_helper_and_source_map_authority_are_retained() {
    let source =
        "interface Host {new():{value:number};} export const result:number=new Host().value;";
    let result = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        policy(),
    );
    assert!(result.output.is_none());
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.typescript.as_ref().is_some_and(|t| t.code == 2693)));
    let temp = Temporary::new("output-authority");
    let mut case = row();
    let outside = temp.0.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("owner.txt"), "preserve").unwrap();
    case["options"]["outDir"] = serde_json::json!("../outside");
    let root = temp.project(&case);
    assert!(!command(&root).status.success());
    assert_eq!(
        fs::read_to_string(outside.join("owner.txt")).unwrap(),
        "preserve"
    );
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
}
