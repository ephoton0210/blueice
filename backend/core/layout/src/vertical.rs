// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Vertical writing modes (`writing-mode: vertical-rl | vertical-lr`).
//!
//! A vertical box is laid out in a turned frame and then turned back. Inside it, `Ctx::writing`
//! makes every style read name the sides the turned frame sees (its `width` is the page's
//! `height`, its `left` the page's top, ...), so the whole block and inline algorithm runs
//! unchanged with an inline size equal to the box's height; the result is then rotated into the
//! page's frame. A box's own margin, border, padding and background stay in the page's frame,
//! laid out by the box's parent as for any block.
//!
//! In the turned frame a line is a row; in the page it is a column. Blocks stack from the right
//! (`vertical-rl`) or the left (`vertical-lr`); inside a line, the side letters grow toward is
//! the page's right in both. Text runs are cut where their orientation changes: ideographs,
//! kana, hangul and fullwidth forms stand upright, everything else lies on its side (clockwise),
//! as `text-orientation: mixed` says; `upright` and `sideways` force one.

use crate::block::{layout_flow, PendingAbs};
use crate::ctx::{Ctx, Writing};
use crate::fragment::{Fragment, FragmentKind, TextOrient};
use blueice_dom::NodeId;

/// What laying out a vertical box's content produced, in the page's frame, relative to the
/// box's content box.
pub(crate) struct VerticalFlow {
    pub children: Vec<Fragment>,
    /// The content's height: the box's own when it has one, otherwise the longest line.
    pub height: f64,
    pub pending_abs: Vec<PendingAbs>,
}

/// Lays out `node`'s children as vertical text in a content box `content_width` wide (across the
/// lines) and `content_height` tall if it has a height. `inline_room` is how long a line may be
/// when the height is not set.
pub(crate) fn layout_vertical_flow(
    ctx: &Ctx<'_>,
    node: NodeId,
    mode: Writing,
    content_width: f64,
    content_height: Option<f64>,
    inline_room: f64,
) -> VerticalFlow {
    let length = content_height.unwrap_or(inline_room);
    let previous = ctx.writing.replace(mode);
    let flow = layout_flow(ctx, node, length, None, true, true, &[]);
    let mut children = flow.children;
    rotate(ctx, &mut children, content_width, mode == Writing::VerticalRl, mode);
    ctx.writing.set(previous);
    VerticalFlow {
        children,
        height: content_height.unwrap_or_else(|| flow.extent.clamp(0.0, length)),
        pending_abs: Vec::new(),
    }
}

/// Whether `c` stands upright in a vertical line under `text-orientation: mixed`.
fn upright_in_mixed(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x11FF | 0x2E80..=0x2FDF | 0x3000..=0x303F | 0x3040..=0x30FF | 0x3100..=0x312F
        | 0x3130..=0x318F | 0x31A0..=0x31FF | 0x3200..=0x4DBF | 0x4E00..=0x9FFF | 0xA960..=0xA97F
        | 0xAC00..=0xD7FF | 0xF900..=0xFAFF | 0xFE10..=0xFE1F | 0xFE30..=0xFE6F | 0xFF00..=0xFFEF
        | 0x1F000..=0x1FAFF | 0x20000..=0x2FA1F)
}

/// The orientation a character of `style` takes in a vertical line.
fn orient_of(c: char, text_orientation: Option<&str>) -> TextOrient {
    match text_orientation {
        Some("upright") => TextOrient::Upright,
        Some("sideways" | "sideways-right") => TextOrient::Sideways,
        _ if upright_in_mixed(c) => TextOrient::Upright,
        _ => TextOrient::Sideways,
    }
}

/// Turns `children` (laid out in the turned frame, positions relative to their parent's content
/// box, which is `block_size` thick across the lines) into the page's frame. `reverse` is
/// whether blocks stack from the far side, as they do from the right in `vertical-rl`, and
/// inside every line.
fn rotate(ctx: &Ctx<'_>, children: &mut Vec<Fragment>, block_size: f64, reverse: bool, mode: Writing) {
    let mut out = Vec::with_capacity(children.len());
    for mut child in std::mem::take(children) {
        let across = if reverse {
            block_size - (child.y + child.height)
        } else {
            child.y
        };
        let (along, length, thick) = (child.x, child.width, child.height);
        // What is inside the child is turned relative to the child's own content box.
        let (inner_block, inner_reverse) = match &child.kind {
            FragmentKind::Line => (child.height, true),
            _ => {
                let style = ctx.style_of_fragment(&child);
                let (pad, border) = (style.padding(0.0), style.border(0.0));
                (
                    (child.height - pad.top - pad.bottom - border.top - border.bottom).max(0.0),
                    mode == Writing::VerticalRl,
                )
            }
        };
        if let FragmentKind::Text(text) = &child.kind {
            let text = text.clone();
            let line_across = across;
            out.extend(split_runs(ctx, &child, &text, along, length, thick, line_across));
            continue;
        }
        rotate(ctx, &mut child.children, inner_block, inner_reverse, mode);
        child.x = across;
        child.y = along;
        child.width = thick;
        child.height = length;
        child.baseline = None;
        out.push(child);
    }
    *children = out;
}

/// A text fragment of the turned frame as the runs it is made of in the page's frame: one
/// fragment for each stretch of characters that share an orientation, one below the other.
fn split_runs(
    ctx: &Ctx<'_>,
    f: &Fragment,
    text: &str,
    along: f64,
    length: f64,
    thick: f64,
    across: f64,
) -> Vec<Fragment> {
    let style = f.node.map(|n| ctx.style(n));
    let orientation = style.as_ref().and_then(|s| s.keyword("text-orientation"));
    let mut runs: Vec<(String, TextOrient)> = Vec::new();
    for c in text.chars() {
        let o = orient_of(c, orientation);
        match runs.last_mut() {
            Some((run, last)) if *last == o => run.push(c),
            _ => runs.push((c.to_string(), o)),
        }
    }
    let measure = |run: &str| match &style {
        Some(s) => blueice_font::measure_text_width_in(
            run,
            s.font_size(),
            s.style.is_some_and(|st| st.is_bold()),
            s.style.is_some_and(|st| st.is_italic()),
            s.web_face(),
        ),
        None => blueice_font::measure_text_width(run, 16.0, false, false),
    };
    let count = runs.len();
    let mut cursor = along;
    runs.into_iter()
        .enumerate()
        .map(|(i, (run, orient))| {
            // A single run is the whole fragment, so its length is exactly the layout's.
            let w = if count == 1 { length } else { measure(&run) };
            let fragment = Fragment {
                kind: FragmentKind::Text(run),
                x: across,
                y: cursor,
                width: thick,
                height: w,
                orient,
                rtl: false,
                ..f.clone()
            };
            cursor += w;
            let _ = i;
            Fragment {
                children: Vec::new(),
                ..fragment
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::block::layout_block;
    use crate::fragment::{Fragment, FragmentKind, TextOrient};
    use blueice_css::{cascade, ua_stylesheet, Origin};
    use blueice_dom::{NodeData, NodeId};

    fn find(doc: &blueice_dom::Document, from: NodeId, tag: &str) -> Option<NodeId> {
        if matches!(doc.data(from), NodeData::Element { tag_name, .. } if tag_name == tag) {
            return Some(from);
        }
        doc.children(from).find_map(|c| find(doc, c, tag))
    }

    /// The body of `html` laid out 400px wide (viewport 720px tall), with the UA sheet and `css`.
    fn body(html: &str, css: &str) -> Fragment {
        let doc = blueice_html::parse(html);
        let ua = ua_stylesheet();
        let author = blueice_css::parse(&format!(
            "html,body,div,p{{margin:0;padding:0}} body{{font-size:16px}} {css}"
        ))
        .rules;
        let styles = cascade(&doc, &[(Origin::Ua, &ua), (Origin::Author, &author)]);
        let body = find(&doc, doc.root(), "body").unwrap();
        layout_block(&doc, body, &styles, 400.0)
    }

    /// Every text fragment under `f` as `(text, orient, x, y, width, height)` in the coordinates
    /// of the root's content box.
    fn texts(f: &Fragment) -> Vec<(String, TextOrient, f64, f64, f64, f64)> {
        fn walk(
            f: &Fragment,
            (ox, oy): (f64, f64),
            out: &mut Vec<(String, TextOrient, f64, f64, f64, f64)>,
        ) {
            let (x, y) = (ox + f.x, oy + f.y);
            if let FragmentKind::Text(t) = &f.kind {
                out.push((t.clone(), f.orient, x, y, f.width, f.height));
            }
            for c in &f.children {
                walk(c, (x, y), out);
            }
        }
        let mut out = Vec::new();
        for c in &f.children {
            walk(c, (0.0, 0.0), &mut out);
        }
        out
    }

    fn div(f: &Fragment) -> &Fragment {
        &f.children[0]
    }

    const LATIN: &str = "aaa bbb ccc ddd eee fff";

    #[test]
    fn a_vertical_box_keeps_its_physical_size_and_its_text_runs_down() {
        let f = body(
            &format!("<div>{LATIN}</div>"),
            "div { writing-mode: vertical-rl; width: 100px; height: 300px }",
        );
        let d = div(&f);
        assert_eq!((d.width, d.height), (100.0, 300.0));
        let t = texts(d);
        assert!(!t.is_empty());
        for (_, orient, x, y, w, h) in &t {
            assert_eq!(*orient, TextOrient::Sideways, "Latin lies on its side");
            assert!(*x >= 0.0 && x + w <= 100.0 + 0.01, "inside the box across: {x} {w}");
            assert!(*y >= 0.0 && y + h <= 300.0 + 0.01, "inside the box along: {y} {h}");
        }
        // "aaa bbb ccc ddd eee fff" is about 150px of text, so it is one column of six words
        assert_eq!(t.len(), 6);
        assert!(t.iter().all(|r| r.2 == t[0].2), "all in one column");
        assert_eq!(t[0].3, 0.0, "starts at the top");
        assert!(t.windows(2).all(|w| w[1].3 > w[0].3), "each word is below the one before");
    }

    #[test]
    fn a_text_run_is_as_long_down_the_page_as_it_is_wide_across_a_horizontal_page() {
        let vertical = body("<div>hello</div>", "div { writing-mode: vertical-rl; width: 60px; height: 200px }");
        let horizontal = body("<div>hello</div>", "div { width: 200px }");
        let v = &texts(div(&vertical))[0];
        let h = &texts(div(&horizontal))[0];
        assert!((v.5 - h.4).abs() < 0.01, "length down == length across: {} vs {}", v.5, h.4);
        assert!((v.4 - h.5).abs() < 0.01, "column width == line height: {} vs {}", v.4, h.5);
    }

    #[test]
    fn vertical_rl_stacks_lines_from_the_right_and_vertical_lr_from_the_left() {
        let css = |mode: &str| {
            format!("div {{ writing-mode: {mode}; width: 200px; height: 60px; line-height: 20px }}")
        };
        let lines = |mode: &str| {
            let f = body(&format!("<div>{LATIN}</div>"), &css(mode));
            let first = texts(div(&f)); // in reading order
            let mut columns: Vec<f64> = first.iter().map(|t| t.2).collect();
            columns.dedup(); // the words of one line share a column
            assert!(columns.len() >= 3, "{first:?}");
            columns
        };
        let rl = lines("vertical-rl");
        assert!(rl.windows(2).all(|w| w[0] > w[1]), "each line is left of the one before: {rl:?}");
        assert!(rl[0] + 20.0 > 199.0, "the first line is at the right edge");
        let lr = lines("vertical-lr");
        assert!(lr.windows(2).all(|w| w[0] < w[1]), "each line is right of the one before: {lr:?}");
        assert!(lr[0] < 1.0, "the first line is at the left edge");
    }

    #[test]
    fn ideographs_stand_upright_and_latin_lies_down_in_the_same_line() {
        let f = body(
            "<div>\u{65e5}\u{672c}\u{8a9e}abc</div>",
            "div { writing-mode: vertical-rl; width: 60px; height: 300px }",
        );
        let t = texts(div(&f));
        assert_eq!(t.len(), 2, "{t:?}");
        assert_eq!((t[0].0.as_str(), t[0].1), ("\u{65e5}\u{672c}\u{8a9e}", TextOrient::Upright));
        assert_eq!((t[1].0.as_str(), t[1].1), ("abc", TextOrient::Sideways));
        assert_eq!(t[0].3, 0.0);
        assert!((t[1].3 - (t[0].3 + t[0].5)).abs() < 0.01, "abc follows 日本語 down the line");
        // an upright run is three characters tall, one em each
        assert!((t[0].5 - 48.0).abs() < 1.0, "{}", t[0].5);
    }

    #[test]
    fn text_orientation_upright_stands_latin_up_too() {
        let f = body(
            "<div>abc</div>",
            "div { writing-mode: vertical-rl; text-orientation: upright; width: 60px; height: 300px }",
        );
        assert_eq!(texts(div(&f))[0].1, TextOrient::Upright);
    }

    #[test]
    fn an_auto_height_vertical_box_is_as_tall_as_its_longest_line() {
        let f = body("<div>hello</div>", "div { writing-mode: vertical-rl; width: 60px }");
        let d = div(&f);
        let run = texts(d)[0].5;
        assert_eq!(d.width, 60.0);
        assert!((d.height - run).abs() < 0.01, "height {} is the line's length {run}", d.height);
    }

    #[test]
    fn lines_break_at_the_height_of_the_box() {
        let f = body(
            "<div>\u{65e5}\u{672c}\u{8a9e}\u{65e5}\u{672c}\u{8a9e}\u{65e5}\u{672c}\u{8a9e}\u{65e5}\u{672c}\u{8a9e}</div>",
            "div { writing-mode: vertical-rl; width: 200px; height: 100px }",
        );
        let t = texts(div(&f));
        assert!(t.len() >= 2, "twelve characters do not fit in 100px: {t:?}");
        assert!(t.iter().all(|r| r.3 + r.5 <= 100.01), "no line is longer than the box");
    }

    #[test]
    fn padding_follows_the_physical_sides() {
        // padding-top is at the start of each line in a vertical box
        let f = body(
            "<div>x</div>",
            "div { writing-mode: vertical-rl; width: 100px; height: 200px; padding: 10px 0 0 0; box-sizing: border-box }",
        );
        let t = &texts(div(&f))[0];
        assert_eq!(t.3, 10.0, "the text starts below the top padding");
        // padding-right is at the start of the block direction: the first column is inset by it
        let f = body(
            "<div>x</div>",
            "div { writing-mode: vertical-rl; width: 100px; height: 200px; padding: 0 30px 0 0; box-sizing: border-box }",
        );
        let t = &texts(div(&f))[0];
        assert!(t.2 + t.4 <= 70.01, "the column ends 30px short of the right edge: {} + {}", t.2, t.4);
    }

    #[test]
    fn horizontal_content_around_a_vertical_box_is_untouched() {
        let f = body(
            "<p>before</p><div>x</div><p>after</p>",
            "div { writing-mode: vertical-rl; width: 50px; height: 80px }",
        );
        assert_eq!(f.children[0].y, 0.0);
        assert_eq!(f.children[1].y, f.children[0].height);
        assert_eq!(f.children[2].y, f.children[1].y + 80.0);
        assert!(texts(&f.children[0]).iter().all(|t| t.1 == TextOrient::Horizontal));
    }
}
