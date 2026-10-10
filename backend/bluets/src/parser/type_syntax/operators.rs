// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Conditional, mapped and template-literal type grammar.

use super::*;

impl Parser {
    pub(super) fn parse_conditional(&mut self, stop: &[&str]) -> Type {
        let start = self.current().start;
        let check = self.parse_union(stop);
        if stop.contains(&"extends") || !self.consume("extends") {
            return check;
        }
        self.infer_depth += 1;
        let extends = self.parse_type_until(&["?"]);
        self.infer_depth -= 1;
        self.expect("?");
        let true_start = self.current().start;
        let when_true = self.parse_type_until(&[":"]);
        let when_true_span = SourceSpan::new(&self.id, true_start, self.previous().end);
        self.expect(":");
        let when_false = self.parse_type_until(stop);
        Type::Conditional(Box::new(ConditionalType {
            check,
            extends,
            when_true,
            when_false,
            when_true_span,
            span: SourceSpan::new(&self.id, start, self.previous().end),
        }))
    }

    pub(super) fn parse_infer_type(&mut self) -> Type {
        let start = self.current().start;
        self.expect("infer");
        let name = self.require_identifier("expected an inferred type parameter name");
        let constraint = self
            .consume("extends")
            .then(|| self.parse_type_until(&["?", ",", ">", "]", ")", "}"]));
        let span = SourceSpan::new(&self.id, start, self.previous().end);
        if self.infer_depth == 0 {
            self.diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::TypeMismatch,
                    span.clone(),
                    "inferred type parameters require a conditional extends pattern",
                )
                .with_typescript(1338, vec![]),
            );
        }
        Type::Infer(Box::new(TypeParameter {
            is_const: false,
            variance: None,
            name,
            constraint,
            default: None,
            span,
        }))
    }

    pub(super) fn starts_mapped_type(&self) -> bool {
        let mut index = self.index;
        if self
            .tokens
            .get(index)
            .is_some_and(|token| token.is("+") || token.is("-"))
        {
            index += 1;
        }
        if self
            .tokens
            .get(index)
            .is_some_and(|token| token.is("readonly"))
        {
            index += 1;
        }
        self.tokens.get(index).is_some_and(|token| token.is("["))
            && self
                .tokens
                .get(index + 2)
                .is_some_and(|token| token.is("in"))
    }

    pub(super) fn parse_mapped_type(&mut self) -> Type {
        let start = self.previous().start;
        let remove = self.consume("-");
        self.consume("+");
        let readonly = if self.consume("readonly") {
            if remove {
                MappedModifier::Remove
            } else {
                MappedModifier::Add
            }
        } else {
            MappedModifier::Preserve
        };
        self.expect("[");
        let parameter_start = self.current().start;
        let name = self.require_identifier("expected a mapped type parameter name");
        self.expect("in");
        let constraint = self.parse_type_until(&["as", "]"]);
        let parameter = TypeParameter {
            is_const: false,
            variance: None,
            name,
            constraint: Some(constraint),
            default: None,
            span: SourceSpan::new(&self.id, parameter_start, self.previous().end),
        };
        let name_type = self.consume("as").then(|| self.parse_type_until(&["]"]));
        self.expect("]");
        let remove = self.consume("-");
        self.consume("+");
        let optional = if self.consume("?") {
            if remove {
                MappedModifier::Remove
            } else {
                MappedModifier::Add
            }
        } else {
            MappedModifier::Preserve
        };
        self.expect(":");
        let value = self.parse_type_until(&[";", "}"]);
        self.consume(";");
        self.expect("}");
        Type::Mapped(Box::new(MappedType {
            parameter,
            name_type,
            value,
            readonly,
            optional,
            span: SourceSpan::new(&self.id, start, self.previous().end),
        }))
    }

    pub(super) fn parse_template_type(&mut self) -> Type {
        let token = self.current().clone();
        self.bump();
        let bytes = token.text.as_bytes();
        let mut index = 1;
        let mut segment_start = index;
        let mut head = String::new();
        let mut spans = Vec::new();
        while index + 1 < bytes.len() {
            if bytes[index] == b'\\' {
                index += 2;
                continue;
            }
            if !bytes[index..].starts_with(b"${") {
                index += 1;
                continue;
            }
            let text = token.text[segment_start..index].to_string();
            if let Some((_, tail)) = spans.last_mut() {
                *tail = text;
            } else {
                head = text;
            }
            let start = token.start + index + 2;
            let (end, mut tokens) =
                match crate::syntax::lex_expression(&self.id, &self.source, start) {
                    Ok(value) => value,
                    Err(errors) => {
                        self.error_at(token.span(&self.id), DiagnosticCode::ParseError, errors);
                        break;
                    }
                };
            tokens.push(Token {
                kind: TokenKind::Eof,
                text: String::new(),
                start: end,
                end,
            });
            let original = std::mem::replace(
                &mut self.tokens,
                split_generic_closers(merge_private_names(tokens)),
            );
            let saved = self.index;
            self.index = 0;
            let value = self.parse_type_until(&[]);
            if !self.at_eof() {
                self.error_here(DiagnosticCode::ParseError, "unexpected template type token");
            }
            self.index = saved;
            self.tokens = original;
            spans.push((value, String::new()));
            index = end - token.start + 1;
            segment_start = index;
        }
        let text = token.text[segment_start..bytes.len() - 1].to_string();
        if let Some((_, tail)) = spans.last_mut() {
            *tail = text;
        } else {
            head = text;
        }
        Type::TemplateLiteral(TemplateLiteralType {
            head,
            spans,
            span: token.span(&self.id),
        })
    }
}
