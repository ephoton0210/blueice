// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Frozen nested declaration and output-option products through the public CLI.
use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
#[path = "oracle_support/ge4_boundaries.rs"]
mod boundaries;
#[path = "oracle_support/project_paths.rs"]
mod project_paths;
#[path = "oracle_support/ge4_support.rs"]
mod support;
use support::*;

fn recorder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../development/browser_core/phase-18-bluets/tools/record_ge4_declaration_options.cjs",
    )
}

#[test]
fn manifest_retains_every_nested_declaration_option_product() {
    let input = input();
    let reference = reference();
    let rows = input["cases"].as_array().unwrap();
    assert_eq!(rows.len(), 1536);
    assert_eq!(
        rows.iter().filter(|row| row["accepts"] == true).count(),
        1152
    );
    assert_eq!(input["typescript"], "5.9.3");
    assert_eq!(reference["typescript"], "5.9.3");
    assert_eq!(reference["cases"].as_array().unwrap().len(), 1536);
    assert_eq!(input["ownerControls"], reference["ownerControls"]);
    assert_eq!(input["ownerControls"].as_array().unwrap().len(), 6);
    assert_eq!(
        rows.iter()
            .map(|row| row["id"].as_str().unwrap())
            .collect::<BTreeSet<_>>()
            .len(),
        1536
    );
    for form in input["forms"].as_array().unwrap() {
        let products = rows
            .iter()
            .filter(|row| &row["form"] == form)
            .map(|row| {
                let options = &row["options"];
                assert_eq!(options["target"], "ES2022");
                assert_eq!(options["lib"], serde_json::json!(["ES2022"]));
                (
                    options["module"].to_string(),
                    options["stripInternal"].as_bool().unwrap(),
                    options["isolatedDeclarations"].as_bool().unwrap(),
                    options["newLine"].to_string(),
                    options["emitBOM"].as_bool().unwrap(),
                    options["removeComments"].as_bool().unwrap(),
                    options["useDefineForClassFields"].as_bool().unwrap(),
                )
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(products.len(), 128, "{form}");
    }
    assert_eq!(input["consumerOptions"]["noEmit"], true);
    assert_eq!(input["consumerOptions"]["isolatedDeclarations"], false);
    assert_eq!(input["consumerOptions"]["declaration"], false);
}

#[test]
fn all_nested_graphs_diagnostics_and_byte_exact_declarations_match_native() {
    observations("matrix");
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3, Acorn 8.15.0 and actual Node loaders"]
fn frozen_native_declaration_and_output_products_are_reproducible() {
    let output = Command::new("node").arg(recorder()).output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "requires pinned declaration consumers, exact output observations and Node"]
fn exact_declarations_consumers_output_profiles_and_execution_match_native() {
    let actual = observations("outputs");
    assert_eq!(actual.len(), 1152);
    let mut child = Command::new("node")
        .arg(recorder())
        .arg("--outputs")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut pipe = child.stdin.take().unwrap();
    let bytes = serde_json::to_vec(&actual).unwrap();
    let writer = std::thread::spawn(move || pipe.write_all(&bytes));
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
