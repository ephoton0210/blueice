// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Direct-expression lowering into the BlueJS AST.

use super::*;

mod calls;
mod writes;

pub(super) struct ExpressionLowerer<'a> {
    pub(super) module: &'a str,
    pub(super) module_ast: Option<&'a Module>,
    pub(super) tokens: &'a [Token],
    pub(super) index: usize,
}

impl<'a> ExpressionLowerer<'a> {
    pub(super) fn new(module: &'a str, tokens: &'a [Token]) -> Self {
        Self {
            module,
            module_ast: None,
            tokens,
            index: 0,
        }
    }

    pub(super) fn for_module(module: &'a Module, tokens: &'a [Token]) -> Self {
        Self {
            module: &module.id,
            module_ast: Some(module),
            tokens,
            index: 0,
        }
    }

    pub(super) fn parse(mut self) -> Result<bluejs::Expr, BridgeError> {
        if let Some(module) = self.module_ast {
            let spans = module.erased_assertion_spans().collect::<Vec<_>>();
            let erased = |token: &Token| {
                spans
                    .iter()
                    .any(|span| span.start <= token.start && token.end <= span.end)
            };
            if self.tokens.iter().any(erased) {
                let tokens = self
                    .tokens
                    .iter()
                    .filter(|token| !erased(token))
                    .cloned()
                    .collect::<Vec<_>>();
                return ExpressionLowerer::for_module(module, &tokens).parse();
            }
        }
        let expression = self.parse_sequence()?;
        if let Some(token) = self.tokens.get(self.index) {
            return Err(unsupported(
                self.token_span(token),
                format!("unsupported expression token `{}`", token.text),
            ));
        }
        Ok(expression)
    }

    pub(super) fn parse_sequence(&mut self) -> Result<bluejs::Expr, BridgeError> {
        let first = self.parse_assignment()?;
        if self
            .tokens
            .get(self.index)
            .is_none_or(|token| token.text != ",")
        {
            return Ok(first);
        }
        let mut expressions = vec![first];
        while self
            .tokens
            .get(self.index)
            .is_some_and(|token| token.text == ",")
        {
            self.index += 1;
            expressions.push(self.parse_assignment()?);
        }
        Ok(bluejs::Expr::Sequence(expressions))
    }

    pub(super) fn parse_conditional(&mut self) -> Result<bluejs::Expr, BridgeError> {
        let test = self.parse_nullish()?;
        if self
            .tokens
            .get(self.index)
            .is_none_or(|token| token.text != "?")
        {
            return Ok(test);
        }
        self.index += 1;
        let consequent = self.parse_assignment()?;
        let Some(colon) = self.tokens.get(self.index) else {
            return Err(unsupported(
                SourceSpan::new(self.module, 0, 0),
                "unterminated conditional expression",
            ));
        };
        if colon.text != ":" {
            return Err(unsupported(
                self.token_span(colon),
                "expected `:` in conditional expression",
            ));
        }
        self.index += 1;
        Ok(bluejs::Expr::Conditional {
            test: Box::new(test),
            consequent: Box::new(consequent),
            alternate: Box::new(self.parse_assignment()?),
        })
    }

    pub(super) fn parse_nullish(&mut self) -> Result<bluejs::Expr, BridgeError> {
        let (mut expression, logical) = self.parse_logical_or()?;
        while self
            .tokens
            .get(self.index)
            .is_some_and(|token| token.text == "??")
        {
            let coalescing_span = self
                .tokens
                .get(self.index)
                .map(|token| self.token_span(token))
                .expect("a nullish-coalescing operator was just inspected");
            if logical {
                return Err(unsupported(
                    coalescing_span,
                    "parentheses are required when mixing `??` with `&&` or `||`",
                ));
            }
            self.index += 1;
            let (right, logical_right) = self.parse_logical_or()?;
            if logical_right {
                return Err(unsupported(
                    coalescing_span,
                    "parentheses are required when mixing `??` with `&&` or `||`",
                ));
            }
            expression = bluejs::Expr::Logical {
                op: bluejs::LogicalOp::Nullish,
                left: Box::new(expression),
                right: Box::new(right),
            };
        }
        Ok(expression)
    }

    pub(super) fn parse_logical_or(&mut self) -> Result<(bluejs::Expr, bool), BridgeError> {
        let (mut expression, mut logical) = self.parse_logical_and()?;
        while self
            .tokens
            .get(self.index)
            .is_some_and(|token| token.text == "||")
        {
            self.index += 1;
            expression = bluejs::Expr::Logical {
                op: bluejs::LogicalOp::Or,
                left: Box::new(expression),
                right: Box::new(self.parse_logical_and()?.0),
            };
            logical = true;
        }
        Ok((expression, logical))
    }

    pub(super) fn parse_logical_and(&mut self) -> Result<(bluejs::Expr, bool), BridgeError> {
        let mut expression = self.parse_bitwise_or()?;
        let mut logical = false;
        while self
            .tokens
            .get(self.index)
            .is_some_and(|token| token.text == "&&")
        {
            self.index += 1;
            expression = bluejs::Expr::Logical {
                op: bluejs::LogicalOp::And,
                left: Box::new(expression),
                right: Box::new(self.parse_bitwise_or()?),
            };
            logical = true;
        }
        Ok((expression, logical))
    }

    pub(super) fn parse_equality(&mut self) -> Result<bluejs::Expr, BridgeError> {
        let mut expression = self.parse_relational()?;
        while let Some(token) = self.tokens.get(self.index) {
            let op = match token.text.as_str() {
                "==" => bluejs::BinaryOp::Eq,
                "!=" => bluejs::BinaryOp::NotEq,
                "===" => bluejs::BinaryOp::StrictEq,
                "!==" => bluejs::BinaryOp::StrictNotEq,
                _ => break,
            };
            self.index += 1;
            expression = bluejs::Expr::Binary {
                op,
                left: Box::new(expression),
                right: Box::new(self.parse_relational()?),
            };
        }
        Ok(expression)
    }

    pub(super) fn parse_bitwise_or(&mut self) -> Result<bluejs::Expr, BridgeError> {
        let mut expression = self.parse_bitwise_xor()?;
        while self
            .tokens
            .get(self.index)
            .is_some_and(|token| token.text == "|")
        {
            self.index += 1;
            expression = bluejs::Expr::Binary {
                op: bluejs::BinaryOp::BitOr,
                left: Box::new(expression),
                right: Box::new(self.parse_bitwise_xor()?),
            };
        }
        Ok(expression)
    }

    pub(super) fn parse_bitwise_xor(&mut self) -> Result<bluejs::Expr, BridgeError> {
        let mut expression = self.parse_bitwise_and()?;
        while self
            .tokens
            .get(self.index)
            .is_some_and(|token| token.text == "^")
        {
            self.index += 1;
            expression = bluejs::Expr::Binary {
                op: bluejs::BinaryOp::BitXor,
                left: Box::new(expression),
                right: Box::new(self.parse_bitwise_and()?),
            };
        }
        Ok(expression)
    }

    pub(super) fn parse_bitwise_and(&mut self) -> Result<bluejs::Expr, BridgeError> {
        let mut expression = self.parse_equality()?;
        while self
            .tokens
            .get(self.index)
            .is_some_and(|token| token.text == "&")
        {
            self.index += 1;
            expression = bluejs::Expr::Binary {
                op: bluejs::BinaryOp::BitAnd,
                left: Box::new(expression),
                right: Box::new(self.parse_equality()?),
            };
        }
        Ok(expression)
    }

    pub(super) fn parse_relational(&mut self) -> Result<bluejs::Expr, BridgeError> {
        // `#name in object` is a distinct relational form: a private name
        // cannot otherwise begin an expression. BlueJS keeps the name without
        // its `#`.
        let mut expression = if let Some(name) = self
            .tokens
            .get(self.index)
            .filter(|token| token.text.starts_with('#'))
            .filter(|_| {
                self.tokens
                    .get(self.index + 1)
                    .is_some_and(|next| next.text == "in")
            })
            .map(|token| token.text[1..].to_string())
        {
            self.index += 2;
            bluejs::Expr::PrivateIn {
                name,
                object: Box::new(self.parse_shift()?),
            }
        } else {
            self.parse_shift()?
        };
        while let Some(token) = self.tokens.get(self.index) {
            if self.shift_operator_at(self.index).is_some() {
                break;
            }
            let op = match token.text.as_str() {
                "<" => bluejs::BinaryOp::Lt,
                ">" => bluejs::BinaryOp::Gt,
                "<=" => bluejs::BinaryOp::LtEq,
                ">=" => bluejs::BinaryOp::GtEq,
                "in" => bluejs::BinaryOp::In,
                "instanceof" => bluejs::BinaryOp::Instanceof,
                _ => break,
            };
            self.index += 1;
            expression = bluejs::Expr::Binary {
                op,
                left: Box::new(expression),
                right: Box::new(self.parse_shift()?),
            };
        }
        Ok(expression)
    }

    pub(super) fn parse_shift(&mut self) -> Result<bluejs::Expr, BridgeError> {
        let mut expression = self.parse_additive()?;
        while let Some((op, width)) = self.shift_operator_at(self.index) {
            self.index += width;
            expression = bluejs::Expr::Binary {
                op,
                left: Box::new(expression),
                right: Box::new(self.parse_additive()?),
            };
        }
        Ok(expression)
    }

    pub(super) fn parse_additive(&mut self) -> Result<bluejs::Expr, BridgeError> {
        let mut expression = self.parse_multiplicative()?;
        while let Some(token) = self.tokens.get(self.index) {
            let op = match token.text.as_str() {
                "+" => bluejs::BinaryOp::Add,
                "-" => bluejs::BinaryOp::Sub,
                _ => break,
            };
            self.index += 1;
            expression = bluejs::Expr::Binary {
                op,
                left: Box::new(expression),
                right: Box::new(self.parse_multiplicative()?),
            };
        }
        Ok(expression)
    }

    pub(super) fn parse_multiplicative(&mut self) -> Result<bluejs::Expr, BridgeError> {
        let mut expression = self.parse_exponentiation()?;
        while let Some(token) = self.tokens.get(self.index) {
            let op = match token.text.as_str() {
                "*" => bluejs::BinaryOp::Mul,
                "/" => bluejs::BinaryOp::Div,
                "%" => bluejs::BinaryOp::Mod,
                _ => break,
            };
            self.index += 1;
            expression = bluejs::Expr::Binary {
                op,
                left: Box::new(expression),
                right: Box::new(self.parse_exponentiation()?),
            };
        }
        Ok(expression)
    }

    pub(super) fn parse_exponentiation(&mut self) -> Result<bluejs::Expr, BridgeError> {
        let unary_base = self.starts_unary_expression();
        let expression = if unary_base {
            self.parse_unary()?
        } else {
            self.parse_update()?
        };
        if self
            .tokens
            .get(self.index)
            .is_none_or(|token| token.text != "**")
        {
            return Ok(expression);
        }
        let span = self
            .tokens
            .get(self.index)
            .map(|token| self.token_span(token))
            .expect("an exponentiation operator was just inspected");
        if unary_base {
            return Err(unsupported(
                span,
                "a unary expression cannot be the unparenthesized base of exponentiation",
            ));
        }
        self.index += 1;
        Ok(bluejs::Expr::Binary {
            op: bluejs::BinaryOp::Exponent,
            left: Box::new(expression),
            right: Box::new(self.parse_exponentiation()?),
        })
    }

    pub(super) fn starts_unary_expression(&self) -> bool {
        self.tokens.get(self.index).is_some_and(|token| {
            matches!(
                token.text.as_str(),
                "!" | "+" | "-" | "~" | "typeof" | "void" | "delete" | "await"
            )
        })
    }

    pub(super) fn parse_unary(&mut self) -> Result<bluejs::Expr, BridgeError> {
        // `await operand`: valid in an async function and at a module's top level,
        // which BlueTS has already established before the bridge sees the token.
        if self
            .tokens
            .get(self.index)
            .is_some_and(|token| token.text == "await")
        {
            self.index += 1;
            return Ok(bluejs::Expr::Await(Box::new(self.parse_unary()?)));
        }
        let op = self
            .tokens
            .get(self.index)
            .and_then(|token| match token.text.as_str() {
                "!" => Some(bluejs::UnaryOp::Not),
                "+" => Some(bluejs::UnaryOp::Plus),
                "-" => Some(bluejs::UnaryOp::Neg),
                "~" => Some(bluejs::UnaryOp::BitNot),
                "typeof" => Some(bluejs::UnaryOp::Typeof),
                "void" => Some(bluejs::UnaryOp::Void),
                "delete" => Some(bluejs::UnaryOp::Delete),
                _ => None,
            });
        if let Some(op) = op {
            let span = self
                .tokens
                .get(self.index)
                .map(|token| self.token_span(token))
                .expect("a unary operator was just inspected");
            self.index += 1;
            let arg = self.parse_unary()?;
            if op == bluejs::UnaryOp::Delete && !matches!(arg, bluejs::Expr::Member { .. }) {
                return Err(unsupported(
                    span,
                    "only property delete targets are in the v1 direct bridge subset",
                ));
            }
            return Ok(bluejs::Expr::Unary {
                op,
                arg: Box::new(arg),
            });
        }
        self.parse_update()
    }

    pub(super) fn parse_primary(&mut self) -> Result<bluejs::Expr, BridgeError> {
        let expression = self.parse_atom()?;
        self.parse_suffixes(expression)
    }

    pub(super) fn parse_atom(&mut self) -> Result<bluejs::Expr, BridgeError> {
        let Some(token) = self.tokens.get(self.index) else {
            return Err(unsupported(
                SourceSpan::new(self.module, 0, 0),
                "expected a runtime expression",
            ));
        };
        if let Some((module, expression)) = self.module_ast.and_then(|module| {
            module
                .class_expression(token.start)
                .map(|expression| (module, expression))
        }) {
            let bluejs::Stmt::ClassDecl(mut class) = lowering::classes::lower_class(
                module,
                &expression.class,
                jsx_direct::defines_class_fields(),
            )?
            else {
                unreachable!("class lowering produces a class declaration");
            };
            class.name = expression.name.clone();
            self.index = self
                .tokens
                .partition_point(|token| token.start < expression.class.span.end);
            return Ok(bluejs::Expr::Class(class));
        }
        self.index += 1;
        match token.kind {
            TokenKind::Number => token
                .text
                .replace('_', "")
                .parse::<f64>()
                .map(bluejs::Expr::Number)
                .map_err(|_| unsupported(self.token_span(token), "unsupported numeric literal")),
            TokenKind::String => lower_string(self.module, token),
            TokenKind::Template => lower_template(self.module, token),
            TokenKind::JsxElement => self.lower_jsx_element(token),
            TokenKind::Identifier => Ok(bluejs::Expr::Identifier(token.text.clone())),
            TokenKind::Keyword => match token.text.as_str() {
                "true" => Ok(bluejs::Expr::Bool(true)),
                "false" => Ok(bluejs::Expr::Bool(false)),
                "null" => Ok(bluejs::Expr::Null),
                "undefined" => Ok(bluejs::Expr::Identifier("undefined".to_string())),
                "new" => self.parse_new_expression(token),
                "this" => Ok(bluejs::Expr::This),
                // `super(..)` and `super.member` are an ordinary call and
                // member access on this node; BlueJS checks where it may occur.
                "super" => Ok(bluejs::Expr::Super),
                _ => Err(unsupported(
                    self.token_span(token),
                    format!(
                        "unsupported keyword `{}` in a runtime expression",
                        token.text
                    ),
                )),
            },
            TokenKind::Punct if token.text == "(" => {
                let expression = self.parse_sequence()?;
                let Some(closing) = self.tokens.get(self.index) else {
                    return Err(unsupported(
                        self.token_span(token),
                        "unterminated parenthesized expression",
                    ));
                };
                if closing.text != ")" {
                    return Err(unsupported(
                        self.token_span(closing),
                        "expected `)` in runtime expression",
                    ));
                }
                self.index += 1;
                Ok(bluejs::Expr::Parenthesized(Box::new(expression)))
            }
            TokenKind::Punct if token.text == "[" => self.parse_array_literal(token),
            TokenKind::Punct if token.text == "{" => self.parse_object_literal(token),
            _ => Err(unsupported(
                self.token_span(token),
                format!("unsupported runtime expression token `{}`", token.text),
            )),
        }
    }

    pub(super) fn parse_array_literal(
        &mut self,
        opening: &Token,
    ) -> Result<bluejs::Expr, BridgeError> {
        let opening_span = self.token_span(opening);
        let mut elements = Vec::new();
        loop {
            let Some(token) = self.tokens.get(self.index) else {
                return Err(unsupported(opening_span, "unterminated array literal"));
            };
            if token.text == "]" {
                self.index += 1;
                return Ok(bluejs::Expr::Array(elements));
            }
            if token.text == "," {
                if elements
                    .iter()
                    .any(|element| matches!(element, Some(bluejs::ArrayElement::Spread(_))))
                {
                    return Err(unsupported(
                        self.token_span(token),
                        "array literals cannot combine holes and spread elements in the v1 direct bridge subset",
                    ));
                }
                self.index += 1;
                elements.push(None);
                continue;
            }
            let element = if token.text == "..." {
                if elements.iter().any(Option::is_none) {
                    return Err(unsupported(
                        self.token_span(token),
                        "array literals cannot combine holes and spread elements in the v1 direct bridge subset",
                    ));
                }
                self.index += 1;
                bluejs::ArrayElement::Spread(self.parse_assignment()?)
            } else {
                bluejs::ArrayElement::Normal(self.parse_assignment()?)
            };
            elements.push(Some(element));
            let Some(separator) = self.tokens.get(self.index) else {
                return Err(unsupported(opening_span, "unterminated array literal"));
            };
            match separator.text.as_str() {
                "," => self.index += 1,
                "]" => {
                    self.index += 1;
                    return Ok(bluejs::Expr::Array(elements));
                }
                _ => {
                    return Err(unsupported(
                        self.token_span(separator),
                        "expected `,` or `]` in array literal",
                    ));
                }
            }
        }
    }

    pub(super) fn parse_object_literal(
        &mut self,
        opening: &Token,
    ) -> Result<bluejs::Expr, BridgeError> {
        let opening_span = self.token_span(opening);
        let mut properties = Vec::new();
        loop {
            let Some(token) = self.tokens.get(self.index).cloned() else {
                return Err(unsupported(opening_span, "unterminated object literal"));
            };
            if token.text == "}" {
                self.index += 1;
                return Ok(bluejs::Expr::Object(properties));
            }
            if token.text == "..." {
                self.index += 1;
                properties.push(bluejs::ObjectProp::Spread(self.parse_assignment()?));
            } else {
                let (key, shorthand_name) = if token.text == "[" {
                    let key_span = self.token_span(&token);
                    self.index += 1;
                    let expression = self.parse_assignment()?;
                    let Some(closing) = self.tokens.get(self.index) else {
                        return Err(unsupported(
                            key_span,
                            "unterminated computed object property key",
                        ));
                    };
                    if closing.text != "]" {
                        return Err(unsupported(
                            self.token_span(closing),
                            "expected `]` after a computed object property key",
                        ));
                    }
                    self.index += 1;
                    (bluejs::PropertyKey::Computed(Box::new(expression)), None)
                } else {
                    let (key, shorthand_name) = match token.kind {
                        TokenKind::Identifier => (
                            bluejs::PropertyKey::Identifier(token.text.clone()),
                            Some(token.text.clone()),
                        ),
                        TokenKind::String => match lower_string(self.module, &token)? {
                            bluejs::Expr::String(value) => {
                                (bluejs::PropertyKey::String(value), None)
                            }
                            _ => {
                                unreachable!(
                                    "string lowering always constructs a string expression"
                                )
                            }
                        },
                        TokenKind::Number => (
                            bluejs::PropertyKey::Number(
                                token.text.replace('_', "").parse().map_err(|_| {
                                    unsupported(
                                        self.token_span(&token),
                                        "unsupported numeric object key",
                                    )
                                })?,
                            ),
                            None,
                        ),
                        _ => {
                            return Err(unsupported(
                                self.token_span(&token),
                                "only identifier, string, numeric, and computed object property keys are in the v1 direct bridge subset",
                            ));
                        }
                    };
                    self.index += 1;
                    (key, shorthand_name)
                };
                let Some(colon) = self.tokens.get(self.index) else {
                    return Err(unsupported(opening_span, "unterminated object literal"));
                };
                let (value, shorthand) = if colon.text == ":" {
                    self.index += 1;
                    (self.parse_assignment()?, false)
                } else if matches!(colon.text.as_str(), "," | "}") && shorthand_name.is_some() {
                    (bluejs::Expr::Identifier(shorthand_name.unwrap()), true)
                } else {
                    return Err(unsupported(
                        self.token_span(colon),
                        "only identifier object keys may use shorthand; methods are not in the v1 direct bridge subset",
                    ));
                };
                properties.push(bluejs::ObjectProp::KeyValue {
                    key,
                    value,
                    shorthand,
                });
            }
            let Some(separator) = self.tokens.get(self.index) else {
                return Err(unsupported(opening_span, "unterminated object literal"));
            };
            match separator.text.as_str() {
                "," => self.index += 1,
                "}" => {
                    self.index += 1;
                    return Ok(bluejs::Expr::Object(properties));
                }
                _ => {
                    return Err(unsupported(
                        self.token_span(separator),
                        "expected `,` or `}` in object literal",
                    ));
                }
            }
        }
    }

    pub(super) fn shift_operator_at(&self, index: usize) -> Option<(bluejs::BinaryOp, usize)> {
        if self
            .tokens
            .get(index)
            .is_some_and(|token| token.text == "<<")
        {
            return Some((bluejs::BinaryOp::ShiftLeft, 1));
        }
        let first = self.tokens.get(index)?;
        let second = self.tokens.get(index + 1)?;
        if first.text != ">" || second.text != ">" || first.end != second.start {
            return None;
        }
        if let Some(third) = self.tokens.get(index + 2) {
            if third.text == ">" && second.end == third.start {
                return Some((bluejs::BinaryOp::UnsignedShiftRight, 3));
            }
        }
        Some((bluejs::BinaryOp::ShiftRight, 2))
    }

    pub(super) fn token_span(&self, token: &Token) -> SourceSpan {
        token_span(self.module, token)
    }
}

pub(super) fn lower_string(module: &str, token: &Token) -> Result<bluejs::Expr, BridgeError> {
    let mut characters = token.text.chars();
    let quote = characters
        .next()
        .filter(|quote| matches!(quote, '\'' | '\"'))
        .ok_or_else(|| unsupported(token_span(module, token), "invalid string token"))?;
    let body = characters
        .next_back()
        .filter(|last| *last == quote)
        .map(|_| &token.text[quote.len_utf8()..token.text.len() - quote.len_utf8()])
        .ok_or_else(|| unsupported(token_span(module, token), "unterminated string token"))?;
    Ok(bluejs::Expr::String(
        decode_string_escapes(module, token, body)?.into(),
    ))
}

pub(super) fn decode_string_escapes(
    module: &str,
    token: &Token,
    body: &str,
) -> Result<String, BridgeError> {
    let mut output = String::new();
    let mut characters = body.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            output.push(character);
            continue;
        }
        let Some(escape) = characters.next() else {
            return Err(unsupported(
                token_span(module, token),
                "unterminated string escape",
            ));
        };
        output.push(match escape {
            '\\' => '\\',
            '\'' => '\'',
            '"' => '"',
            '`' => '`',
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            'b' => '\u{0008}',
            'f' => '\u{000C}',
            'v' => '\u{000B}',
            '0' => '\0',
            _ => {
                return Err(unsupported(
                    token_span(module, token),
                    "unsupported string escape in the v1 direct bridge subset",
                ));
            }
        });
    }
    Ok(output)
}

pub(super) fn lower_template(module: &str, token: &Token) -> Result<bluejs::Expr, BridgeError> {
    let body = token
        .text
        .strip_prefix('`')
        .and_then(|text| text.strip_suffix('`'))
        .ok_or_else(|| unsupported(token_span(module, token), "invalid template token"))?;
    let mut quasis = Vec::new();
    let mut expressions = Vec::new();
    let mut remainder = body;
    let mut remainder_offset = 1usize;
    while let Some(start) = template_substitution_start(remainder) {
        quasis.push(decode_string_escapes(module, token, &remainder[..start])?.into());
        let expression_start = start + 2;
        let Some(end) = template_substitution_end(&remainder[expression_start..]) else {
            return Err(unsupported(
                token_span(module, token),
                "unterminated template substitution",
            ));
        };
        let expression = &remainder[expression_start..expression_start + end];
        expressions.push(lower_template_substitution(
            module,
            token,
            expression,
            remainder_offset + expression_start,
        )?);
        remainder_offset += expression_start + end + 1;
        remainder = &remainder[expression_start + end + 1..];
    }
    quasis.push(decode_string_escapes(module, token, remainder)?.into());
    Ok(bluejs::Expr::Template {
        quasis,
        expressions,
    })
}

pub(super) fn template_substitution_start(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut index = 0usize;
    while index + 1 < bytes.len() {
        if bytes[index] == b'\\' {
            index += 2;
        } else if bytes[index] == b'$' && bytes[index + 1] == b'{' {
            return Some(index);
        } else {
            index += 1;
        }
    }
    None
}

pub(super) fn template_substitution_end(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut index = 0usize;
    let mut depth = 1usize;
    let mut quote = None;
    while index < bytes.len() {
        if let Some(delimiter) = quote {
            if bytes[index] == b'\\' {
                index += 2;
                continue;
            }
            if bytes[index] == delimiter {
                quote = None;
            }
            index += 1;
            continue;
        }
        match bytes[index] {
            b'\'' | b'"' => quote = Some(bytes[index]),
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
        index += 1;
    }
    None
}

pub(super) fn lower_template_substitution(
    module: &str,
    template: &Token,
    source: &str,
    source_offset: usize,
) -> Result<bluejs::Expr, BridgeError> {
    if source.is_empty() {
        return Err(unsupported(
            token_span(module, template),
            "empty template substitutions are not expressions",
        ));
    }
    let mut tokens = lex(module, source).map_err(|_| {
        unsupported(
            token_span(module, template),
            "invalid expression in a template substitution",
        )
    })?;
    let eof = tokens
        .pop()
        .expect("BlueTS lexer always terminates with EOF");
    debug_assert_eq!(eof.kind, TokenKind::Eof);
    if tokens.is_empty() {
        return Err(unsupported(
            token_span(module, template),
            "empty template substitutions are not expressions",
        ));
    }
    for token in &mut tokens {
        token.start += template.start + source_offset;
        token.end += template.start + source_offset;
    }
    if let Some(optional) = tokens.iter().find(|token| token.is("?.")) {
        return Err(unsupported(
            token_span(module, optional),
            "unsupported expression token `?.` in a template substitution",
        ));
    }
    ExpressionLowerer::new(module, &tokens).parse()
}

pub(super) fn token_span(module: &str, token: &Token) -> SourceSpan {
    SourceSpan::new(module, token.start, token.end)
}
