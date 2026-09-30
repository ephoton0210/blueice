// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `nonextensible-applies-to-private`: adding a private field, method or
//! accessor to an object that is not extensible throws a TypeError, whether
//! the object comes from a base constructor's return override, is sealed by
//! an earlier field initializer, or is the class constructor itself.

use blueice_bluejs::{compile, parse, Value, Vm};

fn assert_true(source: &str) {
    let program = parse(source).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    let result = Vm::default().execute(&compile(&program).unwrap());
    assert_eq!(result, Ok(Value::Bool(true)), "{source}");
}

fn throws_type_error(action: &str) -> String {
    format!("(() => {{ try {{ {action}; return false; }} catch (e) {{ return e instanceof TypeError; }} }})()")
}

#[test]
fn a_returned_non_extensible_object_rejects_every_kind_of_private_element() {
    for element in [
        "#v = 1;",
        "#v;",
        "#m() { return 1; }",
        "get #a() { return 1; }",
    ] {
        let check = throws_type_error("new Sub(Object.preventExtensions({}))");
        assert_true(&format!(
            "class Base {{ constructor(o) {{ return o; }} }}
             class Sub extends Base {{ {element} }}
             {check}"
        ));
        // An extensible object is fine, so the rejection is about extensibility.
        assert_true(&format!(
            "class Base {{ constructor(o) {{ return o; }} }}
             class Sub extends Base {{ {element} }}
             new Sub({{}}) instanceof Sub === false"
        ));
    }
}

#[test]
fn a_field_initializer_that_seals_the_instance_rejects_its_own_private_field() {
    assert_true(&format!(
        "class Sealing {{ #g = (Object.preventExtensions(this), 'x'); }}
         {}",
        throws_type_error("new Sealing()")
    ));
    // Objects sealed before construction of a subclass instance behave alike.
    assert_true(&format!(
        "class Base {{ constructor(seal) {{ if (seal) Object.preventExtensions(this); }} }}
         class Sub extends Base {{ #v = 1; constructor(seal) {{ super(seal); }} val() {{ return this.#v; }} }}
         new Sub(false).val() === 1 && {}",
        throws_type_error("new Sub(true)")
    ));
}

#[test]
fn a_static_private_field_of_a_sealed_class_constructor_is_rejected() {
    assert_true(&throws_type_error(
        "class C { static #g = (Object.preventExtensions(C), 1); }",
    ));
    assert_true("class C { static #g = 5; static get() { return C.#g; } } C.get() === 5");
}

#[test]
fn private_names_can_still_be_tested_on_non_extensible_objects() {
    assert_true(
        "class C { #x = 1; static has(o) { return #x in o; } }
         const sealed = Object.preventExtensions({});
         C.has(sealed) === false && C.has(new C()) === true",
    );
}
