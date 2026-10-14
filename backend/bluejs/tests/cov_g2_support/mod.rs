// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Shared helpers for the `cov_g2_*` test targets: running scripts and
//! sweeping the managed-heap ceiling so that every allocation a script makes
//! is, in turn, the one that fails.
#![allow(dead_code)]

use blueice_bluejs::{compile, parse, HeapConfig, HeapError, RuntimeError, Value, Vm, VmConfig};

pub fn run(vm: &mut Vm, source: &str) -> Result<Value, RuntimeError> {
    vm.execute(&compile(&parse(source).unwrap()).unwrap())
}

/// Evaluates `source` in a fresh VM and expects `true`.
pub fn expect_true(source: &str) {
    let mut vm = Vm::default();
    assert_eq!(run(&mut vm, source), Ok(Value::Bool(true)), "{source}");
}

/// Evaluates `source` as a classic script (top-level `var`s become properties
/// of the global object) and expects `true`.
pub fn expect_script_true(source: &str) {
    let mut vm = Vm::default();
    let result = vm.execute_script(&compile(&parse(source).unwrap()).unwrap());
    assert_eq!(result, Ok(Value::Bool(true)), "{source}");
}

/// Evaluates `source` (a script that sets `globalThis.result` once its
/// promise chain has finished), drains the promise jobs and expects the result
/// to be `true`.
pub fn expect_async_true(source: &str) {
    let mut vm = Vm::default();
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap();
    vm.run_promise_jobs().unwrap();
    assert_eq!(
        run(&mut vm, "globalThis.result"),
        Ok(Value::Bool(true)),
        "{source}"
    );
}

/// Evaluates `source` in a fresh VM and returns the rendered error.
pub fn error_of(source: &str) -> String {
    let mut vm = Vm::default();
    format!("{:?}", run(&mut vm, source).expect_err(source))
}

fn vm_with_ceiling(ceiling: usize, nursery: usize) -> Option<Vm> {
    Vm::new(VmConfig {
        heap: HeapConfig {
            nursery_capacity: nursery,
            max_heap_bytes: ceiling,
            ..HeapConfig::default()
        },
        ..VmConfig::default()
    })
    .ok()
}

/// The smallest heap ceiling under which `attempt` succeeds (found by
/// doubling and then bisecting; success is assumed monotone in the ceiling).
fn smallest_ceiling(attempt: impl Fn(usize) -> bool) -> usize {
    let mut high = 4096;
    while !attempt(high) {
        high *= 2;
    }
    let mut low = high / 2;
    while low + 1 < high {
        let middle = low + (high - low) / 2;
        if attempt(middle) {
            high = middle;
        } else {
            low = middle;
        }
    }
    high
}

/// Runs `script` (after `warmup`, which materializes the lazily built
/// intrinsics the script relies on) under every heap ceiling between what the
/// warm-up needs and what the script needs, `step` bytes apart. The ceiling
/// that lets everything succeed must produce `expected`; every smaller one
/// must produce either `expected` or a heap-limit error, never a panic or any
/// other failure. Each ceiling makes a different allocation the failing one,
/// which is how the error path of each allocating call is reached.
pub fn sweep_heap_ceiling(warmup: &str, script: &str, expected: Value, step: usize) {
    let succeeds = |limit: usize, with_script: bool| {
        vm_with_ceiling(limit, 1).is_some_and(|mut vm| {
            run(&mut vm, warmup).is_ok()
                && (!with_script || run(&mut vm, script) == Ok(expected.clone()))
        })
    };
    let floor = smallest_ceiling(|limit| succeeds(limit, false));
    let ceiling = smallest_ceiling(|limit| succeeds(limit, true));
    for limit in (floor..=ceiling).step_by(step) {
        let Some(mut vm) = vm_with_ceiling(limit, 1) else {
            continue;
        };
        if run(&mut vm, warmup).is_err() {
            continue;
        }
        let result = run(&mut vm, script);
        assert!(
            result == Ok(expected.clone())
                || matches!(
                    result,
                    Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { .. }))
                ),
            "ceiling {limit}: {script}: {result:?}"
        );
    }
}

/// [`sweep_heap_ceiling`] over `body`, a script fragment that must evaluate to
/// `true`. The heap limit only fails the first allocation that pushes the live
/// bytes past it, so an operation can only be made to fail if it is a new
/// high-water mark of the run. The fragment therefore calls `b()` (which adds
/// a few kilobytes of live ballast) before each operation it wants to reach:
/// the ballast outgrows whatever transient state the previous operation left
/// behind, making every allocation of the next one a new high-water mark.
pub fn sweep_ops(warmup: &str, body: &str, step: usize) {
    let warmup = format!(
        "{warmup}; globalThis.keep = [];
         globalThis.b = function () {{ keep.push('x'.repeat(800) + keep.length); }};
         keep.push('y'.repeat(20000)); 0"
    );
    let script = format!("(function () {{ {body} }})()");
    sweep_heap_ceiling(&warmup, &script, Value::Bool(true), step);
}

/// Runs `script` under every instruction budget from 1 upward until it
/// completes with `expected`. Each budget makes a different fuel charge the
/// one that fails, which is how the error path of each charging site is
/// reached; every smaller budget must fail with the instruction limit.
pub fn sweep_instruction_budget(script: &str, expected: Value) {
    let mut budget = 1;
    loop {
        let mut vm = Vm::new(VmConfig {
            instruction_budget: budget,
            ..VmConfig::default()
        })
        .unwrap();
        match run(&mut vm, script) {
            Err(RuntimeError::InstructionLimit) => budget += 1,
            other => {
                assert_eq!(other, Ok(expected), "budget {budget}: {script}");
                return;
            }
        }
    }
}
