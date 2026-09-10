// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public-pipeline regressions for Completion records and try handlers.

use blueice_bluejs::{compile, parse, CompileError, HeapConfig, RuntimeError, Value, Vm, VmConfig};

fn execute(vm: &mut Vm, source: &str) -> Result<Value, RuntimeError> {
    vm.execute(&compile(&parse(source).unwrap()).unwrap())
}

#[test]
fn catch_receives_thrown_values_and_language_errors_in_a_fresh_scope() {
    let mut vm = Vm::default();
    for source in [
        "let result;try{throw {answer:42};}catch(error){result=error.answer;}result===42",
        "let result;try{null.value;}catch(error){result=error instanceof TypeError&&error.name==='TypeError';}result",
        "try{throw 1;}catch(error){let inside=error;}typeof error==='undefined'&&typeof inside==='undefined'",
        "let ok;try{throw []}catch([named=function(){}]){ok=named.name==='named';}ok",
        "let ok;try{throw []}catch([named=()=>{}]){ok=named.name==='named';}ok",
        "let parameter;let block;let x='outer';try{throw []}catch([_=parameter=()=>x]){block=()=>x;let x='inner';}parameter()==='outer'&&block()==='inner'",
        "let before;let during;let after;try{throw 'exception';}catch(err){before=err;for(var err='loop';err!=='done';err='done'){during=err;}after=err;}before==='exception'&&during==='loop'&&after==='done'",
        "let joined;try{throw ['first'].concat(['second']);}catch(value){joined=value;}joined.length===2&&joined[0]==='first'&&joined[1]==='second'",
    ] {
        assert_eq!(execute(&mut vm, source), Ok(Value::Bool(true)), "{source}");
    }
    assert_eq!(
        execute(&mut vm, "try{throw 7;}catch{7}"),
        Ok(Value::Number(7.0))
    );
}

#[test]
fn direct_eval_uses_the_callers_bindings_completion_and_strictness() {
    let mut vm = Vm::default();
    for source in [
        "eval('1;try{2}finally{}')===2",
        "eval('2;try{3}catch(error){}')===3",
        "eval('2;try{throw null}catch(error){3}')===3",
        "eval('for(var i=0;i<2;i++){if(i){try{throw null}catch(error){break}}\"ignored\";}')===undefined",
        "var value=1;eval('var value=3');value===3",
        "let value=1;let read;try{throw []}catch([_=(eval('var value=3'),read=()=>value)]){}read()===3&&value===3",
        "eval(42)===42",
        "\"use strict\";let caught;try{eval('try{}catch(eval){}')}catch(error){caught=error instanceof SyntaxError;}caught",
    ] {
        assert_eq!(execute(&mut vm, source), Ok(Value::Bool(true)), "{source}");
    }
    assert_eq!(
        execute(
            &mut vm,
            "let a=eval('1; try { } catch (err) { }');let b=eval('2; try { 3; } catch (err) { }');let c=eval('4; try { } catch (err) { 5; }');let d=eval('6; try { 7; } catch (err) { 8; }');[a===undefined,b===3,c===undefined,d===7].join(',')",
        ),
        Ok(Value::String("true,true,true,true".into()))
    );
}

#[test]
fn named_function_expressions_bind_their_name_and_reuse_tail_frames() {
    let mut vm = Vm::new(VmConfig {
        instruction_budget: 5_000_000,
        ..VmConfig::default()
    })
    .unwrap();
    for source in [
        "let f=(function self(){return self;});typeof self==='undefined'&&f()===f",
        "let caught;try{(function self(){'use strict';self=1;})()}catch(error){caught=error instanceof TypeError;}caught",
        "let calls=0;(function self(n){'use strict';if(n===0){calls++;return;}try{throw null;}catch(error){return self(n-1);}})(100000);calls===1",
        "let calls=0;(function self(n){'use strict';if(n===0){calls++;return;}try{}finally{return self(n-1);}})(100000);calls===1",
        "let calls=0;(function self(n){'use strict';try{if(n===0){calls++;return;}}catch(error){}finally{if(n!==0)return self(n-1);}})(100000);calls===1",
    ] {
        assert_eq!(execute(&mut vm, source), Ok(Value::Bool(true)), "{source}");
    }
}

#[test]
fn generators_suspend_resume_and_close_as_iterators() {
    let mut vm = Vm::default();
    for source in [
        "let iter=(function*(){yield 1;yield 2;})();let first=iter.next();let second=iter.next();let done=iter.next();first.value===1&&!first.done&&second.value===2&&!second.done&&done.done",
        "let first=0;let second=0;let iter=(function*(){first++;yield;second++;})();let value;try{throw iter}catch([,]){value=first===1&&second===0;}value",
        "let first=0;let second=0;let iter=(function*(){first++;yield;second++;})();let value;try{throw iter}catch([...[,]]){value=first===1&&second===1;}value",
        "let values=[];Array.prototype[Symbol.iterator]=function*(){yield this[0];yield this[1];yield 42;};try{throw [1,2,3]}catch([x,y,z]){values=[x,y,z]}values.join(',')==='1,2,42'",
    ] {
        assert_eq!(execute(&mut vm, source), Ok(Value::Bool(true)), "{source}");
    }
}

#[test]
fn classes_construct_instances_install_methods_and_keep_static_block_early_errors() {
    let mut vm = Vm::default();
    assert_eq!(
        execute(
            &mut vm,
            "let result;try{throw []}catch([cls=class{},named=class Named{},shadow=class{static name(){}}]){result=cls.name==='cls'&&named.name==='Named'&&shadow.name!=='shadow'}result",
        ),
        Ok(Value::Bool(true))
    );
    assert_eq!(
        execute(&mut vm, "class C{static{(()=>{try{}catch(await){}})}};true"),
        Ok(Value::Bool(true))
    );
    for source in [
        "class C{constructor(value){this.value=value;}twice(){return this.value*2;}static create(value){return new C(value);}}let instance=C.create(21);instance.twice()===42&&instance.constructor===C&&Object.keys(C.prototype).length===0",
        "class C{get value(){return this.stored;}set value(value){this.stored=value;}static name(){return 'method';}}let instance=new C;instance.value=7;instance.value===7&&C.name()==='method'",
        "class C{method(){return this===undefined;}}let method=C.prototype.method;let rejected=false;try{C()}catch(error){rejected=error instanceof TypeError;}method()&&rejected",
        "let rejected=false;try{class C{static ['prototype'](){}}}catch(error){rejected=error instanceof TypeError;}rejected",
        "class C{constructor(value){this.value=value;}*items(){yield 1;yield this.value;}static *single(){yield 3;}}let iter=(new C(2)).items();iter.next().value===1&&iter.next().value===2&&C.single().next().value===3",
        "class C{static{this.answer=41;this.answer++;}}C.answer===42",
        "class C{static{this.self=C;}}C.self===C",
        "class C{first=1;second=this.first+1;static first=3;static second=this.first+1;}let value=new C;value.first===1&&value.second===2&&C.first===3&&C.second===4&&Object.keys(value).length===2",
        "let rejected=false;try{class C{static prototype=1;}}catch(error){rejected=error instanceof TypeError;}rejected",
        "let C=class Inner{static{this.self=Inner;}value(){return Inner;}};let value=new C;C.self===C&&value.value()===C&&typeof Inner==='undefined'",
        "let C=class Inner{replace(){Inner=1;}};let rejected=false;try{(new C).replace()}catch(error){rejected=error instanceof TypeError;}rejected",
    ] {
        assert_eq!(execute(&mut vm, source), Ok(Value::Bool(true)), "{source}");
    }
    let invalid = parse("class C{static{try{}catch(await){}}}").unwrap_err();
    assert!(invalid.known_syntax);
}

#[test]
fn derived_classes_construct_through_super_and_keep_home_object_receivers() {
    let mut vm = Vm::default();
    for source in [
        "class Base{constructor(value){this.value=value;}method(){return this.value+1;}get label(){return this.value;}set label(value){this.value=value;}static number(){return 7;}}class Derived extends Base{constructor(value){super(value*2);this.after=super.method();}method(){return super.method()+1;}static number(){return super.number()+1;}}let value=new Derived(2);value.value===4&&value.after===5&&value.method()===6&&value.label===4&&(value.label=5,value.value===5)&&Derived.number()===8&&value instanceof Derived&&value instanceof Base&&Object.getPrototypeOf(Derived)===Base&&Object.getPrototypeOf(Derived.prototype)===Base.prototype",
        "class Base{constructor(value){this.value=value;}}class Derived extends Base{}let value=new Derived(3);value.value===3&&value instanceof Derived&&value instanceof Base",
        "class Base{constructor(first,second){this.value=first+second;}}class Derived extends Base{constructor(...values){super(...values);}}(new Derived(2,3)).value===5",
        "class Base{constructor(value){this.value=value;}}class Derived extends Base{copy=this.value;constructor(value){super(value);this.observed=this.copy;}}(new Derived(3)).observed===3",
        "class Base{constructor(value){this.value=value;}}class Derived extends Base{constructor(value){(()=>super(value+1))();this.observed=this.value;}}(new Derived(3)).observed===4",
        "let executed=false;class Base{}class Derived extends Base{field=eval('executed=true;()=>super();')}let syntax=false;try{new Derived}catch(error){syntax=error instanceof SyntaxError;}syntax&&!executed",
        "class C{write=()=>{super.value=7;}static writeStatic=()=>{super.value=11;}}let value=new C;value.write();C.writeStatic();value.value===7&&C.value===11",
        "class Base{static get answer(){return this.value;}static set answer(value){this.value=value;}}class Derived extends Base{}Derived.answer=42;Derived.answer===42&&Base.value===undefined",
        "class Base{constructor(){this._count=1;}get count(){return this._count;}set count(value){this._count=value;}*items(){yield this.count;}}class Derived extends Base{constructor(){super();}*items(){yield super.items().next().value;yield super.count++;}}let value=new Derived;let iter=value.items();iter.next().value===1&&iter.next().value===1&&iter.next().done&&value.count===2",
        "class Base{method(){return this.value;}static method(){return this.value;}}class Derived extends Base{constructor(){super();this.value=7;}method(){return (()=>super.method())();}static method(){this.value=11;return (()=>super.method())();}}let value=new Derived;value.method()===7&&Derived.method()===11",
    ] {
        assert_eq!(execute(&mut vm, source), Ok(Value::Bool(true)), "{source}");
    }
}

#[test]
fn class_async_method_syntax_is_classified_as_an_execution_gap() {
    let program = parse(
        "class Derived extends Base{async method(){return 1;}static async *items(){yield 2;}}",
    )
    .unwrap();
    assert!(matches!(
        compile(&program),
        Err(blueice_bluejs::CompileError::Unsupported("async functions"))
    ));
    let program = parse("class Base{async method(){return 1;}}class Derived extends Base{async method(value=super.method()){return await value;}static async *items(){for await(let item of [])yield* await super.method();}}").unwrap();
    assert!(matches!(
        compile(&program),
        Err(blueice_bluejs::CompileError::Unsupported("async functions"))
    ));
    let program = parse("async function helper(){return await 1;}").unwrap();
    assert!(matches!(
        compile(&program),
        Err(blueice_bluejs::CompileError::Unsupported("async functions"))
    ));
    let program = parse("async function helper(){return new.target;}").unwrap();
    assert!(matches!(
        compile(&program),
        Err(blueice_bluejs::CompileError::Unsupported("async functions"))
    ));
    let program = parse("let helper=async value=>await value;").unwrap();
    assert!(matches!(
        compile(&program),
        Err(blueice_bluejs::CompileError::Unsupported("async functions"))
    ));
    assert!(parse("class C{async constructor(){}}").is_err());
    assert!(parse("class C{async\nmethod(){}}").is_ok());
    for source in [
        "class C{field=super();}",
        "class C{static field=()=>super();}",
        "class C{method(){super();}}",
        "class C{static method(){super();}}",
        "class C{get field(){super();}}",
        "class C{static set field(value){super();}}",
        "class C{constructor(){super();}}",
        "class C{static{super();}}",
    ] {
        let error = parse(source).unwrap_err();
        assert!(error.known_syntax, "{source}: {error:?}");
    }
    assert!(
        parse("class Base{}class Derived extends Base{constructor(){(()=>super())();}}").is_ok()
    );
    let program =
        parse("class Derived extends Base{field=1;constructor(){if(true)super();}}").unwrap();
    assert!(matches!(
        compile(&program),
        Err(blueice_bluejs::CompileError::Unsupported(
            "instance fields in an explicit derived constructor without a direct super() call"
        ))
    ));
}

#[test]
fn empty_async_case_declarations_instantiate_before_execution_support() {
    for source in [
        "switch(0){default:async function x(){}}x;",
        "switch(0){default:async function*x(){}}x;",
    ] {
        assert!(
            matches!(execute(&mut Vm::default(), source), Err(RuntimeError::ReferenceError(name)) if name == "x"),
            "{source}"
        );
    }
    for source in [
        "async function x(){}typeof x==='function'",
        "async function*x(){}typeof x==='function'",
    ] {
        assert_eq!(
            execute(&mut Vm::default(), source),
            Ok(Value::Bool(true)),
            "{source}"
        );
    }
    for source in ["(async function(){})()", "(async function*(){})()"] {
        assert_eq!(
            execute(&mut Vm::default(), source),
            Err(RuntimeError::Unsupported("async function execution")),
            "{source}"
        );
    }
}

#[test]
fn function_and_inheritance_early_errors_are_classified() {
    for source in [
        "function f(){super();}",
        "function f(){super.value;}",
        "function f(...rest=[]){}",
        "function f(...rest,){}",
        "class C{async method(value=await){}}",
        "class C{*method(value=yield){}}",
        "class C{static{function await(){}}}",
    ] {
        let error = parse(source).unwrap_err();
        assert!(error.known_syntax, "{source}: {error:?}");
    }
    assert!(parse("class C{\\u0065xtends(){return 1;}}").is_ok());
    for source in [
        "function f(a=0,a){}",
        "function f(a=0){'use strict';}",
        "'use strict';function f(arguments){}",
        "'use strict';function*g(){function f(value=yield){unbound=value;}}",
    ] {
        let program = parse(source).unwrap();
        assert!(
            matches!(
                compile(&program),
                Err(blueice_bluejs::CompileError::InvalidSyntax(_))
            ),
            "{source}"
        );
    }
}

#[test]
fn class_home_metadata_survives_minor_collection_while_a_generator_is_suspended() {
    let mut vm = Vm::new(VmConfig {
        heap: HeapConfig {
            nursery_capacity: 1,
            major_threshold_bytes: 256,
            max_heap_bytes: 512 * 1024,
        },
        ..VmConfig::default()
    })
    .unwrap();
    assert_eq!(
        execute(
            &mut vm,
            "class Base{get answer(){return this.value;}}class Derived extends Base{constructor(){super();this.value=7;}*items(){let read=()=>super.answer;yield 1;yield read();}}let iter=(new Derived).items();let first=iter.next();let padding=[{},{},{},{},{},{},{},{}];first.value===1&&iter.next().value===7",
        ),
        Ok(Value::Bool(true))
    );
}

#[test]
fn with_scopes_resolve_properties_and_unwind_at_handlers() {
    let mut vm = Vm::default();
    assert_eq!(
        execute(
            &mut vm,
            "let object={first:'one',second:'two'};let caught;try{with(object){first='changed';throw second}}catch(error){caught=error}object.first==='changed'&&caught==='two'",
        ),
        Ok(Value::Bool(true))
    );
    assert!(compile(&parse("'use strict';with({}){} ").unwrap()).is_err());
}

#[test]
fn finally_runs_for_normal_throw_return_break_and_continue_completions() {
    let mut vm = Vm::default();
    for source in [
        "let trace='';try{trace+='t';}finally{trace+='f';}trace==='tf'",
        "let trace='';try{throw 1;}catch(error){trace+='c';}finally{trace+='f';}trace==='cf'",
        "let trace='';function f(){try{return 1;}finally{trace+='f';}}f()===1&&trace==='f'",
        "let trace='';for(let i=0;i<3;i++){try{if(i===0)continue;if(i===1)break;}finally{trace+=i;}}trace==='01'",
        "let count=0;let trace='';while(count<2){try{count++;break;}finally{trace+='f';continue;}}count===2&&trace==='ff'",
        "let trace='';try{while(true){try{break;}finally{trace+='i';}}trace+='o';}finally{trace+='f';}trace==='iof'",
        "let trace='';function f(){try{throw 1;}finally{try{trace+='i';}finally{trace+='f';}}}try{f();}catch(error){trace+=error;}trace==='if1'",
    ] {
        assert_eq!(execute(&mut vm, source), Ok(Value::Bool(true)), "{source}");
    }
}

#[test]
fn finalizer_overrides_prior_abrupt_completion_and_preserves_normal_values() {
    let mut vm = Vm::default();
    assert_eq!(execute(&mut vm, "try{1}finally{2}"), Ok(Value::Number(1.0)));
    assert_eq!(execute(&mut vm, "try{}finally{2}"), Ok(Value::Undefined));
    assert_eq!(
        execute(&mut vm, "function f(){try{return 1;}finally{return 2;}}f()"),
        Ok(Value::Number(2.0))
    );
    assert_eq!(
        execute(&mut vm, "function f(){try{return 1;}finally{throw 2;}}f()"),
        Err(RuntimeError::Thrown(Value::Number(2.0)))
    );
}

#[test]
fn caught_values_stay_rooted_and_host_limits_do_not_enter_catch() {
    let mut vm = Vm::new(VmConfig {
        heap: HeapConfig {
            nursery_capacity: 1,
            ..HeapConfig::default()
        },
        ..VmConfig::default()
    })
    .unwrap();
    let source = "let result;try{throw {answer:42};}catch(error){for(let i=0;i<20;i++){let garbage={i};}result=error;}result.answer";
    assert_eq!(execute(&mut vm, source), Ok(Value::Number(42.0)));
    let mut limited = Vm::new(VmConfig {
        instruction_budget: 200,
        ..VmConfig::default()
    })
    .unwrap();
    assert_eq!(
        execute(&mut limited, "try{for(;;){}}catch(error){99}"),
        Err(RuntimeError::InstructionLimit)
    );
}

#[test]
fn switch_breaks_are_completion_aware_and_case_matching_is_strict() {
    let mut vm = Vm::default();
    for source in [
        "let trace='';switch(2){case 1:trace+='one';break;default:trace+='default';case 2:trace+='two';break;}trace==='two'",
        "let trace='';switch(1){case 1:trace+='a';case 2:trace+='b';break;default:trace+='c';}trace==='ab'",
        "let trace='';try{switch(1){case 1:trace+='switch';break;}trace+='try';}finally{trace+='finally';}trace==='switchtryfinally'",
        "let get;switch(0){case 0:let value=42;get=()=>value;break;}get()===42",
        "let x='outside';let probeExpr;let probeSelector;let probeStmt;switch(probeExpr=function(){return x;},null){case probeSelector=function(){return x;},null:probeStmt=function(){return x;};let x='inside';}probeExpr()==='outside'&&probeSelector()==='inside'&&probeStmt()==='inside'",
    ] {
        assert_eq!(execute(&mut vm, source), Ok(Value::Bool(true)), "{source}");
    }
    assert!(matches!(
        execute(&mut vm, "switch(0){default:function* hidden(){}}hidden"),
        Err(RuntimeError::ReferenceError(name)) if name == "hidden"
    ));
}

#[test]
fn switch_case_function_declarations_obey_lexical_early_errors() {
    for source in [
        "'use strict';switch(0){case 0:function f(){}default:function f(){}}",
        "switch(0){case 0:function f(){}default:function*f(){}}",
        "switch(0){case 0:function f(){}default:var f;}",
        "switch(0){case 0:var f;default:function f(){}}",
    ] {
        assert!(
            matches!(
                compile(&parse(source).unwrap()),
                Err(CompileError::InvalidSyntax(_))
            ),
            "{source}"
        );
    }
    assert!(compile(&parse("switch(0){default:function f(){}}").unwrap()).is_ok());
}

#[test]
fn for_in_enumerates_inherited_enumerable_strings_and_runs_finalizers() {
    let mut vm = Vm::default();
    for source in [
        "let trace='';let parent={inherited:1};let object={own:1,__proto__:parent};for(let key in object){try{trace+=key;}finally{trace+='!';}}trace==='own!inherited!'",
        "let trace='';for(let key in {first:1,second:2}){try{trace+=key;break;}finally{trace+='!';}}trace==='first!'",
        "let count=0;let finalized=0;for(let key in {first:1,second:2,third:3}){try{count++;break;}finally{finalized++;continue;}}count===3&&finalized===3",
    ] {
        assert_eq!(execute(&mut vm, source), Ok(Value::Bool(true)), "{source}");
    }
}
