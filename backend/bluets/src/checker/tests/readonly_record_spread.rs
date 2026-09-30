// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn record_spreads_keep_nested_readonly_event_references() {
    let source = "interface Event { readonly type: 'click'; readonly target: string; readonly currentTarget: string; }\n\
                  interface Holder { event: Event; }\n\
                  type HolderAlias = Holder;\n\
                  interface Container { holder: Holder; }\n\
                  function identity(holder: Holder): Holder { return holder; }\n\
                  function write(holder: Holder, named: HolderAlias, container: Container): void {\n\
                    const direct = { ...holder };\n\
                    direct.event.type = 'click';\n\
                    const alias = { ...named };\n\
                    alias.event.target = 'other';\n\
                    const member = { ...container.holder };\n\
                    member.event.currentTarget = 'other';\n\
                    const nested = { ...({ picked: holder.event }) };\n\
                    nested.picked.type = 'click';\n\
                    const called = { ...identity(holder) };\n\
                    called.event.type = 'click';\n\
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
    assert_eq!(readonly, 5, "{:#?}", result.diagnostics);
    assert_eq!(result.diagnostics.len(), 5, "{:#?}", result.diagnostics);

    let valid = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "interface Event { readonly type: 'click'; } interface MutableEvent { type: string; } interface Holder { event: Event; } interface MutableHolder { event: MutableEvent; } interface ReadonlyLabel { readonly label: string; } function write(holder: Holder, mutable: MutableHolder, label: ReadonlyLabel): void { const overridden = { ...holder, event: mutable.event }; overridden.event.type = 'ok'; const copied = { ...label }; copied.label = 'ok'; }",
        )]),
        CompilerOptions::default(),
    );
    assert!(!valid.has_errors(), "{:#?}", valid.diagnostics);
}
