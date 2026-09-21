// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Parser {
    pub(super) fn validate_binding_identifier(
        &self,
        name: &str,
        escaped: bool,
    ) -> Result<(), ParseError> {
        // These are ReservedWords which the tokenizer preserves as
        // IdentifierName tokens because they remain valid property names,
        // together with every keyword spelled with a Unicode escape (which
        // the tokenizer also returns as an IdentifierName). A
        // BindingIdentifier may not use them, escaped or otherwise.
        if matches!(
            name,
            "class" | "debugger" | "enum" | "export" | "extends" | "import" | "super" | "with"
        ) || Keyword::from_str(name).is_some_and(|keyword| keyword != Keyword::Let)
        {
            return Err(self.syntax_error("a reserved word cannot be used as a binding identifier"));
        }
        if name == "await" && (self.async_depth != 0 || self.module_await) {
            let detail = if escaped {
                "the await keyword cannot contain an escape"
            } else {
                "await cannot be used as a binding identifier in an async function or module"
            };
            return Err(self.syntax_error(detail));
        }
        // "It is a Syntax Error if the code matched by this production is
        // nested, directly or indirectly (but not crossing function or static
        // initialization block boundaries), within a ClassStaticBlock and the
        // StringValue of Identifier is "await"." Function parameters are
        // parsed with `function_depth` already advanced, so only bindings that
        // really sit directly in the block reach this.
        if name == "await" && self.static_block_function_depths.last() == Some(&self.function_depth)
        {
            return Err(self.syntax_error("await cannot be bound directly in a class static block"));
        }
        if name == "yield" && self.generator_depth != 0 {
            let detail = if escaped {
                "the yield keyword cannot contain an escape"
            } else {
                "yield cannot be used as a binding identifier in a generator function"
            };
            return Err(self.syntax_error(detail));
        }
        Ok(())
    }

    /// In sloppy code `let` is an ordinary identifier unless the token after
    /// it can begin a lexical binding (a BindingIdentifier, `[` or `{`), in
    /// which case it starts a `let` declaration. Strict code (and modules)
    /// reserve `let`, so it always introduces a declaration there.
    pub(super) fn let_starts_declaration(&self) -> bool {
        self.strict
            || matches!(
                self.peek_at(1),
                Token::Identifier(_)
                    | Token::Keyword(Keyword::Let)
                    | Token::Punct(Punct::LBracket | Punct::LBrace)
            )
    }

    /// "It is a Syntax Error if the BoundNames of BindingList contains "let""
    /// for `let`, `const`, `using` and `await using` declarations. (`var`
    /// may bind `let` in sloppy code, which `parse_binding_pattern` allows.)
    pub(super) fn check_lexical_binding_names(
        &self,
        kind: DeclKind,
        pattern: &Pattern,
    ) -> Result<(), ParseError> {
        if kind != DeclKind::Var
            && super::module::pattern_bound_names(pattern)
                .iter()
                .any(|name| name == "let")
        {
            return Err(self.syntax_error("a lexical declaration cannot bind the name 'let'"));
        }
        Ok(())
    }

    pub(super) fn parse_binding_pattern(&mut self) -> Result<Pattern, ParseError> {
        match self.peek().clone() {
            Token::Identifier(name) => {
                self.validate_binding_identifier(&name, self.current_identifier_escaped())?;
                self.advance();
                Ok(Pattern::Identifier(name))
            }
            Token::Keyword(Keyword::Let) if !self.strict => {
                self.advance();
                Ok(Pattern::Identifier("let".to_string()))
            }
            Token::Punct(Punct::LBracket) => self.parse_array_pattern(),
            Token::Punct(Punct::LBrace) => self.parse_object_pattern(),
            _ => Err(self.syntax_error("expected a binding target (identifier, '[', or '{')")),
        }
    }

    pub(super) fn parse_array_pattern(&mut self) -> Result<Pattern, ParseError> {
        self.expect_punct(Punct::LBracket)?;
        let mut elements = Vec::new();
        while !self.check_punct(Punct::RBracket) {
            if self.check_punct(Punct::Comma) {
                self.advance();
                elements.push(None);
                continue;
            }
            if self.eat_punct(Punct::Ellipsis) {
                let pattern = self.parse_binding_pattern()?;
                elements.push(Some(ArrayPatternElement {
                    pattern,
                    default: None,
                    rest: true,
                }));
                if !self.check_punct(Punct::RBracket) {
                    return Err(self.syntax_error("a binding rest element must be final"));
                }
                break;
            } else {
                let pattern = self.parse_binding_pattern()?;
                let default = if self.eat_punct(Punct::Assign) {
                    Some(self.parse_assignment()?)
                } else {
                    None
                };
                elements.push(Some(ArrayPatternElement {
                    pattern,
                    default,
                    rest: false,
                }));
            }
            if !self.check_punct(Punct::RBracket) {
                self.expect_punct(Punct::Comma)?;
            }
        }
        self.expect_punct(Punct::RBracket)?;
        Ok(Pattern::Array(elements))
    }

    pub(super) fn parse_object_pattern(&mut self) -> Result<Pattern, ParseError> {
        self.expect_punct(Punct::LBrace)?;
        let mut props = Vec::new();
        while !self.check_punct(Punct::RBrace) {
            if self.eat_punct(Punct::Ellipsis) {
                props.push(ObjectPatternProp::Rest(self.parse_binding_pattern()?));
                if !self.check_punct(Punct::RBrace) {
                    return Err(self.syntax_error("a binding rest property must be final"));
                }
                break;
            } else {
                // `parse_property_key` accepts every IdentifierName, including
                // keywords. A shorthand property is also a binding target, so
                // retain the lexical-token distinction and apply the same
                // early errors as `parse_binding_pattern`.
                let binding_identifier = match self.peek().clone() {
                    Token::Identifier(name) => Some((name, self.current_identifier_escaped())),
                    _ => None,
                };
                let key = self.parse_property_key()?;
                if self.eat_punct(Punct::Colon) {
                    let value = self.parse_binding_pattern()?;
                    let default = if self.eat_punct(Punct::Assign) {
                        Some(self.parse_assignment()?)
                    } else {
                        None
                    };
                    props.push(ObjectPatternProp::KeyValue {
                        key,
                        value,
                        default,
                    });
                } else {
                    let Some((_, escaped)) = binding_identifier else {
                        return Err(self.syntax_error(
                            "expected a binding identifier in destructuring pattern",
                        ));
                    };
                    let name = match &key {
                        PropertyKey::Identifier(n) => n.clone(),
                        _ => return Err(self.error("expected ':' in destructuring pattern")),
                    };
                    self.validate_binding_identifier(&name, escaped)?;
                    let default = if self.eat_punct(Punct::Assign) {
                        Some(self.parse_assignment()?)
                    } else {
                        None
                    };
                    props.push(ObjectPatternProp::KeyValue {
                        key: PropertyKey::Identifier(name.clone()),
                        value: Pattern::Identifier(name),
                        default,
                    });
                }
            }
            if !self.check_punct(Punct::RBrace) {
                self.expect_punct(Punct::Comma)?;
            }
        }
        self.expect_punct(Punct::RBrace)?;
        Ok(Pattern::Object(props))
    }

    pub(super) fn parse_property_key(&mut self) -> Result<PropertyKey, ParseError> {
        match self.peek().clone() {
            Token::Identifier(name) => {
                self.advance();
                Ok(PropertyKey::Identifier(name))
            }
            Token::Keyword(k) => {
                self.advance();
                Ok(PropertyKey::Identifier(keyword_as_str(k).to_string()))
            }
            Token::String(s) => {
                self.reject_legacy_octal_escape()?;
                self.advance();
                Ok(PropertyKey::String(s))
            }
            Token::Number(n) => {
                self.advance();
                Ok(PropertyKey::Number(n))
            }
            Token::BigInt(n) => {
                // LiteralPropertyName: NumericLiteral -- "Let nbr be the
                // NumericValue of NumericLiteral. Return ! ToString(nbr)."
                // BigInt's ToString is exactly its decimal `Display`, so
                // this needs no further numeric formatting pass.
                self.advance();
                Ok(PropertyKey::String(n.to_string().into()))
            }
            Token::Punct(Punct::LBracket) => {
                self.advance();
                let expr = self.parse_assignment()?;
                self.expect_punct(Punct::RBracket)?;
                Ok(PropertyKey::Computed(Box::new(expr)))
            }
            Token::PrivateIdentifier(_) => {
                Err(self.syntax_error("a private name is not a property key here"))
            }
            _ => Err(self.error("expected a property key")),
        }
    }

    pub(super) fn parse_class_element_key(&mut self) -> Result<PropertyKey, ParseError> {
        if let Token::PrivateIdentifier(name) = self.peek().clone() {
            self.advance();
            if name == "constructor" {
                return Err(self.syntax_error("a private name cannot be constructor"));
            }
            return Ok(PropertyKey::Identifier(format!("#{name}")));
        }
        self.parse_property_key()
    }
}
