// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Global declaration changes must invalidate consumers outside import edges.

use blueice_bluets::{compile, CompilerOptions, IncrementalCompiler, MapLoader, ModuleSource};

fn sources(change: &str) -> MapLoader {
    let declaration = if change == "augmentation" {
        "import type { Item as Imported } from './dep.ts'; export {}; \
         declare global { interface GlobalEnvelope { item: { value: string }; } }"
    } else {
        "import type { Item as Imported } from './dep.ts'; export {}; \
         declare global { interface GlobalEnvelope { item: Imported; } }"
    };
    let dependency = if change == "dependency" {
        "export interface Item { value: string; }"
    } else {
        "export interface Item { value: number; }"
    };
    MapLoader::from([
        ModuleSource::new(
            "memory:///main.ts",
            "import type {} from './augment.d.ts'; \
             import { answer } from './consumer.ts'; export const result = answer;",
        ),
        ModuleSource::new(
            "memory:///consumer.ts",
            if change == "alias-scope" {
                "export const answer: GlobalEnvelope = { item: { value: 42 } }; export type Leaked = Imported;"
            } else {
                "export const answer: GlobalEnvelope = { item: { value: 42 } };"
            },
        ),
        ModuleSource::new("memory:///augment.d.ts", declaration),
        ModuleSource::new("memory:///dep.ts", dependency),
    ])
}

fn assert_global_change_invalidates(change: &str) {
    let options = CompilerOptions::default();
    let mut compiler = IncrementalCompiler::new();
    let initial = compiler.compile("memory:///main.ts", &sources("none"), options.clone());
    assert!(
        !initial.compilation.has_errors(),
        "{:?}",
        initial.compilation.diagnostics
    );
    let changed = sources(change);
    let fresh = compile("memory:///main.ts", &changed, options.clone());
    assert!(fresh.has_errors(), "fresh control must reject {change}");
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
        "global {change} reused an accepted consumer: {:?}",
        incremental.reused_checked_modules
    );
    assert!(incremental.compilation.output.is_none());
    assert!(incremental
        .rechecked_modules
        .contains("memory:///consumer.ts"));
    assert_eq!(incremental.compilation.diagnostics, fresh.diagnostics);
}

#[test]
fn changed_global_augmentation_rechecks_an_unconnected_consumer() {
    assert_global_change_invalidates("augmentation");
}

#[test]
fn changed_global_import_dependency_rechecks_an_unconnected_consumer() {
    assert_global_change_invalidates("dependency");
}

#[test]
fn augmentation_import_alias_remains_local_to_its_declaration_module() {
    let compilation = compile(
        "memory:///main.ts",
        &sources("alias-scope"),
        CompilerOptions::default(),
    );
    assert!(compilation.has_errors());
    assert!(compilation.output.is_none());
    assert!(
        compilation.diagnostics.iter().any(|diagnostic| {
            diagnostic.typescript.as_ref().is_some_and(|counterpart| {
                counterpart.code == 2304 && counterpart.span.module == "memory:///consumer.ts"
            })
        }),
        "{:?}",
        compilation.diagnostics
    );
}
