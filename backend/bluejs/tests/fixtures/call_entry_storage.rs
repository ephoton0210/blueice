// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Small reserved vectors check native call-entry ownership and admission.

use super::*;

fn execute(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}

fn observed_vm() -> (Vm, Rc<RefCell<Vec<usize>>>) {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.remaining_instructions = 100_000;
    let observed = Rc::new(RefCell::new(Vec::new()));
    let output = observed.clone();
    let counter = vm.displaced_execution_control_bytes.clone();
    vm.install_host_function("observe", 0, move |_args: &[HostValue]| {
        output.borrow_mut().push(counter.get());
        Ok(HostValue::Undefined)
    })
    .unwrap();
    (vm, observed)
}

#[cfg_attr(test, test)]
fn call_entry_storage_reserved_arguments_remain_charged_during_native_callbacks() {
    let (mut vm, observed) = observed_vm();
    let callback = vm.lookup_global_name("observe").unwrap().unwrap();
    let args = Vec::with_capacity(32);
    let bytes = args.capacity() * std::mem::size_of::<Value>();
    vm.call_native(callback, Value::Undefined, args, false)
        .unwrap();
    assert_eq!(*observed.borrow(), vec![bytes]);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn call_entry_storage_refuses_before_native_callback_or_frame_publication() {
    let (mut vm, observed) = observed_vm();
    let callback = vm.lookup_global_name("observe").unwrap().unwrap();
    let args = Vec::with_capacity(32);
    let bytes = args.capacity() * std::mem::size_of::<Value>();
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    let _peer = vm
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - bytes + 1)
        .unwrap();
    assert_eq!(
        vm.call_native(callback, Value::Undefined, args, false),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(observed.borrow().is_empty());
    assert_eq!(vm.call_depth, 0);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn call_entry_storage_constructor_callbacks_retain_reserved_argument_capacity() {
    let (mut vm, observed) = observed_vm();
    let constructor = execute(&mut vm, "class C {field=observe();} C;");
    vm.call_native(constructor.clone(), Value::Undefined, Vec::new(), true)
        .unwrap();
    let args = Vec::with_capacity(32);
    let bytes = args.capacity() * std::mem::size_of::<Value>();
    vm.call_native(constructor, Value::Undefined, args, true)
        .unwrap();
    assert_eq!(observed.borrow()[1] - observed.borrow()[0], bytes);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn call_entry_storage_host_conversion_owns_its_vector_and_string_copy() {
    let (mut vm, observed) = observed_vm();
    let callback = vm.lookup_global_name("observe").unwrap().unwrap();
    vm.call_native(
        callback,
        Value::Undefined,
        vec![Value::String("tiny".into())],
        false,
    )
    .unwrap();
    // The input Vec stays in the call-entry counter. Converted primitive
    // copies now have independent native credit (host_marshalling_storage
    // checks that credit at callback entry and after VM destruction).
    assert_eq!(*observed.borrow(), vec![std::mem::size_of::<Value>()]);
}

#[cfg_attr(test, test)]
fn call_entry_storage_bound_arguments_use_one_final_buffer_and_conversion_owner() {
    let (mut vm, observed) = observed_vm();
    execute(&mut vm, "observe.bind(null,1,2).bind(null,3)(4);0;");
    assert_eq!(*observed.borrow(), vec![4 * std::mem::size_of::<Value>()]);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

impl crate::Vm {
    /// Runs call entry storage contracts in unit and coverage builds.
    #[doc(hidden)]
    pub fn verify_call_entry_storage_contracts() {
        call_entry_storage_reserved_arguments_remain_charged_during_native_callbacks();
        call_entry_storage_refuses_before_native_callback_or_frame_publication();
        call_entry_storage_constructor_callbacks_retain_reserved_argument_capacity();
        call_entry_storage_host_conversion_owns_its_vector_and_string_copy();
        call_entry_storage_bound_arguments_use_one_final_buffer_and_conversion_owner();
        call_entry_storage_parameter_handoff_does_not_charge_the_argument_vector_twice();
        call_entry_storage_proxy_forwarding_preserves_one_argument_owner();
        call_entry_storage_host_conversion_refuses_before_callback();
        call_entry_storage_bound_flattening_refuses_before_callback();
        call_entry_storage_tail_calls_reuse_the_carrier_allocation();
    }
}

#[cfg_attr(test, test)]
fn call_entry_storage_parameter_handoff_does_not_charge_the_argument_vector_twice() {
    for kind in ["function", "function*", "async function", "async function*"] {
        let (mut vm, observed) = observed_vm();
        let callee = execute(&mut vm, &format!("({kind}(checked=observe()){{}});"));
        vm.call_native(callee.clone(), Value::Undefined, Vec::new(), false)
            .unwrap();
        vm.call_native(callee, Value::Undefined, Vec::with_capacity(32), false)
            .unwrap();
        assert_eq!(observed.borrow().len(), 2);
        assert_eq!(observed.borrow()[0], observed.borrow()[1], "{kind}");
        for _ in 0..16 {
            if !vm.run_next_promise_job().unwrap() {
                break;
            }
        }
        assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    }
}

#[cfg_attr(test, test)]
fn call_entry_storage_proxy_forwarding_preserves_one_argument_owner() {
    for source in [
        "observe;",
        "new Proxy(observe,{});",
        "new Proxy(new Proxy(observe,{}),{});",
    ] {
        let (mut vm, observed) = observed_vm();
        let callee = execute(&mut vm, source);
        let args = Vec::with_capacity(32);
        let bytes = args.capacity() * std::mem::size_of::<Value>();
        vm.call_native(callee, Value::Undefined, args, false)
            .unwrap();
        assert_eq!(*observed.borrow(), vec![bytes], "{source}");
        assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    }
}

#[cfg_attr(test, test)]
fn call_entry_storage_host_conversion_refuses_before_callback() {
    let (mut vm, observed) = observed_vm();
    let callee = vm.lookup_global_name("observe").unwrap().unwrap();
    vm.stack.reserve(32);
    let args = vec![Value::String("tiny".into())];
    let bytes = host_scheduling::slots(&args) + host_scheduling::values_bytes(&args);
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    let _peer = vm
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - bytes)
        .unwrap();
    assert_eq!(
        vm.call_native(callee, Value::Undefined, args, false),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(observed.borrow().is_empty());
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn call_entry_storage_bound_flattening_refuses_before_callback() {
    let (mut vm, observed) = observed_vm();
    let callee = execute(&mut vm, "observe.bind(null,1,2).bind(null,3);");
    vm.stack.reserve(32);
    let args = vec![Value::Number(4.0)];
    let room = host_scheduling::slots(&args)
        + 4 * std::mem::size_of::<ObjectId>()
        + 4 * std::mem::size_of::<Value>()
        - 1;
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    let _peer = vm
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - room)
        .unwrap();
    assert_eq!(
        vm.call_native(callee, Value::Undefined, args, false),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(observed.borrow().is_empty());
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn call_entry_storage_tail_calls_reuse_the_carrier_allocation() {
    let (mut vm, observed) = observed_vm();
    execute(
        &mut vm,
        "function f(a,b){'use strict';return observe(a,b);} f(1,2);0;",
    );
    assert_eq!(*observed.borrow(), vec![4 * std::mem::size_of::<Value>()]);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}
