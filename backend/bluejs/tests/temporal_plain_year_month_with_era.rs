// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for `Temporal.PlainYearMonth.prototype.with`'s
//! `era`/`eraYear` support, pinned directly from the real Test262 fixture
//! `intl402/Temporal/PlainYearMonth/prototype/with/mutually-exclusive-fields-gregory.js`.
//!
//! Before this pass, `temporal_year_month_with`
//! (`backend/bluejs/src/vm/temporal.rs`) never read `era`/`eraYear` off the
//! `like` argument at all -- only `year`/`month`/`monthCode` -- so every
//! `with({ era, eraYear })` call on an era-supporting calendar fell straight
//! through to "Temporal.with requires at least one recognized property"
//! (wrongly, since `era`/`eraYear` *are* recognized calendar-field names per
//! `CalendarFields.cpp`'s `PrepareCalendarFields`), and `with({ eraYear })`/
//! `with({ era })` alone (missing the paired field) had no dedicated
//! validation to produce the spec's `TypeError` either.
//!
//! Ported from `CalendarFields.cpp`'s `NonISOFieldKeysToIgnore`/
//! `NonISOResolveFields` (`development/browser_core/reference/gecko/js/src/builtin/temporal/CalendarFields.cpp`):
//! for a calendar that supports eras, `era`/`eraYear`/`year` are mutually
//! exclusive as a *group* in `with()` -- providing any one of them drops the
//! receiver's own value for all three -- and `era`/`eraYear` must be
//! supplied together or not at all.

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

fn assert_true(source: &str) {
    assert_eq!(evaluate(source), Value::Bool(true), "{source}");
}

fn assert_throws_type_error(source: &str) {
    let program = compile(&parse(source).unwrap()).unwrap();
    match Vm::default().execute(&program) {
        Err(RuntimeError::TypeError(_)) => {}
        other => panic!("{source}\n  -> expected TypeError, got {other:?}"),
    }
}

/// `era` and `eraYear` together resolve the year, excluding the receiver's
/// own `year` -- the fixture's first assertion
/// (`instance.with({ era: "bce", eraYear: 1 }, options)` on a 1981 Gregorian
/// `PlainYearMonth` must produce year `0` = "bce" year 1, keeping month 12).
#[test]
fn era_and_era_year_together_resolve_the_year_and_exclude_the_receivers_year() {
    assert_true(
        r#"
        const instance = Temporal.PlainYearMonth.from(
            { year: 1981, monthCode: "M12", calendar: "gregory" },
            { overflow: "reject" }
        );
        const changed = instance.with({ era: "bce", eraYear: 1 }, { overflow: "reject" });
        changed.year === 0 && changed.month === 12 && changed.monthCode === "M12"
            && changed.era === "bce" && changed.eraYear === 1
    "#,
    );
}

/// A bare `year` override excludes the receiver's own `era`/`eraYear` --
/// they are recomputed fresh from the new extended year instead.
#[test]
fn year_alone_excludes_era_and_era_year_and_is_recomputed() {
    assert_true(
        r#"
        const instance = Temporal.PlainYearMonth.from(
            { year: 1981, monthCode: "M12", calendar: "gregory" },
            { overflow: "reject" }
        );
        const changed = instance.with({ year: -2 }, { overflow: "reject" });
        changed.year === -2 && changed.month === 12 && changed.monthCode === "M12"
            && changed.era === "bce" && changed.eraYear === 3
    "#,
    );
}

/// `eraYear` without `era` is a `TypeError` -- the pair must be supplied
/// together on an era-supporting calendar.
#[test]
fn era_year_alone_throws_type_error() {
    assert_throws_type_error(
        r#"
        const instance = Temporal.PlainYearMonth.from(
            { year: 1981, monthCode: "M12", calendar: "gregory" },
            { overflow: "reject" }
        );
        instance.with({ eraYear: 1 });
    "#,
    );
}

/// `era` without `eraYear` is likewise a `TypeError`.
#[test]
fn era_alone_throws_type_error() {
    assert_throws_type_error(
        r#"
        const instance = Temporal.PlainYearMonth.from(
            { year: 1981, monthCode: "M12", calendar: "gregory" },
            { overflow: "reject" }
        );
        instance.with({ era: "bce" });
    "#,
    );
}
