// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Ordinary, document-bound assistant operations. These carry no grant authority.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssistantDocument {
    pub tab_id: u64,
    pub frame_source: u64,
    pub document_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssistantPageAction {
    Summarize,
    Organize { instruction: String },
    ShowTranslation { shown: bool },
}
