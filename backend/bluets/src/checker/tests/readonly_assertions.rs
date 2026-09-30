// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn erased_assertions_keep_readonly_member_qualifiers() {
    let source = "interface Event { readonly type: 'click'; }\n\
                  interface Holder { event: Event; type: string; }\n\
                  function write(holder: Holder): void {\n\
                    const asserted = holder.event as Event;\n\
                    asserted.type = 'click';\n\
                    const checked = holder.event satisfies Event;\n\
                    checked.type = 'click';\n\
                    const boxed = { picked: holder.event as Event };\n\
                    boxed.picked.type = 'click';\n\
                    const listed = [holder.event satisfies Event][0];\n\
                    listed.type = 'click';\n\
                    (holder.event as Event).type = 'click';\n\
                    const recast = holder.event as { type: string };\n\
                    recast.type = 'click';\n\
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
    assert_eq!(readonly, 6, "{:#?}", result.diagnostics);
    assert_eq!(result.diagnostics.len(), 6, "{:#?}", result.diagnostics);

    let valid = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "interface Holder { type: string; as: string; satisfies: string; } function write(holder: Holder): void { const asserted = holder as Holder; asserted.type = 'ok'; const checked = holder satisfies Holder; checked.type = 'ok'; const a: string = holder.as + '!'; const b: string = holder.satisfies + '!'; }",
        )]),
        CompilerOptions::default(),
    );
    assert!(!valid.has_errors(), "{:#?}", valid.diagnostics);
}
