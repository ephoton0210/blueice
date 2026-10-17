// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Module export resolution (ResolveExport, GetExportedNames) and the
//! construction of Module Namespace Exotic Objects.

use super::*;

impl Vm {
    pub(in super::super) fn resolve_export(
        modules: &HashMap<String, Bytecode>,
        module: &str,
        export_name: &str,
        resolve_set: &mut Vec<(String, String)>,
    ) -> Result<ExportResolution, RuntimeError> {
        let pair = (module.to_string(), export_name.to_string());
        if resolve_set.contains(&pair) {
            return Ok(ExportResolution::Missing);
        }
        resolve_set.push(pair);
        let result = (|| {
            let code = modules.get(module).ok_or_else(|| {
                RuntimeError::ModuleResolution(format!(
                    "module {module} was not supplied by the host"
                ))
            })?;
            for export in &code.module_exports {
                match export {
                    ModuleExport::Local {
                        export_name: name,
                        local_slot,
                    } if name == export_name => {
                        return Ok(ExportResolution::Binding {
                            module: module.to_string(),
                            slot: *local_slot as usize,
                        });
                    }
                    ModuleExport::Indirect {
                        export_name: name,
                        module_request,
                        import_name,
                        module_type,
                    } if name == export_name => {
                        let target =
                            Self::resolve_module_target(module, module_request, *module_type)?;
                        return Self::resolve_export(modules, &target, import_name, resolve_set);
                    }
                    ModuleExport::Namespace {
                        export_name: name,
                        module_request,
                        module_type,
                    } if name == export_name => {
                        return Ok(ExportResolution::Namespace {
                            module: Self::resolve_module_target(
                                module,
                                module_request,
                                *module_type,
                            )?,
                        });
                    }
                    ModuleExport::DeferredNamespace {
                        export_name: name,
                        module_request,
                        module_type,
                    } if name == export_name => {
                        return Ok(ExportResolution::DeferredNamespace {
                            module: Self::resolve_module_target(
                                module,
                                module_request,
                                *module_type,
                            )?,
                        });
                    }
                    ModuleExport::Source {
                        export_name: name,
                        module_request,
                    } if name == export_name => {
                        return Ok(ExportResolution::Source {
                            module: Self::resolve_module_request(module, module_request)?,
                        });
                    }
                    _ => {}
                }
            }
            if export_name == "default" {
                return Ok(ExportResolution::Missing);
            }
            let mut candidate = ExportResolution::Missing;
            for export in &code.module_exports {
                let ModuleExport::Star {
                    module_request,
                    module_type,
                } = export
                else {
                    continue;
                };
                let target = Self::resolve_module_target(module, module_request, *module_type)?;
                match Self::resolve_export(modules, &target, export_name, resolve_set)? {
                    ExportResolution::Missing => {}
                    ExportResolution::Ambiguous => return Ok(ExportResolution::Ambiguous),
                    found @ (ExportResolution::Binding { .. }
                    | ExportResolution::Namespace { .. }
                    | ExportResolution::DeferredNamespace { .. }
                    | ExportResolution::Source { .. }) => {
                        if candidate == ExportResolution::Missing {
                            candidate = found;
                        } else if candidate != found {
                            return Ok(ExportResolution::Ambiguous);
                        }
                    }
                }
            }
            Ok(candidate)
        })();
        resolve_set.pop();
        result
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
        let cache = if deferred {
            &self.module_deferred_namespace_cache
        } else {
            &self.module_namespace_cache
        };
        if let Some(namespace) = cache.get(module) {
            return Ok(*namespace);
        }
        let record = linked.get(module).ok_or_else(|| {
            RuntimeError::ModuleResolution(format!("module {module} was not linked"))
        })?;
        if let Some(namespace) = if deferred {
            record.deferred_namespace
        } else {
            record.namespace
        } {
            return Ok(namespace);
        }
        // Cycles need an early placeholder, but a failed allocation must
        // not publish that incomplete namespace to a later importer. One
        // outer transaction owns every recursively published placeholder.
        self.synchronize_scheduling_usage()?;
        let prepared =
            namespace_cache_storage::PreparedNamespaceEntry::prepare(self, module, deferred)?;
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
            let mut names = Self::exported_names(modules, module, self.host_scheduling.as_ref())?;
            if deferred {
                names.exclude("then");
            }
            // Publish an identity-stable placeholder before resolving namespace
            // exports. A module may re-export its own namespace, directly or
            // through a cycle; waiting until after recursive resolution would
            // recurse indefinitely and overflow the host stack.
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
            let mut exports = Vec::with_capacity(names.len());
            for &name in names.iter() {
                let resolution = Self::resolve_export(modules, module, name, &mut Vec::new())?;
                let deferred_target =
                    matches!(resolution, ExportResolution::DeferredNamespace { .. });
                let cell = match resolution {
                    ExportResolution::Binding {
                        module: exporter,
                        slot,
                    } => {
                        let cell = linked
                            .get(&exporter)
                            .and_then(|record| record.cells.get(&slot))
                            .copied()
                            .ok_or_else(|| {
                                RuntimeError::ModuleResolution(format!(
                                    "export {name} from {exporter} has no binding"
                                ))
                            })?;
                        cell
                    }
                    ExportResolution::Namespace { module }
                    | ExportResolution::DeferredNamespace { module } => {
                        // Namespace exports still need a binding cell: namespace
                        // exotic properties are live bindings uniformly, and this
                        // one is an immutable binding to the target namespace.
                        let value = Value::Object(self.module_namespace(
                            &module,
                            deferred_target,
                            modules,
                            linked,
                            roots,
                        )?);
                        let root_slot = roots.prepare_slot(self.host_scheduling.as_ref())?;
                        let cell = self.with_roots(|heap| heap.alloc_object(None))?;
                        root_slot.root_into(roots, &mut self.heap, cell)?;
                        self.with_roots(|heap| heap.set(cell, "value", value))?;
                        cell
                    }
                    ExportResolution::Source { module } => {
                        let value = Value::Object(self.module_source_object(&module, modules)?);
                        let root_slot = roots.prepare_slot(self.host_scheduling.as_ref())?;
                        let cell = self.with_roots(|heap| heap.alloc_object(None))?;
                        root_slot.root_into(roots, &mut self.heap, cell)?;
                        self.with_roots(|heap| heap.set(cell, "value", value))?;
                        cell
                    }
                    ExportResolution::Missing | ExportResolution::Ambiguous => continue,
                };
                exports.push((name.into(), cell));
            }
            self.with_roots(|heap| heap.initialize_module_namespace(namespace, exports))?;
            Ok(namespace)
        })();
        self.namespace_transaction_depth -= 1;
        if outer_transaction && outcome.is_err() {
            self.rollback_namespace_cache_from(namespace_checkpoint, linked);
            self.release_module_roots_from(roots, roots_checkpoint);
        }
        self.reconcile_scheduling_ownership();
        outcome
    }
}
