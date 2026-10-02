// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Core-owned native text editing. Commands identify a live document, never
//! supply a DOM node to mutate, and do not confer trusted gesture/permission
//! authority. Password contents remain absent from all inspection replies.

use crate::Bounds;
use serde::{Deserialize, Serialize};

pub const TEXT_INPUT_VERSION: u32 = 1;
pub const MAX_EDIT_TEXT_UTF16: usize = 65_536;
pub const MAX_EDIT_CARETS: usize = 1_024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextRange {
    pub location: u32,
    pub length: u32,
}

impl TextRange {
    pub fn end(self) -> Option<u32> {
        self.location.checked_add(self.length)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextInputContext {
    pub version: u32,
    pub frame_source: u64,
    pub document_generation: u64,
    pub focus_generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextMovement {
    Backward,
    Forward,
    WordBackward,
    WordForward,
    Beginning,
    End,
    LineBeginning,
    LineEnd,
    Up,
    Down,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TextInputAction {
    /// A missing range replaces marked text, or otherwise the core selection.
    Replace {
        text: String,
        replacement: Option<TextRange>,
    },
    /// Selection is relative to the inserted marked string, as in AppKit.
    Compose {
        text: String,
        selection: TextRange,
        replacement: Option<TextRange>,
    },
    FinishComposition,
    CancelComposition,
    Select {
        range: TextRange,
    },
    SelectAll,
    Move {
        direction: TextMovement,
        extend: bool,
    },
    Delete {
        forward: bool,
    },
    /// Adjust selection only in the already-focused control. Ordinary click
    /// hit-testing and default focus run before this command.
    Pointer {
        x: f64,
        y: f64,
        extend: bool,
        click_count: u8,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextCaret {
    pub offset: u32,
    pub bounds: Bounds,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextControlState {
    pub node_id: u64,
    /// Protected controls provide length/range metadata, never plaintext.
    pub text: Option<String>,
    pub text_length: u32,
    pub protected: bool,
    pub writable: bool,
    pub multiline: bool,
    pub selection: TextRange,
    pub marked: Option<TextRange>,
    pub bounds: Bounds,
    pub caret: Bounds,
    pub carets: Vec<TextCaret>,
    pub selection_rects: Vec<Bounds>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextInputState {
    pub version: u32,
    pub frame_source: u64,
    pub document_generation: u64,
    pub focus_generation: u64,
    pub frame_generation: u64,
    pub tab_id: u64,
    pub scroll_y: f64,
    pub focused: Option<TextControlState>,
}
