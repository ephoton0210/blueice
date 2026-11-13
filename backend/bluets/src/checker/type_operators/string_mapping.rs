// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Pure string operations share ordinary substitution and expansion budgets.

use super::*;
use crate::parser::StringMappingType;
use crate::utf16::{CaseMapping, StringLiteral, Utf16String};

#[cfg(test)]
#[path = "string_mapping/tests.rs"]
mod tests;

fn symbolic(mapping: &StringMappingType, operand: Type) -> Type {
    Type::StringMapping(Box::new(StringMappingType {
        operand,
        ..mapping.clone()
    }))
}

pub(super) fn resolve(
    mapping: &StringMappingType,
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> Option<Type> {
    let canonical =
        super::super::type_relations::canonical_parameter_references(&mapping.operand, aliases);
    let parameter = matches!(&canonical, Type::Named { name, .. }
        if aliases.get(name).is_some_and(|definition| definition.kind == TypeDefinitionKind::Parameter));
    let operand = if parameter {
        canonical
    } else {
        expanded(&canonical, aliases, &mut visited.clone(), budget)
    };
    if budget.exhausted {
        return None;
    }
    if let Some(value) = operand.string_value() {
        return Some(Type::StringLiteral(StringLiteral::generated(
            value.mapped(mapping.operation),
        )));
    }
    match operand {
        Type::Never => Some(Type::Never),
        Type::Union(mut parts) => {
            if let Some(TypeDefinition {
                value: Type::Literal(profile),
                ..
            }) = aliases.get(super::STRING_LITERAL_PROFILE)
            {
                super::literal_order::order_strings(&mut parts, profile);
            }
            let mut mapped = Vec::with_capacity(parts.len());
            for operand in parts {
                if !budget.consume() {
                    return None;
                }
                mapped.push(resolve(
                    &StringMappingType {
                        operand,
                        ..mapping.clone()
                    },
                    aliases,
                    &mut visited.clone(),
                    budget,
                )?);
            }
            Some(union(mapped))
        }
        Type::StringMapping(previous)
            if previous.operation == mapping.operation && previous.origin == mapping.origin =>
        {
            Some(Type::StringMapping(previous))
        }
        Type::TemplateLiteral(template) => {
            if template.head.is_empty()
                && matches!(template.spans.as_slice(), [(Type::Number | Type::BigInt, tail)] if tail.is_empty())
            {
                return Some(symbolic(mapping, Type::TemplateLiteral(template)));
            }
            template_mapping(mapping, template, aliases, visited, budget)
        }
        operand @ (Type::Number | Type::BigInt) => Some(symbolic(
            mapping,
            Type::TemplateLiteral(TemplateLiteralType {
                head: String::new(),
                spans: vec![(operand, String::new())],
                span: mapping.origin.clone(),
                generated: true,
            }),
        )),
        operand @ (Type::Any
        | Type::String
        | Type::StringMapping(_)
        | Type::IndexedAccess { .. }
        | Type::Conditional(_)
        | Type::Named { .. }) => Some(symbolic(mapping, operand)),
        // A source intrinsic need not declare an extends-string constraint.
        // Non-string operands keep their ordinary type; validation of an
        // actual declared constraint stays with generic argument checking.
        operand => Some(operand),
    }
}

fn template_mapping(
    mapping: &StringMappingType,
    mut template: TemplateLiteralType,
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> Option<Type> {
    let full = matches!(mapping.operation, CaseMapping::Upper | CaseMapping::Lower);
    let head = Utf16String::parse_template_chunk(&template.head)?;
    let head_empty = head.units().is_empty();
    template.head = head.mapped(mapping.operation).template_source();
    for (index, (operand, tail)) in template.spans.iter_mut().enumerate() {
        if !budget.consume() {
            return None;
        }
        if full || head_empty && index == 0 {
            *operand = resolve(
                &StringMappingType {
                    operand: operand.clone(),
                    ..mapping.clone()
                },
                aliases,
                &mut visited.clone(),
                budget,
            )?;
        }
        if full {
            *tail = Utf16String::parse_template_chunk(tail)?
                .mapped(mapping.operation)
                .template_source();
        }
    }
    template.generated = true;
    Some(Type::TemplateLiteral(template))
}
