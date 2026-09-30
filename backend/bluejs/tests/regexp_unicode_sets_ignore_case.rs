// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Case-insensitive matching under the `v` flag is the same as under `u`:
//! Unicode simple case folding, WordCharacters extended by U+017F and U+212A.
//! The matcher library only takes a pattern for a Unicode one when it is given
//! `u`, so a `v` pattern was folded through the upper case table like a
//! non-Unicode one (U+0131 matched `i`, the Kelvin sign did not match `k`, and
//! `[^\W]` was not the extended word set). Every expectation below was also
//! checked against V8.

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

#[test]
fn the_v_flag_folds_case_like_the_u_flag() {
    assert_true(
        r#"(function () {
  var failures = [];
  function cp(code) { return String.fromCharCode(code); }
  var LONG_S = cp(0x17F), DOTLESS_I = cp(0x131), KELVIN = cp(0x212A), SHARP_S = cp(0xDF), CAP_SHARP_S = cp(0x1E9E);
  var MICRO = cp(0xB5), GREEK_MU = cp(0x39C), FINAL_SIGMA = cp(0x3C2), SIGMA = cp(0x3A3);
  var ALPHA_YPOGEGRAMMENI = cp(0x1FB3), ALPHA_PROSGEGRAMMENI = cp(0x1FBC), DOTTED_I = cp(0x130);
  function name(text) {
    var out = "";
    for (var i = 0; i < text.length; i++) {
      var c = text.charCodeAt(i);
      out += c > 0x7e || c < 0x20 ? "\\u" + ("0000" + c.toString(16)).slice(-4) : text.charAt(i);
    }
    return out;
  }
  // The `u` and `v` flags fold the same way: through Unicode simple case folding.
  function table(pattern, rows) {
    ["iu", "iv"].forEach(function (flags) {
      rows.forEach(function (row) {
        var got;
        try { got = new RegExp(pattern, flags).test(row[0]); } catch (e) { got = e.name; }
        if (got !== row[1]) failures.push("/" + name(pattern) + "/" + flags + " on \"" + name(row[0]) + "\": " + got + " (wanted " + row[1] + ")");
      });
    });
  }
  // U+0131 has no simple case folding (only a Turkic one), U+017F, U+212A, U+1E9E and the like do.
  table("i", [["I", true], [DOTLESS_I, false], [DOTTED_I, false]]);
  table("I", [[DOTLESS_I, false]]);
  table(DOTLESS_I, [["i", false], ["I", false], [DOTLESS_I, true]]);
  table(DOTTED_I, [["i", false]]);
  table("k", [[KELVIN, true]]);
  table(KELVIN, [["k", true], ["K", true]]);
  table("s", [[LONG_S, true]]);
  table(LONG_S, [["S", true], ["s", true]]);
  table(SHARP_S, [[CAP_SHARP_S, true]]);
  table(CAP_SHARP_S, [[SHARP_S, true]]);
  table(MICRO, [[GREEK_MU, true]]);
  table(FINAL_SIGMA, [[SIGMA, true]]);
  table(ALPHA_YPOGEGRAMMENI, [[ALPHA_PROSGEGRAMMENI, true]]);
  table("[k]", [[KELVIN, true]]);
  table("[i]", [[DOTLESS_I, false]]);
  table("[" + DOTLESS_I + "]", [["i", false]]);
  table("[a-z]", [[DOTLESS_I, false], [LONG_S, true], [KELVIN, true]]);
  table("[" + ALPHA_YPOGEGRAMMENI + "]", [[ALPHA_PROSGEGRAMMENI, true]]);
  // A backreference compares folded code points too.
  table("(i)\\1", [["iI", true], ["i" + DOTLESS_I, false]]);
  table("(s)\\1", [["s" + LONG_S, true]]);
  // \w is extended by U+017F and U+212A (WordCharacters), so \W is not.
  table("\\w", [[LONG_S, true], [KELVIN, true]]);
  table("\\W", [[LONG_S, false], [KELVIN, false], ["-", true]]);
  table("[\\W]", [[LONG_S, false], [KELVIN, false], ["s", false], ["-", true]]);
  table("[^\\W]", [[LONG_S, true], [KELVIN, true], ["s", true], ["-", false]]);
  table("[^\\W_]", [[LONG_S, true], ["_", false], ["-", false]]);
  // Under v a class nests and \W may be an operand of a set operation.
  ["iv"].forEach(function (flags) {
    [["[[a]\\W]", [["a", true], ["-", true], [LONG_S, false]]],
     ["[[a-z]--\\W]", [["s", true], [LONG_S, true], ["-", false]]],
     ["[\\w&&\\W]", [["s", false], [LONG_S, false], ["-", false]]],
     ["[^[a]\\W]", [["b", true], ["a", false], ["-", false], [LONG_S, true]]],
     ["[[a]]\\W", [["a-", true], ["a" + LONG_S, false]]]].forEach(function (entry) {
      entry[1].forEach(function (row) {
        var got;
        try { got = new RegExp(entry[0], flags).test(row[0]); } catch (e) { got = e.name; }
        if (got !== row[1]) failures.push("/" + name(entry[0]) + "/" + flags + " on \"" + name(row[0]) + "\": " + got + " (wanted " + row[1] + ")");
      });
    });
  });
  return failures.length === 0 ? true : failures.join("\n");
})()
"#,
    );
}
