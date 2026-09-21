// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `GetPrototypeFromConstructor` across `$262.createRealm()` realms: the
//! fallback prototype (used when `newTarget.prototype` is not an object)
//! belongs to the realm of `newTarget`, whichever realm the constructor being
//! called lives in.

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

#[test]
fn local_constructors_take_their_fallback_prototype_from_the_new_targets_realm() {
    assert_no_failures(
        r#"
        const other = $262.createRealm().global;
        const failures = [];
        const otherFunctionKind = (source) =>
            Object.getPrototypeOf(other.eval(source)).constructor.prototype;
        const localFunctionKind = (source) =>
            Object.getPrototypeOf((0, eval)(source)).constructor;
        const cases = [
            ['Promise', () => Promise, [function () {}], other.Promise.prototype],
            ['RegExp', () => RegExp, [], other.RegExp.prototype],
            ['String', () => String, [], other.String.prototype],
            ['Date', () => Date, [], other.Date.prototype],
            ['AsyncFunction', () => localFunctionKind('(async function () {})'), [],
                otherFunctionKind('(0, async function () {})')],
            ['GeneratorFunction', () => localFunctionKind('(function* () {})'), [],
                otherFunctionKind('(0, function* () {})')],
            ['AsyncGeneratorFunction', () => localFunctionKind('(async function* () {})'), [],
                otherFunctionKind('(0, async function* () {})')],
        ];
        for (const [name, constructor, args, expected] of cases) {
            for (const value of [undefined, null, true, '', Symbol(), 1]) {
                const newTarget = new other.Function();
                newTarget.prototype = value;
                const made = Reflect.construct(constructor(), args, newTarget);
                if (Object.getPrototypeOf(made) !== expected) {
                    failures.push(name + ' with prototype ' + String(typeof value));
                }
            }
            // A same-realm target keeps the local intrinsic.
            const same = function () {}.bind(null);
            same.prototype = undefined;
            const local = Reflect.construct(constructor(), args, same);
            if (Object.getPrototypeOf(local) === expected) {
                failures.push(name + ' same-realm fallback used the foreign prototype');
            }
        }
        failures.join('; ')
        "#,
    );
}

#[test]
fn foreign_constructors_take_their_fallback_prototype_from_the_new_targets_realm() {
    assert_no_failures(
        r#"
        const failures = [];
        const realmA = $262.createRealm().global;
        const realmB = $262.createRealm().global;
        const kind = (realm, source) =>
            Object.getPrototypeOf(realm.eval(source)).constructor;
        const cases = [
            ['Function', realmA.Function, realmB.Function.prototype],
            ['GeneratorFunction', kind(realmA, '(0, function* () {})'),
                kind(realmB, '(0, function* () {})').prototype],
            ['AsyncFunction', kind(realmA, '(0, async function () {})'),
                kind(realmB, '(0, async function () {})').prototype],
            ['AsyncGeneratorFunction', kind(realmA, '(0, async function* () {})'),
                kind(realmB, '(0, async function* () {})').prototype],
            ['Date', realmA.Date, realmB.Date.prototype],
            ['Promise', realmA.Promise, realmB.Promise.prototype],
            ['Map', realmA.Map, realmB.Map.prototype],
        ];
        for (const [name, constructor, expected] of cases) {
            const newTarget = new realmB.Function();
            newTarget.prototype = null;
            const args = name === 'Promise' ? [function () {}] : [];
            const made = Reflect.construct(constructor, args, newTarget);
            if (Object.getPrototypeOf(made) !== expected) {
                failures.push(name + ' with a foreign new target');
            }
            // The constructor's own realm is not consulted: a new target
            // from the calling realm selects the calling realm's intrinsic.
            const local = function () {}.bind(null);
            local.prototype = 'not an object';
            const localMade = Reflect.construct(constructor, args, local);
            if (Object.getPrototypeOf(localMade) === expected) {
                failures.push(name + ' with a local new target used the foreign prototype');
            }
        }
        // A bound function delegates GetFunctionRealm to its target.
        const realmC = $262.createRealm().global;
        const target = new realmA.Function();
        target.prototype = 'str';
        const bound = realmB.Function.prototype.bind.call(target);
        const date = Reflect.construct(realmC.Date, [], bound);
        if (Object.getPrototypeOf(date) !== realmA.Date.prototype) {
            failures.push('Date with a bound foreign new target');
        }
        if (!(date instanceof realmA.Date)) {
            failures.push('Date instanceof realmA.Date');
        }
        failures.join('; ')
        "#,
    );
}

#[test]
fn a_foreign_constructor_called_with_its_own_new_target_keeps_its_prototype() {
    assert_no_failures(
        r#"
        const other = $262.createRealm().global;
        const failures = [];
        for (const name of ['Date', 'Promise', 'Map', 'RegExp']) {
            const args = name === 'Promise' ? [function () {}] : [];
            const made = new other[name](...args);
            if (Object.getPrototypeOf(made) !== other[name].prototype) {
                failures.push(name);
            }
        }
        failures.join('; ')
        "#,
    );
}
