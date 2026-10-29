// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Frozen execution-first downlevel graph, class, pattern and async boundaries.
use serde_json::Value;
use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
#[path = "oracle_support/ge1_boundaries.rs"]
mod boundaries;
#[path = "oracle_support/project_paths.rs"]
mod project_paths;
#[path = "oracle_support/ge1_support.rs"]
mod support;
use support::*;

fn recorder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools/record_ge1_downlevel.cjs")
}
#[test]
fn manifest_retains_all_native_forms_and_owner_controls() {
    let input = input();
    let reference = reference();
    let rows = input["cases"].as_array().unwrap();
    assert_eq!(input["typescript"], "5.9.3");
    assert_eq!(reference["typescript"], "5.9.3");
    assert_eq!(rows.len(), 576);
    assert_eq!(rows.iter().filter(|r| r["accepts"] == true).count(), 500);
    assert_eq!(
        rows.iter()
            .map(|r| r["id"].as_str().unwrap())
            .collect::<BTreeSet<_>>()
            .len(),
        576
    );
    assert_eq!(
        rows.iter()
            .map(|r| r["form"].as_str().unwrap())
            .collect::<BTreeSet<_>>()
            .len(),
        38
    );
    assert_eq!(reference["cases"].as_array().unwrap().len(), 576);
    assert_eq!(input["ownerControls"], reference["ownerControls"]);
    assert_eq!(input["ownerControls"].as_array().unwrap().len(), 6);
    assert_eq!(
        reference["cases"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["outputs"]
                .as_object()
                .unwrap()
                .keys()
                .filter(|n| n.ends_with(".d.ts"))
                .count())
            .sum::<usize>(),
        524
    );
}
#[test]
fn all_downlevel_graphs_diagnostics_and_declarations_match_native() {
    observations("matrix");
}
#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Acorn 8.15.0"]
fn frozen_native_matrix_is_reproducible() {
    let output = Command::new("node").arg(recorder()).output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
#[ignore = "requires pinned TypeScript 5.9.3, Acorn 8.15.0 and actual Node execution"]
fn exact_declarations_consumers_imports_syntax_and_execution_match_native() {
    let actual = observations("outputs");
    assert_eq!(actual.len(), 500);
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
