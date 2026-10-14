// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Temporal property-bag readers and `Temporal.Duration` operations: throwing
//! getters and conversions, malformed strings and rejected options.
mod cov_g2_support;

use cov_g2_support::expect_true;

const PRELUDE: &str = "function thrown(f) { try { f(); return 'none'; } catch (e) { return e; } }
    function isType(f) { return thrown(f) instanceof TypeError; }
    function isRange(f) { return thrown(f) instanceof RangeError; }
    function isSyntax(f) { return thrown(f) instanceof SyntaxError; }
    var boom = { valueOf() { throw 'boom'; }, toString() { throw 'boom'; } };
    var d = new Temporal.Duration(0, 0, 0, 1, 2, 3);";

fn check(body: &str) {
    expect_true(&format!("{PRELUDE}\n{body}"));
}

#[test]
fn duration_operations_report_throwing_options_and_receivers() {
    check(
        "thrown(() => d.with({ get years() { throw 'y'; } })) === 'y' &&
           thrown(() => d.round({ get largestUnit() { throw 'lu'; } })) === 'lu' &&
           thrown(() => d.round({ largestUnit: 'day', get relativeTo() { throw 'rt'; } })) === 'rt' &&
           thrown(() => d.total({ unit: 'day', get relativeTo() { throw 'rt2'; } })) === 'rt2' &&
           thrown(() => Temporal.Duration.compare(d, d, { get relativeTo() { throw 'rt3'; } })) === 'rt3' &&
           isRange(() => d.add({ years: 1 })) && isRange(() => d.subtract({ months: 1 })) &&
           d.add(d).days === 2 && d.subtract(d).days === 0 &&
           isType(() => Temporal.Duration.prototype.with.call({}, {})) &&
           isType(() => Temporal.Duration.prototype.round.call(1, {})) &&
           isType(() => Temporal.Duration.prototype.toString.call({})) &&
           isType(() => Temporal.Duration.prototype.toLocaleString.call({})) &&
           isType(() => Temporal.Duration.prototype.total.call({}, 'day')) &&
           isType(() => Temporal.Duration.prototype.add.call({}, d)) &&
           isType(() => Temporal.Duration.prototype.negated.call({})) &&
           isType(() => Temporal.Duration.prototype.abs.call({}))",
    );
}

#[test]
fn duration_to_string_validates_its_options() {
    check(
        "thrown(() => d.toString({ get fractionalSecondDigits() { throw 'f'; } })) === 'f' &&
           thrown(() => d.toString({ fractionalSecondDigits: boom })) === 'boom' &&
           isRange(() => d.toString({ fractionalSecondDigits: '\\ud800' })) &&
           isRange(() => d.toString({ fractionalSecondDigits: 'nine' })) &&
           isRange(() => d.toString({ fractionalSecondDigits: NaN })) &&
           isRange(() => d.toString({ fractionalSecondDigits: Infinity })) &&
           isRange(() => d.toString({ fractionalSecondDigits: 10 })) &&
           d.toString({ fractionalSecondDigits: 'auto' }) === 'P1DT2H3M' &&
           d.toString({ fractionalSecondDigits: 2 }) === 'P1DT2H3M0.00S' &&
           isType(() => d.toString(1)) && isRange(() => d.toString({ roundingMode: 'bad' })) &&
           thrown(() => d.toString({ get roundingMode() { throw 'rm'; } })) === 'rm' &&
           isRange(() => d.toString({ smallestUnit: 'day' })) &&
           d.toLocaleString('en').length > 0 && isRange(() => d.toLocaleString('bad_locale!'))",
    );
}

#[test]
fn integer_and_string_property_bag_readers_propagate_getter_errors() {
    check(
        "thrown(() => Temporal.PlainDate.from({ get year() { throw 'y'; }, month: 1, day: 1 })) === 'y' &&
           thrown(() => Temporal.PlainTime.from({ get hour() { throw 'h'; } })) === 'h' &&
           thrown(() => Temporal.PlainDate.from({ year: 2020, month: 1, day: 1, get monthCode() { throw 'mc'; } })) === 'mc' &&
           thrown(() => Temporal.PlainDate.from({ year: 2020, day: 1, monthCode: boom })) === 'boom' &&
           thrown(() => Temporal.PlainDate.from({ calendar: 'gregory', get era() { throw 'e'; }, eraYear: 1, month: 1, day: 1 })) === 'e' &&
           isRange(() => Temporal.PlainDate.from({ calendar: 'gregory', era: '\\ud800', eraYear: 1, month: 1, day: 1 })) &&
           thrown(() => Temporal.ZonedDateTime.from({ timeZone: 'UTC', year: 2020, month: 1, day: 1, get offset() { throw 'o'; } })) === 'o' &&
           thrown(() => Temporal.ZonedDateTime.from({ timeZone: 'UTC', year: 2020, month: 1, day: 1, offset: boom })) === 'boom' &&
           isRange(() => Temporal.ZonedDateTime.from({ timeZone: 'UTC', year: 2020, month: 1, day: 1, offset: '\\ud800' })) &&
           isType(() => Temporal.ZonedDateTime.from({ timeZone: 'UTC', year: 2020, month: 1, day: 1, offset: 0 })) &&
           isRange(() => Temporal.ZonedDateTime.from({ timeZone: 'UTC', year: 2020, month: 1, day: 1, offset: 'nonsense' })) &&
           thrown(() => new Temporal.Duration(1, boom)) === 'boom' &&
           isRange(() => new Temporal.Duration(1.5)) && isRange(() => new Temporal.Duration(Infinity))",
    );
}

#[test]
fn instant_construction_converts_its_argument_like_to_bigint() {
    check(
        "thrown(() => new Temporal.Instant(boom)) === 'boom' && isSyntax(() => new Temporal.Instant('\\ud800')) &&
           isSyntax(() => new Temporal.Instant('12x')) && isType(() => new Temporal.Instant(5)) &&
           new Temporal.Instant('').epochNanoseconds === 0n && new Temporal.Instant(true).epochNanoseconds === 1n &&
           new Temporal.Instant(' 7 ').epochNanoseconds === 7n && new Temporal.Instant(9n).epochNanoseconds === 9n",
    );
}

#[test]
fn zoned_date_time_strings_honor_every_option() {
    check(
        "var z = Temporal.ZonedDateTime.from('2020-01-02T03:04:05.123456789+00:00[UTC]');
         z.toString() === '2020-01-02T03:04:05.123456789+00:00[UTC]' &&
           z.toString({ calendarName: 'always' }).endsWith('[UTC][u-ca=iso8601]') &&
           z.toString({ calendarName: 'critical' }).endsWith('[!u-ca=iso8601]') &&
           z.toString({ calendarName: 'never', timeZoneName: 'never', offset: 'never' }) === '2020-01-02T03:04:05.123456789' &&
           z.toString({ timeZoneName: 'critical' }).includes('[!UTC]') &&
           z.toString({ fractionalSecondDigits: 3 }).includes('.123+') &&
           z.toString({ smallestUnit: 'minute' }).startsWith('2020-01-02T03:04+') &&
           z.toString({ smallestUnit: 'nanosecond' }).includes('.123456789+')",
    );
}

#[test]
fn with_methods_report_throwing_time_and_era_fields() {
    check(
        "var pt = new Temporal.PlainTime(1, 2, 3);
         var pdt = new Temporal.PlainDateTime(2020, 1, 2, 3, 4, 5);
         var ym = new Temporal.PlainYearMonth(2020, 1, 'gregory');
         var pd = new Temporal.PlainDate(2020, 1, 2, 'gregory');
         thrown(() => pt.with({ get hour() { throw 'h'; } })) === 'h' &&
         thrown(() => pdt.with({ get minute() { throw 'm'; } })) === 'm' &&
         thrown(() => pdt.with({ hour: boom })) === 'boom' &&
         thrown(() => pd.with({ get era() { throw 'e'; } })) === 'e' &&
         thrown(() => ym.with({ get era() { throw 'e2'; } })) === 'e2' &&
         thrown(() => pd.with({ era: '\\ud800', eraYear: 1 })) !== 'none' &&
         thrown(() => ym.with({ era: '\\ud800', eraYear: 1 })) !== 'none' &&
         thrown(() => pd.with({ era: Symbol() })) !== 'none'",
    );
}
