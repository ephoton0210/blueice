// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Completion paths used by return inference, including known nonreturning calls.

use super::*;

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn inference_body_completes(
        &self,
        body: &[FunctionBodyItem],
        outer: &BTreeMap<String, Type>,
    ) -> bool {
        if matches!(
            Self::function_body_termination(body),
            binding::StructuredTermination::Terminates
        ) {
            return false;
        }
        let mut scope = outer.clone();
        for item in body {
            match item {
                FunctionBodyItem::Return { .. } | FunctionBodyItem::Throw { .. } => return false,
                FunctionBodyItem::Expression { tokens, .. } => {
                    if self.infer_expression(tokens, &scope) == Type::Never {
                        return false;
                    }
                }
                FunctionBodyItem::Variable(variable) => {
                    let value = self.infer_expression(&variable.initializer, &scope);
                    if value == Type::Never {
                        return false;
                    }
                    scope.insert(
                        variable.name.clone(),
                        variable.annotation.clone().unwrap_or(value),
                    );
                }
                FunctionBodyItem::While(statement) => {
                    if matches!(strip_outer_parentheses(&statement.test), [test] if test.is("true"))
                        && !opaque(&statement.body)
                    {
                        return false;
                    }
                }
                FunctionBodyItem::If(statement) => {
                    let consequent = self.inference_body_completes(&statement.consequent, &scope);
                    let alternate = match &statement.alternate {
                        Some(FunctionElseBranch::Braced(body)) => {
                            self.inference_body_completes(body, &scope)
                        }
                        Some(FunctionElseBranch::ElseIf(branch)) => self.inference_body_completes(
                            &[FunctionBodyItem::If((**branch).clone())],
                            &scope,
                        ),
                        None => true,
                    };
                    let completes = match strip_outer_parentheses(&statement.test) {
                        [test] if test.is("true") => consequent,
                        [test] if test.is("false") => alternate,
                        _ => consequent || alternate,
                    };
                    if !completes {
                        return false;
                    }
                }
                FunctionBodyItem::Try(statement) => {
                    if statement
                        .finalizer
                        .as_ref()
                        .is_some_and(|body| !self.inference_body_completes(body, &scope))
                    {
                        return false;
                    }
                    let block = self.inference_body_completes(&statement.block, &scope);
                    let handler = statement
                        .handler
                        .as_ref()
                        .map(|handler| self.inference_body_completes(&handler.body, &scope));
                    if !block && handler.is_none_or(|completes| !completes) {
                        return false;
                    }
                }
                FunctionBodyItem::Opaque(_) => return true,
                FunctionBodyItem::Function(_) => {}
            }
        }
        true
    }
}

fn opaque(body: &[FunctionBodyItem]) -> bool {
    body.iter().any(|item| match item {
        FunctionBodyItem::Opaque(_) => true,
        FunctionBodyItem::If(statement) => {
            opaque(&statement.consequent)
                || match &statement.alternate {
                    Some(FunctionElseBranch::Braced(body)) => opaque(body),
                    Some(FunctionElseBranch::ElseIf(branch)) => {
                        opaque(&[FunctionBodyItem::If((**branch).clone())])
                    }
                    None => false,
                }
        }
        FunctionBodyItem::While(statement) => opaque(&statement.body),
        FunctionBodyItem::Try(statement) => {
            opaque(&statement.block)
                || statement
                    .handler
                    .as_ref()
                    .is_some_and(|handler| opaque(&handler.body))
                || statement
                    .finalizer
                    .as_ref()
                    .is_some_and(|body| opaque(body))
        }
        _ => false,
    })
}

pub(in crate::checker::module) fn has_return(body: &[FunctionBodyItem]) -> bool {
    body.iter().any(|item| match item {
        FunctionBodyItem::Return { .. } => true,
        FunctionBodyItem::If(statement) => {
            has_return(&statement.consequent)
                || match &statement.alternate {
                    Some(FunctionElseBranch::Braced(body)) => has_return(body),
                    Some(FunctionElseBranch::ElseIf(branch)) => {
                        has_return(&[FunctionBodyItem::If((**branch).clone())])
                    }
                    None => false,
                }
        }
        FunctionBodyItem::While(statement) => has_return(&statement.body),
        FunctionBodyItem::Try(statement) => {
            has_return(&statement.block)
                || statement
                    .handler
                    .as_ref()
                    .is_some_and(|handler| has_return(&handler.body))
                || statement
                    .finalizer
                    .as_ref()
                    .is_some_and(|body| has_return(body))
        }
        _ => false,
    })
}
