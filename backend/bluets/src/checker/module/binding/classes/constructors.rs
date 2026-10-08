// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Constructor capabilities survive value aliases and inherited signatures.

use super::*;
use crate::parser::TypeSignature;

impl ClassConstructorBinding {
    pub(in crate::checker) fn value_signatures(&self, span: &SourceSpan) -> Vec<TypeSignature> {
        self.signatures
            .iter()
            .map(|signature| TypeSignature {
                construct: true,
                abstract_constructor: self.modifiers.abstract_class,
                constructor_visibility: self.visibility,
                constructor_arrow: false,
                type_parameters: signature.type_parameters.clone(),
                parameters: signature.parameters.clone(),
                result: signature.return_type.clone(),
                span: span.clone(),
            })
            .collect()
    }
}

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn bind_class_constructor_value_signatures(&mut self) {
        for (name, binding) in &self.class_constructors {
            let Some(Type::CallableRecord { signatures, .. }) =
                self.values.get_mut(name).map(Type::object_type_mut)
            else {
                continue;
            };
            let Some(span) = signatures.first().map(|signature| signature.span.clone()) else {
                continue;
            };
            *signatures = binding.value_signatures(&span);
        }
    }

    pub(super) fn reject_inaccessible_aliased_constructor(
        &mut self,
        tokens: &[Token],
        callee: &str,
        scope: &BTreeMap<String, Type>,
    ) -> bool {
        if self.is_bound_class_constructor_value(callee, scope) {
            return false;
        }
        let mut value = scope.get(callee).cloned().unwrap_or(Type::Unknown);
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        let mut visited = HashSet::new();
        while let Some(expanded) = instantiate_named(
            &value,
            &self.types,
            &mut visited,
            &mut budget,
            "aliased constructor",
        ) {
            value = expanded;
        }
        let Type::CallableRecord { signatures, .. } = value.object_type() else {
            return false;
        };
        let Some((visibility, name)) = signatures.iter().find_map(|signature| {
            let Type::Named { name, .. } = &signature.result else {
                return None;
            };
            (signature.construct
                && !self.constructor_is_accessible(name, signature.constructor_visibility))
            .then_some((signature.constructor_visibility, name.clone()))
        }) else {
            return false;
        };
        self.type_error(
            &SourceSpan::new(&self.module.id, tokens[0].start, tokens.last().unwrap().end),
            format!(
                "constructor of class {name} is {} and is not accessible here",
                visibility.keyword()
            ),
            DiagnosticCode::TypeMismatch,
        );
        true
    }
}
