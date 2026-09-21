// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression tests found by sweeping the Test262 inventory under GC stress
//! (`BLUEJS_TEST262_NURSERY_CAPACITY=1`, optionally with
//! `BLUEJS_TEST262_MAJOR_THRESHOLD`). Each script must produce the same result
//! under the ordinary collector schedule and under every stress schedule: a
//! difference means a native function holds a heap object in a Rust local
//! across an allocation without rooting it.
use blueice_bluejs::{compile, parse, Value, Vm, VmConfig};

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
