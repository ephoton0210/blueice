// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Arguments objects, legacy function reflection, eval, array literals and the
//! lazily built iterator/generator intrinsics, including allocation failure
//! while each of them is created.

mod cov_g3_support;
use cov_g3_support::{assert_true, heap_limit_sweep, instruction_budget_sweep};

#[test]
fn mapped_and_unmapped_arguments_objects() {
    for source in [
        "function f(a, b) { arguments[0] = 'x'; b = 'y'; return a === 'x' && arguments[1] === 'y' && arguments.length === 2 }
         f(1, 2)",
        "function f(a) { return arguments.callee === f && arguments.length === 3 } f(1, 2, 3)",
        "function f(a, a2) { 'use strict'; arguments[0] = 'x'; return a === 1 && arguments.length === 2 } f(1, 2)",
        "function f(a = 1) { arguments[0] = 'x'; return a === 5 } f(5)",
        "function f() { 'use strict'; try { return arguments.callee } catch (e) { return e instanceof TypeError } } f()",
        "function f() { 'use strict'; const d = Object.getOwnPropertyDescriptor(arguments, 'callee');
                        return d.get === d.set && typeof d.get === 'function' && !d.configurable && Object.isFrozen(d.get) } f()",
        "function f() { return typeof arguments[Symbol.iterator] === 'function' && [...arguments].join() === '1,2' } f(1, 2)",
        "function f(a) { return Object.prototype.toString.call(arguments) === '[object Arguments]' } f()",
        "(function (a, b) { return arguments.length })() === 0",
        "const g = (a) => typeof arguments; typeof g(1) === 'string'",
    ] {
        assert_true(source);
    }
}

#[test]
fn legacy_function_caller_and_arguments() {
    for source in [
        "function callee() { return callee.caller } function caller() { return callee() }
         caller() === caller",
        "function callee() { return callee.caller } callee() === null",
        "function callee() { return callee.caller } function strict() { 'use strict'; return callee() }
         strict() === null",
        "function callee() { return callee.caller } function* gen() { yield callee() }
         gen().next().value === null",
        "function callee() { return callee.caller } async function asyncCaller() { return callee() }
         let result; asyncCaller().then((v) => { result = v }); true",
        "function callee() { return callee.caller } class K { constructor() { this.v = callee() } }
         new K().v === null",
        "function f() { return f.arguments } const args = f(1, 2); args.length === 2 && args[0] === 1 && args.callee === f",
        "function f() { return f.arguments } f.arguments === null",
        "function f() { return g() } function g() { return f.arguments } f(1) === null",
        "const d = Object.getOwnPropertyDescriptor(function () {}, 'caller');
         typeof d.get === 'function' && d.set === undefined && !d.configurable",
        "'use strict'; const d = Object.getOwnPropertyDescriptor(function () {}, 'caller'); d === undefined",
        "function f() {} const desc = Object.getOwnPropertyDescriptor(f, 'arguments');
         typeof desc.get === 'function' && desc.get === Object.getOwnPropertyDescriptor(f, 'arguments').get",
        "function f() { return Object.getOwnPropertyDescriptor(f, 'caller').get.call(1) } f() === null",
        "Object.getOwnPropertyDescriptor(function () {}, 'arguments').get.call(undefined) === null",
    ] {
        assert_true(source);
    }
}

#[test]
fn direct_and_indirect_eval_report_context_errors() {
    for source in [
        "globalThis.x = 1; eval('x + 1') === 2 && eval(5) === 5 && (0, eval)('typeof x') === 'number' && (0, eval)(6) === 6",
        "try { eval('super()'); false } catch (e) { e instanceof SyntaxError }",
        "try { eval('super.x'); false } catch (e) { e instanceof SyntaxError }",
        "class K { x = eval('1 + 1'); } new K().x === 2",
        "class K { x = eval('arguments'); } try { new K(); false } catch (e) { e instanceof SyntaxError }",
        "class K { x = (() => eval('arguments'))(); } try { new K(); false } catch (e) { e instanceof SyntaxError }",
        "class B {} class D extends B { constructor() { eval('super()'); this.ok = true } } new D().ok === true",
        "class B {} class D extends B { x = eval('super()'); } try { new D(); false } catch (e) { e instanceof SyntaxError }",
        "const o = { m() { return eval('super.toString === Object.prototype.toString') } }; o.m()",
        "try { eval('class C { #x; m() { this.#y } }'); false } catch (e) { e instanceof SyntaxError }",
        "try { (0, eval)('class C { #x; m() { this.#y } }'); false } catch (e) { e instanceof SyntaxError }",
        "try { eval('let a; var a'); false } catch (e) { e instanceof SyntaxError }",
        "try { (0, eval)('}'); false } catch (e) { e instanceof SyntaxError }",
        "function f(a) { return eval('a + arguments.length') } f(1) === 2",
        "function f() { 'use strict'; eval('var leaked = 1'); return typeof leaked } f() === 'undefined'",
        "function f() { eval('var seen = 1'); return seen } f() === 1",
        "function outer() { const captured = 7; return function () { return eval('captured') } } outer()() === 7",
        "(0, eval)('var globalFromIndirect = 1'); globalFromIndirect === 1",
        "let e = eval; e('var viaAlias = 2') === undefined && viaAlias === 2",
    ] {
        assert_true(source);
    }
}

#[test]
fn array_literals_with_holes_spreads_and_iterators() {
    for source in [
        "const a = [1, , 3]; a.length === 3 && !(1 in a) && a[2] === 3",
        "const a = [...[1, 2], ...'ab', , 3]; a.length === 6 && a.join() === '1,2,a,b,,3'",
        "const a = [, , ]; a.length === 2",
        "try { [...5]; false } catch (e) { e instanceof TypeError }",
        "try { [1, ...{ [Symbol.iterator]() { throw new EvalError('i') } }]; false } catch (e) { e instanceof EvalError }",
        "const it = [1, 2][Symbol.iterator](); it.next().value === 1 && it[Symbol.toStringTag] === 'Array Iterator'
           && Object.getPrototypeOf(it).next.length === 0",
        "[].values() !== [].values() && Object.getPrototypeOf([].values()) === Object.getPrototypeOf([].keys())",
    ] {
        assert_true(source);
    }
}

#[test]
fn generator_and_async_intrinsics_link_up() {
    for source in [
        "const GeneratorFunction = Object.getPrototypeOf(function* () {}).constructor;
         const generatorPrototype = Object.getPrototypeOf(function* () {}).prototype;
         typeof GeneratorFunction === 'function' && GeneratorFunction.name === 'GeneratorFunction'
           && generatorPrototype[Symbol.toStringTag] === 'Generator'
           && typeof generatorPrototype.next === 'function' && typeof generatorPrototype.throw === 'function'
           && typeof generatorPrototype.return === 'function'",
        "const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;
         AsyncFunction.name === 'AsyncFunction' && AsyncFunction.length === 1
           && Object.getPrototypeOf(async function () {})[Symbol.toStringTag] === 'AsyncFunction'",
        "const proto = Object.getPrototypeOf(async function* () {});
         proto[Symbol.toStringTag] === 'AsyncGeneratorFunction' && proto.constructor.name === 'AsyncGeneratorFunction'
           && typeof proto.prototype.next === 'function'",
        "const asyncIteratorProto = Object.getPrototypeOf(Object.getPrototypeOf((async function* () {}).prototype));
         typeof asyncIteratorProto[Symbol.asyncIterator] === 'function'
           && asyncIteratorProto[Symbol.asyncIterator].call(5) === 5",
        "async function* g() { yield 1 } const it = g(); typeof it.next === 'function' && it[Symbol.asyncIterator]() === it",
        "function* g() { yield 1 } const it = g(); it.next().value === 1 && it.next().done === true",
    ] {
        assert_true(source);
    }
}

#[test]
fn arguments_generators_and_eval_survive_every_allocation_failure() {
    for (warm, script) in [
        (
            "0;",
            "function f(a, b) { arguments[0] = 1; return arguments.length } f(1, 2);
             function s() { 'use strict'; return arguments } s(1);
             function g() { return g.caller } g();
             function h() { return h.arguments } h(1);
             Object.getOwnPropertyDescriptor(function () {}, 'caller');",
        ),
        (
            "0;",
            "function* gen() { yield 1 } gen().next();
             async function* ag() { yield 1 } ag().next();
             (async function () { await 1 })();
             [1, , ...[2]][Symbol.iterator]().next();
             Object.getPrototypeOf(function* () {}).prototype;
             Object.getPrototypeOf(async function () {}).constructor;",
        ),
        (
            "0;",
            "function outer() { const captured = 1; return function inner(a) { return eval('captured + a') } }
             outer()(2); (0, eval)('1 + 1'); eval('var v = 1'); function k() { eval('var w = 2') } k();",
        ),
    ] {
        assert!(heap_limit_sweep(warm, script) > 0, "{script}");
    }
}

#[test]
fn a_throwing_array_iterator_lookup_fails_arguments_object_creation() {
    for source in [
        "Object.defineProperty(Array.prototype, Symbol.iterator, { get() { throw new EvalError('iter') }, configurable: true });
         try { (function () { return arguments })(); false } catch (e) { e instanceof EvalError }",
        "function f() { return f['arg' + 'uments'] }
         Object.defineProperty(Array.prototype, Symbol.iterator, { get() { throw new EvalError('iter') }, configurable: true });
         try { f(); false } catch (e) { e instanceof EvalError }",
        "function f(a, a) { arguments[0] = 'x'; return a } f(1, 2) === 2",
        "function callee() { return callee.caller } [0].map(callee)[0] === null",
        "function callee() { return callee.caller } function outer() { return callee.bind(null)() }
         outer() === null || outer() === outer",
        "function f(eval) { return eval('1') } try { f(5); false } catch (e) { e instanceof TypeError }",
        "for (const source of ['new.target', 'super.x', 'super()', 'import.meta', 'yield 1', 'await 1', '#x in {}']) {
           try { (0, eval)(source); throw 'no error for ' + source } catch (e) { if (!(e instanceof SyntaxError)) throw e }
         } true",
    ] {
        assert_true(source);
    }
}

#[test]
fn spreads_and_evals_survive_running_out_of_instructions_at_every_step() {
    for source in [
        "[...'abcdef', , ...[1, 2, 3]].length",
        "function f() { return arguments.length } f(1, 2, 3); eval('1 + 1'); (0, eval)('2 + 2')",
    ] {
        assert!(instruction_budget_sweep(source) > 0, "{source}");
    }
}

#[test]
fn html_like_comments_and_identifier_escapes_in_scripts() {
    for source in [
        "let x = 1; <!-- a comment to the end of the line
         x += 1;
         --> another one
         x === 2",
        "1;
         /* a block comment */ --> a comment after a block comment
         true",
        "for (const source of ['var \\\\u0031a', 'var a\\\\u0020b', 'var \\\\u{31}b', 'var \\\\u{110000}']) {
           try { eval(source); throw 'accepted ' + source } catch (e) { if (!(e instanceof SyntaxError)) throw e }
         } true",
        "var \\u0061b = 5; ab === 5",
    ] {
        assert_true(source);
    }
}

/// Spreading appends each element the iterator hands out to the array being
/// built. An allocation failure there is reached only when the iterator makes
/// no objects of its own per step (each result here was made beforehand) and
/// the element is the largest thing any step stores.
#[test]
fn spreading_an_iterable_survives_every_allocation_failure_while_appending() {
    let warm = "globalThis.big = { value: 'x'.repeat(2000), done: false };
                globalThis.end = { value: undefined, done: true };
                globalThis.n = 0;
                globalThis.iterable = { [Symbol.iterator]() { n = 0; return { next() { return n++ < 3 ? big : end } } } };";
    assert!(heap_limit_sweep(warm, "[...iterable]") > 0);
    assert_true(&format!(
        "{warm} [...iterable].length === 3 && [...iterable].every((item) => item.length === 2000)"
    ));
}
