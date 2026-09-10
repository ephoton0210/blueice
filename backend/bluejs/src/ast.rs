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
}

/// The class subset currently records the observable constructor name and
/// whether a static `name` method replaces that data property.
#[derive(Debug, Clone, PartialEq)]
pub struct Class {
    pub name: Option<String>,
    pub static_name: bool,
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
    Yield(Option<Box<Expr>>),
    Arrow { params: Vec<Param>, body: ArrowBody },
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
