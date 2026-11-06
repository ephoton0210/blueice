// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Configured declaration roots share ordinary graph ownership and budgets.
use blueice_bluets::{
    compile, CompilerOptions, DiagnosticCode, IncrementalCompiler, ModuleLoader, ModuleSource,
};
use std::{cell::RefCell, collections::BTreeMap};

struct Owner {
    loaded: RefCell<Vec<String>>,
}
impl ModuleLoader for Owner {
    fn load(&self, id: &str) -> Result<ModuleSource, String> {
        self.loaded.borrow_mut().push(id.into());
        let source = match id {
            "main.ts" => "import type {} from './augment'; export const value: Envelope = { item: { value: 42 } };",
            "augment.d.ts" => "import type { Item } from './types'; export {}; declare global { interface Envelope { item: Item; } }",
            "types.d.ts" => "export interface Item { value: number; }",
            _ => return Err(format!("owner refuses {id}")),
        };
        Ok(ModuleSource::new(id, source))
    }
    fn resolve(&self, _: &str, specifier: &str) -> Result<String, String> {
        match specifier {
            "./augment" => Ok("augment.d.ts".into()),
            "./types" => Ok("types.d.ts".into()),
            _ => Err(format!("owner refuses {specifier}")),
        }
    }
}
fn options() -> CompilerOptions {
    CompilerOptions {
        declaration_roots: vec!["augment.d.ts".into()],
        ..CompilerOptions::default()
    }
}

#[test]
fn declaration_root_already_reached_by_import_is_loaded_once_without_host_authority() {
    let owner = Owner {
        loaded: RefCell::new(Vec::new()),
    };
    let result = compile("main.ts", &owner, options());
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let counts = owner
        .loaded
        .borrow()
        .iter()
        .fold(BTreeMap::new(), |mut counts, id| {
            *counts.entry(id.clone()).or_insert(0) += 1;
            counts
        });
    assert_eq!(
        counts,
        BTreeMap::from([
            ("main.ts".into(), 1),
            ("augment.d.ts".into(), 1),
            ("types.d.ts".into(), 1)
        ])
    );
    assert!(!result.project.is_ambient_declaration_module("augment.d.ts"));
    let output = result.output.unwrap();
    assert_eq!(output.artifacts.len(), 1);
    assert!(output.artifacts.contains_key("main.ts"));
}

#[test]
fn configured_roots_retain_graph_limits_and_non_declaration_refusals() {
    let owner = Owner {
        loaded: RefCell::new(Vec::new()),
    };
    for policy in ["modules", "edges", "depth", "bytes", "identity"] {
        let mut options = options();
        match policy {
            "modules" => options.limits.max_modules = 1,
            "edges" => options.limits.max_module_edges = 0,
            "depth" => options.limits.max_module_depth = 0,
            "bytes" => options.limits.max_total_source_bytes = 1,
            "identity" => options.declaration_roots = vec!["other.ts".into()],
            _ => unreachable!(),
        }
        let result = compile("main.ts", &owner, options);
        assert!(result.output.is_none(), "{policy}");
        assert!(
            result.diagnostics.iter().any(|d| d.code
                == if policy == "identity" {
                    DiagnosticCode::InvalidDeclarationFile
                } else {
                    DiagnosticCode::ResourceLimit
                }),
            "{policy}: {:?}",
            result.diagnostics
        );
    }
}

#[test]
fn root_selection_is_part_of_artifact_and_session_identity() {
    let owner = Owner {
        loaded: RefCell::new(Vec::new()),
    };
    let mut session = IncrementalCompiler::new();
    let first = session.compile("main.ts", &owner, options());
    assert!(!first.compilation.has_errors());
    assert!(session.compile("main.ts", &owner, options()).cache_hit);
    let changed = session.compile("main.ts", &owner, CompilerOptions::default());
    assert!(!changed.cache_hit);
    assert_ne!(
        first.compilation.project_fingerprint,
        changed.compilation.project_fingerprint
    );
}

#[test]
fn changing_an_unconnected_script_root_rechecks_its_global_consumers() {
    struct ScriptOwner(RefCell<String>);
    impl ModuleLoader for ScriptOwner {
        fn load(&self, id: &str) -> Result<ModuleSource, String> {
            match id {
                "main.ts" => Ok(ModuleSource::new(
                    id,
                    "export const value: GlobalItem = { value: 42 };",
                )),
                "globals.d.ts" => Ok(ModuleSource::new(id, self.0.borrow().clone())),
                _ => Err(format!("owner refuses {id}")),
            }
        }
    }
    let owner = ScriptOwner(RefCell::new(
        "interface GlobalItem { value: number; }".into(),
    ));
    let options = CompilerOptions {
        declaration_roots: vec!["globals.d.ts".into()],
        ..CompilerOptions::default()
    };
    let mut session = IncrementalCompiler::new();
    let first = session.compile("main.ts", &owner, options.clone());
    assert!(
        !first.compilation.has_errors(),
        "{:?}",
        first.compilation.diagnostics
    );
    assert!(
        session
            .compile("main.ts", &owner, options.clone())
            .cache_hit
    );
    *owner.0.borrow_mut() = "interface GlobalItem { value: string; }".into();
    let changed = session.compile("main.ts", &owner, options.clone());
    let fresh = compile("main.ts", &owner, options);
    assert!(fresh.has_errors());
    assert!(changed.rechecked_modules.contains("main.ts"));
    assert!(changed.compilation.output.is_none());
    assert_eq!(changed.compilation.diagnostics, fresh.diagnostics);
}
