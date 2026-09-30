// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `enum` and `const enum` declarations.

use super::*;

impl Parser {
    /// Parses the rest of `[export] [declare] [const] enum Name { .. }` with the
    /// cursor just after the `enum` keyword.
    pub(in crate::parser::implementation) fn parse_enum(
        &mut self,
        start: usize,
        exported: bool,
        declared: bool,
        is_const: bool,
    ) {
        if !matches!(self.current().kind, TokenKind::Identifier) {
            self.error_here(DiagnosticCode::ParseError, "expected an enum name");
            self.skip_statement();
            return;
        }
        let name_token = self.current().clone();
        self.bump();
        if !self.peek("{") {
            self.error_here(DiagnosticCode::ParseError, "expected an enum body");
            self.skip_statement();
            return;
        }
        let opening = self.index;
        let Some(closing) =
            matching_closing_delimiter(&self.tokens, opening, self.tokens.len() - 1, "{", "}")
        else {
            self.error_here(DiagnosticCode::ParseError, "unterminated enum body");
            self.index = self.tokens.len() - 1;
            return;
        };
        let mut members = Vec::new();
        let mut index = opening + 1;
        while index < closing {
            if self.tokens[index].is(",") {
                index += 1;
                continue;
            }
            let member_start = index;
            let token = self.tokens[index].clone();
            let name = match token.kind {
                TokenKind::Identifier | TokenKind::Keyword => token.text.clone(),
                TokenKind::String => match decode_enum_member_name(&token.text) {
                    Some(name) => name,
                    None => {
                        self.unsupported(
                            token.span(&self.id),
                            "an enum member name with an escape sequence is not supported yet",
                        );
                        index = find_balanced_delimiter(&self.tokens, index, closing, &[","]);
                        continue;
                    }
                },
                _ => {
                    self.error_at(
                        token.span(&self.id),
                        DiagnosticCode::ParseError,
                        "expected an enum member name",
                    );
                    index = find_balanced_delimiter(&self.tokens, index, closing, &[","]);
                    continue;
                }
            };
            index += 1;
            let mut initializer = None;
            if index < closing && self.tokens[index].is("=") {
                let value_start = index + 1;
                let value_end = find_balanced_delimiter(&self.tokens, value_start, closing, &[","]);
                if value_start >= value_end {
                    self.error_at(
                        token.span(&self.id),
                        DiagnosticCode::ParseError,
                        "expected an enum member initializer",
                    );
                } else {
                    // Type assertions and nested functions in a computed
                    // initializer are erased like any other expression.
                    self.collect_expression_type_edits(value_start, value_end);
                    initializer = Some(self.tokens[value_start..value_end].to_vec());
                }
                index = value_end;
            } else if index < closing && !self.tokens[index].is(",") {
                self.error_at(
                    self.tokens[index].span(&self.id),
                    DiagnosticCode::ParseError,
                    "expected `,` or `}` after an enum member",
                );
                index = find_balanced_delimiter(&self.tokens, index, closing, &[","]);
            }
            let member_end = self.tokens[index.saturating_sub(1).max(member_start)].end;
            members.push(EnumMember {
                name,
                name_span: token.span(&self.id),
                initializer,
                span: SourceSpan::new(&self.id, token.start, member_end),
            });
        }
        let body_span = SourceSpan::new(
            &self.id,
            self.tokens[opening].start,
            self.tokens[closing].end,
        );
        let span = SourceSpan::new(&self.id, start, self.tokens[closing].end);
        self.index = closing + 1;
        self.declarations.push(Declaration::Enum(EnumDeclaration {
            name: name_token.text.clone(),
            name_span: name_token.span(&self.id),
            exported,
            declared,
            is_const,
            members,
            body_span,
            span,
        }));
    }
}

/// The value of a string-literal member name, or `None` when it uses an escape
/// sequence.
fn decode_enum_member_name(text: &str) -> Option<String> {
    let inner = text.get(1..text.len().checked_sub(1)?)?;
    (!inner.contains('\\')).then(|| inner.to_string())
}
