// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `import.source()` / `import.defer()` and deferred module namespaces: the
//! phase-specific dynamic-import jobs, the "ready for synchronous execution"
//! analysis over the asynchronous dependency frontier, and the synchronous
//! evaluation a deferred namespace triggers on first observable access.

use super::*;

impl Vm {
    /// Source-phase dynamic import has the same promise and argument
    /// boundary as ordinary import(), but asks the host for a Module Source
    /// object. A source-text module therefore rejects with SyntaxError instead
    /// of linking or evaluating it as an ordinary dynamic import would.
    pub(super) fn dynamic_import_source(
        &mut self,
        promise: ObjectId,
        specifier: &str,
    ) -> Result<(), RuntimeError> {
        let result = (|| {
            let referrer = self
                .active_module_name
                .clone()
                .unwrap_or_else(|| "<script>".to_string());
            let entry = Self::resolve_module_request(&referrer, specifier)?;
            let modules = self.module_registry.clone();
            if modules.contains_key(&entry) {
                return Err(RuntimeError::SyntaxError(
                    "a Source Text Module has no source-phase representation".into(),
                ));
            }
            let source = self.module_source_object(&entry, &modules)?;
            Ok(Value::Object(source))
        })();
        match result {
            Ok(value) => self.settle_promise(promise, PromiseStatus::Fulfilled(value))?,
            Err(error) => {
                let error = self.error_value(error)?;
                self.settle_promise(promise, PromiseStatus::Rejected(error))?;
            }
        }
        Ok(())
    }

    /// ContinueDynamicImport for the `defer` phase: load and link `entry`'s
    /// graph, evaluate only its asynchronous dependency frontier, and hand back
    /// the deferred namespace -- immediately when that frontier is already
    /// idle, otherwise once every module in it has finished.
    pub(super) fn dynamic_import_defer_job(
        &mut self,
        entry: &str,
    ) -> Result<DynamicImportResult, RuntimeError> {
        let modules = self.module_registry.clone();
        self.execute_module_graph_inner(entry, &modules, false, true, ImportPhase::Defer)?;
        let namespace = self
            .last_module_namespace
            .ok_or(RuntimeError::ModuleResolution(format!(
                "dynamic import of {entry} did not produce a namespace"
            )))?;
        let running: Vec<String> = self
            .last_deferred_dependencies
            .iter()
            .filter(|dependency| {
                self.linked_record(dependency)
                    .is_some_and(|record| record.evaluating || record.suspended)
            })
            .cloned()
            .collect();
        if running.is_empty() {
            return Ok(DynamicImportResult::Fulfilled(Value::Object(namespace)));
        }
        Ok(DynamicImportResult::WaitingDeferred {
            namespace,
            modules: running,
        })
    }

    /// IsModuleSCCEvaluated: a module counts as evaluated only once its whole
    /// dependency cycle (over the edges evaluation itself walks: not the
    /// deferred ones) has. A cycle member that already finished can still
    /// belong to a cycle whose asynchronous root is suspended, and anything
    /// that must wait for that root has to see it.
    pub(super) fn scc_evaluated(
        module: &str,
        modules: &HashMap<String, Bytecode>,
        linked: &HashMap<String, LinkedModule>,
    ) -> Result<bool, RuntimeError> {
        if !linked.get(module).is_none_or(|record| record.evaluated) {
            return Ok(false);
        }
        for (other, record) in linked {
            if other != module
                && !record.evaluated
                && Self::module_reaches(module, other, modules, &mut HashSet::new())?
                && Self::module_reaches(other, module, modules, &mut HashSet::new())?
            {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Whether a module's own top-level code awaits (its [[HasTLA]] slot).
    pub(super) fn module_has_tla(code: &Bytecode) -> bool {
        code.instructions()
            .any(|instruction| instruction.opcode == Opcode::Await)
    }

    /// LoadRequestedModules, as far as this host has a load phase: every
    /// module the graph rooted at `entry` requests, whatever the phase, must
    /// be one the host supplied. Nothing may have been evaluated when this
    /// fails, including for a module that is only ever deferred.
    pub(super) fn check_requested_modules_loaded(
        entry: &str,
        modules: &HashMap<String, Bytecode>,
    ) -> Result<(), RuntimeError> {
        let mut seen = HashSet::new();
        let mut pending = vec![entry.to_string()];
        while let Some(name) = pending.pop() {
            if !seen.insert(name.clone()) {
                continue;
            }
            let code = modules.get(&name).ok_or_else(|| {
                RuntimeError::ModuleResolution(format!(
                    "module {name} was not supplied by the host"
                ))
            })?;
            for request in &code.module_requests {
                pending.push(Self::resolve_module_target(
                    &name,
                    &request.module_request,
                    request.module_type,
                )?);
            }
        }
        Ok(())
    }

    /// GatherAsynchronousTransitiveDependencies (import-defer proposal): the
    /// post-order frontier of not-yet-evaluated modules that contain a
    /// top-level `await`. The walk follows every request, of every phase, and
    /// stops at the first asynchronous module of each branch; a module that is
    /// running (synchronously) or already evaluated contributes nothing.
    pub(super) fn gather_async_dependencies(
        module: &str,
        modules: &HashMap<String, Bytecode>,
        linked: &HashMap<String, LinkedModule>,
        seen: &mut HashSet<String>,
    ) -> Result<Vec<String>, RuntimeError> {
        let mut result = Vec::new();
        if !seen.insert(module.to_string()) {
            return Ok(result);
        }
        let (Some(record), Some(code)) = (linked.get(module), modules.get(module)) else {
            return Ok(result);
        };
        // `evaluating` stays set while a module waits on an asynchronous
        // dependency (spec: evaluating-async), which is not the same as being
        // on the synchronous DFS stack.
        if (record.evaluating && !record.suspended) || Self::scc_evaluated(module, modules, linked)?
        {
            return Ok(result);
        }
        if Self::module_has_tla(code) {
            result.push(module.to_string());
            return Ok(result);
        }
        for request in &code.module_requests {
            let target =
                Self::resolve_module_target(module, &request.module_request, request.module_type)?;
            for additional in Self::gather_async_dependencies(&target, modules, linked, seen)? {
                if !result.contains(&additional) {
                    result.push(additional);
                }
            }
        }
        Ok(result)
    }

    /// ReadyForSyncExecution (import-defer proposal): whether `module` and
    /// everything it requests could evaluate to completion without waiting,
    /// i.e. none of it is running, suspended or contains a top-level `await`.
    pub(super) fn ready_for_sync_execution(
        module: &str,
        modules: &HashMap<String, Bytecode>,
        linked: &HashMap<String, LinkedModule>,
        seen: &mut HashSet<String>,
    ) -> Result<bool, RuntimeError> {
        if !seen.insert(module.to_string()) {
            return Ok(true);
        }
        let (Some(record), Some(code)) = (linked.get(module), modules.get(module)) else {
            return Ok(true);
        };
        if Self::scc_evaluated(module, modules, linked)? {
            return Ok(true);
        }
        if record.evaluating || Self::module_has_tla(code) {
            return Ok(false);
        }
        for request in &code.module_requests {
            let target =
                Self::resolve_module_target(module, &request.module_request, request.module_type)?;
            if !Self::ready_for_sync_execution(&target, modules, linked, seen)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Runs `operation` with the module records of the realm's graph, wherever
    /// they currently live: parked for the module code that is running, or in
    /// the installed graph between executions.
    pub(super) fn with_module_records<T>(
        &mut self,
        operation: impl FnOnce(&mut Self, &mut HashMap<String, LinkedModule>) -> T,
    ) -> Result<T, RuntimeError> {
        if let Some(mut linked) = self.evaluating_linked.take() {
            let result = operation(self, &mut linked);
            self.evaluating_linked = Some(linked);
            return Ok(result);
        }
        if let Some(mut graph) = self.module_graph.take() {
            let result = operation(self, &mut graph.linked);
            self.module_graph = Some(graph);
            return Ok(result);
        }
        Err(RuntimeError::TypeError(
            "the module graph of a deferred namespace is not available".into(),
        ))
    }

    /// EvaluateModuleSync for a deferred namespace's module: throws before
    /// evaluating anything when the module could not finish synchronously,
    /// otherwise evaluates it (its deferred dependencies stay deferred) and
    /// rethrows the recorded error if that evaluation failed, now or earlier.
    pub(super) fn evaluate_module_sync(&mut self, module: &str) -> Result<(), RuntimeError> {
        let settled = self.with_module_records(|_, linked| {
            linked
                .get(module)
                .and_then(|record| record.evaluated.then(|| record.error.clone()))
        })?;
        match settled {
            Some(Some(error)) => return Err(RuntimeError::Thrown(error)),
            Some(None) => return Ok(()),
            None => {}
        }
        let modules = self.module_registry.clone();
        let ready = self.with_module_records(|_, linked| {
            Self::ready_for_sync_execution(module, &modules, linked, &mut HashSet::new())
        })??;
        if !ready {
            return Err(RuntimeError::TypeError(
                "a deferred module that is evaluating, suspended or asynchronous cannot be \
                 evaluated synchronously"
                    .into(),
            ));
        }
        // The module body runs in a fresh execution context: displace (and
        // keep rooted) everything the observing code has on the interpreter,
        // exactly as a Promise job that enters a module does.
        let execution = self.suspend_module_execution();
        let roots = self.root_suspended_module_execution(&execution)?;
        let call_depth = std::mem::replace(&mut self.call_depth, 0);
        let result = self
            .with_module_records(|vm, linked| vm.evaluate_module_record(module, &modules, linked));
        if let Some(root) = self.result_root.take() {
            self.heap.unroot(root)?;
        }
        self.restore_module_execution(execution);
        self.call_depth = call_depth;
        for root in roots {
            self.heap.unroot(root)?;
        }
        result?.map(|_| ())
    }

    /// A deferred namespace's own-property operations on string keys other
    /// than `"then"` (and its [[OwnPropertyKeys]]) first evaluate the module
    /// synchronously; symbols and `"then"` behave like on any ordinary object,
    /// and [[GetPrototypeOf]], [[SetPrototypeOf]], [[IsExtensible]],
    /// [[PreventExtensions]] and [[Set]] never trigger. `key` is `None` for
    /// [[OwnPropertyKeys]].
    pub(in super::super) fn trigger_deferred_namespace(
        &mut self,
        object: ObjectId,
        key: Option<&PropertyName>,
    ) -> Result<(), RuntimeError> {
        if self.deferred_namespaces.is_empty() {
            return Ok(());
        }
        if let Some(key) = key {
            if !matches!(key, PropertyName::String(_)) || key == "then" {
                return Ok(());
            }
        }
        let Some(module) = self.deferred_namespaces.get(&object).cloned() else {
            return Ok(());
        };
        self.evaluate_module_sync(&module)
    }

    /// The evaluation error a finished module inherits from its cycle root.
    /// When an asynchronous cycle fails, only the module that threw and its
    /// async parents record the error; a cycle member that had already
    /// finished stays `evaluated` with no error of its own. Evaluate() on such
    /// a member is redirected to its [[CycleRoot]], whose recorded
    /// [[EvaluationError]] is returned again -- so a later import of the member
    /// must reject with that same error rather than fulfill. Here that is: an
    /// errored record in the same strongly connected component as `module`.
    pub(super) fn cycle_root_error(
        module: &str,
        modules: &HashMap<String, Bytecode>,
        linked: &HashMap<String, LinkedModule>,
    ) -> Result<Option<Value>, RuntimeError> {
        let mut candidates: Vec<_> = linked
            .iter()
            .filter(|(name, record)| name.as_str() != module && record.error.is_some())
            .map(|(name, _)| name.as_str())
            .collect();
        candidates.sort_unstable();
        for candidate in candidates {
            if Self::module_reaches(module, candidate, modules, &mut HashSet::new())?
                && Self::module_reaches(candidate, module, modules, &mut HashSet::new())?
            {
                return Ok(linked[candidate].error.clone());
            }
        }
        Ok(None)
    }

    /// Materializes the host identity supplied for a source-phase import.
    /// Source Text Modules deliberately do not expose such a representation:
    /// accepting bytecode here would accidentally link or evaluate a module
    /// whose import phase must remain opaque.
    pub(in super::super) fn module_source_object(
        &mut self,
        module: &str,
        modules: &HashMap<String, Bytecode>,
    ) -> Result<ObjectId, RuntimeError> {
        if let Some(source) = self.module_source_cache.get(module) {
            return Ok(*source);
        }
        if modules.contains_key(module) {
            return Err(RuntimeError::ModuleResolution(format!(
                "{module} is a Source Text Module and has no source-phase representation"
            )));
        }
        if !self.module_source_registry.contains(module) {
            return Err(RuntimeError::TypeError(format!(
                "host did not provide a source-phase representation for {module}"
            )));
        }
        let prototype = self
            .abstract_module_source_prototype
            .unwrap_or(self.object_prototype);
        let source = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        let root = self.heap.root(source)?;
        self.module_source_cache.insert(module.to_string(), source);
        self.module_source_roots.insert(module.to_string(), root);
        Ok(source)
    }
}
