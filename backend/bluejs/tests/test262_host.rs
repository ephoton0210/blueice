// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{compile, compile_module, parse, parse_module, RuntimeError, Value, Vm};
use std::collections::HashMap;

#[test]
fn harness_assertions_fail_closed() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    for source in [
        "assert(true)",
        "assert.sameValue(print('diagnostic'),undefined)",
        "assert.sameValue(NaN,NaN)",
        "assert.notSameValue(0,-0)",
        "assert.throws(TypeError,()=>''.repeat.call(null))",
        "assert.throws(RangeError,()=>''.repeat(-1))",
        "assert.throws(Error,()=>{throw new Error('x');})",
    ] {
        assert_eq!(
            vm.execute(&compile(&parse(source).unwrap()).unwrap())
                .unwrap(),
            Value::Undefined,
            "{source}"
        );
    }
    for source in [
        "assert(false)",
        "assert(1)",
        "assert.sameValue(0,-0)",
        "assert.notSameValue(NaN,NaN)",
        "assert.throws(TypeError,()=>1)",
        "assert.throws(TypeError,()=>{throw 1;})",
        "assert.throws(TypeError,()=>{throw new Error();})",
        "$DONOTEVALUATE()",
    ] {
        assert!(
            matches!(
                vm.execute(&compile(&parse(source).unwrap()).unwrap()),
                Err(RuntimeError::Test262(_))
            ),
            "{source}"
        );
    }
    assert!(matches!(
        Vm::default().execute(&compile(&parse("assert(true)").unwrap()).unwrap()),
        Err(RuntimeError::ReferenceError(_))
    ));
}

#[test]
fn test262_agents_share_bytes_wait_and_report_in_notify_order() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = r#"
        let buffer = new SharedArrayBuffer(12);
        let ints = new Int32Array(buffer);
        $262.agent.start(`
            $262.agent.receiveBroadcast(function (shared) {
                let view = new Int32Array(shared);
                Atomics.add(view, 0, 1);
                $262.agent.report(Atomics.wait(view, 1, 0, 10000));
                $262.agent.leaving();
            });
        `);
        $262.agent.broadcast(buffer);
        while (Atomics.load(ints, 0) !== 1) {}
        $262.agent.sleep(10);
        Atomics.notify(ints, 1, 1) === 1
    "#;
    assert_eq!(
        vm.execute(&compile(&parse(source).unwrap()).unwrap()),
        Ok(Value::Bool(true))
    );
    let report = compile(&parse("$262.agent.sleep(10);$262.agent.getReport()").unwrap()).unwrap();
    assert_eq!(vm.execute(&report), Ok(Value::String("ok".into())));
    assert_eq!(vm.shutdown_test262_agents(), Ok(()));
}

#[test]
fn test262_agents_broadcast_before_wait_preserves_shared_spin_protocol() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = r#"
        let buffer = new SharedArrayBuffer(12);
        let ints = new Int32Array(buffer);
        $262.agent.start(`
            $262.agent.receiveBroadcast(function (shared) {
                let view = new Int32Array(shared);
                Atomics.add(view, 2, 1);
                while (Atomics.load(view, 1) === 0) {}
                $262.agent.report(7);
                Atomics.wait(view, 0, 0);
                $262.agent.report(8);
                $262.agent.leaving();
            });
        `);
        $262.agent.broadcast(buffer);
        while (Atomics.load(ints, 2) !== 1) {}
        Atomics.store(ints, 1, 1);
        $262.agent.sleep(10);
        Atomics.notify(ints, 0, 1) === 1
    "#;
    assert_eq!(
        vm.execute(&compile(&parse(source).unwrap()).unwrap()),
        Ok(Value::Bool(true))
    );
    let report = compile(
        &parse("$262.agent.sleep(10);$262.agent.getReport()+$262.agent.getReport()").unwrap(),
    )
    .unwrap();
    assert_eq!(vm.execute(&report), Ok(Value::String("78".into())));
    assert_eq!(vm.shutdown_test262_agents(), Ok(()));
}

#[test]
fn test262_agents_receive_each_broadcast_in_host_order() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = r#"
        let first = new SharedArrayBuffer(4);
        let second = new SharedArrayBuffer(4);
        Atomics.store(new Int32Array(first), 0, 1);
        Atomics.store(new Int32Array(second), 0, 2);
        $262.agent.start(`
            let values = [];
            $262.agent.receiveBroadcast(shared => {
                values.push(Atomics.load(new Int32Array(shared), 0));
            });
            $262.agent.receiveBroadcast(shared => {
                values.push(Atomics.load(new Int32Array(shared), 0));
            });
            $262.agent.report(values.join(','));
            $262.agent.leaving();
        `);
        $262.agent.broadcast(first);
        $262.agent.broadcast(second);
        let report = null;
        while ((report = $262.agent.getReport()) === null) {
            $262.agent.sleep(1);
        }
        report
    "#;
    assert_eq!(
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap()),
        Ok(Value::String("1,2".into()))
    );
    vm.shutdown_test262_agents().unwrap();
}

#[test]
fn test262_agents_notify_wakes_fifo_waiters() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = r#"
        let buffer = new SharedArrayBuffer(20);
        let ints = new Int32Array(buffer);
        for (let i = 0; i < 3; i++) {
            $262.agent.start(`
                $262.agent.receiveBroadcast(function (shared) {
                    let view = new Int32Array(shared);
                    Atomics.add(view, 4, 1);
                    while (Atomics.load(view, 1 + ${i}) === 0) {}
                    $262.agent.report(${i});
                    Atomics.wait(view, 0, 0);
                    $262.agent.report(${i});
                    $262.agent.leaving();
                });
            `);
        }
        $262.agent.broadcast(buffer);
        while (Atomics.load(ints, 4) !== 3) {}
        for (let i = 0; i < 3; i++) {
            Atomics.store(ints, 1 + i, 1);
            $262.agent.sleep(10);
            $262.agent.getReport();
        }
        Atomics.notify(ints, 0, 1) + Atomics.notify(ints, 0, 1) + Atomics.notify(ints, 0, 1)
    "#;
    assert_eq!(
        vm.execute(&compile(&parse(source).unwrap()).unwrap()),
        Ok(Value::Number(3.0))
    );
    vm.shutdown_test262_agents().unwrap();
}

#[test]
fn test262_agents_atomic_read_modify_write_does_not_lose_updates() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = r#"
        const workers = 8;
        const rounds = 200;
        let buffer = new SharedArrayBuffer(8);
        let view = new Int32Array(buffer);
        for (let worker = 0; worker < workers; worker++) {
            $262.agent.start(`
                $262.agent.receiveBroadcast(shared => {
                    let agent_view = new Int32Array(shared);
                    for (let round = 0; round < ${rounds}; round++) {
                        Atomics.add(agent_view, 0, 1);
                    }
                    Atomics.add(agent_view, 1, 1);
                    $262.agent.leaving();
                });
            `);
        }
        $262.agent.broadcast(buffer);
        while (Atomics.load(view, 1) !== workers) {}
        Atomics.load(view, 0)
    "#;
    assert_eq!(
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap()),
        Ok(Value::Number(1_600.0))
    );
    vm.shutdown_test262_agents().unwrap();
}

#[test]
fn atomics_wait_async_returns_a_promise_and_settles_on_the_vm_thread() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    vm.install_test262_done().unwrap();
    let source = r#"
        assert.sameValue(typeof Atomics.waitAsync, 'function');
        let view = new Int32Array(new SharedArrayBuffer(4));
        let { async, value } = Atomics.waitAsync(view, 0, 0, 1_000);
        assert.sameValue(async, true);
        assert(value instanceof Promise);
        assert.sameValue(Object.getPrototypeOf(value), Promise.prototype);
        value.then(status => {
            assert.sameValue(status, 'ok');
        }).then(() => $DONE(), $DONE);
        Atomics.add(view, 0, 1);
        Atomics.notify(view, 0, 1);
    "#;
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap();
    assert_eq!(vm.run_test262_async_until_done(), Ok(Some(Ok(()))));
}

#[test]
fn async_completion_returns_timeout_after_the_event_loop_becomes_quiescent() {
    let mut vm = Vm::default();
    vm.install_test262_done().unwrap();
    vm.execute_script(&compile(&parse("1").unwrap()).unwrap())
        .unwrap();

    assert_eq!(vm.run_test262_async_until_done(), Ok(None));
}

#[test]
fn async_completion_waits_for_a_registered_host_timer() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    vm.install_test262_done().unwrap();
    vm.execute_script(&compile(&parse("setTimeout($DONE, 1)").unwrap()).unwrap())
        .unwrap();

    assert_eq!(vm.run_test262_async_until_done(), Ok(Some(Ok(()))));
}

#[test]
fn test262_agents_wait_async_registers_two_waiters_before_notify() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = r#"
        let buffer = new SharedArrayBuffer(12);
        let view = new Int32Array(buffer);
        for (let label of ['A', 'B']) {
            $262.agent.start(`
                $262.agent.receiveBroadcast(async function (shared) {
                    let agent_view = new Int32Array(shared);
                    Atomics.add(agent_view, 1, 1);
                    let wait = Atomics.waitAsync(agent_view, undefined, 0);
                    Atomics.add(agent_view, 2, 1);
                    $262.agent.report("${label} " + await wait.value);
                    $262.agent.leaving();
                });
            `);
        }
        $262.agent.broadcast(buffer);
        while (Atomics.load(view, 2) !== 2) {}
        Atomics.notify(view, 0, 2)
    "#;
    assert_eq!(
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap()),
        Ok(Value::Number(2.0))
    );
    let reports = compile(
        &parse("$262.agent.sleep(10); [$262.agent.getReport(), $262.agent.getReport()].sort().join(',')")
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        vm.execute_script(&reports),
        Ok(Value::String("A ok,B ok".into()))
    );
    vm.shutdown_test262_agents().unwrap();
}

#[test]
fn test262_agent_reports_immediate_bigint_wait_async_result() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = r#"
        let buffer = new SharedArrayBuffer(32);
        let view = new BigInt64Array(buffer);
        $262.agent.start(`
            $262.agent.receiveBroadcast(function (shared) {
                let agent_view = new BigInt64Array(shared);
                Atomics.add(agent_view, 1, 1n);
                $262.agent.report(Atomics.store(agent_view, 0, 42n));
                $262.agent.report(Atomics.waitAsync(agent_view, 0, 0n).value);
                $262.agent.leaving();
            });
        `);
        $262.agent.broadcast(buffer);
        while (Atomics.load(view, 1) !== 1n) {}
        $262.agent.sleep(10);
        [$262.agent.getReport(), $262.agent.getReport()].join(',')
    "#;
    assert_eq!(
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap()),
        Ok(Value::String("42,not-equal".into()))
    );
    vm.shutdown_test262_agents().unwrap();
}

#[test]
fn generated_regexp_class_escape_helper_preserves_regexp_verdicts() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    for source in [
        "__bluejsTest262RegExpClassEscape([/^a+$/, /^a+$/], 'aaa', true)",
        "__bluejsTest262RegExpClassEscape([/b/, /c/], 'aaa', false)",
        "__bluejsTest262RegExpClassEscape([/^\\D+$/, /^\\D+$/u, /^\\D+$/v], String.fromCodePoint(0x10000, 0x10001), true)",
    ] {
        assert_eq!(
            vm.execute(&compile(&parse(source).unwrap()).unwrap()),
            Ok(Value::Bool(true)),
            "{source}"
        );
    }
    assert!(matches!(
        vm.execute(
            &compile(&parse("__bluejsTest262RegExpClassEscape([/a/], 'aaa', false)").unwrap())
                .unwrap()
        ),
        Err(RuntimeError::Test262(_))
    ));
}

#[test]
fn typed_array_overlap_helper_uses_the_real_set_operation() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = "let bytes=new Uint8Array(32);let doubles=new Float64Array(bytes.buffer,0,4);__bluejsTest262TypedArrayOverlappingSet(bytes,doubles)";
    assert_eq!(
        vm.execute(&compile(&parse(source).unwrap()).unwrap()),
        Ok(Value::Bool(true))
    );
}

#[test]
fn single_modules_are_strict_and_do_not_publish_classic_globals() {
    let module = compile_module(
        &parse_module("var hidden=1;let local=2;let arrow=()=>this;typeof this==='undefined'&&typeof arrow()==='undefined'&&hidden===1&&local===2")
            .unwrap(),
    )
    .unwrap();
    let mut vm = Vm::default();
    assert_eq!(vm.execute_module(&module), Ok(Value::Bool(true)));

    let lookup =
        compile(&parse("typeof hidden==='undefined'&&typeof local==='undefined'").unwrap())
            .unwrap();
    assert_eq!(vm.execute_script(&lookup), Ok(Value::Bool(true)));

    let strict_with = parse_module("with({}){}").unwrap();
    assert!(compile_module(&strict_with).is_err());
}

#[test]
fn module_parser_enforces_top_level_lexical_and_export_early_errors() {
    for source in [
        "function duplicate(){}function duplicate(){}",
        "var collision;function collision(){}",
        "import { value as eval } from './dependency.js';",
        "import { value as arguments } from './dependency.js';",
        "class Name{}export default function Name(){}",
        "export { 'local' as 'public' };function local(){}",
        "if (true) { import value from './dependency.js'; }",
        "function nested() { export default 1; }",
    ] {
        assert!(parse_module(source).is_err(), "{source}");
    }
    assert!(parse_module("class Name{}export default function Other(){}").is_ok());
    assert!(parse_module("export { local as 'public' };function local(){}").is_ok());
    assert!(parse_module("import('./dependency.js')").is_ok());
    assert!(parse_module("import value from './dependency.js' with { type: 'json', }; export * from './dependency.js' with { type: 'json' }").is_ok());
    assert!(parse_module(
        "import value from './dependency.js' with { type: 'json', 'type': 'json' }"
    )
    .is_err());

    for source in [
        "export default var value = 1;",
        "export default let value = 1;",
        "export default const value = 1;",
        "export default function() {}();",
        "export default function*() {}();",
        "?",
        "0++;",
        "<!-- a legacy script comment",
        "\n-->",
    ] {
        let error = parse_module(source).unwrap_err();
        assert!(error.known_syntax, "{source}: {error:?}");
    }
}

#[test]
fn module_graph_links_named_imports_as_live_bindings_before_evaluation() {
    let sources = [
        (
            "module/main.js",
            "import { value, bump } from './dependency.js'; bump(); value === 2",
        ),
        (
            "module/dependency.js",
            "export let value = 1; export function bump(){ value = 2; }",
        ),
    ];
    let modules: HashMap<_, _> = sources
        .into_iter()
        .map(|(name, source)| {
            (
                name.to_string(),
                compile_module(&parse_module(source).unwrap()).unwrap(),
            )
        })
        .collect();
    assert_eq!(
        Vm::default().execute_module_graph("module/main.js", &modules),
        Ok(Value::Bool(true))
    );
}

#[test]
fn module_graph_evaluates_interleaved_import_and_export_requests_in_source_order() {
    let sources = [
        (
            "order/main.js",
            "import './one.js';export {} from './two.js';import './three.js';globalThis.order==='123'",
        ),
        ("order/one.js", "globalThis.order='1'"),
        ("order/two.js", "globalThis.order+='2'"),
        ("order/three.js", "globalThis.order+='3'"),
    ];
    let modules: HashMap<_, _> = sources
        .into_iter()
        .map(|(name, source)| {
            (
                name.to_string(),
                compile_module(&parse_module(source).unwrap()).unwrap(),
            )
        })
        .collect();
    assert_eq!(
        Vm::default().execute_module_graph("order/main.js", &modules),
        Ok(Value::Bool(true))
    );
}

#[test]
fn dynamic_import_observes_existing_static_dfs_evaluation() {
    let root = "language/module-code/";
    let modules = [
        (
            format!("{root}verify-dfs.js"),
            include_str!("../../../development/browser_core/reference/test262/test/language/module-code/verify-dfs.js"),
        ),
        (
            format!("{root}verify-dfs-a_FIXTURE.js"),
            include_str!("../../../development/browser_core/reference/test262/test/language/module-code/verify-dfs-a_FIXTURE.js"),
        ),
        (
            format!("{root}verify-dfs-b_FIXTURE.js"),
            include_str!("../../../development/browser_core/reference/test262/test/language/module-code/verify-dfs-b_FIXTURE.js"),
        ),
    ]
    .into_iter()
    .map(|(name, source)| {
        (
            name,
            compile_module(&parse_module(source).unwrap()).unwrap(),
        )
    })
    .collect();
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    vm.install_test262_done().unwrap();
    vm.execute_module_graph(&format!("{root}verify-dfs.js"), &modules)
        .unwrap();
    vm.run_promise_jobs().unwrap();
    assert_eq!(vm.take_test262_done(), Some(Ok(())));
}

#[test]
fn module_graph_links_default_function_exports() {
    let source = "let before=f; import f from './main.js'; export default function fName(){return 23;}; before()===23&&f.name==='fName'";
    let modules = HashMap::from([(
        "default/main.js".to_string(),
        compile_module(&parse_module(source).unwrap()).unwrap(),
    )]);
    assert_eq!(
        Vm::default().execute_module_graph("default/main.js", &modules),
        Ok(Value::Bool(true))
    );
}

#[test]
fn anonymous_default_exports_infer_default_without_an_inner_binding() {
    for source in [
        "export default function(){} import value from './main.js';value.name==='default'",
        "export default (function(){});import value from './main.js';value.name==='default'",
        "export default class{} import value from './main.js';value.name==='default'",
        "export default (()=>{});import value from './main.js';value.name==='default'",
    ] {
        let modules = HashMap::from([(
            "default-name/main.js".to_string(),
            compile_module(&parse_module(source).unwrap()).unwrap(),
        )]);
        assert_eq!(
            Vm::default().execute_module_graph("default-name/main.js", &modules),
            Ok(Value::Bool(true)),
            "{source}"
        );
    }
}

#[test]
fn module_graph_keeps_a_thrown_error_alive_for_the_embedding_host() {
    let modules = HashMap::from([(
        "module-error/main.js".to_string(),
        compile_module(&parse_module("throw new Test262Error('expected')").unwrap()).unwrap(),
    )]);
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let Err(RuntimeError::Thrown(Value::Object(error))) =
        vm.execute_module_graph("module-error/main.js", &modules)
    else {
        panic!("module should have thrown its Test262Error");
    };
    assert_eq!(
        vm.heap().get(error, "name").unwrap(),
        Value::String("Test262Error".into())
    );
}

#[test]
fn module_namespace_properties_follow_export_cells() {
    let sources = [
        (
            "namespace/main.js",
            "import * as ns from './dependency.js'; ns.bump(); ns.value === 2",
        ),
        (
            "namespace/dependency.js",
            "export let value=1; export function bump(){value=2}",
        ),
    ];
    let modules: HashMap<_, _> = sources
        .into_iter()
        .map(|(name, source)| {
            (
                name.to_string(),
                compile_module(&parse_module(source).unwrap()).unwrap(),
            )
        })
        .collect();
    assert_eq!(
        Vm::default().execute_module_graph("namespace/main.js", &modules),
        Ok(Value::Bool(true))
    );
}

#[test]
fn module_namespace_has_exotic_descriptor_symbol_and_integrity_semantics() {
    let sources = [
        (
            "namespace-exotic/main.js",
            "import * as ns from './dependency.js';let descriptor=Object.getOwnPropertyDescriptor(ns,'z');let names=Object.getOwnPropertyNames(ns);let symbols=Object.getOwnPropertySymbols(ns);let rejected=false;let frozen=false;try{ns.z=0}catch(error){rejected=error instanceof TypeError}try{Object.freeze(ns)}catch(error){frozen=error instanceof TypeError}Object.getPrototypeOf(ns)===null&&!Object.isExtensible(ns)&&Object.preventExtensions(ns)===ns&&Object.isSealed(ns)&&!Object.isFrozen(ns)&&Reflect.defineProperty(ns,'z',{value:3})&&!Reflect.defineProperty(ns,'z',{value:4})&&!Reflect.set(ns,'z',0)&&!Reflect.deleteProperty(ns,'z')&&frozen&&descriptor.value===3&&descriptor.writable&&descriptor.enumerable&&!descriptor.configurable&&names.length===2&&names[0]==='a'&&names[1]==='z'&&symbols.length===1&&Object.prototype.toString.call(ns)==='[object Module]'&&rejected&&Reflect.get(ns,'z')===3&&ns.z===3",
        ),
        (
            "namespace-exotic/dependency.js",
            "export const z=3;export const a=1",
        ),
    ];
    let modules: HashMap<_, _> = sources
        .into_iter()
        .map(|(name, source)| {
            (
                name.to_string(),
                compile_module(&parse_module(source).unwrap()).unwrap(),
            )
        })
        .collect();
    assert_eq!(
        Vm::default().execute_module_graph("namespace-exotic/main.js", &modules),
        Ok(Value::Bool(true))
    );
}

#[test]
fn dynamic_import_resolves_against_the_module_registry_in_a_promise_job() {
    let sources = [
        (
            "dynamic/main.js",
            "import('./dependency.js').then(ns=>{if(ns.value===42)$DONE();else $DONE(new Error('wrong namespace'))})",
        ),
        ("dynamic/dependency.js", "export const value=42"),
    ];
    let modules: HashMap<_, _> = sources
        .into_iter()
        .map(|(name, source)| {
            (
                name.to_string(),
                compile_module(&parse_module(source).unwrap()).unwrap(),
            )
        })
        .collect();
    let mut vm = Vm::default();
    vm.install_test262_done().unwrap();
    assert!(vm.execute_module_graph("dynamic/main.js", &modules).is_ok());
    assert_eq!(vm.take_test262_done(), None);
    vm.run_promise_jobs().unwrap();
    assert_eq!(vm.take_test262_done(), Some(Ok(())));
}

#[test]
fn async_test_style_promise_chain_observes_an_async_function() {
    let mut vm = Vm::default();
    vm.install_test262_done().unwrap();
    let source = "function asyncTest(test){test().then(function(){$DONE()},function(error){$DONE(error)})}asyncTest(async function(){})";
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap();
    vm.run_promise_jobs().unwrap();
    assert_eq!(vm.take_test262_done(), Some(Ok(())));
}

#[test]
fn promise_constructor_invokes_its_executor_and_settles_once() {
    let mut vm = Vm::default();
    vm.install_test262_done().unwrap();
    let source = "let count=0;let promise=new Promise(function(resolve,reject){resolve(42);reject(new Error('late'))});promise.then(function(value){if(value===42&&count===0){count++;$DONE()}else{$DONE(new Test262Error('wrong fulfillment'))}},function(error){$DONE(error)})";
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap();
    vm.run_promise_jobs().unwrap();
    assert_eq!(vm.take_test262_done(), Some(Ok(())));

    let rejected = compile(
        &parse("new Promise(function(){throw new RangeError('expected')}).then(null,function(error){if(error instanceof RangeError)$DONE();else $DONE(error)})")
            .unwrap(),
    )
    .unwrap();
    vm.execute_script(&rejected).unwrap();
    vm.run_promise_jobs().unwrap();
    assert_eq!(vm.take_test262_done(), Some(Ok(())));

    let capability = compile(
        &parse("let capability=Promise.withResolvers();let values=[];capability.promise.then(function(value){values.push(value);if(values.length===1&&values[0]===7)$DONE();else $DONE(new Test262Error('wrong capability value'))});capability.resolve(7);capability.reject(new Error('late'))")
            .unwrap(),
    )
    .unwrap();
    vm.execute_script(&capability).unwrap();
    vm.run_promise_jobs().unwrap();
    assert_eq!(vm.take_test262_done(), Some(Ok(())));

    let aggregate = compile(
        &parse("let first=Promise.withResolvers();let second=Promise.withResolvers();Promise.all([first.promise,second.promise]).then(function(values){if(values[0]===1&&values[1]===2)$DONE();else $DONE(new Test262Error('wrong Promise.all values'))},$DONE);second.resolve(2);first.resolve(1)")
            .unwrap(),
    )
    .unwrap();
    vm.execute_script(&aggregate).unwrap();
    vm.run_promise_jobs().unwrap();
    assert_eq!(vm.take_test262_done(), Some(Ok(())));
}

#[test]
fn promise_static_methods_observe_constructor_resolve_and_then_failures() {
    for source in [
        "Promise.resolve(1).then(function(value){if(value===1)$DONE();else $DONE(new Test262Error('wrong fulfillment'))},$DONE)",
        "let resolve;let reject;let promise=new Promise(function(r,j){resolve=r;reject=j});let P=function(executor){executor(resolve,reject);return promise};Promise.resolve.call(P,promise).then(function(){$DONE(new Test262Error('fulfilled'))},function(error){if(error.constructor===TypeError)$DONE();else $DONE(error)})",
        "let error=new Test262Error('resolve getter');Object.defineProperty(Promise,'resolve',{get:function(){throw error}});Promise.all([new Promise(function(){})]).then(function(){$DONE(new Test262Error('fulfilled'))},function(reason){if(reason===error)$DONE();else $DONE(reason)})",
        "let promise=new Promise(function(){});let error=new Test262Error('then method');Object.defineProperty(promise,'then',{value:function(){throw error}});Promise.all([promise]).then(function(){$DONE(new Test262Error('fulfilled'))},function(reason){if(reason===error)$DONE();else $DONE(reason)})",
    ] {
        let mut vm = Vm::default();
        vm.install_test262_harness().unwrap();
        vm.install_test262_done().unwrap();
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap();
        vm.run_promise_jobs().unwrap();
        assert_eq!(vm.take_test262_done(), Some(Ok(())), "{source}");
    }
}

#[test]
fn async_test_style_chain_handles_a_rejected_dynamic_import() {
    let modules = HashMap::from([(
        "async-import/broken.js".to_string(),
        compile_module(
            &parse_module("import source value from '<missing>'; export { value };").unwrap(),
        )
        .unwrap(),
    )]);
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    vm.install_test262_done().unwrap();
    vm.set_module_loader_context("async-import/main.js", modules);
    let source = "function asyncTest(test){if(!Object.prototype.hasOwnProperty.call(globalThis,'$DONE')){throw new Test262Error()}if(typeof test!=='function'){$DONE(new Test262Error());return}try{test().then(function(){$DONE()},function(error){$DONE(error)})}catch(error){$DONE(error)}}function expectImportFailure(specifier){return import(specifier).then(function(){throw new Test262Error()},function(error){if(error instanceof SyntaxError){throw new Test262Error()}})}asyncTest(async function(){await expectImportFailure('./broken.js');await expectImportFailure('./broken.js');await expectImportFailure('./broken.js')})";
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap();
    vm.run_promise_jobs().unwrap();
    assert_eq!(vm.take_test262_done(), Some(Ok(())));
}

#[test]
fn source_and_defer_dynamic_imports_reject_through_the_promise_path() {
    let modules = HashMap::from([(
        "source-dynamic/empty.js".to_string(),
        compile_module(&parse_module("export {};").unwrap()).unwrap(),
    )]);
    for source in [
        "import.source('./empty.js').then(()=>{$DONE(new Error())},error=>{if(error instanceof SyntaxError)$DONE();else $DONE(error)})",
        "import.defer({toString(){throw 'expected'}}).then(()=>{$DONE(new Error())},error=>{if(error==='expected')$DONE();else $DONE(error)})",
    ] {
        let mut vm = Vm::default();
        vm.install_test262_done().unwrap();
        vm.set_module_loader_context("source-dynamic/main.js", modules.clone());
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap();
        vm.run_promise_jobs().unwrap();
        assert_eq!(vm.take_test262_done(), Some(Ok(())), "{source}");
    }
}

#[test]
fn async_helpers_accept_source_phase_host_resolution_rejections() {
    let fixture_root = "language/module-code/source-phase-import/";
    let modules = HashMap::from([
        (
            format!("{fixture_root}import-source-binding-name_FIXTURE.js"),
            compile_module(&parse_module(include_str!(
                "../../../development/browser_core/reference/test262/test/language/module-code/source-phase-import/import-source-binding-name_FIXTURE.js"
            ))
            .unwrap())
            .unwrap(),
        ),
        (
            format!("{fixture_root}import-source-binding-name-2_FIXTURE.js"),
            compile_module(&parse_module(include_str!(
                "../../../development/browser_core/reference/test262/test/language/module-code/source-phase-import/import-source-binding-name-2_FIXTURE.js"
            ))
            .unwrap())
            .unwrap(),
        ),
        (
            format!("{fixture_root}import-source-newlines_FIXTURE.js"),
            compile_module(&parse_module(include_str!(
                "../../../development/browser_core/reference/test262/test/language/module-code/source-phase-import/import-source-newlines_FIXTURE.js"
            ))
            .unwrap())
            .unwrap(),
        ),
        (
            format!("{fixture_root}ensure-linking-error_FIXTURE.js"),
            compile_module(&parse_module(include_str!(
                "../../../development/browser_core/reference/test262/test/language/module-code/source-phase-import/ensure-linking-error_FIXTURE.js"
            ))
            .unwrap())
            .unwrap(),
        ),
    ]);
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    vm.install_test262_done().unwrap();
    vm.set_module_loader_context(format!("{fixture_root}import-source.js"), modules);
    vm.execute_script(
        &compile(
            &parse(include_str!(
                "../../../development/browser_core/reference/test262/harness/asyncHelpers.js"
            ))
            .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        vm.execute_script(&compile(&parse("typeof asyncTest").unwrap()).unwrap()),
        Ok(Value::String("function".into()))
    );
    vm.execute_script(
        &compile(&parse(include_str!(
            "../../../development/browser_core/reference/test262/test/language/module-code/source-phase-import/import-source.js"
        ))
        .unwrap())
        .unwrap(),
    )
    .unwrap();
    vm.run_promise_jobs().unwrap();
    assert_eq!(vm.take_test262_done(), Some(Ok(())));
}

#[test]
fn top_level_await_observes_fulfilled_and_rejected_async_completions() {
    for source in [
        "async function value(){return 42}export let observed=await value();observed===42",
        "async function fail(){throw new TypeError('expected')}let caught=false;try{await fail()}catch(error){caught=error instanceof TypeError}caught",
    ] {
        let module = compile_module(&parse_module(source).unwrap()).unwrap();
        let modules = HashMap::from([("await/main.js".to_string(), module)]);
        assert_eq!(
            Vm::default().execute_module_graph("await/main.js", &modules),
            Ok(Value::Bool(true)),
            "{source}"
        );
    }
}

#[test]
fn top_level_await_assimilates_thenables_through_the_job_queue() {
    for source in [
        "let thenable={then(resolve){resolve(42)}};await thenable===42",
        "let marker={};let thenable={then(){throw marker}};let caught;try{await thenable}catch(error){caught=error}caught===marker",
    ] {
        let module = compile_module(&parse_module(source).unwrap()).unwrap();
        let modules = HashMap::from([("thenable/main.js".to_string(), module)]);
        assert_eq!(
            Vm::default().execute_module_graph("thenable/main.js", &modules),
            Ok(Value::Bool(true)),
            "{source}"
        );
    }
}

#[test]
fn top_level_await_drains_a_dynamic_import_job_and_resumes_the_module() {
    let sources = [
        (
            "await-import/main.js",
            "let namespace=await import('./dependency.js');namespace.value===42",
        ),
        ("await-import/dependency.js", "export const value=42"),
    ];
    let modules: HashMap<_, _> = sources
        .into_iter()
        .map(|(name, source)| {
            (
                name.to_string(),
                compile_module(&parse_module(source).unwrap()).unwrap(),
            )
        })
        .collect();
    assert_eq!(
        Vm::default().execute_module_graph("await-import/main.js", &modules),
        Ok(Value::Bool(true))
    );
}

#[test]
fn module_graph_initializes_function_exports_before_cyclic_evaluation() {
    let sources = [
        (
            "cycle/a.js",
            "import { readB } from './b.js'; export function readA(){return 'a';} export let seen = readB();",
        ),
        (
            "cycle/b.js",
            "import { readA } from './a.js'; export function readB(){return readA();}",
        ),
    ];
    let modules: HashMap<_, _> = sources
        .into_iter()
        .map(|(name, source)| {
            (
                name.to_string(),
                compile_module(&parse_module(source).unwrap()).unwrap(),
            )
        })
        .collect();
    let mut vm = Vm::default();
    assert_eq!(
        vm.execute_module_graph("cycle/a.js", &modules),
        Ok(Value::Undefined)
    );
    let observer = compile(&parse("typeof seen === 'undefined'").unwrap()).unwrap();
    assert_eq!(vm.execute_script(&observer), Ok(Value::Bool(true)));
}

#[test]
fn module_graph_keeps_indirect_exports_and_import_immutability() {
    let sources = [
        (
            "indirect/main.js",
            "import { B, results } from './bridge.js'; let initial=B===undefined; export var A=99; let immutable=false; try{B=null}catch(error){immutable=error instanceof TypeError;} initial&&B===99&&immutable&&results.length===0",
        ),
        (
            "indirect/bridge.js",
            "export { A as B } from './main.js'; export const results=[]; try{A}catch(error){}try{B}catch(error){}",
        ),
        ("indirect/unused.js", "export var unused=0"),
    ];
    let modules: HashMap<_, _> = sources
        .into_iter()
        .map(|(name, source)| {
            (
                name.to_string(),
                compile_module(&parse_module(source).unwrap()).unwrap(),
            )
        })
        .collect();
    assert_eq!(
        Vm::default().execute_module_graph("indirect/main.js", &modules),
        Ok(Value::Bool(true))
    );
}

#[test]
fn module_graph_reexports_import_bindings_without_losing_their_origin() {
    let sources = [
        (
            "reexport/consumer.js",
            "import { value } from './bridge.js'; import { namespace } from './namespace-bridge.js'; value===42&&namespace.value===42",
        ),
        (
            "reexport/bridge.js",
            "import { value } from './dependency.js'; export { value };",
        ),
        (
            "reexport/namespace-bridge.js",
            "import * as namespace from './dependency.js'; export { namespace };",
        ),
        ("reexport/dependency.js", "export const value=42;"),
    ];
    let modules: HashMap<_, _> = sources
        .into_iter()
        .map(|(name, source)| {
            (
                name.to_string(),
                compile_module(&parse_module(source).unwrap()).unwrap(),
            )
        })
        .collect();
    assert_eq!(
        Vm::default().execute_module_graph("reexport/consumer.js", &modules),
        Ok(Value::Bool(true))
    );
}

#[test]
fn module_graph_deduplicates_identical_star_reexport_bindings() {
    let sources = [
        (
            "star-reexport/main.js",
            "export * from './through-export.js'; export * from './through-import.js'; import { value } from './main.js'; value===42",
        ),
        (
            "star-reexport/through-export.js",
            "export { value } from './dependency.js';",
        ),
        (
            "star-reexport/through-import.js",
            "import { value } from './dependency.js'; export { value };",
        ),
        ("star-reexport/dependency.js", "export const value=42;"),
    ];
    let modules: HashMap<_, _> = sources
        .into_iter()
        .map(|(name, source)| {
            (
                name.to_string(),
                compile_module(&parse_module(source).unwrap()).unwrap(),
            )
        })
        .collect();
    assert_eq!(
        Vm::default().execute_module_graph("star-reexport/main.js", &modules),
        Ok(Value::Bool(true))
    );
}

#[test]
fn module_namespace_handles_a_self_namespace_reexport_without_recursing() {
    let modules = HashMap::from([(
        "self-namespace/main.js".to_string(),
        compile_module(
            &parse_module(
                "import * as namespace from './main.js';export * as self from './main.js';export const value=42;namespace.self===namespace",
            )
            .unwrap(),
        )
        .unwrap(),
    )]);
    assert_eq!(
        Vm::default().execute_module_graph("self-namespace/main.js", &modules),
        Ok(Value::Bool(true))
    );
}

#[test]
fn module_graph_rejects_invalid_indirect_exports_before_evaluation() {
    let sources = [
        (
            "invalid-export/main.js",
            "$DONOTEVALUATE(); export { value } from './ambiguous.js';",
        ),
        (
            "invalid-export/ambiguous.js",
            "export * from './left.js'; export * from './right.js';",
        ),
        ("invalid-export/left.js", "export const value=1;"),
        ("invalid-export/right.js", "export const value=2;"),
    ];
    let modules: HashMap<_, _> = sources
        .into_iter()
        .map(|(name, source)| {
            (
                name.to_string(),
                compile_module(&parse_module(source).unwrap()).unwrap(),
            )
        })
        .collect();
    assert!(matches!(
        Vm::default().execute_module_graph("invalid-export/main.js", &modules),
        Err(RuntimeError::ModuleResolution(_))
    ));
}

#[test]
fn module_graph_links_source_phase_imports_without_evaluating_the_source_record() {
    let sources = [
        (
            "source-phase/main.js",
            "import { source } from './bridge.js'; typeof source==='object'&&source instanceof $262.AbstractModuleSource",
        ),
        (
            "source-phase/bridge.js",
            "import source source from '<module source>'; export { source };",
        ),
    ];
    let modules: HashMap<_, _> = sources
        .into_iter()
        .map(|(name, source)| {
            (
                name.to_string(),
                compile_module(&parse_module(source).unwrap()).unwrap(),
            )
        })
        .collect();
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    vm.set_module_source_loader_context(vec!["<module source>".to_string()]);
    assert_eq!(
        vm.execute_module_graph("source-phase/main.js", &modules),
        Ok(Value::Bool(true))
    );
}

#[test]
fn error_constructors_and_host_globals() {
    for source in [
        "new TypeError('x').toString() === 'TypeError: x'",
        "Error().toString() === 'Error' && new Error('').message === ''",
        "new RangeError().name === 'RangeError' && new TypeError() instanceof Error",
        "let e=new Error('a',{cause:42}); e.cause === 42 && !Object.getOwnPropertyDescriptor(e,'cause').enumerable",
        "globalThis.hostValue=42; hostValue === 42 && typeof hostValue === 'number' && typeof absent === 'undefined'",
    ] {
        assert_eq!(Vm::default().execute(&compile(&parse(source).unwrap()).unwrap()).unwrap(), Value::Bool(true), "{source}");
    }
}

#[test]
fn object_prevent_extensions_exposes_the_heap_internal_method() {
    for source in [
        "(function(){let object={present:1};return Object.preventExtensions(object)===object&&!Object.isExtensible(object)&&object.present===1&&Object.preventExtensions(1)===1;})()",
        "(function(){'use strict';let object={};Object.preventExtensions(object);try{object.added=1;return false;}catch(error){return error instanceof TypeError;}})()",
        "({own:1}).hasOwnProperty('own')&&!({own:1}).hasOwnProperty('missing')",
    ] {
        assert_eq!(
            Vm::default()
                .execute(&compile(&parse(source).unwrap()).unwrap())
                .unwrap(),
            Value::Bool(true),
            "{source}"
        );
    }
}

#[test]
fn frozen_test262_global_keeps_error_constructors_available() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = "Object.preventExtensions(globalThis);let caught=false;try{eval('var unavailable')}catch(error){caught=error instanceof TypeError;}caught";
    assert_eq!(
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn create_realm_detached_eval_uses_an_isolated_global() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = "var other=$262.createRealm().global;var otherEval=other.eval;otherEval('var x=23;');typeof x==='undefined'&&other.x===23";
    assert_eq!(
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn create_realm_membrane_preserves_foreign_object_identity_and_internal_slots() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = "var other=$262.createRealm().global;var value=other.eval('var state=0;({get value(){state++;return state}})');var regexp=other.eval('/a/g');value.value===1&&other.eval('state')===1&&RegExp(regexp)!==regexp&&RegExp(regexp).toString()==='/a/g'";
    assert_eq!(
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn create_realm_eval_exposes_a_callable_completion_without_foreign_heap_handles() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = "var other=$262.createRealm().global;var fn=other.eval('(0, async function* () {})');var prototype=Object.getPrototypeOf(fn.prototype);fn.prototype=undefined;Object.getPrototypeOf(fn())===prototype";
    assert_eq!(
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn sloppy_global_eval_annex_b_function_does_not_block_a_later_lexical() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source =
        "eval('if(true){function test262Fn(){}}');$262.evalScript('let test262Fn=1');test262Fn===1";
    assert_eq!(
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn global_function_declarations_follow_existing_property_rules() {
    for source in [
        "Object.defineProperty(globalThis,'replaceable',{value:0,configurable:true});Object.preventExtensions(globalThis);$262.evalScript('function replaceable(){}');let descriptor=Object.getOwnPropertyDescriptor(globalThis,'replaceable');typeof replaceable==='function'&&descriptor.writable&&descriptor.enumerable&&!descriptor.configurable",
        "Object.defineProperty(globalThis,'incompatible',{value:0,writable:false,enumerable:true,configurable:false});(function(){try{$262.evalScript('var mustNotExist;function incompatible(){}');return false;}catch(error){return error instanceof TypeError&&typeof mustNotExist==='undefined';}})()",
    ] {
        let mut vm = Vm::default();
        vm.install_test262_harness().unwrap();
        assert_eq!(
            vm.execute(&compile(&parse(source).unwrap()).unwrap()).unwrap(),
            Value::Bool(true),
            "{source}"
        );
    }
}

#[test]
fn sloppy_block_functions_keep_their_lexical_binding_and_update_the_outer_var() {
    let mut vm = Vm::default();
    let source = "var initial,current;{function f(){initial=f;f=123;current=f;return 'block';}}f();initial()==='block'&&current===123&&f()==='block'";
    assert_eq!(
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        vm.execute_script(&compile(&parse("typeof f").unwrap()).unwrap())
            .unwrap(),
        Value::String("function".into())
    );

    let mut strict_vm = Vm::default();
    assert_eq!(
        strict_vm
            .execute_script(
                &compile(&parse("'use strict';{function hidden(){}}typeof hidden").unwrap())
                    .unwrap()
            )
            .unwrap(),
        Value::String("undefined".into())
    );
}

#[test]
fn sloppy_if_clause_functions_use_the_annex_b_synthetic_block() {
    let source = "var initial,current;if(true)function f(){initial=f;f=123;current=f;return 'if';}f();initial()==='if'&&current===123&&f()==='if'";
    assert_eq!(
        Vm::default()
            .execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn sloppy_block_functions_cross_a_simple_catch_parameter() {
    let source =
        "try{throw null;}catch(f){{function f(){return 1;}}}typeof f==='function'&&f()===1";
    assert_eq!(
        Vm::default()
            .execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn class_declaration_bindings_are_mutable_lexicals() {
    assert_eq!(
        Vm::default()
            .execute_script(
                &compile(&parse("class Declaration{};Declaration=1;Declaration===1").unwrap())
                    .unwrap(),
            )
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn global_lexicals_shadow_configurable_intrinsic_properties() {
    let mut vm = Vm::default();
    assert_eq!(
        vm.execute_script(
            &compile(
                &parse("let Array;let descriptor=Object.getOwnPropertyDescriptor(globalThis,'Array');Array===undefined&&typeof globalThis.Array==='function'&&descriptor.configurable&&!descriptor.enumerable&&descriptor.writable")
                    .unwrap(),
            )
            .unwrap(),
        )
        .unwrap(),
        Value::Bool(true)
    );
    vm.install_test262_harness().unwrap();
    assert!(matches!(
        vm.execute(&compile(&parse("$262.evalScript('let undefined')").unwrap()).unwrap()),
        Err(RuntimeError::SyntaxError(_))
    ));
}

#[test]
fn function_constructor_compiles_global_source_without_capturing_caller_bindings() {
    for source in [
        "let hidden=1;let fn=Function('return this;');fn()===globalThis&&Function('a','b','return a+b;')(2,3)===5&&Function('return typeof hidden;')()==='undefined'",
        "Function('<!--','')()===undefined&&Function('\\n-->','')()===undefined&&Function('a','//','')()===undefined&&(function(){try{Function('-->','');return false}catch(error){return error instanceof SyntaxError}})()",
        "(function(){try{Function('return )');return false;}catch(error){return error instanceof SyntaxError;}})()",
    ] {
        assert_eq!(
            Vm::default()
                .execute(&compile(&parse(source).unwrap()).unwrap())
                .unwrap(),
            Value::Bool(true),
            "{source}"
        );
    }
}

#[test]
fn function_intrinsic_graph_and_restricted_properties_follow_the_realm_contract() {
    let source = r#"
        let caller = Object.getOwnPropertyDescriptor(Function.prototype, 'caller');
        let arguments = Object.getOwnPropertyDescriptor(Function.prototype, 'arguments');
        let callerThrows = false;
        let argumentsThrows = false;
        try { Function.prototype.caller; } catch (error) { callerThrows = error instanceof TypeError; }
        try { Function.prototype.arguments = 1; } catch (error) { argumentsThrows = error instanceof TypeError; }
        let constructed = new Function('return 7;');
        let strict = Function('"use strict"; return 1;');
        let AsyncFunction = Object.getPrototypeOf(async function() {}).constructor;
        let dynamicAsync = AsyncFunction('return 1;');
        let object = { method() {} };
        let names = Object.getOwnPropertyNames(Function);
        let lengthIndex = names.indexOf('length');
        let nameIndex = names.indexOf('name');
        Object.getPrototypeOf(Function) === Function.prototype &&
          Function.prototype.constructor === Function &&
          Function.prototype.isPrototypeOf(Function) &&
          constructed.constructor === Function && constructed() === 7 &&
          constructed.caller === undefined && constructed.arguments === null &&
          !strict.hasOwnProperty('caller') && !strict.hasOwnProperty('arguments') &&
          !dynamicAsync.hasOwnProperty('caller') && !dynamicAsync.hasOwnProperty('arguments') &&
          !object.method.hasOwnProperty('caller') && !object.method.hasOwnProperty('arguments') &&
          caller.get === caller.set && caller.enumerable === false && caller.configurable === true &&
          arguments.get === caller.get && arguments.set === caller.set &&
          callerThrows && argumentsThrows &&
          lengthIndex >= 0 && nameIndex === lengthIndex + 1 &&
          Object.prototype.isPrototypeOf.call(null, 0) === false
    "#;
    assert_eq!(
        Vm::default()
            .execute(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn construct_enforces_derived_return_and_revoked_proxy_realm_contracts() {
    let source = r#"
        let derivedReturnThrows = false;
        let derivedThisThrows = false;
        let revokedProxyThrows = false;
        try { new (class extends Object { constructor() { return null; } })(); }
        catch (error) { derivedReturnThrows = error instanceof TypeError; }
        try { new (class extends Object { constructor() {} })(); }
        catch (error) { derivedThisThrows = error instanceof ReferenceError; }
        let handle;
        handle = Proxy.revocable(function() {}, { get() { handle.revoke(); } });
        try { new handle.proxy(); }
        catch (error) { revokedProxyThrows = error instanceof TypeError; }
        derivedReturnThrows && derivedThisThrows && revokedProxyThrows
    "#;
    assert_eq!(
        Vm::default()
            .execute(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn class_call_error_uses_the_class_realm() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = r#"
        let other = $262.createRealm().global;
        let C = other.eval('(class {})');
        let foreignTypeError = other.TypeError;
        let caught;
        try { C(); } catch (error) { caught = error; }
        caught.constructor === foreignTypeError && caught.constructor !== TypeError
    "#;
    assert_eq!(
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn foreign_function_calls_keep_error_and_constructor_prototype_realms() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = r#"
        let other = $262.createRealm().global;
        let foreignFunction = new other.Function();
        let foreignError = false;
        try { foreignFunction.apply(null, false); }
        catch (error) { foreignError = error.constructor === other.TypeError; }

        let C = new other.Function();
        C.prototype = null;
        let localFunction = Reflect.construct(Function, [], C);
        let localConstructor = Reflect.construct(function() {}.bind(), [], C);
        let localObject = Reflect.construct(Object, [], C);
        let localIdentity = {};
        let echo = new other.Function('return this;');
        let transportedIdentity = echo.call(localIdentity) === localIdentity;
        let compareTransported = new other.Function(
          'return arguments[0] === arguments[1];'
        );
        let transportedAlias = compareTransported(localIdentity, localIdentity);

        let realmA = $262.createRealm().global;
        let realmB = $262.createRealm().global;
        let newTarget = new realmB.Function();
        newTarget.prototype = null;
        let foreignFunctionWithForeignTarget = Reflect.construct(
          realmA.Function, [''], newTarget
        );
        let boundForeignTarget = realmA.Function.prototype.bind.call(newTarget);
        let boundForeignObject = Reflect.construct(realmA.Object, [], boundForeignTarget);
        let realmAObject = new realmA.Function('return {};')();
        let realmBIdentity = new realmB.Function('return this;');
        let crossRealmTransportedIdentity =
          realmBIdentity.call(realmAObject) === realmAObject;

        foreignError &&
          Object.getPrototypeOf(foreignFunction) === other.Function.prototype &&
          Object.getPrototypeOf(localFunction) === other.Function.prototype &&
          Object.getPrototypeOf(localConstructor) === other.Object.prototype &&
          Object.getPrototypeOf(localObject) === other.Object.prototype &&
          transportedIdentity &&
          transportedAlias &&
          Object.getPrototypeOf(foreignFunctionWithForeignTarget) === realmB.Function.prototype &&
          Object.getPrototypeOf(foreignFunctionWithForeignTarget.prototype) === realmA.Object.prototype &&
          Object.getPrototypeOf(boundForeignObject) === realmB.Object.prototype &&
          crossRealmTransportedIdentity
    "#;
    assert_eq!(
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn foreign_proxy_revocable_retains_caller_realm_target_and_handler() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = r#"
        let other = $262.createRealm().global;
        let handle;
        handle = other.Proxy.revocable(function() {}, {
            get() { handle.revoke(); }
        });
        let revoked = false;
        try { new handle.proxy(); }
        catch (error) { revoked = error instanceof TypeError; }
        revoked
    "#;
    assert_eq!(
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn foreign_proxy_and_constructor_membranes_preserve_realms() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = r#"
        let other = $262.createRealm();
        let proxy = other.global.eval(
          'new Proxy(function() {}, { apply(_, __, args) { return args; } })'
        );
        let arguments = proxy();

        let realm1 = $262.createRealm().global;
        let realm2 = $262.createRealm().global;
        let realm3 = $262.createRealm().global;
        let newTarget = new realm1.Function();
        newTarget.prototype = false;
        let newTargetProxy = new realm2.Proxy(newTarget, {});
        let array = Reflect.construct(realm3.Array, [], newTargetProxy);

        let foreignThrow = other.evalScript(`
          (function() {
            let handle = Proxy.revocable(function() {}, {});
            handle.revoke();
            return handle.proxy();
          })
        `);
        let foreignError = false;
        try { foreignThrow(); }
        catch (error) { foreignError = error.constructor === other.global.TypeError; }

        arguments.constructor === Array &&
          Object.getPrototypeOf(arguments) === Array.prototype &&
          array instanceof realm1.Array &&
          Object.getPrototypeOf(array) === realm1.Array.prototype &&
          foreignError
    "#;
    assert_eq!(
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn reflect_construct_rejects_date_now_as_a_non_constructor() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = r#"
        typeof Date === 'function' &&
          typeof Date.now === 'function' &&
          (function() {
            try { Reflect.construct(Date.now, []); }
            catch (error) { return error instanceof TypeError; }
            return false;
          })()
    "#;
    assert_eq!(
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn is_html_dda_host_object_keeps_strict_equality_ordinary() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    vm.install_test262_is_html_dda().unwrap();
    for source in [
        "(function(){let value=$262.IsHTMLDDA;return !value;})()",
        "(function(){let value=$262.IsHTMLDDA;return value==null&&value==undefined;})()",
        "(function(){let value=$262.IsHTMLDDA;return typeof value==='undefined';})()",
        "(function(){let value=$262.IsHTMLDDA;switch(value){case undefined:return 1;case null:return 2;case value:return 3;}})()===3",
    ] {
        assert_eq!(
            vm.execute(&compile(&parse(source).unwrap()).unwrap())
                .unwrap(),
            Value::Bool(true),
            "{source}"
        );
    }
}

#[test]
fn harness_compares_arrays_and_propagates_resource_errors() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    for source in [
        "assert.compareArray([NaN,-0],[NaN,-0])",
        "assert.sameValue(assert._isSameValue(0,-0),false)",
        "assert.throws(ReferenceError,()=>missing)",
        "assert.throws(SyntaxError,()=>new RegExp('['))",
        "assert.throws(Test262Error,()=>assert(false))",
    ] {
        assert_eq!(
            vm.execute(&compile(&parse(source).unwrap()).unwrap())
                .unwrap(),
            Value::Undefined,
            "{source}"
        );
    }
    for source in [
        "assert.throws(TypeError,1)",
        "assert.compareArray([1],[1,2])",
        "assert.compareArray([0],[-0])",
    ] {
        assert!(
            matches!(
                vm.execute(&compile(&parse(source).unwrap()).unwrap()),
                Err(RuntimeError::Test262(_))
            ),
            "{source}"
        );
    }
    assert!(matches!(
        vm.execute(
            &compile(&parse("assert.throws(RangeError,()=>''.padStart(10000000))").unwrap())
                .unwrap()
        ),
        Err(RuntimeError::StringLimit { .. })
    ));
    assert_eq!(
        vm.execute(&compile(&parse("typeof Test262Error").unwrap()).unwrap())
            .unwrap(),
        Value::String("function".into())
    );
    for source in [
        "Error.prototype.toString.call({name:'',message:'x'}) === 'x'",
        "new Error('x',{}).message === 'x'",
        "Error.prototype.toString.call({}) === 'Error'",
    ] {
        assert_eq!(
            vm.execute(&compile(&parse(source).unwrap()).unwrap())
                .unwrap(),
            Value::Bool(true),
            "{source}"
        );
    }
    assert!(matches!(
        vm.execute(&compile(&parse("Error.prototype.toString.call(1)").unwrap()).unwrap()),
        Err(RuntimeError::TypeError(_))
    ));
}

#[test]
fn native_uri_decode_fixture_helper_exhaustively_checks_the_shared_decode_operation() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    for source in [
        "__bluejsTest262DecodeUriExhaustive(decodeURI,3)",
        "__bluejsTest262DecodeUriExhaustive(decodeURIComponent,3)",
        "assert.throws(TypeError,()=>__bluejsTest262DecodeUriExhaustive(decodeURI,2));true",
    ] {
        assert_eq!(
            vm.execute(&compile(&parse(source).unwrap()).unwrap())
                .unwrap(),
            Value::Bool(true),
            "{source}"
        );
    }
}

#[test]
fn complete_core_harness_helpers_are_available() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    for source in [
        "assert(isPrimitive(null) && !isPrimitive({}) && isNegativeZero(-0) && !isNegativeZero(0))",
        "assert(compareArray([1],[1]) && !compareArray([1],[2]) && !compareArray([],[1]))",
        "assert.sameValue(compareArray.format([1,'x',Symbol('s')]),'[1, x, Symbol(s)]')",
        "assert.sameValue(formatIdentityFreeValue({}),undefined)",
        "assert.sameValue(formatIdentityFreeValue('x'),'\"x\"')",
        "assert.sameValue(formatIdentityFreeValue(-0),'-0')",
        "assert.sameValue(formatSimpleValue(Symbol('s')),'Symbol(s)')",
        "assert.sameValue(formatSimpleValue({toString(){return 'x';}}),'x')",
        "assert.sameValue(formatSimpleValue({toString:0,valueOf:0}),'[object Object]')",
        "assert.throws(Test262Error,()=>assert.compareArray('x','x'))",
        "assert.throws(TypeError,()=>formatSimpleValue({toString(){throw new TypeError();}}))",
    ] {
        assert_eq!(
            vm.execute(&compile(&parse(source).unwrap()).unwrap())
                .unwrap(),
            Value::Undefined,
            "{source}"
        );
    }
}

#[test]
fn native_property_helpers_validate_descriptors_and_constructibility() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    for source in [
        "verifyProperty(Math,'PI',{value:Math.PI,writable:false,enumerable:false,configurable:false});true",
        "verifyCallableProperty(Math,'abs','abs',1);true",
        "verifyPrimordialCallableProperty(Math,'abs','abs',1);true",
        "verifyEqualTo(Math,'PI',Math.PI);true",
        "verifyNotWritable(Math,'PI');verifyNotEnumerable(Math,'PI');verifyNotConfigurable(Math,'PI');true",
        "verifyWritable(Math,'abs');verifyEnumerable({x:1},'x');verifyConfigurable({x:1},'x');true",
        "verifyPrimordialProperty(Math,'PI',{value:Math.PI,writable:false,enumerable:false,configurable:false});true",
        "let o={};Object.defineProperty(o,'x',{get:function getter(){return 1},set:undefined,enumerable:false,configurable:true});verifyAccessorProperty(o,'x',{get:Object.getOwnPropertyDescriptor(o,'x').get,set:undefined});verifyPrimordialAccessorProperty(o,'x',{get:Object.getOwnPropertyDescriptor(o,'x').get,set:undefined});true",
        "isConstructor(function(){}) && !isConstructor(()=>{})",
        "assert.throws(Test262Error,()=>isConstructor(1));assert.throws(Test262Error,()=>verifyProperty(Math,'PI',undefined));true",
        "assert.throws(TypeError,()=>verifyProperty(1,'x',{}));true",
        "verifyCallableProperty(Math,'abs','abs',1,{writable:true,enumerable:false,configurable:true});true",
        "verifyCallableProperty(Math,'abs',undefined,1);verifyCallableProperty(Math,'abs','abs',1,{writable:true,enumerable:false});true",
        "assert.throws(Test262Error,()=>verifyCallableProperty(Math,'abs','wrong',1));true",
        "let o={};Object.defineProperty(o,Symbol.iterator,{value:function(){},writable:true,enumerable:false,configurable:true});assert.throws(Test262Error,()=>verifyCallableProperty(o,Symbol.iterator,undefined,0));true",
        "let f=function f(){};Object.defineProperty(f,'name',{configurable:false});let o={};Object.defineProperty(o,'f',{value:f,writable:true,enumerable:false,configurable:true});assert.throws(Test262Error,()=>verifyCallableProperty(o,'f','f',0,{writable:true,enumerable:false,configurable:true}));assert.throws(Test262Error,()=>verifyCallableProperty(o,'f','f',0));true",
        "let o={};Object.defineProperty(o,'x',{get:function(){return 1},enumerable:false,configurable:true});assert.throws(Test262Error,()=>verifyAccessorProperty(o,'x',{get:undefined}));assert.throws(Test262Error,()=>verifyAccessorProperty(o,'x',{get:Object.getOwnPropertyDescriptor(o,'x').get,enumerable:true}));assert.throws(Test262Error,()=>verifyAccessorProperty(Math,'PI',{}));true",
        "assert.throws(Test262Error,()=>verifyProperty(Math,'PI',{unknown:1}));true",
    ] {
        assert_eq!(vm.execute(&compile(&parse(source).unwrap()).unwrap()).unwrap(), Value::Bool(true), "{source}");
    }
    let failure = compile(&parse("verifyProperty(Math,'PI',{writable:true})").unwrap()).unwrap();
    assert!(matches!(
        vm.execute(&failure),
        Err(RuntimeError::Test262(_))
    ));
    for source in [
        "verifyCallableProperty(Math,'PI','PI',0)",
        "let o={};Object.defineProperty(o,'f',{value:function f(){},writable:false,enumerable:false,configurable:true});verifyCallableProperty(o,'f','f',0)",
        "let o={};Object.defineProperty(o,Symbol.iterator,{value:function(){},writable:true,enumerable:false,configurable:true});verifyCallableProperty(o,Symbol.iterator,undefined,0)",
        "let f=function f(){};Object.defineProperty(f,'name',{configurable:false});let o={};Object.defineProperty(o,'f',{value:f,writable:true,enumerable:false,configurable:true});verifyCallableProperty(o,'f','f',0,{writable:true,enumerable:false,configurable:true})",
    ] {
        let mut vm = Vm::default();
        vm.install_test262_harness().unwrap();
        assert!(matches!(vm.execute(&compile(&parse(source).unwrap()).unwrap()), Err(RuntimeError::Test262(_))), "{source}");
    }
    let source = "let o={};Object.defineProperty(o,'x',{get:function(){return 1},set:undefined,enumerable:false,configurable:true});verifyProperty(o,'x',{get:Object.getOwnPropertyDescriptor(o,'x').get,set:undefined})";
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        vm.execute(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn harness_allocation_failures_leave_the_vm_usable() {
    use blueice_bluejs::{HeapConfig, HeapError, VmConfig};
    let alive = compile(&parse("1+1").unwrap()).unwrap();
    for ceiling in (64000..125000).step_by(251) {
        let mut vm = Vm::new(VmConfig {
            heap: HeapConfig {
                nursery_capacity: 1,
                major_threshold_bytes: 256,
                max_heap_bytes: ceiling,
            },
            ..Default::default()
        })
        .unwrap();
        match vm.install_test262_harness() {
            Ok(()) => {}
            Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { .. })) => {}
            error => panic!("{error:?}"),
        }
        assert_eq!(vm.execute(&alive).unwrap(), Value::Number(2.0));
    }
}

#[test]
fn classic_scripts_publish_var_function_and_lexical_bindings_in_one_realm() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let harness =
        compile(&parse("var offset=4; function addOffset(value){return value+offset}").unwrap())
            .unwrap();
    assert_eq!(vm.execute_script(&harness).unwrap(), Value::Undefined);
    let test =
        compile(&parse("addOffset(3) === 7 && typeof offset === 'number'").unwrap()).unwrap();
    assert_eq!(vm.execute_script(&test).unwrap(), Value::Bool(true));
    let lexical = compile(&parse("let secret=1; const hidden=2").unwrap()).unwrap();
    assert_eq!(vm.execute_script(&lexical).unwrap(), Value::Undefined);
    let lookup = compile(&parse("secret === 1 && hidden === 2").unwrap()).unwrap();
    assert_eq!(vm.execute(&lookup).unwrap(), Value::Bool(true));
}

#[test]
fn classic_scripts_keep_global_var_and_lexical_bindings_live_across_scripts() {
    let mut vm = Vm::default();
    let script = |source: &str| compile(&parse(source).unwrap()).unwrap();

    assert_eq!(
        vm.execute_script(&script(
            "var counter=1;function readCounter(){return counter}let lexical=4;const fixed=7;function readLexical(){return lexical+fixed}",
        ))
        .unwrap(),
        Value::Undefined
    );
    assert_eq!(
        vm.execute_script(&script("counter=2;globalThis.counter=3;counter"))
            .unwrap(),
        Value::Number(3.0)
    );
    assert_eq!(
        vm.execute_script(&script("lexical=5;lexical")).unwrap(),
        Value::Number(5.0)
    );
    assert_eq!(
        vm.execute_script(&script("readCounter()")),
        Ok(Value::Number(3.0))
    );
    assert_eq!(
        vm.execute_script(&script("readLexical()")),
        Ok(Value::Number(12.0))
    );
    assert_eq!(
        vm.execute_script(&script("globalThis.lexical")),
        Ok(Value::Undefined)
    );
    assert!(matches!(
        vm.execute_script(&script("let lexical=0")),
        Err(RuntimeError::SyntaxError(_))
    ));
    assert!(matches!(
        vm.execute_script(&script("fixed=0")),
        Err(RuntimeError::TypeError(_))
    ));
    assert_eq!(
        vm.execute_script(&script("readCounter()===3&&readLexical()===12"))
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn test262_eval_script_enters_the_current_realm_without_discarding_the_caller() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let evaluate =
        |source: &str, vm: &mut Vm| vm.execute(&compile(&parse(source).unwrap()).unwrap());

    assert_eq!(
        evaluate(
            "$262.evalScript('var shared=1;function readShared(){return shared};let lexical=4;const fixed=7');shared=2;globalThis.shared=3;lexical=5;readShared()===3&&lexical+fixed===12&&globalThis.lexical===undefined",
            &mut vm,
        ),
        Ok(Value::Bool(true))
    );
    assert_eq!(
        evaluate("$262.evalScript('6')", &mut vm),
        Ok(Value::Number(6.0))
    );
    assert_eq!(
        evaluate(
            "let caught=false;try{$262.evalScript('const malformed =')}catch(error){caught=error instanceof SyntaxError;}caught",
            &mut vm,
        ),
        Ok(Value::Bool(true))
    );
}
