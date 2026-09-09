// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! AST -> operand-stack bytecode, per Phase 13's first execution slice.
//! Binding resolution happens here; runtime execution never walks an AST.
//! Every lexical scope has its own slots, reset on entry/exit. Abrupt
//! loop exits emit the same scope cleanup as ordinary block exits.

use crate::bytecode::Binding;
use crate::*;
use std::collections::{BTreeSet, HashMap};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileError {
    Unsupported(&'static str),
    DuplicateBinding(String),
    InvalidSyntax(&'static str),
    ProgramTooLarge,
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(feature) => write!(f, "BlueJS execution does not yet support {feature}"),
            Self::DuplicateBinding(name) => write!(f, "duplicate or conflicting binding: {name}"),
            Self::InvalidSyntax(message) => f.write_str(message),
            Self::ProgramTooLarge => f.write_str("BlueJS program exceeds the bytecode size limit"),
        }
    }
}
impl std::error::Error for CompileError {}

/// Compiles the supported executable subset. The parser deliberately
/// accepts more than the VM can run; unsupported syntax is rejected even
/// in unreachable branches, before any execution or heap mutation.
pub fn compile(program: &Program) -> Result<Bytecode, CompileError> {
    compile_with_limit(program, u32::MAX)
}

/// Compiles with an inclusive limit on emitted instruction bytes.
/// A limit failure returns [`CompileError::ProgramTooLarge`], never partial
/// bytecode. This does not bound AST depth, constant payloads or total memory.
pub fn compile_with_limit(program: &Program, max_bytecode_bytes: u32) -> Result<Bytecode, CompileError> {
    let mut compiler = Compiler { bytecode: Bytecode::empty(), names: Vec::new(), scopes: Vec::new(), loops: Vec::new(), max_bytecode_bytes, function: false, local_scope: 0 };
    compiler.bytecode.strict = strict_body(&program.body);
    let vars = var_names(&program.body)?;
    compiler.enter_scope(lexical_names(&program.body)?, &vars, true)?;
    compiler.statements(&program.body)?;
    compiler.emit(Opcode::Halt, 0)?;
    Ok(compiler.bytecode)
}

struct Loop {
    scope_depth: usize,
    breaks: Vec<usize>,
    continues: Vec<usize>,
    iterator: Option<u32>,
}

struct Compiler {
    bytecode: Bytecode,
    names: Vec<HashMap<String, u32>>,
    scopes: Vec<u32>,
    loops: Vec<Loop>,
    max_bytecode_bytes: u32,
    function: bool,
    local_scope: usize,
}

impl Compiler {
    fn offset(&self) -> Result<u32, CompileError> {
        u32::try_from(self.bytecode.code.len()).map_err(|_| CompileError::ProgramTooLarge)
    }

    fn emit(&mut self, opcode: Opcode, operand: u32) -> Result<usize, CompileError> {
        let offset = self.offset()? as usize;
        if offset.checked_add(opcode.width()).is_none_or(|end| end > self.max_bytecode_bytes as usize) {
            return Err(CompileError::ProgramTooLarge);
        }
        self.bytecode.code.push(opcode as u8);
        if opcode.width() == 5 {
            self.bytecode.code.extend_from_slice(&operand.to_le_bytes());
        }
        Ok(offset)
    }

    fn patch(&mut self, jump: usize, target: u32) {
        self.bytecode.code[jump + 1..jump + 5].copy_from_slice(&target.to_le_bytes());
    }

    fn constant(&mut self, value: Value) -> Result<(), CompileError> {
        let index = u32::try_from(self.bytecode.constants.len()).map_err(|_| CompileError::ProgramTooLarge)?;
        self.bytecode.constants.push(value);
        self.emit(Opcode::Constant, index)?;
        Ok(())
    }

    fn enter_scope(&mut self, lexical: Vec<(String, DeclKind)>, vars: &BTreeSet<String>, global: bool) -> Result<(), CompileError> {
        let mut names = HashMap::new();
        let mut slots = Vec::new();
        let declarations = vars.iter().filter(|_| global).map(|name| (name.clone(), DeclKind::Var)).chain(lexical);
        for (name, kind) in declarations {
            if matches!(name.as_str(), "undefined" | "NaN" | "Infinity") {
                return Err(CompileError::Unsupported("shadowing ambient constants"));
            }
            if names.contains_key(&name) || (kind != DeclKind::Var && vars.contains(&name)) {
                return Err(CompileError::DuplicateBinding(name));
            }
            let slot = u32::try_from(self.bytecode.bindings.len()).map_err(|_| CompileError::ProgramTooLarge)?;
            self.bytecode.bindings.push(Binding { name: name.clone(), mutable: kind != DeclKind::Const, lexical: kind != DeclKind::Var });
            names.insert(name, slot);
            slots.push(slot);
        }
        let scope = u32::try_from(self.bytecode.scopes.len()).map_err(|_| CompileError::ProgramTooLarge)?;
        self.bytecode.scopes.push(slots);
        self.names.push(names);
        self.scopes.push(scope);
        self.emit(Opcode::EnterScope, scope)?;
        Ok(())
    }

    fn leave_scope(&mut self) -> Result<(), CompileError> {
        let scope = self.scopes.pop().expect("compiler scopes are balanced");
        self.names.pop();
        self.emit(Opcode::LeaveScope, scope)?;
        Ok(())
    }

    fn resolve(&self, name: &str) -> Option<u32> {
        self.names.iter().rev().find_map(|scope| scope.get(name).copied())
    }

    fn statements(&mut self, statements: &[Stmt]) -> Result<(), CompileError> {
        for statement in statements {
            if let Stmt::FunctionDecl(function) = statement {
                self.function(function, false)?;
                let slot = self.resolve(function.name.as_ref().expect("declaration has a name")).unwrap();
                self.emit(Opcode::StoreBinding, slot)?;
                self.emit(Opcode::Pop, 0)?;
            }
        }
        for statement in statements {
            self.statement(statement, true)?;
        }
        Ok(())
    }

    fn statement(&mut self, statement: &Stmt, declarations_allowed: bool) -> Result<(), CompileError> {
        match statement {
            Stmt::Throw(value) => {
                self.expression(value)?;
                self.emit(Opcode::Throw, 0)?;
            }
            Stmt::FunctionDecl(_) => {}
            Stmt::Return(value) => {
                if !self.function {
                    return Err(CompileError::InvalidSyntax("return requires a function"));
                }
                if let Some(value) = value {
                    self.expression(value)?;
                } else {
                    self.constant(Value::Undefined)?;
                }
                let iterators: Vec<_> = self.loops.iter().rev().filter_map(|context| context.iterator).collect();
                for iterator in iterators {
                    self.emit(Opcode::GetBinding, iterator)?;
                    self.emit(Opcode::IteratorClose, 0)?;
                }
                self.emit(Opcode::Return, 0)?;
            }
            Stmt::Empty => {}
            Stmt::Expr(expr) => {
                self.expression(expr)?;
                self.emit(Opcode::SetCompletion, 0)?;
            }
            Stmt::Block(body) => {
                self.enter_scope(lexical_names(body)?, &var_names(body)?, false)?;
                self.statements(body)?;
                self.leave_scope()?;
            }
            Stmt::VarDecl(kind, declarations) => {
                if !declarations_allowed && *kind != DeclKind::Var {
                    return Err(CompileError::InvalidSyntax("a lexical declaration requires a block"));
                }
                self.declarations(*kind, declarations)?;
            }
            Stmt::If { test, consequent, alternate } => {
                self.emit(Opcode::ClearCompletion, 0)?;
                self.expression(test)?;
                let no = self.emit(Opcode::JumpIfFalse, 0)?;
                self.statement(consequent, false)?;
                let end = self.emit(Opcode::Jump, 0)?;
                self.patch(no, self.offset()?);
                if let Some(alternate) = alternate {
                    self.statement(alternate, false)?;
                }
                self.patch(end, self.offset()?);
            }
            Stmt::While { test, body } => self.loop_statement(None, Some(test), None, body, false)?,
            Stmt::DoWhile { body, test } => self.loop_statement(None, Some(test), None, body, true)?,
            Stmt::For { init, test, update, body } => self.loop_statement(init.as_ref(), test.as_ref(), update.as_ref(), body, false)?,
            Stmt::ForOf { left, right, body } => self.for_of(left, right, body)?,
            Stmt::Break | Stmt::Continue => {
                let Some(context) = self.loops.last() else { return Err(CompileError::InvalidSyntax("break/continue requires an enclosing loop")) };
                let scopes: Vec<_> = self.scopes[context.scope_depth..].iter().rev().copied().collect();
                let iterator = context.iterator;
                for scope in scopes {
                    self.emit(Opcode::LeaveScope, scope)?;
                }
                if matches!(statement, Stmt::Break) {
                    if let Some(iterator) = iterator {
                        self.emit(Opcode::GetBinding, iterator)?;
                        self.emit(Opcode::IteratorClose, 0)?;
                    }
                }
                let jump = self.emit(Opcode::Jump, 0)?;
                let context = self.loops.last_mut().unwrap();
                if matches!(statement, Stmt::Break) {
                    context.breaks.push(jump)
                } else {
                    context.continues.push(jump)
                }
            }
            _ => return Err(CompileError::Unsupported("this statement kind (functions, for-in/of, switch, return or exceptions)")),
        }
        Ok(())
    }

    fn declarations(&mut self, kind: DeclKind, declarations: &[VarDeclarator]) -> Result<(), CompileError> {
        for declaration in declarations {
            let name = binding_name(&declaration.pattern)?;
            if kind == DeclKind::Const && declaration.init.is_none() {
                return Err(CompileError::InvalidSyntax("const requires an initializer"));
            }
            if kind == DeclKind::Var && declaration.init.is_none() {
                continue;
            }
            if let Some(value) = &declaration.init {
                self.expression(value)?
            } else {
                self.constant(Value::Undefined)?
            }
            let slot = if kind == DeclKind::Var { self.names[self.local_scope][name] } else { self.names.last().unwrap()[name] };
            if kind == DeclKind::Var {
                self.emit(Opcode::StoreBinding, slot)?;
                self.emit(Opcode::Pop, 0)?;
            } else {
                self.emit(Opcode::InitializeBinding, slot)?;
            }
        }
        Ok(())
    }

    fn loop_statement(&mut self, init: Option<&ForInit>, test: Option<&Expr>, update: Option<&Expr>, body: &Stmt, do_first: bool) -> Result<(), CompileError> {
        self.emit(Opcode::ClearCompletion, 0)?;
        let lexical = match init {
            Some(ForInit::VarDecl(kind, decls)) if *kind != DeclKind::Var => declarations_names(*kind, decls)?,
            _ => Vec::new(),
        };
        let own_scope = !lexical.is_empty();
        if own_scope {
            self.enter_scope(lexical, &var_names(std::slice::from_ref(body))?, false)?;
        }
        match init {
            Some(ForInit::VarDecl(kind, declarations)) => self.declarations(*kind, declarations)?,
            Some(ForInit::Expr(expr)) => {
                self.expression(expr)?;
                self.emit(Opcode::Pop, 0)?;
            }
            None => {}
        }
        let start = self.offset()?;
        let mut exit = None;
        if !do_first {
            if let Some(test) = test {
                self.expression(test)?;
                exit = Some(self.emit(Opcode::JumpIfFalse, 0)?);
            }
        }
        self.loops.push(Loop { scope_depth: self.scopes.len(), breaks: Vec::new(), continues: Vec::new(), iterator: None });
        self.statement(body, false)?;
        let continue_at = self.offset()?;
        if let Some(update) = update {
            self.expression(update)?;
            self.emit(Opcode::Pop, 0)?;
        }
        if do_first {
            self.expression(test.expect("do-while always has a condition"))?;
            self.emit(Opcode::JumpIfTrue, start)?;
        } else {
            self.emit(Opcode::Jump, start)?;
        }
        let end = self.offset()?;
        if let Some(exit) = exit {
            self.patch(exit, end);
        }
        let context = self.loops.pop().unwrap();
        for jump in context.breaks {
            self.patch(jump, end);
        }
        for jump in context.continues {
            self.patch(jump, continue_at);
        }
        if own_scope {
            self.leave_scope()?;
        }
        Ok(())
    }

    fn expression(&mut self, expr: &Expr) -> Result<(), CompileError> {
        match expr {
            Expr::RegExp { pattern, flags } => {
                self.constant(Value::String(pattern.clone()))?;
                self.constant(Value::String(flags.clone()))?;
                self.emit(Opcode::RegExpLiteral, 0)?;
            }
            Expr::TaggedTemplate { tag, raw, cooked, expressions } => {
                if matches!(&**tag, Expr::Member { .. }) {
                    self.member_reference(tag)?;
                    self.emit(Opcode::GetMethod, 0)?;
                } else {
                    self.expression(tag)?;
                    self.constant(Value::Undefined)?;
                }
                static NEXT_SITE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
                let id = NEXT_SITE
                    .fetch_update(std::sync::atomic::Ordering::Relaxed, std::sync::atomic::Ordering::Relaxed, |n| n.checked_add(1))
                    .map_err(|_| CompileError::ProgramTooLarge)?;
                let site = self.bytecode.templates.len() as u32;
                self.bytecode.templates.push(crate::bytecode::TemplateSite { id, raw: raw.clone(), cooked: cooked.clone() });
                self.emit(Opcode::TemplateObject, site)?;
                for expression in expressions {
                    self.expression(expression)?;
                }
                self.emit(Opcode::Call, expressions.len() as u32 + 1)?;
            }
            Expr::Number(n) => self.constant(Value::Number(*n))?,
            Expr::String(s) => self.constant(Value::String(s.clone()))?,
            Expr::Bool(b) => self.constant(Value::Bool(*b))?,
            Expr::Null => self.constant(Value::Null)?,
            Expr::Identifier(name) => {
                if let Some(slot) = self.resolve(name) {
                    self.emit(Opcode::GetBinding, slot)?;
                } else {
                    match name.as_str() {
                        "undefined" => self.constant(Value::Undefined)?,
                        "NaN" => self.constant(Value::Number(f64::NAN))?,
                        "Infinity" => self.constant(Value::Number(f64::INFINITY))?,
                        "String" => {
                            self.emit(Opcode::GlobalString, 0)?;
                        }
                        "Symbol" | "RegExp" | "Object" | "Reflect" | "Number" | "Boolean" | "globalThis" => {
                            let index = self.bytecode.constants.len() as u32;
                            self.bytecode.constants.push(Value::String(name.as_str().into()));
                            self.emit(Opcode::Global, index)?;
                        }
                        _ => {
                            let index = u32::try_from(self.bytecode.constants.len()).map_err(|_| CompileError::ProgramTooLarge)?;
                            self.bytecode.constants.push(Value::String(name.clone().into()));
                            self.emit(Opcode::UnboundName, index)?;
                        }
                    }
                }
            }
            Expr::Unary { op, arg } => {
                let opcode = match op {
                    UnaryOp::Neg => Opcode::Negate,
                    UnaryOp::Plus => Opcode::ToNumber,
                    UnaryOp::Not => Opcode::Not,
                    UnaryOp::Typeof => Opcode::Typeof,
                    UnaryOp::Delete => Opcode::DeleteProperty,
                };
                if *op == UnaryOp::Delete {
                    if matches!(&**arg, Expr::Member { .. }) {
                        self.member_reference(arg)?;
                        self.emit(opcode, 0)?;
                    } else if let Expr::Identifier(name) = &**arg {
                        if self.bytecode.strict {
                            return Err(CompileError::InvalidSyntax("cannot delete a binding in strict mode"));
                        }
                        self.constant(Value::Bool(self.resolve(name).is_none()))?;
                    } else {
                        self.expression(arg)?;
                        self.emit(Opcode::Pop, 0)?;
                        self.constant(Value::Bool(true))?;
                    }
                    return Ok(());
                }
                if *op == UnaryOp::Typeof
                    && matches!(&**arg, Expr::Identifier(name) if self.resolve(name).is_none() && !matches!(name.as_str(), "undefined" | "NaN" | "Infinity" | "String" | "Symbol" | "RegExp" | "Object" | "Reflect" | "Number" | "Boolean" | "globalThis"))
                {
                    self.constant(Value::String("undefined".into()))?;
                } else {
                    self.expression(arg)?;
                    self.emit(opcode, 0)?;
                }
            }
            Expr::Binary { op, left, right } => {
                let opcode = binary_opcode(*op)?;
                self.expression(left)?;
                self.expression(right)?;
                self.emit(opcode, 0)?;
            }
            Expr::Logical { op, left, right } => {
                self.expression(left)?;
                self.emit(Opcode::Dup, 0)?;
                let jump = self.emit(
                    match op {
                        LogicalOp::And => Opcode::JumpIfFalse,
                        LogicalOp::Or => Opcode::JumpIfTrue,
                        LogicalOp::Nullish => Opcode::JumpIfNotNullish,
                    },
                    0,
                )?;
                self.emit(Opcode::Pop, 0)?;
                self.expression(right)?;
                self.patch(jump, self.offset()?);
            }
            Expr::Conditional { test, consequent, alternate } => {
                self.expression(test)?;
                let no = self.emit(Opcode::JumpIfFalse, 0)?;
                self.expression(consequent)?;
                let end = self.emit(Opcode::Jump, 0)?;
                self.patch(no, self.offset()?);
                self.expression(alternate)?;
                self.patch(end, self.offset()?);
            }
            Expr::Array(elements) => {
                if elements.iter().any(|element| matches!(element, Some(ArrayElement::Spread(_)))) {
                    self.emit(Opcode::NewArray, 0)?;
                    for element in elements {
                        let kind = match element {
                            None => {
                                self.constant(Value::Undefined)?;
                                1
                            }
                            Some(ArrayElement::Normal(value)) => {
                                self.expression(value)?;
                                0
                            }
                            Some(ArrayElement::Spread(value)) => {
                                self.expression(value)?;
                                2
                            }
                        };
                        self.emit(Opcode::ArrayPush, kind)?;
                    }
                    return Ok(());
                }
                let length = u32::try_from(elements.len()).map_err(|_| CompileError::ProgramTooLarge)?;
                self.emit(Opcode::NewArray, length)?;
                for (index, element) in elements.iter().enumerate() {
                    let Some(element) = element else { continue };
                    let ArrayElement::Normal(value) = element else { return Err(CompileError::Unsupported("array spread")) };
                    self.emit(Opcode::Dup, 0)?;
                    self.constant(Value::String(index.to_string().into()))?;
                    self.expression(value)?;
                    self.emit(Opcode::DefineData, 0)?;
                    self.emit(Opcode::Pop, 0)?;
                }
            }
            Expr::Object(properties) => {
                self.emit(Opcode::NewObject, 0)?;
                let mut has_proto = false;
                for property in properties {
                    if let ObjectProp::Method { key, function } | ObjectProp::Accessor { key, function, .. } = property {
                        self.emit(Opcode::Dup, 0)?;
                        self.property_key(key)?;
                        self.function(function, false)?;
                        std::rc::Rc::get_mut(self.bytecode.functions.last_mut().unwrap()).unwrap().constructible = false;
                        if let ObjectProp::Accessor { getter, .. } = property {
                            self.emit(Opcode::DefineAccessor, u32::from(!getter))?;
                        } else {
                            self.emit(Opcode::DefineData, 0)?;
                        }
                        self.emit(Opcode::Pop, 0)?;
                        continue;
                    }
                    let ObjectProp::KeyValue { key, value, shorthand } = property else { return Err(CompileError::Unsupported("object spread")) };
                    self.emit(Opcode::Dup, 0)?;
                    let prototype_key = match key {
                        PropertyKey::Identifier(name) => name == "__proto__",
                        PropertyKey::String(name) => name == "__proto__",
                        _ => false,
                    };
                    if !shorthand && prototype_key {
                        if has_proto {
                            return Err(CompileError::InvalidSyntax("duplicate literal __proto__ setter"));
                        }
                        has_proto = true;
                        self.expression(value)?;
                        self.emit(Opcode::SetLiteralPrototype, 0)?;
                    } else {
                        self.property_key(key)?;
                        self.expression(value)?;
                        self.emit(Opcode::DefineData, 0)?;
                        self.emit(Opcode::Pop, 0)?;
                    }
                }
            }
            Expr::Member { .. } => {
                self.member_reference(expr)?;
                self.emit(Opcode::GetProperty, 0)?;
            }
            Expr::Assign { op, target, value } => self.assignment(*op, target, value)?,
            Expr::Update { op, arg, prefix } => {
                if let Expr::Identifier(name) = &**arg {
                    let slot = self.resolve(name).ok_or(CompileError::Unsupported("implicit global assignment"))?;
                    self.emit(Opcode::GetBinding, slot)?;
                    self.emit(Opcode::ToNumber, 0)?;
                    if !prefix {
                        self.emit(Opcode::Dup, 0)?;
                    }
                    self.constant(Value::Number(1.0))?;
                    self.emit(if *op == UpdateOp::Inc { Opcode::Add } else { Opcode::Subtract }, 0)?;
                    self.emit(Opcode::StoreBinding, slot)?;
                    if !prefix {
                        self.emit(Opcode::Pop, 0)?;
                    }
                } else {
                    self.member_reference(arg)?;
                    self.emit(Opcode::UpdateProperty, u32::from(*op == UpdateOp::Dec) | (u32::from(*prefix) << 1))?;
                }
            }
            Expr::Template { quasis, expressions } => {
                if quasis.len() != expressions.len() + 1 {
                    return Err(CompileError::InvalidSyntax("invalid template AST"));
                }
                self.constant(Value::String(quasis[0].clone()))?;
                for (expr, tail) in expressions.iter().zip(&quasis[1..]) {
                    self.expression(expr)?;
                    self.emit(Opcode::ToString, 0)?;
                    self.emit(Opcode::Add, 0)?;
                    self.constant(Value::String(tail.clone()))?;
                    self.emit(Opcode::Add, 0)?;
                }
            }
            Expr::Call { callee, args } | Expr::New { callee, args } => {
                let construct = matches!(expr, Expr::New { .. });
                if !construct && matches!(&**callee, Expr::Member { .. }) {
                    self.member_reference(callee)?;
                    self.emit(Opcode::GetMethod, 0)?;
                } else {
                    self.expression(callee)?;
                    self.constant(Value::Undefined)?;
                }
                if args.iter().any(|arg| matches!(arg, Argument::Spread(_))) {
                    self.emit(Opcode::NewArray, 0)?;
                    for arg in args {
                        let (value, kind) = match arg {
                            Argument::Normal(value) => (value, 0),
                            Argument::Spread(value) => (value, 2),
                        };
                        self.expression(value)?;
                        self.emit(Opcode::ArrayPush, kind)?;
                    }
                    self.emit(Opcode::CallSpread, u32::from(construct))?;
                    return Ok(());
                }
                for arg in args {
                    let Argument::Normal(expr) = arg else { unreachable!("spread calls are emitted above") };
                    self.expression(expr)?;
                }
                self.emit(if construct { Opcode::Construct } else { Opcode::Call }, u32::try_from(args.len()).map_err(|_| CompileError::ProgramTooLarge)?)?;
            }
            Expr::This => {
                self.emit(Opcode::This, 0)?;
            }
            Expr::Function(function) => self.function(function, false)?,
            Expr::Arrow { params, body } => {
                let body = match body {
                    ArrowBody::Expr(expr) => vec![Stmt::Return(Some(*expr.clone()))],
                    ArrowBody::Block(body) => body.clone(),
                };
                self.function(&Function { name: None, params: params.clone(), body }, true)?;
            }
        }
        Ok(())
    }

    fn for_of(&mut self, left: &ForHead, right: &Expr, body: &Stmt) -> Result<(), CompileError> {
        self.emit(Opcode::ClearCompletion, 0)?;
        let (pattern, kind) = match left {
            ForHead::Decl(kind, pattern) => (pattern, Some(*kind)),
            ForHead::Pattern(pattern) => (pattern, None),
        };
        let name = binding_name(pattern)?.to_owned();
        let lexical = kind.is_some_and(|kind| kind != DeclKind::Var);
        let mut declarations = vec![("*iterator*".to_owned(), DeclKind::Let)];
        if lexical {
            declarations.push((name.clone(), kind.unwrap()));
        }
        self.enter_scope(declarations, &BTreeSet::new(), false)?;
        let iterator = self.resolve("*iterator*").unwrap();
        self.expression(right)?;
        self.emit(Opcode::GetIterator, 0)?;
        self.emit(Opcode::InitializeBinding, iterator)?;
        let start = self.offset()?;
        self.emit(Opcode::GetBinding, iterator)?;
        let exit = self.emit(Opcode::IteratorStep, 0)?;
        self.loops.push(Loop { scope_depth: self.scopes.len(), breaks: Vec::new(), continues: Vec::new(), iterator: Some(iterator) });
        if lexical {
            self.enter_scope(vec![(name.clone(), kind.unwrap())], &BTreeSet::new(), false)?;
        }
        let slot = self.resolve(&name).ok_or(CompileError::Unsupported("implicit global assignment"))?;
        self.emit(if lexical { Opcode::InitializeBinding } else { Opcode::StoreBinding }, slot)?;
        if !lexical {
            self.emit(Opcode::Pop, 0)?;
        }
        self.statement(body, false)?;
        if lexical {
            self.leave_scope()?;
        }
        self.emit(Opcode::Jump, start)?;
        let end = self.offset()?;
        self.patch(exit, end);
        let context = self.loops.pop().unwrap();
        for jump in context.breaks {
            self.patch(jump, end);
        }
        for jump in context.continues {
            self.patch(jump, start);
        }
        self.leave_scope()?;
        Ok(())
    }

    fn assignment(&mut self, op: AssignOp, target: &Expr, value: &Expr) -> Result<(), CompileError> {
        let binding = if let Expr::Identifier(name) = target {
            Some(self.resolve(name).ok_or(CompileError::Unsupported("implicit global assignment"))?)
        } else {
            self.member_reference(target)?;
            None
        };
        if op != AssignOp::Assign {
            if let Some(slot) = binding {
                self.emit(Opcode::GetBinding, slot)?;
            } else {
                self.emit(Opcode::Dup2, 0)?;
                self.emit(Opcode::GetProperty, 0)?;
            }
        }
        self.expression(value)?;
        if let Some(opcode) = match op {
            AssignOp::Assign => None,
            AssignOp::AddAssign => Some(Opcode::Add),
            AssignOp::SubAssign => Some(Opcode::Subtract),
            AssignOp::MulAssign => Some(Opcode::Multiply),
            AssignOp::DivAssign => Some(Opcode::Divide),
            AssignOp::ModAssign => Some(Opcode::Remainder),
        } {
            self.emit(opcode, 0)?;
        }
        if let Some(slot) = binding {
            self.emit(Opcode::StoreBinding, slot)?;
        } else {
            self.emit(Opcode::SetProperty, 0)?;
        }
        Ok(())
    }

    fn member_reference(&mut self, target: &Expr) -> Result<(), CompileError> {
        let Expr::Member { object, property, computed } = target else { return Err(CompileError::InvalidSyntax("invalid assignment/member AST")) };
        self.expression(object)?;
        if *computed {
            self.expression(property)?;
        } else if let Expr::Identifier(name) = &**property {
            self.constant(Value::String(name.clone().into()))?;
        } else {
            return Err(CompileError::InvalidSyntax("invalid non-computed member AST"));
        }
        self.emit(Opcode::ToPropertyKey, 0)?;
        Ok(())
    }

    fn property_key(&mut self, key: &PropertyKey) -> Result<(), CompileError> {
        match key {
            PropertyKey::Identifier(name) => self.constant(Value::String(name.clone().into()))?,
            PropertyKey::String(name) => self.constant(Value::String(name.clone()))?,
            PropertyKey::Number(n) => self.constant(Value::Number(*n))?,
            PropertyKey::Computed(expr) => self.expression(expr)?,
        }
        self.emit(Opcode::ToPropertyKey, 0)?;
        Ok(())
    }

    fn function(&mut self, function: &Function, arrow: bool) -> Result<(), CompileError> {
        let child_budget = self.max_bytecode_bytes.saturating_sub(self.offset()?);
        let mut child = Compiler {
            bytecode: Bytecode::empty(),
            names: vec![HashMap::new()],
            scopes: Vec::new(),
            loops: Vec::new(),
            max_bytecode_bytes: child_budget,
            function: true,
            local_scope: 1,
        };
        child.bytecode.strict = self.bytecode.strict || strict_body(&function.body);
        child.bytecode.arrow = arrow;
        child.bytecode.constructible = !arrow;
        child.bytecode.function_name = function.name.clone().unwrap_or_default();
        child.bytecode.function_length = function.params.iter().take_while(|p| !p.rest && p.default.is_none()).count() as u32;
        let mut visible = std::collections::BTreeMap::new();
        for scope in &self.names {
            visible.extend(scope.iter().map(|(name, slot)| (name.clone(), *slot)));
        }
        for (name, slot) in visible {
            let index = child.bytecode.bindings.len() as u32;
            child.names[0].insert(name, index);
            child.bytecode.bindings.push(self.bytecode.bindings[slot as usize].clone());
            child.bytecode.captures.push(slot);
        }
        let mut vars = var_names(&function.body)?;
        for param in &function.params {
            vars.insert(binding_name(&param.pattern)?.to_owned());
        }
        child.enter_scope(lexical_names(&function.body)?, &vars, true)?;
        for (index, param) in function.params.iter().enumerate() {
            let slot = child.resolve(binding_name(&param.pattern)?).unwrap();
            child.emit(if param.rest { Opcode::RestArguments } else { Opcode::Argument }, index as u32)?;
            if let Some(default) = &param.default {
                child.emit(Opcode::Dup, 0)?;
                child.constant(Value::Undefined)?;
                child.emit(Opcode::StrictEqual, 0)?;
                let skip = child.emit(Opcode::JumpIfFalse, 0)?;
                child.emit(Opcode::Pop, 0)?;
                child.expression(default)?;
                child.patch(skip, child.offset()?);
            }
            child.emit(Opcode::InitializeBinding, slot)?;
        }
        child.statements(&function.body)?;
        child.constant(Value::Undefined)?;
        child.emit(Opcode::Return, 0)?;
        let child_bytes = child_budget - child.max_bytecode_bytes + child.offset()?;
        self.max_bytecode_bytes = self.max_bytecode_bytes.checked_sub(child_bytes).ok_or(CompileError::ProgramTooLarge)?;
        let index = self.bytecode.functions.len() as u32;
        self.bytecode.functions.push(std::rc::Rc::new(child.bytecode));
        self.emit(Opcode::Closure, index)?;
        Ok(())
    }
}

fn strict_body(body: &[Stmt]) -> bool {
    body.iter().take_while(|stmt| matches!(stmt, Stmt::Expr(Expr::String(_)))).any(|stmt| matches!(stmt, Stmt::Expr(Expr::String(s)) if s == "use strict"))
}

fn binary_opcode(op: BinaryOp) -> Result<Opcode, CompileError> {
    Ok(match op {
        BinaryOp::Add => Opcode::Add,
        BinaryOp::Sub => Opcode::Subtract,
        BinaryOp::Mul => Opcode::Multiply,
        BinaryOp::Div => Opcode::Divide,
        BinaryOp::Mod => Opcode::Remainder,
        BinaryOp::StrictEq => Opcode::StrictEqual,
        BinaryOp::StrictNotEq => Opcode::StrictNotEqual,
        BinaryOp::Lt => Opcode::Less,
        BinaryOp::Gt => Opcode::Greater,
        BinaryOp::LtEq => Opcode::LessEqual,
        BinaryOp::GtEq => Opcode::GreaterEqual,
        BinaryOp::Instanceof => Opcode::Instanceof,
        _ => return Err(CompileError::Unsupported("loose equality or in")),
    })
}

fn binding_name(pattern: &Pattern) -> Result<&str, CompileError> {
    match pattern {
        Pattern::Identifier(name) => Ok(name),
        _ => Err(CompileError::Unsupported("destructuring bindings")),
    }
}

fn declarations_names(kind: DeclKind, declarations: &[VarDeclarator]) -> Result<Vec<(String, DeclKind)>, CompileError> {
    declarations.iter().map(|decl| Ok((binding_name(&decl.pattern)?.to_string(), kind))).collect()
}

fn lexical_names(statements: &[Stmt]) -> Result<Vec<(String, DeclKind)>, CompileError> {
    let mut names = Vec::new();
    for statement in statements {
        if let Stmt::VarDecl(kind, declarations) = statement {
            if *kind != DeclKind::Var {
                names.extend(declarations_names(*kind, declarations)?);
            }
        }
    }
    Ok(names)
}

fn var_names(statements: &[Stmt]) -> Result<BTreeSet<String>, CompileError> {
    let mut names = BTreeSet::new();
    let mut pending: Vec<_> = statements.iter().collect();
    while let Some(statement) = pending.pop() {
        match statement {
            Stmt::FunctionDecl(function) => {
                names.insert(function.name.clone().expect("declaration has a name"));
            }
            Stmt::VarDecl(DeclKind::Var, declarations) => {
                for declaration in declarations {
                    names.insert(binding_name(&declaration.pattern)?.to_string());
                }
            }
            Stmt::Block(body) => pending.extend(body),
            Stmt::If { consequent, alternate, .. } => {
                pending.push(consequent);
                if let Some(alternate) = alternate {
                    pending.push(alternate);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => pending.push(body),
            Stmt::For { init, body, .. } => {
                if let Some(ForInit::VarDecl(DeclKind::Var, declarations)) = init {
                    for declaration in declarations {
                        names.insert(binding_name(&declaration.pattern)?.to_string());
                    }
                }
                pending.push(body);
            }
            Stmt::ForOf { left, body, .. } => {
                if let ForHead::Decl(DeclKind::Var, pattern) = left {
                    names.insert(binding_name(pattern)?.to_string());
                }
                pending.push(body);
            }
            _ => {}
        }
    }
    Ok(names)
}
