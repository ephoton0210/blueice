// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Core-owned, frame-fenced text queries and ordinary editing for platform AT.
//! Password text never enters a reply; these commands confer no permission or
//! trusted-gesture authority and do not expand extension DOM-write capability.
use crate::{input::TextRange, Bounds};
use serde::{Deserialize, Serialize};

pub const ACCESSIBILITY_TEXT_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessibilityContext {
    pub version: u32,
    pub frame_source: u64,
    pub document_generation: u64,
    pub frame_generation: u64,
    pub node_id: u64,
}

pub type AccessibilityTextContext = AccessibilityContext;

/// Native consumption of a document-scoped announcement prefix. This is not
/// acknowledgement of screen-reader speech and never grants input authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessibilityDelivery {
    pub version: u32,
    pub frame_source: u64,
    pub document_generation: u64,
    pub revision: u64,
}

/// Revealing an AT reading target never activates it or changes DOM focus.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccessibilityRevealReply {
    pub context: AccessibilityContext,
    pub bounds: Bounds,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LivePoliteness {
    Polite,
    Assertive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessibilityAnnouncement {
    pub sequence: u64,
    pub region_id: u64,
    pub text: String,
    pub politeness: LivePoliteness,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessibilityName {
    pub node_id: u64,
    pub name: Option<String>,
}

/// Bounded pending announcements, retained until native consumption. Repeated
/// reads have the same revision. Initial content is a baseline; platform clients
/// must also baseline a newly selected document.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessibilitySnapshot {
    pub document_generation: u64,
    pub revision: u64,
    #[serde(default)]
    pub acknowledged_revision: u64,
    #[serde(default)]
    pub delivery_version: u32,
    pub announcements: Vec<AccessibilityAnnouncement>,
    #[serde(default)]
    pub hidden_nodes: Vec<u64>,
    #[serde(default)]
    pub names: Vec<AccessibilityName>,
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AccessibilityTextAction {
    Inspect,
    Bounds { range: TextRange },
    LineForIndex { index: u32 },
    RangeForLine { line: u32 },
    RangeForIndex { index: u32 },
    RangeForPosition { x: f64, y: f64 },
    Select { range: TextRange },
    ReplaceSelection { text: String },
    SetValue { text: String },
    ScrollToRange { range: TextRange },
}
impl AccessibilityTextAction {
    pub fn mutates(&self) -> bool {
        matches!(
            self,
            Self::Select { .. }
                | Self::ReplaceSelection { .. }
                | Self::SetValue { .. }
                | Self::ScrollToRange { .. }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccessibilityTextStyle {
    pub font_size_px: f64,
    pub bold: bool,
    pub italic: bool,
    pub color: [u8; 4],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccessibilityTextState {
    pub text: Option<String>,
    pub text_length: u32,
    pub protected: bool,
    pub writable: bool,
    pub multiline: bool,
    pub focused: bool,
    pub selection: Option<TextRange>,
    pub marked: Option<TextRange>,
    pub visible_range: TextRange,
    pub insertion_line: Option<u32>,
    pub line_count: u32,
    pub style: AccessibilityTextStyle,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AccessibilityTextResult {
    State(Box<AccessibilityTextState>),
    Bounds(Option<Bounds>),
    Index(Option<u32>),
    Range(Option<TextRange>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccessibilityTextReply {
    pub context: AccessibilityTextContext,
    pub result: AccessibilityTextResult,
}
