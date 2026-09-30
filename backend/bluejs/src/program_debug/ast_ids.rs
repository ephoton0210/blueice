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
            Stmt::Empty
            | Stmt::Break(_)
            | Stmt::Continue(_)
            | Stmt::ClassPrivateBrand(_)
            | Stmt::ClassExtraInitializers(_) => {}
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
            Stmt::Labelled { item, .. }
            | Stmt::ClassField(item)
            | Stmt::ClassDecoratedField { field: item, .. } => self.statement(item, false),
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
            Expr::DynamicImport {
                specifier, options, ..
            } => {
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
        for decorator in &class.decorators {
            self.expression(decorator);
        }
        if let Some(extends) = &class.extends {
            self.expression(extends);
        }
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
                    for decorator in decorators {
                        self.expression(decorator);
                    }
                    self.property_key(key);
                    self.function(function);
                }
                ClassElement::Field {
                    key,
                    initializer,
                    decorators,
                    ..
                } => {
                    for decorator in decorators {
                        self.expression(decorator);
                    }
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

    fn counts(program: &BlueJsProgramV1) -> (usize, usize) {
        let nodes = collect_ast_nodes(program);
        let statements = nodes
            .iter()
            .filter(|node| node.kind == BlueJsAstNodeKind::Statement)
            .count();
        let expressions = nodes
            .iter()
            .filter(|node| node.kind == BlueJsAstNodeKind::Expression)
            .count();
        (statements, expressions)
    }

    fn script_counts(source: &str) -> (usize, usize) {
        counts(&BlueJsProgramV1::Script(parse(source).unwrap()))
    }

    #[test]
    fn statement_forms_contribute_their_executable_children() {
        // (source, statement nodes, expression nodes), root excluded.
        let cases: &[(&str, usize, usize)] = &[
            (";", 1, 0),
            ("throw 1;", 1, 1),
            ("a: while(1) { continue a; }", 4, 1),
            ("a: while(1) break a;", 3, 1),
            (
                "var [a=1,,{b:c=2,...d}]=[1,...x],{e,[k]:f=3,...g}={};",
                1,
                8,
            ),
            ("if(a) b; else c;", 3, 3),
            ("if(a) b;", 2, 2),
            ("for(;;){}", 2, 0),
            ("for(var i=0;i<1;i++) ;", 2, 6),
            ("for(i=0;;) ;", 2, 3),
            ("for(var x in o) ;", 2, 1),
            ("for(x of o) ;", 2, 2),
            ("for([a=1] of o) ;", 2, 3),
            ("for(var x=1 in o) ;", 2, 2),
            // Annex B: a call is accepted as a for-in target in sloppy code.
            ("for(f() in o) ;", 2, 3),
            ("var a;", 1, 0),
            ("for(var i;;) ;", 2, 0),
            ("do ; while(a);", 2, 1),
            ("while(a) ;", 2, 1),
            ("switch(a){case b: c; default: d;}", 3, 4),
            ("try{a;}catch({b}){c;}finally{d;}", 4, 3),
            ("try{}catch{}", 1, 0),
            ("try{}finally{}", 1, 0),
            ("with(a) b;", 2, 2),
            ("function f(a=1,[b]){return;}", 2, 1),
            ("function g(){return 1;}", 2, 1),
        ];
        let actual: Vec<_> = cases
            .iter()
            .map(|(source, ..)| (*source, script_counts(source)))
            .collect();
        let expected: Vec<_> = cases
            .iter()
            .map(|(source, statements, expressions)| (*source, (*statements, *expressions)))
            .collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn expression_forms_contribute_their_executable_children() {
        let cases: &[(&str, usize, usize)] = &[
            ("1;'s';1n;true;null;this;x;/r/;", 8, 8),
            ("function f(){new.target;}", 2, 1),
            ("({m(){super.x;}});", 2, 4),
            ("(a);", 1, 1),
            ("(a) = 1;", 1, 4),
            ("(a?.b).c;", 1, 6),
            ("async function f(){await a;}", 2, 2),
            ("`a${b}c${d}`;", 1, 3),
            ("t`a${b}`;", 1, 3),
            ("[1,,...a];", 1, 3),
            (
                "({a:1,[k]:2,...s,m(){},get g(){return 1},set g(v){}});",
                2,
                6,
            ),
            ("(function(){});(class{});", 2, 2),
            ("function*g(){yield;yield 1;}", 3, 3),
            ("import('a');import('a',b);", 2, 5),
            ("((a,b=1)=>a);(()=>{c;});", 3, 5),
            ("!a;a++;", 2, 4),
            ("a+b;a&&b;", 2, 6),
            ("a,b;", 1, 3),
            ("a=1;", 1, 3),
            ("[a]=b;", 1, 3),
            ("({a:b=1,...c}=d);", 1, 5),
            ("({a:b}=c);", 1, 3),
            ("a?b:c;", 1, 4),
            ("f(a,...b);f?.(c);new F(d);", 3, 10),
            ("a.b;a?.b;a[c];", 3, 9),
            ("class C{#p;m(o){#p in o;}}", 2, 2),
            ("[[a=1],{b:c=2,...d}]=e;", 1, 7),
        ];
        let actual: Vec<_> = cases
            .iter()
            .map(|(source, ..)| (*source, script_counts(source)))
            .collect();
        let expected: Vec<_> = cases
            .iter()
            .map(|(source, statements, expressions)| (*source, (*statements, *expressions)))
            .collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn class_elements_contribute_decorators_keys_and_bodies() {
        let source = "@e class C extends B { @d m(){} static x=1; y; [k](){} \
                      @g get z(){return 1} static {a;} @f accessor w = 2; @h v; }";
        // Statements: the declaration, the getter's return, the static block.
        // Expressions: e, B, d, x's initializer, k, g, 1, a, f, 2, h.
        assert_eq!(script_counts(source), (3, 11));
    }

    #[test]
    fn module_roots_and_default_functions_are_inventoried() {
        let module = crate::parse_module("export default function(a=1){ a; }").unwrap();
        let program = BlueJsProgramV1::Module(module);
        let kinds: Vec<_> = collect_ast_nodes(&program)
            .into_iter()
            .map(|node| node.kind)
            .collect();
        assert_eq!(
            kinds,
            vec![
                BlueJsAstNodeKind::Module,
                BlueJsAstNodeKind::Statement,
                BlueJsAstNodeKind::Expression,
                BlueJsAstNodeKind::Statement,
                BlueJsAstNodeKind::Expression,
            ]
        );
    }

    #[test]
    fn compiler_internal_statements_are_transparent_wrappers() {
        let mut program = parse("1;").unwrap();
        program.body = vec![
            Stmt::ClassPrivateBrand("brand".to_string()),
            Stmt::ClassExtraInitializers("record".to_string()),
            Stmt::ClassDecoratedField {
                field: Box::new(Stmt::ClassField(Box::new(Stmt::Expr(Expr::Null)))),
                record: "record".to_string(),
            },
        ];
        // Two leaf markers, then decorated field -> field -> expression.
        assert_eq!(counts(&BlueJsProgramV1::Script(program)), (5, 1));
    }
}
