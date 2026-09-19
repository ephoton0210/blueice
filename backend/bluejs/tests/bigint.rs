// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! BigInt abstract-operation behavior through parse, compile and VM
//! dispatch: `BigInt(...)` coercion (ToPrimitive/NumberToBigInt/ToBigInt),
//! `StringToBigInt`'s radix-prefixed grammar, `BigInt.asIntN`/`asUintN`, and
//! BigInt's participation in Abstract Equality/Relational Comparison with
//! Number and String, per ECMA-262 2026 (read 2026-09-18) sections
//! "sec-bigint-constructor-number-value", "sec-tobigint",
//! "sec-stringtobigint", "sec-bigint.asintn", "sec-bigint.asuintn",
//! "sec-abstract-equality-comparison" and "sec-abstract-relational-comparison".

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm};

fn evaluate(source: &str) -> Result<Value, RuntimeError> {
    Vm::default().execute(&compile(&parse(source).unwrap()).unwrap())
}

fn assert_true_with_test262_harness(source: &str) {
    let mut vm = Vm::default();
    vm.install_test262_harness()
        .expect("test262 harness installs");
    let result = vm
        .execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap();
    assert_eq!(result, Value::Bool(true), "{source}");
}

fn assert_true(source: &str) {
    assert_eq!(evaluate(source).unwrap(), Value::Bool(true), "{source}");
}

#[test]
fn constructor_converts_integral_numbers_and_rejects_non_integral_ones() {
    assert_true("BigInt(10) === 10n && BigInt(-5) === -5n && BigInt(0) === 0n");
    for source in [
        "(()=>{try{BigInt(1.5);return false}catch(e){return e instanceof RangeError}})()",
        "(()=>{try{BigInt(NaN);return false}catch(e){return e instanceof RangeError}})()",
        "(()=>{try{BigInt(Infinity);return false}catch(e){return e instanceof RangeError}})()",
        "(()=>{try{BigInt(-Infinity);return false}catch(e){return e instanceof RangeError}})()",
    ] {
        assert_true(source);
    }
}

#[test]
fn constructor_converts_booleans() {
    assert_true("BigInt(true) === 1n && BigInt(false) === 0n");
}

#[test]
fn constructor_rejects_non_convertible_primitives() {
    for source in [
        "(()=>{try{BigInt(undefined);return false}catch(e){return e instanceof TypeError}})()",
        "(()=>{try{BigInt(null);return false}catch(e){return e instanceof TypeError}})()",
        "(()=>{try{BigInt(Symbol());return false}catch(e){return e instanceof TypeError}})()",
    ] {
        assert_true(source);
    }
}

#[test]
fn constructor_only_coerces_its_argument_once() {
    // Regression for a constructor that ran ToPrimitive, then ran ToBigInt's
    // own coercion again on the (now-primitive) result: an object whose
    // Symbol.toPrimitive throws on a second call must still succeed.
    assert_true(
        "let first=true;let v={[Symbol.toPrimitive](){if(first){first=false;return '42'}throw new Error('called twice')}};BigInt(v) === 42n",
    );
}

#[test]
fn string_to_bigint_parses_decimal_hex_octal_and_binary() {
    assert_true("BigInt('10') === 10n && BigInt('-10') === -10n && BigInt('900') === 900n");
    assert_true("BigInt('0xa') === 10n && BigInt('0Xff') === 255n && BigInt('0xfabc') === 64188n");
    assert_true("BigInt('0o7') === 7n && BigInt('0O20') === 16n");
    assert_true("BigInt('0b1111') === 15n && BigInt('0B10') === 2n");
    assert_true("BigInt('18446744073709551616') === 18446744073709551616n");
}

#[test]
fn string_to_bigint_treats_blank_strings_as_zero_and_trims_whitespace() {
    assert_true("BigInt('') === 0n && BigInt(' ') === 0n && BigInt('     ') === 0n");
    assert_true("BigInt('   0b1111') === 15n");
    assert_true("BigInt('   7   ') === 7n && BigInt('   -197   ') === -197n");
}

#[test]
fn string_to_bigint_rejects_non_integer_grammar() {
    for source in [
        "'10n'",
        "'10x'",
        "'10b'",
        "'10.5'",
        "'0b'",
        "'-0x1'",
        "'-0XFFab'",
        "'0oa'",
        "'000 12'",
        "'0o'",
        "'0x'",
        "'00o'",
        "'00b'",
        "'00x'",
    ] {
        let program = format!(
            "(()=>{{try{{BigInt({source});return false}}catch(e){{return e instanceof SyntaxError}}}})()"
        );
        assert_true(&program);
    }
}

#[test]
fn loose_equality_compares_bigint_with_number_by_mathematical_value() {
    assert_true("1n == 1 && 1 == 1n && 0n == 0 && 0n == -0");
    assert_true("!(1n == 1.5) && !(1n == 2) && !(1n == NaN) && !(NaN == 1n)");
    assert_true("!(1n == Infinity) && !(Infinity == 1n) && !(1n == -Infinity)");
}

#[test]
fn loose_equality_unwraps_a_boxed_object_against_a_bigint_operand() {
    // Regression: the Object<->primitive ToPrimitive fallback in
    // loose_equal only listed Number/String/Symbol as valid companions for
    // an Object operand, so `Object(1n) == 2n` fell through to `false`
    // instead of unwrapping the object via ToPrimitive first.
    assert_true("Object(1n) == 1n && 1n == Object(1n) && !(Object(1n) == 2n)");
    assert_true("Object(5n) == '5' && '5' == Object(5n)");
}

#[test]
fn loose_equality_compares_bigint_with_string_via_string_to_bigint() {
    assert_true("1n == '1' && '1' == 1n && 10n == '0xa' && '0xa' == 10n");
    assert_true("!(1n == '1.5') && !(1n == 'abc') && !('' == 1n) && 0n == ''");
}

#[test]
fn relational_comparison_orders_bigint_against_number_and_string() {
    assert_true("1n < 2 && 2 > 1n && 5n < '6' && '10' < 11n");
    assert_true("!(1n < NaN) && !(NaN < 1n) && !(1n < 'abc') && !('abc' < 1n)");
    assert_true("1n < Infinity && -Infinity < 1n && !(Infinity < 1n)");
    assert_true("1n <= 1 && 1 <= 1n && 1n >= 1 && 1n <= '1'");
}

#[test]
fn as_int_n_and_as_uint_n_wrap_per_spec_examples() {
    assert_true("BigInt.asIntN(0, -2n) === 0n && BigInt.asIntN(0, 1n) === 0n");
    assert_true(
        "BigInt.asIntN(1, -3n) === -1n && BigInt.asIntN(1, 1n) === -1n && BigInt.asIntN(1, 2n) === 0n",
    );
    assert_true("BigInt.asIntN(2, -3n) === 1n && BigInt.asIntN(2, 2n) === -2n");
    assert_true("BigInt.asIntN(8, 0xabn) === -0x55n && BigInt.asIntN(8, 0xabcdn) === -0x33n");
    assert_true(
        "BigInt.asIntN(64, 0xabcdef0123456789abcdefn) === 0x0123456789abcdefn && BigInt.asIntN(65, 0xabcdef0123456789abcdefn) === -0xfedcba9876543211n",
    );
    assert_true("BigInt.asUintN(0, -2n) === 0n && BigInt.asUintN(1, -1n) === 1n");
    assert_true("BigInt.asUintN(8, 0xabn) === 0xabn && BigInt.asUintN(8, 0xabcdn) === 0xcdn");
}

#[test]
fn as_int_n_coerces_bits_via_to_index_then_bigint_via_to_bigint() {
    assert_true("BigInt.asIntN(-0.9, 1n) === 0n && BigInt.asIntN(0.9, 1n) === 0n");
    assert_true("BigInt.asIntN(NaN, 1n) === 0n && BigInt.asIntN(undefined, 1n) === 0n");
    assert_true("BigInt.asIntN(true, 1n) === -1n && BigInt.asIntN('3', 10n) === 2n");
    assert_true("BigInt.asIntN([0], 1n) === 0n && BigInt.asIntN(['1'], 1n) === -1n");
    for source in [
        "(()=>{try{BigInt.asIntN(-1, 0n);return false}catch(e){return e instanceof RangeError}})()",
        "(()=>{try{BigInt.asIntN(9007199254740992, 0n);return false}catch(e){return e instanceof RangeError}})()",
        "(()=>{try{BigInt.asIntN(Infinity, 0n);return false}catch(e){return e instanceof RangeError}})()",
        "(()=>{try{BigInt.asIntN(0n, 0n);return false}catch(e){return e instanceof TypeError}})()",
        "(()=>{try{BigInt.asIntN(Symbol('1'), 0n);return false}catch(e){return e instanceof TypeError}})()",
        "(()=>{try{BigInt.asIntN(0, 0);return false}catch(e){return e instanceof TypeError}})()",
        "(()=>{try{BigInt.asIntN(0, undefined);return false}catch(e){return e instanceof TypeError}})()",
        "(()=>{try{BigInt.asIntN(0, '0b2');return false}catch(e){return e instanceof SyntaxError}})()",
        "(()=>{try{BigInt.asIntN();return false}catch(e){return e instanceof TypeError}})()",
    ] {
        assert_true(source);
    }
}

#[test]
fn as_int_n_coerces_bits_before_bigint_and_each_argument_once() {
    assert_true(
        "let i=0;let bits={valueOf(){if(i!==0)throw new Error('order');i++;return 0}};let bigint={valueOf(){if(i!==1)throw new Error('order');i++;return 0n}};BigInt.asIntN(bits, bigint);i === 2",
    );
}

#[test]
fn as_int_n_and_as_uint_n_are_not_constructors() {
    for source in [
        "(()=>{try{new BigInt.asIntN(64, 1n);return false}catch(e){return e instanceof TypeError}})()",
        "(()=>{try{new BigInt.asUintN(64, 1n);return false}catch(e){return e instanceof TypeError}})()",
    ] {
        assert_true(source);
    }
}

#[test]
fn to_string_supports_radix_and_a_through_z_digits() {
    assert_true(
        "(-100n).toString() === '-100' && (0n).toString() === '0' && (255n).toString(16) === 'ff'",
    );
    assert_true("(-255n).toString(16) === '-ff' && (8n).toString(2) === '1000'");
    assert_true("(35n).toString(36) === 'z' && (10n).toString(11) === 'a'");
    for radix in 2..=36 {
        assert_true(&format!(
            "(0n).toString({radix}) === '0' && (-1n).toString({radix}) === '-1' && (1n).toString({radix}) === '1'"
        ));
    }
}

#[test]
fn to_string_rejects_out_of_range_or_non_numeric_radix() {
    for source in [
        "(()=>{try{(0n).toString(0);return false}catch(e){return e instanceof RangeError}})()",
        "(()=>{try{(0n).toString(1);return false}catch(e){return e instanceof RangeError}})()",
        "(()=>{try{(0n).toString(37);return false}catch(e){return e instanceof RangeError}})()",
        "(()=>{try{(0n).toString(Symbol());return false}catch(e){return e instanceof TypeError}})()",
        "(()=>{try{(0n).toString(0n);return false}catch(e){return e instanceof TypeError}})()",
    ] {
        assert_true(source);
    }
}

#[test]
fn prototype_is_an_ordinary_object_without_bigint_data() {
    // Unlike Number.prototype/Boolean.prototype, BigInt.prototype must not
    // itself behave like a boxed 0n: calling a BigInt.prototype method
    // directly on it (rather than on a real BigInt/boxed BigInt) throws.
    for source in [
        "(()=>{try{BigInt.prototype.toString(1);return false}catch(e){return e instanceof TypeError}})()",
        "(()=>{try{BigInt.prototype.valueOf();return false}catch(e){return e instanceof TypeError}})()",
        "(()=>{try{BigInt.prototype.toString.call({x:1n});return false}catch(e){return e instanceof TypeError}})()",
    ] {
        assert_true(source);
    }
}

#[test]
fn bigint_is_a_constructor_that_always_throws_when_constructed() {
    // BigInt has [[Construct]] (legal as an `extends` target, and accepted
    // by Reflect.construct's IsConstructor check) even though invoking it
    // with `new`/`super` always throws once NewTarget is observed defined.
    assert_true("(()=>{try{new BigInt(5);return false}catch(e){return e instanceof TypeError}})()");
    assert_true(
        "(()=>{try{Reflect.construct(BigInt,[5]);return false}catch(e){return e instanceof TypeError}})()",
    );
    assert_true(
        "class Foo extends BigInt {};(()=>{try{new Foo(5);return false}catch(e){return e instanceof TypeError}})()",
    );
}

#[test]
fn increment_and_decrement_operators_preserve_bigint() {
    // Regression: `++`/`--` compiled a plain identifier's update through
    // ToNumber (which throws on BigInt) and a fixed Number(1.0) addend
    // (which would then throw "cannot mix BigInt and other types" even if
    // ToNumber were bypassed). ToNumeric must round-trip BigInt, and the
    // "one" added/subtracted must match its type.
    assert_true("let i=1n; i++; i === 2n");
    assert_true("let i=1n; ++i; i === 2n");
    assert_true("let i=5n; i--; i === 4n");
    assert_true("let i=5n; --i; i === 4n");
    assert_true("let i=1n; let r=i++; r === 1n && i === 2n");
    assert_true("let i=1n; let r=++i; r === 2n && i === 2n");
    // Regular Number identifiers must still behave exactly as before.
    assert_true("let i=1; i++; i === 2 && typeof i === 'number'");
}

#[test]
fn increment_and_decrement_operators_preserve_bigint_on_properties() {
    assert_true("let o={x:1n}; o.x++; o.x === 2n");
    assert_true("let o={x:1n}; let r=o.x++; r === 1n && o.x === 2n");
    assert_true("let o={x:1n}; let r=++o.x; r === 2n && o.x === 2n");
    assert_true("let o={x:5n}; o.x--; o.x === 4n");
    // `super.x++` reads through the prototype but its `[[Set]]` creates an
    // own shadowing property on `this`, so the update is observed via
    // `this.x` afterward, not by reading `super.x` again.
    assert_true(
        "class Base{} Base.prototype.x=1n; class Derived extends Base{bump(){super.x++;return this.x}} new Derived().bump() === 2n",
    );
}

#[test]
fn relational_comparison_converts_a_boolean_operand_to_number_not_bigint() {
    // Regression: primitive::compare had no Bool<->BigInt case and fell
    // through to `number(&value)`, which throws for a BigInt operand.
    // ToNumeric(Boolean) is Number (0/1), never BigInt, matching the
    // Abstract Relational Comparison algorithm's own ToNumeric step.
    assert_true("!(0n > false) && !(false > 0n) && !(0n > true)");
    assert_true("(true > 0n) && (1n > false) && !(false > 1n) && !(1n > true) && !(true > 1n)");
    assert_true("(31n > true) && !(true > 31n) && !(-3n > true) && (true > -3n)");
    assert_true("!(-3n > false) && (false > -3n)");
}

#[test]
fn bigint_literal_is_a_valid_property_name_converted_to_its_decimal_string() {
    // LiteralPropertyName: NumericLiteral -- "Let nbr be the NumericValue of
    // NumericLiteral. Return ! ToString(nbr)." BigInt's ToString is its
    // plain decimal representation, with no scientific-notation subtlety
    // (unlike a large Number literal used as a property name).
    assert_true("let o={999999999999999999n: true}; o['999999999999999999'] === true");
    assert_true("let o={1n(){return 'bar'}}; o['1']() === 'bar'");
    assert_true("class C{1n(){return 'baz'}} new C()['1']() === 'baz'");
    assert_true("let {1n: a} = {'1': 'foo'}; a === 'foo'");
}

#[test]
fn json_stringify_throws_on_a_cross_realm_boxed_bigint_without_tojson() {
    // Regression: JSON's Object branch only unwrapped a boxed Number/
    // String/BigInt via this realm's own `boxed_primitive`, so a wrapper
    // object built by a *different* Test262 realm (via $262.createRealm())
    // fell through to ordinary-object serialization ("{}") instead of
    // unwrapping to its [[BigIntData]] and throwing TypeError, per
    // SerializeJSONProperty's internal-slot-unwrap step.
    assert_true_with_test262_harness(
        "var other = $262.createRealm().global; \
         var wrapped = other.Object(other.BigInt(100)); \
         var threw = false; \
         try { JSON.stringify(wrapped); } catch (e) { threw = e instanceof TypeError; } \
         threw",
    );
    assert_true_with_test262_harness(
        "var other = $262.createRealm().global; \
         var wrapped = other.Object(other.BigInt(100)); \
         other.BigInt.prototype.toJSON = function () { return this.toString(); }; \
         JSON.stringify(wrapped) === '\"100\"'",
    );
}

#[test]
fn bigint_values_pass_through_proxy_traps_and_reflect_operations_unchanged() {
    // Test262 has no built-ins/Proxy or built-ins/Reflect tests tagged
    // BigInt: neither operates on a value's type, only on property keys and
    // trap results, so a BigInt payload is not special-cased by either.
    // These are our own end-to-end checks of that boundary.
    assert_true(
        "let target={x:1n}; let seen; let p=new Proxy(target,{get(t,k,r){seen=k;return Reflect.get(t,k,r)}}); p.x === 1n && seen === 'x'",
    );
    assert_true(
        "let target={}; let p=new Proxy(target,{set(t,k,v,r){return Reflect.set(t,k,v,r)}}); p.x=5n; target.x === 5n",
    );
    assert_true("Reflect.apply(function(a,b){return a+b}, null, [1n, 2n]) === 3n");
    assert_true("function F(a){this.v=a} let inst=Reflect.construct(F,[7n]); inst.v === 7n");
    // `in`'s left operand goes through ToPropertyKey, which (being a
    // non-Symbol primitive) just means ToString: a BigInt key and its
    // decimal-string equivalent reach the `has` trap identically.
    assert_true(
        "let p=new Proxy({}, {has(t,k){return k === (5n).toString()}}); (5n in p) === true && ('5' in p) === true",
    );
    // A revocable Proxy's ownKeys trap returning a BigInt-keyed property
    // name (as its decimal string, since property keys are never BigInt
    // themselves) round-trips through Reflect.ownKeys.
    assert_true(
        "let target={}; Object.defineProperty(target,'9',{value:1n,enumerable:true,configurable:true}); let p=new Proxy(target,{}); Reflect.ownKeys(p)[0] === '9' && p['9'] === 1n",
    );
}

#[test]
fn as_int_n_and_as_uint_n_enforce_an_explicit_bit_width_cap_rather_than_truncating() {
    // ToIndex alone permits `bits` up to 2**53-1, far beyond what a real
    // 2**bits-sized BigInt allocation could ever be. `asIntN`/`asUintN`
    // enforce a hard, explicit 1,000,000-bit implementation-capacity limit
    // (a RangeError, matching bigint_shift/bigint_exponentiate's existing
    // convention) -- this is a real deliberate ceiling, not a value that
    // gets silently narrowed/wrapped/truncated before use.
    assert_true("BigInt.asIntN(1000000, 1n) === 1n"); // exactly at the cap: still succeeds
    for source in [
        "(()=>{try{BigInt.asIntN(1000001, 1n);return false}catch(e){return e instanceof RangeError}})()",
        "(()=>{try{BigInt.asUintN(1000001, 1n);return false}catch(e){return e instanceof RangeError}})()",
    ] {
        assert_true(source);
    }
}

#[test]
fn as_int_n_and_as_uint_n_have_expected_name_and_length() {
    assert_true(
        "BigInt.asIntN.name === 'asIntN' && BigInt.asIntN.length === 2 && BigInt.asUintN.name === 'asUintN' && BigInt.asUintN.length === 2",
    );
    assert_true(
        "let d=Object.getOwnPropertyDescriptor(BigInt,'asIntN');!d.enumerable && d.writable && d.configurable",
    );
}
