// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Explicit Resource Management: `DisposableStack`/`AsyncDisposableStack`
//! receiver and state checks, disposal ordering and `SuppressedError`
//! folding, `using`/`await using` declarations, and resource-exhaustion
//! sweeps through the same paths.

mod cov_g7_common;
use blueice_bluejs::RuntimeError;
use cov_g7_common::{
    failures, run_with, sweep_fuel_each, sweep_heap_bare, sweep_heap_each, sweep_heap_runs,
    with_setup,
};

const PRELUDE: &str = r#"
var boom = {};
var other = {};
function throwsBoom(f) { try { f(); return false } catch (e) { return e === boom } }
"#;

fn run(body: &str) -> String {
    failures(&format!("{PRELUDE}{body}"))
}

#[test]
fn disposable_stack_state_and_receiver_checks() {
    assert_eq!(
        run(r#"
th('call without new', function () { DisposableStack() }, TypeError);
th('async call without new', function () { AsyncDisposableStack() }, TypeError);
var stack = new DisposableStack();
eq('fresh', stack.disposed, false);
var log = [];
var resource = { [Symbol.dispose]() { log.push('r') } };
eq('use returns value', stack.use(resource), resource);
eq('use null', stack.use(null), null);
eq('use undefined', stack.use(undefined), undefined);
th('use primitive', function () { stack.use(1) }, TypeError);
th('use string', function () { stack.use('s') }, TypeError);
th('use without method', function () { stack.use({}) }, TypeError);
th('use non-callable method', function () { stack.use({ [Symbol.dispose]: 1 }) }, TypeError);
eq('adopt returns value', stack.adopt(5, function (v) { log.push('adopt:' + v) }), 5);
th('adopt non-callable', function () { stack.adopt(1, 2) }, TypeError);
eq('defer returns undefined', stack.defer(function () { log.push('defer') }), undefined);
th('defer non-callable', function () { stack.defer(1) }, TypeError);
var moved = stack.move();
eq('move disposes source', stack.disposed, true);
eq('moved fresh', moved.disposed, false);
eq('moved instance', moved instanceof DisposableStack, true);
eq('dispose moved', moved.dispose(), undefined);
eq('order', log.join(), 'defer,adopt:5,r');
eq('moved disposed', moved.disposed, true);
eq('dispose twice', moved.dispose(), undefined);
eq('dispose disposed source', stack.dispose(), undefined);
th('use after dispose', function () { stack.use(resource) }, ReferenceError);
th('adopt after dispose', function () { stack.adopt(1, function () {}) }, ReferenceError);
th('defer after dispose', function () { stack.defer(function () {}) }, ReferenceError);
th('move after dispose', function () { stack.move() }, ReferenceError);
var P = DisposableStack.prototype;
var asyncStack = new AsyncDisposableStack();
['use', 'adopt', 'defer', 'move', 'dispose'].forEach(function (name) {
  th(name + ' wrong receiver', function () { P[name].call({}, function () {}, function () {}) }, TypeError);
  th(name + ' primitive receiver', function () { P[name].call(1, function () {}, function () {}) }, TypeError);
  th(name + ' async receiver', function () { P[name].call(asyncStack, function () {}, function () {}) }, TypeError);
});
th('disposed getter receiver', function () { Object.getOwnPropertyDescriptor(P, 'disposed').get.call({}) }, TypeError);
th('disposed getter primitive', function () { Object.getOwnPropertyDescriptor(P, 'disposed').get.call(1) }, TypeError);
th('async disposed getter receiver', function () { Object.getOwnPropertyDescriptor(AsyncDisposableStack.prototype, 'disposed').get.call(new DisposableStack()) }, TypeError);
eq('toStringTag', P[Symbol.toStringTag] + ':' + AsyncDisposableStack.prototype[Symbol.toStringTag], 'DisposableStack:AsyncDisposableStack');
eq('dispose alias', P[Symbol.dispose], P.dispose);
eq('async dispose alias', AsyncDisposableStack.prototype[Symbol.asyncDispose], AsyncDisposableStack.prototype.disposeAsync);
class Sub extends DisposableStack {}
var sub = new Sub();
eq('subclass', sub instanceof Sub && sub instanceof DisposableStack, true);
eq('subclass move is a DisposableStack', Object.getPrototypeOf(sub.move()) === DisposableStack.prototype, true);
"#),
        ""
    );
}

#[test]
fn dispose_method_lookups_and_constructor_prototypes_report_their_errors() {
    assert_eq!(
        run(r#"
function throwsGetter(symbol) {
  var o = {};
  Object.defineProperty(o, symbol, { get: function () { throw boom } });
  return o;
}
eq('sync lookup throws', throwsBoom(function () { { using x = throwsGetter(Symbol.dispose) } }), true);
eq('stack use lookup throws', throwsBoom(function () { new DisposableStack().use(throwsGetter(Symbol.dispose)) }), true);
eq('async stack use lookup throws', throwsBoom(function () { new AsyncDisposableStack().use(throwsGetter(Symbol.asyncDispose)) }), true);
eq('async fallback lookup throws', throwsBoom(function () { new AsyncDisposableStack().use(throwsGetter(Symbol.dispose)) }), true);
var NewTarget = function () {}.bind();
Object.setPrototypeOf(NewTarget, { get prototype() { throw boom } });
eq('sync constructor prototype throws', throwsBoom(function () { Reflect.construct(DisposableStack, [], NewTarget) }), true);
eq('async constructor prototype throws', throwsBoom(function () { Reflect.construct(AsyncDisposableStack, [], NewTarget) }), true);
async function awaitUsing(resource) { { await using x = resource } }
afterRejected('await using async lookup throws', awaitUsing(throwsGetter(Symbol.asyncDispose)), function (e) { eq('async lookup', e, boom) });
afterRejected('await using fallback lookup throws', awaitUsing(throwsGetter(Symbol.dispose)), function (e) { eq('fallback lookup', e, boom) });
"#),
        ""
    );
}

#[test]
fn disposal_errors_fold_into_suppressed_errors() {
    assert_eq!(
        run(r#"
var first = new DisposableStack();
first.defer(function () { throw boom });
eq('single error rethrown', (function () { try { first.dispose() } catch (e) { return e } })(), boom);
var second = new DisposableStack();
second.defer(function () { throw boom });
second.defer(function () { throw other });
var folded = (function () { try { second.dispose() } catch (e) { return e } })();
eq('suppressed type', folded instanceof SuppressedError, true);
eq('suppressed error', folded.error, boom);
eq('suppressed suppressed', folded.suppressed, other);
var third = new DisposableStack();
var order = [];
third.defer(function () { order.push('a'); throw 'a' });
third.defer(function () { order.push('b') });
third.defer(function () { order.push('c'); throw 'c' });
var chain = (function () { try { third.dispose() } catch (e) { return e } })();
eq('all disposed', order.join(), 'c,b,a');
eq('chain', chain.error + ':' + chain.suppressed, 'a:c');
var adopted = new DisposableStack();
adopted.adopt('value', function (v) { throw v });
eq('adopt argument', (function () { try { adopted.dispose() } catch (e) { return e } })(), 'value');
var withUsing = (function () {
  try {
    {
      using a = { [Symbol.dispose]() { throw 'from a' } };
      using b = { [Symbol.dispose]() { throw 'from b' } };
    }
  } catch (e) { return e }
})();
eq('using folds', withUsing.error + ':' + withUsing.suppressed, 'from a:from b');
var bodyAndDispose = (function () {
  try {
    {
      using a = { [Symbol.dispose]() { throw 'dispose' } };
      throw 'body';
    }
  } catch (e) { return e }
})();
eq('body error suppressed', bodyAndDispose.error + ':' + bodyAndDispose.suppressed, 'dispose:body');
var log = [];
{
  using a = { [Symbol.dispose]() { log.push('a') } };
  using none = null;
  using missing = undefined;
  using b = { [Symbol.dispose]() { log.push('b') } };
}
eq('using order', log.join(), 'b,a');
th('using primitive', function () { { using x = 1 } }, TypeError);
th('using object without method', function () { { using x = {} } }, TypeError);
var normalThenThrow = (function () {
  try { { using a = { [Symbol.dispose]() { throw 'only' } } } } catch (e) { return e }
})();
eq('using single error', normalThenThrow, 'only');
"#),
        ""
    );
}

#[test]
fn async_disposable_stack_settles_promises_in_reverse_order() {
    assert_eq!(
        run(r#"
var log = [];
var stack = new AsyncDisposableStack();
eq('async use returns', stack.use({ [Symbol.asyncDispose]() { log.push('async'); return Promise.resolve() } }) !== undefined, true);
stack.use({ [Symbol.dispose]() { log.push('sync fallback'); return Promise.reject(boom) } });
stack.use(null);
stack.use(undefined);
th('async use primitive', function () { stack.use(1) }, TypeError);
th('async use no method', function () { stack.use({}) }, TypeError);
stack.adopt('arg', function (v) { log.push('adopt:' + v) });
stack.defer(function () { log.push('defer') });
th('async adopt non-callable', function () { stack.adopt(1, 2) }, TypeError);
th('async defer non-callable', function () { stack.defer(1) }, TypeError);
var moved = stack.move();
eq('async move', stack.disposed + ':' + moved.disposed, 'true:false');
after('dispose async', moved.disposeAsync(), function (value) {
  eq('resolved value', value, undefined);
  eq('async order', log.join(), 'defer,adopt:arg,sync fallback,async');
  eq('async disposed', moved.disposed, true);
});
after('dispose async twice', moved.disposeAsync(), function (value) { eq('twice value', value, undefined) });
after('dispose empty', new AsyncDisposableStack().disposeAsync(), function (value) { eq('empty value', value, undefined) });
afterRejected('dispose invalid receiver', AsyncDisposableStack.prototype.disposeAsync.call({}), function (e) { eq('invalid receiver', e instanceof TypeError, true) });
afterRejected('dispose primitive receiver', AsyncDisposableStack.prototype.disposeAsync.call(1), function (e) { eq('primitive receiver', e instanceof TypeError, true) });
afterRejected('dispose sync stack receiver', AsyncDisposableStack.prototype.disposeAsync.call(new DisposableStack()), function (e) { eq('sync receiver', e instanceof TypeError, true) });
th('use after async dispose', function () { moved.use({ [Symbol.asyncDispose]() {} }) }, ReferenceError);
var failing = new AsyncDisposableStack();
failing.defer(function () { return Promise.reject(boom) });
failing.defer(function () { throw other });
afterRejected('async failures fold', failing.disposeAsync(), function (e) {
  eq('folded type', e instanceof SuppressedError, true);
  eq('folded error', e.error, boom);
  eq('folded suppressed', e.suppressed, other);
});
var single = new AsyncDisposableStack();
single.defer(function () { throw boom });
afterRejected('async single failure', single.disposeAsync(), function (e) { eq('single', e, boom) });
var awaited = [];
var timing = new AsyncDisposableStack();
timing.defer(function () { awaited.push('second-start'); return { then: function (resolve) { awaited.push('thenable'); resolve() } } });
timing.use({ [Symbol.asyncDispose]() { awaited.push('first-start') } });
after('thenable results are awaited', timing.disposeAsync(), function () { eq('timing', awaited.join(), 'first-start,second-start,thenable') });
"#),
        ""
    );
}

#[test]
fn await_using_declarations_dispose_asynchronously() {
    assert_eq!(
        run(r#"
var log = [];
async function run() {
  {
    await using a = { [Symbol.asyncDispose]() { log.push('a'); return Promise.resolve() } };
    await using none = null;
    await using missing = undefined;
    await using sync = { [Symbol.dispose]() { log.push('sync') } };
    await using b = { [Symbol.asyncDispose]() { log.push('b') } };
  }
  return log.join();
}
after('await using order', run(), function (value) { eq('await using order value', value, 'b,sync,a') });
async function failing() {
  {
    await using a = { [Symbol.asyncDispose]() { throw 'a' } };
    await using b = { [Symbol.asyncDispose]() { return Promise.reject('b') } };
  }
}
afterRejected('await using folds', failing(), function (e) { eq('await using folded', e.error + ':' + e.suppressed, 'a:b') });
async function bodyThrows() {
  {
    await using a = { [Symbol.asyncDispose]() { throw 'dispose' } };
    throw 'body';
  }
}
afterRejected('await using body error', bodyThrows(), function (e) { eq('body suppressed', e.error + ':' + e.suppressed, 'dispose:body') });
async function bodyThrowsCleanly() {
  {
    await using a = { [Symbol.asyncDispose]() {} };
    throw 'body only';
  }
}
afterRejected('await using body error only', bodyThrowsCleanly(), function (e) { eq('body only', e, 'body only') });
async function bad() { { await using x = 1 } }
afterRejected('await using primitive', bad(), function (e) { eq('primitive', e instanceof TypeError, true) });
async function noMethod() { { await using x = {} } }
afterRejected('await using no method', noMethod(), function (e) { eq('no method', e instanceof TypeError, true) });
async function loopWithBreak() {
  var n = 0;
  for (var i = 0; i < 3; i++) {
    await using r = { [Symbol.asyncDispose]() { n++ } };
    if (i === 1) break;
  }
  return n;
}
after('await using break', loopWithBreak(), function (n) { eq('break count', n, 2) });
"#),
        ""
    );
}

const SETUP: &str = "var log = [];";

const BODIES: &[&str] = &[
    "var s = new DisposableStack(); s.use({ [Symbol.dispose]() { log.push('u') } }); s.adopt(1, function () {}); s.defer(function () {});",
    "var s = new DisposableStack(); s.defer(function () { throw 'x' }); s.defer(function () { throw 'y' }); try { s.move().dispose() } catch (e) {}",
    "var a = new AsyncDisposableStack(); a.use({ [Symbol.asyncDispose]() { return Promise.resolve() } }); a.use({ [Symbol.dispose]() {} }); a.use(null); a.adopt(1, function () {});",
    "var a = new AsyncDisposableStack(); a.defer(function () { throw 'z' }); a.move().disposeAsync().then(function () {}, function () {});",
    "new AsyncDisposableStack().disposeAsync(); AsyncDisposableStack.prototype.disposeAsync.call({}).then(function () {}, function () {});",
    "try { { using p = { [Symbol.dispose]() { throw 'p' } }; using q = { [Symbol.dispose]() { throw 'q' } }; } } catch (e) {}",
    "async function g() { try { await using x = { [Symbol.asyncDispose]() { throw 'a' } }; await using n = null; throw 'body'; } catch (e) {} } g();",
    "try { { using a = { [Symbol.dispose]() { null.x } }; using b = { [Symbol.dispose]() { null.y } }; } } catch (e) {}",
    "async function h() { try { await using a = { [Symbol.asyncDispose]() { null.x } }; null.z } catch (e) {} } h();",
    "var s = new DisposableStack(); s.defer(function () { null.x }); s.defer(function () { null.y }); try { s.dispose() } catch (e) {}",
    "var s = new AsyncDisposableStack(); s.disposeAsync(); s.disposeAsync();",
];

#[test]
fn every_heap_allocation_failure_reports_the_heap_limit() {
    // Prototype creation on first use, then each operation on warm prototypes.
    sweep_heap_runs(
        "",
        "new DisposableStack(); new AsyncDisposableStack();",
        8000,
    );
    // The prototypes as the very first thing a bare VM builds.
    sweep_heap_bare("new DisposableStack()", 4000);
    sweep_heap_bare("new AsyncDisposableStack()", 4000);
    sweep_heap_each(&with_setup(SETUP, BODIES));
    // With every result kept alive, each allocation has its own failure window.
    sweep_heap_each(&[(
        "var keep = [];",
        "var s = new AsyncDisposableStack(); keep.push(s.disposeAsync()); keep.push(s.disposeAsync());",
    )]);
}

#[test]
fn every_instruction_budget_exhaustion_reports_the_instruction_limit() {
    sweep_fuel_each(&with_setup(SETUP, BODIES));
}

#[test]
fn a_dispose_method_that_runs_out_of_fuel_ends_disposal_with_that_error() {
    for body in [
        "{ using a = { [Symbol.dispose]() { for (;;) {} } }; }",
        "{ using a = { [Symbol.dispose]() { for (;;) {} } }; throw 1 }",
        "var s = new DisposableStack(); s.defer(function () { throw 1 }); s.defer(function () { for (;;) {} }); s.dispose();",
    ] {
        let result = run_with(body, false, &|config| config.instruction_budget = 20_000);
        assert_eq!(result, Err(RuntimeError::InstructionLimit), "{body}");
    }
}
