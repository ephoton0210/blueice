// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The `TemporalDurationString` grammar.

/// Parses the ISO duration strings accepted by `Intl.DurationFormat` and
/// `Temporal.Duration` through Temporal's duration-string grammar. The host
/// service receives a typed, validated ECMA-402 record, so neither this
/// parser nor a Temporal object can trigger observable duration-field
/// accessors while formatting.
///
/// Temporal's grammar, as this implements it (each rule checked against a
/// named fixture in the pinned Test262 corpus rather than assumed — see
/// `built-ins/Temporal/Duration/from/argument-string*.js`):
///
/// - the designators and the `P`/`T` markers are case-insensitive, and an
///   optional leading sign may be `+`, `-` or U+2212 MINUS SIGN;
/// - components must appear in descending order and at most once each;
/// - a decimal fraction (`.` or `,`, one to nine digits) may appear on *any*
///   time component, not only on seconds, but then nothing smaller may
///   follow. `PT0.5H` is 30 minutes, not an error. Its value is carried down
///   through the smaller fields exactly: a fraction of an hour, minute or
///   second is always a whole number of nanoseconds.
pub(crate) fn parse_duration_record(source: &str) -> Option<blueice_ecma402::DurationRecord> {
    let mut characters = source.chars().peekable();
    let sign = match characters.peek() {
        Some('+') => {
            characters.next();
            1_i128
        }
        Some('-' | '\u{2212}') => {
            characters.next();
            -1_i128
        }
        _ => 1_i128,
    };
    matches!(characters.next(), Some('P' | 'p')).then_some(())?;
    let mut values = [0_i128; 10];
    let mut in_time = false;
    let mut saw_component = false;
    let mut saw_time_component = false;
    let mut previous: Option<usize> = None;
    let mut fraction_seen = false;
    while let Some(&character) = characters.peek() {
        // A fractional component must be the last one in the string.
        if fraction_seen {
            return None;
        }
        if matches!(character, 'T' | 't') {
            if in_time {
                return None;
            }
            characters.next();
            in_time = true;
            previous = None;
            continue;
        }
        let mut number = String::new();
        while characters
            .peek()
            .is_some_and(|character| character.is_ascii_digit())
        {
            number.push(characters.next()?);
        }
        if number.is_empty() {
            return None;
        }
        let mut fraction = None;
        if matches!(characters.peek(), Some('.' | ',')) {
            characters.next();
            let mut digits = String::new();
            while characters
                .peek()
                .is_some_and(|character| character.is_ascii_digit())
            {
                digits.push(characters.next()?);
            }
            if digits.is_empty() || digits.len() > 9 {
                return None;
            }
            fraction = Some(digits);
        }
        let index = match (in_time, characters.next()?.to_ascii_uppercase()) {
            (false, 'Y') => 0,
            (false, 'M') => 1,
            (false, 'W') => 2,
            (false, 'D') => 3,
            (true, 'H') => 4,
            (true, 'M') => 5,
            (true, 'S') => 6,
            _ => return None,
        };
        if previous.is_some_and(|previous| index <= previous) {
            return None;
        }
        previous = Some(index);
        saw_component = true;
        saw_time_component |= in_time;
        values[index] = number.parse::<i128>().ok()?;
        if let Some(digits) = fraction {
            // A fraction of an hour, minute or second is an exact whole
            // number of nanoseconds, so this carries down without rounding.
            let unit_nanoseconds: i128 = match index {
                4 => 3_600_000_000_000,
                5 => 60_000_000_000,
                6 => 1_000_000_000,
                _ => return None,
            };
            let scale = 10_i128.pow(digits.len() as u32);
            let mut remaining = digits.parse::<i128>().ok()? * unit_nanoseconds / scale;
            for (slot, unit) in [
                (5_usize, 60_000_000_000_i128),
                (6, 1_000_000_000),
                (7, 1_000_000),
                (8, 1_000),
                (9, 1),
            ] {
                if slot <= index {
                    continue;
                }
                values[slot] = remaining / unit;
                remaining %= unit;
            }
            fraction_seen = true;
        }
    }
    (saw_component && (!in_time || saw_time_component)).then_some(())?;
    blueice_ecma402::DurationRecord::try_new(
        sign * values[0],
        sign * values[1],
        sign * values[2],
        sign * values[3],
        sign * values[4],
        sign * values[5],
        sign * values[6],
        sign * values[7],
        sign * values[8],
        sign * values[9],
    )
    .ok()
}
