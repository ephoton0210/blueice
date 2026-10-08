// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Completion and failure before, during and after exact nested debugger pauses.

use blueice_bluejs::{
    compile, parse, parse_module, BlueJsPageDebuggerLinkedExecutionState,
    BlueJsPageDebuggerNestedExecutionState, BlueJsPageDebuggerValueTarget, BlueJsPageOrigin,
    BlueJsPageRuntime, BlueJsPageRuntimeError, BlueJsProgramHandle, BlueJsProgramRegistry,
    BlueJsProgramV1, BlueJsSafePoint, BlueJsSourceIdentity, Bytecode, RuntimeError, Value, Vm,
    VmConfig, VmDebuggerNestedExecutionState, VmDebuggerScopeEntry,
};

fn origin() -> BlueJsPageOrigin {
    BlueJsPageOrigin::new("https://example.test").unwrap()
}

fn install(
    runtime: &mut BlueJsPageRuntime,
    name: &str,
    source: &str,
    module: bool,
) -> BlueJsProgramHandle {
    let program = if module {
        BlueJsProgramV1::Module(parse_module(source).unwrap())
    } else {
        BlueJsProgramV1::Script(parse(source).unwrap())
    };
    runtime
        .install_program(
            7,
            &origin(),
            BlueJsSourceIdentity::new(name, format!("sha256:{name}")).unwrap(),
            &program,
        )
        .unwrap()
}

fn child_point(
    runtime: &BlueJsPageRuntime,
    program: BlueJsProgramHandle,
    last: bool,
) -> BlueJsSafePoint {
    let points = runtime.safe_points(7, program, 1024).unwrap();
    let mut children = points
        .into_iter()
        .filter(|point| point.code_unit.ordinal() == 1);
    if last {
        children.next_back().unwrap()
    } else {
        children.next().unwrap()
    }
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

#[test]
fn closed_page_realms_reject_every_debugger_entry_before_accessing_programs() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    let program = install(
        &mut runtime,
        "page:///closed.js",
        "function child() { return 1; } child();",
        false,
    );
    let point = child_point(&runtime, program, false);
    let module = install(
        &mut runtime,
        "page:///closed-module.mjs",
        "function child() { return 42; } export const value = child();",
        true,
    );
    let module_point = runtime.module_evaluate_entry_safe_point(7, module).unwrap();
    let module_child_point = child_point(&runtime, module, false);
    runtime.close_realm(7);
    let error = BlueJsPageRuntimeError::UnknownRealm(7);
    assert_eq!(
        runtime.module_evaluate_entry_safe_point(7, module),
        Err(error.clone())
    );
    assert_eq!(
        runtime.execute_program_until_nested_debugger_pause(7, program, point),
        Err(error.clone())
    );
    assert_eq!(
        runtime.validate_linked_nested_debugger_target(
            7,
            module,
            program,
            [module, program],
            point
        ),
        Err(error.clone())
    );
    assert_eq!(
        runtime.execute_module_graph_until_debugger_pause(7, module, [module], module_point),
        Err(error.clone())
    );
    assert_eq!(
        runtime.execute_module_graph_until_nested_debugger_pause(
            7,
            module,
            [module],
            module_child_point
        ),
        Err(error.clone())
    );
    assert_eq!(
        runtime.debugger_uncaught_throw_site(7, program),
        Err(error.clone())
    );
    assert_eq!(runtime.resume_debugger_execution(7), Err(error.clone()));
    assert_eq!(
        runtime.step_debugger_root_instruction(7),
        Err(error.clone())
    );
    assert_eq!(
        runtime.resume_debugger_module_execution(7),
        Err(error.clone())
    );
    assert_eq!(
        runtime.step_debugger_module_root_instruction(7),
        Err(error.clone())
    );
    assert_eq!(
        runtime.run_click_microtask_checkpoint(7),
        Err(error.clone())
    );
    assert_eq!(
        runtime.debugger_value_preview(
            7,
            program,
            None,
            BlueJsPageDebuggerValueTarget {
                frame_index: 0,
                code_unit_ordinal: 1,
                bytecode_offset: point.bytecode_offset,
                scope_entry: VmDebuggerScopeEntry {
                    slot_ordinal: 0,
                    scope_depth: 0
                },
            }
        ),
        Err(error)
    );
}

#[test]
fn failures_before_and_after_a_nested_pause_preserve_the_uncaught_value() {
    for module in [false, true] {
        for before_pause in [false, true] {
            let mut runtime = BlueJsPageRuntime::default();
            runtime.open_realm(7, origin()).unwrap();
            let program = install(
                &mut runtime,
                "page:///uncaught-child.js",
                "function child() { throw 7; } child();",
                module,
            );
            let point = child_point(&runtime, program, before_pause);
            let result = if module {
                runtime.execute_module_graph_until_nested_debugger_pause(
                    7,
                    program,
                    [program],
                    point,
                )
            } else {
                runtime.execute_program_until_nested_debugger_pause(7, program, point)
            };
            let error = BlueJsPageRuntimeError::Runtime(RuntimeError::Thrown(Value::Number(7.0)));
            if before_pause {
                assert_eq!(result, Err(error));
            } else {
                let BlueJsPageDebuggerNestedExecutionState::Paused { frame, .. } = result.unwrap()
                else {
                    panic!("the child did not pause before its throw")
                };
                assert_eq!(runtime.resume_debugger_nested_execution(frame), Err(error));
                assert_eq!(
                    runtime.resume_debugger_nested_execution(frame),
                    Err(BlueJsPageRuntimeError::DebuggerNestedFrameUnavailable)
                );
            }
            let reader = install(&mut runtime, "page:///uncaught-reuse.js", "21 + 21", false);
            assert_eq!(runtime.execute_program(7, reader), Ok(Value::Number(42.0)));
        }
    }
}

#[test]
fn module_debugger_requests_reject_classic_entries_and_already_evaluated_modules() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    let script = install(&mut runtime, "page:///classic-entry.js", "42;", false);
    let point = runtime.safe_points(7, script, 64).unwrap()[0];
    assert_eq!(
        runtime.execute_module_graph_until_debugger_pause(7, script, [script], point),
        Err(BlueJsPageRuntimeError::ProgramShape)
    );
    let module = install(
        &mut runtime,
        "page:///evaluated-entry.mjs",
        "export const answer = 42;",
        true,
    );
    let point = runtime.module_evaluate_entry_safe_point(7, module).unwrap();
    runtime.execute_module_graph(7, module, [module]).unwrap();
    assert_eq!(
        runtime.execute_module_graph_until_debugger_pause(7, module, [module], point),
        Err(BlueJsPageRuntimeError::Runtime(RuntimeError::Unsupported(
            "debugger module entry did not reach the requested safe point"
        )))
    );
}

#[test]
fn an_unresolved_top_level_await_cannot_reach_a_nested_debugger_target() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    let module = install(
        &mut runtime,
        "page:///pending-entry.mjs",
        "function child() {return 42;} await new Promise(() => {}); child();",
        true,
    );
    let point = child_point(&runtime, module, false);
    assert_eq!(
        runtime.execute_module_graph_until_nested_debugger_pause(7, module, [module], point),
        Err(BlueJsPageRuntimeError::Runtime(RuntimeError::Unsupported(
            "nested debugger module target was not reached"
        )))
    );
    let reader = install(&mut runtime, "page:///pending-reuse.js", "21 + 21", false);
    assert_eq!(runtime.execute_program(7, reader), Ok(Value::Number(42.0)));
}

#[test]
fn closed_realms_revoke_nested_and_linked_frames_and_their_snapshots() {
    for linked in [false, true] {
        let mut runtime = BlueJsPageRuntime::default();
        runtime.open_realm(7, origin()).unwrap();
        if linked {
            let dependency = install(
                &mut runtime,
                "page:///close-dep.mjs",
                "export function child() { let local = 1; return local; }",
                true,
            );
            let entry = install(
                &mut runtime,
                "page:///close-entry.mjs",
                "import {child} from './close-dep.mjs'; child();",
                true,
            );
            let point = child_point(&runtime, dependency, false);
            let BlueJsPageDebuggerLinkedExecutionState::Paused { frame, .. } = runtime
                .execute_module_graph_until_linked_nested_debugger_pause(
                    7,
                    entry,
                    dependency,
                    [dependency, entry],
                    point,
                )
                .unwrap()
            else {
                panic!("linked child did not pause")
            };
            let snapshot = runtime
                .debugger_linked_stack_snapshot(7, frame, 2, 256)
                .unwrap();
            runtime.close_realm(7);
            assert_eq!(
                runtime.resume_debugger_linked_nested_execution(frame),
                Err(BlueJsPageRuntimeError::UnknownRealm(7))
            );
            assert_eq!(
                runtime.debugger_linked_stack_snapshot(7, frame, 2, 256),
                Err(BlueJsPageRuntimeError::UnknownRealm(7))
            );
            assert_eq!(
                runtime.debugger_linked_value_preview(
                    7,
                    frame,
                    &snapshot,
                    VmDebuggerScopeEntry {
                        slot_ordinal: 0,
                        scope_depth: 0
                    }
                ),
                Err(BlueJsPageRuntimeError::UnknownRealm(7))
            );
        } else {
            let program = install(
                &mut runtime,
                "page:///close-child.js",
                "function child() { let local = 1; return local; } child();",
                false,
            );
            let point = child_point(&runtime, program, false);
            let BlueJsPageDebuggerNestedExecutionState::Paused { frame, .. } = runtime
                .execute_program_until_nested_debugger_pause(7, program, point)
                .unwrap()
            else {
                panic!("child did not pause")
            };
            runtime.close_realm(7);
            assert_eq!(
                runtime.resume_debugger_nested_execution(frame),
                Err(BlueJsPageRuntimeError::UnknownRealm(7))
            );
            assert_eq!(
                runtime.step_debugger_nested_instruction(frame),
                Err(BlueJsPageRuntimeError::UnknownRealm(7))
            );
        }
    }
}

#[test]
fn debugger_module_pause_requires_the_first_evaluation_instruction() {
    use blueice_bluejs::compile_module;
    use std::collections::HashMap;
    let code = compile_module(&parse_module("export const value = 42;").unwrap()).unwrap();
    let mut graph = HashMap::new();
    graph.insert("entry".to_string(), code);
    let mut vm = Vm::default();
    assert!(matches!(
        vm.execute_module_graph_until_debugger_pause("entry", &graph, u32::MAX),
        Err(RuntimeError::Unsupported(_))
    ));
    assert!(matches!(
        vm.execute_module_graph_until_debugger_pause("absent", &graph, 0),
        Err(RuntimeError::Unsupported(_))
    ));
    graph.insert("script".to_string(), compile(&parse("1").unwrap()).unwrap());
    assert!(matches!(
        vm.execute_module_graph_until_debugger_pause("script", &graph, 0),
        Err(RuntimeError::Unsupported(_))
    ));
    assert_eq!(
        vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
        Ok(Value::Number(42.0))
    );
}

#[test]
fn a_child_return_before_the_requested_pause_completes_the_page_execution() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    let program = install(
        &mut runtime,
        "page:///early.js",
        "function child() { if (globalThis.skip) return 42; return 7; } globalThis.skip = true; globalThis.answer = child();",
        false,
    );
    let point = child_point(&runtime, program, true);
    assert_eq!(
        runtime.execute_program_until_nested_debugger_pause(7, program, point),
        Ok(BlueJsPageDebuggerNestedExecutionState::Completed)
    );
    let reader = install(&mut runtime, "page:///reader.js", "answer", false);
    assert_eq!(runtime.execute_program(7, reader), Ok(Value::Number(42.0)));
}

#[test]
fn a_child_throw_before_the_requested_pause_closes_its_iterator_and_parent() {
    let code = installed_script(
        "function child() { for (var value of iterable) { if (value) throw sentinel; } return 7; } globalThis.closed = 0; var sentinel = {}; var iterable = {[Symbol.iterator]() { return {next() {return {done:false,value:1}}, return() {closed++; return {}}}; }}; for (var parent of iterable) { try { child(); } catch (error) { if (error !== sentinel) throw error; break; } }",
    );
    let offset = code
        .child_code_units()
        .next()
        .unwrap()
        .instructions()
        .last()
        .unwrap()
        .offset as u32;
    let mut vm = Vm::default();
    assert_eq!(
        vm.execute_script_until_nested_debugger_pause(&code, 1, offset),
        Ok(VmDebuggerNestedExecutionState::Completed)
    );
    assert_eq!(
        vm.execute_script(&compile(&parse("closed").unwrap()).unwrap())
            .unwrap(),
        Value::Number(2.0)
    );
}

#[test]
fn failed_global_declarations_clean_up_both_root_and_nested_pause_requests() {
    for nested in [false, true] {
        let mut vm = Vm::default();
        vm.execute_script(
            &compile(&parse("Object.preventExtensions(globalThis)").unwrap()).unwrap(),
        )
        .unwrap();
        let code = installed_script("function unavailable() { return 7; } unavailable();");
        let result = if nested {
            vm.execute_script_until_nested_debugger_pause(&code, 1, 0)
                .map(|_| ())
        } else {
            vm.execute_script_until_debugger_pause(&code, 0).map(|_| ())
        };
        assert!(matches!(result, Err(RuntimeError::TypeError(_))));
        assert_eq!(
            vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap())
                .unwrap(),
            Value::Number(42.0)
        );
    }
}

#[test]
fn a_linked_child_return_before_its_pause_completes_the_graph() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    let dependency = install(
        &mut runtime,
        "page:///dep.mjs",
        "export function child() { if (globalThis.skip) return 42; return 7; }",
        true,
    );
    let entry = install(
        &mut runtime,
        "page:///entry.mjs",
        "import {child} from './dep.mjs'; globalThis.skip = true; globalThis.answer = child();",
        true,
    );
    let point = child_point(&runtime, dependency, true);
    assert_eq!(
        runtime.execute_module_graph_until_linked_nested_debugger_pause(
            7,
            entry,
            dependency,
            [dependency, entry],
            point,
        ),
        Ok(BlueJsPageDebuggerLinkedExecutionState::Completed)
    );
    let reader = install(&mut runtime, "page:///reader.js", "answer", false);
    assert_eq!(runtime.execute_program(7, reader), Ok(Value::Number(42.0)));
}

#[test]
fn a_failed_linked_child_revokes_its_frame_and_preserves_the_module_error() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    let dependency = install(
        &mut runtime,
        "page:///dep.mjs",
        "export function child() { throw 7; }",
        true,
    );
    let entry = install(
        &mut runtime,
        "page:///entry.mjs",
        "import {child} from './dep.mjs'; child();",
        true,
    );
    let point = child_point(&runtime, dependency, false);
    let state = runtime
        .execute_module_graph_until_linked_nested_debugger_pause(
            7,
            entry,
            dependency,
            [dependency, entry],
            point,
        )
        .unwrap();
    let BlueJsPageDebuggerLinkedExecutionState::Paused { frame, .. } = state else {
        panic!("expected the linked child to pause");
    };
    assert_eq!(frame.tab_id(), 7);
    assert_eq!(frame.entry_program(), entry);
    assert_eq!(frame.dependency_program(), dependency);
    assert_eq!(frame.code_unit_ordinal(), 1);
    assert_ne!(frame.invocation_serial(), 0);
    assert_eq!(
        runtime.resume_debugger_linked_nested_execution(frame),
        Err(BlueJsPageRuntimeError::Runtime(RuntimeError::Thrown(
            Value::Number(7.0)
        )))
    );
    assert_eq!(
        runtime.resume_debugger_linked_nested_execution(frame),
        Err(BlueJsPageRuntimeError::DebuggerNestedFrameUnavailable)
    );
    assert_eq!(
        runtime.execute_module_graph(7, entry, [dependency, entry]),
        Err(BlueJsPageRuntimeError::Runtime(RuntimeError::Thrown(
            Value::Number(7.0)
        )))
    );
}

#[test]
fn linked_nested_targets_reject_root_code_units_before_starting_execution() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    let entry = install(
        &mut runtime,
        "page:///entry.mjs",
        "export const v = 1",
        true,
    );
    let point = runtime.module_evaluate_entry_safe_point(7, entry).unwrap();
    assert_eq!(
        runtime.validate_linked_nested_debugger_target(7, entry, entry, [entry], point),
        Err(BlueJsPageRuntimeError::DebuggerNestedCodeUnitOnly)
    );
    assert_eq!(
        runtime.execute_module_graph_until_linked_nested_debugger_pause(
            7,
            entry,
            entry,
            [entry],
            point,
        ),
        Err(BlueJsPageRuntimeError::DebuggerNestedCodeUnitOnly)
    );
}

#[test]
fn every_module_debugger_entry_rejects_an_unadmitted_graph_and_leaves_the_realm_reusable() {
    for mode in 0..4 {
        let mut runtime = BlueJsPageRuntime::default();
        runtime.open_realm(7, origin()).unwrap();
        runtime.open_realm(8, origin()).unwrap();
        let dependency = install(
            &mut runtime,
            "page:///admit-dep.mjs",
            "export function child() {return 42;}",
            true,
        );
        let entry = install(
            &mut runtime,
            "page:///admit-main.mjs",
            "import {child} from './admit-dep.mjs'; child();",
            true,
        );
        let classic = install(&mut runtime, "page:///admit-classic.js", "42", false);
        let other = runtime
            .install_program(
                8,
                &origin(),
                BlueJsSourceIdentity::new("page:///other.mjs", "sha256:other").unwrap(),
                &BlueJsProgramV1::Module(parse_module("export const value = 42;").unwrap()),
            )
            .unwrap();
        let root_point = runtime.module_evaluate_entry_safe_point(7, entry).unwrap();
        let point = child_point(&runtime, dependency, false);
        for graph in [
            Vec::new(),
            vec![entry, entry],
            vec![entry, classic],
            vec![entry, other],
        ] {
            let result = match mode {
                0 => runtime
                    .execute_module_graph_until_debugger_pause(7, entry, graph, root_point)
                    .map(|_| ()),
                1 => runtime
                    .execute_module_graph_until_nested_debugger_pause(7, dependency, graph, point)
                    .map(|_| ()),
                2 => runtime
                    .execute_module_graph_until_linked_nested_debugger_pause(
                        7, entry, dependency, graph, point,
                    )
                    .map(|_| ()),
                _ => runtime
                    .validate_linked_nested_debugger_target(7, entry, dependency, graph, point),
            };
            assert!(matches!(
                result,
                Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm { .. }
                    | BlueJsPageRuntimeError::DuplicateModuleIdentity(_)
                    | BlueJsPageRuntimeError::ProgramShape)
            ));
        }
        let reader = install(&mut runtime, "page:///admit-reuse.js", "21 + 21", false);
        assert_eq!(runtime.execute_program(7, reader), Ok(Value::Number(42.0)));
        assert_eq!(
            runtime.module_evaluate_entry_safe_point(7, classic),
            Err(BlueJsPageRuntimeError::ProgramShape)
        );
    }
}

#[test]
fn nested_debugger_requests_reject_uninstalled_units_and_eval_before_entering_the_body() {
    for (source, installed) in [
        ("function child() {return 42;} child();", false),
        ("function child() {return eval('42');} child();", true),
        ("function child() {return eval(...['42']);} child();", true),
    ] {
        let code = if installed {
            installed_script(source)
        } else {
            compile(&parse(source).unwrap()).unwrap()
        };
        for (ordinal, offset) in [(0, 0), (1, u32::MAX), (u32::MAX, 0), (1, 0)] {
            let mut vm = Vm::default();
            assert!(matches!(
                vm.execute_script_until_nested_debugger_pause(&code, ordinal, offset),
                Err(RuntimeError::Unsupported(_))
            ));
            assert_eq!(
                vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
                Ok(Value::Number(42.0))
            );
        }
    }
}

#[test]
fn a_resource_abort_before_the_nested_pause_releases_the_request_without_renewing_fuel() {
    let code = installed_script(
        "function child() { for (var value of iterable) { while (true) {} } return 7; } globalThis.closed = 0; var iterable = {[Symbol.iterator]() { return {next() {return {done:false,value:1}}, return() {closed++; return {done:true}}}; }}; child();",
    );
    let offset = code
        .child_code_units()
        .next()
        .unwrap()
        .instructions()
        .last()
        .unwrap()
        .offset as u32;
    let config = VmConfig {
        instruction_budget: 2000,
        ..VmConfig::default()
    };
    let mut vm = Vm::new(config).unwrap();
    assert!(matches!(
        vm.execute_script_until_nested_debugger_pause(&code, 1, offset),
        Err(RuntimeError::InstructionLimit)
    ));
    assert!(matches!(
        vm.resume_debugger_nested_execution(1),
        Err(RuntimeError::Unsupported(_))
    ));
    // IteratorClose marks the record done, but invoking its return callback
    // also requires fuel. Cleanup must preserve the resource abort and must
    // not grant that callback a new instruction budget.
    assert_eq!(
        vm.execute_script(&compile(&parse("closed + 42").unwrap()).unwrap()),
        Ok(Value::Number(42.0))
    );
}

#[test]
fn an_object_throw_before_the_nested_pause_closes_active_iterators_once() {
    let source = r#"
        function child() {for (var value of iterable) {throw thrown;}}
        globalThis.closed = 0;
        globalThis.thrown = {answer: 42};
        var iterable = {[Symbol.iterator]() {return {
            next() {return {value: 1, done: false};},
            return() {closed++; var allocation = [{}, {}, {}]; return {done: true};}
        };}};
        child();
    "#;
    let code = installed_script(source);
    let child = code.child_code_units().next().unwrap();
    // Select the child's final return, which the abrupt loop cannot reach.
    let offset = child.instructions().last().unwrap().offset as u32;
    let mut vm = Vm::default();
    let error = vm
        .execute_script_until_nested_debugger_pause(&code, 1, offset)
        .unwrap_err();
    let RuntimeError::Thrown(Value::Object(thrown)) = error else {
        panic!("expected the original object-valued throw: {error:?}");
    };
    assert!(matches!(
        vm.resume_debugger_nested_execution(1),
        Err(RuntimeError::Unsupported(_))
    ));
    assert_eq!(
        vm.execute_script(&compile(&parse("thrown").unwrap()).unwrap()),
        Ok(Value::Object(thrown))
    );
    assert_eq!(
        vm.execute_script(&compile(&parse("closed").unwrap()).unwrap()),
        Ok(Value::Number(1.0))
    );
    assert_eq!(
        vm.execute_script(&compile(&parse("closed + thrown.answer").unwrap()).unwrap()),
        Ok(Value::Number(43.0))
    );
}
