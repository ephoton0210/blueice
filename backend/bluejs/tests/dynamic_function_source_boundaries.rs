// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! CreateDynamicFunction parses the parameter text as FormalParameters and
//! the body text as a FunctionBody, each on its own: neither may reach into
//! the other by leaving a comment, template or bracket open, or by closing the
//! function early, even when the joined wrapper source would happen to parse.
use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    let program = parse(source).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    Vm::default()
        .execute(&compile(&program).unwrap())
        .unwrap_or_else(|e| panic!("{source}: {e:?}"))
}

#[test]
fn text_that_only_parses_when_joined_is_a_syntax_error() {
    for (kind, constructor) in [
        ("Function", "Function"),
        (
            "generator",
            "Object.getPrototypeOf(function* () {}).constructor",
        ),
        (
            "async",
            "Object.getPrototypeOf(async function () {}).constructor",
        ),
        (
            "async generator",
            "Object.getPrototypeOf(async function* () {}).constructor",
        ),
    ] {
        for (parameters, body) in [
            ("/*", "*/) {"),
            ("//", ") {"),
            ("a = `", "` ) {"),
            (") { var x = function (", "} "),
            ("x = function (", "}) {"),
            ("", "} function foo() {"),
            ("a", "}, function () {"),
        ] {
            let source = format!(
                "var r; try {{ {constructor}({parameters:?}, {body:?}); r = 'accepted'; }} \
                 catch (e) {{ r = e instanceof SyntaxError ? 'SyntaxError' : String(e); }} r"
            );
            assert_eq!(
                evaluate(&source),
                Value::String("SyntaxError".into()),
                "{kind}: {parameters:?} / {body:?}"
            );
        }
    }
}

#[test]
fn ordinary_parameter_and_body_text_still_composes() {
    for source in [
        "Function('a, b', 'return a + b')(1, 2) === 3",
        // A trailing line comment belongs to the parameters, not the `)`.
        "Function('a //x', 'return a')(4) === 4",
        "Function('/* c */ a', 'return a')(5) === 5",
        "Function('a = `t${1}`', 'return a')() === 't1'",
        "Function('a', 'b', 'return a * b')(3, 4) === 12",
        "Function('return 7')() === 7",
        "Function('{a}, [b]', 'return a + b')({a: 1}, [2]) === 3",
        "Function('return function () {} + \"\"')().indexOf('function') === 0",
        "Object.getPrototypeOf(function* () {}).constructor('a', 'yield a')(2).next().value === 2",
    ] {
        assert_eq!(evaluate(source), Value::Bool(true), "{source}");
    }
}
