// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for a real bug pinned by the Test262 fixture
//! `built-ins/Temporal/PlainMonthDay/prototype/with/options-undefined.js`:
//! a `day` property-bag field was read with `temporal_integer(&day_v, 1, 31,
//! "day")`, an upper bound of `31` applied *before* the calendar's own
//! `overflow: "constrain"`/`"reject"` regulation ever runs. Per Gecko's own
//! `CalendarFields.cpp` (`PrepareCalendarFields`'s `CalendarField::Day`
//! case), the field-reading step uses `ToPositiveIntegerWithTruncation` --
//! only a lower bound of `1`, no upper bound at all -- so `{ day: 100 }`
//! must reach the calendar's own regulation step and be *constrained* to the
//! month's real day count (29, for February 1972 under the default
//! `overflow: "constrain"`), not rejected outright by the field reader
//! itself.
//!
//! Fixed in `temporal_month_day_with` (the only PlainMonthDay/PlainYearMonth
//! call site this pass changed; `temporal_plain_date_from_fields`/
//! `temporal_date_with`'s own identical-looking `day` bound is `PlainDate`/
//! `PlainDateTime`'s own code, deliberately left untouched here per this
//! phase's file-boundary discipline with the concurrent `PlainDate`/
//! `PlainDateTime` bug-fix session).

use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

/// The fixture's own three equivalent calls (explicit `undefined` options,
/// omitted options, and a callable-but-non-object options argument) all
/// default to `overflow: "constrain"`.
#[test]
fn day_overflow_constrains_by_default_instead_of_being_rejected_by_the_field_reader() {
    for call in [
        "monthday.with({ day: 100 }, undefined).day",
        "monthday.with({ day: 100 }).day",
        "monthday.with({ day: 100 }, () => {}).day",
    ] {
        let source = format!(
            r#"
            const monthday = new Temporal.PlainMonthDay(2, 2);
            {call}
        "#
        );
        assert_eq!(evaluate(&source), Value::Number(29.0), "{source}");
    }
}
