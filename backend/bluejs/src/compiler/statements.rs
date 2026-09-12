// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Compiler {
    pub(super) fn statements(&mut self, statements: &[Stmt]) -> Result<(), CompileError> {
        self.function_declarations(statements)?;
        self.statements_after_function_declarations(statements)
    }

    /// Module linking separates declaration instantiation from evaluation.
    /// Keeping the declaration prefix explicit lets the VM run it for every
    /// member of a cyclic graph before it starts evaluating any body.
    pub(super) fn function_declarations(
        &mut self,
        statements: &[Stmt],
    ) -> Result<(), CompileError> {
        for statement in statements {
            let (function, binding_name) = match statement {
                Stmt::FunctionDecl(function) => (
                    function,
                    function.name.as_ref().expect("declaration has a name"),
                ),
                Stmt::ModuleDefaultFunction { function, binding } => (function, binding),
                _ => continue,
            };
            if matches!(statement, Stmt::ModuleDefaultFunction { binding, .. } if binding == MODULE_DEFAULT_BINDING)
            {
                // An anonymous default function declaration has a private
                // module binding, but its function object is named
                // `"default"`.  This is inference, not a named function
                // expression, so it must not create an inner `default`
                // lexical binding.
                self.function_named(function, false, Some("default"), false)?;
            } else {
                self.function(function, false)?;
            }
            let slot = self.resolve(binding_name).unwrap();
            if self.bytecode.bindings[slot as usize].lexical {
                self.emit(Opcode::InitializeBinding, slot)?;
            } else {
                self.emit(Opcode::StoreBinding, slot)?;
                self.emit(Opcode::Pop, 0)?;
            }
            // Annex B.3.2/B.3.3 only supplies the legacy outer var for
            // ordinary functions. Generator and async declarations stay
            // exclusively lexical even in sloppy code.
            if matches!(statement, Stmt::FunctionDecl(_)) && is_annex_b_function(function) {
                if let Some(outer) = self.annex_b_outer_var_slot(slot) {
                    self.emit(Opcode::GetBinding, slot)?;
                    self.emit(Opcode::StoreBinding, outer)?;
                    self.emit(Opcode::Pop, 0)?;
                }
            }
        }
        Ok(())
    }

    pub(super) fn statements_after_function_declarations(
        &mut self,
        statements: &[Stmt],
    ) -> Result<(), CompileError> {
        for statement in statements {
            self.statement(statement, true)?;
        }
        Ok(())
    }

    pub(super) fn statement(
        &mut self,
        statement: &Stmt,
        declarations_allowed: bool,
    ) -> Result<(), CompileError> {
        match statement {
            Stmt::Throw(value) => {
                self.expression(value)?;
                self.emit(Opcode::Throw, 0)?;
            }
            Stmt::Try {
                block,
                handler,
                finalizer,
            } => self.try_statement(block, handler.as_ref(), finalizer.as_deref())?,
            Stmt::With { object, body } => {
                if self.bytecode.strict {
                    return Err(CompileError::InvalidSyntax(
                        "with is forbidden in strict mode",
                    ));
                }
                self.expression(object)?;
                self.emit(Opcode::EnterWith, 0)?;
                self.with_depth += 1;
                let result = self.statement(body, false);
                self.with_depth -= 1;
                result?;
                self.emit(Opcode::LeaveWith, 0)?;
            }
            Stmt::FunctionDecl(_) | Stmt::ModuleDefaultFunction { .. } => {}
            Stmt::ClassDecl(class) => {
                let slot = self
                    .resolve(class.name.as_deref().expect("class declaration has a name"))
                    .unwrap();
                self.class_expression_with_binding(class, None, Some(slot))?;
            }
            Stmt::ClassField(statement) => {
                self.emit(Opcode::EnterClassFieldInitializer, 0)?;
                self.statement(statement, declarations_allowed)?;
                self.emit(Opcode::LeaveClassFieldInitializer, 0)?;
            }
            Stmt::ClassPrivateBrand(binding) => {
                let slot = self.resolve(binding).ok_or(CompileError::InvalidSyntax(
                    "private brand binding is not available in this function",
                ))?;
                self.emit(Opcode::InitializePrivateBrand, slot)?;
            }
            Stmt::Expr(Expr::Class(class)) => {
                self.class_expression(class, None)?;
                // A parenthesized class expression is still an expression
                // statement. Its value is observable as the completion of
                // direct and indirect eval, including `$262` foreign-realm
                // evaluation, so it must not be discarded here.
                self.emit(Opcode::SetCompletion, 0)?;
            }
            Stmt::Return(value) => {
                if !self.function {
                    return Err(CompileError::InvalidSyntax("return requires a function"));
                }
                if let Some(args) = value
                    .as_ref()
                    .and_then(|value| self.self_tail_call_args(value))
                {
                    for argument in args {
                        let Argument::Normal(value) = argument else {
                            unreachable!("self tail calls exclude spread arguments")
                        };
                        self.expression(value)?;
                    }
                    let iterators: Vec<_> = self
                        .loops
                        .iter()
                        .rev()
                        .filter_map(|context| context.iterator)
                        .collect();
                    for iterator in iterators {
                        self.emit(Opcode::GetBinding, iterator)?;
                        self.emit(Opcode::IteratorClose, 0)?;
                    }
                    self.emit(
                        Opcode::TailRecur,
                        u32::try_from(args.len()).map_err(|_| CompileError::ProgramTooLarge)?,
                    )?;
                    return Ok(());
                }
                if let Some(value) = value {
                    self.expression(value)?;
                } else {
                    self.constant(Value::Undefined)?;
                }
                let iterators: Vec<_> = self
                    .loops
                    .iter()
                    .rev()
                    .filter_map(|context| context.iterator)
                    .collect();
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
                self.enter_scope(block_lexical_names(body)?, &var_names(body)?, false)?;
                self.statements(body)?;
                self.leave_scope()?;
            }
            Stmt::VarDecl(kind, declarations) => {
                if !declarations_allowed && *kind != DeclKind::Var {
                    return Err(CompileError::InvalidSyntax(
                        "a lexical declaration requires a block",
                    ));
                }
                self.declarations(*kind, declarations)?;
            }
            Stmt::If {
                test,
                consequent,
                alternate,
            } => {
                self.emit(Opcode::ClearCompletion, 0)?;
                self.expression(test)?;
                let no = self.emit(Opcode::JumpIfFalse, 0)?;
                self.if_clause_statement(consequent)?;
                let end = self.emit(Opcode::Jump, 0)?;
                self.patch(no, self.offset()?);
                if let Some(alternate) = alternate {
                    self.if_clause_statement(alternate)?;
                }
                self.patch(end, self.offset()?);
            }
            Stmt::While { test, body } => {
                self.loop_statement(None, Some(test), None, body, false, Vec::new())?
            }
            Stmt::DoWhile { body, test } => {
                self.loop_statement(None, Some(test), None, body, true, Vec::new())?
            }
            Stmt::For {
                init,
                test,
                update,
                body,
            } => self.loop_statement(
                init.as_ref(),
                test.as_ref(),
                update.as_ref(),
                body,
                false,
                Vec::new(),
            )?,
            Stmt::ForIn { left, right, body } => self.for_in(left, right, body, Vec::new())?,
            Stmt::ForOf {
                left,
                right,
                body,
                is_await,
            } => self.for_of(left, right, body, *is_await, Vec::new())?,
            Stmt::Switch {
                discriminant,
                cases,
            } => self.switch_statement(discriminant, cases, Vec::new())?,
            Stmt::Labelled { label, item } => self.labelled_statement(label, item)?,
            Stmt::Break(label) => self.control_transfer(label.as_deref(), false)?,
            Stmt::Continue(label) => self.control_transfer(label.as_deref(), true)?,
        }
        Ok(())
    }

    /// Annex B.3.3 parses a sloppy FunctionDeclaration in an `if` clause as
    /// a synthetic block whose lexical function binding is then copied to the
    /// Annex B outer var binding when that clause executes.
    pub(super) fn if_clause_statement(&mut self, statement: &Stmt) -> Result<(), CompileError> {
        if !self.bytecode.strict && matches!(statement, Stmt::FunctionDecl(_)) {
            self.enter_scope(
                block_lexical_names(std::slice::from_ref(statement))?,
                &BTreeSet::new(),
                false,
            )?;
            self.statements(std::slice::from_ref(statement))?;
            self.leave_scope()
        } else {
            self.statement(statement, false)
        }
    }

    pub(super) fn labelled_statement(
        &mut self,
        label: &str,
        item: &Stmt,
    ) -> Result<(), CompileError> {
        let mut labels = vec![label.to_string()];
        let mut item = item;
        while let Stmt::Labelled {
            label,
            item: nested,
        } = item
        {
            labels.push(label.clone());
            item = nested;
        }
        if labels
            .iter()
            .any(|label| label == "yield" && self.bytecode.strict)
        {
            return Err(CompileError::InvalidSyntax(
                "yield cannot be used as a label in strict code",
            ));
        }
        if labels.iter().any(|label| {
            labels.iter().filter(|other| *other == label).count() != 1
                || self
                    .loops
                    .iter()
                    .any(|context| context.labels.iter().any(|other| other == label))
        }) {
            return Err(CompileError::InvalidSyntax("duplicate label"));
        }
        match item {
            Stmt::While { test, body } => {
                self.loop_statement(None, Some(test), None, body, false, labels)
            }
            Stmt::DoWhile { body, test } => {
                self.loop_statement(None, Some(test), None, body, true, labels)
            }
            Stmt::For {
                init,
                test,
                update,
                body,
            } => self.loop_statement(
                init.as_ref(),
                test.as_ref(),
                update.as_ref(),
                body,
                false,
                labels,
            ),
            Stmt::ForIn { left, right, body } => self.for_in(left, right, body, labels),
            Stmt::ForOf {
                left,
                right,
                body,
                is_await,
            } => self.for_of(left, right, body, *is_await, labels),
            Stmt::Switch {
                discriminant,
                cases,
            } => self.switch_statement(discriminant, cases, labels),
            Stmt::VarDecl(kind, _) if *kind != DeclKind::Var => Err(CompileError::InvalidSyntax(
                "a labelled statement cannot contain a lexical declaration",
            )),
            Stmt::ClassDecl(_) => Err(CompileError::InvalidSyntax(
                "a labelled statement cannot contain a class declaration",
            )),
            Stmt::FunctionDecl(function)
                if self.bytecode.strict || function.generator || function.is_async =>
            {
                Err(CompileError::InvalidSyntax(
                    "invalid labelled function declaration",
                ))
            }
            Stmt::FunctionDecl(function) => {
                // Annex B permits this sloppy-mode form. Its binding is
                // var-scoped, while creation occurs when the label executes.
                self.function(function, false)?;
                let slot = self
                    .resolve(function.name.as_ref().expect("declaration has a name"))
                    .unwrap();
                self.emit(Opcode::StoreBinding, slot)?;
                self.emit(Opcode::Pop, 0)?;
                Ok(())
            }
            _ => {
                self.loops.push(Loop {
                    labels,
                    breakable: false,
                    scope_depth: self.scopes.len(),
                    breaks: Vec::new(),
                    continues: None,
                    iterator: None,
                });
                self.statement(item, false)?;
                let end = self.offset()?;
                let context = self.loops.pop().expect("label control is active");
                for (jump, control) in context.breaks {
                    self.patch(jump, end);
                    self.bytecode.abrupt_jumps[control].target = end;
                }
                Ok(())
            }
        }
    }

    pub(super) fn control_transfer(
        &mut self,
        label: Option<&str>,
        is_continue: bool,
    ) -> Result<(), CompileError> {
        let index = match label {
            Some(label) => self
                .loops
                .iter()
                .rposition(|context| context.labels.iter().any(|candidate| candidate == label)),
            None if is_continue => self
                .loops
                .iter()
                .rposition(|context| context.continues.is_some()),
            None => self.loops.iter().rposition(|context| context.breakable),
        };
        let Some(index) = index else {
            return Err(CompileError::InvalidSyntax(if is_continue {
                "continue requires an enclosing iteration statement"
            } else {
                "break requires an enclosing loop, switch, or label"
            }));
        };
        if is_continue && self.loops[index].continues.is_none() {
            return Err(CompileError::InvalidSyntax(
                "continue label does not name an iteration statement",
            ));
        }
        let scopes: Vec<_> = self.scopes[self.loops[index].scope_depth..]
            .iter()
            .rev()
            .copied()
            .collect();
        let first_iterator = index + usize::from(is_continue);
        let iterators: Vec<_> = self.loops[first_iterator..]
            .iter()
            .rev()
            .filter_map(|context| context.iterator)
            .collect();
        // A direct jump would skip a surrounding `finally`. Route to a local
        // cleanup gateway first; handlers resume there only after finalizers.
        let control = self.bytecode.abrupt_jumps.len();
        let control_operand = u32::try_from(control).map_err(|_| CompileError::ProgramTooLarge)?;
        self.bytecode.abrupt_jumps.push(AbruptJump {
            cleanup: 0,
            target: 0,
        });
        self.emit(Opcode::AbruptJump, control_operand)?;
        let cleanup = self.offset()?;
        self.bytecode.abrupt_jumps[control].cleanup = cleanup;
        for iterator in iterators {
            self.emit(Opcode::GetBinding, iterator)?;
            self.emit(Opcode::IteratorClose, 0)?;
        }
        for scope in scopes {
            self.emit(Opcode::LeaveScope, scope)?;
        }
        let jump = self.emit(Opcode::Jump, 0)?;
        let context = &mut self.loops[index];
        if is_continue {
            context
                .continues
                .as_mut()
                .expect("selected context is an iteration statement")
                .push((jump, control));
        } else {
            context.breaks.push((jump, control));
        }
        Ok(())
    }

    pub(super) fn scoped_statements(&mut self, statements: &[Stmt]) -> Result<(), CompileError> {
        self.enter_scope(
            block_lexical_names(statements)?,
            &var_names(statements)?,
            false,
        )?;
        self.statements(statements)?;
        self.leave_scope()
    }

    pub(super) fn switch_statement(
        &mut self,
        discriminant: &Expr,
        cases: &[SwitchCase],
        labels: Vec<String>,
    ) -> Result<(), CompileError> {
        validate_switch_case_declarations(cases, self.bytecode.strict)?;
        self.emit(Opcode::ClearCompletion, 0)?;
        let lexical = switch_lexical_names(cases)?;
        let vars = switch_var_names(cases)?;
        // Switch evaluation creates its case-block lexical environment only
        // after evaluating the discriminant.  A closure created by the
        // discriminant must therefore capture the surrounding binding, while
        // closures created by case selectors or consequents capture the
        // switch-local binding.
        self.expression(discriminant)?;
        self.enter_scope(lexical, &vars, false)?;

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
        self.patch(
            no_match,
            default
                .map(|index| case_stubs[index])
                .unwrap_or(no_match_cleanup),
        );

        self.loops.push(Loop {
            labels,
            breakable: true,
            scope_depth: self.scopes.len(),
            breaks: Vec::new(),
            continues: None,
            iterator: None,
        });
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
    pub(super) fn try_statement(
        &mut self,
        block: &[Stmt],
        handler: Option<&CatchClause>,
        finalizer: Option<&[Stmt]>,
    ) -> Result<(), CompileError> {
        let handler_index = u32::try_from(self.bytecode.handlers.len())
            .map_err(|_| CompileError::ProgramTooLarge)?;
        self.bytecode.handlers.push(Handler {
            try_start: 0,
            try_end: 0,
            catch: None,
            catch_end: None,
            finally: None,
        });
        self.emit(Opcode::PushHandler, handler_index)?;

        // Each TryBlock has its own Completion. An empty block must not leak
        // the value of the preceding statement into TryStatement's
        // UpdateEmpty step.
        self.emit(Opcode::ClearCompletion, 0)?;
        self.bytecode.handlers[handler_index as usize].try_start = self.offset()?;
        self.scoped_statements(block)?;
        self.bytecode.handlers[handler_index as usize].try_end = self.offset()?;
        self.emit(Opcode::PopHandler, 0)?;
        if finalizer.is_some() {
            self.emit(Opcode::SaveCompletion, 0)?;
        }
        let normal_exit = self.emit(Opcode::Jump, 0)?;

        let catch_exit = if let Some(catch) = handler {
            let start = self.offset()?;
            self.bytecode.handlers[handler_index as usize].catch = Some(start);
            let parameter_bound_names = catch.param.as_ref().map(pattern_names).unwrap_or_default();
            if self.bytecode.strict
                && parameter_bound_names
                    .iter()
                    .any(|name| matches!(name.as_str(), "eval" | "arguments"))
            {
                return Err(CompileError::InvalidSyntax(
                    "strict catch parameters cannot bind eval or arguments",
                ));
            }
            if catch_lexical_names(&catch.body)
                .into_iter()
                .any(|name| parameter_bound_names.contains(&name))
            {
                return Err(CompileError::InvalidSyntax(
                    "a catch parameter conflicts with a lexical declaration",
                ));
            }
            let parameter_names = parameter_bound_names
                .into_iter()
                .map(|name| (name, DeclKind::Let))
                .collect();
            self.enter_scope(parameter_names, &BTreeSet::new(), false)?;
            let mut catch_var_slots = HashMap::new();
            if let Some(Pattern::Identifier(name)) = &catch.param {
                let slot = self.resolve(name).expect("catch parameter was declared");
                self.bytecode.bindings[slot as usize].catch_parameter = true;
                catch_var_slots.insert(name.clone(), slot);
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
            self.enter_scope(
                block_lexical_names(&catch.body)?,
                &var_names(&catch.body)?,
                false,
            )?;
            self.statements(&catch.body)?;
            self.leave_scope()?;
            self.catch_var_slots
                .pop()
                .expect("catch var override is active");
            self.leave_scope()?;
            self.bytecode.handlers[handler_index as usize].catch_end = Some(self.offset()?);
            self.emit(Opcode::PopHandler, 0)?;
            if finalizer.is_some() {
                self.emit(Opcode::SaveCompletion, 0)?;
            }
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
            if let Some(exit) = catch_exit {
                self.patch(exit, start);
            }
            debug_assert!(end as usize <= self.bytecode.code.len());
        } else {
            let end = self.offset()?;
            self.patch(normal_exit, end);
            if let Some(exit) = catch_exit {
                self.patch(exit, end);
            }
        }
        Ok(())
    }

    pub(super) fn declarations(
        &mut self,
        kind: DeclKind,
        declarations: &[VarDeclarator],
    ) -> Result<(), CompileError> {
        for declaration in declarations {
            if kind == DeclKind::Const && declaration.init.is_none() {
                return Err(CompileError::InvalidSyntax("const requires an initializer"));
            }
            if matches!(declaration.pattern, Pattern::Identifier(_))
                && kind == DeclKind::Var
                && declaration.init.is_none()
            {
                continue;
            }
            if let Some(value) = &declaration.init {
                let inferred_name = match (&declaration.pattern, self.bytecode.module) {
                    (Pattern::Identifier(name), true) if name == MODULE_DEFAULT_BINDING => {
                        Some("default")
                    }
                    _ => None,
                };
                self.expression_with_name(value, inferred_name)?
            } else {
                if !matches!(declaration.pattern, Pattern::Identifier(_)) {
                    return Err(CompileError::InvalidSyntax(
                        "a destructuring declaration requires an initializer",
                    ));
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
    pub(super) fn bind_pattern(
        &mut self,
        pattern: &Pattern,
        kind: DeclKind,
    ) -> Result<(), CompileError> {
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
                } else {
                    self.names.last().unwrap()[name]
                };
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
                        ObjectPatternProp::KeyValue {
                            key,
                            value,
                            default,
                        } => {
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
    pub(super) fn array_pattern_value(&mut self) -> Result<(), CompileError> {
        self.emit(Opcode::Dup, 0)?;
        let exhausted = self.emit(Opcode::IteratorStep, 0)?;
        let joined = self.emit(Opcode::Jump, 0)?;
        self.patch(exhausted, self.offset()?);
        self.constant(Value::Undefined)?;
        self.patch(joined, self.offset()?);
        Ok(())
    }

    /// Replaces an `undefined` destructuring-assignment value with its
    /// initializer. An anonymous function, class, or arrow default receives
    /// the IdentifierReference target's inferred name; `null` remains a
    /// value, as required by ECMA-262.
    pub(super) fn assignment_pattern_default(
        &mut self,
        default: Option<&Expr>,
        pattern: &AssignmentPattern,
    ) -> Result<(), CompileError> {
        let Some(default) = default else {
            return Ok(());
        };
        self.emit(Opcode::Dup, 0)?;
        self.constant(Value::Undefined)?;
        self.emit(Opcode::StrictEqual, 0)?;
        let skip = self.emit(Opcode::JumpIfFalse, 0)?;
        self.emit(Opcode::Pop, 0)?;
        self.expression_with_name(
            default,
            match pattern {
                AssignmentPattern::Target(target) => match &**target {
                    Expr::Identifier(name) => Some(name.as_str()),
                    _ => None,
                },
                _ => None,
            },
        )?;
        self.patch(skip, self.offset()?);
        Ok(())
    }

    pub(super) fn binding_pattern_default(
        &mut self,
        default: Option<&Expr>,
        pattern: &Pattern,
    ) -> Result<(), CompileError> {
        let Some(default) = default else {
            return Ok(());
        };
        self.emit(Opcode::Dup, 0)?;
        self.constant(Value::Undefined)?;
        self.emit(Opcode::StrictEqual, 0)?;
        let skip = self.emit(Opcode::JumpIfFalse, 0)?;
        self.emit(Opcode::Pop, 0)?;
        self.expression_with_name(
            default,
            match pattern {
                Pattern::Identifier(name) => Some(name.as_str()),
                _ => None,
            },
        )?;
        self.patch(skip, self.offset()?);
        Ok(())
    }

    pub(super) fn expression_with_name(
        &mut self,
        expression: &Expr,
        inferred_name: Option<&str>,
    ) -> Result<(), CompileError> {
        match expression {
            Expr::Parenthesized(expression) => self.expression_with_name(expression, inferred_name),
            Expr::Function(function) if function.name.is_none() && inferred_name.is_some() => {
                self.function_named(function, false, inferred_name, false)
            }
            Expr::Class(class) if class.name.is_none() && inferred_name.is_some() => {
                self.class_expression(class, inferred_name)
            }
            Expr::Arrow {
                params,
                body,
                is_async,
            } if inferred_name.is_some() => {
                let body = match body {
                    ArrowBody::Expr(expr) => vec![Stmt::Return(Some(*expr.clone()))],
                    ArrowBody::Block(body) => body.clone(),
                };
                self.function_named(
                    &Function {
                        name: None,
                        params: params.clone(),
                        body,
                        generator: false,
                        is_async: *is_async,
                    },
                    true,
                    inferred_name,
                    false,
                )
            }
            _ => self.expression(expression),
        }
    }

    pub(super) fn loop_statement(
        &mut self,
        init: Option<&ForInit>,
        test: Option<&Expr>,
        update: Option<&Expr>,
        body: &Stmt,
        do_first: bool,
        labels: Vec<String>,
    ) -> Result<(), CompileError> {
        self.emit(Opcode::ClearCompletion, 0)?;
        let lexical = match init {
            Some(ForInit::VarDecl(kind, decls)) if *kind != DeclKind::Var => {
                declarations_names(*kind, decls)?
            }
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
        self.loops.push(Loop {
            labels,
            breakable: true,
            scope_depth: self.scopes.len(),
            breaks: Vec::new(),
            continues: Some(Vec::new()),
            iterator: None,
        });
        self.statement(body, false)?;
        let continue_at = self.offset()?;
        // CreatePerIterationEnvironment happens after the body and before
        // the update expression.  That leaves closures made by this turn
        // attached to its old cells while the update writes into the next
        // iteration's bindings.  `continue` targets this point as well.
        if own_scope {
            let scope = *self
                .scopes
                .last()
                .expect("lexical for scope remains active");
            self.emit(Opcode::CloneScope, scope)?;
        }
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
}
