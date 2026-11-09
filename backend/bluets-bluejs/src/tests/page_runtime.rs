// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Shared fixtures for direct BlueTS page-realm regressions.

use super::*;
use blueice_bluets::{AuthorizedModule, AuthorizedModuleLoader, AuthorizedModuleResolution};
use std::collections::{BTreeMap, BTreeSet};

const CALLBACK_METHOD_SOURCE: &str = "interface Visitor { visit(kind: 'text', callback: (value: string) => void): void; visit(kind: 'count', callback: (value: number) => void): void; } let observed: number = 0; function onText(value: string): void { if (value === 'A') { observed += 1; } } function onCount(value: number): void { observed += value; } function dispatch(kind: string, listener: any): void { if (kind === 'text') { listener('A'); } else { listener(2); } } const visitor: Visitor = { visit: dispatch }; visitor.visit('text', onText); visitor.visit('count', onCount); observed;";

fn artifact() -> DirectScript {
    compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "const answer: number = 40 + 2; answer;",
        )]),
        CompilerOptions::default(),
    )
    .unwrap()
}

fn origin() -> bluejs::BlueJsPageOrigin {
    bluejs::BlueJsPageOrigin::new("https://example.test").unwrap()
}

fn module_graph() -> DirectModuleGraph {
    let entry = "page:///app/main.ts";
    let dependency = "page:///app/dependency.ts";
    compile_direct_module_graph(
        entry,
        &AuthorizedModuleLoader::new(
            [
                AuthorizedModule::new(
                    entry,
                    "import { value } from './dependency'; \
                     export const answer: number = value + 1; answer;",
                ),
                AuthorizedModule::new(dependency, "export const value: number = 41;"),
            ],
            [AuthorizedModuleResolution::new(
                entry,
                "./dependency",
                dependency,
            )],
        )
        .unwrap(),
        CompilerOptions {
            resolver_fingerprint: "page-authorized-resolver-v1".to_string(),
            ..CompilerOptions::default()
        },
    )
    .unwrap()
}

fn class_options() -> CompilerOptions {
    CompilerOptions::default()
}

fn run_direct_page_script(source: &str) -> bluejs::Value {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        class_options(),
    )
    .unwrap();
    let mut owner = DirectPageRealmOwner::default();
    owner.open_realm(7, origin()).unwrap();
    let attachment = owner.attach_script(&artifact, 7, &origin()).unwrap();
    owner.execute_program(7, &attachment).unwrap()
}

mod classes;
mod control_flow;
mod debugger;
mod declarations;
mod lifecycle;
mod module_graph;
