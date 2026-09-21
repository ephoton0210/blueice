// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The essential internal methods of an object that lives in another
//! `$262.createRealm()` realm: [[GetOwnProperty]], [[DefineOwnProperty]],
//! [[HasProperty]], [[Delete]], [[IsExtensible]], [[PreventExtensions]] and
//! [[SetPrototypeOf]] must act on that object, not on an empty local stand-in.

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
fn own_property_descriptors_of_a_foreign_object_are_visible() {
    let source = format!(
        r#"{HELPERS}
        const f = other.eval('(0, function f(a, b) {{}})');
        const length = Object.getOwnPropertyDescriptor(f, 'length');
        check('length exists', length !== undefined);
        check('length value', length && length.value === 2);
        check('length attributes', length && !length.writable && !length.enumerable && length.configurable);
        check('missing property', Object.getOwnPropertyDescriptor(f, 'nope') === undefined);
        check('hasOwnProperty', Object.prototype.hasOwnProperty.call(f, 'name'));
        check('propertyIsEnumerable', !Object.prototype.propertyIsEnumerable.call(f, 'name'));

        // Values, getters and setters cross the boundary as the same objects.
        const bag = other.eval('({{ a: 1, get g() {{ return 2; }}, set s(v) {{}} }})');
        const g = Object.getOwnPropertyDescriptor(bag, 'g');
        check('accessor getter', g && typeof g.get === 'function' && g.set === undefined);
        check('accessor getter call', g && g.get.call(bag) === 2);
        check('same getter object', g && g.get === Object.getOwnPropertyDescriptor(bag, 'g').get);
        const s = Object.getOwnPropertyDescriptor(bag, 's');
        check('accessor setter', s && s.get === undefined && typeof s.set === 'function');
        const nested = other.eval('({{ inner: {{}} }})');
        check('object value identity',
            Object.getOwnPropertyDescriptor(nested, 'inner').value === nested.inner);
        check('Object.keys', Object.keys(bag).join() === 'a,g,s');
        failures.join('; ')
        "#
    );
    assert_no_failures(&source);
}

#[test]
fn throw_type_error_is_one_function_per_realm() {
    let source = format!(
        r#"{HELPERS}
        const localArgs = (function () {{ 'use strict'; return arguments; }})();
        const otherArgs = new other.Function('"use strict"; return arguments;')();
        const otherArgs2 = new other.Function('"use strict"; return arguments;')();
        const local = Object.getOwnPropertyDescriptor(localArgs, 'callee');
        const foreign = Object.getOwnPropertyDescriptor(otherArgs, 'callee');
        const foreign2 = Object.getOwnPropertyDescriptor(otherArgs2, 'callee');
        check('foreign accessor', foreign && typeof foreign.get === 'function');
        if (foreign) {{
            check('getter is setter', foreign.get === foreign.set);
            check('distinct from the local one', foreign.get !== local.get);
            check('shared by the realm', foreign.get === foreign2.get);
            expectThrown('foreign getter', other.TypeError, () => foreign.get());
            expectThrown('local getter', TypeError, () => local.get());
        }}
        failures.join('; ')
        "#
    );
    assert_no_failures(&source);
}

#[test]
fn defining_and_deleting_properties_of_a_foreign_object_takes_effect() {
    let source = format!(
        r#"{HELPERS}
        const f = other.eval('(0, function () {{}})');
        Object.defineProperty(f, 'extra', {{ value: 7, configurable: true, writable: true }});
        check('defined value', f.extra === 7);
        const extra = Object.getOwnPropertyDescriptor(f, 'extra');
        check('defined descriptor', extra && extra.value === 7 && extra.writable && !extra.enumerable && extra.configurable);
        check('has defined property', 'extra' in f);
        check('delete result', delete f.extra);
        check('deleted', !('extra' in f) && f.extra === undefined);

        const args = other.eval('(0, function () {{ "use strict"; return arguments; }})')();
        check('non-configurable define', Reflect.defineProperty(args, 'callee', {{ value: 1 }}) === false);
        check('non-configurable delete', Reflect.deleteProperty(args, 'callee') === false);
        expectThrown('define throws in this realm', TypeError, () => Object.defineProperty(args, 'callee', {{ value: 1 }}));

        // Inherited properties are found through the foreign prototype chain.
        check('inherited property', 'call' in f && 'hasOwnProperty' in f && !('nope' in f));
        failures.join('; ')
        "#
    );
    assert_no_failures(&source);
}

#[test]
fn extensibility_and_prototype_of_a_foreign_object_are_forwarded() {
    let source = format!(
        r#"{HELPERS}
        const f = other.eval('(0, function () {{}})');
        check('extensible', Object.isExtensible(f));
        check('prevent', Reflect.preventExtensions(f));
        check('no longer extensible', !Object.isExtensible(f));
        check('cannot add', Reflect.defineProperty(f, 'added', {{ value: 1 }}) === false);
        check('frozen check', !Object.isFrozen(f));

        const g = other.eval('(0, function () {{}})');
        const mine = {{}};
        check('set local prototype', Reflect.setPrototypeOf(g, mine));
        check('local prototype visible', Object.getPrototypeOf(g) === mine);
        check('set null prototype', Reflect.setPrototypeOf(g, null));
        check('null prototype visible', Object.getPrototypeOf(g) === null);
        check('same prototype is a no-op', Reflect.setPrototypeOf(g, null));
        const sealed = other.eval('(0, function () {{}})');
        Object.preventExtensions(sealed);
        check('sealed prototype change', Reflect.setPrototypeOf(sealed, mine) === false);
        failures.join('; ')
        "#
    );
    assert_no_failures(&source);
}

#[test]
fn prototype_cycles_are_detected_across_realms() {
    let source = format!(
        r#"{HELPERS}
        const obj = {{}};
        const w = other.Object.create(obj);
        expectThrown('local setPrototypeOf', TypeError, () => Object.setPrototypeOf(obj, w));
        check('local Reflect.setPrototypeOf', Reflect.setPrototypeOf(obj, w) === false);
        expectThrown('foreign setPrototypeOf', other.TypeError, () => other.Object.setPrototypeOf(obj, w));
        check('chain unchanged', Object.getPrototypeOf(obj) === Object.prototype);
        // A non-cyclic change across the boundary still succeeds.
        const free = {{}};
        check('acyclic', Reflect.setPrototypeOf(free, w) && Object.getPrototypeOf(free) === w);
        failures.join('; ')
        "#
    );
    assert_no_failures(&source);
}
