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

#[test]
fn sloppy_direct_eval_copies_annex_b_block_functions_into_its_var_environment() {
    let mut vm = Vm::default();
    for source in [
        "var initial,current,outer;(function(){eval('{function f(){initial=f;f=123;current=f;return 33;}}outer=f;f();')}());initial()===33&&current===123&&outer()===33",
        "function outer(){let f=1;return function(){eval('var f=2');return f;};}outer()()===2",
        "let g=(function*(){eval('var f=2');yield 0;return f;})();g.next();g.next().value===2",
    ] {
        assert_eq!(execute(&mut vm, source), Ok(Value::Bool(true)), "{source}");
    }
}

#[test]
fn async_function_environments_preserve_contextual_names_and_lexical_contexts() {
    for source in [
        "async function(){var await;}",
        "async function(){await:;}",
        "async function await(){}",
        "void \\u0061sync function value(){}",
    ] {
        assert!(parse(source).is_err(), "{source}");
    }

    let mut vm = Vm::default();
    let setup = "
        var value=1;
        globalThis[Symbol.unscopables]={value:true};
        var ignoredName, strictName, withValue, lexicalTarget;
        let sloppy=async function named(){named=1;return named;};
        let strict=async function named(){'use strict';eval('named=1');};
        let scoped=async function(){var value=2;with(globalThis){return value;}};
        let target=async function(){return async()=>new.target;};
        sloppy().then(result=>{ignoredName=result;});
        strict().then(undefined,error=>{strictName=error instanceof TypeError;});
        scoped().then(result=>{withValue=result;});
        target().then(result=>result()).then(result=>{lexicalTarget=result;});
    ";
    vm.execute_script(&compile(&parse(setup).unwrap()).unwrap())
        .unwrap();
    vm.run_promise_jobs().unwrap();
    assert_eq!(
        execute(
            &mut vm,
            "ignoredName===sloppy&&strictName&&withValue===2&&lexicalTarget===undefined",
        ),
        Ok(Value::Bool(true))
    );
}

#[test]
fn async_arrows_share_the_async_function_intrinsic_and_dynamic_constructor() {
    let mut vm = Vm::default();
    execute(
        &mut vm,
        "
            let arrow = async () => 1;
            let AsyncFunction = Object.getPrototypeOf(arrow).constructor;
            let dynamic = AsyncFunction('value', 'return await value + 1;');
            globalThis.asyncPrototypeOK =
                Object.getPrototypeOf(arrow) === AsyncFunction.prototype &&
                Object.getPrototypeOf(AsyncFunction) === Function &&
                AsyncFunction.name === 'AsyncFunction' &&
                AsyncFunction.length === 1 &&
                !Object.prototype.hasOwnProperty.call(dynamic, 'prototype');
            dynamic(Promise.resolve(41)).then(value => {
                globalThis.asyncDynamicResult = value;
            });
        ",
    )
    .unwrap();
    vm.run_promise_jobs().unwrap();
    assert_eq!(
        execute(&mut vm, "asyncPrototypeOK && asyncDynamicResult === 42",),
        Ok(Value::Bool(true))
    );
    assert!(matches!(
        execute(&mut vm, "new (async function() {}).constructor()"),
        Err(RuntimeError::TypeError(_))
    ));
}

#[test]
fn async_generators_expose_promise_requests_and_a_replaceable_instance_prototype() {
    let mut vm = Vm::default();
    execute(
        &mut vm,
        "
            async function* sequence() {
                let input = yield 1;
                yield input + 1;
                return 9;
            }
            let iterator = sequence();
            let fallback = Object.getPrototypeOf(sequence.prototype);
            let prototypeOK = Object.getPrototypeOf(iterator) === sequence.prototype;
            sequence.prototype = null;
            prototypeOK = prototypeOK && Object.getPrototypeOf(sequence()) === fallback;
            globalThis.asyncGeneratorPrototypeOK = prototypeOK;
            iterator.next().then(first => {
                globalThis.asyncGeneratorFirst = first.value === 1 && !first.done;
                iterator.next(4).then(second => {
                    globalThis.asyncGeneratorSecond = second.value === 5 && !second.done;
                    iterator.next().then(last => {
                        globalThis.asyncGeneratorLast = last.value === 9 && last.done;
                    });
                });
            });
            sequence().throw('expected').then(
                () => { globalThis.asyncGeneratorThrow = false; },
                reason => { globalThis.asyncGeneratorThrow = reason === 'expected'; }
            );
        ",
    )
    .unwrap();
    vm.run_promise_jobs().unwrap();
    assert_eq!(
        execute(
            &mut vm,
            "asyncGeneratorPrototypeOK && asyncGeneratorFirst && asyncGeneratorSecond && asyncGeneratorLast && asyncGeneratorThrow",
        ),
        Ok(Value::Bool(true))
    );
}

#[test]
fn async_generator_early_errors_and_parameter_initialization_are_observable() {
    for source in [
        "(async function*() { yield: 1; });",
        "(async function* yield() {});",
        "(async function*() { var yield; });",
        "(async function*() { void yield; });",
    ] {
        assert!(parse(source).is_err(), "{source}");
    }

    let mut vm = Vm::default();
    assert_eq!(
        execute(
            &mut vm,
            "var g=async function*(a=(g.prototype=null)){};let old=g.prototype;let it=g();Object.getPrototypeOf(it)!==old",
        ),
        Ok(Value::Bool(true))
    );
}

#[test]
fn async_generator_yields_awaited_values_through_its_public_promise_interface() {
    let mut vm = Vm::default();
    execute(
        &mut vm,
        "
            async function* values() {
                yield Promise.resolve(3);
                yield { then(resolve) { resolve(4); } };
                yield Promise.reject('expected');
            }
            let iterator = values();
            iterator.next()
                .then(first => {
                    globalThis.firstYield = first.value === 3 && !first.done;
                    return iterator.next();
                })
                .then(second => {
                    globalThis.secondYield = second.value === 4 && !second.done;
                    return iterator.next();
                })
                .then(
                    () => { globalThis.rejectionObserved = false; },
                    reason => {
                        globalThis.rejectionObserved = reason === 'expected';
                        return iterator.next();
                    }
                )
                .then(last => {
                    globalThis.closedAfterRejection = last.done && last.value === undefined;
                });
        ",
    )
    .unwrap();
    vm.run_promise_jobs().unwrap();
    assert_eq!(
        execute(
            &mut vm,
            "firstYield && secondYield && rejectionObserved && closedAfterRejection",
        ),
        Ok(Value::Bool(true))
    );
}

#[test]
fn async_generator_awaits_suspend_and_resume_its_public_request() {
    let mut vm = Vm::default();
    execute(
        &mut vm,
        "
            async function* values() {
                yield await Promise.resolve(3);
                return await Promise.resolve(4);
            }
            let iterator = values();
            iterator.next().then(first => {
                globalThis.awaitFirst = first.value === 3 && !first.done;
                return iterator.next();
            }).then(last => {
                globalThis.awaitLast = last.value === 4 && last.done;
            });
        ",
    )
    .unwrap();
    vm.run_promise_jobs().unwrap();
    assert_eq!(
        execute(&mut vm, "awaitFirst && awaitLast"),
        Ok(Value::Bool(true))
    );
}

#[test]
fn for_await_consumes_async_generator_next_promises() {
    let mut vm = Vm::default();
    execute(
        &mut vm,
        "
            async function* values() { yield 2; yield 3; }
            async function total() {
                let sum = 0;
                for await (let value of values()) sum += value;
                return sum;
            }
            total().then(value => { globalThis.forAwaitTotal = value; });
        ",
    )
    .unwrap();
    vm.run_promise_jobs().unwrap();
    assert_eq!(
        execute(&mut vm, "forAwaitTotal === 5"),
        Ok(Value::Bool(true))
    );
    assert!(parse("async function f(){for await (let value in {});}").is_err());
}

#[test]
fn async_generator_yield_star_respects_the_no_line_terminator_grammar() {
    let error = parse("async function* f(){ yield\n* 1; }").unwrap_err();
    assert!(error.known_syntax);
}

#[test]
fn async_generator_yield_star_forwards_next_values_and_returns_the_inner_completion() {
    let mut vm = Vm::default();
    execute(
        &mut vm,
        "
            async function* inner() { let received = yield 1; yield received; return 9; }
            async function* outer() { let result = yield* inner(); return result; }
            let iterator = outer();
            iterator.next().then(first => {
                globalThis.first = first.value === 1 && !first.done;
                return iterator.next(4);
            }).then(second => {
                globalThis.second = second.value === 4 && !second.done;
                return iterator.next(8);
            }).then(done => {
                globalThis.last = done.value === 9 && done.done;
            });
        ",
    )
    .unwrap();
    vm.run_promise_jobs().unwrap();
    assert_eq!(
        execute(&mut vm, "first && second && last"),
        Ok(Value::Bool(true))
    );
}
