// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Extracts a page's own `<style>` tag content as author-origin CSS
//! rules -- a real gap found while wiring up Phase 4 navigation: real
//! fetched pages carry their CSS *inside* the HTML (`<style>` blocks),
//! not as a separately-supplied string the way `render()`'s own tests
//! and fixtures pass it. `blueice-html` already tokenizes `<style>` as
//! RAWTEXT (so its content lands as an ordinary text child, never
//! tree-constructed), but nothing previously read that text back out
//! and ran it through `blueice_css::parse`.
//!
//! `<link rel="stylesheet">` text is fetched by core before the document is
//! applied (`linked_styles.rs`, behind the same URL review as every other
//! request) and handed in as a [`LinkedStyles`] map keyed by the `href`
//! exactly as written; this walk merges each fetched sheet at the link's own
//! position, so source order between `<style>` and `<link>` is kept.

use blueice_css::Rule;
use blueice_dom::{Document, NodeData, NodeId};
use std::collections::HashMap;

/// Fetched stylesheet text by the `href` attribute as written.
pub type LinkedStyles = HashMap<String, String>;

/// Walks `doc` for every `<style>` element and parses its text content
/// as CSS, in document order.
pub fn extract_inline_stylesheets(doc: &Document) -> Vec<Rule> {
    let mut rules = Vec::new();
    collect(doc, doc.root(), &mut rules, None, &LinkedStyles::new());
    rules
}

/// `<style>` rules and fetched `<link rel=stylesheet>` rules, merged in
/// document order and filtered by each element's `media`.
pub fn extract_inline_stylesheets_with_linked(
    doc: &Document,
    environment: &blueice_css::MediaEnvironment,
    linked: &LinkedStyles,
) -> Vec<Rule> {
    let mut rules = Vec::new();
    collect(doc, doc.root(), &mut rules, Some(environment), linked);
    rules
}

/// The `href` of a `<link>` that asks for a stylesheet: its `rel` has the
/// `stylesheet` token and not `alternate`, and the `href` is not empty.
pub(crate) fn stylesheet_link_href(attributes: &[(String, String)]) -> Option<&str> {
    let attribute = |wanted: &str| {
        attributes
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(wanted))
            .map(|(_, value)| value.as_str())
    };
    let rel = attribute("rel")?;
    let has = |token: &str| {
        rel.split_ascii_whitespace()
            .any(|t| t.eq_ignore_ascii_case(token))
    };
    let href = attribute("href")?.trim();
    (has("stylesheet") && !has("alternate") && !href.is_empty()).then_some(href)
}

/// Every wanted stylesheet `href`, as written, once each, in document order.
pub(crate) fn stylesheet_link_hrefs(doc: &Document) -> Vec<String> {
    fn walk(doc: &Document, node: NodeId, out: &mut Vec<String>) {
        if let NodeData::Element {
            tag_name,
            attributes,
        } = doc.data(node)
        {
            if tag_name == "link" {
                if let Some(href) = stylesheet_link_href(attributes) {
                    if !out.iter().any(|seen| seen == href) {
                        out.push(href.to_string());
                    }
                }
            }
        }
        for child in doc.children(node) {
            walk(doc, child, out);
        }
    }
    let mut hrefs = Vec::new();
    walk(doc, doc.root(), &mut hrefs);
    hrefs
}

fn collect(
    doc: &Document,
    node: NodeId,
    rules: &mut Vec<Rule>,
    environment: Option<&blueice_css::MediaEnvironment>,
    linked: &LinkedStyles,
) {
    if let NodeData::Element {
        tag_name,
        attributes,
    } = doc.data(node)
    {
        let media_matches = environment.is_none_or(|env| {
            attributes
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case("media"))
                .is_none_or(|(_, value)| {
                    value.trim().is_empty() || blueice_css::matches_media(value, env)
                })
        });
        let parse_css = |css_text: &str| {
            environment
                .map_or_else(
                    || blueice_css::parse(css_text),
                    |env| blueice_css::parse_with_environment(css_text, env),
                )
                .rules
        };
        if tag_name == "style" && media_matches {
            let mut css_text = String::new();
            for child in doc.children(node) {
                if let NodeData::Text { data } = doc.data(child) {
                    css_text.push_str(data);
                }
            }
            rules.extend(parse_css(&css_text));
        } else if tag_name == "link" && media_matches {
            if let Some(css_text) =
                stylesheet_link_href(attributes).and_then(|href| linked.get(href))
            {
                rules.extend(parse_css(css_text));
            }
        }
    }
    for child in doc.children(node) {
        collect(doc, child, rules, environment, linked);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_rules_from_a_single_style_tag() {
        let doc = blueice_html::parse(
            "<html><head><style>p { color: red; }</style></head><body><p>x</p></body></html>",
        );
        let rules = extract_inline_stylesheets(&doc);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].declarations[0].property, "color");
    }

    #[test]
    fn extracts_and_concatenates_rules_from_multiple_style_tags_in_document_order() {
        let doc = blueice_html::parse(
            "<style>p { color: red; }</style><body><style>div { color: blue; }</style></body>",
        );
        let rules = extract_inline_stylesheets(&doc);
        assert_eq!(rules.len(), 2);
        assert_eq!(
            rules[0].declarations[0].value,
            blueice_css::Value::Color(blueice_css::Color::Rgba(255, 0, 0, 255))
        );
        assert_eq!(
            rules[1].declarations[0].value,
            blueice_css::Value::Color(blueice_css::Color::Rgba(0, 0, 255, 255))
        );
    }

    #[test]
    fn a_page_with_no_style_tag_yields_no_rules() {
        let doc = blueice_html::parse("<p>hi</p>");
        assert!(extract_inline_stylesheets(&doc).is_empty());
    }

    #[test]
    fn script_tag_content_is_never_mistaken_for_style_content() {
        // sanity check on the tree-walk itself: a <script> tag right
        // next to a <style> tag must not have its (RAWTEXT, non-CSS)
        // content merged in.
        let doc =
            blueice_html::parse("<script>var x = 1;</script><style>p { color: green; }</style>");
        let rules = extract_inline_stylesheets(&doc);
        assert_eq!(rules.len(), 1);
        assert_eq!(
            rules[0].declarations[0].value,
            blueice_css::Value::Color(blueice_css::Color::Rgba(0, 128, 0, 255))
        );
    }

    fn red() -> blueice_css::Value {
        blueice_css::Value::Color(blueice_css::Color::Rgba(255, 0, 0, 255))
    }
    fn blue() -> blueice_css::Value {
        blueice_css::Value::Color(blueice_css::Color::Rgba(0, 0, 255, 255))
    }
    fn linked(entries: &[(&str, &str)]) -> LinkedStyles {
        entries
            .iter()
            .map(|(href, css)| ((*href).to_string(), (*css).to_string()))
            .collect()
    }

    #[test]
    fn linked_stylesheets_interleave_with_style_tags_in_document_order() {
        let doc = blueice_html::parse(
            "<head><style>p{color:red}</style><link rel=stylesheet href=\"/a.css\">\
             <style>p{color:green}</style></head>",
        );
        let env = blueice_css::MediaEnvironment::default();
        let rules = extract_inline_stylesheets_with_linked(
            &doc,
            &env,
            &linked(&[("/a.css", "p{color:blue}")]),
        );
        let colors: Vec<_> = rules
            .iter()
            .map(|r| r.declarations[0].value.clone())
            .collect();
        assert_eq!(colors.len(), 3);
        assert_eq!(colors[0], red());
        assert_eq!(
            colors[1],
            blue(),
            "the link sits between the two style tags"
        );
    }

    #[test]
    fn a_link_is_a_stylesheet_only_by_rel_and_never_when_alternate_or_unfetched() {
        let doc = blueice_html::parse(
            "<link rel=\"preload stylesheet\" href=a.css>\
             <link rel=\"alternate stylesheet\" href=b.css>\
             <link rel=icon href=c.css>\
             <link rel=stylesheet href=missing.css>\
             <link rel=stylesheet>",
        );
        let env = blueice_css::MediaEnvironment::default();
        let sheets = linked(&[
            ("a.css", "p{color:red}"),
            ("b.css", "p{color:blue}"),
            ("c.css", "p{color:blue}"),
        ]);
        let rules = extract_inline_stylesheets_with_linked(&doc, &env, &sheets);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].declarations[0].value, red());
    }

    #[test]
    fn a_linked_stylesheet_honours_its_media_attribute() {
        let doc = blueice_html::parse(
            "<link rel=stylesheet href=print.css media=print>\
             <link rel=stylesheet href=screen.css media=screen>",
        );
        let env = blueice_css::MediaEnvironment::default();
        let sheets = linked(&[
            ("print.css", "p{color:blue}"),
            ("screen.css", "p{color:red}"),
        ]);
        let rules = extract_inline_stylesheets_with_linked(&doc, &env, &sheets);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].declarations[0].value, red());
    }

    #[test]
    fn stylesheet_link_hrefs_lists_each_wanted_href_once_in_order() {
        let doc = blueice_html::parse(
            "<link rel=stylesheet href=a.css><link rel=icon href=i.png>\
             <link rel=stylesheet href=b.css><link rel=stylesheet href=a.css>",
        );
        assert_eq!(stylesheet_link_hrefs(&doc), vec!["a.css", "b.css"]);
    }
}
