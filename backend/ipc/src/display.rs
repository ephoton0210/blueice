// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Explicit display preferences do not grant navigation or input privileges.
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayPreferences {
    pub dark: bool,
    pub high_contrast: bool,
    pub reduced_motion: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayPreferencesState {
    pub tab_id: u64,
    pub frame_source: u64,
    pub frame_generation: u64,
    pub preferences: DisplayPreferences,
}
