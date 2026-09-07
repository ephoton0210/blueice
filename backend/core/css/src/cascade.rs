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
use crate::selector::{matches, specificity, Specificity};
use crate::value::{Color, Value};
use blueice_dom::{Document, NodeData, NodeId};
use std::collections::HashMap;

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
}

impl ComputedStyle {
    /// `font-weight: bold` or a numeric weight of 600 or above --
    /// centralized here (rather than duplicated in `blueice-paint` and
    /// `blueice-layout`, both of which need it: paint to pick a bold
    /// glyph, layout to measure text at the width that glyph will
    /// actually render at) since it's a real-CSS rule about a
    /// `ComputedStyle` value, not paint- or layout-specific logic.
    pub fn is_bold(&self) -> bool {
        matches!(self.other.get("font-weight"), Some(Value::Keyword(k)) if k == "bold") || matches!(self.other.get("font-weight"), Some(Value::Number(n)) if *n >= 600.0)
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
        }
    }
}

const INHERITED_PROPERTIES: &[&str] = &["color", "font-family", "font-size", "font-style", "line-height", "text-align"];

/// Properties real CSS inherits that don't have a dedicated
/// [`ComputedStyle`] field (unlike `color`/`font-family`/etc. above) --
/// handled generically by copying the parent's [`ComputedStyle::other`]
/// entry when the element itself doesn't set one, rather than adding a
/// bespoke field + fallback arm per property the way the others need.
const OTHER_INHERITED_PROPERTIES: &[&str] = &["font-weight"];

fn resolve_font_size(value: &Value, parent_font_size_px: f64) -> Option<f64> {
    match value {
        Value::Length(crate::value::Length::Px(n)) => Some(*n),
        Value::Length(crate::value::Length::Em(n)) => Some(n * parent_font_size_px),
        Value::Length(crate::value::Length::Zero) => Some(0.0),
        Value::Percentage(p) => Some(parent_font_size_px * p / 100.0),
        _ => None,
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

/// Cascades `stylesheets` (in ascending origin precedence) against
/// every element in `doc`, returning each element [`NodeId`]'s computed
/// style. Non-element nodes (the document node, text nodes) never
/// appear in the returned map.
pub fn cascade(doc: &Document, stylesheets: &[(Origin, &[Rule])]) -> HashMap<NodeId, ComputedStyle> {
    let mut styles = HashMap::new();
    cascade_subtree(doc, doc.root(), stylesheets, None, &mut styles);
    styles
}

fn cascade_subtree(
    doc: &Document,
    node: NodeId,
    stylesheets: &[(Origin, &[Rule])],
    parent_style: Option<&ComputedStyle>,
    styles: &mut HashMap<NodeId, ComputedStyle>,
) {
    let is_element = matches!(doc.data(node), NodeData::Element { .. });
    let own_style = if is_element {
        let style = compute_style_for(doc, node, stylesheets, parent_style);
        styles.insert(node, style.clone());
        Some(style)
    } else {
        parent_style.cloned()
    };

    for child in doc.children(node) {
        cascade_subtree(doc, child, stylesheets, own_style.as_ref(), styles);
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

fn compute_style_for(
    doc: &Document,
    node: NodeId,
    stylesheets: &[(Origin, &[Rule])],
    parent_style: Option<&ComputedStyle>,
) -> ComputedStyle {
    let mut matched: Vec<Matched> = Vec::new();
    for (origin, rules) in stylesheets {
        for (source_order, rule) in rules.iter().enumerate() {
            let best_specificity = rule.selectors.iter().filter(|s| matches(doc, node, s)).map(specificity).max();
            let Some(spec) = best_specificity else { continue };
            for decl in &rule.declarations {
                matched.push(Matched { origin: *origin, specificity: spec, source_order, declaration: decl });
            }
        }
    }

    // Inline `style="..."` (plan §"MVP CSS scope": part of the Author
    // origin, same as a `<style>` sheet). Per spec it beats *any*
    // selector-based author rule regardless of specificity, but is
    // still beaten by an `!important` declaration from any origin --
    // achieved here by giving it a specificity no real selector can
    // reach, rather than a new sort-key tier.
    let inline_declarations = inline_style_declarations(doc, node);
    for decl in &inline_declarations {
        matched.push(Matched { origin: Origin::Author, specificity: (u32::MAX, 0, 0), source_order: usize::MAX, declaration: decl });
    }

    matched.sort_by_key(|m| (m.declaration.important, m.origin, m.specificity, m.source_order));

    let mut winners: HashMap<&str, &Value> = HashMap::new();
    for m in &matched {
        winners.insert(m.declaration.property.as_str(), &m.declaration.value);
    }

    let default = ComputedStyle::initial();
    let parent = parent_style.unwrap_or(&default);
    let mut style = ComputedStyle::initial();

    style.font_size_px = winners
        .get("font-size")
        .and_then(|v| resolve_font_size(v, parent.font_size_px))
        .unwrap_or(parent.font_size_px);

    style.color = winners.get("color").and_then(|v| resolve_color(v, parent.color)).unwrap_or(parent.color);

    style.display = winners
        .get("display")
        .and_then(|v| if let Value::Keyword(k) = v { Some(k.clone()) } else { None })
        .unwrap_or_else(|| default.display.clone());

    style.font_family = winners
        .get("font-family")
        .and_then(|v| if let Value::Keyword(k) = v { Some(k.clone()) } else { None })
        .or_else(|| parent.font_family.clone());

    style.font_style = winners
        .get("font-style")
        .and_then(|v| if let Value::Keyword(k) = v { Some(k.clone()) } else { None })
        .or_else(|| parent.font_style.clone());

    style.text_align = winners
        .get("text-align")
        .and_then(|v| if let Value::Keyword(k) = v { Some(k.clone()) } else { None })
        .or_else(|| parent.text_align.clone());

    style.line_height = winners.get("line-height").map(|v| (*v).clone()).or_else(|| parent.line_height.clone());

    for (prop, value) in &winners {
        if INHERITED_PROPERTIES.contains(prop) || *prop == "display" {
            continue;
        }
        // currentColor can appear on any color-valued property
        // (background-color, border-*-color, ...), not just `color`
        // itself -- resolve it here against the color already computed
        // above, rather than leaving it unresolved in `other`.
        let resolved = match value {
            Value::Color(Color::CurrentColor) => Value::Color(style.color),
            other => (*other).clone(),
        };
        style.other.insert(prop.to_string(), resolved);
    }
    for prop in OTHER_INHERITED_PROPERTIES {
        if !style.other.contains_key(*prop) {
            if let Some(v) = parent.other.get(*prop) {
                style.other.insert(prop.to_string(), v.clone());
            }
        }
    }

    style
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
html, body, div, p, ul, ol, li, section, article, header, footer, nav,
main, aside, figure, figcaption, blockquote, pre, form, fieldset,
table, thead, tbody, tfoot, tr, td, th, caption, colgroup,
h1, h2, h3, h4, h5, h6 {
  display: block;
}
button, input, select, textarea {
  display: inline-block;
}
head, title, style, script, link, meta, colgroup {
  display: none;
}
b, strong {
  font-weight: bold;
}
i, em {
  font-style: italic;
}
h1 { font-size: 2em; font-weight: bold; }
h2 { font-size: 1.5em; font-weight: bold; }
h3 { font-size: 1.17em; font-weight: bold; }
h4 { font-size: 1em; font-weight: bold; }
h5 { font-size: 0.83em; font-weight: bold; }
h6 { font-size: 0.67em; font-weight: bold; }
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
            attributes: attrs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
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
        assert_eq!(styles[&p].color, Color::Rgba(0, 0, 255, 255), "author's rule wins even though UA's #id is more specific");
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
        assert_eq!(styles[&span].display, "inline", "span keeps the cascade's initial value, not div's display: none");
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
        assert_eq!(styles[&b].other.get("font-weight"), Some(&Value::Keyword("bold".to_string())));
        assert_eq!(styles[&span].other.get("font-weight"), Some(&Value::Keyword("bold".to_string())), "span must inherit bold from its <b> ancestor");
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
        assert_eq!(styles[&span].other.get("font-weight"), Some(&Value::Keyword("normal".to_string())));
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
        let author = parse("div { font-size: 10px; } span { font-size: 2em; } b { font-size: 2em; }");
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
        assert_eq!(styles[&p].other.get("background-color"), Some(&Value::Color(Color::Rgba(0, 128, 0, 255))));
    }

    #[test]
    fn non_inherited_properties_land_in_other_and_are_specified_not_computed() {
        let mut doc = Document::new();
        let root = doc.root();

        let p = elem(&mut doc, root, "p", &[]);
        let author = parse("p { width: 50%; margin: 10px; }");
        let styles = cascade(&doc, &[(Origin::Author, &author)]);
        assert_eq!(styles[&p].other.get("width"), Some(&Value::Percentage(50.0)));
        assert_eq!(styles[&p].other.get("margin-top"), Some(&Value::Length(Length::Px(10.0))));
    }

    #[test]
    fn non_matching_selector_contributes_nothing() {
        let mut doc = Document::new();
        let root = doc.root();

        let p = elem(&mut doc, root, "p", &[]);
        let author = parse("span { color: red; }");
        let styles = cascade(&doc, &[(Origin::Author, &author)]);
        assert_eq!(styles[&p].color, Color::Rgba(0, 0, 0, 255), "initial black, span's rule never matched");
    }

    #[test]
    fn text_node_is_never_a_key_in_the_computed_style_map() {
        let mut doc = Document::new();
        let text = doc.create_node(NodeData::Text { data: "hi".to_string() });
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
        assert_eq!(styles[&h1].font_size_px, 32.0, "2em against the initial 16px");
        assert_eq!(styles[&h1].other.get("font-weight"), Some(&Value::Keyword("bold".to_string())));
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
        let p = elem(&mut doc, root, "p", &[("style", "color: green !important;")]);
        let author = parse("p { color: red !important; }");
        let styles = cascade(&doc, &[(Origin::Author, &author)]);
        assert_eq!(styles[&p].color, Color::Rgba(0, 128, 0, 255), "later/more-specific tie broken in inline's favor");
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
        let child = doc.create_node(NodeData::Element { tag_name: "span".to_string(), attributes: vec![] });
        doc.append_child(parent, child);
        let styles = cascade(&doc, &[]);
        assert_eq!(styles[&child].opacity(), 1.0, "opacity must not inherit from an ancestor");
    }
}
