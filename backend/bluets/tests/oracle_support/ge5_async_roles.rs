// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Independent Native AST adapter for an entire async driver, body and module.
use super::{cli, declarations, inputs};
use serde_json::json;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 for the independent emitted-JavaScript AST"]
fn complete_named_async_non_es5_modules_match_native_source_roles() {
    run(false);
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 for the independent emitted-JavaScript AST"]
fn complete_native_generator_modules_match_native_source_roles() {
    run(true);
}

fn run(generators: bool) {
    let temporary = cli::Temporary::new("async-module-roles");
    let compiler_sha256 = inputs::sha(&std::fs::read(env!("CARGO_BIN_EXE_bluetsc")).unwrap());
    let forms = if generators {
        vec![
            "opaque-generator-es5",
            "switch-suspension-fallthrough",
            "binary-yield-order",
            "logical-yield-order",
            "computed-assignment-yield",
            "switch-exception-finally",
            "finally-overrides-return",
            "loop-abrupt-finally",
            "delegate-external-return-finally",
            "template-yield-order",
        ]
    } else {
        vec![
            "conditional-await",
            "conditional-expression-await",
            "template-await-order",
            "nested-await-call",
            "helper-name-collisions",
        ]
    }
    .into_iter()
    .map(|name| format!("ge1_downlevel/{name}"))
    .collect::<Vec<_>>();
    let mut observations = std::thread::scope(|scope| {
        let workers = (0..4).map(|worker| {
            let temporary = &temporary;
            let forms = &forms;
            let compiler_sha256 = &compiler_sha256;
            scope.spawn(move || {
                let mut rows = Vec::new();
                for (index, specification) in inputs::input()["cases"].as_array().unwrap().iter().enumerate() {
                    if index % 4 != worker || !forms.iter().any(|form| specification["form"] == *form) { continue; }
                    let row = inputs::resolve(specification);
                    if row["options"]["target"] == "ES5"
                        || !["CommonJS", "ESNext"].iter().any(|module| row["options"]["module"] == *module) { continue; }
                    let actual = cli::observe_builds(temporary, index, &row, 2);
                    let failures = declarations::compare(specification, &row, &inputs::reference()["cases"][index], &actual);
                    assert!(failures.is_empty(), "{}", failures.join("\n"));
                    rows.push(json!({"index":index,"id":specification["id"],"target":row["options"]["target"],
                        "compilerSha256":compiler_sha256,"repeatBuilds":2,"outputs":actual.outputs}));
                }
                rows
            })
        }).collect::<Vec<_>>();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>()
    });
    observations.sort_by_key(|row| row["index"].as_u64().unwrap());
    assert_eq!(observations.len(), if generators { 200 } else { 100 });
    if let Some(path) = std::env::var_os("BLUEICE_GE5_ASYNC_OBSERVATIONS") {
        std::fs::write(path, serde_json::to_vec(&observations).unwrap()).unwrap();
    }
    if let Some(directory) = std::env::var_os("BLUEICE_GE5_ROLE_OBSERVATION_DIRECTORY") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join(if generators {
                "generators.json"
            } else {
                "asynchronous.json"
            }),
            serde_json::to_vec(&observations).unwrap(),
        )
        .unwrap();
    }
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools/verify_ge5_async_roles.cjs");
    let mut command = Command::new("node");
    command.arg(script);
    if generators {
        command.arg("--generators");
    }
    let mut child = command
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
