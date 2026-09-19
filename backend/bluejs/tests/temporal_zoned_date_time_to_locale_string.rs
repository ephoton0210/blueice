// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for `Temporal.ZonedDateTime.prototype.toLocaleString`'s
//! own `GetDateTimeFormat`/`Defaults::ZonedDateTime` shape (per the
//! Temporal-in-`Intl` proposal, `DateTimeFormat.cpp`'s
//! `TemporalObjectToLocaleString`/`CreateDateTimeFormat`), which
//! `temporal_zoned_date_time_to_locale_string` previously implemented by
//! reusing the plain `Intl.DateTimeFormat` constructor path
//! (`create_date_time_format`) with a forced `timeZone` property shadowing
//! whatever the user passed -- silently accepting a user-supplied `timeZone`
//! option instead of rejecting it, and never applying the
//! year/month/day/hour/minute/second + `timeZoneName: "short"` default field
//! set every other Temporal type's own `toLocaleString` has no equivalent
//! of. Every assertion below is pinned to a real, previously-failing
//! Test262 fixture under
//! `intl402/Temporal/ZonedDateTime/prototype/toLocaleString/`.

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

fn assert_true(source: &str) {
    assert_eq!(evaluate(source), Value::Bool(true), "{source}");
}

#[test]
fn options_timezone_is_rejected_even_when_it_agrees_with_the_instance() {
    // `toLocaleString/options-timeZone.js`: a `timeZone` option must throw
    // unconditionally, even naming the receiver's own zone.
    for options in ["{ timeZone: 'Europe/Vienna' }", "{ timeZone: 'UTC' }"] {
        let program = compile(
            &parse(&format!(
                r#"
                const datetime = new Temporal.ZonedDateTime(0n, "UTC");
                datetime.toLocaleString("en-US", {options});
            "#
            ))
            .unwrap(),
        )
        .unwrap();
        match Vm::default().execute(&program) {
            Err(RuntimeError::TypeError(_)) => {}
            other => panic!("expected TypeError for {options}, got {other:?}"),
        }
    }
}

#[test]
fn default_includes_full_date_time_and_a_short_time_zone_name() {
    // `toLocaleString/default-includes-time-and-time-zone-name.js`: with no
    // options at all, the default field set is year/month/day/hour/minute/
    // second plus `timeZoneName: "short"` -- distinct from every other
    // Temporal type's own `toLocaleString` default, which never adds a time
    // zone name.
    assert_true(
        r#"
        const zdt = new Temporal.ZonedDateTime(1735213600_321_000_000n, "UTC");
        const result = zdt.toLocaleString("en");
        result.includes("2024") && result.includes("26")
        && result.includes("11") && result.includes("46") && result.includes("40")
        && !result.includes("321")
        && result.includes("UTC")
    "#,
    );
}

#[test]
fn a_lone_time_zone_name_option_still_gets_the_full_date_time_defaults() {
    // `toLocaleString/lone-options-accepted.js`'s own `timeZoneName` case:
    // `era` and `timeZoneName` are excluded from the "does the caller want
    // defaults" gate (mirroring `Date.prototype.toLocaleString`'s identical
    // exclusion), so a lone `{ timeZoneName: "short" }` must still produce
    // the full year/month/day/hour/minute/second set alongside it, not the
    // time zone name in isolation.
    assert_true(
        r#"
        const zdt = new Temporal.ZonedDateTime(1735213600_321_000_000n, "UTC");
        const withTimeZoneName = zdt.toLocaleString("en", { timeZoneName: "short" });
        const legacyDate = new Date(1735213600_321);
        withTimeZoneName === legacyDate.toLocaleString("en", { timeZoneName: "short", timeZone: "UTC" })
    "#,
    );
}

#[test]
fn a_lone_explicit_component_option_suppresses_the_defaults() {
    // The mirror case: any of the 9 ordinary date/time component fields
    // present alone (unlike `era`/`timeZoneName`) skips the whole default
    // set, matching `Date.prototype.toLocaleString`'s own lone-option
    // behavior exactly.
    assert_true(
        r#"
        const zdt = new Temporal.ZonedDateTime(1735213600_321_000_000n, "UTC");
        zdt.toLocaleString("en", { year: "numeric" }) === "2024"
    "#,
    );
}

#[test]
fn date_style_or_time_style_present_but_undefined_still_gets_the_defaults() {
    // `toLocaleString/dateStyle-timeStyle-undefined.js`: an explicitly
    // `undefined`-valued `dateStyle`/`timeStyle` property must behave
    // exactly as if it were absent.
    assert_true(
        r#"
        const zdt = new Temporal.ZonedDateTime(957270896_987_650_000n, "UTC");
        const expected = new Intl.DateTimeFormat("en", {
            year: "numeric", month: "numeric", day: "numeric",
            hour: "numeric", minute: "numeric", second: "numeric",
            timeZoneName: "short", timeZone: "UTC",
        }).format(zdt.toInstant());
        zdt.toLocaleString("en", { dateStyle: undefined }) === expected
        && zdt.toLocaleString("en", { timeStyle: undefined }) === expected
    "#,
    );
}
