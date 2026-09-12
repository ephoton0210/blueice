// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Parser {
    pub(super) fn parse_statement(&mut self) -> Result<Stmt, ParseError> {
        if self.module
            && ((self.check_identifier("import") && !self.check_punct_at(1, Punct::LParen))
                || self.check_identifier("export"))
        {
            return Err(self.syntax_error(
                "static import/export declarations are only valid at module top level",
            ));
        }
        match self.peek().clone() {
            Token::Punct(Punct::Semicolon) => {
                self.advance();
                Ok(Stmt::Empty)
            }
            Token::Punct(Punct::LBrace) => Ok(Stmt::Block(self.parse_block()?)),
            Token::Keyword(Keyword::Var) => self.parse_var_decl_stmt(DeclKind::Var),
            Token::Keyword(Keyword::Let) => self.parse_var_decl_stmt(DeclKind::Let),
            Token::Keyword(Keyword::Const) => self.parse_var_decl_stmt(DeclKind::Const),
            Token::Keyword(Keyword::Function) => {
                self.advance();
                let f = self.parse_function()?;
                if f.name.is_none() {
                    return Err(self.syntax_error("function declarations require a name"));
                }
                if self.static_block_function_depths.last() == Some(&self.function_depth)
                    && f.name.as_deref() == Some("await")
                {
                    return Err(self.syntax_error(
                        "await cannot be bound by a function declaration in a class static block",
                    ));
                }
                Ok(Stmt::FunctionDecl(f))
            }
            Token::Identifier(name) if name == "async" && self.async_function_follows() => {
                self.require_unescaped_async()?;
                self.advance();
                self.expect_keyword(Keyword::Function)?;
                let f = self.parse_function_with_async(true)?;
                if f.name.is_none() {
                    return Err(self.syntax_error("function declarations require a name"));
                }
                Ok(Stmt::FunctionDecl(f))
            }
            Token::Identifier(name) if name == "class" => {
                self.advance();
                let class = self.parse_class()?;
                if class.name.is_none() {
                    return Err(self.syntax_error("class declarations require a name"));
                }
                Ok(Stmt::ClassDecl(class))
            }
            Token::Identifier(name) if name == "with" => self.parse_with_stmt(),
            Token::Keyword(Keyword::If) => self.parse_if_stmt(),
            Token::Keyword(Keyword::For) => self.parse_for_stmt(),
            Token::Keyword(Keyword::While) => self.parse_while_stmt(),
            Token::Keyword(Keyword::Do) => self.parse_do_while_stmt(),
            Token::Keyword(Keyword::Switch) => self.parse_switch_stmt(),
            Token::Keyword(Keyword::Break) => self.parse_break_or_continue(false),
            Token::Keyword(Keyword::Continue) => self.parse_break_or_continue(true),
            Token::Keyword(Keyword::Return) => self.parse_return_stmt(),
            Token::Keyword(Keyword::Throw) => self.parse_throw_stmt(),
            Token::Keyword(Keyword::Try) => self.parse_try_stmt(),
            Token::Keyword(Keyword::Catch | Keyword::Finally) => {
                Err(self.syntax_error("catch/finally require a preceding try block"))
            }
            Token::Identifier(_) if matches!(self.peek_at(1), Token::Punct(Punct::Colon)) => {
                self.parse_labelled_stmt()
            }
            _ => {
                let expr = self.parse_expression()?;
                self.consume_semicolon()?;
                Ok(Stmt::Expr(expr))
            }
        }
    }

    pub(super) fn parse_break_or_continue(
        &mut self,
        is_continue: bool,
    ) -> Result<Stmt, ParseError> {
        self.advance();
        // A line terminator triggers ASI, so an identifier on the following
        // line begins the next statement rather than naming this transfer.
        let label = if !self.newline_before() {
            match self.peek().clone() {
                Token::Identifier(name) => {
                    self.advance();
                    Some(name)
                }
                _ => None,
            }
        } else {
            None
        };
        self.consume_semicolon()?;
        Ok(if is_continue {
            Stmt::Continue(label)
        } else {
            Stmt::Break(label)
        })
    }

    pub(super) fn parse_labelled_stmt(&mut self) -> Result<Stmt, ParseError> {
        let identifier_escaped = self.current_identifier_escaped();
        let label = self.expect_identifier_name()?;
        self.expect_punct(Punct::Colon)?;
        if label == "await" && (self.async_depth != 0 || self.module_await) {
            let detail = if identifier_escaped {
                "the await keyword cannot contain an escape"
            } else {
                "await cannot be used as a label in an async function or module"
            };
            return Err(self.syntax_error(detail));
        }
        if label == "await"
            && self.static_block_function_depths.last() == Some(&self.function_depth)
        {
            return Err(
                self.syntax_error("await cannot be used as a label in a class static block")
            );
        }
        if label == "yield" && self.generator_depth != 0 {
            let detail = if identifier_escaped {
                "the yield keyword cannot contain an escape"
            } else {
                "yield cannot be used as a label in a generator function"
            };
            return Err(self.syntax_error(detail));
        }

        // In sloppy code, `let` may begin the labelled expression statement
        // `L: let` when ASI follows. It is not a lexical declaration there.
        // `let [` is the one prohibited lookahead form.
        let item = if self.check_keyword(Keyword::Let)
            && self
                .tokens
                .get(self.pos + 1)
                .is_some_and(|token| token.newline_before)
        {
            self.advance();
            if self.check_punct(Punct::LBracket) {
                return Err(self.syntax_error("a labelled expression cannot begin with 'let ['"));
            }
            self.consume_semicolon()?;
            Stmt::Expr(Expr::Identifier("let".to_string()))
        } else {
            self.parse_statement()?
        };
        match &item {
            Stmt::VarDecl(kind, _) if *kind != DeclKind::Var => {
                return Err(
                    self.syntax_error("a labelled statement cannot contain a lexical declaration")
                );
            }
            Stmt::ClassDecl(_) => {
                return Err(
                    self.syntax_error("a labelled statement cannot contain a class declaration")
                )
            }
            Stmt::FunctionDecl(function) if function.generator || function.is_async => {
                return Err(self.syntax_error(
                    "a labelled statement cannot contain a generator or async function declaration",
                ));
            }
            _ => {}
        }
        Ok(Stmt::Labelled {
            label,
            item: Box::new(item),
        })
    }

    pub(super) fn parse_block(&mut self) -> Result<Vec<Stmt>, ParseError> {
        self.expect_punct(Punct::LBrace)?;
        let mut stmts = Vec::new();
        while !self.check_punct(Punct::RBrace) {
            if self.at_eof() {
                return Err(self.error("unterminated block, expected '}'"));
            }
            stmts.push(self.parse_statement()?);
        }
        self.expect_punct(Punct::RBrace)?;
        Ok(stmts)
    }

    pub(super) fn parse_var_decl_stmt(&mut self, kind: DeclKind) -> Result<Stmt, ParseError> {
        self.advance();
        let declarators = self.parse_var_declarators()?;
        self.consume_semicolon()?;
        Ok(Stmt::VarDecl(kind, declarators))
    }

    pub(super) fn parse_var_declarators(&mut self) -> Result<Vec<VarDeclarator>, ParseError> {
        let mut decls = Vec::new();
        loop {
            let pattern = self.parse_binding_pattern()?;
            let init = if self.eat_punct(Punct::Assign) {
                Some(self.parse_assignment()?)
            } else {
                None
            };
            decls.push(VarDeclarator { pattern, init });
            if !self.eat_punct(Punct::Comma) {
                break;
            }
        }
        Ok(decls)
    }

    pub(super) fn parse_if_stmt(&mut self) -> Result<Stmt, ParseError> {
        self.advance();
        self.expect_punct(Punct::LParen)?;
        let test = self.parse_expression()?;
        self.expect_punct(Punct::RParen)?;
        let consequent = Box::new(self.parse_statement()?);
        let alternate = if self.eat_keyword(Keyword::Else) {
            Some(Box::new(self.parse_statement()?))
        } else {
            None
        };
        Ok(Stmt::If {
            test,
            consequent,
            alternate,
        })
    }

    pub(super) fn parse_while_stmt(&mut self) -> Result<Stmt, ParseError> {
        self.advance();
        self.expect_punct(Punct::LParen)?;
        let test = self.parse_expression()?;
        self.expect_punct(Punct::RParen)?;
        let body = Box::new(self.parse_statement()?);
        Ok(Stmt::While { test, body })
    }

    pub(super) fn parse_do_while_stmt(&mut self) -> Result<Stmt, ParseError> {
        self.advance();
        let body = Box::new(self.parse_statement()?);
        self.expect_keyword(Keyword::While)?;
        self.expect_punct(Punct::LParen)?;
        let test = self.parse_expression()?;
        self.expect_punct(Punct::RParen)?;
        // A trailing `;` after `do ... while (test)` is conventional
        // but the spec (and real scripts) tolerate its absence too;
        // ASI's `consume_semicolon` already accepts either.
        self.consume_semicolon()?;
        Ok(Stmt::DoWhile { body, test })
    }

    pub(super) fn parse_for_stmt(&mut self) -> Result<Stmt, ParseError> {
        self.advance();
        // Preserve `await` in the AST: async iterators await their `.next()`
        // Promise instead of silently taking the synchronous for-of path.
        let is_await = if (self.async_depth != 0 || self.module_await)
            && matches!(self.peek(), Token::Identifier(name) if name == "await")
        {
            self.advance();
            true
        } else {
            false
        };
        self.expect_punct(Punct::LParen)?;

        if self.eat_punct(Punct::Semicolon) {
            return self.parse_for_rest(None);
        }

        let decl_kind = match self.peek() {
            Token::Keyword(Keyword::Var) => Some(DeclKind::Var),
            Token::Keyword(Keyword::Let) => Some(DeclKind::Let),
            Token::Keyword(Keyword::Const) => Some(DeclKind::Const),
            _ => None,
        };
        if let Some(decl_kind) = decl_kind {
            self.advance();
            let pattern = self.parse_binding_pattern()?;

            if self.eat_keyword(Keyword::In) {
                if is_await {
                    return Err(self.syntax_error("for await requires an of clause"));
                }
                let right = self.parse_expression()?;
                self.expect_punct(Punct::RParen)?;
                let body = Box::new(self.parse_statement()?);
                return Ok(Stmt::ForIn {
                    left: ForHead::Decl(decl_kind, pattern),
                    right,
                    body,
                });
            }
            if self.is_contextual_of() {
                self.advance();
                let right = self.parse_assignment()?;
                self.expect_punct(Punct::RParen)?;
                let body = Box::new(self.parse_statement()?);
                return Ok(Stmt::ForOf {
                    left: ForHead::Decl(decl_kind, pattern),
                    right,
                    body,
                    is_await,
                });
            }

            let initializer = self.parse_optional_for_init_value()?;
            if self.check_keyword(Keyword::In) {
                if let Some(initializer) = initializer {
                    if decl_kind == DeclKind::Var && matches!(pattern, Pattern::Identifier(_)) {
                        self.advance();
                        let right = self.parse_expression()?;
                        self.expect_punct(Punct::RParen)?;
                        let body = Box::new(self.parse_statement()?);
                        return Ok(Stmt::ForIn {
                            left: ForHead::AnnexBVarInit(pattern, initializer),
                            right,
                            body,
                        });
                    }
                    return Err(known_syntax(self.syntax_error(
                        "for-in/of declaration heads cannot have initializers",
                    )));
                }
            }
            let mut declarators = vec![VarDeclarator {
                pattern,
                init: initializer,
            }];
            while self.eat_punct(Punct::Comma) {
                let pattern = self.parse_binding_pattern()?;
                declarators.push(VarDeclarator {
                    pattern,
                    init: self.parse_optional_for_init_value()?,
                });
            }
            // A lexical/var declaration with an initializer cannot form a
            // for-in/of head. Parsing its initializer with `in` disabled
            // intentionally leaves the separator available for this exact
            // early-error classification instead of degrading into a generic
            // missing-semicolon parser failure.
            if (self.check_keyword(Keyword::In) || self.is_contextual_of())
                && declarators
                    .iter()
                    .any(|declarator| declarator.init.is_some())
            {
                return Err(known_syntax(self.syntax_error(
                    "for-in/of declaration heads cannot have initializers",
                )));
            }
            self.expect_punct(Punct::Semicolon)?;
            return self.parse_for_rest(Some(ForInit::VarDecl(decl_kind, declarators)));
        }

        self.no_in = true;
        let expr = self.parse_expression();
        self.no_in = false;
        let expr = expr?;

        if self.eat_keyword(Keyword::In) {
            if is_await {
                return Err(self.syntax_error("for await requires an of clause"));
            }
            let left = expr_to_for_head(expr).map_err(known_syntax)?;
            let right = self.parse_expression()?;
            self.expect_punct(Punct::RParen)?;
            let body = Box::new(self.parse_statement()?);
            return Ok(Stmt::ForIn { left, right, body });
        }
        if self.is_contextual_of() {
            self.advance();
            let left = expr_to_for_head(expr).map_err(known_syntax)?;
            let right = self.parse_assignment()?;
            self.expect_punct(Punct::RParen)?;
            let body = Box::new(self.parse_statement()?);
            return Ok(Stmt::ForOf {
                left,
                right,
                body,
                is_await,
            });
        }
        if is_await {
            return Err(self.syntax_error("for await requires an of clause"));
        }
        self.expect_punct(Punct::Semicolon)?;
        self.parse_for_rest(Some(ForInit::Expr(expr)))
    }

    /// `= <assignment expr>` with `in` disabled, or nothing -- shared by
    /// each declarator in a classic `for (let a = 1, b = 2; ...)` head.
    pub(super) fn parse_optional_for_init_value(&mut self) -> Result<Option<Expr>, ParseError> {
        if !self.eat_punct(Punct::Assign) {
            return Ok(None);
        }
        self.no_in = true;
        let value = self.parse_assignment();
        self.no_in = false;
        Ok(Some(value?))
    }

    pub(super) fn parse_for_rest(&mut self, init: Option<ForInit>) -> Result<Stmt, ParseError> {
        let test = if self.check_punct(Punct::Semicolon) {
            None
        } else {
            Some(self.parse_expression()?)
        };
        self.expect_punct(Punct::Semicolon)?;
        let update = if self.check_punct(Punct::RParen) {
            None
        } else {
            Some(self.parse_expression()?)
        };
        self.expect_punct(Punct::RParen)?;
        let body = Box::new(self.parse_statement()?);
        Ok(Stmt::For {
            init,
            test,
            update,
            body,
        })
    }

    pub(super) fn parse_switch_stmt(&mut self) -> Result<Stmt, ParseError> {
        self.advance();
        self.expect_punct(Punct::LParen).map_err(known_syntax)?;
        if self.check_punct(Punct::RParen) {
            return Err(self.syntax_error("switch requires a discriminant expression"));
        }
        let discriminant = self.parse_expression()?;
        self.expect_punct(Punct::RParen).map_err(known_syntax)?;
        self.expect_punct(Punct::LBrace).map_err(known_syntax)?;
        let mut cases = Vec::new();
        let mut saw_default = false;
        while !self.check_punct(Punct::RBrace) {
            let test = if self.eat_keyword(Keyword::Case) {
                if self.check_punct(Punct::Colon) {
                    return Err(self.syntax_error("case requires an expression"));
                }
                let e = self.parse_expression()?;
                self.expect_punct(Punct::Colon).map_err(known_syntax)?;
                Some(e)
            } else {
                if saw_default {
                    return Err(
                        self.syntax_error("a switch statement can contain only one default clause")
                    );
                }
                self.expect_keyword(Keyword::Default)
                    .map_err(known_syntax)?;
                self.expect_punct(Punct::Colon).map_err(known_syntax)?;
                saw_default = true;
                None
            };
            let mut consequent = Vec::new();
            while !self.check_punct(Punct::RBrace)
                && !self.check_keyword(Keyword::Case)
                && !self.check_keyword(Keyword::Default)
            {
                if self.at_eof() {
                    return Err(self.syntax_error("unterminated switch statement, expected '}'"));
                }
                consequent.push(self.parse_statement()?);
            }
            cases.push(SwitchCase { test, consequent });
        }
        self.expect_punct(Punct::RBrace)?;
        Ok(Stmt::Switch {
            discriminant,
            cases,
        })
    }

    pub(super) fn parse_return_stmt(&mut self) -> Result<Stmt, ParseError> {
        self.advance();
        if self.check_punct(Punct::Semicolon)
            || self.check_punct(Punct::RBrace)
            || self.at_eof()
            || self.newline_before()
        {
            self.consume_semicolon()?;
            return Ok(Stmt::Return(None));
        }
        let expr = self.parse_expression()?;
        self.consume_semicolon()?;
        Ok(Stmt::Return(Some(expr)))
    }

    pub(super) fn parse_throw_stmt(&mut self) -> Result<Stmt, ParseError> {
        self.advance();
        if self.newline_before() {
            return Err(self.error("illegal line terminator between 'throw' and its expression"));
        }
        let expr = self.parse_expression()?;
        self.consume_semicolon()?;
        Ok(Stmt::Throw(expr))
    }

    pub(super) fn parse_try_stmt(&mut self) -> Result<Stmt, ParseError> {
        self.advance();
        if !self.check_punct(Punct::LBrace) {
            return Err(self.syntax_error("try requires a block"));
        }
        let block = self.parse_block()?;
        let handler = if self.eat_keyword(Keyword::Catch) {
            let param = if self.eat_punct(Punct::LParen) {
                let p = self.parse_binding_pattern().map_err(known_syntax)?;
                self.expect_punct(Punct::RParen).map_err(known_syntax)?;
                if self.static_block_function_depths.last() == Some(&self.function_depth)
                    && matches!(&p, Pattern::Identifier(name) if name == "await")
                {
                    return Err(
                        self.syntax_error("await cannot be bound directly in a class static block")
                    );
                }
                Some(p)
            } else {
                None
            };
            if !self.check_punct(Punct::LBrace) {
                return Err(self.syntax_error("catch requires a block"));
            }
            Some(CatchClause {
                param,
                body: self.parse_block()?,
            })
        } else {
            None
        };
        let finalizer = if self.eat_keyword(Keyword::Finally) {
            if !self.check_punct(Punct::LBrace) {
                return Err(self.syntax_error("finally requires a block"));
            }
            Some(self.parse_block()?)
        } else {
            None
        };
        if handler.is_none() && finalizer.is_none() {
            return Err(self.syntax_error("'try' must be followed by 'catch', 'finally', or both"));
        }
        Ok(Stmt::Try {
            block,
            handler,
            finalizer,
        })
    }

    pub(super) fn parse_with_stmt(&mut self) -> Result<Stmt, ParseError> {
        self.advance();
        self.expect_punct(Punct::LParen)?;
        let object = self.parse_expression()?;
        self.expect_punct(Punct::RParen)?;
        let body = self.parse_statement()?;
        Ok(Stmt::With {
            object,
            body: Box::new(body),
        })
    }

    // ---- Patterns ----
}
