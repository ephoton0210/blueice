// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded class-method checks before class runtime admission.

use super::*;
use crate::parser::{ClassDeclaration, ClassMethod};

impl ModuleChecker<'_> {
    pub(super) fn bind_class(&mut self, class: &ClassDeclaration) {
        let existing_type = self
            .types
            .get(&class.name)
            .map(|definition| definition.kind);
        if self.values.contains_key(&class.name)
            || matches!(
                existing_type,
                Some(TypeDefinitionKind::Alias | TypeDefinitionKind::Class)
            )
        {
            self.duplicate(&class.name, class.name_span.clone());
            return;
        }

        // TypeScript permits an interface with the same name as a class.
        // Retain the prior interface surface until declaration merging is
        // checked later; class output is refused throughout this phase.
        if existing_type.is_none() {
            self.types.insert(
                class.name.clone(),
                TypeDefinition {
                    kind: TypeDefinitionKind::Class,
                    parameters: Vec::new(),
                    value: class_instance_type(class),
                },
            );
        }
        self.values
            .insert(class.name.clone(), class_constructor_side_type(class));
    }

    pub(super) fn validate_class_method_groups(&mut self, class: &ClassDeclaration) {
        let mut implementations: BTreeMap<&str, usize> = BTreeMap::new();
        for group in &class.method_groups {
            if group.implementation_member_index.is_some() {
                *implementations.entry(&group.name).or_default() += 1;
            }
        }

        for group in &class.method_groups {
            if implementations
                .get(group.name.as_str())
                .copied()
                .unwrap_or(0)
                > 1
            {
                for index in group
                    .signature_member_indices
                    .iter()
                    .copied()
                    .chain(group.implementation_member_index)
                {
                    self.type_error(
                        &class.members[index].span,
                        format!("duplicate implementation of class method `{}`", group.name),
                        DiagnosticCode::DuplicateDeclaration,
                    );
                }
                continue;
            }

            let Some(implementation_index) = group.implementation_member_index else {
                if let Some(&last_signature) = group.signature_member_indices.last() {
                    self.type_error(
                        &class.members[last_signature].span,
                        format!(
                            "overload signature for class method `{}` requires an immediately following implementation",
                            group.name
                        ),
                        DiagnosticCode::TypeMismatch,
                    );
                }
                continue;
            };
            let implementation = class.members[implementation_index]
                .method
                .as_ref()
                .expect("method group implementation is parsed");
            for &signature_index in &group.signature_member_indices {
                let signature = class.members[signature_index]
                    .method
                    .as_ref()
                    .expect("method group signature is parsed");
                match class_method_overload_is_compatible(
                    signature,
                    implementation,
                    &self.types,
                    self.max_type_expansions,
                ) {
                    Ok(true) => {}
                    Ok(false) => self.type_error(
                        &signature.span,
                        format!(
                            "overload signature for class method `{}` is incompatible with its implementation",
                            group.name
                        ),
                        DiagnosticCode::TypeMismatch,
                    ),
                    Err(()) => self.type_error(
                        &signature.span,
                        format!(
                            "class method overload compatibility exceeds the {} generic-expansion limit",
                            self.max_type_expansions
                        ),
                        DiagnosticCode::ResourceLimit,
                    ),
                }
            }
        }
    }
}

fn class_instance_type(class: &ClassDeclaration) -> Type {
    let mut fields = Vec::new();
    for group in &class.method_groups {
        let members = group.signature_member_indices.iter().copied().chain(
            group
                .signature_member_indices
                .is_empty()
                .then_some(group.implementation_member_index)
                .flatten(),
        );
        for index in members {
            let method = class.members[index]
                .method
                .as_ref()
                .expect("method group member is parsed");
            fields.push(TypeField {
                name: method.name.clone(),
                readonly: false,
                optional: false,
                value: Type::Function {
                    parameters: method.parameters.clone(),
                    result: Box::new(method.return_type.clone().unwrap_or(Type::Unknown)),
                },
                span: method.span.clone(),
            });
        }
    }
    Type::Record(fields)
}

fn class_constructor_side_type(class: &ClassDeclaration) -> Type {
    Type::Record(vec![TypeField {
        name: "prototype".to_string(),
        readonly: true,
        optional: false,
        value: Type::Named {
            name: class.name.clone(),
            arguments: Vec::new(),
        },
        span: class.name_span.clone(),
    }])
}

fn class_method_overload_is_compatible(
    signature: &ClassMethod,
    implementation: &ClassMethod,
    aliases: &BTreeMap<String, TypeDefinition>,
    max_type_expansions: usize,
) -> Result<bool, ()> {
    let required_signature = signature
        .parameters
        .iter()
        .filter(|parameter| !parameter.optional)
        .count();
    let required_implementation = implementation
        .parameters
        .iter()
        .filter(|parameter| !parameter.optional)
        .count();
    if required_signature < required_implementation
        || signature.parameters.len() > implementation.parameters.len()
    {
        return Ok(false);
    }

    let mut budget = TypeExpansionBudget::new(max_type_expansions);
    let substitutions = BTreeMap::new();
    for (signature_parameter, implementation_parameter) in
        signature.parameters.iter().zip(&implementation.parameters)
    {
        let actual = parameter_expected_type(signature_parameter, &substitutions);
        let expected = parameter_expected_type(implementation_parameter, &substitutions);
        if !is_assignable(
            &actual,
            &expected,
            aliases,
            &mut HashSet::new(),
            &mut budget,
        ) {
            return if budget.exhausted { Err(()) } else { Ok(false) };
        }
        if budget.exhausted {
            return Err(());
        }
    }

    let actual = signature.return_type.as_ref().unwrap_or(&Type::Unknown);
    let expected = implementation
        .return_type
        .as_ref()
        .unwrap_or(&Type::Unknown);
    let compatible = is_assignable(actual, expected, aliases, &mut HashSet::new(), &mut budget);
    if budget.exhausted {
        Err(())
    } else {
        Ok(compatible)
    }
}
