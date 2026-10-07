// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Shared classification, registration and receiver contracts in both builds.

use super::*;
use crate::heap::TemporalKind;
use crate::{compile, parse};

fn execute(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("execution failed for {source}: {error:?}"))
}

fn collect_vm_roots(vm: &mut Vm) {
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
}

#[cfg_attr(test, test)]
fn classification_preserves_primitives_facades_and_revoked_proxy_capabilities() {
    for nursery_capacity in [256, 1] {
        let mut vm = Vm::new(VmConfig {
            heap: HeapConfig {
                nursery_capacity,
                ..HeapConfig::default()
            },
            ..VmConfig::default()
        })
        .unwrap();
        vm.install_test262_harness().unwrap();
        vm.install_test262_is_html_dda().unwrap();
        for (source, truthy, type_name, callable, constructor) in [
            ("undefined", false, "undefined", false, false),
            ("null", false, "object", false, false),
            ("false", false, "boolean", false, false),
            ("true", true, "boolean", false, false),
            ("NaN", false, "number", false, false),
            ("0", false, "number", false, false),
            ("42", true, "number", false, false),
            ("''", false, "string", false, false),
            ("'value'", true, "string", false, false),
            ("0n", false, "bigint", false, false),
            ("1n", true, "bigint", false, false),
            ("Symbol()", true, "symbol", false, false),
            ("({})", true, "object", false, false),
            ("Array", true, "function", true, true),
            ("Math.abs", true, "function", true, false),
            ("(() => 1)", true, "function", true, false),
            ("(function () {})", true, "function", true, true),
            ("(function () {}).bind(null)", true, "function", true, true),
            ("(() => 1).bind(null)", true, "function", true, false),
            ("Symbol", true, "function", true, true),
            ("BigInt", true, "function", true, true),
            ("$262.IsHTMLDDA", false, "undefined", true, false),
            ("$262.createRealm().global.Array", true, "function", true, true),
            ("$262.createRealm().global.eval('() => 1')", true, "function", true, false),
            ("new Proxy({}, {})", true, "object", false, false),
            ("new Proxy(function () {}, {})", true, "function", true, true),
            ("new Proxy(() => 1, {})", true, "function", true, false),
            ("(() => {let r = Proxy.revocable(function () {}, {}); r.revoke(); return r.proxy;})()", true, "function", true, true),
            ("(() => {let r = Proxy.revocable(() => 1, {}); r.revoke(); return r.proxy;})()", true, "function", true, false),
        ] {
            let value = execute(&mut vm, source);
            let root = value.object_id().map(|id| vm.heap.root(id).unwrap());
            collect_vm_roots(&mut vm);
            let base = vm.stack.len();
            assert_eq!(vm.to_boolean(&value), Ok(truthy), "{source}");
            assert_eq!(vm.typeof_value(&value), Ok(type_name), "{source}");
            assert_eq!(vm.is_callable(&value), Ok(callable), "{source}");
            assert_eq!(vm.is_constructor(&value), Ok(constructor), "{source}");
            assert_eq!(vm.stack.len(), base);
            if let Some(root) = root {
                vm.heap.unroot(root).unwrap();
            }
        }
        assert_eq!(
            execute(
                &mut vm,
                r#"
            var traps = 0;
            var proxy = new Proxy(function () {}, {get() {traps++; throw 7;}});
            var realm = $262.createRealm();
            realm.global.parentCallable = proxy;
            realm.global.eval('typeof parentCallable === "function" && Boolean(parentCallable)')
                && typeof proxy === 'function' && Boolean(proxy) && traps === 0
        "#
            ),
            Value::Bool(true)
        );
    }
}

#[cfg_attr(test, test)]
fn classification_keeps_invalid_heap_ingress_fallible_and_allows_reuse() {
    let mut owner = Vm::default();
    let id = owner.heap.alloc_object(None).unwrap();
    let root = owner.heap.root(id).unwrap();
    let value = Value::Object(id);
    let mut vm = Vm::default();
    let expected = Err(RuntimeError::Heap(HeapError::InvalidObject(id)));
    assert_eq!(vm.is_callable(&value), expected);
    assert_eq!(vm.is_constructor(&value), expected);
    assert_eq!(vm.to_boolean(&value), expected);
    assert_eq!(vm.typeof_value(&value).map(|_| false), expected);
    owner.heap.unroot(root).unwrap();
    collect_vm_roots(&mut owner);
    assert_eq!(owner.is_callable(&value), expected);
    assert_eq!(owner.is_constructor(&value), expected);
    assert_eq!(owner.to_boolean(&value), expected);
    assert_eq!(owner.typeof_value(&value).map(|_| false), expected);
    assert_eq!(execute(&mut vm, "21 + 21"), Value::Number(42.0));
    assert_eq!(execute(&mut owner, "21 + 21"), Value::Number(42.0));
}

#[cfg_attr(test, test)]
fn host_registration_reserves_both_accessor_tags_before_publication() {
    for (count, offset) in [
        (0, 0),
        (1, 0),
        (0, 1),
        (u32::MAX as usize - 1, 1),
        (u32::MAX as usize, 0),
    ] {
        assert_eq!(
            host_registration_index(count, offset, "capacity"),
            Ok(count as u32)
        );
    }
    assert_eq!(
        host_registration_index(u32::MAX as usize, 1, "capacity"),
        Err(RuntimeError::RangeError("capacity".into()))
    );
    if let Some(count) = (u32::MAX as usize).checked_add(1) {
        assert_eq!(
            host_registration_index(count, 0, "capacity"),
            Err(RuntimeError::RangeError("capacity".into()))
        );
    }
}

#[cfg_attr(test, test)]
fn temporal_snapshots_keep_owned_fields_and_check_brand_before_options() {
    let mut vm = Vm::default();
    let instant = execute(&mut vm, "new Temporal.Instant(123456789n)");
    let root = vm.heap.root(instant.object_id().unwrap()).unwrap();
    let snapshot = vm
        .validate_temporal_receiver(&instant, TemporalKind::Instant)
        .unwrap();
    collect_vm_roots(&mut vm);
    assert_eq!(snapshot.instant_epoch(), BigInt::from(123456789));
    assert_eq!(snapshot.value(), &instant);
    vm.heap.unroot(root).unwrap();
    let time = execute(&mut vm, "new Temporal.PlainTime(1, 2, 3, 4, 5, 6)");
    let snapshot = vm
        .validate_temporal_receiver(&time, TemporalKind::PlainTime)
        .unwrap();
    collect_vm_roots(&mut vm);
    assert_eq!(snapshot.time_fields(), (1, 2, 3, 4, 5, 6));
    assert_eq!(
        execute(
            &mut vm,
            r#"
        var observed = 0;
        var options = {get smallestUnit() {observed++; throw 7;}};
        for (var method of [Temporal.PlainTime.prototype.round, Temporal.Instant.prototype.round]) {
            var rejected = false;
            try {method.call({}, options);} catch (error) {rejected = error instanceof TypeError;}
            if (!rejected) throw 'wrong brand';
        }
        observed === 0 && new Temporal.PlainTime(1, 2, 3).round('second').second === 3
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg(coverage)]
impl Vm {
    #[doc(hidden)]
    pub fn verify_common_boundary_contracts() {
        classification_preserves_primitives_facades_and_revoked_proxy_capabilities();
        classification_keeps_invalid_heap_ingress_fallible_and_allows_reuse();
        host_registration_reserves_both_accessor_tags_before_publication();
        temporal_snapshots_keep_owned_fields_and_check_brand_before_options();
        reaction_registration_keeps_brand_errors_and_schedules_each_settled_outcome();
        promise_then_retains_handlers_through_allocation_and_species_callbacks();
        promise_then_refusals_restore_operands_and_allow_reuse();
        private_declarations_share_missing_name_errors_after_brand_validation();
        regexp_classification_preserves_match_getters_and_primitive_results();
    }
}

#[cfg_attr(test, test)]
fn reaction_registration_keeps_brand_errors_and_schedules_each_settled_outcome() {
    let mut vm = Vm::default();
    let ordinary = vm.heap.alloc_object(None).unwrap();
    assert_eq!(
        vm.perform_promise_then(
            ordinary,
            Value::Undefined,
            Value::Undefined,
            ReactionTarget::Native(ordinary),
        ),
        Err(RuntimeError::TypeError("invalid Promise receiver".into()))
    );
    for (source, queued) in [
        ("var retained = new Promise(() => {}); retained", false),
        ("var retained = Promise.resolve(7); retained", true),
        (
            "var retained = Promise.reject(8); retained.catch(() => {}); retained",
            true,
        ),
    ] {
        let receiver = execute(&mut vm, source).object_id().unwrap();
        let target = vm.new_promise().unwrap();
        let target_root = vm.heap.root(target).unwrap();
        let before = vm.promise_jobs.len();
        vm.perform_promise_then(
            receiver,
            Value::Undefined,
            Value::Undefined,
            ReactionTarget::Native(target),
        )
        .unwrap();
        assert_eq!(vm.promise_jobs.len(), before + usize::from(queued));
        if !queued {
            assert_eq!(vm.promises[&receiver].reactions.len(), 1);
        }
        assert_eq!(execute(&mut vm, "21 + 21"), Value::Number(42.0));
        vm.heap.unroot(target_root).unwrap();
    }
}

#[cfg_attr(test, test)]
fn promise_then_retains_handlers_through_allocation_and_species_callbacks() {
    for nursery_capacity in [256, 1] {
        for species in [false, true] {
            let mut vm = Vm::new(VmConfig {
                heap: HeapConfig {
                    nursery_capacity,
                    ..HeapConfig::default()
                },
                ..VmConfig::default()
            })
            .unwrap();
            // Retain the handler only while preparing the receiver. The then
            // boundary must retain it during derived allocation and getters.
            let handler = execute(&mut vm, "value => value + 1");
            let root = vm.heap.root(handler.object_id().unwrap()).unwrap();
            let receiver = execute(
                &mut vm,
                r#"
                var speciesReads = 0;
                var holder = {get [Symbol.species]() {
                    speciesReads++;
                    for (var i = 0; i < 16; i++) {({value:i});}
                    return Promise;
                }};
                var source = Promise.resolve(41);
                Object.defineProperty(source, 'constructor', {get() {
                    for (var i = 0; i < 16; i++) {({value:i});}
                    return holder;
                }});
                source
                "#,
            );
            vm.heap.unroot(root).unwrap();
            let base = vm.stack.len();
            let derived = if species {
                vm.promise_prototype_then(&receiver, &[handler]).unwrap()
            } else {
                vm.promise_then(&receiver, &[handler]).unwrap()
            };
            assert_eq!(vm.stack.len(), base);
            vm.run_promise_jobs().unwrap();
            assert!(matches!(
                vm.promises[&derived.object_id().unwrap()].status,
                PromiseStatus::Fulfilled(Value::Number(42.0))
            ));
            assert_eq!(
                execute(&mut vm, "speciesReads"),
                Value::Number(if species { 1.0 } else { 0.0 })
            );
        }
    }
}

#[cfg_attr(test, test)]
fn promise_then_refusals_restore_operands_and_allow_reuse() {
    for species in [false, true] {
        let mut vm = Vm::default();
        let receiver = execute(&mut vm, "new Promise(() => {})");
        vm.stack.push(Value::Number(17.0));
        let before = vm.stack.clone();
        collect_vm_roots(&mut vm);
        let limit = vm.heap.allow_only(0);
        let result = if species {
            vm.promise_prototype_then(&receiver, &[])
        } else {
            vm.promise_then(&receiver, &[])
        };
        assert_eq!(
            result,
            Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
        );
        assert_eq!(vm.stack, before);
        vm.heap.allow_only(16 * 1024 * 1024);
        let derived = if species {
            vm.promise_prototype_then(&receiver, &[]).unwrap()
        } else {
            vm.promise_then(&receiver, &[]).unwrap()
        };
        assert!(vm.promises.contains_key(&derived.object_id().unwrap()));
        assert_eq!(vm.stack, before);
        vm.stack.clear();
        assert_eq!(execute(&mut vm, "21 + 21"), Value::Number(42.0));
    }
    let mut vm = Vm::default();
    let receiver = execute(
        &mut vm,
        "var observed = 0; ({get constructor() {observed++; throw 7;}})",
    );
    let before = vm.stack.clone();
    assert_eq!(
        vm.promise_prototype_then(&receiver, &[]),
        Err(RuntimeError::TypeError(
            "Promise.prototype.then receiver".into()
        ))
    );
    assert_eq!(vm.stack, before);
    assert_eq!(execute(&mut vm, "observed"), Value::Number(0.0));
    let receiver = execute(
        &mut vm,
        "var source = Promise.resolve(1); Object.defineProperty(source, 'constructor', {get() {throw 123;}}); source",
    );
    let before = vm.stack.clone();
    assert_eq!(
        vm.promise_prototype_then(&receiver, &[]),
        Err(RuntimeError::Thrown(Value::Number(123.0)))
    );
    assert_eq!(vm.stack, before);
    assert_eq!(execute(&mut vm, "21 + 21"), Value::Number(42.0));
}

#[cfg_attr(test, test)]
fn private_declarations_share_missing_name_errors_after_brand_validation() {
    let mut vm = Vm::default();
    let receiver = execute(
        &mut vm,
        "class Owner {static #field = 1; static #method() {return 42;} static get #accessor() {return 7;}} Owner",
    );
    let owner = receiver.object_id().unwrap();
    let missing: JsString = "absent".into();
    let expected = Err(RuntimeError::TypeError(
        "private element is not declared by this class".into(),
    ));
    assert_eq!(vm.private_get(&receiver, owner, &missing), expected);
    assert_eq!(
        vm.private_set(&receiver, owner, missing.clone(), Value::Number(2.0)),
        expected.map(|_| ())
    );
    assert_eq!(
        vm.private_get(&receiver, owner, &"field".into()),
        Ok(Value::Number(1.0))
    );
    vm.private_set(&receiver, owner, "field".into(), Value::Number(2.0))
        .unwrap();
    assert_eq!(
        vm.private_get(&receiver, owner, &"field".into()),
        Ok(Value::Number(2.0))
    );
    assert_eq!(
        vm.private_get(&receiver, owner, &"accessor".into()),
        Ok(Value::Number(7.0))
    );
    let unrelated = execute(&mut vm, "({})");
    assert_eq!(
        vm.private_get(&unrelated, owner, &missing),
        Err(RuntimeError::TypeError(
            "receiver does not have the requested private element".into()
        ))
    );
    let mut foreign = Vm::default();
    let foreign_owner = foreign.heap.alloc_object(None).unwrap();
    assert!(matches!(
        vm.private_element_or_throw(foreign_owner, &missing),
        Err(RuntimeError::Heap(HeapError::InvalidObject(id))) if id == foreign_owner
    ));
    assert_eq!(execute(&mut vm, "21 + 21"), Value::Number(42.0));
}

#[cfg_attr(test, test)]
fn regexp_classification_preserves_match_getters_and_primitive_results() {
    let mut vm = Vm::default();
    for source in [
        "undefined",
        "null",
        "true",
        "42",
        "'pattern'",
        "1n",
        "Symbol()",
    ] {
        let value = execute(&mut vm, source);
        assert_eq!(vm.is_regexp(&value), Ok(false));
    }
    for (source, expected) in [
        ("({})", false),
        ("({[Symbol.match]: true})", true),
        ("({[Symbol.match]: false})", false),
        ("/pattern/", true),
        (
            "Object.assign(/pattern/, {[Symbol.match]: undefined})",
            true,
        ),
    ] {
        let value = execute(&mut vm, source);
        assert_eq!(vm.is_regexp(&value), Ok(expected));
    }
    let value = execute(&mut vm, "({get [Symbol.match]() {throw 123;}})");
    let before = vm.stack.clone();
    assert_eq!(
        vm.is_regexp(&value),
        Err(RuntimeError::Thrown(Value::Number(123.0)))
    );
    assert_eq!(vm.stack, before);
    assert_eq!(execute(&mut vm, "21 + 21"), Value::Number(42.0));
}
