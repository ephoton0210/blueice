// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use crate::compiler::{CompilerOptions, MapLoader, ModuleSource};

#[test]
fn rejects_a_primitive_initializer_with_the_wrong_annotation() {
    let loader = MapLoader::from([ModuleSource::new(
        "memory:///app.ts",
        "const count: number = 'one';",
    )]);
    let result = crate::compile("memory:///app.ts", &loader, CompilerOptions::default());
    assert!(result.has_errors());
    assert!(result
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch));
}

#[test]
fn checks_direct_calls_in_function_expression_statements() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "function takes_number(value: number): number { return value; }\n\
             function invalid(): void { takes_number('wrong'); }",
        )]),
        CompilerOptions::default(),
    );
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
fn checks_direct_calls_in_function_throw_statements() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "function takes_number(value: number): number { return value; }\n\
             function fail(): never { throw takes_number('wrong'); }",
        )]),
        CompilerOptions::default(),
    );
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
fn rejects_function_throw_statements_without_a_same_line_value() {
    for source in [
        "function fail(): never { throw; }",
        "function fail(): never { throw\n'broken'; }",
    ] {
        let result = crate::compile(
            "memory:///main.ts",
            &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
            CompilerOptions::default(),
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == DiagnosticCode::ParseError),
            "{source}: {:#?}",
            result.diagnostics
        );
    }
}

#[test]
fn infers_boolean_comparisons_and_conditional_branch_types() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "const count: number = 41;\n\
             const exact: boolean = count + 1 === 42;\n\
             const bounded: boolean = (exact || false) && count < 42 && !false;\n\
             const positive: number = +count;\n\
             const signed: number = -count;\n\
             const inverted: number = ~count;\n\
             const selected: number = bounded ? count + 1 : count - 1;\n\
             const nested: number = false ? true ? count + 1 : count - 1 : count;\n\
             function enabled(value: number): boolean { return value >= 0 ? !false : false; }\n\
             const invalid: boolean = selected ? count : 'no';",
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
fn infers_in_and_instanceof_expressions_as_boolean() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "const record: { label: string } = { label: 'Ada' };\n\
             const hasLabel: boolean = 'label' in record;\n\
             const isObject: boolean = record instanceof Object;\n\
             const invalid: string = 'label' in record;",
        )]),
        CompilerOptions::default(),
    );
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
fn infers_typeof_and_void_unary_expression_types() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "const kind: string = typeof 1;\n\
             const absent: undefined = void 1;\n\
             const invalid: number = typeof false;",
        )]),
        CompilerOptions::default(),
    );
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
fn infers_array_literal_element_types() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "const values: number[] = [1, 2, 3];\n\
             const invalid: string[] = [1, 2, 3];",
        )]),
        CompilerOptions::default(),
    );
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
fn infers_known_array_spread_elements() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "const suffix: number[] = [2, 3];\n\
             const values: number[] = [1, ...suffix, 4];\n\
             const invalid: string[] = [1, ...suffix];",
        )]),
        CompilerOptions::default(),
    );
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
fn infers_known_record_spread_properties() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "const source: { label: string } = { label: 'Ada' };\n\
             const person: { label: string; title: string } = { ...source, label: 'Grace', title: 'Countess' };\n\
             const invalid: { label: number; title: string } = { ...source, label: 'Grace', title: 'Countess' };",
        )]),
        CompilerOptions::default(),
    );
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
fn expands_tuple_spread_arguments_for_direct_function_calls() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "function add(left: number, right: number): number { return left + right; }\n\
             const pair: [number, number] = [40, 2];\n\
             const value: number = add(...pair);\n\
             const array: number[] = [40, 2];\n\
             const invalid: number = add(...array);",
        )]),
        CompilerOptions::default(),
    );
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
fn checks_array_typed_rest_parameters_for_normal_and_tuple_spread_calls() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "function sum(base: number, ...values: number[]): number { return base + values[0]; }\n\
             const pair: [number] = [2];\n\
             const fromSpread: number = sum(40, ...pair);\n\
             const fromArguments: number = sum(20, 22);\n\
             const invalid: number = sum(40, 'two');",
        )]),
        CompilerOptions::default(),
    );
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
fn infers_generic_returns_from_array_typed_rest_parameters() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "function first<T>(...values: T[]): T { return values[0]; }\n\
             const value: number = first(42, 7);\n\
             const invalid: string = first(42, 7);",
        )]),
        CompilerOptions::default(),
    );
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
fn rejects_rest_parameters_that_are_not_final_array_parameters() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "function misplaced(...values: number[], tail: number): number { return tail; }\n\
             function scalar(...value: number): number { return value; }",
        )]),
        CompilerOptions::default(),
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch)
            .count(),
        2,
        "{:#?}",
        result.diagnostics
    );
}

#[test]
fn checks_default_parameter_initializers_and_omitted_calls() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "function add(left: number, right: number): number { return left + right; }\n\
             function scale(value: number = add(20, 1), multiplier: number = 2): number { return value * multiplier; }\n\
             const omitted: number = scale();\n\
             const explicitUndefined: number = scale(undefined);\n\
             const invalid: number = scale('wrong');",
        )]),
        CompilerOptions::default(),
    );
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
fn checks_optional_parameter_values_in_function_bodies() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "function label(value?: string): string { return value ?? 'guest'; }\n\
             const omitted: string = label();\n\
             const explicitUndefined: string = label(undefined);\n\
             const invalidArgument: string = label(1);\n\
             function invalidReturn(value?: number): number { return value; }",
        )]),
        CompilerOptions::default(),
    );
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
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == DiagnosticCode::ReturnTypeMismatch)
            .count(),
        1,
        "{:#?}",
        result.diagnostics
    );
}

#[test]
fn rejects_a_default_parameter_initializer_with_the_wrong_type() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "function invalid(value: number = 'wrong'): number { return value; }",
        )]),
        CompilerOptions::default(),
    );
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
fn infers_nullish_coalescing_after_excluding_null_and_undefined() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "const optional: string | undefined = undefined;\n\
             const label: string = optional ?? 'guest';\n\
             const count: number = null ?? 42;\n\
             const invalid: number = undefined ?? 'guest';",
        )]),
        CompilerOptions::default(),
    );
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
fn infers_bitwise_and_shift_expressions_and_rejects_known_non_numbers() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "const shifted: number = 20 << 1;\n\
             const combined: number = ((shifted | 1) & 62) ^ 10;\n\
             const signed: number = 20 >> 1;\n\
             const unsigned: number = 20 >>> 1;\n\
             const invalid: number = 'BlueIce' & 1;",
        )]),
        CompilerOptions::default(),
    );
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
fn infers_right_associative_exponentiation_and_rejects_known_non_numbers() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "const chained: number = 2 ** 3 ** 2;\n\
             const reciprocal: number = 2 ** -3;\n\
             const squared: number = (-2) ** 2;\n\
             const invalid: number = 'BlueIce' ** 2;",
        )]),
        CompilerOptions::default(),
    );
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
fn keeps_bounded_expression_inference_structural() {
    let grouped = crate::syntax::lex("memory:///tokens.ts", "((flag))").unwrap();
    let grouped = &grouped[..grouped.len() - 1];
    assert_eq!(strip_outer_parentheses(grouped)[0].text, "flag");

    let logical = crate::syntax::lex("memory:///tokens.ts", "left && (right || tail)").unwrap();
    let logical = &logical[..logical.len() - 1];
    let (left, _, right) = top_level_binary_parts(logical, &["&&"], |_| false).unwrap();
    assert_eq!(left[0].text, "left");
    assert_eq!(right[0].text, "(");

    let conditional =
        crate::syntax::lex("memory:///tokens.ts", "condition ? inner ? 1 : 2 : 3").unwrap();
    let conditional = &conditional[..conditional.len() - 1];
    let (condition, consequent, alternate) = conditional_expression_parts(conditional).unwrap();
    assert_eq!(condition[0].text, "condition");
    assert_eq!(consequent[0].text, "inner");
    assert_eq!(alternate[0].text, "3");

    assert_eq!(
        infer_boolean_logical_expression(Type::Number, Type::Boolean),
        Type::Unknown
    );
    assert_eq!(
        merge_conditional_branch_types(Type::Number, Type::String),
        Type::Union(vec![Type::Number, Type::String])
    );
}

#[test]
fn infers_generic_direct_calls_inside_arithmetic_expressions() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "function identity<T>(value: T): T { return value; }\n\
             const sum: number = identity<number>(41) + 1;\n\
             const difference: number = identity<number>(41) - 1;\n\
             const product: number = identity<number>(41) * 2;\n\
             const quotient: number = identity<number>(41) / 2;\n\
             const remainder: number = identity<number>(41) % 2;\n\
             const label: string = identity<string>('Ada') + ' Lovelace';\n\
             const invalid: string = identity<number>(41) + 1;",
        )]),
        CompilerOptions::default(),
    );
    assert!(result.has_errors(), "{:#?}", result.diagnostics);
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
fn rejects_known_invalid_arithmetic_operands_in_initializers_and_returns() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "const label: string = 'Ada';\n\
             const invalid: number = label - 1;\n\
             function invalid_return(): number { return false * 2; }",
        )]),
        CompilerOptions::default(),
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch)
            .count(),
        2,
        "{:#?}",
        result.diagnostics
    );
    assert!(result.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("operator `-` cannot be applied to types `string` and `number`")
    }));
}

#[test]
fn rejects_strict_equality_between_disjoint_known_primitives() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "const invalid: boolean = 1 === 'one';\n\
             function invalid_return(): boolean { return false !== 0; }",
        )]),
        CompilerOptions::default(),
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch)
            .count(),
        2,
        "{:#?}",
        result.diagnostics
    );
    assert!(result.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("operator `===` compares disjoint types `number` and `string`")
    }));
}

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
