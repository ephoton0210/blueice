// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_css::{matches_media, parse_with_environment, Color, MediaEnvironment, Value};

#[test]
fn native_preferences_match_discrete_and_boolean_media_features() {
    let light = MediaEnvironment::default();
    let dark = MediaEnvironment {
        dark: true,
        high_contrast: true,
        reduced_motion: true,
        ..light
    };
    for query in [
        "(prefers-color-scheme: dark)",
        "(prefers-contrast: more)",
        "(prefers-reduced-motion: reduce)",
        "(prefers-contrast)",
        "(prefers-reduced-motion)",
    ] {
        assert!(!matches_media(query, &light), "{query}");
        assert!(matches_media(query, &dark), "{query}");
    }
    for query in [
        "(prefers-color-scheme: light)",
        "(prefers-contrast: no-preference)",
        "(prefers-reduced-motion: no-preference)",
    ] {
        assert!(matches_media(query, &light), "{query}");
        assert!(!matches_media(query, &dark), "{query}");
    }
    assert!(matches_media("(prefers-color-scheme)", &light));
    assert!(matches_media(
        "SCREEN and (PREFERS-COLOR-SCHEME: DARK)",
        &dark
    ));
    assert!(!matches_media(
        "screen and (prefers-color-scheme: light) or (prefers-contrast: more)",
        &light
    ));
}

#[test]
fn logical_groups_and_unknown_features_follow_three_valued_media_logic() {
    let env = MediaEnvironment::default();
    for q in [
        "not (unsupported-feature: yes)",
        "not (prefers-color-scheme: sepia)",
        "(prefers-color-scheme: dark) and (unsupported: yes)",
        "(prefers-color-scheme: light) and (unsupported: yes)",
        "screen and",
        "only (prefers-color-scheme: light)",
    ] {
        assert!(!matches_media(q, &env), "{q}");
    }
    for q in [
        "(unsupported: yes) or (prefers-color-scheme: light)",
        "not ((prefers-color-scheme: dark) and (unsupported: yes))",
        "print, screen and (prefers-color-scheme: light)",
        "not print",
        "only screen and (prefers-color-scheme: light)",
        "(not (prefers-color-scheme: dark)) and (prefers-color-scheme: light)",
    ] {
        assert!(matches_media(q, &env), "{q}");
    }
    assert!(!matches_media("(prefers-color-scheme: light) or (prefers-contrast: more) and (prefers-reduced-motion: reduce)", &env), "Mixed operators require grouping");
}

#[test]
fn css_viewport_and_resolution_features_use_current_environment_units() {
    let env = MediaEnvironment {
        width: 400.0,
        height: 300.0,
        resolution: 2.0,
        ..Default::default()
    };
    for q in [
        "(min-width: 25em)",
        "(max-height: 300px)",
        "(resolution: 192dpi)",
        "(min-resolution: 2dppx)",
        "(width >= 400px)",
        "(200px < width <= 400px)",
        "(400px >= width > 200px)",
    ] {
        assert!(matches_media(q, &env), "{q}");
    }
    for q in [
        "(max-width: 399px)",
        "(width > 400px)",
        "(resolution: 1dppx)",
        "(200px < width > 100px)",
        "not (min-width: 10bogus)",
    ] {
        assert!(!matches_media(q, &env), "{q}");
    }
}

#[test]
fn matching_nested_rules_keep_source_order_and_unrelated_at_rules_stay_skipped() {
    let env = MediaEnvironment {
        dark: true,
        high_contrast: true,
        ..Default::default()
    };
    let sheet = parse_with_environment("p {color:red} @media (prefers-color-scheme:dark) { p {color:blue} @media (prefers-contrast:more) {p {color:green !important}} } @supports anything { p {color:black} } p {color:white}", &env);
    let colors: Vec<_> = sheet
        .rules
        .iter()
        .map(|r| r.declarations[0].value.clone())
        .collect();
    assert_eq!(
        colors,
        [
            Value::Color(Color::Rgba(255, 0, 0, 255)),
            Value::Color(Color::Rgba(0, 0, 255, 255)),
            Value::Color(Color::Rgba(0, 128, 0, 255)),
            Value::Color(Color::Rgba(255, 255, 255, 255))
        ]
    );
    assert!(sheet.rules[2].declarations[0].important);
    let light = parse_with_environment(
        "@media (prefers-color-scheme:dark){p{color:blue}} p{color:white}",
        &Default::default(),
    );
    assert_eq!(light.rules.len(), 1);
    assert_eq!(
        blueice_css::parse("@media screen {p{color:red}} p{color:blue}")
            .rules
            .len(),
        1,
        "Legacy parser contract stays available"
    );
}

#[test]
fn malformed_queries_deep_nesting_and_nonfinite_environments_are_bounded() {
    let env = MediaEnvironment::default();
    for q in [
        "",
        "(prefers-color-scheme:light",
        "(prefers-color-scheme:light) trailing",
        "not not (prefers-color-scheme:dark)",
        "(prefers-color-scheme:light) or garbage",
        "(prefers-color-scheme:light) or not not (prefers-color-scheme:dark)",
        "not (prefers-color-scheme:dark) and (prefers-color-scheme:light)",
        "(prefers-color-scheme:light) or (width:300px) (height:200px)",
        "screen and (unsupported: fn(a,screen,b))",
        "(width: NaNpx)",
    ] {
        assert!(!matches_media(q, &env), "{q}");
    }
    assert!(!matches_media(
        "screen",
        &MediaEnvironment {
            width: f64::NAN,
            ..env
        }
    ));
    let css = format!(
        "{}p{{color:red}}{} p{{color:blue}}",
        "@media screen{".repeat(1000),
        "}".repeat(1000)
    );
    let sheet = parse_with_environment(&css, &env);
    assert_eq!(sheet.rules.len(), 1);
    let q = format!(
        "{}(prefers-color-scheme: light){}",
        "(".repeat(1000),
        ")".repeat(1000)
    );
    assert!(!matches_media(&q, &env));
}
