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
