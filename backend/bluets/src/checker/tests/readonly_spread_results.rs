// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn tuple_spread_results_keep_possible_readonly_event_elements() {
    let source = "interface Event { readonly type: 'click'; readonly target: string; readonly currentTarget: string; }\n\
                  interface MutableEvent { type: string; target: string; currentTarget: string; }\n\
                  type EventPair = [Event, MutableEvent];\n\
                  function write(pair: [Event, MutableEvent], named: EventPair): void {\n\
                    const direct = [...pair][0];\n\
                    direct.type = 'click';\n\
                    const alias = [...named][0];\n\
                    alias.target = 'other';\n\
                    ([...pair][0]).currentTarget = 'other';\n\
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
    assert_eq!(readonly, 3, "{:#?}", result.diagnostics);
    assert_eq!(result.diagnostics.len(), 3, "{:#?}", result.diagnostics);

    let valid = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "interface MutableEvent { type: string; } type Pair = [MutableEvent, MutableEvent]; function write(pair: Pair): void { const value = [...pair][0]; value.type = 'ok'; ([...pair][0]).type = 'ok'; }",
        )]),
        CompilerOptions::default(),
    );
    assert!(!valid.has_errors(), "{:#?}", valid.diagnostics);
}
