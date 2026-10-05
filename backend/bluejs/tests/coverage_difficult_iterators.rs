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
fn iterator_prototype_setters_throw_independently_of_caller_strictness() {
    run(
        r#"
        var sentinel = {}, checked = 0;
        for (var key of ['constructor', Symbol.toStringTag]) {
            var setter = Object.getOwnPropertyDescriptor(Iterator.prototype, key).set;
            for (var invoke of [
                (receiver, value) => setter.call(receiver, value),
                function(receiver, value) {'use strict'; return setter.call(receiver, value);}
            ]) {
                var readonly = Object.defineProperty({}, key, {value: 7, writable:false});
                var getterOnly = Object.defineProperty({}, key, {get() {return 7;}});
                var refused = new Proxy({[key]:7}, {set() {return false;}});
                var creationRefused = new Proxy({}, {defineProperty() {return false;}});
                for (var receiver of [readonly, getterOnly, refused, creationRefused]) {
                    try {invoke(receiver, 42); throw 'accepted refused write';}
                    catch (error) {if (!(error instanceof TypeError)) throw error; checked++;}
                }
                var throwing = Object.defineProperty({}, key, {set() {throw sentinel;}});
                var ownTrapThrowing = new Proxy({}, {getOwnPropertyDescriptor() {throw sentinel;}});
                var defineTrapThrowing = new Proxy({}, {defineProperty() {throw sentinel;}});
                for (var receiver of [throwing, ownTrapThrowing, defineTrapThrowing]) {
                    try {invoke(receiver, 42); throw 'ignored accessor or trap throw';}
                    catch (error) {if (error !== sentinel) throw error; checked++;}
                }
                var plain = {}, received;
                if (invoke(plain, 42) !== undefined || plain[key] !== 42) throw 'creation failed';
                var existing = Object.defineProperty({}, key, {set(value) {received = value;}});
                if (invoke(existing, 42) !== undefined || received !== 42) throw 'setter failed';
                checked += 2;
                var log = [], target = {}, proxy = new Proxy(target, {
                    getOwnPropertyDescriptor(object, property) {
                        log.push('own'); return Reflect.getOwnPropertyDescriptor(object, property);
                    },
                    defineProperty(object, property, descriptor) {
                        log.push('define');
                        if (descriptor.value !== 42 || !descriptor.writable || !descriptor.enumerable || !descriptor.configurable)
                            throw 'wrong creation descriptor';
                        return Reflect.defineProperty(object, property, descriptor);
                    }
                });
                if (invoke(proxy, 42) !== undefined || target[key] !== 42 || log.join() !== 'own,define')
                    throw 'incorrect Proxy creation protocol';
                checked++;
            }
        }
        var locked = Object.defineProperty({}, 'answer', {value:7, writable:false});
        locked.answer = 42;
        checked === 40 && locked.answer === 7;
        "#,
        false,
    );
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
fn reduce_validates_its_callback_before_reading_next_and_only_invokes_it_for_values() {
    run(
        r#"
        var events = [];
        var revoked = Proxy.revocable(function(a, b) {return a + b;}, {});
        var source = {get next() {
            events.push('next-get'); revoked.revoke();
            return function() {events.push('next-call'); return {done:true};};
        }};
        var answer = Iterator.prototype.reduce.call(source, revoked.proxy, 42);
        if (answer !== 42 || events.join() !== 'next-get,next-call') throw 'empty reduction';
        var closed = 0;
        try {
            Iterator.prototype.reduce.call({get next() {throw 'must not read';}, return() {closed++; return {}; }}, null, 0);
            throw 'missing callback error';
        } catch (error) {
            if (!(error instanceof TypeError) || closed !== 1) throw 'callback validation order';
        }
        true
    "#,
        false,
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

#[test]
fn callback_failures_close_single_source_helpers_once_and_preserve_identity() {
    run(
        r#"
        var passed = 0;
        for (var kind of ['map', 'filter', 'flatMap']) {
            var sentinel = {}, closeError = {}, closes = 0;
            var source = {next() {return {done:false, value:7};},
                return() {closes++; throw closeError;}};
            var helper = Iterator.from(source)[kind](() => {throw sentinel;});
            var same = false;
            try {helper.next();} catch (error) {same = error === sentinel;}
            if (same && closes === 1 && helper.next().done && helper.return().done) passed++;
        }
        passed === 3
    "#,
        false,
    );
}

#[test]
fn flat_map_return_closes_inner_before_outer_even_when_both_throw() {
    run(
        r#"
        var log = [], innerError = {}, outerError = {};
        var source = {next() {return {done:false, value:7};},
            return() {log.push('outer'); throw outerError;}};
        var inner = {[Symbol.iterator]() {return this;},
            next() {return {done:false, value:42};},
            return() {log.push('inner'); throw innerError;}};
        var helper = Iterator.from(source).flatMap(() => inner);
        var first = helper.next(), same = false;
        try {helper.return();} catch (error) {same = error === innerError;}
        first.value === 42 && same && log.join() === 'inner,outer' && helper.next().done
    "#,
        false,
    );
}

#[test]
fn recursive_helper_requests_fail_without_consuming_the_outer_request() {
    run(
        r#"
        var passed = 0;
        for (var kind of ['map', 'filter', 'flatMap']) {
            var errors = 0, helper;
            helper = Iterator.from([7])[kind](value => {
                for (var operation of [() => helper.next(), () => helper.return()]) {
                    try {operation();} catch (error) {if (error instanceof TypeError) errors++;}
                }
                return kind === 'flatMap' ? [value] : kind === 'filter' ? true : value;
            });
            var first = helper.next();
            if (errors === 2 && first.value === 7 && !first.done && helper.next().done) passed++;
        }
        passed === 3
    "#,
        false,
    );
}

#[test]
fn array_from_async_handles_reentrant_collection_and_constructor_failures() {
    run(
        r#"
        var sentinel = {}, calls = 0;
        function Throwing() {calls++; throw sentinel;}
        for (var source of [[1,2], {length:1, 0:7}]) {
            var same = false;
            try {await Array.fromAsync.call(Throwing, source);} catch (error) {same = error === sentinel;}
            check(same);
        }
        var values = await Array.fromAsync([1,2], async value => {
            var nested = await Array.fromAsync([value + 10]);
            return nested[0];
        });
        check(calls === 2 && values.join() === '11,12');
    "#,
        true,
    );
}
