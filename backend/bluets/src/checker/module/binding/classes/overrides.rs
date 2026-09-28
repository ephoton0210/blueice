// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Compatibility checks for inherited class method declarations.

use super::*;
use crate::parser::{ClassMethodGroup, Parameter};

struct InheritedMethod<'a> {
    base_name: String,
    parameters: &'a [Parameter],
    return_type: Option<&'a Type>,
}

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn validate_class_method_overrides(
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
            let Some(inherited) = nearest_inherited_method(
                &self.module.declarations,
                &self.types,
                &self.values,
                &self.class_constructors,
                class,
                group,
                self.max_type_expansions,
            ) else {
                continue;
            };
            // Different fixed prefixes or one-sided rests need positional
            // expansion. Matching final array rests compare as array types.
            if !matching_array_rests(&derived.parameters, inherited.parameters) {
                continue;
            }
            if derived.return_type.is_none()
                || inherited.return_type.is_none()
                || derived
                    .parameters
                    .iter()
                    .chain(inherited.parameters)
                    .any(|parameter| parameter.annotation.is_none())
            {
                continue;
            }
            let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
            // TypeScript compares class-method parameters bivariantly, then
            // requires the overriding result to fit the inherited result.
            // An override may omit inherited positions or add omittable ones;
            // it cannot require more positions than the base declares.
            let required_derived = derived
                .parameters
                .iter()
                .filter(|parameter| !parameter.optional && !parameter.rest)
                .count();
            let parameters_compatible = required_derived <= inherited.parameters.len()
                && derived
                    .parameters
                    .iter()
                    .zip(inherited.parameters)
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
                    inherited.return_type.unwrap_or(&Type::Unknown),
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
                        group.name, inherited.base_name
                    ),
                    DiagnosticCode::TypeMismatch,
                );
            }
        }
    }
}

fn nearest_inherited_method<'a>(
    declarations: &'a [Declaration],
    types: &'a BTreeMap<String, TypeDefinition>,
    values: &'a BTreeMap<String, Type>,
    constructors: &BTreeMap<String, ClassConstructorBinding>,
    class: &ClassDeclaration,
    group: &ClassMethodGroup,
    max_edges: usize,
) -> Option<InheritedMethod<'a>> {
    let mut base_name = class.extends_name.clone();
    let mut visited = BTreeSet::new();
    for _ in 0..max_edges {
        let name = base_name?;
        if !visited.insert(name.clone()) {
            return None;
        }
        let base = declarations.iter().find_map(|declaration| {
            let Declaration::Class(base) = declaration else {
                return None;
            };
            (base.name == name && base.name_span.start < class.name_span.start).then_some(base)
        });
        if let Some(base) = base {
            if let Some(candidate) = base.method_groups.iter().find(|candidate| {
                candidate.name == group.name && candidate.is_static == group.is_static
            }) {
                if !candidate.signature_member_indices.is_empty() {
                    return None;
                }
                return candidate
                    .implementation_member_index
                    .and_then(|index| base.members[index].method.as_ref())
                    .map(|method| InheritedMethod {
                        base_name: base.name.clone(),
                        parameters: &method.parameters,
                        return_type: method.return_type.as_ref(),
                    });
            }
            base_name = base.extends_name.clone();
            continue;
        }
        // A forward local base has its own heritage diagnostic. Only a bound
        // value-imported class can provide a runtime base surface here.
        if declarations.iter().any(
            |declaration| matches!(declaration, Declaration::Class(local) if local.name == name),
        ) || !constructors.contains_key(&name)
        {
            return None;
        }
        let surface = if group.is_static {
            values.get(&name)
        } else {
            types.get(&name).and_then(|definition| {
                (definition.kind == TypeDefinitionKind::Class).then_some(&definition.value)
            })
        };
        let Type::Record(fields) = surface? else {
            return None;
        };
        let mut matching = fields.iter().filter(|field| field.name == group.name);
        let field = matching.next()?;
        if matching.next().is_some() {
            return None;
        }
        let Type::Function { parameters, result } = &field.value else {
            return None;
        };
        return Some(InheritedMethod {
            base_name: name,
            parameters,
            return_type: Some(result),
        });
    }
    None
}

fn matching_array_rests(derived: &[Parameter], inherited: &[Parameter]) -> bool {
    let has_rest = derived
        .iter()
        .chain(inherited)
        .any(|parameter| parameter.rest);
    if !has_rest {
        return true;
    }
    let (Some((derived_rest, derived_fixed)), Some((inherited_rest, inherited_fixed))) =
        (derived.split_last(), inherited.split_last())
    else {
        return false;
    };
    derived_rest.rest
        && inherited_rest.rest
        && derived_fixed.len() == inherited_fixed.len()
        && !derived_fixed
            .iter()
            .chain(inherited_fixed)
            .any(|parameter| parameter.rest)
        && matches!(derived_rest.annotation.as_ref(), Some(Type::Array(_)))
        && matches!(inherited_rest.annotation.as_ref(), Some(Type::Array(_)))
}
