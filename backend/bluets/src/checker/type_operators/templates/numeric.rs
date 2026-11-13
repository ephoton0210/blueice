// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Placeholder validation follows string conversion, not numeric source syntax.

pub(super) fn number(units: &[u16]) -> Option<f64> {
    let text = String::from_utf16(units).ok()?;
    if text.is_empty() {
        return None;
    }
    let text = text.trim_matches(|c| {
        matches!(c,
        '\u{0009}'..='\u{000D}' | '\u{0020}' | '\u{00A0}' | '\u{1680}' |
        '\u{2000}'..='\u{200A}' | '\u{2028}' | '\u{2029}' | '\u{202F}' |
        '\u{205F}' | '\u{3000}' | '\u{FEFF}')
    });
    if text.is_empty() {
        return Some(0.0);
    }
    for (prefix, radix) in [("0x", 16), ("0o", 8), ("0b", 2)] {
        if text
            .get(..2)
            .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
        {
            let digits = &text[2..];
            if digits.is_empty() {
                return None;
            }
            let mut number = 0.0;
            for digit in digits.chars() {
                number = number * f64::from(radix) + f64::from(digit.to_digit(radix)?);
            }
            return number.is_finite().then_some(number);
        }
    }
    text.parse::<f64>().ok().filter(|number| number.is_finite())
}

pub(super) fn bigint(units: &[u16]) -> bool {
    let Ok(text) = String::from_utf16(units) else {
        return false;
    };
    let text = text.strip_prefix('-').unwrap_or(&text);
    for (prefix, radix) in [("0x", 16), ("0o", 8), ("0b", 2)] {
        if text
            .get(..2)
            .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
        {
            let digits = &text[2..];
            return !digits.is_empty() && digits.chars().all(|digit| digit.is_digit(radix));
        }
    }
    text == "0"
        || text
            .as_bytes()
            .first()
            .is_some_and(|digit| (b'1'..=b'9').contains(digit))
            && text.bytes().all(|digit| digit.is_ascii_digit())
}
