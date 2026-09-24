// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ArrayBuffer, SharedArrayBuffer, DataView and Atomics: constructor and
//! receiver validation, argument coercion, species construction, resizing and
//! transfer, and the errors each step reports.

mod cov_g4_common;
use cov_g4_common::failures;

const PRELUDE: &str = r#"
var boom = {};
function throwsBoom(f) { try { f(); return false } catch (e) { return e === boom } }
var thrower = { valueOf: function () { throw boom } };
function detach(buffer) { $262.detachArrayBuffer(buffer) }
"#;

fn run(body: &str) -> String {
    failures(&format!("{PRELUDE}{body}"))
}

#[test]
fn buffer_constructors_validate_their_arguments() {
    assert_eq!(
        run(r#"
th('ArrayBuffer without new', function () { ArrayBuffer(1) }, TypeError);
th('SharedArrayBuffer without new', function () { SharedArrayBuffer(1) }, TypeError);
eq('ArrayBuffer without arguments', new ArrayBuffer().byteLength, 0);
eq('SharedArrayBuffer without arguments', new SharedArrayBuffer().byteLength, 0);
eq('ArrayBuffer length conversion', throwsBoom(function () { new ArrayBuffer(thrower) }), true);
eq('SharedArrayBuffer length conversion', throwsBoom(function () { new SharedArrayBuffer(thrower) }), true);
eq('maxByteLength getter', throwsBoom(function () { new ArrayBuffer(1, { get maxByteLength() { throw boom } }) }), true);
eq('SharedArrayBuffer maxByteLength getter', throwsBoom(function () { new SharedArrayBuffer(1, { get maxByteLength() { throw boom } }) }), true);
eq('maxByteLength conversion', throwsBoom(function () { new ArrayBuffer(1, { maxByteLength: thrower }) }), true);
eq('options that are not an object', new ArrayBuffer(4, 5).resizable, false);
eq('options without maxByteLength', new ArrayBuffer(4, {}).resizable, false);
eq('undefined maxByteLength', new ArrayBuffer(4, { maxByteLength: undefined }).resizable, false);
eq('resizable', new ArrayBuffer(4, { maxByteLength: 8 }).maxByteLength, 8);
eq('growable', new SharedArrayBuffer(4, { maxByteLength: 8 }).maxByteLength, 8);
th('length above the maximum', function () { new ArrayBuffer(10, { maxByteLength: 5 }) }, RangeError);
th('shared length above the maximum', function () { new SharedArrayBuffer(10, { maxByteLength: 5 }) }, RangeError);
th('negative length', function () { new ArrayBuffer(-1) }, RangeError);
th('infinite length', function () { new ArrayBuffer(Infinity) }, RangeError);
th('huge length', function () { new ArrayBuffer(2 ** 53) }, RangeError);
eq('negative zero length', new ArrayBuffer(-0.5).byteLength, 0);
var NewTarget = function () {}.bind();
Object.setPrototypeOf(NewTarget, { get prototype() { throw boom } });
eq('ArrayBuffer new.target prototype', throwsBoom(function () { Reflect.construct(ArrayBuffer, [1], NewTarget) }), true);
eq('SharedArrayBuffer new.target prototype', throwsBoom(function () { Reflect.construct(SharedArrayBuffer, [1], NewTarget) }), true);
eq('DataView new.target prototype', throwsBoom(function () { Reflect.construct(DataView, [new ArrayBuffer(1)], NewTarget) }), true);
eq('Uint8Array new.target prototype', throwsBoom(function () { Reflect.construct(Uint8Array, [1], NewTarget) }), true);
"#),
        ""
    );
}

#[test]
fn buffer_methods_validate_their_receivers() {
    assert_eq!(
        run(r#"
var abProto = ArrayBuffer.prototype, sabProto = SharedArrayBuffer.prototype;
var ab = new ArrayBuffer(8), sab = new SharedArrayBuffer(8, { maxByteLength: 16 });
[['resize', [1]], ['slice', [0]], ['transfer', []], ['transferToFixedLength', []]].forEach(function (entry) {
  [{}, 1, undefined, sab].forEach(function (receiver, index) {
    th('ArrayBuffer ' + entry[0] + ' receiver ' + index, function () { abProto[entry[0]].apply(receiver, entry[1]) }, TypeError);
  });
});
[['grow', [9]], ['slice', [0]]].forEach(function (entry) {
  [{}, 1, undefined, ab].forEach(function (receiver, index) {
    th('SharedArrayBuffer ' + entry[0] + ' receiver ' + index, function () { sabProto[entry[0]].apply(receiver, entry[1]) }, TypeError);
  });
});
var immutable = new ArrayBuffer(8).transferToImmutable();
th('resize an immutable buffer', function () { immutable.resize(4) }, TypeError);
th('resize a fixed-length buffer', function () { ab.resize(4) }, TypeError);
th('grow a fixed-length shared buffer', function () { new SharedArrayBuffer(8).grow(9) }, TypeError);
var rab = new ArrayBuffer(4, { maxByteLength: 8 });
eq('resize length conversion', throwsBoom(function () { rab.resize(thrower) }), true);
th('resize past the maximum', function () { rab.resize(9) }, RangeError);
eq('resize', (rab.resize(6), rab.byteLength), 6);
eq('grow length conversion', throwsBoom(function () { sab.grow(thrower) }), true);
th('grow past the maximum', function () { sab.grow(17) }, RangeError);
th('shrink a shared buffer', function () { sab.grow(2) }, RangeError);
eq('grow', (sab.grow(12), sab.byteLength), 12);
"#),
        ""
    );
}

#[test]
fn transfer_copies_into_a_new_buffer_and_detaches_the_source() {
    assert_eq!(
        run(r#"
var rab = new ArrayBuffer(4, { maxByteLength: 8 });
new Uint8Array(rab).set([1, 2, 3, 4]);
var moved = rab.transfer(6);
eq('detached source', rab.detached, true);
eq('resizable transfer', moved.resizable + ':' + moved.byteLength + ':' + moved.maxByteLength, 'true:6:8');
eq('transferred bytes', new Uint8Array(moved).join(), '1,2,3,4,0,0');
var rab2 = new ArrayBuffer(4, { maxByteLength: 8 });
th('transfer beyond the maximum', function () { rab2.transfer(9) }, RangeError);
eq('a failed transfer keeps the source', rab2.detached, false);
var fixed = rab2.transferToFixedLength(2);
eq('fixed-length transfer', fixed.resizable + ':' + fixed.byteLength, 'false:2');
var plain = new ArrayBuffer(4);
eq('plain transfer', plain.transfer().byteLength, 4);
th('transfer a detached buffer', function () { plain.transfer() }, TypeError);
"#),
        ""
    );
}

#[test]
fn buffer_slice_resolves_its_species_constructor() {
    assert_eq!(
        run(r#"
function slices(make, methodOwner, sliceName) {
  var name = methodOwner === SharedArrayBuffer ? 'SharedArrayBuffer' : 'ArrayBuffer';
  var slice = methodOwner.prototype.slice;
  var b = make();
  b.constructor = undefined;
  eq(name + ' constructor undefined', slice.call(b, 0).constructor === methodOwner, true);
  b = make(); b.constructor = 1;
  th(name + ' constructor not an object', function () { slice.call(b, 0) }, TypeError);
  b = make(); b.constructor = { [Symbol.species]: null };
  eq(name + ' species null', slice.call(b, 0).constructor === methodOwner, true);
  b = make(); b.constructor = { [Symbol.species]: undefined };
  eq(name + ' species undefined', slice.call(b, 0).constructor === methodOwner, true);
  b = make(); b.constructor = { [Symbol.species]: 1 };
  th(name + ' species not a constructor', function () { slice.call(b, 0) }, TypeError);
  b = make(); b.constructor = { [Symbol.species]: function () {}.bind().bind() && (() => {}) };
  th(name + ' species arrow function', function () { slice.call(b, 0) }, TypeError);
  b = make(); Object.defineProperty(b, 'constructor', { get: function () { throw boom } });
  eq(name + ' constructor getter', throwsBoom(function () { slice.call(b, 0) }), true);
  b = make(); b.constructor = { get [Symbol.species]() { throw boom } };
  eq(name + ' species getter', throwsBoom(function () { slice.call(b, 0) }), true);
  b = make();
  eq(name + ' start conversion', throwsBoom(function () { slice.call(b, thrower) }), true);
  eq(name + ' end conversion', throwsBoom(function () { slice.call(b, 0, thrower) }), true);
  eq(name + ' negative bounds', slice.call(b, -4, -1).byteLength, 3);
  eq(name + ' infinite bounds', slice.call(b, -Infinity, Infinity).byteLength, 8);
  eq(name + ' NaN start', slice.call(b, NaN, 2).byteLength, 2);
  eq(name + ' reversed', slice.call(b, 5, 2).byteLength, 0);
  b = make(); b.constructor = { [Symbol.species]: function (n) { return { byteLength: n } } };
  th(name + ' species result is not a buffer', function () { slice.call(b, 0) }, TypeError);
  b = make(); b.constructor = { [Symbol.species]: function (n) { return b } };
  th(name + ' species result is the source', function () { slice.call(b, 0) }, TypeError);
  b = make(); b.constructor = { [Symbol.species]: function (n) { return new methodOwner(n - 1) } };
  th(name + ' species result is too small', function () { slice.call(b, 0) }, TypeError);
  b = make(); var copy = new methodOwner(8);
  b.constructor = { [Symbol.species]: function (n) { return copy } };
  eq(name + ' species result is used', slice.call(b, 2, 5) === copy, true);
}
slices(function () { return new ArrayBuffer(8) }, ArrayBuffer);
slices(function () { return new SharedArrayBuffer(8) }, SharedArrayBuffer);
"#),
        ""
    );
}

#[test]
fn array_buffer_slice_reports_a_species_that_changes_the_source_or_result() {
    assert_eq!(
        run(r#"
var b = new ArrayBuffer(8);
detach(b);
th('detached source', function () { b.slice(0) }, TypeError);
b = new ArrayBuffer(8);
b.constructor = { [Symbol.species]: function (n) { detach(b); return new ArrayBuffer(n) } };
th('detached by the species', function () { b.slice(0) }, TypeError);
b = new ArrayBuffer(8);
b.constructor = { [Symbol.species]: function (n) { var r = new ArrayBuffer(n); detach(r); return r } };
th('detached result', function () { b.slice(0) }, TypeError);
b = new ArrayBuffer(8);
b.constructor = { [Symbol.species]: function (n) { return new ArrayBuffer(n).transferToImmutable() } };
th('immutable result', function () { b.slice(0) }, TypeError);
b = new ArrayBuffer(8, { maxByteLength: 16 });
b.constructor = { [Symbol.species]: function (n) { b.resize(2); return new ArrayBuffer(n) } };
th('shrunk by the species', function () { b.slice(0, 8) }, Error);
b = new ArrayBuffer(8);
b.constructor = { [Symbol.species]: function (n) { return new SharedArrayBuffer(n) } };
th('shared result', function () { b.slice(0) }, TypeError);
"#),
        ""
    );
}

#[test]
fn data_view_construction_validates_its_buffer_offset_and_length() {
    assert_eq!(
        run(r#"
th('DataView without new', function () { DataView(new ArrayBuffer(8)) }, TypeError);
th('DataView of a primitive', function () { new DataView(1) }, TypeError);
th('DataView of an object that is not a buffer', function () { new DataView({}) }, TypeError);
var buffer = new ArrayBuffer(8);
eq('offset conversion', throwsBoom(function () { new DataView(buffer, thrower) }), true);
eq('length conversion', throwsBoom(function () { new DataView(buffer, 0, thrower) }), true);
th('offset past the end', function () { new DataView(buffer, 9) }, RangeError);
th('length past the end', function () { new DataView(buffer, 4, 5) }, RangeError);
th('negative offset', function () { new DataView(buffer, -1) }, RangeError);
var detached = new ArrayBuffer(8);
detach(detached);
th('detached buffer', function () { new DataView(detached) }, TypeError);
eq('offset conversion before the detached check', throwsBoom(function () { new DataView(detached, thrower) }), true);
var shrinking = new ArrayBuffer(8, { maxByteLength: 16 });
var NewTarget = function () {}.bind();
Object.defineProperty(NewTarget, 'prototype', { value: DataView.prototype });
var late = { get prototype() { detach(detached2); return DataView.prototype } };
var detached2 = new ArrayBuffer(8);
var lateTarget = function () {}.bind();
Object.setPrototypeOf(lateTarget, late);
th('detached by new.target.prototype', function () { Reflect.construct(DataView, [detached2], lateTarget) }, TypeError);
var shrunk = new ArrayBuffer(8, { maxByteLength: 16 });
var shrinkTarget = function () {}.bind();
Object.setPrototypeOf(shrinkTarget, { get prototype() { shrunk.resize(2); return DataView.prototype } });
th('offset past a buffer shrunk by new.target.prototype', function () { Reflect.construct(DataView, [shrunk, 4], shrinkTarget) }, RangeError);
var shrunk2 = new ArrayBuffer(8, { maxByteLength: 16 });
var shrinkTarget2 = function () {}.bind();
Object.setPrototypeOf(shrinkTarget2, { get prototype() { shrunk2.resize(3); return DataView.prototype } });
th('length past a buffer shrunk by new.target.prototype', function () { Reflect.construct(DataView, [shrunk2, 0, 8], shrinkTarget2) }, RangeError);
var tracking = new ArrayBuffer(8, { maxByteLength: 16 });
var view = new DataView(tracking, 2);
eq('length tracking', view.byteLength, 6);
tracking.resize(10);
eq('tracking grows', view.byteLength, 8);
eq('fixed length', new DataView(buffer, 2, 4).byteLength, 4);
eq('shared buffer', new DataView(new SharedArrayBuffer(4)).byteLength, 4);
var realm = $262.createRealm();
eq('foreign buffer', new DataView(new realm.global.ArrayBuffer(4)).byteLength, 4);
th('foreign object that is not a buffer', function () { new DataView(new realm.global.Object()) }, TypeError);
"#),
        ""
    );
}

#[test]
fn data_view_accessors_validate_their_receiver_index_and_value() {
    assert_eq!(
        run(r#"
var buffer = new ArrayBuffer(8, { maxByteLength: 16 });
var view = new DataView(buffer);
var protos = DataView.prototype;
['getInt8', 'getUint8', 'getInt16', 'getFloat32', 'getFloat64', 'getBigInt64'].forEach(function (name) {
  th(name + ' on a primitive', function () { protos[name].call(1, 0) }, TypeError);
  th(name + ' on an object', function () { protos[name].call({}, 0) }, TypeError);
  eq(name + ' index conversion', throwsBoom(function () { view[name](thrower) }), true);
  th(name + ' index past the end', function () { view[name](8) }, RangeError);
  th(name + ' negative index', function () { view[name](-1) }, RangeError);
  th(name + ' index beyond 2^53', function () { view[name](2 ** 53) }, RangeError);
});
['setInt8', 'setUint16', 'setFloat32', 'setFloat64', 'setBigInt64'].forEach(function (name) {
  var value = name.indexOf('Big') >= 0 ? 1n : 1;
  th(name + ' on a primitive', function () { protos[name].call(1, 0, value) }, TypeError);
  th(name + ' on an object', function () { protos[name].call({}, 0, value) }, TypeError);
  eq(name + ' index conversion', throwsBoom(function () { view[name](thrower, value) }), true);
  eq(name + ' value conversion', throwsBoom(function () { view[name](0, thrower) }), true);
  th(name + ' index past the end', function () { view[name](8, value) }, RangeError);
});
th('setBigInt64 with a number', function () { view.setBigInt64(0, 1) }, TypeError);
eq('little-endian flag conversion', throwsBoom(function () { view.getInt16(0, { valueOf: function () { throw boom }, get [Symbol.toPrimitive]() { throw boom } }) }), false);
eq('set then get', (view.setInt16(0, -2, true), view.getInt16(0, true)), -2);
eq('big-endian', (view.setUint16(0, 258), view.getUint8(0) + ':' + view.getUint8(1)), '1:2');
eq('bigint', (view.setBigInt64(0, -5n), view.getBigInt64(0)), -5n);
eq('float', (view.setFloat32(0, 1.5), view.getFloat32(0)), 1.5);
var late = new DataView(new ArrayBuffer(8));
th('value conversion detaches', function () { late.setInt8(0, { valueOf: function () { detach(late.buffer); return 1 } }) }, TypeError);
var shrinker = new ArrayBuffer(8, { maxByteLength: 16 });
var shrinking = new DataView(shrinker, 0, 8);
th('a view over a shrunk buffer', function () { shrinker.resize(2); shrinking.getInt8(0) }, TypeError);
th('setter on a view over a shrunk buffer', function () { shrinking.setInt8(0, 1) }, TypeError);
var immutable = new ArrayBuffer(8).transferToImmutable();
th('set on an immutable buffer', function () { new DataView(immutable).setInt8(0, 1) }, TypeError);
eq('get on an immutable buffer', new DataView(immutable).getInt8(0), 0);
eq('shared', (new DataView(new SharedArrayBuffer(4)).setUint8(3, 9), 1), 1);
"#),
        ""
    );
}

#[test]
fn atomics_validate_their_typed_array_index_and_operands() {
    assert_eq!(
        run(r#"
var ops = ['add', 'and', 'compareExchange', 'exchange', 'load', 'or', 'store', 'sub', 'xor'];
var i32 = new Int32Array(new SharedArrayBuffer(16));
var f64 = new Float64Array(2);
var big = new BigInt64Array(2);
ops.forEach(function (name) {
  var args = function (target, index) { return name === 'compareExchange' ? [target, index, 0, 1] : [target, index, 1] };
  th(name + ' on a primitive', function () { Atomics[name].apply(null, args(1, 0)) }, TypeError);
  th(name + ' on a plain object', function () { Atomics[name].apply(null, args({}, 0)) }, TypeError);
  th(name + ' on a float array', function () { Atomics[name].apply(null, args(f64, 0)) }, TypeError);
  th(name + ' index past the end', function () { Atomics[name].apply(null, args(i32, 4)) }, RangeError);
  eq(name + ' index conversion', throwsBoom(function () { Atomics[name].apply(null, args(i32, thrower)) }), true);
  if (name !== 'load') {
    eq(name + ' operand conversion', throwsBoom(function () { Atomics[name](i32, 0, thrower, thrower) }), true);
    th(name + ' on a bigint array with a number', function () { Atomics[name](big, 0, 1, 1) }, TypeError);
  }
});
var detached = new Int32Array(4);
detach(detached.buffer);
th('load from a detached array', function () { Atomics.load(detached, 0) }, TypeError);
var rab = new ArrayBuffer(16, { maxByteLength: 16 });
var shrinking = new Int32Array(rab);
th('operand conversion shrinks the buffer', function () { Atomics.add(shrinking, 3, { valueOf: function () { rab.resize(4); return 1 } }) }, RangeError);
var rab2 = new ArrayBuffer(16, { maxByteLength: 16 });
var detaching = new Int32Array(rab2);
th('operand conversion detaches the buffer', function () { Atomics.add(detaching, 0, { valueOf: function () { detach(rab2); return 1 } }) }, TypeError);
var results = [];
var a = new Int32Array(new ArrayBuffer(16));
Atomics.store(a, 0, 6);
results.push(Atomics.add(a, 0, 3), Atomics.sub(a, 0, 1), Atomics.and(a, 0, 12), Atomics.or(a, 0, 3), Atomics.xor(a, 0, 5), Atomics.exchange(a, 0, 9), Atomics.compareExchange(a, 0, 9, 4), Atomics.load(a, 0));
eq('number operations', results.join(), '6,9,8,8,11,14,9,4');
var b = new BigInt64Array(new ArrayBuffer(16));
Atomics.store(b, 0, 6n);
results = [Atomics.add(b, 0, 3n), Atomics.sub(b, 0, 1n), Atomics.and(b, 0, 12n), Atomics.or(b, 0, 3n), Atomics.xor(b, 0, 5n), Atomics.exchange(b, 0, 9n), Atomics.compareExchange(b, 0, 9n, 4n), Atomics.load(b, 0)];
eq('bigint operations', results.join(), '6,9,8,8,11,14,9,4');
var shared = new Int32Array(new SharedArrayBuffer(16));
Atomics.store(shared, 1, 7);
eq('shared operations', [Atomics.add(shared, 1, 1), Atomics.load(shared, 1)].join(), '7,8');
th('store into an immutable buffer', function () { Atomics.store(new Int32Array(new ArrayBuffer(8).transferToImmutable()), 0, 1) }, TypeError);
eq('isLockFree', [Atomics.isLockFree(1), Atomics.isLockFree(3), Atomics.isLockFree(Infinity), Atomics.isLockFree(NaN), Atomics.isLockFree(8.9)].join(), 'true,false,false,false,true');
eq('isLockFree conversion', throwsBoom(function () { Atomics.isLockFree(thrower) }), true);
"#),
        ""
    );
}

#[test]
fn atomics_wait_and_notify_validate_their_arguments() {
    assert_eq!(
        run(r#"
var sab = new SharedArrayBuffer(16);
var i32 = new Int32Array(sab);
var plain = new Int32Array(4);
var i16 = new Int16Array(sab);
['wait', 'waitAsync'].forEach(function (name) {
  th(name + ' on a primitive', function () { Atomics[name](1, 0, 0, 0) }, TypeError);
  th(name + ' on a plain object', function () { Atomics[name]({}, 0, 0, 0) }, TypeError);
  th(name + ' on a non-shared array', function () { Atomics[name](plain, 0, 0, 0) }, TypeError);
  th(name + ' on a narrow array', function () { Atomics[name](i16, 0, 0, 0) }, TypeError);
  th(name + ' index past the end', function () { Atomics[name](i32, 4, 0, 0) }, RangeError);
  eq(name + ' index conversion', throwsBoom(function () { Atomics[name](i32, thrower, 0, 0) }), true);
  eq(name + ' value conversion', throwsBoom(function () { Atomics[name](i32, 0, thrower, 0) }), true);
  eq(name + ' timeout conversion', throwsBoom(function () { Atomics[name](i32, 0, 0, thrower) }), true);
});
eq('wait with a different value', Atomics.wait(i32, 0, 1, 0), 'not-equal');
eq('wait that times out at once', Atomics.wait(i32, 0, 0, 0), 'timed-out');
eq('wait that times out', Atomics.wait(i32, 0, 0, 1), 'timed-out');
eq('wait with a negative timeout', Atomics.wait(i32, 0, 0, -5), 'timed-out');
eq('waitAsync with a different value', Atomics.waitAsync(i32, 0, 1, 0).value, 'not-equal');
eq('waitAsync that times out at once', Atomics.waitAsync(i32, 0, 0, 0).value, 'timed-out');
var pending = Atomics.waitAsync(i32, 0, 0, 5);
eq('waitAsync that waits', pending.async, true);
eq('waitAsync with a NaN timeout', Atomics.waitAsync(i32, 1, 0, NaN).async, true);
eq('waitAsync without a timeout', Atomics.waitAsync(i32, 2, 0).async, true);
eq('waitAsync with an infinite timeout', Atomics.waitAsync(i32, 3, 0, Infinity).async, true);
var big = new BigInt64Array(new SharedArrayBuffer(16));
eq('bigint wait', Atomics.wait(big, 0, 0n, 0), 'timed-out');
th('bigint wait with a number', function () { Atomics.wait(big, 0, 0, 0) }, TypeError);
th('notify on a primitive', function () { Atomics.notify(1, 0, 1) }, TypeError);
th('notify on a narrow array', function () { Atomics.notify(i16, 0, 1) }, TypeError);
th('notify index past the end', function () { Atomics.notify(i32, 9, 1) }, RangeError);
eq('notify count conversion', throwsBoom(function () { Atomics.notify(i32, 0, thrower) }), true);
eq('notify non-shared', Atomics.notify(plain, 0, 1), 0);
var lonely = new Int32Array(new SharedArrayBuffer(16));
eq('notify nobody', [Atomics.notify(lonely, 0), Atomics.notify(lonely, 0, 1), Atomics.notify(lonely, 0, NaN), Atomics.notify(lonely, 0, -1), Atomics.notify(lonely, 0, Infinity), Atomics.notify(lonely, 0, undefined)].join(), '0,0,0,0,0,0');
"#),
        ""
    );
}

#[test]
fn typed_array_construction_validates_buffers_sources_and_lengths() {
    assert_eq!(
        run(r#"
th('without new', function () { Uint8Array(1) }, TypeError);
th('symbol length', function () { new Uint8Array(Symbol()) }, TypeError);
th('bigint length', function () { new Uint8Array(1n) }, TypeError);
th('huge length', function () { new Uint8Array(2 ** 53 - 1) }, RangeError);
th('negative length', function () { new Uint8Array(-1) }, RangeError);
th('huge wide length', function () { new Float64Array(2 ** 53 - 1) }, RangeError);
eq('undefined length', new Uint8Array(undefined).length, 0);
var buffer = new ArrayBuffer(8);
th('unaligned offset', function () { new Int16Array(buffer, 1) }, RangeError);
th('offset past the end', function () { new Int16Array(buffer, 10) }, RangeError);
th('unaligned buffer length', function () { new Int16Array(new ArrayBuffer(7)) }, RangeError);
th('length past the end', function () { new Int16Array(buffer, 2, 4) }, RangeError);
eq('offset conversion', throwsBoom(function () { new Int16Array(buffer, thrower) }), true);
eq('length conversion', throwsBoom(function () { new Int16Array(buffer, 0, thrower) }), true);
var detached = new ArrayBuffer(8);
detach(detached);
th('detached buffer', function () { new Uint8Array(detached) }, TypeError);
var rab = new ArrayBuffer(7, { maxByteLength: 16 });
eq('auto-length view over a resizable buffer', new Int16Array(rab).length, 3);
eq('auto-length view over a growable buffer', new Int16Array(new SharedArrayBuffer(7, { maxByteLength: 16 })).length, 3);
var source = new Uint8Array([1, 2, 3]);
eq('typed array source', new Int16Array(source).join(), '1,2,3');
var bad = new Uint8Array([1, 2, 3]);
detach(bad.buffer);
th('detached typed array source', function () { new Uint8Array(bad) }, TypeError);
eq('iterator getter throws boom', throwsBoom(function () { new Uint8Array({ get [Symbol.iterator]() { throw boom } }) }), true);
eq('iterable', new Uint8Array(new Set([4, 5])).join(), '4,5');
eq('iterator next throws', throwsBoom(function () { new Uint8Array({ [Symbol.iterator]: function () { return { next: function () { throw boom } } } }) }), true);
eq('array-like', new Uint8Array({ length: 2, 0: 7, 1: 8 }).join(), '7,8');
eq('array-like length getter', throwsBoom(function () { new Uint8Array({ get length() { throw boom } }) }), true);
eq('array-like length conversion', throwsBoom(function () { new Uint8Array({ length: thrower }) }), true);
eq('array-like element getter', throwsBoom(function () { new Uint8Array({ length: 1, get 0() { throw boom } }) }), true);
th('array-like too long', function () { new Uint8Array({ length: 2 ** 53 - 1 }) }, RangeError);
eq('element conversion', throwsBoom(function () { new Uint8Array([thrower]) }), true);
th('bigint element into a number array', function () { new Uint8Array([1n]) }, TypeError);
th('number element into a bigint array', function () { new BigInt64Array([1]) }, TypeError);
var realm = $262.createRealm();
eq('foreign buffer', new Int16Array(new realm.global.ArrayBuffer(8)).length, 4);
eq('foreign typed array', new Uint8Array(new realm.global.Uint8Array([1, 2])).join(), '1,2');
eq('subclass', new (class extends Uint8Array {})(2).length, 2);
"#),
        ""
    );
}
