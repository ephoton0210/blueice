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

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthesizes `key` into a fresh module map, returning the map and the
    /// number of roots the call registered.
    fn synthesize(
        vm: &mut Vm,
        key: &str,
    ) -> (Result<(), RuntimeError>, HashMap<String, Bytecode>, usize) {
        let mut modules = HashMap::new();
        let mut roots = Vec::new();
        // The module loader gives the synthesis a budget before calling it.
        vm.remaining_instructions = vm.config.instruction_budget;
        let (_, module_type) = ModuleType::split_module_key(key);
        let synthetic_type = SyntheticModuleType::from_module_type(module_type)
            .expect("test keys always name a synthetic module type");
        let result = vm.ensure_synthetic_module(key, synthetic_type, &mut modules, &mut roots);
        (result, modules, roots.len())
    }

    fn vm_with_sources() -> Vm {
        let mut vm = Vm::default();
        vm.set_json_module_sources(HashMap::from([
            ("t/object.json".to_string(), "{\"a\": 1}".to_string()),
            ("t/number.json".to_string(), "5".to_string()),
            ("t/broken.json".to_string(), "{".to_string()),
        ]));
        vm.set_text_module_sources(HashMap::from([("t/x.txt".to_string(), "abc".to_string())]));
        vm.set_bytes_module_sources(HashMap::from([("t/x.bin".to_string(), vec![1, 2, 3])]));
        vm
    }

    #[test]
    fn a_source_text_module_is_never_synthesized() {
        // `SyntheticModuleType` has no Source Text Module variant, so this
        // is checked before a `ModuleType::JavaScript` key can ever reach
        // `ensure_synthetic_module`.
        assert!(SyntheticModuleType::from_module_type(ModuleType::JavaScript).is_none());
    }

    #[test]
    fn json_text_and_bytes_modules_get_one_default_export() {
        let mut vm = vm_with_sources();
        // An object value is rooted until a cell holds it; a primitive is not.
        for (path, module_type, roots) in [
            ("t/object.json", ModuleType::Json, 1),
            ("t/number.json", ModuleType::Json, 0),
            ("t/x.txt", ModuleType::Text, 0),
            ("t/x.bin", ModuleType::Bytes, 1),
        ] {
            let key = module_type.module_key(path);
            let (result, modules, rooted) = synthesize(&mut vm, &key);
            assert_eq!(result, Ok(()), "{key:?}");
            assert_eq!(rooted, roots, "{key:?}");
            let code = &modules[&key];
            assert_eq!(code.module_exports.len(), 1, "{key:?}");
            assert!(code.synthetic_default_export.is_some(), "{key:?}");
        }
    }

    #[test]
    fn a_module_that_already_exists_is_not_synthesized_again() {
        let mut vm = vm_with_sources();
        let key = ModuleType::Text.module_key("t/x.txt");
        let mut modules = HashMap::from([(key.clone(), Bytecode::empty())]);
        let mut roots = Vec::new();
        assert_eq!(
            vm.ensure_synthetic_module(&key, SyntheticModuleType::Text, &mut modules, &mut roots),
            Ok(())
        );
        assert!(modules[&key].synthetic_default_export.is_none());
    }

    #[test]
    fn a_resource_the_host_did_not_provide_is_a_type_error() {
        let mut vm = vm_with_sources();
        for (module_type, kind) in [
            (ModuleType::Json, "JSON"),
            (ModuleType::Text, "text"),
            (ModuleType::Bytes, "bytes"),
        ] {
            let (result, modules, _) = synthesize(&mut vm, &module_type.module_key("t/absent"));
            assert_eq!(
                result,
                Err(RuntimeError::TypeError(format!(
                    "host did not provide a {kind} module source for t/absent"
                )))
            );
            assert!(modules.is_empty());
        }
    }

    #[test]
    fn malformed_json_is_a_module_resolution_failure() {
        let mut vm = vm_with_sources();
        let (result, modules, _) =
            synthesize(&mut vm, &ModuleType::Json.module_key("t/broken.json"));
        assert_eq!(
            result,
            Err(RuntimeError::ModuleResolution(
                "invalid JSON module t/broken.json: SyntaxError: invalid JSON text".into()
            ))
        );
        assert!(modules.is_empty());
    }

    #[test]
    fn a_bytes_resource_over_the_buffer_limit_is_a_range_error() {
        let mut vm = Vm::new(VmConfig {
            heap: HeapConfig {
                nursery_capacity: 256,
                major_threshold_bytes: 262_144,
                max_heap_bytes: 262_144,
            },
            ..VmConfig::default()
        })
        .unwrap();
        vm.set_bytes_module_sources(HashMap::from([("t/big.bin".to_string(), vec![0; 300_000])]));
        let (result, modules, _) = synthesize(&mut vm, &ModuleType::Bytes.module_key("t/big.bin"));
        assert_eq!(
            result,
            Err(RuntimeError::RangeError(
                "immutable ArrayBuffer length is too large".into()
            ))
        );
        assert!(modules.is_empty());
    }
}
