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
            } else if member.kind == ClassMemberKind::Method {
                self.parse_class_method(opening + 1 + member.token_start, member);
            } else if member.kind == ClassMemberKind::Opaque
                && matches!(
                    body[member.token_start].kind,
                    TokenKind::Identifier | TokenKind::Keyword
                )
                && body
                    .get(member.token_start + 1)
                    .is_some_and(|token| token.is("("))
            {
                self.error_at(
                    member.span.clone(),
                    DiagnosticCode::ParseError,
                    "incomplete class method declaration",
                );
            }
        }
        let method_groups = class_method_groups(&self.id, &members);
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
            method_groups,
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

    fn parse_class_method(&mut self, start: usize, member: &mut ClassMemberShell) {
        let is_static = self.tokens[start].is("static")
            && self
                .tokens
                .get(start + 2)
                .is_some_and(|token| token.is("("));
        let name_index = start + usize::from(is_static);
        let name = self.tokens[name_index].text.clone();
        self.index = name_index + 1;
        let parameters = self.parse_parameters();
        let return_start = self.current().start;
        let (return_type, return_type_span) = if self.consume(":") {
            let type_start = self.current().start;
            let value = self.parse_type_until(&["{", ";"]);
            let type_end = self.previous().end;
            self.edits.push(TextEdit {
                start: return_start,
                end: self.current().start,
                replacement: String::new(),
            });
            (
                Some(value),
                Some(SourceSpan::new(&self.id, type_start, type_end)),
            )
        } else {
            (None, None)
        };
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
            self.error_here(DiagnosticCode::ParseError, "expected a method body");
            return;
        };
        if self.previous().end != member.span.end {
            self.error_at(
                member.span.clone(),
                DiagnosticCode::ParseError,
                "method body did not match its source boundary",
            );
            return;
        }
        member.method = Some(ClassMethod {
            name,
            is_static,
            parameters,
            return_type,
            return_type_span,
            body,
            span: member.span.clone(),
        });
    }
}

fn class_method_groups(module: &str, members: &[ClassMemberShell]) -> Vec<ClassMethodGroup> {
    let mut groups: Vec<ClassMethodGroup> = Vec::new();
    let mut pending_signature_group: Option<usize> = None;
    for (member_index, member) in members.iter().enumerate() {
        let Some(method) = &member.method else {
            pending_signature_group = None;
            continue;
        };
        let group_index = pending_signature_group
            .filter(|&index| {
                groups[index].name == method.name && groups[index].is_static == method.is_static
            })
            .unwrap_or_else(|| {
                groups.push(ClassMethodGroup {
                    name: method.name.clone(),
                    is_static: method.is_static,
                    signature_member_indices: Vec::new(),
                    implementation_member_index: None,
                    span: member.span.clone(),
                });
                groups.len() - 1
            });
        let group = &mut groups[group_index];
        group.span = SourceSpan::new(module, group.span.start, member.span.end);
        if method.body.is_none() {
            group.signature_member_indices.push(member_index);
            pending_signature_group = Some(group_index);
        } else {
            group.implementation_member_index = Some(member_index);
            pending_signature_group = None;
        }
    }
    groups
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
        let static_modifier = tokens[index].is("static")
            && tokens.get(index + 1).is_some_and(|token| {
                matches!(token.kind, TokenKind::Identifier | TokenKind::Keyword)
            })
            && tokens.get(index + 2).is_some_and(|token| token.is("("));
        let name_index = index + usize::from(static_modifier);
        let name_token = &tokens[name_index];
        let method_head = matches!(name_token.kind, TokenKind::Identifier | TokenKind::Keyword)
            && tokens
                .get(name_index + 1)
                .is_some_and(|token| token.is("("));
        if method_head {
            if let Some(parameters_end) =
                matching_closing_delimiter(tokens, name_index + 1, tokens.len(), "(", ")")
            {
                let boundary = method_body_boundary(tokens, parameters_end + 1);
                let member_end = match boundary
                    .and_then(|index| tokens.get(index).map(|token| (index, token)))
                {
                    Some((index, token)) if token.is("{") => {
                        matching_closing_delimiter(tokens, index, tokens.len(), "{", "}")
                            .map(|end| end + 1)
                    }
                    Some((index, token)) if token.is(";") => Some(index + 1),
                    _ => None,
                };
                if let Some(end) = member_end {
                    members.push(class_member_shell(
                        module,
                        tokens,
                        start,
                        end,
                        if !static_modifier && name_token.is("constructor") {
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

/// Finds the class element's body or signature semicolon without treating a
/// record return type as the method body. The type parser validates the
/// retained tokens later; this scan only preserves the source boundary.
fn method_body_boundary(tokens: &[Token], after_parameters: usize) -> Option<usize> {
    if !tokens.get(after_parameters)?.is(":") {
        return Some(after_parameters);
    }
    let type_start = after_parameters + 1;
    let mut index = type_start;
    while index < tokens.len() {
        let token = &tokens[index];
        if token.is(";") {
            return Some(index);
        }
        if token.is("{") {
            let record_type = index == type_start
                || tokens.get(index - 1).is_some_and(|previous| {
                    matches!(previous.text.as_str(), "|" | "&" | "<" | "," | "(" | "=>")
                });
            if !record_type {
                return Some(index);
            }
            index = matching_closing_delimiter(tokens, index, tokens.len(), "{", "}")? + 1;
            continue;
        }
        if token.is("(") || token.is("[") {
            let closing = if token.is("(") { ")" } else { "]" };
            index =
                matching_closing_delimiter(tokens, index, tokens.len(), &token.text, closing)? + 1;
            continue;
        }
        index += 1;
    }
    None
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
        method: None,
    }
}
