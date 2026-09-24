// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fault-injection helpers shared by the `cov_g8_*` tests.
//!
//! An injected fault is a script-visible exception thrown from the `n`-th
//! observable step of an operation (a Proxy trap, a `valueOf`/`toString` hook
//! or a callback), for every `n` in turn until the operation finishes without
//! reaching step `n`. That drives every error-propagation path of a built-in
//! that reads or writes through user-visible hooks.

#![allow(dead_code)]

use blueice_bluejs::{compile, parse, HeapConfig, RuntimeError, Value, Vm, VmConfig};

/// The script-side half of the injection: `fault()` counts steps and throws at
/// step `at`; `P` wraps a value in a Proxy whose every trap is a step; `V(x)`
/// is an object converting to `x` through a step; `F(f)` wraps a callback;
/// `A` is `Array.prototype`.
pub const FAULT_SETUP: &str = "
globalThis.count = 0;
globalThis.at = 0;
globalThis.fault = function () { if (++count === at) throw new EvalError('fault'); };
globalThis.handler = {
  get(t, k, r) { fault(); return Reflect.get(t, k, r); },
  set(t, k, v, r) { fault(); return Reflect.set(t, k, v, r); },
  has(t, k) { fault(); return Reflect.has(t, k); },
  deleteProperty(t, k) { fault(); return Reflect.deleteProperty(t, k); },
  defineProperty(t, k, d) { fault(); return Reflect.defineProperty(t, k, d); },
  getOwnPropertyDescriptor(t, k) { fault(); return Reflect.getOwnPropertyDescriptor(t, k); },
  ownKeys(t) { fault(); return Reflect.ownKeys(t); },
  getPrototypeOf(t) { fault(); return Reflect.getPrototypeOf(t); },
};
globalThis.P = function (target) { return new Proxy(target, handler); };
globalThis.V = function (x) { return { valueOf() { fault(); return x; }, toString() { fault(); return String(x); } }; };
globalThis.F = function (f) { return function () { fault(); return f.apply(this, arguments); }; };
globalThis.A = Array.prototype;
";

/// Outcome of one injected run: whether the operation got past the injection
/// point without reaching it, and how it ended.
fn injected_run(vm: &mut Vm, op: &str, at: u32) -> String {
    let source = format!(
        "count = 0; at = {at}; (function () {{ var r; try {{ {op}; r = 'ok' }} catch (e) {{ r = String(e && e.message) }} return (count < at ? 'done:' : 'more:') + r }})()"
    );
    match vm.execute(&compile(&parse(&source).unwrap()).unwrap()) {
        Ok(Value::String(text)) => text.to_utf8().unwrap(),
        other => format!("unexpected {other:?}"),
    }
}

/// Runs `op` once for every injection point in a VM prepared by `setup`
/// (after [`FAULT_SETUP`]), returning the number of points it has.
pub fn fault_sweep(setup: &str, op: &str) -> u32 {
    let mut vm = Vm::default();
    for script in [FAULT_SETUP, setup] {
        vm.execute(&compile(&parse(script).unwrap()).unwrap())
            .unwrap();
    }
    for at in 1..=2000 {
        let outcome = injected_run(&mut vm, op, at);
        assert!(
            !outcome.starts_with("unexpected"),
            "{op} at {at}: {outcome}"
        );
        if outcome.starts_with("done:") {
            return at - 1;
        }
    }
    panic!("{op}: still reaching injection points after 2000 steps");
}

/// Runs `source` under every instruction budget from one upward until it
/// completes; a shorter budget must stop with the instruction limit.
pub fn budget_sweep(source: &str) -> u64 {
    let program = compile(&parse(source).unwrap()).unwrap();
    let mut budget = 1;
    loop {
        let mut vm = Vm::new(VmConfig {
            instruction_budget: budget,
            ..VmConfig::default()
        })
        .unwrap();
        match vm.execute(&program) {
            Ok(_) => return budget,
            Err(error) => assert_eq!(error, RuntimeError::InstructionLimit, "{source}"),
        }
        budget += 1;
    }
}

fn heap_config(bytes: usize) -> VmConfig {
    VmConfig {
        heap: HeapConfig {
            nursery_capacity: 256,
            major_threshold_bytes: bytes,
            max_heap_bytes: bytes,
        },
        ..VmConfig::default()
    }
}

/// One attempt of `source` in a VM whose whole heap is `bytes`: `true` when it
/// completed, `false` when the heap limit stopped it (any other outcome fails).
fn heap_attempt(bytes: usize, source: &str, harness: bool) -> bool {
    let Ok(mut vm) = Vm::new(heap_config(bytes)) else {
        return false;
    };
    if harness {
        if let Err(error) = vm.install_test262_harness() {
            assert_eq!(
                format!("{error:?}"),
                format!("Heap(HeapLimitExceeded {{ limit: {bytes} }})"),
                "installing the Test262 host in {bytes} bytes"
            );
            return false;
        }
    }
    match vm.execute(&compile(&parse(source).unwrap()).unwrap()) {
        Ok(_) => true,
        Err(error) => {
            assert_eq!(
                format!("{error:?}"),
                format!("Heap(HeapLimitExceeded {{ limit: {bytes} }})"),
                "{source} in {bytes} bytes"
            );
            false
        }
    }
}

/// Runs `source` in VMs whose heaps grow from `back_from` bytes below the
/// smallest heap it completes in, by `step` bytes, up to that size: every
/// allocation the script makes near its end is in turn the one that no longer
/// fits. Returns how many attempts the heap limit stopped.
pub fn heap_sweep(source: &str, back_from: usize, step: usize) -> usize {
    heap_sweep_with(source, back_from, step, false)
}

/// As [`heap_sweep`], with the Test262 host installed first when `harness`.
pub fn heap_sweep_with(source: &str, back_from: usize, step: usize, harness: bool) -> usize {
    let (mut low, mut high) = (512, 64 * 1024 * 1024);
    while high - low > 8 {
        let middle = low + (high - low) / 2;
        if heap_attempt(middle, source, harness) {
            high = middle;
        } else {
            low = middle;
        }
    }
    let mut stopped = 0;
    let mut bytes = high.saturating_sub(back_from).max(512);
    while bytes <= high {
        stopped += usize::from(!heap_attempt(bytes, source, harness));
        bytes += step;
    }
    assert!(heap_attempt(high + 8, source, harness), "{source}");
    stopped
}

/// Evaluates `expr` in a fresh VM (with the Test262 host when `harness`) and
/// reports its `String` conversion, or `throws Name: message`.
pub fn observe(expr: &str, harness: bool) -> String {
    let source = format!(
        "(function(){{ try {{ return String({expr}) }} catch (e) {{ return 'throws ' + e.constructor.name + ': ' + e.message }} }})()"
    );
    let mut vm = Vm::default();
    if harness {
        vm.install_test262_harness().unwrap();
    }
    match vm.execute(&compile(&parse(&source).unwrap()).unwrap()) {
        Ok(Value::String(text)) => text.to_utf8().unwrap(),
        other => format!("unexpected {other:?}"),
    }
}

/// Checks each `(expression, expected)` pair, reporting every mismatch at once.
pub fn check_cases(cases: &[(&str, &str)], harness: bool) {
    let mismatches: Vec<_> = cases
        .iter()
        .map(|(expr, expected)| (*expr, *expected, observe(expr, harness)))
        .filter(|(_, expected, actual)| actual != expected)
        .collect();
    assert_eq!(mismatches, Vec::<(&str, &str, String)>::new());
}
