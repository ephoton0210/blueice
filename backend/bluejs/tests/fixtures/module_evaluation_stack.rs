// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny public module graphs verify explicit evaluation dependency frames.

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
fn module_evaluation_stack_public_chain_uses_one_active_dependency_call() {
    let mut vm = page();
    let modules = graph(&[
        ("t/a", "import './b';"),
        ("t/b", "import './c';"),
        ("t/c", ""),
    ]);
    let (result, depth) = CallProbe::measure(|| vm.execute_module_graph("t/a", &modules));
    result.unwrap();
    assert_eq!(depth, 1);
}
#[cfg(test)]
#[test]
fn module_evaluation_stack_public_cycle_uses_one_active_dependency_call() {
    let mut vm = page();
    let modules = graph(&[
        ("t/a", "import './b';"),
        ("t/b", "import './c';"),
        ("t/c", "import './a';"),
    ]);
    let (result, depth) = CallProbe::measure(|| vm.execute_module_graph("t/a", &modules));
    result.unwrap();
    assert_eq!(depth, 1);
}
#[cfg(test)]
#[test]
fn module_evaluation_stack_public_await_chain_uses_one_active_dependency_call() {
    let mut vm = page();
    let modules = graph(&[
        ("t/a", "import './b';"),
        ("t/b", "import './c';"),
        ("t/c", "await Promise.resolve(0);"),
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
fn record(vm: &Vm) -> LinkedModule {
    LinkedModule {
        cells: HashMap::new(),
        cells_charge: vm.charge_module_cells(0),
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
fn records(vm: &Vm, modules: &ModuleRegistry) -> ModuleRecords {
    let mut linked = ModuleRecords::default();
    for name in modules.keys() {
        linked
            .insert_new(name, record(vm), vm.host_scheduling.as_ref())
            .unwrap();
    }
    linked
}
fn script(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}

#[cfg_attr(test, test)]
fn module_evaluation_stack_frame_backing_outlives_the_vm() {
    let vm = page();
    let before = vm.host_scheduling_snapshot().payload_bytes;
    let observer = Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    };
    let mut frames = module_walk_storage::Storage::<EvaluationFrame<'_>>::default();
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
fn module_evaluation_stack_exact_frame_room_and_one_short_refusal() {
    let mut preview = module_walk_storage::Storage::<EvaluationFrame<'_>>::default();
    preview.reserve(1, None).unwrap();
    let bytes = preview.bytes();
    for accepts in [false, true] {
        let mut vm = page();
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        let peer = leave_room(&mut vm, bytes - usize::from(!accepts));
        let mut frames = module_walk_storage::Storage::<EvaluationFrame<'_>>::default();
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
fn module_evaluation_stack_spare_control_capacity_needs_no_headroom() {
    let mut vm = page();
    let mut frames = module_walk_storage::Storage::<EvaluationFrame<'_>>::default();
    frames.reserve(1, vm.host_scheduling.as_ref()).unwrap();
    let pointer = frames.data.as_ptr();
    let _peer = leave_room(&mut vm, 0);
    frames.reserve(1, vm.host_scheduling.as_ref()).unwrap();
    assert_eq!(frames.data.as_ptr(), pointer);
}

#[cfg_attr(test, test)]
fn module_evaluation_stack_reallocation_keeps_owned_request_state() {
    let vm = page();
    let modules = ModuleRegistry::new(graph(&[("a", "import 'b';"), ("b", "")]), None).unwrap();
    let mut frame = EvaluationFrame::new("a", &modules, 7);
    let mut names = ModuleNames::default();
    names.push_copy("b", vm.host_scheduling.as_ref()).unwrap();
    frame.targets = Some(names.into_iter());
    frame
        .pending
        .push_copy("p", vm.host_scheduling.as_ref())
        .unwrap();
    let mut frames = module_walk_storage::Storage::default();
    frames.reserve(1, vm.host_scheduling.as_ref()).unwrap();
    frames.data.push(frame);
    frames.reserve(5, vm.host_scheduling.as_ref()).unwrap();
    let frame = frames.data.last_mut().unwrap();
    assert_eq!(frame.epoch, 7);
    assert_eq!(
        frame.targets.as_mut().unwrap().next().unwrap().as_str(),
        "b"
    );
    assert_eq!(&frame.pending[..], ["p"]);
}

#[cfg_attr(test, test)]
fn module_evaluation_stack_child_refusal_precedes_the_evaluating_mark() {
    let mut vm = page();
    let modules = ModuleRegistry::new(
        graph(&[("a", ""), ("b", ""), ("c", ""), ("d", ""), ("e", "")]),
        None,
    )
    .unwrap();
    let mut linked = records(&vm, &modules);
    let mut frames = module_walk_storage::Storage::default();
    frames.reserve(4, vm.host_scheduling.as_ref()).unwrap();
    for name in ["a", "b", "c", "d"] {
        linked.get_mut(name).unwrap().evaluating = true;
        frames.data.push(EvaluationFrame::new(name, &modules, 0));
    }
    let target = ModuleName::copy("e", vm.host_scheduling.as_ref()).unwrap();
    let pointer = frames.data.as_ptr();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.push_module_dependency(target, &modules, &mut linked, &mut frames),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(!linked["e"].evaluating);
    assert_eq!(frames.data.len(), 4);
    assert_eq!(frames.data.as_ptr(), pointer);
}

#[cfg_attr(test, test)]
fn module_evaluation_stack_entry_refusal_leaves_records_inactive() {
    let mut vm = page();
    let modules = ModuleRegistry::new(graph(&[("a", "")]), None).unwrap();
    let mut linked = records(&vm, &modules);
    let heap = vm.heap.stats();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.evaluate_module_record("a", &modules, &mut linked),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(!linked["a"].evaluating && !linked["a"].evaluated);
    assert_eq!(vm.heap.stats(), heap);
}

#[cfg_attr(test, test)]
fn module_evaluation_stack_cached_completed_and_busy_records_need_no_frame_room() {
    for busy in [false, true] {
        let mut vm = page();
        let modules = ModuleRegistry::new(graph(&[("a", "")]), None).unwrap();
        let mut linked = records(&vm, &modules);
        linked.get_mut("a").unwrap().evaluating = busy;
        linked.get_mut("a").unwrap().evaluated = !busy;
        let _peer = leave_room(&mut vm, 0);
        assert_eq!(
            vm.evaluate_module_record("a", &modules, &mut linked),
            Ok(Value::Undefined)
        );
        assert_eq!(linked["a"].evaluating, busy);
    }
}

#[cfg_attr(test, test)]
fn module_evaluation_stack_diamond_keeps_dfs_order_and_runs_shared_code_once() {
    let mut vm = page();
    script(&mut vm, "var log=[];");
    let modules = graph(&[
        ("a", "import 'b';import 'c';log.push('a');log.join();"),
        ("b", "import 'd';log.push('b');"),
        ("c", "import 'd';log.push('c');"),
        ("d", "log.push('d');"),
    ]);
    assert_eq!(
        vm.execute_module_graph("a", &modules),
        Ok(Value::String("d,b,c,a".into()))
    );
}

#[cfg_attr(test, test)]
fn module_evaluation_stack_cycles_finish_in_dfs_order() {
    let mut vm = page();
    script(&mut vm, "var log=[];");
    let modules = graph(&[
        ("a", "import 'b';log.push('a');log.join();"),
        ("b", "import 'c';log.push('b');"),
        ("c", "import 'a';log.push('c');"),
    ]);
    assert_eq!(
        vm.execute_module_graph("a", &modules),
        Ok(Value::String("c,b,a".into()))
    );
    assert!(vm
        .module_graph
        .as_ref()
        .unwrap()
        .linked
        .values()
        .all(|r| r.evaluated && !r.evaluating));
}

#[cfg_attr(test, test)]
fn module_evaluation_stack_awaited_siblings_keep_fresh_fuel_and_parent_order() {
    let mut vm = page();
    script(&mut vm, "var log=[];");
    let modules = graph(&[
        ("root", "import 'parent';log.push('root');log.join();"),
        ("parent", "import 'a';import 'b';log.push('parent');"),
        ("a", "await null;log.push('a');"),
        ("b", "await null;await null;log.push('b');"),
    ]);
    assert_eq!(
        vm.execute_module_graph("root", &modules),
        Ok(Value::String("a,b,parent,root".into()))
    );
    assert!(vm.module_pending_dependencies.is_empty());
    assert!(vm.module_async_parents.is_empty());
}

#[cfg_attr(test, test)]
fn module_evaluation_stack_deferred_requests_only_evaluate_async_dependencies() {
    let mut vm = page();
    script(&mut vm, "var log=[];");
    let modules = graph(&[
        (
            "root",
            "import defer * as ns from 'branch';log.push('root');log.join();",
        ),
        ("branch", "import 'async';import 'cold';log.push('branch');"),
        ("async", "await null;log.push('async');"),
        ("cold", "log.push('cold');"),
    ]);
    assert_eq!(
        vm.execute_module_graph("root", &modules),
        Ok(Value::String("async,root".into()))
    );
}

#[cfg_attr(test, test)]
fn module_evaluation_stack_deferred_property_reentry_restores_parent_context() {
    let mut vm = page();
    script(&mut vm, "var log=[];");
    let modules=graph(&[("root","import defer * as ns from 'dep'; const own=import.meta;log.push('before');log.push(ns.x);log.push(own===import.meta);log.push('after');log.join();"),("dep","log.push('dep');export const x=7;")]);
    assert_eq!(
        vm.execute_module_graph("root", &modules),
        Ok(Value::String("before,dep,7,true,after".into()))
    );
}

#[cfg_attr(test, test)]
fn module_evaluation_stack_language_error_identity_is_shared_and_replayed() {
    let mut vm = page();
    let modules = graph(&[
        ("a", "import 'b';"),
        ("b", "import 'c';"),
        ("c", "throw {marker:7};"),
    ]);
    let Err(RuntimeError::Thrown(value)) = vm.execute_module_graph("a", &modules) else {
        panic!("the leaf throws")
    };
    let id = value.object_id().unwrap();
    let graph = vm.module_graph.as_ref().unwrap();
    for name in ["a", "b", "c"] {
        let r = &graph.linked[name];
        assert!(r.evaluated && !r.evaluating);
        assert_eq!(r.error.as_ref().unwrap().value.object_id(), Some(id));
    }
    assert_eq!(
        vm.execute_module_graph("a", &modules),
        Err(RuntimeError::Thrown(Value::Object(id)))
    );
}

#[cfg_attr(test, test)]
fn module_evaluation_stack_error_after_control_growth_unwinds_every_frame() {
    let mut vm = page();
    let modules = graph(&[
        ("a", "import 'b';"),
        ("b", "import 'c';"),
        ("c", "import 'd';"),
        ("d", "import 'e';"),
        ("e", "import 'f';"),
        ("f", "throw 7;"),
    ]);
    assert_eq!(
        vm.execute_module_graph("a", &modules),
        Err(RuntimeError::Thrown(Value::Number(7.0)))
    );
    assert!(vm
        .module_graph
        .as_ref()
        .unwrap()
        .linked
        .values()
        .all(|r| r.evaluated && !r.evaluating && r.error.is_some()));
}

#[cfg_attr(test, test)]
fn module_evaluation_stack_noncatchable_instruction_abort_clears_active_frames() {
    let mut vm = page();
    let modules = ModuleRegistry::new(
        graph(&[("a", "import 'b';"), ("b", "import 'c';"), ("c", "1+1;")]),
        None,
    )
    .unwrap();
    let mut linked = records(&vm, &modules);
    vm.config.instruction_budget = 0;
    assert_eq!(
        vm.evaluate_module_record("a", &modules, &mut linked),
        Err(RuntimeError::InstructionLimit)
    );
    assert!(linked
        .values()
        .all(|r| !r.evaluating && !r.evaluated && r.error.is_none()));
}

#[cfg_attr(test, test)]
fn module_evaluation_stack_error_retention_refusal_still_clears_the_active_mark() {
    let mut vm = page();
    let modules = ModuleRegistry::new(graph(&[("a", "")]), None).unwrap();
    let mut linked = records(&vm, &modules);
    linked.get_mut("a").unwrap().evaluating = true;
    let epoch = vm.throw_epoch;
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.finish_module_evaluation(
            "a",
            epoch,
            Err(RuntimeError::Thrown(Value::Null)),
            &mut linked
        ),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(!linked["a"].evaluating && !linked["a"].evaluated && linked["a"].error.is_none());
}

#[cfg_attr(test, test)]
fn module_evaluation_stack_error_object_allocation_refusal_clears_the_active_mark() {
    let mut vm = page();
    let modules = ModuleRegistry::new(graph(&[("a", "")]), None).unwrap();
    let mut linked = records(&vm, &modules);
    linked.get_mut("a").unwrap().evaluating = true;
    vm.heap.allow_only(0);
    let epoch = vm.throw_epoch;
    assert!(matches!(
        vm.finish_module_evaluation(
            "a",
            epoch,
            Err(RuntimeError::RangeError("small".into())),
            &mut linked
        ),
        Err(RuntimeError::Heap(_))
    ));
    assert!(!linked["a"].evaluating && !linked["a"].evaluated);
}

#[cfg_attr(test, test)]
fn module_evaluation_stack_suspended_mark_survives_completion_processing() {
    let mut vm = page();
    let modules = ModuleRegistry::new(graph(&[("a", "")]), None).unwrap();
    let mut linked = records(&vm, &modules);
    {
        let r = linked.get_mut("a").unwrap();
        r.evaluating = true;
        r.suspended = true;
    }
    assert_eq!(
        vm.finish_module_evaluation("a", vm.throw_epoch, Ok(Value::Undefined), &mut linked),
        Ok(Value::Undefined)
    );
    assert!(linked["a"].evaluating && linked["a"].suspended && !linked["a"].evaluated);
}

#[cfg_attr(test, test)]
fn module_evaluation_stack_gc_keeps_awaited_module_state_alive() {
    let mut vm = page();
    script(&mut vm, "var gate=Promise.withResolvers();var reached=0;");
    let modules = graph(&[
        ("a", "import {x} from 'b';reached=x;"),
        ("b", "await gate.promise;export const x=42;"),
    ]);
    vm.execute_module_graph("a", &modules).unwrap();
    vm.collect_realm_garbage().unwrap();
    assert_eq!(script(&mut vm, "reached"), Value::Number(0.0));
    script(&mut vm, "gate.resolve();");
    vm.run_promise_jobs_bounded(8).unwrap();
    assert_eq!(script(&mut vm, "reached"), Value::Number(42.0));
}

#[cfg_attr(test, test)]
fn module_evaluation_stack_public_record_flags_complete_after_success() {
    let mut vm = page();
    let modules = graph(&[
        ("a", "import 'b';export const x=1;"),
        ("b", "export const y=2;"),
    ]);
    vm.execute_module_graph("a", &modules).unwrap();
    assert!(vm
        .module_graph
        .as_ref()
        .unwrap()
        .linked
        .values()
        .all(|r| r.evaluated && !r.evaluating && !r.suspended && r.completion.is_some()));
}

#[cfg_attr(test, test)]
fn module_evaluation_stack_host_neutral_order_remains_unchanged() {
    let mut vm = Vm::default();
    script(&mut vm, "var log=[];");
    let modules = graph(&[
        ("a", "import 'b';log.push('a');log.join();"),
        ("b", "log.push('b');"),
    ]);
    assert_eq!(
        vm.execute_module_graph("a", &modules),
        Ok(Value::String("b,a".into()))
    );
    assert!(vm.host_scheduling.is_none());
}

impl crate::Vm {
    /// Runs finite module dependency evaluation and frame ownership contracts.
    #[doc(hidden)]
    pub fn verify_module_evaluation_stack_contracts() {
        module_evaluation_stack_frame_backing_outlives_the_vm();
        module_evaluation_stack_exact_frame_room_and_one_short_refusal();
        module_evaluation_stack_spare_control_capacity_needs_no_headroom();
        module_evaluation_stack_reallocation_keeps_owned_request_state();
        module_evaluation_stack_child_refusal_precedes_the_evaluating_mark();
        module_evaluation_stack_entry_refusal_leaves_records_inactive();
        module_evaluation_stack_cached_completed_and_busy_records_need_no_frame_room();
        module_evaluation_stack_diamond_keeps_dfs_order_and_runs_shared_code_once();
        module_evaluation_stack_cycles_finish_in_dfs_order();
        module_evaluation_stack_awaited_siblings_keep_fresh_fuel_and_parent_order();
        module_evaluation_stack_deferred_requests_only_evaluate_async_dependencies();
        module_evaluation_stack_deferred_property_reentry_restores_parent_context();
        module_evaluation_stack_language_error_identity_is_shared_and_replayed();
        module_evaluation_stack_error_after_control_growth_unwinds_every_frame();
        module_evaluation_stack_noncatchable_instruction_abort_clears_active_frames();
        module_evaluation_stack_error_retention_refusal_still_clears_the_active_mark();
        module_evaluation_stack_error_object_allocation_refusal_clears_the_active_mark();
        module_evaluation_stack_suspended_mark_survives_completion_processing();
        module_evaluation_stack_gc_keeps_awaited_module_state_alive();
        module_evaluation_stack_public_record_flags_complete_after_success();
        module_evaluation_stack_host_neutral_order_remains_unchanged();
    }
}
