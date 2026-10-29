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
        ) || matches!(declaration, Declaration::Function(function) if function.overload)
    }) {
        return;
    }
    let Ok(tokens) = lex(&module.id, &module.source) else {
        return;
    };
    for declaration in &module.declarations {
        if !(matches!(
            declaration,
            Declaration::Interface(_) | Declaration::TypeAlias(_)
        ) || matches!(declaration, Declaration::Function(function) if function.overload))
        {
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

/// Opening-brace trailing trivia has no runtime node after a class/namespace
/// transform. Standalone leading member comments retain their own provenance.
pub(super) fn erase_opening_comments(module: &Module, edits: &mut Vec<TextEdit>) {
    let declarations = super::runtime_declarations(&module.declarations);
    let openings = declarations
        .iter()
        .filter_map(|(declaration, _)| match declaration {
            Declaration::Class(class) => Some(class.body_span.start + 1),
            Declaration::Namespace(namespace) => Some(namespace.header_span.end),
            _ => None,
        })
        .chain(
            module
                .class_expressions
                .values()
                .map(|expression| expression.class.body_span.start + 1),
        );
    for mut at in openings {
        loop {
            let rest = &module.source[at..];
            let trimmed = rest.trim_start_matches([' ', '\t']);
            at += rest.len() - trimmed.len();
            if !trimmed.starts_with("/*") {
                break;
            }
            let Some(end) = trimmed.find("*/").map(|end| at + end + 2) else {
                break;
            };
            edits.push(TextEdit {
                start: at,
                end,
                replacement: String::new(),
            });
            at = end;
        }
    }
}

/// The lexer excludes strings/templates/regular expressions from trivia gaps.
fn comment_ranges(
    module: &Module,
    source: &str,
) -> Result<Vec<std::ops::Range<usize>>, Diagnostic> {
    let tokens = lex(&module.id, source).map_err(|errors| errors[0].clone())?;
    let mut ranges = Vec::new();
    let mut cursor = 0;
    for token in tokens {
        let gap = &source[cursor..token.start];
        let mut index = 0;
        while index < gap.len() {
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
                ranges.push(cursor + index..cursor + end);
                index = end;
            } else {
                index += gap[index..].chars().next().unwrap().len_utf8();
            }
        }
        cursor = token.end;
    }
    Ok(ranges)
}

pub(super) fn format_javascript(
    mut emitted: EmittedJavaScript,
    module: &Module,
    options: &CompilerOptions,
) -> Result<EmittedJavaScript, Diagnostic> {
    if options.remove_comments {
        let header = detached_header(module);
        let mut retained: Vec<String> = comment_ranges(module, &emitted.javascript)?
            .into_iter()
            .map(|range| emitted.javascript[range].to_string())
            .collect();
        let mut missing = String::new();
        for range in comment_ranges(module, &header)? {
            let comment = &header[range];
            if let Some(index) = retained.iter().position(|text| text == comment) {
                retained.remove(index);
            } else {
                missing.push_str(comment);
                missing.push('\n');
            }
        }
        if !missing.is_empty() {
            emitted = targets::mapped_edits(
                emitted,
                vec![TextEdit {
                    start: 0,
                    end: 0,
                    replacement: missing,
                }],
            );
        }
        let edits = comment_ranges(module, &emitted.javascript)?
            .into_iter()
            .filter_map(|range| {
                let text = &emitted.javascript[range.clone()];
                if text.starts_with("/*!") {
                    return None;
                }
                let lines: String = text.chars().filter(|c| matches!(c, '\r' | '\n')).collect();
                Some(TextEdit {
                    start: range.start,
                    end: range.end,
                    replacement: if lines.is_empty() { " ".into() } else { lines },
                })
            })
            .collect::<Vec<_>>();
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
    let mut emitted = if edits.is_empty() {
        emitted
    } else {
        targets::mapped_edits(emitted, edits)
    };
    // A final line ending has no source location. Appending it must not
    // reproject previously unmapped module wrappers or generated helpers.
    if !emitted.javascript.is_empty() && !emitted.javascript.ends_with(['\r', '\n']) {
        emitted.javascript.push_str(match options.new_line {
            crate::NewLine::Lf => "\n",
            crate::NewLine::CrLf => "\r\n",
        });
    }
    Ok(emitted)
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
        let before = &prefix[..index];
        let line = before.rsplit(['\r', '\n']).next().unwrap_or("");
        if comment.starts_with("/**") && line.trim().is_empty() {
            comments.push(comment);
        }
        prefix = prefix[..index].trim_end();
    }
    comments.reverse();
    comments
}

/// Pinned file headers survive `removeComments`, including declaration output.
/// A synthetic namespace module must not acquire the enclosing file's header.
pub(super) fn declaration_header(module: &Module) -> String {
    let Some(first) = module.declarations.first() else {
        return String::new();
    };
    let prefix = &module.source[..first.span().start];
    let Ok(tokens) = lex(&module.id, prefix) else {
        return String::new();
    };
    if tokens.iter().any(|token| token.kind != TokenKind::Eof) {
        return String::new();
    }
    let mut output = String::new();
    let mut rest = prefix.trim_start_matches(['\u{feff}', ' ', '\t', '\r', '\n']);
    while rest.starts_with("/*") || rest.starts_with("//") {
        let end = if rest.starts_with("/*") {
            rest.find("*/").map_or(rest.len(), |end| end + 2)
        } else {
            rest.find(['\r', '\n']).unwrap_or(rest.len())
        };
        if rest.starts_with("/*!") {
            output.push_str(&rest[..end]);
            output.push('\n');
        }
        rest = rest[end..].trim_start();
    }
    output
}

/// With comments disabled, only pinned comments participate in detachment.
/// A header adjacent to an erased declaration is erased with that declaration.
pub(super) fn detached_header(module: &Module) -> String {
    let Some(first) = module.declarations.first() else {
        return String::new();
    };
    let prefix = &module.source[..first.span().start];
    let Ok(ranges) = comment_ranges(module, prefix) else {
        return String::new();
    };
    let last = ranges
        .into_iter()
        .rfind(|range| prefix[range.clone()].starts_with("/*!"));
    if last.is_some_and(|range| prefix[range.end..].matches('\n').count() >= 2) {
        declaration_header(module)
    } else {
        String::new()
    }
}

/// Same-line comments belong to the preceding node, rather than the next one.
pub(super) fn trailing_comments(
    source: &str,
    end: usize,
    output: &mut String,
    options: &CompilerOptions,
) {
    if options.remove_comments {
        return;
    }
    let Some(mut rest) = source.get(end..) else {
        return;
    };
    let mut comments = Vec::new();
    loop {
        rest = rest.trim_start_matches([' ', '\t']);
        if !rest.starts_with("/*") {
            break;
        }
        let Some(end) = rest.find("*/").map(|end| end + 2) else {
            break;
        };
        comments.push(&rest[..end]);
        rest = &rest[end..];
    }
    if !comments.is_empty() {
        let newline = output.ends_with('\n');
        if newline {
            output.pop();
        }
        for comment in comments {
            output.push(' ');
            output.push_str(comment);
        }
        if newline {
            output.push('\n');
        }
    }
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
