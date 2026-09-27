// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{parse, parse_module};

#[test]
fn parser_keeps_token_boundaries_for_regex_and_tagged_templates() {
    for source in [
        "  /x/;",
        "  tag`x`;",
        "tag`\\\r\nx`;",
        "tag`\r\nx`;",
        "<!-- comment\n/x/;",
    ] {
        assert!(parse(source).is_ok(), "{source:?}");
    }
    assert!(parse("tag`${").is_err());
}

#[test]
fn parser_rejects_escaped_identifier_errors_from_lexing() {
    assert!(parse("class C { #\\u0061; test() { return this.#a; } }").is_ok());
    for source in [
        "class C { #\\u0030; }",
        "class C { #a\\u0020; }",
        "class C { #\\u{xyz}; }",
        "class C { #\\u00zz; }",
        "let \\u0030 = 1;",
        "let \\u{xyz} = 1;",
    ] {
        assert!(parse(source).is_err(), "{source:?}");
    }
}

#[test]
fn html_comments_and_legacy_octal_follow_the_script_goal() {
    assert!(parse("'\\4';").is_ok());
    for source in ["<!-- comment\n1;", "--> comment\n1;"] {
        assert!(parse(source).is_ok(), "{source:?}");
        assert!(parse_module(source).is_err(), "{source:?}");
    }
}
