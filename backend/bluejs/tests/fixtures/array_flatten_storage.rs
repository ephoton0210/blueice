// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed tiny Array flattening frame, owner, and public GC contracts.

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

#[cfg_attr(test, test)]
fn array_flatten_storage_frames_outlive_their_vm() {
    let mut vm = page();
    let view = observer(&vm);
    let source = run(&mut vm, "[1];").object_id().unwrap();
    let frames = vm.array_flatten_frames(source, 1, 1.0).unwrap();
    let bytes = frames.retained_slots();
    assert!(bytes > 0);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(frames);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn array_flatten_storage_neutral_flat_checks_terminal_depth_conversion() {
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
        "var warm=[],receiver={length:0},depth={valueOf(){markTerminal();return 0;}};receiver;",
    );
    let depth = neutral.lookup_global_name("depth").unwrap().unwrap();
    let roots = RealmCallRoots::new(2, owner.as_ref()).unwrap();
    roots.push(receiver.object_id().unwrap());
    roots.push(depth.object_id().unwrap());
    assert_eq!(
        neutral.with_realm_call_roots(roots, |vm| vm.array_flat(&receiver, &depth)),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(neutral.host_scheduling.is_none());
    assert!(neutral.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn array_flatten_storage_neutral_flat_map_checks_terminal_empty_mapper() {
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
        "var empty=[],receiver=[1],mapper=()=>{markTerminal();return empty;};receiver;",
    );
    let mapper = neutral.lookup_global_name("mapper").unwrap().unwrap();
    let roots = RealmCallRoots::new(2, owner.as_ref()).unwrap();
    roots.push(receiver.object_id().unwrap());
    roots.push(mapper.object_id().unwrap());
    assert_eq!(
        neutral.with_realm_call_roots(roots, |vm| {
            vm.array_flat_map(&receiver, &mapper, &Value::Undefined)
        }),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(neutral.host_scheduling.is_none());
    assert!(neutral.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn array_flatten_storage_public_getter_keeps_detached_inner_source_through_gc() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var source=[[1]];Object.defineProperty(source[0],'0',{get(){delete source[0];$262.gc();return {tag:7};}});
        source.flat(2)[0].tag===7;
    "#,
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn array_flatten_storage_public_holes_depth_and_outer_mapper_order_are_preserved() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var source=[,1,[,2,[3]]],log='';var mapped=[1,,3].flatMap((value,index)=>{log+=index;return [[value]];});
        JSON.stringify(source.flat())==='[1,2,[3]]'&&JSON.stringify(source.flat(2))==='[1,2,3]'&&
        JSON.stringify(source.flat(0))==='[1,[null,2,[3]]]'&&log==='02'&&JSON.stringify(mapped)==='[[1],[3]]';
    "#,
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn array_flatten_storage_growth_and_truncation_conserve_backing_credit() {
    let mut vm = page();
    let view = observer(&vm);
    let source = run(&mut vm, "[1];").object_id().unwrap();
    vm.synchronize_scheduling_usage().unwrap();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let mut frames = vm.array_flatten_frames(source, 1, 2.0).unwrap();
    for depth in [1.0, 0.0] {
        frames
            .push(ArrayFlattenFrame {
                source,
                len: 1,
                index: 0,
                depth,
            })
            .unwrap();
    }
    let bytes = frames.retained_slots();
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before + bytes);
    frames.truncate(0);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before + bytes);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(frames);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn array_flatten_storage_neutral_root_loan_keeps_its_document_credit() {
    let source = page();
    let view = observer(&source);
    let owner = source.argument_list_owner().unwrap();
    let mut neutral = Vm::default();
    let object = run(&mut neutral, "[1];").object_id().unwrap();
    let roots = RealmCallRoots::new(1, owner.as_ref()).unwrap();
    roots.push(object);
    let frames = neutral
        .with_realm_call_roots(roots, |vm| vm.array_flatten_frames(object, 1, 1.0))
        .unwrap();
    let bytes = frames.retained_slots();
    assert!(neutral.host_scheduling.is_none());
    assert!(neutral.realm_call_roots.is_none());
    drop(owner);
    drop(source);
    drop(neutral);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(frames);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn array_flatten_storage_frame_metadata_does_not_root_the_heap() {
    let mut vm = page();
    let view = observer(&vm);
    let source = vm.with_roots(|heap| heap.alloc_array(1, None)).unwrap();
    let frames = vm.array_flatten_frames(source, 1, 1.0).unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.heap.contains(source));
    let bytes = frames.retained_slots();
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(frames);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn array_flatten_storage_completed_source_is_collectible_during_a_later_getter() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    let child = run(&mut vm, "var source=[[1],2],child=source[0];child;")
        .object_id()
        .unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        Object.defineProperty(source,'1',{get(){delete source[0];child=null;$262.gc();return 2;}});
        source.flat().join(',')==='1,2';
    "#
        ),
        Value::Bool(true)
    );
    assert!(!vm.heap.contains(child));
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn array_flatten_storage_throws_restore_the_outer_roots_and_stack() {
    for mapped in [false, true] {
        let mut vm = page();
        let view = observer(&vm);
        let receiver = run(&mut vm, "var receiver=[[1]],mapper=()=>{throw 7;};Object.defineProperty(receiver[0],'0',{get(){throw 7;}});receiver;");
        let mapper = vm.lookup_global_name("mapper").unwrap().unwrap();
        let owner = vm.argument_list_owner().unwrap();
        let roots = RealmCallRoots::new(1, owner.as_ref()).unwrap();
        roots.push(receiver.object_id().unwrap());
        vm.stack.push(Value::Number(9.0));
        vm.with_realm_call_roots(roots.clone(), |vm| {
            let result = if mapped {
                vm.array_flat_map(&receiver, &mapper, &Value::Undefined)
            } else {
                vm.array_flat(&receiver, &Value::Number(1.0))
            };
            assert_eq!(result, Err(RuntimeError::Thrown(Value::Number(7.0))));
            assert!(Rc::ptr_eq(vm.realm_call_roots.as_ref().unwrap(), &roots));
            assert_eq!(vm.stack, vec![Value::Number(9.0)]);
            Ok(())
        })
        .unwrap();
        assert!(vm.realm_call_roots.is_none());
        drop(roots);
        drop(owner);
        drop(vm);
        assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
    }
}

#[cfg_attr(test, test)]
fn array_flatten_storage_terminal_has_trap_stops_before_the_index_getter() {
    for mapped in [false, true] {
        let source = page();
        let owner = source.argument_list_owner().unwrap();
        let scope = source.host_scheduling.as_ref().unwrap().child();
        let entered = Rc::new(std::cell::Cell::new(false));
        let output = entered.clone();
        let mut neutral = Vm::default();
        neutral
            .install_host_function("markTerminal", 0, move |_: &[crate::HostValue]| {
                let _ = scope.fail();
                Ok(crate::HostValue::Bool(true))
            })
            .unwrap();
        neutral
            .install_host_function("observe", 0, move |_: &[crate::HostValue]| {
                output.set(true);
                Ok(crate::HostValue::Number(1.0))
            })
            .unwrap();
        let receiver = run(&mut neutral, "var receiver=new Proxy({length:1,get 0(){return observe();}},{has(){return markTerminal();}}),mapper=value=>value;receiver;");
        let mapper = neutral.lookup_global_name("mapper").unwrap().unwrap();
        let roots = RealmCallRoots::new(2, owner.as_ref()).unwrap();
        roots.push(receiver.object_id().unwrap());
        roots.push(mapper.object_id().unwrap());
        let result = neutral.with_realm_call_roots(roots, |vm| {
            if mapped {
                vm.array_flat_map(&receiver, &mapper, &Value::Undefined)
            } else {
                vm.array_flat(&receiver, &Value::Number(1.0))
            }
        });
        assert_eq!(result, Err(RuntimeError::SchedulingLimit));
        assert!(!entered.get());
        assert!(neutral.host_scheduling.is_none());
        assert!(neutral.realm_call_roots.is_none());
    }
}

#[cfg_attr(test, test)]
fn array_flatten_storage_terminal_index_getter_stops_before_the_mapper() {
    let source = page();
    let owner = source.argument_list_owner().unwrap();
    let scope = source.host_scheduling.as_ref().unwrap().child();
    let entered = Rc::new(std::cell::Cell::new(false));
    let output = entered.clone();
    let mut neutral = Vm::default();
    neutral
        .install_host_function("markTerminal", 0, move |_: &[crate::HostValue]| {
            let _ = scope.fail();
            Ok(crate::HostValue::Number(1.0))
        })
        .unwrap();
    neutral
        .install_host_function("observe", 0, move |_: &[crate::HostValue]| {
            output.set(true);
            Ok(crate::HostValue::Number(1.0))
        })
        .unwrap();
    let receiver = run(
        &mut neutral,
        "var receiver={length:1,get 0(){return markTerminal();}},mapper=()=>observe();receiver;",
    );
    let mapper = neutral.lookup_global_name("mapper").unwrap().unwrap();
    let roots = RealmCallRoots::new(2, owner.as_ref()).unwrap();
    roots.push(receiver.object_id().unwrap());
    roots.push(mapper.object_id().unwrap());
    assert_eq!(
        neutral.with_realm_call_roots(roots, |vm| {
            vm.array_flat_map(&receiver, &mapper, &Value::Undefined)
        }),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(!entered.get());
    assert!(neutral.host_scheduling.is_none());
    assert!(neutral.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn array_flatten_storage_terminal_nested_length_stops_before_indexed_reads() {
    for mapped in [false, true] {
        let source = page();
        let owner = source.argument_list_owner().unwrap();
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
                Ok(crate::HostValue::Undefined)
            })
            .unwrap();
        let receiver = run(&mut neutral, "var inner=new Proxy([1],{get(target,key){if(key==='length'){markTerminal();return 1;}if(key==='0')observe();return Reflect.get(target,key);}}),receiver=[inner],mapper=()=>inner;receiver;");
        let mapper = neutral.lookup_global_name("mapper").unwrap().unwrap();
        let roots = RealmCallRoots::new(2, owner.as_ref()).unwrap();
        roots.push(receiver.object_id().unwrap());
        roots.push(mapper.object_id().unwrap());
        assert_eq!(
            neutral.with_realm_call_roots(roots, |vm| {
                if mapped {
                    vm.array_flat_map(&receiver, &mapper, &Value::Undefined)
                } else {
                    vm.array_flat(&receiver, &Value::Number(1.0))
                }
            }),
            Err(RuntimeError::SchedulingLimit)
        );
        assert!(!entered.get());
        assert!(neutral.host_scheduling.is_none());
        assert!(neutral.realm_call_roots.is_none());
    }
}

fn native_bytes_at_leaf(large: bool) -> usize {
    let mut vm = page();
    let view = observer(&vm);
    let seen = Rc::new(std::cell::Cell::new(None));
    let output = seen.clone();
    vm.install_host_function("observe", 0, move |_: &[crate::HostValue]| {
        output.set(Some(view.host_scheduling_snapshot().payload_bytes));
        Ok(crate::HostValue::Undefined)
    })
    .unwrap();
    let source = run(&mut vm, "var source=[1];source;").object_id().unwrap();
    if large {
        vm.heap
            .set(
                source,
                "0",
                Value::String(JsString::from_code_units(vec![0x61; 32])),
            )
            .unwrap();
    }
    let target = run(
        &mut vm,
        "new Proxy({},{defineProperty(){observe();return true;}});",
    )
    .object_id()
    .unwrap();
    vm.array_flatten_into(target, source, 1, 0.0, None).unwrap();
    seen.get().unwrap()
}

#[cfg_attr(test, test)]
fn array_flatten_storage_leaf_handoff_keeps_one_primitive_lease() {
    let small = native_bytes_at_leaf(false);
    let large = native_bytes_at_leaf(true);
    assert_eq!(large - small, 64, "large={large}, small={small}");
}

#[cfg_attr(test, test)]
fn array_flatten_storage_refused_species_write_stops_after_the_first_leaf() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        ['flat','flatMap'].every(method=>{
            var source=[1,2],reads=0,writes=0;
            Object.defineProperty(source,'0',{get(){reads++;return 1;}});
            Object.defineProperty(source,'1',{get(){reads++;return 2;}});
            source.constructor={[Symbol.species]:function(){return new Proxy({},{defineProperty(){writes++;return false;}});}};
            try{if(method==='flat')source.flat();else source.flatMap(value=>value);}
            catch(error){return error instanceof TypeError&&reads===1&&writes===1;}return false;
        });
    "#
        ),
        Value::Bool(true)
    );
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn array_flatten_storage_length_depth_and_species_order_are_preserved() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var log='',source=new Proxy([1],{get(target,key){if(key==='length')log+='l';if(key==='constructor')log+='c';if(key==='0')log+='g';return Reflect.get(target,key);}});
        source.constructor={[Symbol.species]:function(){log+='s';return [];}};
        var result=source.flat({valueOf(){log+='d';return 1;}}),okay=result[0]===1&&log==='ldcsg';
        var untouched=true;Object.defineProperty(source,'constructor',{get(){untouched=false;throw 7;}});
        try{source.flatMap(0);}catch(error){okay=okay&&error instanceof TypeError&&untouched;}okay;
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn array_flatten_storage_late_inherited_indices_are_observed() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        ['flat','flatMap'].every(method=>{
            var source=[,1,,],prototype=Object.create(Array.prototype);Object.setPrototypeOf(source,prototype);
            Object.defineProperty(source,'1',{get(){prototype[2]=[3];return [1];}});
            var result=method==='flat'?source.flat():source.flatMap(value=>value);return result.join(',')==='1,3';
        });
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn array_flatten_storage_primitive_receiver_preserves_the_borrowed_this_arg() {
    let mut vm = page();
    let mapper = run(&mut vm, "(function(value,index,object){'use strict';return [typeof this==='string'&&this.length===32&&object[0]==='a'&&object[1]==='b'&&value===(index===0?'a':'b')];});");
    let context = Value::String(JsString::from_code_units(vec![0x61; 32]));
    let result = vm
        .array_flat_map(&Value::String("ab".into()), &mapper, &context)
        .unwrap()
        .object_id()
        .unwrap();
    assert_eq!(vm.heap.get(result, "0").unwrap(), Value::Bool(true));
    assert_eq!(vm.heap.get(result, "1").unwrap(), Value::Bool(true));
    assert!(vm.realm_call_roots.is_none());
    assert!(vm.stack.is_empty());
}

#[cfg_attr(test, test)]
fn array_flatten_storage_depth_throw_precedes_species_lookup() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var source=[1],seen=false,caught=false;Object.defineProperty(source,'constructor',{get(){seen=true;return undefined;}});
        try{source.flat({valueOf(){throw 7;}});}catch(error){caught=error===7;}caught&&!seen;
    "#
        ),
        Value::Bool(true)
    );
    assert!(vm.realm_call_roots.is_none());
    assert!(vm.stack.is_empty());
}

#[cfg_attr(test, test)]
fn array_flatten_storage_borrowed_inputs_survive_length_conversion_gc() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    let receiver = run(
        &mut vm,
        r#"
        var receiver={0:2,get length(){return {valueOf(){mapper=null;context=null;$262.gc();return 1;}};}};
        var mapper=function(value){'use strict';return [this.tag+value];},context={tag:5};receiver;
    "#,
    );
    let mapper = vm.lookup_global_name("mapper").unwrap().unwrap();
    let context = vm.lookup_global_name("context").unwrap().unwrap();
    let result = vm
        .array_flat_map(&receiver, &mapper, &context)
        .unwrap()
        .object_id()
        .unwrap();
    assert_eq!(vm.heap.get(result, "0").unwrap(), Value::Number(7.0));
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn array_flatten_storage_native_mapper_errors_keep_their_realm() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var child=$262.createRealm(),foreignError=child.global.TypeError;
        var foreignGetter=child.evalScript("Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,'byteLength').get;");
        var forward=false;try{[1].flatMap(foreignGetter);}catch(error){forward=error instanceof foreignError&&!(error instanceof TypeError);}
        child.global.parentGetter=Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,'byteLength').get;child.global.parentError=TypeError;
        forward&&child.evalScript("try{[1].flatMap(parentGetter);false;}catch(error){error instanceof parentError&&!(error instanceof TypeError);}");
    "#
        ),
        Value::Bool(true)
    );
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn array_flatten_storage_tail_called_mappers_finish_results_and_throws() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        function leaf(value){'use strict';return [value+1];}function mapper(value){'use strict';return leaf(value);}
        function fail(){'use strict';throw 7;}function bad(){'use strict';return fail();}
        var caught=false;try{[1].flatMap(bad);}catch(error){caught=error===7;}caught&&[1,2].flatMap(mapper).join(',')==='2,3';
    "#
        ),
        Value::Bool(true)
    );
    assert!(vm.pending_tail_call.is_none());
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn array_flatten_storage_promise_mapper_keeps_selected_values_through_gc() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    run(
        &mut vm,
        r#"
        var result=0;Promise.resolve().then(()=>{var source=[{tag:7}];result=source.flatMap(value=>{delete source[0];$262.gc();return [value];})[0].tag;});0;
    "#,
    );
    vm.run_promise_jobs().unwrap();
    assert_eq!(run(&mut vm, "result;"), Value::Number(7.0));
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn array_flatten_storage_manual_timer_and_promise_keep_selected_values() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    run(&mut vm, "delete globalThis.setTimeout;delete globalThis.clearTimeout;delete globalThis.setInterval;delete globalThis.clearInterval;delete globalThis.queueMicrotask;0;");
    vm.install_host_timer_functions().unwrap();
    run(
        &mut vm,
        r#"
        var result=0;setTimeout(()=>{var source=[[{tag:7}]],selected=source.flat();source=null;
            Promise.resolve().then(()=>{result=selected.flatMap(value=>{$262.gc();return [value.tag];})[0];});
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

#[cfg_attr(test, test)]
fn array_flatten_storage_empty_frames_reject_a_terminal_original_owner() {
    let source = page();
    let owner = source.argument_list_owner().unwrap();
    let mut neutral = Vm::default();
    let input = run(&mut neutral, "[];").object_id().unwrap();
    let target = run(&mut neutral, "({});").object_id().unwrap();
    let roots = RealmCallRoots::new(2, owner.as_ref()).unwrap();
    roots.push(input);
    roots.push(target);
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        neutral.with_realm_call_roots(roots, |vm| {
            vm.array_flatten_into(target, input, 0, 1.0, None)
        }),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(neutral.host_scheduling.is_none());
    assert!(neutral.realm_call_roots.is_none());
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_array_flatten_storage_contracts() {
        array_flatten_storage_frames_outlive_their_vm();
        array_flatten_storage_neutral_flat_checks_terminal_depth_conversion();
        array_flatten_storage_neutral_flat_map_checks_terminal_empty_mapper();
        array_flatten_storage_public_getter_keeps_detached_inner_source_through_gc();
        array_flatten_storage_public_holes_depth_and_outer_mapper_order_are_preserved();
        array_flatten_storage_growth_and_truncation_conserve_backing_credit();
        array_flatten_storage_neutral_root_loan_keeps_its_document_credit();
        array_flatten_storage_frame_metadata_does_not_root_the_heap();
        array_flatten_storage_completed_source_is_collectible_during_a_later_getter();
        array_flatten_storage_throws_restore_the_outer_roots_and_stack();
        array_flatten_storage_terminal_has_trap_stops_before_the_index_getter();
        array_flatten_storage_terminal_index_getter_stops_before_the_mapper();
        array_flatten_storage_terminal_nested_length_stops_before_indexed_reads();
        array_flatten_storage_leaf_handoff_keeps_one_primitive_lease();
        array_flatten_storage_refused_species_write_stops_after_the_first_leaf();
        array_flatten_storage_length_depth_and_species_order_are_preserved();
        array_flatten_storage_late_inherited_indices_are_observed();
        array_flatten_storage_primitive_receiver_preserves_the_borrowed_this_arg();
        array_flatten_storage_depth_throw_precedes_species_lookup();
        array_flatten_storage_borrowed_inputs_survive_length_conversion_gc();
        array_flatten_storage_native_mapper_errors_keep_their_realm();
        array_flatten_storage_tail_called_mappers_finish_results_and_throws();
        array_flatten_storage_promise_mapper_keeps_selected_values_through_gc();
        array_flatten_storage_manual_timer_and_promise_keep_selected_values();
        array_flatten_storage_empty_frames_reject_a_terminal_original_owner();
    }
}
