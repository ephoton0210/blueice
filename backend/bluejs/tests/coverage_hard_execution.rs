// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Eval environments, completion cleanup and delegated generator requests.

use blueice_bluejs::{compile, parse, Value, Vm, VmConfig};

fn configurations() -> [VmConfig; 2] {
    let mut stressed = VmConfig::default();
    stressed.heap.nursery_capacity = 1;
    [VmConfig::default(), stressed]
}

fn assert_script(source: &str) {
    for config in configurations() {
        let mut vm = Vm::new(config).unwrap();
        assert_eq!(
            vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
                .unwrap(),
            Value::Bool(true),
            "{source}"
        );
    }
}

fn assert_async(source: &str) {
    for config in configurations() {
        let mut vm = Vm::new(config).unwrap();
        vm.install_test262_done().unwrap();
        let source = format!(
            "function check(value) {{ if (!value) throw new Error('assertion failed'); }}
             (async () => {{ {source} }})().then(() => $DONE(), error => $DONE(error));"
        );
        vm.execute_script(&compile(&parse(&source).unwrap()).unwrap())
            .unwrap();
        vm.run_promise_jobs().unwrap();
        assert_eq!(vm.take_test262_done(), Some(Ok(())), "{source}");
        assert_eq!(
            vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap())
                .unwrap(),
            Value::Number(42.0)
        );
    }
}

#[test]
fn captured_eval_bindings_can_be_deleted_from_an_inner_function() {
    assert_script(
        r#"
        function outer() {
            eval('var dynamic = 7');
            return function() {
                var before = dynamic;
                var deleted = delete dynamic;
                return before === 7 && deleted && typeof dynamic === 'undefined' && delete dynamic;
            };
        }
        outer()()
    "#,
    );
}

#[test]
fn current_eval_bindings_can_be_deleted_from_the_declaring_function() {
    assert_script(
        r#"
        function local() {
            eval('var dynamic = 7');
            var before = dynamic, deleted = delete dynamic;
            return before === 7 && deleted && typeof dynamic === 'undefined';
        }
        local()
    "#,
    );
}

#[test]
fn eval_initializers_recreate_deleted_global_and_function_bindings() {
    assert_script(
        r#"
        eval('var globalEval = (delete globalEval, 11)');
        function local() {
            eval('var localEval = (delete localEval, 13)');
            return localEval;
        }
        globalEval === 11 && local() === 13 && delete globalEval && typeof globalEval === 'undefined'
    "#,
    );
}

#[test]
fn with_assignment_and_eval_preserve_captured_binding_and_unscopables() {
    assert_script(
        r#"
        var record = {value: 1}, write, read;
        with (record) {
            write = function(next) { value = next; };
            read = function() { return value; };
        }
        write(8);
        var first = record.value === 8 && read() === 8;
        record[Symbol.unscopables] = {value: true};
        var value = 21;
        write(34);
        first && record.value === 8 && value === 34 && read() === 34
    "#,
    );
}

#[test]
fn private_access_brand_errors_do_not_replace_finally_completion() {
    assert_script(
        r#"
        class C {
            #value = 7;
            get(other) { return other.#value; }
            set(other) { other.#value = 9; }
        }
        var c = new C(), count = 0;
        for (var other of [null, 1, {}, new Proxy(c, {})]) {
            try { c.get(other); } catch (e) { if (e instanceof TypeError) count++; }
            try { c.set(other); } catch (e) { if (e instanceof TypeError) count++; }
        }
        count === 8 && c.get(c) === 7
    "#,
    );
}

#[test]
fn nested_iterator_closes_preserve_the_first_throw_and_close_outer_records() {
    assert_script(
        r#"
        var sentinel = {}, log = [];
        function iterable(name) {
            return {[Symbol.iterator]() { return {
                next() { return {done: false, value: 1}; },
                return() { log.push(name); if (name === 'inner') throw sentinel; return {}; }
            }; }};
        }
        function finish() {
            for (var outer of iterable('outer')) {
                for (var inner of iterable('inner')) { return 42; }
            }
        }
        var same = false;
        try { finish(); } catch (e) { same = e === sentinel; }
        same && log.join() === 'inner,outer'
    "#,
    );
}

#[test]
fn generator_abrupt_delegation_runs_finally_and_retains_throw_identity() {
    assert_script(
        r#"
        var sentinel = {}, log = [];
        var delegate = {[Symbol.iterator]() { return this; },
            next() { return {done: false, value: 1}; },
            throw() { return {get done() { throw sentinel; }}; },
            return() { log.push('close'); return {done: true, value: 5}; }
        };
        function* g() { try { yield* delegate; } finally { log.push('finally'); } }
        var it = g(), first = it.next(), same = false;
        try { it.throw(7); } catch (error) { same = error === sentinel; }
        var ended = it.next();
        first.value === 1 && same && ended.done && log.join() === 'finally'
    "#,
    );
}

#[test]
fn generator_return_can_yield_from_finally_before_finishing_the_request() {
    assert_script(
        r#"
        function* g() { try { yield 1; } finally { yield 2; } }
        var it = g(); it.next();
        var pending = it.return(9), final = it.next();
        pending.value === 2 && !pending.done && final.value === 9 && final.done
    "#,
    );
}

#[test]
fn async_delegate_throw_and_return_complete_queued_requests_in_order() {
    assert_async(
        r#"
        var log = [];
        var delegate = {[Symbol.asyncIterator]() { return this; },
            next() { log.push('next'); return Promise.resolve({done: false, value: 1}); },
            throw(v) { log.push('throw'); return Promise.resolve({done: false, value: v}); },
            return(v) { log.push('return'); return Promise.resolve({done: true, value: v}); }
        };
        async function* g() { try { yield* delegate; } finally { log.push('finally'); } }
        var it = g(), first = it.next(), thrown = it.throw(7), returned = it.return(9), ended = it.next();
        var values = await Promise.all([first, thrown, returned, ended]);
        check(values[0].value === 1 && !values[0].done);
        check(values[1].value === 7 && !values[1].done);
        check(values[2].value === 9 && values[2].done && values[3].done);
        check(log.join() === 'next,throw,return,finally');
    "#,
    );
}

#[test]
fn async_delegate_result_getters_reject_without_stranding_the_request_queue() {
    assert_async(
        r#"
        for (var operation of ['next', 'throw', 'return']) {
            for (var property of ['done', 'value']) {
                var sentinel = {}, calls = 0;
                var bad = {done: true, value: 1};
                Object.defineProperty(bad, property, {get() { throw sentinel; }});
                var delegate = {[Symbol.asyncIterator]() { return this; },
                    next() { return ++calls === 1 && operation !== 'next' ? {done:false,value:1} : bad; },
                    throw() { return bad; }, return() { return bad; }
                };
                async function* g() { yield* delegate; }
                var it = g();
                if (operation !== 'next') await it.next();
                var same = false;
                try { await it[operation](7); } catch (error) { same = error === sentinel; }
                check(same && (await it.next()).done);
            }
        }
    "#,
    );
}

#[test]
fn missing_async_delegate_throw_closes_before_rejecting_and_runs_finally() {
    assert_async(
        r#"
        var log = [];
        var delegate = {[Symbol.asyncIterator]() { return this; },
            next() { return {done: false, value: 1}; },
            return() { log.push('close'); return Promise.resolve({done: true}); }
        };
        async function* g() { try { yield* delegate; } finally { log.push('finally'); } }
        var it = g(); await it.next();
        var rejected = false;
        try { await it.throw(7); } catch (e) { rejected = e instanceof TypeError; }
        check(rejected && log.join() === 'close,finally' && (await it.next()).done);
    "#,
    );
}

#[test]
fn missing_async_delegate_throw_preserves_close_failures_and_finally_yields() {
    assert_async(
        r#"
        var sentinel = {}, passed = 0;
        for (var close of [
            {get return() { throw sentinel; }},
            {return() { throw sentinel; }},
            {return() { return Promise.reject(sentinel); }},
            {return() { return {get then() { throw sentinel; }}; }},
            {return() { return 7; }},
            {},
            {return() { return Promise.resolve({done: true}); }}
        ]) {
            var delegate = Object.assign({[Symbol.asyncIterator]() { return this; },
                next() { return {done: false, value: 1}; }}, {});
            Object.defineProperties(delegate, Object.getOwnPropertyDescriptors(close));
            var caught;
            async function* g() {
                try { yield* delegate; }
                catch (e) { caught = e; }
                finally { yield 42; }
                return 9;
            }
            var it = g(); await it.next();
            var answer = await it.throw(8);
            check(answer.value === 42 && !answer.done);
            check(passed < 4 ? caught === sentinel : caught instanceof TypeError);
            var end = await it.next(); check(end.done && end.value === 9);
            passed++;
        }
        check(passed === 7);
    "#,
    );
}

#[test]
fn missing_throw_on_a_sync_delegate_does_not_await_its_close_result() {
    assert_async(
        r#"
        var observed = false, closed = 0, log = [];
        var delegate = {[Symbol.iterator]() { return this; },
            next() { return {done: false, value: 1}; },
            return() { closed++; log.push('close'); return {get then() { observed = true; throw 7; }}; }
        };
        async function* g() { try { yield* delegate; } catch (e) { log.push('catch'); return e instanceof TypeError; } }
        var it = g(); await it.next(); var pending = it.throw(8); log.push('after'); var end = await pending;
        check(end.done && end.value && closed === 1 && !observed && log.join() === 'close,after,catch');
    "#,
    );
}
