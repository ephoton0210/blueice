// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Small closures keep module associations only while their heap owner lives.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn compile(source: &str) -> Bytecode {
    crate::compile(&crate::parse(source).unwrap()).unwrap()
}

fn bounded_vm() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.remaining_instructions = 100_000;
    vm
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
fn module_referrer_ownership_counts_persistent_table_and_name_capacity() {
    let mut vm = bounded_vm();
    let callee = vm
        .execute_script(&compile("(function(){return 42;});"))
        .unwrap();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let mut name = String::with_capacity(64);
    name.push_str("page/home.js");
    let payload = name.capacity();
    vm.module_closure_referrers
        .insert(callee.object_id().unwrap(), name);
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes - before,
        host_scheduling::map_slots(&vm.module_closure_referrers) + payload
    );
}

#[cfg_attr(test, test)]
fn module_referrer_ownership_collection_releases_dead_closure_names_and_empty_table() {
    let mut vm = bounded_vm();
    vm.set_module_loader_context("page/home.js", HashMap::new());
    let callee = vm
        .execute_script(&compile("(function(){return 42;});"))
        .unwrap()
        .object_id()
        .unwrap();
    assert!(vm.module_closure_referrers.contains_key(&callee));
    vm.execute_script(&compile("0;")).unwrap();
    assert!(!vm.heap.contains(callee));
    assert!(!vm.module_closure_referrers.contains_key(&callee));
    assert_eq!(vm.module_closure_referrers.capacity(), 0);
}

#[cfg_attr(test, test)]
fn module_referrer_ownership_call_refuses_before_spending_instruction_or_displacing_caller() {
    let mut vm = bounded_vm();
    vm.set_module_loader_context("page/home.js", HashMap::new());
    let callee = vm
        .execute_script(&compile("(function(){return 42;});"))
        .unwrap();
    vm.active_module_name = Some("page/caller.js".into());
    vm.stack.reserve(64);
    vm.stack.push(Value::Number(7.0));
    vm.remaining_instructions = 100_000;
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.call_native(callee, Value::Undefined, Vec::new(), false),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.remaining_instructions, 100_000);
    assert_eq!(vm.stack, vec![Value::Number(7.0)]);
    assert_eq!(vm.active_module_name.as_deref(), Some("page/caller.js"));
    assert_eq!(vm.call_depth, 0);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn module_referrer_ownership_closure_registration_refuses_before_publication() {
    let mut vm = bounded_vm();
    vm.function_prototype().unwrap();
    let observer = Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    };
    let held = Rc::new(RefCell::new(None));
    let output = held.clone();
    vm.install_host_function("fill", 0, move |_: &[HostValue]| {
        let used = observer.host_scheduling_snapshot().payload_bytes;
        *output.borrow_mut() = Some(
            observer
                .host_scheduling
                .as_ref()
                .unwrap()
                .shared_charge(MAX_HOST_SCHEDULING_BYTES - used)
                .unwrap(),
        );
        Ok(HostValue::Undefined)
    })
    .unwrap();
    vm.set_module_loader_context("page/home.js", HashMap::new());
    let result = vm.execute_script(&compile("fill();(function(){return 42;});"));
    assert!(held.borrow().is_some());
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(vm.module_closure_referrers.is_empty());
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn module_referrer_ownership_creation_joint_budget_and_one_byte_refusal() {
    for full in [false, true] {
        for accepts in [false, true] {
            let mut vm = bounded_vm();
            let callee = vm
                .execute_script(&compile("(function(){return 42;});"))
                .unwrap()
                .object_id()
                .unwrap();
            let code = vm.heap.closure_code(callee).unwrap().unwrap();
            let prototype = vm.function_prototype().unwrap();
            for _ in 0..if full { 3 } else { 1 } {
                let id = vm
                    .heap
                    .alloc_closure(code.clone(), Vec::new(), Value::Undefined, prototype)
                    .unwrap();
                vm.module_closure_referrers.insert(id, "kept".into());
            }
            let mut name = String::with_capacity(64);
            name.push_str("page/home.js");
            vm.active_module_name = Some(name);
            let table_bytes = if full {
                let mut table = HashMap::<ObjectId, String>::new();
                table.reserve(vm.module_closure_referrers.len() + 1);
                host_scheduling::map_slots(&table)
            } else {
                0
            };
            let bytes = table_bytes + "page/home.js".len();
            let state = vm.module_closure_referrers.clone();
            let capacity = vm.module_closure_referrers.capacity();
            let _peer = leave_room(&mut vm, bytes - usize::from(!accepts));
            let before = vm.host_scheduling_snapshot().payload_bytes;
            let result = vm.prepare_closure_module_referrer();
            if accepts {
                let prepared = result.unwrap();
                assert_eq!(vm.displaced_execution_control_bytes.get(), bytes);
                assert_eq!(vm.pending_module_referrers.get(), 1);
                drop(prepared);
            } else {
                assert!(matches!(result, Err(RuntimeError::SchedulingLimit)));
            }
            assert_eq!(vm.module_closure_referrers, state);
            assert_eq!(vm.module_closure_referrers.capacity(), capacity);
            assert_eq!(vm.pending_module_referrers.get(), 0);
            assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
            vm.reconcile_scheduling_ownership();
            assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
        }
    }
}

#[cfg_attr(test, test)]
fn module_referrer_ownership_call_copy_is_leased_until_module_installation() {
    for accepts in [false, true] {
        let mut vm = bounded_vm();
        vm.set_module_loader_context("page/home.js", HashMap::new());
        let callee = vm
            .execute_script(&compile("(function(){return 42;});"))
            .unwrap();
        let mut caller = String::with_capacity(64);
        caller.push_str("caller");
        let pointer = caller.as_ptr();
        let capacity = caller.capacity();
        vm.active_module_name = Some(caller);
        let _peer = leave_room(&mut vm, "page/home.js".len() - usize::from(!accepts));
        let prepared = vm.prepare_call_module_referrer(&callee);
        if accepts {
            let mut prepared = prepared.unwrap();
            assert_eq!(
                vm.displaced_execution_control_bytes.get(),
                "page/home.js".len()
            );
            let previous = prepared.install(&mut vm).unwrap();
            let caller_charge = vm.charge_execution_control(previous.as_ref().unwrap().capacity());
            drop(prepared);
            assert_eq!(vm.active_module_name.as_deref(), Some("page/home.js"));
            vm.synchronize_scheduling_usage().unwrap();
            vm.active_module_name = previous;
            drop(caller_charge);
        } else {
            assert!(matches!(prepared, Err(RuntimeError::SchedulingLimit)));
        }
        assert_eq!(vm.active_module_name.as_ref().unwrap().as_ptr(), pointer);
        assert_eq!(vm.active_module_name.as_ref().unwrap().capacity(), capacity);
        assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    }
}

#[cfg_attr(test, test)]
fn module_referrer_ownership_collection_preserves_reserved_publication_slots() {
    for nursery in [1, 16] {
        let mut config = VmConfig::default();
        config.heap.nursery_capacity = nursery;
        let mut vm = Vm::new(config).unwrap();
        vm.use_bounded_host_microtask_checkpoints().unwrap();
        vm.set_module_loader_context("page/home.js", HashMap::new());
        let callee = vm
            .execute_script(&compile("(function(){return 42;});"))
            .unwrap()
            .object_id()
            .unwrap();
        let code = vm.heap.closure_code(callee).unwrap().unwrap();
        let capacity = vm.module_closure_referrers.capacity();
        let prepared = vm.prepare_closure_module_referrer().unwrap();
        if let Some(root) = vm.result_root.take() {
            vm.release_root(root);
        }
        vm.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })
        .unwrap();
        assert!(!vm.heap.contains(callee));
        assert!(vm.module_closure_referrers.is_empty());
        assert_eq!(vm.module_closure_referrers.capacity(), capacity);
        let prototype = vm.function_prototype().unwrap();
        let new = vm
            .with_roots(|heap| heap.alloc_closure(code, Vec::new(), Value::Undefined, prototype))
            .unwrap();
        prepared.publish(&mut vm, new);
        vm.stack.push(Value::Object(new));
        assert_eq!(vm.module_closure_referrers.capacity(), capacity);
        assert_eq!(
            vm.module_closure_referrers.get(&new).map(String::as_str),
            Some("page/home.js")
        );
        assert_eq!(vm.pending_module_referrers.get(), 0);
        assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
        vm.stack.clear();
        vm.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })
        .unwrap();
        assert_eq!(vm.module_closure_referrers.capacity(), 0);
    }
}

#[cfg_attr(test, test)]
fn module_referrer_ownership_heap_refusal_refunds_unpublished_storage() {
    let mut vm = bounded_vm();
    vm.function_prototype().unwrap();
    vm.set_module_loader_context("page/home.js", HashMap::new());
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let limit = vm.heap.allow_only(0);
    assert_eq!(
        vm.execute_script(&compile("(function(){return 42;});")),
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
    );
    assert!(vm.module_closure_referrers.is_empty());
    assert_eq!(vm.module_closure_referrers.capacity(), 0);
    assert_eq!(vm.pending_module_referrers.get(), 0);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    let retained = vm.host_scheduling_usage().unwrap().payload_bytes;
    assert!(retained <= before);
    assert_eq!(
        retained,
        vm.active_module_name.as_ref().unwrap().capacity() + vm.gc_trace_cache.retained_bytes()
    );
    vm.heap.allow_only(16 * 1024 * 1024);
    assert!(vm
        .execute_script(&compile("(function(){return 42;});"))
        .is_ok());
}

#[cfg_attr(test, test)]
fn module_referrer_ownership_absent_and_empty_names_have_distinct_call_semantics() {
    let mut vm = bounded_vm();
    let callee = vm
        .execute_script(&compile("(function(){return 42;});"))
        .unwrap();
    vm.active_module_name = Some("caller".into());
    let mut prepared = vm.prepare_call_module_referrer(&callee).unwrap();
    assert!(prepared.install(&mut vm).is_none());
    drop(prepared);
    assert_eq!(vm.active_module_name.as_deref(), Some("caller"));
    vm.module_closure_referrers
        .insert(callee.object_id().unwrap(), String::new());
    let _peer = leave_room(&mut vm, 0);
    let mut prepared = vm.prepare_call_module_referrer(&callee).unwrap();
    let previous = prepared.install(&mut vm).unwrap();
    assert_eq!(vm.active_module_name.as_deref(), Some(""));
    vm.active_module_name = previous;
    drop(prepared);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn module_referrer_ownership_real_calls_restore_and_release_the_caller_name() {
    for abrupt in [false, true] {
        let mut vm = bounded_vm();
        let seen = Rc::new(std::cell::Cell::new(0));
        let output = seen.clone();
        let counter = vm.displaced_execution_control_bytes.clone();
        vm.install_host_function("observe", 0, move |_: &[HostValue]| {
            output.set(counter.get());
            Ok(HostValue::Undefined)
        })
        .unwrap();
        vm.set_module_loader_context("page/home.js", HashMap::new());
        let body = if abrupt { "throw 42" } else { "return 42" };
        let callee = vm
            .execute_script(&compile(&format!("(function(){{observe();{body};}});")))
            .unwrap();
        let mut caller = String::with_capacity(64);
        caller.push_str("caller");
        let pointer = caller.as_ptr();
        let capacity = caller.capacity();
        vm.active_module_name = Some(caller);
        let result = vm.call_native(callee, Value::Undefined, Vec::new(), false);
        assert_eq!(
            result,
            if abrupt {
                Err(RuntimeError::Thrown(Value::Number(42.0)))
            } else {
                Ok(Value::Number(42.0))
            }
        );
        assert!(seen.get() >= capacity);
        assert_eq!(vm.active_module_name.as_ref().unwrap().as_ptr(), pointer);
        assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    }
}

#[cfg_attr(test, test)]
fn module_referrer_ownership_surviving_factory_child_keeps_its_import_origin() {
    let mut vm = bounded_vm();
    let module =
        crate::compile_module(&crate::parse_module("export const value=42;").unwrap()).unwrap();
    vm.set_module_loader_context(
        "page/home.js",
        HashMap::from([("page/value.js".into(), module.clone())]),
    );
    vm.execute_script(&compile(
        "var child=(function(){return function(){return import('./value.js');};})();0;",
    ))
    .unwrap();
    let child = vm
        .lookup_global_name("child")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap();
    assert_eq!(
        vm.module_closure_referrers.get(&child).map(String::as_str),
        Some("page/home.js")
    );
    vm.set_module_loader_context(
        "other/main.js",
        HashMap::from([("page/value.js".into(), module)]),
    );
    vm.execute_script(&compile("var answer=0;child().then(m=>answer=m.value);0;"))
        .unwrap();
    for _ in 0..16 {
        if !vm.run_next_promise_job().unwrap() {
            break;
        }
    }
    assert!(vm.promise_jobs.is_empty());
    assert_eq!(
        vm.lookup_global_name("answer").unwrap(),
        Some(Value::Number(42.0))
    );
    assert_eq!(vm.active_module_name.as_deref(), Some("other/main.js"));
}

#[cfg_attr(test, test)]
fn module_referrer_ownership_collection_refunds_names_after_terminal_failure() {
    let mut vm = bounded_vm();
    vm.set_module_loader_context("page/home.js", HashMap::new());
    let callee = vm
        .execute_script(&compile("(function(){return 42;});"))
        .unwrap()
        .object_id()
        .unwrap();
    vm.host_scheduling.as_ref().unwrap().fail();
    if let Some(root) = vm.result_root.take() {
        vm.release_root(root);
    }
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    assert!(!vm.heap.contains(callee));
    assert_eq!(vm.module_closure_referrers.capacity(), 0);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

impl crate::Vm {
    /// Runs finite closure module-referrer contracts in unit/coverage builds.
    #[doc(hidden)]
    pub fn verify_module_referrer_ownership_contracts() {
        module_referrer_ownership_counts_persistent_table_and_name_capacity();
        module_referrer_ownership_collection_releases_dead_closure_names_and_empty_table();
        module_referrer_ownership_call_refuses_before_spending_instruction_or_displacing_caller();
        module_referrer_ownership_closure_registration_refuses_before_publication();
        module_referrer_ownership_creation_joint_budget_and_one_byte_refusal();
        module_referrer_ownership_call_copy_is_leased_until_module_installation();
        module_referrer_ownership_collection_preserves_reserved_publication_slots();
        module_referrer_ownership_heap_refusal_refunds_unpublished_storage();
        module_referrer_ownership_absent_and_empty_names_have_distinct_call_semantics();
        module_referrer_ownership_real_calls_restore_and_release_the_caller_name();
        module_referrer_ownership_surviving_factory_child_keeps_its_import_origin();
        module_referrer_ownership_collection_refunds_names_after_terminal_failure();
    }
}
