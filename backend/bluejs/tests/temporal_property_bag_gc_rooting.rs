// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A native function that holds an object-valued `Get` result in a Rust
//! local across a later call that can run JavaScript (and therefore allocate)
//! must keep that object rooted, or a collection triggered by the later
//! allocation frees it and the eventual `ToPrimitive`/`ToString` on it fails
//! with "unknown or collected BlueJS object".
//!
//! Test262's `order-of-operations.js` fixtures hit this: their
//! `propertyBagObserver` is a `Proxy` whose `get` trap builds a *fresh*
//! `toPrimitiveObserver` object on every read, so each field value is
//! referenced only by the Rust caller. Running with `nursery_capacity: 1`
//! makes every allocation a collection point, which turns the fixture's
//! incidental timing into a deterministic check.

use blueice_bluejs::{compile, parse, HeapConfig, RuntimeError, Value, Vm, VmConfig};

/// Every allocation may collect, so an unrooted value cannot survive a later
/// allocation by luck.
fn gc_stress_vm() -> Vm {
    Vm::new(VmConfig {
        heap: HeapConfig {
            nursery_capacity: 1,
            ..HeapConfig::default()
        },
        ..VmConfig::default()
    })
    .unwrap()
}

fn evaluate(source: &str) -> Result<Value, RuntimeError> {
    gc_stress_vm().execute(&compile(&parse(source).unwrap()).unwrap())
}

/// A bag whose every property read returns a brand-new converting object,
/// recording the observable operations the way the Test262 helper does.
const OBSERVED_BAG: &str = r#"
    const actual = [];
    function observer(name, value) {
        return {
            get toString() { actual.push("get " + name + ".toString"); return function () { actual.push("call " + name + ".toString"); return value; }; },
            get valueOf() { actual.push("get " + name + ".valueOf"); return function () { actual.push("call " + name + ".valueOf"); return value; }; },
        };
    }
    function bagOf(values, label) {
        return new Proxy(values, {
            get(target, key) {
                if (typeof key === "symbol") return undefined;
                actual.push("get " + label + "." + key);
                const value = target[key];
                return (typeof value === "string" && key !== "calendar") || typeof value === "number"
                    ? observer(label + "." + key, value)
                    : value;
            },
            has() { return true; },
        });
    }
"#;

fn assert_true(body: &str) {
    let source = format!("{OBSERVED_BAG}\n{body}");
    assert_eq!(evaluate(&source), Ok(Value::Bool(true)), "{body}");
}

/// `PlainMonthDay/from/order-of-operations.js`: the fields are read and
/// converted one at a time, alphabetically, each conversion immediately after
/// its own read.
#[test]
fn plain_month_day_from_keeps_each_observed_field_alive_and_reads_alphabetically() {
    assert_true(
        r#"
        const fields = bagOf({ year: 1.7, month: 1.7, monthCode: "M01", day: 1.7, calendar: "iso8601" }, "fields");
        Temporal.PlainMonthDay.from(fields);
        actual.join() === [
            "get fields.calendar",
            "get fields.day", "get fields.day.valueOf", "call fields.day.valueOf",
            "get fields.month", "get fields.month.valueOf", "call fields.month.valueOf",
            "get fields.monthCode", "get fields.monthCode.toString", "call fields.monthCode.toString",
            "get fields.year", "get fields.year.valueOf", "call fields.year.valueOf",
        ].join()
    "#,
    );
}

/// `PlainMonthDay/prototype/with/order-of-operations.js`.
#[test]
fn plain_month_day_with_keeps_each_observed_field_alive() {
    assert_true(
        r#"
        const md = new Temporal.PlainMonthDay(5, 2);
        const fields = bagOf({ year: 1.7, month: 1.7, monthCode: "M01", day: 1.7 }, "fields");
        md.with(fields);
        actual.filter((entry) => entry.startsWith("call ")).join() === [
            "call fields.day.valueOf",
            "call fields.month.valueOf",
            "call fields.monthCode.toString",
            "call fields.year.valueOf",
        ].join()
    "#,
    );
}

/// `ZonedDateTime/prototype/with/order-of-operations.js`.
#[test]
fn zoned_date_time_with_keeps_each_observed_field_alive() {
    assert_true(
        r#"
        const zdt = new Temporal.ZonedDateTime(0n, "UTC");
        const fields = bagOf({
            year: 1.7, month: 1.7, monthCode: "M01", day: 1.7,
            hour: 1.7, minute: 1.7, second: 1.7,
            millisecond: 1.7, microsecond: 1.7, nanosecond: 1.7,
            offset: "+00:00",
        }, "fields");
        zdt.with(fields);
        actual.filter((entry) => entry.startsWith("call ")).length >= 10
    "#,
    );
}

/// The remaining steps of `PlainMonthDay/from/order-of-operations.js`: the
/// `options` object is read even when the first argument is already a
/// `PlainMonthDay` or is a string, and the fields are still read before the
/// `TypeError` for a primitive `options`.
#[test]
fn plain_month_day_from_reads_options_for_instances_and_strings() {
    assert_true(
        r#"
        const options = bagOf({ overflow: "constrain", extra: "property" }, "options");
        const optionsReading = ["get options.overflow", "get options.overflow.toString", "call options.overflow.toString"];
        const seen = () => actual.splice(0).join();

        Temporal.PlainMonthDay.from(new Temporal.PlainMonthDay(5, 2), options);
        const fromInstance = seen();
        Temporal.PlainMonthDay.from("05-02", options);
        const fromString = seen();

        const fields = bagOf({ year: 1.7, month: 1.7, monthCode: "M01", day: 1.7, calendar: "iso8601" }, "fields");
        let threw = false;
        try { Temporal.PlainMonthDay.from(fields, null); } catch (error) { threw = error instanceof TypeError; }
        const fieldReads = seen();

        fromInstance === optionsReading.join() && fromString === optionsReading.join() && threw
            && fieldReads.split(",").filter((entry) => entry.startsWith("call ")).length === 4
    "#,
    );
}
