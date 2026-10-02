// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The accepted/rejected legacy-decorator checker matrix (J.5.4).
//!
//! `legacy-decorators-checker-matrix.tsv` records, for every decorator fixture entry, whether
//! pinned TypeScript 5.9.3 accepts or rejects it. The ordinary test compares
//! BlueTSC's verdict with that record, and the ignored oracle test re-derives
//! the record from the pinned compiler, so the two compilers agree
//! transitively. Decorators are admitted, so BlueTSC succeeds on exactly the
//! entries pinned TypeScript accepts and fails on the rest, except a short
//! deferred list of accepted entries that use a member BlueTS cannot erase yet:
//! those must fail with the unsupported-syntax code and nothing else.
//!
//! To regenerate the record after adding a decorator fixture, run the oracle test
//! with `BLUEICE_WRITE_LEGACY_DECORATORS_MATRIX=1`.

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const MATRIX: &str =
    include_str!("fixtures/typescript_oracle/legacy-decorators-checker-matrix.tsv");
/// Entries pinned TypeScript accepts that BlueTS deliberately refuses, only as
/// unsupported syntax, until the feature they use exists (J.5).
const DEFERRED: &str =
    include_str!("fixtures/typescript_oracle/legacy-decorators-checker-matrix-deferred.txt");
const UNSUPPORTED_CODE: &str = "BTS1001";

fn deferred_entries() -> BTreeSet<String> {
    DEFERRED
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_string)
        .collect()
}

/// Legacy decorators: `experimentalDecorators`, the ES2022 target.
fn tsc_flags(_entry: &str) -> Vec<String> {
    vec!["--experimentalDecorators".to_string()]
}

fn bluetsc_flags(_entry: &str) -> Vec<String> {
    vec![
        "--experimental-decorators".to_string(),
        "--target".to_string(),
        "es2022".to_string(),
    ]
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/typescript_oracle")
}

/// `(relative entry path, pinned TypeScript accepts it)`.
fn recorded_rows() -> Vec<(String, bool)> {
    MATRIX
        .lines()
        .map(|line| {
            let (path, verdict) = line.split_once('\t').expect("tab-separated row");
            let accepts = match verdict {
                "accept" => true,
                "reject" => false,
                other => panic!("unknown verdict {other:?} for {path}"),
            };
            (path.to_string(), accepts)
        })
        .collect()
}

/// Every decorator fixture directory contributes `main.ts`, or else each
/// `*valid.ts` / `*error.ts` file when it holds several entry modules.
fn discovered_entries() -> BTreeSet<String> {
    let mut entries = BTreeSet::new();
    for directory in fs::read_dir(fixtures()).unwrap() {
        let directory = directory.unwrap();
        let name = directory.file_name().into_string().unwrap();
        if !name.starts_with("legacy-") || !directory.path().is_dir() {
            continue;
        }
        if directory.path().join("main.ts").is_file() {
            entries.insert(format!("{name}/main.ts"));
            continue;
        }
        for file in fs::read_dir(directory.path()).unwrap() {
            let file = file.unwrap().file_name().into_string().unwrap();
            if file.ends_with("valid.ts") || file.ends_with("error.ts") {
                entries.insert(format!("{name}/{file}"));
            }
        }
    }
    entries
}

fn diagnostic_codes(output: &Output) -> BTreeSet<String> {
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let mut codes = BTreeSet::new();
    let mut rest = text.as_str();
    while let Some(position) = rest.find("BTS") {
        let candidate = &rest[position..];
        if candidate.len() >= 7 && candidate[3..7].bytes().all(|byte| byte.is_ascii_digit()) {
            codes.insert(candidate[..7].to_string());
        }
        rest = &rest[position + 3..];
    }
    codes
}

#[test]
fn matrix_lists_every_legacy_decorator_fixture_entry() {
    let recorded: BTreeSet<String> = recorded_rows().into_iter().map(|(path, _)| path).collect();
    let discovered = discovered_entries();
    let missing: Vec<_> = discovered.difference(&recorded).collect();
    let stale: Vec<_> = recorded.difference(&discovered).collect();
    assert!(
        missing.is_empty() && stale.is_empty(),
        "legacy-decorators-checker-matrix.tsv is out of date (regenerate with the ignored oracle test and \
         BLUEICE_WRITE_LEGACY_DECORATORS_MATRIX=1); missing: {missing:?}; stale: {stale:?}"
    );
}

#[test]
fn deferred_entries_are_a_subset_of_the_accepted_rows() {
    let accepted: BTreeSet<String> = recorded_rows()
        .into_iter()
        .filter(|(_, accepts)| *accepts)
        .map(|(path, _)| path)
        .collect();
    for entry in deferred_entries() {
        assert!(
            accepted.contains(&entry),
            "{entry} is deferred but pinned TypeScript does not accept it"
        );
    }
}

#[test]
fn bluetsc_verdicts_match_the_recorded_pinned_typescript_verdicts() {
    let deferred = deferred_entries();
    for (path, accepts) in recorded_rows() {
        let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .arg("check")
            .arg(fixtures().join(&path))
            .args(bluetsc_flags(&path))
            .output()
            .unwrap();
        let codes = diagnostic_codes(&output);
        if deferred.contains(&path) {
            assert!(
                !output.status.success(),
                "{path}: a deferred entry must fail"
            );
            assert_eq!(
                codes,
                BTreeSet::from([UNSUPPORTED_CODE.to_string()]),
                "{path}: a deferred entry may fail only as unsupported syntax"
            );
            continue;
        }
        assert_eq!(
            output.status.success(),
            accepts,
            "{path}: BlueTSC {codes:?} disagrees with pinned TypeScript (accepts: {accepts})"
        );
        assert!(
            !accepts || codes.is_empty(),
            "{path}: an accepted decorator program must report no diagnostic, got {codes:?}"
        );
        assert!(
            accepts || !codes.is_empty(),
            "{path}: a rejected decorator program must report a diagnostic"
        );
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            accepts || !text.contains("is not emitted yet"),
            "{path}: a rejected decorator program must be rejected by the checker, not only by the emitter"
        );
    }
}

#[test]
fn bluetsc_build_emits_every_accepted_namespace_fixture_and_nothing_for_a_rejected_one() {
    let root = env::temp_dir().join(format!("bluets-namespace-matrix-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let deferred = deferred_entries();
    for (index, (path, accepts)) in recorded_rows().into_iter().enumerate() {
        let accepts = accepts && !deferred.contains(&path);
        let out_dir = root.join(index.to_string());
        let built = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .arg("build")
            .arg(fixtures().join(&path))
            .arg("--out-dir")
            .arg(&out_dir)
            .args(bluetsc_flags(&path))
            .output()
            .unwrap();
        assert_eq!(built.status.success(), accepts, "{path}");
        let produced = out_dir.exists()
            && fs::read_dir(&out_dir)
                .map(|mut entries| entries.next().is_some())
                .unwrap_or(true);
        assert_eq!(
            produced, accepts,
            "{path}: output exists exactly when the decorator program is accepted"
        );
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn recorded_verdicts_match_the_pinned_typescript_compiler() {
    let tsc = PathBuf::from(
        env::var_os("BLUEICE_BLUETSC_ORACLE")
            .expect("set BLUEICE_BLUETSC_ORACLE to the TypeScript 5.9.3 tsc executable"),
    );
    let version = Command::new(&tsc).arg("--version").output().unwrap();
    assert!(
        String::from_utf8_lossy(&version.stdout).contains("5.9.3"),
        "BLUEICE_BLUETSC_ORACLE must be the pinned TypeScript 5.9.3 compiler"
    );
    let mut fresh = Vec::new();
    for path in discovered_entries() {
        let output = Command::new(&tsc)
            .args([
                "--target",
                "ES2022",
                "--module",
                "ES2022",
                "--strict",
                "--pretty",
                "false",
                "--allowImportingTsExtensions",
                "--noEmit",
            ])
            .args(tsc_flags(&path))
            .arg(fixtures().join(&path))
            .output()
            .unwrap();
        fresh.push((path, output.status.success()));
    }
    if env::var_os("BLUEICE_WRITE_LEGACY_DECORATORS_MATRIX").is_some() {
        let text: String = fresh
            .iter()
            .map(|(path, accepts)| {
                format!("{path}\t{}\n", if *accepts { "accept" } else { "reject" })
            })
            .collect();
        fs::write(
            fixtures().join("legacy-decorators-checker-matrix.tsv"),
            text.trim_end_matches('\n').to_string() + "\n",
        )
        .unwrap();
        return;
    }
    assert_eq!(
        fresh,
        recorded_rows(),
        "recorded verdicts differ from the pinned compiler"
    );
}
