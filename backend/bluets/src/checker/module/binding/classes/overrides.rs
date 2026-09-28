// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Compatibility checks for inherited class method declarations.

use super::*;
use crate::parser::ClassMethodGroup;

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn validate_local_class_method_overrides(
        &mut self,
        class: &ClassDeclaration,
    ) {
        for group in &class.method_groups {
            if !group.signature_member_indices.is_empty() {
                continue;
            }
            let Some(derived) = group
                .implementation_member_index
                .and_then(|index| class.members[index].method.as_ref())
            else {
                continue;
            };
            let Some((base, base_method)) = nearest_local_method(
                &self.module.declarations,
                class,
                group,
                self.max_type_expansions,
            ) else {
                continue;
            };
            // Optional, rest, and differing-arity method variance belongs to
            // the broader override relation. Preserve those cases for it.
            if derived.return_type.is_none()
                || base_method.return_type.is_none()
                || derived.parameters.len() != base_method.parameters.len()
                || derived
                    .parameters
                    .iter()
                    .chain(&base_method.parameters)
                    .any(|parameter| {
                        parameter.optional
                            || parameter.rest
                            || parameter.default.is_some()
                            || parameter.annotation.is_none()
                    })
            {
                continue;
            }
            let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
            // TypeScript compares class-method parameters bivariantly, then
            // requires the overriding result to fit the inherited result.
            let parameters_compatible =
                derived
                    .parameters
                    .iter()
                    .zip(&base_method.parameters)
                    .all(|(derived, base)| {
                        let derived = derived.annotation.as_ref().unwrap_or(&Type::Unknown);
                        let base = base.annotation.as_ref().unwrap_or(&Type::Unknown);
                        is_assignable(derived, base, &self.types, &mut HashSet::new(), &mut budget)
                            || is_assignable(
                                base,
                                derived,
                                &self.types,
                                &mut HashSet::new(),
                                &mut budget,
                            )
                    });
            let compatible = parameters_compatible
                && is_assignable(
                    derived.return_type.as_ref().unwrap_or(&Type::Unknown),
                    base_method.return_type.as_ref().unwrap_or(&Type::Unknown),
                    &self.types,
                    &mut HashSet::new(),
                    &mut budget,
                );
            if budget.exhausted {
                self.type_error(
                    &derived.span,
                    format!(
                        "class method override compatibility exceeds the {} generic-expansion limit",
                        self.max_type_expansions
                    ),
                    DiagnosticCode::ResourceLimit,
                );
            } else if !compatible {
                self.type_error(
                    &derived.span,
                    format!(
                        "class method `{}` is incompatible with inherited method from `{}`",
                        group.name, base.name
                    ),
                    DiagnosticCode::TypeMismatch,
                );
            }
        }
    }
}

fn nearest_local_method<'a>(
    declarations: &'a [Declaration],
    class: &ClassDeclaration,
    group: &ClassMethodGroup,
    max_edges: usize,
) -> Option<(&'a ClassDeclaration, &'a ClassMethod)> {
    let mut base_name = class.extends_name.as_deref();
    let mut visited = BTreeSet::new();
    for _ in 0..max_edges {
        let name = base_name?;
        if !visited.insert(name.to_string()) {
            return None;
        }
        let base = declarations.iter().find_map(|declaration| {
            let Declaration::Class(base) = declaration else {
                return None;
            };
            (base.name == name && base.name_span.start < class.name_span.start).then_some(base)
        })?;
        if let Some(candidate) = base.method_groups.iter().find(|candidate| {
            candidate.name == group.name && candidate.is_static == group.is_static
        }) {
            if !candidate.signature_member_indices.is_empty() {
                return None;
            }
            return candidate
                .implementation_member_index
                .and_then(|index| base.members[index].method.as_ref())
                .map(|method| (base, method));
        }
        base_name = base.extends_name.as_deref();
    }
    None
}
