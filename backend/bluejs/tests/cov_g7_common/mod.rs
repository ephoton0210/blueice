// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Shared helpers for the `cov_g7_*` integration tests: a tiny JavaScript
//! assertion prelude, and resource-exhaustion sweeps that fail every heap
//! allocation, instruction step and string growth of a script in turn.

#![allow(dead_code)]

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm, VmConfig};

/// Assertion prelude: `eq(name, actual, expected)` and
/// `th(name, fn, ErrorConstructor)` record failures; `after`/`afterRejected`
/// check a promise's settlement once the jobs have drained (an unsettled one
/// is reported as pending). The failure list is read back last.
const PRELUDE: &str = "var __f=[];\
     function eq(n,a,b){if(a!==b)__f.push(n+': '+a+' !== '+b)}\
     function th(n,f,E){try{f();__f.push(n+': no throw')}catch(e){if(!(e instanceof E))__f.push(n+': wrong error '+e)}}\
     var __pending=0;\
     function after(n,p,f){__pending++;p.then(function(v){__pending--;f(v)},function(e){__pending--;__f.push(n+': rejected '+e)})}\
     function afterRejected(n,p,f){__pending++;p.then(function(v){__pending--;__f.push(n+': resolved')},function(e){__pending--;f(e)})}\n";

pub fn run_with(
    body: &str,
    harness: bool,
    tweak: &dyn Fn(&mut VmConfig),
) -> Result<Value, RuntimeError> {
    let mut config = VmConfig::default();
    tweak(&mut config);
    let mut vm = Vm::new(config).map_err(RuntimeError::Heap)?;
    if harness {
        vm.install_test262_harness()?;
    }
    let script = format!("{PRELUDE}try {{\n{body}\n}} catch (e) {{ __f.push('uncaught: ' + e) }}");
    vm.execute_script(&compile(&parse(&script).unwrap()).unwrap())?;
    vm.run_promise_jobs()?;
    vm.execute(
        &compile(
            &parse("(__pending ? 'pending:' + __pending + ' ' : '') + __f.join(' | ')").unwrap(),
        )
        .unwrap(),
    )
}

pub fn eval(body: &str) -> Result<Value, RuntimeError> {
    run_with(body, true, &|_| {})
}

/// Runs `body` after the assertion prelude (draining promise jobs) and
/// returns the failure list: empty when every check held.
pub fn failures(body: &str) -> String {
    match eval(body) {
        Ok(Value::String(text)) => text.to_utf8().unwrap(),
        other => panic!("unexpected result {other:?}"),
    }
}

/// The Test262 harness is only installed for scripts that use `$262`: it
/// costs far more than the scripts being swept.
fn needs_harness(body: &str) -> bool {
    body.contains("$262")
}

fn passes(result: &Result<Value, RuntimeError>) -> bool {
    matches!(result, Ok(Value::String(text)) if text.is_empty())
}

/// Smallest value of a resource setting (found by doubling from `start`, then
/// bisecting) under which a script completes without a failed check.
fn smallest(start: u64, cap: u64, attempt: &dyn Fn(u64) -> Result<Value, RuntimeError>) -> u64 {
    let mut high = start;
    while !passes(&attempt(high)) {
        high *= 2;
        assert!(high <= cap, "the script never passes: {:?}", attempt(high));
    }
    let mut low = high / 2;
    while high - low > 1 {
        let middle = (low + high) / 2;
        if passes(&attempt(middle)) {
            high = middle;
        } else {
            low = middle;
        }
    }
    high
}

/// Fails the heap allocations `body` makes after `setup` has run, in turn:
/// for a dense range of `max_heap_bytes` values between what `setup` alone and
/// `setup` followed by `body` need, the run must end either normally (with no
/// failed check) or in a heap error. At most 1500 limits are tried,
/// evenly spread.
pub fn sweep_heap_after(setup: &str, body: &str) {
    sweep_heap_runs(setup, body, 1500);
}

/// [`sweep_heap_after`] with an explicit cap on the number of limits tried.
pub fn sweep_heap_runs(setup: &str, body: &str, max_runs: u64) {
    let script = format!("{setup}\n{body}");
    let harness = needs_harness(&script);
    let limited = |source: &str, limit: u64| {
        run_with(source, harness, &|config| {
            config.heap.max_heap_bytes = limit as usize;
            config.heap.major_threshold_bytes =
                config.heap.major_threshold_bytes.min(limit as usize);
        })
    };
    let needed = smallest(1 << 18, 1 << 28, &|limit| limited(&script, limit));
    let floor = smallest(1 << 18, 1 << 28, &|limit| limited(setup, limit));
    // Every allocation is charged at least a few tens of bytes, so a step of
    // eight cannot skip past one allocation's whole failure window.
    let step = ((needed - floor) / max_runs).max(8);
    let mut limit = floor;
    while limit <= needed {
        let result = limited(&script, limit);
        assert!(
            passes(&result) || matches!(&result, Err(RuntimeError::Heap(_))),
            "limit {limit}: {result:?}"
        );
        limit += step;
    }
}

/// Exhausts the instruction budget at every step `body` takes after `setup`.
pub fn sweep_fuel_after(setup: &str, body: &str) {
    let script = format!("{setup}\n{body}");
    let harness = needs_harness(&script);
    let limited = |source: &str, budget: u64| {
        run_with(source, harness, &|config| {
            config.instruction_budget = budget
        })
    };
    let needed = smallest(64, 1 << 30, &|budget| limited(&script, budget));
    let floor = smallest(64, 1 << 30, &|budget| limited(setup, budget));
    for budget in floor..=needed {
        let result = limited(&script, budget);
        assert!(
            passes(&result) || result == Err(RuntimeError::InstructionLimit),
            "budget {budget}: {result:?}"
        );
    }
}

/// [`sweep_heap_after`] over each `(setup, body)` pair in turn.
pub fn sweep_heap_each(pairs: &[(&str, &str)]) {
    for (setup, body) in pairs {
        sweep_heap_after(setup, body);
    }
}

/// [`sweep_fuel_after`] over each `(setup, body)` pair in turn.
pub fn sweep_fuel_each(pairs: &[(&str, &str)]) {
    for (setup, body) in pairs {
        sweep_fuel_after(setup, body);
    }
}

/// Pairs every body with the same `setup`.
pub fn with_setup<'a>(setup: &'a str, bodies: &[&'a str]) -> Vec<(&'a str, &'a str)> {
    bodies.iter().map(|body| (setup, *body)).collect()
}

/// Lowers the per-string byte limit through every value up to `longest_bytes`
/// (without the Test262 harness, whose own strings would hit the limit first).
pub fn sweep_strings(body: &str, longest_bytes: usize) {
    for limit in 0..=longest_bytes {
        let result = run_with(body, false, &|config| config.max_string_bytes = limit);
        assert!(
            passes(&result) || matches!(&result, Err(RuntimeError::StringLimit { .. })),
            "string limit {limit}: {result:?}"
        );
    }
}
