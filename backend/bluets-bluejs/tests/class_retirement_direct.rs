// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! K.5.3 class accessor writes and constructor initialization reach the VM.

use blueice_bluejs::{Value, Vm};
use blueice_bluets::{CheckingOptions, CompilerOptions, MapLoader, ModuleSource};
use blueice_bluets_bluejs::compile_direct_script;

#[test]
fn separate_accessor_read_and_write_types_preserve_runtime_values() {
    for define_fields in [true, false] {
        for source in [
            "class Box { n: number = 1; get value(): number { return this.n; } set value(value: string) { this.n = value.length; } } const b = new Box(); b.value = \"four\"; b.value;",
            "class Box { static n: number = 1; static get value(): number { return this.n; } static set value(value: string) { this.n = value.length; } } Box.value = \"four\"; Box.value;",
            "class Box<T> { constructor(private n: T) {} get value(): T { return this.n; } set value(value: T[]) { this.n = value[0]; } } const b = new Box(1); b.value = [4]; b.value;",
        ] {
            let artifact = compile_direct_script(
                "memory:///main.ts",
                &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
                CompilerOptions {
                    use_define_for_class_fields: Some(define_fields),
                    checking: Some(CheckingOptions::default()),
                    ..CompilerOptions::default()
                },
            ).unwrap_or_else(|error| panic!("define fields {define_fields}: {source}: {error:?}"));
            assert_eq!(Vm::default().execute(&artifact.bytecode).unwrap(), Value::Number(4.0));
        }
    }
}

#[test]
fn constructor_branch_initialization_preserves_both_field_modes() {
    let source = "class Box { value: number; constructor(flag: boolean) { if (flag) { this.value = 3; } else { this.value = 4; } } } new Box(true).value + new Box(false).value;";
    for define_fields in [true, false] {
        let artifact = compile_direct_script(
            "memory:///main.ts",
            &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
            CompilerOptions {
                use_define_for_class_fields: Some(define_fields),
                checking: Some(CheckingOptions::default()),
                ..CompilerOptions::default()
            },
        )
        .unwrap_or_else(|error| panic!("define fields {define_fields}: {error:?}"));
        assert_eq!(
            Vm::default().execute(&artifact.bytecode).unwrap(),
            Value::Number(7.0)
        );
    }
}

#[test]
fn invalid_accessor_writes_and_incomplete_constructors_are_rejected() {
    for source in [
        "class Box { get value(): number { return 1; } set value(value: string) {} } const b = new Box(); b.value = 7;",
        "class Box { value: number; constructor(flag: boolean) { if (flag) { return; } this.value = 3; } } new Box(true).value;",
        "class Box { value: number; constructor(flag: boolean) { if (flag) { this.value = 3; } } } new Box(false).value;",
    ] {
        assert!(compile_direct_script(
            "memory:///main.ts",
            &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
            CompilerOptions { checking: Some(CheckingOptions::default()), ..CompilerOptions::default() },
        ).is_err(), "{source}");
    }
}
