// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The `Intl` namespace's lazy installation, exact-decimal NumberFormat
//! inputs and Temporal partial-date `dateStyle` formatting.

mod cov_g7_common;
use blueice_bluejs::{compile, parse, Value, Vm, VmConfig};
use cov_g7_common::{
    failures, sweep_fuel_after, sweep_fuel_each, sweep_heap_bare, sweep_heap_each, sweep_heap_runs,
    with_setup,
};

#[test]
fn the_namespace_exposes_every_service_constructor_and_locale_member() {
    assert_eq!(
        failures(
            r#"
var names = ['Collator', 'NumberFormat', 'DateTimeFormat', 'DisplayNames', 'DurationFormat', 'ListFormat',
  'PluralRules', 'RelativeTimeFormat', 'Segmenter', 'Locale'];
names.forEach(function (name) {
  var C = Intl[name];
  eq(name + ' function', typeof C, 'function');
  eq(name + ' tag', C.prototype[Symbol.toStringTag], 'Intl.' + name);
  eq(name + ' constructor', C.prototype.constructor, C);
  if (name !== 'Locale') eq(name + ' supportedLocalesOf', typeof C.supportedLocalesOf, 'function');
});
eq('DisplayNames length', Intl.DisplayNames.length, 2);
eq('Collator compare', typeof Object.getOwnPropertyDescriptor(Intl.Collator.prototype, 'compare').get, 'function');
eq('NumberFormat members', ['format', 'formatToParts', 'formatRange', 'formatRangeToParts', 'resolvedOptions'].every(function (m) { return m in Intl.NumberFormat.prototype }), true);
eq('DateTimeFormat members', ['format', 'formatToParts', 'formatRange', 'formatRangeToParts', 'resolvedOptions'].every(function (m) { return m in Intl.DateTimeFormat.prototype }), true);
eq('DisplayNames members', ['of', 'resolvedOptions'].every(function (m) { return m in Intl.DisplayNames.prototype }), true);
eq('DurationFormat members', ['format', 'formatToParts', 'resolvedOptions'].every(function (m) { return m in Intl.DurationFormat.prototype }), true);
eq('ListFormat members', ['format', 'formatToParts', 'resolvedOptions'].every(function (m) { return m in Intl.ListFormat.prototype }), true);
eq('PluralRules members', ['select', 'selectRange', 'resolvedOptions'].every(function (m) { return m in Intl.PluralRules.prototype }), true);
eq('RelativeTimeFormat members', ['format', 'formatToParts', 'resolvedOptions'].every(function (m) { return m in Intl.RelativeTimeFormat.prototype }), true);
eq('Segmenter members', ['segment', 'resolvedOptions'].every(function (m) { return m in Intl.Segmenter.prototype }), true);
var locale = ['toString', 'maximize', 'minimize', 'getCalendars', 'getCollations', 'getHourCycles', 'getNumberingSystems',
  'getTextInfo', 'getTimeZones', 'getWeekInfo', 'baseName', 'language', 'script', 'region', 'variants', 'calendar',
  'collation', 'hourCycle', 'caseFirst', 'numeric', 'numberingSystem', 'firstDayOfWeek'];
eq('Locale members', locale.every(function (m) { return m in Intl.Locale.prototype }), true);
eq('namespace tag', Intl[Symbol.toStringTag], 'Intl');
eq('global property', Object.getOwnPropertyDescriptor(globalThis, 'Intl').enumerable, false);
eq('canonical locales', Intl.getCanonicalLocales('EN-us').join(), 'en-US');
eq('supported values', Array.isArray(Intl.supportedValuesOf('calendar')), true);
"#
        ),
        ""
    );
}

#[test]
fn number_format_reads_exact_decimal_strings_and_falls_back_to_numbers() {
    assert_eq!(
        failures(
            r#"
var nf = new Intl.NumberFormat('en-US', { maximumFractionDigits: 20 });
var same = [['1e3', 1000], ['  12.5E-1 ', 1.25], ['1.5e+3', 1500], ['-2.5e1', -25], ['0.10', 0.1], ['7', 7], ['.5', 0.5], ['1E2', 100]];
same.forEach(function (pair) { eq('decimal ' + pair[0], nf.format(pair[0]), nf.format(pair[1])) });
eq('exact digits', nf.format('12345678901234567890.123456789'), '12,345,678,901,234,567,890.123456789');
eq('scientific exact', nf.format('123456789012345678901234567890e-10'), '12,345,678,901,234,567,890.123456789');
var fallback = ['', '1e', 'e5', '1e3e4', '1.2.3', '1e99999', '0x10', 'abc', '1 2', '+', '-', '.', 'Infinity'];
fallback.forEach(function (text) { eq('fallback ' + JSON.stringify(text), nf.format(text), nf.format(Number(text))) });
eq('bigint', nf.format(123456789012345678901234567890n), '123,456,789,012,345,678,901,234,567,890');
eq('formatToParts decimal', nf.formatToParts('1.5').map(function (p) { return p.type }).join(), 'integer,decimal,fraction');
eq('range', typeof nf.formatRange('1e3', '2e3'), 'string');
"#
        ),
        ""
    );
}

#[test]
fn date_time_format_applies_date_styles_to_partial_temporal_dates() {
    assert_eq!(
        failures(
            r#"
var ym = new Temporal.PlainYearMonth(2020, 3, 'gregory');
var md = new Temporal.PlainMonthDay(3, 15, 'gregory');
['full', 'long', 'medium', 'short'].forEach(function (style) {
  var f = new Intl.DateTimeFormat('en-US', { dateStyle: style, timeZone: 'UTC' });
  var yearMonth = f.format(ym);
  var monthDay = f.format(md);
  eq('year-month ' + style + ' year', yearMonth.indexOf('20') >= 0, true);
  eq('year-month ' + style + ' no day', yearMonth.indexOf('15') < 0 && yearMonth.indexOf('1') < 0 || style === 'short', true);
  eq('month-day ' + style + ' day', monthDay.indexOf('15') >= 0, true);
  eq('month-day ' + style + ' no year', monthDay.indexOf('2020') < 0, true);
});
var medium = new Intl.DateTimeFormat('en-US', { dateStyle: 'medium', timeZone: 'UTC' });
eq('medium year-month', medium.format(ym), 'Mar 2020');
eq('medium month-day', medium.format(md), 'Mar 15');
var long = new Intl.DateTimeFormat('en-US', { dateStyle: 'long', timeZone: 'UTC' });
eq('long year-month', long.format(ym), 'March 2020');
eq('long month-day', long.format(md), 'March 15');
var short = new Intl.DateTimeFormat('en-US', { dateStyle: 'short', timeZone: 'UTC' });
eq('short year-month', short.format(ym), '3/20');
eq('short month-day', short.format(md), '3/15');
var defaults = new Intl.DateTimeFormat('en-US', { timeZone: 'UTC' });
eq('default plain date', typeof defaults.format(new Temporal.PlainDate(2020, 3, 15)), 'string');
eq('default date time', defaults.format(new Temporal.PlainDateTime(2020, 3, 15, 1, 2, 3)).indexOf('2020') >= 0, true);
eq('default time', defaults.format(new Temporal.PlainTime(1, 2, 3)).indexOf('1') >= 0, true);
eq('default instant', defaults.format(Temporal.Instant.from('2020-03-15T01:02:03Z')).indexOf('2020') >= 0, true);
th('zoned', function () { defaults.format(Temporal.ZonedDateTime.from('2020-03-15T01:02:03+00:00[UTC]')) }, TypeError);
eq('default year-month', defaults.format(ym).indexOf('2020') >= 0, true);
eq('default month-day', defaults.format(md).indexOf('15') >= 0, true);
th('duration', function () { defaults.format(Temporal.Duration.from('PT1S')) }, TypeError);
th('time with date style', function () { medium.format(new Temporal.PlainTime(1, 2, 3)) }, TypeError);
th('date with time style', function () { new Intl.DateTimeFormat('en', { timeStyle: 'short' }).format(new Temporal.PlainDate(2020, 1, 1)) }, TypeError);
"#
        ),
        ""
    );
}

#[test]
fn the_intl_namespace_needs_a_definable_global() {
    assert_eq!(
        failures(
            r#"
Object.defineProperty(globalThis, 'Intl', { value: 1, configurable: false });
th('Intl cannot be installed over a locked global', function () { Intl }, TypeError);
"#
        ),
        ""
    );
    // Run without a global object at all: the namespace is still created.
    let mut vm = Vm::new(VmConfig::default()).unwrap();
    let value = vm
        .execute(&compile(&parse("Intl.getCanonicalLocales('EN-us')[0]").unwrap()).unwrap())
        .unwrap();
    assert_eq!(value, Value::String("en-US".into()));
}

const SETUP: &str = "var ym = new Temporal.PlainYearMonth(2020, 3, 'gregory'); var md = new Temporal.PlainMonthDay(3, 15, 'gregory');";

const BODIES: &[&str] = &[
    "new Intl.NumberFormat('en').format('1e3'); new Intl.NumberFormat('en').format('12.5E-1');",
    "new Intl.DateTimeFormat('en', { dateStyle: 'medium', timeZone: 'UTC', calendar: 'gregory' }).format(ym);",
    "new Intl.DateTimeFormat('en', { dateStyle: 'full', timeZone: 'UTC', calendar: 'gregory' }).format(md);",
    "new Intl.Collator().compare('a', 'b');",
    "new Intl.Locale('en-US').maximize();",
];

#[test]
fn every_heap_allocation_failure_reports_the_heap_limit() {
    // The namespace's own (large) creation, then services on a warm namespace.
    sweep_heap_runs("", "typeof Intl", 8000);
    // The namespace as the very first thing a bare VM builds, reached both by
    // name and through a locale-sensitive method that never names it.
    sweep_heap_bare("Intl.getCanonicalLocales('en')", 4000);
    sweep_heap_bare("(1234.5).toLocaleString('en')", 4000);
    sweep_heap_each(&with_setup(SETUP, BODIES));
}

#[test]
fn every_instruction_budget_exhaustion_reports_the_instruction_limit() {
    sweep_fuel_each(&with_setup(SETUP, BODIES));
    sweep_fuel_after("", "typeof Intl");
}
