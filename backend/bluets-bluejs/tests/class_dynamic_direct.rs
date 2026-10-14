// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Class type erasure and property keys reach the authorized direct VM.

use blueice_bluejs::{Value, Vm};
use blueice_bluets::{CheckingOptions, CompilerOptions, EcmaTarget, MapLoader, ModuleSource};
use blueice_bluets_bluejs::compile_direct_script;

#[test]
fn generic_heritage_methods_and_declare_fields_preserve_runtime_values() {
    for define_fields in [true, false] {
        for (source, expected) in [
            ("class B<T> { constructor(public value: T) {} read(): T { return this.value; } } class C extends B<number> {} new C(7).read();", 7.0),
            ("class C { read<T>(value: T): T { return value; } static read<T>(value: T): T { return value; } } new C().read(3) + C.read(4);", 7.0),
            ("class B { value = 7; } class C extends B { declare value: number; } new C().value;", 7.0),
            ("class C { \"value\" = 3; 1(): number { return this.value; } } new C()[1]();", 3.0),
            ("const key = \"value\"; class C { [key] = 3; static [key] = 4; } new C().value + C.value;", 7.0),
            ("class B<T> { [key: string]: T; constructor(value: T) { this.value = value; } } class C extends B<number> {} new C(7)[\"value\"];", 7.0),
            ("class C { static [key: string]: number; static value = 7; } C[\"value\"];", 7.0),
            ("class C { readonly [key: string]: number; value = 3; } const item = new C(); item[\"value\"] = 7; item.value;", 7.0),
            ("const C = class { value = 7; }; new C().value;", 7.0),
            ("const C = class<T> { constructor(public value: T) {} }; new C(7).value;", 7.0),
            ("class B { value = 7; } const C = class extends B {}; new C().value;", 7.0),
            ("const C = class Inner { value = 7; clone(): Inner { return new Inner(); } }; new C().clone().value;", 7.0),
            ("function make() { return class { value = 7; }; } const C = make(); new C().value;", 7.0),
            ("function make<T>(value: T) { return class { item: T = value; }; } const C = make(7); const result: number = new C().item; result;", 7.0),
            ("function make<T>(value: T) { return class Inner { item: T = value; clone(): Inner { return new Inner(); } }; } const C = make(7); const result: number = new C().clone().item; result;", 7.0),
            ("const B = class<T> { constructor(public value: T) {} }; const C = class extends B<number> {}; const result: number = new C(7).value; result;", 7.0),
            ("const B = class<T> { constructor(public value: T) {} }; const Alias = B; const C = class extends Alias<number> {}; const result: number = new C(7).value; result;", 7.0),
            ("function make<T>(value: T) { return class { item: T = value; }; } const B = make(7); const C = class extends B {}; const result: number = new C().item; result;", 7.0),
            ("const Anonymous = class { value = 3; }; const Named = class Inner { value = 4; }; Anonymous.name === \"Anonymous\" && Named.name === \"Inner\" ? new Anonymous().value + new Named().value : 0;", 7.0),
            ("let count = 0; class Keys { static get value(): \"value\" { count++; return \"value\"; } } class C { [Keys.value] = 7; } const first: any = new C(); const second: any = new C(); first.value + second.value + count;", 15.0),
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
            assert_eq!(Vm::default().execute(&artifact.bytecode).unwrap(), Value::Number(expected));
        }
    }
}

#[test]
fn index_contracts_keep_abstract_constructor_and_readonly_rejections() {
    for source in [
        "abstract class C { [key: string]: number; } const Alias = C; new Alias();",
        "class C { readonly [key: string]: number; value = 3; } const item = new C(); item[\"other\"] = 4;",
        "class C { [key: string]: number; value = \"wrong\"; } new C();",
        "function make<T>(value: T) { return class { item: T = value; }; } const C = make(7); const result: string = new C().item;",
        "const B = class<T> { constructor(public value: T) {} }; const C = class extends B<number> {}; new C(\"wrong\");",
        "function make<T>(value: T) { return class Inner { item: T = value; clone(): Inner { return new Inner(); } }; } const C = make(7); const result: string = new C().clone().item;",
        "function make<T>(value: T) { return class { item: T = value; }; } const B = make(7); const C = class extends B {}; const result: string = new C().item;",
    ] {
        assert!(compile_direct_script(
            "memory:///main.ts",
            &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
            CompilerOptions { checking: Some(CheckingOptions::default()), ..CompilerOptions::default() },
        ).is_err(), "{source}");
    }
}

#[test]
fn computed_method_names_are_evaluated_once_when_the_class_is_created() {
    let source = "let count = 0; class C { [++count](): number { return count; } } const first: any = new C(); const second: any = new C(); first[1]() + second[1]() + count;";
    let artifact = compile_direct_script(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
        CompilerOptions {
            checking: Some(CheckingOptions::default()),
            ..CompilerOptions::default()
        },
    )
    .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(
        Vm::default().execute(&artifact.bytecode).unwrap(),
        Value::Number(3.0)
    );
}

#[test]
fn es2020_expression_fields_and_private_state_are_created_per_evaluation() {
    for define_fields in [true, false] {
        for (source, expected) in [
            ("const C = class { static value: number = 7; }; C.name === \"C\" ? C.value : 0;", 7.0),
            ("const C = class Inner { static value: number = 7; read(): number { return Inner.value; } }; C.name === \"Inner\" ? new C().read() : 0;", 7.0),
            ("function make(value: number) { return class Inner { #value: number = value; static #count: number = 2; #read(): number { return this.#value; } read(): number { return this.#read() + Inner.#count; } }; } const A = make(3); const B = make(4); new A().read() + new B().read();", 11.0),
            ("let count = 0; class Keys { static get value(): \"first\" { return (++count === 1 ? \"first\" : \"second\") as \"first\"; } } function make() { return class { [Keys.value]: number = 7; }; } const A = make(); const B = make(); const a: any = new A(); const b: any = new B(); a.first + b.second + count;", 16.0),
            ("class Holder { C: { new(): { item: number }; value: number } = class { static value: number = 7; item: number = 5; }; } const C = new Holder().C; C.name === \"C\" ? C.value + new C().item : 0;", 12.0),
        ] {
            let artifact = compile_direct_script(
                "memory:///main.ts",
                &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
                CompilerOptions {
                    target: EcmaTarget::Es2020,
                    use_define_for_class_fields: Some(define_fields),
                    checking: Some(CheckingOptions::default()),
                    ..CompilerOptions::default()
                },
            )
            .unwrap_or_else(|error| panic!("define fields {define_fields}: {source}: {error:?}"));
            assert_eq!(Vm::default().execute(&artifact.bytecode).unwrap(), Value::Number(expected));
        }
    }
}
