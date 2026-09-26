// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The per-type entry points (`parse_date_time`, `parse_year_month`,
//! `parse_month_day`, `parse_time`, `parse_instant`) and the [`Parsed`] result
//! they share, assembled from the scanners in the sibling modules.

use super::annotations::{parse_annotation_suffix, scan_annotations};
use super::offset::{parse_utc_offset_prefix, scan_utc_offset_suffix};
use super::scan::{
    is_valid_date, parse_iso_date_prefix, parse_iso_time_prefix, scan_date, scan_time, scan_year,
    Cursor,
};
use crate::vm::temporal::epoch::{CivilDate, CivilTime};

/// A time of day: `(hour, minute, second, millisecond, microsecond,
/// nanosecond)`.
pub(crate) type Time = (u8, u8, u8, u16, u16, u16);

/// The result of scanning one Temporal date/time string.
///
/// Which fields are meaningful depends on the entry point that produced it:
/// [`parse_year_month`] can report a source that never spelled a day, and
/// [`parse_month_day`] one that never spelled a year, so the presence flags
/// are part of the result rather than something a caller can re-derive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Parsed {
    pub(crate) year: i32,
    pub(crate) month: u8,
    pub(crate) day: u8,
    /// Whether the source spelled a year (false for `--11-18`/`11-18`).
    pub(crate) year_present: bool,
    /// Whether the source spelled a day (false for `1976-11`).
    pub(crate) day_present: bool,
    pub(crate) time: Option<Time>,
    /// Whether the source ended its time with the UTC designator `Z`/`z`.
    pub(crate) utc_designator: bool,
    /// A numeric UTC offset, in nanoseconds east of UTC.
    pub(crate) offset_nanoseconds: Option<i64>,
    /// Whether `offset_nanoseconds` was spelled with an explicit seconds (or
    /// fractional) component (`-00:44:30`, `-00:45:00`), as opposed to
    /// minute-only (`-00:45`) or absent. This is `InterpretISODateTimeOffset`'s
    /// `MatchBehaviour` switch: a *minute-only* leading offset fuzzy-matches
    /// a named zone's real historical offset once rounded to the nearest
    /// minute, while a sub-minute-precision spelling requires an exact
    /// match, however the given and real values individually round.
    pub(crate) offset_sub_minute_precision: bool,
    /// The time-zone annotation's identifier, if one was present.
    pub(crate) time_zone: Option<String>,
    /// The first `u-ca=` annotation's value, if one was present.
    pub(crate) calendar: Option<String>,
}

impl Default for Parsed {
    fn default() -> Self {
        Self {
            year: 1970,
            month: 1,
            day: 1,
            year_present: false,
            day_present: false,
            time: None,
            utc_designator: false,
            offset_nanoseconds: None,
            offset_sub_minute_precision: false,
            time_zone: None,
            calendar: None,
        }
    }
}

/// The pieces of a `TemporalInstantString`: a date, a time-of-day, and the
/// UTC offset that places them on the epoch timeline. Both the time and the
/// offset are mandatory for an `Instant` — a bare date, or a date with an
/// offset but no time, carries too little information (Test262's
/// `argument-string-date-with-utc-offset.js`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InstantParts {
    pub(crate) date: CivilDate,
    pub(crate) time: CivilTime,
    pub(crate) offset_nanoseconds: i128,
}

/// Parses a complete `TemporalInstantString`, rejecting trailing junk.
pub(crate) fn parse_instant(source: &str) -> Option<InstantParts> {
    let (date, rest) = parse_iso_date_prefix(source)?;
    let (time, rest) = parse_iso_time_prefix(rest.strip_prefix(['T', 't', ' '])?)?;
    let (offset, rest) = parse_utc_offset_prefix(rest)?;
    parse_annotation_suffix(rest).ok()?;
    Some(InstantParts {
        date,
        time,
        offset_nanoseconds: offset.nanoseconds,
    })
}

/// `AnnotatedDateTime`: a required `ISODate`, an optional time of day (and,
/// only then, an optional UTC offset), and optional annotations. This is
/// the form `Temporal.PlainDate`, `PlainDateTime`, `Instant` and
/// `ZonedDateTime` strings are built from.
pub(crate) fn parse_date_time(source: &str) -> Option<Parsed> {
    let mut cursor = Cursor::new(source);
    let (year, month, day) = scan_date(&mut cursor)?;
    let mut parsed = Parsed {
        year,
        month,
        day,
        year_present: true,
        day_present: true,
        ..Parsed::default()
    };
    if cursor.eat_any(b"Tt ").is_some() {
        parsed.time = Some(scan_time(&mut cursor)?);
        scan_utc_offset_suffix(&mut cursor, &mut parsed)?;
    }
    let (time_zone, calendar) = scan_annotations(&mut cursor).ok()?;
    parsed.time_zone = time_zone;
    parsed.calendar = calendar;
    cursor.done().then_some(parsed)
}

/// The `DateYear ['-'] DateMonth` half of `TemporalYearMonthString` — the
/// form that carries no day at all.
pub(super) fn parse_year_month_only(source: &str) -> Option<Parsed> {
    let mut cursor = Cursor::new(source);
    let year = scan_year(&mut cursor)?;
    cursor.eat(b'-');
    let month = cursor.digits(2)? as u8;
    if !(1..=12).contains(&month) {
        return None;
    }
    let (time_zone, calendar) = scan_annotations(&mut cursor).ok()?;
    cursor.done().then_some(Parsed {
        year,
        month,
        day: 1,
        year_present: true,
        day_present: false,
        time_zone,
        calendar,
        ..Parsed::default()
    })
}

/// `TemporalYearMonthString`: the day-less form, or any full
/// `AnnotatedDateTime`.
pub(crate) fn parse_year_month(source: &str) -> Option<Parsed> {
    parse_year_month_only(source).or_else(|| parse_date_time(source))
}

/// The `['--'] DateMonth ['-'] DateDay` half of `TemporalMonthDayString` —
/// the form that carries no year at all, so day validity is judged against
/// the ISO reference year 1972 (a leap year, which is why `02-29` is a
/// valid month-day).
pub(super) fn parse_month_day_only(source: &str) -> Option<Parsed> {
    let mut cursor = Cursor::new(source);
    if cursor.eat(b'-') && !cursor.eat(b'-') {
        return None;
    }
    let month = cursor.digits(2)? as u8;
    cursor.eat(b'-');
    let day = cursor.digits(2)? as u8;
    if !is_valid_date(1972, month, day) {
        return None;
    }
    let (time_zone, calendar) = scan_annotations(&mut cursor).ok()?;
    cursor.done().then_some(Parsed {
        year: 1972,
        month,
        day,
        year_present: false,
        day_present: true,
        time_zone,
        calendar,
        ..Parsed::default()
    })
}

/// `TemporalMonthDayString`: the year-less form, or any full
/// `AnnotatedDateTime`.
pub(crate) fn parse_month_day(source: &str) -> Option<Parsed> {
    parse_month_day_only(source).or_else(|| parse_date_time(source))
}

/// Whether a bare (undesignated) time string would also read as a
/// year-month or month-day string. Temporal resolves that ambiguity by
/// requiring the `T` designator rather than by preferring one reading:
/// `1130` is a syntax error as a time, while `1314` (no such month) is
/// 13:14 — see `TemporalHelpers.ISO.plainTimeStringsAmbiguous()`.
pub(super) fn is_ambiguous_with_a_date(source: &str) -> bool {
    parse_year_month_only(source).is_some() || parse_month_day_only(source).is_some()
}

/// `AnnotatedTime`: a time of day, optionally introduced by the `T`/`t`
/// designator, with an optional UTC offset and annotations.
pub(super) fn parse_time_only(source: &str) -> Option<Parsed> {
    let mut cursor = Cursor::new(source);
    let designated = cursor.eat_any(b"Tt").is_some();
    let mut parsed = Parsed {
        time: Some(scan_time(&mut cursor)?),
        ..Parsed::default()
    };
    scan_utc_offset_suffix(&mut cursor, &mut parsed)?;
    let (time_zone, calendar) = scan_annotations(&mut cursor).ok()?;
    parsed.time_zone = time_zone;
    parsed.calendar = calendar;
    if !cursor.done() || (!designated && is_ambiguous_with_a_date(source)) {
        return None;
    }
    Some(parsed)
}

/// `TemporalTimeString`: a time-only string, or a full
/// `AnnotatedDateTime` that actually carries a time (a date alone never
/// implies midnight —
/// `PlainTime/from/argument-string-no-implicit-midnight.js`).
pub(crate) fn parse_time(source: &str) -> Option<Parsed> {
    if let Some(parsed) = parse_time_only(source) {
        return Some(parsed);
    }
    let parsed = parse_date_time(source)?;
    parsed.time.is_some().then_some(parsed)
}

/// `TemporalTimeString` reduced to just its time of day, which is all a
/// `Temporal.PlainTime` retains. A UTC designator asserts an exact instant,
/// so it is not valid on a wall-clock time
/// (`PlainTime/from/argument-string-with-utc-designator.js`).
pub(crate) fn parse_plain_time(source: &str) -> Option<Time> {
    let parsed = parse_time(source)?;
    if parsed.utc_designator {
        return None;
    }
    parsed.time
}

#[cfg(test)]
mod tests {
    use super::{parse_month_day_only, parse_time_only};

    #[test]
    fn invalid_annotations_and_offsets_fail_in_their_respective_date_forms() {
        assert!(parse_month_day_only("--02-29[!foo=bar]").is_none());
        assert!(parse_time_only("12:34+25:00").is_none());
    }
}
