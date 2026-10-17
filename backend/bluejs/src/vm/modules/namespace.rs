// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Module export resolution (ResolveExport, GetExportedNames) and the
//! construction of Module Namespace Exotic Objects.

use super::*;

#[cfg(any(test, coverage))]
#[path = "../../../tests/fixtures/namespace_build_stack.rs"]
mod stack_tests;

use crate::vm::module_export_resolution::ResolvedModule;
use crate::vm::module_walk_storage::Storage;

pub(super) struct NamespaceFrame<'a> {
    module: &'a str,
    namespace: ObjectId,
    names: module_export_names::ExportNames<'a>,
    exports: module_binding_storage::NamespaceExports,
    next: usize,
    waiting: Option<ObjectId>,
}
enum NamespaceStep<'a> {
    Finish,
    Cell(ObjectId),
    Target(ResolvedModule<'a>, bool),
    Source(ResolvedModule<'a>),
    Value(ObjectId),
    Skip,
}

impl Vm {
    pub(in super::super) fn resolve_export<'a>(
        modules: &'a HashMap<String, Bytecode>,
        module: &str,
        export_name: &str,
        scope: Option<&SchedulingScope>,
    ) -> Result<ExportResolution<'a>, RuntimeError> {
        module_export_resolution::resolve(modules, module, export_name, scope)
    }

    pub(in super::super) fn exported_names<'a>(
        modules: &'a HashMap<String, Bytecode>,
        module: &str,
        scope: Option<&SchedulingScope>,
    ) -> Result<module_export_names::ExportNames<'a>, RuntimeError> {
        module_export_names::ExportNames::collect(modules, module, scope)
    }

    /// GetModuleNamespace(module, phase): the module's namespace object, made
    /// on first request. `deferred` selects the import-defer proposal's
    /// separate *deferred* namespace, which leaves out the export name
    /// `"then"` so that awaiting it cannot evaluate the module.
    pub(in super::super) fn module_namespace(
        &mut self,
        module: &str,
        deferred: bool,
        modules: &HashMap<String, Bytecode>,
        linked: &mut HashMap<String, LinkedModule>,
        roots: &mut ModuleRoots,
    ) -> Result<ObjectId, RuntimeError> {
        #[cfg(test)]
        let _call = stack_tests::CallProbe::enter();
        if let Some(namespace) = self.cached_module_namespace(module, deferred, linked)? {
            return Ok(namespace);
        }
        self.synchronize_scheduling_usage()?;
        // Invalid publication epochs refuse before even the controller buffer
        // allocates; cache hits above preserve their existing fast path.
        self.next_namespace_epoch.checked_add(1).ok_or_else(|| {
            self.host_scheduling
                .as_ref()
                .map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail)
        })?;
        let roots_checkpoint = roots.len();
        let namespace_checkpoint = self.next_namespace_epoch;
        let outer_transaction = self.namespace_transaction_depth == 0;
        self.namespace_transaction_depth = self
            .namespace_transaction_depth
            .checked_add(1)
            .ok_or_else(|| {
                self.host_scheduling
                    .as_ref()
                    .map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail)
            })?;
        let outcome = (|| {
            let mut frames = Storage::default();
            frames.reserve(1, self.host_scheduling.as_ref())?;
            let root = self.prepare_namespace_frame(module, deferred, modules, linked, roots)?;
            let namespace = root.namespace;
            frames.data.push(root);
            loop {
                let step = {
                    let frame = frames.data.last_mut().expect("namespace work has a frame");
                    if let Some(namespace) = frame.waiting.take() {
                        NamespaceStep::Value(namespace)
                    } else if let Some(&name) = frame.names.get(frame.next) {
                        frame.next += 1;
                        match Self::resolve_export(
                            modules,
                            frame.module,
                            name,
                            self.host_scheduling.as_ref(),
                        )? {
                            ExportResolution::Binding {
                                module: exporter,
                                slot,
                            } => {
                                let cell = linked
                                    .get(exporter.as_str())
                                    .and_then(|record| record.cells.get(&slot))
                                    .copied()
                                    .ok_or_else(|| {
                                        RuntimeError::ModuleResolution(format!(
                                            "export {name} from {exporter} has no binding"
                                        ))
                                    })?;
                                NamespaceStep::Cell(cell)
                            }
                            ExportResolution::Namespace { module } => {
                                NamespaceStep::Target(module, false)
                            }
                            ExportResolution::DeferredNamespace { module } => {
                                NamespaceStep::Target(module, true)
                            }
                            ExportResolution::Source { module } => NamespaceStep::Source(module),
                            ExportResolution::Missing | ExportResolution::Ambiguous => {
                                NamespaceStep::Skip
                            }
                        }
                    } else {
                        NamespaceStep::Finish
                    }
                };
                let cell = match step {
                    NamespaceStep::Finish => {
                        let frame = frames.data.pop().unwrap();
                        frame.exports.publish(self, frame.namespace)?;
                        if frames.data.is_empty() {
                            return Ok(namespace);
                        }
                        continue;
                    }
                    NamespaceStep::Cell(cell) => cell,
                    NamespaceStep::Value(namespace) => {
                        self.namespace_value_cell(namespace, roots)?
                    }
                    NamespaceStep::Source(module) => {
                        let source = self.module_source_object(&module, modules)?;
                        self.namespace_value_cell(source, roots)?
                    }
                    NamespaceStep::Target(module, deferred) => {
                        if let Some(namespace) =
                            self.cached_module_namespace(&module, deferred, linked)?
                        {
                            self.namespace_value_cell(namespace, roots)?
                        } else {
                            // Reserve the complete coexisting frame backing before
                            // a child can publish its placeholder. Frame payloads
                            // own their buffers independently of this movable Vec.
                            frames.reserve(1, self.host_scheduling.as_ref())?;
                            let child = self.prepare_namespace_frame(
                                &module, deferred, modules, linked, roots,
                            )?;
                            frames.data.last_mut().unwrap().waiting = Some(child.namespace);
                            frames.data.push(child);
                            continue;
                        }
                    }
                    NamespaceStep::Skip => {
                        frames.data.last_mut().unwrap().exports.skip_next();
                        continue;
                    }
                };
                frames.data.last_mut().unwrap().exports.keep_next(cell);
            }
        })();
        self.namespace_transaction_depth -= 1;
        if outer_transaction && outcome.is_err() {
            self.rollback_namespace_cache_from(namespace_checkpoint, linked);
            self.release_module_roots_from(roots, roots_checkpoint);
        }
        self.reconcile_scheduling_ownership();
        outcome
    }

    fn cached_module_namespace(
        &self,
        module: &str,
        deferred: bool,
        linked: &HashMap<String, LinkedModule>,
    ) -> Result<Option<ObjectId>, RuntimeError> {
        let cache = if deferred {
            &self.module_deferred_namespace_cache
        } else {
            &self.module_namespace_cache
        };
        if let Some(namespace) = cache.get(module) {
            return Ok(Some(*namespace));
        }
        let record = linked.get(module).ok_or_else(|| {
            RuntimeError::ModuleResolution(format!("module {module} was not linked"))
        })?;
        Ok(if deferred {
            record.deferred_namespace
        } else {
            record.namespace
        })
    }

    fn prepare_namespace_frame<'a>(
        &mut self,
        module: &str,
        deferred: bool,
        modules: &'a HashMap<String, Bytecode>,
        linked: &mut HashMap<String, LinkedModule>,
        roots: &mut ModuleRoots,
    ) -> Result<NamespaceFrame<'a>, RuntimeError> {
        self.synchronize_scheduling_usage()?;
        let prepared =
            namespace_cache_storage::PreparedNamespaceEntry::prepare(self, module, deferred)?;
        let mut names = Self::exported_names(modules, module, self.host_scheduling.as_ref())?;
        if deferred {
            names.exclude("then");
        }
        let exports = module_binding_storage::NamespaceExports::prepare(
            &names,
            self.host_scheduling.as_ref(),
        )?;
        let saved_capacity = self.preserve_promise_table_capacity;
        self.preserve_promise_table_capacity = true;
        let placeholder = (|| {
            let root_slot = roots.prepare_slot(self.host_scheduling.as_ref())?;
            let namespace =
                self.with_roots(|heap| heap.alloc_module_namespace(Vec::new(), deferred))?;
            root_slot.root_into(roots, &mut self.heap, namespace)?;
            let cache_root = self.heap.root(namespace)?;
            prepared.publish(self, module, linked, namespace, cache_root);
            Ok::<_, RuntimeError>(namespace)
        })();
        self.preserve_promise_table_capacity = saved_capacity;
        let namespace = placeholder?;
        Ok(NamespaceFrame {
            module: modules
                .get_key_value(module)
                .expect("exported names require a supplied module")
                .0,
            namespace,
            names,
            exports,
            next: 0,
            waiting: None,
        })
    }

    fn namespace_value_cell(
        &mut self,
        namespace: ObjectId,
        roots: &mut ModuleRoots,
    ) -> Result<ObjectId, RuntimeError> {
        let root_slot = roots.prepare_slot(self.host_scheduling.as_ref())?;
        let cell = self.with_roots(|heap| heap.alloc_object(None))?;
        root_slot.root_into(roots, &mut self.heap, cell)?;
        self.with_roots(|heap| heap.set(cell, "value", Value::Object(namespace)))?;
        Ok(cell)
    }
}
