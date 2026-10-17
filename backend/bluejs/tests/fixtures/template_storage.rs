// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny template/control buffers verify independent pre-allocation ownership.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.remaining_instructions = 100_000;
    vm.next_host_native_gc_bytes = usize::MAX;
    vm.next_host_heap_gc_bytes = usize::MAX;
    vm
}
fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    }
}
fn table_bytes(count: usize) -> usize {
    host_scheduling::map_slots(&HashMap::<TemplateSiteId, ObjectId>::with_capacity(count))
}
fn leave_room(vm: &mut Vm, bytes: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - bytes)
        .unwrap()
}
#[cfg_attr(test, test)]
fn template_storage_detached_table_keeps_native_lease_after_vm_destruction() {
    let mut vm = page();
    vm.stack.reserve(8);
    let view = observer(&vm);
    let prepared = vm.prepare_template_cache_insert().unwrap();
    drop(vm);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        table_bytes(1)
    );
    drop(prepared);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn template_storage_operand_growth_requires_full_candidate_not_net_delta() {
    let mut vm = page();
    vm.templates
        .insert(TemplateSiteId::new(), vm.object_prototype);
    vm.stack = Vec::with_capacity(1);
    vm.stack.push(Value::Number(7.0));
    let _peer = leave_room(&mut vm, 2 * std::mem::size_of::<Value>());
    let pointer = vm.stack.as_ptr();
    let heap = vm.heap.stats();
    assert!(matches!(
        vm.prepare_template_cache_insert(),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert_eq!(vm.stack.as_ptr(), pointer);
    assert_eq!(vm.stack.capacity(), 1);
    assert_eq!(vm.stack, vec![Value::Number(7.0)]);
    assert_eq!(vm.heap.stats(), heap);
}
#[cfg_attr(test, test)]
fn template_storage_joint_short_budget_preserves_table_and_operand_backing() {
    let mut vm = page();
    for _ in 0..3 {
        vm.templates
            .insert(TemplateSiteId::new(), vm.object_prototype);
    }
    vm.stack = Vec::with_capacity(1);
    vm.stack.push(Value::Number(7.0));
    let _peer = leave_room(
        &mut vm,
        table_bytes(4) + 3 * std::mem::size_of::<Value>() - 1,
    );
    let pointer = vm.stack.as_ptr();
    assert!(matches!(
        vm.prepare_template_cache_insert(),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert_eq!(vm.stack.as_ptr(), pointer);
    assert_eq!(vm.stack.capacity(), 1);
    assert_eq!(vm.templates.len(), 3);
    assert_eq!(vm.templates.capacity(), 3);
}

fn fill(vm: &mut Vm, count: usize) {
    for _ in 0..count {
        vm.templates
            .insert(TemplateSiteId::new(), vm.object_prototype);
    }
}
fn code(source: &str) -> Bytecode {
    crate::compile(&crate::parse(source).unwrap()).unwrap()
}
#[cfg_attr(test, test)]
fn template_storage_operand_transfer_keeps_primitive_buffer_without_copying() {
    let mut vm = page();
    fill(&mut vm, 1);
    vm.stack = Vec::with_capacity(1);
    vm.stack.push(Value::String("tiny".into()));
    let Value::String(text) = &vm.stack[0] else {
        panic!("a primitive operand")
    };
    let pointer = text.as_code_units().as_ptr();
    let slot = std::mem::size_of::<Value>();
    let _peer = leave_room(&mut vm, 3 * slot);
    let prepared = vm.prepare_template_cache_insert().unwrap();
    let Value::String(text) = &vm.stack[0] else {
        panic!("the same primitive operand")
    };
    assert_eq!(text.as_code_units().as_ptr(), pointer);
    assert_eq!(vm.stack.capacity(), 3);
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - slot
    );
    drop(prepared);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}
#[cfg_attr(test, test)]
fn template_storage_after_operand_adoption_only_detached_table_outlives_vm() {
    let mut vm = page();
    let view = observer(&vm);
    let prepared = vm.prepare_template_cache_insert().unwrap();
    assert_eq!(vm.stack.capacity(), 2);
    drop(vm);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        table_bytes(1)
    );
    drop(prepared);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn template_storage_spare_table_and_operand_slots_need_no_fictitious_charge() {
    let mut vm = page();
    fill(&mut vm, 1);
    vm.stack.reserve(8);
    let site = TemplateSiteId::new();
    let pointer = vm.stack.as_ptr();
    let _peer = leave_room(&mut vm, 0);
    let prepared = vm.prepare_template_cache_insert().unwrap();
    let object = vm.object_prototype;
    prepared.publish(&mut vm, site, object);
    assert_eq!(vm.templates.len(), 2);
    assert_eq!(vm.stack.as_ptr(), pointer);
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}
#[cfg_attr(test, test)]
fn template_storage_exact_table_publication_preserves_old_entries_and_refunds_staging() {
    let mut vm = page();
    fill(&mut vm, 3);
    vm.stack.reserve(8);
    let old = vm.templates.clone();
    let old_bytes = host_scheduling::map_slots(&vm.templates);
    let site = TemplateSiteId::new();
    let _peer = leave_room(&mut vm, table_bytes(4));
    let prepared = vm.prepare_template_cache_insert().unwrap();
    let object = vm.object_prototype;
    prepared.publish(&mut vm, site.clone(), object);
    assert_eq!(vm.templates.len(), 4);
    assert_eq!(vm.templates[&site], object);
    assert!(old
        .iter()
        .all(|(site, object)| vm.templates.get(site) == Some(object)));
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - old_bytes
    );
}
#[cfg_attr(test, test)]
fn template_storage_abandonment_refunds_table_and_keeps_only_adopted_operand_cache() {
    let mut vm = page();
    fill(&mut vm, 3);
    vm.stack = Vec::with_capacity(1);
    vm.stack.push(Value::Number(7.0));
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    let bytes = table_bytes(4) + 3 * std::mem::size_of::<Value>();
    let view = observer(&vm);
    let peer = leave_room(&mut vm, bytes);
    let prepared = vm.prepare_template_cache_insert().unwrap();
    drop(prepared);
    assert_eq!(vm.templates.len(), 3);
    assert_eq!(vm.stack, vec![Value::Number(7.0)]);
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - bytes + 2 * std::mem::size_of::<Value>()
    );
    drop(vm);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - used - bytes
    );
    drop(peer);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn template_storage_reported_table_refusal_refunds_lease_without_stack_mutation() {
    let mut vm = page();
    fill(&mut vm, 3);
    vm.stack.reserve(8);
    let pointer = vm.stack.as_ptr();
    let _peer = leave_room(&mut vm, table_bytes(4) - 1);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    assert!(matches!(
        vm.prepare_template_cache_insert(),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert_eq!(vm.templates.len(), 3);
    assert_eq!(vm.stack.as_ptr(), pointer);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}
#[cfg_attr(test, test)]
fn template_storage_host_neutral_cache_preserves_identity() {
    let mut vm = Vm::default();
    let code = code("tag`tiny`");
    let first = vm.template_object(&code.templates[0]).unwrap();
    assert_eq!(vm.template_object(&code.templates[0]).unwrap(), first);
    assert!(vm.stack.is_empty());
    assert_eq!(vm.templates.len(), 1);
}
#[cfg_attr(test, test)]
fn template_storage_managed_failure_refunds_table_and_restores_operands() {
    let mut vm = page();
    fill(&mut vm, 3);
    vm.stack = Vec::with_capacity(1);
    vm.stack.push(Value::Number(7.0));
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    let view = observer(&vm);
    let code = code("tag`tiny`");
    let limit = vm.heap.allow_only(0);
    assert_eq!(
        vm.template_object(&code.templates[0]),
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
    );
    assert_eq!(vm.templates.len(), 3);
    assert_eq!(vm.stack, vec![Value::Number(7.0)]);
    assert_eq!(vm.stack.capacity(), 3);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn template_storage_string_refusal_leaves_only_the_admitted_operand_cache() {
    let mut vm = page();
    fill(&mut vm, 3);
    vm.stack = Vec::with_capacity(1);
    vm.stack.push(Value::Number(7.0));
    let code = code("tag`long`");
    vm.config.max_string_bytes = 2;
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let heap = vm.heap.stats();
    assert_eq!(
        vm.template_object(&code.templates[0]),
        Err(RuntimeError::StringLimit { limit: 2 })
    );
    assert_eq!(vm.heap.stats(), heap);
    assert_eq!(vm.templates.len(), 3);
    assert_eq!(vm.stack, vec![Value::Number(7.0)]);
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        before + 2 * std::mem::size_of::<Value>()
    );
}
#[cfg_attr(test, test)]
fn template_storage_managed_gc_preserves_cached_and_new_template_objects() {
    let mut vm = page();
    let code = code("tag`a`;tag`b`;tag`c`;tag`d`");
    let old: Vec<_> = code.templates[..3]
        .iter()
        .map(|site| vm.template_object(site).unwrap())
        .collect();
    let mut garbage = Vec::new();
    for _ in 0..4 {
        let event = vm.new_web_event_state(2, "unused".into());
        let id = vm
            .heap
            .alloc_drag_event(event, vm.object_prototype)
            .unwrap();
        garbage.push(vm.heap.root(id).unwrap());
    }
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    for root in garbage {
        vm.heap.unroot(root).unwrap();
    }
    let major = vm.heap.stats().major_collections;
    vm.heap.allow_only(0);
    let created = vm.template_object(&code.templates[3]).unwrap();
    assert!(vm.heap.stats().major_collections > major);
    assert_eq!(vm.templates.len(), 4);
    for (site, expected) in code.templates[..3].iter().zip(old) {
        assert_eq!(vm.template_object(site).unwrap(), expected);
    }
    assert_eq!(vm.template_object(&code.templates[3]).unwrap(), created);
    assert!(vm
        .heap
        .get_own(created.object_id().unwrap(), "raw")
        .unwrap()
        .is_some());
    assert!(vm.stack.is_empty());
}
#[cfg_attr(test, test)]
fn template_storage_public_refusal_preserves_operand_backing_heap_and_fuel() {
    let mut vm = page();
    vm.stack = Vec::with_capacity(1);
    vm.stack.push(Value::Number(7.0));
    vm.remaining_instructions = 17;
    let code = code("tag`tiny`");
    let heap = vm.heap.stats();
    let pointer = vm.stack.as_ptr();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.template_object(&code.templates[0]),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.stack.as_ptr(), pointer);
    assert_eq!(vm.stack, vec![Value::Number(7.0)]);
    assert_eq!(vm.heap.stats(), heap);
    assert_eq!(vm.remaining_instructions, 17);
    assert!(vm.templates.is_empty());
}
#[cfg_attr(test, test)]
fn template_storage_terminal_preparation_does_not_publish_spare_storage() {
    let mut vm = page();
    fill(&mut vm, 1);
    vm.stack.reserve(8);
    let pointer = vm.stack.as_ptr();
    vm.host_scheduling.as_ref().unwrap().fail();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    assert!(matches!(
        vm.prepare_template_cache_insert(),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert_eq!(vm.stack.as_ptr(), pointer);
    assert_eq!(vm.templates.len(), 1);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}
#[cfg_attr(test, test)]
fn template_storage_multiple_preparations_retain_separate_post_vm_owners() {
    let mut vm = page();
    let first = vm.prepare_template_cache_insert().unwrap();
    let second = vm.prepare_template_cache_insert().unwrap();
    let view = observer(&vm);
    drop(vm);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        2 * table_bytes(1)
    );
    drop(first);
    assert_eq!(
        view.host_scheduling_snapshot().payload_bytes,
        table_bytes(1)
    );
    drop(second);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}

impl crate::Vm {
    /// Runs finite template/control preparation contracts in unit/coverage builds.
    #[doc(hidden)]
    pub fn verify_template_storage_contracts() {
        template_storage_detached_table_keeps_native_lease_after_vm_destruction();
        template_storage_operand_growth_requires_full_candidate_not_net_delta();
        template_storage_joint_short_budget_preserves_table_and_operand_backing();
        template_storage_operand_transfer_keeps_primitive_buffer_without_copying();
        template_storage_after_operand_adoption_only_detached_table_outlives_vm();
        template_storage_spare_table_and_operand_slots_need_no_fictitious_charge();
        template_storage_exact_table_publication_preserves_old_entries_and_refunds_staging();
        template_storage_abandonment_refunds_table_and_keeps_only_adopted_operand_cache();
        template_storage_reported_table_refusal_refunds_lease_without_stack_mutation();
        template_storage_host_neutral_cache_preserves_identity();
        template_storage_managed_failure_refunds_table_and_restores_operands();
        template_storage_string_refusal_leaves_only_the_admitted_operand_cache();
        template_storage_managed_gc_preserves_cached_and_new_template_objects();
        template_storage_public_refusal_preserves_operand_backing_heap_and_fuel();
        template_storage_terminal_preparation_does_not_publish_spare_storage();
        template_storage_multiple_preparations_retain_separate_post_vm_owners();
    }
}
