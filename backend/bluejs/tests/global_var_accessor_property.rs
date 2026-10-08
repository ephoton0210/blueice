// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! CreateGlobalVarBinding leaves an existing own property of the global
//! object alone, so a `var` (or eval `var`) whose name is an accessor must not
//! replace that accessor with a data property or call it.
use blueice_bluejs::{compile, parse, HeapConfig, Value, Vm, VmConfig};

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

#[test]
fn deleted_eval_globals_reach_replacement_accessors_in_captured_references() {
    for nursery_capacity in [1, HeapConfig::default().nursery_capacity] {
        for deletion in [
            "delete gone",
            "delete globalThis.gone",
            "Reflect.deleteProperty(globalThis, 'gone')",
            "delete new Proxy(globalThis, {}).gone",
        ] {
            let source = format!(
                "eval('var gone = 7; globalThis.readGone = () => gone; \
                 globalThis.writeGone = () => gone = 42;');
                 if (!({deletion})) throw 'deletion refused';
                 var stored = 0;
                 Object.defineProperty(globalThis, 'gone', {{
                   get() {{return 35;}}, set(value) {{stored = value;}}
                 }});
                 var read = readGone();
                 writeGone();
                 read === 35 && stored === 42"
            );
            let mut vm = Vm::new(VmConfig {
                heap: HeapConfig {
                    nursery_capacity,
                    ..HeapConfig::default()
                },
                ..VmConfig::default()
            })
            .unwrap();
            let code = compile(&parse(&source).unwrap()).unwrap();
            assert_eq!(vm.execute_script(&code), Ok(Value::Bool(true)), "{source}");
            assert_eq!(
                vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
                Ok(Value::Number(42.0))
            );
        }
    }
}

#[test]
fn property_deletion_preserves_lexical_and_nonconfigurable_global_bindings() {
    assert_true(
        "let lexical = 7; globalThis.lexical = 9;
         var removed = Reflect.deleteProperty(globalThis, 'lexical');
         eval('var protectedGlobal = 35');
         Object.defineProperty(globalThis, 'protectedGlobal', {configurable:false});
         var retained = !Reflect.deleteProperty(globalThis, 'protectedGlobal');
         removed && lexical === 7 && retained && protectedGlobal === 35",
    );
}
