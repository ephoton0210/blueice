// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! All parts of a class are strict mode code, so the heritage and computed-key
//! expressions of a class written in sloppy code run with strict semantics
//! (failed assignments throw), and the surrounding sloppy code resumes
//! afterwards, also when the class throws into a handler of the same function.
use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    let program = parse(source).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    Vm::default()
        .execute(&compile(&program).unwrap())
        .unwrap_or_else(|e| panic!("{source}: {e:?}"))
}

#[test]
fn a_heritage_expression_in_sloppy_code_runs_strict() {
    for class in [
        // The value would be a valid heritage, so only the failed assignment
        // itself can throw.
        "class b extends (Object.preventExtensions({}).prop = function () {}) { constructor() { super(); } }",
        "var b = class extends (Object.preventExtensions({}).prop = function () {}) { };",
    ] {
        let source = format!(
            "function shouldThrow() {{ {class} }}
             var r; try {{ shouldThrow(); r = 'no error'; }} catch (e) {{ r = e instanceof TypeError; }} r"
        );
        assert_eq!(evaluate(&source), Value::Bool(true), "{class}");
    }
}

#[test]
fn a_computed_key_in_sloppy_code_runs_strict() {
    for class in [
        "class b { [Object.preventExtensions({}).prop = 4]() { } constructor() { } }",
        "var b = class { [Object.preventExtensions({}).prop = 4]() { } constructor() { } };",
        "class b { static [Object.preventExtensions({}).prop = 4] = 1; }",
    ] {
        let source = format!(
            "function shouldThrow() {{ {class} }}
             var r; try {{ shouldThrow(); r = 'no error'; }} catch (e) {{ r = e instanceof TypeError; }} r"
        );
        assert_eq!(evaluate(&source), Value::Bool(true), "{class}");
    }
}

#[test]
fn sloppy_code_resumes_after_the_class_and_after_a_caught_throw() {
    assert_eq!(
        evaluate(
            "function f() {
               var log = [];
               class ok { [(Object.preventExtensions({}), 'k')]() {} }
               // Sloppy again: a failed assignment is silently ignored.
               Object.preventExtensions({}).a = 1; log.push('after class');
               try { class b extends (Object.preventExtensions({}).prop = function () {}) {} }
               catch (e) { log.push(e instanceof TypeError ? 'thrown' : 'wrong'); }
               Object.preventExtensions({}).b = 2; log.push('after catch');
               try { class c { [Object.preventExtensions({}).prop = 4]() {} } }
               finally { Object.preventExtensions({}).c = 3; log.push('in finally'); }
             }
             var out;
             try { f(); } catch (e) { out = e instanceof TypeError; }
             out"
        ),
        Value::Bool(true)
    );
    assert_eq!(
        evaluate(
            "var log = [];
             function f() {
               try { class b extends (Object.preventExtensions({}).prop = function () {}) {} }
               catch (e) { log.push(e instanceof TypeError ? 'thrown' : 'wrong'); }
               Object.preventExtensions({}).b = 2; log.push('after catch');
               try { class c { [Object.preventExtensions({}).prop = 4]() {} } }
               catch (e) { }
               finally { Object.preventExtensions({}).c = 3; log.push('in finally'); }
             }
             f(); log.join()"
        ),
        Value::String("thrown,after catch,in finally".into())
    );
}

#[test]
fn strict_code_stays_strict_and_functions_keep_their_own_strictness() {
    assert_eq!(
        evaluate(
            "function f() { 'use strict'; class a { [1]() {} } try { Object.preventExtensions({}).x = 1; } catch (e) { return e instanceof TypeError; } }
             f()"
        ),
        Value::Bool(true)
    );
}
