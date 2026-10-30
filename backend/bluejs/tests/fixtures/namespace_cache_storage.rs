// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny namespace caches verify persistent native ownership and rollback.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}
fn compile(source: &str) -> Bytecode {
    crate::compile_module(&crate::parse_module(source).unwrap()).unwrap()
}
fn record() -> LinkedModule {
    LinkedModule {
        cells: Default::default(),
        cells_charge: Vm::default().charge_module_cells(0),
        namespace: None,
        deferred_namespace: None,
        namespace_epochs: [0; 2],
        evaluated: false,
        evaluating: false,
        suspended: false,
        completion: None,
        error: None,
        host_error_pending: false,
    }
}
fn leave_room(vm: &mut Vm, bytes: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - bytes)
        .unwrap()
}
#[cfg(test)]
#[test]
fn namespace_cache_storage_refusal_precedes_snapshot_and_name_scratch() {
    let mut vm = page();
    let modules = HashMap::from([("leaf".into(), compile(""))]);
    let mut linked = HashMap::from([("leaf".into(), record())]);
    let mut roots = ModuleRoots::default();
    let _peer = leave_room(&mut vm, 0);
    let (result, allocations) = allocation_observer::count(|| {
        vm.module_namespace("leaf", false, &modules, &mut linked, &mut roots)
    });
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(allocations, 0);
    assert!(roots.is_empty());
    assert!(vm.module_namespace_cache.is_empty());
    assert!(linked["leaf"].namespace.is_none());
}
#[cfg_attr(test, test)]
fn namespace_cache_storage_table_backing_is_charged_in_every_layer() {
    for layer in 0..5 {
        let mut vm = page();
        let before = vm.own_scheduling_usage().payload_bytes;
        let expected = match layer {
            0 => {
                vm.module_namespace_cache.reserve(1);
                vm.module_namespace_cache.capacity() * std::mem::size_of::<(String, ObjectId)>()
            }
            1 => {
                vm.module_namespace_roots.reserve(1);
                vm.module_namespace_roots.capacity() * std::mem::size_of::<(String, RootId)>()
            }
            2 => {
                vm.deferred_namespaces.reserve(1);
                vm.deferred_namespaces.capacity() * std::mem::size_of::<(ObjectId, String)>()
            }
            3 => {
                vm.module_deferred_namespace_cache.reserve(1);
                vm.module_deferred_namespace_cache.capacity()
                    * std::mem::size_of::<(String, ObjectId)>()
            }
            _ => {
                vm.module_deferred_namespace_roots.reserve(1);
                vm.module_deferred_namespace_roots.capacity()
                    * std::mem::size_of::<(String, RootId)>()
            }
        };
        assert!(expected > 0);
        assert_eq!(
            vm.own_scheduling_usage().payload_bytes - before,
            expected,
            "layer={layer}"
        );
    }
}
#[cfg_attr(test, test)]
fn namespace_cache_storage_name_capacity_is_charged_in_every_layer() {
    for layer in 0..5 {
        let mut vm = page();
        let object = vm.heap.alloc_object(None).unwrap();
        let root = vm.heap.root(object).unwrap();
        match layer {
            0 => vm.module_namespace_cache.reserve(1),
            1 => vm.module_namespace_roots.reserve(1),
            2 => vm.deferred_namespaces.reserve(1),
            3 => vm.module_deferred_namespace_cache.reserve(1),
            _ => vm.module_deferred_namespace_roots.reserve(1),
        }
        let before = vm.own_scheduling_usage().payload_bytes;
        let mut name = String::with_capacity(64);
        name.push_str("leaf");
        let expected = name.capacity();
        match layer {
            0 => {
                vm.module_namespace_cache.insert(name, object);
            }
            1 => {
                vm.module_namespace_roots.insert(name, root);
            }
            2 => {
                vm.deferred_namespaces.insert(object, name);
            }
            3 => {
                vm.module_deferred_namespace_cache.insert(name, object);
            }
            _ => {
                vm.module_deferred_namespace_roots.insert(name, root);
            }
        }
        assert_eq!(
            vm.own_scheduling_usage().payload_bytes - before,
            expected,
            "layer={layer}"
        );
    }
}

fn empty_graph(names: &[&str]) -> (HashMap<String, Bytecode>, HashMap<String, LinkedModule>) {
    (
        names
            .iter()
            .map(|name| ((*name).to_owned(), compile("")))
            .collect(),
        names
            .iter()
            .map(|name| ((*name).to_owned(), record()))
            .collect(),
    )
}
fn capacities(vm: &Vm) -> [usize; 5] {
    [
        vm.module_namespace_cache.capacity(),
        vm.module_namespace_roots.capacity(),
        vm.deferred_namespaces.capacity(),
        vm.module_deferred_namespace_cache.capacity(),
        vm.module_deferred_namespace_roots.capacity(),
    ]
}
fn grow_all(vm: &mut Vm) {
    vm.module_namespace_cache.reserve(8);
    vm.module_namespace_roots.reserve(8);
    vm.deferred_namespaces.reserve(8);
    vm.module_deferred_namespace_cache.reserve(8);
    vm.module_deferred_namespace_roots.reserve(8);
}
#[cfg_attr(test, test)]
fn namespace_cache_storage_eager_and_deferred_keep_distinct_stable_identities() {
    let mut vm = page();
    let (modules, mut linked) = empty_graph(&["leaf"]);
    let mut roots = ModuleRoots::default();
    let eager = vm
        .module_namespace("leaf", false, &modules, &mut linked, &mut roots)
        .unwrap();
    let deferred = vm
        .module_namespace("leaf", true, &modules, &mut linked, &mut roots)
        .unwrap();
    assert_ne!(eager, deferred);
    assert_eq!(linked["leaf"].namespace_epochs, [1, 2]);
    assert_eq!(vm.deferred_namespaces[&deferred], "leaf");
    assert_eq!(vm.module_namespace_roots.len(), 1);
    assert_eq!(vm.module_deferred_namespace_roots.len(), 1);
    let retained = vm.namespace_cache_bytes();
    let _peer = leave_room(&mut vm, 0);
    for (phase, id) in [(false, eager), (true, deferred)] {
        #[cfg(test)]
        {
            let (result, allocations) = allocation_observer::count(|| {
                vm.module_namespace("leaf", phase, &modules, &mut linked, &mut roots)
            });
            assert_eq!(result, Ok(id));
            assert_eq!(allocations, 0);
        }
        #[cfg(not(test))]
        assert_eq!(
            vm.module_namespace("leaf", phase, &modules, &mut linked, &mut roots),
            Ok(id)
        );
    }
    assert_eq!(vm.namespace_cache_bytes(), retained);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}
#[cfg_attr(test, test)]
fn namespace_cache_storage_preparation_preserves_graph_heap_and_roots() {
    let mut vm = page();
    let (modules, mut linked) = empty_graph(&["good", "new"]);
    let mut roots = ModuleRoots::default();
    vm.module_namespace("good", false, &modules, &mut linked, &mut roots)
        .unwrap();
    let before = capacities(&vm);
    let heap = vm.heap.stats();
    let root_count = roots.len();
    let epoch = vm.next_namespace_epoch;
    let prepared =
        namespace_cache_storage::PreparedNamespaceEntry::prepare(&vm, "new", true).unwrap();
    assert_eq!(capacities(&vm), before);
    assert_eq!(vm.heap.stats(), heap);
    assert_eq!(roots.len(), root_count);
    assert_eq!(vm.next_namespace_epoch, epoch);
    assert!(linked["new"].namespace.is_none());
    drop(prepared);
    assert_eq!(capacities(&vm), before);
}
#[cfg_attr(test, test)]
fn namespace_cache_storage_abandoned_preparations_refund_every_layer() {
    for deferred in [false, true] {
        let mut vm = page();
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        let prepared =
            namespace_cache_storage::PreparedNamespaceEntry::prepare(&vm, "leaf", deferred)
                .unwrap();
        assert!(vm.host_scheduling_snapshot().payload_bytes > before);
        drop(prepared);
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
        assert_eq!(vm.namespace_cache_bytes(), 0);
    }
}
#[cfg_attr(test, test)]
fn namespace_cache_storage_detached_preparation_survives_vm_destruction() {
    let mut vm = page();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let view = Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    };
    let prepared =
        namespace_cache_storage::PreparedNamespaceEntry::prepare(&vm, "leaf", true).unwrap();
    let held = vm.host_scheduling_snapshot().payload_bytes - before;
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, held);
    drop(prepared);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn namespace_cache_storage_exact_and_short_staging_preserve_existing_allocations() {
    for deferred in [false, true] {
        for short in [false, true] {
            let mut vm = page();
            let before = vm.host_scheduling_usage().unwrap().payload_bytes;
            let preview =
                namespace_cache_storage::PreparedNamespaceEntry::prepare(&vm, "leaf", deferred)
                    .unwrap();
            let stage = vm.host_scheduling_snapshot().payload_bytes - before;
            drop(preview);
            let shape = capacities(&vm);
            let heap = vm.heap.stats();
            let _peer = leave_room(&mut vm, stage - usize::from(short));
            let used = vm.host_scheduling_snapshot().payload_bytes;
            let result =
                namespace_cache_storage::PreparedNamespaceEntry::prepare(&vm, "leaf", deferred);
            if short {
                assert_eq!(result.err(), Some(RuntimeError::SchedulingLimit));
            } else {
                let value = result.unwrap();
                assert_eq!(
                    vm.host_scheduling_snapshot().payload_bytes,
                    MAX_HOST_SCHEDULING_BYTES
                );
                drop(value);
            }
            assert_eq!(capacities(&vm), shape);
            assert_eq!(vm.heap.stats(), heap);
            assert_eq!(vm.host_scheduling_snapshot().payload_bytes, used);
        }
    }
}
#[cfg_attr(test, test)]
fn namespace_cache_storage_publication_transfers_exact_staging_without_recharge() {
    for deferred in [false, true] {
        let mut vm = page();
        let (_, mut linked) = empty_graph(&["leaf"]);
        let object = vm
            .with_roots(|heap| heap.alloc_module_namespace(Vec::new(), deferred))
            .unwrap();
        let root = vm.heap.root(object).unwrap();
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        let preview =
            namespace_cache_storage::PreparedNamespaceEntry::prepare(&vm, "leaf", deferred)
                .unwrap();
        let stage = vm.host_scheduling_snapshot().payload_bytes - before;
        drop(preview);
        let _peer = leave_room(&mut vm, stage);
        let prepared =
            namespace_cache_storage::PreparedNamespaceEntry::prepare(&vm, "leaf", deferred)
                .unwrap();
        prepared.publish(&mut vm, "leaf", &mut linked, object, root);
        assert_eq!(
            vm.host_scheduling_snapshot().payload_bytes,
            MAX_HOST_SCHEDULING_BYTES - stage + vm.namespace_cache_bytes()
        );
        assert_eq!(linked["leaf"].namespace_epochs[usize::from(deferred)], 1);
        assert_eq!(vm.next_namespace_epoch, 2);
    }
}
#[cfg_attr(test, test)]
fn namespace_cache_storage_terminal_rollback_allocates_nothing_and_preserves_older_cache() {
    let mut vm = page();
    let (modules, mut linked) = empty_graph(&["good", "dead"]);
    let mut roots = ModuleRoots::default();
    let good = vm
        .module_namespace("good", false, &modules, &mut linked, &mut roots)
        .unwrap();
    let checkpoint = vm.next_namespace_epoch;
    let root_checkpoint = roots.len();
    vm.module_namespace("dead", false, &modules, &mut linked, &mut roots)
        .unwrap();
    vm.module_namespace("dead", true, &modules, &mut linked, &mut roots)
        .unwrap();
    let _peer = leave_room(&mut vm, 0);
    vm.host_scheduling.as_ref().unwrap().fail();
    #[cfg(test)]
    {
        let (_, allocations) = allocation_observer::count(|| {
            vm.rollback_namespace_cache_from(checkpoint, &mut linked);
            vm.release_module_roots_from(&mut roots, root_checkpoint);
        });
        assert_eq!(allocations, 0);
    }
    #[cfg(not(test))]
    {
        vm.rollback_namespace_cache_from(checkpoint, &mut linked);
        vm.release_module_roots_from(&mut roots, root_checkpoint);
    }
    assert_eq!(vm.module_namespace_cache["good"], good);
    assert!(!vm.module_namespace_cache.contains_key("dead"));
    assert!(vm.module_deferred_namespace_cache.is_empty() && vm.deferred_namespaces.is_empty());
    assert_eq!(linked["dead"].namespace_epochs, [0, 0]);
    assert_eq!(roots.len(), root_checkpoint);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}
#[cfg_attr(test, test)]
fn namespace_cache_storage_empty_rollback_releases_all_five_backings() {
    let mut vm = page();
    let (modules, mut linked) = empty_graph(&["leaf"]);
    let mut roots = ModuleRoots::default();
    vm.module_namespace("leaf", false, &modules, &mut linked, &mut roots)
        .unwrap();
    vm.module_namespace("leaf", true, &modules, &mut linked, &mut roots)
        .unwrap();
    vm.host_scheduling.as_ref().unwrap().fail();
    vm.rollback_namespace_cache_from(1, &mut linked);
    vm.release_module_roots_from(&mut roots, 0);
    assert_eq!(capacities(&vm), [0; 5]);
    assert_eq!(vm.namespace_cache_bytes(), 0);
    assert_eq!(linked["leaf"].namespace_epochs, [0, 0]);
}
#[cfg_attr(test, test)]
fn namespace_cache_storage_deletion_refunds_names_and_retains_nonempty_backing() {
    let mut vm = page();
    let (modules, mut linked) = empty_graph(&["good", "dead"]);
    let mut roots = ModuleRoots::default();
    vm.module_namespace("good", false, &modules, &mut linked, &mut roots)
        .unwrap();
    let checkpoint = vm.next_namespace_epoch;
    vm.module_namespace("dead", false, &modules, &mut linked, &mut roots)
        .unwrap();
    vm.module_namespace_cache.reserve(8);
    vm.module_namespace_roots.reserve(8);
    let tables = (
        vm.module_namespace_cache.retained_bytes(),
        vm.module_namespace_roots.retained_bytes(),
    );
    let released = vm
        .module_namespace_cache
        .get_key_value("dead")
        .unwrap()
        .0
        .capacity()
        + vm.module_namespace_roots
            .get_key_value("dead")
            .unwrap()
            .0
            .capacity();
    let before = vm.namespace_cache_bytes();
    vm.rollback_namespace_cache_from(checkpoint, &mut linked);
    assert_eq!(
        (
            vm.module_namespace_cache.retained_bytes(),
            vm.module_namespace_roots.retained_bytes()
        ),
        tables
    );
    assert_eq!(vm.namespace_cache_bytes(), before - released);
}
#[cfg_attr(test, test)]
fn namespace_cache_storage_sparse_reclamation_requires_complete_candidate_headroom() {
    let mut vm = page();
    let (modules, mut linked) = empty_graph(&["leaf"]);
    let mut roots = ModuleRoots::default();
    vm.module_namespace("leaf", false, &modules, &mut linked, &mut roots)
        .unwrap();
    vm.module_namespace("leaf", true, &modules, &mut linked, &mut roots)
        .unwrap();
    grow_all(&mut vm);
    let before = capacities(&vm);
    let bytes = vm.namespace_cache_bytes();
    let _peer = leave_room(&mut vm, 0);
    vm.reclaim_namespace_cache_storage();
    assert_eq!(capacities(&vm), before);
    assert_eq!(vm.namespace_cache_bytes(), bytes);
    assert_eq!(vm.check_scheduling_failure(), Ok(()));
}
#[cfg_attr(test, test)]
fn namespace_cache_storage_sequential_reclamation_preserves_names_and_roots() {
    let mut vm = page();
    let (modules, mut linked) = empty_graph(&["leaf"]);
    let mut roots = ModuleRoots::default();
    let eager = vm
        .module_namespace("leaf", false, &modules, &mut linked, &mut roots)
        .unwrap();
    let deferred = vm
        .module_namespace("leaf", true, &modules, &mut linked, &mut roots)
        .unwrap();
    grow_all(&mut vm);
    let name = vm
        .module_namespace_cache
        .get_key_value("leaf")
        .unwrap()
        .0
        .as_ptr();
    let reverse = vm.deferred_namespaces[&deferred].as_ptr();
    let root = vm.module_namespace_roots["leaf"];
    let before = vm.namespace_cache_bytes();
    let allowance = HashMap::<String, ObjectId>::with_capacity(1).capacity()
        * std::mem::size_of::<(String, ObjectId)>();
    let _peer = leave_room(&mut vm, allowance);
    vm.reclaim_namespace_cache_storage();
    assert_eq!(capacities(&vm), [3; 5]);
    assert_eq!(vm.module_namespace_cache["leaf"], eager);
    assert_eq!(vm.module_namespace_roots["leaf"], root);
    assert_eq!(
        vm.module_namespace_cache
            .get_key_value("leaf")
            .unwrap()
            .0
            .as_ptr(),
        name
    );
    assert_eq!(vm.deferred_namespaces[&deferred].as_ptr(), reverse);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - allowance - before + vm.namespace_cache_bytes()
    );
}
#[cfg_attr(test, test)]
fn namespace_cache_storage_reclamation_retries_after_peer_release() {
    let mut vm = page();
    let (modules, mut linked) = empty_graph(&["leaf"]);
    let mut roots = ModuleRoots::default();
    vm.module_namespace("leaf", false, &modules, &mut linked, &mut roots)
        .unwrap();
    vm.module_namespace("leaf", true, &modules, &mut linked, &mut roots)
        .unwrap();
    grow_all(&mut vm);
    let peer = leave_room(&mut vm, 0);
    vm.reclaim_namespace_cache_storage();
    assert!(capacities(&vm)[0] > 3);
    drop(peer);
    vm.reclaim_namespace_cache_storage();
    assert_eq!(capacities(&vm), [3; 5]);
}
#[cfg_attr(test, test)]
fn namespace_cache_storage_serial_refusal_precedes_allocation_and_publication() {
    let mut vm = page();
    let (modules, mut linked) = empty_graph(&["leaf"]);
    let mut roots = ModuleRoots::default();
    vm.host_scheduling_usage().unwrap();
    vm.next_namespace_epoch = u64::MAX;
    let heap = vm.heap.stats();
    #[cfg(test)]
    let result = {
        let (result, allocations) = allocation_observer::count(|| {
            vm.module_namespace("leaf", false, &modules, &mut linked, &mut roots)
        });
        assert_eq!(allocations, 0);
        result
    };
    #[cfg(not(test))]
    let result = vm.module_namespace("leaf", false, &modules, &mut linked, &mut roots);
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(vm.heap.stats(), heap);
    assert_eq!(vm.next_namespace_epoch, u64::MAX);
    assert_eq!(vm.namespace_transaction_depth, 0);
    assert!(roots.is_empty());
}
#[cfg_attr(test, test)]
fn namespace_cache_storage_nested_failure_clears_every_new_placeholder_and_root() {
    for deferred in [false, true] {
        let mut vm = page();
        let (modules, mut linked) = empty_graph(&["good", "root", "child"]);
        let mut modules = modules;
        let mut roots = ModuleRoots::default();
        modules.insert(
            "root".into(),
            compile("export * as child from 'child'; export const zz=1;"),
        );
        modules.insert("child".into(), compile("export * as back from 'root';"));
        let good = vm
            .module_namespace("good", false, &modules, &mut linked, &mut roots)
            .unwrap();
        let checkpoint = roots.len();
        let epoch = vm.next_namespace_epoch;
        let bytes = vm.namespace_cache_bytes();
        assert!(matches!(
            vm.module_namespace("root", deferred, &modules, &mut linked, &mut roots),
            Err(RuntimeError::ModuleResolution(_))
        ));
        assert_eq!(vm.module_namespace_cache.len(), 1);
        assert_eq!(vm.module_namespace_cache["good"], good);
        assert!(vm.module_deferred_namespace_cache.is_empty() && vm.deferred_namespaces.is_empty());
        assert_eq!(roots.len(), checkpoint);
        assert_eq!(vm.namespace_transaction_depth, 0);
        assert!(vm.next_namespace_epoch > epoch);
        assert_eq!(linked["root"].namespace_epochs, [0, 0]);
        assert_eq!(linked["child"].namespace_epochs, [0, 0]);
        assert_eq!(vm.namespace_cache_bytes(), bytes);
    }
}
#[cfg_attr(test, test)]
fn namespace_cache_storage_root_registration_failure_keeps_cache_unpublished() {
    for remaining in [0, 1] {
        let mut vm = page();
        let (modules, mut linked) = empty_graph(&["leaf"]);
        let mut roots = ModuleRoots::default();
        vm.heap.allow_root_registrations(remaining);
        assert_eq!(
            vm.module_namespace("leaf", false, &modules, &mut linked, &mut roots),
            Err(RuntimeError::Heap(crate::heap::HeapError::IdExhausted))
        );
        assert_eq!(vm.namespace_cache_bytes(), 0);
        assert_eq!(vm.next_namespace_epoch, 1);
        assert!(roots.is_empty());
        assert!(!vm.preserve_promise_table_capacity);
        assert_eq!(vm.namespace_transaction_depth, 0);
        assert!(linked["leaf"].namespace.is_none());
    }
}
#[cfg_attr(test, test)]
fn namespace_cache_storage_managed_refusal_restores_guards_and_allows_retry() {
    let mut vm = page();
    let (modules, mut linked) = empty_graph(&["leaf"]);
    let mut roots = ModuleRoots::default();
    vm.heap.allow_only(0);
    assert!(matches!(
        vm.module_namespace("leaf", false, &modules, &mut linked, &mut roots),
        Err(RuntimeError::Heap(_))
    ));
    assert_eq!(vm.namespace_cache_bytes(), 0);
    assert_eq!(vm.next_namespace_epoch, 1);
    assert_eq!(vm.namespace_transaction_depth, 0);
    assert!(!vm.preserve_promise_table_capacity);
    vm.heap.allow_only(16 * 1024 * 1024);
    assert!(vm
        .module_namespace("leaf", false, &modules, &mut linked, &mut roots)
        .is_ok());
}
#[cfg_attr(test, test)]
fn namespace_cache_storage_linking_failure_removes_earlier_successful_namespace_and_retry_uses_new_cells(
) {
    let mut vm = page();
    let retained = HashMap::from([("test/retained.mjs".into(), compile("export const value=0;"))]);
    vm.execute_module_graph("test/retained.mjs", &retained)
        .unwrap();
    let existing = vm.last_module_namespace.unwrap();
    let checkpoint = vm.next_namespace_epoch;
    let mut modules = retained;
    modules.insert(
        "test/a.mjs".into(),
        compile("import * as ns from './new.mjs'; import {missing} from './new.mjs'; export const value=ns.value;"),
    );
    modules.insert("test/new.mjs".into(), compile("export const value=1;"));
    assert!(matches!(
        vm.execute_module_graph("test/a.mjs", &modules),
        Err(RuntimeError::ModuleResolution(_))
    ));
    assert!(vm.next_namespace_epoch > checkpoint);
    assert!(!vm.module_namespace_cache.contains_key("test/new.mjs"));
    assert_eq!(vm.module_namespace_cache["test/retained.mjs"], existing);
    assert_eq!(vm.module_graph.as_ref().unwrap().linked.len(), 1);
    modules.insert("test/new.mjs".into(), compile("export const value=2;"));
    modules.insert("test/a.mjs".into(), compile("import * as ns from './new.mjs'; import {value as other} from './new.mjs'; export const value=ns.value;"));
    vm.execute_module_graph("test/a.mjs", &modules).unwrap();
    let namespace = Value::Object(vm.last_module_namespace.unwrap());
    assert_eq!(
        vm.get_property(&namespace, &"value".into()),
        Ok(Value::Number(2.0))
    );
}
#[cfg_attr(test, test)]
fn namespace_cache_storage_completed_identity_survives_collection_and_vm_drop_refunds_cache() {
    let mut vm = page();
    let (modules, mut linked) = empty_graph(&["leaf"]);
    let mut roots = ModuleRoots::default();
    let id = vm
        .module_namespace("leaf", false, &modules, &mut linked, &mut roots)
        .unwrap();
    vm.release_module_roots_from(&mut roots, 0);
    let bytes = vm.namespace_cache_bytes();
    vm.collect_realm_garbage().unwrap();
    assert_eq!(vm.module_namespace_cache["leaf"], id);
    assert_eq!(vm.namespace_cache_bytes(), bytes);
    assert_eq!(
        vm.module_namespace("leaf", false, &modules, &mut linked, &mut roots),
        Ok(id)
    );
    let view = Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    };
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn namespace_cache_storage_host_neutral_construction_and_rollback_preserve_identity() {
    let mut vm = Vm::default();
    let (modules, mut linked) = empty_graph(&["leaf"]);
    let mut roots = ModuleRoots::default();
    let id = vm
        .module_namespace("leaf", false, &modules, &mut linked, &mut roots)
        .unwrap();
    assert_eq!(
        vm.module_namespace("leaf", false, &modules, &mut linked, &mut roots),
        Ok(id)
    );
    vm.rollback_namespace_cache_from(1, &mut linked);
    vm.release_module_roots_from(&mut roots, 0);
    assert_eq!(vm.namespace_cache_bytes(), 0);
    assert!(linked["leaf"].namespace.is_none());
}

impl crate::Vm {
    /// Runs finite namespace cache admission and transaction lifetime contracts.
    #[doc(hidden)]
    pub fn verify_namespace_cache_storage_contracts() {
        #[cfg(test)]
        namespace_cache_storage_refusal_precedes_snapshot_and_name_scratch();
        namespace_cache_storage_table_backing_is_charged_in_every_layer();
        namespace_cache_storage_name_capacity_is_charged_in_every_layer();
        namespace_cache_storage_eager_and_deferred_keep_distinct_stable_identities();
        namespace_cache_storage_preparation_preserves_graph_heap_and_roots();
        namespace_cache_storage_abandoned_preparations_refund_every_layer();
        namespace_cache_storage_detached_preparation_survives_vm_destruction();
        namespace_cache_storage_exact_and_short_staging_preserve_existing_allocations();
        namespace_cache_storage_publication_transfers_exact_staging_without_recharge();
        namespace_cache_storage_terminal_rollback_allocates_nothing_and_preserves_older_cache();
        namespace_cache_storage_empty_rollback_releases_all_five_backings();
        namespace_cache_storage_deletion_refunds_names_and_retains_nonempty_backing();
        namespace_cache_storage_sparse_reclamation_requires_complete_candidate_headroom();
        namespace_cache_storage_sequential_reclamation_preserves_names_and_roots();
        namespace_cache_storage_reclamation_retries_after_peer_release();
        namespace_cache_storage_serial_refusal_precedes_allocation_and_publication();
        namespace_cache_storage_nested_failure_clears_every_new_placeholder_and_root();
        namespace_cache_storage_root_registration_failure_keeps_cache_unpublished();
        namespace_cache_storage_managed_refusal_restores_guards_and_allows_retry();
        namespace_cache_storage_linking_failure_removes_earlier_successful_namespace_and_retry_uses_new_cells();
        namespace_cache_storage_completed_identity_survives_collection_and_vm_drop_refunds_cache();
        namespace_cache_storage_host_neutral_construction_and_rollback_preserve_identity();
    }
}
