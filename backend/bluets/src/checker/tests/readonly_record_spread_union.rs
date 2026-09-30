// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn union_record_spreads_retain_every_reachable_readonly_event() {
    let source = "interface Event { readonly type: 'click'; readonly target: string; readonly currentTarget: string; }\n\
                  interface MutableEvent { type: string; target: string; currentTarget: string; }\n\
                  interface ReadonlyHolder { event: Event; }\n\
                  interface MutableHolder { event: MutableEvent; }\n\
                  type Holder = ReadonlyHolder | MutableHolder;\n\
                  function write(holder: Holder): void {\n\
                    const copied = { ...holder };\n\
                    copied.event.type = 'click';\n\
                    ({ ...holder }).event.target = 'other';\n\
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
    assert_eq!(readonly, 2, "{:#?}", result.diagnostics);
    assert_eq!(result.diagnostics.len(), 2, "{:#?}", result.diagnostics);

    let valid = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "interface Event { readonly type: 'click'; } interface MutableEvent { type: string; } interface ReadonlyHolder { event: Event; } interface First { event: MutableEvent; } interface Second { event: MutableEvent; } type MutableHolder = First | Second; function write(source: MutableHolder, readonlyHolder: ReadonlyHolder, mutable: First): void { const copied = { ...source }; copied.event.type = 'ok'; const overridden = { ...readonlyHolder, event: mutable.event }; overridden.event.type = 'ok'; }",
        )]),
        CompilerOptions::default(),
    );
    assert!(!valid.has_errors(), "{:#?}", valid.diagnostics);
}

#[test]
fn optional_union_spread_cannot_erase_an_earlier_readonly_event() {
    let source = "interface Event { readonly type: 'click'; }\n\
                  interface MutableEvent { type: string; }\n\
                  interface ReadonlyHolder { event: Event; }\n\
                  interface MutableHolder { event: MutableEvent; }\n\
                  interface Label { label: string; }\n\
                  type MaybeEvent = MutableHolder | Label;\n\
                  function write(readonlyHolder: ReadonlyHolder, maybe: MaybeEvent): void {\n\
                    const copied = { ...readonlyHolder, ...maybe };\n\
                    copied.event.type = 'click';\n\
                  }";
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
        CompilerOptions::default(),
    );
    assert!(
        result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::TypeMismatch
                && diagnostic.message.contains("readonly")
        }),
        "{:#?}",
        result.diagnostics
    );

    let absent = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "interface Event { readonly type: 'click'; } interface ReadonlyHolder { event: Event; } interface Label { label: string; } type MaybeEvent = ReadonlyHolder | Label; function write(maybe: MaybeEvent): void { const copied = { ...maybe }; copied.event.type = 'click'; }",
        )]),
        CompilerOptions::default(),
    );
    assert!(
        absent
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch),
        "{:#?}",
        absent.diagnostics
    );
}

#[test]
fn union_record_spread_expansion_limit_fails_closed() {
    let variants = vec!["{ event: Event }"; 257].join(" | ");
    let source = format!(
        "interface Event {{ readonly type: 'click'; }} type Many = {variants}; function write(many: Many): void {{ const copied = {{ ...many }}; copied.event.type = 'click'; }}"
    );
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new("memory:///main.ts", &source)]),
        CompilerOptions::default(),
    );
    assert!(
        result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::ResourceLimit
                && diagnostic.message.contains("record spread")
        }),
        "{:#?}",
        result.diagnostics
    );
}
