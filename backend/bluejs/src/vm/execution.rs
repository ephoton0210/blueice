// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Script execution, realm declarations, scope frames, and direct `eval`.

use super::*;

/// The synchronous root entry cannot suspend at a generator or await boundary.
pub(super) fn root_interpreter_exit(exit: InterpreterExit) -> Result<Value, RuntimeError> {
    match exit {
        InterpreterExit::Return(value) => Ok(value),
        InterpreterExit::Yield { .. } => Err(RuntimeError::TypeError(
            "yield requires a generator function".into(),
        )),
        InterpreterExit::Suspend { .. } => unreachable!("only generator entry suspends"),
        InterpreterExit::Await { .. } => {
            unreachable!("only module evaluation can suspend at await")
        }
    }
}

/// The standard global properties of the realm global object that are created
/// on first use rather than eagerly.
pub(super) const LAZY_STANDARD_GLOBALS: &[&str] = &[
    "String",
    "Symbol",
    "RegExp",
    "Object",
    "Reflect",
    "Math",
    "Number",
    "Boolean",
    "BigInt",
    "Atomics",
    "Array",
    "ArrayBuffer",
    "SharedArrayBuffer",
    "DataView",
    "Int8Array",
    "Uint8Array",
    "Uint8ClampedArray",
    "Int16Array",
    "Uint16Array",
    "Int32Array",
    "Uint32Array",
    "Float16Array",
    "Float32Array",
    "Float64Array",
    "BigInt64Array",
    "BigUint64Array",
    "Map",
    "Set",
    "WeakMap",
    "WeakSet",
    "WeakRef",
    "FinalizationRegistry",
    "DisposableStack",
    "AsyncDisposableStack",
    "ShadowRealm",
    "Iterator",
    "Function",
    "Proxy",
    "Promise",
    "Intl",
    "Temporal",
    "Error",
    "TypeError",
    "RangeError",
    "SyntaxError",
    "ReferenceError",
    "EvalError",
    "URIError",
    "AggregateError",
    "SuppressedError",
    "eval",
    "isNaN",
    "isFinite",
    "parseInt",
    "parseFloat",
    "encodeURI",
    "encodeURIComponent",
    "decodeURI",
    "decodeURIComponent",
    "escape",
    "unescape",
    "JSON",
];

impl Vm {
    pub(super) fn execute_with_global_bindings(
        &mut self,
        code: &Bytecode,
        publish_globals: bool,
        module: bool,
    ) -> Result<Value, RuntimeError> {
        self.prepare_root_execution(code, module)?;
        let result = if publish_globals {
            self.prepare_global_declarations(code)
                .and_then(|()| self.run(code))
        } else {
            self.run(code)
        };
        self.finish_root_execution(result)
    }

    /// Initializes an outer script/module execution context. The native
    /// debugger continuation owns this exact context while paused, so another
    /// public execution entry must fail closed rather than replacing its
    /// bindings, operand stack, or remaining instruction budget.
    pub(super) fn prepare_root_execution(
        &mut self,
        code: &Bytecode,
        module: bool,
    ) -> Result<(), RuntimeError> {
        self.ensure_no_debugger_continuation()?;
        self.pending_throw_site = None;
        self.uncaught_throw_site = None;
        self.pending_diagnostic_origin = None;
        self.uncaught_diagnostic_origin = None;
        self.throw_epoch = 0;
        if let Some(root) = self.result_root.take() {
            self.release_root(root);
        }
        // ClearKeptObjects runs at the end of the preceding ECMAScript job.
        // The next job's initial collection must not retain its WeakRef
        // targets merely because a prior job touched them.
        self.kept_weak_objects.clear();
        self.collect_realm_garbage()?;
        self.enqueue_finalization_cleanup_jobs()?;
        let mut prepared = self.prepare_frame_bindings(code, FrameBindingKind::Script)?;
        let (bindings, metadata, _) = prepared.take_parts();
        self.bindings = bindings;
        self.binding_metadata = metadata;
        drop(prepared);
        self.synchronize_scheduling_usage()?;
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
        // `this` lazily materializes the realm global only when script code
        // actually observes it. This keeps data-only executions within small
        // heap configurations while preserving script and arrow semantics.
        self.this = Value::Undefined;
        self.class_field_initializer = false;
        Ok(())
    }

    /// Finishes the outer script/module execution context initialized by
    /// [`Self::prepare_root_execution`]. It deliberately mirrors the normal
    /// public execution cleanup so a debugger-resumed script cannot retain
    /// frame-local roots, lexical bindings, or WeakRef keep-alive state.
    pub(super) fn finish_root_execution(
        &mut self,
        result: Result<Value, RuntimeError>,
    ) -> Result<Value, RuntimeError> {
        self.discard_finished_debugger_frames();
        let result = result.and_then(|value| {
            if let Value::Object(id) = value {
                self.result_root = Some(self.heap.root(id)?);
            }
            Ok(value)
        });
        let result = match result {
            Err(RuntimeError::Thrown(Value::Object(id))) => match self.heap.root(id) {
                Ok(root) => {
                    self.result_root = Some(root);
                    Err(RuntimeError::Thrown(Value::Object(id)))
                }
                Err(error) => Err(error.into()),
            },
            result => result,
        };
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
        self.inherited_with_depth = 0;
        self.top_level_module = false;
        self.pending_completions.clear();
        self.completion_saves.clear();
        self.disposables = Default::default();
        self.dispose_marks = Default::default();
        self.call_stack.clear();
        self.release_finished_execution_control();
        self.refresh_scheduling_ownership();
        // WeakRef's KeepDuringJob guarantee ends only after the complete
        // script/module job (including abrupt completion cleanup) has run.
        self.kept_weak_objects.clear();
        self.collect_realm_garbage()?;
        self.enqueue_finalization_cleanup_jobs()?;
        self.uncaught_throw_site = if result.as_ref().is_err_and(RuntimeError::is_catchable) {
            self.pending_throw_site
        } else {
            None
        };
        self.pending_throw_site = None;
        self.synchronize_scheduling_usage()?;
        self.uncaught_diagnostic_origin = if result.as_ref().is_err_and(RuntimeError::is_catchable)
        {
            self.pending_diagnostic_origin.take()
        } else {
            None
        };
        self.pending_diagnostic_origin = None;
        if let Err(error) = &result {
            self.report_host_exception_with_origin(error, self.uncaught_diagnostic_origin.clone())?;
        }
        result
    }

    pub(super) fn enqueue_finalization_cleanup_jobs(&mut self) -> Result<(), RuntimeError> {
        if self.host_scheduling.is_some() {
            return self.enqueue_host_finalization_tasks();
        }
        // Host-neutral queues have no document admission policy. Move cells
        // straight into their traced queue without bulk jobs/live-cell copies.
        let jobs = &mut self.promise_jobs;
        self.heap
            .drain_finalization_registry_cleanup_jobs(|callback, holdings| {
                jobs.push_back(PromiseJob::FinalizationCleanup { callback, holdings });
            });
        Ok(())
    }

    /// GlobalDeclarationInstantiation for this VM's implemented classic
    /// script subset. Validation happens before execution, while bindings are
    /// created before any initializer so an abrupt script still leaves the
    /// required persistent TDZ state in its realm.
    pub(super) fn prepare_global_declarations(
        &mut self,
        code: &Bytecode,
    ) -> Result<(), RuntimeError> {
        let result = self.prepare_global_declarations_inner(code);
        self.reconcile_scheduling_ownership();
        result
    }

    fn prepare_global_declarations_inner(&mut self, code: &Bytecode) -> Result<(), RuntimeError> {
        let slots = code.scopes.first().map(Vec::as_slice).unwrap_or_default();
        if slots.is_empty() {
            return Ok(());
        }
        let global = self
            .global("globalThis")?
            .object_id()
            .expect("globalThis is an object");
        let prepared = self.prepare_global_declaration_records(global, code, false)?;

        for &slot in slots {
            let binding = &code.bindings[slot as usize];
            if binding.lexical {
                self.materialize_lexical_global(global, &binding.name)?;
            }
        }

        for &slot in slots {
            let binding = &code.bindings[slot as usize];
            let existing = self.global_bindings.get(&binding.name);
            if binding.lexical {
                if existing.is_some_and(|binding| !binding.property)
                    || self
                        .heap
                        .get_own_property_descriptor(global, binding.name.as_str())
                        .expect("globalThis is the permanently rooted ordinary realm global")
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

        self.publish_global_declaration_records(global, code, false, prepared)
    }

    /// Whether a `var` declaration names an existing accessor property of the
    /// global object. CreateGlobalVarBinding leaves an existing own property
    /// untouched, but a cell-backed global binding mirrors a *data* property,
    /// so it can neither read through the getter nor forward to the setter:
    /// such a `var` gets no global binding at all. Name lookups and
    /// assignments then reach the accessor through the global object itself.
    pub(super) fn global_var_is_accessor(
        &self,
        global: ObjectId,
        binding: &Binding,
        function: bool,
    ) -> bool {
        if function || binding.lexical {
            return false;
        }
        // Both declaration-instantiation callers resolved the permanently
        // rooted ordinary realm global before this non-allocating query.
        self.heap
            .get_own_property_descriptor(global, binding.name.as_str())
            .expect("declaration instantiation retains its validated ordinary global")
            .is_some_and(|descriptor| descriptor.accessor())
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
        if LAZY_STANDARD_GLOBALS.contains(&name) {
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
            || self
                .heap
                .is_extensible(global)
                .expect("the preceding descriptor read validated the ordinary global"))
    }

    pub(super) fn can_declare_global_function(
        &mut self,
        global: ObjectId,
        name: &str,
    ) -> Result<bool, RuntimeError> {
        self.materialize_lexical_global(global, name)?;
        let Some(descriptor) = self.heap.get_own_property_descriptor(global, name)? else {
            return Ok(self
                .heap
                .is_extensible(global)
                .expect("the preceding descriptor read validated the ordinary global"));
        };
        Ok(descriptor.configurable == Some(true)
            || (descriptor.value.is_some()
                && descriptor.writable == Some(true)
                && descriptor.enumerable == Some(true)))
    }

    pub(super) fn initialize_global_binding(
        &mut self,
        global: ObjectId,
        binding: &Binding,
        function: bool,
        configurable: bool,
        prepared: global_bindings::PreparedGlobalValue,
    ) -> Result<GlobalBinding, RuntimeError> {
        let property = !binding.lexical;
        let global_bindings::PreparedGlobalValue {
            initial,
            existed,
            configurable: was_configurable,
            charge,
        } = prepared;
        let mut initial_charge = Some(charge);
        self.synchronize_scheduling_usage()?;
        let cell = self.with_roots(|heap| heap.alloc_object(None))?;
        let root = self.heap.root(cell)?;
        let result = (|| {
            if property {
                if function && was_configurable || !existed {
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
                    }).expect("global declaration validation established an existing writable data property; replacing it with undefined cannot grow storage");
                }
            }
            if property {
                self.with_roots(|heap| {
                    let result = heap.set(cell, "value", initial);
                    drop(initial_charge.take());
                    result
                })?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.release_root(root);
            return Err(error);
        }
        Ok(GlobalBinding {
            cell,
            mutable: binding.mutable,
            property,
            _root: root,
        })
    }

    /// Drops every purely-lexical (non-property) entry from
    /// `global_bindings`, releasing each one's GC root first. `var`/function
    /// global declarations (`property: true`, backed by a real `globalThis`
    /// data property) are left exactly as they are -- only the bookkeeping
    /// this VM uses to detect an invalid global lexical redeclaration is
    /// reset, for a caller (`ShadowRealm.prototype.evaluate`, so far the
    /// only one) whose own semantics need a fresh global lexical scope on
    /// each call rather than this VM's ordinary one-persistent-scope model.
    pub(super) fn reset_lexical_global_bindings(&mut self) {
        let heap = &mut self.heap;
        self.global_bindings.retain(|_, binding| {
            if !binding.property {
                heap.unroot(binding._root)
                    .expect("the lexical binding owns its root");
            }
            binding.property
        });
        if self.global_bindings.is_empty() && self.reserved_global_binding_slots == 0 {
            self.global_bindings = HashMap::new();
        }
        self.reconcile_scheduling_ownership();
    }

    pub(super) fn global_binding_value(&self, name: &str) -> Result<Option<Value>, RuntimeError> {
        let Some(binding) = self.global_bindings.get(name) else {
            return Ok(None);
        };
        Ok(self
            .heap
            .get_own(binding.cell, "value")
            .expect("each published GlobalBinding owns its cell's permanent root"))
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
        if self
            .heap
            .get_own(cell, "value")
            .expect("each published GlobalBinding owns its cell's permanent root")
            .is_none()
        {
            return Err(RuntimeError::ReferenceError(name.into()));
        }
        // Global immutable declarations are const/using bindings. A named
        // function expression's non-strict self binding belongs to its call
        // frame and is handled by binding_allows_assignment, never here.
        if !mutable {
            return Err(RuntimeError::TypeError(format!(
                "assignment to constant {name}"
            )));
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

    /// Whether `slot` is an eval-created `var` whose binding was deleted (its
    /// cell then has no value): closures still hold the cell, but the name
    /// resolves outward again.
    pub(super) fn eval_var_deleted(&self, slot: usize) -> bool {
        if !self
            .binding_metadata
            .get(slot)
            .is_some_and(|binding| binding.eval_var)
        {
            return false;
        }
        let Some(&cell) = self.cells.get(&slot) else {
            return false;
        };
        self.heap
            .get_own(cell, "value")
            .expect("the current frame retains its eval binding cell")
            .is_none()
    }

    /// PutValue for a reference that resolved to binding slot `slot`.
    /// `recreate` marks the initialization of an eval `var` declaration: its
    /// reference was resolved before the initializer ran, so a binding the
    /// initializer deleted (`var x = delete x`) is created again, where a
    /// later assignment to the deleted name would not resolve to it.
    pub(super) fn assign_binding_slot(
        &mut self,
        code: &Bytecode,
        slot: usize,
        value: Value,
        recreate: bool,
    ) -> Result<(), RuntimeError> {
        let name = &code.bindings[slot].name;
        if self.store_dynamic_eval_shadowing_binding(slot, name, value.clone())? {
            return Ok(());
        }
        if self.eval_var_deleted(slot) {
            return if recreate {
                self.recreate_eval_var(slot, name, value)
            } else {
                self.assign_unbound_name(name, value, code.strict)
            };
        }
        // ECMA-262 §9.1.1.1.5: TDZ takes precedence over the immutable-binding
        // assignment error, including const.
        if self.binding_value(slot).expect("the current frame retains this binding cell, and deleted eval bindings were resolved before this read").is_none() {
            return Err(RuntimeError::ReferenceError(name.clone()));
        }
        if binding_allows_assignment(&code.bindings[slot], code.strict)? {
            self.store_binding(slot, value)?;
        }
        Ok(())
    }

    /// PutValue for a name that no static binding resolves: an eval-created
    /// binding, a global binding or a property of the global object (created
    /// when missing, except in strict code).
    pub(super) fn assign_unbound_name(
        &mut self,
        name: &str,
        value: Value,
        strict: bool,
    ) -> Result<(), RuntimeError> {
        if self.set_dynamic_eval_binding(name, value.clone())?
            || self.set_global_binding(name, value.clone())?
        {
            return Ok(());
        }
        let global = self.global("globalThis")?;
        let global_id = global.object_id().expect("globalThis is an object");
        let key: PropertyName = name.into();
        self.materialize_lexical_global(global_id, name)?;
        if strict && !self.has_property(global_id, &key)? {
            return Err(RuntimeError::ReferenceError(name.into()));
        }
        self.set_property(&global, &key, &value)
    }

    /// Creates again an eval `var` binding that its own initializer deleted.
    /// Only the declaring eval code can do this: its own slot tells whether
    /// the variable environment is the global one or a function's.
    fn recreate_eval_var(
        &mut self,
        slot: usize,
        name: &str,
        value: Value,
    ) -> Result<(), RuntimeError> {
        let cell = *self.cells.get(&slot).expect(
            "assign_binding_slot just verified this retained cell has a deleted eval value",
        );
        if self.script_global_slots.contains_key(&slot) {
            self.recreate_global_eval_binding(slot, name, cell, value)
        } else {
            // InitializeBinding can request recreation only for the declaring
            // eval's own var slot. Global slots were handled above; captures
            // of existing function eval vars and fresh declarations are both
            // recorded by prepare_dynamic_eval_bindings before interpretation.
            debug_assert!(self.eval_dynamic_slots.contains_key(&slot));
            self.recreate_function_eval_var(slot, name, cell, value)
        }
    }

    /// `delete name` for a name that statically resolved to an eval-created
    /// `var` or function binding. The binding is removed from whichever
    /// environment holds it (a function's eval variables or the global
    /// object) and its cell loses its value, so every closure that captured
    /// the cell sees the name resolve outward. A binding that is already gone
    /// is looked up again like any unresolved name.
    pub(super) fn delete_eval_var(&mut self, slot: usize) -> Result<bool, RuntimeError> {
        let name = self.binding_metadata[slot].name.clone();
        let cell = *self
            .cells
            .get(&slot)
            .expect("eval declaration instantiation or closure capture installed this cell");
        if self
            .heap
            .get_own(cell, "value")
            .expect("the current frame retains its eval binding cell")
            .is_none()
        {
            return self.delete_unbound_name(&name);
        }
        let global_binding = self
            .global_bindings
            .get(&name)
            .is_some_and(|binding| binding.cell == cell && binding.property);
        if global_binding {
            return self.delete_unbound_name(&name);
        }
        self.remove_eval_binding(&name, cell);
        Ok(true)
    }

    /// Removes the eval-created binding `name` whose cell is `cell` from
    /// every record of it: the function's eval variables, the variable
    /// environment object closures capture, and the cell's own value.
    fn remove_eval_binding(&mut self, name: &str, cell: ObjectId) {
        if self
            .dynamic_eval_bindings
            .get(name)
            .is_some_and(|binding| binding.cell == cell)
        {
            self.dynamic_eval_bindings.remove(name);
        } else {
            for bindings in self.dynamic_eval_outer_bindings.iter_mut().rev() {
                if bindings
                    .get(name)
                    .is_some_and(|binding| binding.cell == cell)
                {
                    bindings.remove(name);
                    break;
                }
            }
        }
        for object in self.with_objects.clone() {
            let id = object
                .object_id()
                .expect("with entry boxes its operand before retaining the environment");
            if self.is_parameter_eval_env(id)
                && self
                    .heap
                    .get_own(id, name)
                    .expect("the frame retains its ordinary parameter-eval environment")
                    == Some(Value::Object(cell))
            {
                self.heap
                    .delete(id, name)
                    .expect("removing an ordinary environment data property cannot allocate");
            }
        }
        self.heap
            .delete(cell, "value")
            .expect("the resolved eval cell is retained and deletion cannot allocate");
    }

    /// `delete name` where the name was found in a function's variable
    /// environment object (see `new_parameter_eval_env`): deleting an eval
    /// variable is always possible.
    pub(super) fn delete_eval_env_var(
        &mut self,
        env: ObjectId,
        name: &str,
    ) -> Result<bool, RuntimeError> {
        let Some(cell) = self
            .heap
            .get_own(env, name)?
            .and_then(|value| value.object_id())
        else {
            return Ok(true);
        };
        self.remove_eval_binding(name, cell);
        // The binding may be missing from the environment of the frame that
        // is running (a closure), so drop this record of it in any case.
        self.heap
            .delete(env, name)
            .expect("the preceding own lookup validated this retained ordinary environment");
        Ok(true)
    }

    pub(super) fn delete_dynamic_eval_binding(&mut self, name: &str) -> Result<bool, RuntimeError> {
        // delete_unbound_name selects this path only after finding the name;
        // no user code runs between that lookup and removing its binding.
        let binding = self
            .dynamic_eval_bindings
            .remove(name)
            .or_else(|| {
                self.dynamic_eval_outer_bindings
                    .iter_mut()
                    .rev()
                    .find_map(|bindings| bindings.remove(name))
            })
            .expect("deletion resolved a dynamic eval binding");
        self.heap.delete(binding.cell, "value").map_err(Into::into)
    }

    /// The environment object of a sloppy function whose parameter list has a
    /// direct eval. It is an ordinary object without prototype, entered like a
    /// `with` object; the marker property is not a valid identifier, so no
    /// `var` an eval declares can collide with it. Each own property maps a
    /// declared name to the cell that holds its value.
    pub(super) fn new_parameter_eval_env(&mut self) -> Result<ObjectId, RuntimeError> {
        let env = self.with_roots(|heap| heap.alloc_object(None))?;
        self.stack.push(Value::Object(env));
        let marked = self.with_roots(|heap| heap.set(env, "#eval-env", Value::Bool(true)));
        self.stack.pop();
        marked?;
        Ok(env)
    }

    /// Whether `object` is such an environment (as opposed to a `with` object).
    pub(super) fn is_parameter_eval_env(&self, object: ObjectId) -> bool {
        matches!(
            self.heap.get_own(object, "#eval-env"),
            Ok(Some(Value::Bool(true)))
        )
    }

    /// Whether `name`, which no function or block binding resolves, resolves
    /// to an eval-created binding, a global binding or a property of the
    /// global object (the standard globals are created lazily).
    pub(super) fn unbound_name_resolves(&mut self, name: &str) -> Result<bool, RuntimeError> {
        if self.dynamic_eval_bindings.contains_key(name)
            || self
                .dynamic_eval_outer_bindings
                .iter()
                .any(|bindings| bindings.contains_key(name))
            || self.global_bindings.contains_key(name)
        {
            return Ok(true);
        }
        let global = self
            .global("globalThis")?
            .object_id()
            .expect("globalThis is an object");
        self.materialize_lexical_global(global, name)?;
        self.has_property(global, &name.into())
    }

    /// `delete name` for a name that no function or block binding resolves:
    /// the reference is looked up in eval-created bindings, then the global
    /// Environment Record (§9.1.1.4.7 DeleteBinding). A declarative (`let`,
    /// `const`, `class`) global binding cannot be deleted; a property of the
    /// global object is deleted when it is configurable, and a name that
    /// resolves nowhere deletes "successfully".
    pub(super) fn delete_unbound_name(&mut self, name: &str) -> Result<bool, RuntimeError> {
        let in_eval_binding = self.dynamic_eval_bindings.contains_key(name)
            || self
                .dynamic_eval_outer_bindings
                .iter()
                .any(|bindings| bindings.contains_key(name));
        if in_eval_binding {
            return self.delete_dynamic_eval_binding(name);
        }
        if self
            .global_bindings
            .get(name)
            .is_some_and(|binding| !binding.property)
        {
            return Ok(false);
        }
        let global = self
            .global("globalThis")?
            .object_id()
            .expect("globalThis is an object");
        self.object_delete(global, &name.into())
    }

    pub(super) fn store_global_cell(
        &mut self,
        cell: ObjectId,
        value: Value,
    ) -> Result<(), RuntimeError> {
        let property = self.global_bindings.iter().find_map(|(name, binding)| {
            (binding.cell == cell && binding.property).then(|| name.clone())
        });
        // SetMutableBinding of the global Environment Record: a binding backed
        // by a non-writable global property (`NaN`, `undefined`) rejects the
        // write, silently in sloppy code and with a TypeError in strict code.
        if let Some(name) = &property {
            let global = *self.globals.get("globalThis").expect(
                "a property-backed global binding was published with its permanent global object",
            );
            if self
                .heap
                .get_own_property_descriptor(global, name.as_str())
                .expect("the property-backed binding retains the ordinary realm global")
                .is_some_and(|descriptor| descriptor.writable == Some(false))
            {
                return if self.strict {
                    Err(RuntimeError::TypeError(format!(
                        "cannot assign to read-only global {name}"
                    )))
                } else {
                    Ok(())
                };
            }
        }
        self.with_roots(|heap| heap.set(cell, "value", value.clone()))?;
        if let Some(name) = property {
            let global = *self.globals.get("globalThis").expect(
                "a property-backed global binding was published with its permanent global object",
            );
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
            // CloneScope is emitted only after every initializer in a lexical
            // for declaration has completed. These retained lexical cells do
            // not use the deleted-eval-variable or global-property fallback.
            let value = self
                .binding_value(slot)
                .expect("the active lexical for cell remains live")
                .expect("lexical for initializers complete before CloneScope");
            let cell = self.with_roots(|heap| heap.alloc_object(None))?;
            // Insert before the allocation-backed store so the fresh cell is
            // an interpreter root if the store needs to collect.
            self.cells.insert(slot, cell);
            self.bindings[slot] = None;
            self.with_roots(|heap| heap.set(cell, "value", value))?;
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

    pub(super) fn close_iterators_to(&mut self, iterators: &mut IteratorRecords, depth: usize) {
        let origin = self.take_pending_throw_origin();
        let _origin_charge = self.charge_diagnostic_payload(origin.payload_bytes());
        let _ = self.close_iterators_to_first_error(iterators, depth);
        self.restore_pending_throw_origin(origin);
    }

    /// IteratorClose with an existing throw ignores a close failure, including
    /// its source position and throw epoch. Cleanup may also catch new errors.
    pub(super) fn iterator_close_preserving_throw(&mut self, record: &Value) {
        let origin = self.take_pending_throw_origin();
        let _origin_charge = self.charge_diagnostic_payload(origin.payload_bytes());
        let _ = self.iterator_close(record);
        self.restore_pending_throw_origin(origin);
    }

    /// Closes the records being left and returns the first close error after
    /// still giving every outer record its own cleanup opportunity.
    fn close_iterators_to_first_error(
        &mut self,
        iterators: &mut IteratorRecords,
        depth: usize,
    ) -> Option<RuntimeError> {
        // A compiler-emitted `break` can close its target for-of iterator
        // before a surrounding handler starts finalizer cleanup.
        // Scope teardown can release all of these records before cleanup.
        // A callback on the inner iterator can collect before the outer one
        // is visited, so retain the entire pending set until cleanup ends.
        let base = self.stack.len();
        self.stack.extend(iterators.iter().skip(depth).cloned());
        let origin = self.take_pending_throw_origin();
        let _origin_charge = self.charge_diagnostic_payload(origin.payload_bytes());
        let mut first_error = None;
        let mut first_origin = None;
        let mut first_origin_charge = self.charge_diagnostic_payload(0);
        while iterators.len() > depth {
            let record = iterators.pop().expect("pending iterator depth was checked");
            let result = self.iterator_close(&record);
            if let Err(error) = result {
                if first_error.is_none() {
                    if let RuntimeError::Thrown(value) = &error {
                        self.stack.push(value.clone());
                    }
                    first_error = Some(error);
                    let origin = self.take_pending_throw_origin();
                    first_origin_charge.set_bytes(origin.payload_bytes());
                    first_origin = Some(origin);
                }
            }
        }
        self.stack.truncate(base);
        self.restore_pending_throw_origin(first_origin.unwrap_or(origin));
        first_error
    }

    /// Closes active iterators while completing an ordinary `return`.
    ///
    /// `IteratorCloseAll` changes the completion to the first close failure,
    /// then retains that throw while it closes outer iterators. This differs
    /// from cleanup for a pre-existing throw, where the original throw wins
    /// over every close failure.
    pub(super) fn close_iterators_for_return(
        &mut self,
        iterators: &mut IteratorRecords,
        depth: usize,
    ) -> Result<(), RuntimeError> {
        self.close_iterators_to_first_error(iterators, depth)
            .map_or(Ok(()), Err)
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
            RuntimeError::StringLimit { limit } => {
                self.error_object("RangeError", format!("BlueJS string exceeds {limit} bytes"))
            }
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

    /// Whether the running function frame can be replaced by a tail call: it
    /// is an ordinary (not construct) call made through `call_with_target`,
    /// which runs the callee once this frame is gone. An arrow is never a
    /// construct call (its `new.target` is only the one it captured).
    pub(super) fn frame_can_be_replaced(&self, code: &Bytecode) -> bool {
        self.call_depth != 0 && (code.arrow || self.new_target == Value::Undefined)
    }

    pub(super) fn resolve_completion(
        &mut self,
        code: &Bytecode,
        handlers: &mut HandlerRecords,
        iterators: &mut IteratorRecords,
        completion: Completion,
    ) -> Result<CompletionAction, RuntimeError> {
        let mut handler_charge =
            self.charge_diagnostic_payload(Self::diagnostic_handler_bytes(handlers));
        if let Completion::Halt(value) = completion {
            return Ok(CompletionAction::Return(value));
        }
        if let Completion::Resume(metadata) = completion {
            let frame = handlers
                .pop_if(|frame| frame.metadata == metadata)
                .expect("a finalizer runs under its handler frame");
            // The finalizer completed normally, so the try statement's
            // completion value is the one the try or catch block produced
            // (or the abrupt completion carried, such as a `break`), not
            // whatever the finalizer's own statements produced. It was saved
            // when the finalizer was entered.
            self.restore_completion();
            return match frame.pending {
                Some(pending) => {
                    let completion = self.pending_completions[pending].clone();
                    if matches!(completion, Completion::Throw(_)) {
                        let mut origin = frame.pending_origin;
                        // A suspended finalizer can resume in another script
                        // whose counter happens to equal the old throw epoch.
                        // This is a fresh propagation of the saved origin.
                        origin.epoch = self.throw_epoch.wrapping_add(1);
                        self.restore_pending_throw_origin(origin);
                    }
                    handler_charge.set_bytes(0);
                    self.resolve_completion(code, handlers, iterators, completion)
                }
                None => Ok(CompletionAction::Continue),
            };
        }

        // Keep a potential thrown/returned object reachable while scope and
        // iterator cleanup can call user code and trigger collection.
        self.pending_completions.push(completion.clone());
        let mut completion = completion;
        loop {
            handler_charge.set_bytes(Self::diagnostic_handler_bytes(handlers));
            let Some(frame) = handlers.last() else {
                return Ok(match completion {
                    Completion::Throw(error) => CompletionAction::Throw(error),
                    // Every finalizer has run, so the for-of iterators still
                    // open are closed now, innermost first. A failing
                    // `return()` replaces the return with its own throw,
                    // which no handler of this function can catch anymore.
                    Completion::Return(value) => {
                        match self.close_iterators_to_first_error(iterators, 0) {
                            None => CompletionAction::Return(value),
                            Some(error) if error.is_catchable() => CompletionAction::Throw(error),
                            Some(error) => return Err(error),
                        }
                    }
                    Completion::TailRecur(args) => CompletionAction::TailRecur(args),
                    Completion::TailCall(values) => CompletionAction::TailCall(values),
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
                    // A finalizer running for a pending abrupt completion may
                    // contain its own loops: a jump within the finalizer has
                    // not left it, and must keep the pending completion.
                    HandlerState::Finally => handler
                        .finally
                        .zip(handler.finally_end)
                        .map(|(start, end)| (start as usize, end as usize)),
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
                let original_origin = matches!(completion, Completion::Throw(_))
                    .then(|| self.take_pending_throw_origin());
                let _original_origin_charge = self.charge_diagnostic_payload(
                    original_origin
                        .as_ref()
                        .map_or(0, ThrowOrigin::payload_bytes),
                );
                if let Some(error) = self.close_iterators_to_first_error(iterators, iterator_depth)
                {
                    // IteratorClose preserves a pre-existing throw, but a
                    // return/break/continue is replaced by its first close
                    // failure. A host resource abort must remain outside the
                    // JavaScript completion path.
                    if !matches!(completion, Completion::Throw(_)) {
                        if !error.is_catchable() {
                            return Err(error);
                        }
                        completion = Completion::Throw(error);
                        *self
                            .pending_completions
                            .last_mut()
                            .expect("completion remains rooted") = completion.clone();
                    }
                }
                if let Some(origin) = original_origin {
                    self.restore_pending_throw_origin(origin);
                }
                self.unwind_with(with_depth);
            }

            if state == HandlerState::Try {
                if let (Some(target), Completion::Throw(error)) = (catch, &completion) {
                    let value = self.error_value(error.clone())?;
                    let origin = self.take_pending_throw_origin();
                    let frame = handlers.last_mut().expect("handler was inspected above");
                    frame.state = HandlerState::Catch;
                    frame.pending_origin = origin;
                    self.stack.push(value);
                    // A class's heritage or computed key may have thrown while
                    // the function was running as strict code.
                    self.strict = code.strict;
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
                    // Save the completion value the abrupt completion carries;
                    // a normal finalizer restores it (see `Completion::Resume`).
                    self.completion_saves
                        .push((self.completion.clone(), self.completion_empty));
                    let frame = handlers.last_mut().expect("handler was inspected above");
                    frame.state = HandlerState::Finally;
                    frame.pending = Some(pending);
                    frame.pending_origin = self.take_pending_throw_origin();
                    self.strict = code.strict;
                    return Ok(CompletionAction::Jump(target as usize));
                }
            }
            handlers.pop();
            // A finalizer that completes abruptly replaces the completion that
            // entered it, so the value saved at its entry is never restored.
            if state == HandlerState::Finally {
                self.completion_saves.pop();
            }
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
        let prepared = self.prepare_gc_trace()?;
        let before = self.heap.stats();
        self.heap.push_scoped_roots(prepared.batch);
        let result = operation(&mut self.heap);
        let collection_error = self.heap.take_collection_error();
        self.finish_gc_trace(prepared.charge);
        let after = self.heap.stats();
        if before.minor_collections != after.minor_collections
            || before.major_collections != after.major_collections
        {
            self.prune_collected_promise_state();
        }
        if let Some(error) = collection_error {
            return Err(error.into());
        }
        result.map_err(RuntimeError::from)
    }

    pub(super) fn run(&mut self, code: &Bytecode) -> Result<Value, RuntimeError> {
        let pending_base = self.pending_completions.len();
        let save_base = self.completion_saves.len();
        let mut iterators = self.iterator_records(Vec::new());
        let result = self
            .interpret(code, &mut iterators, 0, None, None, None)
            .and_then(root_interpreter_exit);
        if result.is_err() {
            if let Err(RuntimeError::Thrown(value)) = &result {
                self.stack.push(value.clone());
            }
            self.stack.extend(iterators.iter().cloned());
            while let Some(record) = iterators.pop() {
                // IteratorClose preserves an existing throw even when return
                // throws too. Resource exhaustion retains the original limit.
                self.iterator_close_preserving_throw(&record);
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
        let captures = NativeCaptures::adopt(captures, self.host_scheduling.as_ref())?;
        self.execute_eval_owned(code, captures, global_var_environment)
    }

    pub(super) fn execute_eval_owned(
        &mut self,
        code: &Bytecode,
        captures: NativeCaptures,
        global_var_environment: bool,
    ) -> Result<Value, RuntimeError> {
        let result = self.execute_eval_inner(code, captures, global_var_environment);
        self.reconcile_scheduling_ownership();
        result
    }

    fn execute_eval_inner(
        &mut self,
        code: &Bytecode,
        captures: NativeCaptures,
        global_var_environment: bool,
    ) -> Result<Value, RuntimeError> {
        let mut prepared = self.prepare_frame_bindings(code, FrameBindingKind::Script)?;
        let mut prepared_maps = self.prepare_frame_maps(captures.values.len(), 0, 0)?;
        let _caller_payload = self.charge_execution_payload(self.bindings.iter().flatten().fold(
            host_scheduling::value_payload_bytes(&self.completion),
            |bytes, value| bytes.saturating_add(host_scheduling::value_payload_bytes(value)),
        ));
        let _caller_control = self.charge_execution_control(
            self.displaced_control_bytes(execution_payload::ControlTransfer::Eval),
        );
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
        self.stack.extend(
            self.arguments
                .iter()
                .filter_map(Value::object_id)
                .map(Value::Object),
        );
        self.stack.extend(
            self.disposables
                .iter()
                .flat_map(DisposableResource::values)
                .filter_map(Value::object_id)
                .map(Value::Object),
        );
        let disposables = std::mem::take(&mut self.disposables);
        let dispose_marks = std::mem::take(&mut self.dispose_marks);
        let (frame_bindings, frame_metadata, _) = prepared.take_parts();
        let bindings = std::mem::replace(&mut self.bindings, frame_bindings);
        let binding_metadata = std::mem::replace(&mut self.binding_metadata, frame_metadata);
        drop(prepared);
        let (captures, capture_charge) = captures.into_parts();
        prepared_maps.adopt_captures(captures);
        drop(capture_charge);
        let cells = std::mem::replace(&mut self.cells, prepared_maps.take_cells());
        drop(prepared_maps);
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
        self.disposables = disposables;
        self.dispose_marks = dispose_marks;
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
        self.publish_function_eval_declarations(code)
    }

    /// EvalDeclarationInstantiation's global-variable branch. The eval
    /// lexical environment remains transient, so only `var` and top-level
    /// function bindings are published into the realm's global environment.
    pub(super) fn prepare_eval_global_var_declarations(
        &mut self,
        code: &Bytecode,
    ) -> Result<(), RuntimeError> {
        let result = self.prepare_eval_global_var_declarations_inner(code);
        self.reconcile_scheduling_ownership();
        result
    }

    fn prepare_eval_global_var_declarations_inner(
        &mut self,
        code: &Bytecode,
    ) -> Result<(), RuntimeError> {
        let slots = code.scopes.first().map(Vec::as_slice).unwrap_or_default();
        let global = self
            .global("globalThis")?
            .object_id()
            .expect("globalThis is an object");
        let prepared = self.prepare_global_declaration_records(global, code, true)?;

        for &slot in slots {
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

        self.publish_global_declaration_records(global, code, true, prepared)
    }

    /// Evaluates a new classic script in the current realm while another
    /// script/function frame is active (the Test262 `$262.evalScript` host
    /// path). It deliberately gets fresh script bindings and global `this`,
    /// but preserves the caller frame and the remaining resource budget.
    pub(super) fn execute_nested_script(&mut self, code: &Bytecode) -> Result<Value, RuntimeError> {
        let result = self.execute_nested_script_inner(code);
        self.reconcile_scheduling_ownership();
        result
    }

    fn execute_nested_script_inner(&mut self, code: &Bytecode) -> Result<Value, RuntimeError> {
        let mut prepared = self.prepare_frame_bindings(code, FrameBindingKind::Script)?;
        // Global construction can refuse an allocation. Resolve it before
        // displacing the caller's frame so an entry refusal cannot strand
        // that frame in the nested execution state.
        let global_this = self.global("globalThis")?;
        let _caller_payload = self.charge_execution_payload(self.caller_execution_payload_bytes());
        let _caller_control = self.charge_execution_control(
            self.displaced_control_bytes(execution_payload::ControlTransfer::NestedScript),
        );
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
        self.stack.extend(
            self.arguments
                .iter()
                .filter_map(Value::object_id)
                .map(Value::Object),
        );
        self.stack.extend(self.with_objects.iter().cloned());
        self.stack.extend(
            self.disposables
                .iter()
                .flat_map(DisposableResource::values)
                .filter_map(Value::object_id)
                .map(Value::Object),
        );
        self.stack.extend(
            self.disposables
                .iter()
                .flat_map(DisposableResource::values)
                .filter_map(Value::object_id)
                .map(Value::Object),
        );
        let disposables = std::mem::take(&mut self.disposables);
        let dispose_marks = std::mem::take(&mut self.dispose_marks);
        let (frame_bindings, frame_metadata, _) = prepared.take_parts();
        let bindings = std::mem::replace(&mut self.bindings, frame_bindings);
        let binding_metadata = std::mem::replace(&mut self.binding_metadata, frame_metadata);
        drop(prepared);
        let cells = std::mem::take(&mut self.cells);
        let completion = std::mem::replace(&mut self.completion, Value::Undefined);
        let completion_empty = std::mem::replace(&mut self.completion_empty, true);
        let active_scopes = std::mem::take(&mut self.active_scopes);
        let active_scope_slots = std::mem::take(&mut self.active_scope_slots);
        let with_objects = std::mem::take(&mut self.with_objects);
        let strict = std::mem::replace(&mut self.strict, code.strict);
        let top_level_module = std::mem::replace(&mut self.top_level_module, false);
        let this = std::mem::replace(&mut self.this, global_this);
        let arguments = std::mem::take(&mut self.arguments);
        let new_target = std::mem::replace(&mut self.new_target, Value::Undefined);
        let new_target_allowed = std::mem::replace(&mut self.new_target_allowed, false);
        let home_object = std::mem::take(&mut self.home_object);
        let class_field_initializer = std::mem::take(&mut self.class_field_initializer);
        let script_global_slots = std::mem::take(&mut self.script_global_slots);
        let result = self
            .prepare_global_declarations(code)
            .and_then(|()| self.run(code));
        self.disposables = disposables;
        self.dispose_marks = dispose_marks;
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
        self.class_field_initializer = class_field_initializer;
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

    /// The `fallback` argument `with_lookup`/`with_get`/`with_get_or_undefined`
    /// use for the enclosing binding behind the active `with` objects (or, for
    /// a function whose body contains a direct eval, behind its own
    /// parameter/variable environment): `Some(Some(value))` for an
    /// initialized binding, `Some(None)` for one still in its temporal dead
    /// zone (a plain reference throws for this either way; `typeof` throws
    /// only for this case), and `None` when there is no local binding to
    /// fall back on at all. A sloppy direct eval's own deletable `var` that
    /// has been deleted, with nothing for its name to resolve outward to
    /// either, reports `None` here rather than `Some(None)`: unlike TDZ, its
    /// name is genuinely unresolvable, which `with_get`/`with_get_method`
    /// still turn into the same ReferenceError but `with_get_or_undefined`
    /// must not.
    pub(super) fn with_binding_fallback(
        &mut self,
        name: &str,
    ) -> Result<Option<Option<Value>>, RuntimeError> {
        let Some(slot) = self.active_binding_slot(name) else {
            return Ok(None);
        };
        let value = self.eval_aware_binding_value(slot, name)?;
        if value.is_none() && self.eval_var_deleted(slot) {
            return Ok(None);
        }
        Ok(Some(value))
    }

    /// Finds the innermost currently-active lexical or captured binding for
    /// an object-environment lookup. Bytecode can contain several slots with
    /// the same source name (notably catch parameters), so the last matching
    /// slot is not necessarily live.
    pub(super) fn active_binding_slot(&self, name: &str) -> Option<usize> {
        self.active_scope_slots
            .iter()
            .rev()
            .find_map(|slots| {
                slots.iter().rev().find_map(|slot| {
                    let slot = *slot as usize;
                    (self.binding_metadata[slot].name == name).then_some(slot)
                })
            })
            .or_else(|| {
                self.cells
                    .keys()
                    .copied()
                    .filter(|slot| self.binding_metadata[*slot].name == name)
                    .max()
            })
    }

    /// Lexical names between a direct eval site and the active function's
    /// VariableEnvironment. Unlike captured outer bindings, these prevent a
    /// sloppy eval `var` declaration from being instantiated.
    pub(super) fn eval_lexical_conflicts(&self) -> Vec<String> {
        let variable_scope_position = self
            .active_scopes
            .iter()
            .position(|scope| *scope == self.variable_scope);
        // The variable scope's own lexical declarations conflict too: this
        // engine keeps a function body's top-level `let`/`const`/`class`
        // beside its vars, where the specification uses a separate lexical
        // environment precisely so that a direct eval can see them.
        let start = variable_scope_position.unwrap_or(0);
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
            .and_then(|callee| self.heap.closure_code(callee).ok().flatten())
            .is_some_and(|code| !code.arrow);
        if variable_scope_position.is_none() && ordinary_function {
            conflicts.extend(self.variable_scope_lexicals.iter().cloned());
        }
        conflicts.into_iter().collect()
    }
}
