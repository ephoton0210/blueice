// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `@font-face` rules and the family list that picks among them.

use blueice_css::{font_faces, FontFaceRule, FontSource};

fn one(css: &str) -> FontFaceRule {
    let mut faces = font_faces(css);
    assert_eq!(faces.len(), 1, "{css}: {faces:?}");
    faces.remove(0)
}

#[test]
fn a_font_face_names_its_family_files_weight_and_style() {
    let face = one(
        "@font-face{font-family:FT Kunst Grotesk;src:url(/fonts/Kunst%20Grotesk%20Regular.woff2)format(\"woff2\");\
         font-weight:400;font-style:normal;font-display:swap}",
    );
    assert_eq!(face.family, "ft kunst grotesk");
    assert_eq!(
        face.sources,
        [FontSource {
            url: "/fonts/Kunst%20Grotesk%20Regular.woff2".to_string(),
            format: Some("woff2".to_string()),
        }]
    );
    assert_eq!((face.weight_min, face.weight_max), (400, 400));
    assert!(!face.italic);
    assert!(face.unicode_ranges.is_empty());
}

#[test]
fn sources_come_in_order_with_their_formats_and_local_sources_are_left_out() {
    let face = one(
        "@font-face { font-family: 'Inter'; src: local('Inter'), local(Inter-Regular), \
         url('a.woff2') format('woff2') tech(variations), url(b.woff) format('woff'), url(\"c.ttf\"), \
         url(d.eot?#iefix) format('embedded-opentype'); }",
    );
    let got: Vec<(&str, Option<&str>)> = face
        .sources
        .iter()
        .map(|s| (s.url.as_str(), s.format.as_deref()))
        .collect();
    assert_eq!(
        got,
        [
            ("a.woff2", Some("woff2")),
            ("b.woff", Some("woff")),
            ("c.ttf", None),
            ("d.eot?#iefix", Some("embedded-opentype")),
        ]
    );
    assert_eq!(face.family, "inter");
}

#[test]
fn weights_styles_and_ranges() {
    let w = |decl: &str| {
        let f = one(&format!("@font-face {{ font-family: a; src: url(x.woff2); {decl} }}"));
        (f.weight_min, f.weight_max, f.italic)
    };
    assert_eq!(w(""), (400, 400, false));
    assert_eq!(w("font-weight: bold"), (700, 700, false));
    assert_eq!(w("font-weight: normal"), (400, 400, false));
    assert_eq!(w("font-weight: 300 900"), (300, 900, false));
    assert_eq!(w("font-weight: 600; font-style: italic"), (600, 600, true));
    assert_eq!(w("font-style: oblique 10deg"), (400, 400, true));
}

#[test]
fn unicode_ranges() {
    let f = one(
        "@font-face { font-family: a; src: url(x.woff2); \
         unicode-range: U+0000-00FF, U+0131, U+0152-0153, U+2000-206F, U+4??; }",
    );
    assert_eq!(
        f.unicode_ranges,
        [(0x0, 0xFF), (0x131, 0x131), (0x152, 0x153), (0x2000, 0x206F), (0x400, 0x4FF)]
    );
}

#[test]
fn rules_without_a_family_or_a_usable_source_are_dropped_and_others_survive() {
    let faces = font_faces(
        "@font-face { src: url(a.woff2); }\
         @font-face { font-family: b; }\
         @font-face { font-family: c; src: local(c); }\
         @font-face { font-family: d; src: url(d.woff2); }\
         p { color: red }",
    );
    assert_eq!(faces.len(), 1);
    assert_eq!(faces[0].family, "d");
}

#[test]
fn faces_inside_layers_and_supports_blocks_are_found_and_media_is_left_alone() {
    let faces = font_faces(
        "@layer base { @font-face { font-family: a; src: url(a.woff2); } }\
         @supports (color: red) { @font-face { font-family: b; src: url(b.woff2); } }\
         @supports (unknown-feature: 1) { @font-face { font-family: e; src: url(e.woff2); } }\
         @media print { @font-face { font-family: c; src: url(c.woff2); } }\
         @font-face { font-family: d; src: url(d.woff2); }",
    );
    let names: Vec<&str> = faces.iter().map(|f| f.family.as_str()).collect();
    assert_eq!(names, ["a", "b", "d"]);
}

#[test]
fn the_family_list_keeps_every_name_for_fallback() {
    let rules = blueice_css::parse(
        r#"p { font-family: "Helvetica Neue", Apercu Mono Pro, system-ui, sans-serif; }"#,
    )
    .rules;
    assert_eq!(
        rules[0].declarations[0].value,
        blueice_css::Value::Keyword("helvetica neue,apercu mono pro,system-ui,sans-serif".to_string())
    );
}

#[test]
fn escaped_characters_in_selectors_do_not_hide_the_faces_that_follow() {
    // Tailwind's escaped class names: an escaped parenthesis or brace is a character, not
    // structure, and must not throw off what follows.
    let css = concat!(
        r".w-\[calc\(100\%-1px\)\]{width:1px}",
        r".has-\[\:not\(a\)\]\:x{color:red}",
        r".open-paren-\({color:blue}",
        r".brace-\{{color:green}",
        r".close-brace-\}{color:black}",
        r"@layer utilities{.a\(b{color:green}}",
        "@font-face{font-family:Late;src:url(late.woff2)}",
    );
    let faces = font_faces(css);
    assert_eq!(faces.len(), 1, "{faces:?}");
    assert_eq!(faces[0].family, "late");
}
