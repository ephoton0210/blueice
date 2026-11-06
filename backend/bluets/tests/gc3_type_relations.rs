// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Ordinary type boundaries shared by decorator calls and replacements.
use blueice_bluets::{compile, CheckingOptions, CompilerOptions, MapLoader, ModuleSource};

fn accepts(source: &str) -> bool {
    !compile(
        "main.ts",
        &MapLoader::from([ModuleSource::new("main.ts", source)]),
        CompilerOptions::default(),
    )
    .has_errors()
}

#[test]
fn intersection_members_check_each_callback_receiver() {
    for receiver in ["Base", "Subject"] {
        let source = format!(
            "interface Base {{ state: string; }}
             interface Subject {{ state: string; value: number; }}
             interface Context<T> {{ kind: string; name: string; access: {{ has: (object: T) => boolean }}; }}
             declare const input: Context<Subject> & {{ name: 'value' }};
             const output: Context<{receiver}> = input;"
        );
        assert_eq!(accepts(&source), receiver == "Subject", "{source}");
    }
}

#[test]
fn constructor_relations_preserve_generic_constraints() {
    for constraint in ["string", "number"] {
        let source = format!(
            "interface Expected {{ new <T extends string>(value: T): {{ value: T }}; }}
             interface Actual {{ new <U extends {constraint}>(value: U): {{ value: U }}; }}
             declare const input: Actual;
             const output: Expected = input;"
        );
        assert_eq!(accepts(&source), constraint == "string", "{source}");
    }
}

#[test]
fn construct_signatures_have_an_implicit_prototype_without_granting_static_members() {
    let source = "interface Shape { value: number; read(): number; }
        abstract class Base { constructor(public value: number) {} abstract read(): number; }
        declare const factory: new (value: number) => Shape;
        const output: typeof Base = factory;";
    assert!(accepts(source));
    assert!(!accepts(&source.replace(
        "abstract class Base {",
        "abstract class Base { static readonly marker = 'base';"
    )));
}

#[test]
fn logical_operators_keep_only_the_returned_left_branch() {
    assert!(accepts(
        "declare const input: string | undefined;
         const output: string = input || 'fallback';"
    ));
    assert!(!accepts(
        "declare const input: string | undefined;
         const output: string = input && 'suffix';"
    ));
    assert!(!accepts(
        "declare const input: string | undefined;
         const output: string = input || undefined;"
    ));
}

#[test]
fn named_constraint_only_arguments_do_not_erase_direct_signature_constraints() {
    let declarations = "type Animal = {name:string}; type Dog = {name:string;bark:number};
        type Factory<T> = {run:<U extends T>(value:U)=>U};
        type Constructor<T> = {new<U extends T>(value:U):{value:U}};
        type FnAlias<T> = <U extends T>(value:U)=>U;
        type Carrier<T> = {value:T;run:<U extends T>(value:U)=>U};
        declare const animalFactory: Factory<Animal>; declare const dogFactory: Factory<Dog>;
        declare const stringFactory: Factory<string>; declare const numberFactory: Factory<number>;
        declare const animalAlias: FnAlias<Animal>; declare const dogAlias: FnAlias<Dog>;
        declare const animalCarrier: Carrier<Animal>; declare const dogCarrier: Carrier<Dog>;
        declare const animalMember: typeof animalFactory.run; declare const dogMember: typeof dogFactory.run;
        declare const animalFunction: <U extends Animal>(value:U)=>U;
        declare const dogFunction: <U extends Dog>(value:U)=>U;
        declare const stringFunction: <U extends string>(value:U)=>U;
        declare const numberFunction: <U extends number>(value:U)=>U;
        declare const animalConstructor: Constructor<Animal>; declare const dogConstructor: Constructor<Dog>;
        declare const stringConstructor: Constructor<string>; declare const numberConstructor: Constructor<number>;
        interface DirectAnimalConstructor {new<U extends Animal>(value:U):{value:U}}
        interface DirectDogConstructor {new<U extends Dog>(value:U):{value:U}}
        interface DirectStringConstructor {new<U extends string>(value:U):{value:U}}
        interface DirectNumberConstructor {new<U extends number>(value:U):{value:U}}
        declare const directAnimalConstructor: DirectAnimalConstructor;
        declare const directDogConstructor: DirectDogConstructor;
        declare const directStringConstructor: DirectStringConstructor;
        declare const directNumberConstructor: DirectNumberConstructor;";
    for (actual, expected, accepted) in [
        ("dogAlias", "animalAlias", true),
        ("animalAlias", "dogAlias", true),
        ("dogCarrier", "animalCarrier", true),
        ("animalCarrier", "dogCarrier", false),
        ("dogMember", "animalMember", true),
        ("animalMember", "dogMember", true),
        ("dogFactory", "animalFactory", true),
        ("animalFactory", "dogFactory", true),
        ("numberFactory", "stringFactory", true),
        ("dogFunction", "animalFunction", false),
        ("animalFunction", "dogFunction", true),
        ("numberFunction", "stringFunction", false),
        ("dogConstructor", "animalConstructor", true),
        ("animalConstructor", "dogConstructor", true),
        ("numberConstructor", "stringConstructor", true),
        ("directDogConstructor", "directAnimalConstructor", false),
        ("directAnimalConstructor", "directDogConstructor", true),
        ("directNumberConstructor", "directStringConstructor", false),
    ] {
        let source = format!("{declarations} export const result: typeof {expected} = {actual};");
        let result = compile(
            "main.ts",
            &MapLoader::from([ModuleSource::new("main.ts", source)]),
            CompilerOptions {
                checking: Some(CheckingOptions::default()),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            !result.has_errors(),
            accepted,
            "{actual} -> {expected}: {:?}",
            result.diagnostics
        );
    }
}
