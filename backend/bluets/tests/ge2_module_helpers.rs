// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Frozen Node inputs, real loaders and the published tslib helper ABI.
use serde_json::Value;
use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
#[path = "oracle_support/ge2_boundaries.rs"]
mod boundaries;
#[path = "oracle_support/project_paths.rs"]
mod project_paths;
#[path = "oracle_support/ge2_support.rs"]
mod support;
use support::*;

fn recorder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools/record_ge2_module_helpers.cjs")
}

#[test]
fn manifest_retains_every_module_target_flag_product_and_owner_control() {
    let input = input();
    let reference = reference();
    let rows = input["cases"].as_array().unwrap();
    assert_eq!(input["typescript"], "5.9.3");
    assert_eq!(input["helperProvider"]["version"], "2.8.1");
    assert_eq!(reference["typescript"], "5.9.3");
    assert_eq!(rows.len(), 2604);
    assert_eq!(
        rows.iter()
            .map(|r| r["id"].as_str().unwrap())
            .collect::<BTreeSet<_>>()
            .len(),
        2604
    );
    let products = rows
        .iter()
        .filter(|r| r["family"] == "helper-product")
        .collect::<Vec<_>>();
    assert_eq!(products.len(), 2520);
    assert_eq!(
        products
            .iter()
            .map(|r| r["form"].as_str().unwrap())
            .collect::<BTreeSet<_>>()
            .len(),
        14
    );
    for form in products
        .iter()
        .map(|r| r["form"].as_str().unwrap())
        .collect::<BTreeSet<_>>()
    {
        let product = products
            .iter()
            .filter(|r| r["form"] == form)
            .map(|r| {
                (
                    r["runtimeModule"].to_string(),
                    r["options"]["module"].to_string(),
                    r["options"]["target"].to_string(),
                    r["options"]["importHelpers"].as_bool().unwrap(),
                    r["options"]["noEmitHelpers"].as_bool().unwrap(),
                )
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(product.len(), 180, "{form}");
    }
    assert_eq!(reference["cases"].as_array().unwrap().len(), 2604);
    assert_eq!(input["ownerControls"], reference["ownerControls"]);
    assert_eq!(input["ownerControls"].as_array().unwrap().len(), 6);
    assert!(rows.iter().all(|r| r["accepts"].is_boolean()));
}

#[test]
fn all_module_graphs_diagnostics_suffixes_and_exact_declarations_match_native() {
    observations("matrix");
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3, Acorn 8.15.0 and actual Node loaders"]
fn frozen_native_loader_and_helper_matrix_is_reproducible() {
    let output = Command::new("node").arg(recorder()).output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "requires pinned TypeScript, real tslib, Node loaders and declaration consumers"]
fn exact_declarations_consumers_imported_abi_and_real_loader_completions_match() {
    let actual = observations("outputs");
    let expected = input()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["accepts"] == true && r["nativeRuntimeDefect"] != true)
        .count();
    assert_eq!(actual.len(), expected);
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
