// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Finite graph walks exercise native admission and traversal lifetimes.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn compile(source: &str) -> Bytecode {
    crate::compile_module(&crate::parse_module(source).unwrap()).unwrap()
}
fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}
fn record(evaluating: bool) -> LinkedModule {
    LinkedModule {
        cells: HashMap::new(),
        cells_charge: Vm::default().charge_module_cells(0),
        namespace: None,
        deferred_namespace: None,
        namespace_epochs: [0; 2],
        evaluated: false,
        evaluating,
        suspended: evaluating,
        completion: None,
        error: None,
        host_error_pending: false,
    }
}
fn graph(cycle: bool) -> (HashMap<String, Bytecode>, HashMap<String, LinkedModule>) {
    let modules = HashMap::from([
        (
            "test/root.mjs".into(),
            compile("import './leaf.mjs'; export const value=1;"),
        ),
        (
            "test/leaf.mjs".into(),
            compile(if cycle {
                "import './root.mjs'; export const value=2;"
            } else {
                "export const value=2;"
            }),
        ),
    ]);
    let linked = HashMap::from([
        ("test/root.mjs".into(), record(cycle)),
        ("test/leaf.mjs".into(), record(false)),
    ]);
    (modules, linked)
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
fn module_walk_storage_frontier_refusal_precedes_visited_allocation() {
    let mut vm = page();
    let (modules, linked) = graph(false);
    let _peer = leave_room(&mut vm, 0);
    let (result, allocations) = allocation_observer::count(|| {
        Vm::gather_async_dependency_names(
            "test/root.mjs",
            &modules,
            &linked,
            vm.host_scheduling.as_ref(),
        )
    });
    assert_eq!(result.err(), Some(RuntimeError::SchedulingLimit));
    assert_eq!(allocations, 0, "visited scratch must be admitted first");
}
#[cfg(test)]
#[test]
fn module_walk_storage_cycle_root_refuses_before_reachability_scratch() {
    let mut vm = page();
    let (modules, linked) = graph(true);
    vm.module_async_parents.reserve(1);
    vm.module_async_parents
        .insert("test/leaf.mjs".into(), vec!["test/root.mjs".into()]);
    let _peer = leave_room(&mut vm, 0);
    let (result, allocations) =
        allocation_observer::count(|| vm.async_dependency_root("test/leaf.mjs", &modules, &linked));
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(allocations, 0);
}
#[cfg(test)]
#[test]
fn module_walk_storage_sync_readiness_refuses_before_language_error_allocation() {
    let mut vm = page();
    let script =
        crate::compile(&crate::parse("var gate=Promise.withResolvers();").unwrap()).unwrap();
    vm.execute_script(&script).unwrap();
    let modules = HashMap::from([(
        "test/leaf.mjs".into(),
        compile("await gate.promise; export const value=1;"),
    )]);
    vm.execute_module_graph("test/leaf.mjs", &modules).unwrap();
    let heap = vm.heap.stats();
    let _peer = leave_room(&mut vm, 0);
    let (result, allocations) =
        allocation_observer::count(|| vm.evaluate_module_sync("test/leaf.mjs"));
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(allocations, 0);
    assert_eq!(vm.heap.stats(), heap);
    assert!(vm.linked_record("test/leaf.mjs").unwrap().suspended);
}

fn chain(count: usize) -> HashMap<String, Bytecode> {
    (0..count)
        .map(|index| {
            let source = if index + 1 < count {
                format!("import 'n{}';", index + 1)
            } else {
                "export const value=1;".into()
            };
            (format!("n{index}"), compile(&source))
        })
        .collect()
}
#[cfg_attr(test, test)]
fn module_walk_storage_identity_uses_no_scratch_at_full_quota() {
    let mut vm = page();
    let (modules, _) = graph(false);
    let mut queries = ModuleQueries::new(&modules, vm.host_scheduling.as_ref()).unwrap();
    let _peer = leave_room(&mut vm, 0);
    #[cfg(test)]
    {
        let (result, allocations) =
            allocation_observer::count(|| queries.reaches("absent", "absent"));
        assert_eq!(result, Ok(true));
        assert_eq!(allocations, 0);
    }
    #[cfg(not(test))]
    assert_eq!(queries.reaches("absent", "absent"), Ok(true));
    assert_eq!(queries.bytes(), 0);
}
#[cfg_attr(test, test)]
fn module_walk_storage_visited_names_borrow_canonical_registry_keys() {
    let vm = page();
    let (modules, _) = graph(false);
    let mut queries = ModuleQueries::new(&modules, vm.host_scheduling.as_ref()).unwrap();
    let mut supplied = String::with_capacity(64);
    supplied.push_str("test/root.mjs");
    assert_eq!(queries.reaches(&supplied, "absent"), Ok(false));
    assert_eq!(queries.visited().count(), 2);
    for name in queries.visited() {
        let canonical = modules.get_key_value(name).unwrap().0;
        assert_eq!(name.as_ptr(), canonical.as_ptr());
        assert_ne!(name.as_ptr(), supplied.as_ptr());
    }
}
#[cfg_attr(test, test)]
fn module_walk_storage_reuses_visited_and_frames_at_full_quota() {
    let mut vm = page();
    let modules = chain(1);
    let mut queries = ModuleQueries::new(&modules, vm.host_scheduling.as_ref()).unwrap();
    assert_eq!(queries.reaches("n0", "absent"), Ok(false));
    let retained = queries.bytes();
    let _peer = leave_room(&mut vm, 0);
    #[cfg(test)]
    {
        let (result, allocations) = allocation_observer::count(|| queries.reaches("n0", "absent"));
        assert_eq!(result, Ok(false));
        assert_eq!(allocations, 0);
    }
    #[cfg(not(test))]
    assert_eq!(queries.reaches("n0", "absent"), Ok(false));
    assert_eq!(queries.bytes(), retained);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}
#[cfg_attr(test, test)]
fn module_walk_storage_exact_and_short_operation_peaks_preserve_graph_and_heap() {
    for short in [false, true] {
        let mut vm = page();
        let modules = chain(5);
        let scope = vm.host_scheduling.as_ref().unwrap().child();
        vm.host_scheduling_usage().unwrap();
        let mut preview = ModuleQueries::new(&modules, Some(&scope)).unwrap();
        let (result, peak) = scope.measure_payload_peak(|| preview.reaches("n0", "absent"));
        assert_eq!(result, Ok(false));
        drop(preview);
        let heap = vm.heap.stats();
        let _peer = leave_room(&mut vm, peak - usize::from(short));
        let before = vm.host_scheduling_snapshot().payload_bytes;
        let mut queries = ModuleQueries::new(&modules, Some(&scope)).unwrap();
        let result = queries.reaches("n0", "absent");
        assert_eq!(
            result,
            if short {
                Err(RuntimeError::SchedulingLimit)
            } else {
                Ok(false)
            }
        );
        assert_eq!(
            vm.host_scheduling_snapshot().payload_bytes,
            before + queries.bytes()
        );
        assert_eq!(vm.heap.stats(), heap);
        assert_eq!(modules.len(), 5);
        drop(queries);
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    }
}
#[cfg_attr(test, test)]
fn module_walk_storage_visited_growth_requires_the_full_hash_candidate() {
    let mut vm = page();
    let modules = chain(4);
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let mut queries = ModuleQueries::new(&modules, Some(&scope)).unwrap();
    assert_eq!(queries.reaches("n3", "absent"), Ok(false));
    let before = queries.bytes();
    let net = (HashSet::<&str>::with_capacity(4).capacity()
        - HashSet::<&str>::with_capacity(1).capacity())
        * std::mem::size_of::<&str>();
    let _peer = leave_room(&mut vm, net);
    let used = vm.host_scheduling_snapshot().payload_bytes;
    assert_eq!(
        queries.reaches("n0", "absent"),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(queries.bytes(), before);
    assert_eq!(queries.visited().count(), 3);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, used);
}
#[cfg_attr(test, test)]
fn module_walk_storage_frame_growth_requires_the_full_vector_candidate() {
    let mut vm = page();
    let modules = chain(5);
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let mut queries = ModuleQueries::new(&modules, Some(&scope)).unwrap();
    assert_eq!(queries.reaches("n1", "absent"), Ok(false));
    let before = queries.bytes();
    let _peer = leave_room(&mut vm, queries.frame_bytes());
    let used = vm.host_scheduling_snapshot().payload_bytes;
    assert_eq!(
        queries.reaches("n0", "absent"),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(queries.visited().count(), 5);
    assert_eq!(queries.bytes(), before);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, used);
}
#[cfg_attr(test, test)]
fn module_walk_storage_detached_scratch_survives_vm_destruction() {
    let vm = page();
    let view = Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    };
    let modules = chain(3);
    let mut queries = ModuleQueries::new(&modules, vm.host_scheduling.as_ref()).unwrap();
    assert_eq!(queries.reaches("n0", "absent"), Ok(false));
    let held = queries.bytes();
    assert!(held > 0);
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, held);
    drop(queries);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn module_walk_storage_terminal_reuse_refuses_without_mutating_scratch() {
    let mut vm = page();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let modules = chain(2);
    let mut queries = ModuleQueries::new(&modules, Some(&scope)).unwrap();
    assert_eq!(queries.reaches("n0", "absent"), Ok(false));
    let held = queries.bytes();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    scope.fail();
    assert_eq!(
        queries.reaches("n0", "n0"),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(queries.bytes(), held);
    assert_eq!(queries.visited().count(), 2);
    drop(queries);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before - held);
}
#[cfg_attr(test, test)]
fn module_walk_storage_reachability_ignores_deferred_edges_and_keeps_missing_goal_semantics() {
    let modules = HashMap::from([
        (
            "a".into(),
            compile("import defer * as ns from 'b'; import 'missing';"),
        ),
        ("b".into(), compile("import 'a';")),
    ]);
    let mut queries = ModuleQueries::new(&modules, None).unwrap();
    assert_eq!(queries.reaches("a", "missing"), Ok(true));
    assert_eq!(
        queries.reaches("a", "b"),
        Err(RuntimeError::ModuleResolution(
            "module missing was not linked".into()
        ))
    );
    assert_eq!(queries.reaches("b", "a"), Ok(true));
}
#[cfg_attr(test, test)]
fn module_walk_storage_cycle_and_diamond_frontiers_keep_source_order_and_unique_members() {
    let mut vm = page();
    let modules: HashMap<String, Bytecode> = HashMap::from([
        ("root".into(), compile("import 'a'; import 'b';")),
        ("a".into(), compile("import 'hot'; import 'root';")),
        ("b".into(), compile("import 'hot'; import 'cold';")),
        ("hot".into(), compile("await 0;")),
        ("cold".into(), compile("await 0;")),
    ]);
    let linked = modules
        .keys()
        .map(|name| (name.clone(), record(false)))
        .collect();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let heap = vm.heap.stats();
    let result =
        Vm::gather_async_dependency_names("root", &modules, &linked, vm.host_scheduling.as_ref())
            .unwrap();
    assert_eq!(result.as_slice(), ["hot", "cold"]);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        before + result.bytes()
    );
    assert_eq!(vm.heap.stats(), heap);
    drop(result);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}
#[cfg_attr(test, test)]
fn module_walk_storage_sync_cycles_and_missing_records_remain_ready() {
    let (modules, mut linked) = graph(true);
    linked.get_mut("test/root.mjs").unwrap().evaluating = false;
    linked.get_mut("test/root.mjs").unwrap().suspended = false;
    assert_eq!(
        Vm::ready_for_sync_execution("test/root.mjs", &modules, &linked, None),
        Ok(true)
    );
    linked.remove("test/leaf.mjs");
    assert_eq!(
        Vm::ready_for_sync_execution("test/root.mjs", &modules, &linked, None),
        Ok(true)
    );
    assert_eq!(
        Vm::ready_for_sync_execution("absent", &modules, &linked, None),
        Ok(true)
    );
}
#[cfg_attr(test, test)]
fn module_walk_storage_scc_stops_at_an_unsettled_eager_cycle_only() {
    for deferred in [false, true] {
        let source = if deferred {
            "import defer * as ns from 'b';"
        } else {
            "import 'b';"
        };
        let modules = HashMap::from([
            ("a".into(), compile(source)),
            ("b".into(), compile("import 'a'; await 0;")),
        ]);
        let mut linked = HashMap::from([("a".into(), record(false)), ("b".into(), record(true))]);
        linked.get_mut("a").unwrap().evaluated = true;
        assert_eq!(
            Vm::scc_evaluated("a", &modules, &linked, None),
            Ok(deferred)
        );
    }
}
#[cfg_attr(test, test)]
fn module_walk_storage_scc_candidate_storage_is_admitted_before_allocation() {
    let mut vm = page();
    let (modules, mut linked) = graph(false);
    linked.get_mut("test/root.mjs").unwrap().evaluated = true;
    let _peer = leave_room(&mut vm, 0);
    let result = Vm::scc_evaluated(
        "test/root.mjs",
        &modules,
        &linked,
        vm.host_scheduling.as_ref(),
    );
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}
#[cfg_attr(test, test)]
fn module_walk_storage_scc_candidates_and_reachability_reuse_complete_backing() {
    let mut vm = page();
    let modules = HashMap::from([
        ("a".into(), compile("export const value=1;")),
        ("b".into(), compile("export const value=2;")),
    ]);
    let mut linked = HashMap::from([("a".into(), record(false)), ("b".into(), record(false))]);
    linked.get_mut("a").unwrap().evaluated = true;
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    let mut queries = ModuleQueries::new(&modules, Some(&scope)).unwrap();
    assert_eq!(queries.scc_evaluated("a", &linked), Ok(true));
    let held = queries.bytes();
    let _peer = leave_room(&mut vm, 0);
    #[cfg(test)]
    {
        let (result, allocations) =
            allocation_observer::count(|| queries.scc_evaluated("a", &linked));
        assert_eq!(result, Ok(true));
        assert_eq!(allocations, 0);
    }
    #[cfg(not(test))]
    assert_eq!(queries.scc_evaluated("a", &linked), Ok(true));
    assert_eq!(queries.bytes(), held);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}
#[cfg_attr(test, test)]
fn module_walk_storage_cycle_errors_keep_lexical_candidate_identity() {
    let mut vm = page();
    let modules = HashMap::from([
        ("a".into(), compile("import 'c'; import 'b';")),
        ("b".into(), compile("import 'a';")),
        ("c".into(), compile("import 'a';")),
    ]);
    let mut linked = HashMap::from([
        ("a".into(), record(false)),
        ("c".into(), record(false)),
        ("b".into(), record(false)),
    ]);
    for (name, value) in [("c", 3.0), ("b", 2.0)] {
        linked.get_mut(name).unwrap().error = Some(
            vm.retain_module_error(&Value::Number(value), ThrowOrigin::default())
                .unwrap(),
        );
    }
    let mut queries = ModuleQueries::new(&modules, vm.host_scheduling.as_ref()).unwrap();
    let selected = queries.cycle_error("a", &linked).unwrap().unwrap();
    assert_eq!(
        vm.propagate_module_error(selected),
        RuntimeError::Thrown(Value::Number(2.0))
    );
}
#[cfg_attr(test, test)]
fn module_walk_storage_loading_keeps_resolution_and_reverse_pending_error_order() {
    for (source, expected) in [
        (
            "import 'missing'; import '../../escape';",
            "relative module request ../../escape escapes its host root",
        ),
        (
            "import '../../escape'; import 'missing';",
            "relative module request ../../escape escapes its host root",
        ),
        (
            "import 'first'; import 'last';",
            "module last was not supplied by the host",
        ),
    ] {
        let modules = HashMap::from([("test/root.mjs".into(), compile(source))]);
        assert_eq!(
            Vm::check_requested_modules_loaded("test/root.mjs", &modules, None),
            Err(RuntimeError::ModuleResolution(expected.into()))
        );
    }
}
#[cfg_attr(test, test)]
fn module_walk_storage_loading_checks_deferred_requests_and_skips_unreachable_records() {
    let mut modules = HashMap::from([
        ("root".into(), compile("export const value=1;")),
        ("unreachable".into(), compile("import '../outside';")),
    ]);
    assert_eq!(
        Vm::check_requested_modules_loaded("root", &modules, None),
        Ok(())
    );
    modules.insert(
        "root".into(),
        compile("import defer * as ns from 'missing';"),
    );
    assert_eq!(
        Vm::check_requested_modules_loaded("root", &modules, None),
        Err(RuntimeError::ModuleResolution(
            "module missing was not supplied by the host".into()
        ))
    );
}
#[cfg_attr(test, test)]
fn module_walk_storage_loading_refuses_before_pending_buffer_allocation() {
    let mut vm = page();
    let modules = chain(3);
    let _peer = leave_room(&mut vm, 0);
    #[cfg(test)]
    let result = {
        let (result, allocations) = allocation_observer::count(|| {
            Vm::check_requested_modules_loaded("n0", &modules, vm.host_scheduling.as_ref())
        });
        assert_eq!(allocations, 0);
        result
    };
    #[cfg(not(test))]
    let result = Vm::check_requested_modules_loaded("n0", &modules, vm.host_scheduling.as_ref());
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
}
#[cfg_attr(test, test)]
fn module_walk_storage_cached_import_propagates_candidate_refusal_without_panic() {
    let mut vm = page();
    let modules = HashMap::from([
        (
            "test/root.mjs".into(),
            compile("import './leaf.mjs'; export const value=1;"),
        ),
        (
            "test/leaf.mjs".into(),
            compile("import './root.mjs'; export const value=2;"),
        ),
    ]);
    vm.execute_module_graph("test/root.mjs", &modules).unwrap();
    vm.with_module_records(|vm, linked| {
        linked.get_mut("test/leaf.mjs").unwrap().error = Some(
            vm.retain_module_error(&Value::Number(7.0), ThrowOrigin::default())
                .unwrap(),
        );
    })
    .unwrap();
    let target = ModuleName::resolve(
        "test/main.js",
        "./root.mjs",
        ModuleType::JavaScript,
        vm.host_scheduling.as_ref(),
    )
    .unwrap();
    let bytes = target.bytes();
    drop(target);
    let _peer = leave_room(&mut vm, bytes);
    let heap = vm.heap.stats();
    let result = vm.dynamic_import_job(
        "test/main.js",
        "./root.mjs",
        ModuleType::JavaScript,
        ImportPhase::Evaluation,
    );
    assert_eq!(result.err(), Some(RuntimeError::SchedulingLimit));
    assert_eq!(vm.heap.stats(), heap);
    assert!(vm.linked_record("test/root.mjs").unwrap().evaluated);
}
#[cfg_attr(test, test)]
fn module_walk_storage_sync_readiness_keeps_ordinary_language_refusal() {
    let mut vm = page();
    let script =
        crate::compile(&crate::parse("var gate=Promise.withResolvers();").unwrap()).unwrap();
    vm.execute_script(&script).unwrap();
    let modules = HashMap::from([("test/leaf.mjs".into(), compile("await gate.promise;"))]);
    vm.execute_module_graph("test/leaf.mjs", &modules).unwrap();
    let result = vm.evaluate_module_sync("test/leaf.mjs");
    assert!(matches!(result, Err(RuntimeError::TypeError(_))));
    assert_eq!(vm.check_scheduling_failure(), Ok(()));
    assert_eq!(vm.displaced_execution_bytes.get(), 0);
}

impl crate::Vm {
    /// Runs finite module traversal admission and lifetime contracts.
    #[doc(hidden)]
    pub fn verify_module_walk_storage_contracts() {
        #[cfg(test)]
        {
            module_walk_storage_frontier_refusal_precedes_visited_allocation();
            module_walk_storage_cycle_root_refuses_before_reachability_scratch();
            module_walk_storage_sync_readiness_refuses_before_language_error_allocation();
        }
        module_walk_storage_identity_uses_no_scratch_at_full_quota();
        module_walk_storage_visited_names_borrow_canonical_registry_keys();
        module_walk_storage_reuses_visited_and_frames_at_full_quota();
        module_walk_storage_exact_and_short_operation_peaks_preserve_graph_and_heap();
        module_walk_storage_visited_growth_requires_the_full_hash_candidate();
        module_walk_storage_frame_growth_requires_the_full_vector_candidate();
        module_walk_storage_detached_scratch_survives_vm_destruction();
        module_walk_storage_terminal_reuse_refuses_without_mutating_scratch();
        module_walk_storage_reachability_ignores_deferred_edges_and_keeps_missing_goal_semantics();
        module_walk_storage_cycle_and_diamond_frontiers_keep_source_order_and_unique_members();
        module_walk_storage_sync_cycles_and_missing_records_remain_ready();
        module_walk_storage_scc_stops_at_an_unsettled_eager_cycle_only();
        module_walk_storage_scc_candidate_storage_is_admitted_before_allocation();
        module_walk_storage_scc_candidates_and_reachability_reuse_complete_backing();
        module_walk_storage_cycle_errors_keep_lexical_candidate_identity();
        module_walk_storage_loading_keeps_resolution_and_reverse_pending_error_order();
        module_walk_storage_loading_checks_deferred_requests_and_skips_unreachable_records();
        module_walk_storage_loading_refuses_before_pending_buffer_allocation();
        module_walk_storage_cached_import_propagates_candidate_refusal_without_panic();
        module_walk_storage_sync_readiness_keeps_ordinary_language_refusal();
    }
}
