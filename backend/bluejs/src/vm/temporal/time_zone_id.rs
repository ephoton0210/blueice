// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host-neutral Temporal time-zone *identifier* handling.
//!
//! Covers the spec's `TimeZoneIdentifier` grammar (a minute-precision UTC
//! offset or an IANA name), `ParseTemporalTimeZoneString`'s fallback that
//! reads a zone out of a full ISO date-time string, and
//! `GetAvailableNamedTimeZoneIdentifier`'s case-insensitive lookup against the
//! same pinned IANA registry `Intl.supportedValuesOf("timeZone")` already
//! exposes — so Temporal and ECMA-402 can never disagree about which zone
//! names exist.
//!
//! Deliberately *identifier*-only. Resolving a named zone's UTC offset at an
//! arbitrary instant needs the IANA transition history, which is Phase 26
//! Stage 1 Track E's `time_zone.rs`, not this file. [`offset_seconds`]
//! therefore answers only for `UTC` and fixed-offset identifiers and returns
//! `None` for every other (valid, available) named zone, so a caller that
//! genuinely needs an offset raises a loud error rather than silently
//! reporting a UTC-shifted wall clock.

use std::collections::BTreeMap;
use std::sync::OnceLock;

/// The time zone `Temporal.Now`'s optional argument defaults to, and what
/// `Temporal.Now.timeZoneId()` reports.
///
/// `UTC` rather than the host's configured zone, matching the default
/// `Intl.DateTimeFormat` already applies when no `timeZone` option is given
/// (`blueice_ecma402`'s `DateTimeFormat::try_new`). The two must agree: a
/// `Temporal.Now.zonedDateTimeISO()` value formatted through
/// `toLocaleString()` would otherwise shift.
pub(crate) const SYSTEM: &str = "UTC";

/// A syntactically valid `TimeZoneIdentifier`.
#[derive(Debug, Eq, PartialEq)]
enum TimeZoneIdentifier {
    /// A fixed UTC offset, in whole minutes east of UTC.
    Offset(i32),
    /// An IANA name, exactly as written — not yet checked for availability.
    Named(String),
}

/// Parses `TimeZoneIdentifier`: either `UTCOffset[~SubMinutePrecision]` or a
/// `TimeZoneIANAName`. Sub-minute offsets are deliberately *not* accepted as
/// identifiers, unlike the offsets allowed inside an instant string.
fn parse_identifier(source: &str) -> Option<TimeZoneIdentifier> {
    if let Some(minutes) = parse_offset_minutes(source) {
        return Some(TimeZoneIdentifier::Offset(minutes));
    }
    is_iana_name(source).then(|| TimeZoneIdentifier::Named(source.to_string()))
}

/// Parses `UTCOffset[~SubMinutePrecision]` (`±HH`, `±HHMM` or `±HH:MM`) into
/// signed whole minutes east of UTC.
fn parse_offset_minutes(source: &str) -> Option<i32> {
    let (sign, rest) = match source.as_bytes().split_first()? {
        (b'+', rest) => (1_i32, rest),
        (b'-', rest) => (-1_i32, rest),
        _ => return None,
    };
    let (hour, minute) = match rest {
        [hour_tens, hour_ones] => (two_digits(*hour_tens, *hour_ones)?, 0),
        [hour_tens, hour_ones, minute_tens, minute_ones]
        | [hour_tens, hour_ones, b':', minute_tens, minute_ones] => (
            two_digits(*hour_tens, *hour_ones)?,
            two_digits(*minute_tens, *minute_ones)?,
        ),
        _ => return None,
    };
    (hour <= 23 && minute <= 59).then(|| sign * (i32::from(hour) * 60 + i32::from(minute)))
}

fn two_digits(tens: u8, ones: u8) -> Option<u8> {
    (tens.is_ascii_digit() && ones.is_ascii_digit()).then(|| (tens - b'0') * 10 + (ones - b'0'))
}

/// `FormatOffsetTimeZoneIdentifier`: always the `±HH:MM` extended form, with
/// negative zero normalized to `+00:00`.
fn format_offset(minutes: i32) -> String {
    let sign = if minutes < 0 { '-' } else { '+' };
    let absolute = minutes.unsigned_abs();
    format!("{sign}{:02}:{:02}", absolute / 60, absolute % 60)
}

/// `TimeZoneIANAName`: slash-separated components, each starting with an ASCII
/// letter, `.` or `_`, continuing with those plus digits, `-` and `+`, and
/// never being exactly `.` or `..`.
fn is_iana_name(source: &str) -> bool {
    let leading = |byte: u8| byte.is_ascii_alphabetic() || matches!(byte, b'.' | b'_');
    !source.is_empty()
        && source.split('/').all(|component| {
            component != "."
                && component != ".."
                && matches!(component.as_bytes().split_first(), Some((first, rest))
                if leading(*first)
                    && rest.iter().all(|byte| {
                        leading(*byte) || byte.is_ascii_digit() || matches!(byte, b'-' | b'+')
                    }))
        })
}

/// The pinned IANA Zone-and-Link registry, keyed by its ASCII-lowercased
/// spelling so lookup is case-insensitive as the spec requires.
fn named_registry() -> &'static BTreeMap<String, String> {
    static REGISTRY: OnceLock<BTreeMap<String, String>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        blueice_ecma402::supported_values_of("timeZone")
            .unwrap_or_default()
            .into_iter()
            .map(|zone| (zone.to_ascii_lowercase(), zone))
            .collect()
    })
}

/// `GetAvailableNamedTimeZoneIdentifier`: the registry's own spelling of
/// `name`, matched without regard to ASCII case, or `None` when no such zone
/// is bundled.
fn available_named(name: &str) -> Option<&'static str> {
    named_registry()
        .get(&name.to_ascii_lowercase())
        .map(String::as_str)
}

/// `ToTemporalTimeZoneIdentifier`'s string path: resolves `source` to the
/// canonical identifier it names, or `Err(())` for a `RangeError`.
pub(crate) fn resolve(source: &str) -> Result<String, ()> {
    match parse_identifier(source) {
        Some(identifier) => resolve_parsed(identifier),
        None => resolve_from_date_time(source),
    }
}

fn resolve_parsed(identifier: TimeZoneIdentifier) -> Result<String, ()> {
    match identifier {
        TimeZoneIdentifier::Offset(minutes) => Ok(format_offset(minutes)),
        TimeZoneIdentifier::Named(name) => available_named(&name).map(str::to_owned).ok_or(()),
    }
}

/// `ParseTemporalTimeZoneString` step 3 onwards: a string that is not itself a
/// time-zone identifier may still be an ISO date-time string that *names* one.
/// The bracketed time-zone annotation wins; failing that a `Z` suffix means
/// UTC and a trailing offset becomes a fixed-offset identifier. A bare
/// date-time naming no zone at all is a `RangeError`.
fn resolve_from_date_time(source: &str) -> Result<String, ()> {
    // `-000000` is syntactically a signed six-digit extended year but is
    // rejected outright: negative zero is not a year. `iso::parse_date` would
    // otherwise read it as year 0.
    if has_negative_zero_year(source) {
        return Err(());
    }
    super::iso::parse_date(source).ok_or(())?;
    let annotations_at = source.find('[');
    if let Some(index) = annotations_at {
        let rest = &source[index + 1..];
        let end = rest.find(']').ok_or(())?;
        let body = rest[..end].strip_prefix('!').unwrap_or(&rest[..end]);
        // A `key=value` body is an ordinary annotation (e.g. `[u-ca=hebrew]`);
        // only a bare body in the first bracket is a time-zone annotation.
        if !body.contains('=') {
            return resolve_parsed(parse_identifier(body).ok_or(())?);
        }
    }
    let date_time = &source[..annotations_at.unwrap_or(source.len())];
    let time = &date_time[date_time.find(['T', 't']).ok_or(())? + 1..];
    let suffix = &time[time.find(['Z', 'z', '+', '-']).ok_or(())?..];
    if matches!(suffix.as_bytes(), [b'Z' | b'z']) {
        return Ok(SYSTEM.to_string());
    }
    parse_offset_minutes(suffix).map(format_offset).ok_or(())
}

fn has_negative_zero_year(source: &str) -> bool {
    source
        .strip_prefix('-')
        .or_else(|| source.strip_prefix('\u{2212}'))
        .and_then(|rest| rest.get(..6))
        .is_some_and(|year| year == "000000")
}

/// The UTC offset a resolved identifier implies, in seconds east of UTC.
///
/// `None` for a named zone other than `UTC`: that answer depends on the
/// instant and the IANA transition history, which Phase 26 Stage 1 Track E
/// owns. Callers turn `None` into a `RangeError` rather than assuming zero.
pub(crate) fn offset_seconds(identifier: &str) -> Option<i32> {
    if identifier.eq_ignore_ascii_case(SYSTEM) {
        return Some(0);
    }
    parse_offset_minutes(identifier).map(|minutes| minutes * 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minute_precision_offsets_but_not_sub_minute_ones() {
        assert_eq!(parse_offset_minutes("+01"), Some(60));
        assert_eq!(parse_offset_minutes("+01:30"), Some(90));
        assert_eq!(parse_offset_minutes("-0700"), Some(-420));
        assert_eq!(parse_offset_minutes("-00:00"), Some(0));
        assert_eq!(parse_offset_minutes("+24:00"), None);
        assert_eq!(parse_offset_minutes("+01:60"), None);
        // Sub-minute precision is valid inside an instant string but never as
        // a time-zone identifier.
        assert_eq!(parse_offset_minutes("-12:12:59"), None);
        assert_eq!(parse_offset_minutes("-12:12:59.9"), None);
        assert_eq!(parse_offset_minutes("01:30"), None);
        assert_eq!(parse_offset_minutes("+0"), None);
        assert_eq!(parse_offset_minutes("+1:30"), None);
        assert_eq!(parse_offset_minutes("+"), None);
        assert_eq!(parse_offset_minutes(""), None);
    }

    #[test]
    fn formats_offsets_in_the_extended_form_with_normalized_zero() {
        assert_eq!(format_offset(0), "+00:00");
        assert_eq!(format_offset(-420), "-07:00");
        assert_eq!(format_offset(106), "+01:46");
    }

    #[test]
    fn distinguishes_offsets_from_iana_names() {
        assert_eq!(
            parse_identifier("+01:00"),
            Some(TimeZoneIdentifier::Offset(60))
        );
        assert_eq!(
            parse_identifier("America/Los_Angeles"),
            Some(TimeZoneIdentifier::Named("America/Los_Angeles".into()))
        );
        assert_eq!(
            parse_identifier("UtC"),
            Some(TimeZoneIdentifier::Named("UtC".into()))
        );
        assert_eq!(parse_identifier(""), None);
        assert_eq!(parse_identifier("2021-08-19T17:30"), None);
        assert_eq!(parse_identifier("-12:12:59.9"), None);
        assert_eq!(parse_identifier("America//Vancouver"), None);
        assert_eq!(parse_identifier(".."), None);
    }

    #[test]
    fn looks_up_bundled_zone_names_without_regard_to_case() {
        assert_eq!(available_named("UTC"), Some("UTC"));
        assert_eq!(available_named("UtC"), Some("UTC"));
        assert_eq!(
            available_named("america/los_angeles"),
            Some("America/Los_Angeles")
        );
        assert_eq!(available_named("Mars/Olympus_Mons"), None);
    }

    #[test]
    fn resolves_plain_identifiers() {
        assert_eq!(resolve("UTC"), Ok("UTC".into()));
        assert_eq!(resolve("UtC"), Ok("UTC".into()));
        assert_eq!(resolve("+01:30"), Ok("+01:30".into()));
        assert_eq!(resolve("-07:00"), Ok("-07:00".into()));
        assert_eq!(resolve("America/Vancouver"), Ok("America/Vancouver".into()));
        assert_eq!(resolve(""), Err(()));
        assert_eq!(resolve("Mars/Olympus_Mons"), Err(()));
    }

    #[test]
    fn reads_a_zone_out_of_an_iso_date_time_string() {
        // Cases taken directly from Test262's
        // built-ins/Temporal/Now/zonedDateTimeISO/timezone-string-datetime.js.
        assert_eq!(resolve("2021-08-19T17:30Z"), Ok("UTC".into()));
        assert_eq!(resolve("2021-08-19T1730Z"), Ok("UTC".into()));
        assert_eq!(resolve("2021-08-19T17:30-07:00"), Ok("-07:00".into()));
        assert_eq!(resolve("2021-08-19T1730-0700"), Ok("-07:00".into()));
        assert_eq!(resolve("2021-08-19T17:30[UTC]"), Ok("UTC".into()));
        assert_eq!(resolve("2021-08-19T17:30Z[UTC]"), Ok("UTC".into()));
        assert_eq!(resolve("2021-08-19T17:30-07:00[UTC]"), Ok("UTC".into()));
        assert_eq!(
            resolve("2021-08-19T17:30[America/Vancouver]"),
            Ok("America/Vancouver".into())
        );
        // The bracketed annotation wins over the string's own offset.
        assert_eq!(
            resolve("2021-08-19T17:30:45.123456789-12:12[+01:46]"),
            Ok("+01:46".into())
        );
        // A leap second is a valid ISO string, so the zone still resolves.
        assert_eq!(resolve("2016-12-31T23:59:60+00:00[UTC]"), Ok("UTC".into()));
        // A critical time-zone annotation names the same zone.
        assert_eq!(resolve("2021-08-19T17:30[!UTC]"), Ok("UTC".into()));
        // Date-only is allowed when an annotation supplies the zone.
        assert_eq!(resolve("2000-05-02[UTC]"), Ok("UTC".into()));
        // A `key=value` first bracket is an ordinary annotation, never a zone,
        // so such a string names no zone of its own.
        assert_eq!(resolve("2000-05-02T15:23[u-ca=hebrew]"), Err(()));
        assert_eq!(resolve("2000-05-02T15:23Z[u-ca=hebrew]"), Ok("UTC".into()));
    }

    #[test]
    fn rejects_date_time_strings_that_name_no_usable_zone() {
        assert_eq!(resolve("2021-08-19T17:30"), Err(()));
        for offset in [
            "-07:00:01",
            "-07:00:00",
            "-07:00:00.1",
            "-07:00:00.000000000",
        ] {
            assert_eq!(resolve(&format!("2021-08-19T17:30{offset}")), Err(()));
        }
        assert_eq!(
            resolve("2021-08-19T17:30:45.123456789+23:59[+23:59:60]"),
            Err(())
        );
        assert_eq!(
            resolve("2021-08-19T17:30:45.123456789-12:12:59.9[-12:12:59.9]"),
            Err(())
        );
        assert_eq!(resolve("-000000-10-31T17:45Z"), Err(()));
        assert_eq!(resolve("-000000-10-31T17:45+00:00[UTC]"), Err(()));
        assert_eq!(resolve("2021-08-19T17:30[Mars/Olympus_Mons]"), Err(()));
    }

    #[test]
    fn answers_offsets_only_for_utc_and_fixed_offset_identifiers() {
        assert_eq!(offset_seconds("UTC"), Some(0));
        assert_eq!(offset_seconds("+01:30"), Some(5_400));
        assert_eq!(offset_seconds("-07:00"), Some(-25_200));
        // Track E's transition-history work, not this module's.
        assert_eq!(offset_seconds("America/Vancouver"), None);
    }
}
