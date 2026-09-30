// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Where `await` is an ordinary identifier and where it is not: a class field
//! initializer is parsed with `[~Await]`, so a script may read a variable
//! named `await` there even inside an async function, while module code
//! reserves `await` at every nesting depth, plain functions included.
use blueice_bluejs::{compile, parse, parse_module, Value, Vm};

fn is_rejected(source: &str) -> bool {
    match parse(source) {
        Err(error) => {
            assert!(error.known_syntax, "{source:?}: {error:?}");
            true
        }
        Ok(program) => compile(&program).is_err(),
    }
}

fn module_error(source: &str) -> bool {
    match parse_module(source) {
        Err(error) => {
            assert!(error.known_syntax, "{source:?}: {error:?}");
            true
        }
        Ok(_) => false,
    }
}

#[test]
fn a_class_field_initializer_reads_await_as_an_identifier_in_a_script() {
    let program = parse(
        "var await = 1; var seen;
         async function getClass() { return class { x = await; }; }
         (async function () { class C { x = await; y = () => await; } seen = [new C().x, new C().y()]; })();
         seen.join()",
    )
    .expect("await is an IdentifierReference in a field initializer");
    assert_eq!(
        Vm::default().execute(&compile(&program).unwrap()),
        Ok(Value::String("1,1".into()))
    );
}

#[test]
fn await_is_still_an_operator_in_a_computed_key_and_an_error_after_a_field_initializer_await() {
    for source in [
        "async () => class { [await] = 1 };",
        "async () => class { x = await 1 };",
        "async function f() { class C { x = await 1; } }",
    ] {
        assert!(is_rejected(source), "{source}");
    }
    // A computed key keeps the enclosing [Await], so this is an operator.
    assert!(!is_rejected(
        "async function f() { class C { [await 1] = 2; } }"
    ));
}

#[test]
fn module_code_reserves_await_inside_nested_plain_functions() {
    for source in [
        "function f() { await; }",
        "function f() { var await; }",
        "function f(await) {}",
        "function f() { await: 1; }",
        "function f() { (function await() {}); }",
        "var o = { f() { return await; } };",
        "class C { m() { return await; } }",
        "class C { x = await; }",
    ] {
        assert!(module_error(source), "{source}");
    }
    // The same code is ordinary script code.
    for source in [
        "function f() { await; }",
        "function f() { var await; }",
        "function f(await) {}",
        "function f() { await: 1; }",
    ] {
        assert!(!is_rejected(source), "{source}");
    }
}
