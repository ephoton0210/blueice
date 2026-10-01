// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Completion and failure before, during and after exact nested debugger pauses.

use blueice_bluejs::{
    compile, parse, parse_module, BlueJsPageDebuggerLinkedExecutionState,
    BlueJsPageDebuggerNestedExecutionState, BlueJsPageOrigin, BlueJsPageRuntime,
    BlueJsPageRuntimeError, BlueJsProgramHandle, BlueJsProgramRegistry, BlueJsProgramV1,
    BlueJsSafePoint, BlueJsSourceIdentity, Bytecode, RuntimeError, Value, Vm,
    VmDebuggerNestedExecutionState,
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
