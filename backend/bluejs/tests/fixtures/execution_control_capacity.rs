// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Finite execution frames exercise native control allocation ownership.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn compile(source: &str) -> Bytecode {
    crate::compile(&crate::parse(source).unwrap()).unwrap()
}

fn bounded_vm() -> Vm {
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

fn grow<T>(values: &mut Vec<T>) -> usize {
    let old = values.capacity();
    values.reserve(old + 7);
    (values.capacity() - old) * std::mem::size_of::<T>()
}

fn grow_map<K: Eq + std::hash::Hash, V, S: std::hash::BuildHasher>(
    values: &mut HashMap<K, V, S>,
) -> usize {
    let old = values.capacity();
    values.reserve(old + 7);
    (values.capacity() - old) * std::mem::size_of::<(K, V)>()
}

#[cfg_attr(test, test)]
fn active_control_capacity_tracks_independent_container_allocations() {
    for part in 0..18 {
        let mut vm = bounded_vm();
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        let delta = match part {
            0 => grow(&mut vm.stack),
            1 => grow(&mut vm.bindings),
            // The binding table is shared with the code and charged at its exact length, so
            // what a frame pays for is the entries it holds, not spare capacity.
            2 => {
                std::sync::Arc::make_mut(&mut vm.binding_metadata).push(Binding {
                    name: "x".into(),
                    mutable: true,
                    strict_immutable: false,
                    lexical: false,
                    catch_parameter: false,
                    eval_var: false,
                });
                std::mem::size_of::<Binding>() + 1
            }
            3 => grow(&mut vm.arguments),
            4 => grow(&mut vm.with_objects),
            5 => grow(&mut vm.active_scopes),
            6 => grow(&mut vm.active_scope_slots),
            7 => grow(&mut vm.pending_completions),
            8 => grow(&mut vm.completion_saves),
            9 => grow_map(&mut vm.cells),
            10 => grow_map(&mut vm.dynamic_eval_bindings),
            11 => grow_map(&mut vm.eval_dynamic_slots),
            12 => grow(&mut vm.dynamic_eval_outer_bindings),
            13 => grow_map(&mut vm.script_global_slots),
            14 => grow(&mut vm.variable_scope_lexicals),
            15 => grow_map(&mut vm.templates),
            16 => grow(&mut vm.call_stack),
            _ => {
                vm.pending_tail_call = Some(Vec::new());
                grow(vm.pending_tail_call.as_mut().unwrap())
            }
        };
        assert!(delta > 0);
        assert_eq!(
            vm.host_scheduling_usage().unwrap().payload_bytes - before,
            delta,
            "part={part}"
        );
    }
}

#[cfg_attr(test, test)]
fn active_control_capacity_tracks_nested_buffers_and_retained_names() {
    for part in 0..5 {
        let mut vm = bounded_vm();
        match part {
            0 => vm.active_scope_slots.push(Vec::new()),
            1 => vm.variable_scope_lexicals.push(String::new()),
            2 => {
                vm.eval_dynamic_slots.insert(0, String::new());
            }
            3 => {
                vm.dynamic_eval_bindings.insert(
                    String::new(),
                    DynamicEvalBinding {
                        cell: vm.object_prototype,
                        shadowed_cells: Vec::new(),
                    },
                );
            }
            _ => {
                vm.active_module_name = Some(String::new());
            }
        }
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        let delta = match part {
            0 => grow(&mut vm.active_scope_slots[0]),
            1 => {
                vm.variable_scope_lexicals[0].reserve(31);
                vm.variable_scope_lexicals[0].capacity()
            }
            2 => {
                let name = vm.eval_dynamic_slots.get_mut(&0).unwrap();
                name.reserve(31);
                name.capacity()
            }
            3 => grow(&mut vm.dynamic_eval_bindings.get_mut("").unwrap().shadowed_cells),
            _ => {
                let name = vm.active_module_name.as_mut().unwrap();
                name.reserve(31);
                name.capacity()
            }
        };
        assert_eq!(
            vm.host_scheduling_usage().unwrap().payload_bytes - before,
            delta,
            "part={part}"
        );
    }
}

#[cfg_attr(test, test)]
fn active_control_capacity_refuses_before_script_callbacks() {
    let mut vm = bounded_vm();
    let called = Rc::new(Cell::new(false));
    let seen = called.clone();
    vm.install_host_function("observe", 0, move |_args: &[HostValue]| {
        seen.set(true);
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let _other_owner = leave_room(&mut vm, 0);
    let result = vm.execute_script(&compile("let value=1; observe(); value;"));
    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
    assert!(!called.get());
}

#[cfg_attr(test, test)]
fn displaced_control_capacity_follows_call_eval_and_generator_frames() {
    for source in [
        "function parent(a,b,c){ function child(){observe();} child(); } parent(); 0;",
        "function parent(a,b,c){ eval('observe();'); } parent(); 0;",
        "function parent(a,b,c){ function* child(x=observe()){} child(); } parent(); 0;",
        "function* child(){observe(); yield 1;} var g=child(); function parent(a,b,c){g.next();} parent(); 0;",
        "async function child(){observe(); await 0;} function parent(a,b,c){child();} parent(); 0;",
    ] {
        let mut vm = bounded_vm();
        let counter = vm.displaced_execution_control_bytes.clone();
        let observed = Rc::new(RefCell::new(Vec::new()));
        let output = observed.clone();
        let seen = counter.clone();
        vm.install_host_function("observe", 0, move |_args: &[HostValue]| {
            output.borrow_mut().push(seen.get()); Ok(HostValue::Undefined)
        }).unwrap();
        assert_eq!(vm.execute_script(&compile(source)), Ok(Value::Number(0.0)));
        assert!(observed.borrow()[0] >= 3 * std::mem::size_of::<Option<Value>>(), "{source}: {:?}", observed.borrow());
        for _ in 0..8 {
            if !vm.run_next_promise_job().unwrap() { break; }
        }
        assert_eq!(counter.get(), 0, "{source}");
    }
}

#[cfg_attr(test, test)]
fn active_control_capacity_releases_finished_root_allocations() {
    let mut vm = bounded_vm();
    let program = compile("function f(a,b,c){try{return a+b+c;}finally{}} f(1,2,3); 0;");
    vm.execute_script(&program).unwrap();
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    assert_eq!(vm.stack.capacity(), 0);
    assert_eq!(vm.bindings.capacity(), 0);
    assert_eq!(vm.binding_metadata.capacity(), 0);
    assert_eq!(vm.active_scopes.capacity(), 0);
    assert_eq!(vm.active_scope_slots.capacity(), 0);
    assert_eq!(vm.call_stack.capacity(), 0);
    // The realm's function binding outlives these released script containers.
    assert!(vm.global_bindings.contains_key("f"));
    let persistent = vm.global_binding_storage_bytes();
    assert!(persistent > 0);
    // What remains is what the program itself retains (its function and
    // bindings). A rerun of the same program, which reuses that code, must
    // leave the account unchanged: no control container charge outlives a run.
    // One warm-up rerun first: collector and root-table capacity grow once.
    vm.execute_script(&program).unwrap();
    vm.collect_realm_garbage().unwrap();
    let first = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.execute_script(&program).unwrap();
    vm.collect_realm_garbage().unwrap();
    assert_eq!(vm.host_scheduling_usage().unwrap().payload_bytes, first);
}

impl crate::Vm {
    /// Runs active execution-control contracts in unit and coverage builds.
    #[doc(hidden)]
    pub fn verify_execution_control_capacity_contracts() {
        active_control_capacity_tracks_independent_container_allocations();
        active_control_capacity_tracks_nested_buffers_and_retained_names();
        active_control_capacity_refuses_before_script_callbacks();
        displaced_control_capacity_follows_call_eval_and_generator_frames();
        active_control_capacity_releases_finished_root_allocations();
        active_control_capacity_moves_suspended_owners_without_duplicate_charges();
        active_control_capacity_async_stack_splits_keep_only_the_saved_allocation();
        active_control_capacity_preserves_forwarded_eval_owners_without_backups();
        active_control_capacity_releases_abort_paths();
        active_control_capacity_preserves_template_identity_between_jobs();
        active_control_capacity_debugger_frames_preserve_owners_and_release_on_resume();
    }
}

#[cfg_attr(test, test)]
fn active_control_capacity_moves_suspended_owners_without_duplicate_charges() {
    for bounded in [false, true] {
        let mut vm = if bounded { bounded_vm() } else { Vm::default() };
        vm.stack.reserve(7);
        vm.bindings.reserve(7);
        vm.binding_metadata = compile("let retainedName=1;").bindings;
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        let mut execution = vm.suspend_module_execution();
        assert_eq!(vm.host_scheduling_usage().unwrap().payload_bytes, before);
        let capacity = execution.stack.capacity();
        execution.stack.reserve(capacity + 7);
        let delta = (execution.stack.capacity() - capacity) * std::mem::size_of::<Value>();
        execution.refresh_execution_charges();
        assert_eq!(
            vm.host_scheduling_usage().unwrap().payload_bytes,
            before + if bounded { delta } else { 0 }
        );
        vm.restore_module_execution(execution);
        assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
        assert_eq!(
            vm.host_scheduling_usage().unwrap().payload_bytes,
            before + if bounded { delta } else { 0 }
        );
    }
}

#[cfg_attr(test, test)]
fn active_control_capacity_async_stack_splits_keep_only_the_saved_allocation() {
    let mut vm = bounded_vm();
    vm.execute_script(&compile("var gate=Promise.withResolvers(); var result=(async()=>{await gate.promise; return 1;})(); 0;")).unwrap();
    assert_eq!(vm.async_continuations.len(), 1);
    let frame = &vm.async_continuations.values().next().unwrap().execution;
    assert_eq!(frame.control_charge.bytes(), frame.control_bytes());
    assert_eq!(
        vm.displaced_execution_control_bytes.get(),
        frame.control_bytes()
    );
    vm.execute_script(&compile("gate.resolve(); 0;")).unwrap();
    for _ in 0..8 {
        if !vm.run_next_promise_job().unwrap() {
            break;
        }
    }
    assert!(vm.async_continuations.is_empty());
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn active_control_capacity_preserves_forwarded_eval_owners_without_backups() {
    let mut vm = bounded_vm();
    let observed = Rc::new(Cell::new(usize::MAX));
    let output = observed.clone();
    let counter = vm.displaced_execution_control_bytes.clone();
    vm.install_host_function("observe", 0, move |_args: &[HostValue]| {
        output.set(counter.get());
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let callee = vm
        .execute_script(&compile("(function(){observe(); return 1;});"))
        .unwrap();
    vm.dynamic_eval_bindings.insert(
        "retainedEvalName".into(),
        DynamicEvalBinding {
            cell: vm.object_prototype,
            shadowed_cells: vec![vm.array_prototype],
        },
    );
    vm.dynamic_eval_bindings.reserve(31);
    let before = vm.dynamic_eval_bindings.capacity();
    assert_eq!(
        vm.call_native(callee, Value::Undefined, Vec::new(), false),
        Ok(Value::Number(1.0))
    );
    assert_eq!(
        observed.get(),
        0,
        "ordinary calls forward eval owners and need no recovery clone"
    );
    assert_eq!(vm.dynamic_eval_bindings.capacity(), before);
    assert!(vm.dynamic_eval_bindings.contains_key("retainedEvalName"));
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn active_control_capacity_releases_abort_paths() {
    for source in [
        "function parent(a,b,c){ function child(){observe();} child();} parent();",
        "function parent(a,b,c){eval('observe();');} parent();",
        "function* child(){observe(); yield 1;} var g=child(); function parent(a,b,c){g.next();} parent();",
    ] {
        let mut vm = bounded_vm();
        let scope = vm.host_scheduling.as_ref().unwrap().child();
        vm.install_host_function("observe", 0, move |_args: &[HostValue]| {
            scope.fail(); Ok(HostValue::Undefined)
        }).unwrap();
        assert_eq!(vm.execute_script(&compile(source)), Err(RuntimeError::SchedulingLimit));
        assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
        // Every active execution-control charge is released on abort; only
        // what the (now failed) program itself retains remains charged.
        assert_eq!(vm.current_execution_control_bytes(), 0, "{source}");
        assert_eq!(vm.active_scheduling_bytes, 0, "{source}");
        assert_eq!(vm.stack.capacity(), 0);
    }
}

#[cfg_attr(test, test)]
fn active_control_capacity_preserves_template_identity_between_jobs() {
    let mut vm = bounded_vm();
    vm.execute_script(&compile(
        "function tag(a){return a;} function get(){return tag`x`;} var first=get(); 0;",
    ))
    .unwrap();
    let capacity = vm.templates.capacity();
    assert!(capacity > 0);
    assert!(
        vm.host_scheduling_usage().unwrap().payload_bytes
            >= capacity * std::mem::size_of::<(TemplateSiteId, ObjectId)>()
    );
    assert_eq!(
        vm.execute_script(&compile("first===get();")),
        Ok(Value::Bool(true))
    );
}

#[cfg_attr(test, test)]
fn active_control_capacity_debugger_frames_preserve_owners_and_release_on_resume() {
    for module in [false, true] {
        let source = "globalThis.calls=0; function inner(){calls++; return 'saved';} globalThis.result=inner();";
        let mut registry = crate::BlueJsProgramRegistry::default();
        let program = if module {
            crate::compile_module(&crate::parse_module(source).unwrap()).unwrap()
        } else {
            compile(source)
        };
        let handle = registry
            .install_precompiled(
                crate::BlueJsSourceIdentity::new("page:///control.mjs", "sha256:control").unwrap(),
                program,
            )
            .unwrap();
        let code = registry.get(handle).unwrap().bytecode().clone();
        let graph = HashMap::from([("page:///control.mjs".to_owned(), code.clone())]);
        let mut vm = bounded_vm();
        let state = if module {
            vm.execute_module_graph_until_nested_debugger_pause("page:///control.mjs", &graph, 1, 0)
        } else {
            vm.execute_script_until_nested_debugger_pause(&code, 1, 0)
        }
        .unwrap();
        let VmDebuggerNestedExecutionState::Paused { frame_serial, .. } = state else {
            panic!("nested frame must pause")
        };
        let child = &vm.debugger_nested_continuation.as_ref().unwrap().execution;
        assert!(child.control_bytes() > 0);
        assert_eq!(child.control_charge.bytes(), child.control_bytes());
        // The paused frame's control bytes sit on its own charge, which the
        // document account carries.
        assert!(vm.host_scheduling_snapshot().payload_bytes >= child.control_bytes());
        assert!(matches!(
            vm.resume_debugger_nested_execution(frame_serial).unwrap(),
            VmDebuggerNestedExecutionState::FrameReturned { .. }
        ));
        if module {
            let root = &vm.debugger_module_continuation.as_ref().unwrap().execution;
            assert_eq!(root.payload_charge.bytes(), root.payload_bytes());
            assert_eq!(root.control_charge.bytes(), root.control_bytes());
            assert_eq!(
                vm.resume_debugger_module_execution().unwrap(),
                VmDebuggerExecutionState::Completed
            );
        } else {
            assert_eq!(
                vm.resume_debugger_execution().unwrap(),
                VmDebuggerExecutionState::Completed
            );
        }
        assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
        assert_eq!(
            vm.lookup_global_name("calls").unwrap(),
            Some(Value::Number(1.0))
        );
        assert_eq!(
            vm.lookup_global_name("result").unwrap(),
            Some(Value::String("saved".into()))
        );
    }
}
