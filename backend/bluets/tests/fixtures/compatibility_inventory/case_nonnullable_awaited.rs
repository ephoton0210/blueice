// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Direct utility/ordinary-operator observations and separate demand controls.

use super::case_sources::{self, Case, Comparison};
use super::snapshot::{Result, Snapshot};

fn rows(snapshot: &Snapshot, file: &str, pointer: &str, unit: &str) -> Result<Vec<Case>> {
    let path = format!("backend/bluets/tests/fixtures/nonnullable_awaited/{file}");
    case_sources::json_cases(&path, snapshot.source(&path)?, pointer, unit)
}

pub fn primary(snapshot: &Snapshot) -> Result<Vec<Case>> {
    let mut cases = Vec::new();
    for file in ["reference.json", "operator-reference.json"] {
        cases.extend(rows(snapshot, file, "/cases", "program/configuration")?);
    }
    Ok(cases)
}

pub fn additional(snapshot: &Snapshot) -> Result<Vec<Case>> {
    let mut cases = rows(
        snapshot,
        "demand-reference.json",
        "/cases",
        "semantic-instantiation/program-control",
    )?;
    for (file, unit, reason) in [
        (
            "boundary-controls.json",
            "owner/runtime/resource-control",
            "Owner/local declaration precedence, static/runtime authority separation and configured work/depth refusal are separate policy controls, outside the frozen direct comparison matrix.",
        ),
        (
            "demand-resource-controls.json",
            "configured-type-work-control",
            "Declared and inferred operations refuse zero/one-work budgets without publication or a Native TS2589 counterpart; configured resource policy is separate from semantic nontermination.",
        ),
    ] {
        let mut controls = rows(snapshot, file, "/controls", unit)?;
        for control in &mut controls {
            control.comparison = Comparison::BoundaryControl;
            control.reason = Some(reason.into());
        }
        cases.extend(controls);
    }
    Ok(cases)
}
