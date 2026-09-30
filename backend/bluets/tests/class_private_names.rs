// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ECMAScript private names (J.3.2.5): `#field`, `#method()`, `get #a()`,
//! their static forms and `#name in object`. Verdicts on real programs are
//! pinned against TypeScript by the `class-private-name-*` matrix fixtures;
//! these tests pin codes, messages and emitted text.

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

const COUNTER: &str = "class Counter { #count: number = 0; static #made: number = 0; \
                       #bump(step: number): number { this.#count = this.#count + step; return this.#count; } \
                       get #double(): number { return this.#count * 2; } \
                       set #double(value: number) { this.#count = value / 2; } \
                       static #make(): Counter { Counter.#made = Counter.#made + 1; return new Counter(); } \
                       run(other: Counter): number { this.#double = 8; \
                         return this.#bump(1) + other.#count + this.#double + Counter.#make().#count; } } ";

#[test]
fn private_names_keep_their_text_and_lose_only_their_annotations() {
    let compiled = compile_with(COUNTER, CompilerOptions::default());
    assert!(
        compiled.diagnostics.is_empty(),
        "{:?}",
        compiled.diagnostics
    );
    let output = compiled.output.unwrap().artifacts[ENTRY].javascript.clone();
    for kept in [
        "#count = 0;",
        "static #made = 0;",
        "#bump(step)",
        "get #double()",
        "set #double(value)",
        "static #make()",
        "this.#count = this.#count + step;",
        "other.#count",
        "Counter.#make().#count",
    ] {
        assert!(output.contains(kept), "{kept} missing from {output}");
    }
    assert!(!output.contains(": number"), "{output}");
}

#[test]
fn a_private_name_is_reachable_only_inside_its_own_class_body() {
    assert_accepted(&format!(
        "{COUNTER} const n: number = new Counter().run(new Counter());"
    ));
    for access in [
        "c.#count",
        "c.#count + 1",
        "(c).#count",
        "Counter.#made",
        "make().#count",
    ] {
        let source = format!(
            "{COUNTER} const c = new Counter(); function make(): Counter {{ return c; }} const v = {access};"
        );
        assert_rejected(
            &source,
            DiagnosticCode::TypeMismatch,
            "is private and only accessible within class `Counter`",
        );
    }
    assert_rejected(
        "class A { #x: number = 1; } class B extends A { run(): number { return this.#x; } }",
        DiagnosticCode::TypeMismatch,
        "`#x` is private",
    );
    assert_rejected(
        "class A { #x: number = 1; run(): number { return this.#y; } }",
        DiagnosticCode::TypeMismatch,
        "does not exist on type",
    );
    assert_rejected(
        "class A { #x: number = 1; run(): number { return this.#y + 1; } }",
        DiagnosticCode::TypeMismatch,
        "does not exist on type",
    );
}

#[test]
fn a_private_name_is_scoped_to_its_class_and_never_overridden() {
    assert_accepted(
        "class Base { #x: number = 1; baseX(): number { return this.#x; } } \
         class Derived extends Base { #x: string = 's'; derivedX(): string { return this.#x; } } \
         const d = new Derived(); const n: number = d.baseX(); const s: string = d.derivedX();",
    );
    assert_accepted(
        "class A { #x: number = 1; x: string = 's'; run(): number { return this.#x; } } \
         const n: string = new A().x;",
    );
    // An instance member is not a static one.
    assert_rejected(
        "class A { static #s: number = 1; run(): number { return this.#s; } }",
        DiagnosticCode::TypeMismatch,
        "does not exist on type",
    );
}

#[test]
fn private_names_are_typed_like_other_members() {
    assert_rejected(
        "class A { #x: number = 1; run(): void { this.#x = 's'; } }",
        DiagnosticCode::TypeMismatch,
        "not assignable to property `#x`",
    );
    assert_rejected(
        "class A { #m(x: number): number { return x; } run(): number { return this.#m('s'); } }",
        DiagnosticCode::TypeMismatch,
        "",
    );
    assert_rejected(
        "class A { readonly #x: number = 1; run(): void { this.#x = 2; } }",
        DiagnosticCode::TypeMismatch,
        "cannot mutate readonly property `#x`",
    );
    assert_accepted("class A { readonly #x: number; constructor(v: number) { this.#x = v; } }");
    assert_rejected(
        "class A { get #v(): number { return 1; } run(): void { this.#v = 2; } }",
        DiagnosticCode::TypeMismatch,
        "cannot mutate readonly property `#v`",
    );
    assert_accepted(
        "class A { #x: number = 1; run(): number { this.#x += 2; this.#x = this.#x * 2; return this.#x + 1; } }",
    );
}

#[test]
fn definite_assignment_applies_to_private_fields() {
    assert_rejected(
        "class A { #x: number; }",
        DiagnosticCode::TypeMismatch,
        "not definitely assigned",
    );
    assert_accepted(
        "class A { #a!: number; #b?: number; #c: number; constructor() { this.#c = 1; } }",
    );
}

#[test]
fn classes_with_private_names_are_nominal() {
    assert_rejected(
        "class A { #x: number = 1; } const a: A = { };",
        DiagnosticCode::TypeMismatch,
        "not assignable to `A`",
    );
    assert_rejected(
        "class A { #x: number = 1; } class B { #x: number = 1; } const b: B = new A();",
        DiagnosticCode::TypeMismatch,
        "not assignable to `B`",
    );
    assert_accepted("class A { #x: number = 1; } class B extends A {} const a: A = new B();");
    assert_rejected(
        "interface H { x: number } class A { #x: number = 1; } const h: H = new A();",
        DiagnosticCode::TypeMismatch,
        "not assignable to `H`",
    );
}

#[test]
fn a_private_name_in_an_optional_chain_or_with_a_modifier_is_an_error() {
    // An optional read is already refused as a whole; a private name adds
    // nothing that could make it acceptable.
    let found = diagnostics(
        "class A { #x: number = 1; run(o: A | undefined): number | undefined { return o?.#x; } }",
    );
    assert!(
        !found.is_empty(),
        "an optional chain on a private name must not compile"
    );
    for source in [
        "class A { private #x: number = 1; }",
        "class A { public #m() {} }",
        "class A { #constructor: number = 1; }",
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
}

#[test]
fn names_may_not_collide_within_a_class() {
    assert_rejected(
        "class A { #x: number = 1; #x: number = 2; }",
        DiagnosticCode::DuplicateDeclaration,
        "duplicate declaration of `#x`",
    );
    assert_rejected(
        "class A { #x: number = 1; get #x(): number { return 1; } }",
        DiagnosticCode::DuplicateDeclaration,
        "duplicate declaration of `#x`",
    );
}

#[test]
fn a_brand_check_is_valid_only_for_a_name_the_enclosing_class_declares() {
    assert_accepted(
        "class Brand { #tag: number = 1; static has(v: any): boolean { return #tag in v; } \
         check(v: any): boolean { return #tag in v; } } const b: boolean = Brand.has(new Brand());",
    );
    assert_rejected(
        "class A { #x: number = 1; } const b = #x in new A();",
        DiagnosticCode::UnknownName,
        "not declared by the enclosing class",
    );
    assert_rejected(
        "class A { #x: number = 1; run(o: any): boolean { return #y in o; } }",
        DiagnosticCode::UnknownName,
        "`#y` is not declared by the enclosing class",
    );
    assert_rejected(
        "class A { #x: number = 1; run(): number { return #x; } }",
        DiagnosticCode::TypeMismatch,
        "only allowed after a `.` or before `in`",
    );
    // Declared by a method, an accessor or a parameter property's class.
    assert_accepted(
        "class A { #m() {} get #g(): number { return 1; } \
         run(o: any): boolean { return #m in o && #g in o; } }",
    );
}

#[test]
fn private_names_and_parameter_properties_coexist() {
    assert_accepted(
        "class A { #secret: number = 1; constructor(public open: number, private hidden: number) {} \
         run(): number { return this.#secret + this.open + this.hidden; } }",
    );
}

#[test]
fn private_names_are_refused_on_an_es2020_target() {
    for source in [
        "class A { #x: number = 1; }",
        "class A { #m(): void {} }",
        "class A { get #g(): number { return 1; } }",
    ] {
        let compiled = compile_with(
            source,
            CompilerOptions {
                target: EcmaTarget::Es2020,
                ..CompilerOptions::default()
            },
        );
        assert!(compiled.output.is_none(), "`{source}`");
        assert!(
            compiled
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == DiagnosticCode::UnsupportedSyntax),
            "`{source}`: {:?}",
            compiled.diagnostics
        );
    }
}

#[test]
fn declaration_output_declares_all_private_names_once_as_private() {
    let compiled = compile_with(
        "export class A { a: number = 1; #x: number = 1; b(): number { return 1; } #m(): void {} \
         get #g(): number { return 1; } static #s = 1; c: number = 2; } \
         export class B extends A { d: number = 3; } \
         export class C extends A { #y = 1; } \
         export class D { static #only = 1; e: number = 1; private p: number = 1; }",
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
            "export declare class A {\n    #private;\n    a: number;\n    b(): number;\n    c: number;\n}\n\
             export declare class B extends A {\n    d: number;\n}\n\
             export declare class C extends A {\n    #private;\n}\n\
             export declare class D {\n    #private;\n    e: number;\n    private p;\n}\n"
        )
    );
}
