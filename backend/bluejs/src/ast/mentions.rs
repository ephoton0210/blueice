// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Which identifier names a function's source mentions, anywhere inside it.
//!
//! A closure used to capture every name visible where it was created, whether or not it
//! used them, so a function made in a scope with thousands of bindings paid for thousands of
//! captures on every call. The compiler instead captures the names this reports, plus every
//! compiler-internal name (they begin with NUL and never appear in source).
//!
//! The answer is a superset on purpose: any identifier in any position counts (a property
//! name after `.`, a declaration, an assignment target), nested functions and classes are
//! entered (a name they use must come through this function), and a mention of `eval` anywhere
//! means "everything", since a direct eval can name a variable only at run time. Every
//! `match` here is exhaustive, so a new kind of node cannot be skipped silently; and the
//! compiler does not depend on this being complete, because it notices a name it pruned being
//! looked up and compiles the function again with all of them.

use super::*;
use std::collections::BTreeSet;

pub(crate) struct Mentions<'a> {
    names: BTreeSet<&'a str>,
    eval: bool,
}

impl Mentions<'_> {
    /// Whether code inside may use `name` from an enclosing scope.
    pub(crate) fn may_use(&self, name: &str) -> bool {
        self.eval || name.starts_with('\0') || self.names.contains(name)
    }
}

pub(crate) fn function_mentions(function: &Function) -> Mentions<'_> {
    let mut found = Mentions {
        names: BTreeSet::new(),
        eval: false,
    };
    found.function(function);
    found
}

impl<'a> Mentions<'a> {
    fn name(&mut self, name: &'a str) {
        if name == "eval" {
            self.eval = true;
        }
        self.names.insert(name);
    }

    fn function(&mut self, function: &'a Function) {
        self.params(&function.params);
        self.stmts(&function.body);
    }

    fn params(&mut self, params: &'a [Param]) {
        for param in params {
            self.pattern(&param.pattern);
            self.opt_expr(param.default.as_ref());
        }
    }

    fn stmts(&mut self, statements: &'a [Stmt]) {
        for statement in statements {
            self.stmt(statement);
        }
    }

    fn opt_expr(&mut self, expr: Option<&'a Expr>) {
        if let Some(expr) = expr {
            self.expr(expr);
        }
    }

    fn exprs(&mut self, exprs: &'a [Expr]) {
        for expr in exprs {
            self.expr(expr);
        }
    }

    fn class(&mut self, class: &'a Class) {
        if let Some(name) = &class.name {
            self.name(name);
        }
        self.opt_expr(class.extends.as_deref());
        self.exprs(&class.decorators);
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
                    self.key(key);
                    self.function(function);
                    self.exprs(decorators);
                }
                ClassElement::Field {
                    key,
                    initializer,
                    decorators,
                    ..
                } => {
                    self.key(key);
                    self.opt_expr(initializer.as_ref());
                    self.exprs(decorators);
                }
                ClassElement::StaticBlock(body) => self.stmts(body),
            }
        }
    }

    fn key(&mut self, key: &'a PropertyKey) {
        match key {
            PropertyKey::Identifier(name) => self.name(name),
            PropertyKey::String(_) | PropertyKey::Number(_) => {}
            PropertyKey::Computed(expr) => self.expr(expr),
        }
    }

    fn pattern(&mut self, pattern: &'a Pattern) {
        match pattern {
            Pattern::Identifier(name) => self.name(name),
            Pattern::Array(elements) => {
                for element in elements.iter().flatten() {
                    self.pattern(&element.pattern);
                    self.opt_expr(element.default.as_ref());
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
                            self.key(key);
                            self.pattern(value);
                            self.opt_expr(default.as_ref());
                        }
                        ObjectPatternProp::Rest(pattern) => self.pattern(pattern),
                    }
                }
            }
        }
    }

    fn assignment_pattern(&mut self, pattern: &'a AssignmentPattern) {
        match pattern {
            AssignmentPattern::Target(expr) => self.expr(expr),
            AssignmentPattern::Array(elements) => {
                for element in elements.iter().flatten() {
                    self.assignment_pattern(&element.pattern);
                    self.opt_expr(element.default.as_ref());
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
                            self.key(key);
                            self.assignment_pattern(value);
                            self.opt_expr(default.as_ref());
                        }
                        AssignmentPatternProp::Rest(pattern) => self.assignment_pattern(pattern),
                    }
                }
            }
        }
    }

    fn declarators(&mut self, declarations: &'a [VarDeclarator]) {
        for declaration in declarations {
            self.pattern(&declaration.pattern);
            self.opt_expr(declaration.init.as_ref());
        }
    }

    fn for_head(&mut self, head: &'a ForHead) {
        match head {
            ForHead::Decl(_, pattern) => self.pattern(pattern),
            ForHead::AnnexBVarInit(pattern, expr) => {
                self.pattern(pattern);
                self.expr(expr);
            }
            ForHead::Assignment(pattern) => self.assignment_pattern(pattern),
            ForHead::Expr(expr) => self.expr(expr),
        }
    }

    fn stmt(&mut self, statement: &'a Stmt) {
        match statement {
            Stmt::Empty | Stmt::Break(_) | Stmt::Continue(_) => {}
            Stmt::Expr(expr) | Stmt::Throw(expr) => self.expr(expr),
            Stmt::Block(statements) => self.stmts(statements),
            Stmt::VarDecl(_, declarations) => self.declarators(declarations),
            Stmt::If {
                test,
                consequent,
                alternate,
            } => {
                self.expr(test);
                self.stmt(consequent);
                if let Some(alternate) = alternate {
                    self.stmt(alternate);
                }
            }
            Stmt::For {
                init,
                test,
                update,
                body,
            } => {
                match init {
                    Some(ForInit::VarDecl(_, declarations)) => self.declarators(declarations),
                    Some(ForInit::Expr(expr)) => self.expr(expr),
                    None => {}
                }
                self.opt_expr(test.as_ref());
                self.opt_expr(update.as_ref());
                self.stmt(body);
            }
            Stmt::ForIn { left, right, body }
            | Stmt::ForOf {
                left, right, body, ..
            } => {
                self.for_head(left);
                self.expr(right);
                self.stmt(body);
            }
            Stmt::While { test, body } | Stmt::DoWhile { body, test } => {
                self.expr(test);
                self.stmt(body);
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.expr(discriminant);
                for case in cases {
                    self.opt_expr(case.test.as_ref());
                    self.stmts(&case.consequent);
                }
            }
            Stmt::Labelled { item, .. } => self.stmt(item),
            Stmt::Return(expr) => self.opt_expr(expr.as_ref()),
            Stmt::Try {
                block,
                handler,
                finalizer,
            } => {
                self.stmts(block);
                if let Some(handler) = handler {
                    if let Some(param) = &handler.param {
                        self.pattern(param);
                    }
                    self.stmts(&handler.body);
                }
                if let Some(finalizer) = finalizer {
                    self.stmts(finalizer);
                }
            }
            Stmt::With { object, body } => {
                self.expr(object);
                self.stmt(body);
            }
            Stmt::FunctionDecl(function) => {
                if let Some(name) = &function.name {
                    self.name(name);
                }
                self.function(function);
            }
            Stmt::ModuleDefaultFunction { function, binding } => {
                self.name(binding);
                self.function(function);
            }
            Stmt::ClassDecl(class) => self.class(class),
            Stmt::ClassField(inner) => self.stmt(inner),
            Stmt::ClassPrivateBrand(name) | Stmt::ClassExtraInitializers(name) => self.name(name),
            Stmt::ClassDecoratedField { field, record } => {
                self.name(record);
                self.stmt(field);
            }
        }
    }

    fn expr(&mut self, expr: &'a Expr) {
        match expr {
            Expr::Number(_)
            | Expr::BigInt(_)
            | Expr::String(_)
            | Expr::Bool(_)
            | Expr::Null
            | Expr::This
            | Expr::RegExp { .. }
            | Expr::Super
            | Expr::NewTarget
            | Expr::ImportMeta => {}
            Expr::Identifier(name) => self.name(name),
            Expr::PrivateIn { object, .. } => self.expr(object),
            Expr::Parenthesized(inner) | Expr::Await(inner) => self.expr(inner),
            Expr::Template { expressions, .. } => self.exprs(expressions),
            Expr::TaggedTemplate {
                tag, expressions, ..
            } => {
                self.expr(tag);
                self.exprs(expressions);
            }
            Expr::Array(elements) => {
                for element in elements.iter().flatten() {
                    match element {
                        ArrayElement::Normal(expr) | ArrayElement::Spread(expr) => self.expr(expr),
                    }
                }
            }
            Expr::Object(properties) => {
                for property in properties {
                    match property {
                        ObjectProp::KeyValue { key, value, .. } => {
                            self.key(key);
                            self.expr(value);
                        }
                        ObjectProp::Spread(expr) => self.expr(expr),
                        ObjectProp::Method { key, function }
                        | ObjectProp::Accessor { key, function, .. } => {
                            self.key(key);
                            self.function(function);
                        }
                    }
                }
            }
            Expr::Function(function) => {
                if let Some(name) = &function.name {
                    self.name(name);
                }
                self.function(function);
            }
            Expr::Class(class) => self.class(class),
            Expr::Yield { value, .. } => self.opt_expr(value.as_deref()),
            Expr::DynamicImport {
                specifier, options, ..
            } => {
                self.expr(specifier);
                self.opt_expr(options.as_deref());
            }
            Expr::Arrow { params, body, .. } => {
                self.params(params);
                match body {
                    ArrowBody::Expr(expr) => self.expr(expr),
                    ArrowBody::Block(statements) => self.stmts(statements),
                }
            }
            Expr::Unary { arg, .. } | Expr::Update { arg, .. } => self.expr(arg),
            Expr::Binary { left, right, .. } | Expr::Logical { left, right, .. } => {
                self.expr(left);
                self.expr(right);
            }
            Expr::Sequence(expressions) => self.exprs(expressions),
            Expr::Assign { target, value, .. } => {
                self.expr(target);
                self.expr(value);
            }
            Expr::DestructureAssign { pattern, value } => {
                self.assignment_pattern(pattern);
                self.expr(value);
            }
            Expr::Conditional {
                test,
                consequent,
                alternate,
            } => {
                self.expr(test);
                self.expr(consequent);
                self.expr(alternate);
            }
            Expr::Call { callee, args }
            | Expr::OptionalCall { callee, args }
            | Expr::New { callee, args } => {
                self.expr(callee);
                for argument in args {
                    match argument {
                        Argument::Normal(expr) | Argument::Spread(expr) => self.expr(expr),
                    }
                }
            }
            Expr::Member {
                object, property, ..
            }
            | Expr::OptionalMember {
                object, property, ..
            } => {
                self.expr(object);
                self.expr(property);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mentions_of(source: &str) -> (Vec<String>, bool) {
        let program = crate::parse(source).expect(source);
        let Some(Stmt::FunctionDecl(function)) = program.body.first() else {
            panic!("the test source starts with a function declaration");
        };
        let found = function_mentions(function);
        (
            found.names.iter().map(|n| (*n).to_string()).collect(),
            found.eval,
        )
    }

    fn uses(source: &str, name: &str) -> bool {
        let program = crate::parse(source).expect(source);
        let Some(Stmt::FunctionDecl(function)) = program.body.first() else {
            panic!("the test source starts with a function declaration");
        };
        function_mentions(function).may_use(name)
    }

    #[test]
    fn a_name_that_is_not_written_is_not_used() {
        assert!(uses("function f() { return a + b; }", "a"));
        assert!(!uses("function f() { return a + b; }", "c"));
    }

    #[test]
    fn every_position_counts() {
        for (source, name) in [
            ("function f(p = a) {}", "a"),
            ("function f({ k = a }) {}", "a"),
            ("function f() { a = 1; }", "a"),
            ("function f() { a++; }", "a"),
            ("function f() { [a] = x; }", "a"),
            ("function f() { ({ y: a } = x); }", "a"),
            ("function f() { for (a of x) ; }", "a"),
            ("function f() { for (a in x) ; }", "a"),
            ("function f() { for (let i = a; ;) break; }", "a"),
            ("function f() { typeof a; }", "a"),
            ("function f() { delete a.b; }", "a"),
            ("function f() { `${a}`; }", "a"),
            ("function f() { t`${a}`; }", "a"),
            ("function f() { ({ [a]: 1 }); }", "a"),
            ("function f() { ({ a }); }", "a"),
            ("function f() { ({ ...a }); }", "a"),
            ("function f() { return a ? 1 : 2; }", "a"),
            ("function f() { switch (a) { case b: } }", "b"),
            ("function f() { try {} catch ({ m = a }) {} }", "a"),
            ("function f() { with (a) {} }", "a"),
            ("function f() { new a(); }", "a"),
            ("function f() { a?.(); }", "a"),
            ("function f() { a?.b; }", "a"),
            ("function f() { import(a); }", "a"),
            ("function f() { return () => a; }", "a"),
            ("function f() { return function () { return a; }; }", "a"),
            ("function f() { return class { m() { return a; } }; }", "a"),
            ("function f() { return class extends a {}; }", "a"),
            ("function f() { return class { [a]() {} }; }", "a"),
            ("function f() { return class { x = a; }; }", "a"),
            ("function f() { return class { static { a; } }; }", "a"),
            ("function f() { class C { @a m() {} } }", "a"),
            ("function f() { label: for (;;) { a; } }", "a"),
            ("function f() { a: while (b) {} }", "b"),
            ("function f() { do { } while (a); }", "a"),
            ("function f() { throw a; }", "a"),
            ("function f() { `x` + a; }", "a"),
            ("async function f() { await a; }", "a"),
            ("function* f() { yield a; }", "a"),
            ("function f() { a = b, c; }", "c"),
            ("function f() { return [...a]; }", "a"),
            ("function f() { g(...a); }", "a"),
            ("function f() { 1 in a; }", "a"),
            ("function f() { using u = a; }", "a"),
        ] {
            assert!(uses(source, name), "{source} should use {name}");
        }
    }

    #[test]
    fn eval_anywhere_means_everything_and_internal_names_are_always_kept() {
        assert!(uses("function f() { eval('a'); }", "anything"));
        assert!(uses("function f() { return () => eval('a'); }", "anything"));
        assert!(uses("function f() { (0, eval)('a'); }", "anything"));
        assert!(uses("function f() { }", "\0bluejs_derived_this"));
        assert!(!uses("function f() { }", "anything"));
        let (names, eval) = mentions_of("function f() { return a; }");
        assert_eq!((names, eval), (vec!["a".to_string()], false));
    }
}
