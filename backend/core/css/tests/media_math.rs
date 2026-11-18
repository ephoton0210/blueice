// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A media range whose bound is `calc()` (MDN's `(width >= calc(1rem * 2 + (15rem + 2rem) * 2
//! + 31rem))`) is the same query as the pixel length it comes to.

use blueice_css::{matches_media, MediaEnvironment};

fn at(width: f64, query: &str) -> bool {
    matches_media(query, &MediaEnvironment { width, ..MediaEnvironment::default() })
}

#[test]
fn a_calc_bound_is_evaluated_like_a_length() {
    let wide = "(width >= calc(1rem * 2 + (15rem + 2rem) * 2 + 31rem))"; // 1072px
    assert!(at(1280.0, wide));
    assert!(at(1072.0, wide));
    assert!(!at(1000.0, wide));
    let narrow = "(width < calc(1rem * 2 + 15rem + 2rem + 31rem))"; // 800px
    assert!(!at(1280.0, narrow));
    assert!(at(700.0, narrow));
    assert!(at(900.0, "(min-width: 500px) and (width <= max(800px, 1000px))"));
}
