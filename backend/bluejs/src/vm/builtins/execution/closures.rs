// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

/// How the first, synchronous stretch of an async function's body ended.
enum AsyncBody {
    Returned(Value),
    Awaiting {
        promise: ObjectId,
        pc: usize,
        handlers: HandlerRecords,
    },
    Failed(RuntimeError),
}

impl AsyncBody {
    /// An async function that is not a generator contains no `yield`, and only
    /// generators suspend on entry, so those two exits are failures here.
    fn classify(result: Result<InterpreterExit, RuntimeError>) -> Self {
        match result {
            Ok(InterpreterExit::Return(value)) => Self::Returned(value),
            Ok(InterpreterExit::Await {
                promise,
                pc,
                handlers,
            }) => Self::Awaiting {
                promise,
                pc,
                handlers,
            },
            Ok(InterpreterExit::Yield { .. }) => Self::Failed(RuntimeError::TypeError(
                "yield requires an async generator function".into(),
            )),
            Ok(InterpreterExit::Suspend { .. }) => Self::Failed(RuntimeError::TypeError(
                "an ordinary async function has no entry suspend".into(),
            )),
            Err(error) => Self::Failed(error),
        }
    }
}

impl Vm {
    /// Marks a generator this call has just allocated as an async generator.
    fn enable_async_generator(&mut self, generator: ObjectId) {
        self.heap
            .enable_async_generator(generator)
            .expect("the generator was allocated by this call");
    }

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
        entry_payload: execution_payload::CallEntryCharge,
    ) -> Result<Value, RuntimeError> {
        self.with_rejection_origin(None, |vm| vm.call_closure_inner(call, entry_payload))
    }

    fn call_closure_inner(
        &mut self,
        call: ClosureCall,
        mut entry_payload: execution_payload::CallEntryCharge,
    ) -> Result<Value, RuntimeError> {
        let entry_epoch = self.throw_epoch;
        let ClosureCall {
            code,
            captures,
            callee,
            mut receiver,
            args,
            construct,
            home,
            with_objects: mut closure_with_objects,
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
        let intermediate_debugger_frame = nested_debugger_target.is_none()
            && self.call_depth == 1
            && self.debugger_nested_direct_call
            && !construct
            && !code.async_function
            && !code.generator
            && !code.class_constructor
            && self
                .debugger_nested_pause_request
                .as_ref()
                .is_some_and(|request| {
                    request
                        .intermediate_program_generation
                        .is_some_and(|generation| {
                            code.debugger_program_generation == Some(generation)
                        })
                });
        if nested_debugger_target.is_some()
            && (!(self.call_depth == 1
                || (self.call_depth == 2 && self.debugger_nested_intermediate_active))
                || !self.debugger_nested_direct_call
                || construct
                || code.async_function
                || code.generator
                || code.class_constructor)
        {
            return Err(RuntimeError::Unsupported(
                "nested debugger pause requires a bounded direct synchronous caller chain",
            ));
        }
        // Reject serial exhaustion before replacing the caller's bindings,
        // stack and execution context. An early error in the nested body path
        // would otherwise skip the common frame restoration below.
        let next_debugger_frame_serial = if nested_debugger_target.is_some() {
            Some(self.next_debugger_frame_serial.checked_add(1).ok_or(
                RuntimeError::Unsupported("nested debugger frame serial exhausted"),
            )?)
        } else {
            None
        };
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
        let mut prepared = if !code.generator || code.generator_initializes_parameters {
            Some(self.prepare_frame_bindings(&code, FrameBindingKind::Function)?)
        } else {
            None
        };
        let mut prepared_maps = if prepared.is_some() {
            Some(self.prepare_frame_maps(captures.len(), 0, 0)?)
        } else {
            None
        };
        // Async generator requests own their own Promise capabilities. Do not
        // allocate an ordinary async-function capability before entering the
        // generator branch, where it would be unreachable and leak state.
        let async_function = code.async_function && !code.generator;
        let can_suspend =
            async_function || nested_debugger_target.is_some() || intermediate_debugger_frame;
        let mut prepared_environment = if !code.generator {
            Some(self.prepare_call_environment(can_suspend)?)
        } else {
            None
        };
        // AsyncFunctionStart creates the promise capability before executing
        // the body.  If the body reaches await, that same promise owns the
        // saved continuation; otherwise its immediate completion settles it.
        let async_promise = async_function.then(|| self.new_promise()).transpose()?;
        // The invocation owns its result capability before bindings and
        // arguments are installed. enter_call releases this stack suffix on
        // every exit; a suspended frame subsequently traces the same target.
        self.stack.extend(async_promise.map(Value::Object));
        if construct && code.derived_constructor {
            receiver = Value::Undefined;
        } else if construct {
            let prototype = self.constructor_prototype(self.object_prototype)?;
            receiver = Value::Object(self.with_roots(|heap| heap.alloc_object(Some(prototype)))?);
        } else if !code.arrow && !code.strict {
            if matches!(receiver, Value::Null | Value::Undefined) {
                receiver = self.global("globalThis")?;
            } else {
                receiver = Value::Object(self.coerce_object(&receiver)?);
            }
        }
        entry_payload.refresh(
            &callee,
            &receiver,
            &args,
            host_scheduling::slots(&captures)
                .saturating_add(host_scheduling::slots(&closure_with_objects))
                .saturating_add(host_scheduling::values_bytes(&closure_with_objects)),
        );
        self.synchronize_execution_payload()?;
        if construct && code.class_constructor && !code.derived_constructor {
            // A base class constructor's InitializeInstanceElements runs
            // right after `this` is created, before the body (and even
            // before its parameters are evaluated). A derived constructor's
            // runs when its `super()` returns.
            self.initialize_instance_elements(&callee, &receiver)?;
        }
        if code.generator {
            let async_generator = code.async_function;
            // Creating the generator function (in the interpreter or the
            // dynamic function constructors) already materialized both
            // prototypes, so neither lookup can allocate here.
            let default_prototype = if code.async_function {
                self.async_generator_prototype()
            } else {
                self.generator_prototype()
            }
            .expect(GENERATOR_INTRINSICS);
            let prototype = self
                .get_property(&callee, &"prototype".into())
                .expect(GENERATOR_PROTOTYPE)
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
                let generator = self.with_roots(|heap| {
                    let result = heap.alloc_generator(state, prototype);
                    drop(entry_payload);
                    result
                })?;
                if async_generator {
                    self.enable_async_generator(generator);
                }
                return Ok(Value::Object(generator));
            }
            self.prepare_generator_entry(
                &mut closure_with_objects,
                receiver.object_id().is_some(),
                code.parameter_eval_scope,
                &mut entry_payload,
            )?;
            // Inputs remain rooted by the call frame and the closure. Only
            // a newly boxed receiver needs an extra edge before allocation.
            let base = self.stack.len();
            self.stack.extend(receiver.object_id().map(Value::Object));
            let allocated =
                self.with_roots(|heap| heap.alloc_generator(GeneratorState::Running, prototype));
            let generator = match allocated {
                Ok(generator) => generator,
                Err(error) => {
                    self.stack.truncate(base);
                    return Err(error);
                }
            };
            self.stack.push(Value::Object(generator));
            if async_generator {
                self.enable_async_generator(generator);
            }
            let state = self.initialize_generator(
                code,
                captures,
                callee.clone(),
                receiver,
                args,
                home,
                closure_with_objects,
                entry_payload,
                prepared
                    .take()
                    .expect("parameter entry prepared its bindings"),
                prepared_maps
                    .take()
                    .expect("parameter entry prepared its cell table"),
            );
            self.stack.truncate(base);
            let state = state?;
            // FunctionDeclarationInstantiation is observable to a parameter
            // initializer. Read `.prototype` again after it completes: a
            // default such as `(g.prototype = null)` must affect the freshly
            // created generator object's [[Prototype]].
            let prototype = self
                .get_property(&callee, &"prototype".into())
                .expect(GENERATOR_PROTOTYPE)
                .object_id()
                .unwrap_or(default_prototype);
            self.heap
                .set_prototype(generator, Some(prototype))
                .expect("a freshly allocated generator cannot form a prototype cycle");
            self.stack.push(Value::Object(generator));
            let stored = state.store(self, generator);
            self.stack.pop();
            stored?;
            return Ok(Value::Object(generator));
        }
        let caller_payload = self.charge_execution_payload(
            self.caller_execution_payload_bytes()
                .saturating_add(self.pending_execution_payload_bytes()),
        );
        let caller_control = self.charge_execution_control(
            self.displaced_control_bytes(execution_payload::ControlTransfer::Call),
        );
        self.stack
            .push(receiver.object_id().map_or(Value::Undefined, Value::Object));
        let base = self.stack.len();
        self.stack.extend(
            self.bindings
                .iter()
                .flatten()
                .filter_map(Value::object_id)
                .map(Value::Object),
        );
        self.stack
            .extend(self.cells.values().copied().map(Value::Object));
        self.stack.push(
            self.completion
                .object_id()
                .map_or(Value::Undefined, Value::Object),
        );
        self.stack.push(
            self.this
                .object_id()
                .map_or(Value::Undefined, Value::Object),
        );
        // A resumed async/generator caller is absent from call_stack. Its
        // function must survive while the callee replaces Vm::callee and
        // allocates, so restoring the caller cannot publish a collected ID.
        self.stack.push(
            self.callee
                .object_id()
                .map_or(Value::Undefined, Value::Object),
        );
        self.stack.extend(
            self.arguments
                .iter()
                .filter_map(Value::object_id)
                .map(Value::Object),
        );
        // The callee sees only the with objects it closed over, not the
        // caller's; keep the caller's rooted on the stack meanwhile.
        self.stack.extend(self.with_objects.iter().cloned());
        self.stack.extend(
            self.disposables
                .iter()
                .flat_map(DisposableResource::values)
                .filter_map(Value::object_id)
                .map(Value::Object),
        );
        let pending_roots = self.pending_execution_references();
        self.stack
            .extend(pending_roots.into_iter().map(Value::Object));
        let frame_base = self.stack.len();
        let mut prepared = prepared
            .take()
            .expect("ordinary entry prepared its bindings");
        let mut prepared_maps = prepared_maps
            .take()
            .expect("ordinary entry prepared its cell table");
        prepared_maps.adopt_captures(captures);
        let (mut frame_bindings, frame_metadata, frame_lexicals) = prepared.take_parts();
        if let Some(slot) = code.self_slot {
            frame_bindings[slot as usize] = Some(callee.clone());
        }
        let disposables = std::mem::take(&mut self.disposables);
        let dispose_marks = std::mem::take(&mut self.dispose_marks);
        let bindings = std::mem::replace(&mut self.bindings, frame_bindings);
        let binding_metadata = std::mem::replace(&mut self.binding_metadata, frame_metadata);
        let cells = std::mem::replace(&mut self.cells, prepared_maps.take_cells());
        let dynamic_eval_bindings = std::mem::take(&mut self.dynamic_eval_bindings);
        let mut dynamic_eval_outer_bindings = std::mem::take(&mut self.dynamic_eval_outer_bindings);
        dynamic_eval_outer_bindings.push(dynamic_eval_bindings);
        self.dynamic_eval_outer_bindings = dynamic_eval_outer_bindings;
        let eval_dynamic_slots = std::mem::take(&mut self.eval_dynamic_slots);
        let script_global_slots = std::mem::take(&mut self.script_global_slots);
        let variable_scope = std::mem::replace(&mut self.variable_scope, code.variable_scope);
        let variable_scope_lexicals =
            std::mem::replace(&mut self.variable_scope_lexicals, frame_lexicals);
        let this = std::mem::replace(&mut self.this, receiver);
        let arguments = std::mem::replace(&mut self.arguments, args);
        // Prototype getters and base-class field initializers can run before
        // this transfer. Keep entry buffers charged until the VM owns them.
        drop(entry_payload);
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
        // A callee owns a separate completion context. In particular, async
        // Return must not leave its primitive result in the caller's records
        // or capture the caller's pending finally return across an await.
        let pending_completions = std::mem::take(&mut self.pending_completions);
        let completion_saves = std::mem::take(&mut self.completion_saves);
        let with_objects = std::mem::replace(&mut self.with_objects, closure_with_objects);
        let inherited_with_depth =
            std::mem::replace(&mut self.inherited_with_depth, self.with_objects.len());
        let parameter_eval_env = self.parameter_eval_env.take();
        drop(prepared);
        drop(prepared_maps);
        let mut entry_error = None;
        if code.parameter_eval_scope {
            // Roots: `with_objects` is a root, and so is the callee frame's
            // stack, which holds the caller's copy until the call returns.
            match self.new_parameter_eval_env() {
                Ok(env) => {
                    self.with_objects.push(Value::Object(env));
                    // The environment is nested inside the objects the function
                    // closed over, and its own bindings inside it.
                    self.inherited_with_depth = self.with_objects.len();
                    self.parameter_eval_env = Some(env);
                }
                Err(error) => entry_error = Some(error),
            }
        }
        let mut prepared_environment = prepared_environment
            .take()
            .expect("ordinary entry prepared its environment stack");
        let (mut frame_dynamic_eval_outer_bindings, mut active_module_name) =
            prepared_environment.take_parts();
        let top_level_module = self.top_level_module;
        let remaining_instructions = self.remaining_instructions;
        let new_target = self.new_target.clone();
        let new_target_allowed = self.new_target_allowed;
        let result_root = self.result_root.take();
        let mut suspended = false;
        let mut suspended_async = None;
        let running = callee
            .object_id()
            .expect("a closure call has a function object as its callee");
        self.call_stack.push(running);
        let result = if let Some(error) = entry_error {
            Err(error)
        } else if async_function {
            let mut iterators = self.iterator_records(Vec::new());
            let mut suspension = None;
            let body = match AsyncBody::classify(self.interpret(
                &code,
                &mut iterators,
                0,
                None,
                None,
                None,
            )) {
                body @ AsyncBody::Awaiting { .. } => {
                    match self.prepare_suspension_storage(frame_base, false) {
                        Ok(storage) => {
                            suspension = Some(storage);
                            body
                        }
                        Err(error) => AsyncBody::Failed(error),
                    }
                }
                body => body,
            };
            match body {
                AsyncBody::Returned(value) => Ok(value),
                AsyncBody::Awaiting {
                    promise,
                    pc,
                    handlers,
                } => {
                    let execution = self.capture_prepared_execution(
                        suspension.take().expect("await prepared its stack buffer"),
                    );
                    let state = AsyncContinuation {
                        gc_live: false,
                        generator: None,
                        target: async_promise.expect("async function has a promise"),
                        code: code.clone(),
                        pc,
                        execution,
                        iterators,
                        handlers,
                        call_depth: self.call_depth,
                    };
                    suspended = true;
                    suspended_async = Some((state, promise));
                    Ok(Value::Undefined)
                }
                AsyncBody::Failed(error) => {
                    let base = self.stack.len();
                    if let RuntimeError::Thrown(value) = &error {
                        self.stack.push(value.clone());
                    }
                    self.stack.extend(iterators.iter().cloned());
                    while let Some(record) = iterators.pop() {
                        self.iterator_close_preserving_throw(&record);
                    }
                    self.stack.truncate(base);
                    Err(error)
                }
            }
        } else if let Some((target_offset, target_ordinal)) = nested_debugger_target {
            let frame_serial = self.next_debugger_frame_serial;
            let next_frame_serial = next_debugger_frame_serial
                .expect("nested invocation reserved its serial before installing the frame");
            let pending_base = self.pending_completions.len();
            let save_base = self.completion_saves.len();
            let mut iterators = self.iterator_records(Vec::new());
            let result = match DebuggerInterpreterExit::classify(
                self.interpret(
                    &code,
                    &mut iterators,
                    0,
                    None,
                    Some(InterpreterSuspensionPoint::Offset(target_offset)),
                    None,
                ),
                DebuggerFrameKind::NestedEntry,
            ) {
                DebuggerInterpreterExit::Suspended {
                    pc,
                    iterators: saved_iterators,
                    handlers,
                } => match crate::vm::root_code_storage::SavedRootCode::copy(&code, self).and_then(
                    |saved| {
                        self.capture_suspension_execution(frame_base)
                            .map(|execution| (saved, execution))
                    },
                ) {
                    Ok((saved_code, execution)) => {
                        self.next_debugger_frame_serial = next_frame_serial;
                        self.debugger_nested_continuation = Some(NestedDebuggerContinuation {
                            frame_serial,
                            code_unit_ordinal: target_ordinal,
                            code: saved_code,
                            pc,
                            execution,
                            iterators: saved_iterators,
                            handlers,
                        });
                        suspended = true;
                        Ok(Value::Undefined)
                    }
                    Err(error) => {
                        iterators = saved_iterators;
                        Err(error)
                    }
                },
                DebuggerInterpreterExit::Completed(result) => result,
            };
            if result.is_err() {
                let base = self.stack.len();
                if let Err(RuntimeError::Thrown(value)) = &result {
                    self.stack.push(value.clone());
                }
                self.stack.extend(iterators.iter().cloned());
                while let Some(record) = iterators.pop() {
                    self.iterator_close_preserving_throw(&record);
                }
                self.stack.truncate(base);
            }
            if !suspended {
                self.pending_completions.truncate(pending_base);
                self.completion_saves.truncate(save_base);
            }
            result
        } else if intermediate_debugger_frame {
            let pending_base = self.pending_completions.len();
            let save_base = self.completion_saves.len();
            let mut iterators = self.iterator_records(Vec::new());
            let previous = std::mem::replace(&mut self.debugger_nested_intermediate_active, true);
            let exit = self.interpret(&code, &mut iterators, 0, None, None, None);
            self.debugger_nested_intermediate_active = previous;
            let result =
                match DebuggerInterpreterExit::classify(exit, DebuggerFrameKind::NestedEntry) {
                    DebuggerInterpreterExit::Suspended {
                        pc,
                        iterators: saved_iterators,
                        handlers,
                    } => {
                        debug_assert!(self.debugger_nested_continuation.is_some());
                        debug_assert!(self.debugger_nested_intermediate_continuation.is_none());
                        match crate::vm::root_code_storage::SavedRootCode::copy(&code, self)
                            .and_then(|saved| {
                                self.capture_suspension_execution(frame_base)
                                    .map(|execution| (saved, execution))
                            }) {
                            Ok((saved_code, execution)) => {
                                self.debugger_nested_intermediate_continuation =
                                    Some(IntermediateDebuggerContinuation {
                                        code: saved_code,
                                        pc,
                                        execution,
                                        iterators: saved_iterators,
                                        handlers,
                                    });
                                suspended = true;
                                Ok(Value::Undefined)
                            }
                            Err(error) => {
                                iterators = saved_iterators;
                                Err(error)
                            }
                        }
                    }
                    DebuggerInterpreterExit::Completed(result) => result,
                };
            if result.is_err() {
                let base = self.stack.len();
                if let Err(RuntimeError::Thrown(value)) = &result {
                    self.stack.push(value.clone());
                }
                self.stack.extend(iterators.iter().cloned());
                while let Some(record) = iterators.pop() {
                    self.iterator_close_preserving_throw(&record);
                }
                self.stack.truncate(base);
            }
            if !suspended {
                self.pending_completions.truncate(pending_base);
                self.completion_saves.truncate(save_base);
            }
            result
        } else {
            self.run(&code)
        };
        self.call_stack.pop();
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
        if suspended {
            self.result_root = result_root;
            self.with_objects = with_objects;
            self.inherited_with_depth = inherited_with_depth;
            self.parameter_eval_env = parameter_eval_env;
            self.dynamic_eval_outer_bindings =
                std::mem::take(&mut frame_dynamic_eval_outer_bindings);
            self.top_level_module = top_level_module;
            self.remaining_instructions = remaining_instructions;
            self.new_target = new_target;
            self.new_target_allowed = new_target_allowed;
            self.active_module_name = active_module_name.take();
        } else {
            self.result_root = result_root;
            self.with_objects = with_objects;
            self.inherited_with_depth = inherited_with_depth;
            self.parameter_eval_env = parameter_eval_env;
        }
        self.pending_completions = pending_completions;
        self.completion_saves = completion_saves;
        self.disposables = disposables;
        self.dispose_marks = dispose_marks;
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
        drop(caller_payload);
        drop(caller_control);
        drop(frame_dynamic_eval_outer_bindings);
        drop(active_module_name);
        drop(prepared_environment);
        // A failure to suspend takes the same route as a failure of the body:
        // it ends the call (or rejects the async function's promise) below.
        let result = match suspended_async {
            Some((state, awaited)) => self.suspend_async_await(state, awaited).and(result),
            None => result,
        };
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
                let origin = self.caught_rejection_origin(entry_epoch);
                let value = self.error_value(error)?;
                self.reject_promise_with_origin(promise, value, origin)?;
            }
        }
        Ok(Value::Object(promise))
    }
}

/// The `expect` message for a generator prototype looked up by a call to a
/// generator function that already exists.
const GENERATOR_INTRINSICS: &str = "a generator function's prototypes are already materialized";

/// A generator function's own `prototype` is a writable, non-configurable data
/// property, so reading it can neither throw nor allocate.
const GENERATOR_PROTOTYPE: &str = "a generator function's prototype is a data property";

#[cfg(any(test, coverage))]
#[path = "../../../../tests/fixtures/closure_execution_internal.rs"]
mod tests;
