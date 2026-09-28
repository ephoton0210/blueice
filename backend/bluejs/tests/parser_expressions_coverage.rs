// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{parse, parse_module};

#[test]
fn expression_grammar_rejects_invalid_await_yield_import_and_optional_tags() {
    for source in [
        "function* g() { void yield 1; }",
        "object?.tag`x`",
        "object?.`x`",
        "import.42",
        "({ *method: 1 })",
        "({ *get value() {} })",
        "({ true } = source);",
        "({ ['key'] } = source);",
        "function* g() { return +yield; }",
    ] {
        assert!(parse(source).is_err(), "{source} should fail to parse");
    }
    for source in [
        "await;",
        "new await value;",
        "a\\u0077ait 1;",
        "const value = import.42;",
    ] {
        assert!(
            parse_module(source).is_err(),
            "{source} should fail to parse as a module"
        );
    }
    assert!(parse_module("const value = import. 42;")
        .expect_err("an import phase name must be an identifier")
        .message
        .starts_with("expected a phase or 'meta' after 'import.'"));
}

#[test]
fn nested_new_and_nested_destructuring_keep_their_grammar_boundaries() {
    parse("new new Constructor()").expect("a nested new expression retains constructor precedence");
    assert!(parse("({ value } = source").is_err());
}
