// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The Promise abstract operations and the `Promise` constructor and
//! prototype methods (ECMA-262 27.2): resolving functions with their shared
//! [[AlreadyResolved]] flag, PromiseCapability records built through
//! NewPromiseCapability(C) for user constructors, ResolvePromise with real
//! thenable jobs, PerformPromiseThen, and species-aware `then`, `catch` and
//! `finally`. The static combinators live in `promise_combinators.rs`.
//!
//! Closure state that the specification keeps in Abstract Closure captures
//! (the [[AlreadyResolved]] record, `finally`'s `onFinally`, a value thunk's
//! value) lives in small null-prototype heap objects that the native function
//! variants reference, so the collector traces it and no JavaScript can see it.

use super::*;

impl Vm {
    pub(in super::super) fn new_promise(&mut self) -> Result<ObjectId, RuntimeError> {
        // `promise_prototype()` alone builds the prototype object (`then`/
        // `catch`/`finally`/`@@toStringTag`) but not its "constructor" link
        // back to the `Promise` function -- that property is only added
        // when the `Promise` *global* itself is materialized (`globals.rs`).
        // An internally created promise (dynamic import, `await`,
        // `Promise.all`/`race`/etc., Atomics.waitAsync, ...) must still
        // observe `promise.constructor === Promise` even when nothing has
        // referenced the bare `Promise` identifier yet. `self.global` is
        // idempotent/cached, so this is a cheap no-op once materialized
        // (including when called *from* that very materialization, via
        // `promise_prototype()`'s own cache check -- no circularity).
        self.global("Promise")?;
        let prototype = self.promise_prototype()?;
        self.new_promise_with_prototype(prototype)
    }

    fn new_promise_with_prototype(
        &mut self,
        prototype: ObjectId,
    ) -> Result<ObjectId, RuntimeError> {
        let promise = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        self.promises.insert(
            promise,
            PromiseRecord {
                status: PromiseStatus::Pending,
                reactions: Vec::new(),
            },
        );
        Ok(promise)
    }

    /// A built-in function object with no name: `length`, then `name` (both
    /// non-writable, non-enumerable, configurable), in the order CreateBuiltin-
    /// Function defines them.
    pub(super) fn promise_native_function(
        &mut self,
        function: NativeFunction,
        length: u32,
    ) -> Result<Value, RuntimeError> {
        let prototype = self.function_prototype()?;
        let id = self.with_roots(|heap| heap.alloc_native_function(function, "", prototype))?;
        self.stack.push(Value::Object(id));
        let result = (|| {
            self.define_data(
                id,
                "length",
                Value::Number(f64::from(length)),
                false,
                false,
                true,
            )?;
            self.define_data(id, "name", Value::String("".into()), false, false, true)?;
            Ok(Value::Object(id))
        })();
        self.stack.pop();
        result
    }

    /// A fresh null-prototype heap record for closure state; the caller keeps
    /// it rooted while it fills it in.
    pub(super) fn promise_state(&mut self) -> Result<ObjectId, RuntimeError> {
        self.with_roots(|heap| heap.alloc_object(None))
    }

    pub(super) fn promise_state_get(
        &self,
        state: ObjectId,
        key: &str,
    ) -> Result<Value, RuntimeError> {
        Ok(self.heap.get_own(state, key)?.unwrap_or(Value::Undefined))
    }

    pub(super) fn promise_state_set(
        &mut self,
        state: ObjectId,
        key: impl Into<PropertyName>,
        value: Value,
    ) -> Result<(), RuntimeError> {
        self.with_roots(|heap| heap.set(state, key, value))
    }

    /// CreateResolvingFunctions ( promise ): the resolve and reject functions
    /// share one [[AlreadyResolved]] record, so the first call to either
    /// disables both. Both results are unrooted on return; the caller must
    /// push them before allocating again.
    pub(in super::super) fn promise_resolving_functions(
        &mut self,
        promise: ObjectId,
    ) -> Result<(Value, Value), RuntimeError> {
        let base = self.stack.len();
        let result = (|| {
            let state = self.promise_state()?;
            self.stack.push(Value::Object(state));
            let resolve = self.promise_native_function(
                NativeFunction::PromiseResolvingFunction {
                    promise,
                    fulfill: true,
                    state,
                },
                1,
            )?;
            self.stack.push(resolve.clone());
            let reject = self.promise_native_function(
                NativeFunction::PromiseResolvingFunction {
                    promise,
                    fulfill: false,
                    state,
                },
                1,
            )?;
            Ok((resolve, reject))
        })();
        self.stack.truncate(base);
        result
    }

    /// Reads and sets the shared [[AlreadyResolved]] flag; true when a
    /// resolving function of the pair already ran.
    pub(in super::super) fn promise_already_resolved(
        &mut self,
        state: ObjectId,
    ) -> Result<bool, RuntimeError> {
        if self.promise_state_get(state, "resolved")? == Value::Bool(true) {
            return Ok(true);
        }
        self.promise_state_set(state, "resolved", Value::Bool(true))?;
        Ok(false)
    }

    /// NewPromiseCapability ( C ). For `%Promise%` itself the executor
    /// protocol is unobservable, so the promise and its resolving functions
    /// are created directly; any other constructor is really constructed with
    /// an executor and must hand back two callables.
    pub(in super::super) fn new_promise_capability(
        &mut self,
        constructor: &Value,
    ) -> Result<PromiseCapability, RuntimeError> {
        if !self.is_constructor(constructor)? {
            return Err(RuntimeError::TypeError(
                "Promise constructor must be a constructor".into(),
            ));
        }
        let intrinsic = self.global("Promise")?;
        let base = self.stack.len();
        let result = (|| {
            if *constructor == intrinsic {
                let promise = self.new_promise()?;
                self.stack.push(Value::Object(promise));
                let (resolve, reject) = self.promise_resolving_functions(promise)?;
                return Ok(PromiseCapability {
                    promise: Value::Object(promise),
                    resolve,
                    reject,
                });
            }
            let storage = self.promise_state()?;
            self.stack.push(Value::Object(storage));
            let executor = self.promise_native_function(
                NativeFunction::PromiseCapabilityExecutor { storage },
                2,
            )?;
            self.stack.push(executor.clone());
            let promise =
                self.call_native(constructor.clone(), Value::Undefined, vec![executor], true)?;
            self.stack.push(promise.clone());
            let Value::Object(_) = promise else {
                return Err(RuntimeError::TypeError(
                    "Promise constructor must return an object".into(),
                ));
            };
            let resolve = self.promise_state_get(storage, "resolve")?;
            let reject = self.promise_state_get(storage, "reject")?;
            if !self.is_callable(&resolve)? || !self.is_callable(&reject)? {
                return Err(RuntimeError::TypeError(
                    "Promise constructor did not provide resolving functions".into(),
                ));
            }
            Ok(PromiseCapability {
                promise,
                resolve,
                reject,
            })
        })();
        self.stack.truncate(base);
        result
    }

    /// The GetCapabilitiesExecutor function of NewPromiseCapability.
    pub(in super::super) fn promise_capability_executor(
        &mut self,
        storage: ObjectId,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        if self.promise_state_get(storage, "resolve")? != Value::Undefined
            || self.promise_state_get(storage, "reject")? != Value::Undefined
        {
            return Err(RuntimeError::TypeError(
                "Promise capability executor was already called".into(),
            ));
        }
        self.promise_state_set(storage, "resolve", native::argument(args, 0).clone())?;
        self.promise_state_set(storage, "reject", native::argument(args, 1).clone())?;
        Ok(Value::Undefined)
    }

    /// PromiseResolve ( C, x ).
    pub(in super::super) fn promise_resolve_constructor(
        &mut self,
        constructor: &Value,
        value: Value,
    ) -> Result<Value, RuntimeError> {
        if value
            .object_id()
            .is_some_and(|promise| self.promises.contains_key(&promise))
            && self.get_property(&value, &"constructor".into())? == *constructor
        {
            return Ok(value);
        }
        let base = self.stack.len();
        self.stack.extend([constructor.clone(), value.clone()]);
        let result = (|| {
            let capability = self.new_promise_capability(constructor)?;
            self.stack.extend([
                capability.promise.clone(),
                capability.resolve.clone(),
                capability.reject.clone(),
            ]);
            self.call_native(capability.resolve, Value::Undefined, vec![value], false)?;
            Ok(capability.promise)
        })();
        self.stack.truncate(base);
        result
    }

    /// `Promise.resolve ( x )`.
    pub(in super::super) fn promise_resolve_static(
        &mut self,
        constructor: &Value,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        if !matches!(constructor, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "Promise.resolve requires an object receiver".into(),
            ));
        }
        self.promise_resolve_constructor(constructor, value.clone())
    }

    /// `Promise.reject ( r )`.
    pub(in super::super) fn promise_reject_static(
        &mut self,
        constructor: &Value,
        reason: &Value,
    ) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        self.stack.extend([constructor.clone(), reason.clone()]);
        let result = (|| {
            let capability = self.new_promise_capability(constructor)?;
            self.stack.extend([
                capability.promise.clone(),
                capability.resolve.clone(),
                capability.reject.clone(),
            ]);
            self.call_native(
                capability.reject,
                Value::Undefined,
                vec![reason.clone()],
                false,
            )?;
            Ok(capability.promise)
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn promise_constructor(
        &mut self,
        executor: Value,
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if !construct {
            return Err(RuntimeError::TypeError(
                "Promise constructor must be called with new".into(),
            ));
        }
        if !self.is_callable(&executor)? {
            return Err(RuntimeError::TypeError(
                "Promise resolver is not a function".into(),
            ));
        }
        // PromiseCreate uses OrdinaryCreateFromConstructor, so a distinct
        // newTarget can observe its `prototype` getter (and a revoked Proxy
        // there must throw) before the promise record is allocated.
        let default_prototype = self.promise_prototype()?;
        let prototype = self.constructor_prototype(default_prototype)?;
        let promise = self.new_promise_with_prototype(prototype)?;
        // The capability and both resolving functions must survive further
        // allocations. In particular, creating `reject` can collect the
        // freshly-created `resolve` function before the executor observes it.
        let base = self.stack.len();
        self.stack.push(Value::Object(promise));
        let result = (|| {
            let (resolve, reject) = self.promise_resolving_functions(promise)?;
            self.stack.extend([resolve.clone(), reject.clone()]);
            if let Err(error) = self.call_native(
                executor,
                Value::Undefined,
                vec![resolve, reject.clone()],
                false,
            ) {
                // An abrupt executor rejects through the reject function, so
                // an earlier resolve() call still wins.
                let reason = self.error_value(error)?;
                self.stack.push(reason.clone());
                self.call_native(reject, Value::Undefined, vec![reason], false)?;
            }
            Ok(Value::Object(promise))
        })();
        self.stack.truncate(base);
        result
    }

    /// `Promise.withResolvers ( )`.
    pub(in super::super) fn promise_with_resolvers(
        &mut self,
        constructor: &Value,
    ) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        self.stack.push(constructor.clone());
        let result = (|| {
            let capability = self.new_promise_capability(constructor)?;
            self.stack.extend([
                capability.promise.clone(),
                capability.resolve.clone(),
                capability.reject.clone(),
            ]);
            let object_prototype = self.object_prototype;
            let record = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
            self.stack.push(Value::Object(record));
            self.define_data(record, "promise", capability.promise, true, true, true)?;
            self.define_data(record, "resolve", capability.resolve, true, true, true)?;
            self.define_data(record, "reject", capability.reject, true, true, true)?;
            Ok(Value::Object(record))
        })();
        self.stack.truncate(base);
        result
    }

    /// `Promise.try ( callbackfn, ...args )`: the callback runs first; a normal
    /// result goes through PromiseResolve(C, value), so a promise of the
    /// receiver's own constructor is returned as is instead of being wrapped,
    /// and an abrupt completion rejects a new capability of C.
    pub(in super::super) fn promise_try(
        &mut self,
        constructor: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        if !matches!(constructor, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "Promise.try requires an object receiver".into(),
            ));
        }
        let base = self.stack.len();
        self.stack.push(constructor.clone());
        self.stack.extend(args.iter().cloned());
        let result = (|| {
            let callback = native::argument(args, 0).clone();
            let rest = args.iter().skip(1).cloned().collect();
            match self.call_native(callback, Value::Undefined, rest, false) {
                Ok(value) => {
                    self.stack.push(value.clone());
                    self.promise_resolve_constructor(constructor, value)
                }
                Err(error) => {
                    let reason = self.error_value(error)?;
                    self.stack.push(reason.clone());
                    let capability = self.new_promise_capability(constructor)?;
                    self.stack.extend([
                        capability.promise.clone(),
                        capability.resolve.clone(),
                        capability.reject.clone(),
                    ]);
                    self.call_native(capability.reject, Value::Undefined, vec![reason], false)?;
                    Ok(capability.promise)
                }
            }
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn settle_promise(
        &mut self,
        promise: ObjectId,
        status: PromiseStatus,
    ) -> Result<(), RuntimeError> {
        let record = self
            .promises
            .get_mut(&promise)
            .ok_or(RuntimeError::TypeError("invalid Promise receiver".into()))?;
        if !matches!(record.status, PromiseStatus::Pending) {
            return Ok(());
        }
        let fulfilled = matches!(status, PromiseStatus::Fulfilled(_));
        let value = match &status {
            PromiseStatus::Fulfilled(value) | PromiseStatus::Rejected(value) => value.clone(),
            PromiseStatus::Pending => unreachable!("Promise settlement is final"),
        };
        let reactions = std::mem::take(&mut record.reactions);
        record.status = status;
        self.promise_jobs
            .extend(reactions.into_iter().map(|reaction| match reaction {
                PromiseReaction::Then(reaction) => PromiseJob::Reaction {
                    target: reaction.target,
                    handler: if fulfilled {
                        reaction.on_fulfilled
                    } else {
                        reaction.on_rejected
                    },
                    value: value.clone(),
                    fulfilled,
                },
                PromiseReaction::ModuleAwait { continuation } => PromiseJob::ModuleAwait {
                    continuation,
                    value: value.clone(),
                    fulfilled,
                },
                PromiseReaction::AsyncAwait { continuation } => PromiseJob::AsyncAwait {
                    continuation,
                    value: value.clone(),
                    fulfilled,
                },
                PromiseReaction::AsyncGeneratorYield {
                    generator,
                    target,
                    result,
                } => PromiseJob::AsyncGeneratorYield {
                    generator,
                    target,
                    result,
                    value: value.clone(),
                    fulfilled,
                },
                PromiseReaction::AsyncGeneratorDelegate {
                    generator,
                    target,
                    kind,
                } => PromiseJob::AsyncGeneratorDelegate {
                    generator,
                    target,
                    kind,
                    value: value.clone(),
                    fulfilled,
                },
            }));
        Ok(())
    }

    /// The body of a Promise Resolve Function after its [[AlreadyResolved]]
    /// check (steps 7-16): a thenable is adopted through a queued
    /// NewPromiseResolveThenableJob, so `then` is looked up here but called
    /// only on a later turn, whatever kind of object the resolution is.
    pub(in super::super) fn resolve_promise(
        &mut self,
        promise: ObjectId,
        resolution: Value,
    ) -> Result<(), RuntimeError> {
        if resolution.object_id() == Some(promise) {
            let error = self.error_object("TypeError", "Promise resolved with itself".into())?;
            return self.settle_promise(promise, PromiseStatus::Rejected(error));
        }
        if !matches!(resolution, Value::Object(_)) {
            return self.settle_promise(promise, PromiseStatus::Fulfilled(resolution));
        }
        let base = self.stack.len();
        self.stack.push(resolution.clone());
        let then = self.get_property(&resolution, &"then".into());
        self.stack.truncate(base);
        let then = match then {
            Ok(then) => then,
            Err(error) => {
                let error = self.error_value(error)?;
                return self.settle_promise(promise, PromiseStatus::Rejected(error));
            }
        };
        if !self.is_callable(&then)? {
            return self.settle_promise(promise, PromiseStatus::Fulfilled(resolution));
        }
        self.promise_jobs.push_back(PromiseJob::Thenable {
            target: promise,
            thenable: resolution,
            then,
        });
        Ok(())
    }

    /// PerformPromiseThen ( promise, onFulfilled, onRejected, resultCapability ).
    pub(in super::super) fn perform_promise_then(
        &mut self,
        promise: ObjectId,
        on_fulfilled: Value,
        on_rejected: Value,
        target: ReactionTarget,
    ) -> Result<(), RuntimeError> {
        let reaction = PromiseThenReaction {
            target,
            on_fulfilled,
            on_rejected,
        };
        let record = self
            .promises
            .get_mut(&promise)
            .ok_or(RuntimeError::TypeError("invalid Promise receiver".into()))?;
        let (fulfilled, value) = match &record.status {
            PromiseStatus::Pending => {
                record.reactions.push(PromiseReaction::Then(reaction));
                return Ok(());
            }
            PromiseStatus::Fulfilled(value) => (true, value.clone()),
            PromiseStatus::Rejected(value) => (false, value.clone()),
        };
        self.promise_jobs.push_back(PromiseJob::Reaction {
            target: reaction.target,
            handler: if fulfilled {
                reaction.on_fulfilled
            } else {
                reaction.on_rejected
            },
            value,
            fulfilled,
        });
        Ok(())
    }

    /// PerformPromiseThen for VM-internal callers: `receiver` is a promise the
    /// VM already knows, no species is consulted, and the derived promise is a
    /// plain `%Promise%`.
    pub(in super::super) fn promise_then(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let promise = receiver
            .object_id()
            .filter(|id| self.promises.contains_key(id))
            .ok_or(RuntimeError::TypeError(
                "Promise.prototype.then receiver".into(),
            ))?;
        let target = self.new_promise()?;
        self.perform_promise_then(
            promise,
            native::argument(args, 0).clone(),
            native::argument(args, 1).clone(),
            ReactionTarget::Native(target),
        )?;
        Ok(Value::Object(target))
    }

    /// SpeciesConstructor ( promise, %Promise% ).
    fn promise_species_constructor(&mut self, promise: &Value) -> Result<Value, RuntimeError> {
        let default = self.global("Promise")?;
        let constructor = self.get_property(promise, &"constructor".into())?;
        if constructor == Value::Undefined {
            return Ok(default);
        }
        if !matches!(constructor, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "Promise constructor must be an object".into(),
            ));
        }
        let species = self.get_property(&constructor, &JsSymbol::well_known("species").into())?;
        if matches!(species, Value::Undefined | Value::Null) {
            return Ok(default);
        }
        if !self.is_constructor(&species)? {
            return Err(RuntimeError::TypeError(
                "Promise species must be a constructor".into(),
            ));
        }
        Ok(species)
    }

    /// `Promise.prototype.then ( onFulfilled, onRejected )`.
    pub(in super::super) fn promise_prototype_then(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let promise = receiver
            .object_id()
            .filter(|id| self.promises.contains_key(id))
            .ok_or(RuntimeError::TypeError(
                "Promise.prototype.then receiver".into(),
            ))?;
        let base = self.stack.len();
        self.stack.extend(args.iter().cloned());
        let result = (|| {
            let constructor = self.promise_species_constructor(receiver)?;
            self.stack.push(constructor.clone());
            let (target, derived) = if constructor == self.global("Promise")? {
                let derived = self.new_promise()?;
                (ReactionTarget::Native(derived), Value::Object(derived))
            } else {
                let capability = self.new_promise_capability(&constructor)?;
                let derived = capability.promise.clone();
                (ReactionTarget::Capability(capability), derived)
            };
            self.stack.push(derived.clone());
            self.perform_promise_then(
                promise,
                native::argument(args, 0).clone(),
                native::argument(args, 1).clone(),
                target,
            )?;
            Ok(derived)
        })();
        self.stack.truncate(base);
        result
    }

    /// Invoke ( V, P, args ).
    pub(super) fn invoke(
        &mut self,
        value: &Value,
        name: &str,
        args: Vec<Value>,
    ) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        self.stack.push(value.clone());
        self.stack.extend(args.iter().cloned());
        let result = self
            .get_property(value, &name.into())
            .and_then(|method| self.call_native(method, value.clone(), args, false));
        self.stack.truncate(base);
        result
    }

    /// `Promise.prototype.catch ( onRejected )`.
    pub(in super::super) fn promise_catch(
        &mut self,
        receiver: &Value,
        on_rejected: &Value,
    ) -> Result<Value, RuntimeError> {
        self.invoke(
            receiver,
            "then",
            vec![Value::Undefined, on_rejected.clone()],
        )
    }

    /// `Promise.prototype.finally ( onFinally )`.
    pub(in super::super) fn promise_finally(
        &mut self,
        receiver: &Value,
        on_finally: &Value,
    ) -> Result<Value, RuntimeError> {
        if !matches!(receiver, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "Promise.prototype.finally requires an object receiver".into(),
            ));
        }
        let base = self.stack.len();
        self.stack.extend([receiver.clone(), on_finally.clone()]);
        let result = (|| {
            let constructor = self.promise_species_constructor(receiver)?;
            self.stack.push(constructor.clone());
            let (then_finally, catch_finally) = if self.is_callable(on_finally)? {
                let state = self.promise_state()?;
                self.stack.push(Value::Object(state));
                self.promise_state_set(state, "onFinally", on_finally.clone())?;
                self.promise_state_set(state, "constructor", constructor)?;
                let then_finally = self.promise_native_function(
                    NativeFunction::PromiseFinallyFunction {
                        state,
                        catch: false,
                    },
                    1,
                )?;
                self.stack.push(then_finally.clone());
                let catch_finally = self.promise_native_function(
                    NativeFunction::PromiseFinallyFunction { state, catch: true },
                    1,
                )?;
                (then_finally, catch_finally)
            } else {
                (on_finally.clone(), on_finally.clone())
            };
            self.stack
                .extend([then_finally.clone(), catch_finally.clone()]);
            self.invoke(receiver, "then", vec![then_finally, catch_finally])
        })();
        self.stack.truncate(base);
        result
    }

    /// A `thenFinally` / `catchFinally` function of `finally`: run `onFinally`,
    /// resolve its result with the species constructor, and continue with a
    /// thunk that restores the original value or reason.
    pub(in super::super) fn promise_finally_function(
        &mut self,
        state: ObjectId,
        catch: bool,
        argument: &Value,
    ) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        self.stack.push(argument.clone());
        let result = (|| {
            let on_finally = self.promise_state_get(state, "onFinally")?;
            let constructor = self.promise_state_get(state, "constructor")?;
            let outcome = self.call_native(on_finally, Value::Undefined, Vec::new(), false)?;
            self.stack.push(outcome.clone());
            let promise = self.promise_resolve_constructor(&constructor, outcome)?;
            self.stack.push(promise.clone());
            let thunk_state = self.promise_state()?;
            self.stack.push(Value::Object(thunk_state));
            self.promise_state_set(thunk_state, "value", argument.clone())?;
            let thunk = self.promise_native_function(
                NativeFunction::PromiseValueThunk {
                    state: thunk_state,
                    thrower: catch,
                },
                0,
            )?;
            self.stack.push(thunk.clone());
            self.invoke(&promise, "then", vec![thunk])
        })();
        self.stack.truncate(base);
        result
    }

    /// The value thunk (returns the value) or thrower (throws the reason) that
    /// `finally` chains after `onFinally`'s result.
    pub(in super::super) fn promise_value_thunk(
        &mut self,
        state: ObjectId,
        thrower: bool,
    ) -> Result<Value, RuntimeError> {
        let value = self.promise_state_get(state, "value")?;
        if thrower {
            Err(RuntimeError::Thrown(value))
        } else {
            Ok(value)
        }
    }

    /// Implements the settled-value portion of Await. A pending promise needs
    /// a saved interpreter continuation, which remains a separate boundary.
    pub(in super::super) fn await_value(&self, value: Value) -> Result<Value, RuntimeError> {
        let Some(promise) = value.object_id() else {
            return Ok(value);
        };
        let Some(record) = self.promises.get(&promise) else {
            return Ok(value);
        };
        match &record.status {
            PromiseStatus::Pending => Err(RuntimeError::Unsupported("pending await continuation")),
            PromiseStatus::Fulfilled(value) => Ok(value.clone()),
            PromiseStatus::Rejected(value) => Err(RuntimeError::Thrown(value.clone())),
        }
    }

    /// PromiseResolve ( %Promise%, value ) for VM-internal callers (Await,
    /// async iteration): a promise whose `constructor` is `%Promise%` is
    /// returned as is, anything else is resolved into a new promise.
    pub(in super::super) fn promise_resolve(
        &mut self,
        value: Value,
    ) -> Result<Value, RuntimeError> {
        // PromiseResolve(%Promise%, value) may return a native Promise only
        // after it observes `value.constructor`. That lookup is observable
        // (and may throw), including through AsyncFromSyncIteratorContinuation.
        // Keep `value` rooted while lazy intrinsic initialization or thenable
        // lookup can allocate.
        let base = self.stack.len();
        self.stack.push(value.clone());
        let outcome = (|| {
            let constructor = self.global("Promise")?;
            if value
                .object_id()
                .is_some_and(|promise| self.promises.contains_key(&promise))
                && self.get_property(&value, &"constructor".into())? == constructor
            {
                return Ok(value);
            }
            let promise = self.new_promise()?;
            self.stack.push(Value::Object(promise));
            self.resolve_promise(promise, value)?;
            Ok(Value::Object(promise))
        })();
        self.stack.truncate(base);
        outcome
    }

    pub(in super::super) fn promise_reject(&mut self, value: Value) -> Result<Value, RuntimeError> {
        // Allocating the promise can collect, and `value` (typically a freshly
        // built error object) is not stored anywhere until it is settled.
        let base = self.stack.len();
        self.stack.push(value.clone());
        let promise = self.new_promise();
        let settled = promise.and_then(|promise| {
            self.settle_promise(promise, PromiseStatus::Rejected(value))?;
            Ok(Value::Object(promise))
        });
        self.stack.truncate(base);
        settled
    }

    /// A `{ status, value | reason }` record as `Promise.allSettled` reports it.
    pub(super) fn promise_settlement_record(
        &mut self,
        fulfilled: bool,
        value: Value,
    ) -> Result<Value, RuntimeError> {
        let prototype = self.object_prototype;
        let record = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        let base = self.stack.len();
        self.stack.push(Value::Object(record));
        self.stack.push(value.clone());
        let result = (|| {
            self.define_data(
                record,
                "status",
                Value::String(if fulfilled { "fulfilled" } else { "rejected" }.into()),
                true,
                true,
                true,
            )?;
            self.define_data(
                record,
                if fulfilled { "value" } else { "reason" },
                value,
                true,
                true,
                true,
            )?;
            Ok(Value::Object(record))
        })();
        self.stack.truncate(base);
        result
    }
}
