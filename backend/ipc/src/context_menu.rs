// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Native context menus inspect core hit targets, never a frontend DOM copy.
use crate::input::{DocumentSelectionState, TextInputState};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ContextMenuContext {
    pub tab_id: u64,
    pub frame_source: u64,
    pub document_generation: u64,
    pub frame_generation: u64,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextMenuState {
    pub context: ContextMenuContext,
    /// Public, resolved link URL; scripts, local paths and unsupported schemes
    /// are not offered by the native menu. Copying does not authorize a fetch.
    pub link_url: Option<String>,
    /// Only the hit editor's redacted public state, never an unrelated focus.
    pub input: Option<TextInputState>,
    /// Document commands for public/blank targets, separate from editor state.
    #[serde(default)]
    pub document: Option<Box<DocumentSelectionState>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContextMenuLinkAction {
    Copy,
    Open,
    OpenInNewTab,
}
