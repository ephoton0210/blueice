// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The syntax tree of a GLSL ES translation unit.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scalar {
    Float,
    Int,
    UInt,
    Bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SamplerKind {
    S2D,
    Cube,
    S3D,
    S2DArray,
    S2DShadow,
    CubeShadow,
    IS2D,
    US2D,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Ty {
    Void,
    Scalar(Scalar),
    Vec(Scalar, u8),
    /// `Mat(columns, rows)`.
    Mat(u8, u8),
    Sampler(SamplerKind),
    Struct(String),
    Array(Box<Ty>, usize),
}

impl Ty {
    pub const FLOAT: Ty = Ty::Scalar(Scalar::Float);
    pub const INT: Ty = Ty::Scalar(Scalar::Int);
    pub const BOOL: Ty = Ty::Scalar(Scalar::Bool);
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Literal {
    Float(f32),
    Int(i32),
    UInt(u32),
    Bool(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Plus,
    Not,
    BitNot,
    PreInc,
    PreDec,
    PostInc,
    PostDec,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Shl,
    Shr,
    Lt,
    Gt,
    Le,
    Ge,
    Eq,
    Ne,
    BitAnd,
    BitXor,
    BitOr,
    And,
    Xor,
    Or,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Lit(Literal),
    Var(String),
    Field(Box<Expr>, String),
    Index(Box<Expr>, Box<Expr>),
    /// A function call, a constructor (`vec3(..)`, a struct name) or a built-in.
    Call(String, Vec<Expr>),
    /// `float[3](a, b, c)`.
    ArrayCtor(Ty, Vec<Expr>),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    /// `a = b` (`None`) or `a += b` and friends.
    Assign(Option<BinOp>, Box<Expr>, Box<Expr>),
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
    Comma(Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct VarDecl {
    pub ty: Ty,
    pub name: String,
    pub init: Option<Expr>,
    pub constant: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Decl(Vec<VarDecl>),
    Expr(Expr),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>),
    For(Option<Box<Stmt>>, Option<Expr>, Option<Expr>, Box<Stmt>),
    While(Expr, Box<Stmt>),
    DoWhile(Box<Stmt>, Expr),
    Switch(Expr, Vec<(Option<Expr>, Vec<Stmt>)>),
    Return(Option<Expr>),
    Break,
    Continue,
    Discard,
    Block(Vec<Stmt>),
    Empty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Storage {
    None,
    Const,
    Attribute,
    Varying,
    Uniform,
    In,
    Out,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GlobalVar {
    pub storage: Storage,
    pub ty: Ty,
    pub name: String,
    pub init: Option<Expr>,
    pub flat: bool,
    pub location: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamDir {
    In,
    Out,
    InOut,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub dir: ParamDir,
    pub ty: Ty,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    pub name: String,
    pub ret: Ty,
    pub params: Vec<Param>,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructDef {
    pub name: String,
    pub fields: Vec<(String, Ty)>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Unit {
    pub structs: Vec<StructDef>,
    pub globals: Vec<GlobalVar>,
    pub functions: Vec<Function>,
}
