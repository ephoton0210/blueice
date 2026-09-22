// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `Math.sumPrecise` (ECMA-262 21.3.2.34): an exactly-rounded sum of an
//! iterable of Numbers. The expected sums below were computed with exact
//! rational arithmetic (Python `fractions`), not with floating-point addition.

use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

/// Expects `true`; a script may return a string describing the first failure.
fn assert_true(source: &str) {
    match evaluate(source) {
        Value::Bool(true) => {}
        Value::String(reason) => panic!("{}", reason.to_utf8().unwrap()),
        other => panic!("{source}\n  -> {other:?}"),
    }
}

#[test]
fn the_function_has_its_specified_shape() {
    assert_true(
        r#"(function() {
          const d = Object.getOwnPropertyDescriptor(Math, "sumPrecise");
          if (!d || !d.writable || d.enumerable || !d.configurable) return "attributes";
          if (d.value.name !== "sumPrecise" || d.value.length !== 1) return "name/length";
          try { new d.value([]); return "constructor"; } catch (e) { if (!(e instanceof TypeError)) return "wrong error"; }
          return true;
        })()"#,
    );
}

#[test]
fn sums_are_exactly_rounded() {
    assert_true(
        r#"(function() {
          const cases = [
            [[1.0, 2.0, 3.0], 6.0],
            [[1e308, -1e308], 0.0],
            [[0.1, 0.1], 0.2],
            [[0.1, 0.2, 0.3], 0.6],
            [[1e30, 0.1, -1e30], 0.1],
            [[5e-324, 5e-324], 1e-323],
            [[1.5e-323, -5e-324], 1e-323],
            [[1.7976931348623157e308, 1.7976931348623157e308, -1.7976931348623157e308], 1.7976931348623157e308],
            [[9007199254740992.0, 1.0, -1.0], 9007199254740992.0],
            [[6.509344730398538e-147, -3.656889169125855e-227, -3.7495658441984877e-243, 6.985542357461894e142, 5.9110506078989164e-210], 6.985542357461894e142],
            [[-6.306259157317371e-175, -5.771029486174987e295, -9.7625510559292e105, -2.8960928633167627e-254, -5.709136896467344e-154, -1.0305571244359135e272, -3.723975427257312e283], -5.771029486178711e295],
            [[-6.190095931735539e-237, 7.77228774980807e207, 3.6158235594456636e175, -6.989944337295713e-47, -5.74423710258671e-52, 8.751374955734288e236, 6.089590190364036e158], 8.751374955734288e236],
            [[1.6496210364357323e-181, -9.332702121806376e49, -9.620190834121097e130], -9.620190834121097e130],
            [[3.401223621911955e270, 5.7989520428249214e57, -8.399677805125415e166], 3.401223621911955e270],
            [[-6.066942759721971e183, 2.8459553209414924e16, 2.2562928055588574e93, 1.6804837890654457e171, 5.895441933131041e-183], -6.066942759720291e183]
          ];
          for (const [values, expected] of cases) {
            const actual = Math.sumPrecise(values);
            if (!Object.is(actual, expected)) return "sumPrecise([" + values + "]) = " + actual + ", expected " + expected;
          }
          // Overflow: a finite sum that rounds beyond the largest finite Number is an infinity.
          if (Math.sumPrecise([1.7976931348623157e308, 1.7976931348623157e308]) !== Infinity) return "overflow";
          if (Math.sumPrecise([-1.7976931348623157e308, -1.7976931348623157e308]) !== -Infinity) return "negative overflow";
          // The exact sum stays finite even though every partial sum of a naive loop would overflow.
          if (Math.sumPrecise([1.7976931348623157e308, 1.7976931348623157e308, -1.7976931348623157e308]) !== 1.7976931348623157e308) return "no intermediate overflow";
          // Exactly half way between the largest finite Number and 2**1024 rounds (to even) up to Infinity.
          if (Math.sumPrecise([8.98846567431158e307, 8.98846567431158e307]) !== Infinity) return "tie to Infinity";
          return true;
        })()"#,
    );
}

#[test]
fn zeroes_infinities_and_nan_follow_the_state_machine() {
    assert_true(
        r#"(function() {
          const is = (values, expected) => Object.is(Math.sumPrecise(values), expected);
          if (!is([], -0)) return "empty is -0";
          if (!is([-0], -0) || !is([-0, -0], -0)) return "only negative zeros";
          if (!is([0], 0) || !is([-0, 0], 0) || !is([0, -0], 0)) return "a positive zero wins";
          if (!is([1, -1], 0)) return "cancellation is +0";
          if (!is([Infinity], Infinity) || !is([-Infinity], -Infinity)) return "single infinity";
          if (!is([Infinity, 1e308, -1e308], Infinity) || !is([-Infinity, 5], -Infinity)) return "finite values are ignored after an infinity";
          if (!is([Infinity, -Infinity], NaN) || !is([-Infinity, Infinity], NaN)) return "opposite infinities";
          if (!is([NaN], NaN) || !is([1, NaN, Infinity], NaN) || !is([NaN, 1], NaN)) return "NaN is absorbing";
          return true;
        })()"#,
    );
}

#[test]
fn the_argument_is_an_iterable_of_numbers_only() {
    assert_true(
        r#"(function() {
          if (Math.sumPrecise(new Set([1, 2, 3])) !== 6) return "set";
          if (Math.sumPrecise((function*() { yield 0.1; yield 0.2; yield 0.3; })()) !== 0.6) return "generator";
          for (const bad of [undefined, null, 1, {}, true, Symbol()]) {
            try { Math.sumPrecise(bad); return "accepted a non-iterable"; } catch (e) { if (!(e instanceof TypeError)) return "wrong error"; }
          }
          // A string is iterable, but yields strings, which are not Numbers.
          try { Math.sumPrecise("12"); return "string yields strings"; } catch (e) { if (!(e instanceof TypeError)) return "wrong error"; }
          let coercions = 0;
          const object = { valueOf() { coercions++; return 1; }, toString() { coercions++; return "1"; } };
          for (const values of [[{}], [0n], ["1"], [object], [NaN, object], [Infinity, -Infinity, object], [null], [undefined], [true]]) {
            try { Math.sumPrecise(values); return "accepted a non-number"; } catch (e) { if (!(e instanceof TypeError)) return "wrong error"; }
          }
          if (coercions !== 0) return "a value was coerced";
          // The iterator is closed when a non-number is met, and not otherwise.
          let closed = 0;
          const iterable = { [Symbol.iterator]() { return { next() { return { done: false, value: object }; }, return() { closed++; return {}; } }; } };
          try { Math.sumPrecise(iterable); return "no throw"; } catch (e) { if (!(e instanceof TypeError) || closed !== 1) return "close on non-number " + closed; }
          closed = 0;
          const finite = { [Symbol.iterator]() { let i = 0; return { next() { return i++ < 2 ? { done: false, value: 1 } : { done: true }; }, return() { closed++; return {}; } }; } };
          if (Math.sumPrecise(finite) !== 2 || closed !== 0) return "no close on exhaustion";
          // A throwing next() propagates without closing the iterator.
          const throwing = { [Symbol.iterator]() { return { next() { throw new RangeError("x"); }, return() { closed++; return {}; } }; } };
          try { Math.sumPrecise(throwing); return "no throw"; } catch (e) { if (!(e instanceof RangeError) || closed !== 0) return "next throw"; }
          return true;
        })()"#,
    );
}
