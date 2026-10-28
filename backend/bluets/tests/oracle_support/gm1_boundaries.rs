// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! These five owner controls are not native TypeScript agreements.
use super::*;
use blueice_bluets::{
    compile, CheckingOptions, CompilerOptions, DiagnosticCode, MapLoader, ModuleSource,
};

fn row(form: &str) -> Value {
    input()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["form"] == form && r["accepts"] == true && r["runtimeModule"] == "CommonJS")
        .unwrap()
        .clone()
}
fn success(result: &std::process::Output) {
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn canonical_js_sources_cannot_escape_owner_roots() {
    let temporary = Temporary::new("confine");
    let package = row("package-main-js");
    let root = temporary.project(&package);
    success(&command(&root));
    let mut chain = package.clone();
    let depth_temporary = Temporary::new("depth");
    chain["files"]["node_modules/pkg/index.js"] = json!("export {double} from './bridge.js';\n");
    chain["files"]["node_modules/pkg/bridge.js"] = json!("export {double} from 'second';\n");
    chain["files"]["node_modules/second/package.json"] =
        json!("{\"type\":\"module\",\"main\":\"index.js\"}");
    chain["files"]["node_modules/second/index.js"] =
        package["files"]["node_modules/pkg/index.js"].clone();
    let bounded = depth_temporary.project(&chain);
    let refused = command(&bounded);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("maxNodeModuleJsDepth"));
    assert!(outputs(&bounded).is_empty());
    chain["options"]["maxNodeModuleJsDepth"] = json!(2);
    success(&command(&depth_temporary.project(&chain)));
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let outside = temporary.0.join("outside");
        write_files(
            &outside,
            [
                (
                    "package.json",
                    "{\"type\":\"module\",\"main\":\"index.js\"}",
                ),
                ("index.js", "throw new Error('PRIVATE_OUTSIDE_BODY');"),
            ],
        );
        fs::rename(
            root.join("node_modules/pkg"),
            root.join("node_modules/original-pkg"),
        )
        .unwrap();
        symlink(&outside, root.join("node_modules/pkg")).unwrap();
        let refused = command(&root);
        assert!(!refused.status.success());
        let error = String::from_utf8_lossy(&refused.stderr);
        assert!(error.contains("outside"), "{error}");
        assert!(!error.contains("PRIVATE_OUTSIDE_BODY"));
        let mapped = temporary.project(&row("paths-exact-js"));
        fs::remove_file(mapped.join("vendor/math.js")).unwrap();
        symlink(outside.join("index.js"), mapped.join("vendor/math.js")).unwrap();
        let refused = command(&mapped);
        assert!(!refused.status.success());
        assert!(String::from_utf8_lossy(&refused.stderr).contains("outside"));
    }
}

#[test]
fn ordered_fallback_does_not_bypass_an_authority_failure() {
    let temporary = Temporary::new("fallback");
    let mut case = row("paths-ordered-fallback");
    case["options"]["paths"]["math/*"] = json!(["first/*", "vendor/*"]);
    let root = temporary.project(&case);
    success(&command(&root));
    #[cfg(unix)]
    {
        let outside = temporary.0.join("outside");
        write_files(
            &outside,
            [(
                "math.js",
                "export function double(value) { return 'PRIVATE_OUTSIDE'; }",
            )],
        );
        std::os::unix::fs::symlink(&outside, root.join("first")).unwrap();
        let refused = command(&root);
        assert!(!refused.status.success());
        let error = String::from_utf8_lossy(&refused.stderr);
        assert!(error.contains("outside"), "{error}");
        assert!(!error.contains("PRIVATE_OUTSIDE"));
    }
}

#[test]
fn sibling_selection_and_explicit_javascript_mapping_keep_native_priority() {
    let temporary = Temporary::new("siblings");
    let case = row("paths-typescript-sibling");
    let root = temporary.project(&case);
    let initial = command(&root);
    success(&initial);
    let files = String::from_utf8_lossy(&initial.stdout);
    assert!(files.contains("vendor/math.ts"));
    assert!(!files.contains("vendor/math.js"));
    fs::remove_file(root.join("vendor/math.ts")).unwrap();
    assert!(!command(&root).status.success());
    fs::write(
        root.join("vendor/math.ts"),
        case["files"]["vendor/math.ts"].as_str().unwrap(),
    )
    .unwrap();
    success(&command(&root));
    let explicit = temporary.project(&row("paths-explicit-js-sibling"));
    let result = command(&explicit);
    success(&result);
    let files = String::from_utf8_lossy(&result.stdout);
    assert!(files.contains("vendor/math.js"));
    assert!(!files.contains("vendor/math.ts"));
}

#[test]
fn manifest_changes_missing_inputs_and_restoration_rebuild_safely() {
    let temporary = Temporary::new("invalidation");
    let case = row("package-main-js");
    let root = temporary.project(&case);
    let file = root.join("tsconfig.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    config["compilerOptions"]["composite"] = json!(true);
    fs::write(&file, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    let build = || {
        Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .arg("--build")
            .arg(&file)
            .arg("--verbose")
            .output()
            .unwrap()
    };
    let first = build();
    success(&first);
    let original = outputs(&root);
    let warm = build();
    success(&warm);
    assert!(!String::from_utf8_lossy(&warm.stdout).contains("Building project:"));
    let source = root.join("node_modules/pkg/index.js");
    let original_source = fs::read(&source).unwrap();
    fs::write(&source, "/** @param {string} value\n * @returns {number} */\nexport function double(value) { return 42; }\n").unwrap();
    let changed_source = build();
    assert!(!changed_source.status.success());
    assert!(String::from_utf8_lossy(&changed_source.stdout).contains("TS2345"));
    fs::remove_file(&source).unwrap();
    assert!(!build().status.success());
    fs::write(&source, original_source).unwrap();
    success(&build());
    assert_eq!(outputs(&root), original);
    let manifest = root.join("node_modules/pkg/package.json");
    let original_manifest = fs::read(&manifest).unwrap();
    fs::write(root.join("node_modules/pkg/other.js"),"/** @param {string} value\n * @returns {number} */\nexport function double(value) { return 42; }\n").unwrap();
    fs::write(&manifest, "{\"type\":\"module\",\"main\":\"other.js\"}").unwrap();
    let changed = build();
    assert!(!changed.status.success());
    assert!(String::from_utf8_lossy(&changed.stdout).contains("TS2345"));
    fs::remove_file(&manifest).unwrap();
    let missing = build();
    success(&missing);
    assert!(String::from_utf8_lossy(&missing.stdout).contains("Building project:"));
    fs::write(&manifest, original_manifest).unwrap();
    let restored = build();
    success(&restored);
    assert_eq!(outputs(&root), original);
}

#[test]
fn jsdoc_edges_share_work_limits_and_do_not_create_runtime_imports() {
    let sources=MapLoader::from([
        ModuleSource::new("memory:///entry.ts","import {read} from './api.js'; export const result = read({value:42});"),
        ModuleSource::new("memory:///api.js","/** @param {import('./schema.ts').Item} value\n * @returns {number} */\nexport function read(value) { return value.value; }"),
        ModuleSource::new("memory:///schema.ts","export interface Item { value: number; }"),
    ]);
    let policy = CompilerOptions {
        allow_js: true,
        check_js: true,
        checking: Some(CheckingOptions::default()),
        ..CompilerOptions::default()
    };
    let initial = compile("memory:///entry.ts", &sources, policy.clone());
    assert!(initial.diagnostics.is_empty(), "{:?}", initial.diagnostics);
    assert!(initial.project.modules.contains_key("memory:///schema.ts"));
    assert!(!initial.output.unwrap().artifacts["memory:///api.js"]
        .javascript
        .contains("from './schema"));
    for limit in ["modules", "edges", "depth", "bytes", "expansion", "tokens"] {
        let mut bounded = policy.clone();
        match limit {
            "modules" => bounded.limits.max_modules = 2,
            "edges" => bounded.limits.max_module_edges = 1,
            "depth" => bounded.limits.max_module_depth = 1,
            "bytes" => bounded.limits.max_total_source_bytes = 80,
            "expansion" => bounded.limits.max_type_expansions = 0,
            "tokens" => bounded.limits.parser.max_tokens = 15,
            _ => unreachable!(),
        }
        let result = compile("memory:///entry.ts", &sources, bounded);
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
    // Ordinary JavaScript tokens fit; only the erased comment exhausts the
    // same token allowance. This isolates comment work from graph limits.
    let source = "/** @type {number | number | number | number} */\nexport const value = 42;";
    let loader = MapLoader::from([ModuleSource::new("memory:///budget.js", source)]);
    let ordinary = blueice_bluets::lex("memory:///budget.js", source)
        .unwrap()
        .len()
        - 1;
    let mut bounded = policy;
    bounded.limits.parser.max_tokens = ordinary + 2;
    let result = compile("memory:///budget.js", &loader, bounded);
    assert!(result.output.is_none());
    assert!(result
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == DiagnosticCode::ResourceLimit));
    let nested = MapLoader::from([ModuleSource::new("memory:///nested.js", "function read() { /** @type {string} */ const value = 42; return value; } export const result = 42;")]);
    let result = compile(
        "memory:///nested.js",
        &nested,
        CompilerOptions {
            allow_js: true,
            check_js: true,
            ..CompilerOptions::default()
        },
    );
    assert!(result.output.is_none());
    assert!(result
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == DiagnosticCode::UnsupportedSyntax));
}
