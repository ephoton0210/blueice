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

fn compile_modules(
    modules: &[(&str, &str)],
    options: CompilerOptions,
) -> blueice_bluets::Compilation {
    let sources: Vec<ModuleSource> = modules
        .iter()
        .map(|(file, text)| ModuleSource::new(format!("memory:///{file}"), *text))
        .collect();
    compile("memory:///main.ts", &MapLoader::from(sources), options)
}

fn emitted(compilation: &blueice_bluets::Compilation, module: &str) -> String {
    assert!(
        compilation.diagnostics.is_empty(),
        "{:?}",
        compilation.diagnostics
    );
    compilation.output.as_ref().unwrap().artifacts[&format!("memory:///{module}")]
        .javascript
        .clone()
}

fn options(preserve: bool, isolated: bool) -> CompilerOptions {
    CompilerOptions {
        preserve_const_enums: preserve,
        isolated_modules: isolated,
        ..CompilerOptions::default()
    }
}

#[test]
fn a_const_enum_is_erased_and_its_uses_become_values_with_a_comment() {
    let output = javascript(
        "const enum E { A = 1, B, S = 's', 'x-y' = 7, N = -3, F = 1.5 } \
         const a: number = E.A; const b: number = E['B'] + E['x-y']; const s: string = E.S; \
         const t: string = `${E.A}:${E.S}`; const n: number = E.N; const f: number = E.F;",
    );
    assert!(
        !output.contains("var E"),
        "the declaration is erased: {output}"
    );
    for expected in [
        "const a= 1 /* E.A */;",
        "2 /* E[\"B\"] */ + 7 /* E[\"x-y\"] */",
        "\"s\" /* E.S */",
        "`${1 /* E.A */}:${\"s\" /* E.S */}`",
        "(-3 /* E.N */)",
        "1.5 /* E.F */",
    ] {
        assert!(
            output.contains(expected),
            "`{expected}` missing from {output}"
        );
    }
}

#[test]
fn a_negative_or_non_finite_value_is_parenthesized_so_the_output_stays_valid() {
    let output = javascript(
        "const enum E { N = -3, Z = 1 / 0 } const a: number = E.N ** 2; const b: number = -E.N; \
         const c: number = E.Z;",
    );
    assert!(output.contains("(-3 /* E.N */) ** 2"), "{output}");
    assert!(output.contains("-(-3 /* E.N */)"), "{output}");
    assert!(output.contains("(Infinity /* E.Z */)"), "{output}");
}

#[test]
fn preserve_const_enums_keeps_the_object_and_isolated_modules_stops_inlining() {
    let source = "const enum E { A = 1 } const a: number = E.A;";
    let preserved = emitted(
        &compile_modules(&[("main.ts", source)], options(true, false)),
        "main.ts",
    );
    assert!(
        preserved.contains("var E;") && preserved.contains("const a= 1 /* E.A */;"),
        "{preserved}"
    );
    let isolated = emitted(
        &compile_modules(&[("main.ts", source)], options(false, true)),
        "main.ts",
    );
    assert!(
        isolated.contains("var E;") && isolated.contains("const a= E.A;"),
        "{isolated}"
    );
    // Without types (transpile-only) the value cannot be known either.
    let transpile = emitted(
        &compile_modules(
            &[("main.ts", source)],
            CompilerOptions {
                runtime_policy: blueice_bluets::RuntimePolicy::TranspileOnly,
                ..CompilerOptions::default()
            },
        ),
        "main.ts",
    );
    assert!(
        transpile.contains("var E;") && transpile.contains("const a= E.A;"),
        "{transpile}"
    );
}

#[test]
fn an_ambient_const_enum_is_inlined_but_not_when_modules_are_isolated() {
    let source = "declare const enum E { A, B = 2 } const n: number = E.A + E.B;";
    let output = emitted(
        &compile_modules(&[("main.ts", source)], options(false, false)),
        "main.ts",
    );
    assert!(output.contains("0 /* E.A */ + 2 /* E.B */"), "{output}");
    assert!(!output.contains("var E"), "{output}");
    let isolated = compile_modules(&[("main.ts", source)], options(false, true));
    assert!(
        isolated.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::TypeMismatch
                && diagnostic.message.contains("ambient const enum")
        }),
        "{:?}",
        isolated.diagnostics
    );
}

#[test]
fn a_const_enum_can_only_be_used_by_a_member_name() {
    assert_rejected(
        "const enum E { A, B } const x = E[0];",
        DiagnosticCode::TypeMismatch,
        "can only be accessed with a string literal",
    );
    assert_rejected(
        "const enum E { A } declare const k: string; const x = E[k];",
        DiagnosticCode::TypeMismatch,
        "can only be accessed with a string literal",
    );
    assert_rejected(
        "const enum E { A } const v = E;",
        DiagnosticCode::TypeMismatch,
        "can only be used in a property or index access",
    );
    assert_rejected(
        "declare function f(n: number): number; const enum E { A = f(1) }",
        DiagnosticCode::TypeMismatch,
        "must be a constant expression",
    );
    assert_rejected(
        "const enum E { A } E.A = 1;",
        DiagnosticCode::TypeMismatch,
        "cannot mutate readonly property `A`",
    );
    // A local of the same name is its own value.
    assert_accepted("const enum E { A } function f(E: number): number { return E; }");
}

#[test]
fn const_enum_declarations_merge_and_type_like_regular_ones() {
    assert_accepted("const enum E { A } const enum E { B = 1 } const n: number = E.A + E.B;");
    assert_accepted("const enum E { A = 1 << 2, B = A | 1, C = 's' + 't' } const n: number = E.B; const s: string = E.C;");
    assert_rejected(
        "const enum E { A } const a: E = 4;",
        DiagnosticCode::TypeMismatch,
        "not assignable to `E`",
    );
}

#[test]
fn an_enum_crosses_modules_with_its_members_and_its_object() {
    let colors = "export const enum Color { Red, Green = 5 } export enum Mode { Fast = 'fast' } \
                  const enum Hidden { X = 9 } export function hidden(): number { return Hidden.X; }";
    let main = "import { Color, Mode, hidden } from './colors.ts'; \
                const c: Color = Color.Green; const n: number = c + hidden(); const m: Mode = Mode.Fast;";
    let compilation = compile_modules(
        &[("main.ts", main), ("colors.ts", colors)],
        options(false, false),
    );
    let main_output = emitted(&compilation, "main.ts");
    assert!(
        main_output.contains("const c= 5 /* Color.Green */;"),
        "{main_output}"
    );
    assert!(
        main_output.contains("Mode.Fast"),
        "a regular enum is used through its object: {main_output}"
    );
    let colors_output = emitted(&compilation, "colors.ts");
    assert!(
        colors_output.contains("export var Color;"),
        "an exported const enum keeps its object so importers stay valid: {colors_output}"
    );
    assert!(
        !colors_output.contains("var Hidden"),
        "a private const enum is erased: {colors_output}"
    );
    assert!(
        colors_output.contains("9 /* Hidden.X */"),
        "{colors_output}"
    );
    // The type of an imported enum is the enum's.
    for (program, message) in [
        (
            "import { Color } from './colors.ts'; const c: Color = 99;",
            "not assignable to `Color`",
        ),
        (
            "import { Color } from './colors.ts'; const s = Color[0];",
            "can only be accessed",
        ),
        (
            "import { Color } from './colors.ts'; const v = Color;",
            "can only be used in a property",
        ),
        (
            "import { Missing } from './colors.ts'; const m = Missing.A;",
            "has no exported member `Missing`",
        ),
        (
            "import { Hidden } from './colors.ts'; const n = Hidden.X;",
            "has no exported member `Hidden`",
        ),
        (
            "import type { Mode } from './colors.ts'; const m = Mode.Fast;",
            "imported with `import type`",
        ),
    ] {
        let found = compile_modules(
            &[("main.ts", program), ("colors.ts", colors)],
            options(false, false),
        );
        assert!(
            found
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains(message)),
            "`{program}`: expected `{message}` in {:?}",
            found.diagnostics
        );
    }
    let types_only = compile_modules(
        &[
            (
                "main.ts",
                "import type { Color, Mode } from './colors.ts'; declare const c: Color; \
                 declare const m: Mode; const n: number = c; const s: string = m;",
            ),
            ("colors.ts", colors),
        ],
        options(false, false),
    );
    assert!(
        types_only.diagnostics.is_empty(),
        "{:?}",
        types_only.diagnostics
    );
}

#[test]
fn the_const_enum_options_are_part_of_the_fingerprint() {
    let source = "const enum E { A } const n: number = E.A;";
    let fingerprint = |options| {
        compile_modules(&[("main.ts", source)], options)
            .output
            .unwrap()
            .fingerprint
    };
    let base = fingerprint(options(false, false));
    assert_ne!(base, fingerprint(options(true, false)));
    assert_ne!(base, fingerprint(options(false, true)));
    assert_eq!(base, fingerprint(options(false, false)));
}

fn declaration_of(source: &str, options: CompilerOptions) -> String {
    let compiled = compile_modules(
        &[("main.ts", source)],
        CompilerOptions {
            declaration: true,
            ..options
        },
    );
    assert!(
        compiled.diagnostics.is_empty(),
        "{:?}",
        compiled.diagnostics
    );
    compiled.output.unwrap().artifacts["memory:///main.ts"]
        .declaration
        .clone()
        .expect("declaration requested")
}

#[test]
fn declaration_output_prints_enums_the_way_typescript_does() {
    let output = declaration_of(
        "declare function seed(): number; \
         export enum Num { A, B = 5, C, Neg = -1, F = 1.5 } \
         export enum Str { A = 'a', 'q-r' = 'x' } \
         export enum Computed { A = seed(), B = 2 } \
         export const enum Konst { X = 1, Y = 'y' } \
         export declare enum Amb { A, B = 2, C } \
         export enum Merged { A } export enum Merged { B = 1 } \
         enum Local { A } export { Local } \
         enum Private { A } \
         export enum Empty {} \
         export enum Big { A = 2 ** 40, B = 1e21, Inf = 1 / 0, N = 0 / 0 }",
        CompilerOptions::default(),
    );
    assert_eq!(
        output,
        "export declare enum Num {\n    A = 0,\n    B = 5,\n    C = 6,\n    Neg = -1,\n    F = 1.5\n}\n\
         export declare enum Str {\n    A = \"a\",\n    \"q-r\" = \"x\"\n}\n\
         export declare enum Computed {\n    A,\n    B = 2\n}\n\
         export declare const enum Konst {\n    X = 1,\n    Y = \"y\"\n}\n\
         export declare enum Amb {\n    A,\n    B = 2,\n    C\n}\n\
         export declare enum Merged {\n    A = 0\n}\n\
         export declare enum Merged {\n    B = 1\n}\n\
         declare enum Local {\n    A = 0\n}\n\
         export { Local };\n\
         export declare enum Empty {\n}\n\
         export declare enum Big {\n    A = 1099511627776,\n    B = 1e+21,\n    Inf = Infinity,\n    N = NaN\n}\n"
    );
}

#[test]
fn declaration_output_does_not_depend_on_the_const_enum_options() {
    let source =
        "export const enum K { A = 1 } export enum E { X } export declare const enum Ambient { Q }";
    let base = declaration_of(source, options(false, false));
    for (preserve, isolated) in [(true, false), (false, true), (true, true)] {
        assert_eq!(base, declaration_of(source, options(preserve, isolated)));
    }
}

#[test]
fn an_incremental_session_recompiles_when_a_const_enum_option_changes() {
    use blueice_bluets::IncrementalCompiler;
    let loader = MapLoader::from([ModuleSource::new(
        ENTRY,
        "const enum E { A = 1 } const n: number = E.A;",
    )]);
    let mut session = IncrementalCompiler::new();
    let inlined = session.compile(ENTRY, &loader, options(false, false));
    assert!(!inlined.cache_hit);
    assert!(
        session
            .compile(ENTRY, &loader, options(false, false))
            .cache_hit
    );
    let isolated = session.compile(ENTRY, &loader, options(false, true));
    assert!(
        !isolated.cache_hit,
        "a changed option must not reuse the cache"
    );
    let inlined_text = inlined.compilation.output.unwrap().artifacts[ENTRY]
        .javascript
        .clone();
    let isolated_text = isolated.compilation.output.unwrap().artifacts[ENTRY]
        .javascript
        .clone();
    assert!(inlined_text.contains("/* E.A */") && !isolated_text.contains("/* E.A */"));
}
