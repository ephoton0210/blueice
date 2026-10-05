// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Calls, construction and member suffixes.

use super::*;

impl<'a> ExpressionLowerer<'a> {
    pub(crate) fn parse_new_expression(
        &mut self,
        keyword: &Token,
    ) -> Result<bluejs::Expr, BridgeError> {
        let Some(callee) = self.tokens.get(self.index) else {
            return Err(unsupported(
                self.token_span(keyword),
                "expected a constructor after `new`",
            ));
        };
        if callee.kind != TokenKind::Identifier {
            return Err(unsupported(
                self.token_span(callee),
                "only identifier constructors are in the v1 direct bridge subset",
            ));
        }
        let mut callee = bluejs::Expr::Identifier(callee.text.clone());
        self.index += 1;
        // A namespace member, `new N.C()`.
        while self
            .tokens
            .get(self.index)
            .is_some_and(|dot| dot.text == ".")
            && self
                .tokens
                .get(self.index + 1)
                .is_some_and(|name| name.kind == TokenKind::Identifier)
        {
            let name = self.tokens[self.index + 1].text.clone();
            callee = bluejs::Expr::Member {
                object: Box::new(callee),
                property: Box::new(bluejs::Expr::Identifier(name)),
                computed: false,
            };
            self.index += 2;
        }
        let Some(opening) = self.tokens.get(self.index) else {
            return Err(unsupported(
                self.token_span(keyword),
                "expected `(` after a constructor",
            ));
        };
        if opening.text != "(" {
            return Err(unsupported(
                self.token_span(opening),
                "only constructor calls with parentheses are in the v1 direct bridge subset",
            ));
        }
        self.index += 1;
        let mut args = Vec::new();
        if self
            .tokens
            .get(self.index)
            .is_some_and(|token| token.text == ")")
        {
            self.index += 1;
        } else {
            loop {
                args.push(self.parse_argument()?);
                let Some(separator) = self.tokens.get(self.index) else {
                    return Err(unsupported(
                        self.token_span(keyword),
                        "unterminated constructor call",
                    ));
                };
                match separator.text.as_str() {
                    "," => self.index += 1,
                    ")" => {
                        self.index += 1;
                        break;
                    }
                    _ => {
                        return Err(unsupported(
                            self.token_span(separator),
                            "expected `,` or `)` in constructor call",
                        ))
                    }
                }
            }
        }
        Ok(bluejs::Expr::New {
            callee: Box::new(callee),
            args,
        })
    }

    pub(crate) fn parse_argument(&mut self) -> Result<bluejs::Argument, BridgeError> {
        if self
            .tokens
            .get(self.index)
            .is_some_and(|token| token.text == "...")
        {
            self.index += 1;
            Ok(bluejs::Argument::Spread(self.parse_assignment()?))
        } else {
            Ok(bluejs::Argument::Normal(self.parse_assignment()?))
        }
    }

    pub(crate) fn parse_suffixes(
        &mut self,
        mut expression: bluejs::Expr,
    ) -> Result<bluejs::Expr, BridgeError> {
        loop {
            let Some(token) = self.tokens.get(self.index) else {
                return Ok(expression);
            };
            if token.text == "!" {
                // TypeScript's postfix non-null assertion is erased. The
                // checker narrows the static result; runtime null access
                // still fails normally in BlueJS.
                self.index += 1;
                continue;
            }
            if token.text == "?." {
                let optional_span = self.token_span(token);
                if !matches!(expression, bluejs::Expr::Identifier(_)) {
                    return Err(unsupported(
                        optional_span,
                        "only a direct identifier receiver may use optional dot access",
                    ));
                }
                self.index += 1;
                let Some(property) = self.tokens.get(self.index) else {
                    return Err(unsupported(
                        optional_span,
                        "expected an identifier after optional dot access",
                    ));
                };
                if property.kind != TokenKind::Identifier {
                    return Err(unsupported(
                        self.token_span(property),
                        "only an identifier property is in the first optional dot subset",
                    ));
                }
                let name = property.text.clone();
                self.index += 1;
                expression = bluejs::Expr::OptionalMember {
                    object: Box::new(expression),
                    property: Box::new(bluejs::Expr::Identifier(name)),
                    computed: false,
                };
                continue;
            }
            if token.text == "." {
                let dot_span = self.token_span(token);
                self.index += 1;
                let Some(property) = self.tokens.get(self.index) else {
                    return Err(unsupported(dot_span, "expected a property name after `.`"));
                };
                if property.kind != TokenKind::Identifier {
                    return Err(unsupported(
                        self.token_span(property),
                        "only identifier dot property names are in the v1 direct bridge subset",
                    ));
                }
                let name = property.text.clone();
                self.index += 1;
                expression = bluejs::Expr::Member {
                    object: Box::new(expression),
                    property: Box::new(bluejs::Expr::Identifier(name)),
                    computed: false,
                };
                continue;
            }
            if token.text == "[" {
                let opening_span = self.token_span(token);
                self.index += 1;
                let property = self.parse_sequence()?;
                let Some(closing) = self.tokens.get(self.index) else {
                    return Err(unsupported(
                        opening_span,
                        "unterminated computed property access",
                    ));
                };
                if closing.text != "]" {
                    return Err(unsupported(
                        self.token_span(closing),
                        "expected `]` in computed property access",
                    ));
                }
                self.index += 1;
                expression = bluejs::Expr::Member {
                    object: Box::new(expression),
                    property: Box::new(property),
                    computed: true,
                };
                continue;
            }
            if token.text != "(" {
                return Ok(expression);
            }
            if !matches!(
                &expression,
                bluejs::Expr::Identifier(_) | bluejs::Expr::Member { .. } | bluejs::Expr::Super
            ) {
                return Err(unsupported(
                    self.token_span(token),
                    "only direct identifier and property calls are in the v1 direct bridge subset",
                ));
            }
            self.index += 1;
            let mut args = Vec::new();
            if self
                .tokens
                .get(self.index)
                .is_some_and(|token| token.text == ")")
            {
                self.index += 1;
            } else {
                loop {
                    args.push(self.parse_argument()?);
                    let Some(separator) = self.tokens.get(self.index) else {
                        return Err(unsupported(
                            SourceSpan::new(self.module, 0, 0),
                            "unterminated call expression",
                        ));
                    };
                    match separator.text.as_str() {
                        "," => self.index += 1,
                        ")" => {
                            self.index += 1;
                            break;
                        }
                        _ => {
                            return Err(unsupported(
                                self.token_span(separator),
                                "expected `,` or `)` in call expression",
                            ));
                        }
                    }
                }
            }
            expression = bluejs::Expr::Call {
                callee: Box::new(expression),
                args,
            };
        }
    }
}
