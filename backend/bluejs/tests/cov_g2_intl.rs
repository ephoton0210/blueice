// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `Intl.NumberFormat` receivers, inputs, parts and resolved options, plus
//! the locale-sensitive string casing and segmentation helpers of `intl.rs`.
mod cov_g2_support;

use blueice_bluejs::{RuntimeError, Value, Vm, VmConfig};
use cov_g2_support::{expect_true, run};

const PRELUDE: &str = "function thrown(f) { try { f(); return 'none'; } catch (e) { return e; } }
    function isType(f) { return thrown(f) instanceof TypeError; }
    function isRange(f) { return thrown(f) instanceof RangeError; }
    var boom = { valueOf() { throw 'boom'; }, toString() { throw 'boom'; } };
    function getter(object, name) { return Object.getOwnPropertyDescriptor(object, name).get; }
    function ro(options) { return new Intl.NumberFormat('en', options).resolvedOptions(); }";

fn check(body: &str) {
    expect_true(&format!("{PRELUDE}\n{body}"));
}

#[test]
fn resolved_options_report_every_option_value() {
    check(
        "[ro({ style: 'percent' }).style === 'percent',
          ro({ style: 'currency', currency: 'USD' }).currencySign === 'standard',
          ro({ style: 'currency', currency: 'USD', currencySign: 'accounting' }).currencySign === 'accounting',
          ro({ style: 'currency', currency: 'USD', currencyDisplay: 'code' }).currencyDisplay === 'code',
          ro({ style: 'currency', currency: 'USD', currencyDisplay: 'name' }).currencyDisplay === 'name',
          ro({ style: 'currency', currency: 'USD', currencyDisplay: 'narrowSymbol' }).currencyDisplay === 'narrowSymbol',
          ro({ style: 'currency', currency: 'USD' }).currencyDisplay === 'symbol',
          ro({ style: 'unit', unit: 'meter', unitDisplay: 'narrow' }).unitDisplay === 'narrow',
          ro({ style: 'unit', unit: 'meter', unitDisplay: 'long' }).unitDisplay === 'long',
          ro({ style: 'unit', unit: 'meter' }).unitDisplay === 'short',
          ro({ notation: 'scientific' }).notation === 'scientific',
          ro({ notation: 'engineering' }).notation === 'engineering',
          ro({ notation: 'compact' }).compactDisplay === 'short',
          ro({ notation: 'compact', compactDisplay: 'long' }).compactDisplay === 'long',
          ro({ signDisplay: 'never' }).signDisplay === 'never',
          ro({ signDisplay: 'always' }).signDisplay === 'always',
          ro({ signDisplay: 'exceptZero' }).signDisplay === 'exceptZero',
          ro({ signDisplay: 'negative' }).signDisplay === 'negative',
          ['ceil', 'floor', 'expand', 'trunc', 'halfCeil', 'halfFloor', 'halfExpand', 'halfTrunc', 'halfEven']
            .every(m => ro({ roundingMode: m }).roundingMode === m),
          ro({ minimumSignificantDigits: 2, maximumSignificantDigits: 4 }).maximumSignificantDigits === 4,
          ro({ minimumSignificantDigits: 2, roundingPriority: 'morePrecision' }).roundingPriority === 'morePrecision',
          ro({ minimumSignificantDigits: 2, roundingPriority: 'lessPrecision' }).roundingPriority === 'lessPrecision',
          ro({ useGrouping: 'always' }).useGrouping === 'always',
          ro({ useGrouping: 'min2' }).useGrouping === 'min2',
          ro({ useGrouping: false }).useGrouping === false,
          ro({ trailingZeroDisplay: 'stripIfInteger' }).trailingZeroDisplay === 'stripIfInteger',
          ro({ roundingIncrement: 5, maximumFractionDigits: 2, minimumFractionDigits: 2 }).roundingIncrement === 5,
         ].every(Boolean)",
    );
}

#[test]
fn receivers_are_unwrapped_through_the_legacy_fallback_symbol() {
    check(
        "var proto = Intl.NumberFormat.prototype;
         var formatGetter = getter(proto, 'format');
         var legacy = Intl.NumberFormat.call(Object.create(proto));
         var symbol = Object.getOwnPropertySymbols(legacy)[0];
         var impostor = Object.create(proto);
         Object.defineProperty(impostor, symbol, { value: {} });
         var primitiveFallback = Object.create(proto);
         Object.defineProperty(primitiveFallback, symbol, { value: 1 });
         var trapped = new Proxy(legacy, { get(t, k, r) { if (typeof k === 'symbol') throw 'sym'; return t[k]; } });
         isType(() => formatGetter.call(1)) && isType(() => formatGetter.call({})) &&
           isType(() => formatGetter.call(impostor)) && isType(() => formatGetter.call(primitiveFallback)) &&
           thrown(() => formatGetter.call(trapped)) === 'sym' &&
           formatGetter.call(legacy)(1234.5) === '1,234.5' && legacy.resolvedOptions().style === 'decimal' &&
           isType(() => proto.resolvedOptions.call({})) && isType(() => proto.formatToParts.call({}, 1)) &&
           isType(() => proto.formatToParts.call(1, 1)) && isType(() => proto.formatRange.call({}, 1, 2)) &&
           isType(() => proto.formatRangeToParts.call(1, 1, 2)) &&
           new Intl.NumberFormat().format === new Intl.NumberFormat().format === false",
    );
}

#[test]
fn inputs_are_converted_like_intl_mathematical_values() {
    check(
        "var nf = new Intl.NumberFormat('en');
         nf.format('1e5') === '100,000' && nf.format('1.5E2') === '150' && nf.format(12n) === '12' &&
           nf.format('\\ud800') === 'NaN' && nf.format('abc') === 'NaN' && nf.format(true) === '1' &&
           nf.format(null) === '0' && nf.format(undefined) === 'NaN' &&
           thrown(() => nf.format(boom)) === 'boom' && isType(() => nf.format(Symbol())) &&
           nf.formatToParts(NaN)[0].type === 'nan' && nf.formatToParts(Infinity)[0].type === 'infinity' &&
           nf.formatToParts(-1.5).map(p => p.type).join() === 'minusSign,integer,decimal,fraction'",
    );
}

#[test]
fn ranges_report_missing_and_nan_endpoints() {
    check(
        "var nf = new Intl.NumberFormat('en');
         isType(() => nf.formatRange(undefined, 1)) && isType(() => nf.formatRange(1, undefined)) &&
           isType(() => nf.formatRangeToParts(1)) &&
           isRange(() => nf.formatRange(NaN, 1)) && isRange(() => nf.formatRangeToParts(1, NaN)) &&
           thrown(() => nf.formatRange(boom, 1)) === 'boom' && thrown(() => nf.formatRange(1, boom)) === 'boom' &&
           nf.formatRange(1, 3) === '1\u{2013}3' && nf.formatRange(2, 2) === '~2' &&
           nf.formatRangeToParts(2, 2).some(p => p.type === 'approximatelySign') &&
           nf.formatRangeToParts(1, 3).map(p => p.source).includes('endRange')",
    );
}

#[test]
fn bigint_locale_strings_use_the_number_formatter() {
    check(
        "1234n.toLocaleString('en') === '1,234' && isRange(() => 1n.toLocaleString('bad_locale!')) &&
           (12345n).toLocaleString('en', { style: 'currency', currency: 'USD' }).startsWith('$')",
    );
}

#[test]
fn locale_casing_and_segmentation_cover_lone_surrogates() {
    check(
        "'a\\ud800b'.toLocaleUpperCase() === 'A\\ud800B' && '\\ud800'.toLocaleLowerCase('tr') === '\\ud800' &&
           'I'.toLocaleLowerCase('tr') === '\\u0131' &&
           thrown(() => 'a'.toLocaleUpperCase('bad_locale!')) instanceof RangeError &&
           (function () { var out = []; for (var s of new Intl.Segmenter('en', { granularity: 'word' }).segment('ab cd')) out.push(s.segment); return out.join('|') === 'ab| |cd'; })() &&
           new Intl.Segmenter().segment('abc').containing(1).segment === 'b' &&
           new Intl.Segmenter().segment('abc').containing(9) === undefined &&
           new Intl.Segmenter().segment('a\\ud800').containing(1).segment === '\\ud800'",
    );
}

#[test]
fn locale_casing_reports_oversized_results() {
    let mut vm = Vm::new(VmConfig {
        max_string_bytes: 512,
        ..VmConfig::default()
    })
    .unwrap();
    for (source, limit) in [
        // The mapped run alone overflows.
        ("'ß'.repeat(200).toLocaleUpperCase()", 512),
        // The run fits, but the lone surrogate after it does not.
        ("('a'.repeat(256) + '\\ud800').toLocaleUpperCase()", 512),
        // A surrogate run that overflows on its own.
        ("('\\ud800'.repeat(257)).toLocaleUpperCase()", 512),
    ] {
        assert_eq!(
            run(&mut vm, source),
            Err(RuntimeError::StringLimit { limit }),
            "{source}"
        );
    }
    assert_eq!(
        run(&mut vm, "'ab'.toLocaleUpperCase()"),
        Ok(Value::String("AB".into()))
    );
}
