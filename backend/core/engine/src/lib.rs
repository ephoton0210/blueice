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
//! API. For now it only wires the pipeline stages together end to end
//! (`blueice-paint` is still a stub, so calling this still panics at
//! the last step until Phase 3's paint checklist item lands).

use blueice_css::{cascade, ua_stylesheet, Origin};
use blueice_paint::Frame;

pub fn render(html: &str, css: &str, viewport_width: f64) -> Frame {
    let doc = blueice_html::parse(html);
    let ua = ua_stylesheet();
    let author = blueice_css::parse(css).rules;
    let styles = cascade(&doc, &[(Origin::Ua, &ua), (Origin::Author, &author)]);
    let fragment = blueice_layout::layout(&doc, doc.root(), &styles, blueice_layout::Constraints { available_width: viewport_width });
    blueice_paint::paint(&fragment)
}
