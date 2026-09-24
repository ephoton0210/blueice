// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

mod deferred;
mod namespace;
mod synthetic;

use super::*;

/// How code that is not a generator -- a module body, an async function body
/// -- finishes a run: it returns, or it awaits. It never yields, and no
/// running code suspends at a generator's entry.
enum StraightLineExit {
    Return(Value),
    Await {
        promise: ObjectId,
        pc: usize,
        handlers: Vec<HandlerFrame>,
    },
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
        iterators: Vec<Value>,
        handlers: Vec<HandlerFrame>,
    },
    Await {
        promise: ObjectId,
        pc: usize,
        handlers: Vec<HandlerFrame>,
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
        iterators: &mut Vec<Value>,
        pc: usize,
        mut handlers: Vec<HandlerFrame>,
        value: Value,
        tail_call_message: &'static str,
    ) -> Result<InterpreterExit, RuntimeError> {
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
        iterators: &mut Vec<Value>,
        pc: usize,
        handlers: Vec<HandlerFrame>,
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
                self.release_root(root);
            }
            if let Some(root) = self.last_module_namespace_root.take() {
                self.release_root(root);
            }
            self.last_module_namespace = None;
            self.collect_module_garbage();
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
                        roots.push(self.root_new_object(cell));
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
                                let cell = self.with_roots(|heap| heap.alloc_object(None))?;
                                roots.push(self.root_new_object(cell));
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
                                let cell = self.with_roots(|heap| heap.alloc_object(None))?;
                                roots.push(self.root_new_object(cell));
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
                // Loading the requested modules resolved every request the walk
                // follows.
                let dependencies =
                    Self::gather_async_dependencies(entry, modules, &linked, &mut HashSet::new())
                        .expect("every request of the graph was resolved when it was loaded");
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
            self.last_module_namespace_root = Some(self.root_new_object(namespace));
            if let Value::Object(id) = value {
                self.result_root = Some(self.root_new_object(id));
            }
            Ok(value)
        })();

        // Keep an abrupt object completion observable to the embedding host
        // until the next execution, just as `execute_with_global_bindings`
        // does for scripts.  In particular, Test262 needs to inspect an
        // Error's `name` after a module evaluation rejects.
        if let Err(RuntimeError::Thrown(Value::Object(id))) = &result {
            self.result_root = Some(self.root_new_object(*id));
        }

        if result.is_err() && evaluation_started {
            self.store_module_graph(ModuleGraphState { linked, roots }, nested_in_evaluation);
        } else if result.is_err() {
            if fresh_graph {
                // The whole graph never became usable; discard everything.
                for root in roots {
                    self.release_root(root);
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
                    self.release_root(root);
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
        self.collect_module_garbage();
        self.enqueue_finalization_cleanup_jobs();
        result
    }

    /// Roots an object a module record holds (its completion value or its
    /// evaluation error): the records are not heap objects, so the collector
    /// would otherwise reclaim what a later run of the same graph replays.
    /// The root belongs to the graph, which takes it over once it is stored.
    fn keep_module_value_alive(&mut self, value: &Value) {
        if let Value::Object(id) = value {
            let root = self.root_new_object(*id);
            self.nested_module_roots.push(root);
        }
    }

    /// A major collection with everything the VM holds as a root. It reclaims
    /// memory and cannot fail.
    fn collect_module_garbage(&mut self) {
        self.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })
        .expect("a collection has no way to fail");
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
        let root = self.root_new_object(meta);
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
                Ok((specifier, module_type)) if phase == ImportPhase::Source => {
                    self.dynamic_import_source(promise, &specifier, module_type)?;
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
                    self.settle_promise(promise, PromiseStatus::Rejected(error))
                        .expect("the promise of an import() call is one of this VM's");
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
                return self
                    .with_module_records(|vm, linked| {
                        // Every request of an evaluated graph resolves.
                        if let Some(error) = Self::cycle_root_error(&entry, &modules, linked)
                            .expect("every request of the graph resolves")
                        {
                            return Err(RuntimeError::Thrown(error));
                        }
                        // The namespace already exists for an evaluated module;
                        // nothing new is rooted through this scratch list.
                        vm.module_namespace(&entry, false, &modules, linked, &mut Vec::new())
                            .map(|namespace| {
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
        self.execute_module_graph_inner(&entry, &modules, false, true, ImportPhase::Evaluation)?;
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
            class_field_initializer: std::mem::take(&mut self.class_field_initializer),
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
        self.class_field_initializer = execution.class_field_initializer;
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
        // A completion waits across an await only while a finalizer runs, and
        // that is a return, a throw or a jump: a tail call is never compiled
        // inside a try statement, and yields, resumptions and halts are
        // handled where they arise.
        for completion in &execution.pending_completions {
            if let Completion::Return(value) | Completion::Throw(RuntimeError::Thrown(value)) =
                completion
            {
                add_value(value);
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
        references.extend(execution.home_object);
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
        Ok(self.root_suspended_execution(execution))
    }

    /// Roots every heap edge of a displaced frame until the roots are handed
    /// back to [`Self::release_root`].
    fn root_suspended_execution(&mut self, execution: &SuspendedModuleExecution) -> Vec<RootId> {
        Self::suspended_execution_references(execution)
            .into_iter()
            .map(|id| self.root_new_object(id))
            .collect()
    }

    pub(super) fn run_next_job_while_module_suspended(&mut self) -> Result<bool, RuntimeError> {
        let execution = self.suspend_module_execution();
        let roots = self.root_suspended_execution(&execution);
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
        // A reaction names the continuation of the await it was made for, and
        // fires once.
        let continuation = self
            .module_continuations
            .remove(&continuation)
            .expect("an await reaction names a stored continuation");
        let ModuleContinuation {
            module,
            code,
            pc,
            execution,
            mut iterators,
            handlers,
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
        let outcome = straight_line_exit(outcome, "yield requires a generator function");
        let result = match outcome {
            Ok(StraightLineExit::Return(value)) => {
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
                self.keep_module_value_alive(&value);
                let modules = self.module_registry.clone();
                self.settle_dynamic_import_waiters(&module);
                self.settle_module_parents(module.clone(), &modules, &mut graph.linked)?;
                Ok(value)
            }
            Ok(StraightLineExit::Await {
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
                );
                Ok(Value::Undefined)
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
                self.keep_module_value_alive(&error);
                self.reject_module_and_parents(module, error, &mut graph.linked);
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
                    self.settle_dynamic_import_waiters(&parent);
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
    ) {
        let mut rejected = std::collections::VecDeque::from([module]);
        while let Some(module) = rejected.pop_front() {
            self.reject_dynamic_import_waiters(&module, error.clone());
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
                self.keep_module_value_alive(&error);
                rejected.push_back(parent);
            }
        }
    }

    /// Fulfills every `import()` promise that waited for `module` with its
    /// namespace. Every such promise was made by this VM, which is all
    /// settling one needs.
    pub(super) fn settle_dynamic_import_waiters(&mut self, module: &str) {
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
            self.settle_promise(promise, PromiseStatus::Fulfilled(Value::Object(namespace)))
                .expect("a waiting import promise is one of this VM's");
        }
        let Some(waiters) = self.module_import_waiters.remove(module) else {
            return;
        };
        let namespace = self
            .module_namespace_cache
            .get(module)
            .copied()
            .expect("a module that finished has a namespace");
        for promise in waiters {
            self.settle_promise(promise, PromiseStatus::Fulfilled(Value::Object(namespace)))
                .expect("a waiting import promise is one of this VM's");
        }
    }

    /// Rejects every `import()` promise that waited for `module` with `error`.
    pub(super) fn reject_dynamic_import_waiters(&mut self, module: &str, error: Value) {
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
            self.settle_promise(promise, PromiseStatus::Rejected(error.clone()))
                .expect("a waiting import promise is one of this VM's");
        }
        let Some(waiters) = self.module_import_waiters.remove(module) else {
            return;
        };
        for promise in waiters {
            self.settle_promise(promise, PromiseStatus::Rejected(error.clone()))
                .expect("a waiting import promise is one of this VM's");
        }
    }

    /// Registers a module frame on the Promise it awaits. A fulfilled input
    /// still goes through the job queue, preserving the required asynchronous
    /// boundary before the frame resumes.
    pub(super) fn suspend_module_await(
        &mut self,
        continuation_state: ModuleContinuation,
        promise: ObjectId,
    ) {
        let continuation = self.next_module_continuation;
        // The counter is a u64: it cannot run out.
        self.next_module_continuation = self
            .next_module_continuation
            .checked_add(1)
            .expect("continuation numbers cannot run out");
        self.module_continuations
            .insert(continuation, continuation_state);
        let status = self
            .promises
            .get(&promise)
            .expect("PromiseResolve returns a Promise this VM tracks")
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
    }

    /// Registers an ordinary async-function frame on the Promise it awaits
    /// (see [`Self::suspend_async_frame`]). Registering cannot fail; the
    /// `Result` is the shape callers in other modules already handle.
    pub(super) fn suspend_async_await(
        &mut self,
        continuation_state: AsyncContinuation,
        promise: ObjectId,
    ) -> Result<(), RuntimeError> {
        self.suspend_async_frame(continuation_state, promise);
        Ok(())
    }

    /// Registers an ordinary async-function frame on the Promise it awaits.
    /// A fulfilled input still goes through the job queue, preserving the
    /// required asynchronous boundary before the frame resumes.
    fn suspend_async_frame(&mut self, continuation_state: AsyncContinuation, promise: ObjectId) {
        let continuation = self.next_async_continuation;
        // The counter is a u64: it cannot run out.
        self.next_async_continuation = self
            .next_async_continuation
            .checked_add(1)
            .expect("continuation numbers cannot run out");
        self.async_continuations
            .insert(continuation, continuation_state);
        let status = self
            .promises
            .get(&promise)
            .expect("PromiseResolve returns a Promise this VM tracks")
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
                ambient.templates.extend(self.templates.clone());
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
                self.suspend_async_frame(state, promise);
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
        handlers: Vec<HandlerFrame>,
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
                    .set_generator_state(generator, GeneratorState::Done)?;
                ambient.templates.extend(self.templates.clone());
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
                    with_objects: std::mem::take(&mut self.with_objects),
                };
                self.heap.set_generator_state(generator, state)?;
                ambient.templates.extend(self.templates.clone());
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
                self.suspend_async_frame(state, promise);
                Ok(())
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
        // Only a module compiled as one gets here, and it has an entry.
        let entry = code
            .module_evaluate_entry
            .expect("a module has an evaluation entry") as usize;
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
        modules: &HashMap<String, Bytecode>,
        linked: &mut HashMap<String, LinkedModule>,
    ) -> Result<Value, RuntimeError> {
        // Loading the requested modules checked that every module of the graph
        // exists, and linking made a record for each.
        let record = linked
            .get(name)
            .expect("every module of the graph is linked");
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
                for target in evaluation_list {
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
            // Linking already instantiated this module's declarations.
            let entry = code
                .module_evaluate_entry
                .expect("a linked module has an evaluation entry") as usize;
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
            match straight_line_exit(outcome, "yield requires a generator function") {
                Ok(StraightLineExit::Return(value)) => {
                    self.active_module_name = previous_module;
                    let cells = std::mem::take(&mut self.cells);
                    let record = linked.get_mut(name).expect("checked module record exists");
                    record.cells = cells;
                    record.completion = Some(value.clone());
                    self.keep_module_value_alive(&value);
                    Ok(value)
                }
                Ok(StraightLineExit::Await {
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
                    );
                    Ok(Value::Undefined)
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
                self.keep_module_value_alive(&value);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn promise() -> ObjectId {
        Vm::default().heap.alloc_object(None).unwrap()
    }

    fn yield_exit() -> InterpreterExit {
        InterpreterExit::Yield {
            value: Value::Undefined,
            pc: 3,
            iterators: Vec::new(),
            handlers: Vec::new(),
        }
    }

    fn suspend_exit(pc: usize) -> InterpreterExit {
        InterpreterExit::Suspend {
            pc,
            iterators: Vec::new(),
            handlers: Vec::new(),
        }
    }

    fn await_exit() -> InterpreterExit {
        InterpreterExit::Await {
            promise: promise(),
            pc: 5,
            handlers: Vec::new(),
        }
    }

    fn straight_line(outcome: Result<InterpreterExit, RuntimeError>) -> String {
        match straight_line_exit(outcome, "no yield here") {
            Ok(StraightLineExit::Return(value)) => format!("return {value:?}"),
            Ok(StraightLineExit::Await { pc, .. }) => format!("await at {pc}"),
            Err(error) => format!("error {error:?}"),
        }
    }

    fn async_generator(outcome: Result<InterpreterExit, RuntimeError>) -> String {
        match async_generator_exit(outcome) {
            Ok(AsyncGeneratorExit::Return(value)) => format!("return {value:?}"),
            Ok(AsyncGeneratorExit::Yield { pc, .. }) => format!("yield at {pc}"),
            Ok(AsyncGeneratorExit::Await { pc, .. }) => format!("await at {pc}"),
            Err(error) => format!("error {error:?}"),
        }
    }

    #[test]
    fn code_that_is_not_a_generator_only_returns_or_awaits() {
        assert_eq!(
            straight_line(Ok(InterpreterExit::Return(Value::Number(1.0)))),
            "return Number(1.0)"
        );
        assert_eq!(straight_line(Ok(await_exit())), "await at 5");
        assert_eq!(
            straight_line(Ok(yield_exit())),
            "error TypeError(\"no yield here\")"
        );
        assert_eq!(
            straight_line(Ok(suspend_exit(0))),
            "error TypeError(\"only a generator suspends at its entry\")"
        );
        assert_eq!(
            straight_line(Err(RuntimeError::InstructionLimit)),
            "error InstructionLimit"
        );
    }

    #[test]
    fn an_async_generator_resumption_returns_yields_or_awaits() {
        assert_eq!(
            async_generator(Ok(InterpreterExit::Return(Value::Null))),
            "return Null"
        );
        assert_eq!(async_generator(Ok(yield_exit())), "yield at 3");
        assert_eq!(async_generator(Ok(await_exit())), "await at 5");
        assert_eq!(
            async_generator(Ok(suspend_exit(0))),
            "error TypeError(\"only a generator suspends at its entry\")"
        );
        assert_eq!(
            async_generator(Err(RuntimeError::InstructionLimit)),
            "error InstructionLimit"
        );
    }

    #[test]
    fn a_declaration_prefix_must_suspend_exactly_at_the_evaluation_entry() {
        assert_eq!(expect_entry_suspend(suspend_exit(7), 7), Ok(()));
        let unsupported = Err(RuntimeError::Unsupported(
            "module declaration prefix did not suspend at its evaluation entry",
        ));
        assert_eq!(expect_entry_suspend(suspend_exit(6), 7), unsupported);
        assert_eq!(
            expect_entry_suspend(InterpreterExit::Return(Value::Undefined), 7),
            unsupported
        );
    }

    fn rejected(action: CompletionAction) -> String {
        match rejected_await_step(action, 7, "no tail calls here") {
            Ok(RejectedAwait::Resume(pc)) => format!("resume at {pc}"),
            Ok(RejectedAwait::Return(value)) => format!("return {value:?}"),
            Err(error) => format!("error {error:?}"),
        }
    }

    #[test]
    fn a_rejected_await_resumes_in_its_handler_returns_or_fails() {
        assert_eq!(rejected(CompletionAction::Continue), "resume at 7");
        assert_eq!(rejected(CompletionAction::Jump(9)), "resume at 9");
        assert_eq!(
            rejected(CompletionAction::Return(Value::Number(1.0))),
            "return Number(1.0)"
        );
        for action in [
            CompletionAction::TailRecur(Vec::new()),
            CompletionAction::TailCall(Vec::new()),
        ] {
            assert_eq!(rejected(action), "error TypeError(\"no tail calls here\")");
        }
        assert_eq!(
            rejected(CompletionAction::Throw(RuntimeError::Thrown(
                Value::Number(2.0)
            ))),
            "error Thrown(Number(2.0))"
        );
    }

    /// A module the host built by hand: the requests and exports it names,
    /// and the offset of its evaluation entry (none for a broken one).
    fn hand_built(
        exports: Vec<ModuleExport>,
        requests: &[(&str, bool)],
        entry: Option<u32>,
    ) -> Bytecode {
        let mut code = Bytecode::empty();
        code.module = true;
        code.strict = true;
        code.module_evaluate_entry = entry;
        code.module_exports = exports;
        code.module_requests = requests
            .iter()
            .map(|(request, deferred)| crate::bytecode::ModuleRequest {
                module_request: (*request).to_string(),
                module_type: ModuleType::JavaScript,
                deferred: *deferred,
            })
            .collect();
        code
    }

    fn compiled_module(source: &str) -> Bytecode {
        crate::compile_module(&crate::parse_module(source).unwrap()).unwrap()
    }

    const ESCAPES: &str = "relative module request ../../x escapes its host root";

    #[test]
    fn imports_are_named_in_errors_the_way_they_are_spelled() {
        assert_eq!(
            imported_name_text(&ModuleImportName::Named("x".into())),
            "x"
        );
        assert_eq!(imported_name_text(&ModuleImportName::Namespace), "*");
        assert_eq!(
            imported_name_text(&ModuleImportName::DeferredNamespace),
            "*"
        );
        assert_eq!(imported_name_text(&ModuleImportName::Source), "source");
    }

    #[test]
    fn a_frame_carries_on_as_its_thrown_value_was_resolved() {
        let mut vm = Vm::default();
        let mut resume = |action: Result<CompletionAction, RuntimeError>| {
            straight_line(vm.interpret_after_throw(
                &Bytecode::empty(),
                &mut Vec::new(),
                4,
                Vec::new(),
                action,
                "no tails",
            ))
        };
        assert_eq!(
            resume(Ok(CompletionAction::Return(Value::Number(3.0)))),
            "return Number(3.0)"
        );
        assert_eq!(
            resume(Ok(CompletionAction::TailCall(Vec::new()))),
            "error TypeError(\"no tails\")"
        );
        assert_eq!(
            resume(Err(RuntimeError::InstructionLimit)),
            "error InstructionLimit"
        );
    }

    #[test]
    fn a_module_only_counts_as_reachable_when_every_request_on_the_way_resolves() {
        let modules = HashMap::from([
            (
                "t/a.js".to_string(),
                hand_built(
                    Vec::new(),
                    &[("./done.js", true), ("./b.js", false), ("./c.js", false)],
                    Some(0),
                ),
            ),
            (
                "t/b.js".to_string(),
                hand_built(Vec::new(), &[("../../x", false)], Some(0)),
            ),
            (
                "t/c.js".to_string(),
                hand_built(Vec::new(), &[("./a.js", false)], Some(0)),
            ),
            (
                "t/leaf.js".to_string(),
                hand_built(Vec::new(), &[], Some(0)),
            ),
            (
                "t/d.js".to_string(),
                hand_built(Vec::new(), &[("./leaf.js", false)], Some(0)),
            ),
        ]);
        let reaches =
            |from: &str, goal: &str| Vm::module_reaches(from, goal, &modules, &mut HashSet::new());
        assert_eq!(
            reaches("t/nowhere.js", "t/goal.js"),
            Err(RuntimeError::ModuleResolution(
                "module t/nowhere.js was not linked".into()
            ))
        );
        let escapes = Err(RuntimeError::ModuleResolution(ESCAPES.into()));
        assert_eq!(reaches("t/b.js", "t/goal.js"), escapes);
        assert_eq!(reaches("t/a.js", "t/goal.js"), escapes);
        // A module reaches itself, and what its requests reach; a deferred
        // request is no edge, and a module that is asked about twice while
        // one walk is under way is only walked once.
        assert_eq!(reaches("t/a.js", "t/a.js"), Ok(true));
        assert_eq!(reaches("t/d.js", "t/leaf.js"), Ok(true));
        assert_eq!(reaches("t/d.js", "t/a.js"), Ok(false));
        let mut visited = HashSet::from(["t/leaf.js".to_string()]);
        assert_eq!(
            Vm::module_reaches("t/leaf.js", "t/a.js", &modules, &mut visited),
            Ok(false)
        );
    }

    fn record(evaluating: bool, suspended: bool) -> LinkedModule {
        LinkedModule {
            cells: HashMap::new(),
            namespace: None,
            deferred_namespace: None,
            evaluated: false,
            evaluating,
            suspended,
            completion: None,
            error: None,
        }
    }

    #[test]
    fn the_cycle_root_of_a_dependency_is_the_waiting_parent_it_reaches() {
        let modules = HashMap::from([
            (
                "t/dep.js".to_string(),
                hand_built(Vec::new(), &[("./parent.js", false)], Some(0)),
            ),
            (
                "t/parent.js".to_string(),
                hand_built(Vec::new(), &[], Some(0)),
            ),
            (
                "t/broken.js".to_string(),
                hand_built(Vec::new(), &[("../../x", false)], Some(0)),
            ),
        ]);
        let mut vm = Vm::default();
        vm.module_async_parents.insert(
            "t/dep.js".to_string(),
            vec![
                "t/gone.js".to_string(),
                "t/idle.js".to_string(),
                "t/parent.js".to_string(),
            ],
        );
        vm.module_async_parents
            .insert("t/broken.js".to_string(), vec!["t/parent.js".to_string()]);
        let linked = HashMap::from([
            ("t/idle.js".to_string(), record(false, false)),
            ("t/parent.js".to_string(), record(true, false)),
        ]);
        // A parent that is unknown or idle is passed over; the one that waits
        // and is reached from the dependency is its cycle root.
        assert_eq!(
            vm.async_dependency_root("t/dep.js", &modules, &linked),
            Ok("t/parent.js".to_string())
        );
        // One that waits without being reachable leaves the dependency itself.
        assert_eq!(
            vm.async_dependency_root("t/parent.js", &modules, &linked),
            Ok("t/parent.js".to_string())
        );
        assert_eq!(
            vm.async_dependency_root("t/broken.js", &modules, &linked),
            Err(RuntimeError::ModuleResolution(ESCAPES.into()))
        );
    }

    #[test]
    fn a_rejected_await_is_caught_where_the_frame_left_off() {
        let modules = HashMap::from([(
            "t/main.js".to_string(),
            compiled_module(
                "let seen; try { await Promise.reject(7) } catch (e) { seen = e } seen === 7",
            ),
        )]);
        assert_eq!(
            Vm::default().execute_module_graph("t/main.js", &modules),
            Ok(Value::Bool(true))
        );
    }
}
