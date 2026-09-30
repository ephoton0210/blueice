// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for `Temporal.PlainYearMonth.prototype.with`
//! (`temporal_year_month_with`, `backend/bluejs/src/vm/temporal.rs`)
//! rejecting `era`/`eraYear` on a calendar with no era concept at all,
//! pinned directly from the real Test262 fixtures
//! `intl402/Temporal/PlainYearMonth/prototype/with/mutually-exclusive-fields-chinese.js`
//! and `.../mutually-exclusive-fields-dangi.js`.
//!
//! Before this pass, `calendar_supports_era` returning `false` for a
//! calendar (true for `iso8601`, `chinese` and `dangi` alike) made the
//! function take the same "silently use the extended year" path for all
//! three -- correct for `iso8601` (which must ignore `era`/`eraYear`
//! entirely, `PlainDate`'s own `with/time-units-ignored.js`), but wrong for
//! `chinese`/`dangi`, where Temporal's own behavior is to *reject* any use
//! of `era`/`eraYear` with a `TypeError` rather than silently drop it.

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm};

fn assert_throws_type_error(source: &str) {
    let program = compile(&parse(source).unwrap()).unwrap();
    match Vm::default().execute(&program) {
        Err(RuntimeError::TypeError(_)) => {}
        other => panic!("{source}\n  -> expected TypeError, got {other:?}"),
    }
}

fn assert_true(source: &str) {
    let program = compile(&parse(source).unwrap()).unwrap();
    assert_eq!(
        Vm::default().execute(&program).unwrap(),
        Value::Bool(true),
        "{source}"
    );
}

#[test]
fn chinese_era_and_era_year_together_throws_type_error() {
    assert_throws_type_error(
        r#"
        const options = { overflow: "reject" };
        const instance = Temporal.PlainYearMonth.from(
            { year: 1981, monthCode: "M12", calendar: "chinese" }, options
        );
        instance.with({ eraYear: 2025, era: "ce" });
    "#,
    );
}

#[test]
fn dangi_era_and_era_year_together_throws_type_error() {
    assert_throws_type_error(
        r#"
        const options = { overflow: "reject" };
        const instance = Temporal.PlainYearMonth.from(
            { year: 1981, monthCode: "M12", calendar: "dangi" }, options
        );
        instance.with({ eraYear: 2025, era: "ce" });
    "#,
    );
}

/// Unlike `chinese`/`dangi`, `iso8601` must keep silently ignoring
/// `era`/`eraYear` rather than throw -- there is no dedicated Test262
/// fixture for this on `PlainYearMonth` specifically, but the behavior must
/// stay symmetric with `PlainDate`'s own established `iso8601` case.
#[test]
fn iso8601_era_is_silently_ignored() {
    assert_true(
        r#"
        const instance = Temporal.PlainYearMonth.from({ year: 2020, month: 1 });
        const changed = instance.with({ month: 6, era: "BC" });
        changed.year === 2020 && changed.month === 6
    "#,
    );
}

/// Non-era fields on `chinese`/`dangi` are unaffected by the new check.
#[test]
fn chinese_month_change_without_era_still_works() {
    assert_true(
        r#"
        const options = { overflow: "reject" };
        const instance = Temporal.PlainYearMonth.from(
            { year: 1981, monthCode: "M12", calendar: "chinese" }, options
        );
        const changed = instance.with({ month: 5 }, options);
        changed.year === 1981 && changed.month === 5 && changed.monthCode === "M05"
    "#,
    );
}
