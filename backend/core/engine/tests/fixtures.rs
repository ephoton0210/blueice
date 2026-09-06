// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Phase 3's end-to-end smoke test: fixture HTML(+CSS) -> asserted
//! paint output, through `blueice_engine::render` -- the crate's real
//! public entry point, not the individual pipeline stages tested in
//! isolation by `blueice-html`/`blueice-css`/`blueice-layout`/
//! `blueice-paint`'s own fixture tests.
//!
//! Reuses the exact same `#paint` fixture sections and expected dump
//! format `blueice-paint`'s own fixture test checks (`dump_frame`) --
//! deliberately not a separate expectation, since the whole point of
//! this test is that `render()`'s output must be identical to calling
//! `parse`/`cascade`/`layout`/`paint` by hand in the right order.

use blueice_engine::render;
use blueice_paint::dump_frame;
use blueice_testing::{fixtures_dir, load_fixtures};

const VIEWPORT_WIDTH: f64 = 320.0;

#[test]
fn end_to_end_render_fixtures() {
    let fixtures = load_fixtures(&fixtures_dir());
    assert!(!fixtures.is_empty(), "fixture corpus should not be empty");

    let mut checked = 0;
    let mut failures = Vec::new();

    for fixture in &fixtures {
        let Some(expected) = fixture.section("paint") else { continue };
        checked += 1;

        let css = fixture.section("css").unwrap_or("");
        let frame = render(fixture.data(), css, VIEWPORT_WIDTH);

        let actual = dump_frame(&frame);
        let actual = actual.strip_suffix('\n').unwrap_or(&actual);
        if actual != expected {
            failures.push(format!(
                "{}:\n--- input ---\n{}\n--- css ---\n{}\n--- expected ---\n{}\n--- actual ---\n{}\n",
                fixture.name,
                fixture.data(),
                css,
                expected,
                actual
            ));
        }
    }

    assert!(checked > 0, "at least one fixture must have a #paint section");
    assert!(failures.is_empty(), "{} fixture(s) mismatched end to end through render():\n\n{}", failures.len(), failures.join("\n"));
}
