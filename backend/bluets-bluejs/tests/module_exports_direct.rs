// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! K.6.1 default and re-export forms reach the closed direct module graph.

use blueice_bluejs::{Value, Vm};
use blueice_bluets::{CheckingOptions, CompilerOptions, MapLoader, ModuleSource};
use blueice_bluets_bluejs::compile_direct_module_graph;

fn options() -> CompilerOptions {
    CompilerOptions {
        checking: Some(CheckingOptions::default()),
        ..CompilerOptions::default()
    }
}

fn run<const N: usize>(sources: [(&str, &str); N]) -> Value {
    let loader = MapLoader::from(sources.map(|(name, source)| ModuleSource::new(name, source)));
    let graph = compile_direct_module_graph("graph/main.ts", &loader, options()).unwrap();
    Vm::default()
        .execute_module_graph(&graph.entry, &graph.bytecode_map())
        .unwrap()
}

#[test]
fn default_expressions_and_anonymous_declarations_reach_the_vm() {
    for (main, base) in [
        (
            "import value from './base.ts'; value;",
            "export default 40 + 2;",
        ),
        (
            "import fn from './base.ts'; fn(41);",
            "export default function(value: number): number { return value + 1; }",
        ),
        (
            "import Box from './base.ts'; new Box(42).value;",
            "export default class { constructor(public value: number) {} }",
        ),
    ] {
        assert_eq!(
            run([("graph/main.ts", main), ("graph/base.ts", base)]),
            Value::Number(42.0)
        );
    }
}

#[test]
fn named_and_star_reexports_keep_live_bindings() {
    for barrel in [
        "export {value, inc} from './base.ts';",
        "export * from './base.ts';",
    ] {
        assert_eq!(
            run([
                (
                    "graph/main.ts",
                    "import {value, inc} from './barrel.ts'; inc(); value;"
                ),
                ("graph/barrel.ts", barrel),
                (
                    "graph/base.ts",
                    "export let value: number = 1; export function inc(): void { value += 1; }"
                ),
            ]),
            Value::Number(2.0)
        );
    }
}

#[test]
fn namespace_reexports_keep_live_values_and_canonical_targets() {
    assert_eq!(
        run([
            (
                "graph/main.ts",
                "import {box} from './barrel.ts'; box.inc(); box.value;"
            ),
            ("graph/barrel.ts", "export * as box from './base.ts';"),
            (
                "graph/base.ts",
                "export let value: number = 1; export function inc(): void { value += 1; }"
            ),
        ]),
        Value::Number(2.0)
    );
}

#[test]
fn declaration_only_reexports_do_not_acquire_runtime_authority() {
    let loader = MapLoader::from([
        ModuleSource::new("graph/main.ts", "export {value} from './types.d.ts';"),
        ModuleSource::new("graph/types.d.ts", "export declare const value: number;"),
    ]);
    assert!(compile_direct_module_graph("graph/main.ts", &loader, options()).is_err());
}
