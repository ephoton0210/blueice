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

    #[allow(dead_code)] // used by float and flex layout, which follow
    /// The block-level form of this display (what `float` and absolute
    /// positioning compute to).
    pub fn blockified(self) -> Display {
        match self {
            Display::Inline | Display::InlineBlock => Display::Block,
            Display::InlineFlex => Display::Flex,
            Display::InlineGrid => Display::Grid,
            Display::InlineTable => Display::Table,
            other => other,
        }
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
#[allow(dead_code)] // used by float layout, which follows
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

    #[allow(dead_code)]
    pub fn is_element(&self, node: NodeId) -> bool {
        matches!(self.doc.data(node), NodeData::Element { .. })
    }

    pub fn attr(&self, node: NodeId, name: &str) -> Option<&str> {
        match self.doc.data(node) {
            NodeData::Element { attributes, .. } => attributes
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.as_str()),
            _ => None,
        }
    }

    pub fn tag(&self, node: NodeId) -> Option<&str> {
        match self.doc.data(node) {
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
        self.style.and_then(|s| s.other.get(property))
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
        self.raw(property)?
            .to_px(&self.length_context(percent_base))
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
        match self.keyword("float") {
            Some("left" | "inline-start") => Some(Side::Left),
            Some("right" | "inline-end") => Some(Side::Right),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn clear(&self) -> Clear {
        match self.keyword("clear") {
            Some("left" | "inline-start") => Clear::Left,
            Some("right" | "inline-end") => Clear::Right,
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
