// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Ties the pipeline stages together: fetch -> parse -> DOM -> CSS
//! cascade -> layout -> paint -> raster. This is Phase 3's end-to-end
//! path (see `tests/fixtures.rs` for the fixture-driven smoke test)
//! plus Phase 4's page-state layer ([`Page`]) that the `core` process
//! binary (`src/bin/blueice-core.rs`) drives from IPC messages.

mod ai_snapshot;
pub mod credits;
mod page;
pub mod session;
mod stylesheet;

use blueice_css::{cascade, ua_stylesheet, Origin};
use blueice_paint::Frame;

pub use page::Page;

/// One-shot render: parse `html`, cascade with `css` (an explicit
/// stylesheet, e.g. from a test fixture) plus any `<style>` tags found
/// inside `html` itself (see `stylesheet.rs`), lay out at
/// `viewport_width`, and paint. For anything stateful (navigation,
/// resize, scrolling, hit-testing across repeated interactions), use
/// [`Page`] instead -- this function re-does every stage from scratch
/// on each call, which is exactly right for a single fixture assertion
/// and exactly wrong for an interactive session.
pub fn render(html: &str, css: &str, viewport_width: f64) -> Frame {
    let doc = blueice_html::parse(html);
    let ua = ua_stylesheet();
    let mut author = blueice_css::parse(css).rules;
    author.extend(stylesheet::extract_inline_stylesheets(&doc));
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

    #[test]
    fn render_applies_inline_style_tags_found_in_the_html_itself() {
        let frame = render("<html><head><style>p { color: purple; }</style></head><body><p>hi</p></body></html>", "", 320.0);
        assert!(frame.commands.iter().any(|c| matches!(c, PaintCommand::Text { color, .. } if *color == blueice_css::Color::Rgba(128, 0, 128, 255))));
    }

    #[test]
    fn explicit_css_param_and_inline_style_tags_both_apply() {
        let frame = render("<html><body><style>p { color: red; }</style><p>hi</p></body></html>", "div { background-color: yellow; }", 320.0);
        assert!(frame.commands.iter().any(|c| matches!(c, PaintCommand::Text { color, .. } if *color == blueice_css::Color::Rgba(255, 0, 0, 255))));
    }
}
