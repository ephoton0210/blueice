// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Constructor parameter properties (J.3.2.3): erasure, the emitted field
//! declarations and `this.p = p;` assignments, typing, access rules and
//! declaration output. Verdicts on real programs are pinned against TypeScript
//! by the `class-param-property-*` matrix fixtures; these tests pin codes,
//! messages and emitted text.

use blueice_bluets::{
    compile, CompilerOptions, Diagnostic, DiagnosticCode, EcmaTarget, MapLoader, ModuleSource,
};

const ENTRY: &str = "memory:///main.ts";

fn compile_with(source: &str, options: CompilerOptions) -> blueice_bluets::Compilation {
    compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        options,
    )
}

fn diagnostics(source: &str) -> Vec<Diagnostic> {
    compile_with(source, CompilerOptions::default()).diagnostics
}

#[track_caller]
fn assert_rejected(source: &str, code: DiagnosticCode, message: &str) {
    let found = diagnostics(source);
    let Some(first) = found.first() else {
        panic!("`{source}` should be rejected: {message}");
    };
    assert_eq!(first.code, code, "`{source}`: {found:?}");
    assert!(
        first.message.contains(message),
        "`{source}`: expected `{message}` in `{}`",
        first.message
    );
}

#[track_caller]
fn assert_accepted(source: &str) {
    let found = diagnostics(source);
    assert!(found.is_empty(), "`{source}` should compile: {found:?}");
}

fn javascript(source: &str) -> String {
    let compiled = compile_with(source, CompilerOptions::default());
    assert!(
        compiled.diagnostics.is_empty(),
        "{:?}",
        compiled.diagnostics
    );
    compiled.output.unwrap().artifacts[ENTRY].javascript.clone()
}

#[test]
fn modifiers_are_erased_and_the_property_fields_and_assignments_are_emitted() {
    let output = javascript(
        "class A { z: number = 5; \
         constructor(public x: number, private y: string, protected readonly w: number = 3, readonly v?: string) { this.z = 1; } }",
    );
    assert!(
        output.contains("class A { x; y; w; v; z = 5;"),
        "fields first, before declared ones: {output}"
    );
    assert!(
        output.contains("constructor(x, y, w= 3, v) { this.x = x; this.y = y; this.w = w; this.v = v; this.z = 1; }"),
        "assignments at the start of the body: {output}"
    );
    for keyword in [
        "public",
        "private",
        "protected",
        "readonly",
        ": number",
        "?",
    ] {
        assert!(!output.contains(keyword), "{keyword} survived in {output}");
    }
}

#[test]
fn a_derived_constructor_assigns_after_its_super_call() {
    let output = javascript(
        "class B { constructor(public id: number) {} } \
         class D extends B { constructor(public q: number) { const s: number = q + 1; super(s); this.q = q + 1; } }",
    );
    assert!(
        output.contains("class D extends B { q;"),
        "field declared: {output}"
    );
    assert!(
        output.contains("const s= q + 1; super(s); this.q = q; this.q = q + 1;"),
        "assignment right after super, before later statements: {output}"
    );
}

#[test]
fn a_derived_constructor_with_no_top_level_super_call_is_refused() {
    let compiled = compile_with(
        "class B { constructor(public id: number) {} } \
         class D extends B { constructor(public q: number, f: boolean) { if (f) { super(1); } else { super(2); } } }",
        CompilerOptions::default(),
    );
    assert!(compiled.output.is_none());
    assert!(compiled.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::UnsupportedSyntax
            && diagnostic.message.contains("top-level `super(...)`")
    }));
}

#[test]
fn a_property_takes_its_parameters_type_or_a_literal_defaults_widened_type() {
    assert_accepted(
        "class A { constructor(public a: number, public b = 'x', public c = 3, readonly d = true) {} } \
         const a = new A(1); const n: number = a.a + a.c; const s: string = a.b; const t: boolean = a.d;",
    );
    assert_rejected(
        "class A { constructor(public a = 3) {} } const s: string = new A().a;",
        DiagnosticCode::TypeMismatch,
        "not assignable to `string`",
    );
    assert_rejected(
        "class A { constructor(public a?: number) {} } const n: number = new A().a;",
        DiagnosticCode::TypeMismatch,
        "not assignable to `number`",
    );
    assert_rejected(
        "class A { constructor(public a) {} }",
        DiagnosticCode::TypeMismatch,
        "implicitly has an `any` type",
    );
    assert_rejected(
        "class A { constructor(public a = [1]) {} }",
        DiagnosticCode::UnsupportedSyntax,
        "needs a type annotation",
    );
}

#[test]
fn a_parameter_property_follows_the_member_rules() {
    assert_rejected(
        "class A { a: number = 1; constructor(public a: number) {} }",
        DiagnosticCode::DuplicateDeclaration,
        "duplicate declaration of `a`",
    );
    assert_rejected(
        "class A { constructor(public a: number, private a: number) {} }",
        DiagnosticCode::DuplicateDeclaration,
        "duplicate declaration of `a`",
    );
    assert_rejected(
        "class A { constructor(private a: number) {} } const n: number = new A(1).a;",
        DiagnosticCode::TypeMismatch,
        "is private",
    );
    assert_rejected(
        "class A { constructor(protected a: number) {} } const n: number = new A(1).a;",
        DiagnosticCode::TypeMismatch,
        "is protected",
    );
    assert_accepted(
        "class A { constructor(protected a: number) {} } \
         class B extends A { constructor(public a: number) { super(a); } } const n: number = new B(1).a;",
    );
    assert_rejected(
        "class A { constructor(public a: number) {} } \
         class B extends A { constructor(private a: number) { super(a); } }",
        DiagnosticCode::TypeMismatch,
        "`a` is private in class `B` but public in the base class `A`",
    );
    assert_rejected(
        "class A { constructor(private a: number) {} } class B { constructor(private a: number) {} } \
         const b: B = new A(1);",
        DiagnosticCode::TypeMismatch,
        "not assignable to `B`",
    );
}

#[test]
fn a_readonly_parameter_property_is_assignable_only_in_the_constructor() {
    assert_accepted("class A { constructor(public readonly a: number) { this.a = a + 1; } }");
    assert_rejected(
        "class A { constructor(readonly a: number) {} m(): void { this.a = 2; } }",
        DiagnosticCode::TypeMismatch,
        "cannot mutate readonly property `a`",
    );
    assert_rejected(
        "class A { constructor(public a: number) {} m(): void { this.a = 'x'; } }",
        DiagnosticCode::TypeMismatch,
        "not assignable to property `a`",
    );
}

#[test]
fn only_a_constructor_implementation_may_declare_parameter_properties() {
    for source in [
        "class A { constructor(public a: number); constructor(public a: string); constructor(a: any) {} }",
        "class A { run(public a: number): number { return a; } }",
        "class A { constructor(public ...items: number[]) {} }",
        "class A { constructor(public { a }: { a: number }) {} }",
        "function f(public a: number): number { return a; }",
    ] {
        let compiled = compile_with(source, CompilerOptions::default());
        assert!(
            compiled.output.is_none() && !compiled.diagnostics.is_empty(),
            "`{source}` must be refused: {:?}",
            compiled.diagnostics
        );
    }
    // The implementation alone may declare them.
    assert_accepted(
        "class A { constructor(a: number); constructor(a: string); constructor(public a: any) {} } \
         const x = new A(1);",
    );
}

#[test]
fn parameter_modifiers_must_be_in_order_and_not_repeated() {
    for source in [
        "class A { constructor(readonly public a: number) {} }",
        "class A { constructor(public public a: number) {} }",
        "class A { constructor(readonly readonly a: number) {} }",
    ] {
        let compiled = compile_with(source, CompilerOptions::default());
        assert!(
            compiled.output.is_none() && !compiled.diagnostics.is_empty(),
            "`{source}` must be refused"
        );
    }
}

#[test]
fn a_parameter_property_class_is_lowered_for_an_es2020_target() {
    let compiled = compile_with(
        "class A { z: number = 5; constructor(public a: number, private b: number) {} }",
        CompilerOptions {
            target: EcmaTarget::Es2020,
            ..CompilerOptions::default()
        },
    );
    assert!(
        compiled.diagnostics.is_empty(),
        "{:?}",
        compiled.diagnostics
    );
    let output = compiled.output.unwrap().artifacts[ENTRY].javascript.clone();
    assert!(
        output.contains("constructor(a, b) { this.a = a; this.b = b; this.z = 5;"),
        "parameter properties first, then fields: {output}"
    );
    assert!(
        !output.contains("class A { a;"),
        "no native field declarations: {output}"
    );
}

#[test]
fn declaration_output_lists_property_members_first_and_prints_optional_undefined() {
    let compiled = compile_with(
        "export class A { z: number = 5; \
         constructor(public x: number, private y: string, protected readonly w: number = 3, \
         readonly v?: string, public flag = true) {} m(): number { return 1; } }",
        CompilerOptions {
            declaration: true,
            ..CompilerOptions::default()
        },
    );
    assert!(
        compiled.diagnostics.is_empty(),
        "{:?}",
        compiled.diagnostics
    );
    assert_eq!(
        compiled.output.unwrap().artifacts[ENTRY]
            .declaration
            .as_deref(),
        Some(
            "export declare class A {\n    x: number;\n    private y;\n    protected readonly w: number;\n    \
             readonly v?: string | undefined;\n    flag: boolean;\n    z: number;\n    \
             constructor(x: number, y: string, w?: number, v?: string | undefined, flag?: boolean);\n    \
             m(): number;\n}\n"
        )
    );
}
