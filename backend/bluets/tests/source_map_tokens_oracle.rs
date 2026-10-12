// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! K.11: compare decoded source-map mapping sets, not encoded VLQ text.

use blueice_bluets::{compile, CompilerOptions, MapLoader, ModuleSource};
use serde_json::Value;
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const MATRIX: &str = include_str!("fixtures/k11_source_maps/source-map-tokens-checker-matrix.tsv");

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
    for row in MATRIX.lines().skip(1) {
        let fields = row.split('\t').collect::<Vec<_>>();
        assert_eq!(fields.len(), 3, "invalid matrix row: {row}");
        let (name, verdict, file) = (fields[0], fields[1], fields[2]);
        assert_eq!(verdict, "accept", "this baseline has only accepted forms");
        let source = fs::read_to_string(corpus.join(file)).unwrap();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory =
            env::temp_dir().join(format!("blueice-k11-{name}-{}-{nonce}", std::process::id()));
        let native = directory.join("native");
        let blueice = directory.join("blueice");
        fs::create_dir_all(&native).unwrap();
        fs::create_dir_all(&blueice).unwrap();
        let input = directory.join("main.ts");
        fs::write(&input, &source).unwrap();
        let output = Command::new(&tsc)
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
            output.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let module = "memory:///main.ts";
        let compilation = compile(
            module,
            &MapLoader::from([ModuleSource::new(module, source.clone())]),
            CompilerOptions {
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
        assert_eq!(blue_map["names"], native_map["names"], "{name}: map names");
        assert_eq!(
            blue_map["sourcesContent"], native_map["sourcesContent"],
            "{name}: inline source"
        );

        fs::write(native.join("package.json"), "{\"type\":\"module\"}\n").unwrap();
        fs::write(blueice.join("package.json"), "{\"type\":\"module\"}\n").unwrap();
        fs::write(blueice.join("main.js"), &artifact.javascript).unwrap();
        let node = env::var_os("BLUEICE_NODE_ORACLE").unwrap_or_else(|| "node".into());
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
            "{name}: native execution failed: {}",
            String::from_utf8_lossy(&native_run.stderr)
        );
        assert!(
            blue_run.status.success(),
            "{name}: BlueTSC execution failed: {}",
            String::from_utf8_lossy(&blue_run.stderr)
        );
        assert_eq!(blue_run.stdout, native_run.stdout, "{name}: runtime output");

        let native_mappings = decode_mappings(native_map["mappings"].as_str().unwrap());
        let blue_mappings = decode_mappings(blue_map["mappings"].as_str().unwrap());
        let native_javascript = fs::read_to_string(native.join("main.js")).unwrap();
        let native_tokens = mapped_tokens(&native_javascript, &native_mappings);
        let blue_tokens = mapped_tokens(&artifact.javascript, &blue_mappings);
        let native_spellings = native_tokens
            .iter()
            .map(|token| token.token.as_str())
            .collect::<Vec<_>>();
        let blue_spellings = blue_tokens
            .iter()
            .map(|token| token.token.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            blue_spellings, native_spellings,
            "{name}: emitted token stream"
        );
        if native_tokens != blue_tokens {
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

    let native_mappings = decode_mappings(native_map["mappings"].as_str().unwrap());
    let blue_mappings = decode_mappings(blue_map["mappings"].as_str().unwrap());
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
    let native_call = native_javascript.find("__esDecorate(this, null").unwrap();
    let blue_call = artifact
        .javascript
        .find("__bluetsEsDecorate(this, null")
        .unwrap();
    let native_call_site = mapped_source_at(&native_javascript, &native_mappings, native_call);
    let blue_call_site = mapped_source_at(&artifact.javascript, &blue_mappings, blue_call);
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

    let native_mappings = decode_mappings(native_map["mappings"].as_str().unwrap());
    let blue_mappings = decode_mappings(blue_map["mappings"].as_str().unwrap());
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

    let native_mappings = decode_mappings(native_map["mappings"].as_str().unwrap());
    let blue_mappings = decode_mappings(blue_map["mappings"].as_str().unwrap());
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

    let native_mappings = decode_mappings(native_map["mappings"].as_str().unwrap());
    let blue_mappings = decode_mappings(blue_map["mappings"].as_str().unwrap());
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

    let native_mappings = decode_mappings(native_map["mappings"].as_str().unwrap());
    let blue_mappings = decode_mappings(blue_map["mappings"].as_str().unwrap());
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
            class_call,
            class_call + javascript[class_call..].find("classDecor").unwrap(),
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
        source_site(source.find("class Example").unwrap() + "class Example".len()),
        source_site(source.find("@classDecor").unwrap() + 1),
    ];
    assert_eq!(native_sites, expected, "pinned TypeScript legacy frames");
    assert_eq!(blue_sites, native_sites, "legacy generated and user frames");
}

fn source_position(text: &str, offset: usize) -> (i64, i64) {
    let prefix = &text[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() as i64;
    let column = prefix.rsplit('\n').next().unwrap().encode_utf16().count() as i64;
    (line, column)
}

fn mapped_source_at(
    javascript: &str,
    mappings: &BTreeSet<Mapping>,
    offset: usize,
) -> Option<(i64, i64, i64, Option<i64>)> {
    let (line, column) = source_position(javascript, offset);
    mappings
        .iter()
        .rfind(|mapping| {
            mapping.generated_line == line as usize && mapping.generated_column <= column
        })
        .filter(|mapping| mapping.source_index >= 0)
        .map(|mapping| {
            (
                mapping.source_index,
                mapping.source_line,
                mapping.source_column,
                mapping.name_index,
            )
        })
}

fn decode_mappings(encoded: &str) -> BTreeSet<Mapping> {
    let mut mappings = BTreeSet::new();
    let mut source_index = 0i64;
    let mut source_line = 0i64;
    let mut source_column = 0i64;
    let mut name_index = 0i64;
    for (generated_line, line) in encoded.split(';').enumerate() {
        let mut generated_column = 0i64;
        for segment in line.split(',').filter(|segment| !segment.is_empty()) {
            let fields = decode_vlq(segment);
            assert!(
                matches!(fields.len(), 1 | 4 | 5),
                "invalid segment: {segment}"
            );
            generated_column += fields[0];
            if fields.len() == 1 {
                mappings.insert(Mapping {
                    generated_line,
                    generated_column,
                    source_index: -1,
                    source_line: -1,
                    source_column: -1,
                    name_index: None,
                });
                continue;
            }
            source_index += fields[1];
            source_line += fields[2];
            source_column += fields[3];
            let name = fields.get(4).map(|delta| {
                name_index += delta;
                name_index
            });
            mappings.insert(Mapping {
                generated_line,
                generated_column,
                source_index,
                source_line,
                source_column,
                name_index: name,
            });
        }
    }
    mappings
}

/// Align equivalent emitted tokens so formatting and map-URL comments do not
/// turn a source-position comparison into a generated-whitespace comparison.
/// The K.11 baseline fixtures deliberately use only ASCII identifiers,
/// numbers, punctuation and line comments; broaden this scanner with the
/// corpus when more emitted token forms join the oracle.
fn mapped_tokens(javascript: &str, mappings: &BTreeSet<Mapping>) -> Vec<TokenMapping> {
    let bytes = javascript.as_bytes();
    let mut tokens = Vec::new();
    let (mut index, mut line, mut column) = (0usize, 0usize, 0i64);
    while index < bytes.len() {
        if bytes[index] == b'\n' {
            index += 1;
            line += 1;
            column = 0;
            continue;
        }
        if matches!(bytes[index], b' ' | b'\t' | b'\r') {
            index += 1;
            column += 1;
            continue;
        }
        if bytes[index..].starts_with(b"//") {
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
                column += 1;
            }
            continue;
        }
        let start = index;
        let start_column = column;
        if bytes[index].is_ascii_alphanumeric() || matches!(bytes[index], b'_' | b'$') {
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || matches!(bytes[index], b'_' | b'$'))
            {
                index += 1;
                column += 1;
            }
        } else {
            assert!(bytes[index].is_ascii(), "extend the K.11 token scanner");
            index += 1;
            column += 1;
        }
        let mapping = mappings
            .iter()
            .rfind(|mapping| {
                mapping.generated_line == line && mapping.generated_column <= start_column
            })
            .unwrap_or_else(|| panic!("missing generated mapping at {line}:{start_column}"));
        tokens.push(TokenMapping {
            token: String::from_utf8(bytes[start..index].to_vec()).unwrap(),
            source_index: mapping.source_index,
            source_line: mapping.source_line,
            source_column: mapping.source_column,
            name_index: mapping.name_index,
        });
    }
    tokens
}

fn decode_vlq(encoded: &str) -> Vec<i64> {
    let mut fields = Vec::new();
    let mut value = 0u64;
    let mut shift = 0u32;
    for byte in encoded.bytes() {
        let digit = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => panic!("invalid base64 VLQ digit"),
        };
        value |= u64::from(digit & 0b1_1111) << shift;
        shift += 5;
        if digit & 0b10_0000 == 0 {
            let magnitude = (value >> 1) as i64;
            fields.push(if value & 1 == 1 {
                -magnitude
            } else {
                magnitude
            });
            value = 0;
            shift = 0;
        }
    }
    assert_eq!(shift, 0, "unterminated VLQ field");
    fields
}
