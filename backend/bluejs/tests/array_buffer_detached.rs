// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `ArrayBuffer.prototype.detached` (ES2024, `arraybuffer-transfer`) and its
//! interaction with the other attachment-aware accessors.

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm, VmConfig};

/// Runs `source` with the Test262 host installed, so `$262.detachArrayBuffer`
/// is available alongside the language-level `transfer` detach path. A
/// `nursery_capacity` of 1 collects on nearly every allocation, so an object
/// a native leaves unrooted across a later allocation fails deterministically.
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

#[test]
fn detached_reports_attachment_for_fixed_and_resizable_buffers() {
    assert_no_failures(
        r#"
        const failures = [];
        function check(label, actual, expected) {
          if (actual !== expected) failures.push(label + ': ' + String(actual));
        }
        const fixed = new ArrayBuffer(1);
        check('fixed fresh', fixed.detached, false);
        $262.detachArrayBuffer(fixed);
        check('fixed after host detach', fixed.detached, true);

        for (const [length, maximum] of [[0, 0], [0, 23], [42, 42]]) {
          const resizable = new ArrayBuffer(length, { maxByteLength: maximum });
          check('resizable ' + length + '/' + maximum + ' fresh', resizable.detached, false);
          $262.detachArrayBuffer(resizable);
          check('resizable ' + length + '/' + maximum + ' detached', resizable.detached, true);
        }

        // transfer() detaches its source without any host hook, and the
        // transferred-to buffer is a fresh, attached one.
        const grown = new ArrayBuffer(1, { maxByteLength: 4 });
        grown.resize(4);
        check('resizing does not detach', grown.detached, false);
        const source = new ArrayBuffer(4);
        const moved = source.transfer();
        check('transfer source', source.detached, true);
        check('transfer target', moved.detached, false);
        const fixedMoved = moved.transferToFixedLength();
        check('transferToFixedLength source', moved.detached, true);
        check('transferToFixedLength target', fixedMoved.detached, false);
        failures.join('; ');
        "#,
    );
}

#[test]
fn detached_is_a_length_zero_accessor_on_the_prototype() {
    assert_no_failures(
        r#"
        const failures = [];
        function check(label, actual, expected) {
          if (actual !== expected) failures.push(label + ': ' + String(actual));
        }
        const descriptor = Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'detached');
        check('is accessor', typeof descriptor.get, 'function');
        check('no setter', descriptor.set, undefined);
        check('enumerable', descriptor.enumerable, false);
        check('configurable', descriptor.configurable, true);
        check('getter name', descriptor.get.name, 'get detached');
        check('getter length', descriptor.get.length, 0);
        check('getter is not a constructor', (() => {
          try { new descriptor.get(); return 'constructed'; } catch (error) { return error instanceof TypeError; }
        })(), true);
        check('not on SharedArrayBuffer', Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype, 'detached'), undefined);
        // Reading it off the prototype itself has no [[ArrayBufferData]] to inspect.
        check('read through the prototype', (() => {
          try { return ArrayBuffer.prototype.detached; } catch (error) { return error instanceof TypeError; }
        })(), true);
        failures.join('; ');
        "#,
    );
}

#[test]
fn detached_requires_a_non_shared_array_buffer_receiver() {
    assert_no_failures(
        r#"
        const failures = [];
        const getter = Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'detached').get;
        function rejects(label, receiver) {
          try {
            getter.call(receiver);
            failures.push(label + ': did not throw');
          } catch (error) {
            if (!(error instanceof TypeError)) failures.push(label + ': ' + error);
          }
        }
        rejects('undefined', undefined);
        rejects('null', null);
        rejects('number', 1);
        rejects('string', 'abc');
        rejects('symbol', Symbol());
        rejects('plain object', {});
        rejects('array', []);
        rejects('typed array', new Int8Array(8));
        rejects('data view', new DataView(new ArrayBuffer(8), 0));
        rejects('shared', new SharedArrayBuffer(4));
        rejects('growable shared', new SharedArrayBuffer(4, { maxByteLength: 8 }));
        failures.join('; ');
        "#,
    );
}

#[test]
fn detached_is_visible_through_a_views_buffer_accessor() {
    assert_no_failures(
        r#"
        const failures = [];
        function check(label, actual, expected) {
          if (actual !== expected) failures.push(label + ': ' + String(actual));
        }
        const typed = new Uint8Array(new ArrayBuffer(8, { maxByteLength: 16 }));
        const buffer = typed.buffer;
        check('view buffer attached', typed.buffer.detached, false);
        $262.detachArrayBuffer(buffer);
        check('view buffer detached', typed.buffer.detached, true);
        check('same buffer object', typed.buffer, buffer);
        failures.join('; ');
        "#,
    );
}

#[test]
fn resizable_is_unaffected_by_attachment() {
    assert_no_failures(
        r#"
        const failures = [];
        function check(label, actual, expected) {
          if (actual !== expected) failures.push(label + ': ' + String(actual));
        }
        const fixed = new ArrayBuffer(1);
        $262.detachArrayBuffer(fixed);
        check('detached fixed', fixed.resizable, false);
        const resizable = new ArrayBuffer(1, { maxByteLength: 1 });
        $262.detachArrayBuffer(resizable);
        check('detached resizable', resizable.resizable, true);
        const moved = new ArrayBuffer(2, { maxByteLength: 4 });
        moved.transfer();
        check('transferred-from resizable', moved.resizable, true);
        check('transferred-from resizable is detached', moved.detached, true);
        // The unrelated attachment-aware accessors stay in step with it.
        check('detached maxByteLength', moved.maxByteLength, 0);
        check('detached byteLength', moved.byteLength, 0);
        failures.join('; ');
        "#,
    );
}

#[test]
fn resizable_still_rejects_shared_and_non_buffer_receivers() {
    assert_no_failures(
        r#"
        const failures = [];
        const getter = Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'resizable').get;
        for (const [label, receiver] of [
          ['shared', new SharedArrayBuffer(1)],
          ['object', {}],
          ['undefined', undefined],
        ]) {
          try {
            getter.call(receiver);
            failures.push(label + ': did not throw');
          } catch (error) {
            if (!(error instanceof TypeError)) failures.push(label + ': ' + error);
          }
        }
        failures.join('; ');
        "#,
    );
}
