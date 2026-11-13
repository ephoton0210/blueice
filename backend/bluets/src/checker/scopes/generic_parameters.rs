// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Flow values retain the generic declaration that owns each parameter name.
use super::*;

impl ScopeModel<'_> {
    pub(super) fn declare_type_parameters(&mut self, scope: ScopeId, parameters: &[TypeParameter]) {
        for parameter in parameters {
            self.scopes[scope].types.insert(parameter.name.clone());
            self.generic_parameters
                .insert((scope, parameter.name.clone()), parameter.clone());
        }
        let substitutions = self.parameter_substitutions(scope);
        for parameter in parameters {
            self.type_definitions.insert(
                crate::parser::type_parameter_identity(parameter),
                TypeDefinition {
                    kind: TypeDefinitionKind::Parameter,
                    parameters: Vec::new(),
                    value: parameter
                        .constraint
                        .as_ref()
                        .map(|value| substitute_type(value, &substitutions))
                        .unwrap_or(Type::StrictUnknown),
                },
            );
        }
    }

    fn parameter_substitutions(&self, scope: ScopeId) -> BTreeMap<String, Type> {
        let mut substitutions = BTreeMap::new();
        let mut seen = BTreeSet::new();
        let mut current = Some(scope);
        while let Some(scope) = current {
            for name in &self.scopes[scope].types {
                if !seen.insert(name) {
                    continue;
                }
                if let Some(parameter) = self.generic_parameters.get(&(scope, name.clone())) {
                    substitutions.insert(
                        name.clone(),
                        Type::Named {
                            name: crate::parser::type_parameter_identity(parameter),
                            arguments: Vec::new(),
                        },
                    );
                }
            }
            current = self.scopes[scope].parent;
        }
        substitutions
    }

    pub(super) fn flow_parameter_type(&self, value: &Type, declaration_scope: ScopeId) -> Type {
        // Captured values use the declaring scope, including when a nested
        // function shadows the same spelling with an unrelated parameter.
        match self.parameter_substitution_cache.get(&declaration_scope) {
            Some(substitutions) => substitute_type(value, substitutions),
            None => substitute_type(value, &self.parameter_substitutions(declaration_scope)),
        }
    }

    pub(in crate::checker) fn parameter_type_at(
        &self,
        value: &Type,
        declaration_offset: usize,
    ) -> Type {
        self.flow_parameter_type(value, self.scope_at(declaration_offset))
    }

    pub(super) fn cache_parameter_substitutions(&mut self) {
        // Build only after the complete scope tree is collected. Repeated flow
        // snapshots need no new walk over module and enclosing type names.
        for scope in 0..self.scopes.len() {
            let substitutions = self.parameter_substitutions(scope);
            self.parameter_substitution_cache
                .insert(scope, substitutions);
        }
    }

    pub(in crate::checker) fn flow_type_parameters(&self) -> BTreeMap<String, TypeDefinition> {
        self.generic_parameters
            .values()
            .map(|parameter| {
                let identity = crate::parser::type_parameter_identity(parameter);
                let definition = self.type_definitions[&identity].clone();
                (identity, definition)
            })
            .collect()
    }
}
