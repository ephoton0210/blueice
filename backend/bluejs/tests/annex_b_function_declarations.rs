// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Annex B.3.2 (function declarations in blocks and `switch` cases): duplicate
//! declarations in sloppy code.

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
fn block_functions_still_hoist_when_the_name_is_not_a_parameter() {
    check("var after=(function(p){{function f(){return 1}}return f()})(0);after===1");
    check("(function(f){{function g(){}}return typeof g})(1)==='function'");
    check("(function(){var before=typeof f;{function f(){}}return before+typeof f})()==='undefinedfunction'");
}
