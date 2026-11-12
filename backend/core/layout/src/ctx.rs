// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! What every layout algorithm reads: the document, the computed styles, the
//! viewport, and typed views of a node's style (`display`, `position`,
//! lengths resolved against the right font size and percentage base).

use blueice_css::{ComputedStyle, LengthContext, Value};
use blueice_dom::{Document, NodeData, NodeId};
use std::collections::HashMap;

pub type StyleMap = HashMap<NodeId, ComputedStyle>;

pub(crate) struct Ctx<'a> {
    pub doc: &'a Document,
    pub styles: &'a StyleMap,
    pub viewport_width: f64,
    pub viewport_height: f64,
    pub root_font_size: f64,
    /// Memoised `(min-content, max-content)` widths, by node, for one pass.
    pub intrinsic: std::cell::RefCell<HashMap<NodeId, (f64, f64)>>,
    /// Memoised content-only widths (ignoring the node's own `width`).
    pub content_widths: std::cell::RefCell<HashMap<NodeId, (f64, f64)>>,
    /// Block-level layouts already computed in this pass, by node and
    /// constraint: layout is a pure function of both, and flex measuring
    /// would otherwise redo nested subtrees at every level.
    pub layouts: std::cell::RefCell<HashMap<crate::block::LayoutKey, crate::block::Placed>>,
    /// Floats a block being laid out must flow around, in the coordinates of
    /// its border box; empty inside any box that starts a new formatting
    /// context. A layout that sees any is not memoised.
    pub floats: std::cell::RefCell<Vec<crate::float::FloatRect>>,
    /// Generated `::before` / `::after` boxes and their text: ids no DOM node
    /// has, mapped to the data a node of that kind would carry.
    pub pseudo: HashMap<NodeId, NodeData>,
    /// The writing mode the content being laid out is flowing in. Inside a vertical box the
    /// layout runs in a turned frame (inline axis across the page's height), and style reads
    /// name the sides that frame sees: see [`Writing`].
    pub writing: std::cell::Cell<Writing>,
    /// How wide, across the page, a horizontal box nested in a vertical one may be when it sets
    /// no width: the content width of the vertical box around it.
    pub orth_room: std::cell::Cell<f64>,
}

/// `text-combine-upright`: which runs of characters in a vertical line are set side by side in
/// the room of one upright character.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Combine {
    /// All the characters of the box.
    All,
    /// Runs of up to this many ASCII digits.
    Digits(u32),
}

/// A writing mode as layout flows it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum Writing {
    #[default]
    Horizontal,
    /// Lines run down the page and stack from the right.
    VerticalRl,
    /// Lines run down the page and stack from the left.
    VerticalLr,
    /// Lines run up the page, their letters turned a quarter counter-clockwise, and stack from
    /// the left.
    SidewaysLr,
}

impl Writing {
    pub fn is_vertical(self) -> bool {
        self != Writing::Horizontal
    }

    /// The property a vertical frame reads when layout asks for `property`. Layout works as if
    /// writing left to right, top to bottom: its `left` and `right` are the page's top and
    /// bottom, its `top` the page's right (left in `vertical-lr`) and its `bottom` the opposite
    /// side; its `width` is the page's `height`.
    fn physical(self, property: &str) -> Option<String> {
        if !self.is_vertical() {
            return None;
        }
        let rl = self == Writing::VerticalRl;
        let up = self == Writing::SidewaysLr;
        let side = |s: &str| -> &'static str {
            match s {
            "left" => if up { "bottom" } else { "top" },
            "right" => if up { "top" } else { "bottom" },
            "top" => {
                if rl {
                    "right"
                } else {
                    "left"
                }
            }
            "bottom" => {
                if rl {
                    "left"
                } else {
                    "right"
                }
            }
            // Only the four sides reach here.
            _ => "top",
            }
        };
        Some(match property {
            "width" => "height".to_string(),
            "height" => "width".to_string(),
            "min-width" => "min-height".to_string(),
            "min-height" => "min-width".to_string(),
            "max-width" => "max-height".to_string(),
            "max-height" => "max-width".to_string(),
            _ => {
                let parts: Vec<&str> = property.split('-').collect();
                match parts.as_slice() {
                    [family @ ("margin" | "padding"), s @ ("left" | "right" | "top" | "bottom")] => {
                        format!("{family}-{}", side(s))
                    }
                    ["border", s @ ("left" | "right" | "top" | "bottom"), what @ ("width" | "style" | "color")] => {
                        format!("border-{}-{what}", side(s))
                    }
                    _ => return None,
                }
            }
        })
    }
}

/// The generated boxes of every element in `styles` that has a `content`.
pub(crate) fn generated_nodes(doc: &Document, styles: &StyleMap) -> HashMap<NodeId, NodeData> {
    let mut out = HashMap::new();
    for (&id, style) in styles {
        if !blueice_css::is_pseudo_node(id) {
            continue;
        }
        let owner = blueice_css::pseudo_owner(id);
        if !doc.contains(owner) {
            continue;
        }
        let Some(Value::Unparsed(tokens)) = style.other.get("content") else {
            continue;
        };
        let attr = |name: &str| match doc.data(owner) {
            NodeData::Element { attributes, .. } => attributes
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.clone()),
            _ => None,
        };
        let Some(text) = blueice_css::generated_text(tokens, &attr) else {
            continue;
        };
        let name = match blueice_css::pseudo_kind(id) {
            blueice_css::PseudoElement::Before => "::before",
            blueice_css::PseudoElement::After => "::after",
        };
        out.insert(
            id,
            NodeData::Element {
                tag_name: name.to_string(),
                attributes: Vec::new(),
            },
        );
        if !text.is_empty() {
            out.insert(
                blueice_css::pseudo_text_node_id(id),
                NodeData::Text { data: text },
            );
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Display {
    None,
    /// The element generates no box of its own; its children take its place.
    Contents,
    Block,
    Inline,
    InlineBlock,
    ListItem,
    Flex,
    InlineFlex,
    Grid,
    InlineGrid,
    Table,
    InlineTable,
    TableRowGroup,
    TableRow,
    TableCell,
    TableCaption,
    /// `table-column` and `table-column-group`: no box.
    TableColumn,
}

impl Display {
    fn parse(text: &str) -> Display {
        match text {
            "none" => Display::None,
            "contents" => Display::Contents,
            "block" | "flow-root" | "run-in" => Display::Block,
            "inline" | "ruby" => Display::Inline,
            "inline-block" => Display::InlineBlock,
            "list-item" => Display::ListItem,
            "flex" | "-webkit-box" | "-webkit-flex" | "-ms-flexbox" => Display::Flex,
            "inline-flex" | "-webkit-inline-box" | "-webkit-inline-flex" | "-ms-inline-flexbox" => {
                Display::InlineFlex
            }
            "grid" => Display::Grid,
            "inline-grid" => Display::InlineGrid,
            "table" => Display::Table,
            "inline-table" => Display::InlineTable,
            "table-row-group" | "table-header-group" | "table-footer-group" => {
                Display::TableRowGroup
            }
            "table-row" => Display::TableRow,
            "table-cell" => Display::TableCell,
            "table-caption" => Display::TableCaption,
            "table-column" | "table-column-group" => Display::TableColumn,
            _ => Display::Inline,
        }
    }

    /// Whether the box participates in a line as one atomic unit.
    pub fn is_atomic_inline(self) -> bool {
        matches!(
            self,
            Display::InlineBlock | Display::InlineFlex | Display::InlineGrid | Display::InlineTable
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Position {
    Static,
    Relative,
    Absolute,
    Fixed,
    Sticky,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Side {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Clear {
    None,
    Left,
    Right,
    Both,
}

/// A side-by-side top/right/bottom/left quantity.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Edges {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

impl Edges {
    pub fn horizontal(&self) -> f64 {
        self.left + self.right
    }
    pub fn vertical(&self) -> f64 {
        self.top + self.bottom
    }
}

/// A node's computed style with typed accessors. A node without a computed
/// style (a text node, say) reads as all-initial values.
pub(crate) struct Style<'a> {
    #[allow(dead_code)]
    pub node: NodeId,
    pub style: Option<&'a ComputedStyle>,
    ctx: &'a Ctx<'a>,
}

impl<'a> Ctx<'a> {
    pub fn style(&'a self, node: NodeId) -> Style<'a> {
        Style {
            node,
            style: self.styles.get(&node),
            ctx: self,
        }
    }

    /// The style of the box a fragment renders: its own node's, or the generated box's.
    pub fn style_of_fragment(&'a self, f: &crate::fragment::Fragment) -> Style<'a> {
        let node = f.node.or(f.style_node).unwrap_or_else(|| NodeId::from_u64(0));
        self.style(node)
    }

    /// A node's data, generated pseudo-element nodes included.
    pub fn node(&self, node: NodeId) -> &NodeData {
        if blueice_css::is_pseudo_node(node) {
            if let Some(data) = self.pseudo.get(&node) {
                return data;
            }
        }
        self.doc.data(node)
    }

    /// The children layout sees: a `::before` box first and an `::after` box
    /// last when the element generates them, and a generated box's text.
    pub fn children(&self, node: NodeId) -> Vec<NodeId> {
        if blueice_css::is_pseudo_node(node) {
            let text = blueice_css::pseudo_text_node_id(node);
            return if self.pseudo.contains_key(&text) {
                vec![text]
            } else {
                Vec::new()
            };
        }
        let mut out = Vec::new();
        let generated = |which| {
            let id = blueice_css::pseudo_node_id(node, which);
            self.pseudo.contains_key(&id).then_some(id)
        };
        out.extend(generated(blueice_css::PseudoElement::Before));
        out.extend(self.doc.children(node));
        out.extend(generated(blueice_css::PseudoElement::After));
        out
    }

    /// The parent of a node; a generated text's parent is its box.
    pub fn parent(&self, node: NodeId) -> Option<NodeId> {
        if blueice_css::is_pseudo_node(node) {
            let raw = node.as_u64();
            // Text ids carry the text bit; their box is the same id without it.
            return Some(NodeId::from_u64(raw & !(1 << 61))).filter(|p| *p != node);
        }
        self.doc.parent(node)
    }

    pub fn attr(&self, node: NodeId, name: &str) -> Option<&str> {
        match self.node(node) {
            NodeData::Element { attributes, .. } => attributes
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.as_str()),
            _ => None,
        }
    }

    pub fn tag(&self, node: NodeId) -> Option<&str> {
        match self.node(node) {
            NodeData::Element { tag_name, .. } => Some(tag_name.as_str()),
            _ => None,
        }
    }
}

impl<'a> Style<'a> {
    pub fn font_size(&self) -> f64 {
        self.style.map_or(16.0, |s| s.font_size_px)
    }

    pub fn raw(&self, property: &str) -> Option<&'a Value> {
        let style = self.style?;
        match self.ctx.writing.get().physical(property) {
            Some(physical) => style.other.get(&physical),
            None => style.other.get(property),
        }
    }

    /// This box's `text-combine-upright`, where it means anything: in a vertical line.
    pub fn combine_upright(&self) -> Option<Combine> {
        if !self.ctx.writing.get().is_vertical() {
            return None;
        }
        match self.raw("text-combine-upright")? {
            Value::Keyword(k) if k == "all" => Some(Combine::All),
            Value::Keyword(k) if k == "digits" => Some(Combine::Digits(2)),
            Value::Unparsed(tokens) => {
                let mut words = tokens.iter().filter(|t| !matches!(t, blueice_css::Token::Whitespace));
                match (words.next(), words.next()) {
                    (Some(blueice_css::Token::Ident(w)), n) if w.eq_ignore_ascii_case("digits") => {
                        let n = match n {
                            Some(blueice_css::Token::Number(n)) if (2.0..=4.0).contains(n) => *n as u32,
                            None => 2,
                            _ => return None,
                        };
                        Some(Combine::Digits(n))
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Whether `hanging-punctuation` lets a stop or comma hang past the end of a line.
    pub fn hangs_at_end(&self) -> bool {
        match self.raw("hanging-punctuation") {
            Some(Value::Keyword(k)) => matches!(k.as_str(), "allow-end" | "force-end"),
            Some(Value::Unparsed(tokens)) => tokens.iter().any(|t| {
                matches!(t, blueice_css::Token::Ident(w) if matches!(w.as_str(), "allow-end" | "force-end"))
            }),
            _ => false,
        }
    }

    /// This box's `writing-mode`, as layout flows it.
    pub fn writing_mode(&self) -> Writing {
        match self.style.and_then(|s| s.other.get("writing-mode")) {
            Some(Value::Keyword(k)) => match k.as_str() {
                "vertical-rl" | "sideways-rl" => Writing::VerticalRl,
                "vertical-lr" => Writing::VerticalLr,
                "sideways-lr" => Writing::SidewaysLr,
                _ => Writing::Horizontal,
            },
            _ => Writing::Horizontal,
        }
    }

    /// The page-supplied face this element's text is drawn with, if the engine found one for
    /// its `font-family` (set on the style by the page after the cascade; see
    /// `blueice_font::web`).
    pub fn web_face(&self) -> Option<blueice_font::web::WebFontId> {
        match self.raw("-blueice-web-font") {
            Some(Value::Number(n)) if *n >= 0.0 => Some(blueice_font::web::WebFontId::from_u32(*n as u32)),
            _ => None,
        }
    }

    /// `direction: rtl`.
    pub fn is_rtl(&self) -> bool {
        self.keyword("direction") == Some("rtl")
    }

    pub fn keyword(&self, property: &str) -> Option<&'a str> {
        match self.raw(property) {
            Some(Value::Keyword(k)) => Some(k.as_str()),
            _ => None,
        }
    }

    /// The context lengths of this element resolve in; `percent_base` is NaN
    /// where a percentage is indeterminate.
    pub fn length_context(&self, percent_base: f64) -> LengthContext {
        LengthContext {
            font_size: self.font_size(),
            root_font_size: self.ctx.root_font_size,
            viewport_width: self.ctx.viewport_width,
            viewport_height: self.ctx.viewport_height,
            percent_base,
        }
    }

    /// `property` in pixels, or `None` when unset, `auto`, or a percentage
    /// with no base.
    pub fn px(&self, property: &str, percent_base: f64) -> Option<f64> {
        // While a box is measured for its content width the room is a stand-in for
        // "unlimited" (`intrinsic::UNBOUNDED`): a percentage of that is not a size, it is
        // `auto` (the percentage is cyclic with the content that is being measured).
        let base = if percent_base >= crate::intrinsic::UNBOUNDED / 2.0 {
            f64::NAN
        } else {
            percent_base
        };
        self.raw(property)?.to_px(&self.length_context(base))
    }

    /// Whether the property is `auto` (an unset property is not).
    pub fn is_auto(&self, property: &str) -> bool {
        self.keyword(property) == Some("auto")
    }

    /// A box-model side: 0 when unset or not a length.
    pub fn side(&self, property: &str, percent_base: f64) -> f64 {
        self.px(property, percent_base).unwrap_or(0.0)
    }

    pub fn display(&self) -> Display {
        self.style
            .map_or(Display::Inline, |s| Display::parse(&s.display))
    }

    pub fn position(&self) -> Position {
        match self.keyword("position") {
            Some("relative") => Position::Relative,
            Some("absolute") => Position::Absolute,
            Some("fixed") => Position::Fixed,
            Some("sticky" | "-webkit-sticky") => Position::Sticky,
            _ => Position::Static,
        }
    }

    pub fn is_out_of_flow(&self) -> bool {
        matches!(self.position(), Position::Absolute | Position::Fixed)
    }

    pub fn float(&self) -> Option<Side> {
        if self.is_out_of_flow() {
            return None;
        }
        let rtl = self.container_is_rtl();
        match self.keyword("float") {
            Some("left") => Some(Side::Left),
            Some("right") => Some(Side::Right),
            Some("inline-start") => Some(if rtl { Side::Right } else { Side::Left }),
            Some("inline-end") => Some(if rtl { Side::Left } else { Side::Right }),
            _ => None,
        }
    }

    /// The containing block runs right to left: the direction `inline-start` and `inline-end`
    /// floats and clears are relative to.
    fn container_is_rtl(&self) -> bool {
        self.ctx
            .parent(self.node)
            .is_some_and(|parent| self.ctx.style(parent).is_rtl())
    }

    pub fn clear(&self) -> Clear {
        let rtl = self.container_is_rtl();
        match self.keyword("clear") {
            Some("left") => Clear::Left,
            Some("right") => Clear::Right,
            Some("inline-start") => if rtl { Clear::Right } else { Clear::Left },
            Some("inline-end") => if rtl { Clear::Left } else { Clear::Right },
            Some("both") => Clear::Both,
            _ => Clear::None,
        }
    }

    pub fn is_border_box(&self) -> bool {
        self.keyword("box-sizing") == Some("border-box")
    }

    /// Whether `overflow-x` or `-y` is anything but `visible`: the box clips
    /// its content and establishes its own block formatting context.
    pub fn clips_overflow(&self) -> bool {
        ["overflow-x", "overflow-y"]
            .iter()
            .any(|p| matches!(self.keyword(p), Some("hidden" | "scroll" | "auto" | "clip")))
    }

    pub fn is_visible(&self) -> bool {
        !matches!(self.keyword("visibility"), Some("hidden" | "collapse"))
    }

    /// `z-index`, or `None` for `auto`.
    pub fn z_index(&self) -> Option<i32> {
        match self.raw("z-index") {
            Some(Value::Number(n)) if n.fract() == 0.0 => Some(*n as i32),
            _ => None,
        }
    }

    /// The border-box edges for padding/border, resolved against the
    /// containing block width.
    pub fn padding(&self, cb_width: f64) -> Edges {
        Edges {
            top: self.side("padding-top", cb_width).max(0.0),
            right: self.side("padding-right", cb_width).max(0.0),
            bottom: self.side("padding-bottom", cb_width).max(0.0),
            left: self.side("padding-left", cb_width).max(0.0),
        }
    }

    /// Border widths; a side with no style (or `none`/`hidden`) has no width.
    pub fn border(&self, cb_width: f64) -> Edges {
        let side = |name: &str| {
            let styled = match self.keyword(&format!("border-{name}-style")) {
                // The engine has long drawn any border that has a width and
                // no explicit style as solid; keep that for compatibility.
                None => true,
                Some(style) => !matches!(style, "none" | "hidden"),
            };
            if styled {
                self.side(&format!("border-{name}-width"), cb_width)
                    .max(0.0)
            } else {
                0.0
            }
        };
        Edges {
            top: side("top"),
            right: side("right"),
            bottom: side("bottom"),
            left: side("left"),
        }
    }
}
