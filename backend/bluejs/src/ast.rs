// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The AST `parser.rs` builds -- scoped to exactly
//! `phase-2-mvp-scope/PLAN.md`'s "MVP JS scope (decided)" section. Two
//! omissions worth calling out because they're easy to expect and
//! aren't oversights: there is no `ClassDecl`/`ClassExpr` (that section
//! defers `class` entirely). Assignment patterns are deliberately a
//! separate AST from binding patterns because their leaves may be
//! existing member references as well as identifiers.

use crate::JsString;

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclKind {
    Var,
    Let,
    Const,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    pub name: Option<String>,
    pub params: Vec<Param>,
    pub body: Vec<Stmt>,
    pub generator: bool,
    /// Contextual `async` on a method. Async execution itself is a later
    /// suspension slice, but retaining the grammar prevents valid programs
    /// from being misreported as malformed source.
    pub is_async: bool,
}

/// A class definition with the executable elements currently supported by the
/// compiler. Private elements and decorators remain outside this AST subset.
#[derive(Debug, Clone, PartialEq)]
pub struct Class {
    pub name: Option<String>,
    pub extends: Option<Box<Expr>>,
    pub elements: Vec<ClassElement>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClassElement {
    Method { key: PropertyKey, function: Function, is_static: bool },
    Accessor { key: PropertyKey, function: Function, getter: bool, is_static: bool },
    Field { key: PropertyKey, initializer: Option<Expr>, is_static: bool },
    StaticBlock(Vec<Stmt>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub pattern: Pattern,
    pub default: Option<Expr>,
    pub rest: bool,
}

/// A binding target -- for `var`/`let`/`const` declarators, function
/// parameters, and `for`/`for-in`/`for-of` loop heads. Not used for
/// plain assignment-expression targets; see this module's doc comment.
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    Identifier(String),
    Array(Vec<Option<ArrayPatternElement>>),
    Object(Vec<ObjectPatternProp>),
}

/// A target used by a plain destructuring assignment such as
/// `([name, target.value] = source)`. Unlike [`Pattern`], it never
/// declares names and may write an existing member reference.
#[derive(Debug, Clone, PartialEq)]
pub enum AssignmentPattern {
    Target(Box<Expr>),
    Array(Vec<Option<AssignmentPatternElement>>),
    Object(Vec<AssignmentPatternProp>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct AssignmentPatternElement {
    pub pattern: AssignmentPattern,
    pub default: Option<Expr>,
    pub rest: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AssignmentPatternProp {
    KeyValue { key: PropertyKey, value: AssignmentPattern, default: Option<Expr> },
    Rest(AssignmentPattern),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArrayPatternElement {
    pub pattern: Pattern,
    pub default: Option<Expr>,
    pub rest: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ObjectPatternProp {
    KeyValue { key: PropertyKey, value: Pattern, default: Option<Expr> },
    Rest(Pattern),
}

#[derive(Debug, Clone, PartialEq)]
pub enum PropertyKey {
    Identifier(String),
    String(JsString),
    Number(f64),
    Computed(Box<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct VarDeclarator {
    pub pattern: Pattern,
    pub init: Option<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SwitchCase {
    /// `None` for a `default:` case.
    pub test: Option<Expr>,
    pub consequent: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CatchClause {
    pub param: Option<Pattern>,
    pub body: Vec<Stmt>,
}

/// A `for`-loop head that isn't a fresh declaration -- e.g. `for (x of
/// arr)` where `x` was already declared elsewhere. Restricted to a
/// [`Pattern`] shape (identifier or destructuring) rather than a full
/// [`Expr`]: real ECMAScript actually allows an arbitrary
/// `LeftHandSideExpression` here (e.g. `for (obj.prop of arr)`), but a
/// hand-written DOM script's `for-in`/`for-of` targets are essentially
/// always a bare identifier -- assigning into a member expression from
/// a loop head is the "honest cut" this crate makes rather than
/// building out full left-hand-side-expression support for a case this
/// MVP's scope doesn't call for.
#[derive(Debug, Clone, PartialEq)]
pub enum ForHead {
    Decl(DeclKind, Pattern),
    Pattern(Pattern),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ForInit {
    VarDecl(DeclKind, Vec<VarDeclarator>),
    Expr(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Empty,
    Expr(Expr),
    Block(Vec<Stmt>),
    VarDecl(DeclKind, Vec<VarDeclarator>),
    If { test: Expr, consequent: Box<Stmt>, alternate: Option<Box<Stmt>> },
    For { init: Option<ForInit>, test: Option<Expr>, update: Option<Expr>, body: Box<Stmt> },
    ForIn { left: ForHead, right: Expr, body: Box<Stmt> },
    ForOf { left: ForHead, right: Expr, body: Box<Stmt> },
    While { test: Expr, body: Box<Stmt> },
    DoWhile { body: Box<Stmt>, test: Expr },
    Switch { discriminant: Expr, cases: Vec<SwitchCase> },
    Break,
    Continue,
    Return(Option<Expr>),
    Throw(Expr),
    Try { block: Vec<Stmt>, handler: Option<CatchClause>, finalizer: Option<Vec<Stmt>> },
    With { object: Expr, body: Box<Stmt> },
    FunctionDecl(Function),
    ClassDecl(Class),
    /// Compiler-internal wrapper for an instance field lowered into its
    /// constructor body. The VM uses it to retain field-initializer lexical
    /// context for direct eval early errors.
    ClassField(Box<Stmt>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Plus,
    Not,
    Void,
    Typeof,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateOp {
    Inc,
    Dec,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    NotEq,
    StrictEq,
    StrictNotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    Instanceof,
    In,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicalOp {
    And,
    Or,
    Nullish,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    Assign,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
    ModAssign,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ArrayElement {
    Normal(Expr),
    Spread(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ObjectProp {
    KeyValue { key: PropertyKey, value: Expr, shorthand: bool },
    Spread(Expr),
    Method { key: PropertyKey, function: Function },
    Accessor { key: PropertyKey, function: Function, getter: bool },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Argument {
    Normal(Expr),
    Spread(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ArrowBody {
    Expr(Box<Expr>),
    Block(Vec<Stmt>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Number(f64),
    String(JsString),
    Bool(bool),
    Null,
    This,
    Identifier(String),
    Template { quasis: Vec<JsString>, expressions: Vec<Expr> },
    TaggedTemplate { tag: Box<Expr>, raw: Vec<JsString>, cooked: Vec<Option<JsString>>, expressions: Vec<Expr> },
    RegExp { pattern: JsString, flags: JsString },
    Array(Vec<Option<ArrayElement>>),
    Object(Vec<ObjectProp>),
    Function(Function),
    Class(Class),
    Super,
    NewTarget,
    Yield { value: Option<Box<Expr>>, delegate: bool },
    /// `await` is retained for async function grammar even though async
    /// execution and Promise jobs remain an explicit compiler gap.
    Await(Box<Expr>),
    Arrow { params: Vec<Param>, body: ArrowBody, is_async: bool },
    Unary { op: UnaryOp, arg: Box<Expr> },
    Update { op: UpdateOp, arg: Box<Expr>, prefix: bool },
    Binary { op: BinaryOp, left: Box<Expr>, right: Box<Expr> },
    Logical { op: LogicalOp, left: Box<Expr>, right: Box<Expr> },
    /// A left-to-right `Expression` sequence separated by commas. The value
    /// of the sequence is its final assignment expression.
    Sequence(Vec<Expr>),
    Assign { op: AssignOp, target: Box<Expr>, value: Box<Expr> },
    DestructureAssign { pattern: AssignmentPattern, value: Box<Expr> },
    Conditional { test: Box<Expr>, consequent: Box<Expr>, alternate: Box<Expr> },
    Call { callee: Box<Expr>, args: Vec<Argument> },
    New { callee: Box<Expr>, args: Vec<Argument> },
    Member { object: Box<Expr>, property: Box<Expr>, computed: bool },
}

/// Whether eval source contains a `super()` that belongs to the surrounding
/// eval context. A nested class establishes its own constructor context, so
/// its elements do not contribute to this check.
pub(crate) fn contains_super_call_outside_class(program: &Program) -> bool {
    program.body.iter().any(stmt_contains_super_call)
}

/// Whether an expression contains a `super()` belonging to its surrounding
/// class context. Nested classes establish their own context.
pub(crate) fn expr_contains_super_call_outside_class(expr: &Expr) -> bool {
    expr_contains_super_call(expr)
}

/// Whether statements contain a `super()` belonging to their surrounding
/// class context. Nested classes establish their own context.
pub(crate) fn statements_contain_super_call_outside_class(statements: &[Stmt]) -> bool {
    stmts_contain_super_call(statements)
}

/// Whether a function's parameters or body contain a `super()` belonging to
/// its surrounding class context. Nested classes establish their own context.
pub(crate) fn function_contains_super_call_outside_class(function: &Function) -> bool {
    function_contains_super_call(function)
}

fn stmts_contain_super_call(statements: &[Stmt]) -> bool {
    statements.iter().any(stmt_contains_super_call)
}

fn stmt_contains_super_call(statement: &Stmt) -> bool {
    match statement {
        Stmt::Empty | Stmt::Break | Stmt::Continue | Stmt::ClassDecl(_) => false,
        Stmt::Expr(expr) | Stmt::Throw(expr) => expr_contains_super_call(expr),
        Stmt::Block(statements) => stmts_contain_super_call(statements),
        Stmt::VarDecl(_, declarations) => declarations
            .iter()
            .any(|declaration| pattern_contains_super_call(&declaration.pattern) || declaration.init.as_ref().is_some_and(expr_contains_super_call)),
        Stmt::If { test, consequent, alternate } => {
            expr_contains_super_call(test)
                || stmt_contains_super_call(consequent)
                || alternate.as_deref().is_some_and(stmt_contains_super_call)
        }
        Stmt::For { init, test, update, body } => {
            init.as_ref().is_some_and(for_init_contains_super_call)
                || test.as_ref().is_some_and(expr_contains_super_call)
                || update.as_ref().is_some_and(expr_contains_super_call)
                || stmt_contains_super_call(body)
        }
        Stmt::ForIn { left, right, body } | Stmt::ForOf { left, right, body } => {
            for_head_contains_super_call(left) || expr_contains_super_call(right) || stmt_contains_super_call(body)
        }
        Stmt::While { test, body } | Stmt::DoWhile { body, test } => expr_contains_super_call(test) || stmt_contains_super_call(body),
        Stmt::Switch { discriminant, cases } => {
            expr_contains_super_call(discriminant)
                || cases.iter().any(|case| case.test.as_ref().is_some_and(expr_contains_super_call) || stmts_contain_super_call(&case.consequent))
        }
        Stmt::Return(value) => value.as_ref().is_some_and(expr_contains_super_call),
        Stmt::Try { block, handler, finalizer } => {
            stmts_contain_super_call(block)
                || handler.as_ref().is_some_and(|handler| pattern_option_contains_super_call(handler.param.as_ref()) || stmts_contain_super_call(&handler.body))
                || finalizer.as_deref().is_some_and(stmts_contain_super_call)
        }
        Stmt::With { object, body } => expr_contains_super_call(object) || stmt_contains_super_call(body),
        Stmt::FunctionDecl(function) => function_contains_super_call(function),
        Stmt::ClassField(statement) => stmt_contains_super_call(statement),
    }
}

fn for_init_contains_super_call(init: &ForInit) -> bool {
    match init {
        ForInit::Expr(expr) => expr_contains_super_call(expr),
        ForInit::VarDecl(_, declarations) => declarations
            .iter()
            .any(|declaration| pattern_contains_super_call(&declaration.pattern) || declaration.init.as_ref().is_some_and(expr_contains_super_call)),
    }
}

fn for_head_contains_super_call(head: &ForHead) -> bool {
    match head {
        ForHead::Decl(_, pattern) | ForHead::Pattern(pattern) => pattern_contains_super_call(pattern),
    }
}

fn function_contains_super_call(function: &Function) -> bool {
    function.params.iter().any(|param| pattern_contains_super_call(&param.pattern) || param.default.as_ref().is_some_and(expr_contains_super_call))
        || stmts_contain_super_call(&function.body)
}

fn pattern_option_contains_super_call(pattern: Option<&Pattern>) -> bool {
    pattern.is_some_and(pattern_contains_super_call)
}

fn pattern_contains_super_call(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::Identifier(_) => false,
        Pattern::Array(elements) => elements
            .iter()
            .flatten()
            .any(|element| pattern_contains_super_call(&element.pattern) || element.default.as_ref().is_some_and(expr_contains_super_call)),
        Pattern::Object(properties) => properties.iter().any(|property| match property {
            ObjectPatternProp::KeyValue { key, value, default } => {
                property_key_contains_super_call(key) || pattern_contains_super_call(value) || default.as_ref().is_some_and(expr_contains_super_call)
            }
            ObjectPatternProp::Rest(pattern) => pattern_contains_super_call(pattern),
        }),
    }
}

fn assignment_pattern_contains_super_call(pattern: &AssignmentPattern) -> bool {
    match pattern {
        AssignmentPattern::Target(expr) => expr_contains_super_call(expr),
        AssignmentPattern::Array(elements) => elements
            .iter()
            .flatten()
            .any(|element| assignment_pattern_contains_super_call(&element.pattern) || element.default.as_ref().is_some_and(expr_contains_super_call)),
        AssignmentPattern::Object(properties) => properties.iter().any(|property| match property {
            AssignmentPatternProp::KeyValue { key, value, default } => {
                property_key_contains_super_call(key) || assignment_pattern_contains_super_call(value) || default.as_ref().is_some_and(expr_contains_super_call)
            }
            AssignmentPatternProp::Rest(pattern) => assignment_pattern_contains_super_call(pattern),
        }),
    }
}

fn property_key_contains_super_call(key: &PropertyKey) -> bool {
    matches!(key, PropertyKey::Computed(expr) if expr_contains_super_call(expr))
}

fn expr_contains_super_call(expr: &Expr) -> bool {
    match expr {
        Expr::Number(_) | Expr::String(_) | Expr::Bool(_) | Expr::Null | Expr::This | Expr::Identifier(_) | Expr::RegExp { .. } | Expr::Super | Expr::NewTarget => false,
        Expr::Template { expressions, .. } => expressions.iter().any(expr_contains_super_call),
        Expr::TaggedTemplate { tag, expressions, .. } => expr_contains_super_call(tag) || expressions.iter().any(expr_contains_super_call),
        Expr::Array(elements) => elements.iter().flatten().any(|element| match element {
            ArrayElement::Normal(expr) | ArrayElement::Spread(expr) => expr_contains_super_call(expr),
        }),
        Expr::Object(properties) => properties.iter().any(|property| match property {
            ObjectProp::KeyValue { key, value, .. } => property_key_contains_super_call(key) || expr_contains_super_call(value),
            ObjectProp::Spread(expr) => expr_contains_super_call(expr),
            ObjectProp::Method { key, function } | ObjectProp::Accessor { key, function, .. } => {
                property_key_contains_super_call(key) || function_contains_super_call(function)
            }
        }),
        Expr::Function(function) => function_contains_super_call(function),
        Expr::Class(_) => false,
        Expr::Yield { value, .. } => value.as_deref().is_some_and(expr_contains_super_call),
        Expr::Await(expr) | Expr::Unary { arg: expr, .. } | Expr::Update { arg: expr, .. } => expr_contains_super_call(expr),
        Expr::Arrow { params, body, .. } => {
            params.iter().any(|param| pattern_contains_super_call(&param.pattern) || param.default.as_ref().is_some_and(expr_contains_super_call))
                || match body {
                    ArrowBody::Expr(expr) => expr_contains_super_call(expr),
                    ArrowBody::Block(statements) => stmts_contain_super_call(statements),
                }
        }
        Expr::Binary { left, right, .. } | Expr::Logical { left, right, .. } => expr_contains_super_call(left) || expr_contains_super_call(right),
        Expr::Sequence(expressions) => expressions.iter().any(expr_contains_super_call),
        Expr::Assign { target, value, .. } => expr_contains_super_call(target) || expr_contains_super_call(value),
        Expr::DestructureAssign { pattern, value } => assignment_pattern_contains_super_call(pattern) || expr_contains_super_call(value),
        Expr::Conditional { test, consequent, alternate } => {
            expr_contains_super_call(test) || expr_contains_super_call(consequent) || expr_contains_super_call(alternate)
        }
        Expr::Call { callee, args } => {
            matches!(callee.as_ref(), Expr::Super)
                || expr_contains_super_call(callee)
                || args.iter().any(|argument| match argument {
                    Argument::Normal(expr) | Argument::Spread(expr) => expr_contains_super_call(expr),
                })
        }
        Expr::New { callee, args } => {
            expr_contains_super_call(callee)
                || args.iter().any(|argument| match argument {
                    Argument::Normal(expr) | Argument::Spread(expr) => expr_contains_super_call(expr),
                })
        }
        Expr::Member { object, property, .. } => expr_contains_super_call(object) || expr_contains_super_call(property),
    }
}
