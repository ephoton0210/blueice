// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Lowered method parameters and bodies retain their original source subtrees.
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

fn fixture(target: EcmaTarget, name: &str, variant: &str) -> (String, Value, String, SourceMap) {
    let id = format!(
        "ge1_downlevel/async-object-method-{name}-commonjs-assign-iterate/{variant}/external"
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
    let module = "memory:///main.ts";
    let result = compile(
        module,
        &MapLoader::from([ModuleSource::new(
            module,
            row["files"]["main.ts"].as_str().unwrap(),
        )]),
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
        inputs::blob(reference["outputs"]["out/main.js"].as_str().unwrap()).to_string(),
        decoded::decoded(inputs::blob(
            reference["outputs"]["out/main.js.map"].as_str().unwrap(),
        )),
        artifact.javascript,
        artifact.source_map.unwrap(),
    )
}

fn same_owner(
    native: &str,
    expected: &Value,
    expected_at: usize,
    actual: &str,
    map: &SourceMap,
    actual_at: usize,
) {
    let (line, column) = position(native, expected_at);
    let owner = expected["segments"]
        .as_array()
        .unwrap()
        .iter()
        .rfind(|point| point[0] == line && point[1].as_u64().unwrap() as usize <= column)
        .filter(|point| point.as_array().unwrap().len() > 2)
        .map(|point| {
            (
                point[3].as_u64().unwrap() as usize,
                point[4].as_u64().unwrap() as usize,
            )
        });
    assert!(owner.is_some());
    let (line, column) = position(actual, actual_at);
    let observed = map
        .original_position_at(actual, line, column, 100_000, 32_000_000)
        .unwrap()
        .map(|point| (point.line, point.column_utf16));
    assert_eq!(observed, owner, "method source role at {line}:{column}");
}

#[test]
fn lowered_object_method_keeps_every_awaited_call_and_receiver_interior() {
    for (target, name) in [
        (EcmaTarget::Es5, "es5"),
        (EcmaTarget::Es2015, "es2015"),
        (EcmaTarget::Es2016, "es2016"),
    ] {
        for variant in ["original", "crlfAstral"] {
            let (native, expected, actual, map) = fixture(target, name, variant);
            for text in ["Promise.resolve(extra)", "this.value"] {
                let n = native.rfind(text).unwrap();
                let a = actual.rfind(text).unwrap();
                for unit in 0..text.len() {
                    same_owner(&native, &expected, n + unit, &actual, &map, a + unit);
                }
            }
        }
    }
}

#[test]
fn moved_object_method_parameter_keeps_its_native_binding_origin() {
    for (target, name) in [
        (EcmaTarget::Es5, "es5"),
        (EcmaTarget::Es2015, "es2015"),
        (EcmaTarget::Es2016, "es2016"),
    ] {
        for variant in ["original", "crlfAstral"] {
            let (native, expected, actual, map) = fixture(target, name, variant);
            let n = native.find("extra").unwrap();
            let a = actual.find("extra").unwrap();
            for unit in 0.."extra".len() {
                same_owner(&native, &expected, n + unit, &actual, &map, a + unit);
            }
        }
    }
}

#[test]
fn retained_object_method_body_starts_at_its_parameter_list_completion() {
    for (target, name) in [
        (EcmaTarget::Es2017, "es2017"),
        (EcmaTarget::Es2018, "es2018"),
        (EcmaTarget::Es2022, "es2022"),
    ] {
        for variant in ["original", "crlfAstral"] {
            let (native, expected, actual, map) = fixture(target, name, variant);
            let opening = |code: &str| {
                let tokens = blueice_bluets::lex("method.js", code).unwrap();
                let method = tokens.iter().position(|token| token.is("read")).unwrap();
                tokens[method..]
                    .iter()
                    .find(|token| token.is("{"))
                    .unwrap()
                    .start
            };
            same_owner(
                &native,
                &expected,
                opening(&native),
                &actual,
                &map,
                opening(&actual),
            );
        }
    }
}

#[test]
fn lowered_method_name_is_copied_and_argument_transport_has_no_source() {
    for (target, name) in [
        (EcmaTarget::Es5, "es5"),
        (EcmaTarget::Es2015, "es2015"),
        (EcmaTarget::Es2016, "es2016"),
    ] {
        for variant in ["original", "crlfAstral"] {
            let (native, expected, actual, map) = fixture(target, name, variant);
            let method_name = |code: &str| {
                blueice_bluets::lex("method.js", code)
                    .unwrap()
                    .into_iter()
                    .find(|token| token.is("read"))
                    .unwrap()
                    .start
            };
            let n = method_name(&native);
            let a = method_name(&actual);
            for unit in 0.."read".len() {
                same_owner(&native, &expected, n + unit, &actual, &map, a + unit);
            }
            let tokens = blueice_bluets::lex("method.js", &actual).unwrap();
            for token in tokens.iter().filter(|token| {
                token.text.starts_with("__blueice_target_async_parameter_") || token.is("apply")
            }) {
                for offset in token.start..token.end {
                    let (line, column) = position(&actual, offset);
                    assert!(
                        map.original_position_at(&actual, line, column, 100_000, 32_000_000)
                            .unwrap()
                            .is_none(),
                        "generated method argument transport at {offset}"
                    );
                }
            }
        }
    }
}

#[test]
fn method_awaiter_completion_keeps_the_original_method_endpoint() {
    for (target, name) in [
        (EcmaTarget::Es2015, "es2015"),
        (EcmaTarget::Es2016, "es2016"),
    ] {
        for variant in ["original", "crlfAstral"] {
            let (native, expected, actual, map) = fixture(target, name, variant);
            let completion = |code: &str| {
                let tokens = blueice_bluets::lex("method.js", code).unwrap();
                let generator = tokens
                    .windows(2)
                    .position(|pair| pair[0].is("function") && pair[1].is("*"))
                    .unwrap();
                let open = generator
                    + tokens[generator..]
                        .iter()
                        .position(|token| token.is("{"))
                        .unwrap();
                let mut depth = 0usize;
                for index in open..tokens.len() {
                    if tokens[index].is("{") {
                        depth += 1;
                    }
                    if tokens[index].is("}") {
                        depth -= 1;
                        if depth == 0 {
                            assert!(tokens[index + 1].is(")"));
                            return tokens[index + 1].start;
                        }
                    }
                }
                panic!("method generator callback must close");
            };
            same_owner(
                &native,
                &expected,
                completion(&native),
                &actual,
                &map,
                completion(&actual),
            );
        }
    }
}
