// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! HTML's presentational attributes (`align`, `valign`, `bgcolor`, `cellpadding`,
//! `cellspacing`, `nowrap`, a cell's `width`, `<font color>`...) as the CSS they stand for
//! (HTML Living Standard, "Rendering", "Non-CSS presentational hints").
//!
//! They take part in the cascade as author declarations of specificity zero that come
//! before every author rule, so any stylesheet rule overrides them and they override the
//! user-agent sheet. Pages still served as plain HTML 3.2 (Google's basic home page is
//! one) depend on them for all of their layout.

use crate::parser::{parse_declarations, Declaration};
use blueice_dom::{Document, NodeData, NodeId};

/// The declarations `node`'s presentational attributes stand for, in source order.
pub(crate) fn hints(doc: &Document, node: NodeId) -> Vec<Declaration> {
    let NodeData::Element {
        tag_name,
        attributes,
        ..
    } = doc.data(node)
    else {
        return Vec::new();
    };
    // A cell's padding comes from its table's attribute, so a bare cell is not skipped.
    if attributes.is_empty() && !matches!(tag_name.as_str(), "td" | "th") {
        return Vec::new();
    }
    let attr = |name: &str| {
        attributes
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.trim())
    };
    let tag = tag_name.as_str();
    let cell = matches!(tag, "td" | "th");
    let table_part = matches!(tag, "table" | "tr" | "td" | "th" | "thead" | "tbody" | "tfoot");
    let mut css = String::new();

    if let Some(align) = attr("align") {
        let align = align.to_ascii_lowercase();
        match tag {
            "table" => match align.as_str() {
                "center" | "middle" => css.push_str("margin-left:auto;margin-right:auto;"),
                "left" | "right" => css.push_str(&format!("float:{align};")),
                _ => {}
            },
            "img" | "object" | "embed" => {
                if matches!(align.as_str(), "left" | "right") {
                    css.push_str(&format!("float:{align};"));
                }
            }
            "div" | "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "caption" | "tr" | "td"
            | "th" | "thead" | "tbody" | "tfoot" | "col" | "colgroup" => {
                match align.as_str() {
                    "center" | "middle" => css.push_str("text-align:center;"),
                    "left" | "right" | "justify" => css.push_str(&format!("text-align:{align};")),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    if matches!(tag, "tr" | "td" | "th" | "thead" | "tbody" | "tfoot") {
        if let Some(v) = attr("valign") {
            let v = v.to_ascii_lowercase();
            if matches!(v.as_str(), "top" | "middle" | "bottom" | "baseline") {
                css.push_str(&format!("vertical-align:{v};"));
            }
        }
    }
    if cell && attr("nowrap").is_some() {
        css.push_str("white-space:nowrap;");
    }
    if matches!(tag, "body" | "table" | "tr" | "td" | "th" | "thead" | "tbody" | "tfoot") {
        if let Some(color) = attr("bgcolor").and_then(legacy_color) {
            css.push_str(&format!("background-color:{color};"));
        }
    }
    if table_part && matches!(tag, "table" | "td" | "th") {
        for property in ["width", "height"] {
            if let Some(length) = attr(property).and_then(dimension) {
                css.push_str(&format!("{property}:{length};"));
            }
        }
    }
    if tag == "table" {
        if let Some(n) = attr("cellspacing").and_then(non_negative_integer) {
            css.push_str(&format!("border-spacing:{n}px;"));
        }
    }
    if cell {
        if let Some(n) = owning_table_attribute(doc, node, "cellpadding")
            .as_deref()
            .and_then(non_negative_integer)
        {
            css.push_str(&format!("padding:{n}px;"));
        }
    }
    if tag == "font" {
        if let Some(color) = attr("color").and_then(legacy_color) {
            css.push_str(&format!("color:{color};"));
        }
        if let Some(face) = attr("face").filter(|f| !f.is_empty() && !f.contains([';', '{', '}'])) {
            css.push_str(&format!("font-family:{face};"));
        }
        if let Some(size) = attr("size").and_then(font_size) {
            css.push_str(&format!("font-size:{size};"));
        }
    }

    if css.is_empty() {
        return Vec::new();
    }
    parse_declarations(&crate::tokenizer::tokenize(&css))
}

/// The value of `name` on the nearest `<table>` above `node`.
fn owning_table_attribute(doc: &Document, node: NodeId, name: &str) -> Option<String> {
    let mut at = doc.parent(node);
    while let Some(ancestor) = at {
        if let NodeData::Element {
            tag_name,
            attributes,
            ..
        } = doc.data(ancestor)
        {
            if tag_name == "table" {
                return attributes
                    .iter()
                    .find(|(k, _)| k == name)
                    .map(|(_, v)| v.trim().to_string());
            }
        }
        at = doc.parent(ancestor);
    }
    None
}

fn non_negative_integer(text: &str) -> Option<u32> {
    let digits: String = text.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// A `width`/`height` attribute: digits are pixels, digits and `%` a percentage.
fn dimension(text: &str) -> Option<String> {
    let digits: String = text
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    if digits.is_empty() || digits.parse::<f64>().is_err() {
        return None;
    }
    let rest = text[digits.len()..].trim();
    match rest {
        "%" => Some(format!("{digits}%")),
        "" | "px" => Some(format!("{digits}px")),
        _ => None,
    }
}

/// The legacy colour forms: a CSS colour as is, or six (three) hex digits without the `#`.
fn legacy_color(text: &str) -> Option<String> {
    if text.is_empty() || text.contains([';', '{', '}']) {
        return None;
    }
    let bare_hex = matches!(text.len(), 3 | 6) && text.chars().all(|c| c.is_ascii_hexdigit());
    Some(if bare_hex {
        format!("#{text}")
    } else {
        text.to_string()
    })
}

/// `<font size>`: 1 to 7 as the font-size keywords.
fn font_size(text: &str) -> Option<&'static str> {
    Some(match text.trim() {
        "1" => "x-small",
        "2" => "small",
        "3" => "medium",
        "4" => "large",
        "5" => "x-large",
        "6" => "xx-large",
        "7" | "8" | "9" => "xxx-large",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use crate::cascade::{cascade_with_viewport, ua_stylesheet, ComputedStyle, Origin};
    use crate::parser::parse;
    use crate::value::{Length, Value};
    use blueice_dom::NodeData;

    fn style_of(html: &str, css: &str, tag: &str) -> ComputedStyle {
        let doc = blueice_html::parse(html);
        let ua = ua_stylesheet();
        let author = parse(css);
        let styles = cascade_with_viewport(
            &doc,
            &[(Origin::Ua, &ua), (Origin::Author, &author)],
            1000.0,
            500.0,
        );
        let node = styles
            .keys()
            .copied()
            .filter(
                |n| matches!(doc.data(*n), NodeData::Element { tag_name, .. } if tag_name == tag),
            )
            .min_by_key(|n| n.as_u64())
            .unwrap_or_else(|| panic!("no <{tag}> in {html}"));
        styles[&node].clone()
    }

    fn px(n: f64) -> Value {
        Value::Length(Length::Px(n))
    }

    fn table_cell(attrs: &str) -> ComputedStyle {
        style_of(&format!("<table><tr><td {attrs}>x</td></tr></table>"), "", "td")
    }

    #[test]
    fn align_on_a_cell_or_block_is_text_align() {
        assert_eq!(table_cell("align=center").text_align.as_deref(), Some("center"));
        assert_eq!(table_cell("align=right").text_align.as_deref(), Some("right"));
        assert_eq!(table_cell("align=middle").text_align.as_deref(), Some("center"));
        assert_eq!(
            style_of("<p align=right>x</p>", "", "p").text_align.as_deref(),
            Some("right")
        );
        assert_eq!(
            style_of("<div align=CENTER>x</div>", "", "div").text_align.as_deref(),
            Some("center")
        );
        // An unknown word stands for nothing.
        assert_ne!(table_cell("align=sideways").text_align.as_deref(), Some("sideways"));
    }

    #[test]
    fn a_centered_element_centers_its_text() {
        // `-webkit-center`: the text centers, and so do the tables and blocks inside it.
        let style = style_of("<center><p>x</p></center>", "", "p");
        assert_eq!(style.text_align.as_deref(), Some("-webkit-center"));
    }

    #[test]
    fn valign_nowrap_and_bgcolor_on_a_cell() {
        let cell = table_cell("valign=top nowrap bgcolor=#ff0000");
        assert_eq!(cell.other["vertical-align"], Value::Keyword("top".into()));
        assert_eq!(cell.other["white-space"], Value::Keyword("nowrap".into()));
        assert!(cell.other.contains_key("background-color"));
        // The legacy form without the `#`.
        let bare = table_cell("bgcolor=00ff00");
        assert_eq!(bare.other["background-color"], table_cell("bgcolor=#00ff00").other["background-color"]);
    }

    #[test]
    fn a_cells_width_and_height_attributes_are_lengths() {
        assert_eq!(table_cell("width=120").other["width"], px(120.0));
        assert_eq!(table_cell("width=25%").other["width"], Value::Percentage(25.0));
        assert_eq!(table_cell("height=40").other["height"], px(40.0));
        assert!(!table_cell("width=auto").other.contains_key("width"));
    }

    #[test]
    fn cellpadding_and_cellspacing_belong_to_the_table_but_pad_its_cells() {
        let html = "<table cellpadding=7 cellspacing=3><tr><td>x</td></tr></table>";
        let table = style_of(html, "", "table");
        assert_eq!(table.other["border-spacing"], px(3.0));
        let cell = style_of(html, "", "td");
        for side in ["top", "right", "bottom", "left"] {
            assert_eq!(cell.other[&format!("padding-{side}")], px(7.0), "padding-{side}");
        }
        // A cell in a nested table answers to its own table, not the outer one.
        let nested = style_of(
            "<table cellpadding=9><tr><td><table><tr><td id=n>x</td></tr></table></td></tr></table>",
            "",
            "td",
        );
        assert_eq!(nested.other["padding-left"], px(9.0));
    }

    #[test]
    fn a_stylesheet_rule_beats_a_presentational_attribute() {
        let cell = style_of(
            "<table><tr><td align=center width=100>x</td></tr></table>",
            "td { text-align: right; width: 50px }",
            "td",
        );
        assert_eq!(cell.text_align.as_deref(), Some("right"));
        assert_eq!(cell.other["width"], px(50.0));
    }

    #[test]
    fn a_table_aligned_center_gets_auto_side_margins_and_left_or_right_floats() {
        let centered = style_of("<table align=center><tr><td>x</td></tr></table>", "", "table");
        assert_eq!(centered.other["margin-left"], Value::Keyword("auto".into()));
        assert_eq!(centered.other["margin-right"], Value::Keyword("auto".into()));
        let floated = style_of("<table align=right><tr><td>x</td></tr></table>", "", "table");
        assert_eq!(floated.other["float"], Value::Keyword("right".into()));
    }

    #[test]
    fn font_color_face_and_size() {
        let font = style_of("<font color=#336699 face=Arial size=5>x</font>", "", "font");
        assert_eq!(font.color, crate::value::Color::Rgba(0x33, 0x66, 0x99, 255));
        assert!(font.font_family.as_deref().is_some_and(|f| f.contains("arial")));
        assert!(font.font_size_px > 16.0);
    }

    #[test]
    fn attributes_on_other_elements_stand_for_nothing() {
        // `align` means nothing on a span; `bgcolor` is not a span attribute either.
        let span = style_of("<span align=center bgcolor=red>x</span>", "", "span");
        assert_ne!(span.text_align.as_deref(), Some("center"));
        assert!(!span.other.contains_key("background-color"));
    }
}
