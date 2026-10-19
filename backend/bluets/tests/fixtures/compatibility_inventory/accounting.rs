// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Row labels describe the asserted scope, never whole-language parity.

use super::case_sources::{Case, Comparison};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Closed,
    Open,
    Refused,
}

impl Status {
    pub const fn suite(open: usize, refused: usize) -> Self {
        if open > 0 {
            Self::Open
        } else if refused > 0 {
            Self::Refused
        } else {
            Self::Closed
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Closed => "closed",
            Self::Open => "open",
            Self::Refused => "refused",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Evidence {
    pub assertion: String,
    pub measurement: String,
}

#[derive(Debug, Serialize)]
pub struct AccountedCase {
    #[serde(flatten)]
    pub case: Case,
    pub status: Status,
    pub scope: &'static str,
    pub evidence: Evidence,
}

pub fn case(case: Case, measurement: &str) -> AccountedCase {
    let (status, scope) = match case.comparison {
        Comparison::Compared => (Status::Closed, "compared_properties"),
        Comparison::Open => (Status::Open, "documented_difference"),
        Comparison::Refused => (Status::Refused, "documented_refusal"),
        Comparison::ReferenceOnly => (Status::Closed, "native_reference_control_only"),
        Comparison::BoundaryControl => (Status::Closed, "bluets_owner_control_only"),
    };
    AccountedCase {
        evidence: Evidence {
            assertion: case.id.clone(),
            measurement: measurement.into(),
        },
        case,
        status,
        scope,
    }
}
