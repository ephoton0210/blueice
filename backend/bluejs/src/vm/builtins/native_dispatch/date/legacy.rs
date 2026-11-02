// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The date formats `Date.parse` reads beyond ISO 8601.
//!
//! ECMA-262 leaves them to the implementation, but every engine reads the same family and
//! pages depend on it: `11/2/2026, 5:06:22 PM` (what `toLocaleString` prints, and what dayjs and
//! moment hand back to `new Date` to move between time zones), `Nov 2, 2026`, `2026/11/02`,
//! `Mon Nov 02 2026 17:06:22 GMT+0800 (CST)`. The reading is token based, like V8's: numbers
//! followed by a colon are the time, a month name settles which numbers are the day and the
//! year, and otherwise three numbers are month/day/year (or year/month/day when the first has
//! three digits or more). Nothing is guessed that the text does not say: a date with no year,
//! a time with no date, an unknown word, or a field out of range is not a date.

use super::date_from_parts;

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number { value: i128, digits: usize },
    Word(String),
    Symbol(char),
}

fn tokens(source: &str) -> Option<Vec<Token>> {
    let chars: Vec<char> = source.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '(' {
            // A comment, nesting allowed: `(Coordinated Universal Time)`.
            let mut depth = 0;
            while i < chars.len() {
                match chars[i] {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            i += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            if depth != 0 {
                return None;
            }
        } else if c.is_ascii_digit() {
            let start = i;
            let mut value: i128 = 0;
            while i < chars.len() && chars[i].is_ascii_digit() {
                value = value.checked_mul(10)?.checked_add(i128::from(chars[i] as u8 - b'0'))?;
                if value > 1_000_000_000_000 {
                    return None;
                }
                i += 1;
            }
            out.push(Token::Number {
                value,
                digits: i - start,
            });
        } else if c.is_ascii_alphabetic() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_alphabetic() {
                i += 1;
            }
            out.push(Token::Word(
                chars[start..i].iter().collect::<String>().to_ascii_lowercase(),
            ));
        } else if matches!(c, ':' | '/' | '-' | '+' | ',' | '.') {
            out.push(Token::Symbol(c));
            i += 1;
        } else {
            return None;
        }
    }
    Some(out)
}

fn month_of(word: &str) -> Option<i128> {
    const MONTHS: [&str; 12] = [
        "january", "february", "march", "april", "may", "june", "july", "august", "september",
        "october", "november", "december",
    ];
    if word.len() < 3 {
        return None;
    }
    MONTHS
        .iter()
        .position(|name| name.starts_with(word))
        .map(|index| index as i128 + 1)
}

fn is_weekday(word: &str) -> bool {
    const DAYS: [&str; 7] = [
        "sunday", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday",
    ];
    word.len() >= 3 && DAYS.iter().any(|name| name.starts_with(word))
}

/// A time zone written as a word, as minutes east of UTC.
fn zone_of(word: &str) -> Option<i128> {
    Some(match word {
        "gmt" | "utc" | "ut" | "z" => 0,
        "est" => -300,
        "edt" => -240,
        "cst" => -360,
        "cdt" => -300,
        "mst" => -420,
        "mdt" => -360,
        "pst" => -480,
        "pdt" => -420,
        _ => return None,
    })
}

#[derive(Default)]
struct Fields {
    time: Option<(i128, i128, i128, i128)>,
    pm: Option<bool>,
    zone: Option<i128>,
    month: Option<i128>,
    numbers: Vec<(i128, usize)>,
}

pub(super) fn parse_legacy_date(source: &str) -> Option<f64> {
    let source = source.trim();
    // Anything shaped like an ISO date (a leading year then a dash) is only ever read by the
    // ISO parsers: a malformed one is invalid, not a legacy date.
    let mut shape = source.chars().peekable();
    if matches!(shape.peek(), Some('+' | '-')) {
        shape.next();
    }
    let lead: String = shape.by_ref().take_while(|c| c.is_ascii_digit()).collect();
    if lead.len() >= 4 && source.contains('-') && source[source.find(&lead)? + lead.len()..].starts_with('-') {
        return None;
    }
    let tokens = tokens(source)?;
    let mut fields = Fields::default();
    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i] {
            Token::Number { value, .. } if tokens.get(i + 1) == Some(&Token::Symbol(':')) => {
                if fields.time.is_some() {
                    return None;
                }
                let hour = *value;
                let Some(Token::Number { value: minute, .. }) = tokens.get(i + 2) else {
                    return None;
                };
                i += 3;
                let mut second = 0;
                let mut millisecond = 0;
                if tokens.get(i) == Some(&Token::Symbol(':')) {
                    let Some(Token::Number { value, .. }) = tokens.get(i + 1) else {
                        return None;
                    };
                    second = *value;
                    i += 2;
                    if tokens.get(i) == Some(&Token::Symbol('.')) {
                        let Some(Token::Number { value, digits }) = tokens.get(i + 1) else {
                            return None;
                        };
                        // The first three digits of the fraction are the milliseconds.
                        millisecond = match *digits {
                            0 => 0,
                            1 => value * 100,
                            2 => value * 10,
                            3 => *value,
                            n => value / 10i128.pow(n as u32 - 3),
                        };
                        i += 2;
                    }
                }
                fields.time = Some((hour, *minute, second, millisecond));
            }
            Token::Number { value, digits } => {
                fields.numbers.push((*value, *digits));
                i += 1;
            }
            Token::Word(word) => {
                if let Some(month) = month_of(word) {
                    if fields.month.replace(month).is_some() {
                        return None;
                    }
                } else if word == "am" || word == "pm" {
                    if fields.pm.replace(word == "pm").is_some() {
                        return None;
                    }
                } else if let Some(zone) = zone_of(word) {
                    if fields.zone.replace(zone).is_some() {
                        return None;
                    }
                } else if !is_weekday(word) {
                    return None;
                }
                i += 1;
            }
            Token::Symbol(sign @ ('+' | '-')) => {
                // After a time (or a zone word) a sign opens an offset; before one it is the
                // dash of `11-02-2026`.
                let offset_context = fields.time.is_some() || fields.zone.is_some();
                if offset_context {
                    let Some(Token::Number { value, digits }) = tokens.get(i + 1) else {
                        return None;
                    };
                    i += 2;
                    let minutes = if tokens.get(i) == Some(&Token::Symbol(':')) {
                        let Some(Token::Number { value: minute, .. }) = tokens.get(i + 1) else {
                            return None;
                        };
                        i += 2;
                        *value * 60 + *minute
                    } else if *digits <= 2 {
                        *value * 60
                    } else {
                        (*value / 100) * 60 + *value % 100
                    };
                    let minutes = if *sign == '-' { -minutes } else { minutes };
                    // `GMT+0800` replaces the zone word's zero; two numeric zones are nonsense.
                    if fields.zone.is_some_and(|z| z != 0) || minutes.abs() > 24 * 60 {
                        return None;
                    }
                    fields.zone = Some(minutes);
                } else {
                    i += 1;
                }
            }
            Token::Symbol(_) => i += 1,
        }
    }

    let (year, month, day) = assemble(&fields)?;
    let (mut hour, minute, second, millisecond) = fields.time.unwrap_or((0, 0, 0, 0));
    if let Some(pm) = fields.pm {
        if fields.time.is_none() || !(1..=12).contains(&hour) {
            return None;
        }
        hour = hour % 12 + if pm { 12 } else { 0 };
    }
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let local = date_from_parts(year, month - 1, day, hour, minute, second, millisecond);
    if local.is_nan() {
        return None;
    }
    let value = local - (fields.zone.unwrap_or(0) * 60_000) as f64;
    // Re-clip: an offset can carry a date at the edge of the range out of it.
    let clipped = super::time_clip(value);
    (!clipped.is_nan()).then_some(clipped)
}

/// `(year, month, day)` from the numbers and the month name, if the text settles all three.
fn assemble(fields: &Fields) -> Option<(i128, i128, i128)> {
    let two_digit_year = |(value, digits): (i128, usize)| {
        if digits <= 2 {
            if value < 50 {
                value + 2000
            } else {
                value + 1900
            }
        } else {
            value
        }
    };
    let valid = |year: i128, month: i128, day: i128| {
        ((1..=12).contains(&month) && (1..=31).contains(&day)).then_some((year, month, day))
    };
    let numbers = &fields.numbers;
    if let Some(month) = fields.month {
        // A month name: of the other numbers one is the day and one the year, the year being
        // the one that cannot be a day.
        return match numbers.as_slice() {
            [a, b] => {
                let a_is_year = a.1 >= 3 || a.0 > 31;
                let b_is_year = b.1 >= 3 || b.0 > 31;
                match (a_is_year, b_is_year) {
                    (true, false) => valid(two_digit_year(*a), month, b.0),
                    (false, true) => valid(two_digit_year(*b), month, a.0),
                    (false, false) => valid(two_digit_year(*b), month, a.0),
                    (true, true) => None,
                }
            }
            _ => None,
        };
    }
    match numbers.as_slice() {
        [a, b, c] => {
            if a.1 >= 3 || a.0 > 31 {
                valid(two_digit_year(*a), b.0, c.0)
            } else {
                valid(two_digit_year(*c), a.0, b.0)
            }
        }
        _ => None,
    }
}
