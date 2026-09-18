// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for the leap-month-calendar (`chinese`/`dangi`/
//! `hebrew`) `since`/`until` gap Phase 26 Stage 2's `PlainDate`/
//! `PlainDateTime` second pass documented and deliberately left open
//! (`development/browser_core/phase-26-ecma262-temporal/PLAN.md`):
//! `vm/temporal/plain_date.rs`'s `calendar_difference_date_leap_month`
//! previously compared candidates by raw ordinal month rather than by
//! `monthCode` identity, which misorders whenever the two years being
//! compared put their leap month in a different position.
//!
//! Cases are taken directly, with the same expected values, from Test262's
//! `intl402/Temporal/PlainDate/prototype/since/leap-months-chinese.js` and
//! `intl402/Temporal/PlainDate/prototype/until/wrapping-at-end-of-month-chinese.js`,
//! reproduced through the real public `Temporal.PlainDate.prototype.{since,until}`
//! surface (not `plain_date.rs`'s internals directly).

use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

fn assert_true(source: &str) {
    assert_eq!(evaluate(source), Value::Bool(true), "{source}");
}

/// `leap-months-chinese.js`: 2001 is a leap year with an `M04L` leap month.
/// `common1Month4.since(leapMonth4, { largestUnit: "months" })` must be
/// `-12` months (2000 M04 -> 2001 M04, skipping the *later* M04L that year),
/// not `-13` -- the ordinal-month comparison this fixes previously mis-added
/// an extra month because M04L's ordinal position (5) sits between M04's
/// (4) and M05's (6) only in the leap year, misordering the candidate
/// bubbling.
#[test]
fn since_months_chinese_common_to_leap_year_same_month_code_is_twelve_months() {
    assert_true(
        r#"
        (function() {
          const calendar = "chinese";
          const options = { overflow: "reject" };
          const one = Temporal.PlainDate.from({ year: 2000, monthCode: "M04", day: 1, calendar }, options);
          const two = Temporal.PlainDate.from({ year: 2001, monthCode: "M04", day: 1, calendar }, options);
          const d = one.since(two, { largestUnit: "months" });
          return d.years === 0 && d.months === -12 && d.weeks === 0 && d.days === 0;
        })()
        "#,
    );
}

/// Same fixture: `leapMonth4.since(common2Month4)` (2001 M04L-adjacent M04
/// forward to 2002 M04) must be `-13` months, not `-12` -- the leap month
/// that year genuinely adds an extra month between the two `M04`s.
#[test]
fn since_months_chinese_leap_year_to_common_same_month_code_is_thirteen_months() {
    assert_true(
        r#"
        (function() {
          const calendar = "chinese";
          const options = { overflow: "reject" };
          const one = Temporal.PlainDate.from({ year: 2001, monthCode: "M04", day: 1, calendar }, options);
          const two = Temporal.PlainDate.from({ year: 2002, monthCode: "M04", day: 1, calendar }, options);
          const d = one.since(two, { largestUnit: "months" });
          return d.years === 0 && d.months === -13 && d.weeks === 0 && d.days === 0;
        })()
        "#,
    );
}

/// `leapMonth4.since(common1Month4, { largestUnit: "years" })` (backwards:
/// 2001-M04 -> 2000-M04) must be `1y` (forward direction, negated) -- the
/// fixture's "M04-M04 leap-common is 1y" case, `largestUnit: "years"`.
#[test]
fn since_years_chinese_leap_to_common_same_month_code_is_one_year() {
    assert_true(
        r#"
        (function() {
          const calendar = "chinese";
          const options = { overflow: "reject" };
          const one = Temporal.PlainDate.from({ year: 2001, monthCode: "M04", day: 1, calendar }, options);
          const two = Temporal.PlainDate.from({ year: 2000, monthCode: "M04", day: 1, calendar }, options);
          const d = one.since(two, { largestUnit: "years" });
          return d.years === 1 && d.months === 0 && d.weeks === 0 && d.days === 0;
        })()
        "#,
    );
}

/// `leapMonth4L.since(common2Month4)`: `M04L` (2001) to `M04` (2002) is
/// `-12` months, not `-1` year -- the fixture's own note that this
/// "exhibits calendar-specific constraining" (the leap month doesn't recur
/// every year, so adding a year to `M04L` constrains down to the regular
/// `M04`, folding what would otherwise be a whole year into 12 months plus
/// no separate leap-month remainder).
#[test]
fn since_months_chinese_leap_month_code_to_next_year_common_is_twelve_months_not_one_year() {
    assert_true(
        r#"
        (function() {
          const calendar = "chinese";
          const options = { overflow: "reject" };
          const one = Temporal.PlainDate.from({ year: 2001, monthCode: "M04L", day: 1, calendar }, options);
          const two = Temporal.PlainDate.from({ year: 2002, monthCode: "M04", day: 1, calendar }, options);
          const years = one.since(two, { largestUnit: "years" });
          const months = one.since(two, { largestUnit: "months" });
          return years.years === 0 && years.months === -12 && years.weeks === 0 && years.days === 0
              && months.years === 0 && months.months === -12 && months.weeks === 0 && months.days === 0;
        })()
        "#,
    );
}

/// `wrapping-at-end-of-month-chinese.js`'s first case: `M05-29` (a 30-day
/// month) to `M06-29` (a 29-day month) in 2023 is exactly one month in
/// *every* rounding-relevant `largestUnit`, but `M05-30` (the last day of
/// that 30-day month) to the same `M06-29` is 29 days, not one month --
/// pins that the "unconstrained candidate" surpass check (not a
/// day-clamped one) is preserved in the monthCode-aware rewrite.
#[test]
fn until_chinese_wrapping_at_end_of_month_matches_the_fixed_day_count() {
    assert_true(
        r#"
        (function() {
          const calendar = "chinese";
          const options = { overflow: "reject" };
          const end = Temporal.PlainDate.from({ year: 2023, monthCode: "M06", day: 29, calendar }, options);
          const start29 = Temporal.PlainDate.from({ year: 2023, monthCode: "M05", day: 29, calendar }, options);
          const start30 = Temporal.PlainDate.from({ year: 2023, monthCode: "M05", day: 30, calendar }, options);
          const d29 = start29.until(end, { largestUnit: "months" });
          const d30 = start30.until(end, { largestUnit: "months" });
          return d29.years === 0 && d29.months === 1 && d29.weeks === 0 && d29.days === 0
              && d30.years === 0 && d30.months === 0 && d30.weeks === 0 && d30.days === 29;
        })()
        "#,
    );
}

/// The same fixture's leap-month-specific wrapping case: 30-day `M04` to
/// 29-day `M04L` (2020) is one month; the last day of `M04` (day 30) to
/// the same `M04L` end is 29 days, not one month.
#[test]
fn until_chinese_wrapping_across_a_leap_month_matches_the_fixed_day_count() {
    assert_true(
        r#"
        (function() {
          const calendar = "chinese";
          const options = { overflow: "reject" };
          const end = Temporal.PlainDate.from({ year: 2020, monthCode: "M04L", day: 29, calendar }, options);
          const start29 = Temporal.PlainDate.from({ year: 2020, monthCode: "M04", day: 29, calendar }, options);
          const start30 = Temporal.PlainDate.from({ year: 2020, monthCode: "M04", day: 30, calendar }, options);
          const d29 = start29.until(end, { largestUnit: "months" });
          const d30 = start30.until(end, { largestUnit: "months" });
          return d29.years === 0 && d29.months === 1 && d29.weeks === 0 && d29.days === 0
              && d30.years === 0 && d30.months === 0 && d30.weeks === 0 && d30.days === 29;
        })()
        "#,
    );
}

/// A multi-year span crossing a leap month, `largestUnit: "months"`:
/// 2021-M05-29 to 2023-M09-29 is 29 months (the fixture's own worked
/// example) -- exercises the "fold years into months, honoring each
/// crossed year's own real month count" branch.
#[test]
fn until_chinese_multi_year_span_folds_leap_years_into_months_correctly() {
    assert_true(
        r#"
        (function() {
          const calendar = "chinese";
          const options = { overflow: "reject" };
          const end = Temporal.PlainDate.from({ year: 2023, monthCode: "M09", day: 29, calendar }, options);
          const start = Temporal.PlainDate.from({ year: 2021, monthCode: "M05", day: 29, calendar }, options);
          const months = start.until(end, { largestUnit: "months" });
          const years = start.until(end, { largestUnit: "years" });
          return months.years === 0 && months.months === 29 && months.weeks === 0 && months.days === 0
              && years.years === 2 && years.months === 4 && years.weeks === 0 && years.days === 0;
        })()
        "#,
    );
}

/// `hebrew` calendar sanity check (not just `chinese`): Adar I (`M05L`)
/// only exists in a Hebrew leap year, so a monthCode-based difference must
/// treat it as a genuinely distinct identity from plain Adar (`M05`), not
/// an ordinal-position coincidence. `dangi` shares `plain_date.rs`'s own
/// `calendar_has_leap_months` set with `chinese` and is covered indirectly
/// by using the identical algorithm; this exercises `hebrew` specifically
/// since it has a different `monthsPerYear`/leap-month position shape.
#[test]
fn until_hebrew_leap_month_calendar_uses_month_code_identity_not_ordinal_position() {
    assert_true(
        r#"
        (function() {
          const calendar = "hebrew";
          const options = { overflow: "reject" };
          // 5784 (2023-2024) is a Hebrew leap year; 5783 and 5785 are not.
          const commonAdar = Temporal.PlainDate.from({ year: 5783, monthCode: "M05", day: 1, calendar }, options);
          const leapAdar1 = Temporal.PlainDate.from({ year: 5784, monthCode: "M05L", day: 1, calendar }, options);
          const leapAdar2 = Temporal.PlainDate.from({ year: 5784, monthCode: "M06", day: 1, calendar }, options);
          const nextCommonAdar = Temporal.PlainDate.from({ year: 5785, monthCode: "M05", day: 1, calendar }, options);
          // From the year before the leap year's Adar (M05, ordinal 5 in a
          // common year) to that leap year's Adar II (M06) is one year
          // *plus two months*, not one year flat: by ordinal position, one
          // year lands on the leap year's own M05 (still ordinal 5 -- the
          // leap month doesn't shift the months before it), and M06 sits
          // two ordinal positions further (M05L at ordinal 6, M06 itself
          // at ordinal 7), even though "M06" is M05's very next monthCode.
          const a = commonAdar.until(leapAdar2, { largestUnit: "years" });
          // From the leap year's own Adar I to the following year's plain
          // Adar (M05) is 12 months exactly -- the leap month doesn't
          // recur, so this skips straight past it.
          const b = leapAdar1.until(nextCommonAdar, { largestUnit: "months" });
          return a.years === 1 && a.months === 2 && a.weeks === 0 && a.days === 0
              && b.years === 0 && b.months === 12 && b.weeks === 0 && b.days === 0;
        })()
        "#,
    );
}
