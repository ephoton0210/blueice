// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny ImportCall cases observe native admission across argument callbacks.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}
fn execute(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}
fn leave_room(vm: &mut Vm, room: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - room)
        .unwrap()
}

#[cfg(test)]
#[test]
fn import_call_storage_refuses_before_input_copy_or_root_allocation() {
    let mut vm = page();
    let specifier = Value::String("./tiny.mjs".into());
    let _peer = leave_room(&mut vm, 0);
    let (result, allocations) = allocation_observer::count(|| {
        vm.dynamic_import(specifier, Value::Undefined, ImportPhase::Evaluation)
    });
    assert_eq!(allocations, 0);
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
}

#[cfg_attr(test, test)]
fn import_call_storage_refusal_preserves_operand_capacity_and_promises() {
    let mut vm = page();
    let specifier = Value::String("./tiny.mjs".into());
    let before = (
        vm.stack.capacity(),
        vm.promises.len(),
        vm.async_frame_roots.len(),
    );
    let _peer = leave_room(&mut vm, 0);
    let result = vm.dynamic_import(specifier, Value::Undefined, ImportPhase::Evaluation);
    assert_eq!(
        (
            vm.stack.capacity(),
            vm.promises.len(),
            vm.async_frame_roots.len()
        ),
        before
    );
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(vm.promise_jobs.is_empty());
}

#[cfg_attr(test, test)]
fn import_call_storage_utf8_specifier_remains_charged_during_options_getter() {
    let mut vm = page();
    let observed = Rc::new(RefCell::new(Vec::new()));
    let output = observed.clone();
    let view = Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    };
    vm.install_host_function("observe", 0, move |_args: &[HostValue]| {
        output
            .borrow_mut()
            .push(view.host_scheduling_snapshot().payload_bytes);
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let options = execute(&mut vm, "var options={get with(){observe();}}; options;");
    drop(
        vm.evaluate_import_call_arguments(Value::String("warm".into()), options.clone())
            .unwrap(),
    );
    observed.borrow_mut().clear();
    for text in ["ab", "aπ😀"] {
        drop(
            vm.evaluate_import_call_arguments(Value::String(text.into()), options.clone())
                .unwrap(),
        );
    }
    let values = observed.borrow();
    assert_eq!(values.len(), 2);
    assert_eq!(
        values[1] as isize - values[0] as isize,
        5,
        "observed={values:?}"
    );
}

#[cfg_attr(test, test)]
fn import_call_storage_input_owner_counts_actual_padded_capacity() {
    let mut vm = page();
    vm.prepare_async_frame_root_slots(4).unwrap();
    let mut units = Vec::with_capacity(19);
    units.extend([97, 98]);
    let specifier = Value::String(JsString::from_code_units(units));
    let bytes = host_scheduling::value_payload_bytes(&specifier);
    assert!(bytes > 4);
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let owner = vm
        .prepare_import_call_inputs(&specifier, &Value::Undefined)
        .unwrap();
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes - before, bytes);
    drop(owner);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn import_call_storage_prepared_root_slots_need_no_extra_quota() {
    let mut vm = page();
    vm.prepare_async_frame_root_slots(4).unwrap();
    let _peer = leave_room(&mut vm, 0);
    let owner = vm
        .prepare_import_call_inputs(&Value::Null, &Value::Undefined)
        .unwrap();
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
    drop(owner);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}

#[cfg_attr(test, test)]
fn import_call_storage_utf8_exact_and_one_short_admission() {
    let text = JsString::from("aπ😀");
    for accepts in [false, true] {
        let mut vm = page();
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        let peer = leave_room(&mut vm, 7 - usize::from(!accepts));
        let result = ModuleName::from_utf16(&text, vm.host_scheduling.as_ref());
        if accepts {
            let name = result.unwrap();
            assert_eq!(name.as_str(), "aπ😀");
            assert_eq!(name.bytes(), 7);
            assert_eq!(
                vm.host_scheduling_snapshot().payload_bytes,
                MAX_HOST_SCHEDULING_BYTES
            );
            drop(name);
        } else {
            assert_eq!(result.err(), Some(RuntimeError::SchedulingLimit));
        }
        drop(peer);
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    }
}

#[cfg_attr(test, test)]
fn import_call_storage_utf8_empty_and_unpaired_surrogate() {
    let vm = page();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let empty = ModuleName::from_utf16(&JsString::default(), vm.host_scheduling.as_ref()).unwrap();
    assert!(empty.is_empty());
    assert_eq!(empty.bytes(), 0);
    let invalid = JsString::from_code_units(vec![0xd800]);
    assert!(matches!(
        ModuleName::from_utf16(&invalid, vm.host_scheduling.as_ref()),
        Err(RuntimeError::TypeError(_))
    ));
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    vm.check_scheduling_failure().unwrap();
}

#[cfg_attr(test, test)]
fn import_call_storage_utf8_owner_outlives_its_vm() {
    let vm = page();
    let view = Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    };
    let name =
        ModuleName::from_utf16(&JsString::from("aπ😀"), vm.host_scheduling.as_ref()).unwrap();
    let bytes = name.bytes();
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, bytes);
    drop(name);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

use crate::vm::import_call_storage::ImportAttributeKeys;

#[cfg_attr(test, test)]
fn import_call_storage_attribute_keys_own_backing_and_names_after_vm_drop() {
    let vm = page();
    let view = Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    };
    let mut keys = Vec::with_capacity(7);
    let mut units = Vec::with_capacity(11);
    units.extend([97, 98]);
    keys.push(PropertyName::String(JsString::from_code_units(units)));
    keys.push(PropertyName::Symbol(JsSymbol::new(Some("tiny".into()))));
    let expected = keys.capacity() * std::mem::size_of::<PropertyName>() + 22 + 8;
    let owner = ImportAttributeKeys::new(keys, vm.host_scheduling.as_ref()).unwrap();
    assert_eq!(owner.bytes(), expected);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, expected);
    drop(owner);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn import_call_storage_attribute_key_adoption_refuses_without_extra_headroom() {
    let mut vm = page();
    let keys = vec![PropertyName::from("type")];
    let _peer = leave_room(&mut vm, 0);
    let result = ImportAttributeKeys::new(keys, vm.host_scheduling.as_ref());
    assert_eq!(result.err(), Some(RuntimeError::SchedulingLimit));
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}

#[cfg_attr(test, test)]
fn import_call_storage_public_callbacks_preserve_order_and_roots_through_gc() {
    let mut vm = page();
    execute(&mut vm, "function placeholder(){}; 0;");
    let global = vm.global("globalThis").unwrap().object_id().unwrap();
    let prototype = vm.function_prototype().unwrap();
    vm.install_native(
        global,
        prototype,
        "collectNow",
        0,
        NativeFunction::Test262("gc"),
    )
    .unwrap();
    let module =
        crate::compile_module(&crate::parse_module("export const value=7;").unwrap()).unwrap();
    vm.set_module_loader_context(
        "test/main.js",
        HashMap::from([("test/aπ😀.mjs".into(), module)]),
    );
    execute(
        &mut vm,
        r#"
        var trace=[], result=0;
        import({[Symbol.toPrimitive](){trace.push('specifier'); collectNow(); return './aπ😀.mjs';}}, {
            get with(){trace.push('with'); return new Proxy({extra:'ignored'}, {
                ownKeys(){trace.push('keys'); collectNow(); return ['extra'];},
                getOwnPropertyDescriptor(){trace.push('descriptor'); collectNow(); return {enumerable:true, configurable:true};},
                get(target,key){trace.push('value'); collectNow(); return target[key];}
            });}
        }).then(ns=>{trace.push('fulfilled'); result=ns.value;}); 0;
    "#,
    );
    vm.run_promise_jobs_bounded(32).unwrap();
    assert_eq!(
        execute(&mut vm, "trace.join(',');"),
        Value::String("specifier,with,keys,descriptor,value,fulfilled".into())
    );
    assert_eq!(execute(&mut vm, "result;"), Value::Number(7.0));
    assert!(vm.async_frame_roots.is_empty());
}

#[cfg_attr(test, test)]
fn import_call_storage_public_language_errors_reject_with_original_identity() {
    let mut vm = page();
    execute(&mut vm, "var sentinel={}, caught=false, synchronous=false; try { import('./tiny.mjs',{get with(){throw sentinel;}}).catch(e=>caught=e===sentinel); } catch(e){synchronous=true;} 0;");
    vm.run_promise_jobs_bounded(16).unwrap();
    assert_eq!(
        execute(&mut vm, "caught && !synchronous;"),
        Value::Bool(true)
    );
    assert!(vm.async_frame_roots.is_empty());
}

#[cfg_attr(test, test)]
fn import_call_storage_uncatchable_callback_abort_releases_temporary_roots() {
    let mut vm = page();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    vm.install_host_function("abortDocument", 0, move |_args: &[HostValue]| {
        scope.fail();
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let code = crate::compile(
        &crate::parse("import('./tiny.mjs',{get with(){abortDocument();}});").unwrap(),
    )
    .unwrap();
    assert_eq!(vm.execute_script(&code), Err(RuntimeError::SchedulingLimit));
    assert!(vm.async_frame_roots.is_empty());
    assert!(vm.promise_jobs.is_empty());
}

#[cfg_attr(test, test)]
fn import_call_storage_job_count_refusal_is_returned_to_the_caller() {
    let mut vm = page();
    let mut peer = vm.host_scheduling.as_ref().unwrap().child();
    peer.check(
        HostSchedulingUsage {
            pending_microtasks: MAX_PENDING_HOST_MICROTASKS,
            ..Default::default()
        },
        HostSchedulingUsage::default(),
    )
    .unwrap();
    let result = vm.dynamic_import(
        Value::String("./tiny.mjs".into()),
        Value::Undefined,
        ImportPhase::Evaluation,
    );
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(vm.async_frame_roots.is_empty());
    assert!(vm.promise_jobs.is_empty());
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_import_call_storage_contracts() {
        import_call_storage_refusal_preserves_operand_capacity_and_promises();
        import_call_storage_utf8_specifier_remains_charged_during_options_getter();
        import_call_storage_input_owner_counts_actual_padded_capacity();
        import_call_storage_prepared_root_slots_need_no_extra_quota();
        import_call_storage_utf8_exact_and_one_short_admission();
        import_call_storage_utf8_empty_and_unpaired_surrogate();
        import_call_storage_utf8_owner_outlives_its_vm();
        import_call_storage_attribute_keys_own_backing_and_names_after_vm_drop();
        import_call_storage_attribute_key_adoption_refuses_without_extra_headroom();
        import_call_storage_public_callbacks_preserve_order_and_roots_through_gc();
        import_call_storage_public_language_errors_reject_with_original_identity();
        import_call_storage_uncatchable_callback_abort_releases_temporary_roots();
        import_call_storage_job_count_refusal_is_returned_to_the_caller();
    }
}
