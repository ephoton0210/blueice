// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Built-in functions of one `$262.createRealm()` realm that are generic over
//! their receiver (the Array methods, the Iterator helpers, the
//! `Error.prototype.stack` accessors) applied to objects of another realm:
//! the algorithm runs on the real operands, while the objects and errors it
//! creates belong to the realm of the function, and errors raised by
//! callbacks belong to the callback's realm.

use blueice_bluejs::{compile, parse, Value, Vm, VmConfig};

fn evaluate(source: &str, nursery_capacity: Option<usize>) -> Value {
    let mut config = VmConfig::default();
    if let Some(capacity) = nursery_capacity {
        config.heap.nursery_capacity = capacity;
    }
    let mut vm = Vm::new(config).unwrap();
    vm.install_test262_harness().unwrap();
    vm.execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap()
}

/// Runs `source` (which evaluates to a `;`-joined failure list) with and
/// without a one-object nursery, and requires no failures.
fn assert_no_failures(source: &str) {
    for nursery_capacity in [None, Some(1)] {
        let Value::String(failures) = evaluate(source, nursery_capacity) else {
            panic!("script did not return its failure list");
        };
        assert_eq!(
            failures.to_utf8().unwrap(),
            "",
            "violated expectations (nursery capacity {nursery_capacity:?})"
        );
    }
}

const HELPERS: &str = r#"
    const other = $262.createRealm().global;
    const failures = [];
    function check(label, condition) {
        if (!condition) failures.push(label);
    }
    function expectThrown(label, expected, action) {
        try {
            action();
        } catch (error) {
            check(label + ': wrong realm or type', Object.getPrototypeOf(error) === expected.prototype);
            return;
        }
        failures.push(label + ': no error');
    }
"#;

#[test]
fn a_foreign_array_method_operates_on_the_local_receiver() {
    let source = format!(
        r#"{HELPERS}
        const local = [3, 1, 2];
        check('push result', other.Array.prototype.push.call(local, 4) === 4);
        check('push mutates the receiver', local.length === 4 && local[3] === 4);
        other.Array.prototype.sort.call(local, (a, b) => a - b);
        check('sort mutates the receiver', local.join() === '1,2,3,4');
        check('sort returns the receiver', other.Array.prototype.reverse.call(local) === local);
        check('reverse', local.join() === '4,3,2,1');
        check('includes', other.Array.prototype.includes.call(local, 3) === true);
        check('indexOf on an array-like',
            other.Array.prototype.indexOf.call({{ length: 2, 0: 'a', 1: 'b' }}, 'b') === 1);
        // Callbacks run in the caller's realm with the caller's values.
        let seen = 0;
        other.Array.prototype.forEach.call([1, 2], function (value) {{ seen += value; }});
        check('callback ran', seen === 3);
        failures.join('; ')
        "#
    );
    assert_no_failures(&source);
}

#[test]
fn errors_come_from_the_realm_of_the_foreign_function() {
    let source = format!(
        r#"{HELPERS}
        const arrayLike = {{
            get '0'() {{ throw new Error('Get 0'); }},
            length: 2 ** 32,
        }};
        const toSorted = other.Array.prototype.toSorted;
        const toSpliced = other.Array.prototype.toSpliced;
        expectThrown('bad comparator', other.TypeError, () => toSorted.call([], 5));
        expectThrown('null receiver', other.TypeError, () => toSorted.call(null));
        expectThrown('array too long', other.RangeError, () => toSorted.call(arrayLike));
        expectThrown('with out of range', other.RangeError, () => other.Array.prototype.with.call([0, 1, 2], 3, 7));
        arrayLike.length = 2 ** 53 - 1;
        expectThrown('toSpliced too long', other.TypeError, () => toSpliced.call(arrayLike, 0, 0, 1));
        // A local iterator consumed by a foreign method.
        expectThrown('iterator find without predicate', other.TypeError,
            () => other.Iterator.prototype.find.call([].values()));

        // Errors raised by callbacks stay in the callback's realm.
        expectThrown('callback body error', TypeError,
            () => other.Array.prototype.forEach.call([1], () => {{ null.property; }}));
        expectThrown('callback explicit error', RangeError,
            () => other.Array.prototype.map.call([1], () => {{ throw new RangeError(); }}));
        failures.join('; ')
        "#
    );
    assert_no_failures(&source);
}

#[test]
fn fresh_arrays_belong_to_the_realm_of_the_foreign_function() {
    let source = format!(
        r#"{HELPERS}
        const results = [
            ['with', other.Array.prototype.with.call([1, 2, 3], 1, 3)],
            ['toSpliced', other.Array.prototype.toSpliced.call([1, 2, 3], 0, 1, 4, 5)],
            ['toReversed', other.Array.prototype.toReversed.call([1, 2, 3])],
            ['toSorted', other.Array.prototype.toSorted.call([1, 2, 3])],
            ['toArray', other.Iterator.prototype.toArray.call([1, 2, 3].values())],
        ];
        for (const [name, array] of results) {{
            check(name + ' is an Array', Array.isArray(array) && array.length >= 3);
            check(name + ' is not local', !(array instanceof Array));
            check(name + ' is foreign', array instanceof other.Array);
        }}
        check('toArray contents', results[4][1].join() === '1,2,3');
        // The local realm's own methods are unaffected.
        check('local toArray', [1, 2, 3].values().toArray() instanceof Array);
        failures.join('; ')
        "#
    );
    assert_no_failures(&source);
}

#[test]
fn array_from_of_another_realm_builds_instances_of_the_local_constructor() {
    let source = format!(
        r#"{HELPERS}
        class Local extends Array {{}}
        check('from with a local Array', other.Array.from.call(Array, [3, 4, 5]) instanceof Array);
        check('from with a local subclass', other.Array.from.call(Local, [1]) instanceof Local);
        check('of with a local subclass', other.Array.of.call(Local, 1, 2) instanceof Local);
        check('from without a constructor stays foreign', other.Array.from([1, 2]) instanceof other.Array);
        // A non-constructor receiver falls back to the function's own Array.
        const fallback = other.Array.from.call({{}}, [1, 2]);
        check('from fallback', fallback instanceof other.Array && !(fallback instanceof Array));

        // The mapping function keeps its own realm's sloppy `this`.
        const third = $262.createRealm().global;
        third.mainGlobal = globalThis;
        third.eval('function f() {{ mainGlobal.observedThis = this; }}');
        other.Array.from.call(Array, [1], third.f);
        third.globalName = 'third';
        check('mapper this', globalThis.observedThis !== undefined && globalThis.observedThis.globalName === 'third');
        failures.join('; ')
        "#
    );
    assert_no_failures(&source);
}

#[test]
fn the_stack_accessors_of_another_realm_apply_to_local_errors() {
    let source = format!(
        r#"{HELPERS}
        const getB = Object.getOwnPropertyDescriptor(other.Error.prototype, 'stack').get;
        const setB = Object.getOwnPropertyDescriptor(other.Error.prototype, 'stack').set;
        check('getter on a local error', typeof getB.call(new Error('msg')) === 'string');
        check('getter on a local plain object', getB.call({{}}) === undefined);
        const plain = {{}};
        setB.call(plain, 'sentinel');
        const descriptor = Object.getOwnPropertyDescriptor(plain, 'stack');
        check('setter defines an own data property',
            descriptor && descriptor.value === 'sentinel' && descriptor.writable && descriptor.enumerable && descriptor.configurable);
        expectThrown('setter on its own Error.prototype', other.TypeError, () => setB.call(other.Error.prototype, 'x'));
        failures.join('; ')
        "#
    );
    assert_no_failures(&source);
}
