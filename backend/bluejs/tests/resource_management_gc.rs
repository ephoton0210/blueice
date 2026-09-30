// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Explicit Resource Management under a one-object nursery (a collection on
//! nearly every allocation). A dispose capability's resources leave the
//! `DisposableStack` side table when disposal starts, so every value they
//! hold, every intermediate array and every pending error must stay rooted
//! across the allocations and calls that follow.
use blueice_bluejs::{compile, parse, Vm, VmConfig};

fn run_gc_stress(script: &str) {
    let mut config = VmConfig::default();
    config.heap.nursery_capacity = 1;
    let mut vm = Vm::new(config).unwrap();
    vm.install_test262_done().unwrap();
    let source = format!(
        "function finish(ok) {{ if (ok) $DONE(); else $DONE(new Error('check failed')); }}{script}"
    );
    vm.execute_script(&compile(&parse(&source).unwrap()).unwrap())
        .unwrap();
    vm.run_promise_jobs().unwrap();
    vm.take_test262_done()
        .expect("the script never reported through $DONE")
        .unwrap_or_else(|error| panic!("{script}\n=> {error:?}"));
}

#[test]
fn dispose_async_of_an_empty_stack_survives_collection() {
    run_gc_stress(
        "var stack = new AsyncDisposableStack();\
         var p = stack.disposeAsync();\
         var threw = false;\
         try { stack.defer(async _ => {}); } catch (e) { threw = e instanceof ReferenceError; }\
         p.then(v => finish(v === undefined && threw && stack.disposed), () => finish(false));",
    );
}

#[test]
fn dispose_async_awaits_resources_in_reverse_order_under_gc_stress() {
    run_gc_stress(
        "var log = [];\
         var stack = new AsyncDisposableStack();\
         stack.defer(async () => { log.push('a'); });\
         stack.adopt({ tag: 'b' }, async v => { log.push(v.tag); });\
         stack.use({ [Symbol.asyncDispose]() { log.push('c'); return Promise.resolve(); } });\
         stack.use({ [Symbol.dispose]() { log.push('d'); } });\
         stack.use(null);\
         stack.disposeAsync().then(\
           v => finish(v === undefined && log.join() === 'd,c,b,a'), () => finish(false));",
    );
}

#[test]
fn dispose_async_builds_a_suppressed_error_chain_under_gc_stress() {
    run_gc_stress(
        "var e1 = new Error('1'), e2 = new Error('2'), e3 = new Error('3');\
         var stack = new AsyncDisposableStack();\
         stack.defer(async () => { throw e1; });\
         stack.defer(() => { throw e2; });\
         stack.defer(async () => { throw e3; });\
         stack.disposeAsync().then(() => finish(false), e => finish(\
           e instanceof SuppressedError && e.error === e1\
           && e.suppressed instanceof SuppressedError\
           && e.suppressed.error === e2 && e.suppressed.suppressed === e3));",
    );
}

#[test]
fn moved_async_stack_keeps_its_resources_under_gc_stress() {
    run_gc_stress(
        "var log = [];\
         var stack = new AsyncDisposableStack();\
         stack.defer(async () => { log.push('x'); });\
         stack.adopt({ tag: 'y' }, async v => { log.push(v.tag); });\
         var moved = stack.move();\
         var before = stack.disposed && !moved.disposed;\
         moved.disposeAsync().then(\
           () => finish(before && moved.disposed && log.join() === 'y,x'), () => finish(false));",
    );
}

#[test]
fn sync_dispose_builds_a_suppressed_error_chain_under_gc_stress() {
    run_gc_stress(
        "var e1 = new Error('1'), e2 = new Error('2'), e3 = new Error('3');\
         var stack = new DisposableStack();\
         stack.defer(function () { throw e1; });\
         stack.defer(function () { throw e2; });\
         stack.defer(function () { throw e3; });\
         try { stack.dispose(); finish(false); } catch (e) {\
           finish(e instanceof SuppressedError && e.error === e1\
             && e.suppressed instanceof SuppressedError\
             && e.suppressed.error === e2 && e.suppressed.suppressed === e3);\
         }",
    );
}

#[test]
fn sync_dispose_runs_every_resource_in_reverse_order_under_gc_stress() {
    run_gc_stress(
        "var log = [];\
         var stack = new DisposableStack();\
         stack.defer(function () { log.push('a'); });\
         stack.adopt({ tag: 'b' }, function (v) { log.push(v.tag); });\
         stack.use({ [Symbol.dispose]() { log.push('c'); } });\
         stack.move().dispose();\
         finish(log.join() === 'c,b,a');",
    );
}

#[test]
fn using_declarations_throw_suppressed_errors_under_gc_stress() {
    run_gc_stress(
        "var e1 = new Error('1'), e2 = new Error('2'), e3 = new Error('3');\
         try {\
           { using a = { [Symbol.dispose]() { throw e1; } };\
             using b = { [Symbol.dispose]() { throw e2; } };\
             using c = { [Symbol.dispose]() { throw e3; } }; }\
           finish(false);\
         } catch (e) {\
           finish(e instanceof SuppressedError && e.error === e1\
             && e.suppressed instanceof SuppressedError\
             && e.suppressed.error === e2 && e.suppressed.suppressed === e3);\
         }",
    );
}

#[test]
fn using_declaration_error_alone_and_with_a_body_error_under_gc_stress() {
    run_gc_stress(
        "var e1 = new Error('1'), e2 = new Error('2'), body = new Error('body');\
         try { { using a = { [Symbol.dispose]() { throw e1; } }; } finish(false); }\
         catch (e) {\
           try { { using a = { [Symbol.dispose]() { throw e2; } }; throw body; } finish(false); }\
           catch (f) {\
             finish(e === e1 && f instanceof SuppressedError && f.error === e2 && f.suppressed === body);\
           }\
         }",
    );
}

#[test]
fn await_using_disposes_resources_in_order_under_gc_stress() {
    run_gc_stress(
        "var log = [];\
         (async () => {\
           try {\
             await using a = { async [Symbol.asyncDispose]() { log.push('a'); throw new RangeError('a'); } };\
             await using b = { [Symbol.dispose]() { log.push('b'); } };\
             await using c = null;\
             await using d = { async [Symbol.asyncDispose]() { log.push('d'); throw new TypeError('d'); } };\
             throw new EvalError('body');\
           } catch (e) {\
             finish(log.join() === 'd,b,a' && e instanceof SuppressedError\
               && e.error instanceof RangeError\
               && e.suppressed instanceof SuppressedError\
               && e.suppressed.error instanceof TypeError\
               && e.suppressed.suppressed instanceof EvalError);\
           }\
         })();",
    );
}

#[test]
fn stack_use_and_adopt_keep_their_values_reachable_under_gc_stress() {
    run_gc_stress(
        "var log = [];\
         var stack = new AsyncDisposableStack();\
         var resource = { [Symbol.asyncDispose]() { log.push('use'); } };\
         for (var i = 0; i < 20; i++) { stack.use(i % 2 ? resource : null); }\
         stack.adopt({ tag: 'adopted' }, v => { log.push(v.tag); });\
         var junk = []; for (var i = 0; i < 50; i++) junk.push({ i });\
         stack.disposeAsync().then(\
           () => finish(log.length === 11 && log[0] === 'adopted' && log[1] === 'use'), () => finish(false));",
    );
}

#[test]
fn allocating_dispose_getter_keeps_the_new_resource_alive_under_gc_stress() {
    run_gc_stress(
        "var log = [];\
         var junk = [];\
         function make(tag) {\
           return { get [Symbol.dispose]() {\
             for (var i = 0; i < 30; i++) junk.push({ i });\
             return function () { log.push(tag); }; } };\
         }\
         { using a = make('a'); using b = make('b'); }\
         var stack = new DisposableStack();\
         stack.use(make('c'));\
         stack.dispose();\
         finish(log.join() === 'b,a,c');",
    );
}

#[test]
fn native_error_from_the_body_is_suppressed_by_a_disposal_error_under_gc_stress() {
    run_gc_stress(
        "var e = new Error('dispose');\
         try {\
           { using a = { [Symbol.dispose]() { throw e; } }; null.property; }\
           finish(false);\
         } catch (f) {\
           finish(f instanceof SuppressedError && f.error === e && f.suppressed instanceof TypeError);\
         }",
    );
}

#[test]
fn dispose_async_of_a_rejecting_native_error_under_gc_stress() {
    run_gc_stress(
        "var stack = new AsyncDisposableStack();\
         stack.defer(() => { null.property; });\
         stack.defer(() => { throw new RangeError('second'); });\
         stack.disposeAsync().then(() => finish(false), e => finish(\
           e instanceof SuppressedError && e.error instanceof TypeError\
           && e.suppressed instanceof RangeError));",
    );
}
