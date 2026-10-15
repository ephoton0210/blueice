// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bind class paths recorded by the actual lowering, after exact replay.

use super::*;
use crate::lowering::classes::{AssignedFieldOrigin, ClassElementOrigin, ClassOrigins};
use blueice_bluets::{ClassMemberShell, Decorator};

impl Evidence<'_> {
    pub(super) fn class(&mut self, original: &ClassDeclaration, class: &bluejs::Class) {
        if self.error.is_some() {
            return;
        }
        let result = crate::lowering::classes::lower_class_with_origins(
            self.module,
            original,
            self.define_fields,
        );
        let (replayed, origins) = match result {
            Ok((bluejs::Stmt::ClassDecl(mut replayed), origins)) => {
                // Class-expression NamedEvaluation supplies this name outside
                // class lowering; its enclosing expression was already proven.
                replayed.name = class.name.clone();
                (replayed, origins)
            }
            Err(error) => {
                self.error = Some(error);
                return;
            }
            _ => {
                self.class_invalid();
                return;
            }
        };
        if &replayed != class || origins.elements.len() != class.elements.len() {
            self.class_invalid();
            return;
        }
        self.class_decorators(&original.decorators, &class.decorators);
        if let (Some(span), Some(base)) = (&original.extends_span, &class.extends) {
            if let Some(source) = self.module.source.get(span.start..span.end) {
                if let Ok(mut tokens) = lex(&self.module.id, source) {
                    tokens.pop();
                    for token in &mut tokens {
                        token.start += span.start;
                        token.end += span.start;
                    }
                    self.expression(base, &tokens);
                }
            }
        }
        for (element, origin) in class.elements.iter().zip(&origins.elements) {
            match origin {
                ClassElementOrigin::Generated => {}
                ClassElementOrigin::PendingKeys(members) => {
                    let bluejs::ClassElement::StaticBlock(body) = element else {
                        self.class_invalid();
                        return;
                    };
                    if body.len() != members.len() {
                        self.class_invalid();
                        return;
                    }
                    for (statement, index) in body.iter().zip(members) {
                        let bluejs::Stmt::Expr(expression) = statement else {
                            self.class_invalid();
                            return;
                        };
                        self.class_key_assignment(original, *index, expression);
                    }
                }
                ClassElementOrigin::Constructor => {
                    let bluejs::ClassElement::Method { function, .. } = element else {
                        self.class_invalid();
                        return;
                    };
                    self.class_field_assignments(
                        original,
                        &origins,
                        &function.body,
                        usize::from(original.extends_name.is_some()),
                    );
                }
                ClassElementOrigin::Member(origin) => {
                    let Some(member) = original.members.get(origin.member) else {
                        self.class_invalid();
                        return;
                    };
                    match element {
                        bluejs::ClassElement::Method {
                            key,
                            function,
                            decorators,
                            ..
                        } => {
                            self.class_key(original, member, key, &origin.key_prefixes);
                            self.class_decorators(&member.decorators, decorators);
                            if let Some(constructor) = &member.constructor {
                                let Some(body) = &constructor.body else {
                                    self.class_invalid();
                                    return;
                                };
                                self.parameters(&constructor.parameters, function);
                                if let Some(start) = origin.prologue_start {
                                    let at = constructor
                                        .prologue_insertion
                                        .map(|insertion| insertion.item_index);
                                    let Some(at) = at.filter(|at| *at <= body.len()) else {
                                        self.class_invalid();
                                        return;
                                    };
                                    let field_start =
                                        start + constructor.parameter_properties.len();
                                    let suffix = field_start + origins.assigned_fields.len();
                                    if start > function.body.len() || suffix > function.body.len() {
                                        self.class_invalid();
                                        return;
                                    }
                                    self.body(&body[..at], &function.body[..start]);
                                    self.class_field_assignments(
                                        original,
                                        &origins,
                                        &function.body,
                                        field_start,
                                    );
                                    self.body(&body[at..], &function.body[suffix..]);
                                } else {
                                    self.body(body, &function.body);
                                }
                            } else if let Some(method) = &member.method {
                                if let Some(body) = &method.body {
                                    self.function(&method.parameters, body, function);
                                }
                            } else {
                                self.class_invalid();
                                return;
                            }
                        }
                        bluejs::ClassElement::Accessor {
                            key,
                            function,
                            decorators,
                            ..
                        } => {
                            self.class_key(original, member, key, &origin.key_prefixes);
                            self.class_decorators(&member.decorators, decorators);
                            if let Some(accessor) = &member.accessor {
                                self.function(&accessor.parameters, &accessor.body, function);
                            } else {
                                self.class_invalid();
                                return;
                            }
                        }
                        bluejs::ClassElement::Field {
                            key,
                            initializer,
                            decorators,
                            ..
                        } => {
                            self.class_key(original, member, key, &origin.key_prefixes);
                            self.class_decorators(&member.decorators, decorators);
                            if let (Some(value), Some(tokens)) = (
                                initializer,
                                member
                                    .field
                                    .as_ref()
                                    .and_then(|field| field.initializer.as_deref()),
                            ) {
                                self.expression(value, tokens);
                            }
                        }
                        bluejs::ClassElement::StaticBlock(body) => {
                            if let Some(block) = &member.static_block {
                                self.body(&block.body, body);
                            } else if body.len() == 1 {
                                self.class_field_assignment(
                                    original,
                                    &AssignedFieldOrigin {
                                        member: origin.member,
                                        wrapped_class: origin.wrapped_initializer,
                                    },
                                    &body[0],
                                );
                            } else {
                                self.class_invalid();
                                return;
                            }
                        }
                    }
                }
            }
            if self.error.is_some() {
                return;
            }
        }
    }

    fn class_invalid(&mut self) {
        if self.error.is_none() {
            self.error = Some(invalid());
        }
    }

    fn class_decorators(&mut self, originals: &[Decorator], expressions: &[bluejs::Expr]) {
        if originals.len() != expressions.len() {
            self.class_invalid();
            return;
        }
        for (original, expression) in originals.iter().zip(expressions) {
            self.expression(expression, &original.tokens);
        }
    }

    fn class_key_tokens(member: &ClassMemberShell) -> &[Token] {
        if member.key.first().is_some_and(|token| token.is("["))
            && member.key.last().is_some_and(|token| token.is("]"))
        {
            &member.key[1..member.key.len() - 1]
        } else {
            &member.key
        }
    }

    fn class_key(
        &mut self,
        class: &ClassDeclaration,
        member: &ClassMemberShell,
        key: &bluejs::PropertyKey,
        prefixes: &[usize],
    ) {
        let bluejs::PropertyKey::Computed(expression) = key else {
            if !prefixes.is_empty() {
                self.class_invalid();
            }
            return;
        };
        if prefixes.is_empty() {
            self.expression(expression, Self::class_key_tokens(member));
        } else {
            let bluejs::Expr::Sequence(expressions) = expression.as_ref() else {
                self.class_invalid();
                return;
            };
            if expressions.len() != prefixes.len() + 1 {
                self.class_invalid();
                return;
            }
            for (expression, index) in expressions.iter().zip(prefixes) {
                self.class_key_assignment(class, *index, expression);
            }
            let written = expressions.last().unwrap();
            self.expression(written, Self::class_key_tokens(member));
            // Exact class replay proves this sequence runs pending erased
            // fields before the written key. ToPropertyKey consumes the
            // sequence result, so its owner needs the written key's certified
            // root origin. Prefix assignments and hidden bindings stay gaps.
            let written_key = super::key(DiagnosticSourceNode::Expression(written));
            if let Some(offset) = self
                .nodes
                .iter()
                .rfind(|(kind, address, _)| (*kind, *address) == written_key)
                .map(|(_, _, offset)| *offset)
            {
                self.bind(
                    DiagnosticSourceNode::Expression(expression),
                    &SourceSpan::new(&self.module.id, offset, offset),
                );
            }
        }
    }

    fn class_key_assignment(
        &mut self,
        class: &ClassDeclaration,
        index: usize,
        expression: &bluejs::Expr,
    ) {
        let Some(member) = class.members.get(index) else {
            self.class_invalid();
            return;
        };
        let bluejs::Expr::Assign { value, .. } = expression else {
            self.class_invalid();
            return;
        };
        self.expression(value, Self::class_key_tokens(member));
    }

    fn class_field_assignments(
        &mut self,
        class: &ClassDeclaration,
        origins: &ClassOrigins,
        body: &[bluejs::Stmt],
        start: usize,
    ) {
        let Some(statements) = body.get(start..start + origins.assigned_fields.len()) else {
            self.class_invalid();
            return;
        };
        for (statement, origin) in statements.iter().zip(&origins.assigned_fields) {
            self.class_field_assignment(class, origin, statement);
        }
    }

    fn class_field_assignment(
        &mut self,
        class: &ClassDeclaration,
        origin: &AssignedFieldOrigin,
        statement: &bluejs::Stmt,
    ) {
        let Some(tokens) = class
            .members
            .get(origin.member)
            .and_then(|member| member.field.as_ref())
            .and_then(|field| field.initializer.as_deref())
        else {
            self.class_invalid();
            return;
        };
        let bluejs::Stmt::Expr(bluejs::Expr::Assign { value, .. }) = statement else {
            self.class_invalid();
            return;
        };
        let value = if origin.wrapped_class {
            let bluejs::Expr::Member { object, .. } = value.as_ref() else {
                self.class_invalid();
                return;
            };
            let bluejs::Expr::Object(properties) = object.as_ref() else {
                self.class_invalid();
                return;
            };
            let [bluejs::ObjectProp::KeyValue { value, .. }] = properties.as_slice() else {
                self.class_invalid();
                return;
            };
            value
        } else {
            value
        };
        self.expression(value, tokens);
    }
}
