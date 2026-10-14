// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Calendar rounding, range rejection and DST duration bubbling at VM entry.

use blueice_bluejs::{compile, parse, Value, Vm};

fn assert_script(source: &str) {
    assert_eq!(
        Vm::default()
            .execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true),
        "{source}"
    );
}

#[test]
fn plain_calendar_rounding_preserves_until_since_direction_across_units() {
    assert_script(
        r#"
        var passed = 0;
        for (var calendar of ['iso8601', 'gregory', 'hebrew']) {
            var a = Temporal.PlainDateTime.from('2020-01-31T23:59:59.999999999').withCalendar(calendar);
            var b = Temporal.PlainDateTime.from('2021-03-01T00:00:00.000000001').withCalendar(calendar);
            for (var unit of ['years', 'months', 'weeks', 'days']) {
                for (var mode of ['ceil', 'floor', 'expand', 'trunc', 'halfEven']) {
                    var options = {largestUnit:unit, smallestUnit:unit, roundingMode:mode};
                    if (a.until(b, options).toString() !== b.since(a, options).toString()) throw 'positive direction';
                    if (b.until(a, options).toString() !== a.since(b, options).toString()) throw 'negative direction';
                    passed++;
                }
            }
        }
        passed === 60
    "#,
    );
}

#[test]
fn zoned_rounding_bubbles_past_a_month_boundary_and_clears_smaller_fields() {
    assert_script(
        r#"
        var a = Temporal.ZonedDateTime.from('2020-01-01T00:00Z[UTC]');
        var b = Temporal.ZonedDateTime.from('2020-11-30T00:00Z[UTC]');
        var d = a.until(b, {largestUnit:'years', smallestUnit:'days', roundingIncrement:7, roundingMode:'ceil'});
        d.years === 0 && d.months === 11 && d.days === 0 && d.toString() === 'P11M'
    "#,
    );
}

#[test]
fn dst_differences_keep_until_since_direction_and_exact_day_totals() {
    assert_script(
        r#"
        var passed = 0;
        for (var pair of [
            ['2019-03-09T12:00-05:00[America/New_York]', '2019-03-11T12:00-04:00[America/New_York]', 47],
            ['2019-11-02T12:00-04:00[America/New_York]', '2019-11-04T12:00-05:00[America/New_York]', 49]
        ]) {
            var a = Temporal.ZonedDateTime.from(pair[0]), b = Temporal.ZonedDateTime.from(pair[1]);
            var options = {largestUnit:'days', smallestUnit:'hours', roundingMode:'halfExpand'};
            if (a.until(b, options).toString() !== b.since(a, options).toString()) throw 'direction';
            if (a.until(b, {largestUnit:'hours'}).hours !== pair[2]) throw 'hours';
            if (Temporal.Duration.from('P2D').total({unit:'hours', relativeTo:a}) !== pair[2]) throw 'total';
            passed++;
        }
        passed === 2
    "#,
    );
}

#[test]
fn maximum_rounding_increments_reject_out_of_range_calendar_brackets() {
    assert_script(
        r#"
        var a = Temporal.PlainDateTime.from('2020-01-01T00:00');
        var b = Temporal.PlainDateTime.from('2021-01-01T00:00');
        var za = a.toZonedDateTime('UTC'), zb = b.toZonedDateTime('UTC'), passed = 0;
        for (var unit of ['years', 'months', 'weeks']) {
            for (var pair of [[a,b], [b,a], [za,zb], [zb,za]]) {
                try {pair[0].until(pair[1], {largestUnit:unit, smallestUnit:unit, roundingIncrement:1000000000});}
                catch (e) {if (e instanceof RangeError) passed++;}
            }
        }
        passed === 12
    "#,
    );
}

#[test]
fn plain_duration_arithmetic_distinguishes_zero_and_out_of_range_endpoints() {
    assert_script(
        r#"
        var zero = Temporal.Duration.from('PT0S').round({smallestUnit:'days', relativeTo:'-271821-04-19'});
        var passed = 0;
        for (var operation of [
            () => Temporal.Duration.from('P1D').total({unit:'hours', relativeTo:'-271821-04-19'}),
            () => Temporal.Duration.from('P1D').round({smallestUnit:'days', relativeTo:'+275760-09-13'}),
            () => Temporal.Duration.from('P1Y').total({unit:'months', relativeTo:'+275760-09-13'})
        ]) {
            try {operation();} catch (e) {if (e instanceof RangeError) passed++;}
        }
        zero.blank && passed === 3
    "#,
    );
}

#[test]
fn temporal_conversion_errors_precede_observable_rounding_options() {
    assert_script(
        r#"
        var sentinel = {}, reads = 0, options = {get roundingMode() {reads++; throw sentinel;}};
        var date = Temporal.PlainDateTime.from('2020-01-01T00:00'), range = false;
        try {date.until('invalid', options);} catch (e) {range = e instanceof RangeError;}
        var same = false;
        try {date.until('2020-01-02T00:00', options);} catch (e) {same = e === sentinel;}
        range && same && reads === 1
    "#,
    );
}
