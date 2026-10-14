// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Hashbang comments (`#!...`): a single-line comment recognized only at the
//! very start of Script or Module source text, including source evaluated by
//! `eval`, and never inside a dynamically created function body.

use blueice_bluejs::{compile, parse, parse_module, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap()
}

#[test]
fn a_hashbang_at_the_start_of_a_script_is_a_comment() {
    for source in [
        "#!",
        "#!\n",
        "#! comment",
        "#!2\n",
        "#!\n1",
        "#!\r\n1",
        "#!\u{2028}1",
    ] {
        assert!(parse(source).is_ok(), "{source:?}");
    }
    assert_eq!(evaluate("#!\n1"), Value::Number(1.0));
    assert_eq!(evaluate("#! 1 + 1\n2"), Value::Number(2.0));
    assert_eq!(evaluate("#!\u{2028}3"), Value::Number(3.0));
    assert_eq!(evaluate("#!\r3"), Value::Number(3.0));
}

#[test]
fn a_hashbang_at_the_start_of_a_module_is_a_comment() {
    assert!(parse_module("#!\nexport {};").is_ok());
    assert!(parse_module("#!").is_ok());
}

#[test]
fn a_hashbang_anywhere_else_is_a_syntax_error() {
    for source in [
        " #!\n",
        "\n#!\n",
        "//\n#!\n",
        "/**/#!\n",
        "#!\n#!\n",
        "\"use strict\"\n#!\n",
        ";#!\n",
        "{#!\n}",
        "# !\n",
    ] {
        let error = parse(source).expect_err(source);
        assert!(error.known_syntax, "{source:?}: {error:?}");
    }
    let error = parse_module(" #!\n").expect_err("module");
    assert!(error.known_syntax, "{error:?}");
}

#[test]
fn eval_source_may_start_with_a_hashbang() {
    assert_eq!(evaluate("eval('#!\\n1')"), Value::Number(1.0));
    assert_eq!(evaluate("(0, eval)('#!\\n2')"), Value::Number(2.0));
    assert_eq!(evaluate("eval('#!')"), Value::Undefined);
}

#[test]
fn a_dynamic_function_body_cannot_contain_a_hashbang() {
    assert_eq!(
        evaluate("try { Function('#!\\n'); 0 } catch (e) { e instanceof SyntaxError ? 1 : 2 }"),
        Value::Number(1.0)
    );
}
