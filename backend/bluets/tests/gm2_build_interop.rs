// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Frozen G-M2 build continuation, bidirectional state, bare CLI and real watch.
use serde_json::json;
use std::collections::BTreeSet;
use std::process::Command;
#[path = "oracle_support/gm2_boundaries.rs"]
mod boundaries;
#[path = "oracle_support/project_paths.rs"]
mod project_paths;
#[path = "oracle_support/gm2_support.rs"]
mod support;
#[path = "oracle_support/gm2_watch.rs"]
mod watch;
#[path = "oracle_support/gm2_workflows.rs"]
mod workflows;

#[test]
fn manifest_retains_all_native_workflows_and_owner_controls() {
    let input = support::input();
    let reference = support::reference();
    assert_eq!(input["typescript"], "5.9.3");
    assert_eq!(reference["typescript"], "5.9.3");
    assert_eq!(reference["cases"].as_array().unwrap().len(), 296);
    let ids = reference["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["id"].as_str().unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(ids.len(), 296);
    for (kind, count) in [("build", 96), ("state", 160), ("cli", 20), ("watch", 20)] {
        assert_eq!(
            reference["cases"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|row| row["kind"] == kind)
                .count(),
            count
        );
    }
    assert_eq!(reference["ownerControls"], input["ownerControls"]);
    assert_eq!(input["ownerControls"].as_array().unwrap().len(), 6);
    assert_eq!(reference["seeds"].as_object().unwrap().len(), 32);
    let original = include_bytes!("fixtures/gm2_build_interop/reference.capture-v1.json");
    let hash = ring::digest::digest(&ring::digest::SHA256, original)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    assert_eq!(
        hash,
        "260c0b0a2c2bb3b0b16651d1d9b9ea6adf7df51629861ef20eb84e1113253e2a"
    );
}
#[test]
fn default_and_stop_builds_match_all_frozen_events_and_diagnostics() {
    workflows::builds();
}
#[test]
fn native_state_versions_signatures_and_cache_reuse_are_verified() {
    workflows::states(false);
}
#[test]
fn source_file_cli_matches_frozen_flags_diagnostics_and_publication() {
    workflows::cli(false);
}
#[test]
fn actual_bare_file_first_commands_use_the_native_entry_point() {
    workflows::cli(true);
}
#[test]
fn real_imported_and_package_watch_cycles_equal_native_and_clean_builds() {
    watch::observations();
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Acorn 8.15.0"]
fn frozen_native_workflows_are_reproducible() {
    let output = Command::new("node")
        .arg(support::recorder())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
#[ignore = "requires pinned TypeScript 5.9.3, Acorn 8.15.0 and actual native state consumers"]
fn exact_artifacts_consumers_execution_and_native_cross_use_match() {
    let mut rows = workflows::builds();
    rows.extend(workflows::states(true));
    rows.extend(workflows::cli(false));
    rows.extend(watch::observations());
    assert_eq!(rows.len(), 296);
    let result = support::node("--outputs", &json!(rows));
    assert_eq!(result["observations"], 296);
}
