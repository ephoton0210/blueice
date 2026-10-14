// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression tests found by sweeping the Test262 inventory under GC stress
//! (`BLUEJS_TEST262_NURSERY_CAPACITY=1`, optionally with
//! `BLUEJS_TEST262_MAJOR_THRESHOLD`). Each script must produce the same result
//! under the ordinary collector schedule and under every stress schedule: a
//! difference means a native function holds a heap object in a Rust local
//! across an allocation without rooting it.
use blueice_bluejs::{compile, compile_module, parse, parse_module, Value, Vm, VmConfig};
use std::collections::HashMap;

fn evaluate(
    source: &str,
    nursery_capacity: Option<usize>,
    major_threshold_bytes: Option<usize>,
) -> Result<Value, String> {
    let mut config = VmConfig::default();
    if let Some(capacity) = nursery_capacity {
        config.heap.nursery_capacity = capacity;
    }
    if let Some(bytes) = major_threshold_bytes {
        config.heap.major_threshold_bytes = bytes;
    }
    Vm::new(config)
        .unwrap()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .map_err(|error| format!("{error:?}"))
}

/// Runs `source` ordinarily and under a one-object nursery with a sweep of
/// major-collection thresholds; every run must produce `true`.
fn gc_stress_matches_ordinary(source: &str) {
    assert_eq!(
        evaluate(source, None, None),
        Ok(Value::Bool(true)),
        "ordinary mode: {source}"
    );
    for threshold in [
        None,
        Some(20_000),
        Some(60_000),
        Some(120_000),
        Some(300_000),
    ] {
        assert_eq!(
            evaluate(source, Some(1), threshold),
            Ok(Value::Bool(true)),
            "nursery 1, major threshold {threshold:?}: {source}"
        );
    }
}

/// `Array.from` reads the `@@iterator` method before it constructs the result
/// array. A getter that returns a fresh function leaves that method reachable
/// only from a Rust local while the result array is allocated.
#[test]
fn array_from_keeps_a_freshly_read_iterator_method_alive_while_the_result_is_allocated() {
    gc_stress_matches_ordinary(
        "var ok = true;\
         for (var primitive of [true, 3.14, 'hello', Symbol()]) {\
           var prototype = Object.getPrototypeOf(primitive);\
           Object.defineProperty(prototype, Symbol.iterator, {\
             configurable: true,\
             get() { 'use strict'; return () => [this][Symbol.iterator](); }\
           });\
           ok = ok && Array.from(primitive)[0] === primitive;\
           delete prototype[Symbol.iterator];\
         }\
         ok",
    );
}

/// `Iterator.from` reads `next` from the iterator, then asks for the
/// iterator's prototype chain (observable through a Proxy) and allocates the
/// wrapper prototype. The `next` read through a Proxy handler whose `get`
/// returns a fresh bound function is reachable only from a Rust local across
/// those steps.
#[test]
fn iterator_from_keeps_a_freshly_read_next_method_alive_until_the_wrapper_exists() {
    gc_stress_matches_ordinary(
        "var handlerProxy = new Proxy({}, {\
           get: (target, key, receiver) => (...args) => {\
             var item = Reflect[key](...args);\
             return typeof item === 'function' ? item.bind(receiver) : item;\
           }\
         });\
         var iter = new Proxy({ next: () => ({ done: false, value: 7 }) }, handlerProxy);\
         var wrap = Iterator.from(iter);\
         var first = wrap.next(), second = wrap.next();\
         first.value === 7 && second.done === false",
    );
}

/// A Proxy whose target is itself a Proxy: validating a trap's result against
/// the target's invariants runs the inner Proxy's traps, which are user code
/// that allocates. The outer trap's fresh result object must survive that.
const NESTED_PROXY_TARGET: &str = "\
    function inner(handler) { return new Proxy({}, handler); }\
    function churn() { (() => 1)(); return [{}, []]; }\
    var allocating = {\
      getOwnPropertyDescriptor(t, k) { churn(); return undefined; },\
      isExtensible(t) { churn(); return Reflect.isExtensible(t); },\
      getPrototypeOf(t) { churn(); return Reflect.getPrototypeOf(t); }\
    };";

#[test]
fn proxy_get_keeps_the_trap_result_alive_while_the_target_is_checked() {
    gc_stress_matches_ordinary(&format!(
        "{NESTED_PROXY_TARGET}\
         var outer = new Proxy(inner(allocating), {{ get(t, k) {{ return {{tag: k}}; }} }});\
         outer.x.tag === 'x'"
    ));
}

#[test]
fn proxy_get_own_property_keeps_the_trap_result_alive_while_the_target_is_checked() {
    gc_stress_matches_ordinary(&format!(
        "{NESTED_PROXY_TARGET}\
         var outer = new Proxy(inner(allocating), {{\
           getOwnPropertyDescriptor(t, k) {{\
             return {{value: 1, writable: true, enumerable: true, configurable: true}};\
           }}\
         }});\
         var descriptor = Object.getOwnPropertyDescriptor(outer, 'x');\
         descriptor.value === 1 && descriptor.configurable === true"
    ));
}

#[test]
fn proxy_get_prototype_keeps_the_trap_result_alive_while_the_target_is_checked() {
    gc_stress_matches_ordinary(&format!(
        "{NESTED_PROXY_TARGET}\
         var outer = new Proxy(inner(allocating), {{ getPrototypeOf(t) {{ return {{tag: 'proto'}}; }} }});\
         Object.getPrototypeOf(outer).tag === 'proto'"
    ));
}

/// `Atomics.waitAsync` allocates its `{async, value}` result record first and
/// then allocates the Promise (and defines properties, which may collect)
/// before returning it: the record must stay rooted throughout.
#[test]
fn atomics_wait_async_keeps_its_result_record_alive_while_the_promise_is_made() {
    gc_stress_matches_ordinary(
        "var view = new Int32Array(new SharedArrayBuffer(4));\
         var pending = Atomics.waitAsync(view, 0, 0, 20);\
         var mismatch = Atomics.waitAsync(view, 0, 1);\
         var immediate = Atomics.waitAsync(view, 0, 0, 0);\
         pending.async === true && pending.value instanceof Promise\
           && mismatch.async === false && mismatch.value === 'not-equal'\
           && immediate.async === false && immediate.value === 'timed-out'",
    );
}

/// `ShadowRealm.prototype.importValue` creates its Promise capability, then
/// runs the whole import in the child realm and allocates wrappers in the
/// caller's heap before it finally calls the capability's `resolve`/`reject`
/// functions, which nothing else refers to.
#[test]
fn shadow_realm_import_value_keeps_its_promise_capability_alive_across_the_import() {
    let source = "var outcome = 'pending';\
        var realm = new ShadowRealm();\
        realm.importValue('./mod.js', 'missing').then(\
          () => { outcome = 'resolved'; },\
          error => { outcome = Object.getPrototypeOf(error) === TypeError.prototype ? 'rejected' : 'wrong'; });\
        realm.importValue('./mod.js', 'x').then(value => { outcome += ':' + value; });";
    let run = |nursery: Option<usize>, threshold: Option<usize>| {
        let mut config = VmConfig::default();
        if let Some(capacity) = nursery {
            config.heap.nursery_capacity = capacity;
        }
        if let Some(bytes) = threshold {
            config.heap.major_threshold_bytes = bytes;
        }
        let modules = HashMap::from([(
            "shadow/mod.js".to_string(),
            compile_module(&parse_module("export var x = 42;").unwrap()).unwrap(),
        )]);
        let mut vm = Vm::new(config).unwrap();
        vm.set_module_loader_context("shadow/main.js", modules);
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap_or_else(|error| panic!("nursery {nursery:?}: {error:?}"));
        vm.run_promise_jobs().unwrap();
        vm.execute_script(&compile(&parse("outcome").unwrap()).unwrap())
            .unwrap()
    };
    let ordinary = run(None, None);
    assert_eq!(ordinary, Value::String("rejected:42".into()));
    for threshold in [
        None,
        Some(20_000),
        Some(60_000),
        Some(120_000),
        Some(300_000),
    ] {
        assert_eq!(run(Some(1), threshold), ordinary, "threshold {threshold:?}");
    }
}
