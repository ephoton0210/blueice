// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use crate::heap::AsyncGeneratorOrigin;
use crate::vm::generator_input::{self, GeneratorInputKind, InterpreterResumeInput};
use crate::vm::native_value_storage::NativeValue;
use crate::vm::realm_call_storage::RealmCallRoots;

#[cfg(any(test, coverage))]
#[path = "../../../tests/fixtures/generator_delegation_boundaries.rs"]
mod boundary_tests;

#[cfg(any(test, coverage))]
#[path = "../../../tests/fixtures/await_registration.rs"]
mod await_registration_tests;

/// How a synchronous `yield*` delegate answered `next`, `throw` or `return`.
enum SyncDelegateStep {
    /// The delegate is not finished: its own result object is yielded as is.
    Yield(Value),
    /// The delegate finished with this value.
    Done(Value),
}

enum AwaitedGeneratorOperand {
    Return,
    ReturnWithoutDelegateMethod,
}

enum DelegatedAbrupt {
    Return,
    Throw,
}

enum AsyncGeneratorRequestKind {
    Next,
    Return,
    Throw,
}

#[derive(Debug, PartialEq)]
enum CompletedAsyncGeneratorRequest {
    Next,
    Return(Value),
    Throw(Value),
}

impl CompletedAsyncGeneratorRequest {
    fn classify(completion: AsyncGeneratorCompletion) -> Result<Self, RuntimeError> {
        match completion {
            AsyncGeneratorCompletion::Next(_) => Ok(Self::Next),
            AsyncGeneratorCompletion::Return(value) => Ok(Self::Return(value)),
            AsyncGeneratorCompletion::Throw(value) => Ok(Self::Throw(value)),
            AsyncGeneratorCompletion::ReturnAwaited(_)
            | AsyncGeneratorCompletion::ResumeReturn(_)
            | AsyncGeneratorCompletion::ResumeThrow(_) => Err(RuntimeError::Unsupported(
                "a completed async generator cannot retain a resumed request",
            )),
        }
    }
}

#[derive(Debug, PartialEq)]
enum AsyncGeneratorRunCompletion {
    Next(Value),
    ReturnAwaited(Value),
    ResumeReturn(Value),
    ResumeThrow(Value),
    Throw(Value),
}

#[derive(Debug, PartialEq)]
enum AsyncGeneratorRequestTurn {
    AwaitReturn(Value),
    Run(AsyncGeneratorRunCompletion),
}

impl AsyncGeneratorRequestTurn {
    fn classify(completion: AsyncGeneratorCompletion) -> Self {
        match completion {
            AsyncGeneratorCompletion::Return(value) => Self::AwaitReturn(value),
            AsyncGeneratorCompletion::Next(value) => {
                Self::Run(AsyncGeneratorRunCompletion::Next(value))
            }
            AsyncGeneratorCompletion::ReturnAwaited(value) => {
                Self::Run(AsyncGeneratorRunCompletion::ReturnAwaited(value))
            }
            AsyncGeneratorCompletion::ResumeReturn(value) => {
                Self::Run(AsyncGeneratorRunCompletion::ResumeReturn(value))
            }
            AsyncGeneratorCompletion::ResumeThrow(value) => {
                Self::Run(AsyncGeneratorRunCompletion::ResumeThrow(value))
            }
            AsyncGeneratorCompletion::Throw(value) => {
                Self::Run(AsyncGeneratorRunCompletion::Throw(value))
            }
        }
    }
}

#[derive(Debug, PartialEq)]
enum GeneratorAbruptAction {
    Resume(Option<usize>),
    Return(Value),
}

impl GeneratorAbruptAction {
    fn classify(action: Result<CompletionAction, RuntimeError>) -> Result<Self, RuntimeError> {
        match action {
            Ok(CompletionAction::Continue) => Ok(Self::Resume(None)),
            Ok(CompletionAction::Jump(target)) => Ok(Self::Resume(Some(target))),
            Ok(CompletionAction::Return(value)) => Ok(Self::Return(value)),
            Ok(CompletionAction::TailRecur(_) | CompletionAction::TailCall(_)) => {
                Err(RuntimeError::TypeError(
                    "generator cannot tail recur across an abrupt resume".into(),
                ))
            }
            Ok(CompletionAction::Throw(error)) | Err(error) => Err(error),
        }
    }
}

/// Generator instantiation has one suspension boundary, before its body.
fn generator_entry_exit(exit: InterpreterExit) -> (usize, IteratorRecords, HandlerRecords) {
    match exit {
        InterpreterExit::Suspend {
            pc,
            iterators,
            handlers,
        } => (pc, iterators, handlers),
        InterpreterExit::Return(_) | InterpreterExit::Yield { .. } => {
            unreachable!("generator entry contains only instantiation bytecode")
        }
        InterpreterExit::Await { .. } => {
            unreachable!("generator entry cannot contain top-level await")
        }
    }
}

/// A resumed generator can return, yield or await, but cannot pause at entry.
enum GeneratorRunExit {
    Return(Value),
    Yield {
        value: Value,
        pc: usize,
        iterators: IteratorRecords,
        handlers: HandlerRecords,
    },
    Await {
        promise: ObjectId,
        pc: usize,
        handlers: HandlerRecords,
    },
}

impl GeneratorRunExit {
    fn from_interpreter(exit: InterpreterExit) -> Self {
        match exit {
            InterpreterExit::Return(value) => Self::Return(value),
            InterpreterExit::Yield {
                value,
                pc,
                iterators,
                handlers,
            } => Self::Yield {
                value,
                pc,
                iterators,
                handlers,
            },
            InterpreterExit::Await {
                promise,
                pc,
                handlers,
            } => Self::Await {
                promise,
                pc,
                handlers,
            },
            InterpreterExit::Suspend { .. } => {
                unreachable!("ordinary generator execution has no suspend boundary")
            }
        }
    }
}

impl Vm {
    /// Generator function invocation performs parameter initialization now,
    /// then suspends immediately before body evaluation. This makes a direct
    /// eval in a default parameter observable (including its early errors)
    /// at `generatorFunction()` rather than at the first `.next()`.
    #[allow(clippy::too_many_arguments)]
    pub(in super::super) fn initialize_generator(
        &mut self,
        code: SharedBytecode,
        captures: Vec<ObjectId>,
        callee: Value,
        receiver: Value,
        args: Vec<Value>,
        home: Option<ObjectId>,
        closure_with_objects: Vec<Value>,
        entry_payload: execution_payload::CallEntryCharge,
        mut prepared: PreparedFrameBindings,
        mut prepared_maps: PreparedFrameMaps,
    ) -> Result<OwnedGeneratorState, RuntimeError> {
        let _caller_payload = self.charge_execution_payload(
            self.caller_execution_payload_bytes()
                .saturating_add(self.pending_execution_payload_bytes()),
        );
        let _caller_control = self.charge_execution_control(
            self.displaced_control_bytes(execution_payload::ControlTransfer::Generator),
        );
        let base = self.stack.len();
        self.push_generator_entry_roots(&receiver, &closure_with_objects);
        let frame_base = self.stack.len();

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
        let dynamic_eval_outer_bindings = std::mem::take(&mut self.dynamic_eval_outer_bindings);
        let eval_dynamic_slots = std::mem::take(&mut self.eval_dynamic_slots);
        let script_global_slots = std::mem::take(&mut self.script_global_slots);
        let this = std::mem::replace(&mut self.this, receiver);
        let arguments = std::mem::replace(&mut self.arguments, args);
        drop(entry_payload);
        let frame_callee = std::mem::replace(&mut self.callee, callee);
        let completion = std::mem::replace(&mut self.completion, Value::Undefined);
        let completion_empty = std::mem::replace(&mut self.completion_empty, true);
        let active_scopes = std::mem::take(&mut self.active_scopes);
        let active_scope_slots = std::mem::take(&mut self.active_scope_slots);
        let strict = std::mem::replace(&mut self.strict, code.strict);
        let home_object = std::mem::replace(&mut self.home_object, home);
        // A generator is never constructed, so its own `new.target` is
        // `undefined`; whether a direct eval in it may mention `new.target`
        // is a property of its code, not of whoever called `next()`.
        let new_target = std::mem::replace(&mut self.new_target, Value::Undefined);
        let new_target_allowed =
            std::mem::replace(&mut self.new_target_allowed, code.new_target_allowed);
        let variable_scope = std::mem::replace(&mut self.variable_scope, code.variable_scope);
        let variable_scope_lexicals =
            std::mem::replace(&mut self.variable_scope_lexicals, frame_lexicals);
        let with_objects = std::mem::replace(&mut self.with_objects, closure_with_objects);
        let inherited_with_depth =
            std::mem::replace(&mut self.inherited_with_depth, self.with_objects.len());
        let parameter_eval_env = self.parameter_eval_env.take();
        let pending_completions = std::mem::take(&mut self.pending_completions);
        let completion_saves = std::mem::take(&mut self.completion_saves);
        drop(prepared);
        drop(prepared_maps);
        let mut entry_error = None;
        if code.parameter_eval_scope {
            match self.new_parameter_eval_env() {
                Ok(env) => {
                    self.with_objects.push(Value::Object(env));
                    self.inherited_with_depth = self.with_objects.len();
                    self.parameter_eval_env = Some(env);
                }
                Err(error) => entry_error = Some(error),
            }
        }

        let mut iterators = self.iterator_records(Vec::new());
        let outcome = if let Some(error) = entry_error {
            Err(error)
        } else {
            self.interpret(
                &code,
                &mut iterators,
                0,
                None,
                Some(InterpreterSuspensionPoint::Offset(
                    code.generator_entry as usize,
                )),
                None,
            )
        };
        let state = outcome
            .map(generator_entry_exit)
            .and_then(|(pc, iterators, handlers)| {
                debug_assert_eq!(pc, code.generator_entry as usize);
                debug_assert!(iterators.is_empty());
                debug_assert!(handlers.is_empty());
                let mut storage = self.prepare_suspension_storage(frame_base, true)?;
                let stack = storage.take_stack(self);
                let pending = storage.take_pending(self);
                let state = GeneratorState::Suspended {
                    disposables: std::mem::take(&mut self.disposables).into_vec(),
                    dispose_marks: std::mem::take(&mut self.dispose_marks).into_vec(),
                    code,
                    pc,
                    stack,
                    bindings: std::mem::take(&mut self.bindings),
                    cells: std::mem::take(&mut self.cells),
                    this: std::mem::replace(&mut self.this, Value::Undefined),
                    args: std::mem::take(&mut self.arguments),
                    completion: std::mem::replace(&mut self.completion, Value::Undefined),
                    completion_empty: std::mem::replace(&mut self.completion_empty, true),
                    active_scopes: std::mem::take(&mut self.active_scopes),
                    iterators: iterators.into_vec(),
                    handlers: Vec::new(),
                    pending_completions: pending,
                    completion_saves: Vec::new(),
                    async_delegate: None,
                    delegate: None,
                    dynamic_bindings: std::mem::take(&mut self.dynamic_eval_bindings),
                    home: std::mem::take(&mut self.home_object),
                    callee: std::mem::replace(&mut self.callee, Value::Undefined),
                    with_objects: std::mem::take(&mut self.with_objects),
                };
                Ok(storage.into_generator(self, state))
            });
        let state = state.and_then(|state| state.admit(self));
        self.with_objects = with_objects;
        self.inherited_with_depth = inherited_with_depth;
        self.parameter_eval_env = parameter_eval_env;
        self.disposables = disposables;
        self.dispose_marks = dispose_marks;
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
        self.new_target = new_target;
        self.new_target_allowed = new_target_allowed;
        self.variable_scope = variable_scope;
        self.variable_scope_lexicals = variable_scope_lexicals;
        self.pending_completions = pending_completions;
        self.completion_saves = completion_saves;
        self.stack.truncate(base);
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
        self.generator_resume_with_input(receiver, value, async_target, abrupt, &mut None)
    }

    pub(in super::super) fn generator_resume_with_input(
        &mut self,
        receiver: &Value,
        value: Option<Value>,
        async_target: Option<ObjectId>,
        abrupt: Option<Completion>,
        input_storage: &mut Option<host_scheduling::SharedSchedulingCharge>,
    ) -> Result<Value, RuntimeError> {
        let mut result = (|| {
            if input_storage.is_none() {
                let bytes = value
                    .as_ref()
                    .map_or(0, host_scheduling::value_payload_bytes)
                    .saturating_add(
                        abrupt
                            .as_ref()
                            .map_or(0, generator_input::scalar_completion_bytes),
                    );
                *input_storage = self
                    .argument_list_owner()?
                    .as_ref()
                    .map(|owner| owner.reserve_peer(bytes))
                    .transpose()?;
            }
            if let Some(owner) = input_storage.as_ref() {
                owner.reserve_peer(0)?;
                if let Some(scope) = self.host_scheduling.as_ref() {
                    if !owner.belongs_to(scope) {
                        return Err(scope.fail());
                    }
                }
            }
            self.generator_resume_inner(receiver, value, async_target, abrupt, input_storage)
        })();
        if input_storage.is_some() {
            let bytes = generator_input::raw_result_bytes(&result);
            if let Err(error) = generator_input::retain_generator_payload(input_storage, bytes) {
                result = Err(error);
                generator_input::release_generator_payload(input_storage, 0);
            }
        }
        self.reconcile_scheduling_ownership();
        result
    }

    fn generator_resume_inner(
        &mut self,
        receiver: &Value,
        value: Option<Value>,
        async_target: Option<ObjectId>,
        abrupt: Option<Completion>,
        input_storage: &mut Option<host_scheduling::SharedSchedulingCharge>,
    ) -> Result<Value, RuntimeError> {
        let Value::Object(generator) = receiver else {
            return Err(RuntimeError::TypeError(
                "Generator next requires a generator".into(),
            ));
        };
        if self.active_generator_delegations.contains(generator) {
            return Err(RuntimeError::TypeError(
                "Generator is already running".into(),
            ));
        }
        let mut prepared = match self.heap.generator_execution_code(*generator)? {
            Some((code, start)) => Some(self.prepare_frame_bindings(
                &code,
                if start {
                    FrameBindingKind::Function
                } else {
                    FrameBindingKind::ResumedGenerator
                },
            )?),
            None => None,
        };
        let (initial_bindings, frame_metadata, frame_lexicals) = prepared
            .as_mut()
            .map(PreparedFrameBindings::take_parts)
            .unwrap_or_default();
        let mut prepared_scopes = self.prepare_generator_scope_slots(*generator)?;
        let mut prepared_maps = self.prepare_generator_frame_maps(*generator)?;
        let mut prepared_resume = self.prepare_generator_resume(*generator)?;
        let state = self.heap.take_generator_state(*generator)?;
        prepared_resume.retain_state(&state);
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
            frame_with_objects,
            frame_disposables,
            frame_dispose_marks,
        ) = match state {
            GeneratorState::Running => {
                self.heap
                    .set_generator_state(*generator, GeneratorState::Running)
                    .expect(
                        "restoring Running after a verified take does not increase frame bytes",
                    );
                return Err(RuntimeError::TypeError(
                    "Generator is already running".into(),
                ));
            }
            GeneratorState::Done => {
                self.heap
                    .set_generator_state(*generator, GeneratorState::Done)
                    .expect("restoring Done after a verified take does not increase frame bytes");
                drop(value);
                if let Some(Completion::Throw(error)) = abrupt {
                    return Err(error);
                }
                if let Some(Completion::Return(value)) = abrupt {
                    return if input_storage.is_some() {
                        self.iterator_result_retained(
                            NativeValue::from_retained_parts(value, input_storage.take()),
                            true,
                        )
                    } else {
                        self.iterator_result(value, true)
                    };
                }
                generator_input::release_generator_payload(input_storage, 0);
                return self.iterator_result(Value::Undefined, true);
            }
            GeneratorState::Start {
                code,
                captures,
                callee,
                receiver,
                args,
                home,
                with_objects,
            } => {
                drop(value);
                if abrupt.is_none() {
                    generator_input::release_generator_payload(input_storage, 0);
                }
                prepared_maps.adopt_captures(captures);
                let mut bindings = initial_bindings;
                let variable_scope = code.variable_scope;
                let variable_scope_lexicals = frame_lexicals;
                if let Some(slot) = code.self_slot {
                    bindings[slot as usize] = Some(callee.clone());
                }
                (
                    code,
                    0,
                    None,
                    Vec::new(),
                    bindings,
                    prepared_maps.take_cells(),
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
                    with_objects,
                    Vec::new(),
                    Vec::new(),
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
                disposables,
                dispose_marks,
                dynamic_bindings,
                home,
                callee,
                with_objects,
            } => {
                prepared_maps.adopt_saved_maps(cells, dynamic_bindings);
                let variable_scope = code.variable_scope;
                let variable_scope_lexicals = frame_lexicals;
                (
                    code,
                    pc,
                    value,
                    stack,
                    bindings,
                    prepared_maps.take_cells(),
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
                    prepared_maps.take_dynamic(),
                    with_objects,
                    disposables,
                    dispose_marks,
                )
            }
        };
        self.heap
            .set_generator_state(*generator, GeneratorState::Running)
            .expect("Running replaces a verified generator frame without increasing its bytes");

        let caller_payload = self.charge_execution_payload(
            self.caller_execution_payload_bytes()
                .saturating_add(self.pending_execution_payload_bytes()),
        );
        let caller_control = self.charge_execution_control(
            self.displaced_control_bytes(execution_payload::ControlTransfer::Generator),
        );
        let base = self.stack.len();
        let remaining_instructions = self.remaining_instructions;
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
        // The resumer's `with` objects stay rooted while the generator's own
        // are in scope. Both sets sit below the frame's base: everything above
        // it is the generator's operand stack, which is saved again at the
        // next yield and must hold only the frame's own operands.
        self.stack.extend(self.with_objects.iter().cloned());
        self.stack.extend(
            self.disposables
                .iter()
                .flat_map(DisposableResource::values)
                .filter_map(Value::object_id)
                .map(Value::Object),
        );
        self.stack.extend(frame_with_objects.iter().cloned());
        self.push_pending_execution_roots();
        let frame_base = self.stack.len();
        self.stack.extend(frame_stack);

        let with_objects = std::mem::replace(&mut self.with_objects, frame_with_objects);
        // The objects the function itself closed over (and its parameter
        // environment) are inherited; a `with` statement the body is inside
        // of is not.
        let inherited_with_depth = std::mem::replace(
            &mut self.inherited_with_depth,
            (code.with_depth as usize + usize::from(code.parameter_eval_scope))
                .min(self.with_objects.len()),
        );
        let disposables = std::mem::take(&mut self.disposables);
        let dispose_marks = std::mem::take(&mut self.dispose_marks);
        self.disposables = self.owned_records(frame_disposables);
        self.dispose_marks = self.owned_records(frame_dispose_marks);
        let bindings = std::mem::replace(&mut self.bindings, frame_bindings);
        let binding_metadata = std::mem::replace(&mut self.binding_metadata, frame_metadata);
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
        let active_scope_slots =
            std::mem::replace(&mut self.active_scope_slots, prepared_scopes.take());
        let strict = std::mem::replace(&mut self.strict, code.strict);
        let home_object = std::mem::replace(&mut self.home_object, frame_home);
        let new_target = std::mem::replace(&mut self.new_target, Value::Undefined);
        let new_target_allowed =
            std::mem::replace(&mut self.new_target_allowed, code.new_target_allowed);
        let callee = std::mem::replace(&mut self.callee, frame_callee);
        // A generator body resumes wherever its request happened to come
        // from -- typically a Promise job with no ambient script/module
        // identity -- so an `import()` inside it must resolve against the
        // module or script that created the generator function, exactly as
        // an ordinary call of a module closure does.
        let active_module_name = prepared_resume.install_module(self);
        let module_name_control =
            self.charge_execution_control(active_module_name.as_ref().map_or(0, String::capacity));
        let variable_scope = std::mem::replace(&mut self.variable_scope, frame_variable_scope);
        let variable_scope_lexicals = std::mem::replace(
            &mut self.variable_scope_lexicals,
            frame_variable_scope_lexicals,
        );
        let pending_completions = std::mem::replace(
            &mut self.pending_completions,
            prepared_resume.take_completions(frame_pending_completions),
        );
        let completion_saves =
            std::mem::replace(&mut self.completion_saves, frame_completion_saves);
        drop(prepared);
        drop(prepared_scopes);
        drop(prepared_maps);
        let mut iterators = self.iterator_records(frame_iterators);
        let mut handlers = self.handler_records(frame_handlers);
        drop(prepared_resume);
        let mut outcome = if let Some(completion) = abrupt {
            // `GeneratorState` keeps handler offsets relative to its saved
            // operand stack. `resolve_completion` runs before `interpret`,
            // so inflate them here, then return them to their stored form
            // before the resumed interpreter takes ownership.
            for handler in &mut handlers {
                handler.stack_depth += frame_base;
            }
            let action = self.resolve_completion_with_input(
                &code,
                &mut handlers,
                &mut iterators,
                completion,
                input_storage,
            );
            for handler in &mut handlers {
                handler.stack_depth -= frame_base;
            }
            match GeneratorAbruptAction::classify(action) {
                Ok(GeneratorAbruptAction::Resume(target)) => self.interpret_with_input(
                    &code,
                    &mut iterators,
                    target.unwrap_or(pc),
                    InterpreterResumeInput {
                        value: None,
                        storage: Some(&mut *input_storage),
                    },
                    None,
                    Some((handlers, frame_base)),
                ),
                Ok(GeneratorAbruptAction::Return(value)) => Ok(InterpreterExit::Return(value)),
                Err(error) => Err(error),
            }
        } else {
            self.interpret_with_input(
                &code,
                &mut iterators,
                pc,
                InterpreterResumeInput {
                    value: resume_value,
                    storage: Some(&mut *input_storage),
                },
                None,
                Some((handlers, frame_base)),
            )
        };
        let output_bytes = generator_input::interpreter_result_bytes(&outcome);
        if let Err(error) = generator_input::retain_generator_payload(input_storage, output_bytes) {
            outcome = Err(error);
            generator_input::release_generator_payload(input_storage, 0);
        }
        let mut suspended_async = None;
        // A `yield*` over a synchronous delegate yields the delegate's own
        // result object, which is returned without wrapping it again.
        let mut yielded_delegate_result = false;

        let mut suspension = None;
        let outcome = outcome
            .map(GeneratorRunExit::from_interpreter)
            .and_then(|mut exit| {
                let pending = matches!(exit, GeneratorRunExit::Yield { .. });
                if pending
                    || (async_target.is_some() && matches!(exit, GeneratorRunExit::Await { .. }))
                {
                    match self.prepare_suspension_storage(frame_base, pending) {
                        Ok(storage) => suspension = Some(storage),
                        Err(error) => {
                            // Return yielded iterator records to the common error
                            // cleanup before restoring the caller's frame.
                            if let GeneratorRunExit::Yield {
                                iterators: records, ..
                            } = &mut exit
                            {
                                iterators = std::mem::take(records);
                            }
                            return Err(error);
                        }
                    }
                }
                Ok(exit)
            });
        let (next_state, result) = match outcome {
            Ok(GeneratorRunExit::Return(value)) => {
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
                let result = match close {
                    Ok(()) => Ok((value, true)),
                    Err(error) => {
                        drop(value);
                        generator_input::release_generator_payload(input_storage, 0);
                        Err(error)
                    }
                };
                (GeneratorState::Done, result)
            }
            Ok(GeneratorRunExit::Yield {
                value,
                pc,
                iterators,
                handlers,
            }) => {
                let storage = suspension
                    .as_mut()
                    .expect("yield prepared snapshot storage");
                let stack = storage.take_stack(self);
                let pending = storage.take_pending(self);
                // Every compiled async yield first awaits its operand or
                // delegate's next result. resume_async_continuation captures
                // asynchronous delegation after that await; this direct exit
                // only captures synchronous delegation.
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
                    disposables: std::mem::take(&mut self.disposables).into_vec(),
                    dispose_marks: std::mem::take(&mut self.dispose_marks).into_vec(),
                    code,
                    pc,
                    stack,
                    bindings: std::mem::take(&mut self.bindings),
                    cells: std::mem::take(&mut self.cells),
                    this: std::mem::replace(&mut self.this, Value::Undefined),
                    args: std::mem::take(&mut self.arguments),
                    completion: std::mem::replace(&mut self.completion, Value::Undefined),
                    completion_empty: std::mem::replace(&mut self.completion_empty, true),
                    active_scopes: std::mem::take(&mut self.active_scopes),
                    iterators: iterators.into_vec(),
                    handlers: handlers.into_vec(),
                    pending_completions: pending,
                    completion_saves: std::mem::take(&mut self.completion_saves),
                    async_delegate: None,
                    delegate,
                    dynamic_bindings: std::mem::take(&mut self.dynamic_eval_bindings),
                    home: std::mem::take(&mut self.home_object),
                    callee: std::mem::replace(&mut self.callee, Value::Undefined),
                    with_objects: std::mem::take(&mut self.with_objects),
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
            Ok(GeneratorRunExit::Await {
                promise,
                pc,
                handlers,
            }) => {
                if let Some(target) = async_target {
                    let execution = self.capture_prepared_execution(
                        suspension.take().expect("await prepared snapshot storage"),
                    );
                    suspended_async = Some((
                        AsyncContinuation {
                            gc_live: false,
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
        let mut result = result;
        let result_bytes = generator_input::generator_result_bytes(&result);
        if let Err(error) = generator_input::retain_generator_payload(input_storage, result_bytes) {
            result = Err(error);
            generator_input::release_generator_payload(input_storage, 0);
        }
        let next_state = self.own_generator_state(next_state);
        drop(suspension);
        match &result {
            Ok((value, _)) | Err(RuntimeError::Thrown(value)) => {
                self.stack.extend(value.object_id().map(Value::Object));
            }
            Err(_) => {}
        }
        let stored = next_state.store(self, *generator);
        // A failed store must still hand the caller's frame back: leaving the
        // swapped-out execution state in place would unbalance the interpreter
        // (for example an empty `dynamic_eval_outer_bindings` stack) on the
        // very next call return.
        self.with_objects = with_objects;
        self.inherited_with_depth = inherited_with_depth;
        self.disposables = disposables;
        self.dispose_marks = dispose_marks;
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
        self.new_target = new_target;
        self.new_target_allowed = new_target_allowed;
        self.callee = callee;
        self.variable_scope = variable_scope;
        self.variable_scope_lexicals = variable_scope_lexicals;
        self.remaining_instructions = remaining_instructions;
        self.active_module_name = active_module_name;
        self.stack.truncate(base);
        drop(caller_payload);
        drop(caller_control);
        drop(module_name_control);
        stored?;
        if let Some((state, promise)) = suspended_async {
            self.suspend_async_await(state, promise)?;
            return Ok(Value::Undefined);
        }
        let (value, done) = result?;
        if yielded_delegate_result {
            return Ok(value);
        }
        if input_storage.is_some() {
            self.iterator_result_retained(
                NativeValue::from_retained_parts(value, input_storage.take()),
                done,
            )
        } else {
            self.iterator_result(value, done)
        }
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
        let owner = self.argument_list_owner()?;
        if !matches!(result, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "yield* delegate method must return an object".into(),
            ));
        }
        let result = NativeValue::adopt(result, owner.as_ref())?;
        result.with_value(self, |vm, result| {
            let done = vm.get_property(result, &"done".into())?;
            let done = NativeValue::adopt(done, owner.as_ref())?;
            let complete = vm
                .to_boolean(done.value())
                .expect("the delegate getter returned a live value in this realm");
            drop(done);
            if complete {
                let value = vm.get_property(result, &"value".into())?;
                let value = NativeValue::adopt(value, owner.as_ref())?;
                Ok(SyncDelegateStep::Done(value.into_value()))
            } else {
                Ok(SyncDelegateStep::Yield(result.clone()))
            }
        })
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
        let mut state = self
            .heap
            .take_generator_state(generator)
            .expect("the delegate caller retains its validated generator");
        let finished = match &mut state {
            GeneratorState::Suspended {
                pc,
                stack,
                delegate,
                iterators,
                ..
            } => delegate.take().map(|delegate| {
                *pc = delegate.exit_pc;
                iterators.retain(|active| active != &delegate.record);
                *stack
                    .last_mut()
                    .expect("yield* delegation keeps its iterator record") = value;
                delegate.record
            }),
            _ => None,
        };
        // A stale delegate answer must leave the original state resumable.
        let base = self.stack.len();
        self.stack.extend(finished.iter().cloned());
        let stored = self.with_roots(|heap| heap.set_generator_state(generator, state));
        self.stack.truncate(base);
        stored.expect("finishing delegation removes iterator and delegate edges from the retained frame; its replacement value adds at most one edge");
        let record = finished.ok_or(RuntimeError::Unsupported(
            "lost synchronous yield* delegation state",
        ))?;
        let record = record
            .object_id()
            .expect("yield* retains an object iterator record");
        self.heap
            .set(record, "done", Value::Bool(true))
            .expect("the private delegate record replaces its existing Boolean without growing");
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
        let owner = self.argument_list_owner()?;
        let roots = RealmCallRoots::new(2, owner.as_ref())?;
        receiver
            .object_id()
            .into_iter()
            .try_for_each(|id| roots.push(&self.heap, id))?;
        value
            .object_id()
            .into_iter()
            .try_for_each(|id| roots.push(&self.heap, id))?;
        let result = self.with_realm_call_roots(roots, |vm| {
            let generator = receiver.object_id().ok_or_else(|| {
                RuntimeError::TypeError("Generator return requires a generator".into())
            })?;
            if vm.active_generator_delegations.contains(&generator) {
                return Err(RuntimeError::TypeError(
                    "Generator is already running".into(),
                ));
            }
            let state = vm.heap.take_generator_state(generator)?;
            let Some((record, _)) = Self::sync_yield_star_delegate(&state) else {
                vm.heap
                    .set_generator_state(generator, state)
                    .expect("restoring the unchanged, verified generator frame cannot allocate");
                return Ok(None);
            };
            vm.heap
                .set_generator_state(generator, state)
                .expect("restoring the unchanged, verified generator frame cannot allocate");
            vm.begin_generator_delegation(generator)?;
            let base = vm.stack.len();
            vm.stack.push(value.clone());
            let step = (|| {
                let iterator = vm
                    .heap
                    .get_own(
                        record.object_id().expect("a delegate record is an object"),
                        "iterator",
                    )
                    .expect("the suspended frame retains its initialized private delegate record")
                    .expect("the private delegate record owns its iterator field");
                vm.stack.push(iterator.clone());
                let method = vm.get_method(&iterator, &"return".into())?;
                if method == Value::Undefined {
                    return Ok(None);
                }
                let result = vm.call_native(method, iterator, vec![value.clone()], false)?;
                vm.sync_delegate_step(result).map(Some)
            })();
            vm.end_generator_delegation(generator);
            vm.stack.truncate(base);
            if let Some(owner) = &owner {
                owner.reserve_peer(0)?;
            }
            Ok(match step {
                Ok(None) => {
                    // The return completion leaves the generator directly, so
                    // the frame must not close the delegate a second time.
                    let record = record
                        .object_id()
                        .expect("yield* retains an object iterator record");
                    vm.heap.set(record, "done", Value::Bool(true)).expect(
                        "the private delegate record replaces its existing Boolean without growing",
                    );
                    None
                }
                Ok(Some(SyncDelegateStep::Yield(result))) => Some(Ok(result)),
                Ok(Some(SyncDelegateStep::Done(inner))) => {
                    vm.stack.push(inner.clone());
                    vm.finish_sync_delegation(generator, inner.clone()).expect(
                        "the executing delegate callback cannot resume or replace its saved frame",
                    );
                    let result =
                        vm.generator_resume(receiver, None, None, Some(Completion::Return(inner)));
                    vm.stack.pop();
                    Some(result)
                }
                Err(error) => Some(vm.throw_into_generator(receiver, &record, error)),
            })
        });
        if let Some(owner) = owner {
            owner.reserve_peer(0)?;
        }
        result
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
        let result = {
            let record = record
                .object_id()
                .expect("yield* retains an object iterator record");
            self.heap.set(record, "done", Value::Bool(true)).expect(
                "the private delegate record replaces its existing Boolean without growing",
            );
            self.generator_resume(receiver, None, None, Some(Completion::Throw(error)))
        };
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn generator_throw(
        &mut self,
        receiver: &Value,
        value: Value,
    ) -> Result<Value, RuntimeError> {
        let owner = self.argument_list_owner()?;
        let roots = RealmCallRoots::new(2, owner.as_ref())?;
        receiver
            .object_id()
            .into_iter()
            .try_for_each(|id| roots.push(&self.heap, id))?;
        value
            .object_id()
            .into_iter()
            .try_for_each(|id| roots.push(&self.heap, id))?;
        let result = self.with_realm_call_roots(roots, |vm| {
            let generator = receiver.object_id().ok_or_else(|| {
                RuntimeError::TypeError("Generator throw requires a generator".into())
            })?;
            if vm.active_generator_delegations.contains(&generator) {
                return Err(RuntimeError::TypeError(
                    "Generator is already running".into(),
                ));
            }
            let state = vm.heap.take_generator_state(generator)?;
            let Some((record, _)) = Self::sync_yield_star_delegate(&state) else {
                vm.heap
                    .set_generator_state(generator, state)
                    .expect("restoring the unchanged, verified generator frame cannot allocate");
                return vm.generator_resume(
                    receiver,
                    None,
                    None,
                    Some(Completion::Throw(RuntimeError::Thrown(value))),
                );
            };
            vm.heap
                .set_generator_state(generator, state)
                .expect("restoring the unchanged, verified generator frame cannot allocate");
            vm.begin_generator_delegation(generator)?;
            let base = vm.stack.len();
            vm.stack.push(value.clone());
            let step = (|| {
                let iterator = vm
                    .heap
                    .get_own(
                        record.object_id().expect("a delegate record is an object"),
                        "iterator",
                    )
                    .expect("the suspended frame retains its initialized private delegate record")
                    .expect("the private delegate record owns its iterator field");
                vm.stack.push(iterator.clone());
                let method = vm.get_method(&iterator, &"throw".into())?;
                if method == Value::Undefined {
                    // A delegate that cannot take the throw still gets to clean
                    // up; the protocol violation is thrown once it has.
                    vm.iterator_close(&record)?;
                    return Err(RuntimeError::TypeError(
                        "yield* iterator does not provide a throw method".into(),
                    ));
                }
                let result = vm.call_native(method, iterator, vec![value.clone()], false)?;
                vm.sync_delegate_step(result)
            })();
            vm.end_generator_delegation(generator);
            vm.stack.truncate(base);
            if let Some(owner) = &owner {
                owner.reserve_peer(0)?;
            }
            match step {
                Ok(SyncDelegateStep::Yield(result)) => Ok(result),
                Ok(SyncDelegateStep::Done(inner)) => {
                    vm.stack.push(inner.clone());
                    vm.finish_sync_delegation(generator, inner).expect(
                        "the executing delegate callback cannot resume or replace its saved frame",
                    );
                    let result = vm.generator_next(receiver, None, None);
                    vm.stack.pop();
                    result
                }
                Err(error) => vm.throw_into_generator(receiver, &record, error),
            }
        });
        if let Some(owner) = owner {
            owner.reserve_peer(0)?;
        }
        result
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
    fn async_generator_delegate_request(
        &mut self,
        generator: ObjectId,
        target: ObjectId,
        abrupt: DelegatedAbrupt,
        input: &NativeValue,
    ) -> Result<Option<Value>, RuntimeError> {
        let owner = input.owner_view()?;
        input.with_value(self, |vm, value| {
            vm.async_generator_delegate_request_value(
                generator,
                target,
                abrupt,
                value,
                owner.as_ref(),
            )
        })
    }

    fn async_generator_delegate_request_value(
        &mut self,
        generator: ObjectId,
        target: ObjectId,
        abrupt: DelegatedAbrupt,
        value: &Value,
        owner: Option<&host_scheduling::SharedSchedulingCharge>,
    ) -> Result<Option<Value>, RuntimeError> {
        let state = self
            .heap
            .take_generator_state(generator)
            .expect("the queued request retains its validated async generator");
        let Some((record, _)) = Self::yield_star_delegate(&state) else {
            self.heap
                .set_generator_state(generator, state)
                .expect("restoring the unchanged, verified generator frame cannot allocate");
            return Ok(None);
        };
        self.heap
            .set_generator_state(generator, state)
            .expect("restoring the unchanged, verified generator frame cannot allocate");
        let record = record
            .object_id()
            .expect("yield* retains an object iterator record");
        let iterator = self
            .heap
            .get_own(record, "iterator")
            .expect("the suspended frame retains its initialized private delegate record")
            .expect("the private delegate record owns its iterator field");
        let (kind, name) = match abrupt {
            DelegatedAbrupt::Return => (AsyncGeneratorDelegateKind::Return, "return"),
            DelegatedAbrupt::Throw => (AsyncGeneratorDelegateKind::Throw, "throw"),
        };
        let method_epoch = self.throw_epoch;
        let method = match self.get_method(&iterator, &name.into()) {
            Ok(method) => method,
            Err(error) => {
                // A throwing `return`/`throw` getter is an abrupt completion
                // of the `yield*` like any other failed delegate call.
                let origin = self.caught_rejection_origin(method_epoch);
                let error = self.error_value(error)?;
                self.with_rejection_origin(Some(origin), |vm| {
                    vm.finish_async_generator_delegate(generator, target, kind, error, false)
                })?;
                return Ok(Some(Value::Undefined));
            }
        };
        if method == Value::Undefined {
            if matches!(kind, AsyncGeneratorDelegateKind::Throw) {
                // AsyncIteratorClose runs before the protocol TypeError. A
                // failed close replaces that error, and both completions
                // must enter the generator's enclosing catch/finally.
                let close_epoch = self.throw_epoch;
                let close = self
                    .get_method(&iterator, &"return".into())
                    .and_then(|method| {
                        if method == Value::Undefined {
                            Ok(None)
                        } else {
                            self.call_native(method, iterator, Vec::new(), false)
                                .map(Some)
                        }
                    });
                if matches!(
                    self.heap
                        .get_own(record, "asyncFromSync")
                        .expect("the suspended frame retains its private delegate record"),
                    Some(Value::Bool(true))
                ) {
                    // AsyncFromSyncIterator.throw uses IteratorClose, without
                    // assimilating the return value. The outer yield* awaits
                    // the wrapper's rejected Promise before entering handlers.
                    let error = match close {
                        Err(error) => self.error_value(error)?,
                        Ok(Some(result)) if !matches!(result, Value::Object(_)) => self
                            .error_object(
                                "TypeError",
                                "yield* delegate method must return an object".into(),
                            )?,
                        Ok(_) => self.error_object(
                            "TypeError",
                            "yield* iterator does not provide a throw method".into(),
                        )?,
                    };
                    self.enqueue_promise_job(PromiseJob::AsyncGeneratorDelegate {
                        generator,
                        target,
                        kind: AsyncGeneratorDelegateKind::CloseMissingThrow,
                        value: error,
                        fulfilled: false,
                        origin: self.caught_rejection_origin(close_epoch),
                    });
                    return Ok(Some(Value::Undefined));
                }
                match close {
                    Ok(Some(result)) => {
                        let await_epoch = self.throw_epoch;
                        if let Err(error) = self.start_async_generator_await(
                            generator,
                            target,
                            AsyncGeneratorDelegateKind::CloseMissingThrow,
                            result,
                        ) {
                            if matches!(
                                &error,
                                RuntimeError::SchedulingLimit
                                    | RuntimeError::Heap(_)
                                    | RuntimeError::InstructionLimit
                            ) {
                                return Err(error);
                            }
                            let origin = self.caught_rejection_origin(await_epoch);
                            let error = self.error_value(error)?;
                            self.with_rejection_origin(Some(origin), |vm| {
                                vm.throw_at_async_delegate_exit(generator, target, error)
                            })?;
                        }
                    }
                    Ok(None) => self.finish_async_generator_delegate(
                        generator,
                        target,
                        AsyncGeneratorDelegateKind::CloseMissingThrow,
                        Value::Object(record),
                        true,
                    )?,
                    Err(error) => {
                        let origin = self.caught_rejection_origin(close_epoch);
                        let error = self.error_value(error)?;
                        self.with_rejection_origin(Some(origin), |vm| {
                            vm.throw_at_async_delegate_exit(generator, target, error)
                        })?;
                    }
                }
                return Ok(Some(Value::Undefined));
            }
            // GetMethod already observed the delegate's `return` property.
            // The yield* step for a delegate without `return` awaits the
            // operand once more and then returns it: the completion still
            // crosses the outer generator's finally records. The delegate
            // itself is not closed: unwinding the generator would otherwise
            // ask it for `return` a second time.
            let value = NativeValue::copied(value, owner)?;
            self.heap.set(record, "done", Value::Bool(true)).expect(
                "the private delegate record replaces its existing Boolean without growing",
            );
            let await_epoch = self.throw_epoch;
            return match self.start_async_generator_await_owned(
                generator,
                target,
                AsyncGeneratorDelegateKind::AwaitReturnNoMethod,
                value,
            ) {
                Ok(()) => Ok(Some(Value::Undefined)),
                Err(
                    error @ (RuntimeError::SchedulingLimit
                    | RuntimeError::Heap(_)
                    | RuntimeError::InstructionLimit),
                ) => Err(error),
                // PromiseResolve threw: the throw is raised at the `yield*`
                // at once, with no turn in between.
                Err(error) => {
                    let origin = self.caught_rejection_origin(await_epoch);
                    let reason = self.error_value(error)?;
                    self.publish_rejection_origin(origin);
                    self.generator_resume(
                        &Value::Object(generator),
                        None,
                        Some(target),
                        Some(Completion::Throw(RuntimeError::Thrown(reason))),
                    )
                    .map(Some)
                }
            };
        }
        // A delegate method that throws (or whose result cannot be turned
        // into a promise) is an abrupt completion of the `yield*`: like a
        // rejected result it is thrown at the `yield*` site.
        let async_from_sync = matches!(
            self.heap
                .get_own(record, "asyncFromSync")
                .expect("the suspended frame retains its private delegate record"),
            Some(Value::Bool(true))
        );
        let called_epoch = self.throw_epoch;
        let argument = NativeValue::copied(value, owner)?;
        let called = argument_list_storage::ArgumentValues::single_retained(self, argument)?
            .call_native(self, method, iterator, false)
            .and_then(|result| {
                if async_from_sync {
                    // Over a synchronous iterator the delegate is the
                    // AsyncFromSyncIterator: `return` and `throw` results go
                    // through its continuation (`throw` closes an unfinished
                    // iterator whose value rejects, `return` never does).
                    self.async_from_sync_continue(
                        record,
                        result,
                        matches!(kind, AsyncGeneratorDelegateKind::Throw),
                    )
                } else {
                    self.promise_resolve(result)
                }
            });
        let promise = match called {
            Ok(promise) => promise
                .object_id()
                .expect("Promise.resolve returns a Promise"),
            Err(error) => {
                let origin = self.caught_rejection_origin(called_epoch);
                let error = self.error_value(error)?;
                self.with_rejection_origin(Some(origin), |vm| {
                    vm.finish_async_generator_delegate(generator, target, kind, error, false)
                })?;
                return Ok(Some(Value::Undefined));
            }
        };
        let work = self.prepare_continuation_await(
            promise,
            AwaitContinuation::GeneratorDelegate {
                generator,
                target,
                kind,
            },
            0,
        )?;
        work.publish_and_handle(self, promise)?;
        Ok(Some(Value::Undefined))
    }

    pub(in super::super) fn set_async_generator_status(
        &mut self,
        generator: ObjectId,
        status: AsyncGeneratorStatus,
    ) -> Result<(), RuntimeError> {
        if self.heap.async_generator_control_ref(generator)?.is_none() {
            return Err(RuntimeError::TypeError(
                "Async generator receiver required".into(),
            ));
        }
        self.heap.set_async_generator_status(generator, status)?;
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
        self.complete_async_generator_request_with_input(generator, target, status, None)
    }

    fn complete_async_generator_request_with_input(
        &mut self,
        generator: ObjectId,
        target: ObjectId,
        status: PromiseStatus,
        input: Option<host_scheduling::SharedSchedulingCharge>,
    ) -> Result<(), RuntimeError> {
        let retired = (|| {
            if let Some(input) = &input {
                input.reserve_peer(0)?;
                if let Some(scope) = &self.host_scheduling {
                    if !input.belongs_to(scope) {
                        return Err(scope.fail());
                    }
                }
            }
            let control = self
                .heap
                .async_generator_control_ref(generator)?
                .ok_or_else(|| {
                    RuntimeError::TypeError("Async generator receiver required".into())
                })?;
            let request = control
                .requests
                .front()
                .ok_or(RuntimeError::Unsupported("missing async generator request"))?;
            if request.target != target {
                return Err(RuntimeError::Unsupported(
                    "async generator request completed out of order",
                ));
            }
            if control
                .requests
                .iter()
                .skip(1)
                .any(|queued| queued.id <= request.id)
            {
                return Err(RuntimeError::Unsupported(
                    "async generator queue identifiers are not FIFO",
                ));
            }
            let next = if self.heap.generator_state_is_done(generator)? {
                AsyncGeneratorStatus::Completed
            } else {
                AsyncGeneratorStatus::SuspendedYield
            };
            self.heap
                .pop_async_generator_request(generator, next)
                .expect("removing a verified async generator head cannot allocate");
            Ok(())
        })();
        if let Err(error) = retired {
            drop(status);
            drop(input);
            return Err(error);
        }
        self.settle_promise_with_input_charge(target, status, input)
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
                .async_generator_control_ref(generator)?
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
            let head = control
                .requests
                .front()
                .expect("checked async generator queue is non-empty");
            let status = control.status;
            let request = head.try_snapshot(control.owner.as_ref())?;
            let (origin, origin_storage) = request.origin.copied_value()?;
            let owner = request
                .value_storage
                .as_ref()
                .map(|storage| storage.reserve_peer(0))
                .transpose()?;
            let roots = RealmCallRoots::new(
                2 + usize::from(request.completion.value().object_id().is_some()),
                owner.as_ref(),
            )?;
            roots.push(&self.heap, generator)?;
            roots.push(&self.heap, request.target)?;
            if let Some(object) = request.completion.value().object_id() {
                roots.push(&self.heap, object)?;
            }
            let advance = self.with_realm_call_roots(roots, |vm| {
                vm.with_leased_rejection_origin(Some(origin), origin_storage, |vm| {
                    vm.resume_async_generator_request(generator, status, request)
                })
            })?;
            if !advance {
                return Ok(());
            }
        }
    }

    /// Returns whether the scheduler can immediately inspect its next head.
    fn resume_async_generator_request(
        &mut self,
        generator: ObjectId,
        status: AsyncGeneratorStatus,
        request: AsyncGeneratorRequest,
    ) -> Result<bool, RuntimeError> {
        let target = request.target;
        let mut input_storage = request.value_storage;
        let completion = request.completion;
        if status == AsyncGeneratorStatus::Completed {
            let completion = match CompletedAsyncGeneratorRequest::classify(completion)? {
                CompletedAsyncGeneratorRequest::Next => {
                    generator_input::release_generator_payload(&mut input_storage, 0);
                    PromiseStatus::Fulfilled(self.iterator_result(Value::Undefined, true)?)
                }
                CompletedAsyncGeneratorRequest::Return(value) => {
                    // AsyncGeneratorAwaitReturn: even a completed
                    // generator awaits the value it is asked to return
                    // (a rejection or a broken `constructor` rejects
                    // this request), so it settles on a later turn.
                    let input = NativeValue::from_retained_parts(value, input_storage.take());
                    let result = self.iterator_result_retained(input, true)?;
                    return self
                        .await_async_generator_yield(generator, target, result)
                        .map(|()| false);
                }
                CompletedAsyncGeneratorRequest::Throw(value) => PromiseStatus::Rejected(value),
            };
            self.complete_async_generator_request_with_input(
                generator,
                target,
                completion,
                input_storage.take(),
            )?;
            return Ok(true);
        }

        // A return or throw request to a generator that has not started
        // completes it first (the request then behaves as one to a
        // finished generator: a return awaits its operand, a throw
        // rejects).
        if matches!(
            completion,
            AsyncGeneratorCompletion::Return(_) | AsyncGeneratorCompletion::Throw(_)
        ) && self
            .async_generator_is_suspended_start(generator)
            .expect("the scheduler just validated its retained generator control")
        {
            self.heap
                .set_generator_state(generator, GeneratorState::Done)
                .expect("completing the verified suspended-start frame cannot grow its bytes");
            self.set_async_generator_status(generator, AsyncGeneratorStatus::Completed)
                .expect("completing the verified scheduler frame only changes its status");
            return Ok(true);
        }
        // AsyncGeneratorUnwrapYieldResumption: a return request to a
        // generator suspended at a `yield` awaits its operand first. A
        // rejection then arrives as a throw at the `yield`, and an
        // operand whose PromiseResolve throws does so at once.
        let completion = match AsyncGeneratorRequestTurn::classify(completion) {
            AsyncGeneratorRequestTurn::AwaitReturn(value) => {
                // PromiseResolve can run a constructor getter which queues
                // another request. Keep that request behind the current
                // head until its await has been installed or refused.
                self.set_async_generator_status(generator, AsyncGeneratorStatus::Executing)
                    .expect("the scheduler retains its validated queue head");
                let epoch = self.throw_epoch;
                let input = NativeValue::from_retained_parts(value, input_storage.take());
                match self.start_async_generator_await_owned(
                    generator,
                    target,
                    AsyncGeneratorDelegateKind::AwaitReturn,
                    input,
                ) {
                    Ok(()) => {
                        return self
                            .set_async_generator_status(generator, AsyncGeneratorStatus::Awaiting)
                            .map(|()| false);
                    }
                    Err(
                        error @ (RuntimeError::SchedulingLimit
                        | RuntimeError::Heap(_)
                        | RuntimeError::InstructionLimit),
                    ) => {
                        self.set_async_generator_status(
                            generator,
                            AsyncGeneratorStatus::SuspendedYield,
                        )
                        .expect("restoring the retained scheduler status cannot allocate");
                        return Err(error);
                    }
                    Err(error) => {
                        self.set_async_generator_status(
                            generator,
                            AsyncGeneratorStatus::SuspendedYield,
                        )
                        .expect("restoring the retained scheduler status cannot allocate");
                        let origin = self.caught_rejection_origin(epoch);
                        let reason = self.error_value(error)?;
                        self.replace_async_generator_request(
                            generator,
                            AsyncGeneratorCompletion::Throw(reason),
                            Some(origin),
                        )?;
                        return Ok(true);
                    }
                }
            }
            AsyncGeneratorRequestTurn::Run(completion) => completion,
        };

        self.set_async_generator_status(generator, AsyncGeneratorStatus::Executing)
            .expect(
                "the verified queue head retains its generator; a status change cannot allocate",
            );
        let receiver = Value::Object(generator);
        let resume_epoch = self.throw_epoch;
        if matches!(
            completion,
            AsyncGeneratorRunCompletion::Throw(_) | AsyncGeneratorRunCompletion::ResumeThrow(_)
        ) {
            self.publish_rejection_origin(self.current_rejection_origin());
        }
        let result = match completion {
            AsyncGeneratorRunCompletion::Next(value) => {
                let input = NativeValue::from_retained_parts(value, input_storage.take());
                self.generator_resume_owned(
                    &receiver,
                    target,
                    input,
                    GeneratorInputKind::Next,
                    &mut input_storage,
                )
            }
            AsyncGeneratorRunCompletion::ReturnAwaited(value) => {
                let input = NativeValue::from_retained_parts(value, input_storage.take());
                match self.async_generator_delegate_request(
                    generator,
                    target,
                    DelegatedAbrupt::Return,
                    &input,
                ) {
                    Ok(Some(result)) => Ok(result),
                    Ok(None) => self.generator_resume_owned(
                        &receiver,
                        target,
                        input,
                        GeneratorInputKind::Return,
                        &mut input_storage,
                    ),
                    Err(error) => Err(error),
                }
            }
            AsyncGeneratorRunCompletion::ResumeReturn(value) => {
                let input = NativeValue::from_retained_parts(value, input_storage.take());
                self.generator_resume_owned(
                    &receiver,
                    target,
                    input,
                    GeneratorInputKind::Return,
                    &mut input_storage,
                )
            }
            AsyncGeneratorRunCompletion::ResumeThrow(value) => {
                let input = NativeValue::from_retained_parts(value, input_storage.take());
                self.generator_resume_owned(
                    &receiver,
                    target,
                    input,
                    GeneratorInputKind::Throw,
                    &mut input_storage,
                )
            }
            AsyncGeneratorRunCompletion::Throw(value) => {
                let input = NativeValue::from_retained_parts(value, input_storage.take());
                match self.async_generator_delegate_request(
                    generator,
                    target,
                    DelegatedAbrupt::Throw,
                    &input,
                ) {
                    Ok(Some(result)) => Ok(result),
                    Ok(None) => self.generator_resume_owned(
                        &receiver,
                        target,
                        input,
                        GeneratorInputKind::Throw,
                        &mut input_storage,
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
                        .get(&target)
                        .expect("queued async-generator target exists")
                        .status,
                    PromiseStatus::Pending
                ) =>
            {
                return self
                    .set_async_generator_status(generator, AsyncGeneratorStatus::Awaiting)
                    .map(|()| false);
            }
            Ok(Value::Undefined) => return Ok(false),
            Ok(result) => {
                return self
                    .finish_async_generator_run(generator, target, result)
                    .map(|()| false);
            }
            Err(
                error @ (RuntimeError::SchedulingLimit
                | RuntimeError::Heap(_)
                | RuntimeError::InstructionLimit),
            ) => return Err(error),
            Err(error) => {
                let origin = self.caught_rejection_origin(resume_epoch);
                let value = self.error_value_with_input(error, input_storage.take())?;
                value.with_owned_context(self, |vm, value| {
                    vm.with_rejection_origin(Some(origin), |vm| {
                        let (value, storage) = value.into_retained_parts();
                        vm.complete_async_generator_request_with_input(
                            generator,
                            target,
                            PromiseStatus::Rejected(value),
                            storage,
                        )
                    })
                })?;
            }
        }
        Ok(true)
    }

    /// Whether `generator` has not run yet (suspended-start).
    fn async_generator_is_suspended_start(
        &mut self,
        generator: ObjectId,
    ) -> Result<bool, RuntimeError> {
        let state = self.heap.take_generator_state(generator)?;
        let start = matches!(state, GeneratorState::Start { .. });
        // Taking the state retains its accounted bytes. Restoring exactly
        // that state therefore adds no bytes and cannot collect.
        self.heap
            .set_generator_state(generator, state)
            .expect("restoring an unchanged generator state cannot allocate");
        Ok(start)
    }

    /// Replaces the completion of the request at the head of the queue (the
    /// request keeps its promise and its place) and makes the generator
    /// resumable again.
    fn replace_async_generator_request(
        &mut self,
        generator: ObjectId,
        completion: AsyncGeneratorCompletion,
        origin: Option<ThrowOrigin>,
    ) -> Result<(), RuntimeError> {
        let mut control = self
            .heap
            .async_generator_control(generator)?
            .ok_or_else(|| RuntimeError::TypeError("Async generator receiver required".into()))?;
        let request = control
            .requests
            .front_mut()
            .ok_or(RuntimeError::Unsupported("missing async generator request"))?;
        let value_storage = control
            .owner
            .as_ref()
            .map(|budget| budget.reserve(HeapNativeBudget::value_payload_bytes(completion.value())))
            .transpose()?;
        request.completion = completion;
        request.value_storage = value_storage;
        if let Some(origin) = origin {
            request.origin = AsyncGeneratorOrigin::prepared(
                origin.payload_bytes(),
                control.owner.as_ref(),
                || origin,
            )?;
        }
        control.status = AsyncGeneratorStatus::SuspendedYield;
        self.with_roots(|heap| heap.set_async_generator_control(generator, control))?;
        Ok(())
    }

    /// Await(value) on behalf of the request at the head of the queue:
    /// PromiseResolve, then a continuation that always runs in a later job
    /// (delivery never resumes the generator synchronously). Errors propagate a
    /// PromiseResolve failure, native preparation refusal or committed
    /// notification failure; resource failures stay uncatchable.
    fn start_async_generator_await(
        &mut self,
        generator: ObjectId,
        target: ObjectId,
        kind: AsyncGeneratorDelegateKind,
        value: Value,
    ) -> Result<(), RuntimeError> {
        let owner = self.argument_list_owner()?;
        let input = NativeValue::adopt(value, owner.as_ref())?;
        self.start_async_generator_await_owned(generator, target, kind, input)
    }

    fn start_async_generator_await_owned(
        &mut self,
        generator: ObjectId,
        target: ObjectId,
        kind: AsyncGeneratorDelegateKind,
        input: NativeValue,
    ) -> Result<(), RuntimeError> {
        input.with_owned_context(self, |vm, input| {
            let base = vm.stack.len();
            vm.stack
                .extend([Value::Object(generator), Value::Object(target)]);
            let outcome = (|| {
                let promise = vm.promise_resolve_retained(input)?;
                vm.stack.push(promise.clone());
                let promise = promise
                    .object_id()
                    .expect("Promise.resolve always returns a Promise");
                let work = vm.prepare_continuation_await(
                    promise,
                    AwaitContinuation::GeneratorDelegate {
                        generator,
                        target,
                        kind,
                    },
                    0,
                )?;
                work.publish_and_handle(vm, promise)
            })();
            vm.stack.truncate(base);
            outcome
        })
    }

    /// The continuation of [`Vm::start_async_generator_await`]: the awaited
    /// operand is now known, so the request that was waiting for it becomes
    /// the completion it stands for, and the scheduler carries on.
    fn resume_after_async_generator_await(
        &mut self,
        generator: ObjectId,
        kind: AwaitedGeneratorOperand,
        value: Value,
        fulfilled: bool,
    ) -> Result<(), RuntimeError> {
        let completion = match (kind, fulfilled) {
            (AwaitedGeneratorOperand::Return, true) => {
                AsyncGeneratorCompletion::ReturnAwaited(value)
            }
            // A rejected operand is a throw completion at the `yield`, which
            // a `yield*` forwards to its delegate like any other throw.
            (AwaitedGeneratorOperand::Return, false) => AsyncGeneratorCompletion::Throw(value),
            (AwaitedGeneratorOperand::ReturnWithoutDelegateMethod, true) => {
                AsyncGeneratorCompletion::ResumeReturn(value)
            }
            (AwaitedGeneratorOperand::ReturnWithoutDelegateMethod, false) => {
                AsyncGeneratorCompletion::ResumeThrow(value)
            }
        };
        let origin = (!fulfilled).then(|| self.current_rejection_origin());
        self.replace_async_generator_request(generator, completion, origin)?;
        self.resume_async_generator_next(generator)
    }

    /// Async generator methods append one request and return its capability.
    /// The scheduler is the only path that may enter the generator frame.
    pub(in super::super) fn async_generator_request(
        &mut self,
        receiver: &Value,
        value: Value,
        function: NativeFunction,
    ) -> Result<Value, RuntimeError> {
        let kind = match function {
            NativeFunction::AsyncGeneratorNext => AsyncGeneratorRequestKind::Next,
            NativeFunction::AsyncGeneratorReturn => AsyncGeneratorRequestKind::Return,
            NativeFunction::AsyncGeneratorThrow => AsyncGeneratorRequestKind::Throw,
            _ => return Err(RuntimeError::Unsupported("not an async generator request")),
        };
        // AsyncGeneratorValidate failing is not a throw: the method returns a
        // promise rejected with the TypeError.
        let generator = match receiver.object_id() {
            // Anything but a live async generator (including any other kind of
            // heap object) fails the brand check.
            Some(generator)
                if matches!(
                    self.heap.async_generator_control_ref(generator),
                    Ok(Some(_))
                ) =>
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
        self.admit_scheduling_work(HostSchedulingUsage {
            pending_reactions: 1,
            ..HostSchedulingUsage::default()
        })?;
        let current = self.argument_list_owner()?;
        let budget = self
            .heap
            .async_generator_request_budget(generator)?
            .or_else(|| {
                current
                    .as_ref()
                    .map(|owner| owner.scheduling_scope().heap_native_budget())
            });
        let scheduling = match &budget {
            Some(budget) => {
                let only = self.host_scheduling.is_none().then_some(generator);
                self.retain_async_generator_request_counts_for(budget, only)?;
                self.heap.retain_async_generator_backing(budget, only)?;
                budget.reserve_reaction_lease()?
            }
            None => HeapReactionLease::unowned(),
        };
        let input_owner = budget
            .as_ref()
            .map(|budget| budget.reserve(0))
            .transpose()?;
        let input = NativeValue::adopt(value, input_owner.as_ref())?;
        let base = self.stack.len();
        self.stack.extend([
            Value::Object(generator),
            input
                .value()
                .object_id()
                .map_or(Value::Undefined, Value::Object),
        ]);
        let result = (|| {
            let mut control = self
                .heap
                .async_generator_control(generator)?
                .expect("validated async generator remains rooted");
            control.reserve_request_slot()?;
            let origin = AsyncGeneratorOrigin::prepared(
                self.current_rejection_origin_payload_bytes(),
                budget.as_ref(),
                || self.current_rejection_origin(),
            )?;
            let target = self.new_promise()?;
            self.stack.push(Value::Object(target));
            let id = control.next_request_id;
            control.next_request_id =
                control
                    .next_request_id
                    .checked_add(1)
                    .ok_or(RuntimeError::RangeError(
                        "async generator request identifiers exhausted".into(),
                    ))?;
            let (value, value_storage) = input.into_retained_parts();
            let completion = match kind {
                AsyncGeneratorRequestKind::Next => AsyncGeneratorCompletion::Next(value),
                AsyncGeneratorRequestKind::Return => AsyncGeneratorCompletion::Return(value),
                AsyncGeneratorRequestKind::Throw => AsyncGeneratorCompletion::Throw(value),
            };
            control.push_request(AsyncGeneratorRequest {
                id,
                completion,
                target,
                origin,
                scheduling,
                value_storage,
            })?;
            // Queue growth may collect. Protect the caller's operands and
            // the VM-owned promises, not only the queue's own heap edges.
            self.with_roots(|heap| heap.set_async_generator_control(generator, control))?;
            self.synchronize_scheduling_usage()?;
            self.resume_async_generator_next(generator)?;
            Ok(Value::Object(target))
        })();
        self.stack.truncate(base);
        result
    }

    /// Completes the request whose generator run just produced `result` (a
    /// yielded value or the return value) and moves on to the next one. The
    /// body has already awaited whatever the specification awaits there
    /// (`yield` and `return` operands), so the result is delivered as is.
    pub(in super::super) fn finish_async_generator_run(
        &mut self,
        generator: ObjectId,
        target: ObjectId,
        result: Value,
    ) -> Result<(), RuntimeError> {
        self.complete_async_generator_request(generator, target, PromiseStatus::Fulfilled(result))
            .expect("the scheduler completes the retained FIFO head and its tracked promise");
        self.resume_async_generator_next(generator)
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
            self.set_async_generator_status(generator, AsyncGeneratorStatus::Awaiting)
                .expect("the pending yield roots its validated async generator; changing status cannot allocate");
            // This result comes from iterator_result, not from a delegate or
            // user callback. Its initialized own data field cannot invoke JS.
            let value = self
                .heap
                .get_own(result, "value")
                .expect("the queued yield roots its private IteratorResult")
                .expect("iterator_result initializes its own value field");
            self.stack.push(value.clone());
            // A value whose PromiseResolve throws (a hostile `constructor`
            // getter) is awaited as an already rejected promise.
            let epoch = self.throw_epoch;
            let awaited = match self.promise_resolve(value) {
                Ok(promise) => promise,
                Err(error) => {
                    let origin = self.caught_rejection_origin(epoch);
                    let error = self.error_value(error)?;
                    self.with_rejection_origin(Some(origin), |vm| vm.promise_reject(error))?
                }
            };
            self.stack.push(awaited.clone());
            let awaited = awaited
                .object_id()
                .expect("Promise.resolve always returns a Promise");
            let work = self.prepare_continuation_await(
                awaited,
                AwaitContinuation::GeneratorYield {
                    generator,
                    target,
                    result,
                },
                0,
            )?;
            work.publish_and_handle(self, awaited)
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
            self.close_async_generator(generator)
                .expect("AsyncGeneratorYield reactions here originate from a completed FIFO request; its retained Done frame cannot allocate or close user iterators");
            self.complete_async_generator_request(
                generator,
                target,
                PromiseStatus::Rejected(value),
            )
            .expect("the scheduler completes the retained FIFO head and its tracked promise");
            return self.resume_async_generator_next(generator);
        }
        let value_payload =
            self.charge_execution_payload(host_scheduling::value_payload_bytes(&value));
        self.synchronize_execution_payload()?;
        self.set_property(&Value::Object(result), &"value".into(), &value)?;
        drop(value);
        drop(value_payload);
        self.complete_async_generator_request(
            generator,
            target,
            PromiseStatus::Fulfilled(Value::Object(result)),
        )
        .expect("the scheduler completes the retained FIFO head and its tracked promise");
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
        let returning = match kind {
            AsyncGeneratorDelegateKind::AwaitReturn => {
                return self.resume_after_async_generator_await(
                    generator,
                    AwaitedGeneratorOperand::Return,
                    result,
                    fulfilled,
                )
            }
            AsyncGeneratorDelegateKind::AwaitReturnNoMethod => {
                return self.resume_after_async_generator_await(
                    generator,
                    AwaitedGeneratorOperand::ReturnWithoutDelegateMethod,
                    result,
                    fulfilled,
                )
            }
            AsyncGeneratorDelegateKind::Return => true,
            AsyncGeneratorDelegateKind::Throw | AsyncGeneratorDelegateKind::CloseMissingThrow => {
                false
            }
        };
        // A rejected delegate call, or a result that is not an object, is an
        // abrupt completion of the `yield*` expression: it is thrown at the
        // `yield*` site (the delegate is finished and not closed again).
        if !fulfilled {
            return self.throw_at_async_delegate_exit(generator, target, result);
        }
        if !matches!(result, Value::Object(_)) {
            drop(result);
            let error = self.error_object(
                "TypeError",
                "yield* delegate method must return an object".into(),
            )?;
            return self.throw_at_async_delegate_exit(generator, target, error);
        }
        if matches!(kind, AsyncGeneratorDelegateKind::CloseMissingThrow) {
            let error = self.error_object(
                "TypeError",
                "yield* iterator does not provide a throw method".into(),
            )?;
            return self.throw_at_async_delegate_exit(generator, target, error);
        }
        // IteratorComplete and IteratorValue run inside the generator, so an
        // exception from a getter is thrown at the `yield*` site (where the
        // generator's own try/catch can see it), not out of this reaction.
        let read_epoch = self.throw_epoch;
        let read = self.get_property(&result, &"done".into()).and_then(|done| {
            Ok((
                self.to_boolean(&done)
                    .expect("the delegate getter returned a live value in this realm"),
                self.get_property(&result, &"value".into())?,
            ))
        });
        let (done, value) = match read {
            Ok(pair) => pair,
            Err(error) => {
                let origin = self.caught_rejection_origin(read_epoch);
                let error = self.error_value(error)?;
                return self.with_rejection_origin(Some(origin), |vm| {
                    vm.throw_at_async_delegate_exit(generator, target, error)
                });
            }
        };
        if !done {
            // The delegate's answer is forwarded without an Await.
            let iterator_result = self.iterator_result(value, false)?;
            self.complete_async_generator_request(
                generator,
                target,
                PromiseStatus::Fulfilled(iterator_result),
            )
            .expect("the scheduler completes the retained FIFO head and its tracked promise");
            return self.resume_async_generator_next(generator);
        }
        // The completed value may not fit back into the retained frame at the
        // heap limit; the refusal restores the delegation and propagates.
        self.finish_async_delegation(generator, value.clone())?;
        let resume_epoch = self.throw_epoch;
        let result = if returning {
            // Preserve the original return through any enclosing finally.
            self.generator_resume(
                &Value::Object(generator),
                None,
                Some(target),
                Some(Completion::Return(value)),
            )
        } else {
            self.generator_next(&Value::Object(generator), None, Some(target))
        };
        match result {
            Ok(Value::Undefined) => Ok(()),
            Ok(result) => self.finish_async_generator_run(generator, target, result),
            Err(error) => {
                let origin = self.caught_rejection_origin(resume_epoch);
                let error = self.error_value(error)?;
                self.with_rejection_origin(Some(origin), |vm| {
                    vm.complete_async_generator_request(
                        generator,
                        target,
                        PromiseStatus::Rejected(error),
                    )
                })?;
                self.resume_async_generator_next(generator)
            }
        }
    }

    /// Removes a completed delegate and commits its value at the compiler's
    /// exit boundary. Missing delegation leaves the saved frame intact.
    fn finish_async_delegation(
        &mut self,
        generator: ObjectId,
        value: Value,
    ) -> Result<(), RuntimeError> {
        let state = self.heap.take_generator_state(generator)?;
        // The commit replaces the iterator record on the stack, drops it from
        // the open-iterator list and clears the delegate. A refused commit
        // puts them back, so they must survive a collection the commit itself
        // triggers.
        let displaced: Vec<ObjectId> = match &state {
            GeneratorState::Suspended {
                stack,
                iterators,
                async_delegate: Some(delegate),
                ..
            } => stack
                .last()
                .and_then(Value::object_id)
                .into_iter()
                .chain(iterators.iter().filter_map(Value::object_id))
                .chain(delegate.record.object_id())
                .collect(),
            _ => Vec::new(),
        };
        self.commit_async_delegation(generator, state, value, &displaced)
    }

    fn commit_async_delegation(
        &mut self,
        generator: ObjectId,
        mut state: GeneratorState,
        value: Value,
        displaced: &[ObjectId],
    ) -> Result<(), RuntimeError> {
        // What the commit replaces, so a refused commit can put it all back.
        let mut previous = None;
        if let GeneratorState::Suspended {
            pc,
            stack,
            iterators,
            async_delegate,
            ..
        } = &mut state
        {
            if let Some(delegate) = async_delegate.take() {
                let old_iterators = iterators.clone();
                let old_pc = std::mem::replace(pc, delegate.exit_pc);
                iterators.retain(|active| active != &delegate.record);
                let old_top = std::mem::replace(
                    stack
                        .last_mut()
                        .expect("yield* delegation keeps its iterator record"),
                    value,
                );
                previous = Some((delegate, old_pc, old_iterators, old_top));
            }
        }
        let Some((delegate, old_pc, old_iterators, old_top)) = previous else {
            // Nothing was changed; the frame goes back exactly as it was.
            self.with_roots(|heap| heap.set_generator_state(generator, state))?;
            return Err(RuntimeError::Unsupported(
                "lost async yield* delegation state",
            ));
        };
        // The completed value can be larger than the iterator record it
        // replaces, so the commit may be refused at the heap limit. Refuse
        // atomically: restore the delegation and report the resource error.
        let mut refused = None;
        let committed = self.with_roots(|heap| {
            heap.try_set_generator_state(generator, state, displaced)
                .map_err(|(error, state)| {
                    refused = Some(state);
                    error
                })
        });
        let Err(error) = committed else {
            return Ok(());
        };
        if let Some(mut state) = refused {
            if let GeneratorState::Suspended {
                pc,
                stack,
                iterators,
                async_delegate,
                ..
            } = &mut *state
            {
                *pc = old_pc;
                *iterators = old_iterators;
                *async_delegate = Some(delegate);
                *stack
                    .last_mut()
                    .expect("yield* delegation keeps its iterator record") = old_top;
            }
            // The original frame was already stored; this only reinstates it.
            self.with_roots(|heap| heap.set_generator_state(generator, *state))?;
        }
        Err(error)
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
        self.finish_async_delegation(generator, Value::Undefined)
            .expect("the scheduler throws at the exit of its outstanding async delegation");
        let resume_epoch = self.throw_epoch;
        self.publish_rejection_origin(self.current_rejection_origin());
        let result = self.generator_resume(
            &Value::Object(generator),
            None,
            Some(target),
            Some(Completion::Throw(RuntimeError::Thrown(error))),
        );
        match result {
            Ok(Value::Undefined) => Ok(()),
            Ok(result) => self.finish_async_generator_run(generator, target, result),
            Err(error) => {
                let origin = self.caught_rejection_origin(resume_epoch);
                let error = self.error_value(error)?;
                self.with_rejection_origin(Some(origin), |vm| {
                    vm.complete_async_generator_request(
                        generator,
                        target,
                        PromiseStatus::Rejected(error),
                    )
                })?;
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
        let mut iterators = self.iterator_records(iterators);
        self.heap
            .set_generator_state(generator, GeneratorState::Done)
            .expect("closing a verified generator only decreases its accounted state bytes");
        self.close_iterators_for_return(&mut iterators, 0)
    }
}

#[cfg(any(test, coverage))]
#[path = "../../../tests/fixtures/async_generator_request_lifetime.rs"]
mod request_lifetime_tests;

#[cfg(any(test, coverage))]
#[path = "../../../tests/fixtures/async_generator_capacity.rs"]
mod capacity_tests;

#[cfg(any(test, coverage))]
#[path = "../../../tests/fixtures/async_generator_origins.rs"]
mod origin_lifetime_tests;

#[cfg(any(test, coverage))]
#[path = "../../../tests/fixtures/async_generator_values.rs"]
mod value_lifetime_tests;

#[cfg(any(test, coverage))]
#[path = "../../../tests/fixtures/async_generator_inputs.rs"]
mod input_lifetime_tests;
