// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Parser {
    pub(super) fn parse_expression(&mut self) -> Result<Expr, ParseError> {
        // `Expression` is a left-associative sequence of assignment
        // expressions. Callers which require AssignmentExpression
        // (arguments, initializers, patterns, and conditional arms) stay on
        // `parse_assignment`, because commas delimit those productions.
        let first = self.parse_assignment()?;
        let mut expressions = vec![first];
        while self.eat_punct(Punct::Comma) {
            expressions.push(self.parse_assignment()?);
        }
        if expressions.len() == 1 {
            Ok(expressions.pop().unwrap())
        } else {
            Ok(Expr::Sequence(expressions))
        }
    }

    pub(super) fn parse_assignment(&mut self) -> Result<Expr, ParseError> {
        let left = if let Some(arrow) = self.try_parse_arrow_function()? {
            arrow
        } else {
            if self.destructuring_assignment_ahead() {
                let pattern = self.parse_assignment_pattern()?;
                self.expect_punct(Punct::Assign)?;
                let value = self.parse_assignment()?;
                return Ok(Expr::DestructureAssign {
                    pattern,
                    value: Box::new(value),
                });
            }
            self.parse_conditional()?
        };
        let op = match self.peek() {
            Token::Punct(Punct::Assign) => Some(AssignOp::Assign),
            Token::Punct(Punct::PlusAssign) => Some(AssignOp::AddAssign),
            Token::Punct(Punct::MinusAssign) => Some(AssignOp::SubAssign),
            Token::Punct(Punct::StarAssign) => Some(AssignOp::MulAssign),
            Token::Punct(Punct::StarStarAssign) => Some(AssignOp::ExponentAssign),
            Token::Punct(Punct::SlashAssign) => Some(AssignOp::DivAssign),
            Token::Punct(Punct::PercentAssign) => Some(AssignOp::ModAssign),
            Token::Punct(Punct::ShiftLeftAssign) => Some(AssignOp::ShiftLeftAssign),
            Token::Punct(Punct::ShiftRightAssign) => Some(AssignOp::ShiftRightAssign),
            Token::Punct(Punct::UnsignedShiftRightAssign) => {
                Some(AssignOp::UnsignedShiftRightAssign)
            }
            Token::Punct(Punct::AndAssign) => Some(AssignOp::BitAndAssign),
            Token::Punct(Punct::XorAssign) => Some(AssignOp::BitXorAssign),
            Token::Punct(Punct::OrAssign) => Some(AssignOp::BitOrAssign),
            Token::Punct(Punct::AndAndAssign) => Some(AssignOp::LogicalAndAssign),
            Token::Punct(Punct::OrOrAssign) => Some(AssignOp::LogicalOrAssign),
            Token::Punct(Punct::QuestionQuestionAssign) => Some(AssignOp::NullishAssign),
            _ => None,
        };
        let Some(op) = op else {
            return Ok(unparenthesize(left));
        };
        let annex_b_call_target = is_annex_b_call_assignment_target(&left)
            && !matches!(
                op,
                AssignOp::LogicalAndAssign | AssignOp::LogicalOrAssign | AssignOp::NullishAssign
            );
        if !is_valid_ref_target(&left) && !annex_b_call_target {
            // An AssignmentExpression whose left-hand side was parsed
            // successfully but is not a reference is an ECMAScript early
            // error, rather than an unsupported production.  This notably
            // covers `(await value) = other` in modules and async functions.
            return Err(self.syntax_error("invalid assignment target"));
        }
        self.advance();
        let value = self.parse_assignment()?;
        Ok(Expr::Assign {
            op,
            target: Box::new(left),
            value: Box::new(value),
        })
    }

    /// A leading array/object cover grammar is a destructuring target only
    /// when its matching delimiter is immediately followed by `=`. This
    /// keeps ordinary literals on the regular expression path and lets
    /// parenthesized object assignment (`({x} = source)`) parse correctly.
    pub(super) fn destructuring_assignment_ahead(&self) -> bool {
        if !matches!(self.peek(), Token::Punct(Punct::LBracket | Punct::LBrace)) {
            return false;
        }
        let mut delimiters = Vec::new();
        let mut index = self.pos;
        loop {
            match self.tokens[index].token {
                Token::Punct(Punct::LParen) => delimiters.push(Punct::RParen),
                Token::Punct(Punct::LBracket) => delimiters.push(Punct::RBracket),
                Token::Punct(Punct::LBrace) => delimiters.push(Punct::RBrace),
                Token::Punct(punct) if delimiters.last() == Some(&punct) => {
                    delimiters.pop();
                    if delimiters.is_empty() {
                        return matches!(
                            self.tokens.get(index + 1).map(|token| &token.token),
                            Some(Token::Punct(Punct::Assign))
                        );
                    }
                }
                Token::Eof => return false,
                _ => {}
            }
            index += 1;
        }
    }

    pub(super) fn parse_assignment_pattern(&mut self) -> Result<AssignmentPattern, ParseError> {
        match self.peek() {
            Token::Punct(Punct::LBracket | Punct::LBrace)
                if !self.cover_assignment_target_has_lhs_suffix() =>
            {
                if self.check_punct(Punct::LBracket) {
                    self.parse_array_assignment_pattern()
                } else {
                    self.parse_object_assignment_pattern()
                }
            }
            _ => {
                let target = self.parse_lhs_expression()?;
                if !is_valid_ref_target(&target) {
                    return Err(self.syntax_error("invalid destructuring assignment target"));
                }
                Ok(AssignmentPattern::Target(Box::new(target)))
            }
        }
    }

    /// Array/object literals are cover grammar in a destructuring assignment.
    /// They form a nested pattern unless the closing delimiter is immediately
    /// followed by a left-hand-side suffix.  For example, `{}[key]` is a
    /// member target (including after `...`), whereas `{key}` is a nested
    /// object pattern.  Looking only at the opener used to misparse the
    /// former as a pattern and reject a valid rest target as non-final.
    pub(super) fn cover_assignment_target_has_lhs_suffix(&self) -> bool {
        let Some(open) = (match self.peek() {
            Token::Punct(Punct::LBracket) => Some(Punct::RBracket),
            Token::Punct(Punct::LBrace) => Some(Punct::RBrace),
            _ => None,
        }) else {
            return false;
        };
        let mut delimiters = vec![open];
        let mut index = self.pos + 1;
        while let Some(token) = self.tokens.get(index) {
            match token.token {
                Token::Punct(Punct::LParen) => delimiters.push(Punct::RParen),
                Token::Punct(Punct::LBracket) => delimiters.push(Punct::RBracket),
                Token::Punct(Punct::LBrace) => delimiters.push(Punct::RBrace),
                Token::Punct(punct) if delimiters.last() == Some(&punct) => {
                    delimiters.pop();
                    if delimiters.is_empty() {
                        return matches!(
                            self.tokens.get(index + 1).map(|token| &token.token),
                            Some(Token::Punct(Punct::Dot | Punct::LBracket | Punct::LParen))
                        );
                    }
                }
                Token::Eof => return false,
                _ => {}
            }
            index += 1;
        }
        false
    }

    pub(super) fn parse_array_assignment_pattern(
        &mut self,
    ) -> Result<AssignmentPattern, ParseError> {
        self.expect_punct(Punct::LBracket)?;
        let mut elements = Vec::new();
        while !self.check_punct(Punct::RBracket) {
            if self.eat_punct(Punct::Comma) {
                elements.push(None);
                continue;
            }
            let rest = self.eat_punct(Punct::Ellipsis);
            let pattern = self.parse_assignment_pattern()?;
            let default = if rest {
                None
            } else if self.eat_punct(Punct::Assign) {
                Some(self.parse_assignment()?)
            } else {
                None
            };
            elements.push(Some(AssignmentPatternElement {
                pattern,
                default,
                rest,
            }));
            if rest && !self.check_punct(Punct::RBracket) {
                return Err(self.syntax_error(
                    "a rest element must be last in a destructuring assignment pattern",
                ));
            }
            if !self.check_punct(Punct::RBracket) {
                self.expect_punct(Punct::Comma).map_err(known_syntax)?;
            }
        }
        self.expect_punct(Punct::RBracket)?;
        Ok(AssignmentPattern::Array(elements))
    }

    pub(super) fn parse_object_assignment_pattern(
        &mut self,
    ) -> Result<AssignmentPattern, ParseError> {
        self.expect_punct(Punct::LBrace)?;
        let mut properties = Vec::new();
        while !self.check_punct(Punct::RBrace) {
            if self.eat_punct(Punct::Ellipsis) {
                properties.push(AssignmentPatternProp::Rest(
                    self.parse_assignment_pattern()?,
                ));
                if !self.check_punct(Punct::RBrace) {
                    return Err(self.syntax_error(
                        "a rest property must be last in a destructuring assignment pattern",
                    ));
                }
            } else {
                let shorthand_is_identifier_reference =
                    self.assignment_property_is_identifier_reference();
                let key = self.parse_property_key()?;
                let (value, default) = if self.eat_punct(Punct::Colon) {
                    let value = self.parse_assignment_pattern()?;
                    let default = if self.eat_punct(Punct::Assign) {
                        Some(self.parse_assignment()?)
                    } else {
                        None
                    };
                    (value, default)
                } else {
                    if !shorthand_is_identifier_reference {
                        return Err(self.syntax_error(
                            "destructuring assignment shorthand requires an IdentifierReference",
                        ));
                    }
                    let PropertyKey::Identifier(name) = &key else {
                        return Err(
                            self.syntax_error("expected ':' in destructuring assignment pattern")
                        );
                    };
                    let default = if self.eat_punct(Punct::Assign) {
                        Some(self.parse_assignment()?)
                    } else {
                        None
                    };
                    (
                        AssignmentPattern::Target(Box::new(Expr::Identifier(name.clone()))),
                        default,
                    )
                };
                properties.push(AssignmentPatternProp::KeyValue {
                    key,
                    value,
                    default,
                });
            }
            if !self.check_punct(Punct::RBrace) {
                self.expect_punct(Punct::Comma).map_err(known_syntax)?;
            }
        }
        self.expect_punct(Punct::RBrace)?;
        Ok(AssignmentPattern::Object(properties))
    }

    pub(super) fn parse_conditional(&mut self) -> Result<Expr, ParseError> {
        let test = self.parse_nullish()?;
        if self.eat_punct(Punct::Question) {
            // `in` is allowed again between `?` and `:` even inside a
            // no-in `for`-head context (matches real ECMAScript
            // grammar: the no-in restriction doesn't propagate through
            // a parenthesized-equivalent sub-expression).
            let saved_no_in = self.no_in;
            self.no_in = false;
            let consequent = self.parse_assignment();
            self.no_in = saved_no_in;
            let consequent = consequent?;
            self.expect_punct(Punct::Colon)?;
            let alternate = self.parse_assignment()?;
            return Ok(Expr::Conditional {
                test: Box::new(test),
                consequent: Box::new(consequent),
                alternate: Box::new(alternate),
            });
        }
        Ok(test)
    }

    pub(super) fn parse_nullish(&mut self) -> Result<Expr, ParseError> {
        let (mut left, logical) = self.parse_logical_or()?;
        while self.eat_punct(Punct::QuestionQuestion) {
            if logical {
                return Err(self.error("parentheses required when mixing ?? with && or ||"));
            }
            let (right, logical_right) = self.parse_logical_or()?;
            if logical_right {
                return Err(self.error("parentheses required when mixing ?? with && or ||"));
            }
            left = Expr::Logical {
                op: LogicalOp::Nullish,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    pub(super) fn parse_logical_or(&mut self) -> Result<(Expr, bool), ParseError> {
        let (mut left, mut logical) = self.parse_logical_and()?;
        while self.eat_punct(Punct::OrOr) {
            let (right, _) = self.parse_logical_and()?;
            left = Expr::Logical {
                op: LogicalOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            };
            logical = true;
        }
        Ok((left, logical))
    }

    pub(super) fn parse_logical_and(&mut self) -> Result<(Expr, bool), ParseError> {
        let mut left = self.parse_bitwise_or()?;
        let mut logical = false;
        while self.eat_punct(Punct::AndAnd) {
            let right = self.parse_bitwise_or()?;
            left = Expr::Logical {
                op: LogicalOp::And,
                left: Box::new(left),
                right: Box::new(right),
            };
            logical = true;
        }
        Ok((left, logical))
    }

    pub(super) fn parse_bitwise_or(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_bitwise_xor()?;
        while self.eat_punct(Punct::Or) {
            let right = self.parse_bitwise_xor()?;
            left = Expr::Binary {
                op: BinaryOp::BitOr,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    pub(super) fn parse_bitwise_xor(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_bitwise_and()?;
        while self.eat_punct(Punct::Xor) {
            let right = self.parse_bitwise_and()?;
            left = Expr::Binary {
                op: BinaryOp::BitXor,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    pub(super) fn parse_bitwise_and(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_equality()?;
        while self.eat_punct(Punct::And) {
            let right = self.parse_equality()?;
            left = Expr::Binary {
                op: BinaryOp::BitAnd,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    pub(super) fn parse_equality(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_relational()?;
        loop {
            let op = if self.check_punct(Punct::EqEq) {
                BinaryOp::Eq
            } else if self.check_punct(Punct::NotEq) {
                BinaryOp::NotEq
            } else if self.check_punct(Punct::EqEqEq) {
                BinaryOp::StrictEq
            } else if self.check_punct(Punct::NotEqEq) {
                BinaryOp::StrictNotEq
            } else {
                break;
            };
            self.advance();
            let right = self.parse_relational()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    pub(super) fn parse_relational(&mut self) -> Result<Expr, ParseError> {
        // `#name in object` is a distinct relational-expression production:
        // a private identifier cannot otherwise begin an expression.  Keep
        // its RHS at ShiftExpression precedence, matching ordinary `in`.
        let mut left = if !self.no_in
            && matches!(self.peek(), Token::PrivateIdentifier(_))
            && matches!(self.peek_at(1), Token::Keyword(Keyword::In))
        {
            let Token::PrivateIdentifier(name) = self.advance().clone() else {
                unreachable!("private identifier was checked above")
            };
            self.advance(); // `in`
            Expr::PrivateIn {
                name,
                object: Box::new(self.parse_shift()?),
            }
        } else {
            self.parse_shift()?
        };
        loop {
            let op = if self.check_punct(Punct::Lt) {
                BinaryOp::Lt
            } else if self.check_punct(Punct::Gt) {
                BinaryOp::Gt
            } else if self.check_punct(Punct::LtEq) {
                BinaryOp::LtEq
            } else if self.check_punct(Punct::GtEq) {
                BinaryOp::GtEq
            } else if self.check_keyword(Keyword::Instanceof) {
                BinaryOp::Instanceof
            } else if self.check_keyword(Keyword::In) && !self.no_in {
                BinaryOp::In
            } else {
                break;
            };
            self.advance();
            let right = self.parse_shift()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    pub(super) fn parse_shift(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_additive()?;
        loop {
            let op = if self.check_punct(Punct::ShiftLeft) {
                BinaryOp::ShiftLeft
            } else if self.check_punct(Punct::ShiftRight) {
                BinaryOp::ShiftRight
            } else if self.check_punct(Punct::UnsignedShiftRight) {
                BinaryOp::UnsignedShiftRight
            } else {
                break;
            };
            self.advance();
            let right = self.parse_additive()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    pub(super) fn parse_additive(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_multiplicative()?;
        loop {
            let op = if self.check_punct(Punct::Plus) {
                BinaryOp::Add
            } else if self.check_punct(Punct::Minus) {
                BinaryOp::Sub
            } else {
                break;
            };
            self.advance();
            let right = self.parse_multiplicative()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    pub(super) fn parse_multiplicative(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_exponentiation()?;
        loop {
            let op = if self.check_punct(Punct::Star) {
                BinaryOp::Mul
            } else if self.check_punct(Punct::Slash) {
                BinaryOp::Div
            } else if self.check_punct(Punct::Percent) {
                BinaryOp::Mod
            } else {
                break;
            };
            self.advance();
            let right = self.parse_exponentiation()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    pub(super) fn parse_exponentiation(&mut self) -> Result<Expr, ParseError> {
        // Exponentiation's left operand is an UpdateExpression, not a
        // UnaryExpression. This rejects `-x ** y` while still admitting a
        // unary expression on the right (`x ** -y`) and update expressions
        // on either side. Parenthesized unary expressions enter through
        // parse_update and remain valid bases.
        let unary_base = self.starts_unary_expression();
        let left = if unary_base {
            self.parse_unary()?
        } else {
            self.parse_update_expression()?
        };
        if !self.eat_punct(Punct::StarStar) {
            return Ok(left);
        }
        if unary_base {
            return Err(self.syntax_error(
                "a unary expression cannot be the unparenthesized base of exponentiation",
            ));
        }
        let right = self.parse_exponentiation()?;
        Ok(Expr::Binary {
            op: BinaryOp::Exponent,
            left: Box::new(left),
            right: Box::new(right),
        })
    }

    pub(super) fn starts_unary_expression(&self) -> bool {
        matches!(
            self.peek(),
            Token::Punct(Punct::Bang | Punct::Minus | Punct::Plus | Punct::Tilde)
                | Token::Keyword(Keyword::Typeof | Keyword::Void | Keyword::Delete)
        ) || ((self.async_depth != 0 || self.module_await)
            && matches!(self.peek(), Token::Identifier(name) if name == "await"))
    }

    pub(super) fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        if self.eat_punct(Punct::Bang) {
            return Ok(Expr::Unary {
                op: UnaryOp::Not,
                arg: Box::new(self.parse_unary_operand()?),
            });
        }
        if self.eat_punct(Punct::Minus) {
            return Ok(Expr::Unary {
                op: UnaryOp::Neg,
                arg: Box::new(self.parse_unary_operand()?),
            });
        }
        if self.eat_punct(Punct::Plus) {
            return Ok(Expr::Unary {
                op: UnaryOp::Plus,
                arg: Box::new(self.parse_unary_operand()?),
            });
        }
        if self.eat_punct(Punct::Tilde) {
            return Ok(Expr::Unary {
                op: UnaryOp::BitNot,
                arg: Box::new(self.parse_unary_operand()?),
            });
        }
        if self.eat_keyword(Keyword::Typeof) {
            return Ok(Expr::Unary {
                op: UnaryOp::Typeof,
                arg: Box::new(self.parse_unary_operand()?),
            });
        }
        if self.eat_keyword(Keyword::Void) {
            return Ok(Expr::Unary {
                op: UnaryOp::Void,
                arg: Box::new(self.parse_unary_operand()?),
            });
        }
        if self.eat_keyword(Keyword::Delete) {
            return Ok(Expr::Unary {
                op: UnaryOp::Delete,
                arg: Box::new(self.parse_unary_operand()?),
            });
        }
        if (self.async_depth != 0 || self.module_await)
            && matches!(self.peek(), Token::Identifier(name) if name == "await")
        {
            if self.current_identifier_escaped() {
                return Err(self.syntax_error("the await keyword cannot contain an escape"));
            }
            self.advance();
            if matches!(
                self.peek(),
                Token::Punct(Punct::Semicolon | Punct::RBrace | Punct::RParen) | Token::Eof
            ) {
                return Err(self.syntax_error("await requires an operand"));
            }
            return Ok(Expr::Await(Box::new(self.parse_unary_operand()?)));
        }
        self.parse_update_expression()
    }

    /// `YieldExpression` occupies the `AssignmentExpression` grammar tier,
    /// not `UnaryExpression`.  Keeping this check at the unary boundary
    /// rejects `void yield` while retaining a top-level `yield value` and a
    /// parenthesized yield expression where the grammar admits one.
    pub(super) fn parse_unary_operand(&mut self) -> Result<Expr, ParseError> {
        let operand = self.parse_unary()?;
        if self.generator_depth != 0 && matches!(operand, Expr::Yield { .. }) {
            return Err(self.syntax_error("yield cannot be used as a unary operand"));
        }
        Ok(operand)
    }

    pub(super) fn parse_update_expression(&mut self) -> Result<Expr, ParseError> {
        let op = if self.eat_punct(Punct::PlusPlus) {
            Some(UpdateOp::Inc)
        } else if self.eat_punct(Punct::MinusMinus) {
            Some(UpdateOp::Dec)
        } else {
            None
        };
        if let Some(op) = op {
            let arg = self.parse_unary()?;
            if !is_valid_ref_target(&arg) && !is_annex_b_call_assignment_target(&arg) {
                return Err(self.syntax_error("invalid update operand"));
            }
            return Ok(Expr::Update {
                op,
                arg: Box::new(arg),
                prefix: true,
            });
        }
        self.parse_postfix()
    }

    pub(super) fn parse_postfix(&mut self) -> Result<Expr, ParseError> {
        let expr = self.parse_lhs_expression()?;
        // Postfix `++`/`--` is forbidden across a line terminator
        // (ASI); see `token.rs`'s `SpannedToken` doc comment.
        if !self.newline_before() {
            if self.check_punct(Punct::PlusPlus) {
                if !is_valid_ref_target(&expr) && !is_annex_b_call_assignment_target(&expr) {
                    return Err(self.syntax_error("invalid '++' operand"));
                }
                self.advance();
                return Ok(Expr::Update {
                    op: UpdateOp::Inc,
                    arg: Box::new(expr),
                    prefix: false,
                });
            }
            if self.check_punct(Punct::MinusMinus) {
                if !is_valid_ref_target(&expr) && !is_annex_b_call_assignment_target(&expr) {
                    return Err(self.syntax_error("invalid '--' operand"));
                }
                self.advance();
                return Ok(Expr::Update {
                    op: UpdateOp::Dec,
                    arg: Box::new(expr),
                    prefix: false,
                });
            }
        }
        Ok(expr)
    }

    pub(super) fn parse_lhs_expression(&mut self) -> Result<Expr, ParseError> {
        let mut expr = if self.check_keyword(Keyword::New)
            && matches!(self.peek_at(1), Token::Punct(Punct::Dot))
            && matches!(self.peek_at(2), Token::Identifier(name) if name == "target")
        {
            self.advance();
            self.advance();
            self.advance();
            Expr::NewTarget
        } else if self.eat_keyword(Keyword::New) {
            self.parse_new_expression()?
        } else {
            self.parse_primary()?
        };
        loop {
            if self.eat_punct(Punct::QuestionDot) {
                if self.check_punct(Punct::LParen) {
                    let args = self.parse_arguments()?;
                    expr = Expr::OptionalCall {
                        callee: Box::new(expr),
                        args,
                    };
                    continue;
                }
                if self.tokenizer.at_template(self.positions[self.pos]) {
                    return Err(known_syntax(self.syntax_error(
                        "an optional chain cannot be used as a template tag",
                    )));
                }
                let (property, computed) = if self.eat_punct(Punct::LBracket) {
                    let property = self.parse_expression()?;
                    self.expect_punct(Punct::RBracket)?;
                    (property, true)
                } else {
                    let name = self.expect_identifier_name()?;
                    (Expr::Identifier(name), false)
                };
                expr = Expr::OptionalMember {
                    object: Box::new(expr),
                    property: Box::new(property),
                    computed,
                };
            } else if self.eat_punct(Punct::Dot) {
                let name = self.expect_member_name()?;
                if matches!(expr, Expr::Super) && name.starts_with('#') {
                    return Err(self.syntax_error("super cannot access a private element"));
                }
                expr = Expr::Member {
                    object: Box::new(expr),
                    property: Box::new(Expr::Identifier(name)),
                    computed: false,
                };
            } else if self.eat_punct(Punct::LBracket) {
                let prop = self.parse_expression()?;
                self.expect_punct(Punct::RBracket)?;
                expr = Expr::Member {
                    object: Box::new(expr),
                    property: Box::new(prop),
                    computed: true,
                };
            } else if self.check_punct(Punct::LParen) {
                let args = self.parse_arguments()?;
                expr = Expr::Call {
                    callee: Box::new(expr),
                    args,
                };
            } else if self.tokenizer.at_template(self.positions[self.pos]) {
                if optional_chain_expression(&expr) {
                    return Err(known_syntax(self.syntax_error(
                        "an optional chain cannot be used as a template tag",
                    )));
                }
                let (raw, cooked, sources) = self
                    .tokenizer
                    .tagged_template_at(self.positions[self.pos])?;
                self.rescan_suffix();
                let expressions = sources
                    .iter()
                    .map(|source| parse_expression_from_source(source))
                    .collect::<Result<Vec<_>, _>>()?;
                expr = Expr::TaggedTemplate {
                    tag: Box::new(expr),
                    raw,
                    cooked,
                    expressions,
                };
            } else {
                break;
            }
        }
        Ok(expr)
    }

    /// `new` already consumed by the caller. Real ECMAScript's
    /// `MemberExpression`/`NewExpression` mutual recursion is
    /// approximated here rather than reproduced exactly: the callee is
    /// a primary expression plus member accesses (`new a.b.c(...)`,
    /// `new (a.b)(...)`) but not a call (`new a()()` parses `a()` as
    /// the callee only via the outer loop picking the trailing `()` up
    /// afterward, at [`Parser::parse_lhs_expression`]'s level) --
    /// covers every realistic `new` usage a hand-written DOM script
    /// makes without needing the spec's full grammar distinction.
    pub(super) fn parse_new_expression(&mut self) -> Result<Expr, ParseError> {
        // `new` takes a NewExpression/MemberExpression operand.  An
        // AwaitExpression is only admitted with explicit parentheses, as in
        // `new (await Constructor)`; `new await` is an early SyntaxError.
        if self.module_await && self.check_identifier("await") {
            return Err(self.syntax_error("await cannot immediately follow new"));
        }
        let mut callee = if self.eat_keyword(Keyword::New) {
            self.parse_new_expression()?
        } else {
            self.parse_primary()?
        };
        loop {
            if self.eat_punct(Punct::Dot) {
                let name = self.expect_member_name()?;
                callee = Expr::Member {
                    object: Box::new(callee),
                    property: Box::new(Expr::Identifier(name)),
                    computed: false,
                };
            } else if self.eat_punct(Punct::LBracket) {
                let prop = self.parse_expression()?;
                self.expect_punct(Punct::RBracket)?;
                callee = Expr::Member {
                    object: Box::new(callee),
                    property: Box::new(prop),
                    computed: true,
                };
            } else {
                break;
            }
        }
        let args = if self.check_punct(Punct::LParen) {
            self.parse_arguments()?
        } else {
            Vec::new()
        };
        Ok(Expr::New {
            callee: Box::new(callee),
            args,
        })
    }

    pub(super) fn parse_arguments(&mut self) -> Result<Vec<Argument>, ParseError> {
        self.expect_punct(Punct::LParen)?;
        let mut args = Vec::new();
        while !self.check_punct(Punct::RParen) {
            if self.eat_punct(Punct::Ellipsis) {
                args.push(Argument::Spread(self.parse_assignment()?));
            } else {
                args.push(Argument::Normal(self.parse_assignment()?));
            }
            if !self.check_punct(Punct::RParen) {
                self.expect_punct(Punct::Comma)?;
            }
        }
        self.expect_punct(Punct::RParen)?;
        Ok(args)
    }

    pub(super) fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        match self.peek().clone() {
            Token::Punct(Punct::Slash | Punct::SlashAssign) => {
                let (pattern, flags) = self.tokenizer.regexp_at(self.positions[self.pos])?;
                crate::regexp::RegExp::compile(pattern.clone(), &flags).map_err(|error| {
                    let resource = if matches!(error, crate::RuntimeError::SyntaxError(_)) {
                        None
                    } else {
                        Some(error.clone())
                    };
                    ParseError {
                        message: error.to_string(),
                        resource,
                        known_syntax: matches!(error, crate::RuntimeError::SyntaxError(_)),
                    }
                })?;
                self.rescan_suffix();
                Ok(Expr::RegExp { pattern, flags })
            }
            Token::Invalid(message) => Err(self.error(&message)),
            Token::Number(n) => {
                self.advance();
                Ok(Expr::Number(n))
            }
            Token::BigInt(n) => {
                self.advance();
                Ok(Expr::BigInt(n))
            }
            Token::String(s) => {
                self.advance();
                Ok(Expr::String(s))
            }
            Token::Template {
                quasis,
                raw_expressions,
            } => {
                self.advance();
                parse_template(quasis, raw_expressions)
            }
            Token::Keyword(Keyword::True) => {
                self.advance();
                Ok(Expr::Bool(true))
            }
            Token::Keyword(Keyword::False) => {
                self.advance();
                Ok(Expr::Bool(false))
            }
            Token::Keyword(Keyword::Null) => {
                self.advance();
                Ok(Expr::Null)
            }
            Token::Keyword(Keyword::This) => {
                self.advance();
                Ok(Expr::This)
            }
            Token::Keyword(Keyword::Function) => {
                self.advance();
                Ok(Expr::Function(self.parse_function()?))
            }
            Token::Identifier(name) if name == "async" && self.async_function_follows() => {
                self.require_unescaped_async()?;
                self.advance();
                self.expect_keyword(Keyword::Function)?;
                Ok(Expr::Function(self.parse_function_with_async(true)?))
            }
            Token::Identifier(name) if name == "class" => {
                self.advance();
                Ok(Expr::Class(self.parse_class()?))
            }
            Token::Identifier(name) if name == "super" => {
                self.advance();
                Ok(Expr::Super)
            }
            Token::Identifier(name) if name == "yield" && self.generator_depth != 0 => {
                self.advance();
                // YieldExpression forbids a LineTerminator before `*`. It
                // cannot instead be parsed as a multiplicative expression:
                // yield is an AssignmentExpression, not a PrimaryExpression.
                if self.newline_before() && self.check_punct(Punct::Star) {
                    return Err(self.syntax_error("yield* cannot contain a line terminator"));
                }
                let delegate = self.eat_punct(Punct::Star);
                let value = if !delegate
                    && (self.newline_before()
                        || matches!(
                            self.peek(),
                            Token::Punct(
                                Punct::Semicolon
                                    | Punct::Comma
                                    | Punct::RBrace
                                    | Punct::RBracket
                                    | Punct::RParen
                            ) | Token::Eof
                        )) {
                    None
                } else {
                    Some(Box::new(self.parse_assignment()?))
                };
                Ok(Expr::Yield { value, delegate })
            }
            Token::Identifier(name)
                if name == "import" && self.check_punct_at(1, Punct::LParen) =>
            {
                self.advance();
                self.expect_punct(Punct::LParen)?;
                if self.check_punct(Punct::RParen) {
                    return Err(self.syntax_error("import() requires a module specifier"));
                }
                let specifier = self.parse_assignment()?;
                self.expect_punct(Punct::RParen)?;
                Ok(Expr::DynamicImport(Box::new(specifier)))
            }
            Token::Identifier(name)
                if name == "import"
                    && self.check_punct_at(1, Punct::Dot)
                    && self.check_identifier_at(2, "meta") =>
            {
                self.advance();
                self.advance();
                self.advance();
                if !self.module {
                    return Err(self.syntax_error("import.meta is only valid in module code"));
                }
                Ok(Expr::ImportMeta)
            }
            Token::Identifier(name)
                if name == "await"
                    && self.async_depth == 0
                    && !self.module_await
                    // A call/member suffix belongs to the IdentifierReference
                    // `await`; it is not a second primary expression.
                    && !matches!(
                        self.peek_at(1),
                        Token::Punct(
                            Punct::LParen | Punct::LBracket | Punct::Dot | Punct::QuestionDot
                        )
                    )
                    && self.token_starts_expression(1) =>
            {
                // In a non-async function, `await` is an IdentifierReference.
                // A second primary expression cannot follow it without an
                // operator, so this is grammar-invalid, not an unsupported
                // await production (e.g. `function f() { await 0; }`).
                Err(self.syntax_error("unexpected expression after await identifier"))
            }
            Token::Identifier(name) => {
                self.advance();
                Ok(Expr::Identifier(name))
            }
            Token::Punct(Punct::LParen) => {
                self.advance();
                let expr = self.parse_expression()?;
                self.expect_punct(Punct::RParen).map_err(known_syntax)?;
                if is_assignment_operator(self.peek()) || optional_chain_expression(&expr) {
                    Ok(Expr::Parenthesized(Box::new(expr)))
                } else {
                    Ok(expr)
                }
            }
            Token::Punct(Punct::LBracket) => self.parse_array_literal(),
            Token::Punct(Punct::LBrace) => self.parse_object_literal(),
            Token::Punct(Punct::Assign | Punct::Star | Punct::Question) => {
                Err(self.syntax_error("expected an expression"))
            }
            _ => Err(self.error("expected an expression")),
        }
    }

    pub(super) fn token_starts_expression(&self, offset: usize) -> bool {
        matches!(
            self.peek_at(offset),
            Token::Number(_)
                | Token::BigInt(_)
                | Token::String(_)
                | Token::Template { .. }
                | Token::Identifier(_)
                | Token::Keyword(
                    Keyword::True
                        | Keyword::False
                        | Keyword::Null
                        | Keyword::This
                        | Keyword::Function
                        | Keyword::New
                )
                | Token::Punct(Punct::LParen | Punct::LBracket | Punct::LBrace)
        )
    }

    pub(super) fn parse_array_literal(&mut self) -> Result<Expr, ParseError> {
        self.expect_punct(Punct::LBracket)?;
        let mut elements = Vec::new();
        while !self.check_punct(Punct::RBracket) {
            if self.check_punct(Punct::Comma) {
                self.advance();
                elements.push(None);
                continue;
            }
            if self.eat_punct(Punct::Ellipsis) {
                elements.push(Some(ArrayElement::Spread(self.parse_assignment()?)));
            } else {
                elements.push(Some(ArrayElement::Normal(self.parse_assignment()?)));
            }
            if !self.check_punct(Punct::RBracket) {
                self.expect_punct(Punct::Comma)?;
            }
        }
        self.expect_punct(Punct::RBracket)?;
        Ok(Expr::Array(elements))
    }

    pub(super) fn parse_object_literal(&mut self) -> Result<Expr, ParseError> {
        self.expect_punct(Punct::LBrace)?;
        let mut props = Vec::new();
        while !self.check_punct(Punct::RBrace) {
            if self.eat_punct(Punct::Ellipsis) {
                props.push(ObjectProp::Spread(self.parse_assignment()?));
            } else {
                let is_async = self.object_async_method_follows();
                if is_async {
                    self.advance();
                }
                let generator = self.eat_punct(Punct::Star);
                let key = self.parse_property_key()?;
                if self.eat_punct(Punct::Colon) {
                    if is_async || generator {
                        return Err(self.error("invalid object method"));
                    }
                    let value = self.parse_assignment()?;
                    props.push(ObjectProp::KeyValue {
                        key,
                        value,
                        shorthand: false,
                    });
                } else if self.check_punct(Punct::LParen) {
                    let name = class_element_name(&key);
                    props.push(ObjectProp::Method {
                        key,
                        function: self.parse_method_function(Some(name), generator, is_async)?,
                    });
                } else if matches!(&key, PropertyKey::Identifier(name) if name == "get" || name == "set")
                    && !self.check_punct(Punct::Comma)
                    && !self.check_punct(Punct::RBrace)
                {
                    if is_async || generator {
                        return Err(self.error("invalid object accessor"));
                    }
                    let getter = matches!(&key, PropertyKey::Identifier(name) if name == "get");
                    let key = self.parse_property_key()?;
                    let name = class_element_name(&key);
                    let function = self.parse_method_function(
                        Some(format!("{} {}", if getter { "get" } else { "set" }, name)),
                        false,
                        false,
                    )?;
                    if (getter && !function.params.is_empty())
                        || (!getter && (function.params.len() != 1 || function.params[0].rest))
                    {
                        return Err(self.error("invalid accessor parameter list"));
                    }
                    props.push(ObjectProp::Accessor {
                        key,
                        function,
                        getter,
                    });
                } else {
                    if is_async || generator {
                        return Err(self.error("expected object method parameters"));
                    }
                    let name = match &key {
                        PropertyKey::Identifier(n) => n.clone(),
                        _ => return Err(self.error("expected ':' after object property key")),
                    };
                    props.push(ObjectProp::KeyValue {
                        key: PropertyKey::Identifier(name.clone()),
                        value: Expr::Identifier(name),
                        shorthand: true,
                    });
                }
            }
            if !self.check_punct(Punct::RBrace) {
                self.expect_punct(Punct::Comma)?;
            }
        }
        self.expect_punct(Punct::RBrace)?;
        Ok(Expr::Object(props))
    }
}
