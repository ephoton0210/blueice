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
            Self::ProgramTooLarge => f.write_str("BlueJS program exceeds the u32 bytecode address space"),
        }
    }
}
impl std::error::Error for CompileError {}

/// Compiles the supported executable subset. The parser deliberately
/// accepts more than the VM can run; unsupported syntax is rejected even
/// in unreachable branches, before any execution or heap mutation.
pub fn compile(program: &Program) -> Result<Bytecode, CompileError> {
    let mut compiler = Compiler { bytecode: Bytecode::empty(), names: Vec::new(), scopes: Vec::new(), loops: Vec::new() };
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
}

struct Compiler {
    bytecode: Bytecode,
    names: Vec<HashMap<String, u32>>,
    scopes: Vec<u32>,
    loops: Vec<Loop>,
}

impl Compiler {
    fn offset(&self) -> Result<u32, CompileError> {
        u32::try_from(self.bytecode.code.len()).map_err(|_| CompileError::ProgramTooLarge)
    }

    fn emit(&mut self, opcode: Opcode, operand: u32) -> Result<usize, CompileError> {
        let offset = self.offset()? as usize;
        if offset.checked_add(opcode.width()).is_none_or(|end| end > u32::MAX as usize) {
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
            self.bytecode.bindings.push(Binding { name: name.clone(), mutable: kind != DeclKind::Const });
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
            self.statement(statement, true)?;
        }
        Ok(())
    }

    fn statement(&mut self, statement: &Stmt, declarations_allowed: bool) -> Result<(), CompileError> {
        match statement {
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
            Stmt::Break | Stmt::Continue => {
                let Some(context) = self.loops.last() else { return Err(CompileError::InvalidSyntax("break/continue requires an enclosing loop")) };
                let scopes: Vec<_> = self.scopes[context.scope_depth..].iter().rev().copied().collect();
                for scope in scopes {
                    self.emit(Opcode::LeaveScope, scope)?;
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
            let slot = if kind == DeclKind::Var { self.names[0][name] } else { self.names.last().unwrap()[name] };
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
        self.loops.push(Loop { scope_depth: self.scopes.len(), breaks: Vec::new(), continues: Vec::new() });
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
                        _ => {
                            let index = u32::try_from(self.bytecode.constants.len()).map_err(|_| CompileError::ProgramTooLarge)?;
                            self.bytecode.constants.push(Value::String(name.clone()));
                            self.emit(Opcode::UnboundName, index)?;
                        }
                    }
                }
            }
            Expr::Unary { op, arg } => {
                if *op == UnaryOp::Typeof && matches!(&**arg, Expr::Identifier(name) if self.resolve(name).is_none() && !matches!(name.as_str(), "undefined" | "NaN" | "Infinity")) {
                    self.constant(Value::String("undefined".into()))?;
                } else {
                    self.expression(arg)?;
                    self.emit(
                        match op {
                            UnaryOp::Neg => Opcode::Negate,
                            UnaryOp::Plus => Opcode::ToNumber,
                            UnaryOp::Not => Opcode::Not,
                            UnaryOp::Typeof => Opcode::Typeof,
                        },
                        0,
                    )?;
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
                let length = u32::try_from(elements.len()).map_err(|_| CompileError::ProgramTooLarge)?;
                self.emit(Opcode::NewArray, length)?;
                for (index, element) in elements.iter().enumerate() {
                    let Some(element) = element else { continue };
                    let ArrayElement::Normal(value) = element else { return Err(CompileError::Unsupported("array spread")) };
                    self.emit(Opcode::Dup, 0)?;
                    self.constant(Value::String(index.to_string()))?;
                    self.expression(value)?;
                    self.emit(Opcode::SetProperty, 0)?;
                    self.emit(Opcode::Pop, 0)?;
                }
            }
            Expr::Object(properties) => {
                self.emit(Opcode::NewObject, 0)?;
                let mut has_proto = false;
                for property in properties {
                    let ObjectProp::KeyValue { key, value, shorthand } = property else { return Err(CompileError::Unsupported("object spread")) };
                    self.emit(Opcode::Dup, 0)?;
                    if !shorthand && matches!(key, PropertyKey::Identifier(name) | PropertyKey::String(name) if name == "__proto__") {
                        if has_proto {
                            return Err(CompileError::InvalidSyntax("duplicate literal __proto__ setter"));
                        }
                        has_proto = true;
                        self.expression(value)?;
                        self.emit(Opcode::SetLiteralPrototype, 0)?;
                    } else {
                        self.property_key(key)?;
                        self.expression(value)?;
                        self.emit(Opcode::SetProperty, 0)?;
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
            _ => return Err(CompileError::Unsupported("functions/calls, constructors or this")),
        }
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
            self.constant(Value::String(name.clone()))?;
        } else {
            return Err(CompileError::InvalidSyntax("invalid non-computed member AST"));
        }
        self.emit(Opcode::ToString, 0)?;
        Ok(())
    }

    fn property_key(&mut self, key: &PropertyKey) -> Result<(), CompileError> {
        match key {
            PropertyKey::Identifier(name) | PropertyKey::String(name) => self.constant(Value::String(name.clone()))?,
            PropertyKey::Number(n) => self.constant(Value::Number(*n))?,
            PropertyKey::Computed(expr) => self.expression(expr)?,
        }
        self.emit(Opcode::ToString, 0)?;
        Ok(())
    }
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
        _ => return Err(CompileError::Unsupported("loose equality, in or instanceof")),
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
            _ => {}
        }
    }
    Ok(names)
}
