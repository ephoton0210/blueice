// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Context-insensitive evidence is gathered before contextual callbacks are read.
use super::*;

impl ModuleChecker<'_> {
    pub(super) fn contextual_callback_widens_parameter(
        &self,
        signature: &FunctionSignature,
        index: usize,
        arguments: &[&[Token]],
    ) -> bool {
        let Some(Type::Named {
            name,
            arguments: types,
        }) = signature
            .parameters
            .get(index)
            .and_then(|p| p.annotation.as_ref())
        else {
            return false;
        };
        if !types.is_empty()
            || !signature
                .type_parameters
                .iter()
                .any(|p| p.name == *name && !p.is_const && p.constraint.is_none())
        {
            return false;
        }
        signature.parameters.iter().zip(arguments).any(|(parameter, argument)| {
            let Some(function) = strip_outer_parentheses(argument).first().and_then(|t| self.module.nested_functions.get(&t.start)) else {return false;};
            let Some(Type::Function {parameters,..}) = &parameter.annotation else {return false;};
            function.parameters.iter().zip(parameters).any(|(actual, expected)| actual.annotation.is_none()
                && matches!(&expected.annotation, Some(Type::Named {name: n, arguments}) if n == name && arguments.is_empty()))
        })
    }

    pub(super) fn generic_argument_seeds(
        &self,
        arguments: &[&[Token]],
        scope: &BTreeMap<String, Type>,
        signatures: &[FunctionSignature],
    ) -> Option<Vec<Type>> {
        if signatures.iter().all(|s| s.type_parameters.is_empty())
            || arguments
                .iter()
                .any(|a| a.first().is_some_and(|t| t.is("...")))
        {
            return None;
        }
        // A previous provisional context must not become a new argument's
        // independent evidence. Explicit annotations are never discarded.
        for argument in arguments {
            let (Some(first), Some(last)) = (argument.first(), argument.last()) else {
                continue;
            };
            for function in self
                .module
                .nested_functions
                .values()
                .filter(|f| first.start <= f.span.start && f.span.end <= last.end)
            {
                self.return_inference
                    .results
                    .borrow_mut()
                    .remove(&function.span.start);
                for parameter in &function.parameters {
                    if parameter.annotation.is_none() {
                        self.return_inference
                            .parameters
                            .borrow_mut()
                            .remove(&parameter.span.start);
                    }
                }
            }
        }
        self.expanded_call_argument_types(arguments, scope).ok()
    }
}
