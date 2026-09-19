// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `ShadowRealm` through parse, compile and VM dispatch: construction,
//! `evaluate`'s primitive/callable boundary (`GetWrappedValue`,
//! `WrappedFunctionCreate`) and its `SyntaxError`-vs-opaque-`TypeError`
//! split (`PerformShadowRealmEval`), and `importValue`'s reuse of the host
//! module registry. `ShadowRealm` is a TC39 "stage 2.7" proposal, not part
//! of published ECMA-262 edition 17 -- see
//! `development/browser_core/phase-13-bluejs-engine/ECMASCRIPT_2026.md` --
//! implemented here per an explicit request regardless of that status.

use blueice_bluejs::{compile, compile_module, parse, parse_module, RuntimeError, Value, Vm};
use std::collections::HashMap;

fn evaluate(source: &str) -> Result<Value, RuntimeError> {
    Vm::default().execute(&compile(&parse(source).unwrap()).unwrap())
}

fn assert_true(source: &str) {
    assert_eq!(evaluate(source).unwrap(), Value::Bool(true), "{source}");
}

fn assert_true_with_test262_harness(source: &str) {
    let mut vm = Vm::default();
    vm.install_test262_harness()
        .expect("test262 harness installs");
    let result = vm
        .execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap();
    assert_eq!(result, Value::Bool(true), "{source}");
}

#[test]
fn constructor_creates_distinct_instances_with_the_expected_prototype_chain() {
    assert_true(
        "typeof ShadowRealm === 'function' \
         && Object.getPrototypeOf(ShadowRealm) === Function.prototype",
    );
    assert_true(
        "const a = new ShadowRealm(); const b = new ShadowRealm(); \
         a !== b && a instanceof ShadowRealm \
         && Object.getPrototypeOf(a) === ShadowRealm.prototype",
    );
    assert_true("(()=>{try{ShadowRealm();return false}catch(e){return e instanceof TypeError}})()");
}

#[test]
fn evaluate_returns_primitive_values_unwrapped() {
    assert_true("new ShadowRealm().evaluate('1 + 1') === 2");
    assert_true("new ShadowRealm().evaluate('null') === null");
    assert_true("new ShadowRealm().evaluate('') === undefined");
    assert_true("new ShadowRealm().evaluate('true') === true");
    assert_true("new ShadowRealm().evaluate('\"str\"') === 'str'");
    assert_true("(()=>{const x = new ShadowRealm().evaluate('NaN'); return x !== x})()");
    assert_true("typeof new ShadowRealm().evaluate('Symbol(\"x\")') === 'symbol'");
}

#[test]
fn evaluate_rejects_a_non_primitive_non_callable_result() {
    assert_true(
        "(()=>{try{new ShadowRealm().evaluate('({})');return false}\
         catch(e){return e instanceof TypeError}})()",
    );
    assert_true(
        "(()=>{try{new ShadowRealm().evaluate('[1,2,3]');return false}\
         catch(e){return e instanceof TypeError}})()",
    );
}

#[test]
fn evaluate_distinguishes_syntax_errors_from_opaque_runtime_type_errors() {
    // A parse failure throws a real SyntaxError directly (before any
    // execution context exists), never the opaque TypeError a runtime
    // abrupt completion produces.
    assert_true(
        "(()=>{try{new ShadowRealm().evaluate('...');return false}\
         catch(e){return e instanceof SyntaxError}})()",
    );
    // Any abrupt completion once the script is actually running -- however
    // it originated -- surfaces as a message-less TypeError, not the
    // original error identity.
    assert_true(
        "(()=>{try{new ShadowRealm().evaluate('throw new RangeError(\"boom\")');return false}\
         catch(e){return e instanceof TypeError && e.message === ''}})()",
    );
}

#[test]
fn evaluate_wraps_a_returned_function_as_a_fresh_callable_facade() {
    assert_true(
        "(()=>{const fn = new ShadowRealm().evaluate('(a, b) => a + b'); \
         return typeof fn === 'function' && fn.length === 2 && fn.name === '' \
         && fn(2, 3) === 5})()",
    );
    // WrappedFunctionCreate always produces a brand-new facade -- calling
    // evaluate twice for logically "the same" function never shares
    // identity.
    assert_true(
        "(()=>{const r = new ShadowRealm(); const src = '() => 1'; \
         return r.evaluate(src) !== r.evaluate(src)})()",
    );
}

#[test]
fn wrapped_function_rejects_non_primitive_non_callable_arguments_and_results() {
    assert_true(
        "(()=>{const fn = new ShadowRealm().evaluate('() => {}'); \
         try{fn(1, {});return false}catch(e){return e instanceof TypeError}})()",
    );
    assert_true(
        "(()=>{const fn = new ShadowRealm().evaluate('() => ({})'); \
         try{fn();return false}catch(e){return e instanceof TypeError}})()",
    );
    // A thrown value inside the wrapped call is likewise opaque.
    assert_true(
        "(()=>{const fn = new ShadowRealm().evaluate('() => { throw \"nope\" }'); \
         try{fn();return false}catch(e){return e instanceof TypeError}})()",
    );
}

#[test]
fn wrapped_function_forwards_a_callable_argument_back_into_the_caller_realm() {
    // The classic ShadowRealm round-trip: a caller-realm function passed as
    // an argument into a wrapped call is itself wrapped *into* the callee
    // realm (`GetWrappedValue(targetRealm, arg)`), so the callee can invoke
    // it and observe its side effect back in the caller.
    assert_true(
        "(()=>{ \
           const r = new ShadowRealm(); \
           let seen; \
           const blueFn = (x) => { seen = x; return x * 2; }; \
           const redFn = r.evaluate('(cb, a, b) => cb(a) * b'); \
           return redFn(blueFn, 3, 10) === 60 && seen === 3; \
         })()",
    );
}

#[test]
fn multiple_shadow_realms_can_exchange_wrapped_functions_across_a_gc_boundary() {
    // Regression: a wrapped function's target used to have no GC root of
    // its own in the realm that created it, so an unrelated allocation in
    // that realm (here, a later `evaluate` call) could reclaim it before a
    // second realm's wrapper of it was ever invoked.
    assert_true(
        "(()=>{ \
           const realm1 = new ShadowRealm(); \
           const realm2 = new ShadowRealm(); \
           const r1wrapped = realm1.evaluate('globalThis.count = 0; () => globalThis.count += 1;'); \
           const r2wrapper = realm2.evaluate('(fn) => globalThis.wrapped = fn;'); \
           const rewrapped = r2wrapper(r1wrapped); \
           realm1.evaluate('globalThis.count'); \
           const r2wrapped = realm2.evaluate('globalThis.wrapped'); \
           return r2wrapped() === 1 && rewrapped() === 2 \
             && realm1.evaluate('globalThis.count') === 2; \
         })()",
    );
}

#[test]
fn evaluate_gives_each_call_a_fresh_lexical_scope_that_does_not_conflict_with_earlier_ones() {
    // GetShadowRealmContext ( shadowRealmRecord, strictEval ): "1. Let
    // lexEnv be NewDeclarativeEnvironment(shadowRealmRecord.[[GlobalEnv]])."
    // -- a *fresh* declarative environment on every single `evaluate` call,
    // unlike an ordinary repeated top-level Script (where global
    // `let`/`const` redeclaration genuinely is an error against the same
    // realm, matching real engines' <script>-tag behavior). Two separate
    // `evaluate` calls declaring the same top-level `const` name must not
    // conflict with each other.
    assert_true(
        "(()=>{ \
           const r = new ShadowRealm(); \
           r.evaluate('const x = 1; x'); \
           return r.evaluate('const x = 2; x') === 2; \
         })()",
    );
    // `var`/function declarations are unaffected: GetShadowRealmContext's
    // `varEnv` stays the realm's own persistent GlobalEnv even though
    // `lexEnv` is fresh each time, so they keep being real, visible
    // globalThis properties across calls exactly as before.
    assert_true(
        "(()=>{ \
           const r = new ShadowRealm(); \
           r.evaluate('var y = 1;'); \
           return r.evaluate('y') === 1; \
         })()",
    );
}

#[test]
fn a_shadowrealm_instance_keeps_its_identity_across_a_test262_realm_transport() {
    // A `ShadowRealm` instance created in one Test262 `$262.createRealm()`
    // realm and passed as a value into a *third*, unrelated realm must
    // still be recognized there as the exact same live `ShadowRealm` --
    // same [[ShadowRealm]] brand, same child realm (so a later `evaluate`
    // reached through either path observes the other's side effects) --
    // not Test262's ordinary opaque, brand-less membrane stand-in (which
    // cannot represent "this is a ShadowRealm" at all).
    assert_true_with_test262_harness(
        "var other = $262.createRealm().global; \
         var OtherShadowRealm = other.ShadowRealm; \
         var yetAnother = $262.createRealm().global; \
         var YetAnotherShadowRealm = yetAnother.ShadowRealm; \
         var realm = Reflect.construct(OtherShadowRealm, []); \
         realm.evaluate('globalThis.count = 1;'); \
         var seenThroughYetAnother = YetAnotherShadowRealm.prototype.evaluate.call(realm, 'globalThis.count'); \
         YetAnotherShadowRealm.prototype.evaluate.call(realm, 'globalThis.count = 2;'); \
         var seenThroughOther = realm.evaluate('globalThis.count'); \
         seenThroughYetAnother === 1 && seenThroughOther === 2;",
    );
}

#[test]
fn wrapped_functions_keep_callable_arguments_across_a_test262_realm_transport() {
    // Regression for Test262's
    // `wrapped-function-proto-from-caller-realm.js`: this crosses two
    // `$262.createRealm()` membranes before the callable is presented to a
    // ShadowRealm, so the intermediate opaque stand-in must retain its
    // callable capability for `GetWrappedValue`.
    assert_true_with_test262_harness(
        "var other = $262.createRealm().global; \
         var OtherShadowRealm = other.ShadowRealm; \
         var realm = Reflect.construct(OtherShadowRealm, []); \
         var checkArgWrapperFn = realm.evaluate('(x) => Object.getPrototypeOf(x) === Function.prototype'); \
         checkArgWrapperFn(() => {}) === true;",
    );
}

#[test]
fn evaluate_and_import_value_require_a_shadowrealm_receiver() {
    assert_true(
        "(()=>{try{ShadowRealm.prototype.evaluate.call({}, '1');return false}\
         catch(e){return e instanceof TypeError}})()",
    );
    assert_true(
        "(()=>{try{ShadowRealm.prototype.importValue.call({}, 'x', 'y');return false}\
         catch(e){return e instanceof TypeError}})()",
    );
}

#[test]
fn import_value_coerces_specifier_but_requires_export_name_to_already_be_a_string() {
    assert_true(
        "(()=>{try{new ShadowRealm().importValue('x', {});return false}\
         catch(e){return e instanceof TypeError}})()",
    );
}

#[test]
fn import_value_resolves_a_named_export_through_the_returned_promise() {
    let modules = HashMap::from([(
        "shadow/mod.js".to_string(),
        compile_module(&parse_module("export var x = 42;").unwrap()).unwrap(),
    )]);
    let mut vm = Vm::default();
    vm.install_test262_done().unwrap();
    vm.set_module_loader_context("shadow/main.js", modules);
    let source = "const r = new ShadowRealm(); \
        r.importValue('./mod.js', 'x').then( \
            v => { if (v === 42) $DONE(); else $DONE(new Error('wrong value: ' + v)); }, \
            $DONE, \
        );";
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap();
    vm.run_promise_jobs().unwrap();
    assert_eq!(vm.take_test262_done(), Some(Ok(())));
}

#[test]
fn import_value_resolves_a_lazily_supplied_module_export() {
    // Test262's runner leaves a fixture reached only by `importValue` as
    // source text until that import actually occurs.  The ShadowRealm child
    // must inherit that lazy source registry as well as compiled modules.
    let mut vm = Vm::default();
    vm.install_test262_done().unwrap();
    vm.set_module_loader_context("shadow/main.js", HashMap::new());
    vm.set_dynamic_module_sources(HashMap::from([(
        "shadow/mod.js".to_string(),
        "export var x = 42;".to_string(),
    )]));
    let source = "const r = new ShadowRealm(); \
        r.importValue('./mod.js', 'x').then( \
            v => { if (v === 42) $DONE(); else $DONE(new Error('wrong value: ' + v)); }, \
            $DONE, \
        );";
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap();
    vm.run_promise_jobs().unwrap();
    assert_eq!(vm.take_test262_done(), Some(Ok(())));
}

#[test]
fn import_value_rejects_with_a_typeerror_when_the_export_does_not_exist() {
    let modules = HashMap::from([(
        "shadow/mod.js".to_string(),
        compile_module(&parse_module("export var x = 42;").unwrap()).unwrap(),
    )]);
    let mut vm = Vm::default();
    vm.install_test262_done().unwrap();
    vm.set_module_loader_context("shadow/main.js", modules);
    let source = "const r = new ShadowRealm(); \
        r.importValue('./mod.js', 'missing').then( \
            () => { $DONE(new Error('unexpectedly resolved')); }, \
            err => { \
                if (Object.getPrototypeOf(err) === TypeError.prototype) $DONE(); \
                else $DONE(new Error('wrong rejection: ' + err)); \
            }, \
        );";
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap();
    vm.run_promise_jobs().unwrap();
    assert_eq!(vm.take_test262_done(), Some(Ok(())));
}
