// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Preserve lexical parameter identities before another binder can shadow them.

use super::*;

pub(in crate::checker) fn canonical_parameter_references(
    value: &Type,
    definitions: &BTreeMap<String, TypeDefinition>,
) -> Type {
    let substitutions = definitions
        .iter()
        .filter_map(|(name, definition)| {
            if definition.kind != TypeDefinitionKind::Alias || !definition.parameters.is_empty() {
                return None;
            }
            let Type::Named {
                name: target,
                arguments,
            } = &definition.value
            else {
                return None;
            };
            (arguments.is_empty()
                && definitions
                    .get(target)
                    .is_some_and(|definition| definition.kind == TypeDefinitionKind::Parameter))
            .then(|| (name.clone(), definition.value.clone()))
        })
        .collect();
    substitute_type(value, &substitutions)
}
