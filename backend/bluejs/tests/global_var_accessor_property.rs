// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! CreateGlobalVarBinding leaves an existing own property of the global
//! object alone, so a `var` (or eval `var`) whose name is an accessor must not
//! replace that accessor with a data property or call it.
use blueice_bluejs::{compile, parse, Value, Vm};

fn assert_true(source: &str) {
    let program = parse(source).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    let code = compile(&program).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    assert_eq!(
        Vm::default()
            .execute_script(&code)
            .unwrap_or_else(|e| panic!("{source}: {e:?}")),
        Value::Bool(true),
        "{source}"
    );
}

#[test]
fn an_eval_var_does_not_replace_or_call_a_global_accessor() {
    assert_true(
        "var hit = 0;
         Object.defineProperty(this, 'x', { get: function () { return ++hit; }, configurable: true });
         eval('var x;');
         var untouched = hit === 0;
         var first = x, second = x;
         var d = Object.getOwnPropertyDescriptor(this, 'x');
         untouched && first === 1 && second === 2 && typeof d.get === 'function' && d.set === undefined",
    );
}

#[test]
fn an_eval_var_without_an_initializer_keeps_a_global_setter() {
    assert_true(
        "var stored;
         Object.defineProperty(this, 'y', {
           get: function () { return 'got'; }, set: function (v) { stored = v; }, configurable: true });
         eval('var y;');
         y = 7;
         stored === 7 && y === 'got'",
    );
}

#[test]
fn a_global_data_property_is_still_reused_by_an_eval_var() {
    assert_true(
        "globalThis.z = 3; eval('var z;'); z === 3 && Object.getOwnPropertyDescriptor(this, 'z').value === 3",
    );
}
