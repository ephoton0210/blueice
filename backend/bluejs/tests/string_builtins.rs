// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{compile, parse, HeapConfig, HeapError, RuntimeError, Value, Vm, VmConfig};

fn evaluate(source: &str) -> Result<Value, RuntimeError> {
    Vm::default().execute(&compile(&parse(source).unwrap()).unwrap())
}

#[test]
fn constructor_statics_and_native_method_identity() {
    for source in [
        "String() === '' && String(undefined) === 'undefined' && String(null) === 'null'",
        "String(42) === '42' && String(true) === 'true'",
        "typeof String === 'function' && typeof ''.slice === 'function'",
        "String.prototype.constructor === String && ''.slice === String.prototype.slice",
        "String.length === 1 && String.name === 'String' && ''.slice.length === 2",
        r"String.fromCharCode(65, -1, 65536, NaN, Infinity) === 'A\uffff\0\0\0'",
        r"String.fromCodePoint(0x1f600, 0xd800, 0x10ffff) === '😀\ud800\udbff\udfff'",
        "String.fromCharCode() === '' && String.fromCodePoint() === ''",
    ] {
        assert_eq!(evaluate(source).unwrap(), Value::Bool(true), "{source}");
    }
    for source in [
        "String.fromCodePoint(-1)",
        "String.fromCodePoint(0.5)",
        "String.fromCodePoint(NaN)",
        "String.fromCodePoint(Infinity)",
        "String.fromCodePoint(0x110000)",
    ] {
        assert!(
            matches!(evaluate(source), Err(RuntimeError::RangeError(_))),
            "{source}"
        );
    }
}

#[test]
fn indexing_and_slicing_use_code_units() {
    for source in [
        r"'A😀B'.length === 4 && 'A😀B'[1] === '\ud83d' && 'A😀B'[2] === '\ude00'",
        "'abc'[-1] === undefined && 'abc'['01'] === undefined && 'abc'[3] === undefined",
        r"'A😀B'.at(-2) === '\ude00' && 'abc'.at(Infinity) === undefined",
        "'abc'.at(-4) === undefined && 'abc'.at(NaN) === 'a'",
        "'abc'.charAt(-1) === '' && 'abc'.charAt(0.9) === 'a'",
        "'😀'.charCodeAt(0) === 55357 && '😀'.codePointAt(0) === 128512 && '😀'.codePointAt(1) === 56832",
        "'abc'.codePointAt(3) === undefined && 'abc'.charCodeAt(3) !== 'abc'.charCodeAt(3)",
        r"'A😀B'.slice(1, 2) === '\ud83d' && 'abcd'.slice(-3,-1) === 'bc'",
        "'abcd'.slice(3,1) === '' && 'abcd'.substring(3,1) === 'bc'",
        "'abcd'.substring(-2,Infinity) === 'abcd' && 'abcd'.slice(-Infinity,2) === 'ab'",
    ] {
        assert_eq!(evaluate(source).unwrap(), Value::Bool(true), "{source}");
    }
}

#[test]
fn boxed_strings_preserve_identity_and_read_only_virtual_properties() {
    for source in [
        "let s=new String('abc'); typeof s === 'object' && s.length === 3 && s.valueOf() === 'abc'",
        "new String().toString() === '' && String.prototype.valueOf() === ''",
        "let s=new String('abc'); s[0]='z'; s.length=7; s.extra=4; s[0] === 'a' && s.length === 3 && s.extra === 4",
        "String.prototype.toString.call(new String('abc')) === 'abc'",
    ] {
        assert_eq!(evaluate(source).unwrap(), Value::Bool(true), "{source}");
    }
    assert!(matches!(
        evaluate("new String.fromCharCode(65)"),
        Err(RuntimeError::TypeError(_))
    ));
}

#[test]
fn searching_and_well_formedness_preserve_surrogate_boundaries() {
    for source in [
        "'abcabc'.indexOf('bc', 2) === 4 && 'abc'.indexOf('x') === -1",
        "'abcabc'.lastIndexOf('bc') === 4 && 'abcabc'.lastIndexOf('bc', NaN) === 4",
        "'abcabc'.lastIndexOf('bc', 3) === 1 && 'abc'.lastIndexOf('abcd') === -1",
        "'abc'.indexOf('',Infinity) === 3 && 'abc'.lastIndexOf('',-Infinity) === 0",
        "'abc'.startsWith('ab') && 'abc'.endsWith('ab',2) && 'abc'.includes('b',1)",
        "!'abc'.startsWith('ab',1) && !'abc'.endsWith('abcd') && !'abc'.includes('b',2)",
        r"'😀'.includes('\ude00') && '😀'.indexOf('\ude00') === 1",
        r"'😀'.isWellFormed() && !'\ud800'.isWellFormed() && ''.isWellFormed()",
        r"'\udc00\ud800A😀\udfff'.toWellFormed() === '\ufffd\ufffdA😀\ufffd'",
        r"'a'.concat('\ud800', 2, null) === 'a\ud8002null'",
    ] {
        assert_eq!(evaluate(source).unwrap(), Value::Bool(true), "{source}");
    }
}

#[test]
fn calls_preserve_receivers_shadowing_and_evaluation_order() {
    for source in [
        "let o={s:'abc'.slice}; o.s.call('abcd',1,3) === 'bc'",
        "String.prototype.charAt.call(123,1) === '2'",
        "let f=String; {let String=3; f(42) === '42'}",
        "let s='abcd'; s.slice(s=1) === 'bcd'",
    ] {
        assert_eq!(evaluate(source).unwrap(), Value::Bool(true), "{source}");
    }
    for source in [
        "let f='abc'.slice; f(1)",
        "String.prototype.slice.call(null)",
        "String.prototype.valueOf.call(3)",
        "'abc'.missing()",
        "1()",
    ] {
        assert!(
            matches!(evaluate(source), Err(RuntimeError::TypeError(_))),
            "{source}"
        );
    }
}

#[test]
fn padding_repetition_and_trimming_follow_utf16_and_es_whitespace() {
    for source in [
        "'a'.padStart(4,'bc') === 'bcba' && 'a'.padEnd(4,'bc') === 'abcb'",
        r"'a'.padEnd(2,'😀') === 'a\ud83d'",
        "'a'.padStart(3) === '  a' && 'a'.padStart(Infinity,'') === 'a'",
        "'abc'.padEnd(2,{}) === 'abc' && 'a'.padStart(-1) === 'a'",
        r"'\ud800'.repeat(2.9) === '\ud800\ud800'",
        "'a'.repeat(NaN) === '' && 'a'.repeat(-0.5) === '' && ''.repeat(1e300) === ''",
        r"'\uFEFF\u2028 \ud800 \u3000'.trim() === '\ud800'",
        "' a '.trimStart() === 'a ' && ' a '.trimEnd() === ' a'",
        r"'\u0085x\u0085'.trim() === '\u0085x\u0085'",
        "' '.trim() === '' && ''.trimStart() === ''",
    ] {
        assert_eq!(evaluate(source).unwrap(), Value::Bool(true), "{source}");
    }
    for source in ["''.repeat(-1)", "''.repeat(Infinity)"] {
        assert!(
            matches!(evaluate(source), Err(RuntimeError::RangeError(_))),
            "{source}"
        );
    }
    for source in ["'a'.repeat(1e300)", "'a'.padStart(Infinity)"] {
        assert!(
            matches!(evaluate(source), Err(RuntimeError::StringLimit { .. })),
            "{source}"
        );
    }
}

#[test]
fn raw_and_split_use_array_like_properties_and_code_units() {
    for source in [
        "String.raw({raw:['a','b','c']},1,2) === 'a1b2c'",
        "String.raw({raw:{length:2,0:'a',1:'b'}}) === 'ab'",
        "String.raw({raw:'ab'},3) === 'a3b' && String.raw({raw:[]},{}) === ''",
        "String.raw({raw:{length:1.9}}) === 'undefined'",
        "let a='a,b,'.split(','); a.length === 3 && a[0] === 'a' && a[2] === ''",
        "'a,b,c'.split(',',2).length === 2 && 'a'.split(undefined,0).length === 0",
        "'abc'.split()[0] === 'abc' && 'abc'.split(undefined,NaN).length === 0",
        "''.split('').length === 0 && ''.split(',').length === 1",
        r"let a='😀'.split(''); a.length === 2 && a[0] === '\ud83d' && a[1] === '\ude00'",
        "'abc'.split('',-1).length === 3 && 'abc'.split('',1.9).length === 1",
    ] {
        assert_eq!(evaluate(source).unwrap(), Value::Bool(true), "{source}");
    }
    for source in ["String.raw()", "String.raw({})", "String.raw({raw:null})"] {
        assert!(
            matches!(evaluate(source), Err(RuntimeError::TypeError(_))),
            "{source}"
        );
    }
}

#[test]
fn string_replacement_patterns_and_native_callbacks() {
    for source in [
        "'abcabc'.replace('b','X') === 'aXcabc' && 'abcabc'.replaceAll('b','X') === 'aXcaXc'",
        r#"'abc'.replace('b',"$$:$&:$`:$':$1:$<x>:$") === 'a$:b:a:c:$1:$<x>:$c'"#,
        "'abc'.replace('x','Y') === 'abc' && 'abc'.replaceAll('x','Y') === 'abc'",
        "'ab'.replaceAll('','-') === '-a-b-' && ''.replace('','x') === 'x'",
        "'ab'.replace('b') === 'aundefined' && 'ab'.replaceAll('b',String) === 'ab'",
        r"'😀'.replaceAll('', '_') === '_\ud83d_\ude00_'",
        "'aaa'.replaceAll('aa','b') === 'ba'",
    ] {
        assert_eq!(evaluate(source).unwrap(), Value::Bool(true), "{source}");
    }
    assert!(matches!(
        evaluate("'a'.replace('a',String.fromCodePoint)"),
        Err(RuntimeError::RangeError(_))
    ));
}

#[test]
fn unicode_case_mapping_and_all_normalization_forms_preserve_lone_surrogates() {
    // Rust's case mapping tables and unicode-normalization can be built from
    // different supported Unicode data releases. These stable ECMAScript
    // examples verify observable behavior without pinning the toolchain data.
    for source in [
        "'Straße ﬃ'.toUpperCase() === 'STRASSE FFI'",
        "'ΟΣ ΟΣΑ İ'.toLowerCase() === 'ος οσα i̇'",
        r"'A\ud800Σ'.toLowerCase() === 'a\ud800σ'",
        r"'a\udfffz'.toUpperCase() === 'A\udfffZ'",
        r"'e\u0301'.normalize() === 'é' && 'é'.normalize('NFD') === 'e\u0301'",
        r"'ﬃ'.normalize('NFKC') === 'ffi' && '①é'.normalize('NFKD') === '1e\u0301'",
        r"'\ud800e\u0301\udfff'.normalize() === '\ud800é\udfff'",
        r"'\u1100\u1161'.normalize('NFC') === '가'",
        "''.normalize() === '' && ''.toLowerCase() === ''",
    ] {
        assert_eq!(evaluate(source).unwrap(), Value::Bool(true), "{source}");
    }
    for source in [
        "'a'.normalize('bad')",
        "'a'.normalize(null)",
        r"'a'.normalize('\ud800')",
    ] {
        assert!(
            matches!(evaluate(source), Err(RuntimeError::RangeError(_))),
            "{source}"
        );
    }
}

#[test]
fn legacy_browser_string_wrappers_and_substr_are_real_methods() {
    for (method, tag) in [
        ("big", "big"),
        ("blink", "blink"),
        ("bold", "b"),
        ("fixed", "tt"),
        ("italics", "i"),
        ("small", "small"),
        ("strike", "strike"),
        ("sub", "sub"),
        ("sup", "sup"),
    ] {
        assert_eq!(
            evaluate(&format!("'<x>'.{method}()")).unwrap(),
            Value::String(format!("<{tag}><x></{tag}>").into())
        );
    }
    for (method, tag, attribute) in [
        ("anchor", "a", "name"),
        ("link", "a", "href"),
        ("fontcolor", "font", "color"),
        ("fontsize", "font", "size"),
    ] {
        assert_eq!(
            evaluate(&format!(r#"'x'.{method}('"&<>')"#)).unwrap(),
            Value::String(format!("<{tag} {attribute}=\"&quot;&<>\">x</{tag}>").into())
        );
    }
    for source in [
        "'abcd'.substr(-2,1) === 'c' && 'abcd'.substr(1) === 'bcd'",
        "'abc'.substr(-Infinity,Infinity) === 'abc' && 'abc'.substr(Infinity) === ''",
        "'abc'.substr(1,-1) === '' && 'abc'.substr(NaN,2) === 'ab'",
        "' x '.trimLeft() === 'x ' && ' x '.trimRight() === ' x'",
        "''.trimLeft === ''.trimStart && ''.trimRight === ''.trimEnd && ''.trimLeft.name === 'trimStart'",
    ] {
        assert_eq!(evaluate(source).unwrap(), Value::Bool(true), "{source}");
    }
}

#[test]
fn native_growth_limits_and_partial_bootstrap_do_not_leak_roots() {
    let mut vm = Vm::new(VmConfig {
        max_string_bytes: 64,
        ..VmConfig::default()
    })
    .unwrap();
    for source in [
        "'a'.repeat(33)",
        "'a'.concat('a'.repeat(32))",
        "String.fromCharCode(65).repeat(33)",
        "'a'.repeat(32).replaceAll('a','aa')",
        "'a'.repeat(32).replace('a','aa')",
        "'a'.repeat(32).bold()",
        "'ß'.repeat(32).toUpperCase()",
        "'é'.repeat(32).normalize('NFD')",
        "String.raw({raw:['a'.repeat(32),'b']})",
    ] {
        let code = compile(&parse(source).unwrap()).unwrap();
        assert_eq!(
            vm.execute(&code),
            Err(RuntimeError::StringLimit { limit: 64 }),
            "{source}"
        );
    }
    let static_codes = format!("String.fromCharCode({})", vec!["65"; 33].join(","));
    assert_eq!(
        vm.execute(&compile(&parse(&static_codes).unwrap()).unwrap()),
        Err(RuntimeError::StringLimit { limit: 64 })
    );
    // Exercise failures before and after allocating/rooting the constructor,
    // including partial method registration. Retry in the same VM each time.
    for ceiling in [512, 1024, 2048, 4096, 8192, 16384] {
        let mut vm = Vm::new(VmConfig {
            heap: HeapConfig {
                nursery_capacity: 1,
                major_threshold_bytes: 256,
                max_heap_bytes: ceiling,
            },
            ..VmConfig::default()
        })
        .unwrap();
        let baseline = vm.heap().stats().managed_bytes;
        for _ in 0..2 {
            assert!(matches!(
                vm.execute(&compile(&parse("String").unwrap()).unwrap()),
                Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { .. }))
            ));
            assert_eq!(
                vm.heap().stats().managed_bytes,
                baseline,
                "ceiling {ceiling}"
            );
        }
        assert_eq!(
            vm.execute(&compile(&parse("42").unwrap()).unwrap())
                .unwrap(),
            Value::Number(42.0)
        );
    }
}

#[test]
fn native_call_inputs_and_split_results_survive_gc_and_errors() {
    let mut vm = Vm::new(VmConfig {
        heap: HeapConfig {
            nursery_capacity: 1,
            major_threshold_bytes: 256,
            max_heap_bytes: 128 * 1024,
        },
        ..VmConfig::default()
    })
    .unwrap();
    for source in [
        "let o={raw:['a','b']}; let s=String.raw(o,7); o.raw[0]+s === 'aa7b'",
        "let a='x'.repeat(128).split(''); let garbage={}; a[127] === 'x' && a.length === 128",
        "'a'.replace('abcd','x') === 'a' && 'abc'.lastIndexOf('x') === -1",
        "String.prototype.charAt.call(true) === 't' && 'abc'.slice(1) === 'bc'",
        "'abc'.length++ === 3 && ('x'[0]='z') === 'z'",
    ] {
        assert_eq!(
            vm.execute(&compile(&parse(source).unwrap()).unwrap())
                .unwrap(),
            Value::Bool(true),
            "{source}"
        );
    }
    for source in [
        "String.prototype.split.call(null)",
        "String.prototype.replace.call(undefined)",
        "String.prototype.toString.call({})",
    ] {
        assert!(
            matches!(evaluate(source), Err(RuntimeError::TypeError(_))),
            "{source}"
        );
    }
    assert_eq!(
        evaluate("String({})").unwrap(),
        Value::String("[object Object]".into())
    );
    assert_eq!(
        evaluate("String.prototype.slice.call({})").unwrap(),
        Value::String("[object Object]".into())
    );
    assert_eq!(evaluate("(1).missing").unwrap(), Value::Undefined);
    let mut limited = Vm::new(VmConfig {
        instruction_budget: 50,
        ..VmConfig::default()
    })
    .unwrap();
    let raw = "let r={length:1000}; String.raw({raw:r})";
    assert_eq!(
        limited.execute(&compile(&parse(raw).unwrap()).unwrap()),
        Err(RuntimeError::InstructionLimit)
    );
    assert_eq!(
        limited.execute(&compile(&parse("'a'.repeat(100).split('')").unwrap()).unwrap()),
        Err(RuntimeError::InstructionLimit)
    );
}
