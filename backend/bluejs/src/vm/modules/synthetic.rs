// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Synthetic Module Records: the modules a `with { type }` import attribute
//! selects instead of a Source Text Module -- JSON modules, text modules
//! (tc39/proposal-import-text) and bytes modules (tc39/proposal-import-bytes).
//! Each has one `default` export and no code to run.

use super::*;

/// A module whose record is synthesized from host data rather than parsed as
/// JavaScript. Keeping this separate from `ModuleType` makes the source-text
/// case impossible inside `ensure_synthetic_module`.
#[derive(Clone, Copy)]
pub(super) enum SyntheticModuleType {
    Json,
    Text,
    Bytes,
}

impl SyntheticModuleType {
    pub(super) fn from_module_type(module_type: ModuleType) -> Option<Self> {
        match module_type {
            ModuleType::JavaScript => None,
            ModuleType::Json => Some(Self::Json),
            ModuleType::Text => Some(Self::Text),
            ModuleType::Bytes => Some(Self::Bytes),
        }
    }
}

impl Vm {
    /// The synthetic-module half of HostLoadImportedModule: builds the Module
    /// Record for a request whose `with { type }` attribute names a synthetic
    /// module, from the host's raw resource data. Each one is a Synthetic
    /// Module Record whose sole export is an already-initialized, immutable
    /// `default` binding (CreateDefaultExportSyntheticModule):
    ///
    /// * `json` -- ParseJSONModule (Import Attributes proposal §1.4/§1.5,
    ///   merged into the published edition): `? Call(%JSON.parse%,
    ///   undefined, «source»)`, whose abrupt completion (malformed JSON) is a
    ///   module *resolution* failure, not an ordinary runtime SyntaxError.
    /// * `text` -- CreateTextModule (tc39/proposal-import-text): the
    ///   resource decoded as UTF-8 into a String.
    /// * `bytes` -- CreateBytesModule (tc39/proposal-import-bytes): a
    ///   `Uint8Array` over an immutable ArrayBuffer holding the bytes.
    ///
    /// Idempotent per module key: a later request for the same resource and
    /// type reuses the same synthesized `Bytecode` (and, once linked, the
    /// same cell/value), which is what gives repeated imports of one
    /// resource the required object identity
    /// (`language/import/import-attributes/json-idempotency.js`).
    ///
    /// `key` must already be the *resolved* module key
    /// ([`ModuleType::module_key`]). `roots` roots a freshly built object
    /// value immediately: it is not yet reachable from any GC root until a
    /// later step attaches it to a module's cell, and this function may run
    /// before that step's own major collection.
    pub(super) fn ensure_synthetic_module(
        &mut self,
        key: &str,
        module_type: SyntheticModuleType,
        modules: &mut HashMap<String, Bytecode>,
        roots: &mut Vec<RootId>,
    ) -> Result<(), RuntimeError> {
        if modules.contains_key(key) {
            return Ok(());
        }
        let (path, _) = ModuleType::split_module_key(key);
        let value = match module_type {
            SyntheticModuleType::Json => {
                let Some(source) = self.json_module_sources.get(path).cloned() else {
                    return Err(RuntimeError::TypeError(format!(
                        "host did not provide a JSON module source for {path}"
                    )));
                };
                self.json_parse(&Value::String(source.into()), None)
                    .map_err(|error| {
                        RuntimeError::ModuleResolution(format!(
                            "invalid JSON module {path}: {error}"
                        ))
                    })?
            }
            SyntheticModuleType::Text => {
                let Some(source) = self.text_module_sources.get(path).cloned() else {
                    return Err(RuntimeError::TypeError(format!(
                        "host did not provide a text module source for {path}"
                    )));
                };
                Value::String(source.into())
            }
            SyntheticModuleType::Bytes => {
                let Some(bytes) = self.bytes_module_sources.get(path).cloned() else {
                    return Err(RuntimeError::TypeError(format!(
                        "host did not provide a bytes module source for {path}"
                    )));
                };
                self.create_bytes_module_value(bytes)?
            }
        };
        if let Value::Object(id) = value {
            // A JSON parse or bytes-module allocation just returned this
            // object; neither caller nor this branch allocates before rooting.
            roots.push(
                self.heap
                    .root(id)
                    .expect("a newly synthesized module value is live"),
            );
        }
        let mut code = Bytecode::empty();
        code.module = true;
        code.strict = true;
        code.bindings.push(Binding {
            name: "default".to_string(),
            mutable: false,
            strict_immutable: true,
            lexical: false,
            catch_parameter: false,
            eval_var: false,
        });
        code.scopes.push(vec![0]);
        // No function-declaration prefix exists to hoist, so the whole
        // (empty) instruction stream is the "evaluate" phase; nothing ever
        // actually runs it, since the module is linked pre-`evaluated`
        // below (see `execute_module_graph_inner`'s per-`order` pass and its
        // single-entry counterpart above).
        code.module_evaluate_entry = Some(0);
        code.module_exports.push(ModuleExport::Local {
            export_name: "default".to_string(),
            local_slot: 0,
        });
        code.synthetic_default_export = Some(value);
        modules.insert(key.to_string(), code);
        Ok(())
    }

    /// Scans a fresh static module graph's own requests for a `with`
    /// attribute of `type: "json" | "text" | "bytes"` (both `import`/`export
    /// ... from` and the import side of a re-export) and registers each
    /// resolved target via [`Self::ensure_synthetic_module`]. A synthetic
    /// module never itself requests further modules, so one pass over the
    /// initially supplied set is exhaustive; no further transitive discovery
    /// is needed.
    pub(super) fn register_static_synthetic_modules(
        &mut self,
        modules: &mut HashMap<String, Bytecode>,
        roots: &mut Vec<RootId>,
    ) -> Result<(), RuntimeError> {
        let mut targets = Vec::new();
        // `modules` is host-supplied, so its HashMap iteration order must not
        // choose which resolution error a cyclic graph reports first. Keep
        // each module's request order below (that is source order), while
        // visiting independent module records in canonical key order.
        let mut names: Vec<_> = modules.keys().collect();
        names.sort_unstable();
        for name in names {
            let code = modules
                .get(name)
                .expect("module key was collected from this map");
            for import in &code.module_imports {
                if let Some(synthetic_type) =
                    SyntheticModuleType::from_module_type(import.module_type)
                {
                    if !matches!(import.import_name, ModuleImportName::Source) {
                        targets.push((
                            Self::resolve_module_target(
                                name,
                                &import.module_request,
                                import.module_type,
                            )?,
                            synthetic_type,
                        ));
                    }
                }
            }
            for export in &code.module_exports {
                let (module_request, module_type) = match export {
                    ModuleExport::Indirect {
                        module_request,
                        module_type,
                        ..
                    }
                    | ModuleExport::Star {
                        module_request,
                        module_type,
                    }
                    | ModuleExport::Namespace {
                        module_request,
                        module_type,
                        ..
                    }
                    | ModuleExport::DeferredNamespace {
                        module_request,
                        module_type,
                        ..
                    } => (module_request, *module_type),
                    ModuleExport::Local { .. } | ModuleExport::Source { .. } => continue,
                };
                if let Some(synthetic_type) = SyntheticModuleType::from_module_type(module_type) {
                    targets.push((
                        Self::resolve_module_target(name, module_request, module_type)?,
                        synthetic_type,
                    ));
                }
            }
        }
        for (target, synthetic_type) in targets {
            self.ensure_synthetic_module(&target, synthetic_type, modules, roots)?;
        }
        Ok(())
    }
}
