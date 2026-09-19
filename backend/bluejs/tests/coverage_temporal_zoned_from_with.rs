// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Coverage for `Temporal.ZonedDateTime`'s construction and field-merging
//! surface (`vm/temporal/zoned.rs`): `ToTemporalZonedDateTime`'s string and
//! property-bag branches, `InterpretISODateTimeOffset`'s `offset`/
//! `disambiguation` handling, `with`, `withTimeZone` and `withPlainTime`.
//!
//! Only fixed IANA identifiers and fixed instants are used, so nothing here
//! depends on the host's own time zone or locale. Every expectation follows
//! the ECMAScript Temporal specification.

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm};

fn run(source: &str) -> Result<Value, RuntimeError> {
    Vm::default().execute(&compile(&parse(source).unwrap()).unwrap())
}

fn assert_str(source: &str, expected: &str) {
    match run(source) {
        Ok(Value::String(actual)) => assert_eq!(actual, expected, "{source}"),
        other => panic!("{source}\n  -> expected string {expected:?}, got {other:?}"),
    }
}

fn assert_true(source: &str) {
    assert_eq!(run(source), Ok(Value::Bool(true)), "{source}");
}

fn assert_range_errors(sources: &[&str]) {
    for source in sources {
        match run(source) {
            Err(RuntimeError::RangeError(_)) => {}
            other => panic!("{source}\n  -> expected RangeError, got {other:?}"),
        }
    }
}

fn assert_type_errors(sources: &[&str]) {
    for source in sources {
        match run(source) {
            Err(RuntimeError::TypeError(_)) => {}
            other => panic!("{source}\n  -> expected TypeError, got {other:?}"),
        }
    }
}

const NY: &str = "America/New_York";

#[test]
fn from_string_resolves_a_spring_forward_gap_by_disambiguation() {
    let source = r#"
        const z = (options) => Temporal.ZonedDateTime.from("2020-03-08T02:30:00[America/New_York]", options).toString();
        [z(), z({ disambiguation: "compatible" }), z({ disambiguation: "earlier" }),
         z({ disambiguation: "later" })].join("|")
    "#;
    assert_str(
        source,
        "2020-03-08T03:30:00-04:00[America/New_York]|\
         2020-03-08T03:30:00-04:00[America/New_York]|\
         2020-03-08T01:30:00-05:00[America/New_York]|\
         2020-03-08T03:30:00-04:00[America/New_York]",
    );
    assert_range_errors(&[
        r#"Temporal.ZonedDateTime.from("2020-03-08T02:30:00[America/New_York]", { disambiguation: "reject" })"#,
    ]);
}

#[test]
fn from_string_resolves_a_fall_back_overlap_by_disambiguation() {
    let source = r#"
        const z = (options) => Temporal.ZonedDateTime.from("2020-11-01T01:30:00[America/New_York]", options).toString();
        [z(), z({ disambiguation: "earlier" }), z({ disambiguation: "later" })].join("|")
    "#;
    assert_str(
        source,
        "2020-11-01T01:30:00-04:00[America/New_York]|\
         2020-11-01T01:30:00-04:00[America/New_York]|\
         2020-11-01T01:30:00-05:00[America/New_York]",
    );
    assert_range_errors(&[
        r#"Temporal.ZonedDateTime.from("2020-11-01T01:30:00[America/New_York]", { disambiguation: "reject" })"#,
    ]);
}

#[test]
fn from_string_offset_option_governs_a_mismatched_offset() {
    let z = |options: &str| {
        format!(
            r#"Temporal.ZonedDateTime.from("2020-06-01T12:00:00-05:00[America/New_York]", {options}).toString()"#
        )
    };
    // The default `offset: "reject"` refuses an offset the zone never had.
    assert_range_errors(&[&z("undefined"), &z(r#"{ offset: "reject" }"#)]);
    // "use" trusts the written offset: 12:00-05:00 is 17:00Z, i.e. 13:00 EDT.
    assert_str(
        &z(r#"{ offset: "use" }"#),
        "2020-06-01T13:00:00-04:00[America/New_York]",
    );
    // "ignore" and "prefer" fall back to plain zone resolution.
    assert_str(
        &z(r#"{ offset: "ignore" }"#),
        "2020-06-01T12:00:00-04:00[America/New_York]",
    );
    assert_str(
        &z(r#"{ offset: "prefer" }"#),
        "2020-06-01T12:00:00-04:00[America/New_York]",
    );
}

#[test]
fn from_string_offset_selects_the_matching_side_of_an_overlap() {
    let z = |text: &str| format!(r#"Temporal.ZonedDateTime.from("{text}").toString()"#);
    assert_str(
        &z("2020-11-01T01:30:00-04:00[America/New_York]"),
        "2020-11-01T01:30:00-04:00[America/New_York]",
    );
    assert_str(
        &z("2020-11-01T01:30:00-05:00[America/New_York]"),
        "2020-11-01T01:30:00-05:00[America/New_York]",
    );
}

#[test]
fn from_string_with_z_designator_uses_the_exact_instant() {
    assert_str(
        r#"Temporal.ZonedDateTime.from("2020-06-01T16:00:00Z[America/New_York]").toString()"#,
        "2020-06-01T12:00:00-04:00[America/New_York]",
    );
    assert_str(
        r#"Temporal.ZonedDateTime.from("2020-06-01T16:00:00Z[UTC]").toString()"#,
        "2020-06-01T16:00:00+00:00[UTC]",
    );
}

#[test]
fn from_string_variants_annotations_and_precision() {
    // A date-only string is midnight in its zone.
    assert_str(
        r#"Temporal.ZonedDateTime.from("2020-06-01[America/New_York]").toString()"#,
        "2020-06-01T00:00:00-04:00[America/New_York]",
    );
    // A numeric-offset zone identifier.
    assert_str(
        r#"Temporal.ZonedDateTime.from("2020-01-01T00:00:00+01:00[+01:00]").toString()"#,
        "2020-01-01T00:00:00+01:00[+01:00]",
    );
    // Fractional seconds survive a round trip.
    assert_str(
        r#"Temporal.ZonedDateTime.from("2020-01-01T12:34:56.123456789[UTC]").toString()"#,
        "2020-01-01T12:34:56.123456789+00:00[UTC]",
    );
    // A calendar annotation is retained.
    assert_str(
        r#"Temporal.ZonedDateTime.from("2020-01-01T00:00:00[UTC][u-ca=japanese]").toString()"#,
        "2020-01-01T00:00:00+00:00[UTC][u-ca=japanese]",
    );
    assert_str(
        r#"Temporal.ZonedDateTime.from("2020-01-01T00:00:00[UTC][u-ca=japanese]").calendarId"#,
        "japanese",
    );
    // The string branch accepts an options bag and validates it.
    assert_str(
        r#"Temporal.ZonedDateTime.from("2020-01-01T00:00:00[UTC]", { overflow: "reject" }).timeZoneId"#,
        "UTC",
    );
    // An IANA identifier keeps its written spelling case-insensitively
    // canonicalised.
    assert_str(
        r#"Temporal.ZonedDateTime.from("2020-01-01T00:00:00[america/new_york]").timeZoneId"#,
        NY,
    );
}

#[test]
fn from_string_rejects_malformed_input() {
    assert_range_errors(&[
        // No time zone annotation at all.
        r#"Temporal.ZonedDateTime.from("2020-06-01T12:00:00")"#,
        r#"Temporal.ZonedDateTime.from("2020-06-01T12:00:00Z")"#,
        // Unknown zone and unknown calendar.
        r#"Temporal.ZonedDateTime.from("2020-06-01T12:00:00[Nope/Zone]")"#,
        r#"Temporal.ZonedDateTime.from("2020-06-01T12:00:00[UTC][u-ca=notacal]")"#,
        // Garbage and out-of-range calendar fields.
        r#"Temporal.ZonedDateTime.from("not a date")"#,
        r#"Temporal.ZonedDateTime.from("")"#,
        r#"Temporal.ZonedDateTime.from("2020-13-01T00:00:00[UTC]")"#,
        r#"Temporal.ZonedDateTime.from("2020-02-30T00:00:00[UTC]")"#,
        // Beyond the representable Instant range.
        r#"Temporal.ZonedDateTime.from("+275760-09-13T00:00:00.000000001Z[UTC]")"#,
        r#"Temporal.ZonedDateTime.from("-271821-04-19T23:59:59.999999999Z[UTC]")"#,
        // Invalid option values.
        r#"Temporal.ZonedDateTime.from("2020-01-01T00:00:00[UTC]", { disambiguation: "bogus" })"#,
        r#"Temporal.ZonedDateTime.from("2020-01-01T00:00:00[UTC]", { offset: "bogus" })"#,
        r#"Temporal.ZonedDateTime.from("2020-01-01T00:00:00[UTC]", { overflow: "bogus" })"#,
    ]);
    assert_type_errors(&[
        r#"Temporal.ZonedDateTime.from("2020-01-01T00:00:00[UTC]", 1)"#,
        r#"Temporal.ZonedDateTime.from("2020-01-01T00:00:00[UTC]", "constrain")"#,
        r#"Temporal.ZonedDateTime.from()"#,
        r#"Temporal.ZonedDateTime.from(Symbol())"#,
    ]);
}

#[test]
fn from_property_bag_builds_a_value_from_calendar_and_time_fields() {
    assert_str(
        r#"Temporal.ZonedDateTime.from({ year: 2020, month: 6, day: 1, hour: 12, minute: 30, second: 15,
            millisecond: 1, microsecond: 2, nanosecond: 3, timeZone: "America/New_York" }).toString()"#,
        "2020-06-01T12:30:15.001002003-04:00[America/New_York]",
    );
    // `monthCode` in place of `month`; time fields default to zero.
    assert_str(
        r#"Temporal.ZonedDateTime.from({ year: 2020, monthCode: "M02", day: 29, timeZone: "UTC" }).toString()"#,
        "2020-02-29T00:00:00+00:00[UTC]",
    );
    // A non-ISO calendar in the bag.
    assert_str(
        r#"Temporal.ZonedDateTime.from({ year: 2020, month: 1, day: 1, calendar: "gregory", timeZone: "UTC" }).toString()"#,
        "2020-01-01T00:00:00+00:00[UTC][u-ca=gregory]",
    );
    assert_str(
        r#"Temporal.ZonedDateTime.from({ era: "reiwa", eraYear: 2, month: 3, day: 4, calendar: "japanese", timeZone: "UTC" }).toString()"#,
        "2020-03-04T00:00:00+00:00[UTC][u-ca=japanese]",
    );
    // The bag's own `timeZone` may be an offset string or a ZonedDateTime-ish
    // identifier string with annotation syntax.
    assert_str(
        r#"Temporal.ZonedDateTime.from({ year: 2020, month: 1, day: 1, timeZone: "-08:00" }).toString()"#,
        "2020-01-01T00:00:00-08:00[-08:00]",
    );
}

#[test]
fn from_property_bag_offset_field_and_options() {
    let z = |bag: &str, options: &str| {
        format!(r#"Temporal.ZonedDateTime.from({bag}, {options}).toString()"#)
    };
    let base = r#"{ year: 2020, month: 6, day: 1, hour: 12, timeZone: "America/New_York", offset: "-05:00" }"#;
    assert_range_errors(&[&z(base, "undefined"), &z(base, r#"{ offset: "reject" }"#)]);
    assert_str(
        &z(base, r#"{ offset: "use" }"#),
        "2020-06-01T13:00:00-04:00[America/New_York]",
    );
    assert_str(
        &z(base, r#"{ offset: "ignore" }"#),
        "2020-06-01T12:00:00-04:00[America/New_York]",
    );
    assert_str(
        &z(base, r#"{ offset: "prefer" }"#),
        "2020-06-01T12:00:00-04:00[America/New_York]",
    );
    // A matching offset is accepted under "reject".
    assert_str(
        &z(
            r#"{ year: 2020, month: 6, day: 1, hour: 12, timeZone: "America/New_York", offset: "-04:00" }"#,
            r#"{ offset: "reject" }"#,
        ),
        "2020-06-01T12:00:00-04:00[America/New_York]",
    );
    // The offset field disambiguates a fall-back overlap.
    assert_str(
        &z(
            r#"{ year: 2020, month: 11, day: 1, hour: 1, minute: 30, timeZone: "America/New_York", offset: "-05:00" }"#,
            "undefined",
        ),
        "2020-11-01T01:30:00-05:00[America/New_York]",
    );
    // Disambiguation applies when no offset is given.
    assert_str(
        &z(
            r#"{ year: 2020, month: 3, day: 8, hour: 2, minute: 30, timeZone: "America/New_York" }"#,
            r#"{ disambiguation: "earlier" }"#,
        ),
        "2020-03-08T01:30:00-05:00[America/New_York]",
    );
    // An offset spelled with sub-minute precision is a bag-level MatchExactly
    // comparison: Monrovia's real 1970 offset is -00:44:30.
    assert_str(
        &z(
            r#"{ year: 1970, month: 1, day: 1, timeZone: "Africa/Monrovia", offset: "-00:44:30" }"#,
            "undefined",
        ),
        "1970-01-01T00:00:00-00:44:30[Africa/Monrovia]",
    );
    // Overflow constrain clamps, reject throws.
    assert_str(
        &z(
            r#"{ year: 2021, month: 2, day: 31, timeZone: "UTC" }"#,
            "undefined",
        ),
        "2021-02-28T00:00:00+00:00[UTC]",
    );
    assert_range_errors(&[&z(
        r#"{ year: 2021, month: 2, day: 31, timeZone: "UTC" }"#,
        r#"{ overflow: "reject" }"#,
    )]);
}

#[test]
fn from_property_bag_rejects_bad_fields() {
    assert_type_errors(&[
        // `timeZone` is required.
        r#"Temporal.ZonedDateTime.from({ year: 2020, month: 1, day: 1 })"#,
        // The offset field must already be a string.
        r#"Temporal.ZonedDateTime.from({ year: 2020, month: 1, day: 1, timeZone: "UTC", offset: 1000 })"#,
        r#"Temporal.ZonedDateTime.from({ year: 2020, month: 1, day: 1, timeZone: "UTC", offset: null })"#,
        r#"Temporal.ZonedDateTime.from({ year: 2020, month: 1, day: 1, timeZone: "UTC", offset: true })"#,
        r#"Temporal.ZonedDateTime.from({ year: 2020, month: 1, day: 1, timeZone: "UTC", offset: 1000n })"#,
        // A non-string, non-object time zone.
        r#"Temporal.ZonedDateTime.from({ year: 2020, month: 1, day: 1, timeZone: 5 })"#,
        // Missing required calendar fields.
        r#"Temporal.ZonedDateTime.from({ month: 1, day: 1, timeZone: "UTC" })"#,
        r#"Temporal.ZonedDateTime.from({ year: 2020, timeZone: "UTC" })"#,
        r#"Temporal.ZonedDateTime.from({ year: 2020, month: 1, timeZone: "UTC" })"#,
        // Options that are not an object.
        r#"Temporal.ZonedDateTime.from({ year: 2020, month: 1, day: 1, timeZone: "UTC" }, 5)"#,
    ]);
    assert_range_errors(&[
        r#"Temporal.ZonedDateTime.from({ year: 2020, month: 1, day: 1, timeZone: "Nope/Zone" })"#,
        r#"Temporal.ZonedDateTime.from({ year: 2020, month: 1, day: 1, timeZone: "UTC", offset: "--00:00" })"#,
        r#"Temporal.ZonedDateTime.from({ year: 2020, month: 1, day: 1, timeZone: "UTC", offset: "+25:00" })"#,
        r#"Temporal.ZonedDateTime.from({ year: 2020, month: 1, day: 1, timeZone: "UTC", offset: "garbage" })"#,
        r#"Temporal.ZonedDateTime.from({ year: 2020, month: 13, day: 1, timeZone: "UTC" }, { overflow: "reject" })"#,
        r#"Temporal.ZonedDateTime.from({ year: 2020, month: 1, day: 1, timeZone: "UTC", calendar: "notacal" })"#,
        r#"Temporal.ZonedDateTime.from({ year: 275760, month: 9, day: 14, timeZone: "UTC" })"#,
        r#"Temporal.ZonedDateTime.from({ year: 2020, month: 1, day: 1, hour: 2, timeZone: "UTC" }, { disambiguation: "nonsense" })"#,
    ]);
}

#[test]
fn from_a_zoned_date_time_copies_it_and_still_validates_options() {
    assert_true(
        r#"
        const original = new Temporal.ZonedDateTime(1_600_000_000_123_456_789n, "Asia/Tokyo");
        const copy = Temporal.ZonedDateTime.from(original, { overflow: "reject", disambiguation: "later", offset: "use" });
        copy !== original && copy.equals(original) && copy.timeZoneId === "Asia/Tokyo"
        "#,
    );
    assert_range_errors(&[
        r#"Temporal.ZonedDateTime.from(new Temporal.ZonedDateTime(0n, "UTC"), { overflow: "bogus" })"#,
        r#"Temporal.ZonedDateTime.from(new Temporal.ZonedDateTime(0n, "UTC"), { disambiguation: "bogus" })"#,
        r#"Temporal.ZonedDateTime.from(new Temporal.ZonedDateTime(0n, "UTC"), { offset: "bogus" })"#,
    ]);
}

#[test]
fn constructor_validates_arguments_and_sets_local_fields() {
    assert_str(
        r#"new Temporal.ZonedDateTime(0n, "America/New_York").toString()"#,
        "1969-12-31T19:00:00-05:00[America/New_York]",
    );
    assert_str(
        r#"new Temporal.ZonedDateTime(0n, "Asia/Kolkata").toString()"#,
        "1970-01-01T05:30:00+05:30[Asia/Kolkata]",
    );
    assert_str(
        r#"new Temporal.ZonedDateTime(-1n, "UTC").toString()"#,
        "1969-12-31T23:59:59.999999999+00:00[UTC]",
    );
    assert_str(
        r#"new Temporal.ZonedDateTime(0n, "UTC", "gregory").toString()"#,
        "1970-01-01T00:00:00+00:00[UTC][u-ca=gregory]",
    );
    // The extreme representable instants.
    assert_str(
        r#"new Temporal.ZonedDateTime(-8640000000000000000000n, "UTC").toString()"#,
        "-271821-04-20T00:00:00+00:00[UTC]",
    );
    assert_str(
        r#"new Temporal.ZonedDateTime(8640000000000000000000n, "UTC").toString()"#,
        "+275760-09-13T00:00:00+00:00[UTC]",
    );
    assert_type_errors(&[
        r#"new Temporal.ZonedDateTime()"#,
        r#"new Temporal.ZonedDateTime(0, "UTC")"#,
        r#"new Temporal.ZonedDateTime(0n)"#,
        r#"new Temporal.ZonedDateTime(0n, 5)"#,
        r#"new Temporal.ZonedDateTime(0n, "UTC", 5)"#,
        r#"Temporal.ZonedDateTime(0n, "UTC")"#,
    ]);
    assert_range_errors(&[
        r#"new Temporal.ZonedDateTime(8640000000000000000001n, "UTC")"#,
        r#"new Temporal.ZonedDateTime(-8640000000000000000001n, "UTC")"#,
        r#"new Temporal.ZonedDateTime(0n, "Nope/Zone")"#,
        r#"new Temporal.ZonedDateTime(0n, "UTC", "notacal")"#,
    ]);
}

#[test]
fn getters_report_local_fields_offset_and_day_length() {
    assert_str(
        r#"
        const z = Temporal.ZonedDateTime.from("2020-03-08T12:00:00[America/New_York]");
        [z.year, z.month, z.monthCode, z.day, z.hour, z.minute, z.second, z.millisecond, z.microsecond,
         z.nanosecond, z.dayOfWeek, z.dayOfYear, z.weekOfYear, z.yearOfWeek, z.daysInWeek, z.daysInMonth,
         z.daysInYear, z.monthsInYear, z.inLeapYear, z.offset, z.offsetNanoseconds, z.hoursInDay,
         z.calendarId, z.timeZoneId, z.era, z.eraYear, z.epochMilliseconds, z.epochNanoseconds].join(",")
        "#,
        "2020,3,M03,8,12,0,0,0,0,0,7,68,10,2020,7,31,366,12,true,-04:00,-14400000000000,23,\
         iso8601,America/New_York,,,1583683200000,1583683200000000000",
    );
    // A fall-back day is 25 hours long and an ordinary one 24.
    assert_str(
        r#"[Temporal.ZonedDateTime.from("2020-11-01T12:00:00[America/New_York]").hoursInDay,
            Temporal.ZonedDateTime.from("2020-06-01T12:00:00[America/New_York]").hoursInDay,
            Temporal.ZonedDateTime.from("2020-06-01T12:00:00[UTC]").hoursInDay].join(",")"#,
        "25,24,24",
    );
    // Eras of a non-ISO calendar.
    assert_str(
        r#"
        const j = Temporal.ZonedDateTime.from("2020-06-01T00:00:00[UTC][u-ca=japanese]");
        [j.era, j.eraYear, j.year, j.monthCode].join(",")
        "#,
        "reiwa,2,2020,M06",
    );
    // Sub-minute historical offsets are reported exactly.
    assert_str(
        r#"new Temporal.ZonedDateTime(0n, "Africa/Monrovia").offset"#,
        "-00:44:30",
    );
    assert_true(
        r#"new Temporal.ZonedDateTime(0n, "Africa/Monrovia").offsetNanoseconds === -2670e9"#,
    );
}

#[test]
fn with_merges_fields_and_reresolves_through_the_zone() {
    let base = r#"const z = Temporal.ZonedDateTime.from("2020-06-15T12:34:56.789012345[America/New_York]");"#;
    let with = |bag: &str| format!("{base}\nz.with({bag}).toString()");
    assert_str(
        &with("{ year: 2021 }"),
        "2021-06-15T12:34:56.789012345-04:00[America/New_York]",
    );
    assert_str(
        &with("{ month: 12 }"),
        "2020-12-15T12:34:56.789012345-05:00[America/New_York]",
    );
    assert_str(
        &with(r#"{ monthCode: "M02", day: 29 }"#),
        "2020-02-29T12:34:56.789012345-05:00[America/New_York]",
    );
    assert_str(
        &with("{ hour: 1, minute: 2, second: 3, millisecond: 4, microsecond: 5, nanosecond: 6 }"),
        "2020-06-15T01:02:03.004005006-04:00[America/New_York]",
    );
    // Day overflow: constrain by default, reject on request.
    assert_str(
        &with("{ month: 2, day: 31 }"),
        "2020-02-29T12:34:56.789012345-05:00[America/New_York]",
    );
    assert_range_errors(&[&with(r#"{ month: 2, day: 31 }, { overflow: "reject" }"#)]);
    // `with` accepts the same option-bag validation as `from`.
    assert_range_errors(&[
        &with(r#"{ year: 2021 }, { disambiguation: "bogus" }"#),
        &with(r#"{ year: 2021 }, { offset: "bogus" }"#),
        &with(r#"{ year: 2021 }, { overflow: "bogus" }"#),
    ]);
    assert_type_errors(&[&with("{ year: 2021 }, 5")]);
}

#[test]
fn with_offset_field_and_option_pick_the_right_instant() {
    // Fall-back overlap: 01:30 occurs twice.
    let base = r#"const z = Temporal.ZonedDateTime.from("2020-11-01T00:30:00[America/New_York]");"#;
    let with = |bag: &str| format!("{base}\nz.with({bag}).toString()");
    // Default `offset: "prefer"` keeps the receiver's own offset (-04:00) when
    // it is valid for the new wall time.
    assert_str(
        &with("{ hour: 1 }"),
        "2020-11-01T01:30:00-04:00[America/New_York]",
    );
    // An explicit `offset` field selects the later occurrence.
    assert_str(
        &with(r#"{ hour: 1, offset: "-05:00" }"#),
        "2020-11-01T01:30:00-05:00[America/New_York]",
    );
    // An offset that fits neither candidate: "reject" throws; "use" trusts
    // it; "ignore"/"prefer" fall back to the zone.
    assert_range_errors(&[&with(
        r#"{ hour: 1, offset: "+09:00" }, { offset: "reject" }"#,
    )]);
    assert_str(
        &with(r#"{ hour: 1, offset: "+09:00" }, { offset: "use" }"#),
        "2020-10-31T12:30:00-04:00[America/New_York]",
    );
    assert_str(
        &with(r#"{ hour: 1, offset: "+09:00" }, { offset: "ignore" }"#),
        "2020-11-01T01:30:00-04:00[America/New_York]",
    );
    assert_str(
        &with(r#"{ hour: 1, offset: "+09:00" }"#),
        "2020-11-01T01:30:00-04:00[America/New_York]",
    );
    // Setting a wall time inside a spring-forward gap uses `disambiguation`.
    let gap = r#"const g = Temporal.ZonedDateTime.from("2020-03-08T00:30:00[America/New_York]");"#;
    assert_str(
        &format!("{gap}\ng.with({{ hour: 2 }}).toString()"),
        "2020-03-08T03:30:00-04:00[America/New_York]",
    );
    assert_str(
        &format!(
            r#"{gap}
g.with({{ hour: 2 }}, {{ disambiguation: "earlier" }}).toString()"#
        ),
        "2020-03-08T01:30:00-05:00[America/New_York]",
    );
    assert_range_errors(&[&format!(
        r#"{gap}
g.with({{ hour: 2, offset: "-05:00" }}, {{ disambiguation: "reject", offset: "reject" }})"#
    )]);
}

#[test]
fn with_rejects_invalid_arguments() {
    let base = r#"const z = Temporal.ZonedDateTime.from("2020-06-15T12:00:00[UTC]");"#;
    let with = |bag: &str| format!("{base}\nz.with({bag})");
    assert_type_errors(&[
        &with("undefined"),
        &with("5"),
        &with(r#""2020-01-01""#),
        &with("{}"),
        &with("{ unrelated: 1 }"),
        &with(r#"{ calendar: "iso8601" }"#),
        &with(r#"{ timeZone: "UTC" }"#),
        &with(r#"{ year: 2020, timeZone: "UTC" }"#),
        &with("Temporal.PlainDate.from('2020-01-01')"),
        &with("z"),
        "Temporal.ZonedDateTime.prototype.with.call({}, { year: 2020 })",
    ]);
    assert_range_errors(&[
        &with("{ hour: 24 }, { overflow: 'reject' }"),
        &with(r#"{ offset: "garbage" }"#),
        &with("{ year: 300000 }"),
        &with(r#"{ month: 13 }, { overflow: "reject" }"#),
        &with(r#"{ monthCode: "M13" }"#),
        // `month` and `monthCode` disagreeing.
        &with(r#"{ month: 3, monthCode: "M04" }"#),
    ]);
}

#[test]
fn with_offset_only_bag_is_a_recognised_property() {
    // `offset` alone counts as a recognised property, so no TypeError.
    assert_str(
        r#"Temporal.ZonedDateTime.from("2020-06-15T12:00:00-04:00[America/New_York]")
            .with({ offset: "-04:00" }).toString()"#,
        "2020-06-15T12:00:00-04:00[America/New_York]",
    );
}

#[test]
fn with_era_fields_follow_the_calendar_rules() {
    let japanese =
        r#"const j = Temporal.ZonedDateTime.from("2020-06-15T12:00:00[UTC][u-ca=japanese]");"#;
    let with = |bag: &str| format!("{japanese}\nj.with({bag}).toString()");
    assert_str(
        &with(r#"{ era: "heisei", eraYear: 10 }"#),
        "1998-06-15T12:00:00+00:00[UTC][u-ca=japanese]",
    );
    assert_type_errors(&[&with(r#"{ era: "heisei" }"#), &with("{ eraYear: 10 }")]);
    // `chinese`/`dangi` have no eras at all: any use is rejected.
    let chinese =
        r#"const c = Temporal.ZonedDateTime.from("2020-06-15T12:00:00[UTC][u-ca=chinese]");"#;
    assert_type_errors(&[
        &format!(
            r#"{chinese}
c.with({{ era: "x", eraYear: 1 }})"#
        ),
        &format!(
            r#"{chinese}
c.with({{ eraYear: 1 }})"#
        ),
    ]);
    // Without era fields the year is replaced directly.
    assert_str(
        &format!("{chinese}\nc.with({{ day: 1 }}).calendarId"),
        "chinese",
    );
    // ISO silently ignores `era`/`eraYear`.
    assert_str(
        r#"Temporal.ZonedDateTime.from("2020-06-15T12:00:00[UTC]").with({ era: "ce", eraYear: 1999, day: 1 }).toString()"#,
        "2020-06-01T12:00:00+00:00[UTC]",
    );
}

#[test]
fn with_time_zone_keeps_the_instant_and_changes_the_presentation() {
    assert_str(
        r#"Temporal.ZonedDateTime.from("2020-06-15T12:00:00[UTC]").withTimeZone("Asia/Tokyo").toString()"#,
        "2020-06-15T21:00:00+09:00[Asia/Tokyo]",
    );
    assert_str(
        r#"Temporal.ZonedDateTime.from("2020-06-15T12:00:00[UTC]").withTimeZone("+02:30").toString()"#,
        "2020-06-15T14:30:00+02:30[+02:30]",
    );
    assert_true(
        r#"
        const z = Temporal.ZonedDateTime.from("2020-06-15T12:00:00[UTC]");
        z.withTimeZone("Asia/Tokyo").epochNanoseconds === z.epochNanoseconds
        "#,
    );
    assert_type_errors(&[
        r#"Temporal.ZonedDateTime.from("2020-06-15T12:00:00[UTC]").withTimeZone()"#,
        r#"Temporal.ZonedDateTime.from("2020-06-15T12:00:00[UTC]").withTimeZone(5)"#,
    ]);
    assert_range_errors(&[
        r#"Temporal.ZonedDateTime.from("2020-06-15T12:00:00[UTC]").withTimeZone("Nope/Zone")"#,
    ]);
}

#[test]
fn with_plain_time_replaces_the_time_of_day() {
    let z = r#"const z = Temporal.ZonedDateTime.from("2020-06-15T12:34:56[America/New_York]");"#;
    assert_str(
        &format!("{z}\nz.withPlainTime().toString()"),
        "2020-06-15T00:00:00-04:00[America/New_York]",
    );
    assert_str(
        &format!(
            r#"{z}
z.withPlainTime("08:15:30.5").toString()"#
        ),
        "2020-06-15T08:15:30.5-04:00[America/New_York]",
    );
    assert_str(
        &format!("{z}\nz.withPlainTime({{ hour: 1, minute: 2 }}).toString()"),
        "2020-06-15T01:02:00-04:00[America/New_York]",
    );
    assert_str(
        &format!("{z}\nz.withPlainTime(new Temporal.PlainTime(23, 59)).toString()"),
        "2020-06-15T23:59:00-04:00[America/New_York]",
    );
    // A time inside a spring-forward gap resolves with "compatible".
    assert_str(
        r#"Temporal.ZonedDateTime.from("2020-03-08T12:00:00[America/New_York]").withPlainTime("02:30").toString()"#,
        "2020-03-08T03:30:00-04:00[America/New_York]",
    );
    assert_type_errors(&[
        &format!("{z}\nz.withPlainTime(5)"),
        &format!("{z}\nz.withPlainTime(null)"),
        "Temporal.ZonedDateTime.prototype.withPlainTime.call({})",
    ]);
    assert_range_errors(&[&format!(
        r#"{z}
z.withPlainTime("25:00")"#
    )]);
}
