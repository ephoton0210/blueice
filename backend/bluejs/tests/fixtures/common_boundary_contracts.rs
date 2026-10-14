// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Shared classification, registration and receiver contracts in both builds.

use super::*;
use crate::heap::TemporalKind;
use crate::native::StringMethod;
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
        shared_array_classification_preserves_revocation_and_push_errors();
        shared_temporal_options_and_epoch_bounds_preserve_public_results();
        shared_numeric_conversion_preserves_signed_infinity_and_coercion();
        shared_promise_disposal_preserves_settlement_and_await_order();
        shared_iterator_result_refusals_restore_operands_and_allow_retry();
        shared_foreign_handle_ingress_remains_fallible();
        shared_json_and_class_producers_preserve_public_descriptors();
        shared_native_entry_guards_preserve_constructor_and_decimal_errors();
        shared_properties_preserve_ingress_and_copy_refusals();
        shared_cold_initializers_and_temporal_options_restore_roots();
        shared_foreign_array_prototype_refusal_restores_operands();
    }
}

#[cfg_attr(test, test)]
fn shared_array_classification_preserves_revocation_and_push_errors() {
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
            execute(
                &mut vm,
                r#"
            (() => {
                const wrapped = new Proxy(new Proxy([1, [2]], {}), {});
                if (!Array.isArray(wrapped) || JSON.stringify(wrapped) !== '[1,[2]]') throw 1;
                if (Array.prototype.flat.call(wrapped).join() !== '1,2') throw 2;
                const revoked = Proxy.revocable([], {}); revoked.revoke();
                try { Array.isArray(revoked.proxy); throw 3; }
                catch (e) { if (!(e instanceof TypeError)) throw e; }
                const parent = Proxy.revocable({}, {}), array = [];
                Object.setPrototypeOf(array, parent.proxy); parent.revoke();
                if (Array.prototype.push.call(array) !== 0) throw 4;
                try { Array.prototype.push.call(array, 1); throw 5; }
                catch (e) { if (!(e instanceof TypeError)) throw e; }
                if (array.length !== 0 || Object.hasOwn(array, '0')) throw 6;
                const ordinary = Object.create(parent.proxy);
                Object.defineProperty(ordinary, 'present', {value: 1, writable: true});
                if (!Reflect.set(ordinary, 'present', 2) || ordinary.present !== 2) throw 7;
                try { Reflect.set(ordinary, 'missing', 1); throw 8; }
                catch (e) { if (!(e instanceof TypeError)) throw e; }
                let reentrant;
                reentrant = Proxy.revocable({}, {getOwnPropertyDescriptor() {reentrant.revoke();}});
                try { Reflect.set({}, 'x', 1, reentrant.proxy); throw 9; }
                catch (e) { if (!(e instanceof TypeError)) throw e; }
                return !Object.hasOwn(ordinary, 'missing');
            })()
        "#
            ),
            Value::Bool(true)
        );
        assert!(vm.stack.is_empty());
        assert_eq!(execute(&mut vm, "21 + 21"), Value::Number(42.0));
    }
}

#[cfg_attr(test, test)]
fn shared_temporal_options_and_epoch_bounds_preserve_public_results() {
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
            execute(
                &mut vm,
                r#"
            (() => {
                const min = new Temporal.Instant(-8640000000000000000000n);
                const max = new Temporal.Instant(8640000000000000000000n);
                if (min.until(max, {largestUnit:'hour'}).hours !== 4800000000) throw 'difference';
                if (max.since(min, {largestUnit:'second'}).seconds !== 17280000000000) throw 'seconds';
                for (const instant of [min, max]) {
                    if (instant.round('nanosecond').epochNanoseconds !== instant.epochNanoseconds) throw 'round';
                    if (Temporal.Instant.from(instant.toString()).epochNanoseconds !== instant.epochNanoseconds) throw 'text';
                    for (const zone of ['UTC','Asia/Taipei']) {
                        const zoned = new Temporal.ZonedDateTime(0n, zone);
                        if (instant.toString({timeZone: zoned}) !== instant.toString({timeZone: zone})) throw 'zone';
                    }
                }
                for (const epoch of [-8640000000000000000001n,8640000000000000000001n]) {
                    try { new Temporal.Instant(epoch); throw 'bound'; }
                    catch (e) { if (!(e instanceof RangeError)) throw e; }
                }
                const time = new Temporal.PlainTime(1,2,3,4,5,6);
                const units = ['minute','second','millisecond','microsecond','nanosecond'];
                const texts = ['01:02','01:02:03','01:02:03.004','01:02:03.004005','01:02:03.004005006'];
                for (let i=0;i<units.length;i++) if (time.toString({smallestUnit:units[i]}) !== texts[i]) throw units[i];
                const trace = [], options = new Proxy({}, {get(_,key) {trace.push(key);return undefined;}});
                time.toString(options);
                if (trace.join() !== 'fractionalSecondDigits,roundingMode,smallestUnit') throw trace.join();
                const marker = {};
                for (const key of ['fractionalSecondDigits','roundingMode','smallestUnit']) {
                    const o = {[key]: {toString() {throw marker;},valueOf() {throw marker;}}};
                    try {time.toString(o); throw key;} catch(e) {if(e !== marker) throw e;}
                }
                for (const text of ['\ud800', '\udfff']) {
                    for (const op of [() => Temporal.Instant.from(text), () => Temporal.PlainTime.from(text),
                        () => time.toString({fractionalSecondDigits:text}), () => min.toString({timeZone:text})]) {
                        try {op();throw 'UTF16';} catch(e) {if(!(e instanceof RangeError)) throw e;}
                    }
                }
                const midnight = new Temporal.PlainTime(), end = new Temporal.PlainTime(23,59,59,999,999,999);
                return midnight.until(end).nanoseconds === 999 && end.until(midnight).hours === -23;
            })()
        "#
            ),
            Value::Bool(true)
        );
        assert!(vm.stack.is_empty());
    }
}

#[cfg_attr(test, test)]
fn shared_numeric_conversion_preserves_signed_infinity_and_coercion() {
    let mut vm = Vm::default();
    assert_eq!(
        execute(
            &mut vm,
            r#"
        (() => {
            const huge = 1n << 2048n;
            if (Number(huge) !== Infinity || Number(-huge) !== -Infinity || Number(0n) !== 0 || Number(-42n) !== -42) throw 'BigInt';
            const marker = {}, digits = {valueOf() {throw marker;}};
            for (const method of ['toFixed','toExponential','toPrecision']) {
                try {Number.prototype[method].call(Infinity,digits);throw method;}
                catch(e) {if(e !== marker) throw e;}
            }
            try {(Infinity).toFixed(101);throw 'fixed';} catch(e) {if(!(e instanceof RangeError)) throw e;}
            return (Infinity).toExponential(101) === 'Infinity' && (NaN).toPrecision(0) === 'NaN'
                && (1e21).toFixed(2) === '1e+21' && (-0).toPrecision() === '0';
        })()
    "#
        ),
        Value::Bool(true)
    );
}

#[cfg_attr(test, test)]
fn shared_promise_disposal_preserves_settlement_and_await_order() {
    for nursery_capacity in [256, 1] {
        let mut vm = Vm::new(VmConfig {
            heap: HeapConfig {
                nursery_capacity,
                ..HeapConfig::default()
            },
            ..VmConfig::default()
        })
        .unwrap();
        execute(
            &mut vm,
            r#"
            globalThis.sharedSettlement = false;
            (async () => {
                const trace = [], marker = {};
                const p = new Promise((resolve,reject) => {resolve(7); reject(marker); resolve(8);});
                if (await p !== 7) throw 'resolver';
                let closed = 0;
                const iterable = {[Symbol.iterator]() {return {
                    next() {return {done:false,value:Promise.reject(marker)};},
                    return() {closed++;throw 'close';}
                };}};
                try {for await (const value of iterable) {throw value;}} catch(e) {if(e !== marker) throw e;}
                if (closed !== 1) throw 'IteratorClose';
                const stack = new AsyncDisposableStack();
                stack.use(null);
                stack.use({[Symbol.dispose]() {trace.push('dispose'); return {then() {throw 'must not await';}};}});
                const result = stack.disposeAsync();
                trace.push('returned');
                await result;
                if (trace.join() !== 'dispose,returned') throw trace.join();
                await stack.disposeAsync();
                const sync = new DisposableStack();
                sync.defer(() => {throw 1;}); sync.defer(() => {throw 2;});
                try {sync.dispose();throw 'suppression';} catch(e) {
                    if(!(e instanceof SuppressedError) || e.error !== 1 || e.suppressed !== 2) throw e;
                }
                globalThis.sharedSettlement = true;
            })();
        "#,
        );
        vm.run_promise_jobs().unwrap();
        assert_eq!(execute(&mut vm, "sharedSettlement"), Value::Bool(true));
        assert!(vm.stack.is_empty());

        let resolve = execute(
            &mut vm,
            "globalThis.sharedPending = new Promise((resolve,reject) => {globalThis.sharedResolve=resolve;globalThis.sharedReject=reject;}); sharedResolve",
        )
        .object_id()
        .unwrap();
        let reject = execute(&mut vm, "sharedReject").object_id().unwrap();
        let resolve_tag = vm.heap.native_function(resolve).unwrap().unwrap();
        let reject_tag = vm.heap.native_function(reject).unwrap().unwrap();
        let NativeFunction::PromiseResolvingFunction { promise, state, .. } = resolve_tag else {
            panic!("the Promise constructor supplies a native resolver");
        };
        vm.stack.push(Value::Number(17.0));
        collect_vm_roots(&mut vm);
        let before = vm.stack.clone();
        assert_eq!(vm.heap.get_own(state, "resolved").unwrap(), None);
        let limit = vm.heap.allow_only(0);
        assert_eq!(
            vm.native_call(
                resolve_tag,
                Value::Undefined,
                vec![Value::Number(7.0)],
                false
            ),
            Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
        );
        assert_eq!(vm.stack, before);
        assert_eq!(vm.heap.get_own(state, "resolved").unwrap(), None);
        assert!(matches!(
            vm.promises[&promise].status,
            PromiseStatus::Pending
        ));
        vm.heap.allow_only(16 * 1024 * 1024);
        assert_eq!(
            vm.native_call(
                resolve_tag,
                Value::Undefined,
                vec![Value::Number(7.0)],
                false
            ),
            Ok(Value::Undefined)
        );
        assert_eq!(vm.stack, before);
        assert_eq!(
            vm.heap.get_own(state, "resolved").unwrap(),
            Some(Value::Bool(true))
        );
        vm.heap.allow_only(0);
        for tag in [resolve_tag, reject_tag] {
            assert_eq!(
                vm.native_call(tag, Value::Undefined, vec![Value::Number(8.0)], false),
                Ok(Value::Undefined)
            );
            assert_eq!(vm.stack, before);
            assert!(
                matches!(&vm.promises[&promise].status, PromiseStatus::Fulfilled(Value::Number(value)) if *value == 7.0)
            );
        }
        vm.heap.allow_only(16 * 1024 * 1024);
        vm.stack.pop();
        assert_eq!(execute(&mut vm, "21 + 21"), Value::Number(42.0));
    }
}

#[cfg_attr(test, test)]
fn shared_iterator_result_refusals_restore_operands_and_allow_retry() {
    let mut extra = 0;
    let mut refusals = 0;
    loop {
        let mut vm = Vm::default();
        let value = execute(&mut vm, "globalThis.sharedValue = {}; sharedValue");
        vm.stack.push(Value::Number(17.0));
        let before = vm.stack.clone();
        collect(&mut vm);
        let limit = vm.heap.allow_only(extra);
        let result = vm.iterator_result(value.clone(), false);
        assert_eq!(vm.stack, before);
        match result {
            Ok(result) => {
                let id = result.object_id().unwrap();
                assert_eq!(vm.heap.get_own(id, "value").unwrap(), Some(value));
                assert_eq!(
                    vm.heap.get_own(id, "done").unwrap(),
                    Some(Value::Bool(false))
                );
                assert!(refusals > 0);
                break;
            }
            Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit: actual })) => {
                assert_eq!(actual, limit);
                refusals += 1;
                extra = vm.heap.next_allocation_headroom(extra);
                assert!(extra <= 16 * 1024 * 1024);
            }
            Err(error) => panic!("iterator result: {error:?}"),
        }
        vm.stack.clear();
        vm.heap.allow_only(16 * 1024 * 1024);
        assert_eq!(execute(&mut vm, "21 + 21"), Value::Number(42.0));
    }
}

#[cfg_attr(test, test)]
fn shared_json_and_class_producers_preserve_public_descriptors() {
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
            execute(
                &mut vm,
                r#"
            (() => {
                const raw = JSON.rawJSON('123'), d = Object.getOwnPropertyDescriptor(raw, 'rawJSON');
                if(d.value !== '123' || d.writable || !d.enumerable || d.configurable || Object.isExtensible(raw)) throw 'raw';
                if(Object.getPrototypeOf(raw) !== null || JSON.stringify({raw}) !== '{"raw":123}') throw 'serialize';
                const symbol = Symbol('key'), obj = {[symbol]: class {static {delete this.name;}}};
                if(obj[symbol].name !== '[key]') throw 'name';
                class C {#x = 3; get() {return this.#x;} static read(o) {return o.#x;}}
                if(new C().get() !== 3) throw 'private';
                try {C.read({});throw 'brand';} catch(e) {if(!(e instanceof TypeError)) throw e;}
                const text = JSON.stringify(['😀', '\ud800', 'a\nb', {x:1}], [0,'x']);
                return JSON.parse(text)[0] === '😀' && JSON.parse(text)[1] === '\ud800';
            })()
        "#
            ),
            Value::Bool(true)
        );
    }
}

#[cfg_attr(test, test)]
fn shared_foreign_handle_ingress_remains_fallible() {
    let mut owner = Vm::default();
    let id = owner.heap.alloc_object(None).unwrap();
    let root = owner.heap.root(id).unwrap();
    let foreign = Value::Object(id);
    let mut vm = Vm::default();
    let inputs = execute(&mut vm, "globalThis.sharedInputs = [[], new Promise(()=>{}), new DisposableStack(), new Temporal.PlainTime(), {}, new AsyncDisposableStack()]; sharedInputs").object_id().unwrap();
    let value = |vm: &Vm, index: &str| vm.heap.get_own(inputs, index).unwrap().unwrap();
    let array = value(&vm, "0");
    let promise = value(&vm, "1");
    let stack = value(&vm, "2");
    let time = value(&vm, "3");
    let object = value(&vm, "4");
    let async_stack = value(&vm, "5");
    let expected = Err(RuntimeError::Heap(HeapError::InvalidObject(id)));
    let arrays: &[Operation] = &[
        |v, r, f| v.array_flat_map(r, f, &Value::Undefined),
        |v, r, f| v.array_for_each(r, f, &Value::Undefined),
        |v, r, f| v.array_map(r, f, &Value::Undefined),
        |v, r, f| v.array_filter(r, f, &Value::Undefined),
        |v, r, f| v.array_every(r, f, &Value::Undefined),
        |v, r, f| v.array_find(r, f, &Value::Undefined, false, false),
        |v, r, f| v.array_reduce(r, std::slice::from_ref(f)),
        |v, r, f| v.array_reduce_right(r, std::slice::from_ref(f)),
        |v, r, f| v.array_sort(r, f),
    ];
    for operation in arrays {
        assert_eq!(operation(&mut vm, &array, &foreign), expected);
        assert!(vm.stack.is_empty());
    }
    assert_eq!(vm.promise_constructor(foreign.clone(), true), expected);
    assert_eq!(
        vm.new_promise_capability(&foreign)
            .map(|_| Value::Undefined),
        expected
    );
    assert_eq!(vm.promise_finally(&promise, &foreign), expected);
    // Published properties reject an object from another heap. Keep the
    // negative ingress at that real boundary instead of fabricating storage.
    assert_eq!(
        vm.heap
            .set(object.object_id().unwrap(), "callback", foreign.clone()),
        Err(HeapError::InvalidObject(id))
    );
    assert_eq!(
        vm.heap
            .get_own(object.object_id().unwrap(), "callback")
            .unwrap(),
        None
    );
    for source in [
        "(function C(executor) {executor({},()=>{});})",
        "(function C(executor) {executor(()=>{},{});})",
    ] {
        let constructor = execute(&mut vm, source);
        assert!(matches!(
            vm.new_promise_capability(&constructor),
            Err(RuntimeError::TypeError(_))
        ));
        assert!(vm.stack.is_empty());
    }
    for (receiver, is_async) in [(&stack, false), (&async_stack, true)] {
        assert_eq!(
            vm.disposable_stack_adopt(receiver, Value::Undefined, foreign.clone(), is_async),
            expected
        );
        assert_eq!(
            vm.disposable_stack_defer(receiver, foreign.clone(), is_async),
            expected
        );
    }
    assert_eq!(
        vm.temporal_to_instant_epoch(&foreign)
            .map(|_| Value::Undefined),
        expected
    );
    assert_eq!(
        vm.temporal_to_plain_time(&foreign, &Value::Undefined)
            .map(|_| Value::Undefined),
        expected
    );
    let receiver = vm
        .validate_temporal_receiver(&time, TemporalKind::PlainTime)
        .unwrap();
    assert_eq!(
        vm.temporal_plain_time_with(&receiver, &foreign, &Value::Undefined),
        expected
    );
    assert_eq!(vm.number_receiver(&foreign).map(Value::Number), expected);
    assert_eq!(
        vm.dispatch_string_method(StringMethod::ToString, &foreign, &[]),
        expected
    );
    for all in [false, true] {
        assert_eq!(
            vm.string_replace(
                &Value::String("a".into()),
                &[Value::String("a".into()), foreign.clone()],
                all,
            ),
            expected
        );
        assert!(vm.stack.is_empty());
    }
    assert_eq!(vm.is_array(&foreign).map(Value::Bool), expected);
    assert_eq!(vm.json_is_raw_json(&foreign), expected);
    assert_eq!(
        vm.json_parse(&Value::String("1".into()), Some(&foreign)),
        expected
    );
    assert_eq!(vm.json_stringify(&[Value::Null, foreign.clone()]), expected);
    assert_eq!(
        vm.json_stringify(&[Value::Null, Value::Undefined, foreign.clone()]),
        expected
    );
    assert_eq!(
        vm.json_stringify(std::slice::from_ref(&array)),
        Ok(Value::String("[]".into()))
    );
    for native in [
        NativeFunction::MapSize,
        NativeFunction::SetSize,
        NativeFunction::ArrayIteratorNext,
        NativeFunction::TypedArrayByteLength,
        NativeFunction::TypedArrayByteOffset,
        NativeFunction::TypedArrayLength,
        NativeFunction::FunctionToString,
        NativeFunction::Apply,
        NativeFunction::SymbolToString,
        NativeFunction::SymbolDescription,
        NativeFunction::BigIntToString,
        NativeFunction::BigIntValueOf,
        NativeFunction::BigIntToLocaleString,
        NativeFunction::IteratorNext,
        NativeFunction::PrimitiveMethod {
            boolean: true,
            string: false,
        },
    ] {
        assert_eq!(
            vm.native_call(native, foreign.clone(), vec![], false),
            expected,
            "{native:?}"
        );
    }
    assert_eq!(
        vm.native_call(
            NativeFunction::ReflectApply,
            Value::Undefined,
            vec![foreign.clone()],
            false
        ),
        expected
    );
    assert_eq!(
        vm.native_call(
            NativeFunction::ReflectConstruct,
            Value::Undefined,
            vec![foreign.clone()],
            false
        ),
        expected
    );
    assert_eq!(
        vm.native_call(
            NativeFunction::PrimitiveConstructor(true),
            Value::Undefined,
            vec![foreign.clone()],
            false
        ),
        expected
    );
    assert_eq!(
        vm.native_call(
            NativeFunction::ArrayToString,
            foreign.clone(),
            vec![],
            false
        ),
        expected
    );
    assert_eq!(
        vm.native_call(
            NativeFunction::ObjectDefineAccessor { getter: true },
            object.clone(),
            vec![Value::String("key".into()), foreign.clone()],
            false
        ),
        expected
    );
    assert_eq!(
        vm.native_call(
            NativeFunction::ObjectToLocaleString,
            foreign.clone(),
            vec![],
            false
        ),
        expected
    );
    let object_constructor = vm.global("Object").unwrap();
    assert_eq!(
        vm.native_call(
            NativeFunction::ReflectConstruct,
            Value::Undefined,
            vec![object_constructor, Value::Undefined, foreign.clone()],
            false
        ),
        expected
    );
    assert!(vm.stack.is_empty());
    owner.heap.unroot(root).unwrap();
    assert_eq!(execute(&mut vm, "21 + 21"), Value::Number(42.0));

    vm.install_test262_harness().unwrap();
    assert_eq!(
        execute(
            &mut vm,
            r#"
        (() => {
            const marker = {}, realm = $262.createRealm();
            const method = realm.global.eval('() => "foreign"');
            if ([{toLocaleString: method}].toLocaleString() !== 'foreign') throw 'locale';
            if (JSON.stringify({toJSON: realm.global.eval('() => 7')}) !== '7') throw 'json';
            const revoked = Proxy.revocable(() => 1, {}); revoked.revoke();
            for (const operation of [
                () => JSON.stringify({get toJSON() {throw marker;}}),
                () => Object.prototype.toLocaleString.call({get toString() {throw marker;}}),
                () => Array.prototype.toString.call({get join() {throw marker;}}),
                () => Number({get valueOf() {throw marker;}})
            ]) {
                try {operation();throw 'getter';} catch(e) {if(e !== marker) throw e;}
            }
            for (const operation of [
                () => JSON.stringify({toJSON: revoked.proxy}),
                () => [{toLocaleString: revoked.proxy}].toLocaleString(),
                () => Object.prototype.toLocaleString.call({toString: revoked.proxy}),
                () => Array.prototype.toString.call({join: revoked.proxy}),
                () => Number({valueOf: revoked.proxy}),
                () => String({[Symbol.toPrimitive]: {}})
            ]) {
                try {operation();throw 'callback';} catch(e) {if(!(e instanceof TypeError)) throw e;}
            }
            const symbol = Object(Symbol('s')), bigint = Object(3n);
            delete Symbol.prototype[Symbol.toStringTag];
            delete BigInt.prototype[Symbol.toStringTag];
            return Object.prototype.toString.call(symbol) === '[object Object]'
                && Object.prototype.toString.call(bigint) === '[object Object]';
        })()
    "#
        ),
        Value::Bool(true)
    );
    assert!(vm.stack.is_empty());

    // Keep the established released-realm ingress contract at native entry.
    // The facade is produced normally; no private payload is fabricated.
    let released = execute(&mut vm, "$262.createRealm().global.eval('Object(3n)')");
    vm.test262_realms.clear();
    let released_error = Err(RuntimeError::TypeError(
        "foreign Test262 realm is no longer available".into(),
    ));
    for native in [
        NativeFunction::SymbolToString,
        NativeFunction::SymbolDescription,
        NativeFunction::BigIntToString,
        NativeFunction::BigIntToLocaleString,
    ] {
        assert_eq!(
            vm.native_call(native, released.clone(), vec![], false),
            released_error,
            "{native:?}"
        );
        assert!(vm.stack.is_empty());
    }
}

#[cfg_attr(test, test)]
fn shared_native_entry_guards_preserve_constructor_and_decimal_errors() {
    let mut vm = Vm::default();
    // Obtain immutable tags from installed functions rather than inventing private payloads.
    for source in [
        "Symbol",
        "Iterator.from",
        "new ShadowRealm().evaluate('(x)=>x')",
    ] {
        let function = execute(&mut vm, source).object_id().unwrap();
        let tag = vm.heap.native_function(function).unwrap().unwrap();
        assert!(
            matches!(
                vm.native_call(tag, Value::Undefined, vec![], true),
                Err(RuntimeError::TypeError(_))
            ),
            "{source}"
        );
    }
    assert_eq!(
        execute(
            &mut vm,
            r#"
        (() => {
            const decimal = BigInt('1' + '0'.repeat(32769));
            try { decimal.toLocaleString('en'); throw 'decimal capacity'; }
            catch(e) {return e instanceof RangeError;}
        })()
    "#
        ),
        Value::Bool(true)
    );
    assert!(vm.stack.is_empty());
}

#[cfg_attr(test, test)]
fn shared_properties_preserve_ingress_and_copy_refusals() {
    let mut owner = Vm::default();
    let foreign_id = owner.heap.alloc_object(None).unwrap();
    let root = owner.heap.root(foreign_id).unwrap();
    let foreign = Value::Object(foreign_id);
    let expected = Err(RuntimeError::Heap(HeapError::InvalidObject(foreign_id)));
    let mut vm = Vm::default();
    let class = execute(
        &mut vm,
        "globalThis.SharedOwner = class {static #field = 1;}; SharedOwner",
    )
    .object_id()
    .unwrap();
    assert_eq!(
        vm.private_receiver(&foreign, class)
            .map(|_| Value::Undefined),
        expected
    );
    assert_eq!(vm.super_call(foreign.clone(), vec![]), expected);
    assert_eq!(
        vm.private_field_add(&foreign, class, "field".into(), Value::Undefined)
            .map(|_| Value::Undefined),
        expected
    );
    assert_eq!(
        vm.set_function_name_from_key(&foreign, &Value::String("key".into()), 0)
            .map(|_| Value::Undefined),
        expected
    );
    assert_eq!(
        vm.set_property(&foreign, &"x".into(), &Value::Undefined)
            .map(|_| Value::Undefined),
        expected
    );
    assert!(matches!(
        vm.private_field_add(&Value::Number(1.0), class, "field".into(), Value::Undefined),
        Err(RuntimeError::TypeError(_))
    ));
    assert!(matches!(
        vm.private_field_add(
            &Value::Object(class),
            class,
            "field".into(),
            Value::Undefined
        ),
        Err(RuntimeError::TypeError(_))
    ));
    assert!(vm
        .set_function_name_from_key(&Value::Number(1.0), &Value::String("key".into()), 0)
        .is_ok());
    let source = execute(&mut vm, "globalThis.copySource = {x:1}; copySource");
    let target = execute(
        &mut vm,
        "globalThis.copyTarget = Object.preventExtensions({}); copyTarget",
    )
    .object_id()
    .unwrap();
    assert!(matches!(
        vm.copy_data_properties(target, &source, &[]),
        Err(RuntimeError::TypeError(_))
    ));
    for excluded in [
        "({get length() {throw 11;}})",
        "({length:{valueOf() {throw 11;}}})",
        "({length:1,get 0() {throw 11;}})",
        "({length:1,0:{toString() {throw 11;}}})",
    ] {
        let excluded = execute(&mut vm, excluded);
        assert_eq!(
            vm.destructure_object_rest(&source, &excluded),
            Err(RuntimeError::Thrown(Value::Number(11.0)))
        );
        assert!(vm.stack.is_empty());
    }
    vm.remaining_instructions = vm.config.instruction_budget;
    assert!(matches!(
        vm.build_async_dispose_state(vec![], Some(RuntimeError::InstructionLimit)),
        Err(RuntimeError::InstructionLimit)
    ));
    assert!(vm.stack.is_empty());
    owner.heap.unroot(root).unwrap();
    assert_eq!(execute(&mut vm, "21+21"), Value::Number(42.0));
}

#[cfg_attr(test, test)]
fn shared_cold_initializers_and_temporal_options_restore_roots() {
    type Initializer = fn(&mut Vm) -> Result<Value, RuntimeError>;
    let cases: &[(&str, Initializer)] = &[
        ("JSON", |v| v.json_global()),
        ("done", |v| {
            v.install_test262_done().map(|_| Value::Undefined)
        }),
        ("sync prototype", |v| {
            v.disposable_stack_prototype(false).map(Value::Object)
        }),
        ("async prototype", |v| {
            v.disposable_stack_prototype(true).map(Value::Object)
        }),
        ("sync constructor", |v| {
            v.disposable_stack_constructor(false, true)
        }),
        ("async constructor", |v| {
            v.disposable_stack_constructor(true, true)
        }),
    ];
    for &(name, initialize) in cases {
        let mut extra = 0;
        let mut refusals = 0;
        loop {
            let mut vm = Vm::new(VmConfig {
                heap: HeapConfig {
                    nursery_capacity: 1,
                    ..HeapConfig::default()
                },
                ..VmConfig::default()
            })
            .unwrap();
            if name.ends_with("constructor") {
                vm.new_target = vm.global("Object").unwrap();
            }
            vm.remaining_instructions = vm.config.instruction_budget;
            collect(&mut vm);
            let limit = vm.heap.allow_only(extra);
            match initialize(&mut vm) {
                Ok(_) => {
                    assert!(refusals > 0, "{name}");
                    break;
                }
                Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit: actual })) => {
                    assert_eq!(actual, limit, "{name}");
                    refusals += 1;
                    extra = vm.heap.next_allocation_headroom(extra);
                    assert!(extra <= 16 * 1024 * 1024, "{name}");
                }
                Err(error) => panic!("{name}, headroom {extra}: {error:?}"),
            }
            assert!(vm.stack.is_empty(), "{name}");
            vm.heap.allow_only(16 * 1024 * 1024);
            collect(&mut vm);
            initialize(&mut vm).unwrap();
            assert!(vm.stack.is_empty(), "{name}");
        }
    }
    let mut vm = Vm::default();
    let time = execute(
        &mut vm,
        "globalThis.coldTime = new Temporal.PlainTime(1); coldTime",
    );
    let receiver = vm
        .validate_temporal_receiver(&time, TemporalKind::PlainTime)
        .unwrap();
    collect(&mut vm);
    let limit = vm.heap.allow_only(0);
    assert_eq!(
        vm.temporal_plain_time_round(&receiver, &Value::String("second".into())),
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
    );
    assert!(vm.stack.is_empty());
    vm.heap.allow_only(16 * 1024 * 1024);
    vm.config.max_string_bytes = 4;
    assert_eq!(
        vm.json_parse(&Value::String("\"abcdef\"".into()), None),
        Err(RuntimeError::StringLimit { limit: 4 })
    );
    assert!(vm.stack.is_empty());
}

#[cfg_attr(test, test)]
fn shared_foreign_array_prototype_refusal_restores_operands() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let realm = execute(
        &mut vm,
        "globalThis.sharedRealm = $262.createRealm(); sharedRealm.global",
    )
    .object_id()
    .unwrap();
    let receiver = execute(
        &mut vm,
        "globalThis.sharedArrayLike = {length:0}; sharedArrayLike",
    );
    vm.acting_realm = Some(realm);
    vm.remaining_instructions = vm.config.instruction_budget;
    let limit = vm
        .test262_realms
        .get_mut(&realm)
        .unwrap()
        .vm
        .heap
        .allow_only(0);
    assert_eq!(
        vm.array_flat(&receiver, &Value::Undefined),
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
    );
    assert!(vm.stack.is_empty());
    vm.test262_realms
        .get_mut(&realm)
        .unwrap()
        .vm
        .heap
        .allow_only(16 * 1024 * 1024);
    assert!(vm.array_flat(&receiver, &Value::Undefined).is_ok());
    assert!(vm.stack.is_empty());
    vm.acting_realm = None;
    assert_eq!(execute(&mut vm, "21+21"), Value::Number(42.0));
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
