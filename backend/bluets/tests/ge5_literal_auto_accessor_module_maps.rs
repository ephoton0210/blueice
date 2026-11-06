// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Complete Native module contracts for every frozen literal auto-accessor decorator product.
#[allow(dead_code)]
#[path = "oracle_support/ge5_cli.rs"]
mod cli;
#[allow(dead_code)]
#[path = "oracle_support/ge5_declarations.rs"]
mod declarations;
#[allow(dead_code)]
#[path = "oracle_support/gd2_maps.rs"]
mod decoded;
#[allow(dead_code)]
#[path = "oracle_support/ge5_inputs.rs"]
mod inputs;
#[allow(dead_code)]
#[path = "oracle_support/ge5_javascript_maps.rs"]
mod javascript_maps;
#[path = "oracle_support/project_paths.rs"]
mod project_paths;

use serde_json::json;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

#[test]
#[ignore = "requires pinned TypeScript 5.9.3, Acorn 8.15 and every frozen literal accessor product"]
fn literal_auto_accessor_decorators_preserve_every_complete_module_role() {
    inputs::verify_authorities();
    let group = "literal-auto-accessors";
    let temporary = cli::Temporary::new("decorator-call-roles");
    let compiler = inputs::sha(&std::fs::read(env!("CARGO_BIN_EXE_bluetsc")).unwrap());
    let specifications = inputs::input()["cases"].as_array().unwrap();
    let selection = specifications
        .iter()
        .enumerate()
        .filter(|(_, specification)| {
            specification["form"] == "ge3_decorator_compositions/literal-auto-accessors"
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    assert_eq!(selection.len(), 51);
    let mut observations = std::thread::scope(|scope| {
        let workers = (0..2)
            .map(|worker| {
                let temporary = &temporary;
                let compiler = &compiler;
                let selection = &selection;
                scope.spawn(move || {
                    let mut result = Vec::new();
                    for (position, &index) in selection.iter().enumerate() {
                        if position % 2 != worker {
                            continue;
                        }
                        let specification = &specifications[index];
                        let original = inputs::resolve(specification);
                        let actual = cli::observe_builds(temporary, index, &original, 2);
                        let differences = declarations::compare(
                            specification,
                            &original,
                            &inputs::reference()["cases"][index],
                            &actual,
                        );
                        assert!(differences.is_empty(), "{}", differences.join("\n"));
                        result.push(json!({"index":index,"id":specification["id"],
                            "compilerSha256":compiler,"repeatBuilds":2,
                            "options":original["options"],"outputs":actual.outputs}));
                    }
                    result
                })
            })
            .collect::<Vec<_>>();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>()
    });
    observations.sort_by_key(|row| row["index"].as_u64().unwrap());
    if let Some(directory) = std::env::var_os("BLUEICE_GE5_ROLE_OBSERVATION_DIRECTORY") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join(format!("decorator-{group}.json")),
            serde_json::to_vec(&observations).unwrap(),
        )
        .unwrap();
    }
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools")
        .join("verify_ge5_auto_accessor_roles.cjs");
    let mut child = Command::new("node")
        .args(["--experimental-vm-modules"])
        .arg(script)
        .arg(group)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let written = child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&observations).unwrap());
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    written.unwrap();
    eprintln!("{}", String::from_utf8_lossy(&result.stdout));
}
