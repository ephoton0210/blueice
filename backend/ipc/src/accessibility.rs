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
pub struct AccessibilityTextContext {
    pub version: u32,
    pub frame_source: u64,
    pub document_generation: u64,
    pub frame_generation: u64,
    pub node_id: u64,
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
