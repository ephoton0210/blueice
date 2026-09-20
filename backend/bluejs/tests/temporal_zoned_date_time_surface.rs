// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for four small `Temporal.ZonedDateTime` gaps found while
//! closing the Test262 `ZonedDateTime/` tree after the until/since rewrite:
//!
//! 1. `with()` range-checked every time-of-day field while *reading* it, so
//!    `overflow: "constrain"` (the default) threw `RangeError` for `hour: 29`
//!    instead of constraining to 23. `RegulateTime` runs after every option is
//!    read, clamping under `constrain` and throwing only under `reject`.
//! 2. `toString()` validated `smallestUnit` before reading `timeZoneName`, so
//!    the last option was never read when the unit was invalid
//!    (`GetTemporalShowTimeZoneNameOption` is read with the rest of the options,
//!    before any algorithmic validation).
//! 3. `toString()` printed a sub-minute historical offset exactly
//!    (`+00:09:21`); `TemporalZonedDateTimeToString` uses
//!    `FormatDateTimeUTCOffsetRounded`, i.e. `+00:09`, while the `offset` getter
//!    stays exact.
//! 4. `getTimeZoneTransition` returned rule changes that leave the total UTC
//!    offset unchanged (Europe/London 1968), which are not transitions.
//!
//! Also pins `TimeZone` equality for the `GMT` link group.
//!
//! Every expectation is taken from a real Test262 fixture, named at each test,
//! except where a comment says it is derived from the specification algorithm
//! (`RegulateTime` clamping a negative time field under `constrain`).

use blueice_bluejs::{compile, parse, Value, Vm};

const PRELUDE: &str = r#"
function assertSame(actual, expected, message) {
  if (!Object.is(actual, expected)) {
    throw new Error(message + ": got " + String(actual) + " want " + String(expected));
  }
}
function assertRangeError(fn, message) {
  try { fn(); } catch (e) {
    if (e instanceof RangeError) return;
    throw new Error(message + ": expected RangeError, got " + e);
  }
  throw new Error(message + ": expected RangeError, nothing thrown");
}
"#;

/// Runs `body` after [`PRELUDE`], failing with the JS-side message on a throw.
fn check(body: &str) {
    let source =
        format!("{PRELUDE}\ntry {{\n{body}\n\"ok\"\n}} catch (e) {{ \"FAILED: \" + e.message }}");
    let value = Vm::default()
        .execute(&compile(&parse(&source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{body}\n  -> {error:?}"));
    match value {
        Value::String(text) => assert_eq!(text.to_utf8().unwrap(), "ok", "{body}"),
        other => panic!("unexpected completion value {other:?}"),
    }
}

/// `built-ins/Temporal/ZonedDateTime/prototype/with/{overflow-options,
/// overflow-undefined}.js`.
#[test]
fn with_constrains_out_of_range_time_fields_unless_overflow_is_reject() {
    check(
        r#"
        const zdt = new Temporal.PlainDateTime(1976, 11, 18, 15, 23, 30, 123, 456, 789).toZonedDateTime("UTC");
        const overflow = "constrain";
        const same = (actual, iso, message) => assertSame(actual.epochNanoseconds,
          Temporal.ZonedDateTime.from(iso).epochNanoseconds, message);
        same(zdt.with({ month: 29 }, { overflow }), "1976-12-18T15:23:30.123456789+00:00[UTC]", "month");
        same(zdt.with({ day: 31 }, { overflow }), "1976-11-30T15:23:30.123456789+00:00[UTC]", "day");
        same(zdt.with({ hour: 29 }, { overflow }), "1976-11-18T23:23:30.123456789+00:00[UTC]", "hour");
        same(zdt.with({ minute: 99 }, { overflow }), "1976-11-18T15:59:30.123456789+00:00[UTC]", "minute");
        same(zdt.with({ second: 67 }, { overflow }), "1976-11-18T15:23:59.123456789+00:00[UTC]", "second");
        same(zdt.with({ nanosecond: 9000 }, { overflow }), "1976-11-18T15:23:30.123456999+00:00[UTC]", "nanosecond");
        same(zdt.with({ hour: -1 }, { overflow }), "1976-11-18T00:23:30.123456789+00:00[UTC]", "negative hour clamps to 0");

        // `constrain` is the default, however the options argument is spelled.
        const datetime = new Temporal.ZonedDateTime(1_000_000_000_987_654_321n, "UTC");
        assertSame(datetime.with({ second: 67 }, { overflow: undefined }).epochNanoseconds, 1_000_000_019_987_654_321n, "explicit undefined");
        assertSame(datetime.with({ second: 67 }, {}).epochNanoseconds, 1_000_000_019_987_654_321n, "empty options");
        assertSame(datetime.with({ second: 67 }, () => {}).epochNanoseconds, 1_000_000_019_987_654_321n, "function options");

        // `reject` still throws, for every time field, above and below the range.
        for (const field of ["hour", "minute", "second", "millisecond", "microsecond", "nanosecond"]) {
          assertRangeError(() => zdt.with({ [field]: 1000 }, { overflow: "reject" }), field + " too large");
          assertRangeError(() => zdt.with({ [field]: -1 }, { overflow: "reject" }), field + " negative");
        }
        // An infinite value is never a valid integer, whatever `overflow` says.
        assertRangeError(() => zdt.with({ hour: Infinity }, { overflow }), "infinity");
    "#,
    );
}

/// `built-ins/Temporal/ZonedDateTime/prototype/toString/options-read-before-
/// algorithmic-validation.js`.
#[test]
fn to_string_reads_every_option_before_validating_the_smallest_unit() {
    check(
        r#"
        const seen = [];
        const options = new Proxy({ calendarName: "always", timeZoneName: "always", smallestUnit: "month",
          fractionalSecondDigits: "auto", roundingMode: "expand", offset: "auto" }, {
          get(target, key) { seen.push(String(key)); return target[key]; },
        });
        assertRangeError(() => new Temporal.ZonedDateTime(2n, "UTC").toString(options), "date unit");
        const reads = seen.filter((key) => key !== "toString");
        assertSame(reads.join(","),
          "calendarName,fractionalSecondDigits,offset,roundingMode,smallestUnit,timeZoneName", "read order");
    "#,
    );
}

/// `intl402/Temporal/ZonedDateTime/prototype/getTimeZoneTransition/
/// subtract-second-and-nanosecond-from-last-transition.js`: Paris was on local
/// mean time (+00:09:21) until 1911. `toString` rounds that offset to the
/// minute (`FormatDateTimeUTCOffsetRounded`); the `offset` getter does not.
#[test]
fn to_string_rounds_a_sub_minute_offset_but_the_offset_getter_does_not() {
    check(
        r#"
        const zdt = new Temporal.PlainDateTime(1800, 1, 1).toZonedDateTime("Europe/Paris");
        assertSame(zdt.toString(), "1800-01-01T00:00:00+00:09[Europe/Paris]", "toString");
        assertSame(zdt.offset, "+00:09:21", "offset getter");
        assertSame(zdt.offsetNanoseconds, (9 * 60 + 21) * 1_000_000_000, "offsetNanoseconds");
        // Halves round away from zero: Monrovia's -00:44:30 is exactly half a minute past -00:44.
        const monrovia = new Temporal.ZonedDateTime(0n, "Africa/Monrovia");
        assertSame(monrovia.offset, "-00:44:30", "Monrovia offset");
        assertSame(monrovia.toString({ timeZoneName: "never" }), "1969-12-31T23:15:30-00:45", "Monrovia toString");
        // The rounded string is what `getTimeZoneTransition` walks back through.
        const first = zdt.getTimeZoneTransition("next");
        assertSame(first.toString(), "1911-03-10T23:50:39+00:00[Europe/Paris]", "the 1911 transition");
        const beforeBySecond = first.add({ seconds: -1 });
        assertSame(beforeBySecond.toString(), "1911-03-10T23:59:59+00:09[Europe/Paris]", "one second before");
        assertSame(beforeBySecond.getTimeZoneTransition("next").toString(), "1911-03-10T23:50:39+00:00[Europe/Paris]", "next from -1s");
        const beforeByNs = first.add({ nanoseconds: -1 });
        assertSame(beforeByNs.toString(), "1911-03-10T23:59:59.999999999+00:09[Europe/Paris]", "one ns before");
        assertSame(beforeByNs.getTimeZoneTransition("next").toString(), "1911-03-10T23:50:39+00:00[Europe/Paris]", "next from -1ns");
        // Looking back, a fractional instant just after a transition already has it in the past,
        // and a transition does not count as before itself. Paris' local mean time (+00:09:21) and
        // Paris Mean Time before 1891 shared one offset, so nothing precedes the 1911 change.
        assertSame(first.getTimeZoneTransition("previous"), null, "no earlier offset change");
        const second = first.getTimeZoneTransition("next");
        assertSame(second.epochNanoseconds > first.epochNanoseconds, true, "the next one is later");
        assertSame(second.getTimeZoneTransition("previous").epochNanoseconds, first.epochNanoseconds, "previous of the second");
        assertSame(second.add({ nanoseconds: 1 }).getTimeZoneTransition("previous").epochNanoseconds, second.epochNanoseconds, "previous from +1ns");
        assertSame(second.add({ seconds: 1 }).getTimeZoneTransition("previous").epochNanoseconds, second.epochNanoseconds, "previous from +1s");
        assertSame(second.add({ nanoseconds: -1 }).getTimeZoneTransition("previous").epochNanoseconds, first.epochNanoseconds, "previous from -1ns");
    "#,
    );
}

/// `intl402/Temporal/ZonedDateTime/prototype/getTimeZoneTransition/
/// rule-change-without-offset-transition.js`: a TZDB rule change (Europe/London
/// and America/Anchorage in the 1960s) that leaves the total UTC offset alone
/// is not a transition.
#[test]
fn get_time_zone_transition_skips_rule_changes_that_keep_the_utc_offset() {
    check(
        r#"
        const offsetChanges = (zdt) => zdt.offsetNanoseconds !== zdt.subtract({ nanoseconds: 1 }).offsetNanoseconds;
        const londonPrev = new Temporal.ZonedDateTime(0n, "Europe/London").getTimeZoneTransition("previous");
        assertSame(offsetChanges(londonPrev), true, "London previous is an offset change");
        assertSame(londonPrev.epochNanoseconds, -59004000000000000n, "London previous is 1968-02-18T03:00+01:00");
        const londonNext = new Temporal.ZonedDateTime(-39488400000000000n, "Europe/London").getTimeZoneTransition("next");
        assertSame(offsetChanges(londonNext), true, "London next is an offset change");
        assertSame(londonNext.epochNanoseconds, 57722400000000000n, "London next is 1971-10-31T02:00+00:00");
        const anchoragePrev = new Temporal.ZonedDateTime(-84290400000000000n, "America/Anchorage").getTimeZoneTransition("previous");
        assertSame(offsetChanges(anchoragePrev), true, "Anchorage previous is an offset change");
        assertSame(anchoragePrev.epochNanoseconds, -765378000000000000n, "Anchorage previous");
        const anchorageNext = new Temporal.ZonedDateTime(-94658400000000000n, "America/Anchorage").getTimeZoneTransition("next");
        assertSame(offsetChanges(anchorageNext), true, "Anchorage next is an offset change");
        assertSame(anchorageNext.epochNanoseconds, -21470400000000000n, "Anchorage next");
    "#,
    );
}

/// `intl402/Temporal/ZonedDateTime/links.js`: the whole `GMT` link group is one
/// primary zone (`UTC`), so its members are equal to it and to each other.
#[test]
fn every_member_of_the_gmt_link_group_equals_utc() {
    check(
        r#"
        const utc = new Temporal.ZonedDateTime(0n, "UTC");
        for (const link of ["Etc/GMT", "Etc/GMT+0", "Etc/GMT-0", "Etc/GMT0", "Etc/Greenwich", "GMT", "GMT+0", "GMT-0",
                            "GMT0", "Greenwich", "Etc/UTC", "Etc/UCT", "Etc/Universal", "Etc/Zulu", "UCT", "Universal", "Zulu"]) {
          const zdt = new Temporal.ZonedDateTime(0n, link);
          assertSame(zdt.timeZoneId, link, "the spelling is preserved: " + link);
          assertSame(zdt.equals(utc), true, link + " equals UTC");
          assertSame(utc.equals(zdt), true, "UTC equals " + link);
          assertSame(zdt.offsetNanoseconds, 0, link + " has no offset");
        }
        assertSame(new Temporal.ZonedDateTime(0n, "Etc/GMT+0").equals(new Temporal.ZonedDateTime(0n, "Greenwich")), true, "two aliases");
        // A different zone with the same offset today is still a different zone.
        assertSame(new Temporal.ZonedDateTime(0n, "Africa/Abidjan").equals(utc), false, "Abidjan is not UTC");
        assertSame(new Temporal.ZonedDateTime(0n, "Etc/GMT+1").equals(utc), false, "Etc/GMT+1 is not UTC");
    "#,
    );
}
