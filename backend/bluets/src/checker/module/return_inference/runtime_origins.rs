// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! An explicit result annotation cannot discard runtime-origin evidence.

use super::*;
use crate::parser::RuntimeQualifiers;

impl ModuleChecker<'_> {
    /// This query has no semantic-result cache: an annotated body must never
    /// replace the declaration result or reuse provisional semantic inference.
    pub(in crate::checker::module) fn annotated_runtime_result(
        &self,
        annotation: &Type,
        parameters: &[Parameter],
        body: &[FunctionBodyItem],
        outer: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) -> Type {
        let conservative = RuntimeQualifiers {
            opaque: true,
            may_readonly: true,
        };
        {
            let active = self.return_inference.runtime_active.borrow();
            if active.contains(&span.start) {
                return annotation.clone().with_runtime_qualifiers(conservative);
            }
            if active.len() >= self.max_type_expansions {
                self.generic_inference_failure.set(Some(span.clone()));
                return annotation.clone().with_runtime_qualifiers(conservative);
            }
        }
        self.return_inference
            .runtime_active
            .borrow_mut()
            .push(span.start);
        let mut scope = outer.clone();
        for parameter in self.return_parameters(parameters, outer) {
            let value = parameter.annotation.clone().unwrap_or(Type::Unknown);
            let value = self.scopes.as_ref().map_or_else(
                || value.clone(),
                |scopes| scopes.parameter_type_at(&value, parameter.span.start),
            );
            if let Some(pattern) = &parameter.pattern {
                self.infer_return_pattern(pattern, &value, &mut scope);
            } else {
                scope.insert(parameter.name, value);
            }
        }
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        let qualifiers = self.body_runtime_origins(body, &mut scope, &mut budget, annotation);
        self.return_inference.runtime_active.borrow_mut().pop();
        if budget.exhausted {
            self.generic_inference_failure.set(Some(span.clone()));
        }
        annotation.clone().with_runtime_qualifiers(qualifiers)
    }

    fn body_runtime_origins(
        &self,
        body: &[FunctionBodyItem],
        scope: &mut BTreeMap<String, Type>,
        budget: &mut TypeExpansionBudget,
        annotation: &Type,
    ) -> RuntimeQualifiers {
        let mut result = RuntimeQualifiers::default();
        for item in body {
            if !budget.consume() {
                return result.join(RuntimeQualifiers {
                    opaque: true,
                    may_readonly: true,
                });
            }
            match item {
                FunctionBodyItem::Return { tokens, .. } => {
                    let value = self.infer_expression(tokens, scope);
                    let qualifiers = if value.semantic_type() == annotation.semantic_type() {
                        value.runtime_qualifiers()
                    } else {
                        self.runtime_origin_qualifiers(&value, budget)
                            .unwrap_or_else(|()| {
                                budget.exhausted = true;
                                RuntimeQualifiers {
                                    opaque: true,
                                    may_readonly: true,
                                }
                            })
                    };
                    result = result.join(qualifiers);
                }
                FunctionBodyItem::Variable(variable) => {
                    let value = self.infer_expression(&variable.initializer, scope);
                    let value = variable.annotation.as_ref().map_or_else(
                        || value.clone(),
                        |annotation| annotation.clone().retaining_runtime_qualifiers(&value),
                    );
                    scope.insert(variable.name.clone(), value);
                }
                FunctionBodyItem::If(statement) => {
                    result = result.join(self.body_runtime_origins(
                        &statement.consequent,
                        &mut scope.clone(),
                        budget,
                        annotation,
                    ));
                    let alternate = match &statement.alternate {
                        Some(FunctionElseBranch::Braced(body)) => {
                            self.body_runtime_origins(body, &mut scope.clone(), budget, annotation)
                        }
                        Some(FunctionElseBranch::ElseIf(branch)) => self.body_runtime_origins(
                            &[FunctionBodyItem::If((**branch).clone())],
                            &mut scope.clone(),
                            budget,
                            annotation,
                        ),
                        None => RuntimeQualifiers::default(),
                    };
                    result = result.join(alternate);
                }
                FunctionBodyItem::While(statement) => {
                    result = result.join(self.body_runtime_origins(
                        &statement.body,
                        &mut scope.clone(),
                        budget,
                        annotation,
                    ));
                }
                FunctionBodyItem::Try(statement) => {
                    result = result.join(self.body_runtime_origins(
                        &statement.block,
                        &mut scope.clone(),
                        budget,
                        annotation,
                    ));
                    if let Some(handler) = &statement.handler {
                        let mut caught = scope.clone();
                        caught.insert(
                            handler.binding.clone(),
                            handler.annotation.clone().unwrap_or(Type::Unknown),
                        );
                        result = result.join(self.body_runtime_origins(
                            &handler.body,
                            &mut caught,
                            budget,
                            annotation,
                        ));
                    }
                    if let Some(finalizer) = &statement.finalizer {
                        result = result.join(self.body_runtime_origins(
                            finalizer,
                            &mut scope.clone(),
                            budget,
                            annotation,
                        ));
                    }
                }
                FunctionBodyItem::Function(_)
                | FunctionBodyItem::Expression { .. }
                | FunctionBodyItem::Throw { .. }
                | FunctionBodyItem::TypeAlias(_)
                | FunctionBodyItem::Opaque(_) => {}
            }
        }
        result
    }
}
