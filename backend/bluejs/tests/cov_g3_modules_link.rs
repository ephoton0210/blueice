// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Linking a module graph: the requests that cannot be resolved or supplied
//! at each step, source-phase and synthetic modules, dynamic modules that
//! fail to link or compile after the graph is running, and allocation failure
//! while the records and cells of all of them are made.

use blueice_bluejs::{
    compile, compile_module, parse, parse_module, Bytecode, HeapConfig, RuntimeError, Value, Vm,
    VmConfig,
};
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

/// A VM that has the host resources the graphs below import.
fn host() -> Vm {
    let mut vm = Vm::default();
    configure(&mut vm);
    vm
}

fn configure(vm: &mut Vm) {
    vm.set_json_module_sources(HashMap::from([(
        "t/a.json".to_string(),
        "{\"x\":1}".to_string(),
    )]));
    vm.set_text_module_sources(HashMap::from([("t/a.txt".to_string(), "text".to_string())]));
    vm.set_module_source_loader_context(vec!["t/x.wasm".to_string()]);
}

fn run(sources: &[(&str, &str)]) -> Result<Value, RuntimeError> {
    host().execute_module_graph("t/main.js", &build(sources))
}

#[test]
fn a_fresh_graph_that_fails_to_synthesize_a_module_releases_what_it_rooted() {
    // `a.json` is made (and rooted) before `b.json` turns out to have no
    // source.
    let sources = [(
        "main.js",
        "import a from './a.json' with { type: 'json' };
         import b from './b.json' with { type: 'json' };",
    )];
    assert_eq!(
        run(&sources),
        Err(RuntimeError::TypeError(
            "host did not provide a JSON module source for t/b.json".into()
        ))
    );
}

#[test]
fn every_kind_of_request_that_escapes_its_host_root_fails_where_it_is_linked() {
    for (source, expected) in [
        (
            "export { x } from '../../escape.js';",
            "relative module request ../../escape.js escapes its host root",
        ),
        (
            "import source x from '../../escape.js'; export { x };",
            "relative module request ../../escape.js escapes its host root",
        ),
        (
            "import source x from '../../escape.js';",
            "relative module request ../../escape.js escapes its host root",
        ),
        (
            "import { x } from '../../escape.js';",
            "relative module request ../../escape.js escapes its host root",
        ),
    ] {
        assert_eq!(
            run(&[("main.js", source)]),
            Err(RuntimeError::ModuleResolution(expected.into())),
            "{source}"
        );
    }
}

#[test]
fn a_source_phase_import_links_to_the_hosts_source_object_and_can_be_reexported() {
    let sources = [
        (
            "main.js",
            "import source own from './x.wasm';
             import { reexported } from './mid.js';
             typeof own + ',' + (own === reexported)",
        ),
        (
            "mid.js",
            "import source reexported from './x.wasm'; export { reexported };",
        ),
    ];
    assert_eq!(run(&sources), Ok(Value::String("object,true".into())));
}

#[test]
fn a_source_phase_import_of_a_module_the_host_only_knows_as_source_text_fails() {
    // `x.wasm` is registered as a source-phase record, but the host also
    // supplied it as an executable module.
    let sources = [
        ("main.js", "import source x from './x.wasm';"),
        ("x.wasm", "export const y = 1;"),
    ];
    assert_eq!(
        run(&sources),
        Err(RuntimeError::ModuleResolution(
            "t/x.wasm is a Source Text Module and has no source-phase representation".into()
        ))
    );
}

#[test]
fn a_module_that_fails_to_link_after_the_graph_started_is_rolled_back() {
    // `late.js` exists only as text for a dynamic import: it is linked while
    // `main.js` runs, gets a cell for `v`, and then fails on its import. The
    // failure undoes the import, cells included, and the graph carries on.
    let mut vm = host();
    vm.set_dynamic_module_sources(HashMap::from([
        (
            "t/late.js".to_string(),
            "var v = 1; import { nothing } from './dep.js';".to_string(),
        ),
        ("t/broken.js".to_string(), "new.target;".to_string()),
    ]));
    let modules = build(&[
        (
            "main.js",
            "const seen = [];
             for (const request of ['./late.js', './late.js', './broken.js']) {
               try { await import(request); seen.push('loaded') } catch (e) { seen.push(e.constructor.name) }
             }
             seen.join()",
        ),
        ("dep.js", "export const something = 1;"),
    ]);
    assert_eq!(
        vm.execute_module_graph("t/main.js", &modules),
        Ok(Value::String("SyntaxError,SyntaxError,SyntaxError".into()))
    );
}

#[test]
fn running_a_graph_again_after_a_module_failed_replays_the_recorded_error() {
    let modules = build(&[
        ("main.js", "import './dep.js';"),
        ("dep.js", "throw new RangeError('boom')"),
    ]);
    let mut vm = Vm::default();
    let first = vm.execute_module_graph("t/main.js", &modules);
    let second = vm.execute_module_graph("t/main.js", &modules);
    assert!(matches!(first, Err(RuntimeError::Thrown(_))), "{first:?}");
    assert!(matches!(second, Err(RuntimeError::Thrown(_))), "{second:?}");
}

/// How [`sweep`] reads an outcome that is neither a success nor the heap
/// error itself.
#[derive(Clone, Copy)]
enum Other {
    /// The graph fails on its own account: as good as a success.
    OwnFailure,
    /// The heap error, reported by another name (a JSON module that runs out
    /// of heap while it is parsed is an invalid JSON module).
    HeapLimit,
}

fn classify(error: &RuntimeError) -> Option<Other> {
    match error {
        RuntimeError::Thrown(_) => Some(Other::OwnFailure),
        RuntimeError::ModuleResolution(message) if message.contains("heap limit exceeded") => {
            Some(Other::HeapLimit)
        }
        _ => None,
    }
}

/// A VM whose heap ceiling is `limit`, with the host resources of these
/// tests, after `warm_up` (a script that builds what the graph is not about);
/// `None` when the ceiling is too small for that.
fn tight(limit: usize, nursery_capacity: usize, warm_up: Option<&Bytecode>) -> Option<Vm> {
    let mut vm = Vm::new(VmConfig {
        heap: HeapConfig {
            nursery_capacity,
            major_threshold_bytes: limit,
            max_heap_bytes: limit,
        },
        ..VmConfig::default()
    })
    .ok()?;
    configure(&mut vm);
    if let Some(code) = warm_up {
        vm.execute(code).ok()?;
    }
    Some(vm)
}

/// Runs `sources` on VMs whose heap ceiling rises from the smallest one that
/// holds the VM after `warm_up` in steps of eight bytes, for several nursery
/// sizes, so that each allocation made while the graph is linked and run
/// fails in turn. A heap ceiling fails an allocation only when nothing
/// earlier in the run demanded as much, which is why the intrinsics a graph
/// needs are built by the warm-up instead of by the graph. Every outcome must
/// be a success, the heap-limit error or one [`classify`] accepts. Returns
/// how many runs hit the limit.
fn sweep_after(warm_up: &str, sources: &[(&str, &str)]) -> usize {
    let modules = build(sources);
    let warm = compile(&parse(warm_up).unwrap()).unwrap();
    // The least ceiling that holds the warm VM.
    let (mut low, mut high) = (0, 16 * 1024 * 1024);
    while high - low > 8 {
        let middle = low + (high - low) / 2;
        if tight(middle, 1, Some(&warm)).is_some() {
            high = middle;
        } else {
            low = middle;
        }
    }
    let mut failures = 0;
    for nursery_capacity in [1, 2, 3] {
        let mut successes = 0;
        for limit in (high..).step_by(8) {
            let Some(mut vm) = tight(limit, nursery_capacity, Some(&warm)) else {
                failures += 1;
                continue;
            };
            let outcome = vm.execute_module_graph("t/main.js", &modules);
            match outcome.as_ref().map_err(classify) {
                Ok(_) | Err(Some(Other::OwnFailure)) => successes += 1,
                Err(Some(Other::HeapLimit)) => {
                    failures += 1;
                    successes = 0;
                }
                Err(None) if matches!(outcome, Err(RuntimeError::Heap(_))) => {
                    failures += 1;
                    successes = 0;
                }
                Err(None) => panic!("limit {limit}: {outcome:?} for {sources:?}"),
            }
            if successes == 16 {
                break;
            }
        }
    }
    failures
}

/// [`sweep_after`] on a warm-up that builds the Promise and error machinery.
fn sweep(sources: &[(&str, &str)]) -> usize {
    sweep_after(
        "Promise; Promise.resolve; Promise.reject; TypeError; Object.keys;",
        sources,
    )
}

#[test]
fn graphs_survive_every_allocation_failure_while_their_cells_are_made() {
    for sources in [
        // A `var` binding, a namespace import and a live binding.
        vec![
            (
                "main.js",
                "var v = 1; import def, { x } from './dep.js'; import * as ns from './dep.js'; export const y = x + ns.x;",
            ),
            ("dep.js", "export let x = 1; export default 2;"),
        ],
        // Synthetic modules: JSON, text.
        vec![(
            "main.js",
            "import a from './a.json' with { type: 'json' };
             import t from './a.txt' with { type: 'text' };
             export const both = [a, t];",
        )],
        // A source-phase import and its re-export.
        vec![
            (
                "main.js",
                "import source own from './x.wasm'; import { reexported } from './mid.js';",
            ),
            (
                "mid.js",
                "import source reexported from './x.wasm'; export { reexported };",
            ),
        ],
        // Hoisted functions, instantiated before anything runs.
        vec![(
            "main.js",
            "export function f() {} export function g() {} export class C {}",
        )],
    ] {
        assert!(sweep(&sources) > 0, "{sources:?}");
    }
}

fn string(text: &str) -> Value {
    Value::String(text.into())
}

#[test]
fn a_parent_waits_for_every_asynchronous_dependency() {
    let sources = [
        (
            "main.js",
            "import { log } from './log.js'; import './parent.js'; log.join()",
        ),
        ("log.js", "export const log = [];"),
        (
            "parent.js",
            "import { log } from './log.js'; import './a.js'; import './b.js'; log.push('parent')",
        ),
        (
            "a.js",
            "import { log } from './log.js'; await null; log.push('a')",
        ),
        (
            "b.js",
            "import { log } from './log.js'; await null; await null; log.push('b')",
        ),
    ];
    assert_eq!(run(&sources), Ok(string("a,b,parent")));
}

#[test]
fn deferred_imports_wait_for_each_of_their_asynchronous_dependencies() {
    let sources = [
        (
            "main.js",
            "const both = import.defer('./both.js');
             const fails = import.defer('./fails.js');
             const outcomes = await Promise.allSettled([both, fails]);
             outcomes.map((outcome) => outcome.status).join()",
        ),
        ("both.js", "import './a.js'; import './b.js';"),
        ("a.js", "await null;"),
        ("b.js", "await null; await null;"),
        ("fails.js", "import './bad.js';"),
        ("bad.js", "await Promise.reject(new RangeError('no'));"),
    ];
    assert_eq!(run(&sources), Ok(string("fulfilled,rejected")));
}

#[test]
fn errors_the_engine_raises_at_the_top_level_of_a_module_become_its_evaluation_error() {
    for body in [
        "undeclared_name;",
        "null.property;",
        "eval('(');",
        "'x'.repeat(-1);",
    ] {
        let sources = [
            ("main.js", "import './dep.js'; export const ran = true;"),
            ("dep.js", body),
        ];
        let outcome = run(&sources);
        assert!(
            matches!(outcome, Err(RuntimeError::Thrown(_))),
            "{body}: {outcome:?}"
        );
    }
}

#[test]
fn a_parent_that_throws_after_its_dependency_settled_ends_the_graph() {
    let sources = [
        (
            "main.js",
            "import './dep.js'; throw new EvalError('parent')",
        ),
        ("dep.js", "await null;"),
    ];
    let outcome = run(&sources);
    assert!(
        matches!(outcome, Err(RuntimeError::Thrown(_))),
        "{outcome:?}"
    );
}

#[test]
fn a_module_body_can_complete_with_an_object() {
    let outcome = run(&[("main.js", "({ a: 1 })")]);
    assert!(matches!(outcome, Ok(Value::Object(_))), "{outcome:?}");
    // And a second run of the graph replays it after everything unrooted
    // was collected.
    let modules = build(&[("main.js", "({ a: 1 })")]);
    let mut vm = Vm::default();
    let first = vm.execute_module_graph("t/main.js", &modules);
    let second = vm.execute_module_graph("t/main.js", &modules);
    assert!(matches!(first, Ok(Value::Object(_))), "{first:?}");
    assert!(matches!(second, Ok(Value::Object(_))), "{second:?}");
}

#[test]
fn a_source_phase_export_of_an_import_the_host_never_registered_fails_to_link() {
    for source in [
        "import source x from './missing.wasm'; export { x };",
        "import source x from './missing.wasm';",
    ] {
        assert_eq!(
            run(&[("main.js", source)]),
            Err(RuntimeError::TypeError(
                "host did not provide a source-phase representation for t/missing.wasm".into()
            )),
            "{source}"
        );
    }
}

#[test]
fn asynchronous_graphs_survive_every_allocation_failure() {
    for sources in [
        // A dynamic import that waits for a module with a top-level await.
        vec![
            ("main.js", "await import('./dep.js'); export {}"),
            ("dep.js", "await null; export const x = 1;"),
        ],
        // ... that rejects, and one that imports the same module twice.
        vec![
            (
                "main.js",
                "const results = await Promise.allSettled([import('./dep.js'), import('./dep.js')]); export const n = results.length",
            ),
            ("dep.js", "await Promise.reject(new RangeError('no'))"),
        ],
        // Static parents of asynchronous modules, one of which throws.
        vec![
            ("main.js", "import './a.js'; import './b.js'; export {}"),
            ("a.js", "await null; export const a = 1;"),
            ("b.js", "import './a.js'; await null; throw new EvalError('b')"),
        ],
        // Deferred imports waiting for asynchronous modules.
        vec![
            (
                "main.js",
                "const both = import.defer('./both.js');
                 const fails = import.defer('./fails.js');
                 await Promise.allSettled([both, fails]);",
            ),
            ("both.js", "import './a.js'; import './b.js';"),
            ("a.js", "await null;"),
            ("b.js", "await null; await null;"),
            ("fails.js", "import './bad.js';"),
            ("bad.js", "await Promise.reject(new RangeError('no'));"),
        ],
        // A dynamic import of a namespace that already exists.
        vec![
            (
                "main.js",
                "import * as ns from './dep.js'; const again = await import('./dep.js'); export const same = again === ns",
            ),
            ("dep.js", "export const x = 1;"),
        ],
    ] {
        assert!(sweep(&sources) > 0, "{sources:?}");
    }
}

#[test]
fn a_dynamic_import_reports_an_escaping_specifier_and_replays_a_failed_module() {
    let sources = [
        (
            "main.js",
            "const seen = [];
             for (const request of ['../../escape.js', './bad.js', './bad.js']) {
               try { await import(request); seen.push('loaded') } catch (e) { seen.push(e.constructor.name + ':' + e.message) }
             }
             seen.join('|')",
        ),
        ("bad.js", "throw new RangeError('boom')"),
    ];
    assert_eq!(
        run(&sources),
        Ok(string(
            "SyntaxError:relative module request ../../escape.js escapes its host root|RangeError:boom|RangeError:boom"
        ))
    );
}

#[test]
fn a_dynamic_import_of_an_evaluated_module_makes_its_namespace_when_asked() {
    let sources = [
        (
            "main.js",
            "import { x } from './dep.js'; const ns = await import('./dep.js'); export const y = ns.x + x",
        ),
        ("dep.js", "export const x = 1;"),
    ];
    assert_eq!(run(&sources), Ok(Value::Undefined));
    assert!(sweep(&sources) > 0);
}

#[test]
fn dynamic_imports_and_engine_errors_survive_every_allocation_failure() {
    for sources in [
        // Bad options reject the `import()` promise right away.
        vec![
            (
                "main.js",
                "import('./dep.js', 5).catch(() => {}); export {};",
            ),
            ("dep.js", "export const x = 1;"),
        ],
        // The source phase, and one that cannot be resolved.
        vec![("main.js", "await import.source('./x.wasm'); export {};")],
        vec![(
            "main.js",
            "await import.source('../../x.wasm').catch(() => {}); export {};",
        )],
        // An error the engine raises at the top level of a module.
        vec![
            ("main.js", "import './dep.js';"),
            ("dep.js", "null.x;"),
        ],
        // A dynamic import of a module that is already evaluated.
        vec![
            (
                "main.js",
                "import { x } from './dep.js'; const ns = await import('./dep.js'); export const y = 1;",
            ),
            ("dep.js", "export const x = 1;"),
        ],
        // A deferred import.
        vec![
            ("main.js", "await import.defer('./dep.js'); export {};"),
            ("dep.js", "export const x = 1;"),
        ],
    ] {
        assert!(sweep(&sources) > 0, "{sources:?}");
    }
}

/// Statements after which the awaits of a module run in the queue's own turns
/// (a rejected await, a `finally` that awaits, an async function that uses
/// `eval`, an async generator).
const INLINE_PREFIX: &str = "const log = [];
    try { await Promise.reject(new Error('caught')); log.push('not reached') } catch (e) { log.push(e.message) }
    try { try { throw 1 } finally { await 0; log.push('finally') } } catch (e) { log.push('outer ' + e) }
    async function withEval() { eval('var seen = {}'); await 0; return 'evaled' }
    log.push(await withEval());
    async function* generator() { try { yield 1 } finally { await 0; log.push('cleanup') } }
    const it = generator(); await it.next(); await it.return();";

#[test]
fn an_import_with_an_attribute_other_than_type_is_accepted_after_such_awaits() {
    let main = format!(
        "{INLINE_PREFIX}
         const ns = await import('./dep.js', {{ with: {{ note: 'kept' }} }});
         log.push(ns.value);
         log.join()"
    );
    let sources = [
        ("main.js", main.as_str()),
        // Its body completes with an object.
        ("dep.js", "export const value = 'dep'; ({ done: true })"),
    ];
    assert_eq!(
        run(&sources),
        Ok(string("caught,finally,outer 1,evaled,cleanup,dep"))
    );
}

#[test]
fn a_module_that_finished_in_a_cycle_shares_its_cycle_roots_rejection() {
    let sources = [
        (
            "main.js",
            "const seen = [];
             for (const request of ['./a.js', './b.js']) {
               try { await import(request); seen.push('loaded') } catch (e) { seen.push(e) }
             }
             seen.join()",
        ),
        ("a.js", "import './b.js'; await Promise.reject('boom');"),
        ("b.js", "import './a.js'; export const b = 1;"),
    ];
    assert_eq!(run(&sources), Ok(string("boom,boom")));
}

#[test]
fn a_parent_rejected_by_one_dependency_is_left_alone_by_the_next() {
    let sources = [
        ("main.js", "import './parent.js';"),
        ("parent.js", "import './a.js'; import './b.js';"),
        ("a.js", "await Promise.reject(new RangeError('a'));"),
        ("b.js", "await Promise.reject(new RangeError('b'));"),
    ];
    let outcome = run(&sources);
    assert!(
        matches!(outcome, Err(RuntimeError::Thrown(_))),
        "{outcome:?}"
    );
}

#[test]
fn a_cycle_root_is_found_through_a_parent_whose_own_wait_ended() {
    // `parent.js` waits on both `slow.js` and `fails.js`; when `fails.js`
    // rejects it fails, and stays on `slow.js`'s list of waiting parents.
    // The import of `late.js`, which needs `slow.js` too, then finds it there.
    let sources = [
        (
            "main.js",
            "const first = import('./parent.js').catch((e) => e.message);
             for (let tick = 0; tick < 12; tick++) await null;
             const second = import('./late.js');
             for (let tick = 0; tick < 12; tick++) await null;
             globalThis.release();
             const results = await Promise.all([first, second.then(() => 'late loaded')]);
             results.join()",
        ),
        ("parent.js", "import './slow.js'; import './fails.js';"),
        (
            "slow.js",
            "await new Promise((resolve) => { globalThis.release = resolve });",
        ),
        ("fails.js", "await Promise.reject(new RangeError('fails'));"),
        ("late.js", "import './slow.js';"),
    ];
    assert_eq!(run(&sources), Ok(string("fails,late loaded")));
}

/// Starting an async generator makes the entry's later awaits run in the
/// queue's own turns, and an `import()` that is still queued then runs while
/// the entry's records are parked: it joins the running graph.
const JOINING: &str = "const p = import('./dep.js');
    async function* g() { yield 1 }
    const it = g(); it.next();
    const ns = await p;
    ns.value";

#[test]
fn an_import_queued_while_the_entry_runs_joins_its_graph_and_hands_back_an_object() {
    let sources = [
        ("main.js", JOINING),
        // Its body completes with an object.
        ("dep.js", "export const value = 'dep'; ({ done: true })"),
    ];
    assert_eq!(run(&sources), Ok(string("dep")));
}

/// Known limitation: a module of the joined graph that awaits is resumed while
/// the running graph's records are parked, and its resumption cannot find
/// them.
#[test]
fn an_asynchronous_module_joined_to_the_running_graph_cannot_be_resumed() {
    let sources = [
        ("main.js", JOINING),
        ("dep.js", "await null; export const value = 'dep';"),
    ];
    assert_eq!(
        run(&sources),
        Err(RuntimeError::Unsupported("module graph continuation"))
    );
}

#[test]
fn a_module_with_a_deferred_request_is_walked_for_cycles_with_a_failed_module() {
    let sources = [
        (
            "main.js",
            "await import('./dep.js');
             try { await import('./bad.js') } catch (e) {}
             const ns = await import('./dep.js');
             ns.x",
        ),
        (
            "dep.js",
            "import defer * as inner from './inner.js'; export const x = 'dep';",
        ),
        ("inner.js", "export const y = 1;"),
        ("bad.js", "throw new RangeError('bad')"),
    ];
    assert_eq!(run(&sources), Ok(string("dep")));
}
