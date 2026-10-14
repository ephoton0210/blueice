// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ReservedWords are not IdentifierReferences (§13.1.1), whether spelled
//! plainly or with a Unicode escape, in a shorthand property or an ordinary
//! reference; the escaped spelling of a keyword is never that keyword, but is
//! still a valid IdentifierName (a property name).

use blueice_bluejs::{compile, parse, Value, Vm};

fn is_rejected(source: &str) -> bool {
    match parse(source) {
        Err(error) => {
            assert!(error.known_syntax, "{source:?}: {error:?}");
            true
        }
        Ok(program) => compile(&program).is_err(),
    }
}

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap()
}

#[test]
fn a_keyword_spelled_with_an_escape_is_not_that_keyword() {
    for source in [
        "tru\\u{65};",
        "fals\\u{65};",
        "n\\u{75}ll;",
        "th\\u0069s;",
        "\\u0069f (1) ;",
        "x = \\u0077ith;",
        "var tru\\u{65};",
        "for (var i = 0; i \\u0069n {}; ) ;",
        "typ\\u0065of 1;",
    ] {
        assert!(is_rejected(source), "{source}");
    }
}

#[test]
fn a_reserved_word_cannot_be_a_shorthand_property() {
    for word in [
        "true", "false", "null", "this", "if", "for", "class", "with", "enum", "export", "extends",
        "super", "debugger", "import", "new", "typeof",
    ] {
        assert!(is_rejected(&format!("({{ {word} }});")), "{word}");
    }
}

#[test]
fn strict_reserved_words_cannot_be_shorthand_properties_in_strict_code() {
    for word in [
        "implements",
        "interface",
        "package",
        "private",
        "protected",
        "public",
        "static",
        "yield",
        "let",
    ] {
        assert!(!is_rejected(&format!("({{ {word} }});")), "sloppy {word}");
        assert!(
            is_rejected(&format!("\"use strict\"; ({{ {word} }});")),
            "strict {word}"
        );
    }
}

#[test]
fn yield_and_await_shorthands_follow_their_context() {
    assert!(is_rejected("function* g() { ({ yield }); }"));
    assert!(is_rejected("async function f() { ({ await }); }"));
    assert!(!is_rejected("({ yield });"));
    assert!(!is_rejected("({ await });"));
    assert!(!is_rejected("function f() { ({ await }); }"));
}

#[test]
fn await_is_reserved_in_a_class_static_block_including_arrow_parameters() {
    for source in [
        "class C { static { ({ await }); } }",
        "class C { static { (await => 0); } }",
        "class C { static { ((x = await) => 0); } }",
        "class C { static { await; } }",
    ] {
        assert!(is_rejected(source), "{source}");
    }
    // A nested ordinary function has its own [Await] context again.
    for source in [
        "class C { static { (function () { ({ await }); }); } }",
        "class C { static { (function await() {}); } }",
    ] {
        assert!(!is_rejected(source), "{source}");
    }
}

#[test]
fn new_target_cannot_contain_an_escape() {
    assert!(is_rejected("function f() { n\\u0065w.target; }"));
    assert!(is_rejected("function f() { new.t\\u0061rget; }"));
    assert!(!is_rejected("function f() { new.target; }"));
}

#[test]
fn an_escaped_reserved_word_is_still_a_property_name() {
    assert_eq!(
        evaluate("({ tru\\u{65}: 1, \\u0069f: 2, cl\\u0061ss: 3 }).true"),
        Value::Number(1.0)
    );
    assert_eq!(evaluate("({ \\u0069f: 2 }).if"), Value::Number(2.0));
    assert_eq!(
        evaluate("var o = { for: 4 }; o.f\\u006fr"),
        Value::Number(4.0)
    );
    assert_eq!(
        evaluate("var o = { true: 5 }; o.tru\\u{65}"),
        Value::Number(5.0)
    );
}

#[test]
fn an_escaped_let_is_an_identifier_reference_not_a_declaration() {
    assert_eq!(
        evaluate("this.let = 7; l\\u0065t // ASI\na; var a; let"),
        Value::Number(7.0)
    );
}

#[test]
fn a_reserved_word_cannot_be_a_label() {
    for source in [
        "tru\\u0065: ;",
        "fals\\u0065: ;",
        "n\\u0075ll: ;",
        "if: ;",
        "class: ;",
        "\"use strict\"; static: ;",
        "\"use strict\"; l\\u0065t: ;",
        "function* g() { yield: ; }",
        "async function f() { await: ; }",
    ] {
        assert!(is_rejected(source), "{source}");
    }
    for source in ["yield: ;", "await: ;", "static: ;", "a: b: ;"] {
        assert!(!is_rejected(source), "{source}");
    }
}

#[test]
fn an_escaped_let_cannot_be_a_strict_binding() {
    for source in [
        "\"use strict\"; var l\\u0065t = 1;",
        "\"use strict\"; var x = ({ l\\u0065t }) => {};",
        "\"use strict\"; function f(l\\u0065t) {}",
    ] {
        assert!(is_rejected(source), "{source}");
    }
    assert!(!is_rejected("var l\\u0065t = 1;"));
}

#[test]
fn a_debugger_statement_does_nothing() {
    assert_eq!(
        evaluate("var n = 0; debugger; n++; debugger\nn++; n"),
        Value::Number(2.0)
    );
    assert_eq!(
        evaluate("if (false) debugger; else debugger; 5"),
        Value::Number(5.0)
    );
    assert_eq!(evaluate("while (false) debugger; 6"), Value::Number(6.0));
}

#[test]
fn a_debugger_statement_is_not_an_expression_and_cannot_be_escaped() {
    for source in [
        "d\\u0065bugger;",
        "x = debugger;",
        "(debugger);",
        "debugger: ;",
        "debugger = 1;",
    ] {
        assert!(is_rejected(source), "{source}");
    }
}
