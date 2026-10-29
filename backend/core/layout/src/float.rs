// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Float geometry: the rectangles floated boxes occupy (margin boxes, in the
//! coordinates of the block that is currently being laid out) and the
//! questions block and inline layout ask of them -- how much room a line has
//! at some height, where a new float fits, and where `clear` lands.

use crate::ctx::{Clear, Side};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FloatRect {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub side: Side,
}

pub(crate) fn translate(floats: &[FloatRect], dx: f64, dy: f64) -> Vec<FloatRect> {
    floats
        .iter()
        .map(|f| FloatRect {
            left: f.left + dx,
            right: f.right + dx,
            top: f.top + dy,
            bottom: f.bottom + dy,
            side: f.side,
        })
        .collect()
}

fn intersects(f: &FloatRect, y0: f64, y1: f64) -> bool {
    let y1 = y1.max(y0 + 0.01);
    f.top < y1 && f.bottom > y0 && f.bottom > f.top
}

/// The `(left, right)` edges free of floats over `[y0, y1)` in a block
/// `width` wide.
pub(crate) fn free_span(floats: &[FloatRect], y0: f64, y1: f64, width: f64) -> (f64, f64) {
    let mut left = 0.0f64;
    let mut right = width;
    for f in floats.iter().filter(|f| intersects(f, y0, y1)) {
        match f.side {
            Side::Left => left = left.max(f.right),
            Side::Right => right = right.min(f.left),
        }
    }
    (left, right.max(left))
}

/// The lowest float bottom below `y` among floats that cover `y`: where the
/// next change in available room happens.
pub(crate) fn next_edge(floats: &[FloatRect], y: f64) -> Option<f64> {
    floats
        .iter()
        .filter(|f| f.top <= y && f.bottom > y)
        .map(|f| f.bottom)
        .fold(None, |best, b| Some(best.map_or(b, |m: f64| m.min(b))))
}

/// Where a float of margin-box `w` x `h` goes: `(x, y)` of its margin box,
/// no higher than `top`, as far to its side as earlier floats allow.
pub(crate) fn place(
    floats: &[FloatRect],
    side: Side,
    w: f64,
    h: f64,
    top: f64,
    width: f64,
) -> (f64, f64) {
    let mut y = top;
    for _ in 0..1000 {
        let (left, right) = free_span(floats, y, y + h, width);
        if right - left >= w - 0.01 {
            let x = match side {
                Side::Left => left,
                Side::Right => right - w,
            };
            return (x, y);
        }
        match next_edge(floats, y) {
            Some(next) => y = next,
            None => {
                let x = match side {
                    Side::Left => left,
                    Side::Right => (right - w).max(0.0),
                };
                return (x, y);
            }
        }
    }
    (0.0, y)
}

/// The height below which `clear` is satisfied.
pub(crate) fn clear_y(floats: &[FloatRect], clear: Clear) -> f64 {
    floats
        .iter()
        .filter(|f| match clear {
            Clear::None => false,
            Clear::Both => true,
            Clear::Left => f.side == Side::Left,
            Clear::Right => f.side == Side::Right,
        })
        .map(|f| f.bottom)
        .fold(0.0, f64::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(side: Side, left: f64, top: f64, right: f64, bottom: f64) -> FloatRect {
        FloatRect {
            left,
            top,
            right,
            bottom,
            side,
        }
    }

    #[test]
    fn free_span_shrinks_from_both_sides() {
        let floats = [
            rect(Side::Left, 0.0, 0.0, 100.0, 50.0),
            rect(Side::Right, 300.0, 20.0, 400.0, 80.0),
        ];
        assert_eq!(free_span(&floats, 0.0, 10.0, 400.0), (100.0, 400.0));
        assert_eq!(free_span(&floats, 30.0, 40.0, 400.0), (100.0, 300.0));
        assert_eq!(free_span(&floats, 60.0, 70.0, 400.0), (0.0, 300.0));
        assert_eq!(free_span(&floats, 90.0, 95.0, 400.0), (0.0, 400.0));
    }

    #[test]
    fn a_float_sits_beside_earlier_ones_until_the_row_is_full() {
        let mut floats = vec![rect(Side::Left, 0.0, 0.0, 150.0, 40.0)];
        assert_eq!(place(&floats, Side::Left, 150.0, 30.0, 0.0, 400.0), (150.0, 0.0));
        floats.push(rect(Side::Left, 150.0, 0.0, 300.0, 30.0));
        assert_eq!(
            place(&floats, Side::Left, 150.0, 30.0, 0.0, 400.0),
            (150.0, 30.0),
            "no room beside both: drops below the shorter float and tucks in"
        );
    }

    #[test]
    fn right_floats_hug_the_right_edge() {
        assert_eq!(place(&[], Side::Right, 100.0, 10.0, 5.0, 400.0), (300.0, 5.0));
    }

    #[test]
    fn clear_waits_for_the_named_side() {
        let floats = [
            rect(Side::Left, 0.0, 0.0, 10.0, 40.0),
            rect(Side::Right, 90.0, 0.0, 100.0, 70.0),
        ];
        assert_eq!(clear_y(&floats, Clear::Left), 40.0);
        assert_eq!(clear_y(&floats, Clear::Right), 70.0);
        assert_eq!(clear_y(&floats, Clear::Both), 70.0);
        assert_eq!(clear_y(&floats, Clear::None), 0.0);
    }

    mod layout {
        use crate::block::layout_block;
        use crate::fragment::{Fragment, FragmentKind};
        use blueice_css::{cascade, ua_stylesheet, Origin};
        use blueice_dom::{NodeData, NodeId};

        fn find(doc: &blueice_dom::Document, from: NodeId, tag: &str) -> Option<NodeId> {
            if matches!(doc.data(from), NodeData::Element { tag_name, .. } if tag_name == tag) {
                return Some(from);
            }
            doc.children(from).find_map(|c| find(doc, c, tag))
        }

        pub fn body(html: &str, css: &str, width: f64) -> Fragment {
            let doc = blueice_html::parse(html);
            let ua = ua_stylesheet();
            let author = blueice_css::parse(&format!(
                "body, p {{ margin: 0; }} .f {{ height: 30px; }} {css}"
            ))
            .rules;
            let styles = cascade(&doc, &[(Origin::Ua, &ua), (Origin::Author, &author)]);
            let body = find(&doc, doc.root(), "body").unwrap();
            layout_block(&doc, body, &styles, width)
        }

        pub fn first_text_x(f: &Fragment) -> Option<f64> {
            if let FragmentKind::Line = f.kind {
                return f.children.first().map(|c| f.x + c.x);
            }
            f.children.iter().find_map(first_text_x)
        }
    }

    #[test]
    fn a_left_float_sits_at_the_edge_and_text_flows_beside_it() {
        let f = layout::body(
            "<div class=f style='float:left;width:100px'></div><p>text beside</p>",
            "",
            400.0,
        );
        let float = &f.children[0];
        assert_eq!((float.x, float.y, float.width), (0.0, 0.0, 100.0));
        let line = f.children.iter().find_map(layout::first_text_x).unwrap();
        assert_eq!(line, 100.0, "the paragraph's line starts after the float");
    }

    #[test]
    fn a_right_float_takes_the_right_edge() {
        let f = layout::body(
            "<div class=f style='float:right;width:80px'></div><p>x</p>",
            "",
            400.0,
        );
        assert_eq!(f.children[0].x, 320.0);
    }

    #[test]
    fn text_returns_to_the_edge_below_the_float() {
        let text = "word ".repeat(60);
        let f = layout::body(
            &format!("<div class=f style='float:left;width:100px'></div><p>{text}</p>"),
            "",
            300.0,
        );
        let p = &f.children[1];
        let xs: Vec<f64> = p.children.iter().map(|l| l.x).collect();
        assert_eq!(xs[0], 100.0);
        assert!(xs.iter().any(|&x| x == 0.0), "later lines clear the float: {xs:?}");
    }

    #[test]
    fn floats_stack_side_by_side_then_wrap() {
        let f = layout::body(
            "<div class=f style='float:left;width:150px'></div><div class=f style='float:left;width:150px'></div><div class=f style='float:left;width:150px'></div>",
            "",
            400.0,
        );
        let at: Vec<(f64, f64)> = f.children.iter().map(|c| (c.x, c.y)).collect();
        assert_eq!(at, vec![(0.0, 0.0), (150.0, 0.0), (0.0, 30.0)]);
    }

    #[test]
    fn a_float_does_not_add_height_to_its_plain_parent() {
        let f = layout::body(
            "<div id=o><div class=f style='float:left;width:50px'></div></div><div class=f id=n></div>",
            "",
            400.0,
        );
        assert_eq!(f.children[0].height, 0.0);
    }

    #[test]
    fn a_formatting_context_root_contains_its_floats() {
        let f = layout::body(
            "<div style='overflow:hidden'><div class=f style='float:left;width:50px'></div></div>",
            "",
            400.0,
        );
        assert_eq!(f.children[0].height, 30.0);
    }

    #[test]
    fn clear_moves_a_block_below_the_float() {
        let f = layout::body(
            "<div class=f style='float:left;width:50px'></div><div id=c style='clear:both;height:5px'></div>",
            "",
            400.0,
        );
        assert_eq!(f.children[1].y, 30.0);
    }

    #[test]
    fn a_floated_inline_element_becomes_a_block_with_shrink_to_fit_width() {
        let f = layout::body("<span style='float:left'>abc</span>", "", 400.0);
        let w = blueice_font::measure_text_width("abc", 16.0, false, false);
        assert!((f.children[0].width - w).abs() < 0.5);
    }

    #[test]
    fn floats_in_a_child_still_push_later_content_aside() {
        let f = layout::body(
            "<div><div class=f style='float:left;width:60px'></div></div><p>wrapped</p>",
            "",
            400.0,
        );
        let x = f.children.iter().find_map(layout::first_text_x).unwrap();
        assert_eq!(x, 60.0);
    }
}
