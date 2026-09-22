// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for `Temporal.ZonedDateTime.from`'s default
//! (`"constrain"`) overflow handling of an out-of-range `day` property-bag
//! field, pinned directly from the real Test262 fixtures
//! `built-ins/Temporal/ZonedDateTime/from/overflow-options.js` and
//! `overflow-undefined.js`.
//!
//! `temporal_plain_date_from_fields`'s own `day` field read
//! (`vm/temporal.rs`) validated the raw property-bag value against a
//! hardcoded `1..=31` bound *before* the calendar's own overflow-aware
//! `Date::try_from_fields` ever ran -- so `{ day: 32 }` always threw
//! `RangeError: invalid Temporal day`, even under the default `"constrain"`
//! overflow, which must instead clamp it to the month's real last day. Every
//! `.with()`-style call site in this same file already widened this same
//! bound to `1..=i32::MAX` for exactly this reason (`with/overflow.js`'s own
//! `wrapping-at-end-of-month-*.js` cases); `temporal_plain_date_from_fields`
//! -- shared by every type's `from`/constructor path, not just
//! `ZonedDateTime`'s -- had simply never had the same widening applied.

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

fn assert_true(source: &str) {
    assert_eq!(evaluate(source), Value::Bool(true), "{source}");
}

/// `overflow-options.js`: default (no `overflow` option at all) and explicit
/// `"constrain"` both clamp `day: 32` in a 31-day month down to `31`;
/// explicit `"reject"` still throws.
#[test]
fn from_constrains_an_out_of_range_day_by_default() {
    assert_true(
        r#"
        const bad = { year: 2019, month: 1, day: 32, timeZone: "+01:00" };
        const expected = new Temporal.ZonedDateTime(1548889200000000000n, "+01:00");
        Temporal.ZonedDateTime.from(bad).epochNanoseconds === expected.epochNanoseconds
            && Temporal.ZonedDateTime.from(bad, { overflow: "constrain" }).epochNanoseconds
                === expected.epochNanoseconds
    "#,
    );
    let program = compile(
        &parse(
            r#"
            Temporal.ZonedDateTime.from(
                { year: 2019, month: 1, day: 32, timeZone: "+01:00" },
                { overflow: "reject" }
            );
        "#,
        )
        .unwrap(),
    )
    .unwrap();
    match Vm::default().execute(&program) {
        Err(RuntimeError::RangeError(_)) => {}
        other => panic!("expected RangeError, got {other:?}"),
    }
}

/// `overflow-undefined.js`: an explicit `{ overflow: undefined }` behaves
/// exactly like the option being entirely absent.
#[test]
fn from_treats_an_explicit_undefined_overflow_option_as_the_default() {
    assert_true(
        r#"
        const propertyBag = { year: 2000, month: 15, day: 34, hour: 12, timeZone: "UTC" };
        const explicit = Temporal.ZonedDateTime.from(propertyBag, { overflow: undefined });
        explicit.epochNanoseconds === 978_264_000_000_000_000n
    "#,
    );
}
