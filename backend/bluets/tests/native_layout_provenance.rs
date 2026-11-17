// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Printing changes trivia without losing owned syntax or Native boundaries.

use blueice_bluets::{
    compile, CompilerOptions, EcmaTarget, JavaScriptLayout, MapLoader, ModuleKind, ModuleSource,
    NewLine,
};
use serde_json::{json, Value};

#[path = "oracle_support/gd2_maps.rs"]
mod maps;

fn artifact(source: &str, target: EcmaTarget, new_line: NewLine) -> (String, Value) {
    let id = "memory:///main.ts";
    let result = compile(
        id,
        &MapLoader::from([ModuleSource::new(id, source)]),
        CompilerOptions {
            target,
            libraries: Some(vec![EcmaTarget::Es2022]),
            module_kind: ModuleKind::CommonJs,
            source_map: true,
            javascript_layout: JavaScriptLayout::TypeScript,
            new_line,
            ..CompilerOptions::default()
        },
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let artifact = &result.output.as_ref().unwrap().artifacts[id];
    (
        artifact.javascript.clone(),
        maps::decoded(&artifact.source_map.as_ref().unwrap().to_json()),
    )
}

fn owner(code: &str, decoded: &Value, offset: usize) -> Value {
    let prefix = &code[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let column = prefix.rsplit('\n').next().unwrap().encode_utf16().count();
    decoded["segments"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|point| point[0] == line && point[1].as_u64().unwrap() <= column as u64)
        .filter(|point| point.as_array().unwrap().len() > 2)
        .map(|point| json!(point.as_array().unwrap()[2..].to_vec()))
        .unwrap_or(Value::Null)
}

#[test]
fn implicit_constructor_completion_keeps_its_native_owner_after_newline_projection() {
    for prefix in ["", "const label: string = '🌟'; "] {
        let source = format!(
            "{prefix}class Base {{ value: number = 1; method(n: number): number {{ return this.value + n; }} }}"
        );
        for (new_line, ending) in [(NewLine::Lf, "\n"), (NewLine::CrLf, "\r\n")] {
            let (code, decoded) = artifact(&source, EcmaTarget::Es2015, new_line);
            let ending_pattern = format!("{ending}    }}{ending}");
            let close = code.find(&ending_pattern).unwrap() + ending.len() + 4;
            // Native maps the generated implicit constructor completion to the
            // original closing brace, including the astral prefix's UTF-16 width.
            assert_eq!(
                owner(&code, &decoded, close),
                json!([0, 0, source.encode_utf16().count() - 1]),
                "{code}; {decoded}"
            );
        }
    }
}

#[test]
fn native_class_factory_completion_and_invocation_keep_coincident_boundaries() {
    let source = "class Item { value: number = 1; }";
    let (_, decoded) = artifact(source, EcmaTarget::Es5, NewLine::Lf);
    let points = decoded["segments"].as_array().unwrap();
    let found = points.windows(2).any(|pair| {
        pair[0][0] == pair[1][0]
            && pair[0][1] == pair[1][1]
            && pair[0].as_array().unwrap()[2..]
                == json!([0, 0, source.len()]).as_array().unwrap()[..]
            && pair[1].as_array().unwrap()[2..] == json!([0, 0, 0]).as_array().unwrap()[..]
    });
    assert!(found, "{decoded}");
}

#[test]
fn a_moved_static_class_expression_keeps_distinct_factory_and_invocation_events() {
    let source = "function replace(value: any): any { return class extends value { static replacement = true; }; }";
    let (code, decoded) = artifact(source, EcmaTarget::Es5, NewLine::Lf);
    let points = decoded["segments"].as_array().unwrap();
    for pair in points.windows(2) {
        assert!(
            pair[0][0] != pair[1][0] || pair[0][1] != pair[1][1],
            "distinct factory completion and invocation events: {pair:?}; {code}"
        );
    }
}

#[test]
fn a_factory_capture_prefix_keeps_its_owner_through_inserted_indentation() {
    let source = "function replace(value: any): any { return class extends value { static replacement = true; }; }";
    let (code, decoded) = artifact(source, EcmaTarget::Es5, NewLine::Lf);
    let start = code.find("return (function () {").unwrap() + "return ".len();
    let end = start + code[start..].find("var ").unwrap();
    for offset in start..end {
        assert_eq!(
            owner(&code, &decoded, offset),
            json!([0, 0, source.find("return").unwrap()]),
            "factory prefix offset {offset}: {code}"
        );
    }
}

#[test]
fn crlf_conversion_preserves_the_complete_lf_mapping_inventory() {
    let source =
        "class Box<T> { constructor(public value: T) {} read(): T { return this.value; } }";
    let (_, lf) = artifact(source, EcmaTarget::Es5, NewLine::Lf);
    let (_, crlf) = artifact(source, EcmaTarget::Es5, NewLine::CrLf);
    assert_eq!(crlf, lf);
}

#[test]
fn lowered_await_ends_keep_source_and_generated_boundaries_distinct() {
    let source =
        "async function read(n: number): Promise<number> { return await Promise.resolve(n); }";
    let (code, decoded) = artifact(source, EcmaTarget::Es2015, NewLine::Lf);
    for pair in decoded["segments"].as_array().unwrap().windows(2) {
        assert!(
            pair[0][0] != pair[1][0] || pair[0][1] != pair[1][1],
            "lowered await completion: {pair:?}; {code}"
        );
    }
}

#[test]
fn a_nested_declaration_wrapper_keeps_the_native_binding_name_end_owner() {
    for prefix in ["", "/* 🧭 */ "] {
        let source = format!("{prefix}class Base {{}} function make(): any {{ class Nested extends Base {{ static value: number = 7; }} return Nested; }}");
        for (target, outer, inner, origin) in [
            (EcmaTarget::Es5, "var Nested", "var Nested", "class Nested"),
            (EcmaTarget::Es2015, "let Nested", "const Nested", "extends"),
        ] {
            for newline in [NewLine::Lf, NewLine::CrLf] {
                let (code, decoded) = artifact(&source, target, newline);
                let start = code.find(outer).unwrap() + outer.len();
                let end = start + code[start..].find(inner).unwrap();
                let column = source[..source.find(origin).unwrap()]
                    .encode_utf16()
                    .count();
                for offset in start..end {
                    assert_eq!(
                        owner(&code, &decoded, offset),
                        json!([0, 0, column]),
                        "nested wrapper offset {offset}: {code}"
                    );
                }
            }
        }
    }
}

#[test]
fn exported_class_keywords_inherit_the_native_modifier_end_without_an_extra_boundary() {
    let id = "memory:///main.ts";
    for gap in [" ", "  ", "\t"] {
        let source = format!(
            "export{gap}class Model {{ value: number = 1; }}\nexport{gap}default{gap}class Default {{ value: number = 2; }}\n"
        );
        for new_line in [NewLine::Lf, NewLine::CrLf] {
            let result = compile(
                id,
                &MapLoader::from([ModuleSource::new(id, source.clone())]),
                CompilerOptions {
                    target: EcmaTarget::Es2022,
                    module_kind: ModuleKind::Esm,
                    source_map: true,
                    new_line,
                    javascript_layout: JavaScriptLayout::TypeScript,
                    ..CompilerOptions::default()
                },
            );
            assert!(!result.has_errors(), "{:?}", result.diagnostics);
            let artifact = &result.output.as_ref().unwrap().artifacts[id];
            let decoded = maps::decoded(&artifact.source_map.as_ref().unwrap().to_json());
            // Pinned TypeScript 5.9.3 retains modifier ends, but does not
            // emit another event at the class keyword's generated start.
            for (name, line, column) in [
                ("class Model", 0, 6),
                ("class Default", 1, 13 + gap.encode_utf16().count()),
            ] {
                let offset = artifact.javascript.find(name).unwrap();
                assert_eq!(
                    owner(&artifact.javascript, &decoded, offset),
                    json!([0, line, column]),
                    "{name}: {}",
                    artifact.javascript
                );
                let before = &artifact.javascript[..offset];
                let generated_line = before.bytes().filter(|byte| *byte == b'\n').count();
                let generated_column = before.rsplit('\n').next().unwrap().encode_utf16().count();
                assert!(
                    !decoded["segments"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|segment| {
                            segment[0] == generated_line && segment[1] == generated_column
                        }),
                    "extra class keyword boundary: {decoded}"
                );
            }
        }
    }
}

#[test]
fn lowered_await_opcode_prefix_ends_at_the_unmapped_transport_group() {
    for source in [
        "/* 🧭 */ async function read(n: number): Promise<number> { return await Promise.resolve(n); }",
        "/* 🧭 */ class Box { value: number = 40; async read(n: number): Promise<number> { return this.value + await Promise.resolve(n); } }",
        "/* 🧭 */ const box = { value: 40, async read(n: number): Promise<number> { return this.value + await Promise.resolve(n); } };",
    ] {
        let original = source[..source.find("await ").unwrap()]
            .encode_utf16()
            .count();
        for newline in [NewLine::Lf, NewLine::CrLf] {
            let (code, decoded) = artifact(source, EcmaTarget::Es5, newline);
            let (start, prefix) = code
                .match_indices("return { kind: \"yield\", value: ")
                .next()
                .expect("the lowered await has a completion transport");
            let group = start + prefix.len();
            assert_eq!(&code[group..group + 1], "(");
            for offset in start..group {
                assert_eq!(owner(&code, &decoded, offset), json!([0, 0, original]),
                    "await opcode prefix at {offset}: {code}");
            }
            let end = group + code[group..].find(')').unwrap();
            for offset in group..end {
                assert_eq!(owner(&code, &decoded, offset), Value::Null,
                    "generated await grouping/slot at {offset}: {code}");
            }
        }
    }
}

#[test]
fn lowered_default_routing_keeps_its_transfer_on_one_physical_line() {
    let source = "const calculate=async(value:number=40):Promise<number>=>{const extra=await Promise.resolve(2);return value+extra;};\nexport const result: Promise<number> = calculate();\n";
    for newline in [NewLine::Lf, NewLine::CrLf] {
        let (code, _) = artifact(source, EcmaTarget::Es5, newline);
        let branches = code
            .lines()
            .filter(|line| line.contains(".label =") && line.contains('?'))
            .collect::<Vec<_>>();
        assert!(
            !branches.is_empty(),
            "the default parameter has a branch: {code}"
        );
        for branch in branches {
            assert!(branch.contains("; continue;"),
                "a routing clause must remain removable without changing map line coordinates: {branch}");
        }
    }
}

#[test]
fn binary_operand_starts_after_the_operator_trivia() {
    for prefix in ["", "/* 🧭 */ "] {
        let source = format!("{prefix}const calculate=async(value:number=40):Promise<number>=>{{const extra=await Promise.resolve(2);return value+extra;}};\nexport const result: Promise<number> = calculate();\n");
        let original = source[..source.find("+extra").unwrap()]
            .encode_utf16()
            .count();
        for target in [EcmaTarget::Es5, EcmaTarget::Es2015, EcmaTarget::Es2022] {
            for newline in [NewLine::Lf, NewLine::CrLf] {
                let (code, decoded) = artifact(&source, target, newline);
                let start = code
                    .rfind("value + extra")
                    .expect("the original addition is retained");
                for offset in start + 5..start + 8 {
                    assert_eq!(
                        owner(&code, &decoded, offset),
                        json!([0, 0, original]),
                        "operator and its trivia at {offset}: {code}"
                    );
                }
                assert_eq!(
                    owner(&code, &decoded, start + 8),
                    json!([0, 0, original + 1])
                );
            }
        }
    }
}

#[test]
fn retained_finally_trivia_does_not_inherit_a_try_body_source() {
    let source = "const log:string[]=[]; async function* sequence():AsyncGenerator<string,number,number>{try{yield `v${await Promise.resolve(1)}`;}finally{log.push(\"finally\");yield `f${await Promise.resolve(2)}`;}return 3;}";
    for target in [EcmaTarget::Es2018, EcmaTarget::Es2022] {
        for newline in [NewLine::Lf, NewLine::CrLf] {
            let (code, decoded) = artifact(source, target, newline);
            let at = code
                .find("finally {")
                .expect("the original finally block is retained");
            assert_eq!(
                owner(&code, &decoded, at - 1),
                Value::Null,
                "before finally: {code}"
            );
            assert_eq!(
                owner(&code, &decoded, at + 7),
                Value::Null,
                "before finally body: {code}"
            );
            assert_eq!(
                owner(&code, &decoded, at + 9),
                json!([0, 0, source.find("log.push").unwrap()]),
                "after finally body entry: {code}"
            );
        }
    }
}

#[test]
fn a_finally_effect_keeps_its_compact_case_statement_group() {
    let source = "const events:string[]=[]; async function read(value:number):Promise<number>{try{return await Promise.resolve(value);}finally{events.push(\"done\");}}";
    for newline in [NewLine::Lf, NewLine::CrLf] {
        let (code, _) = artifact(source, EcmaTarget::Es5, newline);
        let effect = code
            .lines()
            .find(|line| line.contains("events.push("))
            .unwrap();
        assert!(
            effect.contains(".label =") && effect.contains("; continue;"),
            "the original effect and generated transfer keep their inline clause: {effect}"
        );
    }
}

#[test]
fn a_plain_expression_implicit_constructor_keeps_the_original_class_close() {
    for source in [
        "function make(): any { return class { static value: number = 1; }; }",
        "function replace(value: any): any { return class extends value { static replacement = true; }; }",
    ] {
        for newline in [NewLine::Lf, NewLine::CrLf] {
            let (code, decoded) = artifact(source, EcmaTarget::Es5, newline);
            let returned = code.rfind("return ").unwrap() + "return ".len();
            let name = code[returned..].split(';').next().unwrap().trim();
            let constructor = code.find(&format!("function {name}(")).expect("the expression has an implicit constructor");
            let open = constructor + code[constructor..].find('{').unwrap();
            let close = open + code[open..].find('}').unwrap();
            let original = source.rfind("};").unwrap();
            assert_eq!(owner(&code, &decoded, close), json!([0, 0, original]),
                "Native plain-expression implicit constructor close: {code}");
            assert_eq!(owner(&code, &decoded, close + 1), json!([0, 0, original + 1]),
                "Native plain-expression implicit constructor endpoint: {code}");
        }
    }
}

#[test]
fn lowered_callable_initializer_trivia_keeps_the_binding_endpoint() {
    for prefix in ["", "/* 🧭 */ "] {
        let source = format!("{prefix}const calculate=async(value:number=40):Promise<number>=>{{const extra=await Promise.resolve(2);return value+extra;}};\nexport const result: Promise<number> = calculate();\n");
        let original = source[..source.find("=async").unwrap()]
            .encode_utf16()
            .count();
        for target in [
            EcmaTarget::Es5,
            EcmaTarget::Es2015,
            EcmaTarget::Es2016,
            EcmaTarget::Es2017,
        ] {
            for newline in [NewLine::Lf, NewLine::CrLf] {
                let (code, decoded) = artifact(&source, target, newline);
                let at = code.find("calculate =").unwrap() + "calculate =".len();
                assert_eq!(&code[at..at + 1], " ");
                assert_eq!(
                    owner(&code, &decoded, at),
                    json!([0, 0, original]),
                    "callable initializer separator trivia: {code}"
                );
            }
        }
    }
}

#[test]
fn a_generated_yield_group_keeps_the_previous_statement_endpoint() {
    let source = "const log:string[]=[]; async function* sequence():AsyncGenerator<string,number,number>{try{yield `v${await Promise.resolve(1)}`;}finally{log.push(\"finally\");yield `f${await Promise.resolve(2)}`;}return 3;}";
    let original = source.find(";yield `f").unwrap() + 1;
    for target in [EcmaTarget::Es2015, EcmaTarget::Es2016] {
        for newline in [NewLine::Lf, NewLine::CrLf] {
            let (code, decoded) = artifact(source, target, newline);
            let at = code.find("log.push(\"finally\");").unwrap() + "log.push(\"finally\");".len();
            assert_eq!(
                owner(&code, &decoded, at),
                json!([0, 0, original]),
                "previous statement endpoint before generated group: {code}"
            );
        }
    }
}

#[test]
fn every_retained_async_method_segment_matches_the_frozen_native_reference() {
    let source = "export class Box{value:number=40;async read(extra:number):Promise<number>{return this.value+await Promise.resolve(extra);}}\nexport const result: Promise<number> = new Box().read(2);\n";
    let reference: Value = serde_json::from_str(
        &std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/ge5_source_maps/reference.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let case = &reference["cases"][784];
    assert_eq!(
        case["id"],
        "ge1_downlevel/async-class-method-es2017-commonjs-assign-iterate/original/external"
    );
    let map = &case["maps"]["out/main.js.map"];
    assert!(map["coordinateIssues"].as_array().unwrap().is_empty());
    let raw = reference["blobs"][map["raw"].as_str().unwrap()]
        .as_str()
        .unwrap();
    let expected = maps::decoded(raw);
    for newline in [NewLine::Lf, NewLine::CrLf] {
        let (code, actual) = artifact(source, EcmaTarget::Es2017, newline);
        if newline == NewLine::Lf {
            let native = reference["blobs"][case["outputs"]["out/main.js"].as_str().unwrap()]
                .as_str()
                .unwrap();
            assert_eq!(
                code.trim_end(),
                native
                    .split("//# sourceMappingURL=")
                    .next()
                    .unwrap()
                    .trim_end()
            );
        }
        assert_eq!(
            actual["segments"], expected["segments"],
            "complete retained async method map: {code}"
        );
    }
}

#[test]
fn a_native_class_field_starts_on_its_own_generated_line() {
    let source = "class Box{value:number=40;}";
    let original = source.find("value").unwrap();
    for newline in [NewLine::Lf, NewLine::CrLf] {
        let (code, decoded) = artifact(source, EcmaTarget::Es2022, newline);
        let open = code.find('{').unwrap();
        let header = code[..open].bytes().filter(|byte| *byte == b'\n').count();
        for point in decoded["segments"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|point| {
                point.as_array().unwrap().len() > 2 && point[3] == 0 && point[4] == original
            })
        {
            assert!(
                point[0].as_u64().unwrap() as usize > header,
                "field entry must not acquire the class header line: {point}; {code}"
            );
        }
        let value = code.find("value =").unwrap();
        assert_eq!(owner(&code, &decoded, value), json!([0, 0, original]));
    }
}

fn assert_complete_frozen_map(index: usize, expected_id: &str, target: EcmaTarget) {
    assert_complete_frozen_module_map(index, expected_id, target, "main");
}

fn assert_complete_frozen_module_map(
    index: usize,
    expected_id: &str,
    target: EcmaTarget,
    stem: &str,
) {
    let reference: Value = serde_json::from_str(
        &std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/ge5_source_maps/reference.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let case = &reference["cases"][index];
    assert_eq!(case["id"], expected_id);
    let map_name = format!("out/{stem}.js.map");
    let map = &case["maps"][map_name.as_str()];
    assert!(map["coordinateIssues"].as_array().unwrap().is_empty());
    let raw = reference["blobs"][map["raw"].as_str().unwrap()]
        .as_str()
        .unwrap();
    let expected = maps::decoded(raw);
    let source = expected["metadata"]["sourcesContent"][0].as_str().unwrap();
    let output_name = format!("out/{stem}.js");
    let native = reference["blobs"][case["outputs"][output_name.as_str()].as_str().unwrap()]
        .as_str()
        .unwrap()
        .split("//# sourceMappingURL=")
        .next()
        .unwrap()
        .trim_end();
    for newline in [NewLine::Lf, NewLine::CrLf] {
        let (code, actual) = artifact(source, target, newline);
        let expected_code = if newline == NewLine::CrLf {
            native.replace('\n', "\r\n")
        } else {
            native.to_owned()
        };
        assert_eq!(code.trim_end(), expected_code, "frozen case {expected_id}");
        assert_eq!(
            actual["segments"], expected["segments"],
            "complete map for {expected_id}: {code}"
        );
    }
}

#[test]
fn retained_callable_entries_and_statement_ends_match_every_native_segment() {
    for (index, id, target) in [
        (820, "ge1_downlevel/async-object-method-es2017-commonjs-assign-iterate/original/external", EcmaTarget::Es2017),
        (644, "ge1_downlevel/nested-parameter-es2015-commonjs-assign-iterate/original/external", EcmaTarget::Es2015),
        (408, "ge1_downlevel/super-write-receiver-order-es2015-commonjs-assign-iterate/original/external", EcmaTarget::Es2015),
        (856, "ge1_downlevel/async-super-method-es2017-commonjs-assign-iterate/original/external", EcmaTarget::Es2017),
    ] {
        assert_complete_frozen_map(index, id, target);
    }
}

#[test]
fn nested_binding_access_chains_match_every_native_segment() {
    assert_complete_frozen_map(
        544,
        "ge1_downlevel/object-nested-binding-es5-commonjs-assign-iterate/original/external",
        EcmaTarget::Es5,
    );
}

#[test]
fn es5_copied_class_bodies_and_leading_comments_match_every_native_segment() {
    for (index, variant) in [(264, "original"), (265, "crlfAstral")] {
        let id = format!(
            "ge1_downlevel/called-imported-heritage-es5-commonjs-assign-iterate/{variant}/external"
        );
        assert_complete_frozen_module_map(index, &id, EcmaTarget::Es5, "base");
    }
}

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#[test]
fn a_standard_decorator_implicit_constructor_keeps_its_generated_close_unmapped() {
    let reference: Value = serde_json::from_str(
        &std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/ge5_source_maps/reference.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let case = &reference["cases"][6182];
    assert_eq!(case["id"], "ge3_decorator_compositions/ge3-decorators-rejected-double-decorated-accessors-standard-es5-commonjs-assign/original/external");
    let map = &case["maps"]["out/main.js.map"];
    assert!(map["coordinateIssues"].as_array().unwrap().is_empty());
    let expected = maps::decoded(
        reference["blobs"][map["raw"].as_str().unwrap()]
            .as_str()
            .unwrap(),
    );
    let source = expected["metadata"]["sourcesContent"][0].as_str().unwrap();
    let native = reference["blobs"][case["outputs"]["out/main.js"].as_str().unwrap()]
        .as_str()
        .unwrap();
    let close = |code: &str| {
        let constructor = code
            .find("function C(")
            .expect("the retained class C constructor");
        let open = constructor + code[constructor..].find('{').unwrap();
        open + code[open..].find('}').unwrap()
    };
    assert_eq!(owner(native, &expected, close(native)), Value::Null);
    assert_eq!(owner(native, &expected, close(native) + 1), Value::Null);
    for newline in [NewLine::Lf, NewLine::CrLf] {
        let (code, actual) = artifact(source, EcmaTarget::Es5, newline);
        let closing = close(&code);
        assert_eq!(
            owner(&code, &actual, closing),
            Value::Null,
            "Native standard-decorator implicit constructor close must remain generated: {code}"
        );
        assert_eq!(
            owner(&code, &actual, closing + 1),
            Value::Null,
            "Native standard-decorator implicit constructor endpoint must remain generated: {code}"
        );
    }
}

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#[test]
fn a_derived_super_statement_keeps_its_endpoint_before_generated_return_trivia() {
    for (declaration, statement) in [
        ("class Base { constructor(value: number) {} } export class Derived extends Base { constructor() {super(40);} }", "super(40);"),
        ("class Base { constructor(...values: number[]) {} } export class Derived extends Base { constructor(...values: number[]) {super(...values);} }", "super(...values);"),
    ] {
        for prefix in ["", "/* 🧭 */ "] {
            let source = format!("{prefix}{declaration}");
            let original = source.find(statement).unwrap() + statement.len();
            let expected = json!([0, 0, source[..original].encode_utf16().count()]);
            for newline in [NewLine::Lf, NewLine::CrLf] {
                let (code, decoded) = artifact(&source, EcmaTarget::Es5, newline);
                let call = code.find(" = __blueice_target_call_base_")
                    .expect("the explicit super call has a retained receiver assignment");
                let endpoint = call + code[call..].find(';').unwrap() + 1;
                assert_eq!(owner(&code, &decoded, endpoint), expected,
                    "the complete original super statement ends before the generated receiver return: {code}");
            }
        }
    }
}

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#[test]
fn a_final_native_class_field_keeps_every_frozen_statement_endpoint() {
    for (index, variant) in [(402, "original"), (403, "crlfAstral")] {
        let id = format!("ge1_downlevel/computed-super-call-order-es2022-commonjs-define-iterate/{variant}/external");
        assert_complete_frozen_map(index, &id, EcmaTarget::Es2022);
    }
}

#[test]
fn retained_async_expression_arrows_keep_every_frozen_segment() {
    for (index, variant, target, product) in [
        (
            712,
            "original",
            EcmaTarget::Es2017,
            "es2017-commonjs-assign",
        ),
        (
            713,
            "crlfAstral",
            EcmaTarget::Es2017,
            "es2017-commonjs-assign",
        ),
        (
            730,
            "original",
            EcmaTarget::Es2022,
            "es2022-commonjs-define",
        ),
        (
            731,
            "crlfAstral",
            EcmaTarget::Es2022,
            "es2022-commonjs-define",
        ),
    ] {
        let id =
            format!("ge1_downlevel/async-arrow-lexical-this-{product}-iterate/{variant}/external");
        assert_complete_frozen_map(index, &id, target);
    }
}

#[test]
fn a_callable_named_async_keeps_ordinary_call_parentheses_adjacent() {
    let source =
        "function async(value:number):number{return value+1;}export const result:number=async(41);";
    for newline in [NewLine::Lf, NewLine::CrLf] {
        let (code, _) = artifact(source, EcmaTarget::Es2017, newline);
        assert!(code.contains("function async(value)"), "{code}");
        assert!(code.contains("async(41)"), "{code}");
    }
}

#[test]
fn external_object_helpers_preserve_every_native_segment_without_synthetic_events() {
    let reference: Value = serde_json::from_str(
        &std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/ge5_source_maps/reference.json"),
        )
        .unwrap(),
    )
    .unwrap();
    for (index, variant) in [(2442, "original"), (2443, "crlfAstral")] {
        let case = &reference["cases"][index];
        assert_eq!(
            case["id"],
            format!("ge2_module_helpers/object-rest-spread-cjs-es5-i0-n1/{variant}/external")
        );
        let record = &case["maps"]["out/main.js.map"];
        assert!(record["coordinateIssues"].as_array().unwrap().is_empty());
        let decoded = maps::decoded(
            reference["blobs"][record["raw"].as_str().unwrap()]
                .as_str()
                .unwrap(),
        );
        let source = decoded["metadata"]["sourcesContent"][0].as_str().unwrap();
        let native = reference["blobs"][case["outputs"]["out/main.js"].as_str().unwrap()]
            .as_str()
            .unwrap()
            .split("//# sourceMappingURL=")
            .next()
            .unwrap()
            .trim_end();
        for newline in [NewLine::Lf, NewLine::CrLf] {
            let id = "memory:///main.ts";
            let result = compile(
                id,
                &MapLoader::from([ModuleSource::new(id, source)]),
                CompilerOptions {
                    target: EcmaTarget::Es5,
                    libraries: Some(vec![EcmaTarget::Es2022]),
                    module_kind: ModuleKind::CommonJs,
                    source_map: true,
                    no_emit_helpers: true,
                    javascript_layout: JavaScriptLayout::TypeScript,
                    new_line: newline,
                    ..CompilerOptions::default()
                },
            );
            assert!(!result.has_errors(), "{:?}", result.diagnostics);
            let artifact = &result.output.as_ref().unwrap().artifacts[id];
            let expected = if newline == NewLine::CrLf {
                native.replace('\n', "\r\n")
            } else {
                native.to_owned()
            };
            assert_eq!(
                artifact.javascript.trim_end(),
                expected,
                "native helper JavaScript"
            );
            assert_eq!(
                maps::decoded(&artifact.source_map.as_ref().unwrap().to_json())["segments"],
                decoded["segments"],
                "complete external helper map: {}",
                artifact.javascript
            );
        }
    }
}

#[test]
fn a_retained_ordinary_arrow_has_no_synthetic_event_inside_its_token() {
    for (index, variant) in [(5588, "original"), (5589, "crlfAstral")] {
        let id =
            format!("ge2_module_helpers/helper-name-collision-cjs-es2017-i0-n0/{variant}/external");
        assert_complete_frozen_map(index, &id, EcmaTarget::Es2017);
    }
}

fn function_completion(code: &str, name: &str) -> (usize, usize) {
    let tokens = blueice_bluets::lex("memory:///generated.js", code).unwrap();
    let start = tokens
        .windows(2)
        .position(|pair| pair[0].is("function") && pair[1].is(name))
        .unwrap();
    let open = (start..tokens.len())
        .find(|&index| tokens[index].is("{"))
        .unwrap();
    let mut depth = 0;
    let close = (open..tokens.len())
        .find(|&index| {
            if tokens[index].is("{") {
                depth += 1;
            }
            if tokens[index].is("}") {
                depth -= 1;
            }
            depth == 0
        })
        .unwrap();
    let callback = tokens[open..close]
        .iter()
        .rfind(|token| token.is("}"))
        .unwrap();
    (callback.end, tokens[close].end)
}

fn frozen_native_map(reference: &Value, case: &Value, name: &str) -> Value {
    let record = &case["maps"][name];
    let raw = record["raw"].as_str().unwrap();
    if record["coordinateIssues"].as_array().unwrap().is_empty() {
        return maps::decoded(reference["blobs"][raw].as_str().unwrap());
    }
    // Use the same exact, reproduced correction ledger as the original
    // verifier. Stock System maps contain a known out-of-bounds EOF point.
    let bytes = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/ge5_source_maps/native-map-defects.json"),
    )
    .unwrap();
    let hash = |bytes: &[u8]| {
        ring::digest::digest(&ring::digest::SHA256, bytes)
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    assert_eq!(
        hash(&bytes),
        "097a9b0df2857de39e2210e427c21f3dbcadebb4f8ed8d9dabbe5f0a87b04007"
    );
    let ledger: Value = serde_json::from_slice(&bytes).unwrap();
    let defect = ledger["defects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|defect| defect["caseId"] == case["id"] && defect["map"] == name)
        .unwrap();
    assert_eq!(defect["stockRawSha256"], raw);
    assert_eq!(record["coordinateIssues"], json!([defect["issue"]]));
    let corrected = ledger["patchedBlobs"][defect["patchedRawSha256"].as_str().unwrap()]
        .as_str()
        .unwrap();
    assert_eq!(hash(corrected.as_bytes()), defect["patchedRawSha256"]);
    maps::decoded(corrected)
}

#[test]
fn lowered_async_generator_completion_keeps_native_ownership_through_wrapper_indentation() {
    let reference: Value = serde_json::from_str(
        &std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/ge5_source_maps/reference.json"),
        )
        .unwrap(),
    )
    .unwrap();
    for (module, kind) in [
        ("amd", ModuleKind::Amd),
        ("umd", ModuleKind::Umd),
        ("system", ModuleKind::System),
    ] {
        for (target_name, target) in [
            ("es2015", EcmaTarget::Es2015),
            ("es2017", EcmaTarget::Es2017),
        ] {
            for variant in ["original", "crlfAstral"] {
                let id = format!("ge2_module_helpers/async-generator-finally-{module}-{target_name}-i0-n0/{variant}/external");
                let case = reference["cases"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|case| case["id"] == id)
                    .unwrap();
                let native_map = frozen_native_map(&reference, case, "out/main.js.map");
                let source = native_map["metadata"]["sourcesContent"][0]
                    .as_str()
                    .unwrap();
                let native = reference["blobs"][case["outputs"]["out/main.js"].as_str().unwrap()]
                    .as_str()
                    .unwrap();
                let (begin, end) = function_completion(native, "sequence");
                let expected = owner(native, &native_map, begin);
                assert!(!expected.is_null());
                for at in begin..end {
                    assert_eq!(owner(native, &native_map, at), expected);
                }
                for newline in [NewLine::Lf, NewLine::CrLf] {
                    let entry = "memory:///main.ts";
                    let result = compile(
                        entry,
                        &MapLoader::from([ModuleSource::new(entry, source)]),
                        CompilerOptions {
                            target,
                            module_kind: kind,
                            source_map: true,
                            libraries: Some(vec![EcmaTarget::Es2022]),
                            javascript_layout: JavaScriptLayout::TypeScript,
                            new_line: newline,
                            ..CompilerOptions::default()
                        },
                    );
                    assert!(!result.has_errors(), "{:?}", result.diagnostics);
                    let artifact = &result.output.as_ref().unwrap().artifacts[entry];
                    let actual = maps::decoded(&artifact.source_map.as_ref().unwrap().to_json());
                    let code = &artifact.javascript;
                    let (begin, end) = function_completion(code, "sequence");
                    for at in begin..end {
                        assert_eq!(owner(code, &actual, at), expected, "complete generated driver/function ending for {id}, {newline:?}, byte {at}: {}", &code[begin..end]);
                    }
                    assert_eq!(
                        owner(code, &actual, end),
                        owner(
                            native,
                            &native_map,
                            function_completion(native, "sequence").1
                        )
                    );
                }
            }
        }
    }
}

#[test]
fn a_generated_iterator_capture_resets_source_before_its_flag_gap() {
    for prefix in ["", "/* 🧭 */ "] {
        let source = format!("{prefix}const values:number[]=[1,2]; let total:number=0; for (const value of values) {{ total+=value;break; }} export const result:number=total;");
        let binding_end = source[..source.find("value of").unwrap() + "value".len()]
            .encode_utf16()
            .count();
        for newline in [NewLine::Lf, NewLine::CrLf] {
            let entry = "memory:///main.ts";
            let result = compile(
                entry,
                &MapLoader::from([ModuleSource::new(entry, source.clone())]),
                CompilerOptions {
                    target: EcmaTarget::Es5,
                    module_kind: ModuleKind::CommonJs,
                    source_map: true,
                    downlevel_iteration: true,
                    libraries: Some(vec![EcmaTarget::Es2022]),
                    javascript_layout: JavaScriptLayout::TypeScript,
                    new_line: newline,
                    ..CompilerOptions::default()
                },
            );
            assert!(!result.has_errors(), "{:?}", result.diagnostics);
            let artifact = &result.output.as_ref().unwrap().artifacts[entry];
            let code = &artifact.javascript;
            let decoded = maps::decoded(&artifact.source_map.as_ref().unwrap().to_json());
            let begin = code.rfind(".value;").unwrap() + ".value;".len();
            assert_eq!(owner(code, &decoded, begin - 1), json!([0, 0, binding_end]));
            let end = begin
                + code[begin..]
                    .find("__blueice_target_iterator_finished_")
                    .unwrap();
            assert!(code[begin..end].contains('\n'));
            for at in begin..end {
                assert_eq!(
                    owner(code, &decoded, at),
                    Value::Null,
                    "generated iterator capture/flag gap at {at}, {newline:?}"
                );
            }
        }
    }
}

#[test]
fn source_map_publication_does_not_change_lowered_rest_callable_javascript() {
    let source = "function wrap():any{return function(...args:any[]):any{return args[0];};} export const result:number=wrap()(42);";
    let entry = "memory:///main.ts";
    for layout in [
        JavaScriptLayout::PreserveSource,
        JavaScriptLayout::TypeScript,
    ] {
        let mut options = CompilerOptions {
            target: EcmaTarget::Es5,
            module_kind: ModuleKind::CommonJs,
            libraries: Some(vec![EcmaTarget::Es2022]),
            javascript_layout: layout,
            ..CompilerOptions::default()
        };
        let loader = MapLoader::from([ModuleSource::new(entry, source)]);
        let plain = compile(entry, &loader, options.clone());
        options.source_map = true;
        let mapped = compile(entry, &loader, options);
        assert!(!plain.has_errors(), "{:?}", plain.diagnostics);
        assert!(!mapped.has_errors(), "{:?}", mapped.diagnostics);
        let plain = &plain.output.as_ref().unwrap().artifacts[entry];
        let mapped = &mapped.output.as_ref().unwrap().artifacts[entry];
        assert!(plain.source_map.is_none());
        assert!(mapped.source_map.is_some());
        assert_eq!(
            plain.javascript, mapped.javascript,
            "map publication changes JavaScript for {layout:?}"
        );
    }
}

#[test]
fn map_publication_keeps_the_complete_frozen_decorator_replacement_body_identical() {
    let reference: Value = serde_json::from_str(
        &std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/ge5_source_maps/reference.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let case = &reference["cases"][6326];
    let native = frozen_native_map(&reference, case, "out/main.js.map");
    let source = native["metadata"]["sourcesContent"][0].as_str().unwrap();
    let entry = "memory:///main.ts";
    let loader = MapLoader::from([ModuleSource::new(entry, source)]);
    let mut options = CompilerOptions {
        target: EcmaTarget::Es5,
        module_kind: ModuleKind::CommonJs,
        libraries: Some(vec![EcmaTarget::Es2022]),
        javascript_layout: JavaScriptLayout::TypeScript,
        ..CompilerOptions::default()
    };
    let plain = compile(entry, &loader, options.clone());
    options.source_map = true;
    let mapped = compile(entry, &loader, options);
    assert!(!plain.has_errors(), "{:?}", plain.diagnostics);
    assert!(!mapped.has_errors(), "{:?}", mapped.diagnostics);
    let plain = &plain.output.as_ref().unwrap().artifacts[entry];
    let mapped = &mapped.output.as_ref().unwrap().artifacts[entry];
    assert!(plain.source_map.is_none());
    assert!(mapped.source_map.is_some());
    assert_eq!(
        plain.javascript, mapped.javascript,
        "complete frozen source: {}",
        case["id"]
    );
}

#[test]
fn unpublished_layout_boundaries_keep_the_existing_segment_budget_and_no_public_map() {
    let entry = "memory:///main.ts";
    let mut options = CompilerOptions {
        source_map: false,
        javascript_layout: JavaScriptLayout::TypeScript,
        ..CompilerOptions::default()
    };
    options.limits.max_source_map_segments = 0;
    let result = compile(
        entry,
        &MapLoader::from([ModuleSource::new(entry, "export const result:number=42;")]),
        options,
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let artifact = &result.output.as_ref().unwrap().artifacts[entry];
    assert!(artifact.source_map.is_none());
    assert!(artifact.javascript.contains("42"));
}

struct StarOwnerLoader(MapLoader);
impl blueice_bluets::ModuleLoader for StarOwnerLoader {
    fn load(&self, module_id: &str) -> Result<ModuleSource, String> {
        blueice_bluets::ModuleLoader::load(&self.0, module_id)
    }
    fn resolve(&self, from: &str, specifier: &str) -> Result<String, String> {
        if from == "memory:///main.ts" && specifier == "./dep.js" {
            Ok("memory:///dep.ts".into())
        } else {
            Err("outside the authorized star-export edge".into())
        }
    }
}

#[test]
fn original_star_publication_owns_its_inline_forwarder_through_inserted_newlines() {
    for prefix in ["", "/* 🧭 */ "] {
        let source = format!("{prefix}export * from './dep.js';\nexport const result:number=1;");
        let start = json!([0, 0, prefix.encode_utf16().count()]);
        let end = json!([
            0,
            0,
            source[..source.find(';').unwrap() + 1]
                .encode_utf16()
                .count()
        ]);
        for kind in [
            ModuleKind::CommonJs,
            ModuleKind::Amd,
            ModuleKind::Umd,
            ModuleKind::System,
        ] {
            for newline in [NewLine::Lf, NewLine::CrLf] {
                let entry = "memory:///main.ts";
                let loader = StarOwnerLoader(MapLoader::from([
                    ModuleSource::new(entry, source.clone()),
                    ModuleSource::new("memory:///dep.ts", "export const value:number=42;"),
                ]));
                let result = compile(
                    entry,
                    &loader,
                    CompilerOptions {
                        target: EcmaTarget::Es5,
                        module_kind: kind,
                        source_map: true,
                        libraries: Some(vec![EcmaTarget::Es2022]),
                        javascript_layout: JavaScriptLayout::TypeScript,
                        new_line: newline,
                        ..CompilerOptions::default()
                    },
                );
                assert!(!result.has_errors(), "{:?}", result.diagnostics);
                let artifact = &result.output.as_ref().unwrap().artifacts[entry];
                let code = &artifact.javascript;
                let map = maps::decoded(&artifact.source_map.as_ref().unwrap().to_json());
                let tokens = blueice_bluets::lex("memory:///emitted.js", code).unwrap();
                let first = tokens.iter().position(|token| token.is("for")).unwrap();
                let open = (first..tokens.len()).find(|&i| tokens[i].is("{")).unwrap();
                let mut depth = 0;
                let close = (open..tokens.len())
                    .find(|&i| {
                        if tokens[i].is("{") {
                            depth += 1;
                        }
                        if tokens[i].is("}") {
                            depth -= 1;
                        }
                        depth == 0
                    })
                    .unwrap();
                let expected = if kind == ModuleKind::System {
                    Value::Null
                } else {
                    start.clone()
                };
                for at in tokens[first].start..tokens[close].end {
                    assert_eq!(
                        owner(code, &map, at),
                        expected,
                        "complete inline star publication {kind:?}/{newline:?} at {at}"
                    );
                }
                if kind != ModuleKind::System {
                    assert_eq!(
                        owner(code, &map, tokens[close].end),
                        end,
                        "exact original star endpoint"
                    );
                }
            }
        }
    }
}

struct InteropOwnerLoader(MapLoader);
impl blueice_bluets::ModuleLoader for InteropOwnerLoader {
    fn load(&self, module_id: &str) -> Result<ModuleSource, String> {
        blueice_bluets::ModuleLoader::load(&self.0, module_id)
    }
    fn resolve(&self, from: &str, specifier: &str) -> Result<String, String> {
        if from == "memory:///main.ts" {
            match specifier {
                "./dep.js" => return Ok("memory:///dep.ts".into()),
                "./left.js" => return Ok("memory:///left.ts".into()),
                _ => {}
            }
        }
        Err("outside the authorized interop edges".into())
    }
}

#[test]
fn retained_namespace_import_and_star_export_headers_keep_every_native_segment() {
    let reference: Value = serde_json::from_str(
        &std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/ge5_source_maps/reference.json"),
        )
        .unwrap(),
    )
    .unwrap();
    for (index, variant) in [(4892, "original"), (4893, "crlfAstral")] {
        let case = &reference["cases"][index];
        assert_eq!(
            case["id"],
            format!("ge2_module_helpers/interop-exportstar-cycle-esm-es5-i0-n0/{variant}/external")
        );
        let decoded = frozen_native_map(&reference, case, "out/main.js.map");
        let source = decoded["metadata"]["sourcesContent"][0].as_str().unwrap();
        let native = reference["blobs"][case["outputs"]["out/main.js"].as_str().unwrap()]
            .as_str()
            .unwrap()
            .split("//# sourceMappingURL=")
            .next()
            .unwrap()
            .trim_end();
        let entry = "memory:///main.ts";
        let loader = InteropOwnerLoader(MapLoader::from([
            ModuleSource::new(entry,source),
            ModuleSource::new("memory:///dep.ts", "export default function add(n:number):number{return n+1;} export const value:number=40;"),
            ModuleSource::new("memory:///left.ts", "export function read():number{return 1;}")
        ]));
        for newline in [NewLine::Lf, NewLine::CrLf] {
            let result = compile(
                entry,
                &loader,
                CompilerOptions {
                    target: EcmaTarget::Es5,
                    module_kind: ModuleKind::Esm,
                    es_module_interop: true,
                    source_map: true,
                    libraries: Some(vec![EcmaTarget::Es2022]),
                    javascript_layout: JavaScriptLayout::TypeScript,
                    new_line: newline,
                    ..CompilerOptions::default()
                },
            );
            assert!(!result.has_errors(), "{:?}", result.diagnostics);
            let actual = &result.output.as_ref().unwrap().artifacts[entry];
            let expected = if newline == NewLine::CrLf {
                native.replace('\n', "\r\n")
            } else {
                native.to_owned()
            };
            assert_eq!(actual.javascript.trim_end(), expected);
            assert_eq!(
                maps::decoded(&actual.source_map.as_ref().unwrap().to_json())["segments"],
                decoded["segments"],
                "complete retained interop map for {variant}/{newline:?}"
            );
        }
    }
}

#[test]
fn retained_private_brand_operands_keep_the_return_endpoint_and_every_native_segment() {
    for (index, variant) in [(3868, "original"), (3869, "crlfAstral")] {
        let id = format!("ge2_module_helpers/private-accessor-cjs-es2022-i0-n0/{variant}/external");
        assert_complete_frozen_map(index, &id, EcmaTarget::Es2022);
    }
}

#[test]
fn a_compact_private_brand_starts_after_return_trivia() {
    for prefix in ["", "/* 🧭 */ "] {
        let source = format!("{prefix}export class Box{{#value=1;has(other:object):boolean{{return#value in other;}}}}");
        let keyword = source[..source.find("return#").unwrap()]
            .encode_utf16()
            .count();
        let operand = keyword + "return".len();
        for newline in [NewLine::Lf, NewLine::CrLf] {
            let (code, map) = artifact(&source, EcmaTarget::Es2022, newline);
            let start = code.find("return #value").unwrap();
            assert_eq!(
                owner(&code, &map, start + "return".len()),
                json!([0, 0, keyword])
            );
            assert_eq!(
                owner(&code, &map, start + "return ".len()),
                json!([0, 0, operand])
            );
        }
    }
}

#[test]
fn a_standard_class_decorator_preserves_the_original_implicit_constructor_close() {
    let reference: Value = serde_json::from_str(
        &std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/ge5_source_maps/reference.json"),
        )
        .unwrap(),
    )
    .unwrap();
    for (index, variant) in [(6518, "original"), (6519, "crlfAstral")] {
        let case = &reference["cases"][index];
        assert_eq!(case["id"], format!("ge3_decorator_compositions/ge3-decorators-static-super-computed-reads-standard-es5-commonjs-assign/{variant}/external"));
        let expected = frozen_native_map(&reference, case, "out/main.js.map");
        let source = expected["metadata"]["sourcesContent"][0].as_str().unwrap();
        let native = reference["blobs"][case["outputs"]["out/main.js"].as_str().unwrap()]
            .as_str()
            .unwrap();
        let close = |code: &str| {
            let constructor = code
                .find("function C")
                .or_else(|| {
                    let factory = code.find("var _classSuper = Base;")?;
                    Some(factory + code[factory..].find("function _btsClass")?)
                })
                .expect("the original C class's implicit constructor");
            constructor + code[constructor..].find('}').unwrap()
        };
        let native_close = close(native);
        assert_ne!(owner(native, &expected, native_close), Value::Null);
        assert_ne!(owner(native, &expected, native_close + 1), Value::Null);
        for newline in [NewLine::Lf, NewLine::CrLf] {
            let (code, actual) = artifact(source, EcmaTarget::Es5, newline);
            let actual_close = close(&code);
            for endpoint in 0..=1 {
                assert_eq!(
                    owner(&code, &actual, actual_close + endpoint),
                    owner(native, &expected, native_close + endpoint),
                    "retained decorated class constructor ending {index} + {endpoint}: {code}"
                );
            }
        }
    }
}

#[test]
fn class_and_instance_decorators_keep_distinct_implicit_constructor_end_owners() {
    for heritage in ["", "extends Base "] {
        for prefix in ["", "/* 🧭 */ "] {
            for (member, generated) in [
                ("", false),
                ("static x = 1;", false),
                ("x = 1;", false),
                ("f() {}", false),
                ("@dec static f() {}", false),
                ("@dec f() {}", true),
                ("@dec x = 1;", true),
                ("@dec get x() { return 1; }", true),
            ] {
                let source = format!("{prefix}function dec(value: any, context: any): any {{ return value; }} class Base {{}} @dec export class C {heritage}{{ {member} }}");
                let original_close = source.rfind('}').unwrap();
                for newline in [NewLine::Lf, NewLine::CrLf] {
                    let (code, actual) = artifact(&source, EcmaTarget::Es5, newline);
                    let constructor = code
                        .find("function C")
                        .or_else(|| code.find("function _btsClass"))
                        .expect("the original C constructor");
                    let close = constructor + code[constructor..].find('}').unwrap();
                    for endpoint in 0..=1 {
                        let expected = if generated {
                            Value::Null
                        } else {
                            json!([
                                0,
                                0,
                                source[..original_close + endpoint].encode_utf16().count()
                            ])
                        };
                        assert_eq!(
                            owner(&code, &actual, close + endpoint),
                            expected,
                            "constructor end {member:?}, {heritage:?}, endpoint {endpoint}: {code}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn tagged_template_entries_keep_every_native_segment_after_inserted_trivia() {
    let reference: Value = serde_json::from_str(
        &std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/ge5_source_maps/reference.json"),
        )
        .unwrap(),
    )
    .unwrap();
    for (index, format, variant, module_kind) in [
        (3516, "cjs-es2022-i0-n0", "original", ModuleKind::CommonJs),
        (3517, "cjs-es2022-i0-n0", "crlfAstral", ModuleKind::CommonJs),
        (
            3842,
            "nodenext-esm-es2022-i1-n1",
            "original",
            ModuleKind::Esm,
        ),
        (
            3843,
            "nodenext-esm-es2022-i1-n1",
            "crlfAstral",
            ModuleKind::Esm,
        ),
    ] {
        let case = &reference["cases"][index];
        assert_eq!(
            case["id"],
            format!("ge2_module_helpers/tagged-template-{format}/{variant}/external")
        );
        let expected = frozen_native_map(&reference, case, "out/main.js.map");
        let source = expected["metadata"]["sourcesContent"][0].as_str().unwrap();
        let native = reference["blobs"][case["outputs"]["out/main.js"].as_str().unwrap()]
            .as_str()
            .unwrap()
            .split("//# sourceMappingURL=")
            .next()
            .unwrap()
            .trim_end();
        for newline in [NewLine::Lf, NewLine::CrLf] {
            let id = "memory:///main.ts";
            let result = compile(
                id,
                &MapLoader::from([ModuleSource::new(id, source)]),
                CompilerOptions {
                    target: EcmaTarget::Es2022,
                    module_kind,
                    source_map: true,
                    javascript_layout: JavaScriptLayout::TypeScript,
                    new_line: newline,
                    ..CompilerOptions::default()
                },
            );
            assert!(!result.has_errors(), "{:?}", result.diagnostics);
            let artifact = &result.output.as_ref().unwrap().artifacts[id];
            let code = &artifact.javascript;
            let expected_code = if newline == NewLine::CrLf {
                native.replace('\n', "\r\n")
            } else {
                native.to_owned()
            };
            assert_eq!(code.trim_end(), expected_code);
            let actual = maps::decoded(&artifact.source_map.as_ref().unwrap().to_json());
            assert_eq!(
                actual["segments"], expected["segments"],
                "complete tagged template {index}: {code}"
            );
        }
    }
}

#[test]
fn adjacent_decorated_classes_keep_distinct_original_constructor_endings() {
    for prefix in ["", "/* 🧭 */\r\n"] {
        let source = format!("{prefix}function dec(value: any, context: any): any {{ return value; }} @dec export class A {{}} @dec export class B {{}}");
        let endings = [
            source.find("class A {}").unwrap() + "class A {".len(),
            source.find("class B {}").unwrap() + "class B {".len(),
        ];
        for newline in [NewLine::Lf, NewLine::CrLf] {
            let (code, actual) = artifact(&source, EcmaTarget::Es5, newline);
            let constructors = code
                .match_indices("function _btsClass")
                .map(|(at, _)| at + code[at..].find('}').unwrap())
                .collect::<Vec<_>>();
            assert_eq!(constructors.len(), 2, "{code}");
            for (close, original) in constructors.into_iter().zip(endings) {
                for endpoint in 0..=1 {
                    let prefix = &source[..original + endpoint];
                    assert_eq!(
                        owner(&code, &actual, close + endpoint),
                        json!([
                            0,
                            prefix.bytes().filter(|b| *b == b'\n').count(),
                            prefix.rsplit('\n').next().unwrap().encode_utf16().count()
                        ]),
                        "distinct original decorated class ending: {code}"
                    );
                }
            }
        }
    }
}

#[path = "native_layout_provenance/class_origins.rs"]
mod class_origins;

#[path = "native_layout_provenance/signatures.rs"]
mod signatures;

#[path = "native_layout_provenance/namespaces.rs"]
mod namespaces;

#[path = "native_layout_provenance/legacy_auto.rs"]
mod legacy_auto;

#[path = "native_layout_provenance/retained_edges.rs"]
mod retained_edges;
