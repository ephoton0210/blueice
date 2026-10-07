// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Enum binding and checking.
//!
//! An enum `E` binds four things: the type `E` (the union of its member
//! types, or plain `number`/`string | number` when a member is computed), one
//! type `E.A` per constant member (the enum literal type, with its value
//! recorded as a literal so assignability can tell `E.A` from the number `0`),
//! the object type `typeof E`, and the value `E`. Declarations of the same name
//! merge.

use super::*;
use crate::enum_eval::{js_number_text, EnumValue, EvaluatedEnum, EvaluatedMember};
use crate::parser::EnumDeclaration;

/// A member's constant as the literal type that stands for it.
fn value_literal(value: &EnumValue) -> Type {
    match value {
        EnumValue::Number(number) => Type::Literal(js_number_text(*number)),
        EnumValue::Text(text) => Type::Literal(format!(
            "\"{}\"",
            text.replace('\\', "\\\\").replace('"', "\\\"")
        )),
    }
}

impl ModuleChecker<'_> {
    pub(super) fn bind_enum(&mut self, declaration: &EnumDeclaration, evaluated: &EvaluatedEnum) {
        let name = declaration.name.clone();
        let existing = self.types.get(&name).map(|definition| definition.kind);
        if matches!(existing, Some(kind) if kind != TypeDefinitionKind::Enum)
            || (existing.is_none() && self.values.contains_key(&name))
        {
            self.duplicate(&name, declaration.name_span.clone());
            return;
        }
        let members = self.enum_members.entry(name.clone()).or_default();
        members.extend(evaluated.members.iter().cloned());
        let members = members.clone();
        let spans: BTreeMap<String, SourceSpan> = declaration
            .members
            .iter()
            .map(|member| (member.name.clone(), member.name_span.clone()))
            .collect();
        self.bind_enum_members(
            &name,
            &members,
            &spans,
            declaration.name_span.clone(),
            declaration.is_const,
            declaration.declared,
            true,
        );
    }

    /// Binds an enum imported from another module. `with_value` is false for a
    /// type-only import, which brings the types and not the object.
    pub(super) fn bind_imported_enum(
        &mut self,
        local: &str,
        exported: &ExportedEnum,
        span: &SourceSpan,
        with_value: bool,
    ) {
        let existing = self.types.get(local).map(|definition| definition.kind);
        if existing.is_some() || (with_value && self.values.contains_key(local)) {
            self.duplicate(local, span.clone());
            return;
        }
        self.bind_enum_members(
            local,
            &exported.members,
            &BTreeMap::new(),
            span.clone(),
            exported.is_const,
            exported.declared,
            with_value,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn bind_enum_members(
        &mut self,
        name: &str,
        members: &[EvaluatedMember],
        spans: &BTreeMap<String, SourceSpan>,
        fallback_span: SourceSpan,
        is_const: bool,
        declared: bool,
        with_value: bool,
    ) {
        let name = name.to_string();
        let mut fields: Vec<TypeField> = Vec::new();
        let mut member_types = Vec::new();
        let mut has_text = false;
        let mut has_computed = false;
        for member in members {
            let span = spans
                .get(&member.name)
                .cloned()
                .unwrap_or_else(|| fallback_span.clone());
            let value = match &member.value {
                Some(value) => {
                    has_text |= matches!(value, EnumValue::Text(_));
                    let member_type = format!("{name}.{}", member.name);
                    self.types.insert(
                        member_type.clone(),
                        TypeDefinition {
                            kind: TypeDefinitionKind::EnumMember,
                            parameters: Vec::new(),
                            value: value_literal(value),
                        },
                    );
                    member_types.push(Type::Literal(member_type.clone()));
                    Type::Literal(member_type)
                }
                None => {
                    has_computed = true;
                    // A computed member has the type of the whole enum.
                    Type::Named {
                        name: name.clone(),
                        arguments: Vec::new(),
                    }
                }
            };
            if fields.iter().any(|field| field.name == member.name) {
                continue;
            }
            fields.push(TypeField {
                name: member.name.clone(),
                readonly: true,
                optional: false,
                value,
                span,
            });
        }
        let enum_value = if has_computed {
            if has_text {
                Type::Union(vec![Type::Number, Type::String])
            } else {
                Type::Number
            }
        } else if member_types.is_empty() {
            Type::Never
        } else {
            Type::Union(member_types)
        };
        self.types.insert(
            name.clone(),
            TypeDefinition {
                kind: TypeDefinitionKind::Enum,
                parameters: Vec::new(),
                value: enum_value,
            },
        );
        let object = format!("typeof {name}");
        self.types.insert(
            object.clone(),
            TypeDefinition {
                kind: TypeDefinitionKind::EnumObject,
                parameters: Vec::new(),
                value: Type::Record(fields),
            },
        );
        if is_const {
            self.const_enums.insert(name.clone());
            if declared {
                self.ambient_const_enums.insert(name.clone());
            }
        }
        if !with_value {
            self.type_only_enums.insert(name.clone());
        }
        if with_value {
            self.values.insert(
                name,
                Type::Named {
                    name: object,
                    arguments: Vec::new(),
                },
            );
        }
    }

    /// The diagnostics of one enum declaration: its own evaluation errors, a
    /// computed member in an enum with text members, a computed initializer
    /// that is not a number, and a sibling referenced without its enum name.
    pub(super) fn check_enum(&mut self, declaration: &EnumDeclaration, evaluated: &EvaluatedEnum) {
        for (span, message, code) in &evaluated.errors {
            self.type_error(span, message.clone(), *code);
        }
        let siblings: Vec<&str> = declaration
            .members
            .iter()
            .map(|member| member.name.as_str())
            .collect();
        for (member, evaluated_member) in declaration.members.iter().zip(&evaluated.members) {
            let Some(tokens) = &member.initializer else {
                continue;
            };
            if evaluated_member.value.is_some() {
                continue;
            }
            // A bare sibling name would need rewriting to `E.A`.
            for (index, token) in tokens.iter().enumerate() {
                let after_dot = index
                    .checked_sub(1)
                    .and_then(|before| tokens.get(before))
                    .is_some_and(|before| before.is("."));
                if !after_dot && siblings.contains(&token.text.as_str()) {
                    let mut diagnostic = Diagnostic::error(
                        DiagnosticCode::UnsupportedSyntax,
                        SourceSpan::new(&member.span.module, token.start, token.end),
                        format!(
                            "a computed initializer that refers to the member `{}` must write it \
                             as `{}.{}`",
                            token.text, declaration.name, token.text
                        ),
                    );
                    if declaration
                        .members
                        .iter()
                        .position(|sibling| sibling.name == token.text)
                        > declaration
                            .members
                            .iter()
                            .position(|sibling| sibling.name == member.name)
                    {
                        diagnostic = diagnostic.with_typescript(2651, Vec::new());
                    }
                    self.diagnostics.push(diagnostic);
                }
            }
            let scope = self.values.clone();
            self.check_direct_runtime_expression(tokens, &scope, &member.span);
            let actual = self.infer_expression(tokens, &scope);
            if !self.is_assignable_bounded(&actual, &Type::Number, &member.span) {
                self.type_error(
                    &member.span,
                    format!(
                        "a computed enum member must be a number, not `{}`",
                        type_label(&actual)
                    ),
                    DiagnosticCode::TypeMismatch,
                );
            }
        }
    }

    /// A `const enum` exists only at compile time, so it can be used only as
    /// `E.A` or `E["A"]`, never as a value and never by reverse lookup; and
    /// when modules are emitted one at a time an ambient one cannot be used at
    /// all, since its values are not known to the module that uses it.
    pub(in crate::checker::module) fn check_const_enum_uses(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) {
        if self.const_enums.is_empty() {
            return;
        }
        for (index, token) in tokens.iter().enumerate() {
            if token.kind != TokenKind::Identifier
                || !self.const_enums.contains(&token.text)
                || index
                    .checked_sub(1)
                    .and_then(|before| tokens.get(before))
                    .is_some_and(|before| before.is("."))
                || scope.get(&token.text) != self.values.get(&token.text)
            {
                continue;
            }
            let token_span = SourceSpan::new(&span.module, token.start, token.end);
            if self.isolated_modules && self.ambient_const_enums.contains(&token.text) {
                self.type_error(
                    &token_span,
                    format!(
                        "the ambient const enum `{}` cannot be used when modules are isolated",
                        token.text
                    ),
                    DiagnosticCode::TypeMismatch,
                );
            }
            match tokens.get(index + 1) {
                Some(next) if next.is(".") => {}
                Some(next) if next.is("[") => {
                    let literal_key = tokens
                        .get(index + 2)
                        .is_some_and(|key| key.kind == TokenKind::String)
                        && tokens.get(index + 3).is_some_and(|close| close.is("]"));
                    if !literal_key {
                        self.type_error(
                            &token_span,
                            format!(
                                "a member of the const enum `{}` can only be accessed with a \
                                 string literal",
                                token.text
                            ),
                            DiagnosticCode::TypeMismatch,
                        );
                    }
                }
                _ => self.type_error(
                    &token_span,
                    format!(
                        "the const enum `{}` can only be used in a property or index access",
                        token.text
                    ),
                    DiagnosticCode::TypeMismatch,
                ),
            }
        }
    }

    /// An enum imported with `import type` has no runtime object.
    pub(in crate::checker::module) fn check_type_only_enum_value_uses(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) {
        if self.type_only_enums.is_empty() {
            return;
        }
        for (index, token) in tokens.iter().enumerate() {
            if token.kind != TokenKind::Identifier
                || !self.type_only_enums.contains(&token.text)
                || scope.contains_key(&token.text)
                || index
                    .checked_sub(1)
                    .and_then(|before| tokens.get(before))
                    .is_some_and(|before| before.is("."))
            {
                continue;
            }
            self.type_error(
                &SourceSpan::new(&span.module, token.start, token.end),
                format!(
                    "`{}` was imported with `import type` and cannot be used as a value",
                    token.text
                ),
                DiagnosticCode::UnknownName,
            );
        }
    }
}
