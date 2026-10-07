// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Later interface declaration groups precede earlier overload groups.
use super::*;

pub(super) fn merge_interface_values(earlier: &Type, later: &Type) -> Type {
    let members = |value: &Type| match value {
        Type::Record(fields) => Some((fields.clone(), Vec::new())),
        Type::CallableRecord { fields, signatures } => Some((fields.clone(), signatures.clone())),
        _ => None,
    };
    if let (Some((old_fields, old_signatures)), Some((mut fields, mut signatures))) =
        (members(earlier), members(later))
    {
        for field in old_fields {
            if matches!(
                field.value,
                Type::Function { .. } | Type::GenericFunction { .. }
            ) || !fields.iter().any(|existing| existing.name == field.name)
            {
                fields.push(field);
            }
        }
        signatures.extend(old_signatures);
        return if signatures.is_empty() {
            Type::Record(fields)
        } else {
            Type::CallableRecord { fields, signatures }
        };
    }
    Type::Intersection(vec![later.clone(), earlier.clone()])
}
