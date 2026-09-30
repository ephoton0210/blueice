// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Explicit Resource Management (ECMA-262 edition 17, `using`/`await using`
//! declarations, `DisposableStack`/`AsyncDisposableStack`, `SuppressedError`).
//!
//! This module implements the spec's "Operations on Disposable Objects"
//! abstract operations (`GetDisposeMethod`, `CreateDisposableResource`,
//! `AddDisposableResource`, `Dispose`, `DisposeResources`) plus the
//! `DisposableStack`/`AsyncDisposableStack` constructors and prototypes that
//! are built directly on top of them. The `using` declaration compiler/VM
//! support (which reuses `add_disposable_resource`/`dispose_resources_sync`
//! below) lives in the compiler and in `vm/interpreter.rs`.

use super::*;

impl Vm {
    /// `GetDisposeMethod ( V, hint )`. `V` must already be known to be an
    /// Object; callers that also need to handle nullish `V` do so in
    /// `create_disposable_resource` below, mirroring the spec's own split
    /// between the two operations. The flag reports that an async-dispose
    /// hint fell back to the sync `@@dispose` method.
    pub(in super::super) fn get_dispose_method(
        &mut self,
        value: &Value,
        hint: DisposeHint,
    ) -> Result<(Value, bool), RuntimeError> {
        if hint == DisposeHint::Async {
            let method = self.get_method(value, &JsSymbol::well_known("asyncDispose").into())?;
            if method != Value::Undefined {
                return Ok((method, false));
            }
            // The spec wraps a sync `@@dispose` fallback in a fresh Abstract
            // Closure that calls it and resolves a promise with `undefined`:
            // the sync method's own return value is discarded, never awaited.
            // Calling the raw sync method with the same receiver produces the
            // same observable calls, since that wrapper is never itself
            // exposed to script; `sync_fallback` makes `Dispose` discard the
            // result.
            let method = self.get_method(value, &JsSymbol::well_known("dispose").into())?;
            return Ok((method, true));
        }
        Ok((
            self.get_method(value, &JsSymbol::well_known("dispose").into())?,
            false,
        ))
    }

    /// `CreateDisposableResource ( V, hint [, method ] )`.
    ///
    /// Returns `None` only for the sync-hint/nullish/no-explicit-method case
    /// that `AddDisposableResource` turns into "add nothing at all" -- every
    /// other outcome either returns a resource or throws.
    pub(in super::super) fn create_disposable_resource(
        &mut self,
        value: Value,
        hint: DisposeHint,
        method: Option<Value>,
    ) -> Result<Option<DisposableResource>, RuntimeError> {
        let mut sync_fallback = false;
        let method = match method {
            None => {
                if matches!(value, Value::Null | Value::Undefined) {
                    if hint == DisposeHint::Sync {
                        return Ok(None);
                    }
                    // `await using x = null` (or an AsyncDisposableStack
                    // resource explicitly added as nullish) still records a
                    // method-less resource: DisposeResources still performs
                    // an Await for it, per the async-dispose hint.
                    return Ok(Some(DisposableResource {
                        receiver: Value::Undefined,
                        argument: None,
                        method: None,
                        hint,
                        sync_fallback: false,
                    }));
                }
                if !matches!(value, Value::Object(_)) {
                    return Err(RuntimeError::TypeError(
                        "using declaration value must be an object, null, or undefined".into(),
                    ));
                }
                let (method, fallback) = self.get_dispose_method(&value, hint)?;
                if method == Value::Undefined {
                    return Err(RuntimeError::TypeError(
                        "resource has no Symbol.dispose/Symbol.asyncDispose method".into(),
                    ));
                }
                sync_fallback = fallback;
                method
            }
            Some(method) => {
                if !self.is_callable(&method)? {
                    return Err(RuntimeError::TypeError(
                        "dispose callback must be callable".into(),
                    ));
                }
                method
            }
        };
        Ok(Some(DisposableResource {
            receiver: value,
            argument: None,
            method: Some(method),
            hint,
            sync_fallback,
        }))
    }

    /// `AddDisposableResource ( disposeCapability, V, hint [, method ] )`.
    pub(in super::super) fn add_disposable_resource(
        &mut self,
        capability: &mut Vec<DisposableResource>,
        value: Value,
        hint: DisposeHint,
        method: Option<Value>,
    ) -> Result<(), RuntimeError> {
        if let Some(resource) = self.create_disposable_resource(value, hint, method)? {
            capability.push(resource);
        }
        Ok(())
    }

    /// `Dispose ( V, hint, method )` folded into one call.
    fn dispose_one(&mut self, resource: &DisposableResource) -> Result<Value, RuntimeError> {
        match &resource.method {
            None => Ok(Value::Undefined),
            Some(method) => {
                let args = resource.argument.clone().map_or_else(Vec::new, |v| vec![v]);
                self.call_native(method.clone(), resource.receiver.clone(), args, false)
            }
        }
    }

    /// `DisposeResources ( disposeCapability, completion )` for the
    /// `sync-dispose`-only case (`using` declarations and
    /// `DisposableStack.prototype.dispose`, which by construction never add
    /// an `async-dispose` resource to this list): calls every resource's
    /// dispose method synchronously and folds a disposal error into any
    /// already-pending error as a `SuppressedError`, in reverse declaration
    /// order, returning the final completion as an ordinary `Result`.
    ///
    /// `await using`/`AsyncDisposableStack.prototype.disposeAsync` need
    /// real per-resource `Await` interleaving instead and do not use this;
    /// see `Compiler::compile_async_dispose_finally` and
    /// `Vm::async_dispose_helper`.
    pub(in super::super) fn dispose_resources_sync(
        &mut self,
        resources: Vec<DisposableResource>,
        prior: Option<RuntimeError>,
    ) -> Result<(), RuntimeError> {
        // The resources were drained out of a rooted table (`self.disposables`
        // or a stack's side table), so the values they hold -- and every
        // pending error value -- are reachable only from this frame. Root
        // them until disposal finishes.
        let base = self.stack.len();
        self.root_resources(&resources);
        if let Some(error) = &prior {
            self.root_error(error);
        }
        let result = self.dispose_resources_rooted(resources, prior);
        self.stack.truncate(base);
        result
    }

    fn dispose_resources_rooted(
        &mut self,
        mut resources: Vec<DisposableResource>,
        prior: Option<RuntimeError>,
    ) -> Result<(), RuntimeError> {
        let mut completion = prior;
        while let Some(resource) = resources.pop() {
            if resource.method.is_none() {
                // A method-less `sync-dispose` resource (e.g. a `using`
                // binding whose value was null/undefined) is a pure no-op.
                // The spec's method-less `async-dispose` case still performs
                // `Await(undefined)` -- one guaranteed microtask tick -- which
                // this synchronous loop does not reproduce; see
                // `dispose_resources_sync`'s own doc comment.
                continue;
            }
            if let Err(new_error) = self.dispose_one(&resource) {
                if !new_error.is_catchable() {
                    return Err(new_error);
                }
                self.root_error(&new_error);
                completion = Some(match completion {
                    Some(prior_error) if prior_error.is_catchable() => {
                        let error_value = self.error_value(new_error)?;
                        self.stack.push(error_value.clone());
                        let suppressed_value = self.error_value(prior_error)?;
                        self.stack.push(suppressed_value.clone());
                        let suppressed =
                            self.make_suppressed_error(error_value, suppressed_value)?;
                        self.stack.push(suppressed.clone());
                        RuntimeError::Thrown(suppressed)
                    }
                    _ => new_error,
                });
            }
        }
        completion.map_or(Ok(()), Err)
    }

    /// Pushes every heap value held by `resources` onto the VM stack so a
    /// collection cannot reclaim it. The caller truncates the stack to the
    /// length it recorded before calling.
    fn root_resources(&mut self, resources: &[DisposableResource]) {
        for resource in resources {
            self.stack.push(resource.receiver.clone());
            if let Some(argument) = &resource.argument {
                self.stack.push(argument.clone());
            }
            if let Some(method) = &resource.method {
                self.stack.push(method.clone());
            }
        }
    }

    /// Roots the JS value carried by a thrown error (native errors that have
    /// not been converted to objects yet hold no heap reference).
    fn root_error(&mut self, error: &RuntimeError) {
        if let RuntimeError::Thrown(value) = error {
            self.stack.push(value.clone());
        }
    }

    /// Converts a drained resource list plus any prior pending error into
    /// the plain JS value `[hasError, pendingError, entries]` that
    /// `Compiler::compile_async_dispose_finally`'s synthesized `while`/
    /// `try`/`catch` loop destructures and iterates. `entries` is a real
    /// Array of `[receiver, method, hasArgument, argument, isAsync,
    /// syncFallback]` records, one per resource, in declaration order (the loop walks it
    /// back to front, i.e. reverse declaration order).
    pub(in super::super) fn build_async_dispose_state(
        &mut self,
        resources: Vec<DisposableResource>,
        prior: Option<RuntimeError>,
    ) -> Result<Value, RuntimeError> {
        // The drained resources are reachable only from this frame; root
        // them before `error_value` (which may allocate) and before any
        // entry array is built.
        let base = self.stack.len();
        self.root_resources(&resources);
        let result = (|| {
            let (has_error, pending_error) = match prior {
                None => (false, Value::Undefined),
                Some(error) => {
                    if !error.is_catchable() {
                        return Err(error);
                    }
                    (true, self.error_value(error)?)
                }
            };
            self.stack.push(pending_error.clone());
            let entries = self.entries_array_from_resources(resources)?;
            self.stack.push(entries.clone());
            self.array_from(vec![Value::Bool(has_error), pending_error, entries])
        })();
        self.stack.truncate(base);
        result
    }

    /// The `entries` half of `build_async_dispose_state`'s return value:
    /// a real Array of `[receiver, method, hasArgument, argument, isAsync,
    /// syncFallback]` records, one per resource, in declaration order.
    fn entries_array_from_resources(
        &mut self,
        resources: Vec<DisposableResource>,
    ) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        self.root_resources(&resources);
        let result = (|| {
            let mut entry_values = Vec::with_capacity(resources.len());
            for resource in resources {
                let entry = self.array_from(vec![
                    resource.receiver,
                    resource.method.unwrap_or(Value::Undefined),
                    Value::Bool(resource.argument.is_some()),
                    resource.argument.unwrap_or(Value::Undefined),
                    Value::Bool(resource.hint == DisposeHint::Async),
                    Value::Bool(resource.sync_fallback),
                ])?;
                // Keep every already-built entry array reachable while
                // building the rest: each is otherwise held only by this
                // Rust-local `Vec`, which the GC cannot see.
                self.stack.push(entry.clone());
                entry_values.push(entry);
            }
            self.array_from(entry_values)
        })();
        self.stack.truncate(base);
        result
    }

    /// Lazily compiles (once) and caches an internal async function
    /// implementing the exact same `Await`-interleaved disposal loop as
    /// `Compiler::compile_async_dispose_finally`'s synthesized bytecode --
    /// `(hasError, pendingError, entries) => { ...loop...; if (hasError)
    /// throw pendingError; }` -- reused by `AsyncDisposableStack.prototype.disposeAsync`.
    /// A *native* method cannot itself contain a bytecode `Await`, so
    /// calling this real (compiled, not hand-emitted) async function is how
    /// `disposeAsync` gets a real per-resource `Await` -- a dispose method's
    /// own returned promise is genuinely awaited, and one that later
    /// rejects becomes `disposeAsync`'s own rejection -- rather than a
    /// weaker synchronous approximation.
    fn async_dispose_helper(&mut self) -> Result<Value, RuntimeError> {
        if let Some(helper) = &self.async_dispose_helper {
            return Ok(helper.clone());
        }
        const SOURCE: &str = r#"(async function (hasError, pendingError, entries) {
            let i = entries.length;
            while (i > 0) {
                i = i - 1;
                let entry = entries[i];
                try {
                    if (entry[1] !== undefined) {
                        let result = entry[2] ? entry[1].call(entry[0], entry[3]) : entry[1].call(entry[0]);
                        if (entry[4]) { await (entry[5] ? undefined : result); }
                    } else if (entry[4]) {
                        await undefined;
                    }
                } catch (e) {
                    if (hasError) {
                        pendingError = new SuppressedError(e, pendingError);
                    } else {
                        pendingError = e;
                        hasError = true;
                    }
                }
            }
            if (hasError) throw pendingError;
        })"#;
        let program =
            crate::parse(SOURCE).map_err(|error| RuntimeError::SyntaxError(error.message))?;
        let code = crate::compiler::compile_eval(
            &program,
            &[],
            &[],
            &[],
            crate::compiler::EvalContext::default(),
            crate::compiler::CompileLimits::default(),
        )
        .map_err(|error| RuntimeError::SyntaxError(error.to_string()))?;
        let helper = self.execute_eval(&code, Vec::new(), !code.strict)?;
        self.async_dispose_helper = Some(helper.clone());
        Ok(helper)
    }

    /// Builds a `new SuppressedError(error, suppressed)` object (no message).
    fn make_suppressed_error(
        &mut self,
        error: Value,
        suppressed: Value,
    ) -> Result<Value, RuntimeError> {
        // `construct: false` selects the plain intrinsic prototype rather
        // than consulting `self.new_target`, which reflects an unrelated,
        // possibly-absent `new` call already in progress elsewhere on the
        // Rust call stack -- this construction happens purely internally,
        // with no corresponding JS `new SuppressedError(...)` expression.
        self.error_constructor(
            "SuppressedError",
            &[error, suppressed, Value::Undefined],
            false,
        )
    }

    /// Looks up this instance's `[[DisposeCapability]]` side table
    /// (`RequireInternalSlot`'s "has the internal slot" half); `is_async`
    /// selects which brand's table to check.
    fn dispose_capability_id(
        &self,
        receiver: &Value,
        is_async: bool,
    ) -> Result<ObjectId, RuntimeError> {
        let missing = || {
            RuntimeError::TypeError(
                "receiver is not a DisposableStack/AsyncDisposableStack instance".into(),
            )
        };
        let id = receiver.object_id().ok_or_else(missing)?;
        let present = if is_async {
            self.async_disposable_stacks.contains_key(&id)
        } else {
            self.disposable_stacks.contains_key(&id)
        };
        if present {
            Ok(id)
        } else {
            Err(missing())
        }
    }

    pub(in super::super) fn disposable_stack_prototype(
        &mut self,
        is_async: bool,
    ) -> Result<ObjectId, RuntimeError> {
        if let Some(prototype) = if is_async {
            self.async_disposable_stack_prototype
        } else {
            self.disposable_stack_prototype
        } {
            return Ok(prototype);
        }
        let object_prototype = self.object_prototype;
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
        let function_prototype = self.function_prototype()?;
        let tag = if is_async {
            "AsyncDisposableStack"
        } else {
            "DisposableStack"
        };
        let dispose_key = if is_async { "asyncDispose" } else { "dispose" };
        let dispose_name = if is_async { "disposeAsync" } else { "dispose" };
        self.stack.push(Value::Object(prototype));
        let result: Result<(), RuntimeError> = (|| {
            self.define_data(
                prototype,
                JsSymbol::well_known("toStringTag"),
                Value::String(tag.into()),
                false,
                false,
                true,
            )?;
            self.install_native(
                prototype,
                function_prototype,
                dispose_name,
                0,
                NativeFunction::DisposableStackDispose { is_async },
            )?;
            let dispose = self.heap.get(prototype, dispose_name)?;
            self.define_data(
                prototype,
                JsSymbol::well_known(dispose_key),
                dispose,
                true,
                false,
                true,
            )?;
            self.install_native(
                prototype,
                function_prototype,
                "use",
                1,
                NativeFunction::DisposableStackUse { is_async },
            )?;
            self.install_native(
                prototype,
                function_prototype,
                "adopt",
                2,
                NativeFunction::DisposableStackAdopt { is_async },
            )?;
            self.install_native(
                prototype,
                function_prototype,
                "defer",
                1,
                NativeFunction::DisposableStackDefer { is_async },
            )?;
            self.install_native(
                prototype,
                function_prototype,
                "move",
                0,
                NativeFunction::DisposableStackMove { is_async },
            )?;
            self.install_getter(
                prototype,
                function_prototype,
                "disposed".into(),
                "get disposed",
                NativeFunction::DisposableStackDisposedGetter { is_async },
            )?;
            Ok(())
        })();
        self.stack.pop();
        result?;
        if is_async {
            self.async_disposable_stack_prototype = Some(prototype);
        } else {
            self.disposable_stack_prototype = Some(prototype);
        }
        Ok(prototype)
    }

    pub(in super::super) fn disposable_stack_constructor(
        &mut self,
        is_async: bool,
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if !construct {
            let name = if is_async {
                "AsyncDisposableStack"
            } else {
                "DisposableStack"
            };
            return Err(RuntimeError::TypeError(format!(
                "{name} constructor must be called with new"
            )));
        }
        let default = self.disposable_stack_prototype(is_async)?;
        let prototype = self.constructor_prototype(default)?;
        let id = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        if is_async {
            self.async_disposable_stacks
                .insert(id, DisposeCapabilityState::default());
        } else {
            self.disposable_stacks
                .insert(id, DisposeCapabilityState::default());
        }
        Ok(Value::Object(id))
    }

    pub(in super::super) fn disposable_stack_use(
        &mut self,
        receiver: &Value,
        value: Value,
        is_async: bool,
    ) -> Result<Value, RuntimeError> {
        let id = self.dispose_capability_id(receiver, is_async)?;
        let disposed = if is_async {
            self.async_disposable_stacks[&id].disposed
        } else {
            self.disposable_stacks[&id].disposed
        };
        if disposed {
            return Err(RuntimeError::ReferenceError(
                "disposable stack has already been disposed".into(),
            ));
        }
        let hint = if is_async {
            DisposeHint::Async
        } else {
            DisposeHint::Sync
        };
        let resource = self.create_disposable_resource(value.clone(), hint, None)?;
        if let Some(resource) = resource {
            let state = if is_async {
                self.async_disposable_stacks.get_mut(&id)
            } else {
                self.disposable_stacks.get_mut(&id)
            }
            .expect("presence checked above");
            state.resources.push(resource);
        }
        Ok(value)
    }

    pub(in super::super) fn disposable_stack_adopt(
        &mut self,
        receiver: &Value,
        value: Value,
        on_dispose: Value,
        is_async: bool,
    ) -> Result<Value, RuntimeError> {
        let id = self.dispose_capability_id(receiver, is_async)?;
        let disposed = if is_async {
            self.async_disposable_stacks[&id].disposed
        } else {
            self.disposable_stacks[&id].disposed
        };
        if disposed {
            return Err(RuntimeError::ReferenceError(
                "disposable stack has already been disposed".into(),
            ));
        }
        if !self.is_callable(&on_dispose)? {
            return Err(RuntimeError::TypeError(
                "adopt requires a callable onDispose".into(),
            ));
        }
        let hint = if is_async {
            DisposeHint::Async
        } else {
            DisposeHint::Sync
        };
        let state = if is_async {
            self.async_disposable_stacks.get_mut(&id)
        } else {
            self.disposable_stacks.get_mut(&id)
        }
        .expect("presence checked above");
        state.resources.push(DisposableResource {
            receiver: Value::Undefined,
            argument: Some(value.clone()),
            method: Some(on_dispose),
            hint,
            sync_fallback: false,
        });
        Ok(value)
    }

    pub(in super::super) fn disposable_stack_defer(
        &mut self,
        receiver: &Value,
        on_dispose: Value,
        is_async: bool,
    ) -> Result<Value, RuntimeError> {
        let id = self.dispose_capability_id(receiver, is_async)?;
        let disposed = if is_async {
            self.async_disposable_stacks[&id].disposed
        } else {
            self.disposable_stacks[&id].disposed
        };
        if disposed {
            return Err(RuntimeError::ReferenceError(
                "disposable stack has already been disposed".into(),
            ));
        }
        if !self.is_callable(&on_dispose)? {
            return Err(RuntimeError::TypeError(
                "defer requires a callable onDispose".into(),
            ));
        }
        let hint = if is_async {
            DisposeHint::Async
        } else {
            DisposeHint::Sync
        };
        let state = if is_async {
            self.async_disposable_stacks.get_mut(&id)
        } else {
            self.disposable_stacks.get_mut(&id)
        }
        .expect("presence checked above");
        state.resources.push(DisposableResource {
            receiver: Value::Undefined,
            argument: None,
            method: Some(on_dispose),
            hint,
            sync_fallback: false,
        });
        Ok(Value::Undefined)
    }

    pub(in super::super) fn disposable_stack_move(
        &mut self,
        receiver: &Value,
        is_async: bool,
    ) -> Result<Value, RuntimeError> {
        let id = self.dispose_capability_id(receiver, is_async)?;
        let disposed = if is_async {
            self.async_disposable_stacks[&id].disposed
        } else {
            self.disposable_stacks[&id].disposed
        };
        if disposed {
            return Err(RuntimeError::ReferenceError(
                "disposable stack has already been disposed".into(),
            ));
        }
        // `move`'s `OrdinaryCreateFromConstructor(%DisposableStack%, ...)`
        // names the intrinsic constructor directly, not `new.target` (there
        // is none -- `move` is an ordinary method call), so the plain
        // intrinsic prototype is used rather than `constructor_prototype`.
        let prototype = self.disposable_stack_prototype(is_async)?;
        let new_id = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        let moved = if is_async {
            let mut state = self
                .async_disposable_stacks
                .remove(&id)
                .expect("presence checked above");
            let moved = std::mem::take(&mut state.resources);
            state.disposed = true;
            self.async_disposable_stacks.insert(id, state);
            moved
        } else {
            let mut state = self
                .disposable_stacks
                .remove(&id)
                .expect("presence checked above");
            let moved = std::mem::take(&mut state.resources);
            state.disposed = true;
            self.disposable_stacks.insert(id, state);
            moved
        };
        let new_state = DisposeCapabilityState {
            resources: moved,
            disposed: false,
        };
        if is_async {
            self.async_disposable_stacks.insert(new_id, new_state);
        } else {
            self.disposable_stacks.insert(new_id, new_state);
        }
        Ok(Value::Object(new_id))
    }

    pub(in super::super) fn disposable_stack_disposed(
        &self,
        receiver: &Value,
        is_async: bool,
    ) -> Result<Value, RuntimeError> {
        let id = self.dispose_capability_id(receiver, is_async)?;
        let disposed = if is_async {
            self.async_disposable_stacks[&id].disposed
        } else {
            self.disposable_stacks[&id].disposed
        };
        Ok(Value::Bool(disposed))
    }

    /// `DisposableStack.prototype.dispose`. `AsyncDisposableStack` has no
    /// synchronous `dispose`; its `disposeAsync` is handled separately.
    pub(in super::super) fn disposable_stack_dispose(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let id = self.dispose_capability_id(receiver, false)?;
        if self.disposable_stacks[&id].disposed {
            return Ok(Value::Undefined);
        }
        let resources = {
            let state = self
                .disposable_stacks
                .get_mut(&id)
                .expect("presence checked above");
            state.disposed = true;
            std::mem::take(&mut state.resources)
        };
        self.dispose_resources_sync(resources, None)?;
        Ok(Value::Undefined)
    }

    /// `AsyncDisposableStack.prototype.disposeAsync`. Always returns a
    /// genuine Promise -- including when `this` fails `RequireInternalSlot`,
    /// which rejects the returned promise rather than throwing
    /// synchronously (confirmed against `this-not-object-rejects.js`/
    /// `this-does-not-have-internal-asyncdisposablestate-rejects.js`).
    /// Disposal itself runs through `async_dispose_helper`, a real compiled
    /// async function, so this gets the exact same per-resource `Await`
    /// semantics as `await using` -- not a separate, weaker
    /// implementation.
    pub(in super::super) fn disposable_stack_dispose_async(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let id = match self.dispose_capability_id(receiver, true) {
            Ok(id) => id,
            Err(error) => return self.reject_with(error),
        };
        if self.async_disposable_stacks[&id].disposed {
            let promise_id = self.new_promise()?;
            self.resolve_promise(promise_id, Value::Undefined)?;
            return Ok(Value::Object(promise_id));
        }
        let resources = {
            let state = self
                .async_disposable_stacks
                .get_mut(&id)
                .expect("presence checked above");
            state.disposed = true;
            std::mem::take(&mut state.resources)
        };
        let base = self.stack.len();
        let result = (|| {
            let entries = match self.entries_array_from_resources(resources) {
                Ok(entries) => entries,
                Err(error) => return self.reject_with(error),
            };
            // `entries` and the helper are reachable only from this frame
            // until the call below roots them as its arguments/callee, and
            // compiling the helper on first use allocates.
            self.stack.push(entries.clone());
            let helper = self.async_dispose_helper()?;
            self.stack.push(helper.clone());
            self.call_native(
                helper,
                Value::Undefined,
                vec![Value::Bool(false), Value::Undefined, entries],
                false,
            )
        })();
        self.stack.truncate(base);
        result
    }

    /// A catchable `RuntimeError` becomes a rejected Promise (the caller's
    /// own return value); a host resource error still propagates raw.
    fn reject_with(&mut self, error: RuntimeError) -> Result<Value, RuntimeError> {
        if !error.is_catchable() {
            return Err(error);
        }
        let value = self.error_value(error)?;
        self.promise_reject(value)
    }
}
