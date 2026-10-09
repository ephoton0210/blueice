// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Owner resolution observations bind cache reuse and native artifact identity.

use blueice_bluets::{CompilerOptions, IncrementalCompiler, MapLoader, ModuleLoader, ModuleSource};
use std::cell::Cell;

struct ObservedLoader {
    modules: MapLoader,
    policy: Cell<u64>,
    loaded: Cell<bool>,
}

impl ModuleLoader for ObservedLoader {
    fn load(&self, module_id: &str) -> Result<ModuleSource, String> {
        self.loaded.set(true);
        self.modules.load(module_id)
    }

    fn resolution_fingerprint(&self) -> String {
        assert!(
            self.loaded.get(),
            "collect observations after graph loading"
        );
        format!("owner-observations-{}", self.policy.get())
    }
}

#[test]
fn changed_resolution_observations_refresh_artifacts_without_rechecking_identical_types() {
    const ENTRY: &str = "memory:///main.ts";
    let loader = ObservedLoader {
        modules: MapLoader::from([ModuleSource::new(
            ENTRY,
            "export const answer: number = 42;",
        )]),
        policy: Cell::new(0),
        loaded: Cell::new(false),
    };
    let options = CompilerOptions::default();
    let mut compiler = IncrementalCompiler::new();
    let first = compiler.compile(ENTRY, &loader, options.clone());
    assert!(!first.compilation.has_errors());

    loader.loaded.set(false);
    let unchanged = compiler.compile(ENTRY, &loader, options.clone());
    assert!(unchanged.cache_hit);
    assert_eq!(
        first.compilation.project_fingerprint,
        unchanged.compilation.project_fingerprint
    );

    loader.policy.set(1);
    loader.loaded.set(false);
    let changed = compiler.compile(ENTRY, &loader, options.clone());
    assert!(!changed.compilation.has_errors());
    assert!(!changed.cache_hit);
    assert!(changed.rechecked_modules.is_empty());
    assert!(changed.reused_checked_modules.contains(ENTRY));
    assert_ne!(
        first.compilation.project_fingerprint,
        changed.compilation.project_fingerprint
    );
    assert_ne!(
        first.compilation.output.unwrap().fingerprint,
        changed.compilation.output.as_ref().unwrap().fingerprint
    );
    let native = changed
        .compilation
        .emit_for_native_cli(&options)
        .unwrap()
        .unwrap();
    assert_eq!(native.fingerprint, changed.compilation.project_fingerprint);
}
