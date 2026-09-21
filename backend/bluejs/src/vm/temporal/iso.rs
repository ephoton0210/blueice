// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host-neutral ISO 8601 / Temporal grammar parsing.
//!
//! Every function here is plain Rust with no `Value`/heap/Realm coupling —
//! directly unit-testable without a VM, per Phase 26's foundation/adapter
//! split (`development/browser_core/phase-26-ecma262-temporal/PLAN.md`).
//! `vm/temporal.rs`'s `impl Vm` methods are the only callers that bridge
//! these results into JavaScript-visible values.
//!
//! This is a strict recursive-descent scan of Temporal's own grammar
//! (`ISODateTime`, `AnnotatedDateTime`, `TemporalYearMonthString`,
//! `TemporalMonthDayString`, `TemporalTimeString`, `TemporalDurationString`),
//! not a lenient split-on-separators approximation:
//!
//! - Both the extended (`1976-11-18`, `15:23:30`) and basic (`19761118`,
//!   `152330`) forms are accepted, and the separator choice must be
//!   consistent within one date or one time (`2020-0101` and `00:0000` are
//!   syntax errors).
//! - Every numeric field has an exact digit count: a year is 4 digits, or a
//!   sign plus exactly 6 (`+002020`, with `-000000` rejected outright);
//!   months, days, hours, minutes and seconds are exactly 2.
//! - A decimal fraction may be written with `.` or `,` and holds 1 to 9
//!   digits — a 10th digit is a syntax error rather than silently truncated.
//! - The date/time separator is `T`, `t` or a single space.
//! - A `60` seconds field (a leap second) parses and is clamped to `59`.
//! - A UTC offset may carry sub-minute precision with its own fraction
//!   (`+00:19:32.37`), so offsets are returned in nanoseconds; a time-zone
//!   *annotation*, by contrast, is restricted to minute precision.
//! - Parsing never applies Temporal's per-type representable-range limits:
//!   the same `-999999-10-01` that is out of range for a `PlainDate` is a
//!   perfectly valid `PlainMonthDay` string. Callers apply the range rule
//!   their type actually has (see `epoch::is_date_time_within_limits` and
//!   `is_year_month_within_limits`).
//!
//! The rules above are read off the pinned Test262 corpus rather than
//! assumed; the unit tests below cite the fixture each case comes from.

mod annotations;
mod datetime;
mod duration;
mod offset;
mod scan;
#[cfg(test)]
mod tests;

// The module's crate-facing API. Not every name has an external caller today;
// the facade keeps the pre-split `iso::*` paths stable for all of them.
#[allow(unused_imports)]
pub(crate) use self::annotations::{is_time_zone_identifier, parse_annotation_suffix, Annotations};
#[allow(unused_imports)]
pub(crate) use self::datetime::{
    parse_date_time, parse_instant, parse_month_day, parse_plain_time, parse_time,
    parse_year_month, InstantParts, Parsed, Time,
};
#[allow(unused_imports)]
pub(crate) use self::duration::parse_duration_record;
#[allow(unused_imports)]
pub(crate) use self::offset::{
    parse_minute_precision_offset, parse_offset_string_nanoseconds, parse_utc_offset_prefix,
    resolve_time_zone_offset, UtcOffset,
};
#[allow(unused_imports)]
pub(crate) use self::scan::{parse_date, parse_iso_date_prefix, parse_iso_time_prefix};

/// Returns whether `year` is a leap year in the proleptic Gregorian
/// calendar (the ISO 8601 calendar).
pub(crate) fn is_leap_year(year: i32) -> bool {
    year.rem_euclid(4) == 0 && (year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0)
}

/// Returns the number of days in `month` of `year`, or `None` for a month
/// outside `1..=12`.
pub(crate) fn days_in_month(year: i32, month: u8) -> Option<u8> {
    Some(match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => return None,
    })
}

/// `ISOYearMonthWithinLimits`: the representable range of a
/// `Temporal.PlainYearMonth`, which is one month wider at each end than the
/// `PlainDate` range so that every in-range date's year-month is itself
/// in range.
pub(crate) fn is_year_month_within_limits(year: i32, month: u8) -> bool {
    match year {
        -271_821 => month >= 4,
        275_760 => month <= 9,
        _ => (-271_821..=275_760).contains(&year),
    }
}
