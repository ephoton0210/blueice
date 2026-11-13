// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Frozen mapped comparisons and separate owner/runtime/resource controls.

use super::case_sources::{self, Case, Comparison};
use super::snapshot::{Result, Snapshot};

pub fn primary(snapshot: &Snapshot) -> Result<Vec<Case>> {
    let path = "backend/bluets/tests/fixtures/mapped_inference/reference.json";
    case_sources::json_cases(
        path,
        snapshot.source(path)?,
        "/cases",
        "program/configuration",
    )
}

pub fn additional(snapshot: &Snapshot) -> Result<Vec<Case>> {
    let path = "backend/bluets/tests/fixtures/mapped_inference/boundary-controls.json";
    let mut cases = case_sources::json_cases(
        path,
        snapshot.source(path)?,
        "/controls",
        "owner/runtime/resource-control",
    )?;
    for control in &mut cases {
        control.comparison = Comparison::BoundaryControl;
        control.reason = Some("Ambient owner definitions, static/runtime authority separation and configured work/depth refusal are separate policy controls outside direct Native agreement.".into());
    }
    Ok(cases)
}
