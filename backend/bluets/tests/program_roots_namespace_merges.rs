// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Merged globals retain their value shape when another root consumes them.

use blueice_bluets::{compile_program, CompilerOptions, EcmaTarget, MapLoader, ModuleSource};

fn check(provider: &str, consumer: &str, target: EcmaTarget) -> blueice_bluets::Compilation {
    let roots = vec!["program:///main.ts".into(), "program:///consumer.ts".into()];
    compile_program(
        &roots,
        &MapLoader::from([
            ModuleSource::new(&roots[0], provider),
            ModuleSource::new(&roots[1], consumer),
        ]),
        CompilerOptions {
            target,
            declaration: true,
            ..CompilerOptions::default()
        },
    )
}

#[test]
fn function_namespace_static_members_survive_cross_root_binding() {
    for target in [EcmaTarget::Es5, EcmaTarget::Es2022] {
        let result = check(
            "function build(value: number): number { return value; } namespace build { export const version = 1; }",
            "const check: number = build(1); const version: number = build.version;",
            target,
        );
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
    }
}

#[test]
fn class_namespace_static_members_preserve_the_constructor() {
    for target in [EcmaTarget::Es5, EcmaTarget::Es2022] {
        let result = check(
            "class Box { value: number = 1; static own: string = 'x'; } namespace Box { export const version = 1; }",
            "const check: number = new Box().value; const version: number = Box.version; const own: string = Box.own;",
            target,
        );
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
    }
}

#[test]
fn merged_function_aliases_keep_generic_calls_and_static_property_types() {
    let provider = "function build<T>(value: T): T { return value; } namespace build { export const version: number = 1; }";
    let result = check(
        provider,
        "const alias = build; const check: string = alias('x'); const version: number = alias.version;",
        EcmaTarget::Es2022,
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    for consumer in [
        "const version: string = build.version;",
        "const missing = build.missing;",
    ] {
        let rejected = check(provider, consumer, EcmaTarget::Es2022);
        assert!(rejected.has_errors(), "accepted {consumer}");
    }
}
