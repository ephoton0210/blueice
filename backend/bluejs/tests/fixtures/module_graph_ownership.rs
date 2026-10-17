// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Small graphs and scalar peers verify record-table and name admission.

use super::*;
use crate::vm::host_scheduling::{map_slots, SharedSchedulingCharge};

fn module(source: &str) -> Bytecode {
    crate::compile_module(&crate::parse_module(source).unwrap()).unwrap()
}
fn bounded_vm() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}
fn load(vm: &mut Vm) {
    vm.set_module_loader_context(
        "entry",
        HashMap::from([("entry".into(), module("export let answer=42;"))]),
    );
    let snapshot = vm.module_registry.clone();
    vm.execute_module_registry_inner("entry", &snapshot, false, false, ImportPhase::Evaluation)
        .unwrap();
}
fn metadata(linked: &HashMap<String, LinkedModule>) -> usize {
    linked
        .keys()
        .fold(map_slots(linked), |bytes, name| bytes + name.capacity())
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
fn module_graph_ownership_retained_records_and_names_have_a_native_owner() {
    let mut vm = bounded_vm();
    load(&mut vm);
    let bytes = metadata(&vm.module_graph.as_ref().unwrap().linked);
    assert!(bytes > 0);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes
            - vm.own_scheduling_usage().payload_bytes
            - vm.module_value_storage_bytes()
            - vm.heap.collector_storage_bytes()
            - vm.module_registry.retained_bytes()
            - vm.module_root_storage_bytes(),
        bytes
    );
}

#[cfg_attr(test, test)]
fn module_graph_ownership_rust_held_table_retains_and_then_refunds_its_owner() {
    let mut vm = bounded_vm();
    load(&mut vm);
    let graph = vm.module_graph.take().unwrap();
    let bytes = metadata(&graph.linked);
    let values = graph
        .linked
        .values()
        .filter_map(|record| record.completion.as_ref())
        .map(|value| value.retained_bytes())
        .sum::<usize>();
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes
            - vm.own_scheduling_usage().payload_bytes
            - vm.module_value_storage_bytes()
            - vm.heap.collector_storage_bytes()
            - vm.module_registry.retained_bytes(),
        bytes + graph.roots.retained_bytes() + values
    );
    for root in &graph.roots {
        vm.heap.unroot(*root).unwrap();
    }
    drop(graph);
    vm.reconcile_scheduling_ownership();
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        vm.own_scheduling_usage().payload_bytes
            + vm.module_value_storage_bytes()
            + vm.heap.collector_storage_bytes()
            + vm.module_registry.retained_bytes()
    );
}

#[cfg_attr(test, test)]
fn module_graph_ownership_refuses_record_metadata_before_managed_cells() {
    let mut vm = bounded_vm();
    vm.set_module_loader_context(
        "entry",
        HashMap::from([("entry".into(), module("export let answer=42;"))]),
    );
    let snapshot = vm.module_registry.clone();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    let _peer = leave_room(&mut vm, 98);
    vm.heap.allow_only(0);
    assert_eq!(
        vm.execute_module_registry_inner("entry", &snapshot, false, false, ImportPhase::Evaluation),
        Err(RuntimeError::SchedulingLimit)
    );
}

fn record(vm: &Vm) -> LinkedModule {
    LinkedModule {
        cells: HashMap::new(),
        cells_charge: vm.charge_module_cells(0),
        namespace: None,
        deferred_namespace: None,
        evaluated: false,
        evaluating: false,
        suspended: false,
        completion: None,
        error: None,
        host_error_pending: false,
    }
}
fn records(vm: &Vm, count: usize) -> ModuleRecords {
    let mut linked = ModuleRecords::default();
    linked.reserve(count, vm.host_scheduling.as_ref()).unwrap();
    for index in 0..count {
        linked
            .insert_new(&index.to_string(), record(vm), vm.host_scheduling.as_ref())
            .unwrap();
    }
    linked
}

#[cfg_attr(test, test)]
fn module_graph_ownership_growth_admits_old_and_new_tables_and_preserves_refused_records() {
    for accepts in [false, true] {
        let mut vm = bounded_vm();
        let mut linked = records(&vm, 3);
        assert_eq!(linked.capacity(), 3);
        let old_table = map_slots(&linked);
        let key_pointer = linked
            .keys()
            .find(|key| key.as_str() == "0")
            .unwrap()
            .as_ptr();
        let record_pointer = linked.get("0").unwrap() as *const LinkedModule;
        let mut probe = HashMap::<String, LinkedModule>::new();
        probe.try_reserve(4).unwrap();
        let new_table = map_slots(&probe);
        let _peer = leave_room(&mut vm, new_table - usize::from(!accepts));
        let before = vm.host_scheduling_snapshot().payload_bytes;
        let result = linked.reserve(1, vm.host_scheduling.as_ref());
        if accepts {
            result.unwrap();
            assert_eq!(linked.capacity(), probe.capacity());
            assert_eq!(
                vm.host_scheduling_snapshot().payload_bytes,
                before + new_table - old_table
            );
        } else {
            assert_eq!(result, Err(RuntimeError::SchedulingLimit));
            assert_eq!(linked.capacity(), 3);
            assert_eq!(
                linked.get("0").unwrap() as *const LinkedModule,
                record_pointer
            );
            assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
        }
        assert_eq!(
            linked
                .keys()
                .find(|key| key.as_str() == "0")
                .unwrap()
                .as_ptr(),
            key_pointer
        );
    }
}

#[cfg_attr(test, test)]
fn module_graph_ownership_existing_slots_accept_an_empty_key_at_full_scalar_budget() {
    let mut vm = bounded_vm();
    let mut linked = ModuleRecords::default();
    linked.reserve(1, vm.host_scheduling.as_ref()).unwrap();
    let _peer = leave_room(&mut vm, 0);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    linked
        .insert_new("", record(&vm), vm.host_scheduling.as_ref())
        .unwrap();
    assert!(linked.contains_key(""));
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn module_graph_ownership_key_refusal_preserves_reserved_slots() {
    let mut vm = bounded_vm();
    let mut linked = ModuleRecords::default();
    linked.reserve(1, vm.host_scheduling.as_ref()).unwrap();
    let bytes = linked.retained_bytes();
    let _peer = leave_room(&mut vm, 4);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    assert_eq!(
        linked.insert_new("entry", record(&vm), vm.host_scheduling.as_ref()),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(linked.is_empty());
    assert_eq!(linked.retained_bytes(), bytes);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn module_graph_ownership_terminal_removal_refunds_names_and_drop_refunds_reserved_slots() {
    let mut vm = bounded_vm();
    let mut linked = records(&vm, 1);
    let table = map_slots(&linked);
    let _peer = leave_room(&mut vm, 0);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    vm.host_scheduling.as_ref().unwrap().fail();
    drop(linked.remove("0"));
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before - 1);
    assert_eq!(linked.retained_bytes(), table);
    drop(linked);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        before - 1 - table
    );
}

#[cfg_attr(test, test)]
fn module_graph_ownership_name_selection_borrows_sorted_keys_and_owns_only_vector_slots() {
    let mut vm = bounded_vm();
    let linked = ModuleRecords::default();
    let mut first = String::with_capacity(64);
    first.push('a');
    let source = HashMap::from([(first, module("0;")), ("z".into(), module("0;"))]);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let names = vm.prepare_module_names(&source, &linked).unwrap();
    assert_eq!(&*names, &["a", "z"]);
    for name in names.iter() {
        let source_key = source.keys().find(|key| key.as_str() == *name).unwrap();
        assert_eq!(name.as_ptr(), source_key.as_ptr());
    }
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        before + host_scheduling::slots(&names.names)
    );
    drop(names);
    vm.reconcile_scheduling_ownership();
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn module_graph_ownership_name_selection_refusal_keeps_the_existing_graph() {
    let mut vm = bounded_vm();
    load(&mut vm);
    let graph = vm.module_graph.take().unwrap();
    let pointer = graph.linked.get("entry").unwrap() as *const LinkedModule;
    let source = HashMap::from([("new".into(), module("0;"))]);
    let _peer = leave_room(&mut vm, std::mem::size_of::<&str>() - 1);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    assert!(matches!(
        vm.prepare_module_names(&source, &graph.linked),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert_eq!(
        graph.linked.get("entry").unwrap() as *const LinkedModule,
        pointer
    );
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    vm.module_graph = Some(graph);
}

#[cfg_attr(test, test)]
fn module_graph_ownership_late_activation_attaches_metadata_once() {
    let mut vm = Vm::default();
    load(&mut vm);
    let pointer = vm.linked_record("entry").unwrap() as *const LinkedModule;
    let bytes = metadata(&vm.module_graph.as_ref().unwrap().linked);
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    assert_eq!(
        before,
        vm.own_scheduling_usage().payload_bytes
            + vm.module_value_storage_bytes()
            + vm.heap.collector_storage_bytes()
            + vm.module_registry.retained_bytes()
            + bytes
            + vm.module_root_storage_bytes()
    );
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(
        vm.linked_record("entry").unwrap() as *const LinkedModule,
        pointer
    );
}

#[cfg_attr(test, test)]
fn module_graph_ownership_foreign_attachment_and_missing_scope_preserve_the_owner() {
    let vm = bounded_vm();
    let mut linked = records(&vm, 1);
    let foreign = bounded_vm();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    assert!(matches!(
        linked.attach(foreign.host_scheduling.as_ref().unwrap()),
        Err(RuntimeError::TypeError(_))
    ));
    assert_eq!(
        foreign.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(linked.reserve(1, None), Err(RuntimeError::SchedulingLimit));
    assert!(linked.contains_key("0"));
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn module_graph_ownership_linking_rollback_refunds_new_names_and_preserves_old_records() {
    let mut vm = bounded_vm();
    load(&mut vm);
    let bytes = vm.module_record_storage_bytes();
    let roots = vm.module_graph.as_ref().unwrap().roots.snapshot();
    let snapshot = ModuleRegistry::new(
        HashMap::from([
            ("entry".into(), module("export let answer=42;")),
            (
                "bad".into(),
                module("import {missing} from './entry';export let extra=1;"),
            ),
        ]),
        vm.host_scheduling.as_ref(),
    )
    .unwrap();
    assert!(matches!(
        vm.execute_module_registry_inner("bad", &snapshot, false, false, ImportPhase::Evaluation),
        Err(RuntimeError::ModuleResolution(_))
    ));
    assert_eq!(vm.module_record_storage_bytes(), bytes);
    assert_eq!(vm.module_graph.as_ref().unwrap().roots.snapshot(), roots);
    assert!(vm.linked_record("bad").is_none());
    let observer = Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Default::default()
    };
    assert_eq!(
        observer.host_scheduling_snapshot().payload_bytes,
        vm.own_scheduling_usage().payload_bytes
            + vm.module_value_storage_bytes()
            + vm.heap.collector_storage_bytes()
            + vm.module_registry.retained_bytes()
            + snapshot.retained_bytes()
            + bytes
            + vm.module_root_storage_bytes()
    );
}

#[cfg_attr(test, test)]
fn module_graph_ownership_fresh_link_failure_releases_all_record_owners() {
    let mut vm = bounded_vm();
    let snapshot = ModuleRegistry::new(
        HashMap::from([
            ("dep".into(), module("export let answer=42;")),
            (
                "bad".into(),
                module("import {missing} from './dep';export let extra=1;"),
            ),
        ]),
        vm.host_scheduling.as_ref(),
    )
    .unwrap();
    assert!(matches!(
        vm.execute_module_registry_inner("bad", &snapshot, false, false, ImportPhase::Evaluation),
        Err(RuntimeError::ModuleResolution(_))
    ));
    assert!(vm.module_graph.is_none());
    assert_eq!(vm.linked_module_cell_bytes.get(), 0);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    let observer = Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Default::default()
    };
    assert_eq!(
        observer.host_scheduling_snapshot().payload_bytes,
        vm.own_scheduling_usage().payload_bytes
            + vm.heap.collector_storage_bytes()
            + snapshot.retained_bytes()
    );
}

impl crate::Vm {
    /// Runs finite record-table contracts in unit and ordinary coverage builds.
    #[doc(hidden)]
    pub fn verify_module_graph_ownership_contracts() {
        module_graph_ownership_retained_records_and_names_have_a_native_owner();
        module_graph_ownership_rust_held_table_retains_and_then_refunds_its_owner();
        module_graph_ownership_refuses_record_metadata_before_managed_cells();
        module_graph_ownership_growth_admits_old_and_new_tables_and_preserves_refused_records();
        module_graph_ownership_existing_slots_accept_an_empty_key_at_full_scalar_budget();
        module_graph_ownership_key_refusal_preserves_reserved_slots();
        module_graph_ownership_terminal_removal_refunds_names_and_drop_refunds_reserved_slots();
        module_graph_ownership_name_selection_borrows_sorted_keys_and_owns_only_vector_slots();
        module_graph_ownership_name_selection_refusal_keeps_the_existing_graph();
        module_graph_ownership_late_activation_attaches_metadata_once();
        module_graph_ownership_foreign_attachment_and_missing_scope_preserve_the_owner();
        module_graph_ownership_linking_rollback_refunds_new_names_and_preserves_old_records();
        module_graph_ownership_fresh_link_failure_releases_all_record_owners();
    }
}
