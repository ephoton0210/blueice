// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed tiny concat/slice ownership, early cancellation, and GC contracts.

use super::*;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}

fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: vm.host_scheduling.as_ref().map(SchedulingScope::child),
        ..Vm::default()
    }
}

fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}

fn concat_native_bytes_at_leaf(large: bool) -> usize {
    let mut vm = page();
    let view = observer(&vm);
    let seen = Rc::new(std::cell::Cell::new(None));
    let output = seen.clone();
    vm.install_host_function("observe", 0, move |_: &[crate::HostValue]| {
        output.set(Some(view.host_scheduling_snapshot().payload_bytes));
        Ok(crate::HostValue::Undefined)
    })
    .unwrap();
    let source = run(&mut vm, r#"
        var source=[1];source.constructor={[Symbol.species]:function(length){
            return new Proxy({length},{defineProperty(target,key,descriptor){if(key==='0')observe();return Reflect.defineProperty(target,key,descriptor);}});
        }};source;
    "#).object_id().unwrap();
    if large {
        vm.heap
            .set(
                source,
                "0",
                Value::String(JsString::from_code_units(vec![0x61; 32])),
            )
            .unwrap();
    }
    run(&mut vm, "source.concat();0;");
    seen.get().unwrap()
}

#[cfg_attr(test, test)]
fn array_copy_storage_concat_owns_one_leaf_copy_during_proxy_definition() {
    let small = concat_native_bytes_at_leaf(false);
    let large = concat_native_bytes_at_leaf(true);
    assert_eq!(large - small, 64, "large={large}, small={small}");
}

#[cfg_attr(test, test)]
fn array_copy_storage_slice_restores_the_outer_operand_stack() {
    let mut vm = page();
    let receiver = run(&mut vm, "[1];");
    vm.stack.push(Value::Number(9.0));
    let result = vm.array_slice(&receiver, &[Value::Number(0.0)]).unwrap();
    assert_eq!(
        vm.heap.get(result.object_id().unwrap(), "0").unwrap(),
        Value::Number(1.0)
    );
    assert_eq!(vm.stack, vec![Value::Number(9.0)]);
}

#[cfg_attr(test, test)]
fn array_copy_storage_terminal_slice_start_does_not_allocate_a_result() {
    let source = page();
    let owner = source.argument_list_owner().unwrap();
    let scope = source.host_scheduling.as_ref().unwrap().child();
    let mut neutral = Vm::default();
    neutral
        .install_host_function("markTerminal", 0, move |_: &[crate::HostValue]| {
            let _ = scope.fail();
            Ok(crate::HostValue::Undefined)
        })
        .unwrap();
    let receiver = run(
        &mut neutral,
        "var warm=[],receiver={length:0},start={valueOf(){markTerminal();return 0;}};receiver;",
    );
    let start = neutral.lookup_global_name("start").unwrap().unwrap();
    let roots = RealmCallRoots::new(2, owner.as_ref()).unwrap();
    roots
        .push(&neutral.heap, receiver.object_id().unwrap())
        .unwrap();
    roots
        .push(&neutral.heap, start.object_id().unwrap())
        .unwrap();
    let before = neutral.heap.stats();
    let before = before.nursery_objects + before.tenured_objects;
    let result = neutral.with_realm_call_roots(roots, |vm| vm.array_slice(&receiver, &[start]));
    let after = neutral.heap.stats();
    assert_eq!(after.nursery_objects + after.tenured_objects, before);
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(neutral.host_scheduling.is_none());
    assert!(neutral.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn array_copy_storage_public_slice_getter_value_survives_species_trap_gc() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var source=[1];Object.defineProperty(source,'0',{get(){return {tag:7};}});
        source.constructor={[Symbol.species]:function(length){return new Proxy({length},{get defineProperty(){delete source[0];$262.gc();return function(target,key,descriptor){$262.gc();return Reflect.defineProperty(target,key,descriptor);};}});}};
        source.slice()[0].tag===7;
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn array_copy_storage_public_holes_spreadability_and_species_are_preserved() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var source=[1,,3],log='';source.constructor={[Symbol.species]:function(length){log+=length;return {length};}};
        var spread={0:'a',length:1,[Symbol.isConcatSpreadable]:true},joined=source.concat(spread),sliced=source.slice(1,3);
        joined.length===4&&joined[0]===1&&!(1 in joined)&&joined[2]===3&&joined[3]==='a'&&sliced.length===2&&!(0 in sliced)&&sliced[1]===3&&log==='02';
    "#
        ),
        Value::Bool(true)
    );
}

fn terminal_pair() -> (Vm, Vm, Rc<std::cell::Cell<bool>>) {
    let source = page();
    let scope = source.host_scheduling.as_ref().unwrap().child();
    let entered = Rc::new(std::cell::Cell::new(false));
    let output = entered.clone();
    let mut neutral = Vm::default();
    neutral
        .install_host_function("markTerminal", 0, move |_: &[crate::HostValue]| {
            let _ = scope.fail();
            Ok(crate::HostValue::Undefined)
        })
        .unwrap();
    neutral
        .install_host_function("observe", 0, move |_: &[crate::HostValue]| {
            output.set(true);
            Ok(crate::HostValue::Number(1.0))
        })
        .unwrap();
    (source, neutral, entered)
}

fn loaned_copy(
    source: &Vm,
    neutral: &mut Vm,
    receiver: &Value,
    args: &[Value],
    concat: bool,
) -> Result<Value, RuntimeError> {
    let owner = source.argument_list_owner()?;
    let roots = RealmCallRoots::new(
        1 + args.iter().filter_map(Value::object_id).count(),
        owner.as_ref(),
    )?;
    roots.push(&neutral.heap, receiver.object_id().unwrap())?;
    args.iter()
        .filter_map(Value::object_id)
        .try_for_each(|id| roots.push(&neutral.heap, id))?;
    neutral.with_realm_call_roots(roots, |vm| {
        if concat {
            vm.array_concat(receiver, args)
        } else {
            vm.array_slice(receiver, args)
        }
    })
}

#[cfg_attr(test, test)]
fn array_copy_storage_exceptions_restore_outer_roots_and_stack() {
    for concat in [false, true] {
        for setup in [
            "var source=[1];Object.defineProperty(source,'constructor',{get(){throw 7;}});source;",
            "var source=[1];Object.defineProperty(source,'0',{get(){throw 7;}});source;",
            "var source=[1];source.constructor={[Symbol.species]:function(){return new Proxy({},{defineProperty(){throw 7;}});}};source;",
        ] {
            let mut vm = page();
            let view = observer(&vm);
            let receiver = run(&mut vm, setup);
            let owner = vm.argument_list_owner().unwrap();
            let roots = RealmCallRoots::new(1, owner.as_ref()).unwrap();
            roots.push(&vm.heap, receiver.object_id().unwrap()).unwrap();
            vm.stack.push(Value::Number(9.0));
            vm.with_realm_call_roots(roots.clone(), |vm| {
                let result = if concat { vm.array_concat(&receiver, &[]) } else { vm.array_slice(&receiver, &[]) };
                assert_eq!(result, Err(RuntimeError::Thrown(Value::Number(7.0))));
                assert!(Rc::ptr_eq(vm.realm_call_roots.as_ref().unwrap(), &roots));
                assert_eq!(vm.stack, vec![Value::Number(9.0)]);
                Ok(())
            }).unwrap();
            assert!(vm.realm_call_roots.is_none());
            drop(roots);
            drop(owner);
            drop(vm);
            assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
        }
    }
}

#[cfg_attr(test, test)]
fn array_copy_storage_concat_pins_arguments_through_species_gc() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    let receiver = run(
        &mut vm,
        r#"
        var source=[],first={tag:7},second={tag:8};Object.defineProperty(source,'constructor',{get(){
            first=null;second=null;$262.gc();return {[Symbol.species]:function(){return [];}};
        }});source;
    "#,
    );
    let first = vm.lookup_global_name("first").unwrap().unwrap();
    let second = vm.lookup_global_name("second").unwrap().unwrap();
    let result = vm.array_concat(&receiver, &[first, second]).unwrap();
    assert_eq!(
        vm.get_property(
            &vm.heap.get(result.object_id().unwrap(), "0").unwrap(),
            &"tag".into()
        )
        .unwrap(),
        Value::Number(7.0)
    );
    assert_eq!(
        vm.get_property(
            &vm.heap.get(result.object_id().unwrap(), "1").unwrap(),
            &"tag".into()
        )
        .unwrap(),
        Value::Number(8.0)
    );
    assert!(vm.realm_call_roots.is_none());
    assert!(vm.stack.is_empty());
}

#[cfg_attr(test, test)]
fn array_copy_storage_slice_pins_ranges_through_length_conversion_gc() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    let receiver = run(
        &mut vm,
        r#"
        var log='',receiver={0:7,length:{valueOf(){log+='l';start=null;end=null;$262.gc();return 1;}}};
        var start={valueOf(){log+='s';return 0;}},end={valueOf(){log+='e';return 1;}};receiver;
    "#,
    );
    let start = vm.lookup_global_name("start").unwrap().unwrap();
    let end = vm.lookup_global_name("end").unwrap().unwrap();
    let result = vm.array_slice(&receiver, &[start, end]).unwrap();
    assert_eq!(
        vm.heap.get(result.object_id().unwrap(), "0").unwrap(),
        Value::Number(7.0)
    );
    assert_eq!(run(&mut vm, "log;"), Value::String("lse".into()));
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn array_copy_storage_public_proxy_probe_order_is_preserved() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var log='',arg=new Proxy({0:'a',length:1,[Symbol.isConcatSpreadable]:true},{
            get(target,key){if(key===Symbol.isConcatSpreadable)log+='s';if(key==='length')log+='l';if(key==='0')log+='g';return Reflect.get(target,key);},
            has(target,key){log+='h'+key;return Reflect.has(target,key);}
        });var joined=[].concat(arg),okay=joined[0]==='a'&&log==='slh0g';
        log='';var source=new Proxy({length:2,1:7},{get(target,key){log+='g'+key;return Reflect.get(target,key);},has(target,key){log+='h'+key;return Reflect.has(target,key);}});
        var sliced=Array.prototype.slice.call(source);okay&&sliced.length===2&&!(0 in sliced)&&sliced[1]===7&&log==='glengthh0h1g1';
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn array_copy_storage_terminal_concat_marker_skips_length_getter() {
    let (source, mut neutral, entered) = terminal_pair();
    let receiver = run(&mut neutral, "var receiver=[],arg={get [Symbol.isConcatSpreadable](){markTerminal();return true;},get length(){observe();return 0;}};receiver;");
    let arg = neutral.lookup_global_name("arg").unwrap().unwrap();
    assert_eq!(
        loaned_copy(&source, &mut neutral, &receiver, &[arg], true),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(!entered.get());
    assert!(neutral.host_scheduling.is_none());
    assert!(neutral.realm_call_roots.is_none());
    assert!(neutral.stack.is_empty());
}

#[cfg_attr(test, test)]
fn array_copy_storage_terminal_concat_length_skips_index_probes() {
    let (source, mut neutral, entered) = terminal_pair();
    let receiver = run(&mut neutral, "var receiver=[],arg=new Proxy({[Symbol.isConcatSpreadable]:true,length:{valueOf(){markTerminal();return 1;}},0:7},{has(){observe();return true;}});receiver;");
    let arg = neutral.lookup_global_name("arg").unwrap().unwrap();
    assert_eq!(
        loaned_copy(&source, &mut neutral, &receiver, &[arg], true),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(!entered.get());
    assert!(neutral.host_scheduling.is_none());
    assert!(neutral.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn array_copy_storage_terminal_slice_start_skips_end_conversion() {
    let (source, mut neutral, entered) = terminal_pair();
    let receiver = run(&mut neutral, "var warm=[],receiver={length:0},start={valueOf(){markTerminal();return 0;}},end={valueOf(){observe();return 0;}};receiver;");
    let start = neutral.lookup_global_name("start").unwrap().unwrap();
    let end = neutral.lookup_global_name("end").unwrap().unwrap();
    assert_eq!(
        loaned_copy(&source, &mut neutral, &receiver, &[start, end], false),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(!entered.get());
    assert!(neutral.host_scheduling.is_none());
    assert!(neutral.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn array_copy_storage_terminal_slice_end_prevents_result_allocation() {
    let (source, mut neutral, _) = terminal_pair();
    let receiver = run(
        &mut neutral,
        "var warm=[],receiver={length:1},end={valueOf(){markTerminal();return 1;}};receiver;",
    );
    let end = neutral.lookup_global_name("end").unwrap().unwrap();
    let before = neutral.heap.stats();
    let before = before.nursery_objects + before.tenured_objects;
    let result = loaned_copy(
        &source,
        &mut neutral,
        &receiver,
        &[Value::Number(0.0), end],
        false,
    );
    let after = neutral.heap.stats();
    assert_eq!(after.nursery_objects + after.tenured_objects, before);
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(neutral.host_scheduling.is_none());
    assert!(neutral.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn array_copy_storage_terminal_has_stops_before_index_getter() {
    for concat in [false, true] {
        let (source, mut neutral, entered) = terminal_pair();
        let receiver = run(&mut neutral, "var receiver=new Proxy({length:1,[Symbol.isConcatSpreadable]:true,get 0(){return observe();}},{has(){markTerminal();return true;}});receiver;");
        assert_eq!(
            loaned_copy(&source, &mut neutral, &receiver, &[], concat),
            Err(RuntimeError::SchedulingLimit)
        );
        assert!(!entered.get());
        assert!(neutral.host_scheduling.is_none());
        assert!(neutral.realm_call_roots.is_none());
    }
}

fn concat_native_bytes_at_argument(large: bool) -> usize {
    let mut vm = page();
    let view = observer(&vm);
    let seen = Rc::new(std::cell::Cell::new(None));
    let output = seen.clone();
    vm.install_host_function("observe", 0, move |_: &[crate::HostValue]| {
        output.set(Some(view.host_scheduling_snapshot().payload_bytes));
        Ok(crate::HostValue::Undefined)
    })
    .unwrap();
    let receiver = run(&mut vm, "var source=[];source.constructor={[Symbol.species]:function(){return new Proxy({},{defineProperty(target,key,descriptor){if(key==='0')observe();return Reflect.defineProperty(target,key,descriptor);}});}};source;");
    let owner = vm.argument_list_owner().unwrap();
    let mut arguments = TransportVector::with_capacity(1, owner.as_ref()).unwrap();
    let value = if large {
        Value::String(JsString::from_code_units(vec![0x61; 32]))
    } else {
        Value::Number(1.0)
    };
    arguments
        .push_owned(NativeValue::adopt(value, owner.as_ref()).unwrap())
        .unwrap();
    vm.array_concat(&receiver, &arguments).unwrap();
    seen.get().unwrap()
}

#[cfg_attr(test, test)]
fn array_copy_storage_concat_keeps_original_argument_and_one_admitted_copy() {
    let small = concat_native_bytes_at_argument(false);
    let large = concat_native_bytes_at_argument(true);
    assert_eq!(large - small, 128, "large={large}, small={small}");
}

#[cfg_attr(test, test)]
fn array_copy_storage_terminal_final_length_set_rejects_success() {
    for concat in [false, true] {
        let (source, mut neutral, _) = terminal_pair();
        let receiver = run(
            &mut neutral,
            r#"
            var receiver=[1];receiver.constructor={[Symbol.species]:function(){return new Proxy({},{
                defineProperty(){return true;},set(target,key,value){if(key==='length')markTerminal();return true;}
            });}};receiver;
        "#,
        );
        assert_eq!(
            loaned_copy(&source, &mut neutral, &receiver, &[], concat),
            Err(RuntimeError::SchedulingLimit)
        );
        assert!(neutral.host_scheduling.is_none());
        assert!(neutral.realm_call_roots.is_none());
        assert!(neutral.stack.is_empty());
    }
}

#[cfg_attr(test, test)]
fn array_copy_storage_slice_ignores_extra_arguments_and_non_array_constructor() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var source={length:2,0:'a',1:'b',get constructor(){throw 7;}},ignored={valueOf(){throw 8;}};
        Array.prototype.slice.call(source,0,2,ignored).join(',')==='a,b';
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn array_copy_storage_concat_preserves_primitives_and_nonspread_identity() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var array=[2,3];array[Symbol.isConcatSpreadable]=false;var symbol=Symbol('identity');
        var result=[1].concat(array,null,7,'x',symbol,1n),boxed=Array.prototype.concat.call(5,'a');
        result.length===7&&result[1]===array&&result[2]===null&&result[3]===7&&result[4]==='x'&&result[5]===symbol&&result[6]===1n&&
        typeof boxed[0]==='object'&&boxed[0].valueOf()===5&&boxed[1]==='a';
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn array_copy_storage_live_mutations_preserve_holes() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        ['concat','slice'].every(method=>{
            var source=[1,2,3];Object.defineProperty(source,'0',{get(){delete source[1];source[2]=7;return 1;}});
            var result=source[method]();return result.length===3&&result[0]===1&&!(1 in result)&&result[2]===7;
        });
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn array_copy_storage_species_mutation_preserves_length_observation_order() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var log='';function check(method){var source=[1,2];source.constructor={[Symbol.species]:function(length){log+=length;source.length=0;return [];}};return source[method]();}
        var joined=check('concat'),sliced=check('slice');joined.length===0&&sliced.length===2&&!(0 in sliced)&&!(1 in sliced)&&log==='02';
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn array_copy_storage_completed_slice_does_not_retain_its_receiver() {
    let mut vm = page();
    run(&mut vm, "0;");
    let source = vm.with_roots(|heap| heap.alloc_array(1, None)).unwrap();
    vm.heap.set(source, "0", Value::Number(7.0)).unwrap();
    let result = vm
        .array_slice(&Value::Object(source), &[])
        .unwrap()
        .object_id()
        .unwrap();
    let root = vm.heap.root(result).unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.heap.contains(source));
    assert_eq!(vm.heap.get(result, "0").unwrap(), Value::Number(7.0));
    vm.heap.unroot(root).unwrap();
    assert!(vm.stack.is_empty());
}

#[cfg_attr(test, test)]
fn array_copy_storage_foreign_getter_errors_keep_their_realm() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    // Stop an unexpected receiver snapshot at its second indexed read. The
    // regression must expose re-entry without relying on recursion limits.
    assert_eq!(
        run(
            &mut vm,
            r#"
        var child=$262.createRealm(),foreignError=child.global.TypeError;
        var foreignGetter=child.evalScript("Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,'byteLength').get;");
        var forward=['concat','slice'].map(method=>{var source=[1],calls=0;Object.defineProperty(source,'0',{get(){if(++calls>1)throw 7;return Reflect.apply(foreignGetter,this,[]);}});try{source[method]();}catch(error){return method+':'+error.name+':'+(error instanceof foreignError)+':'+(error instanceof TypeError)+':'+calls;}return method+':no-error';}).join(';');
        child.global.parentGetter=Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,'byteLength').get;child.global.parentError=TypeError;
        var reverse=child.evalScript("['concat','slice'].map(method=>{var source=[1],calls=0;Object.defineProperty(source,'0',{get(){if(++calls>1)throw 7;return Reflect.apply(parentGetter,this,[]);}});try{source[method]();}catch(error){return method+':'+error.name+':'+(error instanceof parentError)+':'+(error instanceof TypeError)+':'+calls;}return method+':no-error';}).join(';');");
        forward+'|'+reverse;
    "#
        ),
        Value::String("concat:TypeError:true:false:1;slice:TypeError:true:false:1|concat:TypeError:true:false:1;slice:TypeError:true:false:1".into())
    );
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn array_copy_storage_foreign_buffer_getters_read_slots_without_snapshotting() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var child=$262.createRealm(),foreignError=child.global.TypeError;
        var ignored=[1],calls=0;
        Object.defineProperty(ignored,'0',{get(){calls++;throw 7;}});
        var buffer=new ArrayBuffer(3,{maxByteLength:5}),shared=new SharedArrayBuffer(2,{maxByteLength:4});
        var cases=[['ArrayBuffer','byteLength',buffer,3],['ArrayBuffer','maxByteLength',buffer,5],['ArrayBuffer','resizable',buffer,true],['ArrayBuffer','detached',buffer,false],['ArrayBuffer','immutable',buffer,false],['SharedArrayBuffer','byteLength',shared,2],['SharedArrayBuffer','maxByteLength',shared,4],['SharedArrayBuffer','growable',shared,true]];
        cases.every(([kind,key,receiver,expected])=>{
            var getter=child.evalScript("Object.getOwnPropertyDescriptor("+kind+".prototype,'"+key+"').get;");
            if(Reflect.apply(getter,receiver,[ignored])!==expected||getter.call(receiver,ignored)!==expected)return false;
            try{Reflect.apply(getter,ignored,[]);}catch(error){return error instanceof foreignError&&!(error instanceof TypeError);}
            return false;
        })&&calls===0;
    "#
        ),
        Value::Bool(true)
    );
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn array_copy_storage_tail_called_species_finish_results_and_throws() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        function make(length){'use strict';return {length};}function Good(length){'use strict';return make(length);}
        function fail(){'use strict';throw 7;}function Bad(){'use strict';return fail();}
        ['concat','slice'].every(method=>{var source=[1];source.constructor={[Symbol.species]:Good};var result=source[method]();source.constructor[Symbol.species]=Bad;var caught=false;try{source[method]();}catch(error){caught=error===7;}return result[0]===1&&result.length===1&&caught;});
    "#
        ),
        Value::Bool(true)
    );
    assert!(vm.pending_tail_call.is_none());
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn array_copy_storage_promise_getter_keeps_selected_values_through_gc() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    run(
        &mut vm,
        r#"
        var result=0;Promise.resolve().then(()=>{var source=[1];Object.defineProperty(source,'0',{get(){$262.gc();return {tag:7};}});result=source.concat().slice()[0].tag;});0;
    "#,
    );
    vm.run_promise_jobs().unwrap();
    assert_eq!(run(&mut vm, "result;"), Value::Number(7.0));
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn array_copy_storage_manual_timer_and_promise_keep_selected_values() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    run(&mut vm, "delete globalThis.setTimeout;delete globalThis.clearTimeout;delete globalThis.setInterval;delete globalThis.clearInterval;delete globalThis.queueMicrotask;0;");
    vm.install_host_timer_functions().unwrap();
    run(
        &mut vm,
        r#"
        var result=0;setTimeout(()=>{var source=[{tag:7}],selected=source.concat();source=null;
            Promise.resolve().then(()=>{$262.gc();result=selected.slice()[0].tag;});
        },1);0;
    "#,
    );
    let turn = vm.run_due_host_timers(1).unwrap();
    assert_eq!(turn.tasks_run, 1);
    assert_eq!(turn.uncaught_errors, 0);
    assert_eq!(vm.next_host_timer_due(), None);
    assert_eq!(run(&mut vm, "result;"), Value::Number(7.0));
    assert!(vm.realm_call_roots.is_none());
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_array_copy_storage_contracts() {
        array_copy_storage_concat_owns_one_leaf_copy_during_proxy_definition();
        array_copy_storage_slice_restores_the_outer_operand_stack();
        array_copy_storage_terminal_slice_start_does_not_allocate_a_result();
        array_copy_storage_public_slice_getter_value_survives_species_trap_gc();
        array_copy_storage_public_holes_spreadability_and_species_are_preserved();
        array_copy_storage_exceptions_restore_outer_roots_and_stack();
        array_copy_storage_concat_pins_arguments_through_species_gc();
        array_copy_storage_slice_pins_ranges_through_length_conversion_gc();
        array_copy_storage_public_proxy_probe_order_is_preserved();
        array_copy_storage_terminal_concat_marker_skips_length_getter();
        array_copy_storage_terminal_concat_length_skips_index_probes();
        array_copy_storage_terminal_slice_start_skips_end_conversion();
        array_copy_storage_terminal_slice_end_prevents_result_allocation();
        array_copy_storage_terminal_has_stops_before_index_getter();
        array_copy_storage_concat_keeps_original_argument_and_one_admitted_copy();
        array_copy_storage_terminal_final_length_set_rejects_success();
        array_copy_storage_slice_ignores_extra_arguments_and_non_array_constructor();
        array_copy_storage_concat_preserves_primitives_and_nonspread_identity();
        array_copy_storage_live_mutations_preserve_holes();
        array_copy_storage_species_mutation_preserves_length_observation_order();
        array_copy_storage_completed_slice_does_not_retain_its_receiver();
        array_copy_storage_foreign_getter_errors_keep_their_realm();
        array_copy_storage_foreign_buffer_getters_read_slots_without_snapshotting();
        array_copy_storage_tail_called_species_finish_results_and_throws();
        array_copy_storage_promise_getter_keeps_selected_values_through_gc();
        array_copy_storage_manual_timer_and_promise_keep_selected_values();
    }
}
