// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Output trivia policy, composed with the existing source provenance.

use super::*;
use crate::syntax::{lex, TokenKind};

pub(super) fn erase_type_comments(module: &Module, edits: &mut [TextEdit]) {
    if !module.declarations.iter().any(|declaration| {
        matches!(
            declaration,
            Declaration::Interface(_) | Declaration::TypeAlias(_)
        )
    }) {
        return;
    }
    let Ok(tokens) = lex(&module.id, &module.source) else {
        return;
    };
    for declaration in &module.declarations {
        if !matches!(
            declaration,
            Declaration::Interface(_) | Declaration::TypeAlias(_)
        ) {
            continue;
        }
        let span = declaration.span();
        let Some(edit) = edits.iter_mut().find(|edit| {
            edit.replacement.is_empty() && edit.start <= span.start && edit.end >= span.end
        }) else {
            continue;
        };
        let previous = tokens
            .partition_point(|token| token.end <= edit.start)
            .checked_sub(1)
            .map_or(0, |index| tokens[index].end);
        let gap = &module.source[previous..edit.start];
        let mut cursor = 0;
        let mut attached = 0;
        while cursor < gap.len() {
            if gap[cursor..].starts_with("//") {
                cursor += gap[cursor..]
                    .find(['\r', '\n'])
                    .unwrap_or(gap.len() - cursor);
            } else if gap[cursor..].starts_with("/*") {
                cursor = gap[cursor + 2..]
                    .find("*/")
                    .map_or(gap.len(), |end| cursor + 2 + end + 2);
            } else {
                let start = cursor;
                while cursor < gap.len()
                    && !gap[cursor..].starts_with("//")
                    && !gap[cursor..].starts_with("/*")
                {
                    cursor += gap[cursor..].chars().next().unwrap().len_utf8();
                }
                if gap[start..cursor].matches('\n').count() >= 2 {
                    attached = cursor;
                }
            }
        }
        edit.start = previous + attached;
    }
}

pub(super) fn format_javascript(
    mut emitted: EmittedJavaScript,
    module: &Module,
    options: &CompilerOptions,
) -> Result<EmittedJavaScript, Diagnostic> {
    if options.remove_comments {
        let tokens = lex(&module.id, &emitted.javascript).map_err(|errors| errors[0].clone())?;
        let mut cursor = 0;
        let mut edits = Vec::new();
        for token in tokens {
            let gap = &emitted.javascript[cursor..token.start];
            let bytes = gap.as_bytes();
            let mut index = 0;
            while index < bytes.len() {
                if gap[index..].starts_with("//") || gap[index..].starts_with("/*") {
                    let end = if gap[index..].starts_with("//") {
                        gap[index..]
                            .find(['\r', '\n'])
                            .map_or(gap.len(), |end| index + end)
                    } else {
                        gap[index + 2..]
                            .find("*/")
                            .map_or(gap.len(), |end| index + 2 + end + 2)
                    };
                    let lines: String = gap[index..end]
                        .chars()
                        .filter(|c| matches!(c, '\r' | '\n'))
                        .collect();
                    edits.push(TextEdit {
                        start: cursor + index,
                        end: cursor + end,
                        replacement: if lines.is_empty() {
                            " ".to_string()
                        } else {
                            lines
                        },
                    });
                    index = end;
                } else {
                    index += gap[index..].chars().next().unwrap().len_utf8();
                }
            }
            cursor = token.end;
        }
        if !edits.is_empty() {
            emitted = targets::mapped_edits(emitted, edits);
        }
    }
    let tokens = lex(&module.id, &emitted.javascript).map_err(|errors| errors[0].clone())?;
    let literals: Vec<_> = tokens
        .iter()
        .filter(|token| {
            matches!(
                token.kind,
                TokenKind::String | TokenKind::Template | TokenKind::JsxElement
            )
        })
        .collect();
    let mut edits = Vec::new();
    let mut index = 0;
    let mut literal = 0;
    while index < emitted.javascript.len() {
        if let Some(token) = literals.get(literal) {
            if index == token.start {
                index = token.end;
                literal += 1;
                continue;
            }
        }
        let source = &emitted.javascript[index..];
        let end = if source.starts_with("\r\n") {
            index + 2
        } else if source.starts_with(['\r', '\n']) {
            index + 1
        } else {
            index += source.chars().next().unwrap().len_utf8();
            continue;
        };
        let replacement = match options.new_line {
            crate::NewLine::Lf => "\n",
            crate::NewLine::CrLf => "\r\n",
        };
        if &emitted.javascript[index..end] != replacement {
            edits.push(TextEdit {
                start: index,
                end,
                replacement: replacement.to_string(),
            });
        }
        index = end;
    }
    if options.emit_bom {
        edits.push(TextEdit {
            start: 0,
            end: 0,
            replacement: "\u{feff}".to_string(),
        });
    }
    Ok(if edits.is_empty() {
        emitted
    } else {
        targets::mapped_edits(emitted, edits)
    })
}

pub(super) fn format_text(text: String, options: &CompilerOptions) -> String {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let text = if options.new_line == crate::NewLine::CrLf {
        text.replace('\n', "\r\n")
    } else {
        text
    };
    if options.emit_bom {
        format!("\u{feff}{text}")
    } else {
        text
    }
}

pub(super) fn leading_jsdoc(source: &str, start: usize) -> Vec<&str> {
    let mut prefix = source[..start].trim_end();
    let mut comments = Vec::new();
    while prefix.ends_with("*/") {
        let Some(index) = prefix.rfind("/*") else {
            break;
        };
        let comment = &prefix[index..];
        if comment.starts_with("/**") {
            comments.push(comment);
        }
        prefix = prefix[..index].trim_end();
    }
    comments.reverse();
    comments
}

pub(super) fn is_internal(comment: &str) -> bool {
    comment.match_indices("@internal").any(|(index, tag)| {
        comment[index + tag.len()..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric() && c != '_')
    })
}

pub(super) fn declaration_comments(
    source: &str,
    start: usize,
    indent: &str,
    output: &mut String,
    options: &CompilerOptions,
) -> bool {
    let comments = leading_jsdoc(source, start);
    if options.strip_internal && comments.iter().any(|comment| is_internal(comment)) {
        return false;
    }
    if !options.remove_comments {
        for comment in comments {
            output.push_str(indent);
            output.push_str(comment);
            output.push('\n');
        }
    }
    true
}
