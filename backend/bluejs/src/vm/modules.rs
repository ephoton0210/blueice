// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

mod deferred;
mod namespace;
mod synthetic;

use super::*;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/module_dependency_ownership.rs"]
mod dependency_ownership_tests;

/// How code that is not a generator -- a module body, an async function body
/// -- finishes a run: it returns, or it awaits. It never yields, and no
/// running code suspends at a generator's entry.
enum StraightLineExit {
    Return(Value),
    Await {
        promise: ObjectId,
        pc: usize,
        handlers: HandlerRecords,
    },
}

/// An ESM body may park at a debugger boundary or finish an interpreter turn.
/// Once the parked case is consumed, terminal cleanup only sees return,
/// await, or an error; it has no second suspension case to handle.
enum ModuleRun {
    Paused {
        pc: usize,
        iterators: IteratorRecords,
        handlers: HandlerRecords,
    },
    Completed(Result<StraightLineExit, RuntimeError>),
}

impl ModuleRun {
    fn classify(outcome: Result<InterpreterExit, RuntimeError>) -> Self {
        match outcome {
            Ok(InterpreterExit::Suspend {
                pc,
                iterators,
                handlers,
            }) => Self::Paused {
                pc,
                iterators,
                handlers,
            },
            outcome => Self::Completed(straight_line_exit(
                outcome,
                "yield requires a generator function",
            )),
        }
    }
}

/// Narrows an interpreter exit to [`StraightLineExit`]; the two exits such
/// code cannot produce are errors instead.
fn straight_line_exit(
    outcome: Result<InterpreterExit, RuntimeError>,
    yield_message: &str,
) -> Result<StraightLineExit, RuntimeError> {
    match outcome? {
        InterpreterExit::Return(value) => Ok(StraightLineExit::Return(value)),
        InterpreterExit::Await {
            promise,
            pc,
            handlers,
        } => Ok(StraightLineExit::Await {
            promise,
            pc,
            handlers,
        }),
        InterpreterExit::Yield { .. } => Err(RuntimeError::TypeError(yield_message.into())),
        InterpreterExit::Suspend { .. } => Err(RuntimeError::TypeError(
            "only a generator suspends at its entry".into(),
        )),
    }
}

/// How the resumption of an async generator finishes a run: it returns,
/// yields or awaits, but as it is not the generator's first entry it never
/// suspends at one.
enum AsyncGeneratorExit {
    Return(Value),
    Yield {
        value: Value,
        pc: usize,
        iterators: IteratorRecords,
        handlers: HandlerRecords,
    },
    Await {
        promise: ObjectId,
        pc: usize,
        handlers: HandlerRecords,
    },
}

/// Narrows an interpreter exit to [`AsyncGeneratorExit`].
fn async_generator_exit(
    outcome: Result<InterpreterExit, RuntimeError>,
) -> Result<AsyncGeneratorExit, RuntimeError> {
    match outcome? {
        InterpreterExit::Return(value) => Ok(AsyncGeneratorExit::Return(value)),
        InterpreterExit::Yield {
            value,
            pc,
            iterators,
            handlers,
        } => Ok(AsyncGeneratorExit::Yield {
            value,
            pc,
            iterators,
            handlers,
        }),
        InterpreterExit::Await {
            promise,
            pc,
            handlers,
        } => Ok(AsyncGeneratorExit::Await {
            promise,
            pc,
            handlers,
        }),
        InterpreterExit::Suspend { .. } => Err(RuntimeError::TypeError(
            "only a generator suspends at its entry".into(),
        )),
    }
}

/// A module's declaration prefix always ends by suspending at the offset of
/// its evaluation entry.
fn expect_entry_suspend(exit: InterpreterExit, entry: usize) -> Result<(), RuntimeError> {
    match exit {
        InterpreterExit::Suspend { pc, .. } if pc == entry => Ok(()),
        _ => Err(RuntimeError::Unsupported(
            "module declaration prefix did not suspend at its evaluation entry",
        )),
    }
}

/// How an import declaration spells what it imports in an error message.
fn imported_name_text(name: &ModuleImportName) -> &str {
    match name {
        ModuleImportName::Named(name) => name,
        ModuleImportName::Namespace | ModuleImportName::DeferredNamespace => "*",
        ModuleImportName::Source => "source",
    }
}

/// Where a suspended frame carries on once its await was rejected and the
/// thrown value has been offered to the frame's handlers.
enum RejectedAwait {
    /// Interpret again from this offset.
    Resume(usize),
    /// The frame finishes with this return value.
    Return(Value),
}

/// Reads how resolving a throw ended. A throw ends in a handler (a jump), or
/// leaves the frame; the actions only a resumption (`pc`) or a return can
/// produce are accepted too, and a tail call cannot cross an await
/// (`tail_call_message` says so).
fn rejected_await_step(
    action: CompletionAction,
    pc: usize,
    tail_call_message: &'static str,
) -> Result<RejectedAwait, RuntimeError> {
    match action {
        CompletionAction::Continue => Ok(RejectedAwait::Resume(pc)),
        CompletionAction::Jump(target) => Ok(RejectedAwait::Resume(target)),
        CompletionAction::Return(value) => Ok(RejectedAwait::Return(value)),
        CompletionAction::TailRecur(_) | CompletionAction::TailCall(_) => {
            Err(RuntimeError::TypeError(tail_call_message.into()))
        }
        CompletionAction::Throw(error) => Err(error),
    }
}

impl Vm {
    /// Resumes a frame suspended at an await that was rejected with `value`:
    /// the value is thrown at the await, and the frame carries on in whichever
    /// handler catches it, or ends with the error.
    fn interpret_rejected_await(
        &mut self,
        code: &Bytecode,
        iterators: &mut IteratorRecords,
        pc: usize,
        mut handlers: HandlerRecords,
        value: Value,
        tail_call_message: &'static str,
    ) -> Result<InterpreterExit, RuntimeError> {
        if code
            .instruction(pc)
            .is_some_and(|instruction| instruction.opcode == Opcode::AsyncIteratorCloseFinish)
            && handlers
                .last()
                .and_then(|frame| frame.pending)
                .and_then(|pending| self.pending_completions.get(pending))
                .is_some_and(|completion| matches!(completion, Completion::Throw(_)))
        {
            // AsyncIteratorClose awaits even with a pending throw, then
            // restores that original throw ahead of the return rejection.
            return self.interpret(
                code,
                iterators,
                pc,
                Some(Value::Undefined),
                None,
                Some((handlers, 0)),
            );
        }
        self.publish_rejection_origin(self.current_rejection_origin());
        let action = self.resolve_completion(
            code,
            &mut handlers,
            iterators,
            Completion::Throw(RuntimeError::Thrown(value)),
        );
        self.interpret_after_throw(code, iterators, pc, handlers, action, tail_call_message)
    }

    /// Carries a frame on as `action`, how a value thrown at its await was
    /// resolved, says (see [`rejected_await_step`]).
    fn interpret_after_throw(
        &mut self,
        code: &Bytecode,
        iterators: &mut IteratorRecords,
        pc: usize,
        handlers: HandlerRecords,
        action: Result<CompletionAction, RuntimeError>,
        tail_call_message: &'static str,
    ) -> Result<InterpreterExit, RuntimeError> {
        match action.and_then(|action| rejected_await_step(action, pc, tail_call_message)) {
            Ok(RejectedAwait::Resume(at)) => {
                self.interpret(code, iterators, at, None, None, Some((handlers, 0)))
            }
            Ok(RejectedAwait::Return(value)) => Ok(InterpreterExit::Return(value)),
            Err(error) => Err(error),
        }
    }

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
        self.synchronize_scheduling_usage()?;
        let registry = ModuleRegistry::copy_host(modules, self.host_scheduling.as_ref())?;
        self.execute_module_registry_inner(entry, &registry, drain_jobs, is_dynamic_import, phase)
    }

    pub(super) fn execute_module_registry_inner(
        &mut self,
        entry: &str,
        modules: &ModuleRegistry,
        drain_jobs: bool,
        is_dynamic_import: bool,
        phase: ImportPhase,
    ) -> Result<Value, RuntimeError> {
        self.execute_module_graph_with_frontier(
            entry,
            modules,
            drain_jobs,
            is_dynamic_import,
            phase,
            &mut Vec::new(),
        )
    }

    pub(super) fn execute_module_graph_with_frontier(
        &mut self,
        entry: &str,
        modules: &ModuleRegistry,
        drain_jobs: bool,
        is_dynamic_import: bool,
        phase: ImportPhase,
        frontier: &mut Vec<String>,
    ) -> Result<Value, RuntimeError> {
        self.synchronize_scheduling_usage()?;
        self.module_execution_depth += 1;
        let result = self.execute_module_graph_inner_scoped(
            entry,
            modules,
            drain_jobs,
            is_dynamic_import,
            phase,
            frontier,
        );
        self.module_execution_depth -= 1;
        result
    }

    fn execute_module_graph_inner_scoped(
        &mut self,
        entry: &str,
        modules: &ModuleRegistry,
        drain_jobs: bool,
        is_dynamic_import: bool,
        phase: ImportPhase,
        frontier: &mut Vec<String>,
    ) -> Result<Value, RuntimeError> {
        if let Some(scope) = &self.host_scheduling {
            modules.attach(scope)?;
        }
        // An import that starts while module code is running (a dynamic
        // `import()` in a Promise job the entry's `await` lets run) joins the
        // graph that code belongs to instead of linking a second copy of it.
        let mut nested_in_evaluation = false;
        let (mut linked, mut roots, fresh_graph) = match self.module_graph.take() {
            Some(graph) => (graph.linked, graph.roots, false),
            None => match self.evaluating_linked.take() {
                Some(linked) => {
                    nested_in_evaluation = true;
                    (linked, ModuleRoots::default(), false)
                }
                None => (ModuleRecords::default(), ModuleRoots::default(), true),
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
        let roots_checkpoint = roots.len();
        let registry = if is_dynamic_import || fresh_graph {
            let mut owned_modules = ModuleRegistryDraft::new(modules.clone());
            let prepared = (|| {
                if is_dynamic_import {
                    // `entry` is already the resolved target: `dynamic_import_job`
                    // resolves the specifier against its referrer before this call.
                    let (_, module_type) = ModuleType::split_module_key(entry);
                    if let Some(synthetic_type) =
                        synthetic::SyntheticModuleType::from_module_type(module_type)
                    {
                        self.ensure_synthetic_module(
                            entry,
                            synthetic_type,
                            &mut owned_modules,
                            &mut roots,
                        )?;
                    } else {
                        self.ensure_dynamic_module_compiled(entry, &mut owned_modules)?;
                    }
                }
                if fresh_graph {
                    self.register_static_synthetic_modules(&mut owned_modules, &mut roots)?;
                }
                Ok(())
            })();
            if let Err(error) = prepared {
                self.release_module_roots_from(&mut roots, roots_checkpoint);
                // A module that cannot even be compiled or synthesized fails
                // only this request; an existing graph stays usable.
                if fresh_graph {
                    for root in roots {
                        self.release_root(root);
                    }
                } else {
                    self.store_module_graph(
                        ModuleGraphState { linked, roots },
                        nested_in_evaluation,
                    );
                }
                return Err(error);
            }
            owned_modules.finish()
        } else {
            modules.clone()
        };
        let modules: &HashMap<String, Bytecode> = &registry;
        // Every module in `modules` not already in `linked` is new to this
        // graph -- either every one of them, on a fresh graph, or just the
        // one(s) `ensure_synthetic_module`/`ensure_dynamic_module_compiled` (or a
        // fresh static scan) added just above to an *existing* graph. The
        // per-`new_names` linking pass below (inside the closure) treats
        // both cases identically, so a synthetic module or an ordinary module
        // compiled on demand for a dynamic import gets exactly the same
        // real linking (cell creation, export validation, import aliasing,
        // declaration instantiation) a fresh graph's own modules get.
        let new_names = match self.prepare_module_names(modules, &linked) {
            Ok(names) => names,
            Err(error) => {
                self.release_module_roots_from(&mut roots, roots_checkpoint);
                if fresh_graph {
                    for root in roots {
                        self.release_root(root);
                    }
                } else {
                    self.store_module_graph(
                        ModuleGraphState { linked, roots },
                        nested_in_evaluation,
                    );
                }
                return Err(error);
            }
        };
        // Roots added since preparation began, until this call's success/failure
        // is known, belong only to `new_names`'s delta -- on a linking
        // failure for that delta (not a fresh graph, which instead discards
        // every root wholesale below), only these need unrooting, and only
        // `new_names` need removing from `linked`, so a distinct later
        // operation on the rest of an existing graph is unaffected and a
        // retried dynamic import of the same specifier is treated as new
        // again rather than silently resuming a half-linked record.
        // Once a module body has started running, its evaluation error (or
        // completion) belongs to the module record and is observed again by
        // every later importer, so the graph is kept rather than rolled back.
        let mut evaluation_started = false;
        let mut previous_registry = None;
        let mut result = (|| {
            previous_registry = Some(std::mem::replace(
                &mut self.module_registry,
                registry.clone(),
            ));
            // Resolution errors can be converted to a dynamic-import Promise
            // rejection before graph setup reaches the usual execution reset.
            // Give that host-visible error construction a fresh budget too.
            self.remaining_instructions = self.config.instruction_budget;
            // The host supplies the closed source set for this realm;
            // `new_names` (computed above, before this closure) link every
            // newly-supplied record once so a later dynamic import shares a
            // static import's cells instead of constructing a second module
            // instance for the same canonical path.
            if let Some(root) = self.result_root.take() {
                self.release_root(root);
            }
            if let Some(root) = self.last_module_namespace_root.take() {
                self.release_root(root);
            }
            self.last_module_namespace = None;
            self.collect_module_garbage()?;
            self.enqueue_finalization_cleanup_jobs()?;
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
                linked.reserve(new_names.len(), self.host_scheduling.as_ref())?;
                for &name in new_names.iter() {
                    let code = modules
                        .get(name)
                        .expect("reachable module was checked during collection");
                    if !code.module {
                        return Err(RuntimeError::ModuleResolution(format!(
                            "{name} was not compiled using the module goal"
                        )));
                    }
                    let prepared =
                        self.prepare_linked_module_cells(code.scopes.first().map_or(0, Vec::len))?;
                    let mut record = LinkedModule {
                        cells: HashMap::new(),
                        cells_charge: self.charge_module_cells(0),
                        namespace: None,
                        evaluated: false,
                        evaluating: false,
                        suspended: false,
                        completion: None,
                        error: None,
                        host_error_pending: false,
                        deferred_namespace: None,
                    };
                    prepared.install(&mut record);
                    linked.insert_new(name, record, self.host_scheduling.as_ref())?;
                }

                // ModuleDeclarationInstantiation creates all own bindings before
                // wiring imports.  A `var` binding is initialized immediately;
                // lexical bindings deliberately have no `value` property yet.
                for &name in new_names.iter() {
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
                    let slots = code.scopes.first().map(Vec::as_slice).unwrap_or(&[]);
                    let record = linked
                        .get_mut(name)
                        .expect("linked record was allocated for every module");
                    for slot in slots {
                        let slot = *slot as usize;
                        if imported_slots.contains(&slot) {
                            continue;
                        }
                        let root_slot = roots.prepare_slot(self.host_scheduling.as_ref())?;
                        let cell = self.with_roots(|heap| heap.alloc_object(None))?;
                        root_slot.root_into(&mut roots, &mut self.heap, cell)?;
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
                for &name in new_names.iter() {
                    let code = modules
                        .get(name)
                        .expect("reachable module was checked during collection");
                    if let Some(value) = code.synthetic_default_export.as_ref() {
                        let record = linked
                            .get_mut(name)
                            .expect("linked record was allocated for every module");
                        let cell = *record
                            .cells
                            .get(&0)
                            .expect("synthetic module's default slot has a cell");
                        self.install_synthetic_export(cell, value)?;
                        record.evaluated = true;
                    }
                }

                // Validate every named indirect export before evaluating any
                // module body.  A missing or ambiguous `export { x } from ...`
                // is a ModuleDeclarationInstantiation error, including when the
                // body would otherwise call $DONOTEVALUATE().  Star exports do
                // not themselves fail here; their ambiguity matters only when a
                // particular name is resolved by an import or indirect export.
                for &name in new_names.iter() {
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
                for &name in new_names.iter() {
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
                            // The loop above already resolved this request.
                            Self::resolve_module_request(name, &import.module_request)
                                .expect("a source-phase request was resolved while it was loaded")
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
                                // Every module of the graph made a cell for
                                // each binding of its own above.
                                .expect("an exported binding has a cell"),
                            ExportResolution::Namespace { module }
                            | ExportResolution::DeferredNamespace { module } => {
                                let namespace = self.module_namespace(
                                    &module,
                                    deferred_target,
                                    modules,
                                    &mut linked,
                                    &mut roots,
                                )?;
                                let root_slot =
                                    roots.prepare_slot(self.host_scheduling.as_ref())?;
                                let cell = self.with_roots(|heap| heap.alloc_object(None))?;
                                root_slot.root_into(&mut roots, &mut self.heap, cell)?;
                                self.with_roots(|heap| {
                                    heap.set(cell, "value", Value::Object(namespace))
                                })?;
                                cell
                            }
                            ExportResolution::Source { module } => {
                                // The loop above already made (and cached) it.
                                let source = self
                                    .module_source_object(&module, modules)
                                    .expect("a source-phase record was made while it was loaded");
                                let root_slot =
                                    roots.prepare_slot(self.host_scheduling.as_ref())?;
                                let cell = self.with_roots(|heap| heap.alloc_object(None))?;
                                root_slot.root_into(&mut roots, &mut self.heap, cell)?;
                                self.with_roots(|heap| {
                                    heap.set(cell, "value", Value::Object(source))
                                })?;
                                cell
                            }
                            ExportResolution::Missing | ExportResolution::Ambiguous => {
                                return Err(RuntimeError::ModuleResolution(format!(
                                    "{} does not export {}",
                                    import.module_request,
                                    imported_name_text(&import.import_name)
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
                for &name in new_names.iter() {
                    let code = modules
                        .get(name)
                        .expect("reachable module was checked during collection");
                    let record = linked
                        .get_mut(name)
                        .expect("linked record was allocated for every module");
                    self.initialize_linked_module_record(name, code, record)?;
                }
            }

            // LoadRequestedModules has to have succeeded for every phase before
            // anything is evaluated: a missing module fails the whole request
            // even when the only edge to it is a deferred one.
            Self::check_requested_modules_loaded(entry, modules)?;
            evaluation_started = true;
            drop(previous_registry.take());
            if !is_dynamic_import && self.host_error_reporting.is_some() {
                if let Some(record) = linked.get_mut(entry) {
                    record.host_error_pending = true;
                }
            }
            let (value, namespace) = if phase == ImportPhase::Defer {
                // A deferred import evaluates nothing of `entry` itself; only
                // the asynchronous part of its dependency graph runs now.
                // Loading the requested modules resolved every request the walk
                // follows.
                let dependencies =
                    Self::gather_async_dependencies(entry, modules, &linked, &mut HashSet::new())
                        .expect("every request of the graph was resolved when it was loaded");
                // This vector remains Rust-owned across dependency JavaScript.
                // Cache hits run no code; their immediate result handoff stays
                // within the existing transient native-operation policy.
                let executes = dependencies.iter().any(|name| {
                    linked.get(name).is_some_and(|record| {
                        !record.evaluated && !record.evaluating && record.error.is_none()
                    })
                });
                let frontier_payload = self.charge_execution_payload(if executes {
                    module_dependencies::string_vector_bytes(&dependencies)
                } else {
                    0
                });
                self.synchronize_scheduling_usage()?;
                for dependency in &dependencies {
                    self.evaluate_module_record(dependency, &registry, &mut linked)?;
                }
                drop(frontier_payload);
                *frontier = dependencies;
                let namespace =
                    self.module_namespace(entry, true, modules, &mut linked, &mut roots)?;
                (Value::Undefined, namespace)
            } else {
                let value = self.evaluate_module_record(entry, &registry, &mut linked)?;
                if self.debugger_module_continuation.is_some() {
                    return Ok(Value::Undefined);
                }
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
            match self.heap.root(*id) {
                Ok(root) => self.result_root = Some(root),
                Err(error) => result = Err(error.into()),
            }
        }

        if result.is_err() && evaluation_started {
            self.store_module_graph(ModuleGraphState { linked, roots }, nested_in_evaluation);
        } else if result.is_err() {
            if let Some(previous) = previous_registry.take() {
                self.module_registry = previous;
            }
            if fresh_graph {
                // The whole graph never became usable; discard everything.
                for root in roots {
                    self.release_root(root);
                }
                drop(linked);
                self.reconcile_scheduling_ownership();
            } else {
                // Only `new_names`'s delta was attempted against an existing,
                // still-otherwise-valid graph: roll back just that delta (see
                // `roots_checkpoint`'s own comment above) and keep the rest,
                // so an unrelated later operation on the existing graph is
                // unaffected and a retried dynamic import of the same
                // specifier is treated as new again.
                for &name in new_names.iter() {
                    linked.remove(name);
                }
                self.release_module_roots_from(&mut roots, roots_checkpoint);
                self.store_module_graph(ModuleGraphState { linked, roots }, nested_in_evaluation);
            }
        } else {
            self.store_module_graph(ModuleGraphState { linked, roots }, nested_in_evaluation);
        }
        drop(new_names);
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
        if result.is_ok()
            && self.debugger_module_continuation.is_none()
            && drain_jobs
            && entry_is_async
            && !self.promise_jobs.is_empty()
        {
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
                result = Err(self.propagate_module_error(error));
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
        self.finish_module_graph_execution(result)
    }

    pub(super) fn finish_module_graph_execution(
        &mut self,
        result: Result<Value, RuntimeError>,
    ) -> Result<Value, RuntimeError> {
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
        self.disposables = Default::default();
        self.dispose_marks = Default::default();
        if let Err(error) = self.collect_module_garbage() {
            self.uncaught_throw_site = None;
            self.pending_throw_site = None;
            return Err(error);
        }
        self.enqueue_finalization_cleanup_jobs()?;
        self.uncaught_throw_site = if result.as_ref().is_err_and(RuntimeError::is_catchable) {
            self.pending_throw_site
        } else {
            None
        };
        self.pending_throw_site = None;
        self.report_pending_host_module_errors()?;
        result
    }

    /// Roots an object a module record holds (its completion value or its
    /// evaluation error): the records are not heap objects, so the collector
    /// would otherwise reclaim what a later run of the same graph replays.
    /// The root belongs to the graph, which takes it over once it is stored.
    fn keep_module_value_alive(&mut self, value: &Value) -> Result<(), RuntimeError> {
        if let Value::Object(id) = value {
            self.nested_module_roots.root_object(
                &mut self.heap,
                self.host_scheduling.as_ref(),
                *id,
            )?;
        }
        Ok(())
    }

    /// A major collection with everything the VM holds as a root. It reclaims
    /// memory after admitting the collector's native workspace.
    fn collect_module_garbage(&mut self) -> Result<(), RuntimeError> {
        self.with_roots(|heap| heap.try_collect_major())
    }

    pub(super) fn resume_debugger_module_inner(
        &mut self,
        suspension_point: Option<InterpreterSuspensionPoint>,
    ) -> Result<VmDebuggerExecutionState, RuntimeError> {
        self.module_execution_depth += 1;
        let result = self.resume_debugger_module_scoped(suspension_point);
        self.module_execution_depth -= 1;
        result
    }

    fn resume_debugger_module_scoped(
        &mut self,
        suspension_point: Option<InterpreterSuspensionPoint>,
    ) -> Result<VmDebuggerExecutionState, RuntimeError> {
        let continuation =
            self.debugger_module_continuation
                .take()
                .ok_or(RuntimeError::Unsupported(
                    "no debugger-paused root module is available",
                ))?;
        let Some(mut graph) = self.module_graph.take() else {
            // Refusing a resume must retain the frame that still owns its
            // execution state, so restoration of the graph can be retried.
            self.debugger_module_continuation = Some(continuation);
            return Err(RuntimeError::Unsupported(
                "debugger-paused module graph is unavailable",
            ));
        };
        let ModuleDebuggerContinuation {
            module,
            mut module_charge,
            code,
            pc,
            execution,
            mut iterators,
            handlers,
        } = continuation;
        if !module_charge.enabled() {
            module_charge = self.charge_execution_control(module.capacity());
        }
        let previous_module = self.take_module_name();
        self.restore_module_execution(execution);
        let modules = code.registry().clone();
        debug_assert!(self.evaluating_linked.is_none());
        self.evaluating_linked = Some(std::mem::take(&mut graph.linked));
        let outcome = self.interpret(
            &code,
            &mut iterators,
            pc,
            None,
            suspension_point,
            Some((handlers, 0)),
        );
        graph.linked = self
            .evaluating_linked
            .take()
            .expect("paused module records return after the entry body runs");
        let (outcome, mut prepared_cells) = self.stage_module_exit_cells(outcome);
        let outcome = match ModuleRun::classify(outcome) {
            ModuleRun::Paused {
                pc,
                iterators,
                handlers,
            } => {
                let execution = self.suspend_module_execution();
                let record = graph
                    .linked
                    .get_mut(&module)
                    .expect("the stepped entry remains in its linked graph");
                prepared_cells
                    .take()
                    .expect("suspension cell table was admitted")
                    .install(record);
                record.suspended = true;
                previous_module.restore(self);
                self.debugger_module_continuation = Some(ModuleDebuggerContinuation {
                    module,
                    module_charge,
                    code,
                    pc,
                    execution,
                    iterators,
                    handlers,
                });
                self.store_module_graph(graph, false);
                return Ok(VmDebuggerExecutionState::Paused {
                    bytecode_offset: u32::try_from(pc)
                        .expect("verified BlueJS module offsets fit the debugger wire range"),
                });
            }
            ModuleRun::Completed(outcome) => outcome,
        };
        previous_module.restore(self);
        let mut awaiting = false;
        let mut result = match outcome {
            Ok(StraightLineExit::Return(value)) => Ok(value),
            Ok(StraightLineExit::Await {
                promise,
                pc,
                handlers,
            }) => (|| {
                let (next_module, next_module_charge) = self.prepare_module_frame_name(&module)?;
                let execution = self.suspend_module_execution();
                let record = graph
                    .linked
                    .get_mut(&module)
                    .expect("the paused entry remains in its linked graph");
                prepared_cells
                    .take()
                    .expect("suspension cell table was admitted")
                    .install(record);
                record.suspended = true;
                match self.suspend_module_await(
                    ModuleContinuation {
                        module: next_module,
                        module_charge: next_module_charge,
                        code,
                        pc,
                        execution,
                        iterators: std::mem::take(&mut iterators),
                        handlers,
                    },
                    promise,
                ) {
                    Ok(()) => {
                        awaiting = true;
                        Ok(Value::Undefined)
                    }
                    Err(error) => Err(error),
                }
            })(),
            Err(error) => Err(error),
        };
        if !awaiting {
            result = self.finish_debugger_interpret_result(result, &mut iterators, 0, 0);
            let record = graph
                .linked
                .get_mut(&module)
                .expect("the paused entry remains in its linked graph");
            record.cells = std::mem::take(&mut self.cells);
            record.refresh_cells_charge();
            record.suspended = false;
            record.evaluating = false;
            match &result {
                Ok(value) => match self.keep_module_value_alive(value) {
                    Ok(()) => {
                        record.evaluated = true;
                        record.completion = Some(value.clone());
                    }
                    Err(error) => result = Err(error),
                },
                Err(error) => {
                    if error.is_catchable() {
                        match self.error_value(error.clone()) {
                            Ok(value) => {
                                let origin = ThrowOrigin {
                                    source: self.pending_diagnostic_origin.clone(),
                                    debugger_site: self.pending_throw_site,
                                    epoch: self.throw_epoch,
                                };
                                match self.retain_module_error(value.clone(), origin).and_then(
                                    |error| {
                                        self.keep_module_value_alive(&value)?;
                                        Ok(error)
                                    },
                                ) {
                                    Ok(error) => {
                                        record.evaluated = true;
                                        record.error = Some(error);
                                        result = Err(RuntimeError::Thrown(value));
                                    }
                                    Err(error) => result = Err(error),
                                }
                            }
                            Err(error) => result = Err(error),
                        }
                    }
                }
            }
        }
        if result.is_ok() {
            match self.module_namespace(
                &module,
                false,
                &modules,
                &mut graph.linked,
                &mut graph.roots,
            ) {
                Ok(namespace) => {
                    self.last_module_namespace = Some(namespace);
                    match self.heap.root(namespace) {
                        Ok(root) => self.last_module_namespace_root = Some(root),
                        Err(error) => result = Err(error.into()),
                    }
                }
                Err(error) => result = Err(error),
            }
        }
        let error_root = match &result {
            Err(RuntimeError::Thrown(Value::Object(id))) => Some(graph.roots.root_object(
                &mut self.heap,
                self.host_scheduling.as_ref(),
                *id,
            )),
            _ => None,
        };
        self.store_module_graph(graph, false);
        if let Some(Err(error)) = error_root {
            result = Err(error);
        }
        if awaiting && result.is_ok() && !self.promise_jobs.is_empty() {
            self.remaining_instructions = self.config.instruction_budget;
            if let Err(error) = self.run_promise_jobs() {
                result = Err(error);
            }
        }
        if result.is_ok() {
            if let Some(error) = self
                .linked_record(&module)
                .and_then(|record| record.error.clone())
            {
                result = Err(self.propagate_module_error(error));
            } else if let Some(value) = self
                .linked_record(&module)
                .and_then(|record| record.completion.clone())
            {
                result = Ok(value);
            }
        }
        match &result {
            Ok(Value::Object(id)) | Err(RuntimeError::Thrown(Value::Object(id))) => {
                match self.heap.root(*id) {
                    Ok(root) => self.result_root = Some(root),
                    Err(error) => result = Err(error.into()),
                }
            }
            _ => {}
        }
        self.finish_module_graph_execution(result)
            .map(|_| VmDebuggerExecutionState::Completed)
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
        modules: &mut ModuleRegistryDraft,
    ) -> Result<(), RuntimeError> {
        if modules.contains_key(target) {
            return Ok(());
        }
        let Some(source) = self.dynamic_module_sources.get(target) else {
            return Ok(());
        };
        let program = crate::parse_module(source)
            .map_err(|error| RuntimeError::SyntaxError(error.message))?;
        let code = crate::compile_module_with_limit(&program, u32::MAX)
            .map_err(|error| RuntimeError::SyntaxError(error.to_string()))?;
        modules.insert(target, code, self.host_scheduling.as_ref())
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
        // NUL separates a synthetic module's path from its type in the
        // registry key (`ModuleType::module_key`); a specifier spelling one
        // could reach a typed record without the `type` attribute.
        if request.contains('\0') {
            return Err(RuntimeError::ModuleResolution(
                "a module specifier cannot contain a NUL character".into(),
            ));
        }
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
            let argument_epoch = self.throw_epoch;
            match self.evaluate_import_call_arguments(specifier, options) {
                Ok((specifier, module_type)) if phase == ImportPhase::Source => {
                    self.dynamic_import_source(promise, &specifier, module_type)?;
                }
                Ok((specifier, module_type)) => {
                    let referrer = self
                        .active_module_name
                        .clone()
                        .unwrap_or_else(|| "<script>".to_string());
                    self.enqueue_promise_job(PromiseJob::DynamicImport {
                        target: promise,
                        referrer,
                        specifier,
                        module_type,
                        phase,
                    });
                }
                Err(error) => {
                    let origin = self.caught_rejection_origin(argument_epoch);
                    let error = self.error_value(error)?;
                    self.reject_promise_with_origin(promise, error, origin)?;
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
                let error = error.clone();
                return Err(self.propagate_module_error(error));
            }
            if record.evaluated {
                let modules = self.module_registry.clone();
                return self
                    .with_module_records(|vm, linked| {
                        // Every request of an evaluated graph resolves.
                        if let Some(error) = Self::cycle_root_error(&entry, &modules, linked)
                            .expect("every request of the graph resolves")
                        {
                            return Err(vm.propagate_module_error(error));
                        }
                        // A dependency can finish without ever being an entry
                        // or namespace import. Its namespace is then created
                        // here, with permanent cache ownership and temporary
                        // construction roots released after publication.
                        let mut roots = ModuleRoots::default();
                        let result =
                            vm.module_namespace(&entry, false, &modules, linked, &mut roots);
                        for root in roots {
                            vm.release_root(root);
                        }
                        result.map(|namespace| {
                            DynamicImportResult::Fulfilled(Value::Object(namespace))
                        })
                    })
                    // The record of this module was just found in a graph, which
                    // is therefore available.
                    .expect("the graph that holds the module record is available");
            }
            if record.evaluating || record.suspended {
                return Ok(DynamicImportResult::Waiting(entry));
            }
        }
        let modules = self.module_registry.clone();
        self.execute_module_registry_inner(&entry, &modules, false, true, ImportPhase::Evaluation)?;
        if self
            .linked_record(&entry)
            .is_some_and(|record| record.evaluating || record.suspended)
        {
            return Ok(DynamicImportResult::Waiting(entry));
        }
        // A graph that was evaluated without failing recorded its namespace.
        let namespace = self
            .last_module_namespace
            .expect("a completed graph evaluation records the namespace of its entry");
        Ok(DynamicImportResult::Fulfilled(Value::Object(namespace)))
    }

    /// The record of `name` in whichever graph currently owns the module
    /// records: the installed one, or the one parked for running module code.
    pub(super) fn linked_record(&self, name: &str) -> Option<&LinkedModule> {
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
            self.nested_module_roots.merge(state.roots);
        } else {
            state
                .roots
                .merge(std::mem::take(&mut self.nested_module_roots));
            self.module_graph = Some(state);
        }
    }

    pub(super) fn suspend_module_execution(&mut self) -> SuspendedModuleExecution {
        SuspendedModuleExecution {
            disposables: std::mem::take(&mut self.disposables),
            dispose_marks: std::mem::take(&mut self.dispose_marks),
            payload_charge: self.charge_execution_payload(self.current_execution_payload_bytes()),
            control_charge: self.charge_execution_control(self.current_execution_control_bytes()),
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
            new_target: std::mem::replace(&mut self.new_target, Value::Undefined),
            new_target_allowed: std::mem::replace(&mut self.new_target_allowed, false),
            home_object: self.home_object.take(),
            class_field_initializer: std::mem::take(&mut self.class_field_initializer),
            active_module_name: self.active_module_name.take(),
        }
    }

    pub(super) fn restore_module_execution(&mut self, execution: SuspendedModuleExecution) {
        self.disposables = execution.disposables;
        self.dispose_marks = execution.dispose_marks;
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
        self.new_target = execution.new_target;
        self.new_target_allowed = execution.new_target_allowed;
        self.home_object = execution.home_object;
        self.class_field_initializer = execution.class_field_initializer;
        self.active_module_name = execution.active_module_name;
        drop(execution.payload_charge);
        drop(execution.control_charge);
        self.reconcile_scheduling_ownership();
    }

    pub(super) fn run_next_job_while_module_suspended(&mut self) -> Result<bool, RuntimeError> {
        self.check_scheduling_failure()?;
        if self.promise_jobs.is_empty() {
            return Ok(false);
        }
        let execution = self.suspend_module_execution();
        let roots = match self.root_suspended_module_execution(&execution) {
            Ok(roots) => roots,
            Err(error) => {
                self.restore_module_execution(execution);
                return Err(error);
            }
        };
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
            self.release_root(root);
        }
        self.restore_module_execution(execution);
        self.call_depth = call_depth;
        for root in roots {
            self.release_root(root);
        }
        result
    }

    pub(super) fn resume_module_await(
        &mut self,
        continuation: u64,
        value: Value,
        fulfilled: bool,
    ) -> Result<(), RuntimeError> {
        let ambient = self.suspend_module_execution();
        let root_base = self.async_frame_roots.len();
        if let Err(error) =
            self.extend_async_frame_roots(Self::suspended_execution_references(&ambient))
        {
            self.restore_module_execution(ambient);
            return Err(error);
        }
        self.module_execution_depth += 1;
        let result = self.resume_module_await_scoped(continuation, value, fulfilled);
        self.module_execution_depth -= 1;
        self.restore_module_execution(ambient);
        self.async_frame_roots.truncate(root_base);
        result
    }

    fn resume_module_await_scoped(
        &mut self,
        continuation: u64,
        value: Value,
        fulfilled: bool,
    ) -> Result<(), RuntimeError> {
        // A reaction names the continuation of the await it was made for, and
        // fires once.
        let continuation = self
            .module_continuations
            .remove(&continuation)
            .expect("an await reaction names a stored continuation");
        let ModuleContinuation {
            module,
            mut module_charge,
            code,
            pc,
            execution,
            mut iterators,
            handlers,
        } = continuation;
        if !module_charge.enabled() {
            module_charge = self.charge_execution_control(module.capacity());
        }
        self.restore_module_execution(execution);
        let _module_charge = module_charge;
        let resume_epoch = self.throw_epoch;
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
            self.interpret_rejected_await(
                &code,
                &mut iterators,
                pc,
                handlers,
                value,
                "top-level await cannot recur",
            )
        };
        let mut graph = self
            .module_graph
            .take()
            .ok_or(RuntimeError::Unsupported("module graph continuation"))?;
        let (outcome, mut prepared_cells) = self.stage_module_exit_cells(outcome);
        let outcome = straight_line_exit(outcome, "yield requires a generator function");
        // Keep the graph installed even if a later settlement, allocation,
        // or continuation registration refuses this turn.
        let result = (|| match outcome {
            Ok(StraightLineExit::Return(value)) => {
                self.keep_module_value_alive(&value)?;
                {
                    let record = graph
                        .linked
                        .get_mut(&module)
                        .expect("suspended module remains linked");
                    record.cells = std::mem::take(&mut self.cells);
                    record.refresh_cells_charge();
                    record.evaluating = false;
                    record.suspended = false;
                    record.evaluated = true;
                    record.completion = Some(value.clone());
                }
                let modules = self.module_registry.clone();
                self.settle_dynamic_import_waiters(&module, &mut graph.linked)?;
                self.settle_module_parents(module.clone(), &modules, &mut graph.linked)?;
                Ok(value)
            }
            Ok(StraightLineExit::Await {
                promise,
                pc,
                handlers,
            }) => {
                let (next_module, next_module_charge) = self.prepare_module_frame_name(&module)?;
                let execution = self.suspend_module_execution();
                let record = graph
                    .linked
                    .get_mut(&module)
                    .expect("suspended module remains linked");
                prepared_cells
                    .take()
                    .expect("suspension cell table was admitted")
                    .install(record);
                record.suspended = true;
                self.suspend_module_await(
                    ModuleContinuation {
                        module: next_module,
                        module_charge: next_module_charge,
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
            Err(error) => {
                let origin = self.caught_rejection_origin(resume_epoch);
                let value = self.error_value(error)?;
                let error = self.retain_module_error(value, origin)?;
                self.keep_module_value_alive(&error.value)?;
                let record = graph
                    .linked
                    .get_mut(&module)
                    .expect("suspended module remains linked");
                record.cells = std::mem::take(&mut self.cells);
                record.refresh_cells_charge();
                record.evaluating = false;
                record.suspended = false;
                record.evaluated = true;
                record.completion = None;
                record.error = Some(error.clone());
                self.reject_module_and_parents(module.clone(), error, &mut graph.linked)?;
                Ok(Value::Undefined)
            }
        })();
        let error_root = graph
            .linked
            .get(&module)
            .and_then(|record| record.error.as_ref())
            .and_then(|error| error.value.object_id())
            .map(|id| {
                graph
                    .roots
                    .root_object(&mut self.heap, self.host_scheduling.as_ref(), id)
            });
        self.module_graph = Some(graph);
        if let Some(Err(error)) = error_root {
            return Err(error);
        }
        result.map(|_| ())
    }

    /// Completing an async dependency releases the modules that were waiting
    /// on it during InnerModuleEvaluation. Process one breadth-first layer at
    /// a time: siblings retain source/DFS order, while an indirect parent is
    /// considered only after its direct siblings have had their turn.
    pub(super) fn settle_module_parents(
        &mut self,
        completed: String,
        modules: &ModuleRegistry,
        linked: &mut ModuleRecords,
    ) -> Result<(), RuntimeError> {
        let result = self.settle_module_parents_owned(completed, modules, linked);
        self.reconcile_scheduling_ownership();
        result
    }

    fn settle_module_parents_owned(
        &mut self,
        completed: String,
        modules: &ModuleRegistry,
        linked: &mut ModuleRecords,
    ) -> Result<(), RuntimeError> {
        let mut completed = std::collections::VecDeque::from([completed]);
        while let Some(child) = completed.pop_front() {
            let mut work_payload = self.charge_execution_payload(
                module_dependencies::string_queue_bytes(&completed)
                    .saturating_add(child.capacity()),
            );
            let parents = self.module_async_parents.remove(&child).unwrap_or_default();
            let capacity = parents.capacity();
            let mut parents = parents.into_iter();
            let mut parents_payload = self.charge_execution_payload(
                module_dependencies::string_iterator_bytes(capacity, parents.as_slice()),
            );
            self.reclaim_module_dependency_storage();
            self.synchronize_scheduling_usage()?;
            while let Some(parent) = parents.next() {
                let parent_payload = self.charge_execution_payload(parent.capacity());
                drop(parents_payload);
                parents_payload = self.charge_execution_payload(
                    module_dependencies::string_iterator_bytes(capacity, parents.as_slice()),
                );
                self.synchronize_scheduling_usage()?;
                let ready = self
                    .module_pending_dependencies
                    .get_mut(&parent)
                    .is_some_and(|dependencies| {
                        dependencies.remove(&child);
                        if dependencies.len().saturating_mul(4) < dependencies.capacity() {
                            dependencies.shrink_to_fit();
                        }
                        dependencies.is_empty()
                    });
                if !ready {
                    continue;
                }
                self.remove_module_dependency_parent(&parent);
                self.synchronize_scheduling_usage()?;
                let record = linked
                    .get_mut(&parent)
                    .expect("async parent remains linked");
                record.evaluating = false;
                record.suspended = false;
                if let Err(error) = self.evaluate_module_record(&parent, modules, linked) {
                    let retained = if error.is_catchable() {
                        linked.get(&parent).and_then(|record| record.error.clone())
                    } else {
                        None
                    };
                    let Some(retained) = retained else {
                        return Err(error);
                    };
                    // A ready parent's language failure is a module rejection,
                    // not a second exception from this dependency's Promise
                    // job. Settle its waiters/ancestors and continue the ready
                    // siblings. Resource aborts still escape unchanged.
                    drop(parent_payload);
                    self.reject_module_and_parents(parent, retained, linked)?;
                    continue;
                }
                if linked.get(&parent).is_some_and(|record| record.evaluated) {
                    self.settle_dynamic_import_waiters(&parent, linked)?;
                    completed.push_back(parent);
                    drop(parent_payload);
                    drop(work_payload);
                    work_payload = self.charge_execution_payload(
                        module_dependencies::string_queue_bytes(&completed)
                            .saturating_add(child.capacity()),
                    );
                    self.synchronize_scheduling_usage()?;
                }
            }
            drop(parents);
            drop(parents_payload);
        }
        drop(completed);
        self.reclaim_module_dependency_storage();
        self.synchronize_scheduling_usage()
    }

    /// Propagate an async module evaluation error through its static parents.
    /// Dynamic imports observe rejection from each affected module, while the
    /// surrounding promise checkpoint continues processing independent jobs.
    pub(super) fn reject_module_and_parents(
        &mut self,
        module: String,
        error: ModuleEvaluationError,
        linked: &mut HashMap<String, LinkedModule>,
    ) -> Result<(), RuntimeError> {
        let result = self.reject_module_and_parents_owned(module, error, linked);
        self.reconcile_scheduling_ownership();
        result
    }

    fn reject_module_and_parents_owned(
        &mut self,
        module: String,
        error: ModuleEvaluationError,
        linked: &mut HashMap<String, LinkedModule>,
    ) -> Result<(), RuntimeError> {
        let mut rejected = std::collections::VecDeque::from([module]);
        while let Some(module) = rejected.pop_front() {
            let mut work_payload = self.charge_execution_payload(
                module_dependencies::string_queue_bytes(&rejected)
                    .saturating_add(module.capacity()),
            );
            self.remove_module_dependency_parent(&module);
            self.synchronize_scheduling_usage()?;
            self.reject_dynamic_import_waiters(&module, error.clone())?;
            let parents = self
                .module_async_parents
                .remove(&module)
                .unwrap_or_default();
            let capacity = parents.capacity();
            let mut parents = parents.into_iter();
            let mut parents_payload = self.charge_execution_payload(
                module_dependencies::string_iterator_bytes(capacity, parents.as_slice()),
            );
            self.reclaim_module_dependency_storage();
            self.synchronize_scheduling_usage()?;
            while let Some(parent) = parents.next() {
                let parent_payload = self.charge_execution_payload(parent.capacity());
                drop(parents_payload);
                parents_payload = self.charge_execution_payload(
                    module_dependencies::string_iterator_bytes(capacity, parents.as_slice()),
                );
                self.remove_module_dependency_parent(&parent);
                self.synchronize_scheduling_usage()?;
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
                self.keep_module_value_alive(&error.value)?;
                record.error = Some(error.clone());
                rejected.push_back(parent);
                drop(parent_payload);
                drop(work_payload);
                work_payload = self.charge_execution_payload(
                    module_dependencies::string_queue_bytes(&rejected)
                        .saturating_add(module.capacity()),
                );
                self.synchronize_scheduling_usage()?;
            }
            drop(parents);
            drop(parents_payload);
        }
        drop(rejected);
        self.reclaim_module_dependency_storage();
        self.synchronize_scheduling_usage()
    }

    /// Resolves every waiting `import()` with its namespace, observing an
    /// exported `then` through the same Promise resolution as an eager import.
    pub(super) fn settle_dynamic_import_waiters(
        &mut self,
        module: &str,
        linked: &mut HashMap<String, LinkedModule>,
    ) -> Result<(), RuntimeError> {
        // Importing an already suspended dependency registers a waiter without
        // executing a new graph entry. Such a module need not have a namespace
        // yet. Allocate it before removing any waiter, retaining both queues if
        // construction refuses memory or root registration.
        let namespace = if self.module_import_waiters.contains_key(module) {
            let modules = self.module_registry.clone();
            let mut roots = ModuleRoots::default();
            let result = self.module_namespace(module, false, &modules, linked, &mut roots);
            for root in roots {
                self.release_root(root);
            }
            Some(result?)
        } else {
            None
        };
        // An `import.defer()` resolves once the last asynchronous dependency
        // it was waiting for has finished.
        let mut finished = Vec::new();
        for waiter in &mut self.deferred_import_waiters {
            waiter.pending.remove(module);
            if waiter.pending.len().saturating_mul(4) < waiter.pending.capacity() {
                waiter.pending.shrink_to_fit();
            }
        }
        self.deferred_import_waiters.retain(|waiter| {
            if waiter.pending.is_empty() {
                finished.push((waiter.promise, waiter.namespace));
                false
            } else {
                true
            }
        });
        if let Some(namespace) = namespace {
            let waiters = self
                .module_import_waiters
                .remove(module)
                .expect("namespace construction cannot remove the retained import waiters");
            finished.extend(waiters.into_iter().map(|promise| (promise, namespace)));
        }
        self.reclaim_import_waiter_storage();
        self.synchronize_scheduling_usage()?;
        for (promise, namespace) in finished {
            self.resolve_promise(promise, Value::Object(namespace))
                .expect("completed import waiters retain initialized ordinary namespace exports or deferred namespaces without then; Promise resolution only queues callable exports");
        }
        self.synchronize_scheduling_usage()
    }

    /// Rejects every `import()` promise that waited for `module` with `error`.
    pub(super) fn reject_dynamic_import_waiters(
        &mut self,
        module: &str,
        error: ModuleEvaluationError,
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
        self.reclaim_import_waiter_storage();
        self.synchronize_scheduling_usage()?;
        for promise in failed {
            self.reject_promise_with_origin(
                promise,
                error.value.clone(),
                error.origin.as_ref().clone(),
            )?;
        }
        let Some(waiters) = self.module_import_waiters.remove(module) else {
            return self.synchronize_scheduling_usage();
        };
        self.reclaim_import_waiter_storage();
        self.synchronize_scheduling_usage()?;
        for promise in waiters {
            self.reject_promise_with_origin(
                promise,
                error.value.clone(),
                error.origin.as_ref().clone(),
            )?;
        }
        self.synchronize_scheduling_usage()
    }

    /// Registers a module frame on the Promise it awaits. A fulfilled input
    /// still goes through the job queue, preserving the required asynchronous
    /// boundary before the frame resumes.
    pub(super) fn suspend_module_await(
        &mut self,
        continuation_state: ModuleContinuation,
        promise: ObjectId,
    ) -> Result<(), RuntimeError> {
        self.mark_promise_handled(promise);
        // Validate before publishing a frame or consuming its serial.
        let status = self
            .promises
            .get(&promise)
            .ok_or(RuntimeError::TypeError("invalid await Promise".into()))?
            .clone_for_await();
        let continuation = self.next_module_continuation;
        let next = self
            .next_module_continuation
            .checked_add(1)
            .ok_or(RuntimeError::InstructionLimit)?;
        let work = match status {
            PromiseAwaitStatus::Pending => AwaitRegistration::Reaction {
                promise,
                reaction: PromiseReaction::ModuleAwait { continuation },
            },
            PromiseAwaitStatus::Fulfilled(value) => {
                AwaitRegistration::Job(PromiseJob::ModuleAwait {
                    continuation,
                    value,
                    fulfilled: true,
                    origin: ThrowOrigin::default(),
                })
            }
            PromiseAwaitStatus::Rejected(value, origin) => {
                AwaitRegistration::Job(PromiseJob::ModuleAwait {
                    continuation,
                    value,
                    fulfilled: false,
                    origin,
                })
            }
        };
        let growth = SchedulingMapCapacity::for_insert(
            &self.module_continuations,
            self.host_scheduling.as_ref(),
        )?;
        work.admit(
            self,
            HostSchedulingUsage {
                payload_bytes: continuation_state.scheduling_payload_bytes(),
                ..HostSchedulingUsage::default()
            }
            .with_map_growth(&growth),
        )?;
        growth.commit(&mut self.module_continuations);
        self.next_module_continuation = next;
        self.module_continuations
            .insert(continuation, continuation_state);
        work.publish(self)
    }

    /// Registers an ordinary async-function frame on the Promise it awaits
    /// (see [`Self::suspend_async_frame`]). Promise validation and serial
    /// reservation precede publication of the continuation.
    pub(super) fn suspend_async_await(
        &mut self,
        continuation_state: AsyncContinuation,
        promise: ObjectId,
    ) -> Result<(), RuntimeError> {
        self.suspend_async_frame(continuation_state, promise)
    }

    /// Registers an ordinary async-function frame on the Promise it awaits.
    /// A fulfilled input still goes through the job queue, preserving the
    /// required asynchronous boundary before the frame resumes.
    fn suspend_async_frame(
        &mut self,
        continuation_state: AsyncContinuation,
        promise: ObjectId,
    ) -> Result<(), RuntimeError> {
        self.mark_promise_handled(promise);
        // Validate before publishing a frame or consuming its serial.
        let status = self
            .promises
            .get(&promise)
            .ok_or(RuntimeError::TypeError("invalid await Promise".into()))?
            .clone_for_await();
        let continuation = self.next_async_continuation;
        let next = self
            .next_async_continuation
            .checked_add(1)
            .ok_or(RuntimeError::InstructionLimit)?;
        let work = match status {
            PromiseAwaitStatus::Pending => AwaitRegistration::Reaction {
                promise,
                reaction: PromiseReaction::AsyncAwait { continuation },
            },
            PromiseAwaitStatus::Fulfilled(value) => {
                AwaitRegistration::Job(PromiseJob::AsyncAwait {
                    continuation,
                    value,
                    fulfilled: true,
                    origin: ThrowOrigin::default(),
                })
            }
            PromiseAwaitStatus::Rejected(value, origin) => {
                AwaitRegistration::Job(PromiseJob::AsyncAwait {
                    continuation,
                    value,
                    fulfilled: false,
                    origin,
                })
            }
        };
        let growth = SchedulingMapCapacity::for_insert(
            &self.async_continuations,
            self.host_scheduling.as_ref(),
        )?;
        work.admit(
            self,
            HostSchedulingUsage {
                payload_bytes: continuation_state.scheduling_payload_bytes(),
                ..HostSchedulingUsage::default()
            }
            .with_map_growth(&growth),
        )?;
        growth.commit(&mut self.async_continuations);
        self.next_async_continuation = next;
        self.async_continuations
            .insert(continuation, continuation_state);
        work.publish(self)
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
        // Reserve while the stored continuation still owns its frame. A
        // refusal must not consume its only resumption owner.
        let state = self
            .async_continuations
            .get(&continuation)
            .expect("an await reaction names a stored continuation");
        let additional = usize::from(state.generator.is_some())
            + 1
            + Self::suspended_execution_references(&state.execution).count()
            + state.iterators.iter().filter_map(Value::object_id).count();
        self.prepare_async_frame_root_slots(additional)?;
        // A reaction names the continuation of the await it was made for, and
        // fires once.
        let continuation = self
            .async_continuations
            .remove(&continuation)
            .expect("an await reaction names a stored continuation");
        let AsyncContinuation {
            generator,
            target,
            code,
            pc,
            execution,
            mut iterators,
            handlers,
            call_depth,
        } = continuation;
        let resume_epoch = self.throw_epoch;
        let root_base = self.async_frame_roots.len();
        // All of these slots were admitted before removing the continuation.
        self.async_frame_roots.extend(generator);
        self.async_frame_roots.push(target);
        self.async_frame_roots
            .extend(Self::suspended_execution_references(&execution));
        self.async_frame_roots
            .extend(iterators.iter().filter_map(Value::object_id));
        let result = (|| {
            if let Some(generator) = generator {
                return self.resume_async_generator_await(
                    generator, target, code, pc, execution, iterators, handlers, call_depth, value,
                    fulfilled,
                );
            }
            let ambient = self.suspend_module_execution();
            if let Err(error) =
                self.extend_async_frame_roots(Self::suspended_execution_references(&ambient))
            {
                self.restore_module_execution(ambient);
                return Err(error);
            }
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
                self.interpret_rejected_await(
                    &code,
                    &mut iterators,
                    pc,
                    handlers,
                    value,
                    "async function cannot tail recur across await",
                )
            };

            let outcome = straight_line_exit(outcome, "yield requires an async generator function");
            let result = match outcome {
                Ok(StraightLineExit::Return(value)) => {
                    self.restore_module_execution(ambient);
                    self.call_depth = ambient_call_depth;
                    self.resolve_promise(target, value)
                }
                Ok(StraightLineExit::Await {
                    promise,
                    pc,
                    handlers,
                }) => {
                    let execution = self.suspend_module_execution();
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
                    self.suspend_async_frame(state, promise)?;
                    Ok(())
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
                    while let Some(record) = iterators.pop() {
                        self.iterator_close_preserving_throw(&record);
                    }
                    self.stack.truncate(base);
                    self.restore_module_execution(ambient);
                    self.call_depth = ambient_call_depth;
                    let origin = self.caught_rejection_origin(resume_epoch);
                    let error = self.error_value(error)?;
                    self.reject_promise_with_origin(target, error, origin)
                }
            };
            result
        })();
        self.async_frame_roots.truncate(root_base);
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
        mut iterators: IteratorRecords,
        handlers: HandlerRecords,
        call_depth: usize,
        value: Value,
        fulfilled: bool,
    ) -> Result<(), RuntimeError> {
        let mut ambient = Some(self.suspend_module_execution());
        if let Err(error) = self.extend_async_frame_roots(Self::suspended_execution_references(
            ambient
                .as_ref()
                .expect("the async job's displaced frame is present"),
        )) {
            self.restore_module_execution(
                ambient.take().expect("the ambient frame is still staged"),
            );
            return Err(error);
        }
        let ambient_call_depth = std::mem::replace(&mut self.call_depth, call_depth);
        self.restore_module_execution(execution);
        let resume_epoch = self.throw_epoch;
        let result = (|| {
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
                self.interpret_rejected_await(
                    &code,
                    &mut iterators,
                    pc,
                    handlers,
                    value,
                    "async generator cannot tail recur across await",
                )
            };

            match async_generator_exit(outcome) {
                Ok(AsyncGeneratorExit::Return(value)) => {
                    self.heap
                        .set_generator_state(generator, GeneratorState::Done)
                        .expect("the resumed async frame retains its generator; Done cannot grow");
                    let ambient = ambient
                        .take()
                        .expect("each async generator turn restores its displaced frame once");
                    self.restore_module_execution(ambient);
                    self.call_depth = ambient_call_depth;
                    let result = self.iterator_result(value, true)?;
                    self.finish_async_generator_run(generator, target, result)
                }
                Ok(AsyncGeneratorExit::Yield {
                    value,
                    pc,
                    iterators,
                    handlers,
                }) => {
                    let mut storage =
                        self.prepare_suspension_storage(0, true).inspect_err(|_| {
                            let _ = self
                                .heap
                                .set_generator_state(generator, GeneratorState::Done);
                        })?;
                    let stack = storage.take_stack(self);
                    let pending = storage.take_pending(self);
                    let async_delegate = code
                        .async_yield_delegates
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
                        disposables: std::mem::take(&mut self.disposables).into_vec(),
                        dispose_marks: std::mem::take(&mut self.dispose_marks).into_vec(),
                        code,
                        pc,
                        stack,
                        bindings: std::mem::take(&mut self.bindings),
                        cells: std::mem::take(&mut self.cells),
                        this: std::mem::replace(&mut self.this, Value::Undefined),
                        args: std::mem::take(&mut self.arguments),
                        completion: std::mem::replace(&mut self.completion, Value::Undefined),
                        completion_empty: std::mem::replace(&mut self.completion_empty, true),
                        active_scopes: std::mem::take(&mut self.active_scopes),
                        iterators: iterators.into_vec(),
                        handlers: handlers.into_vec(),
                        pending_completions: pending,
                        completion_saves: std::mem::take(&mut self.completion_saves),
                        async_delegate,
                        delegate: None,
                        dynamic_bindings: std::mem::take(&mut self.dynamic_eval_bindings),
                        home: std::mem::take(&mut self.home_object),
                        callee: std::mem::replace(&mut self.callee, Value::Undefined),
                        with_objects: std::mem::take(&mut self.with_objects),
                    };
                    self.extend_async_frame_roots(value.object_id().into_iter())?;
                    storage.into_generator(self, state).store(self, generator)?;
                    let ambient = ambient
                        .take()
                        .expect("each async generator turn restores its displaced frame once");
                    self.restore_module_execution(ambient);
                    self.call_depth = ambient_call_depth;
                    let result = self.iterator_result(value, false)?;
                    self.finish_async_generator_run(generator, target, result)
                }
                Ok(AsyncGeneratorExit::Await {
                    promise,
                    pc,
                    handlers,
                }) => {
                    let execution = self.suspend_module_execution();
                    let ambient = ambient
                        .take()
                        .expect("each async generator turn restores its displaced frame once");
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
                    self.suspend_async_frame(state, promise)?;
                    Ok(())
                }
                Err(error) => {
                    let base = self.stack.len();
                    if let RuntimeError::Thrown(value) = &error {
                        self.stack.push(value.clone());
                    }
                    self.stack.extend(iterators.iter().cloned());
                    while let Some(record) = iterators.pop() {
                        self.iterator_close_preserving_throw(&record);
                    }
                    self.stack.truncate(base);
                    self.heap
                        .set_generator_state(generator, GeneratorState::Done)
                        .expect("the resumed async frame retains its generator; Done cannot grow");
                    let ambient = ambient
                        .take()
                        .expect("each async generator turn restores its displaced frame once");
                    self.restore_module_execution(ambient);
                    self.call_depth = ambient_call_depth;
                    let origin = self.caught_rejection_origin(resume_epoch);
                    let value = self.error_value(error)?;
                    self.with_rejection_origin(Some(origin), |vm| {
                        vm.complete_async_generator_request(
                            generator,
                            target,
                            PromiseStatus::Rejected(value),
                        )
                    })?;
                    self.resume_async_generator_next(generator)
                }
            }
        })();
        // A failed frame store or iterator-result allocation must restore the
        // caller just as a successful turn does.
        if let Some(ambient) = ambient {
            self.restore_module_execution(ambient);
            self.call_depth = ambient_call_depth;
        }
        result
    }

    #[cfg(any(test, coverage))]
    pub(super) fn enter_module_record(
        &mut self,
        code: &Bytecode,
        cells: &mut HashMap<usize, ObjectId>,
    ) -> Result<(), RuntimeError> {
        self.enter_module_record_storage(code, cells, None)
    }

    fn enter_module_record_storage(
        &mut self,
        code: &Bytecode,
        cells: &mut HashMap<usize, ObjectId>,
        cells_charge: Option<&mut execution_payload::ExecutionPayloadCharge>,
    ) -> Result<(), RuntimeError> {
        let mut prepared = self.prepare_frame_bindings(code, FrameBindingKind::Script)?;
        let (bindings, metadata, _) = prepared.take_parts();
        self.stack.clear();
        self.bindings = bindings;
        self.binding_metadata = metadata;
        self.cells = std::mem::take(cells);
        if let Some(charge) = cells_charge {
            charge.resize(0);
        }

        drop(prepared);
        self.completion = Value::Undefined;
        self.completion_empty = true;
        self.active_scopes.clear();
        self.active_scope_slots.clear();
        self.with_objects.clear();
        self.script_global_slots.clear();
        self.strict = true;
        self.this = Value::Undefined;
        self.top_level_module = true;
        Ok(())
    }

    #[cfg(any(test, coverage))]
    pub(super) fn initialize_module_record(
        &mut self,
        name: &str,
        code: &Bytecode,
        cells: &mut HashMap<usize, ObjectId>,
    ) -> Result<(), RuntimeError> {
        let result = self.initialize_module_record_inner(name, code, cells, None);
        self.reconcile_scheduling_ownership();
        result
    }

    fn initialize_linked_module_record(
        &mut self,
        name: &str,
        code: &Bytecode,
        record: &mut LinkedModule,
    ) -> Result<(), RuntimeError> {
        let result = self.initialize_module_record_inner(
            name,
            code,
            &mut record.cells,
            Some(&mut record.cells_charge),
        );
        self.reconcile_scheduling_ownership();
        result
    }

    fn initialize_module_record_inner(
        &mut self,
        name: &str,
        code: &Bytecode,
        cells: &mut HashMap<usize, ObjectId>,
        mut cells_charge: Option<&mut execution_payload::ExecutionPayloadCharge>,
    ) -> Result<(), RuntimeError> {
        // Only a module compiled as one gets here, and it has an entry.
        let entry = code
            .module_evaluate_entry
            .expect("a module has an evaluation entry") as usize;
        let mut previous_module = self.prepare_module_name(name)?;
        self.enter_module_record_storage(code, cells, cells_charge.as_deref_mut())?;
        let mut iterators = self.iterator_records(Vec::new());
        // The declaration prefix instantiates the module's hoisted functions;
        // each records the module it is created in (its [[ScriptOrModule]]),
        // which `import.meta` and `import()` inside it resolve against no
        // matter which module later calls it.
        previous_module.swap_into(self);
        let result = self.interpret(
            code,
            &mut iterators,
            0,
            None,
            Some(InterpreterSuspensionPoint::Offset(entry)),
            None,
        );
        previous_module.restore(self);
        *cells = std::mem::take(&mut self.cells);
        if let Some(charge) = cells_charge {
            charge.resize(if charge.enabled() {
                host_scheduling::map_slots(cells)
            } else {
                0
            });
        }
        self.stack.clear();
        self.active_scopes.clear();
        self.active_scope_slots.clear();
        expect_entry_suspend(result?, entry)
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
        modules: &ModuleRegistry,
        linked: &mut ModuleRecords,
    ) -> Result<Value, RuntimeError> {
        // Loading the requested modules checked that every module of the graph
        // exists, and linking made a record for each.
        let record = linked
            .get(name)
            .expect("every module of the graph is linked");
        if let Some(error) = &record.error {
            let error = error.clone();
            return Err(self.propagate_module_error(error));
        }
        if record.evaluated || record.evaluating {
            return Ok(Value::Undefined);
        }
        linked
            .get_mut(name)
            .expect("checked module record exists")
            .evaluating = true;
        let evaluation_epoch = self.throw_epoch;
        let result = (|| {
            let code = modules.code(name);
            let mut pending_dependencies = Vec::new();
            for request in &code.module_requests {
                // Loading the requested modules resolved every request.
                let target =
                    Self::resolve_module_target(name, &request.module_request, request.module_type)
                        .expect("every request of the graph resolves");
                // A deferred request contributes only the asynchronous part of
                // its graph; an eager one contributes the module itself.
                let evaluation_list = if request.deferred {
                    Self::gather_async_dependencies(&target, modules, linked, &mut HashSet::new())
                        .expect("every request of the graph resolves")
                } else {
                    vec![target]
                };
                let capacity = evaluation_list.capacity();
                let mut evaluation_list = evaluation_list.into_iter();
                while let Some(target) = evaluation_list.next() {
                    let executes = linked.get(&target).is_some_and(|record| {
                        !record.evaluated && !record.evaluating && record.error.is_none()
                    });
                    // Earlier pending names and later evaluation-list entries
                    // stay Rust-owned while this dependency can execute JS.
                    let _pending_payload = self.charge_execution_payload(if executes {
                        module_dependencies::string_vector_bytes(&pending_dependencies)
                    } else {
                        0
                    });
                    let _evaluation_payload = self.charge_execution_payload(if executes {
                        module_dependencies::string_iterator_bytes(
                            capacity,
                            evaluation_list.as_slice(),
                        )
                        .saturating_add(target.capacity())
                    } else {
                        0
                    });
                    if executes {
                        self.synchronize_scheduling_usage()?;
                    }
                    self.evaluate_module_record(&target, modules, linked)?;
                    if linked.get(&target).is_some_and(|record| record.suspended) {
                        // `target` is part of the graph, whose requests all
                        // resolve.
                        pending_dependencies.push(
                            self.async_dependency_root(&target, modules, linked)
                                .expect("every request of the graph resolves"),
                        );
                    }
                }
            }
            if !pending_dependencies.is_empty() {
                self.register_async_module_dependencies(name, pending_dependencies)?;
                linked
                    .get_mut(name)
                    .expect("checked module record exists")
                    .suspended = true;
                return Ok(Value::Undefined);
            }
            // Linking already instantiated this module's declarations.
            let entry = code
                .module_evaluate_entry
                .expect("a linked module has an evaluation entry") as usize;
            let mut previous_module = self.prepare_module_name(name)?;
            let record = linked.get_mut(name).expect("checked module record exists");
            self.enter_module_record_storage(
                &code,
                &mut record.cells,
                Some(&mut record.cells_charge),
            )?;
            // A sibling Source Text Module runs in a fresh execution context.
            // A suspended async dependency deliberately leaves its displaced
            // frame with zero ambient fuel, which must not exhaust this
            // independent module before its first instruction.
            self.remaining_instructions = self.config.instruction_budget;
            self.enter_scope(&code, 0)?;
            let mut iterators = self.iterator_records(Vec::new());
            previous_module.swap_into(self);
            // Park the graph's records where user code can reach them: a
            // deferred namespace observed by this module (or a module it
            // triggers) evaluates other modules of the same graph.
            self.evaluating_linked = Some(std::mem::take(linked));
            let suspension = self
                .debugger_module_pause_request
                .as_ref()
                .filter(|request| request.entry == name)
                .map(|request| InterpreterSuspensionPoint::Offset(request.target));
            let outcome = self.interpret(&code, &mut iterators, entry, None, suspension, None);
            *linked = self
                .evaluating_linked
                .take()
                .expect("module records are restored after the module body ran");
            let (outcome, mut prepared_cells) = self.stage_module_exit_cells(outcome);
            match ModuleRun::classify(outcome) {
                ModuleRun::Completed(Ok(StraightLineExit::Return(value))) => {
                    previous_module.restore(self);
                    let cells = std::mem::take(&mut self.cells);
                    let record = linked.get_mut(name).expect("checked module record exists");
                    record.cells = cells;
                    record.refresh_cells_charge();
                    self.keep_module_value_alive(&value)?;
                    record.completion = Some(value.clone());
                    Ok(value)
                }
                ModuleRun::Completed(Ok(StraightLineExit::Await {
                    promise,
                    pc,
                    handlers,
                })) => {
                    // Move the realm frame out before another sibling module
                    // evaluates. The continuation is attached to the Await
                    // promise and resumes in its own Promise job turn.
                    let (module, module_charge) = match self.prepare_module_frame_name(name) {
                        Ok(prepared) => prepared,
                        Err(error) => {
                            previous_module.restore(self);
                            linked
                                .get_mut(name)
                                .expect("checked module record exists")
                                .cells = std::mem::take(&mut self.cells);
                            linked
                                .get_mut(name)
                                .expect("checked module record exists")
                                .refresh_cells_charge();
                            return Err(error);
                        }
                    };
                    let execution = self.suspend_module_execution();
                    previous_module.restore(self);
                    let record = linked.get_mut(name).expect("checked module record exists");
                    prepared_cells
                        .take()
                        .expect("suspension cell table was admitted")
                        .install(record);
                    record.suspended = true;
                    self.suspend_module_await(
                        ModuleContinuation {
                            module,
                            module_charge,
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
                ModuleRun::Paused {
                    pc,
                    iterators,
                    handlers,
                } => {
                    let (module, module_charge) = match self.prepare_module_frame_name(name) {
                        Ok(prepared) => prepared,
                        Err(error) => {
                            previous_module.restore(self);
                            linked
                                .get_mut(name)
                                .expect("checked module record exists")
                                .cells = std::mem::take(&mut self.cells);
                            linked
                                .get_mut(name)
                                .expect("checked module record exists")
                                .refresh_cells_charge();
                            return Err(error);
                        }
                    };
                    let execution = self.suspend_module_execution();
                    let record = linked.get_mut(name).expect("checked module record exists");
                    prepared_cells
                        .take()
                        .expect("suspension cell table was admitted")
                        .install(record);
                    record.suspended = true;
                    previous_module.restore(self);
                    self.debugger_module_continuation = Some(ModuleDebuggerContinuation {
                        module,
                        module_charge,
                        code: code.clone(),
                        pc,
                        execution,
                        iterators,
                        handlers,
                    });
                    Ok(Value::Undefined)
                }
                ModuleRun::Completed(Err(error)) => {
                    previous_module.restore(self);
                    let cells = std::mem::take(&mut self.cells);
                    let record = linked.get_mut(name).expect("checked module record exists");
                    record.cells = cells;
                    record.refresh_cells_charge();
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
            Err(error)
                if error.is_catchable()
                    && !linked.get(name).is_some_and(|record| record.suspended) =>
            {
                let origin = self.caught_rejection_origin(evaluation_epoch);
                let value = self.error_value(error)?;
                let retained = self.retain_module_error(value.clone(), origin)?;
                let record = linked.get_mut(name).expect("checked module record exists");
                record.evaluating = false;
                self.keep_module_value_alive(&value)?;
                record.evaluated = true;
                record.completion = None;
                record.error = Some(retained);
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

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/module_linking_internal.rs"]
mod tests;
