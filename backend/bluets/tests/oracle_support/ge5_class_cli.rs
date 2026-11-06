// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public CLI observations for complete frozen class source families.
use super::{cli, declarations, inputs};
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

pub(super) fn run(selected: fn(&Value) -> bool, configurations: usize, report: &str, script: &str) {
    let temporary = cli::Temporary::new("class-complete-roles");
    let compiler = inputs::sha(&std::fs::read(env!("CARGO_BIN_EXE_bluetsc")).unwrap());
    let mut observations = std::thread::scope(|scope| {
        let workers = (0..4)
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
                        if index % 4 != worker || !selected(specification) {
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
    assert_eq!(observations.len(), configurations);
    if let Some(directory) = std::env::var_os("BLUEICE_GE5_ROLE_OBSERVATION_DIRECTORY") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join(format!("{report}.json")),
            serde_json::to_vec(&observations).unwrap(),
        )
        .unwrap();
    }
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools")
        .join(script);
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
