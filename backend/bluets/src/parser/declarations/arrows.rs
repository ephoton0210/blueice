// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Structured arrow functions inside runtime expressions.
//!
//! The scan over an expression range reaches an arrow head at the start of an
//! operand. The parameter list and result annotation are parsed with the same
//! routines as a named function, so their annotations are erased through the
//! same edits, and the body becomes a structured function body the checker can
//! walk. Shapes outside this subset (destructured parameters, `async`, generic
//! arrows) are left as tokens and refused by the erasure audit when they carry
//! an annotation.

use super::*;

impl Parser {
    /// Parses the arrow function whose head is at `index`, returning the token
    /// index where the enclosing scan continues, or `None` when there is no
    /// arrow of the supported shape here.
    pub(in crate::parser::implementation) fn try_parse_arrow_function(
        &mut self,
        range_start: usize,
        index: usize,
        end: usize,
    ) -> Option<usize> {
        let starts_operand =
            index == range_start || !token_ends_runtime_primary(&self.tokens[index - 1]);
        // An `async` arrow's result is a Promise, which BlueTS does not model.
        let async_arrow = index > 0 && self.tokens[index - 1].is("async");
        if !starts_operand || async_arrow {
            return None;
        }
        let (parameters_close, single_name) = if self.tokens[index].is("(") {
            let close = matching_close(&self.tokens, index, end)?;
            if !self.simple_parameter_list(index, close) {
                return None;
            }
            (close, false)
        } else if self.tokens[index].kind == TokenKind::Identifier
            && self.tokens.get(index + 1).is_some_and(|next| next.is("=>"))
            && index + 1 < end
        {
            (index, true)
        } else {
            return None;
        };
        let after = parameters_close + 1;
        let has_return_annotation = self.tokens.get(after).is_some_and(|token| token.is(":"));
        if has_return_annotation && self.in_unmatched_conditional(range_start, index) {
            return None;
        }
        if !has_return_annotation && !self.tokens.get(after).is_some_and(|token| token.is("=>")) {
            return None;
        }

        let saved_index = self.index;
        let saved_diagnostics = self.diagnostics.len();
        let saved_edits = self.edits.len();
        self.index = index;
        let parameters = if single_name {
            let name = self.tokens[index].text.clone();
            let span = self.tokens[index].span(&self.id);
            self.index = index + 1;
            vec![Parameter {
                name,
                rest: false,
                optional: false,
                annotation: None,
                default: None,
                span,
            }]
        } else {
            self.parse_parameters()
        };
        let return_start = self.current().start;
        let return_type = if self.consume(":") {
            let value = self.parse_type_until(&["=>"]);
            // Keep the space before `=>`.
            let return_end = self.previous().end;
            self.edits.push(TextEdit {
                start: return_start,
                end: return_end,
                replacement: String::new(),
            });
            Some(value)
        } else {
            None
        };
        if !self.consume("=>") {
            // Not an arrow after all (a conditional that only looked like one).
            self.diagnostics.truncate(saved_diagnostics);
            self.edits.truncate(saved_edits);
            self.index = saved_index;
            return None;
        }
        let start_offset = self.tokens[index].start;
        let (body, body_end, next) = if self.consume("{") {
            let body_start = self.previous().start;
            let mut items = Vec::new();
            let mut returns = Vec::new();
            let mut locals = Vec::new();
            self.parse_function_body(body_start, &mut items, &mut returns, &mut locals);
            (
                ArrowBody::Block {
                    items,
                    returns,
                    locals,
                },
                self.previous().end,
                self.index,
            )
        } else {
            let body_start = self.index;
            let body_stop = find_balanced_delimiter(
                &self.tokens,
                body_start,
                end.min(self.tokens.len() - 1),
                &[",", ";", ")", "]", "}"],
            );
            if body_stop == body_start {
                self.error_here(
                    DiagnosticCode::ParseError,
                    "expected an arrow function body",
                );
                self.index = saved_index;
                return None;
            }
            (
                ArrowBody::Expression(self.tokens[body_start..body_stop].to_vec()),
                self.tokens[body_stop - 1].end,
                body_start,
            )
        };
        self.arrow_functions.insert(
            start_offset,
            ArrowFunction {
                parameters,
                return_type,
                body,
                span: SourceSpan::new(&self.id, start_offset, body_end),
            },
        );
        self.index = saved_index;
        Some(next)
    }

    /// Every parameter in `(` .. `)` is a plain name, optionally a rest, with
    /// an annotation, an optional marker or a default. A destructuring pattern
    /// makes the list unsupported here.
    fn simple_parameter_list(&self, open: usize, close: usize) -> bool {
        let mut depth = 0usize;
        let mut at_parameter_start = true;
        for token in &self.tokens[open + 1..close] {
            match token.text.as_str() {
                "(" | "[" | "{" => {
                    if at_parameter_start && depth == 0 {
                        return false;
                    }
                    depth += 1;
                }
                ")" | "]" | "}" => depth = depth.saturating_sub(1),
                "," if depth == 0 => {
                    at_parameter_start = true;
                    continue;
                }
                _ => {}
            }
            if depth == 0 && !token.is("...") {
                at_parameter_start = false;
            }
        }
        true
    }

    /// Whether `index` lies between the `?` and `:` of a conditional at its
    /// own nesting level, where `(a) : b =>` is more likely the alternate of a
    /// conditional than an arrow with a result annotation.
    fn in_unmatched_conditional(&self, range_start: usize, index: usize) -> bool {
        // The nearest enclosing opener bounds the level being examined.
        let mut depth = 0usize;
        let mut boundary = range_start;
        for cursor in (range_start..index).rev() {
            match self.tokens[cursor].text.as_str() {
                ")" | "]" | "}" => depth += 1,
                "(" | "[" | "{" => {
                    if depth == 0 {
                        boundary = cursor + 1;
                        break;
                    }
                    depth -= 1;
                }
                _ => {}
            }
        }
        let mut open_conditionals = 0usize;
        let mut nesting = 0usize;
        for cursor in boundary..index {
            match self.tokens[cursor].text.as_str() {
                "(" | "[" | "{" => nesting += 1,
                ")" | "]" | "}" => nesting = nesting.saturating_sub(1),
                "?" if nesting == 0 => open_conditionals += 1,
                ":" if nesting == 0 && open_conditionals > 0 => open_conditionals -= 1,
                _ => {}
            }
        }
        open_conditionals > 0
    }
}

/// The index of the `)` matching the `(` at `open`, within `end`.
fn matching_close(tokens: &[Token], open: usize, end: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, token) in tokens[open..end.min(tokens.len())].iter().enumerate() {
        match token.text.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return token.is(")").then_some(open + offset);
                }
            }
            _ => {}
        }
    }
    None
}
