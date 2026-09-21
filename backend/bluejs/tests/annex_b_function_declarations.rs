// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Annex B.3.2 (function declarations in blocks and `switch` cases): duplicate
//! declarations in sloppy code, and the parameter-name / `arguments` exclusions
//! from the legacy var-hoisting of block functions.

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm};

fn evaluate(source: &str) -> Result<Value, RuntimeError> {
    Vm::default().execute(&compile(&parse(source).unwrap()).unwrap())
}

fn check(source: &str) {
    assert_eq!(evaluate(source).unwrap(), Value::Bool(true), "{source}");
}

fn is_syntax_error(source: &str) -> bool {
    match parse(source) {
        Err(_) => true,
        Ok(program) => compile(&program).is_err(),
    }
}

#[test]
fn sloppy_blocks_may_redeclare_a_function_and_the_last_declaration_wins() {
    check(
        "(function(){var inside;{inside=a();function a(){return 1}function a(){return 2}}\
         return inside===2&&a()===2})()",
    );
    check("{function a(){return 1}function a(){return 2}}a()===2");
    check("if(true){function a(){return 1}function a(){return 2}}a()===2");
    check(
        "(function(){switch(1){case 1:function a(){return 1}case 2:function a(){return 2}}return a()})()===2",
    );
    check(
        "{function a(){}function a(){}}switch(0){case 0:function b(){}default:function b(){}}true",
    );
}

#[test]
fn only_ordinary_function_declarations_may_be_redeclared_and_only_in_sloppy_code() {
    for source in [
        "'use strict';{function a(){}function a(){}}",
        "'use strict';switch(0){case 0:function a(){}default:function a(){}}",
        "{function a(){}function* a(){}}",
        "{function* a(){}function a(){}}",
        "{function a(){}async function a(){}}",
        "{function a(){}let a;}",
        "{let a;function a(){}}",
        "{function a(){}class a{}}",
        "{var a;function a(){}}",
        "switch(0){case 0:function a(){}default:function* a(){}}",
        "switch(0){case 0:let a;default:function a(){}}",
    ] {
        assert!(is_syntax_error(source), "{source}");
    }
}

#[test]
fn a_block_function_named_like_a_parameter_does_not_touch_the_parameter() {
    check(
        "var init,after;(function(f){init=f;{function f(){}}after=f}(123));init===123&&after===123",
    );
    check(
        "var init,after;(function(f=123){init=f;{function f(){}}after=f}());init===123&&after===123",
    );
    check("var after;(function(f){var f;{function f(){}}after=f}(7));after===7");
    // No body-level variable is created for the name, so an assignment in the
    // body is visible to a closure created by the parameter list.
    check("(function(f=123,g=function(){return f}){f=5;{function f(){}}return g()})()===5");
    check("(function([f]){{function f(){}}return f})([9])===9");
}

#[test]
fn a_block_function_named_arguments_does_not_replace_the_arguments_object() {
    let body = "assert(arguments.toString()==='[object Arguments]');\
                {assert(arguments()===undefined);function arguments(){}assert(arguments()===undefined)}\
                assert(arguments.toString()==='[object Arguments]');";
    for parameters in ["", "x", "..._", "x=1"] {
        check(&format!(
            "function assert(c){{if(!c)throw new Error('failed')}}\
             (function({parameters}){{{body}}}());true"
        ));
    }
    // An arrow function has no arguments binding of its own, so the ordinary
    // hoisting applies to it.
    check("var f=()=>{{function arguments(){return 1}}return arguments()};f()===1");
}

#[test]
fn block_functions_still_hoist_when_the_name_is_not_a_parameter() {
    check("var after=(function(p){{function f(){return 1}}return f()})(0);after===1");
    check("(function(f){{function g(){}}return typeof g})(1)==='function'");
    check("(function(){var before=typeof f;{function f(){}}return before+typeof f})()==='undefinedfunction'");
}
