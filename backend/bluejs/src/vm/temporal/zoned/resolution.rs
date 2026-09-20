// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host-neutral wall-clock/instant resolution helpers shared by every
//! `Temporal.ZonedDateTime` adapter: local-field derivation, start-of-day
//! and range checks, and `InterpretISODateTimeOffset`.

use super::super::*;

/// Rewrites a value's ISO fields to the local wall-clock fields its
/// `epoch_nanoseconds` really has in `zone`. The ISO fields a `ZonedDateTime`
/// carries are local, so they need the offset the zone was really observing at
/// that instant — Track E's whole reason for existing.
pub(in super::super) fn temporal_set_local_fields(
    value: &mut TemporalValue,
    zone: &time_zone::TimeZone,
) {
    let offset = zone.offset_nanoseconds_for(&value.epoch_nanoseconds);
    let ((year, month, day), (hour, minute, second, millisecond, microsecond, nanosecond)) =
        epoch::instant_fields(&(&value.epoch_nanoseconds + BigInt::from(offset)));
    value.year = year;
    value.month = month;
    value.day = day;
    value.hour = hour;
    value.minute = minute;
    value.second = second;
    value.millisecond = millisecond;
    value.microsecond = microsecond;
    value.nanosecond = nanosecond;
}

/// `RangeError` unless `instant` is a representable `Temporal.Instant`.
pub(in super::super) fn temporal_require_instant_range(
    instant: BigInt,
) -> Result<BigInt, RuntimeError> {
    if epoch::is_in_instant_range(&instant) {
        Ok(instant)
    } else {
        Err(RuntimeError::RangeError(
            "Temporal.ZonedDateTime epoch nanoseconds are outside the supported range".into(),
        ))
    }
}

/// `GetStartOfDay(timeZone, date)`, which throws a `RangeError` when that
/// instant is not representable -- a `ZonedDateTime` at the edge of the range
/// has a wall-clock date whose start of day lies just outside it
/// (`startOfDay/throws-if-epoch-nanoseconds-outside-valid-limits.js`,
/// `withPlainTime/get-start-of-day-throws.js`).
pub(in super::super) fn temporal_checked_start_of_day(
    zone: &time_zone::TimeZone,
    date: epoch::CivilDate,
) -> Result<BigInt, RuntimeError> {
    temporal_require_instant_range(zone.start_of_day(date))
}

/// Maps a host-neutral zone-resolution failure onto the `RangeError` the spec
/// raises for it.
pub(in super::super) fn temporal_resolution_error(
    _: time_zone::AmbiguousLocalTime,
) -> RuntimeError {
    RuntimeError::RangeError(
        "the local time is ambiguous or does not exist in this time zone".into(),
    )
}

/// Re-parses a `ZonedDateTime` value's own stored `time_zone` identifier
/// back into a [`time_zone::TimeZone`]. Always succeeds: the identifier only
/// ever comes from [`time_zone::TimeZone::identifier`] itself (the
/// constructor/`from`/every method below all store it that way), which is
/// always round-trippable through [`time_zone::parse_identifier`].
pub(in super::super) fn temporal_zoned_date_time_zone(
    value: &TemporalValue,
) -> time_zone::TimeZone {
    time_zone::parse_identifier(&value.time_zone)
        .expect("a ZonedDateTime value's own stored time zone is always a valid identifier")
}

/// `RoundNumberToIncrement(offsetNanoseconds, 60e9, "halfExpand")`: rounds a
/// real UTC offset to the nearest whole minute, ties rounding away from
/// zero. Only meaningful for [`temporal_interpret_offset`]'s `match_minutes`
/// (`MatchBehaviour::MatchMinutes`) comparison -- see that function's own
/// doc comment.
fn round_offset_nanoseconds_to_minutes(offset_nanoseconds: i64) -> i64 {
    const MINUTE: i64 = 60_000_000_000;
    let quotient = offset_nanoseconds / MINUTE;
    let remainder = offset_nanoseconds % MINUTE;
    let rounded = if remainder.unsigned_abs() * 2 >= MINUTE.unsigned_abs() {
        quotient + if offset_nanoseconds > 0 { 1 } else { -1 }
    } else {
        quotient
    };
    rounded * MINUTE
}

/// `InterpretISODateTimeOffset`, collapsed to this engine's own three
/// offset-behaviour shapes:
///
/// - `utc_exact` (the ISO string `Z` designator only): the offset is exactly
///   zero, and the zone/disambiguation are never consulted at all.
/// - `offset_nanoseconds: None` (`"wall"` behaviour -- no offset spelled at
///   all): resolved purely through the zone and `disambiguation`.
/// - `offset_nanoseconds: Some(_)` (`"option"` behaviour -- a property-bag
///   `offset` field or a string's own numeric offset): under `"prefer"` and
///   `"reject"` it is used directly whenever it matches one of the zone's real
///   possible instants for that local date/time, otherwise `"reject"` throws
///   and `"prefer"` falls back to zone/disambiguation resolution. `"use"`
///   trusts the offset outright (`local - offset`), and `"ignore"` discards it
///   and resolves through the zone alone.
///
/// `match_minutes` (`MatchBehaviour::MatchMinutes` vs. `MatchExactly`):
/// besides an exact match against a real candidate's own offset, also accept
/// a candidate whose real offset *rounded to the nearest minute* equals the
/// given offset -- legacy back-compat for a `ZonedDateTime` string's
/// minute-precision (no seconds spelled) leading offset against a named
/// zone with genuine historical sub-minute precision (`Africa/Monrovia`'s
/// pre-1972 `-00:44:30`, matched by a written `-00:45`). A property-bag
/// `offset` field and `.with()`'s own `offset` property are always
/// `MatchExactly`, per Gecko's `ZonedDateTime.cpp`
/// (`ToTemporalZonedDateTime`'s object overload, and `with`, both construct
/// `MatchBehaviour::MatchExactly` unconditionally -- only the *string*
/// overload of `ToTemporalZonedDateTime` ever picks `MatchMinutes`, and only
/// when the leading offset itself was not spelled with sub-minute
/// precision).
#[allow(clippy::too_many_arguments)]
pub(in super::super) fn temporal_interpret_offset(
    zone: &time_zone::TimeZone,
    date: epoch::CivilDate,
    time: epoch::CivilTime,
    offset_nanoseconds: Option<i64>,
    utc_exact: bool,
    disambiguation: time_zone::Disambiguation,
    offset_option: &str,
    match_minutes: bool,
) -> Result<BigInt, RuntimeError> {
    let local = epoch::nanoseconds_since_epoch(date, time, 0);
    if utc_exact {
        return Ok(local - BigInt::from(offset_nanoseconds.unwrap_or(0)));
    }
    let Some(offset_ns) = offset_nanoseconds else {
        return zone
            .epoch_nanoseconds_for(date, time, disambiguation)
            .map_err(temporal_resolution_error);
    };
    // `InterpretISODateTimeOffset` steps 3-4: `ignore` discards the offset and
    // resolves the wall clock through the zone alone (a repeated time picks
    // `disambiguation`'s occurrence, not the one the offset names), while `use`
    // trusts the offset outright -- even a minute-rounded one that no real
    // candidate has exactly (`zoneddatetime-sub-minute-offset.js`,
    // `with/dst-option-offset.js`). Neither consults the possible instants.
    match offset_option {
        "ignore" => {
            return zone
                .epoch_nanoseconds_for(date, time, disambiguation)
                .map_err(temporal_resolution_error);
        }
        "use" => return Ok(&local - BigInt::from(offset_ns)),
        _ => {}
    }
    // Step 7 (`prefer`/`reject`): matching the offset against the zone's
    // possible instants starts from the *wall-clock* date itself, which must be within
    // `CheckISODaysRange`'s +/-10^8 days of the epoch -- a day narrower at
    // the start of the range than `PlainDateTime`'s own limits, so
    // `-271821-04-19T23:00-01:00[-01:00]` (an in-range instant) is still
    // rejected (`ZonedDateTime/from/argument-string-limits.js`).
    if matches!(offset_option, "prefer" | "reject")
        && plain_date::iso_date_to_epoch_days(date).abs() > 100_000_000
    {
        return Err(RuntimeError::RangeError(
            "the wall-clock date is outside the representable range of ZonedDateTime".into(),
        ));
    }
    let possible = zone.possible_epoch_nanoseconds(date, time);
    for candidate in &possible {
        let candidate_offset = zone.offset_nanoseconds_for(candidate);
        if candidate_offset == offset_ns
            || (match_minutes && round_offset_nanoseconds_to_minutes(candidate_offset) == offset_ns)
        {
            return Ok(candidate.clone());
        }
    }
    if offset_option == "reject" {
        return Err(RuntimeError::RangeError(
            "the given offset does not match the time zone".into(),
        ));
    }
    // `prefer`, with no candidate matching: fall back to the zone.
    zone.epoch_nanoseconds_for(date, time, disambiguation)
        .map_err(temporal_resolution_error)
}
