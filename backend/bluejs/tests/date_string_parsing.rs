// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `Date.parse` and `new Date(string)`: the ISO format is required; a space
//! in place of the `T`, one-digit month/day/time fields and an offset without
//! a colon are accepted as an implementation-defined extension (the form
//! SpiderMonkey and V8 both read), while a `T` keeps the strict format.

use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

fn assert_true(source: &str) {
    match evaluate(source) {
        Value::Bool(true) => {}
        Value::String(reason) => panic!("{}", reason.to_utf8().unwrap()),
        other => panic!("{source}\n  -> {other:?}"),
    }
}

/// Each pair of strings must denote the same time value, through both entry points.
fn assert_same_time(pairs: &[(&str, &str)]) {
    let list = pairs
        .iter()
        .map(|(relaxed, strict)| format!("[{relaxed:?}, {strict:?}]"))
        .collect::<Vec<_>>()
        .join(", ");
    assert_true(&format!(
        r#"(function() {{
          for (var [relaxed, strict] of [{list}]) {{
            var expected = new Date(strict).getTime();
            if (Number.isNaN(expected)) return "the reference " + strict + " is not a date";
            if (new Date(relaxed).getTime() !== expected) return "new Date(" + relaxed + ")";
            if (Date.parse(relaxed) !== expected) return "Date.parse(" + relaxed + ")";
          }}
          return true;
        }})()"#
    ));
}

fn assert_invalid(strings: &[&str]) {
    let list = strings
        .iter()
        .map(|s| format!("{s:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    assert_true(&format!(
        r#"(function() {{
          for (var text of [{list}]) {{
            if (!Number.isNaN(new Date(text).getTime())) return "new Date(" + text + ")";
            if (!Number.isNaN(Date.parse(text))) return "Date.parse(" + text + ")";
          }}
          return true;
        }})()"#
    ));
}

#[test]
fn a_space_may_replace_the_t_and_time_fields_may_have_one_digit() {
    assert_same_time(&[
        ("1997-03-08 1:1:1.01", "1997-03-08T01:01:01.01"),
        ("1997-03-08 11:19:20", "1997-03-08T11:19:20"),
        ("1997-03-08 11:19", "1997-03-08T11:19"),
        ("1997-03-08 1:19", "1997-03-08T01:19"),
        ("1997-03-08 1:1", "1997-03-08T01:01"),
        ("1997-03-08 1:1:01", "1997-03-08T01:01:01"),
        ("1997-03-08 1:1:1", "1997-03-08T01:01:01"),
        ("1997-03-08 24:00", "1997-03-09T00:00"),
    ]);
}

#[test]
fn twenty_four_hundred_is_the_end_of_the_day_in_both_formats() {
    assert_same_time(&[
        ("1997-03-08T24:00", "1997-03-09T00:00"),
        ("1997-03-08T24:00:00.000", "1997-03-09T00:00:00"),
        ("1997-02-28 24:00", "1997-03-01T00:00"),
        ("1997-12-31T24:00Z", "1998-01-01T00:00Z"),
    ]);
    // A day that does not exist stays invalid, even at 24:00.
    assert_invalid(&[
        "1997-02-29T24:00",
        "1997-02-30 24:00",
        "1997-03-08T24:00:01",
    ]);
}

#[test]
fn month_and_day_may_have_one_digit_when_a_space_separates_the_time() {
    assert_same_time(&[
        ("1997-3-08 11:19:20", "1997-03-08T11:19:20"),
        ("1997-3-8 11:19:20", "1997-03-08T11:19:20"),
        ("1997-03-8 11:19:20", "1997-03-08T11:19:20"),
        ("+001997-3-8 11:19:20", "1997-03-08T11:19:20"),
        ("+001997-03-8 11:19:20", "1997-03-08T11:19:20"),
    ]);
}

#[test]
fn an_offset_may_be_written_with_or_without_a_colon_or_minutes() {
    assert_same_time(&[
        ("1997-03-08 11:19:10-07", "1997-03-08T11:19:10-07:00"),
        ("1997-03-08 11:19:10-0700", "1997-03-08T11:19:10-07:00"),
        ("1997-03-08 11:19:10-07:00", "1997-03-08T11:19:10-07:00"),
        ("1997-03-08 11:19:10+0530", "1997-03-08T11:19:10+05:30"),
        ("1997-03-08 11:19:10Z", "1997-03-08T11:19:10Z"),
        ("1997-3-8 1:1:1.5Z", "1997-03-08T01:01:01.500Z"),
    ]);
}

#[test]
fn a_t_keeps_the_strict_iso_format() {
    assert_invalid(&[
        "1997-03-08T11:19:10-07",
        "1997-03-08T",
        "1997-3-8T11:19:20",
        "1997-03-8T11:19:20",
        "+001997-3-8T11:19:20",
        "+001997-3-08T11:19:20",
        "1997-03-08T1:19",
        "1997-03-08T1:1",
        "1997-03-08T1:1:01",
        "1997-03-08T1:1:1",
    ]);
}

#[test]
fn the_relaxed_format_still_rejects_what_is_not_a_time() {
    assert_invalid(&[
        "1997-03-08 11",
        "1997-03-08 25:00",
        "1997-03-08 24:01",
        "1997-03-08 11:60",
        "1997-03-08 11:19:60",
        "1997-03-08 11:19:",
        "1997-03-08 11:19:20.",
        "1997-03-08 111:19",
        "1997-03-08 11:191",
        "1997-03-08  11:19",
        "1997-02-30 10:00",
        "1997-13-01 10:00",
        "1997-0-01 10:00",
        "1997-003-08 11:19",
        "-000000-03-08 11:19",
        "1997-03-08 11:19:10-",
        "1997-03-08 11:19:10-7",
        "1997-03-08 11:19:10-07:0",
        "1997-03-08 11:19 x",
        "1997-3-8",
    ]);
}

#[test]
fn a_date_string_of_the_constructor_and_of_parse_agree_for_the_engines_own_format() {
    // `new Date(string)` is specified as `parse(string)`, so the strings a
    // Date prints (which parse must round-trip) work in both places.
    assert_true(
        r#"(function() {
          var date = new Date(Date.UTC(1997, 2, 8, 11, 19, 20));
          for (var text of [date.toString(), date.toUTCString(), date.toISOString()]) {
            if (new Date(text).getTime() !== date.getTime()) return "new Date(" + text + ")";
            if (Date.parse(text) !== date.getTime()) return "Date.parse(" + text + ")";
          }
          return true;
        })()"#,
    );
}
