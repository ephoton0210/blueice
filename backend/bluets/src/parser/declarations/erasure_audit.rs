// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Audit of runtime text after type erasure.
//!
//! The parser structurally erases annotations on named functions, class
//! methods and variable declarations. Functions nested in expressions (arrows
//! with a return type, function expressions, object methods, functions
//! declared inside a body) keep their tokens verbatim, so a TypeScript
//! annotation on one would be copied into the emitted JavaScript unchanged.
//! Once erasure has been applied no type position remains, so any parameter
//! annotation, return annotation or optional parameter still present is a
//! TypeScript form that would produce invalid JavaScript. Such a form is
//! refused here until the parser erases it structurally.

use super::*;
use crate::syntax::lex;

const MAX_AUDIT_DIAGNOSTICS: usize = 16;

impl Parser {
    pub(in crate::parser::implementation) fn audit_erased_function_annotations(&mut self) {
        self.edits.sort_by_key(|edit| (edit.start, edit.end));
        let mut erased = String::with_capacity(self.source.len());
        // (erased start, erased end, source start, is a replacement)
        let mut segments: Vec<(usize, usize, usize, bool)> = Vec::new();
        let mut cursor = 0usize;
        for edit in &self.edits {
            if edit.start < cursor || edit.end < edit.start || edit.end > self.source.len() {
                continue;
            }
            if edit.start > cursor {
                segments.push((
                    erased.len(),
                    erased.len() + (edit.start - cursor),
                    cursor,
                    false,
                ));
                erased.push_str(&self.source[cursor..edit.start]);
            }
            if !edit.replacement.is_empty() {
                segments.push((
                    erased.len(),
                    erased.len() + edit.replacement.len(),
                    edit.start,
                    true,
                ));
                erased.push_str(&edit.replacement);
            }
            cursor = edit.end;
        }
        if cursor < self.source.len() {
            segments.push((
                erased.len(),
                erased.len() + (self.source.len() - cursor),
                cursor,
                false,
            ));
            erased.push_str(&self.source[cursor..]);
        }
        let Ok(tokens) = lex(&self.id, &erased) else {
            return;
        };
        let source_len = self.source.len();
        let source_offset = |offset: usize| {
            segments
                .iter()
                .find(|(start, end, _, _)| offset >= *start && offset < *end)
                .map(|(start, _, source, replacement)| {
                    if *replacement {
                        *source
                    } else {
                        source + (offset - start)
                    }
                })
                .unwrap_or(source_len)
        };
        let flagged = annotated_function_tokens(&tokens);
        for index in flagged.into_iter().take(MAX_AUDIT_DIAGNOSTICS) {
            let start = source_offset(tokens[index].start);
            let end = source_offset(tokens[index].end.saturating_sub(1)) + 1;
            self.unsupported(
                SourceSpan::new(&self.id, start, end.max(start)),
                "TypeScript annotations on functions nested in expressions are not supported yet",
            );
        }
    }
}

fn is_name(token: &Token) -> bool {
    matches!(token.kind, TokenKind::Identifier | TokenKind::Keyword)
}

/// Indices of tokens that mark an annotated function-like form.
fn annotated_function_tokens(tokens: &[Token]) -> Vec<usize> {
    let mut opens: Vec<usize> = Vec::new();
    // For each close bracket, the matching open; for each open, the enclosing
    // open (or none at the top level).
    let mut matching_open = vec![None; tokens.len()];
    let mut enclosing = vec![None; tokens.len()];
    for (index, token) in tokens.iter().enumerate() {
        match token.text.as_str() {
            "(" | "[" | "{" => {
                enclosing[index] = opens.last().copied();
                opens.push(index);
            }
            ")" | "]" | "}" => {
                matching_open[index] = opens.pop();
            }
            _ => enclosing[index] = opens.last().copied(),
        }
    }
    let text = |index: usize| tokens.get(index).map(|token| token.text.as_str());
    let mut flagged = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        let inside_parentheses = enclosing[index].is_some_and(|open| tokens[open].is("("));
        // A parameter name (optionally a rest) that starts a parameter and is
        // followed by an annotation or an optional marker.
        if is_name(token) && inside_parentheses {
            let start = if index > 0 && text(index - 1) == Some("...") {
                index - 1
            } else {
                index
            };
            let starts_parameter = start > 0 && matches!(text(start - 1), Some("(") | Some(","));
            let annotated = text(index + 1) == Some(":");
            let optional = text(index + 1) == Some("?")
                && matches!(text(index + 2), Some(":") | Some(",") | Some(")"));
            if starts_parameter && (annotated || optional) {
                flagged.push(index);
                continue;
            }
        }
        // A destructured parameter followed by an annotation.
        if (token.is("}") || token.is("]")) && text(index + 1) == Some(":") {
            if let Some(open) = matching_open[index] {
                let parameter_start = open > 0 && matches!(text(open - 1), Some("(") | Some(","));
                let parent_is_parentheses =
                    enclosing[open].is_some_and(|parent| tokens[parent].is("("));
                if parameter_start && parent_is_parentheses {
                    flagged.push(index);
                    continue;
                }
            }
        }
        // A return annotation: `) : Type =>` or `) : Type {` on a function.
        if token.is(")") && text(index + 1) == Some(":") {
            let Some(open) = matching_open[index] else {
                continue;
            };
            let mut depth = 0usize;
            let mut end = None;
            let mut cursor = index + 2;
            while cursor < tokens.len() {
                match tokens[cursor].text.as_str() {
                    "(" | "[" | "{" if depth > 0 || !tokens[cursor].is("{") => depth += 1,
                    ")" | "]" | "}" if depth > 0 => depth -= 1,
                    "=>" | "{" if depth == 0 => {
                        end = Some(cursor);
                        break;
                    }
                    ";" | "," | ")" | "]" | "}" | "?" | ":" if depth == 0 => break,
                    _ => {}
                }
                cursor += 1;
            }
            let Some(end) = end else {
                continue;
            };
            let annotation = &tokens[index + 2..end];
            if annotation.is_empty() || annotation[0].is("(") {
                continue;
            }
            let arrow = tokens[end].is("=>");
            let function_like = open > 0
                && (text(open - 1) == Some("function")
                    || (is_name(&tokens[open - 1])
                        && (open < 2
                            || matches!(
                                text(open - 2),
                                Some("function")
                                    | Some("{")
                                    | Some(",")
                                    | Some("async")
                                    | Some("*")
                                    | Some("get")
                                    | Some("set")
                            ))));
            if arrow || function_like {
                flagged.push(index);
            }
            continue;
        }
        // Type parameters left in front of a parameter list at the start of an
        // operand: a generic arrow that was not structured.
        if token.is("<")
            && (index == 0 || !ends_primary(&tokens[index - 1]))
            && closes_type_parameters_before_parameters(tokens, index)
        {
            flagged.push(index);
            continue;
        }
        // A generic function expression or nested declaration.
        if token.is("function") {
            let next =
                if text(index + 1).is_some_and(|value| value != "(") && index + 1 < tokens.len() {
                    index + 2
                } else {
                    index + 1
                };
            if text(next) == Some("<") {
                flagged.push(index);
            }
        }
    }
    flagged
}

/// Whether a token can end an operand, so a following `<` is a comparison.
fn ends_primary(token: &Token) -> bool {
    matches!(
        token.kind,
        TokenKind::Identifier | TokenKind::Number | TokenKind::String | TokenKind::Template
    ) || matches!(
        token.text.as_str(),
        ")" | "]" | "true" | "false" | "null" | "undefined" | "this" | "super"
    )
}

/// Whether the `<` at `open` closes with a `>` that is followed by `(`, within
/// a short distance. `>>` and `>>>` close two and three levels.
fn closes_type_parameters_before_parameters(tokens: &[Token], open: usize) -> bool {
    let mut depth = 0usize;
    for (offset, token) in tokens[open..].iter().take(64).enumerate() {
        match token.text.as_str() {
            "<" => depth += 1,
            ">" | ">>" | ">>>" => {
                let closers = token.text.len();
                if closers > depth {
                    return false;
                }
                depth -= closers;
                if depth == 0 {
                    return tokens
                        .get(open + offset + 1)
                        .is_some_and(|next| next.is("("));
                }
            }
            ";" | "{" | "}" => return false,
            _ => {}
        }
    }
    false
}
