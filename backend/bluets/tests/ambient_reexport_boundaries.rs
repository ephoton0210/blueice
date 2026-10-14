// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Type-only re-exports permit queries while retaining declaration authority.

use blueice_bluets::{compile, CompilerOptions, MapLoader, ModuleSource, RuntimePolicy};

fn compilation(source: &str, policy: RuntimePolicy) -> blueice_bluets::Compilation {
    compile(
        "memory:///main.ts",
        &MapLoader::from([
            ModuleSource::new("memory:///main.ts", source),
            ModuleSource::new(
                "memory:///public.ts",
                "export type { answer } from 'owned';",
            ),
        ]),
        CompilerOptions {
            runtime_policy: policy,
            ambient_declaration_modules: vec![ModuleSource::new(
                "memory:///ambient.d.ts",
                "declare module 'owned' { export const answer: number; }",
            )],
            ..CompilerOptions::default()
        },
    )
}

#[test]
fn reexported_declaration_queries_publish_no_runtime_binding() {
    for policy in [RuntimePolicy::Checked, RuntimePolicy::TranspileOnly] {
        let result = compilation(
            "import { answer } from './public.ts'; export const result: typeof answer = 42;",
            policy,
        );
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        assert_eq!(
            result
                .project
                .resolved_module("memory:///public.ts", "owned"),
            None
        );
        assert!(!result.project.modules.keys().any(|id| id.contains('\0')));
        let output = result.output.unwrap();
        for artifact in output.artifacts.values() {
            assert!(!artifact.javascript.contains("owned"));
        }
        assert!(!output.artifacts["memory:///main.ts"]
            .javascript
            .contains("import"));
    }
}

fn refuses_runtime_value(policy: RuntimePolicy) {
    let result = compilation(
        "import { answer } from './public.ts'; export const result = answer;",
        policy,
    );
    assert!(
        result.has_errors(),
        "declaration authorized a value under {policy:?}"
    );
    assert!(result.output.is_none());
    let primary = result
        .diagnostics
        .iter()
        .find_map(|diagnostic| {
            diagnostic
                .typescript
                .as_ref()
                .filter(|counterpart| counterpart.code == 1362)
        })
        .expect("pinned TypeScript rejects an export-type value use with TS1362");
    assert_eq!(primary.span.module, "memory:///main.ts");
    assert!(primary
        .related_information
        .iter()
        .any(|related| { related.code == 1377 && related.span.module == "memory:///public.ts" }));
}

#[test]
fn checked_reexported_declarations_cannot_supply_runtime_values() {
    refuses_runtime_value(RuntimePolicy::Checked);
}

#[test]
fn transpile_only_retains_unchecked_output_without_ambient_resolution() {
    let result = compilation(
        "import { answer } from './public.ts'; export const result = answer;",
        RuntimePolicy::TranspileOnly,
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    assert_eq!(
        result
            .project
            .resolved_module("memory:///public.ts", "owned"),
        None
    );
    assert!(!result.project.modules.keys().any(|id| id.contains('\0')));
    for artifact in result.output.unwrap().artifacts.values() {
        assert!(!artifact.javascript.contains("owned"));
    }
}
