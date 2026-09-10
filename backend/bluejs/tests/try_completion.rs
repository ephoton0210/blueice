// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public-pipeline regressions for Completion records and try handlers.

use blueice_bluejs::{compile, parse, HeapConfig, RuntimeError, Value, Vm, VmConfig};

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
    ] {
        assert_eq!(execute(&mut vm, source), Ok(Value::Bool(true)), "{source}");
    }
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
