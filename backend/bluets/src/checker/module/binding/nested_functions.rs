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

    /// `await` is only valid inside an `async` function. Tokens inside a
    /// structured nested function are checked in that function's own context,
    /// and an expression holding an `async` token may contain an `await` in an
    /// async form the parser left unstructured, so it is not judged here.
    pub(in crate::checker::module) fn check_await_context(
        &mut self,
        tokens: &[Token],
        span: &SourceSpan,
    ) {
        if self.async_context == Some(true) || !tokens.iter().any(|token| token.is("await")) {
            return;
        }
        let module = self.module;
        // An `async` token that starts a structured nested function is judged
        // in that function's own context; any other may begin an async form the
        // parser left unstructured, which can hold an `await`.
        if tokens
            .iter()
            .any(|token| token.is("async") && !module.nested_functions.contains_key(&token.start))
        {
            return;
        }
        let range_start = tokens.first().map_or(0, |token| token.start);
        // Only a function that starts inside the checked tokens is nested in
        // them; one that encloses them is the function being checked.
        let inside_nested = |offset: usize| {
            module
                .nested_functions
                .range(range_start..=offset)
                .next_back()
                .is_some_and(|(_, nested)| offset < nested.span.end)
        };
        let Some(token) = tokens
            .iter()
            .find(|token| token.is("await") && !inside_nested(token.start))
        else {
            return;
        };
        let at = SourceSpan::new(&span.module, token.start, token.end);
        match self.async_context {
            // The top level of a module may await; the top level of a script may
            // not, nor may a namespace's body, which is a function that is not async.
            None if self.namespace_path.is_empty()
                && self.module_kind == crate::compiler::ModuleKind::CommonJs =>
            {
                self.type_error(
                    &at,
                    "top-level `await` is not allowed in a CommonJS module".to_string(),
                    DiagnosticCode::TypeMismatch,
                )
            }
            None if self.namespace_path.is_empty() && self.module_has_module_syntax() => {}
            None if self.namespace_path.is_empty() => self.type_error(
                &at,
                "`await` at the top level of a file requires a module: add an import or an \
                 export (`export {}`)"
                    .to_string(),
                DiagnosticCode::TypeMismatch,
            ),
            _ => self.type_error(
                &at,
                "`await` is only valid inside an async function".to_string(),
                DiagnosticCode::TypeMismatch,
            ),
        }
    }

    /// Whether the module has an `import` or `export`, which is what makes a file
    /// a module and lets its top level await.
    fn module_has_module_syntax(&self) -> bool {
        self.module
            .declarations
            .iter()
            .any(|declaration| match declaration {
                Declaration::Import(_)
                | Declaration::TypeExport(_)
                | Declaration::DefaultExport(_)
                | Declaration::ValueExport(_) => true,
                Declaration::Variable(item) => item.exported,
                Declaration::Function(item) => item.exported,
                Declaration::Class(item) => item.exported,
                Declaration::Enum(item) => item.exported,
                Declaration::Interface(item) => item.exported,
                Declaration::TypeAlias(item) => item.exported,
                Declaration::Namespace(item) => item.exported,
                Declaration::Ambient(_) | Declaration::Raw(_) | Declaration::UmdExport(_) => false,
            })
    }

    fn check_nested_function(&mut self, arrow: &NestedFunction, scope: &BTreeMap<String, Type>) {
        if self.target == crate::EcmaTarget::Es5
            && arrow.kind == crate::parser::NestedFunctionKind::Arrow
        {
            let reads = self
                .scopes
                .as_ref()
                .map(|scopes| scopes.arguments_reads_at(arrow.span.start).to_vec())
                .unwrap_or_default();
            for span in reads {
                self.typescript_type_error(
                    &span,
                    "The 'arguments' object cannot be referenced in an arrow function in ES5. Consider using a standard function expression.".to_string(),
                    DiagnosticCode::UnsupportedSyntax,
                    2496,
                    Vec::new(),
                );
            }
        }
        if let (Some(first), Some(last)) = (arrow.computed_key.first(), arrow.computed_key.last()) {
            self.check_direct_runtime_expression(
                &arrow.computed_key,
                scope,
                &SourceSpan::new(&self.module.id, first.start, last.end),
            );
        }
        // A nested function is not the constructor: it may not assign readonly
        // fields even when it is written inside one.
        let constructor_readonly = self.constructor_readonly_fields.take();
        self.check_nested_function_body(arrow, scope);
        self.constructor_readonly_fields = constructor_readonly;
    }

    fn check_nested_function_body(
        &mut self,
        arrow: &NestedFunction,
        scope: &BTreeMap<String, Type>,
    ) {
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
            async_function: arrow.async_function,
            generator: arrow.generator,
            body_open: None,
            type_parameters: arrow.type_parameters.clone(),
            parameters: arrow.parameters.clone(),
            return_type: arrow.return_type.clone(),
            body,
            returns,
            locals,
            exported: false,
            default_export: false,
            anonymous: false,
            declared: false,
            overload: false,
            span: arrow.span.clone(),
        };
        let mut body_scope = scope.clone();
        if arrow.contextual_this {
            body_scope.insert("this".to_string(), Type::Unknown);
        } else if arrow.kind != crate::parser::NestedFunctionKind::Arrow {
            body_scope.remove("this");
        }
        if let Some(name) = &arrow.name {
            // A named function expression is visible inside its own body.
            body_scope.insert(
                name.clone(),
                declared_function_type(
                    &arrow.parameters,
                    async_result(
                        arrow.return_type.clone().unwrap_or(Type::Unknown),
                        arrow.async_function && arrow.return_type.is_none(),
                    ),
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
        let result = self.inferred_nested_result(arrow, scope);
        let parameters = self.return_parameters(&arrow.parameters, scope);
        Some(declared_function_type(
            &parameters,
            result,
            &arrow.type_parameters,
        ))
    }
}

/// The result of an unannotated `async` function is a promise of its body's
/// result; an annotated one already states `Promise<T>`.
pub(in crate::checker::module) fn async_result(result: Type, wrap: bool) -> Type {
    if wrap {
        Type::Named {
            name: "Promise".to_string(),
            arguments: vec![result],
        }
    } else {
        result
    }
}

/// The function type of a nested declaration, retaining generic binders for
/// contextual inference and instantiation at each call site.
pub(in crate::checker::module) fn declared_function_type(
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
    Type::GenericFunction {
        type_parameters: type_parameters.to_vec(),
        parameters: parameters.to_vec(),
        result: Box::new(result),
        span: type_parameters
            .first()
            .expect("generic function parameter")
            .span
            .clone(),
    }
}
