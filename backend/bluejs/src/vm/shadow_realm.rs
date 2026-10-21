// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `ShadowRealm` (TC39 "stage 2.7" as of 2026-09-18; not part of published
//! ECMA-262 edition 17 -- see
//! `development/browser_core/phase-13-bluejs-engine/ECMASCRIPT_2026.md`).
//! Implemented anyway per an explicit request; only its *edition-17
//! applicability* is affected by that status, not whether it belongs here.
//!
//! A `ShadowRealm`'s own isolated realm is the same "a realm is a whole
//! child `Vm`" primitive `test262.rs`'s `$262.createRealm()` already
//! established (see [`super::Test262Realm`]), reused here rather than
//! reinvented. What differs is the boundary: Test262's realm membrane
//! forwards arbitrary object operations (get/set/call) so cross-realm
//! Test262 fixtures can exercise ordinary object semantics, while
//! `ShadowRealm`'s boundary is deliberately narrow -- `GetWrappedValue`
//! lets only primitives and callables (wrapped through
//! `WrappedFunctionCreate`) cross at all; every other object is a
//! `TypeError`.

use super::compilation_text_storage::NativeText;
use super::eval_compilation_storage::{ParsedEvalInput, PreparedEvalCode};
use super::realm_call_storage::{RealmCallInput, RealmCallRoots};
#[cfg(any(test, coverage))]
use super::realm_reentrancy::register_active;
use super::realm_reentrancy::{
    active_scheduling_scope, prepare_active_registration, resolve_active_entry, ActiveVmEntry,
};
use super::*;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/shadow_eval_storage.rs"]
mod eval_storage_tests;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/shadow_realm_boundaries.rs"]
mod boundary_tests;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/shadow_displaced_roots.rs"]
mod displaced_root_tests;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/realm_alias_home.rs"]
mod alias_home_tests;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/shadow_registry_storage.rs"]
mod registry_storage_tests;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/shadow_argument_storage.rs"]
mod argument_storage_tests;

enum EvaluationCode<'a> {
    #[cfg(any(test, coverage))]
    Borrowed(&'a Bytecode),
    Prepared(&'a PreparedEvalCode),
}

struct PreparedShadowTables {
    records: promise_queue_storage::PreparedMap<ObjectId, ShadowRealmRecord>,
    index: promise_queue_storage::PreparedMap<u64, ObjectId>,
}
impl EvaluationCode<'_> {
    fn execute(&self, child: &mut Vm) -> Result<Value, RuntimeError> {
        match self {
            #[cfg(any(test, coverage))]
            Self::Borrowed(code) => child.execute_script(code),
            Self::Prepared(code) => child.execute_prepared_script(code),
        }
    }
}

impl Vm {
    fn prepare_shadow_tables(
        &mut self,
        instance: ObjectId,
        heap: u64,
    ) -> Result<PreparedShadowTables, RuntimeError> {
        let records = self
            .shadow_realms
            .prepare_set(&instance, self.host_scheduling.as_ref())?;
        let index = self
            .shadow_realm_by_heap
            .prepare_set(&heap, self.host_scheduling.as_ref())?;
        Ok(PreparedShadowTables { records, index })
    }
    fn publish_shadow_record(
        &mut self,
        prepared: PreparedShadowTables,
        instance: ObjectId,
        record: ShadowRealmRecord,
    ) {
        self.shadow_realms.commit(prepared.records);
        self.shadow_realm_by_heap.commit(prepared.index);
        let previous = self.shadow_realms.get(&instance).map(|old| old.heap);
        self.shadow_realm_by_heap.set(record.heap, instance);
        let heap = record.heap;
        self.shadow_realms.set(instance, record);
        if let Some(previous) = previous.filter(|previous| *previous != heap) {
            if self.shadow_realm_by_heap.get(&previous) == Some(&instance) {
                let replacement = self
                    .shadow_realms
                    .iter()
                    .find_map(|(&id, record)| (record.heap == previous).then_some(id));
                if let Some(id) = replacement {
                    *self
                        .shadow_realm_by_heap
                        .get_mut(&previous)
                        .expect("existing previous index") = id;
                } else {
                    self.shadow_realm_by_heap.remove(&previous);
                }
            }
        }
    }
    fn collect_before_shadow_realm_entry(&mut self) -> Result<(), RuntimeError> {
        if self.heap.document_heap_budget().is_some() {
            // Collect the owner before borrowing its child. Once the child
            // runs, its local collector cannot release dead ancestor storage.
            self.collect_realm_garbage()?;
        }
        Ok(())
    }

    pub(super) fn shadow_realm_constructor(
        &mut self,
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if !construct {
            return Err(RuntimeError::TypeError(
                "Constructor ShadowRealm requires 'new'".into(),
            ));
        }
        self.collect_before_shadow_realm_entry()?;
        let default = self.shadow_realm_prototype()?;
        let prototype = self.constructor_prototype(default)?;
        let mut child = Vm::new(self.config)
            .map_err(|_| RuntimeError::RangeError("could not create a ShadowRealm".into()))?;
        // Realms created within one ShadowRealm agent share the
        // GlobalSymbolRegistry, matching `$262.createRealm()`'s identical
        // choice for the same spec-mandated reason (`Symbol.for` is
        // agent-wide even though every Realm keeps its own globals).
        child.symbol_registry = Rc::clone(&self.symbol_registry);
        child.inherit_document_policy(self)?;
        let record = ShadowRealmRecord::new(child)?;
        let instance = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        let prepared = self.prepare_shadow_tables(instance, record.heap)?;
        self.publish_shadow_record(prepared, instance, record);
        Ok(Value::Object(instance))
    }

    /// Registers `instance` (in this `Vm`'s own heap) as denoting the exact
    /// same live `ShadowRealm` child realm as `record` -- used when a
    /// `ShadowRealm` *instance itself* crosses a Test262
    /// `$262.createRealm()` boundary into this realm (see
    /// `test262_transport_value`'s ShadowRealm-specific branch), so that
    /// `evaluate`/`importValue`/a wrapped-function call reached through the
    /// transported stand-in observes the identical realm -- same
    /// `globalThis`, same prior `evaluate` side effects -- as the original.
    pub(super) fn adopt_shadow_realm(
        &mut self,
        instance: ObjectId,
        mut record: ShadowRealmRecord,
    ) -> Result<(), RuntimeError> {
        let tables = self.prepare_shadow_tables(instance, record.heap)?;
        let owner = record.owners.prepare_owner(self.host_scheduling.as_ref())?;
        // A new table entry is an independent owner. Temporary traversal and
        // call clones retain the same token and do not create new owners.
        record.owner = record.owners.commit_owner(owner);
        self.publish_shadow_record(tables, instance, record);
        Ok(())
    }

    /// The live record for `instance`, if this `Vm` has one -- for
    /// `test262.rs` to clone (a cheap `Rc` bump) into another realm via
    /// [`Vm::adopt_shadow_realm`].
    pub(super) fn shadow_realm_record(&self, instance: ObjectId) -> Option<ShadowRealmRecord> {
        self.shadow_realms.get(&instance).cloned()
    }

    pub(super) fn shadow_realm_prototype(&mut self) -> Result<ObjectId, RuntimeError> {
        if let Some(prototype) = self.shadow_realm_prototype {
            return Ok(prototype);
        }
        let function_prototype = self.function_prototype()?;
        let object_prototype = self.object_prototype;
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
        // The intrinsic can outlive a refused constructor initialization.
        // Its cache therefore needs a permanent root before publication.
        let root = self.heap.root(prototype)?;
        let result: Result<(), RuntimeError> = (|| {
            self.define_data(
                prototype,
                JsSymbol::well_known("toStringTag"),
                Value::String("ShadowRealm".into()),
                false,
                false,
                true,
            )?;
            self.install_native(
                prototype,
                function_prototype,
                "evaluate",
                1,
                NativeFunction::ShadowRealmEvaluate,
            )?;
            self.install_native(
                prototype,
                function_prototype,
                "importValue",
                2,
                NativeFunction::ShadowRealmImportValue,
            )?;
            Ok(())
        })();
        if let Err(error) = result {
            self.release_root(root);
            return Err(error);
        }
        self.shadow_realm_prototype = Some(prototype);
        Ok(prototype)
    }

    /// `ShadowRealm.prototype.evaluate` (`PerformShadowRealmEval`). Runs
    /// `source_text` as a classic Script in this instance's own realm and
    /// returns `GetWrappedValue(callerRealm, result)`. Any catchable abrupt
    /// completion -- a parse failure, a thrown value, or an unsettled
    /// promise job's own rejection -- becomes an opaque, message-less
    /// `TypeError` in the *caller's* realm (`CreateTypeErrorCopy`): the
    /// proposal deliberately does not leak another realm's error identity
    /// or message across the boundary. Fatal host aborts retain their identity.
    pub(super) fn shadow_realm_evaluate(
        &mut self,
        this: Value,
        source_text: &Value,
    ) -> Result<Value, RuntimeError> {
        self.synchronize_scheduling_usage()?;
        self.collect_before_shadow_realm_entry()?;
        let this_id = self.require_shadow_realm(&this, "evaluate")?;
        let Value::String(source_text) = source_text else {
            return Err(RuntimeError::TypeError(
                "ShadowRealm.prototype.evaluate requires a string".into(),
            ));
        };
        let record = self
            .shadow_realms
            .get(&this_id)
            .cloned()
            .expect("require_shadow_realm confirmed this_id names a live ShadowRealm record");
        let target_scope = match record.vm.try_borrow() {
            Ok(child) => child.host_scheduling.as_ref().map(SchedulingScope::child),
            Err(_) => active_scheduling_scope(record.heap),
        };
        if let (Some(caller), Some(target)) = (&self.host_scheduling, &target_scope) {
            if !caller.same_document(target) {
                return Err(target.fail());
            }
        }
        let scope =
            target_scope.or_else(|| self.host_scheduling.as_ref().map(SchedulingScope::child));
        // Parsing happens before an eval execution context is ever pushed:
        // a `HostEnsureCanCompileStrings`/`ParseText` failure throws a real
        // `SyntaxError` directly, distinct from the opaque `TypeError`
        // `PerformShadowRealmEval`'s later *evaluation* step produces on any
        // abrupt completion (`CreateTypeErrorCopy`). Both propagate through
        // the realm whose `evaluate` function is actually executing --
        // ordinarily `self`, or (through `Test262`'s own foreign-call
        // membrane) another realm's `Vm` when `evaluate` was reached via a
        // cross-realm facade, with no extra work required here.
        let text = NativeText::encoded(source_text, scope.as_ref())?;
        let program = ParsedEvalInput::parse_script(&text, scope.as_ref())?;
        drop(text);
        let mut code = program.compile_with(scope.as_ref(), crate::compile)?;
        // `this_id` names a live record (checked above), so this record is
        // always present; it may still be a *shared* one -- see
        // `ShadowRealmRecord`'s own doc comment -- if it was ever
        // transported into another realm.
        return match record.vm.try_borrow_mut() {
            Ok(mut child) => {
                let child_registration = prepare_active_registration(&mut child, scope.as_ref())?;
                let child_guard = child_registration.publish(&mut child)?;
                let outcome = Self::run_prepared_evaluate(self, &mut child, &mut code, false);
                drop(child_guard);
                match outcome {
                    Ok(value) => self.shadow_wrap_into(&mut child, value),
                    Err(error) if !error.is_catchable() => Err(error),
                    Err(_) => Err(RuntimeError::TypeError(String::new())),
                }
            }
            Err(_) => {
                // Already mid-call further up this exact synchronous chain
                // (a wrapped-function call chain that loops back to
                // `evaluate` on a realm one of those calls is already
                // running through). Reach it the same way a callee reaches
                // any other ancestor: through `ACTIVE`, keyed by the
                // permanent tag lookup below (unlike a fresh borrow, this
                // mapping does not itself change while checked out).
                let heap_tag = self
                    .shadow_realm_by_heap
                    .iter()
                    .find(|&(_, &id)| id == this_id)
                    .map(|(&tag, _)| tag);
                let entry = self.resolve_shadow_ancestor(heap_tag)?;
                entry.with(|record_vm| {
                    let outcome = Self::run_prepared_evaluate(self, record_vm, &mut code, true);
                    match outcome {
                        Ok(value) => self.shadow_wrap_into(record_vm, value),
                        Err(error) if !error.is_catchable() => Err(error),
                        Err(_) => Err(RuntimeError::TypeError(String::new())),
                    }
                })
            }
        };
    }

    fn run_prepared_evaluate(
        caller: &mut Vm,
        child: &mut Vm,
        code: &mut PreparedEvalCode,
        reentrant: bool,
    ) -> Result<Value, RuntimeError> {
        child.synchronize_scheduling_usage()?;
        // Related bounded realms share one account. A neutral caller attaches
        // to the child's account; a neutral child retains its caller's lease.
        // Foreign-document refusal precedes lexical reset or frame suspension.
        if let Some(scope) = child
            .host_scheduling
            .as_ref()
            .or(caller.host_scheduling.as_ref())
        {
            code.attach(scope)?;
        }
        Self::run_evaluation_code(caller, child, EvaluationCode::Prepared(code), reentrant)
    }

    /// Runs a parsed/compiled `evaluate` body in `child` (registering
    /// `caller` as active for the duration, so `child`'s own execution can
    /// call back into it) and drains `child`'s own promise jobs -- the
    /// shared inner step both of `shadow_realm_evaluate`'s two paths (child
    /// freshly checked out vs. already active further up this call chain)
    /// need identically.
    #[cfg(any(test, coverage))]
    fn run_evaluate(
        caller: &mut Vm,
        child: &mut Vm,
        code: &Bytecode,
        reentrant: bool,
    ) -> Result<Value, RuntimeError> {
        Self::run_evaluation_code(caller, child, EvaluationCode::Borrowed(code), reentrant)
    }
    fn run_evaluation_code(
        caller: &mut Vm,
        child: &mut Vm,
        code: EvaluationCode<'_>,
        reentrant: bool,
    ) -> Result<Value, RuntimeError> {
        // Reentrant setup temporarily hides the ancestor's call depth, even
        // when its caller is neutral. Its document job still owns kept targets.
        let _checkpoint_deferral = if reentrant {
            child
                .host_scheduling
                .as_ref()
                .map(SchedulingScope::defer_checkpoint)
                .transpose()?
        } else {
            None
        };
        let registration = prepare_active_registration(caller, child.host_scheduling.as_ref())?;
        if !reentrant {
            let _guard = registration.publish(caller)?;
            child.reset_lexical_global_bindings();
            let value = code.execute(child)?;
            child.run_promise_jobs()?;
            // Metadata traps execute after this function returns. Retain the
            // evaluation's completion root and remaining instruction budget.
            return Ok(value);
        }
        // evaluate may reenter an ancestor realm that is still executing a
        // wrapped function. Its fresh script context must not erase that
        // function's bindings, operand stack, or dynamic eval environments.
        let lexical_bindings = child.suspend_lexical_global_bindings()?;
        let execution = child.suspend_module_execution();
        let mut roots = match child.root_suspended_module_execution(&execution) {
            Ok(roots) => roots,
            Err(error) => {
                child.restore_module_execution(execution);
                lexical_bindings.restore(child);
                return Err(error);
            }
        };
        // Stage roots for fields outside the common frame before taking any
        // of them. Registration/admission failure must leave all owners intact.
        let extra_roots = Self::append_registered_frame_roots(
            &mut roots,
            &mut child.heap,
            child.host_scheduling.as_ref(),
            child
                .parameter_eval_env
                .into_iter()
                .chain(child.kept_weak_objects.iter().copied()),
        );
        if let Err(error) = extra_roots {
            child.release_module_roots_from(&mut roots, 0);
            child.restore_module_execution(execution);
            lexical_bindings.restore(child);
            return Err(error);
        }
        let call_depth = std::mem::replace(&mut child.call_depth, 0);
        let parameter_eval_env = child.parameter_eval_env.take();
        // Script cleanup clears these even when an ancestor call is active.
        let call_stack = std::mem::take(&mut child.call_stack);
        // Document evaluations share the surrounding kept-target set. Taking
        // it would discard inner targets when the outer context is restored.
        let kept_weak_objects = child
            .host_scheduling
            .is_none()
            .then(|| std::mem::take(&mut child.kept_weak_objects));
        let inherited_with_depth = std::mem::replace(&mut child.inherited_with_depth, 0);
        // GetShadowRealmContext ( shadowRealmRecord, strictEval ): "Let
        // lexEnv be NewDeclarativeEnvironment(shadowRealmRecord.[[GlobalEnv]])."
        // Every single `evaluate` call gets a *fresh* declarative
        // environment for its own top-level `let`/`const` -- unlike an
        // ordinary repeated top-level Script, which runs
        // GlobalDeclarationInstantiation directly against the realm's own,
        // persistent Global Environment Record (so a second `let x` in a
        // second <script> genuinely does conflict with an earlier one,
        // matching real engines). This Vm's `execute_script` only
        // implements that latter, ordinary case: it tracks lexical global
        // redeclarations in one persistent `global_bindings` map with no
        // notion of "this call's own fresh lexEnv is now out of scope".
        // Clear its lexical (non-property) entries before every fresh
        // `evaluate` script, releasing their GC roots, so this call's
        // top-level declarations never see a previous call's now-defunct
        // ones as already declared. `var`/function declarations are
        // untouched: `varEnv` stays the realm's shared GlobalEnv even
        // though `lexEnv` is fresh, so those must keep being real, visible
        // `globalThis` properties across calls exactly as `execute_script`
        // already provides.
        let outcome = (|| {
            let _guard = registration.publish(caller)?;
            child.reset_lexical_global_bindings();
            let value = code.execute(child)?;
            // Promise reactions belong to this evaluation context too.
            child.run_promise_jobs()?;
            Ok(value)
        })();
        child.reset_lexical_global_bindings();
        lexical_bindings.restore(child);
        if let Some(root) = child.result_root.take() {
            child.release_root(root);
        }
        child.restore_module_execution(execution);
        child.call_depth = call_depth;
        child.call_stack = call_stack;
        if let Some(kept) = kept_weak_objects {
            child.kept_weak_objects = kept;
        }
        child.parameter_eval_env = parameter_eval_env;
        child.inherited_with_depth = inherited_with_depth;
        for root in roots {
            child.release_root(root);
        }
        outcome
    }

    /// `ShadowRealm.prototype.importValue` (`ShadowRealmImportValue`),
    /// reusing this engine's existing host-supplied module registry --
    /// exactly the mechanism ordinary dynamic `import()` already uses
    /// (`Vm::dynamic_import`) -- rather than a separate loader. Every
    /// failure (bad specifier, missing/throwing module, missing export)
    /// rejects the returned promise with an opaque `TypeError`, mirroring
    /// `evaluate`'s identical boundary and the proposal's own
    /// `%ThrowTypeError%` rejection handler.
    pub(super) fn shadow_realm_import_value(
        &mut self,
        this: Value,
        specifier: Value,
        export_name: Value,
    ) -> Result<Value, RuntimeError> {
        self.synchronize_scheduling_usage()?;
        self.collect_before_shadow_realm_entry()?;
        let this_id = self.require_shadow_realm(&this, "importValue")?;
        // ToString happens in the caller's own realm and propagates
        // directly (not as a promise rejection) -- it runs before
        // `ShadowRealmImportValue`'s own promise capability even exists.
        let specifier = self.coerce_string(&specifier)?;
        // Unlike `specifier`, `exportName` is a plain type check with no
        // coercion attempted ("If exportName is not a String, throw a
        // TypeError exception") -- a throwing `toString`/`valueOf` on a
        // non-string `exportName` must never run.
        let Value::String(export_name) = export_name else {
            return Err(RuntimeError::TypeError(
                "ShadowRealm.prototype.importValue requires exportName to be a string".into(),
            ));
        };
        let mut inherited_module = Some(self.prepare_active_module_name_copy()?);
        let inherited_registry = match self
            .module_registry
            .for_shadow_realm(self.host_scheduling.as_ref())
        {
            Ok(registry) => registry,
            Err(error) => {
                drop(inherited_module);
                self.reconcile_scheduling_ownership();
                return Err(error);
            }
        };
        let promise_constructor = self.global("Promise")?;
        let PromiseCapability {
            promise,
            resolve,
            reject,
        } = self.new_promise_capability(&promise_constructor)?;
        // The capability's functions are referenced only from these locals
        // while the child realm runs and the caller heap allocates wrappers
        // and an error value before they are finally called.
        let base = self.stack.len();
        self.stack
            .extend([promise.clone(), resolve.clone(), reject.clone()]);
        let record = self
            .shadow_realms
            .get(&this_id)
            .cloned()
            .expect("require_shadow_realm confirmed this_id names a live ShadowRealm record");
        let outcome: Result<Value, RuntimeError> = match record.vm.try_borrow_mut() {
            Ok(mut child) => {
                let inner: Result<Value, RuntimeError> = (|| {
                    if let Some(scope) = child.host_scheduling.as_ref() {
                        self.attach_module_sources(scope)?;
                    }
                    // A ShadowRealm has its own module graph and heap, but
                    // shares immutable host source and compiled snapshots.
                    // Only object-valued synthetic records need filtering;
                    // graph/source-object caches contain caller ObjectIds.
                    if let Some(scope) = &child.host_scheduling {
                        inherited_registry.attach(scope)?;
                    }
                    child.module_registry = inherited_registry;
                    // A synthetic module the caller already built (JSON, bytes)
                    // carries a value living in the *caller's* heap; the child
                    // rebuilds its own from the host sources copied below.
                    child.dynamic_module_sources = self.dynamic_module_sources.clone();
                    child.json_module_sources = self.json_module_sources.clone();
                    child.text_module_sources = self.text_module_sources.clone();
                    child.bytes_module_sources = self.bytes_module_sources.clone();
                    child.module_source_registry = self.module_source_registry.clone();
                    child.active_module_name = inherited_module
                        .take()
                        .expect("the import transfers its referrer once")
                        .into_name();
                    self.reconcile_scheduling_ownership();
                    child.synchronize_scheduling_usage()?;
                    child.remaining_instructions = child.config.instruction_budget;
                    let promise_value = child.dynamic_import(
                        Value::String(specifier.clone()),
                        Value::Undefined,
                        ImportPhase::Evaluation,
                    )?;
                    let inner_promise = promise_value
                        .object_id()
                        .expect("dynamic_import always returns a Promise object");
                    child.run_promise_jobs()?;
                    let namespace = match child.promises.get(&inner_promise).map(|p| &p.status) {
                        Some(PromiseStatus::Fulfilled(value)) => value.clone(),
                        _ => {
                            return Err(RuntimeError::TypeError(
                                "module import did not resolve".into(),
                            ))
                        }
                    };
                    let Value::Object(namespace_id) = namespace else {
                        return Err(RuntimeError::TypeError(
                            "module namespace is not an object".into(),
                        ));
                    };
                    let export_name_utf8 = export_name.to_utf8().map_err(|_| {
                        RuntimeError::TypeError("export name is not a Unicode string".into())
                    })?;
                    let has_export = child
                        .heap
                        .get_own_property_descriptor(namespace_id, export_name_utf8.as_str())?
                        .is_some();
                    if !has_export {
                        return Err(RuntimeError::TypeError(
                            "the requested export does not exist".into(),
                        ));
                    }
                    child.get_property(&namespace, &PropertyName::String(export_name.clone()))
                })();
                match inner {
                    Ok(value) => self.shadow_wrap_into(&mut child, value),
                    Err(error) => Err(error),
                }
            }
            Err(_) => Err(RuntimeError::TypeError(
                "this ShadowRealm is already mid-call".into(),
            )),
        };
        drop(inherited_module);
        self.reconcile_scheduling_ownership();
        let settled = match outcome {
            Ok(value) => self.call_native(resolve, Value::Undefined, vec![value], false),
            Err(_) => self
                .error_value(RuntimeError::TypeError(String::new()))
                .and_then(|error| self.call_native(reject, Value::Undefined, vec![error], false)),
        };
        self.stack.truncate(base);
        settled?;
        Ok(promise)
    }

    /// Dispatches a call to a `WrappedFunctionCreate` facade. Reached from
    /// `dispatch_call` before the callee is reduced to a bare
    /// `NativeFunction` tag, since (unlike an ordinary method) a wrapped
    /// function's own identity -- not its receiver -- selects which
    /// cross-realm target it forwards to.
    pub(super) fn shadow_call_wrapped(
        &mut self,
        wrapper: ObjectId,
        this_arg: Value,
        args: Vec<Value>,
        construct: bool,
        entry: execution_payload::CallEntryCharge,
    ) -> Result<Value, RuntimeError> {
        self.synchronize_scheduling_usage()?;
        self.collect_before_shadow_realm_entry()?;
        if construct {
            return Err(RuntimeError::TypeError(
                "a ShadowRealm wrapped function has no [[Construct]]".into(),
            ));
        }
        let input = RealmCallInput {
            callee: Value::Object(wrapper),
            receiver: this_arg,
            arguments: args,
            entry,
        };
        let ShadowWrappedFunction {
            home_heap, target, ..
        } = self
            .shadow_wrapped_functions
            .get(&wrapper)
            .cloned()
            .expect("dispatch_call only routes here for a registered wrapper");
        // The common case: `self` directly holds a (possibly shared, if it
        // was ever transported through Test262's own membrane -- see
        // `ShadowRealmRecord`'s doc comment) reference to the target's
        // realm, and it is not already mid-call further up this exact
        // synchronous chain.
        if let Some(&realm_key) = self.shadow_realm_by_heap.get(&home_heap) {
            let record = self
                .shadow_realms
                .get(&realm_key)
                .cloned()
                .expect("shadow_realm_by_heap stays in sync with shadow_realms");
            match record.vm.try_borrow_mut() {
                Ok(mut other) => {
                    // Register this child as active, by its own heap tag,
                    // for exactly the duration of this borrow -- so a call
                    // chain that loops back through it (reaching it only
                    // through `ACTIVE`, below) finds it there instead of
                    // trying to borrow it a second time.
                    let child_registration =
                        prepare_active_registration(&mut other, self.host_scheduling.as_ref())?;
                    let child_guard = child_registration.publish(&mut other)?;
                    let result = self.shadow_call_across(&mut other, target, input);
                    drop(child_guard);
                    return result;
                }
                Err(_) => {
                    // Fall through to the `ACTIVE`-based lookup below: some
                    // ancestor frame is mid-call through this exact realm
                    // (reached itself only through `ACTIVE`, e.g. a
                    // wrapped-function chain looping back through a realm
                    // `self` also owns directly), so this borrow_mut cannot
                    // succeed a second time.
                }
            };
        }
        // A short alias need not activate the callable's original owner.
        // Its weak handle preserves allocation credit without keeping a VM alive.
        let home = self.shadow_wrapped_functions[&wrapper].home.clone();
        if let Some(home) = home.and_then(|home| home.upgrade()) {
            if let Ok(mut other) = home.try_borrow_mut() {
                let registration =
                    prepare_active_registration(&mut other, self.host_scheduling.as_ref())?;
                let guard = registration.publish(&mut other)?;
                let result = self.shadow_call_across(&mut other, target, input);
                drop(guard);
                return result;
            }
        }
        // The home realm is only reachable through an active ancestor call; a
        // missing one is this boundary's own catchable error.
        let entry = self.resolve_shadow_ancestor(Some(home_heap))?;
        entry.with(|other| self.shadow_call_across(other, target, input))
    }

    /// Validate the active ancestor before either reentrant boundary reborrows
    /// its pointer. An absent ancestor and a self-reference are catchable.
    pub(super) fn resolve_shadow_ancestor(
        &self,
        heap_tag: Option<u64>,
    ) -> Result<ActiveVmEntry, RuntimeError> {
        let unavailable = || {
            RuntimeError::TypeError(
                "the ShadowRealm this function belongs to is no longer reachable".into(),
            )
        };
        let Some(tag) = heap_tag else {
            return Err(unavailable());
        };
        if tag == self.object_prototype.heap && realm_reentrancy::is_active(tag) {
            return Err(RuntimeError::TypeError(
                "a ShadowRealm boundary cannot resolve to itself".into(),
            ));
        }
        // An absent ancestor is this boundary's own catchable error; leave the
        // generic entry failure for a live ancestor that cannot be borrowed.
        if !realm_reentrancy::is_active(tag) {
            return Err(unavailable());
        }
        resolve_active_entry(self.object_prototype.heap, tag)?.ok_or_else(unavailable)
    }

    /// `OrdinaryWrappedFunctionCall`: wraps `this`/each argument *into*
    /// `other`'s realm (`GetWrappedValue(targetRealm, _)`), invokes the
    /// target there, then wraps the result (or a fresh `TypeError` for a
    /// catchable target exception) back into `self`'s realm
    /// (`GetWrappedValue(callerRealm, _)`).
    fn shadow_call_across(
        &mut self,
        other: &mut Vm,
        target: ObjectId,
        mut input: RealmCallInput,
    ) -> Result<Value, RuntimeError> {
        let owner = self.realm_call_native_owner(other)?;
        input.entry.retain_native(
            self,
            &input.callee,
            &input.receiver,
            &input.arguments,
            owner.as_ref(),
        )?;
        let source_roots = RealmCallRoots::new(input.objects().count(), owner.as_ref())?;
        input
            .objects()
            .try_for_each(|id| source_roots.push(&self.heap, id))?;
        let target_owner = other.realm_call_native_owner(self)?;
        let count = 1
            + input.receiver.object_id().is_some() as usize
            + input.arguments.iter().filter_map(Value::object_id).count();
        let target_roots = RealmCallRoots::new(count, target_owner.as_ref())?;
        target_roots.push(&other.heap, target)?;
        let registration = prepare_active_registration(self, other.host_scheduling.as_ref())?;
        let _guard = registration.publish(self)?;
        // Guest metadata getters can collect in either heap. Original IDs
        // remain rooted in the source; completed facades get destination IDs.
        // Primitive buffers move in place in the incoming allocation.
        let result = self.with_realm_call_roots(source_roots, |source| {
            other.with_realm_call_roots(target_roots.clone(), |other| {
                input.receiver = other.shadow_wrap_into(source, input.receiver)?;
                if let Some(id) = input.receiver.object_id() {
                    target_roots.push(&other.heap, id)?;
                }
                for slot in &mut input.arguments {
                    let value = std::mem::replace(slot, Value::Undefined);
                    *slot = other.shadow_wrap_into(source, value)?;
                    if let Some(id) = slot.object_id() {
                        target_roots.push(&other.heap, id)?;
                    }
                }
                other.remaining_instructions = other.config.instruction_budget;
                // Keep conversion errors distinct from target exceptions.
                Ok(input.invoke(other, target, false))
            })
        })?;
        if let Some(owner) = &owner {
            owner.reserve_peer(0)?;
        }
        match result {
            Ok(value) => self.shadow_wrap_into(other, value),
            Err(error) if !error.is_catchable() => Err(error),
            Err(_) => Err(RuntimeError::TypeError(String::new())),
        }
    }

    /// `GetWrappedValue(callerRealm = self, value)`: a value owned by
    /// `source` crossing into `self`. Non-Object values (this engine's
    /// String/Number/Bool/BigInt/Symbol/Undefined/Null carry no heap
    /// affinity -- see `Value`) cross unchanged. An Object must be
    /// callable in `source`, becoming a fresh `WrappedFunctionCreate`
    /// facade allocated in `self`; any other Object is a `TypeError`.
    fn shadow_wrap_into(&mut self, source: &mut Vm, value: Value) -> Result<Value, RuntimeError> {
        let Value::Object(target) = value else {
            return Ok(value);
        };
        if !source.is_callable(&Value::Object(target))? {
            return Err(RuntimeError::TypeError(
                "only primitive values and functions may cross a ShadowRealm boundary".into(),
            ));
        }
        self.shadow_wrapped_function_create(source, target)
    }

    /// `WrappedFunctionCreate(callerRealm = self, Target = target)`.
    fn shadow_wrapped_function_create(
        &mut self,
        source: &mut Vm,
        target: ObjectId,
    ) -> Result<Value, RuntimeError> {
        // `target` may otherwise be an unstored expression completion value
        // with nothing else in `source`'s own reachability graph keeping it
        // alive until the facade is collected. Its admitted pin remains in the
        // home heap until safepoint cleanup observes no external handles; dropping
        // a handle needs no allocation or borrow of an active ancestor.
        let target_root = source.heap.realm_root(target)?;
        let outcome = (|| {
            let owner = self.realm_call_native_owner(source)?;
            let roots = RealmCallRoots::new(1, owner.as_ref())?;
            self.with_realm_call_roots(roots.clone(), |vm| {
                let function_prototype = vm.function_prototype()?;
                let wrapper = vm.with_roots(|heap| {
                    heap.alloc_native_function(
                        NativeFunction::ShadowRealmWrappedFunction,
                        "",
                        function_prototype,
                    )
                })?;
                roots.push(&vm.heap, wrapper)?;
                let copied = vm.copy_name_and_length(source, wrapper, target);
                // Metadata getters must not expose their original exception
                // across the boundary. The root transfers to the wrapper record
                // only after every descriptor has been copied successfully.
                copied.map_err(|error| {
                    if error.is_catchable() {
                        RuntimeError::TypeError(
                            "could not wrap this function across a ShadowRealm".into(),
                        )
                    } else {
                        error
                    }
                })?;
                let prepared = vm
                    .shadow_wrapped_functions
                    .prepare_insert(vm.host_scheduling.as_ref())?;
                vm.shadow_wrapped_functions.commit(prepared);
                vm.shadow_wrapped_functions.insert(
                    wrapper,
                    ShadowWrappedFunction {
                        home_heap: target.heap,
                        target,
                        target_root: target_root.clone(),
                        home: source.realm_home.clone(),
                    },
                );
                Ok(Value::Object(wrapper))
            })
        })();
        if outcome.is_err() {
            drop(target_root);
            source.heap.release_realm_roots();
        }
        outcome
    }

    /// `CopyNameAndLength(F = wrapper, Target = target)` with no prefix and
    /// `argCount = 0`, the shape `WrappedFunctionCreate` always uses.
    fn copy_name_and_length(
        &mut self,
        source: &mut Vm,
        wrapper: ObjectId,
        target: ObjectId,
    ) -> Result<(), RuntimeError> {
        // `HasOwnProperty(Target, "length")` through the ordinary
        // Proxy-trap-aware path -- the raw heap record does not dispatch a
        // Proxy's own `getOwnPropertyDescriptor` trap (or its revocation
        // check), and `Target` may be exactly such a Proxy.
        let has_length = source
            .proxy_get_own_property(target, &"length".into())?
            .is_some();
        let length = if has_length {
            match source.get_property(&Value::Object(target), &"length".into())? {
                Value::Number(n) if n.is_infinite() && n.is_sign_positive() => f64::INFINITY,
                Value::Number(n) if n.is_infinite() => 0.0,
                Value::Number(n) if n.is_nan() => 0.0,
                Value::Number(n) => n.trunc().max(0.0),
                _ => 0.0,
            }
        } else {
            0.0
        };
        let name = match source.get_property(&Value::Object(target), &"name".into())? {
            Value::String(name) => name,
            _ => JsString::default(),
        };
        self.define_data(wrapper, "length", Value::Number(length), false, false, true)?;
        self.define_data(wrapper, "name", Value::String(name), false, false, true)?;
        Ok(())
    }

    fn require_shadow_realm(&self, this: &Value, method: &str) -> Result<ObjectId, RuntimeError> {
        match this.object_id() {
            Some(id) if self.shadow_realms.contains_key(&id) => Ok(id),
            _ => Err(RuntimeError::TypeError(format!(
                "ShadowRealm.prototype.{method} called on a non-ShadowRealm receiver"
            ))),
        }
    }
}
