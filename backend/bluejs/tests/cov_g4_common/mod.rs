// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Shared helpers for the `cov_g4_*` integration tests: a tiny JavaScript
//! assertion prelude, and resource-exhaustion sweeps that fail every heap
//! allocation, instruction step and string growth of a script in turn.

#![allow(dead_code)]

pub mod test262;

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
    // An allocation's failure window is as wide as the bytes it is charged
    // (an ArrayBuffer's are just its length), so unless the range is too wide
    // to try every limit, try every limit.
    let step = ((needed - floor) / max_runs).max(1);
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

/// [`sweep_heap_runs`] with a filler kept alive before `body` starts. The
/// limits tried begin at the setup's own peak, where the collector still
/// reclaims the setup's garbage, so an allocation early in `body` would never
/// be the one to fail; the filler lifts everything after it above that peak.
pub fn sweep_heap_filled(setup: &str, body: &str) {
    let filled = format!("var __filler = new Uint8Array(32768); {body}");
    sweep_heap_runs(setup, &filled, 8000);
}

/// Fails the heap allocations of `script` run on a bare VM -- nothing, not
/// even the assertion prelude, has run first -- so the lazy creation of every
/// intrinsic the script is the first to touch is swept as well. Every limit
/// from the smallest VM up to the smallest that lets the script finish is
/// tried (at most `max_runs` of them, evenly spread); each run must end
/// normally or in a heap error.
pub fn sweep_heap_bare(script: &str, max_runs: u64) {
    let run = |limit: u64| -> Result<Value, RuntimeError> {
        let mut config = VmConfig::default();
        config.heap.max_heap_bytes = limit as usize;
        config.heap.major_threshold_bytes = config.heap.major_threshold_bytes.min(limit as usize);
        let mut vm = Vm::new(config).map_err(RuntimeError::Heap)?;
        vm.execute(&compile(&parse(script).unwrap()).unwrap())
    };
    let created = |limit: u64| {
        let mut config = VmConfig::default();
        config.heap.max_heap_bytes = limit as usize;
        config.heap.major_threshold_bytes = config.heap.major_threshold_bytes.min(limit as usize);
        Vm::new(config).is_ok()
    };
    let mut high = 1 << 16;
    while run(high).is_err() {
        high *= 2;
        assert!(
            high <= 1 << 28,
            "the script never finishes: {:?}",
            run(high)
        );
    }
    let (mut low, mut floor) = (0, high);
    while !created(floor) {
        floor *= 2;
    }
    let mut needed = high;
    while needed - low > 1 {
        let middle = (low + needed) / 2;
        if run(middle).is_ok() {
            needed = middle;
        } else {
            low = middle;
        }
    }
    low = 0;
    while floor - low > 1 {
        let middle = (low + floor) / 2;
        if created(middle) {
            floor = middle;
        } else {
            low = middle;
        }
    }
    let step = ((needed - floor) / max_runs).max(1);
    let mut limit = floor;
    while limit <= needed {
        let result = run(limit);
        assert!(
            result.is_ok() || matches!(&result, Err(RuntimeError::Heap(_))),
            "limit {limit}: {result:?}"
        );
        limit += step;
    }
}

/// Runs each of `sources` in order as its own classic script on one shared VM
/// (the Test262 harness installed) and returns how each one ended: `ok`
/// followed by its completion value, or the error it reported.
pub fn scripts(sources: &[&str]) -> Vec<String> {
    let mut vm = Vm::new(VmConfig::default()).unwrap();
    vm.install_test262_harness().unwrap();
    sources
        .iter()
        .map(|source| {
            let program = match parse(source) {
                Ok(program) => program,
                Err(error) => return format!("SyntaxError: {}", error.message),
            };
            let code = match compile(&program) {
                Ok(code) => code,
                Err(error) => return format!("SyntaxError: {error}"),
            };
            match vm.execute_script(&code) {
                Ok(Value::String(text)) => format!("ok {}", text.to_utf8().unwrap_or_default()),
                Ok(Value::Number(number)) => format!("ok {number}"),
                Ok(Value::Bool(flag)) => format!("ok {flag}"),
                Ok(Value::Undefined) => "ok undefined".into(),
                Ok(other) => format!("ok {other:?}"),
                Err(error) => error.to_string(),
            }
        })
        .collect()
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
