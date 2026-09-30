// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Async iteration over synchronous iterables under a one-object nursery
//! (a collection on nearly every allocation): AsyncFromSyncIteratorContinuation
//! builds two handlers and a derived promise per step, and a rejection is
//! built from a freshly allocated error, so each must stay rooted across the
//! allocation that follows it.
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
fn for_await_over_a_sync_iterable_survives_collection() {
    run_gc_stress(
        "(async () => { var seen = 0; for await (var x of [1, 2, 3]) seen += x; finish(seen === 6); })();",
    );
    run_gc_stress("(async () => { for await (var x of []) finish(false); finish(true); })();");
}

#[test]
fn for_await_rejects_with_a_sync_iterators_thrown_error_under_gc_stress() {
    run_gc_stress(
        "(async () => { try { for await (var x of (function* () { throw new RangeError('gen'); })()); }\
           catch (e) { finish(e instanceof RangeError && e.message === 'gen'); return; } finish(false); })();",
    );
}

#[test]
fn promise_reject_keeps_a_fresh_error_alive() {
    run_gc_stress(
        "Promise.reject(new RangeError('fresh')).then(() => finish(false), e => finish(e.message === 'fresh'));",
    );
}
