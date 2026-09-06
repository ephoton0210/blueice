// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Ties the pipeline stages together: parse -> DOM -> CSS cascade ->
//! layout -> paint. This is Phase 3's end-to-end path -- every stage
//! `render` calls is now a real implementation, not a stub (see
//! `tests/fixtures.rs` for the fixture-driven smoke test this crate's
//! own Definition of Done calls for: fixture HTML+CSS -> asserted paint
//! output, through this exact public function).
//!
//! This crate will eventually own the `core` process's main loop and
//! expose the IPC surface described in
//! `development/browser_core/BROWSER_CORE_PLAN.md` §1 (Process
//! architecture) to `extension`, `frontend`, and the Phase 5 AI-facing
//! API.

use blueice_css::{cascade, ua_stylesheet, Origin};
use blueice_paint::Frame;

pub fn render(html: &str, css: &str, viewport_width: f64) -> Frame {
    let doc = blueice_html::parse(html);
    let ua = ua_stylesheet();
    let author = blueice_css::parse(css).rules;
    let styles = cascade(&doc, &[(Origin::Ua, &ua), (Origin::Author, &author)]);
    let fragment = blueice_layout::layout(&doc, doc.root(), &styles, blueice_layout::Constraints { available_width: viewport_width });
    blueice_paint::paint(&fragment, &styles)
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_paint::PaintCommand;

    #[test]
    fn render_produces_a_frame_with_paint_commands_for_a_simple_page() {
        let frame = render("<p>hi</p>", "p { color: red; }", 320.0);
        assert_eq!(frame.width, 320.0);
        assert!(frame.commands.iter().any(|c| matches!(c, PaintCommand::Text { text, .. } if text == "hi")));
    }

    #[test]
    fn render_applies_both_ua_and_author_styles() {
        let frame = render("<div>x</div>", "div { background-color: blue; }", 320.0);
        assert!(frame.commands.iter().any(|c| matches!(c, PaintCommand::Rect { color, .. } if *color == blueice_css::Color::Rgba(0, 0, 255, 255))));
    }
}
