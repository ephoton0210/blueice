// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Proxy receiver semantics, RegExp compile ordering and host API limits.

use blueice_bluejs::{compile, parse, HostValue, RuntimeError, Value, Vm, VmConfig};

#[test]
fn nested_proxy_invariants_propagate_the_targets_internal_method_errors() {
    let source = r#"
        var sentinel = {}, passed = 0;
        for (var run of [
            () => {var target = new Proxy({value:7}, {isExtensible() {throw sentinel;}}); 'value' in new Proxy(target, {has() {return false;}});},
            () => {var target = new Proxy({value:7}, {isExtensible() {throw sentinel;}}); delete new Proxy(target, {deleteProperty() {return true;}}).value;},
            () => {var target = new Proxy({}, {ownKeys() {throw sentinel;}}); Reflect.ownKeys(new Proxy(target, {ownKeys() {return [];}}));},
            () => {var target = new Proxy({}, {isExtensible() {throw sentinel;}}); Object.getOwnPropertyDescriptor(new Proxy(target, {getOwnPropertyDescriptor() {return undefined;}}), 'value');},
            () => {var target = new Proxy({}, {isExtensible() {throw sentinel;}}); Reflect.defineProperty(new Proxy(target, {defineProperty() {return true;}}), 'value', {value:7});},
            () => {var target = new Proxy({}, {isExtensible() {throw sentinel;}}); Reflect.isExtensible(new Proxy(target, {isExtensible() {return true;}}));},
            () => {var target = new Proxy({}, {isExtensible() {throw sentinel;}}); Reflect.preventExtensions(new Proxy(target, {preventExtensions() {return true;}}));},
            () => {var target = new Proxy({}, {isExtensible() {throw sentinel;}}); Reflect.ownKeys(new Proxy(target, {ownKeys() {return [];}}));},
        ]) {
            try {run(); throw 'accepted target failure';} catch(error) {if(error !== sentinel) throw error;}
            passed++;
        }
        passed === 8
    "#;
    let code = compile(&parse(source).unwrap()).unwrap();
    for capacity in [1, VmConfig::default().heap.nursery_capacity] {
        let mut config = VmConfig::default();
        config.heap.nursery_capacity = capacity;
        assert_eq!(
            Vm::new(config).unwrap().execute_script(&code),
            Ok(Value::Bool(true))
        );
    }
}

#[test]
fn lazy_intrinsics_keep_the_original_function_prototype_after_string_prototype_changes() {
    for replacement in ["null", "{}"] {
        for operation in [
            "Object.getPrototypeOf(RegExp) === expected && /a/.test('a')",
            "Object.getPrototypeOf(Error) === expected && new Error('message').message === 'message'",
            "Object.getPrototypeOf(Object.getPrototypeOf(TypeError)) === expected",
            "Object.getPrototypeOf(Function) === expected && Function('return 42')() === 42",
            "Object.getPrototypeOf(ArrayBuffer) === expected && new ArrayBuffer(8).byteLength === 8",
            "Object.getPrototypeOf(Object.getPrototypeOf(Uint8Array)) === expected && new Uint8Array(2).length === 2",
            "Object.getPrototypeOf(Temporal.PlainDate) === expected && Temporal.PlainDate.from('2000-01-01').year === 2000",
            "Object.getPrototypeOf(Intl.Collator) === expected && new Intl.Collator('en').compare('a','b') < 0",
            "Object.getPrototypeOf(new Intl.Collator('en').compare) === expected",
            "Object.getPrototypeOf(new Intl.NumberFormat('en').format) === expected",
            "Object.getPrototypeOf(new Intl.DateTimeFormat('en').format) === expected",
            "Object.getPrototypeOf(new Intl.Segmenter('en').segment('abc')[Symbol.iterator]().next) === expected",
            "Object.getPrototypeOf('abc'[Symbol.iterator]().next) === expected",
            "Object.getPrototypeOf([7][Symbol.iterator]().next) === expected",
            "Object.getPrototypeOf(/a/g[Symbol.matchAll]('aa').next) === expected",
            "Object.getPrototypeOf(Iterator.prototype.map) === expected && Iterator.from([7]).map(x => x + 1).next().value === 8",
            "Object.getPrototypeOf(Object.getOwnPropertyDescriptor((function() {'use strict'; return arguments;})(), 'callee').get) === expected",
            "Object.getPrototypeOf(Object.getOwnPropertyDescriptor((function named() {}), 'caller').get) === expected",
            "Object.getPrototypeOf((function*() {})().next) === expected && (function*() {yield 7;})().next().value === 7",
        ] {
            for nursery_capacity in [1, VmConfig::default().heap.nursery_capacity] {
                let mut config = VmConfig::default();
                config.heap.nursery_capacity = nursery_capacity;
                let mut vm = Vm::new(config).unwrap();
                let source = format!("var expected = Object.getPrototypeOf(String); Object.setPrototypeOf(String, {replacement}); {operation}");
                assert_eq!(vm.execute_script(&compile(&parse(&source).unwrap()).unwrap()), Ok(Value::Bool(true)), "{source}");
                assert_eq!(vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()), Ok(Value::Number(42.0)));
            }
        }
    }
    let mut vm = Vm::default();
    vm.execute_script(&compile(&parse("Object.setPrototypeOf(String, null)").unwrap()).unwrap())
        .unwrap();
    vm.install_test262_harness().unwrap();
    vm.install_test262_is_html_dda().unwrap();
    assert_eq!(
        vm.execute_script(&compile(&parse("$262.IsHTMLDDA == null").unwrap()).unwrap()),
        Ok(Value::Bool(true))
    );
}

fn assert_script(source: &str) {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true),
        "{source}"
    );
}

#[test]
fn object_algorithms_preserve_throws_from_each_internal_method_boundary() {
    assert_script(
        r#"
        var sentinel = {}, checked = 0;
        var key = {[Symbol.toPrimitive]() {throw sentinel;}};
        var keys = () => new Proxy({}, {ownKeys() {throw sentinel;}});
        var descriptors = () => new Proxy({answer:42}, {
            ownKeys() {return ['answer'];}, getOwnPropertyDescriptor() {throw sentinel;}
        });
        var values = () => ({get answer() {throw sentinel;}});
        for (var run of [
            () => Object.hasOwn({}, key),
            () => Object.hasOwn(new Proxy({}, {getOwnPropertyDescriptor() {throw sentinel;}}), 'answer'),
            () => Object.getOwnPropertyDescriptors(keys()),
            () => Object.defineProperties({}, keys()),
            () => Object.defineProperties({}, descriptors()),
            () => Object.defineProperties({}, values()),
            () => Object.create(null, keys()),
            () => Object.create(null, descriptors()),
            () => Object.create(null, values()),
            () => Object.defineProperty({}, 'answer', new Proxy({}, {has() {throw sentinel;}})),
            () => Object.seal(keys()),
            () => Object.freeze(descriptors()),
            () => Object.freeze(new Proxy({answer:42}, {defineProperty() {throw sentinel;}})),
            () => Object.isSealed(new Proxy({}, {isExtensible() {throw sentinel;}})),
            () => Object.isSealed(new Proxy(Object.preventExtensions({}), {ownKeys() {throw sentinel;}})),
            () => Object.isFrozen(new Proxy(Object.preventExtensions({answer:42}), {getOwnPropertyDescriptor() {throw sentinel;}})),
            () => Error.prototype.toString.call({name:key}),
            () => String.prototype.match.call(key, /a/),
            () => String.prototype.search.call(key, /a/),
        ]) {
            try {run(); throw 'accepted abrupt boundary';}
            catch(error) {if (error !== sentinel) throw error; checked++;}
        }
        var absent = new Proxy(Object.preventExtensions({answer:42}), {
            getOwnPropertyDescriptor(target, key) {delete target[key]; return undefined;}
        });
        Object.seal(absent);
        checked === 19 && Object.isSealed(absent)
    "#,
    );
}

#[test]
fn object_proxy_traps_preserve_abrupt_metadata_and_vanishing_properties() {
    assert_script(
        r#"
        var sentinel = {}, checked = 0;
        var operations = [
            proxy => proxy.value,
            proxy => 'value' in proxy,
            proxy => Reflect.set(proxy, 'value', 42),
            proxy => Reflect.deleteProperty(proxy, 'value'),
            proxy => Reflect.ownKeys(proxy),
            proxy => Reflect.getOwnPropertyDescriptor(proxy, 'value'),
            proxy => Reflect.defineProperty(proxy, 'value', {value:42}),
            proxy => Reflect.isExtensible(proxy),
            proxy => Reflect.getPrototypeOf(proxy),
            proxy => Reflect.setPrototypeOf(proxy, {}),
            proxy => Reflect.preventExtensions(proxy),
            proxy => proxy(),
            proxy => new proxy(),
        ];
        for (var operation of operations) {
            var handler = new Proxy({}, {get() {throw sentinel;}});
            var proxy = new Proxy(function() {}, handler);
            try {operation(proxy); throw 'accepted trap getter';}
            catch (error) {if (error !== sentinel) throw error; checked++;}
        }
        for (var keys of [
            {get length() {throw sentinel;}},
            {length:{valueOf() {throw sentinel;}}},
            {length:1,get 0() {throw sentinel;}},
        ]) {
            try {Reflect.ownKeys(new Proxy({}, {ownKeys() {return keys;}})); throw 'accepted key list';}
            catch (error) {if (error !== sentinel) throw error; checked++;}
        }
        for (var freeze of [false, true]) {
            var target = {value:7}; Object.preventExtensions(target);
            var proxy = new Proxy(target, {getOwnPropertyDescriptor(object, key) {delete object[key]; return undefined;}});
            if (freeze) Object.freeze(proxy);
            else if (!Object.isFrozen(proxy)) throw 'missing property was frozen';
            if (Reflect.ownKeys(target).length !== 0) throw 'property remained';
            checked++;
        }
        checked === 18
    "#,
    );
}

#[test]
fn binary_conversion_and_foreign_construction_preserve_user_errors() {
    assert_script(
        r#"
        var child = $262.createRealm().global, sentinel = {}, checked = 0;
        for (var run of [
            () => Uint8Array.from.call(child.eval('(function() {throw 7;})'), {length:1,0:42}),
            () => Uint8Array.of.call(child.eval('(function() {throw 7;})'), 42),
        ]) {
            try {run(); throw 'accepted foreign failure';} catch (error) {if (error !== 7) throw error;}
            checked++;
        }
        for (var run of [
            () => Uint8Array.from({length:1,0:42}, () => {throw sentinel;}),
            () => Uint8Array.from([42], () => {throw sentinel;}),
            () => Uint8Array.from({length:1,0:{valueOf() {throw sentinel;}}}),
            () => new Uint8Array(1).set({length:1,0:{valueOf() {throw sentinel;}}}),
        ]) {
            try {run(); throw 'accepted coercion';} catch (error) {if (error !== sentinel) throw error;}
            checked++;
        }
        var target = new Uint8Array(1), buffer = target.buffer;
        target.set({length:1,get 0() {$262.detachArrayBuffer(buffer); return 42;}});
        if (!buffer.detached) throw 'detachment lost';
        for (var run of [
            () => Uint8Array.from({length:1,0:1n}),
            () => new Uint8Array(1).set(new child.BigInt64Array([1n])),
            () => new BigInt64Array(1).set(new child.Uint8Array([1])),
        ]) {
            try {run(); throw 'accepted content type';} catch (error) {if (!(error instanceof TypeError)) throw error;}
            checked++;
        }
        checked === 9
    "#,
    );
}

#[test]
fn regexp_search_match_all_and_legacy_statics_preserve_abrupt_getters() {
    assert_script(
        r#"
        var sentinel = {}, reads = 0, checked = 0;
        var searcher = {get lastIndex() {if (++reads === 2) throw sentinel; return 0;}, exec() {return null;}};
        try {RegExp.prototype[Symbol.search].call(searcher, 'abc'); throw 'accepted second getter';}
        catch (error) {if (error !== sentinel || reads !== 2) throw error; checked++;}
        for (var receiver of [
            {flags:'g',constructor:{[Symbol.species]:RegExp},get lastIndex() {throw sentinel;}},
            {get source() {throw sentinel;}},
        ]) {
            try {
                if ('flags' in receiver) RegExp.prototype[Symbol.matchAll].call(receiver, 'abc');
                else RegExp.prototype.toString.call(receiver);
                throw 'accepted getter';
            } catch (error) {if (error !== sentinel) throw error; checked++;}
        }
        function Species() {return Object.defineProperty({}, 'lastIndex', {value:0});}
        try {RegExp.prototype[Symbol.matchAll].call({flags:'g',lastIndex:0,constructor:{[Symbol.species]:Species}}, 'abc'); throw 'accepted matcher';}
        catch (error) {if (!(error instanceof TypeError)) throw error; checked++;}
        try {RegExp.input = Symbol(); throw 'accepted symbol';}
        catch (error) {if (!(error instanceof TypeError)) throw error; checked++;}
        checked === 5 && new RegExp(']').source === ']' && new RegExp('[a]', 'v').source === '[a]'
    "#,
    );
}

#[test]
fn foreign_buffer_species_validate_ordinary_and_shared_brands_before_copying() {
    for nursery_capacity in [1, VmConfig::default().heap.nursery_capacity] {
        let mut config = VmConfig::default();
        config.heap.nursery_capacity = nursery_capacity;
        let mut vm = Vm::new(config).unwrap();
        vm.install_test262_harness().unwrap();
        let source = r#"
            var other = $262.createRealm().global, checked = 0;
            for (var shared of [false, true]) {
                var Kind = shared ? SharedArrayBuffer : ArrayBuffer;
                var Wrong = shared ? other.ArrayBuffer : other.SharedArrayBuffer;
                var Right = shared ? other.SharedArrayBuffer : other.ArrayBuffer;
                var buffer = new Kind(4);
                new Uint8Array(buffer).set([7, 42, 13, 19]);
                buffer.constructor = {[Symbol.species]: Wrong};
                try {buffer.slice(1, 3); throw 'accepted wrong species';}
                catch (error) {if (!(error instanceof TypeError)) throw error;}
                if (buffer.byteLength !== 4) throw 'source changed';
                buffer.constructor = {[Symbol.species]: Right};
                var result = buffer.slice(1, 3);
                if (new Uint8Array(result).join() !== '42,13') throw 'wrong bytes';
                checked++;
            }
            checked === 2
        "#;
        assert_eq!(
            vm.execute_script(&compile(&parse(source).unwrap()).unwrap()),
            Ok(Value::Bool(true)),
            "nursery_capacity={nursery_capacity}"
        );
        assert_eq!(
            vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
            Ok(Value::Number(42.0))
        );
    }
}

#[test]
fn date_time_options_preserve_styles_widths_and_user_failures() {
    assert_script(
        r#"
        var checked = 0;
        for (var style of ['full', 'long', 'medium', 'short']) {
            for (var name of ['dateStyle', 'timeStyle']) {
                var options = {timeZone:'UTC', formatMatcher:'best fit'};
                options[name] = style;
                var formatter = new Intl.DateTimeFormat('en', options);
                if (formatter.resolvedOptions()[name] !== style) throw 'style lost';
                if (typeof formatter.format(0) !== 'string') throw 'format failed';
                checked++;
            }
        }
        for (var width of ['short', 'long', 'narrow']) {
            var formatter = new Intl.DateTimeFormat('en', {timeZone:'UTC', dayPeriod:width, hour:'numeric', hour12:true});
            if (formatter.resolvedOptions().dayPeriod !== width) throw 'day period lost';
            checked++;
        }
        var sentinel = {};
        for (var run of [
            () => new Intl.DateTimeFormat('en', {fractionalSecondDigits:{valueOf() {throw sentinel;}}}),
            () => new Date(0).toLocaleString('en', {get year() {throw sentinel;}}),
            () => Intl.DateTimeFormat.supportedLocalesOf({get length() {throw sentinel;}}),
            () => Intl.DateTimeFormat.supportedLocalesOf('en', {get localeMatcher() {throw sentinel;}}),
            () => new Intl.DateTimeFormat('en').format({valueOf() {throw sentinel;}}),
            () => new Intl.DateTimeFormat('en').formatRange({valueOf() {throw sentinel;}}, 0),
            () => new Intl.DateTimeFormat('en').formatRange(0, {valueOf() {throw sentinel;}})
        ]) {
            var error; try {run();} catch (e) {error = e;}
            if (error !== sentinel) throw 'coercion failure changed';
            checked++;
        }
        for (var run of [
            () => new Intl.DateTimeFormat('en', null),
            () => Intl.DateTimeFormat.supportedLocalesOf('en', null),
            () => new Intl.DateTimeFormat('en').formatToParts(new Temporal.Duration(1)),
            () => new Intl.DateTimeFormat('en').formatRange(new Temporal.Duration(1), new Temporal.Duration(2)),
            () => new Intl.DateTimeFormat('en').formatRange(Temporal.ZonedDateTime.from('2000-01-01T00:00[UTC]'), Temporal.ZonedDateTime.from('2000-01-02T00:00[UTC]')),
            () => new Intl.DateTimeFormat('en', {dateStyle:'full'}).formatRange(new Temporal.PlainTime(1), new Temporal.PlainTime(2)),
            () => new Intl.DateTimeFormat('en', {timeStyle:'long'}).formatRange(new Temporal.PlainDate(2000,1,1), new Temporal.PlainDate(2000,1,2))
        ]) {
            var error; try {run();} catch (e) {error = e;}
            if (!(error instanceof TypeError)) throw 'wrong option or value error';
            checked++;
        }
        var error; try {new Intl.DateTimeFormat('en', {numberingSystem:'12'});} catch (e) {error=e;}
        checked === 25 && error instanceof RangeError && typeof new Intl.DateTimeFormat('en').format() === 'string'
    "#,
    );
}

#[test]
fn legacy_date_time_receivers_observe_define_and_fallback_traps() {
    assert_script(
        r#"
        var sentinel = {}, calls = 0, prototype = Intl.DateTimeFormat.prototype;
        for (var throws of [false, true]) {
            var receiver = new Proxy(Object.create(prototype), {defineProperty() {calls++; if (throws) throw sentinel; return false;}});
            var error; try {Intl.DateTimeFormat.call(receiver, 'en');} catch (e) {error = e;}
            if (throws ? error !== sentinel : !(error instanceof TypeError)) throw 'define trap error';
        }
        var receiver = Object.create(prototype);
        if (Intl.DateTimeFormat.call(receiver, 'en', {timeZone:'UTC'}) !== receiver) throw 'legacy receiver';
        var fallback = Object.getOwnPropertySymbols(receiver)[0];
        // A Proxy's symbol Get can throw while unwrapping. Its target must
        // not already own the non-configurable hidden property.
        var wrapped = new Proxy(Object.create(prototype), {get(target,key) {if (key === fallback) throw sentinel; return Reflect.get(target,key);}});
        var error; try {prototype.resolvedOptions.call(wrapped);} catch (e) {error=e;}
        if (error !== sentinel) throw 'fallback Get changed';
        for (var value of [undefined, 7, {}]) {
            var wrong = Object.create(prototype); Object.defineProperty(wrong, fallback, {value});
            var error; try {prototype.resolvedOptions.call(wrong);} catch (e) {error=e;}
            if (!(error instanceof TypeError)) throw 'fallback brand';
        }
        calls === 2 && receiver.resolvedOptions().timeZone === 'UTC'
    "#,
    );
}

#[test]
fn all_atomic_operations_preserve_element_width_in_shared_and_plain_buffers() {
    assert_script(
        r#"
        var checked = 0;
        for (var shared of [false, true]) {
            for (var Kind of [Int8Array, Uint8Array, Int16Array, Uint16Array, Int32Array, Uint32Array, BigInt64Array, BigUint64Array]) {
                var buffer = shared ? new SharedArrayBuffer(Kind.BYTES_PER_ELEMENT) : new ArrayBuffer(Kind.BYTES_PER_ELEMENT);
                var view = new Kind(buffer), big = Kind === BigInt64Array || Kind === BigUint64Array;
                var convert = value => big ? BigInt(value) : value;
                for (var entry of [['add',22], ['and',8], ['or',14], ['sub',2], ['xor',6]]) {
                    view[0] = convert(12);
                    if (Atomics[entry[0]](view, 0, convert(10)) !== convert(12) || view[0] !== convert(entry[1])) throw 'arithmetic';
                }
                if (Atomics.store(view, 0, convert(7)) !== convert(7) || Atomics.load(view, 0) !== convert(7)) throw 'store/load';
                if (Atomics.exchange(view, 0, convert(9)) !== convert(7)) throw 'exchange';
                if (Atomics.compareExchange(view, 0, convert(8), convert(3)) !== convert(9) || view[0] !== convert(9)) throw 'failed comparison';
                if (Atomics.compareExchange(view, 0, convert(9), convert(3)) !== convert(9) || view[0] !== convert(3)) throw 'successful comparison';
                checked++;
            }
        }
        checked === 16
    "#,
    );
}

#[test]
fn atomic_index_and_operand_coercions_revalidate_shrinking_and_detached_buffers() {
    assert_script(
        r#"
        var checked = 0;
        for (var operation of ['load', 'store', 'add', 'and', 'or', 'sub', 'xor', 'exchange', 'compareExchange']) {
            for (var atIndex of [true, false]) {
                if (operation === 'load' && !atIndex) continue;
                var buffer = new ArrayBuffer(8, {maxByteLength: 16}), view = new Int32Array(buffer), log = [];
                var bad = {valueOf() {log.push('coerce'); buffer.resize(0); return 0;}};
                var args = [view, atIndex ? bad : 0, atIndex ? 1 : bad, 1], rejected = false;
                try {Atomics[operation](...args);} catch (error) {rejected = error instanceof RangeError || error instanceof TypeError;}
                if (!rejected || log.join() !== 'coerce' || buffer.byteLength !== 0) throw 'revalidation';
                checked++;
            }
        }
        checked === 17
    "#,
    );
}

#[test]
fn descriptor_conversion_and_proxy_failures_preserve_order_and_sentinel_identity() {
    assert_script(
        r#"
        var sentinel = {}, checked = 0;
        for (var trap of ['get', 'set', 'has', 'deleteProperty', 'ownKeys', 'getOwnPropertyDescriptor', 'defineProperty', 'getPrototypeOf', 'setPrototypeOf', 'isExtensible', 'preventExtensions']) {
            var handler = {}; handler[trap] = function() {throw sentinel;};
            var proxy = new Proxy({}, handler), error;
            try {
                if (trap === 'get') Reflect.get(proxy, 'value');
                else if (trap === 'set') Reflect.set(proxy, 'value', 1);
                else if (trap === 'has') Reflect.has(proxy, 'value');
                else if (trap === 'deleteProperty') Reflect.deleteProperty(proxy, 'value');
                else if (trap === 'ownKeys') Reflect.ownKeys(proxy);
                else if (trap === 'getOwnPropertyDescriptor') Object.getOwnPropertyDescriptor(proxy, 'value');
                else if (trap === 'defineProperty') Reflect.defineProperty(proxy, 'value', {value: 1});
                else if (trap === 'getPrototypeOf') Reflect.getPrototypeOf(proxy);
                else if (trap === 'setPrototypeOf') Reflect.setPrototypeOf(proxy, null);
                else if (trap === 'isExtensible') Reflect.isExtensible(proxy);
                else Reflect.preventExtensions(proxy);
            } catch (e) {error = e;}
            if (error !== sentinel) throw 'trap identity';
            checked++;
        }
        var log = [], descriptor = new Proxy({}, {has(t,k) {log.push('has:' + k); return true;}, get(t,k) {log.push('get:' + k); if (k === 'get') throw sentinel; return k === 'value' ? 7 : false;}});
        var same = false;
        try {Object.defineProperty({}, 'x', descriptor);} catch (error) {same = error === sentinel;}
        checked === 11 && same && log.join() === 'has:enumerable,get:enumerable,has:configurable,get:configurable,has:value,get:value,has:writable,get:writable,has:get,get:get'
    "#,
    );
}

#[test]
fn regexp_compile_rejects_foreign_patterns_and_resets_nonwritable_last_index_in_order() {
    assert_script(
        r#"
        var other = $262.createRealm().global, receiver = /old/g, calls = 0, error;
        var foreign = other.eval('/new/i');
        try {receiver.compile(foreign, {toString() {calls++; return 'g';}});} catch (e) {error = e;}
        if (!(error instanceof TypeError) || calls !== 0) throw 'flags order';
        Object.defineProperty(receiver, 'lastIndex', {writable:false});
        var failed = false;
        try {receiver.compile('new', 'i');} catch (e) {failed = e instanceof TypeError;}
        failed && receiver.source === 'new' && receiver.flags === 'i' && receiver.lastIndex === 0
    "#,
    );
}

#[test]
fn typed_array_ancestor_set_uses_the_distinct_receiver_and_converts_after_lookup() {
    assert_script(
        r#"
        var typed = new Uint8Array(1), target = Object.create(typed), conversions = 0;
        var value = {valueOf() {conversions++; return 257;}};
        var stored = Reflect.set(target, '0', value, typed);
        var ordinary = {};
        var copied = Reflect.set(target, '0', value, ordinary);
        stored && copied && typed[0] === 1 && ordinary[0] === value && conversions === 1
    "#,
    );
}

#[test]
fn a_resized_typed_array_ancestor_rechecks_the_index_after_value_conversion() {
    assert_script(
        r#"
        var buffer = new ArrayBuffer(1, {maxByteLength:4}), typed = new Uint8Array(buffer);
        var target = Object.create(typed), value = {valueOf() {buffer.resize(4); return 7;}};
        Reflect.set(target, '3', value, typed) && typed[3] === 7
    "#,
    );
}

#[test]
fn a_typed_array_ancestor_preserves_abrupt_conversion_and_skips_a_shrunken_index() {
    assert_script(
        r#"
        var sentinel = {}, buffer = new ArrayBuffer(4, {maxByteLength:8});
        var typed = new Uint8Array(buffer), target = Object.create(typed), caught = false;
        try {Reflect.set(target, '0', {valueOf() {throw sentinel;}}, typed);}
        catch(error) {caught = error === sentinel;}
        var value = {valueOf() {buffer.resize(1); return 42;}};
        caught && Reflect.set(target, '3', value, typed) && typed[3] === undefined
    "#,
    );
}

#[test]
fn revocable_proxies_reject_primitive_targets_and_handlers() {
    assert_script(
        r#"
        var passed = 0;
        for (var value of [undefined, null, 1, 'x', true, Symbol(), 1n]) {
            try {Proxy.revocable(value, {});} catch (e) {if (e instanceof TypeError) passed++;}
            try {Proxy.revocable({}, value);} catch (e) {if (e instanceof TypeError) passed++;}
        }
        passed === 14
    "#,
    );
}

#[test]
fn foreign_descriptors_and_explicit_receivers_keep_their_real_property_owners() {
    assert_script(
        r#"
        var other = $262.createRealm().global;
        var foreign = other.eval('({get value() {return this.mark;}})');
        var receiver = {mark:42};
        var descriptor = Object.getOwnPropertyDescriptor(foreign, 'value');
        typeof descriptor.get === 'function' && descriptor.get.call(receiver) === 42 &&
            Reflect.get(foreign, 'value', receiver) === 42
    "#,
    );
}

#[test]
fn regexp_compile_uses_pattern_slots_without_coercing_a_regexp_argument() {
    assert_script(
        r#"
        var receiver = /old/g, pattern = /new/i, calls = 0;
        pattern.toString = function() {calls++; throw 'coerced';};
        receiver.lastIndex = 3;
        var same = receiver.compile(pattern) === receiver;
        same && receiver.source === 'new' && receiver.flags === 'i' &&
            receiver.lastIndex === 0 && calls === 0
    "#,
    );
}

#[test]
fn regexp_compile_failure_preserves_existing_matcher_until_a_later_success() {
    assert_script(
        r#"
        var receiver = /old/g, sentinel = {}, same = false;
        try {receiver.compile({toString() {throw sentinel;}});} catch (e) {same = e === sentinel;}
        var syntax = false;
        try {receiver.compile('[');} catch (e) {syntax = e instanceof SyntaxError;}
        var retained = receiver.source === 'old' && receiver.flags === 'g';
        receiver.compile('new', 'i');
        same && syntax && retained && receiver.test('NEW')
    "#,
    );
}

#[test]
fn regexp_methods_reject_invalid_receivers_before_coercing_arguments() {
    assert_script(
        r#"
        var calls = 0, argument = {toString() {calls++; return 'x';}}, rejected = 0;
        for (var name of ['compile', 'exec', 'test', 'toString']) {
            try {RegExp.prototype[name].call(null, argument);}
            catch (e) {if (e instanceof TypeError) rejected++;}
        }
        rejected === 4 && calls === 0
    "#,
    );
}

#[test]
fn host_registration_rejects_invalid_names_without_retaining_callbacks() {
    let mut vm = Vm::default();
    for name in ["", "9value", "a-b", "a.b", "a\0b", "aé", "é"] {
        assert!(matches!(
            vm.install_host_function(name, 0, |_args: &[HostValue]| Ok(HostValue::Undefined)),
            Err(RuntimeError::TypeError(_))
        ));
        assert!(matches!(
            vm.install_host_object(name),
            Err(RuntimeError::TypeError(_))
        ));
    }
    let object = vm.install_host_object("$host_0").unwrap();
    vm.install_host_method(object, "_get9", 0, |_args: &[HostValue]| {
        Ok(HostValue::Number(42.0))
    })
    .unwrap();
    assert_eq!(
        vm.execute_script(&compile(&parse("$host_0._get9()").unwrap()).unwrap())
            .unwrap(),
        Value::Number(42.0)
    );
}

#[test]
fn host_return_string_limits_propagate_and_leave_the_vm_reusable() {
    let mut vm = Vm::new(VmConfig {
        max_string_bytes: 64,
        ..VmConfig::default()
    })
    .unwrap();
    vm.install_host_function("hostValue", 0, |_args: &[HostValue]| {
        Ok(HostValue::String("x".repeat(65).into()))
    })
    .unwrap();
    assert!(vm
        .execute_script(&compile(&parse("hostValue()").unwrap()).unwrap())
        .is_err());
    assert_eq!(
        vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap())
            .unwrap(),
        Value::Number(42.0)
    );
}

#[test]
fn regexp_generic_algorithms_preserve_abrupt_getters_and_coercions() {
    for (receiver, operation) in [
        ("{get lastIndex() {throw sentinel;}, exec() {return null;}}", "RegExp.prototype[Symbol.search].call(receiver, 'a')"),
        ("{source:{toString() {throw sentinel;}}, flags:''}", "RegExp.prototype.toString.call(receiver)"),
        ("{source:'a', get flags() {throw sentinel;}}", "RegExp.prototype.toString.call(receiver)"),
        ("{source:'a', flags:{toString() {throw sentinel;}}}", "RegExp.prototype.toString.call(receiver)"),
        ("{flags:'g', set lastIndex(value) {}, get lastIndex() {throw sentinel;}, exec() {return {0:''};}}", "RegExp.prototype[Symbol.match].call(receiver, 'a')"),
        ("{flags:'g', set lastIndex(value) {}, exec() {return {get 0() {throw sentinel;}};}}", "RegExp.prototype[Symbol.replace].call(receiver, 'a', 'x')"),
        ("{flags:'g', set lastIndex(value) {}, exec() {return {0:{toString() {throw sentinel;}}};}}", "RegExp.prototype[Symbol.replace].call(receiver, 'a', 'x')"),
        ("{flags:'', exec() {return {0:'a', length:1, index:{valueOf() {throw sentinel;}}};}}", "RegExp.prototype[Symbol.replace].call(receiver, 'a', 'x')"),
        ("{flags:'', exec() {return {0:'a', length:1, index:0};}}", "RegExp.prototype[Symbol.replace].call(receiver, 'a', () => ({toString() {throw sentinel;}}))"),
        ("{flags:'', exec() {return {0:'a', length:1, index:0, groups:{get name() {throw sentinel;}}};}}", "RegExp.prototype[Symbol.replace].call(receiver, 'a', '$<name>')"),
    ] {
        assert_script(&format!("var sentinel = {{}}, receiver = {receiver}, thrown; try {{{operation};}} catch (error) {{thrown = error;}} thrown === sentinel"));
    }
}
