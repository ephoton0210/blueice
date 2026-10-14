// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Homomorphic modifiers survive key remapping and bounded property expansion.
use super::*;

fn modifier(value: MappedModifier, inherited: bool) -> bool {
    match value {
        MappedModifier::Preserve => inherited,
        MappedModifier::Add => true,
        MappedModifier::Remove => false,
    }
}

pub(super) fn resolve(
    value: &MappedType,
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> Option<Type> {
    let constraint = value.parameter.constraint.as_ref()?;
    let origin = match (constraint, &value.value) {
        (Type::KeyOf(object), _) => Some(expanded(object, aliases, &mut visited.clone(), budget)),
        (_, Type::IndexedAccess { object, index, .. })
            if matches!(index.as_ref(), Type::Named { name, arguments }
                if name == &value.parameter.name && arguments.is_empty()) =>
        {
            Some(expanded(object, aliases, &mut visited.clone(), budget))
        }
        _ => None,
    };
    if value.name_type.is_none() {
        if let Some(origin) = &origin {
            let (source, readonly) = match origin {
                Type::Readonly(source) => (source.as_ref(), true),
                source => (source, false),
            };
            let sequence = match source {
                Type::Array(_) => {
                    let substitutions =
                        BTreeMap::from([(value.parameter.name.clone(), Type::Number)]);
                    let item = expanded(
                        &substitute_type(&value.value, &substitutions),
                        aliases,
                        &mut visited.clone(),
                        budget,
                    );
                    Some(Type::Array(Box::new(item)))
                }
                Type::Tuple(items) => {
                    let mut result = Vec::new();
                    for (index, item) in items.iter().enumerate() {
                        if !budget.consume() {
                            return None;
                        }
                        let substitutions = BTreeMap::from([(
                            value.parameter.name.clone(),
                            Type::Literal(format!("\"{index}\"")),
                        )]);
                        result.push(TupleTypeElement {
                            annotation: expanded(
                                &substitute_type(&value.value, &substitutions),
                                aliases,
                                &mut visited.clone(),
                                budget,
                            ),
                            optional: modifier(value.optional, item.optional),
                            ..item.clone()
                        });
                    }
                    Some(Type::Tuple(result))
                }
                _ => None,
            };
            if let Some(sequence) = sequence {
                return Some(if modifier(value.readonly, readonly) {
                    Type::Readonly(Box::new(sequence))
                } else {
                    sequence
                });
            }
        }
    }
    let keys = expanded(constraint, aliases, &mut visited.clone(), budget);
    let keys = match keys {
        Type::Never => vec![],
        Type::Union(keys) => keys,
        Type::Literal(_) => vec![keys],
        _ => return None,
    };
    let mut fields: Vec<TypeField> = Vec::new();
    for key in keys {
        if !budget.consume() {
            return None;
        }
        let substitutions = BTreeMap::from([(value.parameter.name.clone(), key.clone())]);
        let mapped_key = value
            .name_type
            .as_ref()
            .map(|name| substitute_type(name, &substitutions))
            .unwrap_or_else(|| key.clone());
        let mapped_key = expanded(&mapped_key, aliases, &mut visited.clone(), budget);
        let mapped_keys = match mapped_key {
            Type::Never => vec![],
            Type::Union(keys) => keys,
            key => vec![key],
        };
        let inherited = match &origin {
            Some(Type::Record(fields) | Type::CallableRecord { fields, .. }) => fields
                .iter()
                .find(|field| Some(field.name.clone()) == keys::key_name(&key)),
            _ => None,
        };
        let property = substitute_type(&value.value, &substitutions);
        let mut property = expanded(&property, aliases, &mut visited.clone(), budget);
        if value.optional == MappedModifier::Remove && inherited.is_some_and(|field| field.optional)
        {
            if let Type::Union(parts) = property {
                property = union(parts.into_iter().filter(|part| *part != Type::Undefined));
            }
        }
        for key in mapped_keys {
            if !budget.consume() {
                return None;
            }
            let name = keys::key_name(&key)?;
            if let Some(field) = fields.iter_mut().find(|field| field.name == name) {
                field.value = union([field.value.clone(), property.clone()]);
            } else {
                fields.push(TypeField {
                    accessor_write_type: None,
                    method: false,
                    name,
                    value: property.clone(),
                    readonly: modifier(
                        value.readonly,
                        inherited.is_some_and(|field| field.readonly),
                    ),
                    optional: modifier(
                        value.optional,
                        inherited.is_some_and(|field| field.optional),
                    ),
                    span: inherited
                        .filter(|_| value.name_type.is_none())
                        .map(|field| field.span.clone())
                        .unwrap_or_else(|| value.span.clone()),
                });
            }
        }
    }
    Some(Type::Record(fields))
}
