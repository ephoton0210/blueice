// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Declaration maps carry locations, never extra source/runtime authority.
use super::*;
use blueice_bluets::{
    compile, AuthorizedModule, AuthorizedModuleLoader, CheckingOptions, CompilerOptions,
    DiagnosticCode, EcmaTarget, IncrementalCompiler, MapLoader, ModuleSource, SourceMapLookupError,
};

const SOURCE:&str="export class Box {value:number=42; read():number {return this.value;}} export const result:number=new Box().read();";
fn policy() -> CompilerOptions {
    CompilerOptions {
        target: EcmaTarget::Es2022,
        declaration: true,
        declaration_map: true,
        checking: Some(CheckingOptions::default()),
        ..CompilerOptions::default()
    }
}
fn sources(source: &str) -> MapLoader {
    MapLoader::from([ModuleSource::new("memory:///main.ts", source)])
}

#[test]
fn charged_declaration_map_work() {
    let normal = compile("memory:///main.ts", &sources(SOURCE), policy());
    assert!(normal.diagnostics.is_empty(), "{:?}", normal.diagnostics);
    for limit in ["segments", "expansions", "depth", "tokens", "bytes"] {
        let mut options = policy();
        match limit {
            "segments" => options.limits.max_source_map_segments = 0,
            "expansions" => options.limits.max_type_expansions = 0,
            "depth" => options.limits.parser.max_type_depth = 0,
            "tokens" => options.limits.parser.max_tokens = 10,
            "bytes" => options.limits.max_total_source_bytes = 10,
            _ => unreachable!(),
        }
        let result = compile("memory:///main.ts", &sources(SOURCE), options);
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
fn canonical_source_and_declaration_authority() {
    let valid = MapLoader::from([
        ModuleSource::new(
            "memory:///types.d.ts",
            "export interface Shape {value:number;} export declare const hidden:number;",
        ),
        ModuleSource::new(
            "memory:///main.ts",
            "import type {Shape} from './types.d.ts'; export const value:Shape={value:42};",
        ),
    ]);
    let result = compile("memory:///main.ts", &valid, policy());
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert!(result.output.unwrap().artifacts["memory:///main.ts"]
        .declaration_map
        .is_some());
    let invalid = MapLoader::from([
        ModuleSource::new(
            "memory:///types.d.ts",
            "export declare const hidden:number;",
        ),
        ModuleSource::new(
            "memory:///main.ts",
            "import {hidden} from './types.d.ts'; export const result=hidden;",
        ),
    ]);
    let result = compile("memory:///main.ts", &invalid, policy());
    assert!(result.output.is_none());
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.code == DiagnosticCode::InvalidDeclarationFile));
    for specifier in ["../outside.ts", "https://foreign.invalid/private.ts"] {
        let loader=AuthorizedModuleLoader::new([
            AuthorizedModule::new("owned:///main.ts",format!("import type {{Shape}} from '{specifier}'; export const value:Shape={{value:42}};")),
            AuthorizedModule::new("owned:///ungranted.ts","export interface Shape {value:number;}"),
        ],[]).unwrap();
        let result = compile("owned:///main.ts", &loader, policy());
        assert!(result.output.is_none());
        assert!(result
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::ModuleNotFound));
        assert!(!result.project.modules.contains_key("owned:///ungranted.ts"));
    }
}

#[test]
fn owner_output_root_and_symlink_confinement() {
    let input = input();
    let original = input["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["form"] == "class-fields" && r["accepts"] == true)
        .unwrap();
    for mode in ["escape", "directory-link", "map-link"] {
        let temporary = Temporary::new(mode);
        let outside = temporary.0.join("outside");
        fs::create_dir_all(&outside).unwrap();
        let sentinel = outside.join("owner.txt");
        fs::write(&sentinel, "preserve").unwrap();
        let mut row = original.clone();
        if mode == "escape" {
            row["options"]["declarationDir"] = json!("../outside");
        }
        let root = temporary.project(&row);
        if mode != "escape" {
            #[cfg(unix)]
            {
                if mode == "directory-link" {
                    std::os::unix::fs::symlink(&outside, root.join("types")).unwrap();
                } else {
                    fs::create_dir_all(root.join("types")).unwrap();
                    std::os::unix::fs::symlink(&sentinel, root.join("types/main.d.ts.map"))
                        .unwrap();
                }
            }
            #[cfg(not(unix))]
            {
                continue;
            }
        }
        let result = command(&root);
        assert!(!result.status.success(), "{mode}");
        assert!(!root.join("out").exists(), "{mode}");
        assert_eq!(fs::read_to_string(&sentinel).unwrap(), "preserve");
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
        if mode == "map-link" {
            assert!(!root.join("types/main.d.ts").exists());
        }
    }
}

#[test]
fn failed_publication_is_atomic() {
    let temporary = Temporary::new("atomic");
    let input = input();
    for row in input["cases"].as_array().unwrap().iter().filter(|r| {
        r["accepts"] == false
            && r["isolatedError"] == true
            && r["options"]["emitDeclarationOnly"] == true
    }) {
        let root = temporary.project(row);
        fs::write(root.join("src/main.ts"), "export const result:number=42;\n").unwrap();
        let valid = command(&root);
        assert!(
            valid.status.success(),
            "{}",
            String::from_utf8_lossy(&valid.stderr)
        );
        let before = outputs(&root);
        assert!(!before.is_empty());
        fs::write(
            root.join("src/main.ts"),
            row["files"]["src/main.ts"].as_str().unwrap(),
        )
        .unwrap();
        let invalid = command(&root);
        assert!(!invalid.status.success(), "{}", row["id"]);
        assert_eq!(outputs(&root), before, "{}", row["id"]);
    }
}

#[test]
fn cached_fresh_and_repeat_map_symbol_stability() {
    let mut compiler = IncrementalCompiler::new();
    let first = compiler.compile("memory:///main.ts", &sources(SOURCE), policy());
    assert!(
        first.compilation.diagnostics.is_empty(),
        "{:?}",
        first.compilation.diagnostics
    );
    let symbols = first.compilation.checked.unwrap().modules["memory:///main.ts"]
        .symbols
        .clone();
    let output = first.compilation.output.unwrap();
    let second = compiler.compile("memory:///main.ts", &sources(SOURCE), policy());
    assert!(second.cache_hit);
    assert_eq!(second.compilation.output.as_ref(), Some(&output));
    assert_eq!(
        second.compilation.checked.unwrap().modules["memory:///main.ts"].symbols,
        symbols
    );
    let edited = format!("// shift the original locations without changing declarations\n{SOURCE}");
    let next = compiler.compile("memory:///main.ts", &sources(&edited), policy());
    let fresh = compile("memory:///main.ts", &sources(&edited), policy());
    assert!(!next.cache_hit);
    assert_eq!(next.compilation.diagnostics, fresh.diagnostics);
    assert_eq!(next.compilation.output, fresh.output);
    let next = next.compilation.output.unwrap();
    assert_eq!(
        next.artifacts["memory:///main.ts"].declaration,
        output.artifacts["memory:///main.ts"].declaration
    );
    assert_ne!(
        next.artifacts["memory:///main.ts"].declaration_map,
        output.artifacts["memory:///main.ts"].declaration_map
    );
    let temporary = Temporary::new("repeat");
    for row in input()["cases"].as_array().unwrap().iter().filter(|r| {
        r["accepts"] == true
            && matches!(
                r["form"].as_str(),
                Some(
                    "class-generic"
                        | "namespace-merging"
                        | "inferred-record"
                        | "cross-module-types"
                )
            )
    }) {
        let root = temporary.project(row);
        let first = command(&root);
        assert!(
            first.status.success(),
            "{}: {}",
            row["id"],
            String::from_utf8_lossy(&first.stderr)
        );
        let before = outputs(&root);
        let second = command(&root);
        assert!(second.status.success());
        assert_eq!(outputs(&root), before);
        assert_eq!(second.stdout, first.stdout);
    }
}

#[test]
fn bounded_corrupt_map_rejection() {
    let result = compile("memory:///main.ts", &sources(SOURCE), policy());
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let output = result.output.unwrap();
    let artifact = &output.artifacts["memory:///main.ts"];
    let declaration = artifact.declaration.as_ref().unwrap();
    let mut map = artifact.declaration_map.clone().unwrap();
    // This owner supplies source text explicitly for validated pure lookup.
    map.sources_content = vec![SOURCE.into()];
    assert!(map
        .original_position_at(declaration, 0, 0, 100_000, 1_000_000)
        .unwrap()
        .is_some());
    assert_eq!(
        map.original_position_at(declaration, 0, 0, 0, 1_000_000),
        Err(SourceMapLookupError::SegmentLimit)
    );
    assert_eq!(
        map.original_position_at(declaration, 0, 0, 100_000, 1),
        Err(SourceMapLookupError::InputLimit)
    );
    for bad in ["!", "AAAA,AAAA", "ACAA", "AAACD", "gggggggggggggggggA"] {
        let mut corrupt = map.clone();
        corrupt.mappings = bad.into();
        assert_eq!(
            corrupt.original_position_at(declaration, 0, 0, 100_000, 1_000_000),
            Err(SourceMapLookupError::MalformedMap),
            "{bad}"
        );
    }
}
