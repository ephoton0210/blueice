// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Checking of arrow functions parsed inside runtime expressions.
//!
//! An arrow is checked as an anonymous function whose body sees the scope it
//! appears in, so its parameters, result type and closed-over variables follow
//! the same rules as a named function. Its type is a function type built from
//! its parameters and result annotation.

use super::*;
use crate::parser::{ArrowBody, ArrowFunction};

impl ModuleChecker<'_> {
    /// Checks every outermost arrow function inside `tokens`. An arrow nested
    /// in another arrow's body is checked when that body is.
    pub(in crate::checker::module) fn check_arrow_functions_in(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
    ) {
        let (Some(first), Some(last)) = (tokens.first(), tokens.last()) else {
            return;
        };
        let module = self.module;
        let mut cursor = first.start;
        for (_, arrow) in module.arrow_functions.range(first.start..last.end) {
            if arrow.span.start < cursor || arrow.span.end > last.end {
                continue;
            }
            cursor = arrow.span.end;
            if !self.checked_arrows.insert(arrow.span.start) {
                continue;
            }
            self.check_arrow_function(arrow, scope);
        }
    }

    fn check_arrow_function(&mut self, arrow: &ArrowFunction, scope: &BTreeMap<String, Type>) {
        let (body, returns, locals) = match &arrow.body {
            ArrowBody::Expression(tokens) => (
                vec![FunctionBodyItem::Return {
                    tokens: tokens.clone(),
                    span: arrow.span.clone(),
                }],
                vec![tokens.clone()],
                Vec::new(),
            ),
            ArrowBody::Block {
                items,
                returns,
                locals,
            } => (items.clone(), returns.clone(), locals.clone()),
        };
        let function = FunctionDeclaration {
            name: "<arrow>".to_string(),
            async_function: false,
            body_open: None,
            type_parameters: Vec::new(),
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
        self.check_function_in_scope(&function, scope.clone());
    }

    /// The function type of `tokens` when they are exactly one arrow function.
    pub(in crate::checker::module) fn arrow_function_type(
        &self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
    ) -> Option<Type> {
        let (first, last) = (tokens.first()?, tokens.last()?);
        let arrow = self.module.arrow_functions.get(&first.start)?;
        if arrow.span.end != last.end {
            return None;
        }
        let result = match (&arrow.return_type, &arrow.body) {
            (Some(return_type), _) => return_type.clone(),
            (None, ArrowBody::Expression(body)) => {
                let mut inner = scope.clone();
                for parameter in &arrow.parameters {
                    inner.insert(
                        parameter.name.clone(),
                        parameter.annotation.clone().unwrap_or(Type::Unknown),
                    );
                }
                self.infer_expression(body, &inner)
            }
            (None, ArrowBody::Block { .. }) => Type::Unknown,
        };
        Some(Type::Function {
            parameters: arrow.parameters.clone(),
            result: Box::new(result),
        })
    }
}
