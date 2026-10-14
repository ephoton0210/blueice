// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Implements clauses compare the complete instance surface structurally.

use super::*;
use crate::diagnostic::type_text::{detail, render_in};

fn object_fields(
    value: &Type,
    types: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> Option<Vec<TypeField>> {
    if !budget.consume() {
        return None;
    }
    match value {
        Type::Record(fields) | Type::CallableRecord { fields, .. } => Some(fields.clone()),
        Type::IndexedRecord { object, .. } => object_fields(object, types, visited, budget),
        Type::Intersection(parts) => {
            let mut fields = Vec::new();
            for part in parts {
                fields.extend(object_fields(part, types, &mut visited.clone(), budget)?);
            }
            Some(fields)
        }
        Type::Named { .. } => {
            let expanded = instantiate_named(value, types, visited, budget, "implemented object")?;
            object_fields(&expanded, types, visited, budget)
        }
        _ => None,
    }
}

impl ModuleChecker<'_> {
    pub(in crate::checker::module::binding) fn validate_class_implements(
        &mut self,
        class: &ClassDeclaration,
    ) {
        let own = self
            .types
            .get(&class.name)
            .map(|definition| definition.value.clone())
            .unwrap_or(Type::Unknown);
        let (Some(own_fields), _) = self.expanded_record_fields(own.clone()) else {
            return;
        };
        for (target, span) in &class.implements {
            if matches!(
                target,
                Type::Any
                    | Type::Unknown
                    | Type::Never
                    | Type::Void
                    | Type::Undefined
                    | Type::Null
                    | Type::Number
                    | Type::String
                    | Type::Boolean
                    | Type::BigInt
                    | Type::Symbol
            ) {
                self.typescript_type_error(
                    span,
                    "class cannot implement a primitive type".into(),
                    DiagnosticCode::TypeMismatch,
                    2864,
                    vec![render_in(target, self.project)],
                );
                continue;
            }
            let before = self.diagnostics.len();
            self.check_type_with_parameters(target, span, &class.type_parameters);
            if self.diagnostics.len() != before {
                continue;
            }
            let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
            let Some(expected_fields) =
                object_fields(target, &self.types, &mut HashSet::new(), &mut budget)
            else {
                if budget.exhausted {
                    self.type_error(
                        span,
                        "implemented object exceeds its expansion limit".into(),
                        DiagnosticCode::ResourceLimit,
                    );
                } else {
                    self.typescript_type_error(
                        span,
                        "class implementation requires statically known object members".into(),
                        DiagnosticCode::TypeMismatch,
                        2422,
                        Vec::new(),
                    );
                }
                continue;
            };
            let target_text = render_in(target, self.project);
            let mut failed = false;
            for expected in &expected_fields {
                let actual = own_fields.iter().find(|field| field.name == expected.name);
                let visibility = visibility::effective_visibility(&own_fields, &expected.name);
                if actual.is_none() && (!expected.optional || visibility.is_some()) {
                    let reason = match visibility {
                        Some(Visibility::Private) => format!(
                            "\n  Property '{}' is private in type '{}' but not in type '{}'.",
                            expected.name, class.name, target_text
                        ),
                        Some(Visibility::Protected) => format!(
                            "\n  Property '{}' is protected in type '{}' but public in type '{}'.",
                            expected.name, class.name, target_text
                        ),
                        _ => format!(
                            "\n  Property '{}' is missing in type '{}' but required in type '{}'.",
                            expected.name, class.name, target_text
                        ),
                    };
                    self.typescript_type_error(
                        &class.name_span,
                        "class incorrectly implements its structural target".into(),
                        DiagnosticCode::TypeMismatch,
                        2420,
                        vec![class.name.clone(), target_text.clone()],
                    );
                    let counterpart = self
                        .diagnostics
                        .last_mut()
                        .unwrap()
                        .typescript
                        .as_mut()
                        .unwrap();
                    counterpart.message.push_str(&reason);
                    if visibility.is_none() {
                        let mut origin = expected.span.clone();
                        if !expected.method {
                            origin.end = origin.start + expected.name.len();
                        }
                        counterpart
                            .related_information
                            .push(crate::TypeScriptRelatedInformation {
                                code: 2728,
                                message: format!("'{}' is declared here.", expected.name),
                                span: origin,
                                position: None,
                            });
                    }
                    failed = true;
                    break;
                }
                let Some(actual) = actual else { continue };
                if !self.is_assignable_bounded(
                    &Type::Record(vec![actual.clone()]),
                    &Type::Record(vec![expected.clone()]),
                    &actual.span,
                ) {
                    let source = class
                        .members
                        .iter()
                        .find_map(|member| {
                            member
                                .field
                                .as_ref()
                                .filter(|field| field.name == expected.name)
                                .map(|field| field.name_span.clone())
                                .or_else(|| {
                                    member
                                        .method
                                        .as_ref()
                                        .filter(|method| method.name == expected.name)
                                        .map(|method| method.name_span.clone())
                                })
                                .or_else(|| {
                                    member
                                        .accessor
                                        .as_ref()
                                        .filter(|accessor| accessor.name == expected.name)
                                        .map(|accessor| accessor.name_span.clone())
                                })
                        })
                        .unwrap_or_else(|| actual.span.clone());
                    self.typescript_type_error(
                        &source,
                        "implemented member has an incompatible type".into(),
                        DiagnosticCode::TypeMismatch,
                        2416,
                        vec![
                            expected.name.clone(),
                            class.name.clone(),
                            target_text.clone(),
                        ],
                    );
                    let suffix = detail(&actual.value, &expected.value, self.project)
                        .lines()
                        .skip(1)
                        .map(|line| format!("\n  {line}"))
                        .collect::<String>();
                    let reason = format!(
                        "\n  Type '{}' is not assignable to type '{}'.{suffix}",
                        render_in(&actual.value, self.project),
                        render_in(&expected.value, self.project)
                    );
                    self.diagnostics
                        .last_mut()
                        .unwrap()
                        .typescript
                        .as_mut()
                        .unwrap()
                        .message
                        .push_str(&reason);
                    failed = true;
                }
            }
            if !failed && !self.is_assignable_bounded(&own, target, span) {
                self.typescript_type_error(
                    &class.name_span,
                    "class incorrectly implements its structural target".into(),
                    DiagnosticCode::TypeMismatch,
                    2420,
                    vec![class.name.clone(), target_text],
                );
                let actual = Type::Named {
                    name: class.name.clone(),
                    arguments: class
                        .type_parameters
                        .iter()
                        .map(|parameter| Type::Named {
                            name: parameter.name.clone(),
                            arguments: Vec::new(),
                        })
                        .collect(),
                };
                let reason = detail(&actual, target, self.project);
                self.diagnostics
                    .last_mut()
                    .unwrap()
                    .typescript
                    .as_mut()
                    .unwrap()
                    .message
                    .push_str(&reason);
            }
        }
    }
}
