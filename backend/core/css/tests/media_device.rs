// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! What BlueIce says about itself as a device: a desktop window with a mouse, a browser
//! with scripts, no forced colours. Pages choose whole interfaces on these (a header that only
//! reveals itself for `(hover: hover) and (pointer: fine)`), so "unknown" is not an answer.

use blueice_css::{matches_media, MediaEnvironment};

fn yes(query: &str) {
    assert!(matches_media(query, &MediaEnvironment::default()), "{query} should match");
}

fn no(query: &str) {
    assert!(!matches_media(query, &MediaEnvironment::default()), "{query} should not match");
}

#[test]
fn a_mouse_is_a_fine_pointer_that_can_hover() {
    for query in [
        "(hover: hover)",
        "(hover)",
        "(pointer: fine)",
        "(pointer)",
        "(any-hover: hover)",
        "(any-hover)",
        "(any-pointer: fine)",
        "(any-pointer)",
        "(hover: hover) and (pointer: fine)",
        "(hover: hover) and (any-pointer: fine) and (min-width: 768px)",
        "not (hover: none)",
        "screen and (hover: hover)",
    ] {
        yes(query);
    }
    for query in [
        "(hover: none)",
        "(hover: on-demand)",
        "(pointer: coarse)",
        "(pointer: none)",
        "(any-hover: none)",
        "(any-pointer: coarse)",
        "(any-pointer: none)",
        "not (hover: hover)",
        "(hover: none), (pointer: coarse)",
    ] {
        no(query);
    }
}

#[test]
fn the_window_is_a_landscape_or_portrait_screen_browser_with_scripts() {
    yes("(orientation: landscape)");
    no("(orientation: portrait)");
    let tall = MediaEnvironment {
        width: 600.0,
        height: 900.0,
        ..MediaEnvironment::default()
    };
    assert!(matches_media("(orientation: portrait)", &tall));
    assert!(!matches_media("(orientation: landscape)", &tall));
    // A square window is portrait, as in browsers (height >= width).
    let square = MediaEnvironment {
        width: 800.0,
        height: 800.0,
        ..MediaEnvironment::default()
    };
    assert!(matches_media("(orientation: portrait)", &square));
    for query in [
        "(scripting: enabled)",
        "(update: fast)",
        "(forced-colors: none)",
        "(display-mode: browser)",
    ] {
        yes(query);
    }
    for query in [
        "(scripting: none)",
        "(scripting: initial-only)",
        "(update: slow)",
        "(update: none)",
        "(forced-colors: active)",
        "(display-mode: standalone)",
        "(display-mode: fullscreen)",
    ] {
        no(query);
    }
}

#[test]
fn a_feature_it_does_not_know_still_does_not_match() {
    no("(made-up-feature: yes)");
    no("(hover: sometimes)");
}
