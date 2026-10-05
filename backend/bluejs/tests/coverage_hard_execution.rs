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
fn property_coercion_and_deleted_eval_references_preserve_observable_errors() {
    assert_script(
        r#"
        var object = {value:7}, key = {[Symbol.toPrimitive](){throw 7;}}, checked = 0;
        for (var operation of [()=>delete object[key], ()=>object[key], ()=>object[key] = 42, ()=>object[key]++]) {
            try {operation();} catch (error) {if(error === 7) checked++; else throw error;}
        }
        var hostile = new Proxy({}, {has(){return true;}, get(){throw 7;}, deleteProperty(){throw 7;}});
        for (var operation of [()=>{with(hostile){answer;}}, ()=>{with(hostile){answer();}}, ()=>{with(hostile){typeof answer;}}, ()=>{with(hostile){delete answer;}}]) {
            try {operation();} catch(error) {if(error === 7) checked++; else throw error;}
        }
        var numeric = {[Symbol.toPrimitive](){throw 7;}};
        try {with({item:numeric}){item++;}} catch(error) {if(error === 7) checked++; else throw error;}
        var protectedObject = new Proxy({item:7}, {set(){throw 7;}});
        try {with(protectedObject){item = 42;}} catch(error) {if(error === 7) checked++; else throw error;}
        globalThis.strictVictim = 7;
        try {(function(){'use strict'; strictVictim = (delete globalThis.strictVictim, 42);})();}
        catch(error) {if(error instanceof ReferenceError) checked++; else throw error;}
        function deleted() {
            eval('var vanished = 7');
            delete vanished;
            Object.defineProperty(globalThis, 'vanished', {get(){throw 7;}, configurable:true});
            try {with({}) {typeof vanished;}} catch(error) {if(error === 7) checked++; else throw error;}
            delete globalThis.vanished;
        }
        deleted();
        checked === 12 && object.value === 7 && !('strictVictim' in globalThis)
    "#,
    );
}

#[test]
fn private_methods_and_accessors_retain_their_home_objects_across_collection() {
    let source = r#"
        class Base {static answer() {return 42;} answer() {return 42;}}
        class Method extends Base {
            static #answer() {return super.answer();}
            static read() {return this.#answer();}
        }
        class Getter extends Base {
            static get #answer() {return super.answer();}
            static read() {return this.#answer;}
        }
        class Setter extends Base {
            static set #answer(value) {this.saved = value + super.answer();}
            static write() {this.#answer = 0; return this.saved;}
        }
        class Instance extends Base {
            #answer() {return () => super.answer();}
            read() {return this.#answer()();}
        }
        Method.read() === 42 && Getter.read() === 42 && Setter.write() === 42 &&
            new Instance().read() === 42
    "#;
    let program = compile(&parse(source).unwrap()).unwrap();
    let reuse = compile(&parse("21 + 21").unwrap()).unwrap();
    let mut major = VmConfig::default();
    major.heap.nursery_capacity = 1;
    major.heap.major_threshold_bytes = 1;
    for config in configurations().into_iter().chain([major]) {
        let mut vm = Vm::new(config).unwrap();
        assert_eq!(vm.execute_script(&program), Ok(Value::Bool(true)));
        assert_eq!(vm.execute_script(&reuse), Ok(Value::Number(42.0)));
    }
}

#[test]
fn compiled_references_preserve_private_super_with_and_destructuring_failures() {
    assert_script(
        r#"
        var sentinel = {}, checked = 0;
        for (var run of [
            () => {class C {#value; static read(o) {return o.#value;}} C.read(null);},
            () => {class C {#value; static write(o) {o.#value = 7;}} C.write(null);},
            () => {class C {#value; static has(o) {return #value in o;}} C.has(null);},
            () => {class Base {constructor() {return Object.preventExtensions({});}} class C extends Base {#value = 7;} new C();},
            () => {class C {static #value = Symbol(); static update() {return this.#value++;}} C.update();},
            () => {var o = {update() {return super.value++;}}; Object.setPrototypeOf(o, {value:Symbol()}); o.update();},
            () => {class Base {} class C extends Base {constructor() {this.value = 7;}} new C();},
            () => {class Base {} class C extends Base {constructor() {super(); super();}} new C();},
        ]) {
            try {run(); throw 'accepted invalid reference';}
            catch (error) {if (!(error instanceof TypeError) && !(error instanceof ReferenceError)) throw error;}
            checked++;
        }
        for (var run of [
            () => {var o = {}; o[{toString() {throw sentinel;}}] = 42;},
            () => {var o = {}; ({answer:o[{toString() {throw sentinel;}}]} = {answer:42});},
            () => {var o = {}; [o[{toString() {throw sentinel;}}]] = [42];},
            () => {var o = {get value() {throw sentinel;}}; o.value++;},
            () => {var o = {update() {return super.value++;}}; Object.setPrototypeOf(o, {get value() {throw sentinel;}}); o.update();},
            () => {var o = {update() {return super.value++;}}; Object.setPrototypeOf(o, {get value() {return 7;}, set value(v) {throw sentinel;}}); o.update();},
            () => {var o = new Proxy({value:7}, {has() {throw sentinel;}}); with(o) {value;}},
            () => {var o = {value:{valueOf() {throw sentinel;}}}; with(o) {value++;}},
            () => {var o = {value:7,[Symbol.unscopables]:{get value() {throw sentinel;}}}; with(o) {delete value;}},
            () => eval(...{[Symbol.iterator]() {throw sentinel;}}),
            () => {var iterable = {[Symbol.iterator]() {return {next() {throw sentinel;}}}}; var [first,...rest] = iterable;},
        ]) {
            try {run(); throw 'accepted user failure';}
            catch (error) {if (error !== sentinel) throw error;}
            checked++;
        }
        checked === 19
    "#,
    );
}

#[test]
fn property_reference_and_eval_failures_retain_their_public_error_identity() {
    assert_script(
        r#"
        var sentinel = {}, passed = 0;
        for (var run of [
            () => null.value,
            () => delete null.value,
            () => {var target = null; target.value++;},
            () => eval(...['var =']),
            () => {class Base {constructor() {return new Proxy({}, {isExtensible() {throw sentinel;}});}} class C extends Base {#method() {}} new C();},
            () => {var target = {}; delete target[{toString() {throw sentinel;}}];},
            () => {var target = {}; target[{toString() {throw sentinel;}}]++;},
            () => {var target = {}; ({value:target[{toString() {throw sentinel;}}]} = {value:42});},
            () => {var read = () => {with({}) {return later;}}; read(); let later = 42;},
            () => {var read = () => typeof later; read(); let later = 42;},
        ]) {
            try {run(); throw 'accepted invalid operation';}
            catch(error) {if (error !== sentinel && !(error instanceof TypeError) && !(error instanceof SyntaxError) && !(error instanceof ReferenceError)) throw error;}
            passed++;
        }
        function dynamicDelete() {
            eval('var dynamicOnly = 7');
            if (!delete dynamicOnly || typeof dynamicOnly !== 'undefined') return false;
            return true;
        }
        passed === 10 && dynamicDelete()
    "#,
    );
}

#[test]
fn dynamic_eval_shadowing_preserves_compound_and_postfix_reference_cells() {
    assert_script(
        r#"
        var outer = 7;
        function container() {
            function inner() {
                eval('var outer = 11');
                var read = () => outer;
                outer += 2;
                var previous = outer++;
                outer--;
                return previous === 13 && outer === 13 && read() === 13;
            }
            return inner();
        }
        container() && outer === 7
    "#,
    );
}

#[test]
fn eval_shadowing_keeps_captured_local_reference_cells_and_prior_resolutions_separate() {
    assert_script(
        r#"
        function parent() {
            var captured = 7;
            var readParent = () => captured;
            return function() {
                eval('var captured = 11');
                captured += 2;
                var previous = captured++;
                captured--;
                return previous === 13 && captured === 13 && readParent() === 7;
            };
        }
        function priorResolution() {
            var captured = 7;
            var readParent = () => captured;
            function inner() {
                captured += eval('var captured = 11; 2');
                return captured === 11 && readParent() === 9;
            }
            return inner();
        }
        parent()() && priorResolution()
    "#,
    );
}

#[test]
fn deleted_eval_references_resolve_again_and_parameter_environments_remain_deletable() {
    assert_script(
        r#"
        function retained() {
            return eval('var vanished = 7; vanished += (delete vanished, 35); typeof vanished');
        }
        function parameters(value = eval('var parameterVar = 7')) {
            var read = () => parameterVar;
            var before = read();
            var deleted = delete parameterVar;
            var missing = typeof parameterVar;
            eval('var parameterVar = 42');
            return before === 7 && deleted && missing === 'undefined' && read() === 42 && delete parameterVar && delete parameterVar;
        }
        retained() === 'number' && vanished === 42 && delete vanished && parameters()
    "#,
    );
}

#[test]
fn an_async_delegate_preserves_a_rejected_return_operand_through_its_throw_method() {
    assert_async(
        r#"
        var sentinel = {}, finallyRuns = 0;
        var delegate = {[Symbol.asyncIterator]() {return this;}, next() {return {done:false,value:7};}, throw(error) {throw error;}};
        async function* outer() {
            try {yield* delegate;}
            catch (error) {check(error === sentinel); yield 42;}
            finally {finallyRuns++;}
        }
        var iterator = outer();
        check((await iterator.next()).value === 7);
        var answer = await iterator.return(Promise.reject(sentinel));
        check(answer.value === 42 && !answer.done);
        check((await iterator.next()).done && finallyRuns === 1);
        check((await iterator.next()).done && finallyRuns === 1);
    "#,
    );
}

#[test]
fn delegated_sync_methods_cover_missing_noncallable_primitive_and_abrupt_results() {
    assert_script(
        r#"
        var checked = 0;
        for (var operation of ['throw', 'return']) {
            for (var mode of ['absent', 'noncallable', 'primitive', 'throw', 'done', 'value', 'yield', 'finish']) {
                var sentinel = {}, log = [];
                var delegate = {[Symbol.iterator]() { return this; }, next() { return {done:false, value:1}; }};
                if (operation === 'throw') delegate.return = function(v) { log.push('close'); return {done:true, value:v}; };
                if (mode === 'noncallable') delegate[operation] = 1;
                else if (mode !== 'absent') delegate[operation] = function(v) {
                    log.push(operation);
                    if (mode === 'primitive') return 1;
                    if (mode === 'throw') throw sentinel;
                    if (mode === 'done') return {get done() {throw sentinel;}};
                    if (mode === 'value') return {done:true, get value() {throw sentinel;}};
                    return {done:mode === 'finish', value:v};
                };
                function* g() { try { return yield* delegate; } finally { log.push('finally'); } }
                var it = g();
                if (it.next().value !== 1) throw 'initial delegation';
                var answer, error = undefined;
                try { answer = it[operation](7); } catch (e) {error = e;}
                if (mode === 'throw' || mode === 'done' || mode === 'value') {
                    if (error !== sentinel) throw 'exception identity';
                } else if (mode === 'noncallable' || mode === 'primitive' || (operation === 'throw' && mode === 'absent')) {
                    if (!(error instanceof TypeError)) throw 'missing TypeError';
                } else {
                    if (error !== undefined || answer.value !== 7 || answer.done !== (mode !== 'yield')) throw 'delegated answer';
                }
                if (mode === 'yield') {mode = 'finish'; it.return(9);}
                if (!it.next().done || log.filter(v => v === 'finally').length !== 1) throw 'completion cleanup';
                checked++;
            }
        }
        checked === 16
    "#,
    );
}

#[test]
fn delegated_async_abrupt_results_do_not_strand_queued_requests() {
    assert_async(
        r#"
        for (var operation of ['next', 'throw', 'return']) {
            for (var mode of ['primitive', 'throw', 'reject', 'done', 'value']) {
                var sentinel = {}, calls = 0, log = [];
                var delegate = {[Symbol.asyncIterator]() {return this;},
                    next() {return {done:false, value:1};},
                    throw(v) {return {done:false, value:v};},
                    return(v) {return {done:true, value:v};}
                };
                var originalNext = delegate.next;
                var bad = function() {
                    if (mode === 'primitive') return Promise.resolve(1);
                    if (mode === 'throw') throw sentinel;
                    if (mode === 'reject') return Promise.reject(sentinel);
                    if (mode === 'done') return {get done() {throw sentinel;}};
                    return {done:true, get value() {throw sentinel;}};
                };
                delegate[operation] = operation === 'next' ? function() {return ++calls === 1 ? originalNext() : bad();} : bad;
                async function* g() {try {yield* delegate;} finally {log.push('finally');}}
                var it = g(); check((await it.next()).value === 1);
                var first = it[operation](7), later = it.next(), returned = it.return(9);
                var error;
                try {await first;} catch (e) {error = e;}
                check(mode === 'primitive' ? error instanceof TypeError : error === sentinel);
                check((await later).done);
                var last = await returned; check(last.done && last.value === 9);
                check(log.join() === 'finally');
            }
        }
    "#,
    );
}

#[test]
fn async_return_awaits_rejection_when_delegation_has_no_return_method() {
    assert_async(
        r#"
        for (var started of [false, true]) {
            var sentinel = {}, log = [];
            var delegate = {[Symbol.asyncIterator]() {return this;}, next() {return {done:false, value:1};}};
            async function* g() {try {yield* delegate;} catch (e) {log.push('caught'); throw e;} finally {log.push('finally');}}
            var it = g(); if (started) await it.next();
            var error = undefined, later = it.return(Promise.reject(sentinel));
            try {await later;} catch (e) {error = e;}
            // At an active yield*, rejecting the return operand becomes a
            // throw request. A delegate without throw reports its protocol
            // TypeError; an unstarted generator simply rejects the operand.
            check((started ? error instanceof TypeError : error === sentinel) && (await it.next()).done);
            check(log.join() === (started ? 'caught,finally' : ''));
        }
    "#,
    );
}

#[test]
fn generator_parameter_eval_errors_restore_outer_bindings_before_reuse() {
    assert_script(
        r#"
        var sentinel = {}, receiver = {value: 42}, calls = 0;
        function outer() {
            var local = 7;
            function* fail(value = eval('throw sentinel')) {yield value;}
            try {fail();} catch (error) {if (error !== sentinel) throw error; calls++;}
            return local === 7 && this === receiver && arguments[0] === 9;
        }
        outer.call(receiver, 9) && calls === 1 && (function* () {yield 42;})().next().value === 42
    "#,
    );
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
fn repeated_eval_declarations_shadow_captured_outer_cells_and_reuse_the_dynamic_binding() {
    assert_script(
        r#"
        function outer() {
            let captured = 7;
            function inner() {
                var before = captured;
                eval('var captured = 11;');
                var first = captured;
                eval('var captured = 13;');
                captured = 42;
                var read = () => captured;
                return before === 7 && first === 11 && read() === 42;
            }
            return inner() && captured === 7;
        }
        outer()
    "#,
    );
    assert_script(
        r#"
        function outer() {
            const captured = 7;
            function inner() {
                eval('var captured = 11;');
                eval('var captured = 13;');
                { const captured = 19; eval('captured;'); }
                return captured === 13;
            }
            return inner() && captured === 7;
        }
        outer()
        "#,
    );
    assert_async(
        r#"
        async function outer() {
            let captured = 7;
            async function inner() {
                var before = captured;
                eval('var captured = 11;');
                var first = captured;
                eval('var captured = 13;');
                return before === 7 && first === 11 && captured === 13;
            }
            check(await inner());
            check(captured === 7);
        }
        await outer();
        "#,
    );
}

#[test]
fn eval_capture_selection_preserves_resolved_references_and_lexical_environments() {
    assert_script(
        r#"
        var outside = 7;
        function unrelated() { return outside; }
        function caller() {
            var before = () => outside;
            outside = (eval('var outside;'), 42);
            if (outside !== undefined || unrelated() !== 42 || before() !== undefined) return false;
            eval('var outside = 11');
            eval('var outside = 13');
            return outside === 13 && before() === 13 && unrelated() === 42;
        }
        function enclosing() {
            const captured = 7;
            function local() {
                eval('var captured = 11');
                eval('var captured = (delete captured, 42)');
                if (captured !== 42) return false;
                { const captured = 19; if (eval('captured') !== 19) return false; }
                eval('captured = 13');
                return captured === 13;
            }
            return local() && captured === 7;
        }
        function caught() {
            try { throw 8; } catch (parameter) {
                eval('var parameter = 42');
                eval('var parameter = 19');
                if (parameter !== 19) return false;
            }
            return typeof parameter === 'undefined';
        }
        caller() && outside === 42 && enclosing() && caught() && typeof globalThis.captured === 'undefined'
        "#,
    );
}

#[test]
fn queued_async_generator_requests_survive_after_the_caller_releases_the_generator() {
    assert_async(
        r#"
        var first, returned, last;
        {
            let generator = (async function*() { yield {answer: 1}; })();
            first = generator.next();
            returned = generator.return(Promise.resolve({answer: 42}));
            last = generator.next();
            generator = null;
        }
        let results = await Promise.all([first, returned, last]);
        check(results[0].value.answer === 1 && !results[0].done);
        check(results[1].value.answer === 42 && results[1].done);
        check(results[2].done);
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

#[test]
fn async_eval_shadowing_keeps_compound_and_postfix_references_on_the_dynamic_cell() {
    assert_async(
        r#"
        function outer() {
            let captured = 7;
            var readParent = () => captured;
            return async function() {
                eval('var captured = 11');
                captured += 2;
                var previous = captured++;
                captured--;
                return previous === 13 && captured === 13 && readParent() === 7;
            };
        }
        check(await outer()());
        function parameters(value = eval('var parameterVar = (delete parameterVar, 42)')) {
            return parameterVar === 42;
        }
        check(parameters());
        check((function named() {named = 7; return typeof named === 'function';})());
    "#,
    );
}
