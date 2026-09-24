// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Helpers shared by the `cov_g3_*` coverage tests.
#![allow(dead_code)]

use blueice_bluejs::{compile, parse, Bytecode, HeapConfig, RuntimeError, Value, Vm, VmConfig};

/// Runs `source` on a default VM.
pub fn run(source: &str) -> Result<Value, RuntimeError> {
    Vm::default().execute(&compile(&parse(source).unwrap()).unwrap())
}

/// Runs `source` as a script that reports through `$DONE` once its promise
/// jobs have run: `Some(Ok(()))` for `$DONE()`, `Some(Err(..))` for a failure.
pub fn run_async(source: &str) -> Option<Result<(), String>> {
    run_async_with(VmConfig::default(), source)
}

pub fn run_async_with(config: VmConfig, source: &str) -> Option<Result<(), String>> {
    let mut vm = Vm::new(config).unwrap();
    vm.install_test262_done().unwrap();
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap();
    vm.run_promise_jobs().unwrap();
    vm.take_test262_done()
        .map(|done| done.map_err(|error| format!("{error:?}")))
}

/// Runs `source` and expects it to evaluate to `true`.
pub fn assert_true(source: &str) {
    assert_eq!(run(source), Ok(Value::Bool(true)), "{source}");
}

/// A script that calls `factory` (JavaScript source of a function taking one
/// options object) once per option-read position and per failure mode: the
/// read at that position throws, returns a value whose conversion throws,
/// or returns a string, negative, fractional or huge number. Every call
/// must either succeed or throw (never crash); evaluates to `true` once at
/// least one call threw.
pub fn option_read_sweep(factory: &str) -> String {
    format!(
        "const factory = {factory};
         let thrown = 0;
         for (const mode of ['throw', 'coerce', 'bogus', 'negative', 'fraction', 'huge', 'object']) {{
           for (let position = 1; position <= 40; position++) {{
             let reads = 0;
             const options = new Proxy({{}}, {{
               get(target, key) {{
                 if (typeof key === 'symbol') return undefined;
                 if (++reads !== position) return undefined;
                 switch (mode) {{
                   case 'throw': throw new EvalError('get');
                   case 'coerce': return {{ valueOf() {{ throw new EvalError('value') }}, toString() {{ throw new EvalError('text') }} }};
                   case 'bogus': return 'bogus';
                   case 'negative': return -1;
                   case 'fraction': return 1.5;
                   case 'huge': return 1e9;
                   default: return {{}};
                 }}
               }}
             }});
             try {{ factory(options) }} catch (e) {{ thrown++ }}
           }}
         }}
         thrown > 0"
    )
}

/// The managed bytes of a VM that has executed each of `sources` in turn.
fn managed_bytes_after(sources: &[&str], harness: bool) -> usize {
    let mut vm = Vm::default();
    if harness {
        vm.install_test262_harness().unwrap();
    }
    for source in sources {
        let _ = vm.execute(&compile(&parse(source).unwrap()).unwrap());
    }
    vm.heap().stats().managed_bytes
}

/// Executes `source` on VMs whose heap ceiling rises in small steps from the
/// size of a VM that has only executed `warm_up` (whose lazily built
/// intrinsics and stored objects the operation must not be charged for; pass
/// `"0;"` to charge it everything) to past what the script needs, so that
/// each allocation the script performs fails in turn. The step is 8 bytes for
/// a small operation and grows with the operation's size, so that a large one
/// still finishes quickly. The sweep repeats for several nursery sizes
/// because the nursery size decides which allocation call is the one that has
/// to make room. Each outcome must be either `Ok` or the heap-limit error,
/// never another error or a panic. Returns how many runs hit the limit.
pub fn heap_limit_sweep(warm_up: &str, source: &str) -> usize {
    sweep(warm_up, source, false)
}

/// [`heap_limit_sweep`] on VMs with the Test262 host harness (`$262`)
/// installed.
pub fn heap_limit_sweep_with_harness(warm_up: &str, source: &str) -> usize {
    sweep(warm_up, source, true)
}

/// A VM with the heap ceiling `limit` that has run `warm_up`, or `None` when
/// the ceiling is too small for that.
fn build(limit: usize, nursery_capacity: usize, harness: bool, warm_up: &Bytecode) -> Option<Vm> {
    let mut vm = Vm::new(VmConfig {
        heap: HeapConfig {
            nursery_capacity,
            major_threshold_bytes: limit,
            max_heap_bytes: limit,
        },
        ..VmConfig::default()
    })
    .ok()?;
    if harness {
        vm.install_test262_harness().ok()?;
    }
    vm.execute(warm_up).ok()?;
    Some(vm)
}

fn sweep(warm_up: &str, source: &str, harness: bool) -> usize {
    let warm_code = compile(&parse(warm_up).unwrap()).unwrap();
    let base = managed_bytes_after(&[warm_up], harness);
    let needed = managed_bytes_after(&[warm_up, source], harness).saturating_sub(base);
    let step = (needed / 250 / 8 * 8).clamp(8, 128);
    // The least ceiling that still builds: it only has to hold the live
    // objects, since a full heap is collected before an allocation fails.
    let (mut low, mut high) = (0, base);
    while high - low > 8 {
        let middle = low + (high - low) / 2;
        if build(middle, 1, harness, &warm_code).is_some() {
            high = middle;
        } else {
            low = middle;
        }
    }
    let mut failures = 0;
    for (nursery_capacity, throwaways) in [(1, 0), (2, 1)] {
        // Throwaway allocations just before the operation shift which of its
        // allocation calls is the one that has to make room.
        let code =
            compile(&parse(&format!("{}{source}", "[];".repeat(throwaways))).unwrap()).unwrap();
        let mut successes_in_a_row = 0;
        for limit in (high..).step_by(step) {
            let Some(mut vm) = build(limit, nursery_capacity, harness, &warm_code) else {
                failures += 1;
                continue;
            };
            let outcome = vm
                .execute(&code)
                .and_then(|_| vm.run_promise_jobs().map(|_| Value::Undefined));
            match outcome {
                Ok(_) => successes_in_a_row += 1,
                Err(RuntimeError::Heap(_)) => {
                    failures += 1;
                    successes_in_a_row = 0;
                }
                Err(other) => panic!("limit {limit}: unexpected {other:?} for {source}"),
            }
            if successes_in_a_row == 16 {
                break;
            }
        }
    }
    failures
}

/// [`heap_limit_sweep`] for each `(warm_up, operation)` pair. An operation
/// that never allocates on the managed heap simply never fails.
pub fn sweep_each(cases: &[(&str, &str)]) {
    for (warm_up, operation) in cases {
        heap_limit_sweep(warm_up, operation);
    }
}

/// Executes `source` with the per-execution instruction budget rising one
/// instruction at a time until the script completes, so that the budget runs
/// out at each point the script charges for a step. Running out is an engine
/// error (`InstructionLimit`), never a panic or another error. Returns how
/// many budgets were too small.
pub fn instruction_budget_sweep(source: &str) -> usize {
    let code = compile(&parse(source).unwrap()).unwrap();
    let mut exhausted = 0;
    for budget in 1.. {
        let mut vm = Vm::new(VmConfig {
            instruction_budget: budget,
            ..VmConfig::default()
        })
        .unwrap();
        let outcome = vm
            .execute(&code)
            .and_then(|_| vm.run_promise_jobs().map(|_| Value::Undefined));
        match outcome {
            Ok(_) => return exhausted,
            Err(RuntimeError::InstructionLimit) => exhausted += 1,
            Err(other) => panic!("budget {budget}: unexpected {other:?} for {source}"),
        }
    }
    unreachable!("the budget grows without bound")
}

/// [`sweep_each`] on VMs with the Test262 host harness installed.
pub fn sweep_each_with_harness(cases: &[(&str, &str)]) {
    for (warm_up, operation) in cases {
        heap_limit_sweep_with_harness(warm_up, operation);
    }
}
