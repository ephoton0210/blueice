// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Named capture groups: a name is an identifier made of code points even
//! without the `u` flag (a literal astral character, `\u{...}` and a pair of
//! `\uXXXX` escapes spell the same name), and `\k<name>` for a name shared by
//! several groups refers to whichever of them took part in the match.

use blueice_bluejs::{compile, parse, Value, Vm};

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
fn astral_group_names_work_without_the_unicode_flag() {
    assert_true(
        r#"(function() {
          var text = "The quick brown fox jumped over the lazy dog's back";
          var m = text.match(/(?<𝑓𝑜𝑥>fox).*(?<𝓓𝓸𝓰>dog)/);
          if (m.groups["𝑓𝑜𝑥"] !== "fox" || m.groups["𝓓𝓸𝓰"] !== "dog") return "literal astral names";
          if (text.match(/(?<\u{1d4d1}\u{1d4fb}\u{1d4f8}\u{1d500}\u{1d4f7}>brown)/).groups["𝓑𝓻𝓸𝔀𝓷"] !== "brown") return "code point escapes";
          if (text.match(/(?<𝓑𝓻𝓸𝔀𝓷>brown)/).groups["𝓑𝓻𝓸𝔀𝓷"] !== "brown") return "surrogate escapes";
          if (text.match(/(?<the𝟚>the)/).groups["the𝟚"] !== "the") return "ID_Continue-only astral character after the start";
          // A backreference by astral name reaches the same group.
          var back = "It is a dog eat dog world.".match(/(?<𝓓𝓸𝓰>dog)(.*?)(\k<𝓓𝓸𝓰>)/);
          if (!back || back[3] !== "dog" || back[2] !== " eat ") return "backreference";
          // Astral text outside a name still matches code unit by code unit.
          if (/^.$/.test("𝑓") || !/^..$/.test("𝑓")) return "a non-Unicode dot is one code unit";
          // Inside a character class, `(?<` is not a group.
          if (!/^[(?<]+$/.test("(?<")) return "class is literal";
          // Without any named group, `\k<name>` is an identity escape followed by literal text.
          if (!/\k<𝑓>/.test("k<𝑓>")) return "Annex B \\k without groups";
          return true;
        })()"#,
    );
}

#[test]
fn a_reference_to_a_shared_group_name_matches_whichever_group_took_part() {
    assert_true(
        r#"(function() {
          var eq = (a, b) => a === b || (a && b && a.length === b.length && Array.from(a).every((x, i) => x === b[i]));
          var cases = [
            [/(?<x>a)|(?<x>b)/, "bab", ["b", undefined, "b"]],
            [/(?:(?<x>a)|(?<x>b))\k<x>/, "aa", ["aa", "a", undefined]],
            [/(?:(?<x>a)|(?<x>b))\k<x>/, "bb", ["bb", undefined, "b"]],
            [/(?:(?<x>a)|(?<x>b))\k<x>/, "abab", null],
            [/(?:(?<x>a)|(?<x>b))\k<x>/, "cdef", null],
            [/(?:(?:(?<x>a)|(?<x>b))\k<x>){2}/, "aabb", ["aabb", undefined, "b"]],
            [/(?:(?:(?<x>a)|(?<x>b))\k<x>){2}/, "abab", null],
            [/^(?:(?<a>x)|(?<a>y)|z)\k<a>$/, "xx", ["xx", "x", undefined]],
            [/^(?:(?<a>x)|(?<a>y)|z)\k<a>$/, "z", ["z", undefined, undefined]],
            [/^(?:(?<a>x)|(?<a>y)|z)\k<a>$/, "zz", null],
            [/(?<a>x)|(?:zy\k<a>)/, "zy", ["zy", undefined]],
            [/^(?:(?<a>x)|(?<a>y)|z){2}\k<a>$/, "xz", ["xz", undefined, undefined]],
            [/^(?:(?<a>x)|(?<a>y)|z){2}\k<a>$/, "xzx", null],
            [/(?:(?<x>a)|(?<x>b))\k<x>/u, "bb", ["bb", undefined, "b"]],
            [/(?:(?<x>a)|(?<x>B))\k<x>/i, "Bb", ["Bb", undefined, "B"]],
            [/(?:(?<x>a)|(?<x>b))\k<x>/, "bb", ["bb", undefined, "b"]],
          ];
          for (var [regexp, input, expected] of cases) {
            var actual = regexp.exec(input);
            if (!eq(actual, expected)) return regexp + " on " + input + " gave " + JSON.stringify(actual) + ", expected " + JSON.stringify(expected);
          }
          if (/(?:(?<x>a)|(?<x>b))\k<x>/.exec("bb").groups.x !== "b") return "groups.x";
          if (!/(?:(?<x>a)|(?<x>b))\k<x>/.test("aa") || /(?:(?<x>a)|(?<x>b))\k<x>/.test("ab")) return "test()";
          return true;
        })()"#,
    );
}
