// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed tiny sort snapshot, terminal-owner and streaming root contracts.

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
fn array_sort_storage_input_keeps_its_backing_and_payload_after_vm_destruction() {
    let vm = page();
    let view = observer(&vm);
    let mut values = vm.array_sort_input().unwrap();
    vm.push_array_sort_input(
        &mut values,
        Value::String(JsString::from_code_units(vec![0x61; 12])),
    )
    .unwrap();
    let bytes =
        values.capacity() * std::mem::size_of::<Value>() + host_scheduling::values_bytes(&values);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(values);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn array_sort_storage_completed_write_retires_its_value_before_the_next_setter() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    run(
        &mut vm,
        r#"
        var first={tag:1},second={tag:2},weak=new WeakRef(first),collected=false;
        var target={0:first,1:second,length:2};
        var source=new Proxy(target,{set(target,key,value){
            if(key==='0'){delete target[0];delete target[1];first=second=null;return true;}
            if(key==='1'){$262.gc();collected=weak.deref()===undefined;return true;}
            return Reflect.set(target,key,value);
        }});0;
    "#,
    );
    vm.finish_host_checkpoint().unwrap();
    assert_eq!(
        run(
            &mut vm,
            "Array.prototype.sort.call(source,(a,b)=>a.tag-b.tag);collected;"
        ),
        Value::Bool(true)
    );
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn array_sort_storage_terminal_length_rejects_both_empty_results() {
    for copy in [false, true] {
        let source = page();
        let owner = source.argument_list_owner().unwrap();
        let scope = source.host_scheduling.as_ref().unwrap().child();
        let mut neutral = Vm::default();
        neutral
            .install_host_function("markTerminal", 0, move |_: &[crate::HostValue]| {
                let _ = scope.fail();
                Ok(crate::HostValue::Number(0.0))
            })
            .unwrap();
        let receiver = run(
            &mut neutral,
            "({length:{valueOf(){return markTerminal();}}});",
        );
        let roots = realm_call_storage::RealmCallRoots::new(1, owner.as_ref()).unwrap();
        roots
            .push(&neutral.heap, receiver.object_id().unwrap())
            .unwrap();
        let result = neutral.with_realm_call_roots(roots, |vm| {
            if copy {
                vm.array_to_sorted(&receiver, &Value::Undefined)
            } else {
                vm.array_sort(&receiver, &Value::Undefined)
            }
        });
        assert_eq!(result, Err(RuntimeError::SchedulingLimit), "copy={copy}");
        assert!(neutral.stack.is_empty() && neutral.realm_call_roots.is_none());
    }
}

#[cfg_attr(test, test)]
fn array_sort_storage_terminal_has_stops_before_the_index_getter() {
    let source = page();
    let owner = source.argument_list_owner().unwrap();
    let scope = source.host_scheduling.as_ref().unwrap().child();
    let seen = Rc::new(std::cell::Cell::new(false));
    let output = seen.clone();
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
            Ok(crate::HostValue::Number(7.0))
        })
        .unwrap();
    let receiver = run(
        &mut neutral,
        "new Proxy({length:1,get 0(){return observe();}},{has(){return markTerminal();}});",
    );
    let roots = realm_call_storage::RealmCallRoots::new(1, owner.as_ref()).unwrap();
    roots
        .push(&neutral.heap, receiver.object_id().unwrap())
        .unwrap();
    let result =
        neutral.with_realm_call_roots(roots, |vm| vm.array_sort(&receiver, &Value::Undefined));
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(!seen.get());
}

#[cfg_attr(test, test)]
fn array_sort_storage_terminal_first_string_skips_second_conversion() {
    let source = page();
    let owner = source.argument_list_owner().unwrap();
    let scope = source.host_scheduling.as_ref().unwrap().child();
    let seen = Rc::new(std::cell::Cell::new(false));
    let output = seen.clone();
    let mut neutral = Vm::default();
    neutral
        .install_host_function("markTerminal", 0, move |_: &[crate::HostValue]| {
            let _ = scope.fail();
            Ok(crate::HostValue::String("a".into()))
        })
        .unwrap();
    neutral
        .install_host_function("observe", 0, move |_: &[crate::HostValue]| {
            output.set(true);
            Ok(crate::HostValue::String("b".into()))
        })
        .unwrap();
    let receiver = run(
        &mut neutral,
        "[{toString(){return observe();}},{toString(){return markTerminal();}}];",
    );
    let roots = realm_call_storage::RealmCallRoots::new(1, owner.as_ref()).unwrap();
    roots
        .push(&neutral.heap, receiver.object_id().unwrap())
        .unwrap();
    let result =
        neutral.with_realm_call_roots(roots, |vm| vm.array_sort(&receiver, &Value::Undefined));
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(!seen.get());
}

#[cfg_attr(test, test)]
fn array_sort_storage_inputs_pin_the_comparator_before_length_getter_gc() {
    for copy in [false, true] {
        let mut vm = page();
        vm.install_test262_harness().unwrap();
        run(&mut vm, "var compare=(a,b)=>a-b;var source={get length(){compare=null;$262.gc();return 2;},0:9,1:7};0;");
        let receiver = vm.lookup_global_name("source").unwrap().unwrap();
        let compare = vm.lookup_global_name("compare").unwrap().unwrap();
        let result = if copy {
            vm.array_to_sorted(&receiver, &compare)
        } else {
            vm.array_sort(&receiver, &compare)
        }
        .unwrap();
        assert_eq!(
            vm.get_property(&result, &"0".into()).unwrap(),
            Value::Number(7.0)
        );
        assert!(vm.stack.is_empty() && vm.realm_call_roots.is_none());
    }
}

#[cfg_attr(test, test)]
fn array_sort_storage_public_stability_holes_and_probe_order_are_preserved() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var log='';var source=new Proxy({length:3,0:undefined,2:7},{get(target,key,receiver){log+='g'+key+';';return Reflect.get(target,key,receiver);},has(target,key){log+='h'+key+';';return Reflect.has(target,key);}});
        var sorted=Array.prototype.toSorted.call(source);var copyLog=log;log='';
        Array.prototype.sort.call(source);var mutateLog=log;
        var stable=[{rank:1,tag:'a'},{rank:0,tag:'b'},{rank:1,tag:'c'}].toSorted((a,b)=>a.rank-b.rank);
        copyLog==='glength;g0;g1;g2;'&&mutateLog==='glength;h0;g0;h1;h2;g2;'&&sorted.length===3&&sorted[0]===7&&sorted[1]===undefined&&(2 in sorted)&&source[0]===7&&source[1]===undefined&&!(2 in source)&&stable.map(value=>value.tag).join('')==='bac';
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn array_sort_storage_public_comparator_result_coercion_survives_gc() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        ['sort','toSorted'].every(method=>{
            var source=[3,1,2],result=source[method]((a,b)=>({valueOf(){$262.gc();return a-b;}}));
            return result.join('')==='123'&&(method==='sort'?result===source:source.join('')==='312');
        });
    "#
        ),
        Value::Bool(true)
    );
    assert!(vm.stack.is_empty() && vm.realm_call_roots.is_none());
}

fn string_input(vm: &Vm) -> transport_buffer_storage::TransportVector<Value> {
    let mut values = vm.array_sort_input().unwrap();
    for units in [12, 8] {
        vm.push_array_sort_input(
            &mut values,
            Value::String(JsString::from_code_units(vec![0x61; units])),
        )
        .unwrap();
    }
    values
}

#[cfg_attr(test, test)]
fn array_sort_storage_iterator_moves_payload_credit_and_retains_its_backing() {
    let vm = page();
    let view = observer(&vm);
    let values = string_input(&vm);
    let slots = values.capacity() * std::mem::size_of::<Value>();
    let mut items = values.into_owned_iter().unwrap();
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, slots + 40);
    let first = items.next_owned().unwrap().unwrap();
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, slots + 40);
    drop(first);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, slots + 16);
    let second = items.next_owned().unwrap().unwrap();
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, slots + 16);
    drop(second);
    assert!(items.next_owned().unwrap().is_none());
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, slots);
    drop(items);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn array_sort_storage_terminal_iterator_does_not_consume_remaining_values() {
    let vm = page();
    let view = observer(&vm);
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let values = string_input(&vm);
    let slots = values.capacity() * std::mem::size_of::<Value>();
    let mut items = values.into_owned_iter().unwrap();
    drop(vm);
    let first = items.next_owned().unwrap().unwrap();
    let _ = scope.fail();
    assert!(matches!(
        items.next_owned(),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, slots + 40);
    drop(first);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, slots + 16);
    drop(items);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn array_sort_storage_later_values_survive_gc_after_the_receiver_is_emptied() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var seen='',target={length:3,0:{tag:3},1:{tag:1},2:{tag:2}};
        var source=new Proxy(target,{set(target,key,value){delete target[0];delete target[1];delete target[2];$262.gc();seen+=value.tag;return true;}});
        Array.prototype.sort.call(source,(a,b)=>a.tag-b.tag);seen==='123';
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn array_sort_storage_comparators_keep_snapshots_alive_after_emptying_the_receiver() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        ['sort','toSorted'].every(method=>{var source=[{tag:3},{tag:1},{tag:2}];var result=source[method]((a,b)=>{source.length=0;$262.gc();return a.tag-b.tag;});return result.map(value=>value.tag).join('')==='123'&&(method==='sort'?result===source:source.length===0);});
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn array_sort_storage_native_callback_errors_keep_their_realm() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var child=$262.createRealm(),foreignError=child.global.TypeError;
        var foreignGetter=child.evalScript("Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,'byteLength').get;");
        var forward=['sort','toSorted'].every(method=>{try{[2,1][method](foreignGetter);}catch(error){return error instanceof foreignError&&!(error instanceof TypeError);}return false;});
        child.global.parentGetter=Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,'byteLength').get;child.global.parentError=TypeError;
        forward&&child.evalScript("['sort','toSorted'].every(method=>{try{[2,1][method](parentGetter);}catch(error){return error instanceof parentError&&!(error instanceof TypeError);}return false;});");
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn array_sort_storage_tail_called_comparators_finish_results_and_throws() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        function order(a,b){'use strict';return a-b;}function compare(a,b){'use strict';return order(a,b);}
        function fail(){'use strict';throw 7;}function bad(){'use strict';return fail();}
        ['sort','toSorted'].every(method=>{var result=[3,1,2][method](compare),caught=false;try{[2,1][method](bad);}catch(error){caught=error===7;}return result.join('')==='123'&&caught;});
    "#
        ),
        Value::Bool(true)
    );
    assert!(vm.pending_tail_call.is_none() && vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn array_sort_storage_terminal_comparator_skips_result_coercion() {
    for copy in [false, true] {
        let source = page();
        let owner = source.argument_list_owner().unwrap();
        let scope = source.host_scheduling.as_ref().unwrap().child();
        let seen = Rc::new(std::cell::Cell::new(false));
        let output = seen.clone();
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
                Ok(crate::HostValue::Number(0.0))
            })
            .unwrap();
        let receiver = run(
            &mut neutral,
            "var compare=()=>{markTerminal();return {valueOf(){return observe();}};};[2,1];",
        );
        let compare = neutral.lookup_global_name("compare").unwrap().unwrap();
        let roots = realm_call_storage::RealmCallRoots::new(2, owner.as_ref()).unwrap();
        roots
            .push(&neutral.heap, receiver.object_id().unwrap())
            .unwrap();
        roots
            .push(&neutral.heap, compare.object_id().unwrap())
            .unwrap();
        let result = neutral.with_realm_call_roots(roots, |vm| {
            if copy {
                vm.array_to_sorted(&receiver, &compare)
            } else {
                vm.array_sort(&receiver, &compare)
            }
        });
        assert_eq!(result, Err(RuntimeError::SchedulingLimit));
        assert!(!seen.get());
        assert!(neutral.stack.is_empty() && neutral.realm_call_roots.is_none());
    }
}

#[cfg_attr(test, test)]
fn array_sort_storage_terminal_setter_stops_before_the_next_write() {
    let source = page();
    let owner = source.argument_list_owner().unwrap();
    let scope = source.host_scheduling.as_ref().unwrap().child();
    let seen = Rc::new(std::cell::Cell::new(0));
    let output = seen.clone();
    let mut neutral = Vm::default();
    neutral
        .install_host_function("markTerminal", 0, move |_: &[crate::HostValue]| {
            output.set(output.get() + 1);
            let _ = scope.fail();
            Ok(crate::HostValue::Bool(true))
        })
        .unwrap();
    let receiver = run(
        &mut neutral,
        "new Proxy({length:2,0:2,1:1},{set(){return markTerminal();}});",
    );
    let roots = realm_call_storage::RealmCallRoots::new(1, owner.as_ref()).unwrap();
    roots
        .push(&neutral.heap, receiver.object_id().unwrap())
        .unwrap();
    let result =
        neutral.with_realm_call_roots(roots, |vm| vm.array_sort(&receiver, &Value::Undefined));
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(seen.get(), 1);
    assert!(neutral.stack.is_empty() && neutral.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn array_sort_storage_neutral_loans_restore_the_outer_stack_and_root_chain() {
    for copy in [false, true] {
        let source = page();
        let owner = source.argument_list_owner().unwrap();
        let mut neutral = Vm::default();
        let receiver = run(&mut neutral, "[3,1,2];");
        neutral.stack.push(Value::Number(9.0));
        let outer = realm_call_storage::RealmCallRoots::new(1, owner.as_ref()).unwrap();
        outer
            .push(&neutral.heap, receiver.object_id().unwrap())
            .unwrap();
        let result = neutral
            .with_realm_call_roots(outer.clone(), |vm| {
                let value = if copy {
                    vm.array_to_sorted(&receiver, &Value::Undefined)?
                } else {
                    vm.array_sort(&receiver, &Value::Undefined)?
                };
                assert!(Rc::ptr_eq(vm.realm_call_roots.as_ref().unwrap(), &outer));
                Ok(value)
            })
            .unwrap();
        assert_eq!(
            neutral.get_property(&result, &"0".into()).unwrap(),
            Value::Number(1.0)
        );
        assert_eq!(neutral.stack, [Value::Number(9.0)]);
        assert!(neutral.host_scheduling.is_none() && neutral.realm_call_roots.is_none());
    }
}

#[cfg_attr(test, test)]
fn array_sort_storage_comparator_sequence_and_numeric_result_semantics_are_preserved() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        ['sort','toSorted'].every(method=>{var log='';var sorted=[3,1,2][method]((a,b)=>{log+=a+''+b+',';return a-b;});var nan=[2,1][method](()=>NaN),zero=[2,1][method](()=>-0);return log==='13,21,23,'&&sorted.join('')==='123'&&nan.join('')==='21'&&zero.join('')==='21';});
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn array_sort_storage_retargeted_snapshot_checks_the_original_document_after_callback() {
    let source = page();
    let source_view = observer(&source);
    let scope = source.host_scheduling.as_ref().unwrap().child();
    let mut values = source.array_sort_input().unwrap();
    for value in [3.0, 1.0, 2.0] {
        source
            .push_array_sort_input(&mut values, Value::Number(value))
            .unwrap();
    }
    let mut destination = page();
    destination
        .install_host_function("markTerminal", 0, move |_: &[crate::HostValue]| {
            let _ = scope.fail();
            Ok(crate::HostValue::Undefined)
        })
        .unwrap();
    let compare = run(
        &mut destination,
        "var calls=0;(a,b)=>{calls++;markTerminal();return a-b;};",
    );
    assert!(matches!(
        destination.array_sort_values(values, &compare),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert!(destination
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(0)
        .is_ok());
    assert_eq!(run(&mut destination, "calls;"), Value::Number(1.0));
    assert!(destination.realm_call_roots.is_none());
    drop(source);
    assert_eq!(source_view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn array_sort_storage_manual_timer_and_promise_keep_sort_values_through_gc() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    run(&mut vm,"delete globalThis.setTimeout;delete globalThis.clearTimeout;delete globalThis.setInterval;delete globalThis.clearInterval;delete globalThis.queueMicrotask;0;");
    vm.install_host_timer_functions().unwrap();
    run(
        &mut vm,
        r#"
        var result='';setTimeout(()=>{
            var source=[{tag:3},{tag:1},{tag:2}];
            var ordered=source.toSorted((a,b)=>{$262.gc();return a.tag-b.tag;});source=null;
            Promise.resolve().then(()=>{ordered.sort((a,b)=>{$262.gc();return b.tag-a.tag;});result=ordered.map(value=>value.tag).join('');});
        },1);0;
    "#,
    );
    let turn = vm.run_due_host_timers(1).unwrap();
    assert_eq!(turn.tasks_run, 1);
    assert_eq!(turn.uncaught_errors, 0);
    assert_eq!(vm.next_host_timer_due(), None);
    assert_eq!(run(&mut vm, "result;"), Value::String("321".into()));
    assert!(vm.realm_call_roots.is_none() && vm.stack.is_empty());
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_array_sort_storage_contracts() {
        array_sort_storage_input_keeps_its_backing_and_payload_after_vm_destruction();
        array_sort_storage_completed_write_retires_its_value_before_the_next_setter();
        array_sort_storage_terminal_length_rejects_both_empty_results();
        array_sort_storage_terminal_has_stops_before_the_index_getter();
        array_sort_storage_terminal_first_string_skips_second_conversion();
        array_sort_storage_inputs_pin_the_comparator_before_length_getter_gc();
        array_sort_storage_public_stability_holes_and_probe_order_are_preserved();
        array_sort_storage_public_comparator_result_coercion_survives_gc();
        array_sort_storage_iterator_moves_payload_credit_and_retains_its_backing();
        array_sort_storage_terminal_iterator_does_not_consume_remaining_values();
        array_sort_storage_later_values_survive_gc_after_the_receiver_is_emptied();
        array_sort_storage_comparators_keep_snapshots_alive_after_emptying_the_receiver();
        array_sort_storage_native_callback_errors_keep_their_realm();
        array_sort_storage_tail_called_comparators_finish_results_and_throws();
        array_sort_storage_terminal_comparator_skips_result_coercion();
        array_sort_storage_terminal_setter_stops_before_the_next_write();
        array_sort_storage_neutral_loans_restore_the_outer_stack_and_root_chain();
        array_sort_storage_comparator_sequence_and_numeric_result_semantics_are_preserved();
        array_sort_storage_retargeted_snapshot_checks_the_original_document_after_callback();
        array_sort_storage_manual_timer_and_promise_keep_sort_values_through_gc();
    }
}
