// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed tiny native activity ownership, callback and cleanup contracts.

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
fn active_native_storage_join_backing_outlives_the_vm() {
    let mut vm = page();
    let view = observer(&vm);
    let object = vm.heap.alloc_object(None).unwrap();
    vm.begin_array_join(object).unwrap();
    let backing = std::mem::take(&mut vm.joining);
    let bytes = backing.capacity() * std::mem::size_of::<ObjectId>();
    assert!(bytes > 0);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(backing);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn active_native_storage_delegation_backing_outlives_the_vm() {
    let mut vm = page();
    let view = observer(&vm);
    let object = vm.heap.alloc_object(None).unwrap();
    vm.begin_generator_delegation(object).unwrap();
    let backing = std::mem::take(&mut vm.active_generator_delegations);
    let bytes = backing.capacity() * std::mem::size_of::<ObjectId>();
    assert!(bytes > 0);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(backing);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn active_native_storage_join_restores_the_outer_operand_stack() {
    let mut vm = page();
    let receiver = run(&mut vm, "[1,2];");
    vm.stack.push(Value::Number(9.0));
    assert_eq!(
        vm.array_join(&receiver, &Value::Undefined).unwrap(),
        Value::String("1,2".into())
    );
    assert_eq!(vm.stack, [Value::Number(9.0)]);
    assert!(vm.joining.is_empty());
}

#[cfg_attr(test, test)]
fn active_native_storage_join_throw_restores_the_outer_operand_stack() {
    let mut vm = page();
    let receiver = run(&mut vm, "({get length(){throw 7;}});");
    vm.stack.push(Value::Number(9.0));
    assert_eq!(
        vm.array_join(&receiver, &Value::Undefined),
        Err(RuntimeError::Thrown(Value::Number(7.0)))
    );
    assert_eq!(vm.stack, [Value::Number(9.0)]);
    assert!(vm.joining.is_empty());
}

#[cfg_attr(test, test)]
fn active_native_storage_completed_join_releases_its_receiver() {
    let mut vm = page();
    run(&mut vm, "0;");
    let source = vm.with_roots(|heap| heap.alloc_array(1, None)).unwrap();
    vm.heap.set(source, "0", Value::Number(7.0)).unwrap();
    assert_eq!(
        vm.array_join(&Value::Object(source), &Value::Undefined)
            .unwrap(),
        Value::String("7".into())
    );
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.heap.contains(source));
    assert!(vm.stack.is_empty());
}

#[cfg_attr(test, test)]
fn active_native_storage_terminal_length_skips_separator_conversion() {
    let source = page();
    let owner = source.argument_list_owner().unwrap();
    let scope = source.host_scheduling.as_ref().unwrap().child();
    let seen = Rc::new(std::cell::Cell::new(false));
    let output = seen.clone();
    let mut neutral = Vm::default();
    neutral
        .install_host_function("markTerminal", 0, move |_: &[crate::HostValue]| {
            let _ = scope.fail();
            Ok(crate::HostValue::Number(0.0))
        })
        .unwrap();
    neutral
        .install_host_function("observe", 0, move |_: &[crate::HostValue]| {
            output.set(true);
            Ok(crate::HostValue::String("x".into()))
        })
        .unwrap();
    let receiver = run(&mut neutral, "var separator={toString(){return observe();}};({length:{valueOf(){return markTerminal();}}});");
    let separator = neutral.lookup_global_name("separator").unwrap().unwrap();
    let roots = realm_call_storage::RealmCallRoots::new(2, owner.as_ref()).unwrap();
    roots
        .push(&neutral.heap, receiver.object_id().unwrap())
        .unwrap();
    roots
        .push(&neutral.heap, separator.object_id().unwrap())
        .unwrap();
    let result = neutral.with_realm_call_roots(roots, |vm| vm.array_join(&receiver, &separator));
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(!seen.get());
    assert!(neutral.stack.is_empty() && neutral.realm_call_roots.is_none());
    assert!(neutral.host_scheduling.is_none());
}

#[cfg_attr(test, test)]
fn active_native_storage_join_preserves_holes_conversion_order_and_gc() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var log='',source={get length(){log+='l';return 3;},get 1(){log+='g';return {toString(){$262.gc();log+='v';return '7';}};}};
        var separator={toString(){log+='s';$262.gc();return '-';}};
        Array.prototype.join.call(source,separator)==='-7-'&&log==='lsgv';
    "#
        ),
        Value::Bool(true)
    );
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn active_native_storage_delegates_preserve_finite_return_throw_and_gc() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        ['return','throw'].every(method=>{
            var delegate={[Symbol.iterator](){return this;},next(){return {done:false,value:1};}};
            delegate[method]=function(value){$262.gc();return {done:true,value:value+1};};
            var generator=(function*(){return yield* delegate;})();
            var first=generator.next(),last=generator[method](7);
            return first.value===1&&!first.done&&last.value===8&&last.done&&generator.next().done;
        });
    "#
        ),
        Value::Bool(true)
    );
    assert!(vm.active_generator_delegations.is_empty());
}

#[cfg_attr(test, test)]
fn active_native_storage_nested_join_entries_keep_one_backing_until_the_last_exit() {
    let mut vm = page();
    let view = observer(&vm);
    let first = vm.heap.alloc_object(None).unwrap();
    let second = vm.heap.alloc_object(None).unwrap();
    let baseline = view.host_scheduling_snapshot().payload_bytes;
    vm.begin_array_join(first).unwrap();
    let bytes = vm.joining.capacity() * std::mem::size_of::<ObjectId>();
    vm.begin_array_join(second).unwrap();
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        baseline + bytes
    );
    vm.end_array_join(second);
    assert_eq!(&vm.joining[..], &[first]);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        baseline + bytes
    );
    vm.end_array_join(first);
    assert_eq!(vm.joining.capacity(), 0);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, baseline);
}

#[cfg_attr(test, test)]
fn active_native_storage_nested_delegations_keep_one_backing_until_the_last_exit() {
    let mut vm = page();
    let view = observer(&vm);
    let first = vm.heap.alloc_object(None).unwrap();
    let second = vm.heap.alloc_object(None).unwrap();
    let baseline = view.host_scheduling_snapshot().payload_bytes;
    vm.begin_generator_delegation(first).unwrap();
    let bytes = vm.active_generator_delegations.bytes();
    vm.begin_generator_delegation(second).unwrap();
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        baseline + bytes
    );
    vm.end_generator_delegation(second);
    assert!(vm.active_generator_delegations.contains(&first));
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        baseline + bytes
    );
    vm.end_generator_delegation(first);
    assert_eq!(vm.active_generator_delegations.capacity(), 0);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, baseline);
}

fn loaned_entries(neutral: &mut Vm, source: &Vm, object: ObjectId) {
    let owner = source.argument_list_owner().unwrap();
    let roots = realm_call_storage::RealmCallRoots::new(1, owner.as_ref()).unwrap();
    roots.push(&neutral.heap, object).unwrap();
    neutral
        .with_realm_call_roots(roots, |vm| {
            vm.begin_array_join(object)?;
            vm.begin_generator_delegation(object)
        })
        .unwrap();
    assert!(neutral.host_scheduling.is_none() && neutral.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn active_native_storage_neutral_backing_keeps_its_original_loan_after_both_vms_drop() {
    let source = page();
    let view = observer(&source);
    let mut neutral = Vm::default();
    let object = neutral.heap.alloc_object(None).unwrap();
    loaned_entries(&mut neutral, &source, object);
    let joining = std::mem::take(&mut neutral.joining);
    let delegating = std::mem::take(&mut neutral.active_generator_delegations);
    let join_bytes = joining.capacity() * std::mem::size_of::<ObjectId>();
    let delegate_bytes = delegating.bytes();
    drop(neutral);
    drop(source);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        join_bytes + delegate_bytes
    );
    drop(joining);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        delegate_bytes
    );
    drop(delegating);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn active_native_storage_terminal_original_loan_rejects_another_marker() {
    let source = page();
    let mut neutral = Vm::default();
    let first = neutral.heap.alloc_object(None).unwrap();
    let second = neutral.heap.alloc_object(None).unwrap();
    loaned_entries(&mut neutral, &source, first);
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        neutral.begin_array_join(second),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(
        neutral.begin_generator_delegation(second),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(&neutral.joining[..], &[first]);
    assert_eq!(neutral.active_generator_delegations.len(), 1);
    neutral.end_array_join(first);
    neutral.end_generator_delegation(first);
    assert_eq!(neutral.joining.capacity(), 0);
    assert_eq!(neutral.active_generator_delegations.capacity(), 0);
}

#[cfg_attr(test, test)]
fn active_native_storage_neutral_policy_adopts_both_existing_allocations_once() {
    let mut vm = Vm::default();
    let object = vm.heap.alloc_object(None).unwrap();
    vm.begin_array_join(object).unwrap();
    vm.begin_generator_delegation(object).unwrap();
    let second = vm.heap.alloc_object(None).unwrap();
    vm.begin_generator_delegation(second).unwrap();
    let bytes = vm.joining.capacity() * std::mem::size_of::<ObjectId>()
        + vm.active_generator_delegations.bytes();
    vm.end_generator_delegation(second);
    assert_eq!(vm.unowned_active_native_storage_bytes(), bytes);
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    assert_eq!(vm.unowned_active_native_storage_bytes(), 0);
    let before = vm.host_scheduling_snapshot();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    assert_eq!(vm.host_scheduling_snapshot(), before);
    let view = observer(&vm);
    let joining = std::mem::take(&mut vm.joining);
    let delegating = std::mem::take(&mut vm.active_generator_delegations);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop((joining, delegating));
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn active_native_storage_foreign_document_cannot_reassign_existing_loans() {
    let source = page();
    let other = page();
    let mut neutral = Vm::default();
    let object = neutral.heap.alloc_object(None).unwrap();
    loaned_entries(&mut neutral, &source, object);
    let before = source.host_scheduling_snapshot();
    let scope = other.host_scheduling.as_ref().unwrap();
    assert!(!neutral.active_native_storage_can_join(scope));
    assert_eq!(
        neutral.attach_active_native_storage(scope),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(source.host_scheduling_snapshot(), before);
    assert!(neutral.active_native_storage_can_join(source.host_scheduling.as_ref().unwrap()));
    neutral.end_array_join(object);
    neutral.end_generator_delegation(object);
}

#[cfg_attr(test, test)]
fn active_native_storage_join_roots_retire_each_completed_receiver() {
    let mut vm = page();
    let first = vm.heap.alloc_object(None).unwrap();
    let second = vm.heap.alloc_object(None).unwrap();
    vm.begin_array_join(first).unwrap();
    vm.begin_array_join(second).unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(vm.heap.contains(first) && vm.heap.contains(second));
    vm.end_array_join(second);
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(vm.heap.contains(first) && !vm.heap.contains(second));
    vm.end_array_join(first);
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.heap.contains(first));
}

#[cfg_attr(test, test)]
fn active_native_storage_terminal_separator_rejects_an_empty_join() {
    let source = page();
    let owner = source.argument_list_owner().unwrap();
    let scope = source.host_scheduling.as_ref().unwrap().child();
    let mut neutral = Vm::default();
    neutral
        .install_host_function("markTerminal", 0, move |_: &[crate::HostValue]| {
            let _ = scope.fail();
            Ok(crate::HostValue::String("x".into()))
        })
        .unwrap();
    let receiver = run(
        &mut neutral,
        "var separator={toString(){return markTerminal();}};({length:0});",
    );
    let separator = neutral.lookup_global_name("separator").unwrap().unwrap();
    let roots = realm_call_storage::RealmCallRoots::new(2, owner.as_ref()).unwrap();
    roots
        .push(&neutral.heap, receiver.object_id().unwrap())
        .unwrap();
    roots
        .push(&neutral.heap, separator.object_id().unwrap())
        .unwrap();
    assert_eq!(
        neutral.with_realm_call_roots(roots, |vm| vm.array_join(&receiver, &separator)),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(neutral.joining.is_empty() && neutral.stack.is_empty());
}

#[cfg_attr(test, test)]
fn active_native_storage_join_pins_the_separator_before_the_length_getter() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    run(&mut vm, "var log='',separator={toString(){log+='s';return '|';}},source={get length(){source=null;separator=null;$262.gc();return 1;},0:7};0;");
    let receiver = vm.lookup_global_name("source").unwrap().unwrap();
    let separator = vm.lookup_global_name("separator").unwrap().unwrap();
    assert_eq!(
        vm.array_join(&receiver, &separator).unwrap(),
        Value::String("7".into())
    );
    assert_eq!(run(&mut vm, "log;"), Value::String("s".into()));
    assert!(vm.stack.is_empty() && vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn active_native_storage_nested_joins_preserve_utf16_holes_and_gc() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var inner=[{toString(){$262.gc();return String.fromCharCode(0xd800);}},,null];
        var outer=[{toString(){return inner.join(String.fromCharCode(0xdc00));}},9];
        var result=outer.join('|');
        result.length===5&&result.charCodeAt(0)===0xd800&&result.charCodeAt(1)===0xdc00&&result.charCodeAt(2)===0xdc00&&result.slice(3)==='|9';
    "#
        ),
        Value::Bool(true)
    );
    assert_eq!(vm.joining.capacity(), 0);
    assert!(vm.stack.is_empty());
}

#[cfg_attr(test, test)]
fn active_native_storage_abrupt_delegate_getters_release_the_running_marker() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        ['return','throw'].every(method=>['method','done','value'].every(stage=>{
            var delegate={[Symbol.iterator](){return this;},next(){return {done:false,value:1};}};
            if(stage==='method')Object.defineProperty(delegate,method,{get(){throw 7;}});
            else delegate[method]=function(){return stage==='done'?{get done(){throw 7;}}:{done:true,get value(){throw 7;}};};
            var generator=(function*(){try{yield* delegate;}catch(error){return error;}})();
            generator.next();var result=generator[method](9);
            return result.done&&result.value===7&&generator.next().done;
        }));
    "#
        ),
        Value::Bool(true)
    );
    assert_eq!(vm.active_generator_delegations.capacity(), 0);
    assert!(vm.stack.is_empty() && vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn active_native_storage_delegate_callbacks_reject_reentry_through_result_getters() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        ['return','throw'].every(method=>{
            var generator,checks=0;
            function check(){for(var operation of ['next','return','throw']){try{generator[operation](7);return false;}catch(error){if(!(error instanceof TypeError))return false;checks++;}}$262.gc();return true;}
            var result={get done(){if(!check())throw 8;return true;},get value(){if(!check())throw 8;return 42;}};
            var delegate={[Symbol.iterator](){return this;},next(){return {value:1,done:false};}};
            Object.defineProperty(delegate,method,{get(){if(!check())throw 8;return function(){if(!check())throw 8;return result;};}});
            generator=(function*(){return yield* delegate;})();generator.next();
            var answer=generator[method](9);return answer.done&&answer.value===42&&checks===12&&generator.next().done;
        });
    "#
        ),
        Value::Bool(true)
    );
    assert_eq!(vm.active_generator_delegations.capacity(), 0);
    assert!(vm.stack.is_empty() && vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn active_native_storage_terminal_delegate_done_skips_the_value_getter() {
    for method in ["return", "throw"] {
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
        let receiver = run(&mut neutral, "var delegate={[Symbol.iterator](){return this;},next(){return {done:false,value:1};},return(){return {get done(){return markTerminal();},get value(){return observe();}};},throw(){return this.return();}};var generator=(function*(){yield* delegate;})();generator.next();generator;");
        let roots = realm_call_storage::RealmCallRoots::new(1, owner.as_ref()).unwrap();
        roots
            .push(&neutral.heap, receiver.object_id().unwrap())
            .unwrap();
        let result = neutral.with_realm_call_roots(roots, |vm| {
            if method == "return" {
                vm.generator_return(&receiver, Value::Number(9.0))
            } else {
                vm.generator_throw(&receiver, Value::Number(9.0))
            }
        });
        assert_eq!(result, Err(RuntimeError::SchedulingLimit));
        assert!(!seen.get(), "{method} read value after cancellation");
        assert_eq!(neutral.active_generator_delegations.capacity(), 0);
        assert!(neutral.stack.is_empty() && neutral.realm_call_roots.is_none());
    }
}

fn join_output_bytes_during_later_getter(units: usize) -> usize {
    let mut vm = page();
    let view = observer(&vm);
    let seen = Rc::new(std::cell::Cell::new(None));
    let output = seen.clone();
    vm.install_host_function("observe", 0, move |_: &[crate::HostValue]| {
        output.set(Some(view.host_scheduling_snapshot().payload_bytes));
        Ok(crate::HostValue::Undefined)
    })
    .unwrap();
    let source = run(
        &mut vm,
        "var source=[1,2];Object.defineProperty(source,'1',{get(){observe();return '';}});source;",
    )
    .object_id()
    .unwrap();
    vm.heap
        .set(
            source,
            "0",
            Value::String(JsString::from_code_units(vec![0x61; units])),
        )
        .unwrap();
    run(&mut vm, "source.join('');0;");
    seen.get().unwrap()
}

#[cfg_attr(test, test)]
fn active_native_storage_join_output_keeps_native_credit_during_a_later_getter() {
    let empty = join_output_bytes_during_later_getter(0);
    let twelve_units = join_output_bytes_during_later_getter(12);
    assert!(
        twelve_units >= empty + 24,
        "empty={empty}, with output={twelve_units}"
    );
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_active_native_storage_contracts() {
        active_native_storage_join_backing_outlives_the_vm();
        active_native_storage_delegation_backing_outlives_the_vm();
        active_native_storage_join_restores_the_outer_operand_stack();
        active_native_storage_join_throw_restores_the_outer_operand_stack();
        active_native_storage_completed_join_releases_its_receiver();
        active_native_storage_terminal_length_skips_separator_conversion();
        active_native_storage_join_preserves_holes_conversion_order_and_gc();
        active_native_storage_delegates_preserve_finite_return_throw_and_gc();
        active_native_storage_nested_join_entries_keep_one_backing_until_the_last_exit();
        active_native_storage_nested_delegations_keep_one_backing_until_the_last_exit();
        active_native_storage_neutral_backing_keeps_its_original_loan_after_both_vms_drop();
        active_native_storage_terminal_original_loan_rejects_another_marker();
        active_native_storage_neutral_policy_adopts_both_existing_allocations_once();
        active_native_storage_foreign_document_cannot_reassign_existing_loans();
        active_native_storage_join_roots_retire_each_completed_receiver();
        active_native_storage_terminal_separator_rejects_an_empty_join();
        active_native_storage_join_pins_the_separator_before_the_length_getter();
        active_native_storage_nested_joins_preserve_utf16_holes_and_gc();
        active_native_storage_abrupt_delegate_getters_release_the_running_marker();
        active_native_storage_delegate_callbacks_reject_reentry_through_result_getters();
        active_native_storage_terminal_delegate_done_skips_the_value_getter();
        active_native_storage_join_output_keeps_native_credit_during_a_later_getter();
    }
}
