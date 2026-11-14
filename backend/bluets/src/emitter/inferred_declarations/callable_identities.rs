// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! An unchanged function value retains its declaration instead of a new arrow.

use super::*;

fn matches_function(
    function: &crate::parser::FunctionDeclaration,
    value: &Type,
    inferred: Option<&Type>,
) -> bool {
    let Some(first) = function.type_parameters.first() else {
        return false;
    };
    let Type::GenericFunction { span, .. } = value else {
        return false;
    };
    if *span != first.span {
        return false;
    }
    let Some(result) = function.return_type.as_ref().or(inferred) else {
        return false;
    };
    // Newly created higher-order values have a call-site origin. Structural
    // equivalence alone cannot identify a borrowed original function value.
    *value
        == Type::GenericFunction {
            type_parameters: function.type_parameters.clone(),
            parameters: function.parameters.clone(),
            result: Box::new(result.clone()),
            span: first.span.clone(),
        }
}

impl Context<'_> {
    pub(super) fn copied_function_name(&self, value: &Type) -> Option<String> {
        if !matches!(value, Type::GenericFunction { .. }) {
            return None;
        }
        for declaration in &self.module.declarations {
            if let Declaration::Function(function) = declaration {
                if matches_function(
                    function,
                    value,
                    self.checked.inferred_returns.get(&function.span.start),
                ) {
                    return Some(function.name.clone());
                }
            }
        }
        for declaration in &self.module.declarations {
            let Declaration::Import(import) = declaration else {
                continue;
            };
            let Some(source) = self
                .project
                .resolved_import(&self.module.id, import)
                .and_then(|id| self.project.modules.get(id))
            else {
                continue;
            };
            for binding in &import.bindings {
                if binding.imported == "*" {
                    continue;
                }
                let local = exported_local(source, &binding.imported);
                if source.declarations.iter().any(|declaration| {
                    matches!(declaration, Declaration::Function(function)
                        if function.name == local && matches_function(function, value, None))
                }) {
                    return Some(binding.local.clone());
                }
            }
        }
        None
    }
}
