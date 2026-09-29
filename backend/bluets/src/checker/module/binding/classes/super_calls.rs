// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Derived-constructor `super` call checks.
//!
//! A derived class constructor must contain a `super(...)` call, may not read
//! `this` before it, and must supply arguments accepted by one of the base
//! constructor's signatures. A `super(...)` call in a class with no base is
//! rejected. Calls inside nested functions or arrows are not searched; a
//! statement containing one is left unchecked for `this` ordering.

use super::*;
use crate::parser::{FunctionElseBranch, FunctionIfStatement};

/// Every expression-bearing statement of a body in source order, descending
/// into braced blocks.
fn statements<'a>(items: &'a [FunctionBodyItem], out: &mut Vec<(&'a [Token], &'a SourceSpan)>) {
    for item in items {
        match item {
            FunctionBodyItem::Variable(variable) => {
                out.push((&variable.initializer, &variable.span))
            }
            FunctionBodyItem::Expression { tokens, span }
            | FunctionBodyItem::Throw { tokens, span }
            | FunctionBodyItem::Return { tokens, span } => out.push((tokens, span)),
            FunctionBodyItem::If(statement) => if_statements(statement, out),
            FunctionBodyItem::While(statement) => {
                out.push((&statement.test, &statement.span));
                statements(&statement.body, out);
            }
            FunctionBodyItem::Try(statement) => {
                statements(&statement.block, out);
                if let Some(handler) = &statement.handler {
                    statements(&handler.body, out);
                }
                if let Some(finalizer) = &statement.finalizer {
                    statements(finalizer, out);
                }
            }
            FunctionBodyItem::Opaque(_) => {}
        }
    }
}

fn if_statements<'a>(
    statement: &'a FunctionIfStatement,
    out: &mut Vec<(&'a [Token], &'a SourceSpan)>,
) {
    out.push((&statement.test, &statement.span));
    statements(&statement.consequent, out);
    match &statement.alternate {
        Some(FunctionElseBranch::Braced(items)) => statements(items, out),
        Some(FunctionElseBranch::ElseIf(next)) => if_statements(next, out),
        None => {}
    }
}

fn contains_super_call(tokens: &[Token]) -> bool {
    tokens
        .windows(2)
        .any(|pair| pair[0].is("super") && pair[1].is("("))
}

fn is_deferred_this_context(tokens: &[Token]) -> bool {
    tokens
        .iter()
        .any(|token| token.is("=>") || token.is("function"))
}

impl ModuleChecker<'_> {
    /// The type `super` has in a class body: the base instance type for
    /// constructors and instance methods, the base constructor side for
    /// static methods. `None` when there is no bound base.
    pub(super) fn super_scope_type(
        &self,
        class: &ClassDeclaration,
        is_static: bool,
    ) -> Option<Type> {
        let base = class.extends_name.as_ref()?;
        if !self.class_constructors.contains_key(base) {
            return None;
        }
        if is_static {
            self.values.get(base).cloned()
        } else {
            Some(Type::Named {
                name: base.clone(),
                arguments: Vec::new(),
            })
        }
    }

    /// `super` is only valid in a derived class, and `super(...)` only in its
    /// constructor.
    pub(super) fn check_method_super_placement(
        &mut self,
        class: &ClassDeclaration,
        body: &[FunctionBodyItem],
    ) {
        let mut flat = Vec::new();
        statements(body, &mut flat);
        for (tokens, span) in flat {
            if contains_super_call(tokens) {
                self.type_error(
                    span,
                    "a `super` call is only permitted in a constructor".to_string(),
                    DiagnosticCode::TypeMismatch,
                );
                return;
            }
            if class.extends_name.is_none() && tokens.iter().any(|token| token.is("super")) {
                self.type_error(
                    span,
                    format!(
                        "`super` can only be referenced in a derived class, and `{}` has no base",
                        class.name
                    ),
                    DiagnosticCode::TypeMismatch,
                );
                return;
            }
        }
    }

    /// Presence and ordering checks for one constructor body.
    pub(super) fn check_constructor_super_placement(
        &mut self,
        class: &ClassDeclaration,
        constructor: &ClassConstructor,
        body: &[FunctionBodyItem],
    ) {
        let mut flat = Vec::new();
        statements(body, &mut flat);
        let Some(base) = &class.extends_name else {
            if let Some((_, span)) = flat.iter().find(|(tokens, _)| contains_super_call(tokens)) {
                self.type_error(
                    span,
                    format!(
                        "`super` can only be called in the constructor of a derived class, and `{}` has no base",
                        class.name
                    ),
                    DiagnosticCode::TypeMismatch,
                );
            }
            return;
        };
        // An unbound base already has a heritage diagnostic.
        if !self.class_constructors.contains_key(base) {
            return;
        }
        let first_super = flat
            .iter()
            .position(|(tokens, _)| contains_super_call(tokens));
        let Some(first_super) = first_super else {
            self.type_error(
                &constructor.span,
                format!(
                    "constructor of derived class `{}` must contain a `super` call",
                    class.name
                ),
                DiagnosticCode::TypeMismatch,
            );
            return;
        };
        let before_super = flat[..first_super].iter().copied().chain(std::iter::once((
            // Arguments of the first call itself also precede the call.
            flat[first_super]
                .0
                .split_at(2.min(flat[first_super].0.len()))
                .1,
            flat[first_super].1,
        )));
        for (tokens, span) in before_super {
            if !is_deferred_this_context(tokens) && tokens.iter().any(|token| token.is("this")) {
                self.type_error(
                    span,
                    "`super` must be called before `this` is accessed in a derived constructor"
                        .to_string(),
                    DiagnosticCode::TypeMismatch,
                );
                return;
            }
        }
    }

    /// Argument check for a top-level `super(...)` statement.
    pub(super) fn check_super_call_arguments(
        &mut self,
        base: &str,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) {
        let tokens = match tokens.split_last() {
            Some((last, rest)) if last.is(";") => rest,
            _ => tokens,
        };
        let [keyword, opening, arguments @ ..] = tokens else {
            return;
        };
        if !keyword.is("super") || !opening.is("(") {
            return;
        }
        let Some(binding) = self.class_constructors.get(base) else {
            return;
        };
        // The base signature of an omitted derived constructor is unresolved;
        // its heritage diagnostic prevents an invented check here.
        if binding.inherited {
            return;
        }
        let Some(arguments) = split_call_arguments(arguments) else {
            return;
        };
        let signatures = binding.signatures.clone();
        if let Some(alternatives) = self.optional_spread_scopes(&arguments, scope) {
            let before = self.diagnostics.len();
            for alternative in &alternatives {
                self.check_super_call_arguments(base, tokens, alternative, span);
            }
            self.dedupe_diagnostics_since(before);
            return;
        }
        let Ok(actuals) = self.expanded_call_argument_types_for(&arguments, scope, &signatures)
        else {
            self.type_error(
                span,
                "a `super` call spread must have a fixed-length tuple type".to_string(),
                DiagnosticCode::TypeMismatch,
            );
            return;
        };
        match self.select_function_signature(&signatures, &actuals, None) {
            Ok(Some(_)) => {}
            Ok(None) => self.type_error(
                span,
                format!("no constructor of class {base} accepts the `super` call arguments"),
                DiagnosticCode::TypeMismatch,
            ),
            Err(()) => self.type_error(
                span,
                format!(
                    "`super` call selection exceeds the {} generic-expansion limit",
                    self.max_type_expansions
                ),
                DiagnosticCode::ResourceLimit,
            ),
        }
    }
}
