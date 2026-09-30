// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

fn compile_main(source: &str) -> crate::Compilation {
    crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
        CompilerOptions::default(),
    )
}

#[test]
fn opaque_receiver_touching_a_readonly_value_is_rejected() {
    // `pick` has no return annotation the checker can model, so inference
    // loses the receiver type. The readonly event flows in as an argument,
    // so the write cannot be proven safe and must fail closed.
    let source = "interface Event { readonly type: 'click'; }\n\
                  declare function pick(value: unknown): unknown;\n\
                  function direct(event: Event): void {\n\
                    pick(event).type = 'click';\n\
                  }\n\
                  function nested(event: Event): void {\n\
                    const list = [pick(event).type = 'click'];\n\
                  }\n\
                  function alias(events: Event[]): void {\n\
                    const chosen = pick(events);\n\
                    chosen.type = 'click';\n\
                  }";
    let result = compile_main(source);
    let opaque = result
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.message.contains("cannot prove"))
        .count();
    assert!(opaque >= 3, "{:#?}", result.diagnostics);
}

#[test]
fn opaque_receiver_unrelated_to_readonly_values_is_still_allowed() {
    let source = "interface Event { readonly type: 'click'; }\n\
                  declare function pick(value: unknown): unknown;\n\
                  function write(count: number): void {\n\
                    pick(count).total = 1;\n\
                  }";
    let result = compile_main(source);
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
}
