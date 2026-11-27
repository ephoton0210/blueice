// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Original discovery limitations remain visible beside the finite repair.

use super::case_sources::{self, Case, Comparison};
use super::snapshot::{Result, Snapshot};

const PATH: &str = "backend/bluets/tests/fixtures/erased_type_regions/reference.json";
const CONTROLS: &str = "backend/bluets/tests/fixtures/erased_type_regions/discovery-controls.json";

pub fn primary(snapshot: &Snapshot) -> Result<Vec<Case>> {
    selected(snapshot.source(PATH)?, snapshot.source(CONTROLS)?)
}

fn selected(reference: &str, controls: &str) -> Result<Vec<Case>> {
    let controls: serde_json::Value = serde_json::from_str(controls)?;
    let controls = controls["cases"]
        .as_array()
        .ok_or("missing discovery controls")?;
    let mut cases =
        case_sources::json_cases(PATH, reference, "/cases", "call-semantics/configuration")?;
    for control in controls {
        let id = control["id"]
            .as_str()
            .ok_or("missing discovery control id")?;
        let case = cases
            .iter_mut()
            .find(|case| case.label == id)
            .ok_or("foreign discovery control")?;
        case.comparison = if id.starts_with("constructor-as-argument/") {
            Comparison::Refused
        } else {
            Comparison::Open
        };
        case.reason = Some(
            control["reason"]
                .as_str()
                .ok_or("missing discovery control reason")?
                .into(),
        );
    }
    Ok(cases)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_original_observation_remains_counted_in_its_actual_scope() {
        let cases = selected(
            include_str!("../erased_type_regions/reference.json"),
            include_str!("../erased_type_regions/discovery-controls.json"),
        )
        .unwrap();
        assert_eq!(cases.len(), 136);
        assert_eq!(
            cases
                .iter()
                .filter(|row| row.comparison == Comparison::Compared)
                .count(),
            124
        );
        assert_eq!(
            cases
                .iter()
                .filter(|row| row.comparison == Comparison::Open)
                .count(),
            8
        );
        assert_eq!(
            cases
                .iter()
                .filter(|row| row.comparison == Comparison::Refused)
                .count(),
            4
        );
        assert!(cases
            .iter()
            .filter(|row| row.comparison != Comparison::Compared)
            .all(|row| row.reason.as_ref().is_some_and(|reason| !reason.is_empty())));
    }
}
