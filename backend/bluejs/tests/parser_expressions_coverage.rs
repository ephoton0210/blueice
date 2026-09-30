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

#[test]
fn incomplete_operator_operands_and_member_forms_are_syntax_errors() {
    for source in [
        "value = ;",
        "value || ;",
        "value && ;",
        "value | ;",
        "value ^ ;",
        "value & ;",
        "value == ;",
        "value < ;",
        "value << ;",
        "value + ;",
        "value * ;",
        "value ** ;",
        "!;",
        "~;",
        "typeof ;",
        "void ;",
        "delete ;",
        "new ;",
        "value ? : other;",
        "value ? other;",
        "value[;",
        "value.;",
        "value?.;",
        "new value.;",
        "fn(,);",
        "fn(...);",
        "[...];",
        "({ ... });",
        "({ key: });",
        "[item =] = source;",
        "({ item = } = source);",
    ] {
        let error = parse(source).expect_err(source);
        assert!(error.known_syntax, "{source}: {error:?}");
    }
}

#[test]
fn nested_expression_delimiters_report_known_syntax_errors() {
    for source in [
        "[left right] = values;",
        "[left] = ;",
        "({ left: right other } = values);",
        "({ [ ]: value } = values);",
        "[left =] = values;",
        "({ left: right = } = values);",
        "value?.[index extra];",
        "value?.[;",
        "value?.(,);",
        "value[index extra];",
        "value ?? ;",
        "-;",
        "new Constructor[index extra];",
        "new Constructor[;",
        "new Constructor(,);",
        "new new ;",
        "call(first second);",
        "[first second];",
        "({ first: second third });",
        "import('module', options extra);",
        "import(;",
        "tag`${yield 1}`;",
        "new tag`${yield 1}`;",
        "function* g() { yield* ; }",
        "@decorator class C {",
        "let C = @;",
        "let C = @decorator 1;",
    ] {
        let error = parse(source).expect_err(source);
        assert!(error.known_syntax, "{source}: {error:?}");
    }
    for source in [
        "const value = import(;",
        "const value = import.source(;",
        "const value = object?.[index extra];",
        "@decorator class C {",
    ] {
        let error = parse_module(source).expect_err(source);
        assert!(error.known_syntax, "{source}: {error:?}");
    }
}
