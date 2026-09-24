// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `Number.prototype` formatting (radix strings, fixed/exponential/precision,
//! locale strings), `DataView` accessors of every width and byte order, the
//! Proxy descriptor invariants, dynamic-function comment stripping and every
//! `Intl.NumberFormat` option, with faults injected into each option read.

mod cov_g8_common;

use cov_g8_common::{budget_sweep, check_cases, fault_sweep, heap_sweep};

include!("cov_g8_tables/numbers.in");
include!("cov_g8_tables/number_format.in");

#[test]
fn number_formatting_reports_the_reference_results() {
    check_cases(NUMBER_CASES, false);
}

#[test]
fn number_format_options_report_the_reference_results() {
    check_cases(NUMBER_FORMAT_CASES, false);
}

/// Every NumberFormat option, each read and converted through a step that can
/// throw, for each style that reads a different set of them.
const OPTION_SETS: &[&str] = &[
    "P({ style: V('currency'), currency: V('USD'), currencyDisplay: V('code'), currencySign: V('accounting'), useGrouping: V('always'), notation: V('compact'), compactDisplay: V('long'), signDisplay: V('always'), minimumIntegerDigits: V(2), minimumFractionDigits: V(1), maximumFractionDigits: V(3), roundingIncrement: V(1), roundingMode: V('ceil'), roundingPriority: V('auto'), trailingZeroDisplay: V('auto'), numberingSystem: V('latn'), localeMatcher: V('lookup') })",
    "P({ style: V('unit'), unit: V('kilometer'), unitDisplay: V('long'), useGrouping: V(false), notation: V('scientific'), signDisplay: V('exceptZero'), minimumSignificantDigits: V(2), maximumSignificantDigits: V(4), roundingMode: V('halfEven'), roundingPriority: V('lessPrecision'), trailingZeroDisplay: V('stripIfInteger') })",
    "P({ style: V('percent'), notation: V('engineering'), roundingIncrement: V(5), minimumFractionDigits: V(2), maximumFractionDigits: V(2), roundingMode: V('trunc') })",
];

#[test]
fn every_number_format_option_read_can_fail() {
    let mut points = 0;
    for options in OPTION_SETS {
        points += fault_sweep("", &format!("new Intl.NumberFormat(V('en'), {options})"));
        points += fault_sweep(
            "",
            &format!("new Intl.NumberFormat(V('en'), {options}).format(1234.5)"),
        );
        points += fault_sweep(
            "",
            &format!("(1234.5).toLocaleString(P(['en']), {options})"),
        );
        points += fault_sweep(
            "",
            &format!("Intl.NumberFormat.supportedLocalesOf(P(['en', 'de']), {options})"),
        );
    }
    assert!(points > 200, "{points}");
}

/// Number methods with their argument conversions as fault-injecting steps.
const NUMBER_CALLS: &[&str] = &[
    "(255.5).toString(V(16))",
    "(-0.25).toString(V(2))",
    "(255.5).toFixed(V(2))",
    "(255.5).toExponential(V(2))",
    "(255.5).toPrecision(V(4))",
    "(255.5).toLocaleString(P(['en']), { minimumFractionDigits: V(2) })",
    "Number.prototype.toString.call(new Number(5), V(2))",
    "Number.prototype.toString.call(P(new Number(5)), V(2))",
];

#[test]
fn every_number_method_conversion_can_fail() {
    let mut points = 0;
    for call in NUMBER_CALLS {
        points += fault_sweep("", call);
    }
    assert!(points > 8, "{points}");
}

#[test]
fn number_formatting_runs_out_of_instructions_and_heap() {
    for source in [
        "(255.5).toString(16)",
        "(0.1).toString(3)",
        "(1234.5).toLocaleString('en', { style: 'currency', currency: 'USD' })",
        "new Intl.NumberFormat('en', { notation: 'compact', roundingIncrement: 5, minimumFractionDigits: 2, maximumFractionDigits: 2 }).format(1234567)",
        "new DataView(new ArrayBuffer(8)).setFloat64(0, 1.5)",
    ] {
        assert!(budget_sweep(source) > 0, "{source}");
        heap_sweep(source, 4000, 8);
    }
}

#[test]
fn the_intl_namespace_is_built_from_an_almost_full_heap() {
    let stopped = heap_sweep("new Intl.NumberFormat('en')", 400_000, 16)
        + heap_sweep("Intl.NumberFormat.call({}, 'en')", 400_000, 16)
        + heap_sweep("(1).toLocaleString('en')", 400_000, 16)
        + heap_sweep("(1n).toLocaleString('en')", 400_000, 16)
        + heap_sweep("[1, 2].toLocaleString('en')", 400_000, 16);
    assert!(stopped > 100, "{stopped}");
}
