// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Checking of arrow functions and function expressions parsed inside runtime
//! expressions.
//!
//! A nested function is checked as an anonymous function whose body sees the
//! scope it appears in, so its parameters, result type and closed-over variables follow
//! the same rules as a named function. Its type is a function type built from
//! its parameters and result annotation.

use super::*;
use crate::parser::{NestedFunction, NestedFunctionBody};

impl ModuleChecker<'_> {
    /// Checks every outermost arrow function inside `tokens`. An arrow nested
    /// in another arrow's body is checked when that body is.
    pub(in crate::checker::module) fn check_nested_functions_in(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
    ) {
        let (Some(first), Some(last)) = (tokens.first(), tokens.last()) else {
            return;
        };
        let module = self.module;
        let mut cursor = first.start;
        for (_, arrow) in module.nested_functions.range(first.start..last.end) {
            if arrow.span.start < cursor || arrow.span.end > last.end {
                continue;
            }
            cursor = arrow.span.end;
            if !self.checked_nested_functions.insert(arrow.span.start) {
                continue;
            }
            self.check_nested_function(arrow, scope);
        }
    }

    fn check_nested_function(&mut self, arrow: &NestedFunction, scope: &BTreeMap<String, Type>) {
        let (body, returns, locals) = match &arrow.body {
            NestedFunctionBody::Expression(tokens) => (
                vec![FunctionBodyItem::Return {
                    tokens: tokens.clone(),
                    span: arrow.span.clone(),
                }],
                vec![tokens.clone()],
                Vec::new(),
            ),
            NestedFunctionBody::Block {
                items,
                returns,
                locals,
            } => (items.clone(), returns.clone(), locals.clone()),
        };
        let function = FunctionDeclaration {
            name: "<arrow>".to_string(),
            async_function: false,
            body_open: None,
            type_parameters: arrow.type_parameters.clone(),
            parameters: arrow.parameters.clone(),
            return_type: arrow.return_type.clone(),
            body,
            returns,
            locals,
            exported: false,
            default_export: false,
            declared: false,
            overload: false,
            span: arrow.span.clone(),
        };
        let mut body_scope = scope.clone();
        if let Some(name) = &arrow.name {
            // A named function expression is visible inside its own body.
            body_scope.insert(
                name.clone(),
                erased_function_type(
                    &arrow.parameters,
                    arrow.return_type.clone().unwrap_or(Type::Unknown),
                    &arrow.type_parameters,
                ),
            );
        }
        self.check_function_in_scope(&function, body_scope);
    }

    /// The function type of `tokens` when they are exactly one arrow function.
    pub(in crate::checker::module) fn nested_function_type(
        &self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
    ) -> Option<Type> {
        let (first, last) = (tokens.first()?, tokens.last()?);
        let arrow = self.module.nested_functions.get(&first.start)?;
        if arrow.span.end != last.end {
            return None;
        }
        let result = match (&arrow.return_type, &arrow.body) {
            (Some(return_type), _) => return_type.clone(),
            (None, NestedFunctionBody::Expression(body)) => {
                let mut inner = scope.clone();
                for parameter in &arrow.parameters {
                    inner.insert(
                        parameter.name.clone(),
                        parameter.annotation.clone().unwrap_or(Type::Unknown),
                    );
                }
                self.infer_expression(body, &inner)
            }
            (None, NestedFunctionBody::Block { .. }) => Type::Unknown,
        };
        Some(erased_function_type(
            &arrow.parameters,
            result,
            &arrow.type_parameters,
        ))
    }
}

/// The function type of a nested function. Its type parameters are erased to
/// their constraints (or `unknown`), so a call site is checked against the
/// widest types the function accepts and cannot be rejected for passing a
/// concrete argument where the declaration says `T`. The result type is erased
/// the same way, so a generic result is not more precise than its constraint.
pub(in crate::checker::module) fn erased_function_type(
    parameters: &[Parameter],
    result: Type,
    type_parameters: &[TypeParameter],
) -> Type {
    if type_parameters.is_empty() {
        return Type::Function {
            parameters: parameters.to_vec(),
            result: Box::new(result),
        };
    }
    let substitutions = type_parameter_constraint_substitutions(type_parameters);
    Type::Function {
        parameters: parameters
            .iter()
            .map(|parameter| Parameter {
                annotation: parameter
                    .annotation
                    .as_ref()
                    .map(|annotation| substitute_type(annotation, &substitutions)),
                ..parameter.clone()
            })
            .collect(),
        result: Box::new(substitute_type(&result, &substitutions)),
    }
}
