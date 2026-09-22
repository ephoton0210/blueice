// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! AST-to-bytecode lowering.
//!
//! This module is deliberately the sole consumer of [`crate::Program`] at
//! runtime. The VM receives [`crate::BytecodeModule`], not AST nodes, so
//! execution stays a fixed-width bytecode stack machine as planned.

use crate::ast::*;
use crate::bytecode::*;
use crate::value::Value;
use std::fmt;

/// A malformed AST that cannot be produced by BlueJS's parser, but can be
/// supplied by an embedding caller constructing AST nodes manually.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileError {
    pub message: String,
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CompileError {}

/// Lowers a parser AST into a self-contained bytecode module. Function zero
/// is the script entry point and always ends in [`Opcode::Halt`].
pub fn compile(program: &Program) -> Result<BytecodeModule, CompileError> {
    let mut compiler = Compiler::new();
    for statement in &program.body {
        compiler.compile_statement(statement)?;
    }
    compiler.emit(Opcode::Halt, 0);
    Ok(compiler.module)
}

#[derive(Debug, Default)]
struct LoopContext {
    breaks: Vec<usize>,
    continues: Vec<usize>,
    allows_continue: bool,
}

impl LoopContext {
    fn loop_body() -> Self {
        Self {
            breaks: Vec::new(),
            continues: Vec::new(),
            allows_continue: true,
        }
    }
}

struct Compiler {
    module: BytecodeModule,
    current_function: usize,
    loops: Vec<LoopContext>,
}

impl Compiler {
    fn new() -> Self {
        Self {
            module: BytecodeModule {
                constants: Vec::new(),
                functions: vec![BytecodeFunction {
                    name: Some("<script>".to_string()),
                    parameters: Vec::new(),
                    code: Vec::new(),
                    is_arrow: false,
                }],
                patterns: Vec::new(),
            },
            current_function: 0,
            loops: Vec::new(),
        }
    }

    fn code(&self) -> &Vec<Instruction> {
        &self.module.functions[self.current_function].code
    }

    fn code_mut(&mut self) -> &mut Vec<Instruction> {
        &mut self.module.functions[self.current_function].code
    }

    fn position(&self) -> usize {
        self.code().len()
    }

    fn emit(&mut self, opcode: Opcode, operand: u64) -> usize {
        let position = self.position();
        self.code_mut().push(Instruction::new(opcode, operand));
        position
    }

    fn emit_jump(&mut self, opcode: Opcode) -> usize {
        self.emit(opcode, 0)
    }

    fn patch_jump(&mut self, position: usize, target: usize) {
        let hint = self.code()[position].tiering_hint();
        self.code_mut()[position] =
            Instruction::new(self.code()[position].opcode(), target as u64).with_tiering_hint(hint);
    }

    fn constant_value(&mut self, value: Value) -> u64 {
        let index = self.module.constants.len() as u64;
        self.module.constants.push(Constant::Value(value));
        index
    }

    fn constant_string(&mut self, value: impl Into<String>) -> u64 {
        let index = self.module.constants.len() as u64;
        self.module.constants.push(Constant::String(value.into()));
        index
    }

    fn emit_name(&mut self, opcode: Opcode, name: &str) {
        let index = self.constant_string(name);
        self.emit(opcode, index);
    }

    fn compile_statement(&mut self, statement: &Stmt) -> Result<(), CompileError> {
        match statement {
            Stmt::Empty => {
                self.emit(Opcode::Nop, 0);
            }
            Stmt::Expr(expression) => {
                self.compile_expression(expression)?;
                self.emit(Opcode::SetCompletion, 0);
            }
            Stmt::Block(statements) => {
                self.emit(Opcode::EnterScope, 0);
                for statement in statements {
                    self.compile_statement(statement)?;
                }
                self.emit(Opcode::LeaveScope, 0);
            }
            Stmt::VarDecl(kind, declarations) => {
                for declaration in declarations {
                    self.compile_declarator(*kind, declaration)?;
                }
            }
            Stmt::If {
                test,
                consequent,
                alternate,
            } => {
                self.compile_expression(test)?;
                let false_jump = self.emit_jump(Opcode::JumpIfFalse);
                self.emit(Opcode::Pop, 0);
                self.compile_statement(consequent)?;
                let end_jump = self.emit_jump(Opcode::Jump);
                self.patch_jump(false_jump, self.position());
                self.emit(Opcode::Pop, 0);
                if let Some(alternate) = alternate {
                    self.compile_statement(alternate)?;
                }
                self.patch_jump(end_jump, self.position());
            }
            Stmt::For {
                init,
                test,
                update,
                body,
            } => self.compile_for(init.as_ref(), test.as_ref(), update.as_ref(), body)?,
            Stmt::ForIn { left, right, body } => self.compile_for_each(left, right, body, false)?,
            Stmt::ForOf { left, right, body } => self.compile_for_each(left, right, body, true)?,
            Stmt::While { test, body } => self.compile_while(test, body)?,
            Stmt::DoWhile { body, test } => self.compile_do_while(body, test)?,
            Stmt::Switch {
                discriminant,
                cases,
            } => self.compile_switch(discriminant, cases)?,
            Stmt::Break => {
                if self.loops.is_empty() {
                    return Err(CompileError {
                        message: "break outside a loop or switch".to_string(),
                    });
                }
                let jump = self.emit_jump(Opcode::Jump);
                let context = self
                    .loops
                    .last_mut()
                    .expect("loop context was checked above");
                context.breaks.push(jump);
            }
            Stmt::Continue => {
                let Some(context_index) = self
                    .loops
                    .iter()
                    .rposition(|context| context.allows_continue)
                else {
                    return Err(CompileError {
                        message: "continue outside a loop".to_string(),
                    });
                };
                let jump = self.emit_jump(Opcode::Jump);
                self.loops[context_index].continues.push(jump);
            }
            Stmt::Return(value) => {
                if let Some(value) = value {
                    self.compile_expression(value)?;
                } else {
                    self.emit(Opcode::LoadUndefined, 0);
                }
                self.emit(Opcode::Return, 0);
            }
            Stmt::Throw(value) => {
                self.compile_expression(value)?;
                self.emit(Opcode::Throw, 0);
            }
            Stmt::Try {
                block,
                handler,
                finalizer,
            } => self.compile_try(block, handler.as_ref(), finalizer.as_deref())?,
            Stmt::FunctionDecl(function) => {
                let Some(name) = &function.name else {
                    return Err(CompileError {
                        message: "function declarations require a name".to_string(),
                    });
                };
                let function_id = self.compile_function(function, false)?;
                self.emit(Opcode::MakeFunction, function_id as u64);
                self.emit_name(Opcode::DeclareVar, name);
            }
        }
        Ok(())
    }

    fn compile_declarator(
        &mut self,
        kind: DeclKind,
        declaration: &VarDeclarator,
    ) -> Result<(), CompileError> {
        if let Some(initializer) = &declaration.init {
            self.compile_expression(initializer)?;
        } else {
            self.emit(Opcode::LoadUndefined, 0);
        }
        match &declaration.pattern {
            Pattern::Identifier(name) => self.emit_name(declaration_opcode(kind), name),
            pattern => {
                let index = self.register_pattern(pattern)?;
                self.emit(declaration_pattern_opcode(kind), index as u64);
            }
        };
        Ok(())
    }

    fn compile_while(&mut self, test: &Expr, body: &Stmt) -> Result<(), CompileError> {
        let start = self.position();
        self.compile_expression(test)?;
        let exit = self.emit_jump(Opcode::JumpIfFalse);
        self.emit(Opcode::Pop, 0);
        self.loops.push(LoopContext::loop_body());
        self.compile_statement(body)?;
        let context = self.loops.pop().expect("just pushed a loop context");
        for jump in context.continues {
            self.patch_jump(jump, start);
        }
        self.emit(Opcode::Jump, start as u64);
        let end = self.position();
        self.patch_jump(exit, end);
        self.emit(Opcode::Pop, 0);
        for jump in context.breaks {
            self.patch_jump(jump, self.position());
        }
        Ok(())
    }

    fn compile_do_while(&mut self, body: &Stmt, test: &Expr) -> Result<(), CompileError> {
        let start = self.position();
        self.loops.push(LoopContext::loop_body());
        self.compile_statement(body)?;
        let test_start = self.position();
        let context = self.loops.pop().expect("just pushed a loop context");
        for jump in context.continues {
            self.patch_jump(jump, test_start);
        }
        self.compile_expression(test)?;
        let exit = self.emit_jump(Opcode::JumpIfFalse);
        self.emit(Opcode::Pop, 0);
        self.emit(Opcode::Jump, start as u64);
        self.patch_jump(exit, self.position());
        self.emit(Opcode::Pop, 0);
        let end = self.position();
        for jump in context.breaks {
            self.patch_jump(jump, end);
        }
        Ok(())
    }

    fn compile_for(
        &mut self,
        init: Option<&ForInit>,
        test: Option<&Expr>,
        update: Option<&Expr>,
        body: &Stmt,
    ) -> Result<(), CompileError> {
        self.emit(Opcode::EnterScope, 0);
        if let Some(init) = init {
            match init {
                ForInit::VarDecl(kind, declarations) => {
                    for declaration in declarations {
                        self.compile_declarator(*kind, declaration)?;
                    }
                }
                ForInit::Expr(expression) => {
                    self.compile_expression(expression)?;
                    self.emit(Opcode::Pop, 0);
                }
            }
        }
        let test_start = self.position();
        let exit = if let Some(test) = test {
            self.compile_expression(test)?;
            Some(self.emit_jump(Opcode::JumpIfFalse))
        } else {
            None
        };
        if exit.is_some() {
            self.emit(Opcode::Pop, 0);
        }
        self.loops.push(LoopContext::loop_body());
        self.compile_statement(body)?;
        let update_start = self.position();
        let context = self.loops.pop().expect("just pushed a loop context");
        for jump in context.continues {
            self.patch_jump(jump, update_start);
        }
        if let Some(update) = update {
            self.compile_expression(update)?;
            self.emit(Opcode::Pop, 0);
        }
        self.emit(Opcode::Jump, test_start as u64);
        if let Some(exit) = exit {
            self.patch_jump(exit, self.position());
            self.emit(Opcode::Pop, 0);
        }
        let end = self.position();
        for jump in context.breaks {
            self.patch_jump(jump, end);
        }
        self.emit(Opcode::LeaveScope, 0);
        Ok(())
    }

    fn compile_for_each(
        &mut self,
        left: &ForHead,
        right: &Expr,
        body: &Stmt,
        of: bool,
    ) -> Result<(), CompileError> {
        self.emit(Opcode::EnterScope, 0);
        self.compile_expression(right)?;
        self.emit(
            if of {
                Opcode::IteratorStartOf
            } else {
                Opcode::IteratorStartIn
            },
            0,
        );
        let start = self.position();
        let exit = self.emit_jump(Opcode::IteratorNext);
        self.bind_for_head(left)?;
        self.loops.push(LoopContext::loop_body());
        self.compile_statement(body)?;
        let context = self.loops.pop().expect("just pushed a loop context");
        for jump in context.continues {
            self.patch_jump(jump, start);
        }
        self.emit(Opcode::Jump, start as u64);
        self.patch_jump(exit, self.position());
        let end = self.position();
        for jump in context.breaks {
            self.patch_jump(jump, end);
        }
        self.emit(Opcode::LeaveScope, 0);
        Ok(())
    }

    fn bind_for_head(&mut self, head: &ForHead) -> Result<(), CompileError> {
        match head {
            ForHead::Decl(kind, Pattern::Identifier(name)) => {
                self.emit_name(declaration_opcode(*kind), name)
            }
            ForHead::Decl(kind, pattern) => {
                let index = self.register_pattern(pattern)?;
                self.emit(declaration_pattern_opcode(*kind), index as u64);
            }
            ForHead::Pattern(pattern) => {
                let index = self.register_pattern(pattern)?;
                self.emit(Opcode::BindPattern, index as u64);
            }
        }
        Ok(())
    }

    fn compile_switch(
        &mut self,
        discriminant: &Expr,
        cases: &[SwitchCase],
    ) -> Result<(), CompileError> {
        self.compile_expression(discriminant)?;
        let mut case_jumps = Vec::new();
        let mut default_case = None;
        for (index, case) in cases.iter().enumerate() {
            if let Some(test) = &case.test {
                self.emit(Opcode::Dup, 0);
                self.compile_expression(test)?;
                self.emit(Opcode::StrictEqual, 0);
                let matched = self.emit_jump(Opcode::JumpIfTruePop);
                case_jumps.push((index, matched));
            } else {
                default_case = Some(index);
            }
        }
        let fallback = self.emit_jump(Opcode::Jump);
        let mut trampolines = Vec::with_capacity(cases.len());
        for _ in cases {
            let entry = self.position();
            self.emit(Opcode::Pop, 0);
            let body_jump = self.emit_jump(Opcode::Jump);
            trampolines.push((entry, body_jump));
        }
        for (case, jump) in case_jumps {
            self.patch_jump(jump, trampolines[case].0);
        }
        let unmatched = self.position();
        self.emit(Opcode::Pop, 0);
        let unmatched_end = self.emit_jump(Opcode::Jump);
        let fallback_target = default_case.map_or(unmatched, |case| trampolines[case].0);
        self.patch_jump(fallback, fallback_target);
        self.loops.push(LoopContext::default());
        for (index, case) in cases.iter().enumerate() {
            let body = self.position();
            self.patch_jump(trampolines[index].1, body);
            for statement in &case.consequent {
                self.compile_statement(statement)?;
            }
        }
        let context = self.loops.pop().expect("just pushed switch context");
        let end = self.position();
        self.patch_jump(unmatched_end, end);
        for jump in context.breaks {
            self.patch_jump(jump, end);
        }
        Ok(())
    }

    fn compile_try(
        &mut self,
        block: &[Stmt],
        handler: Option<&CatchClause>,
        finalizer: Option<&[Stmt]>,
    ) -> Result<(), CompileError> {
        let handler_jump = self.emit_jump(Opcode::TryBegin);
        for statement in block {
            self.compile_statement(statement)?;
        }
        self.emit(Opcode::TryEnd, 0);
        if let Some(finalizer) = finalizer {
            for statement in finalizer {
                self.compile_statement(statement)?;
            }
        }
        let end_jump = self.emit_jump(Opcode::Jump);
        let handler_start = self.position();
        self.patch_jump(handler_jump, handler_start);
        if let Some(handler) = handler {
            self.emit(Opcode::EnterScope, 0);
            if let Some(pattern) = &handler.param {
                let index = self.register_pattern(pattern)?;
                self.emit(Opcode::DeclarePatternLet, index as u64);
            } else {
                self.emit(Opcode::Pop, 0);
            }
            for statement in &handler.body {
                self.compile_statement(statement)?;
            }
            self.emit(Opcode::LeaveScope, 0);
            if let Some(finalizer) = finalizer {
                for statement in finalizer {
                    self.compile_statement(statement)?;
                }
            }
        } else {
            if let Some(finalizer) = finalizer {
                for statement in finalizer {
                    self.compile_statement(statement)?;
                }
            }
            self.emit(Opcode::Throw, 0);
        }
        self.patch_jump(end_jump, self.position());
        Ok(())
    }

    fn compile_expression(&mut self, expression: &Expr) -> Result<(), CompileError> {
        match expression {
            Expr::Number(value) => {
                let constant = self.constant_value(Value::Number(*value));
                self.emit(Opcode::LoadConstant, constant);
            }
            Expr::String(value) => {
                let constant = self.constant_value(Value::String(value.clone()));
                self.emit(Opcode::LoadConstant, constant);
            }
            Expr::Bool(true) => {
                self.emit(Opcode::LoadTrue, 0);
            }
            Expr::Bool(false) => {
                self.emit(Opcode::LoadFalse, 0);
            }
            Expr::Null => {
                self.emit(Opcode::LoadNull, 0);
            }
            Expr::This => {
                self.emit(Opcode::LoadThis, 0);
            }
            Expr::Identifier(name) => self.emit_name(Opcode::LoadBinding, name),
            Expr::Template {
                quasis,
                expressions,
            } => self.compile_template(quasis, expressions)?,
            Expr::Array(elements) => {
                self.emit(Opcode::MakeArray, 0);
                for element in elements {
                    match element {
                        None => {
                            self.emit(Opcode::ArrayHole, 0);
                        }
                        Some(ArrayElement::Normal(expression)) => {
                            self.compile_expression(expression)?;
                            self.emit(Opcode::ArrayPush, 0);
                        }
                        Some(ArrayElement::Spread(expression)) => {
                            self.compile_expression(expression)?;
                            self.emit(Opcode::ArraySpread, 0);
                        }
                    }
                }
            }
            Expr::Object(properties) => {
                self.emit(Opcode::MakeObject, 0);
                for property in properties {
                    match property {
                        ObjectProp::KeyValue { key, value, .. } => {
                            self.compile_property_key(key)?;
                            self.compile_expression(value)?;
                            self.emit(Opcode::ObjectSet, 0);
                        }
                        ObjectProp::Spread(value) => {
                            self.compile_expression(value)?;
                            self.emit(Opcode::ObjectSpread, 0);
                        }
                    }
                }
            }
            Expr::Function(function) => {
                let id = self.compile_function(function, false)?;
                self.emit(Opcode::MakeFunction, id as u64);
            }
            Expr::Arrow { params, body } => {
                let function = Function {
                    name: None,
                    params: params.clone(),
                    body: arrow_body_to_statements(body),
                };
                let id = self.compile_function(&function, true)?;
                self.emit(Opcode::MakeFunction, id as u64);
            }
            Expr::Unary { op, arg } => {
                self.compile_expression(arg)?;
                self.emit(unary_opcode(*op), 0);
            }
            Expr::Update { op, arg, prefix } => self.compile_update(*op, arg, *prefix)?,
            Expr::Binary { op, left, right } => {
                self.compile_expression(left)?;
                self.compile_expression(right)?;
                self.emit(binary_opcode(*op), 0);
            }
            Expr::Logical { op, left, right } => self.compile_logical(*op, left, right)?,
            Expr::Assign { op, target, value } => self.compile_assignment(*op, target, value)?,
            Expr::Conditional {
                test,
                consequent,
                alternate,
            } => {
                self.compile_expression(test)?;
                let false_jump = self.emit_jump(Opcode::JumpIfFalse);
                self.emit(Opcode::Pop, 0);
                self.compile_expression(consequent)?;
                let end_jump = self.emit_jump(Opcode::Jump);
                self.patch_jump(false_jump, self.position());
                self.emit(Opcode::Pop, 0);
                self.compile_expression(alternate)?;
                self.patch_jump(end_jump, self.position());
            }
            Expr::Call { callee, args } => self.compile_call(callee, args, false)?,
            Expr::New { callee, args } => self.compile_call(callee, args, true)?,
            Expr::Member {
                object,
                property,
                computed,
            } => {
                self.compile_expression(object)?;
                self.compile_member_key(property, *computed)?;
                self.emit(Opcode::GetProperty, 0);
            }
        }
        Ok(())
    }

    fn compile_template(
        &mut self,
        quasis: &[String],
        expressions: &[Expr],
    ) -> Result<(), CompileError> {
        let first = quasis.first().cloned().unwrap_or_default();
        let constant = self.constant_value(Value::String(first));
        self.emit(Opcode::LoadConstant, constant);
        for (index, expression) in expressions.iter().enumerate() {
            self.compile_expression(expression)?;
            self.emit(Opcode::Add, 0);
            let suffix = quasis.get(index + 1).cloned().unwrap_or_default();
            let constant = self.constant_value(Value::String(suffix));
            self.emit(Opcode::LoadConstant, constant);
            self.emit(Opcode::Add, 0);
        }
        Ok(())
    }

    fn compile_logical(
        &mut self,
        op: LogicalOp,
        left: &Expr,
        right: &Expr,
    ) -> Result<(), CompileError> {
        self.compile_expression(left)?;
        self.emit(Opcode::Dup, 0);
        let keep_left = self.emit_jump(match op {
            LogicalOp::And => Opcode::JumpIfFalse,
            LogicalOp::Or => Opcode::JumpIfTrue,
            LogicalOp::Nullish => Opcode::JumpIfNotNullish,
        });
        self.emit(Opcode::Pop, 0);
        self.compile_expression(right)?;
        let end = self.emit_jump(Opcode::Jump);
        self.patch_jump(keep_left, self.position());
        self.emit(Opcode::Pop, 0);
        self.patch_jump(end, self.position());
        Ok(())
    }

    fn compile_assignment(
        &mut self,
        op: AssignOp,
        target: &Expr,
        value: &Expr,
    ) -> Result<(), CompileError> {
        match target {
            Expr::Identifier(name) => {
                if op == AssignOp::Assign {
                    self.compile_expression(value)?;
                } else {
                    self.emit_name(Opcode::LoadBinding, name);
                    self.compile_expression(value)?;
                    self.emit(assign_binary_opcode(op), 0);
                }
                self.emit(Opcode::Dup, 0);
                self.emit_name(Opcode::StoreBinding, name);
            }
            Expr::Member {
                object,
                property,
                computed,
            } => {
                self.compile_expression(object)?;
                self.compile_member_key(property, *computed)?;
                if op != AssignOp::Assign {
                    self.emit(Opcode::Dup2, 0);
                    self.emit(Opcode::GetProperty, 0);
                }
                self.compile_expression(value)?;
                if op != AssignOp::Assign {
                    self.emit(assign_binary_opcode(op), 0);
                }
                self.emit(Opcode::SetProperty, 0);
            }
            _ => {
                return Err(CompileError {
                    message: "assignment target must be an identifier or member expression"
                        .to_string(),
                });
            }
        }
        Ok(())
    }

    fn compile_update(
        &mut self,
        op: UpdateOp,
        target: &Expr,
        prefix: bool,
    ) -> Result<(), CompileError> {
        let binary = match op {
            UpdateOp::Inc => Opcode::Add,
            UpdateOp::Dec => Opcode::Subtract,
        };
        match target {
            Expr::Identifier(name) => {
                self.emit_name(Opcode::LoadBinding, name);
                if !prefix {
                    self.emit(Opcode::Dup, 0);
                }
                let one = self.constant_value(Value::Number(1.0));
                self.emit(Opcode::LoadConstant, one);
                self.emit(binary, 0);
                if prefix {
                    self.emit(Opcode::Dup, 0);
                }
                self.emit_name(Opcode::StoreBinding, name);
            }
            Expr::Member {
                object,
                property,
                computed,
            } => {
                self.compile_expression(object)?;
                self.compile_member_key(property, *computed)?;
                self.emit(Opcode::Dup2, 0);
                self.emit(Opcode::GetProperty, 0);
                if !prefix {
                    self.emit(Opcode::Dup, 0);
                }
                let one = self.constant_value(Value::Number(1.0));
                self.emit(Opcode::LoadConstant, one);
                self.emit(binary, 0);
                self.emit(
                    if prefix {
                        Opcode::SetProperty
                    } else {
                        Opcode::SetPropertyKeepOld
                    },
                    0,
                );
            }
            _ => {
                return Err(CompileError {
                    message: "update target must be an identifier or member expression".to_string(),
                });
            }
        }
        Ok(())
    }

    fn compile_call(
        &mut self,
        callee: &Expr,
        args: &[Argument],
        construct: bool,
    ) -> Result<(), CompileError> {
        let has_spread = args
            .iter()
            .any(|argument| matches!(argument, Argument::Spread(_)));
        if let Expr::Member {
            object,
            property,
            computed,
        } = callee
        {
            self.compile_expression(object)?;
            self.emit(Opcode::Dup, 0);
            self.compile_member_key(property, *computed)?;
            self.emit(Opcode::GetProperty, 0);
            if has_spread {
                self.compile_spread_arguments(args)?;
            } else {
                self.compile_arguments(args)?;
            }
            self.emit(
                if has_spread && construct {
                    Opcode::ConstructSpread
                } else if has_spread {
                    Opcode::CallWithThisSpread
                } else if construct {
                    Opcode::Construct
                } else {
                    Opcode::CallWithThis
                },
                if has_spread { 0 } else { args.len() as u64 },
            );
        } else {
            self.compile_expression(callee)?;
            if has_spread {
                self.compile_spread_arguments(args)?;
            } else {
                self.compile_arguments(args)?;
            }
            self.emit(
                if has_spread && construct {
                    Opcode::ConstructSpread
                } else if has_spread {
                    Opcode::CallSpread
                } else if construct {
                    Opcode::Construct
                } else {
                    Opcode::Call
                },
                if has_spread { 0 } else { args.len() as u64 },
            );
        }
        Ok(())
    }

    fn compile_arguments(&mut self, args: &[Argument]) -> Result<(), CompileError> {
        for argument in args {
            match argument {
                Argument::Normal(expression) | Argument::Spread(expression) => {
                    self.compile_expression(expression)?
                }
            }
        }
        Ok(())
    }

    /// Materializes only call sites containing a spread into an argument
    /// array. Non-spread calls retain their compact count operand; the
    /// separate bytecodes mean a later call-site cache can optimize either
    /// representation without changing the source-level calling convention.
    fn compile_spread_arguments(&mut self, args: &[Argument]) -> Result<(), CompileError> {
        self.emit(Opcode::MakeArray, 0);
        for argument in args {
            match argument {
                Argument::Normal(expression) => {
                    self.compile_expression(expression)?;
                    self.emit(Opcode::ArrayPush, 0);
                }
                Argument::Spread(expression) => {
                    self.compile_expression(expression)?;
                    self.emit(Opcode::ArraySpread, 0);
                }
            }
        }
        Ok(())
    }

    fn compile_property_key(&mut self, key: &PropertyKey) -> Result<(), CompileError> {
        match key {
            PropertyKey::Identifier(value) | PropertyKey::String(value) => {
                let constant = self.constant_value(Value::String(value.clone()));
                self.emit(Opcode::LoadConstant, constant);
            }
            PropertyKey::Number(value) => {
                let constant = self.constant_value(Value::String(value.to_string()));
                self.emit(Opcode::LoadConstant, constant);
            }
            PropertyKey::Computed(expression) => self.compile_expression(expression)?,
        }
        Ok(())
    }

    fn compile_member_key(&mut self, property: &Expr, computed: bool) -> Result<(), CompileError> {
        if computed {
            return self.compile_expression(property);
        }
        let Expr::Identifier(name) = property else {
            return Err(CompileError {
                message: "non-computed member access requires an identifier property".to_string(),
            });
        };
        let constant = self.constant_value(Value::String(name.clone()));
        self.emit(Opcode::LoadConstant, constant);
        Ok(())
    }

    fn compile_function(
        &mut self,
        function: &Function,
        is_arrow: bool,
    ) -> Result<u32, CompileError> {
        let id = self.module.functions.len() as u32;
        self.module.functions.push(BytecodeFunction {
            name: function.name.clone(),
            parameters: Vec::new(),
            code: Vec::new(),
            is_arrow,
        });
        let parameters = function
            .params
            .iter()
            .map(|parameter| self.lower_parameter(parameter))
            .collect::<Result<Vec<_>, _>>()?;
        self.module.functions[id as usize].parameters = parameters;
        let previous_function = self.current_function;
        let previous_loops = std::mem::take(&mut self.loops);
        self.current_function = id as usize;
        for statement in &function.body {
            self.compile_statement(statement)?;
        }
        self.emit(Opcode::LoadUndefined, 0);
        self.emit(Opcode::Return, 0);
        self.current_function = previous_function;
        self.loops = previous_loops;
        Ok(id)
    }

    fn compile_expression_function(&mut self, expression: &Expr) -> Result<u32, CompileError> {
        let function = Function {
            name: None,
            params: Vec::new(),
            body: vec![Stmt::Return(Some(expression.clone()))],
        };
        self.compile_function(&function, false)
    }

    fn lower_parameter(&mut self, parameter: &Param) -> Result<CompiledParameter, CompileError> {
        Ok(CompiledParameter {
            pattern: self.build_pattern(&parameter.pattern)?,
            default_function: parameter
                .default
                .as_ref()
                .map(|expression| self.compile_expression_function(expression))
                .transpose()?,
            rest: parameter.rest,
        })
    }

    fn register_pattern(&mut self, pattern: &Pattern) -> Result<u32, CompileError> {
        let pattern = self.build_pattern(pattern)?;
        let index = self.module.patterns.len() as u32;
        self.module.patterns.push(pattern);
        Ok(index)
    }

    fn build_pattern(&mut self, pattern: &Pattern) -> Result<BindingPattern, CompileError> {
        match pattern {
            Pattern::Identifier(name) => Ok(BindingPattern::Identifier(name.clone())),
            Pattern::Array(elements) => Ok(BindingPattern::Array(
                elements
                    .iter()
                    .map(|element| {
                        element
                            .as_ref()
                            .map(|element| {
                                Ok(ArrayBindingElement {
                                    pattern: self.build_pattern(&element.pattern)?,
                                    default_function: element
                                        .default
                                        .as_ref()
                                        .map(|expression| {
                                            self.compile_expression_function(expression)
                                        })
                                        .transpose()?,
                                    rest: element.rest,
                                })
                            })
                            .transpose()
                    })
                    .collect::<Result<Vec<_>, CompileError>>()?,
            )),
            Pattern::Object(properties) => Ok(BindingPattern::Object(
                properties
                    .iter()
                    .map(|property| match property {
                        ObjectPatternProp::KeyValue {
                            key,
                            value,
                            default,
                        } => Ok(ObjectBindingProperty::KeyValue {
                            key: self.lower_binding_key(key)?,
                            value: self.build_pattern(value)?,
                            default_function: default
                                .as_ref()
                                .map(|expression| self.compile_expression_function(expression))
                                .transpose()?,
                        }),
                        ObjectPatternProp::Rest(pattern) => {
                            Ok(ObjectBindingProperty::Rest(self.build_pattern(pattern)?))
                        }
                    })
                    .collect::<Result<Vec<_>, CompileError>>()?,
            )),
        }
    }

    fn lower_binding_key(&mut self, key: &PropertyKey) -> Result<BindingKey, CompileError> {
        Ok(match key {
            PropertyKey::Identifier(value) | PropertyKey::String(value) => {
                BindingKey::Static(value.clone())
            }
            PropertyKey::Number(value) => BindingKey::Static(value.to_string()),
            PropertyKey::Computed(expression) => {
                BindingKey::ComputedFunction(self.compile_expression_function(expression)?)
            }
        })
    }
}

fn declaration_opcode(kind: DeclKind) -> Opcode {
    match kind {
        DeclKind::Var => Opcode::DeclareVar,
        DeclKind::Let => Opcode::DeclareLet,
        DeclKind::Const => Opcode::DeclareConst,
    }
}

fn declaration_pattern_opcode(kind: DeclKind) -> Opcode {
    match kind {
        DeclKind::Var => Opcode::DeclarePatternVar,
        DeclKind::Let => Opcode::DeclarePatternLet,
        DeclKind::Const => Opcode::DeclarePatternConst,
    }
}

fn unary_opcode(operator: UnaryOp) -> Opcode {
    match operator {
        UnaryOp::Neg => Opcode::UnaryNeg,
        UnaryOp::Plus => Opcode::UnaryPos,
        UnaryOp::Not => Opcode::UnaryNot,
        UnaryOp::Typeof => Opcode::Typeof,
    }
}

fn binary_opcode(operator: BinaryOp) -> Opcode {
    match operator {
        BinaryOp::Add => Opcode::Add,
        BinaryOp::Sub => Opcode::Subtract,
        BinaryOp::Mul => Opcode::Multiply,
        BinaryOp::Div => Opcode::Divide,
        BinaryOp::Mod => Opcode::Remainder,
        BinaryOp::Eq => Opcode::Equal,
        BinaryOp::NotEq => Opcode::NotEqual,
        BinaryOp::StrictEq => Opcode::StrictEqual,
        BinaryOp::StrictNotEq => Opcode::StrictNotEqual,
        BinaryOp::Lt => Opcode::LessThan,
        BinaryOp::Gt => Opcode::GreaterThan,
        BinaryOp::LtEq => Opcode::LessThanOrEqual,
        BinaryOp::GtEq => Opcode::GreaterThanOrEqual,
        BinaryOp::Instanceof => Opcode::Instanceof,
        BinaryOp::In => Opcode::In,
    }
}

fn assign_binary_opcode(operator: AssignOp) -> Opcode {
    match operator {
        AssignOp::Assign => unreachable!("plain assignment has no binary operation"),
        AssignOp::AddAssign => Opcode::Add,
        AssignOp::SubAssign => Opcode::Subtract,
        AssignOp::MulAssign => Opcode::Multiply,
        AssignOp::DivAssign => Opcode::Divide,
        AssignOp::ModAssign => Opcode::Remainder,
    }
}

fn arrow_body_to_statements(body: &ArrowBody) -> Vec<Stmt> {
    match body {
        ArrowBody::Expr(expression) => vec![Stmt::Return(Some((**expression).clone()))],
        ArrowBody::Block(statements) => statements.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    fn compile_source(source: &str) -> BytecodeModule {
        compile(&parse(source).unwrap()).unwrap()
    }

    #[test]
    fn lowers_expression_statements_to_bytecode_not_an_ast_walker_marker() {
        let module = compile_source("let x = 1; x += 2; x;");
        let opcodes = module
            .entry()
            .code
            .iter()
            .map(|instruction| instruction.opcode())
            .collect::<Vec<_>>();

        assert!(opcodes.contains(&Opcode::DeclareLet));
        assert!(opcodes.contains(&Opcode::StoreBinding));
        assert!(opcodes.contains(&Opcode::SetCompletion));
        assert_eq!(opcodes.last(), Some(&Opcode::Halt));
    }

    #[test]
    fn lowers_control_flow_functions_destructuring_and_try_to_fixed_width_instructions() {
        let module = compile_source(
            "function twice({ value = 2 }) { try { return value * 2; } catch (error) { return 0; } finally { console.log('done'); } } for (let value of [1, 2]) { if (value) { twice({ value }); } }",
        );
        let all_opcodes = module
            .functions
            .iter()
            .flat_map(|function| function.code.iter())
            .map(|instruction| instruction.opcode())
            .collect::<Vec<_>>();

        assert!(all_opcodes.contains(&Opcode::MakeFunction));
        assert!(all_opcodes.contains(&Opcode::TryBegin));
        assert!(all_opcodes.contains(&Opcode::IteratorStartOf));
        assert!(all_opcodes.contains(&Opcode::DeclarePatternLet));
        assert!(
            !module.patterns.is_empty(),
            "destructuring metadata is attached to declaration bytecode"
        );
        assert!(module.functions.iter().all(|function| {
            function
                .code
                .iter()
                .all(|instruction| std::mem::size_of_val(instruction) == 8)
        }));
    }

    #[test]
    fn computed_destructuring_keys_compile_to_helper_functions() {
        let module = compile_source("const { [key]: value = 1, ...rest } = source;");

        assert!(
            module.functions.len() >= 3,
            "computed key and default are compiled helpers"
        );
        assert!(
            !module.patterns.is_empty(),
            "the declaration bytecode references the lowered pattern metadata"
        );
    }
}
