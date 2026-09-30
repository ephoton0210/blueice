// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A generator created inside a `with` statement (or in a function whose
//! sloppy direct eval gave it a variable environment object) resumes with
//! its operand stack intact: a `yield` in the middle of an expression that
//! already holds operands must find them again after every resume.
use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    let program = parse(source).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    Vm::default()
        .execute_script(&compile(&program).unwrap())
        .unwrap_or_else(|e| panic!("{source}: {e:?}"))
}

#[test]
fn yields_inside_expressions_survive_resumes_in_a_with_statement() {
    assert_eq!(
        evaluate(
            "function t() {
               with ({}) {
                 var g = function* () { var a = [yield 1, yield 2]; var [b = yield 3, c = yield 4] = []; return [a, b, c].join('|'); };
                 var it = g(); var log = [it.next().value, it.next('x').value, it.next('y').value, it.next('z').value];
                 var last = it.next('w');
                 return log.join() + ';' + last.value + ';' + last.done;
               }
             }
             t()"
        ),
        Value::String("1,2,3,4;x,y|z|w;true".into())
    );
}

#[test]
fn yields_inside_expressions_survive_resumes_in_a_function_with_eval() {
    assert_eq!(
        evaluate(
            "function t() {
               eval('var seen = 1');
               function* g() { var a = [yield seen, yield 2]; return a.join(); }
               var it = g(); var first = it.next().value; it.next('p'); var r = it.next('q');
               return first + ';' + r.value + ';' + r.done;
             }
             t()"
        ),
        Value::String("1;p,q;true".into())
    );
}

#[test]
fn a_generator_in_a_with_statement_still_sees_the_object_after_a_resume() {
    assert_eq!(
        evaluate(
            "function t() {
               var o = { v: 1 };
               with (o) {
                 var g = function* () { yield v; v = 5; yield v; };
                 var it = g(); var a = it.next().value; var b = it.next().value;
                 return a + ',' + b + ',' + o.v;
               }
             }
             t()"
        ),
        Value::String("1,5,5".into())
    );
}
