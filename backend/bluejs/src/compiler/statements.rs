// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

/// The `(key, computed, value)` of a lowered public instance field
/// (`this[key] = value`, see `class_instance_field`); a private field stays a
/// private-name store and any other statement is not a field definition.
fn public_field_definition(statement: &Stmt) -> Option<(&Expr, bool, &Expr)> {
    let Stmt::Expr(Expr::Assign {
        op: AssignOp::Assign,
        target,
        value,
    }) = statement
    else {
        return None;
    };
    let Expr::Member {
        object,
        property,
        computed,
    } = &**target
    else {
        return None;
    };
    if !matches!(**object, Expr::This) {
        return None;
    }
    if let (Expr::Identifier(name), false) = (&**property, computed) {
        if name.starts_with('#') {
            return None;
        }
    }
    Some((property, *computed, value))
}

fn is_function_declaration(statement: &Stmt) -> bool {
    matches!(
        statement,
        Stmt::FunctionDecl(_) | Stmt::ModuleDefaultFunction { .. }
    )
}

impl Compiler {
    pub(super) fn statements(&mut self, statements: &[Stmt]) -> Result<(), CompileError> {
        self.function_declarations(statements)?;
        self.statements_after_function_declarations(statements)
    }

    /// Like `statements`, but when `statements` directly declares a `using`/
    /// `await using` binding, arranges for every such binding's value to be
    /// disposed (reverse declaration order, `SuppressedError` on a second
    /// error) when this statement list's block exits -- normally or via an
    /// early return/break/continue/throw. Implemented as a synthetic
    /// `try { <statements> } finally { <native DisposeResources> }`, reusing
    /// the same handler-stack machinery `try_statement` uses for a real
    /// `finally` clause; see that function and `Opcode::DisposeResources`'s
    /// own doc comment.
    ///
    /// Every caller that can introduce a new using-eligible statement list
    /// (block statements, `try`/`catch`/`finally` bodies, function bodies)
    /// must call this instead of `statements` for the disposal to take
    /// effect; the common no-`using` case is exactly as cheap as before.
    pub(super) fn statements_with_disposal(
        &mut self,
        statements: &[Stmt],
    ) -> Result<(), CompileError> {
        self.function_declarations(statements)?;
        if !has_using_declaration(statements) {
            return self.statements_after_function_declarations(statements);
        }
        let is_async = has_await_using_declaration(statements);
        self.wrap_with_disposal(is_async, |this| {
            this.statements_after_function_declarations(statements)
        })
    }

    /// Wraps `compile_body` in a synthetic
    /// `try { <compile_body> } finally { <dispose> }`, reusing the same
    /// handler-stack machinery `try_statement` uses for a real `finally`
    /// clause. `statements_with_disposal` uses this for a using-declaring
    /// block/function body; `for_each` uses it for a per-iteration
    /// `using`/`await using` `ForBinding` (`for (using x of iterable)`),
    /// wrapping just the current iteration's bind-and-body so the bound
    /// value is disposed at the end of *that* iteration rather than only
    /// once the whole loop exits. `is_async` selects
    /// `compile_async_dispose_finally`'s `Await`-capable loop over the
    /// plain, single-native-opcode `DisposeResources` fast path.
    pub(super) fn wrap_with_disposal(
        &mut self,
        is_async: bool,
        compile_body: impl FnOnce(&mut Self) -> Result<(), CompileError>,
    ) -> Result<(), CompileError> {
        let handler_index = u32::try_from(self.bytecode.handlers.len())
            .map_err(|_| CompileError::ProgramTooLarge)?;
        self.bytecode.handlers.push(Handler {
            try_start: 0,
            try_end: 0,
            catch: None,
            catch_end: None,
            finally: None,
            finally_end: None,
        });
        self.emit(Opcode::PushHandler, handler_index)?;
        self.emit(Opcode::MarkDisposables, 0)?;
        // No `ClearCompletion` here, unlike a real `try` block: a block that
        // declares `using` is not a TryStatement, so its completion is the
        // statement list's own, and disposal returns that completion
        // unchanged (`DisposeResources`). Clearing would turn `4; {using x =
        // null;}` into `undefined` instead of `4`.
        self.bytecode.handlers[handler_index as usize].try_start = self.offset()?;
        // Disposal runs after the block's return value is computed.
        self.tail_call_blockers += 1;
        let body = compile_body(self);
        self.tail_call_blockers -= 1;
        body?;
        self.bytecode.handlers[handler_index as usize].try_end = self.offset()?;
        self.emit(Opcode::PopHandler, 0)?;
        self.emit(Opcode::SaveCompletion, 0)?;
        let normal_exit = self.emit(Opcode::Jump, 0)?;
        let finally_start = self.offset()?;
        self.bytecode.handlers[handler_index as usize].finally = Some(finally_start);
        if is_async {
            self.compile_async_dispose_finally(handler_index)?;
        } else {
            self.emit(Opcode::DisposeResources, handler_index)?;
        }
        self.emit(Opcode::ResumeCompletion, handler_index)?;
        self.bytecode.handlers[handler_index as usize].finally_end = Some(self.offset()?);
        self.patch(normal_exit, finally_start);
        Ok(())
    }

    /// The `await using`-capable disposal finally body: `Opcode::DisposeResources`
    /// runs the entire disposal loop as one atomic native step, which cannot
    /// suspend mid-loop the way a real `await` must. This compiles a
    /// synthesized (never user-visible) `while`/`try`/`catch` loop instead,
    /// built from ordinary AST nodes and compiled through the normal
    /// statement/expression pipeline -- `Await` included -- so a resource
    /// that needs awaiting suspends exactly like any other `await`
    /// expression, resuming and continuing the loop correctly. Only the
    /// initial drain (turning the native disposable-resource list into a
    /// plain JS value) is a native primitive; see `Opcode::DrainAsyncDisposables`
    /// and `Vm::build_async_dispose_state`.
    ///
    /// Equivalent, if it were written as source (`*name*` bindings are
    /// compiler-internal and cannot collide with user identifiers):
    /// ```text
    /// let [*hasError*, *pendingError*, *entries*] = <drain>;
    /// let *i* = *entries*.length;
    /// while (*i* > 0) {
    ///     *i* = *i* - 1;
    ///     let *entry* = *entries*[*i*];
    ///     try {
    ///         if (*entry*[1] !== undefined) {
    ///             let *result* = *entry*[2]
    ///                 ? *entry*[1].call(*entry*[0], *entry*[3])
    ///                 : *entry*[1].call(*entry*[0]);
    ///             if (*entry*[4]) { await (*entry*[5] ? undefined : *result*); }
    ///         } else if (*entry*[4]) {
    ///             await undefined;
    ///         }
    ///     } catch (*caught*) {
    ///         if (*hasError*) {
    ///             *pendingError* = new SuppressedError(*caught*, *pendingError*);
    ///         } else {
    ///             *pendingError* = *caught*;
    ///             *hasError* = true;
    ///         }
    ///     }
    /// }
    /// if (*hasError*) throw *pendingError*;
    /// ```
    fn compile_async_dispose_finally(&mut self, handler_index: u32) -> Result<(), CompileError> {
        fn ident(name: &str) -> Expr {
            Expr::Identifier(name.to_owned())
        }
        fn index(object: &str, at: f64) -> Expr {
            Expr::Member {
                object: Box::new(ident(object)),
                property: Box::new(Expr::Number(at)),
                computed: true,
            }
        }
        fn assign(target: &str, value: Expr) -> Stmt {
            Stmt::Expr(Expr::Assign {
                op: AssignOp::Assign,
                target: Box::new(ident(target)),
                value: Box::new(value),
            })
        }
        fn array_ident_pattern(names: &[&str]) -> Pattern {
            Pattern::Array(
                names
                    .iter()
                    .map(|name| {
                        Some(ArrayPatternElement {
                            pattern: Pattern::Identifier((*name).to_owned()),
                            default: None,
                            rest: false,
                        })
                    })
                    .collect(),
            )
        }
        fn let_decl(name: &str, init: Expr) -> Stmt {
            Stmt::VarDecl(
                DeclKind::Let,
                vec![VarDeclarator {
                    pattern: Pattern::Identifier(name.to_owned()),
                    init: Some(init),
                }],
            )
        }

        self.enter_scope(
            vec![
                ("*hasError*".to_owned(), DeclKind::Let),
                ("*pendingError*".to_owned(), DeclKind::Let),
                ("*entries*".to_owned(), DeclKind::Let),
                ("*i*".to_owned(), DeclKind::Let),
            ],
            &BTreeSet::new(),
            false,
        )?;
        self.emit(Opcode::DrainAsyncDisposables, handler_index)?;
        self.bind_pattern(
            &array_ident_pattern(&["*hasError*", "*pendingError*", "*entries*"]),
            DeclKind::Let,
        )?;
        self.expression(&Expr::Member {
            object: Box::new(ident("*entries*")),
            property: Box::new(ident("length")),
            computed: false,
        })?;
        let i_slot = self.resolve("*i*").expect("just declared above");
        self.emit(Opcode::InitializeBinding, i_slot)?;

        let call_entry = |with_argument: bool| Expr::Call {
            callee: Box::new(Expr::Member {
                object: Box::new(index("*entry*", 1.0)),
                property: Box::new(ident("call")),
                computed: false,
            }),
            args: if with_argument {
                vec![
                    Argument::Normal(index("*entry*", 0.0)),
                    Argument::Normal(index("*entry*", 3.0)),
                ]
            } else {
                vec![Argument::Normal(index("*entry*", 0.0))]
            },
        };
        let await_stmt = |value: Expr| Stmt::Expr(Expr::Await(Box::new(value)));
        let is_async_test = index("*entry*", 4.0);

        let try_block = vec![Stmt::If {
            test: Expr::Binary {
                op: BinaryOp::StrictNotEq,
                left: Box::new(index("*entry*", 1.0)),
                right: Box::new(ident("undefined")),
            },
            consequent: Box::new(Stmt::Block(vec![
                let_decl(
                    "*result*",
                    Expr::Conditional {
                        test: Box::new(index("*entry*", 2.0)),
                        consequent: Box::new(call_entry(true)),
                        alternate: Box::new(call_entry(false)),
                    },
                ),
                Stmt::If {
                    test: is_async_test.clone(),
                    consequent: Box::new(await_stmt(Expr::Conditional {
                        test: Box::new(index("*entry*", 5.0)),
                        consequent: Box::new(ident("undefined")),
                        alternate: Box::new(ident("*result*")),
                    })),
                    alternate: None,
                },
            ])),
            alternate: Some(Box::new(Stmt::If {
                test: is_async_test,
                consequent: Box::new(await_stmt(ident("undefined"))),
                alternate: None,
            })),
        }];
        let catch_clause = CatchClause {
            param: Some(Pattern::Identifier("*caught*".to_owned())),
            body: vec![Stmt::If {
                test: ident("*hasError*"),
                consequent: Box::new(assign(
                    "*pendingError*",
                    Expr::New {
                        callee: Box::new(ident("SuppressedError")),
                        args: vec![
                            Argument::Normal(ident("*caught*")),
                            Argument::Normal(ident("*pendingError*")),
                        ],
                    },
                )),
                alternate: Some(Box::new(Stmt::Block(vec![
                    assign("*pendingError*", ident("*caught*")),
                    assign("*hasError*", Expr::Bool(true)),
                ]))),
            }],
        };

        let loop_body = Stmt::Block(vec![
            assign(
                "*i*",
                Expr::Binary {
                    op: BinaryOp::Sub,
                    left: Box::new(ident("*i*")),
                    right: Box::new(Expr::Number(1.0)),
                },
            ),
            let_decl(
                "*entry*",
                Expr::Member {
                    object: Box::new(ident("*entries*")),
                    property: Box::new(ident("*i*")),
                    computed: true,
                },
            ),
            Stmt::Try {
                block: try_block,
                handler: Some(catch_clause),
                finalizer: None,
            },
        ]);

        self.loop_statement(
            None,
            Some(&Expr::Binary {
                op: BinaryOp::Gt,
                left: Box::new(ident("*i*")),
                right: Box::new(Expr::Number(0.0)),
            }),
            None,
            &loop_body,
            false,
            Vec::new(),
        )?;

        self.statement(
            &Stmt::If {
                test: ident("*hasError*"),
                consequent: Box::new(Stmt::Throw(ident("*pendingError*"))),
                alternate: None,
            },
            true,
        )?;

        self.leave_scope()?;
        Ok(())
    }

    /// Module linking separates declaration instantiation from evaluation.
    /// Keeping the declaration prefix explicit lets the VM run it for every
    /// member of a cyclic graph before it starts evaluating any body.
    pub(super) fn function_declarations(
        &mut self,
        statements: &[Stmt],
    ) -> Result<(), CompileError> {
        for statement in statements {
            self.function_declaration(statement)?;
        }
        Ok(())
    }

    /// Compiles root-level function declaration instantiation while preserving
    /// source-order offsets for the debugger/provenance boundary.
    pub(super) fn top_level_function_declarations(
        &mut self,
        statements: &[Stmt],
        offsets: &mut [Option<u32>],
    ) -> Result<(), CompileError> {
        debug_assert_eq!(statements.len(), offsets.len());
        for (index, statement) in statements.iter().enumerate() {
            if !is_function_declaration(statement) {
                continue;
            }
            offsets[index] = Some(self.offset()?);
            self.function_declaration(statement)?;
        }
        Ok(())
    }

    fn function_declaration(&mut self, statement: &Stmt) -> Result<(), CompileError> {
        let (function, binding_name) = match statement {
            Stmt::FunctionDecl(function) => (
                function,
                function.name.as_ref().expect("declaration has a name"),
            ),
            Stmt::ModuleDefaultFunction { function, binding } => (function, binding),
            _ => return Ok(()),
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
        let Some(slot) = self.resolve(binding_name) else {
            // A sloppy direct eval re-declaring a function that an earlier eval
            // in the same function created: the binding is dynamic, so the new
            // function object replaces its value.
            let index = self.name_constant(binding_name)?;
            self.emit(Opcode::SetUnboundName, index)?;
            self.emit(Opcode::Pop, 0)?;
            return Ok(());
        };
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

    /// Compiles every non-function root statement and records its exact first
    /// emitted root-code-unit instruction. A statement with no output remains
    /// explicitly unbound instead of inheriting the following statement's
    /// offset.
    pub(super) fn top_level_statements_after_function_declarations(
        &mut self,
        statements: &[Stmt],
        offsets: &mut [Option<u32>],
    ) -> Result<(), CompileError> {
        debug_assert_eq!(statements.len(), offsets.len());
        for (index, statement) in statements.iter().enumerate() {
            if is_function_declaration(statement) {
                continue;
            }
            let start = self.offset()?;
            self.statement(statement, true)?;
            if self.offset()? > start {
                offsets[index] = Some(start);
            }
        }
        Ok(())
    }

    pub(super) fn statement(
        &mut self,
        statement: &Stmt,
        declarations_allowed: bool,
    ) -> Result<(), CompileError> {
        if !declarations_allowed {
            // The body of an `if`, loop, `with` or label is a Statement, not a
            // StatementListItem: no class, function or generator/async
            // declaration (Annex B's sloppy `if (x) function f() {}` is
            // rewritten to a block before reaching here), and no labelled
            // function either.
            match statement {
                Stmt::ClassDecl(_) => {
                    return Err(CompileError::InvalidSyntax(
                        "a class declaration is not allowed in statement position",
                    ))
                }
                Stmt::FunctionDecl(_) => {
                    return Err(CompileError::InvalidSyntax(
                        "a function declaration is not allowed in statement position",
                    ))
                }
                Stmt::Labelled { .. } if is_labelled_function(statement) => {
                    return Err(CompileError::InvalidSyntax(
                        "a labelled function declaration is not allowed in statement position",
                    ))
                }
                _ => {}
            }
        }
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
                // UpdateEmpty(stmtResult, undefined): an empty body leaves
                // `undefined`, not the previous statement's value.
                self.emit(Opcode::ClearCompletion, 0)?;
                self.with_depth += 1;
                self.with_scope_depths.push(self.names.len());
                let result = self.statement(body, false);
                self.with_scope_depths
                    .pop()
                    .expect("compiler balances with scopes");
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
                match public_field_definition(statement) {
                    // DefineField: a public instance field is created with
                    // CreateDataPropertyOrThrow, never with [[Set]] -- it
                    // shadows an inherited setter and reaches [[DefineOwnProperty]]
                    // (a Proxy trap, a deferred namespace, ...).
                    Some((key, computed, value)) => {
                        self.expression(&Expr::This)?;
                        match (key, computed) {
                            (Expr::Identifier(name), false) => {
                                self.constant(Value::String(name.as_str().into()))?
                            }
                            (key, _) => self.expression(key)?,
                        }
                        self.expression(value)?;
                        self.emit(Opcode::DefineInstanceField, 0)?;
                    }
                    None => self.statement(statement, declarations_allowed)?,
                }
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
                if let Some(value) = value.as_ref().filter(|_| self.bytecode.strict) {
                    if self.tail_position_return(value)? {
                        return Ok(());
                    }
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
                self.enter_scope(
                    block_lexical_names(body, self.bytecode.strict)?,
                    &var_names(body)?,
                    false,
                )?;
                self.statements_with_disposal(body)?;
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
            } => {
                // `using`/`await using` in a C-style for-head disposes once,
                // when the whole ForStatement completes (confirmed against
                // `initializer-disposed-at-end-of-forstatement.js`'s own
                // naming) -- unlike the ForOf `using ForBinding` production
                // in `for_each`, which disposes each iteration's own
                // binding at the end of *that* iteration.
                let is_async_using =
                    matches!(init, Some(ForInit::VarDecl(DeclKind::AwaitUsing, _)));
                if is_async_using || matches!(init, Some(ForInit::VarDecl(DeclKind::Using, _))) {
                    self.wrap_with_disposal(is_async_using, |this| {
                        this.loop_statement(
                            init.as_ref(),
                            test.as_ref(),
                            update.as_ref(),
                            body,
                            false,
                            Vec::new(),
                        )
                    })?;
                } else {
                    self.loop_statement(
                        init.as_ref(),
                        test.as_ref(),
                        update.as_ref(),
                        body,
                        false,
                        Vec::new(),
                    )?;
                }
            }
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

    /// Closes the iterators of the loops being left, then returns the value
    /// on top of the stack.
    fn emit_return_epilogue(&mut self) -> Result<(), CompileError> {
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
        Ok(())
    }

    /// Compiles `return value` when `value` contains a self tail call in a
    /// tail position: the call itself, or through the branches of `?:`, the
    /// right operand of `&&`/`||`/`??`, the last operand of a comma
    /// expression, or parentheses (§15.10.2 HasCallInTailPosition). Every
    /// path ends in a `TailRecur` or a `Return`. `Ok(false)` means `value`
    /// has no such call and nothing was emitted.
    fn tail_position_return(&mut self, value: &Expr) -> Result<bool, CompileError> {
        if self.tail_call_blockers != 0 || !self.contains_tail_call(value) {
            return Ok(false);
        }
        match value {
            Expr::Parenthesized(inner) => return self.tail_position_return(inner),
            Expr::Conditional {
                test,
                consequent,
                alternate,
            } => {
                self.expression(test)?;
                let no = self.emit(Opcode::JumpIfFalse, 0)?;
                self.tail_position_return_or_value(consequent)?;
                self.patch(no, self.offset()?);
                self.tail_position_return_or_value(alternate)?;
            }
            Expr::Logical { op, left, right } => {
                self.expression(left)?;
                self.emit(Opcode::Dup, 0)?;
                let short_circuit = self.emit(
                    match op {
                        LogicalOp::And => Opcode::JumpIfFalse,
                        LogicalOp::Or => Opcode::JumpIfTrue,
                        LogicalOp::Nullish => Opcode::JumpIfNotNullish,
                    },
                    0,
                )?;
                self.emit(Opcode::Pop, 0)?;
                self.tail_position_return_or_value(right)?;
                self.patch(short_circuit, self.offset()?);
                self.emit_return_epilogue()?;
            }
            Expr::Sequence(expressions) => {
                let (last, rest) = expressions
                    .split_last()
                    .expect("a sequence expression has operands");
                for expression in rest {
                    self.expression(expression)?;
                    self.emit(Opcode::Pop, 0)?;
                }
                self.tail_position_return_or_value(last)?;
            }
            call if self.self_tail_call_args(call).is_none() => {
                // A call to any other function: `TailCall` replaces this
                // frame with the callee's, or (when this frame cannot be
                // replaced, e.g. a constructor) calls it and falls through to
                // the ordinary `Return`.
                self.tail_call_pending = true;
                self.expression(call)?;
                debug_assert!(!self.tail_call_pending, "the call consumed the flag");
                self.tail_call_pending = false;
                self.emit(Opcode::Return, 0)?;
            }
            call => {
                let args = self
                    .self_tail_call_args(call)
                    .expect("contains_tail_call found a self tail call");
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
            }
        }
        Ok(true)
    }

    /// One operand of a tail position: another tail position when it holds a
    /// self tail call, otherwise an ordinary `return operand`.
    fn tail_position_return_or_value(&mut self, value: &Expr) -> Result<(), CompileError> {
        if !self.tail_position_return(value)? {
            self.expression(value)?;
            self.emit_return_epilogue()?;
        }
        Ok(())
    }

    /// Whether `value`, read as a tail position, holds a call this function
    /// can compile as a tail call.
    fn contains_tail_call(&self, value: &Expr) -> bool {
        match value {
            Expr::Parenthesized(inner) => self.contains_tail_call(inner),
            Expr::Conditional {
                consequent,
                alternate,
                ..
            } => self.contains_tail_call(consequent) || self.contains_tail_call(alternate),
            Expr::Logical { right, .. } => self.contains_tail_call(right),
            Expr::Sequence(expressions) => expressions
                .last()
                .is_some_and(|last| self.contains_tail_call(last)),
            call => self.self_tail_call_args(call).is_some() || self.is_general_tail_call(call),
        }
    }

    /// A call that can replace this frame (§15.10.2): a plain call or tagged
    /// template made from strict, non-generator, non-async function code that
    /// is not a class constructor. Spread arguments, `super(...)` calls and
    /// optional chains keep the ordinary call path, and so does a call inside
    /// a loop that owns an iterator: closing it after the call would need the
    /// frame this call replaces.
    fn is_general_tail_call(&self, call: &Expr) -> bool {
        if !self.function
            || !self.bytecode.strict
            || self.bytecode.generator
            || self.bytecode.async_function
            || self.bytecode.class_constructor
            || self.loops.iter().any(|context| context.iterator.is_some())
        {
            return false;
        }
        match call {
            Expr::Call { callee, args } => {
                !matches!(&**callee, Expr::Super)
                    && args
                        .iter()
                        .all(|argument| matches!(argument, Argument::Normal(_)))
            }
            Expr::TaggedTemplate { .. } => true,
            _ => false,
        }
    }

    /// Annex B.3.3 parses a sloppy FunctionDeclaration in an `if` clause as
    /// a synthetic block whose lexical function binding is then copied to the
    /// Annex B outer var binding when that clause executes.
    pub(super) fn if_clause_statement(&mut self, statement: &Stmt) -> Result<(), CompileError> {
        if !self.bytecode.strict
            && matches!(statement, Stmt::FunctionDecl(function)
                if !function.generator && !function.is_async)
        {
            self.enter_scope(
                block_lexical_names(std::slice::from_ref(statement), self.bytecode.strict)?,
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
            // A handler crossed on the way to this cleanup gateway closes
            // iterators inside it before it runs `finally`, then unwinds the
            // corresponding lexical scope. This operation closes an iterator
            // that is still live, while making that already-cleaned path a
            // no-op instead of reading an inactive `*iterator*` binding.
            self.emit(Opcode::CloseIteratorBinding, iterator)?;
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
            block_lexical_names(statements, self.bytecode.strict)?,
            &var_names(statements)?,
            false,
        )?;
        self.statements_with_disposal(statements)?;
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
        let lexical = switch_lexical_names(cases, self.bytecode.strict)?;
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
            finally_end: None,
        });
        self.emit(Opcode::PushHandler, handler_index)?;

        // Each TryBlock has its own Completion. An empty block must not leak
        // the value of the preceding statement into TryStatement's
        // UpdateEmpty step.
        self.emit(Opcode::ClearCompletion, 0)?;
        self.bytecode.handlers[handler_index as usize].try_start = self.offset()?;
        // A call in the try block is not a tail call: the catch and finally
        // clauses have to observe how it ends.
        self.tail_call_blockers += 1;
        let try_block = self.scoped_statements(block);
        self.tail_call_blockers -= 1;
        try_block?;
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
                block_lexical_names(&catch.body, self.bytecode.strict)?,
                &var_names(&catch.body)?,
                false,
            )?;
            // With a finally clause the catch block's call is not a tail
            // call either: the finalizer runs after it returns.
            let blocked = u32::from(finalizer.is_some());
            self.tail_call_blockers += blocked;
            let catch_body = self.statements_with_disposal(&catch.body);
            self.tail_call_blockers -= blocked;
            catch_body?;
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
            self.bytecode.handlers[handler_index as usize].finally_end = Some(end);
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
        let is_using = matches!(kind, DeclKind::Using | DeclKind::AwaitUsing);
        for declaration in declarations {
            if kind == DeclKind::Const && declaration.init.is_none() {
                return Err(CompileError::InvalidSyntax("const requires an initializer"));
            }
            if is_using {
                if !matches!(declaration.pattern, Pattern::Identifier(_)) {
                    return Err(CompileError::InvalidSyntax(
                        "a using declaration cannot use a destructuring pattern",
                    ));
                }
                if declaration.init.is_none() {
                    return Err(CompileError::InvalidSyntax(
                        "a using declaration requires an initializer",
                    ));
                }
            }
            if matches!(declaration.pattern, Pattern::Identifier(_))
                && kind == DeclKind::Var
                && declaration.init.is_none()
            {
                continue;
            }
            // `var name = init` inside `with`: ResolveBinding(name) runs before
            // the initializer, so a with object that has the property gets the
            // assignment and the function-level binding stays untouched.
            if let (DeclKind::Var, Pattern::Identifier(name), Some(value)) =
                (kind, &declaration.pattern, &declaration.init)
            {
                if self.with_depth != 0 && self.resolve_inside_innermost_with(name).is_none() {
                    let index = self.name_constant(name)?;
                    self.emit(Opcode::ResolveWithReference, index)?;
                    self.expression_with_name(value, Some(name.as_str()))?;
                    self.emit(Opcode::StoreWithReference, 0)?;
                    self.emit(Opcode::Pop, 0)?;
                    continue;
                }
            }
            if let Some(value) = &declaration.init {
                // "IsAnonymousFunctionDefinition(Initializer)" NamedEvaluation
                // applies to any `var`/`let`/`const`/`using`/`await using`
                // declarator whose target is a single BindingIdentifier --
                // not just a module's default-export binding.
                let inferred_name = match &declaration.pattern {
                    Pattern::Identifier(name)
                        if self.bytecode.module && name == MODULE_DEFAULT_BINDING =>
                    {
                        Some("default")
                    }
                    Pattern::Identifier(name) => Some(name.as_str()),
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
            if is_using {
                // BindingInitialization happens before AddDisposableResource
                // observes the value (`using` LexicalBinding Evaluation);
                // keep a second copy on the stack for it.
                self.emit(Opcode::Dup, 0)?;
            }
            self.bind_pattern(&declaration.pattern, kind)?;
            if is_using {
                let hint = u32::from(kind == DeclKind::AwaitUsing);
                self.emit(Opcode::AddDisposableResource, hint)?;
            }
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
                    let resolved = self
                        .catch_var_slots
                        .iter()
                        .rev()
                        .find_map(|slots| slots.get(name))
                        .copied()
                        .or_else(|| self.names[self.local_scope].get(name).copied())
                        .or_else(|| self.resolve(name));
                    let Some(slot) = resolved else {
                        // A sloppy direct eval does not re-create a `var` that
                        // an earlier eval already added to the function's
                        // VariableEnvironment: it has no static slot, and the
                        // initializer assigns to that dynamic binding.
                        let index = self.name_constant(name)?;
                        self.emit(Opcode::SetUnboundName, index)?;
                        self.emit(Opcode::Pop, 0)?;
                        return Ok(());
                    };
                    slot
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
        // CreatePerIterationEnvironment also runs once before the first test
        // (ForBodyEvaluation step 2): closures made by the initializer keep
        // the initial bindings, which the loop never writes again.
        if own_scope {
            let scope = *self
                .scopes
                .last()
                .expect("lexical for scope remains active");
            self.emit(Opcode::CloneScope, scope)?;
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

/// `IsLabelledFunction`: a label (or chain of labels) on a function
/// declaration.
fn is_labelled_function(statement: &Stmt) -> bool {
    let mut item = statement;
    while let Stmt::Labelled { item: inner, .. } = item {
        item = inner;
    }
    matches!(item, Stmt::FunctionDecl(_))
}
