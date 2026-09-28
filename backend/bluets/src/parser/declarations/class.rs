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
        let mut members = class_member_shells(&self.id, &body);
        for member in &mut members {
            if member.kind == ClassMemberKind::Constructor {
                self.parse_class_constructor(opening + 1 + member.token_start, member);
            } else if member.kind == ClassMemberKind::Opaque
                && body[member.token_start].is("constructor")
                && body
                    .get(member.token_start + 1)
                    .is_some_and(|token| token.is("("))
            {
                self.error_at(
                    member.span.clone(),
                    DiagnosticCode::ParseError,
                    "incomplete constructor declaration",
                );
            }
        }
        let span = SourceSpan::new(&self.id, start, self.tokens[closing].end);
        let name_span = name_token.span(&self.id);
        self.index = closing + 1;
        self.declarations.push(Declaration::Class(ClassDeclaration {
            name: name_token.text,
            name_span,
            extends_name: heritage.as_ref().map(|token| token.text.clone()),
            extends_span: heritage.as_ref().map(|token| token.span(&self.id)),
            body,
            members,
            body_span,
            exported,
            span,
        }));
    }

    fn parse_class_constructor(&mut self, start: usize, member: &mut ClassMemberShell) {
        self.index = start + 1;
        let parameters = self.parse_parameters();
        let body = if self.consume("{") {
            let body_start = self.previous().start;
            let mut body = Vec::new();
            let mut returns = Vec::new();
            let mut locals = Vec::new();
            self.parse_function_body(body_start, &mut body, &mut returns, &mut locals);
            Some(body)
        } else if self.consume(";") {
            None
        } else {
            self.error_here(DiagnosticCode::ParseError, "expected a constructor body");
            return;
        };
        if self.previous().end != member.span.end {
            self.error_at(
                member.span.clone(),
                DiagnosticCode::ParseError,
                "constructor body did not match its source boundary",
            );
            return;
        }
        member.constructor = Some(ClassConstructor {
            parameters,
            body,
            span: member.span.clone(),
        });
    }
}

fn class_member_shells(module: &str, tokens: &[Token]) -> Vec<ClassMemberShell> {
    let mut members = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        if tokens[index].is(";") {
            index += 1;
            continue;
        }
        let start = index;
        let name_token = &tokens[index];
        let method_head = matches!(name_token.kind, TokenKind::Identifier | TokenKind::Keyword)
            && tokens.get(index + 1).is_some_and(|token| token.is("("));
        if method_head {
            if let Some(parameters_end) =
                matching_closing_delimiter(tokens, index + 1, tokens.len(), "(", ")")
            {
                let mut after_signature = parameters_end + 1;
                if tokens
                    .get(after_signature)
                    .is_some_and(|token| token.is(":"))
                    && tokens.get(after_signature + 1).is_some_and(|token| {
                        matches!(token.kind, TokenKind::Identifier | TokenKind::Keyword)
                    })
                {
                    after_signature += 2;
                }
                let member_end = match tokens.get(after_signature) {
                    Some(token) if token.is("{") => {
                        matching_closing_delimiter(tokens, after_signature, tokens.len(), "{", "}")
                            .map(|end| end + 1)
                    }
                    Some(token) if token.is(";") => Some(after_signature + 1),
                    _ => None,
                };
                if let Some(end) = member_end {
                    members.push(class_member_shell(
                        module,
                        tokens,
                        start,
                        end,
                        if name_token.is("constructor") {
                            ClassMemberKind::Constructor
                        } else {
                            ClassMemberKind::Method
                        },
                        Some(name_token.text.clone()),
                    ));
                    index = end;
                    continue;
                }
            }
        }
        let delimiter = find_balanced_delimiter(tokens, index, tokens.len(), &[";"]);
        let end = if delimiter < tokens.len() {
            delimiter + 1
        } else {
            tokens.len()
        };
        members.push(class_member_shell(
            module,
            tokens,
            start,
            end,
            ClassMemberKind::Opaque,
            None,
        ));
        index = end;
    }
    members
}

fn class_member_shell(
    module: &str,
    tokens: &[Token],
    start: usize,
    end: usize,
    kind: ClassMemberKind,
    name: Option<String>,
) -> ClassMemberShell {
    ClassMemberShell {
        kind,
        name,
        token_start: start,
        token_end: end,
        span: SourceSpan::new(module, tokens[start].start, tokens[end - 1].end),
        constructor: None,
    }
}
