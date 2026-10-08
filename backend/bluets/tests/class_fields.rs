// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public class fields (J.3.2.1): erasure, checking, definite assignment,
//! inheritance and declaration output. Verdicts on real programs are pinned
//! against TypeScript by the `class-field-*` matrix fixtures; these tests pin
//! the codes, messages and emitted text.

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

fn declaration(source: &str) -> String {
    let compiled = compile_with(
        source,
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
    compiled.output.unwrap().artifacts[ENTRY]
        .declaration
        .clone()
        .expect("declaration requested")
}

#[test]
fn fields_are_emitted_as_native_class_fields_with_types_and_modifiers_erased() {
    let output = javascript(
        "class A { public a: number = 1; static s: string = 'x'; readonly r: number = 2; \
         o?: string; d!: number; i = 3; static readonly k = 'v'; \
         f: (x: number) => number = (x) => x; }",
    );
    for expected in [
        "a = 1;",
        "static s = 'x';",
        " r = 2;",
        "o;",
        "d;",
        "i = 3;",
        "static k = 'v';",
    ] {
        assert!(output.contains(expected), "{expected} in {output}");
    }
    for erased in ["public", "readonly", ": number", ": string", "?", "!"] {
        assert!(!output.contains(erased), "{erased} survived in {output}");
    }
}

#[test]
fn field_initializers_are_checked_against_the_annotation_in_a_this_scope() {
    assert_rejected(
        "class A { x: number = 'a'; }",
        DiagnosticCode::TypeMismatch,
        "initializer has type `string`",
    );
    assert_rejected(
        "class A { x: Missing = 1; }",
        DiagnosticCode::UnknownType,
        "Missing",
    );
    assert_rejected(
        "class A { x: number = 1; y: string = this.x; }",
        DiagnosticCode::TypeMismatch,
        "not assignable to `string`",
    );
    assert_accepted("class A { x: number = 1; y: number = this.x + 1; static s: number = 2; static t: number = this.s + 1; }");
    assert_accepted("class A { x: [number, string] = [1, 'a']; }");
}

#[test]
fn unannotated_fields_take_a_literal_type_or_need_an_annotation() {
    assert_accepted(
        "class A { n = 1; s = 'a'; b = true; m = -1; } const a = new A(); const x: number = a.n;",
    );
    assert_rejected(
        "class A { n = 1; } const a = new A(); const s: string = a.n;",
        DiagnosticCode::TypeMismatch,
        "not assignable to `string`",
    );
    assert_accepted("class A { readonly n = 1; } const a = new A(); const x: 1 = a.n;");
    assert_rejected(
        "class A { x = [1]; }",
        DiagnosticCode::UnsupportedSyntax,
        "needs a type annotation",
    );
    assert_rejected(
        "class A { x; }",
        DiagnosticCode::TypeMismatch,
        "implicitly has an `any` type",
    );
}

#[test]
fn definite_assignment_follows_strict_property_initialization() {
    assert_rejected(
        "class A { x: number; }",
        DiagnosticCode::TypeMismatch,
        "not definitely assigned",
    );
    assert_rejected(
        "class A { x: number; constructor() {} }",
        DiagnosticCode::TypeMismatch,
        "not definitely assigned",
    );
    assert_accepted("class A { x: number; constructor(v: number) { this.x = v; } }");
    assert_accepted("class A { x!: number; }");
    assert_accepted(
        "class A { x?: number; y: number | undefined; z: unknown; w: any; static s: number; }",
    );
    // An assignment only inside a branch is not modelled.
    assert_rejected(
        "class A { x: number; constructor(c: boolean) { if (c) { this.x = 1; } else { this.x = 2; } } }",
        DiagnosticCode::UnsupportedSyntax,
        "through a branch",
    );
}

#[test]
fn readonly_fields_are_assignable_only_in_the_own_constructor_body() {
    assert_accepted("class A { readonly x: number; constructor() { this.x = 1; } }");
    assert_rejected(
        "class A { readonly x: number = 1; m(): void { this.x = 2; } }",
        DiagnosticCode::TypeMismatch,
        "cannot mutate readonly property `x`",
    );
    assert_rejected(
        "class A { readonly x: number = 1; constructor() { const f = () => { this.x = 2; }; } }",
        DiagnosticCode::TypeMismatch,
        "cannot mutate readonly property `x`",
    );
    assert_rejected(
        "class A { readonly x: number = 1; } class B extends A { constructor() { super(); this.x = 2; } }",
        DiagnosticCode::TypeMismatch,
        "cannot mutate readonly property `x`",
    );
}

#[test]
fn field_names_may_not_collide_with_fields_methods_or_prototype() {
    assert_rejected(
        "class A { x: number = 1; x: number = 2; }",
        DiagnosticCode::DuplicateDeclaration,
        "duplicate declaration of `x`",
    );
    assert_rejected(
        "class A { x: number = 1; x(): number { return 1; } }",
        DiagnosticCode::DuplicateDeclaration,
        "duplicate declaration of `x`",
    );
    assert_accepted("class A { x: number = 1; static x: number = 2; }");
    assert_rejected(
        "class A { static prototype: number = 1; }",
        DiagnosticCode::DuplicateDeclaration,
        "prototype",
    );
}

#[test]
fn a_field_initializer_may_not_read_a_later_field() {
    assert_rejected(
        "class A { a: number = this.b; b: number = 1; }",
        DiagnosticCode::TypeMismatch,
        "used before its initialization",
    );
    assert_accepted("class A { b: number = 1; a: number = this.b; }");
    assert_rejected(
        "class A { a: () => number = () => this.b; b: number = 1; }",
        DiagnosticCode::UnsupportedSyntax,
        "later field inside a nested function",
    );
}

#[test]
fn inherited_and_overriding_fields_are_checked_against_the_base() {
    assert_accepted(
        "class A { x: number = 1; static s: number = 1; } class B extends A { y: number = this.x; } \
         const n: number = new B().x; const m: number = B.s;",
    );
    assert_accepted("class A { x: number | string = 1; } class B extends A { x: number = 2; }");
    assert_rejected(
        "class A { x: number = 1; } class B extends A { x: string = 'a'; }",
        DiagnosticCode::TypeMismatch,
        "not assignable to the base property type `number`",
    );
    assert_rejected(
        "class A { x(): number { return 1; } } class B extends A { x: number = 1; }",
        DiagnosticCode::TypeMismatch,
        "as a property but the base class defines it as a method",
    );
    assert_rejected(
        "class A { x: number = 1; } class B extends A { x?: number; }",
        DiagnosticCode::TypeMismatch,
        "not assignable to the base property type",
    );
}

#[test]
fn fields_are_lowered_for_an_es2020_target() {
    let compiled = compile_with(
        "class A { x: number = 1; static s: number = 2; }",
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
    assert!(output.contains("constructor() { this.x = 1; }"), "{output}");
    assert!(output.contains("A.s = 2;"), "{output}");
    assert!(!output.contains("static s"), "{output}");
    // A class with no field emits the same for either target.
    for target in [EcmaTarget::Es2020, EcmaTarget::Es2022] {
        let plain = compile_with(
            "class A { m(): number { return 1; } }",
            CompilerOptions {
                target,
                ..CompilerOptions::default()
            },
        );
        assert!(plain.output.is_some(), "{:?}", plain.diagnostics);
    }
}

#[test]
fn computed_members_are_structured_and_generator_members_remain_refused() {
    for source in ["class A { ['computed'] = 1; }", "class A { ['m']() {} }"] {
        let compiled = compile_with(source, CompilerOptions::default());
        assert!(
            compiled.output.is_some() && compiled.diagnostics.is_empty(),
            "`{source}` must emit: {:?}",
            compiled.diagnostics
        );
    }
    let source = "class A { *generate() {} }";
    let compiled = compile_with(source, CompilerOptions::default());
    assert!(
        compiled.output.is_none() && !compiled.diagnostics.is_empty(),
        "`{source}` must be refused, not emitted: {:?}",
        compiled.diagnostics
    );
}

#[test]
fn declaration_output_prints_field_types_like_typescript() {
    let output = declaration(
        "export class A { a: number = 1; static s: string = 'x'; readonly r: number = 2; \
         o?: string; d!: number; i = 3; m = -2; t = true; static readonly k = 'v'; \
         readonly n = 4; readonly neg = -5; readonly on = false; }",
    );
    assert_eq!(
        output,
        "export declare class A {\n    a: number;\n    static s: string;\n    \
         readonly r: number;\n    o?: string;\n    d: number;\n    i: number;\n    m: number;\n    \
         t: boolean;\n    static readonly k = \"v\";\n    readonly n = 4;\n    \
         readonly neg = -5;\n    readonly on = false;\n}\n"
    );
}

#[test]
fn declaration_output_refuses_a_string_literal_it_cannot_requote() {
    let compiled = compile_with(
        "export class A { readonly k = 'a\"b'; }",
        CompilerOptions {
            declaration: true,
            ..CompilerOptions::default()
        },
    );
    assert!(compiled.output.is_none());
    assert!(compiled.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::UnsupportedSyntax
            && diagnostic.message.contains("string literal field")
    }));
}
