// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn simple_assignment_result_retains_the_right_hand_readonly_receiver() {
    let source = "interface Event { readonly type: 'click'; }\n\
                  interface MutableEvent { type: string; }\n\
                  interface Holder { slot: Event; backup: Event; event: MutableEvent; }\n\
                  interface Source { event: Event; }\n\
                  function write(holder: Holder, source: Source): void {\n\
                    const selected = (holder.slot = source.event);\n\
                    selected.type = 'click';\n\
                    (holder.slot = source.event).type = 'click';\n\
                    const boxed = { selected: (holder.slot = source.event) };\n\
                    boxed.selected.type = 'click';\n\
                    const listed = [(holder.slot = source.event)][0];\n\
                    listed.type = 'click';\n\
                    const chained = (holder.slot = holder.backup = source.event);\n\
                    chained.type = 'click';\n\
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
            "interface MutableEvent { type: string; } interface Holder { slot: MutableEvent; event: MutableEvent; } interface Source { event: MutableEvent; } function write(holder: Holder, source: Source): void { const selected = (holder.slot = source.event); selected.type = 'ok'; }",
        )]),
        CompilerOptions::default(),
    );
    assert!(!valid.has_errors(), "{:#?}", valid.diagnostics);
}
