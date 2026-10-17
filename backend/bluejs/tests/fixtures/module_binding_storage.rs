// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Small binding preparations verify document admission and lifetime.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;
use crate::vm::module_binding_storage::{Aliases, ImportedSlots, NamespaceExports};

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}
fn compile(source: &str) -> Bytecode {
    crate::compile_module(&crate::parse_module(source).unwrap()).unwrap()
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
fn module_binding_storage_import_slots_refuse_before_allocation() {
    let mut vm = page();
    let code = compile("import {x} from 'leaf'; export {x};");
    let _peer = leave_room(&mut vm, 0);
    let (result, allocations) =
        allocation_observer::count(|| ImportedSlots::prepare(&code, vm.host_scheduling.as_ref()));
    assert_eq!(allocations, 0);
    assert_eq!(result.err(), Some(RuntimeError::SchedulingLimit));
}
#[cfg(test)]
#[test]
fn module_binding_storage_aliases_refuse_before_growth() {
    let mut vm = page();
    let cell = vm.heap.alloc_object(None).unwrap();
    let code = compile("import {x} from 'leaf'; export {x};");
    let _peer = leave_room(&mut vm, 0);
    let (result, allocations) = allocation_observer::count(|| {
        let mut aliases = Aliases::prepare(&code, vm.host_scheduling.as_ref())?;
        aliases.push(0, cell);
        Ok::<_, RuntimeError>(aliases)
    });
    assert_eq!(allocations, 0);
    assert_eq!(result.err(), Some(RuntimeError::SchedulingLimit));
}
#[cfg(test)]
#[test]
fn module_binding_storage_namespace_list_refuses_before_allocation() {
    let mut vm = page();
    let _peer = leave_room(&mut vm, 0);
    let (result, allocations) = allocation_observer::count(|| {
        NamespaceExports::prepare(&["x"], vm.host_scheduling.as_ref())
    });
    assert_eq!(allocations, 0);
    assert_eq!(result.err(), Some(RuntimeError::SchedulingLimit));
}
#[cfg_attr(test, test)]
fn module_binding_storage_namespace_payload_retains_native_ownership() {
    let mut vm = page();
    let cell = vm.heap.alloc_object(None).unwrap();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let mut exports = NamespaceExports::prepare(&["x"], vm.host_scheduling.as_ref()).unwrap();
    exports.keep_next(cell);
    assert!(
        vm.host_scheduling_snapshot().payload_bytes
            >= before + std::mem::size_of::<(JsString, ObjectId)>() + 2
    );
    drop(exports);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

fn destination(vm: &mut Vm) -> (ObjectId, ObjectId) {
    let cell = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    vm.heap.root(cell).unwrap();
    vm.with_roots(|heap| heap.set(cell, "value", Value::Number(7.0)))
        .unwrap();
    let namespace = vm
        .with_roots(|heap| heap.alloc_module_namespace(Vec::new(), false))
        .unwrap();
    vm.heap.root(namespace).unwrap();
    vm.collect_realm_garbage().unwrap();
    (cell, namespace)
}

#[cfg_attr(test, test)]
fn module_binding_storage_import_slots_keep_contents_and_refund_backing() {
    let code = compile("import 'side'; import {x as a,y as b} from 'leaf'; export {a,b};");
    let mut vm = page();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let slots = ImportedSlots::prepare(&code, vm.host_scheduling.as_ref()).unwrap();
    for slot in code.module_imports.iter().filter_map(|i| i.local_slot) {
        assert!(slots.contains(slot as usize));
    }
    assert!(!slots.contains(99));
    assert!(slots.bytes() > 0);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes - before,
        slots.bytes()
    );
    drop(slots);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn module_binding_storage_import_slots_admit_exact_and_refuse_short_room() {
    let code = compile("import {x} from 'leaf';");
    for short in [false, true] {
        let mut vm = page();
        let preview = ImportedSlots::prepare(&code, vm.host_scheduling.as_ref()).unwrap();
        let bytes = preview.bytes();
        assert!(bytes > 0);
        drop(preview);
        let _peer = leave_room(&mut vm, bytes - usize::from(short));
        let before = vm.host_scheduling_snapshot().payload_bytes;
        let result = ImportedSlots::prepare(&code, vm.host_scheduling.as_ref());
        if short {
            assert_eq!(result.err(), Some(RuntimeError::SchedulingLimit));
        } else {
            let slots = result.unwrap();
            assert!(slots.contains(0));
            assert_eq!(
                vm.host_scheduling_snapshot().payload_bytes,
                MAX_HOST_SCHEDULING_BYTES
            );
            drop(slots);
        }
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    }
}

#[cfg_attr(test, test)]
fn module_binding_storage_aliases_admit_exact_and_refuse_short_room() {
    let code = compile("import {x} from 'leaf';");
    for short in [false, true] {
        let mut vm = page();
        let preview = Aliases::prepare(&code, vm.host_scheduling.as_ref()).unwrap();
        let bytes = preview.bytes();
        assert!(bytes > 0);
        drop(preview);
        let _peer = leave_room(&mut vm, bytes - usize::from(short));
        let before = vm.host_scheduling_snapshot().payload_bytes;
        let result = Aliases::prepare(&code, vm.host_scheduling.as_ref());
        if short {
            assert_eq!(result.err(), Some(RuntimeError::SchedulingLimit));
        } else {
            let aliases = result.unwrap();
            assert_eq!(
                vm.host_scheduling_snapshot().payload_bytes,
                MAX_HOST_SCHEDULING_BYTES
            );
            drop(aliases);
        }
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    }
}

#[cfg_attr(test, test)]
fn module_binding_storage_alias_publication_needs_no_more_storage() {
    let code = compile("import {x} from 'leaf';");
    let mut vm = page();
    let cell = vm.heap.alloc_object(None).unwrap();
    let mut cells = HashMap::with_capacity(1);
    let mut aliases = Aliases::prepare(&code, vm.host_scheduling.as_ref()).unwrap();
    aliases.push(0, cell);
    let bytes = aliases.bytes();
    let _peer = leave_room(&mut vm, 0);
    #[cfg(test)]
    {
        let (_, allocations) = allocation_observer::count(|| aliases.publish(&mut cells));
        assert_eq!(allocations, 0);
    }
    #[cfg(not(test))]
    aliases.publish(&mut cells);
    assert_eq!(cells[&0], cell);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - bytes
    );
}

#[cfg_attr(test, test)]
fn module_binding_storage_detached_import_metadata_outlives_the_vm() {
    let code = compile("import {x} from 'leaf';");
    let vm = page();
    let observer = Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    };
    let slots = ImportedSlots::prepare(&code, vm.host_scheduling.as_ref()).unwrap();
    let aliases = Aliases::prepare(&code, vm.host_scheduling.as_ref()).unwrap();
    let slot_bytes = slots.bytes();
    let alias_bytes = aliases.bytes();
    drop(vm);
    assert_eq!(
        observer.host_scheduling_snapshot().payload_bytes,
        slot_bytes + alias_bytes
    );
    drop(aliases);
    assert_eq!(
        observer.host_scheduling_snapshot().payload_bytes,
        slot_bytes
    );
    drop(slots);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn module_binding_storage_empty_preparations_work_at_full_healthy_quota() {
    let code = compile("export const x=1;");
    let mut vm = page();
    let _peer = leave_room(&mut vm, 0);
    let slots = ImportedSlots::prepare(&code, vm.host_scheduling.as_ref()).unwrap();
    let aliases = Aliases::prepare(&code, vm.host_scheduling.as_ref()).unwrap();
    let exports = NamespaceExports::prepare(&[], vm.host_scheduling.as_ref()).unwrap();
    assert_eq!(slots.bytes() + aliases.bytes() + exports.bytes(), 0);
    assert_eq!(vm.check_scheduling_failure(), Ok(()));
}

#[cfg_attr(test, test)]
fn module_binding_storage_terminal_scope_refuses_every_preparation() {
    for source in ["", "import {x} from 'leaf';"] {
        let code = compile(source);
        let vm = page();
        vm.host_scheduling.as_ref().unwrap().fail();
        assert_eq!(
            ImportedSlots::prepare(&code, vm.host_scheduling.as_ref()).err(),
            Some(RuntimeError::SchedulingLimit)
        );
        assert_eq!(
            Aliases::prepare(&code, vm.host_scheduling.as_ref()).err(),
            Some(RuntimeError::SchedulingLimit)
        );
        for names in [&[][..], &["x"][..]] {
            assert_eq!(
                NamespaceExports::prepare(names, vm.host_scheduling.as_ref()).err(),
                Some(RuntimeError::SchedulingLimit)
            );
        }
    }
}

#[cfg_attr(test, test)]
fn module_binding_storage_utf16_payload_moves_from_native_to_managed_ownership() {
    let mut vm = page();
    let (cell, namespace) = destination(&mut vm);
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let managed = vm.heap.stats().managed_bytes;
    let names = ["", "é", "😀"];
    let mut exports = NamespaceExports::prepare(&names, vm.host_scheduling.as_ref()).unwrap();
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes - before,
        exports.bytes()
    );
    for _ in names {
        exports.keep_next(cell);
    }
    let payload = exports.payload_bytes();
    exports.publish(&mut vm, namespace).unwrap();
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(vm.heap.stats().managed_bytes, managed + payload);
    for name in names {
        assert_eq!(vm.heap.get(namespace, name), Ok(Value::Number(7.0)));
    }
}

#[cfg_attr(test, test)]
fn module_binding_storage_skipped_names_refund_payload_and_retain_backing() {
    let mut vm = page();
    let (cell, namespace) = destination(&mut vm);
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let mut exports =
        NamespaceExports::prepare(&["a", "skip"], vm.host_scheduling.as_ref()).unwrap();
    let staged = exports.bytes();
    exports.skip_next();
    assert_eq!(exports.bytes(), staged - 2);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes - before,
        staged - 2
    );
    exports.keep_next(cell);
    let payload = exports.payload_bytes();
    let managed = vm.heap.stats().managed_bytes;
    exports.publish(&mut vm, namespace).unwrap();
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(vm.heap.stats().managed_bytes, managed + payload);
    assert_eq!(vm.heap.get_own(namespace, "a"), Ok(None));
    assert_eq!(vm.heap.get(namespace, "skip"), Ok(Value::Number(7.0)));
}

#[cfg_attr(test, test)]
fn module_binding_storage_preparation_preserves_heap_roots_and_cache_state() {
    let mut vm = page();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let heap = vm.heap.stats();
    let epoch = vm.next_namespace_epoch;
    let exports = NamespaceExports::prepare(&["x", "longer"], vm.host_scheduling.as_ref()).unwrap();
    assert_eq!(vm.heap.stats(), heap);
    assert!(vm.module_namespace_cache.is_empty());
    assert_eq!(vm.next_namespace_epoch, epoch);
    assert!(!vm.preserve_promise_table_capacity);
    drop(exports);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn module_binding_storage_namespace_preparation_admits_exact_and_refuses_short_room() {
    for short in [false, true] {
        let mut vm = page();
        let preview = NamespaceExports::prepare(&["x", "😀"], vm.host_scheduling.as_ref()).unwrap();
        let bytes = preview.bytes();
        assert!(bytes > 0);
        drop(preview);
        let _peer = leave_room(&mut vm, bytes - usize::from(short));
        let before = vm.host_scheduling_snapshot().payload_bytes;
        let heap = vm.heap.stats();
        let result = NamespaceExports::prepare(&["x", "😀"], vm.host_scheduling.as_ref());
        if short {
            assert_eq!(result.err(), Some(RuntimeError::SchedulingLimit));
        } else {
            let exports = result.unwrap();
            assert_eq!(
                vm.host_scheduling_snapshot().payload_bytes,
                MAX_HOST_SCHEDULING_BYTES
            );
            drop(exports);
        }
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
        assert_eq!(vm.heap.stats(), heap);
    }
}

#[cfg_attr(test, test)]
fn module_binding_storage_detached_namespace_preparation_outlives_the_vm() {
    let vm = page();
    let observer = Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    };
    let exports = NamespaceExports::prepare(&["x", "😀"], vm.host_scheduling.as_ref()).unwrap();
    let bytes = exports.bytes();
    drop(vm);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, bytes);
    drop(exports);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn module_binding_storage_namespace_publication_keeps_cells_through_gc() {
    let mut vm = page();
    let (cell, namespace) = destination(&mut vm);
    let mut exports = NamespaceExports::prepare(&["x"], vm.host_scheduling.as_ref()).unwrap();
    exports.keep_next(cell);
    let major = vm.heap.stats().major_collections;
    vm.heap.allow_only(exports.payload_bytes());
    exports.publish(&mut vm, namespace).unwrap();
    assert!(vm.heap.stats().major_collections > major);
    assert_eq!(vm.heap.get(namespace, "x"), Ok(Value::Number(7.0)));
}

#[cfg_attr(test, test)]
fn module_binding_storage_heap_refusal_releases_namespace_staging() {
    let mut vm = page();
    let (cell, namespace) = destination(&mut vm);
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let mut exports = NamespaceExports::prepare(&["x"], vm.host_scheduling.as_ref()).unwrap();
    exports.keep_next(cell);
    vm.heap.allow_only(0);
    assert!(matches!(
        exports.publish(&mut vm, namespace),
        Err(RuntimeError::Heap(_))
    ));
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(vm.heap.get_own(namespace, "x"), Ok(None));
}

#[cfg_attr(test, test)]
fn module_binding_storage_trace_refusal_drops_data_before_its_owner() {
    let mut vm = page();
    let (cell, namespace) = destination(&mut vm);
    for _ in 0..4 {
        vm.stack.push(Value::Object(cell));
    }
    vm.host_scheduling_usage().unwrap();
    let mut exports = NamespaceExports::prepare(&["x"], vm.host_scheduling.as_ref()).unwrap();
    exports.keep_next(cell);
    let bytes = exports.bytes();
    let _peer = leave_room(&mut vm, 0);
    let heap = vm.heap.stats();
    assert_eq!(
        exports.publish(&mut vm, namespace),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - bytes
    );
    assert_eq!(vm.heap.stats(), heap);
}

#[cfg_attr(test, test)]
fn module_binding_storage_unbounded_preparation_admits_before_document_publication() {
    for short in [false, true] {
        let mut vm = page();
        let (cell, namespace) = destination(&mut vm);
        let mut exports = NamespaceExports::prepare(&["x"], None).unwrap();
        exports.keep_next(cell);
        let payload = exports.payload_bytes();
        let _peer = short.then(|| leave_room(&mut vm, payload - 1));
        let before = vm.host_scheduling_snapshot().payload_bytes;
        let result = exports.publish(&mut vm, namespace);
        if short {
            assert_eq!(result, Err(RuntimeError::SchedulingLimit));
            assert_eq!(vm.heap.get_own(namespace, "x"), Ok(None));
        } else {
            result.unwrap();
            assert_eq!(vm.heap.get(namespace, "x"), Ok(Value::Number(7.0)));
        }
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    }
}

#[cfg_attr(test, test)]
fn module_binding_storage_prepared_namespace_cannot_change_document_owners() {
    for bounded in [false, true] {
        let mut source = page();
        let before = source.host_scheduling_usage().unwrap().payload_bytes;
        let mut target = if bounded { page() } else { Vm::default() };
        let (cell, namespace) = destination(&mut target);
        let heap = target.heap.stats();
        let mut exports =
            NamespaceExports::prepare(&["x"], source.host_scheduling.as_ref()).unwrap();
        exports.keep_next(cell);
        assert_eq!(
            exports.publish(&mut target, namespace),
            Err(RuntimeError::SchedulingLimit)
        );
        assert_eq!(source.host_scheduling_snapshot().payload_bytes, before);
        assert_eq!(target.heap.stats(), heap);
        if bounded {
            assert_eq!(source.check_scheduling_failure(), Ok(()));
            assert_eq!(
                target.check_scheduling_failure(),
                Err(RuntimeError::SchedulingLimit)
            );
        } else {
            assert_eq!(
                source.check_scheduling_failure(),
                Err(RuntimeError::SchedulingLimit)
            );
        }
    }
}

#[cfg_attr(test, test)]
fn module_binding_storage_host_neutral_preparations_preserve_bindings() {
    let mut vm = Vm::default();
    let (cell, namespace) = destination(&mut vm);
    let code = compile("import {x} from 'leaf';");
    let slots = ImportedSlots::prepare(&code, None).unwrap();
    assert!(slots.contains(0));
    let mut aliases = Aliases::prepare(&code, None).unwrap();
    aliases.push(0, cell);
    let mut cells = HashMap::with_capacity(1);
    aliases.publish(&mut cells);
    assert_eq!(cells[&0], cell);
    let mut exports = NamespaceExports::prepare(&["x"], None).unwrap();
    exports.keep_next(cell);
    exports.publish(&mut vm, namespace).unwrap();
    assert_eq!(vm.heap.get(namespace, "x"), Ok(Value::Number(7.0)));
}

#[cfg_attr(test, test)]
fn module_binding_storage_terminal_cleanup_refunds_all_prepared_names() {
    let mut vm = page();
    let (_, namespace) = destination(&mut vm);
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let mut exports =
        NamespaceExports::prepare(&["a", "longer"], vm.host_scheduling.as_ref()).unwrap();
    vm.host_scheduling.as_ref().unwrap().fail();
    exports.skip_next();
    exports.skip_next();
    assert_eq!(
        exports.publish(&mut vm, namespace),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn module_binding_storage_real_mixed_imports_keep_live_and_deferred_semantics() {
    let modules = HashMap::from([
        ("root".into(), compile("import {x,bump} from 'middle'; import * as ns from 'leaf'; import defer * as lazy from 'leaf'; bump(); export const value=x+ns.x+lazy.x; export const same=ns===lazy;")),
        ("middle".into(), compile("export * from 'leaf'; export * as ns from 'leaf';")),
        ("leaf".into(), compile("export let x=1; export function bump(){x++;} export const then=2;")),
    ]);
    let mut vm = page();
    vm.execute_module_graph("root", &modules).unwrap();
    let namespace = Value::Object(vm.last_module_namespace.unwrap());
    assert_eq!(
        vm.get_property(&namespace, &"value".into()),
        Ok(Value::Number(6.0))
    );
    assert_eq!(
        vm.get_property(&namespace, &"same".into()),
        Ok(Value::Bool(false))
    );
}

#[cfg_attr(test, test)]
fn module_binding_storage_failed_link_preserves_old_cache_and_retries_new_cells() {
    let mut vm = page();
    let mut modules = HashMap::from([("keep".into(), compile("export const value=0;"))]);
    vm.execute_module_graph("keep", &modules).unwrap();
    let keep = vm.last_module_namespace.unwrap();
    modules.insert(
        "root".into(),
        compile(
            "import * as ns from 'leaf'; import {missing} from 'leaf'; export const value=ns.x;",
        ),
    );
    modules.insert("leaf".into(), compile("export const x=1;"));
    assert!(matches!(
        vm.execute_module_graph("root", &modules),
        Err(RuntimeError::ModuleResolution(_))
    ));
    assert_eq!(vm.module_namespace_cache["keep"], keep);
    assert!(!vm.module_namespace_cache.contains_key("leaf"));
    modules.insert(
        "root".into(),
        compile("import * as ns from 'leaf'; import {x} from 'leaf'; export const value=ns.x;"),
    );
    modules.insert("leaf".into(), compile("export const x=2;"));
    vm.execute_module_graph("root", &modules).unwrap();
    let namespace = Value::Object(vm.last_module_namespace.unwrap());
    assert_eq!(
        vm.get_property(&namespace, &"value".into()),
        Ok(Value::Number(2.0))
    );
    let observer = Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    };
    drop(vm);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, 0);
}

impl crate::Vm {
    /// Runs finite binding preparation, native lifetime and heap handoff contracts.
    #[doc(hidden)]
    pub fn verify_module_binding_storage_contracts() {
        module_binding_storage_namespace_payload_retains_native_ownership();
        module_binding_storage_import_slots_keep_contents_and_refund_backing();
        module_binding_storage_import_slots_admit_exact_and_refuse_short_room();
        module_binding_storage_aliases_admit_exact_and_refuse_short_room();
        module_binding_storage_alias_publication_needs_no_more_storage();
        module_binding_storage_detached_import_metadata_outlives_the_vm();
        module_binding_storage_empty_preparations_work_at_full_healthy_quota();
        module_binding_storage_terminal_scope_refuses_every_preparation();
        module_binding_storage_utf16_payload_moves_from_native_to_managed_ownership();
        module_binding_storage_skipped_names_refund_payload_and_retain_backing();
        module_binding_storage_preparation_preserves_heap_roots_and_cache_state();
        module_binding_storage_namespace_preparation_admits_exact_and_refuses_short_room();
        module_binding_storage_detached_namespace_preparation_outlives_the_vm();
        module_binding_storage_namespace_publication_keeps_cells_through_gc();
        module_binding_storage_heap_refusal_releases_namespace_staging();
        module_binding_storage_trace_refusal_drops_data_before_its_owner();
        module_binding_storage_unbounded_preparation_admits_before_document_publication();
        module_binding_storage_prepared_namespace_cannot_change_document_owners();
        module_binding_storage_host_neutral_preparations_preserve_bindings();
        module_binding_storage_terminal_cleanup_refunds_all_prepared_names();
        module_binding_storage_real_mixed_imports_keep_live_and_deferred_semantics();
        module_binding_storage_failed_link_preserves_old_cache_and_retries_new_cells();
    }
}
