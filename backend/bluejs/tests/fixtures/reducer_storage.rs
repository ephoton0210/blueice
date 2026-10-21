// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny reducer storage contracts and public order/GC controls.

use super::super::realm_call_storage::RealmCallRoots;
use super::*;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}

fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}

fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: vm.host_scheduling.as_ref().map(SchedulingScope::child),
        ..Vm::default()
    }
}

fn arguments(
    vm: &Vm,
    accumulator: Value,
    value: Value,
    index: f64,
    object: ObjectId,
) -> TransportVector<Value> {
    let owner = vm.argument_list_owner().unwrap();
    vm.reducer_arguments(
        NativeValue::adopt(accumulator, owner.as_ref()).unwrap(),
        NativeValue::adopt(value, owner.as_ref()).unwrap(),
        index,
        object,
    )
    .unwrap()
}

#[cfg_attr(test, test)]
fn reducer_storage_arguments_outlive_the_vm() {
    let mut vm = page();
    let view = observer(&vm);
    let object = run(&mut vm, "new BigInt64Array([1n]);")
        .object_id()
        .unwrap();
    let value = vm.heap.typed_array_index_value(object, 0).unwrap().unwrap();
    let accumulator = Value::String(JsString::from_code_units(vec![0x61; 32]));
    let arguments = arguments(&vm, accumulator, value, 0.0, object);
    let retained = arguments.retained_slots() + host_scheduling::values_bytes(&arguments);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(arguments);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn reducer_storage_array_holes_getters_and_directions_preserve_order() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var log='',source={length:4,1:2,3:4};
        Object.defineProperty(source,'2',{get(){log+='g';$262.gc();return 3;}});
        var left=Array.prototype.reduce.call(source,(acc,value,index)=>{log+=index;return acc+value;});
        var right=Array.prototype.reduceRight.call(source,(acc,value,index)=>{log+=index;return acc+value;});
        left===9&&right===9&&log==='g23g21';
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn reducer_storage_typed_reductions_read_live_elements_in_both_directions() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var left=new BigInt64Array([1n,2n,3n]),right=new BigInt64Array([1n,2n,3n]);
        var first=left.reduce((acc,value,index,array)=>{if(index===1)array[2]=7n;return acc+value;});
        var last=right.reduceRight((acc,value,index,array)=>{if(index===1)array[0]=7n;return acc+value;});
        first===10n&&last===12n;
    "#
        ),
        Value::Bool(true)
    );
}

fn buffered_string() -> Value {
    Value::String(JsString::from_code_units(vec![0x61; 32]))
}

fn retained(arguments: &TransportVector<Value>) -> usize {
    arguments.retained_slots() + host_scheduling::values_bytes(arguments)
}

#[cfg_attr(test, test)]
fn reducer_storage_argument_moves_conserve_credit() {
    let mut vm = page();
    let object = run(&mut vm, "[1];").object_id().unwrap();
    vm.synchronize_scheduling_usage().unwrap();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let arguments = arguments(&vm, buffered_string(), buffered_string(), 1.0, object);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        before + retained(&arguments)
    );
    drop(arguments);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn reducer_storage_neutral_arguments_adopt_the_input_owner() {
    let mut source = page();
    let view = observer(&source);
    let object = run(&mut source, "[1];").object_id().unwrap();
    let owner = source.argument_list_owner().unwrap();
    let neutral = Vm::default();
    let arguments = neutral
        .reducer_arguments(
            NativeValue::adopt(buffered_string(), owner.as_ref()).unwrap(),
            NativeValue::adopt(Value::Number(1.0), None).unwrap(),
            0.0,
            object,
        )
        .unwrap();
    assert!(neutral.host_scheduling.is_none());
    let bytes = retained(&arguments);
    drop(owner);
    drop(source);
    drop(neutral);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(arguments);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn reducer_storage_neutral_root_loan_does_not_rebind_policy() {
    let source = page();
    let view = observer(&source);
    let owner = source.argument_list_owner().unwrap();
    let mut neutral = Vm::default();
    let object = run(&mut neutral, "[1];").object_id().unwrap();
    let roots = RealmCallRoots::new(1, owner.as_ref()).unwrap();
    roots.push(&neutral.heap, object).unwrap();
    let arguments = neutral
        .with_realm_call_roots(roots, |vm| {
            Ok(arguments(
                vm,
                buffered_string(),
                Value::Number(1.0),
                0.0,
                object,
            ))
        })
        .unwrap();
    assert!(neutral.host_scheduling.is_none());
    assert!(neutral.realm_call_roots.is_none());
    let bytes = retained(&arguments);
    drop(owner);
    drop(source);
    drop(neutral);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(arguments);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn reducer_storage_refused_transfer_releases_both_document_payloads() {
    let mut first = page();
    let second = page();
    let first_view = observer(&first);
    let second_view = observer(&second);
    let object = run(&mut first, "[1];").object_id().unwrap();
    let left_owner = first.argument_list_owner().unwrap();
    let right_owner = second.argument_list_owner().unwrap();
    let result = first.reducer_arguments(
        NativeValue::adopt(buffered_string(), left_owner.as_ref()).unwrap(),
        NativeValue::adopt(buffered_string(), right_owner.as_ref()).unwrap(),
        0.0,
        object,
    );
    assert!(matches!(result, Err(RuntimeError::SchedulingLimit)));
    assert!(right_owner.as_ref().unwrap().reserve_peer(0).is_ok());
    drop(result);
    drop(left_owner);
    drop(right_owner);
    drop(first);
    drop(second);
    assert_eq!(first_view.host_scheduling_snapshot().payload_bytes, 0);
    assert_eq!(second_view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn reducer_storage_array_accumulators_survive_getter_gc() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        function check(method,index){
            var source=[1,2,3];Object.defineProperty(source,index,{get(){$262.gc();return index+1;}});
            var result=source[method]((acc,value)=>({sum:acc.sum+value}),{sum:0});return result.sum===6;
        }
        check('reduce',2)&&check('reduceRight',0);
    "#
        ),
        Value::Bool(true)
    );
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn reducer_storage_typed_accumulators_survive_callback_gc() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        ['reduce','reduceRight'].every(method=>new Uint8Array([1,2,3])[method]((acc,value)=>{
            $262.gc();return {sum:acc.sum+value};
        },{sum:0}).sum===6);
    "#
        ),
        Value::Bool(true)
    );
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn reducer_storage_proxy_has_get_and_callback_order_is_preserved() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        function check(method,expected){
            var log='',source=new Proxy({length:3,0:1,2:3},{
                has(target,key){log+='h'+key;return key in target;},
                get(target,key){log+='g'+key;return target[key];}
            });
            var result=Array.prototype[method].call(source,function(acc,value,index,array){
                'use strict';if(this!==undefined||array!==source)throw 7;log+='c'+index;return acc+value;
            },0);
            return result===4&&log===expected;
        }
        check('reduce','glengthh0g0c0h1h2g2c2')&&check('reduceRight','glengthh2g2c2h1h0g0c0');
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn reducer_storage_array_walks_observe_added_and_deleted_indices() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var first=[1,,3],last=[1,,3],seen='';
        var left=first.reduce((acc,value,index,array)=>{seen+=index;if(index===0){array[1]=2;delete array[2];}return acc+value;},0);
        var right=last.reduceRight((acc,value,index,array)=>{seen+=index;if(index===2){array[1]=2;delete array[0];}return acc+value;},0);
        left===3&&right===5&&seen==='0121';
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn reducer_storage_empty_initial_values_include_explicit_undefined() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var initial={tag:1};
        [[],new Uint8Array()].every(source=>['reduce','reduceRight'].every(method=>
            source[method](()=>{throw 7;},undefined)===undefined&&source[method](()=>{throw 7;},initial)===initial));
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn reducer_storage_single_elements_skip_the_callback() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var initial={tag:1};['reduce','reduceRight'].every(method=>
            [initial][method](()=>{throw 7;})===initial&&new BigInt64Array([7n])[method](()=>{throw 7;})===7n);
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn reducer_storage_typed_resize_preserves_missing_values_and_captured_length() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        function check(method,expected,order){
            var buffer=new ArrayBuffer(3,{maxByteLength:3}),source=new Uint8Array(buffer),log='';source.set([1,2,3]);
            var result=source[method]((acc,value,index,array)=>{if(array!==source)throw 7;log+=index;buffer.resize(0);return acc+(value===undefined?10:value);},0);
            return result===expected&&log===order;
        }
        check('reduce',21,'012')&&check('reduceRight',23,'210');
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn reducer_storage_callback_throw_restores_stack_and_root_chain() {
    for source in ["[1,2]", "new Uint8Array([1,2])"] {
        for method in ["reduce", "reduceRight"] {
            let mut vm = page();
            let source = run(&mut vm, &format!("var source={source};source;"));
            let callback = run(&mut vm, "(()=>{throw 7;});");
            let callee = vm.get_property(&source, &method.into()).unwrap();
            let owner = vm.argument_list_owner().unwrap();
            let roots = RealmCallRoots::new(1, owner.as_ref()).unwrap();
            roots.push(&vm.heap, source.object_id().unwrap()).unwrap();
            vm.stack.push(Value::Number(9.0));
            vm.synchronize_scheduling_usage().unwrap();
            vm.with_realm_call_roots(roots.clone(), |vm| {
                assert_eq!(
                    vm.call_native(callee, source, vec![callback, Value::Number(0.0)], false),
                    Err(RuntimeError::Thrown(Value::Number(7.0)))
                );
                assert!(Rc::ptr_eq(vm.realm_call_roots.as_ref().unwrap(), &roots));
                assert_eq!(vm.stack, vec![Value::Number(9.0)]);
                Ok(())
            })
            .unwrap();
            assert!(vm.realm_call_roots.is_none());
        }
    }
}

#[cfg_attr(test, test)]
fn reducer_storage_getter_throw_releases_the_accumulator() {
    for (method, index) in [("reduce", 1), ("reduceRight", 0)] {
        let mut vm = page();
        let view = observer(&vm);
        let source = format!("var marker={{}},source=[1,2],caught=false;Object.defineProperty(source,{index},{{get(){{throw marker;}}}});try{{source.{method}((acc,value)=>({{sum:acc.sum+value}}),{{sum:0}});}}catch(error){{caught=error===marker;}}caught;");
        assert_eq!(run(&mut vm, &source), Value::Bool(true));
        assert!(vm.realm_call_roots.is_none());
        drop(vm);
        assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
    }
}

#[cfg_attr(test, test)]
fn reducer_storage_terminal_callback_releases_all_storage() {
    for source in ["[1,2]", "new Uint8Array([1,2])"] {
        for method in ["reduce", "reduceRight"] {
            let mut vm = page();
            let view = observer(&vm);
            let scope = vm.host_scheduling.as_ref().unwrap().child();
            vm.install_host_function("markTerminal", 0, move |_: &[crate::HostValue]| {
                let _ = scope.fail();
                Ok(crate::HostValue::Undefined)
            })
            .unwrap();
            let source = format!("({source}).{method}((acc,value)=>{{markTerminal();return acc;}},'abcdefghijklmnopqrstuvwxyz0123456789');");
            let script = crate::compile(&crate::parse(&source).unwrap()).unwrap();
            assert_eq!(
                vm.execute_script(&script),
                Err(RuntimeError::SchedulingLimit)
            );
            assert!(vm.realm_call_roots.is_none());
            drop(vm);
            assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
        }
    }
}

#[cfg_attr(test, test)]
fn reducer_storage_terminal_getter_restores_roots() {
    let mut vm = page();
    let view = observer(&vm);
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    vm.install_host_function("markTerminal", 0, move |_: &[crate::HostValue]| {
        let _ = scope.fail();
        Ok(crate::HostValue::Number(2.0))
    })
    .unwrap();
    let script = crate::compile(&crate::parse("var source=[1,2];Object.defineProperty(source,'1',{get(){return markTerminal();}});source.reduce((acc,value)=>({sum:acc.sum+value}),{sum:0});").unwrap()).unwrap();
    assert_eq!(
        vm.execute_script(&script),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(vm.realm_call_roots.is_none());
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn reducer_storage_tail_calls_return_the_final_result() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        function add(acc,value){'use strict';return acc+value;}
        function reducer(acc,value){'use strict';return add(acc,value);}
        [[1,2,3],new Uint8Array([1,2,3])].every(source=>['reduce','reduceRight'].every(method=>source[method](reducer,0)===6));
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn reducer_storage_tail_call_throws_preserve_identity() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var marker={};function fail(){'use strict';throw marker;}
        function reducer(){'use strict';return fail();}
        [[1,2],new Uint8Array([1,2])].every(source=>['reduce','reduceRight'].every(method=>{
            try{source[method](reducer,0);}catch(error){return error===marker;}return false;
        }));
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn reducer_storage_foreign_callbacks_keep_their_gc_realm() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var child=$262.createRealm(),callback=child.evalScript('(function(acc,value){$262.gc();return acc+value;})');
        [[1,2,3],new Uint8Array([1,2,3])].every(source=>['reduce','reduceRight'].every(method=>source[method](callback,0)===6));
    "#
        ),
        Value::Bool(true)
    );
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn reducer_storage_owned_consumer_roots_restore_after_throw() {
    let mut vm = page();
    let anchor = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    let object = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    let owner = vm.argument_list_owner().unwrap();
    let outer = RealmCallRoots::new(1, owner.as_ref()).unwrap();
    outer.push(&vm.heap, anchor).unwrap();
    let item = NativeValue::adopt(Value::Object(object), owner.as_ref()).unwrap();
    vm.with_realm_call_roots(outer.clone(), |vm| {
        let result: Result<(), RuntimeError> = item.with_owned(vm, |vm, item| {
            vm.with_roots(|heap| heap.try_collect_major())?;
            assert!(vm.heap.contains(item.value().object_id().unwrap()));
            Err(RuntimeError::Thrown(Value::Number(7.0)))
        });
        assert_eq!(result, Err(RuntimeError::Thrown(Value::Number(7.0))));
        assert!(Rc::ptr_eq(vm.realm_call_roots.as_ref().unwrap(), &outer));
        Ok(())
    })
    .unwrap();
    assert!(vm.realm_call_roots.is_none());
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.heap.contains(object));
}

#[cfg_attr(test, test)]
fn reducer_storage_zero_payload_arguments_keep_terminal_identity() {
    let mut vm = page();
    let view = observer(&vm);
    let object = run(&mut vm, "[1];").object_id().unwrap();
    let arguments = arguments(&vm, Value::Undefined, Value::Number(1.0), 0.0, object);
    let bytes = retained(&arguments);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    let _ = view.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        arguments.validate_owner(),
        Err(RuntimeError::SchedulingLimit)
    );
    drop(arguments);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn reducer_storage_callback_result_owns_payload_after_vm_destruction() {
    let mut vm = page();
    let view = observer(&vm);
    let object = run(&mut vm, "var callback=(acc)=>acc;[1];")
        .object_id()
        .unwrap();
    let callback = vm.lookup_global_name("callback").unwrap().unwrap();
    let owner = vm.argument_list_owner().unwrap();
    let result = vm
        .call_reducer(
            &callback,
            NativeValue::adopt(buffered_string(), owner.as_ref()).unwrap(),
            NativeValue::adopt(Value::Number(1.0), owner.as_ref()).unwrap(),
            0.0,
            object,
        )
        .unwrap();
    let bytes = host_scheduling::value_payload_bytes(result.value());
    drop(owner);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(result);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn reducer_storage_length_errors_precede_bad_callback_validation() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var marker={},log='';['reduce','reduceRight'].every(method=>{
            var source={get length(){log+='l';$262.gc();throw marker;}};
            try{Array.prototype[method].call(source,null);}catch(error){return error===marker;}return false;
        })&&log==='ll';
    "#
        ),
        Value::Bool(true)
    );
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_reducer_storage_contracts() {
        reducer_storage_arguments_outlive_the_vm();
        reducer_storage_array_holes_getters_and_directions_preserve_order();
        reducer_storage_typed_reductions_read_live_elements_in_both_directions();
        reducer_storage_argument_moves_conserve_credit();
        reducer_storage_neutral_arguments_adopt_the_input_owner();
        reducer_storage_neutral_root_loan_does_not_rebind_policy();
        reducer_storage_refused_transfer_releases_both_document_payloads();
        reducer_storage_array_accumulators_survive_getter_gc();
        reducer_storage_typed_accumulators_survive_callback_gc();
        reducer_storage_proxy_has_get_and_callback_order_is_preserved();
        reducer_storage_array_walks_observe_added_and_deleted_indices();
        reducer_storage_empty_initial_values_include_explicit_undefined();
        reducer_storage_single_elements_skip_the_callback();
        reducer_storage_typed_resize_preserves_missing_values_and_captured_length();
        reducer_storage_callback_throw_restores_stack_and_root_chain();
        reducer_storage_getter_throw_releases_the_accumulator();
        reducer_storage_terminal_callback_releases_all_storage();
        reducer_storage_terminal_getter_restores_roots();
        reducer_storage_tail_calls_return_the_final_result();
        reducer_storage_tail_call_throws_preserve_identity();
        reducer_storage_foreign_callbacks_keep_their_gc_realm();
        reducer_storage_owned_consumer_roots_restore_after_throw();
        reducer_storage_zero_payload_arguments_keep_terminal_identity();
        reducer_storage_callback_result_owns_payload_after_vm_destruction();
        reducer_storage_length_errors_precede_bad_callback_validation();
    }
}
