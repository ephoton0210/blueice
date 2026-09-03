// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! HTML tokenizer and tree builder.
//!
//! Not yet implemented. See
//! `development/browser_core/research/html-parsing.md` for the target
//! architecture (a tokenizer state machine feeding a tree builder
//! through a narrow callback interface, per Blink's split) and the MVP
//! scope recommendations (keep the full tokenizer state machine and
//! adoption-agency/foster-parenting error recovery; cut foreign content,
//! `document.write` reentrancy, and speculative parsing).

use blueice_dom::Document;

/// Parses `input` as HTML into a fresh [`Document`].
pub fn parse(_input: &str) -> Document {
    todo!("HTML tokenizer/tree-builder -- see research/html-parsing.md")
}
