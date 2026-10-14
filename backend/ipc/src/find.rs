// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Document-fenced browser find commands. Results contain geometry, never page text.

use crate::Bounds;
use serde::{Deserialize, Serialize};

pub const MAX_FIND_QUERY_BYTES: usize = 1024;
pub const MAX_FIND_MATCHES: usize = 10_000;
pub const MAX_FIND_RECTS: usize = 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FindAction {
    Update { query: String, case_sensitive: bool },
    Next { backwards: bool },
    Close,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FindState {
    pub tab_id: u64,
    pub frame_source: u64,
    pub document_generation: u64,
    pub revision: u64,
    pub query: String,
    pub case_sensitive: bool,
    pub match_count: u32,
    /// One-based current result; None means there is no match.
    pub active_match: Option<u32>,
    pub wrapped: bool,
    /// A bounded index or result limit was reached; match_count is a lower bound.
    pub limited: bool,
    pub rects: Vec<Bounds>,
}
