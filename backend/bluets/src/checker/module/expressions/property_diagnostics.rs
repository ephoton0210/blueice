// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Missing properties, including the checked constructor's static side.

use super::*;

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn missing_property_error(
        &mut self,
        span: &SourceSpan,
        receiver: &Type,
        member: &str,
        receiver_name: Option<&str>,
    ) {
        let message = format!(
            "property `{member}` does not exist on type `{}`",
            type_label(receiver)
        );
        if let Type::Named { name, .. } = receiver {
            let class_name = name
                .strip_prefix("super ")
                .and_then(|owner| {
                    let owner = owner.split('@').next()?;
                    self.module.declarations.iter().find_map(|declaration| {
                        let Declaration::Class(class) = declaration else {
                            return None;
                        };
                        (class.name == owner)
                            .then_some(class.extends_name.as_deref())
                            .flatten()
                    })
                })
                .unwrap_or(name);
            if !member.starts_with('#')
                && self
                    .types
                    .get(class_name)
                    .is_some_and(|definition| definition.kind == TypeDefinitionKind::Class)
            {
                let constructor = self.values.get(class_name).cloned().or_else(|| {
                    self.module.declarations.iter().find_map(|declaration| {
                        let Declaration::Import(import) = declaration else {
                            return None;
                        };
                        let binding = import
                            .bindings
                            .iter()
                            .find(|binding| binding.local == class_name)?;
                        let resolved = self
                            .project
                            .resolutions
                            .get(&(self.module.id.clone(), import.specifier.clone()))?;
                        self.exports
                            .classes
                            .get(resolved)?
                            .get(&binding.imported)
                            .map(|class| class.constructor_type.clone())
                    })
                });
                if let Some(constructor) = constructor {
                    let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
                    if matches!(
                        property_type(
                            &constructor,
                            member,
                            &self.types,
                            &mut HashSet::new(),
                            &mut budget
                        ),
                        PropertyType::Found { .. }
                    ) {
                        self.typescript_type_error(
                            span,
                            message,
                            DiagnosticCode::TypeMismatch,
                            2576,
                            vec![
                                member.into(),
                                class_name.into(),
                                format!("{class_name}.{member}"),
                            ],
                        );
                        return;
                    }
                }
            }
        }
        // These standard-library additions have a declared minimum lib year.
        // This changes metadata only; missing members remain rejected.
        let year = if (member == "at" && matches!(receiver, Type::Array(_) | Type::String))
            || (member == "hasOwn"
                && receiver_name == Some("Object")
                && self.library_values.contains("Object"))
        {
            Some("es2022")
        } else {
            None
        };
        if let Some(year) = year {
            self.typescript_type_error(
                span,
                message,
                DiagnosticCode::TypeMismatch,
                2550,
                vec![
                    member.into(),
                    if receiver_name == Some("Object") {
                        "ObjectConstructor".into()
                    } else {
                        type_label(receiver)
                    },
                    year.into(),
                ],
            );
            return;
        }
        let mut value = receiver.clone();
        let mut visited = HashSet::new();
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        while matches!(value, Type::Named { .. }) {
            let Some(expanded) = instantiate_named(
                &value,
                &self.types,
                &mut visited,
                &mut budget,
                "diagnostic receiver",
            ) else {
                break;
            };
            value = expanded;
        }
        if let Type::Literal(name) = &value {
            if let Some(definition) = self
                .types
                .get(name)
                .filter(|definition| definition.kind == TypeDefinitionKind::EnumMember)
            {
                value = definition.value.clone();
            }
        }
        let primitive = match &value {
            Type::Literal(text) if text.starts_with(['\'', '"']) => Some(("String", "string")),
            Type::String => Some(("String", "string")),
            Type::Number => Some(("Number", "number")),
            Type::Boolean => Some(("Boolean", "boolean")),
            Type::Array(_) => Some(("Array", "array")),
            _ => None,
        };
        if let Some((library, label)) = primitive {
            if let Some(members) = crate::diagnostic::templates::members(library) {
                if let Some(suggestion) = crate::diagnostic::spelling::suggestion(
                    member,
                    members.keys().map(String::as_str),
                ) {
                    self.typescript_type_error(
                        span,
                        message,
                        DiagnosticCode::TypeMismatch,
                        2551,
                        vec![
                            member.into(),
                            if label == "array" {
                                type_label(receiver)
                            } else {
                                label.into()
                            },
                            suggestion.into(),
                        ],
                    );
                    return;
                }
            }
        }
        self.typescript_type_error(
            span,
            message,
            DiagnosticCode::TypeMismatch,
            2339,
            vec![member.into(), type_label(receiver)],
        );
    }
}
