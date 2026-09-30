// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Annex B.3.4: a direct eval in a catch block may redeclare the catch
//! parameter with `var`. The var binding is created in the function's variable
//! environment, but its initializer assigns the (innermost) catch parameter.
use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    let program = parse(source).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    Vm::default()
        .execute_script(&compile(&program).unwrap())
        .unwrap_or_else(|e| panic!("{source}: {e:?}"))
}

#[test]
fn the_initializer_assigns_the_catch_parameter_and_the_var_lives_outside() {
    assert_eq!(
        evaluate(
            "var x = 'global-x'; var log = '';
             function g() {
               try { throw 8; } catch (x) { eval('var x = 42;'); log += x; }
               log += typeof x;          // the eval var: declared, never initialized
               x = 'g';
               log += x;
             }
             g();
             log + '|' + x"
        ),
        Value::String("42undefinedg|global-x".into())
    );
}

#[test]
fn nested_blocks_of_the_eval_code_reach_the_catch_parameter_too() {
    assert_eq!(
        evaluate(
            "function g() {
               var r = [];
               try { throw 1; } catch (e) {
                 eval('{ var e = 3; }'); r.push(e);
                 eval('for (var e = 4; false; ) {}'); r.push(e);
                 eval('(function () { var e = 9; })()'); r.push(e);
               }
               return r.join();
             }
             g()"
        ),
        Value::String("3,4,4".into())
    );
}

#[test]
fn without_a_catch_parameter_the_var_is_the_binding_itself() {
    assert_eq!(
        evaluate(
            "function g() { try { throw 1; } catch (other) { eval('var x = 5;'); } return x; }
             g()"
        ),
        Value::Number(5.0)
    );
}
