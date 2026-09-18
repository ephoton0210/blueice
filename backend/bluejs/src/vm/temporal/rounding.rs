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
}

/// Parses a time-unit option value, accepting both the singular and plural
/// spelling (Temporal treats them as equivalent — see e.g. Test262's
/// `built-ins/Temporal/Duration/prototype/round/singular-units.js`, and
/// `built-ins/Temporal/Instant/prototype/round/smallestunit-plurals-accepted.js`).
pub(crate) fn parse_time_unit(value: &str) -> Option<TimeUnit> {
    Some(match value {
        "hour" | "hours" => TimeUnit::Hour,
        "minute" | "minutes" => TimeUnit::Minute,
        "second" | "seconds" => TimeUnit::Second,
        "millisecond" | "milliseconds" => TimeUnit::Millisecond,
        "microsecond" | "microseconds" => TimeUnit::Microsecond,
        "nanosecond" | "nanoseconds" => TimeUnit::Nanosecond,
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
            assert_eq!(parse_time_unit(singular), Some(unit));
            assert_eq!(parse_time_unit(plural), Some(unit));
        }
        assert_eq!(parse_time_unit("day"), None);
        assert_eq!(parse_time_unit("auto"), None);
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
    }
}
