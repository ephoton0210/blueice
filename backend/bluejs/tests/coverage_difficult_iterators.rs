// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Async collection and multi-iterator abrupt completion contracts.

use blueice_bluejs::{compile, parse, Value, Vm, VmConfig};

fn run(source: &str, asynchronous: bool) {
    for nursery_capacity in [1, VmConfig::default().heap.nursery_capacity] {
        let mut config = VmConfig::default();
        config.heap.nursery_capacity = nursery_capacity;
        let mut vm = Vm::new(config).unwrap();
        let source = if asynchronous {
            vm.install_test262_done().unwrap();
            format!(
                "function check(value) {{ if (!value) throw new Error('assertion failed'); }}
                 (async () => {{ {source} }})().then(() => $DONE(), error => $DONE(error));"
            )
        } else {
            source.to_string()
        };
        let result = vm
            .execute_script(&compile(&parse(&source).unwrap()).unwrap())
            .unwrap();
        if asynchronous {
            vm.run_promise_jobs().unwrap();
            assert_eq!(vm.take_test262_done(), Some(Ok(())), "{source}");
        } else {
            assert_eq!(result, Value::Bool(true), "{source}");
        }
    }
}

#[test]
fn from_async_next_failures_mark_done_without_closing() {
    run(
        r#"
        for (var mode of ['throw', 'reject', 'done', 'value']) {
            var sentinel = {}, closed = 0;
            var source = {[Symbol.asyncIterator]() { return this; },
                next() {
                    if (mode === 'throw') throw sentinel;
                    if (mode === 'reject') return Promise.reject(sentinel);
                    return mode === 'done' ? {get done() {throw sentinel;}} :
                        {done:false, get value() {throw sentinel;}};
                },
                return() { closed++; return {}; }
            };
            var same = false;
            try { await Array.fromAsync(source); } catch (e) { same = e === sentinel; }
            check(same && closed === 0);
        }
    "#,
        true,
    );
}

#[test]
fn from_async_mapper_failure_awaits_close_and_keeps_the_original_error() {
    run(
        r#"
        for (var closeKind of ['absent', 'getter', 'throw', 'reject', 'resolve']) {
            var sentinel = {}, other = {}, log = [];
            var source = {[Symbol.asyncIterator]() { return this; },
                next() { return {done:false, value:1}; }
            };
            if (closeKind === 'getter') Object.defineProperty(source, 'return', {
                get() { log.push('close'); throw other; }
            });
            else if (closeKind !== 'absent') source.return = function() {
                log.push('close');
                if (closeKind === 'throw') throw other;
                return closeKind === 'reject' ? Promise.reject(other) :
                    Promise.resolve().then(() => {log.push('awaited'); return {};});
            };
            var same = false;
            try { await Array.fromAsync(source, () => Promise.reject(sentinel)); }
            catch (e) { same = e === sentinel; log.push('caught'); }
            check(same);
            check(log.join() === (closeKind === 'absent' ? 'caught' :
                closeKind === 'resolve' ? 'close,awaited,caught' : 'close,caught'));
        }
    "#,
        true,
    );
}

#[test]
fn from_async_array_like_rejects_get_map_and_definition_failures() {
    run(
        r#"
        var sentinel = {}, passed = 0;
        var operations = [
            () => Array.fromAsync({length:1, get 0() {throw sentinel;}}),
            () => Array.fromAsync({length:1, 0:Promise.reject(sentinel)}),
            () => Array.fromAsync({length:1, 0:1}, () => {throw sentinel;}),
            () => Array.fromAsync({length:1, 0:1}, () => Promise.reject(sentinel))
        ];
        for (var operation of operations) {
            try { await operation(); } catch (e) { if (e === sentinel) passed++; }
        }
        function Frozen() { return Object.preventExtensions({}); }
        var rejected = false;
        try { await Array.fromAsync.call(Frozen, {length:1, 0:7}); }
        catch (e) { rejected = e instanceof TypeError; }
        check(passed === operations.length && rejected);
        check((await Array.fromAsync({length:1, 0:7}))[0] === 7);
    "#,
        true,
    );
}

#[test]
fn async_dispose_ignores_fulfillment_values_and_preserves_rejection_identity() {
    run(
        r#"
        var prototype = Object.getPrototypeOf(Object.getPrototypeOf((async function*() {}).prototype));
        var dispose = prototype[Symbol.asyncDispose], sentinel = {}, receiver;
        var returned = await dispose.call({return() {receiver = this; return Promise.resolve(42);}});
        check(returned === undefined && receiver !== undefined);
        check(await dispose.call({}) === undefined);
        var same = false;
        try { await dispose.call({return() {return Promise.reject(sentinel);}}); }
        catch (error) { same = error === sentinel; }
        check(same);
    "#,
        true,
    );
}

#[test]
fn zip_return_closes_all_sources_in_reverse_order_and_retains_the_first_error() {
    run(
        r#"
        for (var started of [false, true]) {
            var log = [], sentinel = {}, later = {};
            function source(name, error) { return {
                next() {return {done:false, value:name};},
                return() {log.push(name); throw error;}
            }; }
            var iterator = Iterator.zip([source('a', later), source('b', sentinel)]);
            if (started) iterator.next();
            var same = false;
            try { iterator.return(); } catch (e) {same = e === sentinel;}
            if (!same || log.join() !== 'b,a' || !iterator.next().done) throw 'wrong close';
        }
        true
    "#,
        false,
    );
}

#[test]
fn concat_and_windows_close_on_abrupt_next_and_remain_completed() {
    run(
        r#"
        var sentinel = {}, passed = 0;
        for (var kind of ['concat', 'windows', 'chunks']) {
            var closes = 0, calls = 0;
            var source = {[Symbol.iterator]() {return this;},
                next() {if (++calls === 2) throw sentinel; return {value:1, done:false};},
                return() {closes++; return {};}
            };
            var iterator = kind === 'concat' ? Iterator.concat(source) : Iterator.from(source)[kind](2);
            var same = false;
            try {iterator.next(); iterator.next();} catch (e) {same = e === sentinel;}
            if (same && iterator.next().done && closes <= 1) passed++;
        }
        passed === 3
    "#,
        false,
    );
}

#[test]
fn longest_zip_collects_and_closes_padding_then_reuses_its_values() {
    run(
        r#"
        var log = [];
        var padding = {[Symbol.iterator]() {log.push('open'); return {
            next() {log.push('padding'); return {done:false, value:9};},
            return() {log.push('close'); return {};}
        };}};
        var iterator = Iterator.zip([[1,2].values(), [3].values()], {mode:'longest', padding});
        var values = iterator.toArray();
        values.length === 2 && values[0].join() === '1,3' && values[1].join() === '2,9' &&
            log.join() === 'open,padding,padding,close'
    "#,
        false,
    );
}
