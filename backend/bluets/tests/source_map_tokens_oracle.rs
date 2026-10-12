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
                matches!(fields.len(), 4 | 5),
                "invalid mapped segment: {segment}"
            );
            generated_column += fields[0];
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
