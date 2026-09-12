// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Parser {
    // ---- Functions ----

    pub(super) fn parse_params(&mut self) -> Result<Vec<Param>, ParseError> {
        self.expect_punct(Punct::LParen)?;
        let mut params = Vec::new();
        while !self.check_punct(Punct::RParen) {
            if self.eat_punct(Punct::Ellipsis) {
                let pattern = self.parse_binding_pattern()?;
                if self.eat_punct(Punct::Assign) {
                    return Err(self.syntax_error("a rest parameter cannot have a default value"));
                }
                params.push(Param {
                    pattern,
                    default: None,
                    rest: true,
                });
                if self.eat_punct(Punct::Comma) {
                    return Err(self.syntax_error("a rest parameter cannot have a trailing comma"));
                }
                break;
            } else {
                let pattern = self.parse_binding_pattern()?;
                let default = if self.eat_punct(Punct::Assign) {
                    if self.async_depth != 0
                        && matches!(self.peek(), Token::Identifier(name) if name == "await")
                    {
                        return Err(self.syntax_error(
                            "await is not allowed in an async function parameter initializer",
                        ));
                    }
                    if self.generator_depth != 0
                        && matches!(self.peek(), Token::Identifier(name) if name == "yield")
                    {
                        return Err(self.syntax_error(
                            "yield is not allowed in a generator parameter initializer",
                        ));
                    }
                    Some(self.parse_assignment()?)
                } else {
                    None
                };
                params.push(Param {
                    pattern,
                    default,
                    rest: false,
                });
            }
            if !self.check_punct(Punct::RParen) {
                self.expect_punct(Punct::Comma)?;
            }
        }
        self.expect_punct(Punct::RParen)?;
        Ok(params)
    }

    /// Parses a `function` declaration/expression body -- the
    /// `function` keyword itself is already consumed by the caller,
    /// since callers need to branch on it first (statement vs.
    /// expression position).
    pub(super) fn parse_function(&mut self) -> Result<Function, ParseError> {
        self.parse_function_with_async(false)
    }

    pub(super) fn parse_function_with_async(
        &mut self,
        is_async: bool,
    ) -> Result<Function, ParseError> {
        let generator = self.eat_punct(Punct::Star);
        if matches!(self.peek(), Token::Invalid(message) if message.contains("unexpected character '#'"))
        {
            return Err(self.syntax_error("a function cannot have a private name"));
        }
        let name = if let Token::Identifier(name) = self.peek() {
            if is_async && name == "await" {
                let detail = if self.current_identifier_escaped() {
                    "the await keyword cannot contain an escape"
                } else {
                    "await cannot be used as an async function name"
                };
                return Err(self.syntax_error(detail));
            }
            Some(self.expect_identifier_name()?)
        } else {
            None
        };
        if generator && matches!(name.as_deref(), Some("yield")) {
            return Err(self.syntax_error("yield cannot be used as a generator function name"));
        }
        if !self.check_punct(Punct::LParen) {
            return Err(self.syntax_error("a function parameter list must begin with '('"));
        }
        let function = self.parse_method_function(name, generator, is_async)?;
        if function_contains_super_call_outside_class(&function)
            || function_contains_super_property_outside_class(&function)
        {
            return Err(self.syntax_error("a normal function cannot contain super"));
        }
        Ok(function)
    }

    pub(super) fn parse_method_function(
        &mut self,
        name: Option<String>,
        generator: bool,
        is_async: bool,
    ) -> Result<Function, ParseError> {
        let outer_async_depth = std::mem::replace(&mut self.async_depth, u32::from(is_async));
        let outer_module_await = std::mem::replace(&mut self.module_await, false);
        let outer_generator_depth =
            std::mem::replace(&mut self.generator_depth, u32::from(generator));
        // `parse_params` also enters grammar that the subset may not yet
        // implement. Preserve an unclassified parse failure from that grammar;
        // explicit parameter early errors mark themselves as known syntax.
        let params = self.parse_params()?;
        self.function_depth += 1;
        let body = self.parse_block();
        self.function_depth -= 1;
        self.generator_depth = outer_generator_depth;
        self.async_depth = outer_async_depth;
        self.module_await = outer_module_await;
        Ok(Function {
            name,
            params,
            body: body?,
            generator,
            is_async,
        })
    }

    pub(super) fn parse_arrow_body(&mut self, is_async: bool) -> Result<ArrowBody, ParseError> {
        self.async_depth += u32::from(is_async);
        let outer_module_await = std::mem::replace(&mut self.module_await, false);
        self.function_depth += 1;
        let body = if self.check_punct(Punct::LBrace) {
            self.parse_block().map(ArrowBody::Block)
        } else {
            self.parse_assignment()
                .map(|value| ArrowBody::Expr(Box::new(value)))
        };
        self.function_depth -= 1;
        self.async_depth -= u32::from(is_async);
        self.module_await = outer_module_await;
        body
    }

    pub(super) fn parse_class(&mut self) -> Result<Class, ParseError> {
        let name = if matches!(self.peek(), Token::Identifier(name) if name != "extends") {
            Some(self.expect_identifier_name()?)
        } else {
            None
        };
        let extends = if matches!(self.peek(), Token::Identifier(keyword) if keyword == "extends") {
            self.advance();
            if self.class_heritage_is_parenthesized_arrow() {
                return Err(self.syntax_error("a class heritage cannot be an arrow function"));
            }
            Some(Box::new(self.parse_lhs_expression()?))
        } else {
            None
        };
        if self.check_punct(Punct::Arrow) {
            return Err(self.syntax_error("a class heritage cannot be an arrow function"));
        }
        self.expect_punct(Punct::LBrace)?;
        let mut elements = Vec::new();
        let mut has_constructor = false;
        while !self.check_punct(Punct::RBrace) {
            if self.eat_punct(Punct::Semicolon) {
                continue;
            }
            let is_static = matches!(self.peek(), Token::Identifier(static_keyword) if static_keyword == "static")
                && !matches!(self.peek_at(1), Token::Punct(Punct::LParen));
            if is_static {
                self.advance();
            }
            if is_static && self.check_punct(Punct::LBrace) {
                self.static_block_function_depths.push(self.function_depth);
                let body = self.parse_block();
                self.static_block_function_depths.pop();
                let body = body?;
                if statements_contain_super_call_outside_class(&body) {
                    return Err(self.syntax_error("a static block cannot contain super()"));
                }
                elements.push(ClassElement::StaticBlock(body));
                continue;
            }
            let is_async = self.class_async_method_follows();
            if is_async {
                self.advance();
            }
            let accessor = match self.peek() {
                Token::Identifier(keyword)
                    if (keyword == "get" || keyword == "set")
                        && !matches!(self.peek_at(1), Token::Punct(Punct::LParen)) =>
                {
                    let getter = keyword == "get";
                    self.advance();
                    Some(getter)
                }
                _ => None,
            };
            let generator = self.eat_punct(Punct::Star);
            let key = self.parse_class_element_key()?;
            let method_name = class_element_name(&key);
            if !self.check_punct(Punct::LParen) {
                if generator || accessor.is_some() {
                    return Err(self.error("expected class method parameters"));
                }
                let initializer = if self.eat_punct(Punct::Assign) {
                    Some(self.parse_assignment()?)
                } else {
                    None
                };
                if initializer
                    .as_ref()
                    .is_some_and(expr_contains_super_call_outside_class)
                {
                    return Err(
                        self.syntax_error("a class field initializer cannot contain super()")
                    );
                }
                let terminated = self.eat_punct(Punct::Semicolon);
                let ends_with_block = self
                    .tokens
                    .get(self.pos.saturating_sub(1))
                    .is_some_and(|token| matches!(token.token, Token::Punct(Punct::RBrace)));
                if !terminated
                    && !ends_with_block
                    && !self.check_punct(Punct::RBrace)
                    && self
                        .tokens
                        .get(self.pos)
                        .is_some_and(|token| !token.newline_before)
                {
                    return Err(
                        self.syntax_error("class fields on one line require a semicolon separator")
                    );
                }
                elements.push(ClassElement::Field {
                    key,
                    initializer,
                    is_static,
                });
                continue;
            }
            let function = self.parse_method_function(Some(method_name), generator, is_async)?;
            let constructor = accessor.is_none()
                && !is_static
                && !matches!(&key, PropertyKey::Computed(_))
                && class_element_name(&key) == "constructor";
            if function_contains_super_call_outside_class(&function)
                && (!constructor || extends.is_none())
            {
                return Err(self.syntax_error("super() is only valid in a derived constructor"));
            }
            if let Some(getter) = accessor {
                if !is_static
                    && !matches!(&key, PropertyKey::Computed(_))
                    && class_element_name(&key) == "constructor"
                {
                    return Err(self.syntax_error("constructor cannot be an accessor"));
                }
                if is_static
                    && !matches!(&key, PropertyKey::Computed(_))
                    && class_element_name(&key) == "prototype"
                {
                    return Err(self.syntax_error("static accessor cannot be named prototype"));
                }
                if is_async
                    || generator
                    || (getter && !function.params.is_empty())
                    || (!getter && (function.params.len() != 1 || function.params[0].rest))
                {
                    return Err(self.error("invalid class accessor parameter list"));
                }
                elements.push(ClassElement::Accessor {
                    key,
                    function,
                    getter,
                    is_static,
                });
            } else {
                if is_static
                    && !matches!(&key, PropertyKey::Computed(_))
                    && class_element_name(&key) == "prototype"
                {
                    return Err(self.syntax_error("static method cannot be named prototype"));
                }
                if constructor {
                    if is_async || generator || has_constructor {
                        return Err(self.syntax_error("invalid class constructor"));
                    }
                    has_constructor = true;
                }
                elements.push(ClassElement::Method {
                    key,
                    function,
                    is_static,
                });
            }
        }
        self.expect_punct(Punct::RBrace)?;
        Ok(Class {
            name,
            extends,
            elements,
        })
    }

    /// A parenthesized arrow cannot be a ClassHeritage (which starts with a
    /// LeftHandSideExpression), but `parse_lhs_expression` deliberately does
    /// not parse arrow parameters. Recognize it here so the failure is a
    /// specified syntax error rather than an unclassified empty-paren error.
    pub(super) fn class_heritage_is_parenthesized_arrow(&self) -> bool {
        if !self.check_punct(Punct::LParen) {
            return false;
        }
        let mut depth = 0usize;
        for index in self.pos..self.tokens.len() {
            match self.tokens[index].token {
                Token::Punct(Punct::LParen) => depth += 1,
                Token::Punct(Punct::RParen) => {
                    depth -= 1;
                    if depth == 0 {
                        return matches!(
                            self.tokens.get(index + 1).map(|token| &token.token),
                            Some(Token::Punct(Punct::Arrow))
                        );
                    }
                }
                _ => {}
            }
        }
        false
    }

    /// `async` is a contextual class-element modifier only when the next
    /// token starts a method without an intervening line terminator. Keeping
    /// `async = value` and `async() {}` as ordinary field/method names is
    /// essential for the class element grammar.
    pub(super) fn class_async_method_follows(&self) -> bool {
        if !matches!(self.peek(), Token::Identifier(name) if name == "async")
            || self
                .tokens
                .get(self.pos + 1)
                .is_none_or(|token| token.newline_before)
        {
            return false;
        }
        match self.peek_at(1) {
            Token::Punct(Punct::Star | Punct::LBracket) => true,
            Token::Identifier(_)
            | Token::PrivateIdentifier(_)
            | Token::Keyword(_)
            | Token::String(_)
            | Token::Number(_) => {
                matches!(self.peek_at(2), Token::Punct(Punct::LParen))
            }
            _ => false,
        }
    }

    pub(super) fn async_function_follows(&self) -> bool {
        matches!(self.peek(), Token::Identifier(name) if name == "async")
            && self.tokens.get(self.pos + 1).is_some_and(|token| {
                !token.newline_before && matches!(token.token, Token::Keyword(Keyword::Function))
            })
    }

    /// Object literals use the same contextual `async` modifier as class
    /// methods, but an unmodified `async()` remains an ordinary method name
    /// and `async: value` remains a data property.
    pub(super) fn object_async_method_follows(&self) -> bool {
        self.class_async_method_follows()
    }

    pub(super) fn async_arrow_follows(&self) -> bool {
        if !matches!(self.peek(), Token::Identifier(name) if name == "async")
            || self
                .tokens
                .get(self.pos + 1)
                .is_none_or(|token| token.newline_before)
        {
            return false;
        }
        match self.peek_at(1) {
            Token::Identifier(_) => matches!(self.peek_at(2), Token::Punct(Punct::Arrow)),
            Token::Punct(Punct::LParen) => match self.matching_close_paren(self.pos + 1) {
                Some(close) => matches!(self.tokens[close + 1].token, Token::Punct(Punct::Arrow)),
                None => false,
            },
            _ => false,
        }
    }

    /// Finds the index of the `)` matching the `(` at `open_idx`, or
    /// `None` if the input runs out first (malformed input; the
    /// eventual real parse of that `(...)` will surface its own,
    /// clearer error).
    pub(super) fn matching_close_paren(&self, open_idx: usize) -> Option<usize> {
        let mut depth = 0i32;
        let mut i = open_idx;
        loop {
            match self.tokens.get(i)?.token {
                Token::Punct(Punct::LParen) => depth += 1,
                Token::Punct(Punct::RParen) => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                Token::Eof => return None,
                _ => {}
            }
            i += 1;
        }
    }

    /// Arrow functions (`x => ...` / `(a, b) => ...`) share
    /// `AssignmentExpression`'s grammar slot in real ECMAScript, so
    /// this is checked at the top of [`Parser::parse_assignment`]
    /// before falling through to the ordinary conditional-expression
    /// chain. Detecting the parenthesized-parameter-list form requires
    /// one token of lookahead past the matching `)` (to see whether
    /// `=>` follows) -- exactly what [`Parser::matching_close_paren`]
    /// exists for, since the token stream is fully materialized up
    /// front rather than a lazy/streaming lexer.
    pub(super) fn try_parse_arrow_function(&mut self) -> Result<Option<Expr>, ParseError> {
        if self.async_arrow_follows() {
            self.require_unescaped_async()?;
            self.advance();
            return self.try_parse_arrow_function_with_async(true);
        }
        self.try_parse_arrow_function_with_async(false)
    }

    pub(super) fn try_parse_arrow_function_with_async(
        &mut self,
        is_async: bool,
    ) -> Result<Option<Expr>, ParseError> {
        if let Token::Identifier(name) = self.peek().clone() {
            if matches!(self.peek_at(1), Token::Punct(Punct::Arrow)) {
                if is_async && name == "await" {
                    let detail = if self.current_identifier_escaped() {
                        "the await keyword cannot contain an escape"
                    } else {
                        "await cannot be used as a binding identifier in an async function"
                    };
                    return Err(self.syntax_error(detail));
                }
                self.advance();
                self.advance();
                let params = vec![Param {
                    pattern: Pattern::Identifier(name),
                    default: None,
                    rest: false,
                }];
                let body = self.parse_arrow_body(is_async)?;
                return Ok(Some(Expr::Arrow {
                    params,
                    body,
                    is_async,
                }));
            }
        }
        if self.check_punct(Punct::LParen) {
            if let Some(close_idx) = self.matching_close_paren(self.pos) {
                if matches!(
                    self.tokens.get(close_idx + 1).map(|t| &t.token),
                    Some(Token::Punct(Punct::Arrow))
                ) {
                    // Await is a grammar parameter of AsyncArrowBindingIdentifier
                    // and its parameter initializers. Keep that context active
                    // while parsing the list, including nested arrow defaults;
                    // `parse_arrow_body` establishes it separately for the body.
                    let outer_async_depth = self.async_depth;
                    if is_async {
                        self.async_depth += 1;
                    }
                    let params = self.parse_params()?;
                    self.async_depth = outer_async_depth;
                    self.expect_punct(Punct::Arrow)?;
                    let body = self.parse_arrow_body(is_async)?;
                    return Ok(Some(Expr::Arrow {
                        params,
                        body,
                        is_async,
                    }));
                }
            }
        }
        Ok(None)
    }

    // ---- Expressions ----
}
