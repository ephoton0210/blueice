// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Decode source spelling without discarding unpaired UTF-16 units.

pub(super) fn parse(source: &str) -> Option<Vec<u16>> {
    let quote = source.chars().next()?;
    if !matches!(quote, '\'' | '"' | '`') || !source.ends_with(quote) || source.len() < 2 {
        return None;
    }
    let mut input = source[1..source.len() - 1].chars().peekable();
    let mut result = Vec::new();
    while let Some(character) = input.next() {
        if character != '\\' {
            if character == quote || quote != '`' && matches!(character, '\r' | '\n') {
                return None;
            }
            if quote == '`' && character == '\r' {
                if input.peek() == Some(&'\n') {
                    input.next();
                }
                result.push(10);
                continue;
            }
            result.extend(character.encode_utf16(&mut [0; 2]).iter().copied());
            continue;
        }
        match input.next()? {
            'b' => result.push(8),
            't' => result.push(9),
            'n' => result.push(10),
            'v' => result.push(11),
            'f' => result.push(12),
            'r' => result.push(13),
            '0' => {
                if input.peek().is_some_and(char::is_ascii_digit) {
                    return None;
                }
                result.push(0);
            }
            'x' => result.push(hex(&mut input, 2)? as u16),
            'u' => {
                let code = if input.peek() == Some(&'{') {
                    input.next();
                    let mut value = 0u32;
                    let mut count = 0;
                    loop {
                        let digit = input.next()?;
                        if digit == '}' {
                            break;
                        }
                        count += 1;
                        value = value.checked_mul(16)?.checked_add(digit.to_digit(16)?)?;
                    }
                    if count == 0 || value > 0x10ffff {
                        return None;
                    }
                    value
                } else {
                    hex(&mut input, 4)?
                };
                if code <= 0xffff {
                    result.push(code as u16);
                } else {
                    result.push(((code - 0x10000) / 0x400 + 0xd800) as u16);
                    result.push(((code - 0x10000) % 0x400 + 0xdc00) as u16);
                }
            }
            '\r' => {
                if input.peek() == Some(&'\n') {
                    input.next();
                }
            }
            '\n' | '\u{2028}' | '\u{2029}' => {}
            '1'..='9' => return None,
            other => result.extend(other.encode_utf16(&mut [0; 2]).iter().copied()),
        }
    }
    Some(result)
}

fn hex(input: &mut impl Iterator<Item = char>, count: usize) -> Option<u32> {
    let mut result = 0u32;
    for _ in 0..count {
        result = result * 16 + input.next()?.to_digit(16)?;
    }
    Some(result)
}
