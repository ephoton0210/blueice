// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `@font-face` rules and the family list that picks among them.

use blueice_css::{face_descriptors, font_faces, FontFaceRule, FontSource};

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
            local: None,
            format: Some("woff2".to_string()),
        }]
    );
    assert_eq!((face.weight_min, face.weight_max), (400, 400));
    assert!(!face.italic);
    assert!(face.unicode_ranges.is_empty());
}

#[test]
fn sources_come_in_order_with_their_formats_and_local_names() {
    let face = one(
        "@font-face { font-family: 'Inter'; src: local('Inter'), local(Inter-Regular), \
         url('a.woff2') format('woff2') tech(variations), url(b.woff) format('woff'), url(\"c.ttf\"), \
         url(d.eot?#iefix) format('embedded-opentype'); }",
    );
    let got: Vec<(&str, Option<&str>, Option<&str>)> = face
        .sources
        .iter()
        .map(|s| (s.url.as_str(), s.local.as_deref(), s.format.as_deref()))
        .collect();
    assert_eq!(
        got,
        [
            ("", Some("Inter"), None),
            ("", Some("Inter-Regular"), None),
            ("a.woff2", None, Some("woff2")),
            ("b.woff", None, Some("woff")),
            ("c.ttf", None, None),
            ("d.eot?#iefix", None, Some("embedded-opentype")),
        ]
    );
    assert_eq!(one("@font-face { font-family: L; src: local(\"Arial Bold\"); }").sources[0].local.as_deref(), Some("Arial Bold"));
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
    let names: Vec<&str> = faces.iter().map(|f| f.family.as_str()).collect();
    assert_eq!(names, ["c", "d"], "a rule with only local() sources is kept for the engine to try");
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

#[test]
fn descriptors_given_as_text_parse_like_those_of_a_rule() {
    use blueice_css::face_descriptors;
    let d = face_descriptors("300 700", "italic", "75% 125%", "U+0041-005A, U+61");
    assert_eq!((d.weight_min, d.weight_max, d.italic), (300, 700, true));
    assert_eq!((d.stretch_min, d.stretch_max), (750, 1250));
    assert_eq!(d.unicode_ranges, vec![(0x41, 0x5A), (0x61, 0x61)]);
    let d = face_descriptors("bold", "normal", "", "");
    assert_eq!((d.stretch_min, d.stretch_max), (1000, 1000));
    assert_eq!((d.weight_min, d.weight_max, d.italic), (700, 700, false));
    assert!(d.unicode_ranges.is_empty());
    let d = face_descriptors("", "", "", "");
    assert_eq!((d.weight_min, d.weight_max), (400, 400));
}

#[test]
fn the_stretch_descriptor_is_a_percentage_range_in_tenths() {
    let stretch = |decl: &str| {
        let f = one(&format!("@font-face {{ font-family: a; src: url(a.woff2); {decl} }}"));
        (f.stretch_min, f.stretch_max)
    };
    assert_eq!(stretch(""), (1000, 1000), "normal is 100%");
    assert_eq!(stretch("font-stretch: 75% 125%"), (750, 1250));
    assert_eq!(stretch("font-stretch: 62.5%"), (625, 625));
    assert_eq!(stretch("font-stretch: condensed"), (750, 750));
    assert_eq!(stretch("font-stretch: condensed expanded"), (750, 1250));
    assert_eq!(stretch("font-stretch: 150% 50%"), (500, 1500), "the ends are put in order");
}

#[test]
fn an_oblique_face_declares_the_angles_it_leans_through() {
    let slant = |style: &str| one(&format!("@font-face{{font-family:A;src:url(a.woff2);font-style:{style}}}")).slant;
    assert_eq!(slant("oblique 5deg 15deg"), Some((50, 150)));
    assert_eq!(slant("oblique 10deg"), Some((100, 100)), "one angle is both ends");
    assert_eq!(slant("oblique"), Some((140, 140)), "no angle is 14 degrees");
    assert_eq!(slant("oblique -10deg 20deg"), Some((-100, 200)));
    assert_eq!(slant("oblique 15deg 5deg"), Some((50, 150)), "the ends in either order");
    assert_eq!(slant("oblique 500deg"), Some((900, 900)), "clamped to the 90 degrees the spec allows");
    assert_eq!(slant("italic"), None);
    assert_eq!(slant("normal"), None);
    assert!(one("@font-face{font-family:A;src:url(a.woff2);font-style:oblique 5deg 15deg}").italic);
    let d = face_descriptors("", "oblique 5deg 15deg", "", "");
    assert_eq!((d.slant, d.italic), (Some((50, 150)), true));
    assert_eq!(face_descriptors("", "italic", "", "").slant, None);
}
