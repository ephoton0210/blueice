// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Temporal error paths: property bags with throwing getters, strings that
//! are not text, and options a method rejects.
mod cov_g6_support;

use cov_g6_support::check;

#[test]
fn with_reads_each_field_and_stops_at_a_throwing_getter() {
    check(&[
        (
            "Temporal.PlainDate.from('2020-01-01').with({ get calendar() { throw 'cal'; } })",
            "throw:undefined:cal",
        ),
        (
            "Temporal.PlainDate.from('2020-01-01').with({ get timeZone() { throw 'zone'; } })",
            "throw:undefined:zone",
        ),
        (
            "Temporal.PlainDate.from('2020-01-01[u-ca=gregory]').with({ get era() { throw 'era'; } })",
            "throw:undefined:era",
        ),
        (
            "Temporal.PlainDate.from('2020-01-01[u-ca=gregory]').with({ get eraYear() { throw 'eraYear'; } })",
            "throw:undefined:eraYear",
        ),
        (
            "Temporal.PlainDateTime.from('2020-01-01T00:00').with({ get microsecond() { throw 'micro'; } })",
            "throw:undefined:micro",
        ),
        (
            "Temporal.PlainDateTime.from('2020-01-01T00:00').with({ get millisecond() { throw 'milli'; } })",
            "throw:undefined:milli",
        ),
        (
            "Temporal.PlainDateTime.from('2020-01-01T00:00').with({ get nanosecond() { throw 'nano'; } })",
            "throw:undefined:nano",
        ),
        (
            "Temporal.PlainDateTime.from('2020-01-01T00:00').with({ get second() { throw 'second'; } })",
            "throw:undefined:second",
        ),
    ]);
}

#[test]
fn date_strings_that_are_not_text_are_range_errors() {
    check(&[
        (
            "Temporal.PlainDate.from('2020-01-01\\ud800')",
            "throw:RangeError:invalid Temporal.PlainDate string",
        ),
        (
            "Temporal.PlainDateTime.from('2020-01-01T00:00\\ud800')",
            "throw:RangeError:invalid Temporal.PlainDateTime string",
        ),
        (
            "Temporal.Now.plainDateISO('UTC\\ud800')",
            "throw:RangeError:invalid Temporal time zone",
        ),
        (
            "new Temporal.Instant(0n).toZonedDateTimeISO('UTC\\ud800')",
            "throw:RangeError:invalid Temporal time zone",
        ),
    ]);
}

#[test]
fn a_plain_date_or_date_time_is_not_made_from_another_kind_of_temporal_object() {
    check(&[
        (
            "Temporal.PlainDateTime.from(new Temporal.Instant(0n))",
            "throw:TypeError:Temporal date fields require year",
        ),
        (
            "Temporal.PlainDate.from(new Temporal.Instant(0n))",
            "throw:TypeError:Temporal date fields require year",
        ),
    ]);
}

#[test]
fn a_time_zone_argument_must_be_text_or_a_zoned_date_time() {
    check(&[
        (
            "Temporal.PlainDate.from('2020-01-01').toZonedDateTime(new Temporal.PlainDate(2020, 1, 1))",
            "throw:TypeError:Temporal time zone must be a string or a Temporal.ZonedDateTime",
        ),
        (
            "Temporal.PlainDate.from('2020-01-01').toZonedDateTime({ timeZone: 'UTC', get plainTime() { throw 'pt'; } })",
            "throw:undefined:pt",
        ),
        (
            "Temporal.PlainDate.from('2020-01-01').toZonedDateTime({ get timeZone() { throw 'tz'; } })",
            "throw:undefined:tz",
        ),
        (
            "Temporal.PlainDate.from('2020-01-01').toZonedDateTime('UTC\\ud800')",
            "throw:RangeError:invalid Temporal time zone",
        ),
    ]);
}

#[test]
fn a_disambiguation_option_that_throws_stops_zoned_construction() {
    check(&[
        (
            "Temporal.ZonedDateTime.from('2020-01-01T00:00[UTC]', { disambiguation: { toString() { throw 'dis'; } } })",
            "throw:undefined:dis",
        ),
        (
            "Temporal.ZonedDateTime.from({ year: 2020, month: 1, day: 1, timeZone: 'UTC' }, { get disambiguation() { throw 'dis2'; } })",
            "throw:undefined:dis2",
        ),
    ]);
}

#[test]
fn an_offset_with_half_a_minute_or_more_rounds_away_from_zero_to_the_minute() {
    // Chicago's local mean time was UTC-5:50:36 and Tokyo's UTC+9:18:59.
    check(&[
        (
            "Temporal.ZonedDateTime.from('1800-01-01T00:00:00-05:51[America/Chicago]', { offset: 'reject' }).toString()",
            "ok:1800-01-01T00:00:00-05:51[America/Chicago]",
        ),
        (
            "Temporal.ZonedDateTime.from('1800-01-01T00:00:00+09:19[Asia/Tokyo]', { offset: 'reject' }).toString()",
            "ok:1800-01-01T00:00:00+09:19[Asia/Tokyo]",
        ),
    ]);
}

#[test]
fn an_unknown_calendar_name_option_is_a_range_error() {
    check(&[(
        "Temporal.PlainDate.from('2020-01-01').toString({ calendarName: 'bogus' })",
        "throw:RangeError:invalid calendarName option",
    )]);
}

#[test]
fn zoned_difference_reads_its_options_in_order_and_stops_at_a_throwing_getter() {
    for option in [
        "largestUnit",
        "roundingIncrement",
        "roundingMode",
        "smallestUnit",
    ] {
        let expression = format!(
            "Temporal.ZonedDateTime.from('2020-01-01T00:00[UTC]').until('2020-01-02T00:00[UTC]',
               {{ get {option}() {{ throw '{option}'; }} }})"
        );
        assert_eq!(
            cov_g6_support::outcome(&expression),
            format!("throw:undefined:{option}"),
            "{expression}"
        );
    }
}
