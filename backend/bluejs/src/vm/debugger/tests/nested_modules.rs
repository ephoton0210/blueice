// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn nested_module_pause_retains_its_entry_graph_after_dependency_evaluation() {
    let entry = "pages/entry.mjs";
    let dependency = "pages/dep.mjs";
    let mut registry = BlueJsProgramRegistry::default();
    let dependency_handle = registry
        .install_precompiled(
            BlueJsSourceIdentity::new(dependency, "sha256:dependency").unwrap(),
            module_code("globalThis.dependencyRuns = (globalThis.dependencyRuns || 0) + 1; export const seed = 41;"),
        )
        .unwrap();
    let entry_handle = registry
        .install_precompiled(
            BlueJsSourceIdentity::new(entry, "sha256:entry").unwrap(),
            module_code("import { seed } from './dep.mjs'; globalThis.entryCalls = 0; function inner(){globalThis.entryCalls++; return seed + 1;} export const answer = inner();"),
        )
        .unwrap();
    let graph = HashMap::from([
        (
            dependency.to_string(),
            registry.get(dependency_handle).unwrap().bytecode().clone(),
        ),
        (
            entry.to_string(),
            registry.get(entry_handle).unwrap().bytecode().clone(),
        ),
    ]);
    let mut vm = Vm::default();
    assert_eq!(
        vm.execute_module_graph_until_nested_debugger_pause(entry, &graph, 1, 0)
            .unwrap(),
        VmDebuggerNestedExecutionState::Paused {
            frame_serial: 1,
            code_unit_ordinal: 1,
            bytecode_offset: 0,
        }
    );
    assert_eq!(
        vm.lookup_global_name("dependencyRuns").unwrap(),
        Some(Value::Number(1.0))
    );
    assert_eq!(
        vm.lookup_global_name("entryCalls").unwrap(),
        Some(Value::Number(0.0))
    );
    assert!(vm.module_graph.is_some());
    assert!(vm.linked_record(dependency).unwrap().evaluated);
    let parent = vm.debugger_module_continuation.as_ref().unwrap();
    assert_eq!(parent.module, entry);
    assert_eq!(
        parent.code.instruction(parent.pc).unwrap().opcode,
        Opcode::Call
    );
    assert!(vm.debugger_nested_continuation.is_some());
    assert!(matches!(
        vm.execute_script(&code("1 + 1")),
        Err(RuntimeError::Unsupported(_))
    ));
    let safe_offsets = graph[entry]
        .child_code_units()
        .next()
        .unwrap()
        .instructions()
        .map(|instruction| instruction.offset as u32)
        .collect::<std::collections::HashSet<_>>();
    let mut returned = None;
    for _ in 0..128 {
        match vm.step_debugger_nested_instruction(1).unwrap() {
            VmDebuggerNestedExecutionState::Paused {
                frame_serial: 1,
                code_unit_ordinal: 1,
                bytecode_offset,
            } => assert!(safe_offsets.contains(&bytecode_offset)),
            VmDebuggerNestedExecutionState::FrameReturned {
                root_bytecode_offset,
            } => {
                returned = Some(root_bytecode_offset);
                break;
            }
            other => panic!("unexpected module child step: {other:?}"),
        }
    }
    assert_eq!(
        vm.debugger_module_continuation.as_ref().unwrap().pc,
        returned.expect("module child must return within its step budget") as usize
    );
    assert!(vm.debugger_nested_continuation.is_none());
    assert!(vm.module_graph.is_some());
    assert_eq!(
        vm.resume_debugger_module_execution().unwrap(),
        VmDebuggerExecutionState::Completed
    );
    assert!(vm.linked_record(entry).unwrap().evaluated);
    assert_eq!(
        vm.lookup_global_name("dependencyRuns").unwrap(),
        Some(Value::Number(1.0))
    );
    assert_eq!(
        vm.lookup_global_name("entryCalls").unwrap(),
        Some(Value::Number(1.0))
    );
}

#[test]
fn linked_module_nested_pause_preserves_each_frame_generation() {
    let entry = "pages/linked-entry.mjs";
    let dependency = "pages/linked-dep.mjs";
    let mut registry = BlueJsProgramRegistry::default();
    let dependency_handle = registry
        .install_precompiled(
            BlueJsSourceIdentity::new(dependency, "sha256:linked-dependency").unwrap(),
            module_code("export function inner(){ return 41; } globalThis.depCall = inner();"),
        )
        .unwrap();
    let entry_handle = registry
        .install_precompiled(
            BlueJsSourceIdentity::new(entry, "sha256:linked-entry").unwrap(),
            module_code(
                "import { inner } from './linked-dep.mjs'; export const answer = inner() + 1;",
            ),
        )
        .unwrap();
    let graph = HashMap::from([
        (
            dependency.to_string(),
            registry.get(dependency_handle).unwrap().bytecode().clone(),
        ),
        (
            entry.to_string(),
            registry.get(entry_handle).unwrap().bytecode().clone(),
        ),
    ]);
    let dependency_generation = dependency_handle.generation().as_u64();
    let entry_generation = entry_handle.generation().as_u64();
    let mut vm = Vm::default();
    assert_eq!(
        vm.execute_module_graph_until_linked_nested_debugger_pause(
            entry,
            dependency,
            &graph,
            VmDebuggerLinkedPauseTarget {
                entry_generation,
                dependency_generation,
                code_unit_ordinal: 1,
                bytecode_offset: 0,
            },
        ),
        Ok(VmDebuggerNestedExecutionState::Paused {
            frame_serial: 1,
            code_unit_ordinal: 1,
            bytecode_offset: 0,
        })
    );
    assert_eq!(
        vm.lookup_global_name("depCall").unwrap(),
        Some(Value::Number(41.0))
    );
    assert!(vm.debugger_stack_snapshot(Some(1), 2, 256).is_err());
    assert!(vm.debugger_linked_stack_snapshot(2, 2, 256).is_err());
    let stack = vm.debugger_linked_stack_snapshot(1, 2, 256).unwrap();
    assert_eq!(stack.program_generation, entry_generation);
    assert_eq!(stack.frames.len(), 2);
    assert_eq!(stack.frames[0].program_generation, dependency_generation);
    assert_eq!(stack.frames[0].code_unit_ordinal, 1);
    assert_eq!(stack.frames[1].program_generation, entry_generation);
    assert_eq!(stack.frames[1].code_unit_ordinal, 0);
    assert!(!stack.stack_truncated);
    assert!(matches!(
        vm.resume_debugger_nested_execution(1),
        Ok(VmDebuggerNestedExecutionState::FrameReturned { .. })
    ));
    assert_eq!(
        vm.resume_debugger_module_execution(),
        Ok(VmDebuggerExecutionState::Completed)
    );
    assert!(vm.debugger_linked_stack_snapshot(1, 2, 256).is_err());
}

#[test]
fn linked_module_nested_pause_rejects_stale_or_unreachable_programs() {
    let entry = "pages/linked-entry.mjs";
    let dependency = "pages/linked-dep.mjs";
    let unrelated = "pages/unrelated.mjs";
    let mut registry = BlueJsProgramRegistry::default();
    let dependency_handle = registry
        .install_precompiled(
            BlueJsSourceIdentity::new(dependency, "sha256:linked-dependency").unwrap(),
            module_code("export function inner(){ return 41; }"),
        )
        .unwrap();
    let entry_handle = registry
        .install_precompiled(
            BlueJsSourceIdentity::new(entry, "sha256:linked-entry").unwrap(),
            module_code(
                "import { inner } from './linked-dep.mjs'; export const answer = inner() + 1;",
            ),
        )
        .unwrap();
    let unrelated_handle = registry
        .install_precompiled(
            BlueJsSourceIdentity::new(unrelated, "sha256:unrelated").unwrap(),
            module_code("export function inner(){ return 99; }"),
        )
        .unwrap();
    let graph = HashMap::from([
        (
            dependency.to_string(),
            registry.get(dependency_handle).unwrap().bytecode().clone(),
        ),
        (
            entry.to_string(),
            registry.get(entry_handle).unwrap().bytecode().clone(),
        ),
        (
            unrelated.to_string(),
            registry.get(unrelated_handle).unwrap().bytecode().clone(),
        ),
    ]);
    let dependency_generation = dependency_handle.generation().as_u64();
    let entry_generation = entry_handle.generation().as_u64();
    let mut vm = Vm::default();
    for (target, expected_entry, expected_dependency) in [
        (dependency, entry_generation + 1, dependency_generation),
        (dependency, entry_generation, dependency_generation + 1),
        (
            unrelated,
            entry_generation,
            unrelated_handle.generation().as_u64(),
        ),
    ] {
        assert!(matches!(
            vm.execute_module_graph_until_linked_nested_debugger_pause(
                entry,
                target,
                &graph,
                VmDebuggerLinkedPauseTarget {
                    entry_generation: expected_entry,
                    dependency_generation: expected_dependency,
                    code_unit_ordinal: 1,
                    bytecode_offset: 0,
                },
            ),
            Err(RuntimeError::Unsupported(_))
        ));
        assert!(vm.debugger_linked_stack_snapshot(1, 2, 256).is_err());
    }
    assert!(matches!(
        vm.execute_module_graph_until_linked_nested_debugger_pause(
            entry,
            dependency,
            &graph,
            VmDebuggerLinkedPauseTarget {
                entry_generation,
                dependency_generation,
                code_unit_ordinal: 1,
                bytecode_offset: 0,
            },
        ),
        Ok(VmDebuggerNestedExecutionState::Paused { .. })
    ));
}

#[test]
fn unhandled_nested_module_throw_releases_both_debugger_frames() {
    let entry = "pages/throwing.mjs";
    let mut registry = BlueJsProgramRegistry::default();
    let handle = registry
        .install_precompiled(
            BlueJsSourceIdentity::new(entry, "sha256:throwing").unwrap(),
            module_code("function inner(){throw 7;} export const answer = inner();"),
        )
        .unwrap();
    let graph = HashMap::from([(
        entry.to_string(),
        registry.get(handle).unwrap().bytecode().clone(),
    )]);
    let mut vm = Vm::default();
    let VmDebuggerNestedExecutionState::Paused { frame_serial, .. } = vm
        .execute_module_graph_until_nested_debugger_pause(entry, &graph, 1, 0)
        .unwrap()
    else {
        panic!("the throwing child must pause before its body");
    };
    let mut thrown = None;
    for _ in 0..32 {
        match vm.step_debugger_nested_instruction(frame_serial) {
            Ok(VmDebuggerNestedExecutionState::Paused { .. }) => {}
            Err(error) => {
                thrown = Some(error);
                break;
            }
            other => panic!("unexpected nested module throw result: {other:?}"),
        }
    }
    assert_eq!(thrown, Some(RuntimeError::Thrown(Value::Number(7.0))));
    assert_eq!(
        vm.debugger_uncaught_throw_site(),
        Some(last_throw_site(
            graph[entry].child_code_units().next().unwrap()
        ))
    );
    assert!(vm.debugger_module_continuation.is_none());
    assert!(vm.debugger_nested_continuation.is_none());
    let record = vm.linked_record(entry).unwrap();
    assert!(!record.suspended);
    assert_eq!(record.error, Some(Value::Number(7.0)));
    assert_eq!(
        vm.execute_script(&code("1 + 1")).unwrap(),
        Value::Number(2.0)
    );
}

#[test]
fn nested_module_throw_resumes_its_original_catch_and_finally() {
    let entry = "pages/catching.mjs";
    let mut registry = BlueJsProgramRegistry::default();
    let handle = registry
        .install_precompiled(
            BlueJsSourceIdentity::new(entry, "sha256:catching").unwrap(),
            module_code("function inner(){throw 7;} globalThis.caught = 0; globalThis.finallyRuns = 0; try { inner(); } catch (error) { globalThis.caught = error; } finally { globalThis.finallyRuns++; } export const answer = globalThis.caught;"),
        )
        .unwrap();
    let graph = HashMap::from([(
        entry.to_string(),
        registry.get(handle).unwrap().bytecode().clone(),
    )]);
    let mut vm = Vm::default();
    let VmDebuggerNestedExecutionState::Paused { frame_serial, .. } = vm
        .execute_module_graph_until_nested_debugger_pause(entry, &graph, 1, 0)
        .unwrap()
    else {
        panic!("the throwing module child must pause before execution");
    };
    let mut returned = false;
    for _ in 0..32 {
        match vm.step_debugger_nested_instruction(frame_serial).unwrap() {
            VmDebuggerNestedExecutionState::Paused { .. } => {}
            VmDebuggerNestedExecutionState::FrameReturned { .. } => {
                returned = true;
                break;
            }
            other => panic!("unexpected module handler state: {other:?}"),
        }
    }
    assert!(returned, "the throw must rejoin the saved module handler");
    assert!(vm.debugger_nested_continuation.is_none());
    assert!(vm.debugger_module_continuation.is_some());
    assert_eq!(
        vm.resume_debugger_module_execution().unwrap(),
        VmDebuggerExecutionState::Completed
    );
    assert_eq!(vm.debugger_uncaught_throw_site(), None);
    let record = vm.linked_record(entry).unwrap();
    assert!(record.evaluated);
    assert!(record.error.is_none());
    assert_eq!(
        vm.lookup_global_name("caught").unwrap(),
        Some(Value::Number(7.0))
    );
    assert_eq!(
        vm.lookup_global_name("finallyRuns").unwrap(),
        Some(Value::Number(1.0))
    );
}

#[test]
fn nested_module_step_preserves_async_dependency_and_entry_await() {
    let entry = "async-nested/entry.mjs";
    let dependency = "async-nested/dep.mjs";
    let mut registry = BlueJsProgramRegistry::default();
    let dependency_handle = registry
        .install_precompiled(
            BlueJsSourceIdentity::new(dependency, "sha256:async-dependency").unwrap(),
            module_code("globalThis.dependencyRuns = (globalThis.dependencyRuns || 0) + 1; await Promise.resolve(); export const seed = 41;"),
        )
        .unwrap();
    let entry_handle = registry
        .install_precompiled(
            BlueJsSourceIdentity::new(entry, "sha256:async-entry").unwrap(),
            module_code("import { seed } from './dep.mjs'; globalThis.entryRuns = (globalThis.entryRuns || 0) + 1; globalThis.innerRuns = 0; function inner(){globalThis.innerRuns++; return seed + 1;} export const answer = inner(); await Promise.resolve(); globalThis.afterAwait = 1;"),
        )
        .unwrap();
    let graph = HashMap::from([
        (
            dependency.to_string(),
            registry.get(dependency_handle).unwrap().bytecode().clone(),
        ),
        (
            entry.to_string(),
            registry.get(entry_handle).unwrap().bytecode().clone(),
        ),
    ]);
    let mut vm = Vm::default();
    let VmDebuggerNestedExecutionState::Paused { frame_serial, .. } = vm
        .execute_module_graph_until_nested_debugger_pause(entry, &graph, 1, 0)
        .unwrap()
    else {
        panic!("async dependency must settle before entry child pause");
    };
    assert_eq!(
        vm.lookup_global_name("dependencyRuns").unwrap(),
        Some(Value::Number(1.0))
    );
    assert_eq!(
        vm.lookup_global_name("entryRuns").unwrap(),
        Some(Value::Number(1.0))
    );
    assert_eq!(
        vm.lookup_global_name("innerRuns").unwrap(),
        Some(Value::Number(0.0))
    );
    let mut returned = false;
    for _ in 0..128 {
        match vm.step_debugger_nested_instruction(frame_serial).unwrap() {
            VmDebuggerNestedExecutionState::Paused { .. } => {}
            VmDebuggerNestedExecutionState::FrameReturned { .. } => {
                returned = true;
                break;
            }
            other => panic!("unexpected async module child step: {other:?}"),
        }
    }
    assert!(returned);
    assert_eq!(
        vm.resume_debugger_module_execution().unwrap(),
        VmDebuggerExecutionState::Completed
    );
    assert!(vm.linked_record(entry).unwrap().evaluated);
    assert!(vm.linked_record(dependency).unwrap().evaluated);
    assert!(vm.module_continuations.is_empty());
    assert!(vm.promise_jobs.is_empty());
    for (name, expected) in [
        ("dependencyRuns", 1.0),
        ("entryRuns", 1.0),
        ("innerRuns", 1.0),
        ("afterAwait", 1.0),
    ] {
        assert_eq!(
            vm.lookup_global_name(name).unwrap(),
            Some(Value::Number(expected)),
            "{name} must run exactly once"
        );
    }
}

#[test]
fn nested_module_step_budget_failure_clears_frames_without_completion() {
    let entry = "budget/entry.mjs";
    let mut registry = BlueJsProgramRegistry::default();
    let handle = registry
        .install_precompiled(
            BlueJsSourceIdentity::new(entry, "sha256:budget").unwrap(),
            module_code("function inner(){while (true) {}} export const answer = inner();"),
        )
        .unwrap();
    let graph = HashMap::from([(
        entry.to_string(),
        registry.get(handle).unwrap().bytecode().clone(),
    )]);
    let config = VmConfig {
        instruction_budget: 256,
        ..VmConfig::default()
    };
    let mut vm = Vm::new(config).unwrap();
    let VmDebuggerNestedExecutionState::Paused { frame_serial, .. } = vm
        .execute_module_graph_until_nested_debugger_pause(entry, &graph, 1, 0)
        .unwrap()
    else {
        panic!("the loop must pause before its first instruction");
    };
    let mut failure = None;
    for _ in 0..512 {
        match vm.step_debugger_nested_instruction(frame_serial) {
            Ok(VmDebuggerNestedExecutionState::Paused { .. }) => {}
            Err(error) => {
                failure = Some(error);
                break;
            }
            other => panic!("loop cannot complete its nested frame: {other:?}"),
        }
    }
    assert_eq!(failure, Some(RuntimeError::InstructionLimit));
    assert!(vm.debugger_nested_continuation.is_none());
    assert!(vm.debugger_module_continuation.is_none());
    assert!(vm.debugger_nested_parent_execution.is_none());
    let record = vm.linked_record(entry).unwrap();
    assert!(!record.evaluated && !record.suspended && !record.evaluating);
    assert!(record.error.is_none());
    assert_eq!(
        vm.execute_script(&code("1 + 1")).unwrap(),
        Value::Number(2.0)
    );
}

#[test]
fn root_safe_point_preserves_operand_and_global_state_until_resume() {
    let program = code("globalThis.before = 1; globalThis.after = 2;");
    let offset = non_entry_root_offset(&program);
    let mut vm = Vm::default();

    assert_eq!(
        vm.execute_script_until_debugger_pause(&program, offset)
            .unwrap(),
        VmDebuggerExecutionState::Paused {
            bytecode_offset: offset
        }
    );
    assert_eq!(
        vm.execute_script(&code("globalThis.before + globalThis.after")),
        Err(RuntimeError::Unsupported(
            "a debugger-paused root script must resume before another execution starts"
        ))
    );
    let mut modules = HashMap::new();
    modules.insert(
        "blocked.mjs".to_string(),
        compile_module(&parse_module("export const blocked = 1;").unwrap()).unwrap(),
    );
    assert_eq!(
        vm.execute_module_graph("blocked.mjs", &modules),
        Err(RuntimeError::Unsupported(
            "a debugger-paused root script must resume before another execution starts"
        ))
    );
    assert_eq!(
        vm.resume_debugger_execution().unwrap(),
        VmDebuggerExecutionState::Completed
    );
    assert_eq!(
        vm.execute_script(&code("globalThis.before + globalThis.after"))
            .unwrap(),
        Value::Number(3.0)
    );
}

#[test]
fn rejects_non_boundary_and_double_resume_without_destroying_a_realm() {
    let code = code("globalThis.value = 1;");
    let mut vm = Vm::default();
    let non_boundary = (0..code.bytes().len())
        .find(|candidate| {
            !code
                .instructions()
                .any(|instruction| instruction.offset == *candidate)
        })
        .expect("fixture contains an instruction operand byte");
    assert!(matches!(
        vm.execute_script_until_debugger_pause(&code, non_boundary as u32),
        Err(RuntimeError::Unsupported(
            "debugger safe point is not a root bytecode instruction boundary"
        ))
    ));
    assert_eq!(
        vm.execute_script_until_debugger_pause(&code, 0).unwrap(),
        VmDebuggerExecutionState::Paused { bytecode_offset: 0 }
    );
    assert_eq!(
        vm.resume_debugger_execution().unwrap(),
        VmDebuggerExecutionState::Completed
    );
    assert!(matches!(
        vm.resume_debugger_execution(),
        Err(RuntimeError::Unsupported(
            "no debugger-paused root script is available"
        ))
    ));
}

#[test]
fn root_step_preserves_one_continuation_across_branches_and_loop_hits() {
    let program =
        code("let index = 0; while (index < 2) { index++; } globalThis.steppedResult = index;");
    let mut vm = Vm::default();
    assert_eq!(
        vm.execute_script_until_debugger_pause(&program, 0).unwrap(),
        VmDebuggerExecutionState::Paused { bytecode_offset: 0 }
    );
    let instruction_offsets: Vec<_> = program
        .instructions()
        .map(|instruction| instruction.offset as u32)
        .collect();
    let first = vm.step_debugger_root_instruction().unwrap();
    assert_eq!(
        first,
        VmDebuggerExecutionState::Paused {
            bytecode_offset: instruction_offsets[1],
        }
    );
    assert!(matches!(
        vm.execute_script(&code("globalThis.forbidden = true;")),
        Err(RuntimeError::Unsupported(
            "a debugger-paused root script must resume before another execution starts"
        ))
    ));
    let mut offsets = vec![0, instruction_offsets[1]];
    let mut completed = false;
    for _ in 0..256 {
        match vm.step_debugger_root_instruction().unwrap() {
            VmDebuggerExecutionState::Paused { bytecode_offset } => {
                assert!(instruction_offsets.contains(&bytecode_offset));
                offsets.push(bytecode_offset);
            }
            VmDebuggerExecutionState::Completed => {
                completed = true;
                break;
            }
        }
    }
    assert!(
        completed,
        "bounded root steps must reach terminal completion"
    );
    assert!(
        offsets
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            < offsets.len(),
        "a loop must revisit at least one real root instruction"
    );
    assert_eq!(
        vm.execute_script(&code("globalThis.steppedResult"))
            .unwrap(),
        Value::Number(2.0)
    );
    assert!(matches!(
        vm.step_debugger_root_instruction(),
        Err(RuntimeError::Unsupported(
            "no debugger-paused root script is available"
        ))
    ));
}

#[test]
fn root_step_can_resume_to_completion_without_restarting_the_script() {
    let program = code("globalThis.once = (globalThis.once || 0) + 1;");
    let mut vm = Vm::default();
    assert_eq!(
        vm.execute_script_until_debugger_pause(&program, 0).unwrap(),
        VmDebuggerExecutionState::Paused { bytecode_offset: 0 }
    );
    assert!(matches!(
        vm.step_debugger_root_instruction().unwrap(),
        VmDebuggerExecutionState::Paused { .. }
    ));
    assert_eq!(
        vm.resume_debugger_execution().unwrap(),
        VmDebuggerExecutionState::Completed
    );
    assert_eq!(
        vm.execute_script(&code("globalThis.once")).unwrap(),
        Value::Number(1.0)
    );
}

#[test]
fn paused_module_entry_resumes_a_linked_graph_without_replaying_dependencies() {
    let entry = "pages/entry.mjs";
    let dependency = "pages/dependency.mjs";
    let entry_code = compile_module(
        &parse_module("import { answer } from './dependency.mjs'; globalThis.entryRuns = (globalThis.entryRuns || 0) + 1; export const result = answer + 1;").unwrap(),
    )
    .unwrap();
    let dependency_code = compile_module(
        &parse_module("globalThis.dependencyRuns = (globalThis.dependencyRuns || 0) + 1; export const answer = 41;").unwrap(),
    )
    .unwrap();
    let target = entry_code
        .instructions()
        .map(|instruction| instruction.offset as u32)
        .find(|offset| *offset >= entry_code.module_evaluate_entry.unwrap())
        .unwrap();
    let modules = HashMap::from([
        (entry.to_string(), entry_code),
        (dependency.to_string(), dependency_code),
    ]);
    let mut vm = Vm::default();
    assert_eq!(
        vm.execute_module_graph_until_debugger_pause(entry, &modules, target)
            .unwrap(),
        VmDebuggerExecutionState::Paused {
            bytecode_offset: target
        }
    );
    let continuation = vm.debugger_module_continuation.as_ref().unwrap();
    assert_eq!(continuation.module, entry);
    assert_eq!(continuation.pc, target as usize);
    assert_eq!(
        continuation.execution.active_module_name.as_deref(),
        Some(entry)
    );
    let linked = &vm.module_graph.as_ref().unwrap().linked;
    assert!(linked.get(dependency).unwrap().evaluated);
    assert!(linked.get(entry).unwrap().suspended);
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    assert_eq!(
        vm.execute_script(&code("globalThis.entryRuns")),
        Err(RuntimeError::Unsupported(
            "a debugger-paused root module must resume before another execution starts"
        ))
    );
    assert_eq!(
        vm.resume_debugger_module_execution().unwrap(),
        VmDebuggerExecutionState::Completed
    );
    assert_eq!(
        vm.execute_script(&code(
            "globalThis.dependencyRuns * 10 + globalThis.entryRuns"
        ))
        .unwrap(),
        Value::Number(11.0)
    );
    assert!(vm.last_module_namespace.is_some());
    vm.execute_module_graph(entry, &modules).unwrap();
    assert_eq!(
        vm.execute_script(&code(
            "globalThis.dependencyRuns * 10 + globalThis.entryRuns"
        ))
        .unwrap(),
        Value::Number(11.0)
    );
}

#[test]
fn paused_module_rejects_concurrent_entry_points_and_recovers_after_throw() {
    let entry = "throwing/main.mjs";
    let program = module_code(
        "globalThis.runCount = (globalThis.runCount || 0) + 1; throw new TypeError('expected');",
    );
    let offset = module_entry_offset(&program);
    let modules = HashMap::from([(entry.to_string(), program)]);
    let mut vm = Vm::default();
    vm.execute_module_graph_until_debugger_pause(entry, &modules, offset)
        .unwrap();
    let blocked = RuntimeError::Unsupported(
        "a debugger-paused root module must resume before another execution starts",
    );
    assert_eq!(
        vm.execute_module_graph(entry, &modules),
        Err(blocked.clone())
    );
    assert_eq!(vm.execute(&code("1")), Err(blocked.clone()));
    assert_eq!(vm.run_promise_jobs(), Err(blocked.clone()));
    assert_eq!(vm.run_promise_jobs_bounded(1), Err(blocked));
    let thrown = vm.resume_debugger_module_execution().unwrap_err();
    assert!(matches!(thrown, RuntimeError::Thrown(Value::Object(_))));
    assert!(vm.debugger_module_continuation.is_none());
    let record = vm.module_graph.as_ref().unwrap().linked.get(entry).unwrap();
    assert!(record.evaluated);
    assert!(!record.evaluating && !record.suspended);
    assert_eq!(
        record.error,
        Some(match &thrown {
            RuntimeError::Thrown(value) => value.clone(),
            _ => unreachable!("checked thrown completion"),
        })
    );
    assert_eq!(vm.execute_module_graph(entry, &modules), Err(thrown));
    assert_eq!(
        vm.execute_script(&code("globalThis.runCount")).unwrap(),
        Value::Number(1.0)
    );
}

#[test]
fn paused_module_resumes_through_top_level_await_and_cleans_up() {
    let entry = "awaiting/main.mjs";
    let program = module_code("globalThis.beforeAwait = 1; await Promise.resolve(42); globalThis.afterAwait = 1; export const answer = 42;");
    let offset = module_entry_offset(&program);
    let modules = HashMap::from([(entry.to_string(), program)]);
    let mut vm = Vm::default();
    vm.execute_module_graph_until_debugger_pause(entry, &modules, offset)
        .unwrap();
    assert_eq!(
        vm.resume_debugger_module_execution(),
        Ok(VmDebuggerExecutionState::Completed)
    );
    let record = vm.module_graph.as_ref().unwrap().linked.get(entry).unwrap();
    assert!(record.evaluated);
    assert!(!record.evaluating && !record.suspended);
    assert!(vm.module_continuations.is_empty());
    assert!(vm.promise_jobs.is_empty());
    assert_eq!(
        vm.execute_script(&code("globalThis.beforeAwait + globalThis.afterAwait"))
            .unwrap(),
        Value::Number(2.0)
    );
}

#[test]
fn paused_module_preserves_async_dependency_order_and_rejection() {
    let entry = "async/entry.mjs";
    let dependency = "async/dependency.mjs";
    let entry_code = module_code("import { answer } from './dependency.mjs'; globalThis.entryRuns = (globalThis.entryRuns || 0) + 1; export const result = answer + 1;");
    let offset = module_entry_offset(&entry_code);
    let modules = HashMap::from([
        (entry.to_string(), entry_code),
        (dependency.to_string(), module_code("globalThis.dependencyRuns = (globalThis.dependencyRuns || 0) + 1; await Promise.resolve(); export const answer = 41;")),
    ]);
    let mut vm = Vm::default();
    assert_eq!(
        vm.execute_module_graph_until_debugger_pause(entry, &modules, offset),
        Ok(VmDebuggerExecutionState::Paused {
            bytecode_offset: offset
        })
    );
    assert_eq!(
        vm.resume_debugger_module_execution(),
        Ok(VmDebuggerExecutionState::Completed)
    );
    assert_eq!(
        vm.execute_script(&code(
            "globalThis.dependencyRuns * 10 + globalThis.entryRuns"
        ))
        .unwrap(),
        Value::Number(11.0)
    );

    let rejection = "async/reject.mjs";
    let rejected_code = module_code("await Promise.resolve(); throw new TypeError('expected');");
    let rejected_offset = module_entry_offset(&rejected_code);
    let rejected_modules = HashMap::from([(rejection.to_string(), rejected_code)]);
    let mut rejected_vm = Vm::default();
    rejected_vm
        .execute_module_graph_until_debugger_pause(rejection, &rejected_modules, rejected_offset)
        .unwrap();
    let error = rejected_vm.resume_debugger_module_execution().unwrap_err();
    assert!(matches!(error, RuntimeError::Thrown(Value::Object(_))));
    let record = rejected_vm
        .module_graph
        .as_ref()
        .unwrap()
        .linked
        .get(rejection)
        .unwrap();
    assert!(record.evaluated);
    assert!(!record.evaluating && !record.suspended);
    assert!(rejected_vm.module_continuations.is_empty());
    assert!(rejected_vm.promise_jobs.is_empty());
    assert_eq!(
        rejected_vm.execute_module_graph(rejection, &rejected_modules),
        Err(error)
    );
}

#[test]
fn module_root_step_retains_the_frame_across_branches_until_completion() {
    let entry = "stepping/entry.mjs";
    let program = module_code("let index = 0; while (index < 2) { index++; } globalThis.moduleStepped = index; export const answer = index;");
    let entry_offset = module_entry_offset(&program);
    let instruction_offsets: Vec<_> = program
        .instructions()
        .map(|instruction| instruction.offset as u32)
        .filter(|offset| *offset >= entry_offset)
        .collect();
    let modules = HashMap::from([(entry.to_string(), program)]);
    let mut vm = Vm::default();
    assert_eq!(
        vm.execute_module_graph_until_debugger_pause(entry, &modules, entry_offset),
        Ok(VmDebuggerExecutionState::Paused {
            bytecode_offset: entry_offset
        })
    );
    let mut offsets = vec![entry_offset];
    let mut completed = false;
    for _ in 0..256 {
        match vm.step_debugger_module_root_instruction().unwrap() {
            VmDebuggerExecutionState::Paused { bytecode_offset } => {
                assert!(instruction_offsets.contains(&bytecode_offset));
                offsets.push(bytecode_offset);
                assert!(vm.debugger_module_continuation.is_some());
            }
            VmDebuggerExecutionState::Completed => {
                completed = true;
                break;
            }
        }
    }
    assert!(completed, "bounded module steps must complete");
    assert!(
        offsets
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            < offsets.len()
    );
    assert!(vm.debugger_module_continuation.is_none());
    assert_eq!(
        vm.execute_script(&code("globalThis.moduleStepped"))
            .unwrap(),
        Value::Number(2.0)
    );
    assert!(matches!(
        vm.step_debugger_module_root_instruction(),
        Err(RuntimeError::Unsupported(
            "no debugger-paused root module is available"
        ))
    ));
}

#[test]
fn paused_continuation_iterator_edges_are_roots_at_gc_safepoints() {
    let mut vm = Vm::default();
    let iterator_record = vm.heap.alloc_object(None).unwrap();
    vm.debugger_continuation = Some(DebuggerContinuation {
        code: Bytecode::empty(),
        pc: 0,
        iterators: vec![Value::Object(iterator_record)],
        handlers: Vec::new(),
    });

    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    assert!(
        vm.heap.contains(iterator_record),
        "a paused iterator record must remain alive across a VM GC safepoint"
    );
}
