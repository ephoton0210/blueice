// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Assertion semantic observations and owner controls have separate scopes.

use super::case_sources::{self, Case, Comparison};
use super::snapshot::{Result, Snapshot};

const PATH: &str = "backend/bluets/tests/fixtures/assertion_results/reference.json";

fn selected(text: &str, role: &str) -> Result<Vec<Case>> {
    let unit = match role {
        "semantic" => "program/configuration",
        "owner" => "owner-mutation-control",
        _ => return Err("unknown assertion result inventory role".into()),
    };
    let root: serde_json::Value = serde_json::from_str(text)?;
    let rows = root["cases"]
        .as_array()
        .ok_or("missing assertion result observations")?;
    let cases = case_sources::json_cases(PATH, text, "/cases", unit)?;
    let mut unique = std::collections::BTreeSet::new();
    let mut selected = Vec::new();
    for (row, mut case) in rows.iter().zip(cases) {
        let observed_role = row["role"]
            .as_str()
            .ok_or("missing assertion observation role")?;
        let representation = row["representation"]
            .as_str()
            .ok_or("missing assertion representation")?;
        if !matches!(observed_role, "semantic" | "owner")
            || !matches!(representation, "original" | "crlf-astral" | "inline-astral")
            || !case.label.starts_with(&format!("{representation}/"))
            || !unique.insert(case.label.clone())
        {
            return Err("invalid or duplicate assertion result observation".into());
        }
        if observed_role != role || representation != "original" {
            continue;
        }
        if role == "owner" {
            case.comparison = Comparison::BoundaryControl;
            case.reason = Some("Readonly/opaque mutation refusals and unchanged mutable controls are BlueTS owner-policy observations. Native cast acceptance does not grant runtime write authority; prefix siblings, repeat builds and supplementary probes add no program denominator.".into());
        }
        selected.push(case);
    }
    Ok(selected)
}

pub fn primary(snapshot: &Snapshot) -> Result<Vec<Case>> {
    selected(snapshot.source(PATH)?, "semantic")
}

pub fn additional(snapshot: &Snapshot) -> Result<Vec<Case>> {
    selected(snapshot.source(PATH)?, "owner")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    const SOURCE: &str = include_str!("../assertion_results/reference.json");

    #[test]
    fn original_semantics_and_owner_controls_count_once_in_separate_scopes() {
        let semantic = selected(SOURCE, "semantic").unwrap();
        let owner = selected(SOURCE, "owner").unwrap();
        assert_eq!(semantic.len(), 136);
        assert_eq!(owner.len(), 80);
        assert!(semantic
            .iter()
            .all(|case| case.comparison == Comparison::Compared));
        assert!(owner
            .iter()
            .all(|case| case.comparison == Comparison::BoundaryControl
                && case
                    .reason
                    .as_ref()
                    .is_some_and(|reason| !reason.is_empty())));
        let cases: Vec<_> = semantic.iter().chain(&owner).collect();
        assert!(cases.iter().all(|case| case.source == PATH
            && case.selector.starts_with("/cases/")
            && case.label.starts_with("original/")));
        assert_eq!(
            cases
                .iter()
                .map(|case| &case.id)
                .collect::<BTreeSet<_>>()
                .len(),
            216
        );
    }

    #[test]
    fn prefix_siblings_do_not_increase_primary_credit() {
        let source: serde_json::Value = serde_json::from_str(SOURCE).unwrap();
        let mut original = source.clone();
        original["cases"]
            .as_array_mut()
            .unwrap()
            .retain(|row| row["representation"] == "original");
        for role in ["semantic", "owner"] {
            let full = selected(SOURCE, role).unwrap();
            let base = selected(&original.to_string(), role).unwrap();
            assert_eq!(
                full.iter().map(|case| &case.label).collect::<Vec<_>>(),
                base.iter().map(|case| &case.label).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn missing_roles_and_duplicate_observation_ids_are_refused() {
        let source: serde_json::Value = serde_json::from_str(SOURCE).unwrap();
        let mut unknown = source.clone();
        unknown["cases"][0]["role"] = "unknown-role".into();
        assert!(selected(&unknown.to_string(), "semantic").is_err());
        let mut duplicate = source;
        duplicate["cases"][1]["id"] = duplicate["cases"][0]["id"].clone();
        assert!(selected(&duplicate.to_string(), "semantic").is_err());
    }
}
