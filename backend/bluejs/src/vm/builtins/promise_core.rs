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

#[cfg(any(test, coverage))]
#[path = "../../../tests/fixtures/thunk_origin_storage.rs"]
mod thunk_origin_storage_tests;

struct PreparedThunkOrigin {
    origin: ThrowOrigin,
    growth: promise_queue_storage::PreparedMap<ObjectId, ThrowOrigin>,
    charge: Option<host_scheduling::SharedSchedulingCharge>,
}
impl PreparedThunkOrigin {
    fn publish(self, vm: &mut Vm, state: ObjectId) {
        let Self {
            origin,
            growth,
            charge,
        } = self;
        growth.commit_after_pruning(&mut vm.promise_thunk_origins);
        vm.promise_thunk_origins.insert(state, origin);
        drop(charge);
        vm.reconcile_scheduling_ownership();
    }
}

impl Vm {
    fn prepare_thunk_origin(&mut self) -> Result<PreparedThunkOrigin, RuntimeError> {
        let (origin, charge) = self.prepare_current_rejection_origin()?;
        let growth = promise_queue_storage::PreparedMap::for_insert(
            &self.promise_thunk_origins,
            self.host_scheduling.as_ref(),
        )?;
        Ok(PreparedThunkOrigin {
            origin,
            growth,
            charge,
        })
    }

    fn allocate_finally_thunk_state(
        &mut self,
        argument: &Value,
        catch: bool,
    ) -> Result<ObjectId, RuntimeError> {
        let before = self.heap.stats();
        let result = self.allocate_finally_thunk_state_inner(argument, catch);
        // All candidates have committed or dropped before deferred GC cleanup.
        self.finish_promise_record_attempt(before);
        result
    }

    fn allocate_finally_thunk_state_inner(
        &mut self,
        argument: &Value,
        catch: bool,
    ) -> Result<ObjectId, RuntimeError> {
        let mut prepared = if catch {
            Some(self.prepare_thunk_origin()?)
        } else {
            None
        };
        let previous = self.preserve_promise_table_capacity;
        self.preserve_promise_table_capacity = previous || prepared.is_some();
        let base = self.stack.len();
        self.stack
            .push(argument.object_id().map_or(Value::Undefined, Value::Object));
        let result = (|| {
            let state = self.promise_state()?;
            self.stack.push(Value::Object(state));
            self.promise_state_set(state, "value", argument.clone())?;
            self.check_scheduling_failure()?;
            if let Some(prepared) = prepared.take() {
                prepared.publish(self, state);
            }
            Ok(state)
        })();
        self.stack.truncate(base);
        drop(prepared);
        self.preserve_promise_table_capacity = previous;
        result
    }

    fn prepare_stored_thunk_origin(
        &mut self,
        state: ObjectId,
    ) -> Result<(ThrowOrigin, Option<host_scheduling::SharedSchedulingCharge>), RuntimeError> {
        self.synchronize_scheduling_usage()?;
        let bytes = self
            .promise_thunk_origins
            .get(&state)
            .map_or(0, ThrowOrigin::payload_bytes);
        let mut charge = self
            .host_scheduling
            .as_ref()
            .map(|scope| scope.shared_charge(bytes))
            .transpose()?;
        let origin = self
            .promise_thunk_origins
            .get(&state)
            .cloned()
            .unwrap_or_default();
        if let Some(charge) = &mut charge {
            charge.resize(origin.payload_bytes())?;
        }
        Ok((origin, charge))
    }

    /// Native resolving functions retain the Promise and AlreadyResolved record
    /// in their immutable NativeFunction payload. The VM traces both records.
    pub(in super::super) fn promise_resolving_function(
        &mut self,
        promise: ObjectId,
        state: ObjectId,
        value: Value,
        fulfill: bool,
    ) -> Result<Value, RuntimeError> {
        // This operation also creates the flag on its first call. Retaining
        // the record does not eliminate that property-allocation refusal.
        if self.promise_already_resolved(state)? {
            return Ok(Value::Undefined);
        }
        if fulfill {
            self.resolve_promise(promise, value)?;
        } else {
            self.settle_promise(promise, PromiseStatus::Rejected(value))
                .expect("a native resolver retains its tracked Promise record");
        }
        Ok(Value::Undefined)
    }

    /// Iterator-result creation on this path allocates only a fresh ordinary
    /// object and its data slots. Its failures are uncatchable resource refusals;
    /// propagate them before settling the retained continuation target.
    pub(in super::super) fn promise_async_from_sync_fulfill(
        &mut self,
        target: ObjectId,
        value: Value,
        done: bool,
    ) -> Result<Value, RuntimeError> {
        let result = self.iterator_result(value, done)?;
        self.settle_promise(target, PromiseStatus::Fulfilled(result))
            .expect("an async-from-sync continuation retains its tracked target");
        Ok(Value::Undefined)
    }

    pub(in super::super) fn promise_async_from_sync_reject(
        &mut self,
        target: ObjectId,
        record: ObjectId,
        value: Value,
    ) -> Result<Value, RuntimeError> {
        // IteratorClose with a throw completion preserves the original rejection.
        self.iterator_close_preserving_throw(&Value::Object(record));
        self.settle_promise(target, PromiseStatus::Rejected(value))
            .expect("an async-from-sync continuation retains its tracked target");
        Ok(Value::Undefined)
    }

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
        let prototype = self.promise_prototype().expect(PROMISE_INTRINSICS);
        self.new_promise_with_prototype(prototype)
    }

    fn new_promise_with_prototype(
        &mut self,
        prototype: ObjectId,
    ) -> Result<ObjectId, RuntimeError> {
        self.allocate_promise_record(prototype)
    }

    /// A built-in function object with no name: `length`, then `name` (both
    /// non-writable, non-enumerable, configurable), in the order CreateBuiltin-
    /// Function defines them.
    pub(super) fn promise_native_function(
        &mut self,
        function: NativeFunction,
        length: u32,
    ) -> Result<Value, RuntimeError> {
        // Every caller already holds a promise or a Promise constructor, and
        // creating either went through `global("Promise")`, which builds the
        // string intrinsics first.
        let prototype = self.function_prototype().expect(PROMISE_INTRINSICS);
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
        Ok(self.promise_field(state, key))
    }

    /// A field of a closure-state record (`Undefined` while unset).
    fn promise_field(&self, state: ObjectId, key: &str) -> Value {
        self.heap
            .get_own(state, key)
            .ok()
            .flatten()
            .unwrap_or(Value::Undefined)
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
        self.stack.push(Value::Object(promise));
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
            self.stack
                .push(resolve.object_id().map_or(Value::Undefined, Value::Object));
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
        if self.promise_field(state, "resolved") == Value::Bool(true) {
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
        let intrinsic = self.global("Promise").expect(PROMISE_INTRINSICS);
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
            self.stack
                .push(executor.object_id().map_or(Value::Undefined, Value::Object));
            let promise =
                self.call_native(constructor.clone(), Value::Undefined, vec![executor], true)?;
            // Construct always yields an object.
            self.stack
                .push(promise.object_id().map_or(Value::Undefined, Value::Object));
            let resolve = self.promise_field(storage, "resolve");
            let reject = self.promise_field(storage, "reject");
            // The executor stores both values through Heap::set, which
            // validates object handles. The retained private record owns
            // them; classifying those values cannot fail a heap lookup.
            if !self
                .is_callable(&resolve)
                .expect("the retained capability record owns its validated resolve value")
                || !self
                    .is_callable(&reject)
                    .expect("the retained capability record owns its validated reject value")
            {
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
        if self.promise_field(storage, "resolve") != Value::Undefined
            || self.promise_field(storage, "reject") != Value::Undefined
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
        self.stack.extend([
            constructor
                .object_id()
                .map_or(Value::Undefined, Value::Object),
            value.object_id().map_or(Value::Undefined, Value::Object),
        ]);
        let result = (|| {
            let capability = self.new_promise_capability(constructor)?;
            self.stack.extend([
                capability
                    .promise
                    .object_id()
                    .map_or(Value::Undefined, Value::Object),
                capability
                    .resolve
                    .object_id()
                    .map_or(Value::Undefined, Value::Object),
                capability
                    .reject
                    .object_id()
                    .map_or(Value::Undefined, Value::Object),
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
        self.stack.extend([
            constructor
                .object_id()
                .map_or(Value::Undefined, Value::Object),
            reason.object_id().map_or(Value::Undefined, Value::Object),
        ]);
        let result = (|| {
            let capability = self.new_promise_capability(constructor)?;
            self.stack.extend([
                capability
                    .promise
                    .object_id()
                    .map_or(Value::Undefined, Value::Object),
                capability
                    .resolve
                    .object_id()
                    .map_or(Value::Undefined, Value::Object),
                capability
                    .reject
                    .object_id()
                    .map_or(Value::Undefined, Value::Object),
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
        let default_prototype = self.promise_prototype().expect(PROMISE_INTRINSICS);
        let prototype = self.constructor_prototype(default_prototype)?;
        let promise = self.new_promise_with_prototype(prototype)?;
        // The capability and both resolving functions must survive further
        // allocations. In particular, creating `reject` can collect the
        // freshly-created `resolve` function before the executor observes it.
        let base = self.stack.len();
        self.stack.push(Value::Object(promise));
        let result = (|| {
            let (resolve, reject) = self.promise_resolving_functions(promise)?;
            self.stack.extend([
                resolve.object_id().map_or(Value::Undefined, Value::Object),
                reject.object_id().map_or(Value::Undefined, Value::Object),
            ]);
            let epoch = self.throw_epoch;
            if let Err(error) = self.call_native(
                executor,
                Value::Undefined,
                vec![resolve, reject.clone()],
                false,
            ) {
                // An abrupt executor rejects through the reject function, so
                // an earlier resolve() call still wins.
                let origin = self.caught_rejection_origin(epoch);
                let reason = self.error_value(error)?;
                self.stack
                    .push(reason.object_id().map_or(Value::Undefined, Value::Object));
                self.with_rejection_origin(Some(origin), |vm| {
                    vm.call_native(reject, Value::Undefined, vec![reason], false)
                })?;
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
        self.stack.push(
            constructor
                .object_id()
                .map_or(Value::Undefined, Value::Object),
        );
        let result = (|| {
            let capability = self.new_promise_capability(constructor)?;
            self.stack.extend([
                capability
                    .promise
                    .object_id()
                    .map_or(Value::Undefined, Value::Object),
                capability
                    .resolve
                    .object_id()
                    .map_or(Value::Undefined, Value::Object),
                capability
                    .reject
                    .object_id()
                    .map_or(Value::Undefined, Value::Object),
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
        self.stack.push(
            constructor
                .object_id()
                .map_or(Value::Undefined, Value::Object),
        );
        self.stack
            .extend(args.iter().filter_map(Value::object_id).map(Value::Object));
        let result = (|| {
            let callback = native::argument(args, 0).clone();
            let rest = args.iter().skip(1).cloned().collect();
            let epoch = self.throw_epoch;
            match self.call_native(callback, Value::Undefined, rest, false) {
                Ok(value) => {
                    self.stack
                        .push(value.object_id().map_or(Value::Undefined, Value::Object));
                    self.promise_resolve_constructor(constructor, value)
                }
                Err(error) => {
                    let origin = self.caught_rejection_origin(epoch);
                    let reason = self.error_value(error)?;
                    self.stack
                        .push(reason.object_id().map_or(Value::Undefined, Value::Object));
                    let capability = self.new_promise_capability(constructor)?;
                    self.stack.extend([
                        capability
                            .promise
                            .object_id()
                            .map_or(Value::Undefined, Value::Object),
                        capability
                            .resolve
                            .object_id()
                            .map_or(Value::Undefined, Value::Object),
                        capability
                            .reject
                            .object_id()
                            .map_or(Value::Undefined, Value::Object),
                    ]);
                    self.with_rejection_origin(Some(origin), |vm| {
                        vm.call_native(capability.reject, Value::Undefined, vec![reason], false)
                    })?;
                    Ok(capability.promise)
                }
            }
        })();
        self.stack.truncate(base);
        result
    }

    /// [`Self::settle_promise`] for a promise the VM created itself and so
    /// knows is tracked (a resolving function's own promise, a fresh derived
    /// promise).
    pub(in super::super) fn settle_tracked_promise(
        &mut self,
        promise: ObjectId,
        status: PromiseStatus,
    ) {
        self.settle_promise(promise, status)
            .expect("the VM settles only promises it tracks");
    }

    pub(in super::super) fn settle_promise(
        &mut self,
        promise: ObjectId,
        status: PromiseStatus,
    ) -> Result<(), RuntimeError> {
        self.settle_promise_with_input_charge(promise, status, None)
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
            self.settle_tracked_promise(promise, PromiseStatus::Rejected(error));
            return Ok(());
        }
        if !matches!(resolution, Value::Object(_)) {
            self.settle_tracked_promise(promise, PromiseStatus::Fulfilled(resolution));
            return Ok(());
        }
        let base = self.stack.len();
        self.stack.push(
            resolution
                .object_id()
                .map_or(Value::Undefined, Value::Object),
        );
        let epoch = self.throw_epoch;
        let then = self.get_property(&resolution, &"then".into());
        self.stack.truncate(base);
        let then = match then {
            Ok(then) => then,
            Err(error) => {
                let origin = self.caught_rejection_origin(epoch);
                let error = self.error_value(error)?;
                self.reject_promise_with_origin(promise, error, origin)?;
                return Ok(());
            }
        };
        if !self
            .is_callable(&then)
            .expect("Get returned a live local value or an imported facade")
        {
            self.settle_tracked_promise(promise, PromiseStatus::Fulfilled(resolution));
            return Ok(());
        }
        self.enqueue_promise_job(PromiseJob::Thenable {
            target: promise,
            thenable: resolution,
            then,
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
        let base = self.stack.len();
        self.stack
            .push(receiver.object_id().map_or(Value::Undefined, Value::Object));
        self.stack
            .extend(args.iter().filter_map(Value::object_id).map(Value::Object));
        let result = (|| {
            let target = self.new_promise()?;
            self.perform_promise_then(
                promise,
                native::argument(args, 0).clone(),
                native::argument(args, 1).clone(),
                ReactionTarget::Native(target),
            )
            .expect("the validated Promise record is retained across derived allocation");
            Ok(Value::Object(target))
        })();
        self.stack.truncate(base);
        result
    }

    /// SpeciesConstructor ( promise, %Promise% ).
    fn promise_species_constructor(&mut self, promise: &Value) -> Result<Value, RuntimeError> {
        let default = self.global("Promise").expect(PROMISE_INTRINSICS);
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
        if !self
            .is_constructor(&species)
            .expect("Get returned a live local value or an imported facade")
        {
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
        self.stack
            .push(receiver.object_id().map_or(Value::Undefined, Value::Object));
        self.stack
            .extend(args.iter().filter_map(Value::object_id).map(Value::Object));
        let result = (|| {
            let constructor = self.promise_species_constructor(receiver)?;
            self.stack.push(
                constructor
                    .object_id()
                    .map_or(Value::Undefined, Value::Object),
            );
            let (target, derived) =
                if constructor == self.global("Promise").expect(PROMISE_INTRINSICS) {
                    let derived = self.new_promise()?;
                    (ReactionTarget::Native(derived), Value::Object(derived))
                } else {
                    let capability = self.new_promise_capability(&constructor)?;
                    let derived = capability.promise.clone();
                    (ReactionTarget::Capability(capability), derived)
                };
            self.stack
                .push(derived.object_id().map_or(Value::Undefined, Value::Object));
            self.perform_promise_then(
                promise,
                native::argument(args, 0).clone(),
                native::argument(args, 1).clone(),
                target,
            )
            .expect("the validated Promise record is retained across species construction");
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
        self.stack
            .push(value.object_id().map_or(Value::Undefined, Value::Object));
        self.stack
            .extend(args.iter().filter_map(Value::object_id).map(Value::Object));
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
        self.stack.extend([
            receiver.object_id().map_or(Value::Undefined, Value::Object),
            on_finally
                .object_id()
                .map_or(Value::Undefined, Value::Object),
        ]);
        let result = (|| {
            let constructor = self.promise_species_constructor(receiver)?;
            self.stack.push(
                constructor
                    .object_id()
                    .map_or(Value::Undefined, Value::Object),
            );
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
                self.stack.push(
                    then_finally
                        .object_id()
                        .map_or(Value::Undefined, Value::Object),
                );
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
        self.stack
            .push(argument.object_id().map_or(Value::Undefined, Value::Object));
        let result = (|| {
            let (origin, origin_charge) = self.prepare_current_rejection_origin()?;
            self.with_leased_rejection_origin(Some(origin), origin_charge, |vm| {
                let on_finally = vm.promise_field(state, "onFinally");
                let constructor = vm.promise_field(state, "constructor");
                let outcome = vm.call_native(on_finally, Value::Undefined, Vec::new(), false)?;
                vm.stack
                    .push(outcome.object_id().map_or(Value::Undefined, Value::Object));
                let promise = vm.promise_resolve_constructor(&constructor, outcome)?;
                vm.stack
                    .push(promise.object_id().map_or(Value::Undefined, Value::Object));
                let thunk_state = vm.allocate_finally_thunk_state(argument, catch)?;
                vm.stack.push(Value::Object(thunk_state));
                let thunk = vm.promise_native_function(
                    NativeFunction::PromiseValueThunk {
                        state: thunk_state,
                        thrower: catch,
                    },
                    0,
                )?;
                vm.stack
                    .push(thunk.object_id().map_or(Value::Undefined, Value::Object));
                vm.invoke(&promise, "then", vec![thunk])
            })
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
        if thrower {
            let (origin, charge) = self.prepare_stored_thunk_origin(state)?;
            let value = self.promise_field(state, "value");
            self.publish_rejection_origin(origin);
            drop(charge);
            self.reconcile_scheduling_ownership();
            Err(RuntimeError::Thrown(value))
        } else {
            Ok(self.promise_field(state, "value"))
        }
    }

    /// Implements the settled-value portion of Await. A pending promise needs
    /// a saved interpreter continuation, which remains a separate boundary.
    pub(in super::super) fn await_value(&mut self, value: Value) -> Result<Value, RuntimeError> {
        let promise = value.object_id().expect("PromiseResolve returns a promise");
        // The immediate path shares copy admission and original-owner guards
        // with saved continuations, without admitting a queued resumption.
        let (status, _copy_storage) = self.prepare_promise_status_copy(promise)?;
        match status {
            PromiseAwaitStatus::Pending => {
                Err(RuntimeError::Unsupported("pending await continuation"))
            }
            PromiseAwaitStatus::Fulfilled(value) => Ok(value),
            PromiseAwaitStatus::Rejected(value, origin) => {
                self.publish_rejection_origin(origin);
                Err(RuntimeError::Thrown(value))
            }
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
        self.stack
            .push(value.object_id().map_or(Value::Undefined, Value::Object));
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
        self.stack
            .push(value.object_id().map_or(Value::Undefined, Value::Object));
        let promise = self.new_promise();
        let settled = promise.map(|promise| {
            self.settle_tracked_promise(promise, PromiseStatus::Rejected(value));
            Value::Object(promise)
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
        self.stack
            .push(value.object_id().map_or(Value::Undefined, Value::Object));
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

/// The `expect` message for an intrinsic looked up while operating on a
/// promise or through the `Promise` constructor: `global("Promise")` builds
/// the `Promise` global, `%Promise.prototype%` and the string intrinsics before
/// either can exist, so the lookup only reads a cache.
const PROMISE_INTRINSICS: &str = "the Promise intrinsics are materialized before any promise";

#[cfg(test)]
mod tests {
    use super::*;

    fn plain_object(vm: &mut Vm) -> ObjectId {
        vm.with_roots(|heap| heap.alloc_object(None)).unwrap()
    }

    #[test]
    fn a_promise_settles_as_fulfilled_or_rejected_never_pending() {
        let mut vm = Vm::default();
        let promise = vm.new_promise().unwrap();
        assert_eq!(
            vm.settle_promise(promise, PromiseStatus::Pending),
            Err(RuntimeError::TypeError(
                "a Promise settles as fulfilled or rejected, never as pending".into()
            ))
        );
        assert_eq!(
            vm.settle_promise(promise, PromiseStatus::Fulfilled(Value::Undefined)),
            Ok(())
        );
        // Settlement is final: a second settlement is ignored.
        assert_eq!(vm.settle_promise(promise, PromiseStatus::Pending), Ok(()));
    }

    #[test]
    fn promise_operations_reject_an_object_that_is_not_a_promise() {
        let mut vm = Vm::default();
        let object = plain_object(&mut vm);
        let invalid = Err(RuntimeError::TypeError("invalid Promise receiver".into()));
        assert_eq!(
            vm.settle_promise(object, PromiseStatus::Fulfilled(Value::Undefined)),
            invalid
        );
        assert_eq!(
            vm.promise_then(&Value::Object(object), &[]),
            Err(RuntimeError::TypeError(
                "Promise.prototype.then receiver".into()
            ))
        );
    }

    #[test]
    fn promise_then_returns_a_new_promise_for_a_known_promise() {
        let mut vm = Vm::default();
        let promise = vm.new_promise().unwrap();
        let derived = vm.promise_then(&Value::Object(promise), &[]).unwrap();
        assert!(vm.promises.contains_key(&derived.object_id().unwrap()));
    }

    #[test]
    fn promise_then_reports_a_heap_that_has_no_room_for_the_derived_promise() {
        let ceiling = 1 << 20;
        let mut vm = Vm::new(crate::VmConfig {
            heap: crate::HeapConfig {
                major_threshold_bytes: ceiling,
                max_heap_bytes: ceiling,
                ..crate::HeapConfig::default()
            },
            ..crate::VmConfig::default()
        })
        .unwrap();
        let promise = vm.new_promise().unwrap();
        vm.stack.push(Value::Object(promise));
        // Fill the heap with objects that stay reachable from the stack.
        while let Ok(object) = vm.with_roots(|heap| heap.alloc_object(None)) {
            vm.stack.push(Value::Object(object));
        }
        assert!(vm.promise_then(&Value::Object(promise), &[]).is_err());
    }

    #[test]
    fn settling_a_promise_queues_a_job_for_every_kind_of_reaction() {
        for status in [
            PromiseStatus::Fulfilled(Value::Number(1.0)),
            PromiseStatus::Rejected(Value::Number(2.0)),
        ] {
            let mut vm = Vm::default();
            let promise = vm.new_promise().unwrap();
            let other = ObjectId { heap: 0, serial: 0 };
            vm.promises.get_mut(&promise).unwrap().reactions.extend([
                PromiseReaction::Then(PromiseThenReaction {
                    target: ReactionTarget::Native(other),
                    on_fulfilled: Value::Undefined,
                    on_rejected: Value::Undefined,
                }),
                PromiseReaction::ModuleAwait { continuation: 1 },
                PromiseReaction::AsyncAwait { continuation: 2 },
                PromiseReaction::AsyncGeneratorYield {
                    generator: other,
                    target: other,
                    result: other,
                },
                PromiseReaction::AsyncGeneratorDelegate {
                    generator: other,
                    target: other,
                    kind: AsyncGeneratorDelegateKind::Return,
                },
            ]);
            vm.settle_promise(promise, status).unwrap();
            assert_eq!(vm.promise_jobs.len(), 5);
        }
    }
}
