// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Complete frozen module families use two public CLI builds per configuration.
use super::{cli, declarations, decoded, inputs, node_roles};
use serde_json::json;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

#[test]
fn retained_optional_module_keeps_every_native_token_origin() {
    retained("optional-nullish");
}

#[test]
fn retained_spread_module_keeps_every_native_token_origin() {
    retained("sparse-array-call-spread");
}

#[test]
fn retained_tagged_template_module_keeps_every_native_token_origin() {
    retained("tagged-template");
}

fn retained(form: &str) {
    let temporary = cli::Temporary::new("retained-optional-module");
    let mut compared = 0;
    for (index, specification) in inputs::input()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        if !specification["id"]
            .as_str()
            .unwrap()
            .starts_with(&format!("ge2_module_helpers/{form}-cjs-es2022-i0-n0/"))
            || specification["profile"] != "external"
        {
            continue;
        }
        let row = inputs::resolve(specification);
        let expected = &inputs::reference()["cases"][index];
        let actual = cli::observe_builds(&temporary, index, &row, 2);
        assert!(declarations::compare(specification, &row, expected, &actual).is_empty());
        let native_js = inputs::blob(expected["outputs"]["out/main.js"].as_str().unwrap());
        let native_map = decoded::decoded(inputs::blob(
            expected["outputs"]["out/main.js.map"].as_str().unwrap(),
        ));
        let actual_map = decoded::decoded(&actual.outputs["out/main.js.map"]);
        let sources = [row["files"]["main.ts"].as_str().unwrap()];
        assert_eq!(
            node_roles::compare(
                native_js,
                &native_map,
                &actual.outputs["out/main.js"],
                &actual_map,
                &sources
            ),
            Ok(()),
            "{}",
            specification["id"]
        );
        compared += 1;
    }
    assert_eq!(compared, 2);
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 for independent module ASTs"]
fn optional_nullish_all_modules_targets_and_helper_products_match_native_roles() {
    family(
        "optional-nullish",
        "optional-modules",
        "verify_ge5_module_roles.cjs",
    );
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 for independent module ASTs"]
fn spread_all_modules_targets_and_helper_products_match_native_roles() {
    family(
        "sparse-array-call-spread",
        "spread-modules",
        "verify_ge5_spread_roles.cjs",
    );
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 for independent module ASTs"]
fn tagged_template_all_modules_targets_and_helper_products_match_native_roles() {
    family(
        "tagged-template",
        "tagged-modules",
        "verify_ge5_tagged_roles.cjs",
    );
}

fn family(form: &str, report: &str, script: &str) {
    let temporary = cli::Temporary::new("optional-module-roles");
    let compiler = inputs::sha(&std::fs::read(env!("CARGO_BIN_EXE_bluetsc")).unwrap());
    let mut observations = std::thread::scope(|scope| {
        let workers = (0..4)
            .map(|worker| {
                let temporary = &temporary;
                let compiler = &compiler;
                scope.spawn(move || {
                    let mut rows = Vec::new();
                    for (index, specification) in inputs::input()["cases"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .enumerate()
                    {
                        if index % 4 != worker
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
                        rows.push(
                            json!({"index":index,"id":specification["id"],"compilerSha256":compiler,
                        "repeatBuilds":2,"options":row["options"],"outputs":actual.outputs}),
                        );
                    }
                    rows
                })
            })
            .collect::<Vec<_>>();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>()
    });
    observations.sort_by_key(|row| row["index"].as_u64().unwrap());
    assert_eq!(observations.len(), 363);
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
