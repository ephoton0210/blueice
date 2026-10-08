// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Callable values retain every call and construct candidate.
use super::*;

pub(in crate::checker::module) fn callable_signatures(
    value: &Type,
    construct: bool,
) -> Option<Vec<FunctionSignature>> {
    match value {
        Type::IndexedRecord { object, .. } => callable_signatures(object, construct),
        Type::Function { parameters, result } if !construct => Some(vec![FunctionSignature {
            parameters: parameters.clone(),
            type_parameters: Vec::new(),
            return_type: *result.clone(),
        }]),
        Type::GenericFunction {
            parameters,
            type_parameters,
            result,
            ..
        } if !construct => Some(vec![FunctionSignature {
            parameters: parameters.clone(),
            type_parameters: type_parameters.clone(),
            return_type: *result.clone(),
        }]),
        Type::CallableRecord { signatures, .. } => {
            let candidates = signatures
                .iter()
                .filter(|signature| signature.construct == construct)
                .map(|signature| FunctionSignature {
                    parameters: signature.parameters.clone(),
                    type_parameters: signature.type_parameters.clone(),
                    return_type: signature.result.clone(),
                })
                .collect::<Vec<_>>();
            (!candidates.is_empty()).then_some(candidates)
        }
        Type::Intersection(parts) => {
            let candidates = parts
                .iter()
                .map(|part| callable_signatures(part, construct))
                .collect::<Option<Vec<_>>>()?
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
            (!candidates.is_empty()).then_some(candidates)
        }
        _ => None,
    }
}

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn function_value_signature(
        &self,
        callee: &str,
        scope: &BTreeMap<String, Type>,
    ) -> Option<FunctionSignature> {
        self.function_value_signatures(callee, scope, false)?.pop()
    }

    pub(in crate::checker::module) fn function_value_signatures(
        &self,
        callee: &str,
        scope: &BTreeMap<String, Type>,
        construct: bool,
    ) -> Option<Vec<FunctionSignature>> {
        let bound = scope.get(callee)?;
        if construct && self.is_bound_class_constructor_value(callee, scope) {
            return None;
        }
        if self.functions.contains_key(callee) && self.values.get(callee) == Some(bound) {
            return None;
        }
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        let mut bound = bound.clone();
        let mut visited = HashSet::new();
        while let Some(expanded) = instantiate_named(
            &bound,
            &self.types,
            &mut visited,
            &mut budget,
            "callable value",
        ) {
            bound = expanded;
        }
        signatures::callable_signatures(&bound, construct)
    }
}
