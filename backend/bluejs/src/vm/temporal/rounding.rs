// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Temporal's unit and rounding-mode vocabulary.
//!
//! `RoundingMode` is not redefined here: Temporal reuses the exact same
//! nine named modes as `Intl.NumberFormat`'s `roundingMode` option (a
//! deliberate shared TC39 vocabulary), already implemented as
//! [`blueice_ecma402::NumberRoundingMode`].

/// One of the six time units `Temporal.Instant`/`Temporal.PlainTime`
/// rounding and arithmetic accept (no calendar units — those types have no
/// calendar). Declared smallest-time-span first so the derived `Ord`
/// directly means "represents less or equal time" (`Nanosecond < Hour`) —
/// exactly what validating `smallestUnit <= largestUnit` needs.
#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub(crate) enum TimeUnit {
    Nanosecond,
    Microsecond,
    Millisecond,
    Second,
    Minute,
    Hour,
}

impl TimeUnit {
    /// The exact length of this unit, in nanoseconds.
    pub(crate) fn nanoseconds(self) -> i128 {
        match self {
            Self::Hour => 3_600_000_000_000,
            Self::Minute => 60_000_000_000,
            Self::Second => 1_000_000_000,
            Self::Millisecond => 1_000_000,
            Self::Microsecond => 1_000,
            Self::Nanosecond => 1,
        }
    }

    /// `MaximumTemporalDurationRoundingIncrement`: the count of this unit
    /// that makes up one of the next larger unit. Used with
    /// `inclusive = false` by `until`/`since`'s increment validation.
    pub(crate) fn increment_dividend(self) -> i128 {
        match self {
            Self::Hour => 24,
            Self::Minute | Self::Second => 60,
            Self::Millisecond | Self::Microsecond | Self::Nanosecond => 1_000,
        }
    }
}

/// One of the ten units Temporal's `largestUnit`/`smallestUnit`/`unit`
/// options accept — [`TimeUnit`] plus `Day` and the three calendar-dependent
/// units. `Temporal.Duration` needs the wider vocabulary even though it can
/// only *evaluate* the `Day`-and-below part without a `relativeTo` anchor:
/// the calendar units still have to be recognised as valid option values so
/// that requesting one throws the `RangeError` the specification requires
/// rather than the "unknown unit" `RangeError`.
///
/// Declared smallest-span-first so the derived `Ord` directly means
/// "represents less or equal time" — exactly what `LargerOfTwoTemporalUnits`
/// and the `smallestUnit <= largestUnit` check need.
#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub(crate) enum TemporalUnit {
    Nanosecond,
    Microsecond,
    Millisecond,
    Second,
    Minute,
    Hour,
    Day,
    Week,
    Month,
    Year,
}

impl TemporalUnit {
    /// The exact length of this unit in nanoseconds, or `None` for the
    /// calendar-dependent units (week/month/year) whose length is only
    /// defined relative to an anchor date.
    ///
    /// `Day` is exactly 24 hours here. That is not an approximation: with no
    /// `relativeTo`, or with a `Temporal.PlainDate`/`PlainDateTime` one,
    /// Temporal defines a day as 86,400 seconds. Only a
    /// `Temporal.ZonedDateTime` anchor can make a day a different length.
    pub(crate) fn nanoseconds(self) -> Option<i128> {
        Some(match self {
            Self::Day => 86_400_000_000_000,
            Self::Hour => 3_600_000_000_000,
            Self::Minute => 60_000_000_000,
            Self::Second => 1_000_000_000,
            Self::Millisecond => 1_000_000,
            Self::Microsecond => 1_000,
            Self::Nanosecond => 1,
            Self::Week | Self::Month | Self::Year => return None,
        })
    }

    /// `IsCalendarUnit`: whether this unit's length depends on a calendar.
    pub(crate) fn is_calendar(self) -> bool {
        matches!(self, Self::Week | Self::Month | Self::Year)
    }

    /// `MaximumTemporalDurationRoundingIncrement`: the exclusive upper bound a
    /// `roundingIncrement` must both stay under and divide evenly into, or
    /// `None` for the date-category units, which only have the general
    /// `1..=1e9` bound.
    pub(crate) fn maximum_rounding_increment(self) -> Option<i128> {
        match self {
            Self::Year | Self::Month | Self::Week | Self::Day => None,
            Self::Hour => Some(24),
            Self::Minute | Self::Second => Some(60),
            Self::Millisecond | Self::Microsecond | Self::Nanosecond => Some(1_000),
        }
    }
}

/// Parses a `TemporalUnit` option value, accepting both spellings the same way
/// [`parse_time_unit`] does (verified against Test262's
/// `built-ins/Temporal/Duration/prototype/round/{singular-units,largestunit-plurals-accepted}.js`).
pub(crate) fn parse_temporal_unit(value: &str) -> Option<TemporalUnit> {
    Some(match value {
        "year" | "years" => TemporalUnit::Year,
        "month" | "months" => TemporalUnit::Month,
        "week" | "weeks" => TemporalUnit::Week,
        "day" | "days" => TemporalUnit::Day,
        "hour" | "hours" => TemporalUnit::Hour,
        "minute" | "minutes" => TemporalUnit::Minute,
        "second" | "seconds" => TemporalUnit::Second,
        "millisecond" | "milliseconds" => TemporalUnit::Millisecond,
        "microsecond" | "microseconds" => TemporalUnit::Microsecond,
        "nanosecond" | "nanoseconds" => TemporalUnit::Nanosecond,
        _ => return None,
    })
}

/// Every spelling [`parse_temporal_unit`] accepts, in the order Temporal's own
/// unit table lists them. Option readers validate against this before parsing,
/// so an unrecognised spelling is rejected once, in one place.
pub(crate) const TEMPORAL_UNIT_NAMES: &[&str] = &[
    "year",
    "years",
    "month",
    "months",
    "week",
    "weeks",
    "day",
    "days",
    "hour",
    "hours",
    "minute",
    "minutes",
    "second",
    "seconds",
    "millisecond",
    "milliseconds",
    "microsecond",
    "microseconds",
    "nanosecond",
    "nanoseconds",
];

/// Correctly-rounded `𝔽(numerator / denominator)` for exact integer inputs.
///
/// `Temporal.Duration.prototype.total` returns the *exact* mathematical ratio
/// of a nanosecond count to a unit length, converted to a Number once at the
/// end. Dividing in `f64` instead would round twice; this does the division as
/// binary long division on exact integers and rounds a single time, to nearest
/// with ties to even, exactly as `𝔽` does.
pub(crate) fn exact_ratio_to_f64(numerator: i128, denominator: i128) -> f64 {
    debug_assert!(denominator > 0);
    if numerator == 0 {
        return 0.0;
    }
    let negative = numerator < 0;
    let magnitude = numerator.unsigned_abs();
    let denominator = denominator.unsigned_abs();
    // Scale the divisor up front when the integer quotient already carries
    // more than a mantissa's worth of bits, so the long-division loop below
    // never has to shift `accumulator` back down (which would discard the
    // remainder it needs for the final rounding decision).
    let quotient_bits = 128 - (magnitude / denominator).leading_zeros() as i32;
    let mut exponent = 0_i32;
    let mut divisor = denominator;
    if quotient_bits > 53 {
        exponent = quotient_bits - 53;
        divisor <<= exponent as u32;
    }
    let mut accumulator = magnitude / divisor;
    let mut remainder = magnitude % divisor;
    while accumulator < (1_u128 << 52) && remainder != 0 {
        accumulator <<= 1;
        remainder <<= 1;
        if remainder >= divisor {
            remainder -= divisor;
            accumulator += 1;
        }
        exponent -= 1;
    }
    if remainder != 0 {
        let doubled = remainder << 1;
        if doubled > divisor || (doubled == divisor && !accumulator.is_multiple_of(2)) {
            accumulator += 1;
        }
    }
    let value = accumulator as f64 * 2_f64.powi(exponent);
    if negative {
        -value
    } else {
        value
    }
}

/// Parses a time-unit option value, accepting both the singular and plural
/// spelling (Temporal treats them as equivalent — see e.g. Test262's
/// `built-ins/Temporal/Duration/prototype/round/singular-units.js`, and
/// `built-ins/Temporal/Instant/prototype/round/smallestunit-plurals-accepted.js`).
///
/// A calendar (date) unit, which `Temporal.Instant` never accepts but whose
/// *name* is still a syntactically valid `smallestUnit`/`largestUnit` value.
/// `GetTemporalUnitValuedOption` validates the spelling; the per-operation
/// unit-group check is a separate, later step, and Test262's
/// `options-read-before-algorithmic-validation.js` fixtures observe the
/// difference.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DateUnit {
    Year,
    Month,
    Week,
    Day,
}

/// Any unit name Temporal's unit-valued options accept.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Unit {
    Date(DateUnit),
    Time(TimeUnit),
}

/// Parses a *time* unit name, singular or plural, rejecting the calendar
/// units outright. Callers that must read a unit-valued option before
/// validating its unit group use [`parse_unit`] instead.
pub(crate) fn parse_time_unit(value: &str) -> Option<TimeUnit> {
    match parse_unit(value)? {
        Unit::Time(unit) => Some(unit),
        Unit::Date(_) => None,
    }
}

/// Parses any unit name, singular or plural.
pub(crate) fn parse_unit(value: &str) -> Option<Unit> {
    Some(match value {
        "year" | "years" => Unit::Date(DateUnit::Year),
        "month" | "months" => Unit::Date(DateUnit::Month),
        "week" | "weeks" => Unit::Date(DateUnit::Week),
        "day" | "days" => Unit::Date(DateUnit::Day),
        "hour" | "hours" => Unit::Time(TimeUnit::Hour),
        "minute" | "minutes" => Unit::Time(TimeUnit::Minute),
        "second" | "seconds" => Unit::Time(TimeUnit::Second),
        "millisecond" | "milliseconds" => Unit::Time(TimeUnit::Millisecond),
        "microsecond" | "microseconds" => Unit::Time(TimeUnit::Microsecond),
        "nanosecond" | "nanoseconds" => Unit::Time(TimeUnit::Nanosecond),
        _ => return None,
    })
}

/// Parses Temporal's `roundingMode` option value, sharing
/// `Intl.NumberFormat`'s exact rounding-mode vocabulary.
pub(crate) fn parse_rounding_mode(value: &str) -> Option<blueice_ecma402::NumberRoundingMode> {
    Some(match value {
        "halfExpand" => blueice_ecma402::NumberRoundingMode::HalfExpand,
        "ceil" => blueice_ecma402::NumberRoundingMode::Ceil,
        "floor" => blueice_ecma402::NumberRoundingMode::Floor,
        "expand" => blueice_ecma402::NumberRoundingMode::Expand,
        "trunc" => blueice_ecma402::NumberRoundingMode::Trunc,
        "halfCeil" => blueice_ecma402::NumberRoundingMode::HalfCeil,
        "halfFloor" => blueice_ecma402::NumberRoundingMode::HalfFloor,
        "halfTrunc" => blueice_ecma402::NumberRoundingMode::HalfTrunc,
        "halfEven" => blueice_ecma402::NumberRoundingMode::HalfEven,
        _ => return None,
    })
}

/// Rounds `value` (a count of whole `increment`-sized buckets, expressed as
/// the exact ratio `value / increment`) to the nearest integer per `mode`,
/// per ECMA-262's `RoundNumberToIncrement`. `value` and `increment` are
/// exact integers (nanosecond counts); this never loses precision the way
/// an `f64` division would for the ranges Temporal deals in.
pub(crate) fn round_to_increment(
    value: i128,
    increment: i128,
    mode: blueice_ecma402::NumberRoundingMode,
) -> i128 {
    use blueice_ecma402::NumberRoundingMode as Mode;
    let negative = value < 0;
    let magnitude = value.unsigned_abs();
    let increment = increment.unsigned_abs();
    let quotient = magnitude / increment;
    let remainder = magnitude % increment;
    if remainder == 0 {
        return value;
    }
    let round_up = (quotient + 1) * increment;
    let round_down = quotient * increment;
    let doubled = remainder * 2;
    // Every branch below is expressed in terms of magnitude/sign — e.g.
    // "ceil" (toward +infinity) rounds a negative value's magnitude DOWN
    // (since a smaller magnitude is a larger, less-negative value).
    let rounded = match mode {
        Mode::Ceil => {
            if negative {
                round_down
            } else {
                round_up
            }
        }
        Mode::Floor => {
            if negative {
                round_up
            } else {
                round_down
            }
        }
        Mode::Expand => round_up,
        Mode::Trunc => round_down,
        Mode::HalfCeil => {
            if doubled > increment || (doubled == increment && !negative) {
                round_up
            } else {
                round_down
            }
        }
        Mode::HalfFloor => {
            if doubled > increment || (doubled == increment && negative) {
                round_up
            } else {
                round_down
            }
        }
        Mode::HalfExpand => {
            if doubled >= increment {
                round_up
            } else {
                round_down
            }
        }
        Mode::HalfTrunc => {
            if doubled > increment {
                round_up
            } else {
                round_down
            }
        }
        Mode::HalfEven => {
            if doubled > increment || (doubled == increment && !quotient.is_multiple_of(2)) {
                round_up
            } else {
                round_down
            }
        }
    };
    let rounded = rounded as i128;
    if negative {
        -rounded
    } else {
        rounded
    }
}

/// `RoundNumberToIncrementAsIfPositive`: rounds `value / increment` to an
/// integer with `mode` applied **as if `value` were positive**, so `ceil`
/// always moves toward +infinity, `trunc`/`floor` always toward -infinity,
/// and a tie under `halfExpand` always goes to the larger value — regardless
/// of `value`'s own sign.
///
/// This is what every `Temporal.Instant` rounding path uses (`RoundTemporalInstant`
/// is defined in terms of it), and it is genuinely different from
/// [`round_to_increment`]: a negative instant rounded with `trunc` moves
/// *earlier*, not toward the epoch. Test262's
/// `Instant/prototype/round/negative-instant.js` and
/// `Instant/prototype/toString/rounding-direction.js` pin both directions.
pub(crate) fn round_to_increment_as_if_positive(
    value: i128,
    increment: i128,
    mode: blueice_ecma402::NumberRoundingMode,
) -> i128 {
    use blueice_ecma402::NumberRoundingMode as Mode;
    let increment = increment.abs();
    let quotient = value.div_euclid(increment);
    let remainder = value.rem_euclid(increment);
    if remainder == 0 {
        return value;
    }
    let doubled = remainder * 2;
    let round_up = match mode {
        Mode::Ceil | Mode::Expand => true,
        Mode::Floor | Mode::Trunc => false,
        Mode::HalfCeil | Mode::HalfExpand => doubled >= increment,
        Mode::HalfFloor | Mode::HalfTrunc => doubled > increment,
        Mode::HalfEven => {
            doubled > increment || (doubled == increment && quotient.rem_euclid(2) != 0)
        }
    };
    (quotient + i128::from(round_up)) * increment
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_ecma402::NumberRoundingMode as Mode;

    #[test]
    fn nanoseconds_per_unit_are_exact() {
        assert_eq!(TimeUnit::Hour.nanoseconds(), 3_600_000_000_000);
        assert_eq!(TimeUnit::Nanosecond.nanoseconds(), 1);
    }

    #[test]
    fn ordering_means_represents_less_or_equal_time() {
        assert!(TimeUnit::Nanosecond < TimeUnit::Hour);
        assert!(TimeUnit::Minute < TimeUnit::Hour);
        assert!(TimeUnit::Second <= TimeUnit::Second);
        // Exactly the validation `smallestUnit <= largestUnit` needs:
        // largestUnit "hour", smallestUnit "minute" is valid.
        assert!(TimeUnit::Minute <= TimeUnit::Hour);
        // largestUnit "minute", smallestUnit "hour" is invalid.
        assert!(TimeUnit::Hour > TimeUnit::Minute);
    }

    #[test]
    fn accepts_both_singular_and_plural_time_unit_spellings() {
        for (singular, plural, unit) in [
            ("hour", "hours", TimeUnit::Hour),
            ("minute", "minutes", TimeUnit::Minute),
            ("second", "seconds", TimeUnit::Second),
            ("millisecond", "milliseconds", TimeUnit::Millisecond),
            ("microsecond", "microseconds", TimeUnit::Microsecond),
            ("nanosecond", "nanoseconds", TimeUnit::Nanosecond),
        ] {
            assert_eq!(parse_unit(singular), Some(Unit::Time(unit)));
            assert_eq!(parse_unit(plural), Some(Unit::Time(unit)));
        }
        // `"auto"` is a separate literal each option handles itself, not a
        // unit name.
        assert_eq!(parse_unit("auto"), None);
    }

    #[test]
    fn parses_every_rounding_mode_name() {
        for name in [
            "halfExpand",
            "ceil",
            "floor",
            "expand",
            "trunc",
            "halfCeil",
            "halfFloor",
            "halfTrunc",
            "halfEven",
        ] {
            assert!(parse_rounding_mode(name).is_some(), "{name}");
        }
        assert_eq!(parse_rounding_mode("nearest"), None);
    }

    #[test]
    fn rounds_exact_multiples_without_adjustment() {
        for mode in [
            Mode::Ceil,
            Mode::Floor,
            Mode::Expand,
            Mode::Trunc,
            Mode::HalfCeil,
            Mode::HalfFloor,
            Mode::HalfExpand,
            Mode::HalfTrunc,
            Mode::HalfEven,
        ] {
            assert_eq!(round_to_increment(30, 10, mode), 30, "{mode:?}");
            assert_eq!(round_to_increment(-30, 10, mode), -30, "{mode:?}");
        }
    }

    #[test]
    fn ceil_and_floor_round_toward_positive_and_negative_infinity() {
        assert_eq!(round_to_increment(12, 10, Mode::Ceil), 20);
        assert_eq!(round_to_increment(-12, 10, Mode::Ceil), -10);
        assert_eq!(round_to_increment(12, 10, Mode::Floor), 10);
        assert_eq!(round_to_increment(-12, 10, Mode::Floor), -20);
    }

    #[test]
    fn expand_and_trunc_round_away_from_and_toward_zero() {
        assert_eq!(round_to_increment(11, 10, Mode::Expand), 20);
        assert_eq!(round_to_increment(-11, 10, Mode::Expand), -20);
        assert_eq!(round_to_increment(19, 10, Mode::Trunc), 10);
        assert_eq!(round_to_increment(-19, 10, Mode::Trunc), -10);
    }

    #[test]
    fn half_expand_rounds_the_exact_midpoint_away_from_zero() {
        assert_eq!(round_to_increment(15, 10, Mode::HalfExpand), 20);
        assert_eq!(round_to_increment(-15, 10, Mode::HalfExpand), -20);
        assert_eq!(round_to_increment(14, 10, Mode::HalfExpand), 10);
    }

    #[test]
    fn half_even_rounds_the_exact_midpoint_to_the_even_neighbor() {
        // 15 / 10: neighbors 10 (even quotient 1? quotient=1 is odd) and 20
        // (quotient 2, even) -> rounds to 20.
        assert_eq!(round_to_increment(15, 10, Mode::HalfEven), 20);
        // 25 / 10: neighbors 20 (quotient 2, even) and 30 (quotient 3, odd)
        // -> rounds to 20.
        assert_eq!(round_to_increment(25, 10, Mode::HalfEven), 20);
    }

    #[test]
    fn half_ceil_and_half_floor_break_ties_toward_a_fixed_infinity() {
        assert_eq!(round_to_increment(15, 10, Mode::HalfCeil), 20);
        assert_eq!(round_to_increment(-15, 10, Mode::HalfCeil), -10);
        assert_eq!(round_to_increment(15, 10, Mode::HalfFloor), 10);
        assert_eq!(round_to_increment(-15, 10, Mode::HalfFloor), -20);
    }

    #[test]
    fn half_trunc_breaks_ties_toward_zero() {
        assert_eq!(round_to_increment(15, 10, Mode::HalfTrunc), 10);
        assert_eq!(round_to_increment(-15, 10, Mode::HalfTrunc), -10);
        // Past the midpoint it still rounds away from zero.
        assert_eq!(round_to_increment(19, 10, Mode::HalfTrunc), 20);
        assert_eq!(round_to_increment(-19, 10, Mode::HalfTrunc), -20);
    }

    #[test]
    fn temporal_units_order_by_the_time_they_represent() {
        assert!(TemporalUnit::Nanosecond < TemporalUnit::Day);
        assert!(TemporalUnit::Day < TemporalUnit::Week);
        assert!(TemporalUnit::Week < TemporalUnit::Month);
        assert!(TemporalUnit::Month < TemporalUnit::Year);
        assert!(TemporalUnit::Hour < TemporalUnit::Day);
    }

    #[test]
    fn only_the_calendar_dependent_units_lack_an_exact_length() {
        assert_eq!(
            TemporalUnit::Day.nanoseconds(),
            Some(86_400_000_000_000),
            "a day is 24 hours without a ZonedDateTime anchor"
        );
        assert_eq!(TemporalUnit::Nanosecond.nanoseconds(), Some(1));
        for unit in [TemporalUnit::Week, TemporalUnit::Month, TemporalUnit::Year] {
            assert_eq!(unit.nanoseconds(), None, "{unit:?}");
            assert!(unit.is_calendar(), "{unit:?}");
        }
        for unit in [
            TemporalUnit::Day,
            TemporalUnit::Hour,
            TemporalUnit::Nanosecond,
        ] {
            assert!(!unit.is_calendar(), "{unit:?}");
        }
    }

    #[test]
    fn maximum_rounding_increments_match_the_next_larger_unit() {
        // Test262's round/invalid-increments.js rejects exactly these
        // boundaries: 24 hours, 60 minutes/seconds, 1000 sub-second units.
        assert_eq!(TemporalUnit::Hour.maximum_rounding_increment(), Some(24));
        assert_eq!(TemporalUnit::Minute.maximum_rounding_increment(), Some(60));
        assert_eq!(TemporalUnit::Second.maximum_rounding_increment(), Some(60));
        assert_eq!(
            TemporalUnit::Millisecond.maximum_rounding_increment(),
            Some(1_000)
        );
        assert_eq!(
            TemporalUnit::Nanosecond.maximum_rounding_increment(),
            Some(1_000)
        );
        // round/roundingincrement-days-large.js accepts 1e7 days, so the
        // date-category units carry no per-unit maximum of their own.
        for unit in [
            TemporalUnit::Day,
            TemporalUnit::Week,
            TemporalUnit::Month,
            TemporalUnit::Year,
        ] {
            assert_eq!(unit.maximum_rounding_increment(), None, "{unit:?}");
        }
    }

    #[test]
    fn every_temporal_unit_name_parses_and_only_those() {
        for name in TEMPORAL_UNIT_NAMES {
            assert!(parse_temporal_unit(name).is_some(), "{name}");
        }
        assert_eq!(parse_temporal_unit("year"), Some(TemporalUnit::Year));
        assert_eq!(parse_temporal_unit("weeks"), Some(TemporalUnit::Week));
        assert_eq!(parse_temporal_unit("auto"), None);
        assert_eq!(parse_temporal_unit("era"), None);
        assert_eq!(parse_temporal_unit("Day"), None);
    }

    #[test]
    fn exact_ratios_round_once_to_the_nearest_double() {
        assert_eq!(exact_ratio_to_f64(0, 1_000_000_000), 0.0);
        assert_eq!(exact_ratio_to_f64(1_000_000_000, 1_000_000_000), 1.0);
        assert_eq!(exact_ratio_to_f64(-60_000_000_000, 60_000_000_000), -1.0);
        assert_eq!(exact_ratio_to_f64(3, 2), 1.5);
        assert_eq!(exact_ratio_to_f64(-3, 2), -1.5);
        // Exactly representable sub-unit ratios (nanoseconds per day/hour).
        assert_eq!(exact_ratio_to_f64(1, 86_400_000_000_000), 1.0 / 86.4e12);
        // Test262's total/total-of-each-unit.js case: 5d5h5m5.005005005s
        // totalled in each unit. The nanosecond total is exact; the coarser
        // units are the correctly-rounded value of the same exact ratio.
        let total = 450_305_005_005_005_i128;
        assert_eq!(exact_ratio_to_f64(total, 1), 450_305_005_005_005.0);
        assert_eq!(
            exact_ratio_to_f64(total, 1_000),
            450_305_005_005_005.0 / 1_000.0
        );
        assert_eq!(
            exact_ratio_to_f64(total, 86_400_000_000_000),
            5.0 + 18_305_005.005_005 / 86_400_000.0
        );
        // A quotient wider than a mantissa still rounds to nearest-even
        // rather than truncating.
        let wide = (1_i128 << 60) + 1;
        assert_eq!(exact_ratio_to_f64(wide, 1), wide as f64);
        assert_eq!(exact_ratio_to_f64(2 * wide + 1, 2), (wide as f64) + 0.5);
        // At this magnitude adjacent doubles differ by one. Exact half-way
        // quotients must select the even neighbor on either side.
        assert_eq!(exact_ratio_to_f64((1_i128 << 53) + 1, 2), 2_f64.powi(52));
        assert_eq!(
            exact_ratio_to_f64((1_i128 << 53) + 3, 2),
            2_f64.powi(52) + 2.0
        );
    }

    #[test]
    fn parses_date_unit_names_that_are_not_time_units() {
        assert_eq!(parse_unit("weeks"), Some(Unit::Date(DateUnit::Week)));
        assert_eq!(parse_unit("day"), Some(Unit::Date(DateUnit::Day)));
        assert_eq!(parse_unit("hour"), Some(Unit::Time(TimeUnit::Hour)));
        // A syntactically valid unit name that is not a *time* unit is still
        // parsed here, and rejected by the caller's unit-group check.
        assert_eq!(parse_unit("hour"), Some(Unit::Time(TimeUnit::Hour)));
        // These are not unit names at all.
        for name in ["era", "eraYear", "SECOND", "other string", ""] {
            assert_eq!(parse_unit(name), None, "{name}");
        }
    }

    #[test]
    fn increment_dividends_match_the_next_larger_unit() {
        assert_eq!(TimeUnit::Hour.increment_dividend(), 24);
        assert_eq!(TimeUnit::Minute.increment_dividend(), 60);
        assert_eq!(TimeUnit::Second.increment_dividend(), 60);
        assert_eq!(TimeUnit::Millisecond.increment_dividend(), 1_000);
        assert_eq!(TimeUnit::Nanosecond.increment_dividend(), 1_000);
    }

    #[test]
    fn as_if_positive_rounding_ignores_the_value_sign() {
        // Test262's Instant/prototype/round/negative-instant.js, in exact
        // nanoseconds: -1e18 rounded to the hour. floor/trunc and every half
        // mode land on the earlier hour; ceil/expand on the later one.
        let value = -1_000_000_000_000_000_000_i128;
        let hour = 3_600_000_000_000_i128;
        let earlier = -1_000_000_800_000_000_000_i128;
        let later = -999_997_200_000_000_000_i128;
        for mode in [
            Mode::Floor,
            Mode::Trunc,
            Mode::HalfCeil,
            Mode::HalfFloor,
            Mode::HalfExpand,
            Mode::HalfTrunc,
            Mode::HalfEven,
        ] {
            assert_eq!(
                round_to_increment_as_if_positive(value, hour, mode),
                earlier,
                "{mode:?}"
            );
        }
        for mode in [Mode::Ceil, Mode::Expand] {
            assert_eq!(
                round_to_increment_as_if_positive(value, hour, mode),
                later,
                "{mode:?}"
            );
        }
    }

    #[test]
    fn as_if_positive_rounding_matches_plain_rounding_for_positive_values() {
        for mode in [
            Mode::Ceil,
            Mode::Floor,
            Mode::Expand,
            Mode::Trunc,
            Mode::HalfCeil,
            Mode::HalfFloor,
            Mode::HalfExpand,
            Mode::HalfTrunc,
            Mode::HalfEven,
        ] {
            for value in [0, 10, 12, 15, 19, 25, 30] {
                assert_eq!(
                    round_to_increment_as_if_positive(value, 10, mode),
                    round_to_increment(value, 10, mode),
                    "{mode:?} {value}"
                );
            }
        }
    }

    #[test]
    fn as_if_positive_half_even_breaks_ties_to_the_even_neighbor() {
        assert_eq!(
            round_to_increment_as_if_positive(15, 10, Mode::HalfEven),
            20
        );
        assert_eq!(
            round_to_increment_as_if_positive(25, 10, Mode::HalfEven),
            20
        );
        // -15/10: the neighbours are -20 (quotient -2, even) and -10
        // (quotient -1, odd), so the tie goes to -20.
        assert_eq!(
            round_to_increment_as_if_positive(-15, 10, Mode::HalfEven),
            -20
        );
        assert_eq!(
            round_to_increment_as_if_positive(-25, 10, Mode::HalfEven),
            -20
        );
    }
}
