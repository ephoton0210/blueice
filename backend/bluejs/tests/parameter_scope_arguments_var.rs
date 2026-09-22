// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! FunctionDeclarationInstantiation with parameter expressions: the body's
//! `var` bindings live in a separate variable environment, so a body
//! `var arguments` is a second binding that starts out holding the arguments
//! object while closures made in the parameter list keep seeing the original.
use blueice_bluejs::{compile, parse, Value, Vm};

fn assert_true(source: &str) {
    let program = parse(source).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    let code = compile(&program).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    assert_eq!(
        Vm::default()
            .execute(&code)
            .unwrap_or_else(|e| panic!("{source}: {e:?}")),
        Value::Bool(true),
        "{source}"
    );
}

#[test]
fn a_body_var_arguments_shadows_the_parameter_scope_arguments() {
    assert_true(
        "function g(h = () => arguments) {
           var arguments = 0;
           return arguments === 0 && h() !== 0 && typeof h() === 'object';
         }
         g()",
    );
}

#[test]
fn a_body_var_arguments_starts_as_the_arguments_object() {
    assert_true(
        "function g(h = () => arguments) {
           var arguments;
           var initial = arguments === h() && typeof arguments === 'object';
           arguments = 0;
           return initial && arguments === 0 && h() !== 0;
         }
         g()",
    );
    // The copy is the same object, not a fresh one: it has the caller's values.
    assert_true(
        "function g(a, h = () => 0) { var arguments; return arguments.length === 3 && arguments[2] === 'z'; }
         g(1, undefined, 'z')",
    );
}

#[test]
fn a_body_function_named_arguments_replaces_the_binding_in_the_body_only() {
    assert_true(
        "function g(h = () => arguments) {
           function arguments() {}
           return typeof arguments === 'function' && typeof h() === 'object';
         }
         g()",
    );
}

#[test]
fn simple_parameter_lists_still_share_one_arguments_binding() {
    assert_true(
        "function g(a) { var arguments = 3; return arguments === 3; }
         function h(a) { var arguments; return typeof arguments === 'object' && arguments.length === 1; }
         function k(a) { function arguments() {} return typeof arguments === 'function'; }
         g(1) && h(1) && k(1)",
    );
}

#[test]
fn a_body_lexical_arguments_still_shadows_only_the_body() {
    assert_true(
        "function g(h = () => arguments) { let arguments = 7; return arguments === 7 && typeof h() === 'object'; }
         g()",
    );
}
