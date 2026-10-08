// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The checked direct bridge erases class modifiers without creating members.

use blueice_bluejs::{Value, Vm};
use blueice_bluets::{CheckingOptions, CompilerOptions, MapLoader, ModuleSource};
use blueice_bluets_bluejs::compile_direct_script;

#[test]
fn abstract_members_and_implements_are_erased_in_both_field_modes() {
    for define_fields in [true, false] {
        for (source, expected) in [
            ("abstract class Base { abstract read(): number; twice(): number { return this.read() * 2; } } class Child extends Base { override read(): number { return 3; } } new Child().twice();", 6.0),
            ("interface Shape { value: number; } abstract class Base implements Shape { abstract value: number; } class Child extends Base { override value = 7; } new Child().value;", 7.0),
            ("abstract class Base { abstract get value(): number; abstract set value(v: number); } class Child extends Base { stored = 1; override get value(): number { return this.stored; } override set value(v: number) { this.stored = v; } } const item = new Child(); item.value = 9; item.value;", 9.0),
        ] {
            let artifact = compile_direct_script(
                "memory:///main.ts",
                &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
                CompilerOptions {
                    use_define_for_class_fields: Some(define_fields),
                    checking: Some(CheckingOptions {
                        no_implicit_override: true,
                        ..CheckingOptions::default()
                    }),
                    ..CompilerOptions::default()
                },
            ).unwrap_or_else(|error| panic!("define fields {define_fields}: {source}: {error:?}"));
            assert_eq!(Vm::default().execute(&artifact.bytecode).unwrap(), Value::Number(expected));
        }
    }
}

#[test]
fn abstract_constructor_aliases_are_rejected_before_runtime_admission() {
    let result = compile_direct_script(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "abstract class Base {} const Alias = Base; new Alias();",
        )]),
        CompilerOptions {
            checking: Some(CheckingOptions::default()),
            ..CompilerOptions::default()
        },
    );
    assert!(result.is_err());
}
