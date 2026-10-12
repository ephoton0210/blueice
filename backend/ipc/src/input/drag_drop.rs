// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A native drag is a document-owned interaction. Enter/updates disclose only
//! format metadata; content arrives after an explicit OS drop. No action
//! grants permission or authorizes reading a path supplied by a page/client.

use super::PointerPoint;
use crate::file_input::FileData;
use serde::{Deserialize, Serialize};

pub const MAX_DRAG_FORMATS: usize = 64;
pub const MAX_DRAG_ITEMS: usize = 64;
pub const MAX_DRAG_TEXT_BYTES: usize = 1_048_576;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NativeDragModifiers {
    pub alt: bool,
    pub control: bool,
    pub meta: bool,
    pub shift: bool,
    pub caps_lock: bool,
}

/// A snapshot captured in the OS drag callback, before any asynchronous work.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeDragMouse {
    pub client_point: PointerPoint,
    pub screen_point: PointerPoint,
    pub modifiers: NativeDragModifiers,
    pub buttons: u16,
}

impl NativeDragMouse {
    pub fn valid(self) -> bool {
        self.client_point.x.is_finite()
            && self.client_point.y.is_finite()
            && self.client_point.x.abs() <= 16384.0
            && self.client_point.y.abs() <= 16384.0
            && self.screen_point.x.is_finite()
            && self.screen_point.y.is_finite()
            && self.screen_point.x.abs() <= 1_000_000.0
            && self.screen_point.y.abs() <= 1_000_000.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NativeDragOperation {
    None,
    Copy,
    Move,
    Link,
}

impl NativeDragOperation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Copy => "copy",
            Self::Move => "move",
            Self::Link => "link",
        }
    }

    pub fn from_effect(effect: &str) -> Option<Self> {
        match effect {
            "none" => Some(Self::None),
            "copy" => Some(Self::Copy),
            "move" => Some(Self::Move),
            "link" => Some(Self::Link),
            _ => None,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum NativeDragItem {
    Text { format: String, data: String },
    File(FileData),
}

impl std::fmt::Debug for NativeDragItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Text { format, data } => f
                .debug_struct("Text")
                .field("format_bytes", &format.len())
                .field("data_bytes", &data.len())
                .finish(),
            Self::File(file) => f.debug_tuple("File").field(file).finish(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum NativeDragAction {
    /// Repeating Enter for this drag updates its point and negotiates the
    /// current target. The core delivers target transitions and dragover.
    Enter {
        drag_id: u64,
        point: PointerPoint,
        types: Vec<String>,
        allowed_operations: Vec<NativeDragOperation>,
    },
    /// Explicit destination leave is independent of focus/frame changes.
    Leave { drag_id: u64 },
    Drop {
        drag_id: u64,
        point: PointerPoint,
        items: Vec<NativeDragItem>,
        /// A synchronous OS destination must commit the operation it advertised.
        /// Older canonical clients may let the core choose from their mask.
        #[serde(default)]
        expected_operation: Option<NativeDragOperation>,
    },
    /// Asks core for the source's ordinary payload at a real source point.
    /// The frontend cannot provide a source DOM node or substitute its text.
    Start {
        drag_id: u64,
        point: PointerPoint,
        allowed_operations: Vec<NativeDragOperation>,
    },
    /// Source movement may leave its viewport. It contains coordinates only.
    SourceMove { drag_id: u64, point: PointerPoint },
    End {
        drag_id: u64,
        operation: NativeDragOperation,
        #[serde(default)]
        point: Option<PointerPoint>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeDragImage {
    /// CSS viewport rectangles. Disjoint selections are masked separately.
    pub regions: Vec<crate::Bounds>,
    pub hotspot: PointerPoint,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeDragState {
    pub version: u32,
    pub drag_id: u64,
    pub frame_source: u64,
    pub document_generation: u64,
    pub frame_generation: u64,
    pub active: bool,
    pub operation: NativeDragOperation,
    pub allowed_operations: Vec<NativeDragOperation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mouse: Option<NativeDragMouse>,
    /// Present only for an explicit source Start, never a hover observation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<NativeDragItem>>,
    /// Present only for a source Start and bound to this reply's frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<NativeDragImage>,
}
