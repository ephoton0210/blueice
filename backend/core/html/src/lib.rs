// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! HTML tokenizer and tree builder.
//!
//! See `development/browser_core/research/html-parsing.md` for the
//! target architecture this follows (a tokenizer state machine feeding a
//! tree builder through a narrow callback/token interface, per Blink's
//! split) and `development/browser_core/phase-2-mvp-scope/PLAN.md`'s
//! "MVP HTML scope" for exactly which elements/attributes are supported
//! and which spec algorithms are kept vs. cut.

mod tokenizer;
mod tree_builder;

pub use blueice_dom::Document;

/// Parses `input` as HTML into a fresh [`Document`].
pub fn parse(input: &str) -> Document {
    tree_builder::parse(input)
}
