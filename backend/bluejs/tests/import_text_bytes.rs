// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `with { type: "text" }` (tc39/proposal-import-text) and `with { type:
//! "bytes" }` (tc39/proposal-import-bytes) through parse, compile and the
//! VM's module graph. Both produce a Synthetic Module Record whose only
//! export is `default` (CreateDefaultExportSyntheticModule): a String for
//! text, a `Uint8Array` over an immutable `ArrayBuffer` for bytes. The
//! `type` attribute is part of a request's identity, so one resource can be
//! a JavaScript module, a JSON module, a text module and a bytes module at
//! once.

use blueice_bluejs::{
    compile, compile_module, parse, parse_module, Bytecode, HeapConfig, RuntimeError, Value, Vm,
    VmConfig,
};
use std::collections::HashMap;

fn build(sources: &[(&str, &str)]) -> HashMap<String, Bytecode> {
    sources
        .iter()
        .map(|(name, source)| {
            let module = parse_module(source).unwrap_or_else(|e| panic!("{name}: {e:?}"));
            (
                format!("t/{name}"),
                compile_module(&module).unwrap_or_else(|e| panic!("{name}: {e:?}")),
            )
        })
        .collect()
}

/// A VM whose host supplies `text` (already decoded) and `bytes` resources
/// under `t/`-relative names.
fn vm_with(config: VmConfig, text: &[(&str, &str)], bytes: &[(&str, &[u8])]) -> Vm {
    let mut vm = Vm::new(config).unwrap();
    vm.set_text_module_sources(
        text.iter()
            .map(|(name, value)| (format!("t/{name}"), value.to_string()))
            .collect(),
    );
    vm.set_bytes_module_sources(
        bytes
            .iter()
            .map(|(name, value)| (format!("t/{name}"), value.to_vec()))
            .collect(),
    );
    vm
}

fn run(
    sources: &[(&str, &str)],
    text: &[(&str, &str)],
    bytes: &[(&str, &[u8])],
) -> Result<Value, RuntimeError> {
    vm_with(VmConfig::default(), text, bytes).execute_module_graph("t/main.js", &build(sources))
}

fn string(value: &str) -> Value {
    Value::String(value.into())
}

#[test]
fn a_text_import_binds_the_resource_as_a_string() {
    let result = run(
        &[(
            "main.js",
            "import value from './data.txt' with { type: 'text' };
             typeof value + ':' + value + ':' + value.length",
        )],
        &[("data.txt", "a string value\n")],
        &[],
    );
    assert_eq!(result, Ok(string("string:a string value\n:15")));
}

#[test]
fn an_empty_text_resource_is_the_empty_string() {
    let result = run(
        &[(
            "main.js",
            "import value from './empty' with { type: 'text' };
             typeof value + '|' + value + '|'",
        )],
        &[("empty", "")],
        &[],
    );
    assert_eq!(result, Ok(string("string||")));
}

#[test]
fn text_keeps_non_ascii_code_points_as_utf16() {
    // U+1F600 is a surrogate pair: two code units in the String.
    let result = run(
        &[(
            "main.js",
            "import value from './data' with { type: 'text' };
             value.length + ':' + value.codePointAt(1)",
        )],
        &[("data", "é😀")],
        &[],
    );
    assert_eq!(result, Ok(string("3:128512")));
}

#[test]
fn a_text_namespace_has_exactly_one_default_export() {
    let result = run(
        &[(
            "main.js",
            "import * as ns from './data' with { type: 'text' };
             Object.getOwnPropertyNames(ns).length + ':' + typeof ns.default",
        )],
        &[("data", "262\n")],
        &[],
    );
    assert_eq!(result, Ok(string("1:string")));
}

#[test]
fn a_named_binding_of_a_text_module_is_a_resolution_error() {
    let result = run(
        &[(
            "main.js",
            "$DONOTEVALUATE(); import { name } from './data' with { type: 'text' };",
        )],
        &[("data", "x")],
        &[],
    );
    assert!(
        matches!(result, Err(RuntimeError::ModuleResolution(_))),
        "{result:?}"
    );
}

#[test]
fn a_module_can_import_its_own_path_as_text() {
    // The text record for `t/main.js` and the Source Text Module `t/main.js`
    // share a resolved path but not a request identity.
    let result = run(
        &[(
            "main.js",
            "import self from './main.js' with { type: 'text' };
             self",
        )],
        &[("main.js", "the source of main.js")],
        &[],
    );
    assert_eq!(result, Ok(string("the source of main.js")));
}

#[test]
fn one_resource_can_be_javascript_json_text_and_bytes_at_once() {
    let mut vm = vm_with(
        VmConfig::default(),
        &[("data.js", "TEXT")],
        &[("data.js", &[7, 8, 9])],
    );
    vm.set_json_module_sources(HashMap::from([(
        "t/data.js".to_string(),
        "{\"n\":1}".to_string(),
    )]));
    let result = vm.execute_module_graph(
        "t/main.js",
        &build(&[
            (
                "main.js",
                "import { js } from './data.js';
                 import json from './data.js' with { type: 'json' };
                 import text from './data.js' with { type: 'text' };
                 import bytes from './data.js' with { type: 'bytes' };
                 [js, json.n, text, bytes.length, bytes[0]].join('|')",
            ),
            ("data.js", "export var js = 'js';"),
        ]),
    );
    assert_eq!(result, Ok(string("js|1|TEXT|3|7")));
}

#[test]
fn re_exporting_a_text_resource_works_through_indirect_and_namespace_exports() {
    let result = run(
        &[
            (
                "main.js",
                "import { text, ns } from './re.js'; text + ':' + ns.default",
            ),
            (
                "re.js",
                "export { default as text } from './data' with { type: 'text' };
                 export * as ns from './data' with { type: 'text' };",
            ),
        ],
        &[("data", "abc")],
        &[],
    );
    assert_eq!(result, Ok(string("abc:abc")));
}

#[test]
fn a_bytes_import_is_a_uint8array_over_an_immutable_arraybuffer() {
    let result = run(
        &[(
            "main.js",
            "import value from './data.bin' with { type: 'bytes' };
             const out = [];
             out.push(value instanceof Uint8Array);
             out.push(value.buffer instanceof ArrayBuffer);
             out.push(value.length, value.buffer.byteLength);
             out.push(value.buffer.immutable);
             out.push(Array.from(value).join());
             for (const f of [() => value.buffer.resize(0), () => value.buffer.transfer()]) {
                 try { f(); out.push('no throw'); } catch (e) { out.push(e instanceof TypeError); }
             }
             out.join('|')",
        )],
        &[],
        &[("data.bin", &[0x89, 0x50, 0x4e, 0x00, 0xff])],
    );
    assert_eq!(
        result,
        Ok(string("true|true|5|5|true|137,80,78,0,255|true|true"))
    );
}

#[test]
fn an_empty_bytes_resource_is_an_empty_immutable_view() {
    let result = run(
        &[(
            "main.js",
            "import value from './empty.bin' with { type: 'bytes' };
             value.length + ':' + value.buffer.byteLength + ':' + value.buffer.immutable",
        )],
        &[],
        &[("empty.bin", &[])],
    );
    assert_eq!(result, Ok(string("0:0:true")));
}

#[test]
fn the_bytes_of_an_immutable_buffer_cannot_be_written_through_the_view() {
    let result = run(
        &[(
            "main.js",
            "import value from './data.bin' with { type: 'bytes' };
             let threw = false;
             try { value.fill(0); } catch (e) { threw = e instanceof TypeError; }
             threw + ':' + value[0]",
        )],
        &[],
        &[("data.bin", &[5, 6])],
    );
    assert_eq!(result, Ok(string("true:5")));
}

#[test]
fn every_importer_of_one_bytes_resource_sees_the_same_object() {
    let result = run(
        &[
            (
                "main.js",
                "import a from './data.bin' with { type: 'bytes' };
                 import { b } from './dep.js';
                 import * as ns from './data.bin' with { type: 'bytes' };
                 (a === b) + ':' + (a === ns.default)",
            ),
            (
                "dep.js",
                "import b from './data.bin' with { type: 'bytes' }; export { b };",
            ),
        ],
        &[],
        &[("data.bin", &[1, 2, 3])],
    );
    assert_eq!(result, Ok(string("true:true")));
}

#[test]
fn text_and_bytes_of_one_resource_are_distinct_records() {
    let result = run(
        &[(
            "main.js",
            "import text from './data' with { type: 'text' };
             import bytes from './data' with { type: 'bytes' };
             typeof text + ':' + (bytes instanceof Uint8Array)",
        )],
        &[("data", "hi")],
        &[("data", &[104, 105])],
    );
    assert_eq!(result, Ok(string("string:true")));
}

#[test]
fn a_bytes_import_survives_a_collection_before_every_allocation() {
    // A one-object nursery collects at nearly every allocation, so the
    // freshly built buffer and view must be rooted until they are linked.
    let config = VmConfig {
        heap: HeapConfig {
            nursery_capacity: 1,
            ..HeapConfig::default()
        },
        ..VmConfig::default()
    };
    let mut vm = vm_with(config, &[("t.txt", "text")], &[("data.bin", &[1, 2, 3, 4])]);
    let result = vm.execute_module_graph(
        "t/main.js",
        &build(&[(
            "main.js",
            "import bytes from './data.bin' with { type: 'bytes' };
             import text from './t.txt' with { type: 'text' };
             [1, 2, 3].map(n => ({ n })).length;
             bytes.buffer.immutable + ':' + bytes.join() + ':' + text",
        )]),
    );
    assert_eq!(result, Ok(string("true:1,2,3,4:text")));
}

/// Runs `script` as the classic-script entry `t/main.js` and returns what it
/// reported through `$DONE`.
fn run_script(
    text: &[(&str, &str)],
    bytes: &[(&str, &[u8])],
    script: &str,
) -> Option<Result<(), String>> {
    let mut vm = vm_with(VmConfig::default(), text, bytes);
    vm.install_test262_done().unwrap();
    vm.set_module_loader_context("t/main.js", HashMap::new());
    vm.execute_script(&compile(&parse(script).unwrap()).unwrap())
        .unwrap();
    vm.run_promise_jobs().unwrap();
    vm.take_test262_done()
        .map(|done| done.map_err(|error| format!("{error:?}")))
}

#[test]
fn dynamic_import_honours_type_text_and_type_bytes() {
    let script = "
        Promise.all([
            import('./data.txt', { with: { type: 'text' } }),
            import('./data.bin', { with: { type: 'bytes' } }),
            import('./data.txt', { with: { type: 'text' } }),
        ]).then(([t, b, again]) => {
            var ok = typeof t.default === 'string' && t.default === 'hello'
                && b.default instanceof Uint8Array && b.default.length === 2
                && b.default.buffer.immutable === true
                && t === again;
            if (ok) $DONE(); else $DONE(new Error('unexpected ' + typeof t.default));
        }, e => $DONE(e));";
    let done = run_script(&[("data.txt", "hello")], &[("data.bin", &[9, 8])], script);
    assert_eq!(done, Some(Ok(())));
}

#[test]
fn dynamic_import_of_a_missing_text_resource_rejects() {
    let script = "
        import('./missing', { with: { type: 'text' } })
            .then(() => $DONE(new Error('resolved')), () => $DONE());";
    assert_eq!(run_script(&[], &[], script), Some(Ok(())));
}

#[test]
fn an_unknown_type_attribute_value_still_loads_a_source_text_module() {
    // Only json/text/bytes select a synthetic module; other values are
    // accepted and ignored exactly like every other attribute.
    let result = run(
        &[
            (
                "main.js",
                "import { x } from './dep.js' with { type: 'javascript' }; x",
            ),
            ("dep.js", "export var x = 4;"),
        ],
        &[],
        &[],
    );
    assert_eq!(result, Ok(Value::Number(4.0)));
}
