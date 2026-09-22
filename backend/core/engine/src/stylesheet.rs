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
//! `<link rel="stylesheet">` (a second network fetch, relative-URL
//! resolution) is explicitly out of scope for this pass -- named here
//! rather than silently missing, same as every other MVP cut in this
//! project.

use blueice_css::Rule;
use blueice_dom::{Document, NodeData, NodeId};

/// Walks `doc` for every `<style>` element and parses its text content
/// as CSS, in document order.
pub fn extract_inline_stylesheets(doc: &Document) -> Vec<Rule> {
    let mut rules = Vec::new();
    collect(doc, doc.root(), &mut rules);
    rules
}

fn collect(doc: &Document, node: NodeId, rules: &mut Vec<Rule>) {
    if let NodeData::Element { tag_name, .. } = doc.data(node) {
        if tag_name == "style" {
            let mut css_text = String::new();
            for child in doc.children(node) {
                if let NodeData::Text { data } = doc.data(child) {
                    css_text.push_str(data);
                }
            }
            rules.extend(blueice_css::parse(&css_text).rules);
        }
    }
    for child in doc.children(node) {
        collect(doc, child, rules);
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
}
