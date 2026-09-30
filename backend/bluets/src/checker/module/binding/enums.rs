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
use crate::enum_eval::{js_number_text, EnumValue, EvaluatedEnum};
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

        let mut fields = Vec::new();
        let mut member_types = Vec::new();
        let mut has_text = false;
        let mut has_computed = false;
        for member in &members {
            let span = declaration
                .members
                .iter()
                .find(|candidate| candidate.name == member.name)
                .map_or_else(
                    || declaration.name_span.clone(),
                    |candidate| candidate.name_span.clone(),
                );
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
            if fields
                .iter()
                .any(|field: &TypeField| field.name == member.name)
            {
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
        self.values.insert(
            name,
            Type::Named {
                name: object,
                arguments: Vec::new(),
            },
        );
    }

    /// The diagnostics of one enum declaration: its own evaluation errors, a
    /// computed member in an enum with text members, a computed initializer
    /// that is not a number, and a sibling referenced without its enum name.
    pub(super) fn check_enum(&mut self, declaration: &EnumDeclaration, evaluated: &EvaluatedEnum) {
        if declaration.is_const {
            self.diagnostics.push(Diagnostic::error(
                DiagnosticCode::UnsupportedSyntax,
                declaration.span.clone(),
                "`const enum` is not supported yet",
            ));
        }
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
                    self.diagnostics.push(Diagnostic::error(
                        DiagnosticCode::UnsupportedSyntax,
                        SourceSpan::new(&member.span.module, token.start, token.end),
                        format!(
                            "a computed initializer that refers to the member `{}` must write it \
                             as `{}.{}`",
                            token.text, declaration.name, token.text
                        ),
                    ));
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
}
