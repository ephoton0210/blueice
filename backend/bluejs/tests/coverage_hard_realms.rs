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
fn foreign_native_functions_use_local_binary_receivers_without_losing_slots() {
    assert_realms(
        r#"
        var child = $262.createRealm().global;
        var local = new Uint8Array([7,42]);
        var length = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(child.Uint8Array).prototype, 'length').get.call(local);
        child.Uint8Array.prototype.set.call(local, {length:1,0:13});
        var sliced = child.Uint8Array.prototype.slice.call(local);
        var iterator = child.Uint8Array.prototype.values.call(local);
        var shared = new SharedArrayBuffer(4), ints = new Int32Array(shared);
        child.Atomics.store(ints, 0, 42);
        var old = child.Atomics.add(ints, 0, 1);
        var copied = child.ArrayBuffer.prototype.slice.call(local.buffer, 0);
        length === 2 && sliced[0] === 13 && sliced[1] === 42 && iterator.next().value === 13 &&
            old === 42 && Atomics.load(ints, 0) === 43 && new Uint8Array(copied).join() === '13,42'
    "#,
    );
}

#[test]
fn foreign_typed_array_methods_retain_local_receivers_when_called_as_own_methods() {
    assert_realms(
        r#"
        var child = $262.createRealm().global, local = new Uint8Array([7,42]);
        local.foreignSlice = child.Uint8Array.prototype.slice;
        local.foreignValues = child.Uint8Array.prototype.values;
        var result = local.foreignSlice(), iterator = local.foreignValues();
        result[0] === 7 && result[1] === 42 && iterator.next().value === 7 &&
            iterator.next().value === 42 && iterator.next().done
    "#,
    );
}

#[test]
fn foreign_atomics_arithmetic_preserves_local_storage_and_callee_errors() {
    assert_realms(
        r#"
        var child = $262.createRealm().global, sentinel = {}, checked = 0;
        var operations = [
            ['load', [], 42, 42], ['store', [7], 7, 7],
            ['add', [7], 42, 49], ['sub', [7], 42, 35],
            ['and', [7], 42, 2], ['or', [7], 42, 47],
            ['xor', [7], 42, 45], ['exchange', [7], 42, 7],
            ['compareExchange', [42, 7], 42, 7]
        ];
        for (var kind of [Int32Array, BigInt64Array]) {
            var bigint = kind === BigInt64Array;
            var convert = value => bigint ? BigInt(value) : value;
            var view = new kind(new SharedArrayBuffer(8));
            for (var row of operations) for (var mode of ['direct', 'call', 'apply']) {
                Atomics.store(view, 0, convert(42));
                var args = [view, 0].concat(row[1].map(convert));
                var fn = child.Atomics[row[0]], answer, reads = 0;
                if (mode === 'direct') answer = fn(...args);
                else if (mode === 'call') answer = fn.call(null, ...args);
                else {
                    var list = {get length() {reads++; return args.length;}};
                    for (var i = 0; i < args.length; i++) list[i] = args[i];
                    answer = fn.apply(null, list);
                    if (reads !== 1) throw 'apply repeated the argument-list getter';
                }
                if (answer !== convert(row[2]) || Atomics.load(view, 0) !== convert(row[3]))
                    throw 'foreign Atomics changed a copied buffer';
                checked++;
            }
        }
        for (var invoke of [
            (fn, args) => fn(...args),
            (fn, args) => fn.call(null, ...args),
            (fn, args) => fn.apply(null, args)
        ]) {
            for (var row of [
                [child.Atomics.load, [], child.TypeError],
                [child.Atomics.load, [{}, 0], child.TypeError],
                [child.Atomics.load, [new Int32Array(1), 2], child.RangeError],
                [child.Atomics.store, [new BigInt64Array(1), 0, 1], child.TypeError]
            ]) {
                try {invoke(row[0], row[1]); throw 'accepted invalid atomic access';}
                catch (error) {if (!(error instanceof row[2])) throw error; checked++;}
            }
            var view = new Int32Array(1);
            for (var args of [[view, {valueOf() {throw sentinel;}}],
                [view, 0, {valueOf() {throw sentinel;}}]]) {
                try {invoke(args.length === 2 ? child.Atomics.load : child.Atomics.add, args); throw 'ignored coercion';}
                catch (error) {if (error !== sentinel) throw error; checked++;}
            }
        }
        try {child.Atomics.load.apply(null, {get length() {throw sentinel;}}); throw 'ignored apply getter';}
        catch (error) {if (error !== sentinel) throw error; checked++;}
        var own = new child.Int32Array(new child.SharedArrayBuffer(4));
        child.Atomics.store(own, 0, 42);
        var third = $262.createRealm().global;
        var alien = new third.Int32Array(new third.SharedArrayBuffer(4));
        child.Atomics.store(alien, 0, 42);
        if (child.Atomics.add.apply(null, [alien, 0, 1]) !== 42 || third.Atomics.load(alien, 0) !== 43)
            throw 'third-Realm storage was copied';
        try {child.Atomics.load.call(null, alien, 2); throw 'accepted third-Realm bounds';}
        catch (error) {if (!(error instanceof child.RangeError)) throw error;}
        checked === 73 && child.Atomics.load(own, 0) === 42;
        "#,
    );
}

#[test]
fn foreign_iterator_setters_observe_local_proxy_traps_and_error_realms() {
    assert_realms(
        r#"
        var child = $262.createRealm().global, sentinel = {};
        var third = $262.createRealm().global;
        for (var key of ['constructor', Symbol.toStringTag]) {
            var setter = Object.getOwnPropertyDescriptor(child.Iterator.prototype, key).set;
            for (var receiver of [
                Object.defineProperty({}, key, {value:7, writable:false}),
                new Proxy({[key]:7}, {set() {return false;}}),
                new Proxy({[key]:7}, {set:7}),
                new Proxy(Object.defineProperty({}, key, {value:7, writable:false, configurable:false}),
                    {set() {return true;}}),
                new Proxy({}, {defineProperty() {return false;}})
            ]) {
                try {setter.call(receiver, 42); throw 'accepted cross-Realm refused write';}
                catch (error) {if (!(error instanceof child.TypeError)) throw error;}
            }
            for (var receiver of [
                new Proxy({}, {getOwnPropertyDescriptor() {throw sentinel;}}),
                new Proxy({}, {defineProperty() {throw sentinel;}})
            ]) {
                try {setter.call(receiver, 42); throw 'ignored cross-Realm trap throw';}
                catch (error) {if (error !== sentinel) throw error;}
            }
            var target = {}, receiver = new Proxy(target, {});
            if (setter.call(receiver, 42) !== undefined || target[key] !== 42)
                throw 'cross-Realm property creation failed';
            var alien = third.eval('Object.defineProperty({}, ' +
                (key === 'constructor' ? "'constructor'" : 'Symbol.toStringTag') +
                ', {value:7, writable:false})');
            try {setter.call(alien, 42); throw 'accepted third-Realm readonly write';}
            catch (error) {if (!(error instanceof child.TypeError)) throw error;}
            try {setter.call(Iterator.prototype, 42); throw 'accepted parent home write';}
            catch (error) {if (!(error instanceof TypeError) || error instanceof child.TypeError) throw error;}
        }
        var constructor = 7;
        Object.getOwnPropertyDescriptor(child.Iterator.prototype, 'constructor').set.call(globalThis, 42);
        if (constructor !== 42 || globalThis.constructor !== 42) throw 'global binding desynchronized';
        if (!Reflect.set(globalThis, 'constructor', 13) || constructor !== 13) throw 'Reflect.set desynchronized';
        true;
        "#,
    );
}

#[test]
fn local_iterator_wrapper_methods_reject_foreign_non_wrappers_in_the_callee_realm() {
    assert_realms(
        r#"
        var child = $262.createRealm().global, third = $262.createRealm().global, checked = 0;
        for (var callee of [globalThis, child, third]) {
            var wrapper = callee.Iterator.from({next() {return {done:true};}});
            for (var owner of [globalThis, child, third]) {
                for (var source of ['({})', '({next:0, return:0})', '({next:{}, return:{}})']) {
                    var receiver = owner.eval(source);
                    for (var method of ['next', 'return']) {
                        try {wrapper[method].call(receiver); throw 'accepted non-wrapper';}
                        catch (error) {
                            if (!(error instanceof callee.TypeError)) throw error;
                            if (callee !== owner && error instanceof owner.TypeError) throw 'native error changed Realm';
                            checked++;
                        }
                    }
                }
            }
        }
        checked === 54;
        "#,
    );
}

#[test]
fn required_regexp_writes_preserve_throw_and_error_realms_across_membranes() {
    assert_realms(
        r#"
        var child = $262.createRealm().global, third = $262.createRealm().global, sentinel = {};
        for (var owner of [globalThis, child, third]) {
            var pattern = owner.eval("Object.defineProperty({global:true, unicode:false, flags:'g', exec() {return null;}}, 'lastIndex', {value:0, writable:false})");
            for (var callee of [globalThis, child]) {
                for (var method of [Symbol.match, Symbol.replace]) {
                    try {callee.RegExp.prototype[method].call(pattern, '', ''); throw 'accepted required regexp write';}
                    catch (error) {if (!(error instanceof callee.TypeError)) throw error;}
                }
            }
        }
        var throwing = {global:true, unicode:false, flags:'g', exec() {return null;},
            get lastIndex() {return 0;}, set lastIndex(value) {throw sentinel;}};
        try {child.RegExp.prototype[Symbol.match].call(throwing, ''); throw 'ignored regexp setter throw';}
        catch (error) {if (error !== sentinel) throw error;}
        var successful = {global:true, unicode:false, flags:'g', lastIndex:7, exec() {return null;}};
        child.RegExp.prototype[Symbol.match].call(successful, '');
        successful.lastIndex === 0;
        "#,
    );
}

#[test]
fn accessor_errors_retain_the_function_realm_across_required_regexp_writes() {
    assert_realms(
        r#"
        var child = $262.createRealm().global, third = $262.createRealm().global;
        var checked = 0;
        var callbacks = [
            ['TypeError', "Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'byteLength').get"],
            ['RangeError', '(function () {return new Array(-1);})'],
            ['ReferenceError', '(function () {return missingAccessorBinding;})'],
            ['SyntaxError', "(function () {return eval('(');})"]
        ];
        for (var owner of [globalThis, child, third]) {
            for (var callback of callbacks) {
                for (var access of ['getter', 'setter', 'get trap', 'set trap']) {
                    var read = access === 'getter' || access === 'get trap';
                    var key = read ? 'flags' : 'lastIndex';
                    var source = "(function () {var pattern = {flags:'g', exec() {return null;}};";
                    if (access === 'getter' || access === 'setter') {
                        source += "Object.defineProperty(pattern, '" + key + "', {" +
                            (read ? 'get:' : 'set:') + callback[1] + '}); return pattern;})()';
                    } else {
                        source += 'return new Proxy(pattern, {' + (read ? 'get' : 'set') +
                            ':function (target, key, value) {if (key === "' + key + '") return (' +
                            callback[1] + ').call(this); return Reflect.' +
                            (read ? 'get(target, key)' : 'set(target, key, value)') + ';}});})()';
                    }
                    var pattern = owner.eval(source);
                    for (var callee of [globalThis, child]) {
                        try {callee.RegExp.prototype[Symbol.match].call(pattern, ''); throw 'ignored accessor error';}
                        catch (error) {
                            if (!(error instanceof owner[callback[0]])) throw error;
                            if (owner !== callee && error instanceof callee[callback[0]]) throw 'accessor error changed Realm';
                            checked++;
                        }
                    }
                }
            }
        }
        checked === 96;
        "#,
    );
}

#[test]
fn shadow_evaluation_makes_queued_capability_errors_opaque() {
    assert_realms(
        r#"
        var child = $262.createRealm().global, checked = 0;
        var source = 'class Capability {constructor(executor) {executor(() => {throw 7;}, () => {throw 9;});}} var p = Promise.resolve(42); p.constructor = {[Symbol.species]:Capability}; p.then(value => value); 42';
        for (var realm of [new ShadowRealm(), new child.ShadowRealm()]) {
            try {realm.evaluate(source); throw 'accepted queued error';}
            catch (error) {if (error.name !== 'TypeError' || error === 7 || error === 9) throw error; checked++;}
            if (realm.evaluate('21 + 21') !== 42) throw 'evaluation frame was not restored';
        }
        checked === 2
    "#,
    );
}

#[test]
fn shadow_evaluation_makes_collected_finalization_callback_errors_opaque() {
    assert_realms(
        r#"
        var realm = new ShadowRealm(), checked = 0;
        var source = 'globalThis.registry = new FinalizationRegistry(() => {throw 7;}); registry.register({}, 42); 42';
        try {realm.evaluate(source); throw 'accepted cleanup callback error';}
        catch(error) {if (!(error instanceof TypeError)) throw error; checked++;}
        checked === 1 && realm.evaluate('21 + 21') === 42
    "#,
    );
}

#[test]
fn a_foreign_fallback_prototype_retains_the_intrinsic_after_its_global_is_replaced() {
    assert_realms(
        r#"
        var other = $262.createRealm().global;
        var original = other.Object.prototype;
        other.eval('globalThis.Object = function Replacement() {}; Object.prototype = 7;');
        var constructed = Reflect.construct(Object, [], other.Object);
        Object.getPrototypeOf(constructed) === original && other.eval('21 + 21') === 42;
        "#,
    );
}

#[test]
fn reentrant_evaluation_can_return_a_new_callable_without_losing_either_context() {
    assert_realms(
        r#"
        var realm = new ShadowRealm();
        var outer = realm.evaluate('(function(callback) {var retained = 9; return callback()() + retained;})');
        var callback = function() {return realm.evaluate('(function() {return 33;})');};
        outer(callback) === 42 && realm.evaluate('21 + 21') === 42;
        "#,
    );
}

#[test]
fn shadow_import_rejects_a_namespace_thenable_that_resolves_to_a_primitive() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    vm.install_test262_done().unwrap();
    vm.set_module_loader_context(
        "shadow/main.mjs",
        HashMap::from([
            (
                "shadow/assimilated.mjs".into(),
                compile_module(
                    &parse_module(
                        "export function then(resolve) {resolve(7);} export const value = 42;",
                    )
                    .unwrap(),
                )
                .unwrap(),
            ),
            (
                "shadow/ordinary.mjs".into(),
                compile_module(&parse_module("export const value = 42;").unwrap()).unwrap(),
            ),
        ]),
    );
    let source = r#"
        var realm = new ShadowRealm();
        realm.importValue('./assimilated.mjs', 'value').then(
            () => $DONE(new Error('primitive namespace was accepted')),
            error => {
                if (!(error instanceof TypeError)) throw new Error('wrong rejection');
                return realm.importValue('./ordinary.mjs', 'value');
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

#[test]
fn reverse_membrane_writes_are_visible_before_callbacks_and_after_throws() {
    assert_realms(
        r#"
        var other = $262.createRealm().global, sentinel = {}, log = [];
        var target = {count: 0};
        Object.defineProperty(target, 'value', {
            set(v) { log.push('set:' + v); this.count = v; },
            get() { log.push('get'); return this.count; }, configurable: true
        });
        other.target = target;
        other.check = function() {
            log.push('callback:' + target.count);
            if (target.count !== 7) throw 'delayed write';
        };
        other.sentinel = sentinel;
        var same = false;
        try { other.eval('target.value = 7; check(); target.extra = 9; throw sentinel'); }
        catch (error) { same = error === sentinel; }
        same && target.count === 7 && target.extra === 9 &&
            log.join() === 'set:7,callback:7' && other.eval('target.value') === 7
    "#,
    );
}

#[test]
fn reverse_membrane_forwards_deletion_descriptors_and_nested_calls_immediately() {
    assert_realms(
        r#"
        var other = $262.createRealm().global, object = {old: 1}, calls = 0;
        other.object = object;
        other.inspect = function() {
            calls++;
            return !('old' in object) && Object.getOwnPropertyDescriptor(object, 'new').value === 42;
        };
        var change = other.eval(`() => {
            delete object.old;
            Object.defineProperty(object, 'new', {value: 42, configurable: true});
            return inspect();
        }`);
        change() && calls === 1 && object.new === 42 && !('old' in object)
    "#,
    );
}

#[test]
fn detached_buffers_and_invalid_foreign_views_preserve_their_error_realm() {
    assert_realms(
        r#"
        var other = $262.createRealm().global, detached = new ArrayBuffer(4);
        $262.detachArrayBuffer(detached);
        other.detached = detached;
        var local = new Uint8Array(1), foreign = other.eval('new Uint8Array(2)'), failures = 0;
        for (var offset of [0, 1, 2]) {
            try { local.set(foreign, offset); }
            catch (error) { if (error instanceof RangeError) failures++; }
        }
        var bad = other.eval('new Uint8Array(2)'), callback = {}, same = false;
        try { Uint8Array.prototype.map.call(bad, () => { throw callback; }); }
        catch (error) { same = error === callback; }
        failures === 3 && same && other.eval('detached.byteLength === 0') &&
            local.length === 1 && foreign.length === 2
    "#,
    );
}

#[test]
fn foreign_typed_array_methods_reject_wrong_receivers_and_out_of_bounds_views() {
    assert_realms(
        r#"
        var other = $262.createRealm().global, failures = 0;
        var buffer = other.eval('new ArrayBuffer(4, {maxByteLength: 8})');
        other.buffer = buffer;
        var view = other.eval('new Uint8Array(buffer, 0, 4)');
        buffer.resize(0);
        for (var invoke of [
            () => Uint8Array.prototype.slice.call(view),
            () => Uint8Array.prototype.set.call(view, []),
            () => new Uint8Array(4).set(view),
            () => other.Uint8Array.prototype.slice.call({}),
            () => Atomics.add(other.eval('({})'), 0, 1)
        ]) {
            try { invoke(); }
            catch (error) { if (error.name === 'TypeError') failures++; }
        }
        buffer.resize(4);
        failures === 5 && view.length === 4 && view.slice().length === 4
    "#,
    );
}

#[test]
fn shadow_wrappers_normalize_length_and_name_without_exposing_objects() {
    assert_realms(
        r#"
        var realm = new ShadowRealm(), checked = 0;
        for (var entry of [
            ['Infinity', Infinity], ['-Infinity', 0], ['NaN', 0], ['-3.5', 0],
            ['3.9', 3], ['"7"', 0], ['undefined', 0]
        ]) {
            var wrapped = realm.evaluate(`(() => {
                function f() { return 42; }
                Object.defineProperty(f, 'length', {value: ${entry[0]}});
                Object.defineProperty(f, 'name', {value: 7});
                return f;
            })()`);
            var length = Object.getOwnPropertyDescriptor(wrapped, 'length');
            var name = Object.getOwnPropertyDescriptor(wrapped, 'name');
            if (wrapped() === 42 && length.value === entry[1] && !length.writable &&
                !length.enumerable && length.configurable && name.value === '' &&
                !name.writable && !name.enumerable && name.configurable) checked++;
        }
        checked === 7
    "#,
    );
}

#[test]
fn shadow_wrappers_read_proxy_metadata_in_order_and_hide_abrupt_completions() {
    assert_realms(
        r#"
        var realm = new ShadowRealm(), failures = 0;
        for (var trap of ['getOwnPropertyDescriptor', 'get']) {
            try {
                realm.evaluate(`new Proxy(function(){}, {
                    ${trap}() { throw new Error('private metadata'); }
                })`);
            } catch (error) { if (error instanceof TypeError && !error.message.includes('private metadata')) failures++; }
        }
        var wrapped = realm.evaluate(`new Proxy(function(){return 42}, {
            getOwnPropertyDescriptor(t,k) { return k === 'length' ? undefined : Reflect.getOwnPropertyDescriptor(t,k); },
            get(t,k,r) { if (k === 'length') throw 'length must not be read'; return Reflect.get(t,k,r); }
        })`);
        failures === 2 && wrapped.length === 0 && wrapped() === 42
    "#,
    );
}

#[test]
fn shadow_reentrant_callbacks_return_to_the_suspended_ancestor_realm() {
    assert_realms(
        r#"
        var realm = new ShadowRealm(), log = [];
        var invoke = realm.evaluate('(callback) => callback(7) + 1');
        var callback = function(value) {
            log.push(value);
            return realm.evaluate('21 + 21');
        };
        var opaque = false;
        try { invoke(() => ({})); }
        catch (error) { opaque = error instanceof TypeError; }
        invoke(callback) === 43 && log.join() === '7' && opaque && realm.evaluate('1 + 1') === 2
    "#,
    );
}

#[test]
fn shadow_reentrant_evaluation_preserves_lexical_cells_arguments_and_abrupt_cleanup() {
    assert_realms(
        r#"
        var realm = new ShadowRealm();
        var invoke = realm.evaluate(`let captured = 7; (callback, input) => {
            let local = {value: input};
            var answer = callback();
            return captured + local.value + answer;
        }`);
        var success = invoke(() => realm.evaluate('let captured = 99; 21 + 21'), 1) === 50;
        var failures = 0;
        for (var source of ['throw 7', 'let captured = 99; throw 8']) {
            var answer = invoke(() => {
                try {realm.evaluate(source);} catch (error) {if (error instanceof TypeError) failures++;}
                return 42;
            }, 1);
            if (answer !== 50) throw 'lost outer lexical cells';
        }
        success && failures === 2
    "#,
    );
}

#[test]
fn reverse_descriptors_and_delete_preserve_accessors_proxy_errors_and_constraints() {
    assert_realms(
        r#"
        var other = $262.createRealm().global, sentinel = {}, reads = 0, written;
        var object = {}; Object.defineProperty(object, 'fixed', {value: 7});
        other.object = object;
        var install = other.eval(`() => {
            Object.defineProperty(object, 'value', {get() {return 42;}, set(value) {globalThis.saved = value;}, configurable:true});
            var fixed = Reflect.deleteProperty(object, 'fixed');
            var absent = Reflect.deleteProperty(object, 'absent');
            return !fixed && absent;
        }`);
        var installed = install(); object.value = 9;
        var saved = other.saved;
        var errorTarget = new Proxy({}, {
            defineProperty() {reads++; throw sentinel;},
            deleteProperty() {reads++; throw sentinel;}
        });
        other.errorTarget = errorTarget;
        var operations = other.eval(`[
            () => Reflect.defineProperty(errorTarget, 'value', {value:7}),
            () => Reflect.deleteProperty(errorTarget, 'value')
        ]`);
        var caught = 0;
        for (var operation of operations) {try {operation();} catch (error) {if (error === sentinel) caught++;}}
        installed && object.value === 42 && saved === 9 && reads === 2 && caught === 2
    "#,
    );
}

#[test]
fn shadow_reentrant_scripts_preserve_parameter_eval_and_outer_job_weak_roots() {
    assert_realms(
        r#"
        var realm = new ShadowRealm();
        var parameter = realm.evaluate(`callback => {
            function* g(value = eval('var local = 7; callback()')) {yield local + value;}
            return g().next().value;
        }`);
        var weak = realm.evaluate(`callback => {
            let target = {}, reference = new WeakRef(target); target = null;
            reference.deref(); callback();
            return reference.deref() !== undefined;
        }`);
        parameter(() => realm.evaluate('21 + 21')) === 49 &&
            weak(() => realm.evaluate('21 + 21'))
    "#,
    );
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
            [other.Function, ['return 42']],
            [other.eval('(async function() {}).constructor'), ['return 42']],
            [other.eval('(function*() {}).constructor'), ['yield 42']],
            [other.eval('(async function*() {}).constructor'), ['yield 42']],
            [other.Date, [0]], [other.Promise, [() => {}]], [other.Map, []],
            [other.RegExp, ['a']], [other.Set, []], [other.WeakMap, []],
            [other.WeakSet, []], [other.WeakRef, [{}]],
            [other.FinalizationRegistry, [() => {}]],
            [other.ArrayBuffer, [4]], [other.SharedArrayBuffer, [4]],
            [other.DataView, [new ArrayBuffer(4)]], [other.Uint8Array, [4]],
            [other.Int8Array, [4]], [other.Int16Array, [4]], [other.Uint16Array, [4]],
            [other.Int32Array, [4]], [other.Uint32Array, [4]], [other.Float32Array, [4]],
            [other.Float64Array, [4]], [other.BigInt64Array, [4]], [other.BigUint64Array, [4]],
            [other.Uint8ClampedArray, [4]], [other.TypeError, ['message']],
            [other.Float16Array, [4]], [other.String, ['x']],
            [other.DisposableStack, []], [other.AsyncDisposableStack, []],
            [other.ShadowRealm, []], [other.Intl.Collator, ['en']],
            [other.Intl.Locale, ['en']], [other.Intl.NumberFormat, ['en']],
            [other.Intl.DateTimeFormat, ['en', {timeZone:'UTC'}]],
            [other.Intl.DisplayNames, ['en', {type:'language'}]],
            [other.Intl.DurationFormat, ['en']], [other.Intl.ListFormat, ['en']],
            [other.Intl.PluralRules, ['en']], [other.Intl.RelativeTimeFormat, ['en']],
            [other.Intl.Segmenter, ['en']],
            [other.Temporal.Duration, []], [other.Temporal.Instant, [0n]],
            [other.Temporal.PlainDate, [2000,1,1]],
            [other.Temporal.PlainDateTime, [2000,1,1]],
            [other.Temporal.PlainMonthDay, [1,1]], [other.Temporal.PlainTime, []],
            [other.Temporal.PlainYearMonth, [2000,1]],
            [other.Temporal.ZonedDateTime, [0n,'UTC']]
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
fn local_constructors_select_the_foreign_new_target_realms_intrinsic_fallback() {
    assert_realms(
        r#"
        var other = $262.createRealm().global;
        var Target = other.eval('(function Target() {})');
        Target.prototype = 7;
        var constructors = [
            [Object, [], other.Object], [Array, [2], other.Array],
            [Boolean, [true], other.Boolean], [Number, [7], other.Number],
            [String, ['x'], other.String], [Function, ['return 42'], other.Function],
            [Date, [0], other.Date], [Promise, [() => {}], other.Promise],
            [Map, [], other.Map], [Set, [], other.Set],
            [WeakMap, [], other.WeakMap], [WeakSet, [], other.WeakSet],
            [WeakRef, [{}], other.WeakRef],
            [FinalizationRegistry, [() => {}], other.FinalizationRegistry],
            [DisposableStack, [], other.DisposableStack],
            [AsyncDisposableStack, [], other.AsyncDisposableStack],
            [ShadowRealm, [], other.ShadowRealm], [RegExp, ['a'], other.RegExp],
            [ArrayBuffer, [4], other.ArrayBuffer], [SharedArrayBuffer, [4], other.SharedArrayBuffer],
            [DataView, [new ArrayBuffer(4)], other.DataView],
            [Int8Array, [1], other.Int8Array], [Uint8Array, [1], other.Uint8Array],
            [Uint8ClampedArray, [1], other.Uint8ClampedArray],
            [Int16Array, [1], other.Int16Array], [Uint16Array, [1], other.Uint16Array],
            [Int32Array, [1], other.Int32Array], [Uint32Array, [1], other.Uint32Array],
            [Float16Array, [1], other.Float16Array], [Float32Array, [1], other.Float32Array],
            [Float64Array, [1], other.Float64Array],
            [BigInt64Array, [1], other.BigInt64Array], [BigUint64Array, [1], other.BigUint64Array],
            [Intl.Collator, ['en'], other.Intl.Collator], [Intl.Locale, ['en'], other.Intl.Locale],
            [Intl.NumberFormat, ['en'], other.Intl.NumberFormat],
            [Intl.DateTimeFormat, ['en', {timeZone:'UTC'}], other.Intl.DateTimeFormat],
            [Intl.DisplayNames, ['en', {type:'language'}], other.Intl.DisplayNames],
            [Intl.DurationFormat, ['en'], other.Intl.DurationFormat],
            [Intl.ListFormat, ['en'], other.Intl.ListFormat], [Intl.PluralRules, ['en'], other.Intl.PluralRules],
            [Intl.RelativeTimeFormat, ['en'], other.Intl.RelativeTimeFormat],
            [Intl.Segmenter, ['en'], other.Intl.Segmenter],
            [Temporal.Duration, [], other.Temporal.Duration], [Temporal.Instant, [0n], other.Temporal.Instant],
            [Temporal.PlainDate, [2000,1,1], other.Temporal.PlainDate],
            [Temporal.PlainDateTime, [2000,1,1], other.Temporal.PlainDateTime],
            [Temporal.PlainMonthDay, [1,1], other.Temporal.PlainMonthDay],
            [Temporal.PlainTime, [], other.Temporal.PlainTime],
            [Temporal.PlainYearMonth, [2000,1], other.Temporal.PlainYearMonth],
            [Temporal.ZonedDateTime, [0n,'UTC'], other.Temporal.ZonedDateTime]
        ];
        var checked = 0;
        for (var entry of constructors) {
            var result = Reflect.construct(entry[0], entry[1], Target);
            if (Object.getPrototypeOf(result) !== entry[2].prototype) throw 'wrong intrinsic fallback';
            checked++;
        }
        checked === constructors.length
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
