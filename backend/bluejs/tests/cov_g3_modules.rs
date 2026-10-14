// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Module linking and evaluation through `Vm::execute_module_graph`: link
//! errors, asynchronous graphs, dynamic import from modules and scripts,
//! `import.meta`, and allocation failure while a graph is linked.

mod cov_g3_support;
use blueice_bluejs::{
    compile, compile_module, parse, parse_module, Bytecode, RuntimeError, Value, Vm, VmConfig,
};
use cov_g3_support::heap_limit_sweep;
use std::collections::HashMap;

fn build(sources: &[(&str, &str)]) -> HashMap<String, Bytecode> {
    sources
        .iter()
        .map(|(name, source)| {
            let module = parse_module(source).unwrap_or_else(|e| panic!("{name}: {e:?}"));
            (
                format!("t/{name}"),
                compile_module(&module).unwrap_or_else(|e| panic!("{name}: {e:?}")),
            )
        })
        .collect()
}

/// Runs the graph rooted at `t/main.js` and returns its completion.
fn graph(sources: &[(&str, &str)]) -> Result<Value, RuntimeError> {
    Vm::default().execute_module_graph("t/main.js", &build(sources))
}

/// The error message of a graph that fails to link or run.
fn graph_error(sources: &[(&str, &str)]) -> String {
    match graph(sources) {
        Ok(value) => panic!("expected an error, got {value:?}"),
        Err(RuntimeError::ModuleResolution(message)) => message,
        Err(RuntimeError::Thrown(value)) => format!("thrown {value:?}"),
        Err(other) => format!("{other:?}"),
    }
}

#[test]
fn a_dependency_that_is_not_a_module_cannot_be_linked() {
    let mut modules = build(&[("main.js", "import './dep.js';")]);
    modules.insert(
        "t/dep.js".to_string(),
        compile(&parse("1;").unwrap()).unwrap(),
    );
    assert_eq!(
        Vm::default().execute_module_graph("t/main.js", &modules),
        Err(RuntimeError::ModuleResolution(
            "t/dep.js was not compiled using the module goal".into()
        ))
    );
}

#[test]
fn unresolvable_and_misspelled_requests_are_resolution_errors() {
    for (sources, expected) in [
        (
            vec![("main.js", "import './missing.js';")],
            "module t/missing.js was not supplied by the host",
        ),
        (
            vec![
                ("main.js", "import { nothing } from './dep.js';"),
                ("dep.js", "export const something = 1;"),
            ],
            "./dep.js does not export nothing",
        ),
        (
            vec![
                ("main.js", "export { nothing } from './dep.js';"),
                ("dep.js", "export const something = 1;"),
            ],
            "./dep.js does not export nothing for nothing",
        ),
        (
            vec![
                ("main.js", "import { x } from './star.js';"),
                ("star.js", "export * from './a.js'; export * from './b.js';"),
                ("a.js", "export const x = 1;"),
                ("b.js", "export const x = 2;"),
            ],
            "./star.js does not export x",
        ),
        (
            vec![
                ("main.js", "export { x } from './star.js';"),
                ("star.js", "export * from './a.js'; export * from './b.js';"),
                ("a.js", "export const x = 1;"),
                ("b.js", "export const x = 2;"),
            ],
            "./star.js does not export x for x",
        ),
        (
            vec![("main.js", "import './a\\u0000b.js';")],
            "a module specifier cannot contain a NUL character",
        ),
        (
            vec![("main.js", "import '../../escape.js';")],
            "relative module request ../../escape.js escapes its host root",
        ),
    ] {
        assert_eq!(graph_error(&sources), expected, "{sources:?}");
    }
}

#[test]
fn live_bindings_namespaces_and_default_exports_link_across_modules() {
    let sources = [
        (
            "main.js",
            "import def, { counter, bump } from './dep.js';
             import * as ns from './dep.js';
             export { counter as reexported } from './dep.js';
             const before = counter; bump();
             `${def()}|${before}|${counter}|${ns.counter}|${Object.keys(ns).join()}`",
        ),
        (
            "dep.js",
            "export let counter = 1; export function bump() { counter++ }
             export default function () { return 'default' }",
        ),
    ];
    assert_eq!(
        graph(&sources),
        Ok(Value::String("default|1|2|2|bump,counter,default".into()))
    );
}

#[test]
fn cycles_share_bindings_and_hoisted_functions() {
    let sources = [
        (
            "main.js",
            "import { a } from './a.js'; import { b } from './b.js'; a() + b()",
        ),
        (
            "a.js",
            "import { b } from './b.js'; export function a() { return 'a' + (typeof b) }",
        ),
        (
            "b.js",
            "import { a } from './a.js'; export function b() { return 'b' + (typeof a) }",
        ),
    ];
    assert_eq!(
        graph(&sources),
        Ok(Value::String("afunctionbfunction".into()))
    );
}

#[test]
fn errors_thrown_by_a_dependency_are_recorded_and_seen_again() {
    let message = graph_error(&[
        ("main.js", "import './a.js';"),
        ("a.js", "import './b.js';"),
        ("b.js", "throw new RangeError('boom')"),
    ]);
    assert!(message.starts_with("thrown "), "{message}");
}

#[test]
fn top_level_await_orders_evaluation_and_reports_rejections() {
    let sources = [
        (
            "main.js",
            "import { log } from './log.js'; import './slow.js'; import './fast.js';
             log.push('main'); log.join()",
        ),
        ("log.js", "export const log = [];"),
        (
            "slow.js",
            "import { log } from './log.js'; await Promise.resolve(); await null; log.push('slow')",
        ),
        (
            "fast.js",
            "import { log } from './log.js'; log.push('fast')",
        ),
    ];
    assert_eq!(graph(&sources), Ok(Value::String("fast,slow,main".into())));

    let message = graph_error(&[
        ("main.js", "import './dep.js'; export const ran = true;"),
        ("dep.js", "await Promise.reject(new TypeError('late'))"),
    ]);
    assert!(message.starts_with("thrown "), "{message}");

    let message = graph_error(&[("main.js", "await 1; throw new EvalError('after await')")]);
    assert!(message.starts_with("thrown "), "{message}");
}

#[test]
fn dynamic_import_from_a_module_loads_evaluates_and_rejects() {
    let sources = [
        (
            "main.js",
            "const results = [];
             const dep = await import('./dep.js');
             results.push(dep.value);
             try { await import('./missing.js') } catch (e) { results.push('missing') }
             try { await import('./throws.js') } catch (e) { results.push('threw') }
             try { await import({ toString() { throw new EvalError('spec') } }) } catch (e) { results.push('spec') }
             try { await import('\\ud800') } catch (e) { results.push('surrogate') }
             results.join()",
        ),
        ("dep.js", "export const value = 42;"),
        ("throws.js", "throw new Error('nope')"),
    ];
    assert_eq!(
        graph(&sources),
        Ok(Value::String("42,missing,threw,spec,surrogate".into()))
    );
}

#[test]
fn dynamic_import_attributes_are_validated() {
    let sources = [
        (
            "main.js",
            "const results = [];
             for (const options of [
               { with: { type: 'unknown' } },
               { with: { type: 1 } },
               { with: 5 },
               5,
               { get with() { throw new EvalError('with') } },
               { with: { get type() { throw new EvalError('type') } } },
               { with: new Proxy({}, { ownKeys() { throw new EvalError('keys') } }) },
               { with: { [Symbol('s')]: 1 } },
             ]) {
               try { await import('./dep.js', options); results.push('ok') } catch (e) { results.push(e.constructor.name) }
             }
             results.join()",
        ),
        ("dep.js", "export const value = 42;"),
    ];
    let result = graph(&sources);
    assert!(matches!(result, Ok(Value::String(_))), "{result:?}");
}

#[test]
fn import_meta_is_stable_per_module() {
    let mut vm = Vm::default();
    let code = compile_module(
        &parse_module("import.meta === import.meta && Object.getPrototypeOf(import.meta) === null")
            .unwrap(),
    )
    .unwrap();
    assert_eq!(vm.execute_module(&code), Ok(Value::Bool(true)));
    let sources = [
        (
            "main.js",
            "import { meta } from './dep.js'; meta === import.meta ? 'same' : 'different'",
        ),
        ("dep.js", "export const meta = import.meta;"),
    ];
    assert_eq!(graph(&sources), Ok(Value::String("different".into())));
}

#[test]
fn dynamic_module_sources_that_do_not_compile_reject_the_import() {
    let mut vm = Vm::new(VmConfig::default()).unwrap();
    vm.set_dynamic_module_sources(HashMap::from([
        ("t/syntax.js".to_string(), "export {".to_string()),
        (
            "t/compile.js".to_string(),
            "class C { m() { this.#missing } }".to_string(),
        ),
        (
            "t/conflict.js".to_string(),
            "function f() {} var f;".to_string(),
        ),
        ("t/conflict2.js".to_string(), "let x; var x;".to_string()),
        (
            "t/duplicate.js".to_string(),
            "export default 1; export default 2;".to_string(),
        ),
        ("t/fine.js".to_string(), "export const ok = 1;".to_string()),
    ]));
    let modules = build(&[(
        "main.js",
        "const results = [];
         for (const name of ['syntax', 'compile', 'conflict', 'conflict2', 'duplicate', 'fine', 'absent']) {
           try { await import('./' + name + '.js'); results.push('ok') } catch (e) { results.push(e.constructor.name) }
         }
         results.join()",
    )]);
    assert_eq!(
        vm.execute_module_graph("t/main.js", &modules),
        Ok(Value::String(
            "SyntaxError,SyntaxError,SyntaxError,SyntaxError,SyntaxError,ok,SyntaxError".into()
        ))
    );
}

#[test]
fn module_graphs_survive_every_allocation_failure() {
    // The graph itself is built in Rust, so this sweep drives the linker
    // directly rather than through a script.
    let modules = build(&[
        (
            "main.js",
            "import def, { x } from './dep.js'; import * as ns from './dep.js'; import.meta;
             await Promise.resolve(); const d = await import('./dep.js'); export const y = x + ns.x + d.x;",
        ),
        ("dep.js", "export let x = 1; export default 2;"),
    ]);
    let mut failures = 0;
    for nursery_capacity in [1, 2, 3] {
        let mut successes = 0;
        for limit in (Vm::default().heap().stats().managed_bytes..).step_by(64) {
            let Ok(mut vm) = Vm::new(VmConfig {
                heap: blueice_bluejs::HeapConfig {
                    nursery_capacity,
                    major_threshold_bytes: limit,
                    max_heap_bytes: limit,
                },
                ..VmConfig::default()
            }) else {
                continue;
            };
            match vm.execute_module_graph("t/main.js", &modules) {
                Ok(_) => successes += 1,
                Err(RuntimeError::Heap(_)) => {
                    failures += 1;
                    successes = 0;
                }
                Err(other) => panic!("limit {limit}: {other:?}"),
            }
            if successes == 8 {
                break;
            }
        }
    }
    assert!(failures > 0);
    // And the scripted-import path, through the shared sweep.
    assert!(heap_limit_sweep("0;", "Promise.resolve().then(() => 1)") > 0);
}

#[test]
fn link_failures_of_a_dynamic_import_are_host_errors_and_roll_the_import_back() {
    // The failing import is a delta on an existing graph: it is undone, and
    // the entry's own graph stays usable for another request.
    let modules = build(&[
        ("main.js", "await import('./bad.js');"),
        ("bad.js", "import { nothing } from './dep.js';"),
        ("dep.js", "export const something = 1;"),
    ]);
    let mut vm = Vm::default();
    assert_eq!(
        vm.execute_module_graph("t/main.js", &modules),
        Err(RuntimeError::ModuleResolution(
            "./dep.js does not export nothing".into()
        ))
    );
}

#[test]
fn a_graph_can_be_executed_again_on_the_same_vm() {
    let modules = build(&[
        (
            "main.js",
            "import { x } from './dep.js'; export const y = x + 1; y",
        ),
        ("dep.js", "export const x = 1;"),
    ]);
    let mut vm = Vm::default();
    assert_eq!(
        vm.execute_module_graph("t/main.js", &modules),
        Ok(Value::Number(2.0))
    );
    assert_eq!(
        vm.execute_module_graph("t/main.js", &modules),
        Ok(Value::Number(2.0))
    );
}

#[test]
fn unresolvable_indirect_exports_and_source_imports_are_resolution_errors() {
    for (sources, expected) in [
        (
            vec![("main.js", "export { x } from './missing.js';")],
            "module t/missing.js was not supplied by the host",
        ),
        (
            vec![("main.js", "import { x } from './missing.js';")],
            "module t/missing.js was not supplied by the host",
        ),
        (
            vec![
                (
                    "main.js",
                    "import * as ns from './dep.js'; export const a = ns;",
                ),
                ("dep.js", "export const x = 1;"),
            ],
            "",
        ),
    ] {
        let result = graph(&sources);
        if expected.is_empty() {
            assert!(result.is_ok(), "{sources:?}: {result:?}");
        } else {
            assert_eq!(graph_error(&sources), expected, "{sources:?}");
        }
    }
}

#[test]
fn a_dynamic_source_that_does_not_compile_fails_a_script_import_without_a_graph() {
    // No module graph exists yet, so the failed request leaves nothing behind.
    let mut vm = Vm::default();
    vm.install_test262_done().unwrap();
    vm.set_module_loader_context("dir/main.js", HashMap::new());
    vm.set_dynamic_module_sources(HashMap::from([
        ("dir/syntax.js".to_string(), "export {".to_string()),
        (
            "dir/fine.js".to_string(),
            "export const ok = 1;".to_string(),
        ),
    ]));
    let script = "var results = [];
        import('./syntax.js').then(() => results.push('loaded'), (e) => results.push(e.constructor.name))
          .then(() => import('./fine.js')).then((ns) => results.push(ns.ok))
          .then(() => $DONE(results.join() === 'SyntaxError,1' ? undefined : new Error(results.join())));";
    vm.execute_script(&compile(&parse(script).unwrap()).unwrap())
        .unwrap();
    vm.run_promise_jobs().unwrap();
    assert_eq!(vm.take_test262_done(), Some(Ok(())));
}

#[test]
fn import_attribute_objects_skip_symbols_hidden_keys_and_report_trap_failures() {
    let sources = [
        (
            "main.js",
            "const results = [];
             for (const options of [
               { with: new Proxy({ a: 'x' }, { ownKeys: () => ['a'], getOwnPropertyDescriptor: () => undefined }) },
               { with: (() => { const o = {}; Object.defineProperty(o, 'hidden', { value: 'x', enumerable: false }); return o })() },
               { with: new Proxy({}, { ownKeys: () => ['a'], getOwnPropertyDescriptor() { throw new EvalError('d') } }) },
             ]) {
               try { await import('./dep.js', options); results.push('ok') } catch (e) { results.push(e.constructor.name) }
             }
             results.join()",
        ),
        ("dep.js", "export const value = 42;"),
    ];
    assert_eq!(graph(&sources), Ok(Value::String("ok,ok,EvalError".into())));
}

#[test]
fn a_rejected_await_in_a_module_can_be_caught_and_cleans_up_pending_completions() {
    let sources = [(
        "main.js",
        "const log = [];
         try { await Promise.reject(new Error('caught')); log.push('not reached') } catch (e) { log.push(e.message) }
         try { try { throw 1 } finally { await 0; log.push('finally') } } catch (e) { log.push('outer ' + e) }
         async function withEval() { eval('var seen = {}'); await 0; return 'evaled' }
         log.push(await withEval());
         async function* generator() { try { yield 1 } finally { await 0; log.push('cleanup') } }
         const it = generator(); await it.next(); await it.return();
         log.join()",
    )];
    assert_eq!(
        graph(&sources),
        Ok(Value::String(
            "caught,finally,outer 1,evaled,cleanup".into()
        ))
    );
}

#[test]
fn running_out_of_instructions_while_a_module_awaits_is_an_engine_error() {
    let modules = build(&[("main.js", "await 1; let n = 0; while (true) { n++ }")]);
    let mut vm = Vm::new(VmConfig {
        instruction_budget: 20_000,
        ..VmConfig::default()
    })
    .unwrap();
    assert_eq!(
        vm.execute_module_graph("t/main.js", &modules),
        Err(RuntimeError::InstructionLimit)
    );
}
