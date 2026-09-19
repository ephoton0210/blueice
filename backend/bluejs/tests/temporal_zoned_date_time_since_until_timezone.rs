// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for `Temporal.ZonedDateTime.prototype.since`/`until`
//! requiring `TimeZoneEquals` between the receiver and the argument, pinned
//! directly from the real Test262 fixture
//! `intl402/Temporal/ZonedDateTime/prototype/since/canonicalize-iana-identifiers-before-comparing.js`.
//!
//! `temporal_zoned_date_time_difference` previously had *no* time-zone
//! check at all -- it silently used the receiver's own zone for calendar
//! bracketing while pulling epoch/local-date fields straight off the
//! argument, regardless of what zone that argument was actually in. Two
//! IANA aliases of the same real zone (`Asia/Calcutta`/`Asia/Kolkata`) must
//! not throw; two genuinely different zones (`Asia/Calcutta`/`Asia/Colombo`)
//! must.

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
fn since_accepts_two_aliases_of_the_same_real_zone() {
    assert_true(
        r#"
        const calcutta = Temporal.ZonedDateTime.from('2020-01-01T00:00:00+05:30[Asia/Calcutta]');
        const kolkata = Temporal.ZonedDateTime.from('2021-09-01T00:00:00+05:30[Asia/Kolkata]');
        calcutta.since(kolkata, { largestUnit: 'day' }).toString() === '-P609D'
    "#,
    );
}

/// A pure time-unit difference (`largestUnit` finer than `"day"`, the
/// default) never consults either operand's zone -- `TimeZoneEquals` is
/// only required once `largestUnit` reaches `"day"` or coarser
/// (`DifferenceTemporalZonedDateTime`'s own `largestUnit > TemporalUnit::Day`
/// branch in Gecko's `ZonedDateTime.cpp`).
#[test]
fn since_and_until_ignore_zone_mismatch_for_a_sub_day_largest_unit() {
    assert_true(
        r#"
        const a = Temporal.ZonedDateTime.from('2020-01-01T00:00:00+05:30[Asia/Calcutta]');
        const b = Temporal.ZonedDateTime.from('2022-08-01T00:00:00+05:30[Asia/Colombo]');
        typeof a.since(b).toString() === 'string' && typeof a.until(b).toString() === 'string'
    "#,
    );
}

#[test]
fn since_and_until_reject_genuinely_different_zones() {
    let program = compile(
        &parse(
            r#"
            const calcutta = Temporal.ZonedDateTime.from('2020-01-01T00:00:00+05:30[Asia/Calcutta]');
            const colombo = Temporal.ZonedDateTime.from('2022-08-01T00:00:00+05:30[Asia/Colombo]');
            calcutta.since(colombo, { largestUnit: 'day' });
        "#,
        )
        .unwrap(),
    )
    .unwrap();
    match Vm::default().execute(&program) {
        Err(RuntimeError::RangeError(_)) => {}
        other => panic!("expected RangeError, got {other:?}"),
    }

    let program = compile(
        &parse(
            r#"
            const calcutta = Temporal.ZonedDateTime.from('2020-01-01T00:00:00+05:30[Asia/Calcutta]');
            const colombo = Temporal.ZonedDateTime.from('2022-08-01T00:00:00+05:30[Asia/Colombo]');
            calcutta.until(colombo, { largestUnit: 'day' });
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
