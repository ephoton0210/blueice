// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Namespaces (J.3.5): parsing, typing, merging and emitted text. Verdicts on
//! real programs are pinned against TypeScript by the `namespace-*` matrix
//! fixtures and the runtime oracle; these tests pin the structure, codes and
//! messages.

use blueice_bluets::{parse_module, Declaration, NamespaceDeclaration};

fn namespaces(source: &str) -> Vec<NamespaceDeclaration> {
    parse_module("memory:///main.ts", source)
        .expect("the source parses")
        .declarations
        .into_iter()
        .filter_map(|declaration| match declaration {
            Declaration::Namespace(namespace) => Some(namespace),
            _ => None,
        })
        .collect()
}

#[test]
fn a_namespace_keeps_its_body_declarations_in_source_order() {
    let found = namespaces(
        "namespace N { export const a = 1; function hidden() {} export interface I { x: number } }",
    );
    assert_eq!(found.len(), 1);
    let namespace = &found[0];
    assert_eq!(namespace.name, "N");
    assert!(!namespace.exported && !namespace.declared && !namespace.implicit);
    let kinds = namespace
        .body
        .iter()
        .map(|declaration| match declaration {
            Declaration::Variable(variable) => ("variable", variable.exported),
            Declaration::Function(function) => ("function", function.exported),
            Declaration::Interface(interface) => ("interface", interface.exported),
            other => panic!("unexpected {other:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        [("variable", true), ("function", false), ("interface", true)]
    );
}

#[test]
fn a_dotted_name_is_one_namespace_per_segment_each_exported_from_its_parent() {
    let found = namespaces("export namespace A.B.C { export const x = 1; }");
    let outer = &found[0];
    assert_eq!(
        (outer.name.as_str(), outer.exported, outer.implicit),
        ("A", true, false)
    );
    let Declaration::Namespace(middle) = &outer.body[0] else {
        panic!("expected B");
    };
    assert_eq!(
        (middle.name.as_str(), middle.exported, middle.implicit),
        ("B", true, true)
    );
    let Declaration::Namespace(inner) = &middle.body[0] else {
        panic!("expected C");
    };
    assert_eq!((inner.name.as_str(), inner.implicit), ("C", true));
    assert!(matches!(&inner.body[0], Declaration::Variable(variable) if variable.name == "x"));
}

#[test]
fn the_module_keyword_declares_a_namespace_and_an_ambient_body_is_ambient() {
    let found = namespaces("declare module M { function f(): void; const v: number; }");
    let namespace = &found[0];
    assert!(namespace.declared);
    assert!(namespace.body.iter().all(|declaration| match declaration {
        Declaration::Function(function) => function.declared,
        Declaration::Variable(variable) => variable.declared,
        _ => false,
    }));
    assert!(namespace.exports_every_member());
    let explicit =
        namespaces("declare namespace M { export function f(): void; function g(): void; }");
    assert!(!explicit[0].exports_every_member());
}

#[test]
fn nested_namespaces_spans_and_header_cover_the_written_text() {
    let source = "export namespace Outer { namespace Inner { export let v = 2; } }";
    let found = namespaces(source);
    let outer = &found[0];
    assert_eq!(
        &source[outer.header_span.start..outer.header_span.end],
        "export namespace Outer {"
    );
    assert_eq!(
        &source[outer.closing_span.start..outer.closing_span.end],
        "}"
    );
    assert_eq!(outer.span.end, source.len());
    let Declaration::Namespace(inner) = &outer.body[0] else {
        panic!("expected Inner");
    };
    assert_eq!(&source[inner.name_span.start..inner.name_span.end], "Inner");
}

#[test]
fn a_keyword_not_followed_by_a_name_on_the_same_line_is_not_a_namespace() {
    // `namespace` then a newline is an expression statement in TypeScript, so it is
    // not read as a declaration header (BlueTS still refuses the identifier itself).
    let error = parse_module("memory:///main.ts", "namespace\nN { }").expect_err("not a namespace");
    assert!(error
        .iter()
        .all(|diagnostic| !diagnostic.message.contains("namespace body")));
}

#[test]
fn what_a_namespace_body_cannot_hold_is_refused() {
    for source in [
        "namespace N { export default 1; }",
        "namespace N { import a from './a'; }",
        "namespace N { export { a }; }",
    ] {
        let error = parse_module("memory:///main.ts", source).expect_err(source);
        assert!(
            error
                .iter()
                .any(|diagnostic| diagnostic.message.contains("inside a namespace body")),
            "{source}: {error:?}"
        );
    }
    let error = parse_module(
        "memory:///main.ts",
        "declare namespace N { declare const x: number; }",
    )
    .expect_err("a redundant declare");
    assert!(error[0].message.contains("already ambient"));
    let error =
        parse_module("memory:///main.ts", "namespace N { const x = 1;").expect_err("unterminated");
    assert!(error[0].message.contains("unterminated namespace body"));
}

fn initializer_texts(source: &str, name: &str) -> Vec<String> {
    let module = parse_module("memory:///main.ts", source).expect("the source parses");
    for declaration in &module.declarations {
        if let Declaration::Variable(variable) = declaration {
            if variable.name == name {
                return variable
                    .initializer
                    .iter()
                    .map(|token| token.text.clone())
                    .collect();
            }
        }
    }
    panic!("no variable {name}");
}

#[test]
fn a_reference_to_an_exported_member_is_one_token() {
    let source =
        "namespace N { export const a = 1; export namespace Inner { export const z = 2; } \
                  const hidden = 3; } \
                  const x = N.a + N.Inner.z; const h = N.hidden;";
    assert_eq!(initializer_texts(source, "x"), ["N.a", "+", "N.Inner.z"]);
    // A non-exported member is not a member as far as a reference goes.
    assert_eq!(initializer_texts(source, "h"), ["N", ".", "hidden"]);
}

#[test]
fn a_class_merged_with_a_namespace_keeps_its_static_member_unmerged() {
    let source = "class K { static s = 1; } namespace K { export const t = 2; } \
                  const y = K.s + K.t;";
    assert_eq!(initializer_texts(source, "y"), ["K", ".", "s", "+", "K.t"]);
}

#[test]
fn inside_a_namespace_an_inner_reference_is_relative_and_spans_stay_exact() {
    let source = "namespace N { export namespace Inner { export const z = 2; } \
                  export const w = Inner.z + 1; }";
    let module = parse_module("memory:///main.ts", source).unwrap();
    let Declaration::Namespace(namespace) = &module.declarations[0] else {
        panic!("expected N");
    };
    let variable = namespace
        .body
        .iter()
        .find_map(|declaration| match declaration {
            Declaration::Variable(variable) => Some(variable),
            _ => None,
        })
        .unwrap();
    assert_eq!(variable.initializer[0].text, "Inner.z");
    let (start, end) = (variable.initializer[0].start, variable.initializer[0].end);
    assert_eq!(&source[start..end], "Inner.z");
}

#[test]
fn a_dotted_declaration_header_is_not_merged() {
    let found =
        namespaces("namespace A { export namespace B {} } namespace A.B { export const k = 1; }");
    assert_eq!(found.len(), 2);
    let Declaration::Namespace(inner) = &found[1].body[0] else {
        panic!("expected B");
    };
    assert_eq!(inner.name, "B");
}

use blueice_bluets::{
    compile, CompilerOptions, Diagnostic, DiagnosticCode, MapLoader, ModuleSource,
};

const ENTRY: &str = "memory:///main.ts";

/// The diagnostics of compiling `source`.
fn check(source: &str) -> Vec<Diagnostic> {
    compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions::default(),
    )
    .diagnostics
}

#[track_caller]
fn assert_accepted(source: &str) {
    let found = check(source);
    assert!(found.is_empty(), "`{source}` should be accepted: {found:?}");
}

#[track_caller]
fn assert_rejected(source: &str, code: DiagnosticCode, message: &str) {
    let found = check(source);
    assert!(
        found
            .iter()
            .any(|diagnostic| diagnostic.code == code && diagnostic.message.contains(message)),
        "`{source}` should be rejected with {code:?} containing `{message}`: {found:?}"
    );
}

#[test]
fn exported_members_are_reached_through_the_namespace() {
    assert_accepted(
        "namespace N { export const a: number = 1; export function f(x: number): number { return x + a; } \
         export interface I { q: number } export type T = string; } \
         const x: number = N.a + N.f(2); const i: N.I = { q: 1 }; const t: N.T = 'v';",
    );
}

#[test]
fn a_member_of_the_wrong_type_is_a_type_error() {
    assert_rejected(
        "namespace N { export const a: number = 1; } const s: string = N.a;",
        DiagnosticCode::TypeMismatch,
        "",
    );
    assert_rejected(
        "namespace N { export function f(x: number): number { return x; } } N.f('a');",
        DiagnosticCode::TypeMismatch,
        "",
    );
}

#[test]
fn a_hidden_member_cannot_be_named_from_outside() {
    assert_rejected(
        "namespace N { interface H { q: number } export interface P { q: number } } \
         const h: N.H = { q: 1 };",
        DiagnosticCode::UnknownType,
        "namespace `N` has no exported member `H`",
    );
    assert_rejected(
        "namespace N { const secret: number = 1; export const open: number = 2; } \
         const v: number = N.secret;",
        DiagnosticCode::TypeMismatch,
        "",
    );
}

#[test]
fn a_type_or_a_type_only_namespace_cannot_be_used_as_a_value() {
    assert_rejected(
        "namespace N { export interface I { q: number } } const v = N.I;",
        DiagnosticCode::TypeMismatch,
        "`N.I` only refers to a type",
    );
    assert_rejected(
        "namespace N { export interface I { q: number } } const v = N;",
        DiagnosticCode::TypeMismatch,
        "namespace `N` has no run-time members",
    );
}

#[test]
fn merged_blocks_see_earlier_exports_but_cannot_redeclare_them() {
    assert_accepted(
        "namespace N { export const a: number = 1; } \
         namespace N { export const b: number = a + 1; } const c: number = N.b;",
    );
    assert_rejected(
        "namespace N { export const a: number = 1; } namespace N { export const a: number = 2; }",
        DiagnosticCode::DuplicateDeclaration,
        "duplicate declaration of `a`",
    );
}

#[test]
fn a_namespace_merges_into_a_function_class_or_enum_that_precedes_it() {
    assert_accepted(
        "function g(): number { return 1; } namespace g { export const m: string = 'm'; } \
         const s: string = g.m; const n: number = g();",
    );
    assert_accepted(
        "class K { static s: number = 1; } namespace K { export const t: number = 2; } \
         const a: number = K.s + K.t;",
    );
    assert_accepted(
        "enum E { A } namespace E { export function f(): number { return 1; } } \
         const e: E = E.A; const n: number = E.f();",
    );
    // The namespace must follow the declaration it merges into.
    assert_rejected(
        "namespace K { export const t: number = 1; } class K { v: number = 1; }",
        DiagnosticCode::DuplicateDeclaration,
        "duplicate declaration of `K`",
    );
}

#[test]
fn an_inner_declaration_shadows_the_enclosing_scope() {
    assert_accepted(
        "const x: string = 'top'; namespace N { const x: number = 5; export const y: number = x + 1; } \
         const s: string = x; const n: number = N.y;",
    );
    assert_rejected(
        "const x: string = 'top'; namespace N { const x: number = 5; export const y: string = x; }",
        DiagnosticCode::TypeMismatch,
        "",
    );
}

#[test]
fn nested_and_dotted_namespaces_check_through_every_level() {
    assert_accepted(
        "namespace A.B.C { export const deep: string = 'd'; } const s: string = A.B.C.deep;",
    );
    assert_rejected(
        "namespace A.B.C { export const deep: string = 'd'; } const n: number = A.B.C.deep;",
        DiagnosticCode::TypeMismatch,
        "",
    );
    assert_accepted(
        "namespace O { export const base: number = 10; export namespace I { export const z: number = base * 2; } \
         export const w: number = I.z; } const n: number = O.I.z + O.w;",
    );
}

#[test]
fn a_body_is_checked_like_the_rest_of_the_module() {
    assert_rejected(
        "namespace N { export function f(): number { return 'a'; } }",
        DiagnosticCode::ReturnTypeMismatch,
        "",
    );
    assert_rejected(
        "namespace N { export const a: number = 'a'; }",
        DiagnosticCode::TypeMismatch,
        "",
    );
}

#[test]
fn an_ambient_variable_cannot_have_an_initializer_but_a_literal_const_may() {
    assert_rejected(
        "declare namespace N { const x: number = 1; }",
        DiagnosticCode::ParseError,
        "initializers are not allowed in ambient contexts",
    );
    assert_rejected(
        "declare let y = 1;",
        DiagnosticCode::ParseError,
        "initializers are not allowed in ambient contexts",
    );
    assert_accepted("declare const z = 1; declare const s = 'a'; const n: number = z;");
}

#[test]
fn an_ambient_namespace_exports_every_member_and_has_no_emit() {
    assert_accepted(
        "declare namespace Lib { const version: number; function describe(n: number): string; } \
         const s: string = Lib.describe(Lib.version);",
    );
}

fn emit(source: &str) -> String {
    emit_with(source, CompilerOptions::default())
}

fn emit_with(source: &str, options: CompilerOptions) -> String {
    let compiled = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        options,
    );
    assert!(
        compiled.diagnostics.is_empty(),
        "{:?}",
        compiled.diagnostics
    );
    compiled.output.unwrap().artifacts[ENTRY].javascript.clone()
}

#[test]
fn a_namespace_is_a_function_over_an_object_that_blocks_merge_into() {
    let output = emit(
        "namespace N {\n  export const a: number = 1;\n  export function f(): number { return a; }\n}\n\
         namespace N {\n  export const b: number = N.a + 1;\n}\n",
    );
    assert_eq!(
        output,
        "var N; (function (N) {\n  N.a= 1;\n  function f(){ return N.a; } N.f = f;\n})(N || (N = {}));\n\
         (function (N) {\n  N.b= N.a + 1;\n})(N || (N = {}));\n"
    );
}

#[test]
fn an_exported_namespace_exports_its_variable_and_a_merge_declares_nothing_twice() {
    assert!(emit("export namespace N { export const a: number = 1; }")
        .starts_with("export var N; (function (N) {"));
    let merged =
        emit("function g(): number { return 1; } namespace g { export const m: number = 2; }");
    assert!(!merged.contains("var g"), "{merged}");
    assert!(
        merged.contains("(function (g) { g.m= 2; })(g || (g = {}))"),
        "{merged}"
    );
}

#[test]
fn a_nested_namespace_is_let_in_its_parent_and_assigned_to_it_when_exported() {
    let output = emit(
        "namespace O {\n  export namespace I { export const z: number = 1; }\n  namespace P { export const q: number = 2; }\n}",
    );
    assert!(
        output.contains("let I; (function (I) { I.z= 1; })(I = O.I || (O.I = {}));"),
        "{output}"
    );
    assert!(
        output.contains("let P; (function (P) { P.q= 2; })(P || (P = {}));"),
        "{output}"
    );
}

#[test]
fn a_dotted_namespace_nests_one_function_per_name() {
    let output = emit("namespace A.B.C { export const k: number = 7; }");
    assert_eq!(
        output,
        "var A; (function (A) { var B; (function (B) { var C; (function (C) { C.k= 7; })(C = B.C || (B.C = {})); })(B = A.B || (A.B = {})); })(A || (A = {}));"
    );
}

#[test]
fn a_namespace_with_nothing_at_run_time_is_removed_and_keeps_its_lines() {
    let source = "namespace T {\n  export interface I { q: number }\n  export type U = string;\n}\nconst x: number = 1;\n";
    let output = emit(source);
    assert!(!output.contains('T'), "{output}");
    assert_eq!(output.matches('\n').count(), source.matches('\n').count());
    let ambient = "declare namespace Lib {\n  const v: number;\n}\nconst y: number = 2;\n";
    let output = emit(ambient);
    assert!(!output.contains("Lib"), "{output}");
    assert_eq!(output.matches('\n').count(), ambient.matches('\n').count());
}

#[test]
fn emission_keeps_every_source_line_in_place() {
    let source = "namespace N {\n  export let a: number = 1;\n  export enum E {\n    X,\n    Y\n  }\n  export class C {\n    v: number = a;\n  }\n}\nconsole.log(N.a);\n";
    let output = emit(source);
    assert_eq!(output.matches('\n').count(), source.matches('\n').count());
    // The statement after the namespace is still on its last line.
    assert!(output.trim_end().ends_with("console.log(N.a);"));
}

#[test]
fn exported_variables_are_read_through_the_object_in_every_expression_form() {
    let output = emit(
        "namespace R {\n export let c: number = 0;\n export function f(): string { return `${c}`; }\n \
         export function o(): { c: number } { return { c }; }\n export class K { c: number = 1; r(): number { return this.c + c; } }\n \
         export const k: number = { c: 5 }.c;\n}",
    );
    assert!(output.contains("`${R.c}`"), "{output}");
    assert!(output.contains("return { c: R.c }"), "{output}");
    assert!(output.contains("this.c + R.c"), "{output}");
    // A class member and an object key named like the variable are not references.
    assert!(output.contains("c = 1;"), "{output}");
    assert!(output.contains("{ c: 5 }.c"), "{output}");
}

#[test]
fn a_reference_to_a_member_another_block_exported_goes_through_the_object() {
    let output = emit(
        "namespace A.B { export const x: number = 1; } namespace A { export const z: number = B.x; }",
    );
    assert!(output.contains("A.z= A.B.x"), "{output}");
}

#[test]
fn what_could_shadow_an_exported_variable_is_refused() {
    for source in [
        "namespace N { export const a: number = 1; export function f(a: number): number { return a; } }",
        "namespace N { export const a: number = 1; export function f(): number { const a: number = 2; return a; } }",
        "namespace N { export let a: number = 1, b: number = 2; }",
        "namespace N { export const a: number = 1; export const g = (a: number): number => a; }",
    ] {
        let compiled = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        assert!(
            compiled
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::UnsupportedSyntax),
            "`{source}` should be refused: {:?}",
            compiled.diagnostics
        );
        assert!(compiled.output.is_none(), "{source}");
    }
}

#[test]
fn a_private_name_in_a_namespace_class_is_refused_below_es2022_only() {
    let source =
        "namespace N { export class C { #p: number = 1; get(): number { return this.#p; } } }";
    let _ = source;
    let source =
        "namespace N { export class C { #p: number = 1; read(): number { return this.#p; } } }";
    let low = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions {
            target: blueice_bluets::EcmaTarget::Es2020,
            ..CompilerOptions::default()
        },
    );
    assert!(low.diagnostics.iter().any(|d| d
        .message
        .contains("a private name in a class inside a namespace")));
    let native = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions::default(),
    );
    assert!(native.output.is_some(), "{:?}", native.diagnostics);
}
