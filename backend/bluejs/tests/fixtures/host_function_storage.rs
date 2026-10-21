// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny primitive-only callbacks keep registration ownership and cancellation.

use super::*;
use std::cell::Cell;

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

fn loan<T>(
    vm: &mut Vm,
    source: &Vm,
    action: impl FnOnce(&mut Vm) -> Result<T, RuntimeError>,
) -> Result<T, RuntimeError> {
    let owner = source.argument_list_owner()?;
    let roots = realm_call_storage::RealmCallRoots::new(0, owner.as_ref())?;
    vm.with_realm_call_roots(roots, action)
}

fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}

fn install(vm: &mut Vm) {
    let value = [7u64, 4];
    vm.install_host_function("read", 0, move |_: &[HostValue]| {
        Ok(HostValue::Number((value[0] + value[1]) as f64))
    })
    .unwrap();
}

fn object_count(vm: &Vm) -> usize {
    let stats = vm.heap.stats();
    stats.nursery_objects + stats.tenured_objects
}

fn bytes(vm: &Vm) -> usize {
    vm.host_functions.capacity() * std::mem::size_of::<Box<dyn HostFunction>>()
        + vm.host_functions
            .iter()
            .map(|function| std::mem::size_of_val(&**function))
            .sum::<usize>()
}

#[cfg_attr(test, test)]
fn host_function_storage_callback_and_slots_outlive_the_vm() {
    let mut vm = page();
    install(&mut vm);
    let view = observer(&vm);
    let retained = bytes(&vm);
    let functions = std::mem::take(&mut vm.host_functions);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(functions);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn host_function_storage_policy_adopts_neutral_backing_and_boxes() {
    let mut vm = Vm::default();
    install(&mut vm);
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let view = observer(&vm);
    let retained = bytes(&vm);
    let functions = std::mem::take(&mut vm.host_functions);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(functions);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn host_function_storage_terminal_caller_precedes_cold_global_allocation() {
    let source = page();
    let mut vm = Vm::default();
    let objects = object_count(&vm);
    let result = loan(&mut vm, &source, |vm| {
        let _ = source.host_scheduling.as_ref().unwrap().fail();
        vm.install_host_function("read", 0, |_: &[HostValue]| Ok(HostValue::Undefined))
    });
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(object_count(&vm), objects);
    assert!(vm.host_functions.is_empty() && vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn host_function_storage_terminal_original_owner_stops_new_global_publication() {
    let source = page();
    let caller = page();
    let mut vm = Vm::default();
    loan(&mut vm, &source, |vm| {
        install(vm);
        Ok(())
    })
    .unwrap();
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    let objects = object_count(&vm);
    let result = loan(&mut vm, &caller, |vm| {
        vm.install_host_function("next", 0, |_: &[HostValue]| Ok(HostValue::Undefined))
    });
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(vm.host_functions.len(), 1);
    assert_eq!(object_count(&vm), objects);
    assert_eq!(
        run(&mut vm, "Object.hasOwn(globalThis,'next');"),
        Value::Bool(false)
    );
    assert!(caller.argument_list_owner().is_ok());
}

#[cfg_attr(test, test)]
fn host_function_storage_terminal_method_does_not_publish_a_native_index() {
    let source = page();
    let mut vm = Vm::default();
    let object = vm.install_host_object("target").unwrap();
    let objects = object_count(&vm);
    let result = loan(&mut vm, &source, |vm| {
        let _ = source.host_scheduling.as_ref().unwrap().fail();
        vm.install_host_method(object, "read", 0, |_: &[HostValue]| {
            Ok(HostValue::Undefined)
        })
    });
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(vm.host_functions.is_empty());
    assert_eq!(object_count(&vm), objects);
    assert!(vm
        .heap
        .get_own_property_descriptor(object.0, "read")
        .unwrap()
        .is_none());
}

#[cfg_attr(test, test)]
fn host_function_storage_terminal_registry_stops_empty_argument_callback_entry() {
    let source = page();
    let caller = page();
    let called = Rc::new(Cell::new(false));
    let observed = called.clone();
    let mut vm = Vm::default();
    loan(&mut vm, &source, |vm| {
        vm.install_host_function("read", 0, move |_: &[HostValue]| {
            observed.set(true);
            Ok(HostValue::Undefined)
        })
    })
    .unwrap();
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    let result = loan(&mut vm, &caller, |vm| {
        vm.host_function_call(0, Value::Undefined, &[], false)
    });
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(!called.get());
    assert!(vm.realm_call_roots.is_none());
    assert!(caller.argument_list_owner().is_ok());
}

fn cancellation_return(original: bool, error: bool) {
    let source = page();
    let caller = page();
    let mut vm = Vm::default();
    let scope = if original { &source } else { &caller }
        .host_scheduling
        .as_ref()
        .unwrap()
        .child();
    loan(&mut vm, &source, |vm| {
        vm.install_host_function("cancel", 0, move |_: &[HostValue]| {
            let _ = scope.fail();
            if error {
                Err(HostFunctionError::new("callback error"))
            } else {
                Ok(HostValue::Undefined)
            }
        })
    })
    .unwrap();
    let result = loan(&mut vm, &caller, |vm| {
        vm.host_function_call(0, Value::Undefined, &[], false)
    });
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(vm.realm_call_roots.is_none());
    let healthy = if original { &caller } else { &source };
    assert!(healthy.argument_list_owner().is_ok());
}

#[cfg_attr(test, test)]
fn host_function_storage_original_cancellation_discards_success() {
    cancellation_return(true, false);
}

#[cfg_attr(test, test)]
fn host_function_storage_original_cancellation_precedes_error_formatting() {
    cancellation_return(true, true);
}

#[cfg_attr(test, test)]
fn host_function_storage_caller_cancellation_discards_success() {
    cancellation_return(false, false);
}

#[cfg_attr(test, test)]
fn host_function_storage_caller_cancellation_precedes_error_formatting() {
    cancellation_return(false, true);
}

#[cfg_attr(test, test)]
fn host_function_storage_neutral_callback_entry_adopts_existing_records() {
    let source = page();
    let view = observer(&source);
    let before = view.host_scheduling_snapshot().payload_bytes;
    let mut vm = Vm::default();
    install(&mut vm);
    let retained = bytes(&vm);
    assert_eq!(
        loan(&mut vm, &source, |vm| vm.host_function_call(
            0,
            Value::Undefined,
            &[],
            false
        )),
        Ok(Value::Number(11.0))
    );
    let functions = std::mem::take(&mut vm.host_functions);
    drop(vm);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        before + retained
    );
    drop(functions);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn host_function_storage_registry_does_not_join_a_different_document() {
    let source = page();
    let mut vm = Vm::default();
    loan(&mut vm, &source, |vm| {
        install(vm);
        Ok(())
    })
    .unwrap();
    assert!(vm.host_registration_storage_can_join(source.host_scheduling.as_ref().unwrap()));
    assert_eq!(
        vm.use_bounded_host_microtask_checkpoints(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(source.argument_list_owner().is_ok());
}

#[cfg_attr(test, test)]
fn host_function_storage_primitive_conversion_and_callback_errors_are_preserved() {
    let mut vm = page();
    vm.install_host_function("echo", 1, |args: &[HostValue]| Ok(args[0].clone()))
        .unwrap();
    for value in [
        Value::Undefined,
        Value::Null,
        Value::Bool(true),
        Value::Number(7.0),
        Value::String("tiny".into()),
    ] {
        assert_eq!(
            vm.host_function_call(0, Value::Undefined, std::slice::from_ref(&value), false),
            Ok(value)
        );
    }
    assert!(
        matches!(vm.host_function_call(0, Value::Undefined, &[Value::Object(vm.object_prototype)], false), Err(RuntimeError::TypeError(message)) if message == "host functions accept primitive values only")
    );
    vm.install_host_function("reject", 0, |_: &[HostValue]| {
        Err(HostFunctionError::new("tiny error"))
    })
    .unwrap();
    assert_eq!(
        vm.host_function_call(1, Value::Undefined, &[], false),
        Err(RuntimeError::TypeError("tiny error".into()))
    );
}

#[cfg_attr(test, test)]
fn host_function_storage_public_indices_metadata_and_duplicate_rejection_are_preserved() {
    let mut vm = page();
    install(&mut vm);
    let object = vm.install_host_object("target").unwrap();
    vm.install_host_method(object, "echo", 1, |args: &[HostValue]| Ok(args[0].clone()))
        .unwrap();
    assert!(matches!(
        vm.install_host_function("read", 0, |_: &[HostValue]| Ok(HostValue::Null)),
        Err(RuntimeError::TypeError(_))
    ));
    assert_eq!(vm.host_functions.len(), 2);
    assert_eq!(run(&mut vm, "read()===11&&target.echo('tiny')==='tiny'&&read.name==='read'&&target.echo.length===1&&(()=>{try{new read();return false}catch(e){return e instanceof TypeError}})();"), Value::Bool(true));
}

#[cfg_attr(test, test)]
fn host_function_storage_growth_releases_old_backing_and_retains_every_callback() {
    let mut vm = page();
    for (index, name) in ["a", "b", "c", "d", "e"].into_iter().enumerate() {
        let value = [index as u64];
        vm.install_host_function(name, 0, move |_: &[HostValue]| {
            Ok(HostValue::Number(value[0] as f64))
        })
        .unwrap();
    }
    assert_eq!(
        run(&mut vm, "a()===0&&b()===1&&c()===2&&d()===3&&e()===4;"),
        Value::Bool(true)
    );
    let view = observer(&vm);
    let retained = bytes(&vm);
    let functions = std::mem::take(&mut vm.host_functions);
    assert_eq!(functions.len(), 5);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(functions);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

struct CallbackProbe {
    view: Rc<Vm>,
    seen: Rc<Cell<usize>>,
    drops: Rc<Cell<usize>>,
}

impl HostFunction for CallbackProbe {
    fn call(&mut self, _: &[HostValue]) -> Result<HostValue, HostFunctionError> {
        Ok(HostValue::Undefined)
    }
}

impl Drop for CallbackProbe {
    fn drop(&mut self) {
        self.seen
            .set(self.view.host_scheduling_snapshot().payload_bytes);
        self.drops.set(self.drops.get() + 1);
    }
}

#[cfg_attr(test, test)]
fn host_function_storage_callback_destructor_observes_live_box_and_slot_credit() {
    let mut vm = page();
    let view = Rc::new(observer(&vm));
    let seen = Rc::new(Cell::new(0));
    let drops = Rc::new(Cell::new(0));
    vm.install_host_function(
        "read",
        0,
        CallbackProbe {
            view: view.clone(),
            seen: seen.clone(),
            drops: drops.clone(),
        },
    )
    .unwrap();
    let retained = bytes(&vm);
    let functions = std::mem::take(&mut vm.host_functions);
    drop(vm);
    assert_eq!(drops.get(), 0);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(functions);
    assert_eq!(drops.get(), 1);
    assert_eq!(seen.get(), retained);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn host_function_storage_failed_publication_releases_prepared_callback_and_slots() {
    let mut vm = page();
    let object = vm.install_host_object("target").unwrap();
    run(&mut vm, "Object.preventExtensions(target);0;");
    let view = Rc::new(observer(&vm));
    let seen = Rc::new(Cell::new(0));
    let drops = Rc::new(Cell::new(0));
    let result = vm.install_host_method(
        object,
        "read",
        0,
        CallbackProbe {
            view: view.clone(),
            seen,
            drops: drops.clone(),
        },
    );
    assert!(matches!(result, Err(RuntimeError::TypeError(_))));
    assert!(vm.host_functions.is_empty());
    assert_eq!(drops.get(), 1);
    assert!(vm
        .heap
        .get_own_property_descriptor(object.0, "read")
        .unwrap()
        .is_none());
    assert!(vm.argument_list_owner().is_ok());
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn host_function_storage_terminal_preparation_stops_zero_sized_box_construction() {
    let vm = page();
    let view = observer(&vm);
    let owner = vm.argument_list_owner().unwrap();
    let prepared = vm.host_functions.prepare(1, owner.as_ref()).unwrap();
    let slots = prepared.staging_bytes();
    let built = Cell::new(false);
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    let result = prepared.callbacks(&[0], || {
        built.set(true);
        [Box::new(|_: &[HostValue]| Ok(HostValue::Undefined)) as Box<dyn HostFunction>]
    });
    assert!(matches!(result, Err(RuntimeError::SchedulingLimit)));
    assert!(!built.get());
    assert!(vm.host_functions.is_empty());
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, slots);
    drop(prepared);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn host_function_storage_zero_sized_callbacks_own_only_the_real_registry_backing() {
    let mut vm = page();
    vm.install_host_function("read", 0, |_: &[HostValue]| Ok(HostValue::Undefined))
        .unwrap();
    let retained = bytes(&vm);
    assert_eq!(
        retained,
        vm.host_functions.capacity() * std::mem::size_of::<Box<dyn HostFunction>>()
    );
    let view = observer(&vm);
    let functions = std::mem::take(&mut vm.host_functions);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, retained);
    drop(functions);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn host_function_storage_manual_timer_promise_and_major_gc_preserve_indices() {
    let mut vm = page();
    vm.install_host_function("read", 0, |_: &[HostValue]| {
        Ok(HostValue::String("tiny".into()))
    })
    .unwrap();
    let object = vm.install_host_object("target").unwrap();
    let value = [7u64];
    vm.install_host_method(object, "value", 0, move |_: &[HostValue]| {
        Ok(HostValue::Number(value[0] as f64))
    })
    .unwrap();
    vm.install_test262_harness().unwrap();
    run(&mut vm,"delete globalThis.setTimeout;delete globalThis.clearTimeout;delete globalThis.setInterval;delete globalThis.clearInterval;delete globalThis.queueMicrotask;0;");
    vm.install_host_timer_functions().unwrap();
    run(&mut vm,"var result=false;setTimeout(()=>Promise.resolve(read()).then(value=>{$262.gc();result=value==='tiny'&&read()===value&&target.value()===7;}),1);0;");
    let turn = vm.run_due_host_timers(1).unwrap();
    assert_eq!(turn.tasks_run, 1);
    assert_eq!(turn.uncaught_errors, 0);
    assert_eq!(vm.next_host_timer_due(), None);
    assert_eq!(run(&mut vm, "result;"), Value::Bool(true));
    assert!(vm.realm_call_roots.is_none() && vm.stack.is_empty());
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_host_function_storage_contracts() {
        host_function_storage_callback_and_slots_outlive_the_vm();
        host_function_storage_policy_adopts_neutral_backing_and_boxes();
        host_function_storage_terminal_caller_precedes_cold_global_allocation();
        host_function_storage_terminal_original_owner_stops_new_global_publication();
        host_function_storage_terminal_method_does_not_publish_a_native_index();
        host_function_storage_terminal_registry_stops_empty_argument_callback_entry();
        host_function_storage_original_cancellation_discards_success();
        host_function_storage_original_cancellation_precedes_error_formatting();
        host_function_storage_caller_cancellation_discards_success();
        host_function_storage_caller_cancellation_precedes_error_formatting();
        host_function_storage_neutral_callback_entry_adopts_existing_records();
        host_function_storage_registry_does_not_join_a_different_document();
        host_function_storage_primitive_conversion_and_callback_errors_are_preserved();
        host_function_storage_public_indices_metadata_and_duplicate_rejection_are_preserved();
        host_function_storage_growth_releases_old_backing_and_retains_every_callback();
        host_function_storage_callback_destructor_observes_live_box_and_slot_credit();
        host_function_storage_failed_publication_releases_prepared_callback_and_slots();
        host_function_storage_terminal_preparation_stops_zero_sized_box_construction();
        host_function_storage_zero_sized_callbacks_own_only_the_real_registry_backing();
        host_function_storage_manual_timer_promise_and_major_gc_preserve_indices();
    }
}
