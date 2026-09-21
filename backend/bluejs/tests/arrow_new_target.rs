// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! An arrow function has no `new.target` of its own: it reads the value of the
//! function it was created in (§9.4.2 GetThisEnvironment), captured at
//! creation and unaffected by whoever calls it later.

use blueice_bluejs::{compile, parse, Value, Vm, VmConfig};

fn truthy(source: &str) {
    for capacity in [None, Some(1)] {
        let mut config = VmConfig::default();
        if let Some(capacity) = capacity {
            config.heap.nursery_capacity = capacity;
        }
        let result = Vm::new(config)
            .unwrap()
            .execute(&compile(&parse(source).unwrap()).unwrap());
        assert_eq!(result, Ok(Value::Bool(true)), "{capacity:?}: {source}");
    }
}

#[test]
fn an_arrow_keeps_the_new_target_of_its_creating_construction() {
    truthy("function F() { this.af = () => new.target; } new F().af() === F");
    truthy("function K() { this.t = (() => new.target)(); } new K().t === K");
    truthy("function N() { this.g = () => () => new.target; } new N().g()() === N");
}

#[test]
fn an_arrow_created_in_an_ordinary_call_sees_undefined() {
    truthy("function G() { return () => new.target; } G()() === undefined");
    // ... even when it is called from inside an unrelated construction.
    truthy("function M() { return () => new.target; } var a = M(); function C() { this.r = a(); } new C().r === undefined");
}

#[test]
fn a_regular_function_still_has_its_own_new_target() {
    truthy("function F() { return new.target; } F() === undefined && new F() instanceof Object");
    truthy("var seen; function P() { seen = new.target; } new P(); seen === P");
    truthy("var seen; function Q() { (() => { seen = new.target; })(); } Reflect.construct(Q, [], Object); seen === Object");
}

#[test]
fn a_class_field_initializer_sees_no_new_target() {
    truthy("class C { x = () => new.target; y = new.target; } var c = new C(); c.x() === undefined && c.y === undefined");
    truthy("var seen = 'unset'; class C { x = eval('() => new.target'); constructor() { seen = new.target; } } var c = new C(); c.x() === undefined && seen === C");
    truthy("class B { constructor() { this.n = new.target; } } class D extends B { f = new.target; } var d = new D(); d.n === D && d.f === undefined");
}
