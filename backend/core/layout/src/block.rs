// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The block/inline layout algorithm, per `research/layout.md` §4: a
//! single-pass, top-down walk that stacks block-level children in the
//! block direction and groups consecutive inline-level children
//! (elements and text) into anonymous line-box runs. No BFC-offset
//! bubbling, no float exclusion space, no CSS2 margin-collapsing
//! "strut" machinery -- `research/layout.md` names these as refinements
//! *within* the block algorithm, safe to cut for an MVP that isn't
//! rendering floats yet.
//!
//! `display` dispatch happens once, in [`classify`]: `none` removes the
//! element and its whole subtree from layout, `inline` joins the
//! current line-box run, and everything else (`block`, `inline-block`,
//! `flex`) currently falls through to an ordinary block box -- this is
//! the seam `research/layout.md` says flex/grid/table plug into later
//! as new arms, not a rewrite. `inline-block`'s real behavior (an
//! atomic inline-level box whose *own* content lays out like a block)
//! isn't implemented -- it's simplified to plain `block` for now, since
//! atomic-inline sizing needs the same text-measurement infrastructure
//! `text.rs` already flags as a placeholder.

use crate::fragment::{Fragment, FragmentKind};
use crate::text::{break_into_lines, char_width, collect_words, default_line_height, StyleMap, Word};
use blueice_css::{Length, Value};
use blueice_dom::{Document, NodeData, NodeId};
use std::collections::HashMap;

enum BoxGen {
    Block,
    Inline,
    None,
}

fn classify(doc: &Document, styles: &StyleMap, node: NodeId) -> BoxGen {
    match doc.data(node) {
        NodeData::Text { .. } => BoxGen::Inline,
        NodeData::Element { .. } => match styles.get(&node).map(|s| s.display.as_str()) {
            Some("none") => BoxGen::None,
            Some("inline") => BoxGen::Inline,
            _ => BoxGen::Block,
        },
        NodeData::Document => BoxGen::None,
    }
}

fn length_px(value: &Value, font_size: f64, percentage_base: f64) -> Option<f64> {
    match value {
        Value::Length(Length::Px(n)) => Some(*n),
        Value::Length(Length::Em(n)) => Some(n * font_size),
        Value::Length(Length::Zero) => Some(0.0),
        Value::Percentage(p) => Some(percentage_base * p / 100.0),
        _ => None,
    }
}

/// A box-model side (margin/padding/border-width): 0 if unset, if the
/// value doesn't resolve to a length (e.g. `margin: auto`), or if it's
/// an outright unsupported shape -- auto-margin centering is a named
/// MVP cut (`phase-2-mvp-scope/PLAN.md` doesn't call for it, and a
/// "just don't center" fallback is a safe, visible-not-silent
/// degradation rather than a crash).
fn side(other: &HashMap<String, Value>, prop: &str, font_size: f64, percentage_base: f64) -> f64 {
    other.get(prop).and_then(|v| length_px(v, font_size, percentage_base)).unwrap_or(0.0)
}

fn is_border_box(other: &HashMap<String, Value>) -> bool {
    matches!(other.get("box-sizing"), Some(Value::Keyword(k)) if k == "border-box")
}

/// `None` means "auto" (or absent) -- the caller fills remaining space.
fn resolve_width(other: &HashMap<String, Value>, font_size: f64, containing_width: f64) -> Option<f64> {
    match other.get("width") {
        None => None,
        Some(Value::Keyword(k)) if k == "auto" => None,
        Some(v) => length_px(v, font_size, containing_width),
    }
}

/// Only resolves an explicit length -- a percentage height against an
/// (MVP-only-ever) auto-sized containing block is indeterminate per
/// spec anyway, so treating it the same as "auto" here is a conservative
/// simplification, not a wrong general rule.
fn resolve_height(other: &HashMap<String, Value>, font_size: f64) -> Option<f64> {
    match other.get("height") {
        Some(Value::Length(Length::Px(n))) => Some(*n),
        Some(Value::Length(Length::Em(n))) => Some(n * font_size),
        Some(Value::Length(Length::Zero)) => Some(0.0),
        _ => None,
    }
}

fn resolve_line_height(value: &Value, font_size: f64) -> Option<f64> {
    match value {
        Value::Number(n) => Some(n * font_size),
        Value::Length(Length::Px(n)) => Some(*n),
        Value::Length(Length::Em(n)) => Some(n * font_size),
        Value::Length(Length::Zero) => Some(0.0),
        Value::Percentage(p) => Some(font_size * p / 100.0),
        _ => None,
    }
}

pub(crate) fn layout_block(doc: &Document, node: NodeId, styles: &StyleMap, available_width: f64) -> Fragment {
    let style = styles.get(&node);
    let font_size = style.map(|s| s.font_size_px).unwrap_or(16.0);
    let empty = HashMap::new();
    let other = style.map(|s| &s.other).unwrap_or(&empty);

    // A fragment's own margin never affects its own box or its
    // children's layout -- only how its *parent* positions it (see the
    // `child_margin_*` computation in the loop below). Only the
    // horizontal margins matter here, for how much width is left over
    // for an `auto`-width box to fill.
    let margin_right = side(other, "margin-right", font_size, available_width);
    let margin_left = side(other, "margin-left", font_size, available_width);
    let padding_top = side(other, "padding-top", font_size, available_width);
    let padding_right = side(other, "padding-right", font_size, available_width);
    let padding_bottom = side(other, "padding-bottom", font_size, available_width);
    let padding_left = side(other, "padding-left", font_size, available_width);
    let border_top = side(other, "border-top-width", font_size, available_width);
    let border_right = side(other, "border-right-width", font_size, available_width);
    let border_bottom = side(other, "border-bottom-width", font_size, available_width);
    let border_left = side(other, "border-left-width", font_size, available_width);

    let border_box_sizing = is_border_box(other);
    let horizontal_padding_border = padding_left + padding_right + border_left + border_right;

    let content_width = match resolve_width(other, font_size, available_width) {
        Some(w) if border_box_sizing => (w - horizontal_padding_border).max(0.0),
        Some(w) => w,
        None => (available_width - margin_left - margin_right - horizontal_padding_border).max(0.0),
    };

    let mut children_fragments = Vec::new();
    let mut cursor_y = 0.0;
    let mut pending_inline: Vec<NodeId> = Vec::new();

    for child in doc.children(node) {
        match classify(doc, styles, child) {
            BoxGen::None => continue,
            BoxGen::Inline => pending_inline.push(child),
            BoxGen::Block => {
                if !pending_inline.is_empty() {
                    let (lines, used_height) = layout_inline_run(doc, &pending_inline, styles, content_width, cursor_y);
                    children_fragments.extend(lines);
                    cursor_y += used_height;
                    pending_inline.clear();
                }

                let child_style = styles.get(&child);
                let child_font_size = child_style.map(|s| s.font_size_px).unwrap_or(font_size);
                let child_empty = HashMap::new();
                let child_other = child_style.map(|s| &s.other).unwrap_or(&child_empty);
                let child_margin_top = side(child_other, "margin-top", child_font_size, content_width);
                let child_margin_bottom = side(child_other, "margin-bottom", child_font_size, content_width);
                let child_margin_left = side(child_other, "margin-left", child_font_size, content_width);

                cursor_y += child_margin_top;
                let mut child_fragment = layout_block(doc, child, styles, content_width);
                child_fragment.x = child_margin_left;
                child_fragment.y = cursor_y;
                cursor_y += child_fragment.height + child_margin_bottom;
                children_fragments.push(child_fragment);
            }
        }
    }
    if !pending_inline.is_empty() {
        let (lines, used_height) = layout_inline_run(doc, &pending_inline, styles, content_width, cursor_y);
        children_fragments.extend(lines);
        cursor_y += used_height;
    }

    let intrinsic_content_height = cursor_y;
    let content_height = match resolve_height(other, font_size) {
        Some(h) if border_box_sizing => (h - padding_top - padding_bottom - border_top - border_bottom).max(0.0),
        Some(h) => h,
        None => intrinsic_content_height,
    };

    let content_origin_x = padding_left + border_left;
    let content_origin_y = padding_top + border_top;
    for f in &mut children_fragments {
        f.x += content_origin_x;
        f.y += content_origin_y;
    }

    Fragment {
        node: Some(node),
        kind: FragmentKind::Block,
        x: 0.0,
        y: 0.0,
        width: content_width + horizontal_padding_border,
        height: content_height + padding_top + padding_bottom + border_top + border_bottom,
        children: children_fragments,
    }
}

fn layout_inline_run(doc: &Document, pending: &[NodeId], styles: &StyleMap, available_width: f64, y_offset: f64) -> (Vec<Fragment>, f64) {
    let mut words: Vec<Word> = Vec::new();
    for &n in pending {
        match doc.data(n) {
            NodeData::Text { .. } => {
                if let Some(parent) = doc.parent(n) {
                    collect_words(doc, n, styles, parent, &mut words);
                }
            }
            NodeData::Element { .. } => collect_words(doc, n, styles, n, &mut words),
            NodeData::Document => {}
        }
    }
    if words.is_empty() {
        return (Vec::new(), 0.0);
    }

    let first_style = styles.get(&words[0].style_node);
    let font_size = first_style.map(|s| s.font_size_px).unwrap_or(16.0);
    let line_height = first_style
        .and_then(|s| s.line_height.as_ref())
        .and_then(|v| resolve_line_height(v, font_size))
        .unwrap_or_else(|| default_line_height(font_size));
    let space_width = char_width(font_size);

    let lines = break_into_lines(&words, available_width, space_width);
    let mut fragments = Vec::new();
    let mut y = y_offset;
    for line_words in &lines {
        fragments.push(layout_line(line_words, line_height, y, space_width));
        y += line_height;
    }
    let total_height = lines.len() as f64 * line_height;
    (fragments, total_height)
}

fn layout_line(words: &[Word], line_height: f64, y: f64, space_width: f64) -> Fragment {
    let mut x = 0.0;
    let mut children = Vec::new();
    for (i, w) in words.iter().enumerate() {
        if i > 0 {
            x += space_width;
        }
        children.push(Fragment { node: Some(w.style_node), kind: FragmentKind::Text(w.text.clone()), x, y: 0.0, width: w.width, height: line_height, children: Vec::new() });
        x += w.width;
    }
    Fragment { node: None, kind: FragmentKind::Line, x: 0.0, y, width: x, height: line_height, children }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_css::{cascade, ua_stylesheet, Origin};

    fn find_by_tag(doc: &Document, root: NodeId, tag: &str) -> Option<NodeId> {
        if let NodeData::Element { tag_name, .. } = doc.data(root) {
            if tag_name == tag {
                return Some(root);
            }
        }
        doc.children(root).find_map(|c| find_by_tag(doc, c, tag))
    }

    fn styled(doc: &Document, html: &str, css: &str) -> (Document, StyleMap) {
        let _ = doc;
        let doc = blueice_html::parse(html);
        let ua = ua_stylesheet();
        let author = blueice_css::parse(css).rules;
        let styles = cascade(&doc, &[(Origin::Ua, &ua), (Origin::Author, &author)]);
        (doc, styles)
    }

    fn body_fragment(html: &str, css: &str, available_width: f64) -> Fragment {
        let placeholder = Document::new();
        let (doc, styles) = styled(&placeholder, html, css);
        let body = find_by_tag(&doc, doc.root(), "body").unwrap();
        layout_block(&doc, body, &styles, available_width)
    }

    #[test]
    fn plain_block_fills_available_width_with_zero_box_model() {
        let f = body_fragment("<div>x</div>", "", 800.0);
        let div = &f.children[0];
        assert_eq!(div.width, 800.0);
        assert_eq!(div.x, 0.0);
    }

    #[test]
    fn explicit_width_and_px_margin_are_respected() {
        let f = body_fragment("<div>x</div>", "div { width: 200px; margin: 10px; }", 800.0);
        let div = &f.children[0];
        assert_eq!(div.width, 200.0);
        assert_eq!(div.x, 10.0, "left margin offsets the box");
        assert_eq!(div.y, 10.0, "top margin offsets the box");
    }

    #[test]
    fn percentage_width_resolves_against_containing_block() {
        let f = body_fragment("<div>x</div>", "div { width: 50%; }", 800.0);
        assert_eq!(f.children[0].width, 400.0);
    }

    #[test]
    fn em_margin_resolves_against_the_elements_own_font_size() {
        let f = body_fragment("<div>x</div>", "div { font-size: 20px; margin-top: 2em; }", 800.0);
        assert_eq!(f.children[0].y, 40.0);
    }

    #[test]
    fn box_sizing_border_box_subtracts_padding_and_border_from_specified_width() {
        let f = body_fragment(
            "<div>x</div>",
            "div { width: 200px; padding: 10px; border-left-width: 5px; border-right-width: 5px; box-sizing: border-box; }",
            800.0,
        );
        assert_eq!(f.children[0].width, 200.0, "border-box width is the total, not content + extra");
    }

    #[test]
    fn content_box_sizing_adds_padding_and_border_on_top_of_specified_width() {
        let f = body_fragment("<div>x</div>", "div { width: 200px; padding: 10px; }", 800.0);
        assert_eq!(f.children[0].width, 220.0);
    }

    #[test]
    fn two_block_children_stack_vertically_by_height_plus_margins() {
        let f = body_fragment(
            "<div>a</div><div>b</div>",
            "div { height: 50px; margin-bottom: 10px; }",
            800.0,
        );
        assert_eq!(f.children.len(), 2);
        assert_eq!(f.children[0].y, 0.0);
        assert_eq!(f.children[1].y, 60.0, "second div starts after first's height + margin-bottom");
    }

    // ---- interaction tests: added by a dedicated post-implementation
    // test-review pass (per TEST_PLAN.md's Definition of Done) -- each
    // of these combines two features whose own single-feature tests
    // above don't actually exercise together.

    #[test]
    fn padding_offsets_a_nested_block_child_not_just_inline_content() {
        // every existing padding/content-origin test above only checks
        // it against inline text; a block *child* needs the same
        // content_origin_x/y offset applied to it.
        let f = body_fragment("<div><p>x</p></div>", "div { padding: 10px; }", 800.0);
        let div = &f.children[0];
        let p = &div.children[0];
        assert_eq!(p.x, 10.0);
        assert_eq!(p.y, 10.0);
    }

    #[test]
    fn inline_run_flushes_before_a_following_block_sibling() {
        let f = body_fragment("<div>text<p>block</p></div>", "", 800.0);
        let div = &f.children[0];
        assert_eq!(div.children.len(), 2, "one line fragment, then the p's block fragment");
        let line = &div.children[0];
        assert_eq!(line.kind, FragmentKind::Line);
        let p = &div.children[1];
        assert_eq!(p.kind, FragmentKind::Block);
        assert_eq!(p.y, line.y + line.height, "p starts immediately after the flushed line, not overlapping it");
    }

    #[test]
    fn inline_run_also_flushes_correctly_when_it_comes_after_a_block_sibling() {
        let f = body_fragment("<p>block</p>text", "", 800.0);
        let p = &f.children[0];
        assert_eq!(p.kind, FragmentKind::Block);
        let line = &f.children[1];
        assert_eq!(line.kind, FragmentKind::Line);
        assert_eq!(line.y, p.y + p.height, "trailing inline text starts after the preceding block, not at y=0");
    }

    #[test]
    fn display_none_removes_the_element_and_its_subtree() {
        let f = body_fragment("<div>a</div><div class=\"gone\"><span>x</span></div><div>b</div>", ".gone { display: none; }", 800.0);
        assert_eq!(f.children.len(), 2, "the display:none div contributes no fragment at all");
    }

    #[test]
    fn inline_text_produces_a_line_fragment_with_correct_word_positions() {
        let f = body_fragment("<p>hello world</p>", "", 800.0);
        let p = &f.children[0];
        assert_eq!(p.children.len(), 1, "one line, both words fit");
        let line = &p.children[0];
        assert_eq!(line.kind, FragmentKind::Line);
        assert_eq!(line.children.len(), 2);
        assert_eq!(line.children[0].kind, FragmentKind::Text("hello".to_string()));
        assert_eq!(line.children[1].kind, FragmentKind::Text("world".to_string()));
        assert!(line.children[1].x > line.children[0].x + line.children[0].width, "world starts after hello plus a space gap");
    }

    #[test]
    fn narrow_container_wraps_inline_content_onto_multiple_lines() {
        let f = body_fragment("<p>aaaa bbbb cccc</p>", "", 40.0);
        let p = &f.children[0];
        assert!(p.children.len() > 1, "text must wrap across multiple lines in a narrow container");
        for line in &p.children {
            assert_eq!(line.kind, FragmentKind::Line);
        }
    }

    #[test]
    fn nested_inline_element_words_are_tagged_with_their_own_node() {
        let placeholder = Document::new();
        let (doc, styles) = styled(&placeholder, "<p>a <b>bold</b> c</p>", "");
        let body = find_by_tag(&doc, doc.root(), "body").unwrap();
        let f = layout_block(&doc, body, &styles, 800.0);
        let p = &f.children[0];
        let b_node = find_by_tag(&doc, doc.root(), "b").unwrap();

        let line = &p.children[0];
        let bold_word = line.children.iter().find(|w| w.kind == FragmentKind::Text("bold".to_string())).unwrap();
        assert_eq!(bold_word.node, Some(b_node), "the word's style node is <b>, not <p>");
    }

    #[test]
    fn empty_element_produces_a_zero_height_block() {
        let f = body_fragment("<div></div>", "", 800.0);
        assert_eq!(f.children[0].height, 0.0);
    }

    #[test]
    fn explicit_height_overrides_intrinsic_content_height() {
        let f = body_fragment("<div>hi</div>", "div { height: 500px; }", 800.0);
        assert_eq!(f.children[0].height, 500.0);
    }

    #[test]
    fn missing_style_falls_back_to_sane_defaults_without_panicking() {
        let mut doc = Document::new();
        let root = doc.root();
        let div = doc.create_node(NodeData::Element { tag_name: "div".to_string(), attributes: vec![] });
        doc.append_child(root, div);
        let styles: StyleMap = HashMap::new();
        let f = layout_block(&doc, div, &styles, 800.0);
        assert_eq!(f.width, 800.0);
    }

    #[test]
    fn whitespace_only_text_between_block_siblings_contributes_no_phantom_line() {
        let f = body_fragment("<div>a</div>\n  <div>b</div>", "", 800.0);
        assert_eq!(f.children.len(), 2, "no extra Line fragment from the whitespace text node");
    }

    #[test]
    fn line_height_number_multiplies_font_size() {
        let f = body_fragment("<p>hi</p>", "p { line-height: 2; }", 800.0);
        let line = &f.children[0].children[0];
        assert_eq!(line.height, 32.0);
    }
}
