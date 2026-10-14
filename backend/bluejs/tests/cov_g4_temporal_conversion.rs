// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Conversions between Temporal values and their property bags, calendars,
//! time zones and locale formatting: the errors each step reports.

mod cov_g4_common;
use cov_g4_common::failures;

const PRELUDE: &str = r#"
var boom = {};
function throwsBoom(f) { try { f(); return false } catch (e) { return e === boom } }
function getter(bag, name) { Object.defineProperty(bag, name, { get: function () { throw boom } }); return bag }
function coercion(bag, name) { bag[name] = { valueOf: function () { throw boom }, toString: function () { throw boom } }; return bag }
var PDT = Temporal.PlainDateTime, ZDT = Temporal.ZonedDateTime, PD = Temporal.PlainDate;
"#;

fn run(body: &str) -> String {
    failures(&format!("{PRELUDE}{body}"))
}

#[test]
fn valueof_on_a_plain_date_or_date_time_is_a_type_error() {
    assert_eq!(
        run(r#"
th('PlainDate.valueOf', function () { new PD(2020, 1, 1).valueOf() }, TypeError);
th('PlainDateTime.valueOf', function () { new PDT(2020, 1, 1).valueOf() }, TypeError);
th('unary plus', function () { +new PD(2020, 1, 1) }, TypeError);
"#),
        ""
    );
}

#[test]
fn a_calendar_identifier_must_be_a_well_formed_calendar_string() {
    assert_eq!(
        run(r#"
th('constructor lone surrogate', function () { new PD(2020, 1, 1, '\ud800') }, RangeError);
th('bag calendar lone surrogate', function () { PD.from({ year: 2020, month: 1, day: 1, calendar: '\ud800' }) }, RangeError);
th('withCalendar lone surrogate', function () { new PD(2020, 1, 1).withCalendar('\ud800') }, RangeError);
th('withCalendar unknown', function () { new PD(2020, 1, 1).withCalendar('nope') }, RangeError);
th('withCalendar missing', function () { new PD(2020, 1, 1).withCalendar() }, TypeError);
eq('withCalendar', new PD(2020, 1, 1).withCalendar('gregory').calendarId, 'gregory');
eq('calendar of a Temporal value', PD.from({ year: 2020, month: 1, day: 1, calendar: new PD(2020, 1, 1, 'gregory') }).calendarId, 'gregory');
"#),
        ""
    );
}

#[test]
fn every_property_bag_field_reports_a_throwing_getter_or_conversion() {
    assert_eq!(
        run(r#"
var base = function () { return { year: 2020, month: 1, day: 1, hour: 1, minute: 1, second: 1, millisecond: 1, microsecond: 1, nanosecond: 1, calendar: 'gregory', era: 'ce', eraYear: 2020 } };
['calendar', 'day', 'era', 'eraYear', 'hour', 'microsecond', 'millisecond', 'minute', 'month', 'monthCode', 'nanosecond', 'second', 'year'].forEach(function (name) {
  eq(name + ' getter', throwsBoom(function () { PDT.from(getter(base(), name)) }), true);
});
['day', 'era', 'eraYear', 'hour', 'microsecond', 'millisecond', 'minute', 'month', 'nanosecond', 'second', 'year'].forEach(function (name) {
  eq(name + ' conversion', throwsBoom(function () { PDT.from(coercion(base(), name)) }), true);
});
var zoned = function () { var bag = base(); bag.timeZone = 'UTC'; return bag };
['timeZone', 'offset'].forEach(function (name) {
  eq(name + ' getter', throwsBoom(function () { ZDT.from(getter(zoned(), name)) }), true);
});
eq('offset conversion', throwsBoom(function () { ZDT.from(coercion(zoned(), 'offset')) }), true);
th('timeZone that is not a string', function () { ZDT.from(coercion(zoned(), 'timeZone')) }, TypeError);
th('offset with a lone surrogate', function () { var bag = zoned(); bag.offset = '\ud800'; ZDT.from(bag) }, RangeError);
th('bad offset option', function () { ZDT.from(zoned(), { offset: 'sometimes' }) }, RangeError);
th('bad disambiguation option', function () { ZDT.from(zoned(), { disambiguation: 'sometimes' }) }, RangeError);
th('era with a lone surrogate', function () { PDT.from({ year: 2020, month: 1, day: 1, calendar: 'gregory', era: '\ud800', eraYear: 1 }) }, RangeError);
eq('era conversion throws', throwsBoom(function () { PDT.from({ year: 2020, month: 1, day: 1, calendar: 'gregory', era: { toString: function () { throw boom } }, eraYear: 1 }) }), true);
"#),
        ""
    );
}

#[test]
fn zoned_date_time_transitions_and_locale_formatting_report_bad_arguments() {
    assert_eq!(
        run(r#"
var z = ZDT.from('2020-01-01T00:00[UTC]');
th('transition without direction', function () { z.getTimeZoneTransition() }, TypeError);
th('transition bag without direction', function () { z.getTimeZoneTransition({}) }, RangeError);
th('transition bad direction', function () { z.getTimeZoneTransition('sideways') }, RangeError);
th('transition lone surrogate direction', function () { z.getTimeZoneTransition({ direction: '\ud800' }) }, RangeError);
eq('transition direction getter', throwsBoom(function () { z.getTimeZoneTransition({ get direction() { throw boom } }) }), true);
eq('transition direction conversion', throwsBoom(function () { z.getTimeZoneTransition({ direction: { toString: function () { throw boom } } }) }), true);
eq('no transition in UTC', z.getTimeZoneTransition('next'), null);
th('locale string bad locale', function () { z.toLocaleString('not a locale') }, RangeError);
th('locale string bad style', function () { z.toLocaleString('en', { dateStyle: 'bogus' }) }, RangeError);
th('locale string style and component', function () { z.toLocaleString('en', { dateStyle: 'short', hour: 'numeric' }) }, TypeError);
th('locale string time zone option', function () { z.toLocaleString('en', { timeZone: 'UTC' }) }, TypeError);
th('locale string calendar mismatch', function () { new ZDT(0n, 'UTC', 'hebrew').toLocaleString('en', { calendar: 'gregory' }) }, RangeError);
eq('locale string options getter', throwsBoom(function () { z.toLocaleString('en', { get hour() { throw boom } }) }), true);
th('locale string offset beyond what the formatter serves', function () { new ZDT(0n, '+23:00').toLocaleString('en') }, RangeError);
th('locale string empty calendar option', function () { z.toLocaleString('en', { calendar: '' }) }, RangeError);
eq('locale string', z.toLocaleString('en'), '1/1/2020, 12:00:00 AM UTC');
eq('locale string at the range limits', new ZDT(8640000000000000000000n, 'UTC').toLocaleString('en'), '9/13/275760, 12:00:00 AM UTC');
"#),
        ""
    );
}

#[test]
fn every_temporal_getter_reads_its_field() {
    assert_eq!(
        run(r#"
var d = new Temporal.Duration(1, 2, 3, 4, 5, 6, 7, 8, 9, 10);
eq('duration fields', [d.years, d.months, d.weeks, d.days, d.hours, d.minutes, d.seconds, d.milliseconds, d.microseconds, d.nanoseconds].join(), '1,2,3,4,5,6,7,8,9,10');
eq('duration sign', d.sign + ':' + d.blank, '1:false');
var t = new Temporal.PlainTime(1, 2, 3, 4, 5, 6);
eq('time fields', [t.hour, t.minute, t.second, t.millisecond, t.microsecond, t.nanosecond].join(), '1,2,3,4,5,6');
var p = new PD(2020, 3, 8);
eq('week fields', [p.dayOfWeek, p.dayOfYear, p.weekOfYear, p.yearOfWeek, p.daysInWeek].join(), '7,68,10,2020,7');
var g = p.withCalendar('gregory');
eq('non-iso week fields', [g.dayOfWeek, g.dayOfYear, g.weekOfYear, g.yearOfWeek, g.daysInWeek].join(), '7,68,,,7');
var h = p.withCalendar('hebrew');
eq('hebrew date', [h.year, h.monthCode, h.day, h.era, h.eraYear].join(), '5780,M06,12,am,5780');
eq('hebrew year shape', h.inLeapYear === (h.monthsInYear === 13) && h.daysInYear > 350 && h.daysInMonth === 29 && h.dayOfYear > 100, true);
eq('iso fields', [p.year, p.month, p.monthCode, p.day, p.era, p.eraYear, p.monthsInYear, p.daysInMonth, p.daysInYear, p.inLeapYear].join(), '2020,3,M03,8,,,12,31,366,true');
var i = Temporal.Instant.fromEpochNanoseconds(-1500000n);
eq('epoch fields', i.epochMilliseconds + ':' + i.epochNanoseconds, '-2:-1500000');
var z = ZDT.from('2020-03-08T12:00-04:00[America/New_York]');
eq('zoned fields', [z.timeZoneId, z.offset, z.offsetNanoseconds, z.hoursInDay].join(), 'America/New_York,-04:00,-14400000000000,23');
"#),
        ""
    );
}
