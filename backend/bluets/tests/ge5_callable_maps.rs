// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Callable lowering keeps the original parameter and expression subtrees.
#![allow(dead_code)]
#[path = "oracle_support/gd2_maps.rs"]
mod decoded;
#[path = "oracle_support/ge5_inputs.rs"]
mod inputs;
use blueice_bluets::{
    compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource, SourceMap,
};
use serde_json::Value;

fn position(text: &str, offset: usize) -> (usize, usize) {
    let prefix = &text[..offset];
    (
        prefix.bytes().filter(|byte| *byte == b'\n').count(),
        prefix.rsplit('\n').next().unwrap().encode_utf16().count(),
    )
}

fn fixture(
    target: EcmaTarget,
    native_target: &str,
    variant: &str,
) -> (String, Value, String, SourceMap) {
    let id = format!("ge1_downlevel/async-arrow-default-block-{native_target}-commonjs-assign-iterate/{variant}/external");
    let specification = inputs::input()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == id)
        .unwrap();
    let row = inputs::resolve(specification);
    let source = row["files"]["main.ts"].as_str().unwrap();
    let reference = inputs::reference()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == id)
        .unwrap();
    let native = inputs::blob(reference["outputs"]["out/main.js"].as_str().unwrap()).to_string();
    let native_map = decoded::decoded(inputs::blob(
        reference["outputs"]["out/main.js.map"].as_str().unwrap(),
    ));
    let module = "memory:///main.ts";
    let result = compile(
        module,
        &MapLoader::from([ModuleSource::new(module, source)]),
        CompilerOptions {
            target,
            module_kind: ModuleKind::CommonJs,
            libraries: Some(vec![EcmaTarget::Es2022]),
            downlevel_iteration: true,
            source_map: true,
            inline_sources: true,
            ..CompilerOptions::default()
        },
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let artifact = result.output.unwrap().artifacts.remove(module).unwrap();
    (
        native,
        native_map,
        artifact.javascript,
        artifact.source_map.unwrap(),
    )
}

fn assert_owner(
    native: &str,
    native_map: &Value,
    expected: usize,
    actual: &str,
    map: &SourceMap,
    offset: usize,
) {
    let (line, column) = position(native, expected);
    let owner = native_map["segments"]
        .as_array()
        .unwrap()
        .iter()
        .rfind(|segment| segment[0] == line && segment[1].as_u64().unwrap() as usize <= column)
        .unwrap();
    assert!(owner.as_array().unwrap().len() > 2);
    let (line, column) = position(actual, offset);
    let actual_owner = map
        .original_position_at(actual, line, column, 100_000, 32_000_000)
        .unwrap()
        .unwrap_or_else(|| panic!("complete Native callable source role at {line}:{column}"));
    assert_eq!(
        (actual_owner.line, actual_owner.column_utf16),
        (
            owner[3].as_u64().unwrap() as usize,
            owner[4].as_u64().unwrap() as usize
        ),
        "source role at {line}:{column}"
    );
}

#[test]
fn lowered_async_arrow_keeps_the_complete_awaited_call_source() {
    for (target, native_target) in [
        (EcmaTarget::Es5, "es5"),
        (EcmaTarget::Es2015, "es2015"),
        (EcmaTarget::Es2016, "es2016"),
    ] {
        for variant in ["original", "crlfAstral"] {
            let (native, native_map, actual, map) = fixture(target, native_target, variant);
            let call = "Promise.resolve(2)";
            let expected = native.rfind(call).unwrap();
            let start = actual.rfind(call).unwrap();
            for offset in 0..call.len() {
                assert_owner(
                    &native,
                    &native_map,
                    expected + offset,
                    &actual,
                    &map,
                    start + offset,
                );
            }
        }
    }
}

#[test]
fn es5_async_arrow_default_branch_keeps_native_parameter_phases() {
    for variant in ["original", "crlfAstral"] {
        let (native, native_map, actual, map) = fixture(EcmaTarget::Es5, "es5", variant);
        let expected = native.find("if (value === void 0)").unwrap();
        let begin = actual.find("value=(40)").unwrap();
        for offset in begin..begin + "value=(40".len() {
            assert_owner(&native, &native_map, expected, &actual, &map, offset);
        }
        let expected_end = native.find("40; }").unwrap() + 2;
        for offset in begin + "value=(40".len()..begin + "value=(40);".len() {
            assert_owner(&native, &native_map, expected_end, &actual, &map, offset);
        }
        let predicate = actual.find("value===void 0").unwrap();
        for offset in predicate..predicate + "value===void 0".len() {
            assert_owner(&native, &native_map, expected, &actual, &map, offset);
        }
    }
}

#[test]
fn lowered_arrow_operation_and_driver_keep_distinct_native_origins() {
    for (target, name) in [
        (EcmaTarget::Es5, "es5"),
        (EcmaTarget::Es2015, "es2015"),
        (EcmaTarget::Es2016, "es2016"),
    ] {
        for variant in ["original", "crlfAstral"] {
            let (native, expected, actual, map) = fixture(target, name, variant);
            let nt = blueice_bluets::lex("<native>", &native).unwrap();
            let at = blueice_bluets::lex("<actual>", &actual).unwrap();
            let native_binding = nt.iter().position(|token| token.is("calculate")).unwrap();
            let native_operation = nt[native_binding..]
                .iter()
                .find(|token| token.is(if target == EcmaTarget::Es5 { "{" } else { "=>" }))
                .unwrap()
                .start;
            let function = at
                .windows(2)
                .position(|pair| {
                    pair[0].is("function")
                        && pair[1].text.starts_with("__blueice_target_async_callable_")
                })
                .unwrap();
            let opening = at[function..]
                .iter()
                .find(|token| token.is("{"))
                .unwrap()
                .start;
            assert_owner(&native, &expected, native_operation, &actual, &map, opening);
            if target != EcmaTarget::Es5 {
                let native_driver = nt
                    .iter()
                    .rfind(|token| token.is("__awaiter"))
                    .unwrap()
                    .start;
                let actual_driver = at[function..]
                    .windows(2)
                    .find(|pair| {
                        pair[0]
                            .text
                            .strip_prefix("__blueice_target_async_")
                            .is_some_and(|suffix| {
                                !suffix.is_empty()
                                    && suffix.bytes().all(|byte| byte.is_ascii_digit())
                            })
                            && pair[1].is("(")
                    })
                    .unwrap()[0]
                    .start;
                assert_owner(
                    &native,
                    &expected,
                    native_driver,
                    &actual,
                    &map,
                    actual_driver,
                );
            }
        }
    }
}

#[test]
fn lowered_arrow_callback_header_keeps_its_complete_native_source_phase() {
    fn header(tokens: &[blueice_bluets::Token]) -> std::ops::Range<usize> {
        let binding = tokens
            .iter()
            .position(|token| token.is("calculate"))
            .unwrap();
        tokens
            .iter()
            .enumerate()
            .skip(binding)
            .find_map(|(index, token)| {
                if !token.is("function") {
                    return None;
                }
                let open = index + 1 + usize::from(tokens.get(index + 1)?.is("*"));
                (tokens.get(open)?.is("(") && tokens.get(open + 1)?.is("value"))
                    .then_some(index..open + 1)
            })
            .unwrap()
    }
    for (target, name) in [
        (EcmaTarget::Es5, "es5"),
        (EcmaTarget::Es2015, "es2015"),
        (EcmaTarget::Es2016, "es2016"),
    ] {
        for variant in ["original", "crlfAstral"] {
            let (native, expected, actual, map) = fixture(target, name, variant);
            let nt = blueice_bluets::lex("<native>", &native).unwrap();
            let at = blueice_bluets::lex("<actual>", &actual).unwrap();
            let nh = &nt[header(&nt)];
            let ah = &at[header(&at)];
            assert_eq!(
                nh.len(),
                ah.len(),
                "{name}/{variant}: complete callback header"
            );
            for (n, a) in nh.iter().zip(ah) {
                assert_eq!(n.text, a.text);
                for unit in 0..n.text.len() {
                    let (line, column) = position(&native, n.start + unit);
                    let owner = expected["segments"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .rfind(|point| {
                            point[0] == line && point[1].as_u64().unwrap() as usize <= column
                        });
                    let expected_owner = owner
                        .filter(|point| point.as_array().unwrap().len() > 2)
                        .map(|point| {
                            (
                                point[3].as_u64().unwrap() as usize,
                                point[4].as_u64().unwrap() as usize,
                            )
                        });
                    let (line, column) = position(&actual, a.start + unit);
                    let actual_owner = map
                        .original_position_at(&actual, line, column, 100_000, 32_000_000)
                        .unwrap()
                        .map(|point| (point.line, point.column_utf16));
                    assert_eq!(
                        actual_owner, expected_owner,
                        "{name}/{variant}: complete {} interior",
                        a.text
                    );
                }
            }
            if target != EcmaTarget::Es5 {
                let body = |tokens: &[blueice_bluets::Token], after: usize| {
                    let open = (after..tokens.len())
                        .find(|&at| tokens[at].is("{"))
                        .unwrap();
                    let mut depth = 1;
                    let close = (open + 1..tokens.len())
                        .find(|&at| {
                            if tokens[at].is("{") {
                                depth += 1;
                            }
                            if tokens[at].is("}") {
                                depth -= 1;
                            }
                            depth == 0
                        })
                        .unwrap();
                    (open, close)
                };
                let (no, nc) = body(&nt, header(&nt).end);
                let (ao, ac) = body(&at, header(&at).end);
                for offset in at[ao].end..at[ao + 1].start {
                    assert_owner(&native, &expected, nt[no].end, &actual, &map, offset);
                }
                assert_owner(&native, &expected, nt[nc].end, &actual, &map, at[ac].end);
                assert!(nt[nc + 1].is(")") && at[ac + 1].is(")"));
                for (n, a) in [
                    (nt[nc + 1].start, at[ac + 1].start),
                    (nt[nc + 1].end, at[ac + 1].end),
                ] {
                    assert_owner(&native, &expected, n, &actual, &map, a);
                }
            }
        }
    }
}
