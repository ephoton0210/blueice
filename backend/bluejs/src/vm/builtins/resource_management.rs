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
    /// between the two operations.
    pub(in super::super) fn get_dispose_method(
        &mut self,
        value: &Value,
        hint: DisposeHint,
    ) -> Result<Value, RuntimeError> {
        if hint == DisposeHint::Async {
            let method = self.get_method(value, &JsSymbol::well_known("asyncDispose").into())?;
            if method != Value::Undefined {
                return Ok(method);
            }
            // The spec wraps a sync `@@dispose` fallback in a fresh Abstract
            // Closure that calls it and returns its result for `Dispose` to
            // await. Calling the raw sync method with the same receiver
            // produces the same observable result, since that wrapper is
            // never itself exposed to script.
            return self.get_method(value, &JsSymbol::well_known("dispose").into());
        }
        self.get_method(value, &JsSymbol::well_known("dispose").into())
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
                    }));
                }
                if !matches!(value, Value::Object(_)) {
                    return Err(RuntimeError::TypeError(
                        "using declaration value must be an object, null, or undefined".into(),
                    ));
                }
                let method = self.get_dispose_method(&value, hint)?;
                if method == Value::Undefined {
                    return Err(RuntimeError::TypeError(
                        "resource has no Symbol.dispose/Symbol.asyncDispose method".into(),
                    ));
                }
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

    /// `DisposeResources ( disposeCapability, completion )`, calling every
    /// resource's dispose method synchronously and folding a disposal error
    /// into any already-pending error as a `SuppressedError`. Disposes in
    /// reverse declaration order and returns the final completion as an
    /// ordinary `Result`.
    ///
    /// For a `sync-dispose` resource this is exactly `DisposeResources`.
    /// `disposable_stack_dispose_async` also uses it for `async-dispose`
    /// resources as a deliberate simplification: a dispose method's *own*
    /// returned promise is never awaited before moving on to the next
    /// resource (unlike the spec's `Await(Call(method, V))` per entry), so a
    /// genuinely-async dispose method that rejects after this call returns
    /// is not observed. Every other outcome -- call order, thrown errors,
    /// `SuppressedError` chaining -- matches an all-synchronous capability.
    pub(in super::super) fn dispose_resources_sync(
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
                completion = Some(match completion {
                    Some(prior_error) if prior_error.is_catchable() => {
                        let error_value = self.error_value(new_error)?;
                        let base = self.stack.len();
                        self.stack.push(error_value.clone());
                        let suppressed_value = self.error_value(prior_error);
                        self.stack.truncate(base);
                        let suppressed_value = suppressed_value?;
                        let suppressed =
                            self.make_suppressed_error(error_value, suppressed_value)?;
                        RuntimeError::Thrown(suppressed)
                    }
                    _ => new_error,
                });
            }
        }
        completion.map_or(Ok(()), Err)
    }

    /// Converts a drained resource list plus any prior pending error into
    /// the plain JS value `[hasError, pendingError, entries]` that
    /// `Compiler::compile_async_dispose_finally`'s synthesized `while`/
    /// `try`/`catch` loop destructures and iterates. `entries` is a real
    /// Array of `[receiver, method, hasArgument, argument, isAsync]`
    /// records, one per resource, in declaration order (the loop walks it
    /// back to front, i.e. reverse declaration order).
    pub(in super::super) fn build_async_dispose_state(
        &mut self,
        resources: Vec<DisposableResource>,
        prior: Option<RuntimeError>,
    ) -> Result<Value, RuntimeError> {
        let (has_error, pending_error) = match prior {
            None => (false, Value::Undefined),
            Some(error) => {
                if !error.is_catchable() {
                    return Err(error);
                }
                (true, self.error_value(error)?)
            }
        };
        let base = self.stack.len();
        self.stack.push(pending_error.clone());
        let result = (|| {
            let mut entry_values = Vec::with_capacity(resources.len());
            for resource in resources {
                let entry = self.array_from(vec![
                    resource.receiver,
                    resource.method.unwrap_or(Value::Undefined),
                    Value::Bool(resource.argument.is_some()),
                    resource.argument.unwrap_or(Value::Undefined),
                    Value::Bool(resource.hint == DisposeHint::Async),
                ])?;
                // Keep every already-built entry array reachable while
                // building the rest: each is otherwise held only by this
                // Rust-local `Vec`, which the GC cannot see.
                self.stack.push(entry.clone());
                entry_values.push(entry);
            }
            let entries = self.array_from(entry_values)?;
            self.stack.push(entries.clone());
            let outcome = self.array_from(vec![Value::Bool(has_error), pending_error, entries]);
            outcome
        })();
        self.stack.truncate(base);
        result
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
    /// genuine Promise; see `dispose_resources_sync`'s doc comment for the
    /// one deliberate simplification this takes versus the full spec
    /// algorithm (per-resource `Await` chaining).
    pub(in super::super) fn disposable_stack_dispose_async(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let id = self.dispose_capability_id(receiver, true)?;
        let promise_id = self.new_promise()?;
        if self.async_disposable_stacks[&id].disposed {
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
        match self.dispose_resources_sync(resources, None) {
            Ok(()) => self.resolve_promise(promise_id, Value::Undefined)?,
            Err(error) => {
                if !error.is_catchable() {
                    return Err(error);
                }
                let value = self.error_value(error)?;
                self.settle_promise(promise_id, PromiseStatus::Rejected(value))?;
            }
        }
        Ok(Value::Object(promise_id))
    }
}
