// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn logical_expression_results_keep_possible_readonly_event_types() {
    let source = "interface Event { readonly type: 'click'; }\n\
                  interface MutableEvent { type: string; }\n\
                  interface Holder { event: Event; mutable: MutableEvent; }\n\
                  function write(holder: Holder): void {\n\
                    const fromOr = holder.mutable || holder.event;\n\
                    fromOr.type = 'click';\n\
                    const fromAnd = holder.mutable && holder.event;\n\
                    fromAnd.type = 'click';\n\
                    const same = holder.event || holder.event;\n\
                    same.type = 'click';\n\
                    (holder.mutable || holder.event).type = 'click';\n\
                  }";
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
        CompilerOptions::default(),
    );
    let readonly = result
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.message.contains("readonly"))
        .count();
    assert_eq!(readonly, 4, "{:#?}", result.diagnostics);
    assert_eq!(result.diagnostics.len(), 4, "{:#?}", result.diagnostics);

    let valid = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "interface MutableEvent { type: string; } interface Holder { first: MutableEvent; second: MutableEvent; } function write(holder: Holder): void { const fromOr = holder.first || holder.second; fromOr.type = 'ok'; const fromAnd = holder.first && holder.second; fromAnd.type = 'ok'; const flags: boolean = true && false; }",
        )]),
        CompilerOptions::default(),
    );
    assert!(!valid.has_errors(), "{:#?}", valid.diagnostics);
}

#[test]
fn logical_expression_inference_has_a_resource_boundary() {
    let chain = format!("{}value", "value || ".repeat(129));
    let source = format!("const value: number = 1; const selected = {chain};");
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new("memory:///main.ts", &source)]),
        CompilerOptions::default(),
    );
    assert!(
        result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::ResourceLimit
                && diagnostic
                    .message
                    .contains("logical expression inference limit")
        }),
        "{:#?}",
        result.diagnostics
    );

    let boundary = format!("{}value", "value || ".repeat(128));
    let source = format!("const value: number = 1; const selected = {boundary};");
    let accepted = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new("memory:///main.ts", &source)]),
        CompilerOptions::default(),
    );
    assert!(!accepted.has_errors(), "{:#?}", accepted.diagnostics);
}
