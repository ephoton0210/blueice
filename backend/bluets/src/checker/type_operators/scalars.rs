// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Scalar aliases have no independent display identity or parameter constraint.
use super::*;

pub(in crate::checker) fn primitive_alias(
    value: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> Option<Type> {
    let mut value = value.clone();
    let mut visited = HashSet::new();
    loop {
        match &value {
            Type::Named { name, .. } => {
                let definition = aliases.get(name)?;
                if definition.kind != TypeDefinitionKind::Alias
                    || !matches!(
                        definition.value,
                        Type::Named { .. }
                            | Type::Any
                            | Type::Unknown
                            | Type::StrictUnknown
                            | Type::Never
                            | Type::Void
                            | Type::Null
                            | Type::Undefined
                            | Type::Boolean
                            | Type::Number
                            | Type::String
                            | Type::BigInt
                            | Type::Symbol
                            | Type::Literal(_)
                            | Type::StringLiteral(_)
                    )
                {
                    return None;
                }
                value = instantiate_named(
                    &value,
                    aliases,
                    &mut visited,
                    budget,
                    "scalar alias display",
                )?;
            }
            Type::Any
            | Type::Unknown
            | Type::StrictUnknown
            | Type::Never
            | Type::Void
            | Type::Null
            | Type::Undefined
            | Type::Boolean
            | Type::Number
            | Type::String
            | Type::BigInt
            | Type::Symbol
            | Type::Literal(_)
            | Type::StringLiteral(_) => return Some(value),
            _ => return None,
        }
    }
}
