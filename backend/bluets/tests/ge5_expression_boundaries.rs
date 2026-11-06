// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Retained delimiter roles are independent of class and helper lowering.
#![allow(dead_code)]
#[path = "oracle_support/ge5_cli.rs"]
mod cli;
#[path = "oracle_support/gd2_maps.rs"]
mod decoded;
#[path = "oracle_support/ge5_inputs.rs"]
mod inputs;
#[path = "oracle_support/project_paths.rs"]
mod project_paths;
use serde_json::{json, Value};

fn fixture(id: &str) -> (String, Value, String, Value) {
    let specification = inputs::input()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == id)
        .unwrap();
    let row = inputs::resolve(specification);
    let expected = inputs::reference()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == id)
        .unwrap();
    let temporary = cli::Temporary::new("expression-boundaries");
    let actual = cli::observe_builds(&temporary, 0, &row, 2);
    assert_eq!(actual.diagnostics, json!([]));
    (
        inputs::blob(expected["outputs"]["out/main.js"].as_str().unwrap()).to_string(),
        decoded::decoded(inputs::blob(
            expected["outputs"]["out/main.js.map"].as_str().unwrap(),
        )),
        actual.outputs["out/main.js"].clone(),
        decoded::decoded(&actual.outputs["out/main.js.map"]),
    )
}

fn owner(code: &str, map: &Value, offset: usize) -> Option<Value> {
    let prefix = &code[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let column = prefix.rsplit('\n').next().unwrap().encode_utf16().count();
    map["segments"]
        .as_array()
        .unwrap()
        .iter()
        .rfind(|segment| segment[0] == line && segment[1].as_u64().unwrap() as usize <= column)
        .filter(|segment| segment.as_array().unwrap().len() > 2)
        .map(|segment| json!(&segment.as_array().unwrap()[2..]))
}

#[test]
fn new_callee_group_keeps_its_own_original_opening() {
    for target in ["es2015", "es2022"] {
        for module in ["commonjs", "esnext"] {
            for variant in ["original", "crlfAstral"] {
                let id = format!("ge1_downlevel/computed-constructor-spread-{target}-{module}-assign-iterate/{variant}/external");
                let (native, expected, actual, map) = fixture(&id);
                let native_open = native.find("new (").unwrap() + "new ".len();
                let actual_open = actual.find("new (").unwrap() + "new ".len();
                let native_owner = owner(&native, &expected, native_open);
                assert!(native_owner.is_some());
                assert_eq!(
                    owner(&actual, &map, actual_open),
                    native_owner,
                    "{id}: constructor callee grouping"
                );
            }
        }
    }
}

#[test]
fn named_import_brace_starts_its_own_source_range() {
    for target in ["es2015", "es2022"] {
        for variant in ["original", "crlfAstral"] {
            let id = format!("ge1_downlevel/called-imported-heritage-{target}-esnext-assign-iterate/{variant}/external");
            let (native, expected, actual, map) = fixture(&id);
            let first_brace = |code: &str| {
                let tokens = blueice_bluets::lex("<emitted>", code).unwrap();
                tokens
                    .windows(2)
                    .find(|pair| pair[0].is("import") && pair[1].is("{"))
                    .unwrap()[1]
                    .start
            };
            let native_owner = owner(&native, &expected, first_brace(&native));
            assert!(native_owner.is_some());
            assert_eq!(
                owner(&actual, &map, first_brace(&actual)),
                native_owner,
                "{id}: named import opening"
            );
            let import_end = |code: &str| {
                blueice_bluets::lex("<emitted>", code)
                    .unwrap()
                    .into_iter()
                    .find(|token| token.is("import"))
                    .unwrap()
                    .end
            };
            let gap_owner = owner(&native, &expected, import_end(&native));
            for offset in import_end(&actual)..first_brace(&actual) {
                assert_eq!(
                    owner(&actual, &map, offset),
                    gap_owner,
                    "{id}: import keyword trailing gap"
                );
            }
        }
    }
}

#[test]
fn named_import_from_keyword_keeps_the_clause_completion() {
    for target in ["es2015", "es2022"] {
        for variant in ["original", "crlfAstral"] {
            let id = format!("ge1_downlevel/called-imported-heritage-{target}-esnext-assign-iterate/{variant}/external");
            let (native, expected, actual, map) = fixture(&id);
            let ranges = |code: &str| {
                let tokens = blueice_bluets::lex("<emitted>", code).unwrap();
                let index = tokens.iter().position(|token| token.is("from")).unwrap();
                (tokens[index].start, tokens[index + 1].start)
            };
            let (native_start, native_end) = ranges(&native);
            let (actual_start, actual_end) = ranges(&actual);
            let native_owner = owner(&native, &expected, native_start);
            assert!(native_owner.is_some());
            for offset in native_start..native_end {
                assert_eq!(owner(&native, &expected, offset), native_owner);
            }
            for offset in actual_start..actual_end {
                assert_eq!(
                    owner(&actual, &map, offset),
                    native_owner,
                    "{id}: from keyword and following gap"
                );
            }
        }
    }
}

#[test]
fn comments_attached_to_erased_type_imports_do_not_acquire_source_owners() {
    for module in ["commonjs", "esnext"] {
        for mode in ["regular", "isolated"] {
            let id = format!("gd2_declaration_maps/cross-module-types-{module}-runtime-{mode}-accept/crlfAstral/external");
            let (native, _, actual, map) = fixture(&id);
            let comment = "/* G-E5 🧭 */";
            assert!(!native.contains(comment));
            let begin = actual.find(comment).unwrap();
            for (unit, _) in comment.char_indices() {
                assert_eq!(
                    owner(&actual, &map, begin + unit),
                    None,
                    "{id}: an erased declaration's retained comment is generated trivia"
                );
            }
        }
    }
}
