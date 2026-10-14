// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Overload-set compatibility for inherited class methods.
//!
//! TypeScript relates an overriding method's signature list to the inherited
//! list: every inherited signature must be matched by some overriding
//! signature, comparing parameters bivariantly and results covariantly. An
//! overloaded declaration exposes only its overload signatures, never its
//! implementation signature. Signatures with rest parameters or missing
//! annotations are not compared and are left to the existing single-signature
//! check.

use super::*;

struct Signature {
    parameters: Vec<Parameter>,
    return_type: Type,
    type_parameters: Vec<TypeParameter>,
    span: SourceSpan,
}

/// The externally visible signatures of one method group, or `None` when a
/// signature is outside the compared subset.
fn visible_signatures(
    class: &ClassDeclaration,
    group: &ClassMethodGroup,
    substitutions: &BTreeMap<String, Type>,
) -> Option<Vec<Signature>> {
    let indices: Vec<usize> = if group.signature_member_indices.is_empty() {
        vec![group.implementation_member_index?]
    } else {
        group.signature_member_indices.clone()
    };
    indices
        .into_iter()
        .map(|index| {
            let method = class.members.get(index)?.method.as_ref()?;
            let return_type = method.return_type.as_ref()?;
            method
                .parameters
                .iter()
                .all(|parameter| !parameter.rest && parameter.annotation.is_some())
                .then(|| {
                    let value = if method.type_parameters.is_empty() {
                        Type::Function {
                            parameters: method.parameters.clone(),
                            result: Box::new(return_type.clone()),
                        }
                    } else {
                        Type::GenericFunction {
                            type_parameters: method.type_parameters.clone(),
                            parameters: method.parameters.clone(),
                            result: Box::new(return_type.clone()),
                            span: method.span.clone(),
                        }
                    };
                    let value = substitute_type(&value, substitutions);
                    let type_parameters = match &value {
                        Type::GenericFunction {
                            type_parameters, ..
                        } => type_parameters.clone(),
                        _ => Vec::new(),
                    };
                    let (Type::Function { parameters, result }
                    | Type::GenericFunction {
                        parameters, result, ..
                    }) = value
                    else {
                        unreachable!()
                    };
                    Signature {
                        parameters,
                        return_type: *result,
                        type_parameters,
                        span: method.span.clone(),
                    }
                })
        })
        .collect()
}

/// The nearest earlier local ancestor declaring the same method side.
fn nearest_local_group<'a>(
    declarations: &'a [Declaration],
    class: &ClassDeclaration,
    group: &ClassMethodGroup,
    max_edges: usize,
) -> Option<(
    &'a ClassDeclaration,
    &'a ClassMethodGroup,
    BTreeMap<String, Type>,
)> {
    let mut base_name = class.extends_name.clone();
    let mut arguments = class.extends_arguments.clone();
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
        })?;
        let substitutions =
            super::super::heritage::heritage_substitutions(&base.type_parameters, &arguments);
        if let Some(candidate) = base.method_groups.iter().find(|candidate| {
            candidate.name == group.name && candidate.is_static == group.is_static
        }) {
            return Some((base, candidate, substitutions));
        }
        arguments = base
            .extends_arguments
            .iter()
            .map(|value| substitute_type(value, &substitutions))
            .collect();
        base_name = base.extends_name.clone();
    }
    None
}

fn signature_related(
    derived: &Signature,
    inherited: &Signature,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> bool {
    if !derived.type_parameters.is_empty() || !inherited.type_parameters.is_empty() {
        let surface = |signature: &Signature| {
            Type::Record(vec![TypeField {
                accessor_write_type: None,
                name: "method".into(),
                method: true,
                readonly: false,
                optional: false,
                value: Type::GenericFunction {
                    type_parameters: signature.type_parameters.clone(),
                    parameters: signature.parameters.clone(),
                    result: Box::new(signature.return_type.clone()),
                    span: signature.span.clone(),
                },
                span: signature.span.clone(),
            }])
        };
        return is_assignable(
            &surface(derived),
            &surface(inherited),
            aliases,
            &mut HashSet::new(),
            budget,
        );
    }
    let required = derived
        .parameters
        .iter()
        .filter(|parameter| !parameter.optional)
        .count();
    required <= inherited.parameters.len()
        && derived
            .parameters
            .iter()
            .zip(&inherited.parameters)
            .all(|(derived, inherited)| {
                parameter_types_compatible(
                    &override_parameter_type(derived),
                    &override_parameter_type(inherited),
                    aliases,
                    budget,
                )
            })
        && (matches!(&inherited.return_type, Type::Void)
            || is_assignable(
                &derived.return_type,
                &inherited.return_type,
                aliases,
                &mut HashSet::new(),
                budget,
            ))
}

impl ModuleChecker<'_> {
    /// Check a method group against an inherited overload set. Returns true
    /// when either side is overloaded, so the single-signature check skips it.
    pub(in crate::checker::module) fn validate_overload_set_override(
        &mut self,
        class: &ClassDeclaration,
        group: &ClassMethodGroup,
    ) -> bool {
        let Some((base, inherited_group, substitutions)) = nearest_local_group(
            &self.module.declarations,
            class,
            group,
            self.max_type_expansions,
        ) else {
            return !group.signature_member_indices.is_empty();
        };
        if group.signature_member_indices.is_empty()
            && inherited_group.signature_member_indices.is_empty()
        {
            return false;
        }
        let (Some(derived), Some(inherited)) = (
            visible_signatures(class, group, &BTreeMap::new()),
            visible_signatures(base, inherited_group, &substitutions),
        ) else {
            return true;
        };
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        let compatible = inherited.iter().all(|inherited| {
            derived
                .iter()
                .any(|derived| signature_related(derived, inherited, &self.types, &mut budget))
        });
        if budget.exhausted {
            self.type_error(
                &group.span,
                format!(
                    "overload comparison exceeds the {} generic-expansion limit",
                    self.max_type_expansions
                ),
                DiagnosticCode::ResourceLimit,
            );
        } else if !compatible {
            let (code, arguments) = super::diagnostics::override_context(class, group);
            self.typescript_type_error(
                &group.span,
                format!(
                    "class method `{}` is not assignable to the inherited overload set of `{}`",
                    group.name, base.name
                ),
                DiagnosticCode::TypeMismatch,
                code,
                arguments,
            );
        }
        true
    }
}
