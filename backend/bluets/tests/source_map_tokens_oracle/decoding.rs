// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Source Map v3 decoder and UTF-16 generated-token lookup for K.11.

use super::{Mapping, TokenMapping};
use blueice_bluets::{lex, TokenKind};
use serde_json::Value;
use std::collections::BTreeSet;

pub(super) fn source_position(text: &str, offset: usize) -> (i64, i64) {
    let prefix = &text[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() as i64;
    let column = prefix.rsplit('\n').next().unwrap().encode_utf16().count() as i64;
    (line, column)
}

pub(super) fn mapped_source_at(
    javascript: &str,
    mappings: &BTreeSet<Mapping>,
    offset: usize,
) -> Option<(i64, i64, i64, Option<i64>)> {
    let (line, column) = source_position(javascript, offset);
    mappings
        .iter()
        .rfind(|mapping| {
            mapping.generated_line == line as usize && mapping.generated_column <= column
        })
        .filter(|mapping| mapping.source_index >= 0)
        .map(|mapping| {
            (
                mapping.source_index,
                mapping.source_line,
                mapping.source_column,
                mapping.name_index,
            )
        })
}

pub(super) fn decode_map(map: &Value) -> BTreeSet<Mapping> {
    assert_eq!(map["version"], 3, "Source Map v3 required");
    let sources = map["sources"].as_array().expect("sources array");
    let names = map["names"].as_array().expect("names array");
    assert!(
        sources.iter().all(Value::is_string),
        "string sources required"
    );
    assert!(names.iter().all(Value::is_string), "string names required");
    let source_lines = map.get("sourcesContent").map(|content| {
        let content = content.as_array().expect("sourcesContent array");
        assert_eq!(
            content.len(),
            sources.len(),
            "sourcesContent must have one entry per source"
        );
        content
            .iter()
            .map(|entry| {
                assert!(
                    entry.is_null() || entry.is_string(),
                    "source text or null required"
                );
                entry.as_str().map(|source| {
                    source
                        .split('\n')
                        .map(|line| line.encode_utf16().count() as i64)
                        .collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>()
    });
    let mappings = decode_mappings(map["mappings"].as_str().expect("mappings string"));
    for mapping in &mappings {
        if mapping.source_index >= 0 {
            assert!(
                mapping.source_index < sources.len() as i64,
                "source index out of bounds"
            );
            if let Some(Some(lines)) = source_lines
                .as_ref()
                .and_then(|content| content.get(mapping.source_index as usize))
            {
                let width = lines
                    .get(mapping.source_line as usize)
                    .expect("original source line out of bounds");
                assert!(
                    mapping.source_column <= *width,
                    "original source column {} exceeds UTF-16 line width {width}",
                    mapping.source_column
                );
            }
        }
        if let Some(name_index) = mapping.name_index {
            assert!(name_index < names.len() as i64, "name index out of bounds");
        }
    }
    mappings
}

pub(super) fn decode_mappings(encoded: &str) -> BTreeSet<Mapping> {
    let mut mappings = BTreeSet::new();
    let mut source_index = 0i64;
    let mut source_line = 0i64;
    let mut source_column = 0i64;
    let mut name_index = 0i64;
    for (generated_line, line) in encoded.split(';').enumerate() {
        if line.is_empty() {
            continue;
        }
        let mut generated_column = 0i64;
        let mut previous_column = None;
        for segment in line.split(',') {
            assert!(!segment.is_empty(), "empty source-map segment");
            let fields = decode_vlq(segment);
            assert!(
                matches!(fields.len(), 1 | 4 | 5),
                "invalid segment: {segment}"
            );
            assert!(fields[0] >= 0, "negative generated-column delta");
            generated_column = generated_column
                .checked_add(fields[0])
                .expect("generated column overflow");
            if let Some(previous) = previous_column {
                assert!(
                    generated_column > previous,
                    "generated columns must increase"
                );
            }
            previous_column = Some(generated_column);
            if fields.len() == 1 {
                assert!(mappings.insert(Mapping {
                    generated_line,
                    generated_column,
                    source_index: -1,
                    source_line: -1,
                    source_column: -1,
                    name_index: None,
                }));
                continue;
            }
            source_index = source_index
                .checked_add(fields[1])
                .expect("source index overflow");
            source_line = source_line
                .checked_add(fields[2])
                .expect("source line overflow");
            source_column = source_column
                .checked_add(fields[3])
                .expect("source column overflow");
            assert!(
                source_index >= 0 && source_line >= 0 && source_column >= 0,
                "negative original source coordinate"
            );
            let name = fields.get(4).map(|delta| {
                name_index = name_index.checked_add(*delta).expect("name index overflow");
                assert!(name_index >= 0, "negative name index");
                name_index
            });
            assert!(mappings.insert(Mapping {
                generated_line,
                generated_column,
                source_index,
                source_line,
                source_column,
                name_index: name,
            }));
        }
    }
    mappings
}

/// Align complete non-trivia emitted tokens, with positions in UTF-16 code
/// units. The public lexer groups quoted strings, templates and multi-character
/// operators rather than mistaking their internals for separate source sites.
pub(super) fn mapped_tokens(javascript: &str, mappings: &BTreeSet<Mapping>) -> Vec<TokenMapping> {
    validate_generated_bounds(javascript, mappings);
    lex("<emitted.js>", javascript)
        .unwrap_or_else(|diagnostics| panic!("emitted JavaScript lexer failed: {diagnostics:#?}"))
        .into_iter()
        .filter(|token| token.kind != TokenKind::Eof)
        .map(|token| {
            let (line, column) = source_position(javascript, token.start);
            let mapping = mappings.iter().rfind(|mapping| {
                mapping.generated_line == line as usize && mapping.generated_column <= column
            });
            TokenMapping {
                token: token.text,
                source_index: mapping.map_or(-1, |mapping| mapping.source_index),
                source_line: mapping.map_or(-1, |mapping| mapping.source_line),
                source_column: mapping.map_or(-1, |mapping| mapping.source_column),
                name_index: mapping.and_then(|mapping| mapping.name_index),
            }
        })
        .collect()
}

pub(super) fn validate_generated_bounds(javascript: &str, mappings: &BTreeSet<Mapping>) {
    let line_widths = javascript
        .split('\n')
        .map(|line| line.encode_utf16().count() as i64)
        .collect::<Vec<_>>();
    for mapping in mappings {
        let width = line_widths
            .get(mapping.generated_line)
            .expect("source-map generated line out of bounds");
        assert!(
            mapping.generated_column <= *width,
            "source-map generated column {} exceeds line {} width {width}",
            mapping.generated_column,
            mapping.generated_line
        );
    }
}

pub(super) fn decode_vlq(encoded: &str) -> Vec<i64> {
    let mut fields = Vec::new();
    let mut value = 0u64;
    let mut shift = 0u32;
    for byte in encoded.bytes() {
        let digit = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => panic!("invalid base64 VLQ digit"),
        };
        let chunk = u64::from(digit & 0b1_1111);
        assert!(
            shift < 64 && chunk <= u64::MAX >> shift,
            "VLQ field overflow"
        );
        value |= chunk << shift;
        shift += 5;
        if digit & 0b10_0000 == 0 {
            let magnitude = value >> 1;
            assert!(magnitude <= i64::MAX as u64, "VLQ magnitude overflow");
            let magnitude = magnitude as i64;
            fields.push(if value & 1 == 1 {
                -magnitude
            } else {
                magnitude
            });
            value = 0;
            shift = 0;
        }
    }
    assert_eq!(shift, 0, "unterminated VLQ field");
    fields
}
