// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Private operations preserve their individual Native source phases.
use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};

const ENTRY: &str = "memory:///main.ts";
const SOURCE: &str = "export class Box { #value: number = 40; get value(): number { return this.#value; } set value(value: number) { this.#value = value; } has(other: object): boolean { return #value in other; } }\nconst box = new Box(); box.value = 42;\nexport const result: number = box.has(box) ? box.value : 0;\n";

fn coordinates(text: &str, offset: usize) -> (usize, usize) {
    let before = &text[..offset];
    (
        before.bytes().filter(|byte| *byte == b'\n').count(),
        before.rsplit('\n').next().unwrap().encode_utf16().count(),
    )
}

#[test]
fn private_fields_and_operations_keep_complete_native_source_phases() {
    for source in [
        SOURCE.to_string(),
        SOURCE.replace('\n', "\r\n"),
        format!("/* G-E5 🧭 */ {}", SOURCE.replace('\n', "\r\n")),
    ] {
        for target in [
            EcmaTarget::Es2015,
            EcmaTarget::Es2017,
            EcmaTarget::Es2018,
            EcmaTarget::Es2022,
        ] {
            let compilation = compile(
                ENTRY,
                &MapLoader::from([ModuleSource::new(ENTRY, &source)]),
                CompilerOptions {
                    target,
                    module_kind: ModuleKind::CommonJs,
                    source_map: true,
                    inline_sources: true,
                    use_define_for_class_fields: Some(false),
                    libraries: Some(vec![EcmaTarget::Es2022]),
                    ..CompilerOptions::default()
                },
            );
            assert!(!compilation.has_errors(), "{:?}", compilation.diagnostics);
            let artifact = &compilation.output.unwrap().artifacts[ENTRY];
            let code = &artifact.javascript;
            let map = artifact.source_map.as_ref().unwrap();
            let check = |from: usize, end: usize, original: usize, label: &str| {
                for offset in from..end {
                    let (line, column) = coordinates(code, offset);
                    let owner = map
                        .original_position_at(code, line, column, 100_000, 32_000_000)
                        .unwrap()
                        .map(|owner| (owner.line, owner.column_utf16));
                    assert_eq!(
                        owner,
                        Some(coordinates(&source, original)),
                        "{target:?}: {label} at {offset}"
                    );
                }
            };
            let field = source.find("#value:").unwrap();
            let name_end = code.find("class Box").unwrap() + "class Box".len();
            let original_name_end = source.find("class Box").unwrap() + "class Box".len();
            check(
                name_end,
                name_end + 1,
                original_name_end,
                "private class binding endpoint",
            );
            let body_open_end = code.find("class Box {").unwrap() + "class Box {".len();
            check(
                body_open_end,
                body_open_end + 1,
                original_name_end,
                "private class body opener endpoint",
            );
            let class_end = code.find("} exports.Box").unwrap() + 1;
            let original_end = source.find(" } }").unwrap() + 4;
            check(
                class_end,
                class_end + 1,
                original_end,
                "private class completion before allocation",
            );
            if target < EcmaTarget::Es2022 {
                let store = code.find("_Box_value.set(this, 40)").unwrap();
                let initializer = store + "_Box_value.set(this, ".len();
                check(store, initializer, field, "private field store prefix");
                let value = source.find("40;").unwrap();
                check(initializer, initializer + 2, value, "private field value");
                check(
                    initializer + 2,
                    initializer + 4,
                    value + 2,
                    "private field end",
                );

                let get = code
                    .rfind("__bluetsClassPrivateGet(this, _Box_value, \"f\")")
                    .unwrap();
                let receiver = source.find("this.#value;").unwrap();
                let get_argument = get + "__bluetsClassPrivateGet(".len();
                check(
                    get,
                    get_argument + 4,
                    receiver,
                    "private read receiver phase",
                );
                check(
                    get_argument + 4,
                    get_argument + 22,
                    receiver + 4,
                    "private read transport phase",
                );

                let set = code
                    .rfind("__bluetsClassPrivateSet(this, _Box_value, value, \"f\")")
                    .unwrap();
                let receiver = source.find("this.#value =").unwrap();
                let set_argument = set + "__bluetsClassPrivateSet(".len();
                check(
                    set,
                    set_argument + 4,
                    receiver,
                    "private write receiver phase",
                );
                let rhs = set_argument + "this, _Box_value, ".len();
                check(
                    set_argument + 4,
                    rhs,
                    receiver + 4,
                    "private write state transport",
                );
                let original_rhs = source.find("= value;").unwrap() + 2;
                check(rhs, rhs + 5, original_rhs, "private write original value");
                check(
                    rhs + 5,
                    rhs + 11,
                    original_rhs + 5,
                    "private write completion",
                );

                let membership = code
                    .rfind("__bluetsClassPrivateIn(_Box_value, other)")
                    .unwrap();
                let other = membership + "__bluetsClassPrivateIn(_Box_value, ".len();
                let returned = source.find("return #value").unwrap();
                check(
                    membership,
                    other,
                    returned,
                    "private-in inherited return phase",
                );
                let original_other = source.find("in other;").unwrap() + 3;
                check(other, other + 5, original_other, "private-in operand");
                check(
                    other + 5,
                    other + 6,
                    original_other + 5,
                    "private-in completion",
                );
            } else {
                let original: Vec<_> = source.match_indices("#value").map(|(at, _)| at).collect();
                let generated: Vec<_> = code.match_indices("#value").map(|(at, _)| at).collect();
                assert_eq!(generated.len(), original.len());
                for (from, original) in generated.into_iter().zip(original) {
                    check(from, from + 6, original, "complete retained private name");
                }
                let generated = code.find(" in other").unwrap() + 1;
                let original = source.find("#value in other").unwrap() + 6;
                check(
                    generated,
                    generated + 3,
                    original,
                    "retained private-in operator and gap",
                );
            }
        }
    }
}
