// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Concrete mapped slots reduce inside signatures without expanding named APIs.
use super::*;

pub(super) fn indexed(
    value: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> Type {
    if !budget.consume() {
        return value.clone();
    }
    if matches!(value, Type::IndexedAccess { .. })
        && !crate::checker::inference::symbolic_indexed_access(value, aliases)
    {
        let reduced =
            crate::checker::inference::result_type(value, &BTreeMap::new(), aliases, budget);
        if reduced != *value && !budget.exhausted {
            return indexed(&reduced, aliases, budget);
        }
    }
    let mut scoped;
    let aliases = if let Type::GenericFunction {
        type_parameters, ..
    } = value
    {
        scoped = aliases.clone();
        for parameter in type_parameters {
            scoped.remove(&parameter.name);
            scoped.remove(&crate::parser::type_parameter_identity(parameter));
        }
        &scoped
    } else {
        aliases
    };
    value.map_children(|child, _| indexed(child, aliases, budget))
}
