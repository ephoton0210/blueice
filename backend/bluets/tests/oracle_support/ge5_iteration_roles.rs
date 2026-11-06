// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Iteration families keep their complete frozen product inventory.
use super::{cli, declarations, inputs};
use serde_json::json;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 for complete Native iteration roles"]
fn iterable_bindings_forof_all_frozen_products() {
    family("iterable-bindings-forof", 339);
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
            Path::new(&directory).join(format!("iteration-{form}.json")),
            serde_json::to_vec(&observations).unwrap(),
        )
        .unwrap();
    }
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../development/browser_core/phase-18-bluets/tools/verify_ge5_iteration_roles.cjs",
    );
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
