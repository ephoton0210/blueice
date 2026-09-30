// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! DefineField: a public instance field is created with
//! CreateDataPropertyOrThrow on the new instance, not assigned with [[Set]].

use blueice_bluejs::{compile, parse, Value, Vm};

fn assert_true(source: &str) {
    let program = parse(source).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    let result = Vm::default().execute(&compile(&program).unwrap());
    assert_eq!(result, Ok(Value::Bool(true)), "{source}");
}

#[test]
fn a_field_shadows_an_inherited_setter_instead_of_calling_it() {
    assert_true(
        "let calls = 0;
         class P { set s(v) { calls++; } }
         class Q extends P { s = 1; }
         const q = new Q();
         const d = Object.getOwnPropertyDescriptor(q, 's');
         calls === 0 && d.value === 1 && d.writable && d.enumerable && d.configurable",
    );
}

#[test]
fn a_field_named_proto_creates_an_own_property() {
    assert_true(
        "class C { __proto__ = 5; ['__proto__'] = 6; }
         const c = new C();
         Object.getPrototypeOf(c) === C.prototype
             && Object.getOwnPropertyNames(c).join() === '__proto__'
             && c.__proto__ === 6",
    );
}

#[test]
fn a_field_reaches_the_receivers_define_own_property_not_its_set_trap() {
    assert_true(
        "const log = [];
         class Base {
             constructor() {
                 return new Proxy({}, {
                     defineProperty(target, key, descriptor) {
                         log.push('define ' + String(key));
                         return Reflect.defineProperty(target, key, descriptor);
                     },
                     set() { log.push('set'); return true; },
                 });
             }
         }
         const sym = Symbol('s');
         class Derived extends Base { a = 1; ['b' + 1] = 2; [sym] = 3; }
         new Derived();
         log.join() === 'define a,define b1,define Symbol(s)'",
    );
}

#[test]
fn defining_a_field_on_a_non_extensible_or_frozen_receiver_throws() {
    for receiver in ["Object.preventExtensions({})", "Object.freeze({ f: 1 })"] {
        assert_true(&format!(
            "class Base {{ constructor() {{ return {receiver}; }} }}
             class Derived extends Base {{ f = 2; }}
             (() => {{ try {{ new Derived(); return false; }} catch (e) {{ return e instanceof TypeError; }} }})()"
        ));
    }
}

#[test]
fn field_initializers_still_see_earlier_fields_and_this() {
    assert_true(
        "class C { a = 1; b = this.a + 1; ['c'] = this.b + 1; #p = 4; d = this.#p; }
         const c = new C();
         c.a === 1 && c.b === 2 && c.c === 3 && c.d === 4",
    );
    // A later field of the same name redefines rather than throws.
    assert_true("class C { a = 1; ['a'] = 2; } new C().a === 2");
}
