// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! An original static array has distinct body and generated value completions.
use super::*;
use crate::emitter::relocations::{source_anchor, unmapped};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FieldForm {
    Assignment,
    Definition,
    Retained,
}

pub(super) fn static_block_header(module: &Module, tokens: &[Token], edits: &mut Vec<TextEdit>) {
    let (Some(keyword), Some(open), Some(first)) = (tokens.first(), tokens.get(1), tokens.get(2))
    else {
        return;
    };
    if !keyword.is("static") || !open.is("{") {
        return;
    }
    let before = &module.source[keyword.end..open.start];
    let after = &module.source[open.end..first.start];
    if !before.chars().all(char::is_whitespace) || !after.chars().all(char::is_whitespace) {
        return;
    }
    edits.push(TextEdit {
        start: keyword.end,
        end: first.start,
        replacement: format!(
            "{}{before}{{{}{}",
            source_anchor(keyword.start),
            if after.is_empty() { " " } else { after },
            source_anchor(first.start)
        ),
    });
}

pub(super) fn field_value(
    module: &Module,
    field: &crate::parser::ClassField,
    tokens: &[Token],
    form: FieldForm,
    edits: &mut Vec<TextEdit>,
) {
    let define = form != FieldForm::Assignment;
    let (Some(first), Some(last)) = (tokens.first(), tokens.last()) else {
        return;
    };
    if !first.is("[") || !last.is("]") || close(tokens, 0) != Some(tokens.len() - 1) {
        if define
            && first.is("super")
            && edits.iter().any(|edit| {
                edit.start == first.start
                    && edit.end == last.end
                    && edit
                        .replacement
                        .ends_with(&format!("{} ", source_anchor(first.end)))
            })
        {
            let after = &module.source[last.end..field.span.end];
            if let Some(semicolon) = after.find(';').filter(|at| after[..*at].trim().is_empty()) {
                edits.push(TextEdit {
                    start: last.end + semicolon,
                    end: last.end + semicolon + 1,
                    replacement: format!(
                        "{};{} ",
                        source_anchor(first.end),
                        source_anchor(field.span.end)
                    ),
                });
            }
        }
        if !define {
            // A direct read can already end with a logical-get carrier. When
            // that read is the complete field value, the enclosing assignment
            // owns the same physical endpoint, just as in Native output.
            if first.is("super") {
                let logical = format!("{} ", source_anchor(first.end));
                for edit in edits
                    .iter_mut()
                    .filter(|edit| edit.start == first.start && edit.end == last.end)
                {
                    if let Some(prefix) = edit.replacement.strip_suffix(&logical) {
                        edit.replacement =
                            format!("{prefix}{} ", source_anchor(field.name_span.end));
                    }
                }
            }
            edits.push(TextEdit {
                start: last.end,
                end: last.end,
                replacement: format!("{} ", source_anchor(field.name_span.end)),
            });
        }
        return;
    }
    let mut references = 0;
    let mut temporary = false;
    let mut closing_read = None;
    for (index, _) in tokens
        .iter()
        .enumerate()
        .filter(|(_, token)| token.is("super"))
    {
        let end = if tokens.get(index + 1).is_some_and(|token| token.is("[")) {
            close(tokens, index + 1)
        } else if tokens.get(index + 1).is_some_and(|token| token.is(".")) {
            Some(index + 2)
        } else {
            None
        };
        let Some(end) = end else { continue };
        let prefix = index
            .checked_sub(1)
            .filter(|at| matches!(tokens[*at].text.as_str(), "++" | "--"));
        if !edits.iter().any(|edit| {
            (edit.start == tokens[index].start
                || prefix.is_some_and(|at| edit.start == tokens[at].start))
                && edit.end >= tokens[end].end
                && edit.end <= last.end
        }) {
            continue;
        }
        references += 1;
        if prefix.is_none() && end + 1 == tokens.len() - 1 {
            closing_read = Some(tokens[index].end);
        }
        if let Some(comma) = tokens
            .get(end + 1)
            .filter(|token| prefix.is_none() && token.is(","))
        {
            let next = tokens.get(end + 2).map_or(comma.end, |token| token.start);
            let trivia = &module.source[comma.end..next];
            let (completion, trivia) = if trivia.chars().all(char::is_whitespace) {
                (next, trivia)
            } else {
                (comma.end, "")
            };
            // A direct Reflect.get finishes at the original `super` keyword.
            // Native leaves that phase on the following array separator,
            // while the next value establishes its own source position.
            edits.push(TextEdit {
                start: comma.start,
                end: completion,
                replacement: format!(
                    "{},{}{trivia} ",
                    source_anchor(tokens[index].end),
                    source_anchor(tokens[index].end)
                ),
            });
        }
        temporary |= index
            .checked_sub(1)
            .is_some_and(|at| matches!(tokens[at].text.as_str(), "++" | "--"))
            || tokens.get(end + 1).is_some_and(|token| {
                ASSIGNMENTS.contains(&token.text.as_str()) || token.is("++") || token.is("--")
            });
    }
    if references == 0 {
        if !define {
            edits.push(TextEdit {
                start: last.end,
                end: last.end,
                replacement: format!("{} ", source_anchor(field.name_span.end)),
            });
        }
        return;
    }
    if let Some(completion) = closing_read {
        edits.push(TextEdit {
            start: last.start,
            end: last.end,
            replacement: format!(
                "{}]{} ",
                source_anchor(completion),
                source_anchor(if temporary || define {
                    last.end
                } else {
                    field.name_span.end
                })
            ),
        });
    }
    if temporary {
        if form == FieldForm::Retained {
            let after = &module.source[last.end..field.span.end];
            if let Some(semicolon) = after.find(';').filter(|at| after[..*at].trim().is_empty()) {
                edits.push(TextEdit {
                    start: last.end + semicolon,
                    end: last.end + semicolon + 1,
                    replacement: format!("{};{} ", unmapped(), source_anchor(field.span.end)),
                });
            }
        }
        // The source array's end is independent of the generated temporary
        // envelope's end. A transparent grouping gives those physical points
        // separate positions without introducing another evaluation scope.
        edits.push(TextEdit {
            start: first.start,
            end: first.start,
            replacement: format!(
                "{}( {}",
                if form == FieldForm::Definition {
                    unmapped().to_string()
                } else {
                    source_anchor(field.name_span.end)
                },
                source_anchor(first.start)
            ),
        });
        edits.push(TextEdit {
            start: last.end,
            end: last.end,
            replacement: format!(
                "{} {} ){} ",
                source_anchor(last.end),
                unmapped(),
                if define {
                    unmapped().to_string()
                } else {
                    source_anchor(field.name_span.end)
                }
            ),
        });
    } else if !define {
        edits.push(TextEdit {
            start: last.end,
            end: last.end,
            replacement: format!("{} ", source_anchor(field.name_span.end)),
        });
    }
}

/// Resolve only operands owned by this super-reference pass. The ordinary
/// relocation expander deliberately excludes marker-bearing nested edits;
/// cutting each operand around its owned child keeps both the child rewrite
/// and its explicit logical-reference endpoints through the outer field.
pub(super) fn resolve_operands(edits: &mut [TextEdit], first: usize) {
    let mut order = (first..edits.len()).collect::<Vec<_>>();
    order.sort_by_key(|index| (std::cmp::Reverse(edits[*index].start), edits[*index].end));
    for index in order {
        let replacement = edits[index].replacement.clone();
        let mut output = String::new();
        let mut rest = replacement.as_str();
        while let Some(open) = rest.find("\0M") {
            output.push_str(&rest[..open]);
            let after = &rest[open + 2..];
            let Some(close) = after.find('\0') else {
                output.push_str(&rest[open..]);
                rest = "";
                break;
            };
            let range = &after[..close];
            let Some((start, end)) = range.split_once(',').and_then(|(start, end)| {
                Some((start.parse::<usize>().ok()?, end.parse::<usize>().ok()?))
            }) else {
                output.push_str(&rest[open..open + 3 + close]);
                rest = &after[close + 1..];
                continue;
            };
            let mut children = edits[first..]
                .iter()
                .filter(|edit| edit.start >= start && edit.end <= end && edit.start < edit.end)
                .collect::<Vec<_>>();
            children.sort_by_key(|edit| (edit.start, std::cmp::Reverse(edit.end)));
            let mut cursor = start;
            for child in children {
                if child.start < cursor {
                    continue;
                }
                if cursor < child.start {
                    output.push_str(&crate::emitter::relocations::relocated_verbatim(
                        cursor,
                        child.start,
                    ));
                }
                output.push_str(&child.replacement);
                cursor = child.end;
            }
            if cursor < end {
                output.push_str(&crate::emitter::relocations::relocated_verbatim(
                    cursor, end,
                ));
            }
            rest = &after[close + 1..];
        }
        output.push_str(rest);
        edits[index].replacement = output;
    }
}
