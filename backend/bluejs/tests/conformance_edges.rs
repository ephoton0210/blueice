// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public-interface regressions from the coverage/conformance review.
use blueice_bluejs::{
    compile, compile_with_limit, parse, CompileError, RuntimeError, Value, Vm, VmConfig,
};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap()
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
    assert!(matches!(
        Vm::default().execute(&code),
        Err(RuntimeError::ReferenceError(_))
    ));
}

#[test]
fn number_bitwise_operators_coerce_mask_and_preserve_reference_evaluation() {
    for source in [
        "(~0)===-1&&(5&3)===1&&(5|2)===7&&(5^3)===6",
        "(1<<33)===2&&(-8>>1)===-4&&(-1>>>0)===4294967295",
        "(4294967295&-1)===-1&&('3'&true)===1",
        "(1|2^3&1)===3",
        "let value=15;value&=10;value|=1;value^=3;value<<=2;value>>=1;value>>>=0;value===16",
        "let calls=0;let object={value:1};function target(){calls+=1;return object;}target().value<<=3;calls===1&&object.value===8",
    ] {
        assert_eq!(evaluate(source), Value::Bool(true), "{source}");
    }
}

#[test]
fn bigint_bitwise_operators_preserve_precision_and_reject_mixed_numeric_types() {
    for source in [
        "(~0n)===-1n&&(5n&3n)===1n&&(5n|2n)===7n&&(5n^3n)===6n",
        "(1n<<33n)===8589934592n&&(5n<<-1n)===2n&&(-8n>>1n)===-4n&&(-8n>>-1n)===-16n",
        "(Object(3n)&1n)===1n&&(({valueOf(){return 2n}}|1n)===3n)",
        "typeof 0n==='bigint'&&String(-2n)==='-2'&&BigInt(2n)===2n&&Object.prototype.toString.call(Object(1n))==='[object BigInt]'",
        "let caught=false;try{1n&1}catch(error){caught=error instanceof TypeError;}caught",
        "let caught=false;try{1n>>>0n}catch(error){caught=error instanceof TypeError;}caught",
    ] {
        let code = compile(&parse(source).unwrap()).unwrap();
        assert_eq!(Vm::default().execute(&code), Ok(Value::Bool(true)), "{source}");
    }
    for source in ["1.0n", "1e1n", "01n", "0x1nn"] {
        assert!(parse(source).is_err(), "{source}");
    }
}

#[test]
fn exponentiation_is_right_associative_and_preserves_numeric_semantics() {
    for source in [
        "2**3===8&&2**3**2===512&&3*2**3===24&&2**-2===0.25&&isNaN((-1)**Infinity)&&isNaN(1**-Infinity)",
        "let base=-3;base**=3;base===-27",
        "let base=4;(--base)**2===9&&(base++)**2===9&&base===4",
        "let trace='';let left={valueOf(){trace+='l';return 3}};let right={valueOf(){trace+='r';return 2}};left**right===9&&trace==='lr'",
        "2n**10n===1024n&&0n**0n===1n&&(-2n)**3n===-8n",
    ] {
        assert_eq!(evaluate(source), Value::Bool(true), "{source}");
    }

    for source in ["-3**2", "+3**2", "!3**2", "typeof 3**2"] {
        assert!(parse(source).is_err(), "{source}");
    }
    for source in ["1n**1", "1**1n"] {
        let code = compile(&parse(source).unwrap()).unwrap();
        assert!(matches!(
            Vm::default().execute(&code),
            Err(RuntimeError::TypeError(_))
        ));
    }
    for source in ["1n**-1n", "0n**-1n"] {
        let code = compile(&parse(source).unwrap()).unwrap();
        assert!(matches!(
            Vm::default().execute(&code),
            Err(RuntimeError::RangeError(_))
        ));
    }
}

#[test]
fn logical_assignments_latch_references_and_short_circuit_the_rhs() {
    for source in [
        "let and=1,or=0,nil=null;and&&=2;or||=3;nil??=4;and===2&&or===3&&nil===4",
        "let calls=0;let and=0,or=1,nil=2;and&&=++calls;or||=++calls;nil??=++calls;calls===0",
        "let calls=0;let object={value:0};function target(){calls++;return object;}target().value||=7;calls===1&&object.value===7",
        "let caught=false;try{let key={toString(){throw new Error;}};null[key]&&=1}catch(error){caught=error instanceof TypeError;}caught",
        "let value=1;value&&=function(){};value.name==='value'",
        "let value;value??=()=>{};value.name==='value'",
        "let value=0n;value||=1n;value===1n",
    ] {
        assert_eq!(evaluate(source), Value::Bool(true), "{source}");
    }
    for source in [
        "let caught=false;try{missing&&=1}catch(error){caught=error instanceof ReferenceError;}caught",
        "'use strict';let object={value:0};Object.defineProperty(object,'value',{writable:false});object.value&&=1;object.value===0",
    ] {
        assert_eq!(evaluate(source), Value::Bool(true), "{source}");
    }
}

#[test]
fn private_slots_keep_class_identity_and_private_reference_semantics() {
    for source in [
        "class C{#field=true;#method(){}get #access(){return this.#field}set #access(value){this.#field=value}update(){this.#field&&=false;this.#access||=true;return this.#field}method(){return this.#method}replaceMethod(){this.#method&&=1}}let value=new C;let method=value.method();let caught=false;try{value.replaceMethod()}catch(error){caught=error instanceof TypeError;}value.update()&&method===value.method()&&caught&&Object.keys(value).length===0",
        "class A{#value=1;read(){return this.#value}}class B{#value=2;read(){return this.#value}}let a=new A,b=new B;a.read()===1&&b.read()===2",
        "class C{#value=1;read(){return this.#value}}let caught=false;try{C.prototype.read.call({})}catch(error){caught=error instanceof TypeError;}caught",
        "class C{get #value(){return false}assign(){return this.#value&&=(() => {throw new Error})()}}new C().assign()===false",
        "class C{#value=1;make(){return function(){return this.#value}}}let value=new C;value.make().call(value)===1",
        "class C{static #value=1;static #method(){return this.#value}static get #access(){return this.#value}static set #access(value){this.#value=value}static run(){let initial=this.#method()+this.#access;this.#access=4;return initial===2&&this.#value===4}}C.run()",
        "class C{static async #method(){}static getMethod(){return this.#method}}C.getMethod().name==='#method'",
        "class C{static #method(){return 1}static throughEval(){return eval('this.#method()')}}C.throughEval()===1",
        "class C{#value;has(value){return #value in value}}let value=new C;value.has(value)&&!value.has({})",
        "class C{#value=1;readThroughNestedClass(){class Nested{read(value){return value.#value}}return new Nested().read(this)}}new C().readThroughNestedClass()===1",
    ] {
        assert_eq!(evaluate(source), Value::Bool(true), "{source}");
    }
}

#[test]
fn private_name_early_errors_are_reported_before_execution() {
    for source in [
        "({}).#missing",
        "#missing in {}",
        "class C{#value;#value}",
        "class C{static #value;#value}",
        "class C{get #value(){}get #value(){}}",
        "class C{#value;method(){delete this.#value}}",
        "class C extends class{x=this.#value}{#value}",
        "class C{#constructor}",
        "class C{#value;method(){return super.#value}}",
    ] {
        match parse(source) {
            Err(_) => {}
            Ok(program) => assert!(
                matches!(compile(&program), Err(CompileError::InvalidSyntax(_))),
                "{source}"
            ),
        }
    }
}

#[test]
fn public_class_element_early_errors_are_classified() {
    for source in [
        "class C extends () => {}{}",
        "class C{constructor(){}constructor(){}}",
        "class C{get constructor(){}}",
        "class C{set constructor(value){}}",
        "class C{async constructor(){}}",
        "class C{*constructor(){}}",
        "class C{static prototype(){}}",
        "class C{static get prototype(){}}",
        "class C{static async prototype(){}}",
        "class C{static *prototype(){}}",
    ] {
        assert!(parse(source).is_err(), "{source}");
    }
}

#[test]
fn number_static_constants_have_spec_values_and_attributes() {
    assert!(matches!(
        evaluate("Number.NaN"),
        Value::Number(value) if value.is_nan()
    ));
    for (source, expected) in [
        ("Number.EPSILON", f64::EPSILON),
        ("Number.MAX_SAFE_INTEGER", 9_007_199_254_740_991.0),
        ("Number.MIN_SAFE_INTEGER", -9_007_199_254_740_991.0),
        ("Number.MAX_VALUE", f64::MAX),
        ("Number.MIN_VALUE", f64::from_bits(1)),
        ("Number.NEGATIVE_INFINITY", f64::NEG_INFINITY),
        ("Number.POSITIVE_INFINITY", f64::INFINITY),
    ] {
        assert_eq!(evaluate(source), Value::Number(expected), "{source}");
    }
    assert_eq!(
        evaluate(
            "let d=Object.getOwnPropertyDescriptor(Number,'MAX_VALUE');d.value===Number.MAX_VALUE&&!d.writable&&!d.enumerable&&!d.configurable",
        ),
        Value::Bool(true)
    );
}

#[test]
fn property_is_enumerable_observes_only_own_enumerable_properties() {
    assert_eq!(
        evaluate(
            "let proto={inherited:1};let object={__proto__:proto};object.propertyIsEnumerable('inherited')===false&&object.propertyIsEnumerable('own')===false&&(object.own=1,object.propertyIsEnumerable('own'))",
        ),
        Value::Bool(true)
    );
}

#[test]
fn strict_unresolvable_assignments_fail_at_put_value_after_the_rhs() {
    for source in [
        "\"use strict\";let marker=0;let caught=false;try{missing=(marker=1)}catch(error){caught=error instanceof ReferenceError;}caught&&marker===1",
        "\"use strict\";let caught=false;try{x<<(x=1)}catch(error){caught=error instanceof ReferenceError;}caught",
        "\"use strict\";globalThis.bound=1;bound=2;globalThis.bound===2",
    ] {
        let code = compile(&parse(source).unwrap()).expect("strict assignment should compile");
        assert_eq!(Vm::default().execute(&code), Ok(Value::Bool(true)), "{source}");
    }
}

#[test]
fn destructuring_assignments_infer_names_for_anonymous_default_definitions() {
    for source in [
        "var fn;[fn=function(){}]=[];fn.name==='fn'",
        "var fn;({fn=function(){}}={});fn.name==='fn'",
        "var fn;[fn=()=>{}]=[];fn.name==='fn'",
        "var fn;({fn=class{}}={});fn.name==='fn'",
    ] {
        assert_eq!(evaluate(source), Value::Bool(true), "{source}");
    }
}

#[test]
fn simple_identifier_assignments_infer_names_for_anonymous_definitions() {
    for source in [
        "var fn;fn=function(){};fn.name==='fn'",
        "var fn;fn=()=>{};fn.name==='fn'",
        "var fn;fn=class{};fn.name==='fn'",
        "var fn;fn=function*(){};fn.name==='fn'",
        "var fn;fn=(function(){});fn.name==='fn'",
        "var fn;fn=(0,function(){});fn.name!== 'fn'",
        "var fn;(fn)=function(){};fn.name===''",
    ] {
        assert_eq!(evaluate(source), Value::Bool(true), "{source}");
    }
}

#[test]
fn simple_member_assignment_converts_the_computed_key_after_the_rhs() {
    let source = "function Expected(){};let rhs=false;let key={toString:function(){throw new Expected;}};let caught=false;try{null[key]=(rhs=true)}catch(error){caught=error instanceof Expected;}rhs&&caught";
    assert_eq!(evaluate(source), Value::Bool(true));
}

#[test]
fn global_constant_properties_materialize_before_a_strict_member_assignment() {
    let source = "'use strict';let caught=false;try{globalThis.Infinity=0}catch(error){caught=error instanceof TypeError;}caught&&globalThis.Infinity===1/0";
    assert_eq!(evaluate(source), Value::Bool(true));
}

#[test]
fn nested_parentheses_and_ordinary_length_do_not_take_special_paths() {
    assert_eq!(evaluate("(((1+2)*3))"), Value::Number(9.0));
    assert_eq!(
        evaluate("let o={length:'text'}; o.length"),
        Value::String("text".into())
    );
    assert_eq!(
        evaluate("let p={length:7}; ({__proto__:p}).length"),
        Value::Number(7.0)
    );
    assert_eq!(
        evaluate("typeof undefined"),
        Value::String("undefined".into())
    );
    assert_eq!(
        evaluate("let x; typeof x"),
        Value::String("undefined".into())
    );
}

#[test]
fn global_length_can_be_bound_by_a_destructuring_assignment() {
    let source = "var x,length;var vals=[null];var result=[...{0:x,length}]=vals;x===null&&length===1&&result===vals";
    let code = compile(&parse(source).unwrap()).unwrap();
    assert_eq!(Vm::default().execute_script(&code), Ok(Value::Bool(true)));
}

#[test]
fn array_reduce_preserves_descriptor_and_skips_holes() {
    assert_eq!(
        evaluate("Array;typeof [1].reduce"),
        Value::String("function".into())
    );
    assert_eq!(
        evaluate("Array;[1,,3].reduce(function(sum,value){return sum+value},2)===6"),
        Value::Bool(true)
    );
}

#[test]
fn proxy_has_trap_controls_with_environment_lookup() {
    let source = "let log=[];let env=new Proxy({},{has(target,key){log.push(key);}});with(env){log.push('body');}log.join(',')";
    assert_eq!(evaluate(source), Value::String("log,body".into()));
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

    for source in [
        "let {}=null",
        "let [x]=null",
        "let x;({x}=null)",
        "let x;([x]=null)",
    ] {
        let code = compile(&parse(source).unwrap()).unwrap();
        assert!(
            matches!(
                Vm::default().execute(&code),
                Err(RuntimeError::TypeError(_))
            ),
            "{source}"
        );
    }
}

#[test]
fn catch_parameter_early_errors_are_reported_by_compilation() {
    for source in [
        "\"use strict\";try{}catch(eval){}",
        "\"use strict\";try{}catch(arguments){}",
        "try{}catch(value){let value;}",
        "function f(){try{}catch(value){function value(){}}}",
    ] {
        let program = parse(source).unwrap();
        assert!(
            matches!(compile(&program), Err(CompileError::InvalidSyntax(_))),
            "{source}"
        );
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
    for source in [
        "1,",
        "(1,)",
        "void",
        "void (1,)",
        "let [,]=;",
        "`${1 2}`",
        "`${1;2}`",
    ] {
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

    let cyclic =
        compile(&parse("let object={};object.self=object;JSON.stringify(object)").unwrap())
            .unwrap();
    assert!(matches!(
        Vm::default().execute(&cyclic),
        Err(RuntimeError::TypeError(_))
    ));

    for source in [r"JSON.parse('\uD800')", "JSON.parse('not JSON')"] {
        let code = compile(&parse(source).unwrap()).unwrap();
        assert!(
            matches!(
                Vm::default().execute(&code),
                Err(RuntimeError::SyntaxError(_))
            ),
            "{source}"
        );
    }

    let oversized = compile(&parse("JSON.stringify('four')").unwrap()).unwrap();
    let mut vm = Vm::new(VmConfig {
        max_string_bytes: 4,
        ..VmConfig::default()
    })
    .unwrap();
    assert_eq!(
        vm.execute(&oversized),
        Err(RuntimeError::StringLimit { limit: 4 })
    );
}

#[test]
fn malformed_hexadecimal_escapes_are_rejected_instead_of_accepting_signs() {
    for escape in [
        r"\x+1",
        r"\x-0",
        r"\u+001",
        r"\u-000",
        r"\u{+1}",
        r"\u{-0}",
        r"\u{}",
        r"\u{FFFFFFFFF}",
    ] {
        for quote in ['\'', '"', '`'] {
            assert!(
                parse(&format!("{quote}{escape}{quote}")).is_err(),
                "{quote}{escape}{quote}"
            );
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
    assert_eq!(
        evaluate(r"'\x00\x7F\xFF\u0041\u{1F600}'"),
        Value::String("\0\u{7f}ÿA😀".into())
    );
    assert_eq!(
        evaluate(r"'\u{00000000000000000000000000000041}'"),
        Value::String("A".into())
    );
}

#[test]
fn strings_preserve_surrogate_code_units_through_the_public_pipeline() {
    for (source, units) in [
        (r"'\uD800'", vec![0xd800]),
        (r"'\u{DFFF}'", vec![0xdfff]),
        (r"'\uD83D\uDE00'", vec![0xd83d, 0xde00]),
    ] {
        let Value::String(value) = evaluate(source) else {
            panic!("string literal must return a string")
        };
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
        assert_eq!(
            Vm::default().execute(&bounded),
            Vm::default().execute(&normal),
            "{source}"
        );
        for insufficient in 0..limit {
            assert_eq!(
                compile_with_limit(&ast, insufficient)
                    .err()
                    .expect("below required size"),
                CompileError::ProgramTooLarge,
                "{source}, limit {insufficient}"
            );
        }
    }
}

#[test]
fn bytecode_budget_rejects_partial_instructions_and_propagates_expression_errors() {
    for (source, limit) in [
        ("", 0),
        ("", 4),
        ("", 5),
        ("1", 9),
        ("-1", 10),
        ("true||false", 15),
        ("false&&true", 15),
        ("null??1", 15),
    ] {
        let error = compile_with_limit(&parse(source).unwrap(), limit)
            .err()
            .expect("insufficient instruction budget");
        assert_eq!(
            error,
            CompileError::ProgramTooLarge,
            "{source}, limit {limit}"
        );
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
            assert_eq!(
                evaluate(&format!("''+({sign}{literal})")),
                Value::String(format!("{sign}{expected}").into()),
                "{sign}{literal}"
            );
        }
    }
}

#[test]
fn decimal_scanning_retains_fraction_exponent_and_overflow_behavior() {
    for (literal, expected) in [
        ("0", 0.0),
        ("1.", 1.0),
        (".25", 0.25),
        ("1.e3", 1000.0),
        ("1e+309", f64::INFINITY),
        ("1e-400", 0.0),
    ] {
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
                let Value::String(text) = vm.execute(&code).unwrap() else {
                    panic!("string concatenation must return a string")
                };
                assert_eq!(
                    text.to_utf8().unwrap().parse::<f64>().unwrap().to_bits(),
                    bits,
                    "{bits:016x}: {text:?}"
                );
            }
        }
    }
}
