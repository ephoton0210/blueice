// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Contextual member programs count once; prefix siblings add no denominator.

use super::case_sources::{self, Case, Comparison};
use super::snapshot::{Result, Snapshot};

pub fn primary(snapshot: &Snapshot) -> Result<Vec<Case>> {
    let path = "backend/bluets/tests/fixtures/contextual_member_results/reference.json";
    let cases = case_sources::json_cases(
        path,
        snapshot.source(path)?,
        "/cases",
        "program/configuration",
    )?;
    Ok(cases
        .into_iter()
        .filter(|case| case.label.starts_with("original/"))
        .collect())
}

pub fn additional(snapshot: &Snapshot) -> Result<Vec<Case>> {
    let path = "backend/bluets/tests/fixtures/contextual_member_results/boundary-controls.json";
    let mut cases = case_sources::json_cases(
        path,
        snapshot.source(path)?,
        "/controls",
        "owner/resource-control",
    )?;
    for case in &mut cases {
        case.comparison = Comparison::BoundaryControl;
        case.reason = Some(
            "Configured work/source budgets, static noEmit authority and unselected-source confinement are separate BlueTS owner controls with no Native agreement credit.".into(),
        );
    }
    Ok(cases)
}
