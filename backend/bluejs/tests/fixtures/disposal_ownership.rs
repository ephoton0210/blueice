// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Finite resources exercise native admission, suspension ownership and GC.

use super::*;

fn execute(vm: &mut Vm, source: &str) -> Result<Value, RuntimeError> {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
}

fn page_vm() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}

#[cfg_attr(test, test)]
fn disposal_ownership_interleaved_generators_keep_their_own_resources() {
    let mut vm = page_vm();
    assert_eq!(execute(&mut vm, "var closed=[]; function* f(label){using r={[Symbol.dispose](){closed.push(label);}}; yield 1;} var a=f('a'),b=f('b'); a.next(); b.next(); a.return(); b.return(); closed.join(',');"), Ok(Value::String("a,b".into())));
}

#[cfg_attr(test, test)]
fn disposal_ownership_interleaved_async_frames_keep_their_own_resources() {
    let mut vm = page_vm();
    execute(&mut vm, "var closed=[]; var gates=[Promise.withResolvers(),Promise.withResolvers()]; async function f(label,i){using r={[Symbol.dispose](){closed.push(label);}}; await gates[i].promise;} var a=f('a',0),b=f('b',1); gates[0].resolve(); 0;").unwrap();
    for _ in 0..16 {
        if !vm.run_next_promise_job().unwrap() {
            break;
        }
    }
    assert_eq!(
        execute(&mut vm, "closed.join(',');"),
        Ok(Value::String("a".into()))
    );
    execute(&mut vm, "gates[1].resolve(); 0;").unwrap();
    for _ in 0..16 {
        if !vm.run_next_promise_job().unwrap() {
            break;
        }
    }
    assert_eq!(
        execute(&mut vm, "closed.join(',');"),
        Ok(Value::String("a,b".into()))
    );
}

#[cfg_attr(test, test)]
fn disposal_ownership_active_resource_and_mark_capacity_is_charged() {
    for marks in [false, true] {
        let mut vm = page_vm();
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        let delta = if marks {
            vm.dispose_marks = vm.owned_records(Vec::with_capacity(4));
            vm.dispose_marks.capacity() * std::mem::size_of::<usize>()
        } else {
            vm.disposables = vm.owned_records(Vec::with_capacity(4));
            vm.disposables.capacity() * std::mem::size_of::<DisposableResource>()
        };
        assert_eq!(
            vm.host_scheduling_usage().unwrap().payload_bytes - before,
            delta
        );
    }
}

#[cfg_attr(test, test)]
fn disposal_ownership_stack_table_capacity_is_charged() {
    for is_async in [false, true] {
        let mut vm = page_vm();
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        let table = if is_async {
            &mut vm.async_disposable_stacks
        } else {
            &mut vm.disposable_stacks
        };
        table.reserve(4);
        let delta = table.capacity() * std::mem::size_of::<(ObjectId, DisposeCapabilityState)>();
        assert_eq!(
            vm.host_scheduling_usage().unwrap().payload_bytes - before,
            delta
        );
    }
}

#[cfg_attr(test, test)]
fn disposal_ownership_unreachable_stack_resources_and_cycles_are_collectible() {
    for is_async in [false, true] {
        for cycle in [false, true] {
            let mut vm = page_vm();
            let name = if is_async {
                "AsyncDisposableStack"
            } else {
                "DisposableStack"
            };
            let source = if cycle {
                format!("var weak; (()=>{{let s=new {name}(); weak=new WeakRef(s); s.defer(()=>s);}})(); 0;")
            } else {
                format!("var weak; (()=>{{let value={{}}; weak=new WeakRef(value); let s=new {name}(); s.adopt(value,()=>{{}});}})(); 0;")
            };
            execute(&mut vm, &source).unwrap();
            vm.collect_realm_garbage().unwrap();
            assert_eq!(
                execute(&mut vm, "weak.deref()===undefined;"),
                Ok(Value::Bool(true)),
                "async={is_async}, cycle={cycle}"
            );
            assert!(vm.disposable_stacks.is_empty());
            assert!(vm.async_disposable_stacks.is_empty());
        }
    }
}

impl crate::Vm {
    /// Runs disposal ownership contracts in unit and coverage builds.
    #[doc(hidden)]
    pub fn verify_disposal_ownership_contracts() {
        disposal_ownership_interleaved_generators_keep_their_own_resources();
        disposal_ownership_interleaved_async_frames_keep_their_own_resources();
        disposal_ownership_active_resource_and_mark_capacity_is_charged();
        disposal_ownership_stack_table_capacity_is_charged();
        disposal_ownership_unreachable_stack_resources_and_cycles_are_collectible();
        disposal_ownership_stack_record_refusal_and_spare_slots_are_atomic();
        disposal_ownership_adopt_charges_only_new_payload_in_spare_slots();
        disposal_ownership_constructor_and_move_refuse_before_table_publication();
        disposal_ownership_suspended_getter_methods_survive_collection();
        disposal_ownership_live_stack_retains_and_disposal_releases_values();
        disposal_ownership_nested_calls_keep_ephemeral_dispose_methods_alive();
        disposal_ownership_vm_destruction_releases_suspended_resources();
        disposal_ownership_module_siblings_keep_their_own_resources();
        disposal_ownership_debugger_pause_roots_ephemeral_dispose_methods();
        disposal_ownership_peer_observes_collected_empty_stack_tables();
    }
}

fn leave_room(vm: &mut Vm, bytes: usize) -> host_scheduling::SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - bytes)
        .unwrap()
}

fn stack_fixture(is_async: bool, count: usize) -> (Vm, ObjectId, Value) {
    let mut vm = page_vm();
    let name = if is_async {
        "AsyncDisposableStack"
    } else {
        "DisposableStack"
    };
    execute(&mut vm, &format!("var s=new {name}(); var cb=()=>{{}}; 0;")).unwrap();
    let id = vm
        .lookup_global_name("s")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap();
    let cb = vm.lookup_global_name("cb").unwrap().unwrap();
    for _ in 0..count {
        vm.disposable_stack_defer(&Value::Object(id), cb.clone(), is_async)
            .unwrap();
    }
    (vm, id, cb)
}

#[cfg_attr(test, test)]
fn disposal_ownership_stack_record_refusal_and_spare_slots_are_atomic() {
    for is_async in [false, true] {
        for spare in [false, true] {
            let count = if spare { 3 } else { 4 };
            let (mut vm, id, cb) = stack_fixture(is_async, count);
            let room = if spare {
                0
            } else {
                4 * std::mem::size_of::<DisposableResource>() - 1
            };
            let _peer = leave_room(&mut vm, room);
            let result = vm.disposable_stack_defer(&Value::Object(id), cb, is_async);
            if spare {
                assert_eq!(result, Ok(Value::Undefined));
            } else {
                assert_eq!(result, Err(RuntimeError::SchedulingLimit));
            }
            let table = if is_async {
                &vm.async_disposable_stacks
            } else {
                &vm.disposable_stacks
            };
            assert_eq!(table[&id].resources.len(), if spare { 4 } else { count });
            assert_eq!(table[&id].resources.capacity(), 4);
            assert!(!table[&id].disposed);
        }
    }
}

#[cfg_attr(test, test)]
fn disposal_ownership_adopt_charges_only_new_payload_in_spare_slots() {
    for is_async in [false, true] {
        for accepts in [false, true] {
            let (mut vm, id, cb) = stack_fixture(is_async, 3);
            let value = Value::String("tiny".into());
            let payload = host_scheduling::value_payload_bytes(&value);
            let _peer = leave_room(&mut vm, payload - usize::from(!accepts));
            let result = vm.disposable_stack_adopt(&Value::Object(id), value.clone(), cb, is_async);
            if accepts {
                assert_eq!(result, Ok(value));
            } else {
                assert_eq!(result, Err(RuntimeError::SchedulingLimit));
            }
            let table = if is_async {
                &vm.async_disposable_stacks
            } else {
                &vm.disposable_stacks
            };
            assert_eq!(table[&id].resources.len(), if accepts { 4 } else { 3 });
            assert_eq!(table[&id].resources.capacity(), 4);
        }
    }
}

#[cfg_attr(test, test)]
fn disposal_ownership_constructor_and_move_refuse_before_table_publication() {
    for is_async in [false, true] {
        let mut vm = page_vm();
        vm.disposable_stack_prototype(is_async).unwrap();
        vm.new_target = vm
            .global(if is_async {
                "AsyncDisposableStack"
            } else {
                "DisposableStack"
            })
            .unwrap();
        vm.stack.reserve(32);
        let _peer = leave_room(&mut vm, 0);
        assert_eq!(
            vm.disposable_stack_constructor(is_async, true),
            Err(RuntimeError::SchedulingLimit)
        );
        assert!(vm.disposable_stacks.is_empty() && vm.async_disposable_stacks.is_empty());

        let (mut vm, id, _) = stack_fixture(is_async, 1);
        let name = if is_async {
            "AsyncDisposableStack"
        } else {
            "DisposableStack"
        };
        execute(
            &mut vm,
            &format!("var keep=[new {name}(),new {name}()]; 0;"),
        )
        .unwrap();
        let table = if is_async {
            &vm.async_disposable_stacks
        } else {
            &vm.disposable_stacks
        };
        assert_eq!(table.len(), table.capacity());
        vm.stack.reserve(32);
        let _peer = leave_room(&mut vm, 0);
        assert_eq!(
            vm.disposable_stack_move(&Value::Object(id), is_async),
            Err(RuntimeError::SchedulingLimit)
        );
        let table = if is_async {
            &vm.async_disposable_stacks
        } else {
            &vm.disposable_stacks
        };
        assert_eq!(table.len(), 3);
        assert!(!table[&id].disposed);
        assert_eq!(table[&id].resources.len(), 1);
    }
}

#[cfg_attr(test, test)]
fn disposal_ownership_suspended_getter_methods_survive_collection() {
    for asynchronous in [false, true] {
        let mut config = VmConfig::default();
        config.heap.nursery_capacity = 1;
        let mut vm = Vm::new(config).unwrap();
        vm.use_bounded_host_microtask_checkpoints().unwrap();
        let body="using r={get [Symbol.dispose](){let token={label};return ()=>closed.push(token.label);}};";
        if asynchronous {
            execute(&mut vm,&format!("var closed=[]; var gate=Promise.withResolvers(); async function f(label){{{body} await gate.promise;}} var result=f('kept'); 0;")).unwrap();
            vm.collect_realm_garbage().unwrap();
            execute(&mut vm, "gate.resolve(); 0;").unwrap();
            for _ in 0..16 {
                if !vm.run_next_promise_job().unwrap() {
                    break;
                }
            }
        } else {
            execute(&mut vm,&format!("var closed=[];function* f(label){{{body}yield 1;}} var g=f('kept'); g.next(); 0;")).unwrap();
            vm.collect_realm_garbage().unwrap();
            execute(&mut vm, "g.return(); 0;").unwrap();
        }
        assert_eq!(
            execute(&mut vm, "closed.join(',');"),
            Ok(Value::String("kept".into()))
        );
        assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    }
}

#[cfg_attr(test, test)]
fn disposal_ownership_live_stack_retains_and_disposal_releases_values() {
    let mut vm = page_vm();
    execute(&mut vm,"var s=new DisposableStack(); var weak; (()=>{let value={};weak=new WeakRef(value);s.adopt(value,()=>{});})(); 0;").unwrap();
    vm.collect_realm_garbage().unwrap();
    assert_eq!(
        execute(&mut vm, "weak.deref()!==undefined;"),
        Ok(Value::Bool(true))
    );
    execute(&mut vm, "s.dispose(); 0;").unwrap();
    vm.collect_realm_garbage().unwrap();
    assert_eq!(
        execute(&mut vm, "weak.deref()===undefined;"),
        Ok(Value::Bool(true))
    );
}

#[cfg_attr(test, test)]
fn disposal_ownership_nested_calls_keep_ephemeral_dispose_methods_alive() {
    for invocation in [
        "child();",
        "eval('child();');",
        "(function*(x=child()){yield 1;})().next();",
    ] {
        let mut config = VmConfig::default();
        config.heap.nursery_capacity = 1;
        let mut vm = Vm::new(config).unwrap();
        vm.use_bounded_host_microtask_checkpoints().unwrap();
        let source=format!("var closed=0;function child(){{var allocations=[{{}},{{}},{{}},{{}}];}} function parent(){{using r={{get [Symbol.dispose](){{let token={{value:1}};return ()=>closed+=token.value;}}}}; {invocation}}} parent();closed;");
        assert_eq!(
            execute(&mut vm, &source),
            Ok(Value::Number(1.0)),
            "{invocation}"
        );
    }
}

#[cfg_attr(test, test)]
fn disposal_ownership_vm_destruction_releases_suspended_resources() {
    let mut vm = page_vm();
    let counter = vm.displaced_execution_control_bytes.clone();
    execute(&mut vm,"var gate=Promise.withResolvers();var result=(async()=>{using r={[Symbol.dispose](){}}; await gate.promise;})();0;").unwrap();
    assert!(counter.get() > 0);
    drop(vm);
    assert_eq!(counter.get(), 0);
}

#[cfg_attr(test, test)]
fn disposal_ownership_module_siblings_keep_their_own_resources() {
    let mut vm = page_vm();
    execute(
        &mut vm,
        "var closed=[];var gates=[Promise.withResolvers(),Promise.withResolvers()];0;",
    )
    .unwrap();
    let compile_module =
        |source: &str| crate::compile_module(&crate::parse_module(source).unwrap()).unwrap();
    let modules = HashMap::from([
        (
            "test/main.mjs".into(),
            compile_module("import './a.mjs';import './b.mjs';"),
        ),
        (
            "test/a.mjs".into(),
            compile_module(
                "using r={[Symbol.dispose](){closed.push('a');}};await gates[0].promise;",
            ),
        ),
        (
            "test/b.mjs".into(),
            compile_module(
                "using r={[Symbol.dispose](){closed.push('b');}};await gates[1].promise;",
            ),
        ),
    ]);
    vm.execute_module_graph("test/main.mjs", &modules).unwrap();
    for (index, expected) in [(0, "a"), (1, "a,b")] {
        execute(&mut vm, &format!("gates[{index}].resolve();0;")).unwrap();
        for _ in 0..16 {
            if !vm.run_next_promise_job().unwrap() {
                break;
            }
        }
        assert_eq!(
            execute(&mut vm, "closed.join(',');"),
            Ok(Value::String(expected.into()))
        );
    }
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn disposal_ownership_debugger_pause_roots_ephemeral_dispose_methods() {
    let mut config = VmConfig::default();
    config.heap.nursery_capacity = 1;
    let mut vm = Vm::new(config).unwrap();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let program=crate::compile(&crate::parse("var closed=0;{using r={get [Symbol.dispose](){let token={value:1};return ()=>closed+=token.value;}};0;} closed;").unwrap()).unwrap();
    let target = program
        .instructions()
        .find(|instruction| instruction.opcode == Opcode::DisposeResources)
        .unwrap()
        .offset as u32;
    assert!(matches!(
        vm.execute_script_until_debugger_pause(&program, target)
            .unwrap(),
        VmDebuggerExecutionState::Paused { .. }
    ));
    assert_eq!(vm.disposables.len(), 1);
    vm.collect_realm_garbage().unwrap();
    assert_eq!(
        vm.resume_debugger_execution().unwrap(),
        VmDebuggerExecutionState::Completed
    );
    assert_eq!(execute(&mut vm, "closed;"), Ok(Value::Number(1.0)));
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn disposal_ownership_peer_observes_collected_empty_stack_tables() {
    let mut vm = page_vm();
    let peer = Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    };
    execute(
        &mut vm,
        "(()=>{new DisposableStack();new AsyncDisposableStack();})();0;",
    )
    .unwrap();
    vm.collect_realm_garbage().unwrap();
    assert!(vm.disposable_stacks.is_empty() && vm.async_disposable_stacks.is_empty());
    assert_eq!(vm.disposable_stacks.capacity(), 0);
    assert_eq!(vm.async_disposable_stacks.capacity(), 0);
    assert_eq!(
        peer.host_scheduling_snapshot().payload_bytes,
        vm.host_scheduling_snapshot().payload_bytes
    );
}
