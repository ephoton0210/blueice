// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The cascade: DOM + stylesheets (in origin order) -> a per-element
//! [`ComputedStyle`] map, per `phase-2-mvp-scope/PLAN.md`'s "MVP CSS
//! scope".
//!
//! Two origins only (`Origin::Ua < Origin::Author`), matching
//! `research/css-cascade.md` §4's recommendation that a from-scratch
//! two-tier subset of the real `UA < User < PresHints < Author <
//! Animations < Transitions` ordering both engines converge on is
//! spec-faithful, not a guess. Sort key is `(important, origin,
//! specificity, source_order)`, exactly mirroring how both Stylo
//! (`CascadeLevel`/`CascadeOrigin` bit-packing) and Blink
//! (`CascadePriority`'s origin/importance XOR trick) implement
//! "`!important` reverses origin precedence, but layer/source order
//! still breaks ties the normal way" as one comparable tuple rather
//! than a multi-step comparator.
//!
//! Only `color`, `font-family`, `font-size`, `font-style`, `line-height`,
//! and `text-align` get real inheritance + computed-value resolution
//! (em/percentage-of-parent-font-size, `currentColor`) -- per
//! `css-cascade.md` §4, that's the minimum a cascade needs to render
//! basic typography correctly, and everything else (box model,
//! position, flex) is stored as its *specified* value in
//! [`ComputedStyle::other`] for `blueice-layout` to resolve once it
//! exists, since resolving percentages/etc. against a containing block
//! is a layout concern, not a cascade one.

use crate::parser::{parse, Declaration, Rule};
use crate::selector::{
    matches, matches_pseudo_element, specificity, PseudoElement, SelectorKey, Specificity,
};
use crate::value::{Color, LengthContext, Value};
use blueice_dom::{Document, NodeData, NodeId};
use std::collections::HashMap;

mod incremental;
pub use incremental::cascade_incremental;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Origin {
    Ua,
    Author,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ComputedStyle {
    pub display: String,
    pub color: Color,
    pub font_size_px: f64,
    pub font_family: Option<String>,
    pub font_style: Option<String>,
    pub line_height: Option<Value>,
    pub text_align: Option<String>,
    /// Every other supported longhand's *specified* value (not
    /// inherited, not computed against a containing block or parent).
    pub other: HashMap<String, Value>,
    /// The element's resolved custom properties (`--name`), inherited from
    /// its parent and overlaid with its own.
    pub custom: crate::vars::SharedCustomProperties,
}

impl ComputedStyle {
    /// `font-weight: bold` or a numeric weight of 600 or above --
    /// centralized here (rather than duplicated in `blueice-paint` and
    /// `blueice-layout`, both of which need it: paint to pick a bold
    /// glyph, layout to measure text at the width that glyph will
    /// actually render at) since it's a real-CSS rule about a
    /// `ComputedStyle` value, not paint- or layout-specific logic.
    pub fn is_bold(&self) -> bool {
        matches!(self.other.get("font-weight"), Some(Value::Keyword(k)) if k == "bold")
            || matches!(self.other.get("font-weight"), Some(Value::Number(n)) if *n >= 600.0)
    }

    pub fn is_italic(&self) -> bool {
        self.font_style.as_deref() == Some("italic")
    }

    /// `opacity`, clamped to CSS's own `[0.0, 1.0]` range -- added for
    /// `phase-1-ai-representation-layer/PLAN.md`'s `AiNode::opacity`
    /// field (a real gap `phase-2-mvp-scope/PLAN.md`'s cross-check
    /// found: nothing sourced this field before). Not inherited, per
    /// spec -- unset on an element defaults to fully opaque regardless
    /// of an ancestor's own `opacity`, matching the `other` map's
    /// general "specified value only" contract (see the struct docs);
    /// `blueice-paint` applies it as a flat per-element alpha multiply,
    /// not real group/layer compositing (out of MVP scope, see
    /// `blueice-paint`'s own module docs).
    pub fn opacity(&self) -> f32 {
        match self.other.get("opacity") {
            Some(Value::Number(n)) => (*n as f32).clamp(0.0, 1.0),
            _ => 1.0,
        }
    }

    fn initial() -> Self {
        ComputedStyle {
            display: "inline".to_string(),
            color: Color::Rgba(0, 0, 0, 255),
            font_size_px: 16.0,
            font_family: None,
            font_style: None,
            line_height: None,
            text_align: None,
            other: HashMap::new(),
            custom: Default::default(),
        }
    }
}

const INHERITED_PROPERTIES: &[&str] = &[
    "color",
    "font-family",
    "font-size",
    "font-style",
    "line-height",
    "text-align",
];

/// Properties real CSS inherits that don't have a dedicated
/// [`ComputedStyle`] field (unlike `color`/`font-family`/etc. above) --
/// handled generically by copying the parent's [`ComputedStyle::other`]
/// entry when the element itself doesn't set one, rather than adding a
/// bespoke field + fallback arm per property the way the others need.
const OTHER_INHERITED_PROPERTIES: &[&str] = &[
    "font-weight",
    "font-variant",
    "font-stretch",
    "letter-spacing",
    "word-spacing",
    "text-transform",
    "text-indent",
    "text-shadow",
    "text-rendering",
    "white-space",
    "word-break",
    "overflow-wrap",
    "word-wrap",
    "hyphens",
    "tab-size",
    "direction",
    "visibility",
    "cursor",
    "pointer-events",
    "list-style-type",
    "list-style-position",
    "list-style-image",
    "quotes",
    "border-collapse",
    "border-spacing",
    "caption-side",
    "empty-cells",
    "color-scheme",
    "fill",
    "stroke",
    "-webkit-text-size-adjust",
    "-webkit-font-smoothing",
];

/// The absolute-size keywords (CSS Fonts 4), in CSS pixels, at the default
/// medium of 16px.
fn font_size_keyword(word: &str) -> Option<f64> {
    Some(match word {
        "xx-small" => 9.0,
        "x-small" => 10.0,
        "small" => 13.0,
        "medium" => 16.0,
        "large" => 18.0,
        "x-large" => 24.0,
        "xx-large" => 32.0,
        "xxx-large" => 48.0,
        _ => return None,
    })
}

fn resolve_font_size(value: &Value, ctx: &LengthContext) -> Option<f64> {
    match value {
        Value::Keyword(word) if word == "smaller" => Some(ctx.font_size / 1.2),
        Value::Keyword(word) if word == "larger" => Some(ctx.font_size * 1.2),
        Value::Keyword(word) => font_size_keyword(word),
        // A percentage of the parent's font size, like `em`.
        other => other.to_px(ctx).filter(|px| *px >= 0.0),
    }
}

fn resolve_color(value: &Value, inherited_color: Color) -> Option<Color> {
    match value {
        Value::Color(Color::CurrentColor) => Some(inherited_color),
        Value::Color(c) => Some(*c),
        _ => None,
    }
}

struct Matched<'a> {
    origin: Origin,
    specificity: Specificity,
    source_order: usize,
    declaration: &'a Declaration,
}

/// The viewport `vw`/`vh` resolve against when a caller does not supply one.
const DEFAULT_VIEWPORT: (f64, f64) = (1280.0, 720.0);

/// Cascades `stylesheets` (in ascending origin precedence) against
/// every element in `doc`, returning each element [`NodeId`]'s computed
/// style. Non-element nodes (the document node, text nodes) never
/// appear in the returned map. Viewport units resolve against a 1280x720
/// viewport; use [`cascade_with_viewport`] to supply the real one.
pub fn cascade(
    doc: &Document,
    stylesheets: &[(Origin, &[Rule])],
) -> HashMap<NodeId, ComputedStyle> {
    cascade_with_viewport(doc, stylesheets, DEFAULT_VIEWPORT.0, DEFAULT_VIEWPORT.1)
}

/// [`cascade`] with the viewport that `vw`/`vh`/`vmin`/`vmax` font sizes and
/// custom-property-fed values resolve against.
pub fn cascade_with_viewport(
    doc: &Document,
    stylesheets: &[(Origin, &[Rule])],
    viewport_width: f64,
    viewport_height: f64,
) -> HashMap<NodeId, ComputedStyle> {
    let index = RuleIndex::build(stylesheets);
    let mut styles = HashMap::new();
    let environment = Environment {
        viewport_width,
        viewport_height,
    };
    cascade_subtree(
        doc,
        doc.root(),
        &index,
        &environment,
        None,
        16.0,
        &mut styles,
    );
    styles
}

#[derive(Clone, Copy)]
struct Environment {
    viewport_width: f64,
    viewport_height: f64,
}

/// Every rule once, with the buckets that find the rules that can match an
/// element without testing them all: by the id, a class or the type its
/// rightmost compound requires, plus the rules that require none.
struct RuleIndex<'a> {
    rules: Vec<IndexedRule<'a>>,
    by_id: HashMap<&'a str, Vec<u32>>,
    by_class: HashMap<&'a str, Vec<u32>>,
    by_type: HashMap<&'a str, Vec<u32>>,
    universal: Vec<u32>,
    /// How the rules look at siblings, for restyling only what a change can reach.
    structure: incremental::StructureUse,
}

struct IndexedRule<'a> {
    origin: Origin,
    source_order: usize,
    rule: &'a Rule,
}

impl<'a> RuleIndex<'a> {
    fn build(stylesheets: &'a [(Origin, &'a [Rule])]) -> Self {
        let mut index = RuleIndex {
            rules: Vec::new(),
            by_id: HashMap::new(),
            by_class: HashMap::new(),
            by_type: HashMap::new(),
            universal: Vec::new(),
            structure: Default::default(),
        };
        for (origin, rules) in stylesheets {
            for (source_order, rule) in rules.iter().enumerate() {
                let id = index.rules.len() as u32;
                index.rules.push(IndexedRule {
                    origin: *origin,
                    source_order,
                    rule,
                });
                for selector in &rule.selectors {
                    index.structure.scan(selector);
                }
                let mut keys: Vec<SelectorKey<'a>> =
                    rule.selectors.iter().map(|s| s.key()).collect();
                keys.dedup();
                for key in keys {
                    let bucket = match key {
                        SelectorKey::Id(name) => index.by_id.entry(name).or_default(),
                        SelectorKey::Class(name) => index.by_class.entry(name).or_default(),
                        SelectorKey::Type(name) => index.by_type.entry(name).or_default(),
                        SelectorKey::Universal => &mut index.universal,
                    };
                    if bucket.last() != Some(&id) {
                        bucket.push(id);
                    }
                }
            }
        }
        index
    }

    /// The rule ids that might match `node`, ascending and without repeats.
    fn candidates(&self, doc: &Document, node: NodeId) -> Vec<u32> {
        let NodeData::Element {
            tag_name,
            attributes,
        } = doc.data(node)
        else {
            return Vec::new();
        };
        let mut found: Vec<u32> = self.universal.clone();
        if let Some(rules) = self.by_type.get(tag_name.as_str()) {
            found.extend(rules);
        }
        for (name, value) in attributes {
            if name == "id" {
                if let Some(rules) = self.by_id.get(value.as_str()) {
                    found.extend(rules);
                }
            } else if name == "class" {
                for class in value.split_whitespace() {
                    if let Some(rules) = self.by_class.get(class) {
                        found.extend(rules);
                    }
                }
            }
        }
        found.sort_unstable();
        found.dedup();
        found
    }
}

fn cascade_subtree(
    doc: &Document,
    node: NodeId,
    index: &RuleIndex<'_>,
    environment: &Environment,
    parent_style: Option<&ComputedStyle>,
    root_font_size: f64,
    styles: &mut HashMap<NodeId, ComputedStyle>,
) {
    let is_element = matches!(doc.data(node), NodeData::Element { .. });
    let mut child_root_font_size = root_font_size;
    let own_style = if is_element {
        let style = compute_style_for(
            doc,
            node,
            index,
            environment,
            parent_style,
            root_font_size,
            None,
        )
        .expect("an element always has a style");
        if doc.parent(node) == Some(doc.root()) {
            child_root_font_size = style.font_size_px;
        }
        // `::before` / `::after` get a style of their own, under ids no DOM
        // node has, when a rule gives them `content`.
        for which in [PseudoElement::Before, PseudoElement::After] {
            if let Some(pseudo) = compute_style_for(
                doc,
                node,
                index,
                environment,
                Some(&style),
                root_font_size,
                Some(which),
            ) {
                if matches!(pseudo.other.get("content"), Some(Value::Unparsed(_))) {
                    styles.insert(pseudo_node_id(node, which), pseudo);
                }
            }
        }
        styles.insert(node, style.clone());
        Some(style)
    } else {
        parent_style.cloned()
    };

    for child in doc.children(node) {
        cascade_subtree(
            doc,
            child,
            index,
            environment,
            own_style.as_ref(),
            child_root_font_size,
            styles,
        );
    }
}

fn inline_style_declarations(doc: &Document, node: NodeId) -> Vec<Declaration> {
    let NodeData::Element { attributes, .. } = doc.data(node) else {
        return Vec::new();
    };
    let Some((_, style_attr)) = attributes.iter().find(|(k, _)| k == "style") else {
        return Vec::new();
    };
    crate::parser::parse_declarations(&crate::tokenizer::tokenize(style_attr))
}

/// The parent's value for `property`, as the specified value `inherit` stands
/// for, or `None` when the parent has none (the property's initial value).
fn parent_value(parent: &ComputedStyle, property: &str) -> Option<Value> {
    let keyword = |text: &Option<String>| text.clone().map(Value::Keyword);
    match property {
        "font-size" => Some(Value::Length(crate::value::Length::Px(parent.font_size_px))),
        "color" => Some(Value::Color(parent.color)),
        "display" => Some(Value::Keyword(parent.display.clone())),
        "font-family" => keyword(&parent.font_family),
        "font-style" => keyword(&parent.font_style),
        "text-align" => keyword(&parent.text_align),
        "line-height" => parent.line_height.clone(),
        other => parent.other.get(other).cloned(),
    }
}

fn is_inherited(property: &str) -> bool {
    INHERITED_PROPERTIES.contains(&property) || OTHER_INHERITED_PROPERTIES.contains(&property)
}

fn compute_style_for(
    doc: &Document,
    node: NodeId,
    index: &RuleIndex<'_>,
    environment: &Environment,
    parent_style: Option<&ComputedStyle>,
    root_font_size: f64,
    pseudo: Option<PseudoElement>,
) -> Option<ComputedStyle> {
    let mut matched: Vec<Matched> = Vec::new();
    for rule_id in index.candidates(doc, node) {
        let indexed = &index.rules[rule_id as usize];
        let best_specificity = indexed
            .rule
            .selectors
            .iter()
            .filter(|s| match pseudo {
                None => matches(doc, node, s),
                Some(which) => {
                    s.pseudo_element() == Some(which) && matches_pseudo_element(doc, node, s)
                }
            })
            .map(specificity)
            .max();
        let Some(spec) = best_specificity else {
            continue;
        };
        for decl in &indexed.rule.declarations {
            matched.push(Matched {
                origin: indexed.origin,
                specificity: spec,
                source_order: indexed.source_order,
                declaration: decl,
            });
        }
    }

    // Inline `style="..."` (plan §"MVP CSS scope": part of the Author
    // origin, same as a `<style>` sheet). Per spec it beats *any*
    // selector-based author rule regardless of specificity, but is
    // still beaten by an `!important` declaration from any origin --
    // achieved here by giving it a specificity no real selector can
    // reach, rather than a new sort-key tier.
    if pseudo.is_some() && matched.is_empty() {
        return None;
    }
    let inline_declarations = if pseudo.is_some() {
        Vec::new()
    } else {
        inline_style_declarations(doc, node)
    };
    for decl in &inline_declarations {
        matched.push(Matched {
            origin: Origin::Author,
            specificity: (u32::MAX, 0, 0),
            source_order: usize::MAX,
            declaration: decl,
        });
    }

    // `!important` fully reverses origin precedence (module docs above):
    // within the `important` bucket, UA must outrank Author, the exact
    // opposite of the normal (non-important) bucket. `Origin`'s own
    // `Ord` only ever encodes the normal direction (`Ua < Author`), so
    // it can't be used directly here -- flip it specifically when
    // `important` is true, rather than relying on `m.origin` sorting
    // consistently across both buckets (it can't, by definition of what
    // "reverses" means).
    matched.sort_by_key(|m| {
        let origin_rank = m.origin as u8;
        let effective_origin_rank = if m.declaration.important {
            1 - origin_rank
        } else {
            origin_rank
        };
        (
            m.declaration.important,
            effective_origin_rank,
            m.specificity,
            m.source_order,
        )
    });

    let default = ComputedStyle::initial();
    let parent = parent_style.unwrap_or(&default);

    // Custom properties first: every `var()` below reads the finished set.
    let mut own_custom: HashMap<String, Vec<crate::tokenizer::Token>> = HashMap::new();
    for m in &matched {
        if m.declaration.property.starts_with("--") {
            if let Value::Unparsed(tokens) = &m.declaration.value {
                own_custom.insert(m.declaration.property.clone(), tokens.clone());
            }
        }
    }
    let custom = crate::vars::resolve(&parent.custom, own_custom);

    // Winning value per property, later (higher precedence) declarations
    // overriding earlier ones. A `var()` declaration is substituted and then
    // parsed here -- a shorthand may expand to several longhands -- and is
    // dropped entirely when a reference cannot be satisfied.
    let mut winners: HashMap<String, Value> = HashMap::new();
    for m in &matched {
        let property = m.declaration.property.as_str();
        if property.starts_with("--") {
            continue;
        }
        match &m.declaration.value {
            Value::Unparsed(tokens) => {
                let Some(substituted) = crate::vars::substitute(tokens, &custom) else {
                    continue;
                };
                for (longhand, value) in crate::parser::expand_property(property, &substituted) {
                    winners.insert(longhand, value);
                }
            }
            value => {
                winners.insert(property.to_string(), value.clone());
            }
        }
    }

    // CSS-wide keywords: `inherit`, `initial`, `unset` (and `revert`, which
    // with no user origin behaves like `unset`).
    let wide: Vec<(String, String)> = winners
        .iter()
        .filter_map(|(property, value)| match value {
            Value::Keyword(k)
                if matches!(k.as_str(), "inherit" | "initial" | "unset" | "revert") =>
            {
                Some((property.clone(), k.clone()))
            }
            _ => None,
        })
        .collect();
    // Properties explicitly returned to their initial value must not pick the
    // parent's value back up through ordinary inheritance below.
    let mut reset: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (property, keyword) in wide {
        let inherit = keyword == "inherit" || (keyword != "initial" && is_inherited(&property));
        match inherit.then(|| parent_value(parent, &property)).flatten() {
            Some(value) => {
                winners.insert(property, value);
            }
            None => {
                winners.remove(&property);
                reset.insert(property);
            }
        }
    }

    let mut style = ComputedStyle::initial();
    style.custom = custom;

    let font_context = LengthContext {
        font_size: parent.font_size_px,
        root_font_size,
        viewport_width: environment.viewport_width,
        viewport_height: environment.viewport_height,
        percent_base: parent.font_size_px,
    };
    style.font_size_px = winners
        .get("font-size")
        .and_then(|v| resolve_font_size(v, &font_context))
        .unwrap_or(parent.font_size_px);

    style.color = winners
        .get("color")
        .and_then(|v| resolve_color(v, parent.color))
        .unwrap_or(parent.color);

    style.display = winners
        .get("display")
        .and_then(|v| {
            if let Value::Keyword(k) = v {
                Some(k.clone())
            } else {
                None
            }
        })
        .unwrap_or_else(|| default.display.clone());

    let inherited_keyword = |property: &str, fallback: &Option<String>| {
        winners
            .get(property)
            .and_then(|v| {
                if let Value::Keyword(k) = v {
                    Some(k.clone())
                } else {
                    None
                }
            })
            .or_else(|| fallback.clone())
    };
    style.font_family = inherited_keyword("font-family", &parent.font_family);
    style.font_style = inherited_keyword("font-style", &parent.font_style);
    style.text_align = inherited_keyword("text-align", &parent.text_align);

    style.line_height = winners
        .get("line-height")
        .cloned()
        .or_else(|| parent.line_height.clone());

    for (prop, value) in &winners {
        let prop = prop.as_str();
        if INHERITED_PROPERTIES.contains(&prop) || prop == "display" {
            continue;
        }
        // currentColor can appear on any color-valued property
        // (background-color, border-*-color, ...), not just `color`
        // itself -- resolve it here against the color already computed
        // above, rather than leaving it unresolved in `other`.
        let resolved = match value {
            Value::Color(Color::CurrentColor) => Value::Color(style.color),
            other => other.clone(),
        };
        style.other.insert(prop.to_string(), resolved);
    }
    for prop in OTHER_INHERITED_PROPERTIES {
        if !style.other.contains_key(*prop) && !reset.contains(*prop) {
            if let Some(v) = parent.other.get(*prop) {
                style.other.insert(prop.to_string(), v.clone());
            }
        }
    }

    Some(style)
}

/// The id under which a node's generated `::before` / `::after` box is
/// styled: no DOM node ever has one (the top bits are set).
pub fn pseudo_node_id(node: NodeId, which: PseudoElement) -> NodeId {
    NodeId::from_u64(PSEUDO_BIT | (node.as_u64() << 1) | u64::from(which == PseudoElement::After))
}

/// The id of the text a pseudo-element's `content` generates.
pub fn pseudo_text_node_id(pseudo: NodeId) -> NodeId {
    NodeId::from_u64(pseudo.as_u64() | PSEUDO_TEXT_BIT)
}

const PSEUDO_BIT: u64 = 1 << 62;
const PSEUDO_TEXT_BIT: u64 = 1 << 61;

/// Whether `id` names a generated pseudo-element box or its text.
pub fn is_pseudo_node(id: NodeId) -> bool {
    id.as_u64() & PSEUDO_BIT != 0
}

/// The element that generated pseudo-element `id` (or its text).
pub fn pseudo_owner(id: NodeId) -> NodeId {
    NodeId::from_u64((id.as_u64() & !(PSEUDO_BIT | PSEUDO_TEXT_BIT)) >> 1)
}

/// The pseudo-element side of `id`.
pub fn pseudo_kind(id: NodeId) -> PseudoElement {
    if id.as_u64() & 1 == 1 {
        PseudoElement::After
    } else {
        PseudoElement::Before
    }
}

/// A built-in default stylesheet giving the Phase 2 HTML element list
/// sensible `display` values and a handful of typographic defaults
/// (headings, bold/italic text-level elements) -- expressed as ordinary
/// CSS text, parsed through the same parser as an author stylesheet,
/// rather than a separate hardcoded Rust representation. Table-related
/// elements get `display: block` (see `phase-2-mvp-scope/PLAN.md`:
/// table layout itself is deferred, so there's no `table`/`table-row`/
/// `table-cell` display value for them to use yet).
pub const UA_STYLESHEET_SOURCE: &str = r#"
/* Display types, after the HTML Standard's "Rendering" section. */
html, body, div, p, ul, ol, li, section, article, header, footer, nav,
main, aside, figure, figcaption, blockquote, pre, form, fieldset, legend,
address, hgroup, details, summary, dialog, dl, dt, dd, menu, center,
h1, h2, h3, h4, h5, h6, hr, search, optgroup {
  display: block;
}
li { display: list-item; }
table { display: table; border-spacing: 2px; box-sizing: border-box; }
caption { display: table-caption; text-align: center; }
thead { display: table-header-group; }
tbody { display: table-row-group; }
tfoot { display: table-footer-group; }
tr { display: table-row; }
td, th { display: table-cell; padding: 1px; vertical-align: inherit; }
col, colgroup { display: table-column; }
thead, tbody, tfoot, tr { vertical-align: middle; }
th { text-align: center; }
button, input, select, textarea {
  display: inline-block;
}
/* The MVP block algorithm deliberately treats inline-block as a block box.
 * Give editable inputs a visible, stable control box within that model so a
 * value written through the AI action path is observable in the same frame as
 * the rest of the page. This is a narrow text-input default, not an attempt
 * to reproduce platform-native form controls. */
input, textarea, select {
  box-sizing: border-box;
  width: 20em;
  height: 2em;
  padding: 2px 4px;
  border: 1px solid #767676;
  background-color: #ffffff;
}
textarea { height: 6em; }
input[type="checkbox"], input[type="radio"] { width: 1.5em; height: 1.5em; padding: 1px; }
input[type="range"] { width: 12em; height: 1.5em; }
[hidden], input[type="hidden"] { display: none; }
area, base, basefont, datalist, head, link, meta, noembed, noframes, param,
rp, script, style, template, title {
  display: none;
}
/* Text-level semantics. */
b, strong, th { font-weight: bold; }
i, em, cite, dfn, var, address { font-style: italic; }
u, ins { text-decoration: underline; }
s, strike, del { text-decoration: line-through; }
a:link { color: #0000ee; text-decoration: underline; }
small { font-size: 0.83em; }
big { font-size: 1.17em; }
sub { font-size: 0.83em; vertical-align: sub; }
sup { font-size: 0.83em; vertical-align: super; }
code, kbd, samp, tt, pre, textarea { font-family: monospace; }
pre { white-space: pre; }
th { text-align: center; }
/* Flow content. */
body { margin: 8px; }
p, blockquote, figure, dl, menu, ul, ol, pre { margin-top: 1em; margin-bottom: 1em; }
blockquote, figure { margin-left: 40px; margin-right: 40px; }
dd { margin-left: 40px; }
ul, ol, menu { padding-left: 40px; }
ul, menu { list-style-type: disc; }
ol { list-style-type: decimal; }
ul ul, ol ul { list-style-type: circle; }
ul ul ul, ol ul ul, ul ol ul, ol ol ul { list-style-type: square; }
ul ul, ul ol, ol ul, ol ol { margin-top: 0; margin-bottom: 0; }
hr { margin: 0.5em auto; border: 1px solid #9a9a9a; height: 0; overflow: hidden; }
fieldset { margin: 0 2px; padding: 0.35em 0.75em 0.625em; border: 2px solid #c0c0c0; }
h1 { font-size: 2em; font-weight: bold; margin-top: 0.67em; margin-bottom: 0.67em; }
h2 { font-size: 1.5em; font-weight: bold; margin-top: 0.83em; margin-bottom: 0.83em; }
h3 { font-size: 1.17em; font-weight: bold; margin-top: 1em; margin-bottom: 1em; }
h4 { font-size: 1em; font-weight: bold; margin-top: 1.33em; margin-bottom: 1.33em; }
h5 { font-size: 0.83em; font-weight: bold; margin-top: 1.67em; margin-bottom: 1.67em; }
h6 { font-size: 0.67em; font-weight: bold; margin-top: 2.33em; margin-bottom: 2.33em; }
"#;

pub fn ua_stylesheet() -> Vec<Rule> {
    parse(UA_STYLESHEET_SOURCE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Length;

    fn elem(doc: &mut Document, parent: NodeId, tag: &str, attrs: &[(&str, &str)]) -> NodeId {
        let id = doc.create_node(NodeData::Element {
            tag_name: tag.to_string(),
            attributes: attrs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        });
        doc.append_child(parent, id);
        id
    }

    #[test]
    fn author_beats_ua_at_equal_specificity_by_origin_order() {
        let mut doc = Document::new();
        let root = doc.root();

        let p = elem(&mut doc, root, "p", &[]);
        let ua = parse("p { color: red; }");
        let author = parse("p { color: blue; }");
        let styles = cascade(&doc, &[(Origin::Ua, &ua), (Origin::Author, &author)]);
        assert_eq!(styles[&p].color, Color::Rgba(0, 0, 255, 255));
    }

    #[test]
    fn origin_outranks_specificity_author_always_beats_ua() {
        // real CSS semantics, not a simplification: origin is the
        // *first* sort key, before specificity -- otherwise a browser's
        // own UA stylesheet could never be reliably overridden by an
        // author's lower-specificity rules.
        let mut doc = Document::new();
        let root = doc.root();

        let p = elem(&mut doc, root, "p", &[("id", "x")]);
        let ua = parse("#x { color: red; }");
        let author = parse("p { color: blue; }");
        let styles = cascade(&doc, &[(Origin::Ua, &ua), (Origin::Author, &author)]);
        assert_eq!(
            styles[&p].color,
            Color::Rgba(0, 0, 255, 255),
            "author's rule wins even though UA's #id is more specific"
        );
    }

    #[test]
    fn higher_specificity_wins_within_the_same_origin() {
        let mut doc = Document::new();
        let root = doc.root();
        let p = elem(&mut doc, root, "p", &[("id", "x")]);
        let author = parse("#x { color: red; } p { color: blue; }");
        let styles = cascade(&doc, &[(Origin::Author, &author)]);
        assert_eq!(styles[&p].color, Color::Rgba(255, 0, 0, 255));
    }

    #[test]
    fn important_reverses_origin_precedence() {
        let mut doc = Document::new();
        let root = doc.root();

        let p = elem(&mut doc, root, "p", &[]);
        let ua = parse("p { color: red !important; }");
        let author = parse("p { color: blue; }");
        let styles = cascade(&doc, &[(Origin::Ua, &ua), (Origin::Author, &author)]);
        assert_eq!(styles[&p].color, Color::Rgba(255, 0, 0, 255));
    }

    #[test]
    fn ua_important_beats_author_important() {
        // Regression: `!important` must fully reverse origin precedence,
        // not just beat non-important rules -- when *both* sides are
        // `!important`, UA must still win over Author.
        let mut doc = Document::new();
        let root = doc.root();

        let p = elem(&mut doc, root, "p", &[]);
        let ua = parse("p { color: red !important; }");
        let author = parse("p { color: blue !important; }");
        let styles = cascade(&doc, &[(Origin::Ua, &ua), (Origin::Author, &author)]);
        assert_eq!(styles[&p].color, Color::Rgba(255, 0, 0, 255));
    }

    #[test]
    fn later_rule_wins_at_equal_specificity_and_origin() {
        let mut doc = Document::new();
        let root = doc.root();

        let p = elem(&mut doc, root, "p", &[]);
        let author = parse("p { color: red; } p { color: blue; }");
        let styles = cascade(&doc, &[(Origin::Author, &author)]);
        assert_eq!(styles[&p].color, Color::Rgba(0, 0, 255, 255));
    }

    #[test]
    fn color_inherits_from_parent_when_not_set() {
        let mut doc = Document::new();
        let root = doc.root();

        let div = elem(&mut doc, root, "div", &[]);
        let p = elem(&mut doc, div, "p", &[]);
        let author = parse("div { color: green; }");
        let styles = cascade(&doc, &[(Origin::Author, &author)]);
        assert_eq!(styles[&p].color, Color::Rgba(0, 128, 0, 255));
    }

    #[test]
    fn display_does_not_inherit() {
        let mut doc = Document::new();
        let root = doc.root();

        let div = elem(&mut doc, root, "div", &[]);
        let span = elem(&mut doc, div, "span", &[]);
        let author = parse("div { display: none; }");
        let styles = cascade(&doc, &[(Origin::Author, &author)]);
        assert_eq!(
            styles[&span].display, "inline",
            "span keeps the cascade's initial value, not div's display: none"
        );
    }

    #[test]
    fn font_weight_inherits_into_a_nested_element_with_no_font_weight_of_its_own() {
        // real CSS inheritance, caught by a review pass while wiring
        // bold text into rendering: <b><span>x</span></b> must make the
        // span's own text bold too, even though nothing ever targets
        // `span` directly with a font-weight declaration -- font-weight
        // has no dedicated ComputedStyle field the way color/font-style
        // do, so it needs the generic OTHER_INHERITED_PROPERTIES path.
        let mut doc = Document::new();
        let root = doc.root();
        let b = elem(&mut doc, root, "b", &[]);
        let span = elem(&mut doc, b, "span", &[]);
        let ua = ua_stylesheet();
        let styles = cascade(&doc, &[(Origin::Ua, &ua)]);
        assert_eq!(
            styles[&b].other.get("font-weight"),
            Some(&Value::Keyword("bold".to_string()))
        );
        assert_eq!(
            styles[&span].other.get("font-weight"),
            Some(&Value::Keyword("bold".to_string())),
            "span must inherit bold from its <b> ancestor"
        );
    }

    #[test]
    fn font_weight_set_directly_overrides_inheritance() {
        let mut doc = Document::new();
        let root = doc.root();
        let b = elem(&mut doc, root, "b", &[]);
        let span = elem(&mut doc, b, "span", &[]);
        let ua = ua_stylesheet();
        let author = parse("span { font-weight: normal; }");
        let styles = cascade(&doc, &[(Origin::Ua, &ua), (Origin::Author, &author)]);
        assert_eq!(
            styles[&span].other.get("font-weight"),
            Some(&Value::Keyword("normal".to_string()))
        );
    }

    #[test]
    fn em_font_size_resolves_against_parent_computed_font_size() {
        let mut doc = Document::new();
        let root = doc.root();

        let div = elem(&mut doc, root, "div", &[]);
        let span = elem(&mut doc, div, "span", &[]);
        let author = parse("div { font-size: 20px; } span { font-size: 2em; }");
        let styles = cascade(&doc, &[(Origin::Author, &author)]);
        assert_eq!(styles[&div].font_size_px, 20.0);
        assert_eq!(styles[&span].font_size_px, 40.0);
    }

    #[test]
    fn font_size_inherits_when_not_set() {
        let mut doc = Document::new();
        let root = doc.root();

        let div = elem(&mut doc, root, "div", &[]);
        let span = elem(&mut doc, div, "span", &[]);
        let author = parse("div { font-size: 20px; }");
        let styles = cascade(&doc, &[(Origin::Author, &author)]);
        assert_eq!(styles[&span].font_size_px, 20.0);
    }

    #[test]
    fn nested_em_compounds_through_ancestors() {
        let mut doc = Document::new();
        let root = doc.root();

        let div = elem(&mut doc, root, "div", &[]);
        let span = elem(&mut doc, div, "span", &[]);
        let b = elem(&mut doc, span, "b", &[]);
        let author =
            parse("div { font-size: 10px; } span { font-size: 2em; } b { font-size: 2em; }");
        let styles = cascade(&doc, &[(Origin::Author, &author)]);
        assert_eq!(styles[&span].font_size_px, 20.0);
        assert_eq!(styles[&b].font_size_px, 40.0);
    }

    #[test]
    fn current_color_resolves_to_the_elements_own_color() {
        let mut doc = Document::new();
        let root = doc.root();

        let p = elem(&mut doc, root, "p", &[]);
        let author = parse("p { color: green; background-color: currentColor; }");
        let styles = cascade(&doc, &[(Origin::Author, &author)]);
        assert_eq!(
            styles[&p].other.get("background-color"),
            Some(&Value::Color(Color::Rgba(0, 128, 0, 255)))
        );
    }

    #[test]
    fn non_inherited_properties_land_in_other_and_are_specified_not_computed() {
        let mut doc = Document::new();
        let root = doc.root();

        let p = elem(&mut doc, root, "p", &[]);
        let author = parse("p { width: 50%; margin: 10px; }");
        let styles = cascade(&doc, &[(Origin::Author, &author)]);
        assert_eq!(
            styles[&p].other.get("width"),
            Some(&Value::Percentage(50.0))
        );
        assert_eq!(
            styles[&p].other.get("margin-top"),
            Some(&Value::Length(Length::Px(10.0)))
        );
    }

    #[test]
    fn non_matching_selector_contributes_nothing() {
        let mut doc = Document::new();
        let root = doc.root();

        let p = elem(&mut doc, root, "p", &[]);
        let author = parse("span { color: red; }");
        let styles = cascade(&doc, &[(Origin::Author, &author)]);
        assert_eq!(
            styles[&p].color,
            Color::Rgba(0, 0, 0, 255),
            "initial black, span's rule never matched"
        );
    }

    #[test]
    fn text_node_is_never_a_key_in_the_computed_style_map() {
        let mut doc = Document::new();
        let text = doc.create_node(NodeData::Text {
            data: "hi".to_string(),
        });
        doc.append_child(doc.root(), text);
        let styles = cascade(&doc, &[]);
        assert!(!styles.contains_key(&text));
    }

    #[test]
    fn ua_stylesheet_gives_block_and_inline_defaults() {
        let mut doc = Document::new();
        let root = doc.root();

        let div = elem(&mut doc, root, "div", &[]);
        let span = elem(&mut doc, div, "span", &[]);
        let ua = ua_stylesheet();
        let styles = cascade(&doc, &[(Origin::Ua, &ua)]);
        assert_eq!(styles[&div].display, "block");
        assert_eq!(styles[&span].display, "inline");
    }

    #[test]
    fn ua_stylesheet_gives_headings_bold_and_larger() {
        let mut doc = Document::new();
        let root = doc.root();

        let body = elem(&mut doc, root, "body", &[]);
        let h1 = elem(&mut doc, body, "h1", &[]);
        let ua = ua_stylesheet();
        let styles = cascade(&doc, &[(Origin::Ua, &ua)]);
        assert_eq!(
            styles[&h1].font_size_px, 32.0,
            "2em against the initial 16px"
        );
        assert_eq!(
            styles[&h1].other.get("font-weight"),
            Some(&Value::Keyword("bold".to_string()))
        );
    }

    #[test]
    fn ua_stylesheet_gives_i_and_em_italic() {
        let mut doc = Document::new();
        let root = doc.root();

        let body = elem(&mut doc, root, "body", &[]);
        let i = elem(&mut doc, body, "i", &[]);
        let ua = ua_stylesheet();
        let styles = cascade(&doc, &[(Origin::Ua, &ua)]);
        assert_eq!(styles[&i].font_style, Some("italic".to_string()));
    }

    #[test]
    fn author_stylesheet_overrides_ua_defaults() {
        let mut doc = Document::new();
        let root = doc.root();

        let div = elem(&mut doc, root, "div", &[]);
        let ua = ua_stylesheet();
        let author = parse("div { display: inline; }");
        let styles = cascade(&doc, &[(Origin::Ua, &ua), (Origin::Author, &author)]);
        assert_eq!(styles[&div].display, "inline");
    }

    #[test]
    fn inline_style_attribute_beats_any_selector_based_author_rule() {
        let mut doc = Document::new();
        let root = doc.root();
        let p = elem(&mut doc, root, "p", &[("style", "color: green;")]);
        let author = parse("#nomatch p, p { color: red; }");
        let styles = cascade(&doc, &[(Origin::Author, &author)]);
        assert_eq!(styles[&p].color, Color::Rgba(0, 128, 0, 255));
    }

    #[test]
    fn important_stylesheet_rule_still_beats_inline_style() {
        let mut doc = Document::new();
        let root = doc.root();
        let p = elem(&mut doc, root, "p", &[("style", "color: green;")]);
        let author = parse("p { color: red !important; }");
        let styles = cascade(&doc, &[(Origin::Author, &author)]);
        assert_eq!(styles[&p].color, Color::Rgba(255, 0, 0, 255));
    }

    #[test]
    fn important_inline_style_beats_important_stylesheet_rule_from_same_origin() {
        let mut doc = Document::new();
        let root = doc.root();
        let p = elem(
            &mut doc,
            root,
            "p",
            &[("style", "color: green !important;")],
        );
        let author = parse("p { color: red !important; }");
        let styles = cascade(&doc, &[(Origin::Author, &author)]);
        assert_eq!(
            styles[&p].color,
            Color::Rgba(0, 128, 0, 255),
            "later/more-specific tie broken in inline's favor"
        );
    }

    #[test]
    fn element_with_no_style_attribute_is_unaffected() {
        let mut doc = Document::new();
        let root = doc.root();
        let p = elem(&mut doc, root, "p", &[]);
        let styles = cascade(&doc, &[]);
        assert_eq!(styles[&p].color, Color::Rgba(0, 0, 0, 255));
    }

    #[test]
    fn is_bold_recognizes_the_bold_keyword() {
        let mut doc = Document::new();
        let root = doc.root();
        let b = elem(&mut doc, root, "b", &[]);
        let ua = ua_stylesheet();
        let styles = cascade(&doc, &[(Origin::Ua, &ua)]);
        assert!(styles[&b].is_bold());
    }

    #[test]
    fn is_bold_recognizes_numeric_weights_of_600_or_above_but_not_below() {
        let mut doc = Document::new();
        let root = doc.root();
        let heavy = elem(&mut doc, root, "div", &[("style", "font-weight: 700;")]);
        let light = elem(&mut doc, root, "div", &[("style", "font-weight: 400;")]);
        let styles = cascade(&doc, &[]);
        assert!(styles[&heavy].is_bold());
        assert!(!styles[&light].is_bold());
    }

    #[test]
    fn is_italic_recognizes_the_italic_keyword() {
        let mut doc = Document::new();
        let root = doc.root();
        let i = elem(&mut doc, root, "i", &[]);
        let ua = ua_stylesheet();
        let styles = cascade(&doc, &[(Origin::Ua, &ua)]);
        assert!(styles[&i].is_italic());
    }

    #[test]
    fn neither_bold_nor_italic_by_default() {
        let mut doc = Document::new();
        let root = doc.root();
        let p = elem(&mut doc, root, "p", &[]);
        let styles = cascade(&doc, &[]);
        assert!(!styles[&p].is_bold());
        assert!(!styles[&p].is_italic());
    }

    #[test]
    fn opacity_defaults_to_fully_opaque() {
        let mut doc = Document::new();
        let root = doc.root();
        let div = elem(&mut doc, root, "div", &[]);
        let styles = cascade(&doc, &[]);
        assert_eq!(styles[&div].opacity(), 1.0);
    }

    #[test]
    fn opacity_reads_a_declared_value() {
        let mut doc = Document::new();
        let root = doc.root();
        let div = elem(&mut doc, root, "div", &[("style", "opacity: 0.5;")]);
        let styles = cascade(&doc, &[]);
        assert_eq!(styles[&div].opacity(), 0.5);
    }

    #[test]
    fn opacity_clamps_to_the_valid_zero_to_one_range() {
        let mut doc = Document::new();
        let root = doc.root();
        let over = elem(&mut doc, root, "div", &[("style", "opacity: 2;")]);
        let under = elem(&mut doc, root, "div", &[("style", "opacity: -1;")]);
        let styles = cascade(&doc, &[]);
        assert_eq!(styles[&over].opacity(), 1.0);
        assert_eq!(styles[&under].opacity(), 0.0);
    }

    #[test]
    fn opacity_is_not_inherited() {
        let mut doc = Document::new();
        let root = doc.root();
        let parent = elem(&mut doc, root, "div", &[("style", "opacity: 0.3;")]);
        let child = doc.create_node(NodeData::Element {
            tag_name: "span".to_string(),
            attributes: vec![],
        });
        doc.append_child(parent, child);
        let styles = cascade(&doc, &[]);
        assert_eq!(
            styles[&child].opacity(),
            1.0,
            "opacity must not inherit from an ancestor"
        );
    }

    // ---- custom properties, var(), CSS-wide keywords, units, the rule index ----

    /// Cascades `css` against `html` and returns the style of the first element
    /// with the given tag.
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
            .unwrap();
        styles[&node].clone()
    }
    fn px(n: f64) -> Value {
        Value::Length(Length::Px(n))
    }

    #[test]
    fn var_substitutes_into_values_and_shorthands() {
        let style = style_of(
            "<p>x</p>",
            "p { --gap: 4px; --c: #102030; margin: var(--gap) 2px; color: var(--c); width: calc(var(--gap) * 3); }",
            "p",
        );
        assert_eq!(style.other["margin-top"], px(4.0));
        assert_eq!(style.other["margin-right"], px(2.0));
        assert_eq!(style.color, Color::Rgba(0x10, 0x20, 0x30, 255));
        assert!(matches!(style.other["width"], Value::Calc(_)));
    }

    #[test]
    fn elements_with_no_custom_properties_of_their_own_share_their_parents_table() {
        let html = "<div><p>a</p><section><p>b</p></section></div>";
        let css = "div { --x: 10px; --y: 2px; } section { --x: 30px; }";
        let doc = blueice_html::parse(html);
        let ua = ua_stylesheet();
        let author = parse(css);
        let styles = cascade(&doc, &[(Origin::Ua, &ua), (Origin::Author, &author)]);
        let by_tag = |tag: &str| {
            let mut ids: Vec<NodeId> = styles
                .keys()
                .copied()
                .filter(|n| matches!(doc.data(*n), NodeData::Element { tag_name, .. } if tag_name == tag))
                .collect();
            ids.sort_by_key(|n| n.as_u64());
            ids
        };
        let (div, ps, section) = (by_tag("div")[0], by_tag("p"), by_tag("section")[0]);
        assert!(
            std::sync::Arc::ptr_eq(&styles[&div].custom, &styles[&ps[0]].custom),
            "a child with nothing of its own reuses the table"
        );
        assert!(
            !std::sync::Arc::ptr_eq(&styles[&div].custom, &styles[&section].custom),
            "an element that sets one gets its own"
        );
        assert!(
            std::sync::Arc::ptr_eq(&styles[&section].custom, &styles[&ps[1]].custom),
            "and its children share that"
        );
        assert_eq!(styles[&section].custom["--x"], styles[&ps[1]].custom["--x"]);
        assert_eq!(styles[&div].custom.len(), 2);
    }

    #[test]
    fn custom_properties_inherit_and_can_be_overridden_per_element() {
        let html = "<div><p>a</p><section><p>b</p></section></div>";
        let css = "div { --x: 10px; } section { --x: 30px; } p { width: var(--x); }";
        let doc = blueice_html::parse(html);
        let ua = ua_stylesheet();
        let author = parse(css);
        let styles = cascade(&doc, &[(Origin::Ua, &ua), (Origin::Author, &author)]);
        let mut widths: Vec<(u64, Value)> = styles
            .iter()
            .filter(|(n, _)| matches!(doc.data(**n), NodeData::Element { tag_name, .. } if tag_name == "p"))
            .map(|(n, s)| (n.as_u64(), s.other["width"].clone()))
            .collect();
        widths.sort_by_key(|(id, _)| *id);
        assert_eq!(widths[0].1, px(10.0), "inherited from div");
        assert_eq!(widths[1].1, px(30.0), "section overrides for its subtree");
    }

    #[test]
    fn var_uses_its_fallback_and_an_unsatisfied_reference_drops_the_declaration() {
        let style = style_of(
            "<p>x</p>",
            "p { width: 7px; width: var(--nope, 9px); height: 5px; height: var(--nope); }",
            "p",
        );
        assert_eq!(style.other["width"], px(9.0), "fallback applies");
        assert_eq!(
            style.other["height"],
            px(5.0),
            "an invalid var() declaration leaves the earlier value standing... as if never declared"
        )
    }

    #[test]
    fn var_cycles_make_the_property_invalid() {
        let style = style_of(
            "<p>x</p>",
            "p { --a: var(--b); --b: var(--a); width: var(--a, 3px); }",
            "p",
        );
        assert_eq!(
            style.other["width"],
            px(3.0),
            "the cycle is invalid, so the fallback is used"
        );
    }

    #[test]
    fn important_and_specificity_apply_to_custom_properties() {
        let style = style_of(
            "<p id=a class=b>x</p>",
            "#a { --w: 1px; } .b { --w: 2px !important; } p { --w: 3px; } p { width: var(--w); }",
            "p",
        );
        assert_eq!(style.other["width"], px(2.0));
    }

    #[test]
    fn inherit_initial_and_unset() {
        let css = "div { color: #123456; width: 40px; visibility: hidden; } \
                   p { color: inherit; width: inherit; visibility: initial; }";
        let style = style_of("<div><p>x</p></div>", css, "p");
        assert_eq!(style.color, Color::Rgba(0x12, 0x34, 0x56, 255));
        assert_eq!(
            style.other["width"],
            px(40.0),
            "inherit copies a non-inherited property"
        );
        assert!(
            !style.other.contains_key("visibility"),
            "initial discards the inherited value"
        );
        let unset = style_of(
            "<div><p>x</p></div>",
            "div { color: red; width: 9px; } p { color: unset; width: unset; }",
            "p",
        );
        assert_eq!(
            unset.color,
            Color::Rgba(255, 0, 0, 255),
            "unset inherits an inherited property"
        );
        assert!(
            !unset.other.contains_key("width"),
            "unset resets a non-inherited one"
        );
    }

    #[test]
    fn visibility_and_other_text_properties_inherit() {
        let style = style_of(
            "<div><p>x</p></div>",
            "div { visibility: hidden; letter-spacing: 2px; white-space: nowrap; cursor: pointer; }",
            "p",
        );
        assert_eq!(
            style.other["visibility"],
            Value::Keyword("hidden".to_string())
        );
        assert_eq!(style.other["letter-spacing"], px(2.0));
        assert_eq!(
            style.other["white-space"],
            Value::Keyword("nowrap".to_string())
        );
    }

    #[test]
    fn font_size_resolves_rem_viewport_units_keywords_and_calc() {
        let rem = |css: &str| style_of("<html><body><p>x</p></body></html>", css, "p").font_size_px;
        assert_eq!(
            rem("html { font-size: 20px; } p { font-size: 2rem; }"),
            40.0
        );
        assert_eq!(
            rem("p { font-size: 1.5rem; }"),
            24.0,
            "default root is 16px"
        );
        assert_eq!(rem("p { font-size: 4vw; }"), 40.0, "1000px wide viewport");
        assert_eq!(rem("p { font-size: 10vh; }"), 50.0);
        assert_eq!(rem("p { font-size: large; }"), 18.0);
        assert_eq!(
            rem("body { font-size: 20px; } p { font-size: smaller; }"),
            20.0 / 1.2
        );
        assert_eq!(
            rem("body { font-size: 20px; } p { font-size: 150%; }"),
            30.0
        );
        assert_eq!(
            rem("body { font-size: 10px; } p { font-size: calc(1em + 2px); }"),
            12.0
        );
        assert_eq!(rem("body { font-size: 10px; } p { font-size: 2em; }"), 20.0);
    }

    #[test]
    fn rem_inside_the_root_font_size_uses_the_initial_size() {
        let doc = blueice_html::parse("<html><body>x</body></html>");
        let ua = ua_stylesheet();
        let author = parse("html { font-size: 2rem; }");
        let styles = cascade(&doc, &[(Origin::Ua, &ua), (Origin::Author, &author)]);
        let html = *styles
            .keys()
            .find(|n| doc.parent(**n) == Some(doc.root()))
            .unwrap();
        assert_eq!(
            styles[&html].font_size_px, 32.0,
            "rem on the root is relative to the 16px initial"
        );
    }

    #[test]
    fn indexed_matching_agrees_with_testing_every_rule() {
        // Many rules reachable only through different index buckets, plus
        // multi-selector rules and attribute/pseudo selectors the index cannot key.
        let css = "* { margin: 1px; } p { padding: 2px; } .a { color: red; } #x { width: 5px; } \
                   .a.b, #y, span { height: 6px; } [title] { opacity: 0.5; } :first-child { top: 1px; } \
                   div > .c { left: 3px; } .zz { right: 9px; }";
        let html =
            "<div><p id=x class='a b' title=t>1</p><span class=c>2</span><b id=y>3</b></div>";
        let doc = blueice_html::parse(html);
        let ua = ua_stylesheet();
        let author = parse(css);
        let styles = cascade(&doc, &[(Origin::Ua, &ua), (Origin::Author, &author)]);
        let get = |tag: &str| {
            let node = styles
                .keys()
                .copied()
                .find(|n| matches!(doc.data(*n), NodeData::Element { tag_name, .. } if tag_name == tag))
                .unwrap();
            styles[&node].clone()
        };
        let p = get("p");
        assert_eq!(p.other["margin-top"], px(1.0), "universal");
        assert_eq!(p.other["padding-top"], px(2.0), "type");
        assert_eq!(p.color, Color::Rgba(255, 0, 0, 255), "class");
        assert_eq!(p.other["width"], px(5.0), "id");
        assert_eq!(
            p.other["height"],
            px(6.0),
            "a multi-selector rule matched through its .a.b selector"
        );
        assert_eq!(p.other["opacity"], Value::Number(0.5), "attribute");
        assert_eq!(p.other["top"], px(1.0), "pseudo-class");
        assert!(!p.other.contains_key("right"));
        let span = get("span");
        assert_eq!(
            span.other["height"],
            px(6.0),
            "same rule through its type selector"
        );
        assert_eq!(
            span.other["left"],
            px(3.0),
            "descendant chain keyed on its rightmost class"
        );
        assert_eq!(get("b").other["height"], px(6.0), "same rule through #y");
    }
}
