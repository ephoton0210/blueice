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
        algorithm_public_options_preserve_abrupt_completion_and_iterator_close();
        algorithm_public_parts_keep_unicode_empty_elements_and_duration_units();
        algorithm_heap_ingress_rejects_foreign_handles_without_panicking();
        algorithm_for_in_tracks_mutation_shadowing_symbols_and_proxy_cycles();
        algorithm_allocator_refusals_restore_operands_and_allow_retry();
        algorithm_cold_initialization_refusals_and_string_limits_remain_fallible();
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

fn evaluate(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}: {error:?}"))
}

fn collect(vm: &mut Vm) {
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
}

#[cfg_attr(test, test)]
fn algorithm_public_options_preserve_abrupt_completion_and_iterator_close() {
    for nursery_capacity in [256, 1] {
        let mut vm = Vm::new(VmConfig {
            heap: HeapConfig {
                nursery_capacity,
                ..HeapConfig::default()
            },
            ..VmConfig::default()
        })
        .unwrap();
        assert_eq!(
            evaluate(
                &mut vm,
                r#"
            var poison = {};
            function throwsSame(f) {try {f();} catch (e) {if(e === poison) return;} throw 'lost throw';}
            function range(f) {try {f();} catch (e) {if(e instanceof RangeError) return;} throw 'missing RangeError';}
            function type(f) {try {f();} catch (e) {if(e instanceof TypeError) return;} throw 'missing TypeError';}
            for (var C of [Intl.ListFormat, Intl.DurationFormat]) {
                type(() => C());
                type(() => new C('en', null));
                range(() => new C('en_XX'));
                range(() => C.supportedLocalesOf('en_XX'));
                throwsSame(() => new C('en', {get localeMatcher() {throw poison;}}));
                throwsSame(() => Reflect.construct(C, ['en'], new Proxy(function(){}, {get(t,k) {if(k === 'prototype') throw poison; return Reflect.get(t,k);}})));
                type(() => C.supportedLocalesOf(['en'], null));
                throwsSame(() => C.supportedLocalesOf(['en'], {get localeMatcher() {throw poison;}}));
                range(() => C.supportedLocalesOf(['en'], {localeMatcher:'bad'}));
                for (var method of [C.prototype.format, C.prototype.formatToParts, C.prototype.resolvedOptions]) {
                    type(() => method.call(undefined));
                    type(() => method.call({}));
                }
            }
            for (var name of ['type','style']) {
                throwsSame(() => new Intl.ListFormat('en', {get [name](){throw poison;}}));
                throwsSame(() => new Intl.ListFormat('en', {[name]:{toString(){throw poison;}}}));
                type(() => new Intl.ListFormat('en', {[name]:Symbol()}));
                range(() => new Intl.ListFormat('en', {[name]:'\ud800'}));
                range(() => new Intl.ListFormat('en', {[name]:'bad'}));
            }
            for (var name of ['numberingSystem','yearsDisplay','fractionalDigits']) {
                throwsSame(() => new Intl.DurationFormat('en', {get [name](){throw poison;}}));
            }
            throwsSame(() => new Intl.DurationFormat('en', {fractionalDigits:{valueOf(){throw poison;}}}));
            type(() => new Intl.DurationFormat('en', {fractionalDigits:Symbol()}));
            range(() => new Intl.DurationFormat('en', {hours:'numeric', minutes:'long'}));
            range(() => new Intl.DurationFormat('en', {milliseconds:'numeric', millisecondsDisplay:'always'}));
            range(() => new Intl.DurationFormat('en', {milliseconds:'numeric', microseconds:'long'}));
            var formatter = new Intl.DurationFormat('en');
            for (var method of [formatter.format, formatter.formatToParts]) {
                throwsSame(() => method.call(formatter, {get years(){throw poison;}}));
                throwsSame(() => method.call(formatter, {seconds:{valueOf(){throw poison;}}}));
                type(() => method.call(formatter, {seconds:Symbol()}));
                range(() => method.call(formatter, '\ud800'));
                range(() => method.call(formatter, {hours:1, seconds:-1}));
                type(() => method.call(formatter, new Temporal.PlainTime(1)));
            }
            var list = new Intl.ListFormat('en');
            var closed = 0;
            var iterable = {[Symbol.iterator](){return {next(){return {value:7,done:false};}, return(){closed++; throw poison;}};}};
            type(() => list.format(iterable));
            type(() => list.formatToParts(iterable));
            var abrupt = {[Symbol.iterator](){return {next(){throw poison;},return(){closed++; throw 9;}};}};
            throwsSame(() => list.format(abrupt));
            throwsSame(() => list.formatToParts(abrupt));
            closed === 2
        "#
            ),
            Value::Bool(true)
        );
        assert!(vm.stack.is_empty());
        assert_eq!(evaluate(&mut vm, "21 + 21"), Value::Number(42.0));
    }
}

#[cfg_attr(test, test)]
fn algorithm_public_parts_keep_unicode_empty_elements_and_duration_units() {
    for nursery_capacity in [256, 1] {
        let mut vm = Vm::new(VmConfig {
            heap: HeapConfig {
                nursery_capacity,
                ..HeapConfig::default()
            },
            ..VmConfig::default()
        })
        .unwrap();
        assert_eq!(
            evaluate(
                &mut vm,
                r#"
            for (var locale of ['en', 'ach', 'zh', 'es', 'he']) {
                for (var type of ['conjunction','disjunction','unit']) {
                    for (var style of ['long','short','narrow']) {
                        var list = new Intl.ListFormat(locale, {type, style});
                        for (var values of [[], [''], ['a'], ['a','b'], ['a','b','c','d'], ['', '\ud800', '', 'x', '']]) {
                            var parts = list.formatToParts(values);
                            if (parts.map(p => p.value).join('') !== list.format(values)) throw 'different list parts';
                            if (values.length > 1 && values.includes('\ud800')) {
                                var elements = parts.filter(p => p.type === 'element').map(p => p.value).join('');
                                if(elements !== '\ud800x') throw 'lost empty/surrogate association';
                            }
                        }
                        var options = list.resolvedOptions();
                        if (options.type !== type || options.style !== style) throw 'list options';
                        if (list.format() !== '') throw 'undefined list';
                    }
                }
            }
            var duration = {years:1,months:2,weeks:3,days:4,hours:5,minutes:6,seconds:7,milliseconds:8,microseconds:9,nanoseconds:10};
            for (var style of ['long','short','narrow','digital']) {
                var f = new Intl.DurationFormat('en', {style, fractionalDigits:9});
                for (var value of [duration, 'PT1.23456789S', new Temporal.Duration(1,2,3,4,5,6,7,8,9,10), {seconds:-7,milliseconds:-8}, {seconds:0}]) {
                    if (f.formatToParts(value).map(p => p.value).join('') !== f.format(value)) throw 'duration parts';
                }
                if (f.resolvedOptions().style !== style) throw 'duration style';
            }
            for (var unit of ['years','months','weeks','days','hours','minutes','seconds','milliseconds','microseconds','nanoseconds']) {
                for (var style of ['long','short','narrow']) {
                    for (var display of ['auto','always']) {
                        var f = new Intl.DurationFormat('en', {[unit]:style, [unit+'Display']:display});
                        var options = f.resolvedOptions();
                        if(options[unit] !== style || options[unit+'Display'] !== display) throw 'duration unit options';
                        var value = {[unit]:7};
                        if (f.formatToParts(value).map(p => p.value).join('') !== f.format(value)) throw 'duration unit';
                    }
                }
            }
            for (var style of ['numeric','2-digit']) {
                var f = new Intl.DurationFormat('en', {hours:style, minutes:style, seconds:style});
                f.formatToParts({hours:1,minutes:2,seconds:3});
                if(f.resolvedOptions().hours !== style) throw 'numeric duration';
            }
            new Temporal.Duration(0,0,0,0,1).toLocaleString('en').length > 0
        "#
            ),
            Value::Bool(true)
        );
        assert!(vm.stack.is_empty());
    }
}

#[cfg_attr(test, test)]
fn algorithm_heap_ingress_rejects_foreign_handles_without_panicking() {
    let mut owner = Vm::default();
    let id = owner.heap.alloc_object(None).unwrap();
    let root = owner.heap.root(id).unwrap();
    let mut vm = Vm::default();
    let value = Value::Object(id);
    let error = RuntimeError::Heap(HeapError::InvalidObject(id));
    assert!(matches!(vm.list_format_data(&value), Err(e) if e == error));
    assert!(matches!(vm.duration_format_data(&value), Err(e) if e == error));
    assert!(matches!(vm.duration_record(&value), Err(e) if e == error));
    assert_eq!(vm.is_for_in_record(id), Err(error.clone()));
    assert_eq!(vm.for_in_step(id), Err(error));
    assert!(vm.stack.is_empty());
    owner.heap.unroot(root).unwrap();
    collect(&mut owner);
    assert_eq!(evaluate(&mut vm, "21 + 21"), Value::Number(42.0));
}

#[cfg_attr(test, test)]
fn algorithm_for_in_tracks_mutation_shadowing_symbols_and_proxy_cycles() {
    for nursery_capacity in [256, 1] {
        let mut vm = Vm::new(VmConfig {
            heap: HeapConfig {
                nursery_capacity,
                ..HeapConfig::default()
            },
            ..VmConfig::default()
        })
        .unwrap();
        assert_eq!(
            evaluate(
                &mut vm,
                r#"
            var base = {hidden:1, shared:2, tail:3};
            var object = Object.create(base);
            Object.defineProperty(object, 'hidden', {value:4, enumerable:false});
            object.first = 1; object.second = 2; object.shared = 5; object[Symbol()] = 6;
            var names = [];
            for (var key in object) {names.push(key); if(key === 'first') delete object.second;}
            if(names.join(',') !== 'first,shared,tail') throw 'mutation/shadowing';
            var cyc;
            cyc = new Proxy({a:1}, {getPrototypeOf(){return cyc;}});
            names = []; for(var key in cyc) names.push(key);
            if(names.join(',') !== 'a') throw 'cycle';
            var removed = new Proxy({a:1}, {ownKeys(){return ['a'];},getOwnPropertyDescriptor(){return undefined;}});
            names = []; for(var key in removed) names.push(key);
            if(names.length) throw 'missing descriptor';
            for(var trap of ['ownKeys','getOwnPropertyDescriptor','getPrototypeOf']) {
                var poison = {};
                var proxy = new Proxy({a:1}, {[trap](){throw poison;}});
                var rejected = false;
                try {for(var key in proxy) {}} catch(e) {rejected = e === poison;}
                if(!rejected) throw 'lost Proxy error';
            }
            var count = 0;
            for(var key in null) count++;
            for(var key in undefined) count++;
            for(var key in 'ab') count++;
            count === 2
        "#
            ),
            Value::Bool(true)
        );
        assert!(vm.stack.is_empty());
    }
}

type Operation = fn(&mut Vm, &Value, &Value) -> Result<Value, RuntimeError>;

#[cfg_attr(test, test)]
fn algorithm_allocator_refusals_restore_operands_and_allow_retry() {
    let cases: &[(&str, &str, Operation)] = &[
        ("list parts", "[new Intl.ListFormat('en'), ['a','b','c']]", |vm,r,v| vm.list_format_format_to_parts(r,v)),
        ("list options", "[new Intl.ListFormat('en'), undefined]", |vm,r,_| vm.list_format_resolved_options(r)),
        ("duration parts", "[new Intl.DurationFormat('en', {fractionalDigits:9}), {years:1,months:2,weeks:3,days:4,hours:5,minutes:6,seconds:7,milliseconds:8,microseconds:9,nanoseconds:10}]", |vm,r,v| vm.duration_format_format_to_parts(r,v)),
        ("duration options", "[new Intl.DurationFormat('en', {fractionalDigits:9}), undefined]", |vm,r,_| vm.duration_format_resolved_options(r)),
        ("list creation", "['en', undefined]", |vm,r,v| vm.create_list_format(&[r.clone(),v.clone()],true)),
        ("duration creation", "['en', undefined]", |vm,r,v| vm.create_duration_format(&[r.clone(),v.clone()],true)),
        ("duration locale creation", "['en', undefined]", |vm,r,v| vm.duration_format_for_locale_string(&[r.clone(),v.clone()])),
        ("for-in creation", "[{a:1,b:2,c:3}, undefined]", |vm,r,_| vm.for_in_iterator(r)),
        ("for-in traversal", "[{a:1,b:2,c:3}, undefined]", |vm,r,_| {
            let record = vm.for_in_iterator(r)?.object_id().unwrap();
            let root = vm.heap.root(record)?;
            let result = (|| {while vm.for_in_step(record)?.is_some() {} Ok(Value::Undefined)})();
            vm.heap.unroot(root)?;
            result
        }),
    ];
    for &(label, setup, operation) in cases {
        let program =
            compile(&parse(&format!("Intl; globalThis.inputs = {setup}; inputs")).unwrap())
                .unwrap();
        let mut extra = 0;
        let mut completed = false;
        let mut refusals = 0;
        while extra <= 16 * 1024 * 1024 {
            let mut vm = Vm::default();
            let inputs = vm.execute_script(&program).unwrap().object_id().unwrap();
            let receiver = vm.heap.get_own(inputs, "0").unwrap().unwrap();
            let value = vm.heap.get_own(inputs, "1").unwrap().unwrap();
            vm.remaining_instructions = vm.config.instruction_budget;
            // These helpers run inside a native constructor entry. Supply the
            // actual installed constructor as new.target, rather than leaving
            // the completed setup script's undefined new.target in place.
            if label == "list creation" {
                vm.new_target = Value::Object(vm.globals["%Intl.ListFormat%"]);
            } else if label == "duration creation" {
                vm.new_target = Value::Object(vm.globals["%Intl.DurationFormat%"]);
            }
            vm.stack.push(Value::Number(17.0));
            let before = vm.stack.clone();
            collect(&mut vm);
            let limit = vm.heap.allow_only(extra);
            match operation(&mut vm, &receiver, &value) {
                Ok(_) => completed = true,
                Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit: actual })) => {
                    assert_eq!(actual, limit, "{label}");
                    refusals += 1;
                    extra = vm.heap.next_allocation_headroom(extra);
                }
                Err(error) => panic!("{label}, headroom {extra}: {error:?}"),
            }
            assert_eq!(vm.stack, before, "{label}, headroom {extra}");
            vm.stack.clear();
            vm.heap.allow_only(16 * 1024 * 1024);
            assert_eq!(evaluate(&mut vm, "21 + 21"), Value::Number(42.0));
            if completed {
                break;
            }
        }
        assert!(
            completed && refusals > 0,
            "{label}: success={completed}, refusals={refusals}"
        );
    }
}

#[cfg_attr(test, test)]
fn algorithm_cold_initialization_refusals_and_string_limits_remain_fallible() {
    for duration in [false, true] {
        let mut vm = Vm::default();
        // This is the valid new.target of Reflect.construct(Intl.X, [], Object).
        // Object initialization leaves the Intl namespace cold.
        vm.new_target = vm.global("Object").unwrap();
        collect(&mut vm);
        let limit = vm.heap.allow_only(0);
        let result = if duration {
            vm.create_duration_format(&[], true)
        } else {
            vm.create_list_format(&[], true)
        };
        assert_eq!(
            result,
            Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
        );
        assert!(vm.stack.is_empty());
        vm.heap.allow_only(16 * 1024 * 1024);
        vm.remaining_instructions = vm.config.instruction_budget;
        let created = if duration {
            vm.create_duration_format(&[], true)
        } else {
            vm.create_list_format(&[], true)
        };
        assert!(created.is_ok(), "duration={duration}: {created:?}");
        assert!(vm.stack.is_empty());
    }
    let mut vm = Vm::default();
    let inputs = evaluate(
        &mut vm,
        "globalThis.inputs = [new Intl.ListFormat('en'), ['abcd','efgh']]; inputs",
    )
    .object_id()
    .unwrap();
    let receiver = vm.heap.get_own(inputs, "0").unwrap().unwrap();
    let values = vm.heap.get_own(inputs, "1").unwrap().unwrap();
    vm.remaining_instructions = vm.config.instruction_budget;
    vm.config.max_string_bytes = 8;
    assert!(matches!(
        vm.list_format_format(&receiver, &values),
        Err(RuntimeError::StringLimit { limit: 8 })
    ));
    assert!(vm.stack.is_empty());
    vm.config.max_string_bytes = VmConfig::default().max_string_bytes;
    assert!(vm.list_format_format(&receiver, &values).is_ok());
    assert!(vm.stack.is_empty());
}
