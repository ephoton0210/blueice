// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Independently implemented package version-range selection for pinned 5.9.3.

type Version = (u32, u32, u32);
const COMPILER: Version = (5, 9, 3);

pub(super) fn matches(range: &str) -> bool {
    range.split("||").any(|part| group(part).unwrap_or(false))
}

fn group(value: &str) -> Option<bool> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if let Some((left, right)) = value.split_once(" - ") {
        let (low, _) = version(left.trim())?;
        let (high, count) = version(right.trim())?;
        return Some(
            COMPILER >= low
                && if count < 3 {
                    COMPILER < upper(high, count)?
                } else {
                    COMPILER <= high
                },
        );
    }
    let mut tokens = value.split_whitespace();
    let mut matched = true;
    while let Some(token) = tokens.next() {
        let combined;
        let token = if matches!(token, ">" | ">=" | "<" | "<=" | "=" | "~" | "^") {
            combined = format!("{token}{}", tokens.next()?);
            combined.as_str()
        } else {
            token
        };
        matched &= comparator(token)?;
    }
    Some(matched)
}

fn comparator(value: &str) -> Option<bool> {
    let (operator, text) = [">=", "<=", ">", "<", "=", "~", "^"]
        .into_iter()
        .find_map(|operator| value.strip_prefix(operator).map(|text| (operator, text)))
        .unwrap_or(("", value));
    let (low, count) = version(text.trim_start_matches('v'))?;
    if count == 0 {
        return Some(true);
    }
    Some(match operator {
        ">=" => COMPILER >= low,
        ">" if count < 3 => COMPILER >= upper(low, count)?,
        ">" => COMPILER > low,
        "<" => COMPILER < low,
        "<=" if count < 3 => COMPILER < upper(low, count)?,
        "<=" => COMPILER <= low,
        "~" => COMPILER >= low && COMPILER < upper(low, count.min(2))?,
        "^" => {
            let bound = if low.0 > 0 || count == 1 {
                1
            } else if low.1 > 0 || count == 2 {
                2
            } else {
                3
            };
            COMPILER >= low && COMPILER < upper(low, bound)?
        }
        "" | "=" if count < 3 => COMPILER >= low && COMPILER < upper(low, count)?,
        "" | "=" => COMPILER == low,
        _ => return None,
    })
}

fn version(value: &str) -> Option<(Version, usize)> {
    let mut result = [0; 3];
    let mut count = 0;
    let mut wildcard = false;
    for (index, part) in value.split('.').enumerate() {
        if index >= 3 {
            return None;
        }
        if matches!(part, "*" | "x" | "X") {
            wildcard = true;
        } else {
            if wildcard || part.is_empty() {
                return None;
            }
            result[index] = part.parse().ok()?;
            count += 1;
        }
    }
    Some(((result[0], result[1], result[2]), count))
}

fn upper(value: Version, count: usize) -> Option<Version> {
    match count {
        1 => Some((value.0.checked_add(1)?, 0, 0)),
        2 => Some((value.0, value.1.checked_add(1)?, 0)),
        3 => Some((value.0, value.1, value.2.checked_add(1)?)),
        _ => None,
    }
}
