// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny module frames preserve admitted names and the original ambient owner.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn compile_module(source: &str) -> Bytecode {
    crate::compile_module(&crate::parse_module(source).unwrap()).unwrap()
}

fn bounded_vm() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.remaining_instructions = 100_000;
    vm
}

fn caller_name(vm: &mut Vm) -> (*const u8, usize) {
    let mut name = String::with_capacity(64);
    name.push_str("caller");
    let result = (name.as_ptr(), name.capacity());
    vm.active_module_name = Some(name);
    result
}

fn leave_room(vm: &mut Vm, room: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - room)
        .unwrap()
}

#[cfg_attr(test, test)]
fn module_context_ownership_instantiation_refuses_before_replacing_caller_fields() {
    let mut vm = bounded_vm();
    let (pointer, capacity) = caller_name(&mut vm);
    vm.stack = Vec::with_capacity(16);
    vm.stack.push(Value::Number(7.0));
    let code = compile_module("0;");
    let mut cells = HashMap::new();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.initialize_module_record("page/module.js", &code, &mut cells),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.stack, vec![Value::Number(7.0)]);
    assert_eq!(vm.active_module_name.as_ref().unwrap().as_ptr(), pointer);
    assert_eq!(vm.active_module_name.as_ref().unwrap().capacity(), capacity);
    assert!(cells.is_empty());
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn module_context_ownership_displaced_caller_name_is_charged_during_module_callbacks() {
    let mut vm = bounded_vm();
    let seen = Rc::new(std::cell::Cell::new(0));
    let output = seen.clone();
    let counter = vm.displaced_execution_control_bytes.clone();
    vm.install_host_function("observe", 0, move |_: &[HostValue]| {
        output.set(counter.get());
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let (_, capacity) = caller_name(&mut vm);
    let code = compile_module("observe();");
    vm.execute_module_graph(
        "page/module.js",
        &HashMap::from([("page/module.js".into(), code)]),
    )
    .unwrap();
    assert!(
        seen.get() >= capacity,
        "{} charged for a {capacity}-byte caller",
        seen.get()
    );
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn module_context_ownership_debugger_pause_and_step_move_the_ambient_allocation() {
    let mut vm = bounded_vm();
    let (pointer, capacity) = caller_name(&mut vm);
    let code = compile_module("globalThis.first=1;globalThis.second=2;");
    let target = code.module_evaluate_entry.unwrap();
    let modules = HashMap::from([("page/module.js".into(), code)]);
    assert!(matches!(
        vm.execute_module_graph_until_debugger_pause("page/module.js", &modules, target)
            .unwrap(),
        VmDebuggerExecutionState::Paused { .. }
    ));
    assert_eq!(vm.active_module_name.as_ref().unwrap().as_ptr(), pointer);
    assert_eq!(vm.active_module_name.as_ref().unwrap().capacity(), capacity);
    assert!(matches!(
        vm.step_debugger_module_root_instruction().unwrap(),
        VmDebuggerExecutionState::Paused { .. }
    ));
    assert_eq!(vm.active_module_name.as_ref().unwrap().as_ptr(), pointer);
    assert_eq!(
        vm.resume_debugger_module_execution().unwrap(),
        VmDebuggerExecutionState::Completed
    );
    assert_eq!(vm.active_module_name.as_ref().unwrap().as_ptr(), pointer);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn module_context_ownership_shadow_name_refusal_precedes_promise_capability() {
    let mut vm = bounded_vm();
    let realm = vm
        .execute_script(&crate::compile(&crate::parse("new ShadowRealm();").unwrap()).unwrap())
        .unwrap();
    vm.global("Promise").unwrap();
    let warm = vm.new_promise().unwrap();
    let root = vm.heap.root(warm).unwrap();
    let module = compile_module("export function answer(){return 42;}");
    vm.set_module_loader_context(
        "page/home.js",
        HashMap::from([("page/value.js".into(), module)]),
    );
    vm.stack.reserve(64);
    let count = vm.promises.len();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.shadow_realm_import_value(
            realm,
            Value::String("./value.js".into()),
            Value::String("answer".into())
        ),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(
        vm.promises.len(),
        count,
        "name refusal published a Promise capability"
    );
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    vm.heap.unroot(root).unwrap();
}

#[cfg_attr(test, test)]
fn module_context_ownership_exact_name_budget_and_one_byte_refusal() {
    for accepts in [false, true] {
        let mut vm = bounded_vm();
        let (pointer, capacity) = caller_name(&mut vm);
        let _peer = leave_room(&mut vm, "entry".len() - usize::from(!accepts));
        let before = vm.host_scheduling_snapshot().payload_bytes;
        let result = vm.prepare_module_name("entry");
        if accepts {
            let mut name = result.unwrap();
            assert_eq!(vm.displaced_execution_control_bytes.get(), "entry".len());
            name.swap_into(&mut vm);
            assert_eq!(vm.active_module_name.as_deref(), Some("entry"));
            assert_eq!(vm.displaced_execution_control_bytes.get(), capacity);
            vm.synchronize_scheduling_usage().unwrap();
            name.restore(&mut vm);
        } else {
            assert!(matches!(result, Err(RuntimeError::SchedulingLimit)));
        }
        assert_eq!(vm.active_module_name.as_ref().unwrap().as_ptr(), pointer);
        assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    }
}

#[cfg_attr(test, test)]
fn module_context_ownership_ambient_move_needs_no_new_budget_including_empty_names() {
    for present in [false, true] {
        for empty in [false, true] {
            let mut vm = bounded_vm();
            if present {
                caller_name(&mut vm);
                if empty {
                    vm.active_module_name.as_mut().unwrap().clear();
                }
            }
            let pointer = vm.active_module_name.as_ref().map(|name| name.as_ptr());
            let capacity = vm.active_module_name.as_ref().map_or(0, String::capacity);
            let _peer = leave_room(&mut vm, 0);
            let before = vm.host_scheduling_snapshot().payload_bytes;
            let name = vm.take_module_name();
            assert!(vm.active_module_name.is_none());
            assert_eq!(vm.displaced_execution_control_bytes.get(), capacity);
            vm.synchronize_scheduling_usage().unwrap();
            name.restore(&mut vm);
            assert_eq!(
                vm.active_module_name.as_ref().map(|name| name.as_ptr()),
                pointer
            );
            assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
            assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
        }
    }
}

fn paused_nested(vm: &mut Vm, abrupt: bool) -> u64 {
    let source = if abrupt {
        "function inner(){throw 7;}try{inner();}catch(e){globalThis.answer=e;}"
    } else {
        "function inner(){return 42;}inner();"
    };
    let mut registry = crate::BlueJsProgramRegistry::default();
    let handle = registry
        .install_precompiled(
            crate::BlueJsSourceIdentity::new("page:///name.mjs", "sha256:name").unwrap(),
            compile_module(source),
        )
        .unwrap();
    let code = registry.get(handle).unwrap().bytecode().clone();
    let modules = HashMap::from([("page:///name.mjs".into(), code)]);
    let state = vm
        .execute_module_graph_until_nested_debugger_pause("page:///name.mjs", &modules, 1, 0)
        .unwrap();
    let VmDebuggerNestedExecutionState::Paused { frame_serial, .. } = state else {
        panic!("the selected frame must pause")
    };
    frame_serial
}

#[cfg_attr(test, test)]
fn module_context_ownership_nested_throw_restores_the_ambient_owner() {
    let mut vm = bounded_vm();
    let (pointer, _) = caller_name(&mut vm);
    let serial = paused_nested(&mut vm, true);
    assert!(matches!(
        vm.resume_debugger_nested_execution(serial).unwrap(),
        VmDebuggerNestedExecutionState::FrameReturned { .. }
    ));
    assert_eq!(vm.active_module_name.as_ref().unwrap().as_ptr(), pointer);
    assert_eq!(
        vm.resume_debugger_module_execution().unwrap(),
        VmDebuggerExecutionState::Completed
    );
    assert_eq!(vm.active_module_name.as_ref().unwrap().as_ptr(), pointer);
    assert_eq!(
        vm.lookup_global_name("answer").unwrap(),
        Some(Value::Number(7.0))
    );
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn module_context_ownership_nested_abort_retains_failure_and_the_ambient_owner() {
    let mut vm = bounded_vm();
    let (pointer, _) = caller_name(&mut vm);
    let serial = paused_nested(&mut vm, false);
    vm.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        vm.resume_debugger_nested_execution(serial),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.active_module_name.as_ref().unwrap().as_ptr(), pointer);
    assert!(vm.debugger_module_continuation.is_none());
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn module_context_ownership_return_throw_and_await_restore_the_original_name() {
    for source in ["0;", "throw 7;", "await 0;0;"] {
        let mut vm = bounded_vm();
        let (pointer, capacity) = caller_name(&mut vm);
        let result = vm.execute_module_graph(
            "page/module.js",
            &HashMap::from([("page/module.js".into(), compile_module(source))]),
        );
        if source == "throw 7;" {
            assert_eq!(result, Err(RuntimeError::Thrown(Value::Number(7.0))));
        } else {
            assert!(result.is_ok());
        }
        assert_eq!(vm.active_module_name.as_ref().unwrap().as_ptr(), pointer);
        assert_eq!(vm.active_module_name.as_ref().unwrap().capacity(), capacity);
        assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    }
}

#[cfg_attr(test, test)]
fn module_context_ownership_later_frame_refusal_refunds_the_prepared_name_immediately() {
    let mut vm = bounded_vm();
    let (pointer, _) = caller_name(&mut vm);
    vm.stack = Vec::with_capacity(16);
    vm.stack.push(Value::Number(7.0));
    let code = compile_module("export let answer=42;");
    let cell = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    let mut cells = HashMap::from([(0, cell)]);
    let _peer = leave_room(&mut vm, "page/module.js".len());
    let before = vm.host_scheduling_snapshot().payload_bytes;
    assert_eq!(
        vm.initialize_module_record("page/module.js", &code, &mut cells),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(vm.stack, vec![Value::Number(7.0)]);
    assert_eq!(cells, HashMap::from([(0, cell)]));
    assert_eq!(vm.active_module_name.as_ref().unwrap().as_ptr(), pointer);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn module_context_ownership_shadow_transfer_counts_the_name_once_and_preserves_imports() {
    let mut vm = bounded_vm();
    let realm = vm
        .execute_script(&crate::compile(&crate::parse("new ShadowRealm();").unwrap()).unwrap())
        .unwrap();
    let module = compile_module("export function answer(){return 42;}");
    vm.set_module_loader_context(
        "page/home.js",
        HashMap::from([("page/value.js".into(), module)]),
    );
    let pointer = vm.active_module_name.as_ref().unwrap().as_ptr();
    let promise = vm
        .shadow_realm_import_value(
            realm.clone(),
            Value::String("./value.js".into()),
            Value::String("answer".into()),
        )
        .unwrap()
        .object_id()
        .unwrap();
    let value = match &vm.promises.get(&promise).unwrap().status {
        PromiseStatus::Fulfilled(value) => value.clone(),
        _ => panic!("one in-memory module must settle"),
    };
    assert_eq!(
        vm.call_native(value, Value::Undefined, Vec::new(), false),
        Ok(Value::Number(42.0))
    );
    assert_eq!(vm.active_module_name.as_ref().unwrap().as_ptr(), pointer);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    let record = vm
        .shadow_realms
        .get(&realm.object_id().unwrap())
        .unwrap()
        .clone();
    let mut child = record.vm.borrow_mut();
    assert_eq!(child.active_module_name.as_deref(), Some("page/home.js"));
    child.synchronize_scheduling_usage().unwrap();
    vm.synchronize_scheduling_usage().unwrap();
    assert_eq!(
        vm.module_registry["page/value.js"].code.as_ptr(),
        child.module_registry["page/value.js"].code.as_ptr()
    );
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        vm.own_scheduling_usage().payload_bytes
            + child.own_scheduling_usage().payload_bytes
            + vm.module_registry.retained_bytes()
            + vm.module_record_storage_bytes()
            + vm.module_root_storage_bytes()
            + child.module_record_storage_bytes()
            + child.module_root_storage_bytes()
    );
}

impl crate::Vm {
    /// Runs finite module context-name contracts in unit/coverage builds.
    #[doc(hidden)]
    pub fn verify_module_context_ownership_contracts() {
        module_context_ownership_instantiation_refuses_before_replacing_caller_fields();
        module_context_ownership_displaced_caller_name_is_charged_during_module_callbacks();
        module_context_ownership_debugger_pause_and_step_move_the_ambient_allocation();
        module_context_ownership_shadow_name_refusal_precedes_promise_capability();
        module_context_ownership_exact_name_budget_and_one_byte_refusal();
        module_context_ownership_ambient_move_needs_no_new_budget_including_empty_names();
        module_context_ownership_nested_throw_restores_the_ambient_owner();
        module_context_ownership_nested_abort_retains_failure_and_the_ambient_owner();
        module_context_ownership_return_throw_and_await_restore_the_original_name();
        module_context_ownership_later_frame_refusal_refunds_the_prepared_name_immediately();
        module_context_ownership_shadow_transfer_counts_the_name_once_and_preserves_imports();
    }
}
