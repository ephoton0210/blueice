// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Read-only frozen print documents. OS dialogs and destination writes belong
//! to the human frontend; this protocol grants no file or permission authority.
use serde::{Deserialize, Serialize};

pub const MAX_PRINT_PAGES: usize = 32;
pub const MAX_PRINT_PIXELS: u64 = 64 * 1024 * 1024;
pub const PRINT_RASTER_SCALE: f64 = 2.0;

/// Logical printable area in PostScript points (72/inch), after native scaling.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PrintProfile {
    pub width_points: f64,
    pub height_points: f64,
    pub backgrounds: bool,
}
impl PrintProfile {
    pub fn css_size(self) -> Result<(f64, f64), String> {
        let width = self.width_points * 4.0 / 3.0;
        let height = self.height_points * 4.0 / 3.0;
        if ![width, height]
            .iter()
            .all(|v| v.is_finite() && *v >= 1.0 && (*v * PRINT_RASTER_SCALE).ceil() <= 4096.0)
        {
            return Err("Printable area must fit the bounded 192 DPI print surface".into());
        }
        Ok((width, height))
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PrintAction {
    Begin {
        frame_source: u64,
        document_generation: u64,
    },
    Render {
        ticket: String,
        profile: PrintProfile,
    },
    Validate {
        ticket: String,
    },
    End {
        ticket: String,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrintPage {
    pub shm_path: String,
    pub width: u32,
    pub height: u32,
    /// Ink height; shorter pages leave white space instead of stretching.
    pub height_points: f64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PrintReply {
    Begun {
        ticket: String,
        document_generation: u64,
    },
    Rendered {
        ticket: String,
        revision: u64,
        profile: PrintProfile,
        pages: Vec<PrintPage>,
    },
    Validated {
        ticket: String,
    },
    Ended {
        ticket: String,
    },
}
