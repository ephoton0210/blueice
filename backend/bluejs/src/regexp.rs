// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::{JsString, RuntimeError};

pub(crate) struct RegExp {
    pub source: JsString,
    pub flags: String,
    pub matcher: regress::Regex,
    pub capture_names: Vec<(String, usize)>,
}

impl RegExp {
    pub fn compile(source: JsString, flags: &JsString) -> Result<Self, RuntimeError> {
        let mut seen = Vec::new();
        for &flag in flags.as_code_units() {
            if !b"dgimsuvy".iter().any(|&f| u16::from(f) == flag) || seen.contains(&flag) {
                return Err(RuntimeError::SyntaxError("invalid RegExp flags".into()));
            }
            seen.push(flag);
        }
        if seen.contains(&u16::from(b'u')) && seen.contains(&u16::from(b'v')) {
            return Err(RuntimeError::SyntaxError("RegExp flags u and v are mutually exclusive".into()));
        }
        seen.sort_unstable();
        let flags: String = seen.iter().map(|&c| char::from_u32(c as u32).unwrap()).collect();
        let unicode = flags.contains(['u', 'v']);
        let points: Vec<u32> = if unicode {
            char::decode_utf16(source.as_code_units().iter().copied()).map(|c| c.map_or_else(|e| e.unpaired_surrogate() as u32, |c| c as u32)).collect()
        } else {
            source.as_code_units().iter().map(|&c| c as u32).collect()
        };
        let matcher = regress::Regex::from_unicode(points.into_iter(), regress::Flags::from(flags.as_str())).map_err(|e| RuntimeError::SyntaxError(e.to_string()))?;
        let capture_names = capture_names(&source, flags.contains('v'));
        Ok(Self { source, flags, matcher, capture_names })
    }

    pub fn unicode(&self) -> bool {
        self.flags.contains(['u', 'v'])
    }

    pub fn find(&self, string: &JsString, start: usize) -> Option<regress::Match> {
        if self.unicode() {
            self.matcher.find_from_utf16(string.as_code_units(), start).next()
        } else {
            self.matcher.find_from_ucs2(string.as_code_units(), start).next()
        }
    }
}

// The matcher exposes named values but not their capture numbers. Retain that
// metadata so indices.groups aliases the corresponding numbered pair, including
// nested groups with identical ranges and duplicate names in alternatives.
// This scan runs only after the matcher has validated the complete pattern.
fn capture_names(source: &JsString, unicode_sets: bool) -> Vec<(String, usize)> {
    let units = source.as_code_units();
    let mut names = Vec::new();
    let (mut index, mut depth, mut capture) = (0, 0, 0);
    while index < units.len() {
        let unit = units[index];
        index += 1;
        match unit {
            0x5c => index += 1,
            0x5b if depth == 0 || unicode_sets => depth += 1,
            0x5d if depth > 0 => depth -= 1,
            0x28 if depth == 0 => {
                if units.get(index) != Some(&0x3f) {
                    capture += 1;
                } else if units.get(index + 1) == Some(&0x3c) && !matches!(units.get(index + 2), Some(0x3d | 0x21)) {
                    capture += 1;
                    index += 2;
                    let mut name = JsString::default();
                    while units[index] != 0x3e {
                        if units[index] == 0x5c {
                            index += 2; // validated Unicode escape, \\uXXXX or \\u{X}
                            let braced = units[index] == 0x7b;
                            index += usize::from(braced);
                            let end = if braced { index + units[index..].iter().position(|c| *c == 0x7d).unwrap() } else { index + 4 };
                            let point = units[index..end].iter().fold(0, |n, c| n * 16 + char::from_u32(*c as u32).unwrap().to_digit(16).unwrap());
                            name.push_code_point(point);
                            index = end + usize::from(braced);
                        } else {
                            name.push_code_point(units[index] as u32);
                            index += 1;
                        }
                    }
                    index += 1;
                    names.push((name.to_utf8().expect("validated capture name"), capture));
                }
            }
            _ => {}
        }
    }
    names
}

pub(crate) fn advance(string: &JsString, position: usize, unicode: bool) -> usize {
    let units = string.as_code_units();
    position.saturating_add(
        if unicode && units.get(position).is_some_and(|c| (0xd800..=0xdbff).contains(c)) && units.get(position + 1).is_some_and(|c| (0xdc00..=0xdfff).contains(c)) { 2 } else { 1 },
    )
}
