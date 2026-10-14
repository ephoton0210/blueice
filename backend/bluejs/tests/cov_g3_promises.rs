// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The static Promise combinators: every combinator with plain, thenable and
//! failing inputs, custom constructors whose capability or `resolve` misbehave,
//! the keyed forms, and allocation failure at each step.

mod cov_g3_support;
use blueice_bluejs::{compile, parse, RuntimeError, Vm, VmConfig};
use cov_g3_support::{heap_limit_sweep, instruction_budget_sweep, run_async, sweep_each};

/// Prelude: `check(condition, message)` reports through `$DONE`.
const PRELUDE: &str = "
    function check(condition, message) {
      if (condition) $DONE(); else $DONE(new Error(message));
    }
    function same(a, b) { return JSON.stringify(a) === JSON.stringify(b); }
";

fn scenario(body: &str) {
    let source = format!("{PRELUDE}\n{body}");
    assert_eq!(run_async(&source), Some(Ok(())), "{body}");
}

#[test]
fn all_collects_values_from_plain_values_promises_and_thenables() {
    scenario(
        "Promise.all([1, Promise.resolve(2), { then(resolve) { resolve(3) } }])
           .then((values) => check(same(values, [1, 2, 3]), 'all ' + JSON.stringify(values)));",
    );
    scenario("Promise.all([]).then((values) => check(same(values, []), 'empty all'));");
    scenario(
        "Promise.all([Promise.resolve(1), Promise.reject(new Error('no'))])
           .then(() => check(false, 'resolved'), (error) => check(error.message === 'no', 'reason'));",
    );
    scenario("Promise.all(new Set([4, 5])).then((values) => check(same(values, [4, 5]), 'set'));");
}

#[test]
fn all_settled_reports_status_records() {
    scenario(
        "Promise.allSettled([1, Promise.reject(2), Promise.resolve(3)]).then((results) =>
           check(same(results, [
             { status: 'fulfilled', value: 1 },
             { status: 'rejected', reason: 2 },
             { status: 'fulfilled', value: 3 }]), JSON.stringify(results)));",
    );
    scenario("Promise.allSettled([]).then((results) => check(same(results, []), 'empty'));");
}

#[test]
fn any_and_race_pick_the_first_settlement() {
    scenario(
        "Promise.any([Promise.reject(1), Promise.resolve(2), 3]).then((value) => check(value === 2, 'any ' + value));",
    );
    scenario(
        "Promise.any([Promise.reject(1), Promise.reject(2)]).then(() => check(false, 'resolved'), (error) =>
           check(error instanceof AggregateError && same(error.errors, [1, 2]), 'aggregate'));",
    );
    scenario(
        "Promise.any([]).then(() => check(false, 'resolved'), (error) =>
           check(error instanceof AggregateError && error.errors.length === 0, 'empty any'));",
    );
    scenario(
        "Promise.race([new Promise(() => {}), Promise.resolve(7)]).then((value) => check(value === 7, 'race'));",
    );
    scenario(
        "Promise.race([Promise.reject(new Error('r')), Promise.resolve(1)]).then(() => check(false, 'resolved'),
           (error) => check(error.message === 'r', 'race reject'));",
    );
    scenario(
        "Promise.race([]);
         Promise.resolve().then(() => check(true, 'a race of nothing stays pending'));",
    );
}

#[test]
fn combinators_reject_their_promise_when_the_iteration_itself_fails() {
    for (call, expected) in [
        ("Promise.all(5)", "TypeError"),
        ("Promise.allSettled(undefined)", "TypeError"),
        ("Promise.any(null)", "TypeError"),
        ("Promise.race({})", "TypeError"),
        (
            "Promise.all({ [Symbol.iterator]() { throw new EvalError('i') } })",
            "EvalError",
        ),
        (
            "Promise.all({ [Symbol.iterator]() { return { next() { throw new EvalError('n') } } } })",
            "EvalError",
        ),
        (
            "Promise.all({ [Symbol.iterator]() { return { next() { return { get done() { throw new EvalError('d') } } } } } })",
            "EvalError",
        ),
    ] {
        scenario(&format!(
            "{call}.then(() => check(false, 'resolved'), (error) => check(error.constructor.name === '{expected}', String(error)));"
        ));
    }
}

#[test]
fn a_throwing_resolve_or_then_rejects_the_promise_and_closes_the_iterator() {
    scenario(
        "let closed = 0;
         const iterable = { [Symbol.iterator]() { return {
           next() { return { done: false, value: 1 } }, return() { closed++; return {} } } } };
         class C extends Promise { static get [Symbol.species]() { return Promise } static resolve() { throw new EvalError('resolve') } }
         C.all(iterable).then(() => check(false, 'resolved'), (error) =>
           check(error instanceof EvalError && closed === 1, 'resolve threw ' + closed));",
    );
    scenario(
        "class C extends Promise { static get [Symbol.species]() { return Promise } static get resolve() { return 1 } }
         C.all([1]).then(() => check(false, 'resolved'), (error) => check(error instanceof TypeError, 'resolve not callable'));",
    );
    scenario(
        "class C extends Promise { static get [Symbol.species]() { return Promise } static resolve() { return { then() { throw new EvalError('then') } } } }
         C.allSettled([1]).then(() => check(false, 'resolved'), (error) => check(error instanceof EvalError, 'then threw'));",
    );
    scenario(
        "class C extends Promise { static get [Symbol.species]() { return Promise } static resolve() { return { then() { throw new EvalError('then') } } } }
         C.any([1]).then(() => check(false, 'resolved'), (error) => check(error instanceof EvalError, 'any then threw'));",
    );
    scenario(
        "class C extends Promise { static get [Symbol.species]() { return Promise } static resolve() { return { then() { throw new EvalError('then') } } } }
         C.race([1]).then(() => check(false, 'resolved'), (error) => check(error instanceof EvalError, 'race then threw'));",
    );
    // A constructor that is not a constructor at all cannot make a capability.
    scenario(
        "try { Promise.all.call(1, []); check(false, 'returned') } catch (e) { check(e instanceof TypeError, 'capability') }",
    );
    scenario(
        "try { Promise.all.call(function () { throw new EvalError('ctor') }, []); check(false, 'returned') }
         catch (e) { check(e instanceof EvalError, 'ctor threw') }",
    );
}

#[test]
fn element_functions_run_once_and_a_throwing_capability_resolve_rejects() {
    // The custom `resolve` hands back a thenable that calls each element
    // function twice: only the first call may count.
    scenario(
        "class C extends Promise { static get [Symbol.species]() { return Promise }
           static resolve(value) { return { then(onFulfilled, onRejected) { onFulfilled(value); onFulfilled('again') } } }
         }
         C.all([1, 2]).then((values) => check(same(values, [1, 2]), JSON.stringify(values)));",
    );
    scenario(
        "class C extends Promise { static get [Symbol.species]() { return Promise }
           static resolve(value) { return { then(onFulfilled, onRejected) { onFulfilled(value); onRejected('again') } } }
         }
         C.allSettled([1]).then((results) => check(same(results, [{ status: 'fulfilled', value: 1 }]), JSON.stringify(results)));",
    );
    scenario(
        "class C extends Promise { static get [Symbol.species]() { return Promise }
           static resolve(value) { return { then(onFulfilled, onRejected) { onRejected(value); onRejected('again') } } }
         }
         C.any([1]).then(() => check(false, 'resolved'), (error) => check(same(error.errors, [1]), JSON.stringify(error.errors)));",
    );
    // A capability whose resolving function throws: the conclusion of the
    // combinator fails and rejects through the other function.
    scenario(
        "class C extends Promise { static get [Symbol.species]() { return Promise }
           constructor(executor) {
             super((resolve, reject) => executor(() => { throw new EvalError('resolve') }, reject));
           }
         }
         C.all([]).then(() => check(false, 'resolved'), (error) => check(error instanceof EvalError, 'conclude'));",
    );
    scenario(
        "class C extends Promise { static get [Symbol.species]() { return Promise }
           constructor(executor) {
             super((resolve, reject) => executor(() => { throw new EvalError('resolve') }, reject));
           }
         }
         C.all([1]).then(() => check(false, 'resolved'), (error) => check(error instanceof EvalError, 'element conclude'));",
    );
}

#[test]
fn keyed_combinators_read_enumerable_own_keys_in_order() {
    scenario(
        "const sym = Symbol('s');
         const source = { a: 1, b: Promise.resolve(2), [sym]: Promise.resolve(3) };
         Object.defineProperty(source, 'hidden', { value: Promise.reject(9), enumerable: false });
         Promise.allKeyed(source).then((result) =>
           check(Object.getPrototypeOf(result) === null && result.a === 1 && result.b === 2
                 && result[sym] === 3 && !('hidden' in result), 'keyed'));",
    );
    scenario(
        "Promise.allSettledKeyed({ ok: 1, bad: Promise.reject(2) }).then((result) =>
           check(same(result.ok, { status: 'fulfilled', value: 1 }) && same(result.bad, { status: 'rejected', reason: 2 }), 'settled keyed'));",
    );
    scenario("Promise.allKeyed({}).then((result) => check(Object.keys(result).length === 0, 'empty keyed'));");
    scenario(
        "Promise.allKeyed({ a: Promise.reject(new Error('k')) }).then(() => check(false, 'resolved'),
           (error) => check(error.message === 'k', 'keyed reject'));",
    );
    // A getter that deletes a later key: that key is skipped.
    scenario(
        "const source = { get a() { delete this.b; return 1 }, b: 2 };
         Promise.allKeyed(source).then((result) => check(same(Object.keys(result), ['a']), Object.keys(result).join()));",
    );
    for source in ["1", "'text'", "null", "undefined"] {
        scenario(&format!(
            "Promise.allKeyed({source}).then(() => check(false, 'resolved'), (error) => check(error instanceof TypeError, 'not an object'));"
        ));
        scenario(&format!(
            "Promise.allSettledKeyed({source}).then(() => check(false, 'resolved'), (error) => check(error instanceof TypeError, 'not an object'));"
        ));
    }
    scenario(
        "Promise.allKeyed(new Proxy({}, { ownKeys() { throw new EvalError('keys') } })).then(() => check(false, 'resolved'),
           (error) => check(error instanceof EvalError, 'ownKeys'));",
    );
    scenario(
        "Promise.allKeyed(new Proxy({ a: 1 }, { getOwnPropertyDescriptor() { throw new EvalError('descriptor') } })).then(
           () => check(false, 'resolved'), (error) => check(error instanceof EvalError, 'descriptor'));",
    );
    scenario(
        "Promise.allKeyed({ get a() { throw new EvalError('get') } }).then(() => check(false, 'resolved'),
           (error) => check(error instanceof EvalError, 'getter'));",
    );
    scenario(
        "class C extends Promise { static get [Symbol.species]() { return Promise } static get resolve() { return 1 } }
         C.allKeyed({ a: 1 }).then(() => check(false, 'resolved'), (error) => check(error instanceof TypeError, 'resolve'));",
    );
    scenario(
        "class C extends Promise { static get [Symbol.species]() { return Promise } static resolve() { return { then() { throw new EvalError('then') } } } }
         C.allSettledKeyed({ a: 1 }).then(() => check(false, 'resolved'), (error) => check(error instanceof EvalError, 'then'));",
    );
    scenario(
        "class C extends Promise { static get [Symbol.species]() { return Promise }
           static resolve(value) { return { then(onFulfilled) { onFulfilled(value); onFulfilled('again') } } }
         }
         C.allKeyed({ a: 1, b: 2 }).then((result) => check(result.a === 1 && result.b === 2, 'twice'));",
    );
    scenario(
        "try { Promise.allKeyed.call(1, {}); check(false, 'returned') } catch (e) { check(e instanceof TypeError, 'capability') }",
    );
}

#[test]
fn running_out_of_fuel_inside_the_iteration_is_not_a_catchable_rejection() {
    let mut vm = Vm::new(VmConfig {
        instruction_budget: 20_000,
        ..VmConfig::default()
    })
    .unwrap();
    let code = compile(&parse("Promise.all((function* () { while (true) yield 1 })());").unwrap())
        .unwrap();
    assert_eq!(vm.execute(&code), Err(RuntimeError::InstructionLimit));
}

#[test]
fn promise_combinators_survive_every_allocation_failure() {
    let script = "
        Promise.all([1, Promise.resolve(2), { then(resolve) { resolve(3) } }]);
        Promise.allSettled([1, Promise.reject(2)]);
        Promise.any([Promise.reject(1), Promise.reject(2)]).catch(() => {});
        Promise.any([]).catch(() => {});
        Promise.race([Promise.resolve(1)]);
        Promise.allKeyed({ a: 1, b: Promise.resolve(2), [Symbol.for('k')]: 3 });
        Promise.allSettledKeyed({ a: 1, b: Promise.reject(2) });
        Promise.all([Promise.reject(1)]).catch(() => {});";
    assert!(heap_limit_sweep("Promise; AggregateError;", script) > 0);
}

#[test]
fn combinators_survive_running_out_of_instructions_at_every_step() {
    let script = "
        Promise.all([1, Promise.resolve(2)]);
        Promise.allSettled([1, 2]);
        Promise.any([Promise.reject(1), 2]);
        Promise.race([1]);
        Promise.allKeyed({ a: 1, b: Promise.resolve(2) });";
    assert!(instruction_budget_sweep(script) > 0);
}

/// A heap ceiling only fails an allocation that is the largest demand made so
/// far, so an allocation late in a combinator is reached only when the
/// operation is small enough that nothing before it is larger: each of these
/// is the smallest operation that makes the allocation.
#[test]
fn each_combinator_allocation_fails_cleanly_in_the_smallest_operation_that_makes_it() {
    let warm = "Promise; AggregateError; Promise.resolve; Symbol.for('k');";
    sweep_each(&[
        (warm, "Promise.all([])"),
        (warm, "Promise.all([1])"),
        (warm, "Promise.all(['x'.repeat(100)])"),
        (warm, "Promise.all([Promise.resolve(1), 2])"),
        (warm, "Promise.allSettled([])"),
        (warm, "Promise.allSettled([1])"),
        (warm, "Promise.allSettled([Promise.reject(1)])"),
        (warm, "Promise.any([])"),
        (warm, "Promise.any([1])"),
        (warm, "Promise.any([Promise.reject(1)]).catch(() => {})"),
        (warm, "Promise.race([1])"),
        (warm, "Promise.allKeyed({})"),
        (warm, "Promise.allKeyed({ a: 1 })"),
        (warm, "Promise.allKeyed({ [Symbol.for('k')]: 1 })"),
        (warm, "Promise.allSettledKeyed({})"),
        (warm, "Promise.allSettledKeyed({ a: 1 })"),
        (
            warm,
            "Promise.allKeyed({ a: Promise.reject(1) }).catch(() => {})",
        ),
    ]);
}

/// A heap ceiling fails an allocation only when nothing earlier in the run
/// demanded as much, so the allocations a combinator makes among a lot of
/// transient objects are reached only when those are kept out of the way:
/// an iterable whose iterator hands out results made beforehand (no result
/// object per step), and a constructor whose `resolve` returns a thenable
/// that keeps the element functions so that they can be called directly
/// (no job in between).
#[test]
fn combinator_state_and_element_functions_fail_cleanly_at_every_allocation() {
    let quiet_iterable = |value: &str| {
        format!(
            "globalThis.it = {{ i: 0, results: [{{ value: {value}, done: false }}, {{ value: undefined, done: true }}],
               [Symbol.iterator]() {{ return this }}, next() {{ return this.results[this.i++] }} }};
             Promise; Promise.resolve;"
        )
    };
    let direct = |call: &str| {
        format!(
            "globalThis.C = class {{ constructor(exec) {{ exec(() => {{}}, () => {{}}) }} static resolve(x) {{ return x }} }};
             globalThis.x = {{ then(f, r) {{ globalThis.f = f; globalThis.r = r }} }};
             Promise; {call};"
        )
    };
    let big = "'x'.repeat(2000)";
    let cases = [
        (quiet_iterable("1"), "Promise.all(it)".to_string()),
        (quiet_iterable("1"), "Promise.allSettled(it)".to_string()),
        (
            quiet_iterable("Promise.reject(1)"),
            "Promise.any(it).catch(() => {})".to_string(),
        ),
        (
            "Promise; Promise.resolve; globalThis.keyed = { a: 1, b: Promise.reject(2) };"
                .to_string(),
            "Promise.allSettledKeyed(keyed).catch(() => {})".to_string(),
        ),
        (direct("Promise.all.call(C, [x])"), format!("f({big})")),
        (
            direct("Promise.allSettled.call(C, [x])"),
            format!("f({big})"),
        ),
        (
            direct("Promise.allSettled.call(C, [x])"),
            format!("r({big})"),
        ),
        (direct("Promise.any.call(C, [x])"), "r(1)".to_string()),
    ];
    for (warm_up, operation) in &cases {
        heap_limit_sweep(warm_up, operation);
    }
}
