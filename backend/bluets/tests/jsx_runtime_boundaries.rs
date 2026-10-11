// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Runtime type providers obey owner refusals, resource bounds and cache identity.
use blueice_bluets::package_resolution::ImportMode;
use blueice_bluets::{
    compile, CompilerOptions, DiagnosticCode, IncrementalCompiler, JsxMode, ModuleKind,
    ModuleLoader, ModuleSource,
};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;
use std::{env, fs};

const ENTRY: &str = "memory:///main.tsx";
const PROVIDER_A: &str = "memory:///runtime-a.d.ts";
const PROVIDER_B: &str = "memory:///runtime-b.d.ts";
const CHILD: &str = "memory:///child.tsx";
const MAIN: &str =
    "const element = <item value={42} />; export const answer: number = element.tag;";
const PROVIDER: &str = include_str!(
    "fixtures/jsx_runtime_namespace/runtype-commonjs-i0/node_modules/typed-ui/jsx-runtime.d.ts"
);

struct Owner {
    modules: RefCell<BTreeMap<String, String>>,
    second: Cell<bool>,
    denied: Cell<bool>,
    loaded: RefCell<Vec<String>>,
    modes: RefCell<Vec<Option<ImportMode>>>,
}

impl Owner {
    fn new() -> Self {
        Self {
            modules: RefCell::new(BTreeMap::from([
                (ENTRY.into(), MAIN.into()),
                (PROVIDER_A.into(), PROVIDER.into()),
                (
                    PROVIDER_B.into(),
                    PROVIDER.replace("value: number", "value: string"),
                ),
            ])),
            second: Cell::new(false),
            denied: Cell::new(false),
            loaded: RefCell::new(Vec::new()),
            modes: RefCell::new(Vec::new()),
        }
    }
}

impl ModuleLoader for Owner {
    fn load(&self, id: &str) -> Result<ModuleSource, String> {
        self.loaded.borrow_mut().push(id.into());
        self.modules
            .borrow()
            .get(id)
            .cloned()
            .map(|source| ModuleSource::new(id, source))
            .ok_or_else(|| format!("owner has no source {id}"))
    }

    fn resolve(&self, _: &str, specifier: &str) -> Result<String, String> {
        if specifier == "./child.tsx" {
            Ok(CHILD.into())
        } else {
            Err(format!("owner has no dependency {specifier}"))
        }
    }

    fn resolve_jsx_runtime_namespace(
        &self,
        _: &str,
        specifier: &str,
        mode: Option<ImportMode>,
    ) -> Result<Option<String>, String> {
        assert_eq!(specifier, "typed-ui/jsx-runtime");
        self.modes.borrow_mut().push(mode);
        if self.denied.get() {
            return Err("owner refused JSX namespace lookup".into());
        }
        Ok(Some(
            if self.second.get() {
                PROVIDER_B
            } else {
                PROVIDER_A
            }
            .into(),
        ))
    }
}

fn options() -> CompilerOptions {
    CompilerOptions {
        jsx: Some(JsxMode::ReactJsx),
        jsx_import_source: Some("typed-ui".into()),
        module_kind: ModuleKind::CommonJs,
        ..CompilerOptions::default()
    }
}

#[test]
fn changed_provider_body_or_identity_rechecks_the_jsx_consumer() {
    for change_identity in [false, true] {
        let owner = Owner::new();
        let mut compiler = IncrementalCompiler::new();
        let first = compiler.compile(ENTRY, &owner, options());
        assert!(
            !first.compilation.has_errors(),
            "{:?}",
            first.compilation.diagnostics
        );
        let same = compiler.compile(ENTRY, &owner, options());
        assert!(same.cache_hit);
        assert!(owner
            .modes
            .borrow()
            .iter()
            .all(|mode| *mode == Some(ImportMode::Require)));
        if change_identity {
            owner.second.set(true);
        } else {
            owner.modules.borrow_mut().insert(
                PROVIDER_A.into(),
                PROVIDER.replace("value: number", "value: string"),
            );
        }
        let fresh = compile(ENTRY, &owner, options());
        let changed = compiler.compile(ENTRY, &owner, options());
        assert!(fresh.has_errors());
        assert!(!changed.cache_hit);
        assert!(changed.compilation.output.is_none());
        assert!(changed.rechecked_modules.contains(ENTRY));
        assert_ne!(
            first.compilation.project_fingerprint,
            changed.compilation.project_fingerprint
        );
        assert_eq!(fresh.diagnostics, changed.compilation.diagnostics);
        assert!(fresh.diagnostics.iter().any(|diagnostic| diagnostic
            .typescript
            .as_ref()
            .is_some_and(|d| d.code == 2322)));
    }
}

#[test]
fn owner_refusal_prevents_fallback_publication_and_provider_load() {
    let owner = Owner::new();
    owner.modules.borrow_mut().insert(ENTRY.into(), format!(
        "declare namespace JSX {{ interface Element {{ tag: number }} interface IntrinsicElements {{ item: {{ value: number }} }} }} {MAIN}"
    ));
    let allowed = compile(ENTRY, &owner, options());
    assert!(!allowed.has_errors(), "{:?}", allowed.diagnostics);
    owner.loaded.borrow_mut().clear();
    owner.denied.set(true);
    let denied = compile(ENTRY, &owner, options());
    assert!(denied.output.is_none());
    assert!(denied
        .diagnostics
        .iter()
        .any(|d| d.message.contains("owner refused JSX namespace lookup")));
    assert_eq!(owner.loaded.borrow().as_slice(), [ENTRY]);
}

#[test]
fn provider_inputs_obey_module_edge_depth_and_source_limits() {
    let owner = Owner::new();
    let control = compile(ENTRY, &owner, options());
    assert!(!control.has_errors(), "{:?}", control.diagnostics);
    for bound in ["modules", "edges", "depth", "bytes"] {
        owner.loaded.borrow_mut().clear();
        let mut limited = options();
        match bound {
            "modules" => limited.limits.max_modules = 1,
            "edges" => limited.limits.max_module_edges = 0,
            "depth" => limited.limits.max_module_depth = 0,
            "bytes" => limited.limits.max_total_source_bytes = MAIN.len() + PROVIDER.len() - 1,
            _ => unreachable!(),
        }
        let result = compile(ENTRY, &owner, limited);
        assert!(result.output.is_none(), "{bound}");
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::ResourceLimit),
            "{bound}: {:?}",
            result.diagnostics
        );
        if bound != "bytes" {
            assert_eq!(owner.loaded.borrow().as_slice(), [ENTRY], "{bound}");
        }
    }
}

#[test]
fn provider_depth_counts_the_importers_own_depth() {
    let owner = Owner::new();
    owner.modules.borrow_mut().insert(
        ENTRY.into(),
        "import { answer } from './child.tsx'; export const result: number = answer;".into(),
    );
    owner.modules.borrow_mut().insert(CHILD.into(), MAIN.into());
    let control = compile(ENTRY, &owner, options());
    assert!(!control.has_errors(), "{:?}", control.diagnostics);
    owner.loaded.borrow_mut().clear();
    let mut limited = options();
    limited.limits.max_module_depth = 1;
    let result = compile(ENTRY, &owner, limited);
    assert!(result.output.is_none());
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.code == DiagnosticCode::ResourceLimit));
    assert!(!owner.loaded.borrow().iter().any(|id| id == PROVIDER_A));
}

struct Tree(PathBuf);
impl Tree {
    fn new(label: &str) -> Self {
        let root = fs::canonicalize(env::temp_dir()).unwrap().join(format!(
            "bluets-jsx-runtime-boundary-{label}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let tree = Self(root);
        tree.write(
            "app/main.tsx",
            &format!(
                "declare namespace JSX {{ interface IntrinsicElements {{ item: any }} }} {MAIN}"
            ),
        );
        tree.write("app/tsconfig.json", r#"{"compilerOptions":{"target":"ES2022","module":"CommonJS","moduleResolution":"Node10","strict":true,"jsx":"react-jsx","jsxImportSource":"typed-ui","outDir":"out"},"files":["main.tsx"]}"#);
        tree.write(
            "app/node_modules/typed-ui/package.json",
            r#"{"name":"typed-ui"}"#,
        );
        tree
    }
    fn write(&self, name: &str, text: &str) {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    fn refusal(&self, message: &str) {
        let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .arg("--project")
            .arg(self.0.join("app/tsconfig.json"))
            .arg("--diagnostics-json")
            .output()
            .unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains(message), "{error}");
        assert!(!error.contains("OUTSIDE_RUNTIME_BODY"), "{error}");
        assert!(!self.0.join("app/out").exists());
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
#[cfg(unix)]
fn runtime_declaration_symlink_escape_is_refused_before_loading() {
    let tree = Tree::new("symlink");
    tree.write("outside.d.ts", "OUTSIDE_RUNTIME_BODY");
    std::os::unix::fs::symlink(
        tree.0.join("outside.d.ts"),
        tree.0.join("app/node_modules/typed-ui/jsx-runtime.d.ts"),
    )
    .unwrap();
    tree.refusal("outside");
}

#[test]
fn malformed_runtime_manifest_is_an_owner_error_with_no_output() {
    let tree = Tree::new("manifest");
    tree.write("app/node_modules/typed-ui/package.json", "{");
    tree.write("app/node_modules/typed-ui/jsx-runtime.d.ts", PROVIDER);
    tree.refusal("invalid");
}
