// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! CSS layout size is independent of native backing density and per-tab zoom.
use serde::{Deserialize, Serialize};
pub const MIN_PAGE_ZOOM: f64 = 0.25;
pub const MAX_PAGE_ZOOM: f64 = 5.0;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DisplayViewport {
    /// Native logical window content dimensions at 100% page zoom.
    pub width: f64,
    pub height: f64,
    /// Raster density, which may be capped to respect physical frame limits.
    pub device_scale: f64,
    /// Actual native backing density for CSS media resolution. Older clients
    /// omit this field and use `device_scale` for both purposes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backing_scale: Option<f64>,
}
impl DisplayViewport {
    pub fn pixel_size(self) -> (u32, u32) {
        (
            (self.width * self.device_scale).ceil() as u32,
            (self.height * self.device_scale).ceil() as u32,
        )
    }

    pub fn validate(self) -> Result<(), String> {
        if ![self.width, self.height, self.device_scale]
            .iter()
            .all(|v| v.is_finite())
            || !(1.0..=4096.0).contains(&self.width)
            || !(1.0..=4096.0).contains(&self.height)
            || !(1.0..=4.0).contains(&self.device_scale)
            || self
                .backing_scale
                .is_some_and(|v| !v.is_finite() || !(1.0..=4.0).contains(&v))
            || (self.width * self.device_scale).ceil() > 4096.0
            || (self.height * self.device_scale).ceil() > 4096.0
        {
            return Err("Display viewport exceeds finite logical/pixel limits".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewportState {
    pub tab_id: u64,
    pub frame_source: u64,
    pub frame_generation: u64,
    pub width: f64,
    pub height: f64,
    pub device_scale: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backing_scale: Option<f64>,
    pub zoom: f64,
    pub css_width: f64,
    pub css_height: f64,
    pub pixel_width: u32,
    pub pixel_height: u32,
}
