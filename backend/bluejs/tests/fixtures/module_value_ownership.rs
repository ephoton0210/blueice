// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny cached values and numeric native allowances exercise module ownership.

use super::*;

fn module(source: &str) -> Bytecode {
    crate::compile_module(&crate::parse_module(source).unwrap()).unwrap()
}
fn vm() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}
fn extra_native_bytes(vm: &Vm) -> usize {
    vm.host_scheduling_snapshot().payload_bytes
        - vm.own_scheduling_usage().payload_bytes
        - vm.module_registry.retained_bytes()
        - vm.module_record_storage_bytes()
        - vm.module_root_storage_bytes()
        - vm.heap.collector_storage_bytes()
}

#[cfg_attr(test, test)]
fn module_value_ownership_cached_completion_retains_its_primitive_payload() {
    let mut vm = vm();
    assert_eq!(
        vm.execute_module_graph(
            "entry",
            &HashMap::from([("entry".into(), module("'tiny';"))])
        ),
        Ok(Value::String("tiny".into()))
    );
    assert!(
        extra_native_bytes(&vm) >= 8,
        "cached completion primitive has a native owner"
    );
}

#[cfg_attr(test, test)]
fn module_value_ownership_cached_error_retains_its_primitive_payload() {
    let mut vm = vm();
    assert_eq!(
        vm.execute_module_graph(
            "entry",
            &HashMap::from([("entry".into(), module("throw 'tiny';"))])
        ),
        Err(RuntimeError::Thrown(Value::String("tiny".into())))
    );
    assert!(
        extra_native_bytes(&vm) >= 8,
        "cached error primitive has a native owner"
    );
}

fn leave_room(vm: &mut Vm, room: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - room)
        .unwrap()
}
fn script(source: &str) -> Bytecode {
    crate::compile(&crate::parse(source).unwrap()).unwrap()
}
fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Default::default()
    }
}
fn loaded() -> Vm {
    let mut vm = vm();
    vm.execute_module_graph(
        "entry",
        &HashMap::from([("entry".into(), module("'tiny';"))]),
    )
    .unwrap();
    vm
}

#[cfg_attr(test, test)]
fn module_value_ownership_completion_refusal_precedes_copy_and_publication() {
    let mut vm = loaded();
    let old = vm
        .linked_record("entry")
        .unwrap()
        .completion
        .clone()
        .unwrap();
    let value = Value::String("replacement".into());
    let pointer = value.as_string().unwrap().as_code_units().as_ptr();
    let before = vm.heap.stats();
    let _peer = leave_room(&mut vm, 0);
    assert!(matches!(
        vm.retain_module_completion(&value),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert!(Rc::ptr_eq(
        &old,
        vm.linked_record("entry")
            .unwrap()
            .completion
            .as_ref()
            .unwrap()
    ));
    assert_eq!(value.as_string().unwrap().as_code_units().as_ptr(), pointer);
    assert_eq!(vm.heap.stats(), before);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}

#[cfg_attr(test, test)]
fn module_value_ownership_exact_copy_allowance_uses_new_utf16_capacity() {
    for accepts in [false, true] {
        let mut vm = vm();
        let mut units = Vec::with_capacity(32);
        units.extend_from_slice(&[116, 105, 110, 121]);
        let value = Value::String(JsString::from_code_units(units));
        let original = host_scheduling::value_payload_bytes(&value);
        assert!(original > 8);
        let bytes = ModuleValueData::<()>::header_bytes() + 8;
        let room = bytes - usize::from(!accepts);
        let _peer = leave_room(&mut vm, room);
        let retained = vm.retain_module_completion(&value);
        if accepts {
            let retained = retained.unwrap();
            assert_eq!(retained.value, value);
            assert_eq!(retained.retained_bytes(), bytes);
            assert_eq!(
                vm.host_scheduling_snapshot().payload_bytes,
                MAX_HOST_SCHEDULING_BYTES
            );
            drop(retained);
        } else {
            assert!(matches!(retained, Err(RuntimeError::SchedulingLimit)));
        }
        assert_eq!(host_scheduling::value_payload_bytes(&value), original);
        assert_eq!(
            vm.host_scheduling_snapshot().payload_bytes,
            MAX_HOST_SCHEDULING_BYTES - room
        );
    }
}

#[cfg_attr(test, test)]
fn module_value_ownership_completion_outlives_the_vm_without_early_refund() {
    let mut vm = vm();
    let observed = observer(&vm);
    let value = vm
        .retain_module_completion(&Value::String("tiny".into()))
        .unwrap();
    let bytes = value.retained_bytes();
    drop(vm);
    assert_eq!(observed.host_scheduling_snapshot().payload_bytes, bytes);
    drop(value);
    assert_eq!(observed.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn module_value_ownership_completion_clones_share_native_storage_at_full_quota() {
    let mut vm = vm();
    let value = vm
        .retain_module_completion(&Value::String("tiny".into()))
        .unwrap();
    let _peer = leave_room(&mut vm, 0);
    let first = value.clone();
    let second = first.clone();
    assert!(Rc::ptr_eq(&value, &second));
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
    assert!(vm.check_scheduling_failure().is_ok());
    drop(first);
    drop(second);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}

#[cfg_attr(test, test)]
fn module_value_ownership_rust_held_graph_keeps_its_cached_completion() {
    let mut vm = loaded();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let graph = vm.module_graph.take().unwrap();
    let value_bytes = graph.linked["entry"]
        .completion
        .as_ref()
        .unwrap()
        .retained_bytes();
    vm.reconcile_scheduling_ownership();
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(vm.module_value_storage_bytes(), 0);
    for root in &graph.roots {
        vm.heap.unroot(*root).unwrap();
    }
    drop(graph);
    vm.reconcile_scheduling_ownership();
    assert!(vm.host_scheduling_snapshot().payload_bytes <= before - value_bytes);
}

#[cfg_attr(test, test)]
fn module_value_ownership_cached_object_completion_remains_rooted() {
    let mut vm = vm();
    let value = vm
        .execute_module_graph(
            "entry",
            &HashMap::from([("entry".into(), module("({answer:42});"))]),
        )
        .unwrap();
    let id = value.object_id().unwrap();
    vm.execute_script(&script("0;")).unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(vm.heap.contains(id));
    assert_eq!(
        vm.get_property(&value, &"answer".into()),
        Ok(Value::Number(42.0))
    );
}

#[cfg_attr(test, test)]
fn module_value_ownership_late_completion_activation_attaches_once() {
    let mut vm = Vm::default();
    vm.execute_module_graph(
        "entry",
        &HashMap::from([("entry".into(), module("'tiny';"))]),
    )
    .unwrap();
    let retained = vm
        .linked_record("entry")
        .unwrap()
        .completion
        .clone()
        .unwrap();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    assert_eq!(extra_native_bytes(&vm), vm.module_value_storage_bytes());
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert!(Rc::ptr_eq(
        &retained,
        vm.linked_record("entry")
            .unwrap()
            .completion
            .as_ref()
            .unwrap()
    ));
}

#[cfg_attr(test, test)]
fn module_value_ownership_late_rust_held_error_activation_attaches_once() {
    let mut vm = Vm::default();
    let error = vm
        .retain_module_error(&Value::String("tiny".into()), ThrowOrigin::default())
        .unwrap();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let bytes = vm.host_scheduling_snapshot().payload_bytes;
    assert_eq!(extra_native_bytes(&vm), error.retained_bytes());
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, bytes);
}

#[cfg_attr(test, test)]
fn module_value_ownership_async_parents_share_one_error_value_and_origin() {
    let mut vm = vm();
    vm.execute_script(&script("globalThis.gate=Promise.withResolvers();0;"))
        .unwrap();
    vm.execute_module_graph(
        "root",
        &HashMap::from([
            ("root".into(), module("import './middle';")),
            ("middle".into(), module("import './leaf';")),
            ("leaf".into(), module("await gate.promise;")),
        ]),
    )
    .unwrap();
    vm.execute_script(&script("gate.reject('tiny');0;"))
        .unwrap();
    vm.run_promise_jobs_bounded(16).unwrap();
    let leaf = vm.linked_record("leaf").unwrap().error.as_ref().unwrap();
    for name in ["root", "middle"] {
        assert!(Rc::ptr_eq(
            leaf,
            vm.linked_record(name).unwrap().error.as_ref().unwrap()
        ));
    }
    assert_eq!(leaf.value, Value::String("tiny".into()));
    assert_eq!(
        vm.module_error_origins
            .iter()
            .filter(|tracker| tracker.is_live())
            .count(),
        1
    );
}

#[cfg_attr(test, test)]
fn module_value_ownership_await_completion_keeps_its_cached_value() {
    let mut vm = vm();
    vm.execute_script(&script("globalThis.gate=Promise.withResolvers();0;"))
        .unwrap();
    vm.execute_module_graph(
        "entry",
        &HashMap::from([("entry".into(), module("await gate.promise;'tiny';"))]),
    )
    .unwrap();
    vm.execute_script(&script("gate.resolve();0;")).unwrap();
    vm.run_promise_jobs_bounded(16).unwrap();
    let completion = vm
        .linked_record("entry")
        .unwrap()
        .completion
        .as_ref()
        .unwrap();
    assert_eq!(completion.value, Value::String("tiny".into()));
    assert!(vm.module_value_storage_bytes() >= completion.retained_bytes());
}

#[cfg_attr(test, test)]
fn module_value_ownership_debugger_completion_keeps_its_cached_value() {
    let mut vm = vm();
    let code = module("'tiny';");
    let target = code.module_evaluate_entry.unwrap();
    vm.execute_module_graph_until_debugger_pause(
        "entry",
        &HashMap::from([("entry".into(), code)]),
        target,
    )
    .unwrap();
    vm.resume_debugger_module_execution().unwrap();
    let completion = vm
        .linked_record("entry")
        .unwrap()
        .completion
        .as_ref()
        .unwrap();
    assert_eq!(completion.value, Value::String("tiny".into()));
    assert!(vm.module_value_storage_bytes() >= completion.retained_bytes());
}

#[cfg_attr(test, test)]
fn module_value_ownership_error_payload_releases_before_weak_header_pruning() {
    let mut vm = vm();
    let error = vm
        .retain_module_error(&Value::String("tiny".into()), ThrowOrigin::default())
        .unwrap();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let header = ModuleValueData::<ThrowOrigin>::header_bytes();
    assert_eq!(error.retained_bytes(), header + 8);
    drop(error);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before - 8);
    assert!(!vm.module_error_origins[0].is_live());
    assert_eq!(vm.module_error_origins[0].lease.bytes.get(), header);
    vm.prune_module_error_origins();
    vm.reconcile_scheduling_ownership();
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn module_value_ownership_error_outlives_the_vm_without_early_refund() {
    let mut vm = vm();
    let observed = observer(&vm);
    let error = vm
        .retain_module_error(&Value::String("tiny".into()), ThrowOrigin::default())
        .unwrap();
    let bytes = error.retained_bytes();
    drop(vm);
    assert_eq!(observed.host_scheduling_snapshot().payload_bytes, bytes);
    drop(error);
    assert_eq!(observed.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn module_value_ownership_error_refusal_refunds_detached_discovery_storage() {
    let mut vm = vm();
    let observed = observer(&vm);
    let room = 4 * std::mem::size_of::<ModuleErrorTracker>();
    let _peer = leave_room(&mut vm, room);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    assert!(matches!(
        vm.retain_module_error(&Value::String("tiny".into()), ThrowOrigin::default()),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert!(vm.module_error_origins.is_empty());
    assert_eq!(vm.module_error_origins.capacity(), 0);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(observed.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn module_value_ownership_foreign_document_attachment_preserves_the_original_owner() {
    let mut original = vm();
    let foreign = vm();
    let value = original
        .retain_module_completion(&Value::String("tiny".into()))
        .unwrap();
    let before = original.host_scheduling_snapshot().payload_bytes;
    assert_eq!(
        value.attach(foreign.host_scheduling.as_ref().unwrap()),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(original.host_scheduling_snapshot().payload_bytes, before);
    assert!(original.check_scheduling_failure().is_ok());
    assert_eq!(
        foreign.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn module_value_ownership_late_activation_refusal_preserves_cached_identity() {
    let mut vm = Vm::default();
    vm.execute_module_graph(
        "entry",
        &HashMap::from([("entry".into(), module("'tiny';"))]),
    )
    .unwrap();
    let completion = vm
        .linked_record("entry")
        .unwrap()
        .completion
        .clone()
        .unwrap();
    let before = vm.heap.stats();
    let mut parent = self::vm();
    let _peer = leave_room(&mut parent, 0);
    vm.host_scheduling = Some(parent.host_scheduling.as_ref().unwrap().child());
    assert_eq!(
        vm.use_bounded_host_microtask_checkpoints(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.heap.stats(), before);
    assert!(Rc::ptr_eq(
        &completion,
        vm.linked_record("entry")
            .unwrap()
            .completion
            .as_ref()
            .unwrap()
    ));
}

#[cfg_attr(test, test)]
fn module_value_ownership_primitive_copies_retain_identity_and_reported_payload() {
    let mut vm = vm();
    for value in [
        Value::String("tiny".into()),
        Value::Symbol(crate::JsSymbol::new(Some("symbol".into()))),
        Value::BigInt(123.into()),
    ] {
        let before = vm.host_scheduling_snapshot().payload_bytes;
        let retained = vm.retain_module_completion(&value).unwrap();
        assert_eq!(retained.value, value);
        assert_eq!(
            vm.host_scheduling_snapshot().payload_bytes - before,
            retained.retained_bytes()
        );
        assert_eq!(
            retained.retained_bytes(),
            ModuleValueData::<()>::header_bytes()
                + host_scheduling::value_payload_bytes(&retained.value)
        );
        drop(retained);
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    }
}

#[cfg_attr(test, test)]
fn module_value_ownership_error_discovery_does_not_create_a_javascript_root() {
    let mut vm = vm();
    let id = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    let error = vm
        .retain_module_error(&Value::Object(id), ThrowOrigin::default())
        .unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.heap.contains(id));
    assert_eq!(error.value, Value::Object(id));
}

#[cfg_attr(test, test)]
fn module_value_ownership_terminal_drop_refunds_values_origins_and_discovery() {
    let mut vm = vm();
    let completion = vm
        .retain_module_completion(&Value::String("tiny".into()))
        .unwrap();
    let error = vm
        .retain_module_error(&Value::String("error".into()), ThrowOrigin::default())
        .unwrap();
    let value_bytes = completion.retained_bytes();
    let error_payload = host_scheduling::value_payload_bytes(&error.value);
    let _peer = leave_room(&mut vm, 0);
    let _ = vm.host_scheduling.as_ref().unwrap().fail();
    drop(completion);
    drop(error);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - value_bytes - error_payload
    );
    let remaining =
        vm.module_value_storage_bytes() + host_scheduling::slots(&vm.module_error_origins);
    vm.prune_module_error_origins();
    vm.reconcile_scheduling_ownership();
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - value_bytes - error_payload - remaining
    );
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_module_value_ownership_contracts() {
        module_value_ownership_cached_completion_retains_its_primitive_payload();
        module_value_ownership_cached_error_retains_its_primitive_payload();
        module_value_ownership_completion_refusal_precedes_copy_and_publication();
        module_value_ownership_exact_copy_allowance_uses_new_utf16_capacity();
        module_value_ownership_completion_outlives_the_vm_without_early_refund();
        module_value_ownership_completion_clones_share_native_storage_at_full_quota();
        module_value_ownership_rust_held_graph_keeps_its_cached_completion();
        module_value_ownership_cached_object_completion_remains_rooted();
        module_value_ownership_late_completion_activation_attaches_once();
        module_value_ownership_late_rust_held_error_activation_attaches_once();
        module_value_ownership_async_parents_share_one_error_value_and_origin();
        module_value_ownership_await_completion_keeps_its_cached_value();
        module_value_ownership_debugger_completion_keeps_its_cached_value();
        module_value_ownership_error_payload_releases_before_weak_header_pruning();
        module_value_ownership_error_outlives_the_vm_without_early_refund();
        module_value_ownership_error_refusal_refunds_detached_discovery_storage();
        module_value_ownership_foreign_document_attachment_preserves_the_original_owner();
        module_value_ownership_late_activation_refusal_preserves_cached_identity();
        module_value_ownership_primitive_copies_retain_identity_and_reported_payload();
        module_value_ownership_error_discovery_does_not_create_a_javascript_root();
        module_value_ownership_terminal_drop_refunds_values_origins_and_discovery();
    }
}
