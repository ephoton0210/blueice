// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The reverse Test262 membrane: a *child* `$262.createRealm()` realm's own
//! code calling back into a live object owned by the parent realm that
//! created it -- the counterpart to `realms.rs`'s (and this crate's other)
//! coverage of the *forward* direction (parent code reaching into a
//! child's real objects), which was already solid. Before this feature,
//! every non-Array/TypedArray/ArrayBuffer value crossing *into* a child
//! became a permanently empty, non-callable stand-in
//! (`test262_transport_value`'s old final `else` branch); these tests
//! exercise the live reverse facade (`Test262ReverseValue`,
//! `vm/test262/reverse.rs`) that replaced it, built on the same
//! `register_active`/`resolve_active` reentrancy primitive
//! `shadow_realm.rs` already established for `ShadowRealm`'s own
//! `WrappedFunctionCreate` facades (now shared via `vm/realm_reentrancy.rs`).
//!
//! Regressions for the three Test262 fixtures that originally motivated this:
//! `language/types/reference/put-value-prop-base-primitive-realm.js` (a
//! parent `Proxy` `set` trap reached through a child prototype-chain walk),
//! `staging/sm/Date/defaultvalue.js` (a parent callable crossed into a
//! child, later called back from within it), and
//! `staging/sm/class/superPropProxies.js` (`super.method()` re-entering a
//! child with `this` bound to a live parent object).

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm, VmConfig};

fn evaluate(source: &str) -> Result<Value, RuntimeError> {
    let mut vm = Vm::new(VmConfig::default()).unwrap();
    vm.install_test262_harness().unwrap();
    vm.execute(&compile(&parse(source).unwrap()).unwrap())
}

fn assert_true(source: &str) {
    match evaluate(source) {
        Ok(value) => assert_eq!(value, Value::Bool(true), "{source}"),
        Err(error) => panic!("{source}\n  threw: {error:?}"),
    }
}

#[test]
fn a_parent_proxy_set_trap_fires_when_a_child_walks_into_it_through_a_primitive_base() {
    // Regression for `put-value-prop-base-primitive-realm.js`: PutValue on a
    // primitive base coerces to an object honoring the *current* execution
    // context's realm, so `0..x = null` evaluated as child code must walk
    // the child's own `Number.prototype` -- whose [[Prototype]] the parent
    // retargeted to a parent-owned `Proxy` -- and actually invoke that
    // Proxy's `set` trap, not silently skip over it.
    assert_true(
        "(() => { \
           var other = $262.createRealm().global; \
           var count = 0; \
           var spy = new Proxy({}, { set: function () { count += 1; return true; } }); \
           Object.setPrototypeOf(other.Number.prototype, spy); \
           other.eval('0..reverseTest = null;'); \
           return count === 1; \
         })()",
    );
}

#[test]
fn a_parent_proxy_set_trap_reached_mid_chain_from_child_code_still_dispatches() {
    // `ordinary_set_with_receiver`'s prototype-chain loop must recognize a
    // reverse facade at *any* position, not only as the initial [[Set]]
    // target: three links deep here (leaf -> mid -> the parent Proxy).
    assert_true(
        "(() => { \
           var other = $262.createRealm(); \
           var count = 0; \
           var spy = new Proxy({}, { set: function () { count += 1; return true; } }); \
           other.global.spy = spy; \
           other.evalScript( \
             'var mid = {}; Object.setPrototypeOf(mid, spy); ' + \
             'var leaf = Object.create(mid); leaf.foo = 1; undefined;' \
           ); \
           return count === 1; \
         })()",
    );
}

#[test]
fn a_parent_callable_crossed_into_a_child_can_be_called_back_from_within_it() {
    // Regression for `staging/sm/Date/defaultvalue.js`: a parent function
    // assigned onto a child-owned object, then invoked *by child code*
    // (not read back out to the parent, which would just hit the existing
    // forward-direction round-trip cache and call the original directly --
    // see `test262_import_foreign_value`'s `imported_values` check). Uses
    // `evalScript` so the call genuinely dispatches inside the child.
    // `tracker` is a *parent*-realm function: OrdinaryCallBindThis
    // substitutes an omitted/undefined receiver from the callee's own
    // realm, not the caller's -- so `this` here is genuinely this
    // top-level realm's globalThis, not the child's.
    assert_true(
        "(() => { \
           var other = $262.createRealm(); \
           var seenThis; \
           other.global.tracker = function (a, b) { seenThis = this; return a + b; }; \
           var sum = other.evalScript('tracker(3, 4)'); \
           return sum === 7 && seenThis === globalThis; \
         })()",
    );
}

#[test]
fn a_reverse_facade_forwards_an_explicit_receiver_and_arguments() {
    assert_true(
        "(() => { \
           var other = $262.createRealm(); \
           other.global.add = function (a, b) { return a + b; }; \
           var sum = other.evalScript('add.call({}, 10, 20)'); \
           return sum === 30; \
         })()",
    );
}

#[test]
fn a_reverse_facade_forwards_construct_and_its_result_crosses_back_as_a_facade() {
    // `new Pt(5)` inside the child dispatches [[Construct]] on the reverse
    // facade, producing a real *parent* object; that result then crosses
    // back into the child (a second, independent reverse facade), and a
    // plain property read off it must reach the genuine parent instance.
    assert_true(
        "(() => { \
           var other = $262.createRealm(); \
           function Pt(x) { this.x = x; } \
           other.global.Pt = Pt; \
           return other.evalScript('var inst = new Pt(5); inst.x') === 5; \
         })()",
    );
}

#[test]
fn is_callable_and_is_constructor_agree_with_the_real_parent_value() {
    // The `new plain()` case's thrown value crosses back as a reverse
    // facade of the *parent's* own TypeError instance, so `instanceof`
    // against the child's local `TypeError` correctly reports false
    // (matching this crate's other cross-realm error checks, e.g.
    // `error_realms.rs`); compare by name instead.
    assert_true(
        "(() => { \
           var other = $262.createRealm(); \
           function Ctor() {} \
           var plain = {}; \
           other.global.ctor = Ctor; \
           other.global.plain = plain; \
           return other.evalScript('typeof ctor') === 'function' \
             && other.evalScript('typeof plain') === 'object' \
             && other.evalScript( \
                  '(function(){try{new plain();return false}catch(e){return e.name===\"TypeError\"}})()' \
                ) === true \
             && other.evalScript('new ctor() instanceof ctor') === true; \
         })()",
    );
}

#[test]
fn a_reverse_facade_supports_own_keys_get_prototype_and_set() {
    assert_true(
        "(() => { \
           var other = $262.createRealm(); \
           var parentObj = { a: 1, b: 2 }; \
           other.global.po = parentObj; \
           var keys = other.evalScript('Object.keys(po).sort().join(\",\")'); \
           var distinctProto = other.evalScript('Object.getPrototypeOf(po) !== Object.prototype'); \
           other.evalScript('po.c = 3; undefined'); \
           return keys === 'a,b' && distinctProto === true && parentObj.c === 3; \
         })()",
    );
}

#[test]
fn a_reverse_facade_supports_set_prototype_of_and_a_missing_descriptor() {
    assert_true(
        "(() => { \
           var other = $262.createRealm(); \
           var parentObj = {}; \
           var parentProto = { fromProto: 1 }; \
           other.global.po = parentObj; \
           other.global.pp = parentProto; \
           var afterSet = other.evalScript( \
             'Object.setPrototypeOf(po, pp); po.fromProto;' \
           ); \
           var missing = other.evalScript( \
             'Object.getOwnPropertyDescriptor(po, \"nope\")' \
           ); \
           return afterSet === 1 && missing === undefined; \
         })()",
    );
}

#[test]
fn setting_a_foreign_facades_prototype_forwards_into_its_real_child_object() {
    // The forward-direction counterpart of the reverse `[[SetPrototypeOf]]`
    // test above: `object_set_prototype` did not previously special-case a
    // foreign facade at all, so `Object.setPrototypeOf` on one silently
    // mutated only the facade's own otherwise-unobserved local slot,
    // leaving the real child object's prototype unchanged.
    assert_true(
        "(() => { \
           var other = $262.createRealm().global; \
           var obj = other.eval('({})'); \
           var newProto = { marker: 99 }; \
           Object.setPrototypeOf(obj, newProto); \
           return Object.getPrototypeOf(obj) === newProto; \
         })()",
    );
}

#[test]
fn a_foreign_facades_own_properties_are_visible_to_object_keys() {
    // The forward-direction counterpart of `object_get_own_property`'s own
    // fix: without it, `Object.keys`/`getOwnPropertyDescriptor`/`in`/
    // `hasOwnProperty` on a foreign facade all treated every real own
    // property as absent, even though `test262_foreign_own_property_keys`
    // already listed their names correctly.
    assert_true(
        "(() => { \
           var other = $262.createRealm().global; \
           var obj = other.eval('({ a: 1, b: 2 })'); \
           return Object.keys(obj).sort().join(',') === 'a,b' \
             && obj.hasOwnProperty('a') === true \
             && ('a' in obj) === true; \
         })()",
    );
}

#[test]
fn super_property_with_a_reverse_home_object_reads_the_live_parent_receiver() {
    // Regression for `staging/sm/class/superPropProxies.js`'s CCW-on-the-
    // prototype-chain case: `super.method()`'s [[HomeObject]] walk reaches
    // a foreign facade (`wrappedBase`, the *first* node -- exercising
    // `get_object_property`'s existing top-level foreign check) whose real
    // method runs as child code with `this` bound to the *parent*
    // receiver, which must itself cross as a live reverse facade so
    // `this.__secretProp__` reads the genuine parent value.
    assert_true(
        "(() => { \
           var g = $262.createRealm().global; \
           var wrappedBase = g.eval(\"({ method() { return this.__secretProp__; } })\"); \
           var unwrappedDerived = { \
             __secretProp__: 42, \
             method() { return super.method(); }, \
           }; \
           Object.setPrototypeOf(unwrappedDerived, wrappedBase); \
           return unwrappedDerived.method() === 42; \
         })()",
    );
}

#[test]
fn get_from_prototype_finds_an_own_property_of_a_reverse_facade_reached_mid_chain() {
    // The read-side counterpart of the mid-chain `[[Set]]` test above:
    // `get_from_prototype`'s own walk must recognize a reverse facade
    // reached partway through, not only as its own `start` argument.
    assert_true(
        "(() => { \
           var other = $262.createRealm(); \
           var parentBase = { answer: 42 }; \
           other.global.parentBase = parentBase; \
           var result = other.evalScript( \
             'var leaf = {}; Object.setPrototypeOf(leaf, parentBase); leaf.answer;' \
           ); \
           return result === 42; \
         })()",
    );
}

#[test]
fn reverse_facades_chain_correctly_across_a_two_level_realm_nesting() {
    // A value crossing parent -> mid, then re-exported mid -> leaf (a
    // grandchild `$262.createRealm()` created *from inside* `mid`'s own
    // script), produces a *second*, independent reverse facade whose
    // `home_heap` names `mid` (the realm that actually performed that
    // second transport) rather than the original top-level realm --
    // calling it from leaf must walk back through both live hops
    // (`resolve_active` finding `mid`, then `mid` itself resolving back to
    // the outermost realm), all within one synchronous call chain.
    assert_true(
        "(() => { \
           var mid = $262.createRealm(); \
           var topFn = function () { return 42; }; \
           mid.global.topFn = topFn; \
           var result = mid.evalScript( \
             'var leaf = $262.createRealm(); ' + \
             'leaf.global.callTop = topFn; ' + \
             \"leaf.evalScript('callTop()');\" \
           ); \
           return result === 42; \
         })()",
    );
}

#[test]
fn a_revoked_parent_proxy_reached_through_a_reverse_facade_throws_a_catchable_error() {
    assert_true(
        "(() => { \
           var other = $262.createRealm(); \
           var record = Proxy.revocable({}, {}); \
           other.global.p = record.proxy; \
           record.revoke(); \
           try { \
             other.evalScript('p.foo'); \
             return false; \
           } catch (e) { \
             return e.name === 'TypeError'; \
           } \
         })()",
    );
}

#[test]
fn a_reverse_call_reached_without_a_live_ancestor_on_the_call_chain_is_a_catchable_type_error() {
    // Not every forward-direction internal-method boundary re-establishes
    // `register_active` for its own dynamic extent -- only `test262_foreign_
    // call`'s own generic-forwarding path does, since it is the one place
    // the three target fixtures actually need it (see `vm/test262/
    // reverse.rs`'s module documentation). A parent-owned accessor
    // property, read through `test262_foreign_get` (deliberately left
    // unwrapped), whose getter calls back into a reverse facade must
    // therefore still fail *safely* -- a catchable TypeError, never a
    // dangling-pointer dereference -- rather than silently doing the wrong
    // thing.
    assert_true(
        "(() => { \
           var other = $262.createRealm(); \
           var pfn = function () { return 99; }; \
           other.global.pfn = pfn; \
           other.evalScript( \
             'Object.defineProperty(globalThis, \"accessor\", ' + \
             '{ get: function () { return pfn(); } });' \
           ); \
           try { \
             other.global.accessor; \
             return false; \
           } catch (e) { \
             return true; \
           } \
         })()",
    );
}

#[test]
fn wrapped_reverse_call_arguments_survive_collection_in_the_parent_realm_until_the_call() {
    // Mirrors `shadow_realm.rs`'s own
    // `wrapped_arguments_survive_collection_in_the_target_realm_until_the_
    // call`: with a one-object nursery every allocation collects, so each
    // argument facade built while importing this call's `this`/arguments
    // into the parent realm (`test262_import_foreign_value`, called once
    // per argument from `test262_reverse_call`) must stay rooted until the
    // call itself runs, not only until the next allocation.
    let mut config = VmConfig::default();
    config.heap.nursery_capacity = 1;
    let mut vm = Vm::new(config).unwrap();
    vm.install_test262_harness().unwrap();
    let source = "(() => { \
        var other = $262.createRealm(); \
        other.global.check = function (a, b, c) { \
            return typeof a + typeof b + typeof c + (a === b) + (a.x === undefined); \
        }; \
        return other.evalScript('check(() => 1, () => 2, () => 3)') \
            === 'functionfunctionfunctionfalsetrue'; \
    })()";
    assert_eq!(
        vm.execute(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn typed_array_species_slice_via_a_reverse_facade_constructor_does_not_panic() {
    // Regression for `staging/sm/TypedArray/slice-bitwise-same.js`: `arr` is
    // a *child*-owned TypedArray; setting `arr.constructor` to the
    // *parent's* own `Float32Array` crosses that constructor into the
    // child as a reverse facade. `arr.slice(0)` dispatches into the child
    // (foreign receiver), where `%TypedArray%.prototype.slice`'s species
    // lookup reads that reverse facade back off `arr.constructor`,
    // forwards through it (`test262_reverse_get`) to the real parent
    // `Float32Array[Symbol.species]` accessor (which returns `this`), and
    // so constructs the result through the reverse facade itself
    // (`test262_reverse_call`) -- this whole path was previously
    // unreachable: before the reverse membrane existed, a parent value
    // crossing into a child became a dead, non-constructible stand-in, so
    // `Get(deadFacade, Symbol.species)` silently read `undefined` and
    // `typed_array_species_constructor` fell back to the child's own local
    // `%Float32Array%` instead -- an accidental pass, not a correct one.
    //
    // `typed_array_create_foreign_target` classified a species constructor
    // as cross-realm by asking `test262_foreign_native_function` alone --
    // which only recognizes *forward* facades -- so a reverse-facade
    // constructor was misclassified as "local" and the slice panicked
    // reading the constructed result's TypedArray info straight from
    // `self.heap` (the result is actually a fresh local object belonging
    // to *this* realm, per `test262_transport_value`'s ordinary TypedArray
    // snapshot -- see below -- but that wasn't checked for either).
    // Fixed by classifying both directions; `length`/`instanceof` are
    // correct as a result.
    //
    // KNOWN REMAINING GAP, NOT fixed here and deliberately not asserted as
    // passing below: the constructed result's *contents* are still wrong.
    // `test262_reverse_call`'s result crosses back into this realm through
    // `test262_transport_value`'s ordinary TypedArray snapshot -- which
    // registers a *round-trip cache* entry (`realm.imported_values`,
    // `foreign.rs`) so a value making a full round trip keeps its original
    // identity. That cache is unconditional: when this facade crosses back
    // OUT again (`test262_import_foreign_value`), the cache is checked
    // first and returns the *original, pristine* parent object the
    // snapshot stood in for -- silently discarding any mutation applied to
    // the snapshot in between, bitwise or ordinary Get/Set alike. So even
    // populating the snapshot correctly (verified directly: reading it back
    // immediately after writing, *before* it crosses back out, shows the
    // right values) has no effect on what the caller ultimately observes.
    // A correct fix needs either constructing the result *with* its data
    // already provided (nothing left to mutate afterward), or a reverse
    // buffer-mirror mechanism symmetric to the forward one -- both are
    // separate, real design work, not attempted here. This test therefore
    // only asserts what today's fix actually guarantees: the call
    // completes (no panic) and the result's shape is right.
    assert_true(
        "(() => { \
           var other = $262.createRealm(); \
           var arr = new other.global.Float32Array(3); \
           arr[0] = 1.5; \
           arr[1] = -2.25; \
           arr[2] = 0; \
           arr.constructor = Float32Array; \
           var sliced = arr.slice(0); \
           return sliced.length === 3 && sliced instanceof Float32Array; \
         })()",
    );
}
