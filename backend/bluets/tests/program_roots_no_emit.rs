// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Static programs keep their own cache identity and cannot recover artifacts.

use blueice_bluets::{
    compile, AuthorizedModule, AuthorizedModuleLoader, AuthorizedModuleResolution, CompilerOptions,
    DiagnosticCode, IncrementalCompiler, MapLoader, ModuleSource,
};

#[test]
fn native_cli_false_override_restores_emit_after_static_configuration() {
    use std::{fs, process::Command, time::SystemTime};
    let root = std::env::temp_dir().join(format!(
        "bluets-no-emit-override-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    let source = "const value: number = 1;\n";
    fs::write(root.join("main.ts"), source).unwrap();
    fs::write(
        root.join("tsconfig.json"),
        r#"{"compilerOptions":{"noEmit":true,"outDir":"dist"},"files":["main.ts"]}"#,
    )
    .unwrap();
    for (mode, flags, emits) in [
        ("static", vec![], false),
        ("override", vec!["--noEmit", "false"], true),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .args(["--project", "tsconfig.json"])
            .args(flags)
            .current_dir(&root)
            .output()
            .unwrap();
        fs::write(root.join(format!("{mode}.stdout")), &output.stdout).unwrap();
        fs::write(root.join(format!("{mode}.stderr")), &output.stderr).unwrap();
        assert!(output.status.success(), "retained {}", root.display());
        assert_eq!(root.join("dist/main.js").is_file(), emits);
        assert_eq!(fs::read_to_string(root.join("main.ts")).unwrap(), source);
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_static_declaration_consumer_grants_neither_runtime_import_nor_emit_recovery() {
    let loader = AuthorizedModuleLoader::new(
        [
            AuthorizedModule::new(
                "program:///main.ts",
                "import { Item } from './types'; const value: Item = { value: 1 };",
            ),
            AuthorizedModule::new(
                "program:///types.d.ts",
                "export interface Item { value: number; }",
            ),
        ],
        [AuthorizedModuleResolution::new(
            "program:///main.ts",
            "./types",
            "program:///types.d.ts",
        )],
    )
    .unwrap();
    let static_options = CompilerOptions {
        no_emit: true,
        declaration: true,
        declaration_map: true,
        source_map: true,
        ..CompilerOptions::default()
    };
    let checked = compile("program:///main.ts", &loader, static_options.clone());
    assert!(!checked.has_errors(), "{:?}", checked.diagnostics);
    assert!(checked.checked.is_some());
    assert!(checked.debug_info.is_some());
    assert!(checked.output.is_none());
    assert!(checked
        .emit_for_native_cli(&static_options)
        .unwrap()
        .is_none());
    let build_options = CompilerOptions {
        no_emit: false,
        ..static_options
    };
    assert!(checked
        .emit_for_native_cli(&build_options)
        .unwrap()
        .is_none());
    let refused = compile("program:///main.ts", &loader, build_options);
    assert!(refused.has_errors());
    assert!(refused.output.is_none());
    assert!(refused
        .diagnostics
        .iter()
        .any(|item| item.code == DiagnosticCode::InvalidDeclarationFile));
    assert_ne!(checked.project_fingerprint, refused.project_fingerprint);
}

#[test]
fn static_checks_skip_emit_work_but_preserve_semantic_and_declaration_errors() {
    let loader = MapLoader::from([ModuleSource::new(
        "program:///main.ts",
        "export const value: number = 1;",
    )]);
    let mut options = CompilerOptions {
        no_emit: true,
        source_map: true,
        ..CompilerOptions::default()
    };
    options.limits.max_source_map_segments = 0;
    let result = compile("program:///main.ts", &loader, options.clone());
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    assert!(result.output.is_none());
    options.no_emit = false;
    let emitted = compile("program:///main.ts", &loader, options);
    assert!(emitted.has_errors());
    assert!(emitted
        .diagnostics
        .iter()
        .any(|item| item.code == DiagnosticCode::ResourceLimit));

    for (entry, source) in [
        ("program:///main.ts", "const value: number = 'wrong';"),
        (
            "program:///types.d.ts",
            "export function run(): number { return 1; }",
        ),
    ] {
        let result = compile(
            entry,
            &MapLoader::from([ModuleSource::new(entry, source)]),
            CompilerOptions {
                no_emit: true,
                ..CompilerOptions::default()
            },
        );
        assert!(result.has_errors(), "accepted {entry}: {source}");
        assert!(result.output.is_none());
        assert!(result.debug_info.is_none());
    }
}

#[test]
fn incremental_modes_keep_distinct_fingerprints_and_artifact_authority() {
    let loader = MapLoader::from([ModuleSource::new(
        "program:///main.ts",
        "export const value: number = 1;",
    )]);
    let mut compiler = IncrementalCompiler::new();
    let build = CompilerOptions::default();
    let cold = compiler.compile("program:///main.ts", &loader, build.clone());
    assert!(!cold.compilation.has_errors());
    assert!(cold.compilation.output.is_some());
    let warm = compiler.compile("program:///main.ts", &loader, build.clone());
    assert!(warm.cache_hit);
    let static_options = CompilerOptions {
        no_emit: true,
        ..build.clone()
    };
    let checked = compiler.compile("program:///main.ts", &loader, static_options.clone());
    assert!(!checked.cache_hit);
    assert!(!checked.compilation.has_errors());
    assert!(checked.compilation.output.is_none());
    assert_ne!(
        cold.compilation.project_fingerprint,
        checked.compilation.project_fingerprint
    );
    let checked_warm = compiler.compile("program:///main.ts", &loader, static_options);
    assert!(checked_warm.cache_hit);
    assert!(checked_warm.compilation.output.is_none());
    assert_eq!(
        checked_warm.compilation.debug_info,
        checked.compilation.debug_info
    );
    let restored = compiler.compile("program:///main.ts", &loader, build);
    assert!(!restored.cache_hit);
    assert!(restored.compilation.output.is_some());
    assert_eq!(restored.compilation.output, cold.compilation.output);
}
