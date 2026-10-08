// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Lexical type binders for classes and methods, including static restrictions.

use super::*;

impl ModuleChecker<'_> {
    pub(in crate::checker::module::binding) fn with_class_type_scope(
        &mut self,
        parameters: &[TypeParameter],
        check: impl FnOnce(&mut Self),
    ) {
        let previous_parameters = self.type_parameters.clone();
        let previous_types = self.types.clone();
        let previous_bound = self.bound_parameters.clone();
        let previous_spreads = self.allowed_tuple_spread_parameters.clone();
        self.bind_lexical_type_parameters(parameters);
        check(self);
        self.type_parameters = previous_parameters;
        self.types = previous_types;
        self.bound_parameters = previous_bound;
        self.allowed_tuple_spread_parameters = previous_spreads;
    }

    pub(in crate::checker::module::binding) fn bind_lexical_type_parameters(
        &mut self,
        parameters: &[TypeParameter],
    ) {
        let substitutions = parameters
            .iter()
            .map(|parameter| (parameter.name.clone(), parameter_reference(parameter)))
            .collect();
        for parameter in parameters {
            // A method binder may shadow its enclosing class binder. Duplicate
            // names within this declaration are still checked by the common gate.
            self.type_parameters.remove(&parameter.name);
            self.bound_parameters
                .insert(parameter.name.clone(), parameter.clone());
            let identity = crate::parser::type_parameter_identity(parameter);
            self.bound_parameters
                .insert(identity.clone(), parameter.clone());
            self.types.insert(
                identity.clone(),
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
            self.types.insert(
                parameter.name.clone(),
                TypeDefinition {
                    kind: TypeDefinitionKind::Alias,
                    parameters: Vec::new(),
                    value: parameter_reference(parameter),
                },
            );
            if let Some(constraint @ (Type::Array(_) | Type::Tuple(_))) = &parameter.constraint {
                self.allowed_tuple_spread_parameters
                    .insert(parameter.name.clone(), constraint.clone());
                self.allowed_tuple_spread_parameters
                    .insert(identity, substitute_type(constraint, &substitutions));
            }
        }
        self.check_type_parameters(parameters);
    }

    pub(in crate::checker::module::binding) fn validate_static_class_parameters(
        &mut self,
        class: &ClassDeclaration,
    ) {
        let parameters = class
            .type_parameters
            .iter()
            .map(|parameter| parameter.name.as_str())
            .collect::<BTreeSet<_>>();
        if parameters.is_empty() {
            return;
        }
        let mut invalid = Vec::new();
        for member in &class.members {
            let is_static = member
                .method
                .as_ref()
                .is_some_and(|method| method.is_static)
                || member.field.as_ref().is_some_and(|field| field.is_static)
                || member
                    .accessor
                    .as_ref()
                    .is_some_and(|accessor| accessor.is_static)
                || member.static_block.is_some()
                || member
                    .index
                    .as_ref()
                    .is_some_and(|(is_static, _)| *is_static);
            if !is_static {
                continue;
            }
            for reference in &self.module.type_references {
                if reference.value_query
                    || reference.span.start < member.span.start
                    || reference.span.end > member.span.end
                    || !parameters.contains(reference.name.as_str())
                    || self.scopes.as_ref().is_some_and(|scopes| {
                        !scopes.type_name_is_bound_in(
                            &reference.name,
                            reference.span.start,
                            &class.span,
                        )
                    })
                {
                    continue;
                }
                invalid.push(reference.span.clone());
            }
        }
        for span in invalid {
            self.typescript_type_error(
                &span,
                "static members cannot reference class type parameters".into(),
                DiagnosticCode::TypeMismatch,
                2302,
                Vec::new(),
            );
        }
    }
}
