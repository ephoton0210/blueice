// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Failure-injection sweeps for the `cov_g2_*` test targets.
//!
//! The VM has no allocation-failure hook, but two of its limits can be swept:
//! the managed-heap ceiling and the runtime string-length limit. Running a
//! script under every ceiling between "what the set-up needs" and "what the
//! script needs" makes each allocation the failing one in turn, which is how
//! the error path behind every allocating call is exercised. Only an
//! allocation that pushes the live bytes past a *new high-water mark* of the
//! run can fail first, so scripts call `b()` (a few kilobytes of live ballast)
//! before each operation they want reached: it outgrows whatever transient
//! state the previous operation left behind.
#![allow(dead_code)]

use blueice_bluejs::{
    compile, compile_module, parse, parse_module, HeapConfig, HeapError, RuntimeError, Value, Vm,
    VmConfig,
};

/// How a swept script is run.
#[derive(Clone, Copy)]
pub struct Mode {
    /// Install the Test262 host functions (`assert`, `compareArray`, ...).
    pub harness: bool,
    /// Run the script as a classic script and then drain the promise jobs, so
    /// that the work done after an `await` is swept too.
    pub jobs: bool,
    /// Compile the script as a module (where a top-level `await` is legal).
    pub module: bool,
}

impl Mode {
    pub const PLAIN: Mode = Mode {
        harness: false,
        jobs: false,
        module: false,
    };
    pub const JOBS: Mode = Mode {
        harness: false,
        jobs: true,
        module: false,
    };
    pub const MODULE: Mode = Mode {
        harness: false,
        jobs: true,
        module: true,
    };
    pub const HARNESS: Mode = Mode {
        harness: true,
        jobs: false,
        module: false,
    };
}

fn new_vm(config: VmConfig, mode: Mode) -> Option<Vm> {
    let mut vm = Vm::new(config).ok()?;
    if mode.harness {
        vm.install_test262_harness().ok()?;
    }
    Some(vm)
}

fn run(vm: &mut Vm, source: &str, mode: Mode) -> Result<Value, RuntimeError> {
    if mode.module {
        return vm.execute(&compile_module(&parse_module(source).unwrap()).unwrap());
    }
    let code = compile(&parse(source).unwrap()).unwrap();
    if mode.jobs {
        let value = vm.execute_script(&code)?;
        vm.run_promise_jobs()?;
        Ok(value)
    } else {
        vm.execute(&code)
    }
}

fn heap_config(ceiling: usize) -> VmConfig {
    VmConfig {
        heap: HeapConfig {
            nursery_capacity: 1,
            max_heap_bytes: ceiling,
            ..HeapConfig::default()
        },
        ..VmConfig::default()
    }
}

/// The smallest value at which `attempt` succeeds (found by doubling and then
/// bisecting; success is assumed monotone).
fn smallest(start: usize, attempt: impl Fn(usize) -> bool) -> usize {
    let mut high = start;
    while !attempt(high) {
        assert!(high < 1 << 30, "the swept script never succeeds");
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

/// A heap ceiling that is not held up by the default major-collection
/// threshold, so it can be as small as the VM's own set-up allows.
fn low_heap_config(ceiling: usize) -> VmConfig {
    VmConfig {
        heap: HeapConfig {
            nursery_capacity: 1,
            major_threshold_bytes: ceiling,
            max_heap_bytes: ceiling,
        },
        ..VmConfig::default()
    }
}

fn is_heap_limit(result: &Result<Value, RuntimeError>) -> bool {
    matches!(
        result,
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { .. }))
    )
}

/// Runs `script` (after `warmup`) under every heap ceiling from what the
/// warm-up needs to what the script needs, `step` bytes apart. The script must
/// evaluate to `true`; a smaller ceiling may instead end in a heap-limit error,
/// but never in a panic or any other failure.
pub fn sweep_heap(mode: Mode, warmup: &str, script: &str, step: usize) {
    let succeeds = |limit: usize, with_script: bool| {
        new_vm(heap_config(limit), mode).is_some_and(|mut vm| {
            run(&mut vm, warmup, mode).is_ok()
                && (!with_script || run(&mut vm, script, mode) == Ok(Value::Bool(true)))
        })
    };
    let floor = smallest(4096, |limit| succeeds(limit, false));
    let ceiling = smallest(floor, |limit| succeeds(limit, true));
    for limit in (floor..=ceiling).step_by(step) {
        let Some(mut vm) = new_vm(heap_config(limit), mode) else {
            continue;
        };
        if run(&mut vm, warmup, mode).is_err() {
            continue;
        }
        let result = run(&mut vm, script, mode);
        assert!(
            result == Ok(Value::Bool(true)) || is_heap_limit(&result),
            "ceiling {limit}: {script}: {result:?}"
        );
    }
}

/// [`sweep_heap`] over `body`, a script fragment that must evaluate to `true`
/// and calls `b()` before each operation it wants to fail.
pub fn sweep_ops(mode: Mode, warmup: &str, body: &str, step: usize) {
    let warmup = format!(
        "{warmup}; globalThis.keep = [];
         globalThis.b = function () {{ keep.push('x'.repeat(800) + keep.length); }};
         keep.push('y'.repeat(20000)); 0"
    );
    sweep_heap(
        mode,
        &warmup,
        &format!("(function () {{ {body} }})()"),
        step,
    );
}

/// A cold sweep: `script` runs in a VM that has done nothing yet, so every
/// intrinsic it touches is materialized under the swept ceiling.
pub fn sweep_cold(mode: Mode, script: &str, step: usize) {
    sweep_heap(mode, "0", script, step);
}

/// Runs `script` under every string-length limit from `first` (large enough
/// for the VM's own bootstrapping) upward, until the limit stops being the
/// reason the script ends. While it is, the error must be a string-limit
/// error; the outcome at the first limit that does not trip is checked with
/// `accepted` (a script may legitimately end in another error, such as a
/// failed assertion).
pub fn sweep_string_limit(
    mode: Mode,
    script: &str,
    first: usize,
    step: usize,
    accepted: impl Fn(&Result<Value, RuntimeError>) -> bool,
) {
    let mut limit = first;
    loop {
        let config = VmConfig {
            max_string_bytes: limit,
            ..VmConfig::default()
        };
        let Some(mut vm) = new_vm(config, mode) else {
            limit += step;
            continue;
        };
        let result = run(&mut vm, script, mode);
        if !matches!(result, Err(RuntimeError::StringLimit { .. })) {
            assert!(accepted(&result), "limit {limit}: {script}: {result:?}");
            return;
        }
        limit += step;
    }
}

/// Runs `script` under every instruction budget from 1 upward until it
/// completes with `expected`; every smaller budget must fail with the
/// instruction limit.
pub fn sweep_budget(mode: Mode, script: &str, expected: Value) {
    let mut budget = 1;
    loop {
        let config = VmConfig {
            instruction_budget: budget,
            ..VmConfig::default()
        };
        let mut vm = new_vm(config, mode).unwrap();
        match run(&mut vm, script, mode) {
            Err(RuntimeError::InstructionLimit) => budget += 1,
            other => {
                assert_eq!(other, Ok(expected), "budget {budget}: {script}");
                return;
            }
        }
    }
}

/// [`sweep_heap`] for a script that is expected to end in an error (or any
/// non-`true` value): `accepted` judges the result of every run that is not
/// cut short by the heap ceiling. `step` is in bytes. The ceiling is not held
/// up by the default collection threshold (see [`low_heap_config`]), so an
/// allocation fails exactly when the *live* data no longer leaves it room.
pub fn sweep_heap_by(
    mode: Mode,
    warmup: &str,
    script: &str,
    step: usize,
    accepted: impl Fn(&Result<Value, RuntimeError>) -> bool,
) {
    let succeeds = |limit: usize, with_script: bool| {
        new_vm(low_heap_config(limit), mode).is_some_and(|mut vm| {
            run(&mut vm, warmup, mode).is_ok()
                && (!with_script || !is_heap_limit(&run(&mut vm, script, mode)))
        })
    };
    let floor = smallest(4096, |limit| succeeds(limit, false));
    let ceiling = smallest(floor, |limit| succeeds(limit, true));
    for limit in (floor..=ceiling).step_by(step) {
        let Some(mut vm) = new_vm(low_heap_config(limit), mode) else {
            continue;
        };
        if run(&mut vm, warmup, mode).is_err() {
            continue;
        }
        let result = run(&mut vm, script, mode);
        assert!(
            is_heap_limit(&result) || accepted(&result),
            "ceiling {limit}: {script}: {result:?}"
        );
    }
}

/// [`sweep_heap_by`] over `body`, with the `b()` ballast helper installed.
/// Unlike [`sweep_ops`], `b()` builds nothing (it stores a string made once
/// during the warm-up), so the ballast leaves no transient garbage that would
/// mask the allocation right after it. `bb()` stores twelve pieces at once:
/// a run's earlier phases can have held more live data than a late job does,
/// and only a new peak of live data can be the first thing to fail.
pub fn sweep_ops_by(
    mode: Mode,
    warmup: &str,
    body: &str,
    step: usize,
    accepted: impl Fn(&Result<Value, RuntimeError>) -> bool,
) {
    let warmup = format!(
        "{warmup}; globalThis.keep = [];
         globalThis.piece = 'x'.repeat(800);
         globalThis.b = function () {{ keep.push(piece); }};
         globalThis.bb = function () {{ for (var i = 0; i < 12; i++) keep.push(piece); }};
         keep.push('y'.repeat(20000)); 0"
    );
    sweep_heap_by(
        mode,
        &warmup,
        &format!("(function () {{ {body} }})()"),
        step,
        accepted,
    );
}

/// Runs a native operation on a cold VM under every heap ceiling between what
/// creating the VM needs and what the operation needs, `step` bytes apart.
/// The operation must succeed or fail with a heap-limit error.
pub fn sweep_native(step: usize, operation: impl Fn(&mut Vm) -> Result<(), RuntimeError>) {
    let succeeds = |limit: usize, with_operation: bool| {
        Vm::new(low_heap_config(limit))
            .ok()
            .is_some_and(|mut vm| !with_operation || operation(&mut vm).is_ok())
    };
    let floor = smallest(4096, |limit| succeeds(limit, false));
    let ceiling = smallest(floor, |limit| succeeds(limit, true));
    for limit in (floor..=ceiling).step_by(step) {
        let Some(mut vm) = Vm::new(low_heap_config(limit)).ok() else {
            continue;
        };
        let result = operation(&mut vm);
        assert!(
            result.is_ok()
                || matches!(
                    result,
                    Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { .. }))
                ),
            "ceiling {limit}: {result:?}"
        );
    }
}

/// [`sweep_heap_by`] for a cold VM under a ceiling as low as its set-up
/// allows: the first operation to need each lazily built intrinsic is the one
/// that builds it while the ceiling is being closed in on.
pub fn sweep_cold_by(
    mode: Mode,
    script: &str,
    step: usize,
    accepted: impl Fn(&Result<Value, RuntimeError>) -> bool,
) {
    let succeeds = |limit: usize, with_script: bool| {
        new_vm(low_heap_config(limit), mode)
            .is_some_and(|mut vm| !with_script || !is_heap_limit(&run(&mut vm, script, mode)))
    };
    let floor = smallest(4096, |limit| succeeds(limit, false));
    let ceiling = smallest(floor, |limit| succeeds(limit, true));
    for limit in (floor..=ceiling).step_by(step) {
        let Some(mut vm) = new_vm(low_heap_config(limit), mode) else {
            continue;
        };
        let result = run(&mut vm, script, mode);
        assert!(
            is_heap_limit(&result) || accepted(&result),
            "ceiling {limit}: {script}: {result:?}"
        );
    }
}

/// [`sweep_cold_by`] for a script that must evaluate to `true`.
pub fn sweep_cold_true(mode: Mode, script: &str, step: usize) {
    sweep_cold_by(mode, script, step, |result| {
        *result == Ok(Value::Bool(true))
    });
}

/// What one run of [`sweep_jobs_after_ballast`] came to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum JobsOutcome {
    /// Everything completed.
    Done,
    /// The ceiling was reached while the promise jobs ran.
    JobsFailed,
    /// The ballast alone already reached the ceiling.
    BallastFailed,
}

/// Runs `script` (which leaves promise jobs pending), then fills the heap with
/// ballast, then runs the promise jobs. The script's own transient allocations
/// are behind it by then, so an allocation made by a job fails as soon as the
/// live data no longer leaves it room: the sweep is over how much ballast is
/// added (`step` two-byte units at a time), from the largest amount under
/// which every job still completes up to the amount that fills the heap on
/// its own. Every job must complete or hit the ceiling.
pub fn sweep_jobs_after_ballast(warmup: &str, script: &str, step: usize) {
    let mode = Mode::PLAIN;
    let prelude = "globalThis.keep = []; globalThis.piece = 'x'.repeat(800); 0";
    let sync_succeeds = |limit: usize| {
        new_vm(low_heap_config(limit), mode).is_some_and(|mut vm| {
            [prelude, warmup, script].iter().all(|source| {
                vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
                    .is_ok()
            })
        })
    };
    let floor = smallest(4096, sync_succeeds);
    let ceiling = floor + 96 * 1024;
    let attempt = |units: usize| {
        let mut vm = new_vm(low_heap_config(ceiling), mode).expect("the ceiling has room to start");
        for source in [prelude, warmup, script] {
            let result = vm.execute_script(&compile(&parse(source).unwrap()).unwrap());
            assert!(result.is_ok(), "{source}: {result:?}");
        }
        let ballast = format!(
            "for (var i = 0; i < {}; i++) keep.push(piece); keep.push(piece.slice(0, {})); 0",
            units / 800,
            units % 800
        );
        let filled = vm.execute_script(&compile(&parse(&ballast).unwrap()).unwrap());
        if is_heap_limit(&filled) {
            return JobsOutcome::BallastFailed;
        }
        assert!(filled.is_ok(), "{ballast}: {filled:?}");
        match vm.run_promise_jobs() {
            Ok(()) => JobsOutcome::Done,
            Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { .. })) => JobsOutcome::JobsFailed,
            Err(error) => panic!("{script}: {error:?}"),
        }
    };
    // The largest ballast under which the jobs complete.
    let mut low = 0;
    let mut high = ceiling / 2;
    assert_eq!(attempt(low), JobsOutcome::Done, "{script}");
    while low + 1 < high {
        let middle = low + (high - low) / 2;
        if attempt(middle) == JobsOutcome::Done {
            low = middle;
        } else {
            high = middle;
        }
    }
    let mut units = low + 1;
    while attempt(units) != JobsOutcome::BallastFailed {
        units += step;
    }
}
