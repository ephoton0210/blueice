// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny callback boundaries retain primitive copies and move native errors.

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

fn observe(view: &Vm, before: &Cell<usize>, seen: &Cell<usize>, args: &[HostValue]) {
    assert!(matches!(args, [HostValue::String(text), HostValue::Number(7.0)] if text == "tiny"));
    seen.set(view.host_scheduling_snapshot().payload_bytes - before.get());
}

fn install_observers(
    vm: &mut Vm,
    view: Rc<Vm>,
    before: Rc<Cell<usize>>,
    seen: Rc<Cell<usize>>,
) -> Result<ObjectId, RuntimeError> {
    let family = vm.create_host_object_family()?;
    let (output, baseline, used) = (view.clone(), before.clone(), seen.clone());
    vm.install_host_function("observe", 2, move |args: &[HostValue]| {
        observe(&output, &baseline, &used, args);
        Ok(HostValue::Number(7.0))
    })?;
    let (output, baseline, used) = (view.clone(), before.clone(), seen.clone());
    vm.install_host_object_factory("target", 2, family, move |args: &[HostValue]| {
        observe(&output, &baseline, &used, args);
        Ok(None)
    })?;
    vm.install_host_object_method(
        family,
        "observe",
        2,
        move |_: HostObjectKey, args: &[HostValue]| {
            observe(&view, &before, &seen, args);
            Ok(HostValue::Number(7.0))
        },
    )?;
    vm.mint_host_family_wrapper(family, HostObjectKey::new(7, 4, 1))
}

fn charged_inputs(kind: u8, borrowed: bool) {
    let source = page();
    let mut vm = if borrowed { Vm::default() } else { page() };
    let view = Rc::new(observer(if borrowed { &source } else { &vm }));
    let before = Rc::new(Cell::new(0));
    let seen = Rc::new(Cell::new(0));
    let object = if borrowed {
        loan(&mut vm, &source, |vm| {
            install_observers(vm, view.clone(), before.clone(), seen.clone())
        })
    } else {
        install_observers(&mut vm, view.clone(), before.clone(), seen.clone())
    }
    .unwrap();
    let call = |vm: &mut Vm| {
        before.set(view.host_scheduling_snapshot().payload_bytes);
        let args = [Value::String("tiny".into()), Value::Number(7.0)];
        match kind {
            0 => vm.host_function_call(0, Value::Undefined, &args, false),
            1 => vm.host_object_factory_call(0, Value::Undefined, &args, false),
            2 => vm.host_object_method_call(0, Value::Object(object), &args, false),
            _ => unreachable!(),
        }
    };
    let result = if borrowed {
        loan(&mut vm, &source, call)
    } else {
        call(&mut vm)
    };
    assert_eq!(
        result,
        Ok(if kind == 1 {
            Value::Null
        } else {
            Value::Number(7.0)
        })
    );
    assert!(
        seen.get() >= 2 * std::mem::size_of::<HostValue>() + 8,
        "native copies observed: {}",
        seen.get()
    );
    assert!(vm.realm_call_roots.is_none());
    vm.synchronize_scheduling_usage().unwrap();
}

#[cfg_attr(test, test)]
fn host_marshalling_storage_borrowed_function_owns_copied_arguments() {
    charged_inputs(0, true);
}
#[cfg_attr(test, test)]
fn host_marshalling_storage_scoped_factory_owns_copied_arguments() {
    charged_inputs(1, false);
}
#[cfg_attr(test, test)]
fn host_marshalling_storage_borrowed_factory_owns_copied_arguments() {
    charged_inputs(1, true);
}
#[cfg_attr(test, test)]
fn host_marshalling_storage_scoped_method_owns_copied_arguments() {
    charged_inputs(2, false);
}
#[cfg_attr(test, test)]
fn host_marshalling_storage_borrowed_method_owns_copied_arguments() {
    charged_inputs(2, true);
}
#[cfg_attr(test, test)]
fn host_marshalling_storage_scoped_function_preserves_argument_ownership() {
    charged_inputs(0, false);
}

fn error_moves(kind: u8) {
    let mut vm = page();
    let family = vm.create_host_object_family().unwrap();
    let object = vm
        .mint_host_family_wrapper(family, HostObjectKey::new(7, 4, 1))
        .unwrap();
    let message = String::from("tiny error");
    let pointer = message.as_ptr() as usize;
    let mut error = Some(HostFunctionError::new(message));
    let result = match kind {
        0 => {
            vm.install_host_function("reject", 0, move |_: &[HostValue]| {
                Err(error.take().unwrap())
            })
            .unwrap();
            vm.host_function_call(0, Value::Undefined, &[], false)
        }
        1 => {
            vm.install_host_object_factory("reject", 0, family, move |_: &[HostValue]| {
                Err(error.take().unwrap())
            })
            .unwrap();
            vm.host_object_factory_call(0, Value::Undefined, &[], false)
        }
        2 => {
            vm.install_host_object_method(
                family,
                "reject",
                0,
                move |_: HostObjectKey, _: &[HostValue]| Err(error.take().unwrap()),
            )
            .unwrap();
            vm.host_object_method_call(0, Value::Object(object), &[], false)
        }
        3 => {
            vm.install_host_object_pair_method(
                family,
                "reject",
                1,
                move |_: HostObjectKey, _: HostObjectKey| Err(error.take().unwrap()),
            )
            .unwrap();
            vm.host_object_pair_method_call(
                0,
                Value::Object(object),
                &[Value::Object(object)],
                false,
            )
        }
        _ => unreachable!(),
    };
    let Err(RuntimeError::TypeError(message)) = result else {
        panic!("native error");
    };
    assert_eq!(message, "tiny error");
    assert_eq!(
        message.as_ptr() as usize,
        pointer,
        "move the existing message without a second allocation"
    );
}

#[cfg_attr(test, test)]
fn host_marshalling_storage_function_moves_the_native_error_buffer() {
    error_moves(0);
}
#[cfg_attr(test, test)]
fn host_marshalling_storage_factory_moves_the_native_error_buffer() {
    error_moves(1);
}
#[cfg_attr(test, test)]
fn host_marshalling_storage_method_moves_the_native_error_buffer() {
    error_moves(2);
}
#[cfg_attr(test, test)]
fn host_marshalling_storage_pair_moves_the_native_error_buffer() {
    error_moves(3);
}

#[cfg_attr(test, test)]
fn host_marshalling_storage_borrowed_preparation_outlives_the_vm() {
    let source = page();
    let view = observer(&source);
    let before = view.host_scheduling_snapshot().payload_bytes;
    let mut vm = Vm::default();
    let prepared = loan(&mut vm, &source, |vm| {
        vm.prepare_host_wait_arguments(&[Value::String("tiny".into())])
    })
    .unwrap();
    let bytes = prepared.values.capacity() * std::mem::size_of::<HostValue>() + 8;
    drop(vm);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        before + bytes
    );
    drop(prepared);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn host_marshalling_storage_borrowed_invalid_input_retains_its_error_owner() {
    let source = page();
    let view = observer(&source);
    let before = view.host_scheduling_snapshot().payload_bytes;
    let mut vm = Vm::default();
    let object = Value::Object(vm.object_prototype);
    let prepared = loan(&mut vm, &source, |vm| {
        vm.prepare_host_wait_arguments(&[Value::String("tiny".into()), object])
    })
    .unwrap();
    assert!(prepared.values.is_empty());
    assert_eq!(prepared.values.capacity(), 0);
    let Some(RuntimeError::TypeError(message)) = &prepared.error else {
        panic!("conversion error");
    };
    let bytes = message.capacity();
    drop(vm);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        before + bytes
    );
    drop(prepared);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn host_marshalling_storage_unsupported_input_precedes_callback_entry() {
    let mut vm = page();
    let called = Rc::new(Cell::new(false));
    let output = called.clone();
    vm.install_host_function("observe", 1, move |_: &[HostValue]| {
        output.set(true);
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let result = vm.host_function_call(
        0,
        Value::Undefined,
        &[Value::Object(vm.object_prototype)],
        false,
    );
    assert!(
        matches!(result, Err(RuntimeError::TypeError(message)) if message == "host functions accept primitive values only")
    );
    assert!(!called.get());
}

#[cfg_attr(test, test)]
fn host_marshalling_storage_public_callable_values_are_preserved() {
    let mut vm = page();
    vm.install_host_function("echo", 1, |args: &[HostValue]| Ok(args[0].clone()))
        .unwrap();
    let code = crate::compile(&crate::parse("echo(null)===null&&echo(undefined)===undefined&&echo(true)&&echo(7)===7&&echo('tiny')==='tiny';").unwrap()).unwrap();
    assert_eq!(vm.execute_script(&code), Ok(Value::Bool(true)));
}

#[cfg_attr(test, test)]
fn host_marshalling_storage_scoped_preparation_keeps_credit_after_vm_drop() {
    let mut vm = page();
    let view = observer(&vm);
    let prepared = vm
        .prepare_host_arguments(&[Value::String("tiny".into())], None)
        .unwrap();
    let bytes = prepared.values.capacity() * std::mem::size_of::<HostValue>() + 8;
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(prepared);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn host_marshalling_storage_caller_owns_transient_copies_beside_another_registry_owner() {
    let source = page();
    let caller = page();
    let first = Rc::new(observer(&source));
    let second = Rc::new(observer(&caller));
    let baseline_a = Rc::new(Cell::new(0));
    let baseline_b = Rc::new(Cell::new(0));
    let (view_a, view_b, before_a, before_b) = (
        first.clone(),
        second.clone(),
        baseline_a.clone(),
        baseline_b.clone(),
    );
    let mut vm = Vm::default();
    loan(&mut vm, &source, |vm| {
        vm.install_host_function("observe", 1, move |args: &[HostValue]| {
            assert!(matches!(args, [HostValue::String(text)] if text == "tiny"));
            assert_eq!(
                view_a.host_scheduling_snapshot().payload_bytes,
                before_a.get()
            );
            assert!(
                view_b.host_scheduling_snapshot().payload_bytes
                    >= before_b.get() + std::mem::size_of::<HostValue>() + 8
            );
            Ok(HostValue::Undefined)
        })
    })
    .unwrap();
    let result = loan(&mut vm, &caller, |vm| {
        baseline_a.set(first.host_scheduling_snapshot().payload_bytes);
        baseline_b.set(second.host_scheduling_snapshot().payload_bytes);
        let result =
            vm.host_function_call(0, Value::Undefined, &[Value::String("tiny".into())], false);
        assert_eq!(
            second.host_scheduling_snapshot().payload_bytes,
            baseline_b.get()
        );
        result
    });
    assert_eq!(result, Ok(Value::Undefined));
    assert!(source.argument_list_owner().is_ok() && caller.argument_list_owner().is_ok());
}

#[cfg_attr(test, test)]
fn host_marshalling_storage_retained_registration_supplies_a_neutral_entry_owner() {
    let source = page();
    let view = Rc::new(observer(&source));
    let baseline = Rc::new(Cell::new(0));
    let (output, before) = (view.clone(), baseline.clone());
    let mut vm = Vm::default();
    loan(&mut vm, &source, |vm| {
        vm.install_host_function("observe", 1, move |args: &[HostValue]| {
            assert!(matches!(args, [HostValue::String(text)] if text == "tiny"));
            assert!(
                output.host_scheduling_snapshot().payload_bytes
                    >= before.get() + std::mem::size_of::<HostValue>() + 8
            );
            Ok(HostValue::Undefined)
        })
    })
    .unwrap();
    baseline.set(view.host_scheduling_snapshot().payload_bytes);
    assert_eq!(
        vm.host_function_call(0, Value::Undefined, &[Value::String("tiny".into())], false),
        Ok(Value::Undefined)
    );
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        baseline.get()
    );
}

#[cfg_attr(test, test)]
fn host_marshalling_storage_empty_arguments_retain_terminal_owner_identity() {
    let source = page();
    let mut vm = Vm::default();
    let prepared = loan(&mut vm, &source, |vm| vm.prepare_host_wait_arguments(&[])).unwrap();
    assert_eq!(prepared.values.capacity(), 0);
    assert!(prepared
        .owner_view()
        .unwrap()
        .unwrap()
        .belongs_to(source.host_scheduling.as_ref().unwrap()));
    let _ = source.host_scheduling.as_ref().unwrap().fail();
    assert!(matches!(
        prepared.owner_view(),
        Err(RuntimeError::SchedulingLimit)
    ));
}

#[cfg_attr(test, test)]
fn host_marshalling_storage_terminal_borrowed_preparation_stops_before_conversion() {
    let source = page();
    let mut vm = Vm::default();
    let result = loan(&mut vm, &source, |vm| {
        let _ = source.host_scheduling.as_ref().unwrap().fail();
        vm.prepare_host_wait_arguments(&[Value::String("tiny".into())])
    });
    assert!(matches!(result, Err(RuntimeError::SchedulingLimit)));
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn host_marshalling_storage_utf16_copy_reconciles_unused_input_capacity() {
    let mut vm = page();
    let view = observer(&vm);
    let mut units = Vec::with_capacity(8);
    units.extend_from_slice(&[0xd800, 0x0078, 0xdc00]);
    let input = Value::String(JsString::from_code_units(units));
    let prepared = vm.prepare_host_wait_arguments(&[input]).unwrap();
    let [HostValue::String(text)] = prepared.values.as_slice() else {
        panic!("copied string");
    };
    assert_eq!(text.as_code_units(), &[0xd800, 0x0078, 0xdc00]);
    let bytes = prepared.values.capacity() * std::mem::size_of::<HostValue>()
        + text.owned_heap_payload_bytes().unwrap();
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(prepared);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn host_marshalling_storage_nested_preparation_restores_the_existing_root_prefix() {
    let source = page();
    let caller = page();
    let mut vm = Vm::default();
    let owner = caller.argument_list_owner().unwrap();
    let prefix = realm_call_storage::RealmCallRoots::new(0, owner.as_ref()).unwrap();
    vm.with_realm_call_roots(prefix.clone(), |vm| {
        let prepared = loan(vm, &source, |vm| {
            vm.prepare_host_wait_arguments(&[Value::String("tiny".into())])
        })?;
        assert!(Rc::ptr_eq(vm.realm_call_roots.as_ref().unwrap(), &prefix));
        let _ = source.host_scheduling.as_ref().unwrap().fail();
        assert!(matches!(
            prepared.owner_view(),
            Err(RuntimeError::SchedulingLimit)
        ));
        drop(prepared);
        assert!(caller.argument_list_owner().is_ok());
        Ok(())
    })
    .unwrap();
    assert!(vm.realm_call_roots.is_none());
}

#[cfg_attr(test, test)]
fn host_marshalling_storage_returned_utf16_buffer_moves_without_a_copy() {
    let mut vm = page();
    let units = vec![0xd800, 0x0078, 0xdc00];
    let pointer = units.as_ptr() as usize;
    let mut value = Some(HostValue::String(JsString::from_code_units(units)));
    vm.install_host_function("read", 0, move |_: &[HostValue]| Ok(value.take().unwrap()))
        .unwrap();
    let Value::String(text) = vm
        .host_function_call(0, Value::Undefined, &[], false)
        .unwrap()
    else {
        panic!("returned string");
    };
    assert_eq!(text.as_code_units(), &[0xd800, 0x0078, 0xdc00]);
    assert_eq!(text.as_code_units().as_ptr() as usize, pointer);
}

fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}

#[cfg_attr(test, test)]
fn host_marshalling_storage_manual_timer_promise_and_major_gc_keep_primitive_values() {
    let mut vm = page();
    let family = vm.create_host_object_family().unwrap();
    vm.install_host_object_factory("target", 1, family, |args: &[HostValue]| {
        let [HostValue::Number(id)] = args else {
            return Ok(None);
        };
        Ok(Some(HostObjectKey::new(7, 4, *id as u64)))
    })
    .unwrap();
    vm.install_host_object_method(family, "echo", 1, |_: HostObjectKey, args: &[HostValue]| {
        Ok(args[0].clone())
    })
    .unwrap();
    vm.install_host_function("echo", 1, |args: &[HostValue]| Ok(args[0].clone()))
        .unwrap();
    vm.install_test262_harness().unwrap();
    run(&mut vm,"delete globalThis.setTimeout;delete globalThis.clearTimeout;delete globalThis.setInterval;delete globalThis.clearInterval;delete globalThis.queueMicrotask;0;");
    vm.install_host_timer_functions().unwrap();
    run(&mut vm,"var result=false;setTimeout(()=>Promise.resolve(echo('tiny')).then(value=>{$262.gc();result=value==='tiny'&&target(1).echo(value)===value&&target(1)===target(1);}),1);0;");
    let turn = vm.run_due_host_timers(1).unwrap();
    assert_eq!(turn.tasks_run, 1);
    assert_eq!(turn.uncaught_errors, 0);
    assert_eq!(vm.next_host_timer_due(), None);
    assert_eq!(run(&mut vm, "result;"), Value::Bool(true));
    assert!(vm.realm_call_roots.is_none() && vm.stack.is_empty());
}

#[cfg_attr(test, test)]
fn host_marshalling_storage_cross_realm_calls_preserve_values_through_gc() {
    let mut vm = page();
    vm.install_host_function("echo", 1, |args: &[HostValue]| Ok(args[0].clone()))
        .unwrap();
    vm.install_test262_harness().unwrap();
    assert_eq!(run(&mut vm, "var child=$262.createRealm();child.global.echo=echo;child.evalScript(\"var value=echo('tiny');$262.gc();echo(value)===value;\");"), Value::Bool(true));
    assert!(vm.realm_call_roots.is_none() && vm.stack.is_empty());
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_host_marshalling_storage_contracts() {
        host_marshalling_storage_borrowed_function_owns_copied_arguments();
        host_marshalling_storage_scoped_factory_owns_copied_arguments();
        host_marshalling_storage_borrowed_factory_owns_copied_arguments();
        host_marshalling_storage_scoped_method_owns_copied_arguments();
        host_marshalling_storage_borrowed_method_owns_copied_arguments();
        host_marshalling_storage_scoped_function_preserves_argument_ownership();
        host_marshalling_storage_function_moves_the_native_error_buffer();
        host_marshalling_storage_factory_moves_the_native_error_buffer();
        host_marshalling_storage_method_moves_the_native_error_buffer();
        host_marshalling_storage_pair_moves_the_native_error_buffer();
        host_marshalling_storage_borrowed_preparation_outlives_the_vm();
        host_marshalling_storage_borrowed_invalid_input_retains_its_error_owner();
        host_marshalling_storage_unsupported_input_precedes_callback_entry();
        host_marshalling_storage_public_callable_values_are_preserved();
        host_marshalling_storage_scoped_preparation_keeps_credit_after_vm_drop();
        host_marshalling_storage_caller_owns_transient_copies_beside_another_registry_owner();
        host_marshalling_storage_retained_registration_supplies_a_neutral_entry_owner();
        host_marshalling_storage_empty_arguments_retain_terminal_owner_identity();
        host_marshalling_storage_terminal_borrowed_preparation_stops_before_conversion();
        host_marshalling_storage_utf16_copy_reconciles_unused_input_capacity();
        host_marshalling_storage_nested_preparation_restores_the_existing_root_prefix();
        host_marshalling_storage_returned_utf16_buffer_moves_without_a_copy();
        host_marshalling_storage_manual_timer_promise_and_major_gc_keep_primitive_values();
        host_marshalling_storage_cross_realm_calls_preserve_values_through_gc();
    }
}
