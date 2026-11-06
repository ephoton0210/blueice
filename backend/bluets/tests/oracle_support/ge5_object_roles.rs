// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Binding families keep their complete frozen product inventory.
use super::{cli, declarations, inputs};
use serde_json::json;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 for complete Native object roles"]
fn object_rest_spread_all_frozen_products() {
    family("object-rest-spread", 351);
}

fn family(form: &str, count: usize) {
    inputs::verify_authorities();
    let temporary = cli::Temporary::new(form);
    let compiler = inputs::sha(&std::fs::read(env!("CARGO_BIN_EXE_bluetsc")).unwrap());
    let mut observations = std::thread::scope(|scope| {
        let workers = (0..2)
            .map(|worker| {
                let temporary = &temporary;
                let compiler = &compiler;
                scope.spawn(move || {
                    let mut observations = Vec::new();
                    for (index, specification) in inputs::input()["cases"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .enumerate()
                    {
                        if index % 2 != worker
                            || specification["form"] != format!("ge2_module_helpers/{form}")
                        {
                            continue;
                        }
                        let row = inputs::resolve(specification);
                        let actual = cli::observe_builds(temporary, index, &row, 2);
                        let failures = declarations::compare(
                            specification,
                            &row,
                            &inputs::reference()["cases"][index],
                            &actual,
                        );
                        assert!(failures.is_empty(), "{}", failures.join("\n"));
                        observations.push(
                            json!({"index":index,"id":specification["id"],"compilerSha256":compiler,
            "repeatBuilds":2,"options":row["options"],"outputs":actual.outputs}),
                        );
                    }
                    observations
                })
            })
            .collect::<Vec<_>>();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>()
    });
    observations.sort_by_key(|row| row["index"].as_u64().unwrap());
    assert_eq!(observations.len(), count);
    if let Some(directory) = std::env::var_os("BLUEICE_GE5_ROLE_OBSERVATION_DIRECTORY") {
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            Path::new(&directory).join(format!("object-{form}.json")),
            serde_json::to_vec(&observations).unwrap(),
        )
        .unwrap();
    }
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools/verify_ge5_object_roles.cjs");
    let mut child = Command::new("node")
        .arg("--experimental-vm-modules")
        .arg(script)
        .arg(form)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&observations).unwrap())
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

#[test]
fn object_rest_copy_and_system_assignment_endings_keep_native_roles() {
    use blueice_bluets::lex;
    use serde_json::Value;
    fn owner(code: &str, map: &Value, offset: usize, lines: usize) -> Option<Value> {
        let prefix = &code[..offset];
        let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
        let column = prefix.rsplit('\n').next().unwrap().encode_utf16().count();
        map["segments"]
            .as_array()
            .unwrap()
            .iter()
            .rfind(|segment| segment[0] == line && segment[1].as_u64().unwrap() as usize <= column)
            .filter(|segment| {
                segment.as_array().unwrap().len() > 2 && segment[3].as_u64().unwrap() < lines as u64
            })
            .map(|segment| json!(&segment.as_array().unwrap()[2..]))
    }
    let temporary = cli::Temporary::new("object-rest-boundaries");
    let mut count = 0;
    for (index, specification) in inputs::input()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        if specification["form"] != "ge2_module_helpers/object-rest-spread"
            || specification["profile"] != "external"
        {
            continue;
        }
        let row = inputs::resolve(specification);
        if !["CommonJS", "System"].contains(&row["options"]["module"].as_str().unwrap())
            || !["ES5", "ES2015", "ES2022"].contains(&row["options"]["target"].as_str().unwrap())
        {
            continue;
        }
        let id = specification["id"].as_str().unwrap();
        if !(id.contains("-i1-n0/")
            || row["options"]["module"] == "System" && row["options"]["target"] == "ES2022")
        {
            continue;
        }
        let expected = &inputs::reference()["cases"][index];
        let actual = cli::observe_builds(&temporary, index, &row, 2);
        assert!(declarations::compare(specification, &row, expected, &actual).is_empty());
        let native_code = inputs::blob(expected["outputs"]["out/main.js"].as_str().unwrap());
        let native_map = super::decoded::decoded(inputs::blob(
            expected["outputs"]["out/main.js.map"].as_str().unwrap(),
        ));
        let actual_code = &actual.outputs["out/main.js"];
        let actual_map = super::decoded::decoded(&actual.outputs["out/main.js.map"]);
        let lines = row["files"]["main.ts"].as_str().unwrap().lines().count();
        for text in [
            "source",
            "a",
            "remaining",
            "copy",
            "result",
            "b",
            "20",
            "21",
            "...",
            ";",
        ] {
            let roles = |code: &str, map: &Value| {
                lex("<javascript>", code)
                    .unwrap()
                    .iter()
                    .filter(|token| token.text == text)
                    .filter_map(|token| {
                        owner(code, map, token.start, lines).map(|_| {
                            code[token.start..token.end]
                                .char_indices()
                                .map(|(offset, _)| owner(code, map, token.start + offset, lines))
                                .collect::<Vec<_>>()
                        })
                    })
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                roles(actual_code, &actual_map),
                roles(native_code, &native_map),
                "{id}: complete {text} interiors"
            );
        }
        count += 1;
    }
    assert_eq!(count, 12);
}
