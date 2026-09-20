// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use super::{annotations::*, datetime::*, duration::*, offset::*};
use num_bigint::BigInt;

/// [`parse_annotation_suffix`], keeping only the resolved calendar.
fn parse_annotations(source: &str) -> Result<Option<String>, ()> {
    parse_annotation_suffix(source).map(|annotations| annotations.calendar)
}

fn date(source: &str) -> Option<(i32, u8, u8)> {
    parse_date_time(source).map(|parsed| (parsed.year, parsed.month, parsed.day))
}

fn time_of(source: &str) -> Option<Time> {
    parse_time(source).and_then(|parsed| parsed.time)
}

fn offset(source: &str) -> Option<i64> {
    parse_date_time(source).and_then(|parsed| parsed.offset_nanoseconds)
}

#[test]
fn parses_extended_and_basic_calendar_dates() {
    // Temporal/PlainDate/from/argument-string.js.
    assert_eq!(date("2000-05-02"), Some((2000, 5, 2)));
    assert_eq!(date("+002020-06-01"), Some((2020, 6, 1)));
    assert_eq!(date("-010583-06-30"), Some((-10583, 6, 30)));
    assert_eq!(date("19761118"), Some((1976, 11, 18)));
    assert_eq!(date("+0019761118"), Some((1976, 11, 18)));
    assert_eq!(date("2000-05-02T15:23"), Some((2000, 5, 2)));
    assert_eq!(date("-999999-01-01"), Some((-999_999, 1, 1)));
}

#[test]
fn rejects_dates_that_break_the_grammar() {
    // Temporal/PlainDate/from/argument-string-invalid.js.
    for source in [
        "",
        "invalid iso8601",
        "2020-01-00",
        "2020-01-32",
        "2020-02-30",
        "2021-02-29",
        "2020-00-01",
        "2020-13-01",
        "02020-01-01",
        "2020-001-01",
        "2020-01-001",
        "+0002020-01-01",
        "2020-W01-1",
        "2020-001",
        "2020-01",
        "+002020-01",
        "01-01",
        "2020-0101",
        "202001-01",
        "1976-11-18junk",
        // Temporal/PlainDate/from/year-zero.js: a signed zero year.
        "-000000-10-31",
        "-000000-10-31T00:45",
        // Temporal/PlainDate/from/argument-string-minus-sign.js: the
        // Unicode minus sign U+2212 is not an ASCII sign.
        "\u{2212}009999-11-18T15:23:30.12",
        // Neither year width may borrow the other's: a six-digit year
        // needs its sign, and a four-digit one must not carry one.
        "002020-06-01",
        "+2020-06-01",
    ] {
        assert_eq!(date(source), None, "{source}");
    }
    // `+000000` is a legal extended year; only the *negative* zero is
    // prohibited.
    assert_eq!(date("+000000-12-07"), Some((0, 12, 7)));
    assert!(is_leap_year(2020) && !is_leap_year(2021));
}

#[test]
fn leap_year_follows_the_full_gregorian_century_rule() {
    // `2020`/`2021` above only exercise the plain "divisible by 4" rule.
    // The proleptic Gregorian rule this is meant to implement also has a
    // century exception (divisible by 100 is not a leap year) and a
    // 400-year exception to that exception (divisible by 400 is), and a
    // naive `year % 4 == 0` implementation would get both wrong: 1900
    // would be misreported as a leap year, and `2000-02-29`/`1900-02-29`
    // would misparse as valid/invalid respectively.
    assert!(is_leap_year(2000), "2000 is divisible by 400");
    assert!(!is_leap_year(1900), "1900 is divisible by 100 but not 400");
    assert!(!is_leap_year(2100), "2100 is divisible by 100 but not 400");
    assert!(is_leap_year(2400), "2400 is divisible by 400");
    assert_eq!(days_in_month(2000, 2), Some(29));
    assert_eq!(days_in_month(1900, 2), Some(28));
    assert_eq!(date("2000-02-29"), Some((2000, 2, 29)));
    assert_eq!(date("1900-02-29"), None);
}

#[test]
fn parses_time_of_day_in_both_forms_and_every_precision() {
    // Temporal/PlainTime/from/argument-string.js.
    assert_eq!(time_of("15"), Some((15, 0, 0, 0, 0, 0)));
    assert_eq!(time_of("15:23"), Some((15, 23, 0, 0, 0, 0)));
    assert_eq!(time_of("152330"), Some((15, 23, 30, 0, 0, 0)));
    assert_eq!(time_of("152330.1"), Some((15, 23, 30, 100, 0, 0)));
    assert_eq!(time_of("T00:30"), Some((0, 30, 0, 0, 0, 0)));
    assert_eq!(time_of("t003000.000000000"), Some((0, 30, 0, 0, 0, 0)));
    assert_eq!(
        time_of("15:23:30.123456789"),
        Some((15, 23, 30, 123, 456, 789))
    );
    // Temporal/Instant/from/argument-string.js: `,` is also a decimal
    // separator.
    assert_eq!(
        time_of("1976-11-18T15:23:30,12"),
        Some((15, 23, 30, 120, 0, 0))
    );
    // A space is a date/time separator, per
    // Temporal/PlainDate/from/argument-string-time-separators.js.
    assert_eq!(time_of("2000-05-02 15:23"), Some((15, 23, 0, 0, 0, 0)));
    assert_eq!(time_of("2000-05-02t15:23"), Some((15, 23, 0, 0, 0, 0)));
    // Temporal/PlainDate/from/argument-leap-second.js: a leap second
    // parses, clamped to :59.
    assert_eq!(time_of("2016-12-31T23:59:60"), Some((23, 59, 59, 0, 0, 0)));
}

#[test]
fn rejects_times_that_break_the_grammar() {
    // Temporal/PlainTime/from/argument-string-invalid.js and
    // Temporal/PlainDate/from/argument-string-{too-many-decimals,
    // invalid}.js, no-fractional-minutes-hours.js.
    for source in [
        "24:00",
        "15:60",
        "25:00:00",
        "01:60:00",
        "01:60:61",
        "001",
        "01:001",
        "01:01:001",
        "00:00:00.1234567891",
        "0000:00",
        "00:0000",
        "00:00:00+00:0000",
        "00:00:00+0000:00",
        "05:07.123",
        "12.5",
        "15:23:30.",
        "2019-10-01",
        "Z",
        "1976-11-18T15:23:30.12\u{2212}02:00",
        "",
        "00:00:00:00",
        "00:",
        "0:0",
        "1",
        "00:61",
        "00:00:61",
        "15:23:30.100junk",
    ] {
        assert_eq!(time_of(source), None, "{source}");
    }
    // A `PlainTime` additionally rejects the UTC designator, which a
    // wall-clock time cannot represent.
    assert_eq!(parse_plain_time("09:00:00Z"), None);
    assert_eq!(parse_plain_time("2019-10-01T09:00:00Z[UTC]"), None);
    assert_eq!(parse_plain_time("T"), None);
    assert_eq!(
        parse_plain_time("1976-11-18 12:34:56.987654321"),
        Some((12, 34, 56, 987, 654, 321))
    );
    assert_eq!(
        parse_plain_time("12:34:56.987654321+00:00:00,0"),
        Some((12, 34, 56, 987, 654, 321))
    );
}

#[test]
fn resolves_the_first_calendar_annotation_and_ignores_later_ones() {
    assert_eq!(
        parse_annotations("[u-ca=hebrew]"),
        Ok(Some("hebrew".to_string()))
    );
    assert_eq!(
        parse_annotations("[u-ca=hebrew][u-ca=discord]"),
        Ok(Some("hebrew".to_string()))
    );
    assert_eq!(
        parse_annotations("[UTC][u-ca=hebrew]"),
        Ok(Some("hebrew".to_string()))
    );
    assert_eq!(parse_annotations("[foo=bar]"), Ok(None));
    assert_eq!(parse_annotations(""), Ok(None));
}

#[test]
fn rejects_invalid_or_critical_unknown_annotations() {
    assert_eq!(parse_annotations("[!foo=bar]"), Err(()));
    assert_eq!(parse_annotations("[FOO=bar]"), Err(()));
    assert_eq!(parse_annotations("[u-CA=iso8601]"), Err(()));
}

fn duration(source: &str) -> Option<[i128; 10]> {
    let record = parse_duration_record(source)?;
    Some([
        record.years,
        record.months,
        record.weeks,
        record.days,
        record.hours,
        record.minutes,
        record.seconds,
        record.milliseconds,
        record.microseconds,
        record.nanoseconds,
    ])
}

#[test]
fn rejects_more_than_one_calendar_annotation_when_any_is_critical() {
    // Test262's PlainTime/prototype/until/argument-string-multiple-calendar.js
    // (and the identically-shaped PlainDate/PlainDateTime fixtures): a
    // repeated `u-ca` annotation is ignored, but becomes a syntax error
    // as soon as any of them carries the critical flag.
    assert_eq!(
        parse_annotations("[u-ca=iso8601][u-ca=discord]"),
        Ok(Some("iso8601".to_string()))
    );
    assert_eq!(parse_annotations("[u-ca=iso8601][!u-ca=iso8601]"), Err(()));
    assert_eq!(parse_annotations("[!u-ca=iso8601][u-ca=iso8601]"), Err(()));
    assert_eq!(
        parse_annotations("[UTC][u-ca=iso8601][!u-ca=iso8601]"),
        Err(())
    );
    // A single critical calendar annotation stays valid.
    assert_eq!(
        parse_annotations("[!u-ca=iso8601]"),
        Ok(Some("iso8601".to_string()))
    );
}

#[test]
fn parses_the_plain_time_string_grammar() {
    // Test262's PlainTime/prototype/until/argument-string-time-separators.js
    // and .../argument-string-calendar-annotation.js.
    for source in [
        "12:34:56.987654321",
        "T12:34:56.987654321",
        "t12:34:56.987654321",
        "1976-11-18T12:34:56.987654321",
        "1976-11-18t12:34:56.987654321",
        "1976-11-18 12:34:56.987654321",
        "12:34:56.987654321[u-ca=iso8601]",
        "12:34:56.987654321[UTC][u-ca=iso8601]",
        "12:34:56.987654321[!u-ca=unknown]",
        "12:34:56.987654321+00:00",
        "12:34:56.987654321-02:30[America/St_Johns]",
        "12:34:56.987654321+00:00:00,0",
        "1976-11-18T12:34:56.987654321+00:00[UTC]",
    ] {
        assert_eq!(
            parse_plain_time(source),
            Some((12, 34, 56, 987, 654, 321)),
            "{source}"
        );
    }
    assert_eq!(parse_plain_time("15:23"), Some((15, 23, 0, 0, 0, 0)));
    // A leap second is constrained to :59, per ParseISODateTime.
    assert_eq!(parse_plain_time("23:59:60"), Some((23, 59, 59, 0, 0, 0)));
    assert_eq!(
        parse_plain_time("23:59:60.170"),
        Some((23, 59, 59, 170, 0, 0))
    );
}

#[test]
fn requires_a_time_designator_only_where_a_date_reading_is_ambiguous() {
    // TemporalHelpers.ISO.plainTimeStringsAmbiguous(), via
    // PlainTime/from/argument-string-time-designator-required-for-disambiguation.js.
    for source in [
        "2021-12",
        "2021-12[-12:00]",
        "1214",
        "0229",
        "1130",
        "12-14",
        "12-14[-14:00]",
        "202112",
        "202112[UTC]",
        "2021-12[u-ca=iso8601]",
        "1130[u-ca=iso8601]",
    ] {
        assert_eq!(time_of(source), None, "ambiguous: {source}");
        assert!(
            time_of(&format!("T{source}")).is_some(),
            "designated: T{source}"
        );
    }
    // A space is only ever a date/time separator, never a substitute for
    // the time designator.
    assert_eq!(time_of(" 1130"), None);
    assert_eq!(time_of(" 15:23"), None);
    // TemporalHelpers.ISO.plainTimeStringsUnambiguous(): no month 13, no
    // 32nd day, no February 30th, no zeroth month.
    for (source, expected) in [
        ("2021-13", (20, 21, 0, 0, 0, 0)),
        ("202113", (20, 21, 13, 0, 0, 0)),
        ("0000-00", (0, 0, 0, 0, 0, 0)),
        ("000000", (0, 0, 0, 0, 0, 0)),
        ("1314", (13, 14, 0, 0, 0, 0)),
        ("13-14", (13, 0, 0, 0, 0, 0)),
        ("1232", (12, 32, 0, 0, 0, 0)),
        ("0230", (2, 30, 0, 0, 0, 0)),
        ("0631", (6, 31, 0, 0, 0, 0)),
        ("0000", (0, 0, 0, 0, 0, 0)),
        ("00-00", (0, 0, 0, 0, 0, 0)),
    ] {
        assert_eq!(time_of(source), Some(expected), "unambiguous: {source}");
    }
}

#[test]
fn parses_duration_strings_including_a_fraction_on_any_time_component() {
    // Every case here is taken from Test262's
    // built-ins/Temporal/Duration/from/argument-string.js.
    assert_eq!(duration("P1D"), Some([0, 0, 0, 1, 0, 0, 0, 0, 0, 0]));
    assert_eq!(
        duration("p1y1m1dt1h1m1s"),
        Some([1, 1, 0, 1, 1, 1, 1, 0, 0, 0]),
        "designators are case-insensitive"
    );
    assert_eq!(
        duration("P1Y1M1W1DT1H1M1.1S"),
        Some([1, 1, 1, 1, 1, 1, 1, 100, 0, 0])
    );
    assert_eq!(
        duration("P1Y1M1W1DT1H1M1.1234567S"),
        Some([1, 1, 1, 1, 1, 1, 1, 123, 456, 700])
    );
    assert_eq!(
        duration("P1Y1M1W1DT1H1M1,12S"),
        Some([1, 1, 1, 1, 1, 1, 1, 120, 0, 0]),
        "a comma is a decimal separator"
    );
    assert_eq!(
        duration("P1DT0.5M"),
        Some([0, 0, 0, 1, 0, 0, 30, 0, 0, 0]),
        "half a minute is thirty seconds"
    );
    assert_eq!(
        duration("P1DT0,5H"),
        Some([0, 0, 0, 1, 0, 30, 0, 0, 0, 0]),
        "half an hour is thirty minutes"
    );
    assert_eq!(
        duration("P1DT2H30.5M"),
        Some([0, 0, 0, 1, 2, 30, 30, 0, 0, 0]),
        "a fraction is valid on the last component present"
    );
    assert_eq!(
        duration("PT0.999999999H"),
        Some([0, 0, 0, 0, 0, 59, 59, 999, 996, 400]),
        "nine fractional hour digits are 3,599,999,996,400 ns, carried down exactly"
    );
    assert_eq!(duration("-P1D"), Some([0, 0, 0, -1, 0, 0, 0, 0, 0, 0]));
    assert_eq!(
        duration("\u{2212}P1D"),
        Some([0, 0, 0, -1, 0, 0, 0, 0, 0, 0]),
        "U+2212 MINUS SIGN is a sign"
    );
    assert_eq!(duration("+P1D"), Some([0, 0, 0, 1, 0, 0, 0, 0, 0, 0]));
    assert_eq!(duration("PT100M"), Some([0, 0, 0, 0, 0, 100, 0, 0, 0, 0]));
}

#[test]
fn rejects_duration_strings_outside_the_grammar() {
    assert_eq!(duration("not-a-duration"), None);
    assert_eq!(duration("P"), None, "at least one component is required");
    assert_eq!(duration("PT"), None, "a time designator needs a component");
    assert_eq!(duration("P1DT"), None);
    assert_eq!(duration("P1.5D"), None, "a date component has no fraction");
    assert_eq!(
        duration("PT0.5H30M"),
        None,
        "nothing smaller may follow a fraction"
    );
    assert_eq!(
        duration("P1M1Y"),
        None,
        "components are in descending order"
    );
    assert_eq!(duration("P1D1D"), None, "a component appears at most once");
    assert_eq!(duration("P1H"), None, "hours require the time designator");
    assert_eq!(duration("PT1D"), None, "days precede the time designator");
    assert_eq!(duration("PT1.1234567890S"), None, "at most nine digits");
    assert_eq!(duration("PT1.S"), None, "a fraction needs a digit");
    assert_eq!(duration("PT1HT1M"), None, "only one time designator");
    assert_eq!(duration("PD"), None, "a designator needs a number");
    assert_eq!(duration("P-1D"), None, "a component may not carry a sign");
}

#[test]
fn parses_duration_strings_with_a_fraction_on_the_last_unit_only() {
    assert!(parse_duration_record("P1Y2M3W4DT5H6M7.008009010S").is_some());
    // A fraction on the last present unit is valid, and cascades into
    // the units below it: `PT30.5M` is 30 minutes and 30 seconds.
    // (Test262 evidence that this *must* parse:
    // `built-ins/Temporal/Instant/prototype/add/argument-string-negative-fractional-units.js`
    // adds `"-PT1440.567890123M"` to an Instant.)
    let record = parse_duration_record("P1DT2H30.5M").expect("a fractional minute is valid");
    assert_eq!(record.minutes, 30);
    assert_eq!(record.seconds, 30);
    // A fraction anywhere but on the last unit is still a syntax error.
    assert!(parse_duration_record("PT30.5M10S").is_none());
    assert!(parse_duration_record("P1.5DT1H").is_none());
    assert!(parse_duration_record("not-a-duration").is_none());
    assert!(parse_duration_record("P1DT").is_none());
}

#[test]
fn cascades_a_fractional_hour_into_every_lower_unit() {
    // Test262's `add/argument-string-fractional-units-rounding-mode.js`:
    // `PT1.03125H` is exactly 3,712.5 seconds.
    let record = parse_duration_record("PT1.03125H").expect("a fractional hour is valid");
    assert_eq!(record.hours, 1);
    assert_eq!(record.minutes, 1);
    assert_eq!(record.seconds, 52);
    assert_eq!(record.milliseconds, 500);
    // `add/argument-string-negative-fractional-units.js`'s exact values.
    let record = parse_duration_record("-PT24.567890123H").expect("a fractional hour is valid");
    assert_eq!(
        (
            record.hours,
            record.minutes,
            record.seconds,
            record.milliseconds,
            record.microseconds,
            record.nanoseconds
        ),
        (-24, -34, -4, -404, -442, -800)
    );
}

#[test]
fn parses_offsets_including_zulu_and_bracket_termination() {
    assert_eq!(
        parse_utc_offset_prefix("Z").map(|(offset, rest)| (offset.nanoseconds, rest)),
        Some((0, ""))
    );
    assert_eq!(
        parse_utc_offset_prefix("+01:00").map(|(offset, rest)| (offset.nanoseconds, rest)),
        Some((3_600_000_000_000, ""))
    );
    assert_eq!(
        parse_utc_offset_prefix("-0130").map(|(offset, rest)| (offset.nanoseconds, rest)),
        Some((-5_400_000_000_000, ""))
    );
    assert_eq!(
        parse_utc_offset_prefix("-08:00[America/Vancouver]")
            .map(|(offset, rest)| (offset.nanoseconds, rest)),
        Some((-28_800_000_000_000, "[America/Vancouver]"))
    );
    assert_eq!(parse_utc_offset_prefix("+25:00"), None);
    assert_eq!(parse_utc_offset_prefix("01:00"), None);
}

#[test]
fn records_whether_an_offset_was_written_with_minute_precision() {
    for (source, minute_precision) in [
        ("Z", true),
        ("+00", true),
        ("+00:00", true),
        ("+0000", true),
        ("+00:00:00", false),
        ("+000000", false),
        ("+00:00:00.5", false),
    ] {
        let (offset, rest) =
            parse_utc_offset_prefix(source).unwrap_or_else(|| panic!("{source} parses"));
        assert!(rest.is_empty(), "{source}");
        assert_eq!(offset.minute_precision, minute_precision, "{source}");
    }
}

#[test]
fn parses_the_instant_grammar_including_leap_seconds_and_variant_separators() {
    assert_eq!(
        parse_instant("1970-01-01T00:00Z"),
        Some(InstantParts {
            date: (1970, 1, 1),
            time: (0, 0, 0, 0, 0, 0),
            offset_nanoseconds: 0,
        })
    );
    for separator in ['T', 't', ' '] {
        assert!(
            parse_instant(&format!("1970-01-01{separator}00:00Z")).is_some(),
            "{separator}"
        );
    }
    // Leap seconds clamp to :59 rather than being rejected.
    assert_eq!(
        parse_instant("2016-12-31T23:59:60Z").map(|parts| parts.time),
        Some((23, 59, 59, 0, 0, 0))
    );
    // Sub-minute offsets are exact in the offset position.
    assert_eq!(
        parse_instant("1970-01-01T00:19:32.37+00:19:32.37")
            .map(|parts| (parts.time, parts.offset_nanoseconds)),
        Some(((0, 19, 32, 370, 0, 0), 1_172_370_000_000))
    );
    // A calendar annotation is accepted and never resolved.
    assert!(parse_instant("1970-01-01T00:00Z[u-ca=discord]").is_some());
    assert!(parse_instant("1970-01-01T00Z[Europe/Vienna]").is_some());
}

#[test]
fn rejects_instant_strings_the_grammar_does_not_allow() {
    for source in [
        "",
        "invalid iso8601",
        // A bare date, or a date without a time, is not an instant.
        "2020-01-01",
        "2020-01-01T00:00:00",
        "2022-09-15Z",
        "2022-09-15+00:00[UTC]",
        "2020-01-01TZ",
        // Out-of-range or malformed components.
        "2020-01-00T00:00Z",
        "2020-02-30T00:00Z",
        "2020-13-01T00:00Z",
        "2020-01-01T25:00:00Z",
        "2020-01-01T01:60:00Z",
        "2020-01-01T00:00-24:00",
        // Trailing junk.
        "2020-01-01T00:00Zjunk",
        "2020-01-01T00:00:00+00:00[UTC][u-ca=iso8601]junk",
        // Unsupported year/component widths.
        "02020-01-01T00:00Z",
        "+0002020-01-01T00:00Z",
        "2020-001-01T00:00Z",
        "2020-01-001T00:00Z",
        "2020-01-01T001Z",
        "2020-01-01T01:001Z",
        "2020-W01-1T00:00Z",
        // Negative zero is never a valid extended year.
        "-000000-03-30T00:45Z",
        // More than nine fractional digits.
        "1970-01-01T00:00:00.1234567891Z",
        "1970-01-01T00+00:00:00.1234567890",
        // A sub-minute offset cannot be a time-zone annotation.
        "2021-08-19T17:30-07:00:01[-07:00:01]",
        "2021-08-19T17:30-07:00:00[-070000]",
        // More than one calendar annotation, any of them critical.
        "1970-01-01T00:00Z[u-ca=iso8601][!u-ca=iso8601]",
        "1970-01-01T00:00Z[!u-ca=iso8601][u-ca=iso8601]",
        // `parse_time_spec` (shared by `parse_iso_time_prefix` and
        // `parse_utc_offset_prefix`) has its own grammar rules distinct
        // from `scan_time`'s: a fourth colon-separated field is always a
        // syntax error...
        "1970-01-01T00:00:00:00Z",
        // ...and a decimal fraction belongs to the *seconds* field only,
        // so a fraction on a bare hour:minute (no seconds field at all)
        // is a syntax error rather than fractional minutes.
        "1970-01-01T00:19.5Z",
        // `parse_iso_time_prefix` clamps a `:60` leap second to `:59`
        // (see the passing case above) but still rejects anything past
        // that, e.g. a `:61`.
        "1970-01-01T00:00:61Z",
    ] {
        assert_eq!(parse_instant(source), None, "{source:?}");
    }
}

/// [`parse_annotation_suffix`]'s `key.is_empty() || value.is_empty()`
/// check, exercised only through [`parse_annotations`] elsewhere in this
/// module, none of which write an empty key or value.
#[test]
fn rejects_annotations_with_an_empty_key_or_value() {
    assert_eq!(parse_annotations("[=bar]"), Err(()));
    assert_eq!(parse_annotations("[foo=]"), Err(()));
}

/// [`scan_offset`] (the `Cursor`-based offset parser [`scan_utc_offset_suffix`]
/// and [`is_valid_time_zone_identifier`] share) has its own range checks
/// distinct from [`parse_time_spec`]'s, reached only through the full
/// `AnnotatedDateTime` grammar ([`parse_date_time`]) elsewhere in this
/// module -- and every existing case there uses a valid offset.
#[test]
fn rejects_out_of_range_offset_fields_in_the_full_date_time_grammar() {
    // An offset minute field over 59...
    assert_eq!(date("1976-11-18T15:23:30+00:60"), None);
    // ...and an offset second field over 59 -- unlike the time-of-day
    // field above it, an offset never gets leap-second tolerance.
    assert_eq!(date("1976-11-18T15:23:30+00:00:60"), None);
    // A valid offset that *does* carry an explicit, unfractioned seconds
    // field is still accepted (the completion path after that field,
    // when no further fraction follows it).
    assert_eq!(date("1976-11-18T15:23:30+05:30:15"), Some((1976, 11, 18)));
}

/// [`scan_annotations`]' leading-time-zone-annotation check
/// (`is_valid_time_zone_identifier`) rejecting a non-identifier-shaped,
/// non-`key=value` bracket body -- exercised elsewhere in this module
/// only through [`parse_annotation_suffix`]'s separate copy of the same
/// rule, never this one.
#[test]
fn rejects_a_leading_annotation_that_is_neither_a_time_zone_nor_key_value() {
    assert_eq!(date("1976-11-18T15:23[123]"), None);
}

#[test]
fn resolves_utc_and_fixed_offset_time_zones_regardless_of_the_instant() {
    let epoch = BigInt::from(0);
    for (source, expected) in [
        ("UTC", Ok(0)),
        ("utc", Ok(0)),
        ("+01:00", Ok(3_600_000_000_000)),
        ("-01:30", Ok(-5_400_000_000_000)),
        ("2021-08-19T17:30Z", Ok(0)),
        ("2021-08-19T17:30-07:00", Ok(-25_200_000_000_000)),
        ("2021-08-19T17:30-07:00[UTC]", Ok(0)),
        (
            "2021-08-19T17:30:45.123456789-12:12[+01:46]",
            Ok(6_360_000_000_000),
        ),
        ("2016-12-31T23:59:60+00:00[UTC]", Ok(0)),
        // Not a real time zone at all.
        ("", Err(())),
        ("2021-08-19T17:30", Err(())),
        ("2021-08-19T17:30-07:00:01", Err(())),
        ("2021-08-19T17:30-07:00:00", Err(())),
        ("2021-08-19T17:30:45.123456789+23:59[+23:59:60]", Err(())),
        // Syntactically zone-shaped, but not a real IANA name.
        ("Mars/Olympus_Mons", Err(())),
    ] {
        assert_eq!(
            resolve_time_zone_offset(source, &epoch),
            expected,
            "{source:?}"
        );
    }
}

#[test]
fn resolves_real_historical_offsets_for_named_iana_zones() {
    // Cases taken directly from the pinned Test262 corpus's
    // intl402/Temporal/Instant/prototype/toString/timezone-offset.js,
    // all at the epoch instant `new Temporal.Instant(0n)` uses.
    let epoch = BigInt::from(0);
    assert_eq!(
        resolve_time_zone_offset("Europe/Berlin", &epoch),
        Ok(3_600_000_000_000)
    );
    assert_eq!(
        resolve_time_zone_offset("America/New_York", &epoch),
        Ok(-5 * 3_600_000_000_000)
    );
    // A sub-minute historical offset: Monrovia was UTC-00:44:30 before
    // 1972 (the fixture's own expected display string,
    // "1969-12-31T23:15:30-00:45", rounds this to the minute for
    // `toString`'s offset field, but the underlying instant's real
    // offset — what this function resolves — is the exact -00:44:30).
    assert_eq!(
        resolve_time_zone_offset("Africa/Monrovia", &epoch),
        Ok(-(44 * 60_000_000_000_i128) - 30_000_000_000)
    );
    // A different instant in the same named zone resolves a different
    // (real, historical) offset — summer vs. winter New York.
    let summer = BigInt::from(1_720_480_004_i64) * 1_000_000_000_u32;
    assert_eq!(
        resolve_time_zone_offset("America/New_York", &summer),
        Ok(-4 * 3_600_000_000_000)
    );
    // A time-zone annotation's IANA name wins over the string's own
    // offset, and is resolved at the receiver's instant, not a fixed
    // offset taken from the string.
    assert_eq!(
        resolve_time_zone_offset("2021-08-19T17:30-07:00[America/Vancouver]", &epoch),
        resolve_time_zone_offset("America/Vancouver", &epoch)
    );
}

#[test]
fn parses_year_month_and_month_day_short_forms() {
    // TemporalHelpers.ISO.plainYearMonthStringsValid().
    for source in ["1976-11", "197611", "+00197611", "1976-11-10"] {
        let parsed = parse_year_month(source).expect(source);
        assert_eq!((parsed.year, parsed.month), (1976, 11), "{source}");
    }
    assert!(!parse_year_month("1976-11").unwrap().day_present);
    assert!(parse_year_month("1976-11-10").unwrap().day_present);
    assert_eq!(parse_year_month("-009999-11").unwrap().year, -9999);
    // TemporalHelpers.ISO.plainYearMonthStringsInvalid().
    for source in ["2020-13", "1976-11[U-CA=iso8601]", "1976-11[FOO=bar]"] {
        assert_eq!(parse_year_month(source), None, "{source}");
    }
    // TemporalHelpers.ISO.plainMonthDayStringsValid().
    for source in ["10-01", "1001", "--10-01", "--1001", "1965-10-01"] {
        let parsed = parse_month_day(source).expect(source);
        assert_eq!((parsed.month, parsed.day), (10, 1), "{source}");
    }
    assert!(!parse_month_day("--10-01").unwrap().year_present);
    assert!(parse_month_day("1965-10-01").unwrap().year_present);
    // February 29th is a valid month-day: the reference year is a leap
    // year.
    assert!(parse_month_day("02-29").is_some());
    assert_eq!(parse_month_day("02-30"), None);
    // TemporalHelpers.ISO.plainMonthDayStringsInvalid().
    assert_eq!(parse_month_day("11-18junk"), None);
    // Out-of-range years stay a caller's concern, not the grammar's.
    assert_eq!(parse_month_day("-999999-10-01").unwrap().year, -999_999);
}

#[test]
fn parses_utc_designators_and_offsets_down_to_nanoseconds() {
    // Temporal/Instant/from/argument-string.js and
    // instant-string-sub-minute-offset.js.
    assert!(parse_date_time("1976-11-18T15:23z").unwrap().utc_designator);
    assert_eq!(offset("1976-11-18T15:23:30+00"), Some(0));
    assert_eq!(
        offset("1976-11-18T15:23:30-02:00"),
        Some(-7_200_000_000_000)
    );
    assert_eq!(offset("1976-11-18T15:23:30+0000"), Some(0));
    assert_eq!(
        offset("1970-01-01T00:19:32.37+00:19:32.37"),
        Some(1_172_370_000_000)
    );
    assert_eq!(
        offset("1976-11-18T15:23:30.123456789-00:00:00.000000001"),
        Some(-1)
    );
    assert_eq!(offset("2000-05-02T00+000000,0"), Some(0));
    // Out-of-range and mis-separated offsets.
    for source in [
        "2020-01-01T00:00-24:00",
        "2020-01-01T00:00+24:00",
        "2025-01-01T00:00:00+00:0000",
        "2025-01-01T00:00:00+0000:00",
    ] {
        assert_eq!(parse_date_time(source), None, "{source}");
    }
    // A UTC offset is only ever legal after a time of day:
    // PlainDate/from/argument-string-date-with-utc-offset.js.
    for source in ["2022-09-15+00:00", "2022-09-15-02:30", "2022-09-15Z"] {
        assert_eq!(parse_date_time(source), None, "{source}");
    }
    // A fixed-offset time-zone identifier, read on its own.
    assert_eq!(
        parse_offset_identifier_nanoseconds("+01:00"),
        Some(3_600_000_000_000)
    );
    assert_eq!(
        parse_offset_identifier_nanoseconds("-0130"),
        Some(-5_400_000_000_000)
    );
    assert_eq!(parse_offset_identifier_nanoseconds("Z"), None);
    assert_eq!(parse_offset_identifier_nanoseconds("+25:00"), None);
    assert_eq!(parse_offset_identifier_nanoseconds("+00:00junk"), None);
    // An identifier stays minute-precision: sub-minute belongs to a
    // string's own offset, never to its zone annotation.
    assert_eq!(parse_offset_identifier_nanoseconds("-07:00:01"), None);
}

#[test]
fn parses_duration_strings_with_a_fraction_on_the_smallest_unit_present() {
    let fields = |source: &str| {
        parse_duration_record(source).map(|record| {
            [
                record.years,
                record.months,
                record.weeks,
                record.days,
                record.hours,
                record.minutes,
                record.seconds,
                record.milliseconds,
                record.microseconds,
                record.nanoseconds,
            ]
        })
    };
    // Temporal/Duration/from/argument-string.js.
    assert_eq!(fields("P1D"), Some([0, 0, 0, 1, 0, 0, 0, 0, 0, 0]));
    assert_eq!(
        fields("p1y1m1dt1h1m1s"),
        Some([1, 1, 0, 1, 1, 1, 1, 0, 0, 0])
    );
    assert_eq!(
        fields("P1Y1M1W1DT1H1M1.123456789S"),
        Some([1, 1, 1, 1, 1, 1, 1, 123, 456, 789])
    );
    assert_eq!(
        fields("P1Y1M1W1DT1H1M1,12S"),
        Some([1, 1, 1, 1, 1, 1, 1, 120, 0, 0])
    );
    assert_eq!(fields("P1DT0.5M"), Some([0, 0, 0, 1, 0, 0, 30, 0, 0, 0]));
    assert_eq!(fields("P1DT0,5H"), Some([0, 0, 0, 1, 0, 30, 0, 0, 0, 0]));
    assert_eq!(fields("+P1D"), Some([0, 0, 0, 1, 0, 0, 0, 0, 0, 0]));
    assert_eq!(
        fields("-P1Y1M1W1DT1H1M1.123456789S"),
        Some([-1, -1, -1, -1, -1, -1, -1, -123, -456, -789])
    );
    assert_eq!(fields("PT100M"), Some([0, 0, 0, 0, 0, 100, 0, 0, 0, 0]));
    // Temporal/Duration/from/string-with-skipped-units.js.
    assert_eq!(fields("P3Y4W"), Some([3, 0, 4, 0, 0, 0, 0, 0, 0, 0]));
    assert_eq!(
        fields("PT3H4.123456789S"),
        Some([0, 0, 0, 0, 3, 0, 4, 123, 456, 789])
    );
    // Temporal/Duration/from/argument-string-fractional-precision.js:
    // fractions are exact, not floating point.
    assert_eq!(
        fields("PT0.999999999H"),
        Some([0, 0, 0, 0, 0, 59, 59, 999, 996, 400])
    );
    assert_eq!(
        fields("PT0.000000011H"),
        Some([0, 0, 0, 0, 0, 0, 0, 0, 39, 600])
    );
    assert_eq!(
        fields("PT0.999999999M"),
        Some([0, 0, 0, 0, 0, 0, 59, 999, 999, 940])
    );
    assert_eq!(
        fields("PT0.000000011M"),
        Some([0, 0, 0, 0, 0, 0, 0, 0, 0, 660])
    );
    // Temporal/Duration/from/argument-string-negative-fractional-units.js.
    assert_eq!(
        fields("-PT24.567890123H"),
        Some([0, 0, 0, 0, -24, -34, -4, -404, -442, -800])
    );
    assert_eq!(
        fields("-PT1440.567890123M"),
        Some([0, 0, 0, 0, 0, -1440, -34, -73, -407, -380])
    );
}

#[test]
fn rejects_duration_strings_that_break_the_grammar() {
    // Temporal/Duration/from/argument-string-invalid.js and
    // argument-string-fractional-with-zero-subparts.js.
    for source in [
        "P1Y1M1W1DT1H1M1.123456789123S",
        "P0.5Y",
        "P1Y0,5M",
        "P1Y1M0.5W",
        "P1Y1M1W0,5D",
        "P1Y1M1W1DT0.5H5S",
        "P1Y1M1W1DT1.5H0,5M",
        "P1Y1M1W1DT1H0.5M0.5S",
        "P",
        "PT",
        "-P",
        "-PT",
        "+P",
        "+PT",
        "P1Y1M1W1DT1H1M1.01Sjunk",
        "P-1Y1M",
        "P1Y-1M",
        "P2H",
        "P2.5M",
        "P2S",
        "PT2.H3M",
        "PT2H3.2M3S",
        "PT.1H",
        "PT,1S",
        "PT0.1H0M",
        "PT0.1H0.0M",
        "PT0.1M0S",
        "",
        "not-a-duration",
        // Ordering and repetition are both fixed by the grammar.
        "P1D1Y",
        "P1Y1Y",
        "PT1S1H",
        "P1DT1HT1M",
        // Temporal/Duration/from/argument-string-is-infinity.js: a
        // component too large to represent is out of range.
        "P999999999999999999999999999999999999999999Y",
    ] {
        assert_eq!(parse_duration_record(source), None, "{source}");
    }
}

#[test]
fn keeps_representable_range_checks_out_of_the_grammar() {
    // Temporal/PlainYearMonth/from/argument-string-limits.js: the
    // year-month range is a month wider than the date range at each end.
    assert!(is_year_month_within_limits(-271_821, 4));
    assert!(!is_year_month_within_limits(-271_821, 3));
    assert!(is_year_month_within_limits(275_760, 9));
    assert!(!is_year_month_within_limits(275_760, 10));
    assert!(!is_year_month_within_limits(999_999, 1));
    assert!(is_year_month_within_limits(1976, 11));
    // The grammar itself accepts any 4- or 6-digit year, leaving the
    // limit to the type that cares: PlainMonthDay accepts
    // "-999999-10-01" that PlainDate must reject.
    assert_eq!(date("+999999-01-01"), Some((999_999, 1, 1)));
}
