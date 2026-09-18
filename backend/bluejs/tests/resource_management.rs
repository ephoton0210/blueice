// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public-pipeline regressions for Explicit Resource Management:
//! `Symbol.dispose`/`Symbol.asyncDispose`, `DisposableStack`/
//! `AsyncDisposableStack`, `SuppressedError`, and `using` declarations.

use blueice_bluejs::{compile, parse, CompileError, ParseError, RuntimeError, Value, Vm};

fn execute(vm: &mut Vm, source: &str) -> Result<Value, RuntimeError> {
    vm.execute(&compile(&parse(source).unwrap()).unwrap())
}

fn compile_error(source: &str) -> CompileError {
    match compile(&parse(source).unwrap()) {
        Ok(_) => panic!("expected a compile error for {source}"),
        Err(error) => error,
    }
}

fn parse_only(source: &str) -> Result<(), ParseError> {
    parse(source).map(|_| ())
}

#[test]
fn symbol_dispose_and_async_dispose_are_well_known_symbols() {
    let mut vm = Vm::default();
    for source in [
        "typeof Symbol.dispose==='symbol'",
        "typeof Symbol.asyncDispose==='symbol'",
        "Symbol.dispose===Symbol.dispose",
        "Symbol.dispose!==Symbol.asyncDispose",
        "Symbol.dispose.description==='Symbol.dispose'",
    ] {
        assert_eq!(execute(&mut vm, source), Ok(Value::Bool(true)), "{source}");
    }
}

#[test]
fn disposable_stack_use_disposes_in_reverse_order_on_dispose_call() {
    let mut vm = Vm::default();
    let source = r#"
        let log = [];
        let stack = new DisposableStack();
        stack.use({ [Symbol.dispose]() { log.push('a'); } });
        stack.use({ [Symbol.dispose]() { log.push('b'); } });
        stack.use({ [Symbol.dispose]() { log.push('c'); } });
        stack.dispose();
        log.join(',') === 'c,b,a' && stack.disposed === true
    "#;
    assert_eq!(execute(&mut vm, source), Ok(Value::Bool(true)));
}

#[test]
fn disposable_stack_dispose_is_idempotent_and_use_after_disposed_throws() {
    let mut vm = Vm::default();
    let source = r#"
        let calls = 0;
        let stack = new DisposableStack();
        stack.use({ [Symbol.dispose]() { calls++; } });
        stack.dispose();
        stack.dispose();
        let threw = false;
        try { stack.use({}); } catch (error) { threw = error instanceof ReferenceError; }
        calls === 1 && threw
    "#;
    assert_eq!(execute(&mut vm, source), Ok(Value::Bool(true)));
}

#[test]
fn disposable_stack_use_allows_nullish_and_rejects_non_object_without_dispose() {
    let mut vm = Vm::default();
    for source in [
        "new DisposableStack().use(null) === null",
        "new DisposableStack().use(undefined) === undefined",
    ] {
        assert_eq!(execute(&mut vm, source), Ok(Value::Bool(true)), "{source}");
    }
    let threw_not_object = r#"
        let threw = false;
        try { new DisposableStack().use(1); } catch (error) { threw = error instanceof TypeError; }
        threw
    "#;
    assert_eq!(execute(&mut vm, threw_not_object), Ok(Value::Bool(true)));
    let threw_missing_method = r#"
        let threw = false;
        try { new DisposableStack().use({}); } catch (error) { threw = error instanceof TypeError; }
        threw
    "#;
    assert_eq!(execute(&mut vm, threw_missing_method), Ok(Value::Bool(true)));
}

#[test]
fn disposable_stack_adopt_and_defer_call_with_expected_receiver_and_argument() {
    let mut vm = Vm::default();
    let source = r#"
        'use strict';
        let log = [];
        let stack = new DisposableStack();
        let resource = { name: 'db' };
        let adopted = stack.adopt(resource, function (value) { log.push(['adopt', this === undefined, value === resource]); });
        let deferred = stack.defer(function () { log.push(['defer', this === undefined]); });
        stack.dispose();
        adopted === resource && deferred === undefined &&
            log.length === 2 &&
            log[0][0] === 'defer' && log[0][1] === true &&
            log[1][0] === 'adopt' && log[1][1] === true && log[1][2] === true
    "#;
    assert_eq!(execute(&mut vm, source), Ok(Value::Bool(true)));
}

#[test]
fn disposable_stack_move_transfers_resources_and_disposes_the_source() {
    let mut vm = Vm::default();
    let source = r#"
        let log = [];
        let stack = new DisposableStack();
        stack.use({ [Symbol.dispose]() { log.push('moved'); } });
        let moved = stack.move();
        let sameStackDisposedAlready = stack.disposed === true && log.length === 0;
        moved.dispose();
        sameStackDisposedAlready && log.join(',') === 'moved' && moved.disposed === true
    "#;
    assert_eq!(execute(&mut vm, source), Ok(Value::Bool(true)));
}

#[test]
fn suppressed_error_wraps_dispose_time_error_over_pending_error() {
    let mut vm = Vm::default();
    let source = r#"
        let stack = new DisposableStack();
        stack.use({ [Symbol.dispose]() { throw new Error('first'); } });
        stack.use({ [Symbol.dispose]() { throw new Error('second'); } });
        let caught;
        try { stack.dispose(); } catch (error) { caught = error; }
        caught instanceof SuppressedError &&
            caught.error.message === 'first' &&
            caught.suppressed.message === 'second'
    "#;
    assert_eq!(execute(&mut vm, source), Ok(Value::Bool(true)));
}

#[test]
fn suppressed_error_constructor_shape() {
    let mut vm = Vm::default();
    for source in [
        "SuppressedError.length === 3",
        "SuppressedError.name === 'SuppressedError'",
        "Object.getPrototypeOf(SuppressedError.prototype) === Error.prototype",
        "new SuppressedError(1, 2).error === 1 && new SuppressedError(1, 2).suppressed === 2",
        "!Object.prototype.hasOwnProperty.call(new SuppressedError(1, 2, undefined), 'message')",
        "new SuppressedError(1, 2, 'msg').message === 'msg'",
    ] {
        assert_eq!(execute(&mut vm, source), Ok(Value::Bool(true)), "{source}");
    }
}

#[test]
fn using_declaration_disposes_at_block_exit_in_reverse_order() {
    let mut vm = Vm::default();
    let source = r#"
        let log = [];
        function resource(name) {
            return { [Symbol.dispose]() { log.push(name); } };
        }
        {
            using a = resource('a');
            using b = resource('b');
            log.push('body');
        }
        log.join(',') === 'body,b,a'
    "#;
    assert_eq!(execute(&mut vm, source), Ok(Value::Bool(true)));
}

#[test]
fn using_declaration_disposes_on_early_return_break_and_throw() {
    let mut vm = Vm::default();
    let source_return = r#"
        let log = [];
        function resource(name) {
            return { [Symbol.dispose]() { log.push(name); } };
        }
        function f() {
            using a = resource('a');
            return 'value';
        }
        f() === 'value' && log.join(',') === 'a'
    "#;
    assert_eq!(execute(&mut vm, source_return), Ok(Value::Bool(true)));

    let source_break = r#"
        let log = [];
        function resource(name) {
            return { [Symbol.dispose]() { log.push(name); } };
        }
        for (let i = 0; i < 1; i++) {
            using a = resource('a');
            break;
        }
        log.join(',') === 'a'
    "#;
    assert_eq!(execute(&mut vm, source_break), Ok(Value::Bool(true)));

    let source_throw = r#"
        let log = [];
        function resource(name) {
            return { [Symbol.dispose]() { log.push(name); } };
        }
        let caught;
        try {
            using a = resource('a');
            throw new Error('boom');
        } catch (error) {
            caught = error.message;
        }
        caught === 'boom' && log.join(',') === 'a'
    "#;
    assert_eq!(execute(&mut vm, source_throw), Ok(Value::Bool(true)));
}

#[test]
fn using_declaration_wraps_double_error_in_suppressed_error() {
    let mut vm = Vm::default();
    let source = r#"
        function resource(name) {
            return { [Symbol.dispose]() { throw new Error(name); } };
        }
        let caught;
        try {
            using a = resource('outer-body-error-suppressed');
            throw new Error('body');
        } catch (error) {
            caught = error;
        }
        caught instanceof SuppressedError &&
            caught.error.message === 'outer-body-error-suppressed' &&
            caught.suppressed.message === 'body'
    "#;
    assert_eq!(execute(&mut vm, source), Ok(Value::Bool(true)));
}

#[test]
fn using_declaration_allows_null_and_undefined_without_disposing() {
    let mut vm = Vm::default();
    let source = r#"
        {
            using a = null;
            using b = undefined;
        }
        true
    "#;
    assert_eq!(execute(&mut vm, source), Ok(Value::Bool(true)));
}

#[test]
fn using_declaration_binding_is_immutable_like_const() {
    let mut vm = Vm::default();
    let source = r#"
        let threw = false;
        {
            using a = { [Symbol.dispose]() {} };
            try { a = 1; } catch (error) { threw = error instanceof TypeError; }
        }
        threw
    "#;
    assert_eq!(execute(&mut vm, source), Ok(Value::Bool(true)));
}

#[test]
fn using_declaration_requires_initializer_and_simple_identifier() {
    assert!(matches!(
        compile_error("{ using a; }"),
        CompileError::InvalidSyntax(_)
    ));
    assert!(matches!(
        compile_error("{ using a = 1, [b] = [2]; }"),
        CompileError::InvalidSyntax(_)
    ));
}

#[test]
fn using_is_still_a_plain_identifier_outside_declaration_position() {
    let mut vm = Vm::default();
    for source in [
        "let using = 1; using === 1",
        "function using(){return 2;} using() === 2",
    ] {
        assert_eq!(execute(&mut vm, source), Ok(Value::Bool(true)), "{source}");
    }
    // `using` followed by a newline is not a declaration (no lookahead
    // across the line terminator), so this is two ordinary statements.
    parse_only("{\n  using\n  x = 1;\n}").expect("using-newline-x is not a declaration");
}

#[test]
fn using_no_using_declaration_is_zero_overhead_fast_path() {
    // A block/function body without any `using` declaration must compile
    // and run exactly as it did before this feature existed (no synthetic
    // try/finally, no disposal opcodes reachable).
    let mut vm = Vm::default();
    assert_eq!(
        execute(&mut vm, "{ let x = 1; x + 1 }"),
        Ok(Value::Number(2.0))
    );
}

#[test]
fn async_disposable_stack_dispose_async_resolves_and_disposes_in_reverse_order() {
    let mut vm = Vm::default();
    let setup = r#"
        var result = 'pending';
        var log = [];
        var stack = new AsyncDisposableStack();
        stack.use({ [Symbol.asyncDispose]() { log.push('a'); } });
        stack.use({ [Symbol.dispose]() { log.push('b'); } });
        stack.disposeAsync().then(function (value) {
            result = value === undefined && log.join(',') === 'b,a' && stack.disposed === true;
        });
    "#;
    vm.execute_script(&compile(&parse(setup).unwrap()).unwrap())
        .unwrap();
    vm.run_promise_jobs().unwrap();
    assert_eq!(execute(&mut vm, "result"), Ok(Value::Bool(true)));
}

#[test]
fn async_disposable_stack_dispose_async_rejects_with_dispose_error() {
    let mut vm = Vm::default();
    let setup = r#"
        var result = 'pending';
        var stack = new AsyncDisposableStack();
        stack.use({ [Symbol.asyncDispose]() { throw new Error('async-fail'); } });
        stack.disposeAsync().then(
            function () { result = false; },
            function (error) { result = error instanceof Error && error.message === 'async-fail'; }
        );
    "#;
    vm.execute_script(&compile(&parse(setup).unwrap()).unwrap())
        .unwrap();
    vm.run_promise_jobs().unwrap();
    assert_eq!(execute(&mut vm, "result"), Ok(Value::Bool(true)));
}

#[test]
fn using_declaration_infers_anonymous_function_name_from_its_binding() {
    let mut vm = Vm::default();
    // NamedEvaluation applies to any single-identifier lexical binding with
    // an anonymous function/arrow/class initializer, `using` included; the
    // dispose call is patched to a no-op afterward so this only exercises
    // name inference, not disposal.
    let source = r#"
        Function.prototype[Symbol.dispose] = function () {};
        let arrowName, fnExprName;
        {
            using arrow = () => {};
            arrowName = arrow.name;
        }
        {
            using fn = function () {};
            fnExprName = fn.name;
        }
        arrowName === 'arrow' && fnExprName === 'fn'
    "#;
    assert_eq!(execute(&mut vm, source), Ok(Value::Bool(true)));
}

#[test]
fn using_directly_in_a_switch_case_is_a_compile_error() {
    assert!(matches!(
        compile_error("switch (0) { case 0: using x = null; break; }"),
        CompileError::InvalidSyntax(_)
    ));
    assert!(matches!(
        compile_error("switch (0) { default: using x = null; }"),
        CompileError::InvalidSyntax(_)
    ));
    // A `using` inside its own block within a case is fine -- only a
    // *direct* case-body declaration is rejected.
    let mut vm = Vm::default();
    let source = r#"
        let disposed = false;
        switch (0) {
            case 0: {
                using x = { [Symbol.dispose]() { disposed = true; } };
                break;
            }
        }
        disposed
    "#;
    assert_eq!(execute(&mut vm, source), Ok(Value::Bool(true)));
}

fn run_async(vm: &mut Vm, setup: &str) -> Value {
    vm.execute_script(&compile(&parse(setup).unwrap()).unwrap())
        .unwrap();
    vm.run_promise_jobs().unwrap();
    execute(vm, "result").unwrap()
}

#[test]
fn await_using_awaits_the_dispose_methods_own_returned_promise() {
    let mut vm = Vm::default();
    // Proves this goes through a real `Await` (suspend/resume), not just a
    // synchronous call: the disposing function only settles once the
    // dispose method's own promise resolves, so `log` observes the
    // dispose-triggered work interleaved correctly rather than skipped.
    let setup = r#"
        var result = 'pending';
        var log = [];
        async function run() {
            await using a = { [Symbol.asyncDispose]() {
                return Promise.resolve().then(function () { log.push('a-resolved'); });
            } };
            log.push('body');
        }
        run().then(function () { result = log.join(','); });
    "#;
    assert_eq!(run_async(&mut vm, setup), Value::String("body,a-resolved".into()));
}

#[test]
fn await_using_disposes_sync_and_async_resources_in_reverse_declaration_order() {
    let mut vm = Vm::default();
    let setup = r#"
        var result = 'pending';
        var log = [];
        async function run() {
            await using a = { [Symbol.asyncDispose]() { log.push('a'); } };
            using b = { [Symbol.dispose]() { log.push('b'); } };
            log.push('body');
            return 'value';
        }
        run().then(function (value) { result = JSON.stringify([value, log.join(',')]); });
    "#;
    assert_eq!(
        run_async(&mut vm, setup),
        Value::String("[\"value\",\"body,b,a\"]".into())
    );
}

#[test]
fn await_using_propagates_a_rejected_dispose_promise_as_a_real_rejection() {
    let mut vm = Vm::default();
    let setup = r#"
        var result = 'pending';
        async function run() {
            await using a = { [Symbol.asyncDispose]() { return Promise.reject(new Error('later-fail')); } };
        }
        run().then(
            function (value) { result = ['fulfilled', value]; },
            function (error) { result = ['rejected', error.message]; }
        );
    "#;
    vm.execute_script(&compile(&parse(setup).unwrap()).unwrap())
        .unwrap();
    vm.run_promise_jobs().unwrap();
    assert_eq!(
        execute(&mut vm, "JSON.stringify(result)"),
        Ok(Value::String("[\"rejected\",\"later-fail\"]".into()))
    );
}

#[test]
fn await_using_wraps_dispose_and_body_errors_in_suppressed_error() {
    let mut vm = Vm::default();
    let setup = r#"
        var result = 'pending';
        async function run() {
            await using a = { [Symbol.asyncDispose]() { throw new Error('dispose-fail'); } };
            throw new Error('body-fail');
        }
        run().then(
            function () { result = false; },
            function (error) {
                result = error instanceof SuppressedError &&
                    error.error.message === 'dispose-fail' &&
                    error.suppressed.message === 'body-fail';
            }
        );
    "#;
    assert_eq!(run_async(&mut vm, setup), Value::Bool(true));
}

#[test]
fn await_using_null_resource_still_resolves_without_error() {
    let mut vm = Vm::default();
    let setup = r#"
        var result = 'pending';
        async function run() {
            await using a = null;
            return 'ok';
        }
        run().then(function (value) { result = value; });
    "#;
    assert_eq!(run_async(&mut vm, setup), Value::String("ok".into()));
}

#[test]
fn await_using_requires_async_context() {
    // Outside an async context `await` is an ordinary identifier
    // (`await_using_declaration_follows` requires `async_depth != 0 ||
    // module_await`, matching a plain `await` expression's own gate), so
    // `await using x = null;` is rejected already at parse time -- `await`
    // followed immediately by another primary expression (`using`) with no
    // operator between them is not valid grammar for a non-async `await`
    // identifier reference either.
    assert!(matches!(
        parse_only("function f() { await using x = null; }"),
        Err(_)
    ));
}
