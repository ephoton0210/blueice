// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Every allocation made by `Intl.NumberFormat` and the Temporal helpers of
//! this group failing in turn.
mod cov_g2_sweep;

use blueice_bluejs::Value;
use cov_g2_sweep::{sweep_cold, sweep_ops, sweep_string_limit, Mode};

const WARMUP: &str = "globalThis.nf = new Intl.NumberFormat('en'); nf.format(1); nf.formatToParts(1); nf.formatRange(1, 2);
    nf.formatRangeToParts(1, 2); nf.resolvedOptions(); 1n.toLocaleString('en');
    var d = new Temporal.Duration(0, 0, 0, 1); d.toLocaleString('en'); d.toString(); d.with({ days: 2 }); d.negated();
    d.abs(); d.add(d); d.round({ largestUnit: 'hour' }); d.total('hour');
    Temporal.ZonedDateTime.from('2020-01-01T00:00+00:00[UTC]').toString(); 0";

#[test]
fn allocation_failures_in_number_formatting() {
    sweep_ops(
        Mode::PLAIN,
        WARMUP,
        "b(); var nf2 = new Intl.NumberFormat('en', { style: 'currency', currency: 'USD' });
         b(); var f = nf2.format; b(); f(1);
         b(); var g = new Intl.NumberFormat('en').format; b(); g(12345.678);
         b(); var p = nf.formatToParts(1234.5); b(); var r = nf.formatRange(1, 3); b(); var rp = nf.formatRangeToParts(1, 3);
         b(); var o = nf.resolvedOptions(); b(); var big = (12345n).toLocaleString('en');
         b(); var legacy = Intl.NumberFormat.call(Object.create(Intl.NumberFormat.prototype));
         return o.style === 'decimal' && p.length > 0;",
        16,
    );
}

#[test]
fn allocation_failures_in_duration_helpers() {
    sweep_ops(
        Mode::PLAIN,
        WARMUP,
        "b(); var d2 = new Temporal.Duration(0, 0, 0, 1, 2, 3);
         b(); var l = d2.toLocaleString('en'); b(); var s = d2.toString({ fractionalSecondDigits: 2 });
         b(); var w = d2.with({ hours: 5 }); b(); var n = d2.negated(); b(); var a = d2.abs();
         b(); var ad = d2.add(d2); b(); var su = d2.subtract(d2); b(); var ro = d2.round({ largestUnit: 'hour' });
         b(); var t = d2.total({ unit: 'minute' }); b(); var c = Temporal.Duration.compare(d2, d2);
         b(); var z = Temporal.ZonedDateTime.from('2020-01-01T00:00+00:00[UTC]').toString({ smallestUnit: 'minute' });
         return l.length > 0 && t > 0;",
        16,
    );
}

#[test]
fn cold_sweep_of_number_format_materialization() {
    for script in [
        "new Intl.NumberFormat('en').format(1) === '1'",
        "typeof new Intl.NumberFormat('en').format === 'function'",
        "new Intl.NumberFormat('en').resolvedOptions().style === 'decimal'",
        "new Intl.NumberFormat('en').formatToParts(1).length === 1",
        "1n.toLocaleString('en') === '1'",
        "new Temporal.Duration(0, 0, 0, 1).toLocaleString('en').length > 0",
    ] {
        sweep_cold(Mode::PLAIN, script, 8);
    }
}

#[test]
fn case_mapping_reports_a_result_that_outgrows_the_string_limit_at_every_step() {
    for script in [
        "'ab\\ud800cd'.toUpperCase()",
        "'AB\\ud800CD'.toLowerCase()",
        "'ab\\ud800cd'.toLocaleUpperCase('tr')",
        "'\\ud800'.toLowerCase()",
    ] {
        sweep_string_limit(Mode::PLAIN, script, 1, 1, |result| {
            matches!(result, Ok(Value::String(_)))
        });
    }
}
