// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Heap operations on the hottest paths (iterator results, `Array.prototype.push`,
//! array construction, typed-array fills, loop-scope cells, global stores) run without
//! the VM first gathering its roots, and retry with them when the heap has to collect.
//! With a nursery of a single object that retry happens on nearly every allocation, so
//! these scripts exercise the fallback constantly: every object that is live in the
//! script must survive it, and the results must be exactly what they are without any
//! pressure.

use blueice_bluejs::{compile, parse, HeapConfig, Value, Vm, VmConfig};

fn run(config: VmConfig, source: &str) -> Value {
    let mut vm = Vm::new(config).unwrap();
    vm.execute(&compile(&parse(source).expect(source)).expect(source))
        .unwrap_or_else(|e| panic!("{source}: {e:?}"))
}

fn tiny() -> VmConfig {
    VmConfig {
        heap: HeapConfig {
            nursery_capacity: 1,
            major_threshold_bytes: 4096,
            max_heap_bytes: 64 * 1024 * 1024,
        },
        ..VmConfig::default()
    }
}

/// Each script ends in a number that does not depend on when collections happen.
const SCRIPTS: &[(&str, f64)] = &[
    // Iterator results: spread and for-of allocate a result object per element.
    ("let s = 0; for (const x of [1,2,3,4,5,6,7,8,9,10]) s += x; s", 55.0),
    ("let a = [...'abcdefgh']; a.length", 8.0),
    (
        "let m = new Map(); for (let i = 0; i < 50; i++) m.set('k' + i, {v: i});\
         let s = 0; for (const [k, o] of m) s += o.v; s",
        1225.0,
    ),
    // push builds an array while each element is a fresh object.
    (
        "let a = []; for (let i = 0; i < 200; i++) a.push({i, tag: 'x' + i});\
         let s = 0; for (const o of a) s += o.i; s + a.length",
        19900.0 + 200.0,
    ),
    // Array construction from a list of fresh values.
    (
        "const f = (...xs) => xs; let t = 0;\
         for (let i = 0; i < 100; i++) { const a = f({n: i}, {n: i + 1}, [i]); t += a[0].n + a[1].n + a[2][0]; } t",
        // (i) + (i + 1) + (i) summed over 0..99
        3.0 * 4950.0 + 100.0,
    ),
    // Typed arrays built from arrays and from iterables.
    (
        "const t = new Float64Array([1,2,3,4,5,6,7,8]); let s = 0; for (const v of t) s += v; s",
        36.0,
    ),
    (
        "const t = Uint8Array.from({length: 40}, (_, i) => i); let s = 0; for (let i = 0; i < t.length; i++) s += t[i]; s",
        780.0,
    ),
    // Closures captured per loop iteration (the loop scope is cloned each time).
    (
        "const fs = []; for (let i = 0; i < 60; i++) fs.push(() => i); let s = 0; for (const f of fs) s += f(); s",
        1770.0,
    ),
    // Stores to global bindings while allocating.
    (
        "var g = {n: 0}; for (var i = 0; i < 100; i++) { g = {n: g.n + i, prev: g}; } g.n",
        4950.0,
    ),
    // Generators and iterator helpers allocate results across suspension points.
    (
        "function* gen() { for (let i = 0; i < 30; i++) yield {i}; }\
         let s = 0; for (const o of gen()) s += o.i; s",
        435.0,
    ),
    (
        "let o = {}; for (let i = 0; i < 80; i++) o['p' + i] = [i, {i}];\
         let s = 0; for (const k of Object.keys(o)) s += o[k][1].i; s",
        3160.0,
    ),
];

#[test]
fn every_hot_path_survives_a_collection_before_nearly_every_allocation() {
    for (source, expected) in SCRIPTS {
        assert_eq!(run(tiny(), source), Value::Number(*expected), "{source}");
    }
}

#[test]
fn the_results_do_not_depend_on_collection_pressure() {
    let relaxed = VmConfig {
        heap: HeapConfig {
            nursery_capacity: 1_000_000,
            major_threshold_bytes: 128 * 1024 * 1024,
            max_heap_bytes: 256 * 1024 * 1024,
        },
        ..VmConfig::default()
    };
    for (source, expected) in SCRIPTS {
        assert_eq!(run(relaxed, source), Value::Number(*expected), "{source}");
    }
}

#[test]
fn a_heap_that_runs_out_still_reports_it_rather_than_corrupting() {
    let mut vm = Vm::new(VmConfig {
        heap: HeapConfig {
            nursery_capacity: 1,
            major_threshold_bytes: 1024,
            max_heap_bytes: 8192,
        },
        ..VmConfig::default()
    })
    .unwrap();
    let source = "let a = []; while (true) a.push({x: [1, 2, 3]});";
    let error = vm
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_err();
    assert!(
        matches!(
            error,
            blueice_bluejs::RuntimeError::Heap(blueice_bluejs::HeapError::HeapLimitExceeded { .. })
        ),
        "{error:?}"
    );
}
