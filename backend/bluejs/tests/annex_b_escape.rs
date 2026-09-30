// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ECMA-262 Annex B.2.1: the global `escape` and `unescape` functions.

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm, VmConfig};

fn evaluate(source: &str) -> Result<Value, RuntimeError> {
    Vm::default().execute(&compile(&parse(source).unwrap()).unwrap())
}

fn string(source: &str) -> Value {
    evaluate(source).unwrap()
}

#[test]
fn escape_keeps_the_unescaped_set_and_encodes_everything_else() {
    for (input, expected) in [
        (r"''", r"''"),
        (r"'AZaz09@*_+-./'", r"'AZaz09@*_+-./'"),
        (r##"' !"#$%&()'"##, r"'%20%21%22%23%24%25%26%28%29'"),
        (r"'\u00e9\u00ff\u0100'", r"'%E9%FF%u0100'"),
        (r"'\ud800\udc00'", r"'%uD800%uDC00'"),
        (r"'\x00\x7f\x80'", r"'%00%7F%80'"),
        (r"'\uffff'", r"'%uFFFF'"),
    ] {
        assert_eq!(
            string(&format!("escape({input})==={expected}")),
            Value::Bool(true),
            "{input}"
        );
    }
}

#[test]
fn unescape_decodes_two_and_four_digit_sequences_and_ignores_malformed_ones() {
    for (input, expected) in [
        (r"''", r"''"),
        (r"'%41%42'", r"'AB'"),
        (r"'%u0041%u00e9%uD800'", r"'A\u00e9\uD800'"),
        (r"'%'", r"'%'"),
        (r"'%4'", r"'%4'"),
        (r"'%zz%4g'", r"'%zz%4g'"),
        (r"'%u004'", r"'%u004'"),
        (r"'%u00zz'", r"'%u00zz'"),
        (r"'%U0041'", r"'%U0041'"),
        (r"'%%41'", r"'%A'"),
        (r"'%u%u0041'", r"'%uA'"),
        (r"'abc%2'", r"'abc%2'"),
        (r"'%e9%FF'", r"'\u00e9\u00ff'"),
    ] {
        assert_eq!(
            string(&format!("unescape({input})==={expected}")),
            Value::Bool(true),
            "{input}"
        );
    }
}

#[test]
fn escape_and_unescape_are_non_constructor_global_functions() {
    for name in ["escape", "unescape"] {
        for check in [
            "typeof NAME==='function'",
            "NAME.length===1",
            "NAME.name==='NAME'",
            "Object.getOwnPropertyDescriptor(globalThis,'NAME').writable===true",
            "Object.getOwnPropertyDescriptor(globalThis,'NAME').enumerable===false",
            "Object.getOwnPropertyDescriptor(globalThis,'NAME').configurable===true",
            "Object.getOwnPropertyNames(globalThis).includes('NAME')",
            "!Object.prototype.hasOwnProperty.call(NAME,'prototype')",
            "(function(){try{new NAME('');return false}catch(e){return e instanceof TypeError}})()",
        ] {
            let source = check.replace("NAME", name);
            assert_eq!(string(&source), Value::Bool(true), "{source}");
        }
    }
}

#[test]
fn escape_and_unescape_coerce_their_argument_once_with_to_string() {
    assert_eq!(
        string("var n=0;var o={toString:function(){n++;return '%u0041'}};unescape(o)==='A'&&n===1"),
        Value::Bool(true)
    );
    assert_eq!(
        string("escape(undefined)==='undefined'&&escape(null)==='null'&&escape(12)==='12'"),
        Value::Bool(true)
    );
    assert_eq!(
        string("(function(){try{escape(Symbol())}catch(e){return e instanceof TypeError}})()"),
        Value::Bool(true)
    );
}

#[test]
fn escape_and_unescape_respect_the_runtime_string_limit() {
    let config = VmConfig {
        max_string_bytes: 100,
        ..VmConfig::default()
    };
    // Twenty spaces are 40 bytes; their escape is 60 code units (120 bytes).
    let mut vm = Vm::new(config).unwrap();
    let result = vm.execute(&compile(&parse("escape(' '.repeat(20))").unwrap()).unwrap());
    assert_eq!(result, Err(RuntimeError::StringLimit { limit: 100 }));
    // Decoding only ever shrinks, so the same text round-trips within the limit.
    let mut vm = Vm::new(config).unwrap();
    let source = "unescape('%20'.repeat(10))===' '.repeat(10)";
    assert_eq!(
        vm.execute(&compile(&parse(source).unwrap()).unwrap()),
        Ok(Value::Bool(true))
    );
}
