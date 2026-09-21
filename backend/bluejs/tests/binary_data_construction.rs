// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Constructor step ordering and cross-realm behavior of the binary-data
//! built-ins: `DataView`, the `TypedArray` constructors and `Atomics`.

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm, VmConfig};

/// Runs `source` with the Test262 host installed (`$262.createRealm`,
/// `$262.detachArrayBuffer`). A `nursery_capacity` of 1 collects on nearly
/// every allocation, so an object a native leaves unrooted across a later
/// allocation fails deterministically.
fn evaluate(source: &str, nursery_capacity: Option<usize>) -> Result<Value, RuntimeError> {
    let mut config = VmConfig::default();
    if let Some(capacity) = nursery_capacity {
        config.heap.nursery_capacity = capacity;
    }
    let mut vm = Vm::new(config).unwrap();
    vm.install_test262_harness().unwrap();
    vm.execute(&compile(&parse(source).unwrap()).unwrap())
}

/// Each script collects one line per violated expectation; an empty result
/// means every check held, and a failing run names the exact checks. It must
/// hold both on the default heap and under collection stress.
fn assert_no_failures(source: &str) {
    for nursery_capacity in [None, Some(1)] {
        match evaluate(source, nursery_capacity).unwrap() {
            Value::String(failures) => assert_eq!(
                failures.to_utf8().unwrap(),
                "",
                "violated expectations (nursery capacity {nursery_capacity:?})"
            ),
            other => panic!("script did not return its failure list: {other:?}"),
        }
    }
}

const PRELUDE: &str = r#"
const failures = [];
function check(label, actual, expected) {
  if (actual !== expected) failures.push(label + ': ' + String(actual));
}
function errorName(action) {
  try {
    action();
    return 'no error';
  } catch (error) {
    return error instanceof TypeError ? 'TypeError'
      : error instanceof RangeError ? 'RangeError'
      : 'other: ' + String(error);
  }
}
"#;

fn run(body: &str) {
    assert_no_failures(&format!("{PRELUDE}{body}\nfailures.join('; ');"));
}

#[test]
fn data_view_rereads_the_buffer_length_after_reading_the_new_target_prototype() {
    run(r#"
        // A resizable buffer shrunk to exactly the view's offset by the
        // `prototype` getter still admits a (now empty) length-tracking view.
        {
          const buffer = new ArrayBuffer(3, { maxByteLength: 3 });
          const newTarget = function () {}.bind(null);
          Object.defineProperty(newTarget, 'prototype', {
            get() { buffer.resize(2); },
          });
          const view = Reflect.construct(DataView, [buffer, 2], newTarget);
          check('shrunk to offset: constructor', view.constructor, DataView);
          check('shrunk to offset: byteLength', view.byteLength, 0);
          buffer.resize(3);
          check('length tracking follows a later grow', view.byteLength, 1);
        }
        // A buffer that grows during the lookup is tracked at its new length.
        {
          const buffer = new ArrayBuffer(2, { maxByteLength: 6 });
          const newTarget = function () {}.bind(null);
          Object.defineProperty(newTarget, 'prototype', {
            get() { buffer.resize(6); },
          });
          const view = Reflect.construct(DataView, [buffer, 1], newTarget);
          check('grown: byteLength', view.byteLength, 5);
        }
        // Shrinking below the offset invalidates even a length-tracking view.
        {
          const buffer = new ArrayBuffer(3, { maxByteLength: 3 });
          const newTarget = function () {}.bind(null);
          Object.defineProperty(newTarget, 'prototype', {
            get() { buffer.resize(1); },
          });
          check('shrunk below offset', errorName(() => Reflect.construct(DataView, [buffer, 2], newTarget)), 'RangeError');
        }
        // An explicit byteLength that no longer fits is rejected too, and one
        // that still fits is kept as given.
        {
          const buffer = new ArrayBuffer(4, { maxByteLength: 4 });
          const newTarget = function () {}.bind(null);
          Object.defineProperty(newTarget, 'prototype', {
            get() { buffer.resize(2); },
          });
          check('shrunk below length', errorName(() => Reflect.construct(DataView, [buffer, 1, 3], newTarget)), 'RangeError');
          const view = Reflect.construct(DataView, [buffer, 1, 1], newTarget);
          check('shrunk but still fits', view.byteLength, 1);
        }
        // Detaching during the lookup is a TypeError.
        {
          const buffer = new ArrayBuffer(4);
          const newTarget = function () {}.bind(null);
          Object.defineProperty(newTarget, 'prototype', {
            get() { $262.detachArrayBuffer(buffer); },
          });
          check('detached during lookup', errorName(() => Reflect.construct(DataView, [buffer, 0], newTarget)), 'TypeError');
        }
        // A growable SharedArrayBuffer only grows, and a length-tracking view sees that.
        {
          const buffer = new SharedArrayBuffer(2, { maxByteLength: 6 });
          const newTarget = function () {}.bind(null);
          Object.defineProperty(newTarget, 'prototype', {
            get() { buffer.grow(6); },
          });
          const view = Reflect.construct(DataView, [buffer, 1], newTarget);
          check('shared grown: byteLength', view.byteLength, 5);
        }
        // Arguments that are already invalid are rejected before the prototype is read.
        {
          const log = [];
          const newTarget = function () {}.bind(null);
          Object.defineProperty(newTarget, 'prototype', {
            get() { log.push('prototype'); },
          });
          const detached = new ArrayBuffer(2);
          $262.detachArrayBuffer(detached);
          check('offset past the end', errorName(() => Reflect.construct(DataView, [new ArrayBuffer(2), 3], newTarget)), 'RangeError');
          check('length past the end', errorName(() => Reflect.construct(DataView, [new ArrayBuffer(2), 1, 2], newTarget)), 'RangeError');
          check('detached buffer', errorName(() => Reflect.construct(DataView, [detached, 0], newTarget)), 'TypeError');
          check('not a buffer', errorName(() => Reflect.construct(DataView, [{}, 0], newTarget)), 'TypeError');
          check('no prototype read for any of them', log.join(), '');
        }
    "#);
}

#[test]
fn data_view_falls_back_to_the_prototype_of_the_new_targets_realm() {
    run(r#"
        const other = $262.createRealm().global;
        const C = new other.Function();
        C.prototype = null;

        const local = Reflect.construct(DataView, [new ArrayBuffer(0), 0], C);
        check('ArrayBuffer', Object.getPrototypeOf(local), other.DataView.prototype);
        const shared = Reflect.construct(DataView, [new SharedArrayBuffer(0), 0], C);
        check('SharedArrayBuffer', Object.getPrototypeOf(shared), other.DataView.prototype);
        check('not this realm', Object.getPrototypeOf(local) === DataView.prototype, false);

        // A same-realm new target keeps using this realm's intrinsic, and an
        // object-valued `prototype` always wins.
        const D = function () {}.bind(null);
        D.prototype = undefined;
        check('same-realm fallback', Object.getPrototypeOf(Reflect.construct(DataView, [new ArrayBuffer(0)], D)), DataView.prototype);
        const custom = {};
        C.prototype = custom;
        check('object prototype wins', Object.getPrototypeOf(Reflect.construct(DataView, [new ArrayBuffer(0)], C)), custom);
    "#);
}

#[test]
fn typed_array_constructors_convert_a_primitive_length_before_reading_the_new_target_prototype() {
    run(r#"
        function newTargetLogging(log) {
          const newTarget = function () {}.bind(null);
          Object.defineProperty(newTarget, 'prototype', {
            get() { log.push('prototype'); throw new EvalError('prototype read'); },
          });
          return newTarget;
        }
        for (const TA of [Int8Array, Uint8Array, Float64Array, BigInt64Array]) {
          for (const [label, argument] of [['symbol', Symbol()], ['bigint', 1n]]) {
            const log = [];
            check(TA.name + ' ' + label + ' throws TypeError',
              errorName(() => Reflect.construct(TA, [argument], newTargetLogging(log))), 'TypeError');
            check(TA.name + ' ' + label + ' skips the prototype read', log.join(), '');
          }
          // ToIndex rejections are also observed first.
          for (const [label, argument] of [['negative', -1], ['infinite', Infinity]]) {
            const log = [];
            check(TA.name + ' ' + label + ' throws RangeError',
              errorName(() => Reflect.construct(TA, [argument], newTargetLogging(log))), 'RangeError');
            check(TA.name + ' ' + label + ' skips the prototype read', log.join(), '');
          }
        }

        // A primitive that converts to an index succeeds, and only then is the prototype read.
        for (const [label, argument] of [['string', '2'], ['null', null], ['boolean', true], ['fraction', 1.7]]) {
          const log = [];
          let error;
          try { Reflect.construct(Uint8Array, [argument], newTargetLogging(log)); } catch (e) { error = e; }
          check(label + ' length reaches the prototype read', log.join(), 'prototype');
          check(label + ' length surfaces the getter error', error instanceof EvalError, true);
        }
        // An allocation failure comes after the prototype read.
        {
          const log = [];
          let error;
          try { Reflect.construct(Uint8Array, [2 ** 52], newTargetLogging(log)); } catch (e) { error = e; }
          check('oversized length reads the prototype first', log.join(), 'prototype');
          check('oversized length surfaces the getter error', error instanceof EvalError, true);
        }
        // Object and absent first arguments read the prototype before anything else.
        for (const [label, args] of [['none', []], ['object', [{ valueOf() { throw new Error('converted'); } }]], ['array', [[1]]]]) {
          const log = [];
          let error;
          try { Reflect.construct(Uint8Array, args, newTargetLogging(log)); } catch (e) { error = e; }
          check(label + ' reads the prototype first', log.join(), 'prototype');
          check(label + ' surfaces the getter error', error instanceof EvalError, true);
        }
        // The undefined length is 0 and the prototype is used.
        {
          // Inherit from %TypedArray%.prototype so the `length` accessor stays reachable.
          const custom = Object.create(Object.getPrototypeOf(Uint8Array.prototype));
          const newTarget = function () {}.bind(null);
          newTarget.prototype = custom;
          const view = Reflect.construct(Uint8Array, [undefined], newTarget);
          check('undefined length', view.length, 0);
          check('undefined length prototype', Object.getPrototypeOf(view), custom);
          check('numeric length prototype', Object.getPrototypeOf(Reflect.construct(Int16Array, [3], newTarget)), custom);
        }
    "#);
}

#[test]
fn atomics_accept_integer_typed_arrays_from_another_realm() {
    run(r#"
        const other = $262.createRealm().global;
        const integerConstructors = [
          other.Int32Array, other.Int16Array, other.Int8Array,
          other.Uint32Array, other.Uint16Array, other.Uint8Array,
        ];
        function fresh(TA, shared) {
          return new TA(shared ? new other.SharedArrayBuffer(4) : new other.ArrayBuffer(4));
        }
        for (const shared of [true, false]) {
          for (const TA of integerConstructors) {
            const name = TA.name + (shared ? ' shared' : ' plain');
            let ta = fresh(TA, shared);
            ta[0] = 1;
            check(name + ' load', Atomics.load(ta, 0), 1);
            ta = fresh(TA, shared);
            check(name + ' store result', Atomics.store(ta, 0, 1), 1);
            check(name + ' store', ta[0], 1);
            ta = fresh(TA, shared);
            ta[0] = 1;
            check(name + ' compareExchange result', Atomics.compareExchange(ta, 0, 1, 2), 1);
            check(name + ' compareExchange', ta[0], 2);
            check(name + ' compareExchange mismatch', Atomics.compareExchange(ta, 0, 1, 9), 2);
            check(name + ' compareExchange mismatch keeps', ta[0], 2);
            ta = fresh(TA, shared);
            ta[0] = 1;
            check(name + ' exchange result', Atomics.exchange(ta, 0, 2), 1);
            check(name + ' exchange', ta[0], 2);
            ta = fresh(TA, shared);
            ta[0] = 1;
            check(name + ' add result', Atomics.add(ta, 0, 2), 1);
            check(name + ' add', ta[0], 3);
            ta = fresh(TA, shared);
            ta[0] = 3;
            check(name + ' sub result', Atomics.sub(ta, 0, 2), 3);
            check(name + ' sub', ta[0], 1);
            ta = fresh(TA, shared);
            ta[0] = 3;
            check(name + ' and result', Atomics.and(ta, 0, 1), 3);
            check(name + ' and', ta[0], 1);
            ta = fresh(TA, shared);
            ta[0] = 2;
            check(name + ' or result', Atomics.or(ta, 0, 1), 2);
            check(name + ' or', ta[0], 3);
            ta = fresh(TA, shared);
            ta[0] = 3;
            check(name + ' xor result', Atomics.xor(ta, 0, 1), 3);
            check(name + ' xor', ta[0], 2);
          }
        }
        // BigInt element kinds cross the boundary too.
        {
          const big = new other.BigInt64Array(new other.SharedArrayBuffer(16));
          big[0] = 5n;
          check('bigint add result', Atomics.add(big, 0, 2n), 5n);
          check('bigint add', big[0], 7n);
        }
        // Atomics.notify and Atomics.wait see the foreign shared buffer.
        {
          const ta = new other.Int32Array(new other.SharedArrayBuffer(8));
          check('notify counts no waiters', Atomics.notify(ta, 0, 1), 0);
          check('wait not-equal', Atomics.wait(ta, 0, 1, 0), 'not-equal');
          check('wait timed-out', Atomics.wait(ta, 0, 0, 0), 'timed-out');
          const asyncResult = Atomics.waitAsync(ta, 0, 1);
          check('waitAsync not-equal', asyncResult.async === false && asyncResult.value === 'not-equal', true);
          const plain = new other.Int32Array(4);
          check('notify on a non-shared foreign array', Atomics.notify(plain, 0, 1), 0);
          check('wait on a non-shared foreign array', errorName(() => Atomics.wait(plain, 0, 0, 0)), 'TypeError');
        }
    "#);
}

#[test]
fn atomics_on_a_foreign_array_coerce_object_arguments_in_the_calling_realm() {
    run(r#"
        const other = $262.createRealm().global;
        const ta = new other.Int32Array(new other.SharedArrayBuffer(8));
        const log = [];
        function hook(name, result) {
          return { valueOf() { log.push(name); return result; } };
        }
        check('store result', Atomics.store(ta, hook('index', 1), hook('value', 7)), 7);
        check('store landed', ta[1], 7);
        check('store order', log.join(), 'index,value');

        log.length = 0;
        check('compareExchange result',
          Atomics.compareExchange(ta, hook('index', 1), hook('expected', 7), hook('replacement', 9)), 7);
        check('compareExchange landed', ta[1], 9);
        check('compareExchange order', log.join(), 'index,expected,replacement');

        const big = new other.BigInt64Array(new other.SharedArrayBuffer(16));
        check('bigint add result', Atomics.add(big, 0, hook('bigint', 2n)), 0n);
        check('bigint add landed', big[0], 2n);

        log.length = 0;
        check('wait not-equal', Atomics.wait(ta, hook('index', 1), hook('expected', 0), hook('timeout', 0)), 'not-equal');
        check('wait order', log.join(), 'index,expected,timeout');
        check('notify count', Atomics.notify(ta, hook('index', 1), hook('count', 1)), 0);

        // A hook's exception is this realm's own thrown value, unchanged.
        const marker = {};
        let thrown;
        try { Atomics.add(ta, 0, { valueOf() { throw marker; } }); } catch (error) { thrown = error; }
        check('exception identity', thrown, marker);
        check('exception left the array alone', ta[0], 0);
        check('symbol value', errorName(() => Atomics.add(ta, 0, Symbol())), 'TypeError');
        check('bigint value into a number array', errorName(() => Atomics.add(ta, 0, 1n)), 'TypeError');
    "#);
}

#[test]
fn atomics_reject_invalid_foreign_receivers_with_errors_from_the_calling_realm() {
    run(r#"
        const other = $262.createRealm().global;
        const ta = new other.Int32Array(new other.SharedArrayBuffer(8));
        check('index out of range', errorName(() => Atomics.load(ta, 2)), 'RangeError');
        check('negative index', errorName(() => Atomics.add(ta, -1, 0)), 'RangeError');
        check('float array', errorName(() => Atomics.load(new other.Float64Array(1), 0)), 'TypeError');
        check('clamped array', errorName(() => Atomics.load(new other.Uint8ClampedArray(1), 0)), 'TypeError');
        check('foreign plain object', errorName(() => Atomics.load(new other.Object(), 0)), 'TypeError');
        check('foreign array', errorName(() => Atomics.load(new other.Array(1), 0)), 'TypeError');
        check('foreign wait on a non-waitable kind', errorName(() => Atomics.wait(new other.Int8Array(new other.SharedArrayBuffer(4)), 0, 0, 0)), 'TypeError');
        // A detached foreign buffer is rejected while validating the array.
        const detachable = new other.Int8Array(4);
        other.$262.detachArrayBuffer(detachable.buffer);
        check('detached foreign buffer', errorName(() => Atomics.load(detachable, 0)), 'TypeError');
        // Same-realm behavior is unchanged.
        check('same-realm load', Atomics.load(new Int32Array(1), 0), 0);
        check('same-realm float array', errorName(() => Atomics.load(new Float64Array(1), 0)), 'TypeError');
    "#);
}
