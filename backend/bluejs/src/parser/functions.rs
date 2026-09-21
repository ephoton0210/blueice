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
        self.parse_function_named(is_async, false)
    }

    /// A function *declaration*'s BindingIdentifier is parsed with the
    /// enclosing context's `[Await]` parameter, unlike a function
    /// expression's own name (which is `[~Yield, ~Await]` for a plain
    /// function and `[+Await]` for an async one). So `async function
    /// await() {}` is valid at script top level but not inside an async
    /// function, a module or a class static block, and `function await() {}`
    /// is invalid in those same enclosing contexts.
    pub(super) fn parse_function_declaration(
        &mut self,
        is_async: bool,
    ) -> Result<Function, ParseError> {
        self.parse_function_named(is_async, true)
    }

    fn parse_function_named(
        &mut self,
        is_async: bool,
        is_declaration: bool,
    ) -> Result<Function, ParseError> {
        let generator = self.eat_punct(Punct::Star);
        if matches!(self.peek(), Token::Invalid(message) if message.contains("unexpected character '#'"))
        {
            return Err(self.syntax_error("a function cannot have a private name"));
        }
        let name = if let Token::Identifier(name) = self.peek() {
            if name == "await" {
                let context_reserves_await = self.async_depth != 0
                    || self.module_await
                    || self.static_block_function_depths.last() == Some(&self.function_depth);
                // Module code reserves `await` even as a function expression's
                // own name, which the [~Await] parameter alone would allow.
                if self.module
                    || (is_async && !is_declaration)
                    || (is_declaration && context_reserves_await)
                {
                    let detail = if self.current_identifier_escaped() {
                        "the await keyword cannot contain an escape"
                    } else {
                        "await cannot be used as a function name here"
                    };
                    return Err(self.syntax_error(detail));
                }
            }
            Some(self.expect_identifier_name()?)
        } else if !self.strict && self.check_keyword(Keyword::Let) {
            // `let` is an ordinary identifier in sloppy code.
            self.advance();
            Some("let".to_string())
        } else {
            None
        };
        if let Some(name) = &name {
            self.validate_function_name(name, is_declaration)?;
        }
        // A generator *declaration* names its binding in the enclosing
        // context, so `yield` is fine there in sloppy non-generator code; a
        // generator expression's name is parsed with [+Yield].
        if generator
            && matches!(name.as_deref(), Some("yield"))
            && (!is_declaration || self.generator_depth != 0 || self.strict)
        {
            return Err(self.syntax_error("yield cannot be used as a generator function name"));
        }
        if !self.check_punct(Punct::LParen) {
            return Err(self.syntax_error("a function parameter list must begin with '('"));
        }
        let function = self.parse_method_function(name, generator, is_async)?;
        // The BindingIdentifier belongs to the function code, so a Use Strict
        // Directive in the body makes the name strict retroactively.
        if !self.strict
            && function
                .name
                .as_deref()
                .is_some_and(is_strict_reserved_word)
            && function_body_has_use_strict(&function.body)
        {
            return Err(self.syntax_error("a strict function cannot be named with a reserved word"));
        }
        if function_contains_super_call_outside_class(&function)
            || function_contains_super_property_outside_class(&function)
        {
            return Err(self.syntax_error("a normal function cannot contain super"));
        }
        Ok(function)
    }

    /// A FieldDefinition's Initializer is parsed with `[~Yield, ~Await]`
    /// whatever surrounds the class: inside an async function or generator
    /// `await` and `yield` do not become operators there, and in a script
    /// `await` is an ordinary IdentifierReference.
    fn parse_field_initializer(&mut self) -> Result<Expr, ParseError> {
        let outer_async_depth = std::mem::replace(&mut self.async_depth, 0);
        let outer_module_await = std::mem::replace(&mut self.module_await, false);
        let outer_generator_depth = std::mem::replace(&mut self.generator_depth, 0);
        let initializer = self.parse_assignment();
        self.generator_depth = outer_generator_depth;
        self.module_await = outer_module_await;
        self.async_depth = outer_async_depth;
        initializer
    }

    /// The early errors of a function's BindingIdentifier that do not depend
    /// on its own body: ReservedWords, the strict-mode reserved words in
    /// strict code, and `yield` in a generator body for a declaration (whose
    /// name is a binding of the enclosing context; `await` is handled by the
    /// caller).
    fn validate_function_name(&self, name: &str, is_declaration: bool) -> Result<(), ParseError> {
        if matches!(
            name,
            "class" | "debugger" | "enum" | "export" | "extends" | "import" | "super" | "with"
        ) || Keyword::from_str(name).is_some_and(|keyword| keyword != Keyword::Let)
        {
            return Err(self.syntax_error("a reserved word cannot be a function name"));
        }
        if self.strict && is_strict_reserved_word(name) {
            return Err(self.syntax_error("a strict mode reserved word cannot be a function name"));
        }
        if is_declaration && name == "yield" && self.generator_depth != 0 {
            return Err(self.syntax_error("yield cannot be used as a function name here"));
        }
        Ok(())
    }

    /// A MethodDefinition's function (object literal or class, including
    /// generator, async and accessor forms). Its parameters are
    /// UniqueFormalParameters: no name repeats, even in sloppy code.
    pub(super) fn parse_method_definition(
        &mut self,
        name: Option<String>,
        generator: bool,
        is_async: bool,
    ) -> Result<Function, ParseError> {
        let function = self.parse_method_function(name, generator, is_async)?;
        let mut names = std::collections::HashSet::new();
        for param in &function.params {
            for name in super::module::pattern_bound_names(&param.pattern) {
                if !names.insert(name) {
                    return Err(self.syntax_error("duplicate parameter name in a method"));
                }
            }
        }
        Ok(function)
    }

    pub(super) fn parse_method_function(
        &mut self,
        name: Option<String>,
        generator: bool,
        is_async: bool,
    ) -> Result<Function, ParseError> {
        self.with_in_allowed(|parser| parser.parse_method_function_in(name, generator, is_async))
    }

    fn parse_method_function_in(
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
        //
        // The parameters already belong to the new function for static-block
        // purposes: a nested function's own `await` parameter is not a
        // binding "directly within" the enclosing class static block.
        self.function_depth += 1;
        let params = self
            .parse_params()
            .inspect_err(|_| self.function_depth -= 1)?;
        let body = self.parse_function_body();
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
            self.parse_function_body().map(ArrowBody::Block)
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
        self.with_in_allowed(Self::parse_class_strict)
    }

    fn parse_class_strict(&mut self) -> Result<Class, ParseError> {
        // Every part of a ClassDefinition, including the heritage expression,
        // is parsed in strict mode. Preserve the caller's grammar context so
        // a nested class does not leak strictness into its surrounding script.
        let outer_strict = std::mem::replace(&mut self.strict, true);
        let class = self.parse_class_definition();
        self.strict = outer_strict;
        class
    }

    fn parse_class_definition(&mut self) -> Result<Class, ParseError> {
        let name = match self.peek() {
            Token::Identifier(name) if name != "extends" => {
                let name = self.expect_identifier_name()?;
                if matches!(
                    name.as_str(),
                    "implements"
                        | "interface"
                        | "let"
                        | "package"
                        | "private"
                        | "protected"
                        | "public"
                        | "static"
                        | "yield"
                ) || (self.module && name == "await")
                {
                    return Err(self.syntax_error("invalid class binding identifier"));
                }
                Some(name)
            }
            Token::Keyword(_) => {
                return Err(self.syntax_error("class declarations require a binding identifier"));
            }
            _ => None,
        };
        if name.as_deref() == Some("await")
            && self.static_block_function_depths.last() == Some(&self.function_depth)
        {
            return Err(self.syntax_error(
                "await cannot be bound by a class declaration in a class static block",
            ));
        }
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
            let decorators = self.parse_decorators()?;
            let is_static = matches!(self.peek(), Token::Identifier(static_keyword) if static_keyword == "static")
                && !self.current_identifier_escaped()
                && !matches!(
                    self.peek_at(1),
                    Token::Punct(Punct::LParen | Punct::Semicolon | Punct::Assign | Punct::RBrace)
                );
            if is_static {
                self.advance();
            }
            if is_static && self.check_punct(Punct::LBrace) {
                if !decorators.is_empty() {
                    return Err(self.syntax_error("a class static block cannot be decorated"));
                }
                self.static_block_function_depths.push(self.function_depth);
                let body = self.parse_block();
                self.static_block_function_depths.pop();
                let body = body?;
                if statements_contain_arguments(&body) {
                    return Err(self.syntax_error(
                        "a class static block cannot contain a lexical arguments reference",
                    ));
                }
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
            // `get`/`set` only introduce an accessor when a property name
            // follows; otherwise (`get() {}`, `get = 1`, or a line break before
            // a `*` generator method) they are the name of a method or field.
            let accessor = match self.peek() {
                Token::Identifier(keyword)
                    if (keyword == "get" || keyword == "set")
                        && !self.current_identifier_escaped()
                        && matches!(
                            self.peek_at(1),
                            Token::Identifier(_)
                                | Token::PrivateIdentifier(_)
                                | Token::Keyword(_)
                                | Token::String(_)
                                | Token::Number(_)
                                | Token::BigInt(_)
                                | Token::Punct(Punct::LBracket)
                        ) =>
                {
                    let getter = keyword == "get";
                    self.advance();
                    Some(getter)
                }
                _ => None,
            };
            // `accessor` starts an auto-accessor field only when a class element
            // name follows on the same line: `accessor` alone, before `=`,
            // `;`, `(` or a line break, is an ordinary field or method name.
            let auto_accessor = !is_async
                && accessor.is_none()
                && matches!(self.peek(), Token::Identifier(name) if name == "accessor")
                && !self.current_identifier_escaped()
                && self
                    .tokens
                    .get(self.pos + 1)
                    .is_some_and(|token| !token.newline_before)
                && matches!(
                    self.peek_at(1),
                    Token::Identifier(_)
                        | Token::PrivateIdentifier(_)
                        | Token::Keyword(_)
                        | Token::String(_)
                        | Token::Number(_)
                        | Token::BigInt(_)
                        | Token::Punct(Punct::LBracket)
                );
            if auto_accessor {
                self.advance();
            }
            let generator = self.eat_punct(Punct::Star);
            let key = self.parse_class_element_key()?;
            let method_name = class_element_name(&key);
            if auto_accessor && (generator || self.check_punct(Punct::LParen)) {
                return Err(self.syntax_error("an auto-accessor cannot be a method"));
            }
            if !self.check_punct(Punct::LParen) {
                if generator || accessor.is_some() {
                    return Err(self.error("expected class method parameters"));
                }
                if !matches!(&key, PropertyKey::Computed(_))
                    && ((!is_static && method_name == "constructor")
                        || (is_static
                            && matches!(method_name.as_str(), "constructor" | "prototype")))
                {
                    return Err(self.syntax_error("invalid public class field name"));
                }
                let initializer = if self.eat_punct(Punct::Assign) {
                    Some(self.parse_field_initializer()?)
                } else {
                    None
                };
                if initializer.as_ref().is_some_and(expr_contains_arguments) {
                    return Err(self.syntax_error(
                        "a class field initializer cannot contain a lexical arguments reference",
                    ));
                }
                if initializer
                    .as_ref()
                    .is_some_and(expr_contains_super_call_outside_class)
                {
                    return Err(
                        self.syntax_error("a class field initializer cannot contain super()")
                    );
                }
                let terminated = self.eat_punct(Punct::Semicolon);
                if !terminated
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
                    accessor: auto_accessor,
                    decorators,
                });
                continue;
            }
            let function = self.parse_method_definition(Some(method_name), generator, is_async)?;
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
                    decorators,
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
                    if !decorators.is_empty() {
                        return Err(self.syntax_error("a class constructor cannot be decorated"));
                    }
                    has_constructor = true;
                }
                elements.push(ClassElement::Method {
                    key,
                    function,
                    is_static,
                    decorators,
                });
            }
        }
        self.expect_punct(Punct::RBrace)?;
        Ok(Class {
            name,
            extends,
            elements,
            decorators: Vec::new(),
        })
    }

    /// `DecoratorList`: every `@` decorator at the current position, in source
    /// order (empty when none starts here).
    pub(super) fn parse_decorators(&mut self) -> Result<Vec<Expr>, ParseError> {
        let mut decorators = Vec::new();
        while self.eat_punct(Punct::At) {
            decorators.push(self.parse_decorator()?);
        }
        Ok(decorators)
    }

    /// One decorator after its `@`. Only three shapes exist, so that a
    /// decorator never needs arbitrary-expression parsing to find its own end:
    /// `DecoratorMemberExpression` (`a.b.#c`), `DecoratorCallExpression` (the
    /// same followed by one argument list) and `DecoratorParenthesizedExpression`
    /// (`(expression)`, the escape hatch for anything else). The result is the
    /// expression whose value is the decorator function.
    fn parse_decorator(&mut self) -> Result<Expr, ParseError> {
        if self.eat_punct(Punct::LParen) {
            let expression = self.with_in_allowed(Self::parse_expression)?;
            self.expect_punct(Punct::RParen).map_err(|mut error| {
                error.known_syntax = true;
                error
            })?;
            return Ok(Expr::Parenthesized(Box::new(expression)));
        }
        let name = match self.peek().clone() {
            Token::Identifier(name) if self.identifier_reference_name_is_valid(&name) => name,
            // `let` is an IdentifierReference in sloppy code only.
            Token::Keyword(Keyword::Let) if !self.strict => "let".to_string(),
            _ => return Err(self.syntax_error("expected a decorator expression")),
        };
        self.advance();
        let mut expression = Expr::Identifier(name);
        while self.eat_punct(Punct::Dot) {
            let property = self.expect_member_name()?;
            expression = Expr::Member {
                object: Box::new(expression),
                property: Box::new(Expr::Identifier(property)),
                computed: false,
            };
        }
        if self.check_punct(Punct::LParen) {
            expression = Expr::Call {
                callee: Box::new(expression),
                args: self.parse_arguments()?,
            };
        }
        Ok(expression)
    }

    /// A class that follows already-parsed decorators: `class` itself, then
    /// the rest of the definition. The decorators are stored on the class.
    pub(super) fn parse_decorated_class(
        &mut self,
        decorators: Vec<Expr>,
    ) -> Result<Class, ParseError> {
        if !matches!(self.peek(), Token::Identifier(name) if name == "class")
            || self.current_identifier_escaped()
        {
            return Err(
                self.syntax_error("a decorator must be followed by a class or class element")
            );
        }
        self.advance();
        let mut class = self.parse_class()?;
        class.decorators = decorators;
        Ok(class)
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
            || self.current_identifier_escaped()
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

    /// `using` is a contextual keyword: `using [no LineTerminator here]
    /// BindingIdentifier` starts a using declaration only in statement
    /// position (not yet supported: a `ForBinding`'s own `using` form).
    /// Anything else (`using;`, `using.foo()`, `using = 1`, `using\nx = 1`)
    /// leaves `using` as an ordinary identifier reference.
    pub(super) fn using_declaration_follows(&self) -> bool {
        self.check_identifier("using")
            && !self.current_identifier_escaped()
            && self.tokens.get(self.pos + 1).is_some_and(|token| {
                !token.newline_before && matches!(token.token, Token::Identifier(_))
            })
    }

    /// Within a for-statement head specifically, `using` immediately
    /// followed by the literal identifier `of` is ambiguous: `for (using of
    /// expr)` is the existing variable `using` iterated via a plain for-of
    /// over `expr` (confirmed directly against `using-for-using-of-of.js`),
    /// but `for (using of = expr;;)` is a using declaration whose bound
    /// identifier's *name* happens to be `of` (confirmed against
    /// `using-for-statement.js`, "'for (using of =' are interpreted as for
    /// loop"). The token after that second identifier disambiguates: `=`
    /// means it is a BindingIdentifier continuing the declaration, anything
    /// else means it was the for-of separator and `using` is a bare
    /// identifier. This ambiguity is specific to the for-of separator, so
    /// `using_declaration_follows` itself doesn't need it.
    pub(super) fn using_declaration_follows_in_for_head(&self) -> bool {
        // Equivalent to `... && !(next-is-"of" && token-after-that-isn't-"=")`,
        // written in the de Morgan form clippy prefers.
        let next_is_of = matches!(self.peek_at(1), Token::Identifier(name) if name == "of");
        let followed_by_assign = matches!(self.peek_at(2), Token::Punct(Punct::Assign));
        self.using_declaration_follows() && (!next_is_of || followed_by_assign)
    }

    /// `await using` has no analogous exclusion: unlike plain `using`,
    /// `await using` can never validly stand alone as a for-of loop
    /// variable (`await using` alone would have to parse as an
    /// AwaitExpression wrapping the identifier `using`, which is not a
    /// valid for-of assignment target), so `for (await using of of expr)`
    /// is unambiguously the declaration form (`of` as its bound
    /// identifier's name), confirmed directly against
    /// `await-using-valid-for-await-using-of-of.js`.
    pub(super) fn await_using_declaration_follows_in_for_head(&self) -> bool {
        self.await_using_declaration_follows()
    }

    /// `await using` requires an async context (async function/method/
    /// arrow/generator body, or a module with top-level await), exactly
    /// like an ordinary `await` expression -- checked the same way
    /// (`async_depth`/`module_await`). Both `await` and `using` are
    /// contextual identifiers, so this needs the same
    /// no-LineTerminator/next-token lookahead as `using_declaration_follows`
    /// applied twice in a row.
    pub(super) fn await_using_declaration_follows(&self) -> bool {
        (self.async_depth != 0 || self.module_await)
            && self.check_identifier("await")
            && !self.current_identifier_escaped()
            && self.tokens.get(self.pos + 1).is_some_and(|token| {
                !token.newline_before
                    && matches!(&token.token, Token::Identifier(name) if name == "using")
            })
            && self.tokens.get(self.pos + 2).is_some_and(|token| {
                !token.newline_before && matches!(token.token, Token::Identifier(_))
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
                if self.tokens[self.pos + 1].newline_before {
                    return Err(self.syntax_error("no line terminator is allowed before =>"));
                }
                if is_async && name == "await" {
                    let detail = if self.current_identifier_escaped() {
                        "the await keyword cannot contain an escape"
                    } else {
                        "await cannot be used as a binding identifier in an async function"
                    };
                    return Err(self.syntax_error(detail));
                }
                self.validate_binding_identifier(&name, self.current_identifier_escaped())?;
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
                    if self.tokens[self.pos].newline_before {
                        return Err(self.syntax_error("no line terminator is allowed before =>"));
                    }
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
