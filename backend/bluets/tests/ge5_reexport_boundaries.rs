// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The public CLI forwards original identifier, keyword and string-named keys.
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn forwarding_identifier_keyword_and_string_names_preserve_maps_and_live_getters() {
    observe(
        &["CommonJS", "ESNext"],
        "verify_ge5_reexport_boundaries.cjs",
    );
}

#[test]
fn system_named_forwarding_retains_the_native_generated_phase() {
    observe(&["System"], "verify_ge5_system_forwarding.cjs");
}

fn observe(modules: &[&str], verifier: &str) {
    let directory = std::env::temp_dir().join(format!(
        "ge5-reexport-boundary-{}-{}",
        std::process::id(),
        modules.join("-")
    ));
    std::fs::create_dir(&directory).unwrap();
    // Provider output is outside this component boundary. The runtime adapter
    // supplies explicit external values; frozen families execute every real module.
    std::fs::write(directory.join("types.ts"),"export class Model { value=1; }\nexport default class Default { value=2; }\nclass Odd { value=3; }\nexport { Odd as \"x-y\" };\n").unwrap();
    let mut rows = Vec::new();
    for prefix in ["", "/* forward 🧭 */\r\n"] {
        for imported in ["Model", "default", "\"x-y\""] {
            let source =
                format!("{prefix}export {{ {imported} as Forwarded }} from './types.js';\r\n");
            std::fs::write(directory.join("barrel.ts"), &source).unwrap();
            for target in ["ES5", "ES2015", "ES2017", "ES2022", "ESNext"] {
                for module in modules {
                    let mut bodies = Vec::new();
                    for maps in [false, true] {
                        let output = directory.join("out");
                        if output.exists() {
                            std::fs::remove_dir_all(&output).unwrap();
                        }
                        std::fs::write(directory.join("tsconfig.json"),serde_json::to_vec(&serde_json::json!({"compilerOptions":{"target":target,"module":module,"sourceMap":maps,"inlineSources":maps,"outDir":"out","rootDir":".","noEmitOnError":true},"files":["barrel.ts"]})).unwrap()).unwrap();
                        let compiler = if modules == ["System"] {
                            std::env::var_os("BLUEICE_GE5_SYSTEM_FORWARDING_CLI")
                                .unwrap_or_else(|| env!("CARGO_BIN_EXE_bluetsc").into())
                        } else {
                            env!("CARGO_BIN_EXE_bluetsc").into()
                        };
                        let result = Command::new(compiler)
                            .arg("--project")
                            .arg(directory.join("tsconfig.json"))
                            .output()
                            .unwrap();
                        assert!(
                            result.status.success(),
                            "{}",
                            String::from_utf8_lossy(&result.stderr)
                        );
                        let code = std::fs::read_to_string(output.join("barrel.js")).unwrap();
                        bodies.push(
                            code.lines()
                                .take_while(|line| !line.starts_with("//# sourceMappingURL="))
                                .collect::<Vec<_>>()
                                .join("\n")
                                .trim_end_matches('\n')
                                .to_string(),
                        );
                        if maps {
                            rows.push(serde_json::json!({"source":source,"target":target,"module":module,"code":code,"raw":std::fs::read_to_string(output.join("barrel.js.map")).unwrap()}));
                        }
                    }
                    assert_eq!(bodies[0], bodies[1], "map/noMap forwarding text");
                }
            }
        }
    }
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools");
    let script = script.join(verifier);
    let mut child = Command::new("node")
        .arg("--experimental-vm-modules")
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
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    eprintln!("{}", String::from_utf8_lossy(&result.stdout));
    std::fs::remove_dir_all(directory).unwrap();
}
