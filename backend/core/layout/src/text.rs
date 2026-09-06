// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Text measurement and greedy line-breaking for inline content.
//!
//! **No font metrics or text shaping exist anywhere in BlueIce yet** --
//! there is no font-loading or glyph-measurement subsystem to consult,
//! so widths here are a crude average-glyph-width approximation
//! (`font_size_px * 0.6` per character, the same order-of-magnitude
//! ratio real fonts average out to for latin text). This is
//! deliberately named as an approximation, not hidden behind a
//! confident-looking API: it's enough to produce *some* real line-
//! breaking geometry (the thing `research/layout.md` §4 asks block/
//! inline layout to actually do), not to be pixel-accurate. Replacing
//! it with real shaping later only touches this module.
//!
//! Line-breaking itself is word-level greedy fill-then-wrap (accumulate
//! words onto a line until the next one would overflow, then start a
//! new line) -- `research/layout.md` §4's "naive algorithm", with no
//! hyphenation, no mid-word breaking on overflow, and no bidi/script
//! segmentation.

use blueice_css::ComputedStyle;
use blueice_dom::{Document, NodeData, NodeId};
use std::collections::HashMap;

pub type StyleMap = HashMap<NodeId, ComputedStyle>;

pub fn char_width(font_size_px: f64) -> f64 {
    font_size_px * 0.6
}

pub fn text_width(text: &str, font_size_px: f64) -> f64 {
    text.chars().count() as f64 * char_width(font_size_px)
}

/// The initial `line-height: normal` value real browsers use absent an
/// explicit one: roughly 1.2x the font size.
pub fn default_line_height(font_size_px: f64) -> f64 {
    font_size_px * 1.2
}

/// One space-separated word of inline content, tagged with the nearest
/// element ancestor it came from (`style_node`) so a fragment built
/// from it can still be traced back to whatever styled it -- color,
/// font-weight, etc. -- even though nested inline elements (`<b>`,
/// `<a>`, ...) are otherwise flattened into their block container's
/// single word stream rather than getting their own nested fragment
/// (a deliberate MVP simplification: with no visual text rendering yet
/// either, a nested inline fragment tree has no consumer to justify its
/// complexity today).
#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub text: String,
    pub width: f64,
    pub style_node: NodeId,
}

/// Recursively collects every word of text under `node`, attributing
/// each to the nearest element ancestor (`style_node`, updated as the
/// walk descends into nested inline elements). Does not itself decide
/// what counts as "inline-level" content -- the caller (`block.rs`)
/// only calls this on nodes it already classified as inline-level.
pub fn collect_words(doc: &Document, node: NodeId, styles: &StyleMap, style_node: NodeId, out: &mut Vec<Word>) {
    match doc.data(node) {
        NodeData::Text { data } => {
            let font_size = styles.get(&style_node).map(|s| s.font_size_px).unwrap_or(16.0);
            for word in data.split_whitespace() {
                out.push(Word { text: word.to_string(), width: text_width(word, font_size), style_node });
            }
        }
        NodeData::Element { .. } => {
            for child in doc.children(node) {
                collect_words(doc, child, styles, node, out);
            }
        }
        NodeData::Document => {}
    }
}

/// Greedily packs `words` onto lines no wider than `available_width`
/// (accounting for a `space_width`-wide gap between words on the same
/// line). A single word wider than `available_width` on its own still
/// gets its own line rather than being split -- overflow, not an
/// error, matching real browsers' default (no forced mid-word breaks).
pub fn break_into_lines(words: &[Word], available_width: f64, space_width: f64) -> Vec<&[Word]> {
    if words.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut line_start = 0;
    let mut line_width = 0.0;

    for i in 0..words.len() {
        let would_add = if i == line_start { words[i].width } else { space_width + words[i].width };
        if i > line_start && line_width + would_add > available_width {
            lines.push(&words[line_start..i]);
            line_start = i;
            line_width = words[i].width;
        } else {
            line_width += would_add;
        }
    }
    lines.push(&words[line_start..]);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_dom::NodeData;

    fn word(text: &str, style_node: NodeId) -> Word {
        Word { text: text.to_string(), width: text_width(text, 16.0), style_node }
    }

    fn dummy_node() -> NodeId {
        let mut doc = Document::new();
        doc.create_node(NodeData::Text { data: String::new() })
    }

    #[test]
    fn text_width_scales_with_font_size_and_length() {
        assert_eq!(text_width("abc", 16.0), 3.0 * char_width(16.0));
        assert_eq!(text_width("abcabc", 16.0), 2.0 * text_width("abc", 16.0));
        assert!(text_width("abc", 32.0) > text_width("abc", 16.0));
    }

    #[test]
    fn default_line_height_is_larger_than_font_size() {
        assert!(default_line_height(16.0) > 16.0);
    }

    #[test]
    fn empty_words_produce_no_lines() {
        let lines: Vec<&[Word]> = break_into_lines(&[], 1000.0, 5.0);
        assert!(lines.is_empty());
    }

    #[test]
    fn words_that_all_fit_stay_on_one_line() {
        let n = dummy_node();
        let words = vec![word("a", n), word("b", n), word("c", n)];
        let lines = break_into_lines(&words, 1000.0, 2.0);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].len(), 3);
    }

    #[test]
    fn words_wrap_onto_a_new_line_when_they_would_overflow() {
        let n = dummy_node();
        // "aaaa" (width 4*0.6*16=38.4) + space(9.6) + "bbbb"(38.4) = 86.4,
        // just over 80 -- must wrap.
        let words = vec![word("aaaa", n), word("bbbb", n)];
        let space = char_width(16.0);
        let lines = break_into_lines(&words, 80.0, space);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], &words[0..1]);
        assert_eq!(lines[1], &words[1..2]);
    }

    #[test]
    fn a_single_word_wider_than_available_width_still_gets_its_own_line() {
        let n = dummy_node();
        let words = vec![word("averyverylongword", n)];
        let lines = break_into_lines(&words, 1.0, 1.0);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].len(), 1);
    }

    #[test]
    fn three_words_two_fit_third_wraps() {
        let n = dummy_node();
        let space = char_width(16.0);
        // three words of width 20 each, space width ~9.6: two fit in 60
        // (20+9.6+20=49.6), a third would push to 79.2 > 60.
        let words: Vec<Word> = (0..3).map(|_| Word { text: "xx".to_string(), width: 20.0, style_node: n }).collect();
        let lines = break_into_lines(&words, 60.0, space);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].len(), 2);
        assert_eq!(lines[1].len(), 1);
    }

    #[test]
    fn collect_words_splits_on_whitespace_and_skips_empty_text() {
        let mut doc = Document::new();
        let root = doc.root();
        let p = doc.create_node(NodeData::Element { tag_name: "p".to_string(), attributes: vec![] });
        doc.append_child(root, p);
        let text = doc.create_node(NodeData::Text { data: "  hello   world  ".to_string() });
        doc.append_child(p, text);

        let styles = StyleMap::new();
        let mut words = Vec::new();
        collect_words(&doc, p, &styles, p, &mut words);
        assert_eq!(words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(), vec!["hello", "world"]);
    }

    #[test]
    fn collect_words_flattens_nested_inline_elements_tagging_their_own_style_node() {
        let mut doc = Document::new();
        let root = doc.root();
        let p = doc.create_node(NodeData::Element { tag_name: "p".to_string(), attributes: vec![] });
        doc.append_child(root, p);
        let t1 = doc.create_node(NodeData::Text { data: "one".to_string() });
        doc.append_child(p, t1);
        let b = doc.create_node(NodeData::Element { tag_name: "b".to_string(), attributes: vec![] });
        doc.append_child(p, b);
        let t2 = doc.create_node(NodeData::Text { data: "two".to_string() });
        doc.append_child(b, t2);

        let styles = StyleMap::new();
        let mut words = Vec::new();
        collect_words(&doc, p, &styles, p, &mut words);
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].style_node, p);
        assert_eq!(words[1].style_node, b, "word inside <b> is tagged with <b>, not <p>");
    }

    #[test]
    fn collect_words_uses_the_style_nodes_own_font_size() {
        let mut doc = Document::new();
        let root = doc.root();
        let p = doc.create_node(NodeData::Element { tag_name: "p".to_string(), attributes: vec![] });
        doc.append_child(root, p);
        let text = doc.create_node(NodeData::Text { data: "hi".to_string() });
        doc.append_child(p, text);

        let mut styles = StyleMap::new();
        styles.insert(
            p,
            ComputedStyle {
                display: "block".to_string(),
                color: blueice_css::Color::Rgba(0, 0, 0, 255),
                font_size_px: 32.0,
                font_family: None,
                font_style: None,
                line_height: None,
                text_align: None,
                other: Default::default(),
            },
        );
        let mut words = Vec::new();
        collect_words(&doc, p, &styles, p, &mut words);
        assert_eq!(words[0].width, text_width("hi", 32.0));
    }
}
