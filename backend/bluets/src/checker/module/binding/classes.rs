// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded class-method checks before class runtime admission.

use super::*;
use crate::parser::{ClassDeclaration, ClassMethod};

impl ModuleChecker<'_> {
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
