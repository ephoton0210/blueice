// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Type expressions and common parser cursor operations.

use super::runtime_syntax::*;
use super::*;

impl Parser {
    pub(super) fn parse_method_signature(&mut self, result_stop: &[&str]) -> Type {
        self.expect("(");
        let mut parameters = Vec::new();
        while !self.at_eof() && !self.consume(")") {
            let start = self.current().start;
            if self.consume("...") {
                self.unsupported(
                    self.previous().span(&self.id),
                    "rest parameters in method signatures are not supported",
                );
            }
            let name = self.require_identifier("expected a method parameter name");
            let optional = self.consume("?");
            self.expect(":");
            let annotation = self.parse_type_until(&[",", ")"]);
            let end = self.previous().end;
            parameters.push(Parameter {
                name,
                rest: false,
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
            result: Box::new(self.parse_type_until(result_stop)),
        }
    }

    pub(super) fn parse_type_parameters(&mut self) -> Vec<TypeParameter> {
        if !self.consume("<") {
            return Vec::new();
        }
        let mut parameters = Vec::new();
        while !self.at_eof() && !self.consume(">") {
            let start = self.current().start;
            let name = self.require_identifier("expected a type parameter name");
            let constraint = self
                .consume("extends")
                .then(|| self.parse_type_until(&["=", ",", ">"]));
            let default = self
                .consume("=")
                .then(|| self.parse_type_until(&[",", ">"]));
            let end = self.previous().end;
            parameters.push(TypeParameter {
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
        let value = self.parse_union(stop);
        if self.index == type_start && !self.at_eof() && !stop.iter().any(|stop| self.peek(stop)) {
            self.error_here(DiagnosticCode::ParseError, "expected a type");
            self.bump();
        }
        self.type_depth -= 1;
        value
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
        let mut value = if self.consume("readonly") {
            self.parse_type_primary(&[])
        } else if self.consume("[") {
            let mut values = Vec::new();
            while !self.at_eof() && !self.consume("]") {
                values.push(self.parse_type_until(&[",", "]"]));
                if !self.consume(",") {
                    self.expect("]");
                    break;
                }
            }
            Type::Tuple(values)
        } else if self.consume("{") {
            let mut fields = Vec::new();
            while !self.at_eof() && !self.consume("}") {
                let start = self.current().start;
                self.consume("readonly");
                let name = self.require_identifier("expected a record field name");
                let optional = self.consume("?");
                let value = if self.peek("(") {
                    self.parse_method_signature(&[";", ",", "}"])
                } else {
                    self.expect(":");
                    self.parse_type_until(&[";", ",", "}"])
                };
                let end = self.previous().end;
                fields.push(TypeField {
                    name,
                    optional,
                    value,
                    span: SourceSpan::new(&self.id, start, end),
                });
                self.consume(";");
                self.consume(",");
            }
            Type::Record(fields)
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
        } else if let Some(mut name) = self.consume_identifier_or_keyword() {
            while self.consume(".") {
                let member = self.require_identifier("expected a qualified type name");
                name.push('.');
                name.push_str(&member);
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
                "unknown" => Type::Unknown,
                "never" => Type::Never,
                "void" => Type::Void,
                "boolean" => Type::Boolean,
                "number" => Type::Number,
                "string" => Type::String,
                _ => Type::Named { name, arguments },
            }
        } else {
            self.error_here(DiagnosticCode::ParseError, "expected a type");
            Type::Unknown
        };

        while self.consume("[") {
            self.expect("]");
            value = Type::Array(Box::new(value));
        }
        value
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

    pub(super) fn consume_identifier(&mut self) -> Option<String> {
        if self.current().kind == TokenKind::Identifier {
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
