// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! UTC-offset grammar (minute- and sub-minute-precision) and resolving a
//! time-zone identifier or ISO date-time string to the offset it observes.

use super::annotations::{is_time_zone_identifier, parse_annotation_suffix};
use super::datetime::Parsed;
use super::scan::{parse_iso_date_prefix, parse_iso_time_prefix, parse_time_spec, Cursor};
use num_bigint::BigInt;

/// `UTCOffset`, `Cursor`-based: shared by [`scan_utc_offset_suffix`] and
/// [`is_valid_time_zone_identifier`]'s own offset form. The second element of
/// the result is whether a seconds (or fractional) component was actually
/// present in the source -- `InterpretISODateTimeOffset`'s `MatchBehaviour`
/// switch (see [`Parsed::offset_sub_minute_precision`]) needs to know this
/// independently of the resulting nanosecond value, since e.g. `-00:45` and
/// `-00:45:00` carry the *same* numeric offset but must be matched
/// differently.
pub(super) fn scan_offset(cursor: &mut Cursor, sub_minute: bool) -> Option<(i64, bool)> {
    let sign = match cursor.eat_any(b"+-")? {
        b'-' => -1,
        _ => 1,
    };
    let hour = i64::from(cursor.digits(2)?);
    if hour > 23 {
        return None;
    }
    let mut minute = 0;
    let mut second = 0;
    let mut nanoseconds = 0;
    let mut has_seconds = false;
    let extended = cursor.eat(b':');
    if extended || cursor.peek_digit() {
        minute = i64::from(cursor.digits(2)?);
        if minute > 59 {
            return None;
        }
        let seconds_follow = if extended {
            cursor.eat(b':')
        } else {
            cursor.peek_digit()
        };
        if seconds_follow {
            if !sub_minute {
                return None;
            }
            has_seconds = true;
            second = i64::from(cursor.digits(2)?);
            if second > 59 {
                return None;
            }
            if cursor.eat_any(b".,").is_some() {
                nanoseconds = i64::from(cursor.fraction_nanoseconds()?);
            }
        }
    }
    Some((
        sign * (((hour * 60 + minute) * 60 + second) * 1_000_000_000 + nanoseconds),
        has_seconds,
    ))
}

/// `DateTimeUTCOffset`: the UTC designator or a numeric offset, both
/// optional — but only ever after a time of day, which is why this is only
/// reached from the time-carrying branches.
pub(super) fn scan_utc_offset_suffix(cursor: &mut Cursor, parsed: &mut Parsed) -> Option<()> {
    if cursor.eat_any(b"Zz").is_some() {
        parsed.utc_designator = true;
    } else if matches!(cursor.peek(), Some(b'+' | b'-')) {
        let (offset, has_seconds) = scan_offset(cursor, true)?;
        parsed.offset_nanoseconds = Some(offset);
        parsed.offset_sub_minute_precision = has_seconds;
    }
    Some(())
}

/// Parses a `±HH`, `±HH:MM` or `±HHMM` offset — the only offset forms a
/// `TimeZoneIdentifier` accepts — into signed nanoseconds. A seconds
/// component disqualifies the string even when it is `00`, because the
/// restriction is syntactic (Test262's `timezone-string-datetime.js` rejects
/// `-07:00:00` alongside `-07:00:01`).
pub(crate) fn parse_minute_precision_offset(value: &str) -> Option<i128> {
    if !value.starts_with(['+', '-']) {
        return None;
    }
    let (offset, rest) = parse_utc_offset_prefix(value)?;
    (rest.is_empty() && offset.minute_precision).then_some(offset.nanoseconds)
}

/// A parsed UTC offset: its exact value, plus whether it was written without
/// a seconds component.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct UtcOffset {
    /// Signed nanoseconds east of UTC.
    pub(crate) nanoseconds: i128,
    /// Whether the written form had no seconds field (a `Z` designator
    /// counts, since it denotes exactly UTC).
    pub(crate) minute_precision: bool,
}

/// Parses a `Z`/`z` designator or a `±HH[[:]MM[[:]SS[.frac]]]` UTC offset at
/// the start of `source`. An offset shares the time-of-day grammar, so the
/// body goes through [`parse_time_spec`] too.
pub(crate) fn parse_utc_offset_prefix(source: &str) -> Option<(UtcOffset, &str)> {
    if let Some(rest) = source.strip_prefix(['Z', 'z']) {
        return Some((
            UtcOffset {
                nanoseconds: 0,
                minute_precision: true,
            },
            rest,
        ));
    }
    let negative = match source.as_bytes().first()? {
        b'+' => false,
        b'-' => true,
        _ => return None,
    };
    // An annotation, not a further offset field, is the only thing that may
    // follow an offset, so it is where the offset body ends.
    let end = source[1..]
        .find('[')
        .map_or(source.len(), |index| index + 1);
    let body = &source[1..end];
    let (hour, minute, second, nanoseconds) = parse_time_spec(body)?;
    if second > 59 {
        return None;
    }
    // Whether a seconds field was *written* (not merely non-zero) is what
    // decides whether the offset can serve as a `TimeZoneIdentifier`.
    let core = body.split(['.', ',']).next().unwrap_or(body);
    let has_second = if core.contains(':') {
        core.matches(':').count() == 2
    } else {
        core.len() == 6
    };
    let total = i128::from(hour) * 3_600_000_000_000
        + i128::from(minute) * 60_000_000_000
        + i128::from(second) * 1_000_000_000
        + i128::from(nanoseconds);
    Some((
        UtcOffset {
            nanoseconds: if negative { -total } else { total },
            minute_precision: !has_second,
        },
        &source[end..],
    ))
}

/// Resolves a Temporal `TimeZoneIdentifier`, or the time-zone information
/// carried by a full ISO date-time string, into the UTC offset (in
/// nanoseconds) that zone was actually observing at `epoch_nanoseconds`.
///
/// `Err(())` means the input is not a time zone at all (either malformed, or
/// syntactically shaped like an IANA name that names no real zone in the
/// pinned database). A named IANA zone's real historical offset comes from
/// Stage 1 Track E's `time_zone.rs`, which owns the actual transition data;
/// `UTC` and a fixed numeric offset are resolved directly here since they do
/// not depend on `epoch_nanoseconds` at all.
pub(crate) fn resolve_time_zone_offset(
    source: &str,
    epoch_nanoseconds: &BigInt,
) -> Result<i128, ()> {
    let identifier = if is_time_zone_identifier(source) {
        source.to_string()
    } else {
        // Not a bare identifier: the only other accepted form is a full ISO
        // date-time carrying either a time-zone annotation (which wins), a
        // `Z` designator, or a UTC offset.
        let (_, rest) = parse_iso_date_prefix(source).ok_or(())?;
        let rest = rest.strip_prefix(['T', 't', ' ']).ok_or(())?;
        let (_, rest) = parse_iso_time_prefix(rest).ok_or(())?;
        let (offset, rest) = match parse_utc_offset_prefix(rest) {
            Some((offset, rest)) => (Some(offset), rest),
            None => (None, rest),
        };
        match parse_annotation_suffix(rest)?.time_zone {
            Some(time_zone) => time_zone,
            // A `Z` designator means UTC; a written offset must be
            // minute-precision to serve as an identifier.
            None => {
                let offset = offset.ok_or(())?;
                if !offset.minute_precision {
                    return Err(());
                }
                return Ok(offset.nanoseconds);
            }
        }
    };
    if let Some(offset) = parse_minute_precision_offset(&identifier) {
        return Ok(offset);
    }
    if identifier.eq_ignore_ascii_case("UTC") {
        return Ok(0);
    }
    // A named IANA zone: `identifier` is already a bare `TimeZoneIdentifier`
    // at this point (either `source` itself, or the body of a winning
    // time-zone annotation), so this only ever takes the
    // `parse_bare_identifier` path inside `time_zone::parse_identifier` — it
    // re-validates the name against the same pinned database rather than
    // trusting the shape check above, and supplies the real offset lookup.
    let zone = crate::vm::temporal::time_zone::parse_identifier(&identifier).ok_or(())?;
    Ok(i128::from(zone.offset_nanoseconds_for(epoch_nanoseconds)))
}

/// A whole-string, minute-precision UTC offset, as a fixed-offset time-zone
/// identifier is spelled. Returns nanoseconds east of UTC.
pub(crate) fn parse_offset_identifier_nanoseconds(source: &str) -> Option<i64> {
    let mut cursor = Cursor::new(source);
    let (offset, _) = scan_offset(&mut cursor, false)?;
    cursor.done().then_some(offset)
}

/// A whole-string UTC offset at full (sub-minute) precision -- the grammar a
/// `Temporal.ZonedDateTime` property-bag `offset` field, or `.with()`'s own
/// `offset` property, is validated against (`ParseDateTimeUTCOffset`).
/// Unlike [`parse_offset_identifier_nanoseconds`], this accepts a genuine
/// historical sub-minute offset (e.g. Monrovia's pre-1972 `-00:44:30`) --
/// exactly what `Temporal.ZonedDateTime.prototype.offset` itself can return,
/// so a round trip through `.with({ offset })` must accept it back.
pub(crate) fn parse_offset_string_nanoseconds(source: &str) -> Option<i64> {
    let mut cursor = Cursor::new(source);
    let (offset, _) = scan_offset(&mut cursor, true)?;
    cursor.done().then_some(offset)
}
