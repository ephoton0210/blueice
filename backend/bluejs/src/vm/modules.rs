// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

mod deferred;
mod namespace;
mod synthetic;

use super::*;

impl Vm {
    /// The Dynamic Import job owns its own observable queue turns. It must
    /// link/evaluate a module graph without recursively draining those jobs;
    /// otherwise a sibling dynamic import can start and finish before the
    /// importing job has installed its completion reaction.
    pub(super) fn execute_module_graph_inner(
        &mut self,
        entry: &str,
        modules: &HashMap<String, Bytecode>,
        drain_jobs: bool,
        is_dynamic_import: bool,
        phase: ImportPhase,
    ) -> Result<Value, RuntimeError> {
        // An import that starts while module code is running (a dynamic
        // `import()` in a Promise job the entry's `await` lets run) joins the
        // graph that code belongs to instead of linking a second copy of it.
        let mut nested_in_evaluation = false;
        let (mut linked, mut roots, fresh_graph) = match self.module_graph.take() {
            Some(graph) => (graph.linked, graph.roots, false),
            None => match self.evaluating_linked.take() {
                Some(linked) => {
                    nested_in_evaluation = true;
                    (linked, Vec::new(), false)
                }
                None => (HashMap::new(), Vec::new(), true),
            },
        };
        // `ensure_synthetic_module`/`ensure_dynamic_module_compiled` (below) can
        // charge VM steps or allocate; give them a real budget before either
        // can run. The closure below resets this again for the graph's own
        // execution regardless.
        self.remaining_instructions = self.config.instruction_budget;
        // Own the supplied module set so a request needing on-demand
        // synthesis or compilation -- `entry` itself (a direct dynamic
        // `import()`, `type`-attributed or an ordinary module reachable only
        // dynamically) or one discovered while scanning a fresh static
        // graph's own `with`-attributed requests -- can be resolved into it
        // before anything below reads `modules`. Rooted through the same
        // `roots` this function already threads through its whole closure,
        // so a freshly parsed/compiled value survives the major collection
        // a few lines below before its cell exists.
        let mut owned_modules;
        let modules: &HashMap<String, Bytecode> = if is_dynamic_import || fresh_graph {
            owned_modules = modules.clone();
            let prepared = (|| {
                if is_dynamic_import {
                    // `entry` is already the resolved target: `dynamic_import_job`
                    // resolves the specifier against its referrer before this call.
                    if ModuleType::split_module_key(entry).1 == ModuleType::JavaScript {
                        self.ensure_dynamic_module_compiled(entry, &mut owned_modules)?;
                    } else {
                        self.ensure_synthetic_module(entry, &mut owned_modules, &mut roots)?;
                    }
                }
                if fresh_graph {
                    self.register_static_synthetic_modules(&mut owned_modules, &mut roots)?;
                }
                Ok(())
            })();
            if let Err(error) = prepared {
                // A module that cannot even be compiled or synthesized fails
                // only this request; an existing graph stays usable.
                if fresh_graph {
                    for root in roots {
                        self.heap.unroot(root)?;
                    }
                } else {
                    self.store_module_graph(
                        ModuleGraphState { linked, roots },
                        nested_in_evaluation,
                    );
                }
                return Err(error);
            }
            &owned_modules
        } else {
            modules
        };
        // Every module in `modules` not already in `linked` is new to this
        // graph -- either every one of them, on a fresh graph, or just the
        // one(s) `ensure_synthetic_module`/`ensure_dynamic_module_compiled` (or a
        // fresh static scan) added just above to an *existing* graph. The
        // per-`new_names` linking pass below (inside the closure) treats
        // both cases identically, so a synthetic module or an ordinary module
        // compiled on demand for a dynamic import gets exactly the same
        // real linking (cell creation, export validation, import aliasing,
        // declaration instantiation) a fresh graph's own modules get.
        let mut order: Vec<_> = modules.keys().cloned().collect();
        order.sort_unstable();
        let new_names: Vec<String> = order
            .iter()
            .filter(|name| !linked.contains_key(name.as_str()))
            .cloned()
            .collect();
        // Roots pushed from here on, until this call's own success/failure
        // is known, belong only to `new_names`'s delta -- on a linking
        // failure for that delta (not a fresh graph, which instead discards
        // every root wholesale below), only these need unrooting, and only
        // `new_names` need removing from `linked`, so a distinct later
        // operation on the rest of an existing graph is unaffected and a
        // retried dynamic import of the same specifier is treated as new
        // again rather than silently resuming a half-linked record.
        let roots_checkpoint = roots.len();
        // Once a module body has started running, its evaluation error (or
        // completion) belongs to the module record and is observed again by
        // every later importer, so the graph is kept rather than rolled back.
        let mut evaluation_started = false;
        let mut result = (|| {
            self.module_registry = modules.clone();
            // Resolution errors can be converted to a dynamic-import Promise
            // rejection before graph setup reaches the usual execution reset.
            // Give that host-visible error construction a fresh budget too.
            self.remaining_instructions = self.config.instruction_budget;
            // The host supplies the closed source set for this realm; `order`/
            // `new_names` (computed above, before this closure) link every
            // newly-supplied record once so a later dynamic import shares a
            // static import's cells instead of constructing a second module
            // instance for the same canonical path.
            if let Some(root) = self.result_root.take() {
                self.heap.unroot(root)?;
            }
            if let Some(root) = self.last_module_namespace_root.take() {
                self.heap.unroot(root)?;
            }
            self.last_module_namespace = None;
            self.with_roots(|heap| {
                heap.collect_major();
                Ok(())
            })?;
            self.enqueue_finalization_cleanup_jobs();
            self.stack.clear();
            self.bindings.clear();
            self.binding_metadata.clear();
            self.cells.clear();
            self.active_scopes.clear();
            self.active_scope_slots.clear();
            self.with_objects.clear();
            self.script_global_slots.clear();
            self.dynamic_eval_bindings.clear();
            self.eval_dynamic_slots.clear();
            self.dynamic_eval_outer_bindings.clear();
            self.completion = Value::Undefined;
            self.completion_empty = true;
            self.remaining_instructions = self.config.instruction_budget;
            self.this = Value::Undefined;
            self.top_level_module = true;

            if !new_names.is_empty() {
                linked.extend(new_names.iter().cloned().map(|name| {
                    (
                        name,
                        LinkedModule {
                            cells: HashMap::new(),
                            namespace: None,
                            evaluated: false,
                            evaluating: false,
                            suspended: false,
                            completion: None,
                            error: None,
                            deferred_namespace: None,
                        },
                    )
                }));

                // ModuleDeclarationInstantiation creates all own bindings before
                // wiring imports.  A `var` binding is initialized immediately;
                // lexical bindings deliberately have no `value` property yet.
                for name in &new_names {
                    let code = modules
                        .get(name)
                        .expect("reachable module was checked during collection");
                    if !code.module {
                        return Err(RuntimeError::ModuleResolution(format!(
                            "{name} was not compiled using the module goal"
                        )));
                    }
                    let imported_slots: HashSet<_> = code
                        .module_imports
                        .iter()
                        .filter_map(|import| import.local_slot.map(|slot| slot as usize))
                        .collect();
                    let slots = code.scopes.first().cloned().unwrap_or_default();
                    let record = linked
                        .get_mut(name)
                        .expect("linked record was allocated for every module");
                    for slot in slots {
                        let slot = slot as usize;
                        if imported_slots.contains(&slot) {
                            continue;
                        }
                        let cell = self.with_roots(|heap| heap.alloc_object(None))?;
                        roots.push(self.heap.root(cell)?);
                        if !code.bindings[slot].lexical {
                            self.with_roots(|heap| heap.set(cell, "value", Value::Undefined))?;
                        }
                        record.cells.insert(slot, cell);
                    }
                }

                // A synthesized module (see `ensure_synthetic_module`) has no
                // bytecode body to run: its "default" slot already got an
                // ordinary `Undefined`-valued cell from the generic loop just
                // above (matching any other non-lexical binding), so
                // overwrite that cell with the already-built value here and
                // mark it evaluated -- `evaluate_module_record` never touches
                // its (empty) instruction stream for a record already
                // marked evaluated.
                for name in &new_names {
                    let code = modules
                        .get(name)
                        .expect("reachable module was checked during collection");
                    if let Some(value) = code.synthetic_default_export.clone() {
                        let record = linked
                            .get_mut(name)
                            .expect("linked record was allocated for every module");
                        let cell = *record
                            .cells
                            .get(&0)
                            .expect("synthetic module's default slot has a cell");
                        self.with_roots(|heap| heap.set(cell, "value", value))?;
                        record.evaluated = true;
                    }
                }

                // Validate every named indirect export before evaluating any
                // module body.  A missing or ambiguous `export { x } from ...`
                // is a ModuleDeclarationInstantiation error, including when the
                // body would otherwise call $DONOTEVALUATE().  Star exports do
                // not themselves fail here; their ambiguity matters only when a
                // particular name is resolved by an import or indirect export.
                for name in &new_names {
                    let code = modules
                        .get(name)
                        .expect("reachable module was checked during collection");
                    for export in &code.module_exports {
                        let ModuleExport::Indirect {
                            export_name,
                            module_request,
                            import_name,
                            module_type,
                        } = export
                        else {
                            continue;
                        };
                        let target =
                            Self::resolve_module_target(name, module_request, *module_type)?;
                        match Self::resolve_export(modules, &target, import_name, &mut Vec::new())?
                        {
                            ExportResolution::Binding { .. }
                            | ExportResolution::Namespace { .. }
                            | ExportResolution::DeferredNamespace { .. }
                            | ExportResolution::Source { .. } => {}
                            ExportResolution::Missing | ExportResolution::Ambiguous => {
                                return Err(RuntimeError::ModuleResolution(format!(
                                "{module_request} does not export {import_name} for {export_name}"
                            )));
                            }
                        }
                    }
                    for export in &code.module_exports {
                        let ModuleExport::Source { module_request, .. } = export else {
                            continue;
                        };
                        let target = Self::resolve_module_request(name, module_request)?;
                        self.module_source_object(&target, modules)?;
                    }
                    // Source-phase requests are resolved while loading the graph,
                    // before ordinary ModuleDeclarationInstantiation validates
                    // named imports elsewhere in that graph. This keeps a host
                    // failure to supply a source record distinct from a later
                    // SyntaxError linking failure.
                    for import in &code.module_imports {
                        if !matches!(import.import_name, ModuleImportName::Source) {
                            continue;
                        }
                        let target = Self::resolve_module_request(name, &import.module_request)?;
                        self.module_source_object(&target, modules)?;
                    }
                }

                // Import bindings are immutable aliases.  The importer stores the
                // exporter's *cell*, so later stores in the exporting module are
                // visible without any copy or notification mechanism.
                for name in &new_names {
                    let code = modules
                        .get(name)
                        .expect("reachable module was checked during collection");
                    let mut aliases = Vec::new();
                    for import in &code.module_imports {
                        let Some(local_slot) = import.local_slot else {
                            continue;
                        };
                        // A source-phase import names the host's source record
                        // for the resource itself, whatever its `type` says.
                        let target = if matches!(import.import_name, ModuleImportName::Source) {
                            Self::resolve_module_request(name, &import.module_request)?
                        } else {
                            Self::resolve_module_target(
                                name,
                                &import.module_request,
                                import.module_type,
                            )?
                        };
                        let resolution = match &import.import_name {
                            ModuleImportName::Named(import_name) => Self::resolve_export(
                                modules,
                                &target,
                                import_name,
                                &mut Vec::new(),
                            )?,
                            ModuleImportName::Namespace => ExportResolution::Namespace {
                                module: target.clone(),
                            },
                            ModuleImportName::DeferredNamespace => {
                                ExportResolution::DeferredNamespace {
                                    module: target.clone(),
                                }
                            }
                            ModuleImportName::Source => ExportResolution::Source {
                                module: target.clone(),
                            },
                        };
                        let deferred_target =
                            matches!(resolution, ExportResolution::DeferredNamespace { .. });
                        let cell = match resolution {
                            ExportResolution::Binding {
                                module: exporter,
                                slot,
                            } => linked
                                .get(&exporter)
                                .and_then(|record| record.cells.get(&slot))
                                .copied()
                                .ok_or_else(|| {
                                    RuntimeError::ModuleResolution(format!(
                                        "export binding from {exporter} has no cell"
                                    ))
                                })?,
                            ExportResolution::Namespace { module }
                            | ExportResolution::DeferredNamespace { module } => {
                                let namespace = self.module_namespace(
                                    &module,
                                    deferred_target,
                                    modules,
                                    &mut linked,
                                    &mut roots,
                                )?;
                                let cell = self.with_roots(|heap| heap.alloc_object(None))?;
                                roots.push(self.heap.root(cell)?);
                                self.with_roots(|heap| {
                                    heap.set(cell, "value", Value::Object(namespace))
                                })?;
                                cell
                            }
                            ExportResolution::Source { module } => {
                                let source = self.module_source_object(&module, modules)?;
                                let cell = self.with_roots(|heap| heap.alloc_object(None))?;
                                roots.push(self.heap.root(cell)?);
                                self.with_roots(|heap| {
                                    heap.set(cell, "value", Value::Object(source))
                                })?;
                                cell
                            }
                            ExportResolution::Missing | ExportResolution::Ambiguous => {
                                let import_name = match &import.import_name {
                                    ModuleImportName::Named(name) => name.as_str(),
                                    ModuleImportName::Namespace
                                    | ModuleImportName::DeferredNamespace => "*",
                                    ModuleImportName::Source => "source",
                                };
                                return Err(RuntimeError::ModuleResolution(format!(
                                    "{} does not export {import_name}",
                                    import.module_request
                                )));
                            }
                        };
                        aliases.push((local_slot as usize, cell));
                    }
                    let record = linked
                        .get_mut(name)
                        .expect("linked record was allocated for every module");
                    for (slot, cell) in aliases {
                        record.cells.insert(slot, cell);
                    }
                }

                // Run declaration instantiation for every reachable module before
                // evaluating any body.  This makes function exports callable
                // across a cycle, while lexical exports remain in their TDZ.
                for name in &new_names {
                    let code = modules
                        .get(name)
                        .expect("reachable module was checked during collection");
                    let record = linked
                        .get_mut(name)
                        .expect("linked record was allocated for every module");
                    self.initialize_module_record(name, code, &mut record.cells)?;
                }
            }

            // LoadRequestedModules has to have succeeded for every phase before
            // anything is evaluated: a missing module fails the whole request
            // even when the only edge to it is a deferred one.
            Self::check_requested_modules_loaded(entry, modules)?;
            evaluation_started = true;
            let (value, namespace) = if phase == ImportPhase::Defer {
                // A deferred import evaluates nothing of `entry` itself; only
                // the asynchronous part of its dependency graph runs now.
                let dependencies =
                    Self::gather_async_dependencies(entry, modules, &linked, &mut HashSet::new())?;
                for dependency in &dependencies {
                    self.evaluate_module_record(dependency, modules, &mut linked)?;
                }
                self.last_deferred_dependencies = dependencies;
                let namespace =
                    self.module_namespace(entry, true, modules, &mut linked, &mut roots)?;
                (Value::Undefined, namespace)
            } else {
                let value = self.evaluate_module_record(entry, modules, &mut linked)?;
                let namespace =
                    self.module_namespace(entry, false, modules, &mut linked, &mut roots)?;
                (value, namespace)
            };
            self.last_module_namespace = Some(namespace);
            self.last_module_namespace_root = Some(self.heap.root(namespace)?);
            if let Value::Object(id) = value {
                self.result_root = Some(self.heap.root(id)?);
            }
            Ok(value)
        })();

        // Keep an abrupt object completion observable to the embedding host
        // until the next execution, just as `execute_with_global_bindings`
        // does for scripts.  In particular, Test262 needs to inspect an
        // Error's `name` after a module evaluation rejects.
        if let Err(RuntimeError::Thrown(Value::Object(id))) = &result {
            self.result_root = Some(self.heap.root(*id)?);
        }

        if result.is_err() && evaluation_started {
            self.store_module_graph(ModuleGraphState { linked, roots }, nested_in_evaluation);
        } else if result.is_err() {
            if fresh_graph {
                // The whole graph never became usable; discard everything.
                for root in roots {
                    self.heap.unroot(root)?;
                }
            } else {
                // Only `new_names`'s delta was attempted against an existing,
                // still-otherwise-valid graph: roll back just that delta (see
                // `roots_checkpoint`'s own comment above) and keep the rest,
                // so an unrelated later operation on the existing graph is
                // unaffected and a retried dynamic import of the same
                // specifier is treated as new again.
                for name in &new_names {
                    linked.remove(name);
                }
                for root in roots.split_off(roots_checkpoint) {
                    self.heap.unroot(root)?;
                }
                self.store_module_graph(ModuleGraphState { linked, roots }, nested_in_evaluation);
            }
        } else {
            self.store_module_graph(ModuleGraphState { linked, roots }, nested_in_evaluation);
        }
        // A module can become asynchronous solely through a dependency.  The
        // host-facing evaluation path must advance that dependency's queued
        // continuation just as it does for an entry containing `await`
        // itself, otherwise an immediately rejected imported module is
        // reported as a successful evaluation.
        let entry_is_async = self
            .linked_record(entry)
            .is_some_and(|record| record.suspended)
            || modules.get(entry).is_some_and(|code| {
                code.instructions()
                    .any(|instruction| instruction.opcode == Opcode::Await)
            });
        if result.is_ok() && drain_jobs && entry_is_async && !self.promise_jobs.is_empty() {
            self.remaining_instructions = self.config.instruction_budget;
            if let Err(error) = self.run_promise_jobs() {
                result = Err(error);
            }
        }
        if result.is_ok() && phase != ImportPhase::Defer {
            if let Some(error) = self
                .linked_record(entry)
                .and_then(|record| record.error.clone())
            {
                result = Err(RuntimeError::Thrown(error));
            }
        }
        if result.is_ok() && phase != ImportPhase::Defer {
            if let Some(value) = self
                .linked_record(entry)
                .and_then(|record| record.completion.clone())
            {
                result = Ok(value);
            }
        }
        self.stack.clear();
        self.bindings.clear();
        self.binding_metadata.clear();
        self.cells.clear();
        self.active_scopes.clear();
        self.active_scope_slots.clear();
        self.with_objects.clear();
        self.script_global_slots.clear();
        self.dynamic_eval_bindings.clear();
        self.eval_dynamic_slots.clear();
        self.dynamic_eval_outer_bindings.clear();
        self.completion = Value::Undefined;
        self.completion_empty = true;
        self.top_level_module = false;
        self.pending_completions.clear();
        self.completion_saves.clear();
        self.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })?;
        self.enqueue_finalization_cleanup_jobs();
        result
    }

    /// Compiles a module reachable only through a dynamic import, on
    /// demand, from host-supplied raw JavaScript text -- the counterpart to
    /// `ensure_json_module` for ordinary (non-JSON) modules the host could
    /// not (or, per the Test262 harness's own reason for this function,
    /// deliberately did not) supply as pre-compiled `Bytecode` up front.
    ///
    /// This exists for exactly one reason: a module that is syntactically
    /// or semantically invalid *only as a module* (e.g. a lexical/`var`
    /// name conflict that is perfectly valid script code) must still fail
    /// lazily, as this dynamic import's own promise rejection, not as an
    /// eager failure before the importing code ever runs -- which is
    /// exactly what happens if a host eagerly parses/compiles every
    /// transitively-reachable sibling before execution starts, whether or
    /// not the entry's own static graph actually needs it. A host that
    /// already has every module pre-compiled (the common case: an ordinary
    /// static or dynamic import of a module also reachable statically)
    /// never reaches this at all, since `modules.contains_key` is checked
    /// first.
    ///
    /// A parse or compile failure becomes a real `RuntimeError::SyntaxError`
    /// (matching `indirect_eval`'s own parse/compile-error mapping), which
    /// -- reached only through a dynamic import's own job -- rejects that
    /// import's promise with a real `SyntaxError` object via `error_value`,
    /// never surfacing as a harness-level or whole-graph failure. `target`
    /// must already be the *resolved* module name. A target this host has
    /// no raw source for at all is left alone (not an error here): the
    /// existing "module was not linked" `ModuleResolution` fallback
    /// (`evaluate_module_record`) still applies exactly as before this
    /// function existed.
    fn ensure_dynamic_module_compiled(
        &mut self,
        target: &str,
        modules: &mut HashMap<String, Bytecode>,
    ) -> Result<(), RuntimeError> {
        if modules.contains_key(target) {
            return Ok(());
        }
        let Some(source) = self.dynamic_module_sources.get(target).cloned() else {
            return Ok(());
        };
        let program = crate::parse_module(&source)
            .map_err(|error| RuntimeError::SyntaxError(error.message))?;
        let code = crate::compile_module_with_limit(&program, u32::MAX)
            .map_err(|error| RuntimeError::SyntaxError(error.to_string()))?;
        modules.insert(target.to_string(), code);
        Ok(())
    }

    /// The module-registry key a request names: its specifier resolved
    /// against `referrer`, qualified by the request's module type.
    pub(super) fn resolve_module_target(
        referrer: &str,
        request: &str,
        module_type: ModuleType,
    ) -> Result<String, RuntimeError> {
        Ok(module_type.module_key(&Self::resolve_module_request(referrer, request)?))
    }

    pub(super) fn resolve_module_request(
        referrer: &str,
        request: &str,
    ) -> Result<String, RuntimeError> {
        if request.starts_with("./") || request.starts_with("../") {
            let mut parts: Vec<&str> = referrer.split('/').collect();
            if parts.len() > 1 {
                parts.pop();
            } else {
                parts.clear();
            }
            for part in request.split('/') {
                match part {
                    "" | "." => {}
                    ".." => {
                        if parts.pop().is_none() {
                            return Err(RuntimeError::ModuleResolution(format!(
                                "relative module request {request} escapes its host root"
                            )));
                        }
                    }
                    part => parts.push(part),
                }
            }
            return Ok(parts.join("/"));
        }
        Ok(request.to_string())
    }

    /// Return the per-Source-Text-Module ImportMeta object. The host supplies
    /// no URL-like fields in this embedding, but the required null prototype
    /// and module-local identity are observable and must be stable.
    pub(super) fn import_meta(&mut self) -> Result<Value, RuntimeError> {
        let module = self
            .active_module_name
            .clone()
            .unwrap_or_else(|| "<module>".to_string());
        if let Some(meta) = self.module_import_meta.get(&module) {
            return Ok(Value::Object(*meta));
        }
        let meta = self.with_roots(|heap| heap.alloc_object(None))?;
        let root = self.heap.root(meta)?;
        self.module_import_meta.insert(module.clone(), meta);
        self.module_import_meta_roots.insert(module, root);
        Ok(Value::Object(meta))
    }

    /// Dynamic `import()` first creates a Promise capability, then defers all
    /// resolution, linking and evaluation to the realm job queue.  The host
    /// registry is deliberately the same finite registry used for static
    /// module graphs, so no JavaScript source can escape the supplied tree.
    ///
    /// `options` is the ImportCall's optional second argument
    /// (`import(specifier, options)`); per EvaluateImportCall it is always
    /// evaluated and validated synchronously, with every abrupt completion
    /// (including a non-string `with` attribute value or a thrown getter)
    /// rejecting the returned promise rather than propagating as a
    /// synchronous throw -- only evaluating the two argument expressions
    /// themselves (already done by the compiler before this opcode fires)
    /// happens outside the promise's `IfAbruptRejectPromise` boundary.
    pub(super) fn dynamic_import(
        &mut self,
        specifier: Value,
        options: Value,
        phase: ImportPhase,
    ) -> Result<Value, RuntimeError> {
        // The operands (already popped by the caller) and the promise are
        // otherwise only in Rust locals while the promise is allocated and the
        // arguments are coerced (user code that allocates): keep them on the
        // operand stack, a GC root, until the import is queued or settled.
        let base = self.stack.len();
        self.stack.extend([specifier.clone(), options.clone()]);
        let result = (|| {
            let promise = self.new_promise()?;
            self.stack.push(Value::Object(promise));
            match self.evaluate_import_call_arguments(specifier, options) {
                Ok((specifier, _)) if phase == ImportPhase::Source => {
                    self.dynamic_import_source(promise, &specifier)?;
                }
                Ok((specifier, module_type)) => {
                    let referrer = self
                        .active_module_name
                        .clone()
                        .unwrap_or_else(|| "<script>".to_string());
                    self.promise_jobs.push_back(PromiseJob::DynamicImport {
                        target: promise,
                        referrer,
                        specifier,
                        module_type,
                        phase,
                    });
                }
                Err(error) => {
                    let error = self.error_value(error)?;
                    self.settle_promise(promise, PromiseStatus::Rejected(error))?;
                }
            }
            Ok(Value::Object(promise))
        })();
        self.stack.truncate(base);
        result
    }

    /// The specifier-ToString and options-validation steps of
    /// EvaluateImportCall, i.e. everything between "NewPromiseCapability"
    /// and "HostImportModuleDynamically". Attribute keys/values are
    /// validated, and the `type` attribute is detected (routing the eventual
    /// import through `ensure_synthetic_module` for `json`, `text` and
    /// `bytes`); every other attribute key/value is accepted here exactly
    /// like this host's static `import ... with {...}` attributes, without
    /// otherwise varying module resolution. Returns the specifier and the
    /// module type the `type` attribute selected.
    fn evaluate_import_call_arguments(
        &mut self,
        specifier: Value,
        options: Value,
    ) -> Result<(String, ModuleType), RuntimeError> {
        let specifier = self.coerce_string(&specifier)?;
        let specifier = specifier.to_utf8().map_err(|_| {
            RuntimeError::TypeError("module specifier is not a Unicode string".into())
        })?;
        let mut module_type = ModuleType::JavaScript;
        if options != Value::Undefined {
            if !matches!(options, Value::Object(_)) {
                return Err(RuntimeError::TypeError(
                    "import() options argument must be an object".into(),
                ));
            }
            let attributes = self.get_property(&options, &"with".into())?;
            if attributes != Value::Undefined {
                let Value::Object(attributes_id) = attributes else {
                    return Err(RuntimeError::TypeError(
                        "import() attributes value must be an object".into(),
                    ));
                };
                // EnumerableOwnPropertyNames(attributesObj, KEY): own,
                // String-keyed (never Symbol), and enumerable per a
                // re-checked [[GetOwnProperty]] -- the same algorithm as
                // Object.keys, including Proxy ownKeys/getOwnPropertyDescriptor
                // trap observance.
                let keys = self.object_own_property_keys(attributes_id)?;
                for key in keys {
                    if !matches!(key, PropertyName::String(_)) {
                        continue;
                    }
                    let Some(descriptor) = self.object_get_own_property(attributes_id, &key)?
                    else {
                        continue;
                    };
                    if descriptor.enumerable != Some(true) {
                        continue;
                    }
                    let Value::String(value) = self.get_property(&attributes, &key)? else {
                        return Err(RuntimeError::TypeError(
                            "import attribute values must be strings".into(),
                        ));
                    };
                    if key == "type" {
                        module_type = value.to_utf8().map_or(ModuleType::JavaScript, |value| {
                            ModuleType::from_attribute_value(&value)
                        });
                    }
                }
            }
        }
        Ok((specifier, module_type))
    }

    pub(super) fn dynamic_import_job(
        &mut self,
        referrer: &str,
        specifier: &str,
        module_type: ModuleType,
        phase: ImportPhase,
    ) -> Result<DynamicImportResult, RuntimeError> {
        let entry = Self::resolve_module_target(referrer, specifier, module_type)?;
        if phase == ImportPhase::Defer {
            return self.dynamic_import_defer_job(&entry);
        }
        if let Some(record) = self.linked_record(&entry) {
            if let Some(error) = &record.error {
                return Err(RuntimeError::Thrown(error.clone()));
            }
            if record.evaluated {
                let modules = self.module_registry.clone();
                return self.with_module_records(|vm, linked| {
                    if let Some(error) = Self::cycle_root_error(&entry, &modules, linked)? {
                        return Err(RuntimeError::Thrown(error));
                    }
                    // The namespace already exists for an evaluated module;
                    // nothing new is rooted through this scratch list.
                    vm.module_namespace(&entry, false, &modules, linked, &mut Vec::new())
                        .map(|namespace| DynamicImportResult::Fulfilled(Value::Object(namespace)))
                })?;
            }
            if record.evaluating || record.suspended {
                return Ok(DynamicImportResult::Waiting(entry));
            }
        }
        let modules = self.module_registry.clone();
        self.execute_module_graph_inner(&entry, &modules, false, true, ImportPhase::Evaluation)?;
        if self
            .linked_record(&entry)
            .is_some_and(|record| record.evaluating || record.suspended)
        {
            return Ok(DynamicImportResult::Waiting(entry));
        }
        let namespace = self
            .last_module_namespace
            .ok_or(RuntimeError::ModuleResolution(format!(
                "dynamic import of {entry} did not produce a namespace"
            )))?;
        Ok(DynamicImportResult::Fulfilled(Value::Object(namespace)))
    }

    /// The record of `name` in whichever graph currently owns the module
    /// records: the installed one, or the one parked for running module code.
    fn linked_record(&self, name: &str) -> Option<&LinkedModule> {
        self.module_graph
            .as_ref()
            .map(|graph| &graph.linked)
            .or(self.evaluating_linked.as_ref())
            .and_then(|linked| linked.get(name))
    }

    /// Hands the module records back to where `execute_module_graph_inner`
    /// found them: the installed graph, or -- for an import that started
    /// while module code was running -- the parking spot of that evaluation.
    /// A nested load cannot reach the root list of the evaluation it joined,
    /// so its roots wait in `nested_module_roots` until the graph is next
    /// installed, which then owns them like every other root of its modules.
    pub(super) fn store_module_graph(
        &mut self,
        mut state: ModuleGraphState,
        nested_in_evaluation: bool,
    ) {
        if nested_in_evaluation {
            self.evaluating_linked = Some(state.linked);
            self.nested_module_roots.append(&mut state.roots);
        } else {
            state.roots.append(&mut self.nested_module_roots);
            self.module_graph = Some(state);
        }
    }

    pub(super) fn suspend_module_execution(&mut self) -> SuspendedModuleExecution {
        SuspendedModuleExecution {
            result_root: self.result_root.take(),
            stack: std::mem::take(&mut self.stack),
            bindings: std::mem::take(&mut self.bindings),
            binding_metadata: std::mem::take(&mut self.binding_metadata),
            completion: std::mem::replace(&mut self.completion, Value::Undefined),
            completion_empty: std::mem::replace(&mut self.completion_empty, true),
            active_scopes: std::mem::take(&mut self.active_scopes),
            active_scope_slots: std::mem::take(&mut self.active_scope_slots),
            with_objects: std::mem::take(&mut self.with_objects),
            pending_completions: std::mem::take(&mut self.pending_completions),
            completion_saves: std::mem::take(&mut self.completion_saves),
            remaining_instructions: std::mem::replace(&mut self.remaining_instructions, 0),
            cells: std::mem::take(&mut self.cells),
            dynamic_eval_bindings: std::mem::take(&mut self.dynamic_eval_bindings),
            eval_dynamic_slots: std::mem::take(&mut self.eval_dynamic_slots),
            dynamic_eval_outer_bindings: std::mem::take(&mut self.dynamic_eval_outer_bindings),
            this: std::mem::replace(&mut self.this, Value::Undefined),
            arguments: std::mem::take(&mut self.arguments),
            callee: std::mem::replace(&mut self.callee, Value::Undefined),
            strict: std::mem::replace(&mut self.strict, false),
            top_level_module: std::mem::replace(&mut self.top_level_module, false),
            script_global_slots: std::mem::take(&mut self.script_global_slots),
            variable_scope: std::mem::replace(&mut self.variable_scope, 0),
            variable_scope_lexicals: std::mem::take(&mut self.variable_scope_lexicals),
            templates: std::mem::take(&mut self.templates),
            new_target: std::mem::replace(&mut self.new_target, Value::Undefined),
            new_target_allowed: std::mem::replace(&mut self.new_target_allowed, false),
            home_object: self.home_object.take(),
            class_constructor: self.class_constructor.take(),
            class_field_initializer_depth: std::mem::replace(
                &mut self.class_field_initializer_depth,
                0,
            ),
            active_module_name: self.active_module_name.take(),
        }
    }

    pub(super) fn restore_module_execution(&mut self, execution: SuspendedModuleExecution) {
        self.result_root = execution.result_root;
        self.stack = execution.stack;
        self.bindings = execution.bindings;
        self.binding_metadata = execution.binding_metadata;
        self.completion = execution.completion;
        self.completion_empty = execution.completion_empty;
        self.active_scopes = execution.active_scopes;
        self.active_scope_slots = execution.active_scope_slots;
        self.with_objects = execution.with_objects;
        self.pending_completions = execution.pending_completions;
        self.completion_saves = execution.completion_saves;
        self.remaining_instructions = execution.remaining_instructions;
        self.cells = execution.cells;
        self.dynamic_eval_bindings = execution.dynamic_eval_bindings;
        self.eval_dynamic_slots = execution.eval_dynamic_slots;
        self.dynamic_eval_outer_bindings = execution.dynamic_eval_outer_bindings;
        self.this = execution.this;
        self.arguments = execution.arguments;
        self.callee = execution.callee;
        self.strict = execution.strict;
        self.top_level_module = execution.top_level_module;
        self.script_global_slots = execution.script_global_slots;
        self.variable_scope = execution.variable_scope;
        self.variable_scope_lexicals = execution.variable_scope_lexicals;
        self.templates = execution.templates;
        self.new_target = execution.new_target;
        self.new_target_allowed = execution.new_target_allowed;
        self.home_object = execution.home_object;
        self.class_constructor = execution.class_constructor;
        self.class_field_initializer_depth = execution.class_field_initializer_depth;
        self.active_module_name = execution.active_module_name;
    }

    /// Heap edges held only by a displaced interpreter frame.  Keeping this
    /// independent from the module machinery lets ordinary async functions
    /// share the same GC contract and prevents continuation state from being
    /// accidentally treated as Rust-only data.
    pub(super) fn suspended_execution_references(
        execution: &SuspendedModuleExecution,
    ) -> Vec<ObjectId> {
        let mut references = Vec::new();
        let mut add_value = |value: &Value| {
            if let Some(id) = value.object_id() {
                references.push(id);
            }
        };
        for value in execution
            .stack
            .iter()
            .chain(execution.bindings.iter().flatten())
            .chain(std::iter::once(&execution.completion))
            .chain(execution.with_objects.iter())
            .chain(std::iter::once(&execution.this))
            .chain(execution.arguments.iter())
            .chain(std::iter::once(&execution.callee))
            .chain(std::iter::once(&execution.new_target))
            .chain(execution.completion_saves.iter().map(|(value, _)| value))
        {
            add_value(value);
        }
        for completion in &execution.pending_completions {
            match completion {
                Completion::Return(value)
                | Completion::Yield(value)
                | Completion::Throw(RuntimeError::Thrown(value)) => add_value(value),
                Completion::TailRecur(values) => {
                    for value in values {
                        add_value(value);
                    }
                }
                Completion::Throw(_)
                | Completion::Jump { .. }
                | Completion::Resume(_)
                | Completion::Halt(_) => {}
            }
        }
        references.extend(execution.cells.values().copied());
        for binding in execution.dynamic_eval_bindings.values().chain(
            execution
                .dynamic_eval_outer_bindings
                .iter()
                .flat_map(|bindings| bindings.values()),
        ) {
            references.push(binding.cell);
            references.extend(binding.shadowed_cells.iter().copied());
        }
        references.extend(execution.templates.values().copied());
        references.extend(
            [execution.home_object, execution.class_constructor]
                .into_iter()
                .flatten(),
        );
        references
    }

    pub(super) fn continuation_references(&self) -> Vec<ObjectId> {
        let mut references = Vec::new();
        for continuation in self.module_continuations.values() {
            references.extend(Self::suspended_execution_references(
                &continuation.execution,
            ));
            references.extend(continuation.iterators.iter().filter_map(Value::object_id));
        }
        for continuation in self.async_continuations.values() {
            references.extend(continuation.generator);
            references.push(continuation.target);
            references.extend(Self::suspended_execution_references(
                &continuation.execution,
            ));
            references.extend(continuation.iterators.iter().filter_map(Value::object_id));
        }
        references
    }

    pub(super) fn root_suspended_module_execution(
        &mut self,
        execution: &SuspendedModuleExecution,
    ) -> Result<Vec<RootId>, RuntimeError> {
        let mut roots = Vec::new();
        let registration: Result<(), HeapError> = (|| {
            for id in Self::suspended_execution_references(execution) {
                roots.push(self.heap.root(id)?);
            }
            Ok(())
        })();
        if let Err(error) = registration {
            for root in roots {
                self.heap.unroot(root)?;
            }
            return Err(error.into());
        }
        Ok(roots)
    }

    pub(super) fn run_next_job_while_module_suspended(&mut self) -> Result<bool, RuntimeError> {
        let execution = self.suspend_module_execution();
        let roots = self.root_suspended_module_execution(&execution)?;
        // Promise jobs run in a fresh ECMAScript execution context.  In
        // particular, a dynamic-import job can enter a module whose
        // top-level `await` must not inherit the suspended caller's function
        // depth; otherwise it is mistaken for an ordinary async-function
        // await and cannot install a module continuation.
        let call_depth = std::mem::replace(&mut self.call_depth, 0);
        // Jobs execute in their own execution contexts. The suspended
        // module's remaining interpreter fuel must not leave every queued
        // reaction with a zero budget after state displacement.
        self.remaining_instructions = self.config.instruction_budget;
        let result = self.run_next_promise_job();
        // The nested graph's normal completion is not the outer module's
        // completion. Its namespace has its own dedicated cache root.
        if let Some(root) = self.result_root.take() {
            self.heap.unroot(root)?;
        }
        self.restore_module_execution(execution);
        self.call_depth = call_depth;
        for root in roots {
            self.heap.unroot(root)?;
        }
        result
    }

    pub(super) fn resume_module_await(
        &mut self,
        continuation: u64,
        value: Value,
        fulfilled: bool,
    ) -> Result<(), RuntimeError> {
        let continuation =
            self.module_continuations
                .remove(&continuation)
                .ok_or(RuntimeError::Unsupported(
                    "unknown module await continuation",
                ))?;
        let ModuleContinuation {
            module,
            code,
            pc,
            execution,
            mut iterators,
            mut handlers,
        } = continuation;
        self.restore_module_execution(execution);
        let outcome = if fulfilled {
            self.interpret(
                &code,
                &mut iterators,
                pc,
                Some(value),
                None,
                Some((handlers, 0)),
            )
        } else {
            match self.resolve_completion(
                &code,
                &mut handlers,
                &mut iterators,
                Completion::Throw(RuntimeError::Thrown(value)),
            )? {
                CompletionAction::Continue => {
                    self.interpret(&code, &mut iterators, pc, None, None, Some((handlers, 0)))
                }
                CompletionAction::Jump(target) => self.interpret(
                    &code,
                    &mut iterators,
                    target,
                    None,
                    None,
                    Some((handlers, 0)),
                ),
                CompletionAction::Return(value) => Ok(InterpreterExit::Return(value)),
                CompletionAction::TailRecur(_) => Err(RuntimeError::TypeError(
                    "top-level await cannot recur".into(),
                )),
                CompletionAction::Throw(error) => Err(error),
            }
        };
        let mut graph = self
            .module_graph
            .take()
            .ok_or(RuntimeError::Unsupported("module graph continuation"))?;
        let result = match outcome {
            Ok(InterpreterExit::Return(value)) => {
                {
                    let record = graph
                        .linked
                        .get_mut(&module)
                        .expect("suspended module remains linked");
                    record.cells = std::mem::take(&mut self.cells);
                    record.evaluating = false;
                    record.suspended = false;
                    record.evaluated = true;
                    record.completion = Some(value.clone());
                }
                let modules = self.module_registry.clone();
                self.settle_dynamic_import_waiters(&module)?;
                self.settle_module_parents(module.clone(), &modules, &mut graph.linked)?;
                Ok(value)
            }
            Ok(InterpreterExit::Await {
                promise,
                pc,
                handlers,
            }) => {
                let execution = self.suspend_module_execution();
                let record = graph
                    .linked
                    .get_mut(&module)
                    .expect("suspended module remains linked");
                record.cells = execution.cells.clone();
                record.suspended = true;
                self.suspend_module_await(
                    ModuleContinuation {
                        module: module.clone(),
                        code,
                        pc,
                        execution,
                        iterators,
                        handlers,
                    },
                    promise,
                )?;
                Ok(Value::Undefined)
            }
            Ok(InterpreterExit::Yield { .. }) => Err(RuntimeError::TypeError(
                "yield requires a generator function".into(),
            )),
            Ok(InterpreterExit::Suspend { .. }) => {
                unreachable!("module execution has no generator suspend boundary")
            }
            Err(error) => {
                let error = self.error_value(error)?;
                let record = graph
                    .linked
                    .get_mut(&module)
                    .expect("suspended module remains linked");
                record.cells = std::mem::take(&mut self.cells);
                record.evaluating = false;
                record.suspended = false;
                record.evaluated = true;
                record.completion = None;
                record.error = Some(error.clone());
                self.reject_module_and_parents(module, error, &mut graph.linked)?;
                Ok(Value::Undefined)
            }
        };
        self.module_graph = Some(graph);
        result.map(|_| ())
    }

    /// Completing an async dependency releases the modules that were waiting
    /// on it during InnerModuleEvaluation. Process one breadth-first layer at
    /// a time: siblings retain source/DFS order, while an indirect parent is
    /// considered only after its direct siblings have had their turn.
    pub(super) fn settle_module_parents(
        &mut self,
        completed: String,
        modules: &HashMap<String, Bytecode>,
        linked: &mut HashMap<String, LinkedModule>,
    ) -> Result<(), RuntimeError> {
        let mut completed = std::collections::VecDeque::from([completed]);
        while let Some(child) = completed.pop_front() {
            let parents = self.module_async_parents.remove(&child).unwrap_or_default();
            for parent in parents {
                let ready = self
                    .module_pending_dependencies
                    .get_mut(&parent)
                    .is_some_and(|dependencies| {
                        dependencies.remove(&child);
                        dependencies.is_empty()
                    });
                if !ready {
                    continue;
                }
                self.module_pending_dependencies.remove(&parent);
                let record = linked
                    .get_mut(&parent)
                    .expect("async parent remains linked");
                record.evaluating = false;
                record.suspended = false;
                self.evaluate_module_record(&parent, modules, linked)?;
                if linked.get(&parent).is_some_and(|record| record.evaluated) {
                    self.settle_dynamic_import_waiters(&parent)?;
                    completed.push_back(parent);
                }
            }
        }
        Ok(())
    }

    /// Propagate an async module evaluation error through its static parents.
    /// Dynamic imports observe rejection from each affected module, while the
    /// surrounding promise checkpoint continues processing independent jobs.
    pub(super) fn reject_module_and_parents(
        &mut self,
        module: String,
        error: Value,
        linked: &mut HashMap<String, LinkedModule>,
    ) -> Result<(), RuntimeError> {
        let mut rejected = std::collections::VecDeque::from([module]);
        while let Some(module) = rejected.pop_front() {
            self.reject_dynamic_import_waiters(&module, error.clone())?;
            for parent in self
                .module_async_parents
                .remove(&module)
                .unwrap_or_default()
            {
                self.module_pending_dependencies.remove(&parent);
                let record = linked
                    .get_mut(&parent)
                    .expect("async parent remains linked");
                if record.error.is_some() {
                    continue;
                }
                record.evaluating = false;
                record.suspended = false;
                record.evaluated = true;
                record.completion = None;
                record.error = Some(error.clone());
                rejected.push_back(parent);
            }
        }
        Ok(())
    }

    pub(super) fn settle_dynamic_import_waiters(
        &mut self,
        module: &str,
    ) -> Result<(), RuntimeError> {
        // An `import.defer()` resolves once the last asynchronous dependency
        // it was waiting for has finished.
        let mut finished = Vec::new();
        for waiter in &mut self.deferred_import_waiters {
            waiter.pending.remove(module);
        }
        self.deferred_import_waiters.retain(|waiter| {
            if waiter.pending.is_empty() {
                finished.push((waiter.promise, waiter.namespace));
                false
            } else {
                true
            }
        });
        for (promise, namespace) in finished {
            self.settle_promise(promise, PromiseStatus::Fulfilled(Value::Object(namespace)))?;
        }
        let Some(waiters) = self.module_import_waiters.remove(module) else {
            return Ok(());
        };
        let namespace = self.module_namespace_cache.get(module).copied().ok_or(
            RuntimeError::ModuleResolution(format!(
                "dynamic import of {module} did not produce a namespace"
            )),
        )?;
        for promise in waiters {
            self.settle_promise(promise, PromiseStatus::Fulfilled(Value::Object(namespace)))?;
        }
        Ok(())
    }

    pub(super) fn reject_dynamic_import_waiters(
        &mut self,
        module: &str,
        error: Value,
    ) -> Result<(), RuntimeError> {
        let mut failed = Vec::new();
        self.deferred_import_waiters.retain(|waiter| {
            if waiter.pending.contains(module) {
                failed.push(waiter.promise);
                false
            } else {
                true
            }
        });
        for promise in failed {
            self.settle_promise(promise, PromiseStatus::Rejected(error.clone()))?;
        }
        let Some(waiters) = self.module_import_waiters.remove(module) else {
            return Ok(());
        };
        for promise in waiters {
            self.settle_promise(promise, PromiseStatus::Rejected(error.clone()))?;
        }
        Ok(())
    }

    pub(super) fn suspend_module_await(
        &mut self,
        continuation_state: ModuleContinuation,
        promise: ObjectId,
    ) -> Result<(), RuntimeError> {
        let continuation = self.next_module_continuation;
        self.next_module_continuation = self
            .next_module_continuation
            .checked_add(1)
            .ok_or(RuntimeError::InstructionLimit)?;
        self.module_continuations
            .insert(continuation, continuation_state);
        let status = self
            .promises
            .get(&promise)
            .ok_or(RuntimeError::TypeError("invalid await Promise".into()))?
            .status
            .clone_for_await();
        match status {
            PromiseAwaitStatus::Pending => self
                .promises
                .get_mut(&promise)
                .expect("checked await Promise exists")
                .reactions
                .push(PromiseReaction::ModuleAwait { continuation }),
            PromiseAwaitStatus::Fulfilled(value) => {
                self.promise_jobs.push_back(PromiseJob::ModuleAwait {
                    continuation,
                    value,
                    fulfilled: true,
                });
            }
            PromiseAwaitStatus::Rejected(value) => {
                self.promise_jobs.push_back(PromiseJob::ModuleAwait {
                    continuation,
                    value,
                    fulfilled: false,
                });
            }
        }
        Ok(())
    }

    /// Registers an ordinary async-function frame on the Promise it awaits.
    /// A fulfilled input still goes through the job queue, preserving the
    /// required asynchronous boundary before the frame resumes.
    pub(super) fn suspend_async_await(
        &mut self,
        continuation_state: AsyncContinuation,
        promise: ObjectId,
    ) -> Result<(), RuntimeError> {
        let continuation = self.next_async_continuation;
        self.next_async_continuation = self
            .next_async_continuation
            .checked_add(1)
            .ok_or(RuntimeError::InstructionLimit)?;
        self.async_continuations
            .insert(continuation, continuation_state);
        let status = self
            .promises
            .get(&promise)
            .ok_or(RuntimeError::TypeError("invalid await Promise".into()))?
            .status
            .clone_for_await();
        match status {
            PromiseAwaitStatus::Pending => self
                .promises
                .get_mut(&promise)
                .expect("checked await Promise exists")
                .reactions
                .push(PromiseReaction::AsyncAwait { continuation }),
            PromiseAwaitStatus::Fulfilled(value) => {
                self.promise_jobs.push_back(PromiseJob::AsyncAwait {
                    continuation,
                    value,
                    fulfilled: true,
                });
            }
            PromiseAwaitStatus::Rejected(value) => {
                self.promise_jobs.push_back(PromiseJob::AsyncAwait {
                    continuation,
                    value,
                    fulfilled: false,
                });
            }
        }
        Ok(())
    }

    /// Continues a suspended ordinary async function in its own Promise job
    /// execution context.  The ambient VM state may itself be a suspended
    /// module or a reaction handler, so it is displaced before the async
    /// frame is restored and put back unchanged after this turn.
    pub(super) fn resume_async_await(
        &mut self,
        continuation: u64,
        value: Value,
        fulfilled: bool,
    ) -> Result<(), RuntimeError> {
        let continuation =
            self.async_continuations
                .remove(&continuation)
                .ok_or(RuntimeError::Unsupported(
                    "unknown async await continuation",
                ))?;
        let AsyncContinuation {
            generator,
            target,
            code,
            pc,
            execution,
            mut iterators,
            mut handlers,
            call_depth,
        } = continuation;
        if let Some(generator) = generator {
            return self.resume_async_generator_await(
                generator, target, code, pc, execution, iterators, handlers, call_depth, value,
                fulfilled,
            );
        }
        let mut ambient = self.suspend_module_execution();
        let ambient_call_depth = std::mem::replace(&mut self.call_depth, call_depth);
        self.restore_module_execution(execution);
        let outcome = if fulfilled {
            self.interpret(
                &code,
                &mut iterators,
                pc,
                Some(value),
                None,
                Some((handlers, 0)),
            )
        } else {
            match self.resolve_completion(
                &code,
                &mut handlers,
                &mut iterators,
                Completion::Throw(RuntimeError::Thrown(value)),
            ) {
                Ok(CompletionAction::Continue) => {
                    self.interpret(&code, &mut iterators, pc, None, None, Some((handlers, 0)))
                }
                Ok(CompletionAction::Jump(target)) => self.interpret(
                    &code,
                    &mut iterators,
                    target,
                    None,
                    None,
                    Some((handlers, 0)),
                ),
                Ok(CompletionAction::Return(value)) => Ok(InterpreterExit::Return(value)),
                Ok(CompletionAction::TailRecur(_)) => Err(RuntimeError::TypeError(
                    "async function cannot tail recur across await".into(),
                )),
                Ok(CompletionAction::Throw(error)) | Err(error) => Err(error),
            }
        };

        let result = match outcome {
            Ok(InterpreterExit::Return(value)) => {
                ambient.templates.extend(self.templates.clone());
                self.restore_module_execution(ambient);
                self.call_depth = ambient_call_depth;
                self.resolve_promise(target, value)
            }
            Ok(InterpreterExit::Await {
                promise,
                pc,
                handlers,
            }) => {
                let execution = self.suspend_module_execution();
                ambient.templates.extend(execution.templates.clone());
                let state = AsyncContinuation {
                    generator,
                    target,
                    code,
                    pc,
                    execution,
                    iterators,
                    handlers,
                    call_depth: self.call_depth,
                };
                self.restore_module_execution(ambient);
                self.call_depth = ambient_call_depth;
                self.suspend_async_await(state, promise)
            }
            Ok(InterpreterExit::Yield { .. }) => {
                ambient.templates.extend(self.templates.clone());
                self.restore_module_execution(ambient);
                self.call_depth = ambient_call_depth;
                Err(RuntimeError::TypeError(
                    "yield requires an async generator function".into(),
                ))
            }
            Ok(InterpreterExit::Suspend { .. }) => {
                unreachable!("async functions have no entry suspend")
            }
            Err(error) => {
                // Iterator records are part of the suspended frame rather
                // than ordinary heap properties. Root them while abrupt
                // cleanup invokes user-provided `return` methods.
                let base = self.stack.len();
                if let RuntimeError::Thrown(value) = &error {
                    self.stack.push(value.clone());
                }
                self.stack.extend(iterators.iter().cloned());
                for record in iterators.into_iter().rev() {
                    let _ = self.iterator_close(&record);
                }
                self.stack.truncate(base);
                ambient.templates.extend(self.templates.clone());
                self.restore_module_execution(ambient);
                self.call_depth = ambient_call_depth;
                let error = self.error_value(error)?;
                self.settle_promise(target, PromiseStatus::Rejected(error))
            }
        };
        result
    }

    /// Resume one pending async-generator request. Its frame is identical to
    /// an ordinary async continuation, but a `yield` settles the request and
    /// keeps the generator resumable instead of resolving a function call.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn resume_async_generator_await(
        &mut self,
        generator: ObjectId,
        target: ObjectId,
        code: Rc<Bytecode>,
        pc: usize,
        execution: SuspendedModuleExecution,
        mut iterators: Vec<Value>,
        mut handlers: Vec<HandlerFrame>,
        call_depth: usize,
        value: Value,
        fulfilled: bool,
    ) -> Result<(), RuntimeError> {
        let mut ambient = self.suspend_module_execution();
        let ambient_call_depth = std::mem::replace(&mut self.call_depth, call_depth);
        self.restore_module_execution(execution);
        let outcome = if fulfilled {
            self.interpret(
                &code,
                &mut iterators,
                pc,
                Some(value),
                None,
                Some((handlers, 0)),
            )
        } else {
            match self.resolve_completion(
                &code,
                &mut handlers,
                &mut iterators,
                Completion::Throw(RuntimeError::Thrown(value)),
            ) {
                Ok(CompletionAction::Continue) => {
                    self.interpret(&code, &mut iterators, pc, None, None, Some((handlers, 0)))
                }
                Ok(CompletionAction::Jump(target)) => self.interpret(
                    &code,
                    &mut iterators,
                    target,
                    None,
                    None,
                    Some((handlers, 0)),
                ),
                Ok(CompletionAction::Return(value)) => Ok(InterpreterExit::Return(value)),
                Ok(CompletionAction::TailRecur(_)) => Err(RuntimeError::TypeError(
                    "async generator cannot tail recur across await".into(),
                )),
                Ok(CompletionAction::Throw(error)) | Err(error) => Err(error),
            }
        };

        match outcome {
            Ok(InterpreterExit::Return(value)) => {
                self.heap
                    .set_generator_state(generator, GeneratorState::Done)?;
                ambient.templates.extend(self.templates.clone());
                self.restore_module_execution(ambient);
                self.call_depth = ambient_call_depth;
                let result = self.iterator_result(value, true)?;
                self.await_async_generator_yield(generator, target, result)
            }
            Ok(InterpreterExit::Yield {
                value,
                pc,
                iterators,
                handlers,
            }) => {
                let stack = std::mem::take(&mut self.stack);
                let async_delegate =
                    code.async_yield_delegates
                        .iter()
                        .find(|(resume, _)| *resume as usize == pc)
                        .and_then(|(_, exit_pc)| {
                            stack.last().cloned().map(|record| {
                                crate::heap::AsyncGeneratorDelegate {
                                    record,
                                    exit_pc: *exit_pc as usize,
                                }
                            })
                        });
                let state = GeneratorState::Suspended {
                    code,
                    pc,
                    stack,
                    bindings: std::mem::take(&mut self.bindings),
                    cells: std::mem::take(&mut self.cells).into_iter().collect(),
                    this: std::mem::replace(&mut self.this, Value::Undefined),
                    args: std::mem::take(&mut self.arguments),
                    completion: std::mem::replace(&mut self.completion, Value::Undefined),
                    completion_empty: std::mem::replace(&mut self.completion_empty, true),
                    active_scopes: std::mem::take(&mut self.active_scopes),
                    iterators,
                    handlers,
                    pending_completions: std::mem::take(&mut self.pending_completions)
                        .into_iter()
                        .map(|completion| {
                            completion
                                .into_generator_pending()
                                .expect("only catchable completions can survive a generator yield")
                        })
                        .collect(),
                    completion_saves: std::mem::take(&mut self.completion_saves),
                    async_delegate,
                    delegate: None,
                    dynamic_bindings: std::mem::take(&mut self.dynamic_eval_bindings)
                        .into_iter()
                        .map(|(name, binding)| (name, binding.cell, binding.shadowed_cells))
                        .collect(),
                    home: std::mem::take(&mut self.home_object),
                    callee: std::mem::replace(&mut self.callee, Value::Undefined),
                };
                self.heap.set_generator_state(generator, state)?;
                ambient.templates.extend(self.templates.clone());
                self.restore_module_execution(ambient);
                self.call_depth = ambient_call_depth;
                let result = self.iterator_result(value, false)?;
                self.await_async_generator_yield(generator, target, result)
            }
            Ok(InterpreterExit::Await {
                promise,
                pc,
                handlers,
            }) => {
                let execution = self.suspend_module_execution();
                ambient.templates.extend(execution.templates.clone());
                let state = AsyncContinuation {
                    generator: Some(generator),
                    target,
                    code,
                    pc,
                    execution,
                    iterators,
                    handlers,
                    call_depth: self.call_depth,
                };
                self.restore_module_execution(ambient);
                self.call_depth = ambient_call_depth;
                self.suspend_async_await(state, promise)
            }
            Ok(InterpreterExit::Suspend { .. }) => {
                unreachable!("async-generator resumption has no entry suspend")
            }
            Err(error) => {
                let base = self.stack.len();
                if let RuntimeError::Thrown(value) = &error {
                    self.stack.push(value.clone());
                }
                self.stack.extend(iterators.iter().cloned());
                for record in iterators.into_iter().rev() {
                    let _ = self.iterator_close(&record);
                }
                self.stack.truncate(base);
                self.heap
                    .set_generator_state(generator, GeneratorState::Done)?;
                ambient.templates.extend(self.templates.clone());
                self.restore_module_execution(ambient);
                self.call_depth = ambient_call_depth;
                let value = self.error_value(error)?;
                self.complete_async_generator_request(
                    generator,
                    target,
                    PromiseStatus::Rejected(value),
                )?;
                self.resume_async_generator_next(generator)
            }
        }
    }

    pub(super) fn enter_module_record(&mut self, code: &Bytecode, cells: HashMap<usize, ObjectId>) {
        self.stack.clear();
        self.bindings = vec![None; code.bindings.len()];
        self.binding_metadata = code.bindings.clone();
        self.cells = cells;
        self.completion = Value::Undefined;
        self.completion_empty = true;
        self.active_scopes.clear();
        self.active_scope_slots.clear();
        self.with_objects.clear();
        self.script_global_slots.clear();
        self.strict = true;
        self.this = Value::Undefined;
        self.top_level_module = true;
    }

    pub(super) fn initialize_module_record(
        &mut self,
        name: &str,
        code: &Bytecode,
        cells: &mut HashMap<usize, ObjectId>,
    ) -> Result<(), RuntimeError> {
        let entry = code.module_evaluate_entry.ok_or(RuntimeError::Unsupported(
            "module declaration instantiation",
        ))? as usize;
        self.enter_module_record(code, std::mem::take(cells));
        let mut iterators = Vec::new();
        // The declaration prefix instantiates the module's hoisted functions;
        // each records the module it is created in (its [[ScriptOrModule]]),
        // which `import.meta` and `import()` inside it resolve against no
        // matter which module later calls it.
        let previous_module = self.active_module_name.replace(name.to_string());
        let result = self.interpret(code, &mut iterators, 0, None, Some(entry), None);
        self.active_module_name = previous_module;
        *cells = std::mem::take(&mut self.cells);
        self.stack.clear();
        self.active_scopes.clear();
        self.active_scope_slots.clear();
        match result? {
            InterpreterExit::Suspend { pc } if pc == entry => Ok(()),
            _ => unreachable!("module declaration prefix always suspends at its evaluation entry"),
        }
    }

    /// Returns whether a requested-module edge can reach `goal`.  This small
    /// graph walk is used only while wiring async parents, after linking has
    /// already validated every request.
    pub(super) fn module_reaches(
        from: &str,
        goal: &str,
        modules: &HashMap<String, Bytecode>,
        visited: &mut HashSet<String>,
    ) -> Result<bool, RuntimeError> {
        if from == goal {
            return Ok(true);
        }
        if !visited.insert(from.to_string()) {
            return Ok(false);
        }
        let code = modules.get(from).ok_or_else(|| {
            RuntimeError::ModuleResolution(format!("module {from} was not linked"))
        })?;
        for request in &code.module_requests {
            // A deferred request is not an edge of the evaluation DFS (its
            // module only runs when a namespace of it is observed), so it can
            // never put two modules into one dependency cycle.
            if request.deferred {
                continue;
            }
            let target =
                Self::resolve_module_target(from, &request.module_request, request.module_type)?;
            if Self::module_reaches(&target, goal, modules, visited)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// InnerModuleEvaluation attaches an importer to a suspended dependency's
    /// cycle root, not necessarily the particular module that appeared in
    /// the import declaration.  Without this distinction an outside importer
    /// can resume between a cycle leaf and its root's own top-level await.
    pub(super) fn async_dependency_root(
        &self,
        dependency: &str,
        modules: &HashMap<String, Bytecode>,
        linked: &HashMap<String, LinkedModule>,
    ) -> Result<String, RuntimeError> {
        for candidate in self
            .module_async_parents
            .get(dependency)
            .into_iter()
            .flatten()
        {
            if linked
                .get(candidate)
                .is_some_and(|record| record.evaluating || record.suspended)
                && Self::module_reaches(dependency, candidate, modules, &mut HashSet::new())?
            {
                return Ok(candidate.clone());
            }
        }
        Ok(dependency.to_string())
    }

    pub(super) fn evaluate_module_record(
        &mut self,
        name: &str,
        modules: &HashMap<String, Bytecode>,
        linked: &mut HashMap<String, LinkedModule>,
    ) -> Result<Value, RuntimeError> {
        let Some(record) = linked.get(name) else {
            return Err(RuntimeError::ModuleResolution(format!(
                "module {name} was not linked"
            )));
        };
        if let Some(error) = &record.error {
            return Err(RuntimeError::Thrown(error.clone()));
        }
        if record.evaluated || record.evaluating {
            return Ok(Value::Undefined);
        }
        linked
            .get_mut(name)
            .expect("checked module record exists")
            .evaluating = true;
        let result = (|| {
            let code = modules.get(name).expect("linked module has bytecode");
            let mut pending_dependencies = Vec::new();
            for request in &code.module_requests {
                let target = Self::resolve_module_target(
                    name,
                    &request.module_request,
                    request.module_type,
                )?;
                // A deferred request contributes only the asynchronous part of
                // its graph; an eager one contributes the module itself.
                let evaluation_list = if request.deferred {
                    Self::gather_async_dependencies(&target, modules, linked, &mut HashSet::new())?
                } else {
                    vec![target]
                };
                for target in evaluation_list {
                    self.evaluate_module_record(&target, modules, linked)?;
                    if linked.get(&target).is_some_and(|record| record.suspended) {
                        pending_dependencies
                            .push(self.async_dependency_root(&target, modules, linked)?);
                    }
                }
            }
            if !pending_dependencies.is_empty() {
                let dependencies = self
                    .module_pending_dependencies
                    .entry(name.to_string())
                    .or_default();
                for dependency in pending_dependencies {
                    if dependencies.insert(dependency.clone()) {
                        self.module_async_parents
                            .entry(dependency)
                            .or_default()
                            .push(name.to_string());
                    }
                }
                linked
                    .get_mut(name)
                    .expect("checked module record exists")
                    .suspended = true;
                return Ok(Value::Undefined);
            }
            let entry = code.module_evaluate_entry.ok_or(RuntimeError::Unsupported(
                "module declaration instantiation",
            ))? as usize;
            let cells = std::mem::take(
                &mut linked
                    .get_mut(name)
                    .expect("checked module record exists")
                    .cells,
            );
            self.enter_module_record(code, cells);
            // A sibling Source Text Module runs in a fresh execution context.
            // A suspended async dependency deliberately leaves its displaced
            // frame with zero ambient fuel, which must not exhaust this
            // independent module before its first instruction.
            self.remaining_instructions = self.config.instruction_budget;
            self.active_scopes.push(0);
            self.active_scope_slots
                .push(code.scopes.first().cloned().unwrap_or_default());
            let mut iterators = Vec::new();
            let previous_module = self.active_module_name.replace(name.to_string());
            // Park the graph's records where user code can reach them: a
            // deferred namespace observed by this module (or a module it
            // triggers) evaluates other modules of the same graph.
            self.evaluating_linked = Some(std::mem::take(linked));
            let outcome = self.interpret(code, &mut iterators, entry, None, None, None);
            *linked = self
                .evaluating_linked
                .take()
                .expect("module records are restored after the module body ran");
            match outcome {
                Ok(InterpreterExit::Return(value)) => {
                    self.active_module_name = previous_module;
                    let cells = std::mem::take(&mut self.cells);
                    let record = linked.get_mut(name).expect("checked module record exists");
                    record.cells = cells;
                    record.completion = Some(value.clone());
                    Ok(value)
                }
                Ok(InterpreterExit::Await {
                    promise,
                    pc,
                    handlers,
                }) => {
                    // Move the realm frame out before another sibling module
                    // evaluates. The continuation is attached to the Await
                    // promise and resumes in its own Promise job turn.
                    let execution = self.suspend_module_execution();
                    self.active_module_name = previous_module;
                    let record = linked.get_mut(name).expect("checked module record exists");
                    record.cells = execution.cells.clone();
                    record.suspended = true;
                    self.suspend_module_await(
                        ModuleContinuation {
                            module: name.to_string(),
                            code: code.clone(),
                            pc,
                            execution,
                            iterators,
                            handlers,
                        },
                        promise,
                    )?;
                    Ok(Value::Undefined)
                }
                Ok(InterpreterExit::Yield { .. }) => Err(RuntimeError::TypeError(
                    "yield requires a generator function".into(),
                )),
                Ok(InterpreterExit::Suspend { .. }) => {
                    unreachable!("module evaluation does not suspend")
                }
                Err(error) => {
                    self.active_module_name = previous_module;
                    let cells = std::mem::take(&mut self.cells);
                    let record = linked.get_mut(name).expect("checked module record exists");
                    record.cells = cells;
                    Err(error)
                }
            }
        })();
        // Evaluate() records an abrupt completion as the module's
        // [[EvaluationError]]: every module on the DFS stack finishes
        // `evaluated` with that same error, which later importers, deferred
        // namespace triggers and dynamic imports all observe unchanged.
        // Errors raised by the engine itself become one JavaScript value here
        // so that identity survives the propagation to the importers.
        let result = match result {
            Err(
                error @ (RuntimeError::Thrown(_)
                | RuntimeError::TypeError(_)
                | RuntimeError::ReferenceError(_)
                | RuntimeError::RangeError(_)
                | RuntimeError::SyntaxError(_)),
            ) if !linked.get(name).is_some_and(|record| record.suspended) => {
                let value = self.error_value(error)?;
                let record = linked.get_mut(name).expect("checked module record exists");
                record.evaluating = false;
                record.evaluated = true;
                record.completion = None;
                record.error = Some(value.clone());
                Err(RuntimeError::Thrown(value))
            }
            result => result,
        };
        let record = linked.get_mut(name).expect("checked module record exists");
        if !record.suspended {
            record.evaluating = false;
        }
        if result.is_ok() && !record.suspended {
            record.evaluated = true;
        }
        result
    }
}
