// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for `Temporal.ZonedDateTime.prototype.year`
//! (`temporal_getter`, `backend/bluejs/src/vm/temporal.rs`), pinned
//! directly from the real Test262 fixtures
//! `built-ins/Temporal/ZonedDateTime/prototype/year/basic.js` and
//! `intl402/Temporal/ZonedDateTime/prototype/year/{arithmetic-year,epoch-year}.js`.
//!
//! `year` was the only calendar-field getter whose dispatch match guard
//! omitted `TemporalKind::ZonedDateTime` (`Day`/`Era`/`EraYear`/
//! `MonthsInYear`/`DaysInMonth`/`DaysInYear`/`InLeapYear` all already listed
//! it, and `Month`/`MonthCode` use a `!= PlainTime` guard that includes it
//! too) -- so `zonedDateTime.year` always threw
//! `"Temporal calendar field is unavailable on this receiver"`, on every
//! calendar including plain `iso8601`. This is a very high-traffic getter
//! (`TemporalHelpers.assertPlainDateTime`/`assertZonedDateTime`-style
//! helpers read it constantly), so this single missing guard entry was
//! reachable from a large, otherwise-unrelated-looking swath of `add`/
//! `subtract`/`since`/`until`/`with`/`from` fixtures across every calendar,
//! not just a `year`-getter-specific one.

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

fn assert_true(source: &str) {
    assert_eq!(evaluate(source), Value::Bool(true), "{source}");
}

/// `built-ins/.../year/basic.js`.
#[test]
fn zoned_date_time_year_getter_reads_the_iso_calendar_year() {
    assert_true(
        r#"
        new Temporal.ZonedDateTime(0n, "UTC").year === 1970
            && Temporal.ZonedDateTime.from("2019-03-15T15:30:26+00:00[UTC]").year === 2019
    "#,
    );
}

/// `intl402/.../year/epoch-year.js`-style: a non-ISO calendar's `year` still
/// resolves through the same shared `temporal_calendar_fields` path
/// `.month`/`.day` already used successfully.
#[test]
fn zoned_date_time_year_getter_reads_a_non_iso_calendar_year() {
    assert_true(
        r#"
        const zdt = Temporal.ZonedDateTime.from({
            year: 2021, month: 5, day: 3, hour: 12, minute: 0,
            timeZone: "UTC", calendar: "gregory",
        });
        zdt.year === 2021 && zdt.era === "ce" && zdt.eraYear === 2021
    "#,
    );
}

/// `arithmetic-year.js`-style: `.year` on the result of `.add()` still works
/// (this is exactly the code path `add`/`subtract`/`since`/`until` Test262
/// fixtures exercise via `TemporalHelpers.assertPlainDateTime` reading the
/// result's `.year`).
#[test]
fn zoned_date_time_year_getter_works_after_add() {
    assert_true(
        r#"
        const start = Temporal.ZonedDateTime.from({
            year: 2000, month: 1, day: 1, hour: 0, minute: 0, timeZone: "UTC",
        });
        const end = start.add({ years: 1, months: 2 });
        end.year === 2001 && end.month === 3
    "#,
    );
}

/// Confirms the fix is scoped correctly: the `year` getter, called directly
/// (`Function.prototype.call`) against a receiver kind with genuinely no
/// year concept (`Temporal.Instant`, which installs no `year` property of
/// its own at all), still throws -- the branding-style check every
/// `prototype/year/branding.js` fixture pins.
#[test]
fn zoned_date_time_year_getter_fix_does_not_leak_to_unrelated_kinds() {
    let program = compile(
        &parse(
            r#"
            const getter = Object.getOwnPropertyDescriptor(
                Temporal.PlainDate.prototype, "year"
            ).get;
            getter.call(new Temporal.Instant(0n));
        "#,
        )
        .unwrap(),
    )
    .unwrap();
    match Vm::default().execute(&program) {
        Err(RuntimeError::TypeError(_)) => {}
        other => panic!("expected TypeError, got {other:?}"),
    }
}
