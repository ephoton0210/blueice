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
        let parameter_property_insertion = match (&body, parameter_properties.is_empty()) {
            (Some(items), false) => parameter_property_insertion(items, body_open, derived),
            _ => None,
        };
        member.constructor = Some(ClassConstructor {
            visibility: modifiers.visibility,
            parameters,
            parameter_properties,
            parameter_property_insertion,
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
        if !matches!(name_token.kind, TokenKind::Identifier | TokenKind::Keyword)
            || name_token.is("constructor")
            || index >= end
        {
            return false;
        }
        let visibility = self.private_name_visibility(&name_token, &modifiers);
        index += 1;
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
                start: name_token.end,
                end: erased_end,
                replacement: String::new(),
            });
        }
        let initializer = initializer_range.map(|(initializer_start, initializer_end)| {
            self.collect_expression_type_edits(initializer_start, initializer_end);
            self.tokens[initializer_start..initializer_end].to_vec()
        });
        member.name = Some(name_token.text.clone());
        member.field = Some(ClassField {
            name: name_token.text.clone(),
            name_span: name_token.span(&self.id),
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
        let visibility = self.private_name_visibility(&name_token, &modifiers);
        self.index = keyword + 2;
        let parameters = self.parse_parameters();
        let return_start = self.current().start;
        let (return_type, return_type_span) = if self.consume(":") {
            let type_start = self.current().start;
            let value = self.parse_type_until(&["{"]);
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
            self.error_at(member.span.clone(), DiagnosticCode::ParseError, message);
        }
        if !self.consume("{") {
            self.error_here(DiagnosticCode::ParseError, "expected an accessor body");
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
                "accessor body did not match its source boundary",
            );
            return;
        }
        member.name = Some(name_token.text.clone());
        member.accessor = Some(ClassAccessor {
            name: name_token.text.clone(),
            name_span: name_token.span(&self.id),
            visibility,
            is_static: modifiers.is_static,
            getter,
            parameters,
            return_type,
            return_type_span,
            body,
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
        let name = self.tokens[name_index].text.clone();
        let visibility = self.private_name_visibility(&self.tokens[name_index].clone(), &modifiers);
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
            visibility,
            is_static,
            parameters,
            return_type,
            return_type_span,
            body,
            span: member.span.clone(),
        });
    }
}

/// The modifiers before a member name, in the only order TypeScript allows:
/// `[public|protected|private] [static] [readonly]`. A modifier word counts
/// only when a name follows it, so a member named `static` or `private` is
/// still a name.
struct MemberModifiers {
    visibility: Visibility,
    /// Token index of an explicit accessibility keyword.
    visibility_token: Option<usize>,
    is_static: bool,
    readonly_token: Option<usize>,
    name_index: usize,
}

fn scan_member_modifiers(tokens: &[Token], start: usize, end: usize) -> Option<MemberModifiers> {
    let name_like = |index: usize| {
        index < end
            && tokens.get(index).is_some_and(|token| {
                matches!(token.kind, TokenKind::Identifier | TokenKind::Keyword)
            })
    };
    let mut index = start;
    let mut modifiers = MemberModifiers {
        visibility: Visibility::Public,
        visibility_token: None,
        is_static: false,
        readonly_token: None,
        name_index: start,
    };
    let visibility = match tokens.get(index).map(|token| token.text.as_str()) {
        Some("public") => Some(Visibility::Public),
        Some("protected") => Some(Visibility::Protected),
        Some("private") => Some(Visibility::Private),
        _ => None,
    };
    if let Some(visibility) = visibility.filter(|_| name_like(index + 1)) {
        modifiers.visibility = visibility;
        modifiers.visibility_token = Some(index);
        index += 1;
    }
    if tokens.get(index).is_some_and(|token| token.is("static")) && name_like(index + 1) {
        modifiers.is_static = true;
        index += 1;
    }
    if tokens.get(index).is_some_and(|token| token.is("readonly")) && name_like(index + 1) {
        modifiers.readonly_token = Some(index);
        index += 1;
    }
    // A modifier out of order (`static private x`) or repeated is not a
    // member this parser structures.
    if tokens.get(index).is_some_and(|token| {
        matches!(
            token.text.as_str(),
            "public" | "protected" | "private" | "static" | "readonly"
        )
    }) && name_like(index + 1)
    {
        return None;
    }
    modifiers.name_index = index;
    Some(modifiers)
}

/// Where a constructor's parameter-property assignments go: the start of the
/// body in a base class, just after the top-level `super(...)` statement in a
/// derived one. A derived constructor with no such statement has no place.
fn parameter_property_insertion(
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
            && tokens.get(name_index + 1).is_some_and(|token| {
                matches!(token.kind, TokenKind::Identifier | TokenKind::Keyword)
            })
            && tokens
                .get(name_index + 2)
                .is_some_and(|token| token.is("("));
        let head_index = name_index + usize::from(accessor_head);
        let name_token = &tokens[head_index];
        let method_head = accessor_head
            || (modifiers
                .as_ref()
                .is_some_and(|modifiers| modifiers.readonly_token.is_none())
                && matches!(name_token.kind, TokenKind::Identifier | TokenKind::Keyword)
                && tokens
                    .get(head_index + 1)
                    .is_some_and(|token| token.is("(")));
        if method_head {
            if let Some(parameters_end) =
                matching_closing_delimiter(tokens, head_index + 1, tokens.len(), "(", ")")
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
        field: None,
        accessor: None,
        static_block: None,
    }
}
