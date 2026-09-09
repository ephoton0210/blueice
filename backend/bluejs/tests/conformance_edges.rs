// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public-interface regressions from the coverage/conformance review.
use blueice_bluejs::{compile, compile_with_limit, parse, CompileError, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default().execute(&compile(&parse(source).unwrap()).unwrap()).unwrap()
}

#[test]
fn all_supported_keywords_work_as_literal_and_member_property_names() {
    for keyword in [
        "var",
        "let",
        "const",
        "function",
        "return",
        "if",
        "else",
        "for",
        "while",
        "do",
        "switch",
        "case",
        "default",
        "break",
        "continue",
        "throw",
        "try",
        "catch",
        "finally",
        "new",
        "typeof",
        "instanceof",
        "in",
        "true",
        "false",
        "null",
        "this",
    ] {
        let source = format!("let o={{{keyword}:7}}; o.{keyword}+=2; o['{keyword}']");
        assert_eq!(evaluate(&source), Value::Number(9.0), "{keyword}");
    }
}

#[test]
fn nested_parentheses_and_ordinary_length_do_not_take_special_paths() {
    assert_eq!(evaluate("(((1+2)*3))"), Value::Number(9.0));
    assert_eq!(evaluate("let o={length:'text'}; o.length"), Value::String("text".into()));
    assert_eq!(evaluate("let p={length:7}; ({__proto__:p}).length"), Value::Number(7.0));
    assert_eq!(evaluate("typeof undefined"), Value::String("undefined".into()));
    assert_eq!(evaluate("let x; typeof x"), Value::String("undefined".into()));
}

#[test]
fn malformed_hexadecimal_escapes_are_rejected_instead_of_accepting_signs() {
    for escape in [r"\x+1", r"\x-0", r"\u+001", r"\u-000", r"\u{+1}", r"\u{-0}", r"\u{}", r"\u{FFFFFFFFF}"] {
        for quote in ['\'', '"', '`'] {
            assert!(parse(&format!("{quote}{escape}{quote}")).is_err(), "{quote}{escape}{quote}");
        }
    }
}

#[test]
fn escape_errors_retain_actionable_diagnostics_through_the_public_parser() {
    for (source, message) in [
        ("'\\", "unterminated escape"),
        ("'\\x", "unterminated \\x"),
        ("'\\x1", "unterminated \\x"),
        (r"'\xGG'", "invalid \\x"),
        ("'\\u", "unterminated \\u"),
        ("'\\u123", "unterminated \\u"),
        (r"'\uZZZZ'", "invalid \\u"),
        ("'\\u{1", "unterminated"),
        (r"'\u{G}'", "invalid \\u"),
        (r"'\u{110000}'", "codepoint"),
    ] {
        let error = parse(source).unwrap_err();
        assert!(error.message.contains(message), "{source}: {error:?}");
    }
    assert_eq!(evaluate(r"'\x00\x7F\xFF\u0041\u{1F600}'"), Value::String("\0\u{7f}ÿA😀".into()));
    assert_eq!(evaluate(r"'\u{00000000000000000000000000000041}'"), Value::String("A".into()));
}

#[test]
fn utf8_storage_still_explicitly_rejects_surrogate_code_units() {
    // These are valid ECMAScript strings, but Rust String cannot represent
    // them. Track the existing representation gap, not a spec SyntaxError.
    for source in [r"'\uD800'", r"'\u{DFFF}'", r"'\uD83D\uDE00'"] {
        assert!(parse(source).unwrap_err().message.contains("codepoint"), "{source}");
    }
}

#[test]
fn bytecode_budget_is_inclusive_and_preserves_default_output() {
    for source in ["", "1", "-1", "true||false", "false&&true", "null??1", "let a=[1,2]; for(let i=0;i<a.length;i++){a[i]++;} a[1]"] {
        let ast = parse(source).unwrap();
        let normal = compile(&ast).unwrap();
        let limit = u32::try_from(normal.bytes().len()).unwrap();
        let bounded = compile_with_limit(&ast, limit).unwrap();
        assert_eq!(bounded.bytes(), normal.bytes(), "{source}");
        assert_eq!(bounded.constants(), normal.constants(), "{source}");
        assert_eq!(Vm::default().execute(&bounded), Vm::default().execute(&normal), "{source}");
        for insufficient in 0..limit {
            assert_eq!(compile_with_limit(&ast, insufficient).err().expect("below required size"), CompileError::ProgramTooLarge, "{source}, limit {insufficient}");
        }
    }
}

#[test]
fn bytecode_budget_rejects_partial_instructions_and_propagates_expression_errors() {
    for (source, limit) in [("", 0), ("", 4), ("", 5), ("1", 9), ("-1", 10), ("true||false", 15), ("false&&true", 15), ("null??1", 15)] {
        let error = compile_with_limit(&parse(source).unwrap(), limit).err().expect("insufficient instruction budget");
        assert_eq!(error, CompileError::ProgramTooLarge, "{source}, limit {limit}");
        assert!(error.to_string().contains("bytecode size limit"));
    }
    assert!(compile_with_limit(&parse("").unwrap(), 6).is_ok());
}

#[test]
fn shortest_decimal_midpoints_neighbors_and_presentation_boundaries() {
    // Fixed expectations checked against Number::toString and Node v24.18.0;
    // never derive the expected shortest decimal from Rust's formatter.
    for (literal, expected) in [
        ("1114289515931746.125", "1114289515931746.1"),
        ("1114289515931746.25", "1114289515931746.2"),
        ("1114289515931746.375", "1114289515931746.4"),
        ("1114289515931746.75", "1114289515931746.8"),
        ("1000000000000000.25", "1000000000000000.2"),
        ("1000000000000000.75", "1000000000000000.8"),
        ("100000000000000.015625", "100000000000000.02"),
        ("100000000000000.03125", "100000000000000.03"),
        ("999999999999999.875", "999999999999999.9"),
        ("9007199254740991", "9007199254740991"),
        ("9007199254740992", "9007199254740992"),
        ("9007199254740994", "9007199254740994"),
        ("1e-7", "1e-7"),
        ("1e-6", "0.000001"),
        ("1e20", "100000000000000000000"),
        ("1e21", "1e+21"),
        ("5e-324", "5e-324"),
        ("2.2250738585072014e-308", "2.2250738585072014e-308"),
        ("1.7976931348623157e308", "1.7976931348623157e+308"),
    ] {
        for sign in ["", "-"] {
            assert_eq!(evaluate(&format!("''+({sign}{literal})")), Value::String(format!("{sign}{expected}")), "{sign}{literal}");
        }
    }
}

#[test]
fn decimal_scanning_retains_fraction_exponent_and_overflow_behavior() {
    for (literal, expected) in [("0", 0.0), ("1.", 1.0), (".25", 0.25), ("1.e3", 1000.0), ("1e+309", f64::INFINITY), ("1e-400", 0.0)] {
        assert_eq!(evaluate(literal), Value::Number(expected), "{literal}");
    }
    for malformed in ["1e", "1e+", "1e-", "1.e+"] {
        assert!(parse(malformed).is_err(), "{malformed}");
    }
}

#[test]
fn number_strings_round_trip_at_every_binary_exponent_boundary() {
    let mut vm = Vm::default();
    for exponent in 0u64..2047 {
        for fraction in [1, (1u64 << 52) - 1] {
            for sign in [0, 1u64 << 63] {
                let bits = sign | (exponent << 52) | fraction;
                let n = f64::from_bits(bits);
                let code = compile(&parse(&format!("''+({n:e})")).unwrap()).unwrap();
                let Value::String(text) = vm.execute(&code).unwrap() else { panic!("string concatenation must return a string") };
                assert_eq!(text.parse::<f64>().unwrap().to_bits(), bits, "{bits:016x}: {text}");
            }
        }
    }
}
