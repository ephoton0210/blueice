// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny public module graphs verify explicit namespace construction frames.

use super::*;

#[cfg(test)]
thread_local! {
    static CALLS: std::cell::Cell<Option<(usize,usize)>> = const { std::cell::Cell::new(None) };
}
#[cfg(test)]
pub(super) struct CallProbe;
#[cfg(test)]
impl CallProbe {
    pub(super) fn enter() -> Self {
        CALLS.with(|calls| {
            if let Some((depth, peak)) = calls.get() {
                calls.set(Some((depth + 1, peak.max(depth + 1))));
            }
        });
        Self
    }
    fn measure<T>(operation: impl FnOnce() -> T) -> (T, usize) {
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                CALLS.with(|calls| calls.set(None));
            }
        }
        CALLS.with(|calls| {
            assert!(calls.get().is_none());
            calls.set(Some((0, 0)));
        });
        let _reset = Reset;
        let result = operation();
        let (depth, peak) = CALLS.with(std::cell::Cell::get).unwrap();
        assert_eq!(depth, 0);
        (result, peak)
    }
}
#[cfg(test)]
impl Drop for CallProbe {
    fn drop(&mut self) {
        CALLS.with(|calls| {
            if let Some((depth, peak)) = calls.get() {
                calls.set(Some((depth - 1, peak)));
            }
        });
    }
}

fn graph(sources: &[(&str, &str)]) -> HashMap<String, Bytecode> {
    sources
        .iter()
        .map(|(name, source)| {
            (
                (*name).to_owned(),
                crate::compile_module(&crate::parse_module(source).unwrap()).unwrap(),
            )
        })
        .collect()
}
fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}

#[cfg(test)]
#[test]
fn namespace_build_stack_public_chain_uses_one_active_host_call() {
    let mut vm = page();
    let modules = graph(&[
        ("t/a", "export * as next from './b';"),
        ("t/b", "export * as next from './c';"),
        ("t/c", ""),
    ]);
    let (result, depth) = CallProbe::measure(|| vm.execute_module_graph("t/a", &modules));
    result.unwrap();
    assert_eq!(depth, 1);
}
#[cfg(test)]
#[test]
fn namespace_build_stack_public_cycle_uses_one_active_host_call() {
    let mut vm = page();
    let modules = graph(&[
        ("t/a", "export * as next from './b';"),
        ("t/b", "export * as next from './c';"),
        ("t/c", "export * as next from './a';"),
    ]);
    let (result, depth) = CallProbe::measure(|| vm.execute_module_graph("t/a", &modules));
    result.unwrap();
    assert_eq!(depth, 1);
}
#[cfg(test)]
#[test]
fn namespace_build_stack_public_deferred_chain_uses_one_active_host_call() {
    let mut vm = page();
    let modules = graph(&[
        ("t/a", "import defer * as next from './b'; export {next};"),
        ("t/b", "import defer * as next from './c'; export {next};"),
        ("t/c", ""),
    ]);
    let (result, depth) = CallProbe::measure(|| vm.execute_module_graph("t/a", &modules));
    result.unwrap();
    assert_eq!(depth, 1);
}

use crate::vm::host_scheduling::SharedSchedulingCharge;
fn leave_room(vm: &mut Vm, room: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - room)
        .unwrap()
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
fn linked(modules: &HashMap<String, Bytecode>) -> HashMap<String, LinkedModule> {
    modules.keys().map(|key| (key.clone(), record())).collect()
}

#[cfg_attr(test, test)]
fn namespace_build_stack_frame_backing_has_a_detached_document_owner() {
    let vm = page();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let observer = Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    };
    let mut frames = Storage::<NamespaceFrame<'_>>::default();
    frames.reserve(1, vm.host_scheduling.as_ref()).unwrap();
    let bytes = frames.bytes();
    assert!(bytes > 0);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes - before, bytes);
    drop(vm);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, bytes);
    drop(frames);
    assert_eq!(observer.host_scheduling_snapshot().payload_bytes, 0);
}

#[cfg_attr(test, test)]
fn namespace_build_stack_exact_frame_admission_and_one_short_refusal() {
    let mut preview = Storage::<NamespaceFrame<'_>>::default();
    preview.reserve(1, None).unwrap();
    let bytes = preview.bytes();
    for accepts in [false, true] {
        let mut vm = page();
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        let peer = leave_room(&mut vm, bytes - usize::from(!accepts));
        let mut frames = Storage::<NamespaceFrame<'_>>::default();
        let result = frames.reserve(1, vm.host_scheduling.as_ref());
        if accepts {
            result.unwrap();
            assert_eq!(
                vm.host_scheduling_snapshot().payload_bytes,
                MAX_HOST_SCHEDULING_BYTES
            );
        } else {
            assert_eq!(result, Err(RuntimeError::SchedulingLimit));
            assert_eq!(frames.data.capacity(), 0);
        }
        drop(frames);
        drop(peer);
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    }
}

#[cfg_attr(test, test)]
fn namespace_build_stack_spare_frame_capacity_needs_no_new_headroom() {
    let mut vm = page();
    let mut frames = Storage::<NamespaceFrame<'_>>::default();
    frames.reserve(1, vm.host_scheduling.as_ref()).unwrap();
    let pointer = frames.data.as_ptr();
    let _peer = leave_room(&mut vm, 0);
    frames.reserve(1, vm.host_scheduling.as_ref()).unwrap();
    assert_eq!(frames.data.as_ptr(), pointer);
}

#[cfg_attr(test, test)]
fn namespace_build_stack_owned_exports_outlive_the_name_input() {
    let mut vm = page();
    let cell = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    vm.heap.root(cell).unwrap();
    vm.with_roots(|heap| heap.set(cell, "value", Value::Number(7.0)))
        .unwrap();
    let namespace = vm
        .with_roots(|heap| heap.alloc_module_namespace(Vec::new(), false))
        .unwrap();
    vm.heap.root(namespace).unwrap();
    let mut exports = {
        let input = ["a😀".to_owned()];
        let source = [input[0].as_str()];
        module_binding_storage::NamespaceExports::prepare(&source, vm.host_scheduling.as_ref())
            .unwrap()
    };
    exports.keep_next(cell);
    exports.publish(&mut vm, namespace).unwrap();
    assert_eq!(vm.heap.get(namespace, "a😀"), Ok(Value::Number(7.0)));
}

#[cfg_attr(test, test)]
fn namespace_build_stack_moving_frames_keeps_names_and_export_payload_valid() {
    let mut vm = page();
    let modules = graph(&[("root", "export const x=1;")]);
    let mut linked = linked(&modules);
    let mut roots = ModuleRoots::default();
    let cell = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    vm.heap.root(cell).unwrap();
    vm.with_roots(|heap| heap.set(cell, "value", Value::Number(9.0)))
        .unwrap();
    let frame = vm
        .prepare_namespace_frame("root", false, &modules, &mut linked, &mut roots)
        .unwrap();
    let namespace = frame.namespace;
    let pointer = frame.names.as_ptr();
    let mut frames = Storage::default();
    frames.reserve(1, vm.host_scheduling.as_ref()).unwrap();
    frames.data.push(frame);
    frames.reserve(5, vm.host_scheduling.as_ref()).unwrap();
    let mut frame = frames.data.pop().unwrap();
    assert_eq!(frame.names.as_ptr(), pointer);
    frame.exports.keep_next(cell);
    frame.exports.publish(&mut vm, namespace).unwrap();
    assert_eq!(vm.heap.get(namespace, "x"), Ok(Value::Number(9.0)));
}

#[cfg_attr(test, test)]
fn namespace_build_stack_growth_refusal_keeps_all_live_frames() {
    let mut vm = page();
    let modules = graph(&[("a", ""), ("b", ""), ("c", ""), ("d", "")]);
    let mut linked = linked(&modules);
    let mut roots = ModuleRoots::default();
    let mut frames = Storage::default();
    frames.reserve(4, vm.host_scheduling.as_ref()).unwrap();
    for name in ["a", "b", "c", "d"] {
        frames.data.push(
            vm.prepare_namespace_frame(name, false, &modules, &mut linked, &mut roots)
                .unwrap(),
        );
    }
    let pointer = frames.data.as_ptr();
    let namespaces: Vec<_> = frames.data.iter().map(|f| f.namespace).collect();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        frames.reserve(1, vm.host_scheduling.as_ref()),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(frames.data.len(), 4);
    assert_eq!(frames.data.as_ptr(), pointer);
    assert_eq!(
        frames.data.iter().map(|f| f.namespace).collect::<Vec<_>>(),
        namespaces
    );
}

#[cfg_attr(test, test)]
fn namespace_build_stack_public_cycle_preserves_every_namespace_identity() {
    let mut vm = page();
    let modules = graph(&[
        ("t/a", "export * as next from './b';"),
        ("t/b", "export * as next from './c';"),
        ("t/c", "export * as next from './a';"),
    ]);
    vm.execute_module_graph("t/a", &modules).unwrap();
    let a = vm.last_module_namespace.unwrap();
    let b = vm.heap.get(a, "next").unwrap().object_id().unwrap();
    let c = vm.heap.get(b, "next").unwrap().object_id().unwrap();
    assert_eq!(vm.heap.get(c, "next"), Ok(Value::Object(a)));
    assert_ne!(a, b);
    assert_ne!(b, c);
    vm.collect_realm_garbage().unwrap();
    assert_eq!(vm.heap.get(c, "next"), Ok(Value::Object(a)));
}

#[cfg_attr(test, test)]
fn namespace_build_stack_self_reference_finishes_one_placeholder() {
    let mut vm = page();
    let modules = graph(&[("root", "export * as own from 'root';")]);
    let mut linked = linked(&modules);
    let mut roots = ModuleRoots::default();
    let namespace = vm
        .module_namespace("root", false, &modules, &mut linked, &mut roots)
        .unwrap();
    assert_eq!(vm.heap.get(namespace, "own"), Ok(Value::Object(namespace)));
    assert_eq!(vm.module_namespace_cache.len(), 1);
    assert_eq!(vm.namespace_transaction_depth, 0);
}

#[cfg_attr(test, test)]
fn namespace_build_stack_deferred_construction_omits_then_before_resolution() {
    let mut vm = page();
    let modules = graph(&[
        (
            "root",
            "export * as then from 'missing'; export * as present from 'child';",
        ),
        ("child", ""),
    ]);
    let mut linked = linked(&modules);
    let mut roots = ModuleRoots::default();
    let namespace = vm
        .module_namespace("root", true, &modules, &mut linked, &mut roots)
        .unwrap();
    assert_eq!(vm.heap.own_keys(namespace), Ok(vec!["present".into()]));
    assert!(!vm.module_namespace_cache.contains_key("missing"));
    assert_eq!(vm.deferred_namespaces[&namespace], "root");
}

#[cfg_attr(test, test)]
fn namespace_build_stack_eager_and_deferred_self_aliases_keep_distinct_identities() {
    let mut vm = page();
    let mut modules = graph(&[("root", "")]);
    modules
        .get_mut("root")
        .unwrap()
        .module_exports
        .push(ModuleExport::DeferredNamespace {
            export_name: "later".into(),
            module_request: "root".into(),
            module_type: ModuleType::JavaScript,
        });
    let mut linked = linked(&modules);
    let mut roots = ModuleRoots::default();
    let eager = vm
        .module_namespace("root", false, &modules, &mut linked, &mut roots)
        .unwrap();
    let deferred = vm.module_deferred_namespace_cache["root"];
    assert_ne!(eager, deferred);
    assert_eq!(vm.heap.get(eager, "later"), Ok(Value::Object(deferred)));
    assert_eq!(vm.heap.get(deferred, "later"), Ok(Value::Object(deferred)));
}

#[cfg_attr(test, test)]
fn namespace_build_stack_source_aliases_use_the_stable_host_identity() {
    let mut vm = page();
    vm.set_module_source_loader_context(vec!["blob".into()]);
    let modules = graph(&[("root", "import source blob from 'blob'; export {blob};")]);
    let mut linked = linked(&modules);
    let mut roots = ModuleRoots::default();
    let namespace = vm
        .module_namespace("root", false, &modules, &mut linked, &mut roots)
        .unwrap();
    assert_eq!(
        vm.heap.get(namespace, "blob"),
        Ok(Value::Object(vm.module_source_cache["blob"]))
    );
}

#[cfg_attr(test, test)]
fn namespace_build_stack_local_exports_reuse_the_original_binding_cell() {
    let mut vm = page();
    let modules = graph(&[("root", "export let x=1;")]);
    let mut linked = linked(&modules);
    let mut roots = ModuleRoots::default();
    let cell = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    vm.heap.root(cell).unwrap();
    vm.with_roots(|heap| heap.set(cell, "value", Value::Number(7.0)))
        .unwrap();
    linked.get_mut("root").unwrap().cells.insert(0, cell);
    let namespace = vm
        .module_namespace("root", false, &modules, &mut linked, &mut roots)
        .unwrap();
    vm.with_roots(|heap| heap.set(cell, "value", Value::Number(8.0)))
        .unwrap();
    assert_eq!(vm.heap.get(namespace, "x"), Ok(Value::Number(8.0)));
}

#[cfg_attr(test, test)]
fn namespace_build_stack_ambiguous_exports_are_omitted() {
    let mut vm = page();
    let modules = graph(&[
        ("root", "export * from 'a';export * from 'b';"),
        ("a", "export const x=1;"),
        ("b", "export const x=2;"),
    ]);
    let mut linked = linked(&modules);
    let mut roots = ModuleRoots::default();
    let namespace = vm
        .module_namespace("root", false, &modules, &mut linked, &mut roots)
        .unwrap();
    assert_eq!(vm.heap.own_keys(namespace), Ok(Vec::new()));
}

#[cfg_attr(test, test)]
fn namespace_build_stack_late_failure_rolls_back_after_frame_growth() {
    let mut vm = page();
    let modules = graph(&[
        ("good", ""),
        ("a", "export * as child from 'b';"),
        ("b", "export * as child from 'c';"),
        ("c", "export * as child from 'd';"),
        ("d", "export * as child from 'e';"),
        ("e", "export * as child from 'f';"),
        ("f", "export * as missing from 'absent';"),
    ]);
    let mut linked = linked(&modules);
    let mut roots = ModuleRoots::default();
    let good = vm
        .module_namespace("good", false, &modules, &mut linked, &mut roots)
        .unwrap();
    let checkpoint = roots.len();
    assert!(matches!(
        vm.module_namespace("a", false, &modules, &mut linked, &mut roots),
        Err(RuntimeError::ModuleResolution(_))
    ));
    assert_eq!(roots.len(), checkpoint);
    assert_eq!(vm.module_namespace_cache.len(), 1);
    assert_eq!(vm.module_namespace_cache["good"], good);
    assert_eq!(vm.namespace_transaction_depth, 0);
    for name in ["a", "b", "c", "d", "e", "f"] {
        assert!(linked[name].namespace.is_none());
        assert_eq!(linked[name].namespace_epochs, [0; 2]);
    }
}

#[cfg_attr(test, test)]
fn namespace_build_stack_cache_hit_needs_no_frame_room() {
    let mut vm = page();
    let modules = graph(&[("root", "")]);
    let mut linked = linked(&modules);
    let mut roots = ModuleRoots::default();
    let namespace = vm
        .module_namespace("root", false, &modules, &mut linked, &mut roots)
        .unwrap();
    let _peer = leave_room(&mut vm, 0);
    let heap = vm.heap.stats();
    assert_eq!(
        vm.module_namespace("root", false, &modules, &mut linked, &mut roots),
        Ok(namespace)
    );
    assert_eq!(vm.heap.stats(), heap);
}

#[cfg_attr(test, test)]
fn namespace_build_stack_inherited_transaction_and_capacity_policy_are_restored() {
    let mut vm = page();
    let modules = graph(&[("root", "export * as missing from 'absent';")]);
    let mut linked = linked(&modules);
    let mut roots = ModuleRoots::default();
    let checkpoint = vm.next_namespace_epoch;
    vm.namespace_transaction_depth = 1;
    vm.preserve_promise_table_capacity = true;
    assert!(matches!(
        vm.module_namespace("root", false, &modules, &mut linked, &mut roots),
        Err(RuntimeError::ModuleResolution(_))
    ));
    assert_eq!(vm.namespace_transaction_depth, 1);
    assert!(vm.preserve_promise_table_capacity);
    assert!(vm.module_namespace_cache.contains_key("root"));
    vm.rollback_namespace_cache_from(checkpoint, &mut linked);
    vm.release_module_roots_from(&mut roots, 0);
    assert!(vm.module_namespace_cache.is_empty());
}

#[cfg_attr(test, test)]
fn namespace_build_stack_root_refusal_restores_outer_state() {
    let mut vm = page();
    let modules = graph(&[("root", "")]);
    let mut linked = linked(&modules);
    let mut roots = ModuleRoots::default();
    vm.heap.allow_root_registrations(0);
    assert_eq!(
        vm.module_namespace("root", false, &modules, &mut linked, &mut roots),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    assert!(roots.is_empty());
    assert!(vm.module_namespace_cache.is_empty());
    assert_eq!(vm.namespace_transaction_depth, 0);
    assert!(!vm.preserve_promise_table_capacity);
}

#[cfg_attr(test, test)]
fn namespace_build_stack_host_neutral_construction_preserves_cycles() {
    let mut vm = Vm::default();
    let modules = graph(&[("root", "export * as own from 'root';")]);
    let mut linked = linked(&modules);
    let mut roots = ModuleRoots::default();
    let namespace = vm
        .module_namespace("root", false, &modules, &mut linked, &mut roots)
        .unwrap();
    assert_eq!(vm.heap.get(namespace, "own"), Ok(Value::Object(namespace)));
    assert!(vm.host_scheduling.is_none());
}

impl crate::Vm {
    /// Runs finite namespace frame ownership and rollback contracts.
    #[doc(hidden)]
    pub fn verify_namespace_build_stack_contracts() {
        namespace_build_stack_frame_backing_has_a_detached_document_owner();
        namespace_build_stack_exact_frame_admission_and_one_short_refusal();
        namespace_build_stack_spare_frame_capacity_needs_no_new_headroom();
        namespace_build_stack_owned_exports_outlive_the_name_input();
        namespace_build_stack_moving_frames_keeps_names_and_export_payload_valid();
        namespace_build_stack_growth_refusal_keeps_all_live_frames();
        namespace_build_stack_public_cycle_preserves_every_namespace_identity();
        namespace_build_stack_self_reference_finishes_one_placeholder();
        namespace_build_stack_deferred_construction_omits_then_before_resolution();
        namespace_build_stack_eager_and_deferred_self_aliases_keep_distinct_identities();
        namespace_build_stack_source_aliases_use_the_stable_host_identity();
        namespace_build_stack_local_exports_reuse_the_original_binding_cell();
        namespace_build_stack_ambiguous_exports_are_omitted();
        namespace_build_stack_late_failure_rolls_back_after_frame_growth();
        namespace_build_stack_cache_hit_needs_no_frame_room();
        namespace_build_stack_inherited_transaction_and_capacity_policy_are_restored();
        namespace_build_stack_root_refusal_restores_outer_state();
        namespace_build_stack_host_neutral_construction_preserves_cycles();
    }
}
