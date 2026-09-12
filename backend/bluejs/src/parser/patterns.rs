// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Parser {
    pub(super) fn parse_binding_pattern(&mut self) -> Result<Pattern, ParseError> {
        match self.peek().clone() {
            Token::Identifier(name) => {
                if name == "await" && (self.async_depth != 0 || self.module_await) {
                    let detail = if self.current_identifier_escaped() {
                        "the await keyword cannot contain an escape"
                    } else {
                        "await cannot be used as a binding identifier in an async function or module"
                    };
                    return Err(self.syntax_error(detail));
                }
                if name == "yield" && self.generator_depth != 0 {
                    let detail = if self.current_identifier_escaped() {
                        "the yield keyword cannot contain an escape"
                    } else {
                        "yield cannot be used as a binding identifier in a generator function"
                    };
                    return Err(self.syntax_error(detail));
                }
                self.advance();
                Ok(Pattern::Identifier(name))
            }
            Token::Punct(Punct::LBracket) => self.parse_array_pattern(),
            Token::Punct(Punct::LBrace) => self.parse_object_pattern(),
            _ => Err(self.error("expected a binding target (identifier, '[', or '{')")),
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
                    let name = match &key {
                        PropertyKey::Identifier(n) => n.clone(),
                        _ => return Err(self.error("expected ':' in destructuring pattern")),
                    };
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
                self.advance();
                Ok(PropertyKey::String(s))
            }
            Token::Number(n) => {
                self.advance();
                Ok(PropertyKey::Number(n))
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
