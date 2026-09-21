// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The import-defer proposal through parse, compile and the VM's module
//! graph: `import defer * as ns from "m"` and `import.defer("m")` link `m`
//! and hand back a *deferred* module namespace object whose first observable
//! use (a property read, descriptor lookup, `in`, delete, define or
//! own-keys enumeration of a non-symbol, non-`"then"` key) evaluates the
//! module synchronously. Every module records a shared `evaluations` log so
//! the tests can observe exactly when each one ran.

use blueice_bluejs::{compile_module, parse_module, RuntimeError, Value, Vm};
use std::collections::HashMap;

const SETUP: &str = "globalThis.evaluations = [];";

fn build(sources: &[(&str, &str)]) -> HashMap<String, blueice_bluejs::Bytecode> {
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

/// Runs the static graph rooted at `t/main.js` and returns its completion.
fn run(sources: &[(&str, &str)]) -> Result<Value, RuntimeError> {
    Vm::default().execute_module_graph("t/main.js", &build(sources))
}

fn string(value: &str) -> Value {
    Value::String(value.into())
}

/// Runs a graph whose `main.js` (after `setup.js`) is `main`, with the
/// supporting `deps`, and returns the string `main` completes with.
fn run_log(main: &str, deps: &[(&str, &str)]) -> Result<Value, RuntimeError> {
    let main = format!("import './setup.js';\n{main}");
    let mut sources = vec![("main.js", main.as_str()), ("setup.js", SETUP)];
    sources.extend_from_slice(deps);
    run(&sources)
}

const DEP: (&str, &str) = (
    "dep.js",
    "globalThis.evaluations.push('dep'); export let exported = 3;",
);

#[test]
fn a_deferred_import_links_without_evaluating_until_first_observation() {
    let result = run_log(
        "import defer * as ns from './dep.js';
         const before = evaluations.join();
         const value = ns.exported;
         before + '|' + evaluations.join() + '|' + value",
        &[DEP],
    );
    assert_eq!(result, Ok(string("|dep|3")));
}

#[test]
fn every_string_keyed_internal_method_triggers_evaluation_and_only_once() {
    // Each operation runs against a fresh deferred namespace of a module that
    // has not been evaluated yet. The result lists the evaluation log before
    // the operation, right after it, and after one more read (which must not
    // evaluate the module a second time).
    for (name, body) in [
        ("get exported", "ns.exported"),
        ("get not exported", "ns.missing"),
        (
            "getOwnPropertyDescriptor",
            "Object.getOwnPropertyDescriptor(ns, 'exported')",
        ),
        ("has via in", "'exported' in ns"),
        ("has missing via in", "'missing' in ns"),
        ("delete", "delete ns.exported"),
        (
            "defineProperty",
            "Reflect.defineProperty(ns, 'exported', { value: 3 })",
        ),
        ("ownKeys", "Reflect.ownKeys(ns)"),
        ("getOwnPropertyNames", "Object.getOwnPropertyNames(ns)"),
        ("getOwnPropertySymbols", "Object.getOwnPropertySymbols(ns)"),
        ("keys", "Object.keys(ns)"),
        ("get in prototype chain", "Object.create(ns).exported"),
        ("has in prototype chain", "'exported' in Object.create(ns)"),
        (
            "super get",
            "({ f() { return super.exported; } , __proto__: ns }).f()",
        ),
    ] {
        let main = format!(
            "import defer * as ns from './dep.js';
             const before = evaluations.length;
             try {{ {body}; }} catch (e) {{}}
             const once = evaluations.join();
             try {{ ns.exported; }} catch (e) {{}}
             before + '|' + once + '|' + evaluations.join()"
        );
        assert_eq!(run_log(&main, &[DEP]), Ok(string("0|dep|dep")), "{name}");
    }
}

#[test]
fn symbol_keys_then_and_metadata_operations_do_not_trigger_evaluation() {
    for (name, body) in [
        ("toStringTag", "ns[Symbol.toStringTag]"),
        ("other symbol", "ns[Symbol.iterator]"),
        (
            "symbol descriptor",
            "Object.getOwnPropertyDescriptor(ns, Symbol.toStringTag)",
        ),
        ("symbol in", "Symbol.toStringTag in ns"),
        ("then get", "ns.then"),
        ("then in", "'then' in ns"),
        (
            "then descriptor",
            "Object.getOwnPropertyDescriptor(ns, 'then')",
        ),
        ("getPrototypeOf", "Object.getPrototypeOf(ns)"),
        ("isExtensible", "Object.isExtensible(ns)"),
        ("preventExtensions", "Object.preventExtensions(ns)"),
        ("setPrototypeOf", "Reflect.setPrototypeOf(ns, null)"),
        ("set string key", "Reflect.set(ns, 'exported', 1)"),
        ("typeof", "typeof ns"),
    ] {
        let main = format!(
            "import defer * as ns from './dep.js';
             {body};
             evaluations.length"
        );
        assert_eq!(run_log(&main, &[DEP]), Ok(Value::Number(0.0)), "{name}");
    }
}

#[test]
fn a_deferred_namespace_is_a_distinct_null_prototype_object_tagged_deferred_module() {
    let dep = (
        "dep.js",
        "export const foo = 1; export const bar = 2; export function then() {}",
    );
    let result = run_log(
        "import defer * as ns from './dep.js';
         import * as eager from './dep.js';
         const keys = Reflect.ownKeys(ns).map(String).join();
         const tag = Object.getOwnPropertyDescriptor(ns, Symbol.toStringTag);
         const foo = Object.getOwnPropertyDescriptor(ns, 'foo');
         [
           keys,
           tag.value, tag.writable, tag.enumerable, tag.configurable,
           foo.value, foo.writable, foo.enumerable, foo.configurable,
           Object.getPrototypeOf(ns), Object.isExtensible(ns),
           ns === eager, eager[Symbol.toStringTag], 'then' in eager,
           Reflect.getOwnPropertyDescriptor(ns, 'nonexistent'),
         ].join('|')",
        &[dep],
    );
    // `then` is an export of the module but is left out of its *deferred*
    // namespace (so awaiting the namespace cannot trigger evaluation), while
    // the eager namespace keeps it.
    assert_eq!(
        result,
        Ok(string(
            "bar,foo,Symbol(Symbol.toStringTag)|Deferred Module|false|false|false|1|true|true|false||false|false|Module|true|"
        ))
    );
}

#[test]
fn deferred_namespaces_have_one_identity_per_module_however_they_are_reached() {
    let result = run_log(
        "import defer * as a from './dep.js';
         import defer * as b from './dep.js';
         import { viaDep } from './reexport.js';
         import * as eager from './dep.js';
         [a === b, a === viaDep, a === eager].join()",
        &[
            DEP,
            (
                "reexport.js",
                "import defer * as viaDep from './dep.js'; export { viaDep };",
            ),
        ],
    );
    assert_eq!(result, Ok(string("true,true,false")));
}

#[test]
fn deferred_sub_dependencies_are_evaluated_only_when_their_own_namespace_is_observed() {
    let result = run_log(
        "import defer * as ns1 from './dep-1.js';
         const start = evaluations.join();
         const ns12 = ns1.ns_1_2;
         const afterOuter = evaluations.join();
         ns1.ns_1_2;
         const again = evaluations.join();
         ns12.foo;
         start + '|' + afterOuter + '|' + again + '|' + evaluations.join()",
        &[
            (
                "dep-1.js",
                "import './dep-1.1.js';
                 import defer * as ns_1_2 from './dep-1.2.js';
                 evaluations.push('1'); export { ns_1_2 };",
            ),
            ("dep-1.1.js", "evaluations.push('1.1');"),
            (
                "dep-1.2.js",
                "evaluations.push('1.2'); export const foo = 1;",
            ),
        ],
    );
    assert_eq!(result, Ok(string("|1.1,1|1.1,1|1.1,1,1.2")));
}

#[test]
fn a_module_imported_both_deferred_and_eagerly_runs_at_its_eager_position() {
    let result = run_log(
        "import defer * as ns1 from './dep-1.js';
         import './dep-2.js';
         import './dep-1.js';
         evaluations.join()",
        &[
            ("dep-1.js", "import './dep-1.1.js'; evaluations.push('1');"),
            ("dep-1.1.js", "evaluations.push('1.1');"),
            ("dep-2.js", "evaluations.push('2');"),
        ],
    );
    assert_eq!(result, Ok(string("2,1.1,1")));
}

#[test]
fn an_evaluation_error_is_recorded_and_rethrown_identically() {
    let result = run_log(
        "import defer * as ns from './throws.js';
         let first, second;
         try { ns.foo; } catch (e) { first = e; }
         try { ns.foo; } catch (e) { second = e; }
         [first.someError, first === second, evaluations.join()].join()",
        &[(
            "throws.js",
            "evaluations.push('throws'); throw { someError: 'boom' };",
        )],
    );
    assert_eq!(result, Ok(string("boom,true,throws")));
}

#[test]
fn a_deferred_namespace_of_a_module_still_evaluating_throws_a_type_error() {
    let result = run_log(
        "import './dep.js'; evaluations.join()",
        &[(
            "dep.js",
            "import defer * as self from './dep.js';
             try { self.foo; } catch (e) { evaluations.push(e instanceof TypeError ? 'TypeError' : 'other'); }",
        )],
    );
    assert_eq!(result, Ok(string("TypeError")));
    // The same holds when the observing module is a dependency of a module
    // that is itself still evaluating.
    let result = run_log(
        "import './dep-1.js'; evaluations.join()",
        &[
            (
                "dep-1.js",
                "import defer * as dep2 from './dep-2.js';
                 globalThis.dep3evaluated = false;
                 try { dep2.foo; } catch (e) { evaluations.push(e instanceof TypeError ? 'TypeError' : 'other'); }
                 evaluations.push('dep3 ' + globalThis.dep3evaluated);",
            ),
            ("dep-2.js", "import './dep-3.js'; import './main.js';"),
            ("dep-3.js", "globalThis.dep3evaluated = true;"),
        ],
    );
    assert_eq!(result, Ok(string("TypeError,dep3 false")));
}

#[test]
fn asynchronous_dependencies_of_a_deferred_module_are_evaluated_eagerly() {
    // Only the frontier of asynchronous (top-level-await) dependencies runs
    // when the importer runs; the synchronous rest waits for the trigger.
    let result = run_log(
        "import defer * as ns from './imports-tla.js';
         const before = evaluations.join();
         ns.x;
         before + '|' + evaluations.join()",
        &[
            (
                "imports-tla.js",
                "import './tla.js'; evaluations.push('imports-tla'); export const x = 1;",
            ),
            (
                "tla.js",
                "evaluations.push('tla start'); await Promise.resolve(0); evaluations.push('tla end');",
            ),
        ],
    );
    assert_eq!(
        result,
        Ok(string("tla start,tla end|tla start,tla end,imports-tla"))
    );
}

#[test]
fn a_deferred_module_that_itself_awaits_is_not_deferred() {
    let result = run_log(
        "import defer * as ns from './tla.js';
         const before = evaluations.join();
         ns.x;
         before + '|' + evaluations.join()",
        &[(
            "tla.js",
            "evaluations.push('start'); await 0; evaluations.push('end'); export const x = 1;",
        )],
    );
    assert_eq!(result, Ok(string("start,end|start,end")));
}

#[test]
fn a_deferred_request_of_a_missing_module_fails_before_anything_is_evaluated() {
    let error = run_log(
        "import defer * as ns from './needs-missing.js'; evaluations.push('main');",
        &[("needs-missing.js", "import './missing.js';")],
    )
    .unwrap_err();
    assert!(
        matches!(error, RuntimeError::ModuleResolution(_)),
        "{error:?}"
    );
}

#[test]
fn a_dynamic_import_defer_resolves_with_the_same_deferred_namespace() {
    let result = run_log(
        "import defer * as staticNs from './dep.js';
         const dynamicNs = await import.defer('./dep.js');
         const before = evaluations.join();
         dynamicNs.exported;
         [dynamicNs === staticNs, before, evaluations.join(), dynamicNs[Symbol.toStringTag]].join('|')",
        &[DEP],
    );
    assert_eq!(result, Ok(string("true||dep|Deferred Module")));
}

#[test]
fn a_dynamic_import_defer_waits_for_asynchronous_dependencies_only() {
    let result = run_log(
        "const ns = await import.defer('./imports-tla.js');
         const before = evaluations.join();
         ns.x;
         before + '|' + evaluations.join()",
        &[
            (
                "imports-tla.js",
                "import './tla.js'; evaluations.push('imports-tla'); export const x = 1;",
            ),
            (
                "tla.js",
                "evaluations.push('tla start'); await Promise.resolve(0); evaluations.push('tla end');",
            ),
        ],
    );
    assert_eq!(
        result,
        Ok(string("tla start,tla end|tla start,tla end,imports-tla"))
    );
}

#[test]
fn a_dynamic_import_defer_of_a_synchronous_graph_evaluates_nothing() {
    let result = run_log(
        "const ns = await import.defer('./sync.js');
         const before = evaluations.join();
         ns.x;
         before + '|' + evaluations.join()",
        &[
            (
                "sync.js",
                "import './dep.js'; evaluations.push('sync'); export const x = 1;",
            ),
            DEP,
        ],
    );
    assert_eq!(result, Ok(string("|dep,sync")));
}

#[test]
fn a_deferred_dependency_waits_for_an_asynchronous_cycle_that_is_still_running() {
    // {a, b} is a cycle rooted at `a`, which awaits a blocker: `b` has run but
    // its cycle has not finished. `middle` defers `d`, which needs `b`, so
    // `middle` must wait for `a` rather than see `b` as already evaluated.
    let result = run(&[
        (
            "main.js",
            "import { aStarted } from './setup.js';
             const pA = import('./a.js');
             await aStarted.promise;
             const pC = import('./c.js');
             await Promise.all([pA, pC]);
             evaluations.join()",
        ),
        (
            "setup.js",
            "globalThis.evaluations = [];
             export const blocker = Promise.withResolvers();
             export const aStarted = Promise.withResolvers();",
        ),
        (
            "a.js",
            "import { blocker, aStarted } from './setup.js'; import './b.js';
             evaluations.push('A-before-await'); aStarted.resolve();
             await blocker.promise; evaluations.push('A-after-await');",
        ),
        ("b.js", "import './a.js'; evaluations.push('B');"),
        ("d.js", "import './b.js'; evaluations.push('D');"),
        (
            "middle.js",
            "import defer * as nsD from './d.js';
             evaluations.push('Middle-before'); nsD.z; evaluations.push('Middle-after');",
        ),
        (
            "blocker.js",
            "import { blocker } from './setup.js'; evaluations.push('resolve-blocker'); blocker.resolve();",
        ),
        (
            "c.js",
            "import './middle.js'; import './blocker.js'; evaluations.push('C');",
        ),
    ]);
    assert_eq!(
        result,
        Ok(string(
            "B,A-before-await,resolve-blocker,A-after-await,Middle-before,D,Middle-after,C"
        ))
    );
}

#[test]
fn a_deferred_edge_back_into_a_finished_asynchronous_module_does_not_block_its_trigger() {
    // `tla` awaits and imports `middle`, which defers `leaf`, which defers
    // `tla` again. Evaluation never walks the deferred edges, so `leaf` is not
    // part of `tla`'s dependency cycle: once `tla` has finished, observing the
    // deferred `leaf` must be allowed to evaluate it.
    let result = run(&[
        (
            "main.js",
            "import './setup.js';
             import './tla.js';
             import defer * as leaf from './leaf.js';
             leaf.value",
        ),
        ("setup.js", "globalThis.evaluations = [];"),
        (
            "tla.js",
            "import './middle.js'; await Promise.resolve(); evaluations.push('tla');",
        ),
        (
            "middle.js",
            "import defer * as leaf from './leaf.js'; evaluations.push('middle');",
        ),
        (
            "leaf.js",
            "import defer * as tla from './tla.js'; evaluations.push('leaf'); export const value = 1;",
        ),
    ]);
    assert_eq!(result, Ok(Value::Number(1.0)));
}
