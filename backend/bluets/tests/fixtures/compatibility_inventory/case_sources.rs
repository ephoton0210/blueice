// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Versioned observations, not Rust test-function denominators.

use super::snapshot::Result;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Comparison {
    Compared,
    Open,
    Refused,
    ReferenceOnly,
    BoundaryControl,
}

#[derive(Clone, Debug, Serialize)]
pub struct Case {
    pub id: String,
    pub label: String,
    pub unit: String,
    pub source: String,
    pub selector: String,
    pub comparison: Comparison,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl Case {
    pub fn new(source: &str, selector: &str, label: &str, unit: &str) -> Self {
        Self {
            id: format!("{source}#{selector}"),
            label: label.to_string(),
            unit: unit.to_string(),
            source: source.to_string(),
            selector: selector.to_string(),
            comparison: Comparison::Compared,
            reason: None,
        }
    }
}

pub fn json_cases(source: &str, text: &str, pointer: &str, unit: &str) -> Result<Vec<Case>> {
    let root: Value = serde_json::from_str(text)?;
    for key in ["version", "typescript"] {
        if let Some(version) = root.get(key) {
            if version != "5.9.3" {
                return Err(format!("{source}: unpinned {key}: {version}").into());
            }
        }
    }
    let rows = root
        .pointer(pointer)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{source}: missing case array at {pointer:?}"))?;
    if rows.is_empty() {
        return Err(format!("{source}: empty case list").into());
    }
    let mut unique = BTreeSet::new();
    rows.iter()
        .enumerate()
        .map(|(index, row)| {
            if !row.is_object() {
                return Err(format!("{source}: non-object case {index}").into());
            }
            if !unique.insert(serde_json::to_string(row)?) {
                return Err(format!("{source}: duplicate observation at {pointer}/{index}").into());
            }
            let label = ["id", "name", "entry"]
                .into_iter()
                .find_map(|key| row[key].as_str())
                .map(String::from)
                .unwrap_or_else(|| format!("observation-{index}"));
            Ok(Case::new(
                source,
                &format!("{pointer}/{index}"),
                &label,
                unit,
            ))
        })
        .collect()
}

pub fn tsv_cases(source: &str, text: &str, header: bool, unit: &str) -> Result<Vec<Case>> {
    let mut rows = Vec::new();
    let mut width = None;
    let mut unique = BTreeSet::new();
    for (index, line) in text.lines().enumerate() {
        let fields: Vec<_> = line.split('\t').collect();
        if fields.len() < 2 || fields[0].is_empty() {
            return Err(format!("{source}: invalid row {}", index + 1).into());
        }
        let expected = *width.get_or_insert(fields.len());
        if fields.len() != expected {
            return Err(format!("{source}: inconsistent width at row {}", index + 1).into());
        }
        if index == 0 && header {
            continue;
        }
        if !unique.insert(line) {
            return Err(format!("{source}: duplicate row {}", index + 1).into());
        }
        rows.push(Case::new(
            source,
            &format!("line:{}", index + 1),
            fields[0],
            unit,
        ));
    }
    if rows.is_empty() {
        return Err(format!("{source}: empty case list").into());
    }
    Ok(rows)
}

pub fn classify(
    cases: &mut [Case],
    labels: &[String],
    prefix: &str,
    comparison: Comparison,
    reason: &str,
) -> Result<()> {
    for label in labels {
        let full = format!("{prefix}{label}");
        let matches: Vec<_> = cases.iter_mut().filter(|case| case.label == full).collect();
        if matches.len() != 1 {
            return Err(format!("classification does not identify one case: {full}").into());
        }
        let case = matches
            .into_iter()
            .next()
            .ok_or("missing classification target")?;
        if case.comparison != Comparison::Compared
            || comparison == Comparison::Compared
            || reason.is_empty()
        {
            return Err(format!("invalid or repeated classification: {full}").into());
        }
        case.comparison = comparison.clone();
        case.reason = Some(reason.to_string());
    }
    Ok(())
}
