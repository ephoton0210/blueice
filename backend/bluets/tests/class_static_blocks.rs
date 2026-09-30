// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `static { .. }` blocks and static initialization order (J.3.2.6). Verdicts
//! on real programs are pinned against TypeScript by the `class-static-*`
//! matrix fixtures and two emitted-output oracle cases; these tests pin codes,
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

#[test]
fn a_static_block_is_emitted_as_written_with_its_annotations_erased() {
    let compiled = compile_with(
        "class A { static count: number = 0; static { const local: number = this.count + 1; A.count = local; } }",
        CompilerOptions::default(),
    );
    assert!(
        compiled.diagnostics.is_empty(),
        "{:?}",
        compiled.diagnostics
    );
    let output = compiled.output.unwrap().artifacts[ENTRY].javascript.clone();
    assert!(
        output.contains("static { const local= this.count + 1; A.count = local; }"),
        "{output}"
    );
    assert!(!output.contains(": number"), "{output}");
}

#[test]
fn a_static_block_sees_the_constructor_side_and_private_static_names() {
    assert_accepted(
        "class R { static count: number = 0; static #secret: number = 7; \
         static { R.count = R.#secret * 2; this.count = this.count + 1; } }",
    );
    assert_rejected(
        "class A { x: number = 1; static { const n: number = this.x; } }",
        DiagnosticCode::TypeMismatch,
        "does not exist on type",
    );
    assert_rejected(
        "class A { static #hidden: number = 1; } class B { static { const n: number = A.#hidden; } }",
        DiagnosticCode::TypeMismatch,
        "is private and only accessible within class `A`",
    );
    assert_rejected(
        "class A { static count: number = 0; static { A.count = 's'; } }",
        DiagnosticCode::TypeMismatch,
        "not assignable to property `count`",
    );
    assert_accepted(
        "class Base { static seed: number = 1; } \
         class Derived extends Base { static { const n: number = super.seed + this.seed; } }",
    );
}

#[test]
fn a_static_block_cannot_return_or_await() {
    assert_rejected(
        "class A { static { return; } }",
        DiagnosticCode::ReturnTypeMismatch,
        "cannot be used inside a class static block",
    );
    assert_accepted(
        "class A { static { const f = (): number => { return 1; }; const n: number = f(); } }",
    );
    let found = diagnostics("class A { static { const p: number = await 1; } }");
    assert!(
        !found.is_empty(),
        "an await in a static block must not compile"
    );
}

#[test]
fn statements_in_a_static_block_are_checked_like_any_function_body() {
    assert_accepted(
        "class A { static value: number = 0; static { let i: number = 0; \
         while (i < 3) { A.value = A.value + i; i = i + 1; } \
         if (A.value > 2) { A.value = A.value * 2; } else { A.value = 0; } \
         try { A.value = 1; } catch (e) { A.value = 2; } finally { A.value = A.value + 10; } } }",
    );
    assert_rejected(
        "class A { static { const n: string = 1; } }",
        DiagnosticCode::TypeMismatch,
        "not assignable to `string`",
    );
}

#[test]
fn static_initializers_may_not_read_a_later_static_field() {
    assert_rejected(
        "class A { static a: number = A.b; static b: number = 1; }",
        DiagnosticCode::TypeMismatch,
        "used before its initialization",
    );
    assert_rejected(
        "class A { static a: number = this.b; static b: number = 1; }",
        DiagnosticCode::TypeMismatch,
        "used before its initialization",
    );
    assert_accepted(
        "class A { static a: number = 1; static b: number = A.a + 1; static c: number = this.b + this.a; }",
    );
    // An instance initializer runs after every static one.
    assert_accepted("class A { x: number = A.later; static later: number = 1; }");
}

#[test]
fn a_static_block_may_not_read_a_static_field_declared_after_it() {
    assert_rejected(
        "class A { static { const n: number = this.later; } static later: number = 1; }",
        DiagnosticCode::TypeMismatch,
        "property `later` is used before its initialization",
    );
    assert_rejected(
        "class A { static { A.later = 2; } static later: number = 1; }",
        DiagnosticCode::TypeMismatch,
        "used before its initialization",
    );
    assert_accepted("class A { static earlier: number = 1; static { const n: number = this.earlier; } static later: number = 2; }");
    // A read hidden in a nested function is not modelled.
    assert_rejected(
        "class A { static { const f = (): number => this.later; } static later: number = 1; }",
        DiagnosticCode::UnsupportedSyntax,
        "inside a nested function",
    );
}

#[test]
fn blocks_and_static_fields_interleave_and_blocks_may_repeat() {
    assert_accepted(
        "class A { static x: number = 1; static { A.x = A.x + 1; } static { A.x = A.x * 10; } } \
         const n: number = A.x;",
    );
}

#[test]
fn a_static_block_is_refused_on_an_es2020_target() {
    let compiled = compile_with(
        "class A { static { } }",
        CompilerOptions {
            target: EcmaTarget::Es2020,
            ..CompilerOptions::default()
        },
    );
    assert!(compiled.output.is_none());
    assert!(compiled.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::UnsupportedSyntax
            && diagnostic
                .message
                .contains("static blocks need the ES2022 target")
    }));
}

#[test]
fn declaration_output_omits_static_blocks() {
    let compiled = compile_with(
        "export class A { static count: number = 0; static { A.count = 1; } a: number = 1; static { A.count = 2; } }",
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
        Some("export declare class A {\n    static count: number;\n    a: number;\n}\n")
    );
}

#[test]
fn a_member_named_static_is_still_a_member() {
    assert_accepted("class A { static: number = 1; static static(): number { return 2; } }");
}
