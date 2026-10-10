// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Statement, class and binding productions retain their grammar context.

use super::*;
use SyntaxEdition::*;

impl<'a> Validator<'a> {
    pub(super) fn statement(
        &mut self,
        stmt: &'a Stmt,
        in_function: bool,
    ) -> Result<(), ParseError> {
        match stmt {
            Stmt::Empty | Stmt::Break(_) | Stmt::Continue(_) => {}
            Stmt::Expr(expr) | Stmt::Throw(expr) => self.expression(expr, in_function),
            Stmt::Block(body) => self.statements(body, in_function),
            Stmt::VarDecl(kind, declarations) => {
                self.declarations(*kind, declarations, in_function)?
            }
            Stmt::If {
                test,
                consequent,
                alternate,
            } => {
                self.expression(test, in_function);
                self.push(Node::Statement(consequent), in_function);
                if let Some(alternate) = alternate {
                    self.push(Node::Statement(alternate), in_function);
                }
            }
            Stmt::For {
                init,
                test,
                update,
                body,
            } => {
                match init {
                    Some(ForInit::VarDecl(kind, declarations)) => {
                        self.declarations(*kind, declarations, in_function)?
                    }
                    Some(ForInit::Expr(expr)) => self.expression(expr, in_function),
                    None => {}
                }
                self.optional_expression(test.as_ref(), in_function);
                self.optional_expression(update.as_ref(), in_function);
                self.push(Node::Statement(body), in_function);
            }
            Stmt::ForIn { left, right, body }
            | Stmt::ForOf {
                left, right, body, ..
            } => {
                if let Stmt::ForOf { is_await, .. } = stmt {
                    self.require(Es2015, "for-of statements")?;
                    if *is_await {
                        self.require(Es2018, "async iteration")?;
                        if !in_function {
                            self.require(Es2022, "top-level await")?;
                        }
                    }
                }
                self.for_head(left, in_function)?;
                self.expression(right, in_function);
                self.push(Node::Statement(body), in_function);
            }
            Stmt::While { test, body } | Stmt::DoWhile { test, body } => {
                self.expression(test, in_function);
                self.push(Node::Statement(body), in_function);
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.expression(discriminant, in_function);
                for case in cases {
                    self.optional_expression(case.test.as_ref(), in_function);
                    self.statements(&case.consequent, in_function);
                }
            }
            Stmt::Labelled { item, .. } => self.push(Node::Statement(item), in_function),
            Stmt::Return(expr) => self.optional_expression(expr.as_ref(), in_function),
            Stmt::Try {
                block,
                handler,
                finalizer,
            } => {
                self.statements(block, in_function);
                if let Some(handler) = handler {
                    if let Some(pattern) = &handler.param {
                        self.push(Node::Pattern(pattern), in_function);
                    } else {
                        self.require(Es2019, "optional catch bindings")?;
                    }
                    self.statements(&handler.body, in_function);
                }
                if let Some(body) = finalizer {
                    self.statements(body, in_function);
                }
            }
            Stmt::With { object, body } => {
                self.expression(object, in_function);
                self.push(Node::Statement(body), in_function);
            }
            Stmt::FunctionDecl(function) | Stmt::ModuleDefaultFunction { function, .. } => {
                self.push(Node::Function(function), in_function)
            }
            Stmt::ClassDecl(class) => self.push(Node::Class(class), in_function),
            Stmt::ClassField(stmt) | Stmt::ClassDecoratedField { field: stmt, .. } => {
                self.push(Node::Statement(stmt), in_function)
            }
            Stmt::ClassPrivateBrand(_) | Stmt::ClassExtraInitializers(_) => {}
        }
        Ok(())
    }

    fn declaration_kind(&self, kind: DeclKind) -> Result<(), ParseError> {
        match kind {
            DeclKind::Var => Ok(()),
            DeclKind::Let | DeclKind::Const => self.require(Es2015, "lexical declarations"),
            DeclKind::Using | DeclKind::AwaitUsing => self.require(EsNext, "resource declarations"),
        }
    }

    fn declarations(
        &mut self,
        kind: DeclKind,
        declarations: &'a [VarDeclarator],
        in_function: bool,
    ) -> Result<(), ParseError> {
        self.declaration_kind(kind)?;
        for declaration in declarations {
            self.push(Node::Pattern(&declaration.pattern), in_function);
            self.optional_expression(declaration.init.as_ref(), in_function);
        }
        Ok(())
    }

    fn for_head(&mut self, head: &'a ForHead, in_function: bool) -> Result<(), ParseError> {
        match head {
            ForHead::Decl(kind, pattern) => {
                self.declaration_kind(*kind)?;
                self.push(Node::Pattern(pattern), in_function);
            }
            ForHead::AnnexBVarInit(pattern, expr) => {
                self.push(Node::Pattern(pattern), in_function);
                self.expression(expr, in_function);
            }
            ForHead::Assignment(pattern) => {
                self.push(Node::AssignmentPattern(pattern), in_function)
            }
            ForHead::Expr(expr) => self.expression(expr, in_function),
        }
        Ok(())
    }

    pub(super) fn class(&mut self, class: &'a Class, in_function: bool) -> Result<(), ParseError> {
        self.require(Es2015, "classes")?;
        if !class.decorators.is_empty() {
            self.require(EsNext, "decorators")?;
        }
        self.optional_expression(class.extends.as_deref(), in_function);
        for element in &class.elements {
            match element {
                ClassElement::Method {
                    key,
                    function,
                    decorators,
                    ..
                }
                | ClassElement::Accessor {
                    key,
                    function,
                    decorators,
                    ..
                } => {
                    if !decorators.is_empty() {
                        self.require(EsNext, "decorators")?;
                    }
                    self.push(Node::Key(key), in_function);
                    self.push(Node::Function(function), in_function);
                }
                ClassElement::Field {
                    key,
                    initializer,
                    accessor,
                    decorators,
                    ..
                } => {
                    self.require(Es2022, "class fields")?;
                    if *accessor || !decorators.is_empty() {
                        self.require(EsNext, "decorators and auto-accessors")?;
                    }
                    self.push(Node::Key(key), in_function);
                    // A field initializer is a separate function context.
                    self.optional_expression(initializer.as_ref(), true);
                }
                ClassElement::StaticBlock(body) => {
                    self.require(Es2022, "class static blocks")?;
                    self.statements(body, true);
                }
            }
        }
        Ok(())
    }

    pub(super) fn pattern(
        &mut self,
        pattern: &'a Pattern,
        in_function: bool,
    ) -> Result<(), ParseError> {
        match pattern {
            Pattern::Identifier(_) => {}
            Pattern::Array(elements) => {
                self.require(Es2015, "array binding patterns")?;
                for element in elements.iter().flatten() {
                    self.push(Node::Pattern(&element.pattern), in_function);
                    self.optional_expression(element.default.as_ref(), in_function);
                }
            }
            Pattern::Object(properties) => {
                self.require(Es2015, "object binding patterns")?;
                for property in properties {
                    match property {
                        ObjectPatternProp::KeyValue {
                            key,
                            value,
                            default,
                        } => {
                            self.push(Node::Key(key), in_function);
                            self.push(Node::Pattern(value), in_function);
                            self.optional_expression(default.as_ref(), in_function);
                        }
                        ObjectPatternProp::Rest(pattern) => {
                            self.require(Es2018, "object rest bindings")?;
                            self.push(Node::Pattern(pattern), in_function);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub(super) fn assignment_pattern(
        &mut self,
        pattern: &'a AssignmentPattern,
        in_function: bool,
    ) -> Result<(), ParseError> {
        match pattern {
            AssignmentPattern::Target(expr) => self.expression(expr, in_function),
            AssignmentPattern::Array(elements) => {
                self.require(Es2015, "array assignment patterns")?;
                for element in elements.iter().flatten() {
                    self.push(Node::AssignmentPattern(&element.pattern), in_function);
                    self.optional_expression(element.default.as_ref(), in_function);
                }
            }
            AssignmentPattern::Object(properties) => {
                self.require(Es2015, "object assignment patterns")?;
                for property in properties {
                    match property {
                        AssignmentPatternProp::KeyValue {
                            key,
                            value,
                            default,
                        } => {
                            self.push(Node::Key(key), in_function);
                            self.push(Node::AssignmentPattern(value), in_function);
                            self.optional_expression(default.as_ref(), in_function);
                        }
                        AssignmentPatternProp::Rest(pattern) => {
                            self.require(Es2018, "object rest assignments")?;
                            self.push(Node::AssignmentPattern(pattern), in_function);
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
