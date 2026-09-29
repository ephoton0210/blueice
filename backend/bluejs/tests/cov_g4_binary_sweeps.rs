// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Resource exhaustion through the binary-data built-ins: every heap
//! allocation and instruction step of each operation fails in turn and must
//! report the matching limit error.

mod cov_g4_common;
use cov_g4_common::{
    sweep_fuel_each, sweep_heap_bare, sweep_heap_each, sweep_heap_filled, with_setup,
};

/// Each of these is the first thing a bare VM builds, so the intrinsics they
/// create lazily are swept too.
const FIRST_USES: &[&str] = &[
    "new ArrayBuffer(4)",
    "new SharedArrayBuffer(4)",
    "new DataView(new ArrayBuffer(8))",
    "new Uint8Array(3)",
    "new Float64Array([1, 2])",
    "Atomics.add(new Int32Array(1), 0, 1)",
    "Atomics.waitAsync(new Int32Array(new SharedArrayBuffer(8)), 0, 0, 5)",
];

const SETUP: &str = "var ab = new ArrayBuffer(64, { maxByteLength: 128 }); var sab = new SharedArrayBuffer(64, { maxByteLength: 128 }); var i32 = new Int32Array(sab); var u8 = new Uint8Array(ab); var realm = $262.createRealm(); var other = realm.global; var foreign = new other.ArrayBuffer(64); var foreignShared = new other.SharedArrayBuffer(64);";

const BODIES: &[&str] = &[
    "ab.slice(8, 40); sab.slice(8, 40);",
    "ab.transfer(32); ab = new ArrayBuffer(64, { maxByteLength: 128 }); ab.transferToFixedLength();",
    "new DataView(ab, 4, 8); new DataView(foreign);",
    "new Uint8Array(ab, 4, 8); new Int16Array(foreign); new Uint8Array(u8); new Uint8Array([1, 2, 3]); new Uint8Array(new Set([1, 2])); new Uint8Array({ length: 2, 0: 1 });",
    "ab.constructor = { [Symbol.species]: other.ArrayBuffer }; ab.slice(4, 20);",
    "sab.constructor = { [Symbol.species]: other.SharedArrayBuffer }; sab.slice(4, 20);",
    "Atomics.waitAsync(i32, 0, 0, 5); Atomics.waitAsync(i32, 0, 1, 5); Atomics.waitAsync(i32, 0, 0, 0);",
    "Atomics.add(i32, 0, 1); Atomics.load(i32, 0); Atomics.compareExchange(i32, 0, 1, 2); Atomics.notify(i32, 0, 1);",
    "Atomics.wait(i32, 0, 1, 0); Atomics.wait(i32, 0, 0, 0);",
];

#[test]
fn every_heap_allocation_failure_reports_the_heap_limit() {
    for script in FIRST_USES {
        sweep_heap_bare(script, 4000);
    }
    sweep_heap_each(&with_setup(SETUP, BODIES));
    // Only the bodies whose early allocations sit below the setup's own
    // peak need the filler.
    for body in &BODIES[..5] {
        sweep_heap_filled(SETUP, body);
    }
}

#[test]
fn every_instruction_budget_exhaustion_reports_the_instruction_limit() {
    sweep_fuel_each(&with_setup(SETUP, BODIES));
}
