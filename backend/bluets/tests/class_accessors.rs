// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `get` / `set` class accessors (J.3.2.4): erasure, typing, the grammar and
//! pairing rules, kind conflicts across inheritance, and declaration output.
//! Verdicts on real programs are pinned against TypeScript by the
//! `class-accessor-*` matrix fixtures; these tests pin codes, messages and
//! emitted text.

use blueice_bluets::{
    compile, CompilerOptions, Diagnostic, DiagnosticCode, MapLoader, ModuleSource,
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

#[test]
fn annotations_and_accessibility_are_erased_and_accessors_are_emitted_unchanged() {
    let compiled = compile_with(
        "class A { private _v: number = 1; \
         get v(): number { return this._v; } set v(value: number) { this._v = value; } \
         private static get s(): string { return 'x'; } protected set p(value: number) {} }",
        CompilerOptions::default(),
    );
    assert!(
        compiled.diagnostics.is_empty(),
        "{:?}",
        compiled.diagnostics
    );
    let output = compiled.output.unwrap().artifacts[ENTRY].javascript.clone();
    for kept in ["get v()", "set v(value)", "static get s()", "set p(value)"] {
        assert!(output.contains(kept), "{kept} missing from {output}");
    }
    for erased in ["private", "protected", ": number", ": string"] {
        assert!(!output.contains(erased), "{erased} survived in {output}");
    }
}

#[test]
fn a_getter_reads_its_type_and_a_getter_only_property_is_readonly() {
    assert_accepted(
        "class A { get v(): number { return 1; } } const a = new A(); const n: number = a.v;",
    );
    assert_rejected(
        "class A { get v(): number { return 1; } } const a = new A(); const s: string = a.v;",
        DiagnosticCode::TypeMismatch,
        "not assignable to `string`",
    );
    assert_rejected(
        "class A { get v(): number { return 1; } } const a = new A(); a.v = 2;",
        DiagnosticCode::TypeMismatch,
        "cannot mutate readonly property `v`",
    );
    assert_rejected(
        "class A { get v(): number { return 1; } m(): void { this.v = 2; } }",
        DiagnosticCode::TypeMismatch,
        "cannot mutate readonly property `v`",
    );
    // A constructor may not assign a getter-only property either.
    assert_rejected(
        "class A { get v(): number { return 1; } constructor() { this.v = 2; } }",
        DiagnosticCode::TypeMismatch,
        "cannot mutate readonly property `v`",
    );
}

#[test]
fn a_setter_makes_the_property_writable_and_types_the_assignment() {
    assert_accepted(
        "class A { private _v: number = 1; get v(): number { return this._v; } \
         set v(value: number) { this._v = value; } inc(): number { this.v = this.v + 1; return this.v; } } \
         const a = new A(); a.v = 3;",
    );
    assert_rejected(
        "class A { set v(value: number) {} } const a = new A(); a.v = 's';",
        DiagnosticCode::TypeMismatch,
        "not assignable to property `v`",
    );
    // A setter-only property reads as the setter's parameter type.
    assert_accepted("class A { set v(value: number) {} } const n: number = new A().v;");
    assert_accepted("class A { static get n(): number { return 1; } static set n(value: number) {} } A.n = 2; const x: number = A.n;");
}

#[test]
fn accessor_bodies_are_checked_like_methods() {
    assert_rejected(
        "class A { get v(): number { return 's'; } }",
        DiagnosticCode::ReturnTypeMismatch,
        "",
    );
    assert_rejected(
        "class A { get v(): number { } }",
        DiagnosticCode::ReturnTypeMismatch,
        "can complete without returning a value",
    );
    assert_rejected(
        "class A { set v(value: number) { return 1; } }",
        DiagnosticCode::ReturnTypeMismatch,
        "",
    );
    assert_rejected(
        "class A { set v(value: number) { const n: string = value; } }",
        DiagnosticCode::TypeMismatch,
        "not assignable to `string`",
    );
}

#[test]
fn the_accessor_grammar_is_enforced() {
    for source in [
        "class A { get v(x: number): number { return 1; } }",
        "class A { set v(x: number, y: number) {} }",
        "class A { set v() {} }",
        "class A { set v(x?: number) {} }",
        "class A { set v(x: number = 1) {} }",
        "class A { set v(...x: number[]) {} }",
        "class A { set v(x: number): void {} }",
    ] {
        let compiled = compile_with(source, CompilerOptions::default());
        assert!(
            compiled.output.is_none()
                && compiled
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.code == DiagnosticCode::ParseError),
            "`{source}` must be a parse error: {:?}",
            compiled.diagnostics
        );
    }
    assert_rejected(
        "class A { set v(value) {} }",
        DiagnosticCode::TypeMismatch,
        "implicitly `any` parameter",
    );
}

#[test]
fn getters_infer_their_body_and_pairs_need_compatible_types_and_accessibility() {
    assert_accepted("class A { get v() { return 1; } }");
    // The setter's parameter annotation supplies it.
    assert_accepted("class A { get v() { return 1; } set v(value: number) {} }");
    assert_rejected(
        "class A { get v(): number { return 1; } set v(value: string) {} }",
        DiagnosticCode::UnsupportedSyntax,
        "have different types",
    );
    assert_rejected(
        "class A { private get v(): number { return 1; } public set v(value: number) {} }",
        DiagnosticCode::TypeMismatch,
        "must have the same accessibility",
    );
    assert_accepted(
        "class A { protected get v(): number { return 1; } protected set v(value: number) {} }",
    );
}

#[test]
fn names_may_not_collide_across_accessors_fields_and_methods() {
    assert_rejected(
        "class A { get v(): number { return 1; } get v(): number { return 2; } }",
        DiagnosticCode::DuplicateDeclaration,
        "duplicate declaration of `v`",
    );
    assert_rejected(
        "class A { set v(x: number) {} set v(x: number) {} }",
        DiagnosticCode::DuplicateDeclaration,
        "duplicate declaration of `v`",
    );
    assert_rejected(
        "class A { v: number = 1; get v(): number { return 1; } }",
        DiagnosticCode::DuplicateDeclaration,
        "duplicate declaration of `v`",
    );
    assert_rejected(
        "class A { v(): number { return 1; } get v(): number { return 1; } }",
        DiagnosticCode::DuplicateDeclaration,
        "duplicate declaration of `v`",
    );
    // An instance accessor and a static one are different members.
    assert_accepted(
        "class A { get v(): number { return 1; } static get v(): string { return 's'; } }",
    );
    // Members named like the accessor keywords stay members.
    assert_accepted(
        "class A { get: number = 1; set(): number { return 2; } \
         get value(): number { return this.get + this.set(); } }",
    );
}

#[test]
fn a_member_may_not_change_kind_when_it_is_redeclared() {
    assert_accepted(
        "class B { get v(): number { return 1; } set v(value: number) {} } \
         class D extends B { get v(): number { return 2; } set v(value: number) {} }",
    );
    assert_rejected(
        "class B { v: number = 1; } class D extends B { get v(): number { return 2; } }",
        DiagnosticCode::TypeMismatch,
        "as an accessor but the base class defines it as a property",
    );
    assert_rejected(
        "class B { get v(): number { return 1; } } class D extends B { v: number = 2; }",
        DiagnosticCode::TypeMismatch,
        "as a property but the base class defines it as an accessor",
    );
    assert_rejected(
        "class B { v(): number { return 1; } } class D extends B { get v(): number { return 2; } }",
        DiagnosticCode::TypeMismatch,
        "as an accessor but the base class defines it as a method",
    );
    assert_rejected(
        "class B { get v(): number { return 1; } } class D extends B { v(): number { return 2; } }",
        DiagnosticCode::TypeMismatch,
        "as a method but the base class defines it as an accessor",
    );
    assert_rejected(
        "class B { get v(): number { return 1; } } class D extends B { get v(): string { return 'a'; } }",
        DiagnosticCode::TypeMismatch,
        "not assignable to the base property type `number`",
    );
}

#[test]
fn accessibility_rules_apply_to_accessors() {
    assert_rejected(
        "class A { private get secret(): number { return 1; } } const n: number = new A().secret;",
        DiagnosticCode::TypeMismatch,
        "is private",
    );
    assert_rejected(
        "class A { private get secret(): number { return 1; } } const a: A = { secret: 1 };",
        DiagnosticCode::TypeMismatch,
        "not assignable to `A`",
    );
    assert_accepted(
        "class A { protected get inner(): number { return 1; } } \
         class B extends A { twice(): number { return this.inner * 2; } } const n: number = new B().twice();",
    );
    assert_rejected(
        "class A { get v(): number { return 1; } } class B extends A { private get v(): number { return 2; } }",
        DiagnosticCode::TypeMismatch,
        "is private in class `B` but public in the base class `A`",
    );
}

#[test]
fn an_accessor_class_is_structurally_a_plain_property_type() {
    assert_accepted(
        "class A { get v(): number { return 1; } } \
         const x: { v: number } = new A(); const y: { readonly v: number } = new A();",
    );
}

#[test]
fn computed_accessor_names_are_refused() {
    let source = "class A { get [k](): number { return 1; } }";
    let compiled = compile_with(source, CompilerOptions::default());
    assert!(
        compiled.output.is_none() && !compiled.diagnostics.is_empty(),
        "`{source}` must be refused: {:?}",
        compiled.diagnostics
    );
}

#[test]
fn declaration_output_prints_accessors_like_typescript() {
    let compiled = compile_with(
        "export class A { get v(): number { return 1; } set v(x: number) {} \
         static get n(): string { return 's'; } protected get p(): number { return 1; } \
         private get h(): number { return 1; } private set h(x: number) {} \
         get u() { return 1; } set u(value: number) {} }",
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
            "export declare class A {\n    get v(): number;\n    set v(x: number);\n    \
             static get n(): string;\n    protected get p(): number;\n    private get h();\n    \
             private set h(value);\n    get u(): number;\n    set u(value: number);\n}\n"
        )
    );
}
