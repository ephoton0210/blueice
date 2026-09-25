// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn opaque_record_spread_sources_fail_closed_before_readonly_mutation() {
    for source in [
        "interface Event { readonly type: 'click'; } interface Holder { event: Event; } function erase(value: Holder): any { return value; } function write(holder: Holder): void { const copied = { ...erase(holder) }; copied.event.type = 'click'; }",
        "function write(source: unknown): void { const copied = { ...source }; copied.event.type = 'click'; }",
        "function write(source: any): void { const copied = { ...source }; copied.event.type = 'click'; }",
        "function write(source: any): void { const copied = { ...source }; }",
        "interface Event { readonly type: 'click'; } interface Holder { event: Event; } type MaybeHolder = Holder | unknown; function write(source: MaybeHolder): void { const copied = { ...source }; copied.event.type = 'click'; }",
        "const copied = { ...42 };",
    ] {
        let result = crate::compile(
            "memory:///main.ts",
            &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
            CompilerOptions::default(),
        );
        assert!(
            result.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == DiagnosticCode::TypeMismatch
                    && diagnostic.message.contains("record spread source")
            }),
            "{source}: {:#?}",
            result.diagnostics
        );
    }

    let valid = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "interface MutableEvent { type: string; } interface Holder { event: MutableEvent; } function write(holder: Holder): void { const copied = { ...holder }; copied.event.type = 'ok'; }",
        )]),
        CompilerOptions::default(),
    );
    assert!(!valid.has_errors(), "{:#?}", valid.diagnostics);

    let transpile_only = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "function copy(source: any): void { const copied = { ...source }; }",
        )]),
        CompilerOptions {
            runtime_policy: crate::compiler::RuntimePolicy::TranspileOnly,
            ..CompilerOptions::default()
        },
    );
    assert!(
        !transpile_only.has_errors(),
        "{:#?}",
        transpile_only.diagnostics
    );
}
