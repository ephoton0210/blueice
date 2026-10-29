// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! State and watch metadata never widen canonical source/output authority.
use super::{support::*, watch, workflows};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;

fn scalar() -> Value {
    input()["state"][0].clone()
}
fn source_slot(info: &Value) -> usize {
    info["fileNames"]
        .as_array()
        .unwrap()
        .iter()
        .position(|name| name.as_str().unwrap().ends_with("src/main.ts"))
        .unwrap()
}
fn hash(bytes: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, bytes)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn ready(name: &str) -> (Temporary, Value) {
    let temp = Temporary::new(name);
    let row = scalar();
    write(&temp.0, &row["files"]);
    workflows::seed(&temp.0, &row, &reference(), None);
    let output = run(&temp.0, &build_args());
    assert!(output.status.success(), "{:?}", output);
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("up to date"),
        "valid native state must be reused"
    );
    (temp, row)
}
#[test]
fn continuation_keeps_canonical_reference_authority() {
    for stop in [false, true] {
        let temp = Temporary::new("reference-owner");
        let outside = Temporary::new("outside");
        let row = input()["build"][0].clone();
        write(&temp.0, &row["files"]);
        let config_path = temp.0.join("tsconfig.json");
        let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
        fs::write(outside.0.join("tsconfig.json"), "owner secret config").unwrap();
        config["references"]
            .as_array_mut()
            .unwrap()
            .push(json!({"path":outside.0}));
        fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
        let mut args = build_args();
        if stop {
            args.push("--stopBuildOnErrors".into());
        }
        let output = run(&temp.0, &args);
        assert!(!output.status.success());
        assert_eq!(outputs(&temp.0, &row["files"]), json!({}));
        assert!(!String::from_utf8_lossy(&output.stderr).contains("owner secret config"));
        assert_eq!(fs::read_dir(&outside.0).unwrap().count(), 1);
    }
}
#[test]
fn native_state_hash_path_and_resource_validation() {
    for mode in [
        "version",
        "hash",
        "source",
        "root",
        "output",
        "edges",
        "latest",
        "unknown",
        "truncated",
        "oversize",
    ] {
        let (temp, row) = ready(mode);
        let path = temp.0.join(row["stateFile"].as_str().unwrap());
        let mut info = reference()["seeds"][row["id"].as_str().unwrap()]["info"].clone();
        let index = source_slot(&info);
        match mode {
            "version" => info["version"] = json!("5.8.3"),
            "hash" => info["fileInfos"][index]["version"] = json!("0".repeat(64)),
            "source" => info["fileNames"][index] = json!("../ungranted-secret.ts"),
            "root" => info["root"] = json!([999999]),
            "output" => info["options"]["outDir"] = json!("../outside"),
            "edges" => {
                info["fileIdsList"] = json!([[1]]);
                info["referencedMap"] = json!([[999999, 1]]);
            }
            "latest" => info["latestChangedDtsFile"] = json!("../outside.d.ts"),
            "unknown" => info["foreignGrant"] = json!(true),
            _ => {}
        }
        if mode == "truncated" {
            fs::write(&path, "{").unwrap();
        } else if mode == "oversize" {
            fs::File::create(&path)
                .unwrap()
                .set_len(8 * 1024 * 1024 + 1)
                .unwrap();
        } else {
            fs::write(&path, serde_json::to_vec(&info).unwrap()).unwrap();
        }
        let output = run(&temp.0, &build_args());
        assert!(output.status.success(), "{mode}: {:?}", output);
        assert_eq!(
            events(&temp.0, &String::from_utf8_lossy(&output.stdout)),
            json!([{"kind":"build","project":"tsconfig.json"}]),
            "{mode}"
        );
        assert!(temp.0.join("out/main.d.ts").is_file());
    }
    #[cfg(unix)]
    {
        let (temp, row) = ready("state-link");
        let outside = Temporary::new("state-external");
        let sentinel = outside.0.join("state.json");
        fs::write(&sentinel, "preserve").unwrap();
        let path = temp.0.join(row["stateFile"].as_str().unwrap());
        fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(&sentinel, path).unwrap();
        let output = run(&temp.0, &build_args());
        assert!(!output.status.success());
        assert_eq!(fs::read_to_string(sentinel).unwrap(), "preserve");
    }
}
#[test]
fn interchange_never_caches_pending_or_failed_checks() {
    for marker in ["errors", "checkPending", "semanticDiagnosticsPerFile"] {
        let (temp, row) = ready("pending");
        let source = "export const result:string=42;\n";
        fs::write(temp.0.join("src/main.ts"), source).unwrap();
        let mut info = reference()["seeds"][row["id"].as_str().unwrap()]["info"].clone();
        let index = source_slot(&info);
        info["fileInfos"][index]["version"] = json!(hash(source.as_bytes()));
        info[marker] = if marker == "semanticDiagnosticsPerFile" {
            json!([index + 1])
        } else {
            json!(true)
        };
        fs::write(
            temp.0.join(row["stateFile"].as_str().unwrap()),
            serde_json::to_vec(&info).unwrap(),
        )
        .unwrap();
        for _ in 0..2 {
            let output = run(&temp.0, &build_args());
            assert!(!output.status.success());
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("2322"),
                "{marker}: {:?}",
                output
            );
            assert!(!String::from_utf8_lossy(&output.stdout).contains("up to date"));
        }
        fs::write(
            temp.0.join("src/main.ts"),
            "export const result:number=44;\n",
        )
        .unwrap();
        assert!(run(&temp.0, &build_args()).status.success());
        let output = run(&temp.0, &build_args());
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("up to date"));
    }
}
#[test]
fn bare_source_owner_output_and_atomicity() {
    let outside = Temporary::new("bare-outside");
    fs::write(
        outside.0.join("secret.ts"),
        "export const result:number=42;",
    )
    .unwrap();
    for mode in ["source", "output", "failed"] {
        let temp = Temporary::new("bare-owner");
        fs::write(
            temp.0.join("main.ts"),
            if mode == "failed" {
                "export const result:string=42;"
            } else {
                "export const result:number=42;"
            },
        )
        .unwrap();
        let args = if mode == "source" {
            vec![outside.0.join("secret.ts").to_string_lossy().into_owned()]
        } else {
            vec![
                "main.ts".into(),
                "--declaration".into(),
                "--noEmitOnError".into(),
                "true".into(),
                "--outDir".into(),
                if mode == "output" {
                    outside.0.to_string_lossy().into_owned()
                } else {
                    "out".into()
                },
            ]
        };
        let output = run(&temp.0, &args);
        assert!(!output.status.success());
        assert!(!temp.0.join("out").exists());
        assert_eq!(fs::read_dir(&outside.0).unwrap().count(), 1);
    }
    #[cfg(unix)]
    {
        let temp = Temporary::new("bare-link");
        fs::write(temp.0.join("main.ts"), "export const result:number=42;").unwrap();
        std::os::unix::fs::symlink(&outside.0, temp.0.join("out")).unwrap();
        assert!(!run(
            &temp.0,
            &["main.ts".into(), "--outDir".into(), "out".into()]
        )
        .status
        .success());
        assert_eq!(fs::read_dir(&outside.0).unwrap().count(), 1);
    }
}
#[test]
fn watch_preserves_owner_current_resolution_policy() {
    let temp = Temporary::new("watch-current");
    let row = input()["watch"][2].clone();
    write(&temp.0, &row["files"]);
    let mut process = watch::Watch::spawn(&temp.0);
    process.cycle(0);
    fs::write(
        temp.0.join("node_modules/pkg/new.d.ts"),
        "export type Value=string;",
    )
    .unwrap();
    fs::write(
        temp.0.join("node_modules/pkg/package.json"),
        "{\"name\":\"pkg\",\"types\":\"new.d.ts\"}",
    )
    .unwrap();
    let lines = process.cycle_result(1, true);
    assert!(
        lines.iter().any(|line| line.contains("TS2322")),
        "{lines:?}"
    );
    process.stop();
}
#[test]
fn watch_lifecycle_and_bounded_work() {
    let row = input()["watch"][0].clone();
    let temp = Temporary::new("watch-stop");
    write(&temp.0, &row["files"]);
    let mut process = watch::Watch::spawn(&temp.0);
    process.cycle(0);
    process.quiet();
    process.stop();
    let budget = Temporary::new("watch-budget");
    write(&budget.0, &row["files"]);
    fs::File::create(budget.0.join("oversized.bin"))
        .unwrap()
        .set_len(64 * 1024 * 1024 + 1)
        .unwrap();
    watch::Watch::spawn(Path::new(&budget.0)).failure();
}
