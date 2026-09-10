// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Runs every fixture in the shared corpus
//! (`development/browser_core/testing/fixtures/`) that has a `#layout`
//! section against `blueice_layout`'s public API, cascading with the UA
//! stylesheet (plus the fixture's own `#css`, if any) exactly the way
//! `blueice-css`'s own fixture test does.
//!
//! This is the concrete start of automated UI/visual verification,
//! integrated into the same shared test interface rather than a
//! separate mechanism: a geometry dump numerically pins down what the
//! engine would actually draw (box positions and sizes) before paint or
//! a real window exist to look at, and `#paint` extends the exact same
//! corpus the same way once `blueice-paint` is real. Real platform UI
//! automation (does a window actually appear, per `TEST_PLAN.md`'s "UI
//! testing strategy") still waits on Phase 4's frontend -- this is the
//! part of "UI testing" that doesn't have to.

use blueice_css::{cascade, ua_stylesheet, Origin, Rule};
use blueice_dom::{Document, NodeData};
use blueice_layout::{layout, Constraints, Fragment, FragmentKind};
use blueice_testing::{fixtures_dir, load_fixtures};

/// A fixed viewport width every `#layout` fixture is checked against,
/// so fixtures don't have to carry their own width and stay comparable
/// across the corpus -- 320px is deliberately narrow enough that
/// `phase-2-mvp-scope`'s "simple pages" corpus exercises real line-
/// wrapping, not just single-line paragraphs.
const VIEWPORT_WIDTH: f64 = 320.0;

fn fmt(n: f64) -> String {
    format!("{n:.1}")
}

fn dump_layout(doc: &Document, fragment: &Fragment, depth: usize, out: &mut String) {
    let indent = "  ".repeat(depth);
    let geometry = format!(
        "{},{} {}x{}",
        fmt(fragment.x),
        fmt(fragment.y),
        fmt(fragment.width),
        fmt(fragment.height)
    );
    match &fragment.kind {
        FragmentKind::Block => {
            let tag = fragment
                .node
                .map(|n| match doc.data(n) {
                    NodeData::Element { tag_name, .. } => tag_name.clone(),
                    _ => "?".to_string(),
                })
                .unwrap_or_else(|| "?".to_string());
            out.push_str(&format!("| {indent}<{tag}> block {geometry}\n"));
        }
        FragmentKind::Line => {
            out.push_str(&format!("| {indent}line {geometry}\n"));
        }
        FragmentKind::Text(text) => {
            out.push_str(&format!("| {indent}\"{text}\" {geometry}\n"));
        }
    }
    for child in &fragment.children {
        dump_layout(doc, child, depth + 1, out);
    }
}

#[test]
fn layout_geometry_fixtures() {
    let fixtures = load_fixtures(&fixtures_dir());
    assert!(!fixtures.is_empty(), "fixture corpus should not be empty");

    let ua = ua_stylesheet();
    let mut checked = 0;
    let mut failures = Vec::new();

    for fixture in &fixtures {
        let Some(expected) = fixture.section("layout") else {
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

        let fragment = layout(
            &doc,
            doc.root(),
            &styles,
            Constraints {
                available_width: VIEWPORT_WIDTH,
            },
        );
        let mut actual = String::new();
        dump_layout(&doc, &fragment, 0, &mut actual);
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
        "at least one fixture must have a #layout section"
    );
    assert!(
        failures.is_empty(),
        "{} fixture(s) mismatched:\n\n{}",
        failures.len(),
        failures.join("\n")
    );
}
