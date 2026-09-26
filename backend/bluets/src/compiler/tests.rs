// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use std::cell::RefCell;

#[derive(Default)]
struct MutableLoader {
    modules: RefCell<BTreeMap<String, ModuleSource>>,
    resolutions: RefCell<BTreeMap<(String, String), String>>,
}

impl MutableLoader {
    fn from(sources: impl IntoIterator<Item = ModuleSource>) -> Self {
        Self {
            modules: RefCell::new(
                sources
                    .into_iter()
                    .map(|source| (source.id.clone(), source))
                    .collect(),
            ),
            resolutions: RefCell::new(BTreeMap::new()),
        }
    }

    fn replace(&self, source: ModuleSource) {
        self.modules.borrow_mut().insert(source.id.clone(), source);
    }

    fn remap(&self, from_module: &str, specifier: &str, module_id: &str) {
        self.resolutions.borrow_mut().insert(
            (from_module.to_string(), specifier.to_string()),
            module_id.to_string(),
        );
    }
}

impl ModuleLoader for MutableLoader {
    fn load(&self, module_id: &str) -> Result<ModuleSource, String> {
        self.modules
            .borrow()
            .get(module_id)
            .cloned()
            .ok_or_else(|| format!("module `{module_id}` is not present in this loader"))
    }

    fn resolve(&self, from_module: &str, specifier: &str) -> Result<String, String> {
        self.resolutions
            .borrow()
            .get(&(from_module.to_string(), specifier.to_string()))
            .cloned()
            .map(Ok)
            .unwrap_or_else(|| resolve_relative_module(from_module, specifier))
    }
}

#[test]
fn resolves_a_relative_url_module_without_filesystem_access() {
    let loader = MapLoader::from([
        ModuleSource::new(
            "memory:///src/main.ts",
            "import type { User } from './types.ts'; const x: User = { name: 'Ada' };",
        ),
        ModuleSource::new(
            "memory:///src/types.ts",
            "export interface User { name: string }",
        ),
    ]);
    let result = compile("memory:///src/main.ts", &loader, CompilerOptions::default());
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    assert_eq!(result.project.modules.len(), 2);
}

#[test]
fn rejects_bare_specifiers_without_host_configuration() {
    let loader = MapLoader::from([ModuleSource::new("memory:///a.ts", "import 'package';")]);
    let result = compile("memory:///a.ts", &loader, CompilerOptions::default());
    assert!(result
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == DiagnosticCode::ModuleNotFound));
}

#[test]
fn enforces_module_graph_and_source_work_limits() {
    let graph = MapLoader::from([
        ModuleSource::new(
            "memory:///main.ts",
            "import type { Model } from './model.ts'; const value: Model = { id: 'ok' };",
        ),
        ModuleSource::new(
            "memory:///model.ts",
            "export interface Model { id: string }",
        ),
    ]);
    let module_limited = compile(
        "memory:///main.ts",
        &graph,
        CompilerOptions {
            limits: CompilerLimits {
                max_modules: 1,
                ..CompilerLimits::default()
            },
            ..CompilerOptions::default()
        },
    );
    assert!(module_limited.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::ResourceLimit
            && diagnostic.message.contains("module limit")
    }));
    assert!(module_limited.output.is_none());

    let depth_limited = compile(
        "memory:///main.ts",
        &MapLoader::from([
            ModuleSource::new(
                "memory:///main.ts",
                "import type { Mid } from './mid.ts'; const value: Mid = { id: 'ok' };",
            ),
            ModuleSource::new(
                "memory:///mid.ts",
                "import type { Leaf } from './leaf.ts'; export type Mid = Leaf;",
            ),
            ModuleSource::new("memory:///leaf.ts", "export interface Leaf { id: string }"),
        ]),
        CompilerOptions {
            limits: CompilerLimits {
                max_module_depth: 1,
                ..CompilerLimits::default()
            },
            ..CompilerOptions::default()
        },
    );
    assert!(depth_limited.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::ResourceLimit
            && diagnostic.message.contains("import-depth limit")
    }));
    assert!(depth_limited.output.is_none());

    let edge_limited = compile(
        "memory:///main.ts",
        &graph,
        CompilerOptions {
            limits: CompilerLimits {
                max_module_edges: 0,
                ..CompilerLimits::default()
            },
            ..CompilerOptions::default()
        },
    );
    assert!(edge_limited.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::ResourceLimit
            && diagnostic.message.contains("import-edge limit")
    }));
    assert!(edge_limited.output.is_none());

    let source_limited = compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "const value: number = 1;",
        )]),
        CompilerOptions {
            limits: CompilerLimits {
                max_total_source_bytes: 8,
                ..CompilerLimits::default()
            },
            ..CompilerOptions::default()
        },
    );
    assert!(source_limited.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::ResourceLimit
            && diagnostic.message.contains("total source-byte limit")
    }));
    assert!(source_limited.output.is_none());
}

#[test]
fn resolver_identity_changes_the_artifact_fingerprint() {
    let loader = MapLoader::from([ModuleSource::new(
        "memory:///a.ts",
        "export const answer: number = 42;",
    )]);
    let default = compile("memory:///a.ts", &loader, CompilerOptions::default())
        .output
        .unwrap()
        .fingerprint;
    let mapped = compile(
        "memory:///a.ts",
        &loader,
        CompilerOptions {
            resolver_fingerprint: "import-map-deadbeef".to_string(),
            ..CompilerOptions::default()
        },
    )
    .output
    .unwrap()
    .fingerprint;
    assert_ne!(default, mapped);
}

#[test]
fn declared_global_call_policy_changes_the_artifact_fingerprint() {
    let loader = MapLoader::from([ModuleSource::new(
        "memory:///a.ts",
        "export const answer: number = 42;",
    )]);
    let default = compile("memory:///a.ts", &loader, CompilerOptions::default())
        .output
        .unwrap()
        .fingerprint;
    let page_profile = compile(
        "memory:///a.ts",
        &loader,
        CompilerOptions {
            require_declared_global_calls: true,
            ..CompilerOptions::default()
        },
    )
    .output
    .unwrap()
    .fingerprint;

    assert_ne!(default, page_profile);
}

#[test]
fn treats_declaration_modules_as_type_only_dependencies() {
    let loader = MapLoader::from([
        ModuleSource::new(
            "memory:///src/main.ts",
            "import type { User } from './types.d.ts'; export const user: User = { id: 'ada' };",
        ),
        ModuleSource::new(
            "memory:///src/types.d.ts",
            "export interface User { id: string }",
        ),
    ]);
    let compilation = compile(
        "memory:///src/main.ts",
        &loader,
        CompilerOptions {
            declaration: true,
            ..CompilerOptions::default()
        },
    );
    assert!(!compilation.has_errors(), "{:?}", compilation.diagnostics);
    let output = compilation.output.unwrap();
    assert!(output.artifacts.contains_key("memory:///src/main.ts"));
    assert!(!output.artifacts.contains_key("memory:///src/types.d.ts"));
    assert_eq!(
        output.declaration_modules.get("memory:///src/types.d.ts"),
        Some(&"export interface User { id: string }".to_string())
    );
    assert!(compilation
        .debug_info
        .unwrap()
        .sources
        .iter()
        .any(|source| source.module == "memory:///src/types.d.ts"));
}

#[test]
fn consumes_host_verified_ambient_declarations_without_emitting_them() {
    let loader = MapLoader::from([ModuleSource::new(
        "memory:///src/main.ts",
        "const answer: number = hostAnswer; answer;",
    )]);
    let options = CompilerOptions {
        ambient_declaration_modules: vec![ModuleSource::new(
            "blueice:///profiles/test/lib.blueice.d.ts",
            "declare const hostAnswer: number;",
        )],
        ..CompilerOptions::default()
    };
    let compilation = compile("memory:///src/main.ts", &loader, options.clone());
    assert!(!compilation.has_errors(), "{:?}", compilation.diagnostics);
    assert!(compilation
        .project
        .modules
        .contains_key("blueice:///profiles/test/lib.blueice.d.ts"));
    let javascript = &compilation.output.unwrap().artifacts["memory:///src/main.ts"].javascript;
    assert!(javascript.contains("hostAnswer"));
    assert!(!javascript.contains("declare const"));

    let mismatch = compile(
        "memory:///src/main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///src/main.ts",
            "const answer: string = hostAnswer;",
        )]),
        options.clone(),
    );
    assert!(mismatch.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::TypeMismatch
            && diagnostic.span.module == "memory:///src/main.ts"
    }));
    assert!(mismatch.output.is_none());

    let declaration_output = compile(
        "memory:///src/main.ts",
        &loader,
        CompilerOptions {
            declaration: true,
            ..options
        },
    )
    .output
    .unwrap();
    assert!(!declaration_output
        .declaration_modules
        .contains_key("blueice:///profiles/test/lib.blueice.d.ts"));
}

#[test]
fn declared_global_call_policy_rejects_unprovided_page_globals() {
    let compilation = compile(
        "memory:///src/main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///src/main.ts",
            "blueiceDocumentText();",
        )]),
        CompilerOptions {
            require_declared_global_calls: true,
            ..CompilerOptions::default()
        },
    );

    assert!(compilation.output.is_none());
    assert!(compilation.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::UnknownName
            && diagnostic.message
                == "function blueiceDocumentText is not declared by this page profile"
    }));
}

#[test]
fn ambient_declarations_are_closed_and_counted_as_declaration_modules() {
    let loader = MapLoader::from([ModuleSource::new("memory:///src/main.ts", "1;")]);
    let invalid_id = compile(
        "memory:///src/main.ts",
        &loader,
        CompilerOptions {
            ambient_declaration_modules: vec![ModuleSource::new(
                "blueice:///profiles/test/lib.blueice.ts",
                "declare const hostAnswer: number;",
            )],
            ..CompilerOptions::default()
        },
    );
    assert!(invalid_id.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::InvalidDeclarationFile
            && diagnostic.message.contains(".d.ts")
    }));

    let imported = compile(
        "memory:///src/main.ts",
        &loader,
        CompilerOptions {
            ambient_declaration_modules: vec![ModuleSource::new(
                "blueice:///profiles/test/lib.blueice.d.ts",
                "import type { Missing } from './missing.d.ts';",
            )],
            ..CompilerOptions::default()
        },
    );
    assert!(imported.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::InvalidDeclarationFile
            && diagnostic.message.contains("cannot import")
    }));

    let limited = compile(
        "memory:///src/main.ts",
        &loader,
        CompilerOptions {
            ambient_declaration_modules: vec![ModuleSource::new(
                "blueice:///profiles/test/lib.blueice.d.ts",
                "declare const hostAnswer: number;",
            )],
            limits: CompilerLimits {
                max_modules: 1,
                ..CompilerLimits::default()
            },
            ..CompilerOptions::default()
        },
    );
    assert!(limited.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::ResourceLimit
            && diagnostic.message.contains("module limit")
    }));
}

#[test]
fn rejects_runtime_use_or_runtime_content_in_a_declaration_module() {
    let value_import = MapLoader::from([
        ModuleSource::new(
            "memory:///src/main.ts",
            "import { createUser } from './types.d.ts'; createUser();",
        ),
        ModuleSource::new(
            "memory:///src/types.d.ts",
            "export declare function createUser(): string;",
        ),
    ]);
    let imported = compile(
        "memory:///src/main.ts",
        &value_import,
        CompilerOptions::default(),
    );
    assert!(imported.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::InvalidDeclarationFile
            && diagnostic.message.contains("type-only")
    }));

    let runtime_content = MapLoader::from([
        ModuleSource::new(
            "memory:///src/main.ts",
            "import type { User } from './types.d.ts'; const user: User = { id: 'ada' };",
        ),
        ModuleSource::new(
            "memory:///src/types.d.ts",
            "export const unexpected: number = 1; export interface User { id: string }",
        ),
    ]);
    let declared = compile(
        "memory:///src/main.ts",
        &runtime_content,
        CompilerOptions::default(),
    );
    assert!(declared.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::InvalidDeclarationFile
            && diagnostic.message.contains("runtime declaration")
    }));
}

#[test]
fn accepts_signature_only_functions_in_a_declaration_module() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([
            ModuleSource::new(
                "memory:///main.ts",
                "import type { Result } from './types/functions.d.ts';\n\
                 const value: Result = 'ok';",
            ),
            ModuleSource::new(
                "memory:///types/functions.d.ts",
                "export type Result = string;\n\
                 export function describe(value: string): string;",
            ),
        ]),
        CompilerOptions::default(),
    );
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
}

#[test]
fn incremental_compiler_rechecks_only_a_changed_module_and_its_dependents() {
    let loader = MutableLoader::from([
        ModuleSource::new(
            "memory:///src/main.ts",
            "import type { Left } from './left.ts';\nimport type { Right } from './right.ts';\nexport const left: Left = { id: 'left' };\nexport const right: Right = { id: 'right' };",
        ),
        ModuleSource::new(
            "memory:///src/left.ts",
            "export interface Left { id: string }",
        ),
        ModuleSource::new(
            "memory:///src/right.ts",
            "export interface Right { id: string }",
        ),
    ]);
    let mut compiler = IncrementalCompiler::new();

    let first = compiler.compile("memory:///src/main.ts", &loader, CompilerOptions::default());
    assert!(
        !first.compilation.has_errors(),
        "{:?}",
        first.compilation.diagnostics
    );
    assert_eq!(first.parsed_modules.len(), 3);
    assert_eq!(first.rechecked_modules.len(), 3);

    loader.replace(ModuleSource::new(
        "memory:///src/left.ts",
        "export interface Left { id: string; revision?: number }",
    ));
    let second = compiler.compile("memory:///src/main.ts", &loader, CompilerOptions::default());
    assert!(
        !second.compilation.has_errors(),
        "{:?}",
        second.compilation.diagnostics
    );
    assert_eq!(
        second.parsed_modules,
        BTreeSet::from(["memory:///src/left.ts".to_string()])
    );
    assert_eq!(
        second.rechecked_modules,
        BTreeSet::from([
            "memory:///src/left.ts".to_string(),
            "memory:///src/main.ts".to_string(),
        ])
    );
    assert_eq!(
        second.reused_checked_modules,
        BTreeSet::from(["memory:///src/right.ts".to_string()])
    );
    assert!(second.compilation.output.is_some());

    let third = compiler.compile("memory:///src/main.ts", &loader, CompilerOptions::default());
    assert!(third.cache_hit);
    assert!(third.rechecked_modules.is_empty());
    assert_eq!(third.reused_checked_modules.len(), 3);
}

#[test]
fn incremental_compiler_refuses_a_cache_entry_from_another_policy() {
    let loader = MutableLoader::from([ModuleSource::new(
        "memory:///src/main.ts",
        "export const answer: number = 42;",
    )]);
    let mut compiler = IncrementalCompiler::new();
    let _ = compiler.compile("memory:///src/main.ts", &loader, CompilerOptions::default());

    let result = compiler.compile(
        "memory:///src/main.ts",
        &loader,
        CompilerOptions {
            runtime_policy: RuntimePolicy::StrictRuntime,
            ..CompilerOptions::default()
        },
    );
    assert!(!result.cache_hit);
    assert_eq!(result.parsed_modules.len(), 1);
    assert_eq!(result.rechecked_modules.len(), 1);
}

#[test]
fn incremental_compiler_invalidates_an_importer_when_resolution_changes() {
    let loader = MutableLoader::from([
        ModuleSource::new(
            "memory:///src/main.ts",
            "import type { Model } from '@model'; export const model: Model = { id: 'model' };",
        ),
        ModuleSource::new(
            "memory:///src/first.ts",
            "export interface Model { id: string }",
        ),
        ModuleSource::new(
            "memory:///src/second.ts",
            "export interface Model { id: string; version?: number }",
        ),
    ]);
    loader.remap("memory:///src/main.ts", "@model", "memory:///src/first.ts");
    let mut compiler = IncrementalCompiler::new();
    let _ = compiler.compile("memory:///src/main.ts", &loader, CompilerOptions::default());

    loader.remap("memory:///src/main.ts", "@model", "memory:///src/second.ts");
    let result = compiler.compile("memory:///src/main.ts", &loader, CompilerOptions::default());
    assert!(
        !result.compilation.has_errors(),
        "{:?}",
        result.compilation.diagnostics
    );
    assert_eq!(
        result.rechecked_modules,
        BTreeSet::from([
            "memory:///src/main.ts".to_string(),
            "memory:///src/second.ts".to_string(),
        ])
    );
}

#[test]
fn incremental_compiler_keeps_the_last_success_after_an_error() {
    let loader = MutableLoader::from([ModuleSource::new(
        "memory:///src/main.ts",
        "export const answer: number = 42;",
    )]);
    let mut compiler = IncrementalCompiler::new();
    let _ = compiler.compile("memory:///src/main.ts", &loader, CompilerOptions::default());

    loader.replace(ModuleSource::new(
        "memory:///src/main.ts",
        "export const answer: number = 'wrong';",
    ));
    let failed = compiler.compile("memory:///src/main.ts", &loader, CompilerOptions::default());
    assert!(failed.compilation.has_errors());
    assert!(failed.compilation.output.is_none());

    loader.replace(ModuleSource::new(
        "memory:///src/main.ts",
        "export const answer: number = 42;",
    ));
    let recovered = compiler.compile("memory:///src/main.ts", &loader, CompilerOptions::default());
    assert!(recovered.cache_hit);
    assert!(recovered.compilation.output.is_some());
}
