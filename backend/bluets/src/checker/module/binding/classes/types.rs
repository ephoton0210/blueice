// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Class instance/constructor surfaces and overload compatibility.

use super::*;

pub(super) fn class_constructor_signatures(class: &ClassDeclaration) -> Vec<FunctionSignature> {
    let constructors = class
        .members
        .iter()
        .filter_map(|member| member.constructor.as_ref())
        .collect::<Vec<_>>();
    let overloads = constructors
        .iter()
        .copied()
        .filter(|constructor| constructor.body.is_none())
        .collect::<Vec<_>>();
    let selected = if overloads.is_empty() {
        constructors
            .into_iter()
            .filter(|constructor| constructor.body.is_some())
            .collect::<Vec<_>>()
    } else {
        overloads
    };
    if selected.is_empty() {
        return vec![FunctionSignature {
            parameters: Vec::new(),
            type_parameters: class.type_parameters.clone(),
            return_type: Type::Named {
                name: class.name.clone(),
                arguments: class
                    .type_parameters
                    .iter()
                    .map(|p| Type::Named {
                        name: p.name.clone(),
                        arguments: Vec::new(),
                    })
                    .collect(),
            },
        }];
    }
    selected
        .into_iter()
        .map(|constructor| FunctionSignature {
            parameters: constructor.parameters.clone(),
            type_parameters: class.type_parameters.clone(),
            return_type: Type::Named {
                name: class.name.clone(),
                arguments: class
                    .type_parameters
                    .iter()
                    .map(|p| Type::Named {
                        name: p.name.clone(),
                        arguments: Vec::new(),
                    })
                    .collect(),
            },
        })
        .collect()
}

/// Whether every class member is a parsed constructor or method, the only
/// forms whose emission is fully erased and declared.
pub(in crate::checker::module) fn class_is_fully_structured(class: &ClassDeclaration) -> bool {
    class.members.iter().all(|member| match member.kind {
        ClassMemberKind::Constructor => member.constructor.is_some(),
        ClassMemberKind::Method => member.method.is_some(),
        ClassMemberKind::Field => member.field.is_some(),
        ClassMemberKind::Accessor => member.accessor.is_some(),
        ClassMemberKind::StaticBlock => member.static_block.is_some(),
        ClassMemberKind::Opaque => false,
    })
}

/// The accessibility of the class's declared constructor, public when it
/// declares none. An omitted constructor's inherited accessibility is filled in
/// once the base is bound.
pub(super) fn class_constructor_visibility(class: &ClassDeclaration) -> Visibility {
    class
        .members
        .iter()
        .filter_map(|member| member.constructor.as_ref())
        .map(|constructor| constructor.visibility)
        .next()
        .unwrap_or_default()
}

pub(super) fn class_declares_constructor(class: &ClassDeclaration) -> bool {
    class
        .members
        .iter()
        .any(|member| member.constructor.is_some())
}

pub(super) fn class_method_fields(class: &ClassDeclaration, is_static: bool) -> Vec<TypeField> {
    let mut fields = fields::class_field_type_fields(class, is_static);
    fields.extend(accessors::class_accessor_type_fields(class, is_static));
    for group in &class.method_groups {
        if group.is_static != is_static {
            continue;
        }
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
                method: true,
                name: visibility::member_field_name(class, method.visibility, &method.name),
                readonly: false,
                optional: method.optional,
                value: Type::Function {
                    parameters: method.parameters.clone(),
                    result: Box::new(method.return_type.clone().unwrap_or(Type::Unknown)),
                },
                span: method.span.clone(),
            });
        }
    }
    // Fields an interface of the same name adds to the instance type; one the
    // class also declares is the class's.
    if !is_static {
        for merged in &class.merged_interface_fields {
            if !fields.iter().any(|field| field.name == merged.name) {
                fields.push(merged.clone());
            }
        }
    }
    fields
}

pub(in crate::checker::module) fn class_constructor_side_type(class: &ClassDeclaration) -> Type {
    let mut fields = vec![TypeField {
        method: false,
        name: "prototype".to_string(),
        readonly: true,
        optional: false,
        value: Type::Named {
            name: class.name.clone(),
            arguments: Vec::new(),
        },
        span: class.name_span.clone(),
    }];
    fields.extend(class_method_fields(class, true));
    let binding = ClassConstructorBinding {
        modifiers: ClassModifierSurface::declared(class),
        signatures: class_constructor_signatures(class),
        inherited: class.extends_name.is_some() && !class_declares_constructor(class),
        visibility: class_constructor_visibility(class),
    };
    Type::CallableRecord {
        fields,
        signatures: binding.value_signatures(&class.name_span),
    }
}

pub(super) fn class_constructor_overload_is_compatible(
    signature: &ClassConstructor,
    implementation: &ClassConstructor,
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
    Ok(true)
}

pub(super) fn class_method_overload_is_compatible(
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
