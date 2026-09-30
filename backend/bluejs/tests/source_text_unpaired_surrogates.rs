// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ECMAScript source text is a sequence of UTF-16 code units, so the strings
//! `eval`, `Function`, indirect eval and `ShadowRealm.prototype.evaluate`
//! compile may contain unpaired surrogates: legal inside comments, string
//! literals, template literals and regular-expression literals, a
//! `SyntaxError` only where an identifier or punctuator must be. The lexer
//! reads Rust text, which cannot hold a surrogate, so these tests pin the
//! whole path: source text in, string values, regular-expression patterns and
//! `Function.prototype.toString` text out, with every code unit intact -- and
//! that the private-use characters the lexer encoding uses never confuse a
//! genuine character with a surrogate.

use blueice_bluejs::{compile, parse, JsString, Value, Vm, VmConfig};

/// Runs `source` and expects it to evaluate to `true`; a string result is the
/// failure description the script produced itself.
fn assert_true(source: &str) {
    let program = parse(source).unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"));
    let value = Vm::default()
        .execute(&compile(&program).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"));
    match value {
        Value::Bool(true) => {}
        Value::String(reason) => panic!("{}", reason.to_utf8().unwrap_or_default()),
        other => panic!("{source}\n  -> {other:?}"),
    }
}

/// [`assert_true`] with a one-object nursery, so that every allocation the
/// native `eval`/`Function`/`toString` paths perform while decoding an
/// unpaired surrogate back out of lexer text may trigger a collection.
fn assert_true_under_gc_stress(source: &str) {
    let program = parse(source).unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"));
    let mut config = VmConfig::default();
    config.heap.nursery_capacity = 1;
    let value = Vm::new(config)
        .unwrap()
        .execute(&compile(&program).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"));
    match value {
        Value::Bool(true) => {}
        Value::String(reason) => panic!("{}", reason.to_utf8().unwrap_or_default()),
        other => panic!("{source}\n  -> {other:?}"),
    }
}

/// Every code unit that is not part of a pair when it stands alone.
const ALL_SURROGATES: &str = "var ALL = []; for (var u = 0xD800; u <= 0xDFFF; u++) ALL.push(u);";
/// A spread of them, for the checks that are costly per unit.
const SOME_SURROGATES: &str = "var ALL = [0xD800, 0xD83D, 0xDBFF, 0xDC00, 0xDC38, 0xDFFF];";

#[test]
fn comments_may_contain_any_unpaired_surrogate() {
    assert_true(&format!(
        r##"(function() {{
          {ALL_SURROGATES}
          for (var i = 0; i < ALL.length; i++) {{
            var c = String.fromCharCode(ALL[i]);
            var tag = ALL[i].toString(16);
            if (eval("40 + 2 // " + c) !== 42) return "line comment " + tag;
            if (eval("// " + c + "\n 7") !== 7) return "line comment before newline " + tag;
            if (eval("// " + c + "\u2028 7") !== 7) return "line comment before LS " + tag;
            if (eval("// " + c + "\u2029 7") !== 7) return "line comment before PS " + tag;
            if (eval("/* " + c + " */ 40 + 2") !== 42) return "block comment " + tag;
            if (eval("/*" + c + "\n" + c + "*/ 40 + 2") !== 42) return "multiline block comment " + tag;
            if (eval("40 + 2 <!-- " + c) !== 42) return "html open comment " + tag;
            if (eval("40 + 2\n--> " + c) !== 42) return "html close comment " + tag;
            if (eval("#!" + c + "\n7") !== 7) return "hashbang " + tag;
            var yy = 0;
            eval("//var " + c + "yy = -1");
            if (yy !== 0) return "the comment ended early " + tag;
          }}
          return true;
        }})()"##
    ));
}

#[test]
fn a_comment_is_ended_only_by_a_line_terminator() {
    assert_true(&format!(
        r#"(function() {{
          {SOME_SURROGATES}
          for (var i = 0; i < ALL.length; i++) {{
            var c = String.fromCharCode(ALL[i]);
            var yy = 0;
            eval("//var " + c + "yy = -1");
            if (yy !== 0) return "same line " + ALL[i].toString(16);
            eval("//" + c + "\nvar yy = -1");
            if (yy !== -1) return "next line " + ALL[i].toString(16);
          }}
          return true;
        }})()"#
    ));
}

#[test]
fn string_literals_hold_unpaired_surrogates_as_that_code_unit() {
    assert_true(&format!(
        r#"(function() {{
          {ALL_SURROGATES}
          for (var i = 0; i < ALL.length; i++) {{
            var unit = ALL[i];
            var c = String.fromCharCode(unit);
            var tag = unit.toString(16);
            var single = eval("'" + c + "'");
            if (typeof single !== "string" || single.length !== 1 || single.charCodeAt(0) !== unit)
              return "single-quoted " + tag;
            if (eval('"a' + c + 'b"') !== "a" + c + "b") return "double-quoted " + tag;
            // A NonEscapeCharacter: the backslash is dropped, the unit stays.
            if (eval("'\\" + c + "'") !== c) return "escaped " + tag;
            // A line continuation contributes nothing.
            if (eval("'" + c + "\\\n" + c + "'") !== c + c) return "continuation " + tag;
          }}
          return true;
        }})()"#
    ));
}

#[test]
fn a_raw_lead_and_a_trail_escape_form_the_pair_a_utf16_engine_would() {
    assert_true(
        r#"(function() {
          if (eval("'\uD83D\\uDC38'") !== "\u{1F438}") return "raw lead, escaped trail";
          if (eval("'\\uD83D\uDC38'") !== "\u{1F438}") return "escaped lead, raw trail";
          // Two separate literals are two separate strings, joined only by +.
          var joined = eval("'\uD83D' + '\uDC38'");
          if (joined !== "\u{1F438}" || joined.length !== 2) return "concatenated halves";
          // Raw units that are adjacent in the source are a pair, and a pair
          // is one character in a template literal too.
          if (eval("'\uD83D\uDC38'") !== "\u{1F438}") return "raw pair";
          if (eval("`\uD83D\uDC38`") !== "\u{1F438}") return "raw pair in template";
          return true;
        })()"#,
    );
}

#[test]
fn template_literals_hold_unpaired_surrogates_in_cooked_and_raw_values() {
    assert_true(&format!(
        r#"(function() {{
          {ALL_SURROGATES}
          function strings(s) {{ return s; }}
          for (var i = 0; i < ALL.length; i++) {{
            var unit = ALL[i];
            var c = String.fromCharCode(unit);
            var tag = unit.toString(16);
            var plain = eval("`" + c + "`");
            if (plain.length !== 1 || plain.charCodeAt(0) !== unit) return "plain " + tag;
            if (eval("`x" + c + "${{1}}" + c + "y`") !== "x" + c + "1" + c + "y")
              return "substitution " + tag;
            if (eval("`${{'" + c + "'}}`") !== c) return "unpaired surrogate in a placeholder " + tag;
            if (eval("`${{`" + c + "`}}`") !== c) return "nested template " + tag;
            var s = eval("strings`" + c + "${{0}}" + c + "`");
            if (s[0] !== c || s[1] !== c) return "cooked " + tag;
            if (s.raw[0] !== c || s.raw[1] !== c) return "raw " + tag;
            var e = eval("strings`\\" + c + "`");
            if (e[0] !== c) return "cooked NonEscapeCharacter " + tag;
            if (e.raw[0] !== "\\" + c) return "raw NonEscapeCharacter " + tag;
            var p = eval("strings`${{'" + c + "'}}`");
            if (p.length !== 2 || p.raw.length !== 2) return "placeholder " + tag;
          }}
          return true;
        }})()"#
    ));
}

#[test]
fn regular_expression_literals_match_an_unpaired_surrogate_as_that_code_unit() {
    assert_true(&format!(
        r#"(function() {{
          {ALL_SURROGATES}
          for (var i = 0; i < ALL.length; i++) {{
            var unit = ALL[i];
            var c = String.fromCharCode(unit);
            var tag = unit.toString(16);
            var subject = "a" + c + "b";
            var plain = eval("/" + c + "/");
            if (plain.source !== c || !plain.test(subject)) return "plain " + tag;
            var unicode = eval("/" + c + "/u");
            if (!unicode.test(subject)) return "unicode " + tag;
            if (unicode.exec(subject).index !== 1) return "unicode index " + tag;
            if (!eval("/[" + c + "]/").test(subject)) return "class " + tag;
            if (!eval("/[" + c + "]/u").test(subject)) return "unicode class " + tag;
            if (!eval("/[^" + c + "]/u").test("a")) return "negated class " + tag;
            if (eval("/[" + c + "]/u").test("a")) return "class matched something else " + tag;
            if (!eval("/" + c + "+/").test(c + c)) return "quantified " + tag;
            // Annex B identity escape without the `u` flag; a SyntaxError with it.
            if (!eval("/\\" + c + "/").test(subject)) return "identity escape " + tag;
            var threw = false;
            try {{ eval("/\\" + c + "/u"); }} catch (e) {{ threw = e instanceof SyntaxError; }}
            if (!threw) return "identity escape with u " + tag;
          }}
          return true;
        }})()"#
    ));
}

#[test]
fn a_raw_lead_in_a_regular_expression_never_matches_half_a_pair_with_the_u_flag() {
    assert_true(
        r#"(function() {
          var lead = "\uD83D", trail = "\uDC38", pair = "\u{1F438}";
          // Without `u` a pattern is a sequence of code units.
          if (!eval("/" + lead + "/").test(pair)) return "lead matches half";
          if (eval("/" + lead + "/").exec(pair)[0] !== lead) return "lead match text";
          if (!eval("/" + trail + "/").test(pair)) return "trail matches half";
          if (eval("/[" + lead + "]/").exec(pair)[0] !== lead) return "class lead match text";
          // With `u` it is a sequence of code points: a lone lead is not half a pair.
          if (eval("/" + lead + "/u").test(pair)) return "u lead";
          if (eval("/" + trail + "/u").test(pair)) return "u trail";
          if (eval("/[" + lead + "]/u").test(pair)) return "u class lead";
          if (!eval("/" + lead + "/u").test(lead)) return "u lead alone";
          // Raw units that are adjacent in the source are one code point.
          if (eval("/" + lead + trail + "/u").exec(pair)[0] !== pair) return "u pair";
          if (eval("/^[" + lead + trail + "]$/u").exec(pair) === null) return "u class pair";
          // An escaped half next to a raw half does not pair.
          if (eval("/\\uD83D" + trail + "/u").test(pair)) return "escaped lead, raw trail";
          if (eval("/" + lead + "\\uDC38/u").test(pair)) return "raw lead, escaped trail";
          return true;
        })()"#,
    );
}

#[test]
fn dynamic_function_source_may_contain_unpaired_surrogates() {
    assert_true(&format!(
        r#"(function() {{
          {ALL_SURROGATES}
          var Generator = Object.getPrototypeOf(function*() {{}}).constructor;
          for (var i = 0; i < ALL.length; i += 7) {{
            var unit = ALL[i];
            var c = String.fromCharCode(unit);
            var tag = unit.toString(16);
            if (Function("return '" + c + "'")() !== c) return "body string " + tag;
            if (Function("a = '" + c + "'", "return a")() !== c) return "parameter default " + tag;
            if (Function("// " + c + "\nreturn 3")() !== 3) return "body comment " + tag;
            if (Function("a /* " + c + " */", "return 1")() !== 1) return "parameter block comment " + tag;
            if (Function("a // " + c, "return 1")() !== 1) return "parameter line comment " + tag;
            if (Function("return `" + c + "${{1}}`")() !== c + "1") return "body template " + tag;
            if (!Function("return /" + c + "/u")().test(c)) return "body regexp " + tag;
            if (new Generator("yield '" + c + "'")().next().value !== c) return "generator " + tag;
          }}
          return true;
        }})()"#
    ));
}

#[test]
fn indirect_eval_and_shadow_realm_accept_unpaired_surrogates_in_literals() {
    assert_true(&format!(
        r#"(function() {{
          {SOME_SURROGATES}
          var realm = new ShadowRealm();
          var indirect = eval;
          for (var i = 0; i < ALL.length; i++) {{
            var c = String.fromCharCode(ALL[i]);
            var tag = ALL[i].toString(16);
            if (indirect("'" + c + "'") !== c) return "indirect eval " + tag;
            if (indirect("// " + c + "\n 7") !== 7) return "indirect eval comment " + tag;
            if (indirect("`" + c + "`") !== c) return "indirect eval template " + tag;
            if (!indirect("/" + c + "/u").test(c)) return "indirect eval regexp " + tag;
            if (realm.evaluate("'" + c + "'") !== c) return "ShadowRealm string " + tag;
            if (realm.evaluate("/* " + c + " */ 7") !== 7) return "ShadowRealm comment " + tag;
            if (realm.evaluate("`" + c + "`") !== c) return "ShadowRealm template " + tag;
          }}
          return true;
        }})()"#
    ));
}

#[test]
fn function_source_text_keeps_unpaired_surrogates_exactly() {
    assert_true(&format!(
        r#"(function() {{
          {SOME_SURROGATES}
          for (var i = 0; i < ALL.length; i++) {{
            var c = String.fromCharCode(ALL[i]);
            var tag = ALL[i].toString(16);
            var text = "function f(a = '" + c + "') {{ return '" + c + "' /* " + c + " */; }}";
            if ((0, eval)("(" + text + ")").toString() !== text) return "function " + tag;
            if (eval("(" + text + ")").toString() !== text) return "direct eval " + tag;
            var arrow = "(a) => '" + c + "'";
            if ((0, eval)("(" + arrow + ")").toString() !== arrow) return "arrow " + tag;
            var method = "m() {{ return '" + c + "'; }}";
            if ((0, eval)("({{ " + method + " }})").m.toString() !== method) return "method " + tag;
            var klass = "class A {{ static x = '" + c + "'; m() {{ return `" + c + "`; }} }}";
            if ((0, eval)("(" + klass + ")").toString() !== klass) return "class " + tag;
            var text2 = Function("a", "return '" + c + "'").toString();
            if (text2 !== "function anonymous(a\n) {{\nreturn '" + c + "'\n}}") return "Function " + tag;
            // Text before the function does not shift the range it reports.
            var shifted = "'" + c + "'; /* " + c + " */ (" + text + ")";
            if ((0, eval)(shifted).toString() !== text) return "shifted " + tag;
            var nested = "(function outer() {{ return function inner() {{ return '" + c + "'; }}; }})";
            if ((0, eval)(nested)().toString() !== "function inner() {{ return '" + c + "'; }}")
              return "nested " + tag;
          }}
          return true;
        }})()"#
    ));
}

#[test]
fn function_source_text_with_unpaired_surrogates_survives_gc_stress() {
    assert_true_under_gc_stress(
        r#"(function() {
          var c = String.fromCharCode(0xD800);
          var text = "function f(a = '" + c + "') { return '" + c + "'; }";
          if ((0, eval)("(" + text + ")").toString() !== text) return "function";
          if (Function("return '" + c + "'").toString() !==
              "function anonymous(\n) {\nreturn '" + c + "'\n}") return "Function";
          return true;
        })()"#,
    );
}

#[test]
fn an_unpaired_surrogate_where_an_identifier_or_punctuator_belongs_is_a_syntax_error() {
    assert_true(&format!(
        r#"(function() {{
          {SOME_SURROGATES}
          var realm = new ShadowRealm();
          var sources = [
            function(c) {{ return c; }},
            function(c) {{ return "var " + c; }},
            function(c) {{ return "var a" + c; }},
            function(c) {{ return "a" + c + "b"; }},
            function(c) {{ return c + "a"; }},
            function(c) {{ return "1 " + c; }},
            function(c) {{ return "1" + c; }},
            function(c) {{ return "x = 1 " + c; }},
            function(c) {{ return "({{ " + c + ": 1 }})"; }},
            function(c) {{ return "({{ a" + c + ": 1 }})"; }},
            function(c) {{ return "\\u0061" + c; }},
            function(c) {{ return "a." + c; }},
            function(c) {{ return "'x' " + c; }},
            function(c) {{ return "class " + c + " {{}}"; }},
            function(c) {{ return "function " + c + "() {{}}"; }},
            function(c) {{ return "`${{" + c + "}}`"; }},
            function(c) {{ return "/x/" + c; }},
          ];
          function throwsSyntaxError(run) {{
            try {{ run(); }} catch (e) {{ return e instanceof SyntaxError; }}
            return false;
          }}
          for (var i = 0; i < ALL.length; i++) {{
            var c = String.fromCharCode(ALL[i]);
            for (var j = 0; j < sources.length; j++) {{
              var source = sources[j](c);
              var where = "source " + j + " unit " + ALL[i].toString(16);
              if (!throwsSyntaxError(function() {{ eval(source); }})) return "eval " + where;
              if (!throwsSyntaxError(function() {{ (0, eval)(source); }})) return "indirect eval " + where;
              if (!throwsSyntaxError(function() {{ Function(source); }})) return "Function " + where;
              if (!throwsSyntaxError(function() {{ realm.evaluate(source); }}))
                return "ShadowRealm " + where;
            }}
          }}
          return true;
        }})()"#
    ));
}

#[test]
fn genuine_private_use_characters_are_never_mistaken_for_surrogates() {
    // U+10F000..=U+10F800 is the block the lexer encoding reserves, so each
    // edge of it, and the characters just outside it, must survive as
    // themselves, next to an unpaired surrogate and after a backslash.
    assert_true(
        r#"(function() {
          var codes = [0x100000, 0x10EFFF, 0x10F000, 0x10F001, 0x10F3FF, 0x10F7FF, 0x10F800, 0x10F801, 0x10FFFF];
          for (var i = 0; i < codes.length; i++) {
            var s = String.fromCodePoint(codes[i]);
            var tag = codes[i].toString(16);
            var lone = "\uD800";
            if (eval("'" + s + "'") !== s) return "string " + tag;
            if (eval("'" + s + s + "'") !== s + s) return "two in a row " + tag;
            if (eval("'\\" + s + "'") !== s) return "escaped " + tag;
            if (eval("'" + lone + s + lone + "'") !== lone + s + lone) return "between surrogates " + tag;
            if (eval("'" + s + lone + s + "'") !== s + lone + s) return "around a surrogate " + tag;
            if (eval("`" + s + "`") !== s) return "template " + tag;
            if (eval("`${'" + s + "'}`") !== s) return "placeholder " + tag;
            var t = eval("(function(s) { return s; })`" + s + lone + "`");
            if (t[0] !== s + lone || t.raw[0] !== s + lone) return "tagged " + tag;
            if (!eval("/" + s + "/u").test(s)) return "regexp u " + tag;
            if (eval("/" + s + "/").source !== s) return "regexp source " + tag;
            if (!eval("/\\" + s + "/").test(s)) return "regexp identity escape " + tag;
            if (!eval("/[" + s + lone + "]/").test(lone)) return "regexp class " + tag;
            if (eval("40 + 2 // " + s) !== 42) return "comment " + tag;
            var text = "function f() { return '" + s + lone + s + "'; }";
            if ((0, eval)("(" + text + ")").toString() !== text) return "toString " + tag;
            // In identifier position it is still not an identifier character.
            var threw = false;
            try { eval("var " + s); } catch (e) { threw = e instanceof SyntaxError; }
            if (!threw) return "identifier " + tag;
            threw = false;
            try { eval("var a" + s); } catch (e) { threw = e instanceof SyntaxError; }
            if (!threw) return "identifier continue " + tag;
          }
          return true;
        })()"#,
    );
}

/// Host text reaches the parser through `parse`, which never sees a lone
/// surrogate, but its private-use characters must still be read as themselves.
#[test]
fn host_source_with_private_use_characters_keeps_them() {
    for (source, expected) in [
        ("'\u{10F000}'", "\u{10F000}"),
        (
            "'\u{10F7FF}\u{10F800}\u{10F801}'",
            "\u{10F7FF}\u{10F800}\u{10F801}",
        ),
        ("`\u{10F000}${'\u{10F800}'}`", "\u{10F000}\u{10F800}"),
        ("/*\u{10F800}*/ '\u{10F000}'", "\u{10F000}"),
        ("'\\\u{10F000}'", "\u{10F000}"),
    ] {
        let program = parse(source).unwrap_or_else(|error| panic!("{source:?}: {error:?}"));
        let value = Vm::default().execute(&compile(&program).unwrap()).unwrap();
        assert_eq!(
            value,
            Value::String(JsString::from(expected)),
            "{source:?} must not read the reserved block as a surrogate"
        );
    }
}

#[test]
fn host_source_function_text_keeps_private_use_characters() {
    let text = "function f() { return '\u{10F000}\u{10F800}'; /* \u{10F7FF} */ }";
    let source = format!("({text}).toString()");
    let program = parse(&source).unwrap();
    let value = Vm::default().execute(&compile(&program).unwrap()).unwrap();
    assert_eq!(value, Value::String(JsString::from(text)));
}

#[test]
fn a_private_use_character_is_not_an_identifier_in_host_source() {
    for source in [
        "var \u{10F000}",
        "var a\u{10F000}",
        "\u{10F800}",
        "1 \u{10F7FF}",
    ] {
        assert!(parse(source).is_err(), "{source:?}");
    }
}
