// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{compile, parse, JsString, RuntimeError, Value, Vm, VmConfig};

fn evaluate(source: &str) -> Result<Value, RuntimeError> {
    Vm::default().execute(&compile(&parse(source).unwrap()).unwrap())
}

#[test]
fn escape_encodes_literal_text_by_ecmascript_categories() {
    for (input, expected) in [
        ("''", ""),
        ("'foo'", r"\x66oo"),
        ("'9abc'", r"\x39abc"),
        ("'AZ09'", r"\x41Z09"),
        ("'^$\\\\.*+?()[]{}|/'", r"\^\$\\\.\*\+\?\(\)\[\]\{\}\|\/"),
        (
            r#"',-=<>#&!%:;@~\'`"'"#,
            r"\x2c\x2d\x3d\x3c\x3e\x23\x26\x21\x25\x3a\x3b\x40\x7e\x27\x60\x22",
        ),
        (r"'\t\n\v\f\r '", r"\t\n\v\f\r\x20"),
        (
            r"'\u00a0\u1680\u2000\u2028\u2029\u202f\u205f\u3000\ufeff'",
            r"\xa0\u1680\u2000\u2028\u2029\u202f\u205f\u3000\ufeff",
        ),
        (r"'\ud800\udbff_\udc00\udfff'", r"\ud800\udbff_\udc00\udfff"),
        ("'😀𐀀􏿿é中文_'", "😀𐀀􏿿é中文_"),
        (
            r"'\x00\x08\x1f\x7f\u0085\u180e\u200b'",
            "\u{0}\u{8}\u{1f}\u{7f}\u{85}\u{180e}\u{200b}",
        ),
    ] {
        assert_eq!(
            evaluate(&format!("RegExp.escape({input})")).unwrap(),
            Value::String(expected.into()),
            "{input}"
        );
    }
}

#[test]
fn escape_rejects_non_strings_without_coercion() {
    for input in [
        "",
        "undefined",
        "null",
        "1",
        "true",
        "Symbol()",
        "new String('x')",
        "{toString(){throw 1;}}",
        "{[Symbol.toPrimitive](){throw 1;}}",
    ] {
        assert!(
            matches!(
                evaluate(&format!("RegExp.escape({input})")),
                Err(RuntimeError::TypeError(_))
            ),
            "{input}"
        );
    }
    for source in [
        "let e=RegExp.escape; e.call(null,'abc') === '\\\\x61bc'",
        "let d=Object.getOwnPropertyDescriptor(RegExp,'escape'); d.writable && !d.enumerable && d.configurable && d.value.length === 1 && d.value.name === 'escape' && d.value.prototype === undefined",
    ] {
        assert_eq!(evaluate(source).unwrap(), Value::Bool(true), "{source}");
    }
    assert!(matches!(
        evaluate("new RegExp.escape('x')"),
        Err(RuntimeError::TypeError(_))
    ));
}

#[test]
fn escaped_text_composes_with_regexps_and_string_protocols() {
    for source in [
        "let s='a+b[0]-😀\\ud800'; let p=RegExp.escape(s); let ok=true; for(let f of ['', 'u', 'v']){ok=ok && new RegExp('^'+p+'$',f).test(s);} ok",
        "new RegExp('(a)\\\\1'+RegExp.escape('1')).test('aa1')",
        "new RegExp('\\\\x0'+RegExp.escape('a')).test('x0a')",
        "let s='a+b'; ('x'+s+'x'+s).replaceAll(new RegExp(RegExp.escape(s),'g'),'!') === 'x!x!'",
        "let s='-'; 'a-b-c'.split(new RegExp(RegExp.escape(s),'u')).join('|') === 'a|b|c'",
        "let s='a.b'; 'a.b ab a.b'.match(new RegExp(RegExp.escape(s),'g')).join('|') === 'a.b|a.b'",
    ] {
        assert_eq!(evaluate(source).unwrap(), Value::Bool(true), "{source}");
    }
}

#[test]
fn escape_checks_output_growth_and_instruction_budget() {
    let mut vm = Vm::new(VmConfig {
        max_string_bytes: 64,
        ..Default::default()
    })
    .unwrap();
    let code = compile(&parse("RegExp.escape(' '.repeat(8))").unwrap()).unwrap();
    assert_eq!(
        vm.execute(&code).unwrap(),
        Value::String(JsString::from(r"\x20".repeat(8)))
    );
    let code = compile(&parse("RegExp.escape(' '.repeat(9))").unwrap()).unwrap();
    assert_eq!(
        vm.execute(&code),
        Err(RuntimeError::StringLimit { limit: 64 })
    );
    let mut vm = Vm::new(VmConfig {
        instruction_budget: 100,
        ..Default::default()
    })
    .unwrap();
    let code = compile(&parse("RegExp.escape('x'.repeat(1000))").unwrap()).unwrap();
    assert_eq!(vm.execute(&code), Err(RuntimeError::InstructionLimit));
}

#[test]
fn escape_preserves_surrogate_pair_boundaries_and_every_lone_surrogate() {
    let mut vm = Vm::default();
    for unit in 0xd800..=0xdfff {
        let code = compile(&parse(&format!("RegExp.escape('\\u{unit:04x}')")).unwrap()).unwrap();
        assert_eq!(
            vm.execute(&code).unwrap(),
            Value::String(format!("\\u{unit:04x}").into())
        );
    }
    for (input, expected) in [
        (r"'\ud800\udc00'", "𐀀"),
        (r"'\udbff\udfff'", "􏿿"),
        (r"'\ud800\ud800\udc00'", "\\ud800𐀀"),
        (r"'\ud800\udc00\udc00'", "𐀀\\udc00"),
        (r"'\udc00\ud800'", r"\udc00\ud800"),
        (r"'\udc00\ud800\udc00\ud800'", "\\udc00𐀀\\ud800"),
    ] {
        assert_eq!(
            evaluate(&format!("RegExp.escape({input})")).unwrap(),
            Value::String(expected.into())
        );
    }
}
