// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Mixed inferred unions retain the Native diagnostic-before-emit allocation order.
use blueice_bluets::{
    compile, CheckingOptions, CompilerOptions, EcmaTarget, MapLoader, ModuleSource,
};

#[test]
fn earlier_array_allocation_precedes_a_later_regular_literal() {
    let identity = "export function identity<T extends number>(value: T): T { return value; }\n";
    for (body, expected) in [
        ("export const values: string[] = []; export const result = [identity(42), values];", "string[] | 42"),
        ("const literal = identity(42); export const values: string[] = []; export const result = [literal, values];", "42 | string[]"),
        ("declare const literal: 42; export const values: string[] = []; export const result = [literal, values];", "42 | string[]"),
        ("export const values: string[] = []; declare const literal: 42; export const result = [literal, values];", "string[] | 42"),
        ("export const values: string[] = []; export const result = [identity(0), values];", "0 | string[]"),
    ] {
        let source = format!("{identity}{body}");
        let result = compile("main.ts", &MapLoader::from([ModuleSource::new("main.ts", source)]), CompilerOptions {
            target: EcmaTarget::Es2022, libraries: Some(vec![EcmaTarget::Es2022]),
            declaration: true, checking: Some(CheckingOptions::default()),
            ..CompilerOptions::default()
        });
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        let declaration = result.output.unwrap().artifacts["main.ts"].declaration.clone().unwrap();
        assert!(declaration.contains(&format!("export declare const result: ({expected})[];")), "{declaration}");
    }
}
