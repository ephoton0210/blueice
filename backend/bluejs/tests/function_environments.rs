// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{compile, parse, CompileError, HeapConfig, RuntimeError, Value, Vm, VmConfig};

fn execute(vm: &mut Vm, source: &str) -> Result<Value, RuntimeError> {
    vm.execute(&compile(&parse(source).expect(source)).expect(source))
}

#[test]
fn parameters_are_uninitialized_until_their_binding_is_reached() {
    let mut vm = Vm::default();
    for source in [
        "function f(a=b,b=2){} f()",
        "let a=8; let f=function(a=a){}; f()",
        "let f=(a=typeof b,b=2)=>a; f()",
        "function f({a=b,b=2}={}){} f()",
        "function f([a=b,b=2]=[]){} f()",
        "function f(a=(b=3),b=2){} f()",
        "function f(a=(()=>b)(),b=2){} f()",
        "class C{method(a=b,b=2){}} new C().method()",
        "class C{constructor(a=a){}} new C()",
    ] {
        assert!(
            matches!(
                execute(&mut vm, source),
                Err(RuntimeError::ReferenceError(_))
            ),
            "{source}"
        );
        assert_eq!(execute(&mut vm, "40+2"), Ok(Value::Number(42.0)));
    }
    for source in [
        "function f(a=1,b=a+1){return b;} f()===2",
        "function f(a=b,b=2){return a+b;} f(5)===7",
        "function f(a,a){return a;} f(1,2)===2&&f(1)===undefined",
        "function f({a=2,b=a+1}={}){return b;} f()===3",
        "function f([a=2,b=a+1]=[]){return b;} f()===3",
    ] {
        assert_eq!(execute(&mut vm, source), Ok(Value::Bool(true)), "{source}");
    }
}

#[test]
fn default_closures_capture_parameters_before_body_declarations_exist() {
    let mut vm = Vm::default();
    for source in [
        "let x=9; function f(a=x){var x=2;return a;} f()===9",
        "let x=9; function f(a=x){let x=2;return a;} f()===9",
        "let x=9; function f(a=()=>x){function x(){return 2;}return a();} f()===9",
        "function f(a=1,get=()=>a){var a=2;return get()*10+a;} f()===12",
        "function f(a=1,get=()=>a){a=3;return get();} f()===3",
        "function f(a=1,get=()=>a){var a;return get()===a;} f()",
        "function f(a=1,get=()=>a){function a(){return 2;}return get()===1&&a()===2;} f()",
        "function f(a=1,get=()=>a){var a=2;return get;} let get=f();get()===1",
        "let key='x';function f({[key]:a}){var key='y';return a;}f({x:7})===7",
        "function f({a:{b=2}}={},get=()=>b){var b=3;return get()*10+b;}f({a:{}})===23",
        "class C{method(a=1,get=()=>a){var a=2;return get()*10+a;}}new C().method()===12",
        "function f(a,...rest){var a;var rest;return a+rest[0];}f(2,3)===5",
        "function f(a=1){var a;try{throw 3;}catch(a){var a=4;}return a;}f()===1",
        "function f({a,...rest},get=()=>rest){var rest={v:9};return get().v;}f({a:1,v:7})===7",
        "function f({a,...rest}){var rest;return rest.v;}f({a:1,v:7})===7",
        "function f([a,...rest]){var rest;return a+rest[0];}f([2,3])===5",
        "function f({a:{b=2}},get=()=>b){var b=3;return get()*10+b;}f({a:{}})===23",
        "function f([a=2],get=()=>a){var a=3;return get()*10+a;}f([])===23",
    ] {
        assert_eq!(execute(&mut vm, source), Ok(Value::Bool(true)), "{source}");
    }
    for source in ["function f(a=1){let a;}", "function f({a}={}){class a{}}"] {
        assert!(
            matches!(
                compile(&parse(source).unwrap()),
                Err(CompileError::DuplicateBinding(_))
            ),
            "{source}"
        );
    }
}

#[test]
fn parameter_closures_survive_collection_and_tail_frame_reuse() {
    let mut vm = Vm::new(VmConfig {
        heap: HeapConfig {
            nursery_capacity: 1,
            major_threshold_bytes: 256,
            max_heap_bytes: 512 * 1024,
        },
        ..VmConfig::default()
    })
    .unwrap();
    assert_eq!(execute(&mut vm, "function f(a={v:7},get=()=>a){var a={v:8};return get;}let get=f();for(let i=0;i<30;i++){let garbage={i};}get().v"), Ok(Value::Number(7.0)));
    assert_eq!(execute(&mut vm, "'use strict';let f=function self(n=20,get=()=>n){var n; if(n===0)return get();return self(n-1);};f()"), Ok(Value::Number(0.0)));
    assert_eq!(execute(&mut vm, "'use strict';let keep=[];let f=function self(n=3,get=()=>n){var n;keep[n]=get;if(n===0)return 0;return self(n-1);};f();keep[3]()+keep[2]()+keep[1]()+keep[0]()"), Ok(Value::Number(6.0)));
}

#[test]
fn classic_for_let_creates_a_fresh_binding_for_each_iteration() {
    let mut vm = Vm::default();
    for source in [
        "let first,second,third;for(let i=1;i<=3;i++){if(i===1)first=()=>i;else if(i===2)second=()=>i;else third=()=>i;}first()*100+second()*10+third()===123",
        "let first,second,third;for(let i=0;i<4;i++){if(i===2)continue;if(i===0)first=()=>i;else if(i===1)second=()=>i;else third=()=>i;}first()*100+second()*10+third()===13",
    ] {
        assert_eq!(execute(&mut vm, source), Ok(Value::Bool(true)), "{source}");
    }
}

#[test]
fn with_var_declarations_are_hoisted_through_nested_control_flow() {
    let mut vm = Vm::default();
    for source in [
        "with({}){var value=3;}value===3",
        "try{with({}){with({}){var value=3;throw 1;}}}catch(e){}value===3",
        "function f(){with({}){while(true){var value=3;break;}}return value;}f()===3",
        "with({}){var f=function(){return 3;};}f()===3",
        "with({}){function f(){return 3;}}f()===3",
        "with({}){var [a,b]=[1,2];}a+b===3",
    ] {
        assert_eq!(execute(&mut vm, source), Ok(Value::Bool(true)), "{source}");
    }
    let source = "{let value;with({}){var value;}}";
    assert!(matches!(
        compile(&parse(source).unwrap()),
        Err(CompileError::DuplicateBinding(_))
    ));
}
