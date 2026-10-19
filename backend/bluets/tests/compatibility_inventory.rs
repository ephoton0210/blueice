// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Source-derived compatibility inventory tooling (K.14.1).

#[path = "fixtures/compatibility_inventory/accounting.rs"]
mod accounting;
#[path = "fixtures/compatibility_inventory/case_inline.rs"]
mod case_inline;
#[path = "fixtures/compatibility_inventory/case_registry.rs"]
mod case_registry;
#[path = "fixtures/compatibility_inventory/case_report.rs"]
mod case_report;
#[path = "fixtures/compatibility_inventory/case_sources.rs"]
mod case_sources;
#[path = "fixtures/compatibility_inventory/case_supplement.rs"]
mod case_supplement;
#[path = "fixtures/compatibility_inventory/inventory.rs"]
mod inventory;
#[path = "fixtures/compatibility_inventory/rust_facts.rs"]
mod rust_facts;
#[path = "fixtures/compatibility_inventory/snapshot.rs"]
mod snapshot;

#[test]
fn row_accounting_preserves_gaps_refusals_and_control_scope() {
    use accounting::Status;
    use case_sources::Comparison;
    for (comparison, status, scope) in [
        (Comparison::Compared, Status::Closed, "compared_properties"),
        (Comparison::Open, Status::Open, "documented_difference"),
        (Comparison::Refused, Status::Refused, "documented_refusal"),
        (
            Comparison::ReferenceOnly,
            Status::Closed,
            "native_reference_control_only",
        ),
        (
            Comparison::BoundaryControl,
            Status::Closed,
            "bluets_owner_control_only",
        ),
    ] {
        let mut case = case_sources::Case::new("source.rs", "function:sample", "sample", "program");
        case.comparison = comparison;
        let row = accounting::case(case, "COMPATIBILITY_MEASUREMENT.json#/suites/0");
        assert_eq!(row.status, status);
        assert_eq!(row.scope, scope);
        assert_eq!(row.evidence.assertion, "source.rs#function:sample");
        assert_eq!(
            row.evidence.measurement,
            "COMPATIBILITY_MEASUREMENT.json#/suites/0"
        );
    }
    assert_eq!(Status::suite(0, 0), Status::Closed);
    assert_eq!(Status::suite(0, 1), Status::Refused);
    assert_eq!(Status::suite(1, 0), Status::Open);
    assert_eq!(Status::suite(1, 1), Status::Open);
}

#[test]
fn every_generated_case_and_suite_has_a_typed_status_and_measurement_pointer() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tree = snapshot::Snapshot::read(&root, "HEAD").unwrap();
    let mut suites = inventory::generate(&tree).unwrap();
    let bytes = std::fs::read(
        root.join(inventory::DIRECTORY)
            .join(case_report::MEASUREMENT),
    )
    .unwrap();
    let measurement: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    suites.source_revision = measurement["source_revision"].as_str().unwrap().into();
    let report = case_report::generate(&tree, &suites, &bytes).unwrap();
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["schema"], 2);
    let mut labels = std::collections::BTreeMap::<String, usize>::new();
    for suite in json["suites"].as_array().unwrap() {
        assert!(matches!(
            suite["status"].as_str(),
            Some("closed" | "open" | "refused")
        ));
        let pointer = suite["evidence"]
            .as_str()
            .unwrap()
            .split_once('#')
            .unwrap()
            .1;
        assert_eq!(
            measurement.pointer(pointer).unwrap()["harness"],
            suite["harness"]
        );
        for case in suite["cases"].as_array().unwrap() {
            let status = case["status"].as_str().unwrap();
            assert!(matches!(status, "closed" | "open" | "refused"));
            *labels.entry(status.into()).or_default() += 1;
            assert_eq!(case["evidence"]["measurement"], suite["evidence"]);
            assert_eq!(case["evidence"]["assertion"], case["id"]);
            assert!(!case["scope"].as_str().unwrap().is_empty());
        }
    }
    assert_eq!(
        labels["closed"],
        report.counts.agreements + report.counts.reference_only + report.counts.boundary_controls
    );
    assert_eq!(labels["open"], report.counts.open);
    assert_eq!(labels["refused"], report.counts.refused);
    let plain = inventory::markdown(&suites, None);
    assert!(plain.contains("| open | [unmeasured source]"));
    let measured = inventory::markdown(&suites, Some(&report));
    assert!(measured.contains("| closed | [measurement]"));
    assert!(measured.contains("| open | [measurement]"));
    assert!(measured.contains("| refused | [measurement]"));
}

#[test]
fn every_feature_and_refusal_row_has_a_scoped_label_and_live_evidence() {
    use std::collections::BTreeSet;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let directory = root.join(inventory::DIRECTORY);
    let document = std::fs::read_to_string(directory.join("COMPATIBILITY_INVENTORY.md")).unwrap();
    let cases: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.join(case_report::ARTIFACT)).unwrap())
            .unwrap();
    let expected: BTreeSet<_> = [("T", 10), ("E", 5), ("D", 2), ("M", 3), ("C", 3)]
        .into_iter()
        .flat_map(|(area, count)| (1..=count).map(move |index| format!("G-{area}{index}")))
        .collect();
    let mut features = BTreeSet::new();
    let mut refusals = BTreeSet::new();
    let mut in_refusals = false;
    let mut areas = 0;
    let mut declared = None;
    for line in document.lines() {
        if line.starts_with("| G-") {
            let fields: Vec<_> = line.strip_prefix("| ").unwrap().splitn(3, " | ").collect();
            assert!(
                features.insert(fields[0].to_string()),
                "duplicate feature row"
            );
            assert_eq!(
                fields[1],
                if fields[0] == "G-C2" {
                    "closed"
                } else {
                    "open"
                }
            );
            let (_, link) = line
                .rsplit_once("[evidence](COMPATIBILITY_CASES.json#/suites/")
                .unwrap();
            let index: usize = link.strip_suffix(") |").unwrap().parse().unwrap();
            assert!(cases["suites"].as_array().unwrap().get(index).is_some());
        }
        in_refusals |= line.starts_with("## 4. Refusals:");
        if !in_refusals {
            continue;
        }
        if line.contains(" refusal sites in ") {
            let fields: Vec<_> = line.split_whitespace().collect();
            declared = Some((
                fields[0].parse::<usize>().unwrap(),
                fields[4].parse::<usize>().unwrap(),
            ));
        }
        areas += usize::from(line.starts_with("### "));
        if !line.starts_with("- ") {
            continue;
        }
        assert!(refusals.insert(line.to_string()), "duplicate refusal row");
        let (location, message) = line
            .strip_prefix("- **refused** `")
            .unwrap()
            .split_once("` — ")
            .unwrap();
        assert!(!message.is_empty());
        let (path, number) = location.rsplit_once(':').unwrap();
        assert!(path.starts_with("backend/") && !path.contains("..") && !path.contains('\\'));
        let source = std::fs::read_to_string(root.join(path)).unwrap();
        let number: usize = number.parse().unwrap();
        let witness = source.lines().nth(number.checked_sub(1).unwrap()).unwrap();
        assert!(
            witness.contains("unsupported(")
                || witness.contains("UnsupportedSyntax")
                || witness.contains("UnsupportedRuntimeTarget"),
            "dangling refusal source: {location}"
        );
    }
    assert_eq!(features, expected);
    assert_eq!(declared, Some((refusals.len(), areas)));
    assert!(document.contains("**Row status: open.** [evidence](COMPATIBILITY_CASES.json#/suites/"));
}

#[test]
fn rust_case_facts_preserve_scope_and_ignore_literal_attribute_text() {
    let source = include_str!("fixtures/compatibility_inventory/samples/harness.rs.txt");
    let facts = rust_facts::parse(source).unwrap();
    assert_eq!(facts.ignored_tests, ["pinned", "nested::second"]);
    assert_eq!(
        facts.includes,
        [
            "fixtures/first.ts",
            "fixtures/in_macro.ts",
            "fixtures/second.ts"
        ]
    );
    assert_eq!(facts.modules.len(), 1);
    assert_eq!(facts.modules[0].name, "external");
    assert_eq!(facts.modules[0].path.as_deref(), Some("support/custom.rs"));
    assert_eq!(facts.arrays.len(), 4);
    assert_eq!(facts.arrays[0].scope, "");
    assert_eq!(facts.arrays[0].binding, "CASES");
    assert_eq!(facts.arrays[0].labels, ["first", "second"]);
    assert_eq!(facts.arrays[1].scope, "pinned");
    assert_eq!(facts.arrays[1].binding, "cases");
    assert_eq!(facts.arrays[1].labels, ["quoted", "raw"]);
    assert_eq!(facts.arrays[2].scope, "pinned");
    assert_eq!(facts.arrays[2].binding, "for@0");
    assert_eq!(facts.arrays[2].labels, ["es2022", "commonjs"]);
    assert_eq!(
        facts.arrays[3].rows,
        [
            vec!["Target::Es2022", "None"],
            vec!["Target::Es2020", "Some(false)"]
        ]
    );
    assert_eq!(facts.program_vectors.len(), 1);
    assert_eq!(facts.program_vectors[0].scope, "programs");
    assert_eq!(
        facts.program_vectors[0].labels,
        ["first-program", "second-program"]
    );
    let controls: Vec<_> = facts
        .calls
        .iter()
        .filter(|call| call.scope == "controls" && call.function == "accepts")
        .collect();
    assert_eq!(controls.len(), 2);
    assert_eq!(
        controls[0].arguments,
        ["const value = 1;", "options", "true"]
    );
}

#[test]
fn malformed_rust_is_an_inventory_error() {
    assert!(rust_facts::parse("fn broken( {").is_err());
}

#[test]
fn measurement_binding_rejects_partial_forged_or_foreign_evidence() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tree = snapshot::Snapshot::read(&root, "HEAD").unwrap();
    let inventory = inventory::generate(&tree).unwrap();
    let rows: Vec<_> = inventory
        .suites
        .iter()
        .map(|suite| {
            serde_json::json!({
                "harness": suite.harness, "test_names": suite.ignored_tests,
                "pinned_tests": suite.ignored_tests, "conclusion":"success",
                "passed":suite.ignored_tests.len(), "failed":0,"ignored":0,"filtered":0,
                "log_sha256":"a".repeat(64)
            })
        })
        .collect();
    let valid = serde_json::json!({"schema":1,"typescript":"5.9.3",
        "source_revision":inventory.source_revision,"source_graph_sha256":inventory.source_graph_sha256,
        "platform":"darwin-arm64","conclusion":"success","finished_at":"2026-10-16T13:06:01.804Z","suites":rows});
    case_report::validate_measurement(&inventory, &valid).unwrap();
    for (pointer, value) in [
        ("/conclusion", serde_json::json!("in_progress")),
        ("/source_graph_sha256", serde_json::json!("foreign")),
        ("/source_revision", serde_json::json!("foreign")),
        ("/typescript", serde_json::json!("5.8.0")),
        ("/platform", serde_json::json!("win32-x64")),
        ("/finished_at", serde_json::Value::Null),
        ("/suites/0/failed", serde_json::json!(1)),
        ("/suites/0/ignored", serde_json::json!(1)),
        ("/suites/0/filtered", serde_json::json!(1)),
        ("/suites/0/passed", serde_json::json!(0)),
        ("/suites/0/log_sha256", serde_json::json!("missing")),
        ("/suites/0/pinned_tests", serde_json::json!([])),
        ("/suites/0/test_names", serde_json::json!([])),
        ("/suites/0/harness", serde_json::json!("foreign")),
    ] {
        let mut bad = valid.clone();
        *bad.pointer_mut(pointer).unwrap() = value;
        assert!(
            case_report::validate_measurement(&inventory, &bad).is_err(),
            "{pointer}"
        );
    }
    let mut missing = valid.clone();
    missing["suites"].as_array_mut().unwrap().pop();
    assert!(case_report::validate_measurement(&inventory, &missing).is_err());
    let mut duplicate = valid.clone();
    duplicate["suites"][1] = duplicate["suites"][0].clone();
    assert!(case_report::validate_measurement(&inventory, &duplicate).is_err());
    let report =
        case_report::generate(&tree, &inventory, &serde_json::to_vec(&valid).unwrap()).unwrap();
    assert_eq!(report.counts.listed, report.counts.checked);
    assert_eq!(
        report.counts.comparison_cases,
        report.counts.agreements + report.counts.open + report.counts.refused
    );
    assert_eq!(
        report.counts.listed,
        report.counts.comparison_cases
            + report.counts.reference_only
            + report.counts.boundary_controls
    );
    assert!(report.counts.open > 0 && report.counts.refused > 0);
    let text = case_report::markdown(&report);
    let original = "intro\n## 2. Subset\nunchanged";
    let once = case_report::replace_section(original, &text).unwrap();
    assert_eq!(case_report::replace_section(&once, &text).unwrap(), once);
    assert!(case_report::replace_section(case_report::START, &text).is_err());
    assert_eq!(
        case_report::digest(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(case_report::ARTIFACT, "COMPATIBILITY_CASES.json");
    assert_eq!(case_report::MEASUREMENT, "COMPATIBILITY_MEASUREMENT.json");
}

#[test]
fn recorded_case_rates_regenerate_from_the_source_and_complete_measurement() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let directory = root.join(inventory::DIRECTORY);
    let tree = snapshot::Snapshot::read(&root, "HEAD").unwrap();
    let mut suites = inventory::generate(&tree).unwrap();
    let recorded: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.join(case_report::ARTIFACT)).unwrap())
            .unwrap();
    assert_eq!(recorded["source_graph_sha256"], suites.source_graph_sha256);
    suites.source_revision = recorded["source_revision"].as_str().unwrap().to_string();
    let bytes = std::fs::read(directory.join(case_report::MEASUREMENT)).unwrap();
    let actual = case_report::generate(&tree, &suites, &bytes).unwrap();
    assert_eq!(serde_json::to_value(&actual).unwrap(), recorded);
    let document = std::fs::read_to_string(directory.join("COMPATIBILITY_INVENTORY.md")).unwrap();
    assert_eq!(
        case_report::replace_section(&document, &case_report::markdown(&actual)).unwrap(),
        document
    );
}

#[test]
fn json_case_identity_keeps_configurations_with_the_same_entry_separate() {
    let source = r#"{"version":"5.9.3","cases":[{"entry":"a.ts","target":"ES5"},{"entry":"a.ts","target":"ES2022"}]}"#;
    let cases = case_sources::json_cases("record.json", source, "/cases", "configuration").unwrap();
    assert_eq!(cases.len(), 2);
    assert_eq!(cases[0].label, cases[1].label);
    assert_ne!(cases[0].id, cases[1].id);
    assert_eq!(cases[1].selector, "/cases/1");
    for bad in [
        r#"{"version":"5.8.0","cases":[{}]}"#,
        r#"{"typescript":"5.9.2","cases":[{}]}"#,
        r#"{"cases":[{"id":"same"},{"id":"same"}]}"#,
        r#"{"cases":[]}"#,
        r#"{"cases":[true]}"#,
    ] {
        assert!(case_sources::json_cases("bad.json", bad, "/cases", "program").is_err());
    }
    assert!(case_sources::json_cases("missing.json", source, "/observations", "program").is_err());
    assert_eq!(
        case_sources::json_cases("cli.json", r#"[{"name":"native"}]"#, "", "configuration")
            .unwrap()[0]
            .label,
        "native"
    );
}

#[test]
fn tsv_case_sources_preserve_header_offsets_and_reject_silent_row_loss() {
    let cases = case_sources::tsv_cases(
        "matrix.tsv",
        "case\tverdict\na\taccept\nb\treject\n",
        true,
        "program",
    )
    .unwrap();
    assert_eq!(cases.len(), 2);
    assert_eq!(cases[0].selector, "line:2");
    for bad in [
        "",
        "a",
        "\taccept",
        "a\taccept\na\taccept\n",
        "a\taccept\nb\treject\textra\n",
    ] {
        assert!(case_sources::tsv_cases("bad.tsv", bad, false, "program").is_err());
    }
}

#[test]
fn known_gaps_and_refusals_remain_in_the_denominator_but_not_the_agreement_numerator() {
    use case_sources::{classify, Comparison};
    let mut cases = case_sources::json_cases(
        "record.json",
        r#"{"cases":[{"id":"scope-ok"},{"id":"scope-gap"},{"id":"scope-refused"}]}"#,
        "/cases",
        "program",
    )
    .unwrap();
    classify(
        &mut cases,
        &["gap".into()],
        "scope-",
        Comparison::Open,
        "known checker gap",
    )
    .unwrap();
    classify(
        &mut cases,
        &["refused".into()],
        "scope-",
        Comparison::Refused,
        "owner/subset policy",
    )
    .unwrap();
    assert_eq!(cases.len(), 3);
    assert_eq!(
        cases
            .iter()
            .filter(|case| case.comparison == Comparison::Compared)
            .count(),
        1
    );
    assert_eq!(cases[1].reason.as_deref(), Some("known checker gap"));
    assert!(classify(
        &mut cases,
        &["missing".into()],
        "scope-",
        Comparison::Open,
        "gap"
    )
    .is_err());
    assert!(classify(
        &mut cases,
        &["gap".into()],
        "scope-",
        Comparison::Open,
        "gap"
    )
    .is_err());
}

#[test]
fn source_family_adapter_counts_the_actual_known_gaps_and_refusals() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tree = snapshot::Snapshot::read(&root, "HEAD").unwrap();
    let suites = inventory::generate(&tree).unwrap();
    let suite = suites
        .suites
        .iter()
        .find(|suite| suite.harness.ends_with("/diagnostic_source_families.rs"))
        .unwrap();
    let cases = case_registry::primary(&tree, suite).unwrap().unwrap();
    let facts = rust_facts::parse(tree.source(&suite.harness).unwrap()).unwrap();
    for (binding, comparison) in [
        ("known_checker_gaps", case_sources::Comparison::Open),
        ("subset_refusals", case_sources::Comparison::Refused),
    ] {
        let expected = &facts
            .arrays
            .iter()
            .find(|array| array.binding == binding)
            .unwrap()
            .labels;
        assert_eq!(
            cases
                .iter()
                .filter(|case| case.comparison == comparison)
                .count(),
            expected.len()
        );
    }
    assert_eq!(
        cases.len(),
        serde_json::from_str::<serde_json::Value>(
            tree.source("backend/bluets/tests/fixtures/diagnostics/source-families.json")
                .unwrap()
        )
        .unwrap()["cases"]
            .as_array()
            .unwrap()
            .len()
    );
}

#[test]
fn case_source_audit_accounts_for_each_suite_without_publishing_a_partial_pass_rate() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tree = snapshot::Snapshot::read(&root, "HEAD").unwrap();
    let inventory = inventory::generate(&tree).unwrap();
    let audit = case_registry::audit(&tree, &inventory.suites).unwrap();
    assert_eq!(audit["publishable"], false);
    let primary = audit["primary_sources"].as_array().unwrap();
    let missing = audit["requires_inline_provider"].as_array().unwrap();
    assert_eq!(primary.len() + missing.len(), inventory.suites.len());
    assert!(primary
        .iter()
        .all(|row| row["primary_observations"].as_u64().unwrap() > 0));
}

#[test]
fn repository_paths_cannot_escape_or_use_host_specific_roots() {
    assert_eq!(
        snapshot::relative("backend/bluets/tests", "../../bluets-bluejs/tests/a.rs").unwrap(),
        "backend/bluets-bluejs/tests/a.rs"
    );
    for path in [
        "../../../../private.ts",
        "/tmp/private.ts",
        "C:/private.ts",
        "..\\private.ts",
    ] {
        assert!(
            snapshot::relative("backend/bluets/tests", path).is_err(),
            "{path}"
        );
    }
}

#[test]
fn native_only_core_controls_never_inflate_direct_agreement() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tree = snapshot::Snapshot::read(&root, "HEAD").unwrap();
    let inventory = inventory::generate(&tree).unwrap();
    let suite = inventory
        .suites
        .iter()
        .find(|suite| suite.harness.ends_with("/typescript_oracle.rs"))
        .unwrap();
    let facts = rust_facts::parse(tree.source(&suite.harness).unwrap()).unwrap();
    let cases = case_inline::programs(&tree, suite).unwrap().unwrap();
    let primary = case_inline::array(&facts, "", "CASES").unwrap();
    assert_eq!(
        cases
            .iter()
            .filter(|case| case.comparison == case_sources::Comparison::Compared)
            .count(),
        primary.labels.len()
    );
    let controls: Vec<_> = cases
        .iter()
        .filter(|case| case.comparison == case_sources::Comparison::ReferenceOnly)
        .collect();
    assert!(!controls.is_empty());
    assert!(controls.iter().all(|case| case
        .reason
        .as_deref()
        .is_some_and(|reason| !reason.is_empty())));
    assert!(controls
        .iter()
        .any(|case| case.selector.contains("pinned_static_class_method_shell")));
    assert!(controls
        .iter()
        .any(|case| case.selector.contains("entry-write")));
}

#[test]
fn supplementary_cases_distinguish_comparison_sides_and_owner_only_controls() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tree = snapshot::Snapshot::read(&root, "HEAD").unwrap();
    let inventory = inventory::generate(&tree).unwrap();
    let project = inventory
        .suites
        .iter()
        .find(|suite| suite.harness.ends_with("/project_config.rs"))
        .unwrap();
    let extras = case_supplement::additional(&tree, project).unwrap();
    assert_eq!(
        extras
            .iter()
            .filter(|case| case.comparison == case_sources::Comparison::Compared)
            .count(),
        1
    );
    assert_eq!(
        extras
            .iter()
            .filter(|case| case.comparison == case_sources::Comparison::BoundaryControl)
            .count(),
        1
    );
    for suite in &inventory.suites {
        let cases = case_registry::cases(&tree, suite).unwrap();
        assert!(!cases.is_empty(), "{}", suite.harness);
        assert_eq!(
            cases
                .iter()
                .map(|case| &case.id)
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            cases.len(),
            "{}",
            suite.harness
        );
        assert!(cases
            .iter()
            .all(|case| tree.blobs.contains_key(&case.source)));
    }
}

#[test]
fn map_configuration_products_keep_their_versioned_matrix_origin() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tree = snapshot::Snapshot::read(&root, "HEAD").unwrap();
    let inventory = inventory::generate(&tree).unwrap();
    for stem in ["source_map_publication", "source_map_tokens_oracle"] {
        let suite = inventory
            .suites
            .iter()
            .find(|suite| suite.harness.ends_with(&format!("/{stem}.rs")))
            .unwrap();
        let base = case_registry::primary(&tree, suite).unwrap().unwrap();
        let cases = case_supplement::configurations(&tree, suite, base.clone()).unwrap();
        assert!(cases.len() > base.len());
        assert!(cases
            .iter()
            .all(|case| case.source.ends_with(".tsv") && case.selector.starts_with("line:")));
    }
}

#[test]
fn generated_section_is_idempotent_and_rejects_broken_markers() {
    let replacement = format!("{}\ngenerated\n{}", inventory::START, inventory::END);
    let original =
        "intro\n## 1. What is compared with `tsc`, and how\nold\n## 2. Subset\nunchanged";
    let once = inventory::replace_section(original, &replacement).unwrap();
    assert!(once.starts_with("intro\n"));
    assert!(once.ends_with("## 2. Subset\nunchanged"));
    assert!(!once.contains("old"));
    assert_eq!(
        inventory::replace_section(&once, &replacement).unwrap(),
        once
    );
    for invalid in [
        inventory::START.to_string(),
        inventory::END.to_string(),
        format!("{replacement}{}", inventory::START),
        format!("{}{}", inventory::END, inventory::START),
    ] {
        assert!(inventory::replace_section(&invalid, &replacement).is_err());
    }
}

#[test]
fn recorded_suite_graph_matches_the_current_committed_inputs() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tree = snapshot::Snapshot::read(&root, "HEAD").unwrap();
    let mut actual = inventory::generate(&tree).unwrap();
    let recorded: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join(inventory::DIRECTORY).join(inventory::ARTIFACT))
            .unwrap(),
    )
    .unwrap();
    actual.source_revision = recorded["source_revision"].as_str().unwrap().to_string();
    assert_eq!(
        actual.source_graph_sha256,
        recorded["source_graph_sha256"].as_str().unwrap(),
        "committed source/fixture tree differs from the measured revision"
    );
    assert_eq!(serde_json::to_value(&actual).unwrap(), recorded);
    let bytes = std::fs::read(
        root.join(inventory::DIRECTORY)
            .join(case_report::MEASUREMENT),
    )
    .unwrap();
    let report = case_report::generate(&tree, &actual, &bytes).unwrap();
    let document = std::fs::read_to_string(
        root.join(inventory::DIRECTORY)
            .join("COMPATIBILITY_INVENTORY.md"),
    )
    .unwrap();
    assert_eq!(
        inventory::replace_section(&document, &inventory::markdown(&actual, Some(&report)))
            .unwrap(),
        document
    );
    let maps = actual
        .suites
        .iter()
        .find(|suite| suite.harness.ends_with("/source_map_tokens_oracle.rs"))
        .unwrap();
    for name in [
        "legacy_metadata::legacy_parameter_and_metadata_frames_match_native",
        "non_decorator::object_spread_helper_frames_match_native",
        "system_wrapper::system_wrapper_frames_match_native",
    ] {
        assert!(
            maps.ignored_tests.iter().any(|test| test == name),
            "missing child module {name}"
        );
    }
    assert!(!actual
        .suites
        .iter()
        .any(|suite| suite.harness.ends_with("/compatibility_inventory.rs")));
}

#[test]
fn git_batch_decoder_rejects_corruption_and_does_not_split_body_newlines() {
    let path = "source.rs".to_string();
    let blob = "0123456789abcdef".to_string();
    let selected = [(&path, &blob)];
    let good = b"0123456789abcdef blob 4\na\nb\n\n";
    assert_eq!(
        snapshot::decode_batch(good, &selected).unwrap()[&path],
        "a\nb\n"
    );
    for invalid in [
        b"0123456789abcdef blob 4".as_slice(),
        b"other blob 0\n\n",
        b"0123456789abcdef tree 0\n\n",
        b"0123456789abcdef blob no\n\n",
        b"0123456789abcdef blob 4\nabc",
        b"0123456789abcdef blob 4\nabcdX",
        b"0123456789abcdef blob 1\n\xff\n",
        b"0123456789abcdef blob 0\n\nextra",
    ] {
        assert!(
            snapshot::decode_batch(invalid, &selected).is_err(),
            "{invalid:?}"
        );
    }
}

#[test]
fn immutable_tree_resolves_nested_modules_and_refuses_missing_inputs() {
    let fixture = GitFixture::new();
    fixture.write(
        "backend/bluets/tests/oracle.rs",
        "mod nested { #[path=\"custom.rs\"] mod child; }",
    );
    fixture.write(
        "backend/bluets/tests/nested/custom.rs",
        "#[test] #[ignore] fn witness() { let _ = include_str!(\"input.ts\"); }",
    );
    fixture.write(
        "backend/bluets/tests/nested/input.ts",
        "export const value = 1;\n",
    );
    fixture.write(
        "backend/bluets/tests/fixtures/diagnostics/reference.json",
        r#"{"version":"5.9.3","templates":[],"cases":[]}"#,
    );
    let unicode = "backend/bluets/tests/fixtures/unicode-冰-😀/main.ts";
    fixture.write(unicode, "export const value = 1;\n");
    let production = "backend/bluets/src/compatibility_inventory.rs";
    fixture.write(production, "pub const INCLUDED: bool = true;\n");
    let data = "backend/bluets/tests/fixtures/recorded-programs.json";
    fixture.write(data, r#"{"version":"5.9.3","cases":[{"id":"program"}]}"#);
    let revision = fixture.commit();
    let tree = snapshot::Snapshot::read(&fixture.0, &revision).unwrap();
    assert!(
        tree.blobs.contains_key(unicode),
        "Git paths must not retain display quoting"
    );
    assert!(
        tree.blobs.contains_key(production),
        "tooling exclusions must not hide production source"
    );
    assert!(tree.source(data).unwrap().contains("program"));
    let measured = inventory::generate(&tree).unwrap();
    assert_eq!(measured.suites.len(), 1);
    assert_eq!(measured.suites[0].ignored_tests, ["nested::child::witness"]);
    assert!(measured.suites[0]
        .inputs
        .contains_key("backend/bluets/tests/nested/input.ts"));
    // Later dirty edits cannot contaminate evidence for the frozen revision.
    fixture.write("backend/bluets/tests/nested/custom.rs", "not valid Rust");
    assert_eq!(
        inventory::generate(&snapshot::Snapshot::read(&fixture.0, &revision).unwrap())
            .unwrap()
            .source_graph_sha256,
        measured.source_graph_sha256
    );
    // Committed missing/escaping inputs fail closed rather than silently reducing counts.
    for input in ["missing.ts", "../../../../../../private.ts"] {
        fixture.write(
            "backend/bluets/tests/nested/custom.rs",
            &format!("#[test] #[ignore] fn witness() {{ let _ = include_str!({input:?}); }}"),
        );
        let revision = fixture.commit();
        assert!(
            inventory::generate(&snapshot::Snapshot::read(&fixture.0, &revision).unwrap()).is_err()
        );
    }
    fixture.write("backend/bluets/tests/oracle.rs", "mod absent;");
    let revision = fixture.commit();
    assert!(
        inventory::generate(&snapshot::Snapshot::read(&fixture.0, &revision).unwrap()).is_err()
    );
}

struct GitFixture(std::path::PathBuf);

impl GitFixture {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "bluets-k14-inventory-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&directory).unwrap();
        let fixture = Self(directory);
        fixture.git(&["init", "--quiet"]);
        fixture
    }

    fn write(&self, path: &str, text: &str) {
        let path = self.0.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn git(&self, arguments: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .current_dir(&self.0)
            .args(arguments)
            .env("GIT_AUTHOR_NAME", "Inventory fixture")
            .env("GIT_COMMITTER_NAME", "Inventory fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    fn commit(&self) -> String {
        self.git(&[
            "-c",
            "core.autocrlf=false",
            "-c",
            "core.safecrlf=false",
            "add",
            ".",
        ]);
        let tree = self.git(&["write-tree"]);
        self.git(&[
            "-c",
            "commit.gpgsign=false",
            "commit-tree",
            &tree,
            "-m",
            "inventory fixture",
        ])
    }
}

impl Drop for GitFixture {
    fn drop(&mut self) {
        // Remove only the unique directory successfully created by this test.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
