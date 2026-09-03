// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Ties the pipeline stages together: parse -> DOM -> CSS cascade ->
//! layout -> paint.
//!
//! This crate will eventually own the `core` process's main loop and
//! expose the IPC surface described in
//! `development/browser_core/BROWSER_CORE_PLAN.md` §1 (Process
//! architecture) to `extension`, `frontend`, and the Phase 5 AI-facing
//! API. For now it only wires the (still-stubbed) pipeline stages
//! together end to end.

use blueice_paint::Frame;

pub fn render(html: &str, css: &str) -> Frame {
    let doc = blueice_html::parse(html);
    let stylesheet = blueice_css::parse(css);
    let fragment = blueice_layout::layout(&doc, doc.root(), &stylesheet);
    blueice_paint::paint(&fragment)
}
