// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public compilation keeps full-source relocation coordinates inside rendered classes.
use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};
use std::process::Command;

fn compile_case(astral: bool, source_map: bool) {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/ge3_decorator_compositions/cases.json");
    let cases: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture).unwrap()).unwrap();
    let row = cases["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "ge3-decorators-literal-fields-standard-es5-commonjs-assign")
        .unwrap();
    let original = row["files"]["main.ts"].as_str().unwrap();
    let source = if astral {
        format!("/* G-E5 🧭 */ {}", original.replace('\n', "\r\n"))
    } else {
        original.to_string()
    };
    let id = "memory:///main.ts";
    let result = compile(
        id,
        &MapLoader::from([ModuleSource::new(id, &source)]),
        CompilerOptions {
            target: EcmaTarget::Es5,
            module_kind: ModuleKind::CommonJs,
            experimental_decorators: false,
            use_define_for_class_fields: Some(false),
            source_map,
            inline_sources: source_map,
            libraries: Some(vec![EcmaTarget::Es2022]),
            ..CompilerOptions::default()
        },
    );
    assert!(
        !result.has_errors(),
        "astral={astral} maps={source_map}: {:?}",
        result.diagnostics
    );
    let artifact = &result.output.unwrap().artifacts[id];
    assert_eq!(artifact.source_map.is_some(), source_map);
    if let Some(map) = &artifact.source_map {
        let code = &artifact.javascript;
        let position = |text: &str, offset| {
            let before = &text[..offset];
            (
                before.bytes().filter(|byte| *byte == b'\n').count(),
                before.rsplit('\n').next().unwrap().encode_utf16().count(),
            )
        };
        let owner = |offset| {
            let (line, column) = position(code, offset);
            map.original_position_at(code, line, column, 100_000, 32_000_000)
                .unwrap()
                .map(|owner| (owner.line, owner.column_utf16))
        };
        let tokens = blueice_bluets::lex("generated.js", code).unwrap();
        let class = source.find("export class C").unwrap();
        // These instance stores carry full-module V ranges. The static store
        // uses the separate, existing plain-text initializer lowering.
        for (key, value) in [("'a-b'", "1"), ("1.5", "2")] {
            let key_end = class + source[class..].find(key).unwrap() + key.len();
            let initializer = key_end + source[key_end..].find("= ").unwrap() + 2;
            assert_eq!(&source[initializer..initializer + value.len()], value);
            let expected = Some(position(&source, initializer));
            let index = tokens.iter().position(|token| token.text == value && owner(token.start) == expected)
                .unwrap_or_else(|| panic!("the relocated {key} initializer must retain its complete original source role"));
            let token = &tokens[index];
            for offset in token.start..token.end {
                assert_eq!(owner(offset), expected);
            }
            let helper = tokens[..index]
                .iter()
                .rfind(|token| token.text.ends_with("RunInitializers"))
                .unwrap();
            assert!(
                helper.end < token.start,
                "the retained operand belongs to its initializer helper"
            );
            assert_ne!(
                owner(token.end), expected,
                "the copied operand's starting owner must not leak into its generated helper boundary"
            );
        }
    }
    let execution = Command::new("node")
        .arg("-e")
        .arg(format!(
            "{}\nprocess.stdout.write(JSON.stringify(exports.result.slice(0,3)));",
            artifact.javascript
        ))
        .output()
        .unwrap();
    assert!(
        execution.status.success(),
        "{}",
        String::from_utf8_lossy(&execution.stderr)
    );
    assert_eq!(
        String::from_utf8(execution.stdout).unwrap(),
        "[2,3,4]",
        "astral={astral} maps={source_map}"
    );
}

#[test]
fn standard_literal_fields_original_without_maps() {
    compile_case(false, false);
}

#[test]
fn standard_literal_fields_original_with_maps() {
    compile_case(false, true);
}

#[test]
fn standard_literal_fields_crlf_astral_without_maps() {
    compile_case(true, false);
}

#[test]
fn standard_literal_fields_crlf_astral_with_maps() {
    compile_case(true, true);
}
