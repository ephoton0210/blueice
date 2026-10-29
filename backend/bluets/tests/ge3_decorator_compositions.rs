// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Frozen decorator compositions, independent native safe equivalents and actual metadata.
use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
#[path = "oracle_support/ge3_boundaries.rs"]
mod boundaries;
#[path = "oracle_support/project_paths.rs"]
mod project_paths;
#[path = "oracle_support/ge3_support.rs"]
mod support;
use support::*;

fn recorder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools/record_ge3_decorator_compositions.cjs")
}

#[test]
fn manifest_retains_every_decorator_metadata_product_and_owner_control() {
    let input = input();
    let reference = reference();
    let rows = input["cases"].as_array().unwrap();
    assert_eq!(input["typescript"], "5.9.3");
    assert_eq!(reference["typescript"], "5.9.3");
    assert_eq!(rows.len(), 1000);
    assert_eq!(
        rows.iter()
            .map(|r| r["id"].as_str().unwrap())
            .collect::<BTreeSet<_>>()
            .len(),
        1000
    );
    let products = rows
        .iter()
        .filter(|r| r["family"] == "decorators")
        .collect::<Vec<_>>();
    assert_eq!(products.len(), 640);
    let forms = products
        .iter()
        .map(|r| r["form"].as_str().unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(forms.len(), 20);
    for form in forms {
        let tuples = products
            .iter()
            .filter(|r| r["form"] == form)
            .map(|r| {
                (
                    r["mode"].to_string(),
                    r["options"]["target"].to_string(),
                    r["options"]["module"].to_string(),
                    r["options"]["useDefineForClassFields"].as_bool().unwrap(),
                )
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(tuples.len(), 32, "{form}");
    }
    assert_eq!(
        rows.iter().filter(|r| r["family"] == "metadata").count(),
        256
    );
    assert_eq!(
        rows.iter()
            .filter(|r| r["family"] == "metadata-controls")
            .count(),
        64
    );
    assert_eq!(
        rows.iter()
            .filter(|r| r["family"] == "native-safe-equivalent")
            .count(),
        40
    );
    assert_eq!(reference["cases"].as_array().unwrap().len(), 1000);
    assert_eq!(input["ownerControls"], reference["ownerControls"]);
    assert_eq!(input["ownerControls"].as_array().unwrap().len(), 6);
    assert!(rows.iter().all(|r| r["accepts"].is_boolean()));
}

#[test]
fn all_decorator_graphs_diagnostics_and_exact_declarations_match_native() {
    observations("matrix");
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3, Acorn 8.15.0 and actual Node loaders"]
fn frozen_native_decorator_and_metadata_matrix_is_reproducible() {
    let output = Command::new("node").arg(recorder()).output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "requires pinned TypeScript, Node loaders and declaration consumers"]
fn exact_declarations_consumers_preserved_syntax_and_actual_execution_match() {
    let actual = observations("outputs");
    let expected = input()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["accepts"] == true)
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

#[test]
fn every_native_runtime_defect_keeps_a_safe_independent_execution_authority() {
    let input = input();
    let reference = reference();
    let rows = input["cases"].as_array().unwrap();
    let natives = reference["cases"].as_array().unwrap();
    let defects = rows
        .iter()
        .filter(|row| row["nativeRuntimeDefect"] == true)
        .collect::<Vec<_>>();
    assert_eq!(defects.len(), 76);
    for row in defects {
        let id = row["id"].as_str().unwrap();
        let peer_id = row["safeExecutionReference"].as_str().unwrap();
        let peer = rows.iter().find(|peer| peer["id"] == peer_id).unwrap();
        let original = natives.iter().find(|native| native["id"] == id).unwrap();
        let safe = natives
            .iter()
            .find(|native| native["id"] == peer_id)
            .unwrap();
        assert!(row["nativeRuntimeDefectReason"]
            .as_str()
            .unwrap()
            .contains("Pinned TypeScript"));
        assert_ne!(original["execution"]["exit"], 0, "{id}");
        assert_eq!(safe["execution"]["exit"], 0, "{peer_id}");
        if row["mode"] == "legacy" {
            assert_eq!(peer["options"]["target"], "ES2022");
            assert_eq!(row["files"], peer["files"]);
            let mut options = row["options"].clone();
            options["target"] = serde_json::json!("ES2022");
            assert_eq!(options, peer["options"]);
        } else {
            assert_eq!(peer["safeEquivalentFor"], id);
            assert_eq!(row["options"], peer["options"]);
            let home = if row["form"] == "private-static-super" {
                "C"
            } else {
                "C.prototype"
            };
            let expected = row["files"]["main.ts"].as_str().unwrap()
                .replace("super.method(", &format!("Reflect.get(Object.getPrototypeOf({home}), 'method', this).call(this, "))
                .replace("return super.amount;", &format!("return Reflect.get(Object.getPrototypeOf({home}), 'amount', this);"))
                .replace("super.amount = n;", &format!("if (!Reflect.set(Object.getPrototypeOf({home}), 'amount', n, this)) throw new TypeError('super assignment failed');"));
            assert_eq!(peer["files"]["main.ts"], expected);
        }
    }
}
