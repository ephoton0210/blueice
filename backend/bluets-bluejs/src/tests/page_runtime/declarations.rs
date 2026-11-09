// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Declarations page-realm regressions.

use super::*;

#[test]
fn direct_routes_refuse_what_an_enum_lowering_cannot_inline() {
    let loader = MapLoader::from([ModuleSource::new(
        ENTRY,
        "declare const enum E { A, B } E.B;",
    )]);
    assert!(matches!(
        compile_direct_script(ENTRY, &loader, CompilerOptions::default()),
        Err(BridgeError::UnsupportedRuntimeTarget { .. })
    ));
    // An exported enum needs the module bridge.
    let exported = MapLoader::from([ModuleSource::new(ENTRY, "export enum E { A } E.A;")]);
    assert!(matches!(
        compile_direct_script(ENTRY, &exported, CompilerOptions::default()),
        Err(BridgeError::UnsupportedRuntimeTarget { .. })
    ));
}

#[test]
fn direct_page_enums_build_reverse_maps_merge_and_run_like_node() {
    let source = "function seed(): number { return 7; } \
        enum Color { Red, Green = 5, Blue } \
        enum M { A = 1 } enum M { B = 2 } \
        enum Comp { A = seed(), B = Comp.A * 2 } \
        enum S { X = 'x', Y = 'yy' } \
        Color.Blue + Color[5].length + M.A * 100 + M.B * 1000 + Comp.B + S.Y.length \
            + Object.keys(M).length * 10000;";
    // Node runs the same program (the enum IIFEs tsc prints) to 42127.
    assert_eq!(
        run_direct_page_script(source),
        bluejs::Value::Number(42127.0)
    );
}

#[test]
fn direct_page_const_enums_read_their_object_and_ambient_enums_are_erased() {
    let source = "const enum K { A = 1, B = A << 2 } \
        declare enum Host { Q } \
        K.B + K.A;";
    assert_eq!(run_direct_page_script(source), bluejs::Value::Number(5.0));
}

#[test]
fn direct_page_namespaces_merge_export_and_read_exported_variables_like_node() {
    // Node runs the JavaScript TypeScript emits for this program to
    // 9 + 3 + 1 + 10 + 101 = 124.
    let source = "namespace N { \
        export const a: number = 1; export let b: number = a + 1; const hidden: number = 5; \
        export function f(): number { return a + b + hidden; } \
        export class C { v: number = b; } export enum E { X, Y } \
        export namespace Inner { export const z: number = a * 10; } \
        b++; } \
        const f: number = N.f(); const v: number = new N.C().v; \
        namespace N { export const extra: number = N.a + 100; } \
        f + v + N.E.Y + N.Inner.z + N.extra;";
    assert_eq!(run_direct_page_script(source), bluejs::Value::Number(124.0));
}

#[test]
fn direct_page_namespace_variables_are_properties_seen_through_closures_and_shorthand() {
    // R.inc() twice, then R.pair().c is 2 and R.c is 2; 2 * 10 + 2 = 22.
    let source = "namespace R { export let c: number = 0; \
        export function inc(): number { c += 1; return c; } \
        export function pair(): { c: number } { return { c }; } } \
        R.inc(); R.inc(); R.pair().c * 10 + R.c;";
    assert_eq!(run_direct_page_script(source), bluejs::Value::Number(22.0));
}

#[test]
fn direct_page_namespaces_merge_into_a_function_and_a_class_without_a_second_variable() {
    // g() is 1, g.meta is 5, K.s is 2, K.t is 3: 1 + 5 + 2 + 3 = 11.
    let source =
        "function g(): number { return 1; } namespace g { export const meta: number = 5; } \
        class K { static s: number = 2; } namespace K { export const t: number = 3; } \
        g() + g.meta + K.s + K.t;";
    assert_eq!(run_direct_page_script(source), bluejs::Value::Number(11.0));
}

#[test]
fn direct_page_dotted_and_reopened_namespaces_nest_and_reach_earlier_blocks() {
    // A.B.x is 1, A.B.y is 2, A.z is 3: 1 + 2 + 3 = 6.
    let source = "namespace A.B { export const x: number = 1; } \
        namespace A.B { export const y: number = x + 1; } \
        namespace A { export const z: number = B.x + B.y; } \
        A.B.x + A.B.y + A.z;";
    assert_eq!(run_direct_page_script(source), bluejs::Value::Number(6.0));
}

#[test]
fn direct_routes_refuse_what_a_namespace_lowering_cannot_do_safely() {
    for source in [
        // A parameter that shadows an exported variable.
        "namespace N { export const a: number = 1; export function f(a: number): number { return a; } }",
        // An exported namespace needs the module bridge.
        "export namespace N { export const a: number = 1; }",
        // A template literal that reads an exported variable.
        "namespace N { export let a: number = 1; export function f(): string { return `${a}`; } }",
    ] {
        assert!(
            matches!(
                compile_direct_script(
                    ENTRY,
                    &MapLoader::from([ModuleSource::new(ENTRY, source)]),
                    CompilerOptions::default()
                ),
                // Either BlueTSC itself refuses it, or the bridge does.
                Err(BridgeError::UnsupportedRuntimeTarget { .. } | BridgeError::BlueTs(_))
            ),
            "{source}"
        );
    }
}

#[test]
fn direct_page_async_functions_are_async_and_return_promises() {
    // An async function returns a promise, so `typeof` its result is "object";
    // it was once lowered as a plain function, whose result is the number.
    let source = "async function f(): Promise<number> { return 1; } \
        const p = f(); const t: string = typeof p; t === 'object' ? 1 : 0;";
    assert_eq!(run_direct_page_script(source), bluejs::Value::Number(1.0));
}

#[test]
fn a_top_level_await_in_a_script_is_refused_before_lowering() {
    let result = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "async function f(): Promise<number> { return 1; } const a: number = await f();",
        )]),
        CompilerOptions::default(),
    );
    assert!(matches!(result, Err(BridgeError::BlueTs(_))));
}

#[test]
fn direct_page_generators_yield_receive_and_delegate_like_node() {
    // Node runs the same generators to 3210, 1115 and 151.
    let counting = "function* count(n: number): Generator<number, number> { \
        let i = 0; while (i < n) { yield i; i++; } return n; } \
        const it = count(3); \
        const a: number = it.next().value; const b: number = it.next().value; \
        const c: number = it.next().value; const d: number = it.next().value; \
        a + b * 10 + c * 100 + d * 1000;";
    assert_eq!(
        run_direct_page_script(counting),
        bluejs::Value::Number(3210.0)
    );
    let echoing = "function* echo(): Generator<number, number, number> { \
        const x: number = yield 1; const y: number = yield x + 1; return x + y; } \
        const e = echo(); e.next(); \
        const p: number = e.next(10).value; const q: number = e.next(5).value; \
        p * 100 + q;";
    assert_eq!(
        run_direct_page_script(echoing),
        bluejs::Value::Number(1115.0)
    );
    let delegating = "function* inner(): Generator<number, number> { yield 1; return 5; } \
        function* outer(): Generator<number, number> { const r: number = yield* inner(); yield r; return 0; } \
        const o = outer(); \
        const first: number = o.next().value; const second: number = o.next().value; \
        const done = o.next().done; \
        first * 100 + second * 10 + (done ? 1 : 0);";
    assert_eq!(
        run_direct_page_script(delegating),
        bluejs::Value::Number(151.0)
    );
}
