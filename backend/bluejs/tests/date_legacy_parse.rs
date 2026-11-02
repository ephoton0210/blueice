// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `Date.parse` beyond ISO. ECMA-262 leaves other formats implementation-defined, but pages
//! depend on the ones every engine reads: `M/D/YYYY, h:mm:ss PM` (what `toLocaleString`
//! prints and what dayjs/moment parse back), `Nov 2, 2026`, `2026/11/02`, and the `toString`
//! output with its zone offset. BlueJS' local time zone is UTC.

use blueice_bluejs::{compile, parse, Value, Vm, VmConfig};

fn eval(source: &str) -> Value {
    let mut vm = Vm::new(VmConfig::default()).unwrap();
    vm.execute(&compile(&parse(source).expect(source)).expect(source))
        .unwrap_or_else(|e| panic!("{source}: {e:?}"))
}

/// `Date.parse(text)` against `Date.UTC(...)`: true when they agree (and are a number).
fn reads_as(text: &str, utc: &str) -> bool {
    matches!(
        eval(&format!(
            "(function () {{ const a = Date.parse({text:?}); const b = Date.UTC({utc}); return a === b && a === a; }})()"
        )),
        Value::Bool(true)
    )
}

fn is_nan(text: &str) -> bool {
    matches!(eval(&format!("Number.isNaN(Date.parse({text:?}))")), Value::Bool(true))
}

#[test]
fn the_locale_string_form_with_a_twelve_hour_clock() {
    assert!(reads_as("11/2/2026, 5:06:22 PM", "2026, 10, 2, 17, 6, 22"));
    assert!(reads_as("11/2/2026, 5:06:22 pm", "2026, 10, 2, 17, 6, 22"));
    assert!(reads_as("11/2/2026 5:06:22 AM", "2026, 10, 2, 5, 6, 22"));
    assert!(reads_as("12/31/2026, 11:59:59 PM", "2026, 11, 31, 23, 59, 59"));
    assert!(reads_as("12/31/2026, 12:00:00 AM", "2026, 11, 31, 0, 0, 0"));
    assert!(reads_as("1/1/2026, 12:30:00 PM", "2026, 0, 1, 12, 30, 0"));
    assert!(reads_as("1/1/2026, 5:06 PM", "2026, 0, 1, 17, 6, 0"));
}

#[test]
fn month_day_year_dates_without_a_time() {
    assert!(reads_as("11/2/2026", "2026, 10, 2"));
    assert!(reads_as("01/02/2026", "2026, 0, 2"));
    assert!(reads_as("1/2/99", "1999, 0, 2"));
    assert!(reads_as("1/2/25", "2025, 0, 2"));
}

#[test]
fn year_month_day_with_slashes() {
    assert!(reads_as("2026/11/02 17:06:22", "2026, 10, 2, 17, 6, 22"));
    assert!(reads_as("2026/11/2", "2026, 10, 2"));
}

#[test]
fn month_names() {
    assert!(reads_as("Nov 2, 2026", "2026, 10, 2"));
    assert!(reads_as("November 2, 2026", "2026, 10, 2"));
    assert!(reads_as("Nov 2 2026", "2026, 10, 2"));
    assert!(reads_as("2 Nov 2026", "2026, 10, 2"));
    assert!(reads_as("November 2, 2026 17:06:22", "2026, 10, 2, 17, 6, 22"));
    assert!(reads_as("2 Nov 2026 17:06:22 GMT", "2026, 10, 2, 17, 6, 22"));
    assert!(reads_as("Mon, Nov 2, 2026", "2026, 10, 2"));
}

#[test]
fn zone_offsets_and_names() {
    assert!(reads_as("Mon Nov 02 2026 17:06:22 GMT+0800 (CST)", "2026, 10, 2, 9, 6, 22"));
    assert!(reads_as("Mon, 02 Nov 2026 17:06:22 +0800", "2026, 10, 2, 9, 6, 22"));
    assert!(reads_as("Nov 2, 2026 17:06:22 GMT-0500", "2026, 10, 2, 22, 6, 22"));
    assert!(reads_as("2 Nov 2026 17:06:22 EST", "2026, 10, 2, 22, 6, 22"));
    assert!(reads_as("2 Nov 2026 17:06:22 PDT", "2026, 10, 3, 0, 6, 22"));
    assert!(reads_as("2 Nov 2026 17:06:22 UTC", "2026, 10, 2, 17, 6, 22"));
    assert!(reads_as("11/2/2026, 5:06:22 PM GMT+2", "2026, 10, 2, 15, 6, 22"));
}

#[test]
fn what_tostring_and_the_locale_string_print_reads_back() {
    let all = eval(
        "(function () { for (const t of [0, 1, 86399000, 951782400000, 1793639182000, 4102444799000, -86400000, 253402300799000]) { const d = new Date(t); const s = Math.floor(t / 1000) * 1000; if (Date.parse(d.toString()) !== s) return 'toString ' + t; if (Date.parse(d.toUTCString()) !== s) return 'utc ' + t; if (Date.parse(d.toLocaleString('en-US')) !== s) return 'locale ' + t + ' ' + d.toLocaleString('en-US'); } return 'ok'; })()",
    );
    let Value::String(text) = all else { panic!("{all:?}") };
    assert_eq!(text.to_utf8().unwrap(), "ok");
}

#[test]
fn nonsense_is_still_not_a_number() {
    for text in [
        "", "garbage", "13/45/2026", "0/1/2026", "Nov 32, 2026", "11/2/2026 25:00:00",
        "11/2/2026 12:61:00", "Smarch 2, 2026", "11/2/2026 5:06 XM", "1/2/3/4", "2026-13-01",
        "11/", "Nov", "5:06:22 PM",
    ] {
        assert!(is_nan(text), "{text:?} must not parse");
    }
}

#[test]
fn iso_strings_keep_their_strict_reading() {
    assert!(reads_as("2026-11-02T17:06:22Z", "2026, 10, 2, 17, 6, 22"));
    assert!(reads_as("2026-11-02", "2026, 10, 2"));
    // A malformed ISO string is not rescued by the legacy reading.
    for text in ["2026-11-32T00:00:00Z", "2026-11-02T25:00:00Z", "+275760-09-13T00:00:00.001Z", "2026-11-02T17:06:22+99:99"] {
        assert!(is_nan(text), "{text:?} must stay invalid");
    }
}
