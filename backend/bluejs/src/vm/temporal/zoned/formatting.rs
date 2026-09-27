// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `Temporal.ZonedDateTime.prototype.toString` and its `TemporalZonedDateTimeToString`
//! formatting helpers.

use super::super::*;
use super::resolution::round_offset_nanoseconds_to_minutes;
use super::resolution::temporal_zoned_date_time_zone;

impl Vm {
    /// `TemporalZonedDateTimeToString`: rounds the exact epoch instant first
    /// (`RoundTemporalInstant`'s own magnitude-based rounding, matching
    /// `Instant.prototype.toString` -- not a zone-day-aware rounding), then
    /// formats the *rounded* instant's local fields plus an exact (not
    /// minute-rounded) offset, an optional time-zone annotation and a
    /// calendar annotation.
    pub(in super::super::super) fn temporal_zoned_date_time_to_string(
        &mut self,
        receiver: &Value,
        options: &Value,
    ) -> Result<Value, RuntimeError> {
        // Native dispatch already checked this receiver's Temporal brand.
        let existing = self
            .temporal_zoned_date_time_receiver(receiver)
            .expect("native dispatch requires a ZonedDateTime receiver");
        let base = self.stack.len();
        let result = (|| {
            let options = self.temporal_options(options)?;
            let show_calendar = self.temporal_string_option(
                &options,
                "calendarName",
                &["auto", "always", "never", "critical"],
            )?;
            let explicit_digits = self.temporal_fractional_second_digits(&options)?;
            let show_offset = self
                .temporal_string_option(&options, "offset", &["auto", "never"])?
                .unwrap_or_else(|| "auto".into());
            let mode =
                self.temporal_rounding_mode(&options, blueice_ecma402::NumberRoundingMode::Trunc)?;
            let smallest_unit = self.temporal_unit_option(&options, "smallestUnit", false)?;
            // `timeZoneName` is the last option, read with the others -- before
            // `smallestUnit` (already read above) is validated, so an invalid
            // unit cannot hide it from an observer.
            let show_time_zone = self
                .temporal_string_option(&options, "timeZoneName", &["auto", "never", "critical"])?
                .unwrap_or_else(|| "auto".into());
            let smallest_unit = Self::temporal_time_unit(smallest_unit, "smallestUnit", false)?;
            let (precision, unit, increment) = match smallest_unit {
                Some(rounding::TimeUnit::Minute) => {
                    (SecondsPrecision::Minute, rounding::TimeUnit::Minute, 1)
                }
                Some(rounding::TimeUnit::Second) => {
                    (SecondsPrecision::Digits(0), rounding::TimeUnit::Second, 1)
                }
                Some(rounding::TimeUnit::Millisecond) => (
                    SecondsPrecision::Digits(3),
                    rounding::TimeUnit::Millisecond,
                    1,
                ),
                Some(rounding::TimeUnit::Microsecond) => (
                    SecondsPrecision::Digits(6),
                    rounding::TimeUnit::Microsecond,
                    1,
                ),
                Some(rounding::TimeUnit::Nanosecond) | Some(rounding::TimeUnit::Hour) => (
                    SecondsPrecision::Digits(9),
                    rounding::TimeUnit::Nanosecond,
                    1,
                ),
                None => match explicit_digits {
                    None => (
                        SecondsPrecision::Auto,
                        rounding::TimeUnit::Nanosecond,
                        1_i128,
                    ),
                    Some(0) => (SecondsPrecision::Digits(0), rounding::TimeUnit::Second, 1),
                    Some(digits @ 1..=3) => (
                        SecondsPrecision::Digits(digits),
                        rounding::TimeUnit::Millisecond,
                        10_i128.pow(u32::from(3 - digits)),
                    ),
                    Some(digits @ 4..=6) => (
                        SecondsPrecision::Digits(digits),
                        rounding::TimeUnit::Microsecond,
                        10_i128.pow(u32::from(6 - digits)),
                    ),
                    Some(digits) => (
                        SecondsPrecision::Digits(digits),
                        rounding::TimeUnit::Nanosecond,
                        10_i128.pow(u32::from(9 - digits)),
                    ),
                },
            };
            // A valid Temporal instant is bounded far inside i128's range.
            let epoch_i128: i128 = existing
                .epoch_nanoseconds
                .to_i128()
                .expect("a valid Temporal instant fits in i128");
            let rounded = duration_math::TimeDuration::from_nanoseconds(epoch_i128)
                .round_as_if_positive(unit, increment, mode)
                .total_nanoseconds();
            let rounded_ns = BigInt::from(rounded);
            let zone = temporal_zoned_date_time_zone(&existing);
            let offset_ns = zone.offset_nanoseconds_for(&rounded_ns);
            let local = &rounded_ns + BigInt::from(offset_ns);
            let mut result = format_zoned_date_time_date_time(&local, precision);
            if show_offset != "never" {
                // `FormatDateTimeUTCOffsetRounded`: the string carries a
                // minute-precision offset even where the zone's real (local mean
                // time) offset has seconds -- unlike the `offset` getter.
                result.push_str(&format_offset_nanoseconds_exact(
                    round_offset_nanoseconds_to_minutes(offset_ns),
                ));
            }
            if show_time_zone != "never" {
                result.push('[');
                if show_time_zone == "critical" {
                    result.push('!');
                }
                result.push_str(&existing.time_zone);
                result.push(']');
            }
            let show = plain_date::parse_show_calendar(show_calendar.as_deref().unwrap_or("auto"))
                .expect("temporal_string_option validated calendarName");
            result.push_str(&plain_date::format_calendar_annotation(
                &existing.calendar,
                show,
            ));
            Ok(Value::String(result.into()))
        })();
        self.stack.truncate(base);
        result
    }
}

/// `FormatUTCOffsetNanoseconds`: an *exact* `±HH:MM[:SS[.sssssssss]]`
/// representation, which is what the `ZonedDateTime` `offset` getter needs --
/// the real, possibly sub-minute historical offset a named zone can carry
/// (e.g. Monrovia's pre-1972 -00:44:30). `toString` rounds the offset to the
/// minute first (`FormatDateTimeUTCOffsetRounded`), since an ISO string's
/// offset is minute-precision.
pub(in super::super) fn format_offset_nanoseconds_exact(offset: i64) -> String {
    let sign = if offset < 0 { '-' } else { '+' };
    let magnitude = offset.unsigned_abs();
    let hours = magnitude / 3_600_000_000_000;
    let minutes = (magnitude / 60_000_000_000) % 60;
    let seconds = (magnitude / 1_000_000_000) % 60;
    let nanoseconds = magnitude % 1_000_000_000;
    let mut result = format!("{sign}{hours:02}:{minutes:02}");
    if seconds != 0 || nanoseconds != 0 {
        result.push_str(&format!(":{seconds:02}"));
        // A zero fraction becomes an empty suffix; nonzero fractions retain
        // their leading zeroes while trailing zeroes are omitted.
        let fraction = format!(".{nanoseconds:09}");
        result.push_str(fraction.trim_end_matches('0').trim_end_matches('.'));
    }
    result
}

/// The date/time portion of `TemporalZonedDateTimeToString`'s output, given
/// an already-rounded *local* epoch value (`rounded_epoch_nanoseconds +
/// offset`, i.e. what `epoch::instant_fields` decomposes as this zone's own
/// wall-clock fields). Deliberately duplicates
/// `Vm::format_instant_string`'s own date/time formatting (rather than
/// reusing it) since that function always appends its *own* offset/`Z`
/// suffix, which `ZonedDateTime`'s exact (not minute-rounded) offset display
/// cannot reuse -- see [`format_offset_nanoseconds_exact`]'s own doc
/// comment.
pub(super) fn format_zoned_date_time_date_time(
    local: &BigInt,
    precision: SecondsPrecision,
) -> String {
    let ((year, month, day), (hour, minute, second, millisecond, microsecond, nanosecond)) =
        epoch::instant_fields(local);
    let mut result = if (0..=9999).contains(&year) {
        format!("{year:04}")
    } else {
        format!("{}{:06}", if year < 0 { "-" } else { "+" }, year.abs())
    };
    result.push_str(&format!("-{month:02}-{day:02}T{hour:02}:{minute:02}"));
    if precision != SecondsPrecision::Minute {
        result.push_str(&format!(":{second:02}"));
        let nanos_total = u32::from(millisecond) * 1_000_000
            + u32::from(microsecond) * 1_000
            + u32::from(nanosecond);
        match precision {
            SecondsPrecision::Minute | SecondsPrecision::Digits(0) => {}
            SecondsPrecision::Digits(digits) => {
                let text = format!("{nanos_total:09}");
                result.push('.');
                result.push_str(&text[..digits as usize]);
            }
            SecondsPrecision::Auto if nanos_total != 0 => {
                let text = format!("{nanos_total:09}");
                result.push('.');
                result.push_str(text.trim_end_matches('0'));
            }
            SecondsPrecision::Auto => {}
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::format_offset_nanoseconds_exact;

    #[test]
    fn exact_offset_preserves_fractional_seconds_without_trailing_zeros() {
        assert_eq!(
            format_offset_nanoseconds_exact(19_812_345_600_000),
            "+05:30:12.3456"
        );
        assert_eq!(
            format_offset_nanoseconds_exact(-19_812_345_600_000),
            "-05:30:12.3456"
        );
    }
}
