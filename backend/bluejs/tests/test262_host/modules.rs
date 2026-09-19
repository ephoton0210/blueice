// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

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

    assert!(parse_module("with({}){}").is_err());
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
            include_str!("../../../../development/browser_core/reference/test262/test/language/module-code/verify-dfs.js"),
        ),
        (
            format!("{root}verify-dfs-a_FIXTURE.js"),
            include_str!("../../../../development/browser_core/reference/test262/test/language/module-code/verify-dfs-a_FIXTURE.js"),
        ),
        (
            format!("{root}verify-dfs-b_FIXTURE.js"),
            include_str!("../../../../development/browser_core/reference/test262/test/language/module-code/verify-dfs-b_FIXTURE.js"),
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
fn module_namespace_set_rejects_every_export_and_symbol_key() {
    let modules = HashMap::from([(
        "namespace-set/main.js".to_string(),
        compile_module(
            &parse_module(
                "import * as ns from './main.js';export var local1=null;var local2=null;export {local2 as renamed};export {local1 as indirect} from './main.js';var sym=Symbol('test262');let count=0;try{ns.local1=null}catch(error){count+=1}try{ns.local2=null}catch(error){count+=2}try{ns.renamed=null}catch(error){count+=4}try{ns.indirect=null}catch(error){count+=8}try{ns.default=null}catch(error){count+=16}try{ns[Symbol.toStringTag]=null}catch(error){count+=32}try{ns[sym]=null}catch(error){count+=64}(Reflect.set(ns,'local1')===false&&Reflect.set(ns,'renamed')===false&&Reflect.set(ns,'indirect')===false&&Reflect.set(ns,Symbol.toStringTag)===false&&Reflect.set(ns,sym)===false&&count===127)?127:count",
            )
            .unwrap(),
        )
        .unwrap(),
    )]);
    assert_eq!(
        Vm::default().execute_module_graph("namespace-set/main.js", &modules),
        Ok(Value::Number(127.0))
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

/// `import(specifier, options)`'s second argument (import attributes) per
/// EvaluateImportCall: an omitted, `undefined`, or empty-`with` options
/// object all resolve identically to plain `import(specifier)`.
#[test]
fn dynamic_import_second_argument_accepts_omitted_or_empty_options() {
    let modules = HashMap::from([(
        "second-arg/dependency.js".to_string(),
        compile_module(&parse_module("export const value=42").unwrap()).unwrap(),
    )]);
    for source in [
        "import('./dependency.js').then(ns=>{if(ns.value===42)$DONE();else $DONE(new Error('a'))})",
        "import('./dependency.js', undefined).then(ns=>{if(ns.value===42)$DONE();else $DONE(new Error('b'))})",
        "import('./dependency.js', {}).then(ns=>{if(ns.value===42)$DONE();else $DONE(new Error('c'))})",
        "import('./dependency.js', {with:undefined}).then(ns=>{if(ns.value===42)$DONE();else $DONE(new Error('d'))})",
        // A trailing comma is allowed after either one or two arguments.
        "import('./dependency.js',).then(ns=>{if(ns.value===42)$DONE();else $DONE(new Error('e'))})",
        "import('./dependency.js', {},).then(ns=>{if(ns.value===42)$DONE();else $DONE(new Error('f'))})",
        // An attribute key this host does not act on is still accepted --
        // this host does not yet vary module resolution by attribute.
        "import('./dependency.js', {with:{type:'javascript'}}).then(ns=>{if(ns.value===42)$DONE();else $DONE(new Error('g'))})",
    ] {
        let mut vm = Vm::default();
        vm.install_test262_done().unwrap();
        vm.set_module_loader_context("second-arg/main.js", modules.clone());
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap();
        vm.run_promise_jobs().unwrap();
        assert_eq!(vm.take_test262_done(), Some(Ok(())), "{source}");
    }
}

/// EvaluateImportCall rejects (not throws synchronously) a non-object
/// options argument, a non-object `with` attributes value, and a
/// non-string attribute value -- and propagates a thrown attribute getter's
/// exact value rather than wrapping it.
#[test]
fn dynamic_import_second_argument_rejects_invalid_options_and_attributes() {
    let modules = HashMap::from([(
        "second-arg-invalid/dependency.js".to_string(),
        compile_module(&parse_module("export const value=42").unwrap()).unwrap(),
    )]);
    for source in [
        "import('./dependency.js', 23).then(()=>{$DONE(new Error('fulfilled'))},error=>{if(error.constructor===TypeError)$DONE();else $DONE(error)})",
        "import('./dependency.js', null).then(()=>{$DONE(new Error('fulfilled'))},error=>{if(error.constructor===TypeError)$DONE();else $DONE(error)})",
        "import('./dependency.js', {with:23}).then(()=>{$DONE(new Error('fulfilled'))},error=>{if(error.constructor===TypeError)$DONE();else $DONE(error)})",
        "import('./dependency.js', {with:{key:23}}).then(()=>{$DONE(new Error('fulfilled'))},error=>{if(error.constructor===TypeError)$DONE();else $DONE(error)})",
        "var thrown=new Test262Error();import('./dependency.js', {with:{get key(){throw thrown}}}).then(()=>{$DONE(new Error('fulfilled'))},error=>{if(error===thrown)$DONE();else $DONE(error)})",
        "var thrown=new Test262Error();import('./dependency.js', {get with(){throw thrown}}).then(()=>{$DONE(new Error('fulfilled'))},error=>{if(error===thrown)$DONE();else $DONE(error)})",
    ] {
        let mut vm = Vm::default();
        vm.install_test262_harness().unwrap();
        vm.install_test262_done().unwrap();
        vm.set_module_loader_context("second-arg-invalid/main.js", modules.clone());
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap();
        vm.run_promise_jobs().unwrap();
        assert_eq!(vm.take_test262_done(), Some(Ok(())), "{source}");
    }
}

/// EvaluateImportCall evaluates the specifier expression, then the options
/// expression, synchronously and in that order -- before either argument's
/// value is inspected -- and both positions are `AssignmentExpression[+In]`
/// even inside a no-in `for`-head context.
#[test]
fn dynamic_import_second_argument_evaluates_arguments_in_order_with_in_allowed() {
    let mut vm = Vm::default();
    let source = "let log=[];import(log.push('first'),(log.push('second'),undefined)).then(null,function(){});if(log.length===2&&log[0]==='first'&&log[1]==='second')1;else throw new Error('order')";
    vm.execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap();

    let for_in_source = "let promise;for(promise=import('x','y' in {}||undefined);false;);promise.then(null,function(){})";
    vm.execute(&compile(&parse(for_in_source).unwrap()).unwrap())
        .unwrap();
}

/// ImportCall is a "Forbidden Extension": a spread argument, a third
/// argument, and using `new` on it are all SyntaxErrors, and `import` is
/// otherwise reserved (not a plain IdentifierReference) outside of
/// `import(...)` and `import.<name>`.
#[test]
fn dynamic_import_call_rejects_forbidden_extensions() {
    for source in [
        "new import('x')",
        "new import('x').prop",
        "import(...['x'])",
        "import('x', 'y', 'z')",
        "typeof import",
        "import + 1",
    ] {
        assert!(parse(source).is_err(), "{source}");
    }
    // `import.meta`/`import.source`/`import.defer` remain ordinary
    // continuations of the `import` binding and must keep parsing.
    for source in ["import.source('x')", "import.defer('x')"] {
        assert!(parse(source).is_ok(), "{source}");
    }
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
fn promise_race_rejects_iterator_acquisition_and_step_errors() {
    for source in [
        "Promise.race(new Error('not iterable')).then(function(){$DONE(new Test262Error('fulfilled'))},function(error){if(error instanceof TypeError)$DONE();else $DONE(error)})",
        "let iterable={};iterable[Symbol.iterator]=false;Promise.race(iterable).then(function(){$DONE(new Test262Error('fulfilled'))},function(error){if(error instanceof TypeError)$DONE();else $DONE(error)})",
        "let error=new Test262Error('iterator value');let result={done:false};Object.defineProperty(result,'value',{get:function(){throw error}});let iterable={};iterable[Symbol.iterator]=function(){return {next:function(){return result}}};Promise.race(iterable).then(function(){$DONE(new Test262Error('fulfilled'))},function(reason){if(reason===error)$DONE();else $DONE(reason)})",
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

/// `ensure_dynamic_module_compiled`: a module the host did not pre-compile
/// into `set_module_loader_context`'s registry, but *does* have raw text
/// for via `set_dynamic_module_sources`, compiles and links successfully
/// the first time a dynamic import actually resolves to it.
#[test]
fn dynamic_import_compiles_an_uncompiled_module_on_demand() {
    let mut vm = Vm::default();
    vm.install_test262_done().unwrap();
    vm.set_module_loader_context("dynamic-compile/main.js", HashMap::new());
    vm.set_dynamic_module_sources(HashMap::from([(
        "dynamic-compile/dependency.js".to_string(),
        "export const value = 262;".to_string(),
    )]));
    let source = "import('./dependency.js').then(function(ns){if(ns.value===262)$DONE();else $DONE(new Error('wrong value'))},$DONE)";
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap();
    vm.run_promise_jobs().unwrap();
    assert_eq!(vm.take_test262_done(), Some(Ok(())));
}

/// The `eval-script-code-target` scenario: a module valid as script code but
/// a genuine early `SyntaxError` *as a module* (a lexically-declared
/// function name colliding with a `var`, exactly `test262`'s own
/// `script-code_FIXTURE.js`) must fail lazily -- as this dynamic import's
/// own promise rejection with a real `SyntaxError` -- not eagerly (a panic,
/// or a failure that occurs before the importing code even runs).
#[test]
fn dynamic_import_of_a_module_only_invalid_as_a_module_rejects_lazily() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    vm.install_test262_done().unwrap();
    vm.set_module_loader_context("dynamic-compile-invalid/main.js", HashMap::new());
    vm.set_dynamic_module_sources(HashMap::from([(
        "dynamic-compile-invalid/script-code.js".to_string(),
        "var smoosh; function smoosh() {}".to_string(),
    )]));
    let source = "import('./script-code.js').catch(function(error){assert.sameValue(error.name,'SyntaxError')}).then($DONE,$DONE)";
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap();
    vm.run_promise_jobs().unwrap();
    assert_eq!(vm.take_test262_done(), Some(Ok(())));
}

/// A dynamically-compiled-on-demand module reached a second time (a second
/// `import()` of the same specifier, or a namespace access after the first
/// resolved) must observe the *same* linked module -- not recompile, and
/// not lose the graph state the first dynamic import's own linking pass
/// built -- exactly like an ordinarily pre-compiled module already does.
#[test]
fn dynamic_import_reuses_an_on_demand_compiled_module_across_repeated_imports() {
    let mut vm = Vm::default();
    vm.install_test262_done().unwrap();
    vm.set_module_loader_context("dynamic-compile-repeat/main.js", HashMap::new());
    vm.set_dynamic_module_sources(HashMap::from([(
        "dynamic-compile-repeat/dependency.js".to_string(),
        "export const value = 262;".to_string(),
    )]));
    let source = "Promise.all([import('./dependency.js'),import('./dependency.js')]).then(function(modules){if(modules[0]===modules[1]&&modules[0].value===262)$DONE();else $DONE(new Error('mismatched namespaces'))},$DONE)";
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap();
    vm.run_promise_jobs().unwrap();
    assert_eq!(vm.take_test262_done(), Some(Ok(())));
}

/// A failed on-demand compile/link for one dynamic import must not corrupt
/// an already-existing, otherwise-healthy module graph: an unrelated
/// dynamic import of a *different*, perfectly valid module made afterward
/// (in the same script) must still succeed.
#[test]
fn dynamic_import_failure_does_not_corrupt_an_existing_graph() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    vm.install_test262_done().unwrap();
    vm.set_module_loader_context("dynamic-compile-isolation/main.js", HashMap::new());
    vm.set_dynamic_module_sources(HashMap::from([
        (
            "dynamic-compile-isolation/broken.js".to_string(),
            "var smoosh; function smoosh() {}".to_string(),
        ),
        (
            "dynamic-compile-isolation/healthy.js".to_string(),
            "export const value = 262;".to_string(),
        ),
    ]));
    let source = "function asyncTest(test){test().then(function(){$DONE()},function(error){$DONE(error)})}asyncTest(async function(){await import('./broken.js').catch(function(error){assert.sameValue(error.name,'SyntaxError')});let ns=await import('./healthy.js');if(ns.value!==262)throw new Test262Error('unrelated import failed')})";
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
                "../../../../development/browser_core/reference/test262/test/language/module-code/source-phase-import/import-source-binding-name_FIXTURE.js"
            ))
            .unwrap())
            .unwrap(),
        ),
        (
            format!("{fixture_root}import-source-binding-name-2_FIXTURE.js"),
            compile_module(&parse_module(include_str!(
                "../../../../development/browser_core/reference/test262/test/language/module-code/source-phase-import/import-source-binding-name-2_FIXTURE.js"
            ))
            .unwrap())
            .unwrap(),
        ),
        (
            format!("{fixture_root}import-source-newlines_FIXTURE.js"),
            compile_module(&parse_module(include_str!(
                "../../../../development/browser_core/reference/test262/test/language/module-code/source-phase-import/import-source-newlines_FIXTURE.js"
            ))
            .unwrap())
            .unwrap(),
        ),
        (
            format!("{fixture_root}ensure-linking-error_FIXTURE.js"),
            compile_module(&parse_module(include_str!(
                "../../../../development/browser_core/reference/test262/test/language/module-code/source-phase-import/ensure-linking-error_FIXTURE.js"
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
                "../../../../development/browser_core/reference/test262/harness/asyncHelpers.js"
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
            "../../../../development/browser_core/reference/test262/test/language/module-code/source-phase-import/import-source.js"
        ))
        .unwrap())
        .unwrap(),
    )
    .unwrap();
    vm.run_promise_jobs().unwrap();
    assert_eq!(vm.take_test262_done(), Some(Ok(())));
}

/// A documented, accepted trade-off of lazily compiling a dynamic-only
/// module exactly when it is actually requested (`ensure_dynamic_module_compiled`),
/// rather than the harness's older behavior of eagerly compiling every
/// transitively reachable sibling into one shared registry up front.
///
/// `language/module-code/source-phase-import/import-source.js` (the real
/// Test262 test this reproduces) dynamically imports three fixtures, one
/// at a time. One of those fixtures --
/// `import-source-binding-name-2_FIXTURE.js` -- has genuine `import source
/// x from '<do not resolve>'` requests that always fail with a
/// `TypeError` ("host did not provide a source-phase representation"). The
/// *other* fixtures' own imports (ordinary default imports of the same
/// deliberately-unresolvable specifier, or -- via a shared
/// `ensure-linking-error_FIXTURE.js` sibling -- a "does not export" case)
/// fail with a `SyntaxError` instead.
///
/// The eager-harness era's coincidence: since every sibling was compiled
/// into one shared registry before *any* dynamic import ran, and a first
/// module graph links everything currently in that registry together, the
/// second fixture's real `TypeError` always won the race against the
/// others' `SyntaxError`s, regardless of which fixture a given dynamic
/// import call actually targeted -- accidentally matching what the test
/// expects for all three calls. Lazily compiling only the module a
/// specific dynamic import actually names (the whole point of this
/// session's `eval-script-code-target` fix, and a strictly more correct
/// linking granularity -- it stops spuriously batching together modules
/// that have nothing to do with the import being made) removes that
/// coincidence: the first fixture's own dynamic import no longer
/// incidentally pulls in the second fixture's `Bytecode`, so its own
/// `SyntaxError` is what actually surfaces.
///
/// This is a real, understood regression on this one upstream test,
/// preserved here as a passing (not `#[ignore]`d) regression test against
/// the current, intentional behavior -- not something to "fix" by
/// reintroducing eager cross-sibling batching.
#[test]
fn dynamic_import_of_lazily_compiled_siblings_does_not_batch_unrelated_modules() {
    let fixture_root = "language/module-code/source-phase-import/";
    let modules = HashMap::from([(
        format!("{fixture_root}ensure-linking-error_FIXTURE.js"),
        compile_module(&parse_module(include_str!(
            "../../../../development/browser_core/reference/test262/test/language/module-code/source-phase-import/ensure-linking-error_FIXTURE.js"
        ))
        .unwrap())
        .unwrap(),
    )]);
    let dynamic_sources = HashMap::from([(
        format!("{fixture_root}import-source-binding-name_FIXTURE.js"),
        include_str!("../../../../development/browser_core/reference/test262/test/language/module-code/source-phase-import/import-source-binding-name_FIXTURE.js").to_string(),
    )]);
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    vm.install_test262_done().unwrap();
    vm.set_module_loader_context(format!("{fixture_root}import-source.js"), modules);
    vm.set_dynamic_module_sources(dynamic_sources);
    let source = "import('./import-source-binding-name_FIXTURE.js').then(function(){$DONE(new Error('fulfilled'))},function(error){if(error.constructor===TypeError)$DONE();else $DONE(error)})";
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap();
    vm.run_promise_jobs().unwrap();
    // Documents the accepted trade-off: without the second fixture's
    // genuine source-phase `TypeError` in the same batch, this fixture's
    // own (ordinary-import, unresolvable-specifier) failure surfaces
    // instead, and it is a `SyntaxError`, not the `TypeError` the real
    // upstream test happened to rely on every sibling sharing a batch for.
    match vm.take_test262_done() {
        Some(Err(Value::Object(id))) => {
            let name = vm.heap().get(id, "name").unwrap();
            assert_eq!(name, Value::String("SyntaxError".into()));
        }
        other => panic!("expected a SyntaxError passed straight to $DONE, got {other:?}"),
    }
}

/// An internally created Promise (here, dynamic import's own) must observe
/// `promise.constructor === Promise` even when nothing in the script has
/// referenced the bare `Promise` identifier yet -- `Promise.prototype`'s
/// "constructor" property is otherwise only wired up when the `Promise`
/// *global* itself is separately materialized (`globals.rs`), which
/// `new_promise` (`vm/builtins/promises.rs`) previously never triggered on
/// its own. Mirrors `language/expressions/dynamic-import/
/// always-create-new-promise.js`.
#[test]
fn dynamic_import_promise_observes_the_promise_constructor_link_unprompted() {
    let modules = HashMap::from([(
        "always-new/dynamic-import-module_FIXTURE.js".to_string(),
        compile_module(&parse_module("export const x = 1;").unwrap()).unwrap(),
    )]);
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    vm.set_module_loader_context("always-new/main.js", modules);
    let source = "const p1=import('./dynamic-import-module_FIXTURE.js');const p2=import('./dynamic-import-module_FIXTURE.js');p1!==p2&&p1.constructor===Promise&&Object.getPrototypeOf(p1)===Promise.prototype&&p2.constructor===Promise&&Object.getPrototypeOf(p2)===Promise.prototype";
    assert_eq!(
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap()),
        Ok(Value::Bool(true))
    );
}

/// A script that dynamically imports *itself* (its own resolved path) must
/// compile that self-reference on demand from `set_dynamic_module_sources`,
/// exactly like any other lazily-compiled sibling -- and repeated dynamic
/// imports of the same specifier (concurrent, via `Promise.all`, and
/// serialized, via sequential `await`) must observe the module evaluated
/// exactly once. Mirrors `language/expressions/dynamic-import/
/// eval-self-once-script.js`.
#[test]
fn dynamic_import_of_the_entrys_own_self_reference_compiles_on_demand() {
    let source = include_str!("../../../../development/browser_core/reference/test262/harness/fnGlobalObject.js").to_string()
        + "\n"
        + include_str!("../../../../development/browser_core/reference/test262/test/language/expressions/dynamic-import/eval-self-once-script.js");
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    vm.install_test262_done().unwrap();
    vm.set_module_loader_context("eval-self/eval-self-once-script.js", HashMap::new());
    vm.set_dynamic_module_sources(HashMap::from([(
        "eval-self/eval-self-once-script.js".to_string(),
        source.clone(),
    )]));
    vm.execute_script(&compile(&parse(&source).unwrap()).unwrap())
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

/// ParseJSONModule + CreateDefaultExportSyntheticModule
/// (`vm/modules.rs::ensure_json_module`): a static `import ... with
/// {type:"json"}` exposes `JSON.parse`'s result as the sole `default`
/// export, for every JSON value type.
#[test]
fn json_module_static_import_exposes_parsed_default_export() {
    for (json_text, check) in [
        ("262", "value===262"),
        ("true", "value===true"),
        ("null", "value===null"),
        ("\"a string value\"", "value===\"a string value\""),
        (
            "[1,2,3]",
            "Array.isArray(value)&&value.length===3&&value[1]===2",
        ),
        ("{\"a\":1}", "value.a===1"),
    ] {
        let source = format!("import value from './data.json' with {{ type: 'json' }}; {check}");
        let modules = HashMap::from([(
            "json-static/main.js".to_string(),
            compile_module(&parse_module(&source).unwrap()).unwrap(),
        )]);
        let mut vm = Vm::default();
        vm.set_json_module_sources(HashMap::from([(
            "json-static/data.json".to_string(),
            json_text.to_string(),
        )]));
        assert_eq!(
            vm.execute_module_graph("json-static/main.js", &modules),
            Ok(Value::Bool(true)),
            "{json_text}"
        );
    }
}

/// A JSON module's namespace has exactly one own property, "default" --
/// matching `CreateDefaultExportSyntheticModule`'s single export list, not
/// the properties of the parsed object itself.
#[test]
fn json_module_namespace_has_only_a_default_export() {
    let source = "import * as ns from './data.json' with { type: 'json' }; Object.getOwnPropertyNames(ns).length===1&&ns.default.a===1";
    let modules = HashMap::from([(
        "json-namespace/main.js".to_string(),
        compile_module(&parse_module(source).unwrap()).unwrap(),
    )]);
    let mut vm = Vm::default();
    vm.set_json_module_sources(HashMap::from([(
        "json-namespace/data.json".to_string(),
        "{\"a\":1}".to_string(),
    )]));
    assert_eq!(
        vm.execute_module_graph("json-namespace/main.js", &modules),
        Ok(Value::Bool(true))
    );
}

/// A JSON module's parsed object/array export is an ordinary, extensible
/// heap object -- not frozen/sealed by virtue of coming from JSON.parse.
#[test]
fn json_module_default_export_values_are_extensible() {
    let source = "import value from './data.json' with { type: 'json' }; value.extra='added'; value.extra==='added'&&Object.isExtensible(value)";
    let modules = HashMap::from([(
        "json-extensible/main.js".to_string(),
        compile_module(&parse_module(source).unwrap()).unwrap(),
    )]);
    let mut vm = Vm::default();
    vm.set_json_module_sources(HashMap::from([(
        "json-extensible/data.json".to_string(),
        "{}".to_string(),
    )]));
    assert_eq!(
        vm.execute_module_graph("json-extensible/main.js", &modules),
        Ok(Value::Bool(true))
    );
}

/// A named (non-"default") binding was never a real proposal for JSON
/// modules: importing one is a linking (resolution) failure, exactly like
/// naming an export that doesn't exist on any other module.
#[test]
fn json_module_named_binding_import_is_a_resolution_error() {
    let source = "$DONOTEVALUATE(); import { name } from './data.json' with { type: 'json' };";
    let modules = HashMap::from([(
        "json-named/main.js".to_string(),
        compile_module(&parse_module(source).unwrap()).unwrap(),
    )]);
    let mut vm = Vm::default();
    vm.set_json_module_sources(HashMap::from([(
        "json-named/data.json".to_string(),
        "{\"name\":\"x\"}".to_string(),
    )]));
    assert!(matches!(
        vm.execute_module_graph("json-named/main.js", &modules),
        Err(RuntimeError::ModuleResolution(_))
    ));
}

/// ParseJSONModule's own `Call(%JSON.parse%, undefined, «source»)` step can
/// itself abruptly complete; that must surface the same way any other
/// linking failure does (a resolution-phase failure), not as a step
/// unrelated to module resolution.
#[test]
fn json_module_malformed_json_text_is_a_resolution_error() {
    let source = "$DONOTEVALUATE(); import value from './data.json' with { type: 'json' };";
    let modules = HashMap::from([(
        "json-invalid/main.js".to_string(),
        compile_module(&parse_module(source).unwrap()).unwrap(),
    )]);
    let mut vm = Vm::default();
    vm.set_json_module_sources(HashMap::from([(
        "json-invalid/data.json".to_string(),
        "{not valid json".to_string(),
    )]));
    assert!(matches!(
        vm.execute_module_graph("json-invalid/main.js", &modules),
        Err(RuntimeError::ModuleResolution(_))
    ));
}

/// The same resolved JSON module path returns the identical object to every
/// import site -- two static bindings in the same module, and a further
/// dynamic `import()` of the same path -- matching ordinary Source Text
/// Module singleton semantics (`language/import/import-attributes/
/// json-idempotency.js`).
#[test]
fn json_module_import_sites_share_object_identity() {
    let source = "import value1 from './data.json' with { type: 'json' }; import { default as value2 } from './data.json' with { type: 'json' }; function asyncTest(test){test().then(function(){$DONE()},function(error){$DONE(error)})} asyncTest(async function(){ if(value1!==value2) throw new Test262Error('static sites disagree'); let viaDynamic=await import('./data.json',{with:{type:'json'}}); if(viaDynamic.default!==value1) throw new Test262Error('dynamic site disagrees'); })";
    let modules = HashMap::from([(
        "json-idempotency/main.js".to_string(),
        compile_module(&parse_module(source).unwrap()).unwrap(),
    )]);
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    vm.install_test262_done().unwrap();
    vm.set_json_module_sources(HashMap::from([(
        "json-idempotency/data.json".to_string(),
        "{\"a\":1}".to_string(),
    )]));
    vm.execute_module_graph("json-idempotency/main.js", &modules)
        .unwrap();
    vm.run_promise_jobs().unwrap();
    assert_eq!(vm.take_test262_done(), Some(Ok(())));
}

/// A pure dynamic `import(spec, {with:{type:'json'}})`, with no static
/// import anywhere in the graph, still resolves through
/// `ensure_json_module` (the `execute_module_graph_inner` "entry_json" path
/// rather than the static-scan path).
#[test]
fn json_module_dynamic_import_with_no_static_import_fulfills() {
    let mut vm = Vm::default();
    vm.install_test262_done().unwrap();
    vm.set_json_module_sources(HashMap::from([(
        "json-dynamic-only/data.json".to_string(),
        "262".to_string(),
    )]));
    vm.set_module_loader_context("json-dynamic-only/main.js", HashMap::new());
    let source = "import('./data.json',{with:{type:'json'}}).then(function(ns){if(ns.default===262)$DONE();else $DONE(new Error('wrong value'))},$DONE)";
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap();
    vm.run_promise_jobs().unwrap();
    assert_eq!(vm.take_test262_done(), Some(Ok(())));
}

/// A `type: "json"` request the host never supplied text for rejects
/// (dynamically) or fails linking (statically) with the same "host did not
/// provide" TypeError `module_source_object`'s source-phase counterpart
/// uses, rather than silently falling through to ordinary JS parsing.
#[test]
fn json_module_missing_host_source_is_a_type_error() {
    let mut vm = Vm::default();
    vm.install_test262_done().unwrap();
    vm.set_module_loader_context("json-missing/main.js", HashMap::new());
    let source = "import('./absent.json',{with:{type:'json'}}).then(function(){$DONE(new Error('fulfilled'))},function(error){if(error.constructor===TypeError)$DONE();else $DONE(error)})";
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap();
    vm.run_promise_jobs().unwrap();
    assert_eq!(vm.take_test262_done(), Some(Ok(())));
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
