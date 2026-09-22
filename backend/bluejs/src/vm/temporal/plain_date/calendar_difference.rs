// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `CalendarDateUntil` for every closed calendar ID: [`difference_iso_date`]
//! (`DifferenceISODate`), the fixed-`monthsPerYear` variant
//! ([`calendar_difference_date_fixed_months`], Gecko's `DifferenceNonISODate`),
//! the leap-month variant ([`calendar_difference_date_leap_month`], Gecko's
//! `DifferenceNonISODateWithLeapMonth`), and the [`calendar_difference_date`]
//! dispatcher between them. No `Value`/heap/Realm coupling.

use super::super::calendar::{calendar_date_from_civil, calendar_months_per_year};
use super::super::epoch::CivilDate;
use super::calendar_add::add_year_month_duration_leap_month;
use super::iso_date::{
    balance_iso_year_month, compare_iso_date, iso_date_to_epoch_days, regulate_iso_date, surpasses,
};
use super::month_structure::{
    calendar_date_from_month, calendar_has_leap_months, calendar_month_identity,
    calendar_ordinal_to_iso, calendar_uses_iso_date_arithmetic, months_in_year_for,
    surpasses_identity, to_calendar_ordinal,
};
use icu_calendar::options::Overflow as IcuOverflow;
use icu_calendar::types::Month;
use icu_calendar::{AnyCalendar, AnyCalendarKind, Date, Iso};
use std::cmp::Ordering;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DateUnit {
    Year,
    Month,
    Week,
    Day,
}

/// `DifferenceISODate(y1, m1, d1, y2, m2, d2, largestUnit)`: the calendar
/// duration `(years, months, weeks, days)` — signed, all the same sign as
/// `end - start` — such that `AddISODate(start, duration) == end`. Ported
/// directly from Gecko's `DifferenceISODate`
/// (`js/src/builtin/temporal/Calendar.cpp`): `years`/`months` are each a
/// direct field subtraction (`end.year - start.year`, `end.month -
/// start.month`), corrected by *at most one* step apiece by comparing an
/// **unconstrained** `(year, month, start.day)` candidate against `end` —
/// not a `start.day`-constrained landing date. That distinction is load-
/// bearing, not cosmetic: constraining the candidate first (e.g. via
/// [`regulate_iso_date`]) before comparing it hides exactly the "wrapping at
/// the end of a month" case Test262 pins
/// (`intl402/Temporal/PlainDate/prototype/since/wrapping-at-end-of-month-*.js`
/// — `Jan 29 -> Feb 28` must report `{ days: -30 }`, not `{ months: -1 }`,
/// because the *unconstrained* `Jan 29 + 1 month = Feb 29` candidate does
/// surpass `Feb 28`, even though `Feb 29` constrained down to `Feb 28`
/// would not). This replaces an earlier estimate-via-day-span-then-bubble-
/// one-month-at-a-time implementation that constrained every candidate
/// before comparing it, which is what let that class of case through.
pub(crate) fn difference_iso_date(
    start: CivilDate,
    end: CivilDate,
    largest_unit: DateUnit,
) -> (i64, i64, i64, i64) {
    let sign = match compare_iso_date(start, end) {
        Ordering::Less => 1_i64,
        Ordering::Greater => -1,
        Ordering::Equal => return (0, 0, 0, 0),
    };
    if !matches!(largest_unit, DateUnit::Year | DateUnit::Month) {
        let days = iso_date_to_epoch_days(end) - iso_date_to_epoch_days(start);
        return if largest_unit == DateUnit::Week {
            (0, 0, days / 7, days % 7)
        } else {
            (0, 0, 0, days)
        };
    }

    let (y1, m1, d1) = (i64::from(start.0), i64::from(start.1), i64::from(start.2));
    let two = (i64::from(end.0), i64::from(end.1), i64::from(end.2));

    let mut years = two.0 - y1;
    let mut months = two.1 - m1;

    if surpasses(sign, (y1 + years, m1, d1), two) {
        years -= sign;
        months += 12 * sign;
    }

    let (iy, im) = balance_iso_year_month(y1 + years, m1 + months);
    if surpasses(sign, (iy, i64::from(im), d1), two) {
        months -= sign;
    }

    if largest_unit == DateUnit::Month {
        months += years * 12;
        years = 0;
    }

    let (by, bm) = balance_iso_year_month(y1 + years, m1 + months);
    // A landing year outside i32 cannot occur for any representable
    // Temporal date pair, so this only ever clamps a same-year overflow.
    let by = i32::try_from(by).unwrap_or(if by > 0 { i32::MAX } else { i32::MIN });
    let constrained =
        regulate_iso_date(by, bm, d1, false).expect("constrain-mode regulation always succeeds");

    let days = iso_date_to_epoch_days(end) - iso_date_to_epoch_days(constrained);
    (years, months, 0, days)
}

/// `DifferenceNonISODate`: the fixed-`monthsPerYear` generalization of
/// [`difference_iso_date`] for every non-ISO-aligned calendar without leap
/// months (`Coptic`/`Ethiopian`/`EthiopianAmeteAlem`/`Indian`/the three
/// Hijri variants/`Persian`) — same direct-subtraction-then-two-corrections
/// shape, just carrying year/month in the target calendar's own numbering
/// via [`to_calendar_ordinal`]/[`calendar_ordinal_to_iso`] instead of the
/// ISO fields directly. `monthsPerYear` is `12` for all of these except the
/// three 13-month calendars (`Coptic`/`Ethiopian`/`EthiopianAmeteAlem`, whose
/// short intercalary `M13` is a real month `until`/`since` must count), per
/// [`calendar_months_per_year`].
pub(super) fn calendar_difference_date_fixed_months(
    calendar: AnyCalendarKind,
    start: CivilDate,
    end: CivilDate,
    largest_unit: DateUnit,
) -> (i64, i64, i64, i64) {
    let months_per_year = calendar_months_per_year(calendar);
    let sign = match compare_iso_date(start, end) {
        Ordering::Less => 1_i64,
        Ordering::Greater => -1,
        Ordering::Equal => return (0, 0, 0, 0),
    };
    let (y1, m1, d1) = to_calendar_ordinal(calendar, start);
    let two = to_calendar_ordinal(calendar, end);

    let mut years = two.0 - y1;
    let mut months = two.1 - m1;

    if surpasses(sign, (y1 + years, m1, d1), two) {
        years -= sign;
        months += months_per_year * sign;
    }

    // Gecko's own `DifferenceNonISODate`/`DifferenceISODate` normalize this
    // intermediate with a single `if > monthsPerYear {} else if < 1 {}` step
    // rather than a full modulo, which is only sound if the correction
    // above bounds `months` tightly enough that one step always suffices.
    // It does not for every calendar/date pair this engine's own
    // `to_calendar_ordinal` can produce (confirmed by a real panic on
    // `intl402/Temporal/PlainDate/prototype/since/basic-indian.js`, where a
    // single step left `bm` still outside `1..=12`) — a full `div_euclid`/
    // `rem_euclid` normalize (mirroring [`balance_iso_year_month`],
    // parameterized on `months_per_year` instead of hardcoding 12) is
    // strictly safer and exactly as correct for the in-range case.
    let normalize = |year: i64, month: i64| -> (i64, i64) {
        let zero_based = month - 1;
        (
            year + zero_based.div_euclid(months_per_year),
            zero_based.rem_euclid(months_per_year) + 1,
        )
    };

    let (by, bm) = normalize(y1 + years, m1 + months);
    if surpasses(sign, (by, bm, d1), two) {
        months -= sign;
    }

    if largest_unit == DateUnit::Month {
        months += years * months_per_year;
        years = 0;
    }

    let (fby, fbm) = normalize(y1 + years, m1 + months);
    let constrained = calendar_ordinal_to_iso(calendar, fby, fbm, d1)
        .expect("constrain-mode regulation always succeeds for a representable date");

    let days = iso_date_to_epoch_days(end) - iso_date_to_epoch_days(constrained);
    (years, months, 0, days)
}

/// The non-ISO generalization of [`difference_iso_date`] for the three
/// leap-month calendars (`Chinese`/`Dangi`/`Hebrew`), where `monthsPerYear`
/// varies by year so [`calendar_difference_date_fixed_months`]'s constant-12
/// carry does not apply. Ported directly from Gecko's own
/// `DifferenceNonISODateWithLeapMonth`
/// (`js/src/builtin/temporal/Calendar.cpp`): every candidate is compared by
/// **`Month` identity** (`(year, Month, day)`, i.e. Gecko's own
/// `CalendarDate`/`MonthCode`), not by raw ordinal month — the fix over the
/// previous estimate-then-bubble-by-ordinal implementation this replaces,
/// which could misorder whenever the two years being compared put their
/// leap month in a different position (`intl402/Temporal/PlainDate/
/// prototype/since/leap-months-{chinese,dangi,hebrew}.js`).
///
/// **Two-step years-only correction, not one** — the specific gap a first
/// rewrite of this function left open (every
/// `leap-months-{chinese,dangi,hebrew}.js` `since`/`until` fixture still
/// failed even after the `Month`-identity rewrite above). Gecko's own
/// `DifferenceNonISODateWithLeapMonth` performs *two* separate
/// surpass-checks before settling on `years`, not the single check this
/// function previously had:
///
/// 1. An **unconstrained** check first: build `(one.year + years, one.month,
///    one.day)` using `one`'s own raw `Month` identity with **no calendar
///    resolution at all** (the requested year/month pair may not even
///    exist — that is fine, since [`surpasses_identity`]'s comparison never
///    touches the calendar). If this already surpasses `two`, back off by
///    one year immediately.
/// 2. Only then the **constrained** check this function already had:
///    re-resolve `one`'s own `Month` in the (possibly-just-adjusted)
///    landing year via [`calendar_date_from_month`] (honoring its own
///    native, per-calendar fallback — see that function's own doc comment
///    for why trusting it, not a second hand-rolled uniform rule, is
///    correct here), and back off by one more year if *that* surpasses
///    `two`.
///
/// Skipping step 1 is exactly what let `M04L`(2001)-to-`M05`(2002) settle on
/// `years = 1, months = 0` instead of the correct `years = 1, months = 1`
/// (verified against `leap-months-chinese.js`'s own "M04L-M05 backwards is
/// -1y -1mo" case): without the raw pre-check, `constrained0` immediately
/// resolves `M04L` in 2002 through the native fallback to `M04`, which
/// does *not* surpass `M05`, so `years` never gets the chance to be
/// reconsidered against the *un*resolved identity first. Both checks are
/// needed because they answer different questions — step 1 asks "does the
/// literal, calendar-blind year shift already overshoot?" (catching a
/// `Chinese`/`Dangi`-style fallback that keeps the *same* month number,
/// which can undershoot what step 2 alone would report), while
/// step 2 asks "does the calendar's *actual* resolution of that identity
/// overshoot?" (catching the opposite: a fallback, like `Hebrew`'s
/// `M05L` -> `M06`, that jumps to a *later* month than the raw one).
pub(super) fn calendar_difference_date_leap_month(
    calendar: AnyCalendarKind,
    start: CivilDate,
    end: CivilDate,
    largest_unit: DateUnit,
) -> (i64, i64, i64, i64) {
    let sign = match compare_iso_date(start, end) {
        Ordering::Less => 1_i64,
        Ordering::Greater => -1,
        Ordering::Equal => return (0, 0, 0, 0),
    };

    let one = calendar_month_identity(calendar, start);
    let two = calendar_month_identity(calendar, end);

    let mut years = two.0 - one.0;

    // Step 1 — unconstrained pre-check (see this function's own doc comment):
    // pure identity comparison, no calendar resolution.
    let unconstrained = (one.0 + years, one.1, one.2);
    if surpasses_identity(sign, unconstrained, two) {
        years -= sign;
    }

    // Step 2 — constrained check: resolve `one`'s own Month identity in the
    // (possibly-just-adjusted) `one.year + years` landing year (constrain
    // mode — a leap month like `M05L` genuinely may not recur every year),
    // and back off by one more year if *that* surpasses `two`. Only the
    // *month* is resolved here: the probe is built at day 1 (which every
    // month has) and the candidate keeps `one`'s own raw `day`, exactly like
    // Gecko's `constrainedStartOfMonth`/`constrainedDate` pair. Constraining
    // the day too would hide a month-end overshoot the same way
    // `difference_iso_date`'s own unconstrained-candidate rule guards
    // against: Adar I 30th 5784 -> Adar 29th 5785 resolves `M05L` to Adar
    // (29 days), and the *constrained* day 29 would tie with `two` instead of
    // surpassing it, wrongly reporting a whole year
    // (`intl402/Temporal/PlainDate/prototype/until/wrapping-at-end-of-month-hebrew.js`,
    // "30th Adar I 5784 to 29th Adar 5785 is 12 months 29 days, not 13
    // months"/"not 1 year").
    let constrained_start_of_month =
        calendar_date_from_month(calendar, one.0 + years, one.1, 1, IcuOverflow::Constrain)
            .expect("constrain-mode regulation always succeeds for a representable date");
    let mut constrained: (i64, Month, i64) = (
        i64::from(constrained_start_of_month.year().extended_year()),
        constrained_start_of_month.month().to_input(),
        one.2,
    );
    if surpasses_identity(sign, constrained, two) {
        years -= sign;
    }

    // Add as many months as possible without surpassing `two`, bubbling one
    // month *of identity* at a time (unlike the ordinal-based version this
    // replaces, a leap month's differing position across years can't
    // misorder this — every candidate is built by re-resolving `one`'s own
    // `Month` in the target year, then walking by ordinal position only
    // within already-identity-resolved years).
    let mut months = 0_i64;
    while let Some((candidate_year, candidate_month)) = add_year_month_duration_leap_month(
        calendar,
        one.0,
        one.1,
        years,
        months + sign,
        IcuOverflow::Constrain,
    ) {
        // `day` carries through unregulated here (Gecko's own
        // `AddYearMonthDuration` leaves it as the anchor's raw `day`),
        // matching `difference_iso_date`'s own "compare an unconstrained
        // candidate" rule for detecting a month-end overshoot correctly.
        let candidate = (candidate_year, candidate_month, one.2);
        if surpasses_identity(sign, candidate, two) {
            break;
        }
        months += sign;
        constrained = candidate;
    }

    if largest_unit == DateUnit::Month && years != 0 {
        let start_cal = calendar_date_from_civil(calendar, start);
        let months_until_end_of_year = |date: &Date<AnyCalendar>| -> i64 {
            i64::from(date.months_in_year()) - i64::from(date.month().ordinal) + 1
        };
        let months_since_start_of_year =
            |date: &Date<AnyCalendar>| -> i64 { i64::from(date.month().ordinal) - 1 };

        if sign > 0 {
            months += months_until_end_of_year(&start_cal);
        } else {
            months -= months_since_start_of_year(&start_cal);
        }

        // Months in each fully-crossed intervening year, using that year's
        // own real month count (not a fixed constant).
        let mut y = sign;
        while y != years {
            let probe_year =
                i32::try_from(one.0 + y).unwrap_or(if y > 0 { i32::MAX } else { i32::MIN });
            if let Some(count) = months_in_year_for(calendar, probe_year) {
                months += i64::from(count) * sign;
            }
            y += sign;
        }

        // Months since/until the landing year's own start/end, from `one`'s
        // own Month identity re-resolved in that year.
        if let Some(dt) =
            calendar_date_from_month(calendar, one.0 + years, one.1, 1, IcuOverflow::Constrain)
        {
            if sign > 0 {
                months += months_since_start_of_year(&dt);
            } else {
                months -= months_until_end_of_year(&dt);
            }
        }
        years = 0;
    }

    let final_probe = calendar_date_from_month(
        calendar,
        constrained.0,
        constrained.1,
        constrained.2,
        IcuOverflow::Constrain,
    )
    .expect("constrain-mode regulation always succeeds for a representable date");
    let final_iso = final_probe.to_calendar(Iso);
    let constrained_iso: CivilDate = (
        final_iso.year().extended_year(),
        final_iso.month().number(),
        final_iso.day_of_month().0,
    );

    let days = iso_date_to_epoch_days(end) - iso_date_to_epoch_days(constrained_iso);
    (years, months, 0, days)
}

/// The non-ISO generalization of [`difference_iso_date`]: dispatches to
/// whichever of [`calendar_uses_iso_date_arithmetic`],
/// [`calendar_difference_date_fixed_months`] or
/// [`calendar_difference_date_leap_month`] matches `calendar`, per Gecko's
/// own `NonISODateUntil` three-way split. `week`/`day` `largestUnit` is
/// always calendar-invariant pure ISO epoch-day math (every supported
/// calendar uses a 7-day week and every concrete date has exactly one ISO
/// form), matching Gecko's own "delegate to the ISO 8601 calendar for
/// weeks/days" shortcut.
pub(crate) fn calendar_difference_date(
    calendar: AnyCalendarKind,
    start: CivilDate,
    end: CivilDate,
    largest_unit: DateUnit,
) -> (i64, i64, i64, i64) {
    if calendar_uses_iso_date_arithmetic(calendar) {
        return difference_iso_date(start, end, largest_unit);
    }
    if !matches!(largest_unit, DateUnit::Year | DateUnit::Month) {
        let days = iso_date_to_epoch_days(end) - iso_date_to_epoch_days(start);
        return if largest_unit == DateUnit::Week {
            (0, 0, days / 7, days % 7)
        } else {
            (0, 0, 0, days)
        };
    }
    if calendar_has_leap_months(calendar) {
        calendar_difference_date_leap_month(calendar, start, end, largest_unit)
    } else {
        calendar_difference_date_fixed_months(calendar, start, end, largest_unit)
    }
}
