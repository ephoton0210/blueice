// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Computed fields require a statically named literal or unique-symbol entity.

use super::*;

fn entity_name(tokens: &[Token]) -> bool {
    !tokens.is_empty()
        && tokens.iter().enumerate().all(|(index, token)| {
            if index % 2 == 0 {
                matches!(token.kind, TokenKind::Identifier | TokenKind::Keyword)
            } else {
                token.is(".")
            }
        })
        && tokens.len() % 2 == 1
}

impl ModuleChecker<'_> {
    pub(in crate::checker::module::binding) fn validate_computed_class_this(
        &mut self,
        class: &ClassDeclaration,
    ) {
        for member in &class.members {
            if !member.key.first().is_some_and(|token| token.is("[")) {
                continue;
            }
            for token in member.key.iter().filter(|token| token.is("this")) {
                // A regular function in the key owns its own receiver. Arrows
                // retain the computed name's forbidden lexical receiver.
                if self.module.nested_functions.values().any(|function| {
                    function.kind == crate::parser::NestedFunctionKind::Function
                        && function.span.start <= token.start
                        && token.end <= function.span.end
                }) {
                    continue;
                }
                self.typescript_type_error(
                    &token.span(&self.module.id),
                    "this cannot be referenced in a computed property name".into(),
                    DiagnosticCode::TypeMismatch,
                    2465,
                    Vec::new(),
                );
            }
        }
    }

    pub(in crate::checker::module::binding) fn validate_computed_class_fields(
        &mut self,
        class: &ClassDeclaration,
    ) {
        for member in &class.members {
            let Some(field) = &member.field else { continue };
            if !field.name.starts_with("[computed@") || member.key.len() < 3 {
                continue;
            }
            let tokens = &member.key[1..member.key.len() - 1];
            let value = self.infer_expression(tokens, &self.values);
            // Name lookup produces the primary diagnostic before the field rule.
            if entity_name(tokens) && value == Type::Unknown {
                continue;
            }
            if entity_name(tokens) && matches!(value, Type::Literal(_) | Type::UniqueSymbol(_)) {
                continue;
            }
            self.typescript_type_error(
                &field.name_span,
                "a computed class field requires a literal or unique-symbol entity name".into(),
                DiagnosticCode::TypeMismatch,
                1166,
                Vec::new(),
            );
        }
    }

    pub(in crate::checker::module::binding) fn bind_computed_class_field_names(&mut self) {
        let mut names = BTreeMap::new();
        for class in self.module.classes() {
            for member in &class.members {
                let Some(field) = &member.field else { continue };
                if !field.name.starts_with("[computed@") || member.key.len() < 3 {
                    continue;
                }
                let tokens = &member.key[1..member.key.len() - 1];
                if !entity_name(tokens) {
                    continue;
                }
                let Type::Literal(text) = self.infer_expression(tokens, &self.values) else {
                    continue;
                };
                let name = if text.starts_with(['\'', '"']) {
                    crate::syntax::string_contents(&Token {
                        kind: TokenKind::String,
                        text,
                        start: 0,
                        end: 0,
                    })
                } else if text.parse::<f64>().is_ok() {
                    Some(text)
                } else {
                    None
                };
                if let Some(name) = name {
                    names.insert(field.name.clone(), name);
                }
            }
        }
        let rename = |value: &mut Type| {
            let (Type::Record(fields) | Type::CallableRecord { fields, .. }) =
                value.object_type_mut()
            else {
                return;
            };
            for field in fields {
                for (marker, name) in &names {
                    if let Some(prefix) = field.name.strip_suffix(marker) {
                        field.name = format!("{prefix}{name}");
                        break;
                    }
                }
            }
        };
        for definition in self.types.values_mut() {
            rename(&mut definition.value);
        }
        for value in self.values.values_mut() {
            rename(value);
        }
    }
}
