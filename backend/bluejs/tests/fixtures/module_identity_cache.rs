// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Small source/meta caches verify persistent document ownership.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
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
fn module_identity_cache_import_meta_hit_borrows_its_lookup_name() {
    let mut vm = page();
    vm.active_module_name = Some("cached.mjs".into());
    let expected = vm.import_meta().unwrap();
    let _peer = leave_room(&mut vm, 0);
    let (result, allocations) = allocation_observer::count(|| vm.import_meta());
    assert_eq!(result, Ok(expected));
    assert_eq!(allocations, 0);
}
#[cfg(test)]
#[test]
fn module_identity_cache_meta_refusal_precedes_lookup_copy_and_heap_work() {
    let mut vm = page();
    vm.active_module_name = Some("cold.mjs".into());
    let _peer = leave_room(&mut vm, 0);
    let before = vm.heap.stats();
    let (result, allocations) = allocation_observer::count(|| vm.import_meta());
    assert_eq!(allocations, 0);
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(vm.heap.stats(), before);
    assert!(vm.module_import_meta.is_empty() && vm.module_import_meta_roots.is_empty());
}
#[cfg_attr(test, test)]
fn module_identity_cache_all_table_backings_join_document_usage() {
    for layer in 0..4 {
        let mut vm = page();
        let before = vm.own_scheduling_usage().payload_bytes;
        let expected = match layer {
            0 => {
                vm.module_source_cache.reserve(1);
                vm.module_source_cache.capacity() * std::mem::size_of::<(String, ObjectId)>()
            }
            1 => {
                vm.module_source_roots.reserve(1);
                vm.module_source_roots.capacity() * std::mem::size_of::<(String, RootId)>()
            }
            2 => {
                vm.module_import_meta.reserve(1);
                vm.module_import_meta.capacity() * std::mem::size_of::<(String, ObjectId)>()
            }
            _ => {
                vm.module_import_meta_roots.reserve(1);
                vm.module_import_meta_roots.capacity() * std::mem::size_of::<(String, RootId)>()
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
fn module_identity_cache_all_owned_key_capacities_join_document_usage() {
    for layer in 0..4 {
        let mut vm = page();
        let object = vm.heap.alloc_object(None).unwrap();
        let root = vm.heap.root(object).unwrap();
        match layer {
            0 => vm.module_source_cache.reserve(1),
            1 => vm.module_source_roots.reserve(1),
            2 => vm.module_import_meta.reserve(1),
            _ => vm.module_import_meta_roots.reserve(1),
        }
        let mut name = String::with_capacity(64);
        name.push_str("leaf");
        let expected = name.capacity();
        let before = vm.own_scheduling_usage().payload_bytes;
        match layer {
            0 => {
                vm.module_source_cache.insert(name, object);
            }
            1 => {
                vm.module_source_roots.insert(name, root);
            }
            2 => {
                vm.module_import_meta.insert(name, object);
            }
            _ => {
                vm.module_import_meta_roots.insert(name, root);
            }
        }
        assert_eq!(
            vm.own_scheduling_usage().payload_bytes - before,
            expected,
            "layer={layer}"
        );
    }
}

use crate::vm::module_identity_cache::{IdentityKind, PreparedIdentity};

fn caps(vm: &Vm) -> [usize; 4] {
    [
        vm.module_source_cache.capacity(),
        vm.module_source_roots.capacity(),
        vm.module_import_meta.capacity(),
        vm.module_import_meta_roots.capacity(),
    ]
}
fn ledgers(vm: &Vm) -> [usize; 4] {
    [
        vm.module_source_cache.retained_bytes(),
        vm.module_source_roots.retained_bytes(),
        vm.module_import_meta.retained_bytes(),
        vm.module_import_meta_roots.retained_bytes(),
    ]
}
fn key_pointers(vm: &Vm) -> [usize; 4] {
    [
        vm.module_source_cache
            .get_key_value("keep")
            .unwrap()
            .0
            .as_ptr() as usize,
        vm.module_source_roots
            .get_key_value("keep")
            .unwrap()
            .0
            .as_ptr() as usize,
        vm.module_import_meta
            .get_key_value("keep")
            .unwrap()
            .0
            .as_ptr() as usize,
        vm.module_import_meta_roots
            .get_key_value("keep")
            .unwrap()
            .0
            .as_ptr() as usize,
    ]
}
fn grow(vm: &mut Vm) {
    vm.module_source_cache.reserve(8);
    vm.module_source_roots.reserve(8);
    vm.module_import_meta.reserve(8);
    vm.module_import_meta_roots.reserve(8);
}
fn identity(vm: &mut Vm, kind: IdentityKind, name: &str) -> Result<ObjectId, RuntimeError> {
    match kind {
        IdentityKind::Source => vm.module_source_object(name, &HashMap::new()),
        IdentityKind::Meta => {
            vm.active_module_name = Some(name.to_owned());
            Ok(vm.import_meta()?.object_id().unwrap())
        }
    }
}
fn populate(vm: &mut Vm) -> [ObjectId; 2] {
    vm.set_module_source_loader_context(vec!["keep".into(), "new".into(), "gone".into()]);
    [
        identity(vm, IdentityKind::Source, "keep").unwrap(),
        identity(vm, IdentityKind::Meta, "keep").unwrap(),
    ]
}
fn one_object_bytes(vm: &mut Vm) -> usize {
    vm.collect_realm_garbage().unwrap();
    let before = vm.heap.stats().managed_bytes;
    vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    let bytes = vm.heap.stats().managed_bytes - before;
    vm.collect_realm_garbage().unwrap();
    bytes
}

#[cfg_attr(test, test)]
fn module_identity_cache_source_and_meta_keep_distinct_stable_identities_at_quota() {
    let mut vm = page();
    let ids = populate(&mut vm);
    assert_ne!(ids[0], ids[1]);
    assert_eq!(vm.heap.prototype(ids[1]), Ok(None));
    assert_eq!(
        vm.heap.prototype(ids[0]),
        Ok(vm.host_module_source_prototype)
    );
    let _peer = leave_room(&mut vm, 0);
    let before = vm.heap.stats();
    #[cfg(test)]
    {
        let (result, allocations) = allocation_observer::count(|| {
            let source = vm.module_source_object("keep", &HashMap::new())?;
            let meta = vm.import_meta()?.object_id().unwrap();
            Ok::<_, RuntimeError>([source, meta])
        });
        assert_eq!(result, Ok(ids));
        assert_eq!(allocations, 0);
    }
    #[cfg(not(test))]
    {
        assert_eq!(vm.module_source_object("keep", &HashMap::new()), Ok(ids[0]));
        assert_eq!(vm.import_meta(), Ok(Value::Object(ids[1])));
    }
    assert_eq!(vm.heap.stats(), before);
}

#[cfg_attr(test, test)]
fn module_identity_cache_meta_fallback_and_module_names_are_stable() {
    let mut vm = page();
    let fallback = vm.import_meta().unwrap();
    assert_eq!(vm.import_meta(), Ok(fallback.clone()));
    vm.active_module_name = Some("<module>".into());
    assert_eq!(vm.import_meta(), Ok(fallback.clone()));
    let first = identity(&mut vm, IdentityKind::Meta, "one").unwrap();
    let second = identity(&mut vm, IdentityKind::Meta, "two").unwrap();
    assert_ne!(first, second);
    assert_ne!(Value::Object(first), fallback);
    assert_eq!(identity(&mut vm, IdentityKind::Meta, "one"), Ok(first));
}

#[cfg_attr(test, test)]
fn module_identity_cache_preparation_changes_no_heap_or_live_cache_storage() {
    for kind in [IdentityKind::Source, IdentityKind::Meta] {
        let mut vm = page();
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        let heap = vm.heap.stats();
        let shape = caps(&vm);
        let bytes = vm.module_identity_cache_bytes();
        let prepared = PreparedIdentity::prepare(&vm, "new", kind).unwrap();
        assert!(vm.host_scheduling_snapshot().payload_bytes > before);
        assert_eq!(vm.heap.stats(), heap);
        assert_eq!(caps(&vm), shape);
        assert_eq!(vm.module_identity_cache_bytes(), bytes);
        assert!(vm.host_module_source_prototype.is_none());
        drop(prepared);
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    }
}

#[cfg_attr(test, test)]
fn module_identity_cache_abandoned_preparations_stay_charged_after_vm_drop() {
    for kind in [IdentityKind::Source, IdentityKind::Meta] {
        let mut vm = page();
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        let observer = Vm {
            host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
            ..Vm::default()
        };
        let prepared = PreparedIdentity::prepare(&vm, "new", kind).unwrap();
        let bytes = vm.host_scheduling_snapshot().payload_bytes - before;
        assert!(bytes > 0);
        drop(vm);
        assert_eq!(observer.host_scheduling_snapshot().payload_bytes, bytes);
        drop(prepared);
        assert_eq!(observer.host_scheduling_snapshot().payload_bytes, 0);
    }
}

#[cfg_attr(test, test)]
fn module_identity_cache_preparation_admits_exact_and_refuses_short_room() {
    for kind in [IdentityKind::Source, IdentityKind::Meta] {
        for short in [false, true] {
            let mut vm = page();
            let before = vm.host_scheduling_usage().unwrap().payload_bytes;
            let preview = PreparedIdentity::prepare(&vm, "new", kind).unwrap();
            let bytes = vm.host_scheduling_snapshot().payload_bytes - before;
            drop(preview);
            let _peer = leave_room(&mut vm, bytes - usize::from(short));
            let used = vm.host_scheduling_snapshot().payload_bytes;
            let heap = vm.heap.stats();
            let shape = caps(&vm);
            let result = PreparedIdentity::prepare(&vm, "new", kind);
            if short {
                assert_eq!(result.err(), Some(RuntimeError::SchedulingLimit));
            } else {
                let prepared = result.unwrap();
                assert_eq!(
                    vm.host_scheduling_snapshot().payload_bytes,
                    MAX_HOST_SCHEDULING_BYTES
                );
                drop(prepared);
            }
            assert_eq!(vm.host_scheduling_snapshot().payload_bytes, used);
            assert_eq!(vm.heap.stats(), heap);
            assert_eq!(caps(&vm), shape);
        }
    }
}

#[cfg_attr(test, test)]
fn module_identity_cache_publication_transfers_exact_ownership_without_readmission() {
    for kind in [IdentityKind::Source, IdentityKind::Meta] {
        let mut vm = page();
        let object = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
        let root = vm.heap.root(object).unwrap();
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        let preview = PreparedIdentity::prepare(&vm, "new", kind).unwrap();
        let staged = vm.host_scheduling_snapshot().payload_bytes - before;
        drop(preview);
        let _peer = leave_room(&mut vm, staged);
        let before_cache = vm.module_identity_cache_bytes();
        let prepared = PreparedIdentity::prepare(&vm, "new", kind).unwrap();
        prepared.publish(&mut vm, object, root);
        let adopted = vm.module_identity_cache_bytes() - before_cache;
        assert_eq!(
            vm.host_scheduling_snapshot().payload_bytes,
            MAX_HOST_SCHEDULING_BYTES - staged + adopted
        );
        assert_eq!(vm.check_scheduling_failure(), Ok(()));
    }
}

#[cfg_attr(test, test)]
fn module_identity_cache_source_refusal_precedes_prototype_heap_and_root_work() {
    for warm in [false, true] {
        let mut vm = page();
        vm.set_module_source_loader_context(vec!["keep".into(), "new".into()]);
        if warm {
            identity(&mut vm, IdentityKind::Source, "keep").unwrap();
        }
        let _peer = leave_room(&mut vm, 0);
        let heap = vm.heap.stats();
        let prototype = vm.host_module_source_prototype;
        assert_eq!(
            vm.module_source_object("new", &HashMap::new()),
            Err(RuntimeError::SchedulingLimit)
        );
        assert_eq!(vm.heap.stats(), heap);
        assert_eq!(vm.host_module_source_prototype, prototype);
        assert!(!vm.module_source_cache.contains_key("new"));
        assert!(!vm.module_source_roots.contains_key("new"));
    }
}

#[cfg_attr(test, test)]
fn module_identity_cache_root_refusal_preserves_old_entries_and_capacity_policy() {
    for kind in [IdentityKind::Source, IdentityKind::Meta] {
        for saved in [false, true] {
            let mut vm = page();
            let ids = populate(&mut vm);
            let shape = caps(&vm);
            let prototype = vm.host_module_source_prototype;
            vm.preserve_promise_table_capacity = saved;
            vm.heap.allow_root_registrations(0);
            assert_eq!(
                identity(&mut vm, kind, "new"),
                Err(RuntimeError::Heap(crate::heap::HeapError::IdExhausted))
            );
            assert_eq!(caps(&vm), shape);
            assert_eq!(vm.preserve_promise_table_capacity, saved);
            assert_eq!(vm.host_module_source_prototype, prototype);
            assert_eq!(vm.module_source_cache["keep"], ids[0]);
            assert_eq!(vm.module_import_meta["keep"], ids[1]);
            assert!(!vm.module_source_cache.contains_key("new"));
            assert!(!vm.module_import_meta.contains_key("new"));
        }
    }
}

#[cfg_attr(test, test)]
fn module_identity_cache_managed_refusal_restores_guards_and_allows_retry() {
    for kind in [IdentityKind::Source, IdentityKind::Meta] {
        let mut vm = page();
        vm.set_module_source_loader_context(vec!["new".into()]);
        vm.active_module_name = Some("new".into());
        vm.collect_realm_garbage().unwrap();
        vm.host_scheduling_usage().unwrap();
        vm.heap.allow_only(0);
        assert!(matches!(
            identity(&mut vm, kind, "new"),
            Err(RuntimeError::Heap(_))
        ));
        assert!(!vm.preserve_promise_table_capacity);
        assert!(vm.module_source_cache.is_empty() && vm.module_import_meta.is_empty());
        assert!(vm.host_module_source_prototype.is_none());
        vm.heap.allow_only(16 * 1024 * 1024);
        assert!(identity(&mut vm, kind, "new").is_ok());
    }
}

#[cfg_attr(test, test)]
fn module_identity_cache_collection_preserves_roots_and_vm_drop_refunds_usage() {
    let mut vm = page();
    let ids = populate(&mut vm);
    let bytes = vm.module_identity_cache_bytes();
    vm.collect_realm_garbage().unwrap();
    assert!(vm.heap.contains(ids[0]) && vm.heap.contains(ids[1]));
    assert_eq!(vm.module_identity_cache_bytes(), bytes);
    assert_eq!(vm.module_source_cache["keep"], ids[0]);
    assert_eq!(vm.module_import_meta["keep"], ids[1]);
    let observer = Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    };
    drop(vm);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn module_identity_cache_deleted_keys_refund_payload_but_retain_nonempty_backing() {
    let mut vm = page();
    populate(&mut vm);
    identity(&mut vm, IdentityKind::Source, "gone").unwrap();
    identity(&mut vm, IdentityKind::Meta, "gone").unwrap();
    grow(&mut vm);
    let backing = ledgers(&vm);
    let released = vm
        .module_source_cache
        .get_key_value("gone")
        .unwrap()
        .0
        .capacity()
        + vm.module_source_roots
            .get_key_value("gone")
            .unwrap()
            .0
            .capacity()
        + vm.module_import_meta
            .get_key_value("gone")
            .unwrap()
            .0
            .capacity()
        + vm.module_import_meta_roots
            .get_key_value("gone")
            .unwrap()
            .0
            .capacity();
    let before = vm.module_identity_cache_bytes();
    vm.module_source_cache.remove("gone");
    vm.module_import_meta.remove("gone");
    for root in [
        vm.module_source_roots.remove("gone").unwrap(),
        vm.module_import_meta_roots.remove("gone").unwrap(),
    ] {
        vm.heap.unroot(root).unwrap();
    }
    assert_eq!(ledgers(&vm), backing);
    assert_eq!(vm.module_identity_cache_bytes(), before - released);
}

#[cfg_attr(test, test)]
fn module_identity_cache_sparse_reclamation_refuses_without_poisoning_the_document() {
    let mut vm = page();
    let ids = populate(&mut vm);
    grow(&mut vm);
    let shape = caps(&vm);
    let pointers = key_pointers(&vm);
    let _peer = leave_room(&mut vm, 0);
    vm.reclaim_module_identity_cache_storage();
    assert_eq!(caps(&vm), shape);
    assert_eq!(key_pointers(&vm), pointers);
    assert_eq!(vm.check_scheduling_failure(), Ok(()));
    assert_eq!(vm.module_source_cache["keep"], ids[0]);
}

#[cfg_attr(test, test)]
fn module_identity_cache_sequential_reclamation_preserves_keys_and_root_ids() {
    let mut vm = page();
    let ids = populate(&mut vm);
    let roots = [
        vm.module_source_roots["keep"],
        vm.module_import_meta_roots["keep"],
    ];
    grow(&mut vm);
    let pointers = key_pointers(&vm);
    let before_bytes = vm.module_identity_cache_bytes();
    let candidate = 3 * std::mem::size_of::<(String, ObjectId)>();
    let _peer = leave_room(&mut vm, candidate);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    vm.reclaim_module_identity_cache_storage();
    assert_eq!(caps(&vm), [3; 4]);
    assert_eq!(key_pointers(&vm), pointers);
    assert_eq!(
        [
            vm.module_source_roots["keep"],
            vm.module_import_meta_roots["keep"]
        ],
        roots
    );
    assert_eq!(
        [
            vm.module_source_cache["keep"],
            vm.module_import_meta["keep"]
        ],
        ids
    );
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        before - (before_bytes - vm.module_identity_cache_bytes())
    );
}

#[cfg_attr(test, test)]
fn module_identity_cache_managed_gc_keeps_prepared_table_shapes() {
    for kind in [IdentityKind::Source, IdentityKind::Meta] {
        let mut vm = page();
        populate(&mut vm);
        let object_bytes = one_object_bytes(&mut vm);
        grow(&mut vm);
        let shape = caps(&vm);
        vm.active_module_name = Some("new".into());
        vm.heap.allow_only(object_bytes);
        let major = vm.heap.stats().major_collections;
        identity(&mut vm, kind, "new").unwrap();
        assert!(vm.heap.stats().major_collections > major);
        assert_eq!(caps(&vm), shape);
        assert!(!vm.preserve_promise_table_capacity);
    }
}

#[cfg_attr(test, test)]
fn module_identity_cache_reclamation_retries_after_peer_release() {
    let mut vm = page();
    populate(&mut vm);
    grow(&mut vm);
    let shape = caps(&vm);
    let peer = leave_room(&mut vm, 0);
    vm.reclaim_module_identity_cache_storage();
    assert_eq!(caps(&vm), shape);
    drop(peer);
    vm.reclaim_module_identity_cache_storage();
    assert_eq!(caps(&vm), [3; 4]);
    assert_eq!(vm.check_scheduling_failure(), Ok(()));
}

#[cfg_attr(test, test)]
fn module_identity_cache_cold_source_gc_protects_the_prototype_without_operands() {
    let mut vm = page();
    vm.set_module_source_loader_context(vec!["new".into()]);
    let object_bytes = one_object_bytes(&mut vm);
    vm.stack = Vec::new();
    vm.heap.allow_only(2 * object_bytes);
    let major = vm.heap.stats().major_collections;
    let source = vm.module_source_object("new", &HashMap::new()).unwrap();
    assert!(vm.heap.stats().major_collections > major);
    assert_eq!(vm.stack.capacity(), 0);
    let prototype = vm.host_module_source_prototype.unwrap();
    assert!(vm.heap.contains(prototype));
    assert_eq!(vm.heap.prototype(source), Ok(Some(prototype)));
}

#[cfg_attr(test, test)]
fn module_identity_cache_host_neutral_source_and_meta_contracts_remain_stable() {
    let mut vm = Vm::default();
    let ids = populate(&mut vm);
    assert_eq!(vm.module_source_object("keep", &HashMap::new()), Ok(ids[0]));
    assert_eq!(vm.import_meta(), Ok(Value::Object(ids[1])));
    assert_eq!(vm.heap.prototype(ids[1]), Ok(None));
    vm.collect_realm_garbage().unwrap();
    assert!(vm.heap.contains(ids[0]) && vm.heap.contains(ids[1]));
}

#[cfg_attr(test, test)]
fn module_identity_cache_source_validation_preserves_existing_error_kinds() {
    let mut vm = page();
    let modules = HashMap::from([(
        "code".into(),
        crate::compile_module(&crate::parse_module("export const x=1;").unwrap()).unwrap(),
    )]);
    vm.set_module_source_loader_context(vec!["code".into()]);
    let before = vm.heap.stats();
    assert!(matches!(
        vm.module_source_object("code", &modules),
        Err(RuntimeError::ModuleResolution(_))
    ));
    assert!(matches!(
        vm.module_source_object("missing", &modules),
        Err(RuntimeError::TypeError(_))
    ));
    assert_eq!(vm.heap.stats(), before);
    assert!(vm.module_source_cache.is_empty());
    assert!(vm.host_module_source_prototype.is_none());
}

#[cfg_attr(test, test)]
fn module_identity_cache_real_source_and_meta_imports_preserve_module_identity() {
    let mut vm = page();
    vm.set_module_source_loader_context(vec!["t/blob.wasm".into()]);
    let modules = [
        ("t/main", "import source blob from './blob.wasm'; import {other,meta} from './other'; export const same=blob===other; export const own=import.meta; export const equal=own===import.meta; export const different=own!==meta;"),
        ("t/other", "import source other from './blob.wasm'; export {other}; export const meta=import.meta;"),
    ].into_iter().map(|(name, source)| (name.to_owned(), crate::compile_module(&crate::parse_module(source).unwrap()).unwrap())).collect();
    vm.execute_module_graph("t/main", &modules).unwrap();
    let namespace = Value::Object(vm.last_module_namespace.unwrap());
    for name in ["same", "equal", "different"] {
        assert_eq!(
            vm.get_property(&namespace, &name.into()),
            Ok(Value::Bool(true))
        );
    }
}

impl crate::Vm {
    /// Runs finite source/import-meta cache preparation and lifetime contracts.
    #[doc(hidden)]
    pub fn verify_module_identity_cache_contracts() {
        module_identity_cache_all_table_backings_join_document_usage();
        module_identity_cache_all_owned_key_capacities_join_document_usage();
        module_identity_cache_source_and_meta_keep_distinct_stable_identities_at_quota();
        module_identity_cache_meta_fallback_and_module_names_are_stable();
        module_identity_cache_preparation_changes_no_heap_or_live_cache_storage();
        module_identity_cache_abandoned_preparations_stay_charged_after_vm_drop();
        module_identity_cache_preparation_admits_exact_and_refuses_short_room();
        module_identity_cache_publication_transfers_exact_ownership_without_readmission();
        module_identity_cache_source_refusal_precedes_prototype_heap_and_root_work();
        module_identity_cache_root_refusal_preserves_old_entries_and_capacity_policy();
        module_identity_cache_managed_refusal_restores_guards_and_allows_retry();
        module_identity_cache_collection_preserves_roots_and_vm_drop_refunds_usage();
        module_identity_cache_deleted_keys_refund_payload_but_retain_nonempty_backing();
        module_identity_cache_sparse_reclamation_refuses_without_poisoning_the_document();
        module_identity_cache_sequential_reclamation_preserves_keys_and_root_ids();
        module_identity_cache_managed_gc_keeps_prepared_table_shapes();
        module_identity_cache_reclamation_retries_after_peer_release();
        module_identity_cache_cold_source_gc_protects_the_prototype_without_operands();
        module_identity_cache_host_neutral_source_and_meta_contracts_remain_stable();
        module_identity_cache_source_validation_preserves_existing_error_kinds();
        module_identity_cache_real_source_and_meta_imports_preserve_module_identity();
    }
}
