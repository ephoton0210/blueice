// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny compiled registries retain one admitted immutable snapshot.

use super::*;

fn bounded_vm() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}

fn module() -> Bytecode {
    crate::compile_module(&crate::parse_module("export const answer='tiny';").unwrap()).unwrap()
}

#[cfg_attr(test, test)]
fn module_registry_ownership_host_install_charges_retained_root_storage() {
    let mut vm = bounded_vm();
    let code = module();
    let minimum = code.code.capacity()
        + code.constants.capacity() * std::mem::size_of::<Value>()
        + std::mem::size_of::<(String, Bytecode)>();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.set_module_loader_context("entry", HashMap::from([("entry".into(), code)]));
    assert!(vm.host_scheduling_snapshot().payload_bytes >= before + minimum);
}

#[cfg_attr(test, test)]
fn module_registry_ownership_refused_replacement_preserves_the_registry_and_referrer() {
    let mut vm = bounded_vm();
    vm.set_module_loader_context("kept", HashMap::from([("kept".into(), module())]));
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    let _peer = vm
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used)
        .unwrap();
    vm.set_module_loader_context("blocked", HashMap::from([("blocked".into(), module())]));
    assert_eq!(vm.active_module_name.as_deref(), Some("kept"));
    assert!(vm.module_registry.contains_key("kept"));
    assert!(!vm.module_registry.contains_key("blocked"));
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn module_registry_ownership_snapshots_share_compiled_root_buffers() {
    let mut vm = bounded_vm();
    vm.set_module_loader_context("entry", HashMap::from([("entry".into(), module())]));
    let snapshot = vm.module_registry.clone();
    assert_eq!(
        snapshot["entry"].code.as_ptr(),
        vm.module_registry["entry"].code.as_ptr()
    );
}

#[cfg_attr(test, test)]
fn module_registry_ownership_late_activation_counts_the_referrer_before_peer_admission() {
    let mut vm = Vm::default();
    vm.set_module_loader_context("entry", HashMap::from([("entry".into(), module())]));
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let actual = vm.host_scheduling_snapshot().payload_bytes;
    assert!(vm
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - actual + 1)
        .is_err());
}

#[cfg_attr(test, test)]
fn module_registry_ownership_activation_refuses_existing_scalar_ownership_and_stays_terminal() {
    let mut vm = Vm::default();
    vm.set_module_loader_context("entry", HashMap::new());
    // Only a scalar owner is represented; no matching large buffer exists.
    vm.displaced_execution_control_bytes
        .set(MAX_HOST_SCHEDULING_BYTES);
    assert_eq!(
        vm.use_bounded_host_microtask_checkpoints(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
    let mut owner = bounded_vm();
    owner.set_module_loader_context("entry", HashMap::from([("entry".into(), module())]));
    let before = owner.host_scheduling_usage().unwrap().payload_bytes;
    let mut other = bounded_vm();
    other.module_registry = owner.module_registry.clone();
    assert!(matches!(
        other.use_bounded_host_microtask_checkpoints(),
        Err(RuntimeError::TypeError(_))
    ));
    assert_eq!(
        other.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
    other.set_module_loader_context("", HashMap::new());
    assert_eq!(owner.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(owner.check_scheduling_failure(), Ok(()));
}

fn leave_room(vm: &mut Vm, room: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - room)
        .unwrap()
}
fn compiled(source: &str) -> Bytecode {
    crate::compile_module(&crate::parse_module(source).unwrap()).unwrap()
}
fn script(source: &str) -> Bytecode {
    crate::compile(&crate::parse(source).unwrap()).unwrap()
}
fn empty_code() -> Bytecode {
    let mut code = Bytecode::empty();
    code.module = true;
    code
}

#[cfg_attr(test, test)]
fn module_registry_ownership_counts_reserved_root_buffers_including_diagnostics_and_constants() {
    let mut vm = bounded_vm();
    let mut code = empty_code();
    code.code = Vec::with_capacity(32);
    code.diagnostic_ast_owners = Vec::with_capacity(4);
    code.constants = Vec::with_capacity(4);
    let mut units = Vec::with_capacity(8);
    units.extend([0x61, 0x62]);
    code.constants
        .push(Value::String(JsString::from_code_units(units)));
    let mut key = String::with_capacity(24);
    key.push_str("entry");
    let data = HashMap::from([(key, code)]);
    let expected = header_bytes()
        + map_slots(&data)
        + 24
        + 32
        + 4 * std::mem::size_of::<crate::diagnostics::DiagnosticInstructionOwner>()
        + 4 * std::mem::size_of::<Value>()
        + 16;
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.set_module_loader_context("", data);
    assert_eq!(vm.module_registry.retained_bytes(), expected);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        before + expected
    );
}

#[cfg_attr(test, test)]
fn module_registry_ownership_host_replacement_exact_peak_and_one_byte_refusal() {
    for accepts in [false, true] {
        let mut vm = bounded_vm();
        vm.set_module_loader_context("kept", HashMap::from([("kept".into(), module())]));
        let old = vm.module_registry.retained_bytes()
            + vm.active_module_name.as_ref().unwrap().capacity();
        let pointer = vm.module_registry["kept"].code.as_ptr();
        let data = HashMap::from([("next".into(), module())]);
        let name = String::from("next");
        let incoming = retained_bytes(&data, payload_bytes(&data)) + name.capacity();
        let _peer = leave_room(&mut vm, incoming - usize::from(!accepts));
        let before = vm.host_scheduling_snapshot().payload_bytes;
        vm.set_module_loader_context(name, data);
        if accepts {
            assert_eq!(vm.active_module_name.as_deref(), Some("next"));
            assert!(vm.module_registry.contains_key("next"));
            assert_eq!(
                vm.host_scheduling_snapshot().payload_bytes,
                before + incoming - old
            );
        } else {
            assert_eq!(vm.active_module_name.as_deref(), Some("kept"));
            assert_eq!(vm.module_registry["kept"].code.as_ptr(), pointer);
            assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
        }
    }
}

#[cfg_attr(test, test)]
fn module_registry_ownership_full_budget_snapshots_refund_only_after_the_last_owner() {
    let mut vm = bounded_vm();
    vm.set_module_loader_context("", HashMap::from([("entry".into(), module())]));
    let bytes = vm.module_registry.retained_bytes();
    let peer = leave_room(&mut vm, 0);
    let snapshot = vm.module_registry.clone();
    vm.set_module_loader_context("", HashMap::new());
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
    drop(peer);
    vm.host_scheduling.as_ref().unwrap().fail();
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        bytes + vm.heap.root_storage_bytes()
    );
    drop(snapshot);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        vm.heap.root_storage_bytes()
    );
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn module_registry_ownership_empty_reserved_tables_retain_their_charge() {
    let mut vm = bounded_vm();
    let data = HashMap::with_capacity(4);
    let expected = header_bytes() + map_slots(&data) + vm.heap.root_storage_bytes();
    vm.set_module_loader_context("", data);
    let snapshot = vm.module_registry.clone();
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, expected);
    vm.host_scheduling.as_ref().unwrap().fail();
    vm.set_module_loader_context("", HashMap::new());
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, expected);
    drop(snapshot);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        vm.heap.root_storage_bytes()
    );
}

#[cfg_attr(test, test)]
fn module_registry_ownership_borrowed_graph_refusal_preserves_fields_before_heap_effects() {
    for minimum_only in [false, true] {
        let mut vm = bounded_vm();
        vm.stack.push(Value::Number(7.0));
        let data = HashMap::from([("entry".into(), module())]);
        let room = if minimum_only {
            header_bytes() + payload_bytes(&data) + std::mem::size_of::<(String, Bytecode)>()
        } else {
            0
        };
        let _peer = leave_room(&mut vm, room);
        let before = vm.heap.stats();
        assert_eq!(
            vm.execute_module_graph("entry", &data),
            Err(RuntimeError::SchedulingLimit)
        );
        assert!(vm.module_registry.is_empty() && vm.module_graph.is_none());
        assert_eq!(vm.stack, vec![Value::Number(7.0)]);
        assert_eq!(vm.heap.stats().managed_bytes, before.managed_bytes);
    }
}

#[cfg_attr(test, test)]
fn module_registry_ownership_unchanged_graph_execution_keeps_the_same_snapshot() {
    let mut vm = bounded_vm();
    vm.set_module_loader_context("entry", HashMap::from([("entry".into(), module())]));
    let snapshot = vm.module_registry.clone();
    let pointer = snapshot["entry"].code.as_ptr();
    vm.execute_module_registry_inner("entry", &snapshot, false, false, ImportPhase::Evaluation)
        .unwrap();
    assert_eq!(vm.module_registry["entry"].code.as_ptr(), pointer);
    assert_eq!(
        vm.module_registry.retained_bytes(),
        snapshot.retained_bytes()
    );
}

#[cfg_attr(test, test)]
fn module_registry_ownership_drafts_charge_independent_buffers_and_preserve_the_source() {
    let mut vm = bounded_vm();
    vm.set_module_loader_context("", HashMap::from([("entry".into(), module())]));
    let pointer = vm.module_registry["entry"].code.as_ptr();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let mut draft = ModuleRegistryDraft::new(vm.module_registry.clone());
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    draft
        .insert("new", empty_code(), vm.host_scheduling.as_ref())
        .unwrap();
    draft
        .insert("new", module(), vm.host_scheduling.as_ref())
        .unwrap();
    assert_eq!(vm.module_registry["entry"].code.as_ptr(), pointer);
    assert_ne!(draft["entry"].code.as_ptr(), pointer);
    let actual = retained_bytes(&draft, payload_bytes(&draft));
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before + actual);
    let snapshot = draft.finish();
    assert_eq!(snapshot.retained_bytes(), actual);
    drop(snapshot);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn module_registry_ownership_draft_growth_moves_entries_and_preserves_a_refused_table() {
    for accepts in [false, true] {
        let mut vm = bounded_vm();
        let mut draft = ModuleRegistryDraft::new(ModuleRegistry::default());
        for index in 0..8 {
            if !draft.is_empty() && draft.len() == draft.capacity() {
                break;
            }
            draft
                .insert(&index.to_string(), module(), vm.host_scheduling.as_ref())
                .unwrap();
        }
        assert_eq!(draft.len(), draft.capacity());
        let capacity = draft.capacity();
        let pointer = draft["0"].code.as_ptr();
        let mut probe = HashMap::<String, Bytecode>::new();
        probe.try_reserve(draft.len() + 1).unwrap();
        let new_table = map_slots(&probe);
        let old_table = map_slots(&draft);
        drop(probe);
        let _peer = leave_room(&mut vm, new_table + 5 - usize::from(!accepts));
        let before = vm.host_scheduling_snapshot().payload_bytes;
        let result = draft.insert("extra", empty_code(), vm.host_scheduling.as_ref());
        if accepts {
            result.unwrap();
            assert!(draft.contains_key("extra"));
            assert_eq!(
                vm.host_scheduling_snapshot().payload_bytes,
                before + 5 + new_table - old_table
            );
        } else {
            assert_eq!(result, Err(RuntimeError::SchedulingLimit));
            assert_eq!(draft.capacity(), capacity);
            assert!(!draft.contains_key("extra"));
            assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
        }
        assert_eq!(draft["0"].code.as_ptr(), pointer);
    }
}

fn loaded_graph() -> Vm {
    let mut vm = bounded_vm();
    vm.set_module_loader_context("kept", HashMap::from([("kept".into(), module())]));
    let snapshot = vm.module_registry.clone();
    vm.execute_module_registry_inner("kept", &snapshot, false, false, ImportPhase::Evaluation)
        .unwrap();
    vm
}

#[cfg_attr(test, test)]
fn module_registry_ownership_refused_preparation_releases_its_new_synthetic_roots() {
    let mut vm = loaded_graph();
    vm.stack.reserve(16);
    vm.set_json_module_sources(HashMap::from([("json".into(), "{}".into())]));
    let roots = vm.module_graph.as_ref().unwrap().roots.snapshot();
    let pointer = vm.module_registry["kept"].code.as_ptr();
    // Reserve a collector slot for the tiny synthetic object before filling
    // the separate converter/registry allowance. The dummy is not retained.
    vm.with_roots(|heap| {
        heap.alloc_object(None)?;
        heap.try_collect_major()
    })
    .unwrap();
    let _peer = leave_room(&mut vm, SyntheticModuleValue::header_bytes());
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let managed = vm.heap.stats().managed_bytes;
    let snapshot = vm.module_registry.clone();
    assert_eq!(
        vm.execute_module_registry_inner(
            "json\0json",
            &snapshot,
            false,
            true,
            ImportPhase::Evaluation
        ),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.module_graph.as_ref().unwrap().roots.snapshot(), roots);
    assert_eq!(vm.module_registry["kept"].code.as_ptr(), pointer);
    assert!(!vm.module_registry.contains_key("json\0json"));
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    assert_eq!(vm.heap.stats().managed_bytes, managed);
}

#[cfg_attr(test, test)]
fn module_registry_ownership_linking_failure_restores_the_previous_cache_and_root_prefix() {
    let mut vm = loaded_graph();
    vm.set_json_module_sources(HashMap::from([("json".into(), "{}".into())]));
    let data = HashMap::from([
        ("kept".into(), module()),
        (
            "main".into(),
            compiled(
                "import value from './json' with {type:'json'}; import {missing} from './kept'; 0;",
            ),
        ),
    ]);
    vm.set_module_loader_context("kept", data);
    let snapshot = vm.module_registry.clone();
    let pointer = snapshot["main"].code.as_ptr();
    let roots = vm.module_graph.as_ref().unwrap().roots.snapshot();
    let managed = vm.heap.stats().managed_bytes;
    assert!(matches!(
        vm.execute_module_registry_inner("main", &snapshot, false, false, ImportPhase::Evaluation),
        Err(RuntimeError::ModuleResolution(_))
    ));
    assert_eq!(vm.module_registry["main"].code.as_ptr(), pointer);
    assert!(!vm.module_registry.contains_key("json\0json"));
    assert_eq!(vm.module_graph.as_ref().unwrap().roots.snapshot(), roots);
    assert_eq!(vm.module_graph.as_ref().unwrap().linked.len(), 1);
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    assert_eq!(vm.heap.stats().managed_bytes, managed);
}

#[cfg_attr(test, test)]
fn module_registry_ownership_shadow_import_shares_unchanged_code_until_the_last_realm_drops() {
    let mut vm = bounded_vm();
    vm.set_module_loader_context(
        "page/main",
        HashMap::from([(
            "page/entry".into(),
            compiled("export function answer(){return 42;}"),
        )]),
    );
    let pointer = vm.module_registry["page/entry"].code.as_ptr();
    let realm = vm.execute_script(&script("new ShadowRealm();")).unwrap();
    vm.shadow_realm_import_value(
        realm.clone(),
        Value::String("./entry".into()),
        Value::String("answer".into()),
    )
    .unwrap();
    let record = vm.shadow_realms[&realm.object_id().unwrap()].clone();
    assert_eq!(
        record.vm.borrow().module_registry["page/entry"]
            .code
            .as_ptr(),
        pointer
    );
    vm.set_module_loader_context("", HashMap::new());
    assert!(vm.host_scheduling_snapshot().payload_bytes > vm.own_scheduling_usage().payload_bytes);
    drop(record);
    vm.execute_script(&script("0;")).unwrap();
    assert!(vm.shadow_realms.is_empty());
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        vm.own_scheduling_usage().payload_bytes
            + vm.heap.collector_storage_bytes()
            + vm.heap.root_storage_bytes()
    );
}

fn with_cached_json() -> Vm {
    let mut vm = bounded_vm();
    vm.set_module_loader_context(
        "page/main",
        HashMap::from([(
            "page/entry".into(),
            compiled("export function answer(){return 42;}"),
        )]),
    );
    vm.set_json_module_sources(HashMap::from([("page/data".into(), "{}".into())]));
    vm.execute_script(&script(
        "var hold;import('./data',{with:{type:'json'}}).then(ns=>hold=ns.default);0;",
    ))
    .unwrap();
    for _ in 0..32 {
        if !vm.run_next_promise_job().unwrap() {
            break;
        }
    }
    assert!(vm.promise_jobs.is_empty());
    vm
}

#[cfg_attr(test, test)]
fn module_registry_ownership_shadow_filter_preserves_parent_object_records() {
    let mut vm = with_cached_json();
    let parent = vm.module_registry["page/data\0json"]
        .synthetic_default_export
        .as_ref()
        .unwrap()
        .value()
        .object_id()
        .unwrap();
    let realm = vm.execute_script(&script("new ShadowRealm();")).unwrap();
    vm.shadow_realm_import_value(
        realm.clone(),
        Value::String("./entry".into()),
        Value::String("answer".into()),
    )
    .unwrap();
    let child = vm.shadow_realms[&realm.object_id().unwrap()].vm.borrow();
    assert!(!child.module_registry.contains_key("page/data\0json"));
    assert!(vm.module_registry.contains_key("page/data\0json"));
    assert!(vm.heap.contains(parent));
}

#[cfg_attr(test, test)]
fn module_registry_ownership_shadow_filter_refusal_precedes_capability_and_refunds_the_referrer() {
    let mut vm = with_cached_json();
    let realm = vm.execute_script(&script("new ShadowRealm();")).unwrap();
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    let record = vm.shadow_realms[&realm.object_id().unwrap()].clone();
    let room = vm.active_module_name.as_ref().unwrap().len();
    let _peer = leave_room(&mut vm, room);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let promises = vm.promises.len();
    assert_eq!(
        vm.shadow_realm_import_value(
            realm,
            Value::String("./entry".into()),
            Value::String("answer".into())
        ),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.promises.len(), promises);
    assert!(record.vm.borrow().module_registry.is_empty());
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(
        record.vm.borrow().host_scheduling_snapshot().payload_bytes,
        before
    );
}

#[cfg_attr(test, test)]
fn module_registry_ownership_clone_admission_uses_the_new_buffers_instead_of_unused_source_capacity(
) {
    let mut vm = bounded_vm();
    let mut code = empty_code();
    code.code.reserve(64);
    code.diagnostic_ast_owners.reserve(8);
    let mut key = String::with_capacity(24);
    key.push_str("entry");
    let data = HashMap::from([(key, code)]);
    let mut probe = HashMap::<String, Bytecode>::new();
    probe.try_reserve(1).unwrap();
    let expected = header_bytes() + map_slots(&probe) + 5;
    let _peer = leave_room(&mut vm, expected);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let snapshot = ModuleRegistry::copy_host(&data, vm.host_scheduling.as_ref())
        .expect("empty cloned vectors do not retain the source's spare capacity");
    assert_eq!(snapshot.retained_bytes(), expected);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        before + expected
    );
    drop(snapshot);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

impl crate::Vm {
    /// Runs finite compiled-registry ownership contracts in unit/coverage builds.
    #[doc(hidden)]
    pub fn verify_module_registry_ownership_contracts() {
        module_registry_ownership_host_install_charges_retained_root_storage();
        module_registry_ownership_refused_replacement_preserves_the_registry_and_referrer();
        module_registry_ownership_snapshots_share_compiled_root_buffers();
        module_registry_ownership_late_activation_counts_the_referrer_before_peer_admission();
        module_registry_ownership_activation_refuses_existing_scalar_ownership_and_stays_terminal();
        module_registry_ownership_counts_reserved_root_buffers_including_diagnostics_and_constants(
        );
        module_registry_ownership_host_replacement_exact_peak_and_one_byte_refusal();
        module_registry_ownership_full_budget_snapshots_refund_only_after_the_last_owner();
        module_registry_ownership_empty_reserved_tables_retain_their_charge();
        module_registry_ownership_borrowed_graph_refusal_preserves_fields_before_heap_effects();
        module_registry_ownership_unchanged_graph_execution_keeps_the_same_snapshot();
        module_registry_ownership_drafts_charge_independent_buffers_and_preserve_the_source();
        module_registry_ownership_draft_growth_moves_entries_and_preserves_a_refused_table();
        module_registry_ownership_refused_preparation_releases_its_new_synthetic_roots();
        module_registry_ownership_linking_failure_restores_the_previous_cache_and_root_prefix();
        module_registry_ownership_shadow_import_shares_unchanged_code_until_the_last_realm_drops();
        module_registry_ownership_shadow_filter_preserves_parent_object_records();
        module_registry_ownership_shadow_filter_refusal_precedes_capability_and_refunds_the_referrer();
        module_registry_ownership_clone_admission_uses_the_new_buffers_instead_of_unused_source_capacity();
    }
}
