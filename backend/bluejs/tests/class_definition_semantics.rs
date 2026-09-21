// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ClassDefinitionEvaluation regressions found by the Test262 class scope:
//! heritage, constructor property order, inner name bindings, and the derived
//! constructor `this`/`new.target` environment.

use blueice_bluejs::{compile, parse, Value, Vm};

fn assert_true(source: &str) {
    let program = parse(source).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    let code = compile(&program).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    let result = Vm::default().execute(&code);
    assert_eq!(result, Ok(Value::Bool(true)), "{source}");
}

#[test]
fn extends_null_keeps_function_prototype_as_the_constructor_parent() {
    assert_true(
        "class Foo extends null {}
         Object.getPrototypeOf(Foo.prototype) === null
             && Object.getPrototypeOf(Foo) === Function.prototype
             && Foo.prototype.constructor === Foo",
    );
    assert_true(
        "const E = class extends null { constructor() {} };
         Object.getPrototypeOf(E) === Function.prototype
             && Object.getPrototypeOf(E.prototype) === null",
    );
}

#[test]
fn class_constructor_own_keys_start_with_length_name_prototype() {
    assert_true(
        "class C { static m() {} static a = 1; static [Symbol.iterator]() {} }
         Object.getOwnPropertyNames(C).join() === 'length,name,prototype,m,a'",
    );
    assert_true(
        "const C = class { static m() {} };
         Object.getOwnPropertyNames(C).join() === 'length,name,prototype,m'",
    );
    assert_true(
        "class C { static [1]() {} static ['x']() {} }
         Object.getOwnPropertyNames(C).join() === '1,length,name,prototype,x'",
    );
}

#[test]
fn an_arrow_function_closes_over_its_creators_new_target() {
    assert_true(
        "function F() { return () => new.target; }
         const viaNew = new F();
         const viaCall = F();
         viaNew() === F && viaCall() === undefined",
    );
    // The caller's own `new.target` never leaks into the arrow.
    assert_true(
        "function F() { return () => new.target; }
         const arrow = F();
         let seen = 'unset';
         function G() { seen = arrow(); }
         new G();
         seen === undefined",
    );
    assert_true(
        "function F() { return () => () => new.target; }
         new F()()() === F",
    );
    assert_true(
        "function F() { return () => eval('new.target'); }
         new F()() === F",
    );
}
