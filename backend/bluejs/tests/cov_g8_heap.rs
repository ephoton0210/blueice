// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Object storage seen from outside the heap: the public `Heap` operations on
//! objects it no longer holds, and the JavaScript operations that reach its
//! exotic objects (module namespaces, arguments objects, typed arrays).

mod cov_g8_common;

use blueice_bluejs::{
    compile, compile_module, parse, parse_module, Heap, HeapConfig, HeapError, JsSymbol,
    PropertyDescriptor, Value, Vm,
};
use cov_g8_common::heap_sweep;
use std::collections::HashMap;

/// A heap and an id it handed out and has since reclaimed.
fn heap_with_stale_id() -> (Heap, blueice_bluejs::ObjectId) {
    let mut heap = Heap::new(HeapConfig::default()).unwrap();
    let id = heap.alloc_object(None).unwrap();
    heap.collect_major();
    assert!(!heap.contains(id));
    (heap, id)
}

#[test]
fn the_public_operations_reject_a_reclaimed_object() {
    let (mut heap, gone) = heap_with_stale_id();
    let invalid = Some(HeapError::InvalidObject(gone));
    assert_eq!(heap.get_own_property_descriptor(gone, "k").err(), invalid);
    assert_eq!(
        heap.define_own_property(
            gone,
            "k",
            PropertyDescriptor::data(Value::Null, true, true, true)
        )
        .err(),
        invalid
    );
    assert_eq!(heap.get_own(gone, "k").err(), invalid);
    assert_eq!(heap.get(gone, "k").err(), invalid);
    assert_eq!(heap.set(gone, "k", Value::Null).err(), invalid);
    assert_eq!(heap.delete(gone, "k").err(), invalid);
    assert_eq!(heap.own_keys(gone).err(), invalid);
    assert_eq!(heap.enumerable_own_keys(gone).err(), invalid);
    assert_eq!(heap.is_array(gone).err(), invalid);
    assert_eq!(heap.root(gone).err(), invalid);
}

#[test]
fn a_value_the_heap_no_longer_holds_cannot_be_defined() {
    let (mut heap, gone) = heap_with_stale_id();
    let object = heap.alloc_object(None).unwrap();
    assert_eq!(
        heap.define_own_property(
            object,
            "k",
            PropertyDescriptor::data(Value::Object(gone), true, true, true)
        ),
        Err(HeapError::InvalidObject(gone))
    );
}

#[test]
fn a_length_set_on_an_ordinary_object_is_an_ordinary_data_property() {
    let mut heap = Heap::new(HeapConfig::default()).unwrap();
    let object = heap.alloc_object(None).unwrap();
    heap.set(object, "length", Value::Number(3.0)).unwrap();
    let descriptor = heap
        .get_own_property_descriptor(object, "length")
        .unwrap()
        .unwrap();
    assert_eq!(descriptor.value, Some(Value::Number(3.0)));
    assert_eq!(
        (
            descriptor.writable,
            descriptor.enumerable,
            descriptor.configurable
        ),
        (Some(true), Some(true), Some(true))
    );
    let symbol = JsSymbol::well_known("toStringTag");
    assert_eq!(heap.get_own(object, symbol).unwrap(), None);
}

fn run_module(source: &str) -> String {
    let mut modules = HashMap::new();
    modules.insert(
        "t/main.js".to_string(),
        compile_module(&parse_module(source).unwrap()).unwrap(),
    );
    match Vm::default().execute_module_graph("t/main.js", &modules) {
        Ok(Value::String(text)) => text.to_utf8().unwrap(),
        other => format!("unexpected {other:?}"),
    }
}

/// Every operation on the namespace of the module itself, run before the
/// module has initialized its `x`.
#[test]
fn a_namespace_reports_an_uninitialized_export_to_every_operation() {
    let log = run_module(
        "import * as ns from './main.js';
         const log = [];
         function t(f) { try { f(); log.push('ok'); } catch (e) { log.push(e.constructor.name); } }
         t(() => ns.x);
         t(() => Reflect.get(ns, 'x'));
         t(() => Object.getOwnPropertyDescriptor(ns, 'x'));
         t(() => Object.getOwnPropertyDescriptors(ns));
         t(() => Object.defineProperty(ns, 'x', { value: 1 }));
         t(() => 'x' in ns);
         t(() => Object.prototype.hasOwnProperty.call(ns, 'x'));
         t(() => Object.entries(ns));
         t(() => Object.assign({}, ns));
         t(() => JSON.stringify(ns));
         t(() => Reflect.set(ns, 'x', 1));
         t(() => Reflect.deleteProperty(ns, 'x'));
         t(() => Object.defineProperty(ns, 'zzz', { value: 1 }));
         t(() => Reflect.deleteProperty(ns, 'zzz'));
         t(() => Reflect.ownKeys(ns));
         export let x = 1;
         log.join();",
    );
    assert_eq!(
        log,
        "ReferenceError,ReferenceError,ReferenceError,ReferenceError,ReferenceError,ok,\
         ReferenceError,ReferenceError,ReferenceError,ReferenceError,ok,ok,TypeError,ok,ok"
    );
}

fn observe(source: &str) -> String {
    let source = format!(
        "(function () {{ try {{ return String({source}); }} catch (e) {{ return 'throws ' + e.constructor.name + ': ' + e.message; }} }})()"
    );
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    match vm.execute(&compile(&parse(&source).unwrap()).unwrap()) {
        Ok(Value::String(text)) => text.to_utf8().unwrap(),
        other => format!("unexpected {other:?}"),
    }
}

const CASES: &[(&str, &str)] = &[
    // A typed array has no own property for a non-canonical numeric key.
    ("Object.getOwnPropertyDescriptor(new Uint8Array(2), '-0')", "undefined"),
    ("Object.getOwnPropertyDescriptor(new Uint8Array(2), '5')", "undefined"),
    ("Object.getOwnPropertyDescriptor(new Uint8Array(2), '1').value", "0"),
    ("Reflect.ownKeys((function () { var t = new Uint8Array(2); $262.detachArrayBuffer(t.buffer); return t; })()).length", "0"),
    ("Reflect.ownKeys(new Uint8Array(2)).join()", "0,1"),
    // Mapped arguments: a descriptor without a value, an accessor, and a value.
    ("(function (a) { Object.defineProperty(arguments, '0', { enumerable: false }); a = 5; return arguments[0] + ',' + Object.getOwnPropertyDescriptor(arguments, '0').enumerable; })(1)", "5,false"),
    ("(function (a) { Object.defineProperty(arguments, '0', { get() { return 9; } }); a = 5; return arguments[0]; })(1)", "9"),
    ("(function (a) { Object.defineProperty(arguments, '0', { value: 7 }); return a; })(1)", "7"),
    ("(function (a) { Object.defineProperty(arguments, '0', { writable: false }); a = 5; return arguments[0]; })(1)", "1"),
    ("(function (a) { delete arguments[0]; a = 5; return arguments[0]; })(1)", "undefined"),
];

#[test]
fn exotic_objects_report_the_reference_results() {
    let mismatches: Vec<_> = CASES
        .iter()
        .map(|(expr, expected)| (*expr, *expected, observe(expr)))
        .filter(|(_, expected, actual)| actual != expected)
        .collect();
    assert_eq!(mismatches, Vec::<(&str, &str, String)>::new());
}

/// Mapped arguments written under a heap that has room for one copy of a long
/// string but not two.
#[test]
fn a_mapped_arguments_object_can_run_out_of_heap_while_writing_its_cell() {
    let long = "'x'.repeat(300)";
    let stopped = heap_sweep(
        &format!("(function (a) {{ Object.defineProperty(arguments, '0', {{ value: {long} }}); return a.length; }})(1)"),
        3000,
        8,
    ) + heap_sweep(
        &format!("(function (a) {{ arguments[0] = {long}; return a.length; }})(1)"),
        3000,
        8,
    );
    assert!(stopped > 0, "{stopped}");
}
