// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Finite active-VM entry and completion cancellation contracts.

use super::*;
use crate::vm::native_value_storage::NativeValue;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}

#[cfg_attr(test, test)]
fn active_realm_resolution_failed_target_refuses_the_callback() {
    let mut target = page();
    let tag = target.object_prototype.heap;
    let caller = Vm::default();
    let before = target.host_scheduling_snapshot().payload_bytes;
    let active = register_active(&mut target).unwrap();
    let _ = target.host_scheduling.as_ref().unwrap().fail();
    let entered = Cell::new(false);
    let result = with_active_vm(caller.object_prototype.heap, tag, |_| {
        entered.set(true);
        Ok(())
    });
    drop(active);
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(!entered.get());
    assert_eq!(target.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(active_backing_bytes(), 0);
}

#[cfg_attr(test, test)]
fn active_realm_resolution_failed_peer_owner_cannot_borrow_healthy_ambient_credit() {
    let owner = page();
    let scope = owner.host_scheduling.as_ref().unwrap();
    let mut target = Vm::default();
    let tag = target.object_prototype.heap;
    let caller = Vm::default();
    let prepared = prepare_active_registration(&mut target, Some(scope)).unwrap();
    let active = prepared.publish(&mut target).unwrap();
    let mut ambient = page();
    let ambient_active = register_active(&mut ambient).unwrap();
    let _ = scope.fail();
    let entered = Cell::new(false);
    let result = with_active_vm(caller.object_prototype.heap, tag, |_| {
        entered.set(true);
        Ok(())
    });
    drop(ambient_active);
    drop(active);
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(!entered.get());
    assert!(ambient.check_scheduling_failure().is_ok());
    assert!(target.host_scheduling.is_none());
    assert_eq!(active_backing_bytes(), 0);
}

#[cfg_attr(test, test)]
fn active_realm_resolution_failed_retained_caller_refuses_neutral_target_entry() {
    let source = page();
    let mut caller = page();
    let scope = caller.host_scheduling.as_ref().unwrap().child();
    let visitor = Vm::default();
    let mut target = Vm::default();
    let tag = target.object_prototype.heap;
    let entered = Cell::new(false);
    let value = NativeValue::adopt(
        Value::String("tiny".into()),
        source.argument_list_owner().unwrap().as_ref(),
    )
    .unwrap();
    let caller_active = register_active(&mut caller).unwrap();
    let result = value.with_owned_context(&mut target, |vm, _| {
        let active = register_active(vm)?;
        let _ = scope.fail();
        let result = with_active_vm(visitor.object_prototype.heap, tag, |_| {
            entered.set(true);
            Ok(())
        });
        drop(active);
        result
    });
    drop(caller_active);
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(!entered.get());
    assert!(source.check_scheduling_failure().is_ok());
    assert!(target.realm_call_roots.is_none());
    assert_eq!(active_backing_bytes(), 0);
}

#[cfg_attr(test, test)]
fn active_realm_resolution_failed_registered_caller_refuses_a_healthy_target() {
    let mut target = page();
    let target_tag = target.object_prototype.heap;
    let target_active = register_active(&mut target).unwrap();
    let mut caller = page();
    let caller_tag = caller.object_prototype.heap;
    let caller_active = register_active(&mut caller).unwrap();
    let _ = caller.host_scheduling.as_ref().unwrap().fail();
    let entered = Cell::new(false);
    let result = with_active_vm(caller_tag, target_tag, |_| {
        entered.set(true);
        Ok(())
    });
    drop(caller_active);
    drop(target_active);
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(!entered.get());
    assert!(target.check_scheduling_failure().is_ok());
    assert_eq!(active_backing_bytes(), 0);
}

fn last_callback(cancel_caller: bool, language_error: bool) {
    let mut target = page();
    let target_tag = target.object_prototype.heap;
    let target_active = register_active(&mut target).unwrap();
    let mut caller = page();
    let caller_tag = caller.object_prototype.heap;
    let caller_active = register_active(&mut caller).unwrap();
    let scope = if cancel_caller { &caller } else { &target }
        .host_scheduling
        .as_ref()
        .unwrap()
        .child();
    let result = with_active_vm(caller_tag, target_tag, |_| {
        let _ = scope.fail();
        if language_error {
            Err(RuntimeError::TypeError("tiny".into()))
        } else {
            Ok(7)
        }
    });
    drop(caller_active);
    drop(target_active);
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(if cancel_caller { &target } else { &caller }
        .check_scheduling_failure()
        .is_ok());
    assert_eq!(active_backing_bytes(), 0);
}

#[cfg_attr(test, test)]
fn active_realm_resolution_last_success_checks_the_target_account() {
    last_callback(false, false);
}

#[cfg_attr(test, test)]
fn active_realm_resolution_last_error_checks_the_target_account() {
    last_callback(false, true);
}

#[cfg_attr(test, test)]
fn active_realm_resolution_last_success_checks_the_calling_account() {
    last_callback(true, false);
}

#[cfg_attr(test, test)]
fn active_realm_resolution_last_error_checks_the_calling_account() {
    last_callback(true, true);
}

#[cfg_attr(test, test)]
fn active_realm_resolution_current_failed_root_refuses_entry() {
    let source = page();
    let scope = source.host_scheduling.as_ref().unwrap().child();
    let visitor = Vm::default();
    let mut target = Vm::default();
    let tag = target.object_prototype.heap;
    let active = register_active(&mut target).unwrap();
    let value = NativeValue::adopt(
        Value::String("tiny".into()),
        source.argument_list_owner().unwrap().as_ref(),
    )
    .unwrap();
    let entered = Cell::new(false);
    let result = value.with_owned_context(&mut target, |_, _| {
        let _ = scope.fail();
        with_active_vm(visitor.object_prototype.heap, tag, |_| {
            entered.set(true);
            Ok(())
        })
    });
    drop(active);
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(!entered.get());
    assert!(target.realm_call_roots.is_none());
    assert_eq!(active_backing_bytes(), 0);
}

#[cfg_attr(test, test)]
fn active_realm_resolution_shadow_lookup_refuses_a_failed_target() {
    let mut target = page();
    let tag = target.object_prototype.heap;
    let visitor = Vm::default();
    let active = register_active(&mut target).unwrap();
    let _ = target.host_scheduling.as_ref().unwrap().fail();
    let result = visitor.resolve_shadow_ancestor(Some(tag)).map(|_| ());
    drop(active);
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(active_backing_bytes(), 0);
}

#[cfg_attr(test, test)]
fn active_realm_resolution_owned_target_ignores_unrelated_failed_ambient_work() {
    let mut target = page();
    let tag = target.object_prototype.heap;
    let caller = Vm::default();
    let active = register_active(&mut target).unwrap();
    let mut unrelated = page();
    let ambient = register_active(&mut unrelated).unwrap();
    let _ = unrelated.host_scheduling.as_ref().unwrap().fail();
    let result = with_active_vm(caller.object_prototype.heap, tag, |_| Ok(7));
    drop(ambient);
    drop(active);
    assert_eq!(result, Ok(7));
    assert!(target.check_scheduling_failure().is_ok());
    assert_eq!(active_backing_bytes(), 0);
}

#[cfg_attr(test, test)]
fn active_realm_resolution_healthy_language_error_is_preserved() {
    let mut target = page();
    let caller = Vm::default();
    let tag = target.object_prototype.heap;
    let active = register_active(&mut target).unwrap();
    let result: Result<(), RuntimeError> =
        with_active_vm(caller.object_prototype.heap, tag, |_| {
            Err(RuntimeError::TypeError("tiny".into()))
        });
    drop(active);
    assert_eq!(result, Err(RuntimeError::TypeError("tiny".into())));
    assert!(target.check_scheduling_failure().is_ok());
    assert_eq!(active_backing_bytes(), 0);
}

#[cfg_attr(test, test)]
fn active_realm_resolution_returned_call_is_unavailable_to_later_callbacks() {
    let mut target = page();
    let tag = target.object_prototype.heap;
    let caller = Vm::default();
    let active = register_active(&mut target).unwrap();
    drop(active);
    let entered = Cell::new(false);
    let result = with_active_vm(caller.object_prototype.heap, tag, |_| {
        entered.set(true);
        Ok(())
    });
    assert!(matches!(result, Err(RuntimeError::TypeError(_))));
    assert!(!entered.get());
    assert!(target.check_scheduling_failure().is_ok());
    assert_eq!(active_backing_bytes(), 0);
}

#[cfg_attr(test, test)]
fn active_realm_resolution_prepared_entry_cannot_extend_the_registration_window() {
    let mut target = page();
    let tag = target.object_prototype.heap;
    let caller = Vm::default();
    let before = target.host_scheduling_snapshot().payload_bytes;
    let active = register_active(&mut target).unwrap();
    let entry = resolve_active_entry(caller.object_prototype.heap, tag)
        .unwrap()
        .unwrap();
    drop(active);
    assert_eq!(active_backing_bytes(), 0);
    assert!(target.host_scheduling_snapshot().payload_bytes > before);
    let entered = Cell::new(false);
    let result = entry.with(|_| {
        entered.set(true);
        Ok(())
    });
    assert!(matches!(result, Err(RuntimeError::TypeError(_))));
    assert!(!entered.get());
    assert_eq!(target.host_scheduling_snapshot().payload_bytes, before);
}

fn execute(vm: &mut Vm, source: &str) -> Result<Value, RuntimeError> {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
}

fn in_plain_child(source: &str, operation: &str) -> Result<Value, RuntimeError> {
    // Construct only the existing realm record and membrane. These fixtures
    // install no conformance harness, timer machinery or external services.
    let mut parent = page();
    let mut child = Vm::default();
    child.inherit_document_policy(&parent).unwrap();
    let handle = realm_cells::RealmHandle::new(child).unwrap();
    let prototype = parent.object_prototype;
    let realm_id = parent
        .with_roots(|heap| heap.alloc_object(Some(prototype)))
        .unwrap();
    let realm_root = parent.heap.root(realm_id).unwrap();
    let slot = parent
        .test262_realms
        .prepare_insert(parent.host_scheduling.as_ref())
        .unwrap();
    parent.test262_realms.commit(slot);
    parent.test262_realms.insert(
        realm_id,
        Test262Realm {
            vm: handle.clone(),
            wrappers: MembraneMap::default(),
            imported_sources: MembraneMap::default(),
            imported_values: MembraneMap::default(),
            shadow_realm_reexports: MembraneMap::default(),
        },
    );
    let target = execute(&mut parent, source).unwrap();
    let facade = parent
        .test262_export_foreign_value(realm_id, &target)
        .unwrap();
    {
        let mut child = handle.borrow_mut();
        let global = child.global("globalThis").unwrap().object_id().unwrap();
        child
            .define_data(global, "target", facade, true, true, true)
            .unwrap();
    }
    let active = register_active(&mut parent).unwrap();
    let result = execute(&mut handle.borrow_mut(), operation);
    drop(active);
    parent.heap.unroot(realm_root).unwrap();
    assert!(parent.check_scheduling_failure().is_ok());
    assert_eq!(active_backing_bytes(), 0);
    result
}

#[cfg_attr(test, test)]
fn active_realm_resolution_plain_child_keeps_reverse_property_semantics() {
    assert_eq!(
        in_plain_child(
            "var object={a:1};object;",
            "var first=Object.getOwnPropertyDescriptor(target,'a');
             var proto=Object.getPrototypeOf(target);
             Object.setPrototypeOf(target,null);
             Object.defineProperty(target,'b',{value:2,writable:true,configurable:true,enumerable:true});
             target.a=3;
             var explicit=Reflect.set(target,'b',4,target);
             var keys=Reflect.ownKeys(target).join(',');
             var removed=delete target.b;
             first.value===1&&proto!==null&&Object.getPrototypeOf(target)===null&&
             target.a===3&&explicit&&keys==='a,b'&&removed&&target.b===undefined;",
        ),
        Ok(Value::Bool(true))
    );
}

#[cfg_attr(test, test)]
fn active_realm_resolution_plain_child_keeps_reverse_calls_construction_and_throws() {
    assert_eq!(
        in_plain_child(
            "var object={call:function(v){return v+1;},Box:function Box(v){this.value=v;},
                         fail:function(){throw 7;}};object;",
            "var called=target.call(4);var box=new target.Box(6);var caught=false;
             try{target.fail();}catch(value){caught=value===7;}
             called===5&&box.value===6&&caught;",
        ),
        Ok(Value::Bool(true))
    );
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_active_realm_resolution_contracts() {
        active_realm_resolution_failed_target_refuses_the_callback();
        active_realm_resolution_failed_peer_owner_cannot_borrow_healthy_ambient_credit();
        active_realm_resolution_failed_retained_caller_refuses_neutral_target_entry();
        active_realm_resolution_failed_registered_caller_refuses_a_healthy_target();
        active_realm_resolution_last_success_checks_the_target_account();
        active_realm_resolution_last_error_checks_the_target_account();
        active_realm_resolution_last_success_checks_the_calling_account();
        active_realm_resolution_last_error_checks_the_calling_account();
        active_realm_resolution_current_failed_root_refuses_entry();
        active_realm_resolution_shadow_lookup_refuses_a_failed_target();
        active_realm_resolution_owned_target_ignores_unrelated_failed_ambient_work();
        active_realm_resolution_healthy_language_error_is_preserved();
        active_realm_resolution_returned_call_is_unavailable_to_later_callbacks();
        active_realm_resolution_prepared_entry_cannot_extend_the_registration_window();
        active_realm_resolution_plain_child_keeps_reverse_property_semantics();
        active_realm_resolution_plain_child_keeps_reverse_calls_construction_and_throws();
    }
}
