// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Generated wrapper/helper code has no authority to claim user coordinates.
use super::{decoded, inputs};
use blueice_bluets::{
    compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource, SourceMap,
};
use serde_json::Value;

fn emitted(source: &str, target: EcmaTarget) -> (String, SourceMap) {
    emitted_module(source, target, ModuleKind::CommonJs)
}

fn emitted_module(
    source: &str,
    target: EcmaTarget,
    module_kind: ModuleKind,
) -> (String, SourceMap) {
    let id = "memory:///main.ts";
    let result = compile(
        id,
        &MapLoader::from([ModuleSource::new(id, source)]),
        CompilerOptions {
            target,
            module_kind,
            libraries: Some(vec![EcmaTarget::Es2022]),
            downlevel_iteration: true,
            use_define_for_class_fields: Some(false),
            source_map: true,
            inline_sources: true,
            ..CompilerOptions::default()
        },
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let artifact = result
        .output
        .unwrap()
        .artifacts
        .into_values()
        .next()
        .unwrap();
    (artifact.javascript, artifact.source_map.unwrap())
}

fn position(text: &str, offset: usize) -> (usize, usize) {
    let prefix = &text[..offset];
    (
        prefix.bytes().filter(|byte| *byte == b'\n').count(),
        prefix.rsplit('\n').next().unwrap().encode_utf16().count(),
    )
}

fn native_mapped_at(map: &Value, generated: &str, offset: usize) -> bool {
    let (line, column) = position(generated, offset);
    map["segments"]
        .as_array()
        .unwrap()
        .iter()
        .rfind(|segment| {
            segment[0].as_u64().unwrap() as usize == line
                && segment[1].as_u64().unwrap() as usize <= column
        })
        .is_some_and(|segment| segment.as_array().unwrap().len() > 2)
}

#[test]
fn commonjs_prologue_cannot_acquire_user_coordinates() {
    let source = inputs::input()["regressions"]["function-body-call"]
        .as_str()
        .unwrap();
    let native = inputs::reference()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "regression/function-body-call/CommonJS/lf/external")
        .unwrap();
    let native_js = inputs::blob(native["outputs"]["out/main.js"].as_str().unwrap());
    let native_map = decoded::decoded(inputs::blob(
        native["outputs"]["out/main.js.map"].as_str().unwrap(),
    ));
    let (javascript, map) = emitted(source, EcmaTarget::Es2022);
    for token in ["\"use strict\"", "Object.defineProperty"] {
        let offset = native_js.find(token).unwrap();
        assert!(
            !native_mapped_at(&native_map, native_js, offset),
            "Native {token} must be generated-only"
        );
        let (line, column) = position(&javascript, javascript.find(token).unwrap());
        assert_eq!(
            map.original_position_at(&javascript, line, column, 100_000, 32_000_000)
                .unwrap(),
            None,
            "{token}: generated CommonJS metadata cannot claim user source"
        );
    }
}

#[test]
fn downlevel_generator_map_has_only_bounded_original_coordinates() {
    let original = inputs::input()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| {
            row["origin"]["suite"] == "ge1_downlevel"
                && row["origin"]["id"] == "opaque-generator-es5-es5-commonjs-assign-iterate"
                && row["variant"] == "original"
                && row["profile"] == "external"
        })
        .unwrap();
    let row = inputs::resolve(original);
    let (javascript, map) = emitted(row["files"]["main.ts"].as_str().unwrap(), EcmaTarget::Es5);
    assert!(
        map.original_position_at(&javascript, 0, 0, 100_000, 32_000_000)
            .is_ok(),
        "every decoded segment, including generated helper tails, must stay inside original source"
    );
}

#[test]
fn object_spread_helper_definition_is_generated_only() {
    let source = "const original = { value: 42 }; export const copy = { ...original };\n";
    let (javascript, map) = emitted(source, EcmaTarget::Es5);
    let marker = "function __blueice_target_object_properties_";
    let offset = javascript
        .find(marker)
        .expect("selected object spread helper definition");
    let (line, column) = position(&javascript, offset);
    assert_eq!(
        map.original_position_at(&javascript, line, column, 100_000, 32_000_000)
            .unwrap(),
        None,
        "helper definitions have no user-source origin"
    );
}

fn generator_artifacts() -> (String, &'static str, Value, String, SourceMap) {
    let specification = inputs::input()["cases"].as_array().unwrap().iter().find(|row| {
        row["id"] == "ge1_downlevel/opaque-generator-es5-es5-commonjs-assign-iterate/original/external"
    }).unwrap();
    let row = inputs::resolve(specification);
    let native = inputs::reference()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == specification["id"])
        .unwrap();
    let source = row["files"]["main.ts"].as_str().unwrap().to_owned();
    let native_js = inputs::blob(native["outputs"]["out/main.js"].as_str().unwrap());
    let native_map = decoded::decoded(inputs::blob(
        native["outputs"]["out/main.js.map"].as_str().unwrap(),
    ));
    let (javascript, map) = emitted(&source, EcmaTarget::Es5);
    (source, native_js, native_map, javascript, map)
}

#[test]
fn generator_continuation_dispatch_is_generated_only() {
    let (_, native_js, native_map, javascript, map) = generator_artifacts();
    let native_offset = native_js.find("switch (_a.label)").unwrap();
    assert!(!native_mapped_at(&native_map, native_js, native_offset));
    let offset = javascript
        .find("switch (__blueice_target_generator_context_")
        .unwrap();
    let (line, column) = position(&javascript, offset);
    assert_eq!(
        map.original_position_at(&javascript, line, column, 100_000, 32_000_000)
            .unwrap(),
        None,
        "the generated continuation dispatcher cannot claim the original function declaration"
    );
}

#[test]
fn generator_name_and_suspension_operand_keep_their_original_tokens() {
    let (source, native_js, native_map, javascript, map) = generator_artifacts();
    for (token, native_offset, offset) in [
        (
            "sequence",
            native_js.find("function sequence").unwrap() + "function ".len(),
            javascript.find("function sequence").unwrap() + "function ".len(),
        ),
        (
            "42",
            native_js.rfind("42").unwrap(),
            javascript.rfind("42").unwrap(),
        ),
    ] {
        let original = position(&source, source.find(token).unwrap());
        let (native_line, native_column) = position(native_js, native_offset);
        let segment = native_map["segments"]
            .as_array()
            .unwrap()
            .iter()
            .rfind(|segment| {
                segment[0].as_u64().unwrap() as usize == native_line
                    && segment[1].as_u64().unwrap() as usize <= native_column
            })
            .unwrap();
        assert_eq!(
            (
                segment[3].as_u64().unwrap() as usize,
                segment[4].as_u64().unwrap() as usize
            ),
            original
        );
        let (line, column) = position(&javascript, offset);
        let actual = map
            .original_position_at(&javascript, line, column, 100_000, 32_000_000)
            .unwrap()
            .expect("retained original token");
        assert_eq!(
            (actual.line, actual.column_utf16),
            original,
            "retained generator token {token}"
        );
    }
}

#[test]
fn named_async_es2015_retains_await_operands_and_excludes_the_constructor_guard() {
    for variant in ["original", "crlfAstral"] {
        let id = format!(
            "ge1_downlevel/conditional-await-es2015-commonjs-assign-iterate/{variant}/external"
        );
        let specification = inputs::input()["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .unwrap();
        let row = inputs::resolve(specification);
        let native = inputs::reference()["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .unwrap();
        let native_js = inputs::blob(native["outputs"]["out/main.js"].as_str().unwrap());
        let native_map = decoded::decoded(inputs::blob(
            native["outputs"]["out/main.js.map"].as_str().unwrap(),
        ));
        let (javascript, map) = emitted(
            row["files"]["main.ts"].as_str().unwrap(),
            EcmaTarget::Es2015,
        );
        let first = javascript.find("function calculate").unwrap();
        let native_first = native_js.find("function calculate").unwrap();
        let actual_calls = javascript
            .match_indices("Promise.resolve")
            .filter(|(offset, _)| *offset > first)
            .collect::<Vec<_>>();
        let native_calls = native_js
            .match_indices("Promise.resolve")
            .filter(|(offset, _)| *offset > native_first)
            .collect::<Vec<_>>();
        assert_eq!(actual_calls.len(), 3);
        assert_eq!(native_calls.len(), 3);
        for ((offset, spelling), (native_offset, _)) in actual_calls.into_iter().zip(native_calls) {
            for character in 0..spelling.len() {
                let (line, column) = position(&javascript, offset + character);
                let actual = map
                    .original_position_at(&javascript, line, column, 100_000, 32_000_000)
                    .unwrap()
                    .expect("retained await operand");
                let (native_line, native_column) = position(native_js, native_offset + character);
                let expected = native_map["segments"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .rfind(|segment| {
                        segment[0].as_u64().unwrap() as usize == native_line
                            && segment[1].as_u64().unwrap() as usize <= native_column
                    })
                    .unwrap();
                assert_eq!(
                    (actual.line, actual.column_utf16),
                    (
                        expected[3].as_u64().unwrap() as usize,
                        expected[4].as_u64().unwrap() as usize
                    ),
                    "{id}: operand at generated {line}:{column}"
                );
            }
        }
        let guard = javascript.find("if (new.target)").unwrap();
        for offset in guard..guard + "if (new.target)".len() {
            let (line, column) = position(&javascript, offset);
            assert_eq!(
                map.original_position_at(&javascript, line, column, 100_000, 32_000_000)
                    .unwrap(),
                None
            );
        }
    }
}

#[test]
fn es5_async_resume_binding_and_return_opcode_keep_distinct_native_origins() {
    for variant in ["original", "crlfAstral"] {
        let id = format!(
            "ge1_downlevel/conditional-await-es5-commonjs-assign-iterate/{variant}/external"
        );
        let specification = inputs::input()["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .unwrap();
        let row = inputs::resolve(specification);
        let native = inputs::reference()["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .unwrap();
        let native_js = inputs::blob(native["outputs"]["out/main.js"].as_str().unwrap());
        let native_map = decoded::decoded(inputs::blob(
            native["outputs"]["out/main.js.map"].as_str().unwrap(),
        ));
        let (javascript, map) = emitted(row["files"]["main.ts"].as_str().unwrap(), EcmaTarget::Es5);
        let emitted_tokens = blueice_bluets::lex("<emitted>", &javascript).unwrap();
        let actual_binding = emitted_tokens
            .windows(7)
            .find(|tokens| {
                tokens[0].text == "value"
                    && tokens[1].text == "="
                    && tokens[2]
                        .text
                        .starts_with("__blueice_target_generator_context_")
                    && tokens[3].text == "."
                    && tokens[4].text == "sent"
                    && tokens[5].text == "("
                    && tokens[6].text == ")"
            })
            .unwrap()[0]
            .start;
        let native_binding = native_js.find("value = _b.sent()").unwrap();
        let actual_return = javascript
            .rfind("return {kind:\"return\", value:(")
            .unwrap();
        let native_return = native_js.rfind("return [2 /*return*/").unwrap();
        for (label, actual, expected) in [
            ("resume binding", actual_binding, native_binding),
            ("return opcode", actual_return, native_return),
        ] {
            let (line, column) = position(&javascript, actual);
            let original = map
                .original_position_at(&javascript, line, column, 100_000, 32_000_000)
                .unwrap()
                .expect(label);
            let (line, column) = position(native_js, expected);
            let expected = native_map["segments"]
                .as_array()
                .unwrap()
                .iter()
                .rfind(|segment| {
                    segment[0] == line && segment[1].as_u64().unwrap() as usize <= column
                })
                .unwrap();
            assert_eq!(
                (original.line, original.column_utf16),
                (
                    expected[3].as_u64().unwrap() as usize,
                    expected[4].as_u64().unwrap() as usize
                ),
                "{label}"
            );
        }
    }
}

#[test]
fn es5_template_await_interpolation_preserves_the_original_operand() {
    for variant in ["original", "crlfAstral"] {
        let id = format!(
            "ge1_downlevel/template-await-order-es5-commonjs-assign-iterate/{variant}/external"
        );
        let specification = inputs::input()["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .unwrap();
        let row = inputs::resolve(specification);
        let native = inputs::reference()["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .unwrap();
        let native_js = inputs::blob(native["outputs"]["out/main.js"].as_str().unwrap());
        let native_map = decoded::decoded(inputs::blob(
            native["outputs"]["out/main.js.map"].as_str().unwrap(),
        ));
        let (javascript, map) = emitted(row["files"]["main.ts"].as_str().unwrap(), EcmaTarget::Es5);
        let marker = "return {kind:\"yield\", value:(";
        let endpoints = javascript
            .rmatch_indices(marker)
            .map(|(offset, _)| {
                let start = offset + marker.len();
                start + javascript[start..].find(")};").unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(endpoints.len(), 2);
        for (index, spelling) in ["Promise.resolve(\"x\")", "Promise.resolve(\"y\")"]
            .into_iter()
            .enumerate()
        {
            let actual = javascript.find(spelling).expect("retained await operand");
            let expected = native_js.find(spelling).expect("Native await operand");
            // The physical temporary-assignment semicolon is generated-only.
            // The logical operand endpoint moves to its suspension opcode.
            let positions = (0..spelling.len())
                .map(|character| (actual + character, expected + character))
                .chain(std::iter::once((
                    endpoints[index],
                    expected + spelling.len(),
                )));
            for (actual, expected) in positions {
                let (line, column) = position(&javascript, actual);
                let original = map
                    .original_position_at(&javascript, line, column, 100_000, 32_000_000)
                    .unwrap()
                    .expect("mapped interpolation");
                let (line, column) = position(native_js, expected);
                let expected = native_map["segments"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .rfind(|segment| {
                        segment[0] == line && segment[1].as_u64().unwrap() as usize <= column
                    })
                    .unwrap();
                assert_eq!(
                    (original.line, original.column_utf16),
                    (
                        expected[3].as_u64().unwrap() as usize,
                        expected[4].as_u64().unwrap() as usize
                    ),
                    "{spelling}: generated byte {actual}"
                );
            }
        }
    }
}

#[test]
fn es5_generator_retains_switch_without_suspension_as_original_syntax() {
    for variant in ["original", "crlfAstral"] {
        let id = format!(
            "ge1_downlevel/opaque-generator-es5-es5-commonjs-assign-iterate/{variant}/external"
        );
        let specification = inputs::input()["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .unwrap();
        let row = inputs::resolve(specification);
        let native = inputs::reference()["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .unwrap();
        let native_js = inputs::blob(native["outputs"]["out/main.js"].as_str().unwrap());
        let native_map = decoded::decoded(inputs::blob(
            native["outputs"]["out/main.js.map"].as_str().unwrap(),
        ));
        let (javascript, map) = emitted(row["files"]["main.ts"].as_str().unwrap(), EcmaTarget::Es5);
        let actual = javascript
            .find("switch (1)")
            .expect("non-suspending switch must retain its original syntax");
        let expected = native_js.find("switch (1)").unwrap();
        for character in 0.."switch (1)".len() {
            let (line, column) = position(&javascript, actual + character);
            let original = map
                .original_position_at(&javascript, line, column, 100_000, 32_000_000)
                .unwrap()
                .expect("retained switch");
            let (line, column) = position(native_js, expected + character);
            let expected = native_map["segments"]
                .as_array()
                .unwrap()
                .iter()
                .rfind(|segment| {
                    segment[0] == line && segment[1].as_u64().unwrap() as usize <= column
                })
                .unwrap();
            assert_eq!(
                (original.line, original.column_utf16),
                (
                    expected[3].as_u64().unwrap() as usize,
                    expected[4].as_u64().unwrap() as usize
                ),
                "{variant}: switch UTF-16 character {character}"
            );
        }
    }
}

#[test]
fn es5_binary_suspension_operators_preserve_native_expression_owners() {
    for variant in ["original", "crlfAstral"] {
        let id = format!(
            "ge1_downlevel/binary-yield-order-es5-esnext-assign-iterate/{variant}/external"
        );
        let specification = inputs::input()["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .unwrap();
        let row = inputs::resolve(specification);
        let native = inputs::reference()["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .unwrap();
        let native_js = inputs::blob(native["outputs"]["out/main.js"].as_str().unwrap());
        let native_map = decoded::decoded(inputs::blob(
            native["outputs"]["out/main.js.map"].as_str().unwrap(),
        ));
        let (javascript, map) = emitted(row["files"]["main.ts"].as_str().unwrap(), EcmaTarget::Es5);
        let actual_tokens = blueice_bluets::lex("<actual>", &javascript).unwrap();
        let native_tokens = blueice_bluets::lex("<native>", native_js).unwrap();
        let actual_start = javascript.find("function sequence").unwrap();
        let native_start = native_js.find("function sequence").unwrap();
        for operator in ["+", "*"] {
            let actual = actual_tokens
                .iter()
                .find(|token| token.start > actual_start && token.text == operator)
                .unwrap();
            let expected = native_tokens
                .iter()
                .find(|token| token.start > native_start && token.text == operator)
                .unwrap();
            let (line, column) = position(&javascript, actual.start);
            let original = map
                .original_position_at(&javascript, line, column, 100_000, 32_000_000)
                .unwrap();
            let (line, column) = position(native_js, expected.start);
            let expected = native_map["segments"]
                .as_array()
                .unwrap()
                .iter()
                .rfind(|segment| {
                    segment[0] == line && segment[1].as_u64().unwrap() as usize <= column
                })
                .unwrap();
            assert_eq!(
                original.map(|position| (position.line, position.column_utf16)),
                Some((
                    expected[3].as_u64().unwrap() as usize,
                    expected[4].as_u64().unwrap() as usize
                )),
                "{variant}: {operator}"
            );
        }
    }
}

#[test]
fn es5_logical_suspension_initializers_keep_the_complete_declaration_endpoint() {
    for variant in ["original", "crlfAstral"] {
        let id = format!(
            "ge1_downlevel/logical-yield-order-es5-esnext-assign-iterate/{variant}/external"
        );
        let specification = inputs::input()["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .unwrap();
        let row = inputs::resolve(specification);
        let native = inputs::reference()["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .unwrap();
        let native_js = inputs::blob(native["outputs"]["out/main.js"].as_str().unwrap());
        let native_map = decoded::decoded(inputs::blob(
            native["outputs"]["out/main.js.map"].as_str().unwrap(),
        ));
        let (javascript, map) = emitted(row["files"]["main.ts"].as_str().unwrap(), EcmaTarget::Es5);
        let tokens = blueice_bluets::lex("<actual>", &javascript).unwrap();
        for name in ["first", "second"] {
            let native_start = native_js.find(&format!("{name} = ")).unwrap();
            let expected_offset = native_start + native_js[native_start..].find(';').unwrap();
            let (line, column) = position(native_js, expected_offset);
            let expected = native_map["segments"]
                .as_array()
                .unwrap()
                .iter()
                .rfind(|segment| {
                    segment[0] == line && segment[1].as_u64().unwrap() as usize <= column
                })
                .unwrap();
            for index in 0..tokens.len() - 1 {
                if tokens[index].text != name || tokens[index + 1].text != "=" {
                    continue;
                }
                let semicolon = tokens[index + 2..]
                    .iter()
                    .find(|token| token.text == ";")
                    .unwrap();
                let (line, column) = position(&javascript, semicolon.start);
                let actual = map
                    .original_position_at(&javascript, line, column, 100_000, 32_000_000)
                    .unwrap();
                assert_eq!(
                    actual.map(|point| (point.line, point.column_utf16)),
                    Some((
                        expected[3].as_u64().unwrap() as usize,
                        expected[4].as_u64().unwrap() as usize
                    )),
                    "{variant}: {name} initializer endpoint"
                );
            }
        }
    }
}

#[test]
fn es5_tagged_raw_spans_and_completion_keep_distinct_native_owners() {
    for (format, module_kind) in [("cjs", ModuleKind::CommonJs), ("esm", ModuleKind::Esm)] {
        for variant in ["original", "crlfAstral"] {
            let id =
                format!("ge2_module_helpers/tagged-template-{format}-es5-i0-n0/{variant}/external");
            let specification = inputs::input()["cases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["id"] == id)
                .unwrap();
            let row = inputs::resolve(specification);
            let native = inputs::reference()["cases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["id"] == id)
                .unwrap();
            let native_js = inputs::blob(native["outputs"]["out/main.js"].as_str().unwrap());
            let native_map = decoded::decoded(inputs::blob(
                native["outputs"]["out/main.js.map"].as_str().unwrap(),
            ));
            let (javascript, map) = emitted_module(
                row["files"]["main.ts"].as_str().unwrap(),
                EcmaTarget::Es5,
                module_kind,
            );
            let offsets = |code: &str| {
                let raw = code.match_indices("[\"a\", \"\"]").nth(1).unwrap().0;
                let tail = raw + "[\"a\", ".len();
                let operand = code.rfind("41").unwrap();
                let terminator = operand + code[operand..].find(';').unwrap();
                vec![
                    raw + 1,
                    raw + "[\"a\"".len(),
                    tail,
                    tail + 2,
                    operand,
                    operand + 1,
                    operand + 2,
                    terminator,
                    terminator + 1,
                ]
            };
            for (expected_offset, actual_offset) in
                offsets(native_js).into_iter().zip(offsets(&javascript))
            {
                let (native_line, native_column) = position(native_js, expected_offset);
                let expected = native_map["segments"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .rfind(|segment| {
                        segment[0].as_u64().unwrap() as usize == native_line
                            && segment[1].as_u64().unwrap() as usize <= native_column
                    })
                    .unwrap();
                assert!(expected.as_array().unwrap().len() > 2);
                let (line, column) = position(&javascript, actual_offset);
                let actual = map
                    .original_position_at(&javascript, line, column, 100_000, 32_000_000)
                    .unwrap()
                    .expect("original tagged-template role");
                assert_eq!(
                    (actual.line, actual.column_utf16),
                    (
                        expected[3].as_u64().unwrap() as usize,
                        expected[4].as_u64().unwrap() as usize
                    ),
                    "{id}: tagged raw/substitution/statement role at {line}:{column}"
                );
            }
            let helper = javascript
                .find("function __blueice_target_template_object_")
                .unwrap();
            let (line, column) = position(&javascript, helper);
            assert_eq!(
                map.original_position_at(&javascript, line, column, 100_000, 32_000_000)
                    .unwrap(),
                None
            );
        }
    }
}

#[test]
fn es5_computed_write_keeps_saved_reference_and_statement_end_owners() {
    for variant in ["original", "crlfAstral"] {
        let id = format!("ge1_downlevel/computed-assignment-yield-es5-commonjs-assign-iterate/{variant}/external");
        let specification = inputs::input()["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .unwrap();
        let row = inputs::resolve(specification);
        let native = inputs::reference()["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .unwrap();
        let native_js = inputs::blob(native["outputs"]["out/main.js"].as_str().unwrap());
        let native_map = decoded::decoded(inputs::blob(
            native["outputs"]["out/main.js.map"].as_str().unwrap(),
        ));
        let (javascript, map) = emitted(row["files"]["main.ts"].as_str().unwrap(), EcmaTarget::Es5);
        let body = javascript.find("function sequence()").unwrap();
        let tokens = blueice_bluets::lex("<computed>", &javascript).unwrap();
        let target = tokens
            .windows(5)
            .find(|tokens| {
                tokens[0].start > body
                    && tokens[0]
                        .text
                        .starts_with("__blueice_target_generator_context_")
                    && tokens[1].is("[")
                    && tokens[2]
                        .text
                        .starts_with("__blueice_target_generator_context_")
                    && tokens[3].is("]")
                    && tokens[4].is("=")
            })
            .unwrap();
        let native_reference = native_js.find("_a[_b]").unwrap();
        let expected_at = |offset| {
            let (line, column) = position(native_js, offset);
            let segment = native_map["segments"]
                .as_array()
                .unwrap()
                .iter()
                .rfind(|segment| {
                    segment[0].as_u64().unwrap() as usize == line
                        && segment[1].as_u64().unwrap() as usize <= column
                })
                .unwrap();
            assert!(segment.as_array().unwrap().len() > 2);
            (
                segment[3].as_u64().unwrap() as usize,
                segment[4].as_u64().unwrap() as usize,
            )
        };
        for token in &target[..4] {
            for offset in token.start..token.end {
                let (line, column) = position(&javascript, offset);
                let actual = map
                    .original_position_at(&javascript, line, column, 100_000, 32_000_000)
                    .unwrap()
                    .expect("saved computed reference");
                assert_eq!(
                    (actual.line, actual.column_utf16),
                    expected_at(native_reference),
                    "{id}: saved reference interior"
                );
            }
        }
        let initializer = javascript.find("values = [0,0];").unwrap() + "values = [0,0];".len();
        for (expected_offset, offset) in [
            (native_reference + "_a[_b] ".len(), target[4].start),
            (
                native_js.find("values = [0, 0];").unwrap() + "values = [0, 0];".len(),
                initializer,
            ),
        ] {
            let (line, column) = position(&javascript, offset);
            let actual = map
                .original_position_at(&javascript, line, column, 100_000, 32_000_000)
                .unwrap()
                .expect("computed write boundary");
            assert_eq!(
                (actual.line, actual.column_utf16),
                expected_at(expected_offset),
                "{id}: computed operation/initializer endpoint"
            );
        }
    }
}

#[test]
fn es5_switch_case_jumps_and_break_keep_distinct_native_owners() {
    for variant in ["original", "crlfAstral"] {
        let id = format!("ge1_downlevel/switch-suspension-fallthrough-es5-commonjs-assign-iterate/{variant}/external");
        let specification = inputs::input()["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .unwrap();
        let row = inputs::resolve(specification);
        let native = inputs::reference()["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .unwrap();
        let native_js = inputs::blob(native["outputs"]["out/main.js"].as_str().unwrap());
        let native_map = decoded::decoded(inputs::blob(
            native["outputs"]["out/main.js.map"].as_str().unwrap(),
        ));
        let (javascript, map) = emitted(row["files"]["main.ts"].as_str().unwrap(), EcmaTarget::Es5);
        let tokens = blueice_bluets::lex("<switch>", &javascript).unwrap();
        let mut pairs = Vec::new();
        for value in ["2", "4"] {
            let header = tokens
                .windows(5)
                .find(|tokens| {
                    tokens[0].is("case")
                        && tokens[1].is(value)
                        && tokens[2].is(":")
                        && tokens[3]
                            .text
                            .starts_with("__blueice_target_generator_context_")
                        && tokens[4].is(".")
                })
                .unwrap();
            let native_header = native_js.find(&format!("case {value}: return [3")).unwrap();
            pairs.push((native_header + "case ".len() + value.len(), header[2].start));
            let native_jump = native_header + format!("case {value}: ").len();
            let end = tokens
                .iter()
                .find(|token| token.start >= header[3].start && token.is(";"))
                .unwrap();
            pairs.extend((header[3].start..end.end).map(|offset| (native_jump, offset)));
            let native_end = native_jump + native_js[native_jump..].find(';').unwrap() + 1;
            pairs.push((native_end, end.end));
        }
        let native_break = native_js.find("return [3 /*break*/, 6]").unwrap();
        let jump = javascript.find("return {kind:\"jump\"").unwrap();
        let jump_end = jump + javascript[jump..].find(';').unwrap() + 1;
        pairs.extend((jump..jump_end).map(|offset| (native_break, offset)));
        pairs.push((
            native_break + native_js[native_break..].find(';').unwrap() + 1,
            jump_end,
        ));
        for (expected_offset, offset) in pairs {
            let (line, column) = position(native_js, expected_offset);
            let expected = native_map["segments"]
                .as_array()
                .unwrap()
                .iter()
                .rfind(|segment| {
                    segment[0].as_u64().unwrap() as usize == line
                        && segment[1].as_u64().unwrap() as usize <= column
                })
                .unwrap();
            assert!(expected.as_array().unwrap().len() > 2);
            let (line, column) = position(&javascript, offset);
            let actual = map
                .original_position_at(&javascript, line, column, 100_000, 32_000_000)
                .unwrap()
                .expect("source case/break role");
            assert_eq!(
                (actual.line, actual.column_utf16),
                (
                    expected[3].as_u64().unwrap() as usize,
                    expected[4].as_u64().unwrap() as usize
                ),
                "{id}: source case/break at {line}:{column}"
            );
        }
    }
}

fn exception_fixture(form: &str, variant: &str) -> (String, Value, String, SourceMap) {
    let id = format!("ge1_downlevel/{form}-es5-commonjs-assign-iterate/{variant}/external");
    let specification = inputs::input()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == id)
        .unwrap();
    let row = inputs::resolve(specification);
    let native = inputs::reference()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == id)
        .unwrap();
    let javascript = inputs::blob(native["outputs"]["out/main.js"].as_str().unwrap()).to_string();
    let map = decoded::decoded(inputs::blob(
        native["outputs"]["out/main.js.map"].as_str().unwrap(),
    ));
    let (actual, actual_map) = emitted(row["files"]["main.ts"].as_str().unwrap(), EcmaTarget::Es5);
    (javascript, map, actual, actual_map)
}

fn same_exception_owner(
    native: &str,
    native_map: &Value,
    actual: &str,
    map: &SourceMap,
    expected: usize,
    offset: usize,
) {
    let (line, column) = position(native, expected);
    let segment = native_map["segments"]
        .as_array()
        .unwrap()
        .iter()
        .rfind(|segment| {
            segment[0].as_u64().unwrap() as usize == line
                && segment[1].as_u64().unwrap() as usize <= column
        })
        .unwrap();
    assert!(segment.as_array().unwrap().len() > 2);
    let (line, column) = position(actual, offset);
    let owner = map
        .original_position_at(actual, line, column, 100_000, 32_000_000)
        .unwrap()
        .expect("source exception role");
    assert_eq!(
        (owner.line, owner.column_utf16),
        (
            segment[3].as_u64().unwrap() as usize,
            segment[4].as_u64().unwrap() as usize
        ),
        "exception role at {line}:{column}"
    );
}

#[test]
fn es5_named_class_publication_retains_the_native_name_span() {
    for variant in ["original", "crlfAstral"] {
        let id = format!(
            "ge1_downlevel/called-imported-heritage-es5-commonjs-assign-iterate/{variant}/external"
        );
        let specification = inputs::input()["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .unwrap();
        let row = inputs::resolve(specification);
        let reference = inputs::reference()["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .unwrap();
        let native = inputs::blob(reference["outputs"]["out/base.js"].as_str().unwrap());
        let native_map = decoded::decoded(inputs::blob(
            reference["outputs"]["out/base.js.map"].as_str().unwrap(),
        ));
        let (actual, map) = emitted(row["files"]["base.ts"].as_str().unwrap(), EcmaTarget::Es5);
        let publication = "exports.Base = Base;";
        let expected = native.rfind(publication).unwrap();
        let start = actual.rfind(publication).unwrap();
        for offset in start..start + publication.len() {
            same_exception_owner(native, &native_map, &actual, &map, expected, offset);
        }
        same_exception_owner(
            native,
            &native_map,
            &actual,
            &map,
            expected + publication.len(),
            start + publication.len(),
        );
    }
}

#[test]
fn system_class_publication_is_generated_transport() {
    for (target, native_target) in [(EcmaTarget::Es5, "es5"), (EcmaTarget::Es2015, "es2015")] {
        for variant in ["original", "crlfAstral"] {
            let id = format!(
                "ge2_module_helpers/inherit-constructor-spread-system-{native_target}-i1-n0/{variant}/external"
            );
            let specification = inputs::input()["cases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["id"] == id)
                .unwrap();
            let row = inputs::resolve(specification);
            let reference = inputs::reference()["cases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["id"] == id)
                .unwrap();
            let native = inputs::blob(reference["outputs"]["out/main.js"].as_str().unwrap());
            let native_map = decoded::decoded(inputs::blob(
                reference["outputs"]["out/main.js.map"].as_str().unwrap(),
            ));
            let (actual, map) = emitted_module(
                row["files"]["main.ts"].as_str().unwrap(),
                target,
                ModuleKind::System,
            );
            for name in ["Base", "Derived"] {
                if target >= EcmaTarget::Es2015 {
                    let assignment = format!("{name} = class {name}");
                    let expected = native.find(&assignment).unwrap();
                    let start = actual.find(&assignment).unwrap();
                    for offset in start..start + name.len() + " = ".len() {
                        same_exception_owner(native, &native_map, &actual, &map, expected, offset);
                    }
                    let native_end = native[expected..].find("};").unwrap() + expected + 1;
                    let actual_end = actual[start..].find("};").unwrap() + start + 1;
                    same_exception_owner(
                        native,
                        &native_map,
                        &actual,
                        &map,
                        native_end,
                        actual_end,
                    );
                }
                let expected = native.find(&format!("(\"{name}\", {name});")).unwrap();
                let end = expected + format!("(\"{name}\", {name});").len();
                for offset in expected..end {
                    assert!(!native_mapped_at(&native_map, native, offset));
                }
                let publication = format!("exports.{name} = {name};");
                let start = actual.find(&publication).unwrap();
                for offset in start..start + publication.len() {
                    let (line, column) = position(&actual, offset);
                    assert_eq!(
                        map.original_position_at(&actual, line, column, 100_000, 32_000_000)
                            .unwrap(),
                        None,
                        "System class publication {name} at {line}:{column}"
                    );
                }
            }
        }
    }
}

#[test]
fn original_leading_block_comment_retains_file_start_after_commonjs_headers() {
    for form in ["super-write-receiver-order", "async-arrow-default-block"] {
        for (target, name) in [
            (EcmaTarget::Es2015, "es2015"),
            (EcmaTarget::Es2022, "es2022"),
        ] {
            for (module_name, module) in [
                ("commonjs", ModuleKind::CommonJs),
                ("esnext", ModuleKind::Esm),
            ] {
                let id = format!(
                    "ge1_downlevel/{form}-{name}-{module_name}-assign-iterate/crlfAstral/external"
                );
                let specification = inputs::input()["cases"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|row| row["id"] == id)
                    .unwrap();
                let row = inputs::resolve(specification);
                let reference = inputs::reference()["cases"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|row| row["id"] == id)
                    .unwrap();
                let native = inputs::blob(reference["outputs"]["out/main.js"].as_str().unwrap());
                let native_map = decoded::decoded(inputs::blob(
                    reference["outputs"]["out/main.js.map"].as_str().unwrap(),
                ));
                let (actual, map) =
                    emitted_module(row["files"]["main.ts"].as_str().unwrap(), target, module);
                let comment = "/* G-E5 🧭 */";
                let expected = native.find(comment).unwrap();
                let begin = actual.find(comment).unwrap();
                let (line, column) = position(&actual, begin);
                for unit in 0..=comment.encode_utf16().count() {
                    let (expected_line, expected_column) = position(native, expected);
                    let segment = native_map["segments"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .rfind(|point| {
                            point[0] == expected_line
                                && point[1].as_u64().unwrap() as usize <= expected_column + unit
                        })
                        .unwrap();
                    assert!(segment.as_array().unwrap().len() > 2);
                    let owner = map.original_position_at(&actual, line, column + unit, 100_000, 32_000_000).unwrap()
                        .expect("original comments retain their complete SourceFile range after generated headers");
                    assert_eq!(
                        (owner.line, owner.column_utf16),
                        (
                            segment[3].as_u64().unwrap() as usize,
                            segment[4].as_u64().unwrap() as usize
                        ),
                        "{id}: comment UTF-16 unit {unit}"
                    );
                }
                let end = begin + comment.len();
                let next = blueice_bluets::lex("<emitted>", &actual)
                    .unwrap()
                    .into_iter()
                    .find(|token| token.start >= end)
                    .unwrap();
                let (source_line, source_column) =
                    position(row["files"]["main.ts"].as_str().unwrap(), comment.len());
                for offset in end..next.start {
                    let (line, column) = position(&actual, offset);
                    if let Some(owner) = map
                        .original_position_at(&actual, line, column, 100_000, 32_000_000)
                        .unwrap()
                    {
                        assert_eq!(
                            (owner.line, owner.column_utf16),
                            (source_line, source_column),
                            "{id}: original comment trailing gap"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn es5_finally_pending_and_overriding_returns_keep_distinct_native_owners() {
    for variant in ["original", "crlfAstral"] {
        let (native, native_map, javascript, map) =
            exception_fixture("finally-overrides-return", variant);
        for value in ["2", "4"] {
            let wanted = format!("return [2 /*return*/, {value}];");
            let observed = format!("return {{kind:\"return\", value:({value})}};");
            let n = native.find(&wanted).unwrap();
            let a = javascript.find(&observed).unwrap();
            for offset in a..a + "return {kind:\"return\", value:".len() {
                same_exception_owner(&native, &native_map, &javascript, &map, n, offset);
            }
            let operand = a + observed.find('(').unwrap() + 1;
            same_exception_owner(
                &native,
                &native_map,
                &javascript,
                &map,
                n + wanted.rfind(", ").unwrap() + 2,
                operand,
            );
            for offset in operand + value.len()..a + observed.len() {
                same_exception_owner(
                    &native,
                    &native_map,
                    &javascript,
                    &map,
                    n + wanted.find(']').unwrap(),
                    offset,
                );
            }
            same_exception_owner(
                &native,
                &native_map,
                &javascript,
                &map,
                n + wanted.len(),
                a + observed.len(),
            );
        }
        let enter = javascript.find("return {kind:\"enter\"").unwrap();
        let end = enter + javascript[enter..].find(';').unwrap() + 1;
        for offset in enter..end {
            let (line, column) = position(&javascript, offset);
            assert_eq!(
                map.original_position_at(&javascript, line, column, 100_000, 32_000_000)
                    .unwrap(),
                None
            );
        }
    }
}

#[test]
fn es5_caught_alias_and_effects_keep_native_owners_without_transport_grants() {
    for variant in ["original", "crlfAstral"] {
        let (native, native_map, javascript, map) =
            exception_fixture("switch-exception-finally", variant);
        let tokens = blueice_bluets::lex("<catch>", &javascript).unwrap();
        let read = tokens
            .windows(4)
            .find(|tokens| {
                tokens[0].is("String")
                    && tokens[1].is("(")
                    && tokens[2].text.starts_with("__blueice_target_binding_")
                    && tokens[3].is(")")
            })
            .unwrap();
        let n = native.find("String(").unwrap() + "String(".len();
        for offset in read[2].start..read[2].end {
            same_exception_owner(&native, &native_map, &javascript, &map, n, offset);
        }
        same_exception_owner(
            &native,
            &native_map,
            &javascript,
            &map,
            n + native[n..].find(')').unwrap(),
            read[2].end,
        );
        let wanted = native.find("log.push(\"finally\")").unwrap() + "log.push(".len();
        let body = javascript.find("function sequence()").unwrap();
        let observed =
            body + javascript[body..].find("log.push(\"finally\")").unwrap() + "log.push(".len();
        for index in 0.."\"finally\"".len() {
            same_exception_owner(
                &native,
                &native_map,
                &javascript,
                &map,
                wanted + index,
                observed + index,
            );
        }
        let binding = tokens
            .windows(3)
            .find(|tokens| {
                tokens[0].text.starts_with("__blueice_target_binding_")
                    && tokens[1].is("=")
                    && tokens[2]
                        .text
                        .starts_with("__blueice_target_generator_context_")
            })
            .unwrap();
        let end = tokens
            .iter()
            .find(|token| token.start >= binding[2].start && token.is(";"))
            .unwrap()
            .end;
        for offset in binding[0].start..end {
            let (line, column) = position(&javascript, offset);
            assert_eq!(
                map.original_position_at(&javascript, line, column, 100_000, 32_000_000)
                    .unwrap(),
                None
            );
        }
    }
}

#[test]
fn es5_loop_initializer_predicates_and_exit_keep_distinct_native_owners() {
    for variant in ["original", "crlfAstral"] {
        let (native, native_map, javascript, map) =
            exception_fixture("loop-abrupt-finally", variant);
        let tokens = blueice_bluets::lex("<loop>", &javascript).unwrap();
        let initializer = tokens
            .windows(3)
            .find(|tokens| {
                tokens[0].text.starts_with("__blueice_target_binding_")
                    && tokens[1].is("=")
                    && tokens[2].is("0")
            })
            .unwrap();
        let initializer_end = tokens
            .iter()
            .find(|token| token.start > initializer[2].start && token.is(";"))
            .unwrap()
            .end;
        let native_end = native.find("index = 0;").unwrap() + "index = 0;".len();
        same_exception_owner(
            &native,
            &native_map,
            &javascript,
            &map,
            native_end,
            initializer_end,
        );
        for value in ["1", "2"] {
            let captured = tokens
                .windows(6)
                .find(|tokens| {
                    tokens[0]
                        .text
                        .starts_with("__blueice_target_generator_context_")
                        && tokens[1].is("=")
                        && tokens[2].text.starts_with("__blueice_target_binding_")
                        && tokens[3].is("===")
                        && tokens[4].is(value)
                        && tokens[5].is(";")
                })
                .unwrap();
            let keyword = native.find(&format!("if (index === {value})")).unwrap();
            for offset in captured[0].start..captured[2].start {
                same_exception_owner(&native, &native_map, &javascript, &map, keyword, offset);
            }
            let close = keyword + native[keyword..].find(')').unwrap();
            for offset in [captured[5].start, captured[5].end] {
                same_exception_owner(&native, &native_map, &javascript, &map, close, offset);
            }
        }
        let guard = tokens
            .windows(5)
            .find(|tokens| {
                tokens[0]
                    .text
                    .starts_with("__blueice_target_generator_context_")
                    && tokens[1].is("=")
                    && tokens[2].text.starts_with("__blueice_target_binding_")
                    && tokens[3].is("<")
                    && tokens[4].is("3")
            })
            .unwrap();
        let branch = tokens
            .windows(9)
            .find(|tokens| {
                tokens[2].is("label")
                    && tokens[3].is("=")
                    && tokens[4].text == guard[0].text
                    && tokens[5].is("?")
                    && tokens[7].is(":")
            })
            .unwrap();
        let condition = native.find("if (!(index < 3))").unwrap();
        let exit = condition + native[condition..].find("return").unwrap();
        for offset in branch[8].start..branch[8].end {
            same_exception_owner(&native, &native_map, &javascript, &map, exit, offset);
        }
    }
}
