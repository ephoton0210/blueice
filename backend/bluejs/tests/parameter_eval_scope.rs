// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A sloppy direct eval in a parameter initializer declares its `var`s in the
//! function's parameter environment (§10.2.11 FunctionDeclarationInstantiation
//! steps 20-28), which lies outside the parameters and encloses every closure
//! made in the parameter list or the body -- so those closures still see the
//! variable after the call has returned, whenever they run.

use blueice_bluejs::{compile, parse, Value, Vm, VmConfig};

fn truthy(source: &str) {
    for capacity in [None, Some(1)] {
        let mut config = VmConfig::default();
        if let Some(capacity) = capacity {
            config.heap.nursery_capacity = capacity;
        }
        let result = Vm::new(config)
            .unwrap()
            .execute(&compile(&parse(source).unwrap()).unwrap());
        assert_eq!(result, Ok(Value::Bool(true)), "{capacity:?}: {source}");
    }
}

#[test]
fn closures_made_after_the_eval_see_its_var() {
    truthy(
        "var x = 'outside'; var probe1, probe2, probeBody;
         function f(_ = (eval('var x = \"inside\";'), probe1 = function() { return x; }),
                    __ = probe2 = function() { return x; }) {
           probeBody = function() { return x; };
         }
         f();
         probe1() === 'inside' && probe2() === 'inside' && probeBody() === 'inside'",
    );
}

#[test]
fn a_closure_made_before_the_eval_sees_its_var_too() {
    truthy(
        "var x = 'outside'; var probe1, probe2;
         function f(_ = probe1 = function() { return x; },
                    __ = (eval('var x = \"inside\";'), probe2 = function() { return x; })) {}
         f();
         probe1() === 'inside' && probe2() === 'inside'",
    );
}

#[test]
fn every_function_form_and_a_rest_parameter_work() {
    truthy(
        "var x = 'outside'; var p;
         var g = function(_ = (eval('var x = 1'), p = () => x)) {}; g(); p() === 1",
    );
    truthy(
        "var x = 'outside'; var p;
         var g = (_ = (eval('var x = 2'), p = () => x)) => {}; g(); p() === 2",
    );
    truthy(
        "var x = 'outside'; var p;
         var o = { m(_ = (eval('var x = 3'), p = () => x)) {} }; o.m(); p() === 3",
    );
    truthy(
        "var x = 'outside'; var p1, p2;
         function f(_ = p1 = () => x, ...[__ = (eval('var x = 4'), p2 = () => x)]) {} f();
         p1() === 4 && p2() === 4",
    );
}

#[test]
fn each_call_has_its_own_parameter_environment() {
    truthy(
        "var x = 'outside'; var probes = [];
         function f(n, _ = (eval('var x = n'), probes.push(() => x))) {}
         f(1); f(2); f(3);
         probes.map(p => p()).join() === '1,2,3' && x === 'outside'",
    );
}

#[test]
fn the_parameters_and_body_still_shadow_and_see_the_eval_var() {
    // A parameter of the same name wins over the eval var; the body sees it.
    truthy(
        "var seen;
         function f(a, b = (eval('var c = 5'), c + 1)) { seen = [b, c]; }
         f(); seen.join() === '6,5'",
    );
    truthy(
        "function f(x = 'param', y = (eval('var x = \"eval\"'), 0)) {}
         var r; try { f(); r = 'no throw'; } catch (e) { r = e instanceof SyntaxError; } r === true",
    );
}

#[test]
fn a_later_body_eval_keeps_its_ordinary_scope() {
    truthy(
        "var x = 'outside'; var read;
         function f(_ = (eval('var x = \"param\"'), 0)) { eval('var y = 7'); read = () => typeof y; return y; }
         f() === 7 && x === 'outside'",
    );
}

#[test]
fn the_variable_is_not_visible_outside_the_call() {
    truthy("function f(_ = eval('var leaked = 1')) {} f(); typeof leaked === 'undefined'");
}

#[test]
fn strict_code_and_plain_evals_keep_their_behavior() {
    // Strict eval code has its own variable environment: nothing leaks.
    truthy(
        "'use strict'; var x = 'outside'; var p;
         function f(_ = (eval('var x = \"inside\"'), p = () => x)) {}
         f(); p() === 'outside'",
    );
    truthy("function f(a = eval('1 + 1')) { return a; } f() === 2");
}
