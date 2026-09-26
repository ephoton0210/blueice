// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn accepts_a_structurally_compatible_record() {
    let loader = MapLoader::from([ModuleSource::new(
        "memory:///app.ts",
        "interface User { name: string; age?: number } const user: User = { name: 'Ada' };",
    )]);
    let result = crate::compile("memory:///app.ts", &loader, CompilerOptions::default());
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
}

#[test]
fn checks_generic_interface_heritage_structurally() {
    let result = crate::compile(
            "memory:///main.ts",
            &MapLoader::from([ModuleSource::new(
                "memory:///main.ts",
                "interface Envelope<T> { payload: T }\n\
                 interface Tagged { tag: string }\n\
                 interface Labeled<T extends string = string> extends Envelope<T>, Tagged { label: T }\n\
                 const valid: Labeled = { payload: 'Ada', tag: 'account', label: 'user' };\n\
                 const parent: Envelope<string> = valid;\n\
                 const property: string = valid.payload;\n\
                 const tag: string = valid.tag;\n\
                 const invalid: Labeled = { payload: 1, tag: 'account', label: 'user' };",
            )]),
            CompilerOptions::default(),
        );
    assert!(result.has_errors());
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch)
            .count(),
        1
    );
}

#[test]
fn rejects_a_type_alias_as_interface_heritage() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "type Scalar = number; interface Invalid extends Scalar { label: string }",
        )]),
        CompilerOptions::default(),
    );
    assert!(result.has_errors());
    assert!(result
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == DiagnosticCode::UnsupportedSyntax));
}

#[test]
fn rejects_an_interface_property_that_conflicts_with_heritage() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "interface Base { id: string }\ninterface Invalid extends Base { id: number }",
        )]),
        CompilerOptions::default(),
    );
    assert!(result.has_errors());
    assert!(result.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::TypeMismatch
            && diagnostic
                .message
                .contains("property `id` is not compatible")
    }));
}

#[test]
fn instantiates_inherited_declaration_interfaces_across_type_imports() {
    let result = crate::compile(
            "memory:///main.ts",
            &MapLoader::from([
                ModuleSource::new(
                    "memory:///main.ts",
                    "import type { Labeled } from './types/model.d.ts';\n\
                     const valid: Labeled = { payload: 'Ada', tag: 'account', label: 'user' };\n\
                     const payload: string = valid.payload;\n\
                     const tag: string = valid.tag;\n\
                     const invalid: Labeled = { payload: 1, tag: 'account', label: 'user' };",
                ),
                ModuleSource::new(
                    "memory:///types/model.d.ts",
                    "interface Envelope<T> { payload: T }\n\
                     interface Tagged { tag: string }\n\
                     export interface Labeled<T extends string = string> extends Envelope<T>, Tagged { label: T }",
                ),
            ]),
            CompilerOptions::default(),
        );
    assert!(result.has_errors());
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch)
            .count(),
        1
    );
}

#[test]
fn checks_and_erases_a_local_typed_variable_before_its_return() {
    let loader = MapLoader::from([ModuleSource::new(
        "memory:///app.ts",
        "function count(): number { const local: number = 'wrong'; return local; }",
    )]);
    let result = crate::compile("memory:///app.ts", &loader, CompilerOptions::default());
    assert!(result
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch));
    assert!(result.output.is_none());
}

#[test]
fn resolves_a_type_through_a_type_only_reexport() {
    let loader = MapLoader::from([
        ModuleSource::new(
            "memory:///main.ts",
            "import type { PublicUser } from './api.ts'; const user: PublicUser = { id: 'ada' };",
        ),
        ModuleSource::new(
            "memory:///api.ts",
            "export type { User as PublicUser } from './model.ts';",
        ),
        ModuleSource::new("memory:///model.ts", "export interface User { id: string }"),
    ]);
    let result = crate::compile("memory:///main.ts", &loader, CompilerOptions::default());
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
}

#[test]
fn instantiates_generic_aliases_for_structural_assignability() {
    let result = crate::compile(
            "memory:///main.ts",
            &MapLoader::from([ModuleSource::new(
                "memory:///main.ts",
                "type Box<T> = { value: T };\nconst valid: Box<number> = { value: 1 };\nconst invalid: Box<number> = { value: 'wrong' };",
            )]),
            CompilerOptions::default(),
        );
    assert!(result.has_errors());
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch)
            .count(),
        1
    );
}

#[test]
fn instantiates_generic_declaration_interfaces_across_type_imports() {
    let result = crate::compile(
            "memory:///main.ts",
            &MapLoader::from([
                ModuleSource::new(
                    "memory:///types/envelope.d.ts",
                    "export interface Envelope<T> { payload: T }",
                ),
                ModuleSource::new(
                    "memory:///main.ts",
                    "import type { Envelope } from './types/envelope.d.ts';\nconst valid: Envelope<string> = { payload: 'ok' };\nconst invalid: Envelope<string> = { payload: 1 };",
                ),
            ]),
            CompilerOptions::default(),
        );
    assert!(result.has_errors());
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch)
            .count(),
        1
    );
}

#[test]
fn infers_a_generic_function_return_from_its_argument() {
    let result = crate::compile(
            "memory:///main.ts",
            &MapLoader::from([ModuleSource::new(
                "memory:///main.ts",
                "function identity<T>(value: T): T { return value; }\nconst valid: string = identity('ok');\nconst invalid: number = identity('wrong');",
            )]),
            CompilerOptions::default(),
        );
    assert!(result.has_errors());
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch)
            .count(),
        1
    );
}

#[test]
fn does_not_leak_declaration_type_parameters_into_module_scope() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "type Box<T> = { value: T };\nconst leaked: T = 'wrong';",
        )]),
        CompilerOptions::default(),
    );
    assert!(result.has_errors());
    assert!(result
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == DiagnosticCode::UnknownType));
}

#[test]
fn rejects_the_wrong_number_of_generic_arguments() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "type Box<T> = { value: T };\nconst invalid: Box = { value: 1 };",
        )]),
        CompilerOptions::default(),
    );
    assert!(result.has_errors());
    assert!(result
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch));
}

#[test]
fn checks_arguments_of_known_function_calls() {
    let result = crate::compile(
            "memory:///main.ts",
            &MapLoader::from([ModuleSource::new(
                "memory:///main.ts",
                "function takesNumber(value: number): number { return value; }\nconst accepted: number = takesNumber(1);\nconst rejected: number = takesNumber('wrong');",
            )]),
            CompilerOptions::default(),
        );
    assert!(result.has_errors());
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch)
            .count(),
        1
    );
}

#[test]
fn resolves_local_function_overloads_for_direct_calls() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "function describe(value: string): string;\n\
                 function describe(value: number): number;\n\
                 function describe(value: string | number): string | number { return value; }\n\
                 const label: string = describe('Ada');\n\
                 const count: number = describe(1);\n\
                 const mismatch: string = describe(1);\n\
                 const invalid: unknown = describe(true);",
        )]),
        CompilerOptions::default(),
    );
    assert!(result.has_errors());
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch)
            .count(),
        2
    );
}

#[test]
fn rejects_overloads_without_a_compatible_implementation() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "function missing(value: string): string;\n\
                 function describe(value: string): string;\n\
                 function describe(value: number): number;\n\
                 function describe(value: string): string { return value; }\n\
                 function optional(value?: string): string;\n\
                 function optional(value: string): string { return value; }",
        )]),
        CompilerOptions::default(),
    );
    assert!(result.has_errors());
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch)
            .count(),
        3
    );
}

#[test]
fn checks_explicit_direct_function_type_arguments() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "function identity<T extends string = string>(value: T): T { return value; }\n\
                 const accepted: string = identity<string>('Ada');\n\
                 const composed: string = identity<string>('Ada') + ' Lovelace';\n\
                 const invalid_argument: string = identity<string>(1);\n\
                 const invalid_constraint: unknown = identity<number>(1);\n\
                 const too_many: unknown = identity<string, number>('Ada');",
        )]),
        CompilerOptions::default(),
    );
    assert!(result.has_errors());
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch)
            .count(),
        3
    );
}

#[test]
fn retains_a_known_return_type_when_optional_arguments_are_omitted() {
    let result = crate::compile(
            "memory:///main.ts",
            &MapLoader::from([ModuleSource::new(
                "memory:///main.ts",
                "function count(value?: number): number { return 1; }\nconst invalid: string = count();",
            )]),
            CompilerOptions::default(),
        );
    assert!(result.has_errors());
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch)
            .count(),
        1
    );
}

#[test]
fn treats_defaulted_parameters_as_omittable_at_call_sites() {
    let result = crate::compile(
            "memory:///main.ts",
            &MapLoader::from([ModuleSource::new(
                "memory:///main.ts",
                "function count(value: number = 1): number { return value; }\nconst invalid: string = count();",
            )]),
            CompilerOptions::default(),
        );
    assert!(result.has_errors());
    assert!(result.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::TypeMismatch
            && diagnostic.message.contains("initializer has type `number`")
    }));
}

#[test]
fn infers_named_interface_properties_in_function_returns() {
    let result = crate::compile(
            "memory:///main.ts",
            &MapLoader::from([ModuleSource::new(
                "memory:///main.ts",
                "interface Account { id: string }\nfunction label(value: Account): string { return value.id; }",
            )]),
            CompilerOptions::default(),
        );
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
}

#[test]
fn checks_direct_call_and_record_property_boundaries() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "interface Account { id: string; revision?: number }\n\
                 const account: Account = { id: 'ada' };\n\
                 const id: string = account.id;\n\
                 const optionalAsNumber: number = account.revision;\n\
                 const missing: string = account.missing;\n\
                 function label(value: string, revision?: number): string { return value; }\n\
                 const accepted: string = label('Ada');\n\
                 const wrongArgument: string = label('Ada', 'wrong');\n\
                 const tooFew: string = label();\n\
                 const tooMany: string = label('Ada', 1, 2);\n\
                 function count(value: number = 1): number { return value; }\n\
                 const defaulted: number = count();",
        )]),
        CompilerOptions::default(),
    );
    assert!(result.has_errors());
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch)
            .count(),
        5,
        "{:#?}",
        result.diagnostics
    );
    assert!(result.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("property `missing` does not exist")
    }));
}

#[test]
fn checks_record_literal_fields_for_direct_property_reads() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "interface Person { name: string; age: number }\n\
             const person: Person = { name: 'Ada', age: 42 };\n\
             const label: string = person.name;\n\
             const invalid: number = person.name;",
        )]),
        CompilerOptions::default(),
    );
    assert!(result.has_errors());
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch)
            .count(),
        1,
        "{:#?}",
        result.diagnostics
    );
}

#[test]
fn infers_record_shorthand_fields_from_local_bindings() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "const label: string = 'Ada';\n\
             const person: { label: string } = { label };\n\
             const invalid: number = person.label;",
        )]),
        CompilerOptions::default(),
    );
    assert!(result.has_errors());
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch)
            .count(),
        1,
        "{:#?}",
        result.diagnostics
    );
}

#[test]
fn checks_template_literal_string_annotations() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            r"const label: string = `BlueTS`; const invalid: number = `BlueTS`;",
        )]),
        CompilerOptions::default(),
    );
    assert!(result.has_errors());
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch)
            .count(),
        1,
        "{:#?}",
        result.diagnostics
    );
}

#[test]
fn checks_optional_fields_and_explicit_undefined_arguments() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "interface OptionalName { name?: string }\n\
                 interface RequiredName { name: string }\n\
                 const optional: OptionalName = {};\n\
                 const wider: string | number | undefined = optional.name;\n\
                 const required: string = optional.name;\n\
                 const incompatible: RequiredName = optional;\n\
                 function count(value: number = 1): number { return value; }\n\
                 const defaulted: number = count(undefined);\n\
                 const invalid: number = count('wrong');",
        )]),
        CompilerOptions::default(),
    );
    assert!(result.has_errors());
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch)
            .count(),
        3,
        "{:#?}",
        result.diagnostics
    );
}

#[test]
fn accepts_a_constrained_generic_overload_with_a_concrete_implementation() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "function label<T extends string>(value: T): T;\n\
                 function label(value: string): string { return value; }\n\
                 const result: string = label('Ada');",
        )]),
        CompilerOptions::default(),
    );
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
}

#[test]
fn instantiates_generic_record_properties_before_assignment_checks() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "type Box<T> = { value: T };\n\
                 const box: Box<number> = { value: 1 };\n\
                 const accepted: number = box.value;\n\
                 const rejected: string = box.value;",
        )]),
        CompilerOptions::default(),
    );
    assert!(result.has_errors());
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch)
            .count(),
        1,
        "{:#?}",
        result.diagnostics
    );
}

#[test]
fn applies_generic_defaults_and_rejects_constraint_violations() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "type Box<T extends string = string> = { value: T };\n\
                 const defaulted: Box = { value: 'ok' };\n\
                 const rejected: Box<number> = { value: 1 };",
        )]),
        CompilerOptions::default(),
    );
    assert!(result.has_errors());
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch)
            .count(),
        1,
        "{:#?}",
        result.diagnostics
    );
    assert!(result.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("does not satisfy constraint `string`")
    }));
}

#[test]
fn uses_defaulted_function_type_parameters_and_checks_inferred_constraints() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "function echo<T extends string = string>(value?: T): string { return ''; }\n\
                 const defaulted: string = echo();\n\
                 const constrained: unknown = echo(1);",
        )]),
        CompilerOptions::default(),
    );
    assert!(result.has_errors());
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch)
            .count(),
        1,
        "{:#?}",
        result.diagnostics
    );
    assert!(result.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("inferred type `number` does not satisfy constraint `string`")
    }));
}

#[test]
fn bounds_generic_alias_expansion_work() {
    let mut source = String::from("type Alias0 = { value: number };\n");
    for index in 1..=8 {
        source.push_str(&format!("type Alias{index} = Alias{};\n", index - 1));
    }
    source.push_str("const value: Alias8 = { value: 'wrong' };");
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
        CompilerOptions {
            limits: crate::CompilerLimits {
                max_type_expansions: 2,
                ..crate::CompilerLimits::default()
            },
            ..CompilerOptions::default()
        },
    );
    assert!(result.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::ResourceLimit
            && diagnostic.message.contains("generic-expansion limit")
    }));
    assert!(result.output.is_none());
}

#[test]
fn fails_closed_when_overload_selection_exhausts_generic_expansion_budget() {
    let mut source = String::from("type Alias0 = { value: number };\n");
    for index in 1..=6 {
        source.push_str(&format!("type Alias{index} = Alias{};\n", index - 1));
    }
    source.push_str(
        "function choose(value: Alias6): Alias6;\n\
             function choose(value: string): string;\n\
             function choose(value: Alias6 | string): Alias6 | string { return value; }\n\
             const selected = choose({ value: 1 });",
    );
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
        CompilerOptions {
            limits: crate::CompilerLimits {
                max_type_expansions: 2,
                ..crate::CompilerLimits::default()
            },
            ..CompilerOptions::default()
        },
    );
    assert!(result.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::ResourceLimit
            && diagnostic.message.contains("generic-expansion limit")
    }));
    assert!(result.output.is_none());
}
