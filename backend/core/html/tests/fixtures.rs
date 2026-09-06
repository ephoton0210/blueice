// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Runs every fixture in the shared corpus
//! (`development/browser_core/testing/fixtures/`) that has a
//! `#document` section against `blueice_html::parse`'s public API.
//!
//! This is the whole point of `blueice-testing` as a shared interface
//! (see its module docs): adding a new DOM-shape fixture to that corpus
//! requires zero new Rust code here, and the exact same fixture files
//! will gain new sections (`#styles`, `#layout`, ...) other crates check
//! independently as those stages become real, per
//! `testing/TEST_PLAN.md`'s "Rendering-correctness fixtures".

use blueice_html::parse;
use blueice_testing::{dump_dom, fixtures_dir, load_fixtures};

#[test]
fn dom_shape_fixtures() {
    let fixtures = load_fixtures(&fixtures_dir());
    assert!(!fixtures.is_empty(), "fixture corpus should not be empty");

    let mut checked = 0;
    let mut failures = Vec::new();
    for fixture in &fixtures {
        let Some(expected) = fixture.document() else {
            continue; // not every fixture has to check DOM shape
        };
        checked += 1;
        let doc = parse(fixture.data());
        let actual = dump_dom(&doc);
        let actual = actual.strip_suffix('\n').unwrap_or(&actual);
        if actual != expected {
            failures.push(format!(
                "{}:\n--- input ---\n{}\n--- expected ---\n{}\n--- actual ---\n{}\n",
                fixture.name,
                fixture.data(),
                expected,
                actual
            ));
        }
    }

    assert!(checked > 0, "at least one fixture must have a #document section");
    assert!(failures.is_empty(), "{} fixture(s) mismatched:\n\n{}", failures.len(), failures.join("\n"));
}
