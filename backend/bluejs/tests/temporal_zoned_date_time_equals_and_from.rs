// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for `Temporal.ZonedDateTime.prototype.equals` and
//! `Temporal.ZonedDateTime.from`'s shared `temporal_to_zoned_date_time`
//! property-bag path, pinned directly from real Test262 fixtures:
//!
//! - `argument-wrong-type.js` (both methods): the non-object branch requires
//!   a literal `String`, never `ToString`-coerced -- a `Number`/`Boolean`/
//!   `null`/`BigInt`/`Symbol` argument is a `TypeError`.
//! - `argument-propertybag-calendar-invalid-iso-string.js` /
//!   `argument-propertybag-calendar-year-zero.js` (both methods): an invalid
//!   `calendar` field is a `RangeError` raised *before* the (also missing,
//!   in these fixtures) `timeZone` field is even checked for presence --
//!   `PrepareCalendarFields` validates `calendar` first thing, ahead of
//!   `ToTemporalZonedDateTime`'s own `timeZone`-presence check.
//! - `canonicalize-iana-names.js`/`canonical-iana-names.js` (equals): two
//!   IANA aliases of the same real zone (`Asia/Calcutta`/`Asia/Kolkata`)
//!   compare equal via `TimeZone::time_zone_equals`.

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

fn assert_true(source: &str) {
    assert_eq!(evaluate(source), Value::Bool(true), "{source}");
}

fn assert_type_error(source: &str) {
    let program = compile(&parse(source).unwrap()).unwrap();
    match Vm::default().execute(&program) {
        Err(RuntimeError::TypeError(_)) => {}
        other => panic!("{source}\n  -> expected TypeError, got {other:?}"),
    }
}

fn assert_range_error(source: &str) {
    let program = compile(&parse(source).unwrap()).unwrap();
    match Vm::default().execute(&program) {
        Err(RuntimeError::RangeError(_)) => {}
        other => panic!("{source}\n  -> expected RangeError, got {other:?}"),
    }
}

#[test]
fn equals_throws_type_error_for_non_string_non_object_arguments() {
    let instance = r#"const instance = new Temporal.ZonedDateTime(0n, "UTC");"#;
    for arg in ["undefined", "null", "true", "1", "19761118", "1n"] {
        assert_type_error(&format!("{instance}\n instance.equals({arg});"));
    }
}

#[test]
fn from_throws_type_error_for_non_string_non_object_arguments() {
    for arg in ["undefined", "null", "true", "1", "19761118", "1n"] {
        assert_type_error(&format!("Temporal.ZonedDateTime.from({arg});"));
    }
}

#[test]
fn equals_validates_calendar_before_checking_timezone_presence() {
    let instance = r#"const instance = new Temporal.ZonedDateTime(0n, "UTC");"#;
    for cal in ["\"\"", "\"1997-12-04[u-ca=notacal]\"", "\"notacal\""] {
        assert_range_error(&format!(
            "{instance}\n instance.equals({{ year: 1970, monthCode: \"M11\", day: 18, calendar: {cal} }});"
        ));
    }
}

#[test]
fn from_validates_calendar_before_checking_timezone_presence() {
    for cal in ["\"\"", "\"1997-12-04[u-ca=notacal]\"", "\"notacal\""] {
        assert_range_error(&format!(
            "Temporal.ZonedDateTime.from({{ year: 1976, monthCode: \"M11\", day: 18, calendar: {cal} }});"
        ));
    }
}

#[test]
fn equals_canonicalizes_iana_aliases_before_comparing() {
    assert_true(
        r#"
        const zdt = new Temporal.ZonedDateTime(0n, "America/Los_Angeles");
        const z1 = zdt.withTimeZone("Asia/Calcutta");
        const z2 = zdt.withTimeZone("Asia/Kolkata");
        z1.equals(z2) && z2.equals(z1) && z1.equals(z2.toString()) && z2.equals(z1.toString())
    "#,
    );
    assert_true(
        r#"
        const zdt = new Temporal.ZonedDateTime(0n, "America/Los_Angeles");
        const neverEqual = zdt.withTimeZone("Asia/Tokyo");
        !zdt.withTimeZone("Asia/Calcutta").equals(neverEqual)
    "#,
    );
}

#[test]
fn equals_treats_the_etc_gmt_family_as_equal_to_primary_utc() {
    assert_true(
        r#"
        const utc = new Temporal.ZonedDateTime(0n, "UTC");
        ["Etc/GMT", "Etc/UTC", "GMT"].every(
            (name) => new Temporal.ZonedDateTime(0n, name).equals(utc)
        )
    "#,
    );
}

/// `from/offset-string-invalid.js`: a syntactically invalid `offset` is a
/// `RangeError` even when `year` is a `Symbol` that would otherwise throw
/// `TypeError` first (offset *syntax* is read ahead of `year`), but a
/// syntactically valid offset that merely doesn't match the zone only
/// surfaces *after* `year` has already thrown (offset *matching* happens
/// only once every other field, including `year`, is fully resolved). This
/// is a real regression this same pass introduced and fixed: an earlier,
/// too-broad reordering (to fix the `calendar`-vs-`timeZone` ordering
/// covered above) moved *all* date/time field resolution, `year` included,
/// ahead of `offset` entirely.
#[test]
fn from_validates_offset_syntax_before_year_type_but_matches_offset_after() {
    assert_range_error(
        r#"
        Temporal.ZonedDateTime.from({
            offset: "--00:00", year: Symbol(), monthCode: "M10", day: 3, timeZone: "UTC",
        });
    "#,
    );
    assert_type_error(
        r#"
        Temporal.ZonedDateTime.from({
            offset: "+04:30", year: Symbol(), monthCode: "M10", day: 3, timeZone: "UTC",
        });
    "#,
    );
}
