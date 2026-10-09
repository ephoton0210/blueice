// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Independent syntax witnesses for K.7.1's edition-selecting parser gate.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../bluets/tests/fixtures/target_syntax")
}

fn rows() -> Vec<[&'static str; 4]> {
    include_str!("../../bluets/tests/fixtures/target_syntax/edition-verdicts.tsv")
        .lines()
        .map(|line| line.split('\t').collect::<Vec<_>>().try_into().unwrap())
        .collect()
}

#[test]
fn edition_evidence_covers_each_witness_and_target() {
    let rows = rows();
    assert_eq!(rows.len(), 605);
    assert_eq!(rows.iter().filter(|row| row[3] == "accept").count(), 403);
    assert_eq!(rows.iter().filter(|row| row[3] == "reject").count(), 202);
    let targets = [
        "ES5", "ES2015", "ES2016", "ES2017", "ES2018", "ES2019", "ES2020", "ES2021", "ES2022",
        "ES2023", "ESNext",
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    let mut recorded = BTreeMap::<&str, BTreeSet<&str>>::new();
    for [entry, mode, target, verdict] in rows {
        assert!(matches!(mode, "script" | "module"));
        assert!(matches!(verdict, "accept" | "reject"));
        assert!(recorded.entry(entry).or_default().insert(target));
    }
    assert_eq!(recorded.len(), 55);
    for actual in recorded.values() {
        assert_eq!(*actual, targets);
    }
    let discovered = fs::read_dir(fixtures())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "js"))
        .map(|path| path.file_name().unwrap().to_str().unwrap().to_string())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        recorded
            .keys()
            .map(|entry| entry.to_string())
            .collect::<BTreeSet<_>>(),
        discovered
    );
}

#[test]
fn existing_bluejs_parser_accepts_every_latest_syntax_witness() {
    let mut failures = Vec::new();
    for [entry, mode, target, verdict] in rows() {
        if target != "ESNext" {
            continue;
        }
        assert_eq!(verdict, "accept");
        let source = fs::read_to_string(fixtures().join(entry)).unwrap();
        let parsed = if mode == "module" {
            blueice_bluejs::parse_module(&source).map(|_| ())
        } else {
            blueice_bluejs::parse(&source).map(|_| ())
        };
        if let Err(error) = parsed {
            failures.push(format!("{entry}: {error:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn selected_bluejs_editions_match_every_independent_syntax_observation() {
    use blueice_bluejs::{parse_module_with_edition, parse_with_edition, SyntaxEdition};

    let mut failures = Vec::new();
    for [entry, mode, target, verdict] in rows() {
        let edition = match target {
            "ES5" => SyntaxEdition::Es5,
            "ES2015" => SyntaxEdition::Es2015,
            "ES2016" => SyntaxEdition::Es2016,
            "ES2017" => SyntaxEdition::Es2017,
            "ES2018" => SyntaxEdition::Es2018,
            "ES2019" => SyntaxEdition::Es2019,
            "ES2020" => SyntaxEdition::Es2020,
            "ES2021" => SyntaxEdition::Es2021,
            "ES2022" => SyntaxEdition::Es2022,
            "ES2023" => SyntaxEdition::Es2023,
            "ESNext" => SyntaxEdition::EsNext,
            _ => panic!("unknown recorded target {target}"),
        };
        let source = fs::read_to_string(fixtures().join(entry)).unwrap();
        let parsed = if mode == "module" {
            parse_module_with_edition(&source, edition).map(|_| ())
        } else {
            parse_with_edition(&source, edition).map(|_| ())
        };
        if parsed.is_ok() != (verdict == "accept") {
            failures.push(format!(
                "{entry}/{mode}/{target}: expected {verdict}, got {parsed:?}"
            ));
        }
        if verdict == "reject" {
            if let Err(error) = parsed {
                assert!(
                    error.known_syntax && error.resource.is_none(),
                    "{entry}/{target}: {error:?}"
                );
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
#[ignore = "requires pinned Acorn 8.15.0 and Node"]
fn syntax_edition_observations_match_the_pinned_independent_parser() {
    let recorder = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools/record_syntax_editions.cjs");
    let output = Command::new("node").arg(recorder).output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
