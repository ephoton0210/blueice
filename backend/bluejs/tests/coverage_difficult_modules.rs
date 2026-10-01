// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Namespace cycles, deferred graphs and module debugger completion cleanup.

use blueice_bluejs::{
    compile, compile_module, parse, parse_module, BlueJsPageDebuggerExecutionState,
    BlueJsPageOrigin, BlueJsPageRuntime, BlueJsPageRuntimeError, BlueJsProgramV1,
    BlueJsSourceIdentity, Bytecode, RuntimeError, Value, Vm, VmConfig,
};
use std::collections::HashMap;

fn modules(sources: &[(&str, &str)]) -> HashMap<String, Bytecode> {
    sources
        .iter()
        .map(|(name, source)| {
            (
                format!("graph/{name}"),
                compile_module(&parse_module(source).unwrap()).unwrap(),
            )
        })
        .collect()
}

fn graph_true(sources: &[(&str, &str)]) {
    for nursery_capacity in [1, VmConfig::default().heap.nursery_capacity] {
        let mut config = VmConfig::default();
        config.heap.nursery_capacity = nursery_capacity;
        let mut vm = Vm::new(config).unwrap();
        assert_eq!(
            vm.execute_module_graph("graph/main.mjs", &modules(sources))
                .unwrap(),
            Value::Bool(true),
            "{sources:?}"
        );
    }
}

#[test]
fn namespaces_reuse_identity_across_star_cycles_and_omit_ambiguous_exports() {
    graph_true(&[
        ("main.mjs", "import * as ns from './hub.mjs'; import {self} from './hub.mjs'; ns === self && !('conflict' in ns) && ns.first === 1 && ns.second === 2"),
        ("hub.mjs", "export * from './a.mjs'; export * from './b.mjs'; export * as self from './hub.mjs';"),
        ("a.mjs", "export const conflict = 1; export const first = 1; export * from './cycle.mjs';"),
        ("b.mjs", "export const conflict = 2; export const second = 2;"),
        ("cycle.mjs", "export * from './hub.mjs';"),
    ]);
}

#[test]
fn ambiguity_propagates_through_intermediate_star_exports() {
    let graph = modules(&[
        ("main.mjs", "import {conflict} from './outer.mjs'; conflict"),
        ("outer.mjs", "export * from './hub.mjs';"),
        (
            "hub.mjs",
            "export * from './a.mjs'; export * from './b.mjs';",
        ),
        ("a.mjs", "export const conflict = 1;"),
        ("b.mjs", "export const conflict = 2;"),
    ]);
    let mut vm = Vm::default();
    assert!(matches!(
        vm.execute_module_graph("graph/main.mjs", &graph),
        Err(RuntimeError::ModuleResolution(_))
    ));
    assert_eq!(
        vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap())
            .unwrap(),
        Value::Number(42.0)
    );
}

#[test]
fn deferred_namespace_identity_is_cached_and_async_dependencies_run_once() {
    graph_true(&[
        ("main.mjs", "const first = await import.defer('./dep.mjs'); const second = await import.defer('./dep.mjs'); const before = globalThis.bodyRuns; const value = first.value; first === second && before === undefined && value === 42 && globalThis.bodyRuns === 1 && globalThis.asyncRuns === 1"),
        ("dep.mjs", "import './async.mjs'; globalThis.bodyRuns = (globalThis.bodyRuns || 0) + 1; export const value = 42;"),
        ("async.mjs", "await Promise.resolve(); globalThis.asyncRuns = (globalThis.asyncRuns || 0) + 1;"),
    ]);
}

#[test]
fn deferred_namespace_replays_evaluation_error_without_repeating_side_effects() {
    graph_true(&[
        ("main.mjs", "const ns = await import.defer('./dep.mjs'); var first, second; try {ns.value;} catch (e) {first = e;} try {ns.value;} catch (e) {second = e;} first === 7 && second === first && globalThis.bodyRuns === 1"),
        ("dep.mjs", "globalThis.bodyRuns = (globalThis.bodyRuns || 0) + 1; throw 7; export const value = 42;"),
    ]);
}

#[test]
fn source_import_without_a_referrer_uses_the_classic_script_host_context() {
    let mut vm = Vm::default();
    vm.install_test262_done().unwrap();
    vm.set_module_source_loader_context(vec!["mod.wasm".to_string()]);
    let source = "Promise.all([import.source('./mod.wasm'), import.source('./mod.wasm')]).then(values => { if (values[0] !== values[1]) throw new Error('source identity changed'); $DONE(); }, $DONE);";
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap();
    vm.run_promise_jobs().unwrap();
    assert_eq!(vm.take_test262_done(), Some(Ok(())));
}

#[test]
fn paused_module_rejection_preserves_the_reason_and_allows_a_later_script() {
    let origin = BlueJsPageOrigin::new("https://example.test").unwrap();
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(7, origin.clone()).unwrap();
    let handle = runtime
        .install_program(
            7,
            &origin,
            BlueJsSourceIdentity::new("graph/main.mjs", "sha256:await-throw").unwrap(),
            &BlueJsProgramV1::Module(parse_module("await Promise.reject(7)").unwrap()),
        )
        .unwrap();
    let point = runtime.module_evaluate_entry_safe_point(7, handle).unwrap();
    assert_eq!(
        runtime.execute_module_graph_until_debugger_pause(7, handle, [handle], point),
        Ok(BlueJsPageDebuggerExecutionState::Paused {
            bytecode_offset: point.bytecode_offset
        })
    );
    assert_eq!(
        runtime.resume_debugger_module_execution(7),
        Err(BlueJsPageRuntimeError::Runtime(RuntimeError::Thrown(
            Value::Number(7.0)
        )))
    );
    let reader = runtime
        .install_program(
            7,
            &origin,
            BlueJsSourceIdentity::new("graph/reader.js", "sha256:reader").unwrap(),
            &BlueJsProgramV1::Script(parse("21 + 21").unwrap()),
        )
        .unwrap();
    assert_eq!(runtime.execute_program(7, reader), Ok(Value::Number(42.0)));
}
