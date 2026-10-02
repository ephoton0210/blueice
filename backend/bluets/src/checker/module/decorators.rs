// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Checking of standard decorators (J.5.3).
//!
//! A decorator is called with the decorated value and a context object. BlueTS
//! has no `lib.decorators.d.ts`, so the context is not typed; what it checks
//! is what is decidable from the decorator's own declaration: it is callable,
//! it does not require more than the two arguments it is given, and what it
//! returns is a kind of value the decorated element accepts (nothing, or a
//! function for a method, accessor half, field initializer or replacement class;
//! an object for an auto-accessor). Placement is checked too: decorators decorate
//! classes and class members, and a method implementation, never a constructor,
//! a static block, an overload signature or a member the class parser does not
//! structure.

use super::*;
use crate::parser::{ClassDeclaration, ClassMemberKind, Decorator};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Class,
    Method,
    Getter,
    Setter,
    Field,
    Accessor,
}

impl Target {
    fn label(self) -> &'static str {
        match self {
            Self::Class => "class",
            Self::Method => "method",
            Self::Getter => "getter",
            Self::Setter => "setter",
            Self::Field => "field",
            Self::Accessor => "auto-accessor",
        }
    }
}

enum Callee {
    /// Not resolvable here: accepted.
    Unknown,
    NotCallable(Type),
    Function {
        parameters: Vec<Parameter>,
        result: Type,
    },
}

impl<'a> ModuleChecker<'a> {
    pub(super) fn check_class_decorators(&mut self, class: &ClassDeclaration) {
        let scope = self.values.clone();
        for decorator in &class.decorators {
            self.check_decorator(decorator, Target::Class, &scope);
        }
        for shell in &class.members {
            if shell.decorators.is_empty() {
                continue;
            }
            let first = &shell.decorators[0].span;
            let target = match shell.kind {
                ClassMemberKind::Constructor | ClassMemberKind::StaticBlock => {
                    self.type_error(
                        first,
                        "decorators are not valid here: they decorate a class or a class member"
                            .to_string(),
                        DiagnosticCode::InvalidDeclarationFile,
                    );
                    continue;
                }
                ClassMemberKind::Opaque => {
                    self.type_error(
                        first,
                        "decorators on this member shape (a computed or literal name, or a form BlueTS does not structure) are not supported"
                            .to_string(),
                        DiagnosticCode::UnsupportedSyntax,
                    );
                    continue;
                }
                ClassMemberKind::Method => {
                    if shell
                        .method
                        .as_ref()
                        .is_some_and(|method| method.body.is_none())
                    {
                        self.type_error(
                            first,
                            "a decorator can only decorate a method implementation, not an overload".to_string(),
                            DiagnosticCode::InvalidDeclarationFile,
                        );
                        continue;
                    }
                    Target::Method
                }
                ClassMemberKind::Accessor => {
                    if shell
                        .accessor
                        .as_ref()
                        .is_some_and(|accessor| accessor.getter)
                    {
                        Target::Getter
                    } else {
                        Target::Setter
                    }
                }
                ClassMemberKind::Field => {
                    if shell.field.as_ref().is_some_and(|field| field.accessor) {
                        Target::Accessor
                    } else {
                        Target::Field
                    }
                }
            };
            for decorator in &shell.decorators {
                self.check_decorator(decorator, target, &scope);
            }
        }
    }

    fn check_decorator(
        &mut self,
        decorator: &Decorator,
        target: Target,
        scope: &BTreeMap<String, Type>,
    ) {
        // The expression is ordinary code: its own errors are reported.
        self.check_direct_runtime_expression(&decorator.tokens, scope, &decorator.span);
        let callee = self.decorator_callee(&decorator.tokens, scope);
        match callee {
            Callee::Unknown => {}
            Callee::NotCallable(actual) => self.type_error(
                &decorator.span,
                format!(
                    "this decorator expression has type `{}`, which is not callable",
                    type_label(&actual)
                ),
                DiagnosticCode::TypeMismatch,
            ),
            Callee::Function { parameters, result } => {
                let required = parameters
                    .iter()
                    .filter(|parameter| {
                        !parameter.optional && !parameter.rest && parameter.default.is_none()
                    })
                    .count();
                if parameters.is_empty() {
                    self.type_error(
                        &decorator.span,
                        "this decorator accepts too few arguments to be used here; did you mean to call it first (`@name()`)?".to_string(),
                        DiagnosticCode::TypeMismatch,
                    );
                }
                if parameters.is_empty() {
                    self.type_error(
                        &decorator.span,
                        "this decorator accepts too few arguments to be used as a decorator; did you mean to call it first and write `@name()`?".to_string(),
                        DiagnosticCode::TypeMismatch,
                    );
                }
                if required > 2 {
                    self.type_error(
                        &decorator.span,
                        format!(
                            "a decorator is called with 2 arguments (the value and its context) but this one requires {required}"
                        ),
                        DiagnosticCode::TypeMismatch,
                    );
                }
                if let Some(bad) = self.invalid_decorator_result(&result, target) {
                    self.type_error(
                        &decorator.span,
                        format!(
                            "this decorator returns `{}`, which a {} decorator cannot return",
                            type_label(&bad),
                            target.label()
                        ),
                        DiagnosticCode::TypeMismatch,
                    );
                }
            }
        }
    }

    /// A part of `result` that the decorated element cannot accept.
    fn invalid_decorator_result(&self, result: &Type, target: Target) -> Option<Type> {
        let parts: Vec<&Type> = match result {
            Type::Union(parts) => parts.iter().collect(),
            other => vec![other],
        };
        parts.into_iter().find_map(|part| {
            let definitely_wrong = match part {
                Type::Number
                | Type::String
                | Type::Boolean
                | Type::Literal(_)
                | Type::Null
                | Type::Array(_)
                | Type::Tuple(_) => true,
                // An object is what an auto-accessor decorator returns; for every
                // other kind only a function (or nothing) is accepted.
                Type::Record(_) => target != Target::Accessor,
                Type::Function { .. } => target == Target::Accessor,
                _ => false,
            };
            definitely_wrong.then(|| part.clone())
        })
    }

    fn decorator_callee(&self, tokens: &[Token], scope: &BTreeMap<String, Type>) -> Callee {
        // `(expression)`.
        if tokens.first().is_some_and(|token| token.is("("))
            && tokens.last().is_some_and(|token| token.is(")"))
        {
            let inner = &tokens[1..tokens.len() - 1];
            return self.callee_of_value(self.infer_expression(inner, scope));
        }
        // `name(args)`: the decorator is what the call returns.
        if let Some(open) = tokens.iter().position(|token| token.is("(")) {
            let callee_tokens = &tokens[..open];
            if callee_tokens.len() == 1 && tokens.last().is_some_and(|token| token.is(")")) {
                return match self.named_callee(&callee_tokens[0].text, scope) {
                    Callee::Function { result, .. } => self.callee_of_value(result),
                    _ => Callee::Unknown,
                };
            }
            return Callee::Unknown;
        }
        if let [only] = tokens {
            return self.named_callee(&only.text, scope);
        }
        Callee::Unknown
    }

    fn named_callee(&self, name: &str, scope: &BTreeMap<String, Type>) -> Callee {
        if let Some(signature) = self
            .functions
            .get(name)
            .and_then(|signatures| signatures.first())
        {
            return Callee::Function {
                parameters: signature.parameters.clone(),
                result: signature.return_type.clone(),
            };
        }
        match scope.get(name).or_else(|| self.values.get(name)) {
            Some(value) => self.callee_of_value(value.clone()),
            None => Callee::Unknown,
        }
    }

    fn callee_of_value(&self, value: Type) -> Callee {
        match value {
            Type::Function { parameters, result } => Callee::Function {
                parameters,
                result: *result,
            },
            Type::Number
            | Type::String
            | Type::Boolean
            | Type::Literal(_)
            | Type::Null
            | Type::Array(_)
            | Type::Tuple(_) => Callee::NotCallable(value),
            _ => Callee::Unknown,
        }
    }
}
