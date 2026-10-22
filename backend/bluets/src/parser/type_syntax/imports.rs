// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Retain static import syntax without manufacturing an executable import.
use super::*;

impl Parser {
    pub(super) fn parse_import_type(&mut self, value_query: bool) -> Type {
        let start = self.current().start;
        self.expect("import");
        self.expect("(");
        let literal = self.current().clone();
        let specifier = if literal.kind == TokenKind::String {
            string_contents(&literal).unwrap_or_default()
        } else {
            self.diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::ParseError,
                    literal.span(&self.id),
                    "a type import requires a string literal",
                )
                .with_typescript(1141, Vec::new()),
            );
            String::new()
        };
        self.bump();
        self.expect(")");
        let mut qualifier = String::new();
        let mut member_span = SourceSpan::new(&self.id, start, self.previous().end);
        while self.consume(".") {
            let token = self.current().clone();
            let member = self.consume_identifier_or_keyword().unwrap_or_else(|| {
                self.error_here(
                    DiagnosticCode::ParseError,
                    "expected an imported type member",
                );
                String::new()
            });
            if !qualifier.is_empty() {
                qualifier.push('.');
            }
            qualifier.push_str(&member);
            member_span = token.span(&self.id);
        }
        let spelling = format!(
            "{}import({}){}{}",
            if value_query { "typeof " } else { "" },
            literal.text,
            if qualifier.is_empty() { "" } else { "." },
            qualifier
        );
        let name = ImportTypeReference::identity(&self.id, &spelling);
        let mut arguments = Vec::new();
        let mut argument_spans = Vec::new();
        if self.consume("<") {
            while !self.at_eof() && !self.consume(">") {
                let start = self.current().start;
                arguments.push(self.parse_type_until(&[",", ">"]));
                argument_spans.push(SourceSpan::new(&self.id, start, self.previous().end));
                if !self.consume(",") {
                    self.expect(">");
                    break;
                }
            }
        }
        self.import_types.push(ImportTypeReference {
            name: name.clone(),
            specifier,
            specifier_span: literal.span(&self.id),
            qualifier,
            member_span,
            value_query,
            arguments: arguments.clone(),
            argument_spans,
            span: SourceSpan::new(&self.id, start, self.previous().end),
        });
        Type::Named { name, arguments }
    }
}
