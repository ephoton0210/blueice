// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The remaining `assert` family paths: helper globals whose members throw at
//! each step, messages that outgrow the string limit at every concatenation,
//! and lazily built intrinsics failing under a cold heap ceiling.
mod cov_g2_sweep;

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm, VmConfig};
use cov_g2_sweep::{sweep_budget, sweep_cold, sweep_string_limit, Mode};

fn results(body: &str) -> String {
    let mut vm = Vm::new(VmConfig::default()).unwrap();
    vm.install_test262_harness().unwrap();
    let source = format!(
        "function attempt(f) {{ try {{ f(); return 'passed'; }} catch (e) {{
           try {{ return e === 'boom' ? 'boom' : (typeof e === 'string' ? e : e.constructor.name + ':' + e.message); }}
           catch (x) {{ return 'unprintable'; }} }} }}
         [{body}].join('|')"
    );
    match vm.execute(&compile(&parse(&source).unwrap()).unwrap()) {
        Ok(Value::String(text)) => text.to_utf8().unwrap(),
        other => panic!("{source}: {other:?}"),
    }
}

#[test]
fn helper_globals_that_throw_at_each_step() {
    let out = results(
        "(function () {
           var out = [];
           var Sym = Symbol.prototype.toString, ArrayProto = Array.prototype;
           var s = Symbol();
           out.push(attempt(() => assert.compareArray({ length: s }, { length: s })));
           out.push(attempt(() => assert.compareArray({ get length() { throw 'boom'; } }, { length: 0 })));
           out.push(attempt(() => { var n = 0; assert.compareArray({ get length() { if (n++ > 1) throw 'boom'; return 1; }, 0: 1 }, { length: 1, 0: 1 }); }));
           out.push(attempt(() => assert.compareArray({ length: { valueOf() { throw 'boom'; } } }, { length: 0 }) ));
           var savedMap = ArrayProto.map;
           ArrayProto.map = function () { throw 'boom'; };
           out.push(attempt(() => assert.compareArray([1], [2])));
           ArrayProto.map = savedMap;
           var savedJoin = ArrayProto.join;
           ArrayProto.join = function () { throw 'boom'; };
           out.push(attempt(() => assert.compareArray([1], [2])));
           Object.defineProperty(ArrayProto, 'join', { get() { throw 'boom'; }, configurable: true });
           out.push(attempt(() => assert.compareArray([1], [2])));
           Object.defineProperty(ArrayProto, 'join', { value: savedJoin, writable: true, configurable: true });
           Object.defineProperty(ArrayProto, 'map', { get() { throw 'boom'; }, configurable: true });
           out.push(attempt(() => assert.compareArray([1], [2])));
           Object.defineProperty(ArrayProto, 'map', { value: savedMap, writable: true, configurable: true });
           var savedArray = globalThis.Array;
           globalThis.Array = { get prototype() { throw 'boom'; } };
           out.push(attempt(() => compareArray.format([1])));
           globalThis.Array = savedArray;
           var savedString = globalThis.String;
           delete globalThis.String;
           out.push(attempt(() => compareArray.format([1])));
           globalThis.String = savedString;
           var savedFormat = compareArray.format;
           Object.defineProperty(compareArray, 'format', { get() { throw 'boom'; }, configurable: true });
           out.push(attempt(() => assert.compareArray([1], [2])));
           var calls = 0;
           Object.defineProperty(compareArray, 'format', { value() { if (calls++) throw 'boom'; return 'x'; }, configurable: true });
           out.push(attempt(() => assert.compareArray([1], [2])));
           Object.defineProperty(compareArray, 'format', { value: savedFormat, configurable: true });
           Object.defineProperty(Symbol.prototype, 'toString', { get() { throw 'boom'; }, configurable: true });
           out.push(attempt(() => assert.compareArray([1], [1], Symbol())));
           Object.defineProperty(Symbol.prototype, 'toString', { value() { throw 'boom'; }, configurable: true });
           out.push(attempt(() => assert.compareArray([1], [1], Symbol())));
           Object.defineProperty(Symbol.prototype, 'toString', { value: Sym, writable: true, configurable: true });
           out.push(attempt(() => assert.throws({ get name() { throw 'boom'; } }, () => {})));
           var savedObject = globalThis.Object;
           globalThis.Object = { get prototype() { throw 'boom'; } };
           out.push(attempt(() => formatSimpleValue({ toString() { throw new TypeError(); } })));
           globalThis.Object = savedObject;
           out.push(attempt(() => formatSimpleValue({ toString() { throw { get name() { throw 'boom'; } }; } })));
           return out.join('~');
         })()",
    );
    assert_eq!(
        out,
        "TypeError:Number conversion requires a non-Symbol primitive~boom~boom~boom~boom~boom~boom~\
         boom~boom~ReferenceError:String~boom~boom~boom~boom~boom~boom~boom"
    );
}

#[test]
fn every_concatenation_of_an_assertion_message_can_exceed_the_limit() {
    for script in [
        "assert(false);",
        "assert.sameValue('a', 'b');",
        "assert.sameValue(1, 2, 'm');",
        "assert.notSameValue(1, 1);",
        "assert.throws(TypeError, () => {});",
        "assert.throws(TypeError, () => { throw new RangeError(); });",
        "assert.throws(TypeError, () => { throw new (class TypeError extends Error {})(); });",
        "assert.compareArray(1, [1]);",
        "assert.compareArray([1], [2]);",
        "assert.compareArray([1], [2], 'msg');",
        "delete globalThis.JSON; assert.sameValue('a', 'b');",
        "assert.sameValue(1n, 2n);",
    ] {
        sweep_string_limit(Mode::HARNESS, script, 1, 1, |result| {
            matches!(result, Err(RuntimeError::Test262(_)))
        });
    }
}

#[test]
fn compare_array_spends_fuel_per_element() {
    sweep_budget(
        Mode::HARNESS,
        "assert.compareArray([1, 2, 3], [1, 2, 3]); true",
        Value::Bool(true),
    );
}

#[test]
fn a_cold_harness_builds_its_error_and_json_intrinsics_under_a_ceiling() {
    for script in [
        "try { assert.sameValue('a', 'b'); } catch (e) {} true",
        "try { assert(false, {}); } catch (e) {} true",
        "try { assert.throws(TypeError, () => {}, {}); } catch (e) {} true",
        "try { assert.compareArray([1], [2]); } catch (e) {} true",
    ] {
        sweep_cold(Mode::HARNESS, script, 16);
    }
}
