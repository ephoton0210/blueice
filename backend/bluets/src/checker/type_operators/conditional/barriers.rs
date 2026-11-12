// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Validate blocked pattern positions after unblocked inference has finished.

use super::*;

pub(super) fn instantiate(
    value: &Type,
    bindings: &BTreeMap<String, Type>,
    budget: &mut TypeExpansionBudget,
) -> Type {
    if !budget.consume() {
        return value.clone();
    }
    if let Type::Infer(parameter) = value {
        return bindings
            .get(&parameter.name)
            .cloned()
            .unwrap_or(Type::StrictUnknown);
    }
    value.map_children(|value, bound| {
        let mut bindings = bindings.clone();
        for name in bound {
            bindings.remove(name);
        }
        instantiate(value, &bindings, budget)
    })
}
