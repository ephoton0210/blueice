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
use num_bigint::BigInt;

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub body: Vec<Stmt>,
}

/// The syntactic information a Source Text Module Record retains after its
/// executable statements have been parsed.  Import and export declarations
/// are deliberately not ordinary statements: they participate in linking
/// before any statement in the graph is evaluated.
#[derive(Debug, Clone, PartialEq)]
pub struct Module {
    pub body: Vec<Stmt>,
    pub imports: Vec<ImportEntry>,
    pub exports: Vec<ExportEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportName {
    Named(String),
    Namespace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportEntry {
    pub module_request: String,
    pub import_name: ImportName,
    /// `None` represents `import "specifier";`, which participates in
    /// dependency evaluation but creates no local binding.
    pub local_name: Option<String>,
}

/// One declarative export.  Local entries point at a binding in this module;
/// indirect and star entries are followed during graph resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportEntry {
    Local {
        export_name: String,
        local_name: String,
    },
    Indirect {
        export_name: String,
        module_request: String,
        import_name: String,
    },
    Star {
        module_request: String,
    },
    Namespace {
        export_name: String,
        module_request: String,
    },
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
    Method {
        key: PropertyKey,
        function: Function,
        is_static: bool,
    },
    Accessor {
        key: PropertyKey,
        function: Function,
        getter: bool,
        is_static: bool,
    },
    Field {
        key: PropertyKey,
        initializer: Option<Expr>,
        is_static: bool,
    },
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
    KeyValue {
        key: PropertyKey,
        value: AssignmentPattern,
        default: Option<Expr>,
    },
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
    KeyValue {
        key: PropertyKey,
        value: Pattern,
        default: Option<Expr>,
    },
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

/// A `for`-loop head. Most non-declaration heads use a [`Pattern`], such as
/// `for (x of values)`. The `Expr` form retains the Annex B web-compat
/// CallExpression target so execution can evaluate its call and then report
/// the required runtime ReferenceError, rather than rejecting the source
/// before the observable call takes place.
#[derive(Debug, Clone, PartialEq)]
pub enum ForHead {
    Decl(DeclKind, Pattern),
    Pattern(Pattern),
    Expr(Expr),
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
    If {
        test: Expr,
        consequent: Box<Stmt>,
        alternate: Option<Box<Stmt>>,
    },
    For {
        init: Option<ForInit>,
        test: Option<Expr>,
        update: Option<Expr>,
        body: Box<Stmt>,
    },
    ForIn {
        left: ForHead,
        right: Expr,
        body: Box<Stmt>,
    },
    ForOf {
        left: ForHead,
        right: Expr,
        body: Box<Stmt>,
    },
    While {
        test: Expr,
        body: Box<Stmt>,
    },
    DoWhile {
        body: Box<Stmt>,
        test: Expr,
    },
    Switch {
        discriminant: Expr,
        cases: Vec<SwitchCase>,
    },
    /// A labelled statement. Labels target the entire wrapped statement;
    /// consecutive labels are collapsed by the compiler onto an iteration
    /// statement when appropriate.
    Labelled {
        label: String,
        item: Box<Stmt>,
    },
    Break(Option<String>),
    Continue(Option<String>),
    Return(Option<Expr>),
    Throw(Expr),
    Try {
        block: Vec<Stmt>,
        handler: Option<CatchClause>,
        finalizer: Option<Vec<Stmt>>,
    },
    With {
        object: Expr,
        body: Box<Stmt>,
    },
    FunctionDecl(Function),
    /// `export default function …` has no ordinary module-local binding for
    /// its source-level name, but its default export is initialized during
    /// ModuleDeclarationInstantiation just like a function declaration.
    ModuleDefaultFunction {
        function: Function,
        binding: String,
    },
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
    BitNot,
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
    ShiftLeft,
    ShiftRight,
    UnsignedShiftRight,
    BitAnd,
    BitXor,
    BitOr,
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
    ShiftLeftAssign,
    ShiftRightAssign,
    UnsignedShiftRightAssign,
    BitAndAssign,
    BitXorAssign,
    BitOrAssign,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ArrayElement {
    Normal(Expr),
    Spread(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ObjectProp {
    KeyValue {
        key: PropertyKey,
        value: Expr,
        shorthand: bool,
    },
    Spread(Expr),
    Method {
        key: PropertyKey,
        function: Function,
    },
    Accessor {
        key: PropertyKey,
        function: Function,
        getter: bool,
    },
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
    BigInt(BigInt),
    String(JsString),
    Bool(bool),
    Null,
    This,
    Identifier(String),
    Template {
        quasis: Vec<JsString>,
        expressions: Vec<Expr>,
    },
    TaggedTemplate {
        tag: Box<Expr>,
        raw: Vec<JsString>,
        cooked: Vec<Option<JsString>>,
        expressions: Vec<Expr>,
    },
    RegExp {
        pattern: JsString,
        flags: JsString,
    },
    Array(Vec<Option<ArrayElement>>),
    Object(Vec<ObjectProp>),
    Function(Function),
    Class(Class),
    Super,
    NewTarget,
    Yield {
        value: Option<Box<Expr>>,
        delegate: bool,
    },
    /// Contextual `await`, valid in async functions and at module top level.
    Await(Box<Expr>),
    /// The `import()` expression is distinct from the static module-item
    /// grammar and always evaluates to a Promise.
    DynamicImport(Box<Expr>),
    Arrow {
        params: Vec<Param>,
        body: ArrowBody,
        is_async: bool,
    },
    Unary {
        op: UnaryOp,
        arg: Box<Expr>,
    },
    Update {
        op: UpdateOp,
        arg: Box<Expr>,
        prefix: bool,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Logical {
        op: LogicalOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// A left-to-right `Expression` sequence separated by commas. The value
    /// of the sequence is its final assignment expression.
    Sequence(Vec<Expr>),
    Assign {
        op: AssignOp,
        target: Box<Expr>,
        value: Box<Expr>,
    },
    DestructureAssign {
        pattern: AssignmentPattern,
        value: Box<Expr>,
    },
    Conditional {
        test: Box<Expr>,
        consequent: Box<Expr>,
        alternate: Box<Expr>,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Argument>,
    },
    New {
        callee: Box<Expr>,
        args: Vec<Argument>,
    },
    Member {
        object: Box<Expr>,
        property: Box<Expr>,
        computed: bool,
    },
}

/// Whether eval source contains a `super()` that belongs to the surrounding
/// eval context. A nested class establishes its own constructor context, so
/// its elements do not contribute to this check.
pub(crate) fn contains_super_call_outside_class(program: &Program) -> bool {
    program
        .body
        .iter()
        .any(|statement| stmt_contains_super(statement, SuperSearch::Call))
}

/// Whether script source contains a `super` property reference that is not
/// owned by a nested class or object method.
pub(crate) fn contains_super_property_outside_class(program: &Program) -> bool {
    program
        .body
        .iter()
        .any(|statement| stmt_contains_super(statement, SuperSearch::Property))
}

/// Whether an expression contains a `super()` belonging to its surrounding
/// class context. Nested classes establish their own context.
pub(crate) fn expr_contains_super_call_outside_class(expr: &Expr) -> bool {
    expr_contains_super(expr, SuperSearch::Call)
}

/// Whether statements contain a `super()` belonging to their surrounding
/// class context. Nested classes establish their own context.
pub(crate) fn statements_contain_super_call_outside_class(statements: &[Stmt]) -> bool {
    stmts_contain_super_call(statements)
}

/// Whether a function's parameters or body contain a `super()` belonging to
/// its surrounding class context. Nested classes establish their own context.
pub(crate) fn function_contains_super_call_outside_class(function: &Function) -> bool {
    function_contains_super(function, SuperSearch::Call)
}

/// Whether a normal function contains a `super` property reference which is
/// not owned by a nested class or object method.
pub(crate) fn function_contains_super_property_outside_class(function: &Function) -> bool {
    function_contains_super(function, SuperSearch::Property)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SuperSearch {
    Call,
    Property,
}

fn stmts_contain_super_call(statements: &[Stmt]) -> bool {
    stmts_contain_super(statements, SuperSearch::Call)
}

fn stmts_contain_super(statements: &[Stmt], search: SuperSearch) -> bool {
    statements
        .iter()
        .any(|statement| stmt_contains_super(statement, search))
}

fn stmt_contains_super(statement: &Stmt, search: SuperSearch) -> bool {
    match statement {
        Stmt::Empty | Stmt::Break(_) | Stmt::Continue(_) | Stmt::ClassDecl(_) => false,
        Stmt::Expr(expr) | Stmt::Throw(expr) => expr_contains_super(expr, search),
        Stmt::Block(statements) => stmts_contain_super(statements, search),
        Stmt::VarDecl(_, declarations) => declarations.iter().any(|declaration| {
            pattern_contains_super(&declaration.pattern, search)
                || declaration
                    .init
                    .as_ref()
                    .is_some_and(|expr| expr_contains_super(expr, search))
        }),
        Stmt::If {
            test,
            consequent,
            alternate,
        } => {
            expr_contains_super(test, search)
                || stmt_contains_super(consequent, search)
                || alternate
                    .as_deref()
                    .is_some_and(|statement| stmt_contains_super(statement, search))
        }
        Stmt::For {
            init,
            test,
            update,
            body,
        } => {
            init.as_ref()
                .is_some_and(|init| for_init_contains_super(init, search))
                || test
                    .as_ref()
                    .is_some_and(|expr| expr_contains_super(expr, search))
                || update
                    .as_ref()
                    .is_some_and(|expr| expr_contains_super(expr, search))
                || stmt_contains_super(body, search)
        }
        Stmt::ForIn { left, right, body } | Stmt::ForOf { left, right, body } => {
            for_head_contains_super(left, search)
                || expr_contains_super(right, search)
                || stmt_contains_super(body, search)
        }
        Stmt::While { test, body } | Stmt::DoWhile { body, test } => {
            expr_contains_super(test, search) || stmt_contains_super(body, search)
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            expr_contains_super(discriminant, search)
                || cases.iter().any(|case| {
                    case.test
                        .as_ref()
                        .is_some_and(|expr| expr_contains_super(expr, search))
                        || stmts_contain_super(&case.consequent, search)
                })
        }
        Stmt::Return(value) => value
            .as_ref()
            .is_some_and(|expr| expr_contains_super(expr, search)),
        Stmt::Try {
            block,
            handler,
            finalizer,
        } => {
            stmts_contain_super(block, search)
                || handler.as_ref().is_some_and(|handler| {
                    pattern_option_contains_super(handler.param.as_ref(), search)
                        || stmts_contain_super(&handler.body, search)
                })
                || finalizer
                    .as_deref()
                    .is_some_and(|statements| stmts_contain_super(statements, search))
        }
        Stmt::With { object, body } => {
            expr_contains_super(object, search) || stmt_contains_super(body, search)
        }
        Stmt::Labelled { item, .. } => stmt_contains_super(item, search),
        Stmt::FunctionDecl(function) | Stmt::ModuleDefaultFunction { function, .. } => {
            function_contains_super(function, search)
        }
        Stmt::ClassField(statement) => stmt_contains_super(statement, search),
    }
}

fn for_init_contains_super(init: &ForInit, search: SuperSearch) -> bool {
    match init {
        ForInit::Expr(expr) => expr_contains_super(expr, search),
        ForInit::VarDecl(_, declarations) => declarations.iter().any(|declaration| {
            pattern_contains_super(&declaration.pattern, search)
                || declaration
                    .init
                    .as_ref()
                    .is_some_and(|expr| expr_contains_super(expr, search))
        }),
    }
}

fn for_head_contains_super(head: &ForHead, search: SuperSearch) -> bool {
    match head {
        ForHead::Decl(_, pattern) | ForHead::Pattern(pattern) => {
            pattern_contains_super(pattern, search)
        }
        ForHead::Expr(expr) => expr_contains_super(expr, search),
    }
}

fn function_contains_super(function: &Function, search: SuperSearch) -> bool {
    function.params.iter().any(|param| {
        pattern_contains_super(&param.pattern, search)
            || param
                .default
                .as_ref()
                .is_some_and(|expr| expr_contains_super(expr, search))
    }) || stmts_contain_super(&function.body, search)
}

fn pattern_option_contains_super(pattern: Option<&Pattern>, search: SuperSearch) -> bool {
    pattern.is_some_and(|pattern| pattern_contains_super(pattern, search))
}

fn pattern_contains_super(pattern: &Pattern, search: SuperSearch) -> bool {
    match pattern {
        Pattern::Identifier(_) => false,
        Pattern::Array(elements) => elements.iter().flatten().any(|element| {
            pattern_contains_super(&element.pattern, search)
                || element
                    .default
                    .as_ref()
                    .is_some_and(|expr| expr_contains_super(expr, search))
        }),
        Pattern::Object(properties) => {
            for property in properties {
                let contains_super = match property {
                    ObjectPatternProp::KeyValue {
                        key,
                        value,
                        default,
                    } => {
                        property_key_contains_super(key, search)
                            || pattern_contains_super(value, search)
                            || default
                                .as_ref()
                                .is_some_and(|expr| expr_contains_super(expr, search))
                    }
                    ObjectPatternProp::Rest(pattern) => pattern_contains_super(pattern, search),
                };
                if contains_super {
                    return true;
                }
            }
            false
        }
    }
}

fn assignment_pattern_contains_super(pattern: &AssignmentPattern, search: SuperSearch) -> bool {
    match pattern {
        AssignmentPattern::Target(expr) => expr_contains_super(expr, search),
        AssignmentPattern::Array(elements) => elements.iter().flatten().any(|element| {
            assignment_pattern_contains_super(&element.pattern, search)
                || element
                    .default
                    .as_ref()
                    .is_some_and(|expr| expr_contains_super(expr, search))
        }),
        AssignmentPattern::Object(properties) => properties.iter().any(|property| match property {
            AssignmentPatternProp::KeyValue {
                key,
                value,
                default,
            } => {
                property_key_contains_super(key, search)
                    || assignment_pattern_contains_super(value, search)
                    || default
                        .as_ref()
                        .is_some_and(|expr| expr_contains_super(expr, search))
            }
            AssignmentPatternProp::Rest(pattern) => {
                assignment_pattern_contains_super(pattern, search)
            }
        }),
    }
}

fn property_key_contains_super(key: &PropertyKey, search: SuperSearch) -> bool {
    matches!(key, PropertyKey::Computed(expr) if expr_contains_super(expr, search))
}

fn expr_contains_super(expr: &Expr, search: SuperSearch) -> bool {
    match expr {
        Expr::Number(_)
        | Expr::BigInt(_)
        | Expr::String(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::This
        | Expr::Identifier(_)
        | Expr::RegExp { .. }
        | Expr::Super
        | Expr::NewTarget => false,
        Expr::Template { expressions, .. } => expressions
            .iter()
            .any(|expr| expr_contains_super(expr, search)),
        Expr::TaggedTemplate {
            tag, expressions, ..
        } => {
            expr_contains_super(tag, search)
                || expressions
                    .iter()
                    .any(|expr| expr_contains_super(expr, search))
        }
        Expr::Array(elements) => elements.iter().flatten().any(|element| match element {
            ArrayElement::Normal(expr) | ArrayElement::Spread(expr) => {
                expr_contains_super(expr, search)
            }
        }),
        Expr::Object(properties) => properties.iter().any(|property| match property {
            ObjectProp::KeyValue { key, value, .. } => {
                property_key_contains_super(key, search) || expr_contains_super(value, search)
            }
            ObjectProp::Spread(expr) => expr_contains_super(expr, search),
            ObjectProp::Method { key, function } | ObjectProp::Accessor { key, function, .. } => {
                property_key_contains_super(key, search)
                    || (search == SuperSearch::Call && function_contains_super(function, search))
            }
        }),
        Expr::Function(function) => function_contains_super(function, search),
        Expr::Class(_) => false,
        Expr::Yield { value, .. } => value
            .as_deref()
            .is_some_and(|expr| expr_contains_super(expr, search)),
        Expr::Await(expr)
        | Expr::DynamicImport(expr)
        | Expr::Unary { arg: expr, .. }
        | Expr::Update { arg: expr, .. } => expr_contains_super(expr, search),
        Expr::Arrow { params, body, .. } => {
            params.iter().any(|param| {
                pattern_contains_super(&param.pattern, search)
                    || param
                        .default
                        .as_ref()
                        .is_some_and(|expr| expr_contains_super(expr, search))
            }) || match body {
                ArrowBody::Expr(expr) => expr_contains_super(expr, search),
                ArrowBody::Block(statements) => stmts_contain_super(statements, search),
            }
        }
        Expr::Binary { left, right, .. } | Expr::Logical { left, right, .. } => {
            expr_contains_super(left, search) || expr_contains_super(right, search)
        }
        Expr::Sequence(expressions) => expressions
            .iter()
            .any(|expr| expr_contains_super(expr, search)),
        Expr::Assign { target, value, .. } => {
            expr_contains_super(target, search) || expr_contains_super(value, search)
        }
        Expr::DestructureAssign { pattern, value } => {
            assignment_pattern_contains_super(pattern, search) || expr_contains_super(value, search)
        }
        Expr::Conditional {
            test,
            consequent,
            alternate,
        } => {
            expr_contains_super(test, search)
                || expr_contains_super(consequent, search)
                || expr_contains_super(alternate, search)
        }
        Expr::Call { callee, args } => {
            (search == SuperSearch::Call && matches!(callee.as_ref(), Expr::Super))
                || expr_contains_super(callee, search)
                || args.iter().any(|argument| match argument {
                    Argument::Normal(expr) | Argument::Spread(expr) => {
                        expr_contains_super(expr, search)
                    }
                })
        }
        Expr::New { callee, args } => {
            expr_contains_super(callee, search)
                || args.iter().any(|argument| match argument {
                    Argument::Normal(expr) | Argument::Spread(expr) => {
                        expr_contains_super(expr, search)
                    }
                })
        }
        Expr::Member {
            object, property, ..
        } => {
            (search == SuperSearch::Property && matches!(object.as_ref(), Expr::Super))
                || expr_contains_super(object, search)
                || expr_contains_super(property, search)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn super_member() -> Expr {
        Expr::Member {
            object: Box::new(Expr::Super),
            property: Box::new(Expr::Identifier("value".into())),
            computed: false,
        }
    }

    fn super_call() -> Expr {
        Expr::Call {
            callee: Box::new(Expr::Super),
            args: Vec::new(),
        }
    }

    fn function(body: Vec<Stmt>) -> Function {
        Function {
            name: None,
            params: Vec::new(),
            body,
            generator: false,
            is_async: false,
        }
    }

    #[test]
    fn super_early_error_scanners_visit_all_executable_container_shapes() {
        for body in [
            vec![Stmt::Switch {
                discriminant: Expr::Number(0.0),
                cases: vec![SwitchCase {
                    test: Some(super_member()),
                    consequent: Vec::new(),
                }],
            }],
            vec![Stmt::Switch {
                discriminant: Expr::Number(0.0),
                cases: vec![SwitchCase {
                    test: None,
                    consequent: vec![Stmt::Expr(super_member())],
                }],
            }],
            vec![Stmt::For {
                init: Some(ForInit::Expr(super_member())),
                test: None,
                update: None,
                body: Box::new(Stmt::Empty),
            }],
            vec![Stmt::ClassField(Box::new(Stmt::Expr(super_member())))],
        ] {
            assert!(function_contains_super_property_outside_class(&function(
                body
            )));
        }
        for expression in [
            Expr::Template {
                quasis: vec!["".into()],
                expressions: vec![super_member()],
            },
            Expr::Array(vec![Some(ArrayElement::Normal(super_member()))]),
            Expr::Array(vec![Some(ArrayElement::Spread(super_member()))]),
            Expr::Object(vec![ObjectProp::Spread(super_member())]),
            Expr::DestructureAssign {
                pattern: AssignmentPattern::Target(Box::new(super_member())),
                value: Box::new(Expr::Identifier("source".into())),
            },
            Expr::DestructureAssign {
                pattern: AssignmentPattern::Array(vec![Some(AssignmentPatternElement {
                    pattern: AssignmentPattern::Target(Box::new(Expr::Identifier("target".into()))),
                    default: Some(super_member()),
                    rest: false,
                })]),
                value: Box::new(Expr::Identifier("source".into())),
            },
            Expr::DestructureAssign {
                pattern: AssignmentPattern::Object(vec![AssignmentPatternProp::KeyValue {
                    key: PropertyKey::Computed(Box::new(super_member())),
                    value: AssignmentPattern::Target(Box::new(Expr::Identifier("target".into()))),
                    default: None,
                }]),
                value: Box::new(Expr::Identifier("source".into())),
            },
            Expr::DestructureAssign {
                pattern: AssignmentPattern::Object(vec![AssignmentPatternProp::KeyValue {
                    key: PropertyKey::Identifier("key".into()),
                    value: AssignmentPattern::Target(Box::new(Expr::Identifier("target".into()))),
                    default: Some(super_member()),
                }]),
                value: Box::new(Expr::Identifier("source".into())),
            },
            Expr::DestructureAssign {
                pattern: AssignmentPattern::Object(vec![AssignmentPatternProp::Rest(
                    AssignmentPattern::Target(Box::new(super_member())),
                )]),
                value: Box::new(Expr::Identifier("source".into())),
            },
        ] {
            assert!(expr_contains_super(&expression, SuperSearch::Property));
        }
        assert!(!expr_contains_super(
            &Expr::Class(Class {
                name: None,
                extends: None,
                elements: Vec::new()
            }),
            SuperSearch::Property
        ));
        assert!(function_contains_super_property_outside_class(&Function {
            name: None,
            params: vec![Param {
                pattern: Pattern::Identifier("parameter".into()),
                default: Some(super_member()),
                rest: false
            }],
            body: Vec::new(),
            generator: false,
            is_async: false
        }));
        assert!(contains_super_call_outside_class(&Program {
            body: vec![Stmt::Expr(Expr::Call {
                callee: Box::new(Expr::Super),
                args: Vec::new()
            })]
        }));
        assert!(expr_contains_super_call_outside_class(&Expr::New {
            callee: Box::new(Expr::Identifier("C".into())),
            args: vec![Argument::Normal(Expr::Call {
                callee: Box::new(Expr::Super),
                args: Vec::new()
            })]
        }));
    }

    #[test]
    fn super_call_scanner_visits_nested_control_and_pattern_shapes() {
        assert!(statements_contain_super_call_outside_class(&[Stmt::Expr(
            super_call()
        )]));
        let binding = Pattern::Array(vec![Some(ArrayPatternElement {
            pattern: Pattern::Identifier("value".into()),
            default: Some(super_call()),
            rest: false,
        })]);
        assert!(for_head_contains_super(
            &ForHead::Pattern(binding.clone()),
            SuperSearch::Call
        ));
        assert!(pattern_option_contains_super(
            Some(&binding),
            SuperSearch::Call
        ));
        let object_binding = Pattern::Object(vec![ObjectPatternProp::KeyValue {
            key: PropertyKey::Identifier("value".into()),
            value: Pattern::Identifier("target".into()),
            default: Some(super_call()),
        }]);
        assert!(pattern_contains_super(&object_binding, SuperSearch::Call));
        let computed_object_binding = Pattern::Object(vec![ObjectPatternProp::KeyValue {
            key: PropertyKey::Computed(Box::new(super_member())),
            value: Pattern::Identifier("target".into()),
            default: None,
        }]);
        assert!(pattern_contains_super(
            &computed_object_binding,
            SuperSearch::Property
        ));
        assert!(pattern_contains_super(
            &Pattern::Object(vec![ObjectPatternProp::Rest(Pattern::Array(vec![Some(
                ArrayPatternElement {
                    pattern: Pattern::Identifier("target".into()),
                    default: Some(super_call()),
                    rest: false,
                }
            )]))]),
            SuperSearch::Call,
        ));
        assert!(!pattern_contains_super(
            &Pattern::Object(vec![ObjectPatternProp::KeyValue {
                key: PropertyKey::Identifier("value".into()),
                value: Pattern::Identifier("target".into()),
                default: None,
            }]),
            SuperSearch::Call,
        ));
        assert!(assignment_pattern_contains_super(
            &AssignmentPattern::Array(vec![Some(AssignmentPatternElement {
                pattern: AssignmentPattern::Target(Box::new(Expr::Identifier("value".into()))),
                default: Some(super_call()),
                rest: false,
            })]),
            SuperSearch::Call,
        ));
        assert!(assignment_pattern_contains_super(
            &AssignmentPattern::Object(vec![AssignmentPatternProp::KeyValue {
                key: PropertyKey::Identifier("value".into()),
                value: AssignmentPattern::Target(Box::new(Expr::Identifier("target".into()))),
                default: Some(super_call()),
            }]),
            SuperSearch::Call,
        ));
        assert!(assignment_pattern_contains_super(
            &AssignmentPattern::Object(vec![AssignmentPatternProp::Rest(
                AssignmentPattern::Target(Box::new(super_call()))
            )]),
            SuperSearch::Call,
        ));

        for statement in [
            Stmt::VarDecl(
                DeclKind::Let,
                vec![VarDeclarator {
                    pattern: Pattern::Identifier("value".into()),
                    init: Some(super_call()),
                }],
            ),
            Stmt::If {
                test: Expr::Bool(false),
                consequent: Box::new(Stmt::Empty),
                alternate: Some(Box::new(Stmt::Expr(super_call()))),
            },
            Stmt::For {
                init: Some(ForInit::VarDecl(
                    DeclKind::Let,
                    vec![VarDeclarator {
                        pattern: Pattern::Identifier("value".into()),
                        init: Some(super_call()),
                    }],
                )),
                test: None,
                update: None,
                body: Box::new(Stmt::Empty),
            },
            Stmt::For {
                init: None,
                test: Some(super_call()),
                update: None,
                body: Box::new(Stmt::Empty),
            },
            Stmt::For {
                init: None,
                test: Some(Expr::Bool(false)),
                update: Some(super_call()),
                body: Box::new(Stmt::Empty),
            },
            Stmt::ForIn {
                left: ForHead::Pattern(Pattern::Identifier("value".into())),
                right: super_call(),
                body: Box::new(Stmt::Empty),
            },
            Stmt::ForOf {
                left: ForHead::Pattern(Pattern::Identifier("value".into())),
                right: Expr::Array(Vec::new()),
                body: Box::new(Stmt::Expr(super_call())),
            },
            Stmt::Switch {
                discriminant: Expr::Number(0.0),
                cases: vec![SwitchCase {
                    test: Some(super_call()),
                    consequent: Vec::new(),
                }],
            },
            Stmt::Switch {
                discriminant: Expr::Number(0.0),
                cases: vec![SwitchCase {
                    test: Some(Expr::Number(1.0)),
                    consequent: vec![Stmt::Expr(super_call())],
                }],
            },
            Stmt::Try {
                block: Vec::new(),
                handler: Some(CatchClause {
                    param: Some(binding),
                    body: Vec::new(),
                }),
                finalizer: None,
            },
            Stmt::Try {
                block: Vec::new(),
                handler: Some(CatchClause {
                    param: None,
                    body: vec![Stmt::Expr(super_call())],
                }),
                finalizer: None,
            },
            Stmt::Try {
                block: Vec::new(),
                handler: None,
                finalizer: Some(vec![Stmt::Expr(super_call())]),
            },
            Stmt::Block(vec![Stmt::Expr(super_call())]),
            Stmt::ClassField(Box::new(Stmt::Expr(super_call()))),
            Stmt::For {
                init: Some(ForInit::Expr(super_call())),
                test: None,
                update: None,
                body: Box::new(Stmt::Empty),
            },
            Stmt::For {
                init: None,
                test: None,
                update: None,
                body: Box::new(Stmt::Expr(super_call())),
            },
            Stmt::While {
                test: Expr::Bool(false),
                body: Box::new(Stmt::Expr(super_call())),
            },
            Stmt::DoWhile {
                body: Box::new(Stmt::Expr(super_call())),
                test: Expr::Bool(false),
            },
            Stmt::With {
                object: Expr::Bool(true),
                body: Box::new(Stmt::Expr(super_call())),
            },
            Stmt::With {
                object: super_call(),
                body: Box::new(Stmt::Empty),
            },
            Stmt::FunctionDecl(function(vec![Stmt::Expr(super_call())])),
        ] {
            assert!(
                stmt_contains_super(&statement, SuperSearch::Call),
                "{statement:?}"
            );
        }

        for expression in [
            Expr::Template {
                quasis: vec!["".into()],
                expressions: vec![super_call()],
            },
            Expr::TaggedTemplate {
                tag: Box::new(Expr::Identifier("tag".into())),
                raw: vec!["".into()],
                cooked: vec![Some("".into())],
                expressions: vec![super_call()],
            },
            Expr::Yield {
                value: Some(Box::new(super_call())),
                delegate: false,
            },
            Expr::Arrow {
                params: vec![Param {
                    pattern: Pattern::Identifier("value".into()),
                    default: Some(super_call()),
                    rest: false,
                }],
                body: ArrowBody::Expr(Box::new(Expr::Number(0.0))),
                is_async: false,
            },
            Expr::Sequence(vec![super_call()]),
            Expr::Call {
                callee: Box::new(Expr::Identifier("call".into())),
                args: vec![
                    Argument::Normal(super_call()),
                    Argument::Spread(super_call()),
                ],
            },
            Expr::New {
                callee: Box::new(Expr::Identifier("Constructor".into())),
                args: vec![
                    Argument::Normal(super_call()),
                    Argument::Spread(super_call()),
                ],
            },
            Expr::Object(vec![ObjectProp::KeyValue {
                key: PropertyKey::Identifier("value".into()),
                value: super_call(),
                shorthand: false,
            }]),
            Expr::Object(vec![ObjectProp::Spread(super_call())]),
            Expr::Object(vec![ObjectProp::Method {
                key: PropertyKey::Identifier("method".into()),
                function: function(vec![Stmt::Expr(super_call())]),
            }]),
            Expr::Object(vec![ObjectProp::Accessor {
                key: PropertyKey::Identifier("value".into()),
                function: function(vec![Stmt::Expr(super_call())]),
                getter: true,
            }]),
            Expr::Function(function(vec![Stmt::Expr(super_call())])),
            Expr::Await(Box::new(super_call())),
            Expr::Unary {
                op: UnaryOp::Void,
                arg: Box::new(super_call()),
            },
            Expr::Update {
                op: UpdateOp::Inc,
                arg: Box::new(super_call()),
                prefix: true,
            },
            Expr::Arrow {
                params: Vec::new(),
                body: ArrowBody::Expr(Box::new(super_call())),
                is_async: false,
            },
            Expr::Arrow {
                params: Vec::new(),
                body: ArrowBody::Block(vec![Stmt::Expr(super_call())]),
                is_async: false,
            },
        ] {
            assert!(
                expr_contains_super_call_outside_class(&expression),
                "{expression:?}"
            );
        }
        assert!(!expr_contains_super_call_outside_class(&Expr::Class(
            Class {
                name: None,
                extends: None,
                elements: Vec::new()
            }
        )));
        let property = Expr::Member {
            object: Box::new(Expr::Identifier("object".into())),
            property: Box::new(super_member()),
            computed: true,
        };
        assert!(expr_contains_super(&property, SuperSearch::Property));
    }
}
