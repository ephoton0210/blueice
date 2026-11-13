// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Additional TypeScript5.9.3 calibration; outside the primary980 denominator.
pub type Match = (
    &'static str,
    &'static str,
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
    bool,
);
pub const MATCHES: &[Match] = &[
    (
        "upper-surrogate",
        "\"pre-\\u{D800}AB\"",
        "pre-",
        &["Uppercase<string>"],
        &[""],
        true,
    ),
    (
        "upper-wrong-case",
        "\"pre-\\u{D800}Ab\"",
        "pre-",
        &["Uppercase<string>"],
        &[""],
        false,
    ),
    (
        "surrogate-delimiter",
        "\"a\\u{D800}B\"",
        "",
        &["string", "Uppercase<string>"],
        &["\\u{D800}", ""],
        true,
    ),
    (
        "replacement-not-delimiter",
        "\"a\\u{FFFD}B\"",
        "",
        &["string", "Uppercase<string>"],
        &["\\u{D800}", ""],
        false,
    ),
    (
        "two-numbers",
        "\"12\"",
        "",
        &["number", "number"],
        &["", ""],
        true,
    ),
    (
        "one-number",
        "\"1\"",
        "",
        &["number", "number"],
        &["", ""],
        false,
    ),
    (
        "no-delimiter-backtracking",
        "\"A-B-12\"",
        "",
        &["Uppercase<string>", "number"],
        &["-", ""],
        false,
    ),
    (
        "valid-delimiter",
        "\"AB-12\"",
        "",
        &["Uppercase<string>", "number"],
        &["-", ""],
        true,
    ),
    (
        "head-surrogate",
        "\"\\u{D800}ABC\"",
        "\\u{D800}",
        &["Uppercase<string>"],
        &[""],
        true,
    ),
    (
        "head-replacement",
        "\"\\u{FFFD}ABC\"",
        "\\u{D800}",
        &["Uppercase<string>"],
        &[""],
        false,
    ),
];
pub const FIRST_UNIT_HEAD: &[u16] = &[55297];
pub const FIRST_UNIT_TAIL: &[u16] = &[56360, 120];
