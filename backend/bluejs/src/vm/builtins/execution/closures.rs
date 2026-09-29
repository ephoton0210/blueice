// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Vm {
    pub(in super::super::super) fn binding_value(
        &mut self,
        slot: usize,
    ) -> Result<Option<Value>, RuntimeError> {
        if let Some(cell) = self.cells.get(&slot) {
            let value = self.heap.get_own(*cell, "value")?;
            if value.is_none() && self.binding_metadata[slot].eval_var {
                // The eval-created binding was deleted: the name resolves
                // outward again, as a fresh reference would.
                let name = self.binding_metadata[slot].name.clone();
                return self.lookup_global_name(&name);
            }
            Ok(value)
        } else {
            Ok(self.bindings[slot].clone())
        }
    }

    pub(in super::super::super) fn store_binding(
        &mut self,
        slot: usize,
        value: Value,
    ) -> Result<(), RuntimeError> {
        if let Some(&cell) = self.cells.get(&slot) {
            self.store_global_cell(cell, value)?;
        } else {
            self.bindings[slot] = Some(value);
        }
        Ok(())
    }

    pub(in super::super::super) fn capture(
        &mut self,
        slot: usize,
    ) -> Result<ObjectId, RuntimeError> {
        if let Some(&cell) = self.cells.get(&slot) {
            return Ok(cell);
        }
        let cell = self.with_roots(|heap| heap.alloc_object(None))?;
        self.cells.insert(slot, cell);
        if let Some(value) = self.bindings[slot].take() {
            self.with_roots(|heap| heap.set(cell, "value", value))?;
        }
        Ok(cell)
    }

    pub(in super::super::super) fn call_closure(
        &mut self,
        call: ClosureCall,
    ) -> Result<Value, RuntimeError> {
        let ClosureCall {
            code,
            captures,
            callee,
            receiver,
            args,
            construct,
            home,
            with_objects: closure_with_objects,
        } = call;
        let nested_debugger_target = self
            .debugger_nested_pause_request
            .as_ref()
            .filter(|request| {
                code.debugger_program_generation == Some(request.program_generation)
                    && code.debugger_code_unit_ordinal == Some(request.code_unit_ordinal)
                    && request.caller_program_generation.is_none_or(|generation| {
                        self.debugger_nested_direct_caller_generation == Some(generation)
                    })
            })
            .map(|request| (request.bytecode_offset, request.code_unit_ordinal));
        if nested_debugger_target.is_some()
            && (self.call_depth != 1
                || !self.debugger_nested_direct_call
                || construct
                || code.async_function
                || code.generator
                || code.class_constructor)
        {
            return Err(RuntimeError::Unsupported(
                "nested debugger pause requires a direct synchronous closure call",
            ));
        }
        if code.class_constructor && !construct {
            // §10.2.1.1's class-constructor rejection is created in the
            // function's Realm. Materialize it before a Test262 membrane can
            // return from this VM, rather than letting the caller allocate a
            // same-named error in its own Realm.
            let error = self.error_value(RuntimeError::TypeError(
                "class constructor cannot be invoked without new".into(),
            ))?;
            return Err(RuntimeError::Thrown(error));
        }
        if construct && !code.constructible {
            return Err(RuntimeError::TypeError(
                "arrow function is not a constructor".into(),
            ));
        }
        // Async generator requests own their own Promise capabilities. Do not
        // allocate an ordinary async-function capability before entering the
        // generator branch, where it would be unreachable and leak state.
        let async_function = code.async_function && !code.generator;
        // AsyncFunctionStart creates the promise capability before executing
        // the body.  If the body reaches await, that same promise owns the
        // saved continuation; otherwise its immediate completion settles it.
        let async_promise = async_function.then(|| self.new_promise()).transpose()?;
        let receiver = if construct && code.derived_constructor {
            Value::Undefined
        } else if construct {
            let prototype = self.constructor_prototype(self.object_prototype)?;
            Value::Object(self.with_roots(|heap| heap.alloc_object(Some(prototype)))?)
        } else if code.arrow || code.strict {
            receiver
        } else if matches!(receiver, Value::Null | Value::Undefined) {
            self.global("globalThis")?
        } else {
            Value::Object(self.coerce_object(&receiver)?)
        };
        if construct && code.class_constructor && !code.derived_constructor {
            // A base class constructor's InitializeInstanceElements runs
            // right after `this` is created, before the body (and even
            // before its parameters are evaluated). A derived constructor's
            // runs when its `super()` returns.
            self.initialize_instance_elements(&callee, &receiver)?;
        }
        if code.generator {
            let async_generator = code.async_function;
            let default_prototype = if code.async_function {
                self.async_generator_prototype()?
            } else {
                self.generator_prototype()?
            };
            let prototype = self
                .get_property(&callee, &"prototype".into())?
                .object_id()
                .unwrap_or(default_prototype);
            if !code.generator_initializes_parameters {
                let state = GeneratorState::Start {
                    code,
                    captures,
                    callee,
                    receiver,
                    args,
                    home,
                    with_objects: closure_with_objects,
                };
                let generator = self.with_roots(|heap| heap.alloc_generator(state, prototype))?;
                if async_generator {
                    self.heap.enable_async_generator(generator)?;
                }
                return Ok(Value::Object(generator));
            }
            // Install a temporary state first so the generator owns every
            // captured edge while the entry phase may allocate. The actual
            // parameter frame replaces it below before the object escapes.
            let generator = self.with_roots(|heap| {
                heap.alloc_generator(
                    GeneratorState::Start {
                        code: code.clone(),
                        captures: captures.clone(),
                        callee: callee.clone(),
                        receiver: receiver.clone(),
                        args: args.clone(),
                        home,
                        with_objects: closure_with_objects.clone(),
                    },
                    prototype,
                )
            })?;
            if async_generator {
                self.heap.enable_async_generator(generator)?;
            }
            let base = self.stack.len();
            self.stack.push(Value::Object(generator));
            let state = self.initialize_generator(
                code,
                captures,
                callee.clone(),
                receiver,
                args,
                home,
                closure_with_objects,
            );
            self.stack.truncate(base);
            let state = state?;
            // FunctionDeclarationInstantiation is observable to a parameter
            // initializer. Read `.prototype` again after it completes: a
            // default such as `(g.prototype = null)` must affect the freshly
            // created generator object's [[Prototype]].
            let prototype = self
                .get_property(&callee, &"prototype".into())?
                .object_id()
                .unwrap_or(default_prototype);
            self.heap.set_prototype(generator, Some(prototype))?;
            self.stack.push(Value::Object(generator));
            let stored = self.with_roots(|heap| heap.set_generator_state(generator, state));
            self.stack.pop();
            stored?;
            return Ok(Value::Object(generator));
        }
        self.stack.push(receiver.clone());
        let base = self.stack.len();
        self.stack.extend(self.bindings.iter().flatten().cloned());
        self.stack
            .extend(self.cells.values().copied().map(Value::Object));
        self.stack.push(self.completion.clone());
        self.stack.push(self.this.clone());
        self.stack.extend(self.arguments.iter().cloned());
        // The callee sees only the with objects it closed over, not the
        // caller's; keep the caller's rooted on the stack meanwhile.
        self.stack.extend(self.with_objects.iter().cloned());
        let frame_base = self.stack.len();
        let mut frame_bindings = vec![None; code.bindings.len()];
        if let Some(slot) = code.self_slot {
            frame_bindings[slot as usize] = Some(callee.clone());
        }
        let bindings = std::mem::replace(&mut self.bindings, frame_bindings);
        let binding_metadata = std::mem::replace(&mut self.binding_metadata, code.bindings.clone());
        let cells = std::mem::replace(&mut self.cells, captures.into_iter().enumerate().collect());
        let dynamic_eval_bindings = std::mem::take(&mut self.dynamic_eval_bindings);
        let mut dynamic_eval_outer_bindings = std::mem::take(&mut self.dynamic_eval_outer_bindings);
        dynamic_eval_outer_bindings.push(dynamic_eval_bindings);
        self.dynamic_eval_outer_bindings = dynamic_eval_outer_bindings;
        let eval_dynamic_slots = std::mem::take(&mut self.eval_dynamic_slots);
        let script_global_slots = std::mem::take(&mut self.script_global_slots);
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
        let this = std::mem::replace(&mut self.this, receiver);
        let arguments = std::mem::replace(&mut self.arguments, args);
        let frame_callee = std::mem::replace(&mut self.callee, callee.clone());
        let completion = std::mem::replace(&mut self.completion, Value::Undefined);
        let completion_empty = std::mem::replace(&mut self.completion_empty, true);
        let active_scopes = std::mem::take(&mut self.active_scopes);
        let active_scope_slots = std::mem::take(&mut self.active_scope_slots);
        let strict = std::mem::replace(&mut self.strict, code.strict);
        let home_object = std::mem::replace(&mut self.home_object, home);
        let class_field_initializer = std::mem::replace(
            &mut self.class_field_initializer,
            code.class_field_initializer,
        );
        let pending_completions = self.pending_completions.clone();
        let completion_saves = self.completion_saves.clone();
        let with_objects = std::mem::replace(&mut self.with_objects, closure_with_objects);
        let inherited_with_depth =
            std::mem::replace(&mut self.inherited_with_depth, self.with_objects.len());
        let parameter_eval_env = self.parameter_eval_env.take();
        if code.parameter_eval_scope {
            // Roots: `with_objects` is a root, and so is the callee frame's
            // stack, which holds the caller's copy until the call returns.
            let env = match self.new_parameter_eval_env() {
                Ok(env) => env,
                Err(error) => {
                    self.with_objects = with_objects;
                    self.inherited_with_depth = inherited_with_depth;
                    self.parameter_eval_env = parameter_eval_env;
                    self.stack.truncate(base - 1);
                    return Err(error);
                }
            };
            self.with_objects.push(Value::Object(env));
            // The environment is nested inside the objects the function was
            // created in, and the function's own bindings inside it.
            self.inherited_with_depth = self.with_objects.len();
            self.parameter_eval_env = Some(env);
        }
        let frame_dynamic_eval_outer_bindings = self.dynamic_eval_outer_bindings.clone();
        let top_level_module = self.top_level_module;
        let remaining_instructions = self.remaining_instructions;
        let new_target = self.new_target.clone();
        let new_target_allowed = self.new_target_allowed;
        let active_module_name = self.active_module_name.clone();
        let result_root = self.result_root.take();
        let mut suspended_parent_stack = None;
        let mut suspended_async = None;
        let running = callee.object_id();
        self.call_stack.extend(running);
        let result = if async_function {
            let mut iterators = Vec::new();
            match self.interpret(&code, &mut iterators, 0, None, None, None) {
                Ok(InterpreterExit::Return(value)) => Ok(value),
                Ok(InterpreterExit::Await {
                    promise,
                    pc,
                    handlers,
                }) => {
                    let stack = self.stack.split_off(frame_base);
                    let mut execution = self.suspend_module_execution();
                    let parent_stack = std::mem::replace(&mut execution.stack, stack);
                    let templates = execution.templates.clone();
                    self.templates = templates;
                    let state = AsyncContinuation {
                        generator: None,
                        target: async_promise.expect("async function has a promise"),
                        code: code.clone(),
                        pc,
                        execution,
                        iterators,
                        handlers,
                        call_depth: self.call_depth,
                    };
                    suspended_parent_stack = Some(parent_stack);
                    suspended_async = Some((state, promise));
                    Ok(Value::Undefined)
                }
                Ok(InterpreterExit::Yield { .. }) => Err(RuntimeError::TypeError(
                    "yield requires an async generator function".into(),
                )),
                Ok(InterpreterExit::Suspend { .. }) => {
                    unreachable!("ordinary async functions have no entry suspend")
                }
                Err(error) => {
                    let base = self.stack.len();
                    if let RuntimeError::Thrown(value) = &error {
                        self.stack.push(value.clone());
                    }
                    self.stack.extend(iterators.iter().cloned());
                    for record in iterators.into_iter().rev() {
                        let _ = self.iterator_close(&record);
                    }
                    self.stack.truncate(base);
                    Err(error)
                }
            }
        } else if let Some((target_offset, target_ordinal)) = nested_debugger_target {
            let frame_serial = self.next_debugger_frame_serial;
            let next_frame_serial =
                frame_serial
                    .checked_add(1)
                    .ok_or(RuntimeError::Unsupported(
                        "nested debugger frame serial exhausted",
                    ))?;
            let pending_base = self.pending_completions.len();
            let save_base = self.completion_saves.len();
            let mut iterators = Vec::new();
            let result = match self.interpret(
                &code,
                &mut iterators,
                0,
                None,
                Some(InterpreterSuspensionPoint::Offset(target_offset)),
                None,
            ) {
                Ok(InterpreterExit::Suspend {
                    pc,
                    iterators,
                    handlers,
                }) => {
                    let stack = self.stack.split_off(frame_base);
                    let mut execution = self.suspend_module_execution();
                    let parent_stack = std::mem::replace(&mut execution.stack, stack);
                    self.next_debugger_frame_serial = next_frame_serial;
                    self.debugger_nested_continuation = Some(NestedDebuggerContinuation {
                        frame_serial,
                        code_unit_ordinal: target_ordinal,
                        code: code.as_ref().clone(),
                        pc,
                        execution,
                        iterators,
                        handlers,
                    });
                    suspended_parent_stack = Some(parent_stack);
                    Ok(Value::Undefined)
                }
                Ok(InterpreterExit::Return(value)) => Ok(value),
                Ok(InterpreterExit::Yield { .. } | InterpreterExit::Await { .. }) => Err(
                    RuntimeError::Unsupported("nested debugger target cannot yield or await"),
                ),
                Err(error) => Err(error),
            };
            if result.is_err() {
                let base = self.stack.len();
                if let Err(RuntimeError::Thrown(value)) = &result {
                    self.stack.push(value.clone());
                }
                self.stack.extend(iterators.iter().cloned());
                for record in iterators.into_iter().rev() {
                    let _ = self.iterator_close(&record);
                }
                self.stack.truncate(base);
            }
            if suspended_parent_stack.is_none() {
                self.pending_completions.truncate(pending_base);
                self.completion_saves.truncate(save_base);
            }
            result
        } else {
            self.run(&code)
        };
        if running.is_some() {
            self.call_stack.pop();
        }
        // A derived constructor's `this` lives in its hidden binding (which
        // `super()` in the constructor or in a nested arrow or eval bound);
        // every other constructor's is the receiver allocated at entry.
        let constructed = match code.derived_this_slot {
            Some(slot) => self
                .binding_value(slot as usize)
                .ok()
                .flatten()
                .unwrap_or(Value::Undefined),
            None => self.this.clone(),
        };
        let suspended = suspended_parent_stack.is_some();
        if let Some(stack) = suspended_parent_stack {
            self.stack = stack;
            self.result_root = result_root;
            self.pending_completions = pending_completions;
            self.completion_saves = completion_saves;
            self.with_objects = with_objects;
            self.inherited_with_depth = inherited_with_depth;
            self.parameter_eval_env = parameter_eval_env;
            self.dynamic_eval_outer_bindings = frame_dynamic_eval_outer_bindings;
            self.top_level_module = top_level_module;
            self.remaining_instructions = remaining_instructions;
            self.new_target = new_target;
            self.new_target_allowed = new_target_allowed;
            self.active_module_name = active_module_name;
        } else {
            self.result_root = result_root;
            self.with_objects = with_objects;
            self.inherited_with_depth = inherited_with_depth;
            self.parameter_eval_env = parameter_eval_env;
        }
        self.bindings = bindings;
        self.binding_metadata = binding_metadata;
        self.cells = cells;
        let mut dynamic_eval_outer_bindings = std::mem::take(&mut self.dynamic_eval_outer_bindings);
        self.dynamic_eval_bindings = dynamic_eval_outer_bindings
            .pop()
            .expect("callee inherits its caller dynamic environment");
        self.dynamic_eval_outer_bindings = dynamic_eval_outer_bindings;
        self.eval_dynamic_slots = eval_dynamic_slots;
        self.script_global_slots = script_global_slots;
        self.variable_scope = variable_scope;
        self.variable_scope_lexicals = variable_scope_lexicals;
        self.this = this;
        self.arguments = arguments;
        self.callee = frame_callee;
        self.completion = completion;
        self.completion_empty = completion_empty;
        self.active_scopes = active_scopes;
        self.active_scope_slots = active_scope_slots;
        self.strict = strict;
        self.home_object = home_object;
        self.class_field_initializer = class_field_initializer;
        self.stack.truncate(base - 1);
        if let Some((state, awaited)) = suspended_async {
            self.suspend_async_await(state, awaited)?;
        }
        let mut completion_check_failed = false;
        let result = result.and_then(|value| {
            if construct && !matches!(value, Value::Object(_)) {
                if code.derived_constructor && value != Value::Undefined {
                    completion_check_failed = true;
                    return Err(RuntimeError::TypeError(
                        "derived constructor returned a non-object value".into(),
                    ));
                }
                if matches!(constructed, Value::Object(_)) {
                    Ok(constructed)
                } else {
                    completion_check_failed = true;
                    Err(RuntimeError::ReferenceError(
                        "derived constructor did not call super()".into(),
                    ))
                }
            } else {
                Ok(value)
            }
        });
        self.construct_completion_check_failed = completion_check_failed;
        if !async_function {
            return result;
        }

        let promise = async_promise.expect("async function has a promise");
        match result {
            // `AsyncFunctionStart` resolves rather than directly fulfills so
            // `return somePromise` adopts its eventual settlement.
            Ok(value) if !suspended => self.resolve_promise(promise, value)?,
            Ok(_) => {}
            Err(error) => {
                let value = self.error_value(error)?;
                self.settle_promise(promise, PromiseStatus::Rejected(value))?;
            }
        }
        Ok(Value::Object(promise))
    }
}
