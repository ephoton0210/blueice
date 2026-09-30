// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `namespace` and `module` declarations with identifier names.

use super::*;

impl Parser {
    /// Whether a line break separates two tokens, which ends a `namespace`
    /// or `module` expression-statement reading.
    pub(super) fn newline_between(&self, left: usize, right: usize) -> bool {
        let (left, right) = (&self.tokens[left], &self.tokens[right]);
        self.source
            .get(left.end..right.start)
            .is_some_and(|between| between.contains('\n'))
    }

    /// Parses the rest of `[export] [declare] namespace A.B { .. }` with the
    /// cursor just after the `namespace` or `module` keyword.
    pub(in crate::parser::implementation) fn parse_namespace(
        &mut self,
        start: usize,
        exported: bool,
        declared: bool,
    ) {
        let mut names = Vec::new();
        loop {
            if self.current().kind != TokenKind::Identifier {
                self.error_here(DiagnosticCode::ParseError, "expected a namespace name");
                self.skip_statement();
                return;
            }
            names.push((self.current().text.clone(), self.current().span(&self.id)));
            self.bump();
            if !self.consume(".") {
                break;
            }
        }
        if !self.peek("{") {
            self.error_here(DiagnosticCode::ParseError, "expected a namespace body");
            self.skip_statement();
            return;
        }
        let opening = self.index;
        let Some(closing) =
            matching_closing_delimiter(&self.tokens, opening, self.tokens.len() - 1, "{", "}")
        else {
            self.error_here(DiagnosticCode::ParseError, "unterminated namespace body");
            self.index = self.tokens.len() - 1;
            return;
        };
        // Parse the body as its own token stream: the closing brace becomes the
        // end of input, so no item scan can run past it.
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
        self.namespace_depth += 1;
        self.ambient_depth += usize::from(declared);
        self.parse_items();
        self.ambient_depth -= usize::from(declared);
        self.namespace_depth -= 1;
        let body = std::mem::replace(&mut self.declarations, outer);
        self.tokens.pop();
        self.tokens.extend(tail);
        self.index = closing + 1;

        let header_span = SourceSpan::new(&self.id, start, self.tokens[opening].end);
        let closing_span = closing_token.span(&self.id);
        let span = SourceSpan::new(&self.id, start, closing_token.end);
        // `A.B.C` is `A { export B { export C { .. } } }`.
        let mut declaration = None;
        for (index, (name, name_span)) in names.into_iter().enumerate().rev() {
            let inner = declaration.take();
            declaration = Some(NamespaceDeclaration {
                name,
                name_span,
                exported: if index == 0 { exported } else { true },
                declared,
                implicit: index > 0,
                body: match inner {
                    Some(inner) => vec![Declaration::Namespace(inner)],
                    None => body.clone(),
                },
                header_span: header_span.clone(),
                closing_span: closing_span.clone(),
                span: span.clone(),
            });
        }
        if let Some(declaration) = declaration {
            self.declarations.push(Declaration::Namespace(declaration));
        }
    }
}
