// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! First bounded class-declaration shell and source provenance.

use super::*;

impl Parser {
    pub(in crate::parser::implementation) fn parse_class(&mut self, start: usize, exported: bool) {
        if self.current().kind != TokenKind::Identifier {
            self.error_here(DiagnosticCode::ParseError, "expected a class name");
            self.index = self.tokens.len() - 1;
            return;
        }
        let name_token = self.current().clone();
        self.bump();

        let heritage = if self.consume("extends") {
            if self.current().kind != TokenKind::Identifier {
                self.error_here(DiagnosticCode::ParseError, "expected a class heritage name");
                self.index = self.tokens.len() - 1;
                return;
            }
            let token = self.current().clone();
            self.bump();
            Some(token)
        } else {
            None
        };

        if !self.peek("{") {
            let token = self.current().clone();
            if matches!(token.text.as_str(), "<" | "." | "[" | "(" | "implements") {
                self.unsupported(
                    token.span(&self.id),
                    "generic, computed, and implemented class heritage is not in the first class form",
                );
            } else {
                self.error_here(DiagnosticCode::ParseError, "expected a class body");
            }
            self.index = self.tokens.len() - 1;
            return;
        }

        let opening = self.index;
        let Some(closing) =
            matching_closing_delimiter(&self.tokens, opening, self.tokens.len() - 1, "{", "}")
        else {
            self.error_here(DiagnosticCode::ParseError, "unterminated class body");
            self.index = self.tokens.len() - 1;
            return;
        };
        let body_span = SourceSpan::new(
            &self.id,
            self.tokens[opening].start,
            self.tokens[closing].end,
        );
        let body = self.tokens[opening + 1..closing].to_vec();
        let span = SourceSpan::new(&self.id, start, self.tokens[closing].end);
        let name_span = name_token.span(&self.id);
        self.index = closing + 1;
        self.declarations.push(Declaration::Class(ClassDeclaration {
            name: name_token.text,
            name_span,
            extends_name: heritage.as_ref().map(|token| token.text.clone()),
            extends_span: heritage.as_ref().map(|token| token.span(&self.id)),
            body,
            body_span,
            exported,
            span,
        }));
    }
}
