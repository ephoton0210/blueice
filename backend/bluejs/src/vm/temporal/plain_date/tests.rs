// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Unit tests for the `plain_date` module tree (moved verbatim from the
//! pre-split `plain_date.rs`'s inline `mod tests`).

use super::super::epoch::CivilDate;
use super::calendar_add::*;
use super::calendar_difference::*;
use super::format::*;
use super::iso_date::*;
use super::month_structure::*;
use super::round_duration::*;
use icu_calendar::options::Overflow as IcuOverflow;
use icu_calendar::types::Month;
use icu_calendar::{AnyCalendarKind, Iso};

#[test]
fn computes_iso_day_of_week_matching_a_known_monday() {
    // Test262's PlainDate/prototype/dayOfWeek/basic.js: 1976-11-15 is a
    // Monday (dayOfWeek 1) and the week runs through 1976-11-21 (7).
    for offset in 1_u8..=7 {
        assert_eq!(iso_day_of_week((1976, 11, 14 + offset)), offset);
    }
}

#[test]
fn computes_iso_week_of_year_across_a_year_boundary() {
    // Test262's PlainDate/prototype/weekOfYear/basic.js.
    for day in 29..=31 {
        assert_eq!(iso_week_of_year((1975, 12, day)), (1, 1976));
    }
    for day in 1..=4 {
        assert_eq!(iso_week_of_year((1976, 1, day)), (1, 1976));
    }
    for day in 27..=31 {
        assert_eq!(iso_week_of_year((1976, 12, day)), (53, 1976));
    }
    for day in 1..=2 {
        assert_eq!(iso_week_of_year((1977, 1, day)), (53, 1976));
    }
}

#[test]
fn reports_days_and_leap_years_matching_known_facts() {
    assert!(is_iso_leap_year(1976));
    assert!(!is_iso_leap_year(1977));
    assert!(is_iso_leap_year(2000));
    assert!(!is_iso_leap_year(1900));
    assert_eq!(iso_days_in_month(2020, 2), 29);
    assert_eq!(iso_days_in_month(2021, 2), 28);
}

#[test]
fn round_trips_epoch_days_across_a_range_of_dates() {
    for date in [
        (1970, 1, 1),
        (2000, 2, 29),
        (1969, 12, 31),
        (1976, 11, 18),
        (-271_821, 4, 19),
        (275_760, 9, 13),
    ] {
        assert_eq!(epoch_days_to_iso_date(iso_date_to_epoch_days(date)), date);
    }
}

#[test]
fn adds_years_months_before_weeks_and_days_like_the_spec_pins() {
    // PlainDate/prototype/add/basic.js's own worked examples.
    assert_eq!(
        add_iso_date((1976, 11, 18), 43, 0, 0, 0, false),
        Some((2019, 11, 18))
    );
    assert_eq!(
        add_iso_date((1976, 11, 18), 0, 3, 0, 0, false),
        Some((1977, 2, 18))
    );
    assert_eq!(
        add_iso_date((1976, 11, 18), 0, 0, 0, 20, false),
        Some((1976, 12, 8))
    );
    assert_eq!(
        add_iso_date((2019, 1, 31), 0, 1, 0, 0, false),
        Some((2019, 2, 28))
    );
    assert_eq!(
        add_iso_date((2020, 2, 29), 1, 0, 0, 0, false),
        Some((2021, 2, 28))
    );
    assert_eq!(
        add_iso_date((2020, 2, 29), 4, 0, 0, 0, false),
        Some((2024, 2, 29))
    );
}

#[test]
fn rejects_an_overflowing_day_only_in_reject_mode() {
    assert_eq!(add_iso_date((2019, 1, 31), 0, 1, 0, 0, true), None);
    assert_eq!(
        add_iso_date((2019, 1, 31), 0, 1, 0, 0, false),
        Some((2019, 2, 28))
    );
}

#[test]
fn differences_two_iso_dates_by_largest_unit() {
    assert_eq!(
        difference_iso_date((1976, 11, 18), (2019, 11, 18), DateUnit::Year),
        (43, 0, 0, 0)
    );
    assert_eq!(
        difference_iso_date((1976, 11, 18), (2019, 11, 18), DateUnit::Month),
        (0, 43 * 12, 0, 0)
    );
    assert_eq!(
        difference_iso_date((2021, 2, 19), (2021, 3, 8), DateUnit::Day),
        (0, 0, 0, 17)
    );
    assert_eq!(
        difference_iso_date((2021, 2, 19), (2021, 3, 8), DateUnit::Week),
        (0, 0, 2, 3)
    );
}

#[test]
fn differences_are_the_inverse_of_add_iso_date() {
    for (start, end, unit) in [
        ((1976, 11, 18), (2019, 11, 18), DateUnit::Year),
        ((2019, 1, 31), (2019, 2, 28), DateUnit::Month),
        ((2021, 2, 19), (2021, 3, 8), DateUnit::Day),
        ((2021, 2, 19), (2021, 3, 8), DateUnit::Week),
        ((2019, 11, 18), (1976, 11, 18), DateUnit::Year),
    ] {
        let (years, months, weeks, days) = difference_iso_date(start, end, unit);
        assert_eq!(
            add_iso_date(start, years, months, weeks, days, false),
            Some(end),
            "{start:?} -> {end:?} via {unit:?}: {years} {months} {weeks} {days}"
        );
    }
}

#[test]
fn formats_iso_dates_with_the_six_digit_extended_year_rule() {
    assert_eq!(format_iso_date((2000, 5, 2)), "2000-05-02");
    assert_eq!(format_iso_date((-271_821, 4, 19)), "-271821-04-19");
    assert_eq!(format_iso_date((275_760, 9, 13)), "+275760-09-13");
}

#[test]
fn formats_calendar_annotations_per_the_showcalendar_option() {
    assert_eq!(
        format_calendar_annotation("iso8601", ShowCalendar::Auto),
        ""
    );
    assert_eq!(
        format_calendar_annotation("iso8601", ShowCalendar::Always),
        "[u-ca=iso8601]"
    );
    assert_eq!(
        format_calendar_annotation("iso8601", ShowCalendar::Critical),
        "[!u-ca=iso8601]"
    );
    assert_eq!(
        format_calendar_annotation("iso8601", ShowCalendar::Never),
        ""
    );
    assert_eq!(
        format_calendar_annotation("hebrew", ShowCalendar::Auto),
        "[u-ca=hebrew]"
    );
}

#[test]
fn calendar_add_date_matches_the_pure_iso_fast_path_for_iso8601() {
    assert_eq!(
        calendar_add_date(AnyCalendarKind::Iso, (1976, 11, 18), 43, 0, 0, 0, false),
        add_iso_date((1976, 11, 18), 43, 0, 0, 0, false)
    );
}

#[test]
fn rounds_calendar_durations_matching_a_real_test262_fixture() {
    // Test262's PlainDate/prototype/until/roundingmode-ceil.js: 2019-01-08
    // until 2021-09-07, rounded up (roundingMode "ceil") to each
    // smallestUnit in turn, with largestUnit implicitly bumped to match.
    use blueice_ecma402::NumberRoundingMode::Ceil;
    let start = (2019, 1, 8);
    let end = (2021, 9, 7);
    for (unit, expected_positive, expected_negative) in [
        (DateUnit::Year, (3, 0, 0, 0), (-2, 0, 0, 0)),
        (DateUnit::Month, (0, 32, 0, 0), (0, -31, 0, 0)),
        (DateUnit::Week, (0, 0, 139, 0), (0, 0, -139, 0)),
        (DateUnit::Day, (0, 0, 0, 973), (0, 0, 0, -973)),
    ] {
        assert_eq!(
            round_calendar_duration(AnyCalendarKind::Iso, start, end, unit, unit, 1, Ceil),
            expected_positive,
            "{unit:?} positive"
        );
        assert_eq!(
            round_calendar_duration(AnyCalendarKind::Iso, end, start, unit, unit, 1, Ceil),
            expected_negative,
            "{unit:?} negative"
        );
    }
}

#[test]
fn rounding_an_exact_multiple_of_the_larger_unit_adds_no_spurious_remainder() {
    // Test262's PlainDate/prototype/since/exact-multiple-of-larger-unit.js:
    // a `{ largestUnit, smallestUnit }` pair where the *unrounded*
    // difference is already an exact whole `largestUnit` (here, exactly
    // one month/one year) must report that exactly, in every rounding
    // mode — not a `smallestUnit`-sized wobble computed by bubbling
    // `smallestUnit` steps from scratch.
    use blueice_ecma402::NumberRoundingMode::{Ceil, Expand, Floor, HalfEven, HalfExpand, Trunc};
    let start = (2012, 1, 1);
    for mode in [Ceil, Floor, Expand, Trunc, HalfExpand, HalfEven] {
        assert_eq!(
            round_calendar_duration(
                AnyCalendarKind::Iso,
                start,
                (2012, 2, 1),
                DateUnit::Month,
                DateUnit::Week,
                1,
                mode
            ),
            (0, 1, 0, 0),
            "P1M weeks..months {mode:?}"
        );
        assert_eq!(
            round_calendar_duration(
                AnyCalendarKind::Iso,
                start,
                (2013, 1, 1),
                DateUnit::Year,
                DateUnit::Month,
                1,
                mode
            ),
            (1, 0, 0, 0),
            "P1Y months..years {mode:?}"
        );
    }
}

#[test]
fn calendar_add_date_carries_years_in_a_non_iso_calendar() {
    // Gregorian is a 12-month solar calendar offset from ISO by no
    // fields at all, so adding a year should land on the same ISO
    // month/day one calendar year later (calendar_add_date must not
    // silently no-op for a non-ISO AnyCalendarKind).
    let result = calendar_add_date(AnyCalendarKind::Gregorian, (2020, 3, 1), 1, 0, 0, 0, false);
    assert_eq!(result, Some((2021, 3, 1)));
}

/// Regression for a real bug in this module's earlier estimate-then-
/// bubble `difference_iso_date`/`calendar_difference_date`, found via
/// Test262's
/// `intl402/Temporal/PlainDate/prototype/since/wrapping-at-end-of-month-*.js`:
/// `Jan 29 -> Feb 28` (a non-leap year) must report a 30-day difference,
/// **not** one month, because the *unconstrained* `Jan 29 + 1 month =
/// Feb 29` candidate surpasses `Feb 28`, even though `Feb 29`
/// constrained down to `Feb 28` would land exactly on it. The earlier
/// implementation compared an already-`calendar_add_date`-constrained
/// candidate (which silently clips `Feb 29` to `Feb 28` before the
/// comparison ever happens) and got this wrong. Covers both the pure
/// ISO path (`difference_iso_date`) and the non-ISO-aligned fixed-
/// months path (`calendar_difference_date` with `Gregorian`, which
/// `wrapping-at-end-of-month-*.js`'s own `buddhist`/`gregory`/etc.
/// variants exercise) since both were rewritten together.
#[test]
fn month_difference_does_not_constrain_before_detecting_an_end_of_month_overshoot() {
    for unit in [DateUnit::Year, DateUnit::Month] {
        assert_eq!(
            difference_iso_date((2020, 1, 29), (2020, 2, 28), unit),
            (0, 0, 0, 30),
            "ISO Jan 29 -> Feb 28, {unit:?}"
        );
        assert_eq!(
            calendar_difference_date(
                AnyCalendarKind::Gregorian,
                (2020, 1, 29),
                (2020, 2, 28),
                unit
            ),
            (0, 0, 0, 30),
            "Gregorian Jan 29 -> Feb 28, {unit:?}"
        );
        assert_eq!(
            calendar_difference_date(AnyCalendarKind::Persian, (2020, 1, 29), (2020, 2, 28), unit),
            calendar_difference_date_fixed_months(
                AnyCalendarKind::Persian,
                (2020, 1, 29),
                (2020, 2, 28),
                unit
            ),
            "Persian (fixed-months path) Jan 29 -> Feb 28 is internally consistent, {unit:?}"
        );
    }
    // Jan 30 -> Feb 28 is 29 days (one day closer than Jan 29's case),
    // and Jan 31 -> Feb 28 is 28 days -- both from the same fixture,
    // pinning the exact day-count, not just "not a whole month".
    assert_eq!(
        difference_iso_date((2020, 1, 30), (2020, 2, 28), DateUnit::Year),
        (0, 0, 0, 29)
    );
    assert_eq!(
        difference_iso_date((2020, 1, 31), (2020, 2, 28), DateUnit::Year),
        (0, 0, 0, 28)
    );
}

/// Regression for a real panic in an earlier draft of
/// [`calendar_difference_date_fixed_months`]: its intermediate
/// year/month normalization used a single `if bm > 12 {} else if bm < 1
/// {}` step (mirroring Gecko's own `DifferenceNonISODate`), which is
/// only sound if the preceding correction bounds the candidate month
/// tightly enough that one step always suffices -- Gecko's own
/// production code apparently relies on invariants this port's simpler
/// `MONTHS_PER_YEAR`-only (no monthCode) representation doesn't
/// preserve. Found via a real crash on
/// `intl402/Temporal/PlainDate/prototype/since/basic-indian.js`
/// ("negative 61 years, 3 months and 17 days", `date19430716` to
/// `date18820330`), not by inspection. Fixed by a full `div_euclid`/
/// `rem_euclid` normalize instead of the single-step version. This test
/// exercises every fixed-months calendar across a multi-decade span
/// with a large year delta, the shape that triggered the crash.
#[test]
fn fixed_months_difference_never_panics_across_a_large_multi_decade_span() {
    for calendar in [
        AnyCalendarKind::Coptic,
        AnyCalendarKind::Ethiopian,
        AnyCalendarKind::EthiopianAmeteAlem,
        AnyCalendarKind::Indian,
        AnyCalendarKind::HijriTabularTypeIIFriday,
        AnyCalendarKind::HijriTabularTypeIIThursday,
        AnyCalendarKind::HijriUmmAlQura,
        AnyCalendarKind::Persian,
    ] {
        for unit in [DateUnit::Year, DateUnit::Month] {
            // date19430716 -> date18820330, the exact pair
            // `basic-indian.js` panicked on.
            let (years, months, weeks, days) =
                calendar_difference_date(calendar, (1943, 7, 16), (1882, 3, 30), unit);
            assert_eq!(weeks, 0);
            // Every field should be non-positive (end is before start)
            // and the whole-duration sign should be consistent with a
            // backward difference.
            assert!(
                years <= 0 && months <= 0 && days <= 0,
                "{calendar:?} {unit:?}: {years} {months} {days}"
            );
        }
    }
}

/// Builds the ISO [`CivilDate`] for a leap-month calendar's own
/// `(year, Month, day)` identity, for use as test input — thin wrapper
/// around [`calendar_date_from_month`] so these tests never need a
/// hand-computed ISO date.
fn civil_date_from_month_code(
    calendar: AnyCalendarKind,
    year: i64,
    month: Month,
    day: i64,
) -> CivilDate {
    let date = calendar_date_from_month(calendar, year, month, day, IcuOverflow::Reject)
        .expect("test fixture month codes are always valid for their stated year");
    let iso = date.to_calendar(Iso);
    (
        iso.year().extended_year(),
        iso.month().number(),
        iso.day_of_month().0,
    )
}

/// A non-recurring leap month resolves via `icu_calendar`'s own native,
/// per-calendar fallback under `Constrain` — for `Chinese`/`Dangi` this
/// *keeps the same month number and drops the leap flag* (`M04L` ->
/// `M04`, not "the next month"; see [`calendar_date_from_month`]'s own
/// doc comment for the full Gecko-sourced explanation, and
/// `calendar_add_date_leap_month_constrains_a_non_recurring_leap_month_to_the_same_number`
/// below for the same fact one layer up). An earlier version of this
/// test asserted the *opposite* ("the next month", `M04L` -> `M05`) —
/// that assertion was itself the bug this project's `since`/`until`
/// leap-month gap-closure pass found and fixed; it did not describe
/// real `icu_calendar`/Gecko behavior. Under `Reject`, the same request
/// must fail outright.
#[test]
fn calendar_date_from_month_constrains_a_non_recurring_chinese_leap_month_to_the_same_number() {
    let constrained = calendar_date_from_month(
        AnyCalendarKind::Chinese,
        2002,
        Month::leap(4),
        1,
        IcuOverflow::Constrain,
    )
    .expect("a non-recurring leap month still resolves under Constrain");
    assert_eq!(constrained.month().to_input(), Month::new(4));

    assert!(
        calendar_date_from_month(
            AnyCalendarKind::Chinese,
            2002,
            Month::leap(4),
            1,
            IcuOverflow::Reject
        )
        .is_none(),
        "a non-recurring leap month must be rejected under Overflow::Reject"
    );
}

/// A leap month that *does* recur in the requested year must resolve to
/// its own genuine leap-month ordinal, not the fallback — 2001 is a
/// real Chinese leap year with an `M04L` (per
/// `intl402/Temporal/PlainDate/prototype/since/leap-months-chinese.js`'s
/// own comment), landing between `M04` (ordinal 4) and `M05` (ordinal
/// 6, since the leap month itself is ordinal 5).
#[test]
fn calendar_date_from_month_resolves_a_genuinely_recurring_leap_month_to_its_own_ordinal() {
    let leap = calendar_date_from_month(
        AnyCalendarKind::Chinese,
        2001,
        Month::leap(4),
        1,
        IcuOverflow::Reject,
    )
    .expect("2001 has a real M04L");
    assert_eq!(leap.month().ordinal, 5);
    assert!(leap.month().to_input().is_leap());
}

/// [`add_year_month_duration_leap_month`] must preserve the anchor's own
/// `Month` identity across a pure-`years` shift (no `months` component)
/// — adding one year to `M04`(2000) lands on `M04`(2001), the same
/// `monthCode`, even though 2001 inserts a leap month right after it.
#[test]
fn add_year_month_duration_leap_month_preserves_identity_across_a_pure_year_shift() {
    let (year, month) = add_year_month_duration_leap_month(
        AnyCalendarKind::Chinese,
        2000,
        Month::new(4),
        1,
        0,
        IcuOverflow::Constrain,
    )
    .expect("a representable in-range shift always succeeds");
    assert_eq!((year, month), (2001, Month::new(4)));
}

/// [`add_year_month_duration_leap_month`] bubbling by one month from a
/// leap year's own `M04` must land on that same year's `M04L` (the
/// month immediately following it that year), not `M05` — pinning that
/// the month-bubbling walk resolves by real ordinal position within an
/// already-year-resolved date, not by skipping straight to the next
/// non-leap `monthCode`.
#[test]
fn add_year_month_duration_leap_month_bubbles_into_the_leap_month_itself() {
    let (year, month) = add_year_month_duration_leap_month(
        AnyCalendarKind::Chinese,
        2000,
        Month::new(4),
        1,
        1,
        IcuOverflow::Constrain,
    )
    .expect("a representable in-range shift always succeeds");
    assert_eq!(year, 2001);
    assert_eq!(month, Month::leap(4));
}

/// A `months` component must not launder a `Reject` overflow into
/// `Constrain`: Chinese 2012 has a leap `M04L`, 2013 does not, so shifting
/// `M04L` by a year lands on a month that does not exist. `Reject` refuses
/// it whether or not `months` follows; `Constrain` falls back to `M04` and
/// then walks the extra month.
#[test]
fn add_year_month_duration_leap_month_honors_reject_when_months_follow_a_year_shift() {
    let shift = |months, overflow| {
        add_year_month_duration_leap_month(
            AnyCalendarKind::Chinese,
            2012,
            Month::leap(4),
            1,
            months,
            overflow,
        )
    };
    assert_eq!(shift(0, IcuOverflow::Reject), Some((2013, Month::leap(4))));
    assert_eq!(shift(1, IcuOverflow::Reject), None);
    assert_eq!(shift(-1, IcuOverflow::Reject), None);
    assert_eq!(
        shift(1, IcuOverflow::Constrain),
        Some((2013, Month::new(5)))
    );
}

/// [`calendar_add_date`]'s own leap-month branch, host-neutral layer:
/// adding 1 year to `M03L`(1966) under `Constrain` lands on `M03`(1967)
/// -- the same-number, drop-the-leap-flag native fallback, not "the next
/// month" -- matching
/// `intl402/Temporal/PlainDate/prototype/add/leap-months-chinese.js`'s
/// own worked example (the VM-level integration test in
/// `backend/bluejs/tests/temporal_leap_month_calendar_add.rs` exercises
/// the same fact through the real `Temporal.PlainDate.prototype.add`
/// surface, including the `overflow: "reject"` throw this test's
/// `Constrain` case does not cover).
#[test]
fn calendar_add_date_leap_month_constrains_a_non_recurring_leap_month_to_the_same_number() {
    let anchor = civil_date_from_month_code(AnyCalendarKind::Chinese, 1966, Month::leap(3), 1);
    let landed = calendar_add_date(AnyCalendarKind::Chinese, anchor, 1, 0, 0, 0, false)
        .expect("constrain-mode add always succeeds for a representable date");
    let (year, month, day) = calendar_month_identity(AnyCalendarKind::Chinese, landed);
    assert_eq!((year, month, day), (1967, Month::new(3), 1));

    assert_eq!(
        calendar_add_date(AnyCalendarKind::Chinese, anchor, 1, 0, 0, 0, true),
        None,
        "overflow: reject must throw when M03L does not recur in the landing year"
    );
}

/// The same scenario on `hebrew`: adding 1 year to Adar I (`M05L`, 5784)
/// under `Constrain` lands on Adar (`M06`, 5785) -- the *next* month,
/// not `M05` -- since `icu_calendar`'s own `Hebrew::ordinal_from_month`
/// natively implements that convention (unlike `Chinese`/`Dangi`'s
/// shared `EastAsianTraditional` implementation, exercised above).
/// Matches `intl402/Temporal/PlainDate/prototype/add/leap-months-hebrew.js`'s
/// "Adding 1 year to Adar I (M05L) lands in common-year Adar (M06) with
/// constrain" and its `overflow: "reject"` throw.
#[test]
fn calendar_add_date_leap_month_hebrew_picks_the_next_month_for_a_non_recurring_leap_month() {
    let anchor = civil_date_from_month_code(AnyCalendarKind::Hebrew, 5784, Month::leap(5), 1);
    let landed = calendar_add_date(AnyCalendarKind::Hebrew, anchor, 1, 0, 0, 0, false)
        .expect("constrain-mode add always succeeds for a representable date");
    let (year, month, day) = calendar_month_identity(AnyCalendarKind::Hebrew, landed);
    assert_eq!((year, month, day), (5785, Month::new(6), 1));

    assert_eq!(
        calendar_add_date(AnyCalendarKind::Hebrew, anchor, 1, 0, 0, 0, true),
        None,
        "overflow: reject must throw when Adar I does not recur in the landing year"
    );
}

/// [`calendar_add_date`]'s leap-month branch preserves `monthCode`
/// identity across a pure-year shift when the leap month *does* recur
/// (2012's `M04L` to 2020's own `M04L`, 8 years later) -- the same
/// "Adding years to go from one M04L to the next M04L" fixture case.
#[test]
fn calendar_add_date_leap_month_preserves_identity_when_the_leap_month_recurs() {
    let anchor = civil_date_from_month_code(AnyCalendarKind::Chinese, 2012, Month::leap(4), 1);
    let landed = calendar_add_date(AnyCalendarKind::Chinese, anchor, 8, 0, 0, 0, true)
        .expect("2020 has a real M04L, so reject mode must succeed");
    let (year, month, day) = calendar_month_identity(AnyCalendarKind::Chinese, landed);
    assert_eq!((year, month, day), (2020, Month::leap(4), 1));
}

/// [`calendar_add_date`]'s leap-month branch bubbles a `months`
/// component by real ordinal position within an already-identity-
/// resolved year, landing correctly on the leap month itself --
/// "adding 2 months to M03 in leap year lands in M04L (leap month)".
#[test]
fn calendar_add_date_leap_month_bubbles_months_into_a_leap_month() {
    let anchor = civil_date_from_month_code(AnyCalendarKind::Chinese, 2020, Month::new(3), 1);
    let landed = calendar_add_date(AnyCalendarKind::Chinese, anchor, 0, 2, 0, 0, true)
        .expect("landing on the real M04L must succeed under reject");
    let (year, month, day) = calendar_month_identity(AnyCalendarKind::Chinese, landed);
    assert_eq!((year, month, day), (2020, Month::leap(4), 1));
}

/// [`calendar_difference_date_leap_month`] reproduces
/// `intl402/Temporal/PlainDate/prototype/since/leap-months-chinese.js`'s
/// own worked examples directly (the same values the VM-level
/// `backend/bluejs/tests/temporal_leap_month_calendar_difference.rs`
/// integration test exercises through the real `Temporal.PlainDate`
/// surface) — this module's own host-neutral, no-VM-required layer.
#[test]
fn calendar_difference_date_leap_month_matches_the_real_test262_fixture_values() {
    let calendar = AnyCalendarKind::Chinese;
    let common1_month4 = civil_date_from_month_code(calendar, 2000, Month::new(4), 1);
    let leap_month4 = civil_date_from_month_code(calendar, 2001, Month::new(4), 1);
    let leap_month4l = civil_date_from_month_code(calendar, 2001, Month::leap(4), 1);
    let common2_month4 = civil_date_from_month_code(calendar, 2002, Month::new(4), 1);

    // "M04-M04 common-leap backwards is -1y" / "-12mo".
    assert_eq!(
        calendar_difference_date(calendar, leap_month4, common1_month4, DateUnit::Year),
        (-1, 0, 0, 0)
    );
    assert_eq!(
        calendar_difference_date(calendar, leap_month4, common1_month4, DateUnit::Month),
        (0, -12, 0, 0)
    );

    // The fixture's "M04L-M04 backwards is -12mo not -1y" is `since`'s
    // own (negated) value; `calendar_difference_date`'s direct,
    // un-negated `start`-to-`end` (`until`-style) direction for the
    // same pair (2001-M04L forward to 2002-M04, chronologically
    // forward, so positive) is `+12` months, `0` years -- the real bug
    // this module's rewrite fixes: the previous ordinal-based
    // comparison computed `+1y`/`0mo` here instead.
    assert_eq!(
        calendar_difference_date(calendar, leap_month4l, common2_month4, DateUnit::Year),
        (0, 12, 0, 0)
    );
    assert_eq!(
        calendar_difference_date(calendar, leap_month4l, common2_month4, DateUnit::Month),
        (0, 12, 0, 0)
    );

    // The fixture's "M04-M04L backwards is -1y -1mo" is `since`'s
    // negation of the forward direction; `calendar_difference_date`
    // itself always computes the forward (`until`-style)
    // `start`-to-`end` direction, so the un-negated value here is
    // `common1Month4` (2000-M04) to `leapMonth4L` (2001-M04L): `+1y
    // +1mo`.
    assert_eq!(
        calendar_difference_date(calendar, common1_month4, leap_month4l, DateUnit::Year),
        (1, 1, 0, 0)
    );
}

/// The specific case the test above did *not* cover, and which a first
/// rewrite of `calendar_difference_date_leap_month` (credited "closed"
/// on the strength of that test alone) still got wrong — found only by
/// running the real Test262 fixture directly, per
/// `development/browser_core/phase-26-ecma262-temporal/PLAN.md`'s
/// 2026-09-18 "Correction" note: `leapMonth4L` (`2001-M04L`) to
/// `common2Month5` (`2002-M05`) must be `1y 1mo`, not `1y 0mo`. The
/// missing *unconstrained* years-only pre-check (see this function's
/// own doc comment for the two-step correction) dropped a month here
/// specifically because the anchor itself is the non-recurring leap
/// month — the `constrained`-only check alone can't distinguish this
/// from the already-covered `leapMonth4L` -> `common2Month4` case.
#[test]
fn calendar_difference_date_leap_month_needs_the_unconstrained_precheck_for_a_leap_month_anchor() {
    let calendar = AnyCalendarKind::Chinese;
    let leap_month4l = civil_date_from_month_code(calendar, 2001, Month::leap(4), 1);
    let common2_month5 = civil_date_from_month_code(calendar, 2002, Month::new(5), 1);
    assert_eq!(
        calendar_difference_date(calendar, leap_month4l, common2_month5, DateUnit::Year),
        (1, 1, 0, 0)
    );
}

/// [`round_calendar_duration`]'s leap-month branch keeps `years` fixed
/// and rounds only the `months` remainder in place (matching Gecko's
/// own `ComputeNudgeWindow`), rather than flattening `years * 12 +
/// months` and re-splitting via `/ 12, % 12` — unsound once a
/// calendar's own reported "year" can span more than 12 months.
/// `2001-M04L` to `2002-M04` (`largestUnit: "years", smallestUnit:
/// "months"`, the default `Temporal.PlainYearMonth.prototype.since`/
/// `until` combination) must stay `(years: 0, months: 12)`, not fold
/// into `(years: 1, months: 0)`.
#[test]
fn round_calendar_duration_keeps_years_fixed_for_a_leap_month_calendars_month_rounding() {
    let calendar = AnyCalendarKind::Chinese;
    let leap_month4l = civil_date_from_month_code(calendar, 2001, Month::leap(4), 1);
    let common2_month4 = civil_date_from_month_code(calendar, 2002, Month::new(4), 1);
    let (years, months, weeks, days) = round_calendar_duration(
        calendar,
        leap_month4l,
        common2_month4,
        DateUnit::Year,
        DateUnit::Month,
        1,
        blueice_ecma402::NumberRoundingMode::Trunc,
    );
    assert_eq!((years, months, weeks, days), (0, 12, 0, 0));
}

#[test]
fn calendar_difference_date_counts_the_intercalary_month_of_a_thirteen_month_calendar() {
    // Coptic: twelve 30-day months plus a 5-day (6 in a leap year) `M13`.
    // Every case is a `PlainDate/prototype/until/{intercalary-month,
    // wrapping-at-end-of-month}-coptic.js` value.
    let coptic = |year, month, day| {
        civil_date_from_month_code(AnyCalendarKind::Coptic, year, Month::new(month), day)
    };
    let until =
        |start, end, unit| calendar_difference_date(AnyCalendarKind::Coptic, start, end, unit);

    // The 13th month is a real month: Mesori (M12) 5th -> M13 5th.
    assert_eq!(
        until(coptic(1970, 12, 5), coptic(1970, 13, 5), DateUnit::Month),
        (0, 1, 0, 0)
    );
    // M01 to the next year's M01 is 13 months (or one year), not 12.
    assert_eq!(
        until(coptic(1970, 1, 15), coptic(1971, 1, 15), DateUnit::Month),
        (0, 13, 0, 0)
    );
    assert_eq!(
        until(coptic(1970, 1, 15), coptic(1971, 1, 15), DateUnit::Year),
        (1, 0, 0, 0)
    );
    // Multi-year: Mesori 5th 1970 -> M13 5th 1973 is 40 months (3 * 13 + 1).
    assert_eq!(
        until(coptic(1970, 12, 5), coptic(1973, 13, 5), DateUnit::Month),
        (0, 40, 0, 0)
    );
    assert_eq!(
        until(coptic(1970, 12, 5), coptic(1973, 13, 5), DateUnit::Year),
        (3, 1, 0, 0)
    );
    // Backwards: the sign of every field follows the direction.
    assert_eq!(
        until(coptic(1973, 13, 5), coptic(1970, 12, 5), DateUnit::Year),
        (-3, -1, 0, 0)
    );
}

#[test]
fn round_calendar_duration_carries_month_rounding_at_thirteen_months_per_year() {
    let calendar = AnyCalendarKind::Ethiopian;
    let start = civil_date_from_month_code(calendar, 2014, Month::new(1), 1);
    // 1 year + 12 months + 2 days: M01 -> the 3rd day of the next year's M13.
    let end = civil_date_from_month_code(calendar, 2015, Month::new(13), 3);
    let round = |mode| {
        round_calendar_duration(
            calendar,
            start,
            end,
            DateUnit::Year,
            DateUnit::Month,
            1,
            mode,
        )
    };
    use blueice_ecma402::NumberRoundingMode as Mode;
    // Two of the intercalary month's five days is under half a month.
    assert_eq!(round(Mode::Trunc), (1, 12, 0, 0));
    assert_eq!(round(Mode::HalfExpand), (1, 12, 0, 0));
    // Rounding up carries out of the 13th month into a whole extra year.
    assert_eq!(round(Mode::Ceil), (2, 0, 0, 0));
}

#[test]
fn calendar_difference_date_leap_month_compares_the_raw_day_when_resolving_the_anchor_month() {
    // Hebrew 5784 is a leap year; Adar I (`M05L`) has 30 days but the
    // non-leap Adar (`M06`, its fallback in 5785) only 29. The
    // constrained-anchor check must compare `M06` day **30** (the
    // anchor's own raw day), not the day clamped to 29, against the end
    // date's `M06` 29th -- otherwise a tie hides the overshoot and a whole
    // year is reported. `wrapping-at-end-of-month-hebrew.js`.
    let calendar = AnyCalendarKind::Hebrew;
    let start = civil_date_from_month_code(calendar, 5784, Month::leap(5), 30);
    let end = civil_date_from_month_code(calendar, 5785, Month::new(6), 29);
    assert_eq!(
        calendar_difference_date(calendar, start, end, DateUnit::Year),
        (0, 12, 0, 29)
    );
    assert_eq!(
        calendar_difference_date(calendar, start, end, DateUnit::Month),
        (0, 12, 0, 29)
    );
    // From the 29th (which Adar has) the same span is a clean year/13 months.
    let start29 = civil_date_from_month_code(calendar, 5784, Month::leap(5), 29);
    assert_eq!(
        calendar_difference_date(calendar, start29, end, DateUnit::Year),
        (1, 0, 0, 0)
    );
    assert_eq!(
        calendar_difference_date(calendar, start29, end, DateUnit::Month),
        (0, 13, 0, 0)
    );
}
