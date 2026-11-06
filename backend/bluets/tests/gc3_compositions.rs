// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Complete frozen compositions, physical outputs and separate owner controls.
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

const REGISTRY: &str = include_str!("fixtures/gc3_compositions/registry.json");
const REFERENCE: &str = include_str!("fixtures/gc3_compositions/reference.json");
const POLICY: &str = include_str!("fixtures/gc3_compositions/policy.json");

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn hash(bytes: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, bytes)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[test]
fn frozen_inputs_roles_and_all_original_k10_bytes_remain_intact() {
    let policy: Value = serde_json::from_str(POLICY).unwrap();
    let matrix_bytes = fs::read(repo().join(policy["frozenMatrixPath"].as_str().unwrap())).unwrap();
    let matrix: Value = serde_json::from_slice(&matrix_bytes).unwrap();
    assert_eq!(hash(&matrix_bytes), policy["frozenMatrixSha256"]);
    assert_eq!(hash(REGISTRY.as_bytes()), policy["registrySha256"]);
    assert_eq!(hash(REFERENCE.as_bytes()), policy["nativeReferenceSha256"]);
    assert_eq!(policy["caseRoles"], matrix["caseRoles"]);
    assert_eq!(policy["retainedK10Files"], matrix["retainedK10Files"]);
    let files = policy["retainedK10Files"].as_object().unwrap();
    assert_eq!(files.len(), 995);
    for (path, expected) in files {
        assert_eq!(
            hash(&fs::read(repo().join(path)).unwrap()),
            *expected,
            "{path}"
        );
    }
    let reference: Value = serde_json::from_str(REFERENCE).unwrap();
    let cases = reference["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 296);
    assert_eq!(matrix["retainedK10Configurations"], 148);
    for row in cases {
        assert!(policy["caseRoles"][row["id"].as_str().unwrap()].is_object());
    }
}

fn run_gate(script: &str, label: &str, blue: bool) {
    let retained = env::var_os("BLUEICE_GC3_REPORT_DIRECTORY");
    let parent = retained
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| env::temp_dir().join(format!("bluets-gc3-full-{}", std::process::id())));
    fs::create_dir_all(&parent).unwrap();
    let directory = parent.join(label);
    assert!(!directory.exists(), "fresh report: {}", directory.display());
    let mut command = Command::new("node");
    command.arg(
        repo()
            .join("development/browser_core/phase-18-bluets/tools")
            .join(script),
    );
    if blue {
        command.arg(env!("CARGO_BIN_EXE_bluetsc"));
    }
    let output = command.arg(&directory).output().unwrap();
    fs::write(parent.join(format!("{label}.stdout.log")), &output.stdout).unwrap();
    fs::write(parent.join(format!("{label}.stderr.log")), &output.stderr).unwrap();
    assert!(
        output.status.success(),
        "{label}: {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value =
        serde_json::from_slice(&fs::read(directory.join("verification.json")).unwrap()).unwrap();
    assert_eq!(result["conclusion"], "success");
    assert_eq!(result["configurations"], 296);
    if blue {
        assert_eq!(result["directCompared"], 220);
        assert_eq!(result["ownerControls"].as_array().unwrap().len(), 76);
        assert_eq!(
            result["nativeRuntimeFailures"].as_array().unwrap().len(),
            12
        );
    }
    if retained.is_none() {
        fs::remove_dir_all(directory).unwrap();
    }
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3, Acorn 8.15.0 and Node"]
fn pinned_native_reproduces_all_frozen_composition_observations() {
    run_gate("verify_gc3_native.cjs", "native", false);
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3, Acorn 8.15.0 and Node"]
fn complete_compositions_match_native_and_keep_owner_denials_separate() {
    run_gate("verify_gc3_compositions.cjs", "blue", true);
}
