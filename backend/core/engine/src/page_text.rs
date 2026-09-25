// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The page's shown prose as plain text, for the assistant's summarize and
//! organize tasks (`phase-7-local-ai/PLAN.md`, step S2). It reads the same
//! DOM and computed styles the frame is painted from, so it describes what a
//! person sees: `display:none` subtrees are absent, translated text is the
//! translation while the translation is showing, and script, style, template,
//! and form-draft text is skipped as it is for translation.
//!
//! Layout structure is approximated without a second layout pass: any element
//! whose computed `display` is not `inline` (and `<br>`) separates the text
//! around it with a newline; text inside one inline run joins with a space.

use crate::translation::SKIPPED_ANCESTORS;
use blueice_css::ComputedStyle;
use blueice_dom::{Document, NodeData, NodeId};
use std::collections::HashMap;

/// The shown text of `doc`, cut at a character boundary to at most
/// `max_bytes`.
pub fn visible_text(
    doc: &Document,
    styles: &HashMap<NodeId, ComputedStyle>,
    max_bytes: usize,
) -> String {
    let mut out = String::new();
    let mut pending_break = false;
    walk(doc, styles, doc.root(), max_bytes, &mut out, &mut pending_break);
    truncate_at_boundary(&mut out, max_bytes);
    out
}

fn walk(
    doc: &Document,
    styles: &HashMap<NodeId, ComputedStyle>,
    node: NodeId,
    max_bytes: usize,
    out: &mut String,
    pending_break: &mut bool,
) {
    for child in doc.children(node) {
        if out.len() >= max_bytes {
            return;
        }
        match doc.data(child) {
            NodeData::Text { data } => {
                let text = data.split_whitespace().collect::<Vec<_>>().join(" ");
                if text.is_empty() {
                    continue;
                }
                if !out.is_empty() {
                    out.push(if *pending_break { '\n' } else { ' ' });
                }
                *pending_break = false;
                out.push_str(&text);
            }
            NodeData::Element { tag_name, .. } => {
                let display = styles.get(&child).map(|style| style.display.as_str());
                if display == Some("none")
                    || SKIPPED_ANCESTORS
                        .iter()
                        .any(|skipped| tag_name.eq_ignore_ascii_case(skipped))
                {
                    continue;
                }
                let separates = tag_name.eq_ignore_ascii_case("br") || display != Some("inline");
                if separates {
                    *pending_break = true;
                }
                walk(doc, styles, child, max_bytes, out, pending_break);
                if separates {
                    *pending_break = true;
                }
            }
            NodeData::Document => walk(doc, styles, child, max_bytes, out, pending_break),
        }
    }
}

fn truncate_at_boundary(text: &mut String, max_bytes: usize) {
    if text.len() <= max_bytes {
        return;
    }
    let mut end = max_bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
}

#[cfg(test)]
mod tests {
    use crate::page::Page;

    fn text_of(html: &str) -> String {
        let mut page = Page::new(320.0, 400.0);
        page.load_html_str(html, None);
        page.visible_text()
    }

    #[test]
    fn blocks_are_separate_lines_and_inline_runs_are_joined() {
        assert_eq!(
            text_of("<h1>Title</h1><p>One <b>two</b> three</p><p>Four</p>"),
            "Title\nOne two three\nFour"
        );
    }

    #[test]
    fn a_line_break_separates_text() {
        assert_eq!(text_of("<p>a<br>b</p>"), "a\nb");
    }

    #[test]
    fn hidden_and_non_prose_content_is_absent() {
        assert_eq!(
            text_of(
                "<head><title>T</title></head><body>\
                 <p style=\"display:none\">secret</p><script>var a=1</script>\
                 <style>p{}</style><textarea>draft</textarea><p>shown</p></body>"
            ),
            "shown"
        );
    }

    #[test]
    fn whitespace_is_collapsed_and_an_empty_page_is_empty() {
        assert_eq!(text_of("<p>  a \n\t b  </p>"), "a b");
        assert_eq!(text_of(""), "");
        assert_eq!(text_of("<p>   </p>"), "");
    }

    #[test]
    fn the_shown_translation_is_what_is_read_and_the_original_when_toggled() {
        let mut page = Page::new(320.0, 400.0);
        page.load_html_translated("<p>Hello</p>", None, &["你好".into()]);
        assert_eq!(page.visible_text(), "你好");
        page.set_translation_shown(false);
        assert_eq!(page.visible_text(), "Hello");
    }

    #[test]
    fn text_is_bounded_at_a_character_boundary() {
        // 3-byte characters: a byte limit that lands mid-character must back
        // off rather than split it.
        let mut page = Page::new(320.0, 400.0);
        page.load_html_str(&format!("<p>{}</p>", "字".repeat(100)), None);
        let cut = super::visible_text(page.doc(), page.styles(), 10);
        assert_eq!(cut, "字".repeat(3));
        assert!(cut.len() <= 10);
        // A page longer than the assistant accepts is cut to that limit.
        let long = "word ".repeat(30_000);
        page.load_html_str(&format!("<p>{long}</p>"), None);
        assert!(page.visible_text().len() <= blueice_ipc::assistant::MAX_REQUEST_TEXT_BYTES);
    }
}
