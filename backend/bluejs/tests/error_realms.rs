// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `OrdinaryCreateFromConstructor` for the Error constructors: when the new
//! target's `prototype` is not an object, the default prototype comes from
//! the *new target's* realm (`GetPrototypeFromConstructor`).

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

#[test]
fn every_error_constructor_takes_its_fallback_prototype_from_the_new_targets_realm() {
    let source = r#"
        const other = $262.createRealm().global;
        const failures = [];
        const cases = [
            ['Error', [], other.Error],
            ['TypeError', [], other.TypeError],
            ['RangeError', [], other.RangeError],
            ['SyntaxError', [], other.SyntaxError],
            ['ReferenceError', [], other.ReferenceError],
            ['EvalError', [], other.EvalError],
            ['URIError', [], other.URIError],
            ['AggregateError', [[]], other.AggregateError],
            ['SuppressedError', [undefined, undefined], other.SuppressedError],
        ];
        for (const [name, args, foreign] of cases) {
            const local = globalThis[name];
            for (const value of [undefined, null, true, '', Symbol(), -1]) {
                const newTarget = new other.Function();
                newTarget.prototype = value;
                const made = Reflect.construct(local, args, newTarget);
                if (Object.getPrototypeOf(made) !== foreign.prototype) {
                    failures.push(name + ' with prototype ' + String(typeof value));
                }
            }
            // A same-realm target and an object-valued prototype are unaffected.
            const same = function () {}.bind(null);
            same.prototype = undefined;
            if (Object.getPrototypeOf(Reflect.construct(local, args, same)) !== local.prototype) {
                failures.push(name + ' same-realm fallback');
            }
            const custom = {};
            const target = new other.Function();
            target.prototype = custom;
            if (Object.getPrototypeOf(Reflect.construct(local, args, target)) !== custom) {
                failures.push(name + ' object-valued prototype');
            }
        }
        failures.join('; ')
    "#;
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
