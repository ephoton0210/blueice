// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Paint: a [`Fragment`] tree + computed styles -> a flat, ordered list
//! of typed [`PaintCommand`]s -- a display list, not pixels.
//!
//! See `development/browser_core/research/paint.md`: both Gecko
//! (`nsDisplayList`) and Blink (`PaintController`/`DisplayItem`)
//! interpose exactly this kind of intermediate representation between
//! layout and actual rasterization, specifically so the same list shape
//! can be replayed by different backends later -- for BlueIce, that's
//! the seam a per-platform Phase 4 `frontend` will eventually consume,
//! without `blueice-paint` itself knowing anything about Direct2D/
//! CoreGraphics/Skia. Per `phase-2-mvp-scope/PLAN.md`'s CSS scope, only
//! three command kinds exist: filled rectangles (`background-color`),
//! border edges (solid, single-color, single-width per side), and text
//! runs (position + content + color + font size, no glyph shaping).
//! Explicitly out of scope, same as the reference engines' own later
//! stages: clipping, transforms, layer/group compositing, paint
//! invalidation/caching (every call repaints the whole tree), box-
//! shadow, background-image, gradients. `opacity` (added for
//! `phase-1-ai-representation-layer/PLAN.md`'s `AiNode::opacity`
//! field) is a narrow exception: applied as a flat per-element alpha
//! multiply on that element's own background/border/text colors
//! ([`apply_opacity`]), not real group compositing -- a `div` with
//! `opacity: 0.5` containing two overlapping children still shows each
//! child's edges through the other, unlike a real browser's isolated
//! compositing layer for that box.

use blueice_css::{ComputedStyle, Length, Value};
use blueice_dom::NodeId;
use blueice_layout::{Fragment, FragmentKind};
use std::collections::HashMap;

// Re-exported: `PaintCommand`'s own public fields are typed with
// `Color`, so a crate depending only on `blueice-paint` (as
// `blueice-raster` deliberately does, to keep its own dependency
// surface minimal) must still be able to name it -- otherwise the
// public interface would be incomplete per `TEST_PLAN.md`'s Definition
// of Done, forcing every downstream consumer to also add `blueice-css`
// as a direct dependency just to spell this crate's own return types.
pub use blueice_css::Color;

type StyleMap = HashMap<NodeId, ComputedStyle>;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PaintCommand {
    /// A solid-filled rectangle (a box's `background-color`).
    Rect { rect: Rect, color: Color },
    /// One border edge, drawn as a filled strip along that side of the
    /// box -- e.g. the top edge is `height: border-top-width` tall and
    /// spans the box's full `width`. No radii, no dashed/dotted pattern
    /// rendering (a `dashed`/`dotted` style still paints as solid for
    /// MVP; only `none`/`hidden` are distinguished, by not painting the
    /// edge at all).
    BorderEdge { rect: Rect, color: Color },
    /// A run of text with no internal structure -- one command per
    /// `blueice_layout` text fragment, at that fragment's own position.
    /// `bold`/`italic` are booleans, not the full CSS value space
    /// (`font-weight: 600`, `font-style: oblique 10deg`, ...) -- all
    /// that a from-scratch rasterizer picking between a handful of
    /// bundled font files can actually act on for MVP.
    Text {
        x: f64,
        y: f64,
        text: String,
        color: Color,
        font_size_px: f64,
        bold: bool,
        italic: bool,
    },
}

/// The paint output for one frame: the root box's own size, plus every
/// command needed to draw it, in back-to-front paint order (a
/// fragment's own background/border are always emitted before its
/// children's, matching CSS's "background and borders painted first"
/// stacking rule for the normal, non-positioned case this MVP covers).
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub width: f64,
    pub height: f64,
    pub commands: Vec<PaintCommand>,
}

fn fmt_num(n: f64) -> String {
    format!("{n:.1}")
}

fn fmt_rect(r: Rect) -> String {
    format!(
        "{},{} {}x{}",
        fmt_num(r.x),
        fmt_num(r.y),
        fmt_num(r.width),
        fmt_num(r.height)
    )
}

fn fmt_color(c: Color) -> String {
    match c {
        Color::Rgba(r, g, b, 255) => format!("#{r:02x}{g:02x}{b:02x}"),
        Color::Rgba(r, g, b, a) => format!("rgba({r},{g},{b},{a})"),
        Color::CurrentColor => "currentcolor".to_string(),
    }
}

/// A canonical, one-line-per-command textual dump of `frame`, in the
/// same spirit as `blueice_testing::dump_dom` and the `#styles`/
/// `#layout` dumps `blueice-css`/`blueice-layout` build for their own
/// fixture tests -- exposed as part of this crate's public API (not
/// just test code) so `blueice-engine`'s end-to-end smoke test can
/// reuse it directly against `render()`'s output, rather than
/// duplicating the format.
pub fn dump_frame(frame: &Frame) -> String {
    let mut out = String::new();
    for command in &frame.commands {
        match command {
            PaintCommand::Rect { rect, color } => {
                out.push_str(&format!("rect {} {}\n", fmt_rect(*rect), fmt_color(*color)))
            }
            PaintCommand::BorderEdge { rect, color } => out.push_str(&format!(
                "border {} {}\n",
                fmt_rect(*rect),
                fmt_color(*color)
            )),
            PaintCommand::Text {
                x,
                y,
                text,
                color,
                font_size_px,
                bold,
                italic,
            } => out.push_str(&format!(
                "text {},{} \"{text}\" {} {} {} {}\n",
                fmt_num(*x),
                fmt_num(*y),
                fmt_color(*color),
                fmt_num(*font_size_px),
                if *bold { "bold" } else { "normal" },
                if *italic { "italic" } else { "normal" }
            )),
        }
    }
    out
}

const BORDER_SIDES: [&str; 4] = ["top", "right", "bottom", "left"];

fn length_px(value: &Value) -> Option<f64> {
    match value {
        Value::Length(Length::Px(n)) => Some(*n),
        Value::Length(Length::Em(_)) => None, // already resolved to px by the time it reaches paint in practice; not a length paint can resolve itself (no font-size context here)
        Value::Length(Length::Zero) => Some(0.0),
        _ => None,
    }
}

fn as_color(value: &Value, current_color: Color) -> Option<Color> {
    match value {
        Value::Color(Color::CurrentColor) => Some(current_color),
        Value::Color(c) => Some(*c),
        _ => None,
    }
}

/// One border side's paint rectangle, or `None` if that side shouldn't
/// be drawn at all (`border-style` unset, `none`, or `hidden` -- CSS's
/// initial `border-style` is `none`, so a `border-*-width` alone,
/// without an explicit non-`none` style, paints nothing, matching real
/// browsers).
fn border_side_rect(
    side: &str,
    box_rect: Rect,
    other: &HashMap<String, Value>,
) -> Option<(Rect, f64)> {
    let style_ok = matches!(other.get(&format!("border-{side}-style")), Some(Value::Keyword(k)) if k != "none" && k != "hidden");
    if !style_ok {
        return None;
    }
    let width = other
        .get(&format!("border-{side}-width"))
        .and_then(length_px)?;
    if width <= 0.0 {
        return None;
    }
    let rect = match side {
        "top" => Rect {
            x: box_rect.x,
            y: box_rect.y,
            width: box_rect.width,
            height: width,
        },
        "bottom" => Rect {
            x: box_rect.x,
            y: box_rect.y + box_rect.height - width,
            width: box_rect.width,
            height: width,
        },
        "left" => Rect {
            x: box_rect.x,
            y: box_rect.y,
            width,
            height: box_rect.height,
        },
        "right" => Rect {
            x: box_rect.x + box_rect.width - width,
            y: box_rect.y,
            width,
            height: box_rect.height,
        },
        _ => unreachable!("BORDER_SIDES only ever names these four"),
    };
    Some((rect, width))
}

/// Multiplies `color`'s alpha channel by `opacity` (already clamped to
/// `[0.0, 1.0]` by `ComputedStyle::opacity`) -- `CurrentColor` is
/// passed through unchanged since it's always resolved to a concrete
/// `Rgba` by the cascade before paint ever sees it (see
/// `blueice-css`'s cascade docs); this arm only exists so the match
/// stays exhaustive against `Color` without paint depending on that
/// resolution having already happened.
fn apply_opacity(color: Color, opacity: f32) -> Color {
    match color {
        Color::Rgba(r, g, b, a) => Color::Rgba(r, g, b, ((a as f32) * opacity).round() as u8),
        Color::CurrentColor => Color::CurrentColor,
    }
}

fn paint_fragment(
    fragment: &Fragment,
    offset_x: f64,
    offset_y: f64,
    styles: &StyleMap,
    out: &mut Vec<PaintCommand>,
) {
    let x = offset_x + fragment.x;
    let y = offset_y + fragment.y;

    match &fragment.kind {
        FragmentKind::Block => {
            if let Some(style) = fragment.node.and_then(|n| styles.get(&n)) {
                let opacity = style.opacity();
                let rect = Rect {
                    x,
                    y,
                    width: fragment.width,
                    height: fragment.height,
                };
                if let Some(bg) = style
                    .other
                    .get("background-color")
                    .and_then(|v| as_color(v, style.color))
                {
                    out.push(PaintCommand::Rect {
                        rect,
                        color: apply_opacity(bg, opacity),
                    });
                }
                for side in BORDER_SIDES {
                    if let Some((edge_rect, _)) = border_side_rect(side, rect, &style.other) {
                        let color = style
                            .other
                            .get(&format!("border-{side}-color"))
                            .and_then(|v| as_color(v, style.color))
                            .unwrap_or(style.color);
                        out.push(PaintCommand::BorderEdge {
                            rect: edge_rect,
                            color: apply_opacity(color, opacity),
                        });
                    }
                }
            }
        }
        FragmentKind::Line => {}
        FragmentKind::Text(text) => {
            if let Some(style) = fragment.node.and_then(|n| styles.get(&n)) {
                let bold = style.is_bold();
                let italic = style.is_italic();
                let color = apply_opacity(style.color, style.opacity());
                out.push(PaintCommand::Text {
                    x,
                    y,
                    text: text.clone(),
                    color,
                    font_size_px: style.font_size_px,
                    bold,
                    italic,
                });
            }
        }
    }

    for child in &fragment.children {
        paint_fragment(child, x, y, styles, out);
    }
}

/// Builds the paint command list for `fragment` (as produced by
/// `blueice_layout::layout`), looking up each fragment's colors/fonts
/// from `styles` (the same computed-style map layout was built from).
pub fn paint(fragment: &Fragment, styles: &HashMap<NodeId, ComputedStyle>) -> Frame {
    let mut commands = Vec::new();
    paint_fragment(fragment, 0.0, 0.0, styles, &mut commands);
    Frame {
        width: fragment.width,
        height: fragment.height,
        commands,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_css::{cascade, ua_stylesheet, Origin};
    use blueice_layout::Constraints;

    fn paint_html(html: &str, css: &str, width: f64) -> Frame {
        let doc = blueice_html::parse(html);
        let ua = ua_stylesheet();
        let author = blueice_css::parse(css).rules;
        let sheets: Vec<(Origin, &[blueice_css::Rule])> = if author.is_empty() {
            vec![(Origin::Ua, &ua)]
        } else {
            vec![(Origin::Ua, &ua), (Origin::Author, &author)]
        };
        let styles = cascade(&doc, &sheets);
        let fragment = blueice_layout::layout(
            &doc,
            doc.root(),
            &styles,
            Constraints {
                available_width: width,
            },
        );
        paint(&fragment, &styles)
    }

    #[test]
    fn frame_size_matches_the_root_fragment() {
        let f = paint_html("<p>hi</p>", "", 320.0);
        assert_eq!(f.width, 320.0);
        assert!(f.height > 0.0);
    }

    #[test]
    fn background_color_produces_a_rect_command() {
        let f = paint_html("<div></div>", "div { background-color: red; }", 320.0);
        let rect = f
            .commands
            .iter()
            .find(|c| matches!(c, PaintCommand::Rect { .. }))
            .expect("a Rect command");
        assert_eq!(
            *rect,
            PaintCommand::Rect {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 320.0,
                    height: 0.0
                },
                color: Color::Rgba(255, 0, 0, 255)
            }
        );
    }

    #[test]
    fn opacity_multiplies_the_backgrounds_alpha_channel() {
        let f = paint_html(
            "<div></div>",
            "div { background-color: red; opacity: 0.5; }",
            320.0,
        );
        let PaintCommand::Rect { color, .. } = f
            .commands
            .iter()
            .find(|c| matches!(c, PaintCommand::Rect { .. }))
            .unwrap()
        else {
            unreachable!()
        };
        assert_eq!(*color, Color::Rgba(255, 0, 0, 128));
    }

    #[test]
    fn opacity_multiplies_text_and_border_alpha_too() {
        let f = paint_html(
            "<div style=\"border: 1px solid black; opacity: 0.5;\">x</div>",
            "",
            320.0,
        );
        let PaintCommand::BorderEdge { color, .. } = f
            .commands
            .iter()
            .find(|c| matches!(c, PaintCommand::BorderEdge { .. }))
            .unwrap()
        else {
            unreachable!()
        };
        assert_eq!(*color, Color::Rgba(0, 0, 0, 128));
        let PaintCommand::Text { color, .. } = f
            .commands
            .iter()
            .find(|c| matches!(c, PaintCommand::Text { .. }))
            .unwrap()
        else {
            unreachable!()
        };
        assert_eq!(*color, Color::Rgba(0, 0, 0, 128));
    }

    #[test]
    fn default_opacity_leaves_alpha_unchanged() {
        let f = paint_html("<div></div>", "div { background-color: red; }", 320.0);
        let PaintCommand::Rect { color, .. } = f
            .commands
            .iter()
            .find(|c| matches!(c, PaintCommand::Rect { .. }))
            .unwrap()
        else {
            unreachable!()
        };
        assert_eq!(*color, Color::Rgba(255, 0, 0, 255));
    }

    #[test]
    fn no_background_color_produces_no_rect_command() {
        let f = paint_html("<div>x</div>", "", 320.0);
        assert!(!f
            .commands
            .iter()
            .any(|c| matches!(c, PaintCommand::Rect { .. })));
    }

    #[test]
    fn text_produces_a_text_command_with_color_and_font_size() {
        let f = paint_html("<p>hi</p>", "p { color: blue; font-size: 20px; }", 320.0);
        let text = f
            .commands
            .iter()
            .find(|c| matches!(c, PaintCommand::Text { .. }))
            .expect("a Text command");
        match text {
            PaintCommand::Text {
                text,
                color,
                font_size_px,
                ..
            } => {
                assert_eq!(text, "hi");
                assert_eq!(*color, Color::Rgba(0, 0, 255, 255));
                assert_eq!(*font_size_px, 20.0);
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn bold_and_italic_elements_produce_text_commands_flagged_accordingly() {
        let f = paint_html("<p>a <b>b</b> <i>c</i> <em>d</em></p>", "", 320.0);
        let flags_for = |word: &str| -> (bool, bool) {
            f.commands
                .iter()
                .find_map(|c| {
                    if let PaintCommand::Text {
                        text, bold, italic, ..
                    } = c
                    {
                        (text == word).then_some((*bold, *italic))
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| panic!("no Text command for {word:?}"))
        };
        assert_eq!(flags_for("a"), (false, false));
        assert_eq!(flags_for("b"), (true, false), "<b> is bold, not italic");
        assert_eq!(flags_for("c"), (false, true), "<i> is italic, not bold");
        assert_eq!(
            flags_for("d"),
            (false, true),
            "<em> (UA stylesheet: font-style: italic) is italic too"
        );
    }

    #[test]
    fn numeric_font_weight_of_600_or_above_counts_as_bold() {
        let f = paint_html("<p>x</p>", "p { font-weight: 700; }", 320.0);
        assert!(matches!(
            f.commands
                .iter()
                .find(|c| matches!(c, PaintCommand::Text { .. })),
            Some(PaintCommand::Text { bold: true, .. })
        ));
    }

    #[test]
    fn border_with_explicit_style_and_color_produces_four_edges() {
        let f = paint_html("<div>x</div>", "div { border: 2px solid green; }", 320.0);
        let edges: Vec<_> = f
            .commands
            .iter()
            .filter(|c| matches!(c, PaintCommand::BorderEdge { .. }))
            .collect();
        assert_eq!(edges.len(), 4);
        for edge in &edges {
            if let PaintCommand::BorderEdge { color, .. } = edge {
                assert_eq!(*color, Color::Rgba(0, 128, 0, 255));
            }
        }
    }

    #[test]
    fn border_width_without_a_style_paints_nothing() {
        // CSS's initial border-style is `none` -- a bare border-width
        // with no explicit style must not draw a border.
        let f = paint_html("<div>x</div>", "div { border-top-width: 5px; }", 320.0);
        assert!(!f
            .commands
            .iter()
            .any(|c| matches!(c, PaintCommand::BorderEdge { .. })));
    }

    #[test]
    fn border_style_none_paints_nothing_even_with_a_width() {
        let f = paint_html(
            "<div>x</div>",
            "div { border-width: 5px; border-style: none; }",
            320.0,
        );
        assert!(!f
            .commands
            .iter()
            .any(|c| matches!(c, PaintCommand::BorderEdge { .. })));
    }

    #[test]
    fn border_color_defaults_to_currentcolor_when_unset() {
        let f = paint_html(
            "<div>x</div>",
            "div { color: purple; border-style: solid; border-width: 1px; }",
            320.0,
        );
        let edge = f
            .commands
            .iter()
            .find(|c| matches!(c, PaintCommand::BorderEdge { .. }))
            .unwrap();
        if let PaintCommand::BorderEdge { color, .. } = edge {
            assert_eq!(*color, Color::Rgba(128, 0, 128, 255));
        }
    }

    #[test]
    fn border_edges_are_positioned_along_the_correct_sides() {
        let f = paint_html(
            "<div>x</div>",
            "div { width: 100px; height: 50px; border: 4px solid black; }",
            320.0,
        );
        let edges: HashMap<_, _> = f
            .commands
            .iter()
            .filter_map(|c| {
                if let PaintCommand::BorderEdge { rect, .. } = c {
                    Some(*rect)
                } else {
                    None
                }
            })
            .map(|r| ((r.x as i64, r.y as i64, r.width as i64, r.height as i64), r))
            .collect();
        // top: full width, 4px tall, at the box's own origin
        assert!(
            edges
                .values()
                .any(|r| r.height == 4.0 && r.width == 108.0 && r.y == 0.0),
            "top edge"
        );
        // bottom: full width, 4px tall, flush with the box's bottom
        assert!(
            edges
                .values()
                .any(|r| r.height == 4.0 && r.width == 108.0 && (r.y - 54.0).abs() < 0.01),
            "bottom edge"
        );
        // left/right: full height, 4px wide
        assert!(
            edges
                .values()
                .any(|r| r.width == 4.0 && r.height == 58.0 && r.x == 0.0),
            "left edge"
        );
        assert!(
            edges
                .values()
                .any(|r| r.width == 4.0 && r.height == 58.0 && (r.x - 104.0).abs() < 0.01),
            "right edge"
        );
    }

    #[test]
    fn nested_boxes_paint_parent_background_before_child_content() {
        let f = paint_html("<div>x</div>", "div { background-color: red; }", 320.0);
        let rect_pos = f
            .commands
            .iter()
            .position(|c| matches!(c, PaintCommand::Rect { .. }))
            .unwrap();
        let text_pos = f
            .commands
            .iter()
            .position(|c| matches!(c, PaintCommand::Text { .. }))
            .unwrap();
        assert!(
            rect_pos < text_pos,
            "background must be emitted before the text painted on top of it"
        );
    }

    // ---- interaction test: added by a dedicated post-implementation
    // test-review pass (per TEST_PLAN.md's Definition of Done) -- the
    // single-box tests above don't exercise paint order across nested
    // boxes that each have their own background, or confirm that a
    // Line fragment (a pure layout grouping) never emits a command of
    // its own.

    #[test]
    fn nested_boxes_each_paint_their_own_background_in_outer_to_inner_order() {
        let f = paint_html(
            "<div><p>x</p></div>",
            "div { background-color: red; } p { background-color: blue; }",
            320.0,
        );
        // exactly: outer rect, inner rect, text -- nothing else (in
        // particular, the Line fragment wrapping "x" contributes no
        // command of its own).
        assert_eq!(f.commands.len(), 3);
        assert_eq!(
            f.commands[0],
            PaintCommand::Rect {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 320.0,
                    height: 19.2
                },
                color: Color::Rgba(255, 0, 0, 255)
            }
        );
        assert_eq!(
            f.commands[1],
            PaintCommand::Rect {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 320.0,
                    height: 19.2
                },
                color: Color::Rgba(0, 0, 255, 255)
            }
        );
        assert!(
            matches!(f.commands[2], PaintCommand::Text { .. }),
            "outer bg, then inner bg, then the text on top of both"
        );
    }

    #[test]
    fn fragment_with_no_computed_style_paints_nothing_for_itself() {
        let fragment = Fragment {
            node: None,
            kind: FragmentKind::Block,
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
            children: vec![],
        };
        let styles = StyleMap::new();
        let frame = paint(&fragment, &styles);
        assert!(frame.commands.is_empty());
    }

    #[test]
    fn dump_frame_formats_each_command_kind_on_its_own_line() {
        let frame = Frame {
            width: 100.0,
            height: 20.0,
            commands: vec![
                PaintCommand::Rect {
                    rect: Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 100.0,
                        height: 20.0,
                    },
                    color: Color::Rgba(255, 0, 0, 255),
                },
                PaintCommand::BorderEdge {
                    rect: Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 100.0,
                        height: 4.0,
                    },
                    color: Color::Rgba(0, 0, 0, 255),
                },
                PaintCommand::Text {
                    x: 1.0,
                    y: 2.0,
                    text: "hi".to_string(),
                    color: Color::Rgba(0, 0, 255, 255),
                    font_size_px: 16.0,
                    bold: false,
                    italic: false,
                },
                PaintCommand::Text {
                    x: 1.0,
                    y: 20.0,
                    text: "yo".to_string(),
                    color: Color::Rgba(0, 0, 255, 255),
                    font_size_px: 16.0,
                    bold: true,
                    italic: true,
                },
            ],
        };
        assert_eq!(
            dump_frame(&frame),
            "rect 0.0,0.0 100.0x20.0 #ff0000\nborder 0.0,0.0 100.0x4.0 #000000\ntext 1.0,2.0 \"hi\" #0000ff 16.0 normal normal\ntext 1.0,20.0 \"yo\" #0000ff 16.0 bold italic\n"
        );
    }

    #[test]
    fn dump_frame_of_an_empty_frame_is_empty_string() {
        let frame = Frame {
            width: 0.0,
            height: 0.0,
            commands: vec![],
        };
        assert_eq!(dump_frame(&frame), "");
    }
}
