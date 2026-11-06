// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Independent emitted-AST contracts for complete ES5 continuation families.
use super::{cli, declarations, inputs};
use serde_json::json;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 for independent continuation ASTs"]
fn opaque_generator_es5_all_publication_products_match_native_roles() {
    run(
        "opaque-generator-es5",
        "verify_ge5_continuation_roles.cjs",
        "continuations.json",
    );
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 for independent continuation ASTs"]
fn binary_generator_es5_all_publication_products_match_native_roles() {
    run(
        "binary-yield-order",
        "verify_ge5_linear_roles.cjs",
        "linear.json",
    );
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 for independent continuation ASTs"]
fn logical_generator_es5_all_publication_products_match_native_roles() {
    run(
        "logical-yield-order",
        "verify_ge5_branch_roles.cjs",
        "branching.json",
    );
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 for independent continuation ASTs"]
fn named_async_es5_helper_collisions_all_publication_products_match_native_roles() {
    run(
        "helper-name-collisions",
        "verify_ge5_named_async_roles.cjs",
        "named-async.json",
    );
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 for independent continuation ASTs"]
fn nested_async_es5_calls_all_publication_products_match_native_roles() {
    run(
        "nested-await-call",
        "verify_ge5_nested_async_roles.cjs",
        "nested-async.json",
    );
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 for independent continuation ASTs"]
fn conditional_expression_async_es5_all_publication_products_match_native_roles() {
    run(
        "conditional-expression-await",
        "verify_ge5_conditional_async_roles.cjs",
        "conditional-async.json",
    );
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 for independent continuation ASTs"]
fn conditional_statement_async_es5_all_publication_products_match_native_roles() {
    run(
        "conditional-await",
        "verify_ge5_conditional_statement_async_roles.cjs",
        "conditional-statement-async.json",
    );
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 for independent continuation ASTs"]
fn template_async_es5_all_publication_products_match_native_roles() {
    run(
        "template-await-order",
        "verify_ge5_template_async_roles.cjs",
        "template-async.json",
    );
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 for independent continuation ASTs"]
fn template_generator_es5_all_publication_products_match_native_roles() {
    run(
        "template-yield-order",
        "verify_ge5_template_generator_roles.cjs",
        "template-generator.json",
    );
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 for independent continuation ASTs"]
fn computed_assignment_generator_es5_all_publication_products_match_native_roles() {
    run(
        "computed-assignment-yield",
        "verify_ge5_computed_roles.cjs",
        "computed.json",
    );
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 for independent continuation ASTs"]
fn switch_generator_es5_all_publication_products_match_native_roles() {
    run(
        "switch-suspension-fallthrough",
        "verify_ge5_switch_roles.cjs",
        "switch.json",
    );
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 for independent continuation ASTs"]
fn finally_return_generator_es5_all_publication_products_match_native_roles() {
    run(
        "finally-overrides-return",
        "verify_ge5_finally_roles.cjs",
        "finally.json",
    );
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 for independent continuation ASTs"]
fn catch_finally_generator_es5_all_publication_products_match_native_roles() {
    run(
        "switch-exception-finally",
        "verify_ge5_catch_finally_roles.cjs",
        "catch-finally.json",
    );
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 for independent continuation ASTs"]
fn loop_finally_generator_es5_all_publication_products_match_native_roles() {
    run(
        "loop-abrupt-finally",
        "verify_ge5_loop_finally_roles.cjs",
        "loop-finally.json",
    );
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 for independent continuation ASTs"]
fn delegated_finally_generator_es5_all_publication_products_match_native_roles() {
    run(
        "delegate-external-return-finally",
        "verify_ge5_delegate_finally_roles.cjs",
        "delegate-finally.json",
    );
}

fn run(form: &str, script: &str, observation: &str) {
    let temporary = cli::Temporary::new("continuation-roles");
    let compiler = inputs::sha(&std::fs::read(env!("CARGO_BIN_EXE_bluetsc")).unwrap());
    let mut observations = Vec::new();
    for (index, specification) in inputs::input()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        if specification["form"] != format!("ge1_downlevel/{form}") {
            continue;
        }
        let row = inputs::resolve(specification);
        if row["options"]["target"] != "ES5" {
            continue;
        }
        let actual = cli::observe_builds(&temporary, index, &row, 2);
        let failures = declarations::compare(
            specification,
            &row,
            &inputs::reference()["cases"][index],
            &actual,
        );
        assert!(failures.is_empty(), "{}", failures.join("\n"));
        observations.push(
            json!({"index":index,"id":specification["id"],"compilerSha256":compiler,
            "repeatBuilds":2,"files":row["files"],"outputs":actual.outputs}),
        );
    }
    assert_eq!(observations.len(), 7);
    if let Some(path) = std::env::var_os("BLUEICE_GE5_CONTINUATION_OBSERVATIONS") {
        std::fs::write(path, serde_json::to_vec(&observations).unwrap()).unwrap();
    }
    if let Some(directory) = std::env::var_os("BLUEICE_GE5_ROLE_OBSERVATION_DIRECTORY") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join(observation),
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
