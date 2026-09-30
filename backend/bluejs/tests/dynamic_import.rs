// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Dynamic `import()` from classic script code through parse, compile and
//! the VM's module registry: the referrer an ImportCall resolves against must
//! survive suspension boundaries (async generators resumed from jobs), and
//! an import of an already-evaluated member of an errored cycle observes the
//! cycle's recorded evaluation error. An `import()` that starts while the
//! entry module is still evaluating shares that module graph.

use blueice_bluejs::{compile, compile_module, parse, parse_module, Value, Vm, VmConfig};
use std::collections::HashMap;

/// Runs `script` as the classic-script entry `dir/main.js` with `modules`
/// (keyed by their `dir/`-relative canonical names) as the host registry and
/// returns what the script reported through `$DONE`.
fn run_script(modules: &[(&str, &str)], script: &str) -> Option<Result<(), String>> {
    run_script_with(VmConfig::default(), modules, script)
}

fn run_script_with(
    config: VmConfig,
    modules: &[(&str, &str)],
    script: &str,
) -> Option<Result<(), String>> {
    let registry: HashMap<_, _> = modules
        .iter()
        .map(|(name, source)| {
            (
                format!("dir/{name}"),
                compile_module(&parse_module(source).unwrap_or_else(|e| panic!("{name}: {e:?}")))
                    .unwrap(),
            )
        })
        .collect();
    let mut vm = Vm::new(config).unwrap();
    vm.install_test262_done().unwrap();
    vm.set_module_loader_context("dir/main.js", registry);
    vm.execute_script(&compile(&parse(script).unwrap()).unwrap())
        .unwrap();
    vm.run_promise_jobs().unwrap();
    vm.take_test262_done()
        .map(|done| done.map_err(|error| format!("{error:?}")))
}

const TRACE: &str = "
    var trace = [];
    function finish(ok) { if (ok) $DONE(); else $DONE(new Error(trace.join(' | '))); }
";

#[test]
fn async_generator_resumed_from_jobs_keeps_its_import_referrer() {
    // Only the first `import()` runs synchronously inside the script; the
    // later ones run when the generator is resumed from a Promise job, where
    // no ambient script/module identity exists.
    let script = format!(
        "{TRACE}
        async function* gen() {{
            yield await import('./a.js');
            yield await import('./b.js');
            yield await import('./c.js');
        }}
        var it = gen();
        (async () => {{
            var p1 = it.next(), p2 = it.next(), p3 = it.next();
            trace.push((await p1).value.x);
            trace.push((await p2).value.x);
            trace.push((await p3).value.x);
        }})().then(() => finish(trace.join(',') === '1,2,3'), e => (trace.push(String(e)), finish(false)));"
    );
    let done = run_script(
        &[
            ("a.js", "export var x = 1;"),
            ("b.js", "export var x = 2;"),
            ("c.js", "export var x = 3;"),
        ],
        &script,
    );
    assert_eq!(done, Some(Ok(())));
}

#[test]
fn plain_generators_and_async_functions_keep_their_import_referrer_across_resumption() {
    let script = format!(
        "{TRACE}
        async function f() {{
            await null;
            var a = await import('./a.js');
            await null;
            var b = await import('./b.js');
            return a.x + b.x;
        }}
        function* g() {{ yield import('./a.js'); yield import('./b.js'); }}
        var it = g();
        var first = it.next().value, second = it.next().value;
        Promise.all([f(), first, second]).then(
            ([sum, a, b]) => finish(sum === 3 && a.x === 1 && b.x === 2),
            e => (trace.push(String(e)), finish(false)));"
    );
    let done = run_script(
        &[("a.js", "export var x = 1;"), ("b.js", "export var x = 2;")],
        &script,
    );
    assert_eq!(done, Some(Ok(())));
}

#[test]
fn importing_an_evaluated_member_of_an_errored_cycle_rejects_with_the_recorded_error() {
    // {a, b, c} is a cycle entered at b; only b throws (after its await).
    // The import of `c` redirects to its cycle root and observes b's error.
    let script = format!(
        "{TRACE}
        (async () => {{
            var fromMain = null;
            try {{ await import('./main.js'); }} catch (e) {{ fromMain = e; }}
            trace.push('main: ' + (fromMain && fromMain.message));
            var fromC = null;
            try {{ await import('./c.js'); }} catch (e) {{ fromC = e; }}
            trace.push('c same error: ' + (fromC === fromMain));
            finish(fromMain !== null && fromC === fromMain
                && fromMain.message === 'async error in B');
        }})().catch(e => (trace.push(String(e)), finish(false)));"
    );
    let done = run_script(
        &[
            ("main.js", "import './b.js'; import './x.js';"),
            ("a.js", "import './b.js'; await Promise.resolve(0);"),
            (
                "b.js",
                "import './c.js'; await Promise.resolve(0); throw new Error('async error in B');",
            ),
            ("c.js", "import './a.js'; await Promise.resolve(0);"),
            ("x.js", "import './a.js'; await Promise.resolve(0);"),
        ],
        &script,
    );
    assert_eq!(done, Some(Ok(())));
}

#[test]
fn importing_an_unrelated_evaluated_module_still_fulfills_after_a_sibling_failed() {
    let script = format!(
        "{TRACE}
        (async () => {{
            try {{ await import('./bad.js'); trace.push('bad resolved'); }} catch (e) {{ trace.push('bad rejected'); }}
            var ok = await import('./good.js');
            finish(ok.x === 7 && trace.join() === 'bad rejected');
        }})().catch(e => (trace.push(String(e)), finish(false)));"
    );
    let done = run_script(
        &[
            (
                "bad.js",
                "await Promise.resolve(0); throw new Error('bad');",
            ),
            ("good.js", "export var x = 7;"),
        ],
        &script,
    );
    assert_eq!(done, Some(Ok(())));
}

#[test]
fn a_dynamic_import_started_while_the_entry_evaluates_shares_the_entry_graph() {
    // `other.js` imports `setup.js` too; it must see the very module instance
    // the still-evaluating entry linked, not a second copy of it.
    let sources = [
        (
            "main.js",
            "import { shared } from './setup.js';
             const ns = await import('./other.js');
             shared.value === 1 && ns.seen === shared",
        ),
        ("setup.js", "export const shared = { value: 0 };"),
        (
            "other.js",
            "import { shared } from './setup.js'; shared.value = 1; export const seen = shared;",
        ),
    ];
    let modules: HashMap<_, _> = sources
        .into_iter()
        .map(|(name, source)| {
            (
                format!("dir/{name}"),
                compile_module(&parse_module(source).unwrap()).unwrap(),
            )
        })
        .collect();
    assert_eq!(
        Vm::default().execute_module_graph("dir/main.js", &modules),
        Ok(Value::Bool(true))
    );
}

#[test]
fn import_source_of_a_source_text_module_rejects_with_a_syntax_error() {
    // The host supplies no Module Source object for a Source Text Module, so
    // the promise rejects instead of linking or evaluating the module.
    let script = format!(
        "{TRACE}
        import.source('./a.js').then(
            () => finish(false),
            e => finish(e instanceof SyntaxError));"
    );
    let done = run_script(
        &[("a.js", "globalThis.ran = true; export var x = 1;")],
        &script,
    );
    assert_eq!(done, Some(Ok(())));
}

#[test]
fn import_source_evaluates_its_arguments_like_import_and_rejects_instead_of_throwing() {
    let script = format!(
        "{TRACE}
        var boom = {{ toString() {{ throw new RangeError('specifier'); }} }};
        var results = [
            import.source(boom).then(() => 'resolved', e => e.name),
            import.source('./a.js', 42).then(() => 'resolved', e => e.name),
            import.source('./a.js', {{ with: {{ type: 1 }} }}).then(() => 'resolved', e => e.name),
            import.defer(boom).then(() => 'resolved', e => e.name),
            import.defer('./a.js', 42).then(() => 'resolved', e => e.name),
            import('./a.js', {{ with: 7 }}).then(() => 'resolved', e => e.name),
        ];
        Promise.all(results).then(
            names => finish(names.join() === 'RangeError,TypeError,TypeError,RangeError,TypeError,TypeError'),
            e => (trace.push(String(e)), finish(false)));"
    );
    let done = run_script(&[("a.js", "export var x = 1;")], &script);
    assert_eq!(done, Some(Ok(())));
}

#[test]
fn import_defer_from_a_script_resolves_relative_to_the_script_and_defers_evaluation() {
    let script = format!(
        "{TRACE}
        globalThis.log = [];
        import.defer('./a.js').then(ns => {{
            trace.push('before ' + log.join());
            var x = ns.x;
            trace.push('after ' + log.join() + ' x=' + x);
            finish(trace.join('|') === 'before |after a x=1');
        }}, e => (trace.push(String(e)), finish(false)));"
    );
    let done = run_script(&[("a.js", "log.push('a'); export var x = 1;")], &script);
    assert_eq!(done, Some(Ok(())));
}

#[test]
fn a_module_that_threw_keeps_its_error_for_every_later_import() {
    // [[EvaluationError]] is recorded once: the second import must observe the
    // very same error object, without evaluating the module body again.
    let script = format!(
        "{TRACE}
        globalThis.runs = 0;
        (async () => {{
            var first, second;
            try {{ await import('./throws.js'); }} catch (e) {{ first = e; }}
            try {{ await import('./throws.js'); }} catch (e) {{ second = e; }}
            finish(first instanceof Error && first === second && runs === 1);
        }})().catch(e => (trace.push(String(e)), finish(false)));"
    );
    let done = run_script(
        &[("throws.js", "runs++; throw new Error('boom');")],
        &script,
    );
    assert_eq!(done, Some(Ok(())));
}

#[test]
fn import_operands_survive_collection_while_the_specifier_is_coerced() {
    // A one-object nursery collects at nearly every allocation. The returned
    // promise and both operands are only reachable from the import call itself
    // while `toString` and the options getter run and allocate.
    let mut config = VmConfig::default();
    config.heap.nursery_capacity = 1;
    let script = format!(
        "{TRACE}
        var specifier = {{ toString() {{ for (var i = 0; i < 20; i++) [{{}}, [], () => i]; return './a.js'; }} }};
        var options = {{ get with() {{ for (var i = 0; i < 20; i++) [{{}}, [], () => i]; return {{}}; }} }};
        import(specifier, options).then(
            ns => finish(ns.x === 1),
            e => (trace.push(String(e)), finish(false)));"
    );
    let done = run_script_with(config, &[("a.js", "export var x = 1;")], &script);
    assert_eq!(done, Some(Ok(())));
}
