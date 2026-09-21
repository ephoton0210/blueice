// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Which realm an error belongs to when an operation on a function from
//! another `$262.createRealm()` realm fails: errors the specification raises
//! from the callee's own execution context belong to the callee's realm, and
//! errors raised by the caller's checks (or by [[Construct]] after the callee's
//! context has been removed) belong to the caller's realm.

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
    // Records a failure unless `action` throws an instance of `expected`
    // (and no other realm's error of the same name).
    function expectThrown(label, expected, action) {
        try {
            action();
        } catch (error) {
            if (Object.getPrototypeOf(error) !== expected.prototype) {
                failures.push(label + ': wrong realm or type (' + Object.prototype.toString.call(error) + ')');
            }
            return;
        }
        failures.push(label + ': no error');
    }
"#;

#[test]
fn construct_checks_of_the_caller_are_created_in_the_callers_realm() {
    let source = format!(
        r#"{HELPERS}
        const nonConstructors = {{
            'native function': other.parseInt,
            'arrow function': other.eval('(0, () => 1)'),
            'generator': other.eval('(0, function* () {{}})'),
            'async function': other.eval('(0, async function () {{}})'),
            'method': other.eval('({{ m() {{}} }}).m'),
        }};
        for (const name of Object.keys(nonConstructors)) {{
            const f = nonConstructors[name];
            expectThrown(name + ' with arguments', TypeError, () => new f(0));
            expectThrown(name + ' without arguments', TypeError, () => new f);
            expectThrown(name + ' via Reflect.construct', TypeError, () => Reflect.construct(f, []));
        }}
        failures.join('; ')
        "#
    );
    assert_no_failures(&source);
}

#[test]
fn derived_constructor_completion_checks_are_created_in_the_callers_realm() {
    let source = format!(
        r#"{HELPERS}
        const returnsNull = other.eval('0, class extends Object {{ constructor() {{ super(); return null; }} }}');
        const returnsNumber = other.eval('0, class extends Object {{ constructor() {{ super(); return 1; }} }}');
        const noSuper = other.eval('0, class extends Object {{ constructor() {{}} }}');
        const returnsNullFirst = other.eval('0, class extends Object {{ constructor() {{ return null; }} }}');
        expectThrown('derived return null before super()', TypeError, () => new returnsNullFirst());
        expectThrown('derived return null', TypeError, () => new returnsNull());
        expectThrown('derived return number', TypeError, () => new returnsNumber());
        expectThrown('derived without super()', ReferenceError, () => new noSuper());
        expectThrown('derived via Reflect.construct', ReferenceError, () => Reflect.construct(noSuper, []));

        // Errors raised by the callee's own code stay in the callee's realm,
        // even after a nested constructor's completion check failed inside it
        // and was caught there.
        const bodyError = other.eval(`0, class extends Object {{
            constructor() {{
                try {{
                    new (class extends Object {{ constructor() {{}} }});
                }} catch (e) {{}}
                super();
                null.property;
            }}
        }}`);
        expectThrown('body error after a caught nested check', other.TypeError, () => new bodyError());
        const throws = other.eval('0, class extends Object {{ constructor() {{ super(); throw new RangeError(); }} }}');
        expectThrown('explicit throw', other.RangeError, () => new throws());
        // A class constructor called without `new` throws from its own realm.
        expectThrown('class call', other.TypeError, () => noSuper());
        // The local realm is unaffected.
        const localNoSuper = (0, eval)('0, class extends Object {{ constructor() {{}} }}');
        expectThrown('local derived without super()', ReferenceError, () => new localNoSuper());
        failures.join('; ')
        "#
    );
    assert_no_failures(&source);
}

#[test]
fn errors_raised_while_running_foreign_code_are_created_in_its_realm() {
    let source = format!(
        r#"{HELPERS}
        const holder = other.eval('({{ get g() {{ null.x; }}, set s(v) {{ undefined.y; }} }})');
        expectThrown('foreign getter body', other.TypeError, () => holder.g);
        expectThrown('foreign setter body', other.TypeError, () => {{ holder.s = 1; }});
        expectThrown('Reflect.set through a setter', other.TypeError, () => Reflect.set(holder, 's', 1));
        expectThrown('Reflect.get through a getter', other.TypeError, () => Reflect.get(holder, 'g'));
        const thrower = other.eval('(0, function () {{ null.x; }})');
        expectThrown('foreign function body', other.TypeError, () => thrower());

        // A local accessor delegating to a foreign object's own setter: the
        // error belongs to the realm whose setter threw it.
        const setA = Object.getOwnPropertyDescriptor(Error.prototype, 'stack').set;
        expectThrown('cross-realm stack setter', other.TypeError, () => setA.call(other.Error.prototype, 'x'));
        expectThrown('same-realm stack setter', TypeError, () => setA.call(Error.prototype, 'x'));
        failures.join('; ')
        "#
    );
    assert_no_failures(&source);
}
