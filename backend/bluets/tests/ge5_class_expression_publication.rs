// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Hoisted ordinary, async and generator exports retain independent endpoints.
#![allow(dead_code)]
#[path = "oracle_support/ge5_cli.rs"]
mod cli;
#[path = "oracle_support/gd2_maps.rs"]
mod decoded;
#[path = "oracle_support/ge5_inputs.rs"]
mod inputs;
#[path = "oracle_support/project_paths.rs"]
mod project_paths;
use serde_json::json;
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn hoisted_function_publications_keep_the_original_function_end_before_generated_trivia() {
    let temporary = cli::Temporary::new("expression-function-publication");
    let body="export function ordinary():number { return 1; }\nexport async function asynchronous():Promise<number> { return await Promise.resolve(2); }\nexport function* generator():Generator<number,number,unknown> { yield 3; return 4; }\nexport const result=[ordinary(),asynchronous(),generator().next().value];\n";
    let mut observations = Vec::new();
    for target in ["ES5", "ES2015", "ES2022"] {
        for coordinate in ["original", "crlfAstral"] {
            let text = if coordinate == "original" {
                body.to_string()
            } else {
                format!("/* G-E5 🧭 */ {}", body.replace('\n', "\r\n"))
            };
            for maps in [true, false] {
                let options = json!({"target":target,"module":"CommonJS","sourceMap":maps,"inlineSources":maps,"declaration":true,"declarationMap":true,"lib":["ES2022"],"outDir":"out","declarationDir":"types","rootDir":".","noEmitOnError":true});
                let row = json!({"id":"ordinary-function-publication","entry":"main.ts","files":{"main.ts":text},"options":options});
                let actual = cli::observe_builds(&temporary, observations.len(), &row, 2);
                assert!(actual.success, "{:?}", actual.diagnostics);
                assert_eq!(actual.diagnostics, json!([]));
                observations.push(json!({"target":target,"coordinate":coordinate,"maps":maps,"source":text,"options":options,"outputs":actual.outputs}));
            }
        }
    }
    if let Some(directory) = std::env::var_os("BLUEICE_GE5_ROLE_OBSERVATION_DIRECTORY") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("class-expression-function-publication.json"),
            serde_json::to_vec(&observations).unwrap(),
        )
        .unwrap();
    }
    let script=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../development/browser_core/phase-18-bluets/tools/verify_ge5_class_expression_function_publication.cjs");
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
fn lowered_named_class_binding_is_initialized_before_its_method_decorator_runs() {
    let temporary = cli::Temporary::new("expression-named-binding-phase");
    let body="const events:any[]=[];\nfunction identity(value:any):any { return value; }\nfunction observe(value:any,context:any):any { events.push(value.call({})); return value; }\nexport const C=@identity class Named { @observe method():string { return Named.name; } };\nexport const result=events;\n";
    let mut observations = Vec::new();
    for target in ["ES5", "ES2015"] {
        for coordinate in ["original", "crlfAstral"] {
            let text = if coordinate == "original" {
                body.to_string()
            } else {
                format!("/* G-E5 🧭 */ {}", body.replace('\n', "\r\n"))
            };
            for maps in [true, false] {
                let options = json!({"target":target,"module":"CommonJS","sourceMap":maps,"inlineSources":maps,"declaration":true,"declarationMap":true,"lib":["ES2022","ESNext.Decorators"],"outDir":"out","declarationDir":"types","rootDir":".","noEmitOnError":true,"experimentalDecorators":false,"useDefineForClassFields":false});
                let row = json!({"id":"named-class-binding-phase","entry":"main.ts","files":{"main.ts":text},"options":options});
                let actual = cli::observe_builds(&temporary, observations.len(), &row, 2);
                assert!(actual.success, "{:?}", actual.diagnostics);
                assert_eq!(actual.diagnostics, json!([]));
                observations.push(json!({"target":target,"coordinate":coordinate,"maps":maps,"source":text,"options":options,"outputs":actual.outputs}));
            }
        }
    }
    if let Some(directory) = std::env::var_os("BLUEICE_GE5_ROLE_OBSERVATION_DIRECTORY") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("class-expression-named-binding-phase.json"),
            serde_json::to_vec(&observations).unwrap(),
        )
        .unwrap();
    }
    let script=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../development/browser_core/phase-18-bluets/tools/verify_ge5_class_expression_binding_phase.cjs");
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
