// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! JSONC preprocessing preserves quoted text and lets serde validate JSON.

pub(super) fn parse(text: &str) -> Result<serde_json::Value, String> {
    let mut bytes = text.trim_start_matches('\u{feff}').as_bytes().to_vec();
    let mut index = 0;
    let mut quoted = false;
    while index < bytes.len() {
        if bytes[index] == b'"' {
            quoted = !quoted;
        } else if quoted && bytes[index] == b'\\' {
            index += 1;
        } else if !quoted && bytes.get(index..index + 2) == Some(b"//") {
            while index < bytes.len() && !matches!(bytes[index], b'\r' | b'\n') {
                bytes[index] = b' ';
                index += 1;
            }
            continue;
        } else if !quoted && bytes.get(index..index + 2) == Some(b"/*") {
            bytes[index..index + 2].fill(b' ');
            index += 2;
            while index + 1 < bytes.len() && bytes.get(index..index + 2) != Some(b"*/") {
                if !matches!(bytes[index], b'\r' | b'\n') {
                    bytes[index] = b' ';
                }
                index += 1;
            }
            if index + 1 >= bytes.len() {
                return Err("unterminated JSONC block comment".to_string());
            }
            bytes[index..index + 2].fill(b' ');
            index += 2;
            continue;
        }
        index += 1;
    }
    quoted = false;
    index = 0;
    while index < bytes.len() {
        if bytes[index] == b'"' {
            quoted = !quoted;
        } else if quoted && bytes[index] == b'\\' {
            index += 1;
        } else if !quoted && bytes[index] == b',' {
            let next = bytes[index + 1..]
                .iter()
                .find(|byte| !byte.is_ascii_whitespace());
            if matches!(next, Some(b'}' | b']')) {
                bytes[index] = b' ';
            }
        }
        index += 1;
    }
    serde_json::from_slice(&bytes).map_err(|error| format!("invalid JSONC: {error}"))
}
