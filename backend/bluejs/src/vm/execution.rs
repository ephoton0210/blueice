// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Script execution, realm declarations, scope frames, and direct `eval`.

use super::*;

impl Vm {
    pub(super) fn execute_with_global_bindings(
        &mut self,
        code: &Bytecode,
        publish_globals: bool,
        module: bool,
    ) -> Result<Value, RuntimeError> {
        if let Some(root) = self.result_root.take() {
            self.heap.unroot(root)?;
        }
        self.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })?;
        self.bindings.resize(code.bindings.len(), None);
        self.binding_metadata = code.bindings.clone();
        self.remaining_instructions = self.config.instruction_budget;
        self.strict = code.strict;
        self.top_level_module = module;
        self.completion_empty = true;
        self.active_scopes.clear();
        self.active_scope_slots.clear();
        self.with_objects.clear();
        self.script_global_slots.clear();
        self.dynamic_eval_bindings.clear();
        self.eval_dynamic_slots.clear();
        self.dynamic_eval_outer_bindings.clear();
        if publish_globals {
            self.prepare_global_declarations(code)?;
        }
        // `this` lazily materializes the realm global only when script code
        // actually observes it. This keeps data-only executions within small
        // heap configurations while preserving script and arrow semantics.
        self.this = Value::Undefined;
        self.class_field_initializer_depth = 0;
        let result = self.run(code).and_then(|value| {
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
        self.dynamic_eval_bindings.clear();
        self.eval_dynamic_slots.clear();
        self.dynamic_eval_outer_bindings.clear();
        self.script_global_slots.clear();
        self.completion = Value::Undefined;
        self.completion_empty = true;
        self.active_scopes.clear();
        self.active_scope_slots.clear();
        self.with_objects.clear();
        self.top_level_module = false;
        self.pending_completions.clear();
        self.completion_saves.clear();
        self.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })?;
        result
    }

    /// GlobalDeclarationInstantiation for this VM's implemented classic
    /// script subset. Validation happens before execution, while bindings are
    /// created before any initializer so an abrupt script still leaves the
    /// required persistent TDZ state in its realm.
    pub(super) fn prepare_global_declarations(
        &mut self,
        code: &Bytecode,
    ) -> Result<(), RuntimeError> {
        let slots = code.scopes.first().cloned().unwrap_or_default();
        let global = self
            .global("globalThis")?
            .object_id()
            .expect("globalThis is an object");

        for &slot in &slots {
            let binding = &code.bindings[slot as usize];
            if binding.lexical {
                self.materialize_lexical_global(global, &binding.name)?;
            }
        }

        for &slot in &slots {
            let binding = &code.bindings[slot as usize];
            let existing = self.global_bindings.get(&binding.name);
            if binding.lexical {
                if existing.is_some_and(|binding| !binding.property)
                    || self
                        .heap
                        .get_own_property_descriptor(global, binding.name.as_str())?
                        .is_some_and(|descriptor| descriptor.configurable == Some(false))
                {
                    return Err(RuntimeError::SyntaxError(format!(
                        "global binding {} cannot be redeclared",
                        binding.name
                    )));
                }
            } else if existing.is_some_and(|binding| !binding.property) {
                return Err(RuntimeError::SyntaxError(format!(
                    "global lexical binding {} conflicts with var declaration",
                    binding.name
                )));
            } else if code.global_function_names.contains(&binding.name) {
                if !self.can_declare_global_function(global, &binding.name)? {
                    return Err(RuntimeError::TypeError(format!(
                        "cannot declare global function {}",
                        binding.name
                    )));
                }
            } else if !self.can_declare_global_var(global, &binding.name)? {
                return Err(RuntimeError::TypeError(format!(
                    "cannot declare global var {}",
                    binding.name
                )));
            }
        }

        for &slot in &slots {
            let binding = &code.bindings[slot as usize];
            if !self.global_bindings.contains_key(&binding.name) {
                self.create_global_binding(
                    global,
                    binding,
                    code.global_function_names.contains(&binding.name),
                    false,
                )?;
            }
            self.script_global_slots
                .insert(slot as usize, binding.name.clone());
        }
        Ok(())
    }

    /// Standard global properties exist independently of a script lexical
    /// declaration that shadows them. Most intrinsics are otherwise lazy, so
    /// materialize only the property that a global lexical declaration needs
    /// to inspect or shadow.
    pub(super) fn materialize_lexical_global(
        &mut self,
        global: ObjectId,
        name: &str,
    ) -> Result<(), RuntimeError> {
        let constant = match name {
            "undefined" => Some(Value::Undefined),
            "NaN" => Some(Value::Number(f64::NAN)),
            "Infinity" => Some(Value::Number(f64::INFINITY)),
            _ => None,
        };
        if let Some(value) = constant {
            if self
                .heap
                .get_own_property_descriptor(global, name)?
                .is_none()
            {
                self.define_data(global, name, value, false, false, false)?;
            }
            return Ok(());
        }
        if matches!(
            name,
            "String"
                | "Symbol"
                | "RegExp"
                | "Object"
                | "Reflect"
                | "Math"
                | "Number"
                | "Boolean"
                | "BigInt"
                | "Atomics"
                | "Array"
                | "ArrayBuffer"
                | "SharedArrayBuffer"
                | "DataView"
                | "Int8Array"
                | "Uint8Array"
                | "Uint8ClampedArray"
                | "Int16Array"
                | "Uint16Array"
                | "Int32Array"
                | "Uint32Array"
                | "Float32Array"
                | "Float64Array"
                | "BigInt64Array"
                | "BigUint64Array"
                | "Map"
                | "Set"
                | "Function"
                | "Proxy"
                | "Promise"
                | "Intl"
                | "Error"
                | "TypeError"
                | "RangeError"
                | "SyntaxError"
                | "ReferenceError"
                | "EvalError"
                | "URIError"
                | "eval"
                | "isNaN"
                | "isFinite"
                | "parseInt"
                | "parseFloat"
                | "encodeURI"
                | "encodeURIComponent"
                | "decodeURI"
                | "decodeURIComponent"
                | "JSON"
        ) {
            self.global(name)?;
        }
        Ok(())
    }

    /// The realm global's intrinsic properties are created lazily. Property
    /// access must still observe their specified descriptors, even when the
    /// name did not first occur as an unqualified identifier.
    pub(super) fn materialize_global_object_property(
        &mut self,
        object: ObjectId,
        key: &PropertyName,
    ) -> Result<(), RuntimeError> {
        if self.globals.get("globalThis") != Some(&object) {
            return Ok(());
        }
        let PropertyName::String(name) = key else {
            return Ok(());
        };
        let Ok(name) = name.to_utf8() else {
            return Ok(());
        };
        self.materialize_lexical_global(object, &name)
    }

    pub(super) fn can_declare_global_var(
        &mut self,
        global: ObjectId,
        name: &str,
    ) -> Result<bool, RuntimeError> {
        self.materialize_lexical_global(global, name)?;
        Ok(self
            .heap
            .get_own_property_descriptor(global, name)?
            .is_some()
            || self.heap.is_extensible(global)?)
    }

    pub(super) fn can_declare_global_function(
        &mut self,
        global: ObjectId,
        name: &str,
    ) -> Result<bool, RuntimeError> {
        self.materialize_lexical_global(global, name)?;
        let Some(descriptor) = self.heap.get_own_property_descriptor(global, name)? else {
            return Ok(self.heap.is_extensible(global)?);
        };
        Ok(descriptor.configurable == Some(true)
            || (descriptor.value.is_some()
                && descriptor.writable == Some(true)
                && descriptor.enumerable == Some(true)))
    }

    pub(super) fn create_global_binding(
        &mut self,
        global: ObjectId,
        binding: &Binding,
        function: bool,
        configurable: bool,
    ) -> Result<(), RuntimeError> {
        let property = !binding.lexical;
        let descriptor = self
            .heap
            .get_own_property_descriptor(global, binding.name.as_str())?;
        let initial = if property && !function {
            descriptor
                .as_ref()
                .and_then(|descriptor| descriptor.value.clone())
                .unwrap_or(Value::Undefined)
        } else {
            Value::Undefined
        };
        let cell = self.with_roots(|heap| heap.alloc_object(None))?;
        let root = self.heap.root(cell)?;
        let result = (|| {
            if property {
                if function
                    && descriptor
                        .as_ref()
                        .is_some_and(|descriptor| descriptor.configurable == Some(true))
                    || descriptor.is_none()
                {
                    let defined = self.with_roots(|heap| {
                        heap.define_own_property(
                            global,
                            binding.name.as_str(),
                            PropertyDescriptor::data(Value::Undefined, true, true, configurable),
                        )
                    })?;
                    if !defined {
                        return Err(RuntimeError::TypeError(
                            "cannot create global binding".into(),
                        ));
                    }
                } else if function {
                    self.with_roots(|heap| {
                        heap.set(global, binding.name.as_str(), Value::Undefined)
                    })?;
                }
            }
            if property {
                self.with_roots(|heap| heap.set(cell, "value", initial.clone()))?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.heap.unroot(root)?;
            return Err(error);
        }
        self.global_bindings.insert(
            binding.name.clone(),
            GlobalBinding {
                cell,
                mutable: binding.mutable,
                strict_immutable: binding.strict_immutable,
                property,
                _root: root,
            },
        );
        Ok(())
    }

    pub(super) fn global_binding_value(&self, name: &str) -> Result<Option<Value>, RuntimeError> {
        let Some(binding) = self.global_bindings.get(name) else {
            return Ok(None);
        };
        self.heap.get_own(binding.cell, "value").map_err(Into::into)
    }

    pub(super) fn set_global_binding(
        &mut self,
        name: &str,
        value: Value,
    ) -> Result<bool, RuntimeError> {
        let Some(binding) = self.global_bindings.get(name) else {
            return Ok(false);
        };
        let cell = binding.cell;
        let mutable = binding.mutable;
        let strict_immutable = binding.strict_immutable;
        if self.heap.get_own(cell, "value")?.is_none() {
            return Err(RuntimeError::ReferenceError(name.into()));
        }
        if !mutable && strict_immutable {
            return Err(RuntimeError::TypeError(format!(
                "assignment to constant {name}"
            )));
        }
        if !mutable {
            return Ok(true);
        }
        self.store_global_cell(cell, value)?;
        Ok(true)
    }

    pub(super) fn dynamic_eval_binding_value(
        &self,
        name: &str,
    ) -> Result<Option<Value>, RuntimeError> {
        let binding = self.dynamic_eval_bindings.get(name).or_else(|| {
            self.dynamic_eval_outer_bindings
                .iter()
                .rev()
                .find_map(|bindings| bindings.get(name))
        });
        match binding {
            Some(binding) => self.heap.get_own(binding.cell, "value").map_err(Into::into),
            None => Ok(None),
        }
    }

    /// Returns the dynamic eval cell that shadows this exact statically
    /// resolved cell. A name match by itself is insufficient: Annex B block
    /// functions in the eval can share the name of the eval's var binding
    /// without being that binding.
    pub(super) fn dynamic_eval_shadowing_cell(
        &self,
        name: &str,
        cell: ObjectId,
    ) -> Option<ObjectId> {
        self.dynamic_eval_bindings
            .get(name)
            .into_iter()
            .chain(
                self.dynamic_eval_outer_bindings
                    .iter()
                    .rev()
                    .filter_map(|bindings| bindings.get(name)),
            )
            .find(|binding| binding.shadowed_cells.contains(&cell))
            .map(|binding| binding.cell)
    }

    pub(super) fn store_dynamic_eval_shadowing_binding(
        &mut self,
        slot: usize,
        name: &str,
        value: Value,
    ) -> Result<bool, RuntimeError> {
        let Some(&cell) = self.cells.get(&slot) else {
            return Ok(false);
        };
        let Some(shadowing) = self.dynamic_eval_shadowing_cell(name, cell) else {
            return Ok(false);
        };
        self.store_global_cell(shadowing, value)?;
        Ok(true)
    }

    pub(super) fn set_dynamic_eval_binding(
        &mut self,
        name: &str,
        value: Value,
    ) -> Result<bool, RuntimeError> {
        let binding = self.dynamic_eval_bindings.get(name).or_else(|| {
            self.dynamic_eval_outer_bindings
                .iter()
                .rev()
                .find_map(|bindings| bindings.get(name))
        });
        let Some(binding) = binding else {
            return Ok(false);
        };
        self.store_global_cell(binding.cell, value)?;
        Ok(true)
    }

    pub(super) fn delete_dynamic_eval_binding(&mut self, name: &str) -> Result<bool, RuntimeError> {
        let binding = self.dynamic_eval_bindings.remove(name).or_else(|| {
            self.dynamic_eval_outer_bindings
                .iter_mut()
                .rev()
                .find_map(|bindings| bindings.remove(name))
        });
        let Some(binding) = binding else {
            return Ok(true);
        };
        self.heap.delete(binding.cell, "value").map_err(Into::into)
    }

    pub(super) fn store_global_cell(
        &mut self,
        cell: ObjectId,
        value: Value,
    ) -> Result<(), RuntimeError> {
        self.with_roots(|heap| heap.set(cell, "value", value.clone()))?;
        let property = self.global_bindings.iter().find_map(|(name, binding)| {
            (binding.cell == cell && binding.property).then(|| name.clone())
        });
        if let Some(name) = property {
            let global = self
                .global("globalThis")?
                .object_id()
                .expect("globalThis is an object");
            self.with_roots(|heap| heap.set(global, name, value))?;
        }
        Ok(())
    }

    pub(super) fn global_property_cell(
        &self,
        object: ObjectId,
        key: &PropertyName,
    ) -> Option<ObjectId> {
        if self.globals.get("globalThis") != Some(&object) {
            return None;
        }
        let PropertyName::String(name) = key else {
            return None;
        };
        let name = name.to_utf8().ok()?;
        self.global_bindings
            .get(&name)
            .filter(|binding| binding.property)
            .map(|binding| binding.cell)
    }

    pub(super) fn pop(&mut self) -> Value {
        self.stack
            .pop()
            .expect("compiler balances the operand stack")
    }

    pub(super) fn reset_scope(&mut self, code: &Bytecode, scope: u32) {
        for slot in &code.scopes[scope as usize] {
            // The graph linker owns module outer-scope cells until every
            // dependent body has finished.  Do not discard them merely
            // because a suspended/resumed module leaves its lexical scope.
            if code.module && scope == 0 {
                self.bindings[*slot as usize] = None;
                continue;
            }
            self.cells.remove(&(*slot as usize));
            self.bindings[*slot as usize] = None;
        }
    }

    /// Creates the next per-iteration environment for a lexical `for`
    /// declaration.  The previous cells deliberately stay alive through any
    /// closures that captured them; this frame starts using fresh cells with
    /// the values left by the completed loop body, ready for its update
    /// expression.
    pub(super) fn clone_scope(&mut self, code: &Bytecode, scope: u32) -> Result<(), RuntimeError> {
        let slots = code.scopes[scope as usize].clone();
        for slot in slots {
            let slot = slot as usize;
            let value = self.binding_value(slot)?;
            let cell = self.with_roots(|heap| heap.alloc_object(None))?;
            // Insert before the allocation-backed store so the fresh cell is
            // an interpreter root if the store needs to collect.
            self.cells.insert(slot, cell);
            self.bindings[slot] = None;
            if let Some(value) = value {
                self.with_roots(|heap| heap.set(cell, "value", value))?;
            }
        }
        Ok(())
    }

    pub(super) fn leave_scope(&mut self, code: &Bytecode, scope: u32) {
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

    pub(super) fn unwind_scopes(&mut self, code: &Bytecode, depth: usize) {
        while self.active_scopes.len() > depth {
            let scope = self.active_scopes.pop().expect("scope length was checked");
            self.active_scope_slots.pop();
            self.reset_scope(code, scope);
        }
    }

    pub(super) fn unwind_with(&mut self, depth: usize) {
        self.with_objects.truncate(depth);
    }

    pub(super) fn close_iterators_to(&mut self, iterators: &mut Vec<Value>, depth: usize) {
        // A compiler-emitted `break` can close its target for-of iterator
        // before a surrounding handler starts finalizer cleanup.
        let active = iterators.split_off(depth.min(iterators.len()));
        for record in active.into_iter().rev() {
            // This is cleanup for an already-selected abrupt completion. The
            // original completion wins over a return() failure.
            let _ = self.iterator_close(&record);
        }
    }

    pub(super) fn error_value(&mut self, error: RuntimeError) -> Result<Value, RuntimeError> {
        match error {
            RuntimeError::Thrown(value) => Ok(value),
            RuntimeError::ReferenceError(message) => self.error_object("ReferenceError", message),
            RuntimeError::TypeError(message) => self.error_object("TypeError", message),
            RuntimeError::RangeError(message) => self.error_object("RangeError", message),
            RuntimeError::SyntaxError(message) => self.error_object("SyntaxError", message),
            // Dynamic import reports a linking/host-resolution failure by
            // rejecting its Promise with a SyntaxError; it must not escape
            // the job queue as an implementation error.
            RuntimeError::ModuleResolution(message) => self.error_object("SyntaxError", message),
            RuntimeError::Test262(message) => self.error_object("Test262Error", message),
            error => Err(error),
        }
    }

    pub(super) fn error_object(
        &mut self,
        name: &str,
        message: String,
    ) -> Result<Value, RuntimeError> {
        let constructor = self.error_global(name)?;
        self.call_native(
            constructor,
            Value::Undefined,
            vec![Value::String(message.into())],
            false,
        )
    }

    pub(super) fn restore_completion(&mut self) {
        let (value, empty) = self
            .completion_saves
            .pop()
            .expect("normal finally entry saves its preceding completion");
        self.completion = value;
        self.completion_empty = empty;
    }

    pub(super) fn resolve_completion(
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
                    // `value` is now a stack root owned by the catch entry.
                    // The temporary completion root protected the original
                    // throw while Error construction and scope cleanup could
                    // allocate; retaining it would keep every caught error
                    // alive until the enclosing script returns.
                    self.pending_completions.pop();
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

    pub(super) fn check_string(&self, value: &Value) -> Result<(), RuntimeError> {
        if matches!(value, Value::String(s) if s.byte_len() > self.config.max_string_bytes) {
            Err(RuntimeError::StringLimit {
                limit: self.config.max_string_bytes,
            })
        } else {
            Ok(())
        }
    }

    pub(super) fn with_roots<T>(
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
                .chain(std::iter::once(&self.callee))
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
            for binding in self.dynamic_eval_bindings.values() {
                roots.push(self.heap.root(binding.cell)?);
            }
            for bindings in &self.dynamic_eval_outer_bindings {
                for binding in bindings.values() {
                    roots.push(self.heap.root(binding.cell)?);
                }
            }
            // A continuation lives in a Rust map while it waits for a Promise
            // job.  Its frame has no ordinary heap owner, so root every edge
            // before any allocation is allowed to trigger collection.
            let continuation_references = self.continuation_references();
            for id in continuation_references {
                roots.push(self.heap.root(id)?);
            }
            for (&promise, record) in &self.promises {
                roots.push(self.heap.root(promise)?);
                if matches!(record.status, PromiseStatus::Pending) {
                    for reaction in &record.reactions {
                        if let PromiseReaction::AsyncGeneratorYield {
                            generator,
                            target,
                            result,
                        } = reaction
                        {
                            roots.push(self.heap.root(*generator)?);
                            roots.push(self.heap.root(*target)?);
                            roots.push(self.heap.root(*result)?);
                        }
                        if let PromiseReaction::AsyncGeneratorDelegate {
                            generator, target, ..
                        } = reaction
                        {
                            roots.push(self.heap.root(*generator)?);
                            roots.push(self.heap.root(*target)?);
                        }
                    }
                }
                let values: Vec<&Value> = match &record.status {
                    PromiseStatus::Pending => record
                        .reactions
                        .iter()
                        .filter_map(|reaction| match reaction {
                            PromiseReaction::Then(reaction) => {
                                Some([&reaction.on_fulfilled, &reaction.on_rejected])
                            }
                            PromiseReaction::ModuleAwait { .. }
                            | PromiseReaction::AsyncAwait { .. }
                            | PromiseReaction::AsyncGeneratorYield { .. }
                            | PromiseReaction::AsyncGeneratorDelegate { .. } => None,
                        })
                        .flatten()
                        .collect(),
                    PromiseStatus::Fulfilled(value) | PromiseStatus::Rejected(value) => {
                        vec![value]
                    }
                };
                for value in values {
                    if let Value::Object(id) = value {
                        roots.push(self.heap.root(*id)?);
                    }
                }
            }
            for state in self.promise_all.values() {
                for value in state.values.iter().flatten() {
                    if let Value::Object(id) = value {
                        roots.push(self.heap.root(*id)?);
                    }
                }
            }
            for job in &self.promise_jobs {
                match job {
                    PromiseJob::Reaction {
                        target,
                        handler,
                        value,
                        ..
                    } => {
                        roots.push(self.heap.root(*target)?);
                        for value in [handler, value] {
                            if let Value::Object(id) = value {
                                roots.push(self.heap.root(*id)?);
                            }
                        }
                    }
                    PromiseJob::Thenable {
                        target,
                        thenable,
                        then,
                    } => {
                        roots.push(self.heap.root(*target)?);
                        for value in [thenable, then] {
                            if let Value::Object(id) = value {
                                roots.push(self.heap.root(*id)?);
                            }
                        }
                    }
                    PromiseJob::DynamicImport { target, .. } => {
                        roots.push(self.heap.root(*target)?)
                    }
                    PromiseJob::ModuleAwait { value, .. }
                    | PromiseJob::AsyncAwait { value, .. } => {
                        if let Value::Object(id) = value {
                            roots.push(self.heap.root(*id)?);
                        }
                    }
                    PromiseJob::AsyncGeneratorYield {
                        generator,
                        target,
                        result,
                        value,
                        ..
                    } => {
                        roots.push(self.heap.root(*generator)?);
                        roots.push(self.heap.root(*target)?);
                        roots.push(self.heap.root(*result)?);
                        if let Value::Object(id) = value {
                            roots.push(self.heap.root(*id)?);
                        }
                    }
                    PromiseJob::AsyncGeneratorDelegate {
                        generator,
                        target,
                        value,
                        ..
                    } => {
                        roots.push(self.heap.root(*generator)?);
                        roots.push(self.heap.root(*target)?);
                        if let Value::Object(id) = value {
                            roots.push(self.heap.root(*id)?);
                        }
                    }
                }
            }
            if let Some(Err(Value::Object(id))) = &self.test262_done {
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

    pub(super) fn run(&mut self, code: &Bytecode) -> Result<Value, RuntimeError> {
        let pending_base = self.pending_completions.len();
        let save_base = self.completion_saves.len();
        let mut iterators = Vec::new();
        let result = self
            .interpret(code, &mut iterators, 0, None, None, None)
            .and_then(|exit| match exit {
                InterpreterExit::Return(value) => Ok(value),
                InterpreterExit::Yield { .. } => Err(RuntimeError::TypeError(
                    "yield requires a generator function".into(),
                )),
                InterpreterExit::Suspend { .. } => unreachable!("only generator entry suspends"),
                InterpreterExit::Await { .. } => {
                    unreachable!("only module evaluation can suspend at await")
                }
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

    pub(super) fn execute_eval(
        &mut self,
        code: &Bytecode,
        captures: Vec<ObjectId>,
        global_var_environment: bool,
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
        let script_global_slots = std::mem::take(&mut self.script_global_slots);
        let eval_dynamic_slots = std::mem::take(&mut self.eval_dynamic_slots);
        let completion = std::mem::replace(&mut self.completion, Value::Undefined);
        let completion_empty = std::mem::replace(&mut self.completion_empty, true);
        let active_scopes = std::mem::take(&mut self.active_scopes);
        let active_scope_slots = std::mem::take(&mut self.active_scope_slots);
        let strict = std::mem::replace(&mut self.strict, code.strict);
        let variable_scope = code
            .strict
            .then(|| std::mem::replace(&mut self.variable_scope, code.variable_scope));
        let result = if global_var_environment {
            self.prepare_eval_global_var_declarations(code)
                .and_then(|()| self.run(code))
        } else if !code.strict {
            self.prepare_eval_dynamic_var_declarations(code)
                .and_then(|()| self.run(code))
        } else {
            self.run(code)
        };
        self.bindings = bindings;
        self.binding_metadata = binding_metadata;
        self.cells = cells;
        self.script_global_slots = script_global_slots;
        self.eval_dynamic_slots = eval_dynamic_slots;
        self.completion = completion;
        self.completion_empty = completion_empty;
        self.active_scopes = active_scopes;
        self.active_scope_slots = active_scope_slots;
        self.strict = strict;
        if let Some(variable_scope) = variable_scope {
            self.variable_scope = variable_scope;
        }
        self.stack.truncate(base);
        result
    }

    /// EvalDeclarationInstantiation's ordinary-function branch. Sloppy
    /// direct eval extends the caller's VariableEnvironment, so fresh `var`
    /// and function cells survive eval and can be captured by closures.
    pub(super) fn prepare_eval_dynamic_var_declarations(
        &mut self,
        code: &Bytecode,
    ) -> Result<(), RuntimeError> {
        for &slot in &code.dynamic_eval_slots {
            let binding = &code.bindings[slot as usize];
            let shadowed_cells: Vec<_> = code
                .captures
                .iter()
                .enumerate()
                .filter_map(|(captured_slot, _)| {
                    (code.bindings[captured_slot].name == binding.name)
                        .then(|| self.cells.get(&captured_slot).copied())
                        .flatten()
                })
                .collect();
            if !self.dynamic_eval_bindings.contains_key(&binding.name) {
                let cell = self.with_roots(|heap| heap.alloc_object(None))?;
                self.dynamic_eval_bindings.insert(
                    binding.name.clone(),
                    DynamicEvalBinding {
                        cell,
                        shadowed_cells: shadowed_cells.clone(),
                    },
                );
                if let Err(error) =
                    self.with_roots(|heap| heap.set(cell, "value", Value::Undefined))
                {
                    self.dynamic_eval_bindings.remove(&binding.name);
                    return Err(error);
                }
            } else if let Some(dynamic) = self.dynamic_eval_bindings.get_mut(&binding.name) {
                for cell in shadowed_cells {
                    if !dynamic.shadowed_cells.contains(&cell) {
                        dynamic.shadowed_cells.push(cell);
                    }
                }
            }
            self.eval_dynamic_slots
                .insert(slot as usize, binding.name.clone());
        }
        Ok(())
    }

    /// EvalDeclarationInstantiation's global-variable branch. The eval
    /// lexical environment remains transient, so only `var` and top-level
    /// function bindings are published into the realm's global environment.
    pub(super) fn prepare_eval_global_var_declarations(
        &mut self,
        code: &Bytecode,
    ) -> Result<(), RuntimeError> {
        let slots = code.scopes.first().cloned().unwrap_or_default();
        let global = self
            .global("globalThis")?
            .object_id()
            .expect("globalThis is an object");

        for &slot in &slots {
            let binding = &code.bindings[slot as usize];
            if binding.lexical {
                continue;
            }
            let existing = self.global_bindings.get(&binding.name);
            if existing.is_some_and(|binding| !binding.property) {
                return Err(RuntimeError::SyntaxError(format!(
                    "global lexical binding {} conflicts with eval declaration",
                    binding.name
                )));
            }
            if code.global_function_names.contains(&binding.name) {
                if !self.can_declare_global_function(global, &binding.name)? {
                    return Err(RuntimeError::TypeError(format!(
                        "cannot declare global function {}",
                        binding.name
                    )));
                }
            } else if !self.can_declare_global_var(global, &binding.name)? {
                return Err(RuntimeError::TypeError(format!(
                    "cannot declare global var {}",
                    binding.name
                )));
            }
        }

        for &slot in &slots {
            let binding = &code.bindings[slot as usize];
            if binding.lexical {
                continue;
            }
            if !self.global_bindings.contains_key(&binding.name) {
                self.create_global_binding(
                    global,
                    binding,
                    code.global_function_names.contains(&binding.name),
                    true,
                )?;
            }
            self.script_global_slots
                .insert(slot as usize, binding.name.clone());
        }
        Ok(())
    }

    /// Evaluates a new classic script in the current realm while another
    /// script/function frame is active (the Test262 `$262.evalScript` host
    /// path). It deliberately gets fresh script bindings and global `this`,
    /// but preserves the caller frame and the remaining resource budget.
    pub(super) fn execute_nested_script(&mut self, code: &Bytecode) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        self.stack.extend(self.bindings.iter().flatten().cloned());
        self.stack
            .extend(self.cells.values().copied().map(Value::Object));
        self.stack.push(self.completion.clone());
        self.stack.push(self.this.clone());
        self.stack.extend(self.arguments.iter().cloned());
        self.stack.extend(self.with_objects.iter().cloned());
        let bindings = std::mem::replace(&mut self.bindings, vec![None; code.bindings.len()]);
        let binding_metadata = std::mem::replace(&mut self.binding_metadata, code.bindings.clone());
        let cells = std::mem::take(&mut self.cells);
        let completion = std::mem::replace(&mut self.completion, Value::Undefined);
        let completion_empty = std::mem::replace(&mut self.completion_empty, true);
        let active_scopes = std::mem::take(&mut self.active_scopes);
        let active_scope_slots = std::mem::take(&mut self.active_scope_slots);
        let with_objects = std::mem::take(&mut self.with_objects);
        let strict = std::mem::replace(&mut self.strict, code.strict);
        let top_level_module = std::mem::replace(&mut self.top_level_module, false);
        let global_this = self.global("globalThis")?;
        let this = std::mem::replace(&mut self.this, global_this);
        let arguments = std::mem::take(&mut self.arguments);
        let new_target = std::mem::replace(&mut self.new_target, Value::Undefined);
        let new_target_allowed = std::mem::replace(&mut self.new_target_allowed, false);
        let home_object = std::mem::take(&mut self.home_object);
        let class_constructor = std::mem::take(&mut self.class_constructor);
        let class_field_initializer_depth =
            std::mem::replace(&mut self.class_field_initializer_depth, 0);
        let script_global_slots = std::mem::take(&mut self.script_global_slots);
        let result = self
            .prepare_global_declarations(code)
            .and_then(|()| self.run(code));
        self.bindings = bindings;
        self.binding_metadata = binding_metadata;
        self.cells = cells;
        self.completion = completion;
        self.completion_empty = completion_empty;
        self.active_scopes = active_scopes;
        self.active_scope_slots = active_scope_slots;
        self.with_objects = with_objects;
        self.strict = strict;
        self.top_level_module = top_level_module;
        self.this = this;
        self.arguments = arguments;
        self.new_target = new_target;
        self.new_target_allowed = new_target_allowed;
        self.home_object = home_object;
        self.class_constructor = class_constructor;
        self.class_field_initializer_depth = class_field_initializer_depth;
        self.script_global_slots = script_global_slots;
        self.stack.truncate(base);
        result
    }

    pub(super) fn eval_visible_bindings(&self) -> Vec<(String, Binding, u32)> {
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
        // A named function expression's immutable name environment is not a
        // block scope, so it is neither in `active_scope_slots` nor captured
        // until a nested closure needs it. Direct eval nevertheless sees that
        // live binding and must retain its strict/sloppy write behavior.
        for (slot, value) in self.bindings.iter().enumerate() {
            if value.is_some() {
                visible
                    .entry(self.binding_metadata[slot].name.clone())
                    .or_insert_with(|| (self.binding_metadata[slot].clone(), slot as u32));
            }
        }
        visible
            .into_iter()
            .map(|(name, (binding, slot))| (name, binding, slot))
            .collect()
    }

    /// The variable environment is narrower than the set of lexically
    /// visible cells. In particular, an inner function may capture `x` from
    /// an outer function while a sloppy direct eval still has to create its
    /// own `var x` in the inner VariableEnvironment.
    pub(super) fn eval_variable_environment_names(&self) -> Vec<String> {
        let mut names = std::collections::BTreeSet::new();
        if let Some(position) = self
            .active_scopes
            .iter()
            .position(|scope| *scope == self.variable_scope)
        {
            names.extend(self.active_scope_slots[position].iter().filter_map(|slot| {
                let binding = &self.binding_metadata[*slot as usize];
                (!binding.lexical).then(|| binding.name.clone())
            }));
        }
        names.extend(self.dynamic_eval_bindings.keys().cloned());
        names.into_iter().collect()
    }

    /// A sloppy direct eval declaration can shadow a static captured binding
    /// from an outer function. The dynamic binding records the exact cell it
    /// masks, so an eval-local block binding with the same name stays visible.
    pub(super) fn eval_aware_binding_value(
        &mut self,
        slot: usize,
        name: &str,
    ) -> Result<Option<Value>, RuntimeError> {
        if let Some(&cell) = self.cells.get(&slot) {
            if let Some(shadowing) = self.dynamic_eval_shadowing_cell(name, cell) {
                return self.heap.get_own(shadowing, "value").map_err(Into::into);
            }
        }
        self.binding_value(slot)
    }

    /// Lexical names between a direct eval site and the active function's
    /// VariableEnvironment. Unlike captured outer bindings, these prevent a
    /// sloppy eval `var` declaration from being instantiated.
    pub(super) fn eval_lexical_conflicts(&self) -> Vec<String> {
        let variable_scope_position = self
            .active_scopes
            .iter()
            .position(|scope| *scope == self.variable_scope);
        let start = variable_scope_position.map_or(0, |index| index + 1);
        let mut conflicts = self.active_scope_slots[start..]
            .iter()
            .flat_map(|slots| slots.iter().copied())
            .filter_map(|slot| {
                let binding = &self.binding_metadata[slot as usize];
                (binding.lexical && !binding.catch_parameter).then(|| binding.name.clone())
            })
            .collect::<std::collections::BTreeSet<_>>();
        // A non-arrow function's parameter expressions retain the separate
        // body VariableEnvironment boundary. Its body lexical declarations
        // therefore block a sloppy direct-eval var declaration even before
        // the body scope is entered. Arrow parameters inherit their outer
        // VariableEnvironment instead, so their not-yet-entered body lexical
        // declarations must not be treated as a conflict.
        let ordinary_function = self
            .callee
            .object_id()
            .and_then(|callee| self.heap.closure(callee).ok().flatten())
            .is_some_and(|(code, _, _, _, _)| !code.arrow);
        if variable_scope_position.is_none() && ordinary_function {
            conflicts.extend(self.variable_scope_lexicals.iter().cloned());
        }
        conflicts.into_iter().collect()
    }
}
