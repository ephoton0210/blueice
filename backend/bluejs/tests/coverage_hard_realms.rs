// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Observable cross-realm writes, buffer lifetimes and wrapped-call ownership.

use blueice_bluejs::{compile, compile_module, parse, parse_module, Value, Vm, VmConfig};
use std::collections::HashMap;

fn assert_realms(source: &str) {
    for nursery_capacity in [1, VmConfig::default().heap.nursery_capacity] {
        let mut config = VmConfig::default();
        config.heap.nursery_capacity = nursery_capacity;
        let mut vm = Vm::new(config).unwrap();
        vm.install_test262_harness().unwrap();
        assert_eq!(
            vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
                .unwrap(),
            Value::Bool(true),
            "nursery {nursery_capacity}: {source}"
        );
    }
}

#[test]
fn foreign_typed_array_from_interleaves_indexed_gets_mapping_and_writes() {
    assert_realms(
        r#"
        var other = $262.createRealm().global, log = [], receiver = {};
        var source = {
            get length() { log.push('length'); return 2; },
            get 0() { log.push('get0'); return 4; },
            get 1() { log.push('get1'); return 7; }
        };
        var result = Uint16Array.from.call(other.Uint16Array, source, function(v, i) {
            if (this !== receiver) throw 'wrong mapping receiver';
            log.push('map' + i);
            return v + i;
        }, receiver);
        Object.getPrototypeOf(result) === other.Uint16Array.prototype &&
            result.length === 2 && result[0] === 4 && result[1] === 8 &&
            log.join() === 'length,get0,map0,get1,map1'
    "#,
    );
}

#[test]
fn foreign_typed_array_from_and_of_preserve_abrupt_completion_identity() {
    assert_realms(
        r#"
        var other = $262.createRealm().global, sentinel = {}, passed = 0;
        var operations = [
            () => Uint16Array.from.call(other.Uint16Array, {length: 1, get 0() { throw sentinel; }}),
            () => Uint16Array.from.call(other.Uint16Array, {length: 1, 0: 1}, () => { throw sentinel; }),
            () => Uint16Array.from.call(other.Uint16Array, [1], () => { throw sentinel; }),
            () => Uint16Array.from.call(other.Uint16Array, {length: 1, 0: {valueOf() { throw sentinel; }}}),
            () => Uint16Array.of.call(other.Uint16Array, {valueOf() { throw sentinel; }})
        ];
        for (var operation of operations) {
            try { operation(); } catch (error) { if (error === sentinel) passed++; }
        }
        passed === operations.length && Uint16Array.of.call(other.Uint16Array, 9)[0] === 9
    "#,
    );
}

#[test]
fn foreign_typed_array_statics_preserve_numeric_and_bigint_kinds() {
    assert_realms(
        r#"
        var other = $262.createRealm().global;
        var numeric = Uint8Array.of.call(other.Uint8Array, 257, -1);
        var raw = Uint8Array.from.call(other.Uint8Array, {length: 2, 0: 258, 1: -2});
        var big = BigInt64Array.of.call(other.BigInt64Array, 1n, -2n);
        var empty = Uint8Array.from.call(other.Uint8Array, {length: 0});
        numeric[0] === 1 && numeric[1] === 255 && raw[0] === 2 && raw[1] === 254 &&
            big[0] === 1n && big[1] === -2n && empty.length === 0
    "#,
    );
}

#[test]
fn buffers_cross_the_membrane_with_resizable_shared_and_detached_state() {
    assert_realms(
        r#"
        var other = $262.createRealm().global;
        var local = new ArrayBuffer(4, {maxByteLength: 8});
        new Uint8Array(local)[0] = 31;
        other.local = local;
        var foreignView = other.eval('new Uint8Array(local)');
        var copied = new Uint8Array(other.eval('new ArrayBuffer(4, {maxByteLength: 8})'));
        var shared = new SharedArrayBuffer(4, {maxByteLength: 8});
        other.shared = shared;
        other.eval('Atomics.store(new Int32Array(shared), 0, 42)');
        var sharedOkay = Atomics.load(new Int32Array(shared), 0) === 42;
        $262.detachArrayBuffer(local);
        var detached;
        try { foreignView.slice(); detached = false; }
        catch (e) { detached = e.name === 'TypeError'; }
        copied.length === 4 && sharedOkay && detached
    "#,
    );
}

#[test]
fn foreign_buffer_transfer_detaches_all_visible_views() {
    assert_realms(
        r#"
        var other = $262.createRealm().global;
        var buffer = other.eval('new ArrayBuffer(4, {maxByteLength: 8})');
        var view = new Uint8Array(buffer);
        view[0] = 17;
        var transferred = buffer.transfer(6);
        buffer.byteLength === 0 && view.length === 0 && transferred.byteLength === 6 &&
            transferred.resizable && new Uint8Array(transferred)[0] === 17
    "#,
    );
}

#[test]
fn foreign_transfers_through_call_apply_and_closures_keep_mirrors_coherent() {
    assert_realms(
        r#"
        var other = $262.createRealm().global, passed = 0;
        for (var invoke of [
            b => b.transfer.call(b, 6),
            b => b.transferToFixedLength.apply(b, [6]),
            b => other.eval('(b => b.transfer(6))')(b)
        ]) {
            var buffer = other.eval('new ArrayBuffer(4, {maxByteLength: 8})');
            var first = new Uint8Array(buffer), second = new DataView(buffer);
            first[0] = 17; second.setUint8(1, 23);
            try { buffer.transfer(-1); } catch (e) { if (e.name !== 'RangeError') throw e; }
            if (first.length !== 4 || second.getUint8(1) !== 23) throw 'failed transfer detached';
            var result = invoke(buffer), detached = false;
            try { second.getUint8(0); } catch (e) { detached = e instanceof TypeError; }
            if (first.length !== 0 || !detached || buffer.byteLength !== 0) throw 'live old view';
            var bytes = new Uint8Array(result);
            if (bytes.length !== 6 || bytes[0] !== 17 || bytes[1] !== 23) throw 'lost bytes';
            passed++;
        }
        passed === 3
    "#,
    );
}

#[test]
fn views_share_foreign_backing_and_track_resize_even_at_maximum_capacity() {
    assert_realms(
        r#"
        var other = $262.createRealm().global;
        var buffer = other.eval('new ArrayBuffer(4, {maxByteLength:4})');
        var first = new Uint8Array(buffer), second = new DataView(buffer), fixed = new Uint8Array(buffer, 0, 4);
        first[0] = 17; second.setUint8(1, 23);
        if (first[1] !== 23 || second.getUint8(0) !== 17) throw 'separate backing stores';
        buffer.resize(2);
        if (first.length !== 2 || second.byteLength !== 2 || fixed.length !== 0) throw 'shrink';
        if (first[0] !== 17 || first[1] !== 23) throw 'lost prefix';
        buffer.resize(4);
        if (first.length !== 4 || second.byteLength !== 4 || fixed.length !== 4 || first[3] !== 0) throw 'grow';
        var local = new ArrayBuffer(4, {maxByteLength:4}); other.local = local;
        var shared = new SharedArrayBuffer(4, {maxByteLength:4}); other.shared = shared;
        other.eval('local.resizable && local.maxByteLength === 4 && shared.growable && shared.maxByteLength === 4')
    "#,
    );
}

#[test]
fn foreign_property_traps_can_reenter_local_objects_and_preserve_thrown_identity() {
    assert_realms(
        r#"
        var other = $262.createRealm().global, sentinel = {}, receiver = {value: 42}, log = [];
        other.receiver = receiver;
        other.callback = function(kind) { log.push(kind); if (kind === 'fail') throw sentinel; return true; };
        var object = other.eval(`new Proxy({get answer() { return this.value; }, set answer(v) { this.value = v; }}, {
            get(t,k,r) { callback('get'); return Reflect.get(t,k,r); },
            set(t,k,v,r) { callback('set'); return Reflect.set(t,k,v,r); },
            getOwnPropertyDescriptor(t,k) { callback('descriptor'); return Reflect.getOwnPropertyDescriptor(t,k); },
            defineProperty(t,k,d) { callback('define'); return Reflect.defineProperty(t,k,d); },
            ownKeys(t) { callback('keys'); return Reflect.ownKeys(t); },
            deleteProperty(t,k) { callback('delete'); return Reflect.deleteProperty(t,k); },
            isExtensible(t) { callback('extensible'); return Reflect.isExtensible(t); },
            setPrototypeOf(t,p) { callback('prototype'); return Reflect.setPrototypeOf(t,p); },
            preventExtensions(t) { callback('prevent'); return Reflect.preventExtensions(t); }
        })`);
        if (Reflect.get(object, 'answer', receiver) !== 42) throw 'receiver';
        if (!Reflect.set(object, 'answer', 7, receiver) || receiver.value !== 7) throw 'setter';
        Object.getOwnPropertyDescriptor(object, 'answer'); Object.defineProperty(object, 'extra', {value:1, configurable:true});
        Reflect.ownKeys(object); Reflect.deleteProperty(object, 'extra'); Object.isExtensible(object);
        Reflect.setPrototypeOf(object, null); Reflect.preventExtensions(object);
        var failing = other.eval('new Proxy({}, {ownKeys() { callback("fail"); }})'), caught;
        try { Reflect.ownKeys(failing); } catch (e) { caught = e; }
        caught === sentinel && log.join() === 'get,set,descriptor,define,keys,delete,extensible,prototype,prevent,fail'
    "#,
    );
}

#[test]
fn data_view_range_errors_follow_value_conversion_and_survive_vm_reuse() {
    assert_realms(
        r#"
        var log = [], view = new DataView(new ArrayBuffer(8)), errors = 0;
        for (var index of [8, 9007199254740991]) {
            try { view.getFloat64(index); } catch (e) { if (e instanceof RangeError) errors++; }
            try { view.setFloat64(index, {valueOf() { log.push(index); return 1; }}); }
            catch (e) { if (e instanceof RangeError) errors++; }
        }
        view.setFloat64(0, 1.5, true);
        errors === 4 && log.length === 2 && view.getFloat64(0, true) === 1.5
    "#,
    );
}

#[test]
fn transported_shadow_realm_retains_wrapped_function_ownership() {
    assert_realms(
        r#"
        var other = $262.createRealm().global, realm = new ShadowRealm();
        other.realm = realm;
        realm.evaluate('globalThis.count = 0');
        var local = realm.evaluate('() => ++count');
        var foreign = other.eval("realm.evaluate('() => ++count')");
        var first = local(), second = foreign();
        other.eval("realm.evaluate('count += 10')");
        first === 1 && second === 2 && local() === 13 && foreign() === 14
    "#,
    );
}

#[test]
fn nested_shadow_wrappers_preserve_primitive_returns_and_opaque_throws() {
    assert_realms(
        r#"
        var realm = new ShadowRealm();
        var make = realm.evaluate('(callback) => () => callback(41)');
        var callback = make(value => value + 1);
        var opaque = false;
        var throwing = make(() => { throw new RangeError('private'); });
        try { throwing(); } catch (e) { opaque = e instanceof TypeError && e.message === ''; }
        callback() === 42 && opaque
    "#,
    );
}

#[test]
fn foreign_constructors_use_the_explicit_new_target_prototype() {
    assert_realms(
        r#"
        var other = $262.createRealm().global;
        function Target() {}
        var marker = Target.prototype, passed = 0;
        var constructors = [
            [other.RegExp, ['a']], [other.Set, []], [other.WeakMap, []],
            [other.WeakSet, []], [other.WeakRef, [{}]],
            [other.FinalizationRegistry, [() => {}]],
            [other.ArrayBuffer, [4]], [other.SharedArrayBuffer, [4]],
            [other.DataView, [new ArrayBuffer(4)]], [other.Uint8Array, [4]],
            [other.TypeError, ['message']]
        ];
        for (var entry of constructors) {
            if (Object.getPrototypeOf(Reflect.construct(entry[0], entry[1], Target)) === marker)
                passed++;
        }
        passed === constructors.length
    "#,
    );
}

#[test]
fn shadow_import_rejects_non_unicode_export_names_and_keeps_later_imports_usable() {
    let mut vm = Vm::default();
    vm.install_test262_done().unwrap();
    vm.set_module_loader_context(
        "shadow/main.mjs",
        HashMap::from([(
            "shadow/mod.mjs".to_string(),
            compile_module(&parse_module("export const value = 42;").unwrap()).unwrap(),
        )]),
    );
    let source = r#"
        var realm = new ShadowRealm();
        realm.importValue('./mod.mjs', '\uD800').then(
            () => $DONE(new Error('invalid export name resolved')),
            error => {
                if (!(error instanceof TypeError)) throw new Error('wrong rejection');
                return realm.importValue('./mod.mjs', 'value');
            }
        ).then(value => {
            if (value !== 42) throw new Error('later import failed');
            $DONE();
        }, $DONE);
    "#;
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap();
    vm.run_promise_jobs().unwrap();
    assert_eq!(vm.take_test262_done(), Some(Ok(())));
}
