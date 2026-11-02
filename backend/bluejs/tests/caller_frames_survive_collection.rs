// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A call parks its caller's bindings and cells where the collector reads them. Objects the
//! caller holds only in a local, and only through a closure cell, must survive every
//! collection the callee (and callees of callees) force, and the caller must read them back.

use blueice_bluejs::{compile, parse, HeapConfig, Value, Vm, VmConfig};

fn number(source: &str) -> f64 {
    let mut vm = Vm::new(VmConfig {
        heap: HeapConfig {
            nursery_capacity: 4,
            major_threshold_bytes: 4 * 1024,
            ..HeapConfig::default()
        },
        instruction_budget: 50_000_000,
        ..VmConfig::default()
    })
    .unwrap();
    match vm
        .execute(&compile(&parse(source).expect(source)).expect(source))
        .unwrap_or_else(|e| panic!("{source}: {e:?}"))
    {
        Value::Number(n) => n,
        other => panic!("{source}: {other:?}"),
    }
}

#[test]
fn objects_held_only_by_a_displaced_caller_survive_the_callees_collections() {
    // `held` lives in a local; `shared` lives in a cell (captured by `bump`).
    let source = "(function () {
        let held = { a: 1, b: [1, 2, 3] };
        let shared = { n: 10 };
        const bump = () => { shared.n += 1; };
        function churn(depth) {
            for (let i = 0; i < 200; i++) { ({ x: i, y: [i] }); }
            return depth > 0 ? churn(depth - 1) : 0;
        }
        for (let round = 0; round < 20; round++) { churn(5); bump(); }
        return held.a + held.b[2] + shared.n;
    })()";
    assert_eq!(number(source), 1.0 + 3.0 + 30.0);
}

#[test]
fn every_displaced_frame_in_a_nested_chain_is_still_read_after_collection() {
    let source = "(function outer() {
        let a = { v: 1 };
        function mid() {
            let b = { v: 2 };
            function inner() {
                let c = { v: 4 };
                for (let i = 0; i < 400; i++) { ({ junk: i }); }
                return c.v;
            }
            const r = inner();
            return r + b.v;
        }
        return mid() + a.v;
    })()";
    assert_eq!(number(source), 7.0);
}
