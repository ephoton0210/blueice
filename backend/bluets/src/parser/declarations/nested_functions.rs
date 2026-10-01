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

/// The parts of `(parameters) [: result] { body }`, with where the body ends.
pub(in crate::parser::implementation) struct ParsedFunctionTail {
    pub(in crate::parser::implementation) parameters: Vec<Parameter>,
    pub(in crate::parser::implementation) return_type: Option<Type>,
    pub(in crate::parser::implementation) body: NestedFunctionBody,
    /// Source offset of the end of the body.
    pub(in crate::parser::implementation) body_end: usize,
    /// Token index after the body.
    pub(in crate::parser::implementation) next_index: usize,
}

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

    /// Parses and erases the `<..>` at the cursor, returning its type
    /// parameters, or `None` when it is not a parameter list.
    fn parse_erased_type_parameters(&mut self) -> Option<Vec<TypeParameter>> {
        let start = self.current().start;
        let parameters = self.parse_type_parameters();
        if parameters.is_empty() {
            return None;
        }
        self.edits.push(TextEdit {
            start,
            end: self.current().start,
            replacement: String::new(),
        });
        Some(parameters)
    }

    /// `function name(parameters) [: result] { body }` at a statement position
    /// inside a function body, returning the item to add. Generators, `async`
    /// and generic declarations and destructured parameters are left to the
    /// token path.
    pub(in crate::parser::implementation) fn try_parse_local_function(
        &mut self,
        async_function: bool,
    ) -> Option<FunctionBodyItem> {
        let start_index = self.index;
        if !self.tokens[start_index].is("function") {
            return None;
        }
        let generator = self.tokens.get(start_index + 1)?.is("*");
        if generator && async_function {
            return None;
        }
        let name_at = start_index + 1 + usize::from(generator);
        let name_token = self.tokens.get(name_at)?;
        if name_token.kind != TokenKind::Identifier {
            return None;
        }
        let name = name_token.text.clone();
        let limit = self.tokens.len() - 1;
        let generic = self.tokens.get(name_at + 1)?.is("<");
        let open = if generic {
            matching_angle_bracket(&self.tokens, name_at + 1, limit)? + 1
        } else {
            name_at + 1
        };
        if !self.tokens.get(open)?.is("(") {
            return None;
        }
        let close = matching_close(&self.tokens, open, limit)?;
        if !self.simple_parameter_list(open, close) {
            return None;
        }
        let type_parameters = if generic {
            let saved = self.index;
            self.index = name_at + 1;
            let parsed = self.parse_erased_type_parameters();
            self.index = saved;
            parsed?
        } else {
            Vec::new()
        };
        let ParsedFunctionTail {
            parameters,
            return_type,
            body,
            body_end,
            next_index: next,
        } = self.parse_parameters_result_and_block(open)?;
        let NestedFunctionBody::Block {
            items,
            returns,
            locals,
        } = body
        else {
            return None;
        };
        let start_offset = self.tokens[start_index - usize::from(async_function)].start;
        self.index = next;
        Some(FunctionBodyItem::Function(Box::new(FunctionDeclaration {
            name,
            async_function,
            generator,
            body_open: None,
            type_parameters,
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
        // `*name(..) { .. }`: a generator method.
        let generator_method = self.tokens[index].is("*")
            && self.tokens.get(index + 1).is_some_and(is_name)
            && self
                .tokens
                .get(index + 2)
                .is_some_and(|token| token.is("("));
        if !generator_method && !is_name(&self.tokens[index]) {
            return None;
        }
        // `async name(..)` starts at `async`; a method named `async` has `(`
        // right after it.
        let async_method = self.tokens[index].is("async")
            && self.tokens.get(index + 1).is_some_and(is_name)
            && self
                .tokens
                .get(index + 2)
                .is_some_and(|token| token.is("("));
        let next = self.tokens.get(index + 1)?;
        let (kind, name, open) = if generator_method || async_method {
            (NestedFunctionKind::Method, None, index + 2)
        } else if next.is("(") {
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
        let ParsedFunctionTail {
            parameters,
            return_type,
            body,
            body_end,
            next_index,
        } = self.parse_parameters_result_and_block(open)?;
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
                async_function: async_method,
                generator: generator_method,
                name,
                type_parameters: Vec::new(),
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
    ) -> Option<ParsedFunctionTail> {
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
        Some(ParsedFunctionTail {
            parameters,
            return_type,
            body: NestedFunctionBody::Block {
                items,
                returns,
                locals,
            },
            body_end,
            next_index,
        })
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
        let async_function = index > range_start && self.tokens[index - 1].is("async");
        let head = if async_function { index - 1 } else { index };
        let starts_operand =
            head == range_start || !token_ends_runtime_primary(&self.tokens[head - 1]);
        if !starts_operand {
            return None;
        }
        let generator = self
            .tokens
            .get(index + 1)
            .is_some_and(|token| token.is("*"));
        if generator && async_function {
            return None;
        }
        let mut cursor = index + 1 + usize::from(generator);
        let name = if self.tokens.get(cursor).is_some_and(|token| {
            token.kind == TokenKind::Identifier
                && self
                    .tokens
                    .get(cursor + 1)
                    .is_some_and(|next| next.is("(") || next.is("<"))
        }) {
            cursor += 1;
            Some(self.tokens[cursor - 1].text.clone())
        } else {
            None
        };
        let generic_start = self
            .tokens
            .get(cursor)
            .is_some_and(|token| token.is("<"))
            .then_some(cursor);
        let open = match generic_start {
            Some(angle) => matching_angle_bracket(&self.tokens, angle, end)? + 1,
            None => cursor,
        };
        if !self.tokens.get(open).is_some_and(|token| token.is("(")) {
            return None;
        }
        let close = matching_close(&self.tokens, open, end)?;
        if !self.simple_parameter_list(open, close) {
            return None;
        }
        let saved_index = self.index;
        let type_parameters = match generic_start {
            Some(angle) => {
                self.index = angle;
                let parsed = self.parse_erased_type_parameters();
                self.index = saved_index;
                parsed?
            }
            None => Vec::new(),
        };
        let ParsedFunctionTail {
            parameters,
            return_type,
            body,
            body_end,
            next_index: next,
        } = self.parse_parameters_result_and_block(open)?;
        let start_offset = self.tokens[head].start;
        self.nested_functions.insert(
            start_offset,
            NestedFunction {
                kind: NestedFunctionKind::Function,
                async_function,
                generator,
                name,
                type_parameters,
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
        // An `async` prefix belongs to the arrow, so the operand starts there.
        let async_arrow = index > range_start && self.tokens[index - 1].is("async");
        let head = if async_arrow { index - 1 } else { index };
        let starts_operand =
            head == range_start || !token_ends_runtime_primary(&self.tokens[head - 1]);
        if !starts_operand {
            return None;
        }
        let generic_open = if self.tokens[index].is("<") {
            let open = matching_angle_bracket(&self.tokens, index, end)? + 1;
            self.tokens
                .get(open)
                .is_some_and(|token| token.is("("))
                .then_some(open)
        } else {
            None
        };
        if self.tokens[index].is("<") && generic_open.is_none() {
            return None;
        }
        let (parameters_close, single_name) = if let Some(open) = generic_open {
            let close = matching_close(&self.tokens, open, end)?;
            if !self.simple_parameter_list(open, close) {
                return None;
            }
            (close, false)
        } else if self.tokens[index].is("(") {
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
        let type_parameters = if generic_open.is_some() {
            match self.parse_erased_type_parameters() {
                Some(parsed) => parsed,
                None => {
                    self.diagnostics.truncate(saved_diagnostics);
                    self.edits.truncate(saved_edits);
                    self.index = saved_index;
                    return None;
                }
            }
        } else {
            Vec::new()
        };
        let parameters = if single_name {
            let name = self.tokens[index].text.clone();
            let span = self.tokens[index].span(&self.id);
            self.index = index + 1;
            vec![Parameter {
                name,
                pattern: None,
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
        let start_offset = self.tokens[head].start;
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
                async_function: async_arrow,
                generator: false,
                name: None,
                type_parameters,
                parameters,
                return_type,
                body,
                span: SourceSpan::new(&self.id, start_offset, body_end),
            },
        );
        self.index = saved_index;
        Some(next)
    }

    /// Every parameter in `(` .. `)` is a plain name or a pattern in the
    /// supported subset, optionally a rest name, with an annotation, an optional
    /// marker or a default. A pattern outside the subset makes the list
    /// unsupported here.
    fn simple_parameter_list(&self, open: usize, close: usize) -> bool {
        let mut index = open + 1;
        let mut at_parameter_start = true;
        let mut depth = 0usize;
        while index < close {
            let token = &self.tokens[index];
            if token.is("...")
                && self
                    .tokens
                    .get(index + 1)
                    .is_some_and(|next| next.is("{") || next.is("["))
            {
                // A destructured rest parameter is outside the subset.
                return false;
            }
            if at_parameter_start && depth == 0 && (token.is("{") || token.is("[")) {
                let Some((_, pattern_close)) =
                    patterns::parse_binding_pattern(&self.tokens, index, &self.id)
                else {
                    return false;
                };
                index = pattern_close + 1;
                at_parameter_start = false;
                continue;
            }
            match token.text.as_str() {
                "(" | "[" | "{" => depth += 1,
                ")" | "]" | "}" => depth = depth.saturating_sub(1),
                "," if depth == 0 => {
                    at_parameter_start = true;
                    index += 1;
                    continue;
                }
                _ => {}
            }
            if depth == 0 && !token.is("...") {
                at_parameter_start = false;
            }
            index += 1;
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
