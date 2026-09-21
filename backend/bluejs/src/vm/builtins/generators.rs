// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

/// How a synchronous `yield*` delegate answered `next`, `throw` or `return`.
enum SyncDelegateStep {
    /// The delegate is not finished: its own result object is yielded as is.
    Yield(Value),
    /// The delegate finished with this value.
    Done(Value),
}

impl Vm {
    /// Generator function invocation performs parameter initialization now,
    /// then suspends immediately before body evaluation. This makes a direct
    /// eval in a default parameter observable (including its early errors)
    /// at `generatorFunction()` rather than at the first `.next()`.
    pub(in super::super) fn initialize_generator(
        &mut self,
        code: Rc<Bytecode>,
        captures: Vec<ObjectId>,
        callee: Value,
        receiver: Value,
        args: Vec<Value>,
        home: Option<ObjectId>,
    ) -> Result<GeneratorState, RuntimeError> {
        self.stack.push(receiver.clone());
        let base = self.stack.len();
        self.stack.extend(self.bindings.iter().flatten().cloned());
        self.stack
            .extend(self.cells.values().copied().map(Value::Object));
        self.stack.push(self.completion.clone());
        self.stack.push(self.this.clone());
        self.stack.extend(self.arguments.iter().cloned());
        let frame_base = self.stack.len();

        let mut frame_bindings = vec![None; code.bindings.len()];
        if let Some(slot) = code.self_slot {
            frame_bindings[slot as usize] = Some(callee.clone());
        }
        let bindings = std::mem::replace(&mut self.bindings, frame_bindings);
        let binding_metadata = std::mem::replace(&mut self.binding_metadata, code.bindings.clone());
        let cells = std::mem::replace(&mut self.cells, captures.into_iter().enumerate().collect());
        let dynamic_eval_bindings = std::mem::take(&mut self.dynamic_eval_bindings);
        let dynamic_eval_outer_bindings = std::mem::take(&mut self.dynamic_eval_outer_bindings);
        let eval_dynamic_slots = std::mem::take(&mut self.eval_dynamic_slots);
        let script_global_slots = std::mem::take(&mut self.script_global_slots);
        let this = std::mem::replace(&mut self.this, receiver);
        let arguments = std::mem::replace(&mut self.arguments, args);
        let frame_callee = std::mem::replace(&mut self.callee, callee);
        let completion = std::mem::replace(&mut self.completion, Value::Undefined);
        let completion_empty = std::mem::replace(&mut self.completion_empty, true);
        let active_scopes = std::mem::take(&mut self.active_scopes);
        let active_scope_slots = std::mem::take(&mut self.active_scope_slots);
        let strict = std::mem::replace(&mut self.strict, code.strict);
        let home_object = std::mem::replace(&mut self.home_object, home);
        let variable_scope = std::mem::replace(&mut self.variable_scope, code.variable_scope);
        let variable_scope_lexicals = std::mem::replace(
            &mut self.variable_scope_lexicals,
            code.scopes
                .get(code.variable_scope as usize)
                .into_iter()
                .flat_map(|scope| scope.iter())
                .filter_map(|slot| {
                    let binding = &code.bindings[*slot as usize];
                    binding.lexical.then(|| binding.name.clone())
                })
                .collect(),
        );

        let mut iterators = Vec::new();
        let outcome = self.interpret(
            &code,
            &mut iterators,
            0,
            None,
            Some(code.generator_entry as usize),
            None,
        );
        let state = match outcome {
            Ok(InterpreterExit::Suspend { pc }) => {
                debug_assert_eq!(pc, code.generator_entry as usize);
                let stack = self.stack.split_off(frame_base);
                Ok(GeneratorState::Suspended {
                    code,
                    pc,
                    stack,
                    bindings: std::mem::take(&mut self.bindings),
                    cells: std::mem::take(&mut self.cells).into_iter().collect(),
                    this: std::mem::replace(&mut self.this, Value::Undefined),
                    args: std::mem::take(&mut self.arguments),
                    completion: std::mem::replace(&mut self.completion, Value::Undefined),
                    completion_empty: std::mem::replace(&mut self.completion_empty, true),
                    active_scopes: std::mem::take(&mut self.active_scopes),
                    iterators,
                    handlers: Vec::new(),
                    pending_completions: Vec::new(),
                    completion_saves: Vec::new(),
                    async_delegate: None,
                    delegate: None,
                    dynamic_bindings: std::mem::take(&mut self.dynamic_eval_bindings)
                        .into_iter()
                        .map(|(name, binding)| (name, binding.cell, binding.shadowed_cells))
                        .collect(),
                    home: std::mem::take(&mut self.home_object),
                    callee: std::mem::replace(&mut self.callee, Value::Undefined),
                })
            }
            Ok(InterpreterExit::Return(_)) | Ok(InterpreterExit::Yield { .. }) => {
                unreachable!("generator entry contains only instantiation bytecode")
            }
            Ok(InterpreterExit::Await { .. }) => {
                unreachable!("generator entry cannot contain top-level await")
            }
            Err(error) => Err(error),
        };

        self.bindings = bindings;
        self.binding_metadata = binding_metadata;
        self.cells = cells;
        self.dynamic_eval_bindings = dynamic_eval_bindings;
        self.dynamic_eval_outer_bindings = dynamic_eval_outer_bindings;
        self.eval_dynamic_slots = eval_dynamic_slots;
        self.script_global_slots = script_global_slots;
        self.this = this;
        self.arguments = arguments;
        self.callee = frame_callee;
        self.completion = completion;
        self.completion_empty = completion_empty;
        self.active_scopes = active_scopes;
        self.active_scope_slots = active_scope_slots;
        self.strict = strict;
        self.home_object = home_object;
        self.variable_scope = variable_scope;
        self.variable_scope_lexicals = variable_scope_lexicals;
        self.stack.truncate(base - 1);
        state
    }

    /// GeneratorValidate's brand check: the receiver of `next`, `return` and
    /// `throw` must be a generator object.
    pub(in super::super) fn generator_validate(
        &self,
        receiver: &Value,
    ) -> Result<(), RuntimeError> {
        match receiver {
            Value::Object(id) if self.heap.is_generator(*id)? => Ok(()),
            _ => Err(RuntimeError::TypeError(
                "Generator method called on an incompatible receiver".into(),
            )),
        }
    }

    pub(in super::super) fn generator_next(
        &mut self,
        receiver: &Value,
        value: Option<Value>,
        async_target: Option<ObjectId>,
    ) -> Result<Value, RuntimeError> {
        self.generator_resume(receiver, value, async_target, None)
    }

    /// Resume a generator either normally after `yield`, or with an abrupt
    /// completion supplied by `.return()` / `.throw()`.  The latter must pass
    /// through saved catch and finally records instead of closing the frame
    /// at the builtin boundary.
    pub(in super::super) fn generator_resume(
        &mut self,
        receiver: &Value,
        value: Option<Value>,
        async_target: Option<ObjectId>,
        abrupt: Option<Completion>,
    ) -> Result<Value, RuntimeError> {
        let Value::Object(generator) = receiver else {
            return Err(RuntimeError::TypeError(
                "Generator next requires a generator".into(),
            ));
        };
        let state = self.heap.take_generator_state(*generator)?;
        let (
            code,
            pc,
            resume_value,
            frame_stack,
            frame_bindings,
            frame_cells,
            frame_this,
            frame_args,
            frame_completion,
            frame_completion_empty,
            frame_scopes,
            frame_iterators,
            frame_handlers,
            frame_pending_completions,
            frame_completion_saves,
            frame_home,
            frame_callee,
            frame_variable_scope,
            frame_variable_scope_lexicals,
            frame_dynamic_bindings,
        ) = match state {
            GeneratorState::Running => {
                self.heap
                    .set_generator_state(*generator, GeneratorState::Running)?;
                return Err(RuntimeError::TypeError(
                    "Generator is already running".into(),
                ));
            }
            GeneratorState::Done => {
                self.heap
                    .set_generator_state(*generator, GeneratorState::Done)?;
                if let Some(Completion::Throw(error)) = abrupt {
                    return Err(error);
                }
                if let Some(Completion::Return(value)) = abrupt {
                    return self.iterator_result(value, true);
                }
                return self.iterator_result(Value::Undefined, true);
            }
            GeneratorState::Start {
                code,
                captures,
                callee,
                receiver,
                args,
                home,
            } => {
                let mut bindings = vec![None; code.bindings.len()];
                let variable_scope = code.variable_scope;
                let variable_scope_lexicals = code
                    .scopes
                    .get(variable_scope as usize)
                    .into_iter()
                    .flat_map(|scope| scope.iter())
                    .filter_map(|slot| {
                        let binding = &code.bindings[*slot as usize];
                        binding.lexical.then(|| binding.name.clone())
                    })
                    .collect();
                if let Some(slot) = code.self_slot {
                    bindings[slot as usize] = Some(callee.clone());
                }
                (
                    code,
                    0,
                    None,
                    Vec::new(),
                    bindings,
                    captures.into_iter().enumerate().collect(),
                    receiver,
                    args,
                    Value::Undefined,
                    true,
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    home,
                    callee,
                    variable_scope,
                    variable_scope_lexicals,
                    HashMap::new(),
                )
            }
            GeneratorState::Suspended {
                code,
                pc,
                stack,
                bindings,
                cells,
                this,
                args,
                completion,
                completion_empty,
                active_scopes,
                iterators,
                handlers,
                pending_completions,
                completion_saves,
                async_delegate: _,
                delegate: _,
                dynamic_bindings,
                home,
                callee,
            } => {
                let variable_scope = code.variable_scope;
                let variable_scope_lexicals = code
                    .scopes
                    .get(variable_scope as usize)
                    .into_iter()
                    .flat_map(|scope| scope.iter())
                    .filter_map(|slot| {
                        let binding = &code.bindings[*slot as usize];
                        binding.lexical.then(|| binding.name.clone())
                    })
                    .collect();
                (
                    code,
                    pc,
                    value,
                    stack,
                    bindings,
                    cells.into_iter().collect(),
                    this,
                    args,
                    completion,
                    completion_empty,
                    active_scopes,
                    iterators,
                    handlers,
                    pending_completions,
                    completion_saves,
                    home,
                    callee,
                    variable_scope,
                    variable_scope_lexicals,
                    dynamic_bindings
                        .into_iter()
                        .map(|(name, cell, shadowed_cells)| {
                            (
                                name,
                                DynamicEvalBinding {
                                    cell,
                                    shadowed_cells,
                                },
                            )
                        })
                        .collect(),
                )
            }
        };
        self.heap
            .set_generator_state(*generator, GeneratorState::Running)?;

        let base = self.stack.len();
        let remaining_instructions = self.remaining_instructions;
        self.stack.extend(self.bindings.iter().flatten().cloned());
        self.stack
            .extend(self.cells.values().copied().map(Value::Object));
        self.stack.push(self.completion.clone());
        self.stack.push(self.this.clone());
        self.stack.extend(self.arguments.iter().cloned());
        let frame_base = self.stack.len();
        self.stack.extend(frame_stack.iter().cloned());

        let bindings = std::mem::replace(&mut self.bindings, frame_bindings);
        let binding_metadata = std::mem::replace(&mut self.binding_metadata, code.bindings.clone());
        let cells = std::mem::replace(&mut self.cells, frame_cells);
        let dynamic_eval_outer_bindings = std::mem::take(&mut self.dynamic_eval_outer_bindings);
        let dynamic_eval_bindings =
            std::mem::replace(&mut self.dynamic_eval_bindings, frame_dynamic_bindings);
        let eval_dynamic_slots = std::mem::take(&mut self.eval_dynamic_slots);
        // The caller's script-level slot -> global-property map names slots of
        // *its* frame; a resumed generator body has its own slot numbering.
        let script_global_slots = std::mem::take(&mut self.script_global_slots);
        let this = std::mem::replace(&mut self.this, frame_this);
        let arguments = std::mem::replace(&mut self.arguments, frame_args);
        let completion = std::mem::replace(&mut self.completion, frame_completion);
        let completion_empty =
            std::mem::replace(&mut self.completion_empty, frame_completion_empty);
        let active_scopes = std::mem::replace(&mut self.active_scopes, frame_scopes);
        let active_scope_slots = std::mem::replace(
            &mut self.active_scope_slots,
            self.active_scopes
                .iter()
                .map(|scope| code.scopes[*scope as usize].clone())
                .collect(),
        );
        let strict = std::mem::replace(&mut self.strict, code.strict);
        let home_object = std::mem::replace(&mut self.home_object, frame_home);
        let callee = std::mem::replace(&mut self.callee, frame_callee);
        // A generator body resumes wherever its request happened to come
        // from -- typically a Promise job with no ambient script/module
        // identity -- so an `import()` inside it must resolve against the
        // module or script that created the generator function, exactly as
        // an ordinary call of a module closure does.
        let referrer = self
            .callee
            .object_id()
            .and_then(|id| self.module_closure_referrers.get(&id).cloned());
        let active_module_name = match referrer {
            Some(module) => self.active_module_name.replace(module),
            None => self.active_module_name.clone(),
        };
        let variable_scope = std::mem::replace(&mut self.variable_scope, frame_variable_scope);
        let variable_scope_lexicals = std::mem::replace(
            &mut self.variable_scope_lexicals,
            frame_variable_scope_lexicals,
        );
        let pending_completions = std::mem::replace(
            &mut self.pending_completions,
            frame_pending_completions
                .into_iter()
                .map(Completion::from_generator_pending)
                .collect(),
        );
        let completion_saves =
            std::mem::replace(&mut self.completion_saves, frame_completion_saves);
        let mut iterators = frame_iterators;
        let mut handlers = frame_handlers;
        let outcome = if let Some(completion) = abrupt {
            // `GeneratorState` keeps handler offsets relative to its saved
            // operand stack. `resolve_completion` runs before `interpret`,
            // so inflate them here, then return them to their stored form
            // before the resumed interpreter takes ownership.
            for handler in &mut handlers {
                handler.stack_depth += frame_base;
            }
            let action = self.resolve_completion(&code, &mut handlers, &mut iterators, completion);
            for handler in &mut handlers {
                handler.stack_depth -= frame_base;
            }
            match action {
                Ok(CompletionAction::Continue) => self.interpret(
                    &code,
                    &mut iterators,
                    pc,
                    None,
                    None,
                    Some((handlers, frame_base)),
                ),
                Ok(CompletionAction::Jump(target)) => self.interpret(
                    &code,
                    &mut iterators,
                    target,
                    None,
                    None,
                    Some((handlers, frame_base)),
                ),
                Ok(CompletionAction::Return(value)) => Ok(InterpreterExit::Return(value)),
                Ok(CompletionAction::TailRecur(_) | CompletionAction::TailCall(_)) => {
                    Err(RuntimeError::TypeError(
                        "generator cannot tail recur across an abrupt resume".into(),
                    ))
                }
                Ok(CompletionAction::Throw(error)) | Err(error) => Err(error),
            }
        } else {
            self.interpret(
                &code,
                &mut iterators,
                pc,
                resume_value,
                None,
                Some((handlers, frame_base)),
            )
        };
        let mut suspended_async = None;
        // A `yield*` over a synchronous delegate yields the delegate's own
        // result object, which is returned without wrapping it again.
        let mut yielded_delegate_result = false;

        let (next_state, result) = match outcome {
            Ok(InterpreterExit::Return(value)) => {
                // An injected generator return skips the compiler's normal
                // IteratorFinish/IteratorClose instructions. Keep the
                // completion and every active record rooted while close
                // callbacks can allocate, then discard the frame as done.
                let close_base = self.stack.len();
                self.stack.push(value.clone());
                self.stack.extend(iterators.iter().cloned());
                let close = self.close_iterators_for_return(&mut iterators, 0);
                self.stack.truncate(close_base);
                self.stack.truncate(frame_base);
                (GeneratorState::Done, close.map(|()| (value, true)))
            }
            Ok(InterpreterExit::Yield {
                value,
                pc,
                iterators,
                handlers,
            }) => {
                let stack = self.stack.split_off(frame_base);
                let async_delegate = code
                    .async_yield_delegates
                    .iter()
                    .find(|(resume, _)| *resume as usize == pc)
                    .and_then(|(_, exit_pc)| {
                        stack.last().cloned().map(|record| AsyncGeneratorDelegate {
                            record,
                            exit_pc: *exit_pc as usize,
                        })
                    });
                let delegate = code
                    .yield_delegates
                    .iter()
                    .find(|(resume, _)| *resume as usize == pc)
                    .and_then(|(_, exit_pc)| {
                        stack
                            .last()
                            .cloned()
                            .map(|record| crate::heap::GeneratorDelegate {
                                record,
                                exit_pc: *exit_pc as usize,
                            })
                    });
                yielded_delegate_result = delegate.is_some();
                let state = GeneratorState::Suspended {
                    code,
                    pc,
                    stack,
                    bindings: std::mem::take(&mut self.bindings),
                    cells: std::mem::take(&mut self.cells).into_iter().collect(),
                    this: std::mem::replace(&mut self.this, Value::Undefined),
                    args: std::mem::take(&mut self.arguments),
                    completion: std::mem::replace(&mut self.completion, Value::Undefined),
                    completion_empty: std::mem::replace(&mut self.completion_empty, true),
                    active_scopes: std::mem::take(&mut self.active_scopes),
                    iterators,
                    handlers,
                    pending_completions: std::mem::take(&mut self.pending_completions)
                        .into_iter()
                        .map(|completion| {
                            completion
                                .into_generator_pending()
                                .expect("only catchable completions can survive a generator yield")
                        })
                        .collect(),
                    completion_saves: std::mem::take(&mut self.completion_saves),
                    async_delegate,
                    delegate,
                    dynamic_bindings: std::mem::take(&mut self.dynamic_eval_bindings)
                        .into_iter()
                        .map(|(name, binding)| (name, binding.cell, binding.shadowed_cells))
                        .collect(),
                    home: std::mem::take(&mut self.home_object),
                    callee: std::mem::replace(&mut self.callee, Value::Undefined),
                };
                (state, Ok((value, false)))
            }
            Err(error) => {
                // The same applies when a resumed generator completes
                // abruptly without a remaining handler. In particular, a
                // return()/throw() request must close a destructuring
                // iterator that was live at the preceding yield.
                let close_base = self.stack.len();
                if let RuntimeError::Thrown(value) = &error {
                    self.stack.push(value.clone());
                }
                self.stack.extend(iterators.iter().cloned());
                self.close_iterators_to(&mut iterators, 0);
                self.stack.truncate(close_base);
                self.stack.truncate(frame_base);
                (GeneratorState::Done, Err(error))
            }
            Ok(InterpreterExit::Suspend { .. }) => {
                unreachable!("ordinary generator execution has no suspend boundary")
            }
            Ok(InterpreterExit::Await {
                promise,
                pc,
                handlers,
            }) => {
                if let Some(target) = async_target {
                    let stack = self.stack.split_off(frame_base);
                    let mut execution = self.suspend_module_execution();
                    let parent_stack = std::mem::replace(&mut execution.stack, stack);
                    self.stack = parent_stack;
                    let templates = execution.templates.clone();
                    self.templates = templates;
                    suspended_async = Some((
                        AsyncContinuation {
                            generator: Some(*generator),
                            target,
                            code: code.clone(),
                            pc,
                            execution,
                            iterators,
                            handlers,
                            call_depth: self.call_depth,
                        },
                        promise,
                    ));
                    (GeneratorState::Done, Ok((Value::Undefined, true)))
                } else {
                    self.stack.truncate(frame_base);
                    (
                        GeneratorState::Done,
                        Err(RuntimeError::TypeError(
                            "await requires an async generator function".into(),
                        )),
                    )
                }
            }
        };
        // Storing the suspended frame grows the generator's managed bytes and
        // can trigger a major collection. Everything this call still holds
        // outside the heap (the caller's frame copied onto the stack above,
        // and the value about to be yielded or thrown) must be visible to it.
        match &result {
            Ok((value, _)) | Err(RuntimeError::Thrown(value)) => self.stack.push(value.clone()),
            Err(_) => {}
        }
        let stored = self.with_roots(|heap| heap.set_generator_state(*generator, next_state));
        // A failed store must still hand the caller's frame back: leaving the
        // swapped-out execution state in place would unbalance the interpreter
        // (for example an empty `dynamic_eval_outer_bindings` stack) on the
        // very next call return.
        self.bindings = bindings;
        self.binding_metadata = binding_metadata;
        self.cells = cells;
        self.dynamic_eval_bindings = dynamic_eval_bindings;
        self.dynamic_eval_outer_bindings = dynamic_eval_outer_bindings;
        self.eval_dynamic_slots = eval_dynamic_slots;
        self.script_global_slots = script_global_slots;
        self.this = this;
        self.arguments = arguments;
        self.completion = completion;
        self.completion_empty = completion_empty;
        self.pending_completions = pending_completions;
        self.completion_saves = completion_saves;
        self.active_scopes = active_scopes;
        self.active_scope_slots = active_scope_slots;
        self.strict = strict;
        self.home_object = home_object;
        self.callee = callee;
        self.variable_scope = variable_scope;
        self.variable_scope_lexicals = variable_scope_lexicals;
        self.remaining_instructions = remaining_instructions;
        self.active_module_name = active_module_name;
        self.stack.truncate(base);
        stored?;
        if let Some((state, promise)) = suspended_async {
            self.suspend_async_await(state, promise)?;
            return Ok(Value::Undefined);
        }
        let (value, done) = result?;
        if yielded_delegate_result {
            return Ok(value);
        }
        self.iterator_result(value, done)
    }

    pub(in super::super) fn generator_return(
        &mut self,
        receiver: &Value,
        value: Value,
    ) -> Result<Value, RuntimeError> {
        if let Some(result) = self.generator_delegate_return(receiver, value.clone())? {
            return result;
        }
        self.generator_resume(receiver, None, None, Some(Completion::Return(value)))
    }

    /// Returns the compiler-declared synchronous `yield*` state at the public
    /// yield boundary. The iterator record lives in the saved operand stack.
    pub(in super::super) fn sync_yield_star_delegate(
        state: &GeneratorState,
    ) -> Option<(Value, usize)> {
        let GeneratorState::Suspended {
            delegate: Some(delegate),
            ..
        } = state
        else {
            return None;
        };
        Some((delegate.record.clone(), delegate.exit_pc))
    }

    /// Classifies the result a synchronous `yield*` delegate returned from
    /// `next`, `throw` or `return`: `IteratorComplete`, then `IteratorValue`
    /// only for a finished delegate. A live result is handed on unchanged (the
    /// spec's `GeneratorYield(innerResult)`), so its own `value` is never read
    /// and the caller of the outer generator sees the delegate's own object.
    fn sync_delegate_step(&mut self, result: Value) -> Result<SyncDelegateStep, RuntimeError> {
        if !matches!(result, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "yield* delegate method must return an object".into(),
            ));
        }
        self.stack.push(result.clone());
        let step = (|| {
            let done = self.get_property(&result, &"done".into())?;
            if self.to_boolean(&done)? {
                self.get_property(&result, &"value".into())
                    .map(SyncDelegateStep::Done)
            } else {
                Ok(SyncDelegateStep::Yield(result.clone()))
            }
        })();
        self.stack.pop();
        step
    }

    /// Completes the delegation after a finished delegate: the outer frame
    /// resumes after its compiler-recorded `yield*` loop with the delegate's
    /// final value as the expression's result, and the delegate is no longer
    /// an active iterator to close.
    fn finish_sync_delegation(
        &mut self,
        generator: ObjectId,
        value: Value,
    ) -> Result<(), RuntimeError> {
        let mut state = self.heap.take_generator_state(generator)?;
        let Some((record, exit)) = Self::sync_yield_star_delegate(&state) else {
            self.heap.set_generator_state(generator, state)?;
            return Err(RuntimeError::Unsupported(
                "lost synchronous yield* delegation state",
            ));
        };
        let GeneratorState::Suspended {
            pc,
            stack,
            delegate,
            iterators,
            ..
        } = &mut state
        else {
            unreachable!("yield* delegation is always suspended")
        };
        *pc = exit;
        *delegate = None;
        iterators.retain(|active| active != &record);
        *stack
            .last_mut()
            .expect("yield* delegation keeps its iterator record") = value;
        self.heap.set_generator_state(generator, state)?;
        if let Value::Object(record) = record {
            self.with_roots(|heap| heap.set(record, "done", Value::Bool(true)))?;
        }
        Ok(())
    }

    /// Forward an ordinary generator's `return()` through a suspended `yield*`
    /// delegate. `None` means that this is an ordinary yield boundary (or that
    /// the delegate has no `return` method). Every abrupt step of the delegate
    /// protocol is thrown inside the generator, where its own handlers can
    /// observe it, rather than out of the `return()` call.
    pub(in super::super) fn generator_delegate_return(
        &mut self,
        receiver: &Value,
        value: Value,
    ) -> Result<Option<Result<Value, RuntimeError>>, RuntimeError> {
        let generator = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError("Generator return requires a generator".into())
        })?;
        let state = self.heap.take_generator_state(generator)?;
        let Some((record, _)) = Self::sync_yield_star_delegate(&state) else {
            self.heap.set_generator_state(generator, state)?;
            return Ok(None);
        };
        self.heap.set_generator_state(generator, state)?;
        let base = self.stack.len();
        self.stack.push(value.clone());
        let step = (|| {
            let iterator = self.get_property(&record, &"iterator".into())?;
            self.stack.push(iterator.clone());
            let method = self.get_method(&iterator, &"return".into())?;
            if method == Value::Undefined {
                return Ok(None);
            }
            let result = self.call_native(method, iterator, vec![value.clone()], false)?;
            self.sync_delegate_step(result).map(Some)
        })();
        self.stack.truncate(base);
        Ok(match step {
            Ok(None) => {
                // The return completion leaves the generator directly, so
                // the frame must not close the delegate a second time.
                if let Value::Object(record) = record {
                    self.with_roots(|heap| heap.set(record, "done", Value::Bool(true)))?;
                }
                None
            }
            Ok(Some(SyncDelegateStep::Yield(result))) => Some(Ok(result)),
            Ok(Some(SyncDelegateStep::Done(inner))) => {
                self.stack.push(inner.clone());
                self.finish_sync_delegation(generator, inner.clone())?;
                let result =
                    self.generator_resume(receiver, None, None, Some(Completion::Return(inner)));
                self.stack.pop();
                Some(result)
            }
            Err(error) => Some(self.throw_into_generator(receiver, &record, error)),
        })
    }

    /// Resumes a suspended generator with the throw completion an abrupt step
    /// of its `yield*` delegate protocol produced. The delegate is not closed
    /// again by the unwinding frame, and the thrown value stays rooted while
    /// the generator's handlers run.
    fn throw_into_generator(
        &mut self,
        receiver: &Value,
        record: &Value,
        error: RuntimeError,
    ) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        self.root_thrown(&error);
        let result = (|| {
            if let Value::Object(record) = record {
                self.with_roots(|heap| heap.set(*record, "done", Value::Bool(true)))?;
            }
            self.generator_resume(receiver, None, None, Some(Completion::Throw(error)))
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn generator_throw(
        &mut self,
        receiver: &Value,
        value: Value,
    ) -> Result<Value, RuntimeError> {
        let generator = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError("Generator throw requires a generator".into())
        })?;
        let state = self.heap.take_generator_state(generator)?;
        let Some((record, _)) = Self::sync_yield_star_delegate(&state) else {
            self.heap.set_generator_state(generator, state)?;
            return self.generator_resume(
                receiver,
                None,
                None,
                Some(Completion::Throw(RuntimeError::Thrown(value))),
            );
        };
        self.heap.set_generator_state(generator, state)?;
        let base = self.stack.len();
        self.stack.push(value.clone());
        let step = (|| {
            let iterator = self.get_property(&record, &"iterator".into())?;
            self.stack.push(iterator.clone());
            let method = self.get_method(&iterator, &"throw".into())?;
            if method == Value::Undefined {
                // A delegate that cannot take the throw still gets to clean
                // up; the protocol violation is thrown once it has.
                self.iterator_close(&record)?;
                return Err(RuntimeError::TypeError(
                    "yield* iterator does not provide a throw method".into(),
                ));
            }
            let result = self.call_native(method, iterator, vec![value.clone()], false)?;
            self.sync_delegate_step(result)
        })();
        self.stack.truncate(base);
        match step {
            Ok(SyncDelegateStep::Yield(result)) => Ok(result),
            Ok(SyncDelegateStep::Done(inner)) => {
                self.stack.push(inner.clone());
                self.finish_sync_delegation(generator, inner)?;
                let result = self.generator_next(receiver, None, None);
                self.stack.pop();
                result
            }
            Err(error) => self.throw_into_generator(receiver, &record, error),
        }
    }

    /// Returns the explicit delegation record installed when the compiler's
    /// async `yield*` loop reached a public yield boundary. It is frame
    /// metadata, so forwarding never depends on recognizing bytecode layout.
    pub(in super::super) fn yield_star_delegate(state: &GeneratorState) -> Option<(Value, usize)> {
        let GeneratorState::Suspended {
            async_delegate: Some(delegate),
            ..
        } = state
        else {
            return None;
        };
        Some((delegate.record.clone(), delegate.exit_pc))
    }

    /// Starts forwarding an abrupt outer request into a suspended async
    /// `yield*` delegate. `None` means this is an ordinary generator yield
    /// and the caller must retain its standard return/throw behaviour.
    pub(in super::super) fn async_generator_delegate_request(
        &mut self,
        generator: ObjectId,
        target: ObjectId,
        kind: AsyncGeneratorDelegateKind,
        value: Value,
    ) -> Result<Option<Value>, RuntimeError> {
        let state = self.heap.take_generator_state(generator)?;
        let Some((record, _)) = Self::yield_star_delegate(&state) else {
            self.heap.set_generator_state(generator, state)?;
            return Ok(None);
        };
        self.heap.set_generator_state(generator, state)?;
        let Value::Object(record) = record else {
            unreachable!("yield* keeps an iterator record on its stack")
        };
        let iterator = self.get_property(&Value::Object(record), &"iterator".into())?;
        let name = match kind {
            AsyncGeneratorDelegateKind::Return => "return",
            AsyncGeneratorDelegateKind::Throw => "throw",
        };
        let method = self.get_method(&iterator, &name.into())?;
        if method == Value::Undefined {
            if matches!(kind, AsyncGeneratorDelegateKind::Throw) {
                self.close_async_generator(generator)?;
                return Err(RuntimeError::TypeError(
                    "yield* iterator does not provide a throw method".into(),
                ));
            }
            // GetMethod already observed the delegate's `return` property.
            // The abrupt completion still crosses the outer generator's
            // finally records.  `generator_resume` owns both that cleanup
            // and the completed-start special case.
            return self
                .generator_resume(
                    &Value::Object(generator),
                    None,
                    Some(target),
                    Some(Completion::Return(value)),
                )
                .map(Some);
        }
        let result = self.call_native(method, iterator, vec![value], false)?;
        let promise = self
            .promise_resolve(result)?
            .object_id()
            .expect("Promise.resolve returns a Promise");
        match self
            .promises
            .get(&promise)
            .expect("Promise.resolve registers its Promise")
            .status
            .clone_for_await()
        {
            PromiseAwaitStatus::Pending => self
                .promises
                .get_mut(&promise)
                .expect("checked pending Promise exists")
                .reactions
                .push(PromiseReaction::AsyncGeneratorDelegate {
                    generator,
                    target,
                    kind,
                }),
            PromiseAwaitStatus::Fulfilled(value) => {
                self.finish_async_generator_delegate(generator, target, kind, value, true)?
            }
            PromiseAwaitStatus::Rejected(value) => {
                self.finish_async_generator_delegate(generator, target, kind, value, false)?
            }
        }
        Ok(Some(Value::Undefined))
    }

    pub(in super::super) fn set_async_generator_status(
        &mut self,
        generator: ObjectId,
        status: AsyncGeneratorStatus,
    ) -> Result<(), RuntimeError> {
        let mut control = self
            .heap
            .async_generator_control(generator)?
            .ok_or_else(|| RuntimeError::TypeError("Async generator receiver required".into()))?;
        control.status = status;
        self.heap.set_async_generator_control(generator, control)?;
        Ok(())
    }

    /// Settles only the queue head. Completion moves the state back to a
    /// resumable boundary before the next request is considered, so a second
    /// `.next()` can never observe the `Done` placeholder used while an async
    /// continuation owns the frame.
    pub(in super::super) fn complete_async_generator_request(
        &mut self,
        generator: ObjectId,
        target: ObjectId,
        status: PromiseStatus,
    ) -> Result<(), RuntimeError> {
        let mut control = self
            .heap
            .async_generator_control(generator)?
            .ok_or_else(|| RuntimeError::TypeError("Async generator receiver required".into()))?;
        let request = control
            .requests
            .pop_front()
            .ok_or(RuntimeError::Unsupported("missing async generator request"))?;
        if request.target != target {
            return Err(RuntimeError::Unsupported(
                "async generator request completed out of order",
            ));
        }
        if control
            .requests
            .iter()
            .any(|queued| queued.id <= request.id)
        {
            return Err(RuntimeError::Unsupported(
                "async generator queue identifiers are not FIFO",
            ));
        }
        control.status = if self.heap.generator_state_is_done(generator)? {
            AsyncGeneratorStatus::Completed
        } else {
            AsyncGeneratorStatus::SuspendedYield
        };
        self.heap.set_async_generator_control(generator, control)?;
        self.settle_promise(target, status)
    }

    /// Implements the single `AsyncGeneratorResumeNext` scheduler. Only the
    /// queue head may enter the interpreter; an `Awaiting` generator keeps
    /// every later request as heap-owned data until its Promise job completes.
    pub(in super::super) fn resume_async_generator_next(
        &mut self,
        generator: ObjectId,
    ) -> Result<(), RuntimeError> {
        loop {
            let control = self
                .heap
                .async_generator_control(generator)?
                .ok_or_else(|| {
                    RuntimeError::TypeError("Async generator receiver required".into())
                })?;
            if control.requests.is_empty()
                || matches!(
                    control.status,
                    AsyncGeneratorStatus::Awaiting | AsyncGeneratorStatus::Executing
                )
            {
                return Ok(());
            }
            let request = control
                .requests
                .front()
                .cloned()
                .expect("checked async generator queue is non-empty");
            if control.status == AsyncGeneratorStatus::Completed {
                let completion = match request.completion {
                    AsyncGeneratorCompletion::Next(_) => {
                        PromiseStatus::Fulfilled(self.iterator_result(Value::Undefined, true)?)
                    }
                    AsyncGeneratorCompletion::Return(value) => {
                        // AsyncGeneratorAwaitReturn: even a completed
                        // generator awaits the value it is asked to return
                        // (a rejection or a broken `constructor` rejects
                        // this request), so it settles on a later turn.
                        let result = self.iterator_result(value, true)?;
                        return self.await_async_generator_yield(generator, request.target, result);
                    }
                    AsyncGeneratorCompletion::Throw(value) => PromiseStatus::Rejected(value),
                };
                self.complete_async_generator_request(generator, request.target, completion)?;
                continue;
            }

            self.set_async_generator_status(generator, AsyncGeneratorStatus::Executing)?;
            let receiver = Value::Object(generator);
            let result = match request.completion {
                AsyncGeneratorCompletion::Next(value) => {
                    self.generator_next(&receiver, Some(value), Some(request.target))
                }
                AsyncGeneratorCompletion::Return(value) => {
                    match self.async_generator_delegate_request(
                        generator,
                        request.target,
                        AsyncGeneratorDelegateKind::Return,
                        value.clone(),
                    ) {
                        Ok(Some(result)) => Ok(result),
                        Ok(None) => self.generator_resume(
                            &receiver,
                            None,
                            Some(request.target),
                            Some(Completion::Return(value)),
                        ),
                        Err(error) => Err(error),
                    }
                }
                AsyncGeneratorCompletion::Throw(value) => {
                    match self.async_generator_delegate_request(
                        generator,
                        request.target,
                        AsyncGeneratorDelegateKind::Throw,
                        value.clone(),
                    ) {
                        Ok(Some(result)) => Ok(result),
                        Ok(None) => self.generator_resume(
                            &receiver,
                            None,
                            Some(request.target),
                            Some(Completion::Throw(RuntimeError::Thrown(value))),
                        ),
                        Err(error) => Err(error),
                    }
                }
            };
            match result {
                // `generator_next` uses this private sentinel after moving
                // the body frame into an async continuation. Delegate awaits
                // use it too; in both cases the queue head remains pending.
                Ok(Value::Undefined)
                    if matches!(
                        self.promises
                            .get(&request.target)
                            .expect("queued async-generator target exists")
                            .status,
                        PromiseStatus::Pending
                    ) =>
                {
                    return self
                        .set_async_generator_status(generator, AsyncGeneratorStatus::Awaiting);
                }
                Ok(Value::Undefined) => return Ok(()),
                Ok(result) => {
                    return self.await_async_generator_yield(generator, request.target, result);
                }
                Err(error) => {
                    let value = self.error_value(error)?;
                    self.complete_async_generator_request(
                        generator,
                        request.target,
                        PromiseStatus::Rejected(value),
                    )?;
                }
            }
        }
    }

    /// Async generator methods append one request and return its capability.
    /// The scheduler is the only path that may enter the generator frame.
    pub(in super::super) fn async_generator_request(
        &mut self,
        receiver: &Value,
        value: Value,
        kind: NativeFunction,
    ) -> Result<Value, RuntimeError> {
        // AsyncGeneratorValidate failing is not a throw: the method returns a
        // promise rejected with the TypeError.
        let generator = match receiver.object_id() {
            // Anything but a live async generator (including any other kind of
            // heap object) fails the brand check.
            Some(generator)
                if matches!(self.heap.async_generator_control(generator), Ok(Some(_))) =>
            {
                generator
            }
            _ => {
                let error = self.error_object(
                    "TypeError",
                    "AsyncGenerator request requires an async generator".into(),
                )?;
                return self.promise_reject(error);
            }
        };
        let base = self.stack.len();
        self.stack.extend([receiver.clone(), value.clone()]);
        let result = (|| {
            let target = self.new_promise()?;
            self.stack.push(Value::Object(target));
            let mut control = self
                .heap
                .async_generator_control(generator)?
                .ok_or_else(|| {
                    RuntimeError::TypeError(
                        "AsyncGenerator request requires an async generator".into(),
                    )
                })?;
            let id = control.next_request_id;
            control.next_request_id =
                control
                    .next_request_id
                    .checked_add(1)
                    .ok_or(RuntimeError::RangeError(
                        "async generator request identifiers exhausted".into(),
                    ))?;
            let completion = match kind {
                NativeFunction::AsyncGeneratorNext => AsyncGeneratorCompletion::Next(value),
                NativeFunction::AsyncGeneratorReturn => AsyncGeneratorCompletion::Return(value),
                NativeFunction::AsyncGeneratorThrow => AsyncGeneratorCompletion::Throw(value),
                _ => unreachable!("only async generator request kinds reach this helper"),
            };
            control.requests.push_back(AsyncGeneratorRequest {
                id,
                completion,
                target,
            });
            self.heap.set_async_generator_control(generator, control)?;
            self.resume_async_generator_next(generator)?;
            Ok(Value::Object(target))
        })();
        self.stack.truncate(base);
        result
    }

    /// Implements the Await in AsyncGeneratorYield. The generator is already
    /// suspended at this point, so only its outstanding request capability
    /// waits; fulfillment writes the awaited value into its IteratorResult.
    pub(in super::super) fn await_async_generator_yield(
        &mut self,
        generator: ObjectId,
        target: ObjectId,
        result: Value,
    ) -> Result<(), RuntimeError> {
        let result = result
            .object_id()
            .expect("generator resumes always produce an IteratorResult object");
        let base = self.stack.len();
        self.stack.extend([
            Value::Object(generator),
            Value::Object(target),
            Value::Object(result),
        ]);
        let outcome = (|| {
            self.set_async_generator_status(generator, AsyncGeneratorStatus::Awaiting)?;
            let value = self.get_property(&Value::Object(result), &"value".into())?;
            self.stack.push(value.clone());
            // A value whose PromiseResolve throws (a hostile `constructor`
            // getter) is awaited as an already rejected promise.
            let awaited = match self.promise_resolve(value) {
                Ok(promise) => promise,
                Err(error) => {
                    let error = self.error_value(error)?;
                    self.promise_reject(error)?
                }
            };
            self.stack.push(awaited.clone());
            let awaited = awaited
                .object_id()
                .expect("Promise.resolve always returns a Promise");
            let status = self
                .promises
                .get(&awaited)
                .expect("Promise.resolve registers its Promise")
                .status
                .clone_for_await();
            match status {
                PromiseAwaitStatus::Pending => self
                    .promises
                    .get_mut(&awaited)
                    .expect("checked pending Promise exists")
                    .reactions
                    .push(PromiseReaction::AsyncGeneratorYield {
                        generator,
                        target,
                        result,
                    }),
                PromiseAwaitStatus::Fulfilled(value) => {
                    // Await always crosses a Promise job boundary, including
                    // for a value that Promise.resolve fulfilled immediately.
                    self.promise_jobs
                        .push_back(PromiseJob::AsyncGeneratorYield {
                            generator,
                            target,
                            result,
                            value,
                            fulfilled: true,
                        });
                }
                PromiseAwaitStatus::Rejected(value) => {
                    self.promise_jobs
                        .push_back(PromiseJob::AsyncGeneratorYield {
                            generator,
                            target,
                            result,
                            value,
                            fulfilled: false,
                        });
                }
            }
            Ok(())
        })();
        self.stack.truncate(base);
        outcome
    }

    pub(in super::super) fn finish_async_generator_yield(
        &mut self,
        generator: ObjectId,
        target: ObjectId,
        result: ObjectId,
        value: Value,
        fulfilled: bool,
    ) -> Result<(), RuntimeError> {
        if !fulfilled {
            self.close_async_generator(generator)?;
            self.complete_async_generator_request(
                generator,
                target,
                PromiseStatus::Rejected(value),
            )?;
            return self.resume_async_generator_next(generator);
        }
        self.set_property(&Value::Object(result), &"value".into(), &value)?;
        self.complete_async_generator_request(
            generator,
            target,
            PromiseStatus::Fulfilled(Value::Object(result)),
        )?;
        self.resume_async_generator_next(generator)
    }

    pub(in super::super) fn finish_async_generator_delegate(
        &mut self,
        generator: ObjectId,
        target: ObjectId,
        kind: AsyncGeneratorDelegateKind,
        result: Value,
        fulfilled: bool,
    ) -> Result<(), RuntimeError> {
        if !fulfilled {
            self.close_async_generator(generator)?;
            self.complete_async_generator_request(
                generator,
                target,
                PromiseStatus::Rejected(result),
            )?;
            return self.resume_async_generator_next(generator);
        }
        if !matches!(result, Value::Object(_)) {
            self.close_async_generator(generator)?;
            let error = self.error_object(
                "TypeError",
                "yield* delegate method must return an object".into(),
            )?;
            self.complete_async_generator_request(
                generator,
                target,
                PromiseStatus::Rejected(error),
            )?;
            return self.resume_async_generator_next(generator);
        }
        // IteratorComplete and IteratorValue run inside the generator, so an
        // exception from a getter is thrown at the `yield*` site (where the
        // generator's own try/catch can see it), not out of this reaction.
        let read = self.get_property(&result, &"done".into()).and_then(|done| {
            Ok((
                self.to_boolean(&done)?,
                self.get_property(&result, &"value".into())?,
            ))
        });
        let (done, value) = match read {
            Ok(pair) => pair,
            Err(error) => {
                let error = self.error_value(error)?;
                return self.throw_at_async_delegate_exit(generator, target, error);
            }
        };
        if !done {
            let iterator_result = self.iterator_result(value, false)?;
            return self.await_async_generator_yield(generator, target, iterator_result);
        }
        match kind {
            AsyncGeneratorDelegateKind::Return => {
                // A completed delegate supplies the final value of the
                // delegated expression, but the original outer return still
                // propagates through any enclosing finally blocks.
                let mut state = self.heap.take_generator_state(generator)?;
                let Some((record, exit)) = Self::yield_star_delegate(&state) else {
                    return Err(RuntimeError::Unsupported(
                        "lost async yield* delegation state",
                    ));
                };
                let GeneratorState::Suspended {
                    pc,
                    stack,
                    iterators,
                    async_delegate,
                    ..
                } = &mut state
                else {
                    unreachable!("yield* delegation is always suspended")
                };
                *pc = exit;
                *async_delegate = None;
                // The delegate has finished: it is no longer an open iterator
                // that the generator's own completion would close again.
                iterators.retain(|active| active != &record);
                *stack
                    .last_mut()
                    .expect("yield* delegation keeps its iterator record") = value.clone();
                self.heap.set_generator_state(generator, state)?;
                let result = self.generator_resume(
                    &Value::Object(generator),
                    None,
                    Some(target),
                    Some(Completion::Return(value)),
                );
                match result {
                    Ok(Value::Undefined) => Ok(()),
                    Ok(result) => self.await_async_generator_yield(generator, target, result),
                    Err(error) => {
                        let error = self.error_value(error)?;
                        self.complete_async_generator_request(
                            generator,
                            target,
                            PromiseStatus::Rejected(error),
                        )?;
                        self.resume_async_generator_next(generator)
                    }
                }
            }
            AsyncGeneratorDelegateKind::Throw => {
                let mut state = self.heap.take_generator_state(generator)?;
                let Some((record, exit)) = Self::yield_star_delegate(&state) else {
                    return Err(RuntimeError::Unsupported(
                        "lost async yield* delegation state",
                    ));
                };
                let GeneratorState::Suspended {
                    pc,
                    stack,
                    iterators,
                    async_delegate,
                    ..
                } = &mut state
                else {
                    unreachable!("yield* delegation is always suspended")
                };
                *pc = exit;
                *async_delegate = None;
                iterators.retain(|active| active != &record);
                *stack
                    .last_mut()
                    .expect("yield* delegation keeps its iterator record") = value;
                self.heap.set_generator_state(generator, state)?;
                let result = self.generator_next(&Value::Object(generator), None, Some(target));
                match result {
                    Ok(Value::Undefined) => Ok(()),
                    Ok(result) => self.await_async_generator_yield(generator, target, result),
                    Err(error) => {
                        let error = self.error_value(error)?;
                        self.complete_async_generator_request(
                            generator,
                            target,
                            PromiseStatus::Rejected(error),
                        )?;
                        self.resume_async_generator_next(generator)
                    }
                }
            }
        }
    }

    /// Resumes an async generator suspended in a `yield*` loop with a throw
    /// completion positioned just past the loop: the delegate is finished (it
    /// is no longer an open iterator to close) and the exception propagates
    /// through the generator's enclosing handlers.
    fn throw_at_async_delegate_exit(
        &mut self,
        generator: ObjectId,
        target: ObjectId,
        error: Value,
    ) -> Result<(), RuntimeError> {
        let mut state = self.heap.take_generator_state(generator)?;
        let Some((record, exit)) = Self::yield_star_delegate(&state) else {
            return Err(RuntimeError::Unsupported(
                "lost async yield* delegation state",
            ));
        };
        let GeneratorState::Suspended {
            pc,
            stack,
            iterators,
            async_delegate,
            ..
        } = &mut state
        else {
            unreachable!("yield* delegation is always suspended")
        };
        *pc = exit;
        *async_delegate = None;
        iterators.retain(|active| active != &record);
        *stack
            .last_mut()
            .expect("yield* delegation keeps its iterator record") = Value::Undefined;
        self.heap.set_generator_state(generator, state)?;
        let result = self.generator_resume(
            &Value::Object(generator),
            None,
            Some(target),
            Some(Completion::Throw(RuntimeError::Thrown(error))),
        );
        match result {
            Ok(Value::Undefined) => Ok(()),
            Ok(result) => self.await_async_generator_yield(generator, target, result),
            Err(error) => {
                let error = self.error_value(error)?;
                self.complete_async_generator_request(
                    generator,
                    target,
                    PromiseStatus::Rejected(error),
                )?;
                self.resume_async_generator_next(generator)
            }
        }
    }

    pub(in super::super) fn close_async_generator(
        &mut self,
        generator: ObjectId,
    ) -> Result<(), RuntimeError> {
        let state = self.heap.take_generator_state(generator)?;
        let iterators = match state {
            GeneratorState::Suspended { iterators, .. } => iterators,
            GeneratorState::Start { .. } | GeneratorState::Running | GeneratorState::Done => {
                Vec::new()
            }
        };
        self.heap
            .set_generator_state(generator, GeneratorState::Done)?;
        let base = self.stack.len();
        self.stack.extend(iterators.iter().cloned());
        let result = iterators
            .iter()
            .rev()
            .try_for_each(|record| self.iterator_close(record));
        self.stack.truncate(base);
        result
    }
}
