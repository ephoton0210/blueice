// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Runtime expression checks and type erasure edits.

use super::*;

impl Parser {
    pub(in crate::parser::implementation) fn collect_expression_type_edits(
        &mut self,
        start: usize,
        end: usize,
    ) {
        self.diagnose_unsupported_opaque_syntax(start, end);
        let mut index = start;
        while index < end {
            if self.tokens[index].kind == TokenKind::Template {
                let token = self.tokens[index].clone();
                self.collect_template_type_edits(&token);
            }
            if matches!(self.tokens[index].text.as_str(), "const" | "let" | "var")
                && self
                    .tokens
                    .get(index + 1)
                    .is_some_and(|token| token.kind == TokenKind::Identifier)
                && self
                    .tokens
                    .get(index + 2)
                    .is_some_and(|token| token.is(":"))
            {
                let saved = self.index;
                self.index = index + 3;
                let annotation = self.parse_type_until(&["=", ";", ",", ")", "of", "in"]);
                self.expression_variable_types
                    .insert(self.tokens[index].start, annotation);
                self.edits.push(TextEdit {
                    start: self.tokens[index + 2].start,
                    end: self.current().start,
                    replacement: String::new(),
                });
                index = self.index;
                self.index = saved;
                continue;
            }
            if let Some(next) = self.try_parse_nested_function(start, index, end) {
                index = next;
                continue;
            }
            if self.tokens[index].kind == TokenKind::Identifier
                && self
                    .tokens
                    .get(index + 1)
                    .is_some_and(|token| token.is("<"))
            {
                if let Some(close) = matching_angle_bracket(&self.tokens, index + 1, end) {
                    if self
                        .tokens
                        .get(close + 1)
                        .is_some_and(|token| token.is("("))
                    {
                        let type_arguments = self.parse_call_type_arguments(index + 2, close);
                        self.edits.push(TextEdit {
                            start: self.tokens[index + 1].start,
                            end: self.tokens[close].end,
                            replacement: String::new(),
                        });
                        self.generic_call_type_arguments
                            .insert(self.tokens[index].start, type_arguments);
                        index = close + 1;
                        continue;
                    }
                }
            }
            if (self.tokens[index].is("as") || self.tokens[index].is("satisfies"))
                && !index.checked_sub(1).is_some_and(|previous| {
                    matches!(self.tokens[previous].text.as_str(), "." | "?.")
                })
                && !self.tokens.get(index + 1).is_some_and(|next| next.is(":"))
            {
                index = self.erase_assertion(index, end);
                continue;
            }
            if self.tokens[index].is(":") && is_typed_arrow_parameter(&self.tokens, index, end) {
                self.unsupported(
                    self.tokens[index].span(&self.id),
                    "typed arrow parameters are not in the initial BlueTS matrix",
                );
            }
            if self.tokens[index].is("!")
                && index > start
                && self
                    .tokens
                    .get(index + 1)
                    .is_some_and(|token| matches!(token.text.as_str(), ";" | "," | ")" | "]" | "."))
            {
                self.edits.push(TextEdit {
                    start: self.tokens[index].start,
                    end: self.tokens[index].end,
                    replacement: String::new(),
                });
            }
            index += 1;
        }
    }

    pub(in crate::parser::implementation) fn collect_following_variable_types(
        &mut self,
        start: usize,
        end: usize,
    ) {
        let mut depth = 0usize;
        let mut index = start;
        while index < end {
            match self.tokens[index].text.as_str() {
                "(" | "[" | "{" => depth += 1,
                ")" | "]" | "}" => depth = depth.saturating_sub(1),
                "," if depth == 0
                    && self
                        .tokens
                        .get(index + 1)
                        .is_some_and(|token| token.kind == TokenKind::Identifier)
                    && self
                        .tokens
                        .get(index + 2)
                        .is_some_and(|token| token.is(":")) =>
                {
                    let saved = self.index;
                    self.index = index + 3;
                    let value = self.parse_type_until(&["=", ",", ";"]);
                    self.expression_variable_types
                        .insert(self.tokens[index + 1].start, value);
                    self.edits.push(TextEdit {
                        start: self.tokens[index + 2].start,
                        end: self.current().start,
                        replacement: String::new(),
                    });
                    index = self.index;
                    self.index = saved;
                    continue;
                }
                _ => {}
            }
            index += 1;
        }
    }

    /// Template substitutions retain their absolute positions and use the
    /// same nested-function and type grammar as every other expression.
    fn collect_template_type_edits(&mut self, token: &Token) {
        let bytes = token.text.as_bytes();
        let mut index = 1;
        while index + 1 < bytes.len() {
            if bytes[index] == b'\\' {
                index += 2;
                continue;
            }
            if bytes[index..].starts_with(b"${") {
                let start = token.start + index + 2;
                if let Ok((end, mut tokens)) =
                    crate::syntax::lex_expression(&self.id, &self.source, start)
                {
                    tokens.push(Token {
                        kind: TokenKind::Eof,
                        text: String::new(),
                        start: end,
                        end,
                    });
                    let original = std::mem::replace(
                        &mut self.tokens,
                        split_generic_closers(merge_private_names(tokens)),
                    );
                    let saved = self.index;
                    self.index = 0;
                    self.collect_expression_type_edits(0, self.tokens.len() - 1);
                    self.index = saved;
                    self.tokens = original;
                    index = end - token.start + 1;
                    continue;
                }
            }
            index += 1;
        }
    }

    /// Opaque expression spans are otherwise preserved for JavaScript
    /// emission. Known TypeScript-only declarations must still be rejected
    /// there, rather than being emitted as invalid JavaScript merely because
    /// they were nested in an arrow initializer, return expression, or raw
    /// statement.
    pub(in crate::parser::implementation) fn diagnose_unsupported_opaque_syntax(
        &mut self,
        start: usize,
        end: usize,
    ) {
        for index in start..end.saturating_sub(1) {
            let previous_is_abstract = self
                .tokens
                .get(index.saturating_sub(1))
                .is_some_and(|token| token.is("abstract"));
            let decorated_class = self
                .tokens
                .get(index.saturating_sub(2))
                .is_some_and(|token| token.is("@"));
            let next = self.tokens.get(index + 1);
            let message = if self.tokens[index].is("@") {
                Some("decorators are supported on top-level class declarations and their members only")
            } else if self.tokens[index].is("abstract")
                && next.is_some_and(|token| token.is("class"))
            {
                Some("`abstract` declarations are not in the initial BlueTS matrix")
            } else if self.tokens[index].is("class")
                && !previous_is_abstract
                && !decorated_class
                && next.is_some_and(|token| {
                    token.kind == TokenKind::Identifier || token.is("extends") || token.is("{")
                })
            {
                Some("`class` is not in the initial BlueTS matrix")
            } else if self.tokens[index].is("enum")
                && next.is_some_and(|token| token.kind == TokenKind::Identifier)
            {
                Some("an `enum` declared inside a body is not supported yet")
            } else if self.tokens[index].is("namespace")
                && next.is_some_and(|token| token.kind == TokenKind::Identifier)
            {
                Some("`namespace` is not in the initial BlueTS matrix")
            } else if self.tokens[index].is("module")
                && next.is_some_and(|token| {
                    token.kind == TokenKind::Identifier || token.kind == TokenKind::String
                })
            {
                Some("`module` is not in the initial BlueTS matrix")
            } else {
                None
            };
            if let Some(message) = message {
                self.unsupported(self.tokens[index].span(&self.id), message);
            }
        }
    }

    /// ECMAScript requires parentheses when `??` appears with `&&` or `||`
    /// in the same logical expression. The bounded parser otherwise retains
    /// runtime expressions as token spans, so enforce this early error before
    /// unsupported raw statements can be copied into emitted JavaScript.
    pub(in crate::parser::implementation) fn diagnose_unparenthesized_nullish_logical_mixing(
        &mut self,
    ) {
        #[derive(Clone, Copy, PartialEq, Eq)]
        enum LogicalFamily {
            Nullish,
            AndOr,
        }

        let mut scopes = vec![None::<LogicalFamily>];
        for index in 0..self.tokens.len() {
            let token = self.tokens[index].clone();
            match token.text.as_str() {
                "(" | "[" | "{" => scopes.push(None),
                ")" | "]" | "}" if scopes.len() > 1 => {
                    scopes.pop();
                }
                "," | ";" | "?" | ":" => {
                    *scopes
                        .last_mut()
                        .expect("the global expression scope remains") = None;
                }
                "??" => {
                    let scope = scopes
                        .last_mut()
                        .expect("the global expression scope remains");
                    if *scope == Some(LogicalFamily::AndOr) {
                        self.error_at(
                            token.span(&self.id),
                            DiagnosticCode::ParseError,
                            "parentheses are required when mixing `??` with `&&` or `||`",
                        );
                        *scope = None;
                    } else {
                        *scope = Some(LogicalFamily::Nullish);
                    }
                }
                "&&" | "||" => {
                    let scope = scopes
                        .last_mut()
                        .expect("the global expression scope remains");
                    if *scope == Some(LogicalFamily::Nullish) {
                        self.error_at(
                            token.span(&self.id),
                            DiagnosticCode::ParseError,
                            "parentheses are required when mixing `??` with `&&` or `||`",
                        );
                        *scope = None;
                    } else {
                        *scope = Some(LogicalFamily::AndOr);
                    }
                }
                _ => {}
            }
        }
    }

    /// `ExponentiationExpression` accepts an update expression as its left
    /// operand, not an unparenthesized unary expression. The bounded parser
    /// otherwise retains runtime token spans, so keep this ECMAScript early
    /// error from reaching emitted JavaScript unchanged.
    pub(in crate::parser::implementation) fn diagnose_unparenthesized_unary_exponentiation(
        &mut self,
    ) {
        for exponent in 0..self.tokens.len() {
            if !self.tokens[exponent].is("**") {
                continue;
            }
            let Some(base_start) = exponentiation_base_start(&self.tokens, exponent) else {
                continue;
            };
            let Some(operator) = base_start.checked_sub(1) else {
                continue;
            };
            if is_unparenthesized_unary_exponent_base(&self.tokens, operator) {
                self.error_at(
                    self.tokens[exponent].span(&self.id),
                    DiagnosticCode::ParseError,
                    "a unary expression cannot be the unparenthesized base of exponentiation",
                );
            }
        }
    }

    pub(in crate::parser::implementation) fn parse_call_type_arguments(
        &mut self,
        start: usize,
        end: usize,
    ) -> Vec<Type> {
        if start == end {
            return Vec::new();
        }
        let eof = self.tokens[end - 1].end;
        let mut tokens = self.tokens[start..end].to_vec();
        tokens.push(Token {
            kind: TokenKind::Eof,
            text: String::new(),
            start: eof,
            end: eof,
        });
        let mut parser = Parser::new(self.id.clone(), String::new(), tokens, self.max_type_depth);
        let mut values = Vec::new();
        while !parser.at_eof() {
            let before = parser.index;
            values.push(parser.parse_type_until(&[","]));
            if parser.index == before {
                parser.error_here(DiagnosticCode::ParseError, "expected a type argument");
                break;
            }
            if !parser.consume(",") {
                break;
            }
        }
        if !parser.at_eof() {
            parser.error_here(
                DiagnosticCode::ParseError,
                "expected a comma between type arguments",
            );
        }
        self.diagnostics.append(&mut parser.diagnostics);
        values
    }

    pub(in crate::parser::implementation) fn erase_assertion(
        &mut self,
        index: usize,
        end: usize,
    ) -> usize {
        let edit_start = self.tokens[index].start;
        let type_start = index + 1;
        let end_index = find_balanced_delimiter(
            &self.tokens,
            type_start,
            end,
            &[";", ",", ")", "]", "}", "&&", "||"],
        );
        // Assertions are erased, but their named types still need lexical
        // resolution. Parse through the same type grammar as annotations.
        let saved_index = self.index;
        self.index = type_start;
        self.parse_type_until(&[";", ",", ")", "]", "}", "&&", "||"]);
        self.index = saved_index;
        self.edits.push(TextEdit {
            start: edit_start,
            end: self.tokens[end_index].start,
            replacement: String::new(),
        });
        end_index
    }
}
