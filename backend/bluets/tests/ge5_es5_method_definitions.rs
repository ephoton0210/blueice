// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ES5 method descriptors and function names follow the selected Native mode.
use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn es5_methods_keep_native_enumerability_and_function_names_in_both_map_modes() {
    let mut rows = Vec::new();
    for prefix in ["", "/* G-E5 🧭 */\r\n"] {
        let source = format!("{prefix}export class C {{ plain():number{{return 1;}} 'a-b'():number{{return 2;}} 0x10():number{{return 3;}} static staticMethod():number{{return 4;}} }}\n");
        for define in [false, true] {
            for maps in [false, true] {
                let id = "memory:///main.ts";
                let result = compile(
                    id,
                    &MapLoader::from([ModuleSource::new(id, &source)]),
                    CompilerOptions {
                        target: EcmaTarget::Es5,
                        module_kind: ModuleKind::CommonJs,
                        use_define_for_class_fields: Some(define),
                        source_map: maps,
                        inline_sources: true,
                        ..CompilerOptions::default()
                    },
                );
                assert!(!result.has_errors(), "{:?}", result.diagnostics);
                let artifact = &result.output.unwrap().artifacts[id];
                rows.push(
                    serde_json::json!({"source":source,"define":define,"maps":maps,
                    "code":artifact.javascript,"raw":artifact.source_map.as_ref().map(|map|map.to_json())}),
                );
            }
        }
    }
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../development/browser_core/phase-18-bluets/tools/verify_ge5_es5_method_definitions.cjs");
    let mut child = Command::new("node")
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&rows).unwrap())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
