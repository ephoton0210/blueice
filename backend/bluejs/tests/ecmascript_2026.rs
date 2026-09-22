// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Edition 17 corrective slices, through public interfaces only.
use blueice_bluejs::{compile, parse, HeapConfig, RuntimeError, Value, Vm, VmConfig};

fn evaluate(source: &str) -> Result<Value, RuntimeError> {
    Vm::default().execute(&compile(&parse(source).expect(source)).expect(source))
}

#[test]
fn lexical_reads_typeof_writes_and_updates_are_in_the_tdz() {
    for source in [
        "x; let x",
        "typeof x; let x",
        "x=1; let x",
        "x+=1; let x",
        "x++; let x",
        "++x; let x",
        "x=1; const x=2",
        "typeof x; const x=2",
        "let x=x",
        "const x=x",
        "let x=1; {x; let x=2;}",
        "let x=1; {typeof x; const x=2;}",
        "let x=y, y=2",
        "for(let i=i;i<1;i++){}",
        "let i=0; while(i++<2){if(i===2)x; let x=7;}",
    ] {
        assert!(
            matches!(evaluate(source), Err(RuntimeError::ReferenceError(_))),
            "{source}"
        );
    }
    assert_eq!(
        evaluate("x=missing; let x"),
        Err(RuntimeError::ReferenceError("missing".into()))
    );
    assert_eq!(
        evaluate("x+=missing; let x"),
        Err(RuntimeError::ReferenceError("x".into()))
    );
}

#[test]
fn initialized_undefined_var_hoisting_and_loop_reentry_remain_distinct() {
    for (source, value) in [
        ("x; var x", Value::Undefined),
        ("let x; x", Value::Undefined),
        ("let x; typeof x", Value::String("undefined".into())),
        ("typeof neverDeclared", Value::String("undefined".into())),
        ("let x=1,y=x; y", Value::Number(1.0)),
        ("let x=1; {let x=2;} x", Value::Number(1.0)),
        ("let x=false&&x; x", Value::Bool(false)),
        ("let x=true?7:x; x", Value::Number(7.0)),
        (
            "let n=0; for(let i=0;i<3;i++){let x=i; n+=x;} n",
            Value::Number(3.0),
        ),
        (
            "let n=0; while(n<3){let x=n; n++; if(x<2)continue; break;} n",
            Value::Number(3.0),
        ),
    ] {
        assert_eq!(evaluate(source).unwrap(), value, "{source}");
    }
    assert!(matches!(
        evaluate("const x=1; x=2"),
        Err(RuntimeError::TypeError(_))
    ));
}

#[test]
fn tdz_failure_releases_roots_and_does_not_poison_reusable_bytecode() {
    let mut vm = Vm::new(VmConfig {
        heap: HeapConfig {
            nursery_capacity: 1,
            ..HeapConfig::default()
        },
        ..VmConfig::default()
    })
    .unwrap();
    let baseline = vm.heap().stats().managed_bytes;
    let failed = compile(&parse("let keep={child:{}}; keep; x={}; let x").unwrap()).unwrap();
    let success = compile(&parse("let x; x={child:{n:7}}; x.child.n").unwrap()).unwrap();
    for _ in 0..3 {
        assert!(matches!(
            vm.execute(&failed),
            Err(RuntimeError::ReferenceError(_))
        ));
        assert_eq!(vm.heap().stats().managed_bytes, baseline);
        assert_eq!(vm.execute(&success).unwrap(), Value::Number(7.0));
        assert_eq!(vm.heap().stats().managed_bytes, baseline);
    }
}

#[test]
fn nullish_mixing_requires_parentheses_at_the_current_grammar_level() {
    for source in [
        "1??2||3",
        "1||2??3",
        "1??2&&3",
        "1&&2??3",
        "1??2??3||4",
        "false&&(1??2||3)",
        "true?1:2??3||4",
        "`${1??2&&3}`",
    ] {
        assert!(parse(source).is_err(), "{source}");
    }
    for (source, value) in [
        ("(0||2)??3", 2.0),
        ("0||(null??3)", 3.0),
        ("null??(0||3)", 3.0),
        ("(null??0)||3", 3.0),
        ("null??(2&&3)", 3.0),
        ("(null??2)&&3", 3.0),
        ("null??undefined??7", 7.0),
        ("null??(true?0||4:5)", 4.0),
        ("null??[0||4][0]", 4.0),
    ] {
        assert_eq!(evaluate(source).unwrap(), Value::Number(value), "{source}");
    }
}

#[test]
fn number_literals_support_radices_separators_and_single_rounding() {
    for (source, expected) in [
        ("0xFF", 255.0f64),
        ("0Xff", 255.0),
        ("0b101", 5.0),
        ("0B101", 5.0),
        ("0o77", 63.0),
        ("0O77", 63.0),
        ("0xAB_CD", 43981.0),
        ("0b1_0_1", 5.0),
        ("0o7_7", 63.0),
        ("1_234.5_6e1_0", 12345600000000.0),
        (".1_25", 0.125),
        ("0.1_25", 0.125),
        ("1_0.", 10.0),
        ("1_0.e+2", 1000.0),
        ("1e-4_00", 0.0),
        ("0x20000000000001", 9007199254740992.0),
        ("0x20000000000003", 9007199254740996.0),
        ("077", 63.0),
        ("000", 0.0),
        ("08", 8.0),
        ("009", 9.0),
        ("08.5", 8.5),
        ("08e1", 80.0),
        ("({0x10:7})[16]", 7.0),
        ("1_0+0xF", 25.0),
    ] {
        let Value::Number(actual) = evaluate(source).unwrap() else {
            panic!("expected number: {source}")
        };
        assert_eq!(actual.to_bits(), expected.to_bits(), "{source}");
    }
    let huge = format!("0x1{}", "0".repeat(256));
    assert_eq!(evaluate(&huge).unwrap(), Value::Number(f64::INFINITY));
}

#[test]
fn malformed_numeric_tokens_are_rejected_before_parsing_statements() {
    for source in [
        "0x", "0b", "0o", "0b2", "0o8", "0xG", "0b12", "0o78", "1e", "1e+", "1.e-", "0_1", "00_1",
        "01_2", "08_9", "1__0", "1_", "1_.0", "1._0", "1_e2", "1e_2", "1e+_2", "1e2_", "0x_FF",
        "0b_1", "0o_7", "0xF_", "0o7__1", "123abc", "3in", "1$", "1\\u0061", "077e1", "077.5",
    ] {
        assert!(parse(source).is_err(), "{source}");
    }
    // Separators belong to source literals, never StringNumericLiteral.
    assert!(matches!(evaluate("+'1_000'").unwrap(), Value::Number(n) if n.is_nan()));
}

#[test]
fn all_four_line_terminators_drive_asi_and_end_line_comments() {
    for newline in ["\n", "\r", "\r\n", "\u{2028}", "\u{2029}"] {
        assert_eq!(
            evaluate(&format!("let x=1{newline}x+2")).unwrap(),
            Value::Number(3.0)
        );
        assert_eq!(
            evaluate(&format!("let x=1// ignored{newline}x+2")).unwrap(),
            Value::Number(3.0)
        );
        assert_eq!(
            evaluate(&format!("let x=1/*{newline}*/x+2")).unwrap(),
            Value::Number(3.0)
        );
        assert_eq!(
            evaluate(&format!("let x=1; let y=2; x{newline}++y; y")).unwrap(),
            Value::Number(3.0)
        );
        assert!(parse(&format!("throw{newline}1")).is_err());
        assert_eq!(
            evaluate(&format!("'a\\{newline}b'")).unwrap(),
            Value::String("ab".into())
        );
    }
}

#[test]
fn quoted_strings_and_templates_apply_their_different_newline_rules() {
    for newline in ["\n", "\r", "\r\n"] {
        assert!(parse(&format!("'a{newline}b'")).is_err());
        assert_eq!(
            evaluate(&format!("`a{newline}b`")).unwrap(),
            Value::String("a\nb".into())
        );
        assert_eq!(
            evaluate(&format!("`a\\{newline}b`")).unwrap(),
            Value::String("ab".into())
        );
    }
    for newline in ["\u{2028}", "\u{2029}"] {
        assert_eq!(
            evaluate(&format!("'a{newline}b'")).unwrap(),
            Value::String(format!("a{newline}b").into())
        );
        assert_eq!(
            evaluate(&format!("`a{newline}b`")).unwrap(),
            Value::String(format!("a{newline}b").into())
        );
    }
}

#[test]
fn whitespace_uses_ecmascript_not_rust_unicode_classification() {
    for space in [
        '\t', '\u{000b}', '\u{000c}', ' ', '\u{00a0}', '\u{1680}', '\u{2000}', '\u{2007}',
        '\u{202f}', '\u{205f}', '\u{3000}', '\u{feff}',
    ] {
        assert_eq!(
            evaluate(&format!("{space}1{space}+{space}2")).unwrap(),
            Value::Number(3.0)
        );
    }
    for invalid in ['\u{0085}', '\u{180e}', '\u{200b}'] {
        assert!(parse(&format!("1{invalid}+2")).is_err());
    }
}

#[test]
fn template_placeholder_comments_cannot_close_the_placeholder() {
    for newline in ["\n", "\r", "\r\n", "\u{2028}", "\u{2029}"] {
        assert_eq!(
            evaluate(&format!("`${{1 // }} ` ' ignored{newline}+2}}`")).unwrap(),
            Value::String("3".into())
        );
        assert_eq!(
            evaluate(&format!("`${{'a\\{newline}b'}}`")).unwrap(),
            Value::String("ab".into())
        );
    }
    assert_eq!(
        evaluate("`${1/* } { ` ' */+2}`").unwrap(),
        Value::String("3".into())
    );
    assert_eq!(
        evaluate("`${`nested ${1/* } */+2}`}`").unwrap(),
        Value::String("nested 3".into())
    );
    assert!(parse("`${1/* unterminated }`").is_err());
    assert!(parse("`${'a\rb'}`").is_err());
}
