// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Heap-exhaustion sweeps over a whole run: fresh VMs whose entire heap budget
//! grows a little at a time, so each allocation made while installing the
//! Test262 host or loading synthetic modules (and the intrinsics they create
//! lazily) is, in turn, the one that no longer fits. Every failure on the way
//! must be reported as the heap limit rather than a panic or some other error,
//! and the run must succeed once the budget suffices. Single built-in calls are
//! swept more precisely by `vm::functions::allocation_exhaustion`.

use blueice_bluejs::{compile, parse, Bytecode, HeapConfig, RuntimeError, Value, Vm, VmConfig};
use std::collections::HashMap;

fn config(bytes: usize) -> VmConfig {
    VmConfig {
        heap: HeapConfig {
            nursery_capacity: 256,
            major_threshold_bytes: bytes,
            max_heap_bytes: bytes,
        },
        ..VmConfig::default()
    }
}

fn is_heap_limit(error: &RuntimeError, budget: usize) -> bool {
    format!("{error:?}") == format!("Heap(HeapLimitExceeded {{ limit: {budget} }})")
}

/// One attempt: a VM with `budget` bytes, optionally with the Test262 host
/// installed, running `run`. `Ok(true)` when it completed, `Ok(false)` when the
/// heap limit stopped it.
fn attempt(
    budget: usize,
    harness: bool,
    run: &dyn Fn(&mut Vm) -> Result<Value, RuntimeError>,
) -> bool {
    let mut vm = Vm::new(config(budget)).unwrap();
    let mut outcome = if harness {
        vm.install_test262_harness().map(|()| Value::Undefined)
    } else {
        Ok(Value::Undefined)
    };
    if outcome.is_ok() {
        outcome = run(&mut vm);
    }
    match outcome {
        Ok(_) => true,
        Err(error) => {
            assert!(is_heap_limit(&error, budget), "budget {budget}: {error:?}");
            false
        }
    }
}

/// The smallest budget (to within 64 bytes) at which `run` completes.
fn smallest_budget(harness: bool, run: &dyn Fn(&mut Vm) -> Result<Value, RuntimeError>) -> usize {
    let (mut low, mut high) = (512, 64 * 1024 * 1024);
    while high - low > 64 {
        let middle = low + (high - low) / 2;
        if attempt(middle, harness, run) {
            high = middle;
        } else {
            low = middle;
        }
    }
    high
}

/// Sweeps every budget in `range` (counted back from the success budget) by
/// `step`, returning how many attempts were stopped by the heap limit.
fn sweep(
    harness: bool,
    run: &dyn Fn(&mut Vm) -> Result<Value, RuntimeError>,
    back_from: usize,
    back_to: usize,
    step: usize,
) -> usize {
    let needed = smallest_budget(harness, run);
    let start = needed.saturating_sub(back_from).max(512);
    let end = (needed + back_to).max(start);
    let mut stopped = 0;
    let mut budget = start;
    while budget <= end {
        stopped += usize::from(!attempt(budget, harness, run));
        budget += step;
    }
    assert!(attempt(needed + 64, harness, run));
    stopped
}

fn script(source: &'static str) -> impl Fn(&mut Vm) -> Result<Value, RuntimeError> {
    let program: Bytecode = compile(&parse(source).unwrap()).unwrap();
    move |vm| vm.execute(&program)
}

#[test]
fn installing_the_test262_host_fails_cleanly_at_every_allocation() {
    let run = script("0");
    let stopped = sweep(true, &run, usize::MAX, 0, 32);
    assert!(stopped > 1000);
}

fn module_graph(source: &str) -> HashMap<String, Bytecode> {
    let module = blueice_bluejs::parse_module(source).unwrap();
    HashMap::from([(
        "t/main.js".to_string(),
        blueice_bluejs::compile_module(&module).unwrap(),
    )])
}

#[test]
fn synthetic_modules_fail_cleanly_when_the_heap_fills() {
    let graph = module_graph(
        "import bytes from './x.bin' with { type: 'bytes' };
         import text from './x.txt' with { type: 'text' };
         bytes.length + text.length",
    );
    let run = |vm: &mut Vm| {
        vm.set_bytes_module_sources(HashMap::from([("t/x.bin".to_string(), vec![1, 2, 3])]));
        vm.set_text_module_sources(HashMap::from([("t/x.txt".to_string(), "abc".to_string())]));
        vm.execute_module_graph("t/main.js", &graph)
    };
    let stopped = sweep(false, &run, 30_000, 0, 16) + sweep(false, &run, usize::MAX, 0, 1024);
    assert!(stopped > 0);
}
