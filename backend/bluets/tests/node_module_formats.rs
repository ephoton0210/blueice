// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Owner format changes invalidate checking without changing source bytes.

use blueice_bluets::{
    CompilerOptions, IncrementalCompiler, ModuleKind, ModuleLoader, ModuleSource,
};
use std::cell::Cell;

struct Owner {
    esm: Cell<bool>,
}
impl ModuleLoader for Owner {
    fn load(&self, id: &str) -> Result<ModuleSource, String> {
        Ok(ModuleSource::new(
            id,
            "export const answer: number = await 42;",
        ))
    }
    fn implied_module_kind(&self, _: &str) -> Result<ModuleKind, String> {
        Ok(if self.esm.get() {
            ModuleKind::Esm
        } else {
            ModuleKind::CommonJs
        })
    }
}

#[test]
fn a_format_change_rechecks_unchanged_source_and_refuses_commonjs_await() {
    let owner = Owner {
        esm: Cell::new(true),
    };
    let options = CompilerOptions {
        module_kind: ModuleKind::NodeNext,
        ..CompilerOptions::default()
    };
    let mut compiler = IncrementalCompiler::new();
    let first = compiler.compile("memory:///main.ts", &owner, options.clone());
    assert!(
        !first.compilation.has_errors(),
        "{:?}",
        first.compilation.diagnostics
    );
    let repeated = compiler.compile("memory:///main.ts", &owner, options.clone());
    assert!(repeated.cache_hit);
    owner.esm.set(false);
    let changed = compiler.compile("memory:///main.ts", &owner, options);
    assert!(!changed.cache_hit);
    assert!(changed.reused_parsed_modules.contains("memory:///main.ts"));
    assert!(changed.rechecked_modules.contains("memory:///main.ts"));
    assert!(
        changed.compilation.has_errors(),
        "{:?}",
        changed.compilation.diagnostics
    );
    assert!(changed.compilation.output.is_none());
    assert_ne!(
        changed.compilation.project_fingerprint,
        first.compilation.project_fingerprint
    );
}
