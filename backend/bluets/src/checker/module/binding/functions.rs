// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Structured function-body checking and return-flow analysis.

use super::*;

mod narrowing;
use narrowing::{narrowed_guard_scopes, selected_guard_local};

impl<'a> ModuleChecker<'a> {
    pub(super) fn check_function(&mut self, function: &FunctionDeclaration) {
        self.check_function_in_scope(function, self.values.clone());
    }

    /// Checks a function whose body sees `scope`: the module values for a
    /// named function, or the enclosing scope for a function nested in an
    /// expression.
    pub(in crate::checker::module) fn check_function_in_scope(
        &mut self,
        function: &FunctionDeclaration,
        mut scope: BTreeMap<String, Type>,
    ) {
        let previous_parameters = self.type_parameters.clone();
        let previous_spreads = self.allowed_tuple_spread_parameters.clone();
        self.allowed_tuple_spread_parameters = function
            .type_parameters
            .iter()
            .filter(|parameter| {
                matches!(parameter.constraint, Some(Type::Array(_) | Type::Tuple(_)))
            })
            .map(|parameter| {
                (
                    parameter.name.clone(),
                    parameter.constraint.clone().expect("filtered constraint"),
                )
            })
            .collect();
        self.check_type_parameters(&function.type_parameters);
        for (index, parameter) in function.parameters.iter().enumerate() {
            if parameter.rest && index + 1 != function.parameters.len() {
                self.type_error(
                    &parameter.span,
                    "a rest parameter must be last".to_string(),
                    DiagnosticCode::TypeMismatch,
                );
            }
            if parameter.rest && parameter.optional {
                self.type_error(
                    &parameter.span,
                    "a rest parameter cannot be optional or have a default initializer".to_string(),
                    DiagnosticCode::TypeMismatch,
                );
            }
            if parameter.rest
                && parameter
                    .annotation
                    .as_ref()
                    .is_some_and(|annotation| !matches!(annotation, Type::Array(_)))
            {
                self.type_error(
                    &parameter.span,
                    "the bounded rest-parameter rule requires an array annotation".to_string(),
                    DiagnosticCode::TypeMismatch,
                );
            }
            if let Some(default) = &parameter.default {
                self.check_direct_runtime_expression(default, &scope, &parameter.span);
                let actual = self.infer_expression(default, &scope);
                let expected = parameter
                    .annotation
                    .as_ref()
                    .cloned()
                    .unwrap_or(Type::Unknown);
                if !self.is_assignable_bounded(&actual, &expected, &parameter.span) {
                    self.type_error(
                        &parameter.span,
                        format!(
                            "default initializer has type `{}`, which is not assignable to parameter `{}` of type `{}`",
                            type_label(&actual),
                            parameter.name,
                            type_label(&expected)
                        ),
                        DiagnosticCode::TypeMismatch,
                    );
                }
            }
            if let Some(annotation) = &parameter.annotation {
                self.check_type(annotation, &parameter.span);
                let parameter_type = if parameter.optional && parameter.default.is_none() {
                    Type::Union(vec![annotation.clone(), Type::Undefined])
                } else {
                    annotation.clone()
                };
                scope.insert(parameter.name.clone(), parameter_type);
            } else {
                scope.insert(parameter.name.clone(), Type::Unknown);
            }
        }
        for local in &function.locals {
            self.check_variable_in_scope(local, &scope);
            let inferred = local
                .annotation
                .clone()
                .unwrap_or_else(|| self.infer_expression(&local.initializer, &scope));
            scope.insert(local.name.clone(), inferred);
        }
        hoist_local_functions(&function.body, &mut scope);
        let selected_local = selected_guard_local(&function.body);
        self.check_function_body_expressions(&function.body, &scope, selected_local);
        let allows_implicit_undefined = if let Some(return_type) = &function.return_type {
            self.check_type(return_type, &function.span);
            self.return_type_allows_implicit_undefined(return_type, &function.span)
        } else {
            true
        };
        self.check_function_body_returns(
            &function.body,
            &scope,
            function.return_type.as_ref(),
            allows_implicit_undefined,
            &function.span,
            selected_local,
        );
        if let Some(return_type) = &function.return_type {
            if !function.declared
                && !function.overload
                && !allows_implicit_undefined
                && matches!(
                    Self::function_body_termination(&function.body),
                    StructuredTermination::FallsThrough
                )
            {
                self.type_error(
                    &function.span,
                    format!(
                        "function with return type `{}` can complete without returning a value",
                        type_label(return_type)
                    ),
                    DiagnosticCode::ReturnTypeMismatch,
                );
            }
        }
        self.type_parameters = previous_parameters;
        self.allowed_tuple_spread_parameters = previous_spreads;
    }

    /// Whether an explicit return annotation permits the JavaScript
    /// fall-through result. `void` is special in TypeScript return positions;
    /// the bounded assignability relation otherwise covers `undefined`,
    /// `any`, `unknown`, aliases, and unions containing one of those types.
    pub(super) fn return_type_allows_implicit_undefined(
        &mut self,
        return_type: &Type,
        span: &SourceSpan,
    ) -> bool {
        matches!(return_type, Type::Void)
            || self.is_assignable_bounded(&Type::Undefined, return_type, span)
    }

    /// Determines whether the structured function body cannot reach its end.
    ///
    /// The parser records unsupported syntax as `Opaque`, which must never be
    /// mistaken for a terminating branch. A recognized `return` (including an
    /// invalid bare return, diagnosed separately) or `throw` terminates its
    /// sequential path. An `if` does so only when both structured branches do.
    /// The unknown result preserves the standalone parser's existing opaque
    /// syntax behavior; the direct bridge rejects that syntax independently.
    pub(super) fn function_body_termination(items: &[FunctionBodyItem]) -> StructuredTermination {
        for item in items {
            match item {
                FunctionBodyItem::Return { .. } | FunctionBodyItem::Throw { .. } => {
                    return StructuredTermination::Terminates;
                }
                FunctionBodyItem::If(statement) => match Self::function_if_termination(statement) {
                    StructuredTermination::Terminates => {
                        return StructuredTermination::Terminates;
                    }
                    StructuredTermination::FallsThrough => {}
                    StructuredTermination::Opaque => return StructuredTermination::Opaque,
                },
                FunctionBodyItem::Opaque(_) => return StructuredTermination::Opaque,
                FunctionBodyItem::Variable(_)
                | FunctionBodyItem::Expression { .. }
                | FunctionBodyItem::Function(_)
                | FunctionBodyItem::While(_)
                | FunctionBodyItem::Try(_) => {}
            }
        }
        StructuredTermination::FallsThrough
    }

    fn function_if_termination(statement: &FunctionIfStatement) -> StructuredTermination {
        let consequent = Self::function_body_termination(&statement.consequent);
        let alternate = match &statement.alternate {
            Some(FunctionElseBranch::Braced(body)) => Self::function_body_termination(body),
            Some(FunctionElseBranch::ElseIf(branch)) => Self::function_if_termination(branch),
            None => StructuredTermination::FallsThrough,
        };
        match (consequent, alternate) {
            (StructuredTermination::Opaque, _) | (_, StructuredTermination::Opaque) => {
                StructuredTermination::Opaque
            }
            (StructuredTermination::Terminates, StructuredTermination::Terminates) => {
                StructuredTermination::Terminates
            }
            (StructuredTermination::FallsThrough, _) | (_, StructuredTermination::FallsThrough) => {
                StructuredTermination::FallsThrough
            }
        }
    }

    fn scope_after_guard(
        statement: &FunctionIfStatement,
        scope: &BTreeMap<String, Type>,
        selected_local: Option<&str>,
    ) -> BTreeMap<String, Type> {
        let Some((then_scope, else_scope)) =
            narrowed_guard_scopes(statement, scope, selected_local)
        else {
            return scope.clone();
        };
        let then_completion = Self::function_body_termination(&statement.consequent);
        let else_completion = match &statement.alternate {
            Some(FunctionElseBranch::Braced(body)) => Self::function_body_termination(body),
            None => StructuredTermination::FallsThrough,
            Some(FunctionElseBranch::ElseIf(_)) => return scope.clone(),
        };
        match (then_completion, else_completion) {
            (StructuredTermination::Terminates, StructuredTermination::FallsThrough) => else_scope,
            (StructuredTermination::FallsThrough, StructuredTermination::Terminates) => then_scope,
            _ => scope.clone(),
        }
    }

    /// Check structured returns in the lexical scope where they execute.
    /// The parser's flat `returns` list remains part of its public source
    /// representation, but cannot distinguish a catch-shadowed binding.
    fn check_function_body_returns(
        &mut self,
        items: &[FunctionBodyItem],
        scope: &BTreeMap<String, Type>,
        return_type: Option<&Type>,
        allows_implicit_undefined: bool,
        function_span: &SourceSpan,
        selected_local: Option<&str>,
    ) {
        let mut current_scope = scope.clone();
        let mut selected_local_is_declared = false;
        let mut guard_used = false;
        for item in items {
            match item {
                FunctionBodyItem::Return { tokens, .. } => self.check_function_return_tokens(
                    tokens,
                    &current_scope,
                    return_type,
                    allows_implicit_undefined,
                    function_span,
                ),
                FunctionBodyItem::If(statement) => {
                    let active_local =
                        selected_local.filter(|_| selected_local_is_declared && !guard_used);
                    guard_used |=
                        narrowed_guard_scopes(statement, &current_scope, active_local).is_some();
                    self.check_function_if_returns(
                        statement,
                        &current_scope,
                        return_type,
                        allows_implicit_undefined,
                        function_span,
                        active_local,
                    );
                    current_scope =
                        Self::scope_after_guard(statement, &current_scope, active_local);
                }
                FunctionBodyItem::While(statement) => self.check_function_body_returns(
                    &statement.body,
                    &current_scope,
                    return_type,
                    allows_implicit_undefined,
                    function_span,
                    None,
                ),
                FunctionBodyItem::Try(statement) => {
                    self.check_function_body_returns(
                        &statement.block,
                        &current_scope,
                        return_type,
                        allows_implicit_undefined,
                        function_span,
                        None,
                    );
                    if let Some(handler) = &statement.handler {
                        let catch_scope =
                            Self::catch_binding_scope(&current_scope, &handler.binding);
                        let previous_strictness = self.strict_catch_unknown;
                        self.strict_catch_unknown = true;
                        self.check_function_body_returns(
                            &handler.body,
                            &catch_scope,
                            return_type,
                            allows_implicit_undefined,
                            function_span,
                            None,
                        );
                        self.strict_catch_unknown = previous_strictness;
                    }
                    if let Some(finalizer) = &statement.finalizer {
                        self.check_function_body_returns(
                            finalizer,
                            &current_scope,
                            return_type,
                            allows_implicit_undefined,
                            function_span,
                            None,
                        );
                    }
                }
                FunctionBodyItem::Variable(variable) => {
                    if selected_local == Some(variable.name.as_str()) {
                        selected_local_is_declared = true;
                    }
                }
                // A nested function's returns belong to that function.
                FunctionBodyItem::Expression { .. }
                | FunctionBodyItem::Throw { .. }
                | FunctionBodyItem::Function(_)
                | FunctionBodyItem::Opaque(_) => {}
            }
        }
    }

    fn check_function_if_returns(
        &mut self,
        statement: &FunctionIfStatement,
        scope: &BTreeMap<String, Type>,
        return_type: Option<&Type>,
        allows_implicit_undefined: bool,
        function_span: &SourceSpan,
        selected_local: Option<&str>,
    ) {
        let (then_scope, else_scope) = narrowed_guard_scopes(statement, scope, selected_local)
            .unwrap_or_else(|| (scope.clone(), scope.clone()));
        self.check_function_body_returns(
            &statement.consequent,
            &then_scope,
            return_type,
            allows_implicit_undefined,
            function_span,
            None,
        );
        match &statement.alternate {
            Some(FunctionElseBranch::Braced(body)) => self.check_function_body_returns(
                body,
                &else_scope,
                return_type,
                allows_implicit_undefined,
                function_span,
                None,
            ),
            Some(FunctionElseBranch::ElseIf(branch)) => self.check_function_if_returns(
                branch,
                &else_scope,
                return_type,
                allows_implicit_undefined,
                function_span,
                None,
            ),
            None => {}
        }
    }

    fn check_function_return_tokens(
        &mut self,
        returned: &[Token],
        scope: &BTreeMap<String, Type>,
        return_type: Option<&Type>,
        allows_implicit_undefined: bool,
        function_span: &SourceSpan,
    ) {
        if returned.is_empty() {
            if let Some(return_type) = return_type.filter(|_| !allows_implicit_undefined) {
                self.type_error(
                    function_span,
                    format!(
                        "return expression has type `undefined`, which is not assignable to `{}`",
                        type_label(return_type)
                    ),
                    DiagnosticCode::ReturnTypeMismatch,
                );
            }
            return;
        }
        self.check_direct_runtime_expression(returned, scope, function_span);
        let Some(return_type) = return_type else {
            return;
        };
        let actual = self.infer_in_context(returned, scope, return_type);
        let return_is_assignable = (matches!(actual, Type::Undefined) && allows_implicit_undefined)
            || self.is_assignable_bounded(&actual, return_type, function_span);
        if !return_is_assignable {
            self.type_error(
                function_span,
                format!(
                    "return expression has type `{}`, which is not assignable to `{}`",
                    type_label(&actual),
                    type_label(return_type)
                ),
                DiagnosticCode::ReturnTypeMismatch,
            );
        }
    }

    fn catch_binding_scope(
        scope: &BTreeMap<String, Type>,
        binding: &str,
    ) -> BTreeMap<String, Type> {
        let mut catch_scope = scope.clone();
        catch_scope.insert(binding.to_string(), Type::Unknown);
        catch_scope
    }

    pub(super) fn check_function_body_expressions(
        &mut self,
        items: &[FunctionBodyItem],
        scope: &BTreeMap<String, Type>,
        selected_local: Option<&str>,
    ) {
        let mut current_scope = scope.clone();
        let mut selected_local_is_declared = false;
        let mut guard_used = false;
        for item in items {
            let (tokens, span) = match item {
                FunctionBodyItem::Expression { tokens, span }
                | FunctionBodyItem::Throw { tokens, span } => (tokens, span),
                FunctionBodyItem::Variable(variable) => {
                    if selected_local == Some(variable.name.as_str()) {
                        selected_local_is_declared = true;
                    }
                    continue;
                }
                FunctionBodyItem::Function(function) => {
                    self.check_function_in_scope(function, current_scope.clone());
                    continue;
                }
                FunctionBodyItem::If(statement) => {
                    let active_local =
                        selected_local.filter(|_| selected_local_is_declared && !guard_used);
                    guard_used |=
                        narrowed_guard_scopes(statement, &current_scope, active_local).is_some();
                    self.check_direct_function_if(statement, &current_scope, active_local);
                    current_scope =
                        Self::scope_after_guard(statement, &current_scope, active_local);
                    continue;
                }
                FunctionBodyItem::While(statement) => {
                    self.check_direct_function_while(statement, &current_scope);
                    continue;
                }
                FunctionBodyItem::Try(statement) => {
                    self.check_function_body_expressions(&statement.block, &current_scope, None);
                    if let Some(handler) = &statement.handler {
                        let catch_scope =
                            Self::catch_binding_scope(&current_scope, &handler.binding);
                        let previous_strictness = self.strict_catch_unknown;
                        self.strict_catch_unknown = true;
                        self.check_function_body_expressions(&handler.body, &catch_scope, None);
                        self.strict_catch_unknown = previous_strictness;
                    }
                    if let Some(finalizer) = &statement.finalizer {
                        self.check_function_body_expressions(finalizer, &current_scope, None);
                    }
                    continue;
                }
                _ => continue,
            };
            self.check_direct_runtime_expression(tokens, &current_scope, span);
        }
    }

    pub(super) fn check_direct_function_if(
        &mut self,
        statement: &FunctionIfStatement,
        scope: &BTreeMap<String, Type>,
        selected_local: Option<&str>,
    ) {
        self.check_direct_runtime_expression(&statement.test, scope, &statement.span);
        let (then_scope, else_scope) = narrowed_guard_scopes(statement, scope, selected_local)
            .unwrap_or_else(|| (scope.clone(), scope.clone()));
        self.check_function_body_expressions(&statement.consequent, &then_scope, None);
        match &statement.alternate {
            Some(FunctionElseBranch::Braced(alternate)) => {
                self.check_function_body_expressions(alternate, &else_scope, None);
            }
            Some(FunctionElseBranch::ElseIf(alternate)) => {
                self.check_direct_function_if(alternate, &else_scope, None);
            }
            None => {}
        }
    }

    pub(super) fn check_direct_function_while(
        &mut self,
        statement: &FunctionWhileStatement,
        scope: &BTreeMap<String, Type>,
    ) {
        self.check_direct_runtime_expression(&statement.test, scope, &statement.span);
        self.check_function_body_expressions(&statement.body, scope, None);
    }
}

/// Binds every function declared in `items`, at any block depth, to its
/// function type in `scope`. Declarations are hoisted, so a body may call one
/// before it appears, a declaration may call itself, and siblings may call each
/// other.
pub(in crate::checker::module) fn hoist_local_functions(
    items: &[FunctionBodyItem],
    scope: &mut BTreeMap<String, Type>,
) {
    for item in items {
        match item {
            FunctionBodyItem::Function(function) => {
                scope.insert(
                    function.name.clone(),
                    Type::Function {
                        parameters: function.parameters.clone(),
                        result: Box::new(function.return_type.clone().unwrap_or(Type::Unknown)),
                    },
                );
            }
            FunctionBodyItem::If(statement) => hoist_from_if(statement, scope),
            FunctionBodyItem::While(statement) => hoist_local_functions(&statement.body, scope),
            FunctionBodyItem::Try(statement) => {
                hoist_local_functions(&statement.block, scope);
                if let Some(handler) = &statement.handler {
                    hoist_local_functions(&handler.body, scope);
                }
                if let Some(finalizer) = &statement.finalizer {
                    hoist_local_functions(finalizer, scope);
                }
            }
            _ => {}
        }
    }
}

fn hoist_from_if(statement: &FunctionIfStatement, scope: &mut BTreeMap<String, Type>) {
    hoist_local_functions(&statement.consequent, scope);
    match &statement.alternate {
        Some(FunctionElseBranch::Braced(items)) => hoist_local_functions(items, scope),
        Some(FunctionElseBranch::ElseIf(next)) => hoist_from_if(next, scope),
        None => {}
    }
}
