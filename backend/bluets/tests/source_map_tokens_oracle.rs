// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! K.11: compare decoded source-map mapping sets, not encoded VLQ text.

use blueice_bluets::{
    compile, lex, CompilerOptions, MapLoader, ModuleKind, ModuleSource, TokenKind,
};
use serde_json::Value;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[path = "source_map_tokens_oracle/matrix.rs"]
mod matrix;
use matrix::matrix_rows;

#[path = "source_map_tokens_oracle/legacy_metadata.rs"]
mod legacy_metadata;

#[path = "source_map_tokens_oracle/decoder_tests.rs"]
mod decoder_tests;

#[path = "source_map_tokens_oracle/decoding.rs"]
mod decoding;
use decoding::{decode_map, decode_mappings, mapped_source_at, mapped_tokens, source_position};

#[path = "source_map_tokens_oracle/alignment.rs"]
mod alignment;

#[path = "source_map_tokens_oracle/generated_gap_roles.rs"]
mod generated_gap_roles;

#[path = "source_map_tokens_oracle/non_decorator.rs"]
mod non_decorator;

#[path = "source_map_tokens_oracle/system_wrapper.rs"]
mod system_wrapper;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Mapping {
    generated_line: usize,
    generated_column: i64,
    source_index: i64,
    source_line: i64,
    source_column: i64,
    name_index: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TokenMapping {
    token: String,
    source_index: i64,
    source_line: i64,
    source_column: i64,
    name_index: Option<i64>,
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to pinned TypeScript 5.9.3"]
fn decoded_token_maps_match_pinned_typescript() {
    let tsc = PathBuf::from(
        env::var_os("BLUEICE_BLUETSC_ORACLE")
            .expect("set BLUEICE_BLUETSC_ORACLE to pinned TypeScript 5.9.3"),
    );
    let version = Command::new(&tsc).arg("--version").output().unwrap();
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8(version.stdout).unwrap().trim(),
        "Version 5.9.3"
    );

    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/k11_source_maps");
    let mut differences = Vec::new();
    for row in matrix_rows() {
        let name = row.name;
        let mut source = fs::read_to_string(corpus.join(row.file)).unwrap();
        match row.line_endings {
            "lf" => assert!(!source.contains('\r'), "{name}: fixture must use LF"),
            "crlf" => {
                assert!(!source.contains('\r'), "{name}: fixture must use LF");
                source = source.replace('\n', "\r\n");
            }
            other => panic!("{name}: unsupported line endings {other}"),
        }
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory =
            env::temp_dir().join(format!("blueice-k11-{name}-{}-{nonce}", std::process::id()));
        let native = directory.join("native");
        let native_without_inline = directory.join("native-no-inline");
        let blueice = directory.join("blueice");
        fs::create_dir_all(&native).unwrap();
        fs::create_dir_all(&native_without_inline).unwrap();
        fs::create_dir_all(&blueice).unwrap();
        let input = directory.join("main.ts");
        fs::write(&input, &source).unwrap();
        let mut native_args = vec![
            "--target",
            row.target.as_str(),
            "--module",
            if row.module == ModuleKind::System {
                "System"
            } else {
                "ES2022"
            },
            "--strict",
            "--pretty",
            "false",
            "--sourceMap",
            "--inlineSources",
        ];
        if row.legacy {
            native_args.push("--experimentalDecorators");
        }
        if row.metadata {
            native_args.push("--emitDecoratorMetadata");
        }
        native_args.push("--outDir");
        let output = Command::new(&tsc)
            .args(&native_args)
            .arg(&native)
            .arg(&input)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        native_args.retain(|arg| *arg != "--inlineSources");
        let no_inline_output = Command::new(&tsc)
            .args(&native_args)
            .arg(&native_without_inline)
            .arg(&input)
            .output()
            .unwrap();
        assert!(
            no_inline_output.status.success(),
            "{name}: Native inlineSources-off: {}",
            String::from_utf8_lossy(&no_inline_output.stderr)
        );

        let module = "memory:///main.ts";
        let compilation = compile(
            module,
            &MapLoader::from([ModuleSource::new(module, source.clone())]),
            CompilerOptions {
                target: row.target,
                module_kind: row.module,
                experimental_decorators: row.legacy,
                emit_decorator_metadata: row.metadata,
                source_map: true,
                inline_sources: true,
                ..CompilerOptions::default()
            },
        );
        assert!(
            !compilation.has_errors(),
            "{name}: {:#?}",
            compilation.diagnostics
        );
        let output = compilation.output.unwrap();
        let artifact = &output.artifacts[module];
        let blue_map: Value =
            serde_json::from_str(&artifact.source_map.as_ref().unwrap().to_json()).unwrap();
        let native_map: Value =
            serde_json::from_str(&fs::read_to_string(native.join("main.js.map")).unwrap()).unwrap();
        let native_no_inline_map: Value = serde_json::from_str(
            &fs::read_to_string(native_without_inline.join("main.js.map")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            blue_map["version"], native_map["version"],
            "{name}: map version"
        );
        assert_eq!(blue_map["file"], native_map["file"], "{name}: output file");
        assert_eq!(
            blue_map["sourceRoot"].as_str().unwrap_or(""),
            native_map["sourceRoot"].as_str().unwrap_or(""),
            "{name}: source root"
        );
        let source_names = |map: &Value| {
            map["sources"]
                .as_array()
                .unwrap()
                .iter()
                .map(|source| {
                    source
                        .as_str()
                        .unwrap()
                        .rsplit(['/', '\\'])
                        .next()
                        .unwrap()
                        .to_string()
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            source_names(&blue_map),
            source_names(&native_map),
            "{name}: sources"
        );
        assert_eq!(blue_map["names"], native_map["names"], "{name}: map names");
        assert_eq!(
            blue_map["sourcesContent"], native_map["sourcesContent"],
            "{name}: inline source"
        );
        let plain = compile(
            module,
            &MapLoader::from([ModuleSource::new(module, source.clone())]),
            CompilerOptions {
                target: row.target,
                module_kind: row.module,
                experimental_decorators: row.legacy,
                emit_decorator_metadata: row.metadata,
                ..CompilerOptions::default()
            },
        );
        assert!(
            !plain.has_errors(),
            "{name}: plain compile: {:#?}",
            plain.diagnostics
        );
        assert_eq!(
            artifact.javascript,
            plain.output.unwrap().artifacts[module].javascript,
            "{name}: map-on/off JavaScript"
        );
        let without_inline = compile(
            module,
            &MapLoader::from([ModuleSource::new(module, source.clone())]),
            CompilerOptions {
                target: row.target,
                module_kind: row.module,
                experimental_decorators: row.legacy,
                emit_decorator_metadata: row.metadata,
                source_map: true,
                inline_sources: false,
                ..CompilerOptions::default()
            },
        );
        assert!(
            !without_inline.has_errors(),
            "{name}: no-inline compile: {:#?}",
            without_inline.diagnostics
        );
        let without_inline_artifact = &without_inline.output.as_ref().unwrap().artifacts[module];
        let without_inline_map: Value = serde_json::from_str(
            &without_inline_artifact
                .source_map
                .as_ref()
                .unwrap()
                .to_json(),
        )
        .unwrap();
        assert!(
            without_inline_map.get("sourcesContent").is_none(),
            "{name}: inlineSources disabled"
        );
        assert!(
            native_no_inline_map.get("sourcesContent").is_none(),
            "{name}: Native inlineSources disabled"
        );
        assert_eq!(
            without_inline_map["mappings"], blue_map["mappings"],
            "{name}: inlineSources must not change mappings"
        );
        assert_eq!(
            native_no_inline_map["mappings"], native_map["mappings"],
            "{name}: Native inlineSources must not change mappings"
        );
        assert_eq!(
            without_inline_map["names"], native_no_inline_map["names"],
            "{name}: no-inline names"
        );
        assert_eq!(
            without_inline_map["file"], native_no_inline_map["file"],
            "{name}: no-inline output file"
        );
        assert_eq!(
            source_names(&without_inline_map),
            source_names(&native_no_inline_map),
            "{name}: no-inline sources"
        );
        assert_eq!(
            without_inline_artifact.javascript, artifact.javascript,
            "{name}: inlineSources must not change JavaScript"
        );
        let repeated = compile(
            module,
            &MapLoader::from([ModuleSource::new(module, source.clone())]),
            CompilerOptions {
                target: row.target,
                module_kind: row.module,
                experimental_decorators: row.legacy,
                emit_decorator_metadata: row.metadata,
                source_map: true,
                inline_sources: true,
                ..CompilerOptions::default()
            },
        );
        assert!(
            !repeated.has_errors(),
            "{name}: repeat compile: {:#?}",
            repeated.diagnostics
        );
        let repeated_artifact = &repeated.output.as_ref().unwrap().artifacts[module];
        assert_eq!(
            repeated_artifact.source_map.as_ref().unwrap().to_json(),
            artifact.source_map.as_ref().unwrap().to_json(),
            "{name}: deterministic map"
        );
        assert_eq!(
            repeated_artifact.javascript, artifact.javascript,
            "{name}: deterministic JavaScript"
        );

        fs::write(native.join("package.json"), "{\"type\":\"module\"}\n").unwrap();
        fs::write(blueice.join("package.json"), "{\"type\":\"module\"}\n").unwrap();
        fs::write(blueice.join("main.js"), &artifact.javascript).unwrap();
        let node = env::var_os("BLUEICE_NODE_ORACLE").unwrap_or_else(|| "node".into());
        let run = |path: &Path| {
            let mut command = Command::new(&node);
            if row.system_runner {
                command.arg("-e").arg(system_wrapper::LOADER);
            }
            command.arg(path).output().unwrap()
        };
        let native_run = run(&native.join("main.js"));
        let blue_run = run(&blueice.join("main.js"));
        assert!(
            native_run.status.success(),
            "{name}: native execution failed: {}",
            String::from_utf8_lossy(&native_run.stderr)
        );
        assert!(
            blue_run.status.success(),
            "{name}: BlueTSC execution failed: {}",
            String::from_utf8_lossy(&blue_run.stderr)
        );
        assert_eq!(blue_run.status, native_run.status, "{name}: exit status");
        assert_eq!(blue_run.stdout, native_run.stdout, "{name}: runtime output");
        assert_eq!(blue_run.stderr, native_run.stderr, "{name}: runtime stderr");

        let native_mappings = decode_map(&native_map);
        let blue_mappings = decode_map(&blue_map);
        alignment::assert_transition_inventory(name, &native_mappings, &blue_mappings);
        let source_map = artifact.source_map.as_ref().unwrap();
        for token in lex("<generated>", &artifact.javascript).unwrap() {
            if token.kind == TokenKind::Eof {
                continue;
            }
            let (line, column) = source_position(&artifact.javascript, token.start);
            let public = source_map
                .original_position_at(
                    &artifact.javascript,
                    line as usize,
                    column as usize,
                    100_000,
                    32_000_000,
                )
                .unwrap();
            let public = public.map(|position| {
                (
                    position.source_index as i64,
                    position.line as i64,
                    position.column_utf16 as i64,
                    position.name_index.map(|index| index as i64),
                )
            });
            assert_eq!(
                public,
                mapped_source_at(&artifact.javascript, &blue_mappings, token.start),
                "{name}: public generated-position lookup at token {:?}",
                token.text
            );
        }
        let native_javascript = fs::read_to_string(native.join("main.js")).unwrap();
        let native_without_trailer = native_javascript
            .split("//# sourceMappingURL=")
            .next()
            .unwrap()
            .trim_end();
        if native_without_trailer == artifact.javascript.trim_end() {
            assert_eq!(
                blue_mappings, native_mappings,
                "{name}: identical JavaScript requires complete mapping-set parity"
            );
        }
        let native_tokens = mapped_tokens(&native_javascript, &native_mappings);
        let blue_tokens = mapped_tokens(&artifact.javascript, &blue_mappings);
        if matches!(
            name,
            "k11-standard-method"
                | "k11-compacted-relocation"
                | "k11-erased-relocation"
                | "k11-composed-decorators"
                | "k11-legacy-class-member"
                | "k11-legacy-parameter-metadata"
                | "k11-nondecorator-helper"
        ) {
            assert_eq!(
                alignment::first_ordered_role_delta(&native_tokens, &blue_tokens),
                None,
                "{name}: every source-anchored role must remain ordered and position-aligned"
            );
        }
        let native_spellings = native_tokens
            .iter()
            .map(|token| token.token.as_str())
            .collect::<Vec<_>>();
        let blue_spellings = blue_tokens
            .iter()
            .map(|token| token.token.as_str())
            .collect::<Vec<_>>();
        if matches!(
            name,
            "k11-standard-method"
                | "k11-compacted-relocation"
                | "k11-erased-relocation"
                | "k11-composed-decorators"
                | "k11-legacy-class-member"
                | "k11-legacy-parameter-metadata"
        ) {
            generated_gap_roles::assert_decorator_token_roles(name, &native_tokens, &blue_tokens);
        } else if name == "k11-nondecorator-helper" {
            assert_eq!(row.file, "object_spread_helper.ts");
            assert_eq!(row.line_endings, "lf");
            assert_eq!(row.target.as_str(), "es5");
            assert_eq!(row.module, ModuleKind::Esm);
            assert!(!row.legacy && !row.metadata);
            non_decorator::object_spread_helper_frames_match_native();
        } else if name == "k11-system-wrapper" {
            assert_eq!(row.file, "system_wrapper.ts");
            assert_eq!(row.line_endings, "lf");
            assert_eq!(row.target.as_str(), "es2022");
            assert_eq!(row.module, ModuleKind::System);
            assert!(!row.legacy && !row.metadata);
            system_wrapper::system_wrapper_frames_match_native();
        } else if blue_spellings != native_spellings {
            let prefix = blue_spellings
                .iter()
                .zip(&native_spellings)
                .take_while(|(blue, native)| blue == native)
                .count();
            differences.push(format!(
                "{name}: emitted token-role table is missing; {} Native vs {} BlueTSC tokens (mapped: {} vs {}), first difference at {prefix}: Native {:?}, BlueTSC {:?}; first mapped-role deltas: {:?}; first ordered mapped-role delta: {:?}; first transition delta: {:?}",
                native_spellings.len(),
                blue_spellings.len(),
                native_tokens.iter().filter(|token| token.source_index >= 0).count(),
                blue_tokens.iter().filter(|token| token.source_index >= 0).count(),
                native_spellings.get(prefix),
                blue_spellings.get(prefix),
                alignment::first_mapped_role_deltas(&native_tokens, &blue_tokens),
                alignment::first_ordered_role_delta(&native_tokens, &blue_tokens),
                alignment::first_transition_delta(&native_tokens, &blue_tokens),
            ));
        } else if native_tokens != blue_tokens {
            let mismatches = native_tokens
                .iter()
                .zip(&blue_tokens)
                .enumerate()
                .filter(|(_, (native, blue))| native != blue)
                .take(8)
                .collect::<Vec<_>>();
            differences.push(format!(
                "{name}: {} Native segments, {} BlueTSC segments; first token mismatches: {mismatches:?}",
                native_mappings.len(),
                blue_mappings.len(),
            ));
        }
        fs::remove_dir_all(&directory).unwrap();
    }
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to pinned TypeScript 5.9.3"]
fn decorator_relocation_and_helper_frames_match_pinned_typescript() {
    let tsc = PathBuf::from(
        env::var_os("BLUEICE_BLUETSC_ORACLE")
            .expect("set BLUEICE_BLUETSC_ORACLE to pinned TypeScript 5.9.3"),
    );
    let version = Command::new(&tsc).arg("--version").output().unwrap();
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8(version.stdout).unwrap().trim(),
        "Version 5.9.3"
    );
    let source = include_str!("fixtures/k11_source_maps/decorated.ts");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = env::temp_dir().join(format!(
        "blueice-k11-decorated-{}-{nonce}",
        std::process::id()
    ));
    let native = directory.join("native");
    let blueice = directory.join("blueice");
    fs::create_dir_all(&native).unwrap();
    fs::create_dir_all(&blueice).unwrap();
    let input = directory.join("main.ts");
    fs::write(&input, source).unwrap();
    let compile_native = Command::new(&tsc)
        .args([
            "--target",
            "ES2022",
            "--module",
            "ES2022",
            "--strict",
            "--pretty",
            "false",
            "--sourceMap",
            "--inlineSources",
            "--outDir",
        ])
        .arg(&native)
        .arg(&input)
        .output()
        .unwrap();
    assert!(
        compile_native.status.success(),
        "{}",
        String::from_utf8_lossy(&compile_native.stderr)
    );
    let module = "memory:///main.ts";
    let compilation = compile(
        module,
        &MapLoader::from([ModuleSource::new(module, source)]),
        CompilerOptions {
            source_map: true,
            inline_sources: true,
            ..CompilerOptions::default()
        },
    );
    assert!(!compilation.has_errors(), "{:#?}", compilation.diagnostics);
    let output = compilation.output.unwrap();
    let artifact = &output.artifacts[module];
    let plain = compile(
        module,
        &MapLoader::from([ModuleSource::new(module, source)]),
        CompilerOptions::default(),
    );
    assert!(!plain.has_errors(), "{:#?}", plain.diagnostics);
    assert_eq!(
        artifact.javascript,
        plain.output.unwrap().artifacts[module].javascript,
        "source-map instrumentation must not change JavaScript"
    );
    let native_javascript = fs::read_to_string(native.join("main.js")).unwrap();
    let native_map: Value =
        serde_json::from_str(&fs::read_to_string(native.join("main.js.map")).unwrap()).unwrap();
    let blue_map: Value =
        serde_json::from_str(&artifact.source_map.as_ref().unwrap().to_json()).unwrap();
    assert_eq!(blue_map["names"], native_map["names"]);
    assert_eq!(blue_map["sourcesContent"], native_map["sourcesContent"]);
    let node = env::var_os("BLUEICE_NODE_ORACLE").unwrap_or_else(|| "node".into());
    fs::write(blueice.join("main.js"), &artifact.javascript).unwrap();
    let native_run = Command::new(&node)
        .arg(native.join("main.js"))
        .output()
        .unwrap();
    let blue_run = Command::new(&node)
        .arg(blueice.join("main.js"))
        .output()
        .unwrap();
    assert!(
        native_run.status.success(),
        "{}",
        String::from_utf8_lossy(&native_run.stderr)
    );
    assert!(
        blue_run.status.success(),
        "{}",
        String::from_utf8_lossy(&blue_run.stderr)
    );
    assert_eq!(blue_run.stdout, native_run.stdout);
    assert_eq!(blue_run.stdout, b"42\n");

    let native_mappings = decode_map(&native_map);
    let blue_mappings = decode_map(&blue_map);
    let source_decorator = source.find("@decoSite").unwrap() + 1;
    let expected = source_position(source, source_decorator);
    let native_decorator = native_javascript
        .find("_method_decorators = [decoSite]")
        .unwrap()
        + "_method_decorators = [".len();
    let blue_decorator = artifact
        .javascript
        .find("_method_decorators = [decoSite]")
        .unwrap()
        + "_method_decorators = [".len();
    let native_decorator_site =
        mapped_source_at(&native_javascript, &native_mappings, native_decorator);
    let blue_decorator_site =
        mapped_source_at(&artifact.javascript, &blue_mappings, blue_decorator);
    let native_helper = native_javascript.find("var __esDecorate").unwrap();
    let blue_helper = artifact.javascript.find("var __bluetsEsDecorate").unwrap();
    let native_helper_site = mapped_source_at(&native_javascript, &native_mappings, native_helper);
    let blue_helper_site = mapped_source_at(&artifact.javascript, &blue_mappings, blue_helper);
    let native_user_start = native_javascript.find("function decoSite").unwrap();
    let blue_helper_start = artifact
        .javascript
        .find("var __bluetsRunInitializers")
        .unwrap();
    let blue_helper_end = artifact.javascript.find("let Example =").unwrap();
    for (javascript, mappings, start, end, compiler) in [
        (
            &native_javascript,
            &native_mappings,
            0,
            native_user_start,
            "Native",
        ),
        (
            &artifact.javascript,
            &blue_mappings,
            blue_helper_start,
            blue_helper_end,
            "BlueTSC",
        ),
    ] {
        for token in lex("<generated-helpers>", javascript).unwrap() {
            if token.kind != TokenKind::Eof && (start..end).contains(&token.start) {
                assert_eq!(
                    mapped_source_at(javascript, mappings, token.start),
                    None,
                    "{compiler}: helper-region token {:?} must be generated-only",
                    token.text
                );
            }
        }
    }
    let native_call = native_javascript.find("__esDecorate(this, null").unwrap();
    let blue_call = artifact
        .javascript
        .find("__bluetsEsDecorate(this, null")
        .unwrap();
    let native_call_site = mapped_source_at(&native_javascript, &native_mappings, native_call);
    let blue_call_site = mapped_source_at(&artifact.javascript, &blue_mappings, blue_call);
    for needle in [
        "let Example =",
        "let _instanceExtraInitializers",
        "const _metadata",
        "_method_decorators = [",
        "if (_metadata)",
        "constructor()",
    ] {
        let native_offset = native_javascript.find(needle).expect("Native scaffold");
        let blue_offset = artifact.javascript.find(needle).expect("BlueTSC scaffold");
        assert_eq!(
            mapped_source_at(&native_javascript, &native_mappings, native_offset),
            None,
            "Native {needle} is synthetic"
        );
        assert_eq!(
            mapped_source_at(&artifact.javascript, &blue_mappings, blue_offset),
            None,
            "BlueTSC {needle} must remain generated-only"
        );
    }
    let name_offset = source.find("class Example").unwrap() + "class ".len();
    let (name_line, name_column) = source_position(source, name_offset);
    let expected_name = Some((0, name_line, name_column, None));
    let (end_line, end_column) = source_position(source, name_offset + "Example".len());
    let expected_name_end = Some((0, end_line, end_column, None));
    let method_end_offset = source.find("method()").unwrap() + "method".len();
    let (method_end_line, method_end_column) = source_position(source, method_end_offset);
    let expected_method_end = Some((0, method_end_line, method_end_column, None));
    for (needle, prefix) in [
        ("let Example =", "let "),
        ("return class Example", "return class "),
    ] {
        let native_offset = native_javascript.find(needle).unwrap() + prefix.len();
        let blue_offset = artifact.javascript.find(needle).unwrap() + prefix.len();
        assert_eq!(
            mapped_source_at(&native_javascript, &native_mappings, native_offset),
            expected_name,
            "Native {needle} name anchor"
        );
        assert_eq!(
            mapped_source_at(&artifact.javascript, &blue_mappings, blue_offset),
            expected_name,
            "BlueTSC {needle} name anchor"
        );
    }
    let native_initializer = native_javascript.rfind("__runInitializers(this,").unwrap();
    let blue_initializer = artifact
        .javascript
        .rfind("__bluetsRunInitializers(this,")
        .unwrap();
    assert_eq!(
        mapped_source_at(&native_javascript, &native_mappings, native_initializer),
        expected_name
    );
    assert_eq!(
        mapped_source_at(&artifact.javascript, &blue_mappings, blue_initializer),
        expected_name,
        "synthetic constructor's initializer call uses the class name"
    );
    for (needle, prefix) in [
        ("let Example =", "let Example "),
        ("return class Example {", "return class Example "),
    ] {
        let native_offset = native_javascript.find(needle).unwrap() + prefix.len();
        let blue_offset = artifact.javascript.find(needle).unwrap() + prefix.len();
        assert_eq!(
            mapped_source_at(&native_javascript, &native_mappings, native_offset),
            expected_name_end,
            "Native {needle} end anchor"
        );
        assert_eq!(
            mapped_source_at(&artifact.javascript, &blue_mappings, blue_offset),
            expected_name_end,
            "BlueTSC {needle} end anchor"
        );
    }
    for (javascript, mappings, call) in [
        (&native_javascript, &native_mappings, native_initializer),
        (&artifact.javascript, &blue_mappings, blue_initializer),
    ] {
        let semicolon = call + javascript[call..].find(");").unwrap() + 1;
        assert_eq!(
            mapped_source_at(javascript, mappings, semicolon),
            expected_name_end,
            "initializer-call semicolon uses the class-name end"
        );
    }
    for (javascript, mappings, call) in [
        (&native_javascript, &native_mappings, native_call),
        (&artifact.javascript, &blue_mappings, blue_call),
    ] {
        let access_close = call + javascript[call..].find("}, metadata:").unwrap();
        let metadata = call + javascript[call..].find("metadata: ").unwrap();
        for offset in [access_close, metadata] {
            assert_eq!(
                mapped_source_at(javascript, mappings, offset),
                expected_method_end,
                "decorator-call tail anchors to the member-name end"
            );
        }
    }
    for (javascript, mappings) in [
        (&native_javascript, &native_mappings),
        (&artifact.javascript, &blue_mappings),
    ] {
        let signature = javascript.find("method() {").unwrap();
        for offset in [signature + "method(".len(), signature + "method() ".len()] {
            assert_eq!(
                mapped_source_at(javascript, mappings, offset),
                expected_method_end,
                "empty method close and body open use the name end"
            );
        }
    }
    let method = source_position(source, source.find("method()").unwrap());
    fs::remove_dir_all(&directory).unwrap();
    assert_eq!(
        native_decorator_site,
        Some((0, expected.0, expected.1, None))
    );
    assert_eq!(
        blue_decorator_site, native_decorator_site,
        "relocated decorator expression"
    );
    assert_eq!(
        native_helper_site, None,
        "Native helper definition is unmapped"
    );
    assert_eq!(
        blue_helper_site, native_helper_site,
        "generated helper definition must not impersonate user source"
    );
    assert_eq!(native_call_site, Some((0, method.0, method.1, None)));
    assert_eq!(
        blue_call_site, native_call_site,
        "generated helper invocation"
    );
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to pinned TypeScript 5.9.3"]
fn compacted_decorator_relocation_keeps_token_columns() {
    let tsc = PathBuf::from(
        env::var_os("BLUEICE_BLUETSC_ORACLE")
            .expect("set BLUEICE_BLUETSC_ORACLE to pinned TypeScript 5.9.3"),
    );
    let version = Command::new(&tsc).arg("--version").output().unwrap();
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8(version.stdout).unwrap().trim(),
        "Version 5.9.3"
    );
    let source = include_str!("fixtures/k11_source_maps/decorated_parenthesized.ts");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = env::temp_dir().join(format!(
        "blueice-k11-decorator-space-{}-{nonce}",
        std::process::id()
    ));
    let native = directory.join("native");
    let blueice = directory.join("blueice");
    fs::create_dir_all(&native).unwrap();
    fs::create_dir_all(&blueice).unwrap();
    let input = directory.join("main.ts");
    fs::write(&input, source).unwrap();
    let compile_native = Command::new(&tsc)
        .args([
            "--target",
            "ES2022",
            "--module",
            "ES2022",
            "--strict",
            "--pretty",
            "false",
            "--sourceMap",
            "--inlineSources",
            "--outDir",
        ])
        .arg(&native)
        .arg(&input)
        .output()
        .unwrap();
    assert!(
        compile_native.status.success(),
        "{}",
        String::from_utf8_lossy(&compile_native.stderr)
    );
    let module = "memory:///main.ts";
    let compilation = compile(
        module,
        &MapLoader::from([ModuleSource::new(module, source)]),
        CompilerOptions {
            source_map: true,
            inline_sources: true,
            ..CompilerOptions::default()
        },
    );
    assert!(!compilation.has_errors(), "{:#?}", compilation.diagnostics);
    let output = compilation.output.unwrap();
    let artifact = &output.artifacts[module];
    let native_javascript = fs::read_to_string(native.join("main.js")).unwrap();
    let native_map: Value =
        serde_json::from_str(&fs::read_to_string(native.join("main.js.map")).unwrap()).unwrap();
    let blue_map: Value =
        serde_json::from_str(&artifact.source_map.as_ref().unwrap().to_json()).unwrap();
    assert_eq!(blue_map["names"], native_map["names"]);
    assert_eq!(blue_map["sourcesContent"], native_map["sourcesContent"]);
    let node = env::var_os("BLUEICE_NODE_ORACLE").unwrap_or_else(|| "node".into());
    fs::write(blueice.join("main.js"), &artifact.javascript).unwrap();
    let native_run = Command::new(&node)
        .arg(native.join("main.js"))
        .output()
        .unwrap();
    let blue_run = Command::new(&node)
        .arg(blueice.join("main.js"))
        .output()
        .unwrap();
    assert!(
        native_run.status.success(),
        "{}",
        String::from_utf8_lossy(&native_run.stderr)
    );
    assert!(
        blue_run.status.success(),
        "{}",
        String::from_utf8_lossy(&blue_run.stderr)
    );
    assert_eq!(blue_run.stdout, native_run.stdout);
    assert_eq!(blue_run.stdout, b"42\n");

    let native_mappings = decode_map(&native_map);
    let blue_mappings = decode_map(&blue_map);
    let original = source.find("@(  decoSite  )").unwrap();
    let source_name = original + "@(  ".len();
    let expected_name = source_position(source, source_name);
    let expected_name_end = source_position(source, source_name + "decoSite".len());
    let expected_expression_end = source_position(source, original + "@(  decoSite  )".len());
    let positions = |javascript: &str| {
        let array =
            javascript.find("_method_decorators = [").unwrap() + "_method_decorators = [".len();
        let bracket = array + javascript[array..].find(']').unwrap();
        let expression = &javascript[array..bracket];
        (
            array + expression.find("decoSite").unwrap(),
            array + expression.rfind(')').unwrap(),
            bracket,
        )
    };
    let (native_name, native_close, native_bracket) = positions(&native_javascript);
    let (blue_name, blue_close, blue_bracket) = positions(&artifact.javascript);
    let native_sites = [native_name, native_close, native_bracket]
        .map(|offset| mapped_source_at(&native_javascript, &native_mappings, offset));
    let blue_sites = [blue_name, blue_close, blue_bracket]
        .map(|offset| mapped_source_at(&artifact.javascript, &blue_mappings, offset));
    fs::remove_dir_all(&directory).unwrap();
    assert_eq!(
        native_sites[0],
        Some((0, expected_name.0, expected_name.1, None))
    );
    assert_eq!(
        native_sites[1],
        Some((0, expected_name_end.0, expected_name_end.1, None))
    );
    assert_eq!(
        native_sites[2],
        Some((
            0,
            expected_expression_end.0,
            expected_expression_end.1,
            None
        ))
    );
    assert_eq!(
        blue_sites, native_sites,
        "compacted decorator token boundaries"
    );
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to pinned TypeScript 5.9.3"]
fn erased_decorator_assertion_keeps_surviving_token_columns() {
    let tsc = PathBuf::from(
        env::var_os("BLUEICE_BLUETSC_ORACLE")
            .expect("set BLUEICE_BLUETSC_ORACLE to pinned TypeScript 5.9.3"),
    );
    let version = Command::new(&tsc).arg("--version").output().unwrap();
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8(version.stdout).unwrap().trim(),
        "Version 5.9.3"
    );

    let source = include_str!("fixtures/k11_source_maps/decorated_erased_assertion.ts");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = env::temp_dir().join(format!(
        "blueice-k11-decorator-erasure-{}-{nonce}",
        std::process::id()
    ));
    let native = directory.join("native");
    let blueice = directory.join("blueice");
    fs::create_dir_all(&native).unwrap();
    fs::create_dir_all(&blueice).unwrap();
    let input = directory.join("main.ts");
    fs::write(&input, source).unwrap();
    let compile_native = Command::new(&tsc)
        .args([
            "--target",
            "ES2022",
            "--module",
            "ES2022",
            "--strict",
            "--pretty",
            "false",
            "--sourceMap",
            "--inlineSources",
            "--outDir",
        ])
        .arg(&native)
        .arg(&input)
        .output()
        .unwrap();
    assert!(
        compile_native.status.success(),
        "{}",
        String::from_utf8_lossy(&compile_native.stderr)
    );
    let module = "memory:///main.ts";
    let compilation = compile(
        module,
        &MapLoader::from([ModuleSource::new(module, source)]),
        CompilerOptions {
            source_map: true,
            inline_sources: true,
            ..CompilerOptions::default()
        },
    );
    assert!(!compilation.has_errors(), "{:#?}", compilation.diagnostics);
    let output = compilation.output.unwrap();
    let artifact = &output.artifacts[module];
    let native_javascript = fs::read_to_string(native.join("main.js")).unwrap();
    assert!(
        native_javascript.contains("_method_decorators = [decoSite];"),
        "pinned TypeScript must remove the assertion grouping"
    );
    assert!(
        artifact
            .javascript
            .contains("_method_decorators = [decoSite];"),
        "BlueTSC must not emit mapped synthetic parentheses"
    );
    let native_map: Value =
        serde_json::from_str(&fs::read_to_string(native.join("main.js.map")).unwrap()).unwrap();
    let blue_map: Value =
        serde_json::from_str(&artifact.source_map.as_ref().unwrap().to_json()).unwrap();
    assert_eq!(blue_map["names"], native_map["names"]);
    assert_eq!(blue_map["sourcesContent"], native_map["sourcesContent"]);

    let node = env::var_os("BLUEICE_NODE_ORACLE").unwrap_or_else(|| "node".into());
    fs::write(blueice.join("main.js"), &artifact.javascript).unwrap();
    let native_run = Command::new(&node)
        .arg(native.join("main.js"))
        .output()
        .unwrap();
    let blue_run = Command::new(&node)
        .arg(blueice.join("main.js"))
        .output()
        .unwrap();
    assert!(
        native_run.status.success(),
        "{}",
        String::from_utf8_lossy(&native_run.stderr)
    );
    assert!(
        blue_run.status.success(),
        "{}",
        String::from_utf8_lossy(&blue_run.stderr)
    );
    assert_eq!(blue_run.stdout, native_run.stdout);
    assert_eq!(blue_run.stdout, b"42\n");

    let native_mappings = decode_map(&native_map);
    let blue_mappings = decode_map(&blue_map);
    let expected_name = source_position(source, source.find("@(decoSite as any)").unwrap() + 2);
    let expected_end = source_position(
        source,
        source.find("@(decoSite as any)").unwrap() + "@(decoSite as any)".len(),
    );
    let positions = |javascript: &str| {
        let array =
            javascript.find("_method_decorators = [").unwrap() + "_method_decorators = [".len();
        let bracket = array + javascript[array..].find(']').unwrap();
        (
            array + javascript[array..bracket].find("decoSite").unwrap(),
            bracket,
        )
    };
    let (native_name, native_bracket) = positions(&native_javascript);
    let (blue_name, blue_bracket) = positions(&artifact.javascript);
    let native_sites = [native_name, native_bracket]
        .map(|offset| mapped_source_at(&native_javascript, &native_mappings, offset));
    let blue_sites = [blue_name, blue_bracket]
        .map(|offset| mapped_source_at(&artifact.javascript, &blue_mappings, offset));
    fs::remove_dir_all(&directory).unwrap();
    assert_eq!(
        native_sites[0],
        Some((0, expected_name.0, expected_name.1, None))
    );
    assert_eq!(
        native_sites[1],
        Some((0, expected_end.0, expected_end.1, None))
    );
    assert_eq!(
        blue_sites, native_sites,
        "surviving decorator token and expression end"
    );
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to pinned TypeScript 5.9.3"]
fn class_and_method_decorators_keep_distinct_source_anchors() {
    let tsc = PathBuf::from(
        env::var_os("BLUEICE_BLUETSC_ORACLE")
            .expect("set BLUEICE_BLUETSC_ORACLE to pinned TypeScript 5.9.3"),
    );
    let version = Command::new(&tsc).arg("--version").output().unwrap();
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8(version.stdout).unwrap().trim(),
        "Version 5.9.3"
    );

    let source = include_str!("fixtures/k11_source_maps/decorated_multiple.ts");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = env::temp_dir().join(format!(
        "blueice-k11-decorator-composition-{}-{nonce}",
        std::process::id()
    ));
    let native = directory.join("native");
    let blueice = directory.join("blueice");
    fs::create_dir_all(&native).unwrap();
    fs::create_dir_all(&blueice).unwrap();
    let input = directory.join("main.ts");
    fs::write(&input, source).unwrap();
    let compile_native = Command::new(&tsc)
        .args([
            "--target",
            "ES2022",
            "--module",
            "ES2022",
            "--strict",
            "--pretty",
            "false",
            "--sourceMap",
            "--inlineSources",
            "--outDir",
        ])
        .arg(&native)
        .arg(&input)
        .output()
        .unwrap();
    assert!(
        compile_native.status.success(),
        "{}",
        String::from_utf8_lossy(&compile_native.stderr)
    );
    let module = "memory:///main.ts";
    let compilation = compile(
        module,
        &MapLoader::from([ModuleSource::new(module, source)]),
        CompilerOptions {
            source_map: true,
            inline_sources: true,
            ..CompilerOptions::default()
        },
    );
    assert!(!compilation.has_errors(), "{:#?}", compilation.diagnostics);
    let output = compilation.output.unwrap();
    let artifact = &output.artifacts[module];
    let native_javascript = fs::read_to_string(native.join("main.js")).unwrap();
    let native_map: Value =
        serde_json::from_str(&fs::read_to_string(native.join("main.js.map")).unwrap()).unwrap();
    let blue_map: Value =
        serde_json::from_str(&artifact.source_map.as_ref().unwrap().to_json()).unwrap();
    assert_eq!(blue_map["names"], native_map["names"]);
    assert_eq!(blue_map["sourcesContent"], native_map["sourcesContent"]);

    let node = env::var_os("BLUEICE_NODE_ORACLE").unwrap_or_else(|| "node".into());
    fs::write(blueice.join("main.js"), &artifact.javascript).unwrap();
    let native_run = Command::new(&node)
        .arg(native.join("main.js"))
        .output()
        .unwrap();
    let blue_run = Command::new(&node)
        .arg(blueice.join("main.js"))
        .output()
        .unwrap();
    assert!(
        native_run.status.success(),
        "{}",
        String::from_utf8_lossy(&native_run.stderr)
    );
    assert!(
        blue_run.status.success(),
        "{}",
        String::from_utf8_lossy(&blue_run.stderr)
    );
    assert_eq!(blue_run.stdout, native_run.stdout);
    assert_eq!(blue_run.stdout, b"inner\nouter\nclass\n42\n");
    let plain = compile(
        module,
        &MapLoader::from([ModuleSource::new(module, source)]),
        CompilerOptions::default(),
    );
    assert!(!plain.has_errors(), "{:#?}", plain.diagnostics);
    assert_eq!(
        artifact.javascript,
        plain.output.unwrap().artifacts[module].javascript,
        "class source-map anchors must not change JavaScript"
    );

    let native_mappings = decode_map(&native_map);
    let blue_mappings = decode_map(&blue_map);
    let positions = |javascript: &str, helper: &str| {
        let class_value =
            javascript.find("_classDecorators = [").unwrap() + "_classDecorators = [".len();
        let method_value =
            javascript.find("_method_decorators = [").unwrap() + "_method_decorators = [".len();
        let method_bracket = method_value + javascript[method_value..].find(']').unwrap();
        [
            class_value,
            method_value,
            method_value
                + javascript[method_value..method_bracket]
                    .find("inner")
                    .unwrap(),
            method_bracket,
            javascript.find(&format!("{helper}(this, null")).unwrap(),
            javascript
                .find(&format!("{helper}(null, _classDescriptor"))
                .unwrap(),
        ]
    };
    let native_sites = positions(&native_javascript, "__esDecorate")
        .map(|offset| mapped_source_at(&native_javascript, &native_mappings, offset));
    let blue_sites = positions(&artifact.javascript, "__bluetsEsDecorate")
        .map(|offset| mapped_source_at(&artifact.javascript, &blue_mappings, offset));
    fs::remove_dir_all(&directory).unwrap();
    let expected = [
        source.find("@classDecor").unwrap() + 1,
        source.find("@outer").unwrap() + 1,
        source.find("@inner").unwrap() + 1,
        source.find("@inner").unwrap() + "@inner".len(),
        source.find("method()").unwrap(),
        source.find("class Example").unwrap(),
    ]
    .map(|offset| {
        let (line, column) = source_position(source, offset);
        Some((0, line, column, None))
    });
    assert_eq!(native_sites, expected, "pinned TypeScript source anchors");
    assert_eq!(
        blue_sites, native_sites,
        "class and member helpers must not share an anchor"
    );
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to pinned TypeScript 5.9.3"]
fn legacy_decorator_helpers_keep_generated_and_source_frames_distinct() {
    let tsc = PathBuf::from(
        env::var_os("BLUEICE_BLUETSC_ORACLE")
            .expect("set BLUEICE_BLUETSC_ORACLE to pinned TypeScript 5.9.3"),
    );
    let version = Command::new(&tsc).arg("--version").output().unwrap();
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8(version.stdout).unwrap().trim(),
        "Version 5.9.3"
    );

    let source = include_str!("fixtures/k11_source_maps/legacy_decorated.ts");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = env::temp_dir().join(format!(
        "blueice-k11-legacy-frames-{}-{nonce}",
        std::process::id()
    ));
    let native = directory.join("native");
    let blueice = directory.join("blueice");
    fs::create_dir_all(&native).unwrap();
    fs::create_dir_all(&blueice).unwrap();
    let input = directory.join("main.ts");
    fs::write(&input, source).unwrap();
    let compile_native = Command::new(&tsc)
        .args([
            "--target",
            "ES2022",
            "--module",
            "ES2022",
            "--strict",
            "--pretty",
            "false",
            "--experimentalDecorators",
            "--sourceMap",
            "--inlineSources",
            "--outDir",
        ])
        .arg(&native)
        .arg(&input)
        .output()
        .unwrap();
    assert!(
        compile_native.status.success(),
        "{}",
        String::from_utf8_lossy(&compile_native.stderr)
    );
    let module = "memory:///main.ts";
    let compilation = compile(
        module,
        &MapLoader::from([ModuleSource::new(module, source)]),
        CompilerOptions {
            experimental_decorators: true,
            source_map: true,
            inline_sources: true,
            ..CompilerOptions::default()
        },
    );
    assert!(!compilation.has_errors(), "{:#?}", compilation.diagnostics);
    let output = compilation.output.unwrap();
    let artifact = &output.artifacts[module];
    let native_javascript = fs::read_to_string(native.join("main.js")).unwrap();
    let native_map: Value =
        serde_json::from_str(&fs::read_to_string(native.join("main.js.map")).unwrap()).unwrap();
    let blue_map: Value =
        serde_json::from_str(&artifact.source_map.as_ref().unwrap().to_json()).unwrap();
    assert_eq!(blue_map["names"], native_map["names"]);
    assert_eq!(blue_map["sourcesContent"], native_map["sourcesContent"]);
    let plain = compile(
        module,
        &MapLoader::from([ModuleSource::new(module, source)]),
        CompilerOptions {
            experimental_decorators: true,
            ..CompilerOptions::default()
        },
    );
    assert!(!plain.has_errors(), "{:#?}", plain.diagnostics);
    assert_eq!(
        artifact.javascript,
        plain.output.unwrap().artifacts[module].javascript,
        "legacy source-map markers must not change JavaScript"
    );

    let node = env::var_os("BLUEICE_NODE_ORACLE").unwrap_or_else(|| "node".into());
    fs::write(blueice.join("main.js"), &artifact.javascript).unwrap();
    let native_run = Command::new(&node)
        .arg(native.join("main.js"))
        .output()
        .unwrap();
    let blue_run = Command::new(&node)
        .arg(blueice.join("main.js"))
        .output()
        .unwrap();
    assert!(
        native_run.status.success(),
        "{}",
        String::from_utf8_lossy(&native_run.stderr)
    );
    assert!(
        blue_run.status.success(),
        "{}",
        String::from_utf8_lossy(&blue_run.stderr)
    );
    assert_eq!(blue_run.stdout, native_run.stdout);
    assert_eq!(blue_run.stdout, b"method\nclass\n42\n");

    let native_mappings = decode_map(&native_map);
    let blue_mappings = decode_map(&blue_map);
    let positions = |javascript: &str, helper: &str| {
        let definition = javascript.find(&format!("var {helper}")).unwrap();
        let body = javascript.find("let Example").unwrap();
        let method_call = body + javascript[body..].find(&format!("{helper}([")).unwrap();
        let class_call = method_call
            + 1
            + javascript[method_call + 1..]
                .find(&format!("{helper}(["))
                .unwrap();
        [
            definition,
            method_call,
            method_call + javascript[method_call..].find("methodDecor").unwrap(),
            method_call + javascript[method_call..].find(']').unwrap(),
            class_call,
            class_call + javascript[class_call..].find("classDecor").unwrap(),
            class_call + javascript[class_call..].find(']').unwrap(),
        ]
    };
    let native_sites = positions(&native_javascript, "__decorate")
        .map(|offset| mapped_source_at(&native_javascript, &native_mappings, offset));
    let blue_sites = positions(&artifact.javascript, "__bluetsDecorate")
        .map(|offset| mapped_source_at(&artifact.javascript, &blue_mappings, offset));
    fs::remove_dir_all(&directory).unwrap();
    let source_site = |offset| {
        let (line, column) = source_position(source, offset);
        Some((0, line, column, None))
    };
    let expected = [
        None,
        source_site(source.find("method()").unwrap()),
        source_site(source.find("@methodDecor").unwrap() + 1),
        None,
        source_site(source.find("class Example").unwrap() + "class Example".len()),
        source_site(source.find("@classDecor").unwrap() + 1),
        None,
    ];
    assert_eq!(native_sites, expected, "pinned TypeScript legacy frames");
    assert_eq!(blue_sites, native_sites, "legacy generated and user frames");
}
