// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Backreferences under the `u` and `v` flags compare code points, not code
//! units: a reference must never match half of a surrogate pair in the
//! subject (ECMA-262 BackreferenceMatcher on a list of code points). The
//! matcher library compares code units, so a captured lone lead surrogate
//! used to "match" the first half of `\uD834\uDC00` and leave the match
//! ending inside the pair (Test262 `staging/sm/RegExp/unicode-back-reference.js`).
//! Every expectation below was also checked against V8.

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
fn a_backreference_never_ends_inside_a_surrogate_pair_of_the_subject() {
    assert_true(
        r#"(function () {
  var L = "\uD834"; // a lead surrogate
  var T = "\uDC00"; // a trail surrogate; L + T is the single code point U+1D000
  var PAIR = L + T;
  function units(s) {
    var o = [];
    for (var i = 0; i < s.length; i++) o.push(s.charCodeAt(i).toString(16));
    return o.join(" ");
  }
  function show(r) {
    if (r === null) return "null";
    return "[" + Array.prototype.map.call(r, function (x) { return x === undefined ? "undefined" : units(x); }).join(" | ") + "] @" + r.index;
  }
  var failures = [];
  function expectMatch(label, pattern, flags, subject, whole, capture, index) {
    var r = new RegExp(pattern, flags).exec(subject);
    var got = show(r);
    var want = "[" + units(whole) + (capture === undefined ? "" : capture === null ? " | undefined" : " | " + units(capture)) + "] @" + (index || 0);
    if (got !== want) failures.push(label + ": " + got + " (wanted " + want + ")");
  }
  function expectNull(label, pattern, flags, subject) {
    var r = new RegExp(pattern, flags).exec(subject);
    if (r !== null) failures.push(label + ": matched " + show(r) + " (wanted null)");
  }

  // A backreference is compared code point by code point: a captured lone lead surrogate does not
  // match the first half of a surrogate pair, however many code units happen to be equal.
  ["u", "v", "ui", "vi"].forEach(function (flags) {
    var p = "foo(.+)bar\\1";
    expectNull("lead vs pair /" + flags, p, flags, "foo" + L + "bar" + L + T);
    expectMatch("lead vs lead /" + flags, p, flags, "foo" + L + "bar" + L + L, "foo" + L + "bar" + L, L);
    expectMatch("lead vs BMP /" + flags, p, flags, "foo" + L + "bar" + L + "A", "foo" + L + "bar" + L, L);
    expectMatch("lead at end /" + flags, p, flags, "foo" + L + "bar" + L, "foo" + L + "bar" + L, L);
    expectMatch("trail vs trail /" + flags, p, flags, "foo" + T + "bar" + T + T, "foo" + T + "bar" + T, T);
    expectMatch("trail vs lead /" + flags, p, flags, "foo" + T + "bar" + T + L, "foo" + T + "bar" + T, T);
    expectMatch("trail vs BMP /" + flags, p, flags, "foo" + T + "bar" + T + "A", "foo" + T + "bar" + T, T);
    expectMatch("BMP then lone trail /" + flags, p, flags, "fooAbarA" + T, "fooAbarA", "A");
    expectMatch("BMP then lone lead /" + flags, p, flags, "fooAbarA" + L, "fooAbarA", "A");
    // A whole pair is an ordinary backreference target.
    expectMatch("pair vs pair /" + flags, p, flags, "foo" + PAIR + "bar" + PAIR, "foo" + PAIR + "bar" + PAIR, PAIR);
    expectNull("odd length /" + flags, "^(.+)\\1$", flags, T + "foobar" + PAIR + "foobar" + L);
    // The guard also applies to a quantified reference and to a named one.
    expectMatch("quantified /" + flags, "^(.)\\1+", flags, L + L + L + T, L + L, L);
    expectNull("single /" + flags, "^(.)\\1", flags, L + L + T);
    expectNull("named /" + flags, "(?<x>.+)bar\\k<x>", flags, L + "bar" + L + T);
    expectMatch("named ok /" + flags, "(?<x>.+)bar\\k<x>", flags, L + "bar" + L, L + "bar" + L, L);
    // A shared name refers to whichever group took part.
    expectNull("shared name /" + flags, "(?:(?<x>" + "\\uD834" + ")|(?<x>b))\\k<x>", flags, L + L + T);
    // A reference to a group that did not take part matches the empty string.
    expectMatch("unset /" + flags, "(a)?\\1b", flags, "b", "b", null);
    // Inside a lookbehind the reference is matched backwards.
    expectNull("lookbehind /" + flags, "(?<=\\1(.))x", flags, PAIR + T + "x");
    expectMatch("lookbehind ok /" + flags, "(?<=\\1(.))x", flags, "aax", "x", "a", 2);
  });

  // Without the u and v flags the subject is a sequence of code units: no pair exists to be split.
  expectMatch("non-unicode partial pair", "foo(.+)bar\\1", "", "foo" + L + "bar" + L + T, "foo" + L + "bar" + L, L);
  expectMatch("non-unicode ignore case partial pair", "foo(.+)bar\\1", "i", "foo" + L + "bar" + L + T, "foo" + L + "bar" + L, L);

  return failures.length === 0 ? true : failures.join("\n");
})()
"#,
    );
}
