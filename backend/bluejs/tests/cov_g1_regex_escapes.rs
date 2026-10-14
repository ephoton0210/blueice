// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regular-expression escape handling: hexadecimal, octal, control, class and
//! identity escapes (including lone surrogates and astral characters), the
//! case-insensitive rewrite, and the leading zeros of braced escapes. The
//! expected strings were produced by Node.

use blueice_bluejs::{compile, parse, Value, Vm};

/// Evaluates `expr` and reports its `String` conversion, or `throws Name`.
fn observe(expr: &str) -> String {
    let source = format!(
        "(function(){{ try {{ return String({expr}) }} catch (e) {{ return 'throws ' + e.constructor.name }} }})()"
    );
    let mut vm = Vm::default();
    match vm.execute(&compile(&parse(&source).unwrap()).unwrap()) {
        Ok(Value::String(text)) => text.to_utf8().unwrap(),
        other => format!("unexpected {other:?}"),
    }
}

#[test]
fn regex_escapes_match_the_reference_results() {
    let mut mismatches = Vec::new();
    for (expr, expected) in RX_CASES {
        let actual = observe(expr);
        if actual != *expected {
            mismatches.push(format!(
                "{expr}\n    expected {expected}\n    actual   {actual}"
            ));
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

const RX_CASES: &[(&str, &str)] = &[
    (
        "new RegExp('\\\\u12\\uD800', 'i').test('U12\\uD800')",
        "true",
    ),
    (
        "new RegExp('\\\\u12\\uD800', 'i').test('u12\\uD800')",
        "true",
    ),
    ("new RegExp('\\\\x1\\uD800', 'i').test('X1\\uD800')", "true"),
    ("new RegExp('\\\\x1\\uD800', 'i').test('x1\\uD800')", "true"),
    ("new RegExp('[\\\\u12\\uD800]', 'i').test('U')", "true"),
    ("new RegExp('[\\\\x1\\uD800]', 'i').test('X')", "true"),
    (
        "new RegExp('\\\\\\uD83D\\uDE00', 'i').test('\\uD83D\\uDE00')",
        "true",
    ),
    (
        "new RegExp('[\\uD83D\\uDE00]', 'i').test('\\uD83D')",
        "true",
    ),
    (
        "new RegExp('[\\uD83D\\uDE00]', 'i').test('\\uDE00')",
        "true",
    ),
    ("new RegExp('[a-\\uD83D\\uDE00]', 'i').test('b')", "true"),
    (
        "new RegExp('[\\uD83D\\uDE00-a]', 'i').test('b')",
        "throws SyntaxError",
    ),
    ("new RegExp('\\\\0', 'i').test('\\0')", "true"),
    ("new RegExp('\\\\08', 'i').test('\\x008')", "true"),
    ("new RegExp('\\\\00', 'i').test('\\0')", "true"),
    ("new RegExp('\\\\012', 'i').test('\\n')", "true"),
    ("new RegExp('\\\\1', 'i').test('\\x01')", "true"),
    ("new RegExp('\\\\7', 'i').test('\\x07')", "true"),
    ("new RegExp('\\\\8', 'i').test('8')", "true"),
    ("new RegExp('\\\\9', 'i').test('9')", "true"),
    ("new RegExp('(a)\\\\1', 'i').test('aA')", "true"),
    ("new RegExp('\\\\1(a)', 'i').test('a')", "true"),
    ("new RegExp('\\\\2(a)', 'i').test('\\x02a')", "true"),
    ("new RegExp('[\\\\1]', 'i').test('\\x01')", "true"),
    ("new RegExp('[\\\\7]', 'i').test('\\x07')", "true"),
    ("new RegExp('[\\\\08]', 'i').test('8')", "true"),
    ("new RegExp('[\\\\0]', 'i').test('\\0')", "true"),
    ("new RegExp('[\\\\c]', 'i').test('\\\\')", "true"),
    ("new RegExp('[\\\\cA]', 'i').test('\\x01')", "true"),
    ("new RegExp('[\\\\c1]', 'i').test('\\x11')", "true"),
    ("new RegExp('[\\\\c_]', 'i').test('\\x1f')", "true"),
    ("new RegExp('\\\\c', 'i').test('\\\\c')", "true"),
    ("new RegExp('\\\\cJ', 'i').test('\\n')", "true"),
    ("new RegExp('\\\\c1', 'i').test('\\\\c1')", "true"),
    ("new RegExp('\\\\k', 'i').test('k')", "true"),
    ("new RegExp('\\\\k<a>(?<a>x)', 'i').test('x')", "true"),
    ("new RegExp('\\\\k<a', 'i').test('k<a')", "true"),
    (
        "new RegExp('(?<a>x)\\\\k', 'i').test('x')",
        "throws SyntaxError",
    ),
    ("new RegExp('(?<a>x)\\\\k<a', 'i')", "throws SyntaxError"),
    ("new RegExp('[\\\\k]', 'i').test('k')", "true"),
    ("new RegExp('\\\\-', 'i').test('-')", "true"),
    ("new RegExp('\\\\/', 'i').test('/')", "true"),
    ("new RegExp('\\\\a', 'i').test('A')", "true"),
    ("new RegExp('[\\\\a]', 'i').test('A')", "true"),
    ("new RegExp('[\\\\b]', 'i').test('\\b')", "true"),
    ("new RegExp('[\\\\d\\\\D]', 'i').test('x')", "true"),
    ("new RegExp('[\\\\s\\\\S]', 'i').test('x')", "true"),
    ("new RegExp('[\\\\w\\\\W]', 'i').test('x')", "true"),
    ("new RegExp('[b-a]', 'i')", "throws SyntaxError"),
    ("new RegExp('[a-\\\\d]', 'i').test('-')", "true"),
    ("new RegExp('[\\\\d-a]', 'i').test('-')", "true"),
    ("new RegExp('[a-\\\\k]', 'i').test('b')", "true"),
    ("new RegExp('[\\\\x41-\\\\x5A]', 'i').test('q')", "true"),
    ("new RegExp('[\\\\u0041-\\\\u005A]', 'i').test('q')", "true"),
    ("new RegExp('[\\\\x4]', 'i').test('x')", "true"),
    ("new RegExp('[\\\\u004]', 'i').test('u')", "true"),
    ("new RegExp('[^\\\\x41]', 'i').test('a')", "false"),
    ("new RegExp('[]', 'i').test('a')", "false"),
    ("new RegExp('[^]', 'i').test('a')", "true"),
    ("new RegExp('\\\\u{41}', 'i').test('u')", "false"),
    ("new RegExp('\\\\u{41}', 'iu').test('a')", "true"),
    ("new RegExp('\\\\u{41}', 'iv').test('A')", "true"),
    ("new RegExp('[\\\\W]', 'iu').test('S')", "false"),
    ("new RegExp('[\\\\W]', 'iv').test('\\u017f')", "false"),
    ("new RegExp('\\\\W', 'iu').test('S')", "false"),
    ("new RegExp('\\\\W', 'iu').test('\\u212a')", "false"),
    ("new RegExp('[^\\\\W]', 'iu').test('k')", "true"),
    ("new RegExp('\\\\w', 'iu').test('\\u017f')", "true"),
    ("new RegExp('\\\\u12\\uD800', 'iu')", "throws SyntaxError"),
    ("new RegExp('\\\\u12\\uD800', 'iv')", "throws SyntaxError"),
    ("new RegExp('\\\\u{000041}', 'iu').test('a')", "true"),
    (
        "new RegExp('\\\\u{0000000000000000000000000041}', 'u').test('A')",
        "true",
    ),
    ("new RegExp('\\\\u{0}', 'u').test('\\0')", "true"),
    ("new RegExp('\\\\u{00}', 'u').test('\\0')", "true"),
    ("new RegExp('\\\\u{000}', 'u').test('\\0')", "true"),
    ("new RegExp('\\\\u{00041}', 'u').test('A')", "true"),
    ("new RegExp('\\\\u{0041', 'u')", "throws SyntaxError"),
    ("new RegExp('\\\\u{}', 'u')", "throws SyntaxError"),
    (
        "new RegExp('[\\\\u{000041}-\\\\u{00005A}]', 'u').test('Q')",
        "true",
    ),
    (
        "new RegExp('\\\\u{00}\\\\u{000}', 'v').test('\\0\\0')",
        "true",
    ),
];
