// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for `InterpretISODateTimeOffset`'s `MatchMinutes`
//! fuzzy-offset-matching behaviour, pinned directly from the real Test262
//! fixture `intl402/Temporal/ZonedDateTime/prototype/equals/sub-minute-offset.js`
//! (and the sibling `from`/`compare` copies of the same fixture).
//!
//! `Africa/Monrovia`'s real historical offset before 1972 was the sub-minute
//! `-00:44:30`. A `ZonedDateTime` string's own *leading* UTC-offset field
//! (before any `[...]` annotation) is matched against a named zone's real
//! offset two ways, per Gecko's `InterpretISODateTimeOffset`
//! (`ZonedDateTime.cpp`):
//!
//! - `MatchExactly`, when the leading offset string itself spelled a seconds
//!   (or fractional) component, however that value rounds -- `-00:44:30`
//!   matches exactly; `-00:44:40` and `-00:45:00` do not (even though the
//!   latter numerically equals the *rounded* real offset) and are a
//!   `RangeError`.
//! - `MatchMinutes`, when the leading offset is HH:MM-only (or absent) --
//!   the real offset is rounded to the nearest minute (half-expand, away
//!   from zero) and compared to the given offset, so `-00:45` (no seconds
//!   spelled) matches `-00:44:30`'s own rounded `-00:45:00`.
//!
//! A property-bag `offset` field always uses `MatchExactly`, never fuzzy
//! matching, regardless of its own spelling.
//!
//! Before this fix, `temporal_interpret_offset` had no `MatchMinutes` branch
//! at all -- every offset comparison was effectively `MatchExactly`, so
//! `-00:44`/`-00:45`-style strings for a sub-minute-offset zone spuriously
//! threw `RangeError: the given offset does not match the time zone`.

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

fn assert_true(source: &str) {
    assert_eq!(evaluate(source), Value::Bool(true), "{source}");
}

fn assert_range_error(source: &str) {
    let program = compile(&parse(source).unwrap()).unwrap();
    match Vm::default().execute(&program) {
        Err(RuntimeError::RangeError(_)) => {}
        other => panic!("{source}\n  -> expected RangeError, got {other:?}"),
    }
}

const MONROVIA_SETUP: &str = r#"
    const expectedNanoseconds = BigInt((44 * 60 + 30) * 1e9);
    const instance = new Temporal.ZonedDateTime(expectedNanoseconds, "Africa/Monrovia");
"#;

/// The exact wall-clock offset (`-00:44:30`), spelled with seconds, always
/// matches exactly.
#[test]
fn equals_accepts_the_exact_unrounded_sub_minute_offset() {
    assert_true(&format!(
        "{MONROVIA_SETUP}\n instance.equals(\"1970-01-01T00:00:00-00:44:30[Africa/Monrovia]\")"
    ));
}

/// A minute-precision offset (no seconds spelled at all) fuzzy-matches the
/// real offset once rounded to the nearest minute.
#[test]
fn equals_accepts_a_minute_precision_offset_via_rounding() {
    assert_true(&format!(
        "{MONROVIA_SETUP}\n instance.equals(\"1970-01-01T00:00:00-00:45[Africa/Monrovia]\")"
    ));
}

/// A *wrong* sub-minute (seconds-spelled) offset never fuzzy-matches, even
/// though it's close to the real value.
#[test]
fn equals_rejects_a_wrong_seconds_spelled_offset() {
    assert_range_error(&format!(
        "{MONROVIA_SETUP}\n instance.equals(\"1970-01-01T00:00:00-00:44:40[Africa/Monrovia]\")"
    ));
}

/// A seconds-spelled offset that happens to equal the *rounded* real offset
/// is still `MatchExactly` (its own spelling opts out of fuzzy matching), so
/// it does not match the true, unrounded real offset either.
#[test]
fn equals_rejects_a_seconds_spelled_offset_matching_only_the_rounded_value() {
    assert_range_error(&format!(
        "{MONROVIA_SETUP}\n instance.equals(\"1970-01-01T00:00:00-00:45:00[Africa/Monrovia]\")"
    ));
}

/// A property-bag `offset` field never gets fuzzy `MatchMinutes` treatment,
/// regardless of its own spelling.
#[test]
fn equals_rejects_fuzzy_offset_in_a_property_bag() {
    assert_range_error(&format!(
        "{MONROVIA_SETUP}
        instance.equals({{
            offset: \"-00:45\", year: 1970, month: 1, day: 1, minute: 44, second: 30,
            timeZone: \"Africa/Monrovia\",
        }})"
    ));
}

/// The same `MatchMinutes` behaviour applies to `Temporal.ZonedDateTime.from`
/// (`intl402/.../from/zoneddatetime-sub-minute-offset.js`) and
/// `Temporal.ZonedDateTime.compare`
/// (`intl402/.../compare/sub-minute-offset.js`), since both reuse the same
/// shared `temporal_interpret_offset`/`temporal_to_zoned_date_time` path.
#[test]
fn from_and_compare_also_accept_the_rounded_minute_precision_offset() {
    assert_true(
        r#"
        const zdt = Temporal.ZonedDateTime.from("1970-01-01T00:00:00-00:45[Africa/Monrovia]");
        zdt.epochNanoseconds === BigInt((44 * 60 + 30) * 1e9)
    "#,
    );
    assert_true(
        r#"
        const a = new Temporal.ZonedDateTime(BigInt((44 * 60 + 30) * 1e9), "Africa/Monrovia");
        Temporal.ZonedDateTime.compare(
            a, "1970-01-01T00:00:00-00:45[Africa/Monrovia]"
        ) === 0
    "#,
    );
}
