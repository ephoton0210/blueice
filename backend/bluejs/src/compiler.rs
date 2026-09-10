// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! AST -> operand-stack bytecode, per Phase 13's first execution slice.
//! Binding resolution happens here; runtime execution never walks an AST.
//! Every lexical scope has its own slots, reset on entry/exit. Abrupt
//! loop exits emit the same scope cleanup as ordinary block exits.

use crate::bytecode::{AbruptJump, Binding, Handler};
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
    let mut compiler = Compiler { bytecode: Bytecode::empty(), names: Vec::new(), scopes: Vec::new(), loops: Vec::new(), catch_var_slots: Vec::new(), max_bytecode_bytes, function: false, local_scope: 0, with_depth: 0 };
    compiler.bytecode.strict = strict_body(&program.body);
    let vars = var_names(&program.body)?;
    compiler.enter_scope(lexical_names(&program.body)?, &vars, true)?;
    compiler.statements(&program.body)?;
    compiler.emit(Opcode::Halt, 0)?;
    Ok(compiler.bytecode)
}

/// Compiles direct-eval source with cells for the caller's visible bindings.
/// The runtime supplies `visible` from its active lexical environments and
/// installs the matching cells before running the resulting bytecode.
pub(crate) fn compile_eval(program: &Program, visible: &[(String, Binding, u32)], strict: bool) -> Result<Bytecode, CompileError> {
    let mut compiler = Compiler {
        bytecode: Bytecode::empty(),
        names: vec![HashMap::new()],
        scopes: Vec::new(),
        loops: Vec::new(),
        catch_var_slots: Vec::new(),
        max_bytecode_bytes: u32::MAX,
        function: false,
        local_scope: 1,
        with_depth: 0,
    };
    compiler.bytecode.strict = strict || strict_body(&program.body);
    for (name, binding, caller_slot) in visible {
        let slot = u32::try_from(compiler.bytecode.bindings.len()).map_err(|_| CompileError::ProgramTooLarge)?;
        compiler.names[0].insert(name.clone(), slot);
        compiler.bytecode.bindings.push(binding.clone());
        compiler.bytecode.captures.push(*caller_slot);
    }
    let vars = var_names(&program.body)?;
    let new_vars = vars.into_iter().filter(|name| !compiler.names[0].contains_key(name)).collect();
    compiler.enter_scope(lexical_names(&program.body)?, &new_vars, true)?;
    compiler.statements(&program.body)?;
    compiler.emit(Opcode::Halt, 0)?;
    Ok(compiler.bytecode)
}

struct Loop {
    scope_depth: usize,
    breaks: Vec<(usize, usize)>,
    continues: Option<Vec<(usize, usize)>>,
    iterator: Option<u32>,
}

struct Compiler {
    bytecode: Bytecode,
    names: Vec<HashMap<String, u32>>,
    scopes: Vec<u32>,
    loops: Vec<Loop>,
    // Annex B permits a simple catch parameter to be redeclared with `var`
    // in its block. Those declaration writes target the catch binding.
    catch_var_slots: Vec<HashMap<String, u32>>,
    max_bytecode_bytes: u32,
    function: bool,
    local_scope: usize,
    with_depth: usize,
}

#[derive(Clone, Copy)]
struct FunctionCompileOptions {
    constructible: bool,
    force_strict: bool,
    class_constructor: bool,
    derived_constructor: bool,
    default_derived_constructor: bool,
}

impl FunctionCompileOptions {
    fn class_method() -> Self {
        Self { constructible: false, force_strict: true, class_constructor: false, derived_constructor: false, default_derived_constructor: false }
    }
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
            Stmt::Try { block, handler, finalizer } => self.try_statement(block, handler.as_ref(), finalizer.as_deref())?,
            Stmt::With { object, body } => {
                if self.bytecode.strict {
                    return Err(CompileError::InvalidSyntax("with is forbidden in strict mode"));
                }
                self.expression(object)?;
                self.emit(Opcode::EnterWith, 0)?;
                self.with_depth += 1;
                let result = self.statement(body, false);
                self.with_depth -= 1;
                result?;
                self.emit(Opcode::LeaveWith, 0)?;
            }
            Stmt::FunctionDecl(_) => {}
            Stmt::ClassDecl(class) => {
                let slot = self.resolve(class.name.as_deref().expect("class declaration has a name")).unwrap();
                self.class_expression_with_binding(class, None, Some(slot))?;
            }
            Stmt::Expr(Expr::Class(class)) => {
                self.class_expression(class, None)?;
                self.emit(Opcode::Pop, 0)?;
            }
            Stmt::Return(value) => {
                if !self.function {
                    return Err(CompileError::InvalidSyntax("return requires a function"));
                }
                if let Some(args) = value.as_ref().and_then(|value| self.self_tail_call_args(value)) {
                    for argument in args {
                        let Argument::Normal(value) = argument else { unreachable!("self tail calls exclude spread arguments") };
                        self.expression(value)?;
                    }
                    let iterators: Vec<_> = self.loops.iter().rev().filter_map(|context| context.iterator).collect();
                    for iterator in iterators {
                        self.emit(Opcode::GetBinding, iterator)?;
                        self.emit(Opcode::IteratorClose, 0)?;
                    }
                    self.emit(Opcode::TailRecur, u32::try_from(args.len()).map_err(|_| CompileError::ProgramTooLarge)?)?;
                    return Ok(());
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
            Stmt::ForIn { left, right, body } => self.for_in(left, right, body)?,
            Stmt::ForOf { left, right, body } => self.for_of(left, right, body)?,
            Stmt::Switch { discriminant, cases } => self.switch_statement(discriminant, cases)?,
            Stmt::Break | Stmt::Continue => {
                let index = if matches!(statement, Stmt::Break) {
                    self.loops.len().checked_sub(1)
                } else {
                    self.loops.iter().rposition(|context| context.continues.is_some())
                };
                let Some(index) = index else { return Err(CompileError::InvalidSyntax(if matches!(statement, Stmt::Break) { "break requires an enclosing loop or switch" } else { "continue requires an enclosing loop" })) };
                let context = &self.loops[index];
                let scopes: Vec<_> = self.scopes[context.scope_depth..].iter().rev().copied().collect();
                let iterator = context.iterator;
                // A direct jump would skip a surrounding `finally`. Route to
                // a local cleanup gateway first; a handler resumes there only
                // after its finalizer has completed, so lexical environments
                // stay live while `finally` runs.
                let control = self.bytecode.abrupt_jumps.len();
                let control_operand = u32::try_from(control).map_err(|_| CompileError::ProgramTooLarge)?;
                self.bytecode.abrupt_jumps.push(AbruptJump { cleanup: 0, target: 0 });
                self.emit(Opcode::AbruptJump, control_operand)?;
                let cleanup = self.offset()?;
                self.bytecode.abrupt_jumps[control].cleanup = cleanup;
                if matches!(statement, Stmt::Break) {
                    if let Some(iterator) = iterator {
                        // Close only after a surrounding finalizer has had a
                        // chance to replace this break with another completion.
                        self.emit(Opcode::GetBinding, iterator)?;
                        self.emit(Opcode::IteratorClose, 0)?;
                    }
                }
                for scope in scopes {
                    self.emit(Opcode::LeaveScope, scope)?;
                }
                let jump = self.emit(Opcode::Jump, 0)?;
                let context = &mut self.loops[index];
                if matches!(statement, Stmt::Break) {
                    context.breaks.push((jump, control));
                } else {
                    context.continues.as_mut().expect("selected context is a loop").push((jump, control));
                }
            }
        }
        Ok(())
    }

    fn scoped_statements(&mut self, statements: &[Stmt]) -> Result<(), CompileError> {
        self.enter_scope(lexical_names(statements)?, &var_names(statements)?, false)?;
        self.statements(statements)?;
        self.leave_scope()
    }

    fn switch_statement(&mut self, discriminant: &Expr, cases: &[SwitchCase]) -> Result<(), CompileError> {
        self.emit(Opcode::ClearCompletion, 0)?;
        let mut lexical = Vec::new();
        let mut vars = BTreeSet::new();
        for case in cases {
            lexical.extend(lexical_names(&case.consequent)?);
            vars.extend(var_names(&case.consequent)?);
        }
        self.enter_scope(lexical, &vars, false)?;
        self.expression(discriminant)?;

        let mut case_entries = vec![None; cases.len()];
        for (index, case) in cases.iter().enumerate() {
            if let Some(test) = &case.test {
                self.emit(Opcode::Dup, 0)?;
                self.expression(test)?;
                self.emit(Opcode::StrictEqual, 0)?;
                let no_match = self.emit(Opcode::JumpIfFalse, 0)?;
                case_entries[index] = Some(self.emit(Opcode::Jump, 0)?);
                self.patch(no_match, self.offset()?);
            }
        }
        let no_match = self.emit(Opcode::Jump, 0)?;
        let no_match_cleanup = self.offset()?;
        self.emit(Opcode::Pop, 0)?;
        let no_match_exit = self.emit(Opcode::Jump, 0)?;

        let mut case_stubs = Vec::with_capacity(cases.len());
        let mut body_jumps = Vec::with_capacity(cases.len());
        for _ in cases {
            case_stubs.push(self.offset()?);
            self.emit(Opcode::Pop, 0)?;
            body_jumps.push(self.emit(Opcode::Jump, 0)?);
        }
        for (entry, stub) in case_entries.into_iter().zip(&case_stubs) {
            if let Some(entry) = entry {
                self.patch(entry, *stub);
            }
        }
        let default = cases.iter().position(|case| case.test.is_none());
        self.patch(no_match, default.map(|index| case_stubs[index]).unwrap_or(no_match_cleanup));

        self.loops.push(Loop { scope_depth: self.scopes.len(), breaks: Vec::new(), continues: None, iterator: None });
        for (case, jump) in cases.iter().zip(body_jumps) {
            self.patch(jump, self.offset()?);
            self.statements(&case.consequent)?;
        }
        let end = self.offset()?;
        self.patch(no_match_exit, end);
        let context = self.loops.pop().expect("switch control is active");
        for (jump, control) in context.breaks {
            self.patch(jump, end);
            self.bytecode.abrupt_jumps[control].target = end;
        }
        self.leave_scope()?;
        Ok(())
    }

    /// Compiles `try` as fixed bytecode plus immutable handler metadata. A
    /// runtime frame records dynamic stack/scope depths, so a throw can safely
    /// enter a catch block or run a finalizer without walking the AST.
    fn try_statement(&mut self, block: &[Stmt], handler: Option<&CatchClause>, finalizer: Option<&[Stmt]>) -> Result<(), CompileError> {
        let handler_index = u32::try_from(self.bytecode.handlers.len()).map_err(|_| CompileError::ProgramTooLarge)?;
        self.bytecode.handlers.push(Handler { try_start: 0, try_end: 0, catch: None, catch_end: None, finally: None });
        self.emit(Opcode::PushHandler, handler_index)?;

        // Each TryBlock has its own Completion. An empty block must not leak
        // the value of the preceding statement into TryStatement's
        // UpdateEmpty step.
        self.emit(Opcode::ClearCompletion, 0)?;
        self.bytecode.handlers[handler_index as usize].try_start = self.offset()?;
        self.scoped_statements(block)?;
        self.bytecode.handlers[handler_index as usize].try_end = self.offset()?;
        self.emit(Opcode::PopHandler, 0)?;
        if finalizer.is_some() { self.emit(Opcode::SaveCompletion, 0)?; }
        let normal_exit = self.emit(Opcode::Jump, 0)?;

        let catch_exit = if let Some(catch) = handler {
            let start = self.offset()?;
            self.bytecode.handlers[handler_index as usize].catch = Some(start);
            let parameter_bound_names = catch.param.as_ref().map(pattern_names).unwrap_or_default();
            if self.bytecode.strict && parameter_bound_names.iter().any(|name| matches!(name.as_str(), "eval" | "arguments")) {
                return Err(CompileError::InvalidSyntax("strict catch parameters cannot bind eval or arguments"));
            }
            if catch_lexical_names(&catch.body).into_iter().any(|name| parameter_bound_names.contains(&name)) {
                return Err(CompileError::InvalidSyntax("a catch parameter conflicts with a lexical declaration"));
            }
            let parameter_names = parameter_bound_names.into_iter().map(|name| (name, DeclKind::Let)).collect();
            self.enter_scope(parameter_names, &BTreeSet::new(), false)?;
            let mut catch_var_slots = HashMap::new();
            if let Some(Pattern::Identifier(name)) = &catch.param {
                catch_var_slots.insert(name.clone(), self.resolve(name).expect("catch parameter was declared"));
            }
            self.catch_var_slots.push(catch_var_slots);
            if let Some(param) = &catch.param {
                // The VM places the caught JavaScript value on the stack at
                // this destination. Binding initialization consumes it.
                self.bind_pattern(param, DeclKind::Let)?;
            } else {
                self.emit(Opcode::Pop, 0)?;
            }
            // CatchParameter initialization is not part of the Block's
            // completion value.
            self.emit(Opcode::ClearCompletion, 0)?;
            self.enter_scope(lexical_names(&catch.body)?, &var_names(&catch.body)?, false)?;
            self.statements(&catch.body)?;
            self.leave_scope()?;
            self.catch_var_slots.pop().expect("catch var override is active");
            self.leave_scope()?;
            self.bytecode.handlers[handler_index as usize].catch_end = Some(self.offset()?);
            self.emit(Opcode::PopHandler, 0)?;
            if finalizer.is_some() { self.emit(Opcode::SaveCompletion, 0)?; }
            Some(self.emit(Opcode::Jump, 0)?)
        } else {
            None
        };

        if let Some(finalizer) = finalizer {
            let start = self.offset()?;
            self.bytecode.handlers[handler_index as usize].finally = Some(start);
            // A normal finally restores its saved prior Completion only when
            // this block remains empty; a non-empty finalizer keeps its own.
            self.emit(Opcode::ClearCompletion, 0)?;
            self.scoped_statements(finalizer)?;
            // On a normal entry the handler has already been popped and this
            // restores the preceding non-empty completion. On an abrupt entry
            // it replays the pending completion after the finalizer finishes.
            self.emit(Opcode::ResumeCompletion, handler_index)?;
            let end = self.offset()?;
            self.patch(normal_exit, start);
            if let Some(exit) = catch_exit { self.patch(exit, start); }
            debug_assert!(end as usize <= self.bytecode.code.len());
        } else {
            let end = self.offset()?;
            self.patch(normal_exit, end);
            if let Some(exit) = catch_exit { self.patch(exit, end); }
        }
        Ok(())
    }

    fn declarations(&mut self, kind: DeclKind, declarations: &[VarDeclarator]) -> Result<(), CompileError> {
        for declaration in declarations {
            if kind == DeclKind::Const && declaration.init.is_none() {
                return Err(CompileError::InvalidSyntax("const requires an initializer"));
            }
            if matches!(declaration.pattern, Pattern::Identifier(_)) && kind == DeclKind::Var && declaration.init.is_none() {
                continue;
            }
            if let Some(value) = &declaration.init {
                self.expression(value)?
            } else {
                if !matches!(declaration.pattern, Pattern::Identifier(_)) {
                    return Err(CompileError::InvalidSyntax("a destructuring declaration requires an initializer"));
                }
                self.constant(Value::Undefined)?
            }
            self.bind_pattern(&declaration.pattern, kind)?;
        }
        Ok(())
    }

    /// Consumes the value at the top of the operand stack and performs a
    /// BindingInitialization for every identifier in `pattern`.  The bytecode
    /// keeps iterator records on the stack while descending into an array
    /// pattern so abrupt completions can close every active iterator.
    fn bind_pattern(&mut self, pattern: &Pattern, kind: DeclKind) -> Result<(), CompileError> {
        match pattern {
            Pattern::Identifier(name) => {
                let slot = if kind == DeclKind::Var {
                    self.catch_var_slots
                        .iter()
                        .rev()
                        .find_map(|slots| slots.get(name))
                        .copied()
                        .or_else(|| self.names[self.local_scope].get(name).copied())
                        .or_else(|| self.resolve(name))
                        .expect("var declaration has a function or eval binding")
                } else { self.names.last().unwrap()[name] };
                if kind == DeclKind::Var {
                    self.emit(Opcode::StoreBinding, slot)?;
                    self.emit(Opcode::Pop, 0)?;
                } else {
                    self.emit(Opcode::InitializeBinding, slot)?;
                }
            }
            Pattern::Array(elements) => {
                self.emit(Opcode::GetIterator, 0)?;
                for element in elements {
                    let Some(element) = element else {
                        self.emit(Opcode::IteratorElision, 0)?;
                        continue;
                    };
                    if element.rest {
                        self.emit(Opcode::IteratorRest, 0)?;
                        self.bind_pattern(&element.pattern, kind)?;
                        return Ok(());
                    }
                    self.array_pattern_value()?;
                    self.binding_pattern_default(element.default.as_ref(), &element.pattern)?;
                    self.bind_pattern(&element.pattern, kind)?;
                }
                self.emit(Opcode::IteratorFinish, 0)?;
            }
            Pattern::Object(properties) => {
                // Even an empty object pattern performs RequireObjectCoercible.
                self.emit(Opcode::RequireObject, 0)?;
                self.emit(Opcode::NewArray, 0)?;
                for property in properties {
                    match property {
                        ObjectPatternProp::KeyValue { key, value, default } => {
                            self.property_key(key)?;
                            self.emit(Opcode::DestructureProperty, 0)?;
                            self.binding_pattern_default(default.as_ref(), value)?;
                            self.bind_pattern(value, kind)?;
                        }
                        ObjectPatternProp::Rest(pattern) => {
                            self.emit(Opcode::ObjectRest, 0)?;
                            self.bind_pattern(pattern, kind)?;
                            return Ok(());
                        }
                    }
                }
                self.emit(Opcode::Pop, 0)?;
                self.emit(Opcode::Pop, 0)?;
            }
        }
        Ok(())
    }

    /// Leaves the array-pattern iterator record below one element value.  A
    /// record remembers exhaustion in the VM, so later elisions do not call
    /// `next` again after the first completed result.
    fn array_pattern_value(&mut self) -> Result<(), CompileError> {
        self.emit(Opcode::Dup, 0)?;
        let exhausted = self.emit(Opcode::IteratorStep, 0)?;
        let joined = self.emit(Opcode::Jump, 0)?;
        self.patch(exhausted, self.offset()?);
        self.constant(Value::Undefined)?;
        self.patch(joined, self.offset()?);
        Ok(())
    }

    /// Replaces an `undefined` binding value with a pattern/parameter
    /// initializer.  `null` remains a value, as required by ECMA-262.
    fn pattern_default(&mut self, default: Option<&Expr>) -> Result<(), CompileError> {
        let Some(default) = default else { return Ok(()) };
        self.emit(Opcode::Dup, 0)?;
        self.constant(Value::Undefined)?;
        self.emit(Opcode::StrictEqual, 0)?;
        let skip = self.emit(Opcode::JumpIfFalse, 0)?;
        self.emit(Opcode::Pop, 0)?;
        self.expression(default)?;
        self.patch(skip, self.offset()?);
        Ok(())
    }

    fn binding_pattern_default(&mut self, default: Option<&Expr>, pattern: &Pattern) -> Result<(), CompileError> {
        let Some(default) = default else { return Ok(()) };
        self.emit(Opcode::Dup, 0)?;
        self.constant(Value::Undefined)?;
        self.emit(Opcode::StrictEqual, 0)?;
        let skip = self.emit(Opcode::JumpIfFalse, 0)?;
        self.emit(Opcode::Pop, 0)?;
        self.expression_with_name(default, match pattern { Pattern::Identifier(name) => Some(name.as_str()), _ => None })?;
        self.patch(skip, self.offset()?);
        Ok(())
    }

    fn expression_with_name(&mut self, expression: &Expr, inferred_name: Option<&str>) -> Result<(), CompileError> {
        match expression {
            Expr::Function(function) if function.name.is_none() && inferred_name.is_some() => self.function_named(function, false, inferred_name, false),
            Expr::Class(class) if class.name.is_none() && inferred_name.is_some() => self.class_expression(class, inferred_name),
            Expr::Arrow { params, body } if inferred_name.is_some() => {
                let body = match body {
                    ArrowBody::Expr(expr) => vec![Stmt::Return(Some(*expr.clone()))],
                    ArrowBody::Block(body) => body.clone(),
                };
                self.function_named(&Function { name: None, params: params.clone(), body, generator: false, is_async: false }, true, inferred_name, false)
            }
            _ => self.expression(expression),
        }
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
        self.loops.push(Loop { scope_depth: self.scopes.len(), breaks: Vec::new(), continues: Some(Vec::new()), iterator: None });
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
        for (jump, control) in context.breaks {
            self.patch(jump, end);
            self.bytecode.abrupt_jumps[control].target = end;
        }
        for (jump, control) in context.continues.expect("loop has continue targets") {
            self.patch(jump, continue_at);
            self.bytecode.abrupt_jumps[control].target = continue_at;
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
                } else if self.with_depth != 0 {
                    let index = u32::try_from(self.bytecode.constants.len()).map_err(|_| CompileError::ProgramTooLarge)?;
                    self.bytecode.constants.push(Value::String(name.clone().into()));
                    self.emit(Opcode::WithGet, index)?;
                } else {
                    match name.as_str() {
                        "undefined" => self.constant(Value::Undefined)?,
                        "NaN" => self.constant(Value::Number(f64::NAN))?,
                        "Infinity" => self.constant(Value::Number(f64::INFINITY))?,
                        "String" => {
                            self.emit(Opcode::GlobalString, 0)?;
                        }
                        "Symbol" | "RegExp" | "Object" | "Reflect" | "Math" | "Number" | "Boolean" | "Array" | "Function" | "globalThis" | "Intl" | "Error" | "TypeError" | "eval"
                        | "isNaN" | "isFinite" | "parseInt" | "parseFloat" | "JSON" | "RangeError" | "SyntaxError" | "ReferenceError" | "EvalError" | "URIError" => {
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
                if *op == UnaryOp::Void {
                    self.expression(arg)?;
                    self.emit(Opcode::Pop, 0)?;
                    self.constant(Value::Undefined)?;
                    return Ok(());
                }
                let opcode = match op {
                    UnaryOp::Neg => Opcode::Negate,
                    UnaryOp::Plus => Opcode::ToNumber,
                    UnaryOp::Not => Opcode::Not,
                    UnaryOp::Typeof => Opcode::Typeof,
                    UnaryOp::Delete | UnaryOp::Void => Opcode::DeleteProperty,
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
                    && matches!(&**arg, Expr::Identifier(name) if self.resolve(name).is_none() && !matches!(name.as_str(), "undefined" | "NaN" | "Infinity" | "String" | "Symbol" | "RegExp" | "Object" | "Reflect" | "Math" | "Number" | "Boolean" | "Array" | "Function" | "globalThis" | "Intl" | "Error" | "TypeError" | "RangeError" | "SyntaxError" | "ReferenceError" | "EvalError" | "URIError" | "isNaN" | "isFinite" | "parseInt" | "parseFloat" | "JSON"))
                {
                    let Expr::Identifier(name) = &**arg else { unreachable!() };
                    let index = u32::try_from(self.bytecode.constants.len()).map_err(|_| CompileError::ProgramTooLarge)?;
                    self.bytecode.constants.push(Value::String(name.as_str().into()));
                    self.emit(Opcode::TypeofName, index)?;
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
            Expr::Sequence(expressions) => {
                for (index, expression) in expressions.iter().enumerate() {
                    self.expression(expression)?;
                    if index + 1 != expressions.len() {
                        self.emit(Opcode::Pop, 0)?;
                    }
                }
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
                    if let ObjectProp::Spread(value) = property {
                        self.expression(value)?;
                        self.emit(Opcode::CopyDataProperties, 0)?;
                        continue;
                    }
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
                    let ObjectProp::KeyValue { key, value, shorthand } = property else { unreachable!("spread is handled above") };
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
            Expr::Super => return Err(CompileError::InvalidSyntax("super must be used as a property access or constructor call")),
            Expr::Member { object, property, computed } if matches!(&**object, Expr::Super) => {
                self.super_property_key(property, *computed)?;
                self.emit(Opcode::SuperGet, 0)?;
            }
            Expr::Member { .. } => {
                self.member_reference(expr)?;
                self.emit(Opcode::GetProperty, 0)?;
            }
            Expr::Assign { op, target, value } => self.assignment(*op, target, value)?,
            Expr::DestructureAssign { pattern, value } => self.destructuring_assignment(pattern, value)?,
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
                } else if let Expr::Member { object, property, computed } = arg.as_ref() {
                    if matches!(&**object, Expr::Super) {
                        self.super_property_key(property, *computed)?;
                        self.emit(Opcode::SuperUpdate, u32::from(*op == UpdateOp::Dec) | (u32::from(*prefix) << 1))?;
                    } else {
                        self.member_reference(arg)?;
                        self.emit(Opcode::UpdateProperty, u32::from(*op == UpdateOp::Dec) | (u32::from(*prefix) << 1))?;
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
                if !construct && matches!(&**callee, Expr::Super) {
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
                        self.emit(Opcode::SuperCallSpread, 0)?;
                    } else {
                        for arg in args {
                            let Argument::Normal(expr) = arg else { unreachable!("super call spreads take the array path") };
                            self.expression(expr)?;
                        }
                        self.emit(Opcode::SuperCall, u32::try_from(args.len()).map_err(|_| CompileError::ProgramTooLarge)?)?;
                    }
                    return Ok(());
                }
                if !construct && matches!(&**callee, Expr::Member { object, .. } if matches!(&**object, Expr::Super)) {
                    let Expr::Member { property, computed, .. } = callee.as_ref() else { unreachable!() };
                    self.super_property_key(property, *computed)?;
                    self.emit(Opcode::SuperGetMethod, 0)?;
                } else if !construct && matches!(&**callee, Expr::Member { .. }) {
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
            Expr::Function(function) => self.function_expression(function)?,
            Expr::Class(class) => self.class_expression(class, None)?,
            Expr::Yield(value) => {
                if !self.bytecode.generator {
                    return Err(CompileError::InvalidSyntax("yield requires a generator function"));
                }
                if let Some(value) = value {
                    self.expression(value)?;
                } else {
                    self.constant(Value::Undefined)?;
                }
                self.emit(Opcode::Yield, 0)?;
            }
            Expr::Arrow { params, body } => {
                let body = match body {
                    ArrowBody::Expr(expr) => vec![Stmt::Return(Some(*expr.clone()))],
                    ArrowBody::Block(body) => body.clone(),
                };
                self.function(&Function { name: None, params: params.clone(), body, generator: false, is_async: false }, true)?;
            }
        }
        Ok(())
    }

    fn for_in(&mut self, left: &ForHead, right: &Expr, body: &Stmt) -> Result<(), CompileError> {
        self.for_each(left, right, body, true)
    }

    fn for_of(&mut self, left: &ForHead, right: &Expr, body: &Stmt) -> Result<(), CompileError> {
        self.for_each(left, right, body, false)
    }

    fn for_each(&mut self, left: &ForHead, right: &Expr, body: &Stmt, for_in: bool) -> Result<(), CompileError> {
        self.emit(Opcode::ClearCompletion, 0)?;
        let (pattern, kind) = match left {
            ForHead::Decl(kind, pattern) => (pattern, Some(*kind)),
            ForHead::Pattern(pattern) => (pattern, None),
        };
        let lexical = kind.is_some_and(|kind| kind != DeclKind::Var);
        let mut declarations = vec![("*iterator*".to_owned(), DeclKind::Let)];
        if lexical {
            declarations.extend(pattern_names(pattern).into_iter().map(|name| (name, kind.unwrap())));
        }
        self.enter_scope(declarations, &BTreeSet::new(), false)?;
        let iterator = self.resolve("*iterator*").unwrap();
        self.expression(right)?;
        if for_in {
            self.emit(Opcode::ForInKeys, 0)?;
        }
        self.emit(Opcode::GetIterator, 0)?;
        self.emit(Opcode::InitializeBinding, iterator)?;
        let start = self.offset()?;
        self.emit(Opcode::GetBinding, iterator)?;
        let exit = self.emit(Opcode::IteratorStep, 0)?;
        self.loops.push(Loop { scope_depth: self.scopes.len(), breaks: Vec::new(), continues: Some(Vec::new()), iterator: Some(iterator) });
        if lexical {
            self.enter_scope(pattern_names(pattern).into_iter().map(|name| (name, kind.unwrap())).collect(), &BTreeSet::new(), false)?;
        }
        match kind {
            Some(kind) => self.bind_pattern(pattern, kind)?,
            None => {
                let Pattern::Identifier(name) = pattern else { return Err(CompileError::Unsupported(if for_in { "a destructuring for-in assignment target" } else { "a destructuring for-of assignment target" })) };
                let slot = self.resolve(name).ok_or(CompileError::Unsupported("implicit global assignment"))?;
                self.emit(Opcode::StoreBinding, slot)?;
                self.emit(Opcode::Pop, 0)?;
            }
        }
        self.statement(body, false)?;
        if lexical {
            self.leave_scope()?;
        }
        self.emit(Opcode::Jump, start)?;
        let end = self.offset()?;
        self.patch(exit, end);
        let context = self.loops.pop().unwrap();
        for (jump, control) in context.breaks {
            self.patch(jump, end);
            self.bytecode.abrupt_jumps[control].target = end;
        }
        for (jump, control) in context.continues.expect("for-of loop has continue targets") {
            self.patch(jump, start);
            self.bytecode.abrupt_jumps[control].target = start;
        }
        self.leave_scope()?;
        Ok(())
    }

    fn assignment(&mut self, op: AssignOp, target: &Expr, value: &Expr) -> Result<(), CompileError> {
        if let Expr::Member { object, property, computed } = target {
            if matches!(&**object, Expr::Super) {
                self.super_property_key(property, *computed)?;
                if op != AssignOp::Assign {
                    self.emit(Opcode::Dup, 0)?;
                    self.emit(Opcode::SuperGet, 0)?;
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
                self.emit(Opcode::SuperSet, 0)?;
                return Ok(());
            }
        }
        if let Expr::Identifier(name) = target {
            if self.resolve(name).is_none() && self.with_depth != 0 {
                if op != AssignOp::Assign {
                    return Err(CompileError::Unsupported("compound assignment in a with statement"));
                }
                self.expression(value)?;
                let index = u32::try_from(self.bytecode.constants.len()).map_err(|_| CompileError::ProgramTooLarge)?;
                self.bytecode.constants.push(Value::String(name.clone().into()));
                self.emit(Opcode::WithSet, index)?;
                return Ok(());
            }
        }
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

    /// AssignmentPatternEvaluation. The first copy of the RHS is the
    /// expression's result; the second is consumed by the recursive pattern.
    fn destructuring_assignment(&mut self, pattern: &AssignmentPattern, value: &Expr) -> Result<(), CompileError> {
        self.expression(value)?;
        self.emit(Opcode::Dup, 0)?;
        self.assign_pattern(pattern)
    }

    fn assign_pattern(&mut self, pattern: &AssignmentPattern) -> Result<(), CompileError> {
        match pattern {
            AssignmentPattern::Target(target) => self.assign_pattern_target(target)?,
            AssignmentPattern::Array(elements) => {
                self.emit(Opcode::GetIterator, 0)?;
                for element in elements {
                    let Some(element) = element else {
                        self.emit(Opcode::IteratorElision, 0)?;
                        continue;
                    };
                    if element.rest {
                        self.emit(Opcode::IteratorRest, 0)?;
                        self.assign_pattern(&element.pattern)?;
                        return Ok(());
                    }
                    self.array_pattern_value()?;
                    self.pattern_default(element.default.as_ref())?;
                    self.assign_pattern(&element.pattern)?;
                }
                self.emit(Opcode::IteratorFinish, 0)?;
            }
            AssignmentPattern::Object(properties) => {
                self.emit(Opcode::RequireObject, 0)?;
                self.emit(Opcode::NewArray, 0)?;
                for property in properties {
                    match property {
                        AssignmentPatternProp::KeyValue { key, value, default } => {
                            self.property_key(key)?;
                            self.emit(Opcode::DestructureProperty, 0)?;
                            self.pattern_default(default.as_ref())?;
                            self.assign_pattern(value)?;
                        }
                        AssignmentPatternProp::Rest(pattern) => {
                            self.emit(Opcode::ObjectRest, 0)?;
                            self.assign_pattern(pattern)?;
                            return Ok(());
                        }
                    }
                }
                self.emit(Opcode::Pop, 0)?;
                self.emit(Opcode::Pop, 0)?;
            }
        }
        Ok(())
    }

    /// Consumes a leaf value while assigning an existing binding or member;
    /// the outer assignment pattern keeps its duplicate RHS beneath it.
    fn assign_pattern_target(&mut self, target: &Expr) -> Result<(), CompileError> {
        if let Expr::Identifier(name) = target {
            let slot = self.resolve(name).ok_or(CompileError::Unsupported("implicit global assignment"))?;
            self.emit(Opcode::StoreBinding, slot)?;
        } else {
            self.member_reference(target)?;
            self.emit(Opcode::SetDestructureProperty, 0)?;
        }
        self.emit(Opcode::Pop, 0)?;
        Ok(())
    }

    fn member_reference(&mut self, target: &Expr) -> Result<(), CompileError> {
        let Expr::Member { object, property, computed } = target else { return Err(CompileError::InvalidSyntax("invalid assignment/member AST")) };
        if matches!(&**object, Expr::Super) {
            return Err(CompileError::InvalidSyntax("super member requires a dedicated operation"));
        }
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

    fn super_property_key(&mut self, property: &Expr, computed: bool) -> Result<(), CompileError> {
        if computed {
            self.expression(property)?;
        } else if let Expr::Identifier(name) = property {
            self.constant(Value::String(name.clone().into()))?;
        } else {
            return Err(CompileError::InvalidSyntax("invalid non-computed super member AST"));
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
        self.function_named(function, arrow, None, false)
    }

    fn function_expression(&mut self, function: &Function) -> Result<(), CompileError> {
        self.function_named(function, false, None, function.name.is_some())
    }

    fn class_expression(&mut self, class: &Class, inferred_name: Option<&str>) -> Result<(), CompileError> {
        let Some(name) = class.name.as_ref() else {
            return self.class_expression_with_binding(class, inferred_name, None);
        };
        self.enter_scope(vec![(name.clone(), DeclKind::Const)], &BTreeSet::new(), true)?;
        let binding = self.resolve(name).expect("class name was entered into its expression scope");
        let result = self.class_expression_with_binding(class, inferred_name, Some(binding));
        self.leave_scope()?;
        result
    }

    fn class_expression_with_binding(&mut self, class: &Class, inferred_name: Option<&str>, binding: Option<u32>) -> Result<(), CompileError> {
        let constructor = class.elements.iter().find_map(|element| match element {
            ClassElement::Method { key, function, is_static: false }
                if !matches!(key, PropertyKey::Computed(_)) && class_property_name(key) == "constructor" => Some(function.clone()),
            _ => None,
        });
        let default_constructor = constructor.is_none();
        let mut constructor = constructor.unwrap_or(Function { name: class.name.clone(), params: Vec::new(), body: Vec::new(), generator: false, is_async: false });
        constructor.name = class.name.clone();
        let fields: Vec<_> = class
            .elements
            .iter()
            .filter_map(|element| match element {
                ClassElement::Field { key, initializer, is_static: false } => Some(class_instance_field(key, initializer.as_ref())),
                _ => None,
            })
            .collect();
        let constructor_body = std::mem::take(&mut constructor.body);
        let body = if class.extends.is_some() {
            if default_constructor { fields.clone() } else { derived_constructor_body(constructor_body, fields.clone())? }
        } else {
            let mut body = fields.clone();
            body.extend(constructor_body);
            body
        };
        constructor.body = body;
        self.function_named_with(
            &constructor,
            false,
            inferred_name,
            false,
            FunctionCompileOptions {
                constructible: true,
                force_strict: true,
                class_constructor: true,
                derived_constructor: class.extends.is_some(),
                default_derived_constructor: class.extends.is_some() && default_constructor,
            },
        )?;
        if let Some(base) = &class.extends {
            self.expression(base)?;
            self.emit(Opcode::SetClassHeritage, 0)?;
        }
        if let Some(slot) = binding {
            self.emit(Opcode::Dup, 0)?;
            self.emit(Opcode::InitializeBinding, slot)?;
        }
        for element in &class.elements {
            match element {
                ClassElement::Method { key, function, is_static } => {
                    if !is_static && !matches!(key, PropertyKey::Computed(_)) && class_property_name(key) == "constructor" {
                        continue;
                    }
                    self.class_property_target(*is_static)?;
                    self.property_key(key)?;
                    self.function_named_with(
                        function,
                        false,
                        None,
                        false,
                        FunctionCompileOptions::class_method(),
                    )?;
                    self.emit(Opcode::DefineMethod, 0)?;
                    self.emit(Opcode::Pop, 0)?;
                }
                ClassElement::Accessor { key, function, getter, is_static } => {
                    self.class_property_target(*is_static)?;
                    self.property_key(key)?;
                    self.function_named_with(
                        function,
                        false,
                        None,
                        false,
                        FunctionCompileOptions::class_method(),
                    )?;
                    self.emit(Opcode::DefineClassAccessor, u32::from(!getter))?;
                    self.emit(Opcode::Pop, 0)?;
                }
                ClassElement::Field { key, initializer, is_static: true } => {
                    self.class_property_target(true)?;
                    self.property_key(key)?;
                    let value = initializer.clone().unwrap_or_else(undefined_expression);
                    let initializer = Function { name: None, params: Vec::new(), body: vec![Stmt::Return(Some(value))], generator: false, is_async: false };
                    self.function_named_with(
                        &initializer,
                        false,
                        None,
                        false,
                        FunctionCompileOptions::class_method(),
                    )?;
                    self.emit(Opcode::DefineClassStaticField, 0)?;
                }
                ClassElement::Field { is_static: false, .. } => {}
                ClassElement::StaticBlock(body) => {
                    let block = Function { name: None, params: Vec::new(), body: body.clone(), generator: false, is_async: false };
                    self.function_named_with(
                        &block,
                        false,
                        None,
                        false,
                        FunctionCompileOptions::class_method(),
                    )?;
                    self.emit(Opcode::CallClassStaticBlock, 0)?;
                }
            }
        }
        Ok(())
    }

    fn class_property_target(&mut self, is_static: bool) -> Result<(), CompileError> {
        self.emit(Opcode::Dup, 0)?;
        if !is_static {
            self.constant(Value::String("prototype".into()))?;
            self.emit(Opcode::GetProperty, 0)?;
        }
        Ok(())
    }

    fn function_named(&mut self, function: &Function, arrow: bool, inferred_name: Option<&str>, named_expression: bool) -> Result<(), CompileError> {
        self.function_named_with(
            function,
            arrow,
            inferred_name,
            named_expression,
            FunctionCompileOptions {
                constructible: !arrow && !function.generator,
                force_strict: false,
                class_constructor: false,
                derived_constructor: false,
                default_derived_constructor: false,
            },
        )
    }

    fn function_named_with(
        &mut self,
        function: &Function,
        arrow: bool,
        inferred_name: Option<&str>,
        named_expression: bool,
        options: FunctionCompileOptions,
    ) -> Result<(), CompileError> {
        if function.is_async {
            return Err(CompileError::Unsupported("async functions"));
        }
        let child_budget = self.max_bytecode_bytes.saturating_sub(self.offset()?);
        let mut child = Compiler {
            bytecode: Bytecode::empty(),
            names: vec![HashMap::new()],
            scopes: Vec::new(),
            loops: Vec::new(),
            catch_var_slots: Vec::new(),
            max_bytecode_bytes: child_budget,
            function: true,
            local_scope: 1,
            with_depth: 0,
        };
        child.bytecode.strict = options.force_strict || self.bytecode.strict || strict_body(&function.body);
        child.bytecode.arrow = arrow;
        child.bytecode.generator = function.generator;
        child.bytecode.constructible = options.constructible;
        child.bytecode.class_constructor = options.class_constructor;
        child.bytecode.derived_constructor = options.derived_constructor;
        child.bytecode.function_name = function.name.clone().or_else(|| inferred_name.map(str::to_owned)).unwrap_or_default();
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
        if named_expression {
            let name = function.name.as_ref().expect("named function expression has a name").clone();
            let slot = u32::try_from(child.bytecode.bindings.len()).map_err(|_| CompileError::ProgramTooLarge)?;
            child.names[0].insert(name.clone(), slot);
            child.bytecode.bindings.push(Binding { name, mutable: false, lexical: true });
            child.bytecode.self_slot = Some(slot);
        }
        let mut vars = var_names(&function.body)?;
        for param in &function.params {
            vars.extend(pattern_names(&param.pattern));
        }
        child.enter_scope(lexical_names(&function.body)?, &vars, true)?;
        for (index, param) in function.params.iter().enumerate() {
            child.emit(if param.rest { Opcode::RestArguments } else { Opcode::Argument }, index as u32)?;
            child.binding_pattern_default(param.default.as_ref(), &param.pattern)?;
            child.bind_pattern(&param.pattern, DeclKind::Let)?;
        }
        if options.default_derived_constructor {
            child.emit(Opcode::SuperCallForward, 0)?;
            child.emit(Opcode::Pop, 0)?;
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

    fn self_tail_call_args<'a>(&self, value: &'a Expr) -> Option<&'a [Argument]> {
        let Expr::Call { callee, args } = value else { return None };
        let Expr::Identifier(name) = callee.as_ref() else { return None };
        let slot = self.bytecode.self_slot?;
        (self.bytecode.strict && self.resolve(name) == Some(slot) && args.iter().all(|argument| matches!(argument, Argument::Normal(_)))).then_some(args)
    }
}

fn strict_body(body: &[Stmt]) -> bool {
    body.iter().take_while(|stmt| matches!(stmt, Stmt::Expr(Expr::String(_)))).any(|stmt| matches!(stmt, Stmt::Expr(Expr::String(s)) if s == "use strict"))
}

fn class_property_name(key: &PropertyKey) -> String {
    match key {
        PropertyKey::Identifier(name) => name.clone(),
        PropertyKey::String(name) => name.to_utf8().unwrap_or_default(),
        PropertyKey::Number(number) => number.to_string(),
        PropertyKey::Computed(_) => String::new(),
    }
}

fn undefined_expression() -> Expr {
    Expr::Unary { op: UnaryOp::Void, arg: Box::new(Expr::Number(0.0)) }
}

fn class_instance_field(key: &PropertyKey, initializer: Option<&Expr>) -> Stmt {
    let (property, computed) = match key {
        PropertyKey::Identifier(name) => (Expr::Identifier(name.clone()), false),
        PropertyKey::String(name) => (Expr::String(name.clone()), true),
        PropertyKey::Number(number) => (Expr::Number(*number), true),
        PropertyKey::Computed(expression) => ((*expression.clone()), true),
    };
    Stmt::Expr(Expr::Assign {
        op: AssignOp::Assign,
        target: Box::new(Expr::Member { object: Box::new(Expr::This), property: Box::new(property), computed }),
        value: Box::new(initializer.cloned().unwrap_or_else(undefined_expression)),
    })
}

/// The VM establishes `this` while executing `super()`. For explicit derived
/// constructors, fields therefore follow the first direct constructor call.
/// More complex control flow needs a dedicated derived-this state machine;
/// report it as unsupported instead of initializing fields at an incorrect
/// point.
fn derived_constructor_body(mut body: Vec<Stmt>, fields: Vec<Stmt>) -> Result<Vec<Stmt>, CompileError> {
    if fields.is_empty() {
        return Ok(body);
    }
    let Some(index) = body.iter().position(|statement| matches!(statement, Stmt::Expr(Expr::Call { callee, .. }) if matches!(&**callee, Expr::Super))) else {
        return Err(CompileError::Unsupported("instance fields in an explicit derived constructor without a direct super() call"));
    };
    body.splice(index + 1..index + 1, fields);
    Ok(body)
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
        BinaryOp::Eq => Opcode::Equal,
        BinaryOp::NotEq => Opcode::NotEqual,
        BinaryOp::Lt => Opcode::Less,
        BinaryOp::Gt => Opcode::Greater,
        BinaryOp::LtEq => Opcode::LessEqual,
        BinaryOp::GtEq => Opcode::GreaterEqual,
        BinaryOp::Instanceof => Opcode::Instanceof,
        BinaryOp::In => Opcode::In,
    })
}

fn pattern_names(pattern: &Pattern) -> Vec<String> {
    match pattern {
        Pattern::Identifier(name) => vec![name.clone()],
        Pattern::Array(elements) => elements.iter().flatten().flat_map(|element| pattern_names(&element.pattern)).collect(),
        Pattern::Object(properties) => properties
            .iter()
            .flat_map(|property| match property {
                ObjectPatternProp::KeyValue { value, .. } | ObjectPatternProp::Rest(value) => pattern_names(value),
            })
            .collect(),
    }
}

fn declarations_names(kind: DeclKind, declarations: &[VarDeclarator]) -> Result<Vec<(String, DeclKind)>, CompileError> {
    Ok(declarations.iter().flat_map(|decl| pattern_names(&decl.pattern).into_iter().map(move |name| (name, kind))).collect())
}

fn lexical_names(statements: &[Stmt]) -> Result<Vec<(String, DeclKind)>, CompileError> {
    let mut names = Vec::new();
    for statement in statements {
        if let Stmt::VarDecl(kind, declarations) = statement {
            if *kind != DeclKind::Var {
                names.extend(declarations_names(*kind, declarations)?);
            }
        }
        if let Stmt::ClassDecl(class) = statement {
            names.push((class.name.clone().expect("class declaration has a name"), DeclKind::Const));
        }
    }
    Ok(names)
}

/// `CatchParameter` has an additional early error against lexical names in
/// its directly nested block. Function declarations participate even though
/// their broader binding behavior is handled separately for Annex B.
fn catch_lexical_names(statements: &[Stmt]) -> Vec<String> {
    let mut names = Vec::new();
    for statement in statements {
        match statement {
            Stmt::VarDecl(kind, declarations) if *kind != DeclKind::Var => {
                names.extend(declarations.iter().flat_map(|declaration| pattern_names(&declaration.pattern)));
            }
            Stmt::FunctionDecl(function) => names.push(function.name.clone().expect("declaration has a name")),
            Stmt::ClassDecl(class) => names.push(class.name.clone().expect("class declaration has a name")),
            _ => {}
        }
    }
    names
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
                    names.extend(pattern_names(&declaration.pattern));
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
                        names.extend(pattern_names(&declaration.pattern));
                    }
                }
                pending.push(body);
            }
            Stmt::ForIn { left, body, .. } | Stmt::ForOf { left, body, .. } => {
                if let ForHead::Decl(DeclKind::Var, pattern) = left {
                    names.extend(pattern_names(pattern));
                }
                pending.push(body);
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    pending.extend(&case.consequent);
                }
            }
            Stmt::Try { block, handler, finalizer } => {
                pending.extend(block);
                if let Some(handler) = handler {
                    pending.extend(&handler.body);
                }
                if let Some(finalizer) = finalizer {
                    pending.extend(finalizer);
                }
            }
            _ => {}
        }
    }
    Ok(names)
}
