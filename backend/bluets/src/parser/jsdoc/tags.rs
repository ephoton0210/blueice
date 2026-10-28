// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Comment ranges come only from lexical trivia, outside strings/templates.

use super::*;
use std::ops::Range;

pub(super) struct Tag {
    pub start: usize,
    pub end: usize,
    pub kind: String,
    pub annotation: Option<Range<usize>>,
    pub name: String,
    pub name_start: usize,
    pub optional: bool,
}

pub(super) fn comments(id: &str, source: &str) -> Result<Vec<Range<usize>>, Vec<Diagnostic>> {
    let tokens = lex(id, source)?;
    let mut result = Vec::new();
    let mut start = 0;
    for token in tokens {
        let mut cursor = start;
        while cursor < token.start {
            let trivia = &source[cursor..token.start];
            if trivia.starts_with("//") {
                cursor += trivia.find('\n').unwrap_or(trivia.len());
            } else if trivia.starts_with("/*") {
                let end = cursor + trivia.find("*/").map_or(trivia.len(), |end| end + 2);
                if trivia.starts_with("/**") {
                    result.push(cursor..end);
                }
                cursor = end;
            } else {
                cursor += trivia.chars().next().map_or(1, char::len_utf8);
            }
        }
        start = token.end;
    }
    Ok(result)
}

pub(super) fn parse(source: &str, range: Range<usize>) -> Vec<Tag> {
    let bytes = source.as_bytes();
    let mut cursor = range.start + 3;
    let end = range.end.saturating_sub(2);
    let mut result = Vec::new();
    while cursor < end {
        if bytes[cursor] != b'@' {
            cursor += 1;
            continue;
        }
        cursor += 1;
        let start = cursor;
        while cursor < end && bytes[cursor].is_ascii_alphabetic() {
            cursor += 1;
        }
        let kind = &source[start..cursor];
        if ![
            "param", "returns", "return", "type", "typedef", "property", "template",
        ]
        .contains(&kind)
        {
            continue;
        }
        while cursor < end && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        let annotation = if cursor < end && bytes[cursor] == b'{' {
            cursor += 1;
            let start = cursor;
            let mut depth = 1;
            let mut quote = None;
            while cursor < end && depth > 0 {
                let byte = bytes[cursor];
                if let Some(delimiter) = quote {
                    if byte == b'\\' {
                        cursor += 1;
                    } else if byte == delimiter {
                        quote = None;
                    }
                } else if byte == b'\'' || byte == b'"' {
                    quote = Some(byte);
                } else if byte == b'{' {
                    depth += 1;
                } else if byte == b'}' {
                    depth -= 1;
                }
                cursor += 1;
            }
            Some(start..cursor.saturating_sub(1))
        } else {
            None
        };
        while cursor < end && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        let optional = cursor < end && bytes[cursor] == b'[';
        if optional {
            cursor += 1;
        }
        let name_start = cursor;
        while cursor < end
            && !bytes[cursor].is_ascii_whitespace()
            && ![b']', b'=', b'*', b'@'].contains(&bytes[cursor])
        {
            cursor += 1;
        }
        result.push(Tag {
            start: start - 1,
            end: cursor + usize::from(optional),
            kind: kind.into(),
            annotation,
            name: source[name_start..cursor].into(),
            name_start,
            optional,
        });
    }
    result
}
