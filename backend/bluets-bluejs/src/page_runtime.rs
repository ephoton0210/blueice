// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Direct BlueTS classic-script admission into a BlueJS page realm.
//!
//! This is a host-neutral seam, not a page loader: callers still authorize the
//! tab, origin, source graph, host-typing profile, and script kind. It ensures
//! that a direct artifact uses the page runtime's ownership/accounting rather
//! than creating an unrelated program registry, then attaches only to the
//! exact just-installed generation.

use super::*;
use std::collections::BTreeMap;

/// Language-side owner for BlueJS page realms and their static BlueTS debug
/// records. A host still supplies every tab, origin, source graph, and script
/// kind, but navigation and close always prune metadata for invalidated
/// generations together with the realm.
pub struct DirectPageRealmOwner {
    runtime: bluejs::BlueJsPageRuntime,
    debug_registry: DirectDebugRegistry,
}

impl Default for DirectPageRealmOwner {
    fn default() -> Self {
        Self::new(
            bluejs::BlueJsPageRuntimeConfig::default(),
            DirectDebugRetentionLimits::default(),
        )
        .expect("the default BlueTS page realm owner configuration is valid")
    }
}

impl DirectPageRealmOwner {
    /// Creates an empty owner with explicit page-runtime and static-debug
    /// limits. It exposes no DOM, IPC, network, or capability bindings.
    pub fn new(
        runtime_config: bluejs::BlueJsPageRuntimeConfig,
        debug_limits: DirectDebugRetentionLimits,
    ) -> Result<Self, BridgeError> {
        Ok(Self {
            runtime: bluejs::BlueJsPageRuntime::new(runtime_config)
                .map_err(BridgeError::PageRuntime)?,
            debug_registry: DirectDebugRegistry::new(debug_limits),
        })
    }

    /// Opens one caller-authorized page realm.
    pub fn open_realm(
        &mut self,
        tab_id: u64,
        origin: bluejs::BlueJsPageOrigin,
    ) -> Result<(), BridgeError> {
        self.runtime
            .open_realm(tab_id, origin)
            .map_err(BridgeError::PageRuntime)
    }

    /// Replaces one realm after a caller-authorized navigation or reload and
    /// immediately prunes every invalidated static metadata record.
    pub fn navigate(
        &mut self,
        tab_id: u64,
        origin: bluejs::BlueJsPageOrigin,
    ) -> Result<(), BridgeError> {
        self.runtime
            .navigate(tab_id, origin)
            .map_err(BridgeError::PageRuntime)?;
        self.debug_registry
            .prune_invalid(self.runtime.program_registry());
        Ok(())
    }

    /// Closes a caller-selected realm and prunes static metadata for the
    /// invalidated generations. Returns false only when the realm was absent.
    pub fn close_realm(&mut self, tab_id: u64) -> bool {
        let closed = self.runtime.close_realm(tab_id);
        self.debug_registry
            .prune_invalid(self.runtime.program_registry());
        closed
    }

    /// Installs caller-owned callbacks through BlueJS's restricted page-realm
    /// registrar. The callback receives no VM, heap, source, or runtime object
    /// access and therefore cannot bypass this owner's program admission.
    pub fn configure_realm_bindings(
        &mut self,
        tab_id: u64,
        configure: impl FnOnce(
            &mut bluejs::BlueJsHostBindingRegistrar<'_>,
        ) -> Result<(), bluejs::RuntimeError>,
    ) -> Result<(), BridgeError> {
        self.runtime
            .configure_realm_bindings(tab_id, configure)
            .map_err(BridgeError::PageRuntime)
    }

    /// Admits one checked classic script and its static metadata together.
    pub fn attach_script(
        &mut self,
        script: &DirectScript,
        tab_id: u64,
        origin: &bluejs::BlueJsPageOrigin,
    ) -> Result<DirectProgramAttachment, BridgeError> {
        script.attach_debug_in_page_realm(
            &mut self.runtime,
            tab_id,
            origin,
            &mut self.debug_registry,
        )
    }

    /// Admits every checked ESM graph module and its static metadata together.
    pub fn attach_module_graph(
        &mut self,
        graph: &DirectModuleGraph,
        tab_id: u64,
        origin: &bluejs::BlueJsPageOrigin,
    ) -> Result<DirectPageModuleGraphAttachment, BridgeError> {
        graph.attach_debug_in_page_realm(
            &mut self.runtime,
            tab_id,
            origin,
            &mut self.debug_registry,
        )
    }

    /// Executes one generation that was admitted by this owner.
    pub fn execute_program(
        &mut self,
        tab_id: u64,
        attachment: &DirectProgramAttachment,
    ) -> Result<bluejs::Value, BridgeError> {
        self.runtime
            .execute_program(tab_id, attachment.handle)
            .map_err(BridgeError::PageRuntime)
    }

    /// Executes a graph that was admitted by this owner.
    pub fn execute_module_graph(
        &mut self,
        tab_id: u64,
        attachment: &DirectPageModuleGraphAttachment,
    ) -> Result<bluejs::Value, BridgeError> {
        attachment.execute_in_page_realm(&mut self.runtime, tab_id)
    }

    /// Returns static metadata only for an exact, currently live generation.
    pub fn debug_metadata(
        &self,
        handle: bluejs::BlueJsProgramHandle,
    ) -> Result<&RetainedDirectDebugInfo, DirectDebugAttachmentError> {
        self.debug_registry
            .get(self.runtime.program_registry(), handle)
    }

    /// Number of static metadata records whose generations remain live.
    pub fn debug_record_count(&self) -> usize {
        self.debug_registry.len()
    }

    /// Returns bounded resource accounting for one live realm.
    pub fn realm_stats(&self, tab_id: u64) -> Result<bluejs::BlueJsPageRealmStats, BridgeError> {
        self.runtime
            .realm_stats(tab_id)
            .map_err(BridgeError::PageRuntime)
    }
}

/// One direct TypeScript module graph attached to exact live programs in a
/// single page realm. These are compilation/provenance records, not runtime
/// module namespaces or debugger scopes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectPageModuleGraphAttachment {
    /// The attached generation selected as the ESM graph entry.
    pub entry: DirectProgramAttachment,
    /// Every canonical runtime module attached in deterministic ID order.
    pub modules: BTreeMap<String, DirectProgramAttachment>,
}

impl DirectPageModuleGraphAttachment {
    /// Executes the fully attached graph in its owning page realm. BlueJS uses
    /// the retained canonical module IDs and does not re-resolve TypeScript
    /// import specifiers at execution time.
    pub fn execute_in_page_realm(
        &self,
        runtime: &mut bluejs::BlueJsPageRuntime,
        tab_id: u64,
    ) -> Result<bluejs::Value, BridgeError> {
        runtime
            .execute_module_graph(
                tab_id,
                self.entry.handle,
                self.modules.values().map(|attachment| attachment.handle),
            )
            .map_err(BridgeError::PageRuntime)
    }
}

impl DirectScript {
    /// Installs this checked classic TypeScript artifact in one already-open
    /// BlueJS page realm and attaches its verified lowering provenance to the
    /// resulting generation. An attachment failure discards the program before
    /// it can be executed, releasing the realm's program/bytecode charge.
    pub fn attach_in_page_realm(
        &self,
        runtime: &mut bluejs::BlueJsPageRuntime,
        tab_id: u64,
        origin: &bluejs::BlueJsPageOrigin,
    ) -> Result<DirectProgramAttachment, BridgeError> {
        let source = source_identity(&self.sources)?;
        let handle = runtime
            .install_program(tab_id, origin, source, &self.program)
            .map_err(BridgeError::PageRuntime)?;
        match self.attach_existing_in(runtime.program_registry(), handle) {
            Ok(attachment) => Ok(attachment),
            Err(error) => {
                runtime
                    .discard_program(tab_id, handle)
                    .map_err(BridgeError::PageRuntime)?;
                Err(error)
            }
        }
    }

    /// Like [`Self::attach_in_page_realm`], additionally retaining static
    /// BlueTS metadata for that exact live generation. The metadata registry
    /// stores no runtime values. Any failed retention discards the just-created
    /// page-realm program so it cannot run without the requested pairing.
    pub fn attach_debug_in_page_realm(
        &self,
        runtime: &mut bluejs::BlueJsPageRuntime,
        tab_id: u64,
        origin: &bluejs::BlueJsPageOrigin,
        debug_registry: &mut DirectDebugRegistry,
    ) -> Result<DirectProgramAttachment, BridgeError> {
        let attachment = self.attach_in_page_realm(runtime, tab_id, origin)?;
        if let Err(error) = debug_registry.retain(
            runtime.program_registry(),
            &attachment,
            &self.language_version,
            &self.compiler_options_fingerprint,
            &self.sources,
            &self.debug_info,
        ) {
            runtime
                .discard_program(tab_id, attachment.handle)
                .map_err(BridgeError::PageRuntime)?;
            return Err(BridgeError::DebugAttachment(error));
        }
        Ok(attachment)
    }
}

impl DirectModuleGraph {
    /// Admits every direct ESM module into one already-open page realm, then
    /// attaches each module's provenance to its exact live generation. Any
    /// source, bytecode, or provenance failure discards every program already
    /// admitted for this graph before returning an error.
    pub fn attach_in_page_realm(
        &self,
        runtime: &mut bluejs::BlueJsPageRuntime,
        tab_id: u64,
        origin: &bluejs::BlueJsPageOrigin,
    ) -> Result<DirectPageModuleGraphAttachment, BridgeError> {
        let mut attachments = BTreeMap::new();
        let mut installed = Vec::new();
        for (module_id, module) in &self.modules {
            let source = match source_identity(&module.sources) {
                Ok(source) if source.canonical_module_id() == module_id => source,
                Ok(_) => {
                    discard_programs(runtime, tab_id, &installed);
                    return Err(BridgeError::ProvenanceAttachment(format!(
                        "direct module `{module_id}` does not retain its own canonical source identity"
                    )));
                }
                Err(error) => {
                    discard_programs(runtime, tab_id, &installed);
                    return Err(error);
                }
            };
            let handle = match runtime.install_program(tab_id, origin, source, &module.program) {
                Ok(handle) => handle,
                Err(error) => {
                    discard_programs(runtime, tab_id, &installed);
                    return Err(BridgeError::PageRuntime(error));
                }
            };
            installed.push(handle);
            match module.attach_existing_in(runtime.program_registry(), handle) {
                Ok(attachment) => {
                    attachments.insert(module_id.clone(), attachment);
                }
                Err(error) => {
                    discard_programs(runtime, tab_id, &installed);
                    return Err(error);
                }
            }
        }
        let Some(entry) = attachments.get(&self.entry).cloned() else {
            discard_programs(runtime, tab_id, &installed);
            return Err(BridgeError::ProvenanceAttachment(
                "direct module graph entry has no attached runtime module".to_string(),
            ));
        };
        Ok(DirectPageModuleGraphAttachment {
            entry,
            modules: attachments,
        })
    }

    /// Like [`Self::attach_in_page_realm`], but retains static BlueTS metadata
    /// for every exact module generation. A retention failure forgets earlier
    /// graph records and discards every admitted program, so callers never
    /// observe a partially debuggable graph.
    pub fn attach_debug_in_page_realm(
        &self,
        runtime: &mut bluejs::BlueJsPageRuntime,
        tab_id: u64,
        origin: &bluejs::BlueJsPageOrigin,
        debug_registry: &mut DirectDebugRegistry,
    ) -> Result<DirectPageModuleGraphAttachment, BridgeError> {
        let attachment = self.attach_in_page_realm(runtime, tab_id, origin)?;
        let mut retained = Vec::new();
        for (module_id, module) in &self.modules {
            let module_attachment = attachment
                .modules
                .get(module_id)
                .expect("every attached direct module is retained under its canonical ID");
            if let Err(error) = debug_registry.retain(
                runtime.program_registry(),
                module_attachment,
                &module.language_version,
                &module.compiler_options_fingerprint,
                &module.sources,
                &module.debug_info,
            ) {
                for handle in retained {
                    debug_registry.forget(handle);
                }
                discard_programs(
                    runtime,
                    tab_id,
                    &attachment
                        .modules
                        .values()
                        .map(|module_attachment| module_attachment.handle)
                        .collect::<Vec<_>>(),
                );
                return Err(BridgeError::DebugAttachment(error));
            }
            retained.push(module_attachment.handle);
        }
        Ok(attachment)
    }
}

fn discard_programs(
    runtime: &mut bluejs::BlueJsPageRuntime,
    tab_id: u64,
    handles: &[bluejs::BlueJsProgramHandle],
) {
    for handle in handles.iter().rev().copied() {
        let _ = runtime.discard_program(tab_id, handle);
    }
}
