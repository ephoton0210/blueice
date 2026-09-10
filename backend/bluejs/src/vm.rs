// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded operand-stack interpreter for compiler-owned bytecode.
//! Allocating instructions root all VM-held objects around heap
//! safepoints. The collector itself additionally protects store inputs.

use crate::bytecode::Binding;
use crate::native::{self, NativeFunction};
use crate::primitive;
use crate::{
    Bytecode, Heap, HeapConfig, HeapError, JsString, JsSymbol, ObjectId, Opcode,
    PropertyDescriptor, PropertyName, RootId, Value,
};
use num_bigint::{BigInt, Sign};
use num_traits::ToPrimitive;
use std::collections::{HashMap, HashSet};
mod builtins;
mod errors;
mod functions;
mod intl;
mod json;
mod regexp;
mod test262;
use std::cmp::Ordering;
use std::fmt;

#[derive(Debug, Clone, Copy)]
pub struct VmConfig {
    pub heap: HeapConfig,
    /// Per-execute dispatch limit, including branches/scopes and native
    /// raw/split/replace loop steps; not a wall-clock or whole-native-work limit.
    pub instruction_budget: u64,
    /// Maximum UTF-16 payload bytes in any one runtime string (not total
    /// RSS): two bytes per code unit, including lone surrogates.
    pub max_string_bytes: usize,
    /// Wall-clock limit for each isolated regex compilation or match.
    pub regex_timeout: std::time::Duration,
}

impl Default for VmConfig {
    fn default() -> Self {
        Self {
            heap: HeapConfig::default(),
            instruction_budget: 1_000_000,
            max_string_bytes: 1024 * 1024,
            regex_timeout: crate::regex_worker::DEFAULT_TIMEOUT,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeError {
    ReferenceError(String),
    TypeError(String),
    RangeError(String),
    SyntaxError(String),
    Thrown(Value),
    Heap(HeapError),
    InstructionLimit,
    StringLimit { limit: usize },
    RegexTimeout,
    Test262(String),
    RegexWorker(String),
    Unsupported(&'static str),
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReferenceError(name) => write!(f, "ReferenceError: {name} is not defined"),
            Self::TypeError(message) => write!(f, "TypeError: {message}"),
            Self::RangeError(message) => write!(f, "RangeError: {message}"),
            Self::SyntaxError(message) => write!(f, "SyntaxError: {message}"),
            Self::Thrown(value) => write!(f, "uncaught JavaScript value: {value:?}"),
            Self::Heap(error) => error.fmt(f),
            Self::InstructionLimit => f.write_str("BlueJS instruction budget exhausted"),
            Self::StringLimit { limit } => write!(f, "BlueJS string exceeds {limit} bytes"),
            Self::Test262(message) => write!(f, "Test262Error: {message}"),
            Self::RegexTimeout => f.write_str("BlueJS regex deadline exceeded"),
            Self::RegexWorker(message) => write!(f, "BlueJS regex worker failed: {message}"),
            Self::Unsupported(reason) => write!(f, "BlueJS unsupported: {reason}"),
        }
    }
}
impl std::error::Error for RuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Heap(error) => Some(error),
            _ => None,
        }
    }
}
impl From<HeapError> for RuntimeError {
    fn from(error: HeapError) -> Self {
        match error {
            HeapError::InvalidArrayLength => Self::RangeError("invalid array length".into()),
            _ => Self::Heap(error),
        }
    }
}

impl RuntimeError {
    /// Language exceptions participate in ECMAScript completion propagation.
    /// Limits, allocation failures and failed isolated host workers remain
    /// uncatchable host aborts: user code must not turn a resource boundary
    /// into an apparent JavaScript success.
    fn is_catchable(&self) -> bool {
        matches!(
            self,
            Self::ReferenceError(_)
                | Self::TypeError(_)
                | Self::RangeError(_)
                | Self::SyntaxError(_)
                | Self::Thrown(_)
                | Self::Test262(_)
        )
    }
}

#[derive(Clone)]
enum Completion {
    Throw(RuntimeError),
    Return(Value),
    TailRecur(Vec<Value>),
    Yield(Value),
    Jump { cleanup: usize, target: usize },
    Resume(usize),
    Halt(Value),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HandlerState {
    Try,
    Catch,
    Finally,
}

struct HandlerFrame {
    metadata: usize,
    stack_depth: usize,
    scope_depth: usize,
    iterator_depth: usize,
    with_depth: usize,
    state: HandlerState,
    pending: Option<usize>,
}

enum CompletionAction {
    Continue,
    Jump(usize),
    Return(Value),
    TailRecur(Vec<Value>),
    Throw(RuntimeError),
}

enum InterpreterExit {
    Return(Value),
    Yield { value: Value, pc: usize },
}

/// An isolated execution context. Each execute has fresh bindings; this
/// is not yet a persistent REPL/global environment or browser host.
pub struct Vm {
    config: VmConfig,
    heap: Heap,
    object_prototype: ObjectId,
    array_prototype: ObjectId,
    string_intrinsics: Option<(ObjectId, ObjectId)>,
    result_root: Option<RootId>,
    stack: Vec<Value>,
    // None is a lexical binding's uninitialized state, never JS undefined.
    bindings: Vec<Option<Value>>,
    // The matching static metadata for `bindings`; nested calls and direct
    // eval swap it together with their runtime binding vector.
    binding_metadata: Vec<Binding>,
    completion: Value,
    completion_empty: bool,
    active_scopes: Vec<u32>,
    active_scope_slots: Vec<Vec<u32>>,
    with_objects: Vec<Value>,
    // Values in suspended finally paths live here rather than in Rust-only
    // handler records, so VM safepoints root them during allocations.
    pending_completions: Vec<Completion>,
    completion_saves: Vec<(Value, bool)>,
    remaining_instructions: u64,
    cells: HashMap<usize, ObjectId>,
    this: Value,
    arguments: Vec<Value>,
    strict: bool,
    call_depth: usize,
    globals: HashMap<String, ObjectId>,
    iterator_prototype: Option<ObjectId>,
    regexp_iterator_prototype: Option<ObjectId>,
    templates: HashMap<u64, ObjectId>,
    new_target: Value,
    // The `[[HomeObject]]` of the currently executing method or class
    // constructor. It is runtime frame state because `super` is lexical.
    home_object: Option<ObjectId>,
    // A class constructor's home object is its instance prototype for
    // `super.property`; `super()` separately needs the constructor closure
    // that owns the evaluated superclass metadata.
    class_constructor: Option<ObjectId>,
    // A direct eval in an instance field is outside a constructor for the
    // `super()` early-error rules even though fields are lowered into the
    // constructor bytecode.
    class_field_initializer_depth: u32,
    iterator_base: Option<ObjectId>,
    array_iterator_prototype: Option<ObjectId>,
    generator_prototype: Option<ObjectId>,
    joining: Vec<ObjectId>,
}

impl Default for Vm {
    fn default() -> Self {
        Self::new(VmConfig::default()).expect("default VM configuration is valid")
    }
}

impl Vm {
    /// Creates an isolated VM and its rooted object/array prototypes.
    /// The heap budget must accommodate both prototype records.
    pub fn new(config: VmConfig) -> Result<Self, HeapError> {
        let mut heap = Heap::new(config.heap)?;
        let object_prototype = heap.alloc_object(None)?;
        // Permanent root, released with the heap. Builtin properties and
        // callable methods are installed lazily by string_intrinsics.
        heap.root(object_prototype)?;
        let array_prototype = heap.alloc_array(0, Some(object_prototype))?;
        heap.root(array_prototype)?;
        Ok(Self {
            config,
            heap,
            object_prototype,
            array_prototype,
            string_intrinsics: None,
            result_root: None,
            stack: Vec::new(),
            bindings: Vec::new(),
            binding_metadata: Vec::new(),
            completion: Value::Undefined,
            completion_empty: true,
            active_scopes: Vec::new(),
            active_scope_slots: Vec::new(),
            with_objects: Vec::new(),
            pending_completions: Vec::new(),
            completion_saves: Vec::new(),
            remaining_instructions: 0,
            cells: HashMap::new(),
            this: Value::Undefined,
            arguments: Vec::new(),
            strict: false,
            call_depth: 0,
            globals: HashMap::new(),
            iterator_prototype: None,
            regexp_iterator_prototype: None,
            templates: HashMap::new(),
            new_target: Value::Undefined,
            home_object: None,
            class_constructor: None,
            class_field_initializer_depth: 0,
            iterator_base: None,
            array_iterator_prototype: None,
            generator_prototype: None,
            joining: Vec::new(),
        })
    }

    pub fn heap(&self) -> &Heap {
        &self.heap
    }

    /// Executes only bytecode produced by [`crate::compile`] with fresh
    /// bindings. A returned object and its reachable graph stay alive until
    /// the next execute (including a failing execute), or until this VM is
    /// dropped. Both success and error paths release temporary runtime roots.
    pub fn execute(&mut self, code: &Bytecode) -> Result<Value, RuntimeError> {
        self.execute_with_global_bindings(code, false)
    }

    /// Executes a classic script in this realm and publishes successful
    /// top-level `var` and function declarations on `globalThis` for a later
    /// classic script. Lexical bindings retain their script-local boundary.
    pub fn execute_script(&mut self, code: &Bytecode) -> Result<Value, RuntimeError> {
        self.execute_with_global_bindings(code, true)
    }

    fn execute_with_global_bindings(
        &mut self,
        code: &Bytecode,
        publish_globals: bool,
    ) -> Result<Value, RuntimeError> {
        if let Some(root) = self.result_root.take() {
            self.heap.unroot(root)?;
        }
        self.heap.collect_major();
        self.bindings.resize(code.bindings.len(), None);
        self.binding_metadata = code.bindings.clone();
        self.remaining_instructions = self.config.instruction_budget;
        self.strict = code.strict;
        self.completion_empty = true;
        self.active_scopes.clear();
        self.active_scope_slots.clear();
        self.with_objects.clear();
        // `this` lazily materializes the realm global only when script code
        // actually observes it. This keeps data-only executions within small
        // heap configurations while preserving script and arrow semantics.
        self.this = Value::Undefined;
        self.class_field_initializer_depth = 0;
        let result = self.run(code).and_then(|value| {
            if publish_globals {
                self.publish_global_bindings(code)?;
            }
            if let Value::Object(id) = value {
                self.result_root = Some(self.heap.root(id)?);
            }
            Ok(value)
        });
        if let Err(RuntimeError::Thrown(Value::Object(id))) = &result {
            self.result_root = Some(self.heap.root(*id)?);
        }
        self.stack.clear();
        self.bindings.clear();
        self.binding_metadata.clear();
        self.cells.clear();
        self.completion = Value::Undefined;
        self.completion_empty = true;
        self.active_scopes.clear();
        self.active_scope_slots.clear();
        self.with_objects.clear();
        self.pending_completions.clear();
        self.completion_saves.clear();
        self.heap.collect_major();
        result
    }

    fn publish_global_bindings(&mut self, code: &Bytecode) -> Result<(), RuntimeError> {
        let global = self
            .global("globalThis")?
            .object_id()
            .expect("globalThis is an object");
        for (slot, binding) in code.bindings.iter().enumerate() {
            if binding.lexical {
                continue;
            }
            let Some(value) = self.binding_value(slot)? else {
                continue;
            };
            self.define_data(global, binding.name.as_str(), value, true, true, false)?;
        }
        Ok(())
    }

    fn pop(&mut self) -> Value {
        self.stack
            .pop()
            .expect("compiler balances the operand stack")
    }

    fn reset_scope(&mut self, code: &Bytecode, scope: u32) {
        for slot in &code.scopes[scope as usize] {
            self.cells.remove(&(*slot as usize));
            self.bindings[*slot as usize] = None;
        }
    }

    fn leave_scope(&mut self, code: &Bytecode, scope: u32) {
        if self.active_scopes.last() == Some(&scope) {
            self.active_scopes.pop();
            self.active_scope_slots.pop();
            self.reset_scope(code, scope);
        } else {
            // A control-transfer gateway can be resumed after a handler has
            // already unwound an inner scope before running `finally`.
            // Gateways still list that lexical scope; it is a no-op now.
            debug_assert!(
                !self.active_scopes.contains(&scope),
                "scope {scope} is below an active inner scope: {:?}",
                self.active_scopes
            );
        }
    }

    fn unwind_scopes(&mut self, code: &Bytecode, depth: usize) {
        while self.active_scopes.len() > depth {
            let scope = self.active_scopes.pop().expect("scope length was checked");
            self.active_scope_slots.pop();
            self.reset_scope(code, scope);
        }
    }

    fn unwind_with(&mut self, depth: usize) {
        self.with_objects.truncate(depth);
    }

    fn close_iterators_to(&mut self, iterators: &mut Vec<Value>, depth: usize) {
        // A compiler-emitted `break` can close its target for-of iterator
        // before a surrounding handler starts finalizer cleanup.
        let active = iterators.split_off(depth.min(iterators.len()));
        for record in active.into_iter().rev() {
            // This is cleanup for an already-selected abrupt completion. The
            // original completion wins over a return() failure.
            let _ = self.iterator_close(&record);
        }
    }

    fn error_value(&mut self, error: RuntimeError) -> Result<Value, RuntimeError> {
        match error {
            RuntimeError::Thrown(value) => Ok(value),
            RuntimeError::ReferenceError(message) => self.error_object("ReferenceError", message),
            RuntimeError::TypeError(message) => self.error_object("TypeError", message),
            RuntimeError::RangeError(message) => self.error_object("RangeError", message),
            RuntimeError::SyntaxError(message) => self.error_object("SyntaxError", message),
            RuntimeError::Test262(message) => self.error_object("Test262Error", message),
            error => Err(error),
        }
    }

    fn error_object(&mut self, name: &str, message: String) -> Result<Value, RuntimeError> {
        let constructor = self.error_global(name)?;
        self.call_native(
            constructor,
            Value::Undefined,
            vec![Value::String(message.into())],
            false,
        )
    }

    fn restore_completion(&mut self) {
        let (value, empty) = self
            .completion_saves
            .pop()
            .expect("normal finally entry saves its preceding completion");
        self.completion = value;
        self.completion_empty = empty;
    }

    fn resolve_completion(
        &mut self,
        code: &Bytecode,
        handlers: &mut Vec<HandlerFrame>,
        iterators: &mut Vec<Value>,
        completion: Completion,
    ) -> Result<CompletionAction, RuntimeError> {
        if let Completion::Halt(value) = completion {
            return Ok(CompletionAction::Return(value));
        }
        if let Completion::Resume(metadata) = completion {
            if let Some(frame) = handlers.pop_if(|frame| frame.metadata == metadata) {
                let pending = frame
                    .pending
                    .expect("only an abrupt finally resumes a handler");
                return self.resolve_completion(
                    code,
                    handlers,
                    iterators,
                    self.pending_completions[pending].clone(),
                );
            }
            self.restore_completion();
            return Ok(CompletionAction::Continue);
        }

        // Keep a potential thrown/returned object reachable while scope and
        // iterator cleanup can call user code and trigger collection.
        self.pending_completions.push(completion.clone());
        let mut completion = completion;
        loop {
            let Some(frame) = handlers.last() else {
                return Ok(match completion {
                    Completion::Throw(error) => CompletionAction::Throw(error),
                    Completion::Return(value) => CompletionAction::Return(value),
                    Completion::TailRecur(args) => CompletionAction::TailRecur(args),
                    Completion::Jump { cleanup, .. } => CompletionAction::Jump(cleanup),
                    Completion::Resume(_) | Completion::Halt(_) | Completion::Yield(_) => {
                        unreachable!("handled above")
                    }
                });
            };
            let metadata = frame.metadata;
            let stack_depth = frame.stack_depth;
            let scope_depth = frame.scope_depth;
            let iterator_depth = frame.iterator_depth;
            let with_depth = frame.with_depth;
            let state = frame.state;
            let handler = &code.handlers[metadata];
            let catch = handler.catch;
            let finally = handler.finally;

            // A break/continue may target a loop that is contained in this
            // try or catch block. It has not left this handler, so it must not
            // consume the frame or spuriously run an outer finalizer.
            if let Completion::Jump { cleanup, target } = completion {
                let region = match state {
                    HandlerState::Try => {
                        Some((handler.try_start as usize, handler.try_end as usize))
                    }
                    HandlerState::Catch => handler.catch.map(|start| {
                        (
                            start as usize,
                            handler.catch_end.expect("catch end is compiled") as usize,
                        )
                    }),
                    HandlerState::Finally => None,
                };
                if region.is_some_and(|(start, end)| (start..end).contains(&target)) {
                    return Ok(CompletionAction::Jump(cleanup));
                }
            }

            // Break/continue resume at compiler-emitted cleanup gateways. If
            // one crosses this handler to reach a finalizer, unwind its try or
            // catch scope before the finalizer runs; its later gateway skips
            // that already-cleared scope. Throws and returns have no bytecode
            // continuation, so they always unwind immediately.
            let unwind = !matches!(completion, Completion::Jump { .. })
                || (state != HandlerState::Finally && finally.is_some());
            if unwind {
                self.stack.truncate(stack_depth);
                self.unwind_scopes(code, scope_depth);
                self.close_iterators_to(iterators, iterator_depth);
                self.unwind_with(with_depth);
            }

            if state == HandlerState::Try {
                if let (Some(target), Completion::Throw(error)) = (catch, &completion) {
                    let value = self.error_value(error.clone())?;
                    handlers
                        .last_mut()
                        .expect("handler was inspected above")
                        .state = HandlerState::Catch;
                    self.stack.push(value);
                    return Ok(CompletionAction::Jump(target as usize));
                }
            }
            if state != HandlerState::Finally {
                if let Some(target) = finally {
                    let pending = self.pending_completions.len();
                    self.pending_completions.push(completion);
                    let frame = handlers.last_mut().expect("handler was inspected above");
                    frame.state = HandlerState::Finally;
                    frame.pending = Some(pending);
                    return Ok(CompletionAction::Jump(target as usize));
                }
            }
            handlers.pop();
            completion = self
                .pending_completions
                .last()
                .expect("completion remains rooted")
                .clone();
        }
    }

    fn check_string(&self, value: &Value) -> Result<(), RuntimeError> {
        if matches!(value, Value::String(s) if s.byte_len() > self.config.max_string_bytes) {
            Err(RuntimeError::StringLimit {
                limit: self.config.max_string_bytes,
            })
        } else {
            Ok(())
        }
    }

    fn with_roots<T>(
        &mut self,
        operation: impl FnOnce(&mut Heap) -> Result<T, HeapError>,
    ) -> Result<T, RuntimeError> {
        let mut roots = Vec::new();
        let registration = (|| {
            for value in self
                .stack
                .iter()
                .chain(self.bindings.iter().flatten())
                .chain(std::iter::once(&self.completion))
                .chain(self.pending_completions.iter().flat_map(|completion| {
                    let values: &[Value] = match completion {
                        Completion::Return(value)
                        | Completion::Yield(value)
                        | Completion::Throw(RuntimeError::Thrown(value)) => {
                            std::slice::from_ref(value)
                        }
                        Completion::TailRecur(args) => args,
                        Completion::Throw(_)
                        | Completion::Jump { .. }
                        | Completion::Resume(_)
                        | Completion::Halt(_) => &[],
                    };
                    values.iter()
                }))
                .chain(self.completion_saves.iter().map(|(value, _)| value))
                .chain(self.with_objects.iter())
            {
                if let Value::Object(id) = value {
                    // Rooting cannot GC, but can exhaust the root-ID
                    // counter. Partial registrations must be released too.
                    roots.push(self.heap.root(*id)?);
                }
            }
            for id in self.cells.values() {
                roots.push(self.heap.root(*id)?);
            }
            Ok(())
        })();
        let result = registration.and_then(|()| operation(&mut self.heap));
        for root in roots {
            self.heap
                .unroot(root)
                .expect("temporary root belongs to this safepoint");
        }
        result.map_err(RuntimeError::from)
    }

    fn run(&mut self, code: &Bytecode) -> Result<Value, RuntimeError> {
        let pending_base = self.pending_completions.len();
        let save_base = self.completion_saves.len();
        let mut iterators = Vec::new();
        let result = self
            .interpret(code, &mut iterators, 0, None)
            .and_then(|exit| match exit {
                InterpreterExit::Return(value) => Ok(value),
                InterpreterExit::Yield { .. } => Err(RuntimeError::TypeError(
                    "yield requires a generator function".into(),
                )),
            });
        if result.is_err() {
            if let Err(RuntimeError::Thrown(value)) = &result {
                self.stack.push(value.clone());
            }
            self.stack.extend(iterators.iter().cloned());
            for record in iterators.into_iter().rev() {
                // IteratorClose preserves an existing throw even when return
                // throws too. Resource exhaustion retains the original limit.
                let _ = self.iterator_close(&record);
            }
        }
        self.pending_completions.truncate(pending_base);
        self.completion_saves.truncate(save_base);
        result
    }

    fn execute_eval(
        &mut self,
        code: &Bytecode,
        captures: Vec<ObjectId>,
    ) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        self.stack.extend(self.bindings.iter().flatten().cloned());
        self.stack
            .extend(self.cells.values().copied().map(Value::Object));
        self.stack.push(self.completion.clone());
        self.stack.push(self.this.clone());
        self.stack.extend(self.arguments.iter().cloned());
        let bindings = std::mem::replace(&mut self.bindings, vec![None; code.bindings.len()]);
        let binding_metadata = std::mem::replace(&mut self.binding_metadata, code.bindings.clone());
        let cells = std::mem::replace(&mut self.cells, captures.into_iter().enumerate().collect());
        let completion = std::mem::replace(&mut self.completion, Value::Undefined);
        let completion_empty = std::mem::replace(&mut self.completion_empty, true);
        let active_scopes = std::mem::take(&mut self.active_scopes);
        let active_scope_slots = std::mem::take(&mut self.active_scope_slots);
        let strict = std::mem::replace(&mut self.strict, code.strict);
        let result = self.run(code);
        self.bindings = bindings;
        self.binding_metadata = binding_metadata;
        self.cells = cells;
        self.completion = completion;
        self.completion_empty = completion_empty;
        self.active_scopes = active_scopes;
        self.active_scope_slots = active_scope_slots;
        self.strict = strict;
        self.stack.truncate(base);
        result
    }

    fn eval_visible_bindings(&self) -> Vec<(String, Binding, u32)> {
        let mut visible = std::collections::BTreeMap::new();
        for scope in &self.active_scope_slots {
            for &slot in scope {
                let slot = slot as usize;
                visible.insert(
                    self.binding_metadata[slot].name.clone(),
                    (self.binding_metadata[slot].clone(), slot as u32),
                );
            }
        }
        for &slot in self.cells.keys() {
            visible
                .entry(self.binding_metadata[slot].name.clone())
                .or_insert_with(|| (self.binding_metadata[slot].clone(), slot as u32));
        }
        visible
            .into_iter()
            .map(|(name, (binding, slot))| (name, binding, slot))
            .collect()
    }

    fn interpret(
        &mut self,
        code: &Bytecode,
        iterators: &mut Vec<Value>,
        start_pc: usize,
        resume_value: Option<Value>,
    ) -> Result<InterpreterExit, RuntimeError> {
        let stack_base = self.stack.len();
        let pending_base = self.pending_completions.len();
        let save_base = self.completion_saves.len();
        if let Some(value) = resume_value {
            self.stack.push(value);
        }
        let mut pc = start_pc;
        let mut handlers = Vec::new();
        loop {
            self.charge_step()?;
            let instruction = code
                .instruction(pc)
                .expect("compiler emits valid instruction boundaries");
            let operand = instruction.operand.unwrap_or(0) as usize;
            pc += instruction.opcode.width();
            let outcome: Result<Option<Completion>, RuntimeError> = (|| {
                match instruction.opcode {
                    Opcode::DefineData
                    | Opcode::DefineAccessor
                    | Opcode::DefineMethod
                    | Opcode::DefineClassAccessor => {
                        let value = self.pop();
                        let (receiver, key) = self.property_reference()?;
                        let object = receiver.object_id().unwrap();
                        if matches!(
                            instruction.opcode,
                            Opcode::DefineMethod | Opcode::DefineClassAccessor
                        ) {
                            if let Value::Object(function) = value {
                                self.with_roots(|heap| heap.set_closure_home(function, object))?;
                            }
                        }
                        if instruction.opcode == Opcode::DefineData {
                            self.define_data(object, key, value.clone(), true, true, true)?;
                        } else {
                            let descriptor = if instruction.opcode == Opcode::DefineMethod {
                                PropertyDescriptor::data(value.clone(), true, false, true)
                            } else {
                                PropertyDescriptor {
                                    get: (operand == 0).then(|| value.clone()),
                                    set: (operand != 0).then(|| value.clone()),
                                    enumerable: Some(instruction.opcode == Opcode::DefineAccessor),
                                    configurable: Some(true),
                                    ..Default::default()
                                }
                            };
                            if !self.with_roots(|heap| {
                                heap.define_own_property(object, key, descriptor)
                            })? {
                                return Err(RuntimeError::TypeError(
                                    "cannot define class property".into(),
                                ));
                            }
                        }
                        self.stack.push(value);
                    }
                    Opcode::DeleteProperty => {
                        let (receiver, key) = self.property_reference()?;
                        let deleted = match receiver {
                            Value::Object(id) => self.heap.delete(id, key)?,
                            Value::String(s) => {
                                !matches!(&key, PropertyName::String(key) if s.own_property(key).is_some())
                            }
                            _ => true,
                        };
                        if !deleted && self.strict {
                            return Err(RuntimeError::TypeError(
                                "cannot delete non-configurable property".into(),
                            ));
                        }
                        self.stack.push(Value::Bool(deleted));
                    }
                    Opcode::Throw => {
                        return Ok(Some(Completion::Throw(RuntimeError::Thrown(self.pop()))))
                    }
                    Opcode::ArrayPush => {
                        let base = self.stack.len() - 2;
                        self.array_push(
                            &self.stack[base].clone(),
                            &self.stack[base + 1].clone(),
                            operand,
                        )?;
                        self.stack.truncate(base + 1);
                    }
                    Opcode::CallSpread => {
                        let base = self.stack.len() - 3;
                        let args = self.array_like_values(&self.stack[base + 2].clone())?;
                        let result = self.call_native(
                            self.stack[base].clone(),
                            self.stack[base + 1].clone(),
                            args,
                            operand != 0,
                        )?;
                        self.stack.truncate(base);
                        self.stack.push(result);
                    }
                    Opcode::CallClassStaticBlock => {
                        let base = self.stack.len() - 2;
                        let Value::Object(target) = self.stack[base].clone() else {
                            unreachable!("class constructors are objects")
                        };
                        if let Value::Object(function) = self.stack[base + 1] {
                            self.with_roots(|heap| heap.set_closure_home(function, target))?;
                        }
                        self.call_native(
                            self.stack[base + 1].clone(),
                            self.stack[base].clone(),
                            Vec::new(),
                            false,
                        )?;
                        self.stack.truncate(base + 1);
                    }
                    Opcode::DefineClassStaticField => {
                        let base = self.stack.len() - 4;
                        let target = self.stack[base + 1].clone();
                        let key = self.coerce_property_key(&self.stack[base + 2].clone())?;
                        let initializer = self.stack[base + 3].clone();
                        if let (Value::Object(target), Value::Object(function)) =
                            (&target, &initializer)
                        {
                            self.with_roots(|heap| heap.set_closure_home(*function, *target))?;
                        }
                        let value =
                            self.call_native(initializer, target.clone(), Vec::new(), false)?;
                        let Value::Object(target) = target else {
                            unreachable!("class fields target the constructor")
                        };
                        if !self.with_roots(|heap| {
                            heap.define_own_property(
                                target,
                                key,
                                PropertyDescriptor::data(value, true, true, true),
                            )
                        })? {
                            return Err(RuntimeError::TypeError(
                                "cannot define class field".into(),
                            ));
                        }
                        self.stack.truncate(base + 1);
                    }
                    Opcode::SetClassHome => self.set_class_home()?,
                    Opcode::SetClassHeritage => self.set_class_heritage()?,
                    Opcode::SuperGet | Opcode::SuperGetMethod => {
                        let key_value = self.pop();
                        let key = self.coerce_property_key(&key_value)?;
                        let value = self.super_get(&key)?;
                        self.check_string(&value)?;
                        self.stack.push(value);
                        if instruction.opcode == Opcode::SuperGetMethod {
                            self.stack.push(self.this.clone());
                        }
                    }
                    Opcode::SuperSet => {
                        let value = self.pop();
                        let key_value = self.pop();
                        let key = self.coerce_property_key(&key_value)?;
                        self.super_set(&key, &value)?;
                        self.stack.push(value);
                    }
                    Opcode::SuperUpdate => {
                        let key_value = self.pop();
                        let key = self.coerce_property_key(&key_value)?;
                        let old_value = self.super_get(&key)?;
                        let old = self.coerce_number(&old_value)?;
                        let new = if operand & 1 == 0 {
                            old + 1.0
                        } else {
                            old - 1.0
                        };
                        self.super_set(&key, &Value::Number(new))?;
                        self.stack
                            .push(Value::Number(if operand & 2 == 0 { old } else { new }));
                    }
                    Opcode::SuperCall | Opcode::SuperCallSpread | Opcode::SuperCallForward => {
                        let args = if instruction.opcode == Opcode::SuperCall {
                            let base = self.stack.len() - operand;
                            let args = self.stack[base..].to_vec();
                            self.stack.truncate(base);
                            args
                        } else if instruction.opcode == Opcode::SuperCallSpread {
                            let arguments = self.pop();
                            self.array_like_values(&arguments)?
                        } else {
                            self.arguments.clone()
                        };
                        let value = self.super_call(args)?;
                        self.stack.push(value);
                    }
                    Opcode::EnterClassFieldInitializer => self.class_field_initializer_depth += 1,
                    Opcode::LeaveClassFieldInitializer => {
                        self.class_field_initializer_depth = self
                            .class_field_initializer_depth
                            .checked_sub(1)
                            .expect("compiler balances class field initializers");
                    }
                    Opcode::RegExpLiteral => {
                        let base = self.stack.len() - 2;
                        let regexp = self.regexp_create(
                            &self.stack[base].clone(),
                            &self.stack[base + 1].clone(),
                        )?;
                        self.stack.truncate(base);
                        self.stack.push(regexp);
                    }
                    Opcode::TemplateObject => {
                        let object = self.template_object(&code.templates[operand])?;
                        self.stack.push(object);
                    }
                    Opcode::GetIterator => {
                        let value = self.stack.last().unwrap().clone();
                        let iterator = self.get_iterator(&value)?;
                        self.pop();
                        self.stack.push(iterator.clone());
                        iterators.push(iterator);
                    }
                    Opcode::ForInKeys => {
                        let source = self.stack.last().expect("for-in has a source").clone();
                        let keys = self.for_in_keys(&source)?;
                        self.pop();
                        self.stack.push(keys);
                    }
                    Opcode::IteratorStep => {
                        let record = self.stack.last().unwrap().clone();
                        iterators.retain(|active| active != &record);
                        let result = self.iterator_step(&record, true)?;
                        self.pop();
                        if let Some(value) = result {
                            iterators.push(record);
                            self.stack.push(value);
                        } else {
                            pc = operand;
                        }
                    }
                    Opcode::IteratorElision => {
                        let record = self.stack.last().unwrap().clone();
                        iterators.retain(|active| active != &record);
                        if self.iterator_step(&record, false)?.is_some() {
                            iterators.push(record);
                        }
                    }
                    Opcode::IteratorClose => {
                        let record = self.stack.last().unwrap().clone();
                        iterators.retain(|active| active != &record);
                        self.iterator_close(&record)?;
                        self.pop();
                    }
                    Opcode::IteratorFinish => {
                        let record = self.stack.last().unwrap().clone();
                        let Value::Object(id) = record else {
                            unreachable!("compiler only emits iterator records")
                        };
                        let done =
                            matches!(self.heap.get_own(id, "done")?, Some(Value::Bool(true)));
                        iterators.retain(|candidate| candidate != &record);
                        if !done {
                            self.iterator_close(&record)?;
                        }
                        self.pop();
                    }
                    Opcode::IteratorRest => {
                        let record = self.stack.last().unwrap().clone();
                        let base = self.stack.len() - 1;
                        iterators.retain(|candidate| candidate != &record);
                        iterators.push(record.clone());
                        // Keep accumulated values in a rooted managed array while
                        // subsequent next/done/value callbacks can trigger GC.
                        let array = self.array_from(Vec::new())?;
                        self.stack.push(array.clone());
                        while let Some(value) = self.iterator_step(&record, true)? {
                            self.charge_step()?;
                            self.array_push(&array, &value, 0)?;
                        }
                        iterators.retain(|candidate| candidate != &record);
                        self.stack.truncate(base);
                        self.stack.push(array);
                    }
                    Opcode::RequireObject => {
                        let value = self.stack.last().unwrap().clone();
                        self.coerce_object(&value)?;
                    }
                    Opcode::DestructureProperty => {
                        let base = self.stack.len() - 3;
                        let source = self.stack[base].clone();
                        let excluded = self.stack[base + 1].clone();
                        let key = self.coerce_property_key(&self.stack[base + 2].clone())?;
                        let value = self.get_property(&source, &key)?;
                        self.array_push(&excluded, &key.value(), 0)?;
                        self.stack.truncate(base);
                        self.stack.extend([source, excluded, value]);
                    }
                    Opcode::ObjectRest => {
                        let base = self.stack.len() - 2;
                        let source = self.stack[base].clone();
                        let excluded = self.stack[base + 1].clone();
                        let rest = self.destructure_object_rest(&source, &excluded)?;
                        self.stack.truncate(base);
                        self.stack.push(rest);
                    }
                    Opcode::CopyDataProperties => {
                        let base = self.stack.len() - 2;
                        let Value::Object(target) = self.stack[base].clone() else {
                            unreachable!("compiler creates an object literal target")
                        };
                        let source = self.stack[base + 1].clone();
                        self.copy_data_properties(target, &source, &[])?;
                        self.pop();
                    }
                    Opcode::Closure => {
                        let child = code.functions[operand].clone();
                        let (_, prototype) = self.string_intrinsics()?;
                        let constructor = self
                            .heap
                            .get(prototype, "constructor")?
                            .object_id()
                            .unwrap();
                        let function_prototype = self.heap.prototype(constructor)?.unwrap();
                        let mut captures = Vec::new();
                        for &slot in &child.captures {
                            captures.push(self.capture(slot as usize)?);
                        }
                        let this = if child.arrow {
                            if self.this == Value::Undefined && self.call_depth == 0 {
                                self.global("globalThis")?
                            } else {
                                self.this.clone()
                            }
                        } else {
                            Value::Undefined
                        };
                        let id = self.with_roots(|heap| {
                            heap.alloc_closure(child.clone(), captures, this, function_prototype)
                        })?;
                        self.stack.push(Value::Object(id));
                        // Arrow functions inherit their containing function's
                        // [[HomeObject]] together with lexical `this`.  Keeping
                        // the new closure on the operand stack first makes it a
                        // GC root while installing metadata may allocate.
                        if child.arrow {
                            if let Some(home) = self.home_object {
                                self.with_roots(|heap| heap.set_closure_home(id, home))?;
                            }
                            // A derived constructor's arrow may invoke `super()`.
                            // Store its resolved superclass on the arrow closure;
                            // the call frame then treats that closure as the
                            // lexical derived-constructor context.
                            if let Some(constructor) = self.class_constructor {
                                if let Some(base) = self.heap.class_base(constructor)? {
                                    self.with_roots(|heap| heap.set_class_base(id, base))?;
                                }
                            }
                        }
                        self.define_data(
                            id,
                            "name",
                            Value::String(child.function_name.clone().into()),
                            false,
                            false,
                            true,
                        )?;
                        self.define_data(
                            id,
                            "length",
                            Value::Number(child.function_length as f64),
                            false,
                            false,
                            true,
                        )?;
                        if child.constructible {
                            let object_prototype = self.object_prototype;
                            let prototype =
                                self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
                            self.define_data(
                                id,
                                "prototype",
                                Value::Object(prototype),
                                !child.class_constructor,
                                false,
                                false,
                            )?;
                            self.define_data(
                                prototype,
                                "constructor",
                                Value::Object(id),
                                true,
                                false,
                                true,
                            )?;
                        }
                    }
                    Opcode::This => {
                        if self.this == Value::Undefined && self.call_depth == 0 {
                            self.this = self.global("globalThis")?;
                        }
                        self.stack.push(self.this.clone());
                    }
                    Opcode::Argument => self
                        .stack
                        .push(native::argument(&self.arguments, operand).clone()),
                    Opcode::RestArguments => {
                        let array = self
                            .array_from(self.arguments.iter().skip(operand).cloned().collect())?;
                        self.stack.push(array);
                    }
                    Opcode::Return => return Ok(Some(Completion::Return(self.pop()))),
                    Opcode::TailRecur => {
                        let base = self.stack.len() - operand;
                        let args = self.stack[base..].to_vec();
                        self.stack.truncate(base);
                        return Ok(Some(Completion::TailRecur(args)));
                    }
                    Opcode::Yield => {
                        if !code.generator || !handlers.is_empty() {
                            return Err(RuntimeError::TypeError(
                                "yield is not supported in this execution context".into(),
                            ));
                        }
                        return Ok(Some(Completion::Yield(self.pop())));
                    }
                    Opcode::EnterWith => {
                        let object = self.pop();
                        let object = self.coerce_object(&object)?;
                        self.with_objects.push(Value::Object(object));
                    }
                    Opcode::LeaveWith => {
                        self.with_objects
                            .pop()
                            .expect("compiler balances with scopes");
                    }
                    Opcode::WithGet => {
                        let Value::String(name) = &code.constants[operand] else {
                            unreachable!("compiler emits a name")
                        };
                        let value = self.with_get(
                            &name.to_utf8().expect("compiler emits a UTF-8 identifier"),
                        )?;
                        self.stack.push(value);
                    }
                    Opcode::WithSet => {
                        let Value::String(name) = &code.constants[operand] else {
                            unreachable!("compiler emits a name")
                        };
                        self.with_set(
                            &name.to_utf8().expect("compiler emits a UTF-8 identifier"),
                            self.stack.last().expect("assignment has a value").clone(),
                        )?;
                    }
                    Opcode::Global => {
                        let Value::String(name) = &code.constants[operand] else {
                            unreachable!()
                        };
                        let value = self.global(&name.to_utf8().unwrap())?;
                        self.stack.push(value);
                    }
                    Opcode::ToPropertyKey => {
                        let value = self.stack.last().unwrap().clone();
                        let key = self.coerce_property_key(&value)?;
                        self.pop();
                        self.stack.push(key.value());
                    }
                    Opcode::Constant => {
                        self.check_string(&code.constants[operand])?;
                        self.stack.push(code.constants[operand].clone());
                    }
                    Opcode::GlobalString => {
                        let (constructor, _) = self.string_intrinsics()?;
                        self.stack.push(Value::Object(constructor));
                    }
                    Opcode::GetBinding => {
                        let value = self.binding_value(operand)?.ok_or_else(|| {
                            RuntimeError::ReferenceError(code.bindings[operand].name.clone())
                        })?;
                        self.stack.push(value);
                    }
                    Opcode::InitializeBinding => {
                        let value = self.pop();
                        self.store_binding(operand, value)?;
                    }
                    Opcode::StoreBinding => {
                        // ECMA-262 §9.1.1.1.5: TDZ takes precedence over the
                        // immutable-binding assignment error, including const.
                        if self.binding_value(operand)?.is_none() {
                            return Err(RuntimeError::ReferenceError(
                                code.bindings[operand].name.clone(),
                            ));
                        }
                        if !code.bindings[operand].mutable {
                            return Err(RuntimeError::TypeError(format!(
                                "assignment to constant {}",
                                code.bindings[operand].name
                            )));
                        }
                        self.store_binding(
                            operand,
                            self.stack.last().expect("store has a value").clone(),
                        )?;
                    }
                    Opcode::UnboundName | Opcode::TypeofName => {
                        let Value::String(name) = &code.constants[operand] else {
                            unreachable!("compiler emits a name")
                        };
                        let name = name.to_utf8().expect("compiler emits a UTF-8 identifier");
                        let value = self.lookup_global_name(&name)?;
                        if instruction.opcode == Opcode::TypeofName {
                            let value = value.unwrap_or(Value::Undefined);
                            self.stack
                                .push(Value::String(self.typeof_value(&value)?.into()));
                        } else {
                            self.stack
                                .push(value.ok_or(RuntimeError::ReferenceError(name))?);
                        }
                    }
                    Opcode::SetUnboundName => {
                        let Value::String(name) = &code.constants[operand] else {
                            unreachable!("compiler emits a name")
                        };
                        let name = name.to_utf8().expect("compiler emits a UTF-8 identifier");
                        let global = self.global("globalThis")?;
                        let global_id = global.object_id().expect("globalThis is an object");
                        let key: PropertyName = name.as_str().into();
                        if code.strict && !self.has_property(global_id, &key)? {
                            return Err(RuntimeError::ReferenceError(name));
                        }
                        let value = self.stack.last().expect("assignment has a value").clone();
                        self.set_property(&global, &key, &value)?;
                    }
                    Opcode::EnterScope => {
                        for slot in &code.scopes[operand] {
                            self.cells.remove(&(*slot as usize));
                            self.bindings[*slot as usize] =
                                if !code.bindings[*slot as usize].lexical {
                                    Some(Value::Undefined)
                                } else {
                                    None
                                };
                        }
                        self.active_scopes.push(operand as u32);
                        self.active_scope_slots.push(code.scopes[operand].clone());
                    }
                    Opcode::LeaveScope => self.leave_scope(code, operand as u32),
                    Opcode::Pop => {
                        self.pop();
                    }
                    Opcode::Dup => self
                        .stack
                        .push(self.stack.last().expect("dup has a value").clone()),
                    Opcode::Dup2 => {
                        let index = self.stack.len() - 2;
                        self.stack.push(self.stack[index].clone());
                        self.stack.push(self.stack[index + 1].clone());
                    }
                    Opcode::Add => self.binary(Self::add)?,
                    Opcode::Subtract => self.numeric(|a, b| a - b)?,
                    Opcode::Multiply => self.numeric(|a, b| a * b)?,
                    Opcode::Divide => self.numeric(|a, b| a / b)?,
                    Opcode::Remainder => self.numeric(|a, b| a % b)?,
                    Opcode::ShiftLeft | Opcode::ShiftRight | Opcode::UnsignedShiftRight => {
                        self.shift(instruction.opcode)?
                    }
                    Opcode::BitAnd | Opcode::BitXor | Opcode::BitOr => {
                        self.bitwise(instruction.opcode)?
                    }
                    Opcode::StrictEqual => self.binary(|_, a, b| Ok(Value::Bool(a == b)))?,
                    Opcode::StrictNotEqual => self.binary(|_, a, b| Ok(Value::Bool(a != b)))?,
                    Opcode::Equal => {
                        self.binary(|vm, a, b| vm.loose_equal(a, b).map(Value::Bool))?
                    }
                    Opcode::NotEqual => self
                        .binary(|vm, a, b| vm.loose_equal(a, b).map(|equal| Value::Bool(!equal)))?,
                    Opcode::Instanceof => self.binary(|vm, value, target| {
                        vm.has_instance(value, target, false).map(Value::Bool)
                    })?,
                    Opcode::In => self
                        .binary(|vm, key, object| vm.property_in(&key, &object).map(Value::Bool))?,
                    Opcode::Less => self.relational(|order| order == Ordering::Less)?,
                    Opcode::Greater => self.relational(|order| order == Ordering::Greater)?,
                    Opcode::LessEqual => self.relational(|order| order != Ordering::Greater)?,
                    Opcode::GreaterEqual => self.relational(|order| order != Ordering::Less)?,
                    Opcode::Negate
                    | Opcode::BitNot
                    | Opcode::ToNumber
                    | Opcode::ToString
                    | Opcode::Not
                    | Opcode::Typeof => {
                        let arg = self.stack.last().unwrap().clone();
                        let value = match instruction.opcode {
                            Opcode::Negate => self.negate(&arg)?,
                            Opcode::BitNot => self.bit_not(&arg)?,
                            Opcode::ToNumber => Value::Number(self.coerce_number(&arg)?),
                            Opcode::ToString => Value::String(self.coerce_string(&arg)?),
                            Opcode::Not => Value::Bool(!self.to_boolean(&arg)?),
                            _ => Value::String(self.typeof_value(&arg)?.into()),
                        };
                        self.check_string(&value)?;
                        self.pop();
                        self.stack.push(value);
                    }
                    Opcode::Jump => pc = operand,
                    Opcode::JumpIfFalse | Opcode::JumpIfTrue | Opcode::JumpIfNotNullish => {
                        let arg = self.pop();
                        let take = match instruction.opcode {
                            Opcode::JumpIfFalse => !self.to_boolean(&arg)?,
                            Opcode::JumpIfTrue => self.to_boolean(&arg)?,
                            _ => !matches!(arg, Value::Null | Value::Undefined),
                        };
                        if take {
                            pc = operand;
                        }
                    }
                    Opcode::NewObject => {
                        let prototype = self.object_prototype;
                        let id = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
                        self.stack.push(Value::Object(id));
                    }
                    Opcode::NewArray => {
                        let prototype = self.array_prototype;
                        let id = self
                            .with_roots(|heap| heap.alloc_array(operand as u32, Some(prototype)))?;
                        self.stack.push(Value::Object(id));
                    }
                    Opcode::GetProperty | Opcode::GetMethod => {
                        let (receiver, key) = self.property_reference()?;
                        let value = self.get_property(&receiver, &key)?;
                        self.check_string(&value)?;
                        self.stack.push(value);
                        if instruction.opcode == Opcode::GetMethod {
                            self.stack.push(receiver);
                        }
                    }
                    Opcode::Call | Opcode::Construct => {
                        // Leave every call input on the stack until dispatch
                        // completes, so native allocations see all GC roots.
                        let base = self.stack.len() - operand - 2;
                        let result = self.call_native(
                            self.stack[base].clone(),
                            self.stack[base + 1].clone(),
                            self.stack[base + 2..].to_vec(),
                            instruction.opcode == Opcode::Construct,
                        )?;
                        self.check_string(&result)?;
                        self.stack.truncate(base);
                        self.stack.push(result);
                    }
                    Opcode::SetProperty => {
                        let value = self.pop();
                        let (object, key) = self.property_reference()?;
                        self.set_property(&object, &key, &value)?;
                        self.stack.push(value);
                    }
                    Opcode::SetDestructureProperty => {
                        // A destructuring leaf has already produced its value;
                        // evaluating a member target appends its object/key after
                        // that value. Preserve the value for the caller to pop.
                        let base = self.stack.len() - 3;
                        let value = self.stack[base].clone();
                        let object = self.stack[base + 1].clone();
                        let key = self.coerce_property_key(&self.stack[base + 2].clone())?;
                        self.set_property(&object, &key, &value)?;
                        self.stack.truncate(base);
                        self.stack.push(value);
                    }
                    Opcode::UpdateProperty => {
                        let (object, key) = self.property_reference()?;
                        self.stack.push(object.clone());
                        let old = self.get_property(&object, &key)?;
                        let old = self.coerce_number(&old)?;
                        let new = if operand & 1 == 0 {
                            old + 1.0
                        } else {
                            old - 1.0
                        };
                        self.set_property(&object, &key, &Value::Number(new))?;
                        self.stack.pop();
                        self.stack
                            .push(Value::Number(if operand & 2 == 0 { old } else { new }));
                    }
                    Opcode::SetLiteralPrototype => {
                        let value = self.pop();
                        let Value::Object(object) = self.pop() else {
                            unreachable!("literal receiver is an object")
                        };
                        match value {
                            Value::Object(prototype) => {
                                self.heap.set_prototype(object, Some(prototype))?
                            }
                            Value::Null => self.heap.set_prototype(object, None)?,
                            _ => {} // Literal __proto__ with a primitive value has no effect.
                        }
                    }
                    Opcode::SetCompletion => {
                        self.completion = self.pop();
                        self.completion_empty = false;
                    }
                    Opcode::ClearCompletion => {
                        self.completion = Value::Undefined;
                        self.completion_empty = true;
                    }
                    Opcode::PushHandler => {
                        debug_assert!(operand < code.handlers.len());
                        handlers.push(HandlerFrame {
                            metadata: operand,
                            stack_depth: self.stack.len(),
                            scope_depth: self.active_scopes.len(),
                            iterator_depth: iterators.len(),
                            with_depth: self.with_objects.len(),
                            state: HandlerState::Try,
                            pending: None,
                        });
                    }
                    Opcode::PopHandler => {
                        handlers
                            .pop()
                            .expect("compiler pops its active try handler");
                    }
                    Opcode::SaveCompletion => self
                        .completion_saves
                        .push((self.completion.clone(), self.completion_empty)),
                    Opcode::ResumeCompletion => return Ok(Some(Completion::Resume(operand))),
                    Opcode::AbruptJump => {
                        let jump = &code.abrupt_jumps[operand];
                        return Ok(Some(Completion::Jump {
                            cleanup: jump.cleanup as usize,
                            target: jump.target as usize,
                        }));
                    }
                    Opcode::Halt => return Ok(Some(Completion::Halt(self.completion.clone()))),
                }
                Ok(None)
            })();
            let completion = match outcome {
                Ok(completion) => completion,
                Err(error) if error.is_catchable() => Some(Completion::Throw(error)),
                Err(error) => return Err(error),
            };
            if let Some(completion) = completion {
                if let Completion::Yield(value) = completion {
                    return Ok(InterpreterExit::Yield { value, pc });
                }
                match self.resolve_completion(code, &mut handlers, iterators, completion)? {
                    CompletionAction::Continue => {}
                    CompletionAction::Jump(target) => pc = target,
                    CompletionAction::Return(value) => return Ok(InterpreterExit::Return(value)),
                    CompletionAction::TailRecur(args) => {
                        self.stack.truncate(stack_base);
                        self.unwind_scopes(code, 0);
                        self.unwind_with(0);
                        self.close_iterators_to(iterators, 0);
                        handlers.clear();
                        self.pending_completions.truncate(pending_base);
                        self.completion_saves.truncate(save_base);
                        self.completion = Value::Undefined;
                        self.completion_empty = true;
                        self.this = Value::Undefined;
                        self.arguments = args;
                        pc = 0;
                    }
                    CompletionAction::Throw(error) => return Err(error),
                }
            }
        }
    }

    fn charge_step(&mut self) -> Result<(), RuntimeError> {
        if self.remaining_instructions == 0 {
            return Err(RuntimeError::InstructionLimit);
        }
        self.remaining_instructions -= 1;
        Ok(())
    }

    fn get_property(
        &mut self,
        receiver: &Value,
        key: &PropertyName,
    ) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        self.stack.push(receiver.clone());
        let result = self.get_property_value(receiver, key);
        self.stack.truncate(base);
        result
    }

    fn get_property_value(
        &mut self,
        receiver: &Value,
        key: &PropertyName,
    ) -> Result<Value, RuntimeError> {
        match receiver {
            Value::Object(id) => {
                if self.string_intrinsics.is_none()
                    && (key == "toString"
                        || key == "valueOf"
                        || key == "join"
                        || key == "forEach"
                        || key == "includes"
                        || *key == PropertyName::from(JsSymbol::well_known("iterator")))
                {
                    self.string_intrinsics()?;
                }
                if key == "propertyIsEnumerable" {
                    self.property_is_enumerable_intrinsic()?;
                }
                self.get_from_prototype(*id, receiver, key)
            }
            Value::String(string) => {
                if let PropertyName::String(key) = key {
                    if let Some(value) = string.own_property(key) {
                        return Ok(value);
                    }
                }
                let (_, prototype) = self.string_intrinsics()?;
                self.get_from_prototype(prototype, receiver, key)
            }
            Value::Null | Value::Undefined => Err(RuntimeError::TypeError(
                "cannot access a property of null or undefined".into(),
            )),
            _ => {
                let constructor = self.global(match receiver {
                    Value::Symbol(_) => "Symbol",
                    Value::Bool(_) => "Boolean",
                    _ => "Number",
                })?;
                let prototype = self
                    .get_property(&constructor, &"prototype".into())?
                    .object_id()
                    .unwrap();
                self.get_from_prototype(prototype, receiver, key)
            }
        }
    }

    fn set_property(
        &mut self,
        receiver: &Value,
        key: &PropertyName,
        value: &Value,
    ) -> Result<(), RuntimeError> {
        let base = self.stack.len();
        self.stack.extend([receiver.clone(), value.clone()]);
        let result = self.set_property_value(receiver, key, value);
        self.stack.truncate(base);
        result
    }

    fn set_property_value(
        &mut self,
        receiver: &Value,
        key: &PropertyName,
        value: &Value,
    ) -> Result<(), RuntimeError> {
        // ToObject provides the lookup chain; accessor calls retain the
        // original primitive receiver. Creating a data property still fails.
        let object = self.coerce_object(receiver)?;
        self.stack.push(Value::Object(object));
        let mut current = Some(object);
        while let Some(id) = current {
            if let Some(desc) = self.heap.get_own_property_descriptor(id, key)? {
                if desc.accessor() {
                    let setter = desc.set.unwrap_or(Value::Undefined);
                    if self.is_callable(&setter)? {
                        self.call_native(setter, receiver.clone(), vec![value.clone()], false)?;
                        return Ok(());
                    }
                    return if self.strict {
                        Err(RuntimeError::TypeError("property has no setter".into()))
                    } else {
                        Ok(())
                    };
                }
                if desc.writable == Some(false) {
                    return if self.strict {
                        Err(RuntimeError::TypeError("property is read-only".into()))
                    } else {
                        Ok(())
                    };
                }
                break;
            }
            current = self.heap.prototype(id)?;
        }
        if !matches!(receiver, Value::Object(_)) {
            return if self.strict {
                Err(RuntimeError::TypeError(
                    "cannot assign to primitive property".into(),
                ))
            } else {
                Ok(())
            };
        }
        let stored = if key == "length" && self.heap.is_array(object)? {
            self.array_length_value(value)?
        } else {
            value.clone()
        };
        match self.with_roots(|heap| heap.set(object, key, stored)) {
            Err(RuntimeError::Heap(HeapError::ReadOnlyProperty)) => {
                if self.strict {
                    Err(RuntimeError::TypeError(
                        "property cannot be assigned".into(),
                    ))
                } else {
                    Ok(())
                }
            }
            result => result,
        }
    }

    fn property_reference(&mut self) -> Result<(Value, PropertyName), RuntimeError> {
        let key = self.pop();
        let key = self.coerce_property_key(&key)?;
        match self.pop() {
            Value::Null | Value::Undefined => Err(RuntimeError::TypeError(
                "cannot access a property of null or undefined".into(),
            )),
            receiver => Ok((receiver, key)),
        }
    }

    fn set_class_heritage(&mut self) -> Result<(), RuntimeError> {
        let base = self.pop();
        let class = self
            .stack
            .last()
            .expect("class closure remains on the stack")
            .object_id()
            .expect("compiler emits a class closure before heritage");
        let prototype = self
            .heap
            .get(class, "prototype")?
            .object_id()
            .expect("class constructors have a prototype object");
        let (constructor_parent, instance_parent) = match &base {
            Value::Null => (None, None),
            Value::Object(base) if self.is_constructor(&Value::Object(*base))? => {
                let instance_parent =
                    match self.get_property(&Value::Object(*base), &"prototype".into())? {
                        Value::Object(prototype) => Some(prototype),
                        Value::Null => None,
                        _ => {
                            return Err(RuntimeError::TypeError(
                                "superclass prototype must be an object or null".into(),
                            ))
                        }
                    };
                (Some(*base), instance_parent)
            }
            _ => {
                return Err(RuntimeError::TypeError(
                    "class extends value is not a constructor or null".into(),
                ))
            }
        };
        self.heap.set_prototype(class, constructor_parent)?;
        self.heap.set_prototype(prototype, instance_parent)?;
        self.with_roots(|heap| heap.set_class_base(class, base))?;
        self.with_roots(|heap| heap.set_closure_home(class, prototype))?;
        Ok(())
    }

    fn set_class_home(&mut self) -> Result<(), RuntimeError> {
        let class = self
            .stack
            .last()
            .expect("class closure remains on the stack")
            .object_id()
            .expect("compiler emits a class closure before setting its home object");
        let prototype = self
            .heap
            .get(class, "prototype")?
            .object_id()
            .expect("class constructors have a prototype object");
        self.with_roots(|heap| heap.set_closure_home(class, prototype))?;
        Ok(())
    }

    fn super_base(&self) -> Result<ObjectId, RuntimeError> {
        let home = self.home_object.ok_or_else(|| {
            RuntimeError::TypeError("super is not available in this function".into())
        })?;
        self.heap
            .prototype(home)?
            .ok_or_else(|| RuntimeError::TypeError("superclass is null".into()))
    }

    fn super_get(&mut self, key: &PropertyName) -> Result<Value, RuntimeError> {
        let base = self.super_base()?;
        self.get_from_prototype(base, &self.this.clone(), key)
    }

    fn super_set(&mut self, key: &PropertyName, value: &Value) -> Result<(), RuntimeError> {
        let base = self.super_base()?;
        let mut current = Some(base);
        while let Some(object) = current {
            if let Some(descriptor) = self.heap.get_own_property_descriptor(object, key)? {
                if descriptor.accessor() {
                    let setter = descriptor.set.unwrap_or(Value::Undefined);
                    if self.is_callable(&setter)? {
                        self.call_native(setter, self.this.clone(), vec![value.clone()], false)?;
                        return Ok(());
                    }
                    return Err(RuntimeError::TypeError(
                        "super property has no setter".into(),
                    ));
                }
                if descriptor.writable == Some(false) {
                    return Err(RuntimeError::TypeError(
                        "super property is read-only".into(),
                    ));
                }
                break;
            }
            current = self.heap.prototype(object)?;
        }
        let Value::Object(receiver) = self.this else {
            return Err(RuntimeError::ReferenceError(
                "this is uninitialized before super()".into(),
            ));
        };
        self.with_roots(|heap| heap.set(receiver, key, value.clone()))
            .map_err(|error| match error {
                RuntimeError::Heap(HeapError::ReadOnlyProperty) => {
                    RuntimeError::TypeError("super property cannot be assigned".into())
                }
                error => error,
            })
    }

    fn super_call(&mut self, args: Vec<Value>) -> Result<Value, RuntimeError> {
        let constructor = self.class_constructor.ok_or_else(|| {
            RuntimeError::TypeError("super() is not available in this function".into())
        })?;
        let base = self.heap.class_base(constructor)?.ok_or_else(|| {
            RuntimeError::TypeError("super() requires a derived constructor".into())
        })?;
        if matches!(base, Value::Null) {
            return Err(RuntimeError::TypeError("super constructor is null".into()));
        }
        let value =
            self.call_with_target(base, Value::Undefined, args, true, self.new_target.clone())?;
        self.this = value.clone();
        Ok(value)
    }
    /// Implements CopyDataProperties for an object-rest binding.  The
    /// compiler supplies an internal array of already-coerced excluded keys;
    /// getters are read from the original source object and copied as normal
    /// enumerable data properties onto a fresh ordinary object.
    fn destructure_object_rest(
        &mut self,
        source: &Value,
        excluded: &Value,
    ) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        let result = (|| {
            let length_value = self.get_property(excluded, &"length".into())?;
            let length = self.coerce_length(&length_value)? as u64;
            let mut excluded_keys = Vec::new();
            for index in 0..length {
                self.charge_step()?;
                let key = self.get_property(excluded, &index.to_string().into())?;
                excluded_keys.push(self.coerce_property_key(&key)?);
            }
            let prototype = self.object_prototype;
            let target = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
            self.stack.push(Value::Object(target));
            self.copy_data_properties(target, source, &excluded_keys)?;
            self.stack.pop();
            Ok(Value::Object(target))
        })();
        self.stack.truncate(base);
        result
    }

    /// The enumerable-property portion of CopyDataProperties.  Object spread
    /// skips nullish sources, while object-rest has already performed
    /// RequireObjectCoercible before arriving here.
    fn copy_data_properties(
        &mut self,
        target: ObjectId,
        source: &Value,
        excluded: &[PropertyName],
    ) -> Result<(), RuntimeError> {
        if matches!(source, Value::Null | Value::Undefined) {
            return Ok(());
        }
        let source_object = self.coerce_object(source)?;
        let base = self.stack.len();
        self.stack.push(Value::Object(source_object));
        let result = (|| {
            for key in self.heap.own_property_keys(source_object)? {
                self.charge_step()?;
                if excluded.iter().any(|excluded| excluded == &key) {
                    continue;
                }
                let descriptor = self
                    .heap
                    .get_own_property_descriptor(source_object, &key)?
                    .expect("own key has an own descriptor");
                if descriptor.enumerable != Some(true) {
                    continue;
                }
                let value = self.get_property(&Value::Object(source_object), &key)?;
                self.with_roots(|heap| {
                    heap.define_own_property(
                        target,
                        key,
                        PropertyDescriptor::data(value, true, true, true),
                    )
                })?;
            }
            Ok(())
        })();
        self.stack.truncate(base);
        result
    }

    /// Snapshots the enumerable string keys visible through an object's
    /// prototype chain. Non-enumerable own keys still suppress an inherited
    /// key with the same name; symbols never participate in `for-in`.
    fn for_in_keys(&mut self, source: &Value) -> Result<Value, RuntimeError> {
        let mut current = Some(self.coerce_object(source)?);
        let mut seen = HashSet::new();
        let mut keys = Vec::new();
        while let Some(object) = current {
            for key in self.heap.own_property_keys(object)? {
                if !seen.insert(key.clone()) {
                    continue;
                }
                if let PropertyName::String(key) = key {
                    if self
                        .heap
                        .get_own_property_descriptor(object, PropertyName::String(key.clone()))?
                        .is_some_and(|descriptor| descriptor.enumerable == Some(true))
                    {
                        keys.push(Value::String(key));
                    }
                }
            }
            current = self.heap.prototype(object)?;
        }
        self.array_from(keys)
    }

    fn string_intrinsics(&mut self) -> Result<(ObjectId, ObjectId), RuntimeError> {
        if let Some(intrinsics) = self.string_intrinsics {
            return Ok(intrinsics);
        }
        let object_prototype = self.object_prototype;
        let function_prototype = self.with_roots(|heap| {
            heap.alloc_native_function(NativeFunction::Empty, "", object_prototype)
        })?;
        let constructor = self.with_roots(|heap| {
            heap.alloc_native_function(NativeFunction::String, "String", function_prototype)
        })?;
        let root = self.heap.root(constructor)?;
        let result = (|| {
            let prototype = self.with_roots(|heap| {
                heap.alloc_string(JsString::default(), Some(object_prototype))
            })?;
            self.define_data(
                constructor,
                "prototype",
                Value::Object(prototype),
                false,
                false,
                false,
            )?;
            self.define_data(
                prototype,
                "constructor",
                Value::Object(constructor),
                true,
                false,
                true,
            )?;
            self.define_data(
                constructor,
                "name",
                Value::String("String".into()),
                false,
                false,
                true,
            )?;
            self.define_data(
                constructor,
                "length",
                Value::Number(1.0),
                false,
                false,
                true,
            )?;
            self.define_data(
                function_prototype,
                "length",
                Value::Number(0.0),
                false,
                false,
                true,
            )?;
            self.define_data(
                function_prototype,
                "name",
                Value::String(JsString::default()),
                false,
                false,
                true,
            )?;
            self.install_native(
                function_prototype,
                function_prototype,
                "call",
                1,
                NativeFunction::Call,
            )?;
            self.install_native(
                function_prototype,
                function_prototype,
                "apply",
                2,
                NativeFunction::Apply,
            )?;
            self.install_native(
                function_prototype,
                function_prototype,
                "bind",
                1,
                NativeFunction::Bind,
            )?;
            self.install_symbol_native(
                function_prototype,
                function_prototype,
                "hasInstance",
                1,
                NativeFunction::HasInstance,
            )?;
            self.install_native(
                function_prototype,
                function_prototype,
                "toString",
                0,
                NativeFunction::FunctionToString,
            )?;
            self.install_native(
                constructor,
                function_prototype,
                "fromCharCode",
                1,
                NativeFunction::FromCharCode,
            )?;
            self.install_native(
                constructor,
                function_prototype,
                "fromCodePoint",
                1,
                NativeFunction::FromCodePoint,
            )?;
            self.install_native(
                constructor,
                function_prototype,
                "raw",
                1,
                NativeFunction::Raw,
            )?;
            self.install_native(
                prototype,
                function_prototype,
                "split",
                2,
                NativeFunction::Split,
            )?;
            self.install_native(
                prototype,
                function_prototype,
                "replace",
                2,
                NativeFunction::Replace,
            )?;
            self.install_native(
                prototype,
                function_prototype,
                "replaceAll",
                2,
                NativeFunction::ReplaceAll,
            )?;
            for &(name, length, method) in native::STRING_METHODS {
                self.install_native(
                    prototype,
                    function_prototype,
                    name,
                    length,
                    NativeFunction::StringMethod(method),
                )?;
            }
            for (name, length, method) in [
                ("toLocaleLowerCase", 0, NativeFunction::ToLocaleLowerCase),
                ("toLocaleUpperCase", 0, NativeFunction::ToLocaleUpperCase),
                ("localeCompare", 1, NativeFunction::LocaleCompare),
            ] {
                self.install_native(prototype, function_prototype, name, length, method)?;
            }
            for (alias, original) in [("trimLeft", "trimStart"), ("trimRight", "trimEnd")] {
                let function = self.heap.get(prototype, original)?;
                self.define_data(prototype, alias, function, true, false, true)?;
            }
            self.install_symbol_native(
                prototype,
                function_prototype,
                "iterator",
                0,
                NativeFunction::StringIterator,
            )?;
            for (name, method) in [
                ("match", native::PatternMethod::Match),
                ("matchAll", native::PatternMethod::MatchAll),
                ("search", native::PatternMethod::Search),
            ] {
                self.install_native(
                    prototype,
                    function_prototype,
                    name,
                    1,
                    NativeFunction::Pattern(method),
                )?;
            }
            self.install_native(
                object_prototype,
                function_prototype,
                "toString",
                0,
                NativeFunction::ObjectToString,
            )?;
            self.install_native(
                object_prototype,
                function_prototype,
                "valueOf",
                0,
                NativeFunction::ObjectValueOf,
            )?;
            self.install_native(
                self.array_prototype,
                function_prototype,
                "toString",
                0,
                NativeFunction::ArrayToString,
            )?;
            self.install_native(
                self.array_prototype,
                function_prototype,
                "concat",
                1,
                NativeFunction::ArrayConcat,
            )?;
            self.install_native(
                self.array_prototype,
                function_prototype,
                "join",
                1,
                NativeFunction::ArrayJoin,
            )?;
            self.install_native(
                self.array_prototype,
                function_prototype,
                "forEach",
                1,
                NativeFunction::ArrayForEach,
            )?;
            self.install_native(
                self.array_prototype,
                function_prototype,
                "includes",
                1,
                NativeFunction::ArrayIncludes,
            )?;
            self.install_symbol_native(
                self.array_prototype,
                function_prototype,
                "iterator",
                0,
                NativeFunction::ArrayIterator,
            )?;
            Ok((constructor, prototype))
        })();
        match result {
            Ok(intrinsics) => {
                self.string_intrinsics = Some(intrinsics);
                Ok(intrinsics)
            }
            Err(error) => {
                // The only pre-existing owners modified by bootstrap are
                // these two empty prototypes. Roll back published edges too.
                for (owner, key) in [
                    (object_prototype, PropertyName::from("toString")),
                    (object_prototype, "valueOf".into()),
                    (self.array_prototype, "toString".into()),
                    (self.array_prototype, "concat".into()),
                    (self.array_prototype, "join".into()),
                    (self.array_prototype, "forEach".into()),
                    (self.array_prototype, "includes".into()),
                    (
                        self.array_prototype,
                        JsSymbol::well_known("iterator").into(),
                    ),
                ] {
                    self.heap.delete(owner, key)?;
                }
                self.heap.unroot(root)?;
                Err(error)
            }
        }
    }

    fn install_native(
        &mut self,
        owner: ObjectId,
        prototype: ObjectId,
        name: &str,
        length: u32,
        function: NativeFunction,
    ) -> Result<(), RuntimeError> {
        let id = self.with_roots(|heap| heap.alloc_native_function(function, name, prototype))?;
        self.stack.push(Value::Object(id));
        let result = (|| {
            self.define_data(id, "name", Value::String(name.into()), false, false, true)?;
            self.define_data(
                id,
                "length",
                Value::Number(f64::from(length)),
                false,
                false,
                true,
            )?;
            self.define_data(owner, name, Value::Object(id), true, false, true)
        })();
        self.stack.pop();
        result?;
        Ok(())
    }

    fn property_is_enumerable_intrinsic(&mut self) -> Result<(), RuntimeError> {
        if self
            .heap
            .get_own_property_descriptor(self.object_prototype, "propertyIsEnumerable")?
            .is_some()
        {
            return Ok(());
        }
        let function_prototype = self.string_intrinsics()?.1;
        self.install_native(
            self.object_prototype,
            function_prototype,
            "propertyIsEnumerable",
            1,
            NativeFunction::ObjectMethod(native::ObjectMethod::PropertyIsEnumerable),
        )
    }

    fn call_native(
        &mut self,
        callee: Value,
        receiver: Value,
        args: Vec<Value>,
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        let target = if construct {
            callee.clone()
        } else {
            Value::Undefined
        };
        self.call_with_target(callee, receiver, args, construct, target)
    }

    fn call_with_target(
        &mut self,
        callee: Value,
        receiver: Value,
        args: Vec<Value>,
        construct: bool,
        target: Value,
    ) -> Result<Value, RuntimeError> {
        if self.call_depth >= 32 {
            return Err(RuntimeError::RangeError(
                "maximum call depth exceeded".into(),
            ));
        }
        // Arrow functions inherit their enclosing `new.target`.  This is
        // observable when a derived-constructor arrow invokes `super()`:
        // the superclass must allocate with the original derived class.
        let arrow = if !construct {
            match callee.object_id() {
                Some(id) => self
                    .heap
                    .closure(id)?
                    .is_some_and(|(code, _, _, _, _)| code.arrow),
                None => false,
            }
        } else {
            false
        };
        let target = if arrow {
            self.new_target.clone()
        } else {
            target
        };
        self.charge_step()?;
        let base = self.stack.len();
        self.stack.extend([callee.clone(), receiver.clone()]);
        self.stack.extend(args.iter().cloned());
        self.stack.push(target.clone());
        let previous_target = std::mem::replace(&mut self.new_target, target);
        self.call_depth += 1;
        let result = self
            .dispatch_call(callee, receiver, args, construct)
            .and_then(|value| {
                self.check_string(&value)?;
                Ok(value)
            });
        self.new_target = previous_target;
        self.call_depth -= 1;
        self.stack.truncate(base);
        result
    }

    fn dispatch_call(
        &mut self,
        mut callee: Value,
        mut receiver: Value,
        mut args: Vec<Value>,
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        // Bound wrappers have no execution contexts of their own. Walk them
        // with fuel rather than consuming Rust stack or the JS frame limit.
        let mut prefixes = Vec::new();
        while let Value::Object(id) = callee {
            let Some(bound) = self.heap.bound_function(id)?.cloned() else {
                break;
            };
            self.charge_step()?;
            if construct && !bound.constructible {
                return Err(RuntimeError::TypeError(
                    "bound target is not a constructor".into(),
                ));
            }
            if construct && self.new_target == callee {
                self.new_target = Value::Object(bound.target);
            }
            callee = Value::Object(bound.target);
            receiver = bound.this;
            prefixes.push(bound.args);
        }
        if !prefixes.is_empty() {
            // Concatenate once, starting at the innermost wrapper, to avoid
            // repeatedly copying the accumulated arguments of long chains.
            args = prefixes.into_iter().rev().flatten().chain(args).collect();
        }
        if let Value::Object(id) = callee {
            if let Some((code, captures, lexical_this, home, class_base)) = self.heap.closure(id)? {
                let receiver = if code.arrow { lexical_this } else { receiver };
                return self.call_closure(builtins::ClosureCall {
                    code,
                    captures,
                    callee,
                    receiver,
                    args,
                    construct,
                    home,
                    class_base,
                });
            }
        }
        let function = if let Value::Object(id) = callee {
            self.heap.native_function(id)?
        } else {
            None
        };
        let Some(function) = function else {
            return Err(RuntimeError::TypeError("value is not callable".into()));
        };
        if construct
            && !matches!(
                function,
                NativeFunction::String
                    | NativeFunction::Array
                    | NativeFunction::Object
                    | NativeFunction::RegExp
                    | NativeFunction::Collator
                    | NativeFunction::Locale
                    | NativeFunction::Error(_)
                    | NativeFunction::PrimitiveConstructor(_)
            )
        {
            return Err(RuntimeError::TypeError("value is not a constructor".into()));
        }
        self.native_call(function, receiver, args, construct)
    }

    fn unbox_string(&self, value: &Value) -> Result<Value, RuntimeError> {
        if let Value::Object(id) = value {
            if let Some(string) = self.heap.boxed_string(*id)? {
                return Ok(Value::String(string.clone()));
            }
        }
        Ok(value.clone())
    }

    fn string_receiver(&mut self, value: &Value) -> Result<JsString, RuntimeError> {
        if matches!(value, Value::Null | Value::Undefined) {
            return Err(RuntimeError::TypeError(
                "String method requires a non-null receiver".into(),
            ));
        }
        self.coerce_string(value)
    }

    fn string_raw(&mut self, args: &[Value]) -> Result<Value, RuntimeError> {
        let template = Value::Object(self.coerce_object(native::argument(args, 0))?);
        self.stack.push(template.clone());
        let raw = self.get_property(&template, &"raw".into())?;
        self.stack.push(raw.clone());
        let raw = Value::Object(self.coerce_object(&raw)?);
        self.stack.push(raw.clone());
        let length = self.get_property(&raw, &"length".into())?;
        let count = self.coerce_length(&length)? as u64;
        let mut result = JsString::default();
        for index in 0..count {
            self.charge_step()?; // A huge array-like length with empty entries must still terminate.
            let literal = self.get_property(&raw, &index.to_string().into())?;
            native::append(
                &mut result,
                &self.coerce_string(&literal)?,
                self.config.max_string_bytes,
            )?;
            if index + 1 < count {
                if let Some(substitution) = args.get(index as usize + 1) {
                    native::append(
                        &mut result,
                        &self.coerce_string(substitution)?,
                        self.config.max_string_bytes,
                    )?;
                }
            }
        }
        Ok(Value::String(result))
    }

    fn string_split(&mut self, receiver: &Value, args: &[Value]) -> Result<Value, RuntimeError> {
        if matches!(receiver, Value::Null | Value::Undefined) {
            return Err(RuntimeError::TypeError(
                "String method requires a non-null receiver".into(),
            ));
        }
        let separator = native::argument(args, 0);
        if !matches!(separator, Value::Null | Value::Undefined) {
            let method = self.get_method(separator, &JsSymbol::well_known("split").into())?;
            if method != Value::Undefined {
                return self.call_native(
                    method,
                    separator.clone(),
                    vec![receiver.clone(), native::argument(args, 1).clone()],
                    false,
                );
            }
        }
        let string = self.string_receiver(receiver)?;
        let separator = native::argument(args, 0);
        let limit = native::argument(args, 1);
        let limit = if matches!(limit, Value::Undefined) {
            u32::MAX
        } else {
            native::uint32(&Value::Number(self.coerce_number(limit)?))?
        };
        // Even a zero limit converts the separator first (§22.1.3.23).
        let search = self.coerce_string(separator)?;
        let prototype = self.array_prototype;
        let array = self.with_roots(|heap| heap.alloc_array(0, Some(prototype)))?;
        self.stack.push(Value::Object(array)); // Root the growing result through every store.
        let mut count = 0u32;
        let units = string.as_code_units();
        let mut start = 0;
        while count < limit {
            self.charge_step()?;
            let end = if matches!(separator, Value::Undefined) {
                units.len()
            } else if search.is_empty() {
                if start == units.len() {
                    break;
                }
                start + 1
            } else if search.len() <= units.len() {
                (start..=units.len() - search.len())
                    .find(|&index| units[index..].starts_with(search.as_code_units()))
                    .unwrap_or(units.len())
            } else {
                units.len()
            };
            let value = Value::String(JsString::from_code_units(units[start..end].to_vec()));
            self.with_roots(|heap| heap.set(array, count.to_string(), value))?;
            count += 1;
            if end == units.len() {
                break;
            }
            start = end + search.len();
        }
        Ok(Value::Object(array))
    }

    fn string_replace(
        &mut self,
        receiver: &Value,
        args: &[Value],
        all: bool,
    ) -> Result<Value, RuntimeError> {
        if matches!(receiver, Value::Null | Value::Undefined) {
            return Err(RuntimeError::TypeError(
                "String method requires a non-null receiver".into(),
            ));
        }
        let search = native::argument(args, 0);
        if !matches!(search, Value::Null | Value::Undefined) {
            if all {
                self.require_global_pattern(search)?;
            }
            let method = self.get_method(search, &JsSymbol::well_known("replace").into())?;
            if method != Value::Undefined {
                return self.call_native(
                    method,
                    search.clone(),
                    vec![receiver.clone(), native::argument(args, 1).clone()],
                    false,
                );
            }
        }
        let string = self.string_receiver(receiver)?;
        let search = self.coerce_string(native::argument(args, 0))?;
        let replace = native::argument(args, 1);
        let callable = self.is_callable(replace)?;
        let template = if callable {
            JsString::default()
        } else {
            self.coerce_string(replace)?
        };
        let mut result = JsString::default();
        let mut end = 0;
        let mut next = 0;
        if search.len() <= string.len() {
            while let Some(position) = (next..=string.len() - search.len())
                .find(|&index| string.as_code_units()[index..].starts_with(search.as_code_units()))
            {
                self.charge_step()?;
                let replacement = if callable {
                    let value = self.call_native(
                        replace.clone(),
                        Value::Undefined,
                        vec![
                            Value::String(search.clone()),
                            Value::Number(position as f64),
                            Value::String(string.clone()),
                        ],
                        false,
                    )?;
                    self.coerce_string(&value)?
                } else {
                    native::substitution(
                        &string,
                        &search,
                        position,
                        &template,
                        self.config.max_string_bytes,
                    )?
                };
                native::append(
                    &mut result,
                    &JsString::from_code_units(string.as_code_units()[end..position].to_vec()),
                    self.config.max_string_bytes,
                )?;
                native::append(&mut result, &replacement, self.config.max_string_bytes)?;
                end = position + search.len();
                if !all {
                    break;
                }
                next = position + search.len().max(1);
            }
        }
        native::append(
            &mut result,
            &JsString::from_code_units(string.as_code_units()[end..].to_vec()),
            self.config.max_string_bytes,
        )?;
        Ok(Value::String(result))
    }

    fn binary(
        &mut self,
        operation: impl FnOnce(&mut Self, Value, Value) -> Result<Value, RuntimeError>,
    ) -> Result<(), RuntimeError> {
        let base = self.stack.len() - 2;
        let value = operation(self, self.stack[base].clone(), self.stack[base + 1].clone())?;
        self.stack.truncate(base);
        self.stack.push(value);
        Ok(())
    }

    fn numeric(&mut self, operation: fn(f64, f64) -> f64) -> Result<(), RuntimeError> {
        self.binary(|vm, a, b| {
            Ok(Value::Number(operation(
                vm.coerce_number(&a)?,
                vm.coerce_number(&b)?,
            )))
        })
    }

    fn negate(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        Ok(match self.coerce_numeric(value)? {
            primitive::Numeric::Number(value) => Value::Number(-value),
            primitive::Numeric::BigInt(value) => Value::BigInt(-value),
        })
    }

    fn bit_not(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        Ok(match self.coerce_numeric(value)? {
            primitive::Numeric::Number(value) => {
                Value::Number((!primitive::to_int32(value)) as f64)
            }
            primitive::Numeric::BigInt(value) => Value::BigInt(!value),
        })
    }

    fn bitwise(&mut self, operation: Opcode) -> Result<(), RuntimeError> {
        self.binary(|vm, left, right| {
            let left = vm.coerce_numeric(&left)?;
            let right = vm.coerce_numeric(&right)?;
            match (left, right) {
                (primitive::Numeric::Number(left), primitive::Numeric::Number(right)) => {
                    let left = primitive::to_int32(left);
                    let right = primitive::to_int32(right);
                    let value = match operation {
                        Opcode::BitAnd => left & right,
                        Opcode::BitXor => left ^ right,
                        Opcode::BitOr => left | right,
                        _ => unreachable!("bitwise caller selects a bitwise opcode"),
                    };
                    Ok(Value::Number(value as f64))
                }
                (primitive::Numeric::BigInt(left), primitive::Numeric::BigInt(right)) => {
                    let value = match operation {
                        Opcode::BitAnd => left & right,
                        Opcode::BitXor => left ^ right,
                        Opcode::BitOr => left | right,
                        _ => unreachable!("bitwise caller selects a bitwise opcode"),
                    };
                    Ok(Value::BigInt(value))
                }
                _ => Err(RuntimeError::TypeError(
                    "cannot mix BigInt and other types in a bitwise operation".into(),
                )),
            }
        })
    }

    fn shift(&mut self, operation: Opcode) -> Result<(), RuntimeError> {
        self.binary(|vm, left, right| {
            let left = vm.coerce_numeric(&left)?;
            let right = vm.coerce_numeric(&right)?;
            match (left, right) {
                (primitive::Numeric::Number(left), primitive::Numeric::Number(right)) => {
                    let left = primitive::to_int32(left);
                    let right = primitive::to_uint32(right) & 0x1f;
                    let value = match operation {
                        Opcode::ShiftLeft => left.wrapping_shl(right) as f64,
                        Opcode::ShiftRight => (left >> right) as f64,
                        Opcode::UnsignedShiftRight => ((left as u32) >> right) as f64,
                        _ => unreachable!("shift caller selects a shift opcode"),
                    };
                    Ok(Value::Number(value))
                }
                (primitive::Numeric::BigInt(left), primitive::Numeric::BigInt(right)) => {
                    if operation == Opcode::UnsignedShiftRight {
                        return Err(RuntimeError::TypeError(
                            "BigInt does not support unsigned right shift".into(),
                        ));
                    }
                    Ok(Value::BigInt(bigint_shift(
                        left,
                        right,
                        operation == Opcode::ShiftLeft,
                    )?))
                }
                _ => Err(RuntimeError::TypeError(
                    "cannot mix BigInt and other types in a shift operation".into(),
                )),
            }
        })
    }

    fn relational(&mut self, accept: fn(Ordering) -> bool) -> Result<(), RuntimeError> {
        self.binary(|vm, a, b| {
            let a = vm.coerce_primitive(&a, "number")?;
            let b = vm.coerce_primitive(&b, "number")?;
            Ok(Value::Bool(primitive::compare(&a, &b)?.is_some_and(accept)))
        })
    }

    fn loose_equal(&mut self, left: Value, right: Value) -> Result<bool, RuntimeError> {
        if std::mem::discriminant(&left) == std::mem::discriminant(&right) {
            return Ok(left == right);
        }
        if matches!(right, Value::Null | Value::Undefined)
            && matches!(&left, Value::Object(object) if self.heap.is_html_dda(*object)?)
        {
            return Ok(true);
        }
        if matches!(left, Value::Null | Value::Undefined)
            && matches!(&right, Value::Object(object) if self.heap.is_html_dda(*object)?)
        {
            return Ok(true);
        }
        if matches!(
            (&left, &right),
            (Value::Null, Value::Undefined) | (Value::Undefined, Value::Null)
        ) {
            return Ok(true);
        }
        match (left, right) {
            (Value::Number(left), Value::String(right)) => {
                Ok(left == primitive::number(&Value::String(right))?)
            }
            (Value::String(left), Value::Number(right)) => {
                Ok(primitive::number(&Value::String(left))? == right)
            }
            (Value::Bool(left), right) => {
                self.loose_equal(Value::Number(if left { 1.0 } else { 0.0 }), right)
            }
            (left, Value::Bool(right)) => {
                self.loose_equal(left, Value::Number(if right { 1.0 } else { 0.0 }))
            }
            (
                Value::Object(left),
                right @ (Value::Number(_) | Value::String(_) | Value::Symbol(_)),
            ) => {
                let left = self.coerce_primitive(&Value::Object(left), "default")?;
                self.loose_equal(left, right)
            }
            (
                left @ (Value::Number(_) | Value::String(_) | Value::Symbol(_)),
                Value::Object(right),
            ) => {
                let right = self.coerce_primitive(&Value::Object(right), "default")?;
                self.loose_equal(left, right)
            }
            _ => Ok(false),
        }
    }

    fn property_in(&mut self, key: &Value, object: &Value) -> Result<bool, RuntimeError> {
        let Value::Object(mut object) = object else {
            return Err(RuntimeError::TypeError(
                "right operand of in must be an object".into(),
            ));
        };
        let key = self.coerce_property_key(key)?;
        loop {
            if self
                .heap
                .get_own_property_descriptor(object, &key)?
                .is_some()
            {
                return Ok(true);
            }
            let Some(prototype) = self.heap.prototype(object)? else {
                return Ok(false);
            };
            object = prototype;
        }
    }

    fn with_get(&mut self, name: &str) -> Result<Value, RuntimeError> {
        let key = Value::String(name.into());
        for object in self.with_objects.clone().into_iter().rev() {
            if self.property_in(&key, &object)? {
                return self.get_property(&object, &name.into());
            }
        }
        self.lookup_global_name(name)?
            .ok_or_else(|| RuntimeError::ReferenceError(name.into()))
    }

    fn with_set(&mut self, name: &str, value: Value) -> Result<(), RuntimeError> {
        let key = Value::String(name.into());
        for object in self.with_objects.clone().into_iter().rev() {
            if self.property_in(&key, &object)? {
                return self.set_property(&object, &name.into(), &value);
            }
        }
        Err(RuntimeError::ReferenceError(name.into()))
    }

    fn add(&mut self, left: Value, right: Value) -> Result<Value, RuntimeError> {
        let left = self.coerce_primitive(&left, "default")?;
        let right = self.coerce_primitive(&right, "default")?;
        if matches!(left, Value::String(_)) || matches!(right, Value::String(_)) {
            let mut a = primitive::string(&left)?;
            let b = primitive::string(&right)?;
            if a.byte_len()
                .checked_add(b.byte_len())
                .is_none_or(|len| len > self.config.max_string_bytes)
            {
                return Err(RuntimeError::StringLimit {
                    limit: self.config.max_string_bytes,
                });
            }
            a.push_str(&b);
            Ok(Value::String(a))
        } else {
            Ok(Value::Number(
                primitive::number(&left)? + primitive::number(&right)?,
            ))
        }
    }
}

/// BigInt shifts use the full signed right operand, unlike Number shifts
/// whose count is reduced modulo 32. A negative count reverses direction.
fn bigint_shift(value: BigInt, count: BigInt, left: bool) -> Result<BigInt, RuntimeError> {
    let reverse = count.sign() == Sign::Minus;
    let shift_left = left != reverse;
    let magnitude = count.magnitude().to_usize();
    let Some(magnitude) = magnitude else {
        if !shift_left {
            return Ok(if value.sign() == Sign::Minus {
                BigInt::from(-1)
            } else {
                BigInt::from(0)
            });
        }
        return Err(RuntimeError::RangeError(
            "BigInt shift count exceeds implementation capacity".into(),
        ));
    };
    Ok(if shift_left {
        value << magnitude
    } else {
        value >> magnitude
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpreter_converts_catchable_errors_and_rejects_a_top_level_yield() {
        let mut vm = Vm::default();
        vm.install_test262_harness().unwrap();
        vm.remaining_instructions = vm.config.instruction_budget;
        let test262_error = vm.error_value(RuntimeError::Test262("failure".into()));
        assert!(
            matches!(test262_error, Ok(Value::Object(_))),
            "{test262_error:?}"
        );
        assert_eq!(
            vm.error_value(RuntimeError::InstructionLimit),
            Err(RuntimeError::InstructionLimit)
        );
        let mut code = Bytecode::empty();
        code.constants.push(Value::Undefined);
        code.code
            .extend([Opcode::Constant as u8, 0, 0, 0, 0, Opcode::Yield as u8]);
        vm.remaining_instructions = vm.config.instruction_budget;
        let yield_error = vm.execute(&code);
        assert!(
            matches!(yield_error, Err(RuntimeError::TypeError(ref message)) if message == "yield is not supported in this execution context"),
            "{yield_error:?}"
        );
        code.generator = true;
        vm.remaining_instructions = vm.config.instruction_budget;
        assert!(
            matches!(vm.execute(&code), Err(RuntimeError::TypeError(message)) if message == "yield requires a generator function")
        );
    }

    #[test]
    fn with_lookup_uses_the_object_then_reports_an_unbound_name() {
        let mut vm = Vm::default();
        let object = vm.heap.alloc_object(None).unwrap();
        vm.heap.set(object, "value", Value::Number(7.0)).unwrap();
        vm.with_objects.push(Value::Object(object));
        assert_eq!(vm.with_get("value"), Ok(Value::Number(7.0)));
        assert_eq!(
            vm.with_get("missing"),
            Err(RuntimeError::ReferenceError("missing".into()))
        );
    }

    #[test]
    fn class_definition_opcodes_assign_home_objects_to_closures() {
        let mut vm = Vm::default();
        let target = vm.heap.alloc_object(None).unwrap();
        let mut function_code = Bytecode::empty();
        function_code.code.push(Opcode::Halt as u8);
        let function = vm
            .heap
            .alloc_closure(
                std::rc::Rc::new(function_code),
                Vec::new(),
                Value::Undefined,
                vm.object_prototype,
            )
            .unwrap();
        let mut code = Bytecode::empty();
        code.code
            .extend([Opcode::DefineMethod as u8, Opcode::Halt as u8]);

        vm.stack = vec![
            Value::Object(target),
            Value::String("method".into()),
            Value::Object(function),
        ];
        vm.remaining_instructions = vm.config.instruction_budget;
        assert!(matches!(
            vm.interpret(&code, &mut Vec::new(), 0, None),
            Ok(InterpreterExit::Return(Value::Undefined))
        ));
        vm.stack = vec![
            Value::Object(target),
            Value::String("empty".into()),
            Value::Undefined,
        ];
        vm.remaining_instructions = vm.config.instruction_budget;
        assert!(matches!(
            vm.interpret(&code, &mut Vec::new(), 0, None),
            Ok(InterpreterExit::Return(Value::Undefined))
        ));

        code.code[0] = Opcode::CallClassStaticBlock as u8;
        vm.stack = vec![Value::Object(target), Value::Object(function)];
        vm.remaining_instructions = vm.config.instruction_budget;
        assert!(matches!(
            vm.interpret(&code, &mut Vec::new(), 0, None),
            Ok(InterpreterExit::Return(Value::Undefined))
        ));
        vm.stack = vec![Value::Object(target), Value::Undefined];
        vm.remaining_instructions = vm.config.instruction_budget;
        assert!(matches!(
            vm.interpret(&code, &mut Vec::new(), 0, None),
            Err(RuntimeError::TypeError(_))
        ));

        code.code[0] = Opcode::DefineClassStaticField as u8;
        vm.stack = vec![
            Value::Undefined,
            Value::Object(target),
            Value::String("field".into()),
            Value::Object(function),
        ];
        vm.remaining_instructions = vm.config.instruction_budget;
        assert!(matches!(
            vm.interpret(&code, &mut Vec::new(), 0, None),
            Ok(InterpreterExit::Return(Value::Undefined))
        ));
        vm.stack = vec![
            Value::Undefined,
            Value::Object(target),
            Value::String("emptyField".into()),
            Value::Undefined,
        ];
        vm.remaining_instructions = vm.config.instruction_budget;
        assert!(matches!(
            vm.interpret(&code, &mut Vec::new(), 0, None),
            Err(RuntimeError::TypeError(_))
        ));
    }

    #[test]
    fn super_assignment_reports_a_non_extensible_receiver() {
        let mut vm = Vm::default();
        let base = vm.heap.alloc_object(None).unwrap();
        let home = vm.heap.alloc_object(Some(base)).unwrap();
        let receiver = vm.heap.alloc_object(None).unwrap();
        vm.heap.prevent_extensions(receiver).unwrap();
        vm.home_object = Some(home);
        vm.this = Value::Object(receiver);
        assert_eq!(
            vm.super_set(&"value".into(), &Value::Number(1.0)),
            Err(RuntimeError::TypeError(
                "super property cannot be assigned".into()
            ))
        );

        let base = vm.heap.alloc_object(None).unwrap();
        let home = vm.heap.alloc_object(Some(base)).unwrap();
        vm.heap.root(home).unwrap();
        let stale_receiver = vm.heap.alloc_object(None).unwrap();
        vm.heap.collect_major();
        vm.home_object = Some(home);
        vm.this = Value::Object(stale_receiver);
        assert_eq!(
            vm.super_set(&"value".into(), &Value::Number(1.0)),
            Err(RuntimeError::Heap(HeapError::InvalidObject(stale_receiver)))
        );
    }

    #[test]
    fn super_and_eval_context_errors_describe_missing_internal_context() {
        let mut vm = Vm::default();
        assert_eq!(
            vm.super_base(),
            Err(RuntimeError::TypeError(
                "super is not available in this function".into()
            ))
        );
        let home = vm.heap.alloc_object(None).unwrap();
        vm.home_object = Some(home);
        assert_eq!(
            vm.super_base(),
            Err(RuntimeError::TypeError("superclass is null".into()))
        );
        assert_eq!(
            vm.super_call(Vec::new()),
            Err(RuntimeError::TypeError(
                "super() is not available in this function".into()
            ))
        );
        let closure = vm
            .heap
            .alloc_closure(
                std::rc::Rc::new(Bytecode::empty()),
                Vec::new(),
                Value::Undefined,
                vm.object_prototype,
            )
            .unwrap();
        vm.class_constructor = Some(closure);
        assert_eq!(
            vm.super_call(Vec::new()),
            Err(RuntimeError::TypeError(
                "super() requires a derived constructor".into()
            ))
        );
        vm.heap.set_class_base(closure, Value::Null).unwrap();
        assert_eq!(
            vm.super_call(Vec::new()),
            Err(RuntimeError::TypeError("super constructor is null".into()))
        );
        vm.binding_metadata.push(Binding {
            name: "captured".into(),
            mutable: true,
            lexical: true,
        });
        vm.cells.insert(0, home);
        assert_eq!(
            vm.eval_visible_bindings()
                .into_iter()
                .map(|(name, _, slot)| (name, slot))
                .collect::<Vec<_>>(),
            vec![("captured".into(), 0)]
        );
    }

    #[test]
    fn compiler_owned_bytecode_invariants_fail_loudly() {
        let no_handler = Bytecode::empty();
        let mut vm = Vm::default();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| vm.resolve_completion(
                &no_handler,
                &mut Vec::new(),
                &mut Vec::new(),
                Completion::Yield(Value::Undefined)
            )))
            .is_err()
        );
        let mut vm = Vm::default();
        vm.stack.push(Value::Number(0.0));
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| vm.set_class_heritage()))
                .is_err()
        );
        let mut vm = Vm::default();
        vm.stack.push(Value::Number(0.0));
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| vm.set_class_home())).is_err()
        );
    }
}
