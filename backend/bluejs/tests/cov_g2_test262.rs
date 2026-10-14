// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The native Test262 `assert` family (`vm/test262/assertions.rs`) under
//! throwing conversions, tampered globals and constrained string lengths, and
//! the `%AbstractModuleSource%` host functions.
mod cov_g2_sweep;

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm, VmConfig};
use cov_g2_sweep::{sweep_ops, sweep_string_limit, Mode};

fn harness_vm(config: VmConfig) -> Vm {
    let mut vm = Vm::new(config).unwrap();
    vm.install_test262_harness().unwrap();
    vm
}

fn run(vm: &mut Vm, source: &str) -> Result<Value, RuntimeError> {
    vm.execute(&compile(&parse(source).unwrap()).unwrap())
}

/// Evaluates `body` in a fresh harness VM; `body` returns the array of
/// results it wants to compare.
fn results(body: &str) -> String {
    let mut vm = harness_vm(VmConfig::default());
    let source = format!(
        "function attempt(f) {{ try {{ f(); return 'passed'; }} catch (e) {{
           try {{ return e === 'boom' ? 'boom' : e.constructor.name + ':' + e.message; }} catch (x) {{ return 'unprintable'; }} }} }}
         var boom = {{ valueOf() {{ throw 'boom'; }}, toString() {{ throw 'boom'; }} }};
         [{body}].join('|')"
    );
    match run(&mut vm, &source) {
        Ok(Value::String(text)) => text.to_utf8().unwrap(),
        other => panic!("{source}: {other:?}"),
    }
}

#[test]
fn assertion_messages_convert_their_pieces_in_order() {
    assert_eq!(
        results(
            "attempt(() => assert.sameValue(1, 2, boom)),
             attempt(() => assert.sameValue(1, 2, 'msg')),
             attempt(() => assert.sameValue(1, 1, boom)),
             attempt(() => assert.notSameValue(1, 1, 'nmsg')),
             attempt(() => assert.notSameValue(1, 1, boom)),
             attempt(() => assert.sameValue({ toString() { throw new TypeError('t'); } }, 2)),
             attempt(() => assert.sameValue(1, { toString() { throw new RangeError('r'); } })),
             attempt(() => assert.sameValue(boom, 2)),
             attempt(() => assert(false)),
             attempt(() => assert(false, boom)),
             attempt(() => assert(false, 'custom')),
             attempt(() => assert(1)),
             attempt(() => assert.sameValue(0n, 1n)),
             attempt(() => assert.sameValue(-0, 0)),
             attempt(() => assert.sameValue('a', 'b')),
             attempt(() => assert.sameValue(Symbol('s'), Symbol('s')))"
        ),
        "boom|\
         Test262Error:msg Expected SameValue(«1», «2») to be true|\
         passed|\
         Test262Error:nmsg Expected SameValue(«1», «1») to be false|\
         boom|\
         Test262Error:Expected SameValue(«[object Object]», «2») to be true|\
         RangeError:r|\
         boom|\
         Test262Error:Expected true but got false|\
         unprintable|\
         Test262Error:custom|\
         Test262Error:Expected true but got 1|\
         Test262Error:Expected SameValue(«0n», «1n») to be true|\
         Test262Error:Expected SameValue(«-0», «0») to be true|\
         Test262Error:Expected SameValue(«\"a\"», «\"b\"») to be true|\
         Test262Error:Expected SameValue(«Symbol(s)», «Symbol(s)») to be true"
    );
}

#[test]
fn assert_throws_reports_each_way_a_call_can_go_wrong() {
    assert_eq!(
        results(
            "attempt(() => assert.throws(TypeError, 1)),
             attempt(() => assert.throws(TypeError, () => {})),
             attempt(() => assert.throws(TypeError, () => {}, boom)),
             attempt(() => assert.throws(TypeError, () => { throw 1; })),
             attempt(() => assert.throws(TypeError, () => { throw 1; }, 'm')),
             attempt(() => assert.throws(TypeError, () => { throw null; })),
             attempt(() => assert.throws(TypeError, () => { throw new RangeError(); })),
             attempt(() => assert.throws(TypeError, () => { throw new (class TypeError extends Error {})(); })),
             attempt(() => assert.throws(TypeError, () => { throw new TypeError(); })),
             attempt(() => assert.throws({ get name() { throw 'boom'; } }, () => {})),
             attempt(() => assert.throws({ name: 'X' }, () => { throw { get constructor() { throw 'boom'; } }; })),
             attempt(() => assert.throws({ get name() { throw 'boom'; } }, () => { throw new RangeError(); })),
             attempt(() => assert.throws(TypeError, () => { throw { constructor: { get name() { throw 'boom'; } } }; }))"
        ),
        "Test262Error:assert.throws requires two arguments: the error constructor and a function to run|\
         Test262Error:Expected a TypeError to be thrown but no exception was thrown at all|\
         boom|\
         Test262Error:Thrown value was not an object!|\
         Test262Error:m Thrown value was not an object!|\
         Test262Error:Thrown value was not an object!|\
         Test262Error:Expected a TypeError but got a RangeError|\
         Test262Error:Expected a TypeError but got a different error constructor with the same name|\
         passed|\
         boom|\
         boom|\
         boom|\
         boom"
    );
}

#[test]
fn compare_array_reads_length_and_elements_in_the_upstream_order() {
    assert_eq!(
        results(
            "attempt(() => assert.compareArray([1, 2], [1, 2])),
             attempt(() => assert.compareArray([1], [2])),
             attempt(() => assert.compareArray([1], [1, 2], 'm')),
             attempt(() => assert.compareArray('x', [])),
             attempt(() => assert.compareArray([], 1, 'p')),
             attempt(() => assert.compareArray([], null, boom)),
             attempt(() => assert.compareArray([1], [2], Symbol('sym'))),
             attempt(() => assert.compareArray([1], [2], { toString() { throw 'boom'; } })),
             attempt(() => assert.compareArray({ get length() { throw 'boom'; } }, [])),
             attempt(() => assert.compareArray([], { get length() { throw 'boom'; } })),
             attempt(() => assert.compareArray({ length: 1, get 0() { throw 'boom'; } }, [1])),
             attempt(() => assert.compareArray({ length: 1, 0: 1 }, { length: 1, get 0() { throw 'boom'; } })),
             attempt(() => assert.compareArray({ length: boom }, { length: 0 })),
             compareArray([1], [1]) && !compareArray([1], [2]) && !compareArray([], [1]),
             attempt(() => compareArray({ get length() { throw 'boom'; } }, [])),
             compareArray.format({ length: 2, 0: 'a', 1: 'b' }),
             attempt(() => compareArray.format({ get length() { throw 'boom'; } }))"
        ),
        "passed|\
         Test262Error:Actual [1] and expected [2] should have the same contents. |\
         Test262Error:Actual [1] and expected [1, 2] should have the same contents. m|\
         Test262Error:Actual argument [x] shouldn't be primitive. |\
         Test262Error:Expected argument [1] shouldn't be primitive. p|\
         boom|\
         Test262Error:Actual [1] and expected [2] should have the same contents. Symbol(sym)|\
         boom|\
         boom|\
         boom|\
         boom|\
         boom|\
         boom|\
         true|\
         boom|\
         [a, b]|\
         boom"
    );
}

#[test]
fn assertions_survive_tampered_globals() {
    assert_eq!(
        results(
            "(function () {
               var saved = { String: globalThis.String, JSON: globalThis.JSON, Object: globalThis.Object, Array: globalThis.Array };
               var out = [];
               delete globalThis.JSON;
               out.push(attempt(() => assert.sameValue('a', 'b')));
               globalThis.JSON = saved.JSON;
               Object.defineProperty(globalThis, 'JSON', { value: undefined, writable: true, configurable: true });
               out.push(attempt(() => assert.sameValue('a', 'b')));
               globalThis.JSON = saved.JSON;
               Object.defineProperty(globalThis, 'String', { get() { throw 'boom'; }, configurable: true });
               out.push(attempt(() => assert.sameValue(1n, 2n)));
               out.push(attempt(() => assert.sameValue(1, 2)));
               delete globalThis.String;
               out.push(attempt(() => assert.sameValue(1, 2)));
               globalThis.String = saved.String;
               delete globalThis.Object;
               out.push(attempt(() => assert.sameValue({ toString: 0, valueOf: 0 }, 2)));
               globalThis.Object = saved.Object;
               delete globalThis.Array;
               out.push(attempt(() => compareArray.format([1])));
               out.push(attempt(() => assert.compareArray([1], [2])));
               globalThis.Array = saved.Array;
               var savedCompare = globalThis.compareArray;
               delete globalThis.compareArray;
               out.push(attempt(() => assert.compareArray([1], [2])));
               globalThis.compareArray = savedCompare;
               return out.join('~');
             })()"
        ),
        "Test262Error:Expected SameValue(«\"a\"», «\"b\"») to be true~\
         Test262Error:Expected SameValue(«\"a\"», «\"b\"») to be true~\
         boom~\
         boom~\
         ReferenceError:String~\
         ReferenceError:Object~\
         ReferenceError:Array~\
         ReferenceError:Array~\
         ReferenceError:compareArray"
    );
}

#[test]
fn assertion_natives_read_the_globals_they_use_at_call_time() {
    assert_eq!(
        results(
            "(function () {
               var out = [];
               var realToString = Object.prototype.toString;
               Object.defineProperty(Object.prototype, 'toString', { get() { throw 'boom'; }, configurable: true });
               out.push(attempt(() => formatSimpleValue({ toString() { throw new TypeError(); } })));
               Object.defineProperty(Object.prototype, 'toString', { value: realToString, writable: true, configurable: true });
               var savedStringify = JSON.stringify;
               JSON.stringify = function () { throw 'boom'; };
               out.push(attempt(() => formatIdentityFreeValue('x')));
               JSON.stringify = savedStringify;
               out.push(formatIdentityFreeValue(null), formatIdentityFreeValue(undefined), formatIdentityFreeValue(true),
                        formatIdentityFreeValue(5n), formatIdentityFreeValue({}), formatSimpleValue(Symbol('q')));
               return out.join('~');
             })()"
        ),
        "boom~boom~null~undefined~true~5n~~Symbol(q)"
    );
}

#[test]
fn assertions_report_oversized_messages_as_string_limit_errors() {
    for script in [
        "assert.sameValue('a'.repeat(60), 'b'.repeat(60), 'c'.repeat(60));",
        "assert.compareArray(['a'.repeat(50)], ['b'.repeat(50)], 'm'.repeat(50));",
        "assert.throws(TypeError, () => { throw new RangeError('x'); }, 'm'.repeat(60));",
        "assert.throws(TypeError, () => {}, 'm'.repeat(60));",
        "assert.notSameValue('a'.repeat(60), 'a'.repeat(60), 'm'.repeat(60));",
        "assert(false, 'x'.repeat(100));",
        "assert(1);",
    ] {
        sweep_string_limit(Mode::HARNESS, script, 160, 4, |result| {
            matches!(result, Err(RuntimeError::Test262(_)))
        });
    }
}

#[test]
fn allocation_failures_while_reporting_assertion_failures() {
    sweep_ops(
        Mode::HARNESS,
        "assert.sameValue(1, 1); assert.compareArray([1], [1]); try { assert.sameValue(1, 2); } catch (e) {}",
        "b(); try { assert.sameValue(1, 2); } catch (e) {}
         b(); try { assert.compareArray([1], [2]); } catch (e) {}
         b(); try { assert.throws(TypeError, () => {}); } catch (e) {}
         b(); try { assert.throws(TypeError, () => { throw new RangeError(); }); } catch (e) {}
         b(); try { assert(false); } catch (e) {}
         b(); try { assert.notSameValue(1, 1); } catch (e) {}
         b(); var f = compareArray.format([1, 2]); b(); var g = formatSimpleValue({ toString: 0 });
         return true;",
        16,
    );
}

#[test]
fn the_abstract_module_source_class_cannot_be_constructed_or_called() {
    assert_eq!(
        results(
            "attempt(() => $262.AbstractModuleSource()),
             attempt(() => new $262.AbstractModuleSource()),
             Object.getOwnPropertyDescriptor($262.AbstractModuleSource.prototype, Symbol.toStringTag).get.call({})"
        ),
        "TypeError:AbstractModuleSource is an abstract constructor|\
         TypeError:value is not a constructor|"
    );
}
