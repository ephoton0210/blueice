// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `Temporal.ZonedDateTime` receiver and argument conversion errors.

mod cov_g3_support;
use cov_g3_support::assert_true;

#[test]
fn zoned_date_time_methods_reject_receivers_that_are_not_zoned_date_times() {
    for method in [
        "with",
        "add",
        "subtract",
        "toString",
        "toPlainDate",
        "startOfDay",
    ] {
        for receiver in [
            "1",
            "undefined",
            "{}",
            "new Temporal.PlainDate(2000, 1, 1)",
            "new Temporal.Instant(0n)",
        ] {
            assert_true(&format!(
                "try {{
                    Temporal.ZonedDateTime.prototype.{method}.call({receiver}, {{}});
                    false
                }} catch (e) {{ e instanceof TypeError }}"
            ));
        }
    }
}

#[test]
fn from_reads_other_temporal_objects_as_property_bags_and_rejects_bad_strings() {
    for source in [
        // A Temporal object that is not a ZonedDateTime is read as a bag,
        // which has no time zone.
        "try { Temporal.ZonedDateTime.from(new Temporal.PlainDate(2000, 1, 1)); false }
         catch (e) { e instanceof TypeError }",
        // A ZonedDateTime argument is returned as-is after reading options.
        "const z = new Temporal.ZonedDateTime(0n, 'UTC');
         Temporal.ZonedDateTime.from(z).equals(z)",
        // A string with a lone surrogate cannot even be decoded.
        "try { Temporal.ZonedDateTime.from('2000-01-01T00:00[UTC]\\ud800'); false }
         catch (e) { e instanceof RangeError }",
        // Non-strings are never stringified.
        "try { Temporal.ZonedDateTime.from(19761118); false } catch (e) { e instanceof TypeError }",
        // A date-only string whose start of day is outside the instant range.
        "try { Temporal.ZonedDateTime.from('-271821-04-18[UTC]'); false }
         catch (e) { e instanceof RangeError }",
        "try { Temporal.ZonedDateTime.from('+275760-09-14[UTC]'); false }
         catch (e) { e instanceof RangeError }",
        // Missing or unusable time-zone annotations and calendars.
        "try { Temporal.ZonedDateTime.from('2000-01-01T00:00'); false } catch (e) { e instanceof RangeError }",
        "try { Temporal.ZonedDateTime.from('2000-01-01T00:00[Nowhere/Zone]'); false } catch (e) { e instanceof RangeError }",
        "try { Temporal.ZonedDateTime.from('2000-01-01T00:00[UTC][u-ca=nonsense]'); false } catch (e) { e instanceof RangeError }",
    ] {
        assert_true(source);
    }
}

#[test]
fn a_duration_relative_to_string_must_be_a_valid_zoned_date_time_string() {
    for relative_to in [
        "'2000-01-01T00:00[Nowhere/Zone]'",
        "'2000-01-01T00:00[UTC][u-ca=nonsense]'",
        "'2000-01-01T00:00[+99:00]'",
    ] {
        assert_true(&format!(
            "try {{
                new Temporal.Duration(0, 0, 0, 1).total({{ unit: 'day', relativeTo: {relative_to} }});
                false
            }} catch (e) {{ e instanceof RangeError }}"
        ));
    }
    assert_true(
        "new Temporal.Duration(0, 0, 0, 1).total({ unit: 'hour', relativeTo: '2000-01-01T00:00[UTC]' }) === 24",
    );
}

#[test]
fn fixed_month_calendar_addition_beyond_the_supported_years_is_a_range_error() {
    for duration in [
        // The year no longer fits 32 bits, going up and going down.
        "{ years: 4294967295 }",
        "{ years: -4294967295, months: -13 }",
        // The months carry never looks at the year, which is out of range.
        "{ years: -4294967295, months: -1 }",
        // The year fits 32 bits but no calendar supports it.
        "{ years: -2000000000, months: -13 }",
        "{ years: 2000000000 }",
    ] {
        assert_true(&format!(
            "const date = Temporal.PlainDate.from('2000-06-15').withCalendar('coptic');
             try {{ date.add({duration}); false }} catch (e) {{ e instanceof RangeError }}"
        ));
    }
    assert_true(
        "const date = Temporal.PlainDate.from('2000-06-15').withCalendar('coptic');
         date.add({ years: 1, months: 1 }).calendarId === 'coptic' && date.add({ days: 0 }).equals(date)",
    );
}

#[test]
fn a_calendar_difference_between_equal_dates_is_empty() {
    for source in [
        "const persian = Temporal.PlainDate.from('2000-06-15').withCalendar('persian');
         persian.until(persian, { largestUnit: 'year' }).blank && persian.until(persian, { largestUnit: 'month' }).blank",
        "const hebrew = Temporal.PlainDate.from('2000-06-15').withCalendar('hebrew');
         hebrew.until(hebrew, { largestUnit: 'year' }).blank && hebrew.since(hebrew, { largestUnit: 'month' }).blank",
        "const chinese = Temporal.PlainDate.from('2000-06-15').withCalendar('chinese');
         chinese.until(chinese, { largestUnit: 'year' }).blank",
        // The same date at different times of day reaches the date difference
        // of the date-time and year-month forms with equal dates.
        "for (const calendar of ['persian', 'hebrew', 'chinese', 'coptic']) {
           const start = Temporal.PlainDateTime.from('2000-06-15T00:00').withCalendar(calendar);
           const end = Temporal.PlainDateTime.from('2000-06-15T12:00').withCalendar(calendar);
           const d = start.until(end, { largestUnit: 'year' });
           if (d.hours !== 12 || d.years !== 0 || d.months !== 0 || d.days !== 0) throw new Error(calendar + JSON.stringify(d));
           const back = end.until(start, { largestUnit: 'month' });
           if (back.hours !== -12) throw new Error(calendar + ' back');
         } true",
        "for (const calendar of ['persian', 'hebrew']) {
           const ym = Temporal.PlainYearMonth.from({ year: 1400, monthCode: 'M01', calendar });
           if (!ym.until(ym, { largestUnit: 'year' }).blank) throw new Error(calendar);
         } true",
    ] {
        assert_true(source);
    }
}
