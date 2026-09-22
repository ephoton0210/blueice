// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `\u` escapes and `\W` in patterns, whose meaning depends on the `u` flag
//! (ECMA-262 RegExpUnicodeEscapeSequence and CharacterClassEscape): without
//! it `\u{41}` is `u{41}` and `\uD83D\uDC38` is two code-unit escapes; with it
//! a lead surrogate escape pairs with a following `\u` trail surrogate escape
//! and with nothing else.

use blueice_bluejs::{compile, parse, HeapConfig, Value, Vm, VmConfig};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

/// Expects `true`; a script may return a string describing the first failure.
fn assert_true(source: &str) {
    match evaluate(source) {
        Value::Bool(true) => {}
        Value::String(reason) => panic!("{}", reason.to_utf8().unwrap()),
        other => panic!("{source}\n  -> {other:?}"),
    }
}

/// `m` is the matched text of a pattern against a subject, or `null`.
const HELPERS: &str = r#"
    function m(pattern, flags, subject) {
      var r = new RegExp(pattern, flags).exec(subject);
      return r === null ? null : r[0];
    }
    function throwsSyntaxError(pattern, flags) {
      try { new RegExp(pattern, flags); return false; } catch (e) { return e instanceof SyntaxError; }
    }
"#;

fn check(body: &str) {
    assert_true(&format!(
        "(function() {{ {HELPERS} {body} return true; }})()"
    ));
}

#[test]
fn braced_unicode_escape_is_an_identity_escape_followed_by_text_without_the_u_flag() {
    check(
        r#"
        if (m("\\u{41}", "", "ABC" + "u".repeat(41)) !== "u".repeat(41)) return "quantifier";
        if (m("\\u{4A}", "", "JKLu{4A}") !== "u{4A}") return "literal braces";
        if (m("\\u{1F438}", "", "u{1F438}") !== "u{1F438}") return "astral spelling";
        if (m("\\u{41}", "", "A") !== null) return "was treated as an escape";
        if (m("[\\u{41}]", "", "4") !== "4" || m("[\\u{41}]", "", "A") !== null) return "in a class";
        // The u flag keeps the braced escape.
        if (m("\\u{41}", "u", "ABC") !== "A" || m("\\u{1F438}", "u", "\u{1F438}") !== "\u{1F438}") return "u flag";
        if (m("\\u{0000000000000000000000}", "u", "\0") !== "\0") return "leading zeros";
        "#,
    );
}

#[test]
fn surrogate_escapes_stay_separate_code_units_without_the_u_flag() {
    check(
        r#"
        var pair = "\u{1F438}";
        if (m("\\uD83D\\uDC38", "", pair) !== pair) return "plain pair";
        // A quantifier applies to the trail unit alone.
        if (m("\\uD83D\\uDC38?", "", pair) !== pair) return "? on a pair";
        if (m("\\uD83D\\uDC38?", "", "") !== null) return "? on an empty subject";
        if (m("\\uD83D\\uDC38?", "", "\uD83D") !== "\uD83D") return "? on a lone lead";
        if (m("\\uD83D\\uDC38+", "", "\uD83D\uDC38\uDC38") !== "\uD83D\uDC38\uDC38") return "+";
        if (m("\\uD83D\\uDC38*", "", "\uD83D") !== "\uD83D") return "* on a lone lead";
        if (m("\\uD83D\\uDC38*", "", "") !== null) return "* on an empty subject";
        // A class holds the two units separately.
        if (m("[\\uD83D\\uDC38]", "", pair) !== "\uD83D") return "class over a pair";
        if (m("[\\uD83D\\uDC38]", "", "\uDC38") !== "\uDC38") return "class over a trail";
        "#,
    );
}

#[test]
fn a_lead_surrogate_escape_pairs_only_with_a_trail_surrogate_hex4_escape_with_the_u_flag() {
    check(
        r#"
        var pair = "\u{1F438}";
        if (m("\\uD83D\\uDC38", "u", pair) !== pair) return "pair";
        if (m("\\uD83D\\uDC38+", "u", pair + pair) !== pair + pair) return "pair+";
        if (m("\\uD83D\\uDC38+", "u", "\uD83D\uDC38\uDC38") !== "\uD83D\uDC38") return "pair+ over a trailing trail";
        // Braced or non-surrogate neighbours do not pair: each stays a lone surrogate.
        if (m("\\uD83D\\u{DC38}+", "u", "\uD83D\uDC38\uDC38") !== null) return "braced trail";
        if (m("\\uD83D\\u3042*", "u", "\uD83D") !== "\uD83D") return "lead then BMP, empty";
        if (m("\\uD83D\\u3042*", "u", "\uD83D\u3042\u3042") !== "\uD83D\u3042\u3042") return "lead then BMP";
        if (m("\\uD83D\\u{3042}*", "u", "\uD83D\u3042\u3042") !== "\uD83D\u3042\u3042") return "lead then braced BMP";
        if (m("[\\uD83D\\u3042]*", "u", "\uD83D\u3042\u3042\uD83D") !== "\uD83D\u3042\u3042\uD83D") return "class of lead and BMP";
        if (m("[\\uD83D\\u{3042}]*", "u", "\uD83D\u3042\u3042\uD83D") !== "\uD83D\u3042\u3042\uD83D") return "class of lead and braced BMP";
        "#,
    );
}

#[test]
fn an_incomplete_unicode_escape_after_a_lead_surrogate_is_a_syntax_error_with_the_u_flag() {
    check(
        r#"
        for (var tail of ["\\u", "\\u0", "\\u00", "\\u000", "\\u000G", "\\u0.00"]) {
          if (!throwsSyntaxError("\\uD83D" + tail, "u")) return "outside a class: " + tail;
          if (!throwsSyntaxError("[\\uD83D" + tail + "]", "u")) return "inside a class: " + tail;
          // Without the flag an incomplete escape is an identity escape.
          if (throwsSyntaxError("\\uD83D" + tail, "")) return "no u flag: " + tail;
        }
        "#,
    );
}

#[test]
fn group_names_keep_their_surrogate_pair_escapes_without_the_u_flag() {
    // A name is a sequence of code points even without the flag, so a lead and
    // a trail escape spell one identifier character there.
    check(
        r#"
        var re = new RegExp("(?<\\ud835\\udc9c>x)\\k<\\ud835\\udc9c>", "");
        if (re.exec("xx").groups["\u{1d49c}"] !== "x") return "escaped pair name";
        if (m("(?<\\u{1d49c}>a)\\k<\\ud835\\udc9c>", "", "aa") !== "aa") return "braced and paired spellings";
        "#,
    );
}

#[test]
fn w_inside_a_class_excludes_the_extra_word_characters_of_unicode_ignore_case() {
    // With /iu the word characters are [A-Za-z0-9_] plus U+017F and U+212A, so
    // \W never matches their case variants s and k.
    check(
        r#"
        for (var c of ["S", "s", "\u017F", "K", "k", "\u212A", "_", "7"]) {
          if (m("[^\\W]", "iu", c) !== c) return "[^\\W] should match " + c;
          if (m("[\\W]", "iu", c) !== null) return "[\\W] should not match " + c;
          if (m("\\W", "iu", c) !== null) return "\\W should not match " + c;
          if (m("[^\\w]", "iu", c) !== null) return "[^\\w] should not match " + c;
        }
        for (var c of ["!", " ", "\u00E9", "\u{1F438}", "\u0180", "\u2129", "\u212B"]) {
          if (m("[\\W]", "iu", c) !== c) return "[\\W] should match " + c;
          if (m("[^\\W]", "iu", c) !== null) return "[^\\W] should not match " + c;
        }
        // Other class members and negation still combine.
        if (m("[\\Wa]", "iu", "A") !== "A") return "[\\Wa]";
        if (m("[^\\W_]", "iu", "_") !== null || m("[^\\W_]", "iu", "S") !== "S") return "[^\\W_]";
        // Without the i flag the case variants are not word characters.
        if (m("[\\W]", "u", "S") !== null) return "[\\W] with u only";
        "#,
    );
}

#[test]
fn braced_escape_with_millions_of_leading_zeros_is_matched_without_shipping_them() {
    // The matcher runs in a child process behind a size-limited request; the
    // zeros carry no information, so the pattern is sent without them.
    let mut vm = Vm::new(VmConfig {
        heap: HeapConfig {
            max_heap_bytes: 512 * 1024 * 1024,
            ..HeapConfig::default()
        },
        max_string_bytes: 64 * 1024 * 1024,
        ..VmConfig::default()
    })
    .unwrap();
    let source = r#"(function() {
      var zeros = "0".repeat(6 * 1024 * 1024);
      var literal = new RegExp("\\u{" + zeros + "1234}", "u").exec("\u{1234}");
      var inClass = new RegExp("[\\u{" + zeros + "41}]", "u").exec("A");
      var tooBig = "";
      try { new RegExp("\\u{" + zeros + "110000}", "u"); tooBig = "no error"; }
      catch (e) { tooBig = e instanceof SyntaxError; }
      return literal !== null && literal[0] === "\u{1234}" && inClass !== null && inClass[0] === "A" && tooBig === true;
    })()"#;
    assert_eq!(
        vm.execute(&compile(&parse(source).unwrap()).unwrap()),
        Ok(Value::Bool(true))
    );
}
