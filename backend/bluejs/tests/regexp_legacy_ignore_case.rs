// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Case-insensitive matching without the `u` and `v` flags (ECMA-262
//! Canonicalize for a non-Unicode `ignoreCase` pattern): a code unit is compared
//! through its upper case, except that a non-ASCII unit is never folded to an
//! ASCII one and a unit whose upper case is several units is not folded at all.
//! So U+017F (long s) and U+0131 (dotless i) match neither `s` nor `i`,
//! `/[a-z]/i` and `/\w/i` do not match them, and the Kelvin sign U+212A is not
//! `k`. The matcher library folds through the Unicode case-folding tables
//! instead (Test262 `staging/sm/RegExp/ignoreCase-non-latin1-to-latin1.js`
//! found it). Every expectation below was also checked against V8.

use blueice_bluejs::{compile, parse, Value, Vm};

/// Runs a script that returns `true`, or a string naming what went wrong.
fn assert_true(source: &str) {
    let value = Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"));
    match value {
        Value::Bool(true) => {}
        Value::String(reason) => panic!("{}", reason.to_utf8().unwrap()),
        other => panic!("{source}\n  -> {other:?}"),
    }
}

/// The script every test runs after `body`: it collects failures in `failures`.
fn check(body: &str) {
    assert_true(&format!(
        "(function() {{ {PRELUDE}\n{body}\nreturn failures.length === 0 ? true : failures.join('\\n'); }})()"
    ));
}

const PRELUDE: &str = r#"
  var failures = [];
  function cp(code) { return String.fromCharCode(code); }
  // Spells a code unit as a RegExp \u escape (or \x escape below 256) in a pattern's source text.
  function u(code) { return "\\u" + ("0000" + code.toString(16)).slice(-4); }
  function x(code) { return "\\x" + ("00" + code.toString(16)).slice(-2); }

  var LONG_S = cp(0x17F);      // U+017F, whose upper case is S
  var DOTLESS_I = cp(0x131);   // U+0131, whose upper case is I
  var KELVIN = cp(0x212A);     // U+212A, already upper case
  var SHARP_S = cp(0xDF);      // U+00DF, whose upper case is the two letters SS
  var CAP_SHARP_S = cp(0x1E9E);
  var MICRO = cp(0xB5);        // U+00B5, whose upper case is the Greek capital mu
  var GREEK_MU = cp(0x39C);
  var GREEK_SMALL_MU = cp(0x3BC);
  var Y_DIAERESIS = cp(0xFF);
  var CAP_Y_DIAERESIS = cp(0x178);
  var DZ_SMALL = cp(0x1C6), DZ_TITLE = cp(0x1C5), DZ_CAP = cp(0x1C4);
  var FINAL_SIGMA = cp(0x3C2), SIGMA = cp(0x3A3), SMALL_SIGMA = cp(0x3C3);
  var ALPHA_YPOGEGRAMMENI = cp(0x1FB3), ALPHA_PROSGEGRAMMENI = cp(0x1FBC);
  var COMBINING_YPOGEGRAMMENI = cp(0x345), SMALL_IOTA = cp(0x3B9);
  var NBSP = cp(0xA0);
  var LEAD = cp(0xD83D), TRAIL = cp(0xDE00);

  function verdict(pattern, flags, subject) {
    try { return new RegExp(pattern, flags).test(subject); } catch (e) { return e.name; }
  }
  function name(text) {
    var out = "";
    for (var i = 0; i < text.length; i++) {
      var c = text.charCodeAt(i);
      out += c > 0x7e || c < 0x20 ? "\\u" + ("0000" + c.toString(16)).slice(-4) : text.charAt(i);
    }
    return out;
  }
  // `rows` are [subject, expected] pairs for one pattern; each pattern in `patterns` is checked against all of them.
  function table(patterns, flags, rows) {
    [].concat(patterns).forEach(function (pattern) {
      rows.forEach(function (row) {
        var got = verdict(pattern, flags, row[0]);
        if (got !== row[1]) failures.push("/" + name(pattern) + "/" + flags + " on \"" + name(row[0]) + "\": " + got + " (wanted " + row[1] + ")");
      });
    });
  }
  function same(label, got, want) {
    if (got !== want) failures.push(label + ": " + name(String(got)) + " (wanted " + name(String(want)) + ")");
  }
"#;

/// A literal is compared through its upper case, unless that would make a non-ASCII
/// unit an ASCII one or is not one unit.
#[test]
fn literals_fold_through_the_upper_case_mapping_without_the_ascii_exclusion() {
    check(
        r#"
  // Without the u flag Canonicalize maps a code unit to its upper case unless that would turn a
  // non-ASCII unit into an ASCII one, so U+017F and U+0131 never match s and i in either direction.
  table(["s", "S", u(0x73), x(0x73), u(0x53), x(0x53), "\\u0053"], "i", [["s", true], ["S", true], [LONG_S, false]]);
  table([LONG_S, u(0x17f), "\\u017F"], "i", [[LONG_S, true], ["s", false], ["S", false]]);
  table(["i", "I", u(0x69), x(0x49)], "i", [["i", true], ["I", true], [DOTLESS_I, false]]);
  table([DOTLESS_I, u(0x131)], "i", [[DOTLESS_I, true], ["i", false], ["I", false]]);
  table("k", "i", [["K", true], [KELVIN, false]]);
  table([KELVIN, u(0x212a), "\\u212A"], "i", [[KELVIN, true], ["k", false], ["K", false]]);
  // An identity escape of a letter is that letter.
  table("\\a", "i", [["a", true], ["A", true], ["b", false]]);
  table("\\i", "i", [["i", true], ["I", true], [DOTLESS_I, false]]);
  table("\\S", "i", [[" ", false], ["s", true], [LONG_S, true]]);

  // Other upper/lower case pairs still fold, through the upper case mapping.
  table([MICRO, u(0xb5)], "i", [[MICRO, true], [GREEK_MU, true], [GREEK_SMALL_MU, true], ["u", false]]);
  table(GREEK_MU, "i", [[MICRO, true], [GREEK_SMALL_MU, true]]);
  table(Y_DIAERESIS, "i", [[CAP_Y_DIAERESIS, true], ["y", false], ["Y", false]]);
  table(x(0xff), "i", [[CAP_Y_DIAERESIS, true], [Y_DIAERESIS, true]]);
  table(CAP_Y_DIAERESIS, "i", [[Y_DIAERESIS, true]]);
  table([DZ_SMALL, DZ_TITLE, DZ_CAP], "i", [[DZ_SMALL, true], [DZ_TITLE, true], [DZ_CAP, true]]);
  table(FINAL_SIGMA, "i", [[SIGMA, true], [SMALL_SIGMA, true]]);
  table(COMBINING_YPOGEGRAMMENI, "i", [[SMALL_IOTA, true], [cp(0x399), true]]);
  // A unit whose upper case is more than one unit is not folded.
  table(SHARP_S, "i", [[SHARP_S, true], [CAP_SHARP_S, false], ["SS", false], ["ss", false]]);
  table(CAP_SHARP_S, "i", [[SHARP_S, false]]);
  table("SS", "i", [[SHARP_S, false], ["ss", true]]);
  table(ALPHA_YPOGEGRAMMENI, "i", [[ALPHA_YPOGEGRAMMENI, true], [ALPHA_PROSGEGRAMMENI, false]]);
  table(ALPHA_PROSGEGRAMMENI, "i", [[ALPHA_YPOGEGRAMMENI, false]]);
  // Case folding is off without the flag, and the u flag keeps Unicode simple case folding.
  table(LONG_S, "", [[LONG_S, true], ["s", false]]);
  table("s", "iu", [[LONG_S, true], ["S", true]]);
  table(LONG_S, "iu", [["s", true], ["S", true]]);
  table(KELVIN, "iu", [["k", true]]);
  table(SHARP_S, "iu", [[CAP_SHARP_S, true]]);
  table("[a-z]", "iu", [[LONG_S, true], [KELVIN, true]]);
        "#,
    );
}

/// A class matches what one of its members is canonically equal to, so neither
/// `[a-z]` nor `[^\W]` involves U+017F or U+212A.
#[test]
fn character_classes_hold_the_canonical_units_of_their_members() {
    check(
        r#"
  table("[a-z]", "i", [["s", true], ["S", true], ["Z", true], [LONG_S, false], [KELVIN, false], [DOTLESS_I, false], ["0", false]]);
  table("[A-Z]", "i", [["s", true], [LONG_S, false], [KELVIN, false]]);
  table("[s]", "i", [["S", true], [LONG_S, false]]);
  table("[i]", "i", [["I", true], [DOTLESS_I, false]]);
  table("[k]", "i", [["K", true], [KELVIN, false]]);
  table(["[" + LONG_S + "]", "[" + u(0x17f) + "]"], "i", [[LONG_S, true], ["s", false], ["S", false]]);
  table("[" + DOTLESS_I + "]", "i", [[DOTLESS_I, true], ["i", false], ["I", false]]);
  table("[" + KELVIN + "]", "i", [[KELVIN, true], ["k", false], ["K", false]]);
  table("[" + SHARP_S + "]", "i", [[SHARP_S, true], [CAP_SHARP_S, false]]);
  table("[" + CAP_SHARP_S + "]", "i", [[SHARP_S, false], [CAP_SHARP_S, true]]);
  table("[" + ALPHA_PROSGEGRAMMENI + "]", "i", [[ALPHA_YPOGEGRAMMENI, false], [ALPHA_PROSGEGRAMMENI, true]]);
  table("[" + MICRO + "]", "i", [[GREEK_MU, true], [GREEK_SMALL_MU, true]]);
  table("[" + DZ_TITLE + "]", "i", [[DZ_SMALL, true], [DZ_CAP, true]]);
  // A range covering a folding unit folds it, and covers what is folded to.
  table("[" + u(0x100) + "-" + u(0x17f) + "]", "i", [["s", false], ["S", false], [LONG_S, true], [cp(0x100), true], [cp(0x101), true]]);
  table(["[" + u(0x41) + "-" + u(0x5a) + "]", "[" + x(0x41) + "-" + x(0x5a) + "]"], "i", [["q", true], ["Q", true], [LONG_S, false]]);
  // Negation applies to the folded set.
  table("[^a-z]", "i", [["s", false], ["S", false], [LONG_S, true], [KELVIN, true], ["0", true]]);
  table("[^s]", "i", [["S", false], [LONG_S, true], ["t", true]]);
  table("[^" + LONG_S + "]", "i", [["s", true], [LONG_S, false]]);
  table("[^" + KELVIN + "]", "i", [["k", true], [KELVIN, false]]);
  table("[^]", "i", [["s", true], [LONG_S, true], ["\n", true]]);
  table("[]", "i", [["s", false], ["", false]]);
  // Class escapes: \w is the ASCII word characters (also under i), \W the rest.
  table("[\\W]", "i", [["s", false], ["S", false], ["k", false], ["_", false], ["-", true], [LONG_S, true], [KELVIN, true]]);
  table("[^\\W]", "i", [["s", true], ["_", true], ["-", false], [LONG_S, false], [KELVIN, false]]);
  table("[\\w]", "i", [["s", true], [LONG_S, false], [KELVIN, false]]);
  table("[^\\w]", "i", [["s", false], [LONG_S, true]]);
  table("[\\D]", "i", [["5", false], ["s", true], [LONG_S, true]]);
  table("[\\S]", "i", [[" ", false], ["s", true], [LONG_S, true]]);
  table("[\\s]", "i", [[" ", true], [NBSP, true], ["s", false]]);
  table("[\\d\\s_]", "i", [["5", true], [" ", true], ["_", true], ["a", false]]);
  table("[a-c\\d]", "i", [["B", true], ["7", true], ["d", false]]);
  // A range whose end is a class escape is a union with a literal hyphen.
  table("[\\d-x]", "i", [["-", true], ["X", true], ["5", true], ["y", false]]);
  table("[a-\\d]", "i", [["-", true], ["A", true], ["5", true], ["b", false]]);
  table("[a\\-z]", "i", [["-", true], ["A", true], ["b", false]]);
  table("[a-]", "i", [["-", true], ["A", true]]);
  table("[-a]", "i", [["-", true], ["A", true]]);
  // The range U+002D-U+0061 holds `B`, so it matches `b`, but not `{`.
  table("[--a]", "i", [["-", true], ["A", true], ["0", true], ["b", true], ["{", false]]);
  table("[\\b]", "i", [["\b", true], ["b", false]]);
  table(["[\\x73]", "[\\u0073]", "[\\163]"], "i", [["S", true], [LONG_S, false]]);
  table("[\\\\]", "i", [["\\", true]]);
  table("[\\]]", "i", [["]", true]]);
  table("[a\\]b]", "i", [["]", true], ["B", true]]);
  table("[\\^]", "i", [["^", true]]);
  table("[[]", "i", [["[", true]]);
  table("[\\n\\t]", "i", [["\n", true], ["\t", true]]);
  table("[\\0]", "i", [["\0", true]]);
  table("[\\8]", "i", [["8", true]]);
  table("[\\cJ\\cj]", "i", [["\n", true]]);
        "#,
    );
}

/// References compare canonical text; assertions and escapes that do not depend on
/// case mean what they always did.
#[test]
fn backreferences_assertions_and_escapes_follow_the_canonical_text() {
    check(
        r#"
  table("(s)\\1", "i", [["sS", true], ["ss", true], ["s" + LONG_S, false], [LONG_S + "s", false]]);
  table("(" + LONG_S + ")\\1", "i", [[LONG_S + LONG_S, true], [LONG_S + "s", false], ["s" + LONG_S, false]]);
  table("(" + MICRO + ")\\1", "i", [[MICRO + GREEK_MU, true], [MICRO + GREEK_SMALL_MU, true]]);
  table("(?<x>s)\\k<x>", "i", [["sS", true], ["s" + LONG_S, false]]);
  table("(?<x>s)\\k<x>", "", [["ss", true], ["sS", false]]);
  table("(s|" + LONG_S + ")\\1", "i", [["sS", true], [LONG_S + LONG_S, true], [LONG_S + "s", false]]);
  table("\\w", "i", [["s", true], [LONG_S, false], [KELVIN, false]]);
  table("\\W", "i", [["s", false], [LONG_S, true], [KELVIN, true], ["-", true]]);
  table("\\bs", "i", [[LONG_S + "s", true], ["as", false]]);
  table("s\\b", "i", [["s" + LONG_S, true], ["sa", false]]);
  table("\\Bs", "i", [["as", true], [LONG_S + "s", false]]);
  table("(?=s)s", "i", [["S", true], [LONG_S, false]]);
  table("(?!s)\\w", "i", [["S", false], ["t", true], [LONG_S, false]]);
  table("(?<=s)t", "i", [["St", true], [LONG_S + "t", false]]);
  table("(?<!s)t", "i", [["St", false], [LONG_S + "t", true]]);
  table("^s$", "i", [["S", true], [LONG_S, false]]);
  table(".", "i", [[LONG_S, true], ["\n", false]]);
  table("s+", "i", [["SsS", true], [LONG_S, false]]);
  table("^(?:s|t){3}$", "i", [["StS", true], ["S" + LONG_S + "t", false]]);
  table("^s{2,3}?$", "i", [["SS", true], ["S" + LONG_S, false]]);
  table("a|" + LONG_S, "i", [["A", true], [LONG_S, true], ["s", false]]);
  table("x\\.y", "i", [["X.Y", true], ["XaY", false]]);
  table("\\/", "i", [["/", true]]);
  table(["\\cJ", "\\cj"], "i", [["\n", true]]);
  table("\\0", "i", [["\0", true]]);
  table("\\t\\n\\v\\f\\r", "i", [["\t\n\v\f\r", true]]);
        "#,
    );
}

/// Legacy octal escapes, `\c` without a letter and the other Annex B forms.
#[test]
fn annex_b_syntax_is_canonicalized_like_the_rest() {
    check(
        r#"
  table("a{", "i", [["A{", true]]);
  table("a{1", "i", [["A{1", true]]);
  table("x{,5}", "i", [["X{,5}", true]]);
  table("]", "i", [["]", true]]);
  table("}", "i", [["}", true]]);
  // `\c` without a letter is a backslash, then a `c`.
  table("\\c", "i", [["\\c", true], ["\\C", true]]);
  table("\\c1", "i", [["\\C1", true], ["\x11", false]]);
  table("[\\c1]", "i", [["\x11", true], ["1", false]]);
  table("[\\c_]", "i", [["\x1f", true]]);
  table("[\\c]", "i", [["\\", true], ["C", true], ["c", true]]);
  // Legacy octal escapes, and `\8` and `\9`.
  table("\\1", "i", [["\x01", true]]);
  table("[\\1]", "i", [["\x01", true]]);
  table("(a)\\2", "i", [["a\x02", true]]);
  table("\\101", "i", [["A", true], ["a", true], ["b", false]]);
  table("\\151", "i", [["I", true], [DOTLESS_I, false]]);
  table("\\163", "i", [["S", true], [LONG_S, false]]);
  table("\\0101", "i", [["\x08" + "1", true]]);
  table("\\08", "i", [["\x008", true]]);
  table("\\8", "i", [["8", true]]);
  table("\\u{41}", "i", [["U".repeat(41), true], ["A", false]]);
  table("\\p{L}", "i", [["P{l}", true], ["a", false]]);
  table("\\u", "i", [["U", true]]);
  table("\\x", "i", [["X", true]]);
  table("\\xg", "i", [["XG", true]]);
  table("\\u00g", "i", [["U00G", true]]);
  table("\\k", "i", [["K", true]]);
  table("\\k<a>", "i", [["K<A>", true]]);
  // Modifiers change the flag inside a group; such patterns keep the matcher's own folding.
  table("(?-i:s)t", "i", [["sT", true], ["ST", false]]);
  table("(?i:s)t", "", [["St", true], ["ST", false]]);
        "#,
    );
}

/// The matcher runs on a canonical copy of the subject; what the methods return
/// comes from the subject itself.
#[test]
fn results_are_read_from_the_original_subject_by_every_regexp_method() {
    check(
        r#"
  var found = new RegExp("(" + GREEK_MU + ")", "i").exec("a" + MICRO + "b");
  same("match text", found && found[0], MICRO);
  same("capture text", found && found[1], MICRO);
  same("match index", found && found.index, 1);
  found = new RegExp("([a-z]+)", "i").exec("--xyZ--");
  same("class match", found && found[0], "xyZ");
  found = new RegExp("(s)(" + LONG_S + ")", "id").exec("aS" + LONG_S);
  same("indices", found && found.indices[2].join(), "2,3");
  same("match all", ("sSs" + LONG_S + "s").match(/s/gi).length, 4);
  same("match all indices", Array.from(("sSs" + LONG_S + "s").matchAll(/s/gi)).map(function (m) { return m.index; }).join(), "0,1,2,4");
  same("replace", ("aS" + LONG_S + "s").replace(/s/gi, "-"), "a-" + LONG_S + "-");
  same("replace with a callback", ("aS" + LONG_S + "s").replace(/s/gi, function (m) { return "[" + m + "]"; }), "a[S]" + LONG_S + "[s]");
  same("split", ("aS" + "b" + LONG_S + "c").split(/s/i).join("|"), "a|b" + LONG_S + "c");
  same("search", ("a" + LONG_S + "S").search(/s/i), 2);
  var re = new RegExp("s", "iy");
  re.lastIndex = 1;
  same("sticky", re.test("sS" + LONG_S), true);
  same("sticky index", re.lastIndex, 2);
  re.lastIndex = 2;
  same("sticky miss", re.test("sS" + LONG_S), false);
  re = /s/gi;
  var subject = "sSs" + LONG_S + "s";
  var indices = [];
  for (var m = re.exec(subject); m !== null; m = re.exec(subject)) indices.push(m.index + ":" + re.lastIndex);
  same("global exec", indices.join(), "0:1,1:2,2:3,4:5");
  found = new RegExp("(?<abc>s)(?<Def>t)", "i").exec("ST");
  same("group names keep their case", found && found.groups && (found.groups.abc + found.groups.Def), "ST");
  same("group name keys", Object.keys(found.groups).join(), "abc,Def");
  found = new RegExp("(?<" + LONG_S + ">s)", "i").exec("S");
  same("group named with a unit that folds", found && found.groups && found.groups[LONG_S], "S");

  // One regular expression over different subjects: the folded subject is not reused.
  re = /s/i;
  [["S", true], [LONG_S, false], ["S", true], ["s" + LONG_S, true], [LONG_S + LONG_S, false], [LONG_S + "S", true], ["", false], [LONG_S, false]].forEach(function (row) {
    same("reused on \"" + name(row[0]) + "\"", re.test(row[0]), row[1]);
  });
  var text = "xS" + LONG_S;
  re = /s/i;
  same("first exec", re.exec(text).index, 1);
  same("second exec of the same text", re.exec(text).index, 1);
  same("a different text of the same length", re.exec("x" + LONG_S + "S").index, 2);
        "#,
    );
}

/// Without the `u` flag a surrogate pair is two units.
#[test]
fn surrogate_units_are_folded_one_at_a_time() {
    check(
        r#"
  // Without the u flag a pair is two units, of which only the last is repeated.
  table(["\\uD83D\\uDE00", LEAD + TRAIL, "\\uD83D" + TRAIL, LEAD + "\\uDE00"], "i", [[LEAD + TRAIL, true], [LEAD, false], [TRAIL, false]]);
  table("\\uD83D\\uDE00+", "i", [[LEAD + TRAIL + TRAIL, true], [LEAD + LEAD + TRAIL, true], [LEAD, false]]);
  same("a repeated trail unit", new RegExp(LEAD + TRAIL + "+", "i").exec(LEAD + TRAIL + TRAIL)[0], LEAD + TRAIL + TRAIL);
  same("a repeated lead unit is not a pair", new RegExp("\\uD83D+", "i").exec(LEAD + LEAD + TRAIL)[0], LEAD + LEAD);
  table("[" + LEAD + TRAIL + "]", "i", [[LEAD, true], [TRAIL, true], [LEAD + TRAIL, true], ["a", false]]);
  table("[^" + LEAD + TRAIL + "]", "i", [[LEAD, false], [TRAIL, false], ["a", true]]);
  table(".", "i", [[LEAD, true]]);
  same("dot is one unit", new RegExp("^.$", "i").test(LEAD + TRAIL), false);
  same("dot with the u flag is one code point", new RegExp("^.$", "iu").test(LEAD + TRAIL), true);
        "#,
    );
}

/// Rewriting a pattern never turns an error into a match.
#[test]
fn invalid_patterns_are_still_syntax_errors() {
    check(
        r#"
  ["(", ")", "[", "[z-a]", "a{2,1}", "*", "+", "?", "a**", "(?<n>a)\\k<m>", "(?<n>a)(?<n>b)", "\\", "(?", "(?<", "(?<n", "(?<n>a", "(?:", "(?=a", "[\\c", "(?<n>a)\\k", "(?<n>a)\\k<n", "(?<1>a)"].forEach(function (pattern) {
    ["i", "im", "is", "iy", "ig"].forEach(function (flags) {
      if (verdict(pattern, flags, "") !== "SyntaxError") failures.push("/" + name(pattern) + "/" + flags + " should be a SyntaxError, was " + verdict(pattern, flags, ""));
    });
  });
        "#,
    );
}
