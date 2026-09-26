// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Explicit host setup, never installed in an ordinary realm. Test262 permits
//! overriding harness functions; failures remain distinct from engine errors.
use super::*;

mod assertions;
mod cases;
mod foreign;
mod harness;
mod reverse;
/// The four numbering-system digit sets selected by the pinned precision
/// matrix. These are the same `numberingSystemDigits` entries consumed by its
/// upstream JavaScript helper.
fn test262_numbering_system_digits(numbering_system: &str) -> Option<&'static str> {
    match numbering_system {
        "arab" => Some("٠١٢٣٤٥٦٧٨٩"),
        "latn" => Some("0123456789"),
        "thai" => Some("๐๑๒๓๔๕๖๗๘๙"),
        "hanidec" => Some("〇一二三四五六七八九"),
        _ => None,
    }
}

/// Splits the two digit runs from upstream `testNumberFormat`'s `1.1` probe.
fn test262_number_format_pattern_parts(
    formatted: &str,
    digits: &str,
) -> Option<(String, String, String)> {
    let is_digit = |character: char| digits.contains(character);
    let mut runs = Vec::with_capacity(2);
    let mut start = None;
    for (index, character) in formatted.char_indices() {
        if is_digit(character) {
            start.get_or_insert(index);
        } else if let Some(start) = start.take() {
            runs.push((start, index));
        }
    }
    if let Some(start) = start {
        runs.push((start, formatted.len()));
    }
    let [(first_start, first_end), (second_start, second_end)] = runs.as_slice() else {
        return None;
    };
    Some((
        formatted[..*first_start].into(),
        formatted[*first_end..*second_start].into(),
        formatted[*second_end..].into(),
    ))
}

/// Builds the expected localized decimal from the fixture's Western-digit
/// result and a positive or negative formatting pattern probe.
fn test262_localize_number(raw: &str, digits: &str, pattern: &(String, String, String)) -> String {
    let digits: Vec<_> = digits.chars().collect();
    let localize = |value: &str| {
        value
            .chars()
            .flat_map(|character| match character.to_digit(10) {
                Some(digit) => digits[digit as usize]
                    .to_string()
                    .chars()
                    .collect::<Vec<_>>(),
                None => vec![character],
            })
            .collect::<String>()
    };
    let mut result = pattern.0.clone();
    if let Some((integer, fraction)) = raw.split_once('.') {
        result.push_str(&localize(integer));
        result.push_str(&pattern.1);
        result.push_str(&localize(fraction));
    } else {
        result.push_str(&localize(raw));
    }
    result.push_str(&pattern.2);
    result
}

#[cfg(test)]
mod tests {
    use super::test262_number_format_pattern_parts;

    #[test]
    fn number_format_pattern_accepts_a_trailing_affix() {
        assert_eq!(
            test262_number_format_pattern_parts("1.1+", "0123456789"),
            Some((String::new(), ".".into(), "+".into()))
        );
    }
}
