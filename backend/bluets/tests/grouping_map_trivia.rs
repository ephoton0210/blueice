// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Formatter-inserted trivia cannot acquire the following grouping's owner.
use blueice_bluets::{
    compile, CompilerOptions, EcmaTarget, JavaScriptLayout, MapLoader, ModuleKind, ModuleSource,
    NewLine,
};
use serde_json::{json, Value};

#[allow(dead_code)]
#[path = "oracle_support/gd2_maps.rs"]
mod maps;

fn owner(code: &str, decoded: &Value, offset: usize) -> Value {
    let prefix = &code[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let column = prefix.rsplit('\n').next().unwrap().encode_utf16().count();
    decoded["segments"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|point| point[0] == line && point[1].as_u64().unwrap() <= column as u64)
        .filter(|point| point.as_array().unwrap().len() > 2)
        .map(|point| json!(point.as_array().unwrap()[2..].to_vec()))
        .unwrap_or(Value::Null)
}

#[test]
fn compact_assignment_groupings_keep_the_native_separator_and_operand_owners() {
    for prefix in ["", "/* exports 🧭 */\r\n"] {
        let source = format!("{prefix}const state:any=globalThis;state.ge3Provided=(state.ge3Provided||0)+1;\r\nexport class Model {{ value:number=17; }}\r\nexport {{ Model as Alias }};\r\n");
        let line = usize::from(!prefix.is_empty());
        for target in [EcmaTarget::Es5, EcmaTarget::Es2022] {
            for kind in [ModuleKind::CommonJs, ModuleKind::Esm] {
                for newline in [NewLine::Lf, NewLine::CrLf] {
                    let id = "memory:///types.ts";
                    let result = compile(
                        id,
                        &MapLoader::from([ModuleSource::new(id, &source)]),
                        CompilerOptions {
                            target,
                            libraries: Some(vec![EcmaTarget::Es2022]),
                            module_kind: kind,
                            source_map: true,
                            javascript_layout: JavaScriptLayout::TypeScript,
                            new_line: newline,
                            ..CompilerOptions::default()
                        },
                    );
                    assert!(!result.has_errors(), "{:?}", result.diagnostics);
                    let artifact = &result.output.as_ref().unwrap().artifacts[id];
                    let code = &artifact.javascript;
                    let decoded = maps::decoded(&artifact.source_map.as_ref().unwrap().to_json());
                    let space = code.find(" = (").unwrap() + 2;
                    // Pinned TypeScript5.9.3 maps the added separator to the
                    // assignment's preceding name end; the grouping starts
                    // only at `(`, followed by the operand's own token entry.
                    for (delta, column) in [(0, 44), (1, 45), (2, 46)] {
                        assert_eq!(
                            owner(code, &decoded, space + delta),
                            json!([0, line, column]),
                            "grouping offset +{delta}, {target:?}/{kind:?}/{newline:?}: {code}"
                        );
                    }
                }
            }
        }
    }
}
