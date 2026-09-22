// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for the leap-month-calendar (`chinese`/`dangi`/
//! `hebrew`) `add`/`subtract` gap Phase 26 Stage 2's leap-month `since`/
//! `until` pass documented and deliberately left open
//! (`development/browser_core/phase-26-ecma262-temporal/PLAN.md`):
//! `vm/temporal/plain_date.rs`'s `calendar_add_date` previously carried
//! `years`/`months` through flat ordinal position for every non-ISO
//! calendar, which is wrong for a leap-month calendar the same way the
//! difference side was (a leap month's ordinal position shifts year to
//! year).
//!
//! Cases are taken directly, with the same expected values, from Test262's
//! `intl402/Temporal/PlainDate/prototype/add/leap-months-{chinese,hebrew}.js`,
//! reproduced through the real public `Temporal.PlainDate.prototype.add`
//! surface (not `plain_date.rs`'s internals directly). Note the add side's
//! own fallback convention for a non-recurring leap month
//! (`LeapMonthFallback::Native` — trust whichever fallback `icu_calendar`'s
//! own `Date::try_from_fields` already applies per calendar) differs from
//! `temporal_leap_month_calendar_difference.rs`'s own `since`/`until`
//! fixtures (`PickNextMonth`, applied uniformly regardless of calendar,
//! e.g. `M04L` constrains to `M05`) — both are real, independently-verified
//! conventions for their own operation. `Native` is itself calendar-
//! dependent, not a second uniform rule: `chinese`/`dangi` drop the leap
//! flag and keep the same month number (`M03L` -> `M03`), while `hebrew`
//! picks the next month (`M05L` -> `M06`) — see `plain_date.rs`'s
//! `LeapMonthFallback` doc comment for the full account, including the
//! `icu_calendar` source citations for each calendar's own native
//! behavior.

use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

fn assert_true(source: &str) {
    assert_eq!(evaluate(source), Value::Bool(true), "{source}");
}

/// "Adding 1 year to leap month M03L lands in common-year M03 with overflow
/// constrain" and its `overflow: "reject"` counterpart, which must throw --
/// the add-side's own `LeapMonthFallback::SameNumberDropLeap` convention
/// (not the difference side's `PickNextMonth`, which would land on `M04`
/// instead).
#[test]
fn add_years_to_a_non_recurring_leap_month_constrains_to_the_same_month_number() {
    assert_true(
        r#"
        (function() {
          const calendar = "chinese";
          const leap196603L = Temporal.PlainDate.from({ year: 1966, monthCode: "M03L", day: 1, calendar });
          const constrained = leap196603L.add(new Temporal.Duration(1));
          let threw = false;
          try {
            leap196603L.add(new Temporal.Duration(1), { overflow: "reject" });
          } catch (e) {
            threw = e instanceof RangeError;
          }
          return constrained.year === 1967 && constrained.monthCode === "M03" && constrained.day === 1 && threw;
        })()
        "#,
    );
}

/// "Adding 1 year to leap month M07L on day 30 constrains to M07 day 29" --
/// pins that the day-clamp and the leap-month-fallback both apply together
/// (M07L doesn't exist in 1939, *and* day 30 doesn't fit the fallback M07).
#[test]
fn add_years_to_a_non_recurring_leap_month_also_clamps_an_overflowing_day() {
    assert_true(
        r#"
        (function() {
          const calendar = "chinese";
          const leap193807L = Temporal.PlainDate.from({ year: 1938, monthCode: "M07L", day: 30, calendar });
          const result = leap193807L.add(new Temporal.Duration(1));
          return result.year === 1939 && result.monthCode === "M07" && result.day === 29;
        })()
        "#,
    );
}

/// "Adding years to go from one M04L to the next M04L": when the leap month
/// genuinely recurs in the landing year, its own identity is preserved
/// exactly, even under `overflow: "reject"`.
#[test]
fn add_years_between_two_years_that_both_have_the_same_leap_month() {
    assert_true(
        r#"
        (function() {
          const calendar = "chinese";
          const options = { overflow: "reject" };
          const start = Temporal.PlainDate.from({ year: 2012, monthCode: "M04L", day: 1, calendar }, options);
          const result = start.add(new Temporal.Duration(8), options);
          return result.year === 2020 && result.monthCode === "M04L" && result.day === 1;
        })()
        "#,
    );
}

/// Month-only arithmetic bubbles by real ordinal position within an
/// already-year-resolved date: "adding 2 months to M03 in leap year lands
/// in M04L (leap month)", and 3 months lands past it in M05 (not M06).
#[test]
fn add_months_bubbles_into_and_past_a_leap_month_by_ordinal_position() {
    assert_true(
        r#"
        (function() {
          const calendar = "chinese";
          const leap202003 = Temporal.PlainDate.from({ year: 2020, monthCode: "M03", day: 1, calendar });
          const plus1 = leap202003.add(new Temporal.Duration(0, 1));
          const plus2 = leap202003.add(new Temporal.Duration(0, 2));
          const plus3 = leap202003.add(new Temporal.Duration(0, 3));
          return plus1.monthCode === "M04" && plus1.year === 2020
              && plus2.monthCode === "M04L" && plus2.year === 2020
              && plus3.monthCode === "M05" && plus3.year === 2020;
        })()
        "#,
    );
}

/// "Adding 13 months to common-year M04 lands in leap-year M04L": crossing
/// a year boundary correctly counts that year's real (13-month) length.
#[test]
fn add_thirteen_months_crossing_a_leap_year_lands_on_the_leap_month() {
    assert_true(
        r#"
        (function() {
          const calendar = "chinese";
          const common201904 = Temporal.PlainDate.from({ year: 2019, monthCode: "M04", day: 1, calendar });
          const result = common201904.add(new Temporal.Duration(0, 13));
          return result.year === 2020 && result.monthCode === "M04L" && result.day === 1;
        })()
        "#,
    );
}

/// Subtracting months bubbles backward through a leap month the same way:
/// "Subtracting 2 months from M06 in leap year lands in M04L (leap month)".
#[test]
fn subtract_months_bubbles_backward_into_a_leap_month() {
    assert_true(
        r#"
        (function() {
          const calendar = "chinese";
          const leap202006 = Temporal.PlainDate.from({ year: 2020, monthCode: "M06", day: 1, calendar });
          const minus1 = leap202006.add(new Temporal.Duration(0, -1));
          const minus2 = leap202006.add(new Temporal.Duration(0, -2));
          const minus3 = leap202006.add(new Temporal.Duration(0, -3));
          return minus1.monthCode === "M05" && minus1.year === 2020
              && minus2.monthCode === "M04L" && minus2.year === 2020
              && minus3.monthCode === "M04" && minus3.year === 2020;
        })()
        "#,
    );
}

/// `hebrew` calendar sanity check (not just `chinese`): subtracting 1 year
/// from Adar I (`M05L`, only exists in a leap year) must constrain to the
/// *next* month, Adar (`M06`) -- confirmed directly against
/// `intl402/Temporal/PlainDate/prototype/add/leap-months-hebrew.js`'s own
/// "Adding 1 year to Adar I (M05L) lands in common-year Adar (M06) with
/// constrain" -- the *opposite*-looking answer from `chinese`/`dangi`'s own
/// "same number, drop leap flag" convention (`M03L` -> `M03`) exercised
/// above, since `hebrew` and `chinese`/`dangi` have genuinely different
/// underlying `icu_calendar` implementations
/// (`Hebrew::ordinal_from_month` vs. the shared `EastAsianTraditional`) --
/// see `plain_date.rs`'s `LeapMonthFallback` doc comment for the citations.
/// `overflow: "reject"` must still throw, exactly like the `chinese` case.
#[test]
fn hebrew_add_years_to_a_non_recurring_leap_month_picks_the_next_month() {
    assert_true(
        r#"
        (function() {
          const calendar = "hebrew";
          // 5784 (2023-2024) is a Hebrew leap year with an Adar I (M05L).
          const leapAdar1 = Temporal.PlainDate.from({ year: 5784, monthCode: "M05L", day: 1, calendar });
          const constrained = leapAdar1.add(new Temporal.Duration(1));
          let threw = false;
          try {
            leapAdar1.add(new Temporal.Duration(1), { overflow: "reject" });
          } catch (e) {
            threw = e instanceof RangeError;
          }
          return constrained.year === 5785 && constrained.monthCode === "M06" && constrained.day === 1 && threw;
        })()
        "#,
    );
}
