// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Static dependency changes must agree with fresh compilation at consumers.

use blueice_bluets::{compile, CompilerOptions, IncrementalCompiler, MapLoader, ModuleSource};

fn sources(form: &str, changed: bool) -> MapLoader {
    let scalar = if changed { "string" } else { "number" };
    let sources = match form {
        "reference" => vec![
            ("main.ts", "/// <reference path=\"./globals.d.ts\" />\nimport { answer } from './consumer.ts'; export const result = answer;".to_string()),
            ("consumer.ts", "export const answer: GlobalItem = { value: 42 };".to_string()),
            ("globals.d.ts", format!("interface GlobalItem {{ value: {scalar}; }}")),
        ],
        "named" => vec![
            ("main.ts", "/// <reference path=\"./named.d.ts\" />\nimport { answer } from './consumer.ts'; export const result = answer;".to_string()),
            ("consumer.ts", "import type { Item } from 'owned'; export const answer: Item = { value: 42 };".to_string()),
            ("named.d.ts", format!("declare module 'owned' {{ interface Item {{ value: {scalar}; }} }}")),
        ],
        "augmentation" => vec![
            ("main.ts", "import type {} from './augment.d.ts'; import { answer } from './consumer.ts'; export const result = answer;".to_string()),
            ("consumer.ts", "import type { Item } from './dep.ts'; export const answer: Item = { value: 42, tag: 'ok' };".to_string()),
            ("dep.ts", "export interface Item { value: number; }".to_string()),
            ("augment.d.ts", format!("import type {{ Item }} from './dep.ts'; export {{}}; declare module './dep.ts' {{ interface Item {{ tag: {}; }} }}", if changed { "number" } else { "string" })),
        ],
        _ => unreachable!("known pinned static graph"),
    };
    MapLoader::from(
        sources
            .into_iter()
            .map(|(name, source)| ModuleSource::new(format!("memory:///{name}"), source)),
    )
}

fn assert_change_rechecks_consumer(form: &str) {
    let options = CompilerOptions::default();
    let mut compiler = IncrementalCompiler::new();
    let initial = compiler.compile("memory:///main.ts", &sources(form, false), options.clone());
    assert!(
        !initial.compilation.has_errors(),
        "{:?}",
        initial.compilation.diagnostics
    );
    let changed = sources(form, true);
    let fresh = compile("memory:///main.ts", &changed, options.clone());
    assert!(fresh.has_errors(), "fresh control must reject {form}");
    assert!(
        fresh.diagnostics.iter().any(|diagnostic| {
            diagnostic.typescript.as_ref().is_some_and(|counterpart| {
                counterpart.code == 2322 && counterpart.span.module == "memory:///consumer.ts"
            })
        }),
        "{:?}",
        fresh.diagnostics
    );
    let incremental = compiler.compile("memory:///main.ts", &changed, options);
    assert!(
        incremental.compilation.has_errors(),
        "static {form} reused an accepted consumer: {:?}",
        incremental.reused_checked_modules
    );
    assert!(incremental.compilation.output.is_none());
    assert!(incremental
        .rechecked_modules
        .contains("memory:///consumer.ts"));
    assert_eq!(incremental.compilation.diagnostics, fresh.diagnostics);
}

#[test]
fn changed_reference_global_rechecks_an_unconnected_consumer() {
    assert_change_rechecks_consumer("reference");
}

#[test]
fn changed_named_ambient_module_rechecks_its_importing_consumer() {
    assert_change_rechecks_consumer("named");
}

#[test]
fn changed_module_augmentation_rechecks_its_target_consumer() {
    assert_change_rechecks_consumer("augmentation");
}
