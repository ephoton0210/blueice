// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Type expressions and common parser cursor operations.

#[path = "type_syntax/callable_objects.rs"]
mod callable_objects;
#[path = "type_syntax/operators.rs"]
mod operators;

use super::runtime_syntax::*;
use super::*;

impl Parser {
    pub(super) fn parse_method_signature(&mut self, result_stop: &[&str]) -> Type {
        self.expect("(");
        let mut parameters = Vec::new();
        while !self.at_eof() && !self.consume(")") {
            let start = self.current().start;
            let rest = self.consume("...");
            let name = self.require_identifier("expected a method parameter name");
            let optional = self.consume("?");
            self.expect(":");
            let annotation = self.parse_type_until(&[",", ")"]);
            let end = self.previous().end;
            parameters.push(Parameter {
                decorators: Vec::new(),
                name,
                pattern: None,
                rest,
                optional,
                annotation: Some(annotation),
                default: None,
                span: SourceSpan::new(&self.id, start, end),
            });
            if !self.consume(",") {
                self.expect(")");
                break;
            }
        }
        self.expect(":");
        Type::Function {
            parameters,
            result: Box::new(self.parse_return_type_until(result_stop)),
        }
    }

    pub(super) fn parse_type_parameters(&mut self) -> Vec<TypeParameter> {
        if !self.consume("<") {
            return Vec::new();
        }
        let mut parameters = Vec::new();
        while !self.at_eof() && !self.consume(">") {
            let start = self.current().start;
            let variance = if self.consume("in") {
                Some(if self.consume("out") {
                    Variance::InOut
                } else {
                    Variance::In
                })
            } else if self.consume("out") {
                Some(Variance::Out)
            } else {
                None
            };
            let name = self.require_identifier("expected a type parameter name");
            let constraint = self
                .consume("extends")
                .then(|| self.parse_type_until(&["=", ",", ">"]));
            let default = self
                .consume("=")
                .then(|| self.parse_type_until(&[",", ">"]));
            let end = self.previous().end;
            parameters.push(TypeParameter {
                variance,
                name,
                constraint,
                default,
                span: SourceSpan::new(&self.id, start, end),
            });
            if !self.consume(",") {
                self.expect(">");
                break;
            }
        }
        parameters
    }

    pub(super) fn parse_type_until(&mut self, stop: &[&str]) -> Type {
        self.parse_type_expression_until(stop, false)
    }

    pub(super) fn parse_return_type_until(&mut self, stop: &[&str]) -> Type {
        self.parse_type_expression_until(stop, true)
    }

    fn parse_type_expression_until(&mut self, stop: &[&str], return_position: bool) -> Type {
        if self.type_depth >= self.max_type_depth {
            self.error_here(
                DiagnosticCode::ResourceLimit,
                format!(
                    "type expression exceeds the {} nesting limit",
                    self.max_type_depth
                ),
            );
            self.skip_type_until(stop);
            return Type::Unknown;
        }
        self.type_depth += 1;
        let type_start = self.index;
        let assertion = self.peek("asserts")
            && self.tokens.get(self.index + 1).is_some_and(|token| {
                !token.is("is") && (token.kind == TokenKind::Identifier || token.is("this"))
            });
        let predicate = assertion
            || ((self.current().kind == TokenKind::Identifier || self.peek("this"))
                && self
                    .tokens
                    .get(self.index + 1)
                    .is_some_and(|token| token.is("is")));
        let value = if predicate {
            self.parse_predicate_until(stop, assertion, return_position)
        } else {
            self.parse_conditional(stop)
        };
        if self.index == type_start && !self.at_eof() && !stop.iter().any(|stop| self.peek(stop)) {
            self.error_here(DiagnosticCode::ParseError, "expected a type");
            self.bump();
        }
        self.type_depth -= 1;
        value
    }

    fn parse_predicate_until(
        &mut self,
        stop: &[&str],
        asserts: bool,
        return_position: bool,
    ) -> Type {
        let start = self.current().start;
        if asserts {
            self.bump();
        }
        let parameter = self.current().clone();
        self.bump();
        if !return_position && !asserts {
            self.type_references.push(TypeReference {
                name: parameter.text.clone(),
                value_query: false,
                span: parameter.span(&self.id),
            });
        }
        let is_span = self.peek("is").then(|| self.current().span(&self.id));
        let (target, target_span) = if self.consume("is") {
            let start = self.current().start;
            let value = self.parse_type_until(stop);
            let span = SourceSpan::new(&self.id, start, self.previous().end);
            if return_position {
                // Polymorphic `this` is resolved in the containing signature,
                // rather than as a module-owned named type.
                self.type_references.retain(|reference| {
                    reference.name != "this"
                        || reference.span.start < span.start
                        || reference.span.end > span.end
                });
            }
            (Some(Box::new(value)), Some(span))
        } else {
            (None, None)
        };
        Type::Predicate(Box::new(TypePredicate {
            parameter: parameter.text.clone(),
            asserts,
            target,
            span: SourceSpan::new(&self.id, start, self.previous().end),
            parameter_span: parameter.span(&self.id),
            is_span,
            target_span,
            return_position,
        }))
    }

    pub(super) fn skip_type_until(&mut self, stop: &[&str]) {
        let mut nesting = 0usize;
        while !self.at_eof() {
            match self.current().text.as_str() {
                "{" | "[" | "<" => nesting += 1,
                "}" | "]" | ">" if nesting > 0 => nesting -= 1,
                _ if nesting == 0 && stop.iter().any(|stop| self.peek(stop)) => break,
                _ => {}
            }
            self.bump();
        }
    }

    pub(super) fn parse_union(&mut self, stop: &[&str]) -> Type {
        let mut values = vec![self.parse_intersection(stop)];
        while self.consume("|") {
            values.push(self.parse_intersection(stop));
        }
        if values.len() == 1 {
            values.pop().expect("one parsed type")
        } else {
            Type::Union(values)
        }
    }

    pub(super) fn parse_intersection(&mut self, stop: &[&str]) -> Type {
        let mut values = vec![self.parse_type_primary(stop)];
        while self.consume("&") {
            values.push(self.parse_type_primary(stop));
        }
        if values.len() == 1 {
            values.pop().expect("one parsed type")
        } else {
            Type::Intersection(values)
        }
    }

    pub(super) fn parse_type_primary(&mut self, _stop: &[&str]) -> Type {
        let mut value = if self.peek("new")
            || (self.peek("abstract")
                && self
                    .tokens
                    .get(self.index + 1)
                    .is_some_and(|token| token.is("new")))
        {
            self.parse_constructor_type(_stop)
        } else if self.peek("infer") {
            self.parse_infer_type()
        } else if self.current().kind == TokenKind::Template {
            self.parse_template_type()
        } else if self.consume("keyof") {
            Type::KeyOf(Box::new(self.parse_type_primary(_stop)))
        } else if self.peek("<") {
            let start = self.current().start;
            let type_parameters = self.parse_type_parameters();
            let function = self.parse_type_primary(_stop);
            match function {
                Type::Function { parameters, result } => Type::GenericFunction {
                    type_parameters,
                    parameters,
                    result,
                    span: SourceSpan::new(&self.id, start, self.previous().end),
                },
                _ => {
                    self.error_here(
                        DiagnosticCode::ParseError,
                        "expected a generic function type",
                    );
                    Type::Unknown
                }
            }
        } else if self.consume("readonly") {
            Type::Readonly(Box::new(self.parse_type_primary(&[])))
        } else if self.consume("unique") {
            let start = self.previous().start;
            self.expect("symbol");
            Type::UniqueSymbol(SourceSpan::new(&self.id, start, self.previous().end))
        } else if self.consume("[") {
            let mut values = Vec::new();
            let mut saw_optional = false;
            let mut saw_rest = false;
            while !self.at_eof() && !self.consume("]") {
                let start = self.current().start;
                let rest = self.consume("...");
                let labeled = matches!(
                    self.current().kind,
                    TokenKind::Identifier | TokenKind::Keyword
                ) && (self
                    .tokens
                    .get(self.index + 1)
                    .is_some_and(|token| token.is(":"))
                    || (self
                        .tokens
                        .get(self.index + 1)
                        .is_some_and(|token| token.is("?"))
                        && self
                            .tokens
                            .get(self.index + 2)
                            .is_some_and(|token| token.is(":"))));
                let (label, optional) = if labeled {
                    let label = self.consume_identifier_or_keyword();
                    let optional = self.consume("?");
                    self.expect(":");
                    (label, optional)
                } else {
                    (None, false)
                };
                let annotation = self.parse_type_until(if labeled {
                    &[",", "]"]
                } else {
                    &["?", ",", "]"]
                });
                let end = self.previous().end;
                let optional = optional || (!labeled && self.consume("?"));
                if saw_rest {
                    if rest {
                        self.error_at(
                            SourceSpan::new(&self.id, start, end),
                            DiagnosticCode::ParseError,
                            "a second tuple rest element is not allowed",
                        );
                    } else if optional {
                        self.error_at(
                            SourceSpan::new(&self.id, start, end),
                            DiagnosticCode::ParseError,
                            "an optional tuple element cannot follow a rest element",
                        );
                    }
                }
                if rest && !matches!(annotation, Type::Array(_) | Type::Named { .. }) {
                    self.unsupported(
                        SourceSpan::new(&self.id, start, end),
                        "tuple rest element must have an array or named tuple annotation",
                    );
                }
                if saw_optional && !optional && !rest {
                    self.error_at(
                        SourceSpan::new(&self.id, start, end),
                        DiagnosticCode::ParseError,
                        "a required tuple element cannot follow an optional element",
                    );
                }
                saw_optional |= optional;
                saw_rest |= rest;
                values.push(TupleTypeElement {
                    optional,
                    label,
                    rest,
                    ..TupleTypeElement::required(annotation)
                });
                if !self.consume(",") {
                    self.expect("]");
                    break;
                }
            }
            Type::Tuple(values)
        } else if self.consume("{") {
            if self.starts_mapped_type() {
                self.parse_mapped_type()
            } else {
                self.parse_record_type("expected a record field name")
            }
        } else if self.peek("(") && !self.parenthesis_starts_function_type() {
            // A parenthesized type only groups: `(A | B)[]`.
            self.bump();
            let grouped = self.parse_type_until(&[")"]);
            self.expect(")");
            grouped
        } else if self.consume("(") {
            let mut parameters = Vec::new();
            while !self.at_eof() && !self.consume(")") {
                let start = self.current().start;
                let rest = self.consume("...");
                let name = self.require_identifier("expected a function type parameter name");
                let optional = self.consume("?");
                self.expect(":");
                let annotation = self.parse_type_until(&[",", ")"]);
                let end = self.previous().end;
                parameters.push(Parameter {
                    decorators: Vec::new(),
                    name,
                    pattern: None,
                    rest,
                    optional,
                    annotation: Some(annotation),
                    default: None,
                    span: SourceSpan::new(&self.id, start, end),
                });
                if !self.consume(",") {
                    self.expect(")");
                    break;
                }
            }
            self.expect("=>");
            Type::Function {
                parameters,
                result: Box::new(self.parse_return_type_until(_stop)),
            }
        } else if self.consume("-") {
            if self.current().kind == TokenKind::Number {
                let literal = Type::Literal(format!("-{}", self.current().text));
                self.bump();
                literal
            } else {
                self.error_here(
                    DiagnosticCode::ParseError,
                    "expected a numeric literal type",
                );
                Type::Unknown
            }
        } else if self.current().kind == TokenKind::String
            || self.current().kind == TokenKind::Number
            || self.peek("true")
            || self.peek("false")
        {
            let value = Type::Literal(self.current().text.clone());
            self.bump();
            value
        } else if self.consume("null") {
            Type::Null
        } else if self.consume("undefined") {
            Type::Undefined
        } else if self.consume("typeof") {
            let start = self.current().start;
            let mut name = self.consume_identifier_or_keyword().unwrap_or_else(|| {
                self.error_here(
                    DiagnosticCode::ParseError,
                    "expected a value name after `typeof`",
                );
                String::new()
            });
            while self.consume(".") {
                name.push('.');
                name.push_str(&self.require_identifier("expected a qualified value name"));
            }
            self.type_references.push(TypeReference {
                name: name.clone(),
                value_query: true,
                span: SourceSpan::new(&self.id, start, self.previous().end),
            });
            Type::Named {
                name: format!("typeof {name}@{start}"),
                arguments: Vec::new(),
            }
        } else if let Some(mut name) = self.consume_identifier_or_keyword() {
            let name_start = self.previous().start;
            while self.consume(".") {
                let member = self.require_identifier("expected a qualified type name");
                name.push('.');
                name.push_str(&member);
            }
            let name_end = self.previous().end;
            if !matches!(
                name.as_str(),
                "any"
                    | "unknown"
                    | "never"
                    | "void"
                    | "boolean"
                    | "number"
                    | "string"
                    | "bigint"
                    | "symbol"
            ) {
                self.type_references.push(TypeReference {
                    name: name.clone(),
                    value_query: false,
                    span: SourceSpan::new(&self.id, name_start, name_end),
                });
            }
            let mut arguments = Vec::new();
            if self.consume("<") {
                while !self.at_eof() && !self.consume(">") {
                    arguments.push(self.parse_type_until(&[",", ">"]));
                    if !self.consume(",") {
                        self.expect(">");
                        break;
                    }
                }
            }
            match name.as_str() {
                "any" => Type::Any,
                "unknown" => Type::StrictUnknown,
                "never" => Type::Never,
                "void" => Type::Void,
                "boolean" => Type::Boolean,
                "number" => Type::Number,
                "bigint" => Type::BigInt,
                "symbol" => Type::Symbol,
                "string" => Type::String,
                _ => Type::Named { name, arguments },
            }
        } else {
            self.error_here(DiagnosticCode::ParseError, "expected a type");
            Type::Unknown
        };

        while self.consume("[") {
            value = if self.consume("]") {
                Type::Array(Box::new(value))
            } else {
                let start = self.current().start;
                let index = self.parse_type_until(&["]"]);
                let index_span = SourceSpan::new(&self.id, start, self.previous().end);
                self.expect("]");
                Type::IndexedAccess {
                    object: Box::new(value),
                    index: Box::new(index),
                    index_span,
                }
            };
        }
        value
    }

    /// Whether the `(` at the cursor opens a function type, that is, whether
    /// its matching `)` is followed by `=>`.
    fn parenthesis_starts_function_type(&self) -> bool {
        let mut depth = 0usize;
        for (offset, token) in self.tokens[self.index..].iter().enumerate() {
            match token.text.as_str() {
                "(" | "[" | "{" => depth += 1,
                ")" | "]" | "}" => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return self
                            .tokens
                            .get(self.index + offset + 1)
                            .is_some_and(|next| next.is("=>"));
                    }
                }
                _ => {}
            }
        }
        false
    }

    pub(super) fn collect_until_statement_end(&mut self) -> Vec<Token> {
        let end = find_balanced_delimiter(&self.tokens, self.index, self.tokens.len() - 1, &[";"]);
        let values = self.tokens[self.index..end].to_vec();
        self.index = end;
        values
    }

    /// Collects one function-body expression statement. Unlike a top-level
    /// declaration initializer, an expression at the end of a function may
    /// rely on automatic semicolon insertion before the enclosing `}`, so a
    /// top-level closing brace is also a statement boundary here.
    pub(super) fn collect_until_function_statement_end(&mut self) -> Vec<Token> {
        let start = self.index;
        let mut parentheses = 0usize;
        let mut brackets = 0usize;
        let mut braces = 0usize;
        while !self.at_eof() {
            let token = self.current();
            let boundary = matches!(token.text.as_str(), ";" | "}")
                && parentheses == 0
                && brackets == 0
                && braces == 0;
            if boundary {
                break;
            }
            match token.text.as_str() {
                "(" => parentheses += 1,
                ")" if parentheses > 0 => parentheses -= 1,
                "[" => brackets += 1,
                "]" if brackets > 0 => brackets -= 1,
                "{" => braces += 1,
                "}" if braces > 0 => braces -= 1,
                _ => {}
            }
            self.bump();
        }
        self.tokens[start..self.index].to_vec()
    }

    pub(super) fn skip_statement(&mut self) {
        let end = find_balanced_delimiter(&self.tokens, self.index, self.tokens.len() - 1, &[";"]);
        self.index = if self.tokens[end].is(";") {
            end + 1
        } else {
            end
        };
    }

    pub(super) fn skip_until(&mut self, stops: &[&str]) {
        while !self.at_eof() && !stops.iter().any(|stop| self.peek(stop)) {
            self.bump();
        }
    }

    pub(super) fn expect(&mut self, text: &str) {
        if !self.consume(text) {
            self.error_here(DiagnosticCode::ParseError, format!("expected `{text}`"));
        }
    }

    pub(super) fn require_identifier(&mut self, message: impl Into<String>) -> String {
        self.consume_identifier().unwrap_or_else(|| {
            self.error_here(DiagnosticCode::ParseError, message);
            "<error>".to_string()
        })
    }

    pub(super) fn require_property_name(&mut self, message: impl Into<String>) -> String {
        if matches!(self.current().kind, TokenKind::Number | TokenKind::String) {
            let name = self.current().text.trim_matches(['\'', '"']).to_string();
            self.bump();
            return name;
        }
        self.consume_identifier_or_keyword().unwrap_or_else(|| {
            self.error_here(DiagnosticCode::ParseError, message);
            "<error>".to_string()
        })
    }

    pub(super) fn consume_identifier(&mut self) -> Option<String> {
        // Contextual TypeScript words can still name a binding or a type.
        if self.current().kind == TokenKind::Identifier
            || matches!(self.current().text.as_str(), "of" | "type")
        {
            let value = self.current().text.clone();
            self.bump();
            Some(value)
        } else {
            None
        }
    }

    pub(super) fn consume_identifier_or_keyword(&mut self) -> Option<String> {
        if matches!(
            self.current().kind,
            TokenKind::Identifier | TokenKind::Keyword
        ) {
            let value = self.current().text.clone();
            self.bump();
            Some(value)
        } else {
            None
        }
    }

    pub(super) fn consume(&mut self, text: &str) -> bool {
        if self.peek(text) {
            self.bump();
            true
        } else {
            false
        }
    }

    pub(super) fn peek(&self, text: &str) -> bool {
        self.current().is(text)
    }

    pub(super) fn peek_any(&self, texts: &[&str]) -> bool {
        texts.iter().any(|text| self.peek(text))
    }

    pub(super) fn current(&self) -> &Token {
        &self.tokens[self.index]
    }

    pub(super) fn previous(&self) -> &Token {
        &self.tokens[self.index.saturating_sub(1)]
    }

    pub(super) fn bump(&mut self) {
        if !self.at_eof() {
            self.index += 1;
        }
    }

    pub(super) fn at_eof(&self) -> bool {
        self.current().kind == TokenKind::Eof
    }

    pub(super) fn error_here(&mut self, code: DiagnosticCode, message: impl Into<String>) {
        let span = self.current().span(&self.id);
        self.error_at(span, code, message);
    }

    pub(super) fn error_at(
        &mut self,
        span: SourceSpan,
        code: DiagnosticCode,
        message: impl Into<String>,
    ) {
        self.diagnostics
            .push(Diagnostic::error(code, span, message));
    }

    pub(super) fn unsupported(&mut self, span: SourceSpan, message: impl Into<String>) {
        self.error_at(span, DiagnosticCode::UnsupportedSyntax, message);
    }
}
