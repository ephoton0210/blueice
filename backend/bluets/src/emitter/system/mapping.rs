// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Move original text ranges while retaining their source provenance.

use super::*;

pub(super) fn leading_comments(
    emitted: &EmittedJavaScript,
    original: &str,
    from: usize,
    end: usize,
) -> usize {
    let mut cursor = from;
    let mut retained = None;
    while cursor < end {
        let rest = &emitted.javascript[cursor..end];
        let width = if rest.starts_with("//") {
            rest.find(['\r', '\n']).unwrap_or(rest.len())
        } else if rest.starts_with("/*") {
            let Some(close) = rest.find("*/") else {
                return end;
            };
            close + 2
        } else if let Some(character) = rest.chars().next().filter(|c| c.is_whitespace()) {
            cursor += character.len_utf8();
            continue;
        } else {
            return end;
        };
        if original.contains(&rest[..width]) {
            retained.get_or_insert(cursor);
        } else {
            retained = None;
        }
        cursor += width;
    }
    retained.unwrap_or(end)
}

pub(super) fn relocate(
    emitted: EmittedJavaScript,
    functions: &[(usize, usize)],
    prefix: &str,
    execute: &str,
    suffix: &str,
) -> EmittedJavaScript {
    let mut line_starts = vec![0];
    let mut characters = emitted.javascript.char_indices().peekable();
    while let Some((offset, character)) = characters.next() {
        if is_line_break(character, characters.peek().map(|(_, c)| *c)) {
            line_starts.push(offset + character.len_utf8());
        }
    }
    let position = |offset: usize| {
        let line = line_starts.partition_point(|start| *start <= offset) - 1;
        let column = emitted.javascript[line_starts[line]..offset]
            .encode_utf16()
            .count();
        (line, column)
    };
    let mut output = EmittedJavaScript {
        javascript: String::new(),
        provenance: Vec::new(),
        segment_limit: emitted.segment_limit,
        overflowed: emitted.overflowed,
    };
    let (mut line, mut column) = (0, 0);
    let mut append = |text: &str, range: Option<(usize, usize, bool)>| {
        if let Some((start, end, include_end)) = range {
            let (start_line, start_column) = position(start);
            let end_position = position(end);
            // A moved function keeps its completion in the factory. Its old
            // execute trivia is generated transport. An adjacent real token
            // may already own this same offset and must keep its mapping.
            let cut = !include_end
                && text.chars().next().is_some_and(char::is_whitespace)
                && functions.iter().any(|(_, end)| *end == start);
            let first = emitted.provenance.partition_point(|segment| {
                let position = (segment.generated_line, segment.generated_column);
                position < (start_line, start_column)
                    || cut && position == (start_line, start_column)
            });
            let boundary = (cut && !text.is_empty()).then_some(ProvenanceSegment {
                generated_line: start_line,
                generated_column: start_column,
                source_line: 0,
                source_column: 0,
                mapped: false,
                name_index: None,
            });
            for segment in boundary
                .iter()
                .chain(emitted.provenance[first..].iter().take_while(|segment| {
                    (segment.generated_line, segment.generated_column) < end_position
                        || include_end
                            && (segment.generated_line, segment.generated_column) == end_position
                }))
            {
                let mut moved = *segment;
                moved.generated_line = line + segment.generated_line - start_line;
                moved.generated_column = if segment.generated_line == start_line {
                    column + segment.generated_column - start_column
                } else {
                    segment.generated_column
                };
                if output
                    .segment_limit
                    .is_some_and(|limit| output.provenance.len() >= limit)
                {
                    output.overflowed = true;
                } else {
                    output.provenance.push(moved);
                }
            }
        }
        output.javascript.push_str(text);
        let mut characters = text.chars().peekable();
        while let Some(character) = characters.next() {
            if is_line_break(character, characters.peek().copied()) {
                line += 1;
                column = 0;
            } else {
                column += character.len_utf16();
            }
        }
    };
    append(prefix, None);
    for &(start, end) in functions {
        append(&emitted.javascript[start..end], Some((start, end, true)));
        append("\n", None);
    }
    append(execute, None);
    let mut cursor = 0;
    for &(start, end) in functions {
        append(
            &emitted.javascript[cursor..start],
            Some((cursor, start, false)),
        );
        cursor = end;
    }
    append(
        &emitted.javascript[cursor..],
        Some((cursor, emitted.javascript.len(), false)),
    );
    append(suffix, None);
    output
}
