// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed small calls observe argument ownership through legitimate ancestor entry.

use super::*;

fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}
fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    run(
        &mut vm,
        "var realm=new ShadowRealm();var fn=realm.evaluate('(text,probe)=>probe()');0;",
    );
    vm
}
fn probe(vm: &mut Vm) -> ObjectId {
    vm.lookup_global_name("probe")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap()
}

#[cfg_attr(test, test)]
fn shadow_argument_storage_reuses_the_incoming_argument_allocation() {
    let mut vm = page();
    let parent = vm.object_prototype.heap;
    let child = vm.shadow_realms.values().next().unwrap().heap;
    let mut args = Vec::with_capacity(7);
    let address = args.as_ptr() as usize;
    let capacity = args.capacity();
    vm.install_host_function("probe", 0, move |_: &[HostValue]| {
        // The child is suspended while its wrapped callback executes here.
        let same = realm_reentrancy::with_active_vm(parent, child, |vm| {
            Ok(vm.arguments.as_ptr() as usize == address && vm.arguments.capacity() == capacity)
        })
        .unwrap();
        Ok(HostValue::Bool(same))
    })
    .unwrap();
    args.push(Value::String("ab".into()));
    args.push(Value::Object(probe(&mut vm)));
    let callee = vm.lookup_global_name("fn").unwrap().unwrap();
    assert_eq!(
        vm.call_native(callee, Value::Undefined, args, false)
            .unwrap(),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn shadow_argument_storage_gc_roots_do_not_clone_primitive_payloads() {
    let mut vm = page();
    let parent = vm.object_prototype.heap;
    let child = vm.shadow_realms.values().next().unwrap().heap;
    let observed = Rc::new(std::cell::Cell::new(None));
    let output = observed.clone();
    vm.install_host_function("inspectRoots", 0, move |_: &[HostValue]| {
        let copied = realm_reentrancy::with_active_vm(parent, child, |vm| {
            Ok(vm
                .stack
                .iter()
                .any(|value| matches!(value, Value::String(_))))
        })
        .unwrap();
        output.set(Some(copied));
        Ok(HostValue::Undefined)
    })
    .unwrap();
    run(&mut vm, "var callback=()=>7;Object.defineProperty(callback,'name',{get:function(){inspectRoots();return 'callback';}});fn('ab',callback);0;");
    assert_eq!(observed.get(), Some(false));
}

#[cfg_attr(test, test)]
fn shadow_argument_storage_source_entry_releases_transferred_control_credit() {
    let mut vm = page();
    let counter = vm.displaced_execution_control_bytes.clone();
    vm.install_host_function("probe", 0, move |_: &[HostValue]| {
        Ok(HostValue::Number(counter.get() as f64))
    })
    .unwrap();
    let mut args = Vec::with_capacity(7);
    args.push(Value::String("ab".into()));
    args.push(Value::Object(probe(&mut vm)));
    let callee = vm.lookup_global_name("fn").unwrap().unwrap();
    assert_eq!(
        vm.call_native(callee, Value::Undefined, args, false)
            .unwrap(),
        Value::Number(0.0)
    );
}

fn bounded() -> Vm {
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

fn input(vm: &Vm, receiver: Value, arguments: Vec<Value>) -> RealmCallInput {
    let entry = vm.call_entry_charge(&Value::Undefined, &receiver, &arguments);
    RealmCallInput {
        callee: Value::Undefined,
        receiver,
        arguments,
        entry,
    }
}

#[cfg_attr(test, test)]
fn shadow_argument_storage_nested_roots_survive_gc_and_restore_after_error() {
    let mut vm = bounded();
    let first = vm.heap.alloc_object(None).unwrap();
    let second = vm.heap.alloc_object(None).unwrap();
    let owner = vm
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(0)
        .unwrap();
    let outer = RealmCallRoots::new(1, Some(&owner)).unwrap();
    outer.push(&vm.heap, first).unwrap();
    let inner = RealmCallRoots::new(1, Some(&owner)).unwrap();
    inner.push(&vm.heap, second).unwrap();
    vm.with_realm_call_roots(outer.clone(), |vm| {
        let error = vm.with_realm_call_roots(inner.clone(), |vm| {
            vm.with_roots(|heap| {
                heap.collect_major();
                Ok(())
            })?;
            assert!(vm.heap.contains(first) && vm.heap.contains(second));
            Err::<(), _>(RuntimeError::TypeError("metadata".into()))
        });
        assert_eq!(error, Err(RuntimeError::TypeError("metadata".into())));
        let mut roots = Vec::new();
        vm.visit_realm_call_roots(|id| roots.push(id));
        assert_eq!(roots, vec![first]);
        vm.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })?;
        assert!(vm.heap.contains(first));
        assert!(!vm.heap.contains(second));
        Ok(())
    })
    .unwrap();
    assert!(vm.realm_call_roots.is_none());
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    assert!(!vm.heap.contains(first));
    // Keeping the detached frame must not keep a previous frame or its IDs rooted.
    assert_eq!(Rc::strong_count(&outer), 1);
    assert_eq!(Rc::strong_count(&inner), 1);
}

#[cfg_attr(test, test)]
fn shadow_argument_storage_root_allocation_credit_outlives_its_vm() {
    let vm = bounded();
    let view = observer(&vm);
    let owner = vm
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(0)
        .unwrap();
    let before = view.host_scheduling_snapshot().payload_bytes;
    let roots = RealmCallRoots::new(3, Some(&owner)).unwrap();
    let retained = view.host_scheduling_snapshot().payload_bytes - before;
    assert!(retained >= 3 * std::mem::size_of::<ObjectId>());
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(roots);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn shadow_argument_storage_input_lease_outlives_the_source_vm() {
    let mut vm = bounded();
    let view = observer(&vm);
    let mut args = Vec::with_capacity(7);
    args.push(Value::String("ab".into()));
    let mut input = input(&vm, Value::String("cd".into()), args);
    let expected = host_scheduling::slots(&input.arguments)
        + host_scheduling::values_bytes(&input.arguments)
        + host_scheduling::value_payload_bytes(&input.receiver);
    input
        .entry
        .retain_native(
            &mut vm,
            &input.callee,
            &input.receiver,
            &input.arguments,
            None,
        )
        .unwrap();
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    assert_eq!(vm.displaced_execution_bytes.get(), 0);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, expected);
    assert_eq!(input.arguments, vec![Value::String("ab".into())]);
    drop(input);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn shadow_argument_storage_foreign_document_handoff_moves_one_entry_lease() {
    let mut source = bounded();
    let target = bounded();
    let source_view = observer(&source);
    let target_view = observer(&target);
    let source_base = source_view.host_scheduling_snapshot().payload_bytes;
    let target_base = target_view.host_scheduling_snapshot().payload_bytes;
    let mut args = Vec::with_capacity(7);
    args.push(Value::String("ab".into()));
    let mut input = input(&source, Value::Undefined, args);
    let bytes =
        host_scheduling::slots(&input.arguments) + host_scheduling::values_bytes(&input.arguments);
    input
        .entry
        .retain_native(
            &mut source,
            &input.callee,
            &input.receiver,
            &input.arguments,
            None,
        )
        .unwrap();
    assert_eq!(
        source_view.host_scheduling_snapshot().payload_bytes,
        source_base + bytes
    );
    input
        .entry
        .retarget_native(source.host_scheduling.as_ref())
        .unwrap();
    assert_eq!(
        source_view.host_scheduling_snapshot().payload_bytes,
        source_base + bytes
    );
    input
        .entry
        .retarget_native(target.host_scheduling.as_ref())
        .unwrap();
    assert_eq!(
        source_view.host_scheduling_snapshot().payload_bytes,
        source_base
    );
    assert_eq!(
        target_view.host_scheduling_snapshot().payload_bytes,
        target_base + bytes
    );
    drop(input);
    assert_eq!(
        target_view.host_scheduling_snapshot().payload_bytes,
        target_base
    );
}

#[cfg_attr(test, test)]
fn shadow_argument_storage_fixed_policy_pairs_preserve_results_and_neutral_policy() {
    for (source_bounded, target_bounded) in
        [(false, false), (true, false), (false, true), (true, true)]
    {
        let mut source = if source_bounded {
            bounded()
        } else {
            Vm::default()
        };
        let mut target = if target_bounded {
            bounded()
        } else {
            Vm::default()
        };
        let callee = run(&mut target, "(text,n)=>text+n;").object_id().unwrap();
        let mut args = Vec::with_capacity(7);
        args.push(Value::String("ab".into()));
        args.push(Value::Number(3.0));
        let input = input(&source, Value::Undefined, args);
        assert_eq!(
            source
                .shadow_call_across(&mut target, callee, input)
                .unwrap(),
            Value::String("ab3".into())
        );
        assert_eq!(source.host_scheduling.is_some(), source_bounded);
        assert_eq!(target.host_scheduling.is_some(), target_bounded);
        assert!(source.realm_call_roots.is_none() && target.realm_call_roots.is_none());
        assert_eq!(source.displaced_execution_control_bytes.get(), 0);
        assert_eq!(target.displaced_execution_control_bytes.get(), 0);
    }
}

#[cfg_attr(test, test)]
fn shadow_argument_storage_neutral_nesting_borrows_only_active_native_credit() {
    let mut owner_vm = bounded();
    let view = observer(&owner_vm);
    let registration = prepare_active_registration(&mut owner_vm, None).unwrap();
    let guard = registration.publish(&mut owner_vm);
    let before = view.host_scheduling_snapshot().payload_bytes;
    let mut source = Vm::default();
    let target = Vm::default();
    let owner = source.realm_call_native_owner(&target).unwrap().unwrap();
    let roots = RealmCallRoots::new(2, Some(&owner)).unwrap();
    let mut args = Vec::with_capacity(4);
    args.push(Value::String("ab".into()));
    let mut input = input(&source, Value::Undefined, args);
    input
        .entry
        .retain_native(
            &mut source,
            &input.callee,
            &input.receiver,
            &input.arguments,
            Some(&owner),
        )
        .unwrap();
    assert!(view.host_scheduling_snapshot().payload_bytes > before);
    assert!(source.host_scheduling.is_none() && target.host_scheduling.is_none());
    drop(input);
    drop(roots);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
    drop(guard);
}

#[cfg_attr(test, test)]
fn shadow_argument_storage_gc_in_metadata_getters_preserves_inputs_and_completed_facades() {
    let mut vm = page();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var call=realm.evaluate('(a,b)=>a()+b()');
        var second=()=>2;
        Object.defineProperty(second,'name',{get:function(){ $262.gc(); return 'second'; }});
        call(()=>7,second);
    "#
        ),
        Value::Number(9.0)
    );
    assert!(vm.realm_call_roots.is_none());
    assert!(vm.shadow_realms.values().all(|record| record
        .vm
        .borrow_mut()
        .realm_call_roots
        .is_none()));
}

#[cfg_attr(test, test)]
fn shadow_argument_storage_metadata_and_target_throws_release_frames() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var bad=()=>7;
        Object.defineProperty(bad,'name',{get:function(){throw 'private';}});
        var masked=false;
        try { fn('ab',bad); } catch(error) { masked=error instanceof TypeError; }
        masked;
    "#
        ),
        Value::Bool(true)
    );
    assert!(vm.realm_call_roots.is_none());
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    assert_eq!(
        run(
            &mut vm,
            r#"
        var throws=realm.evaluate('()=>{throw "private";}');
        var masked=false;
        try { throws(); } catch(error) { masked=error instanceof TypeError && error.message===''; }
        masked;
    "#
        ),
        Value::Bool(true)
    );
    assert!(vm.realm_call_roots.is_none());
    for record in vm.shadow_realms.values() {
        let child = record.vm.borrow_mut();
        assert!(child.realm_call_roots.is_none());
        assert_eq!(child.displaced_execution_control_bytes.get(), 0);
    }
}

#[cfg_attr(test, test)]
fn shadow_argument_storage_explicit_terminal_marker_is_not_masked_as_a_typeerror() {
    let mut vm = page();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    vm.install_host_function("markTerminal", 0, move |_: &[HostValue]| {
        let _ = scope.fail();
        Ok(HostValue::Undefined)
    })
    .unwrap();
    run(
        &mut vm,
        "var invoke=realm.evaluate('(callback)=>callback()');0;",
    );
    let callee = vm.lookup_global_name("invoke").unwrap().unwrap();
    let callback = vm.lookup_global_name("markTerminal").unwrap().unwrap();
    assert_eq!(
        vm.call_native(callee, Value::Undefined, vec![callback], false),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(vm.realm_call_roots.is_none());
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    for record in vm.shadow_realms.values() {
        let child = record.vm.borrow_mut();
        assert!(child.realm_call_roots.is_none());
        assert_eq!(child.displaced_execution_control_bytes.get(), 0);
    }
}

#[cfg_attr(test, test)]
fn shadow_argument_storage_tail_calls_return_the_actual_completion() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var tail=realm.evaluate('(function(){"use strict";return Math.abs(-7);})');
        tail();
    "#
        ),
        Value::Number(7.0)
    );
    for record in vm.shadow_realms.values() {
        assert!(record.vm.borrow_mut().pending_tail_call.is_none());
    }
}

#[cfg_attr(test, test)]
fn shadow_argument_storage_tail_calls_throw_before_boundary_masking() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var tail=realm.evaluate('(function(){function fail(){throw "private";}return function(){"use strict";return fail();};})()');
        var caught=false;
        try { tail(); } catch(error) { caught=error instanceof TypeError && error.message===''; }
        caught;
    "#
        ),
        Value::Bool(true)
    );
    for record in vm.shadow_realms.values() {
        assert!(record.vm.borrow_mut().pending_tail_call.is_none());
    }
}

#[cfg_attr(test, test)]
fn shadow_argument_storage_tail_calls_reenter_the_caller_once() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var count=0;
        var tail=realm.evaluate('(function(callback){"use strict";return callback(3);})');
        var result=tail(function(n){count++;return n+4;});
        result===7 && count===1;
    "#
        ),
        Value::Bool(true)
    );
    assert!(vm.realm_call_roots.is_none());
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    for record in vm.shadow_realms.values() {
        let child = record.vm.borrow_mut();
        assert!(child.pending_tail_call.is_none());
        assert!(child.realm_call_roots.is_none());
        assert_eq!(child.displaced_execution_control_bytes.get(), 0);
    }
}

#[cfg_attr(test, test)]
fn shadow_argument_storage_tail_calls_wrap_the_final_callable() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var tail=realm.evaluate('(function(){function make(){return ()=>9;}return function(){"use strict";return make();};})()');
        var result=tail();
        typeof result==='function' && result()===9;
    "#
        ),
        Value::Bool(true)
    );
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn shadow_argument_storage_tail_calls_validate_the_final_object_result() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var tail=realm.evaluate('(function(){function make(){return {answer:9};}return function(){"use strict";return make();};})()');
        var caught=false;
        try { tail(); } catch(error) { caught=error instanceof TypeError; }
        caught;
    "#
        ),
        Value::Bool(true)
    );
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn shadow_argument_storage_tail_calls_complete_a_fixed_two_step_chain() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var tail=realm.evaluate('(function(){function last(n){return n+1;}function middle(n){"use strict";return last(n);}return function(n){"use strict";return middle(n);};})()');
        tail(3);
    "#
        ),
        Value::Number(4.0)
    );
    for record in vm.shadow_realms.values() {
        assert!(record.vm.borrow_mut().pending_tail_call.is_none());
    }
}

#[cfg_attr(test, test)]
fn shadow_argument_storage_tail_calls_leave_no_work_for_the_next_boundary_call() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var tail=realm.evaluate('(function(){var calls=0;function finish(){calls++;return calls;}return function(){"use strict";return finish();};})()');
        var first=tail();
        var second=tail();
        first===1 && second===2;
    "#
        ),
        Value::Bool(true)
    );
    for record in vm.shadow_realms.values() {
        assert!(record.vm.borrow_mut().pending_tail_call.is_none());
    }
}

#[cfg_attr(test, test)]
fn shadow_argument_storage_tail_calls_propagate_an_explicit_terminal_marker() {
    let mut vm = page();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    vm.install_host_function("markTailTerminal", 0, move |_: &[HostValue]| {
        let _ = scope.fail();
        Ok(HostValue::Undefined)
    })
    .unwrap();
    run(
        &mut vm,
        "var tail=realm.evaluate('(function(callback){\"use strict\";return callback();})');0;",
    );
    let callee = vm.lookup_global_name("tail").unwrap().unwrap();
    let callback = vm.lookup_global_name("markTailTerminal").unwrap().unwrap();
    assert_eq!(
        vm.call_native(callee, Value::Undefined, vec![callback], false),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(vm.realm_call_roots.is_none());
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    for record in vm.shadow_realms.values() {
        let child = record.vm.borrow_mut();
        assert!(child.pending_tail_call.is_none());
        assert!(child.realm_call_roots.is_none());
        assert_eq!(child.displaced_execution_control_bytes.get(), 0);
    }
}

impl Vm {
    pub fn verify_shadow_argument_storage_contracts() {
        shadow_argument_storage_tail_calls_wrap_the_final_callable();
        shadow_argument_storage_tail_calls_validate_the_final_object_result();
        shadow_argument_storage_tail_calls_complete_a_fixed_two_step_chain();
        shadow_argument_storage_tail_calls_leave_no_work_for_the_next_boundary_call();
        shadow_argument_storage_tail_calls_propagate_an_explicit_terminal_marker();
        shadow_argument_storage_tail_calls_return_the_actual_completion();
        shadow_argument_storage_tail_calls_throw_before_boundary_masking();
        shadow_argument_storage_tail_calls_reenter_the_caller_once();
        shadow_argument_storage_reuses_the_incoming_argument_allocation();
        shadow_argument_storage_gc_roots_do_not_clone_primitive_payloads();
        shadow_argument_storage_source_entry_releases_transferred_control_credit();
        shadow_argument_storage_nested_roots_survive_gc_and_restore_after_error();
        shadow_argument_storage_root_allocation_credit_outlives_its_vm();
        shadow_argument_storage_input_lease_outlives_the_source_vm();
        shadow_argument_storage_foreign_document_handoff_moves_one_entry_lease();
        shadow_argument_storage_fixed_policy_pairs_preserve_results_and_neutral_policy();
        shadow_argument_storage_neutral_nesting_borrows_only_active_native_credit();
        shadow_argument_storage_gc_in_metadata_getters_preserves_inputs_and_completed_facades();
        shadow_argument_storage_metadata_and_target_throws_release_frames();
        shadow_argument_storage_explicit_terminal_marker_is_not_masked_as_a_typeerror();
    }
}
