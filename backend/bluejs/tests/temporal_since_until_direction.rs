// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for a real Stage 2 `PlainDate`/`PlainDateTime`
//! `since`/`until` bug (Phase 26 Stage 2, second slice --
//! `development/browser_core/phase-26-ecma262-temporal/PLAN.md`).
//!
//! `Vm::temporal_date_difference` (`backend/bluejs/src/vm/temporal.rs`)
//! previously implemented `since` by *swapping* which date fed
//! `calendar_difference_date`'s `start`/`end` parameters (`from = other`,
//! `to = existing`) and skipping the sign flip `until` doesn't need. That is
//! not equivalent to the real
//! `DifferenceTemporalPlainDate`/`DifferenceTemporalPlainDateTime` algorithm
//! (`js/src/builtin/temporal/PlainDate.cpp`/`PlainDateTime.cpp`), which
//! *always* computes `CalendarDateUntil(calendar, temporalDate, other,
//! largestUnit)` -- i.e. always in the fixed receiver-to-argument direction,
//! exactly like `until` -- and only negates the *finished* Duration
//! afterward for `since` (`DifferenceTemporalPlainDate` step 10).
//!
//! The two are not interchangeable because `CalendarDateUntil`'s own
//! algorithm (ported as `difference_iso_date`/`calendar_difference_date` in
//! `vm/temporal/plain_date.rs`) anchors its year/month bubbling on the
//! *first* argument's day-of-month throughout, so it is not anti-symmetric:
//! `f(other, existing) != -f(existing, other)` in general. Swapping the
//! arguments instead of negating the result silently anchors on the wrong
//! date's day field and can be off by one whole day.
//!
//! Test262 caught this directly:
//! `intl402/Temporal/PlainDate/prototype/since/basic-gregory.js`'s "23
//! years, 11 months and 29 days" case (`date19970716.since(date20210715,
//! { largestUnit: "years" })`) computed `{ years: -23, months: -11, weeks:
//! 0, days: -30 }` instead of the fixture's own `{ years: -23, months: -11,
//! weeks: 0, days: -29 }` -- reproduced here through the real public
//! `Temporal.PlainDate`/`PlainDateTime.prototype.since` surface (not
//! `vm/temporal/plain_date.rs`'s internals) using that exact fixture pair,
//! plus its neighboring "23 years, 11 months and 30 days" case (same day
//! pattern, a different month whose different length makes the correct
//! remainder 30 instead of 29 -- proof this isn't a simple off-by-one
//! constant), and a `since`/`until` cross-check that would have passed even
//! under the old swap-based bug (since `until` was never swapped), so it
//! alone would not have caught this.

use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

fn assert_true(source: &str) {
    assert_eq!(evaluate(source), Value::Bool(true), "{source}");
}

/// `intl402/Temporal/PlainDate/prototype/since/basic-gregory.js`'s "23
/// years, 11 months and 29 days" case, reproduced with the `gregory`
/// calendar (the non-ISO `calendar_difference_date` path) exactly as the
/// fixture has it.
#[test]
fn since_anchors_on_the_receiver_day_not_the_argument_day_gregory_calendar() {
    assert_true(
        r#"
        (function() {
          const calendar = "gregory";
          const one = Temporal.PlainDate.from({ year: 1997, monthCode: "M07", day: 16, calendar });
          const two = Temporal.PlainDate.from({ year: 2021, monthCode: "M07", day: 15, calendar });
          const d = one.since(two, { largestUnit: "years" });
          return d.years === -23 && d.months === -11 && d.weeks === 0 && d.days === -29;
        })()
        "#,
    );
}

/// The neighboring "23 years, 11 months and 30 days" case from the same
/// fixture: the identical day-of-month pattern (16 -> 15) one month
/// earlier (June instead of July). June's shorter length changes the
/// correct day remainder from 29 to 30, which is exactly what a
/// hardcoded off-by-one fix (rather than the real anchor-direction fix)
/// would get wrong.
#[test]
fn since_day_remainder_depends_on_the_actual_month_length_not_a_fixed_offset() {
    assert_true(
        r#"
        (function() {
          const calendar = "gregory";
          const one = Temporal.PlainDate.from({ year: 1997, monthCode: "M06", day: 16, calendar });
          const two = Temporal.PlainDate.from({ year: 2021, monthCode: "M06", day: 15, calendar });
          const d = one.since(two, { largestUnit: "years" });
          return d.years === -23 && d.months === -11 && d.weeks === 0 && d.days === -30;
        })()
        "#,
    );
}

/// The same case again on the plain ISO calendar (`difference_iso_date`,
/// the pure-ISO fast path `calendar_difference_date` delegates to) --
/// confirms the bug and its fix are not gregory-specific, and that a
/// `PlainDateTime` (not just `PlainDate`) is also fixed, since both types
/// share `Vm::temporal_date_difference`.
#[test]
fn since_anchors_on_the_receiver_day_not_the_argument_day_iso_calendar() {
    assert_true(
        r#"
        (function() {
          const one = Temporal.PlainDate.from("1997-07-16");
          const two = Temporal.PlainDate.from("2021-07-15");
          const d = one.since(two, { largestUnit: "years" });
          return d.years === -23 && d.months === -11 && d.weeks === 0 && d.days === -29;
        })()
        "#,
    );
    assert_true(
        r#"
        (function() {
          const one = Temporal.PlainDateTime.from("1997-07-16T00:00:00");
          const two = Temporal.PlainDateTime.from("2021-07-15T00:00:00");
          const d = one.since(two, { largestUnit: "years" });
          return d.years === -23 && d.months === -11 && d.weeks === 0 && d.days === -29;
        })()
        "#,
    );
}

/// `a.since(b)` must always equal the field-wise negation of `a.until(b)`
/// -- the *same* receiver and argument, not swapped -- because
/// `DifferenceTemporalPlainDate` computes `CalendarDateUntil(calendar, a,
/// b, largestUnit)` for *both* operations and only negates the result for
/// `since` (step 10). This is `since`'s real invariant; note it is
/// deliberately *not* "since(a,b) == -until(b,a)" (swapped receiver and
/// argument) -- `CalendarDateUntil` anchors its year/month bubbling on its
/// first argument's day-of-month, so `until(b,a)` uses a different anchor
/// day than `since(a,b)`/`until(a,b)` do and is not interchangeable with
/// either. This pins the invariant the fix relies on going forward; it
/// alone would not have caught the original swap-based bug, since a
/// same-receiver comparison holds under both the buggy and fixed code.
#[test]
fn since_is_exactly_the_negation_of_until_with_the_same_receiver_and_argument() {
    assert_true(
        r#"
        (function() {
          const one = Temporal.PlainDate.from("1997-07-16");
          const two = Temporal.PlainDate.from("2021-07-15");
          const since = one.since(two, { largestUnit: "years" });
          const until = one.until(two, { largestUnit: "years" });
          return since.years === -until.years
              && since.months === -until.months
              && since.weeks === -until.weeks
              && since.days === -until.days;
        })()
        "#,
    );
}
