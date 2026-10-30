// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Compiling a parsed shader to a resolved tree and running its `main`.
//!
//! Names are resolved once, at compile time, to slots, so running a shader
//! per vertex and per fragment does no lookups by name. Values are typed
//! dynamically (see [`super::value`]); declarations, assignments, arguments
//! and returns convert to the declared type.

use super::ast::*;
use super::builtins::{self, Textures};
use super::parse::{builtin_type, parse};
use super::pre::preprocess;
use super::value::*;
use std::collections::HashMap;

/// Most loop iterations plus calls one invocation may take: a shader that does
/// not finish ends with an error instead of hanging the page.
const STEP_LIMIT: u64 = 20_000_000;

#[derive(Debug, Clone)]
enum N {
    Lit(V),
    Local(usize),
    Global(usize),
    Field(Box<N>, String),
    Index(Box<N>, Box<N>),
    CallUser(usize, Vec<N>),
    CallBuiltin(String, Vec<N>),
    Ctor(Ty, Vec<N>),
    ArrCtor(Ty, Vec<N>),
    Un(UnOp, Box<N>),
    Bin(BinOp, Box<N>, Box<N>),
    Assign(Option<BinOp>, Box<N>, Box<N>),
    Cond(Box<N>, Box<N>, Box<N>),
    Comma(Box<N>, Box<N>),
}

#[derive(Debug, Clone)]
enum S {
    Expr(N),
    Decl(usize, Ty, Option<N>),
    If(N, Box<S>, Option<Box<S>>),
    For(Option<Box<S>>, Option<N>, Option<N>, Box<S>),
    While(N, Box<S>),
    DoWhile(Box<S>, N),
    Switch(N, Vec<(Option<N>, Vec<S>)>),
    Return(Option<N>),
    Break,
    Continue,
    Discard,
    Block(Vec<S>),
    Nop,
}

#[derive(Debug, Clone)]
struct Fn_ {
    name: String,
    ret: Ty,
    params: Vec<Param>,
    body: Vec<S>,
    locals: usize,
}

#[derive(Debug, Clone)]
pub struct Global {
    pub name: String,
    pub storage: Storage,
    pub ty: Ty,
    pub flat: bool,
    pub location: Option<u32>,
    init: Option<N>,
}

#[derive(Debug, Clone)]
pub struct Program {
    pub version: u32,
    pub structs: Vec<StructDef>,
    pub globals: Vec<Global>,
    names: HashMap<String, usize>,
    funcs: Vec<Fn_>,
    overloads: HashMap<String, Vec<usize>>,
    main: usize,
}

const BUILTIN_GLOBALS: [(&str, Ty); 11] = [
    ("gl_Position", Ty::Vec(Scalar::Float, 4)),
    ("gl_PointSize", Ty::Scalar(Scalar::Float)),
    ("gl_FragCoord", Ty::Vec(Scalar::Float, 4)),
    ("gl_FragColor", Ty::Vec(Scalar::Float, 4)),
    ("gl_FrontFacing", Ty::Scalar(Scalar::Bool)),
    ("gl_PointCoord", Ty::Vec(Scalar::Float, 2)),
    ("gl_FragDepth", Ty::Scalar(Scalar::Float)),
    ("gl_VertexID", Ty::Scalar(Scalar::Int)),
    ("gl_InstanceID", Ty::Scalar(Scalar::Int)),
    ("gl_VertexIndex", Ty::Scalar(Scalar::Int)),
    ("gl_InstanceIndex", Ty::Scalar(Scalar::Int)),
];

struct Compiler<'a> {
    unit: &'a Unit,
    names: HashMap<String, usize>,
    overloads: HashMap<String, Vec<usize>>,
    fn_params: Vec<(String, Vec<Param>)>,
    scopes: Vec<HashMap<String, usize>>,
    locals: usize,
    local_types: Vec<Ty>,
}

impl Program {
    pub fn compile(source: &str) -> Result<Program, String> {
        let pre = preprocess(source)?;
        let unit = parse(&pre.text)?;
        let mut globals: Vec<Global> = BUILTIN_GLOBALS
            .iter()
            .map(|(name, ty)| Global {
                name: (*name).to_string(),
                storage: Storage::None,
                ty: ty.clone(),
                flat: false,
                location: None,
                init: None,
            })
            .collect();
        globals.push(Global {
            name: "gl_FragData".to_string(),
            storage: Storage::None,
            ty: Ty::Array(Box::new(Ty::Vec(Scalar::Float, 4)), 8),
            flat: false,
            location: None,
            init: None,
        });
        let mut names: HashMap<String, usize> = HashMap::new();
        for (i, g) in globals.iter().enumerate() {
            names.insert(g.name.clone(), i);
        }
        for g in &unit.globals {
            let slot = globals.len();
            names.insert(g.name.clone(), slot);
            globals.push(Global {
                name: g.name.clone(),
                storage: g.storage,
                ty: g.ty.clone(),
                flat: g.flat,
                location: g.location,
                init: None,
            });
        }
        let mut overloads: HashMap<String, Vec<usize>> = HashMap::new();
        let mut fn_params = Vec::new();
        for (i, f) in unit.functions.iter().enumerate() {
            overloads.entry(f.name.clone()).or_default().push(i);
            fn_params.push((f.name.clone(), f.params.clone()));
        }
        let mut compiler = Compiler {
            unit: &unit,
            names: names.clone(),
            overloads: overloads.clone(),
            fn_params,
            scopes: Vec::new(),
            locals: 0,
            local_types: Vec::new(),
        };
        // Global initialisers see earlier globals and constants.
        for (index, g) in unit.globals.iter().enumerate() {
            if let Some(init) = &g.init {
                compiler.begin_function();
                let node = compiler.expr(init)?;
                let slot = names[&g.name];
                globals[slot].init = Some(node);
                let _ = index;
            }
        }
        let mut funcs = Vec::new();
        for f in &unit.functions {
            funcs.push(compiler.function(f)?);
        }
        let main = overloads
            .get("main")
            .and_then(|v| v.first())
            .copied()
            .ok_or("the shader has no main function")?;
        // Struct types used anywhere must exist.
        fn check(ty: &Ty, unit: &Unit) -> Result<(), String> {
            match ty {
                Ty::Struct(name) if !unit.structs.iter().any(|s| &s.name == name) => {
                    Err(format!("unknown struct '{name}'"))
                }
                Ty::Array(inner, _) => check(inner, unit),
                _ => Ok(()),
            }
        }
        for g in &unit.globals {
            check(&g.ty, &unit)?;
        }
        Ok(Program {
            version: pre.version,
            structs: unit.structs.clone(),
            globals,
            names,
            funcs,
            overloads,
            main,
        })
    }

    pub fn slot(&self, name: &str) -> Option<usize> {
        self.names.get(name).copied()
    }

    /// Whether any function reads the global `name` (a static use: it counts
    /// even where the code could never run).
    pub fn reads(&self, name: &str) -> bool {
        let Some(slot) = self.slot(name) else {
            return false;
        };
        fn node(n: &N, slot: usize) -> bool {
            match n {
                N::Global(g) => *g == slot,
                N::Lit(_) | N::Local(_) => false,
                N::Field(a, _) | N::Un(_, a) => node(a, slot),
                N::Index(a, b) | N::Bin(_, a, b) | N::Comma(a, b) => node(a, slot) || node(b, slot),
                N::Assign(op, target, value) => {
                    // A plain `x = ...` does not read `x`; `x += ...` and `x.y = ...` (via index) do.
                    let reads_target = op.is_some() || !matches!(**target, N::Global(_));
                    (reads_target && node(target, slot)) || node(value, slot)
                }
                N::Cond(a, b, c) => node(a, slot) || node(b, slot) || node(c, slot),
                N::CallUser(_, args)
                | N::CallBuiltin(_, args)
                | N::Ctor(_, args)
                | N::ArrCtor(_, args) => args.iter().any(|a| node(a, slot)),
            }
        }
        fn stmt(s: &S, slot: usize) -> bool {
            let opt_n = |n: &Option<N>| n.as_ref().is_some_and(|n| node(n, slot));
            match s {
                S::Expr(e) => node(e, slot),
                S::Decl(_, _, init) => opt_n(init),
                S::If(c, t, e) => {
                    node(c, slot) || stmt(t, slot) || e.as_ref().is_some_and(|e| stmt(e, slot))
                }
                S::For(i, c, st, b) => {
                    i.as_ref().is_some_and(|i| stmt(i, slot))
                        || opt_n(c)
                        || opt_n(st)
                        || stmt(b, slot)
                }
                S::While(c, b) | S::DoWhile(b, c) => node(c, slot) || stmt(b, slot),
                S::Switch(v, cases) => {
                    node(v, slot)
                        || cases
                            .iter()
                            .any(|(label, body)| opt_n(label) || body.iter().any(|s| stmt(s, slot)))
                }
                S::Return(v) => opt_n(v),
                S::Block(b) => b.iter().any(|s| stmt(s, slot)),
                S::Break | S::Continue | S::Discard | S::Nop => false,
            }
        }
        self.funcs
            .iter()
            .any(|f| f.body.iter().any(|s| stmt(s, slot)))
    }

    /// Whether any function assigns the global `name`.
    pub fn writes(&self, name: &str) -> bool {
        let Some(slot) = self.slot(name) else {
            return false;
        };
        fn in_node(n: &N, slot: usize) -> bool {
            match n {
                N::Assign(_, target, value) => {
                    root(target) == Some(slot) || in_node(target, slot) || in_node(value, slot)
                }
                N::Un(UnOp::PreInc | UnOp::PreDec | UnOp::PostInc | UnOp::PostDec, t) => {
                    root(t) == Some(slot)
                }
                N::Un(_, a) | N::Field(a, _) => in_node(a, slot),
                N::Bin(_, a, b) | N::Index(a, b) | N::Comma(a, b) => {
                    in_node(a, slot) || in_node(b, slot)
                }
                N::Cond(a, b, c) => in_node(a, slot) || in_node(b, slot) || in_node(c, slot),
                N::CallUser(_, args)
                | N::CallBuiltin(_, args)
                | N::Ctor(_, args)
                | N::ArrCtor(_, args) => args.iter().any(|a| in_node(a, slot)),
                _ => false,
            }
        }
        fn root(n: &N) -> Option<usize> {
            match n {
                N::Global(g) => Some(*g),
                N::Field(b, _) | N::Index(b, _) => root(b),
                _ => None,
            }
        }
        fn in_stmt(s: &S, slot: usize) -> bool {
            match s {
                S::Expr(e) => in_node(e, slot),
                S::Decl(_, _, Some(e)) => in_node(e, slot),
                S::If(c, t, e) => {
                    in_node(c, slot)
                        || in_stmt(t, slot)
                        || e.as_ref().is_some_and(|e| in_stmt(e, slot))
                }
                S::For(i, c, st, b) => {
                    i.as_ref().is_some_and(|i| in_stmt(i, slot))
                        || c.as_ref().is_some_and(|c| in_node(c, slot))
                        || st.as_ref().is_some_and(|c| in_node(c, slot))
                        || in_stmt(b, slot)
                }
                S::While(c, b) | S::DoWhile(b, c) => in_node(c, slot) || in_stmt(b, slot),
                S::Switch(v, cases) => {
                    in_node(v, slot)
                        || cases
                            .iter()
                            .any(|(_, b)| b.iter().any(|s| in_stmt(s, slot)))
                }
                S::Return(Some(e)) => in_node(e, slot),
                S::Block(b) => b.iter().any(|s| in_stmt(s, slot)),
                _ => false,
            }
        }
        self.funcs
            .iter()
            .any(|f| f.body.iter().any(|s| in_stmt(s, slot)))
    }
}

impl Compiler<'_> {
    fn begin_function(&mut self) {
        self.scopes = vec![HashMap::new()];
        self.locals = 0;
        self.local_types.clear();
    }

    fn declare(&mut self, name: &str, ty: &Ty) -> usize {
        let slot = self.locals;
        self.locals += 1;
        self.local_types.push(ty.clone());
        self.scopes
            .last_mut()
            .expect("a scope is open")
            .insert(name.to_string(), slot);
        slot
    }

    fn lookup(&self, name: &str) -> Option<N> {
        for scope in self.scopes.iter().rev() {
            if let Some(&slot) = scope.get(name) {
                return Some(N::Local(slot));
            }
        }
        self.names.get(name).map(|&g| N::Global(g))
    }

    fn function(&mut self, f: &Function) -> Result<Fn_, String> {
        self.begin_function();
        for p in &f.params {
            self.declare(&p.name, &p.ty);
        }
        let body = self.block(&f.body)?;
        Ok(Fn_ {
            name: f.name.clone(),
            ret: f.ret.clone(),
            params: f.params.clone(),
            body,
            locals: self.locals,
        })
    }

    fn block(&mut self, stmts: &[Stmt]) -> Result<Vec<S>, String> {
        self.scopes.push(HashMap::new());
        let out: Result<Vec<S>, String> = stmts.iter().map(|s| self.stmt(s)).collect();
        self.scopes.pop();
        out
    }

    fn stmt(&mut self, s: &Stmt) -> Result<S, String> {
        Ok(match s {
            Stmt::Empty => S::Nop,
            Stmt::Expr(e) => S::Expr(self.expr(e)?),
            Stmt::Decl(decls) => {
                let mut out = Vec::new();
                for d in decls {
                    // The initialiser cannot see the variable it initialises.
                    let init = d.init.as_ref().map(|e| self.expr(e)).transpose()?;
                    let slot = self.declare(&d.name, &d.ty);
                    out.push(S::Decl(slot, d.ty.clone(), init));
                }
                if out.len() == 1 {
                    out.remove(0)
                } else {
                    S::Block(out)
                }
            }
            Stmt::If(c, t, e) => S::If(
                self.expr(c)?,
                Box::new(self.scoped(t)?),
                e.as_ref()
                    .map(|e| self.scoped(e))
                    .transpose()?
                    .map(Box::new),
            ),
            Stmt::For(init, cond, step, body) => {
                self.scopes.push(HashMap::new());
                let init = init
                    .as_ref()
                    .map(|s| self.stmt(s))
                    .transpose()?
                    .map(Box::new);
                let cond = cond.as_ref().map(|e| self.expr(e)).transpose()?;
                let step = step.as_ref().map(|e| self.expr(e)).transpose()?;
                let body = Box::new(self.scoped(body)?);
                self.scopes.pop();
                S::For(init, cond, step, body)
            }
            Stmt::While(c, b) => S::While(self.expr(c)?, Box::new(self.scoped(b)?)),
            Stmt::DoWhile(b, c) => S::DoWhile(Box::new(self.scoped(b)?), self.expr(c)?),
            Stmt::Switch(value, cases) => {
                let value = self.expr(value)?;
                self.scopes.push(HashMap::new());
                let mut out = Vec::new();
                for (label, body) in cases {
                    let label = label.as_ref().map(|e| self.expr(e)).transpose()?;
                    let body: Result<Vec<S>, String> = body.iter().map(|s| self.stmt(s)).collect();
                    out.push((label, body?));
                }
                self.scopes.pop();
                S::Switch(value, out)
            }
            Stmt::Return(e) => S::Return(e.as_ref().map(|e| self.expr(e)).transpose()?),
            Stmt::Break => S::Break,
            Stmt::Continue => S::Continue,
            Stmt::Discard => S::Discard,
            Stmt::Block(stmts) => S::Block(self.block(stmts)?),
        })
    }

    fn scoped(&mut self, s: &Stmt) -> Result<S, String> {
        self.scopes.push(HashMap::new());
        let out = self.stmt(s);
        self.scopes.pop();
        out
    }

    fn expr(&mut self, e: &Expr) -> Result<N, String> {
        Ok(match e {
            Expr::Lit(l) => N::Lit(match l {
                Literal::Float(f) => V::f(*f),
                Literal::Int(i) => V::i(*i),
                Literal::UInt(u) => V::u(*u),
                Literal::Bool(b) => V::b(*b),
            }),
            Expr::Var(name) => self
                .lookup(name)
                .ok_or_else(|| format!("'{name}' is not declared"))?,
            Expr::Field(base, name) => N::Field(Box::new(self.expr(base)?), name.clone()),
            Expr::Index(base, index) => {
                N::Index(Box::new(self.expr(base)?), Box::new(self.expr(index)?))
            }
            Expr::Call(name, args) => {
                let args: Result<Vec<N>, String> = args.iter().map(|a| self.expr(a)).collect();
                let args = args?;
                if let Some(candidates) = self.overloads.get(name) {
                    let matching: Vec<usize> = candidates
                        .iter()
                        .copied()
                        .filter(|&i| self.fn_params[i].1.len() == args.len())
                        .collect();
                    match matching.as_slice() {
                        [] => {
                            return Err(format!(
                                "no overload of '{name}' takes {} arguments",
                                args.len()
                            ))
                        }
                        [one] => N::CallUser(*one, args),
                        // Several overloads of the same arity: resolved by the
                        // run-time kinds of the arguments (see `pick_overload`).
                        many => N::CallUser(usize::MAX - many[0], args),
                    }
                } else if let Some(ty) = builtin_type(name) {
                    N::Ctor(ty, args)
                } else if self.unit.structs.iter().any(|s| &s.name == name) {
                    N::Ctor(Ty::Struct(name.clone()), args)
                } else if builtins::is_builtin(name) {
                    N::CallBuiltin(name.clone(), args)
                } else {
                    return Err(format!("'{name}' is not a function"));
                }
            }
            Expr::ArrayCtor(ty, args) => {
                let args: Result<Vec<N>, String> = args.iter().map(|a| self.expr(a)).collect();
                N::ArrCtor(ty.clone(), args?)
            }
            Expr::Unary(op, operand) => N::Un(*op, Box::new(self.expr(operand)?)),
            Expr::Binary(op, a, b) => N::Bin(*op, Box::new(self.expr(a)?), Box::new(self.expr(b)?)),
            Expr::Assign(op, target, value) => N::Assign(
                *op,
                Box::new(self.expr(target)?),
                Box::new(self.expr(value)?),
            ),
            Expr::Ternary(c, a, b) => N::Cond(
                Box::new(self.expr(c)?),
                Box::new(self.expr(a)?),
                Box::new(self.expr(b)?),
            ),
            Expr::Comma(a, b) => N::Comma(Box::new(self.expr(a)?), Box::new(self.expr(b)?)),
        })
    }
}

enum Flow {
    Next,
    Break,
    Continue,
    Return(V),
    Discard,
}

/// One part of the path from a variable to the part of it being assigned.
enum Step {
    Field(String),
    Index(i32),
}

/// A shader being run: its variables and the textures it reads.
pub struct Machine<'p> {
    prog: &'p Program,
    pub globals: Vec<V>,
    locals: Vec<V>,
    base: usize,
    textures: &'p dyn Textures,
    steps: u64,
}

impl<'p> Machine<'p> {
    pub fn new(prog: &'p Program, textures: &'p dyn Textures) -> Result<Machine<'p>, String> {
        let globals = prog
            .globals
            .iter()
            .map(|g| zero(&g.ty, &prog.structs))
            .collect();
        let mut m = Machine {
            prog,
            globals,
            locals: Vec::new(),
            base: 0,
            textures,
            steps: 0,
        };
        // Constants and global initialisers.
        for slot in 0..prog.globals.len() {
            if let Some(init) = prog.globals[slot].init.clone() {
                m.locals.clear();
                m.locals.resize(8, V::Void);
                m.base = 0;
                let v = m.eval(&init)?;
                m.globals[slot] = coerce(v, &prog.globals[slot].ty);
            }
        }
        Ok(m)
    }

    pub fn set(&mut self, slot: usize, v: V) {
        self.globals[slot] = coerce(v, &self.prog.globals[slot].ty);
    }

    pub fn get(&self, slot: usize) -> &V {
        &self.globals[slot]
    }

    pub fn set_by_name(&mut self, name: &str, v: V) -> bool {
        match self.prog.slot(name) {
            Some(slot) => {
                self.set(slot, v);
                true
            }
            None => false,
        }
    }

    pub fn get_by_name(&self, name: &str) -> Option<&V> {
        self.prog.slot(name).map(|s| &self.globals[s])
    }

    /// Runs `main`. True when the shader discarded the fragment.
    pub fn run_main(&mut self) -> Result<bool, String> {
        self.steps = 0;
        self.locals.clear();
        let f = &self.prog.funcs[self.prog.main];
        self.locals.resize(f.locals, V::Void);
        self.base = 0;
        match self.exec_block(&f.body)? {
            Flow::Discard => Ok(true),
            _ => Ok(false),
        }
    }

    fn tick(&mut self) -> Result<(), String> {
        self.steps += 1;
        if self.steps > STEP_LIMIT {
            Err("the shader ran too long".into())
        } else {
            Ok(())
        }
    }

    fn exec_block(&mut self, stmts: &[S]) -> Result<Flow, String> {
        for s in stmts {
            match self.exec(s)? {
                Flow::Next => {}
                other => return Ok(other),
            }
        }
        Ok(Flow::Next)
    }

    fn exec(&mut self, s: &S) -> Result<Flow, String> {
        match s {
            S::Nop => Ok(Flow::Next),
            S::Expr(e) => {
                self.eval(e)?;
                Ok(Flow::Next)
            }
            S::Decl(slot, ty, init) => {
                let v = match init {
                    Some(e) => {
                        let v = self.eval(e)?;
                        coerce(v, ty)
                    }
                    None => zero(ty, &self.prog.structs),
                };
                self.locals[self.base + slot] = v;
                Ok(Flow::Next)
            }
            S::If(c, t, e) => {
                if self
                    .eval(c)?
                    .as_bool()
                    .ok_or("an if condition must be a bool")?
                {
                    self.exec(t)
                } else if let Some(e) = e {
                    self.exec(e)
                } else {
                    Ok(Flow::Next)
                }
            }
            S::For(init, cond, step, body) => {
                if let Some(init) = init {
                    self.exec(init)?;
                }
                loop {
                    self.tick()?;
                    if let Some(cond) = cond {
                        if !self
                            .eval(cond)?
                            .as_bool()
                            .ok_or("a loop condition must be a bool")?
                        {
                            break;
                        }
                    }
                    match self.exec(body)? {
                        Flow::Break => break,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                        Flow::Discard => return Ok(Flow::Discard),
                        Flow::Next | Flow::Continue => {}
                    }
                    if let Some(step) = step {
                        self.eval(step)?;
                    }
                }
                Ok(Flow::Next)
            }
            S::While(c, body) => {
                loop {
                    self.tick()?;
                    if !self
                        .eval(c)?
                        .as_bool()
                        .ok_or("a loop condition must be a bool")?
                    {
                        break;
                    }
                    match self.exec(body)? {
                        Flow::Break => break,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                        Flow::Discard => return Ok(Flow::Discard),
                        Flow::Next | Flow::Continue => {}
                    }
                }
                Ok(Flow::Next)
            }
            S::DoWhile(body, c) => {
                loop {
                    self.tick()?;
                    match self.exec(body)? {
                        Flow::Break => break,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                        Flow::Discard => return Ok(Flow::Discard),
                        Flow::Next | Flow::Continue => {}
                    }
                    if !self
                        .eval(c)?
                        .as_bool()
                        .ok_or("a loop condition must be a bool")?
                    {
                        break;
                    }
                }
                Ok(Flow::Next)
            }
            S::Switch(value, cases) => {
                let v = self
                    .eval(value)?
                    .as_i32()
                    .ok_or("a switch needs an integer")?;
                let mut start = None;
                for (i, (label, _)) in cases.iter().enumerate() {
                    if let Some(label) = label {
                        if self.eval(label)?.as_i32() == Some(v) {
                            start = Some(i);
                            break;
                        }
                    }
                }
                let start = start.or_else(|| cases.iter().position(|(l, _)| l.is_none()));
                if let Some(start) = start {
                    for (_, body) in &cases[start..] {
                        match self.exec_block(body)? {
                            Flow::Next => {}
                            Flow::Break => break,
                            other => return Ok(other),
                        }
                    }
                }
                Ok(Flow::Next)
            }
            S::Return(e) => Ok(Flow::Return(match e {
                Some(e) => self.eval(e)?,
                None => V::Void,
            })),
            S::Break => Ok(Flow::Break),
            S::Continue => Ok(Flow::Continue),
            S::Discard => Ok(Flow::Discard),
            S::Block(stmts) => self.exec_block(stmts),
        }
    }

    fn pick_overload(&self, first: usize, args: &[V]) -> usize {
        let name = &self.prog.funcs[first].name;
        let candidates = &self.prog.overloads[name];
        let score = |f: &Fn_| -> usize {
            f.params
                .iter()
                .zip(args)
                .filter(|(p, a)| match (&p.ty, a) {
                    (Ty::Scalar(k), V::Vec { k: ak, n: 1, .. }) => k == ak,
                    (Ty::Vec(k, n), V::Vec { k: ak, n: an, .. }) => k == ak && n == an,
                    (Ty::Mat(c, r), V::Mat { cols, rows, .. }) => c == cols && r == rows,
                    (Ty::Struct(_), V::Struct(..)) | (Ty::Array(..), V::Arr(_)) => true,
                    _ => false,
                })
                .count()
        };
        candidates
            .iter()
            .copied()
            .filter(|&i| self.prog.funcs[i].params.len() == args.len())
            .max_by_key(|&i| score(&self.prog.funcs[i]))
            .unwrap_or(first)
    }

    fn call_user(&mut self, index: usize, arg_nodes: &[N]) -> Result<V, String> {
        let mut args = Vec::with_capacity(arg_nodes.len());
        for a in arg_nodes {
            args.push(self.eval(a)?);
        }
        let index = if index > usize::MAX / 2 {
            self.pick_overload(usize::MAX - index, &args)
        } else {
            index
        };
        self.tick()?;
        let prog = self.prog;
        let f = &prog.funcs[index];
        let saved_base = self.base;
        let new_base = self.locals.len();
        self.locals
            .resize(new_base + f.locals.max(args.len()), V::Void);
        self.base = new_base;
        for (i, (p, a)) in f.params.iter().zip(args).enumerate() {
            self.locals[new_base + i] = coerce(a, &p.ty);
        }
        let flow = self.exec_block(&f.body);
        // Copy out and in-out parameters back to the caller's variables.
        let mut out_values = Vec::new();
        for (i, p) in f.params.iter().enumerate() {
            if p.dir != ParamDir::In {
                out_values.push((i, self.locals[new_base + i].clone()));
            }
        }
        self.locals.truncate(new_base);
        self.base = saved_base;
        let ret = match flow? {
            Flow::Return(v) => coerce(v, &f.ret),
            Flow::Discard => V::Void,
            _ => V::Void,
        };
        for (i, v) in out_values {
            self.assign(&arg_nodes[i], v)?;
        }
        Ok(ret)
    }

    fn eval(&mut self, n: &N) -> Result<V, String> {
        Ok(match n {
            N::Lit(v) => v.clone(),
            N::Local(i) => self.locals[self.base + i].clone(),
            N::Global(i) => self.globals[*i].clone(),
            N::Field(base, name) => {
                let b = self.eval(base)?;
                self.field(&b, name)?
            }
            N::Index(base, index) => {
                let b = self.eval(base)?;
                let i = self
                    .eval(index)?
                    .as_i32()
                    .ok_or("an index must be an integer")?;
                index_value(&b, i)?
            }
            N::CallUser(i, args) => self.call_user(*i, args)?,
            N::CallBuiltin(name, args) => {
                let mut values = Vec::with_capacity(args.len());
                for a in args {
                    values.push(self.eval(a)?);
                }
                builtins::call(name, &values, self.textures)
                    .ok_or_else(|| format!("'{name}' is not a built-in"))??
            }
            N::Ctor(ty, args) => {
                let mut values = Vec::with_capacity(args.len());
                for a in args {
                    values.push(self.eval(a)?);
                }
                construct(ty, values, &self.prog.structs)?
            }
            N::ArrCtor(ty, args) => {
                let mut values = Vec::with_capacity(args.len());
                for a in args {
                    values.push(self.eval(a)?);
                }
                let Ty::Array(inner, _) = ty else {
                    return Err("bad array constructor".into());
                };
                V::Arr(values.into_iter().map(|v| coerce(v, inner)).collect())
            }
            N::Un(op, operand) => match op {
                UnOp::Neg => negate(&self.eval(operand)?)?,
                UnOp::Plus => self.eval(operand)?,
                UnOp::Not => logical_not(&self.eval(operand)?)?,
                UnOp::BitNot => bit_not(&self.eval(operand)?)?,
                UnOp::PreInc | UnOp::PreDec | UnOp::PostInc | UnOp::PostDec => {
                    let old = self.eval(operand)?;
                    let one = match &old {
                        V::Vec { k: Scalar::Int, .. } => V::i(1),
                        V::Vec {
                            k: Scalar::UInt, ..
                        } => V::u(1),
                        _ => V::f(1.0),
                    };
                    let op = if matches!(op, UnOp::PreInc | UnOp::PostInc) {
                        BinOp::Add
                    } else {
                        BinOp::Sub
                    };
                    let new = binary(op, &old, &one)?;
                    self.assign(operand, new.clone())?;
                    if matches!(n, N::Un(UnOp::PostInc | UnOp::PostDec, _)) {
                        old
                    } else {
                        new
                    }
                }
            },
            N::Bin(op, a, b) => match op {
                // && and || short-circuit.
                BinOp::And => {
                    let x = self.eval(a)?.as_bool().ok_or("&& needs bools")?;
                    V::b(x && self.eval(b)?.as_bool().ok_or("&& needs bools")?)
                }
                BinOp::Or => {
                    let x = self.eval(a)?.as_bool().ok_or("|| needs bools")?;
                    V::b(x || self.eval(b)?.as_bool().ok_or("|| needs bools")?)
                }
                _ => {
                    let x = self.eval(a)?;
                    let y = self.eval(b)?;
                    binary(*op, &x, &y)?
                }
            },
            N::Assign(op, target, value) => {
                let rhs = self.eval(value)?;
                let new = match op {
                    None => rhs,
                    Some(op) => {
                        let old = self.eval(target)?;
                        binary(*op, &old, &rhs)?
                    }
                };
                self.assign(target, new.clone())?;
                new
            }
            N::Cond(c, a, b) => {
                if self
                    .eval(c)?
                    .as_bool()
                    .ok_or("a condition must be a bool")?
                {
                    self.eval(a)?
                } else {
                    self.eval(b)?
                }
            }
            N::Comma(a, b) => {
                self.eval(a)?;
                self.eval(b)?
            }
        })
    }

    fn field(&self, base: &V, name: &str) -> Result<V, String> {
        match base {
            V::Struct(def, values) => {
                let i = self.prog.structs[*def]
                    .fields
                    .iter()
                    .position(|(f, _)| f == name)
                    .ok_or_else(|| format!("no field '{name}'"))?;
                Ok(values[i].clone())
            }
            V::Vec { n, .. } => {
                let idx = swizzle_indices(name, *n as usize)
                    .ok_or_else(|| format!("'{name}' is not a swizzle of this vector"))?;
                swizzle(base, &idx)
            }
            _ => Err(format!("'.{name}' does not apply here")),
        }
    }

    /// Resolves an lvalue to its root variable and the path into it.
    // To be restructured with the assignment path; the tuple is (root, path, swizzle).
    #[allow(clippy::type_complexity)]
    fn lvalue(&mut self, n: &N) -> Result<(N, Vec<Step>, Option<Vec<usize>>), String> {
        match n {
            N::Local(_) | N::Global(_) => Ok((n.clone(), Vec::new(), None)),
            N::Field(base, name) => {
                let (root, mut path, swz) = self.lvalue(base)?;
                if swz.is_some() {
                    return Err("cannot assign through a swizzle".into());
                }
                // A swizzle on a vector is the last step; on a struct it is a field.
                let base_value = self.eval(base)?;
                match base_value {
                    V::Vec { n: width, .. } => {
                        let idx = swizzle_indices(name, width as usize).ok_or("bad swizzle")?;
                        Ok((root, path, Some(idx)))
                    }
                    _ => {
                        path.push(Step::Field(name.clone()));
                        Ok((root, path, None))
                    }
                }
            }
            N::Index(base, index) => {
                let (root, mut path, swz) = self.lvalue(base)?;
                if swz.is_some() {
                    return Err("cannot index through a swizzle".into());
                }
                let i = self
                    .eval(index)?
                    .as_i32()
                    .ok_or("an index must be an integer")?;
                path.push(Step::Index(i));
                Ok((root, path, None))
            }
            _ => Err("this cannot be assigned to".into()),
        }
    }

    fn assign(&mut self, target: &N, value: V) -> Result<(), String> {
        let (root, path, swz) = self.lvalue(target)?;
        let slot: &mut V = match root {
            N::Local(i) => &mut self.locals[self.base + i],
            N::Global(i) => &mut self.globals[i],
            _ => unreachable!("lvalue roots are variables"),
        };
        let structs = &self.prog.structs;
        let mut at: &mut V = slot;
        for step in &path {
            at = match (step, at) {
                (Step::Field(name), V::Struct(def, values)) => {
                    let i = structs[*def]
                        .fields
                        .iter()
                        .position(|(f, _)| f == name)
                        .ok_or_else(|| format!("no field '{name}'"))?;
                    &mut values[i]
                }
                (Step::Index(i), V::Arr(items)) => {
                    let len = items.len() as i32;
                    if len == 0 {
                        return Ok(());
                    }
                    &mut items[(*i).clamp(0, len - 1) as usize]
                }
                (Step::Index(i), whole @ (V::Vec { .. } | V::Mat { .. })) => {
                    // The last step into a vector or matrix.
                    let remaining = &path[path.len() - 1..];
                    let _ = remaining;
                    return set_index(whole, *i, value);
                }
                _ => return Err("this cannot be assigned to".into()),
            };
        }
        match swz {
            Some(idx) => set_swizzle(at, &idx, &value),
            None => {
                let converted = match (&*at, &value) {
                    (V::Vec { k, n, .. }, V::Vec { n: vn, .. }) if n == vn => {
                        coerce(value, &Ty::Vec(*k, *n))
                    }
                    (V::Vec { k, n: 1, .. }, V::Vec { .. }) => coerce(value, &Ty::Scalar(*k)),
                    _ => value,
                };
                *at = converted;
                Ok(())
            }
        }
    }
}

fn index_value(base: &V, i: i32) -> R<V> {
    index(base, i)
}

/// Flattens constructor arguments into lanes (matrices column-major).
fn lanes_of(args: &[V]) -> R<Vec<(Scalar, u32)>> {
    let mut out = Vec::new();
    for a in args {
        match a {
            V::Vec { k, n, c } => out.extend((0..*n as usize).map(|i| (*k, c[i]))),
            V::Mat { cols, rows, d } => out.extend(
                d[..(*cols * *rows) as usize]
                    .iter()
                    .map(|x| (Scalar::Float, x.to_bits())),
            ),
            _ => return Err("this cannot be used to build a value".into()),
        }
    }
    Ok(out)
}

pub fn construct(ty: &Ty, args: Vec<V>, structs: &[StructDef]) -> R<V> {
    match ty {
        Ty::Scalar(k) => {
            let lanes = lanes_of(&args)?;
            let (from, bits) = *lanes.first().ok_or("a constructor needs an argument")?;
            Ok(V::Vec {
                k: *k,
                n: 1,
                c: [convert_lane(from, bits, *k), 0, 0, 0],
            })
        }
        Ty::Vec(k, n) => {
            let lanes = lanes_of(&args)?;
            let mut out = [0u32; 4];
            if lanes.len() == 1 {
                let (from, bits) = lanes[0];
                out = [convert_lane(from, bits, *k); 4];
            } else if lanes.len() >= *n as usize {
                for i in 0..*n as usize {
                    out[i] = convert_lane(lanes[i].0, lanes[i].1, *k);
                }
            } else {
                return Err(format!("not enough components for a {n}-vector"));
            }
            Ok(V::Vec {
                k: *k,
                n: *n,
                c: out,
            })
        }
        Ty::Mat(cols, rows) => {
            let mut d = [0.0f32; 16];
            if let [V::Mat {
                cols: sc,
                rows: sr,
                d: sd,
            }] = args.as_slice()
            {
                // From another matrix: copy what overlaps, the rest is identity.
                for c in 0..*cols as usize {
                    for r in 0..*rows as usize {
                        d[c * *rows as usize + r] = if c < *sc as usize && r < *sr as usize {
                            sd[c * *sr as usize + r]
                        } else if c == r {
                            1.0
                        } else {
                            0.0
                        };
                    }
                }
            } else {
                let lanes = lanes_of(&args)?;
                if lanes.len() == 1 {
                    let x = lane_f32(lanes[0].0, lanes[0].1);
                    for i in 0..(*cols).min(*rows) as usize {
                        d[i * *rows as usize + i] = x;
                    }
                } else if lanes.len() >= (*cols * *rows) as usize {
                    for (i, (k, bits)) in lanes.iter().take((*cols * *rows) as usize).enumerate() {
                        d[i] = lane_f32(*k, *bits);
                    }
                } else {
                    return Err("not enough components for a matrix".into());
                }
            }
            Ok(V::Mat {
                cols: *cols,
                rows: *rows,
                d,
            })
        }
        Ty::Struct(name) => {
            let (index, def) = structs
                .iter()
                .enumerate()
                .find(|(_, s)| &s.name == name)
                .ok_or_else(|| format!("unknown struct '{name}'"))?;
            if args.len() != def.fields.len() {
                return Err(format!("struct {name} takes {} values", def.fields.len()));
            }
            Ok(V::Struct(
                index,
                args.into_iter()
                    .zip(&def.fields)
                    .map(|(v, (_, t))| coerce(v, t))
                    .collect(),
            ))
        }
        _ => Err("this type cannot be constructed".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtins::NoTextures;

    /// Compiles `source`, runs `main`, and returns the machine.
    fn run(source: &str) -> (Program, Vec<V>) {
        let program = Program::compile(source).unwrap_or_else(|e| panic!("{e}"));
        let globals = {
            let mut m = Machine::new(&program, &NoTextures).unwrap();
            m.run_main().unwrap_or_else(|e| panic!("{e}"));
            m.globals.clone()
        };
        (program, globals)
    }

    fn out(source: &str, name: &str) -> Vec<f32> {
        let (p, g) = run(source);
        g[p.slot(name).unwrap()].floats().unwrap()
    }

    #[test]
    fn arithmetic_and_assignment() {
        assert_eq!(
            out(
                "void main() { gl_FragColor = vec4(1.0, 2.0, 3.0, 4.0) * 2.0 + 1.0; }",
                "gl_FragColor"
            ),
            vec![3.0, 5.0, 7.0, 9.0]
        );
    }

    #[test]
    fn locals_loops_and_conditionals() {
        let src = "void main() { float s = 0.0; for (int i = 0; i < 5; i++) { if (i == 2) continue; if (i == 4) break; s += float(i); } gl_FragColor = vec4(s); }";
        assert_eq!(out(src, "gl_FragColor")[0], 4.0, "0 + 1 + 3");
        let src = "void main() { int n = 0; while (n < 10) { n += 3; } int m = 0; do { m++; } while (m < 3); gl_FragColor = vec4(float(n), float(m), 0.0, 0.0); }";
        assert_eq!(out(src, "gl_FragColor")[..2], [12.0, 3.0]);
    }

    #[test]
    fn user_functions_with_in_out_and_inout_parameters() {
        let src = "float twice(float x) { return x * 2.0; }\n\
                   void split(in float v, out float a, inout float b) { a = v + 1.0; b += v; }\n\
                   void main() { float a; float b = 10.0; split(3.0, a, b); gl_FragColor = vec4(twice(a), b, 0.0, 0.0); }";
        assert_eq!(out(src, "gl_FragColor")[..2], [8.0, 13.0]);
    }

    #[test]
    fn overloads_are_picked_by_argument_types() {
        let src = "float f(float x) { return 1.0; } float f(vec2 x) { return 2.0; } float f(vec3 x) { return 3.0; }\n\
                   void main() { gl_FragColor = vec4(f(1.0), f(vec2(1.0)), f(vec3(1.0)), 0.0); }";
        assert_eq!(out(src, "gl_FragColor")[..3], [1.0, 2.0, 3.0]);
    }

    #[test]
    fn swizzle_writes_and_compound_assignment() {
        let src = "void main() { vec4 c = vec4(0.0); c.xy = vec2(1.0, 2.0); c.zw += vec2(3.0); c.x *= 5.0; gl_FragColor = c.wzyx; }";
        assert_eq!(out(src, "gl_FragColor"), vec![3.0, 3.0, 2.0, 5.0]);
    }

    #[test]
    fn structs_arrays_and_nested_assignment() {
        // `.length()` is not supported; a shader that uses it must fail to compile.
        assert!(Program::compile("float t[3] = float[3](1.0, 2.0, 3.0); void main() { gl_FragColor = vec4(float(t.length())); }").is_err());
        let src = "struct Light { vec3 color; float power[2]; };\n\
                   float table[3] = float[3](1.0, 2.0, 3.0);\n\
                   void main() { Light l; l.color = vec3(1.0, 0.5, 0.25); l.power[1] = 4.0; l.color.y = 9.0; table[2] += 10.0;\n\
                     gl_FragColor = vec4(l.color.y, l.power[1], table[2], l.color.z); }";
        assert_eq!(out(src, "gl_FragColor"), vec![9.0, 4.0, 13.0, 0.25]);
    }

    #[test]
    fn constructors_convert_and_assemble() {
        let src = "void main() { vec4 a = vec4(vec2(1.0, 2.0), 3.0, 4.0); mat2 m = mat2(vec2(1.0, 2.0), vec2(3.0, 4.0)); vec2 r = m * vec2(1.0, 1.0);\n\
                   int i = int(3.9); float f = float(i); mat3 d = mat3(2.0); gl_FragColor = vec4(a.x + a.w, r.x, f, d[1][1]); }";
        assert_eq!(out(src, "gl_FragColor"), vec![5.0, 4.0, 3.0, 2.0]);
    }

    #[test]
    fn ternary_short_circuit_and_comma() {
        let src = "float side = 0.0; bool bump() { side += 1.0; return true; }\n\
                   void main() { bool a = false && bump(); bool b = true || bump(); float t = a ? 1.0 : 2.0; gl_FragColor = vec4(t, side, 0.0, 0.0); }";
        assert_eq!(out(src, "gl_FragColor")[..2], [2.0, 0.0]);
    }

    #[test]
    fn switch_with_fallthrough_and_default() {
        let src = "void main() { float r = 0.0; for (int i = 0; i < 4; i++) { switch (i) { case 0: r += 1.0; case 1: r += 10.0; break; case 2: r += 100.0; break; default: r += 1000.0; } } gl_FragColor = vec4(r); }";
        assert_eq!(out(src, "gl_FragColor")[0], 11.0 + 10.0 + 100.0 + 1000.0);
    }

    #[test]
    fn discard_is_reported() {
        let program = Program::compile(
            "void main() { if (gl_FragCoord.x < 1.0) discard; gl_FragColor = vec4(1.0); }",
        )
        .unwrap();
        let mut m = Machine::new(&program, &NoTextures).unwrap();
        m.set_by_name("gl_FragCoord", V::vec4(0.5, 0.0, 0.0, 1.0));
        assert!(m.run_main().unwrap());
        m.set_by_name("gl_FragCoord", V::vec4(2.0, 0.0, 0.0, 1.0));
        assert!(!m.run_main().unwrap());
    }

    #[test]
    fn integer_ops_bit_ops_and_hashes() {
        let src = "uint hash(uint x) { x ^= x >> 16u; x *= 0x7feb352du; x ^= x >> 15u; return x; }\n\
                   void main() { uint h = hash(12345u); int a = 7 / 2; int b = -7 % 3; gl_FragColor = vec4(float(h & 255u), float(a), float(b), float(1 << 4)); }";
        let v = out(src, "gl_FragColor");
        let mut x: u32 = 12345;
        x ^= x >> 16;
        x = x.wrapping_mul(0x7feb352d);
        x ^= x >> 15;
        assert_eq!(v, vec![(x & 255) as f32, 3.0, -1.0, 16.0]);
    }

    #[test]
    fn uniforms_attributes_and_varyings_are_set_from_outside() {
        let program = Program::compile("attribute vec3 position; uniform mat4 mvp; varying vec2 uv; void main() { uv = position.xy; gl_Position = mvp * vec4(position, 1.0); }").unwrap();
        let mut m = Machine::new(&program, &NoTextures).unwrap();
        let mut d = [0.0; 16];
        for i in 0..4 {
            d[i * 5] = 2.0;
        }
        assert!(m.set_by_name(
            "mvp",
            V::Mat {
                cols: 4,
                rows: 4,
                d
            }
        ));
        assert!(m.set_by_name("position", V::vec(&[1.0, 2.0, 3.0])));
        assert!(!m.set_by_name("nope", V::f(0.0)));
        m.run_main().unwrap();
        assert_eq!(
            m.get_by_name("gl_Position").unwrap().floats().unwrap(),
            vec![2.0, 4.0, 6.0, 2.0]
        );
        assert_eq!(
            m.get_by_name("uv").unwrap().floats().unwrap(),
            vec![1.0, 2.0]
        );
    }

    #[test]
    fn gles3_syntax_runs() {
        let src = "#version 300 es\nprecision highp float;\nlayout(location = 0) out vec4 o; in vec2 vUv; flat in int id;\nvoid main() { o = vec4(vUv, float(id), 1.0); }";
        let program = Program::compile(src).unwrap();
        assert_eq!(program.version, 300);
        let mut m = Machine::new(&program, &NoTextures).unwrap();
        m.set_by_name("vUv", V::vec(&[0.25, 0.5]));
        m.set_by_name("id", V::i(7));
        m.run_main().unwrap();
        assert_eq!(
            m.get_by_name("o").unwrap().floats().unwrap(),
            vec![0.25, 0.5, 7.0, 1.0]
        );
    }

    #[test]
    fn macros_and_conditionals_shape_the_shader() {
        let src = "#define SCALE 2.0\n#define SQ(x) ((x)*(x))\n#ifdef GL_ES\nprecision mediump float;\n#endif\n#if defined(USE_FOG)\nfloat k = 1.0;\n#else\nfloat k = 3.0;\n#endif\nvoid main() { gl_FragColor = vec4(SQ(SCALE) * k); }";
        assert_eq!(out(src, "gl_FragColor")[0], 12.0);
    }

    #[test]
    fn compile_errors_name_the_problem() {
        assert!(Program::compile("void main() { x = 1.0; }")
            .unwrap_err()
            .contains("'x' is not declared"));
        assert!(Program::compile("void main() { nothing(1.0); }")
            .unwrap_err()
            .contains("not a function"));
        assert!(Program::compile("float f() { return 1.0; }")
            .unwrap_err()
            .contains("no main"));
        assert!(Program::compile("void main() { float x = ; }").is_err());
        assert!(
            Program::compile("float f(float a) { return a; } void main() { f(1.0, 2.0); }")
                .unwrap_err()
                .contains("arguments")
        );
    }

    #[test]
    fn a_runaway_loop_ends_with_an_error() {
        let program = Program::compile("void main() { for (;;) { } }").unwrap();
        let mut m = Machine::new(&program, &NoTextures).unwrap();
        assert!(m.run_main().unwrap_err().contains("too long"));
    }

    #[test]
    fn builtins_are_available_to_shaders() {
        let src = "void main() { vec3 n = normalize(vec3(0.0, 3.0, 4.0)); gl_FragColor = vec4(n.yz, clamp(2.0, 0.0, 1.0), mix(0.0, 8.0, 0.25)); }";
        let v = out(src, "gl_FragColor");
        assert!((v[0] - 0.6).abs() < 1e-6 && (v[1] - 0.8).abs() < 1e-6);
        assert_eq!(v[2..], [1.0, 2.0]);
    }
}
