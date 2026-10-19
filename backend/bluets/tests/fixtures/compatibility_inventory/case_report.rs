// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bind case-level denominators to a complete, source-matched suite run.

use super::case_sources::{Case, Comparison};
use super::inventory::{Inventory, Suite};
use super::snapshot::{Result, Snapshot};
use super::{accounting, case_registry};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const ARTIFACT: &str = "COMPATIBILITY_CASES.json";
pub const MEASUREMENT: &str = "COMPATIBILITY_MEASUREMENT.json";
pub const START: &str = "<!-- BEGIN GENERATED K14 CASES -->";
pub const END: &str = "<!-- END GENERATED K14 CASES -->";

#[derive(Debug, Default, Serialize)]
pub struct Counts {
    pub listed: usize,
    pub checked: usize,
    pub agreements: usize,
    pub comparison_cases: usize,
    pub open: usize,
    pub refused: usize,
    pub reference_only: usize,
    pub boundary_controls: usize,
}

impl Counts {
    fn cases(cases: &[Case]) -> Self {
        let count = |kind| cases.iter().filter(|case| case.comparison == kind).count();
        let agreements = count(Comparison::Compared);
        let open = count(Comparison::Open);
        let refused = count(Comparison::Refused);
        Self {
            listed: cases.len(),
            checked: cases.len(),
            agreements,
            comparison_cases: agreements + open + refused,
            open,
            refused,
            reference_only: count(Comparison::ReferenceOnly),
            boundary_controls: count(Comparison::BoundaryControl),
        }
    }

    fn add(&mut self, other: &Self) {
        self.listed += other.listed;
        self.checked += other.checked;
        self.agreements += other.agreements;
        self.comparison_cases += other.comparison_cases;
        self.open += other.open;
        self.refused += other.refused;
        self.reference_only += other.reference_only;
        self.boundary_controls += other.boundary_controls;
    }
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub schema: u32,
    pub typescript: String,
    pub source_revision: String,
    pub source_graph_sha256: String,
    pub measurement_sha256: String,
    pub measured_on: String,
    pub counts: Counts,
    pub suites: Vec<SuiteReport>,
}

#[derive(Debug, Serialize)]
pub struct SuiteReport {
    pub harness: String,
    pub status: accounting::Status,
    pub evidence: String,
    pub fixture_directories: Vec<String>,
    pub counts: Counts,
    pub log_sha256: String,
    pub cases: Vec<accounting::AccountedCase>,
}

pub fn digest(bytes: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, bytes)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// No partial run, missing target, changed test set or foreign source graph
/// can become a case-level numerator simply because one Rust process exited 0.
pub fn validate_measurement(inventory: &Inventory, measurement: &Value) -> Result<()> {
    if measurement["schema"] != 1
        || measurement["typescript"] != inventory.typescript
        || measurement["source_graph_sha256"] != inventory.source_graph_sha256
        || measurement["source_revision"] != inventory.source_revision
        || measurement["conclusion"] != "success"
        || !measurement["finished_at"].is_string()
    {
        return Err(
            "measurement is incomplete, unpinned or belongs to a different source tree".into(),
        );
    }
    if !measurement["platform"]
        .as_str()
        .is_some_and(|platform| platform.starts_with("darwin-") || platform.starts_with("linux-"))
    {
        return Err("complete-case measurement requires a Unix oracle run; Windows exclusions need separate accounting".into());
    }
    let rows = measurement["suites"]
        .as_array()
        .ok_or("missing measured suites")?;
    if rows.len() != inventory.suites.len() {
        return Err("measurement suite set is incomplete".into());
    }
    let mut measured = BTreeMap::new();
    for row in rows {
        let name = row["harness"].as_str().ok_or("missing measured harness")?;
        if measured.insert(name, row).is_some() {
            return Err("duplicate measured harness".into());
        }
    }
    for suite in &inventory.suites {
        let row = measured
            .get(suite.harness.as_str())
            .ok_or("unmeasured oracle suite")?;
        let all = row["test_names"]
            .as_array()
            .ok_or("missing measured test names")?;
        let ignored = row["pinned_tests"]
            .as_array()
            .ok_or("missing selected ignored-test names")?;
        let strings = |values: &[Value]| -> Result<BTreeSet<String>> {
            values
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .map(String::from)
                        .ok_or_else(|| "non-string test name".into())
                })
                .collect()
        };
        let all_set = strings(all)?;
        let ignored_set = strings(ignored)?;
        let expected: BTreeSet<_> = suite.ignored_tests.iter().cloned().collect();
        let sha = row["log_sha256"].as_str().unwrap_or("");
        if row["conclusion"] != "success"
            || row["failed"] != 0
            || row["ignored"] != 0
            || row["filtered"] != 0
            || row["passed"].as_u64() != Some(all.len() as u64)
            || all.is_empty()
            || all_set.len() != all.len()
            || ignored_set.len() != ignored.len()
            || ignored_set != expected
            || !ignored_set.is_subset(&all_set)
            || sha.len() != 64
            || !sha.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(format!(
                "partial, failed or source-mismatched measurement: {}",
                suite.harness
            )
            .into());
        }
    }
    Ok(())
}

pub fn generate(snapshot: &Snapshot, inventory: &Inventory, bytes: &[u8]) -> Result<Report> {
    let measurement: Value = serde_json::from_slice(bytes)?;
    validate_measurement(inventory, &measurement)?;
    let mut counts = Counts::default();
    let mut suites = Vec::new();
    for suite in &inventory.suites {
        let cases = case_registry::cases(snapshot, suite)?;
        let unique: BTreeSet<_> = cases.iter().map(|case| &case.id).collect();
        if cases.is_empty()
            || unique.len() != cases.len()
            || cases.iter().any(|case| {
                !snapshot.blobs.contains_key(&case.source)
                    || (case.comparison != Comparison::Compared
                        && case.reason.as_deref().is_none_or(str::is_empty))
            })
        {
            return Err(format!("unaccounted or duplicate case source: {}", suite.harness).into());
        }
        let row = measurement["suites"]
            .as_array()
            .ok_or("missing measurement rows")?
            .iter()
            .enumerate()
            .find(|(_, row)| row["harness"] == suite.harness)
            .ok_or("missing measurement row")?;
        let (index, row) = row;
        let evidence = format!("{MEASUREMENT}#/suites/{index}");
        let total = Counts::cases(&cases);
        counts.add(&total);
        suites.push(SuiteReport {
            harness: suite.harness.clone(),
            status: accounting::Status::suite(total.open, total.refused),
            evidence: evidence.clone(),
            fixture_directories: fixture_directories(snapshot, suite, &cases)?,
            counts: total,
            log_sha256: row["log_sha256"]
                .as_str()
                .ok_or("missing evidence hash")?
                .to_string(),
            cases: cases
                .into_iter()
                .map(|case| accounting::case(case, &evidence))
                .collect(),
        });
    }
    Ok(Report {
        schema: 2,
        typescript: inventory.typescript.to_string(),
        source_revision: inventory.source_revision.clone(),
        source_graph_sha256: inventory.source_graph_sha256.clone(),
        measurement_sha256: digest(bytes),
        measured_on: measurement["platform"]
            .as_str()
            .ok_or("missing platform")?
            .to_string(),
        counts,
        suites,
    })
}

fn fixture_directories(snapshot: &Snapshot, suite: &Suite, cases: &[Case]) -> Result<Vec<String>> {
    let corpus = "backend/bluets/tests/fixtures/typescript_oracle/";
    let mut directories = BTreeSet::new();
    let mut paths: BTreeSet<_> = suite.inputs.keys().cloned().collect();
    paths.extend(cases.iter().map(|case| case.source.clone()));
    for path in paths {
        if !path.contains("/fixtures/") {
            continue;
        }
        let (parent, _) = path
            .rsplit_once('/')
            .ok_or("invalid fixture definition path")?;
        directories.insert(parent.to_string());
    }
    let mut records = BTreeMap::<&str, Value>::new();
    for source in cases
        .iter()
        .map(|case| case.source.as_str())
        .filter(|source| source.ends_with(".json"))
    {
        if !records.contains_key(source) {
            records.insert(source, serde_json::from_str(snapshot.source(source)?)?);
        }
    }
    for case in cases {
        let record = if case.source.ends_with(".json") {
            let root = &records[case.source.as_str()];
            root.pointer(&case.selector).cloned()
        } else if case.source.ends_with(".tsv") {
            let base = case
                .selector
                .split(['*', ';'])
                .next()
                .ok_or("invalid matrix selector")?;
            let line: usize = base
                .strip_prefix("line:")
                .ok_or("invalid matrix selector")?
                .parse()?;
            snapshot
                .source(&case.source)?
                .lines()
                .nth(line - 1)
                .and_then(|row| row.split('\t').next())
                .map(|entry| serde_json::json!({"entry":entry}))
        } else {
            None
        };
        if let Some(record) = record {
            if let Some(entry) = record["entry"].as_str() {
                let candidate = format!("{corpus}{entry}");
                if snapshot.blobs.contains_key(&candidate) {
                    directories.insert(
                        candidate
                            .rsplit_once('/')
                            .ok_or("invalid corpus entry")?
                            .0
                            .to_string(),
                    );
                }
            }
            if let Some(id) = record["id"].as_str() {
                let parent = case.source.rsplit_once('/').ok_or("invalid case source")?.0;
                let prefix = format!("{parent}/native/{id}/");
                if snapshot
                    .blobs
                    .range(prefix.clone()..)
                    .next()
                    .is_some_and(|(path, _)| path.starts_with(&prefix))
                {
                    directories.insert(prefix.trim_end_matches('/').to_string());
                }
            }
        }
    }
    Ok(directories.into_iter().collect())
}

pub fn markdown(report: &Report) -> String {
    let total = &report.counts;
    let percentage = |numerator: usize, denominator: usize| {
        if denominator == 0 {
            "n/a".into()
        } else {
            format!("{:.4}%", 100.0 * numerator as f64 / denominator as f64)
        }
    };
    let mut text = format!("{START}\n\n### Versioned case pass rate (AC-K14.2)\n\n\
        Pinned TypeScript **{}**; source `{}`; measured on `{}`.\n\
        [COMPATIBILITY_CASES.json](COMPATIBILITY_CASES.json) lists every suite-qualified case and its origin selector.\n\
        [COMPATIBILITY_MEASUREMENT.json](COMPATIBILITY_MEASUREMENT.json) records the complete suite run and per-log SHA-256.\n\n\
        Case assertions: **{} / {} ({})**. Direct agreements: **{} / {} ({})**, with **{} open** and **{} refused** comparisons.\n\
        **{} native reference controls** and **{} BlueTS-only owner controls** are listed separately and never inflate direct agreement.\n\n\
        A case is a versioned program/matrix row or an explicit supplemental program/configuration probe, not a Rust test function or a compiler invocation.\n\
        Inputs overlap across suites; these are observations, not globally unique programs or a percentage of the TypeScript language.\n\
        Rechecking a row's fields, repeating a deterministic compile, or comparing native/BlueTS output directories does not add a case.\n\
        Explicit configuration axes are expanded. A passing assertion can confirm a documented gap/refusal; it is not an agreement.\n\
        Fixture columns count unique versioned fixture-definition directories (including record files); inline-only programs may have none.\n\
        This measurement is Unix-scoped; Windows exclusions are not silently credited. Full parity remains off.\n\n\
        Every row has a closed/open/refused label and evidence pointer in the schema-2 case inventory.\n\
        Closed means only the asserted finite properties (or explicitly scoped reference/owner controls); it is not whole-feature parity.\n\
        Suite status is open if any case is open, otherwise refused if any case is refused, otherwise closed.\n\n\
        | Suite | Status | Evidence | Case assertions | Direct agreement | Open | Refused | Native / owner controls | Fixture dirs |\n\
        | --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |\n",
        report.typescript, report.source_revision, report.measured_on,
        total.checked, total.listed, percentage(total.checked,total.listed), total.agreements,total.comparison_cases,
        percentage(total.agreements,total.comparison_cases),total.open,total.refused,total.reference_only,total.boundary_controls);
    for suite in &report.suites {
        let c = &suite.counts;
        text.push_str(&format!(
            "| `{}` | {} | [measurement]({}) | {} / {} | {} / {} | {} | {} | {} / {} | {} |\n",
            suite
                .harness
                .trim_start_matches("backend/")
                .trim_end_matches(".rs"),
            suite.status.as_str(),
            suite.evidence,
            c.checked,
            c.listed,
            c.agreements,
            c.comparison_cases,
            c.open,
            c.refused,
            c.reference_only,
            c.boundary_controls,
            suite.fixture_directories.len()
        ));
    }
    text.push_str(&format!("\n{END}"));
    text
}

pub fn replace_section(document: &str, replacement: &str) -> Result<String> {
    match (document.find(START), document.find(END)) {
        (Some(start), Some(end))
            if start < end
                && document.matches(START).count() == 1
                && document.matches(END).count() == 1 =>
        {
            Ok(format!(
                "{}{}{}",
                &document[..start],
                replacement,
                &document[end + END.len()..]
            ))
        }
        (None, None) => {
            let at = document
                .find("## 2.")
                .ok_or("missing supported-subset heading")?;
            Ok(format!(
                "{}{}\n\n{}",
                &document[..at],
                replacement,
                &document[at..]
            ))
        }
        _ => Err("invalid case section markers".into()),
    }
}
