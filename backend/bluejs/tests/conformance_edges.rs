// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public-interface regressions from the coverage/conformance review.
use blueice_bluejs::{CompileError, RuntimeError, Value, Vm, VmConfig, compile, compile_with_limit, parse};

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
        "void",
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
fn void_evaluates_its_operand_and_returns_undefined() {
    for source in [
        "void 0===undefined",
        "let calls=0;void(calls+=1);calls===1",
        "let object={value:0};void(object.value=7);object.value===7",
        "let value=0;void(value=1),value===1",
    ] {
        assert_eq!(evaluate(source), Value::Bool(true), "{source}");
    }
    let code = compile(&parse("void missing").unwrap()).unwrap();
    assert!(matches!(Vm::default().execute(&code), Err(RuntimeError::ReferenceError(_))));
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
fn abstract_equality_and_in_follow_coercion_and_prototype_rules() {
    for source in [
        "null==undefined && undefined==null && null!=0",
        "'1'==1 && 1=='1' && true==1 && 1==true && false==0",
        "({valueOf(){return 7}})==7 && 7==({valueOf(){return 7}})",
        "Symbol()!=Symbol() && Symbol()!=1",
        "let p={inherited:1};let o={__proto__:p,own:1};'own' in o && 'inherited' in o && !('missing' in o)",
        "let key=Symbol('key');let o={};o[key]=1;key in o",
    ] {
        assert_eq!(evaluate(source), Value::Bool(true), "{source}");
    }
    let error = Vm::default().execute(&compile(&parse("'x' in 1").unwrap()).unwrap());
    assert!(matches!(error, Err(RuntimeError::TypeError(_))));
}

#[test]
fn destructuring_binds_nested_patterns_defaults_rest_and_iterator_protocols() {
    for source in [
        "let [a,,b=3,...rest]=[1,2,undefined,4];a===1&&b===3&&rest.length===1&&rest[0]===4",
        "let [a,{b:c},[d=4]]=[1,{b:2},[]];a===1&&c===2&&d===4",
        "let {a,b:c=2,['d']:e,...rest}={a:1,d:3,z:4};a===1&&c===2&&e===3&&rest.z===4&&rest.a===undefined",
        "let count=0;let o={get a(){count++;return 1},b:2};let {a,...rest}=o;a===1&&rest.b===2&&rest.a===undefined&&count===1",
        "function f([a=1,{b}],{c=3,...rest}){return a+b+c+rest.d;}f([undefined,{b:2}],{d:4})===10",
        "let total=0;for(let [a,b] of [[1,2],[3,4]]){total+=a*b;}total===14",
        "let closed=0;let o={[Symbol.iterator](){return {next(){return {value:7,done:false};},return(){closed++;return {};}};}};let [x]=o;x===7&&closed===1",
        "let next=0;let closed=0;let o={[Symbol.iterator](){return {next(){next++;return next===1?{value:1,done:false}:{done:true};},return(){closed++;return {};}};}};let [a,b,c]=o;a===1&&b===undefined&&c===undefined&&next===2&&closed===0",
        "let a,b,rest;let source=[1,2,undefined,4];let result=([a,,b=3,...rest]=source);result===source&&a===1&&b===3&&rest.length===1&&rest[0]===4",
        "let alpha,renamed,rest;let source={alpha:1,gamma:undefined,extra:4};let result=({alpha,gamma:renamed=3,...rest}=source);result===source&&alpha===1&&renamed===3&&rest.extra===4&&rest.alpha===undefined",
        "let target={};([target.first,target['second']]=[1,2]);target.first===1&&target.second===2",
        "let left;let target={};([{value:left=1},{value:target.right}]=[{}, {value:2}]);left===1&&target.right===2",
    ] {
        assert_eq!(evaluate(source), Value::Bool(true), "{source}");
    }

    for source in ["let {}=null", "let [x]=null", "let x;({x}=null)", "let x;([x]=null)"] {
        let code = compile(&parse(source).unwrap()).unwrap();
        assert!(matches!(Vm::default().execute(&code), Err(RuntimeError::TypeError(_))), "{source}");
    }
}

#[test]
fn sequence_expressions_preserve_order_and_enable_assignment_patterns() {
    for source in [
        "let trace='';let value=(trace+='a',trace+='b',7);trace==='ab'&&value===7",
        "let first=0,last=0;(first=1,last=first+2,last)===3&&first===1&&last===3",
        "let value;0,([value]=[7]);value===7",
        "let sum=0;for(let i=0,j=1;i<3;i++,j+=2){sum+=j;}sum===9",
        "function value(){return 1,2,3;}value()===3",
        "let values=[1,2];values[(0,1)]===2",
    ] {
        assert_eq!(evaluate(source), Value::Bool(true), "{source}");
    }
    // Expression commas require another operand, unlike a trailing elision
    // in an array pattern. Unary void also requires an operand.
    for source in ["1,", "(1,)", "void", "void (1,)", "let [,]=;", "`${1 2}`", "`${1;2}`"] {
        assert!(parse(source).is_err(), "{source}");
    }
}

#[test]
fn object_spread_copies_enumerable_own_properties_in_source_order() {
    for source in [
        "let proto={inherited:1};let source={a:1,get b(){return 2},__proto__:proto};let object={x:0,...source,a:3};object.x===0&&object.a===3&&object.b===2&&object.inherited===undefined",
        "let object={...null,...undefined,a:1};object.a===1",
        "let key=Symbol('key');let source={[key]:1};let object={...source};object[key]===1",
        "let object={...'ab'};object[0]==='a'&&object[1]==='b'&&object.length===undefined",
    ] {
        assert_eq!(evaluate(source), Value::Bool(true), "{source}");
    }
}

#[test]
fn global_numeric_conversion_functions_scan_ecmascript_prefixes() {
    for source in [
        "isNaN('not a number')&&isNaN(undefined)&&!isNaN('')&&!isNaN(null)&&!isNaN('Infinity')",
        "isFinite('1')&&!isFinite(Infinity)&&!isFinite('NaN')&&isFinite(null)",
        "parseInt('  -0x10')===-16&&parseInt('11',2)===3&&parseInt('z',36)===35&&parseInt('12z',10)===12",
        "parseFloat('  -1.25e2x')===-125&&parseFloat('Infinitylater')===Infinity&&parseFloat('0x10')===0",
        "''+(1/parseInt('-0'))==='-Infinity'&&isNaN(parseInt('x'))&&isNaN(parseFloat('.'))",
        "isNaN(parseInt('1',1))&&parseInt('F',16)===15&&parseInt('1?',10)===1",
        "parseFloat('1e+2')===100&&parseFloat('1e+')===1",
        "isNaN===globalThis.isNaN&&isFinite===globalThis.isFinite&&parseInt===globalThis.parseInt&&parseFloat===globalThis.parseFloat",
        "this===globalThis",
    ] {
        assert_eq!(evaluate(source), Value::Bool(true), "{source}");
    }
}

#[test]
fn json_parse_and_stringify_preserve_data_properties_and_json_escapes() {
    for source in [
        "let value=JSON.parse('{\"a\":[true,null,3],\"b\":\"x\"}');value.a[0]&&value.a[1]===null&&value.a[2]===3&&value.b==='x'",
        r#"JSON.stringify({b:2,a:'x',skip:undefined,list:[undefined,NaN,Infinity]})==='{"b":2,"a":"x","list":[null,null,null]}'"#,
        r#"JSON.stringify('\ud800\n')==='"\\ud800\\n"'"#,
        "JSON.stringify(undefined)===undefined&&JSON.stringify(null)==='null'&&JSON.stringify(true)==='true'&&JSON.stringify(false)==='false'",
        "JSON.stringify(function(){})===undefined",
        r#"let object={shown:1};Object.defineProperty(object,'hidden',{value:2,enumerable:false});JSON.stringify(object)==='{"shown":1}'"#,
        r#"JSON.stringify('\b\t\f\r"\\\uD83D\uDE00') === '"\\b\\t\\f\\r\\"\\\\\uD83D\uDE00"'"#,
        "JSON===globalThis.JSON&&JSON.parse.length===2&&JSON.stringify.length===3",
    ] {
        assert_eq!(evaluate(source), Value::Bool(true), "{source}");
    }

    let cyclic = compile(&parse("let object={};object.self=object;JSON.stringify(object)").unwrap()).unwrap();
    assert!(matches!(Vm::default().execute(&cyclic), Err(RuntimeError::TypeError(_))));

    for source in [r"JSON.parse('\uD800')", "JSON.parse('not JSON')"] {
        let code = compile(&parse(source).unwrap()).unwrap();
        assert!(matches!(Vm::default().execute(&code), Err(RuntimeError::SyntaxError(_))), "{source}");
    }

    let oversized = compile(&parse("JSON.stringify('four')").unwrap()).unwrap();
    let mut vm = Vm::new(VmConfig { max_string_bytes: 4, ..VmConfig::default() }).unwrap();
    assert_eq!(vm.execute(&oversized), Err(RuntimeError::StringLimit { limit: 4 }));
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
fn strings_preserve_surrogate_code_units_through_the_public_pipeline() {
    for (source, units) in [(r"'\uD800'", vec![0xd800]), (r"'\u{DFFF}'", vec![0xdfff]), (r"'\uD83D\uDE00'", vec![0xd83d, 0xde00])] {
        let Value::String(value) = evaluate(source) else { panic!("string literal must return a string") };
        assert_eq!(value.as_code_units(), units, "{source}");
    }
}

#[test]
fn bytecode_budget_is_inclusive_and_preserves_default_output() {
    for source in [
        "",
        "1",
        "-1",
        "true||false",
        "false&&true",
        "null??1",
        "let a=[1,2]; for(let i=0;i<a.length;i++){a[i]++;} a[1]",
        "String(42)",
        "'abc'.slice(1)",
        "new String('x').valueOf()",
        "let [,x,...rest]=[1,2,3];x+rest[0]",
        "let x;[,x]=[1,2];x",
        "void (1,2)",
    ] {
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
            assert_eq!(evaluate(&format!("''+({sign}{literal})")), Value::String(format!("{sign}{expected}").into()), "{sign}{literal}");
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
                assert_eq!(text.to_utf8().unwrap().parse::<f64>().unwrap().to_bits(), bits, "{bits:016x}: {text:?}");
            }
        }
    }
}
