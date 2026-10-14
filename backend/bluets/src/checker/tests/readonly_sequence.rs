// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn sequence_result_keeps_the_final_readonly_receiver() {
    let source = "interface Event { readonly type: 'click'; }\n\
                  interface MutableEvent { type: string; }\n\
                  interface Holder { event: MutableEvent; }\n\
                  interface Source { event: Event; }\n\
                  function mark(holder: Holder, source: Source): void {}\n\
                  function getEvent(source: Source): Event { return source.event; }\n\
                  function write(holder: Holder, source: Source): void {\n\
                    const selected = (holder, source.event);\n\
                    selected.type = 'click';\n\
                    (holder, source.event).type = 'click';\n\
                    const listed = [(holder, source.event)][0];\n\
                    listed.type = 'click';\n\
                    const called = (mark(holder, source), source.event);\n\
                    called.type = 'click';\n\
                    const returned = (holder, getEvent(source));\n\
                    returned.type = 'click';\n\
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
            "interface Event { readonly type: 'click'; } interface MutableEvent { type: string; } interface Holder { event: MutableEvent; } function write(event: Event, holder: Holder): void { const selected = (event, holder.event); selected.type = 'ok'; }",
        )]),
        CompilerOptions::default(),
    );
    assert!(!valid.has_errors(), "{:#?}", valid.diagnostics);
}
