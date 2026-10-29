// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The GLSL ES parser: a preprocessed source to a [`Unit`].

use super::ast::*;
use super::lex::{lex, Tok, Token};

pub fn builtin_type(name: &str) -> Option<Ty> {
    use Scalar::*;
    Some(match name {
        "void" => Ty::Void,
        "float" => Ty::Scalar(Float),
        "int" => Ty::Scalar(Int),
        "uint" => Ty::Scalar(UInt),
        "bool" => Ty::Scalar(Bool),
        "vec2" => Ty::Vec(Float, 2),
        "vec3" => Ty::Vec(Float, 3),
        "vec4" => Ty::Vec(Float, 4),
        "ivec2" => Ty::Vec(Int, 2),
        "ivec3" => Ty::Vec(Int, 3),
        "ivec4" => Ty::Vec(Int, 4),
        "uvec2" => Ty::Vec(UInt, 2),
        "uvec3" => Ty::Vec(UInt, 3),
        "uvec4" => Ty::Vec(UInt, 4),
        "bvec2" => Ty::Vec(Bool, 2),
        "bvec3" => Ty::Vec(Bool, 3),
        "bvec4" => Ty::Vec(Bool, 4),
        "mat2" | "mat2x2" => Ty::Mat(2, 2),
        "mat3" | "mat3x3" => Ty::Mat(3, 3),
        "mat4" | "mat4x4" => Ty::Mat(4, 4),
        "mat2x3" => Ty::Mat(2, 3),
        "mat2x4" => Ty::Mat(2, 4),
        "mat3x2" => Ty::Mat(3, 2),
        "mat3x4" => Ty::Mat(3, 4),
        "mat4x2" => Ty::Mat(4, 2),
        "mat4x3" => Ty::Mat(4, 3),
        "sampler2D" => Ty::Sampler(SamplerKind::S2D),
        "samplerCube" => Ty::Sampler(SamplerKind::Cube),
        "sampler3D" => Ty::Sampler(SamplerKind::S3D),
        "sampler2DArray" => Ty::Sampler(SamplerKind::S2DArray),
        "sampler2DShadow" => Ty::Sampler(SamplerKind::S2DShadow),
        "samplerCubeShadow" => Ty::Sampler(SamplerKind::CubeShadow),
        "isampler2D" => Ty::Sampler(SamplerKind::IS2D),
        "usampler2D" => Ty::Sampler(SamplerKind::US2D),
        _ => return None,
    })
}

const PRECISIONS: [&str; 3] = ["highp", "mediump", "lowp"];

struct Parser {
    toks: Vec<Token>,
    at: usize,
    structs: Vec<StructDef>,
    depth: usize,
}

type R<T> = Result<T, String>;

pub fn parse(source: &str) -> R<Unit> {
    let toks = lex(source)?;
    let mut p = Parser {
        toks,
        at: 0,
        structs: Vec::new(),
        depth: 0,
    };
    let mut unit = Unit::default();
    while p.at < p.toks.len() {
        p.external(&mut unit)?;
    }
    unit.structs = p.structs;
    Ok(unit)
}

impl Parser {
    fn line(&self) -> u32 {
        self.toks
            .get(self.at.min(self.toks.len().saturating_sub(1)))
            .map_or(0, |t| t.line)
    }
    fn err<T>(&self, message: impl Into<String>) -> R<T> {
        Err(format!("line {}: {}", self.line(), message.into()))
    }
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.at).map(|t| &t.tok)
    }
    fn peek_at(&self, n: usize) -> Option<&Tok> {
        self.toks.get(self.at + n).map(|t| &t.tok)
    }
    fn is(&self, p: &str) -> bool {
        matches!(self.peek(), Some(Tok::Punct(q)) if *q == p)
    }
    fn is_word(&self, w: &str) -> bool {
        matches!(self.peek(), Some(Tok::Ident(i)) if i == w)
    }
    fn eat(&mut self, p: &str) -> bool {
        if self.is(p) {
            self.at += 1;
            true
        } else {
            false
        }
    }
    fn eat_word(&mut self, w: &str) -> bool {
        if self.is_word(w) {
            self.at += 1;
            true
        } else {
            false
        }
    }
    fn expect(&mut self, p: &str) -> R<()> {
        if self.eat(p) {
            Ok(())
        } else {
            self.err(format!("expected '{p}', found {:?}", self.peek()))
        }
    }
    fn ident(&mut self) -> R<String> {
        match self.peek().cloned() {
            Some(Tok::Ident(i)) => {
                self.at += 1;
                Ok(i)
            }
            other => self.err(format!("expected a name, found {other:?}")),
        }
    }

    fn is_type_start(&self) -> bool {
        match self.peek() {
            Some(Tok::Ident(i)) => {
                builtin_type(i).is_some()
                    || self.structs.iter().any(|s| &s.name == i)
                    || PRECISIONS.contains(&i.as_str())
                    || i == "const"
                    || i == "struct"
            }
            _ => false,
        }
    }

    /// A type specifier, with an optional array suffix on the type itself
    /// (`float[3]`).
    fn type_spec(&mut self) -> R<Ty> {
        while matches!(self.peek(), Some(Tok::Ident(i)) if PRECISIONS.contains(&i.as_str())) {
            self.at += 1;
        }
        let name = self.ident()?;
        let base = match builtin_type(&name) {
            Some(t) => t,
            None if self.structs.iter().any(|s| s.name == name) => Ty::Struct(name),
            None => return self.err(format!("unknown type '{name}'")),
        };
        self.array_suffix(base)
    }

    fn array_suffix(&mut self, base: Ty) -> R<Ty> {
        let mut ty = base;
        while self.eat("[") {
            let n = match self.peek().cloned() {
                Some(Tok::Int(n)) if n > 0 => {
                    self.at += 1;
                    n as usize
                }
                // An unsized `[]` (sized by its initializer) is recorded as 0.
                _ if self.is("]") => 0,
                other => return self.err(format!("array size must be a constant, found {other:?}")),
            };
            self.expect("]")?;
            ty = Ty::Array(Box::new(ty), n);
        }
        Ok(ty)
    }

    fn struct_def(&mut self) -> R<String> {
        self.at += 1; // struct
        let name = if matches!(self.peek(), Some(Tok::Ident(_))) {
            self.ident()?
        } else {
            format!("_anonymous{}", self.structs.len())
        };
        self.expect("{")?;
        let mut fields = Vec::new();
        while !self.eat("}") {
            let base = self.type_spec()?;
            loop {
                let field = self.ident()?;
                let ty = self.array_suffix(base.clone())?;
                fields.push((field, ty));
                if !self.eat(",") {
                    break;
                }
            }
            self.expect(";")?;
        }
        self.structs.push(StructDef {
            name: name.clone(),
            fields,
        });
        Ok(name)
    }

    fn external(&mut self, unit: &mut Unit) -> R<()> {
        if self.eat(";") {
            return Ok(());
        }
        // precision highp float;
        if self.is_word("precision") {
            while !self.eat(";") {
                if self.peek().is_none() {
                    return self.err("unterminated precision statement");
                }
                self.at += 1;
            }
            return Ok(());
        }
        // Qualifiers.
        let mut storage = Storage::None;
        let mut flat = false;
        let mut location = None;
        loop {
            let Some(Tok::Ident(word)) = self.peek().cloned() else {
                break;
            };
            match word.as_str() {
                "attribute" => storage = Storage::Attribute,
                "varying" => storage = Storage::Varying,
                "uniform" => storage = Storage::Uniform,
                "in" => storage = Storage::In,
                "out" => storage = Storage::Out,
                "const" => storage = Storage::Const,
                "flat" => flat = true,
                "smooth" | "centroid" | "invariant" | "highp" | "mediump" | "lowp" => {}
                "layout" => {
                    self.at += 1;
                    self.expect("(")?;
                    while !self.eat(")") {
                        let key = self.ident()?;
                        if self.eat("=") {
                            if let Some(Tok::Int(n)) = self.peek().cloned() {
                                self.at += 1;
                                if key == "location" {
                                    location = Some(n as u32);
                                }
                            }
                        }
                        self.eat(",");
                    }
                    continue;
                }
                _ => break,
            }
            self.at += 1;
        }
        // struct S { .. } [vars];
        if self.is_word("struct") {
            let name = self.struct_def()?;
            if self.eat(";") {
                return Ok(());
            }
            let ty = self.array_suffix(Ty::Struct(name))?;
            return self.global_declarators(unit, storage, ty, flat, location);
        }
        // `uniform Block { float a; vec3 b; } instance;` -- the members are
        // read as plain uniforms.
        if storage == Storage::Uniform
            && matches!(self.peek(), Some(Tok::Ident(i)) if builtin_type(i).is_none() && !self.structs.iter().any(|s| &s.name == i))
            && matches!(self.peek_at(1), Some(Tok::Punct("{")))
        {
            self.at += 2;
            while !self.eat("}") {
                let base = self.type_spec()?;
                loop {
                    let name = self.ident()?;
                    let ty = self.array_suffix(base.clone())?;
                    unit.globals.push(GlobalVar {
                        storage: Storage::Uniform,
                        ty,
                        name,
                        init: None,
                        flat: false,
                        location: None,
                    });
                    if !self.eat(",") {
                        break;
                    }
                }
                self.expect(";")?;
            }
            if matches!(self.peek(), Some(Tok::Ident(_))) {
                self.at += 1;
            }
            self.expect(";")?;
            return Ok(());
        }
        let ty = self.type_spec()?;
        let name = self.ident()?;
        if self.is("(") {
            return self.function(unit, ty, name);
        }
        self.at -= 1;
        self.global_declarators(unit, storage, ty, flat, location)
    }

    fn global_declarators(
        &mut self,
        unit: &mut Unit,
        storage: Storage,
        base: Ty,
        flat: bool,
        location: Option<u32>,
    ) -> R<()> {
        loop {
            let name = self.ident()?;
            let ty = self.array_suffix(base.clone())?;
            let init = if self.eat("=") {
                Some(self.assignment()?)
            } else {
                None
            };
            let ty = match (&ty, &init) {
                (Ty::Array(inner, 0), Some(Expr::ArrayCtor(Ty::Array(_, n), _))) => {
                    Ty::Array(inner.clone(), *n)
                }
                _ => ty,
            };
            unit.globals.push(GlobalVar {
                storage,
                ty,
                name,
                init,
                flat,
                location,
            });
            if !self.eat(",") {
                break;
            }
        }
        self.expect(";")
    }

    fn function(&mut self, unit: &mut Unit, ret: Ty, name: String) -> R<()> {
        self.expect("(")?;
        let mut params = Vec::new();
        if self.is_word("void") && matches!(self.peek_at(1), Some(Tok::Punct(")"))) {
            self.at += 1;
        }
        while !self.is(")") {
            let mut dir = ParamDir::In;
            loop {
                match self.peek() {
                    Some(Tok::Ident(w)) if w == "in" => dir = ParamDir::In,
                    Some(Tok::Ident(w)) if w == "out" => dir = ParamDir::Out,
                    Some(Tok::Ident(w)) if w == "inout" => dir = ParamDir::InOut,
                    Some(Tok::Ident(w)) if w == "const" => {}
                    _ => break,
                }
                self.at += 1;
            }
            let base = self.type_spec()?;
            let pname = if matches!(self.peek(), Some(Tok::Ident(_))) {
                self.ident()?
            } else {
                String::new()
            };
            let ty = self.array_suffix(base)?;
            params.push(Param { dir, ty, name: pname });
            if !self.eat(",") {
                break;
            }
        }
        self.expect(")")?;
        if self.eat(";") {
            return Ok(()); // a prototype
        }
        self.expect("{")?;
        let body = self.block_body()?;
        unit.functions.push(Function {
            name,
            ret,
            params,
            body,
        });
        Ok(())
    }

    fn block_body(&mut self) -> R<Vec<Stmt>> {
        let mut out = Vec::new();
        while !self.eat("}") {
            if self.peek().is_none() {
                return self.err("unterminated block");
            }
            out.push(self.statement()?);
        }
        Ok(out)
    }

    fn statement(&mut self) -> R<Stmt> {
        self.depth += 1;
        if self.depth > 200 {
            return self.err("statements are nested too deeply");
        }
        let result = self.statement_inner();
        self.depth -= 1;
        result
    }

    fn statement_inner(&mut self) -> R<Stmt> {
        if self.eat("{") {
            return Ok(Stmt::Block(self.block_body()?));
        }
        if self.eat(";") {
            return Ok(Stmt::Empty);
        }
        if let Some(Tok::Ident(word)) = self.peek().cloned() {
            match word.as_str() {
                "if" => {
                    self.at += 1;
                    self.expect("(")?;
                    let cond = self.expression()?;
                    self.expect(")")?;
                    let then = Box::new(self.statement()?);
                    let otherwise = if self.eat_word("else") {
                        Some(Box::new(self.statement()?))
                    } else {
                        None
                    };
                    return Ok(Stmt::If(cond, then, otherwise));
                }
                "for" => {
                    self.at += 1;
                    self.expect("(")?;
                    let init = if self.eat(";") {
                        None
                    } else {
                        Some(Box::new(self.simple_statement()?))
                    };
                    let cond = if self.is(";") { None } else { Some(self.expression()?) };
                    self.expect(";")?;
                    let step = if self.is(")") { None } else { Some(self.expression()?) };
                    self.expect(")")?;
                    return Ok(Stmt::For(init, cond, step, Box::new(self.statement()?)));
                }
                "while" => {
                    self.at += 1;
                    self.expect("(")?;
                    let cond = self.expression()?;
                    self.expect(")")?;
                    return Ok(Stmt::While(cond, Box::new(self.statement()?)));
                }
                "do" => {
                    self.at += 1;
                    let body = Box::new(self.statement()?);
                    if !self.eat_word("while") {
                        return self.err("expected 'while' after a do body");
                    }
                    self.expect("(")?;
                    let cond = self.expression()?;
                    self.expect(")")?;
                    self.expect(";")?;
                    return Ok(Stmt::DoWhile(body, cond));
                }
                "switch" => {
                    self.at += 1;
                    self.expect("(")?;
                    let value = self.expression()?;
                    self.expect(")")?;
                    self.expect("{")?;
                    let mut cases: Vec<(Option<Expr>, Vec<Stmt>)> = Vec::new();
                    while !self.eat("}") {
                        let label = if self.eat_word("case") {
                            let e = self.expression()?;
                            self.expect(":")?;
                            Some(e)
                        } else if self.eat_word("default") {
                            self.expect(":")?;
                            None
                        } else {
                            return self.err("expected 'case' or 'default'");
                        };
                        let mut body = Vec::new();
                        while !self.is_word("case") && !self.is_word("default") && !self.is("}") {
                            body.push(self.statement()?);
                        }
                        cases.push((label, body));
                    }
                    return Ok(Stmt::Switch(value, cases));
                }
                "return" => {
                    self.at += 1;
                    let value = if self.is(";") { None } else { Some(self.expression()?) };
                    self.expect(";")?;
                    return Ok(Stmt::Return(value));
                }
                "break" => {
                    self.at += 1;
                    self.expect(";")?;
                    return Ok(Stmt::Break);
                }
                "continue" => {
                    self.at += 1;
                    self.expect(";")?;
                    return Ok(Stmt::Continue);
                }
                "discard" => {
                    self.at += 1;
                    self.expect(";")?;
                    return Ok(Stmt::Discard);
                }
                _ => {}
            }
        }
        self.simple_statement()
    }

    /// A declaration or an expression statement, with its `;`.
    fn simple_statement(&mut self) -> R<Stmt> {
        if self.is_type_start() && !self.is_constructor_call() {
            let mut constant = false;
            while self.eat_word("const") {
                constant = true;
            }
            if self.is_word("struct") {
                self.struct_def()?;
                if self.eat(";") {
                    return Ok(Stmt::Empty);
                }
            }
            let base = self.type_spec()?;
            let mut decls = Vec::new();
            loop {
                let name = self.ident()?;
                let ty = self.array_suffix(base.clone())?;
                let init = if self.eat("=") {
                    Some(self.assignment()?)
                } else {
                    None
                };
                let ty = match (&ty, &init) {
                    (Ty::Array(inner, 0), Some(Expr::ArrayCtor(Ty::Array(_, n), _))) => {
                        Ty::Array(inner.clone(), *n)
                    }
                    _ => ty,
                };
                decls.push(VarDecl {
                    ty,
                    name,
                    init,
                    constant,
                });
                if !self.eat(",") {
                    break;
                }
            }
            self.expect(";")?;
            return Ok(Stmt::Decl(decls));
        }
        let e = self.expression()?;
        self.expect(";")?;
        Ok(Stmt::Expr(e))
    }

    /// `vec3(1.0).x;` starts like a declaration but is an expression.
    fn is_constructor_call(&self) -> bool {
        matches!(self.peek(), Some(Tok::Ident(i)) if (builtin_type(i).is_some() || self.structs.iter().any(|s| &s.name == i)))
            && matches!(self.peek_at(1), Some(Tok::Punct("(")))
    }

    // ---- expressions ----

    fn expression(&mut self) -> R<Expr> {
        let mut e = self.assignment()?;
        while self.eat(",") {
            let r = self.assignment()?;
            e = Expr::Comma(Box::new(e), Box::new(r));
        }
        Ok(e)
    }

    fn assignment(&mut self) -> R<Expr> {
        self.depth += 1;
        if self.depth > 200 {
            return self.err("expression is nested too deeply");
        }
        let result = (|| {
            let left = self.ternary()?;
            let op = match self.peek() {
                Some(Tok::Punct("=")) => Some(None),
                Some(Tok::Punct("+=")) => Some(Some(BinOp::Add)),
                Some(Tok::Punct("-=")) => Some(Some(BinOp::Sub)),
                Some(Tok::Punct("*=")) => Some(Some(BinOp::Mul)),
                Some(Tok::Punct("/=")) => Some(Some(BinOp::Div)),
                Some(Tok::Punct("%=")) => Some(Some(BinOp::Rem)),
                Some(Tok::Punct("<<=")) => Some(Some(BinOp::Shl)),
                Some(Tok::Punct(">>=")) => Some(Some(BinOp::Shr)),
                Some(Tok::Punct("&=")) => Some(Some(BinOp::BitAnd)),
                Some(Tok::Punct("|=")) => Some(Some(BinOp::BitOr)),
                Some(Tok::Punct("^=")) => Some(Some(BinOp::BitXor)),
                _ => None,
            };
            match op {
                Some(op) => {
                    self.at += 1;
                    let right = self.assignment()?;
                    Ok(Expr::Assign(op, Box::new(left), Box::new(right)))
                }
                None => Ok(left),
            }
        })();
        self.depth -= 1;
        result
    }

    fn ternary(&mut self) -> R<Expr> {
        let cond = self.binary(0)?;
        if self.eat("?") {
            let a = self.assignment()?;
            self.expect(":")?;
            let b = self.assignment()?;
            return Ok(Expr::Ternary(Box::new(cond), Box::new(a), Box::new(b)));
        }
        Ok(cond)
    }

    fn binary_op(&self) -> Option<(BinOp, u8)> {
        let Some(Tok::Punct(p)) = self.peek() else {
            return None;
        };
        Some(match *p {
            "||" => (BinOp::Or, 1),
            "^^" => (BinOp::Xor, 2),
            "&&" => (BinOp::And, 3),
            "|" => (BinOp::BitOr, 4),
            "^" => (BinOp::BitXor, 5),
            "&" => (BinOp::BitAnd, 6),
            "==" => (BinOp::Eq, 7),
            "!=" => (BinOp::Ne, 7),
            "<" => (BinOp::Lt, 8),
            ">" => (BinOp::Gt, 8),
            "<=" => (BinOp::Le, 8),
            ">=" => (BinOp::Ge, 8),
            "<<" => (BinOp::Shl, 9),
            ">>" => (BinOp::Shr, 9),
            "+" => (BinOp::Add, 10),
            "-" => (BinOp::Sub, 10),
            "*" => (BinOp::Mul, 11),
            "/" => (BinOp::Div, 11),
            "%" => (BinOp::Rem, 11),
            _ => return None,
        })
    }

    fn binary(&mut self, min: u8) -> R<Expr> {
        let mut left = self.unary()?;
        while let Some((op, prec)) = self.binary_op() {
            if prec < min {
                break;
            }
            self.at += 1;
            let right = self.binary(prec + 1)?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn unary(&mut self) -> R<Expr> {
        let op = match self.peek() {
            Some(Tok::Punct("-")) => Some(UnOp::Neg),
            Some(Tok::Punct("+")) => Some(UnOp::Plus),
            Some(Tok::Punct("!")) => Some(UnOp::Not),
            Some(Tok::Punct("~")) => Some(UnOp::BitNot),
            Some(Tok::Punct("++")) => Some(UnOp::PreInc),
            Some(Tok::Punct("--")) => Some(UnOp::PreDec),
            _ => None,
        };
        if let Some(op) = op {
            self.at += 1;
            self.depth += 1;
            if self.depth > 200 {
                return self.err("expression is nested too deeply");
            }
            let operand = self.unary();
            self.depth -= 1;
            return Ok(Expr::Unary(op, Box::new(operand?)));
        }
        self.postfix()
    }

    fn postfix(&mut self) -> R<Expr> {
        let mut e = self.primary()?;
        loop {
            if self.eat(".") {
                let field = self.ident()?;
                e = Expr::Field(Box::new(e), field);
            } else if self.eat("[") {
                let index = self.expression()?;
                self.expect("]")?;
                e = Expr::Index(Box::new(e), Box::new(index));
            } else if self.eat("++") {
                e = Expr::Unary(UnOp::PostInc, Box::new(e));
            } else if self.eat("--") {
                e = Expr::Unary(UnOp::PostDec, Box::new(e));
            } else {
                return Ok(e);
            }
        }
    }

    fn arguments(&mut self) -> R<Vec<Expr>> {
        self.expect("(")?;
        let mut args = Vec::new();
        if self.is_word("void") && matches!(self.peek_at(1), Some(Tok::Punct(")"))) {
            self.at += 1;
        }
        while !self.is(")") {
            args.push(self.assignment()?);
            if !self.eat(",") {
                break;
            }
        }
        self.expect(")")?;
        Ok(args)
    }

    fn primary(&mut self) -> R<Expr> {
        match self.peek().cloned() {
            Some(Tok::Int(n)) => {
                self.at += 1;
                Ok(Expr::Lit(Literal::Int(n as i32)))
            }
            Some(Tok::UInt(n)) => {
                self.at += 1;
                Ok(Expr::Lit(Literal::UInt(n)))
            }
            Some(Tok::Float(f)) => {
                self.at += 1;
                Ok(Expr::Lit(Literal::Float(f)))
            }
            Some(Tok::Punct("(")) => {
                self.at += 1;
                let e = self.expression()?;
                self.expect(")")?;
                Ok(e)
            }
            Some(Tok::Ident(word)) => {
                match word.as_str() {
                    "true" => {
                        self.at += 1;
                        return Ok(Expr::Lit(Literal::Bool(true)));
                    }
                    "false" => {
                        self.at += 1;
                        return Ok(Expr::Lit(Literal::Bool(false)));
                    }
                    _ => {}
                }
                // `float[3](...)`: an array constructor.
                if builtin_type(&word).is_some() && matches!(self.peek_at(1), Some(Tok::Punct("["))) {
                    let ty = self.type_spec()?;
                    let args = self.arguments()?;
                    return Ok(Expr::ArrayCtor(ty, args));
                }
                self.at += 1;
                if self.is("(") {
                    let args = self.arguments()?;
                    Ok(Expr::Call(word, args))
                } else {
                    Ok(Expr::Var(word))
                }
            }
            other => self.err(format!("unexpected {other:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(src: &str) -> Unit {
        parse(src).unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn a_gles1_vertex_shader_parses() {
        let u = unit(
            "precision highp float; attribute vec3 position; uniform mat4 mvp; varying vec2 uv;\n\
             void main() { uv = position.xy * 0.5 + 0.5; gl_Position = mvp * vec4(position, 1.0); }",
        );
        assert_eq!(u.globals.len(), 3);
        assert_eq!(u.globals[0].storage, Storage::Attribute);
        assert_eq!(u.globals[1].ty, Ty::Mat(4, 4));
        assert_eq!(u.functions[0].name, "main");
        assert_eq!(u.functions[0].body.len(), 2);
    }

    #[test]
    fn a_gles3_fragment_shader_with_layout_and_flat_parses() {
        let u = unit(
            "layout(location = 0) out highp vec4 outColor; flat in int id; in vec2 vUv; uniform sampler2D map;\n\
             void main() { outColor = texture(map, vUv); }",
        );
        assert_eq!(u.globals[0].location, Some(0));
        assert_eq!(u.globals[0].storage, Storage::Out);
        assert!(u.globals[1].flat);
        assert_eq!(u.globals[3].ty, Ty::Sampler(SamplerKind::S2D));
    }

    #[test]
    fn structs_arrays_functions_and_control_flow_parse() {
        let u = unit(
            "struct Light { vec3 pos; float power[2]; };\n\
             uniform Light lights[3]; const float K = 2.0; float arr[3] = float[3](1.0, 2.0, 3.0);\n\
             float f(in float a, out float b, inout vec2 c) { b = a; return a * 2.0; }\n\
             void main() {\n\
               float s = 0.0;\n\
               for (int i = 0; i < 3; i++) { if (i == 1) continue; s += lights[i].pos.x; }\n\
               while (s > 10.0) { s -= 1.0; if (s < 5.0) break; }\n\
               do { s++; } while (s < 3.0);\n\
               switch (int(s)) { case 1: s = 2.0; break; default: s = 0.0; }\n\
               vec3 v = vec3(1.0).xzy; v.x += s > 1.0 ? 1.0 : 2.0;\n\
               if (s < 0.0) discard;\n\
             }",
        );
        assert_eq!(u.structs[0].name, "Light");
        assert_eq!(u.structs[0].fields[1].1, Ty::Array(Box::new(Ty::FLOAT), 2));
        assert_eq!(u.globals[0].ty, Ty::Array(Box::new(Ty::Struct("Light".into())), 3));
        assert_eq!(u.globals[2].ty, Ty::Array(Box::new(Ty::FLOAT), 3));
        let f = &u.functions[0];
        assert_eq!(f.params[1].dir, ParamDir::Out);
        assert_eq!(f.params[2].dir, ParamDir::InOut);
        assert_eq!(u.functions[1].body.len(), 8);
    }

    #[test]
    fn operator_precedence_and_associativity() {
        let u = unit("void main() { float x = 1.0 + 2.0 * 3.0 - 4.0; bool b = 1 < 2 && 3 > 2 || false; x = y = 2.0; }");
        let Stmt::Decl(d) = &u.functions[0].body[0] else { panic!() };
        // ((1 + (2 * 3)) - 4)
        let Some(Expr::Binary(BinOp::Sub, l, _)) = &d[0].init else { panic!("{:?}", d[0].init) };
        assert!(matches!(**l, Expr::Binary(BinOp::Add, _, _)));
        let Stmt::Decl(d) = &u.functions[0].body[1] else { panic!() };
        assert!(matches!(d[0].init, Some(Expr::Binary(BinOp::Or, _, _))));
        let Stmt::Expr(Expr::Assign(None, _, r)) = &u.functions[0].body[2] else { panic!() };
        assert!(matches!(**r, Expr::Assign(None, _, _)), "assignment is right-associative");
    }

    #[test]
    fn uniform_blocks_expose_their_members_as_uniforms() {
        let u = unit("uniform Block { vec4 color; float k[2]; } block; void main(){}");
        assert_eq!(u.globals.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(), vec!["color", "k"]);
    }

    #[test]
    fn declarations_with_several_names_and_constructor_statements() {
        let u = unit("void main() { float a = 1.0, b[2], c; vec3(1.0).x; mat2 m = mat2(1.0); }");
        let Stmt::Decl(d) = &u.functions[0].body[0] else { panic!() };
        assert_eq!(d.len(), 3);
        assert_eq!(d[1].ty, Ty::Array(Box::new(Ty::FLOAT), 2));
        assert!(matches!(u.functions[0].body[1], Stmt::Expr(_)));
    }

    #[test]
    fn syntax_errors_name_the_line() {
        let e = parse("void main() {\n  float x = ;\n}").unwrap_err();
        assert!(e.starts_with("line 2"), "{e}");
        assert!(parse("void main() { float x = 1.0 }").is_err());
        assert!(parse("void main() { unknown_type x; }").is_err());
        assert!(parse(&format!("void main() {{ float x = {}1.0{}; }}", "(".repeat(300), ")".repeat(300))).is_err());
    }

    #[test]
    fn prototypes_and_empty_parameter_lists() {
        let u = unit("float f(float x); float f(float x) { return x; } void main(void) { f(1.0); }");
        assert_eq!(u.functions.len(), 2);
    }
}
