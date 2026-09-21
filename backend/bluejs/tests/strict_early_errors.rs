// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Early errors that apply only to strict mode code (§13.1.1, §12.9.3,
//! §12.9.4.1 and Annex B's carve-outs), as source text exercised through the
//! public parse/compile pipeline.

use blueice_bluejs::{compile, parse, Value, Vm};

/// The source is rejected before running: a parse or a compile error.
fn is_rejected(source: &str) -> bool {
    match parse(source) {
        Err(_) => true,
        Ok(program) => compile(&program).is_err(),
    }
}

fn assert_strict_rejected(body: &str) {
    assert!(is_rejected(&format!("\"use strict\";\n{body}")), "strict: {body}");
}

fn assert_sloppy_accepted(body: &str) {
    assert!(!is_rejected(body), "sloppy: {body}");
}

#[test]
fn eval_and_arguments_cannot_be_declared_with_var_in_strict_code() {
    for body in [
        "var eval;",
        "var arguments;",
        "var a, eval, b;",
        "var eval = 1;",
        "var a = 0, arguments = 1;",
        "var [eval] = [];",
        "var {arguments} = {};",
        "for (var eval in null) {}",
        "for (var arguments of []) {}",
        "for (var eval = 0; ; ) {}",
        "function f() { var arguments; }",
        "function f() { for (var arguments in null) {} }",
        "function f() { let eval; }",
        "{ const arguments = 0; }",
    ] {
        assert_strict_rejected(body);
    }
}

#[test]
fn eval_and_arguments_are_ordinary_var_names_in_sloppy_code() {
    for body in [
        "var eval;",
        "var arguments;",
        "for (var eval in null) {}",
        "function f() { var arguments; }",
    ] {
        assert_sloppy_accepted(body);
    }
}

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap()
}

#[test]
fn legacy_octal_and_leading_zero_decimal_literals_are_sloppy_only() {
    for body in [
        "010;",
        "00;",
        "01;",
        "07;",
        "08;",
        "09;",
        "019;",
        "08.5;",
        "var a = 0x1; a = 01;",
        "({ 010: 1 });",
        "({ 08: 1 });",
        "class C { 010() {} }",
        "var { 010: a } = {};",
        "function f() { return 010; }",
    ] {
        assert_strict_rejected(body);
        if !body.starts_with("class") {
            assert_sloppy_accepted(body);
        }
    }
    for body in ["0;", "0.5;", "0e1;", "0x10;", "0b1;", "0o7;", "0n;", "10;", ".5;", "0.0;"] {
        assert!(!is_rejected(&format!("\"use strict\";\n{body}")), "{body}");
    }
    assert_eq!(evaluate("010 + 08 + 08.5"), Value::Number(8.0 + 8.0 + 8.5));
}

#[test]
fn a_function_level_use_strict_directive_applies_to_the_function_body() {
    for source in [
        "function f() { \"use strict\"; return 010; }",
        "function f() { 'use strict'; 08; }",
        "(function() { \"use strict\"; 010; })",
        "(() => { \"use strict\"; 010; })",
        "({ m() { \"use strict\"; 010; } })",
        "({ get x() { \"use strict\"; public = 42; } })",
        "({ set x(value) { \"use strict\"; public = 42; } })",
        "\"use strict\"; ({ get x() { public = 42; } })",
        "\"use strict\"; ({ set x(value) { public = 42; } })",
    ] {
        assert!(is_rejected(source), "{source}");
    }
    for source in [
        "function f() { return 010; }",
        "function f() { \"use strict\"; } function g() { return 010; }",
        "({ get x() { public = 42; } })",
    ] {
        assert!(!is_rejected(source), "{source}");
    }
}

#[test]
fn only_an_exact_unescaped_use_strict_string_is_a_directive() {
    let strict = |body: &str| {
        evaluate(&format!("(function() {{ {body} return this === undefined; }})()"))
    };
    assert_eq!(strict("\"use strict\";"), Value::Bool(true));
    assert_eq!(strict("'use strict'\n"), Value::Bool(true));
    assert_eq!(strict("'use\\u0020strict';"), Value::Bool(false));
    assert_eq!(strict("'use str\\\nict';"), Value::Bool(false));
    assert_eq!(strict("('use strict');"), Value::Bool(false));
    assert_eq!(strict("'use strict' + '';"), Value::Bool(false));
    assert_eq!(strict("'other'; 'use strict';"), Value::Bool(true));
    assert_eq!(strict("1; 'use strict';"), Value::Bool(false));
}

#[test]
fn a_legacy_octal_escape_in_a_prologue_before_use_strict_is_an_error() {
    for source in [
        "'\\1'; 'use strict';",
        "'\\08'; 'use strict';",
        "'\\8'; 'use strict';",
        "'a'; '\\052'; 'use strict';",
        "(function() { 'asterisk: \\052'; 'use strict'; });",
        "function f() { '\\1'; \"use strict\"; }",
        "({ m() { '\\1'; 'use strict'; } })",
    ] {
        assert!(is_rejected(source), "{source}");
    }
    for source in [
        "'\\1'; 'a';",
        "'\\1'; f(); 'use strict';",
        "function f() { '\\1'; }",
        "'use strict'; 'a';",
        "'\\0'; 'use strict';",
    ] {
        assert!(!is_rejected(source), "{source}");
    }
}

#[test]
fn direct_eval_inherits_caller_strictness_for_syntax() {
    let outcome = |caller: &str, code: &str| {
        evaluate(&format!(
            "(function() {{ {caller} try {{ eval({code:?}); return 0; }} catch (e) {{ return e instanceof SyntaxError ? 1 : 2; }} }})()"
        ))
    };
    assert_eq!(outcome("'use strict';", "a = 0x1; a = 01;"), Value::Number(1.0));
    assert_eq!(outcome("'use strict';", "public = 1;"), Value::Number(1.0));
    assert_eq!(outcome("'use strict';", "var arguments;"), Value::Number(1.0));
    assert_eq!(outcome("", "'use strict'; 010;"), Value::Number(1.0));
    assert_eq!(outcome("", "var a; a = 01;"), Value::Number(0.0));
    assert_eq!(outcome("", "var public;"), Value::Number(0.0));
}
