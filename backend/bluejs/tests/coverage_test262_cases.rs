// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public-API coverage for the native Test262 host helpers (`print`,
//! `setTimeout`, `assert.*`, `compareArray`, `buildString`, the
//! property-escape/URI/RegExp/NumberFormat batch adapters, `$262.evalScript`).
//! Each test installs the harness explicitly and drives it with a small
//! purpose-written script, checking both the passing and the failing verdicts.

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm, VmConfig};

fn harness_vm() -> Vm {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    vm
}

fn run(vm: &mut Vm, source: &str) -> Result<Value, RuntimeError> {
    vm.execute(&compile(&parse(source).unwrap()).unwrap())
}

/// Evaluates `source` in a fresh harness realm and expects `true`.
fn expect_true(source: &str) {
    let mut vm = harness_vm();
    assert_eq!(run(&mut vm, source), Ok(Value::Bool(true)), "{source}");
}

fn expect_test262_failure(source: &str) {
    let mut vm = harness_vm();
    let result = run(&mut vm, source);
    assert!(
        matches!(result, Err(RuntimeError::Test262(_))),
        "{source}: {result:?}"
    );
}

fn expect_type_error(source: &str) {
    let mut vm = harness_vm();
    let result = run(&mut vm, source);
    assert!(
        matches!(result, Err(RuntimeError::TypeError(_))),
        "{source}: {result:?}"
    );
}

fn expect_range_error(source: &str) {
    let mut vm = harness_vm();
    let result = run(&mut vm, source);
    assert!(
        matches!(result, Err(RuntimeError::RangeError(_))),
        "{source}: {result:?}"
    );
}

#[test]
fn print_is_a_silent_no_op_and_assert_requires_the_boolean_true() {
    expect_true("print('ignored', 1) === undefined && assert(true) === undefined");
    expect_test262_failure("assert(1)");
    expect_test262_failure("assert('true')");
    expect_test262_failure("assert()");
}

#[test]
fn same_value_family_uses_the_same_value_algorithm() {
    expect_true(
        "assert.sameValue(NaN, NaN);\
         assert.sameValue(0, 0);\
         assert.notSameValue(0, -0);\
         assert.notSameValue(1, '1');\
         assert.notSameValue({}, {});\
         assert._isSameValue(NaN, NaN) === true && assert._isSameValue(0, -0) === false",
    );
    expect_test262_failure("assert.sameValue(0, -0)");
    expect_test262_failure("assert.sameValue(1, 2)");
    expect_test262_failure("assert.notSameValue(NaN, NaN)");
    expect_test262_failure("let o = {}; assert.notSameValue(o, o)");
}

#[test]
fn assert_throws_matches_each_native_error_family_by_constructor() {
    expect_true("assert.throws(TypeError, () => null.x); true");
    expect_true("assert.throws(RangeError, () => 'a'.repeat(-1)); true");
    expect_true("assert.throws(ReferenceError, () => notDeclaredAnywhere); true");
    expect_true("assert.throws(SyntaxError, () => new RegExp('(')); true");
    expect_true("assert.throws(Test262Error, () => assert(false)); true");
    expect_true("assert.throws(Test262Error, () => { throw new Test262Error('x'); }); true");
    expect_true("class E extends Error {}; assert.throws(E, () => { throw new E(); }); true");
}

#[test]
fn assert_throws_rejects_the_wrong_constructor_or_a_missing_throw() {
    expect_test262_failure("assert.throws(RangeError, () => null.x)");
    expect_test262_failure("assert.throws(TypeError, () => 'a'.repeat(-1))");
    expect_test262_failure("assert.throws(TypeError, () => notDeclaredAnywhere)");
    expect_test262_failure("assert.throws(TypeError, () => new RegExp('('))");
    expect_test262_failure("assert.throws(TypeError, () => assert(false))");
    expect_test262_failure("assert.throws(TypeError, () => { throw new RangeError(); })");
    // No exception at all.
    expect_test262_failure("assert.throws(TypeError, () => 1)");
    // A thrown primitive is never an instance of an error constructor.
    expect_test262_failure("assert.throws(TypeError, () => { throw 1; })");
    // The callback must itself be callable.
    expect_test262_failure("assert.throws(TypeError, 1)");
    expect_test262_failure("assert.throws(TypeError, {})");
}

#[test]
fn assert_throws_never_swallows_host_resource_failures() {
    let mut vm = Vm::new(VmConfig {
        instruction_budget: 20_000,
        ..VmConfig::default()
    })
    .unwrap();
    vm.install_test262_harness().unwrap();
    let result = run(&mut vm, "assert.throws(TypeError, () => { for (;;) {} })");
    assert_eq!(result, Err(RuntimeError::InstructionLimit));
}

#[test]
fn global_compare_array_reports_booleans_and_assert_compare_array_throws() {
    expect_true(
        "compareArray([], []) === true &&\
         compareArray([1, 'a', NaN], [1, 'a', NaN]) === true &&\
         compareArray([1, 2], [1, 3]) === false &&\
         compareArray([1], [1, 2]) === false &&\
         compareArray([0], [-0]) === false &&\
         compareArray({ length: 2, 0: 'x', 1: 'y' }, ['x', 'y']) === true",
    );
    expect_true(
        "assert.compareArray([1, 2, 3], [1, 2, 3]);\
         assert.compareArray([], []);\
         true",
    );
    expect_test262_failure("assert.compareArray([1, 2], [1, 3])");
    expect_test262_failure("assert.compareArray([1], [1, 2])");
    expect_test262_failure("assert.compareArray('ab', 'ab')");
    expect_test262_failure("assert.compareArray([], 'ab')");
    expect_test262_failure("assert.compareArray(undefined, [])");
}

#[test]
fn format_helpers_render_arrays_strings_and_negative_zero() {
    expect_true(
        "compareArray.format([]) === '[]' &&\
         compareArray.format([1, 'x', null, undefined]) === '[1, x, null, undefined]' &&\
         compareArray.format({ length: 2, 0: 'a', 1: 2 }) === '[a, 2]' &&\
         formatSimpleValue(-0) === '-0' &&\
         formatSimpleValue('s') === '\"s\"' &&\
         formatSimpleValue(12) === '12' &&\
         formatIdentityFreeValue(Symbol('q')) === undefined &&\
         formatIdentityFreeValue(null) === 'null' &&\
         isNegativeZero(-0) && !isNegativeZero(+0) && !isNegativeZero('-0') &&\
         isPrimitive('s') && isPrimitive(Symbol()) && !isPrimitive(function () {})",
    );
}

#[test]
fn format_helpers_respect_the_string_limit() {
    let mut vm = Vm::new(VmConfig {
        max_string_bytes: 8,
        ..VmConfig::default()
    })
    .unwrap();
    vm.install_test262_harness().unwrap();
    assert!(matches!(
        run(&mut vm, "formatSimpleValue('1234567')"),
        Err(RuntimeError::StringLimit { .. })
    ));
    assert!(matches!(
        run(&mut vm, "compareArray.format(['aaaa', 'bbbb', 'cccc'])"),
        Err(RuntimeError::StringLimit { .. })
    ));
}

#[test]
fn native_deep_equal_compares_nested_records_and_arrays() {
    expect_true(
        "assert.deepEqual(1, 1);\
         assert.deepEqual('a', 'a');\
         assert.deepEqual(NaN, NaN);\
         assert.deepEqual([], []);\
         assert.deepEqual([1, [2, [3]]], [1, [2, [3]]]);\
         assert.deepEqual({ a: 1, b: { c: [1, 2] } }, { b: { c: [1, 2] }, a: 1 });\
         assert.deepEqual([{ type: 'literal', value: 'x' }], [{ value: 'x', type: 'literal' }]);\
         let shared = { k: 1 };\
         assert.deepEqual([shared, shared], [shared, shared]);\
         true",
    );
}

#[test]
fn native_deep_equal_terminates_on_cyclic_structures() {
    expect_true(
        "let a = { v: 1 }; a.self = a;\
         let b = { v: 1 }; b.self = b;\
         assert.deepEqual(a, b);\
         true",
    );
}

#[test]
fn native_deep_equal_rejects_every_kind_of_mismatch() {
    for source in [
        "assert.deepEqual(1, 2)",
        "assert.deepEqual('a', 'b')",
        "assert.deepEqual(1, '1')",
        "assert.deepEqual(null, undefined)",
        "assert.deepEqual({}, 1)",
        "assert.deepEqual(1, {})",
        "assert.deepEqual([1, 2], [1, 3])",
        "assert.deepEqual([1, 2], [1])",
        "assert.deepEqual([], {})",
        "assert.deepEqual({}, [])",
        "assert.deepEqual({ a: 1 }, { a: 2 })",
        "assert.deepEqual({ a: 1 }, { b: 1 })",
        "assert.deepEqual({ a: 1 }, { a: 1, b: 2 })",
        "assert.deepEqual([{ a: [1] }], [{ a: [2] }])",
    ] {
        expect_test262_failure(source);
    }
}

#[test]
fn is_constructor_distinguishes_constructors_and_rejects_non_callables() {
    expect_true(
        "isConstructor(function () {}) && isConstructor(class {}) && isConstructor(Array) &&\
         !isConstructor(() => {}) && !isConstructor(Math.abs) && !isConstructor(async function () {})",
    );
    expect_test262_failure("isConstructor({})");
    expect_test262_failure("isConstructor(undefined)");
}

#[test]
fn build_string_concatenates_lone_code_points_then_ranges() {
    expect_true(
        "let s = buildString({ loneCodePoints: [0x41, 0x1f600], ranges: [[0x61, 0x63], [0x10000, 0x10002]] });\
         s.length === 1 + 2 + 3 + 3 * 2 &&\
         s.codePointAt(0) === 0x41 && s.codePointAt(1) === 0x1f600 &&\
         s.slice(3, 6) === 'abc' && Array.from(s).length === 1 + 1 + 3 + 3",
    );
    expect_true(
        "buildString({ loneCodePoints: [], ranges: [] }) === '' &&\
         buildString({ loneCodePoints: [0xd800], ranges: [] }).charCodeAt(0) === 0xd800 &&\
         buildString({ loneCodePoints: [], ranges: [[0x30, 0x30]] }) === '0'",
    );
}

#[test]
fn build_string_validates_code_points_and_ranges() {
    for source in [
        "buildString({ loneCodePoints: [-1], ranges: [] })",
        "buildString({ loneCodePoints: [1.5], ranges: [] })",
        "buildString({ loneCodePoints: [0x110000], ranges: [] })",
        "buildString({ loneCodePoints: [NaN], ranges: [] })",
        "buildString({ loneCodePoints: [Infinity], ranges: [] })",
        "buildString({ loneCodePoints: [], ranges: [[0, -1]] })",
        "buildString({ loneCodePoints: [], ranges: [[0, 0x110000]] })",
        "buildString({ loneCodePoints: [], ranges: [[5, 4]] })",
    ] {
        expect_range_error(source);
    }
    expect_type_error("buildString({ loneCodePoints: [], ranges: [[1]] })");
    expect_type_error("buildString({ loneCodePoints: [], ranges: [[]] })");
}

#[test]
fn build_string_enforces_the_runtime_string_limit() {
    let mut vm = Vm::new(VmConfig {
        max_string_bytes: 64,
        ..VmConfig::default()
    })
    .unwrap();
    vm.install_test262_harness().unwrap();
    assert!(matches!(
        run(
            &mut vm,
            "buildString({ loneCodePoints: [], ranges: [[0x100, 0x1ff]] })"
        ),
        Err(RuntimeError::StringLimit { .. })
    ));
    // A string that fits is still built normally after the failure.
    assert_eq!(
        run(
            &mut vm,
            "buildString({ loneCodePoints: [], ranges: [[0x61, 0x63]] }) === 'abc'"
        ),
        Ok(Value::Bool(true))
    );
}

#[test]
fn test_property_escapes_requires_the_regexp_to_accept_the_string() {
    expect_true(
        "testPropertyEscapes(/^\\p{Lu}+$/u, 'ABC', '\\\\p{Lu}') === undefined &&\
         testPropertyEscapes(/^\\P{Lu}+$/u, 'abc', '\\\\P{Lu}') === undefined",
    );
    expect_test262_failure("testPropertyEscapes(/^\\p{Lu}+$/u, 'abc', '\\\\p{Lu}')");
    expect_test262_failure("testPropertyEscapes(/^\\P{Lu}+$/u, 'ABC', '\\\\P{Lu}')");
}

#[test]
fn test_property_of_strings_checks_match_and_non_match_lists() {
    expect_true(
        "testPropertyOfStrings({ regExp: /^(?:a|bc)+$/, matchStrings: ['a', 'bc', 'abca'], nonMatchStrings: ['b', 'c', ''] }) === undefined &&\
         testExtendedCharacterClass({ regExp: /^[ab]+$/v, matchStrings: ['a', 'b'] }) === undefined &&\
         testPropertyOfStrings({ regExp: /^x$/, matchStrings: ['x'], nonMatchStrings: undefined }) === undefined",
    );
    // Joined matches pass as a whole, so an individually failing member is
    // only inspected when the concatenation itself is rejected.
    expect_true(
        "testPropertyOfStrings({ regExp: /^(?:a|b)+$/, matchStrings: ['a', 'b'], nonMatchStrings: ['c', 'd'] }) === undefined",
    );
    // Concatenated non-matches are rejected as a whole and therefore pass.
    expect_true(
        "testPropertyOfStrings({ regExp: /^a$/, matchStrings: ['a'], nonMatchStrings: ['b', 'c'] }) === undefined",
    );
}

#[test]
fn test_property_of_strings_reports_the_offending_string() {
    // The joined string fails and so does an individual member.
    expect_test262_failure("testPropertyOfStrings({ regExp: /^a$/, matchStrings: ['a', 'b'] })");
    // Joined non-matches are accepted by the RegExp, so each is re-tested.
    expect_test262_failure(
        "testPropertyOfStrings({ regExp: /^[ab]+$/, matchStrings: ['a'], nonMatchStrings: ['a', 'b'] })",
    );
    expect_test262_failure(
        "testExtendedCharacterClass({ regExp: /^[ab]$/v, matchStrings: ['c'] })",
    );
}

#[test]
fn eval_script_runs_source_text_and_reports_bad_input() {
    expect_true("$262.evalScript('1 + 2') === 3 && $262.evalScript('') === undefined");
    expect_type_error("$262.evalScript(1)");
    expect_type_error("$262.evalScript()");
    let mut vm = harness_vm();
    let result = run(&mut vm, "$262.evalScript('var =')");
    assert!(
        matches!(result, Err(RuntimeError::SyntaxError(_))),
        "{result:?}"
    );
    // A lone surrogate in the source text itself cannot be converted to UTF-8.
    let result = run(&mut vm, "$262.evalScript(String.fromCharCode(0xd800))");
    assert!(
        matches!(result, Err(RuntimeError::SyntaxError(_))),
        "{result:?}"
    );
}

#[test]
fn detach_array_buffer_requires_an_object() {
    expect_true(
        "let b = new ArrayBuffer(4); $262.detachArrayBuffer(b) === undefined && b.byteLength === 0",
    );
    expect_type_error("$262.detachArrayBuffer(1)");
    expect_type_error("$262.detachArrayBuffer()");
}

#[test]
fn set_timeout_validates_its_callback_and_schedules_a_host_timer() {
    expect_type_error("setTimeout(1, 0)");
    expect_type_error("setTimeout({}, 0)");
    expect_type_error("setTimeout()");
    for source in [
        "setTimeout(function () {}, 0) === 0",
        "setTimeout(function () {}) === 0",
        "setTimeout(function () {}, -5) === 0",
        "setTimeout(function () {}, NaN) === 0",
        "setTimeout(function () {}, Infinity) === 0",
        "setTimeout(function () {}, '1') === 0",
    ] {
        let mut vm = harness_vm();
        vm.install_test262_done().unwrap();
        assert_eq!(run(&mut vm, source), Ok(Value::Bool(true)), "{source}");
        // Drain the timer thread so it never outlives the test's VM.
        let _ = vm.run_test262_async_until_done();
    }
}

#[test]
fn set_timeout_delivers_the_callback_after_the_delay() {
    let mut vm = harness_vm();
    vm.install_test262_done().unwrap();
    // Each execution has fresh bindings, so the callback checks the order
    // itself and reports the verdict through $DONE.
    run(
        &mut vm,
        "var order = [];\
         setTimeout(function () {\
           order.push('t');\
           if (order.join(',') === 'sync,t') { $DONE(); } else { $DONE(new Error('order: ' + order)); }\
         }, 2);\
         order.push('sync');",
    )
    .unwrap();
    assert_eq!(vm.run_test262_async_until_done(), Ok(Some(Ok(()))));
}

#[test]
fn regexp_class_escape_validates_its_arguments_and_verdicts() {
    expect_true("__bluejsTest262RegExpClassEscape([/^\\d+$/], '123', true)");
    expect_true("__bluejsTest262RegExpClassEscape({ length: 1, 0: /^\\d+$/ }, 'abc', false)");
    expect_type_error("__bluejsTest262RegExpClassEscape([/a/], 'a', 1)");
    expect_type_error("__bluejsTest262RegExpClassEscape([/a/], 'a')");
    expect_type_error("__bluejsTest262RegExpClassEscape([], 'a', true)");
    expect_test262_failure("__bluejsTest262RegExpClassEscape([/a/, /b/], 'a', true)");
    expect_test262_failure("__bluejsTest262RegExpClassEscape([/a/], 'a', false)");
}

#[test]
fn typed_array_overlap_helper_rejects_a_non_object_target_and_a_non_zero_result() {
    expect_type_error("__bluejsTest262TypedArrayOverlappingSet(1, new Uint8Array(1))");
    // Setting non-zero source values leaves non-zero elements behind.
    expect_test262_failure(
        "__bluejsTest262TypedArrayOverlappingSet(new Uint8Array(4), new Uint8Array([1, 0, 0, 0]))",
    );
    expect_true(
        "__bluejsTest262TypedArrayOverlappingSet(new Uint8Array(4), new Uint8Array([0, 0, 0, 0]))",
    );
}

#[test]
fn uri_decode_helper_accepts_real_decoders_and_rejects_bad_arguments() {
    expect_true("__bluejsTest262DecodeUriExhaustive(decodeURI, 3)");
    expect_type_error("__bluejsTest262DecodeUriExhaustive(decodeURI, 2)");
    expect_type_error("__bluejsTest262DecodeUriExhaustive(decodeURI, '3')");
    expect_type_error("__bluejsTest262DecodeUriExhaustive(decodeURI)");
    expect_type_error("__bluejsTest262DecodeUriExhaustive(1, 3)");
    expect_type_error("__bluejsTest262DecodeUriExhaustive({}, 4)");
}

#[test]
fn uri_decode_helper_fails_when_the_decoder_disagrees_with_utf8() {
    // Three-octet and four-octet enumerations both stop at the first mismatch.
    expect_test262_failure("__bluejsTest262DecodeUriExhaustive(function () { return ''; }, 3)");
    expect_test262_failure("__bluejsTest262DecodeUriExhaustive(function () { return ''; }, 4)");
    expect_test262_failure("__bluejsTest262DecodeUriExhaustive(function (s) { return s; }, 4)");
    // A decoder that throws propagates its exception unchanged.
    expect_true(
        "let thrown; try { __bluejsTest262DecodeUriExhaustive(function () { throw new RangeError('x'); }, 4); } catch (e) { thrown = e; } thrown instanceof RangeError",
    );
}

#[test]
fn uri_decode_helper_visits_four_octet_sequences_in_order() {
    // The enumeration skips the overlong-prefixed second octets, so the first
    // four-octet sequence is U+20000 (F0 A0 80 80). After that the decoder is
    // asked for successive last-octet values: U+20001, U+20002...
    expect_true(
        "var seen = [];\
         try {\
           __bluejsTest262DecodeUriExhaustive(function (s) {\
             seen.push(s);\
             if (seen.length === 3) throw new RangeError('stop');\
             return decodeURIComponent(s);\
           }, 4);\
         } catch (e) { if (!(e instanceof RangeError)) throw e; }\
         seen.length === 3 && seen[0] === '%F0%A0%80%80' && seen[1] === '%F0%A0%80%81' && seen[2] === '%F0%A0%80%82'",
    );
    expect_true(
        "var seen = [];\
         try {\
           __bluejsTest262DecodeUriExhaustive(function (s) {\
             seen.push(s);\
             if (seen.length === 2) throw new RangeError('stop');\
             return decodeURI(s);\
           }, 3);\
         } catch (e) { if (!(e instanceof RangeError)) throw e; }\
         seen.length === 2 && seen[0] === '%E0%A0%80' && seen[1] === '%E0%A0%81'",
    );
}

#[test]
fn uri_encode_helper_checks_three_octet_bmp_ranges_against_the_real_encoder() {
    expect_true("__bluejsTest262EncodeUriExhaustive(encodeURI, 0x0800, 0x08ff)");
    expect_true("__bluejsTest262EncodeUriExhaustive(encodeURIComponent, 0xd7f0, 0xd7ff)");
    expect_true("__bluejsTest262EncodeUriExhaustive(encodeURI, 0xe000, 0xe010)");
    expect_true("__bluejsTest262EncodeUriExhaustive(encodeURI, 0x0800, 0x0800)");
}

#[test]
fn uri_encode_helper_rejects_bad_bounds_and_mismatching_encoders() {
    expect_type_error("__bluejsTest262EncodeUriExhaustive(encodeURI, '2048', 2050)");
    expect_type_error("__bluejsTest262EncodeUriExhaustive(encodeURI, 2048, '2050')");
    expect_type_error("__bluejsTest262EncodeUriExhaustive(encodeURI, 1.5, 2)");
    expect_type_error("__bluejsTest262EncodeUriExhaustive(encodeURI, -1, 5)");
    expect_type_error("__bluejsTest262EncodeUriExhaustive(encodeURI, 0, 0x10000)");
    expect_type_error("__bluejsTest262EncodeUriExhaustive(encodeURI, NaN, 5)");
    expect_type_error("__bluejsTest262EncodeUriExhaustive(encodeURI, 0x900, 0x800)");
    expect_type_error("__bluejsTest262EncodeUriExhaustive({}, 0x800, 0x801)");
    expect_type_error("__bluejsTest262EncodeUriExhaustive(encodeURI)");
    // U+0041 encodes to itself rather than to the three-octet form.
    expect_test262_failure("__bluejsTest262EncodeUriExhaustive(encodeURI, 0x41, 0x41)");
    expect_test262_failure(
        "__bluejsTest262EncodeUriExhaustive(function () { return 'nope'; }, 0x800, 0x801)",
    );
    // A lone surrogate makes the real encoder throw a URIError.
    expect_true(
        "let thrown; try { __bluejsTest262EncodeUriExhaustive(encodeURI, 0xd800, 0xd800); } catch (e) { thrown = e; } thrown instanceof URIError",
    );
}

#[test]
fn regexp_bmp_literal_helper_rejects_bad_variants() {
    expect_type_error("__bluejsTest262RegExpBmpLiteral('0')");
    expect_type_error("__bluejsTest262RegExpBmpLiteral()");
    expect_type_error("__bluejsTest262RegExpBmpLiteral(1.5)");
    expect_type_error("__bluejsTest262RegExpBmpLiteral(NaN)");
    expect_range_error("__bluejsTest262RegExpBmpLiteral(4)");
}

#[test]
fn regexp_bmp_literal_helper_validates_every_variant() {
    for variant in 0..=3 {
        expect_true(&format!("__bluejsTest262RegExpBmpLiteral({variant})"));
    }
}

#[test]
fn regexp_non_whitespace_helper_matches_the_whitespace_class_over_the_bmp() {
    expect_true("__bluejsTest262RegExpNonWhitespaceBmp()");
}

#[test]
fn number_format_matrix_checks_option_records_against_expected_output() {
    let expected = "{'1':'1','1.500':'1.5','1.625':'1.6','1.750':'1.8','1.875':'1.9','2.000':'2'}";
    expect_true(&format!(
        "__bluejsTest262NumberFormatPrecisionMatrix(['en-US'], ['latn'], {{ maximumFractionDigits: 1 }}, {expected}) === undefined"
    ));
    let padded =
        "{'1':'1.00','1.500':'1.50','1.625':'1.63','1.750':'1.75','1.875':'1.88','2.000':'2.00'}";
    expect_true(&format!(
        "__bluejsTest262NumberFormatPrecisionMatrix(['en-US'], ['latn'], {{ minimumFractionDigits: 2, maximumFractionDigits: 2 }}, {padded}) === undefined"
    ));
}

#[test]
fn number_format_matrix_localizes_digits_for_each_numbering_system() {
    let expected = "{'1':'1','1.500':'1.5','1.625':'1.6','1.750':'1.8','1.875':'1.9','2.000':'2'}";
    expect_true(&format!(
        "__bluejsTest262NumberFormatPrecisionMatrix(['en-US'], ['latn', 'arab', 'thai', 'hanidec'], {{ maximumFractionDigits: 1 }}, {expected}) === undefined"
    ));
    expect_true(&format!(
        "__bluejsTest262NumberFormatPrecisionMatrix(['en-US', 'de'], ['latn', 'thai'], {{ maximumFractionDigits: 1 }}, {expected}) === undefined"
    ));
}

#[test]
fn number_format_matrix_reports_bad_fixtures_as_test262_failures() {
    let expected = "{'1':'1','1.500':'1.5','1.625':'1.6','1.750':'1.8','1.875':'1.9','2.000':'2'}";
    // Unknown numbering system: no digit map exists for it.
    expect_test262_failure(&format!(
        "__bluejsTest262NumberFormatPrecisionMatrix(['en-US'], ['bogus'], {{ maximumFractionDigits: 1 }}, {expected})"
    ));
    // Expected data that disagrees with the formatter output.
    expect_test262_failure(
        "__bluejsTest262NumberFormatPrecisionMatrix(['en-US'], ['latn'], { maximumFractionDigits: 1 }, {'1':'1','1.500':'1.5','1.625':'WRONG','1.750':'1.8','1.875':'1.9','2.000':'2'})",
    );
    // A missing expected entry stringifies to `undefined`, which cannot match.
    expect_test262_failure(
        "__bluejsTest262NumberFormatPrecisionMatrix(['en-US'], ['latn'], { maximumFractionDigits: 1 }, {})",
    );
    // Expected values starting with '-' are checked against the negative pattern.
    expect_test262_failure(
        "__bluejsTest262NumberFormatPrecisionMatrix(['en-US'], ['latn'], { maximumFractionDigits: 1 }, {'1':'-1','1.500':'1.5','1.625':'1.6','1.750':'1.8','1.875':'1.9','2.000':'2'})",
    );
    // maximumFractionDigits: 0 formats 1.1 with a single digit run, so the
    // pattern probe cannot be split into two runs.
    expect_test262_failure(&format!(
        "__bluejsTest262NumberFormatPrecisionMatrix(['en-US'], ['latn'], {{ maximumFractionDigits: 0 }}, {expected})"
    ));
    // Lone surrogates cannot be converted to UTF-8 locale/numbering strings.
    expect_test262_failure(&format!(
        "__bluejsTest262NumberFormatPrecisionMatrix(['\\ud800'], ['latn'], {{ maximumFractionDigits: 1 }}, {expected})"
    ));
    expect_test262_failure(&format!(
        "__bluejsTest262NumberFormatPrecisionMatrix(['en-US'], ['\\ud800'], {{ maximumFractionDigits: 1 }}, {expected})"
    ));
}

#[test]
fn number_format_matrix_with_no_locales_or_numbering_systems_is_a_no_op() {
    expect_true(
        "__bluejsTest262NumberFormatPrecisionMatrix([], ['latn'], {}, {}) === undefined &&\
         __bluejsTest262NumberFormatPrecisionMatrix(['en-US'], [], {}, {}) === undefined",
    );
}

#[test]
fn every_native_helper_is_installed_only_by_the_explicit_harness() {
    let mut plain = Vm::default();
    let result = run(
        &mut plain,
        "typeof assert + typeof buildString + typeof $262",
    );
    assert_eq!(
        result,
        Ok(Value::String("undefinedundefinedundefined".into()))
    );
    expect_true(
        "typeof assert === 'function' && typeof buildString === 'function' && typeof $262 === 'object'",
    );
}

#[test]
fn format_simple_value_falls_back_for_symbols_and_objects_without_string_conversion() {
    expect_true(
        "formatSimpleValue(Symbol('tag')) === 'Symbol(tag)' &&\
         formatSimpleValue(Object.create(null)) === '[object Object]' &&\
         formatSimpleValue(undefined) === 'undefined' &&\
         formatSimpleValue(true) === 'true'",
    );
    // An identity-free rendering never exposes objects or symbols.
    expect_true("formatIdentityFreeValue({}) === undefined && formatIdentityFreeValue(1n) === '1'");
}

#[test]
fn create_realm_returns_an_independent_harness_realm() {
    expect_true(
        "let other = $262.createRealm();\
         typeof other === 'object' && typeof other.global === 'object' &&\
         other.global !== globalThis && other.global.Array !== Array",
    );
}

#[test]
fn property_helper_verifies_descriptor_attributes() {
    expect_true(
        "let o = { x: 1 };\
         Object.defineProperty(o, 'y', { value: 2, writable: false, enumerable: false, configurable: false });\
         verifyProperty(o, 'x', { value: 1, writable: true, enumerable: true, configurable: true });\
         verifyProperty(o, 'y', { value: 2, writable: false, enumerable: false, configurable: false });\
         true",
    );
    expect_test262_failure("let o = { x: 1 }; verifyProperty(o, 'x', { value: 2 })");
    expect_test262_failure("let o = { x: 1 }; verifyProperty(o, 'x', { writable: false })");
    expect_test262_failure("verifyProperty({}, 'missing', { value: 1 })");
}

#[test]
fn agent_helpers_are_reachable_from_the_main_realm_without_starting_an_agent() {
    expect_true(
        "$262.agent.sleep(0) === undefined &&\
         typeof $262.agent.monotonicNow() === 'number' &&\
         $262.agent.getReport() === null",
    );
    // Only a started agent may announce that it is leaving.
    expect_type_error("$262.agent.leaving()");
}

#[test]
fn host_gc_hook_runs_a_major_collection_and_keeps_live_values() {
    let mut vm = harness_vm();
    let before = vm.heap().stats().major_collections;
    assert_eq!(
        run(
            &mut vm,
            "var live = {kept: [1, 2, 3]}; \
             typeof $262.gc === 'function' && $262.gc.length === 0 \
               && $262.gc() === undefined && live.kept.length === 3"
        ),
        Ok(Value::Bool(true))
    );
    assert!(vm.heap().stats().major_collections > before);
}
