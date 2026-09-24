// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `Temporal.Instant` (arithmetic, rounding, differences, `toString`, the
//! `from*` constructors), `ZonedDateTime.prototype.{add,subtract,round}` edge
//! cases, and the Temporal duration/ISO-string conversions they share.

mod cov_g7_common;
use cov_g7_common::{failures, sweep_fuel_each, sweep_heap_each, with_setup};

const PRELUDE: &str = r#"
var boom = {};
var I = Temporal.Instant, P = I.prototype, Z = Temporal.ZonedDateTime;
function throwsBoom(f) { try { f(); return false } catch (e) { return e === boom } }
function getter(name) { var o = {}; Object.defineProperty(o, name, { get: function () { throw boom } }); return o }
var i = I.fromEpochNanoseconds(1000000000123456789n);
var a = I.from('2020-01-01T00:00:00Z');
var b = I.from('2020-01-02T03:04:05.006007008Z');
var min = I.fromEpochNanoseconds(-8640000000000000000000n);
var max = I.fromEpochNanoseconds(8640000000000000000000n);
"#;

fn run(body: &str) -> String {
    failures(&format!("{PRELUDE}{body}"))
}

#[test]
fn methods_and_getters_reject_receivers_that_are_not_instants() {
    assert_eq!(
        run(r#"
var receivers = [{}, 1, undefined, null, new Temporal.PlainDate(2020, 1, 1), Temporal.Duration.from('PT1S'), Z.from('2020-01-01T00:00:00+00:00[UTC]')];
var calls = {
  add: [{ hours: 1 }], subtract: [{ hours: 1 }], round: ['second'], since: [a], until: [a], equals: [a],
  toString: [], toLocaleString: [], toJSON: [], valueOf: [], toZonedDateTimeISO: ['UTC']
};
Object.keys(calls).forEach(function (name) {
  receivers.forEach(function (receiver, index) {
    th(name + ' receiver ' + index, function () { P[name].apply(receiver, calls[name]) }, TypeError);
  });
});
['epochMilliseconds', 'epochNanoseconds'].forEach(function (name) {
  var get = Object.getOwnPropertyDescriptor(P, name).get;
  receivers.forEach(function (receiver, index) {
    th(name + ' getter ' + index, function () { get.call(receiver) }, TypeError);
  });
});
th('valueOf', function () { i.valueOf() }, TypeError);
th('valueOf via arithmetic', function () { return i + 1 }, TypeError);
"#),
        ""
    );
}

#[test]
fn add_and_subtract_apply_time_units_and_reject_calendar_units() {
    assert_eq!(
        run(r#"
eq('add', i.add({ hours: 1, minutes: 1, nanoseconds: 1 }).toString(), '2001-09-09T02:47:40.12345679Z');
eq('subtract', i.subtract('PT1H').toString(), '2001-09-09T00:46:40.123456789Z');
eq('add duration', i.add(Temporal.Duration.from({ seconds: 1 })).toString(), '2001-09-09T01:46:41.123456789Z');
eq('add string', i.add('PT0.000000001S').toString(), '2001-09-09T01:46:40.12345679Z');
['years', 'months', 'weeks', 'days'].forEach(function (unit) {
  var bag = {}; bag[unit] = 1;
  th('add ' + unit, function () { i.add(bag) }, RangeError);
  th('subtract ' + unit, function () { i.subtract(bag) }, RangeError);
});
th('add number', function () { i.add(5) }, TypeError);
th('add empty bag', function () { i.add({}) }, TypeError);
th('add bad string', function () { i.add('x') }, RangeError);
th('add lone surrogate', function () { i.add('\ud800') }, RangeError);
th('add non-integer', function () { i.add({ hours: 1.5 }) }, RangeError);
th('add plain date', function () { i.add(new Temporal.PlainDate(2020, 1, 1)) }, TypeError);
th('add overflow', function () { max.add({ nanoseconds: 1 }) }, RangeError);
th('subtract overflow', function () { min.subtract({ nanoseconds: 1 }) }, RangeError);
eq('add at limit', max.subtract({ nanoseconds: 1 }).epochNanoseconds, 8639999999999999999999n);
eq('add getter throws', throwsBoom(function () { i.add(getter('hours')) }), true);
"#),
        ""
    );
}

#[test]
fn round_validates_options_in_specification_order() {
    assert_eq!(
        run(r#"
eq('round string', i.round('second').toString(), '2001-09-09T01:46:40Z');
eq('round object', i.round({ smallestUnit: 'millisecond', roundingMode: 'ceil', roundingIncrement: 5 }).toString(), '2001-09-09T01:46:40.125Z');
eq('round hour', i.round({ smallestUnit: 'hour', roundingMode: 'floor' }).toString(), '2001-09-09T01:00:00Z');
eq('round half', i.round({ smallestUnit: 'minute', roundingMode: 'halfTrunc' }).toString(), '2001-09-09T01:47:00Z');
eq('round nanosecond', i.round('nanosecond').equals(i), true);
th('round nothing', function () { i.round() }, TypeError);
th('round primitive', function () { i.round(1) }, TypeError);
th('round missing unit', function () { i.round({}) }, RangeError);
th('round bad increment', function () { i.round({ smallestUnit: 'hour', roundingIncrement: 7 }) }, RangeError);
th('round zero increment', function () { i.round({ smallestUnit: 'hour', roundingIncrement: 0 }) }, RangeError);
th('round day', function () { i.round({ smallestUnit: 'day' }) }, RangeError);
th('round bad unit', function () { i.round('bogus') }, RangeError);
th('round bad mode', function () { i.round({ smallestUnit: 'hour', roundingMode: 'bogus' }) }, RangeError);
eq('round increment getter throws', throwsBoom(function () { i.round({ smallestUnit: 'hour', get roundingIncrement() { throw boom } }) }), true);
eq('round mode getter throws', throwsBoom(function () { i.round({ smallestUnit: 'hour', get roundingMode() { throw boom } }) }), true);
eq('round unit getter throws', throwsBoom(function () { i.round({ get smallestUnit() { throw boom } }) }), true);
eq('round unit coercion throws', throwsBoom(function () { i.round({ smallestUnit: { toString: function () { throw boom } } }) }), true);
"#),
        ""
    );
}

#[test]
fn since_and_until_resolve_units_increments_and_arguments() {
    assert_eq!(
        run(r#"
eq('until', a.until(b).toString(), 'PT97445.006007008S');
eq('since', b.since(a).toString(), 'PT97445.006007008S');
eq('negative', b.until(a).toString(), '-PT97445.006007008S');
eq('until largest', a.until(b, { largestUnit: 'minutes' }).toString(), 'PT1624M5.006007008S');
eq('until largest auto', a.until(b, { largestUnit: 'auto' }).toString(), 'PT97445.006007008S');
eq('until smallest', a.until(b, { smallestUnit: 'seconds' }).toString(), 'PT97445S');
eq('until hour', a.until(b, { smallestUnit: 'hour' }).toString(), 'PT27H');
eq('until rounding', a.until(b, { smallestUnit: 'second', roundingMode: 'ceil', roundingIncrement: 30 }).toString(), 'PT97470S');
eq('since rounding mode reflected', b.since(a, { smallestUnit: 'second', roundingMode: 'ceil' }).toString(), 'PT97446S');
eq('until nanoseconds', a.until(b, { largestUnit: 'nanoseconds' }).toString(), 'PT97445.006007008S');
eq('string argument', a.until('2020-01-01T00:00:01Z').toString(), 'PT1S');
eq('zoned argument', a.until(Z.from('2020-01-01T00:00:02+00:00[UTC]')).toString(), 'PT2S');
eq('instant argument', a.since(a).toString(), 'PT0S');
th('smallest larger than largest', function () { a.until(b, { largestUnit: 'seconds', smallestUnit: 'hours' }) }, RangeError);
th('increment too large', function () { a.until(b, { smallestUnit: 'hours', roundingIncrement: 24 }) }, RangeError);
th('increment not dividing', function () { a.until(b, { smallestUnit: 'hours', roundingIncrement: 7 }) }, RangeError);
th('largest days', function () { a.until(b, { largestUnit: 'days' }) }, RangeError);
th('smallest days', function () { a.until(b, { smallestUnit: 'days' }) }, RangeError);
th('bad mode', function () { a.until(b, { roundingMode: 'bogus' }) }, RangeError);
th('number argument', function () { a.until(1) }, TypeError);
th('undefined argument', function () { a.until() }, TypeError);
th('bad string argument', function () { a.until('nope') }, RangeError);
th('lone surrogate argument', function () { a.until('\ud800') }, RangeError);
th('options primitive', function () { a.until(b, 5) }, TypeError);
th('plain date argument', function () { a.until(new Temporal.PlainDate(2020, 1, 1)) }, RangeError);
['largestUnit', 'roundingIncrement', 'roundingMode', 'smallestUnit'].forEach(function (name) {
  eq('until ' + name + ' getter', throwsBoom(function () { a.until(b, getter(name)) }), true);
  eq('since ' + name + ' getter', throwsBoom(function () { a.since(b, getter(name)) }), true);
});
eq('argument toPrimitive number', (function () { try { a.until({ [Symbol.toPrimitive]: function () { return 5 } }) } catch (e) { return e instanceof TypeError } })(), true);
eq('argument toPrimitive throws', throwsBoom(function () { a.until({ [Symbol.toPrimitive]: function () { throw boom } }) }), true);
eq('argument object with string', a.until({ toString: function () { return '2020-01-01T00:00:03Z' } }).toString(), 'PT3S');
"#),
        ""
    );
}

#[test]
fn equals_and_compare_order_instants() {
    assert_eq!(
        run(r#"
eq('equals', a.equals(a.toString()), true);
eq('equals other', a.equals(b), false);
eq('equals throws', throwsBoom(function () { a.equals({ [Symbol.toPrimitive]: function () { throw boom } }) }), true);
th('equals number', function () { a.equals(1) }, TypeError);
eq('compare', [I.compare(a, b), I.compare(b, a), I.compare(a, a), I.compare(a.toString(), b)].join(), '-1,1,0,-1');
th('compare first bad', function () { I.compare(1, a) }, TypeError);
th('compare second bad', function () { I.compare(a, 'nope') }, RangeError);
"#),
        ""
    );
}

#[test]
fn to_string_precision_rounding_and_time_zone_options() {
    assert_eq!(
        run(r#"
eq('auto trims', I.fromEpochNanoseconds(1500000000n).toString(), '1970-01-01T00:00:01.5Z');
eq('auto whole', I.fromEpochNanoseconds(2000000000n).toString(), '1970-01-01T00:00:02Z');
var digits = ['2001-09-09T01:46:40Z', '2001-09-09T01:46:40.1Z', '2001-09-09T01:46:40.12Z', '2001-09-09T01:46:40.123Z',
  '2001-09-09T01:46:40.1234Z', '2001-09-09T01:46:40.12345Z', '2001-09-09T01:46:40.123456Z', '2001-09-09T01:46:40.1234567Z',
  '2001-09-09T01:46:40.12345678Z', '2001-09-09T01:46:40.123456789Z'];
digits.forEach(function (expected, n) { eq('digits ' + n, i.toString({ fractionalSecondDigits: n }), expected) });
eq('digits auto', i.toString({ fractionalSecondDigits: 'auto' }), '2001-09-09T01:46:40.123456789Z');
eq('digits fraction', i.toString({ fractionalSecondDigits: 2.9 }), '2001-09-09T01:46:40.12Z');
eq('digits rounding', i.toString({ fractionalSecondDigits: 2, roundingMode: 'ceil' }), '2001-09-09T01:46:40.13Z');
eq('unit minute', i.toString({ smallestUnit: 'minute' }), '2001-09-09T01:46Z');
eq('unit second', i.toString({ smallestUnit: 'second' }), '2001-09-09T01:46:40Z');
eq('unit millisecond', i.toString({ smallestUnit: 'millisecond' }), '2001-09-09T01:46:40.123Z');
eq('unit microsecond', i.toString({ smallestUnit: 'microsecond' }), '2001-09-09T01:46:40.123456Z');
eq('unit nanosecond', i.toString({ smallestUnit: 'nanosecond' }), '2001-09-09T01:46:40.123456789Z');
eq('unit wins over digits', i.toString({ smallestUnit: 'minute', fractionalSecondDigits: 5 }), '2001-09-09T01:46Z');
eq('unit ceil', i.toString({ smallestUnit: 'millisecond', roundingMode: 'ceil' }), '2001-09-09T01:46:40.124Z');
eq('unit plural', i.toString({ smallestUnit: 'seconds' }), '2001-09-09T01:46:40Z');
th('unit hour', function () { i.toString({ smallestUnit: 'hour' }) }, RangeError);
th('unit bogus', function () { i.toString({ smallestUnit: 'bogus' }) }, RangeError);
th('digits 10', function () { i.toString({ fractionalSecondDigits: 10 }) }, RangeError);
th('digits negative', function () { i.toString({ fractionalSecondDigits: -1 }) }, RangeError);
th('digits NaN', function () { i.toString({ fractionalSecondDigits: NaN }) }, RangeError);
th('digits Infinity', function () { i.toString({ fractionalSecondDigits: Infinity }) }, RangeError);
th('digits string', function () { i.toString({ fractionalSecondDigits: 'bogus' }) }, RangeError);
th('digits lone surrogate', function () { i.toString({ fractionalSecondDigits: '\ud800' }) }, RangeError);
th('digits symbol', function () { i.toString({ fractionalSecondDigits: Symbol() }) }, TypeError);
th('bad mode', function () { i.toString({ roundingMode: 'bogus' }) }, RangeError);
th('options primitive', function () { i.toString(5) }, TypeError);
eq('options undefined', i.toString(undefined), i.toString());
['fractionalSecondDigits', 'roundingMode', 'smallestUnit', 'timeZone'].forEach(function (name) {
  eq('toString ' + name + ' getter', throwsBoom(function () { i.toString(getter(name)) }), true);
});
eq('tz UTC', i.toString({ timeZone: 'UTC' }), '2001-09-09T01:46:40.123456789+00:00');
eq('tz positive', i.toString({ timeZone: '+05:30' }), '2001-09-09T07:16:40.123456789+05:30');
eq('tz negative', i.toString({ timeZone: '-08:00' }), '2001-09-08T17:46:40.123456789-08:00');
eq('tz named', i.toString({ timeZone: 'America/New_York' }), '2001-09-08T21:46:40.123456789-04:00');
eq('tz sub-minute offset rounds', I.from('1950-01-01T00:00:00Z').toString({ timeZone: 'Africa/Monrovia' }), '1949-12-31T23:15:30-00:45');
eq('tz zoned datetime', i.toString({ timeZone: Z.from('2020-01-01T00:00:00+02:00[+02:00]') }), '2001-09-09T03:46:40.123456789+02:00');
eq('tz with minute unit', i.toString({ timeZone: '+01:00', smallestUnit: 'minute' }), '2001-09-09T02:46+01:00');
th('tz invalid', function () { i.toString({ timeZone: 'Not/AZone' }) }, RangeError);
th('tz number', function () { i.toString({ timeZone: 5 }) }, TypeError);
th('tz lone surrogate', function () { i.toString({ timeZone: '\ud800' }) }, RangeError);
eq('extended years', I.from('+010000-01-01T00:00:00Z').toString() + ' ' + I.from('-000001-01-01T00:00:00Z').toString() + ' ' + I.from('0000-01-01T00:00:00Z').toString(),
  '+010000-01-01T00:00:00Z -000001-01-01T00:00:00Z 0000-01-01T00:00:00Z');
eq('limits', min.toString() + ' ' + max.toString(), '-271821-04-20T00:00:00Z +275760-09-13T00:00:00Z');
eq('toJSON', i.toJSON(), i.toString());
eq('JSON.stringify', JSON.stringify({ i: a }), '{"i":"2020-01-01T00:00:00Z"}');
eq('toLocaleString', i.toLocaleString('en-US', { timeZone: 'UTC', hour12: false }).indexOf('2001') >= 0, true);
eq('toLocaleString default', typeof i.toLocaleString(), 'string');
th('toLocaleString bad locale', function () { i.toLocaleString('x') }, RangeError);
eq('toLocaleString options getter', throwsBoom(function () { i.toLocaleString('en', getter('timeZone')) }), true);
eq('toLocaleString throws from locales', throwsBoom(function () { i.toLocaleString({ get length() { throw boom } }) }), true);
eq('toZonedDateTimeISO', i.toZonedDateTimeISO('UTC').toString(), '2001-09-09T01:46:40.123456789+00:00[UTC]');
"#),
        ""
    );
}

#[test]
fn from_constructors_validate_and_convert() {
    assert_eq!(
        run(r#"
eq('fromEpochMilliseconds', I.fromEpochMilliseconds(1500).toString(), '1970-01-01T00:00:01.5Z');
eq('fromEpochMilliseconds negative', I.fromEpochMilliseconds(-1).toString(), '1969-12-31T23:59:59.999Z');
eq('fromEpochMilliseconds string', I.fromEpochMilliseconds('3').epochMilliseconds, 3);
eq('fromEpochMilliseconds limit', I.fromEpochMilliseconds(8.64e15).epochNanoseconds, 8640000000000000000000n);
th('ms NaN', function () { I.fromEpochMilliseconds(NaN) }, RangeError);
th('ms Infinity', function () { I.fromEpochMilliseconds(Infinity) }, RangeError);
th('ms fraction', function () { I.fromEpochMilliseconds(1.5) }, RangeError);
th('ms range', function () { I.fromEpochMilliseconds(8.64e15 + 1) }, RangeError);
th('ms symbol', function () { I.fromEpochMilliseconds(Symbol()) }, TypeError);
eq('ms valueOf throws', throwsBoom(function () { I.fromEpochMilliseconds({ valueOf: function () { throw boom } }) }), true);
eq('fromEpochNanoseconds', I.fromEpochNanoseconds(0n).epochNanoseconds, 0n);
th('ns number', function () { I.fromEpochNanoseconds(1) }, TypeError);
th('ns string', function () { I.fromEpochNanoseconds('1') }, TypeError);
th('ns range', function () { I.fromEpochNanoseconds(8640000000000000000001n) }, RangeError);
th('ns negative range', function () { I.fromEpochNanoseconds(-8640000000000000000001n) }, RangeError);
eq('from instant', I.from(a).equals(a), true);
eq('from zoned', I.from(Z.from('2020-01-01T01:00:00+01:00[+01:00]')).equals(a), true);
eq('from offset string', I.from('2020-01-01T01:00:00+01:00').equals(a), true);
eq('from lowercase', I.from('2020-01-01t00:00:00z').equals(a), true);
th('from bad', function () { I.from('x') }, RangeError);
th('from lone surrogate', function () { I.from('\ud800') }, RangeError);
th('from number', function () { I.from(5) }, TypeError);
th('from undefined', function () { I.from() }, TypeError);
th('from object', function () { I.from({}) }, RangeError);
th('from too late', function () { I.from('+275760-09-13T00:00:00.000000001Z') }, RangeError);
th('from too early', function () { I.from('-271821-04-19T23:59:59.999999999Z') }, RangeError);
th('from plain date time', function () { I.from(new Temporal.PlainDateTime(2020, 1, 1)) }, RangeError);
th('from local time without offset', function () { I.from('2020-01-01T00:00:00') }, RangeError);
eq('prototype toStringTag', P[Symbol.toStringTag], 'Temporal.Instant');
"#),
        ""
    );
}

#[test]
fn zoned_date_time_add_and_round_cover_range_and_option_edges() {
    assert_eq!(
        run(r#"
var z = Z.from('2020-03-08T01:30:00-05:00[America/New_York]');
eq('add across gap', z.add({ hours: 1 }).toString(), '2020-03-08T03:30:00-04:00[America/New_York]');
eq('subtract', z.subtract({ days: 1 }).toString(), '2020-03-07T01:30:00-05:00[America/New_York]');
eq('add months reject', z.add({ months: 1 }, { overflow: 'reject' }).toString(), '2020-04-08T01:30:00-04:00[America/New_York]');
th('add overflow option', function () { z.add({ hours: 1 }, { overflow: 'x' }) }, RangeError);
th('add options primitive', function () { z.add({ hours: 1 }, 5) }, TypeError);
eq('add options getter', throwsBoom(function () { z.add({ hours: 1 }, getter('overflow')) }), true);
th('add receiver', function () { Z.prototype.add.call({}, { hours: 1 }) }, TypeError);
th('add bad duration', function () { z.add('x') }, RangeError);
var last = Z.from('+275760-09-13T00:00:00+00:00[UTC]');
th('add beyond range', function () { last.add({ days: 1 }) }, RangeError);
th('add beyond range hours', function () { last.add({ hours: 1 }) }, RangeError);
th('subtract before range', function () { Z.from('-271821-04-20T00:00:00+00:00[UTC]').subtract({ hours: 1 }) }, RangeError);
eq('round shorthand', z.round('hour').toString(), '2020-03-08T03:00:00-04:00[America/New_York]');
eq('round day', z.round('day').toString(), '2020-03-08T00:00:00-05:00[America/New_York]');
eq('round days plural', z.round('days').toString(), '2020-03-08T00:00:00-05:00[America/New_York]');
eq('round day ceil', z.round({ smallestUnit: 'day', roundingMode: 'ceil' }).toString(), '2020-03-09T00:00:00-04:00[America/New_York]');
eq('round options', z.round({ smallestUnit: 'minute', roundingIncrement: 15, roundingMode: 'floor' }).toString(), '2020-03-08T01:30:00-05:00[America/New_York]');
th('round nothing', function () { z.round() }, TypeError);
th('round empty', function () { z.round({}) }, RangeError);
th('round bad unit', function () { z.round('bogus') }, RangeError);
th('round day increment', function () { z.round({ smallestUnit: 'day', roundingIncrement: 2 }) }, RangeError);
th('round hour increment', function () { z.round({ smallestUnit: 'hour', roundingIncrement: 24 }) }, RangeError);
th('round bad mode', function () { z.round({ smallestUnit: 'hour', roundingMode: 'bogus' }) }, RangeError);
th('round receiver', function () { Z.prototype.round.call({}, 'hour') }, TypeError);
['roundingIncrement', 'roundingMode', 'smallestUnit'].forEach(function (name) {
  eq('round ' + name + ' getter', throwsBoom(function () { z.round(getter(name)) }), true);
});
var edge = Z.from('+275760-09-13T23:59:00+23:59[+23:59]');
th('round carries beyond the date range', function () { edge.round({ smallestUnit: 'hour', roundingMode: 'ceil' }) }, RangeError);
var late = Z.from('+275760-09-12T23:50:00+00:00[UTC]').withTimeZone('+00:30');
th('round beyond the instant range', function () { late.round({ smallestUnit: 'hour', roundingMode: 'ceil' }) }, RangeError);
th('round day at the edge', function () { last.round('day') }, RangeError);
"#),
        ""
    );
}

#[test]
fn duration_and_iso_string_conversions_reject_malformed_input() {
    assert_eq!(
        run(r#"
th('duration lone surrogate', function () { Temporal.Duration.from('\ud800') }, RangeError);
th('duration bad string', function () { Temporal.Duration.from('P') }, RangeError);
th('duration number', function () { Temporal.Duration.from(5) }, TypeError);
th('duration no fields', function () { Temporal.Duration.from({}) }, TypeError);
th('duration temporal object', function () { Temporal.Duration.from(new Temporal.PlainDate(2020, 1, 1)) }, TypeError);
th('duration non-integer', function () { Temporal.Duration.from({ hours: 1.5 }) }, RangeError);
th('duration mixed signs', function () { Temporal.Duration.from({ hours: 1, minutes: -1 }) }, RangeError);
eq('duration getter throws', throwsBoom(function () { Temporal.Duration.from(getter('days')) }), true);
th('plain time bad annotation', function () { Temporal.PlainTime.from('12:00[bad zone]') }, RangeError);
th('plain time critical annotation', function () { Temporal.PlainTime.from('12:00[!unknown=1]') }, RangeError);
th('plain month day bad annotation', function () { Temporal.PlainMonthDay.from('01-15[bad zone]') }, RangeError);
th('plain month day critical annotation', function () { Temporal.PlainMonthDay.from('01-15[!unknown=1]') }, RangeError);
eq('plain time annotation', Temporal.PlainTime.from('12:00[u-ca=iso8601]').toString(), '12:00:00');
eq('plain month day annotation', Temporal.PlainMonthDay.from('01-15[u-ca=iso8601]').toString(), '01-15');
"#),
        ""
    );
}

#[test]
fn differences_near_the_range_limits_report_out_of_range_windows() {
    assert_eq!(
        run(r#"
var PDT = Temporal.PlainDateTime, PYM = Temporal.PlainYearMonth;
var expand = { largestUnit: 'years', smallestUnit: 'months', roundingMode: 'expand', roundingIncrement: 5 };
var weeks = { largestUnit: 'years', smallestUnit: 'weeks', roundingMode: 'expand', roundingIncrement: 100 };
var outOfRange = [
  function () { PDT.from('-271820-01-01T00:00').until('-271821-06-01T00:00', expand) },
  function () { PDT.from('-271820-01-01T00:00').since('-271821-06-01T00:00', expand) },
  function () { PDT.from('+275759-01-01T00:00').until('+275760-08-01T00:00', expand) },
  function () { PDT.from('+275759-01-01T00:00').until('+275760-08-01T00:00', weeks) },
  function () { PYM.from('-271820-01').until('-271821-06', expand) },
  function () { PYM.from('+275759-01').until('+275760-08', expand) },
  function () { PYM.from('+275759-03').since('+275760-08', expand) },
  function () { PYM.from('-271821-04').until('-271818-05', expand) },
];
outOfRange.forEach(function (call, index) { th('out of range ' + index, call, RangeError) });
eq('inside the range', PDT.from('-271821-06-01T00:00').until('-271820-01-01T00:00', expand).toString(), 'P10M');
eq('backwards inside the range', PDT.from('+275760-08-01T00:00').until('+275759-01-01T00:00', expand).toString(), '-P1Y10M');
eq('year-month inside the range', PYM.from('+275760-08').until('+275759-03', expand).toString(), '-P1Y5M');
"#),
        ""
    );
}

const SETUP: &str = "var i = Temporal.Instant.fromEpochNanoseconds(1000000000123456789n); var a = Temporal.Instant.from('2020-01-01T00:00:00Z'); var z = Temporal.ZonedDateTime.from('2020-03-08T01:30:00-05:00[America/New_York]');";

const BODIES: &[&str] = &[
    "i.add({ hours: 1 }); i.subtract('PT1S');",
    "i.round('second');",
    "i.until(a); i.since(a, { smallestUnit: 'second' });",
    "i.equals(a); Temporal.Instant.compare(i, a);",
    "i.toString({ fractionalSecondDigits: 4, timeZone: '+01:00' });",
    "i.toString({ smallestUnit: 'minute', timeZone: 'America/New_York' });",
    "Temporal.Instant.fromEpochMilliseconds(5); Temporal.Instant.from(a);",
    "i.toLocaleString('en-US', { timeZone: 'UTC' });",
    "z.round('hour').add({ hours: 1 });",
    "z.round('day');",
    "try { Temporal.Instant.from('nope') } catch (e) {}",
];

#[test]
fn every_heap_allocation_failure_reports_the_heap_limit() {
    sweep_heap_each(&with_setup(SETUP, BODIES));
}

#[test]
fn every_instruction_budget_exhaustion_reports_the_instruction_limit() {
    sweep_fuel_each(&with_setup(SETUP, BODIES));
}
