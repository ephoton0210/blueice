// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use crate::compiler::{CompilerOptions, MapLoader, ModuleSource};

mod member_calls;
mod project_contracts;
mod readonly;
mod readonly_assertions;
mod readonly_assignment_results;
mod readonly_logical_assignment_results;
mod readonly_logical_results;
mod readonly_opaque_receivers;
mod readonly_record_spread;
mod readonly_record_spread_union;
mod readonly_record_spread_unknown;
mod readonly_sequence;
mod readonly_spread_results;

#[test]
fn checks_named_callbacks_against_function_type_and_literal_event_name() {
    let ambient = ModuleSource::new(
        "memory:///events.d.ts",
        "interface ClickEvent { preventDefault(): void; }\n\
         interface Node { addEventListener(eventType: 'click', listener: (event: ClickEvent) => void): void; }\n\
         declare const node: Node;",
    );
    let check = |source| {
        crate::compile(
            "memory:///main.ts",
            &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
            CompilerOptions {
                ambient_declaration_modules: vec![ambient.clone()],
                require_declared_global_calls: true,
                ..CompilerOptions::default()
            },
        )
    };
    let valid = check(
        "function onClick(event: ClickEvent): void { event.preventDefault(); }\n\
         node.addEventListener('click', onClick);",
    );
    assert!(!valid.has_errors(), "{:#?}", valid.diagnostics);
    for source in [
        "node.addEventListener('change', onClick);",
        "node.addEventListener('click', 'not callable');",
        "function wrong(event: string): void {} node.addEventListener('click', wrong);",
    ] {
        let invalid = check(source);
        assert!(
            invalid
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch),
            "{source}: {:#?}",
            invalid.diagnostics
        );
    }
}

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
fn checks_direct_calls_in_braced_if_conditions() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "function takes_number(value: number): number { return value; }\n\
             function choose(): number {\n\
                 if (takes_number('wrong')) { return 1; }\n\
                 return 0;\n\
             }",
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
fn checks_direct_calls_in_braced_else_if_conditions() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "function takes_number(value: number): number { return value; }\n\
             function choose(): number {\n\
                 if (false) { return 0; }\n\
                 else if (takes_number('wrong')) { return 1; }\n\
                 else { return 2; }\n\
             }",
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
fn accepts_array_holes_without_confusing_them_with_expression_elements() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "const values = [1,,3];",
        )]),
        CompilerOptions::default(),
    );
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
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
fn requires_non_undefined_function_returns_on_every_structured_path() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "function complete(value: number): number {\n\
                 if (value > 0) { return 1; }\n\
                 else if (value < 0) { throw 'negative'; }\n\
                 else { return 0; }\n\
             }\n\
             function missing_else(value: number): number { if (value > 0) { return 1; } }\n\
             function bare_return(): number { return; }\n\
             function may_fall_through(value: number): number | undefined { if (value > 0) { return 1; } }\n\
             function unit(): void { return; }\n\
             function unknown_result(): unknown {}\n\
             function any_result(): any {}\n\
             function throw_only(): number { throw 'broken'; }",
        )]),
        CompilerOptions::default(),
    );
    let return_errors = result
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == DiagnosticCode::ReturnTypeMismatch)
        .collect::<Vec<_>>();
    assert_eq!(return_errors.len(), 2, "{:#?}", result.diagnostics);
    assert!(
        return_errors.iter().any(|diagnostic| diagnostic
            .message
            .contains("can complete without returning")),
        "{return_errors:#?}"
    );
    assert!(
        return_errors
            .iter()
            .any(|diagnostic| diagnostic.message.contains("type `undefined`")),
        "{return_errors:#?}"
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
        infer_logical_expression(Type::Number, Type::Boolean),
        Type::Union(vec![Type::Number, Type::Boolean])
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
