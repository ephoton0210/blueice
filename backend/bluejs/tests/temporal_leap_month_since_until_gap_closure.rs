// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for the real gap a prior pass's "closed" claim on
//! `plain_date.rs`'s `calendar_difference_date_leap_month` left open
//! (`development/browser_core/phase-26-ecma262-temporal/PLAN.md`'s
//! 2026-09-18 "Correction" note): running the real Test262 fixtures
//! directly showed every one of
//! `intl402/Temporal/{PlainDate,PlainDateTime,PlainYearMonth,ZonedDateTime}/prototype/{since,until}/leap-months-{chinese,dangi,hebrew}.js`
//! (24 files, 48 modes) still failing, even though the prior pass's own
//! hand-written `temporal_leap_month_calendar_difference.rs` test passed —
//! that test exercised a real fix, but not every assertion the actual
//! fixtures make. Cases here are taken directly, with the same expected
//! values, from those real fixture files, reproduced through the real
//! public `Temporal.{PlainDate,PlainDateTime,PlainYearMonth}.prototype.{since,until}`
//! surface (not `plain_date.rs`'s internals directly).
//!
//! The real bug this file pins: `calendar_difference_date_leap_month`'s
//! years-only correction only ever performed the *constrained* surpass
//! check (re-resolving the anchor's `Month` identity through the
//! leap-month fallback first). Gecko's own
//! `DifferenceNonISODateWithLeapMonth` performs a second, *unconstrained*
//! check first (comparing the anchor's raw, unresolved `Month` identity,
//! with no calendar resolution at all) — omitting it silently under- or
//! over-counts by exactly one month whenever the anchor itself is a
//! non-recurring leap month. A second, independent bug found while closing
//! this one: `Temporal.PlainYearMonth.prototype.since`/`until`'s own
//! dispatch (`vm/temporal.rs`'s `temporal_year_month_difference`) swapped
//! which date was `from`/`to` based on `since` instead of negating the
//! *result* — the exact antisymmetric-algorithm pitfall
//! `temporal_date_difference`'s own comment already documented — which
//! this file also pins directly.

use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

fn assert_true(source: &str) {
    assert_eq!(evaluate(source), Value::Bool(true), "{source}");
}

/// `intl402/Temporal/PlainDate/prototype/since/leap-months-chinese.js`'s
/// "M04L-M05 backwards is -1y -1mo (exhibits calendar-specific
/// constraining)" case: `2001-M04L` since `2002-M05`, `largestUnit:
/// "years"`, must be `-1y -1mo`, not `-1y` — the exact case the missing
/// unconstrained pre-check dropped a month from.
#[test]
fn plain_date_since_years_chinese_leap_month_anchor_needs_the_unconstrained_precheck() {
    assert_true(
        r#"
        (function() {
          const calendar = "chinese";
          const options = { overflow: "reject" };
          const one = Temporal.PlainDate.from({ year: 2001, monthCode: "M04L", day: 1, calendar }, options);
          const two = Temporal.PlainDate.from({ year: 2002, monthCode: "M05", day: 1, calendar }, options);
          const d = one.since(two, { largestUnit: "years" });
          return d.years === -1 && d.months === -1 && d.weeks === 0 && d.days === 0;
        })()
        "#,
    );
}

/// The same fixture's "M04L-M04 is 1y not 1y 1mo (exhibits
/// calendar-specific constraining)" case: `2001-M04L` since `2000-M04`
/// (backwards direction) must be exactly `1y`, not `1y 1mo` — the mirror
/// overcounting bug the same missing pre-check caused in the other
/// direction.
#[test]
fn plain_date_since_years_chinese_leap_month_anchor_does_not_overcount_the_other_direction() {
    assert_true(
        r#"
        (function() {
          const calendar = "chinese";
          const options = { overflow: "reject" };
          const one = Temporal.PlainDate.from({ year: 2001, monthCode: "M04L", day: 1, calendar }, options);
          const two = Temporal.PlainDate.from({ year: 2000, monthCode: "M04", day: 1, calendar }, options);
          const d = one.since(two, { largestUnit: "years" });
          return d.years === 1 && d.months === 0 && d.weeks === 0 && d.days === 0;
        })()
        "#,
    );
}

/// `intl402/Temporal/PlainDate/prototype/until/leap-months-chinese.js`'s
/// mirror case (`until`, not `since`): `2001-M04L` until `2002-M05` must be
/// `1y 1mo`.
#[test]
fn plain_date_until_years_chinese_leap_month_anchor_needs_the_unconstrained_precheck() {
    assert_true(
        r#"
        (function() {
          const calendar = "chinese";
          const options = { overflow: "reject" };
          const one = Temporal.PlainDate.from({ year: 2001, monthCode: "M04L", day: 1, calendar }, options);
          const two = Temporal.PlainDate.from({ year: 2002, monthCode: "M05", day: 1, calendar }, options);
          const d = one.until(two, { largestUnit: "years" });
          return d.years === 1 && d.months === 1 && d.weeks === 0 && d.days === 0;
        })()
        "#,
    );
}

/// `dangi` shares `chinese`'s `EastAsianTraditional` leap-month machinery,
/// but is checked independently (not generalized from `chinese` alone) per
/// this project's own established practice for this exact gap: same
/// worked example as the `chinese` fixture,
/// `intl402/Temporal/PlainDate/prototype/since/leap-months-dangi.js`'s own
/// "M04L-M05 backwards is -1y -1mo" case.
#[test]
fn plain_date_since_years_dangi_leap_month_anchor_needs_the_unconstrained_precheck() {
    assert_true(
        r#"
        (function() {
          const calendar = "dangi";
          const options = { overflow: "reject" };
          const one = Temporal.PlainDate.from({ year: 2001, monthCode: "M04L", day: 1, calendar }, options);
          const two = Temporal.PlainDate.from({ year: 2002, monthCode: "M05", day: 1, calendar }, options);
          const d = one.since(two, { largestUnit: "years" });
          return d.years === -1 && d.months === -1 && d.weeks === 0 && d.days === 0;
        })()
        "#,
    );
}

/// `hebrew` has a genuinely different leap-month shape than
/// `chinese`/`dangi` (`ConstrainMonthCode`'s own hardcoded `M05L` -> `M06`,
/// not the shared `EastAsianTraditional` same-number-drop-leap rule), so
/// it is checked independently too, per
/// `intl402/Temporal/PlainDate/prototype/since/leap-months-hebrew.js`'s own
/// "M05L-M06 is 12mo not 1y" case: `5784-M05L` since `5783-M06` (backwards)
/// must be `0y 12mo`, not `1y`.
#[test]
fn plain_date_since_years_hebrew_leap_month_anchor_needs_the_unconstrained_precheck() {
    assert_true(
        r#"
        (function() {
          const calendar = "hebrew";
          const options = { overflow: "reject" };
          const one = Temporal.PlainDate.from({ year: 5784, monthCode: "M05L", day: 1, calendar }, options);
          const two = Temporal.PlainDate.from({ year: 5783, monthCode: "M06", day: 1, calendar }, options);
          const d = one.since(two, { largestUnit: "years" });
          return d.years === 0 && d.months === 12 && d.weeks === 0 && d.days === 0;
        })()
        "#,
    );
}

/// `intl402/Temporal/PlainDateTime/prototype/since/leap-months-chinese.js`'s
/// own copy of the same "M04L-M05" case, pinning that `PlainDateTime`'s
/// `since`/`until` dispatch (which shares `calendar_difference_date`, not
/// a separate implementation) is fixed too, not just `PlainDate`'s.
#[test]
fn plain_date_time_since_years_chinese_leap_month_anchor_needs_the_unconstrained_precheck() {
    assert_true(
        r#"
        (function() {
          const calendar = "chinese";
          const options = { overflow: "reject" };
          const one = Temporal.PlainDateTime.from({ year: 2001, monthCode: "M04L", day: 1, hour: 12, calendar }, options);
          const two = Temporal.PlainDateTime.from({ year: 2002, monthCode: "M05", day: 1, hour: 12, calendar }, options);
          const d = one.since(two, { largestUnit: "years" });
          return d.years === -1 && d.months === -1 && d.weeks === 0 && d.days === 0;
        })()
        "#,
    );
}

/// `intl402/Temporal/ZonedDateTime/prototype/since/leap-months-chinese.js`'s
/// own copy, pinning `ZonedDateTime`'s `since`/`until` (via
/// `difference_zoned_date_time`, which reuses
/// `plain_date::calendar_difference_date` as-is) too.
#[test]
fn zoned_date_time_since_years_chinese_leap_month_anchor_needs_the_unconstrained_precheck() {
    assert_true(
        r#"
        (function() {
          const calendar = "chinese";
          const options = { overflow: "reject" };
          const one = Temporal.ZonedDateTime.from({ year: 2001, monthCode: "M04L", day: 1, hour: 12, minute: 34, timeZone: "UTC", calendar }, options);
          const two = Temporal.ZonedDateTime.from({ year: 2002, monthCode: "M05", day: 1, hour: 12, minute: 34, timeZone: "UTC", calendar }, options);
          const d = one.since(two, { largestUnit: "years" });
          return d.years === -1 && d.months === -1 && d.weeks === 0 && d.days === 0;
        })()
        "#,
    );
}

/// `intl402/Temporal/PlainYearMonth/prototype/since/leap-months-chinese.js`'s
/// own "M04L-M04 backwards is -12mo not -1y" case, taken at
/// `largestUnit: "years"` specifically: pins the second, independent bug
/// this pass found in `PlainYearMonth`'s own dispatch
/// (`temporal_year_month_difference`), which used to swap `from`/`to`
/// based on `since` (wrong for a non-antisymmetric algorithm) instead of
/// negating the result, *and* the `round_calendar_duration` bug where a
/// leap-month calendar's `years`/`months` split was flattened through
/// `years * 12 + months` and re-split via `/ 12, % 12` -- unsound once a
/// reported "year" can genuinely span 13 months.
#[test]
fn plain_year_month_since_years_chinese_leap_month_anchor_is_not_flattened_through_a_fixed_twelve_months_per_year(
) {
    assert_true(
        r#"
        (function() {
          const calendar = "chinese";
          const options = { overflow: "reject" };
          const one = Temporal.PlainYearMonth.from({ year: 2001, monthCode: "M04L", calendar }, options);
          const two = Temporal.PlainYearMonth.from({ year: 2002, monthCode: "M04", calendar }, options);
          const d = one.since(two, { largestUnit: "years" });
          return d.years === 0 && d.months === -12 && d.weeks === 0 && d.days === 0;
        })()
        "#,
    );
}

/// `intl402/Temporal/PlainYearMonth/prototype/until/leap-months-hebrew.js`-style
/// coverage (`until`, not `since`, and `hebrew` rather than `chinese`) for
/// the same `PlainYearMonth` dispatch fix, so the fix is pinned on both
/// `since`/`until` and on a second, structurally different leap calendar.
#[test]
fn plain_year_month_until_years_hebrew_leap_month_anchor_is_not_flattened_through_a_fixed_twelve_months_per_year(
) {
    assert_true(
        r#"
        (function() {
          const calendar = "hebrew";
          const options = { overflow: "reject" };
          const one = Temporal.PlainYearMonth.from({ year: 5784, monthCode: "M05L", calendar }, options);
          const two = Temporal.PlainYearMonth.from({ year: 5783, monthCode: "M06", calendar }, options);
          const d = one.until(two, { largestUnit: "years" });
          return d.years === 0 && d.months === -12 && d.weeks === 0 && d.days === 0;
        })()
        "#,
    );
}

/// `built-ins/Temporal/PlainYearMonth/prototype/since/roundingmode-ceil.js`'s
/// own worked example: an ISO (non-leap-month) calendar, `smallestUnit:
/// "years"`, `roundingMode: "ceil"`, in *both* directions. This is the
/// second, independent regression this pass's own fix for the
/// `temporal_year_month_difference` from/to-swap bug introduced and then
/// fixed: negating the result for `since` without also reflecting
/// direction-sensitive rounding modes (`ceil`/`floor` round toward a fixed
/// end of the real number line, not of whichever internal `from`/`to`
/// direction was computed) silently swapped `ceil` and `floor` whenever
/// `since` negated a non-exact multiple.
#[test]
fn plain_year_month_since_roundingmode_ceil_rounds_toward_positive_infinity_in_both_directions() {
    assert_true(
        r#"
        (function() {
          const earlier = new Temporal.PlainYearMonth(2019, 1);
          const later = new Temporal.PlainYearMonth(2021, 9);
          const positive = later.since(earlier, { smallestUnit: "years", roundingMode: "ceil" });
          const negative = earlier.since(later, { smallestUnit: "years", roundingMode: "ceil" });
          return positive.years === 3 && negative.years === -2;
        })()
        "#,
    );
}

/// The same fixture's `roundingMode: "floor"` counterpart -- `floor` must
/// round the opposite way in each direction from `ceil` above.
#[test]
fn plain_year_month_since_roundingmode_floor_rounds_toward_negative_infinity_in_both_directions() {
    assert_true(
        r#"
        (function() {
          const earlier = new Temporal.PlainYearMonth(2019, 1);
          const later = new Temporal.PlainYearMonth(2021, 9);
          const positive = later.since(earlier, { smallestUnit: "years", roundingMode: "floor" });
          const negative = earlier.since(later, { smallestUnit: "years", roundingMode: "floor" });
          return positive.years === 2 && negative.years === -3;
        })()
        "#,
    );
}
