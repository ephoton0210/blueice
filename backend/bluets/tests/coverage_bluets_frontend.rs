// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Table-driven coverage for the BlueTS parser, checker and emitter through
//! the public `compile` entry point: the diagnostic code and wording for each
//! rejected construct, and the erased JavaScript for accepted ones.

use blueice_bluets::{compile, CompilerOptions, Diagnostic, MapLoader, ModuleSource};

const ENTRY: &str = "memory:///main.ts";
const HELPER: &str = "export const a: number = 1;\nexport const b: number = 2;\nexport interface Shape { x: number }\nexport type Box<T> = { value: T };\nexport type Id = number | string;\n";

fn compile_with_helper(source: &str) -> blueice_bluets::Compilation {
    let loader = MapLoader::from([
        ModuleSource::new(ENTRY, source),
        ModuleSource::new("memory:///a.ts", HELPER),
    ]);
    compile(ENTRY, &loader, CompilerOptions::default())
}

fn diagnostics(source: &str) -> Vec<Diagnostic> {
    compile_with_helper(source).diagnostics
}

#[track_caller]
fn assert_rejected(source: &str, code: &str, message: &str) {
    let found = diagnostics(source);
    let Some(first) = found.first() else {
        panic!("`{source}` should be rejected with {code}: {message}");
    };
    assert_eq!(first.code.to_string(), code, "`{source}`: {found:?}");
    assert!(
        first.message.contains(message),
        "`{source}`: expected `{message}` in `{}`",
        first.message
    );
    assert!(first.span.start <= first.span.end, "`{source}`");
    assert_eq!(first.span.module, ENTRY, "`{source}`");
}

#[track_caller]
fn assert_accepted(source: &str) {
    let found = diagnostics(source);
    assert!(found.is_empty(), "`{source}` should compile: {found:?}");
}

fn emitted(source: &str) -> String {
    let result = compile_with_helper(source);
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    result.output.unwrap().artifacts[ENTRY].javascript.clone()
}

#[test]
fn unsupported_and_misplaced_syntax_is_diagnosed_not_passed_through() {
    for (source, message) in [
        (
            "export default 1;",
            "default export expressions are not in the initial BlueTS matrix",
        ),
        (
            "export default function () { return 1; }",
            "anonymous default function exports are not in the initial BlueTS matrix",
        ),
        (
            "export = foo;",
            "`export =` is not in the initial BlueTS matrix",
        ),
        (
            "<div/>;",
            "decorators and TSX/JSX are not in the initial BlueTS matrix",
        ),
        (
            "@dec class A {}",
            "decorators and TSX/JSX are not in the initial BlueTS matrix",
        ),
        ("class A {}", "`class` is not in the initial BlueTS matrix"),
        (
            "abstract class A {}",
            "`abstract` declarations are not in the initial BlueTS matrix",
        ),
        (
            "declare abstract class A {}",
            "`abstract` is not in the initial BlueTS matrix",
        ),
        (
            "function make() { abstract class A {} }",
            "`abstract` declarations are not in the initial BlueTS matrix",
        ),
        (
            "if (true) { abstract class A {} }",
            "`abstract` declarations are not in the initial BlueTS matrix",
        ),
        (
            "const make = () => { abstract class A {} };",
            "`abstract` declarations are not in the initial BlueTS matrix",
        ),
        (
            "function make() { class Local {} }",
            "`class` is not in the initial BlueTS matrix",
        ),
        (
            "if (true) { enum State { Ready } }",
            "`enum` is not in the initial BlueTS matrix",
        ),
        (
            "const make = () => { namespace Internal {} };",
            "`namespace` is not in the initial BlueTS matrix",
        ),
        (
            "const make = () => { module Internal {} };",
            "`module` is not in the initial BlueTS matrix",
        ),
        (
            "const decorated = () => { @sealed class A {} };",
            "decorators and TSX/JSX are not in the initial BlueTS matrix",
        ),
        (
            "const enum State { Ready }",
            "`enum` is not in the initial BlueTS matrix",
        ),
        ("enum E { A }", "`enum` is not in the initial BlueTS matrix"),
        (
            "namespace N {}",
            "`namespace` is not in the initial BlueTS matrix",
        ),
        (
            "const x?: number = 1;",
            "optional variables are not valid TypeScript declarations",
        ),
        (
            "const x = (a: number) => a;",
            "typed arrow parameters are not in the initial BlueTS matrix",
        ),
        (
            "const identity = <T>(value) => value;",
            "generic arrow functions are not in the initial BlueTS matrix",
        ),
        (
            "function make() { return <T>(value) => value; }",
            "generic arrow functions are not in the initial BlueTS matrix",
        ),
        (
            "function make() { let identity; identity = <T>(value) => value; }",
            "generic arrow functions are not in the initial BlueTS matrix",
        ),
        (
            "a as number;",
            "TypeScript assertions outside a supported declaration",
        ),
        (
            "const x = 1; x satisfies number;",
            "TypeScript assertions outside a supported declaration",
        ),
        (
            "import { type A, b } from './a.ts';",
            "mixed value/type imports are not in the initial BlueTS matrix",
        ),
        (
            "export { a } from './a.ts';",
            "value re-exports from another module are not in the initial BlueTS matrix",
        ),
        (
            "type A = number; interface B extends A { x: string }",
            "interface heritage A must name an interface declaration",
        ),
        (
            "interface B<T> extends T { x: string }",
            "interface heritage T must name an interface declaration",
        ),
    ] {
        assert_rejected(source, "BTS1001", message);
    }
}

#[test]
fn syntax_errors_carry_a_precise_expectation() {
    for (source, message) in [
        ("export import a from 'x';", "an import cannot be exported"),
        (
            "declare foo;",
            "`declare` must introduce a supported declaration",
        ),
        (
            "declare interface A { x: number }",
            "`declare` must introduce a supported declaration",
        ),
        (
            "declare type A = number;",
            "`declare` must introduce a supported declaration",
        ),
        (
            "async foo;",
            "`async` must precede a function declaration in the initial matrix",
        ),
        (
            "import { type } from './a.ts';",
            "expected an imported binding",
        ),
        (
            "import { , } from './a.ts';",
            "expected an imported binding",
        ),
        (
            "import { a as } from './a.ts';",
            "expected a local import name",
        ),
        ("import { a b } from './a.ts';", "expected `}`"),
        ("import * from './a.ts';", "expected `as`"),
        (
            "import 1 from './a.ts';",
            "expected an import clause or module specifier",
        ),
        ("import", "expected an import clause or module specifier"),
        ("import x", "expected `from`"),
        ("import { a } from 1;", "expected a string module specifier"),
        (
            "export type { A } from 1;",
            "expected a string module specifier",
        ),
        (
            "export type { A as } from './a.ts';",
            "expected an exported type name",
        ),
        ("export type { A B } from './a.ts';", "expected `}`"),
        ("const x: = 1;", "expected a type"),
        ("const x: number[] | = 1;", "expected a type"),
        ("const x: [number,, string] = [1, 'a'];", "expected a type"),
        ("const x: Array<number = [];", "expected `>`"),
        ("const x: { a number } = 1;", "expected `:`"),
        ("const x: { : number } = 1;", "expected a record field name"),
        ("interface A { a }", "expected `:`"),
        ("interface A { a: number", "expected `}`"),
        ("interface { a: number }", "expected an interface name"),
        ("interface A<T extends> { a: T }", "expected a type"),
        ("interface A<T = > { a: T }", "expected a type"),
        ("interface A<T { a: T }", "expected `>`"),
        ("type = number;", "expected a type alias name"),
        ("type A<T> = ;", "expected a type"),
        ("type A = ", "expected a type"),
        (
            "function f({ a }: { a: number }): number { return a; }",
            "expected a parameter name",
        ),
        ("function () {}", "expected a function name"),
        ("function f(: number) {}", "expected a parameter name"),
        ("function f(a: number", "expected `)`"),
        (
            "function f(a: number) number { return 1; }",
            "expected a function body",
        ),
        ("function f(a: number): number", "expected a function body"),
        (
            "function f(a: number): number { return 1;",
            "unterminated function body",
        ),
        ("declare function f(a: number): number", "expected `;`"),
        ("const { a } = obj;", "expected a variable name"),
        ("const [a] = arr;", "expected a variable name"),
        (
            "function f<T>(a: T): T { return a; } const x = f<number string>(1);",
            "expected a comma between type arguments",
        ),
        ("const s = 'unterminated;", "unterminated string literal"),
        ("const c = /* unterminated", "unterminated block comment"),
    ] {
        assert_rejected(source, "BTS1000", message);
    }
}

#[test]
fn checker_diagnostics_name_the_offending_types() {
    let cases: &[(&str, &str, &str)] = &[
        // Names, arity and duplicates.
        ("interface B extends Missing { x: string }", "BTS3002", "cannot find type `Missing`"),
        ("const x: Missing<number> = 1;", "BTS3002", "cannot find type `Missing`"),
        ("function f<T extends Missing>(a: T): T { return a; }", "BTS3002", "cannot find type `Missing`"),
        ("function f<T = Missing>(a: T): T { return a; }", "BTS3002", "cannot find type `Missing`"),
        (
            "interface Box<T> { v: T } const b: Box = { v: 1 };",
            "BTS3003",
            "type `Box` requires 1 to 1 type argument(s), got 0",
        ),
        (
            "interface Box<T> { v: T } const b: Box<number, string> = { v: 1 };",
            "BTS3003",
            "type `Box` requires 1 to 1 type argument(s), got 2",
        ),
        ("const a = 1; const a = 2;", "BTS3000", "duplicate declaration of `a`"),
        (
            "function f(a: number): number { return a; } function f(a: number): number { return a; }",
            "BTS3000",
            "duplicate declaration of `f`",
        ),
        ("function f<T, T>(a: T): T { return a; }", "BTS3000", "duplicate type parameter `T`"),
        (
            "function f<T = string, U>(a: T): T { return a; }",
            "BTS3003",
            "required type parameter `U` cannot follow a defaulted type parameter",
        ),
        (
            "function f<T extends number = string>(a: T): T { return a; }",
            "BTS3003",
            "default type `string` does not satisfy constraint `number` for `T`",
        ),
        // Overloads.
        (
            "function f(a: number): number { return a; } function f(a: string): string;",
            "BTS3003",
            "overload signature for f must precede its implementation",
        ),
        (
            "function f(a: number): number;",
            "BTS3003",
            "overload signature for f requires an implementation",
        ),
        // Interface heritage compatibility.
        (
            "interface P { a: number } interface C extends P { a: string }",
            "BTS3003",
            "property `a` is not compatible with the inherited type `number`",
        ),
        (
            "interface P { a: number } interface C extends P { a?: number }",
            "BTS3003",
            "property `a` is not compatible with the inherited type `number`",
        ),
        (
            "interface P<T> { a: T } interface C extends P<number> { a: string }",
            "BTS3003",
            "property `a` is not compatible with the inherited type `number`",
        ),
        // Assignability of initializers and returns.
        (
            "const x: number = 1; const y: string = x;",
            "BTS3003",
            "initializer has type `number`, which is not assignable to `string`",
        ),
        (
            "function f(a: number) { const b: string = a; return b; }",
            "BTS3003",
            "initializer has type `number`, which is not assignable to `string`",
        ),
        (
            "function f(a: number): number { return a; } const x: string = f(1);",
            "BTS3003",
            "initializer has type `number`, which is not assignable to `string`",
        ),
        (
            "declare const x: number; const y: string = x;",
            "BTS3003",
            "initializer has type `number`, which is not assignable to `string`",
        ),
        (
            "declare function f(a: number): number; const y: string = f(1);",
            "BTS3003",
            "initializer has type `number`, which is not assignable to `string`",
        ),
        (
            "type T = { x: number }; const t: T = { x: 1 }; const n: string = t.x;",
            "BTS3003",
            "initializer has type `number`, which is not assignable to `string`",
        ),
        (
            "const a: number | string = true;",
            "BTS3003",
            "initializer has type `boolean`, which is not assignable to `number | string`",
        ),
        (
            "const a: 'x' | 'y' = 'z';",
            "BTS3003",
            "initializer has type `string`, which is not assignable to `'x' | 'y'`",
        ),
        (
            "const a: number[] = [1, 'x'];",
            "BTS3003",
            "which is not assignable to `number[]`",
        ),
        (
            "const a: [number, string] = [1];",
            "BTS3003",
            "which is not assignable to `[number, string]`",
        ),
        (
            "const a: [number, string] = [1, 'x', 3];",
            "BTS3003",
            "which is not assignable to `[number, string]`",
        ),
        (
            "interface A { x: number } const a: A = { x: 'no' };",
            "BTS3003",
            "which is not assignable to `A`",
        ),
        (
            "interface A { x: number } const a: A = {};",
            "BTS3003",
            "which is not assignable to `A`",
        ),
        (
            "const a: { x: number } & { y: number } = { x: 1 };",
            "BTS3003",
            "which is not assignable to",
        ),
        (
            "interface A { x: number } const a: A = { x: 1 }; const b: number = a.y;",
            "BTS3003",
            "property `y` does not exist on type `A`",
        ),
        // Type re-exports.
        ("export type { A };", "BTS3002", "cannot re-export unknown type `A`"),
        (
            "export type { A } from './a.ts';",
            "BTS3002",
            "cannot re-export unknown type `A`",
        ),
    ];
    for (source, code, message) in cases {
        assert_rejected(source, code, message);
    }
}

#[test]
fn supported_programs_are_accepted() {
    for source in [
        "const x: readonly number[] = [1];",
        "const x: { a: number, b?: string; readonly c: boolean } = { a: 1, c: true };",
        "interface A { readonly a: number; b?: string, c: number }",
        "interface A<T,> { a: T }",
        "interface P { a?: number } interface C extends P { a: number }",
        "function f(a: number = 1, b?: string): number { return a; }",
        "function f(a: number, ...rest: number[]): number { return a; }",
        "declare function f(a: number): number;",
        "declare const x: number;",
        "const x: number = 1 as number;",
        "function f<T>(a: T): T { return a; } const x = f<number>(1);",
        "function f<T>(a: T): T { return a; } const x = f<number,>(1);",
        "const x = y!;",
        "const x = y!.z;",
        "const keywords = { class: 1, enum: 2, namespace: 3, module: 4 };",
        "interface Box<T> { value: T } const box: Box<Box<number>> = { value: { value: 1 } };",
        "let a = 1, b = 2;",
        "function f() { const x: number = 1; let y: string = 'a'; var z = 3; return x; }",
        "function f() { if (true) { return 1; } return 2; }",
        "function f() { return; }",
        "export function f(): number { return 1; }",
        "const a = 1; export { a };",
        "export type X = number; export interface Y {}",
        "type A = number; export type { A }; export type { A as B };",
        "import type { Shape } from './a.ts'; const s: Shape = { x: 1 };",
        "import type { Shape as S, Id } from './a.ts'; const s: S = { x: 1 }; const i: Id = 1;",
        "import type * as N from './a.ts'; const s: N.Shape = { x: 1 };",
        "import type * as N from './a.ts'; const box: N.Box<N.Box<number>> = { value: { value: 1 } };",
        "import './a.ts';",
        "import { a, b as c } from './a.ts'; export const total: number = a;",
        "import * as ns from './a.ts';",
        "export type { Shape } from './a.ts';",
        "export type { Shape as Renamed, Id } from './a.ts';",
        "export type * from './a.ts';",
        "export type {} from './a.ts';",
        "const template = `t ${1}`; const re = /re/g; const big = 1n; const hex = 0x1F + 1e3 + .5;",
        "const text = \"a\\\"b\";",
        "function g(a: number): number; function g(a: string): string; function g(a: number | string): number | string { return a; }",
    ] {
        assert_accepted(source);
    }
}

#[test]
fn type_syntax_is_erased_from_the_emitted_javascript() {
    let source = "\
const ro: readonly number[] = [1];
interface A<T,> { readonly a: T; b?: string, c: number }
function withDefault(a: number = 1, b?: string): number { return a; }
function rest(a: number, ...others: number[]): number { return a; }
declare function external(a: number): number;
declare const externalValue: number;
function id<T>(a: T): T { return a; }
const called = id<number>(1);
const nonNull = called!;
const asserted = called as number;
function locals() { const x: number = 1; let y: string = 'a'; var z = 3; return x as number; }
type Alias = number | string;
export type { Alias };
export { nonNull };
";
    let javascript = emitted(source);
    for erased in [
        ": number",
        ": string",
        "interface",
        "readonly",
        "declare",
        "external",
        "<number>",
        "<T>",
        "as number",
        "type Alias",
        "b?",
        "!;",
    ] {
        assert!(
            !javascript.contains(erased),
            "`{erased}` must be erased from:\n{javascript}"
        );
    }
    for kept in [
        "const ro",
        "function withDefault(a",
        "= 1, b)",
        "function rest(a, ...others)",
        "function id(a)",
        "const called = id(1);",
        "const nonNull = called;",
        "const asserted = called",
        "function locals()",
        "export { nonNull };",
    ] {
        assert!(
            javascript.contains(kept),
            "`{kept}` must survive in:\n{javascript}"
        );
    }
}

#[test]
fn named_default_function_exports_preserve_esm_and_emit_a_public_declaration() {
    let source = "export default function greeting(name: string): string { return `Hello, ${name}`; }\nconsole.log(greeting('Ada'));\n";
    let compilation = compile_with_helper(source);
    assert!(
        compilation.diagnostics.is_empty(),
        "{:#?}",
        compilation.diagnostics
    );
    let javascript = &compilation.output.unwrap().artifacts[ENTRY].javascript;
    assert!(javascript.contains("export default function greeting(name)"));
    assert!(!javascript.contains(": string"));

    let output = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions {
            declaration: true,
            ..CompilerOptions::default()
        },
    )
    .output
    .unwrap();
    let declaration = output.artifacts[ENTRY].declaration.as_deref().unwrap();
    assert_eq!(
        declaration,
        "export default function greeting(name: string): string;\n"
    );
}

#[test]
fn named_default_value_exports_preserve_esm_and_emit_a_public_declaration() {
    let source = "const greeting: string = 'Hello, Ada';\nexport default greeting;\nconsole.log(greeting);\n";
    let compilation = compile_with_helper(source);
    assert!(
        compilation.diagnostics.is_empty(),
        "{:#?}",
        compilation.diagnostics
    );
    let javascript = &compilation.output.unwrap().artifacts[ENTRY].javascript;
    assert!(javascript.contains("const greeting"));
    assert!(javascript.contains("'Hello, Ada'"));
    assert!(javascript.contains("export default greeting"));
    assert!(!javascript.contains(": string"));

    let output = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions {
            declaration: true,
            ..CompilerOptions::default()
        },
    )
    .output
    .unwrap();
    let declaration = output.artifacts[ENTRY].declaration.as_deref().unwrap();
    assert_eq!(
        declaration,
        "declare const greeting: string;\nexport default greeting;\n"
    );
}

#[test]
fn named_default_value_exports_require_a_local_runtime_declaration() {
    assert_rejected(
        "export default missing;",
        "BTS3001",
        "default export `missing` must name a local runtime declaration",
    );
    assert_rejected(
        "declare const ambient: string;\nexport default ambient;",
        "BTS3001",
        "default export `ambient` must name a local runtime declaration",
    );
}

#[test]
fn named_value_exports_preserve_esm_and_emit_a_public_declaration() {
    let source =
        "const label: string = 'Hello, Ada';\nexport { label as greeting };\nconsole.log(label);\n";
    let compilation = compile_with_helper(source);
    assert!(
        compilation.diagnostics.is_empty(),
        "{:#?}",
        compilation.diagnostics
    );
    let javascript = &compilation.output.unwrap().artifacts[ENTRY].javascript;
    assert!(javascript.contains("const label"));
    assert!(javascript.contains("export { label as greeting }"));
    assert!(!javascript.contains(": string"));

    let output = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions {
            declaration: true,
            ..CompilerOptions::default()
        },
    )
    .output
    .unwrap();
    let declaration = output.artifacts[ENTRY].declaration.as_deref().unwrap();
    assert_eq!(
        declaration,
        "declare const label: string;\nexport { label as greeting };\n"
    );
}

#[test]
fn named_value_exports_require_a_local_runtime_declaration() {
    assert_rejected(
        "export { missing };",
        "BTS3001",
        "exported value `missing` must name a local runtime declaration",
    );
}

#[test]
fn imports_and_reexports_link_modules_and_keep_value_imports() {
    let javascript = emitted(
        "import { a, b as renamed } from './a.ts';\nimport type { Shape } from './a.ts';\nconst s: Shape = { x: a };\nexport const total: number = a + renamed;\n",
    );
    assert!(
        javascript.contains("import { a, b as renamed } from './a.js'"),
        "{javascript}"
    );
    assert!(!javascript.contains("Shape"), "{javascript}");
    assert!(javascript.contains("export const total"), "{javascript}");

    // A type-only import leaves no runtime import behind.
    let javascript = emitted("import type { Shape } from './a.ts';\nconst s: Shape = { x: 1 };\n");
    assert!(!javascript.contains("import"), "{javascript}");
}

#[test]
fn compiled_output_is_deterministic_and_tracks_the_source() {
    let first = compile_with_helper("export const value: number = 1;\n");
    let again = compile_with_helper("export const value: number = 1;\n");
    let changed = compile_with_helper("export const value: number = 2;\n");
    let fingerprint = |compilation: &blueice_bluets::Compilation| {
        compilation.output.as_ref().unwrap().fingerprint.clone()
    };
    assert_eq!(fingerprint(&first), fingerprint(&again));
    assert_ne!(fingerprint(&first), fingerprint(&changed));
}
