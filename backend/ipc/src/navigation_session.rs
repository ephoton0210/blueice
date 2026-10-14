// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Durable navigation metadata, without DOM, grants, credentials or POST bodies.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionDocument {
    pub tab_id: u64,
    pub frame_source: u64,
    pub document_generation: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NavigationEntry {
    pub url: Option<String>,
    pub was_post: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NavigationHistory {
    pub entries: Vec<NavigationEntry>,
    pub cursor: usize,
    pub zoom: f64,
}
impl NavigationHistory {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.entries.is_empty()
            || self.entries.len() > 128
            || self.cursor >= self.entries.len()
            || !self.zoom.is_finite()
            || !(0.25..=5.0).contains(&self.zoom)
        {
            return Err("Invalid saved navigation history");
        }
        for entry in &self.entries {
            if entry.was_post && entry.url.is_none() {
                return Err("Invalid saved POST history");
            }
            if let Some(url) = &entry.url {
                if url.is_empty() || url.len() > 4096 || url.chars().any(char::is_control) {
                    return Err("Invalid saved navigation URL");
                }
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NavigationSessionAction {
    Inspect,
    /// GET metadata can only attach to the already reviewed current URL.
    /// Current POST metadata can only replace a blank tab with a fixed warning.
    Restore {
        context: SessionDocument,
        history: NavigationHistory,
    },
}
