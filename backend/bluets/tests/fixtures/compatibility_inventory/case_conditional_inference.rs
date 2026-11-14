// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Conditional-inference comparisons and policy assertions retain distinct accounting roles.
use super::case_sources::{self, Case, Comparison};
use super::snapshot::{Result, Snapshot};

pub fn primary(snapshot: &Snapshot) -> Result<Vec<Case>> {
    let path = "backend/bluets/tests/fixtures/conditional_inference/reference.json";
    case_sources::json_cases(
        path,
        snapshot.source(path)?,
        "/cases",
        "program/configuration",
    )
}

pub fn additional(snapshot: &Snapshot) -> Result<Vec<Case>> {
    let path = "backend/bluets/tests/fixtures/conditional_inference/boundary-controls.json";
    let mut cases = case_sources::json_cases(
        path,
        snapshot.source(path)?,
        "/controls",
        "owner/runtime/resource-control",
    )?;
    for control in &mut cases {
        control.comparison = Comparison::BoundaryControl;
        control.reason=Some("Ambient owners, runtime-authority refusal and configured work/depth limits are separate policy controls, excluded from direct Native agreement.".into());
    }
    Ok(cases)
}

// The separately frozen alias controls are mandatory regressions with zero
// inventory credit; they are deliberately absent from primary/additional.
