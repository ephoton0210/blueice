// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Admission and scheduling support for the bounded JavaScript page host.
//!
//! Keeping declaration admission apart from debugger control prevents the
//! page-executor lifecycle module from becoming an unmaintainable mix of
//! source policy, bytecode admission, and execution state transitions.

use super::*;

/// One declaration accepted into a tab realm but intentionally not yet run.
/// The retained BlueJS handles are private to the owner and remain valid only
/// until navigation closes this realm.
#[derive(Debug, Clone)]
pub(super) enum DeferredJavaScriptExecution {
    Classic {
        handle: BlueJsProgramHandle,
    },
    ModuleGraph {
        entry: BlueJsProgramHandle,
        installed: Vec<BlueJsProgramHandle>,
    },
}

/// The result of admitting one page declaration. Ordinary page-host mode
/// executes synchronously; the opt-in native-debugger mode defers only after
/// complete parsing, compilation, realm admission, and debugger identity
/// registration have succeeded.
#[derive(Debug, Clone)]
pub(super) enum DeclarationExecution {
    Executed,
    Deferred(DeferredJavaScriptExecution),
}

impl JavaScriptPageExecutor {
    pub(super) fn execute_declaration(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        document_url: &str,
        declaration: BlueJsPageScriptDeclaration,
    ) {
        let (ordinal, kind) = declaration_identity(&declaration);
        let result = match declaration {
            BlueJsPageScriptDeclaration::Inline { source, .. } => {
                self.execute_inline(tab_id, document_generation, ordinal, kind, source)
            }
            BlueJsPageScriptDeclaration::External { src, .. } => self.execute_external(
                tab_id,
                document_generation,
                ordinal,
                kind,
                document_url,
                src,
            ),
        };
        match result {
            Ok(DeclarationExecution::Executed) => {
                self.push_report(JavaScriptPageExecutionReport::Executed {
                    tab_id: tab_id.as_u64(),
                    document_generation,
                    ordinal,
                    kind,
                });
            }
            Ok(DeclarationExecution::Deferred(execution)) => {
                if let Err(category) = self.defer_debugger_execution(
                    tab_id,
                    document_generation,
                    ordinal,
                    kind,
                    execution,
                ) {
                    self.push_report(JavaScriptPageExecutionReport::Rejected {
                        tab_id: tab_id.as_u64(),
                        document_generation,
                        ordinal,
                        kind,
                        category,
                    });
                }
            }
            Err(category) => self.push_report(JavaScriptPageExecutionReport::Rejected {
                tab_id: tab_id.as_u64(),
                document_generation,
                ordinal,
                kind,
                category,
            }),
        }
    }

    fn execute_inline(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        ordinal: u32,
        kind: BlueJsPageScriptKind,
        source: String,
    ) -> Result<DeclarationExecution, &'static str> {
        let entry = inline_module_id(tab_id, document_generation, ordinal);
        let module = AuthorizedJavaScriptModule::new(entry.clone(), source)
            .expect("core-generated inline JavaScript identity is valid");
        match kind {
            BlueJsPageScriptKind::Classic => self.execute_classic(tab_id, &module),
            BlueJsPageScriptKind::Module => {
                let graph = AuthorizedJavaScriptModuleGraph::new(
                    entry,
                    [module],
                    [],
                    "core-inline-javascript-v1",
                )
                .expect("one core-generated inline module forms a valid graph");
                self.execute_module_graph(tab_id, &graph)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_external(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        ordinal: u32,
        kind: BlueJsPageScriptKind,
        document_url: &str,
        declared_src: String,
    ) -> Result<DeclarationExecution, &'static str> {
        let Some(authorizer) = self.external_source_authorizer.as_mut() else {
            return Err("external JavaScript declarations require an authorized loader");
        };
        let graph = authorizer
            .authorize(&JavaScriptPageSourceRequest {
                tab_id,
                document_generation,
                ordinal,
                kind,
                document_url: document_url.to_string(),
                declared_src,
            })
            .map_err(|_| "external JavaScript source authorization rejected the page script")?;
        match kind {
            BlueJsPageScriptKind::Classic => self.execute_external_classic(tab_id, &graph),
            BlueJsPageScriptKind::Module => self.execute_module_graph(tab_id, &graph),
        }
    }

    fn execute_external_classic(
        &mut self,
        tab_id: TabId,
        graph: &AuthorizedJavaScriptModuleGraph,
    ) -> Result<DeclarationExecution, &'static str> {
        if graph.modules.len() != 1 {
            return Err("classic JavaScript source graph is not closed");
        }
        let module = graph
            .modules
            .get(graph.entry())
            .expect("graph construction validates its entry");
        self.execute_classic(tab_id, module)
    }

    fn execute_classic(
        &mut self,
        tab_id: TabId,
        module: &AuthorizedJavaScriptModule,
    ) -> Result<DeclarationExecution, &'static str> {
        self.check_module_source(module)?;
        let program = parse(module.source()).map_err(parse_category)?;
        let program = BlueJsProgramV1::Script(program);
        program.compile().map_err(compile_category)?;
        let origin = self.origin_for_tab(tab_id)?;
        let handle = self
            .runtime
            .install_program(tab_id.as_u64(), &origin, source_identity(module), &program)
            .map_err(page_runtime_category)?;
        if let Err(category) = self.register_debugger_programs(tab_id, &[handle]) {
            self.runtime
                .discard_program(tab_id.as_u64(), handle)
                .expect("an admitted classic program remains owned until execution");
            return Err(category);
        }
        if self.config.native_debugger_execution_control {
            return Ok(DeclarationExecution::Deferred(
                DeferredJavaScriptExecution::Classic { handle },
            ));
        }
        self.runtime
            .execute_program(tab_id.as_u64(), handle)
            .map(|_: Value| DeclarationExecution::Executed)
            .map_err(page_runtime_category)
    }

    fn execute_module_graph(
        &mut self,
        tab_id: TabId,
        graph: &AuthorizedJavaScriptModuleGraph,
    ) -> Result<DeclarationExecution, &'static str> {
        if graph.modules.len() > self.config.max_modules_per_graph {
            return Err("JavaScript module graph exceeds configured policy");
        }
        let mut programs = BTreeMap::new();
        for (module_id, module) in &graph.modules {
            self.check_module_source(module)?;
            let mut parsed = parse_module(module.source()).map_err(parse_category)?;
            rewrite_static_module_requests(module_id, &mut parsed, &graph.resolutions)?;
            let program = BlueJsProgramV1::Module(parsed);
            // Preflight every module before admitting any part of the graph.
            program.compile().map_err(compile_category)?;
            programs.insert(module_id.clone(), program);
        }
        let origin = self.origin_for_tab(tab_id)?;
        let mut installed = Vec::new();
        for (module_id, program) in &programs {
            let module = graph
                .modules
                .get(module_id)
                .expect("programs derive from every graph module");
            let handle = match self.runtime.install_program(
                tab_id.as_u64(),
                &origin,
                source_identity(module),
                program,
            ) {
                Ok(handle) => handle,
                Err(error) => {
                    discard_programs(&mut self.runtime, tab_id, &installed);
                    return Err(page_runtime_category(error));
                }
            };
            installed.push(handle);
        }
        let entry = programs
            .keys()
            .position(|module_id| module_id == graph.entry())
            .and_then(|index| installed.get(index).copied())
            .expect("graph construction validates the entry");
        if let Err(category) = self.register_debugger_programs(tab_id, &installed) {
            discard_programs(&mut self.runtime, tab_id, &installed);
            return Err(category);
        }
        if self.config.native_debugger_execution_control {
            return Ok(DeclarationExecution::Deferred(
                DeferredJavaScriptExecution::ModuleGraph { entry, installed },
            ));
        }
        self.runtime
            .execute_module_graph(tab_id.as_u64(), entry, installed)
            .map(|_: Value| DeclarationExecution::Executed)
            .map_err(page_runtime_category)
    }

    fn check_module_source(&self, module: &AuthorizedJavaScriptModule) -> Result<(), &'static str> {
        (module.source().len() <= self.config.max_source_bytes_per_module)
            .then_some(())
            .ok_or("JavaScript source exceeds configured policy")
    }

    fn origin_for_tab(&self, tab_id: TabId) -> Result<BlueJsPageOrigin, &'static str> {
        self.live_documents
            .get(&tab_id)
            .map(|identity| identity.origin.clone())
            .ok_or("page realm is no longer available")
    }
}
