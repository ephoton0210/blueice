// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public owner builds retain module/helper policy and provider input identity.

use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn copy(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn build(config: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .args(["build", "--config"])
        .arg(config)
        .output()
        .unwrap()
}

#[test]
fn owner_manifests_bind_helper_selection_and_provider_bytes() {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("bluets-module-policy-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/module_systems/modemit-helper-options-commonjs-import-emit-provider");
    let mut fingerprints = BTreeSet::new();
    for module in ["commonjs", "amd", "umd"] {
        let directory = root.join(module);
        copy(&fixture, &directory);
        let config = directory.join("owner.json");
        let manifest_path = directory.join("published/bluetsc.manifest.json");
        for imports in [false, true] {
            for omit in [false, true] {
                fs::write(
                    &config,
                    json!({
                        "entries":["main.ts"],"outDir":"published","target":"es5",
                        "lib":["es2020"],"module":module,"moduleResolution":"node10",
                        "declaration":true,"importHelpers":imports,"noEmitHelpers":omit
                    })
                    .to_string(),
                )
                .unwrap();
                let result = build(&config);
                assert!(
                    result.status.success(),
                    "{}",
                    String::from_utf8_lossy(&result.stderr)
                );
                let bytes = fs::read(&manifest_path).unwrap();
                let manifest: Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(manifest["module"], module);
                assert_eq!(manifest["importHelpers"], imports);
                assert_eq!(manifest["noEmitHelpers"], omit);
                assert!(fingerprints.insert(manifest["fingerprint"].as_str().unwrap().to_owned()));
                if imports && !omit {
                    let provider = directory.join("node_modules/tslib/index.d.ts");
                    let original = fs::read_to_string(&provider).unwrap();
                    fs::write(
                        &provider,
                        format!("{original}\n// owner provider revision\n"),
                    )
                    .unwrap();
                    assert!(build(&config).status.success());
                    let revised = fs::read(&manifest_path).unwrap();
                    let revised_value: Value = serde_json::from_slice(&revised).unwrap();
                    assert_ne!(manifest["fingerprint"], revised_value["fingerprint"]);
                    fs::remove_file(&provider).unwrap();
                    assert!(!build(&config).status.success());
                    assert_eq!(fs::read(&manifest_path).unwrap(), revised);
                    fs::write(&provider, original).unwrap();
                }
            }
        }
    }
    assert_eq!(fingerprints.len(), 12);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn helper_selection_requires_an_owner_provider_or_explicit_global_abi() {
    use blueice_bluets::{
        compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource,
    };
    let id = "memory:///main.ts";
    let loader = MapLoader::from([ModuleSource::new(
        id,
        "export async function answer(): Promise<number> { return 42; }",
    )]);
    let ordinary = CompilerOptions {
        target: EcmaTarget::Es5,
        libraries: Some(vec![EcmaTarget::Es2020]),
        module_kind: ModuleKind::CommonJs,
        ..CompilerOptions::default()
    };
    let accepted = compile(id, &loader, ordinary.clone());
    assert!(!accepted.has_errors(), "{:?}", accepted.diagnostics);
    assert!(accepted.output.is_some());
    for (imports, omit) in [(true, false), (false, true), (true, true)] {
        let compilation = compile(
            id,
            &loader,
            CompilerOptions {
                import_helpers: imports,
                no_emit_helpers: omit,
                ..ordinary.clone()
            },
        );
        if imports {
            assert!(
                compilation.output.is_none(),
                "unresolved helper was published"
            );
            assert!(
                compilation.diagnostics.iter().any(|diagnostic| diagnostic
                    .message
                    .contains("imported helper provider cannot be resolved")),
                "{:?}",
                compilation.diagnostics
            );
        } else {
            assert!(!compilation.has_errors(), "{:?}", compilation.diagnostics);
            let output = compilation.output.unwrap();
            let javascript = &output.artifacts[id].javascript;
            assert!(javascript.contains("__awaiter("));
            assert!(javascript.contains("__generator("));
            assert!(!javascript.contains("function __awaiter"));
            assert!(!javascript.contains("function __generator"));
        }
    }
}

#[test]
fn inherited_object_methods_do_not_hide_explicit_private_members() {
    use blueice_bluets::{
        compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource,
    };
    let id = "memory:///main.ts";
    let loader = MapLoader::from([ModuleSource::new(id,
        "class Base { private valueOf(): number { return 42; } }\nexport class Derived extends Base { read(): unknown { return super.valueOf(); } }\n")]);
    let options = CompilerOptions {
        target: EcmaTarget::Es2022,
        module_kind: ModuleKind::CommonJs,
        declaration: true,
        ..CompilerOptions::default()
    };
    let compilation = compile(id, &loader, options.clone());
    assert!(
        compilation.output.is_none(),
        "private super member was published"
    );
    assert!(
        compilation
            .diagnostics
            .iter()
            .any(|d| d.typescript.as_ref().is_some_and(|ts| ts.code == 2341)),
        "{:?}",
        compilation.diagnostics
    );
    for visibility in ["protected", "public"] {
        let source = format!("class Base {{ {visibility} valueOf(): number {{ return 42; }} }}\nexport class Derived extends Base {{ read(): number {{ return super.valueOf(); }} }}\n");
        let loader = MapLoader::from([ModuleSource::new(id, source)]);
        let compilation = compile(id, &loader, options.clone());
        assert!(
            !compilation.has_errors(),
            "{visibility}: {:?}",
            compilation.diagnostics
        );
        assert!(compilation.output.is_some());
    }
}

#[test]
fn repeated_inheritance_uses_one_authorized_helper_edge() {
    use blueice_bluets::{
        compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleLoader, ModuleSource,
    };
    struct Owner(MapLoader);
    impl ModuleLoader for Owner {
        fn load(&self, id: &str) -> Result<ModuleSource, String> {
            self.0.load(id)
        }
        fn resolve(&self, _: &str, specifier: &str) -> Result<String, String> {
            if specifier == "tslib" {
                Ok("memory:///node_modules/tslib/index.d.ts".to_string())
            } else {
                Err("owner exposes only its helper provider".to_string())
            }
        }
    }
    let id = "memory:///main.ts";
    let source =
        "class Base {} export class First extends Base {} export class Second extends Base {}";
    let owner = Owner(MapLoader::from([
        ModuleSource::new(id, source),
        ModuleSource::new(
            "memory:///node_modules/tslib/index.d.ts",
            "export declare function __extends(child: Function, parent: Function): void;",
        ),
    ]));
    let mut options = CompilerOptions {
        target: EcmaTarget::Es5,
        libraries: Some(vec![EcmaTarget::Es2020]),
        module_kind: ModuleKind::CommonJs,
        import_helpers: true,
        ..CompilerOptions::default()
    };
    let ordinary = compile(id, &owner, options.clone());
    assert!(!ordinary.has_errors(), "{:?}", ordinary.diagnostics);
    options.limits.max_module_edges = 1;
    let bounded = compile(id, &owner, options.clone());
    assert!(!bounded.has_errors(), "{:?}", bounded.diagnostics);
    assert!(bounded.output.is_some());
    options.limits.max_module_edges = 0;
    let refused = compile(id, &owner, options);
    assert!(refused.output.is_none());
    assert!(refused
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("edge limit")));
}

#[test]
fn helper_policy_preserves_source_owned_function_names() {
    use blueice_bluets::{
        compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource,
    };
    let id = "memory:///main.ts";
    let loader = MapLoader::from([ModuleSource::new(
        id,
        "export function __blueice_target_owned(): number { return 42; }",
    )]);
    for (imports, omit) in [(true, false), (false, true), (true, true)] {
        let result = compile(
            id,
            &loader,
            CompilerOptions {
                target: EcmaTarget::Es5,
                module_kind: ModuleKind::CommonJs,
                import_helpers: imports,
                no_emit_helpers: omit,
                ..CompilerOptions::default()
            },
        );
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        assert!(result.output.is_some());
    }
}
