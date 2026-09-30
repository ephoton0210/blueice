// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Runs every fixture in the shared corpus
//! (`development/browser_core/testing/fixtures/`) that has a `#styles`
//! section against `blueice_css`'s public API (`parse`, `ua_stylesheet`,
//! `cascade`) plus `blueice_html::parse` for the `#data` HTML input.
//!
//! This is the concrete proof of `TEST_PLAN.md`'s "Rendering-correctness
//! fixtures" extensibility promise: `basic.dat`'s existing fixtures
//! (written for `blueice-html`'s own `#document` checks) gained a
//! `#styles` section with zero changes to `blueice-testing`, the
//! `#document` format, or `blueice-html`'s own fixture test -- adding a
//! pipeline stage's check to an existing fixture is just adding a
//! section.

use blueice_css::{cascade, ua_stylesheet, Color, ComputedStyle, Length, Origin, Rule, Value};
use blueice_dom::{Document, NodeData, NodeId};
use blueice_testing::{fixtures_dir, load_fixtures};
use std::collections::HashMap;

fn format_value(v: &Value) -> String {
    match v {
        Value::Keyword(s) => s.clone(),
        Value::Number(n) => format!("{n}"),
        Value::Percentage(n) => format!("{n}%"),
        Value::Length(Length::Px(n)) => format!("{n}px"),
        Value::Length(Length::Em(n)) => format!("{n}em"),
        Value::Length(Length::Zero) => "0".to_string(),
        Value::Color(Color::CurrentColor) => "currentcolor".to_string(),
        Value::Color(Color::Rgba(r, g, b, 255)) => format!("#{r:02x}{g:02x}{b:02x}"),
        Value::Color(Color::Rgba(r, g, b, a)) => format!("rgba({r},{g},{b},{a})"),
    }
}

fn format_color(c: Color) -> String {
    format_value(&Value::Color(c))
}

/// A canonical, whitespace-exact dump of every element's computed
/// style, in the same `| `-prefixed / 2-space-per-depth tree shape as
/// `blueice_testing::dump_dom`, so a fixture's `#document` and
/// `#styles` sections read consistently side by side. Unlike
/// `dump_dom`, text nodes are skipped entirely (styles are an
/// element-only concept) and only `display`/`color` (always resolved)
/// plus whatever's actually present in `other` (only ever the
/// declarations someone's CSS actually set) are printed -- so a
/// fixture only has to spell out what it cares about.
fn dump_styles(doc: &Document, styles: &HashMap<NodeId, ComputedStyle>) -> String {
    let mut out = String::new();
    for child in doc.children(doc.root()) {
        dump_styles_node(doc, child, styles, 0, &mut out);
    }
    out
}

fn dump_styles_node(
    doc: &Document,
    id: NodeId,
    styles: &HashMap<NodeId, ComputedStyle>,
    depth: usize,
    out: &mut String,
) {
    let NodeData::Element { tag_name, .. } = doc.data(id) else {
        return;
    };
    let indent = "  ".repeat(depth);
    out.push_str(&format!("| {indent}<{tag_name}>\n"));
    if let Some(style) = styles.get(&id) {
        let prop_indent = "  ".repeat(depth + 1);
        out.push_str(&format!("| {prop_indent}display={}\n", style.display));
        out.push_str(&format!(
            "| {prop_indent}color={}\n",
            format_color(style.color)
        ));
        let mut others: Vec<_> = style.other.iter().collect();
        others.sort_by_key(|(k, _)| (*k).clone());
        for (k, v) in others {
            out.push_str(&format!("| {prop_indent}{k}={}\n", format_value(v)));
        }
    }
    for child in doc.children(id) {
        dump_styles_node(doc, child, styles, depth + 1, out);
    }
}

#[test]
fn computed_style_fixtures() {
    let fixtures = load_fixtures(&fixtures_dir());
    assert!(!fixtures.is_empty(), "fixture corpus should not be empty");

    let ua = ua_stylesheet();
    let mut checked = 0;
    let mut failures = Vec::new();

    for fixture in &fixtures {
        let Some(expected) = fixture.section("styles") else {
            continue;
        };
        checked += 1;

        let doc = blueice_html::parse(fixture.data());
        let author: Vec<Rule> = fixture
            .section("css")
            .map(blueice_css::parse)
            .map(|s| s.rules)
            .unwrap_or_default();
        let sheets: Vec<(Origin, &[Rule])> = if author.is_empty() {
            vec![(Origin::Ua, &ua)]
        } else {
            vec![(Origin::Ua, &ua), (Origin::Author, &author)]
        };
        let styles = cascade(&doc, &sheets);

        let actual = dump_styles(&doc, &styles);
        let actual = actual.strip_suffix('\n').unwrap_or(&actual);
        if actual != expected {
            failures.push(format!(
                "{}:\n--- input ---\n{}\n--- css ---\n{}\n--- expected ---\n{}\n--- actual ---\n{}\n",
                fixture.name,
                fixture.data(),
                fixture.section("css").unwrap_or(""),
                expected,
                actual
            ));
        }
    }

    assert!(
        checked > 0,
        "at least one fixture must have a #styles section"
    );
    assert!(
        failures.is_empty(),
        "{} fixture(s) mismatched:\n\n{}",
        failures.len(),
        failures.join("\n")
    );
}
