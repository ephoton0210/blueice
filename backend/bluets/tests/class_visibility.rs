// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `private` and `protected` class members (J.3.2.2): erasure, access checks
//! in every expression form, nominal assignability, override rules,
//! constructor accessibility and declaration output. Verdicts on real programs
//! are pinned against TypeScript by the `class-visibility-*` matrix fixtures;
//! these tests pin the codes, messages and emitted text.

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

const BOX: &str = "class Box { private secret: number = 1; protected shared: number = 2; \
                   private hidden(): number { return 1; } protected open(): number { return 2; } \
                   private static count: number = 3; protected static tag: string = 't'; \
                   public label: string = 'l'; } ";

#[test]
fn accessibility_modifiers_are_erased_from_the_output() {
    let compiled = compile_with(
        "class A { public a: number = 1; private b: number = 2; protected c: number = 3; \
         private static d: number = 4; protected static e: number = 5; \
         private readonly f: number = 6; \
         private m(): number { return 1; } protected static n(): number { return 2; } \
         protected constructor() {} }",
        CompilerOptions::default(),
    );
    assert!(
        compiled.diagnostics.is_empty(),
        "{:?}",
        compiled.diagnostics
    );
    let output = compiled.output.unwrap().artifacts[ENTRY].javascript.clone();
    for keyword in ["public", "private", "protected", "readonly", ": number"] {
        assert!(!output.contains(keyword), "{keyword} survived in {output}");
    }
    for kept in [
        "a = 1;",
        "b = 2;",
        "static d = 4;",
        "static n()",
        "constructor()",
    ] {
        assert!(output.contains(kept), "{kept} missing from {output}");
    }
}

#[test]
fn a_private_member_is_reachable_only_inside_its_class() {
    for access in [
        "b.secret",
        "b.secret + 1",
        "(b).secret",
        "new Box().secret",
        "boxes[0].secret",
        "make().secret",
        "Box.count",
    ] {
        let source = format!(
            "{BOX} const b = new Box(); const boxes: Box[] = [b]; \
             function make(): Box {{ return b; }} const v = {access};"
        );
        assert_rejected(
            &source,
            DiagnosticCode::TypeMismatch,
            "is private and only accessible within class `Box`",
        );
    }
    assert_rejected(
        &format!("{BOX} const b = new Box(); b.secret = 2;"),
        DiagnosticCode::TypeMismatch,
        "is private",
    );
    assert_rejected(
        &format!("{BOX} const b = new Box(); const n: number = b.hidden();"),
        DiagnosticCode::TypeMismatch,
        "`hidden` is private",
    );
    assert_rejected(
        &format!("{BOX} function peek(b: Box): number {{ return b.secret; }}"),
        DiagnosticCode::TypeMismatch,
        "is private",
    );
}

#[test]
fn an_optional_chain_does_not_bypass_accessibility() {
    let found = diagnostics(&format!(
        "{BOX} const b: Box | undefined = new Box(); const v = b?.secret;"
    ));
    assert!(
        found
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch),
        "{found:?}"
    );
    let found = diagnostics(&format!("{BOX} const b = new Box(); const v = b?.secret;"));
    assert!(
        !found.is_empty(),
        "an optional read of a private member must not compile"
    );
}

#[test]
fn a_protected_member_is_reachable_in_the_class_and_its_subclasses_only() {
    assert_rejected(
        &format!("{BOX} const n: number = new Box().shared;"),
        DiagnosticCode::TypeMismatch,
        "is protected and only accessible within class `Box`",
    );
    assert_rejected(
        &format!("{BOX} const t: string = Box.tag;"),
        DiagnosticCode::TypeMismatch,
        "is protected",
    );
    assert_accepted(&format!(
        "{BOX} class Sub extends Box {{ read(): number {{ return this.shared + this.open(); }} \
         static tag2(): string {{ return Sub.tag + Box.tag; }} \
         viaSuper(): number {{ return super.open(); }} }}"
    ));
    // Through an instance of an unrelated or base type, not `this` type.
    assert_rejected(
        &format!(
            "{BOX} class Sub extends Box {{ peek(other: Box): number {{ return other.shared; }} }}"
        ),
        DiagnosticCode::TypeMismatch,
        "is protected",
    );
    assert_rejected(
        &format!("{BOX} class Sub extends Box {{ read(): number {{ return this.secret; }} }}"),
        DiagnosticCode::TypeMismatch,
        "is private",
    );
    assert_rejected(
        &format!("{BOX} class Sub extends Box {{ read(): number {{ return super.secret; }} }}"),
        DiagnosticCode::TypeMismatch,
        "is private",
    );
}

#[test]
fn a_class_body_may_use_the_members_of_other_instances_and_nested_functions() {
    assert_accepted(&format!(
        "{BOX} class Other extends Box {{ \
         read(other: Other, list: Other[]): number {{ \
           const f = (): number => this.shared + other.shared + list[0].shared; return f(); }} }}"
    ));
    assert_accepted(
        "class A { private v: number = 1; \
         sum(other: A, list: A[]): number { \
           return (this).v + other.v + list[0].v + new A().v + A.make().v; } \
         static make(): A { return new A(); } }",
    );
}

#[test]
fn bracket_access_and_untyped_receivers_stay_permitted_as_in_typescript() {
    assert_accepted(&format!(
        "{BOX} const b = new Box(); const v = b['secret'];"
    ));
    assert_accepted(&format!(
        "{BOX} const a: any = new Box(); const v = a.secret;"
    ));
}

#[test]
fn an_unprovable_receiver_is_refused_rather_than_assumed_accessible() {
    let found = diagnostics(&format!(
        "{BOX} declare const u: unknown; const v = u.secret;"
    ));
    assert!(
        found.iter().any(|diagnostic| matches!(
            diagnostic.code,
            DiagnosticCode::UnsupportedSyntax | DiagnosticCode::TypeMismatch
        )),
        "{found:?}"
    );
}

#[test]
fn classes_with_restricted_members_are_nominal() {
    assert_rejected(
        "class Box { private secret: number = 1; } const b: Box = { secret: 1 };",
        DiagnosticCode::TypeMismatch,
        "not assignable to `Box`",
    );
    assert_rejected(
        "class A { private secret: number = 1; } class B { private secret: number = 1; } \
         const b: B = new A();",
        DiagnosticCode::TypeMismatch,
        "not assignable to `B`",
    );
    assert_accepted(
        "class A { private secret: number = 1; read(): number { return this.secret; } } \
         class B extends A {} const a: A = new B(); const n: number = a.read();",
    );
    // A private or protected member never satisfies a public structural type.
    assert_rejected(
        "interface Has { secret: number } class Box { private secret: number = 1; } \
         const h: Has = new Box();",
        DiagnosticCode::TypeMismatch,
        "not assignable to `Has`",
    );
    assert_rejected(
        "interface Has { value: number } class Box { protected value: number = 1; } \
         const h: Has = new Box();",
        DiagnosticCode::TypeMismatch,
        "not assignable to `Has`",
    );
    assert_accepted(
        "interface Has { value: number } class Box { value: number = 1; private hidden: number = 2; } \
         const h: Has = new Box();",
    );
}

#[test]
fn a_subclass_may_not_lower_the_accessibility_it_inherits() {
    assert_accepted(
        "class A { protected v: number = 1; protected m(): number { return 1; } } \
         class B extends A { public v: number = 2; public m(): number { return 2; } } \
         const n: number = new B().v + new B().m();",
    );
    assert_accepted(
        "class A { protected v: number = 1; } class B extends A { protected v: number = 2; }",
    );
    assert_rejected(
        "class A { private v: number = 1; } class B extends A { private v: number = 2; }",
        DiagnosticCode::TypeMismatch,
        "separate declarations of the private property `v`",
    );
    assert_rejected(
        "class A { private m(): number { return 1; } } class B extends A { m(): number { return 2; } }",
        DiagnosticCode::TypeMismatch,
        "private property `m`",
    );
    assert_rejected(
        "class A { public v: number = 1; } class B extends A { protected v: number = 2; }",
        DiagnosticCode::TypeMismatch,
        "`v` is protected in class `B` but public in the base class `A`",
    );
    assert_rejected(
        "class A { protected v: number = 1; } class B extends A { private v: number = 2; }",
        DiagnosticCode::TypeMismatch,
        "`v` is private in class `B` but protected in the base class `A`",
    );
    assert_rejected(
        "class A { protected v: number = 1; } class B extends A { protected v: string = 'a'; }",
        DiagnosticCode::TypeMismatch,
        "not assignable to the base property type `number`",
    );
}

#[test]
fn overloads_must_agree_on_accessibility() {
    assert_rejected(
        "class A { private read(x: number): number; public read(x: string): string; \
         read(x: any): any { return x; } }",
        DiagnosticCode::TypeMismatch,
        "must all be public, private or protected",
    );
    assert_accepted(
        "class A { private read(x: number): number; private read(x: string): string; \
         private read(x: any): any { return x; } use(): number { return this.read(1); } }",
    );
    assert_rejected(
        "class A { private constructor(x: number); public constructor(x: string); \
         constructor(x: any) {} }",
        DiagnosticCode::TypeMismatch,
        "overload signatures of the constructor",
    );
}

#[test]
fn constructor_accessibility_limits_where_a_class_can_be_constructed_and_extended() {
    assert_rejected(
        "class Box { private constructor() {} } const b = new Box();",
        DiagnosticCode::TypeMismatch,
        "constructor of class Box is private",
    );
    assert_rejected(
        "class Box { protected constructor() {} } const b = new Box();",
        DiagnosticCode::TypeMismatch,
        "constructor of class Box is protected",
    );
    assert_accepted(
        "class Box { private constructor() {} static make(): Box { return new Box(); } } \
         const b: Box = Box.make();",
    );
    assert_accepted(
        "class Base { protected constructor() {} static make(): Base { return new Base(); } } \
         class Derived extends Base { constructor() { super(); } static other(): Base { return new Base(); } } \
         const d = new Derived(); const b = Base.make();",
    );
    assert_rejected(
        "class Base { protected constructor() {} } class Derived extends Base {} const d = new Derived();",
        DiagnosticCode::TypeMismatch,
        "constructor of class Derived is protected",
    );
    assert_rejected(
        "class Box { private constructor() {} } class Derived extends Box {}",
        DiagnosticCode::TypeMismatch,
        "its constructor is private",
    );
    assert_rejected(
        "class Box { private constructor() {} } class Other { make(): Box { return new Box(); } }",
        DiagnosticCode::TypeMismatch,
        "constructor of class Box is private",
    );
}

#[test]
fn modifiers_out_of_order_or_on_unsupported_members_are_refused() {
    for source in [
        "class A { static private x: number = 1; }",
        "class A { readonly private x: number = 1; }",
        "class A { private private x: number = 1; }",
        "class A { private readonly m() {} }",
    ] {
        let compiled = compile_with(source, CompilerOptions::default());
        assert!(
            compiled.output.is_none() && !compiled.diagnostics.is_empty(),
            "`{source}` must be refused: {:?}",
            compiled.diagnostics
        );
    }
}

#[test]
fn a_member_named_like_a_modifier_is_still_a_member() {
    assert_accepted(
        "class A { private: number = 1; public(): number { return 2; } static = 3; \
         read(): number { return this.private + this.public() + this.static; } }",
    );
}

#[test]
fn declaration_output_follows_typescript_for_restricted_members() {
    let compiled = compile_with(
        "export class A { private a: number = 1; protected b: string = 'x'; c: number = 2; \
         private readonly d: number = 3; protected readonly e = 'k'; \
         private static f: number = 4; protected static g: number = 5; \
         private h?: string; \
         private m(x: number): number { return x; } protected n(): string { return 'n'; } \
         private static p(): void {} protected static q(): number { return 1; } \
         protected constructor(seed: number) {} }",
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
            "export declare class A {\n    private a;\n    protected b: string;\n    c: number;\n    \
             private readonly d;\n    protected readonly e = \"k\";\n    private static f;\n    \
             protected static g: number;\n    private h?;\n    private m;\n    protected n(): string;\n    \
             private static p;\n    protected static q(): number;\n    \
             protected constructor(seed: number);\n}\n"
        )
    );
}

#[test]
fn a_private_constructor_is_declared_without_parameters_and_overloads_once() {
    let compiled = compile_with(
        "export class A { private read(x: number): number; private read(x: string): string; \
         private read(x: any): any { return x; } \
         private constructor(a: number); private constructor(a: string); \
         private constructor(a: any) {} }",
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
        Some("export declare class A {\n    private read;\n    private constructor();\n}\n")
    );
}
