// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Deterministic executable-AST inventory used by `bluejs-program-debug-v1`.

use super::{AstNodeDescriptor, BlueJsAstNodeKind};
use crate::{
    Argument, ArrayElement, ArrowBody, AssignmentPattern, AssignmentPatternElement,
    AssignmentPatternProp, BlueJsProgramV1, CatchClause, Class, ClassElement, Expr, ForHead,
    ForInit, Function, ObjectPatternProp, ObjectProp, Param, Pattern, PropertyKey, Stmt,
};

pub(super) fn collect_ast_nodes(program: &BlueJsProgramV1) -> Vec<AstNodeDescriptor> {
    let mut visitor = AstNodeVisitor { nodes: Vec::new() };
    match program {
        BlueJsProgramV1::Script(program) => {
            visitor.push(BlueJsAstNodeKind::Script, false);
            visitor.statements(&program.body, true);
        }
        BlueJsProgramV1::Module(module) => {
            visitor.push(BlueJsAstNodeKind::Module, false);
            visitor.statements(&module.body, true);
        }
    }
    visitor.nodes
}

struct AstNodeVisitor {
    nodes: Vec<AstNodeDescriptor>,
}

impl AstNodeVisitor {
    fn push(&mut self, kind: BlueJsAstNodeKind, top_level_statement: bool) {
        self.nodes.push(AstNodeDescriptor {
            kind,
            top_level_statement,
        });
    }

    fn statements(&mut self, statements: &[Stmt], top_level: bool) {
        for statement in statements {
            self.statement(statement, top_level);
        }
    }

    fn statement(&mut self, statement: &Stmt, top_level: bool) {
        self.push(BlueJsAstNodeKind::Statement, top_level);
        match statement {
            Stmt::Empty | Stmt::Break(_) | Stmt::Continue(_) | Stmt::ClassPrivateBrand(_) => {}
            Stmt::Expr(expression) | Stmt::Throw(expression) => self.expression(expression),
            Stmt::Block(statements) => self.statements(statements, false),
            Stmt::VarDecl(_, declarations) => {
                for declaration in declarations {
                    self.pattern(&declaration.pattern);
                    if let Some(initializer) = &declaration.init {
                        self.expression(initializer);
                    }
                }
            }
            Stmt::If {
                test,
                consequent,
                alternate,
            } => {
                self.expression(test);
                self.statement(consequent, false);
                if let Some(alternate) = alternate {
                    self.statement(alternate, false);
                }
            }
            Stmt::For {
                init,
                test,
                update,
                body,
            } => {
                if let Some(init) = init {
                    self.for_init(init);
                }
                if let Some(test) = test {
                    self.expression(test);
                }
                if let Some(update) = update {
                    self.expression(update);
                }
                self.statement(body, false);
            }
            Stmt::ForIn { left, right, body }
            | Stmt::ForOf {
                left, right, body, ..
            } => {
                self.for_head(left);
                self.expression(right);
                self.statement(body, false);
            }
            Stmt::While { test, body } => {
                self.expression(test);
                self.statement(body, false);
            }
            Stmt::DoWhile { body, test } => {
                self.statement(body, false);
                self.expression(test);
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.expression(discriminant);
                for case in cases {
                    if let Some(test) = &case.test {
                        self.expression(test);
                    }
                    self.statements(&case.consequent, false);
                }
            }
            Stmt::Labelled { item, .. } | Stmt::ClassField(item) => self.statement(item, false),
            Stmt::Return(value) => {
                if let Some(value) = value {
                    self.expression(value);
                }
            }
            Stmt::Try {
                block,
                handler,
                finalizer,
            } => {
                self.statements(block, false);
                if let Some(handler) = handler {
                    self.catch_clause(handler);
                }
                if let Some(finalizer) = finalizer {
                    self.statements(finalizer, false);
                }
            }
            Stmt::With { object, body } => {
                self.expression(object);
                self.statement(body, false);
            }
            Stmt::FunctionDecl(function) | Stmt::ModuleDefaultFunction { function, .. } => {
                self.function(function)
            }
            Stmt::ClassDecl(class) => self.class(class),
        }
    }

    fn expression(&mut self, expression: &Expr) {
        self.push(BlueJsAstNodeKind::Expression, false);
        match expression {
            Expr::Number(_)
            | Expr::BigInt(_)
            | Expr::String(_)
            | Expr::Bool(_)
            | Expr::Null
            | Expr::This
            | Expr::Identifier(_)
            | Expr::RegExp { .. }
            | Expr::Super
            | Expr::NewTarget
            | Expr::ImportMeta => {}
            Expr::Parenthesized(expression) | Expr::Await(expression) => {
                self.expression(expression)
            }
            Expr::Template { expressions, .. } => {
                for expression in expressions {
                    self.expression(expression);
                }
            }
            Expr::TaggedTemplate {
                tag, expressions, ..
            } => {
                self.expression(tag);
                for expression in expressions {
                    self.expression(expression);
                }
            }
            Expr::Array(elements) => {
                for element in elements.iter().flatten() {
                    match element {
                        ArrayElement::Normal(expression) | ArrayElement::Spread(expression) => {
                            self.expression(expression)
                        }
                    }
                }
            }
            Expr::Object(properties) => {
                for property in properties {
                    self.object_property(property);
                }
            }
            Expr::Function(function) => self.function(function),
            Expr::Class(class) => self.class(class),
            Expr::Yield { value, .. } => {
                if let Some(value) = value {
                    self.expression(value);
                }
            }
            Expr::DynamicImport { specifier, options } => {
                self.expression(specifier);
                if let Some(options) = options {
                    self.expression(options);
                }
            }
            Expr::Arrow { params, body, .. } => {
                for parameter in params {
                    self.parameter(parameter);
                }
                match body {
                    ArrowBody::Expr(expression) => self.expression(expression),
                    ArrowBody::Block(statements) => self.statements(statements, false),
                }
            }
            Expr::Unary { arg, .. } | Expr::Update { arg, .. } => self.expression(arg),
            Expr::Binary { left, right, .. } | Expr::Logical { left, right, .. } => {
                self.expression(left);
                self.expression(right);
            }
            Expr::Sequence(expressions) => {
                for expression in expressions {
                    self.expression(expression);
                }
            }
            Expr::Assign { target, value, .. } => {
                self.expression(target);
                self.expression(value);
            }
            Expr::DestructureAssign { pattern, value } => {
                self.assignment_pattern(pattern);
                self.expression(value);
            }
            Expr::Conditional {
                test,
                consequent,
                alternate,
            } => {
                self.expression(test);
                self.expression(consequent);
                self.expression(alternate);
            }
            Expr::Call { callee, args }
            | Expr::OptionalCall { callee, args }
            | Expr::New { callee, args } => {
                self.expression(callee);
                self.arguments(args);
            }
            Expr::Member {
                object, property, ..
            }
            | Expr::OptionalMember {
                object, property, ..
            } => {
                self.expression(object);
                self.expression(property);
            }
            Expr::PrivateIn { object, .. } => self.expression(object),
        }
    }

    fn function(&mut self, function: &Function) {
        for parameter in &function.params {
            self.parameter(parameter);
        }
        self.statements(&function.body, false);
    }

    fn parameter(&mut self, parameter: &Param) {
        self.pattern(&parameter.pattern);
        if let Some(default) = &parameter.default {
            self.expression(default);
        }
    }

    fn class(&mut self, class: &Class) {
        if let Some(extends) = &class.extends {
            self.expression(extends);
        }
        for element in &class.elements {
            match element {
                ClassElement::Method { key, function, .. }
                | ClassElement::Accessor { key, function, .. } => {
                    self.property_key(key);
                    self.function(function);
                }
                ClassElement::Field {
                    key, initializer, ..
                } => {
                    self.property_key(key);
                    if let Some(initializer) = initializer {
                        self.expression(initializer);
                    }
                }
                ClassElement::StaticBlock(statements) => self.statements(statements, false),
            }
        }
    }

    fn for_init(&mut self, init: &ForInit) {
        match init {
            ForInit::Expr(expression) => self.expression(expression),
            ForInit::VarDecl(_, declarations) => {
                for declaration in declarations {
                    self.pattern(&declaration.pattern);
                    if let Some(initializer) = &declaration.init {
                        self.expression(initializer);
                    }
                }
            }
        }
    }

    fn for_head(&mut self, head: &ForHead) {
        match head {
            ForHead::Decl(_, pattern) => self.pattern(pattern),
            ForHead::AnnexBVarInit(pattern, initializer) => {
                self.pattern(pattern);
                self.expression(initializer);
            }
            ForHead::Assignment(pattern) => self.assignment_pattern(pattern),
            ForHead::Expr(expression) => self.expression(expression),
        }
    }

    fn catch_clause(&mut self, handler: &CatchClause) {
        if let Some(parameter) = &handler.param {
            self.pattern(parameter);
        }
        self.statements(&handler.body, false);
    }

    fn pattern(&mut self, pattern: &Pattern) {
        match pattern {
            Pattern::Identifier(_) => {}
            Pattern::Array(elements) => {
                for element in elements.iter().flatten() {
                    self.pattern(&element.pattern);
                    if let Some(default) = &element.default {
                        self.expression(default);
                    }
                }
            }
            Pattern::Object(properties) => {
                for property in properties {
                    match property {
                        ObjectPatternProp::KeyValue {
                            key,
                            value,
                            default,
                        } => {
                            self.property_key(key);
                            self.pattern(value);
                            if let Some(default) = default {
                                self.expression(default);
                            }
                        }
                        ObjectPatternProp::Rest(pattern) => self.pattern(pattern),
                    }
                }
            }
        }
    }

    fn assignment_pattern(&mut self, pattern: &AssignmentPattern) {
        match pattern {
            AssignmentPattern::Target(expression) => self.expression(expression),
            AssignmentPattern::Array(elements) => {
                for element in elements.iter().flatten() {
                    self.assignment_pattern_element(element);
                }
            }
            AssignmentPattern::Object(properties) => {
                for property in properties {
                    match property {
                        AssignmentPatternProp::KeyValue {
                            key,
                            value,
                            default,
                        } => {
                            self.property_key(key);
                            self.assignment_pattern(value);
                            if let Some(default) = default {
                                self.expression(default);
                            }
                        }
                        AssignmentPatternProp::Rest(pattern) => self.assignment_pattern(pattern),
                    }
                }
            }
        }
    }

    fn assignment_pattern_element(&mut self, element: &AssignmentPatternElement) {
        self.assignment_pattern(&element.pattern);
        if let Some(default) = &element.default {
            self.expression(default);
        }
    }

    fn property_key(&mut self, key: &PropertyKey) {
        if let PropertyKey::Computed(expression) = key {
            self.expression(expression);
        }
    }

    fn object_property(&mut self, property: &ObjectProp) {
        match property {
            ObjectProp::KeyValue { key, value, .. } => {
                self.property_key(key);
                self.expression(value);
            }
            ObjectProp::Spread(expression) => self.expression(expression),
            ObjectProp::Method { key, function } | ObjectProp::Accessor { key, function, .. } => {
                self.property_key(key);
                self.function(function);
            }
        }
    }

    fn arguments(&mut self, arguments: &[Argument]) {
        for argument in arguments {
            match argument {
                Argument::Normal(expression) | Argument::Spread(expression) => {
                    self.expression(expression)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    #[test]
    fn inventories_executable_nodes_in_root_first_preorder() {
        let program = BlueJsProgramV1::Script(parse("let value=1+2; value;").unwrap());
        assert_eq!(
            collect_ast_nodes(&program)
                .into_iter()
                .map(|node| node.kind)
                .collect::<Vec<_>>(),
            vec![
                BlueJsAstNodeKind::Script,
                BlueJsAstNodeKind::Statement,
                BlueJsAstNodeKind::Expression,
                BlueJsAstNodeKind::Expression,
                BlueJsAstNodeKind::Expression,
                BlueJsAstNodeKind::Statement,
                BlueJsAstNodeKind::Expression,
            ]
        );
    }
}
