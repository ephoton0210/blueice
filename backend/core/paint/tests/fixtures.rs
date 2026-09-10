// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Runs every fixture in the shared corpus
//! (`development/browser_core/testing/fixtures/`) that has a `#paint`
//! section against `blueice_paint`'s public API, building the fragment
//! tree the same way `blueice-layout`'s own fixture test does. See
//! `dump_frame`'s doc comment: this exact expected output is reused
//! verbatim by `blueice-engine`'s end-to-end smoke test.

use blueice_css::{cascade, ua_stylesheet, Origin, Rule};
use blueice_layout::{layout, Constraints};
use blueice_paint::{dump_frame, paint};
use blueice_testing::{fixtures_dir, load_fixtures};

const VIEWPORT_WIDTH: f64 = 320.0;

#[test]
fn paint_command_fixtures() {
    let fixtures = load_fixtures(&fixtures_dir());
    assert!(!fixtures.is_empty(), "fixture corpus should not be empty");

    let ua = ua_stylesheet();
    let mut checked = 0;
    let mut failures = Vec::new();

    for fixture in &fixtures {
        let Some(expected) = fixture.section("paint") else {
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
        let frame = paint(&fragment, &styles);

        let actual = dump_frame(&frame);
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
        "at least one fixture must have a #paint section"
    );
    assert!(
        failures.is_empty(),
        "{} fixture(s) mismatched:\n\n{}",
        failures.len(),
        failures.join("\n")
    );
}
