// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Operators (arithmetic, bitwise, shift, relational, equality, `in`) with
//! abrupt and mixed-type operands, the String helpers built on them, and the
//! `with` statement's object environment records.

mod cov_g3_support;
use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm};
use cov_g3_support::{assert_true, heap_limit_sweep, instruction_budget_sweep, run};

#[test]
fn every_binary_operator_propagates_abrupt_operand_conversion() {
    for op in [
        "-", "*", "/", "%", "&", "|", "^", "<<", ">>", ">>>", "**", "<", ">", "<=", ">=", "+",
        "==", "!=",
    ] {
        for (left, right) in [
            ("{ valueOf() { throw 7 } }", "1"),
            ("1", "{ valueOf() { throw 7 } }"),
        ] {
            assert_true(&format!(
                "try {{ ({left}) {op} ({right}); false }} catch (e) {{ e === 7 }}"
            ));
        }
    }
    for source in [
        "try { -({ valueOf() { throw 7 } }); false } catch (e) { e === 7 }",
        "try { ~({ valueOf() { throw 7 } }); false } catch (e) { e === 7 }",
        "try { let o = { valueOf() { throw 7 } }; o++; false } catch (e) { e === 7 }",
        "try { let o = { valueOf() { throw 7 } }; --o; false } catch (e) { e === 7 }",
        "try { Symbol() + 1; false } catch (e) { e instanceof TypeError }",
        "try { 1 + Symbol(); false } catch (e) { e instanceof TypeError }",
        "try { 1n + Symbol(); false } catch (e) { e instanceof TypeError }",
        "try { Symbol() < 1; false } catch (e) { e instanceof TypeError }",
        "try { 1 < Symbol(); false } catch (e) { e instanceof TypeError }",
    ] {
        assert_true(source);
    }
}

#[test]
fn mixing_bigint_and_number_is_a_type_error_except_for_comparison() {
    for op in [
        "-", "*", "/", "%", "&", "|", "^", "<<", ">>", ">>>", "**", "+",
    ] {
        assert_true(&format!(
            "try {{ 1n {op} 1; false }} catch (e) {{ e instanceof TypeError }}"
        ));
        assert_true(&format!(
            "try {{ 1 {op} 1n; false }} catch (e) {{ e instanceof TypeError }}"
        ));
    }
    for source in [
        "1n < 2 && 2 > 1n && 1n <= 1 && 1 >= 1n && 1n == 1 && 1 == 1n && 1n != 2 && '1' == 1n && 1n == '1'",
        "!(1n < NaN) && !(NaN < 1n) && !(1n == 'x') && 2n > '1' && '3' > 2n",
        "1n <= true && true >= 0n && 0n == false && false == 0n && 1n == true",
    ] {
        assert_true(source);
    }
}

#[test]
fn bigint_arithmetic_shifts_and_exponentiation_edges() {
    for source in [
        "5n / 2n === 2n && 5n % 2n === 1n && -5n % 2n === -1n && 5n - 2n === 3n && 5n * 2n === 10n",
        "try { 1n / 0n; false } catch (e) { e instanceof RangeError }",
        "try { 1n % 0n; false } catch (e) { e instanceof RangeError }",
        "(5n & 3n) === 1n && (5n | 3n) === 7n && (5n ^ 3n) === 6n && ~5n === -6n && -(5n) === -5n",
        "1n << 3n === 8n && 8n >> 1n === 4n && 1n << -1n === 0n && 1n >> -3n === 8n && -8n >> 1n === -4n",
        "1n >> (2n ** 70n) === 0n && -1n >> (2n ** 70n) === -1n",
        "try { 1n << (2n ** 70n); false } catch (e) { e instanceof RangeError }",
        "try { 1n >>> 1n; false } catch (e) { e instanceof TypeError }",
        "try { 2n ** -1n; false } catch (e) { e instanceof RangeError }",
        "0n ** 0n === 1n && 0n ** 5n === 0n && 1n ** (2n ** 70n) === 1n && 2n ** 10n === 1024n && (-1n) ** 3n === -1n",
        "try { 2n ** (2n ** 40n); false } catch (e) { e instanceof RangeError }",
        "try { (-1n) ** (2n ** 70n); false } catch (e) { e instanceof RangeError }",
    ] {
        assert_true(source);
    }
}

#[test]
fn number_operators_follow_ieee_and_int32_rules() {
    for source in [
        "5 - 2 === 3 && 5 * 2 === 10 && 5 / 2 === 2.5 && 5 % 3 === 2 && -5 % 3 === -2 && 2 ** 10 === 1024",
        "(5 & 3) === 1 && (5 | 3) === 7 && (5 ^ 3) === 6 && ~5 === -6 && (1 << 31) === -2147483648",
        "(-8 >> 1) === -4 && (-1 >>> 0) === 4294967295 && (1 << 33) === 2 && (8 >>> 33) === 4",
        "1 ** NaN !== 1 ** NaN && (-1) ** Infinity !== (-1) ** Infinity",
        "'3' * '4' === 12 && '3' - 1 === 2 && null + 1 === 1 && undefined + 1 !== undefined + 1 && true + true === 2",
        "'a' + 1 === 'a1' && 1 + 'a' === '1a' && [] + {} === '[object Object]' && 1 + 2 + '3' === '33'",
        "let i = 1; i++ === 1 && i === 2 && --i === 1 && (i += 2) === 3",
        "let big = 1n; big++; big === 2n && (big--, big) === 1n",
    ] {
        assert_true(source);
    }
}

#[test]
fn string_concatenation_beyond_the_limit_is_an_engine_error() {
    let result = run("const s = 'x'.repeat(700000); s + s");
    assert!(matches!(result, Err(RuntimeError::StringLimit { .. })));
}

#[test]
fn loose_equality_covers_every_type_pair() {
    for source in [
        "null == undefined && undefined == null && !(null == 0) && !(undefined == 0) && !(null == false)",
        "1 == '1' && '1' == 1 && !(1 == 'x') && true == 1 && 1 == true && false == '' && '' == false && '0' == false",
        "true == '1' && '1' == true && !(true == '2') && !('2' == true)",
        "({ valueOf() { return 1 } }) == 1 && 1 == ({ valueOf() { return 1 } }) && ({ toString() { return 'a' } }) == 'a'",
        "Symbol.iterator == Symbol.iterator && !(Symbol('a') == Symbol('a')) && (() => { const s = Symbol(); return Object(s) == s })()",
        "({ valueOf() { return 1n } }) == 1n && 1n == ({ valueOf() { return 1n } })",
        "!({}) == false && [] == '' && [1] == 1 && !({} == {}) && !(null == {})",
        "1n == 1n && !(1n == 2n) && 'a' == 'a' && !('a' == 'b')",
        "try { ({ valueOf() { throw 8 } }) == 1; false } catch (e) { e === 8 }",
        "try { 1 == ({ valueOf() { throw 8 } }); false } catch (e) { e === 8 }",
    ] {
        assert_true(source);
    }
}

#[test]
fn a_document_all_like_object_is_loosely_equal_to_null_and_undefined() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    vm.install_test262_is_html_dda().unwrap();
    for source in [
        "null == $262.IsHTMLDDA && $262.IsHTMLDDA == null && undefined == $262.IsHTMLDDA && $262.IsHTMLDDA == undefined",
        "!$262.IsHTMLDDA && !($262.IsHTMLDDA == 0) && typeof $262.IsHTMLDDA === 'undefined'",
    ] {
        let code = compile(&parse(source).unwrap()).unwrap();
        assert_eq!(vm.execute(&code), Ok(Value::Bool(true)), "{source}");
    }
}

#[test]
fn the_in_operator_checks_its_operands() {
    for source in [
        "'a' in { a: 1 } && !('b' in { a: 1 }) && 1 in [0, 1] && Symbol.iterator in []",
        "try { 'a' in 1; false } catch (e) { e instanceof TypeError }",
        "try { 'a' in 'string'; false } catch (e) { e instanceof TypeError }",
        "try { ({ toString() { throw 5 } }) in {}; false } catch (e) { e === 5 }",
        "const log = []; 'k' in new Proxy({}, { has(t, k) { log.push(k); return true } }) && log.join() === 'k'",
        "try { 'k' in new Proxy({}, { has() { throw 6 } }); false } catch (e) { e === 6 }",
    ] {
        assert_true(source);
    }
}

#[test]
fn string_helpers_convert_and_bound_their_inputs() {
    for source in [
        // String.raw
        "String.raw`a${1}b${2}c` === 'a1b2c' && String.raw({ raw: { length: 2, 0: 'x', 1: 'y' } }, 'S') === 'xSy'
           && String.raw({ raw: { length: 0 } }) === '' && String.raw({ raw: ['a', 'b', 'c'] }, 1) === 'a1bc'",
        "try { String.raw(); false } catch (e) { e instanceof TypeError }",
        "try { String.raw({}); false } catch (e) { e instanceof TypeError }",
        "try { String.raw({ get raw() { throw 1 } }); false } catch (e) { e === 1 }",
        "try { String.raw({ raw: { get length() { throw 2 } } }); false } catch (e) { e === 2 }",
        "try { String.raw({ raw: { length: 1, get 0() { throw 3 } } }); false } catch (e) { e === 3 }",
        "try { String.raw({ raw: { length: 1, 0: { toString() { throw 4 } } } }); false } catch (e) { e === 4 }",
        "try { String.raw({ raw: { length: 2, 0: 'a', 1: 'b' } }, { toString() { throw 5 } }); false } catch (e) { e === 5 }",
        "try { const big = 'x'.repeat(600000); String.raw({ raw: { length: 3, 0: big, 1: big, 2: '' } }, 'y'); false }
         catch (e) { true }",
        // split
        "'a,b,c'.split(',').join('|') === 'a|b|c' && 'a,b,c'.split(',', 2).length === 2 && 'abc'.split('').length === 3",
        "'ab'.split(undefined).length === 1 && 'a'.split('abcd').length === 1 && 'abc'.split('b', 0).length === 0",
        "'a1b1c'.split(1).join() === 'a,b,c' && ''.split('').length === 0 && ''.split('x').length === 1",
        "try { 'a'.split({ [Symbol.split]: 1 }); false } catch (e) { e instanceof TypeError }",
        "try { 'a'.split({ get [Symbol.split]() { throw 1 } }); false } catch (e) { e === 1 }",
        "try { 'a'.split({ toString() { throw 2 } }); false } catch (e) { e === 2 }",
        "try { 'a'.split(',', { valueOf() { throw 3 } }); false } catch (e) { e === 3 }",
        "try { String.prototype.split.call(null, ','); false } catch (e) { e instanceof TypeError }",
        "'x'.split({ [Symbol.split](s, l) { return [s, l] } }, 9).join() === 'x,9'",
        // replace / replaceAll
        "'aXbX'.replace('X', '-') === 'a-bX' && 'aXbX'.replaceAll('X', '-') === 'a-b-' && 'abc'.replace('z', 'y') === 'abc'",
        "'abc'.replace('b', (m, p, s) => m + p + s) === 'ab1abcc' && 'aXa'.replaceAll('a', () => 'b') === 'bXb'",
        "'abc'.replaceAll('', '-') === '-a-b-c-' && 'abc'.replace('', '-') === '-abc'",
        "'$&'.replace('$&', '[$&]') === '[$&]' && 'abc'.replace('b', '$`|$\\'|$$') === 'aa|c|$c'",
        "try { 'a'.replaceAll(/a/, 'b'); false } catch (e) { e instanceof TypeError }",
        "try { 'a'.replace({ get [Symbol.replace]() { throw 1 } }, 'b'); false } catch (e) { e === 1 }",
        "try { 'a'.replace({ toString() { throw 2 } }, 'b'); false } catch (e) { e === 2 }",
        "try { 'a'.replace('a', { toString() { throw 3 } }); false } catch (e) { e === 3 }",
        "try { 'a'.replace('a', () => { throw 4 }); false } catch (e) { e === 4 }",
        "try { 'a'.replace('a', () => ({ toString() { throw 5 } })); false } catch (e) { e === 5 }",
        "try { String.prototype.replace.call(undefined, 'a', 'b'); false } catch (e) { e instanceof TypeError }",
        "'x'.replace({ [Symbol.replace](s, r) { return s + r } }, 'R') === 'xR'",
        "try { const big = 'x'.repeat(400000); (big + 'a').replaceAll('a', big + big); false } catch (e) { true }",
        "'aaa'.replaceAll('a', 'bb') === 'bbbbbb' && 'abc'.replace('abcd', 'x') === 'abc'",
    ] {
        assert_true(source);
    }
}

#[test]
fn with_statements_resolve_names_through_objects_unscopables_and_proxies() {
    for source in [
        "const o = { x: 1 }; with (o) { x = 2; } o.x === 2",
        "const o = { x: 1 }; let r; with (o) { r = x } r === 1",
        "let r; with ({}) { r = typeof missingName } r === 'undefined'",
        "try { with ({}) { missingName } false } catch (e) { e instanceof ReferenceError }",
        "try { with ({}) { missingName() } false } catch (e) { e instanceof ReferenceError }",
        "const o = { f() { return this === o } }; let r; with (o) { r = f() } r",
        "const o = { x: 1, [Symbol.unscopables]: { x: true } }; var x = 'outer'; let r; with (o) { r = x } r === 'outer'",
        "const o = { x: 1, [Symbol.unscopables]: { x: false } }; let r; with (o) { r = x } r === 1",
        "const o = { x: 1, [Symbol.unscopables]: 5 }; let r; with (o) { r = x } r === 1",
        "try { with ({ x: 1, get [Symbol.unscopables]() { throw 1 } }) { x } false } catch (e) { e === 1 }",
        "try { with ({ x: 1, [Symbol.unscopables]: { get x() { throw 2 } } }) { x } false } catch (e) { e === 2 }",
        "try { with (new Proxy({}, { has() { throw 3 } })) { x } false } catch (e) { e === 3 }",
        "try { with (new Proxy({}, { has() { return true }, get() { throw 4 } })) { x } false } catch (e) { e === 4 }",
        "let seen; with (5) { seen = toFixed === Number.prototype.toFixed } seen",
        "try { with (null) { } false } catch (e) { e instanceof TypeError }",
        // The binding vanishes between lookup and read or write.
        "let calls = 0; const p = new Proxy({ x: 1 }, { has(t, k) { return k === 'x' ? calls++ === 0 : false } });
         let r; with (p) { r = x } r === undefined",
        "let calls = 0; const p = new Proxy({ x: 1 }, { has(t, k) { return k === 'x' ? calls++ === 0 : false } });
         with (p) { (function () { 'use strict'; try { x; return false } catch (e) { return e instanceof ReferenceError } })() }",
        // Compound assignment, update, delete and typeof through a with reference.
        "const o = { x: 1 }; with (o) { x += 2; x++; --x; } o.x === 3",
        "const o = { x: 1 }; let r; with (o) { r = delete x } r === true && !('x' in o)",
        "try { with ({}) { tdzLet += 1 } false } catch (e) { e instanceof ReferenceError } let tdzLet = 1;",
        "let assigned = 1; with ({}) { assigned += 1 } assigned === 2",
        "with ({}) { notYetGlobal = 5 } notYetGlobal === 5",
        "with ({}) { (function () { 'use strict'; try { unresolvableStrict = 1; return false } catch (e) { return e instanceof ReferenceError } })() }",
        "with ({}) { (0, eval)('var fromIndirect = 1') } fromIndirect === 1",
        "let r; with ({ y: 1 }) { r = eval('y') } r === 1",
        "try { with ({}) { eval('\"use strict\"; undeclared2 = 1') } false } catch (e) { e instanceof ReferenceError }",
        "with ({ z: 1 }) { eval('var fromWithEval = z + 1') } fromWithEval === 2",
        "let setter = 0; const o = { set s(v) { setter = v } }; with (o) { s = 4 } setter === 4",
        "try { with ({ set s(v) { throw 9 } }) { s = 1 } false } catch (e) { e === 9 }",
        "try { with (new Proxy({ s: 1 }, { set() { throw 10 } })) { s = 1 } false } catch (e) { e === 10 }",
        "try { with ({ get g() { throw 11 } }) { g += 1 } false } catch (e) { e === 11 }",
        "var globalSetterLog = []; Object.defineProperty(globalThis, 'gs', { set(v) { globalSetterLog.push(v) }, get() { return 0 }, configurable: true });
         with ({}) { gs = 5 } globalSetterLog.join() === '5'",
        "Object.defineProperty(globalThis, 'gg', { get() { throw 12 }, configurable: true });
         try { with ({}) { gg } false } catch (e) { e === 12 }",
    ] {
        assert_true(source);
    }
}

#[test]
fn with_reads_and_writes_parameter_environment_eval_variables() {
    for source in [
        "function f(a = eval('var b = 2'), c = () => b) { return c() } f() === 2",
        "function f(a = eval('var b = 2'), c = () => { b = 5; return b }) { return c() } f() === 5",
        "function f(x = eval('var y = 1'), z = eval('y + 1')) { return z } f() === 2",
        "function f(x = eval('var y = 1'), z = eval('y = 3; y')) { return z + y } f() === 6",
        "function f(a = eval('var b = 1'), c = eval('typeof b')) { return c } f() === 'number'",
        "function f(a = eval('var b = 1'), c = eval('delete b')) { return c } f() === true",
        "function f(a = eval('var b = 1'), c = (() => { with ({}) { return b } })()) { return c } f() === 1",
        "function f(a = eval('var b = 1'), c = (() => { with ({}) { b = 4; return b } })()) { return c } f() === 4",
        "function f(a = eval('var b = 1'), c = (() => { with ({}) { return b++ } })()) { return c } f() === 1",
        "function f(a = eval('var b = 1'), c = (() => { with ({}) { b += 2; return b } })()) { return c } f() === 3",
    ] {
        assert_true(source);
    }
}

#[test]
fn operators_and_with_survive_every_allocation_failure() {
    let script = "
        let n = 0;
        try { ({ valueOf() { throw 1 } }) - 1 } catch (e) { n++ }
        try { 1n + 1 } catch (e) { n++ }
        1n << 3n; 2n ** 10n; 5 & 3; 5 >>> 1; '1' == 1; ({ valueOf() { return 1 } }) == 1;
        'a,b'.split(','); 'aXb'.replace('X', () => '-'); String.raw`a${1}b`;
        'k' in { k: 1 };
        const o = { x: 1, [Symbol.unscopables]: { y: true } };
        with (o) { x += 1; n = x; delete x; }
        function f(a = eval('var b = 1'), c = () => b) { return c() } f();
        with ({}) { globalCreated = 1 }";
    assert!(heap_limit_sweep("0;", script) > 0);
}

#[test]
fn more_conversion_and_reference_errors_surface_unchanged() {
    for source in [
        "try { String.raw({ raw: { length: { valueOf() { throw 7 } } } }); false } catch (e) { e === 7 }",
        "try { String.prototype.split.call({ toString() { throw 1 } }, ','); false } catch (e) { e === 1 }",
        "try { String.prototype.replace.call({ toString() { throw 2 } }, 'a', 'b'); false } catch (e) { e === 2 }",
        "try { const o = { x: { valueOf() { throw 3 } } }; o.x++; false } catch (e) { e === 3 }",
        "try { const o = { x: { valueOf() { throw 4 } } }; --o['x']; false } catch (e) { e === 4 }",
        "const o = { x: 1n }; o.x++; o.x === 2n && (--o.x, o.x === 1n)",
        // A binding that exists in the enclosing scope but is read-only.
        "const c = 1; try { with ({}) { c = 2 } false } catch (e) { e instanceof TypeError }",
        "const c = 1; try { with ({}) { c++ } false } catch (e) { e instanceof TypeError }",
        // The binding vanishes before the write, or a global setter throws.
        "let calls = 0; const p = new Proxy({ x: 1 }, { has(t, k) { if (k === 'x' && calls++ === 0) return true; throw 5 } });
         try { with (p) { x = 1 } false } catch (e) { e === 5 }",
        "let calls = 0; const p = new Proxy({ x: 1 }, { has(t, k) { if (k === 'x' && calls++ === 0) return true; throw 6 } });
         try { with (p) { x } false } catch (e) { e === 6 }",
        "Object.defineProperty(globalThis, 'gset', { set() { throw 9 }, get() { return 0 }, configurable: true });
         try { with ({}) { gset = 1 } false } catch (e) { e === 9 }",
        "try { with ({}) { (0, eval)('var viaIndirect = 1') }; viaIndirect === 1 } catch (e) { false }",
    ] {
        assert_true(source);
    }
}

#[test]
fn oversized_results_are_engine_errors_not_truncations() {
    for source in [
        // The literals alone, then a substitution, exceed the string limit.
        "const big = 'x'.repeat(300000); String.raw({ raw: { length: 2, 0: big, 1: big } }, '')",
        "const big = 'x'.repeat(300000); String.raw({ raw: { length: 2, 0: big, 1: '' } }, big)",
        // Every replacement fits, but the tail after the last one does not.
        "const source = 'a' + 'x'.repeat(400000); source.replace('a', 'y'.repeat(200000))",
        // Replacements accumulate beyond the limit.
        "'a'.repeat(300000).replaceAll('a', 'bb')",
        // A single expanded replacement exceeds it.
        "const s = 'a'.repeat(200000); s.replace(s, '$&$&$&')",
        "const s = 'a'.repeat(300000); s + s",
    ] {
        assert!(
            matches!(run(source), Err(RuntimeError::StringLimit { .. })),
            "{source}"
        );
    }
}

#[test]
fn a_document_all_like_object_is_loosely_equal_only_to_nullish_values() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    vm.install_test262_is_html_dda().unwrap();
    // The operands come through parameters so that nothing is folded away.
    let source = "function eq(a, b) { return a == b }
                  const d = $262.IsHTMLDDA;
                  eq(d, null) && eq(null, d) && eq(d, undefined) && eq(undefined, d)
                    && !eq(d, 0) && !eq(0, d) && !eq(d, '') && !eq('', d)";
    let code = compile(&parse(source).unwrap()).unwrap();
    assert_eq!(vm.execute(&code), Ok(Value::Bool(true)));
}

#[test]
fn operators_survive_running_out_of_instructions_at_every_step() {
    for source in [
        "'a'.repeat(50).replaceAll('a', 'b'); 'a,b,c,d'.split(','); String.raw`a${1}b${2}c`;",
        "let s = 0; for (const x of [1, 2, 3]) { s += x } s",
    ] {
        assert!(instruction_budget_sweep(source) > 0, "{source}");
    }
}

#[test]
fn operations_fail_cleanly_in_the_smallest_script_that_makes_each_allocation() {
    // A heap ceiling only fails an allocation that is the largest demand made
    // so far, so each late allocation is swept in a script of its own.
    let warm = "0;";
    cov_g3_support::sweep_each(&[
        (warm, "'a,b'.split(',')"),
        (warm, "'a,b,c'.split(',', 2)"),
        (warm, "'abc'.split('')"),
        (warm, "'aXb'.replace('X', () => '-')"),
        (warm, "'aXb'.replaceAll('X', '$&$&')"),
        (warm, "String.raw`a${1}b`"),
        (warm, "with ({ x: 1 }) { x += 1 }"),
        (warm, "with ({}) { globalCreated = 1 }"),
        (warm, "eval('1')"),
        (warm, "(0, eval)('1')"),
        (warm, "[...'ab']"),
        (warm, "[, 1]"),
        (warm, "[1, , ...[2]]"),
        (warm, "function f() { return f.arguments } f(1)"),
        (warm, "function f(a) { return arguments } f(1)"),
        (warm, "function f() { 'use strict'; return arguments } f(1)"),
    ]);
}

#[test]
fn a_with_block_assigns_to_globals_declared_by_earlier_scripts() {
    let mut vm = Vm::default();
    let run =
        |vm: &mut Vm, source: &str| vm.execute_script(&compile(&parse(source).unwrap()).unwrap());
    run(&mut vm, "const fixed = 1; let loose = 1; var plain = 1;").unwrap();
    assert_eq!(
        run(&mut vm, "with ({}) { loose = 2; plain = 3 } loose + plain"),
        Ok(Value::Number(5.0))
    );
    assert_eq!(
        run(
            &mut vm,
            "try { with ({}) { fixed = 2 } false } catch (e) { e instanceof TypeError }"
        ),
        Ok(Value::Bool(true))
    );
}

/// A replacement pass that has already produced a result close to the string
/// limit fails on the untouched text between two matches, before the next
/// replacement is even appended.
#[test]
fn replace_all_reports_a_result_that_outgrows_the_string_limit_between_matches() {
    let mut vm = Vm::new(blueice_bluejs::VmConfig {
        max_string_bytes: 64,
        ..blueice_bluejs::VmConfig::default()
    })
    .unwrap();
    let outcome = vm.execute(
        &compile(&parse("('a' + 'b'.repeat(10) + 'a').replaceAll('a', 'Z'.repeat(30))").unwrap())
            .unwrap(),
    );
    assert_eq!(outcome, Err(RuntimeError::StringLimit { limit: 64 }));
}

/// The assignment that ends a `with` reference that could not be resolved
/// stores into a variable `eval` created in the meantime.
#[test]
fn storing_into_an_eval_variable_created_after_the_reference_was_resolved() {
    assert_true(
        "(function () {
           with ({}) { missing = (eval('var missing = 1'), 'x'.repeat(4)) }
           return missing === 'xxxx'
         })()",
    );
    let warm = "globalThis.f = function () { with ({}) { z = (eval('var z = 1'), 'x'.repeat(4000)) } }; f;";
    assert!(heap_limit_sweep(warm, "f()") > 0);
    assert!(heap_limit_sweep("0;", "with ({}) { Math }") > 0);
}
