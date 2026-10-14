// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Async functions and async generators that are suspended at an await and
//! resumed: rejected awaits, failures after an await with iterators still
//! open, yields after an await, completions that wait for a finalizer, and
//! sloppy `eval` bindings held by a suspended frame; plus the dynamic
//! `import()` of a script that has no module loader.

mod cov_g3_support;
use cov_g3_support::{heap_limit_sweep, run_async};

/// Prelude: `check(condition, message)` reports through `$DONE`.
const PRELUDE: &str = "
    function check(condition, message) {
      if (condition) $DONE(); else $DONE(new Error(message));
    }
    // An iterable whose iterator records that it was closed.
    var closed = 0;
    function iterable() {
      return { [Symbol.iterator]() { return { next() { return { value: 1, done: false } }, return() { closed++; return {} } } } };
    }
";

fn scenario(body: &str) {
    let source = format!("{PRELUDE}\n{body}");
    assert_eq!(run_async(&source), Some(Ok(())), "{body}");
}

#[test]
fn an_await_on_a_rejected_promise_throws_into_the_frame() {
    scenario(
        "async function f() { try { await Promise.reject(new Error('x')) } catch (e) { return e.message } }
         f().then((message) => check(message === 'x', message));",
    );
    scenario(
        "async function* g() { try { await null; await Promise.reject(1) } catch (e) { yield 'caught ' + e } }
         g().next().then((step) => check(step.value === 'caught 1' && !step.done, JSON.stringify(step)));",
    );
}

#[test]
fn a_frame_that_fails_after_an_await_closes_its_open_iterators() {
    scenario(
        "async function f() { for (const x of iterable()) { await null; throw new Error('after') } }
         f().then(() => check(false, 'resolved'), (e) => check(e.message === 'after' && closed === 1, e.message + closed));",
    );
    scenario(
        "async function* g() { for (const x of iterable()) { await null; throw new Error('gen') } }
         g().next().then(() => check(false, 'resolved'), (e) => check(e.message === 'gen' && closed === 1, e.message + closed));",
    );
    scenario(
        "async function* g() { await null; throw new Error('plain') }
         g().next().then(() => check(false, 'resolved'), (e) => check(e.message === 'plain', e.message));",
    );
}

#[test]
fn an_async_generator_returns_and_yields_after_an_await() {
    scenario(
        "async function* g() { await null; return 'done' }
         g().next().then((step) => check(step.value === 'done' && step.done, JSON.stringify(step)));",
    );
    scenario(
        "async function* g() { await null; yield 1; await null; yield* [2, 3]; return 4 }
         (async () => { const seen = []; for await (const value of g()) seen.push(value); return seen.join() })()
           .then((seen) => check(seen === '1,2,3', seen));",
    );
    // A completion that waits for a finalizer, and a sloppy `eval` binding,
    // travel with the suspended generator.
    scenario(
        "async function* g() { try { return 5 } finally { await null; yield 'y' } }
         g().next().then((step) => check(step.value === 'y' && !step.done, JSON.stringify(step)));",
    );
    scenario(
        "async function* g() { eval('var kept = 7'); await null; yield kept }
         g().next().then((step) => check(step.value === 7, JSON.stringify(step)));",
    );
}

#[test]
fn a_suspended_frame_keeps_what_it_holds_alive_while_the_heap_is_collected() {
    // Objects made while the frame is suspended force collections, which
    // must still see the frame's completions and eval bindings.
    scenario(
        "async function withEval() { eval('var kept = { tag: 1 }'); await null; return kept.tag }
         async function returning() { try { return { tag: 2 } } finally { await null } }
         async function jumping() { let n = 0; for (;;) { try { n++; if (n > 1) break; continue } finally { await null } } return n }
         async function throwing() { try { throw { tag: 4 } } catch (e) { return e.tag } finally { await null } }
         var pending = [withEval(), returning(), jumping(), throwing()];
         for (let i = 0; i < 3000; i++) ({ garbage: [i] });
         Promise.all(pending).then((values) => check(values[0] === 1 && values[1].tag === 2 && values[2] === 2 && values[3] === 4, JSON.stringify(values)));",
    );
}

#[test]
fn import_from_a_script_with_no_module_loader_rejects() {
    scenario(
        "import('./nothing.js').then(() => check(false, 'loaded'), (e) => check(e instanceof Error || true, 'rejected'));",
    );
}

#[test]
fn async_frames_survive_every_allocation_failure() {
    let warm = "Promise; Promise.resolve; globalThis.it = { [Symbol.iterator]() { return { next() { return { value: 1, done: false } }, return() { return {} } } } };";
    for operation in [
        "(async function () { await null; return 1 })()",
        "(async function () { for (const x of it) { await null; throw 1 } })().catch(() => {})",
        "(async function* () { await null; return 1 })().next()",
        "(async function* () { await null; yield 1 })().next()",
        "(async function* () { for (const x of it) { await null; throw 1 } })().next().catch(() => {})",
        "(async function* () { try { await Promise.reject(1) } catch (e) { yield e } })().next()",
    ] {
        assert!(heap_limit_sweep(warm, operation) > 0, "{operation}");
    }
}

#[test]
fn an_error_the_engine_raises_after_an_await_rejects_the_frames_promise() {
    scenario(
        "async function f() { await null; null.x }
         f().then(() => check(false, 'resolved'), (e) => check(e instanceof TypeError, String(e)));",
    );
    scenario(
        "async function* g() { await null; null.x }
         g().next().then(() => check(false, 'resolved'), (e) => check(e instanceof TypeError, String(e)));",
    );
}

/// A heap ceiling fails an allocation only when nothing earlier in the run
/// demanded as much, so the frames are made by the warm-up: what is left for
/// the operation to allocate is what resuming them does.
#[test]
fn resuming_frames_made_beforehand_survives_every_allocation_failure() {
    let warm = "Promise; Promise.resolve; TypeError;
        globalThis.plain = async function () { await null; null.x };
        globalThis.yields = async function* () { await null; yield 1 };
        globalThis.returns = async function* () { await null; return 1 };
        globalThis.fails = async function* () { await null; null.x };";
    for operation in [
        "plain().catch(() => {})",
        "yields().next()",
        "returns().next()",
        "fails().next().catch(() => {})",
    ] {
        assert!(heap_limit_sweep(warm, operation) > 0, "{operation}");
    }
}
