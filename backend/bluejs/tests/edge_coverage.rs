// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public AST and grammar boundary regressions for function and class support.

use blueice_bluejs::{
    compile, parse, ArrowBody, AssignmentPattern, CompileError, Expr, Function, HeapConfig, Param,
    Pattern, Program, RuntimeError, Stmt, UnaryOp, UpdateOp, Value, Vm, VmConfig,
};

fn evaluate(source: &str) -> Result<Value, RuntimeError> {
    Vm::default().execute(&compile(&parse(source).unwrap()).unwrap())
}

fn expression_program(expression: Expr) -> Program {
    Program {
        body: vec![Stmt::Expr(expression)],
    }
}

fn identifier(name: &str) -> Expr {
    Expr::Identifier(name.into())
}

fn super_member(property: Expr, computed: bool) -> Expr {
    Expr::Member {
        object: Box::new(Expr::Super),
        property: Box::new(property),
        computed,
    }
}

#[test]
fn parser_classifies_function_class_and_async_edge_grammar() {
    for source in [
        "async function(){}",
        "class{}",
        "try;",
        "try{}catch;",
        "function #private(){}",
        "class C{get value}",
        "class C{get value(argument){}}",
        "async 1",
        "let \\u{110000}=1;",
        "let \\u{=1;",
        "let \\uD800=1;",
        "let \\u{D800}=1;",
        "let \\u",
    ] {
        assert!(parse(source).is_err(), "{source}");
    }
    assert!(parse("class C{;}").is_ok());
    assert!(parse("class C{async [key](){}}").is_ok());
    assert!(parse("async (value)").is_ok());
    assert!(parse("async value=>value").is_ok());
    assert!(parse("async (value)=>value").is_ok());
    assert!(parse("async").is_ok());
    assert!(parse("value").is_ok());
    assert!(parse("async value").is_err());
    assert!(parse("async (value").is_err());
    assert!(parse("let value=async function named(){};").is_ok());
    assert!(parse("let \\u0061=1;").is_ok());
    assert!(parse("class C{'method'(){return 1}2(){return 2}}").is_ok());
}

#[test]
fn parser_scans_super_calls_inside_class_control_and_expression_containers() {
    for source in [
        "class C{static{if(false){}else super()}}",
        "class C{static{switch(0){case super():}}}",
        "class C{static{switch(0){default:super()}}}",
        "class C{field=([value=super()]=[])}",
        "class C{field=({value:target=super()}={})}",
        "class C{field=`${super()}`}",
        "class C{field=tag`${super()}`}",
        "class C{field=(value=super())=>0}",
        "class C{field={method(){super()}}}",
    ] {
        assert!(parse(source).is_err(), "{source}");
    }
    assert!(parse("function outside({value=super.value}){}").is_err());
    assert!(
        parse("class Base{}class Derived extends Base{constructor({value=super()}){}}").is_ok()
    );
}

#[test]
fn compiler_reports_public_ast_boundaries_without_panicking() {
    assert_eq!(
        CompileError::Unsupported("await").to_string(),
        "BlueJS execution does not yet support await"
    );
    let invalid = [
        (
            Expr::Super,
            "super must be used as a property access or constructor call",
        ),
        (Expr::NewTarget, "new.target"),
        (
            Expr::Await(Box::new(Expr::Number(1.0))),
            "await is only valid in async functions or modules",
        ),
        (
            Expr::Yield {
                value: None,
                delegate: false,
            },
            "yield requires a generator function",
        ),
        (
            Expr::Yield {
                value: Some(Box::new(Expr::Number(1.0))),
                delegate: true,
            },
            "yield requires a generator function",
        ),
    ];
    for (expression, expected) in invalid {
        assert!(
            matches!(compile(&expression_program(expression)), Err(error) if error.to_string().contains(expected)),
            "{expected}"
        );
    }
    assert!(matches!(
        compile(&expression_program(Expr::Yield {
            value: None,
            delegate: true
        })),
        Err(CompileError::InvalidSyntax(_))
    ));
    let generator = Expr::Function(Function {
        name: Some("generator".into()),
        params: vec![Param {
            pattern: Pattern::Identifier("yield".into()),
            default: None,
            rest: false,
        }],
        body: Vec::new(),
        generator: true,
        is_async: false,
    });
    assert!(matches!(
        compile(&expression_program(generator)),
        Err(CompileError::InvalidSyntax(_))
    ));
    let invalid_member = Expr::DestructureAssign {
        pattern: AssignmentPattern::Target(Box::new(super_member(identifier("value"), false))),
        value: Box::new(Expr::Number(1.0)),
    };
    assert!(matches!(
        compile(&expression_program(invalid_member)),
        Err(CompileError::InvalidSyntax(
            "super member requires a dedicated operation"
        ))
    ));
    assert!(matches!(
        compile(&expression_program(super_member(Expr::Number(1.0), false))),
        Err(CompileError::InvalidSyntax(
            "invalid non-computed super member AST"
        ))
    ));
    assert!(compile(&parse("with({}){missing+=1}").unwrap()).is_ok());
    assert!(matches!(
        compile(&parse("function*g(){yield* []}").unwrap()),
        Err(CompileError::Unsupported("synchronous yield*"))
    ));
    let member_update = Expr::Update {
        op: UpdateOp::Inc,
        arg: Box::new(Expr::Member {
            object: Box::new(Expr::Object(Vec::new())),
            property: Box::new(identifier("value")),
            computed: false,
        }),
        prefix: true,
    };
    assert!(compile(&expression_program(member_update)).is_ok());
    let rejected_member_update = Expr::Update {
        op: UpdateOp::Inc,
        arg: Box::new(Expr::Member {
            object: Box::new(Expr::Await(Box::new(Expr::Number(1.0)))),
            property: Box::new(identifier("value")),
            computed: false,
        }),
        prefix: true,
    };
    assert!(matches!(
        compile(&expression_program(rejected_member_update)),
        Err(CompileError::InvalidSyntax(
            "await is only valid in async functions or modules"
        ))
    ));
    assert!(matches!(
        compile(&expression_program(Expr::Update {
            op: UpdateOp::Inc,
            arg: Box::new(Expr::Number(1.0)),
            prefix: true,
        })),
        Err(CompileError::InvalidSyntax("invalid assignment/member AST"))
    ));
    assert!(compile(&Program {
        body: vec![Stmt::Expr(Expr::Class(blueice_bluejs::Class {
            name: None,
            extends: None,
            elements: Vec::new()
        }))]
    })
    .is_ok());
}

#[test]
fn inheritance_operations_cover_computed_keys_fields_and_tail_iterator_cleanup() {
    for source in [
        "class Base{}Base.prototype.value=15;class Derived extends Base{constructor(){super(1,...[2]);}change(){super.value+=1;super.value-=1;super.value*=2;super.value/=2;super.value%=7;return super['value'];}}(new Derived).change()===15",
        "let key='computed';class C{'string'=1;2;[key];static ['static']=3;static{this.block=4;}}let value=new C;value.string===1&&value[2]===undefined&&value.computed===undefined&&C.static===3&&C.block===4",
        "class Base{}class Derived extends Base{get ['value'](){return 1;}set ['value'](next){this.next=next;}}let value=new Derived;value.value=2;value.value===1&&value.next===2",
        "let f=function self(n){'use strict';for(let value of [n]){if(n)return self(n-1);return value;}};f(2)===0",
    ] {
        assert_eq!(evaluate(source), Ok(Value::Bool(true)), "{source}");
    }
    assert!(matches!(
        compile(&parse("try{throw 1}catch(value){class value{}}").unwrap()),
        Err(CompileError::InvalidSyntax(_))
    ));
}

#[test]
fn class_compiler_accepts_async_elements_without_await_and_class_keys() {
    for source in [
        "class C{constructor(){return async()=>1;}}",
        "class C{get value(){return async()=>1;}}",
        "class C{static value=async()=>1;}",
        "class C{static{async()=>1;}}",
    ] {
        assert!(compile(&parse(source).unwrap()).is_ok(), "{source}");
    }
    for source in [
        "class C{'constructor'(){}}",
        "class C{2(){}}",
        "let name='method';class C{[name](){}}",
    ] {
        assert!(compile(&parse(source).unwrap()).is_ok(), "{source}");
    }
}

#[test]
fn runtime_class_and_with_error_paths_are_catchable() {
    for source in [
        "let C=class extends null{};typeof C==='function'",
        "function Base(){}Base.prototype=null;class Derived extends Base{};Object.getPrototypeOf(Derived.prototype)===null",
        "function Base(){}Base.prototype=1;let caught=false;try{class C extends Base{}}catch(error){caught=error instanceof TypeError;}caught",
        "let caught=false;try{class C extends 1{}}catch(error){caught=error instanceof TypeError;}caught",
        "class Base{get value(){return 1;}}class Derived extends Base{write(){super.value=2;}}let caught=false;try{(new Derived).write()}catch(error){caught=error instanceof TypeError;}caught",
        "class Base{}Object.defineProperty(Base.prototype,'value',{value:1,writable:false});class Derived extends Base{write(){super.value=2;}}let caught=false;try{(new Derived).write()}catch(error){caught=error instanceof TypeError;}caught",
        "class Base{}class Derived extends Base{constructor(){super.value=1;}}let caught=false;try{new Derived}catch(error){caught=error instanceof ReferenceError;}caught",
        "class Derived extends null{constructor(){super();}}let caught=false;try{new Derived}catch(error){caught=error instanceof TypeError;}caught",
        "class Base{}class Derived extends Base{constructor(){return 1;}}let caught=false;try{new Derived}catch(error){caught=error instanceof ReferenceError;}caught",
        "let proto={value:1};let object=Object.create(proto);object.value=2;let keys='';for(let key in object){keys+=key;}keys==='value'",
        "let symbol=Symbol('value');let object={[symbol]:1,value:2};let keys='';for(let key in object){keys+=key;}keys==='value'",
        "with({}){missing=1}missing===1",
        "let read;with({value:1}){read=value}read===1",
        "try{with({}){missing}}catch(error){error instanceof ReferenceError}",
        "try{missing}catch(error){error instanceof ReferenceError}",
        "try{'a'.repeat(-1)}catch(error){error instanceof RangeError}",
        "let array=[1,,...[2]];array.length===3&&array[0]===1&&array[2]===2",
        "let caught=false;try{eval('\\uD800')}catch(error){caught=error instanceof SyntaxError;}caught",
        "let caught=false;try{eval('if')}catch(error){caught=error instanceof SyntaxError;}caught",
        "let caught=false;try{eval('with({}){value+=1}')}catch(error){caught=error instanceof ReferenceError;}caught",
    ] {
        assert_eq!(evaluate(source), Ok(Value::Bool(true)), "{source}");
    }
}

#[test]
fn suspended_generator_roots_and_done_state_survive_collection() {
    let mut vm = Vm::new(VmConfig {
        heap: HeapConfig {
            nursery_capacity: 8,
            ..HeapConfig::default()
        },
        ..VmConfig::default()
    })
    .unwrap();
    let source = "function* self(){let saved={value:7};yield 0;return saved.value;}let iter=self();iter.next();let padding=[{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}];iter.next().value===7&&iter.next().done";
    assert_eq!(
        vm.execute(&compile(&parse(source).unwrap()).unwrap()),
        Ok(Value::Bool(true))
    );
    assert!(matches!(
        evaluate("(function*(){throw 1})().next()"),
        Err(RuntimeError::Thrown(Value::Number(1.0)))
    ));
    assert_eq!(
        evaluate(
            "let generator=function* self(){yield self;};generator().next().value===generator"
        ),
        Ok(Value::Bool(true))
    );
}

#[test]
fn abrupt_iterator_cleanup_and_tail_arguments_remain_rooted() {
    for source in [
        "function run(){for(let value of [1]){try{return value}finally{}}}run()===1",
        "let caught=false;try{for(let value of [1]){throw value}}catch(error){caught=error===1;}caught",
        "let count=0;let f=function self(n){'use strict';try{if(n)return self(n-1);return count;}finally{count+=0;}};f(2)===0",
    ] {
        assert_eq!(evaluate(source), Ok(Value::Bool(true)), "{source}");
    }
}

#[test]
fn arrow_body_ast_remains_a_function_boundary() {
    let arrow = Expr::Arrow {
        params: Vec::new(),
        body: ArrowBody::Expr(Box::new(Expr::Unary {
            op: UnaryOp::Void,
            arg: Box::new(Expr::Number(0.0)),
        })),
        is_async: false,
    };
    assert!(compile(&expression_program(arrow)).is_ok());
}
