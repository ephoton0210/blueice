// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! EscapeRegExpPattern: `source` escapes what a literal needs escaped, and
//! round-trips through a regular expression literal unchanged.
use blueice_bluejs::{compile, parse, Value, Vm};

fn check(sources: &[&str]) {
    for source in sources {
        let value = Vm::default()
            .execute(&compile(&parse(source).unwrap()).unwrap())
            .unwrap_or_else(|error| panic!("{source}: {error:?}"));
        assert_eq!(value, Value::Bool(true), "{source}");
    }
}

#[test]
fn a_solidus_inside_a_character_class_is_not_escaped() {
    check(&[
        r#"new RegExp("[/]").source === "[/]" && String(new RegExp("[/]")) === "/[/]/""#,
        r#"new RegExp("a[/]b/c").source === "a[/]b\\/c""#,
        // Already escaped stays escaped, in and out of a class.
        r#"new RegExp("[\\/]").source === "[\\/]" && new RegExp("\\/").source === "\\/""#,
        // A class ends at its first unescaped `]`.
        r#"new RegExp("[a]/").source === "[a]\\/""#,
        r#"new RegExp("[[/]", "").source === "[[/]" && new RegExp("[[a]]/", "v").source === "[[a]]\\/""#,
    ]);
}

#[test]
fn a_backslash_before_a_line_terminator_is_absorbed_into_its_escape() {
    check(&[
        r#"new RegExp("\\\n").source === "\\n""#,
        r#"new RegExp("\\\r").source === "\\r""#,
        r#"new RegExp("\\ ").source === "\\u2028""#,
        r#"new RegExp("\\ ").source === "\\u2029""#,
        // Unescaped terminators, and an escaped backslash before one.
        r#"new RegExp("\n").source === "\\n" && new RegExp("\\\\\n").source === "\\\\\\n""#,
    ]);
}

#[test]
fn source_round_trips_through_a_literal() {
    check(&[
        r#"var re = new RegExp("[/]\n/"); eval("/" + re.source + "/").source === re.source"#,
        r#"var re = new RegExp("\\\n[/\\]]"); eval("/" + re.source + "/").test("\n]")"#,
    ]);
}
