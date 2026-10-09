// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! K.6.2 type-only graph edges never create runtime module requests.

use blueice_bluejs::{Value, Vm};
use blueice_bluets::{CheckingOptions, CompilerOptions, MapLoader, ModuleSource};
use blueice_bluets_bluejs::compile_direct_module_graph;

fn options() -> CompilerOptions {
    CompilerOptions {
        checking: Some(CheckingOptions::default()),
        ..CompilerOptions::default()
    }
}

#[test]
fn type_only_import_of_a_declaration_keeps_the_runtime_graph_closed() {
    let loader = MapLoader::from([
        ModuleSource::new(
            "graph/main.ts",
            "import type {Item} from './types.d.ts'; const item: Item = {value:42}; item.value;",
        ),
        ModuleSource::new("graph/types.d.ts", "export interface Item {value:number;}"),
    ]);
    let graph = compile_direct_module_graph("graph/main.ts", &loader, options()).unwrap();
    assert_eq!(graph.modules.len(), 1);
    assert!(graph.modules.contains_key("graph/main.ts"));
    assert_eq!(
        Vm::default()
            .execute_module_graph(&graph.entry, &graph.bytecode_map())
            .unwrap(),
        Value::Number(42.0)
    );
}

#[test]
fn mixed_import_and_export_link_only_the_value_bindings() {
    let loader = MapLoader::from([
        ModuleSource::new(
            "graph/main.ts",
            "import {type Item, answer} from './barrel.ts'; const item: Item = {value:answer}; item.value;",
        ),
        ModuleSource::new(
            "graph/barrel.ts",
            "export {type Item, answer} from './dep.ts';",
        ),
        ModuleSource::new(
            "graph/dep.ts",
            "export interface Item {value:number;} export const answer: number = 42;",
        ),
    ]);
    let graph = compile_direct_module_graph("graph/main.ts", &loader, options()).unwrap();
    assert_eq!(graph.modules.len(), 3);
    assert_eq!(
        Vm::default()
            .execute_module_graph(&graph.entry, &graph.bytecode_map())
            .unwrap(),
        Value::Number(42.0)
    );
}

#[test]
fn type_only_namespace_reexport_has_no_runtime_dependency() {
    let loader = MapLoader::from([
        ModuleSource::new(
            "graph/main.ts",
            "import type {Types} from './barrel.ts'; const item: Types.Item = {value:42}; item.value;",
        ),
        ModuleSource::new(
            "graph/barrel.ts",
            "export type * as Types from './types.d.ts';",
        ),
        ModuleSource::new("graph/types.d.ts", "export interface Item {value:number;}"),
    ]);
    let graph = compile_direct_module_graph("graph/main.ts", &loader, options()).unwrap();
    assert_eq!(graph.modules.len(), 1);
    assert_eq!(
        Vm::default()
            .execute_module_graph(&graph.entry, &graph.bytecode_map())
            .unwrap(),
        Value::Number(42.0)
    );
}

#[test]
fn type_only_import_and_reexport_cannot_grant_a_declaration_value() {
    for main in [
        "import type {answer} from './types.d.ts'; answer;",
        "export {answer} from './types.d.ts';",
        "import {type answer} from './types.d.ts'; answer;",
    ] {
        let loader = MapLoader::from([
            ModuleSource::new("graph/main.ts", main),
            ModuleSource::new("graph/types.d.ts", "export declare const answer: number;"),
        ]);
        assert!(compile_direct_module_graph("graph/main.ts", &loader, options()).is_err());
    }
}
