// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The public enum export boundary preserves its modifier end and full scaffold.
use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn enum_exports_retain_native_modifier_and_scaffold_endpoints() {
    let mut rows = Vec::new();
    let mut parity = Vec::new();
    for prefix in ["", "/* enum 🧭 */\r\n"] {
        let source=format!("{prefix}const state:any=globalThis;state.ge3Provided=(state.ge3Provided||0)+1;\r\nexport enum Numeric {{ A }}\r\nexport enum Text {{ A='a' }}\r\n");
        for target in [EcmaTarget::Es5, EcmaTarget::Es2022, EcmaTarget::EsNext] {
            for module_kind in [ModuleKind::Esm, ModuleKind::CommonJs] {
                let mut outputs = Vec::new();
                for maps in [false, true] {
                    let id = "memory:///main.ts";
                    let result = compile(
                        id,
                        &MapLoader::from([ModuleSource::new(id, &source)]),
                        CompilerOptions {
                            target,
                            module_kind,
                            source_map: maps,
                            inline_sources: true,
                            libraries: Some(vec![EcmaTarget::Es2022]),
                            ..CompilerOptions::default()
                        },
                    );
                    assert!(!result.has_errors(), "{:?}", result.diagnostics);
                    let artifact = &result.output.unwrap().artifacts[id];
                    outputs.push(
                        artifact
                            .javascript
                            .lines()
                            .filter(|line| !line.starts_with("//# sourceMappingURL="))
                            .collect::<Vec<_>>()
                            .join("\n"),
                    );
                    if maps {
                        rows.push(serde_json::json!({"source":source,"target":match target {EcmaTarget::Es5=>"ES5",EcmaTarget::Es2022=>"ES2022",_=>"ESNext"},"module":if module_kind==ModuleKind::CommonJs{"CommonJS"}else{"ESNext"},"code":artifact.javascript,"raw":artifact.source_map.as_ref().unwrap().to_json()}));
                    }
                }
                parity.push(outputs[0] == outputs[1]);
            }
        }
    }
    let script=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../development/browser_core/phase-18-bluets/tools/verify_ge5_enum_export_boundaries.cjs");
    let mut child = Command::new("node")
        .args(["--experimental-vm-modules"])
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
    eprintln!("{}", String::from_utf8_lossy(&output.stdout));
    assert!(
        parity.into_iter().all(|same| same),
        "enum map/no-map executable bytes"
    );
}

#[test]
fn enum_indentation_preserves_nonexport_const_and_namespace_map_modes() {
    for prefix in ["", "/* enum 🧭 */\r\n"] {
        let source = format!("{prefix}enum Local {{ A, B }}\r\nconst enum Inline {{ X=2 }}\r\nnamespace Box {{ export enum Inside {{ A, B }} }}\r\nexport const values=[Local.B,Inline.X,Box.Inside.B];\r\n");
        for target in [EcmaTarget::Es5, EcmaTarget::Es2022, EcmaTarget::EsNext] {
            for module_kind in [ModuleKind::Esm, ModuleKind::CommonJs] {
                let mut outputs = Vec::new();
                for maps in [false, true] {
                    let id = "memory:///main.ts";
                    let result = compile(
                        id,
                        &MapLoader::from([ModuleSource::new(id, &source)]),
                        CompilerOptions {
                            target,
                            module_kind,
                            source_map: maps,
                            libraries: Some(vec![EcmaTarget::Es2022]),
                            ..CompilerOptions::default()
                        },
                    );
                    assert!(!result.has_errors(), "{:?}", result.diagnostics);
                    outputs.push(
                        result.output.unwrap().artifacts[id]
                            .javascript
                            .lines()
                            .filter(|line| !line.starts_with("//# sourceMappingURL="))
                            .collect::<Vec<_>>()
                            .join("\n"),
                    );
                }
                assert_eq!(
                    outputs[0], outputs[1],
                    "nonexport/const/namespace enum map-mode text"
                );
            }
        }
    }
}
