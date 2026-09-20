// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The Immutable ArrayBuffer proposal (tc39/proposal-immutable-arraybuffer,
//! Test262 feature `immutable-arraybuffer`): the `[[ArrayBufferIsImmutable]]`
//! state, the ArrayBuffer API that creates and rejects it, and every write
//! path that must refuse an immutable-backed view.
//!
//! Each test runs one script and collects the names of the checks that
//! failed, so a regression names the exact behaviour instead of reporting a
//! bare `false`.

use blueice_bluejs::{compile, parse, HeapConfig, Value, Vm, VmConfig};

const PRELUDE: &str = r#"
var failures = [];
function check(name, ok) { if (!ok) failures.push(name); }
function throwsError(ctor, fn) {
  try { fn(); } catch (e) { return e instanceof ctor; }
  return false;
}
function typeError(fn) { return throwsError(TypeError, fn); }
function rangeError(fn) { return throwsError(RangeError, fn); }
function same(a, b) {
  if (a.length !== b.length) return false;
  for (var i = 0; i < a.length; i++) if (a[i] !== b[i]) return false;
  return true;
}
function bytes(buffer) { return Array.from(new Uint8Array(buffer)); }
function immutableOf(values) {
  var source = new ArrayBuffer(values.length);
  var view = new Uint8Array(source);
  for (var i = 0; i < values.length; i++) view[i] = values[i];
  return source.transferToImmutable();
}
"#;

fn assert_no_failures(body: &str) {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = format!("{PRELUDE}\n{body}\nfailures.join('; ')");
    let program = parse(&source).unwrap();
    let result = vm.execute(&compile(&program).unwrap()).unwrap();
    match result {
        Value::String(names) if names.is_empty() => {}
        Value::String(names) => panic!("failed checks: {}", names.to_utf8().unwrap()),
        other => panic!("script did not finish with the failure list: {other:?}"),
    }
}

// --- ArrayBuffer.prototype.immutable -------------------------------------

#[test]
fn immutable_getter_is_an_accessor_with_the_specified_shape() {
    assert_no_failures(
        r#"
var descriptor = Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'immutable');
check('exists', descriptor !== undefined);
check('accessor', typeof descriptor.get === 'function' && descriptor.set === undefined);
check('enumerable', descriptor.enumerable === false);
check('configurable', descriptor.configurable === true);
check('name', descriptor.get.name === 'get immutable');
check('length', descriptor.get.length === 0);
check('sharedArrayBufferHasNone',
  Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype, 'immutable') === undefined);
"#,
    );
}

#[test]
fn immutable_getter_reports_the_slot_of_the_receiver() {
    assert_no_failures(
        r#"
var ordinary = new ArrayBuffer(4);
check('ordinary', ordinary.immutable === false);
check('resizable', new ArrayBuffer(4, { maxByteLength: 8 }).immutable === false);
var transferred = ordinary.transferToImmutable();
check('transferred', transferred.immutable === true);
check('sliced', new ArrayBuffer(4).sliceToImmutable().immutable === true);
check('detachedSourceIsNotImmutable', ordinary.immutable === false);
var detached = new ArrayBuffer(4);
$262.detachArrayBuffer(detached);
check('detached', detached.immutable === false);
check('subclassInstance', new (class extends ArrayBuffer {})(1).immutable === false);
"#,
    );
}

#[test]
fn immutable_getter_rejects_receivers_without_array_buffer_data() {
    assert_no_failures(
        r#"
var getter = Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'immutable').get;
var bad = [undefined, null, 42, '1', true, Symbol('s'), {}, [], function () {},
  ArrayBuffer.prototype, new Int8Array(8), new DataView(new ArrayBuffer(8)),
  new SharedArrayBuffer(4)];
for (var i = 0; i < bad.length; i++) {
  (function (value, index) {
    check('receiver' + index, typeError(function () { getter.call(value); }));
  })(bad[i], i);
}
check('prototypeAccess', typeError(function () { return ArrayBuffer.prototype.immutable; }));
check('bareCall', typeError(function () { getter(); }));
"#,
    );
}

// --- ArrayBuffer.prototype.transferToImmutable ---------------------------

#[test]
fn transfer_to_immutable_has_the_specified_shape() {
    assert_no_failures(
        r#"
var descriptor = Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'transferToImmutable');
check('exists', descriptor !== undefined);
check('method', typeof descriptor.value === 'function');
check('writable', descriptor.writable === true);
check('enumerable', descriptor.enumerable === false);
check('configurable', descriptor.configurable === true);
check('name', descriptor.value.name === 'transferToImmutable');
check('length', descriptor.value.length === 0);
check('notAConstructor', typeError(function () { new (new ArrayBuffer(1)).transferToImmutable(); }));
check('notReflectConstructible', (function () {
  try { Reflect.construct(function () {}, [], ArrayBuffer.prototype.transferToImmutable); }
  catch (e) { return true; }
  return false;
})());
"#,
    );
}

#[test]
fn transfer_to_immutable_moves_the_contents_and_detaches_the_source() {
    assert_no_failures(
        r#"
var makers = {
  fixed: function () { return new ArrayBuffer(4); },
  resizable: function () { return new ArrayBuffer(4, { maxByteLength: 8 }); },
  shrunk: function () {
    var b = new ArrayBuffer(8, { maxByteLength: 8 });
    b.resize(4);
    return b;
  },
  grown: function () {
    var b = new ArrayBuffer(2, { maxByteLength: 4 });
    b.resize(4);
    return b;
  }
};
var lengths = [undefined, 0, 2, 4, 5, 9];
for (var name in makers) {
  for (var i = 0; i < lengths.length; i++) {
    var label = name + '/' + lengths[i];
    var source = makers[name]();
    var view = new Uint8Array(source);
    for (var k = 0; k < 4; k++) view[k] = k + 1;
    var want = lengths[i] === undefined ? 4 : lengths[i];
    var expected = [];
    for (var k = 0; k < want; k++) expected.push(k < 4 ? k + 1 : 0);
    var dest = source.transferToImmutable(lengths[i]);
    check(label + ' immutable', dest.immutable === true);
    check(label + ' notResizable', dest.resizable === false);
    check(label + ' byteLength', dest.byteLength === want);
    check(label + ' maxByteLength', dest.maxByteLength === want);
    check(label + ' contents', same(bytes(dest), expected));
    check(label + ' sourceDetached', source.byteLength === 0 && view.length === 0);
    check(label + ' resultIsFresh', dest !== source);
  }
}
"#,
    );
}

#[test]
fn transfer_to_immutable_ignores_constructor_and_species() {
    assert_no_failures(
        r#"
var log = [];
var buffer = new ArrayBuffer(2);
Object.defineProperty(buffer, 'constructor', {
  get: function () { log.push('constructor'); return undefined; }
});
var result = buffer.transferToImmutable();
check('unobserved', log.length === 0);
check('intrinsicPrototype', Object.getPrototypeOf(result) === ArrayBuffer.prototype);
class Sub extends ArrayBuffer {}
var sub = new Sub(2).transferToImmutable();
check('subclassResultUsesIntrinsic', Object.getPrototypeOf(sub) === ArrayBuffer.prototype);
"#,
    );
}

#[test]
fn transfer_to_immutable_coerces_new_length_like_to_index() {
    assert_no_failures(
        r#"
var good = [[0, 0], [1, 1], [0.9, 0], [1.9, 1], [-0.9, 0], [-0, 0], [null, 0], [false, 0],
  [true, 1], ['', 0], ['8', 8], ['+9', 9], ['10e0', 10], ['0b1110', 14], ['0xf', 15],
  ['0o20', 16], [NaN, 0], ['7up', 0], ['1_0', 0], [undefined, 8]];
for (var i = 0; i < good.length; i++) {
  var got = new ArrayBuffer(8).transferToImmutable(good[i][0]).byteLength;
  check('good' + i, got === good[i][1]);
}
var bad = [[-1, RangeError], [9007199254740992, RangeError], [Infinity, RangeError],
  [-Infinity, RangeError], [Symbol('1'), TypeError], [1n, TypeError]];
for (var i = 0; i < bad.length; i++) {
  (function (raw, ctor, index) {
    check('bad' + index, throwsError(ctor, function () {
      new ArrayBuffer(8).transferToImmutable(raw);
    }));
  })(bad[i][0], bad[i][1], i);
}
var calls = [];
var buffer = new ArrayBuffer(8);
var length = {
  valueOf: function () { calls.push('valueOf'); return {}; },
  toString: function () { calls.push('toString'); return '3'; }
};
check('toStringFallback', buffer.transferToImmutable(length).byteLength === 3);
check('toPrimitiveOrder', calls.join() === 'valueOf,toString');
"#,
    );
}

#[test]
fn transfer_to_immutable_validates_the_receiver_before_reading_the_argument() {
    assert_no_failures(
        r#"
var calls = [];
var length = { valueOf: function () { calls.push('valueOf'); return 1; } };
var bad = [undefined, null, 42, '1', true, Symbol('1'), 1n, {}, [], function () {},
  ArrayBuffer.prototype, new Int8Array(8), new DataView(new ArrayBuffer(8)),
  new SharedArrayBuffer(4)];
for (var i = 0; i < bad.length; i++) {
  (function (value, index) {
    check('receiver' + index, typeError(function () {
      ArrayBuffer.prototype.transferToImmutable.call(value, length);
    }));
  })(bad[i], i);
}
check('argumentUntouched', calls.length === 0);
"#,
    );
}

#[test]
fn transfer_to_immutable_rejects_detached_and_immutable_sources_after_reading_the_argument() {
    assert_no_failures(
        r#"
var calls = [];
var length = { valueOf: function () { calls.push('valueOf'); return 1; } };
var detached = new ArrayBuffer(8);
$262.detachArrayBuffer(detached);
var immutable = new ArrayBuffer(8).transferToImmutable();
check('detached', typeError(function () { detached.transferToImmutable(length); }));
check('detachedReadArgument', calls.length === 1);
calls = [];
check('immutable', typeError(function () { immutable.transferToImmutable(length); }));
check('immutableReadArgument', calls.length === 1);
check('immutableUnchanged', immutable.byteLength === 8 && immutable.immutable === true);
var becomesDetached = new ArrayBuffer(8);
check('becomesDetached', typeError(function () {
  becomesDetached.transferToImmutable({
    valueOf: function () { $262.detachArrayBuffer(becomesDetached); return 1; }
  });
}));
"#,
    );
}

// --- ArrayBuffer.prototype.sliceToImmutable ------------------------------

#[test]
fn slice_to_immutable_has_the_specified_shape() {
    assert_no_failures(
        r#"
var descriptor = Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'sliceToImmutable');
check('exists', descriptor !== undefined);
check('writable', descriptor.writable === true);
check('enumerable', descriptor.enumerable === false);
check('configurable', descriptor.configurable === true);
check('name', descriptor.value.name === 'sliceToImmutable');
check('length', descriptor.value.length === 2);
check('notAConstructor', typeError(function () { new (new ArrayBuffer(1)).sliceToImmutable(); }));
"#,
    );
}

#[test]
fn slice_to_immutable_copies_the_resolved_range() {
    assert_no_failures(
        r#"
function filled() {
  var b = new ArrayBuffer(8);
  var v = new Uint8Array(b);
  for (var i = 0; i < 8; i++) v[i] = i + 1;
  return b;
}
var cases = [
  [[], [1, 2, 3, 4, 5, 6, 7, 8]],
  [[undefined, undefined], [1, 2, 3, 4, 5, 6, 7, 8]],
  [[2], [3, 4, 5, 6, 7, 8]],
  [[2, 5], [3, 4, 5]],
  [[-3], [6, 7, 8]],
  [[-3, -1], [6, 7]],
  [[5, 2], []],
  [[0.9, 2.9], [1, 2]],
  [[-Infinity, Infinity], [1, 2, 3, 4, 5, 6, 7, 8]],
  [[9007199254740992], []],
  [['1', '3'], [2, 3]],
  [[null, false], []],
  [[NaN, NaN], []],
  [[100, 200], []]
];
for (var i = 0; i < cases.length; i++) {
  var source = filled();
  var dest = source.sliceToImmutable.apply(source, cases[i][0]);
  check('case' + i + ' immutable', dest.immutable === true);
  check('case' + i + ' contents', same(bytes(dest), cases[i][1]));
  check('case' + i + ' sourceUntouched', same(bytes(source), [1, 2, 3, 4, 5, 6, 7, 8]));
  check('case' + i + ' notResizable', dest.resizable === false && dest.maxByteLength === dest.byteLength);
}
"#,
    );
}

#[test]
fn slice_to_immutable_reads_start_then_end_and_propagates_errors() {
    assert_no_failures(
        r#"
var calls = [];
var start = { valueOf: function () { calls.push('start'); return {}; }, toString: function () { calls.push('start.toString'); return '1'; } };
var end = { valueOf: function () { calls.push('end'); return 3; } };
var buffer = new ArrayBuffer(8);
buffer.sliceToImmutable(start, end);
check('order', calls.join() === 'start,start.toString,end');
calls = [];
var thrower = { valueOf: function () { throw new RangeError('boom'); } };
check('startErrorSkipsEnd', rangeError(function () { buffer.sliceToImmutable(thrower, end); }));
check('endNotRead', calls.length === 0);
check('symbol', typeError(function () { buffer.sliceToImmutable(Symbol('1')); }));
check('bigint', typeError(function () { buffer.sliceToImmutable(0, 1n); }));
"#,
    );
}

#[test]
fn slice_to_immutable_validates_the_receiver_before_reading_arguments() {
    assert_no_failures(
        r#"
var calls = [];
var arg = { valueOf: function () { calls.push('valueOf'); return 0; } };
var bad = [undefined, null, 42, '1', true, Symbol('1'), 1n, {}, [], function () {},
  ArrayBuffer.prototype, new Int8Array(8), new DataView(new ArrayBuffer(8)),
  new SharedArrayBuffer(4)];
for (var i = 0; i < bad.length; i++) {
  (function (value, index) {
    check('receiver' + index, typeError(function () {
      ArrayBuffer.prototype.sliceToImmutable.call(value, arg, arg);
    }));
  })(bad[i], i);
}
var detached = new ArrayBuffer(8);
$262.detachArrayBuffer(detached);
check('detachedReceiver', typeError(function () { detached.sliceToImmutable(arg, arg); }));
check('argumentsUntouched', calls.length === 0);
"#,
    );
}

#[test]
fn slice_to_immutable_uses_the_length_from_before_argument_coercion() {
    assert_no_failures(
        r#"
// Growth: the bounds come from the original length (10), not the grown one.
var grows = new ArrayBuffer(10, { maxByteLength: 12 });
var view = new Uint8Array(grows);
for (var i = 0; i < 10; i++) view[i] = i + 1;
var dest = grows.sliceToImmutable(
  { valueOf: function () { grows.resize(11); return -7; } },
  { valueOf: function () { grows.resize(12); return -4; } });
check('growContents', same(bytes(dest), [4, 5, 6]));
check('growSource', grows.byteLength === 12);

// Shrinking to a length that still covers the resolved end succeeds.
var shrinks = new ArrayBuffer(10, { maxByteLength: 10 });
var v2 = new Uint8Array(shrinks);
for (var i = 0; i < 10; i++) v2[i] = i + 1;
var shrunk = shrinks.sliceToImmutable(
  { valueOf: function () { shrinks.resize(9); return -7; } },
  { valueOf: function () { shrinks.resize(8); return -4; } });
check('shrinkContents', same(bytes(shrunk), [4, 5, 6]));
check('shrinkSource', shrinks.byteLength === 8);

// Shrinking below the resolved end is a RangeError.
shrinks.resize(10);
check('shrinkBelowEnd', rangeError(function () {
  shrinks.sliceToImmutable(
    { valueOf: function () { return -7; } },
    { valueOf: function () { shrinks.resize(5); return -4; } });
}));

// Detachment while coercing wins over the final bounds check.
var detaching = new ArrayBuffer(8);
check('becomesDetached', typeError(function () {
  detaching.sliceToImmutable(0, { valueOf: function () { $262.detachArrayBuffer(detaching); return 1; } });
}));
"#,
    );
}

#[test]
fn slice_to_immutable_result_is_a_snapshot_independent_of_the_source() {
    assert_no_failures(
        r#"
var source = new ArrayBuffer(8, { maxByteLength: 8 });
var view = new Uint8Array(source);
for (var i = 0; i < 8; i++) view[i] = i + 1;
var dest = source.sliceToImmutable();
var expected = [1, 2, 3, 4, 5, 6, 7, 8];
var destView = new Uint8Array(dest);
view[0] = 86;
check('afterOverwrite', same(destView, expected));
source.resize(4);
check('afterResize', same(destView, expected));
$262.detachArrayBuffer(source);
check('afterDetach', same(destView, expected) && same(bytes(dest), expected));
"#,
    );
}

#[test]
fn slice_to_immutable_ignores_species_and_works_on_immutable_sources() {
    assert_no_failures(
        r#"
var log = [];
var buffer = new ArrayBuffer(4);
Object.defineProperty(buffer, 'constructor', { get: function () { log.push('constructor'); } });
var out = buffer.sliceToImmutable();
check('speciesUnobserved', log.length === 0);
check('intrinsicPrototype', Object.getPrototypeOf(out) === ArrayBuffer.prototype);
var again = immutableOf([1, 2, 3, 4]).sliceToImmutable(1, 3);
check('fromImmutable', again.immutable === true && same(bytes(again), [2, 3]));
"#,
    );
}

// --- Operations that must refuse an immutable ArrayBuffer ----------------

#[test]
fn resize_rejects_an_immutable_buffer_before_reading_its_argument() {
    assert_no_failures(
        r#"
var calls = [];
var iab = new ArrayBuffer(4).transferToImmutable();
check('plain', typeError(function () { iab.resize(0); }));
check('coercedNotRead', typeError(function () {
  iab.resize({ valueOf: function () { calls.push('valueOf'); return 0; } });
}));
check('argumentUntouched', calls.length === 0);
check('unchanged', iab.byteLength === 4 && iab.resizable === false);
"#,
    );
}

#[test]
fn transfer_and_transfer_to_fixed_length_reject_an_immutable_source_after_the_argument() {
    assert_no_failures(
        r#"
var iab = new ArrayBuffer(4).transferToImmutable();
['transfer', 'transferToFixedLength'].forEach(function (name) {
  var calls = [];
  check(name + ' plain', typeError(function () { iab[name](); }));
  check(name + ' withLength', typeError(function () {
    iab[name]({ valueOf: function () { calls.push('valueOf'); return 1; } });
  }));
  check(name + ' argumentRead', calls.length === 1);
  check(name + ' unchanged', iab.byteLength === 4 && iab.immutable === true);
});
"#,
    );
}

#[test]
fn transfer_reads_its_argument_before_rejecting_a_detached_source() {
    assert_no_failures(
        r#"
var calls = [];
var detached = new ArrayBuffer(4);
$262.detachArrayBuffer(detached);
check('transfer', typeError(function () {
  detached.transfer({ valueOf: function () { calls.push('valueOf'); return 1; } });
}));
check('argumentRead', calls.length === 1);
"#,
    );
}

#[test]
fn slice_rejects_a_species_result_that_is_immutable() {
    assert_no_failures(
        r#"
var calls = [];
var arrayBuffer = new ArrayBuffer(8);
var species = {};
species[Symbol.species] = function (length) {
  calls.push('species(' + length + ')');
  return arrayBuffer.sliceToImmutable();
};
arrayBuffer.constructor = species;
check('default', typeError(function () { arrayBuffer.slice(); }));
check('defaultCalls', calls.join() === 'species(8)');
calls = [];
check('withArguments', typeError(function () {
  arrayBuffer.slice({ valueOf: function () { calls.push('start'); return 1; } },
                    { valueOf: function () { calls.push('end'); return 2; } });
}));
check('argumentsBeforeSpecies', calls.join() === 'start,end,species(1)');
"#,
    );
}

#[test]
fn slice_of_an_immutable_buffer_yields_a_mutable_copy() {
    assert_no_failures(
        r#"
var iab = immutableOf([1, 2, 3, 4]);
var copy = iab.slice(1, 3);
check('mutableResult', copy.immutable === false);
check('contents', same(bytes(copy), [2, 3]));
new Uint8Array(copy)[0] = 9;
check('writable', bytes(copy)[0] === 9);
check('sourceIntact', same(bytes(iab), [1, 2, 3, 4]));
"#,
    );
}

#[test]
fn an_immutable_buffer_cannot_be_detached() {
    assert_no_failures(
        r#"
var iab = immutableOf([1, 2, 3, 4]);
var view = new Uint8Array(iab);
check('hostDetachThrows', typeError(function () { $262.detachArrayBuffer(iab); }));
check('stillFull', iab.byteLength === 4 && view.length === 4 && same(view, [1, 2, 3, 4]));
check('stillImmutable', iab.immutable === true);
"#,
    );
}

#[test]
fn views_over_an_immutable_buffer_can_still_read() {
    assert_no_failures(
        r#"
var iab = immutableOf([1, 2, 3, 4]);
var typed = new Uint8Array(iab);
var wide = new Uint16Array(iab);
var view = new DataView(iab);
check('typedRead', same(typed, [1, 2, 3, 4]));
check('wideRead', wide.length === 2 && wide[0] === 0x0201);
check('dataViewRead', view.getUint8(3) === 4 && view.getUint16(0, true) === 0x0201);
check('subarray', same(typed.subarray(1, 3), [2, 3]));
check('slice', same(typed.slice(1, 3), [2, 3]) && typed.slice(1, 3).buffer.immutable === false);
check('iteration', Array.from(typed).join() === '1,2,3,4');
check('map', same(typed.map(function (x) { return x * 2; }), [2, 4, 6, 8]));
check('filter', same(typed.filter(function (x) { return x > 2; }), [3, 4]));
check('toReversed', same(typed.toReversed(), [4, 3, 2, 1]));
check('toSorted', same(typed.toSorted(), [1, 2, 3, 4]));
check('with', same(typed.with(0, 9), [9, 2, 3, 4]));
check('includes', typed.includes(3) && typed.indexOf(3) === 2);
check('atomicsLoad', Atomics.load(typed, 1) === 2);
check('atomicsNotify', Atomics.notify(new Int32Array(new ArrayBuffer(8).transferToImmutable()), 0) === 0);
check('zeroLength', new Uint8Array(new ArrayBuffer(0).transferToImmutable()).length === 0);
"#,
    );
}

#[test]
fn a_failing_check_is_reported_by_name() {
    // The harness itself: a failing `check` must be reported, not swallowed.
    let result = std::panic::catch_unwind(|| assert_no_failures("check('deliberate', false);"));
    let message = *result.unwrap_err().downcast::<String>().unwrap();
    assert!(message.contains("deliberate"), "{message}");
}

// --- Allocation, collection and resource limits ---------------------------

#[test]
fn immutable_buffers_survive_a_collection_on_every_allocation() {
    let mut vm = Vm::new(VmConfig {
        heap: HeapConfig {
            nursery_capacity: 1,
            major_threshold_bytes: 256,
            max_heap_bytes: 1024 * 1024,
        },
        ..Default::default()
    })
    .unwrap();
    // Each iteration allocates a source, a view, an immutable copy made by
    // either transfer or slice, and a view over it, so with a one-object
    // nursery every native allocates between collections.
    let source = r#"
var views = [];
for (var i = 0; i < 24; i++) {
  var buffer = new ArrayBuffer(8);
  var writer = new Uint8Array(buffer);
  for (var k = 0; k < 8; k++) writer[k] = i + k;
  var immutable = i % 2 ? buffer.transferToImmutable(4 + i) : buffer.sliceToImmutable(1, 7);
  views.push(new Uint8Array(immutable));
  var churn = [{}, {}, []];
}
var ok = true;
for (var i = 0; i < 24; i++) {
  var view = views[i];
  var want = [];
  if (i % 2) {
    for (var k = 0; k < 4 + i; k++) want.push(k < 8 ? i + k : 0);
  } else {
    for (var k = 1; k < 7; k++) want.push(i + k);
  }
  ok = ok && view.buffer.immutable === true && view.join() === want.join();
}
ok
"#;
    assert_eq!(
        vm.execute(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn transfer_to_immutable_refuses_an_impossible_length_and_keeps_its_source() {
    assert_no_failures(
        r#"
var source = new ArrayBuffer(4);
new Uint8Array(source).fill(7);
check('tooLarge', rangeError(function () { source.transferToImmutable(Math.pow(2, 40)); }));
check('sourceAttached', source.byteLength === 4 && bytes(source).join() === '7,7,7,7');
check('stillTransferable', source.transferToImmutable().byteLength === 4);
"#,
    );
}
