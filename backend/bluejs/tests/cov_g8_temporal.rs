// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `Temporal.PlainYearMonth`/`PlainMonthDay` (and the option readers and date
//! arithmetic they share): every method against receivers that are not of the
//! type, against error-producing property bags, and with a fault injected into
//! each observable step of every method.

mod cov_g8_common;

use cov_g8_common::{budget_sweep, check_cases, fault_sweep, heap_sweep, observe};

include!("cov_g8_tables/temporal.in");

#[test]
fn year_months_and_month_days_report_the_reference_results() {
    check_cases(TEMPORAL_CASES, false);
}

/// Every own property of these prototypes, as a getter or a method, called on
/// values that are not of the type: objects that are not Temporal objects at
/// all and Temporal objects of another type are all rejected with a TypeError.
const BRAND_CHECKED: &[&str] = &["PlainYearMonth", "PlainMonthDay"];

#[test]
fn every_member_rejects_a_receiver_of_another_type() {
    for type_name in BRAND_CHECKED {
        let script = format!(
            "(function () {{
               var proto = Temporal.{type_name}.prototype, wrong = [undefined, null, 5, 'x', {{}}, [], function () {{}}, Symbol(),
                 Temporal.PlainDate.from('2020-03-04'), Temporal.Duration.from('P1D'), Temporal.PlainTime.from('12:00'),
                 Temporal.PlainYearMonth.from('2020-03'), Temporal.PlainMonthDay.from('03-04')], report = [];
               Object.getOwnPropertyNames(proto).forEach(function (name) {{
                 if (name === 'constructor') return;
                 var d = Object.getOwnPropertyDescriptor(proto, name), f = d.get || d.value;
                 if (typeof f !== 'function') return;
                 wrong.forEach(function (receiver) {{
                   if (receiver instanceof Temporal.{type_name}) return;
                   try {{ f.call(receiver, 1, 1); report.push(name + ' accepted ' + String(typeof receiver)); }}
                   catch (e) {{ if (!(e instanceof TypeError)) report.push(name + ' threw ' + e.name); }}
                 }});
               }});
               return report.join();
             }})()"
        );
        assert_eq!(observe(&script, false), "", "{type_name}");
    }
}

/// The values the fault sweeps work on.
const SETUP: &str = "
globalThis.ym = Temporal.PlainYearMonth.from('2020-03');
globalThis.ymg = Temporal.PlainYearMonth.from({ year: 2020, month: 3, calendar: 'gregory' });
globalThis.ymh = Temporal.PlainYearMonth.from({ year: 5784, month: 3, calendar: 'hebrew' });
globalThis.md = Temporal.PlainMonthDay.from('03-04');
globalThis.mdg = Temporal.PlainMonthDay.from({ monthCode: 'M03', day: 4, calendar: 'gregory' });
globalThis.bag = function (calendar) {
  return P({ year: V(2021), month: V(5), monthCode: V('M05'), era: V('ce'), eraYear: V(2021), day: V(6), calendar: calendar });
};
globalThis.opts = function () {
  return P({ overflow: V('constrain'), calendarName: V('auto'), largestUnit: V('years'), smallestUnit: V('months'), roundingMode: V('trunc'), roundingIncrement: V(1) });
};
";

const FAULT_OPS: &[&str] = &[
    "Temporal.PlainYearMonth.from(bag('iso8601'), opts())",
    "Temporal.PlainYearMonth.from(bag('gregory'), opts())",
    "Temporal.PlainYearMonth.from(P(ym), opts())",
    "Temporal.PlainYearMonth.from('2020-03', opts())",
    "Temporal.PlainYearMonth.from(ymg)",
    "ym.with(P({ year: V(2021), month: V(5), monthCode: V('M05') }), opts())",
    "ymg.with(P({ era: V('ce'), eraYear: V(2021), month: V(5) }), opts())",
    "ymh.with(P({ monthCode: V('M05') }), opts())",
    "ym.add(P({ years: V(1), months: V(2) }), opts())",
    "ym.subtract(P({ years: V(1), months: V(2) }), opts())",
    "ymg.add(P({ months: V(2) }))",
    "ymh.subtract(P({ years: V(1) }), opts())",
    "ym.until(bag('iso8601'), opts())",
    "ym.since(bag('iso8601'), opts())",
    "ymg.until(bag('gregory'), opts())",
    "ymh.since(ymh, opts())",
    "ym.equals(bag('iso8601'))",
    "ymg.equals(bag('gregory'))",
    "Temporal.PlainYearMonth.compare(bag('iso8601'), P(ym))",
    "ym.toString(opts())",
    "ymg.toString(P({ calendarName: V('always') }))",
    "ymg.toLocaleString(P(['en']), P({ month: V('long'), year: V('numeric') }))",
    "ym.toPlainDate(P({ day: V(5) }))",
    "ymg.toPlainDate(P({ day: V(5) }))",
    "Temporal.PlainMonthDay.from(bag('iso8601'), opts())",
    "Temporal.PlainMonthDay.from(bag('gregory'), opts())",
    "Temporal.PlainMonthDay.from(P(md), opts())",
    "Temporal.PlainMonthDay.from('03-04', opts())",
    "md.with(P({ day: V(6), monthCode: V('M05') }), opts())",
    "mdg.with(P({ day: V(6), month: V(5), year: V(2021) }), opts())",
    "md.equals(bag('iso8601'))",
    "mdg.equals(bag('gregory'))",
    "md.toString(opts())",
    "mdg.toString(P({ calendarName: V('always') }))",
    "mdg.toLocaleString(P(['en']), P({ month: V('short'), day: V('numeric') }))",
    "md.toPlainDate(P({ year: V(2021) }))",
    "mdg.toPlainDate(P({ era: V('ce'), eraYear: V(2021) }))",
    "mdg.toPlainDate(P({ year: V(2021) }))",
];

#[test]
fn every_observable_step_of_every_method_can_fail() {
    let mut points = 0;
    for op in FAULT_OPS {
        points += fault_sweep(SETUP, op);
    }
    assert!(points > 300, "{points}");
}

#[test]
fn methods_run_out_of_instructions_and_heap_at_every_step() {
    for op in [
        "Temporal.PlainYearMonth.from({ year: 2021, month: 5 }).until('2022-01').toString()",
        "Temporal.PlainMonthDay.from({ monthCode: 'M03', day: 4 }).toPlainDate({ year: 2021 }).toString()",
        "Temporal.PlainYearMonth.from('2020-03').with({ month: 5 }).add({ months: 1 }).toString()",
    ] {
        assert!(budget_sweep(op) > 0, "{op}");
        heap_sweep(op, 6000, 8);
    }
}

/// Values whose options are read through the shared Temporal option readers.
const OPTION_SETUP: &str = "
globalThis.inst = Temporal.Instant.from('2020-01-01T00:00:00Z');
globalThis.pt = Temporal.PlainTime.from('12:00');
globalThis.pdt = Temporal.PlainDateTime.from('2020-03-04T05:06');
globalThis.zdt = Temporal.ZonedDateTime.from('2020-03-04T05:06[UTC]');
globalThis.pd = Temporal.PlainDate.from('2020-03-04');
globalThis.dur = Temporal.Duration.from('PT40H');
globalThis.topts = function () {
  return P({ smallestUnit: V('second'), largestUnit: V('hour'), roundingMode: V('trunc'), roundingIncrement: V(1), fractionalSecondDigits: V(2), overflow: V('constrain'), calendarName: V('auto'), timeZoneName: V('auto'), offset: V('prefer'), disambiguation: V('compatible') });
};
globalThis.dopts = function () {
  return P({ largestUnit: V('auto'), smallestUnit: V('days'), roundingMode: V('ceil'), roundingIncrement: V(1), overflow: V('reject') });
};
";

const OPTION_OPS: &[&str] = &[
    "inst.round(topts())",
    "inst.round('second')",
    "inst.toString(topts())",
    "inst.until(inst, topts())",
    "inst.since(inst, topts())",
    "pt.round(topts())",
    "pt.toString(topts())",
    "pt.until(pt, topts())",
    "pt.since(pt, topts())",
    "pt.with(P({ hour: V(3) }), topts())",
    "pdt.round(topts())",
    "pdt.toString(topts())",
    "pdt.until(pdt, topts())",
    "pdt.add(P({ days: V(1) }), topts())",
    "zdt.round(topts())",
    "zdt.toString(topts())",
    "zdt.until(zdt, topts())",
    "zdt.with(P({ hour: V(3) }), topts())",
    "dur.round(topts())",
    "dur.total(P({ unit: V('hour') }))",
    "dur.total('second')",
    "dur.toString(topts())",
    "pd.until(pd, dopts())",
    "pd.since(pd, dopts())",
    "pd.until(P(pd), dopts())",
    "pd.add(P({ months: V(1) }), dopts())",
    "pd.toString(P({ calendarName: V('always') }))",
];

#[test]
fn option_reading_can_fail_at_every_step() {
    let mut points = 0;
    for op in OPTION_OPS {
        points += fault_sweep(OPTION_SETUP, op);
    }
    assert!(points > 150, "{points}");
}

#[test]
fn option_strings_must_be_well_formed_text() {
    let lone = format!("'{}ud800'", "\\");
    for expression in [
        "inst.round(LONE)",
        "inst.round({ smallestUnit: 'second', roundingMode: LONE })",
        "inst.round({ smallestUnit: LONE })",
        "inst.toString({ smallestUnit: LONE })",
        "pt.round({ smallestUnit: 'second', roundingIncrement: 1, roundingMode: LONE })",
        "pd.until(pd, { largestUnit: LONE })",
        "dur.round({ largestUnit: LONE })",
        "dur.total({ unit: LONE })",
    ] {
        let source = expression.replace("LONE", &lone);
        let outcome = observe(
            &format!("(function () {{ {OPTION_PRELUDE} return {source} }})()"),
            false,
        );
        assert!(
            outcome.starts_with("throws RangeError: invalid"),
            "{source}: {outcome}"
        );
    }
}

const OPTION_PRELUDE: &str = "var inst = Temporal.Instant.from('2020-01-01T00:00:00Z'), pt = Temporal.PlainTime.from('12:00'), pd = Temporal.PlainDate.from('2020-03-04'), dur = Temporal.Duration.from('PT40H');";

#[test]
fn option_readers_run_out_of_heap_at_every_allocation() {
    for op in [
        "Temporal.Instant.from('2020-01-01T00:00:00Z').round('second').toString()",
        "Temporal.Instant.from('2020-01-01T00:00:00Z').toString()",
        "Temporal.PlainTime.from('12:00').round('minute').toString()",
    ] {
        assert!(heap_sweep(op, 6000, 8) > 0, "{op}");
    }
}
