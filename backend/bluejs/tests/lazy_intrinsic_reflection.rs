// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Intrinsics are created on first use, but every reflective operation must
//! observe them as the ordinary own properties they are: a first
//! `Object.defineProperty` on `Object.prototype.hasOwnProperty` keeps the
//! method's `configurable: true`, a first `delete` really removes it, and the
//! key lists of `Object.prototype` and the global object are complete.
use blueice_bluejs::{compile, parse, Value, Vm};

/// Every check runs in a fresh `Vm` whose first touch of the intrinsic under
/// test is the reflective operation itself.
fn fresh(source: &str) -> Value {
    let program = parse(source).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    Vm::default()
        .execute(&compile(&program).unwrap())
        .unwrap_or_else(|e| panic!("{source}: {e:?}"))
}

#[test]
fn a_first_define_on_a_lazy_object_prototype_method_preserves_its_attributes() {
    for name in ["hasOwnProperty", "propertyIsEnumerable"] {
        let source = format!(
            "Object.defineProperty(Object.prototype, '{name}', {{ set: function () {{}} }});
             var d = Object.getOwnPropertyDescriptor(Object.prototype, '{name}');
             d.configurable === true && d.enumerable === false && typeof d.set === 'function'
               && d.get === undefined"
        );
        assert_eq!(fresh(&source), Value::Bool(true), "{name}");
    }
    // The accessor can be replaced by a data property again, and a setter on
    // the prototype chain intercepts a sloppy assignment.
    assert_eq!(
        fresh(
            "var hits = 0;
             Object.defineProperty(Object.prototype, 'propertyIsEnumerable', { set: function () { hits++; } });
             var o = {}; o.propertyIsEnumerable = 1;
             Object.defineProperty(Object.prototype, 'propertyIsEnumerable', { value: 3 });
             hits === 1 && o.propertyIsEnumerable === 3"
        ),
        Value::Bool(true)
    );
}

#[test]
fn a_first_delete_removes_a_lazy_object_prototype_method() {
    for name in ["hasOwnProperty", "propertyIsEnumerable"] {
        let source = format!(
            "var deleted = delete Object.prototype.{name};
             deleted === true && typeof ({{}}).{name} === 'undefined'
               && !Object.getOwnPropertyNames(Object.prototype).includes('{name}')"
        );
        assert_eq!(fresh(&source), Value::Bool(true), "{name}");
    }
}

#[test]
fn a_first_key_listing_of_object_prototype_includes_the_lazy_methods() {
    assert_eq!(
        fresh(
            "var names = Object.getOwnPropertyNames(Object.prototype);
             ['constructor', 'hasOwnProperty', 'isPrototypeOf', 'propertyIsEnumerable', 'toLocaleString',
              'toString', 'valueOf', '__proto__', '__defineGetter__', '__defineSetter__',
              '__lookupGetter__', '__lookupSetter__'].every(function (n) { return names.includes(n); })"
        ),
        Value::Bool(true)
    );
    assert_eq!(
        fresh("Reflect.ownKeys(Object.prototype).includes('hasOwnProperty')"),
        Value::Bool(true)
    );
}

#[test]
fn the_global_object_lists_every_standard_global() {
    assert_eq!(
        fresh(
            "var names = Object.getOwnPropertyNames(globalThis);
             var expected = ['globalThis', 'Infinity', 'NaN', 'undefined', 'eval', 'isFinite', 'isNaN',
               'parseFloat', 'parseInt', 'decodeURI', 'decodeURIComponent', 'encodeURI',
               'encodeURIComponent', 'escape', 'unescape', 'AggregateError', 'Array', 'ArrayBuffer',
               'BigInt', 'BigInt64Array', 'BigUint64Array', 'Boolean', 'DataView', 'Date', 'Error',
               'EvalError', 'FinalizationRegistry', 'Float32Array', 'Float64Array', 'Function',
               'Int8Array', 'Int16Array', 'Int32Array', 'Iterator', 'Map', 'Number', 'Object',
               'Promise', 'Proxy', 'RangeError', 'ReferenceError', 'RegExp', 'Set', 'SharedArrayBuffer',
               'String', 'Symbol', 'SyntaxError', 'TypeError', 'Uint8Array', 'Uint8ClampedArray',
               'Uint16Array', 'Uint32Array', 'URIError', 'WeakMap', 'WeakRef', 'WeakSet',
               'Atomics', 'JSON', 'Math', 'Reflect', 'Intl', 'Temporal'];
             expected.filter(function (n) { return !names.includes(n); }).join()"
        ),
        Value::String("".into())
    );
}
