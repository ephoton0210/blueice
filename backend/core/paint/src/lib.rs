// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Paint: a fragment tree -> raster/paint-command output.
//!
//! Not yet implemented, and not yet researched -- no reference-checkout
//! notes exist for this stage yet.

use blueice_layout::Fragment;

/// Raster/paint-command output for one frame. Not yet a real type.
pub struct Frame;

pub fn paint(_fragment: &Fragment) -> Frame {
    todo!("paint -- not yet designed")
}
