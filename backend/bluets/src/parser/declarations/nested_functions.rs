// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Structured arrow functions and function expressions inside runtime
//! expressions.
//!
//! The scan over an expression range reaches a function head at the start of
//! an operand. The parameter list and result annotation are parsed with the same
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
    pub(in crate::parser::implementation) fn try_parse_nested_function(
        &mut self,
        range_start: usize,
        index: usize,
        end: usize,
    ) -> Option<usize> {
        self.try_parse_arrow_function(range_start, index, end)
            .or_else(|| self.try_parse_function_expression(range_start, index, end))
            .or_else(|| self.try_parse_object_member(range_start, index, end))
    }

    /// `function name(parameters) [: result] { body }` at a statement position
    /// inside a function body, returning the item to add. Generators, `async`
    /// and generic declarations and destructured parameters are left to the
    /// token path.
    pub(in crate::parser::implementation) fn try_parse_local_function(
        &mut self,
    ) -> Option<FunctionBodyItem> {
        let start_index = self.index;
        if !self.tokens[start_index].is("function")
            || start_index > 0 && self.tokens[start_index - 1].is("async")
        {
            return None;
        }
        let name_token = self.tokens.get(start_index + 1)?;
        let open_token = self.tokens.get(start_index + 2)?;
        if name_token.kind != TokenKind::Identifier || !open_token.is("(") {
            return None;
        }
        let name = name_token.text.clone();
        let open = start_index + 2;
        let close = matching_close(&self.tokens, open, self.tokens.len() - 1)?;
        if !self.simple_parameter_list(open, close) {
            return None;
        }
        let (parameters, return_type, body, body_end, next) =
            self.parse_parameters_result_and_block(open)?;
        let NestedFunctionBody::Block {
            items,
            returns,
            locals,
        } = body
        else {
            return None;
        };
        let start_offset = self.tokens[start_index].start;
        self.index = next;
        Some(FunctionBodyItem::Function(Box::new(FunctionDeclaration {
            name,
            async_function: false,
            body_open: None,
            type_parameters: Vec::new(),
            parameters,
            return_type,
            body: items,
            returns,
            locals,
            exported: false,
            default_export: false,
            declared: false,
            overload: false,
            span: SourceSpan::new(&self.id, start_offset, body_end),
        })))
    }

    /// An object-literal method or accessor at a property position: `name(..)
    /// [: result] { .. }`, `get name() [: result] { .. }` or `set name(value)
    /// { .. }`. Computed and string keys, `async` and generator members are
    /// left as tokens.
    fn try_parse_object_member(
        &mut self,
        range_start: usize,
        index: usize,
        end: usize,
    ) -> Option<usize> {
        if index == range_start
            || !matches!(self.tokens[index - 1].text.as_str(), "{" | ",")
            || !self.directly_in_braces(range_start, index)
        {
            return None;
        }
        let is_name =
            |token: &Token| matches!(token.kind, TokenKind::Identifier | TokenKind::Keyword);
        if !is_name(&self.tokens[index]) {
            return None;
        }
        let next = self.tokens.get(index + 1)?;
        let (kind, name, open) = if next.is("(") {
            (NestedFunctionKind::Method, None, index + 1)
        } else if (self.tokens[index].is("get") || self.tokens[index].is("set"))
            && is_name(next)
            && self
                .tokens
                .get(index + 2)
                .is_some_and(|token| token.is("("))
        {
            let kind = if self.tokens[index].is("get") {
                NestedFunctionKind::Getter
            } else {
                NestedFunctionKind::Setter
            };
            (kind, None, index + 2)
        } else {
            return None;
        };
        let close = matching_close(&self.tokens, open, end)?;
        if !self.simple_parameter_list(open, close) {
            return None;
        }
        let saved_index = self.index;
        let (parameters, return_type, body, body_end, next_index) =
            self.parse_parameters_result_and_block(open)?;
        let start_offset = self.tokens[index].start;
        match kind {
            NestedFunctionKind::Getter if !parameters.is_empty() => self.error_at(
                SourceSpan::new(&self.id, start_offset, body_end),
                DiagnosticCode::ParseError,
                "a getter takes no parameters",
            ),
            NestedFunctionKind::Setter if parameters.len() != 1 || return_type.is_some() => self
                .error_at(
                    SourceSpan::new(&self.id, start_offset, body_end),
                    DiagnosticCode::ParseError,
                    "a setter takes exactly one parameter and no result annotation",
                ),
            _ => {}
        }
        self.nested_functions.insert(
            start_offset,
            NestedFunction {
                kind,
                name,
                parameters,
                return_type,
                body,
                span: SourceSpan::new(&self.id, start_offset, body_end),
            },
        );
        self.index = saved_index;
        Some(next_index)
    }

    /// Parses `(parameters) [: result] { body }` with the cursor placed at the
    /// `(` at `open`, returning the parts, the end offset of the body and the
    /// token index after it. The cursor is restored and `None` returned when no
    /// braced body follows.
    pub(in crate::parser::implementation) fn parse_parameters_result_and_block(
        &mut self,
        open: usize,
    ) -> Option<(
        Vec<Parameter>,
        Option<Type>,
        NestedFunctionBody,
        usize,
        usize,
    )> {
        let saved_index = self.index;
        self.index = open;
        let parameters = self.parse_parameters();
        let return_start = self.current().start;
        let return_type = if self.consume(":") {
            let value = self.parse_type_until(&["{"]);
            // Keep the space before `{`.
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
        if !self.consume("{") {
            self.index = saved_index;
            return None;
        }
        let body_start = self.previous().start;
        let mut items = Vec::new();
        let mut returns = Vec::new();
        let mut locals = Vec::new();
        self.parse_function_body(body_start, &mut items, &mut returns, &mut locals);
        let body_end = self.previous().end;
        let next_index = self.index;
        self.index = saved_index;
        Some((
            parameters,
            return_type,
            NestedFunctionBody::Block {
                items,
                returns,
                locals,
            },
            body_end,
            next_index,
        ))
    }

    /// Whether the nearest enclosing bracket of `index` within the range is a
    /// `{`, that is, whether `index` sits directly in an object literal.
    fn directly_in_braces(&self, range_start: usize, index: usize) -> bool {
        let mut depth = 0usize;
        for cursor in (range_start..index).rev() {
            match self.tokens[cursor].text.as_str() {
                ")" | "]" | "}" => depth += 1,
                "(" | "[" | "{" => {
                    if depth == 0 {
                        return self.tokens[cursor].is("{");
                    }
                    depth -= 1;
                }
                _ => {}
            }
        }
        false
    }

    /// `function [name] (parameters) [: result] { body }` at the start of an
    /// operand. Generators, `async` and generic function expressions, and any
    /// destructured parameter, are left as tokens.
    fn try_parse_function_expression(
        &mut self,
        range_start: usize,
        index: usize,
        end: usize,
    ) -> Option<usize> {
        if !self.tokens[index].is("function") {
            return None;
        }
        let starts_operand =
            index == range_start || !token_ends_runtime_primary(&self.tokens[index - 1]);
        let async_function = index > 0 && self.tokens[index - 1].is("async");
        if !starts_operand || async_function {
            return None;
        }
        let mut open = index + 1;
        let name = if self.tokens.get(open).is_some_and(|token| {
            token.kind == TokenKind::Identifier
                && self.tokens.get(open + 1).is_some_and(|next| next.is("("))
        }) {
            open += 1;
            Some(self.tokens[open - 1].text.clone())
        } else {
            None
        };
        if !self.tokens.get(open).is_some_and(|token| token.is("(")) {
            return None;
        }
        let close = matching_close(&self.tokens, open, end)?;
        if !self.simple_parameter_list(open, close) {
            return None;
        }
        let saved_index = self.index;
        let (parameters, return_type, body, body_end, next) =
            self.parse_parameters_result_and_block(open)?;
        let start_offset = self.tokens[index].start;
        self.nested_functions.insert(
            start_offset,
            NestedFunction {
                kind: NestedFunctionKind::Function,
                name,
                parameters,
                return_type,
                body,
                span: SourceSpan::new(&self.id, start_offset, body_end),
            },
        );
        self.index = saved_index;
        Some(next)
    }

    fn try_parse_arrow_function(
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
                NestedFunctionBody::Block {
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
                NestedFunctionBody::Expression(self.tokens[body_start..body_stop].to_vec()),
                self.tokens[body_stop - 1].end,
                body_start,
            )
        };
        self.nested_functions.insert(
            start_offset,
            NestedFunction {
                kind: NestedFunctionKind::Arrow,
                name: None,
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
