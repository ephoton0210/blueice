// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Enums (J.3.4): member values, emitted text, typing and diagnostics.
//! Verdicts on real programs are pinned against TypeScript by the `enum-*`
//! matrix fixtures and the runtime oracle cases; these tests pin the codes,
//! messages and text.

use blueice_bluets::{
    compile, CompilerOptions, Diagnostic, DiagnosticCode, MapLoader, ModuleSource,
};

const ENTRY: &str = "memory:///main.ts";

fn compile_with(source: &str) -> blueice_bluets::Compilation {
    compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions::default(),
    )
}

fn diagnostics(source: &str) -> Vec<Diagnostic> {
    compile_with(source).diagnostics
}

#[track_caller]
fn assert_rejected(source: &str, code: DiagnosticCode, message: &str) {
    let found = diagnostics(source);
    assert!(
        found
            .iter()
            .any(|diagnostic| diagnostic.code == code && diagnostic.message.contains(message)),
        "`{source}` should be rejected with {code:?} containing `{message}`: {found:?}"
    );
}

#[track_caller]
fn assert_accepted(source: &str) {
    let found = diagnostics(source);
    assert!(found.is_empty(), "`{source}` should compile: {found:?}");
}

fn javascript(source: &str) -> String {
    let compiled = compile_with(source);
    assert!(
        compiled.diagnostics.is_empty(),
        "{:?}",
        compiled.diagnostics
    );
    compiled.output.unwrap().artifacts[ENTRY].javascript.clone()
}

#[test]
fn a_numeric_enum_emits_the_object_with_reverse_mapping() {
    let output = javascript("enum Color { Red, Green = 5, Blue }");
    assert_eq!(
        output.trim(),
        "var Color; (function (Color) { Color[Color[\"Red\"] = 0] = \"Red\"; \
         Color[Color[\"Green\"] = 5] = \"Green\"; Color[Color[\"Blue\"] = 6] = \"Blue\"; \
         })(Color || (Color = {}));"
    );
}

#[test]
fn string_members_have_no_reverse_mapping_and_print_with_double_quotes() {
    let output = javascript("enum S { A = 'a', B = \"b\\\"\", C = `t`, D = 'l1\\nl2' }");
    assert!(output.contains("S[\"A\"] = \"a\";"), "{output}");
    assert!(output.contains("S[\"B\"] = \"b\\\"\";"), "{output}");
    assert!(output.contains("S[\"C\"] = \"t\";"), "{output}");
    assert!(output.contains("S[\"D\"] = \"l1\\nl2\";"), "{output}");
    assert!(!output.contains("S[S["), "no reverse mapping: {output}");
}

#[test]
fn constant_expressions_are_folded_the_way_typescript_folds_them() {
    let output = javascript(
        "enum E { A = 1 << 2, B = A | 1, C = E.B * 2, D = E[\"C\"] + A, F = (A | 8) ^ 2, \
         G = -7 % 3, H = ~5, I = 2 ** 3, J = 7 >>> 1, K = 1 << 33, L = 0x10 + 0b11 + 0o7 + 1_000, \
         M = .5 + 5., N = 1 / 0, O = 0 / 0 }",
    );
    for expected in [
        "E[\"A\"] = 4]",
        "E[\"B\"] = 5]",
        "E[\"C\"] = 10]",
        "E[\"D\"] = 14]",
        "E[\"F\"] = 14]",
        "E[\"G\"] = -1]",
        "E[\"H\"] = -6]",
        "E[\"I\"] = 8]",
        "E[\"J\"] = 3]",
        "E[\"K\"] = 2]",
        "E[\"L\"] = 1026]",
        "E[\"M\"] = 5.5]",
        "E[\"N\"] = Infinity]",
        "E[\"O\"] = NaN]",
    ] {
        assert!(
            output.contains(expected),
            "`{expected}` missing from {output}"
        );
    }
    let strings = javascript("enum S { A = 'a' + 'b', B = 'x' + 1, C = 1 + 'y' }");
    assert!(strings.contains("S[\"A\"] = \"ab\";"), "{strings}");
    assert!(strings.contains("S[\"B\"] = \"x1\";"), "{strings}");
    assert!(strings.contains("S[\"C\"] = \"1y\";"), "{strings}");
}

#[test]
fn members_number_from_the_previous_numeric_value() {
    let output = javascript("enum E { A = 5, B, C = 10, D, E2 = -1, F, G = 1.5, H }");
    for expected in ["B\"] = 6]", "D\"] = 11]", "F\"] = 0]", "H\"] = 2.5]"] {
        assert!(
            output.contains(expected),
            "`{expected}` missing from {output}"
        );
    }
}

#[test]
fn a_computed_member_keeps_its_initializer_and_erased_types() {
    let output = javascript(
        "declare function seed(): number; \
         enum E { A = seed(), B = 2, C = (seed() as number) + 1 }",
    );
    assert!(output.contains("E[E[\"A\"] = seed()] = \"A\";"), "{output}");
    assert!(output.contains("E[E[\"B\"] = 2] = \"B\";"), "{output}");
    assert!(
        output.contains("E[E[\"C\"] = (seed()) + 1] = \"C\";")
            || output.contains("E[E[\"C\"] = (seed() ) + 1]"),
        "{output}"
    );
    assert!(!output.contains(" as "), "{output}");
}

#[test]
fn exported_merged_and_ambient_declarations() {
    let exported = javascript("export enum E { A }");
    assert!(
        exported.starts_with("// ") || exported.contains("export var E;"),
        "{exported}"
    );
    assert!(
        exported.contains("export var E; (function (E)"),
        "{exported}"
    );
    let merged = javascript("enum E { A } enum E { B = 1, C }");
    assert_eq!(
        merged.matches("var E;").count(),
        1,
        "one variable: {merged}"
    );
    assert_eq!(merged.matches("(function (E)").count(), 2, "{merged}");
    assert!(merged.contains("E[E[\"C\"] = 2] = \"C\";"), "{merged}");
    let ambient = javascript("declare enum E { A, B = 2 } const n: number = E.A;");
    assert!(
        !ambient.contains("var E"),
        "an ambient enum has no runtime form: {ambient}"
    );
}

#[test]
fn an_enum_keeps_its_line_count() {
    let source = "enum E {\n    A,\n    B,\n}\nconst x: number = E.B;\n";
    let output = javascript(source);
    let line = |text: &str, needle: &str| text.lines().position(|line| line.contains(needle));
    assert_eq!(
        line(&output, "const x"),
        line(source, "const x"),
        "no later source line moves: {output}"
    );
}

#[test]
fn member_types_and_assignability_follow_typescript() {
    assert_accepted(
        "enum E { A, B } const a: E = E.B; const n: number = E.A; const s: string = E[0]; \
         const t: E.A | E.B = E.A; const only: E.B = E.B;",
    );
    assert_accepted("enum E { A, B } const a: E = 1; const b: E = 0;");
    assert_accepted("enum E { A, B } declare const n: number; const a: E = n;");
    assert_accepted("enum S { A = 'a' } const s: string = S.A; const e: S = S.A;");
    for (source, message) in [
        ("enum E { A, B } const a: E = 5;", "not assignable to `E`"),
        (
            "enum E { A, B } const a: E.A = E.B;",
            "not assignable to `E.A`",
        ),
        (
            "enum S { A = 'a' } const e: S = 'a';",
            "not assignable to `S`",
        ),
        (
            "enum S { A = 'a' } const e: S = 1;",
            "not assignable to `S`",
        ),
        (
            "enum E1 { A } enum E2 { A } const a: E1 = E2.A;",
            "not assignable to `E1`",
        ),
        (
            "enum E { A } function f(e: E): void {} f(3);",
            "not assignable to parameter `e`",
        ),
        (
            "enum E { A, B } const s: string = E.A;",
            "not assignable to `string`",
        ),
        (
            "enum S { A = 'a' } const n: number = S.A;",
            "not assignable to `number`",
        ),
    ] {
        let found = diagnostics(source);
        assert!(
            found
                .iter()
                .any(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch
                    && diagnostic.message.contains(message)),
            "`{source}`: expected `{message}` in {found:?}"
        );
    }
}

#[test]
fn a_number_literal_is_accepted_only_where_a_member_has_that_value() {
    assert_accepted("enum E { A = 1, B = 4 } const a: E = 4; const b: E = 1;");
    assert_accepted("enum E { A, B } function f(e: E): number { return 1; } f(1); f(0);");
    for source in [
        "enum E { A = 1 } const a: E = 2;",
        "enum E { A = 1 } const a: E = -1;",
        "enum E { A, B } function f(e: E): number { return 1; } f(9);",
        "enum E { A, B } function f(): E { return 7; }",
    ] {
        assert!(
            !diagnostics(source).is_empty(),
            "`{source}` must be rejected"
        );
    }
    // An arithmetic result is a plain number, which a numeric enum accepts.
    assert_accepted("enum E { A, B } const a: E = 1 + 1;");
}

#[test]
fn indexing_an_enum_reads_a_member_or_maps_a_number_back() {
    assert_accepted(
        "enum E { A, B } const s: string = E[E.A]; const n: number = E['B']; \
         const k: number = 1; const back: string = E[k];",
    );
    assert_rejected(
        "enum E { A } const x = E['Missing'];",
        DiagnosticCode::TypeMismatch,
        "is not a member of enum `E`",
    );
    assert_rejected(
        "enum S { A = 'a' } const x = S[0];",
        DiagnosticCode::TypeMismatch,
        "cannot be used on enum `S`",
    );
    assert_rejected(
        "enum E { A } const x = E.Missing;",
        DiagnosticCode::TypeMismatch,
        "does not exist on type",
    );
    assert_rejected(
        "enum E { A } E.A = 1;",
        DiagnosticCode::TypeMismatch,
        "cannot mutate readonly property `A`",
    );
}

#[test]
fn heterogeneous_and_computed_enums_are_typed_as_their_members_allow() {
    assert_accepted(
        "enum M { A = 1, B = 'b', C = 3 } const x: M = M.B; const n: number = M.A; const s: string = M.B;",
    );
    assert_accepted("declare function seed(): number; enum E { A = seed(), B = 2 } const n: number = E.A + E.B; const e: E = 77;");
    assert_accepted(
        "declare function seed(): number; enum E { A = 1, B = seed() } const x: E = E.B;",
    );
}

#[test]
fn initializer_and_declaration_errors() {
    assert_rejected(
        "enum E { A, A }",
        DiagnosticCode::DuplicateDeclaration,
        "duplicate identifier `A`",
    );
    assert_rejected(
        "enum E { A = 'a', B }",
        DiagnosticCode::TypeMismatch,
        "enum member `B` must have an initializer",
    );
    assert_rejected(
        "declare function f(): number; enum E { A = f(), B }",
        DiagnosticCode::TypeMismatch,
        "enum member `B` must have an initializer",
    );
    assert_rejected(
        "enum E { A = B, B }",
        DiagnosticCode::TypeMismatch,
        "enum member `B` is used before its initialization",
    );
    assert_rejected(
        "enum E { A } enum E { B }",
        DiagnosticCode::TypeMismatch,
        "only one may omit the initializer of its first member",
    );
    assert_rejected(
        "declare function f(): string; enum E { A = f() }",
        DiagnosticCode::TypeMismatch,
        "a computed enum member must be a number",
    );
    assert_rejected(
        "enum E { A } const E = 1;",
        DiagnosticCode::DuplicateDeclaration,
        "duplicate declaration of `E`",
    );
    assert_rejected(
        "const E = 1; enum E { A }",
        DiagnosticCode::DuplicateDeclaration,
        "duplicate declaration of `E`",
    );
}

#[test]
fn what_is_not_supported_yet_is_refused_not_approximated() {
    assert_rejected(
        "const enum E { A }",
        DiagnosticCode::UnsupportedSyntax,
        "`const enum` is not supported yet",
    );
    assert_rejected(
        "declare function f(n: number): number; enum E { A = 1, B = f(A) }",
        DiagnosticCode::UnsupportedSyntax,
        "must write it as `E.A`",
    );
    assert_rejected(
        "function f() { enum Inner { A } }",
        DiagnosticCode::UnsupportedSyntax,
        "declared inside a body",
    );
    assert_rejected(
        "enum E { 'a\\nb' = 1 }",
        DiagnosticCode::UnsupportedSyntax,
        "escape sequence",
    );
}

#[test]
fn an_enum_works_across_functions_classes_and_object_values() {
    assert_accepted(
        "enum Kind { A, B } class Item { kind: Kind = Kind.A; label(): string { return Kind[this.kind]; } \
         matches(k: Kind): boolean { return this.kind === k; } } \
         const b: boolean = new Item().matches(Kind.B);",
    );
    assert_accepted(
        "enum Level { Low, High } function describe(l: Level): string { return Level[l]; } \
         const s: string = describe(Level.High) + describe(1);",
    );
    assert_accepted("enum E { A, B } const o = E; const n: number = o.A;");
}
