// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A `var` or function declared by sloppy direct eval is a deletable binding
//! (§9.1.1.4.17 / §19.2.1.3): `delete name` from the eval code, or from a
//! closure the eval created, removes it, after which the name resolves outward
//! again and an assignment creates a global property.
use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    let program = parse(source).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    let code = compile(&program).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    Vm::default()
        .execute_script(&code)
        .unwrap_or_else(|e| panic!("{source}: {e:?}"))
}

fn text(value: &str) -> Value {
    Value::String(value.into())
}

#[test]
fn a_closure_created_by_a_function_level_eval_deletes_the_eval_var() {
    assert_eq!(
        evaluate(
            "var y = 42;
             function test() {
               var f = eval(\"var y = 5; (function (a) { if (a === 'get') return y; \
                 if (a === 'set') { y = 71; return; } return eval('delete y'); })\");
               var log = [f('get')];
               log.push(f('delete'));
               log.push(f('get'));          // the global y again
               log.push(y);                 // the creator sees it too
               log.push(f('delete'));       // now the script's own `var y`: not deletable
               f('set');                    // no binding left: assigns the global
               log.push(y, globalThis.y);
               return log.join();
             }
             test()"
        ),
        text("5,true,42,42,false,71,71")
    );
}

#[test]
fn a_closure_created_by_a_global_eval_deletes_the_global_property() {
    assert_eq!(
        evaluate(
            "var f = eval(\"var z = 5; (function (a) { \
                 if (a === 'get') return z; if (a === 'set') { z = 71; return; } \
                 return eval('delete z'); })\");
             var log = [f('get'), Object.getOwnPropertyDescriptor(globalThis, 'z').configurable];
             log.push(f('delete'));
             log.push('z' in globalThis);
             try { f('get'); log.push('no error'); } catch (e) { log.push(e.name); }
             f('set');
             log.push(f('get'), globalThis.z);
             log.join()"
        ),
        text("5,true,true,false,ReferenceError,71,71")
    );
}

#[test]
fn an_ordinary_var_stays_undeletable_from_such_a_closure() {
    assert_eq!(
        evaluate(
            "var x = 17;
             function outer() {
               var x = 2;
               var f = eval(\"var x = 4; (function () { return eval('delete x'); })\");
               return [f(), x].join();
             }
             [(eval(\"var x = 3; (function () { return eval('delete x'); })\"))(), x, outer()].join('|')"
        ),
        text("false|3|false,4")
    );
}

#[test]
fn a_closure_deletes_an_eval_var_without_an_inner_eval() {
    assert_eq!(
        evaluate(
            "function testOuterVar() { return eval('var x; (function () { return delete x; })'); }
             function testOuterFunction() { return eval('function x() {} (function () { return delete x; })'); }
             function testForIn() { return eval('for (var x in {}); (function () { return delete x; })'); }
             function testArgument() { return eval('(function (x) { return delete x; })'); }
             function testArgumentShadow() { return eval('var x; (function (x) { return delete x; })'); }
             function testLocal() { return eval('(function () { var x; return delete x; })'); }
             var results = [testOuterVar, testOuterFunction, testForIn].map(function (t) {
               var f = t(); return [f(), f()].join();
             });
             results.push(testArgument()(), testArgumentShadow()(), testLocal()());
             results.join('|')"
        ),
        text("true,true|true,true|true,true|false|false|false")
    );
}

#[test]
fn deleting_the_var_in_its_own_initializer_neither_throws_nor_leaks() {
    assert_eq!(
        evaluate(
            "(function () { eval('var x = delete(x)'); })();
             typeof x"
        ),
        text("undefined")
    );
}

#[test]
fn typeof_a_deleted_eval_var_is_undefined_without_a_nested_eval() {
    assert_eq!(
        evaluate(
            "var f = eval(\"var gone = 1; delete gone; (function () { return typeof gone; })\");
             f()"
        ),
        text("undefined")
    );
}

#[test]
fn typeof_a_deleted_eval_var_resolves_outward_and_never_throws() {
    assert_eq!(
        evaluate(
            "var outward = 'x';
             function t() {
               var f = eval(\"var gone = 1; var outward = 2; (function () { \
                 var before = typeof gone + typeof outward; \
                 eval('delete gone; delete outward'); \
                 return before + ',' + typeof gone + typeof outward; })\");
               return f();
             }
             t()"
        ),
        text("numbernumber,undefinedstring")
    );
}
