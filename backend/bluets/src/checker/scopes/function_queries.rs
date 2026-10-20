// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A value query retains overloads in order, excluding the implementation.

use super::*;

pub(super) fn overloads(declarations: &[Declaration]) -> BTreeMap<String, Type> {
    let mut groups: BTreeMap<String, Vec<Type>> = BTreeMap::new();
    for declaration in declarations {
        let Declaration::Function(function) = declaration else {
            continue;
        };
        if !function.overload {
            continue;
        }
        let result = Box::new(function.return_type.clone().unwrap_or(Type::Unknown));
        let value = if function.type_parameters.is_empty() {
            Type::Function {
                parameters: function.parameters.clone(),
                result,
            }
        } else {
            Type::GenericFunction {
                type_parameters: function.type_parameters.clone(),
                parameters: function.parameters.clone(),
                result,
                span: function.span.clone(),
            }
        };
        groups.entry(function.name.clone()).or_default().push(value);
    }
    groups
        .into_iter()
        .map(|(name, mut values)| {
            let value = if values.len() == 1 {
                values.pop().unwrap()
            } else {
                Type::Intersection(values)
            };
            (name, value)
        })
        .collect()
}
