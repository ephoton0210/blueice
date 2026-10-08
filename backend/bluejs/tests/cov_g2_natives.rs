// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Built-in functions dispatched by `native_call` and the shared conversion
//! helpers of `general.rs`: rejected receivers, throwing conversions and the
//! other rarely taken completions of each native.
mod cov_g2_support;

use cov_g2_support::expect_true;

/// Common prelude: `thrown(f)` returns what `f` throws (or 'none').
const PRELUDE: &str = "function thrown(f) { try { f(); return 'none'; } catch (e) { return e; } }
    function isType(f) { return thrown(f) instanceof TypeError; }
    function isRange(f) { return thrown(f) instanceof RangeError; }
    var boom = { valueOf() { throw 'boom'; }, toString() { throw 'boom'; } };
    function protoThrows() {
      return new Proxy(function () {}, { get(t, k) { if (k === 'prototype') throw 'proto'; return t[k]; } });
    }
    function getter(object, name) { return Object.getOwnPropertyDescriptor(object, name).get; }
    function setter(object, name) { return Object.getOwnPropertyDescriptor(object, name).set; }";

fn check(body: &str) {
    expect_true(&format!("{PRELUDE}\n{body}"));
}

#[test]
fn locale_compare_reports_bad_receivers_arguments_and_locales() {
    check(
        "isType(() => String.prototype.localeCompare.call(null, 'a')) &&
           thrown(() => 'a'.localeCompare(boom)) === 'boom' &&
           isRange(() => 'a'.localeCompare('b', 'not_a_locale!')) &&
           thrown(() => new Intl.Collator().compare(boom, 'a')) === 'boom' &&
           thrown(() => new Intl.Collator().compare('a', boom)) === 'boom' &&
           new Intl.Collator().compare('a', 'b') < 0",
    );
}

#[test]
fn the_array_constructor_validates_its_length_and_prototype() {
    check(
        "isRange(() => new Array(-1)) && isRange(() => Array(1.5)) && new Array(3).length === 3 &&
           thrown(() => Reflect.construct(Array, [], protoThrows())) === 'proto' &&
           Array(2, 3).length === 2 && Array(7).length === 7",
    );
}

#[test]
fn buffer_accessors_reject_foreign_receivers() {
    check(
        "var names = [[ArrayBuffer, 'byteLength'], [ArrayBuffer, 'detached'], [ArrayBuffer, 'maxByteLength'],
                      [ArrayBuffer, 'resizable'], [SharedArrayBuffer, 'byteLength'],
                      [SharedArrayBuffer, 'maxByteLength'], [SharedArrayBuffer, 'growable'],
                      [DataView, 'buffer'], [DataView, 'byteLength'], [DataView, 'byteOffset']];
         var TA = Object.getPrototypeOf(Int8Array.prototype);
         names.every(([c, n]) => isType(() => getter(c.prototype, n).call({})) &&
                                 isType(() => getter(c.prototype, n).call(1))) &&
           ['buffer', 'byteLength', 'byteOffset', 'length'].every(n =>
             isType(() => getter(TA, n).call({})) && isType(() => getter(TA, n).call(1))) &&
           getter(TA, Symbol.toStringTag).call({}) === undefined &&
           getter(TA, Symbol.toStringTag).call(1) === undefined &&
           getter(TA, Symbol.toStringTag).call(new Uint8Array(1)) === 'Uint8Array' &&
           isType(() => TA()) && isType(() => new TA())",
    );
}

#[test]
fn buffer_accessors_read_ordinary_receivers() {
    check(
        "var ab = new ArrayBuffer(8, { maxByteLength: 16 });
         var sab = new SharedArrayBuffer(4, { maxByteLength: 8 });
         var view = new Uint8Array(ab, 2, 4);
         var dv = new DataView(ab, 1, 3);
         ab.byteLength === 8 && ab.detached === false && ab.maxByteLength === 16 && ab.resizable &&
           sab.byteLength === 4 && sab.maxByteLength === 8 && sab.growable &&
           view.byteOffset === 2 && view.byteLength === 4 && view.length === 4 && view.buffer === ab &&
           dv.byteOffset === 1 && dv.byteLength === 3 && dv.buffer === ab &&
           ArrayBuffer.isView(view) && ArrayBuffer.isView(dv) && !ArrayBuffer.isView(ab) &&
           ArrayBuffer[Symbol.species] === ArrayBuffer && SharedArrayBuffer[Symbol.species] === SharedArrayBuffer",
    );
}

#[test]
fn detached_and_shrunken_views_read_as_empty() {
    check(
        "var ab = new ArrayBuffer(8, { maxByteLength: 8 });
         var view = new Uint8Array(ab, 4, 4);
         ab.resize(2);
         var lengths = [view.length, view.byteLength, view.byteOffset];
         var detached = new ArrayBuffer(4);
         var wrapped = new Uint8Array(detached);
         detached.transfer();
         lengths.join() === '0,0,0' && wrapped.length === 0 && wrapped.byteLength === 0 && wrapped.byteOffset === 0",
    );
}

#[test]
fn typed_array_iterators_and_from_base64() {
    check(
        "['values', 'entries', 'keys'].every(n => isType(() => Int8Array.prototype[n].call({}))) &&
           Uint8Array.fromBase64('AQID').join() === '1,2,3' &&
           Uint8Array.fromHex('0102').join() === '1,2' &&
           [...new Uint8Array([5, 6]).entries()].length === 2",
    );
}

#[test]
fn proxies_can_be_revoked_twice_and_collections_check_their_size_receivers() {
    check(
        "var r = Proxy.revocable({}, {}); r.revoke(); r.revoke();
         var mapSize = getter(Map.prototype, 'size'), setSize = getter(Set.prototype, 'size');
         isType(() => mapSize.call(1)) && isType(() => mapSize.call({})) &&
           isType(() => setSize.call(1)) && isType(() => setSize.call({})) &&
           mapSize.call(new Map([[1, 2]])) === 1 && setSize.call(new Set([1, 2])) === 2",
    );
}

#[test]
fn function_invocation_natives_reject_non_callables() {
    check(
        "function f() { return this; }
         isType(() => Function.prototype.apply.call(1)) &&
           isType(() => Function.prototype.toString.call(1)) &&
           isType(() => Reflect.apply(1, null, [])) &&
           isType(() => Reflect.construct(1, [])) && isType(() => Reflect.construct(f, [], 1)) &&
           thrown(() => Reflect.apply(f, null, { get length() { throw 'len'; } })) === 'len' &&
           thrown(() => f.apply(null, { get length() { throw 'len'; } })) === 'len' &&
           thrown(() => Reflect.construct(f, { get length() { throw 'len'; } })) === 'len' &&
           f.apply(1, null) instanceof Number && Function.prototype.toString.call(f).startsWith('function f') &&
           Function.prototype.toString.call(Math.max).includes('native code')",
    );
}

#[test]
fn number_and_boolean_constructors_convert_and_allocate() {
    check(
        "Number(2n ** 2000n) === Infinity && Number(-(2n ** 2000n)) === -Infinity && Number(5n) === 5 &&
           Number() === 0 && Number('7') === 7 &&
           thrown(() => Number(boom)) === 'boom' &&
           thrown(() => isNaN(boom)) === 'boom' && thrown(() => isFinite(boom)) === 'boom' &&
           thrown(() => Reflect.construct(Number, [1], protoThrows())) === 'proto' &&
           thrown(() => Reflect.construct(Boolean, [1], protoThrows())) === 'proto' &&
           typeof new Number(1) === 'object' && new Boolean(0).valueOf() === false &&
           isNaN('x') && !isFinite(Infinity) && Boolean('') === false",
    );
}

#[test]
fn bigint_natives_validate_receivers_radixes_and_widths() {
    check(
        "isType(() => new BigInt(1)) && isType(() => BigInt.prototype.toString.call(1)) &&
           isType(() => BigInt.prototype.valueOf.call({})) && isType(() => BigInt.prototype.toLocaleString.call(1)) &&
           isRange(() => 1n.toString(NaN)) && isRange(() => 1n.toString(37)) &&
           1n.toString(2) === '1' && Object(2n).toString() === '2' && Object(2n).valueOf() === 2n &&
           isRange(() => BigInt.asIntN(2 ** 40, 1n)) && BigInt.asIntN(0, 5n) === 0n &&
           BigInt.asUintN(8, -1n) === 255n && BigInt.asIntN(8, 255n) === -1n && BigInt.asIntN(8, 127n) === 127n &&
           isRange(() => BigInt(1.5)) && BigInt(3) === 3n && BigInt('12') === 12n && BigInt(true) === 1n &&
           isRange(() => 1n.toLocaleString('bad_locale!')) && typeof 1n.toLocaleString('en') === 'string' &&
           thrown(() => BigInt(boom)) === 'boom' && isType(() => BigInt(Symbol())) &&
           isType(() => BigInt(undefined)) &&
           thrown(() => BigInt.asIntN(boom, 1n)) === 'boom'",
    );
}

#[test]
fn primitive_wrapper_methods_reject_other_receivers() {
    check(
        "isType(() => Number.prototype.toString.call('x')) && isType(() => Boolean.prototype.valueOf.call(1)) &&
           isType(() => Symbol.prototype.toString.call(1)) && isType(() => Symbol.prototype.valueOf.call({})) &&
           isType(() => getter(Symbol.prototype, 'description').call(1)) &&
           getter(Symbol.prototype, 'description').call(Object(Symbol('d'))) === 'd' &&
           Symbol.prototype.toString.call(Object(Symbol('s'))) === 'Symbol(s)' &&
           isType(() => new Symbol()) && isType(() => Symbol.keyFor(1)) &&
           thrown(() => Symbol(boom)) === 'boom' && thrown(() => Symbol.for(boom)) === 'boom' &&
           Symbol.keyFor(Symbol.for('k')) === 'k' && Symbol.keyFor(Symbol('x')) === undefined &&
           new Number(5).toString() === '5' && new Boolean(true).valueOf() === true",
    );
}

#[test]
fn object_prototype_natives_cover_tags_accessors_and_proxies() {
    check(
        "var revoked = Proxy.revocable({}, {}); revoked.revoke();
         function tag(v) { return Object.prototype.toString.call(v); }
         tag(Object(1n)) === '[object BigInt]' && tag(Object(Symbol())) === '[object Symbol]' &&
           tag(new Number(1)) === '[object Number]' && tag(new Boolean(1)) === '[object Boolean]' &&
           tag(new String('')) === '[object String]' && tag(new Date(0)) === '[object Date]' &&
           tag(/x/) === '[object RegExp]' && tag(new Error()) === '[object Error]' &&
           tag(function () {}) === '[object Function]' && tag((function () { return arguments; })()) === '[object Arguments]' &&
           tag([]) === '[object Array]' && tag(null) === '[object Null]' && tag(undefined) === '[object Undefined]' &&
           tag(1) === '[object Number]' && tag('') === '[object String]' && tag(true) === '[object Boolean]' &&
           tag(1n) === '[object BigInt]' && tag(Symbol()) === '[object Symbol]' &&
           tag({ [Symbol.toStringTag]: 'Custom' }) === '[object Custom]' &&
           isType(() => tag(revoked.proxy)) &&
           thrown(() => tag(new Proxy({}, { get(t, k) { if (k === Symbol.toStringTag) throw 'tag'; return t[k]; } }))) === 'tag' &&
           isType(() => Object.prototype.toLocaleString.call(null)) &&
           isType(() => Object.prototype.toLocaleString.call({ toString: 1 })) &&
           Object.prototype.toLocaleString.call({ toString() { return 'ts'; } }) === 'ts' &&
           Array.prototype.toString.call({ join: 1 }) === '[object Object]' &&
           Array.prototype.toString.call({ join() { return 'j'; } }) === 'j'",
    );
}

#[test]
fn legacy_accessor_natives_reject_bad_arguments() {
    check(
        "var o = {};
         var revoked = Proxy.revocable({}, {}); revoked.revoke();
         isType(() => Object.prototype.__defineGetter__.call(null, 'x', function () {})) &&
           isType(() => o.__defineGetter__('x', 1)) && isType(() => o.__defineSetter__('x', 1)) &&
           thrown(() => o.__defineGetter__(boom, function () {})) === 'boom' &&
           isType(() => Object.freeze({}).__defineGetter__('x', function () {})) &&
           isType(() => Object.prototype.__lookupGetter__.call(null, 'x')) &&
           thrown(() => o.__lookupGetter__(boom)) === 'boom' &&
           o.__lookupGetter__('missing') === undefined && o.__lookupSetter__('missing') === undefined &&
           (o.__defineGetter__('g', function () { return 1; }), o.__lookupGetter__('g')() === 1) &&
           (o.__defineSetter__('s', function (v) {}), typeof o.__lookupSetter__('s') === 'function') &&
           Object.create(o).__lookupGetter__('g')() === 1 &&
           isType(() => revoked.proxy.__lookupGetter__('x')) &&
           isType(() => Object.prototype.isPrototypeOf.call(null, {})) &&
           Object.prototype.isPrototypeOf.call(null, 1) === false &&
           Object.prototype.isPrototypeOf.call(Object.prototype, {}) &&
           !Object.prototype.isPrototypeOf.call([], {}) &&
           isType(() => Object.prototype.isPrototypeOf.call(1, revoked.proxy))",
    );
}

#[test]
fn the_proto_accessor_validates_receivers_and_cycles() {
    check(
        "var proto = Object.getOwnPropertyDescriptor(Object.prototype, '__proto__');
         var a = {}, b = Object.create(a);
         isType(() => proto.get.call(null)) && isType(() => proto.set.call(undefined, {})) &&
           proto.set.call(1, {}) === undefined && proto.set.call({}, 1) === undefined &&
           isType(() => proto.set.call(a, b)) && proto.get.call(b) === a && proto.get.call(1) === Number.prototype &&
           proto.set.call(b, null) === undefined && Object.getPrototypeOf(b) === null &&
           isType(() => proto.set.call(Object.preventExtensions({}), {}))",
    );
}

#[test]
fn object_and_iterator_constructors_honor_new_target() {
    check(
        "class C extends Object {}
         class I extends Iterator {}
         new C() instanceof C && new I() instanceof Iterator && Object(null) instanceof Object &&
           new Object(undefined) instanceof Object && Object(1) instanceof Number &&
           thrown(() => Reflect.construct(Object, [], protoThrows())) === 'proto' &&
           thrown(() => Reflect.construct(Iterator, [], protoThrows())) === 'proto' &&
           isType(() => new Iterator()) && isType(() => Iterator()) && isType(() => new Iterator.from([])) &&
           Iterator.from([1]).next().value === 1",
    );
}

#[test]
fn string_natives_convert_and_reject() {
    check(
        "var iterProto = Object.getPrototypeOf(''[Symbol.iterator]());
         thrown(() => String(boom)) === 'boom' && String(Symbol('x')) === 'Symbol(x)' &&
           new String('ab').length === 2 && String() === '' &&
           thrown(() => String.fromCharCode(boom)) === 'boom' && thrown(() => String.fromCodePoint(boom)) === 'boom' &&
           String.fromCharCode(65, 66) === 'AB' && isRange(() => String.fromCodePoint(-1)) &&
           isType(() => String.prototype[Symbol.iterator].call(null)) &&
           isType(() => iterProto.next.call({})) && isType(() => iterProto.next.call(1)) &&
           [...'ab'].join() === 'a,b' &&
           thrown(() => Reflect.construct(String, ['a'], protoThrows())) === 'proto' &&
           isType(() => ''.at.call(null, 0)) && thrown(() => 'abc'.at(boom)) === 'boom' &&
           thrown(() => 'abc'.slice(boom)) === 'boom' && thrown(() => 'abc'.slice(0, boom)) === 'boom' &&
           thrown(() => 'abc'.indexOf(boom)) === 'boom' && thrown(() => 'abc'.indexOf('a', boom)) === 'boom' &&
           thrown(() => 'abc'.concat(boom)) === 'boom' && thrown(() => 'abc'.padStart(boom)) === 'boom' &&
           thrown(() => 'abc'.padStart(5, boom)) === 'boom' && thrown(() => 'abc'.normalize(boom)) === 'boom' &&
           thrown(() => 'abc'.anchor(boom)) === 'boom' && thrown(() => 'abc'.includes(/a/)) instanceof TypeError &&
           thrown(() => 'abc'.startsWith(/a/)) instanceof TypeError && thrown(() => 'abc'.repeat(boom)) === 'boom' &&
           thrown(() => 'abc'.includes(boom)) === 'boom' && thrown(() => 'abc'.substr(boom)) === 'boom' &&
           thrown(() => 'abc'.substring(1, boom)) === 'boom' && thrown(() => 'abc'.charAt(boom)) === 'boom' &&
           thrown(() => 'abc'.padEnd(boom)) === 'boom' && thrown(() => 'abc'.lastIndexOf('a', boom)) === 'boom' &&
           thrown(() => 'abc'.endsWith(boom)) === 'boom' && thrown(() => 'abc'.codePointAt(boom)) === 'boom'",
    );
}

#[test]
fn global_functions_convert_their_arguments() {
    check(
        "thrown(() => parseInt(boom)) === 'boom' && thrown(() => parseInt('1', boom)) === 'boom' &&
           thrown(() => parseFloat(boom)) === 'boom' && parseFloat('-Infinity') === -Infinity &&
           parseFloat('Infinity') === Infinity && isNaN(parseFloat('-')) &&
           thrown(() => encodeURI(boom)) === 'boom' && thrown(() => decodeURI(boom)) === 'boom' &&
           thrown(() => escape(boom)) === 'boom' && thrown(() => unescape(boom)) === 'boom' &&
           thrown(() => decodeURIComponent('%')) instanceof URIError &&
           thrown(() => encodeURI('\\ud800')) instanceof URIError &&
           parseInt('  0x1f') === 31 && parseInt('12', 36) === 38 && isNaN(parseInt('12', 1)) &&
           thrown(() => new Array(2 ** 32)) instanceof RangeError &&
           thrown(() => Object.defineProperty([], 'length', { value: boom })) === 'boom' &&
           thrown(() => [].length = -1) instanceof RangeError",
    );
}

#[test]
fn property_key_coercion_reports_conversion_errors() {
    check(
        "var o = {};
         thrown(() => o[boom]) === 'boom' && thrown(() => { o[boom] = 1; }) === 'boom' &&
           thrown(() => Object.defineProperty(o, boom, {})) === 'boom' &&
           thrown(() => ({ [boom]: 1 })) === 'boom' && o[1n] === undefined && o[Symbol.iterator] === undefined &&
           (o[2n] = 'x', o['2'] === 'x') && thrown(() => o[{ [Symbol.toPrimitive]() { return {}; } }]) instanceof TypeError &&
           thrown(() => o[{ [Symbol.toPrimitive]: 1 }]) instanceof TypeError &&
           thrown(() => o[{ toString: null, valueOf: null }]) instanceof TypeError &&
           thrown(() => o[{ get [Symbol.toPrimitive]() { throw 'prim'; } }]) === 'prim'",
    );
}

#[test]
fn generator_natives_reject_foreign_receivers() {
    check(
        "var G = Object.getPrototypeOf(function* () {}).prototype;
         var AG = Object.getPrototypeOf(async function* () {}).prototype;
         isType(() => G.next.call({})) && isType(() => G.return.call(1)) && isType(() => G.throw.call({})) &&
           (function () { var it = AG.next.call({}); return it instanceof Promise; })()",
    );
}

#[test]
fn array_iterator_next_reports_bad_receivers_and_getters() {
    check(
        "var next = Object.getPrototypeOf([][Symbol.iterator]()).next;
         isType(() => next.call(1)) && isType(() => next.call({})) &&
           thrown(() => Array.prototype.values.call({ get length() { throw 'len'; } }).next()) === 'len' &&
           thrown(() => Array.prototype.values.call({ length: 1, get 0() { throw 'elem'; } }).next()) === 'elem' &&
           thrown(() => Array.prototype.entries.call({ length: 1, get 0() { throw 'elem'; } }).next()) === 'elem' &&
           isType(() => Array.prototype.values.call(null)) &&
           [...Array.prototype.keys.call({ length: 2 })].join() === '0,1' &&
           [...Array.prototype.entries.call({ length: 1, 0: 'a' })][0].join() === '0,a' &&
           new Int8Array([1, 2]).values().next().value === 1",
    );
}

#[test]
fn array_push_takes_the_generic_route_for_locked_receivers() {
    check(
        "var frozen = Object.freeze([1]);
         var proto = Object.setPrototypeOf([], { set 0(v) { throw 'setter'; } });
         isType(() => frozen.push(2)) && thrown(() => Array.prototype.push.call(proto, 1)) === 'setter' &&
           Array.prototype.push.call({ length: 1 }, 'x') === 2 && [1].push(2, 3) === 3 &&
           thrown(() => Array.prototype.push.call(null)) instanceof TypeError &&
           (function () { var a = [1]; Object.defineProperty(a, 'length', { writable: false }); return isType(() => a.push(1)); })()",
    );
}

#[test]
fn error_and_function_helpers_report_receivers() {
    check(
        "isType(() => Function.prototype.call.call(1)) && isType(() => Function.prototype.bind.call(1)) &&
           Function.prototype[Symbol.hasInstance].call(1, {}) === false &&
           (function () { 'use strict'; return isType(() => (function () {}).caller); })() &&
           Function.prototype[Symbol.hasInstance].call(Array, []) &&
           thrown(() => Object.getOwnPropertyDescriptor(Function.prototype, 'caller').get.call(1)) instanceof TypeError",
    );
}
