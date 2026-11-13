// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Generated literal spelling and the final Native-compatible UTF-8 boundary.

pub(super) fn literal(units: &[u16]) -> String {
    render(units, false)
}

pub(super) fn template_chunk(units: &[u16]) -> String {
    render(units, true)
}

fn render(units: &[u16], template: bool) -> String {
    let mut token = Vec::new();
    if !template {
        token.push(u16::from(b'"'));
    }
    for (index, &unit) in units.iter().enumerate() {
        let short = match unit {
            0 if units
                .get(index + 1)
                .is_some_and(|unit| (0x30..=0x39).contains(unit)) =>
            {
                Some("\\x00")
            }
            0 => Some("\\0"),
            8 => Some("\\b"),
            9 => Some("\\t"),
            10 => Some("\\n"),
            11 => Some("\\v"),
            12 => Some("\\f"),
            13 => Some("\\r"),
            0x22 if !template => Some("\\\""),
            0x60 if template => Some("\\`"),
            0x24 if template && units.get(index + 1) == Some(&0x7b) => Some("\\$"),
            0x5c => Some("\\\\"),
            _ => None,
        };
        if let Some(short) = short {
            token.extend(short.encode_utf16());
        } else if unit < 0x20 || matches!(unit, 0x0085 | 0x2028 | 0x2029) {
            token.extend(format!("\\u{unit:04X}").encode_utf16());
        } else {
            token.push(unit);
        }
    }
    if !template {
        token.push(u16::from(b'"'));
    }
    String::from_utf16_lossy(&token)
}
