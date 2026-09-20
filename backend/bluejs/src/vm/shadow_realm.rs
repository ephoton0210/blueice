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

use super::*;

thread_local! {
    /// A stack of `Vm`s currently mid-call as part of one synchronous
    /// cross-`ShadowRealm` call chain on this thread, most recently
    /// registered last.
    ///
    /// Safety model: [`register_active`] pushes a raw pointer to `self`
    /// immediately before `self` calls into another realm, and the returned
    /// [`ActiveGuard`] pops it the instant that nested call returns -- i.e.
    /// its validity window is exactly the dynamic extent of the `&mut self`
    /// call already in progress on the Rust stack when it was pushed. A
    /// `Vm` value can freely be moved by its owner *between* top-level
    /// calls (nothing here assumes a `Vm` is pinned in memory); what must
    /// not happen, and does not, is moving it *while* one of its own
    /// methods holds `&mut self` -- Rust already guarantees that on our
    /// behalf for the exact same reason a safe `&mut` borrow would.
    ///
    /// A wrapped-function facade that a realm retains and calls again long
    /// after the synchronous call that produced it has returned (so no
    /// entry remains for its home realm) simply fails
    /// [`resolve_active`] and reports a catchable error, rather than
    /// dereferencing memory whose lifetime has already ended.
    static ACTIVE: RefCell<Vec<(u64, *mut Vm)>> = const { RefCell::new(Vec::new()) };
}

struct ActiveGuard {
    tag: u64,
}

impl Drop for ActiveGuard {
    fn drop(&mut self) {
        ACTIVE.with(|active| {
            let popped = active.borrow_mut().pop();
            debug_assert!(
                popped.is_some_and(|(tag, _)| tag == self.tag),
                "ShadowRealm active-Vm registrations must nest with the Rust call stack"
            );
        });
    }
}

/// Registers `vm` as reachable by its own heap tag for the dynamic extent of
/// the returned guard. Call this immediately before `vm` calls into another
/// realm, and keep the guard alive across exactly that call.
fn register_active(vm: &mut Vm) -> ActiveGuard {
    let tag = vm.object_prototype.heap;
    ACTIVE.with(|active| active.borrow_mut().push((tag, vm as *mut Vm)));
    ActiveGuard { tag }
}

/// Finds the most recently registered `Vm` for `tag`, if one is currently
/// mid-call on this thread's synchronous cross-realm call chain. See
/// `ACTIVE`'s own documentation for the safety argument governing every
/// caller of this function.
fn resolve_active(tag: u64) -> Option<*mut Vm> {
    ACTIVE.with(|active| {
        active
            .borrow()
            .iter()
            .rev()
            .find(|(t, _)| *t == tag)
            .map(|(_, ptr)| *ptr)
    })
}

impl Vm {
    pub(super) fn shadow_realm_constructor(
        &mut self,
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if !construct {
            return Err(RuntimeError::TypeError(
                "Constructor ShadowRealm requires 'new'".into(),
            ));
        }
        let default = self.shadow_realm_prototype()?;
        let prototype = self.constructor_prototype(default)?;
        let mut child = Vm::new(self.config)
            .map_err(|_| RuntimeError::RangeError("could not create a ShadowRealm".into()))?;
        // Realms created within one ShadowRealm agent share the
        // GlobalSymbolRegistry, matching `$262.createRealm()`'s identical
        // choice for the same spec-mandated reason (`Symbol.for` is
        // agent-wide even though every Realm keeps its own globals).
        child.symbol_registry = Rc::clone(&self.symbol_registry);
        let heap_tag = child.object_prototype.heap;
        let instance = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        self.shadow_realm_by_heap.insert(heap_tag, instance);
        self.shadow_realms.insert(
            instance,
            ShadowRealmRecord {
                vm: Rc::new(RefCell::new(child)),
            },
        );
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
    pub(super) fn adopt_shadow_realm(&mut self, instance: ObjectId, record: ShadowRealmRecord) {
        let heap_tag = record.vm.borrow().object_prototype.heap;
        self.shadow_realm_by_heap.insert(heap_tag, instance);
        self.shadow_realms.insert(instance, record);
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
        let object_prototype = self.object_prototype;
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
        let function_prototype = self.function_prototype()?;
        self.stack.push(Value::Object(prototype));
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
        self.stack.pop();
        result?;
        self.shadow_realm_prototype = Some(prototype);
        Ok(prototype)
    }

    /// `ShadowRealm.prototype.evaluate` (`PerformShadowRealmEval`). Runs
    /// `source_text` as a classic Script in this instance's own realm and
    /// returns `GetWrappedValue(callerRealm, result)`. Any abrupt
    /// completion -- a parse failure, a thrown value, or an unsettled
    /// promise job's own rejection -- becomes an opaque, message-less
    /// `TypeError` in the *caller's* realm (`CreateTypeErrorCopy`): the
    /// proposal deliberately does not leak another realm's error identity
    /// or message across the boundary.
    pub(super) fn shadow_realm_evaluate(
        &mut self,
        this: Value,
        source_text: Value,
    ) -> Result<Value, RuntimeError> {
        let this_id = self.require_shadow_realm(&this, "evaluate")?;
        let Value::String(source_text) = source_text else {
            return Err(RuntimeError::TypeError(
                "ShadowRealm.prototype.evaluate requires a string".into(),
            ));
        };
        // Parsing happens before an eval execution context is ever pushed:
        // a `HostEnsureCanCompileStrings`/`ParseText` failure throws a real
        // `SyntaxError` directly, distinct from the opaque `TypeError`
        // `PerformShadowRealmEval`'s later *evaluation* step produces on any
        // abrupt completion (`CreateTypeErrorCopy`). Both propagate through
        // the realm whose `evaluate` function is actually executing --
        // ordinarily `self`, or (through `Test262`'s own foreign-call
        // membrane) another realm's `Vm` when `evaluate` was reached via a
        // cross-realm facade, with no extra work required here.
        let source_text = source_text.to_utf8().map_err(|_| {
            RuntimeError::SyntaxError("script source contains an unpaired surrogate".into())
        })?;
        let program =
            crate::parse(&source_text).map_err(|error| RuntimeError::SyntaxError(error.message))?;
        let code = crate::compile(&program)
            .map_err(|error| RuntimeError::SyntaxError(error.to_string()))?;
        // `this_id` names a live record (checked above), so this record is
        // always present; it may still be a *shared* one -- see
        // `ShadowRealmRecord`'s own doc comment -- if it was ever
        // transported into another realm.
        let record = self
            .shadow_realms
            .get(&this_id)
            .cloned()
            .expect("require_shadow_realm confirmed this_id names a live ShadowRealm record");
        return match record.vm.try_borrow_mut() {
            Ok(mut child) => {
                let child_guard = register_active(&mut child);
                let outcome = Self::run_evaluate(self, &mut child, &code);
                drop(child_guard);
                match outcome {
                    Ok(value) => self.shadow_wrap_into(&mut child, value),
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
                let Some(ptr) = heap_tag.and_then(resolve_active) else {
                    return Err(RuntimeError::TypeError(
                        "the ShadowRealm this function belongs to is no longer reachable".into(),
                    ));
                };
                if std::ptr::eq(ptr as *const Vm, self as *const Vm) {
                    return Err(RuntimeError::TypeError(
                        "a ShadowRealm boundary cannot resolve to itself".into(),
                    ));
                }
                // SAFETY: see `ACTIVE`'s documentation and the identical
                // reasoning in `shadow_call_wrapped`.
                let record_vm = unsafe { &mut *ptr };
                let outcome = Self::run_evaluate(self, record_vm, &code);
                match outcome {
                    Ok(value) => self.shadow_wrap_into(record_vm, value),
                    Err(_) => Err(RuntimeError::TypeError(String::new())),
                }
            }
        };
    }

    /// Runs a parsed/compiled `evaluate` body in `child` (registering
    /// `caller` as active for the duration, so `child`'s own execution can
    /// call back into it) and drains `child`'s own promise jobs -- the
    /// shared inner step both of `shadow_realm_evaluate`'s two paths (child
    /// freshly checked out vs. already active further up this call chain)
    /// need identically.
    fn run_evaluate(
        caller: &mut Vm,
        child: &mut Vm,
        code: &Bytecode,
    ) -> Result<Value, RuntimeError> {
        child.remaining_instructions = child.config.instruction_budget;
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
        child.reset_lexical_global_bindings()?;
        let _guard = register_active(caller);
        let value = child.execute_script(code)?;
        // This engine has no realm-independent job queue: a Script's own
        // promise reactions are drained as part of running it to
        // completion, the same synchronous unit `evaluate` presents to its
        // caller.
        child.run_promise_jobs()?;
        Ok(value)
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
        let promise_constructor = self.global("Promise")?;
        let (promise, resolve, reject) = self.new_promise_capability(&promise_constructor)?;
        let record = self
            .shadow_realms
            .get(&this_id)
            .cloned()
            .expect("require_shadow_realm confirmed this_id names a live ShadowRealm record");
        let outcome: Result<Value, RuntimeError> = match record.vm.try_borrow_mut() {
            Ok(mut child) => {
                let inner: Result<Value, RuntimeError> = (|| {
                    // A ShadowRealm has its own module graph and heap, but
                    // shares its agent's host module loader.  Copy every
                    // source registry the host supplied, not only eagerly
                    // compiled bytecode: Test262 deliberately keeps some
                    // valid `importValue` fixtures as lazy dynamic sources.
                    // Do not copy module_graph/source-object caches; their
                    // ObjectIds belong to the caller heap.
                    child.module_registry = self.module_registry.clone();
                    child.dynamic_module_sources = self.dynamic_module_sources.clone();
                    child.json_module_sources = self.json_module_sources.clone();
                    child.module_source_registry = self.module_source_registry.clone();
                    child.active_module_name = self.active_module_name.clone();
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
        match outcome {
            Ok(value) => {
                self.call_native(resolve, Value::Undefined, vec![value], false)?;
            }
            Err(_) => {
                let error = self.error_value(RuntimeError::TypeError(String::new()))?;
                self.call_native(reject, Value::Undefined, vec![error], false)?;
            }
        }
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
    ) -> Result<Value, RuntimeError> {
        if construct {
            return Err(RuntimeError::TypeError(
                "a ShadowRealm wrapped function has no [[Construct]]".into(),
            ));
        }
        let ShadowWrappedFunction {
            home_heap, target, ..
        } = *self
            .shadow_wrapped_functions
            .get(&wrapper)
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
                    let child_guard = register_active(&mut other);
                    let result = self.shadow_call_across(&mut other, target, this_arg, args);
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
        // Otherwise this facade points to a realm not directly reachable
        // from `self`'s own map: either an ancestor `Vm` that is not
        // itself a `ShadowRealm` child (e.g. the embedder's own top-level
        // `Vm`), or -- per the fallback above -- one that is, but is
        // already checked out further up this very call stack. Either way
        // it is reachable only through `ACTIVE` (see its own documentation
        // for why that is sound here).
        let Some(ptr) = resolve_active(home_heap) else {
            return Err(RuntimeError::TypeError(
                "the ShadowRealm this function belongs to is no longer reachable".into(),
            ));
        };
        if std::ptr::eq(ptr as *const Vm, self as *const Vm) {
            // A value that somehow crossed all the way back to its own
            // origin realm within one call chain. Calling through would
            // alias `self` with itself; refuse rather than risk it.
            return Err(RuntimeError::TypeError(
                "a ShadowRealm boundary cannot resolve to itself".into(),
            ));
        }
        // SAFETY: `ptr` is only present in `ACTIVE` for the dynamic extent
        // of a `&mut Vm` call currently suspended further up the Rust call
        // stack (see `ACTIVE`'s documentation); we have just confirmed it
        // is not `self`, so this reborrow does not alias any reference
        // `self` itself is holding.
        let other = unsafe { &mut *ptr };
        self.shadow_call_across(other, target, this_arg, args)
    }

    /// `OrdinaryWrappedFunctionCall`: wraps `this`/each argument *into*
    /// `other`'s realm (`GetWrappedValue(targetRealm, _)`), invokes the
    /// target there, then wraps the result (or a fresh `TypeError` on any
    /// abrupt completion) back into `self`'s realm
    /// (`GetWrappedValue(callerRealm, _)`).
    fn shadow_call_across(
        &mut self,
        other: &mut Vm,
        target: ObjectId,
        this_arg: Value,
        args: Vec<Value>,
    ) -> Result<Value, RuntimeError> {
        let _guard = register_active(self);
        let wrapped_this = other.shadow_wrap_into(&mut *self, this_arg)?;
        let mut wrapped_args = Vec::with_capacity(args.len());
        for arg in args {
            wrapped_args.push(other.shadow_wrap_into(&mut *self, arg)?);
        }
        other.remaining_instructions = other.config.instruction_budget;
        let result = other.call_native(Value::Object(target), wrapped_this, wrapped_args, false);
        match result {
            Ok(value) => self.shadow_wrap_into(other, value),
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
        // alive; root it there for as long as this facade (leaked for the
        // Vm's lifetime, like every other cross-realm record here and in
        // `test262.rs`) can still reach it.
        let _target_root = source.heap.root(target)?;
        let function_prototype = self.function_prototype()?;
        let wrapper = self.with_roots(|heap| {
            heap.alloc_native_function(
                NativeFunction::ShadowRealmWrappedFunction,
                "",
                function_prototype,
            )
        })?;
        self.stack.push(Value::Object(wrapper));
        let result: Result<(), RuntimeError> = (|| {
            self.copy_name_and_length(source, wrapper, target)?;
            Ok(())
        })();
        self.stack.pop();
        // CopyNameAndLength's own abrupt completion (a throwing "length" or
        // "name" getter) becomes a TypeError, never the original error --
        // the same opaque-boundary rule `evaluate`'s CreateTypeErrorCopy
        // applies elsewhere in this proposal.
        result.map_err(|_| {
            RuntimeError::TypeError("could not wrap this function across a ShadowRealm".into())
        })?;
        self.shadow_wrapped_functions.insert(
            wrapper,
            ShadowWrappedFunction {
                home_heap: target.heap,
                target,
                _target_root,
            },
        );
        Ok(Value::Object(wrapper))
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
