// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Finite synthetic metadata preparation and registry ownership contracts.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;
use crate::vm::module_code_storage::PreparedModuleCode;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
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

#[cfg(test)]
#[test]
fn module_code_storage_refuses_before_metadata_allocations() {
    let mut vm = page();
    let _peer = leave_room(&mut vm, 0);
    let (result, allocations) =
        allocation_observer::count(|| PreparedModuleCode::synthetic(vm.host_scheduling.as_ref()));
    assert_eq!(allocations, 0);
    assert!(matches!(result, Err(RuntimeError::SchedulingLimit)));
}

#[cfg_attr(test, test)]
fn module_code_storage_retained_metadata_capacities_join_document_usage() {
    let vm = page();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let code = PreparedModuleCode::synthetic(vm.host_scheduling.as_ref()).unwrap();
    assert!(code.bytes() > 0);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes - before,
        code.bytes()
    );
    drop(code);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn module_code_storage_detached_code_keeps_ownership_after_vm_drop() {
    let vm = page();
    let observer = Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    };
    let code = PreparedModuleCode::synthetic(vm.host_scheduling.as_ref()).unwrap();
    let bytes = code.bytes();
    drop(vm);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, bytes);
    drop(code);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg(test)]
#[test]
fn module_code_storage_full_code_refusal_precedes_export_owner() {
    let mut vm = page();
    vm.set_json_module_sources(HashMap::from([("value".into(), "1".into())]));
    vm.remaining_instructions = 100;
    let _peer = leave_room(&mut vm, SyntheticModuleValue::header_bytes());
    let mut roots = ModuleRoots::default();
    let heap = vm.heap.stats();
    let (result, allocations) = allocation_observer::count(|| {
        vm.create_synthetic_module_code("value\0json", SyntheticModuleType::Json, &mut roots)
    });
    assert_eq!(allocations, 0);
    assert!(matches!(result, Err(RuntimeError::SchedulingLimit)));
    assert_eq!(vm.heap.stats(), heap);
    assert!(roots.is_empty());
}

fn resource_page() -> Vm {
    let mut vm = page();
    vm.set_json_module_sources(HashMap::from([
        ("value".into(), "{}".into()),
        ("number".into(), "1".into()),
    ]));
    vm.set_text_module_sources(HashMap::from([("value".into(), "tiny".into())]));
    vm.set_bytes_module_sources(HashMap::from([("value".into(), vec![1, 2])]));
    vm.remaining_instructions = 1000;
    vm
}
fn draft() -> ModuleRegistryDraft {
    ModuleRegistryDraft::new(ModuleRegistry::default())
}

#[cfg_attr(test, test)]
fn module_code_storage_metadata_keeps_synthetic_record_shape() {
    let code = PreparedModuleCode::synthetic(None).unwrap();
    assert!(code.module && code.strict);
    assert!(code.code.bytes().is_empty());
    assert_eq!(code.module_evaluate_entry, Some(0));
    assert_eq!(code.bindings.len(), 1);
    assert_eq!(code.bindings[0].name, "default");
    assert!(!code.bindings[0].mutable && code.bindings[0].strict_immutable);
    assert_eq!(code.scopes, vec![vec![0]]);
    assert!(
        matches!(&code.module_exports[..],[ModuleExport::Local{export_name,local_slot:0}] if export_name=="default")
    );
    assert!(code.synthetic_default_export.is_none());
}

#[cfg_attr(test, test)]
fn module_code_storage_metadata_exact_and_one_short_room() {
    let bytes = PreparedModuleCode::synthetic(None).unwrap().bytes();
    for accepts in [false, true] {
        let mut vm = page();
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        let peer = leave_room(&mut vm, bytes - usize::from(!accepts));
        let result = PreparedModuleCode::synthetic(vm.host_scheduling.as_ref());
        if accepts {
            let code = result.unwrap();
            assert_eq!(code.bytes(), bytes);
            assert_eq!(
                vm.host_scheduling_snapshot().payload_bytes,
                MAX_HOST_SCHEDULING_BYTES
            );
            drop(code);
        } else {
            assert!(matches!(result, Err(RuntimeError::SchedulingLimit)));
        }
        drop(peer);
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    }
}

#[cfg_attr(test, test)]
fn module_code_storage_terminal_document_refuses_preparation() {
    let vm = page();
    vm.host_scheduling.as_ref().unwrap().fail();
    assert!(matches!(
        PreparedModuleCode::synthetic(vm.host_scheduling.as_ref()),
        Err(RuntimeError::SchedulingLimit)
    ));
}

#[cfg_attr(test, test)]
fn module_code_storage_metadata_preparation_uses_no_managed_storage() {
    let mut vm = page();
    vm.heap.allow_only(0);
    let before = vm.heap.stats();
    let code = PreparedModuleCode::synthetic(vm.host_scheduling.as_ref()).unwrap();
    assert_eq!(vm.heap.stats(), before);
    assert!(code.bytes() > 0);
}

#[cfg_attr(test, test)]
fn module_code_storage_full_record_owns_metadata_and_value_separately() {
    let mut vm = resource_page();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let code = vm
        .create_synthetic_module_code(
            "value\0text",
            SyntheticModuleType::Text,
            &mut ModuleRoots::default(),
        )
        .unwrap();
    let value = code.synthetic_default_export.as_ref().unwrap();
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes - before,
        code.bytes() + value.retained_bytes()
    );
    assert_eq!(value.value(), &Value::String("tiny".into()));
    drop(code);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn module_code_storage_full_record_survives_vm_destruction() {
    let mut vm = resource_page();
    let observer = Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    };
    let code = vm
        .create_synthetic_module_code(
            "value\0text",
            SyntheticModuleType::Text,
            &mut ModuleRoots::default(),
        )
        .unwrap();
    let bytes = code.bytes()
        + code
            .synthetic_default_export
            .as_ref()
            .unwrap()
            .retained_bytes();
    drop(vm);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, bytes);
    drop(code);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn module_code_storage_missing_source_validation_precedes_metadata_admission() {
    for kind in [
        SyntheticModuleType::Json,
        SyntheticModuleType::Text,
        SyntheticModuleType::Bytes,
    ] {
        let mut vm = page();
        let _peer = leave_room(&mut vm, 0);
        let heap = vm.heap.stats();
        assert!(matches!(
            vm.create_synthetic_module_code("missing", kind, &mut ModuleRoots::default()),
            Err(RuntimeError::TypeError(_))
        ));
        assert_eq!(vm.check_scheduling_failure(), Ok(()));
        assert_eq!(vm.heap.stats(), heap);
    }
}

#[cfg_attr(test, test)]
fn module_code_storage_exact_registry_adoption_avoids_duplicate_code_charge() {
    let metadata = PreparedModuleCode::synthetic(None).unwrap().bytes();
    let mut probe = draft();
    probe
        .insert_prepared("new", PreparedModuleCode::synthetic(None).unwrap(), None)
        .unwrap();
    let bytes = probe.finish().retained_bytes();
    let extra = bytes - metadata;
    for accepts in [false, true] {
        let mut vm = page();
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        let code = PreparedModuleCode::synthetic(vm.host_scheduling.as_ref()).unwrap();
        let peer = leave_room(&mut vm, extra - usize::from(!accepts));
        let mut modules = draft();
        let result = modules.insert_prepared("new", code, vm.host_scheduling.as_ref());
        if accepts {
            assert_eq!(result, Ok(()));
            assert_eq!(modules.finish().retained_bytes(), bytes);
        } else {
            assert_eq!(result, Err(RuntimeError::SchedulingLimit));
            assert!(modules.is_empty());
            drop(modules);
        }
        drop(peer);
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    }
}

#[cfg_attr(test, test)]
fn module_code_storage_spare_registry_publication_succeeds_at_full_quota() {
    let mut vm = page();
    let mut modules = draft();
    modules
        .insert("", Bytecode::empty(), vm.host_scheduling.as_ref())
        .unwrap();
    let code = PreparedModuleCode::synthetic(vm.host_scheduling.as_ref()).unwrap();
    let pointer = code.bindings.as_ptr();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        modules.insert_prepared("", code, vm.host_scheduling.as_ref()),
        Ok(())
    );
    assert_eq!(modules[""].bindings.as_ptr(), pointer);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}

#[cfg_attr(test, test)]
fn module_code_storage_publication_moves_all_metadata_buffers() {
    let vm = page();
    let code = PreparedModuleCode::synthetic(vm.host_scheduling.as_ref()).unwrap();
    let pointers = |code: &Bytecode| {
        let ModuleExport::Local { export_name, .. } = &code.module_exports[0] else {
            unreachable!()
        };
        [
            code.bindings.as_ptr() as usize,
            code.bindings[0].name.as_ptr() as usize,
            code.scopes.as_ptr() as usize,
            code.scopes[0].as_ptr() as usize,
            code.module_exports.as_ptr() as usize,
            export_name.as_ptr() as usize,
        ]
    };
    let expected = pointers(&code);
    let mut modules = draft();
    modules
        .insert_prepared("new", code, vm.host_scheduling.as_ref())
        .unwrap();
    assert_eq!(pointers(&modules["new"]), expected);
}

#[cfg_attr(test, test)]
fn module_code_storage_foreign_document_cannot_adopt_prepared_buffers() {
    let owner = page();
    let other = page();
    let before = owner.host_scheduling_snapshot().payload_bytes;
    let code = PreparedModuleCode::synthetic(owner.host_scheduling.as_ref()).unwrap();
    let mut modules = draft();
    assert_eq!(
        modules.insert_prepared("new", code, other.host_scheduling.as_ref()),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(modules.is_empty());
    assert_eq!(owner.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(owner.check_scheduling_failure(), Ok(()));
    assert_eq!(
        other.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn module_code_storage_bounded_buffers_cannot_escape_to_unbounded_registry() {
    let owner = page();
    let before = owner.host_scheduling_snapshot().payload_bytes;
    let code = PreparedModuleCode::synthetic(owner.host_scheduling.as_ref()).unwrap();
    let mut modules = draft();
    assert_eq!(
        modules.insert_prepared("new", code, None),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(modules.is_empty());
    assert_eq!(owner.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(
        owner.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn module_code_storage_unbounded_input_attaches_on_registry_adoption() {
    let vm = page();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let mut modules = draft();
    modules
        .insert_prepared(
            "new",
            PreparedModuleCode::synthetic(None).unwrap(),
            vm.host_scheduling.as_ref(),
        )
        .unwrap();
    let registry = modules.finish();
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes - before,
        registry.retained_bytes()
    );
    drop(registry);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn module_code_storage_table_growth_refusal_preserves_existing_entries() {
    let mut vm = page();
    let mut modules = draft();
    for key in ["a", "b", "c"] {
        modules
            .insert(key, Bytecode::empty(), vm.host_scheduling.as_ref())
            .unwrap();
    }
    assert_eq!(modules.capacity(), 3);
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let pointer = modules.get_key_value("a").unwrap().0.as_ptr();
    let code = PreparedModuleCode::synthetic(vm.host_scheduling.as_ref()).unwrap();
    let peer = leave_room(&mut vm, 1);
    assert_eq!(
        modules.insert_prepared("d", code, vm.host_scheduling.as_ref()),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(modules.len(), 3);
    assert_eq!(modules.capacity(), 3);
    assert_eq!(modules.get_key_value("a").unwrap().0.as_ptr(), pointer);
    drop(peer);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn module_code_storage_snapshot_copy_refusal_preserves_source() {
    let mut vm = page();
    let source = ModuleRegistry::new(
        HashMap::from([("old".into(), Bytecode::empty())]),
        vm.host_scheduling.as_ref(),
    )
    .unwrap();
    let pointer = source.keys().next().unwrap().as_ptr();
    let mut modules = ModuleRegistryDraft::new(source.clone());
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let code = PreparedModuleCode::synthetic(vm.host_scheduling.as_ref()).unwrap();
    let peer = leave_room(&mut vm, 3);
    assert_eq!(
        modules.insert_prepared("new", code, vm.host_scheduling.as_ref()),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(modules.len(), 1);
    assert_eq!(modules.keys().next().unwrap().as_ptr(), pointer);
    drop(peer);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(modules.finish().keys().next().unwrap().as_ptr(), pointer);
}

#[cfg_attr(test, test)]
fn module_code_storage_replacing_code_refunds_old_buffers_and_keeps_key() {
    let vm = page();
    let mut modules = draft();
    let code = PreparedModuleCode::synthetic(vm.host_scheduling.as_ref()).unwrap();
    let bytes = code.bytes();
    modules
        .insert_prepared("slot", code, vm.host_scheduling.as_ref())
        .unwrap();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let pointer = modules.keys().next().unwrap().as_ptr();
    modules
        .insert("slot", Bytecode::empty(), vm.host_scheduling.as_ref())
        .unwrap();
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before - bytes);
    assert_eq!(modules.keys().next().unwrap().as_ptr(), pointer);
}

#[cfg_attr(test, test)]
fn module_code_storage_registry_snapshots_keep_one_charge_after_vm_drop() {
    let vm = page();
    let observer = Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    };
    let mut modules = draft();
    modules
        .insert_prepared(
            "new",
            PreparedModuleCode::synthetic(vm.host_scheduling.as_ref()).unwrap(),
            vm.host_scheduling.as_ref(),
        )
        .unwrap();
    let registry = modules.finish();
    let bytes = registry.retained_bytes();
    let snapshot = registry.clone();
    drop(vm);
    drop(registry);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, bytes);
    drop(snapshot);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn module_code_storage_root_refusal_refunds_code_preparation() {
    let mut vm = resource_page();
    // JSON construction retains one admitted operand after the failure.
    vm.stack.try_reserve_exact(1).unwrap();
    vm.collect_realm_garbage().unwrap();
    vm.host_scheduling_usage().unwrap();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    vm.heap.allow_root_registrations(0);
    let mut roots = ModuleRoots::default();
    assert!(matches!(
        vm.create_synthetic_module_code("value\0json", SyntheticModuleType::Json, &mut roots),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    ));
    assert!(roots.is_empty());
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(vm.check_scheduling_failure(), Ok(()));
}

#[cfg_attr(test, test)]
fn module_code_storage_managed_refusal_preserves_error_and_allows_retry() {
    let mut vm = resource_page();
    vm.collect_realm_garbage().unwrap();
    vm.heap.allow_only(0);
    let mut roots = ModuleRoots::default();
    assert!(matches!(
        vm.create_synthetic_module_code("value\0json", SyntheticModuleType::Json, &mut roots),
        Err(RuntimeError::Heap(_))
    ));
    assert!(roots.is_empty());
    vm.heap.allow_only(4096);
    assert!(vm
        .create_synthetic_module_code("value\0json", SyntheticModuleType::Json, &mut roots)
        .is_ok());
}

#[cfg_attr(test, test)]
fn module_code_storage_instruction_failure_refunds_native_metadata() {
    let mut vm = resource_page();
    // Primitive JSON performs no charge_step; one property does.
    vm.set_json_module_sources(HashMap::from([("number".into(), "{\"answer\":42}".into())]));
    vm.stack.try_reserve_exact(1).unwrap();
    vm.collect_realm_garbage().unwrap();
    vm.remaining_instructions = 0;
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    assert!(matches!(
        vm.create_synthetic_module_code(
            "number\0json",
            SyntheticModuleType::Json,
            &mut ModuleRoots::default()
        ),
        Err(RuntimeError::InstructionLimit)
    ));
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn module_code_storage_host_neutral_record_keeps_its_metadata_and_value() {
    let mut vm = Vm::default();
    vm.set_text_module_sources(HashMap::from([("value".into(), "tiny".into())]));
    let code = vm
        .create_synthetic_module_code(
            "value\0text",
            SyntheticModuleType::Text,
            &mut ModuleRoots::default(),
        )
        .unwrap();
    assert!(code.module && code.strict);
    assert!(code.charge.is_none());
    let mut modules = draft();
    modules.insert_prepared("value\0text", code, None).unwrap();
    assert_eq!(
        modules["value\0text"]
            .synthetic_default_export
            .as_ref()
            .unwrap()
            .value(),
        &Value::String("tiny".into())
    );
}

#[cfg_attr(test, test)]
fn module_code_storage_rooted_object_survives_prepared_code_collection() {
    let mut vm = resource_page();
    let mut roots = ModuleRoots::default();
    let code = vm
        .create_synthetic_module_code("value\0json", SyntheticModuleType::Json, &mut roots)
        .unwrap();
    let object = code
        .synthetic_default_export
        .as_ref()
        .unwrap()
        .value()
        .object_id()
        .unwrap();
    let pointer = code.bindings.as_ptr();
    vm.collect_realm_garbage().unwrap();
    assert!(vm.heap.contains(object));
    assert_eq!(code.bindings.as_ptr(), pointer);
    drop(code);
    vm.heap.unroot(roots.pop().unwrap()).unwrap();
    vm.collect_realm_garbage().unwrap();
    assert!(!vm.heap.contains(object));
}

impl crate::Vm {
    /// Runs finite metadata preparation and registry ownership contracts.
    #[doc(hidden)]
    pub fn verify_module_code_storage_contracts() {
        module_code_storage_retained_metadata_capacities_join_document_usage();
        module_code_storage_detached_code_keeps_ownership_after_vm_drop();
        module_code_storage_metadata_keeps_synthetic_record_shape();
        module_code_storage_metadata_exact_and_one_short_room();
        module_code_storage_terminal_document_refuses_preparation();
        module_code_storage_metadata_preparation_uses_no_managed_storage();
        module_code_storage_full_record_owns_metadata_and_value_separately();
        module_code_storage_full_record_survives_vm_destruction();
        module_code_storage_missing_source_validation_precedes_metadata_admission();
        module_code_storage_exact_registry_adoption_avoids_duplicate_code_charge();
        module_code_storage_spare_registry_publication_succeeds_at_full_quota();
        module_code_storage_publication_moves_all_metadata_buffers();
        module_code_storage_foreign_document_cannot_adopt_prepared_buffers();
        module_code_storage_bounded_buffers_cannot_escape_to_unbounded_registry();
        module_code_storage_unbounded_input_attaches_on_registry_adoption();
        module_code_storage_table_growth_refusal_preserves_existing_entries();
        module_code_storage_snapshot_copy_refusal_preserves_source();
        module_code_storage_replacing_code_refunds_old_buffers_and_keeps_key();
        module_code_storage_registry_snapshots_keep_one_charge_after_vm_drop();
        module_code_storage_root_refusal_refunds_code_preparation();
        module_code_storage_managed_refusal_preserves_error_and_allows_retry();
        module_code_storage_instruction_failure_refunds_native_metadata();
        module_code_storage_host_neutral_record_keeps_its_metadata_and_value();
        module_code_storage_rooted_object_survives_prepared_code_collection();
    }
}
