// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Function body parsing.

use super::*;

impl Parser {
    pub(in crate::parser::implementation) fn parse_function_body(
        &mut self,
        body_start: usize,
        body: &mut Vec<FunctionBodyItem>,
        returns: &mut Vec<Vec<Token>>,
        locals: &mut Vec<VariableDeclaration>,
    ) {
        let mut depth = 1usize;
        let mut parentheses = 0usize;
        let mut brackets = 0usize;
        while !self.at_eof() && depth > 0 {
            self.diagnose_unsupported_opaque_syntax(
                self.index,
                self.index.saturating_add(2).min(self.tokens.len()),
            );
            if parentheses == 0
                && brackets == 0
                && is_direct_braced_if_statement(&self.tokens, self.index)
            {
                body.push(FunctionBodyItem::If(
                    self.parse_direct_braced_if_statement(returns, locals),
                ));
                continue;
            }
            if parentheses == 0
                && brackets == 0
                && is_direct_braced_while_statement(&self.tokens, self.index)
            {
                body.push(FunctionBodyItem::While(
                    self.parse_direct_braced_while_statement(returns, locals),
                ));
                continue;
            }
            if parentheses == 0
                && brackets == 0
                && is_direct_braced_try_statement(&self.tokens, self.index)
            {
                body.push(FunctionBodyItem::Try(
                    self.parse_direct_braced_try_statement(returns, locals),
                ));
                continue;
            }
            if self.consume("{") {
                depth += 1;
                body.push(FunctionBodyItem::Opaque(self.previous().span(&self.id)));
                continue;
            }
            if self.consume("}") {
                depth -= 1;
                if depth > 0 {
                    body.push(FunctionBodyItem::Opaque(self.previous().span(&self.id)));
                }
                continue;
            }
            if self.consume("return") {
                let return_start = self.previous().start;
                let expression_start = self.index;
                let expression = self.collect_until_statement_end();
                self.collect_expression_type_edits(expression_start, self.index);
                returns.push(expression.clone());
                self.consume(";");
                body.push(FunctionBodyItem::Return {
                    tokens: expression,
                    span: SourceSpan::new(&self.id, return_start, self.previous().end),
                });
                continue;
            }
            if self.consume("throw") {
                let throw_token = self.previous().clone();
                let line_terminator_after_throw = self
                    .source
                    .get(throw_token.end..self.current().start)
                    .is_some_and(|gap| gap.contains('\n') || gap.contains('\r'));
                let expression_start = self.index;
                let tokens = self.collect_until_function_statement_end();
                self.collect_expression_type_edits(expression_start, self.index);
                if line_terminator_after_throw {
                    self.error_at(
                        throw_token.span(&self.id),
                        DiagnosticCode::ParseError,
                        "a line terminator is not permitted after `throw`",
                    );
                }
                if tokens.is_empty() {
                    self.error_at(
                        throw_token.span(&self.id),
                        DiagnosticCode::ParseError,
                        "expected an expression after `throw`",
                    );
                }
                self.consume(";");
                body.push(FunctionBodyItem::Throw {
                    tokens,
                    span: SourceSpan::new(&self.id, throw_token.start, self.previous().end),
                });
                continue;
            }
            if self.peek("const") || self.peek("let") || self.peek("var") {
                let start = self.current().start;
                let kind = match self.current().text.as_str() {
                    "const" => VariableKind::Const,
                    "let" => VariableKind::Let,
                    "var" => VariableKind::Var,
                    _ => unreachable!("variable declaration was guarded by its keyword"),
                };
                self.bump();
                let variable = self.parse_variable_declaration(start, false, false, kind);
                locals.push(variable.clone());
                body.push(FunctionBodyItem::Variable(variable));
                continue;
            }
            if parentheses == 0
                && brackets == 0
                && starts_runtime_expression_statement(self.current())
            {
                let start = self.current().start;
                let expression_start = self.index;
                let tokens = self.collect_until_function_statement_end();
                self.collect_expression_type_edits(expression_start, self.index);
                self.consume(";");
                body.push(FunctionBodyItem::Expression {
                    tokens,
                    span: SourceSpan::new(&self.id, start, self.previous().end),
                });
                continue;
            }
            if parentheses == 0 && brackets == 0 && self.peek("function") {
                if let Some(item) = self.try_parse_local_function(false) {
                    body.push(item);
                    continue;
                }
            }
            if parentheses == 0
                && brackets == 0
                && self.peek("async")
                && self
                    .tokens
                    .get(self.index + 1)
                    .is_some_and(|token| token.is("function"))
            {
                let saved = self.index;
                self.bump();
                if let Some(item) = self.try_parse_local_function(true) {
                    body.push(item);
                    continue;
                }
                self.index = saved;
            }
            if self.peek("as") || self.peek("satisfies") {
                body.push(FunctionBodyItem::Opaque(self.current().span(&self.id)));
                let end = find_balanced_delimiter(
                    &self.tokens,
                    self.index + 1,
                    self.tokens.len() - 1,
                    &[";", ",", ")", "]", "}", "&&", "||"],
                );
                self.index = self.erase_assertion(self.index, end);
                continue;
            }
            if self.consume(";") {
                continue;
            }
            let token = self.current().clone();
            body.push(FunctionBodyItem::Opaque(token.span(&self.id)));
            match token.text.as_str() {
                "(" => parentheses += 1,
                ")" if parentheses > 0 => parentheses -= 1,
                "[" => brackets += 1,
                "]" if brackets > 0 => brackets -= 1,
                _ => {}
            }
            self.bump();
        }
        if depth != 0 {
            self.error_at(
                SourceSpan::new(&self.id, body_start, self.source.len()),
                DiagnosticCode::ParseError,
                "unterminated function body",
            );
        }
    }

    /// Parses a preflighted direct `if` statement. The preflight guarantees
    /// explicit branch braces and a complete alternate before this method
    /// consumes input, preserving opaque fallback behavior for every other
    /// control-flow shape.
    pub(in crate::parser::implementation) fn parse_direct_braced_if_statement(
        &mut self,
        returns: &mut Vec<Vec<Token>>,
        locals: &mut Vec<VariableDeclaration>,
    ) -> FunctionIfStatement {
        debug_assert!(is_direct_braced_if_statement(&self.tokens, self.index));
        let if_start = self.current().start;
        self.bump();
        let test_start = self.index + 1;
        let test_end =
            matching_closing_delimiter(&self.tokens, self.index, self.tokens.len() - 1, "(", ")")
                .expect("the direct braced-if preflight found a closing parenthesis");
        let test = self.tokens[test_start..test_end].to_vec();
        self.collect_expression_type_edits(test_start, test_end);
        self.index = test_end + 1;
        let consequent = self.parse_direct_function_block(returns, locals);
        let alternate = if self.consume("else") {
            if self.peek("{") {
                Some(FunctionElseBranch::Braced(
                    self.parse_direct_function_block(returns, locals),
                ))
            } else {
                Some(FunctionElseBranch::ElseIf(Box::new(
                    self.parse_direct_braced_if_statement(returns, locals),
                )))
            }
        } else {
            None
        };
        FunctionIfStatement {
            test,
            consequent,
            alternate,
            span: SourceSpan::new(&self.id, if_start, self.previous().end),
        }
    }

    pub(in crate::parser::implementation) fn parse_direct_braced_while_statement(
        &mut self,
        returns: &mut Vec<Vec<Token>>,
        locals: &mut Vec<VariableDeclaration>,
    ) -> FunctionWhileStatement {
        debug_assert!(is_direct_braced_while_statement(&self.tokens, self.index));
        let while_start = self.current().start;
        self.bump();
        let test_start = self.index + 1;
        let test_end =
            matching_closing_delimiter(&self.tokens, self.index, self.tokens.len() - 1, "(", ")")
                .expect("the direct braced-while preflight found a closing parenthesis");
        let test = self.tokens[test_start..test_end].to_vec();
        self.collect_expression_type_edits(test_start, test_end);
        self.index = test_end + 1;
        let body = self.parse_direct_function_block(returns, locals);
        FunctionWhileStatement {
            test,
            body,
            span: SourceSpan::new(&self.id, while_start, self.previous().end),
        }
    }

    pub(in crate::parser::implementation) fn parse_direct_braced_try_statement(
        &mut self,
        returns: &mut Vec<Vec<Token>>,
        locals: &mut Vec<VariableDeclaration>,
    ) -> FunctionTryStatement {
        debug_assert!(is_direct_braced_try_statement(&self.tokens, self.index));
        let try_start = self.current().start;
        self.bump();
        let block = self.parse_direct_function_block(returns, locals);
        let handler = if self.consume("catch") {
            let catch_start = self.previous().start;
            self.expect("(");
            let binding = self.current().text.clone();
            self.bump();
            let annotation_start = self.current().start;
            let annotation = if self.consume(":") {
                let span_start = self.previous().start;
                let value = self.parse_type_until(&[")"]);
                let span_end = self.previous().end;
                self.edits.push(TextEdit {
                    start: annotation_start,
                    end: span_end,
                    replacement: String::new(),
                });
                if !matches!(value, Type::Any | Type::Unknown) {
                    self.error_at(
                        SourceSpan::new(&self.id, span_start, span_end),
                        DiagnosticCode::ParseError,
                        "a catch clause variable type annotation must be `any` or `unknown`",
                    );
                }
                Some(value)
            } else {
                None
            };
            self.expect(")");
            let body = self.parse_direct_function_block(returns, locals);
            Some(FunctionCatchClause {
                binding,
                annotation,
                body,
                span: SourceSpan::new(&self.id, catch_start, self.previous().end),
            })
        } else {
            None
        };
        let finalizer = self
            .consume("finally")
            .then(|| self.parse_direct_function_block(returns, locals));
        FunctionTryStatement {
            block,
            handler,
            finalizer,
            span: SourceSpan::new(&self.id, try_start, self.previous().end),
        }
    }

    pub(in crate::parser::implementation) fn parse_direct_function_block(
        &mut self,
        returns: &mut Vec<Vec<Token>>,
        locals: &mut Vec<VariableDeclaration>,
    ) -> Vec<FunctionBodyItem> {
        self.expect("{");
        let block_start = self.previous().start;
        let mut body = Vec::new();
        self.parse_function_body(block_start, &mut body, returns, locals);
        body
    }
}
