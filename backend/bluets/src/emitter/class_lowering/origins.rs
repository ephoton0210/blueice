// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Preserve each moved field operand and distinguish generated descriptor data.
use super::*;
use crate::emitter::relocations;
pub(super) mod auto_accessors;
mod contexts;
pub(super) use contexts::decorated_static_blocks;

pub(super) fn field_statement(
    source: &str,
    edits: &[TextEdit],
    emit: FieldEmit,
    field: &ClassField,
    key: Option<&String>,
) -> String {
    store(
        source,
        emit,
        "this",
        field,
        key,
        initializer(edits, field, None),
    )
}

pub(super) fn static_statement(
    source: &str,
    edits: &[TextEdit],
    emit: FieldEmit,
    field: &ClassField,
    key: Option<&String>,
    receiver: &str,
    bind_this: bool,
) -> String {
    let value = initializer(edits, field, Some(source)).map(|value| {
        if bind_this {
            format!("(function () {{ return {value}; }}).call({receiver})")
        } else {
            value
        }
    });
    store(source, emit, receiver, field, key, value)
}

fn initializer(edits: &[TextEdit], field: &ClassField, source: Option<&str>) -> Option<String> {
    field.initializer.as_deref().map(|tokens| {
        let (Some(first), Some(last)) = (tokens.first(), tokens.last()) else {
            return String::new();
        };
        let mut local = edits
            .iter()
            .filter(|edit| first.start <= edit.start && edit.end <= last.end)
            .cloned()
            .collect::<Vec<_>>();
        // The parser resolves an adjacent generic close and arrow into distinct
        // tokens. Keep the arrow whole; lexing the un-erased source can instead
        // see >= followed by > and invent a boundary inside the emitted =>.
        for token in tokens.iter().filter(|token| token.is("=>")) {
            local.push(TextEdit {
                start: token.start,
                end: token.end,
                replacement: format!(
                    "{}=>{}",
                    relocations::source_anchor(token.start),
                    relocations::source_anchor(token.end)
                ),
            });
        }
        local.sort_by_key(|edit| (edit.start, edit.end));
        let mut value = String::new();
        let mut cursor = first.start;
        for edit in local {
            if edit.start < cursor {
                continue;
            }
            value.push_str(&relocations::relocated_verbatim(cursor, edit.start));
            if !edit.replacement.is_empty() {
                value.push_str(&relocations::source_anchor(edit.start));
                value.push_str(&edit.replacement);
            } else if let Some(source) = source {
                value.extend(
                    source[edit.start..edit.end]
                        .chars()
                        .filter(|ch| matches!(ch, '\r' | '\n')),
                );
            }
            value.push_str(&relocations::source_anchor(edit.end));
            cursor = edit.end;
        }
        value.push_str(&relocations::relocated_verbatim(cursor, last.end));
        value
    })
}

fn store(
    source: &str,
    emit: FieldEmit,
    receiver: &str,
    field: &ClassField,
    key: Option<&String>,
    initializer: Option<String>,
) -> String {
    let start = relocations::source_anchor(field.name_span.start);
    let name_end = relocations::source_anchor(field.name_span.end);
    let generated = relocations::unmapped();
    let static_completion = field.is_static && emit.static_after_instance;
    if emit.define {
        let mut key = key.cloned().unwrap_or_else(|| format!("{:?}", field.name));
        let literal_name = crate::lex(
            &field.name_span.module,
            &source[field.name_span.start..field.name_span.end],
        )
        .is_ok_and(|tokens| {
            tokens.first().is_some_and(|token| {
                matches!(
                    token.kind,
                    crate::TokenKind::String | crate::TokenKind::Number
                )
            })
        });
        if literal_name {
            key.push_str(&name_end);
        }
        let value = initializer.unwrap_or_else(|| "void 0".into());
        let completion = field
            .initializer
            .as_deref()
            .and_then(|tokens| tokens.last())
            .map(|token| relocations::source_anchor(token.end))
            .unwrap_or_default();
        let statement_end = relocations::source_anchor(field.span.end);
        let semicolon = if field.is_static && emit.static_expression {
            relocations::source_anchor(field.span.end)
        } else if static_completion {
            name_end
        } else {
            String::new()
        };
        format!(
            " {start}Object.defineProperty({receiver}, {key}, {{ {generated}enumerable: true, configurable: true, writable: true, value: {value}{completion} {generated}}}){semicolon};{statement_end}"
        )
    } else {
        let access = if let Some(key) = key {
            format!("[{key}]")
        } else if field
            .name
            .chars()
            .all(|character| character.is_alphanumeric() || matches!(character, '_' | '$'))
            && !field
                .name
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_digit())
        {
            format!(".{}", field.name)
        } else {
            format!("[{:?}]", field.name)
        };
        let end = field
            .initializer
            .as_deref()
            .and_then(|tokens| tokens.last())
            .map_or(field.name_span.end, |token| token.end);
        let end = if field.is_static && emit.static_expression {
            field.span.end
        } else if static_completion {
            field.name_span.end
        } else {
            end
        };
        let end = relocations::source_anchor(end);
        let statement_end = relocations::source_anchor(field.span.end);
        format!(
            " {start}{receiver}{access}{name_end} = {}{end};{statement_end}",
            initializer.unwrap_or_else(|| "void 0".into())
        )
    }
}
