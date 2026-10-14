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
fn error_headers_propagate_uninitialized_namespace_fields() {
    graph_true(&[
        ("main.mjs", "import './observe.mjs'; export let name = 'ready'; export let message = 'ready'; globalThis.checked === 2"),
        ("observe.mjs", r#"
            import * as pending from './main.mjs';
            var getter = Object.getOwnPropertyDescriptor(Error.prototype, 'stack').get;
            globalThis.checked = 0;
            for (var ownName of [false, true]) {
                var error = new Error();
                if (ownName) Object.defineProperty(error, 'name', {value:'own'});
                Object.setPrototypeOf(error, pending);
                try {getter.call(error); throw 'accepted uninitialized header';}
                catch(error) {if (!(error instanceof ReferenceError)) throw error; checked++;}
            }
        "#),
    ]);
}

#[test]
fn proxy_invariant_checks_preserve_namespace_tdz_errors() {
    graph_true(&[
        (
            "main.mjs",
            r#"
            import './observe.mjs';
            export const value = 42;
            globalThis.checked === 7
        "#,
        ),
        (
            "observe.mjs",
            r#"
            import * as ns from './main.mjs';
            globalThis.checked = 0;
            for (const operation of [
                () => new Proxy(ns, {get() {return 7;}}).value,
                () => 'value' in new Proxy(ns, {has() {return false;}}),
                () => Reflect.set(new Proxy(ns, {set() {return true;}}), 'value', 7),
                () => Reflect.deleteProperty(new Proxy(ns, {deleteProperty() {return true;}}), 'value'),
                () => Reflect.ownKeys(new Proxy(ns, {ownKeys() {return ['value'];}})),
                () => Object.getOwnPropertyDescriptor(new Proxy(ns, {getOwnPropertyDescriptor() {return undefined;}}), 'value'),
                () => Reflect.defineProperty(new Proxy(ns, {defineProperty() {return true;}}), 'value', {value:7}),
            ]) {
                try {operation(); throw 'accepted uninitialized export';}
                catch (error) {if (!(error instanceof ReferenceError)) throw error; globalThis.checked++;}
            }
        "#,
        ),
    ]);
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
fn source_import_resolves_inherited_then_and_rejects_a_throwing_then_getter() {
    let source = r#"
        (async () => {
            const first = await import.source('./mod.wasm');
            const prototype = Object.getPrototypeOf(first);
            let calls = 0;
            prototype.then = function(resolve) {
                if (this !== first) throw new Error('source identity changed');
                calls++;
                resolve(42);
            };
            const value = await import.source('./mod.wasm');
            delete prototype.then;
            const sentinel = {};
            Object.defineProperty(prototype, 'then', {
                get() {throw sentinel;}, configurable: true
            });
            let rejected = false;
            try { await import.source('./mod.wasm'); }
            catch (error) { rejected = error === sentinel; }
            delete prototype.then;
            const again = await import.source('./mod.wasm');
            if (value !== 42 || calls !== 1 || !rejected || again !== first)
                throw new Error('source Promise resolution changed');
        })().then(() => $DONE(), $DONE);
    "#;
    let code = compile(&parse(source).unwrap()).unwrap();
    for nursery_capacity in [1, VmConfig::default().heap.nursery_capacity] {
        let mut config = VmConfig::default();
        config.heap.nursery_capacity = nursery_capacity;
        let mut vm = Vm::new(config).unwrap();
        vm.install_test262_done().unwrap();
        vm.set_module_source_loader_context(vec!["mod.wasm".to_string()]);
        vm.execute_script(&code).unwrap();
        vm.run_promise_jobs().unwrap();
        assert_eq!(vm.take_test262_done(), Some(Ok(())));
    }
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

#[test]
fn deferred_cycle_access_rejects_while_an_async_dependency_is_evaluating() {
    graph_true(&[
        ("main.mjs", "import defer * as ns from './dep.mjs'; import {blocked} from './async.mjs'; blocked && ns.value === 42"),
        ("dep.mjs", "import './async.mjs'; export const value = 42;"),
        ("async.mjs", "import defer * as ns from './dep.mjs'; await 1; export let blocked = false; try { ns.value; } catch (error) { blocked = error instanceof TypeError; }"),
    ]);
}

#[test]
fn deferred_namespace_omits_then_and_keeps_the_body_idle_until_value_access() {
    graph_true(&[
        ("main.mjs", "const ns = await import.defer('./dep.mjs'); const idle = globalThis.bodyRuns === undefined; const descriptor = Object.getOwnPropertyDescriptor(ns, 'then'); idle && descriptor === undefined && !('then' in ns) && ns.value === 42 && globalThis.bodyRuns === 1"),
        ("dep.mjs", "globalThis.bodyRuns = (globalThis.bodyRuns || 0) + 1; export const then = 7; export const value = 42;"),
    ]);
}

#[test]
fn eager_and_awaited_dynamic_imports_assimilate_exported_then_functions() {
    graph_true(&[
        (
            "main.mjs",
            r#"
            globalThis.importEvents = [];
            globalThis.importSentinel = {};
            const eager = import('./eager.mjs');
            importEvents.push('before');
            const first = await eager;
            const order = importEvents.join(',') === 'before,then';
            const again = await import('./eager.mjs');
            const awaited = await import('./awaited.mjs');
            let rejected = false;
            try { await import('./rejected.mjs'); }
            catch (error) { rejected = error === importSentinel; }
            const plain = await import('./plain.mjs');
            first === 7 && again === 7 && awaited === 42 && order &&
                rejected && plain.then === 19 && plain.value === 42;
        "#,
        ),
        (
            "eager.mjs",
            "export function then(resolve) {importEvents.push('then'); resolve(7);}",
        ),
        (
            "awaited.mjs",
            "await 0; export function then(resolve) {resolve(42);}",
        ),
        (
            "rejected.mjs",
            "export function then() {throw importSentinel;}",
        ),
        (
            "plain.mjs",
            "export const then = 19; export const value = 42;",
        ),
    ]);
}

#[test]
fn namespace_live_cells_keep_identity_after_module_debugger_resume() {
    let origin = BlueJsPageOrigin::new("https://example.test").unwrap();
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(7, origin.clone()).unwrap();
    let mut handles = Vec::new();
    for (name, source) in [
        (
            "main.mjs",
            "import * as ns from './dep.mjs'; globalThis.namespace = ns;",
        ),
        ("dep.mjs", "export let value = 41; value++;"),
    ] {
        handles.push(
            runtime
                .install_program(
                    7,
                    &origin,
                    BlueJsSourceIdentity::new(format!("graph/{name}"), format!("sha256:{name}"))
                        .unwrap(),
                    &BlueJsProgramV1::Module(parse_module(source).unwrap()),
                )
                .unwrap(),
        );
    }
    let point = runtime
        .module_evaluate_entry_safe_point(7, handles[0])
        .unwrap();
    assert_eq!(
        runtime.execute_module_graph_until_debugger_pause(7, handles[0], handles.clone(), point),
        Ok(BlueJsPageDebuggerExecutionState::Paused {
            bytecode_offset: point.bytecode_offset
        })
    );
    assert_eq!(
        runtime.resume_debugger_module_execution(7),
        Ok(BlueJsPageDebuggerExecutionState::Completed)
    );
    let reader = runtime
        .install_program(
            7,
            &origin,
            BlueJsSourceIdentity::new("graph/reader.js", "sha256:namespace-reader").unwrap(),
            &BlueJsProgramV1::Script(
                parse("namespace.value === 42 && Object.getPrototypeOf(namespace) === null")
                    .unwrap(),
            ),
        )
        .unwrap();
    assert_eq!(runtime.execute_program(7, reader), Ok(Value::Bool(true)));
}
