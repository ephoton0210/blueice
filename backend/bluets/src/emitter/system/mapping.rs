// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Move original text ranges while retaining their source provenance.

use super::*;

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
    };
    let (mut line, mut column) = (0, 0);
    let mut append = |text: &str, range: Option<(usize, usize)>| {
        if let Some((start, end)) = range {
            let (start_line, start_column) = position(start);
            let end_position = position(end);
            let first = emitted.provenance.partition_point(|segment| {
                (segment.generated_line, segment.generated_column) < (start_line, start_column)
            });
            for segment in emitted.provenance[first..].iter().take_while(|segment| {
                (segment.generated_line, segment.generated_column) < end_position
            }) {
                let mut moved = *segment;
                moved.generated_line = line + segment.generated_line - start_line;
                moved.generated_column = if segment.generated_line == start_line {
                    column + segment.generated_column - start_column
                } else {
                    segment.generated_column
                };
                output.provenance.push(moved);
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
        append(&emitted.javascript[start..end], Some((start, end)));
        append("\n", None);
    }
    append(execute, None);
    let mut cursor = 0;
    for &(start, end) in functions {
        append(&emitted.javascript[cursor..start], Some((cursor, start)));
        cursor = end;
    }
    append(
        &emitted.javascript[cursor..],
        Some((cursor, emitted.javascript.len())),
    );
    append(suffix, None);
    output
}
