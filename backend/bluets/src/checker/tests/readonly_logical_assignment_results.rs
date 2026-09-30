// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn logical_assignment_results_preserve_each_possible_readonly_branch() {
    let source = "interface Event { readonly type: 'click'; }\n\
                  interface MutableEvent { type: string; }\n\
                  interface Holder { slot: MutableEvent; event: MutableEvent; }\n\
                  interface Source { event: Event; }\n\
                  function write(holder: Holder, source: Source): void {\n\
                    const fromOr = (holder.slot ||= source.event);\n\
                    fromOr.type = 'click';\n\
                    const fromAnd = (holder.slot &&= source.event);\n\
                    fromAnd.type = 'click';\n\
                    const fromNullish = (holder.slot ??= source.event);\n\
                    fromNullish.type = 'click';\n\
                    (holder.slot ||= source.event).type = 'click';\n\
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
            "interface MutableEvent { type: string; } interface Holder { slot: MutableEvent; event: MutableEvent; } function write(holder: Holder): void { const orValue = (holder.slot ||= holder.event); orValue.type = 'ok'; const andValue = (holder.slot &&= holder.event); andValue.type = 'ok'; const nullishValue = (holder.slot ??= holder.event); nullishValue.type = 'ok'; }",
        )]),
        CompilerOptions::default(),
    );
    assert!(!valid.has_errors(), "{:#?}", valid.diagnostics);
}

#[test]
fn logical_assignment_inference_has_a_resource_boundary() {
    let chain = format!("{}value", "value ||= ".repeat(129));
    let source = format!("let value: number = 1; const selected = {chain};");
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new("memory:///main.ts", &source)]),
        CompilerOptions::default(),
    );
    assert!(
        result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::ResourceLimit
                && diagnostic.message.contains("assignment inference limit")
        }),
        "{:#?}",
        result.diagnostics
    );

    let boundary = format!("{}value", "value ||= ".repeat(128));
    let source = format!("let value: number = 1; const selected = {boundary};");
    let accepted = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new("memory:///main.ts", &source)]),
        CompilerOptions::default(),
    );
    assert!(!accepted.has_errors(), "{:#?}", accepted.diagnostics);
}
