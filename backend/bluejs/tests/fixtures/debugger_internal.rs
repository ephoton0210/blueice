// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use crate::{
    compile, compile_module, parse, parse_module, BlueJsProgramRegistry, BlueJsProgramV1,
    BlueJsSourceIdentity, Value,
};
use std::collections::HashMap;

fn code(source: &str) -> Bytecode {
    compile(&parse(source).expect("test source parses")).expect("test source compiles")
}

fn non_entry_root_offset(code: &Bytecode) -> u32 {
    code.instructions()
        .map(|instruction| instruction.offset as u32)
        .find(|offset| *offset != 0)
        .expect("test source has a non-entry root instruction")
}

fn module_code(source: &str) -> Bytecode {
    compile_module(&parse_module(source).expect("test module parses"))
        .expect("test module compiles")
}

fn module_entry_offset(code: &Bytecode) -> u32 {
    code.instructions()
        .map(|instruction| instruction.offset as u32)
        .find(|offset| *offset >= code.module_evaluate_entry.unwrap())
        .expect("test module has an evaluation instruction")
}

fn installed_script(source: &str) -> Bytecode {
    let mut registry = BlueJsProgramRegistry::default();
    let handle = registry
        .install(
            BlueJsSourceIdentity::new("page:///nested.js", "sha256:nested").unwrap(),
            &BlueJsProgramV1::Script(parse(source).unwrap()),
        )
        .unwrap();
    registry.get(handle).unwrap().bytecode().clone()
}

fn installed_module(entry: &str, source: &str) -> Bytecode {
    let mut registry = BlueJsProgramRegistry::default();
    let handle = registry
        .install_precompiled(
            BlueJsSourceIdentity::new(entry, "sha256:throw-site").unwrap(),
            module_code(source),
        )
        .unwrap();
    registry.get(handle).unwrap().bytecode().clone()
}

fn last_throw_site(code: &Bytecode) -> VmDebuggerThrowSite {
    VmDebuggerThrowSite {
        program_generation: code.debugger_program_generation.unwrap(),
        code_unit_ordinal: code.debugger_code_unit_ordinal.unwrap(),
        bytecode_offset: code
            .instructions()
            .filter(|instruction| instruction.opcode == Opcode::Throw)
            .last()
            .unwrap()
            .offset as u32,
    }
}

#[cfg_attr(test, test)]
fn skipped_root_targets_complete_and_owned_child_frames_block_new_entries() {
    let program = installed_script("if (false) { throw 7; } 42;");
    let target = program
        .instructions()
        .find(|instruction| instruction.opcode == Opcode::Throw)
        .unwrap()
        .offset as u32;
    let mut vm = Vm::default();
    assert_eq!(
        vm.execute_script_until_debugger_pause(&program, target),
        Ok(VmDebuggerExecutionState::Completed)
    );
    assert!(vm.stack.is_empty() && vm.debugger_continuation.is_none());

    let child = installed_script("function child() { return 42; } child();");
    let module = installed_module(
        "pause/main.mjs",
        "function child() { return 42; } export const answer = child();",
    );
    let graph = HashMap::from([("pause/main.mjs".to_string(), module)]);
    let root_offset = module_entry_offset(&graph["pause/main.mjs"]);
    let VmDebuggerNestedExecutionState::Paused { frame_serial, .. } = vm
        .execute_script_until_nested_debugger_pause(&child, 1, 0)
        .unwrap()
    else {
        panic!("the child must pause at its entry")
    };
    let blocked = Err(RuntimeError::Unsupported(
        "a debugger-paused root script must resume before another execution starts",
    ));
    assert_eq!(
        vm.execute_script_until_debugger_pause(&child, 0)
            .map(|_| ()),
        blocked
    );
    assert_eq!(
        vm.execute_script_until_nested_debugger_pause(&child, 1, 0)
            .map(|_| ()),
        blocked
    );
    assert_eq!(
        vm.execute_module_graph_until_debugger_pause("pause/main.mjs", &graph, root_offset)
            .map(|_| ()),
        blocked
    );
    assert_eq!(
        vm.execute_module_graph_until_nested_debugger_pause("pause/main.mjs", &graph, 1, 0)
            .map(|_| ()),
        blocked
    );
    assert_eq!(
        vm.execute_module_graph_until_linked_nested_debugger_pause(
            "pause/main.mjs",
            "pause/dep.mjs",
            &graph,
            VmDebuggerLinkedPauseTarget {
                entry_generation: 1,
                dependency_generation: 1,
                code_unit_ordinal: 1,
                bytecode_offset: 0
            }
        )
        .map(|_| ()),
        blocked
    );

    // Keep the real root continuation in the host's synchronous ownership.
    // The entry refusal runs before preparation, allocation or collection;
    // restore that same caller before the child can resume.
    let caller = vm.debugger_continuation.take().unwrap();
    assert_eq!(
        vm.execute_script(&code("42")),
        Err(RuntimeError::Unsupported(
            "a debugger-paused nested frame must resume before another execution starts"
        ))
    );
    vm.debugger_continuation = Some(caller);
    assert!(matches!(
        vm.resume_debugger_nested_execution(frame_serial),
        Ok(VmDebuggerNestedExecutionState::FrameReturned { .. })
    ));
    assert_eq!(
        vm.resume_debugger_execution(),
        Ok(VmDebuggerExecutionState::Completed)
    );
    assert_eq!(vm.execute_script(&code("21 + 21")), Ok(Value::Number(42.0)));
}

#[cfg_attr(test, test)]
fn nested_module_target_validation_reports_host_graph_errors_before_execution() {
    let entry = "validation/main.mjs";
    let dependency = "validation/dep.mjs";
    let entry_code = installed_module(entry, "import {child} from './dep.mjs'; child();");
    let dependency_code = installed_module(dependency, "export function child() {return 42;}");
    let target = VmDebuggerLinkedPauseTarget {
        entry_generation: entry_code.debugger_program_generation.unwrap(),
        dependency_generation: dependency_code.debugger_program_generation.unwrap(),
        code_unit_ordinal: 1,
        bytecode_offset: 0,
    };
    let graph = HashMap::from([
        (entry.to_string(), entry_code),
        (dependency.to_string(), dependency_code),
    ]);
    let mut vm = Vm::default();
    assert_eq!(
        vm.execute_module_graph_until_nested_debugger_pause("absent.mjs", &graph, 1, 0),
        Err(RuntimeError::Unsupported(
            "nested debugger module entry is absent from the graph"
        ))
    );
    let classic = HashMap::from([(
        entry.to_string(),
        installed_script("function child() {return 42;} child();"),
    )]);
    assert_eq!(
        vm.execute_module_graph_until_nested_debugger_pause(entry, &classic, 1, 0),
        Err(RuntimeError::Unsupported(
            "nested debugger entry is not a module"
        ))
    );
    assert_eq!(
        vm.execute_module_graph_until_nested_debugger_pause(dependency, &graph, 1, u32::MAX),
        Err(RuntimeError::Unsupported(
            "nested debugger target is not an inner instruction boundary"
        ))
    );
    assert_eq!(
        Vm::validate_linked_nested_debugger_target(entry, entry, &graph, target),
        Err(RuntimeError::Unsupported(
            "linked debugger child must belong to a separate dependency"
        ))
    );
    assert_eq!(
        Vm::validate_linked_nested_debugger_target("absent.mjs", dependency, &graph, target),
        Err(RuntimeError::Unsupported(
            "linked debugger entry is absent from the graph"
        ))
    );
    assert_eq!(
        Vm::validate_linked_nested_debugger_target(entry, "absent.mjs", &graph, target),
        Err(RuntimeError::Unsupported(
            "linked debugger dependency is absent from the graph"
        ))
    );
    let invalid = VmDebuggerLinkedPauseTarget {
        bytecode_offset: u32::MAX,
        ..target
    };
    assert_eq!(
        Vm::validate_linked_nested_debugger_target(entry, dependency, &graph, invalid),
        Err(RuntimeError::Unsupported(
            "nested debugger target is not an inner instruction boundary"
        ))
    );
    let mut incomplete = graph.clone();
    incomplete.insert(
        entry.to_string(),
        installed_module(entry, "import './absent.mjs';"),
    );
    assert!(
        matches!(Vm::validate_linked_nested_debugger_target(entry, dependency, &incomplete, target), Err(RuntimeError::ModuleResolution(message)) if message.contains("absent.mjs"))
    );
    assert!(vm.debugger_nested_pause_request.is_none());
    assert_eq!(vm.execute_script(&code("21 + 21")), Ok(Value::Number(42.0)));
}

#[cfg_attr(test, test)]
fn debugger_module_resume_handles_allocation_and_await_serial_exhaustion() {
    let entry = "resources/main.mjs";
    let reuse = code("21 + 21");
    for source in [
        "export const answer = 42;",
        "await Promise.resolve(); export const answer = 42;",
        "export const answer = {value: 42};",
    ] {
        let program = installed_module(entry, source);
        let offset = module_entry_offset(&program);
        let graph = HashMap::from([(entry.to_string(), program)]);
        let mut next_extra = 0;
        let mut failures = 0;
        let mut completed = false;
        while next_extra <= 16 * 1024 * 1024 {
            let extra = next_extra;
            let mut vm = Vm::default();
            vm.execute_script(&code("Promise; TypeError;")).unwrap();
            assert_eq!(
                vm.execute_module_graph_until_debugger_pause(entry, &graph, offset),
                Ok(VmDebuggerExecutionState::Paused {
                    bytecode_offset: offset
                })
            );
            let limit = vm.heap.allow_only(extra);
            match vm.resume_debugger_module_execution() {
                Ok(VmDebuggerExecutionState::Completed) => completed = true,
                Err(RuntimeError::Heap(crate::heap::HeapError::HeapLimitExceeded {
                    limit: observed,
                })) => {
                    assert_eq!(observed, limit);
                    failures += 1;
                    next_extra = vm.heap.next_allocation_headroom(extra);
                }
                result => panic!("module resume {source}, headroom {extra}: {result:?}"),
            }
            assert!(
                vm.debugger_module_continuation.is_none()
                    && vm.debugger_nested_continuation.is_none()
            );
            assert!(vm.stack.is_empty() && vm.bindings.is_empty());
            vm.heap.allow_only(16 * 1024 * 1024);
            assert_eq!(vm.execute_script(&reuse), Ok(Value::Number(42.0)));
            if completed {
                break;
            }
        }
        assert!(
            completed && failures > 0,
            "no allocation refusal for {source}"
        );
    }
    let program = installed_module(entry, "await Promise.resolve(); export const answer = 42;");
    let offset = module_entry_offset(&program);
    let graph = HashMap::from([(entry.to_string(), program)]);
    let mut vm = Vm::default();
    vm.execute_script(&code("Promise;")).unwrap();
    vm.execute_module_graph_until_debugger_pause(entry, &graph, offset)
        .unwrap();
    vm.next_module_continuation = u64::MAX;
    assert_eq!(
        vm.resume_debugger_module_execution(),
        Err(RuntimeError::InstructionLimit)
    );
    assert!(
        vm.debugger_module_continuation.is_none() && vm.stack.is_empty() && vm.bindings.is_empty()
    );
    assert_eq!(vm.execute_script(&reuse), Ok(Value::Number(42.0)));
}

#[cfg_attr(test, test)]
fn nested_throw_protocol_retains_handlers_and_original_errors() {
    assert!(matches!(
        NestedThrowAction::classify(Ok(CompletionAction::Jump(17))),
        NestedThrowAction::Handler(17)
    ));
    let error = RuntimeError::Thrown(Value::Number(7.0));
    for action in [
        Ok(CompletionAction::Throw(error.clone())),
        Err(error.clone()),
    ] {
        match NestedThrowAction::classify(action) {
            NestedThrowAction::Failed(actual) => assert_eq!(actual, error),
            _ => panic!("a thrown completion resumed without a handler"),
        }
    }
    for action in [
        CompletionAction::Continue,
        CompletionAction::Return(Value::Undefined),
        CompletionAction::TailRecur(Vec::new()),
        CompletionAction::TailCall(Vec::new()),
    ] {
        assert!(matches!(
            NestedThrowAction::classify(Ok(action)),
            NestedThrowAction::Failed(RuntimeError::Unsupported(
                "nested throw did not produce a handler successor"
            ))
        ));
    }
}

#[cfg_attr(test, test)]
fn nested_module_object_throws_preserve_identity_and_root_failure_cleans_both_frames() {
    for exhaust_roots in [false, true] {
        let entry = "rooting/main.mjs";
        let program = installed_module(entry,"function child() {throw globalThis.sentinel;} globalThis.sentinel = {answer:42}; child();");
        let graph = HashMap::from([(entry.to_string(), program)]);
        let mut vm = Vm::default();
        let VmDebuggerNestedExecutionState::Paused { frame_serial, .. } = vm
            .execute_module_graph_until_nested_debugger_pause(entry, &graph, 1, 0)
            .unwrap()
        else {
            panic!("child did not pause");
        };
        let object = vm
            .lookup_global_name("sentinel")
            .unwrap()
            .unwrap()
            .object_id()
            .unwrap();
        if exhaust_roots {
            vm.heap.allow_root_registrations(0);
        }
        let error = vm
            .resume_debugger_nested_execution(frame_serial)
            .unwrap_err();
        if exhaust_roots {
            assert_eq!(
                error,
                RuntimeError::Heap(crate::heap::HeapError::IdExhausted)
            );
        } else {
            assert_eq!(error, RuntimeError::Thrown(Value::Object(object)));
            assert_eq!(
                vm.linked_record(entry).unwrap().error,
                Some(Value::Object(object))
            );
        }
        assert!(
            vm.debugger_nested_continuation.is_none() && vm.debugger_module_continuation.is_none()
        );
        assert!(vm.stack.is_empty() && vm.bindings.is_empty() && vm.cells.is_empty());
        assert!(vm.heap.contains(object));
        assert_eq!(vm.execute_script(&code("21 + 21")), Ok(Value::Number(42.0)));
    }
}

#[cfg_attr(test, test)]
fn rejected_async_dependencies_prevent_root_and_linked_child_pauses() {
    let entry = "reject/main.mjs";
    let dependency = "reject/dep.mjs";
    let mut registry = BlueJsProgramRegistry::default();
    let entry_handle = registry
        .install_precompiled(
            BlueJsSourceIdentity::new(entry, "sha256:reject-main").unwrap(),
            module_code(
                "import {child} from './dep.mjs'; globalThis.entryReached = true; child();",
            ),
        )
        .unwrap();
    let dependency_handle = registry
        .install_precompiled(
            BlueJsSourceIdentity::new(dependency, "sha256:reject-dep").unwrap(),
            module_code("await Promise.resolve(); throw 7; export function child() {return 42;}"),
        )
        .unwrap();
    let entry_code = registry.get(entry_handle).unwrap().bytecode().clone();
    let dependency_code = registry.get(dependency_handle).unwrap().bytecode().clone();
    let target = VmDebuggerLinkedPauseTarget {
        entry_generation: entry_code.debugger_program_generation.unwrap(),
        dependency_generation: dependency_code.debugger_program_generation.unwrap(),
        code_unit_ordinal: 1,
        bytecode_offset: 0,
    };
    let offset = module_entry_offset(&entry_code);
    let graph = HashMap::from([
        (entry.to_string(), entry_code),
        (dependency.to_string(), dependency_code),
    ]);
    for linked in [false, true] {
        let mut vm = Vm::default();
        let result = if linked {
            vm.execute_module_graph_until_linked_nested_debugger_pause(
                entry, dependency, &graph, target,
            )
            .map(|_| ())
        } else {
            vm.execute_module_graph_until_debugger_pause(entry, &graph, offset)
                .map(|_| ())
        };
        assert_eq!(result, Err(RuntimeError::Thrown(Value::Number(7.0))));
        assert!(
            vm.debugger_nested_pause_request.is_none()
                && vm.debugger_module_pause_request.is_none()
        );
        assert!(
            vm.debugger_nested_continuation.is_none() && vm.debugger_module_continuation.is_none()
        );
        assert_eq!(vm.lookup_global_name("entryReached").unwrap(), None);
        assert_eq!(vm.execute_script(&code("21 + 21")), Ok(Value::Number(42.0)));
    }
}

#[cfg_attr(test, test)]
fn a_tighter_string_budget_at_a_real_pause_aborts_classic_and_module_frames_cleanly() {
    for module in [false, true] {
        let text = "a".repeat(64);
        let source = format!("function child() {{return '{text}';}} globalThis.answer = child();");
        let program = if module {
            installed_module("m/main.mjs", &source)
        } else {
            installed_script(&source)
        };
        let offset = program
            .child_code_units()
            .next()
            .unwrap()
            .instructions()
            .find(|instruction| instruction.opcode == Opcode::Return)
            .unwrap()
            .offset as u32;
        let mut vm = Vm::default();
        let paused = if module {
            vm.execute_module_graph_until_nested_debugger_pause(
                "m/main.mjs",
                &HashMap::from([("m/main.mjs".into(), program)]),
                1,
                offset,
            )
        } else {
            vm.execute_script_until_nested_debugger_pause(&program, 1, offset)
        };
        let VmDebuggerNestedExecutionState::Paused { frame_serial, .. } = paused.unwrap() else {
            panic!("the real child did not pause at its return");
        };
        let limit = vm.config.max_string_bytes;
        vm.config.max_string_bytes = 8;
        let resumed = vm.resume_debugger_nested_execution(frame_serial);
        vm.config.max_string_bytes = limit;
        if module {
            // Module evaluation retains a catchable error as its rejection
            // value; classic execution reports the runtime error directly.
            let Err(RuntimeError::Thrown(error)) = resumed else {
                panic!("module string exhaustion lost its rejection: {resumed:?}");
            };
            assert_eq!(
                vm.get_property(&error, &"name".into()).unwrap(),
                Value::String("RangeError".into())
            );
            assert_eq!(
                vm.get_property(&error, &"message".into()).unwrap(),
                Value::String("BlueJS string exceeds 8 bytes".into())
            );
        } else {
            assert_eq!(resumed, Err(RuntimeError::StringLimit { limit: 8 }));
        }
        assert!(vm.debugger_nested_continuation.is_none());
        assert!(vm.debugger_continuation.is_none() && vm.debugger_module_continuation.is_none());
        assert!(vm.stack.is_empty() && vm.bindings.is_empty());
        assert_eq!(vm.execute_script(&code("21 + 21")), Ok(Value::Number(42.0)));
    }
}

#[cfg_attr(test, test)]
fn uncaught_classic_throw_site_is_exact_and_caught_or_successor_sites_clear() {
    let program = installed_script("let before = 1; throw 7;");
    let throw_offset = program
        .instructions()
        .find(|instruction| instruction.opcode == Opcode::Throw)
        .unwrap()
        .offset as u32;
    let expected = VmDebuggerThrowSite {
        program_generation: program.debugger_program_generation.unwrap(),
        code_unit_ordinal: 0,
        bytecode_offset: throw_offset,
    };
    let mut vm = Vm::default();
    assert_eq!(vm.debugger_uncaught_throw_site(), None);
    assert_eq!(
        vm.execute_script(&program),
        Err(RuntimeError::Thrown(Value::Number(7.0)))
    );
    assert_eq!(vm.debugger_uncaught_throw_site(), Some(expected));

    let caught = installed_script("try { throw 8; } catch (error) { globalThis.caught = error; }");
    vm.execute_script(&caught).unwrap();
    assert_eq!(vm.debugger_uncaught_throw_site(), None);
    assert_eq!(
        vm.lookup_global_name("caught").unwrap(),
        Some(Value::Number(8.0))
    );

    let successor = installed_script("globalThis.done = true;");
    vm.execute_script(&successor).unwrap();
    assert_eq!(vm.debugger_uncaught_throw_site(), None);
    assert_eq!(
        vm.execute_script(&code("throw 9;")),
        Err(RuntimeError::Thrown(Value::Number(9.0)))
    );
    assert_eq!(vm.debugger_uncaught_throw_site(), None);
}

#[cfg_attr(test, test)]
fn nested_throw_origin_survives_caller_and_rethrow_records_catch_site() {
    let nested = installed_script("function inner() { throw 7; } inner();");
    let child_site = last_throw_site(nested.child_code_units().next().unwrap());
    let mut vm = Vm::default();
    assert_eq!(
        vm.execute_script(&nested),
        Err(RuntimeError::Thrown(Value::Number(7.0)))
    );
    assert_eq!(vm.debugger_uncaught_throw_site(), Some(child_site));

    let rethrow = installed_script(
        "function inner() { throw 7; } try { inner(); } catch (error) { throw 8; }",
    );
    assert_eq!(
        vm.execute_script(&rethrow),
        Err(RuntimeError::Thrown(Value::Number(8.0)))
    );
    assert_eq!(
        vm.debugger_uncaught_throw_site(),
        Some(last_throw_site(&rethrow))
    );
}

#[cfg_attr(test, test)]
fn finally_throw_replaces_prior_site_even_when_the_replacement_is_nested() {
    let direct = installed_script("try { throw 7; } finally { throw 8; }");
    let mut vm = Vm::default();
    assert_eq!(
        vm.execute_script(&direct),
        Err(RuntimeError::Thrown(Value::Number(8.0)))
    );
    assert_eq!(
        vm.debugger_uncaught_throw_site(),
        Some(last_throw_site(&direct))
    );

    let nested = installed_script(
        "function replacement() { throw 9; } try { throw 7; } finally { replacement(); }",
    );
    let child_site = last_throw_site(nested.child_code_units().next().unwrap());
    assert_eq!(
        vm.execute_script(&nested),
        Err(RuntimeError::Thrown(Value::Number(9.0)))
    );
    assert_eq!(vm.debugger_uncaught_throw_site(), Some(child_site));
}

#[cfg_attr(test, test)]
fn module_root_and_nested_throw_sites_keep_their_installed_code_units() {
    let entry = "page:///throw-site.mjs";
    let root = installed_module(entry, "throw 7;");
    let mut vm = Vm::default();
    assert_eq!(
        vm.execute_module_graph(entry, &HashMap::from([(entry.to_string(), root.clone())])),
        Err(RuntimeError::Thrown(Value::Number(7.0)))
    );
    assert_eq!(
        vm.debugger_uncaught_throw_site(),
        Some(last_throw_site(&root))
    );

    let nested = installed_module(
        entry,
        "function inner() { throw 9; } export const answer = inner();",
    );
    let child_site = last_throw_site(nested.child_code_units().next().unwrap());
    let mut vm = Vm::default();
    assert_eq!(
        vm.execute_module_graph(entry, &HashMap::from([(entry.to_string(), nested)])),
        Err(RuntimeError::Thrown(Value::Number(9.0)))
    );
    assert_eq!(vm.debugger_uncaught_throw_site(), Some(child_site));
}

#[cfg_attr(test, test)]
fn captured_parent_binding_is_not_an_active_child_scope_slot() {
    let program =
        installed_script("let outer=7; function inner(){let own=1; return outer+own;} inner();");
    let outer = program.root_declaration_binding_slots()[0].unwrap();
    let child_code = program.child_code_units().next().unwrap();
    let captured = child_code
        .captures
        .iter()
        .position(|slot| *slot == outer)
        .unwrap() as u32;
    let own = child_code
        .bindings
        .iter()
        .position(|binding| binding.name == "own")
        .unwrap() as u32;
    let mut vm = Vm::default();
    let VmDebuggerNestedExecutionState::Paused { frame_serial, .. } = vm
        .execute_script_until_nested_debugger_pause(&program, 1, 0)
        .unwrap()
    else {
        panic!("child must pause");
    };
    for _ in 0..64 {
        let snapshot = vm
            .debugger_stack_snapshot(Some(frame_serial), 2, 256)
            .unwrap();
        if snapshot.frames[0]
            .scope_entries
            .iter()
            .any(|entry| entry.slot_ordinal == own)
        {
            assert!(!snapshot.frames[0]
                .scope_entries
                .iter()
                .any(|entry| entry.slot_ordinal == captured));
            assert!(snapshot.frames[1]
                .scope_entries
                .iter()
                .any(|entry| entry.slot_ordinal == outer));
            return;
        }
        assert!(matches!(
            vm.step_debugger_nested_instruction(frame_serial).unwrap(),
            VmDebuggerNestedExecutionState::Paused { .. }
        ));
    }
    panic!("child local must become active");
}

#[cfg_attr(test, test)]
fn stack_snapshot_excludes_inactive_scopes_and_never_evaluates_a_getter() {
    let program = installed_script(
        "var result = 0; function inner(a, b) { let first = a; let second = b; if (false) { let hidden = 9; } return first + second + watched.value; } result = inner(1, 2);",
    );
    let child_code = program.child_code_units().next().unwrap();
    let slot = |name: &str| {
        u32::try_from(
            child_code
                .bindings
                .iter()
                .position(|binding| binding.name == name)
                .unwrap(),
        )
        .unwrap()
    };
    let (first, second, hidden) = (slot("first"), slot("second"), slot("hidden"));
    let mut vm = Vm::default();
    vm.execute_script(&code(
        "var getterRuns = 0; var watched = { get value() { getterRuns++; return 7; } };",
    ))
    .unwrap();
    let VmDebuggerNestedExecutionState::Paused { frame_serial, .. } = vm
        .execute_script_until_nested_debugger_pause(&program, 1, 0)
        .unwrap()
    else {
        panic!("the direct child must pause");
    };
    for _ in 0..64 {
        let active = &vm
            .debugger_nested_continuation
            .as_ref()
            .unwrap()
            .execution
            .active_scope_slots;
        if active.iter().flatten().any(|slot| *slot == first)
            && active.iter().flatten().any(|slot| *slot == second)
        {
            break;
        }
        assert!(matches!(
            vm.step_debugger_nested_instruction(frame_serial).unwrap(),
            VmDebuggerNestedExecutionState::Paused { .. }
        ));
    }
    let full = vm
        .debugger_stack_snapshot(Some(frame_serial), 2, 256)
        .unwrap();
    assert_eq!(
        full.program_generation,
        program.debugger_program_generation.unwrap()
    );
    assert_eq!(full.frames.len(), 2);
    assert_eq!(full.frames[0].program_generation, full.program_generation);
    assert_eq!(full.frames[1].program_generation, full.program_generation);
    assert_eq!(full.frames[0].code_unit_ordinal, 1);
    assert_eq!(full.frames[1].code_unit_ordinal, 0);
    assert!(!full.stack_truncated);
    assert!(full.frames.iter().all(|frame| !frame.scope_truncated));
    let slots = &full.frames[0].scope_entries;
    assert!(slots.iter().any(|entry| entry.slot_ordinal == first));
    assert!(slots.iter().any(|entry| entry.slot_ordinal == second));
    assert!(!slots.iter().any(|entry| entry.slot_ordinal == hidden));
    let limited = vm
        .debugger_stack_snapshot(Some(frame_serial), 1, 1)
        .unwrap();
    assert_eq!(limited.frames.len(), 1);
    assert!(limited.stack_truncated);
    assert_eq!(limited.frames[0].scope_entries.len(), 1);
    assert!(limited.frames[0].scope_truncated);
    assert_eq!(
        vm.debugger_stack_snapshot(Some(frame_serial), 2, 256)
            .unwrap(),
        full
    );
    assert_eq!(
        vm.lookup_global_name("getterRuns").unwrap(),
        Some(Value::Number(0.0))
    );
    assert!(vm.debugger_stack_snapshot(None, 2, 256).is_err());
    assert!(vm
        .debugger_stack_snapshot(Some(frame_serial + 1), 2, 256)
        .is_err());
    for limits in [(0, 1), (65, 1), (1, 0), (1, 257)] {
        assert!(vm
            .debugger_stack_snapshot(Some(frame_serial), limits.0, limits.1)
            .is_err());
    }
    assert!(matches!(
        vm.resume_debugger_nested_execution(frame_serial).unwrap(),
        VmDebuggerNestedExecutionState::FrameReturned { .. }
    ));
    assert!(vm
        .debugger_stack_snapshot(Some(frame_serial), 2, 256)
        .is_err());
    let root = vm.debugger_stack_snapshot(None, 2, 256).unwrap();
    assert_eq!(root.frames.len(), 1);
    assert_eq!(root.frames[0].code_unit_ordinal, 0);
    assert!(!root.stack_truncated);
    assert_eq!(
        vm.lookup_global_name("getterRuns").unwrap(),
        Some(Value::Number(1.0))
    );
    vm.resume_debugger_execution().unwrap();
    assert!(vm.debugger_stack_snapshot(None, 2, 256).is_err());
}

fn active_entry(frame: &VmDebuggerStackFrame, code: &Bytecode, name: &str) -> VmDebuggerScopeEntry {
    let slot_ordinal = code
        .bindings
        .iter()
        .position(|binding| binding.name == name)
        .and_then(|slot| u32::try_from(slot).ok())
        .expect("binding has a wire-sized slot");
    *frame
        .scope_entries
        .iter()
        .find(|entry| entry.slot_ordinal == slot_ordinal)
        .expect("binding is active in the selected frame")
}

#[cfg_attr(test, test)]
fn paused_root_previews_lossless_primitives_and_rejects_wrong_targets() {
    let program = installed_script(
        "let flag = true; let empty = null; let missing; let minus = -0; let not_a_number = NaN; let count = 123456789012345678901234567890n; let text = '\\ud800'; let too_long = 'x'.repeat(2049); let too_big = 1n << 32768n; let object = { a: 1 };",
    );
    let halt = program.instructions().last().unwrap().offset as u32;
    let mut vm = Vm::default();
    assert_eq!(
        vm.execute_script_until_debugger_pause(&program, halt)
            .unwrap(),
        VmDebuggerExecutionState::Paused {
            bytecode_offset: halt
        }
    );
    let frame = &vm.debugger_stack_snapshot(None, 1, 256).unwrap().frames[0];
    let read = |name| {
        vm.debugger_value_preview(
            None,
            0,
            frame.code_unit_ordinal,
            frame.bytecode_offset,
            active_entry(frame, &program, name),
        )
    };
    assert_eq!(read("flag").unwrap(), VmDebuggerValuePreview::Bool(true));
    assert_eq!(read("empty").unwrap(), VmDebuggerValuePreview::Null);
    assert_eq!(read("missing").unwrap(), VmDebuggerValuePreview::Undefined);
    assert_eq!(
        read("minus").unwrap(),
        VmDebuggerValuePreview::NumberBits((-0.0_f64).to_bits())
    );
    assert!(matches!(
        read("not_a_number").unwrap(),
        VmDebuggerValuePreview::NumberBits(bits) if f64::from_bits(bits).is_nan()
    ));
    assert_eq!(
        read("count").unwrap(),
        VmDebuggerValuePreview::BigIntBytes(
            num_bigint::BigInt::parse_bytes(b"123456789012345678901234567890", 10)
                .unwrap()
                .to_signed_bytes_le()
        )
    );
    assert_eq!(
        read("text").unwrap(),
        VmDebuggerValuePreview::StringUnits(vec![0xd800])
    );
    assert!(read("too_long").is_err());
    assert!(read("too_big").is_err());
    assert_eq!(
        read("object").unwrap(),
        VmDebuggerValuePreview::Record(vec![(
            "a".into(),
            VmDebuggerValuePreview::NumberBits(1.0_f64.to_bits()),
        )])
    );
    let flag = active_entry(frame, &program, "flag");
    for (serial, index, unit, offset, entry) in [
        (Some(1), 0, 0, halt, flag),
        (None, 1, 0, halt, flag),
        (None, 0, 1, halt, flag),
        (None, 0, 0, halt - 1, flag),
        (
            None,
            0,
            0,
            halt,
            VmDebuggerScopeEntry {
                slot_ordinal: u32::MAX,
                scope_depth: 0,
            },
        ),
    ] {
        assert!(vm
            .debugger_value_preview(serial, index, unit, offset, entry)
            .is_err());
    }
    vm.resume_debugger_execution().unwrap();
    assert!(vm.debugger_value_preview(None, 0, 0, halt, flag).is_err());
}

#[cfg_attr(test, test)]
fn paused_nested_preview_reads_cell_backed_capture_without_running_child() {
    let program = installed_script(
        "function inner() { let captured = 17; const sink = () => captured; return captured; } inner();",
    );
    let child = program.child_code_units().next().unwrap();
    let mut vm = Vm::default();
    let VmDebuggerNestedExecutionState::Paused { frame_serial, .. } = vm
        .execute_script_until_nested_debugger_pause(&program, 1, 0)
        .unwrap()
    else {
        panic!("inner closure must pause");
    };
    let mut preview = None;
    for _ in 0..64 {
        let frame = &vm
            .debugger_stack_snapshot(Some(frame_serial), 2, 256)
            .unwrap()
            .frames[0];
        if let Some(entry) = child
            .bindings
            .iter()
            .position(|binding| binding.name == "captured")
            .and_then(|slot| {
                if !vm
                    .debugger_nested_continuation
                    .as_ref()
                    .unwrap()
                    .execution
                    .cells
                    .contains_key(&slot)
                {
                    return None;
                }
                frame
                    .scope_entries
                    .iter()
                    .find(|entry| entry.slot_ordinal == slot as u32)
            })
        {
            preview = Some(
                vm.debugger_value_preview(
                    Some(frame_serial),
                    0,
                    frame.code_unit_ordinal,
                    frame.bytecode_offset,
                    *entry,
                )
                .unwrap(),
            );
            break;
        }
        vm.step_debugger_nested_instruction(frame_serial).unwrap();
    }
    assert_eq!(
        preview,
        Some(VmDebuggerValuePreview::NumberBits(17.0_f64.to_bits()))
    );
}

#[cfg_attr(test, test)]
fn paused_module_root_preview_reads_retained_module_binding() {
    let entry = "preview/main.mjs";
    let mut registry = BlueJsProgramRegistry::default();
    let handle = registry
        .install_precompiled(
            BlueJsSourceIdentity::new(entry, "sha256:preview").unwrap(),
            module_code("let answer = 41; export const result = answer + 1;"),
        )
        .unwrap();
    let program = registry.get(handle).unwrap().bytecode().clone();
    let graph = HashMap::from([(entry.to_string(), program.clone())]);
    let mut vm = Vm::default();
    vm.execute_module_graph_until_debugger_pause(entry, &graph, module_entry_offset(&program))
        .unwrap();
    let mut preview = None;
    for _ in 0..64 {
        let frame = &vm.debugger_stack_snapshot(None, 1, 256).unwrap().frames[0];
        let slot = program
            .bindings
            .iter()
            .position(|binding| binding.name == "answer")
            .unwrap() as u32;
        if let Some(entry) = frame
            .scope_entries
            .iter()
            .find(|entry| entry.slot_ordinal == slot)
        {
            if let Ok(value) = vm.debugger_value_preview(
                None,
                0,
                frame.code_unit_ordinal,
                frame.bytecode_offset,
                *entry,
            ) {
                preview = Some(value);
                break;
            }
        }
        assert!(matches!(
            vm.step_debugger_module_root_instruction().unwrap(),
            VmDebuggerExecutionState::Paused { .. }
        ));
    }
    assert_eq!(
        preview,
        Some(VmDebuggerValuePreview::NumberBits(41.0_f64.to_bits()))
    );
}

fn preview_root_binding(
    vm: &Vm,
    program: &Bytecode,
    name: &str,
) -> Result<VmDebuggerValuePreview, RuntimeError> {
    let frame = &vm.debugger_stack_snapshot(None, 1, 256)?.frames[0];
    vm.debugger_value_preview(
        None,
        0,
        frame.code_unit_ordinal,
        frame.bytecode_offset,
        active_entry(frame, program, name),
    )
}

#[cfg_attr(test, test)]
fn paused_preview_copies_plain_records_and_sparse_arrays_without_handles() {
    let program = installed_script("let data = { a: 1, nested: [true, , 'ok'] };");
    let halt = program.instructions().last().unwrap().offset as u32;
    let mut vm = Vm::default();
    vm.execute_script_until_debugger_pause(&program, halt)
        .unwrap();
    assert_eq!(
        preview_root_binding(&vm, &program, "data").unwrap(),
        VmDebuggerValuePreview::Record(vec![
            (
                "a".into(),
                VmDebuggerValuePreview::NumberBits(1.0_f64.to_bits()),
            ),
            (
                "nested".into(),
                VmDebuggerValuePreview::Array(vec![
                    Some(VmDebuggerValuePreview::Bool(true)),
                    None,
                    Some(VmDebuggerValuePreview::StringUnits(vec![
                        b'o' as u16,
                        b'k' as u16,
                    ])),
                ]),
            ),
        ])
    );
    assert_eq!(
        preview_root_binding(&vm, &program, "data").unwrap(),
        preview_root_binding(&vm, &program, "data").unwrap()
    );
}

#[cfg_attr(test, test)]
fn paused_preview_refuses_accessors_proxies_cycles_and_non_plain_arrays() {
    let program = installed_script(
        "var getterRuns = 0; let accessor = { get x() { getterRuns++; return 1; } }; let proxy = new Proxy({ x: 1 }, { ownKeys() { getterRuns++; return ['x']; } }); let cycle = {}; cycle.self = cycle; let extra = [1]; extra.x = 2; let custom = Object.create({ inherited: 1 }); let symbolKey = { [Symbol('secret')]: 1 }; let typed = new Uint8Array([1]); let arrayGetter = [1]; Object.defineProperty(arrayGetter, '0', { get() { getterRuns++; return 1; } });",
    );
    let halt = program.instructions().last().unwrap().offset as u32;
    let mut vm = Vm::default();
    vm.execute_script_until_debugger_pause(&program, halt)
        .unwrap();
    for name in [
        "accessor",
        "proxy",
        "cycle",
        "extra",
        "custom",
        "symbolKey",
        "typed",
        "arrayGetter",
    ] {
        assert!(preview_root_binding(&vm, &program, name).is_err(), "{name}");
    }
    assert_eq!(
        vm.lookup_global_name("getterRuns").unwrap(),
        Some(Value::Number(0.0))
    );
}

#[cfg_attr(test, test)]
fn paused_preview_refuses_depth_length_node_and_aggregate_byte_excess() {
    let repeated_row = format!("[{}]", vec!["1"; 32].join(","));
    let source = format!(
        "let depth = [[[[[1]]]]]; let length = new Array(33); let nodes = [{}]; let bytes = {{ a: 'x'.repeat(1025), b: 'y'.repeat(1025) }}; let keyBytes = {{ ['z'.repeat(2049)]: 1 }};",
        vec![repeated_row; 9].join(",")
    );
    let program = installed_script(&source);
    let halt = program.instructions().last().unwrap().offset as u32;
    let mut vm = Vm::default();
    vm.execute_script_until_debugger_pause(&program, halt)
        .unwrap();
    for name in ["depth", "length", "nodes", "bytes", "keyBytes"] {
        assert!(preview_root_binding(&vm, &program, name).is_err(), "{name}");
    }
}

#[cfg_attr(test, test)]
fn pauses_a_direct_inner_call_without_consuming_its_parent_call_site() {
    let code = installed_script(
        "var before = 0; var kept = {value: 7}; function inner(arg) { before = before + 1; return arg.value; } inner(kept);",
    );
    let mut vm = Vm::default();
    let state = vm
        .execute_script_until_nested_debugger_pause(&code, 1, 0)
        .unwrap();
    assert_eq!(
        state,
        VmDebuggerNestedExecutionState::Paused {
            frame_serial: 1,
            code_unit_ordinal: 1,
            bytecode_offset: 0,
        }
    );
    assert_eq!(
        vm.lookup_global_name("before").unwrap(),
        Some(Value::Number(0.0))
    );
    let kept = vm
        .lookup_global_name("kept")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap();
    let child = vm.debugger_nested_continuation.as_ref().unwrap();
    assert_eq!(child.execution.arguments[0].object_id(), Some(kept));
    assert!(vm.debugger_continuation_references().contains(&kept));
    let parent = vm.debugger_continuation.as_ref().unwrap();
    assert_eq!(
        parent.code.instruction(parent.pc).unwrap().opcode,
        Opcode::Call
    );
    assert!(
        vm.stack.len() >= 3,
        "call inputs remain on the parent stack"
    );
    assert_eq!(
        vm.execute_script(&code),
        Err(RuntimeError::Unsupported(
            "a debugger-paused root script must resume before another execution starts"
        ))
    );
}

#[cfg_attr(test, test)]
fn nested_pause_rejects_a_deeper_call_before_the_target_executes() {
    let code = installed_script(
        "var reached = 0; function inner() { reached = reached + 1; } function outer() { inner(); } outer();",
    );
    let mut vm = Vm::default();
    assert_eq!(
        vm.execute_script_until_nested_debugger_pause(&code, 1, 0),
        Err(RuntimeError::Unsupported(
            "nested debugger pause requires a direct synchronous closure call"
        ))
    );
    assert_eq!(
        vm.lookup_global_name("reached").unwrap(),
        Some(Value::Number(0.0))
    );
    assert!(vm.debugger_nested_continuation.is_none());
    assert!(vm.debugger_continuation.is_none());
    assert!(vm.debugger_nested_pause_request.is_none());
}

#[cfg_attr(test, test)]
fn nested_pause_rejects_constructor_entry_before_its_body() {
    let code = installed_script("var reached = 0; function Inner() { reached = 1; } new Inner();");
    let mut vm = Vm::default();
    assert_eq!(
        vm.execute_script_until_nested_debugger_pause(&code, 1, 0),
        Err(RuntimeError::Unsupported(
            "nested debugger pause requires a direct synchronous closure call"
        ))
    );
    assert_eq!(
        vm.lookup_global_name("reached").unwrap(),
        Some(Value::Number(0.0))
    );
    assert!(vm.debugger_nested_continuation.is_none());
    assert!(vm.debugger_continuation.is_none());
    assert!(vm.debugger_nested_pause_request.is_none());
}

#[cfg_attr(test, test)]
fn nested_pause_does_not_capture_an_old_closure_with_the_same_ordinal() {
    let mut registry = BlueJsProgramRegistry::default();
    let first = registry
        .install(
            BlueJsSourceIdentity::new("page:///first.js", "sha256:first").unwrap(),
            &BlueJsProgramV1::Script(
                parse("globalThis.foreignHit = 0; globalThis.foreign = function(){globalThis.foreignHit = 1;};")
                    .unwrap(),
            ),
        )
        .unwrap();
    let second = registry
        .install(
            BlueJsSourceIdentity::new("page:///second.js", "sha256:second").unwrap(),
            &BlueJsProgramV1::Script(
                parse("function target(){globalThis.foreignHit = 99;} globalThis.foreign();")
                    .unwrap(),
            ),
        )
        .unwrap();
    let first_code = registry.get(first).unwrap().bytecode();
    let second_code = registry.get(second).unwrap().bytecode();
    assert_eq!(
        first_code
            .child_code_units()
            .next()
            .unwrap()
            .debugger_code_unit_ordinal,
        Some(1)
    );
    assert_eq!(
        second_code
            .child_code_units()
            .next()
            .unwrap()
            .debugger_code_unit_ordinal,
        Some(1)
    );
    assert_ne!(
        first_code.debugger_program_generation,
        second_code.debugger_program_generation
    );
    let mut vm = Vm::default();
    vm.execute_script(first_code).unwrap();
    assert_eq!(
        vm.execute_script_until_nested_debugger_pause(second_code, 1, 0)
            .unwrap(),
        VmDebuggerNestedExecutionState::Completed
    );
    assert_eq!(
        vm.lookup_global_name("foreignHit").unwrap(),
        Some(Value::Number(1.0))
    );
    assert!(vm.debugger_nested_continuation.is_none());
}

#[cfg_attr(test, test)]
fn nested_instruction_steps_rejoin_the_original_call_once() {
    let code = installed_script(
        "var calls = 0; var result = 0; function inner(){var i = 0; while (i < 2) { i++; } calls++; return i + 1;} result = inner() + 1;",
    );
    let safe_offsets = code
        .child_code_units()
        .next()
        .unwrap()
        .instructions()
        .map(|instruction| instruction.offset as u32)
        .collect::<std::collections::HashSet<_>>();
    let mut vm = Vm::default();
    let VmDebuggerNestedExecutionState::Paused {
        frame_serial,
        bytecode_offset: mut previous,
        ..
    } = vm
        .execute_script_until_nested_debugger_pause(&code, 1, 0)
        .unwrap()
    else {
        panic!("the direct inner call must pause before its first instruction");
    };
    assert_eq!(
        vm.step_debugger_nested_instruction(frame_serial + 1),
        Err(RuntimeError::Unsupported(
            "debugger nested frame invocation is stale"
        ))
    );
    let mut saw_backward_successor = false;
    let mut returned = None;
    for _ in 0..128 {
        match vm.step_debugger_nested_instruction(frame_serial).unwrap() {
            VmDebuggerNestedExecutionState::Paused {
                frame_serial: same_frame,
                code_unit_ordinal: 1,
                bytecode_offset,
            } => {
                assert_eq!(same_frame, frame_serial);
                assert!(safe_offsets.contains(&bytecode_offset));
                saw_backward_successor |= bytecode_offset < previous;
                previous = bytecode_offset;
            }
            VmDebuggerNestedExecutionState::FrameReturned {
                root_bytecode_offset,
            } => {
                returned = Some(root_bytecode_offset);
                break;
            }
            other => panic!("unexpected nested step state: {other:?}"),
        }
    }
    assert!(
        saw_backward_successor,
        "loop steps must report their real PC"
    );
    let root_successor = returned.expect("the child must return within the step budget");
    assert_eq!(
        vm.debugger_continuation.as_ref().unwrap().pc,
        root_successor as usize
    );
    assert!(vm.debugger_nested_continuation.is_none());
    assert_eq!(
        vm.step_debugger_nested_instruction(frame_serial),
        Err(RuntimeError::Unsupported(
            "no debugger-paused nested frame is available"
        ))
    );
    assert_eq!(
        vm.resume_debugger_execution().unwrap(),
        VmDebuggerExecutionState::Completed
    );
    assert_eq!(
        vm.lookup_global_name("calls").unwrap(),
        Some(Value::Number(1.0))
    );
    assert_eq!(
        vm.lookup_global_name("result").unwrap(),
        Some(Value::Number(4.0))
    );
}

#[cfg_attr(test, test)]
fn nested_resume_keeps_one_original_call_and_revokes_its_serial() {
    let code = installed_script(
        "var calls = 0; var result = 0; function inner(){ calls++; return 4; } result = inner() + 1;",
    );
    let mut vm = Vm::default();
    let VmDebuggerNestedExecutionState::Paused { frame_serial, .. } = vm
        .execute_script_until_nested_debugger_pause(&code, 1, 0)
        .unwrap()
    else {
        panic!("direct child must pause before its first instruction");
    };
    assert_eq!(
        vm.resume_debugger_nested_execution(frame_serial + 1),
        Err(RuntimeError::Unsupported(
            "debugger nested frame invocation is stale"
        ))
    );
    assert!(matches!(
        vm.step_debugger_nested_instruction(frame_serial),
        Ok(VmDebuggerNestedExecutionState::Paused { .. })
    ));
    let VmDebuggerNestedExecutionState::FrameReturned {
        root_bytecode_offset,
    } = vm.resume_debugger_nested_execution(frame_serial).unwrap()
    else {
        panic!("nested resume must rejoin the original root");
    };
    assert_eq!(
        vm.debugger_continuation.as_ref().unwrap().pc,
        root_bytecode_offset as usize
    );
    assert!(vm.debugger_nested_continuation.is_none());
    assert_eq!(
        vm.resume_debugger_nested_execution(frame_serial),
        Err(RuntimeError::Unsupported(
            "no debugger-paused nested frame is available"
        ))
    );
    assert_eq!(
        vm.resume_debugger_execution().unwrap(),
        VmDebuggerExecutionState::Completed
    );
    assert_eq!(
        vm.lookup_global_name("calls").unwrap(),
        Some(Value::Number(1.0))
    );
    assert_eq!(
        vm.lookup_global_name("result").unwrap(),
        Some(Value::Number(5.0))
    );
}

#[cfg_attr(test, test)]
fn nested_throw_enters_the_original_caller_catch() {
    let code = installed_script(
        "var caught = 0; function inner(){throw 7;} try { inner(); } catch (error) { caught = error; }",
    );
    let mut vm = Vm::default();
    let VmDebuggerNestedExecutionState::Paused { frame_serial, .. } = vm
        .execute_script_until_nested_debugger_pause(&code, 1, 0)
        .unwrap()
    else {
        panic!("the inner throw must be intercepted before execution");
    };
    let mut returned = false;
    for _ in 0..32 {
        match vm.step_debugger_nested_instruction(frame_serial).unwrap() {
            VmDebuggerNestedExecutionState::Paused { .. } => {}
            VmDebuggerNestedExecutionState::FrameReturned { .. } => {
                returned = true;
                break;
            }
            other => panic!("unexpected throw step state: {other:?}"),
        }
    }
    assert!(returned, "the thrown value must reach the waiting caller");
    assert!(vm.debugger_nested_continuation.is_none());
    assert_eq!(
        vm.resume_debugger_execution().unwrap(),
        VmDebuggerExecutionState::Completed
    );
    assert_eq!(vm.debugger_uncaught_throw_site(), None);
    assert_eq!(
        vm.lookup_global_name("caught").unwrap(),
        Some(Value::Number(7.0))
    );
}

#[cfg_attr(test, test)]
fn resumed_nested_throws_rejoin_a_handler_or_finish_the_root() {
    for caught in [false, true] {
        let source = if caught {
            "function child() {throw 7;} try {child();} catch (error) {globalThis.caught = error;}"
        } else {
            "function child() {throw 7;} child();"
        };
        let program = installed_script(source);
        let mut vm = Vm::default();
        let VmDebuggerNestedExecutionState::Paused { frame_serial, .. } = vm
            .execute_script_until_nested_debugger_pause(&program, 1, 0)
            .unwrap()
        else {
            panic!("child must pause before its throw");
        };
        let result = vm.resume_debugger_nested_execution(frame_serial);
        if caught {
            assert!(matches!(
                result,
                Ok(VmDebuggerNestedExecutionState::FrameReturned { .. })
            ));
            assert_eq!(
                vm.resume_debugger_execution(),
                Ok(VmDebuggerExecutionState::Completed)
            );
            assert_eq!(
                vm.lookup_global_name("caught"),
                Ok(Some(Value::Number(7.0)))
            );
        } else {
            assert_eq!(result, Err(RuntimeError::Thrown(Value::Number(7.0))));
            assert!(vm.debugger_continuation.is_none());
        }
        assert!(vm.debugger_nested_continuation.is_none());
        assert!(vm.stack.is_empty());
        assert_eq!(vm.execute_script(&code("21 + 21")), Ok(Value::Number(42.0)));
    }
}

#[cfg_attr(test, test)]
fn nested_step_keeps_the_waiting_callers_operand_alive_through_gc() {
    let code = installed_script(
        "function inner(){var scratch = {x: 1}; return 0;} var result = ({0: 7})[inner()];",
    );
    let mut config = VmConfig::default();
    config.heap.nursery_capacity = 1;
    let mut vm = Vm::new(config).unwrap();
    let VmDebuggerNestedExecutionState::Paused { frame_serial, .. } = vm
        .execute_script_until_nested_debugger_pause(&code, 1, 0)
        .unwrap()
    else {
        panic!("the computed-key call must pause in its child");
    };
    let mut returned = false;
    for _ in 0..64 {
        match vm.step_debugger_nested_instruction(frame_serial).unwrap() {
            VmDebuggerNestedExecutionState::Paused { .. } => {}
            VmDebuggerNestedExecutionState::FrameReturned { .. } => {
                returned = true;
                break;
            }
            other => panic!("unexpected GC step state: {other:?}"),
        }
    }
    assert!(returned);
    assert!(vm.debugger_nested_parent_execution.is_none());
    assert_eq!(
        vm.resume_debugger_execution().unwrap(),
        VmDebuggerExecutionState::Completed
    );
    assert_eq!(
        vm.lookup_global_name("result").unwrap(),
        Some(Value::Number(7.0))
    );
}

#[path = "debugger_nested_modules.rs"]
mod nested_modules;

#[cfg_attr(test, test)]
fn synchronous_debugger_exit_validation_preserves_return_pause_and_error_contracts() {
    let mut vm = Vm::default();
    let promise = vm.new_promise().unwrap();
    let root = vm.heap.root(promise).unwrap();
    for frame in [
        DebuggerFrameKind::Root,
        DebuggerFrameKind::NestedEntry,
        DebuggerFrameKind::NestedResume,
    ] {
        let completed = |result| match DebuggerInterpreterExit::classify(result, frame) {
            DebuggerInterpreterExit::Completed(result) => result,
            DebuggerInterpreterExit::Suspended { .. } => {
                panic!("completion was classified as a pause")
            }
        };
        assert_eq!(
            completed(Ok(InterpreterExit::Return(Value::Number(42.0)))),
            Ok(Value::Number(42.0))
        );
        let sentinel = RuntimeError::Thrown(Value::Number(7.0));
        assert_eq!(completed(Err(sentinel.clone())), Err(sentinel));
        let yield_error = match frame {
            DebuggerFrameKind::Root => {
                RuntimeError::TypeError("yield requires a generator function".into())
            }
            DebuggerFrameKind::NestedEntry => {
                RuntimeError::Unsupported("nested debugger target cannot yield or await")
            }
            DebuggerFrameKind::NestedResume => {
                RuntimeError::Unsupported("nested debugger frame yielded or awaited")
            }
        };
        assert_eq!(
            completed(Ok(InterpreterExit::Yield {
                value: Value::Undefined,
                pc: 0,
                iterators: Vec::new(),
                handlers: Vec::new()
            })),
            Err(yield_error)
        );
        let await_error = match frame {
            DebuggerFrameKind::Root => RuntimeError::Unsupported(
                "root debugger continuation cannot suspend for module await",
            ),
            DebuggerFrameKind::NestedEntry => {
                RuntimeError::Unsupported("nested debugger target cannot yield or await")
            }
            DebuggerFrameKind::NestedResume => {
                RuntimeError::Unsupported("nested debugger frame yielded or awaited")
            }
        };
        assert_eq!(
            completed(Ok(InterpreterExit::Await {
                promise,
                pc: 7,
                handlers: Vec::new()
            })),
            Err(await_error)
        );
        assert!(matches!(
            DebuggerInterpreterExit::classify(
                Ok(InterpreterExit::Suspend {
                    pc: 9,
                    iterators: Vec::new(),
                    handlers: Vec::new()
                }),
                frame
            ),
            DebuggerInterpreterExit::Suspended { pc: 9, .. }
        ));
    }
    vm.heap.unroot(root).unwrap();
}

impl crate::Vm {
    /// Runs debugger lifecycle contracts in unit and coverage builds.
    #[doc(hidden)]
    pub fn verify_debugger_lifecycle_contracts() {
        resumed_nested_throws_rejoin_a_handler_or_finish_the_root();
        module_pause_entry_failures_clear_requests_and_preserve_job_errors();
        skipped_root_targets_complete_and_owned_child_frames_block_new_entries();
        nested_module_target_validation_reports_host_graph_errors_before_execution();
        debugger_module_resume_handles_allocation_and_await_serial_exhaustion();
        a_terminal_module_pause_preserves_namespace_allocation_and_root_refusals();
        debugger_pauses_retain_active_handlers_and_clear_skipped_primitive_calls();
        nested_throw_protocol_retains_handlers_and_original_errors();
        nested_module_object_throws_preserve_identity_and_root_failure_cleans_both_frames();
        rejected_async_dependencies_prevent_root_and_linked_child_pauses();
        a_tighter_string_budget_at_a_real_pause_aborts_classic_and_module_frames_cleanly();
        nested_modules::verify_nested_module_contracts();
        synchronous_debugger_exit_validation_preserves_return_pause_and_error_contracts();
        uncaught_classic_throw_site_is_exact_and_caught_or_successor_sites_clear();
        nested_throw_origin_survives_caller_and_rethrow_records_catch_site();
        finally_throw_replaces_prior_site_even_when_the_replacement_is_nested();
        module_root_and_nested_throw_sites_keep_their_installed_code_units();
        captured_parent_binding_is_not_an_active_child_scope_slot();
        stack_snapshot_excludes_inactive_scopes_and_never_evaluates_a_getter();
        paused_root_previews_lossless_primitives_and_rejects_wrong_targets();
        paused_nested_preview_reads_cell_backed_capture_without_running_child();
        paused_module_root_preview_reads_retained_module_binding();
        paused_preview_copies_plain_records_and_sparse_arrays_without_handles();
        paused_preview_refuses_accessors_proxies_cycles_and_non_plain_arrays();
        paused_preview_refuses_depth_length_node_and_aggregate_byte_excess();
        pauses_a_direct_inner_call_without_consuming_its_parent_call_site();
        nested_pause_rejects_a_deeper_call_before_the_target_executes();
        nested_pause_rejects_constructor_entry_before_its_body();
        nested_pause_does_not_capture_an_old_closure_with_the_same_ordinal();
        nested_instruction_steps_rejoin_the_original_call_once();
        nested_resume_keeps_one_original_call_and_revokes_its_serial();
        nested_throw_enters_the_original_caller_catch();
        nested_step_keeps_the_waiting_callers_operand_alive_through_gc();
    }
}

#[cfg_attr(test, test)]
fn module_pause_entry_failures_clear_requests_and_preserve_job_errors() {
    let entry = "pause-errors/main.mjs";
    for nested in [false, true] {
        let entry_code = installed_module(
            entry,
            "import './dep.mjs'; function child() {return 42;} child();",
        );
        let offset = module_entry_offset(&entry_code);
        let mut vm = Vm::default();
        let incomplete = HashMap::from([(entry.to_string(), entry_code.clone())]);
        let failed = if nested {
            vm.execute_module_graph_until_nested_debugger_pause(entry, &incomplete, 1, 0)
                .map(|_| ())
        } else {
            vm.execute_module_graph_until_debugger_pause(entry, &incomplete, offset)
                .map(|_| ())
        };
        assert!(matches!(failed, Err(RuntimeError::ModuleResolution(_))));
        assert!(vm.debugger_module_pause_request.is_none());
        assert!(vm.debugger_nested_pause_request.is_none());
        assert_eq!(vm.execute_script(&code("21 + 21")), Ok(Value::Number(42.0)));

        let graph = HashMap::from([
            (entry.to_string(), entry_code),
            (
                "pause-errors/dep.mjs".to_string(),
                module_code("await new Promise(() => {}); export const value = 7;"),
            ),
        ]);
        let mut vm = Vm::default();
        // A real Promise species supplies a resolving function that throws.
        // Its queued reaction precedes a pending async module dependency.
        vm.execute_script(&code("var queued = Promise.resolve(1); queued.constructor = {[Symbol.species]: class Capability {constructor(executor) {executor(() => {throw 7;}, () => {throw 9;});}}}; queued.then();")).unwrap();
        assert!(!vm.promise_jobs.is_empty());
        let failed = if nested {
            vm.execute_module_graph_until_nested_debugger_pause(entry, &graph, 1, 0)
                .map(|_| ())
        } else {
            vm.execute_module_graph_until_debugger_pause(entry, &graph, offset)
                .map(|_| ())
        };
        assert_eq!(failed, Err(RuntimeError::Thrown(Value::Number(7.0))));
        assert!(vm.debugger_module_pause_request.is_none());
        assert!(vm.debugger_nested_pause_request.is_none());
        assert!(vm.debugger_module_continuation.is_none());
        assert!(vm.debugger_nested_continuation.is_none());
        assert_eq!(vm.execute_script(&code("21 + 21")), Ok(Value::Number(42.0)));
    }
}

#[cfg_attr(test, test)]
fn a_terminal_module_pause_preserves_namespace_allocation_and_root_refusals() {
    let entry = "terminal/main.mjs";
    let program = installed_module(entry, "export const answer = 42;");
    let offset = program
        .instructions()
        .find(|instruction| instruction.opcode == Opcode::Halt)
        .unwrap()
        .offset as u32;
    let entry_offset = module_entry_offset(&program);
    let graph = HashMap::from([(entry.into(), program)]);
    for root_refusal in [false, true] {
        let mut vm = Vm::default();
        assert_eq!(
            vm.execute_module_graph_until_debugger_pause(entry, &graph, entry_offset),
            Ok(VmDebuggerExecutionState::Paused {
                bytecode_offset: entry_offset
            })
        );
        let mut current = entry_offset;
        while current != offset {
            let VmDebuggerExecutionState::Paused { bytecode_offset } =
                vm.step_debugger_module_root_instruction().unwrap()
            else {
                panic!("the entry must remain paused before its terminal Halt");
            };
            current = bytecode_offset;
        }
        assert!(!vm.module_namespace_cache.contains_key(entry));
        let expected = if root_refusal {
            vm.heap.allow_root_registrations(0);
            RuntimeError::Heap(HeapError::IdExhausted)
        } else {
            let limit = vm.heap.allow_only(0);
            RuntimeError::Heap(HeapError::HeapLimitExceeded { limit })
        };
        assert_eq!(vm.resume_debugger_module_execution(), Err(expected));
        assert!(
            vm.module_graph.is_some()
                && vm.debugger_module_continuation.is_none()
                && vm.stack.is_empty()
        );
        vm.heap.allow_only(16 * 1024 * 1024);
        assert_eq!(vm.execute_script(&code("21 + 21")), Ok(Value::Number(42.0)));
    }
}

#[cfg_attr(test, test)]
fn debugger_pauses_retain_active_handlers_and_clear_skipped_primitive_calls() {
    let program = installed_script("try {throw 7;} catch(error) {42;}");
    let offset = program
        .instructions()
        .find(|instruction| instruction.opcode == Opcode::Throw)
        .unwrap()
        .offset as u32;
    let mut vm = Vm::default();
    assert_eq!(
        vm.execute_script_until_debugger_pause(&program, offset),
        Ok(VmDebuggerExecutionState::Paused {
            bytecode_offset: offset
        })
    );
    assert_eq!(
        vm.resume_debugger_execution(),
        Ok(VmDebuggerExecutionState::Completed)
    );
    assert!(vm.stack.is_empty() && vm.debugger_continuation.is_none());
    let program =
        installed_script("var callable = 7; callable(); function child() {return 42;} child();");
    assert!(matches!(
        vm.execute_script_until_nested_debugger_pause(&program, 1, 0),
        Err(RuntimeError::TypeError(_))
    ));
    assert!(
        vm.stack.is_empty()
            && vm.debugger_nested_pause_request.is_none()
            && vm.debugger_nested_continuation.is_none()
    );
    assert_eq!(vm.execute_script(&code("21 + 21")), Ok(Value::Number(42.0)));
}
