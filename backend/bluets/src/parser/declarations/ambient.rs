// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Static external module bodies and global augmentation retain source spans.

use super::*;

impl Parser {
    pub(super) fn parse_ambient_declaration(&mut self, start: usize) {
        let global = self.consume("global");
        let specifier_span;
        let specifier = if global {
            specifier_span = self.previous().span(&self.id);
            None
        } else {
            self.bump(); // The caller verified `module` followed by a string.
            specifier_span = self.current().span(&self.id);
            let Some(name) = string_contents(self.current()) else {
                self.error_here(
                    DiagnosticCode::ParseError,
                    "expected an ambient module name",
                );
                self.skip_statement();
                return;
            };
            self.bump();
            Some(name)
        };
        if !global && !self.peek("{") {
            let end = self.previous().end;
            self.consume(";");
            let span = SourceSpan::new(&self.id, start, end);
            self.edits.push(TextEdit {
                start: span.start,
                end: self.previous().end,
                replacement: String::new(),
            });
            self.declarations
                .push(Declaration::Ambient(AmbientDeclaration {
                    specifier,
                    specifier_span,
                    body: Vec::new(),
                    shorthand: true,
                    span,
                }));
            return;
        }
        if !self.peek("{") {
            self.error_here(
                DiagnosticCode::ParseError,
                "expected an ambient declaration body",
            );
            self.skip_statement();
            return;
        }
        let opening = self.index;
        let Some(closing) =
            matching_closing_delimiter(&self.tokens, opening, self.tokens.len() - 1, "{", "}")
        else {
            self.error_here(
                DiagnosticCode::ParseError,
                "unterminated ambient declaration body",
            );
            self.index = self.tokens.len() - 1;
            return;
        };
        let tail = self.tokens.split_off(closing);
        let closing_token = tail[0].clone();
        self.tokens.push(Token {
            kind: TokenKind::Eof,
            text: String::new(),
            start: closing_token.start,
            end: closing_token.start,
        });
        self.index = opening + 1;
        let outer = std::mem::take(&mut self.declarations);
        self.ambient_depth += 1;
        self.parse_items();
        self.ambient_depth -= 1;
        let body = std::mem::replace(&mut self.declarations, outer);
        self.tokens.pop();
        self.tokens.extend(tail);
        self.index = closing + 1;
        let span = SourceSpan::new(&self.id, start, closing_token.end);
        self.edits.push(TextEdit {
            start: span.start,
            end: span.end,
            replacement: String::new(),
        });
        self.declarations
            .push(Declaration::Ambient(AmbientDeclaration {
                specifier,
                specifier_span,
                body,
                shorthand: false,
                span,
            }));
    }
}
