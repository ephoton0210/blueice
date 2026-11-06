// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Legal decorator/constructor phases retain their complete System construction chains.
use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn all_legal_system_legacy_constructor_replacement_phases_match_native() {
    let mut rows = Vec::new();
    for decorators in ["class-only", "ctor-param-only", "member-only", "none"] {
        for constructor in ["implicit", "empty", "value"] {
            if decorators == "ctor-param-only" && constructor != "value" {
                continue; // A parameter decorator requires an actual source parameter.
            }
            for metadata in [false, true] {
                for target in [
                    EcmaTarget::Es5,
                    EcmaTarget::Es2015,
                    EcmaTarget::Es2022,
                    EcmaTarget::EsNext,
                ] {
                    for define in [false, true] {
                        for astral in [false, true] {
                            let ctor = match constructor {
                                "implicit" => String::new(),
                                "empty" => "constructor(){}".into(),
                                _ => format!(
                                    "constructor({}value:number){{void value;}}",
                                    if decorators == "ctor-param-only" {
                                        "@dec "
                                    } else {
                                        ""
                                    }
                                ),
                            };
                            let mut source=format!("function dec(...args:any[]):any{{}}\n{}export class C {{ {}method(value:number):number{{return value;}} {ctor} }}\n",if decorators=="class-only"{"@dec "}else{""},if decorators=="member-only"{"@dec "}else{""});
                            if astral {
                                source =
                                    format!("/* phase 🧭 */\r\n{}", source.replace('\n', "\r\n"));
                            }
                            let id = "memory:///main.ts";
                            let options = CompilerOptions {
                                target,
                                module_kind: ModuleKind::System,
                                experimental_decorators: true,
                                emit_decorator_metadata: metadata,
                                use_define_for_class_fields: Some(define),
                                source_map: true,
                                inline_sources: true,
                                libraries: Some(vec![EcmaTarget::Es2022]),
                                ..CompilerOptions::default()
                            };
                            let loader = MapLoader::from([ModuleSource::new(id, &source)]);
                            let result = compile(id, &loader, options.clone());
                            assert!(!result.has_errors(), "{:?}", result.diagnostics);
                            let artifact = &result.output.unwrap().artifacts[id];
                            let plain = compile(
                                id,
                                &loader,
                                CompilerOptions {
                                    source_map: false,
                                    ..options
                                },
                            );
                            assert!(!plain.has_errors(), "{:?}", plain.diagnostics);
                            let plain = &plain.output.unwrap().artifacts[id];
                            let executable = |code: &str| {
                                code.lines()
                                    .filter(|line| !line.starts_with("//# sourceMappingURL="))
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            };
                            assert_eq!(
                                executable(&artifact.javascript),
                                executable(&plain.javascript),
                                "System phase map/no-map text parity"
                            );
                            rows.push(serde_json::json!({"source":source,"target":match target {EcmaTarget::Es5=>"ES5",EcmaTarget::Es2015=>"ES2015",EcmaTarget::Es2022=>"ES2022",_=>"ESNext"},"decorators":decorators,"constructor":constructor,"metadata":metadata,"define":define,"astral":astral,"code":artifact.javascript,"raw":artifact.source_map.as_ref().unwrap().to_json()}));
                        }
                    }
                }
            }
        }
    }
    assert_eq!(rows.len(), 320);
    if let Some(path) = std::env::var_os("BLUEICE_GE5_SYSTEM_PHASE_OBSERVATIONS") {
        std::fs::write(path, serde_json::to_vec(&rows).unwrap()).unwrap();
    }
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../development/browser_core/phase-18-bluets/tools/verify_ge5_system_legacy_phases.cjs",
    );
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
    eprintln!("{}", String::from_utf8_lossy(&output.stdout));
}
