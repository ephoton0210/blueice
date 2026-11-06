// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Real provider, forwarding barrel and consumer retain local export key identity.
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn local_export_keys_preserve_real_three_module_products() {
    let directory = std::env::temp_dir().join(format!("ge5-local-export-{}", std::process::id()));
    std::fs::create_dir(&directory).unwrap();
    let mut rows = Vec::new();
    for prefix in ["", "/* exports 🧭 */\r\n"] {
        for key in ["Alias", "default", "\"x-y\""] {
            let files = serde_json::json!({"main.ts":format!("{prefix}export {{ Renamed as Result }} from './barrel.js';\r\n"),"barrel.ts":format!("{prefix}export {{ {key} as Renamed }} from './types.js';\r\n"),"types.ts":format!("{prefix}const state:any=globalThis;state.ge3Provided=(state.ge3Provided||0)+1;\r\nexport class Model {{ value:number=17; }}\r\nexport {{ Model as {key} }};\r\n")});
            for (name, text) in files.as_object().unwrap() {
                std::fs::write(directory.join(name), text.as_str().unwrap()).unwrap();
            }
            for target in ["ES5", "ES2015", "ES2017", "ES2022", "ESNext"] {
                for module in ["CommonJS", "ESNext"] {
                    let mut modes = Vec::new();
                    for maps in [false, true] {
                        let options = serde_json::json!({"target":target,"module":module,"sourceMap":maps,"inlineSources":maps,"declaration":true,"declarationMap":maps,"outDir":"out","declarationDir":"types","rootDir":".","noEmitOnError":true,"useDefineForClassFields":false,"lib":["ES2022"]});
                        std::fs::write(
                            directory.join("tsconfig.json"),
                            serde_json::to_vec(
                                &serde_json::json!({"compilerOptions":options,"files":["main.ts"]}),
                            )
                            .unwrap(),
                        )
                        .unwrap();
                        let mut outputs = serde_json::Map::new();
                        for build in 0..2 {
                            for folder in ["out", "types"] {
                                let path = directory.join(folder);
                                if path.exists() {
                                    std::fs::remove_dir_all(path).unwrap();
                                }
                            }
                            let result = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
                                .arg("--project")
                                .arg(directory.join("tsconfig.json"))
                                .output()
                                .unwrap();
                            assert!(
                                result.status.success(),
                                "{}",
                                String::from_utf8_lossy(&result.stderr)
                            );
                            let mut found = serde_json::Map::new();
                            for name in ["main", "barrel", "types"] {
                                for (folder, suffix) in [("out", "js"), ("types", "d.ts")] {
                                    let filename = format!("{folder}/{name}.{suffix}");
                                    found.insert(
                                        filename.clone(),
                                        std::fs::read_to_string(directory.join(&filename))
                                            .unwrap()
                                            .into(),
                                    );
                                    if maps {
                                        let filename = format!("{filename}.map");
                                        found.insert(
                                            filename.clone(),
                                            std::fs::read_to_string(directory.join(&filename))
                                                .unwrap()
                                                .into(),
                                        );
                                    }
                                }
                            }
                            if build == 0 {
                                outputs = found;
                            } else {
                                assert_eq!(outputs, found, "repeated CLI products");
                            }
                        }
                        modes.push(
                            outputs
                                .iter()
                                .filter(|(name, _)| name.ends_with(".js"))
                                .map(|(name, text)| {
                                    (
                                        name.clone(),
                                        text.as_str()
                                            .unwrap()
                                            .lines()
                                            .take_while(|line| {
                                                !line.starts_with("//# sourceMappingURL=")
                                            })
                                            .collect::<Vec<_>>()
                                            .join("\n")
                                            .trim_end_matches('\n')
                                            .to_string(),
                                    )
                                })
                                .collect::<std::collections::BTreeMap<_, _>>(),
                        );
                        rows.push(serde_json::json!({"files":files,"options":options,"outputs":outputs,"key":key,"repeatBuilds":2}));
                    }
                    assert_eq!(
                        modes[0], modes[1],
                        "all real module executable bytes map/noMap"
                    );
                }
            }
        }
    }
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../development/browser_core/phase-18-bluets/tools/verify_ge5_local_string_exports.cjs",
    );
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
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    eprintln!("{}", String::from_utf8_lossy(&output.stdout));
    std::fs::remove_dir_all(directory).unwrap();
}
