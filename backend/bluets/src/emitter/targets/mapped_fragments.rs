// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Compose target rewrites before relocation markers are expanded exactly once.
use super::*;
use crate::emitter::relocations;

/// A transformed expression can end before the full original syntax span.
/// Retain that logical endpoint without changing its internal span anchors.
pub(super) fn with_completion(
    previous: EmittedJavaScript,
    mut edits: Vec<TextEdit>,
    boundary: usize,
    origin: usize,
) -> EmittedJavaScript {
    let prefix = &previous.javascript[..origin];
    let key = (
        prefix.bytes().filter(|byte| *byte == b'\n').count(),
        prefix.rsplit('\n').next().unwrap().encode_utf16().count(),
    );
    let index = previous
        .provenance
        .partition_point(|point| (point.generated_line, point.generated_column) <= key);
    let completion = index
        .checked_sub(1)
        .and_then(|index| previous.provenance.get(index))
        .filter(|point| point.generated_line == key.0)
        .copied();
    edits.sort_by_key(|edit| (edit.start, edit.end));
    let (mut cursor, mut destination) = (0, 0);
    for edit in edits.iter().filter(|edit| edit.end <= boundary) {
        if edit.start < cursor {
            continue;
        }
        destination += edit.start - cursor
            + relocations::expand(&edit.replacement, &previous.javascript, &edits)
                .iter()
                .map(|part| part.text().len())
                .sum::<usize>();
        cursor = edit.end;
    }
    destination += boundary - cursor;
    let mut next = mapped_edits(previous, edits);
    if let Some(mut point) = completion {
        let prefix = &next.javascript[..destination];
        point.generated_line = prefix.bytes().filter(|byte| *byte == b'\n').count();
        point.generated_column = prefix.rsplit('\n').next().unwrap().encode_utf16().count();
        let key = (point.generated_line, point.generated_column);
        let index = next
            .provenance
            .partition_point(|current| (current.generated_line, current.generated_column) <= key);
        if let Some(current) = index
            .checked_sub(1)
            .and_then(|index| next.provenance.get_mut(index))
            .filter(|current| (current.generated_line, current.generated_column) == key)
        {
            *current = point;
        } else if next
            .segment_limit
            .is_some_and(|limit| next.provenance.len() >= limit)
        {
            next.overflowed = true;
        } else {
            next.provenance.insert(index, point);
        }
    }
    next
}

pub(super) fn splice(start: usize, end: usize, mut edits: Vec<TextEdit>) -> String {
    edits.sort_by_key(|edit| (edit.start, edit.end));
    let mut result = String::new();
    let mut cursor = start;
    for edit in edits {
        if edit.start < cursor || edit.end > end || edit.end < edit.start {
            continue;
        }
        if cursor < edit.start {
            result.push_str(&relocations::relocated_verbatim(cursor, edit.start));
        }
        result.push_str(&edit.replacement);
        cursor = edit.end;
    }
    if cursor < end {
        result.push_str(&relocations::relocated_verbatim(cursor, end));
    }
    result
}

pub(super) fn template(
    source: &str,
    start: usize,
    end: usize,
    module: &Module,
    depth: usize,
    mut transform: impl FnMut(usize, usize, usize) -> Result<String, Diagnostic>,
) -> Result<String, Diagnostic> {
    if depth == 0 {
        return Err(Diagnostic::error(
            DiagnosticCode::ResourceLimit,
            SourceSpan::new(&module.id, start, end),
            "template suspension exceeds the depth limit",
        ));
    }
    let bytes = source.as_bytes();
    let mut at = start + 1;
    let mut edits = Vec::new();
    while at + 1 < end {
        if bytes[at] == b'\\' {
            at += 2;
            continue;
        }
        if bytes[at..].starts_with(b"${") {
            let expression = at + 2;
            let (close, _) = crate::syntax::lex_expression(&module.id, source, expression)
                .map_err(|message| {
                    Diagnostic::error(
                        DiagnosticCode::ParseError,
                        SourceSpan::new(&module.id, start, end),
                        message,
                    )
                })?;
            edits.push(TextEdit {
                start: expression,
                end: close,
                replacement: transform(expression, close, depth - 1)?,
            });
            at = close + 1;
        } else {
            at += 1;
        }
    }
    Ok(splice(start, end, edits))
}
