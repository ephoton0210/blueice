// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Actual CLI global-augmentation declarations and every map segment match Native.

use serde_json::Value;
use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

#[path = "oracle_support/gd2_maps.rs"]
mod maps;

#[test]
fn global_augmentation_declaration_maps_match_the_complete_native_reference() {
    compare_maps(false);
}

#[test]
fn legacy_global_augmentation_declaration_maps_match_the_complete_native_reference() {
    compare_maps(true);
}

fn compare_maps(legacy: bool) {
    let fixture: Value = serde_json::from_str(include_str!(
        "fixtures/program_roots/global-augmentation-maps.json"
    ))
    .unwrap();
    let base = std::env::temp_dir().join(format!(
        "bluets-program-roots-global-maps-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    for row in fixture["cases"].as_array().unwrap() {
        let root = base.join(row["id"].as_str().unwrap());
        fs::create_dir_all(&root).unwrap();
        for (name, source) in row["files"].as_object().unwrap() {
            fs::write(root.join(name), source.as_str().unwrap()).unwrap();
        }
        fs::write(
            root.join("tsconfig.json"),
            serde_json::to_vec_pretty(&row["config"]).unwrap(),
        )
        .unwrap();
        let arguments = if legacy {
            vec!["build", "--config", "tsconfig.json", "--diagnostics-json"]
        } else {
            vec!["--project", "tsconfig.json", "--diagnostics-json"]
        };
        let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .args(arguments)
            .current_dir(&root)
            .output()
            .unwrap();
        fs::write(root.join("stdout.log"), &output.stdout).unwrap();
        fs::write(root.join("stderr.log"), &output.stderr).unwrap();
        assert!(
            output.status.success(),
            "{}; retained fixture {}: {}",
            row["id"],
            root.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        for (name, expected) in row["declarations"].as_object().unwrap() {
            assert_eq!(
                fs::read_to_string(root.join("dist").join(name)).unwrap(),
                expected.as_str().unwrap(),
                "{}: {name}",
                row["id"]
            );
        }
        for (name, expected) in row["declarationMaps"].as_object().unwrap() {
            let text = fs::read_to_string(root.join("dist").join(name)).unwrap_or_else(|error| {
                panic!(
                    "{}: cannot read {name}: {error}; retained fixture {}",
                    row["id"],
                    root.display()
                )
            });
            assert_eq!(
                maps::decoded(&text),
                *expected,
                "{}: every segment and metadata field of {name}; retained fixture {}",
                row["id"],
                root.display()
            );
        }
    }
    fs::remove_dir_all(base).unwrap();
}
