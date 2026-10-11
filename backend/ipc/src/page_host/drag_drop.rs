// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Private owner-to-page-host drag events. Coordinate bits preserve finite
//! CSS doubles while keeping the private request identity exactly comparable.

use crate::input::{
    NativeDragItem, NativeDragModifiers, NativeDragMouse, NativeDragOperation, PointerPoint,
    MAX_DRAG_FORMATS, MAX_DRAG_ITEMS, MAX_DRAG_TEXT_BYTES,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PageHostDragKind {
    DragStart,
    Drag,
    DragEnter,
    DragOver,
    DragLeave,
    Drop,
    DragEnd,
}

impl PageHostDragKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DragStart => "dragstart",
            Self::Drag => "drag",
            Self::DragEnter => "dragenter",
            Self::DragOver => "dragover",
            Self::DragLeave => "dragleave",
            Self::Drop => "drop",
            Self::DragEnd => "dragend",
        }
    }
    pub fn protected(self) -> bool {
        !matches!(self, Self::DragStart | Self::Drop)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageHostDragEvent {
    pub kind: PageHostDragKind,
    pub client_x_bits: u64,
    pub client_y_bits: u64,
    #[serde(default)]
    pub screen_x_bits: u64,
    #[serde(default)]
    pub screen_y_bits: u64,
    pub page_x_bits: u64,
    pub page_y_bits: u64,
    pub offset_x_bits: u64,
    pub offset_y_bits: u64,
    pub related_target: Option<u64>,
    #[serde(default)]
    pub modifiers: NativeDragModifiers,
    #[serde(default)]
    pub buttons: u16,
    pub types: Vec<String>,
    pub items: Vec<NativeDragItem>,
    pub operation: NativeDragOperation,
    pub effect_allowed: String,
}

impl PageHostDragEvent {
    pub fn new(kind: PageHostDragKind, point: PointerPoint) -> Self {
        Self {
            kind,
            client_x_bits: point.x.to_bits(),
            client_y_bits: point.y.to_bits(),
            screen_x_bits: 0.0_f64.to_bits(),
            screen_y_bits: 0.0_f64.to_bits(),
            page_x_bits: point.x.to_bits(),
            page_y_bits: point.y.to_bits(),
            offset_x_bits: point.x.to_bits(),
            offset_y_bits: point.y.to_bits(),
            related_target: None,
            modifiers: NativeDragModifiers::default(),
            buttons: 0,
            types: Vec::new(),
            items: Vec::new(),
            operation: NativeDragOperation::None,
            effect_allowed: "uninitialized".into(),
        }
    }
    pub fn point(&self) -> PointerPoint {
        PointerPoint {
            x: f64::from_bits(self.client_x_bits),
            y: f64::from_bits(self.client_y_bits),
        }
    }
    pub fn set_mouse(&mut self, mouse: Option<NativeDragMouse>) {
        if let Some(mouse) = mouse {
            self.client_x_bits = mouse.client_point.x.to_bits();
            self.client_y_bits = mouse.client_point.y.to_bits();
            self.screen_x_bits = mouse.screen_point.x.to_bits();
            self.screen_y_bits = mouse.screen_point.y.to_bits();
            self.modifiers = mouse.modifiers;
            self.buttons = mouse.buttons;
        }
    }
    pub fn valid(&self) -> bool {
        let point = self.point();
        point.x.is_finite()
            && point.y.is_finite()
            && [
                self.page_x_bits,
                self.page_y_bits,
                self.offset_x_bits,
                self.offset_y_bits,
            ]
            .iter()
            .all(|bits| f64::from_bits(*bits).is_finite())
            && f64::from_bits(self.screen_x_bits).is_finite()
            && f64::from_bits(self.screen_y_bits).is_finite()
            && f64::from_bits(self.screen_x_bits).abs() <= 1_000_000.0
            && f64::from_bits(self.screen_y_bits).abs() <= 1_000_000.0
            && self.types.len() <= MAX_DRAG_FORMATS
            && self.types.iter().all(|format| valid_format(format))
            && valid_effect_allowed(&self.effect_allowed)
            && (!self.kind.protected() || self.items.is_empty())
            && valid_items(&self.items)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageHostDragOutcome {
    pub default_prevented: bool,
    pub operation: NativeDragOperation,
    pub effect_allowed: String,
    pub items: Vec<NativeDragItem>,
}

impl PageHostDragOutcome {
    pub fn valid(&self, kind: PageHostDragKind) -> bool {
        valid_effect_allowed(&self.effect_allowed)
            && valid_items(&self.items)
            && (kind == PageHostDragKind::DragStart || self.items.is_empty())
    }
}

pub fn valid_format(format: &str) -> bool {
    !format.is_empty()
        && format.len() <= 255
        && format.bytes().all(|byte| (0x20..=0x7e).contains(&byte))
}

pub fn valid_effect_allowed(effect: &str) -> bool {
    [
        "none",
        "copy",
        "copyLink",
        "copyMove",
        "link",
        "linkMove",
        "move",
        "all",
        "uninitialized",
    ]
    .contains(&effect)
}

pub fn valid_items(items: &[NativeDragItem]) -> bool {
    let mut formats = HashSet::new();
    let mut bytes = 0usize;
    items.len() <= MAX_DRAG_ITEMS
        && items.iter().all(|item| {
            bytes = bytes.saturating_add(match item {
                NativeDragItem::Text { format, data } => {
                    if format == "Files"
                        || !valid_format(format)
                        || !formats.insert(format.to_ascii_lowercase())
                    {
                        return false;
                    }
                    data.len()
                }
                NativeDragItem::File(file) => {
                    if !file.valid() {
                        return false;
                    }
                    file.bytes.len()
                }
            });
            bytes <= MAX_DRAG_TEXT_BYTES
        })
}
