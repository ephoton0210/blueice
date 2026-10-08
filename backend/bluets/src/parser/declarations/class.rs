// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! First bounded class-declaration shell and source provenance.

use super::*;

#[path = "class/modifiers.rs"]
mod modifiers;
use modifiers::*;
#[path = "class/keys.rs"]
mod keys;
use keys::*;
#[path = "class/indices.rs"]
mod indices;

impl Parser {
    pub(in crate::parser::implementation) fn parse_class(
        &mut self,
        start: usize,
        exported: bool,
        abstract_modifier: Option<SourceSpan>,
    ) {
        if self.current().kind != TokenKind::Identifier {
            self.error_here(DiagnosticCode::ParseError, "expected a class name");
            self.index = self.tokens.len() - 1;
            return;
        }
        let name_token = self.current().clone();
        self.bump();
        let type_parameter_start = self.current().start;
        let type_parameters = self.parse_type_parameters();
        if !type_parameters.is_empty() {
            self.edits.push(TextEdit {
                start: type_parameter_start,
                end: self.current().start,
                replacement: String::new(),
            });
        }

        let heritage = if self.consume("extends") {
            if self.current().kind != TokenKind::Identifier {
                self.error_here(DiagnosticCode::ParseError, "expected a class heritage name");
                self.index = self.tokens.len() - 1;
                return;
            }
            let mut token = self.current().clone();
            self.bump();
            // A qualified name, `N.Base`, is one token, as the second parse pass
            // merges it for a namespace member.
            while self.peek(".")
                && self
                    .tokens
                    .get(self.index + 1)
                    .is_some_and(|member| member.kind == TokenKind::Identifier)
            {
                self.bump();
                let member = self.current().clone();
                self.bump();
                token.text = format!("{}.{}", token.text, member.text);
                token.end = member.end;
            }
            Some(token)
        } else {
            None
        };

        let mut extends_arguments = Vec::new();
        let argument_start = self.current().start;
        if heritage.is_some() && self.consume("<") {
            loop {
                extends_arguments.push(self.parse_type_until(&[",", ">"]));
                if !self.consume(",") {
                    self.expect(">");
                    break;
                }
            }
            self.edits.push(TextEdit {
                start: argument_start,
                end: self.previous().end,
                replacement: String::new(),
            });
        }
        let extends_type_span = heritage.as_ref().map(|token| {
            SourceSpan::new(&self.id, token.start, self.previous().end.max(token.end))
        });
        let mut implements = Vec::new();
        if self.consume("implements") {
            let begin = self.previous().start;
            loop {
                let type_start = self.current().start;
                let value = self.parse_type_until(&[",", "{"]);
                implements.push((
                    value,
                    SourceSpan::new(&self.id, type_start, self.previous().end),
                ));
                if !self.consume(",") {
                    break;
                }
            }
            self.edits.push(TextEdit {
                start: begin,
                end: self.current().start,
                replacement: String::new(),
            });
        }
        if !self.peek("{") {
            let token = self.current().clone();
            if matches!(token.text.as_str(), "<" | "." | "[" | "(") {
                self.unsupported(
                    token.span(&self.id),
                    "a computed class heritage expression is not supported yet",
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
                self.parse_class_constructor(
                    opening + 1 + member.token_start,
                    member,
                    heritage.is_some(),
                );
            } else if member.kind == ClassMemberKind::Method {
                self.parse_class_method(opening + 1 + member.token_start, member);
            } else if member.kind == ClassMemberKind::Accessor {
                self.parse_class_accessor(opening + 1 + member.token_start, member);
            } else if member.kind == ClassMemberKind::StaticBlock {
                self.parse_class_static_block(opening + 1 + member.token_start, member);
            } else if member.kind == ClassMemberKind::Opaque
                && self.parse_class_index(opening + 1 + member.token_start, member)
            {
                member.kind = ClassMemberKind::IndexSignature;
            } else if member.kind == ClassMemberKind::Opaque
                && !body
                    .get(member.token_start + 1)
                    .is_some_and(|token| token.is("("))
                && self.parse_class_field(
                    opening + 1 + member.token_start,
                    opening + 1 + member.token_end,
                    member,
                )
            {
                member.kind = ClassMemberKind::Field;
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
        for member in &members {
            self.validate_member_modifiers(
                opening + 1 + member.token_start,
                member,
                abstract_modifier.is_some(),
            );
        }
        // Decorator expressions are ordinary expressions: erase their annotations.
        let decorator_ranges: Vec<(usize, usize)> = members
            .iter()
            .flat_map(|member| &member.decorators)
            .map(|decorator| {
                (
                    opening + 1 + decorator.token_start,
                    opening + 1 + decorator.token_end,
                )
            })
            .collect();
        for (from, to) in decorator_ranges {
            self.collect_expression_type_edits(from, to);
        }
        let method_groups = class_method_groups(&self.id, &members);
        let span = SourceSpan::new(&self.id, start, self.tokens[closing].end);
        let name_span = name_token.span(&self.id);
        self.index = closing + 1;
        let decorators = std::mem::take(&mut self.pending_decorators);
        self.declarations.push(Declaration::Class(ClassDeclaration {
            abstract_modifier,
            implements,
            decorators,
            type_parameters,
            captured_type_parameters: Box::default(),
            name: name_token.text,
            name_span,
            extends_name: heritage.as_ref().map(|token| token.text.clone()),
            extends_span: heritage.as_ref().map(|token| token.span(&self.id)),
            extends_arguments,
            extends_type_span,
            body,
            members,
            method_groups,
            merged_interface_fields: Vec::new(),
            body_span,
            exported,
            span,
        }));
    }

    /// A `#name` member is private by construction: an accessibility keyword
    /// on it is an error, and `#constructor` is not a valid name.
    fn private_name_visibility(&mut self, name: &Token, modifiers: &MemberModifiers) -> Visibility {
        if !name.text.starts_with('#') {
            return modifiers.visibility;
        }
        if let Some(keyword) = modifiers.visibility_token {
            self.error_at(
                self.tokens[keyword].span(&self.id),
                DiagnosticCode::ParseError,
                "an accessibility modifier cannot be used with a private identifier",
            );
        }
        if name.text == "#constructor" {
            self.error_at(
                name.span(&self.id),
                DiagnosticCode::ParseError,
                "`#constructor` is not a valid private name",
            );
        }
        Visibility::Private
    }

    /// Erases an explicit `public`/`protected`/`private` keyword and the gap
    /// after it; accessibility is checked, never emitted.
    fn erase_visibility_keyword(&mut self, modifiers: &MemberModifiers) {
        if let Some(token) = modifiers.visibility_token {
            self.edits.push(TextEdit {
                start: self.tokens[token].start,
                end: self.tokens[token + 1].start,
                replacement: String::new(),
            });
        }
    }

    fn parse_class_constructor(
        &mut self,
        start: usize,
        member: &mut ClassMemberShell,
        derived: bool,
    ) {
        let end = start + (member.token_end - member.token_start);
        let Some(modifiers) = scan_member_modifiers(&self.tokens, start, end) else {
            return;
        };
        self.erase_visibility_keyword(&modifiers);
        self.index = modifiers.name_index + 1;
        self.parameter_property_mode = true;
        self.parameter_properties.clear();
        let parameters = self.parse_parameters();
        self.parameter_property_mode = false;
        let parameter_properties = std::mem::take(&mut self.parameter_properties);
        let mut body_open = 0;
        let body = if self.consume("{") {
            let body_start = self.previous().start;
            body_open = body_start;
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
        if !parameter_properties.is_empty() && body.is_none() {
            self.error_at(
                member.span.clone(),
                DiagnosticCode::ParseError,
                "a parameter property is only allowed in a constructor implementation",
            );
        }
        let prologue_insertion = body
            .as_deref()
            .and_then(|items| prologue_insertion(items, body_open, derived));
        member.constructor = Some(ClassConstructor {
            visibility: modifiers.visibility,
            parameters,
            parameter_properties,
            prologue_insertion,
            body,
            span: member.span.clone(),
        });
    }

    /// Parses `[public] [static] [readonly] name[?|!][: T] [= init][;]` between
    /// token indices `start..end`, erasing the modifiers other than `static`
    /// and the optional marker and annotation. Any other shape (other
    /// modifiers, accessors, computed or private names) is left opaque, and
    /// records no edit, so it is refused as unsupported syntax rather than
    /// emitted half-erased.
    fn parse_class_field(
        &mut self,
        start: usize,
        end: usize,
        member: &mut ClassMemberShell,
    ) -> bool {
        let Some(modifiers) = scan_member_modifiers(&self.tokens, start, end) else {
            return false;
        };
        let is_static = modifiers.is_static;
        let readonly = modifiers.readonly_token.is_some();
        let mut erased_modifiers = Vec::new();
        for token in [modifiers.visibility_token, modifiers.readonly_token]
            .into_iter()
            .flatten()
        {
            erased_modifiers.push((self.tokens[token].start, self.tokens[token + 1].start));
        }
        let mut index = modifiers.name_index;
        let name_token = self.tokens[index].clone();
        let Some(key_end) = class_key_end(&self.tokens, index, end) else {
            return false;
        };
        if name_token.is("constructor") {
            return false;
        }
        let (name, name_span) = self.class_member_key(index, key_end, member);
        let visibility = self.private_name_visibility(&name_token, &modifiers);
        index = key_end;
        let optional = self.tokens.get(index).is_some_and(|token| token.is("?")) && index < end;
        let definite = self.tokens.get(index).is_some_and(|token| token.is("!")) && index < end;
        let marker_end = (optional || definite).then(|| self.tokens[index].end);
        if optional || definite {
            index += 1;
        }
        let mut annotation = None;
        let mut annotation_span = None;
        let mut erased_end = marker_end;
        if index < end && self.tokens[index].is(":") {
            self.index = index + 1;
            let type_start = self.current().start;
            let value = self.parse_type_until(&["=", ";"]);
            let type_end = self.previous().end;
            annotation = Some(value);
            annotation_span = Some(SourceSpan::new(&self.id, type_start, type_end));
            erased_end = Some(type_end);
            index = self.index;
        }
        let mut initializer_range = None;
        if index < end && self.tokens[index].is("=") {
            let initializer_start = index + 1;
            let initializer_end = if self.tokens[end - 1].is(";") {
                end - 1
            } else {
                end
            };
            if initializer_start >= initializer_end {
                self.error_at(
                    member.span.clone(),
                    DiagnosticCode::ParseError,
                    "expected a class field initializer",
                );
                return false;
            }
            initializer_range = Some((initializer_start, initializer_end));
            index = initializer_end;
        }
        if index < end && self.tokens[index].is(";") {
            index += 1;
        }
        if index != end {
            return false;
        }
        for (modifier_start, modifier_end) in erased_modifiers {
            self.edits.push(TextEdit {
                start: modifier_start,
                end: modifier_end,
                replacement: String::new(),
            });
        }
        if let Some(erased_end) = erased_end {
            self.edits.push(TextEdit {
                start: self.tokens[key_end - 1].end,
                end: erased_end,
                replacement: String::new(),
            });
        }
        let initializer = initializer_range.map(|(initializer_start, initializer_end)| {
            self.collect_expression_type_edits(initializer_start, initializer_end);
            self.tokens[initializer_start..initializer_end].to_vec()
        });
        if modifiers.declare_token.is_some() {
            if let Some(tokens) = &initializer {
                if let Some(token) = tokens.first() {
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::ParseError,
                            token.span(&self.id),
                            "a declare field cannot have an initializer",
                        )
                        .with_typescript(1039, Vec::new()),
                    );
                }
            }
        }
        member.name = Some(name.clone());
        member.field = Some(ClassField {
            declared: modifiers.declare_token.is_some(),
            accessor: modifiers.accessor_token.is_some(),
            name,
            name_span,
            visibility,
            is_static,
            readonly,
            optional,
            definite,
            annotation,
            annotation_span,
            initializer,
            from_default: false,
            span: member.span.clone(),
        });
        true
    }

    /// `static { .. }`: the statements run once, when the class is defined.
    fn parse_class_static_block(&mut self, start: usize, member: &mut ClassMemberShell) {
        // `start` is the `static` token; the block opens right after it.
        self.index = start + 1;
        if !self.consume("{") {
            return;
        }
        let body_start = self.previous().start;
        let mut body = Vec::new();
        let mut returns = Vec::new();
        let mut locals = Vec::new();
        self.parse_function_body(body_start, &mut body, &mut returns, &mut locals);
        if self.previous().end != member.span.end {
            self.error_at(
                member.span.clone(),
                DiagnosticCode::ParseError,
                "static block did not match its source boundary",
            );
            return;
        }
        member.static_block = Some(ClassStaticBlock {
            body,
            span: member.span.clone(),
        });
    }

    /// `[accessibility] [static] get|set name(..) [: T] { .. }`. The result
    /// annotation and the value parameter's annotation are erased; the checker
    /// requires a getter's annotation and a setter's parameter annotation.
    fn parse_class_accessor(&mut self, start: usize, member: &mut ClassMemberShell) {
        let end = start + (member.token_end - member.token_start);
        let Some(modifiers) = scan_member_modifiers(&self.tokens, start, end) else {
            return;
        };
        self.erase_visibility_keyword(&modifiers);
        let keyword = modifiers.name_index;
        let getter = self.tokens[keyword].is("get");
        let name_token = self.tokens[keyword + 1].clone();
        let Some(key_end) = class_key_end(&self.tokens, keyword + 1, end) else {
            return;
        };
        let (name, name_span) = self.class_member_key(keyword + 1, key_end, member);
        let visibility = self.private_name_visibility(&name_token, &modifiers);
        self.index = key_end;
        let parameters = self.parse_parameters();
        let return_start = self.current().start;
        let (return_type, return_type_span) = if self.consume(":") {
            let type_start = self.current().start;
            let value = self.parse_return_type_until(&["{", ";"]);
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
        // The accessor grammar: a getter has no parameters; a setter has one
        // plain parameter, no default, and no result annotation.
        let arity_error = if getter {
            (!parameters.is_empty()).then_some("a getter cannot have parameters")
        } else if return_type.is_some() {
            Some("a setter cannot have a return type annotation")
        } else if parameters.len() != 1 {
            Some("a setter must have exactly one parameter")
        } else if parameters[0].rest || parameters[0].optional || parameters[0].pattern.is_some() {
            Some("a setter parameter cannot be a rest, optional, defaulted or destructured parameter")
        } else {
            None
        };
        if let Some(message) = arity_error {
            let mut diagnostic =
                Diagnostic::error(DiagnosticCode::ParseError, member.span.clone(), message);
            if !getter && parameters.len() != 1 {
                diagnostic = diagnostic.with_typescript(1049, Vec::new());
            }
            if !getter && parameters.len() == 1 {
                let parameter = &parameters[0];
                let code = if parameter.rest {
                    Some(1053)
                } else if parameter.default.is_some() {
                    Some(1052)
                } else if parameter.optional {
                    Some(1051)
                } else if return_type.is_some() {
                    Some(1095)
                } else {
                    None
                };
                diagnostic = if let Some(code) = code {
                    diagnostic.with_typescript(code, Vec::new())
                } else {
                    diagnostic.blue_only("BlueTSC's class setter subset does not accept destructured parameters; TypeScript permits them.")
                };
            }
            self.diagnostics.push(diagnostic);
        }
        let mut body = Vec::new();
        let body_present = if self.consume("{") {
            let body_start = self.previous().start;
            let mut returns = Vec::new();
            let mut locals = Vec::new();
            self.parse_function_body(body_start, &mut body, &mut returns, &mut locals);
            true
        } else if member.abstract_modifier.is_some() && self.consume(";") {
            false
        } else {
            self.error_here(DiagnosticCode::ParseError, "expected an accessor body");
            return;
        };
        if self.previous().end != member.span.end {
            self.error_at(
                member.span.clone(),
                DiagnosticCode::ParseError,
                "accessor body did not match its source boundary",
            );
            return;
        }
        member.name = Some(name.clone());
        member.accessor = Some(ClassAccessor {
            name,
            name_span,
            visibility,
            is_static: modifiers.is_static,
            getter,
            parameters,
            return_type,
            return_type_span,
            body,
            body_present,
            span: member.span.clone(),
        });
    }

    fn parse_class_method(&mut self, start: usize, member: &mut ClassMemberShell) {
        let end = start + (member.token_end - member.token_start);
        let Some(modifiers) = scan_member_modifiers(&self.tokens, start, end) else {
            return;
        };
        self.erase_visibility_keyword(&modifiers);
        let is_static = modifiers.is_static;
        let name_index = modifiers.name_index;
        let Some(key_end) = class_key_end(&self.tokens, name_index, end) else {
            return;
        };
        let (name, name_span) = self.class_member_key(name_index, key_end, member);
        member.name = Some(name.clone());
        let visibility = self.private_name_visibility(&self.tokens[name_index].clone(), &modifiers);
        self.index = key_end;
        let optional = self.consume("?");
        if optional {
            self.edits.push(TextEdit {
                start: self.previous().start,
                end: self.current().start,
                replacement: String::new(),
            });
        }
        let generic_start = self.current().start;
        let type_parameters = self.parse_type_parameters();
        if !type_parameters.is_empty() {
            self.edits.push(TextEdit {
                start: generic_start,
                end: self.current().start,
                replacement: String::new(),
            });
        }
        let parameters = self.parse_parameters();
        let return_start = self.current().start;
        let (return_type, return_type_span) = if self.consume(":") {
            let type_start = self.current().start;
            let value = self.parse_return_type_until(&["{", ";"]);
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
            name_span,
            optional,
            visibility,
            is_static,
            type_parameters,
            parameters,
            return_type,
            return_type_span,
            body,
            span: member.span.clone(),
        });
    }
}

/// The decorator that starts at `tokens[index]` (an `@`) and the index after it:
/// `@name`, `@a.b.c`, `@name(args)`, `@a.b(args)` or `@(expression)`.
pub(super) fn decorator_at(
    module: &str,
    tokens: &[Token],
    index: usize,
) -> Option<(Decorator, usize)> {
    let at = tokens.get(index).filter(|token| token.is("@"))?;
    let mut next = index + 1;
    let first = tokens.get(next)?;
    if first.is("(") {
        next = matching_closing_delimiter(tokens, next, tokens.len(), "(", ")")? + 1;
    } else {
        if !matches!(first.kind, TokenKind::Identifier | TokenKind::Keyword) {
            return None;
        }
        next += 1;
        while tokens.get(next).is_some_and(|token| token.is("."))
            && tokens.get(next + 1).is_some_and(|token| {
                matches!(token.kind, TokenKind::Identifier | TokenKind::Keyword)
            })
        {
            next += 2;
        }
        if tokens.get(next).is_some_and(|token| token.is("(")) {
            next = matching_closing_delimiter(tokens, next, tokens.len(), "(", ")")? + 1;
        }
    }
    Some((
        Decorator {
            tokens: tokens[index + 1..next].to_vec(),
            span: SourceSpan::new(module, at.start, tokens[next - 1].end),
            token_start: index + 1,
            token_end: next,
        },
        next,
    ))
}

/// Where a constructor's parameter-property assignments go: the start of the
/// body in a base class, just after the top-level `super(...)` statement in a
/// derived one. A derived constructor with no such statement has no place.
fn prologue_insertion(
    items: &[FunctionBodyItem],
    body_open: usize,
    derived: bool,
) -> Option<ParameterPropertyInsertion> {
    if !derived {
        return Some(ParameterPropertyInsertion {
            item_index: 0,
            offset: body_open + 1,
        });
    }
    items.iter().enumerate().find_map(|(index, item)| {
        let FunctionBodyItem::Expression { tokens, span } = item else {
            return None;
        };
        (tokens.first().is_some_and(|token| token.is("super"))
            && tokens.get(1).is_some_and(|token| token.is("(")))
        .then_some(ParameterPropertyInsertion {
            item_index: index + 1,
            offset: span.end,
        })
    })
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
        let mut decorators = Vec::new();
        while let Some((decorator, next)) = decorator_at(module, tokens, index) {
            decorators.push(decorator);
            index = next;
        }
        if index >= tokens.len() {
            break;
        }
        let start = index;
        // `static { .. }` is an initialization block, not a member with a name.
        if tokens[index].is("static") && tokens.get(index + 1).is_some_and(|token| token.is("{")) {
            if let Some(closing) =
                matching_closing_delimiter(tokens, index + 1, tokens.len(), "{", "}")
            {
                members.push(class_member_shell(
                    module,
                    tokens,
                    start,
                    closing + 1,
                    ClassMemberKind::StaticBlock,
                    None,
                    std::mem::take(&mut decorators),
                ));
                index = closing + 1;
                continue;
            }
        }
        let modifiers = scan_member_modifiers(tokens, index, tokens.len());
        let static_modifier = modifiers
            .as_ref()
            .is_some_and(|modifiers| modifiers.is_static);
        let name_index = modifiers
            .as_ref()
            .map_or(index, |modifiers| modifiers.name_index);
        let name_token = &tokens[name_index];
        // `get name(` / `set name(` is an accessor; a member named `get`
        // followed by `(` is an ordinary method.
        let accessor_head = modifiers
            .as_ref()
            .is_some_and(|modifiers| modifiers.readonly_token.is_none())
            && (name_token.is("get") || name_token.is("set"))
            && class_key_end(tokens, name_index + 1, tokens.len())
                .is_some_and(|end| tokens.get(end).is_some_and(|token| token.is("(")));
        let head_index = name_index + usize::from(accessor_head);
        let name_token = &tokens[head_index];
        let key_end = class_key_end(tokens, head_index, tokens.len()).unwrap_or(head_index + 1);
        let mut parameters_index =
            key_end + usize::from(tokens.get(key_end).is_some_and(|token| token.is("?")));
        if tokens
            .get(parameters_index)
            .is_some_and(|token| token.is("<"))
        {
            if let Some(closing) =
                matching_closing_delimiter(tokens, parameters_index, tokens.len(), "<", ">")
            {
                parameters_index = closing + 1;
            }
        }
        let method_head = accessor_head
            || (modifiers
                .as_ref()
                .is_some_and(|modifiers| modifiers.readonly_token.is_none())
                && class_key_end(tokens, head_index, tokens.len()).is_some()
                && tokens
                    .get(parameters_index)
                    .is_some_and(|token| token.is("(")));
        if method_head {
            if let Some(parameters_end) =
                matching_closing_delimiter(tokens, parameters_index, tokens.len(), "(", ")")
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
                        if accessor_head {
                            ClassMemberKind::Accessor
                        } else if !static_modifier && name_token.is("constructor") {
                            ClassMemberKind::Constructor
                        } else {
                            ClassMemberKind::Method
                        },
                        Some(name_token.text.clone()),
                        std::mem::take(&mut decorators),
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
            std::mem::take(&mut decorators),
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
    decorators: Vec<Decorator>,
) -> ClassMemberShell {
    let modifiers = scan_member_modifiers(tokens, start, end);
    ClassMemberShell {
        abstract_modifier: modifiers
            .as_ref()
            .and_then(|m| m.abstract_token)
            .map(|i| tokens[i].span(module)),
        override_modifier: modifiers
            .as_ref()
            .and_then(|m| m.override_token)
            .map(|i| tokens[i].span(module)),
        decorators,
        kind,
        name,
        key: Vec::new(),
        token_start: start,
        token_end: end,
        span: SourceSpan::new(module, tokens[start].start, tokens[end - 1].end),
        constructor: None,
        method: None,
        field: None,
        accessor: None,
        static_block: None,
        index: None,
    }
}
