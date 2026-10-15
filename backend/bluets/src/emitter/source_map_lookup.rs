// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded, source-text-free lookup in a compiler-produced Source Map v3.

use super::SourceMap;

/// One original position selected by an emitted JavaScript location. The
/// source index refers only to the map's source table; this record contains no
/// source text, module identity or capability to inspect a live program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceMapOriginalPosition {
    pub source_index: usize,
    pub line: usize,
    pub column_utf16: usize,
    pub name_index: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceMapLookupError {
    /// The generated location is outside the supplied JavaScript text.
    QueryOutOfBounds,
    /// A map field, segment order, table index or UTF-16 position is invalid.
    MalformedMap,
    /// Decoding would exceed the caller's explicit segment budget.
    SegmentLimit,
    /// The supplied JavaScript or map would exceed the caller's input budget.
    InputLimit,
}

impl std::fmt::Display for SourceMapLookupError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::QueryOutOfBounds => formatter.write_str("generated location is out of bounds"),
            Self::MalformedMap => formatter.write_str("source map is malformed"),
            Self::SegmentLimit => formatter.write_str("source-map segment limit exceeded"),
            Self::InputLimit => formatter.write_str("source-map input limit exceeded"),
        }
    }
}

impl std::error::Error for SourceMapLookupError {}

impl SourceMap {
    /// Resolves a zero-based generated UTF-16 position through the complete
    /// decoded map. An unmapped segment, including generated helper bodies,
    /// returns `None`. The caller must separately authorize any source-table
    /// disclosure or debugger action; this pure lookup grants neither.
    ///
    /// The whole stream is validated before a result is returned, even when
    /// the queried position occurs early. `max_segments` and
    /// `max_input_bytes` bound work on an externally constructed `SourceMap`
    /// as well as compiler artifacts. The byte budget includes the generated
    /// JavaScript, mapping and source-table strings, and table entry counts.
    pub fn original_position_at(
        &self,
        javascript: &str,
        generated_line: usize,
        generated_column_utf16: usize,
        max_segments: usize,
        max_input_bytes: usize,
    ) -> Result<Option<SourceMapOriginalPosition>, SourceMapLookupError> {
        let mut selected = None;
        self.visit_original_positions(
            javascript,
            &[(generated_line, generated_column_utf16)],
            max_segments,
            max_input_bytes,
            |line, column, original| {
                if line == generated_line && column <= generated_column_utf16 {
                    selected = original;
                }
            },
        )?;
        Ok(selected)
    }

    /// Resolves only positions that exactly equal a generated mapping segment.
    /// Results preserve query order and duplicates. Positions between segments
    /// and explicitly unmapped segments remain unavailable. The complete map
    /// is validated once before any result is returned. `max_segments` also
    /// bounds the query count and temporary result storage; input-byte limits
    /// have the same meaning as in `original_position_at`.
    pub fn original_positions_at_exact(
        &self,
        javascript: &str,
        queries: &[(usize, usize)],
        max_segments: usize,
        max_input_bytes: usize,
    ) -> Result<Vec<Option<SourceMapOriginalPosition>>, SourceMapLookupError> {
        if queries.len() > max_segments {
            return Err(SourceMapLookupError::SegmentLimit);
        }
        let mut selected = None;
        self.visit_original_positions(
            javascript,
            queries,
            max_segments,
            max_input_bytes,
            |line, column, original| {
                let selected = selected.get_or_insert_with(|| {
                    queries
                        .iter()
                        .map(|query| (*query, None))
                        .collect::<std::collections::BTreeMap<_, _>>()
                });
                if let Some(slot) = selected.get_mut(&(line, column)) {
                    *slot = original;
                }
            },
        )?;
        Ok(selected.map_or_else(
            || vec![None; queries.len()],
            |selected| queries.iter().map(|query| selected[query]).collect(),
        ))
    }

    fn visit_original_positions(
        &self,
        javascript: &str,
        queries: &[(usize, usize)],
        max_segments: usize,
        max_input_bytes: usize,
        mut visit: impl FnMut(usize, usize, Option<SourceMapOriginalPosition>),
    ) -> Result<(), SourceMapLookupError> {
        let mut input_bytes = javascript.len();
        for table_len in [
            self.sources.len(),
            self.sources_content.len(),
            self.names.len(),
        ] {
            input_bytes = input_bytes
                .checked_add(table_len)
                .ok_or(SourceMapLookupError::InputLimit)?;
        }
        for field in [&self.file, &self.source_root, &self.mappings] {
            input_bytes = input_bytes
                .checked_add(field.len())
                .ok_or(SourceMapLookupError::InputLimit)?;
        }
        if input_bytes > max_input_bytes {
            return Err(SourceMapLookupError::InputLimit);
        }
        for field in self
            .sources
            .iter()
            .chain(&self.sources_content)
            .chain(&self.names)
        {
            input_bytes = input_bytes
                .checked_add(field.len())
                .ok_or(SourceMapLookupError::InputLimit)?;
            if input_bytes > max_input_bytes {
                return Err(SourceMapLookupError::InputLimit);
            }
        }
        let generated_widths = emitted_line_widths(javascript);
        if queries.iter().any(|(line, column)| {
            generated_widths
                .get(*line)
                .is_none_or(|width| *column > *width)
        }) {
            return Err(SourceMapLookupError::QueryOutOfBounds);
        }
        if self.sources.len() != self.sources_content.len() {
            return Err(SourceMapLookupError::MalformedMap);
        }
        let original_widths = self
            .sources_content
            .iter()
            .map(|source| emitted_line_widths(source))
            .collect::<Vec<_>>();
        let mut source_index = 0i64;
        let mut source_line = 0i64;
        let mut source_column = 0i64;
        let mut name_index = 0i64;
        let mut segment_count = 0usize;
        for (line, encoded_line) in self.mappings.split(';').enumerate() {
            let Some(&width) = generated_widths.get(line) else {
                return Err(SourceMapLookupError::MalformedMap);
            };
            if encoded_line.is_empty() {
                continue;
            }
            let mut column = 0i64;
            let mut previous = None;
            for encoded in encoded_line.split(',') {
                segment_count = segment_count
                    .checked_add(1)
                    .ok_or(SourceMapLookupError::SegmentLimit)?;
                if segment_count > max_segments {
                    return Err(SourceMapLookupError::SegmentLimit);
                }
                let fields = decode_segment(encoded)?;
                if fields[0] < 0 {
                    return Err(SourceMapLookupError::MalformedMap);
                }
                column = column
                    .checked_add(fields[0])
                    .ok_or(SourceMapLookupError::MalformedMap)?;
                if previous.is_some_and(|value| column <= value)
                    || usize::try_from(column).map_or(true, |value| value > width)
                {
                    return Err(SourceMapLookupError::MalformedMap);
                }
                previous = Some(column);
                let original = if fields.len() == 1 {
                    None
                } else {
                    source_index = source_index
                        .checked_add(fields[1])
                        .ok_or(SourceMapLookupError::MalformedMap)?;
                    source_line = source_line
                        .checked_add(fields[2])
                        .ok_or(SourceMapLookupError::MalformedMap)?;
                    source_column = source_column
                        .checked_add(fields[3])
                        .ok_or(SourceMapLookupError::MalformedMap)?;
                    let source_index = usize::try_from(source_index)
                        .map_err(|_| SourceMapLookupError::MalformedMap)?;
                    let source_line = usize::try_from(source_line)
                        .map_err(|_| SourceMapLookupError::MalformedMap)?;
                    let source_column = usize::try_from(source_column)
                        .map_err(|_| SourceMapLookupError::MalformedMap)?;
                    let width = original_widths
                        .get(source_index)
                        .and_then(|lines| lines.get(source_line))
                        .ok_or(SourceMapLookupError::MalformedMap)?;
                    if source_column > *width {
                        return Err(SourceMapLookupError::MalformedMap);
                    }
                    let name = if let Some(delta) = fields.get(4) {
                        name_index = name_index
                            .checked_add(*delta)
                            .ok_or(SourceMapLookupError::MalformedMap)?;
                        let name_index = usize::try_from(name_index)
                            .map_err(|_| SourceMapLookupError::MalformedMap)?;
                        if name_index >= self.names.len() {
                            return Err(SourceMapLookupError::MalformedMap);
                        }
                        Some(name_index)
                    } else {
                        None
                    };
                    Some(SourceMapOriginalPosition {
                        source_index,
                        line: source_line,
                        column_utf16: source_column,
                        name_index: name,
                    })
                };
                visit(line, column as usize, original);
            }
        }
        Ok(())
    }
}

/// Match the emitter's CR/LF coordinates, including CRLF's single break at LF.
fn emitted_line_widths(text: &str) -> Vec<usize> {
    let mut widths = vec![0];
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\n' || (character == '\r' && characters.peek() != Some(&'\n')) {
            widths.push(0);
        } else {
            *widths.last_mut().unwrap() += character.len_utf16();
        }
    }
    widths
}

fn decode_segment(encoded: &str) -> Result<Vec<i64>, SourceMapLookupError> {
    if encoded.is_empty() {
        return Err(SourceMapLookupError::MalformedMap);
    }
    let mut fields = Vec::with_capacity(5);
    let mut value = 0u64;
    let mut shift = 0u32;
    for byte in encoded.bytes() {
        let digit = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return Err(SourceMapLookupError::MalformedMap),
        };
        let chunk = u64::from(digit & 31);
        if shift >= 64 || chunk > u64::MAX >> shift {
            return Err(SourceMapLookupError::MalformedMap);
        }
        value |= chunk << shift;
        shift += 5;
        if digit & 32 == 0 {
            let magnitude = value >> 1;
            if magnitude > i64::MAX as u64 {
                return Err(SourceMapLookupError::MalformedMap);
            }
            let magnitude = magnitude as i64;
            fields.push(if value & 1 == 1 {
                -magnitude
            } else {
                magnitude
            });
            if fields.len() > 5 {
                return Err(SourceMapLookupError::MalformedMap);
            }
            value = 0;
            shift = 0;
        }
    }
    if shift != 0 || !matches!(fields.len(), 1 | 4 | 5) {
        return Err(SourceMapLookupError::MalformedMap);
    }
    Ok(fields)
}
