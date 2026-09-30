// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Target- and option-dependent class emit (J.3.3): the exact text each
//! combination of target and `useDefineForClassFields` produces, the option's
//! defaults and diagnostics, and what stays the same. Node parity with pinned
//! TypeScript is in `class_downlevel_oracle.rs`.

use blueice_bluets::{
    compile, CompilerOptions, Diagnostic, DiagnosticCode, EcmaTarget, MapLoader, ModuleSource,
};

const ENTRY: &str = "memory:///main.ts";

fn options(target: EcmaTarget, define: Option<bool>) -> CompilerOptions {
    CompilerOptions {
        target,
        use_define_for_class_fields: define,
        ..CompilerOptions::default()
    }
}

fn compile_with(source: &str, options: CompilerOptions) -> blueice_bluets::Compilation {
    compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        options,
    )
}

fn emit(source: &str, target: EcmaTarget, define: Option<bool>) -> String {
    let compiled = compile_with(source, options(target, define));
    assert!(
        compiled.diagnostics.is_empty(),
        "{:?}",
        compiled.diagnostics
    );
    compiled.output.unwrap().artifacts[ENTRY].javascript.clone()
}

fn diagnostics(source: &str, options: CompilerOptions) -> Vec<Diagnostic> {
    compile_with(source, options).diagnostics
}

const ES2022: EcmaTarget = EcmaTarget::Es2022;
const ES2020: EcmaTarget = EcmaTarget::Es2020;

#[test]
fn the_option_defaults_to_the_targets_semantics() {
    assert!(options(ES2022, None).defines_class_fields());
    assert!(!options(ES2020, None).defines_class_fields());
    assert!(options(ES2020, Some(true)).defines_class_fields());
    assert!(!options(ES2022, Some(false)).defines_class_fields());
    assert!(CompilerOptions::default().defines_class_fields());
}

#[test]
fn es2022_with_define_semantics_keeps_native_fields() {
    let output = emit(
        "class A { x: number = 1; y?: string; static s: number = 2; }",
        ES2022,
        None,
    );
    assert!(output.contains("x = 1;"), "{output}");
    assert!(output.contains("y;"), "{output}");
    assert!(output.contains("static s = 2;"), "{output}");
    assert!(!output.contains("constructor"), "{output}");
}

#[test]
fn assign_semantics_moves_initialized_fields_into_the_constructor() {
    let output = emit(
        "class A { x: number = 1; y?: string; w!: number; z: string = 'z'; }",
        ES2020,
        None,
    );
    // A field with no initializer emits nothing; the rest keep their order.
    assert!(
        output.contains("constructor() { this.x = 1; this.z = 'z'; }"),
        "{output}"
    );
    assert!(
        !output.contains("this.y") && !output.contains("this.w"),
        "{output}"
    );
}

#[test]
fn define_semantics_below_es2022_uses_object_define_property() {
    let output = emit("class A { x: number = 1; y?: string; }", ES2020, Some(true));
    assert!(
        output.contains(
            "Object.defineProperty(this, \"x\", { enumerable: true, configurable: true, writable: true, value: 1 });"
        ),
        "{output}"
    );
    assert!(
        output.contains(
            "Object.defineProperty(this, \"y\", { enumerable: true, configurable: true, writable: true, value: void 0 });"
        ),
        "an uninitialized field is still defined, as undefined: {output}"
    );
}

#[test]
fn es2022_with_assign_semantics_assigns_instance_fields_and_uses_static_blocks() {
    let output = emit(
        "class A { x: number = 1; static s: number = 2; static t: number; static { A.s = 3; } }",
        ES2022,
        Some(false),
    );
    assert!(output.contains("constructor() { this.x = 1; }"), "{output}");
    assert!(output.contains("static { this.s = 2; }"), "{output}");
    assert!(
        !output.contains("static t"),
        "an uninitialized static field emits nothing: {output}"
    );
    assert!(
        output.contains("static { A.s = 3; }"),
        "a static block stays native: {output}"
    );
}

#[test]
fn a_derived_class_assigns_after_super_and_a_missing_constructor_is_added() {
    let output = emit(
        "class B { constructor(public id: number) {} } \
         class D extends B { x: number = this.id; constructor() { super(1); this.x = 2; } } \
         class E extends B { y: number = 5; } \
         class F { z: number = 1; }",
        ES2020,
        None,
    );
    assert!(
        output.contains("super(1); this.x = this.id; this.x = 2;"),
        "{output}"
    );
    assert!(
        output.contains("class E extends B { constructor() { super(...arguments); this.y = 5; }"),
        "{output}"
    );
    assert!(
        output.contains("class F { constructor() { this.z = 1; }"),
        "{output}"
    );
    assert!(
        output.contains("constructor(id) { this.id = id;"),
        "{output}"
    );
}

#[test]
fn a_derived_constructor_without_a_top_level_super_is_refused_when_statements_must_follow_it() {
    let compiled = compile_with(
        "class B { constructor(public id: number) {} } \
         class D extends B { x: number = 1; constructor(f: boolean) { if (f) { super(1); } else { super(2); } } }",
        options(ES2020, None),
    );
    assert!(compiled.output.is_none());
    assert!(compiled.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::UnsupportedSyntax
            && diagnostic.message.contains("top-level `super(...)`")
    }));
}

#[test]
fn static_fields_and_blocks_run_after_the_class_below_es2022() {
    let output = emit(
        "class A { static a: number = 1; static { A.a = 2; } static b: number = A.a + 1; static c: number; }",
        ES2020,
        None,
    );
    assert!(
        output.contains("A.a = 1; (function () { A.a = 2; }).call(A); A.b = A.a + 1;"),
        "in source order, after the class: {output}"
    );
    assert!(!output.contains("static"), "{output}");
    assert!(
        !output.contains("A.c"),
        "an uninitialized static field emits nothing: {output}"
    );
    let defined = emit(
        "class A { static a: number = 1; static c: number; }",
        ES2020,
        Some(true),
    );
    assert!(
        defined.contains("Object.defineProperty(A, \"a\", { enumerable: true, configurable: true, writable: true, value: 1 });")
            && defined.contains("Object.defineProperty(A, \"c\", { enumerable: true, configurable: true, writable: true, value: void 0 });"),
        "{defined}"
    );
}

#[test]
fn a_static_initializer_that_mentions_this_is_evaluated_as_a_function_called_on_the_class() {
    let output = emit(
        "class A { static n: number = 1; static m: number = this.n + 1; static o: number = 5; }",
        ES2020,
        None,
    );
    assert!(
        output.contains("A.m = (function () { return this.n + 1; }).call(A);"),
        "{output}"
    );
    assert!(
        output.contains("A.o = 5;"),
        "an initializer without `this` is not wrapped: {output}"
    );
}

#[test]
fn a_static_initializer_or_block_using_super_cannot_be_lowered() {
    for source in [
        "class B { static s: number = 1; } class D extends B { static t: number = super.s; }",
        "class B { static s: number = 1; } class D extends B { static { const n: number = super.s; } }",
    ] {
        let compiled = compile_with(source, options(ES2020, None));
        assert!(compiled.output.is_none(), "`{source}`");
        assert!(
            compiled
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == DiagnosticCode::UnsupportedSyntax
                    && diagnostic.message.contains("uses `super`")),
            "`{source}`: {:?}",
            compiled.diagnostics
        );
        // The same source is fine where static blocks are native.
        assert!(compile_with(source, options(ES2022, None)).output.is_some());
    }
}

#[test]
fn moved_initializers_keep_their_erased_annotations() {
    let output = emit(
        "class A { f: (x: number) => number = (x: number): number => x + 1; }",
        ES2020,
        None,
    );
    assert!(
        output.contains("this.f = (x)=> x + 1;")
            || output.contains("this.f = (x) => x + 1;")
            || output.contains("this.f = (x)"),
        "{output}"
    );
    assert!(!output.contains(": number"), "{output}");
}

#[test]
fn private_names_below_es2022_use_weak_state_and_versioned_helpers() {
    let output = emit(
        "class A { #x: number = 1; #y?: string; static #s: number = 2; \
         #m(k: number): number { return this.#x * k; } \
         get #g(): number { return this.#x; } set #g(v: number) { this.#x = v; } \
         static #sm(): number { return A.#s; } \
         run(o: A): number { this.#x += 2; this.#x++; this.#g = 3; \
           return this.#m(2) + o.#x + this.#g + A.#sm() + (#x in o ? 1 : 0); } }",
        ES2020,
        None,
    );
    assert!(!output.contains('#'), "no private name is left: {output}");
    assert!(
        output.starts_with("/* bluets-class-helper-v1 */")
            || output.contains("/* bluets-class-helper-v1 */"),
        "{output}"
    );
    for expected in [
        "var _A_instances, _A_x, _A_y, _A_s, _A_m, _A_g_get, _A_g_set, _A_sm;",
        "_A_instances.add(this); _A_x.set(this, 1); _A_y.set(this, void 0);",
        "_A_x = new WeakMap(); _A_y = new WeakMap(); _A_instances = new WeakSet();",
        "_A_m = function _A_m(k)",
        "_A_g_get = function _A_g_get()",
        "_A_g_set = function _A_g_set(v)",
        "_A_sm = function _A_sm()",
        "_A_s = { value: 2 };",
        "__bluetsClassPrivateSet(this, _A_x, __bluetsClassPrivateGet(this, _A_x, \"f\") + (2), \"f\")",
        "__bluetsClassPrivateGet(this, _A_instances, \"m\", _A_m).call(this, 2)",
        "__bluetsClassPrivateSet(this, _A_instances, 3, \"a\", _A_g_set)",
        "__bluetsClassPrivateGet(this, _A_instances, \"a\", _A_g_get)",
        "__bluetsClassPrivateGet(A, A, \"m\", _A_sm).call(A)",
        "__bluetsClassPrivateIn(_A_x, o)",
    ] {
        assert!(output.contains(expected), "`{expected}` missing from {output}");
    }
    // Each helper is defined once.
    for helper in ["Get", "Set", "In"] {
        let definition = format!("var __bluetsClassPrivate{helper} =");
        assert_eq!(output.matches(&definition).count(), 1, "{helper}: {output}");
    }
}

#[test]
fn helpers_are_emitted_once_per_module_and_only_when_used() {
    let two = emit(
        "class A { #x: number = 1; get(): number { return this.#x; } } \
         class B { #y: number = 2; get(): number { return this.#y; } }",
        ES2020,
        None,
    );
    assert_eq!(
        two.matches("var __bluetsClassPrivateGet =").count(),
        1,
        "{two}"
    );
    assert!(
        !two.contains("__bluetsClassPrivateSet"),
        "unused helpers are not emitted: {two}"
    );
    let none = emit("class A { x: number = 1; }", ES2020, None);
    assert!(!none.contains("__bluets"), "{none}");
    // ES2022 keeps private names native, with or without assign semantics.
    let native = emit(
        "class A { #x: number = 1; get(): number { return this.#x; } }",
        ES2022,
        None,
    );
    assert!(
        native.contains("#x = 1;") && native.contains("this.#x"),
        "{native}"
    );
    let assign = emit(
        "class A { #x: number = 1; y: number = 2; }",
        ES2022,
        Some(false),
    );
    assert!(
        assign.contains("#x = 1;") && assign.contains("this.y = 2;"),
        "{assign}"
    );
}

#[test]
fn forms_the_token_rewrite_cannot_do_safely_are_refused_below_es2022() {
    for (source, what) in [
        (
            "class A { #x: number = 1; list: A[] = []; run(): number { return this.list[0].#x; } }",
            "accessed through anything but `this` or a plain identifier",
        ),
        (
            "class A { #x: number = 1; run(o: A): number { return o.self().#x; } self(): A { return this; } }",
            "accessed through anything but `this` or a plain identifier",
        ),
        (
            "class A { #x: number = 1; run(): number { let y: number = 0; y = this.#x++; return y; } }",
            "an increment or decrement used as a value",
        ),
        (
            "class A { #x: number = 1; run(): number { return ++this.#x; } }",
            "an increment or decrement used as a value",
        ),
        (
            "class A { #x: number = 1; run(): number { let y: number = 0; y = this.#x = 2; return y; } }",
            "an assignment used as a value",
        ),
        (
            "class A { #x: number | undefined; run(): void { this.#x ??= 1; } }",
            "a logical assignment",
        ),
        (
            "class A { #x: number = 1; run(o: A[]): boolean { return #x in o[0]; } }",
            "a brand check on a complex operand",
        ),
    ] {
        let compiled = compile_with(source, options(ES2020, None));
        assert!(compiled.output.is_none(), "`{source}` must not emit");
        assert!(
            compiled.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == DiagnosticCode::UnsupportedSyntax
                    && diagnostic.message.contains(what)
                    && diagnostic.message.contains("below ES2022")
            }),
            "`{source}`: {:?}",
            compiled.diagnostics
        );
        // The same program is fine where private names are native.
        assert!(compile_with(source, options(ES2022, None)).output.is_some(), "`{source}`");
    }
}

#[test]
fn a_name_the_lowering_needs_may_not_already_be_used() {
    for source in [
        "class A { #x: number = 1; get(): number { return this.#x; } } const _A_x = 1;",
        "class A { #x: number = 1; get(): number { return this.#x; } } const __bluetsClassPrivateGet = 1;",
    ] {
        let compiled = compile_with(source, options(ES2020, None));
        assert!(compiled.output.is_none(), "`{source}`");
        assert!(
            compiled.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == DiagnosticCode::UnsupportedSyntax
                    && (diagnostic.message.contains("already used")
                        || diagnostic.message.contains("reserved"))
            }),
            "`{source}`: {:?}",
            compiled.diagnostics
        );
    }
}

#[test]
fn a_redeclared_field_without_an_initializer_overwrites_the_base_only_with_define_semantics() {
    let source = "class B { x: number = 1; } class D extends B { x!: number; }";
    let define = diagnostics(source, options(ES2022, None));
    assert!(
        define
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch
                && diagnostic
                    .message
                    .contains("will overwrite the base property")),
        "{define:?}"
    );
    assert!(diagnostics(source, options(ES2020, None)).is_empty());
    assert!(diagnostics(source, options(ES2022, Some(false))).is_empty());
    assert!(!diagnostics(source, options(ES2020, Some(true))).is_empty());
    // An initializer, a constructor assignment, a static field and a private
    // name are all fine.
    for accepted in [
        "class B { x: number = 1; } class D extends B { x: number = 2; }",
        "class B { x: number = 1; } class D extends B { x: number; constructor() { super(); this.x = 2; } }",
        "class B { static x: number = 1; } class D extends B { static x: number; }",
        "class B { x: number = 1; } class D extends B { y!: number; }",
    ] {
        assert!(diagnostics(accepted, options(ES2022, None)).is_empty(), "`{accepted}`");
    }
    assert!(!diagnostics(
        "class B { constructor(public x: number) {} } class D extends B { x!: number; }",
        options(ES2022, None)
    )
    .is_empty());
}

#[test]
fn the_output_is_deterministic_and_the_fingerprint_binds_the_semantics() {
    let source = "class A { x: number = 1; }";
    let fingerprint = |target, define| {
        compile_with(source, options(target, define))
            .output
            .unwrap()
            .fingerprint
    };
    assert_eq!(fingerprint(ES2022, None), fingerprint(ES2022, Some(true)));
    assert_ne!(fingerprint(ES2022, None), fingerprint(ES2022, Some(false)));
    assert_ne!(fingerprint(ES2020, None), fingerprint(ES2020, Some(true)));
    assert_eq!(fingerprint(ES2020, None), fingerprint(ES2020, Some(false)));
    assert_eq!(
        emit(source, ES2020, None),
        emit(source, ES2020, Some(false)),
        "explicit false is the ES2020 default"
    );
}

#[test]
fn source_maps_and_declarations_are_unaffected_by_the_lowering() {
    let source = "export class A { x: number = 1; static s: string = 'a'; constructor(public p: number) {} }";
    let mut declarations = Vec::new();
    for (target, define) in [
        (ES2022, None),
        (ES2022, Some(false)),
        (ES2020, None),
        (ES2020, Some(true)),
    ] {
        let compiled = compile_with(
            source,
            CompilerOptions {
                source_map: true,
                declaration: true,
                ..options(target, define)
            },
        );
        assert!(
            compiled.diagnostics.is_empty(),
            "{:?}",
            compiled.diagnostics
        );
        let artifact = &compiled.output.unwrap().artifacts[ENTRY];
        assert!(
            !artifact.source_map.as_ref().unwrap().mappings.is_empty(),
            "{target:?} {define:?}"
        );
        declarations.push(artifact.declaration.clone().unwrap());
    }
    assert!(
        declarations.windows(2).all(|pair| pair[0] == pair[1]),
        "{declarations:?}"
    );
}
