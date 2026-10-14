// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A JSON object's own `default` property does not replace its module binding.

use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::{env, fs};

fn report(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn primary(output: &Output) -> String {
    report(output)
        .lines()
        .find(|line| line.contains(": error TS"))
        .unwrap()
        .to_string()
}

fn declaration(root: &Path) -> String {
    fs::read_to_string(root.join("main.d.ts"))
        .unwrap()
        .replace("\r\n", "\n")
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Node"]
fn json_default_property_keeps_pinned_inference_execution_and_assignment_diagnostic() {
    let tsc = PathBuf::from(env::var_os("BLUEICE_BLUETSC_ORACLE").unwrap());
    let version = Command::new(&tsc).arg("--version").output().unwrap();
    assert!(version.status.success(), "{}", report(&version));
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        "Version 5.9.3"
    );
    let root = fs::canonicalize(env::temp_dir()).unwrap().join(format!(
        "bluets-json-default-binding-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("data.json"),
        "{\"default\":42,\"nested\":{\"value\":7}}\n",
    )
    .unwrap();
    fs::write(root.join("package.json"), "{\"type\":\"module\"}\n").unwrap();
    let mut controls = Vec::new();
    for (name, source, accepts) in [
        ("inferred", "import data from './data.json' with {type:'json'}; export const answer = data.default; console.log(answer);", true),
        ("wrong-field", "import data from './data.json' with {type:'json'}; export const wrong: string = data.default;", false),
    ] {
        let input = root.join(name);
        fs::create_dir_all(&input).unwrap();
        fs::copy(root.join("data.json"), input.join("data.json")).unwrap();
        fs::write(input.join("main.ts"), source).unwrap();
        let blue = input.join("blue");
        let reference = input.join("reference");
        let config = input.join("tsconfig.json");
        fs::write(&config, json!({"compilerOptions":{
            "target":"ES2022", "module":"ESNext", "moduleResolution":"bundler",
            "strict":true, "resolveJsonModule":true, "declaration":true,
            "noEmitOnError":true, "pretty":false, "outDir":blue
        },"files":["main.ts"]}).to_string()).unwrap();
        let built = Command::new(&tsc).arg("--project").arg(&config)
            .arg("--outDir").arg(&reference).current_dir(&input).output().unwrap();
        assert_eq!(built.status.success(), accepts, "{name}: {}", report(&built));
        let expected = if accepts {
            let run = Command::new("node").arg(reference.join("main.js")).output().unwrap();
            assert!(run.status.success(), "{}", report(&run));
            assert_eq!(run.stdout, b"42\n");
            let text = declaration(&reference);
            assert_eq!(text, "export declare const answer: number;\n");
            (Some(text), None)
        } else {
            let error = primary(&built);
            assert!(error.contains("error TS2322:"), "{error}");
            assert!(!reference.exists());
            (None, Some(error))
        };
        controls.push((name, input, config, blue, accepts, expected));
    }
    let mut failures = Vec::new();
    for (name, input, config, blue, accepts, expected) in controls {
        let built = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .arg("--project")
            .arg(config)
            .current_dir(input)
            .output()
            .unwrap();
        if built.status.success() != accepts {
            failures.push(format!("{name}: verdict differs: {}", report(&built)));
            continue;
        }
        if let Some(text) = expected.0 {
            let run = Command::new("node")
                .arg(blue.join("main.js"))
                .output()
                .unwrap();
            if !run.status.success() || run.stdout != b"42\n" || declaration(&blue) != text {
                failures.push(format!(
                    "{name}: runtime/declaration differs: {}",
                    report(&run)
                ));
            }
        }
        if let Some(error) = expected.1 {
            if primary(&built) != error || blue.exists() {
                failures.push(format!(
                    "{name}: diagnostic/output differs: {}",
                    report(&built)
                ));
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
