// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl BlueJsChildHost {
    /// Creates an empty, isolated page host with the public BlueJS runtime's
    /// default fixed realm/program/bytecode bounds.
    pub fn new() -> Result<Self, BlueJsPageRuntimeError> {
        Self::with_runtime_config(BlueJsPageRuntimeConfig::default())
    }

    /// Creates a host with a runtime configuration and enough child-wide
    /// capacity to admit every permitted realm. The launcher-selected
    /// aggregate envelope uses `with_runtime_limits` instead.
    pub fn with_runtime_config(
        config: BlueJsPageRuntimeConfig,
    ) -> Result<Self, BlueJsPageRuntimeError> {
        let limits = BlueJsHostRuntimeLimits::from_runtime_config(config);
        Ok(Self {
            runtime: BlueJsPageRuntime::new(config)?,
            limits,
            debug_registry: DirectDebugRegistry::default(),
            documents: BTreeMap::new(),
            next_debugger_program_handle: 1,
            next_debugger_program_generation: 1,
            next_debugger_metadata_handle: CHILD_DEBUGGER_METADATA_ID_NAMESPACE_START,
            next_debugger_metadata_generation: CHILD_DEBUGGER_METADATA_ID_NAMESPACE_START,
            script_dom_capability: None,
        })
    }

    /// Installs only a launcher-originated, generation-private script socket.
    /// The mutually exclusive page-visible proof profiles are owner-only;
    /// none grants the general typed DOM/event profile.
    pub fn configure_script_dom_capability(
        &mut self,
        socket_path: PathBuf,
        session_token: String,
        enable_lookup_probe: bool,
        enable_dom_text_profile: bool,
        enable_dom_mutation_profile: bool,
        enable_dom_event_profile: bool,
    ) -> io::Result<()> {
        if !self.documents.is_empty()
            || self.script_dom_capability.is_some()
            || !socket_path.is_absolute()
            || !script::valid_script_session_token(&session_token)
            || [
                enable_lookup_probe,
                enable_dom_text_profile,
                enable_dom_mutation_profile,
                enable_dom_event_profile,
            ]
            .into_iter()
            .filter(|enabled| *enabled)
            .count()
                > 1
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "child script capability must be configured once before document admission",
            ));
        }
        self.script_dom_capability = Some(ScriptDomCapability {
            socket_path,
            session_token,
            enable_lookup_probe,
            enable_dom_text_profile,
            enable_dom_mutation_profile,
            enable_dom_event_profile,
        });
        Ok(())
    }

    /// Creates a child using the complete, immutable launcher-owner envelope.
    pub fn with_runtime_limits(limits: BlueJsHostRuntimeLimits) -> Result<Self, &'static str> {
        let config = limits.runtime_config()?;
        let mut host = Self::with_runtime_config(config)
            .map_err(|_| "BlueJS page-host runtime limits are invalid")?;
        host.limits = limits;
        Ok(host)
    }

    /// Handles one post-handshake request. The connection boundary performs
    /// `Hello` authentication first, so a later `Hello` is rejected rather
    /// than accidentally resetting host state.
    pub fn handle_request(&mut self, request: PageHostRequest) -> PageHostReply {
        match request {
            PageHostRequest::SynchronizeDocument { document } => self.synchronize(document),
            PageHostRequest::DispatchClick {
                tab_id,
                document_generation,
                node_id,
            } => self.dispatch_click(tab_id, document_generation, node_id),
            PageHostRequest::CloseRealm {
                tab_id,
                document_generation,
            } => self.close_realm(tab_id, document_generation),
            PageHostRequest::GetRealmStats {
                tab_id,
                document_generation,
            } => self.realm_stats(tab_id, document_generation),
            PageHostRequest::GetChildStats => self.child_stats(),
            PageHostRequest::ListDebuggerPrograms {
                tab_id,
                document_generation,
            } => self.debugger_programs(tab_id, document_generation),
            PageHostRequest::ListDebuggerBlueTsMetadata {
                tab_id,
                document_generation,
                program,
            } => self.debugger_bluets_metadata(tab_id, document_generation, program),
            PageHostRequest::DescribeDebuggerBlueTsMetadata {
                tab_id,
                document_generation,
                program,
                metadata,
            } => self.debugger_bluets_metadata_summary(
                tab_id,
                document_generation,
                program,
                metadata,
            ),
            PageHostRequest::DescribeDebuggerBlueTsMetadataLoweringSummary {
                tab_id,
                document_generation,
                program,
                metadata,
            } => self.debugger_bluets_metadata_lowering_summary(
                tab_id,
                document_generation,
                program,
                metadata,
            ),
            PageHostRequest::ListDebuggerBlueTsMetadataSources {
                tab_id,
                document_generation,
                program,
                metadata,
            } => self.debugger_bluets_metadata_sources(
                tab_id,
                document_generation,
                program,
                metadata,
            ),
            PageHostRequest::ListDebuggerBlueTsMetadataTypes {
                tab_id,
                document_generation,
                program,
                metadata,
            } => {
                self.debugger_bluets_metadata_types(tab_id, document_generation, program, metadata)
            }
            PageHostRequest::DescribeDebuggerBlueTsMetadataType {
                tab_id,
                document_generation,
                program,
                metadata,
                type_id,
            } => self.debugger_bluets_metadata_type_display(
                tab_id,
                document_generation,
                program,
                metadata,
                type_id,
            ),
            PageHostRequest::ListDebuggerBlueTsMetadataSymbols {
                tab_id,
                document_generation,
                program,
                metadata,
            } => self.debugger_bluets_metadata_symbols(
                tab_id,
                document_generation,
                program,
                metadata,
            ),
            PageHostRequest::ListDebuggerBlueTsMetadataContracts {
                tab_id,
                document_generation,
                program,
                metadata,
            } => self.debugger_bluets_metadata_contracts(
                tab_id,
                document_generation,
                program,
                metadata,
            ),
            PageHostRequest::DescribeDebuggerBlueTsMetadataContract {
                tab_id,
                document_generation,
                program,
                metadata,
                contract_id,
            } => self.debugger_bluets_metadata_contract_display(
                tab_id,
                document_generation,
                program,
                metadata,
                contract_id,
            ),
            PageHostRequest::ValidateDebuggerBlueTsMetadataContract {
                tab_id,
                document_generation,
                program,
                metadata,
                contract_id,
                value,
            } => self.debugger_bluets_metadata_contract_validation(
                tab_id,
                document_generation,
                program,
                metadata,
                contract_id,
                value,
            ),
            PageHostRequest::DescribeDebuggerBlueTsMetadataSymbol {
                tab_id,
                document_generation,
                program,
                metadata,
                symbol_id,
            } => self.debugger_bluets_metadata_symbol_display(
                tab_id,
                document_generation,
                program,
                metadata,
                symbol_id,
            ),
            PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolLocation {
                tab_id,
                document_generation,
                program,
                metadata,
                symbol_id,
            } => self.debugger_bluets_metadata_symbol_location(
                tab_id,
                document_generation,
                program,
                metadata,
                symbol_id,
            ),
            PageHostRequest::DescribeDebuggerBlueTsMetadataContractLocation {
                tab_id,
                document_generation,
                program,
                metadata,
                contract_id,
            } => self.debugger_bluets_metadata_contract_location(
                tab_id,
                document_generation,
                program,
                metadata,
                contract_id,
            ),
            PageHostRequest::DescribeDebuggerBlueTsSafePointSpan {
                tab_id,
                document_generation,
                metadata,
                safe_point,
            } => self.debugger_bluets_safe_point_span(
                tab_id,
                document_generation,
                metadata,
                safe_point,
            ),
            PageHostRequest::DescribeDebuggerBlueTsExceptionLocation {
                tab_id,
                document_generation,
                program,
                metadata,
            } => self.debugger_bluets_exception_location(
                tab_id,
                document_generation,
                program,
                metadata,
            ),
            PageHostRequest::ResolveDebuggerBlueTsSourceBreakpoint {
                tab_id,
                document_generation,
                program,
                metadata,
                source_id,
                source_byte,
            } => self.debugger_bluets_source_breakpoint(
                tab_id,
                document_generation,
                program,
                metadata,
                source_id,
                source_byte,
            ),
            PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolType {
                tab_id,
                document_generation,
                program,
                metadata,
                symbol_id,
                type_id,
            } => self.debugger_bluets_metadata_symbol_type(
                tab_id,
                document_generation,
                program,
                metadata,
                symbol_id,
                type_id,
            ),
            PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolContract {
                tab_id,
                document_generation,
                program,
                metadata,
                symbol_id,
                contract_id,
            } => self.debugger_bluets_metadata_symbol_contract(
                tab_id,
                document_generation,
                program,
                metadata,
                symbol_id,
                contract_id,
            ),
            PageHostRequest::DescribeDebuggerBlueTsMetadataSource {
                tab_id,
                document_generation,
                program,
                metadata,
                source_id,
            } => self.debugger_bluets_metadata_source_provenance(
                tab_id,
                document_generation,
                program,
                metadata,
                source_id,
            ),
            PageHostRequest::ListDebuggerSafePoints {
                tab_id,
                document_generation,
                program,
            } => self.debugger_safe_points(tab_id, document_generation, program),
            PageHostRequest::ValidateDebuggerSafePoint {
                tab_id,
                document_generation,
                safe_point,
            } => self.validate_debugger_safe_point(tab_id, document_generation, safe_point),
            PageHostRequest::SetDebuggerBreakpoint {
                tab_id,
                document_generation,
                safe_point,
            } => self.set_debugger_breakpoint(tab_id, document_generation, safe_point),
            PageHostRequest::ListDebuggerBreakpoints {
                tab_id,
                document_generation,
            } => self.debugger_breakpoints(tab_id, document_generation),
            PageHostRequest::ClearDebuggerBreakpoint {
                tab_id,
                document_generation,
                safe_point,
            } => self.clear_debugger_breakpoint(tab_id, document_generation, safe_point),
            PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
                tab_id,
                document_generation,
                safe_point,
            } => self.arm_debugger_root_safe_point_breakpoint(
                tab_id,
                document_generation,
                safe_point,
            ),
            PageHostRequest::ArmDebuggerNestedSafePointBreakpoint {
                tab_id,
                document_generation,
                safe_point,
            } => self.arm_debugger_nested_safe_point_breakpoint(
                tab_id,
                document_generation,
                safe_point,
            ),
            PageHostRequest::ArmDebuggerLinkedNestedSafePointBreakpoint {
                tab_id,
                document_generation,
                entry_program,
                safe_point,
            } => self.arm_debugger_linked_safe_point_breakpoint(
                tab_id,
                document_generation,
                entry_program,
                safe_point,
            ),
            PageHostRequest::GetDebuggerExecutionState {
                tab_id,
                document_generation,
                program,
            } => self.debugger_execution_state(tab_id, document_generation, program),
            PageHostRequest::ResumeDebuggerExecution {
                tab_id,
                document_generation,
                program,
            } => self.resume_debugger_execution(tab_id, document_generation, program),
            PageHostRequest::StepDebuggerRootInstruction {
                tab_id,
                document_generation,
                program,
            } => self.step_debugger_root_instruction(tab_id, document_generation, program),
            PageHostRequest::StepDebuggerNestedInstruction { frame } => {
                self.step_debugger_nested_instruction(frame)
            }
            PageHostRequest::ResumeDebuggerNestedExecution { frame } => {
                self.resume_debugger_nested_execution(frame)
            }
            PageHostRequest::ResumeDebuggerLinkedNestedExecution { frame } => {
                self.resume_debugger_linked_nested_execution(frame)
            }
            PageHostRequest::GetDebuggerStackSnapshot {
                tab_id,
                document_generation,
                program,
                frame,
                max_frames,
                max_scope_entries,
            } => self.debugger_stack_snapshot(
                tab_id,
                document_generation,
                program,
                frame,
                max_frames,
                max_scope_entries,
            ),
            PageHostRequest::GetDebuggerLinkedStackSnapshot {
                frame,
                max_scope_entries,
            } => self.debugger_linked_stack_reply(frame, max_scope_entries),
            PageHostRequest::DescribeDebuggerLinkedStackSpans {
                frame,
                expected_stack,
                sources,
            } => self.debugger_linked_stack_spans_reply(frame, expected_stack, sources),
            PageHostRequest::GetDebuggerValueSnapshot { target } => {
                self.debugger_value_snapshot(target)
            }
            PageHostRequest::DescribeDebuggerStaticScopeRelation { target } => {
                self.debugger_static_scope_relation(*target)
            }
            PageHostRequest::StepDebuggerBlueTsSourceSpan {
                tab_id,
                document_generation,
                metadata,
                source_id,
                safe_point,
            } => self.step_debugger_bluets_source_span(
                tab_id,
                document_generation,
                metadata,
                source_id,
                safe_point,
            ),
            PageHostRequest::AdvanceDebuggerExecution {
                tab_id,
                document_generation,
            } => self.advance_debugger_execution(tab_id, document_generation),
            PageHostRequest::Shutdown => PageHostReply::ShutdownAck,
            PageHostRequest::Hello { .. } | PageHostRequest::Unknown => invalid_request(),
        }
    }

    pub(super) fn synchronize(&mut self, document: PageHostDocument) -> PageHostReply {
        if document.tab_id == 0 || document.document_generation == 0 {
            return invalid_request();
        }
        if document.scripts.len() > MAX_SCRIPTS_PER_DOCUMENT {
            return resource_limit();
        }
        let mut ordinals = BTreeSet::new();
        if document
            .scripts
            .iter()
            .any(|script| !ordinals.insert(script.ordinal))
        {
            return invalid_request();
        }
        let source_bytes = document
            .scripts
            .iter()
            .flat_map(|script| script.graph.modules.iter())
            .try_fold(0usize, |total, source| {
                total.checked_add(source.source.len())
            });
        if !matches!(source_bytes, Some(total) if total <= MAX_SOURCE_BYTES_PER_DOCUMENT) {
            return resource_limit();
        }
        let origin = match validated_document_origin(&document.snapshot) {
            Ok(origin) => origin,
            Err(DocumentSnapshotError::ResourceLimit) => return resource_limit(),
            Err(DocumentSnapshotError::Invalid) => return invalid_request(),
        };
        if let Some(current) = self.documents.get(&document.tab_id) {
            if document.document_generation < current.generation {
                return stale_document();
            }
            if document.document_generation == current.generation {
                return PageHostReply::Synchronized {
                    tab_id: document.tab_id,
                    document_generation: document.document_generation,
                    already_current: true,
                    reports: Vec::new(),
                };
            }
        }

        if !self.documents.contains_key(&document.tab_id) && !self.can_reserve_new_realm() {
            return resource_limit();
        }

        // Parse/compile every independent declaration first. A graph that
        // cannot be structurally admitted becomes one source-free rejection,
        // while a later declaration remains eligible exactly as browser
        // document-order execution requires. No candidate program enters a
        // new realm until source/graph preflight has completed.
        let page_dom_profile = self
            .script_dom_capability
            .as_ref()
            .map_or(PageDomProfile::Snapshot, ScriptDomCapability::profile);
        let prepared: Vec<_> = document
            .scripts
            .into_iter()
            .map(|script| prepare_script(script, page_dom_profile))
            .collect();

        let lifecycle = if self.documents.contains_key(&document.tab_id) {
            self.runtime.navigate(document.tab_id, origin.clone())
        } else {
            self.runtime.open_realm(document.tab_id, origin.clone())
        };
        if lifecycle.is_err() {
            return host_failure();
        }
        // Realm replacement invalidates every prior program generation for
        // this tab. Prune before the successor is exposed so a stale static
        // record cannot survive the navigation window in the child.
        self.debug_registry
            .prune_invalid(self.runtime.program_registry());
        let click_event_family = match install_document_snapshot_bindings(
            &mut self.runtime,
            document.tab_id,
            document.document_generation,
            &document.snapshot,
            self.script_dom_capability.clone(),
        ) {
            Ok(family) => family,
            Err(_) => {
                // A replacement document whose fixed bindings cannot be installed
                // must not leave a partially initialized successor realm. Closing
                // this fresh VM also drops every copied snapshot immediately.
                self.runtime.close_realm(document.tab_id);
                self.debug_registry
                    .prune_invalid(self.runtime.program_registry());
                self.documents.remove(&document.tab_id);
                return host_failure();
            }
        };
        self.documents.insert(
            document.tab_id,
            LiveDocument {
                generation: document.document_generation,
                origin: origin.clone(),
                click_event_family,
                pending_click_tasks: VecDeque::new(),
                debugger_execution_control: document.debugger_execution_control,
                debugger_programs: BTreeMap::new(),
                debugger_breakpoints: BTreeSet::new(),
                pending_debugger_executions: VecDeque::new(),
                debugger_execution_states: BTreeMap::new(),
            },
        );

        if document.debugger_execution_control {
            return self.defer_debugger_execution_document(
                document.tab_id,
                document.document_generation,
                &origin,
                prepared,
            );
        }

        let mut reports = Vec::with_capacity(prepared.len());
        for prepared in prepared {
            let (ordinal, language, kind, outcome) = match prepared {
                PreparedScript::Rejected {
                    ordinal,
                    language,
                    kind,
                    category,
                } => (
                    ordinal,
                    language,
                    kind,
                    PageHostScriptOutcome::Rejected {
                        category: category.to_string(),
                    },
                ),
                PreparedScript::JavaScriptClassic {
                    ordinal,
                    source,
                    program,
                } => {
                    let outcome = execute_classic(
                        &mut self.runtime,
                        document.tab_id,
                        &origin,
                        source,
                        program,
                    );
                    (
                        ordinal,
                        PageHostScriptLanguage::JavaScript,
                        PageHostScriptKind::Classic,
                        outcome,
                    )
                }
                PreparedScript::JavaScriptModule {
                    ordinal,
                    graph,
                    programs,
                } => {
                    let outcome = execute_module_graph(
                        &mut self.runtime,
                        document.tab_id,
                        &origin,
                        graph,
                        programs,
                    );
                    (
                        ordinal,
                        PageHostScriptLanguage::JavaScript,
                        PageHostScriptKind::Module,
                        outcome,
                    )
                }
                PreparedScript::BlueTsClassic { ordinal, script } => {
                    let outcome = execute_bluets_classic(
                        &mut self.runtime,
                        &mut self.debug_registry,
                        document.tab_id,
                        &origin,
                        &script,
                    );
                    (
                        ordinal,
                        PageHostScriptLanguage::BlueTs,
                        PageHostScriptKind::Classic,
                        outcome,
                    )
                }
                PreparedScript::BlueTsModule { ordinal, graph } => {
                    let outcome = execute_bluets_module_graph(
                        &mut self.runtime,
                        &mut self.debug_registry,
                        document.tab_id,
                        &origin,
                        &graph,
                    );
                    (
                        ordinal,
                        PageHostScriptLanguage::BlueTs,
                        PageHostScriptKind::Module,
                        outcome,
                    )
                }
            };
            reports.push(PageHostScriptReport {
                tab_id: document.tab_id,
                document_generation: document.document_generation,
                ordinal,
                language,
                kind,
                outcome,
            });
        }
        if self.refresh_debugger_programs(document.tab_id).is_err() {
            // A debugger-location record is part of the live-realm contract.
            // Do not report a runnable successor if the child could not mint
            // a bounded opaque inventory for every retained program.
            self.runtime.close_realm(document.tab_id);
            self.debug_registry
                .prune_invalid(self.runtime.program_registry());
            self.documents.remove(&document.tab_id);
            return host_failure();
        }
        PageHostReply::Synchronized {
            tab_id: document.tab_id,
            document_generation: document.document_generation,
            already_current: false,
            reports,
        }
    }

    pub(super) fn dispatch_click(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        node_id: u64,
    ) -> PageHostReply {
        let Some(document) = self.documents.get_mut(&tab_id) else {
            return stale_document();
        };
        if document.generation != document_generation {
            return stale_document();
        }
        if document.pending_click_tasks.len() >= MAX_PENDING_CLICK_TASKS_PER_REALM {
            return resource_limit();
        }
        document.pending_click_tasks.push_back(PendingClickTask {
            generation: document_generation,
            node_id,
        });
        self.run_next_click_task(tab_id)
    }

    pub(super) fn run_next_click_task(&mut self, tab_id: u64) -> PageHostReply {
        let Some(document) = self.documents.get_mut(&tab_id) else {
            return stale_document();
        };
        let Some(task) = document.pending_click_tasks.pop_front() else {
            return invalid_request();
        };
        if document.generation != task.generation {
            return stale_document();
        }
        let family = document.click_event_family;
        let default_prevented = if let Some(family) = family {
            match self.runtime.dispatch_host_click(
                tab_id,
                family,
                HostObjectKey::new(tab_id, task.generation, task.node_id),
            ) {
                Ok(prevented) => prevented,
                Err(_) => return host_failure(),
            }
        } else if self.runtime.run_click_microtask_checkpoint(tab_id).is_err() {
            return host_failure();
        } else {
            false
        };
        PageHostReply::ClickDispatched {
            tab_id,
            document_generation: task.generation,
            default_prevented,
        }
    }

    pub(super) fn can_reserve_new_realm(&self) -> bool {
        let Some(realm_count) = self.documents.len().checked_add(1) else {
            return false;
        };
        if realm_count > self.limits.max_realms {
            return false;
        }
        [
            (
                self.limits.max_programs_per_realm,
                self.limits.max_reserved_programs,
            ),
            (
                self.limits.max_bytecode_bytes_per_realm,
                self.limits.max_reserved_bytecode_bytes,
            ),
            (
                self.limits.max_heap_bytes_per_realm,
                self.limits.max_reserved_heap_bytes,
            ),
        ]
        .into_iter()
        .all(|(per_realm, reserved)| {
            realm_count
                .checked_mul(per_realm)
                .is_some_and(|needed| needed <= reserved)
        })
    }

    /// Retains an explicitly core-selected document in child-owned document
    /// order. Only already-authorized JavaScript classic programs are admitted
    /// before the first advance, because core needs their opaque identity and
    /// exact safe-point inventory before it can arm the single root
    /// continuation. A leading direct BlueTS declaration has no installed
    /// root-continuation pause surface, so it executes immediately while the
    /// document has no earlier deferred work; that preserves document order
    /// and lets its separately gated static-metadata inventory be discovered.
    pub(super) fn defer_debugger_execution_document(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        origin: &BlueJsPageOrigin,
        prepared: Vec<PreparedScript>,
    ) -> PageHostReply {
        let mut reports = Vec::new();
        for prepared in prepared {
            match prepared {
                PreparedScript::Rejected {
                    ordinal,
                    language,
                    kind,
                    category,
                } => reports.push(script_report(
                    tab_id,
                    document_generation,
                    ordinal,
                    language,
                    kind,
                    rejected(category),
                )),
                PreparedScript::JavaScriptClassic {
                    ordinal,
                    source,
                    program,
                } => {
                    let source = match source_identity(&source) {
                        Ok(source) => source,
                        Err(category) => {
                            reports.push(script_report(
                                tab_id,
                                document_generation,
                                ordinal,
                                PageHostScriptLanguage::JavaScript,
                                PageHostScriptKind::Classic,
                                rejected(category),
                            ));
                            continue;
                        }
                    };
                    let handle = match self
                        .runtime
                        .install_program(tab_id, origin, source, &program)
                    {
                        Ok(handle) => handle,
                        Err(error) => {
                            reports.push(script_report(
                                tab_id,
                                document_generation,
                                ordinal,
                                PageHostScriptLanguage::JavaScript,
                                PageHostScriptKind::Classic,
                                rejected(page_runtime_category(error)),
                            ));
                            continue;
                        }
                    };
                    let program = match self.register_debugger_program(tab_id, handle) {
                        Ok(program) => program,
                        Err(()) => return self.fail_debugger_execution_document(tab_id),
                    };
                    let Some(document) = self.documents.get_mut(&tab_id) else {
                        return self.fail_debugger_execution_document(tab_id);
                    };
                    document
                        .debugger_execution_states
                        .insert(program, ChildDebuggerExecutionStatus::Pending);
                    document
                        .pending_debugger_executions
                        .push_back(PendingDebuggerExecution {
                            ordinal,
                            language: PageHostScriptLanguage::JavaScript,
                            kind: PageHostScriptKind::Classic,
                            program: Some(program),
                            nested_safe_point: None,
                            linked_safe_point: None,
                            execution: DeferredChildExecution::JavaScriptClassic {
                                handle,
                                root_safe_point: None,
                            },
                        });
                }
                PreparedScript::JavaScriptModule {
                    ordinal,
                    graph,
                    programs,
                } => {
                    self.enqueue_debugger_execution(
                        tab_id,
                        PendingDebuggerExecution {
                            ordinal,
                            language: PageHostScriptLanguage::JavaScript,
                            kind: PageHostScriptKind::Module,
                            program: None,
                            nested_safe_point: None,
                            linked_safe_point: None,
                            execution: DeferredChildExecution::JavaScriptModule { graph, programs },
                        },
                    );
                }
                PreparedScript::BlueTsClassic { ordinal, script } => {
                    let attachment = match script.attach_debug_in_page_realm(
                        &mut self.runtime,
                        tab_id,
                        origin,
                        &mut self.debug_registry,
                    ) {
                        Ok(attachment) => attachment,
                        Err(error) => {
                            reports.push(script_report(
                                tab_id,
                                document_generation,
                                ordinal,
                                PageHostScriptLanguage::BlueTs,
                                PageHostScriptKind::Classic,
                                rejected(bluets_bridge_category(error)),
                            ));
                            continue;
                        }
                    };
                    let program = match self.register_debugger_program(tab_id, attachment.handle) {
                        Ok(program) => program,
                        Err(()) => return self.fail_debugger_execution_document(tab_id),
                    };
                    let Some(document) = self.documents.get_mut(&tab_id) else {
                        return self.fail_debugger_execution_document(tab_id);
                    };
                    document
                        .debugger_execution_states
                        .insert(program, ChildDebuggerExecutionStatus::Pending);
                    document
                        .pending_debugger_executions
                        .push_back(PendingDebuggerExecution {
                            ordinal,
                            language: PageHostScriptLanguage::BlueTs,
                            kind: PageHostScriptKind::Classic,
                            program: Some(program),
                            nested_safe_point: None,
                            linked_safe_point: None,
                            execution: DeferredChildExecution::BlueTsClassic {
                                handle: attachment.handle,
                                root_safe_point: None,
                            },
                        });
                }
                PreparedScript::BlueTsModule { ordinal, graph } => {
                    let attachment = match graph.attach_debug_in_page_realm(
                        &mut self.runtime,
                        tab_id,
                        origin,
                        &mut self.debug_registry,
                    ) {
                        Ok(attachment) => attachment,
                        Err(error) => {
                            reports.push(script_report(
                                tab_id,
                                document_generation,
                                ordinal,
                                PageHostScriptLanguage::BlueTs,
                                PageHostScriptKind::Module,
                                rejected(bluets_bridge_category(error)),
                            ));
                            continue;
                        }
                    };
                    let program =
                        match self.register_debugger_program(tab_id, attachment.entry.handle) {
                            Ok(program) => program,
                            Err(()) => return self.fail_debugger_execution_document(tab_id),
                        };
                    let document = self
                        .documents
                        .get_mut(&tab_id)
                        .expect("the checked module graph's document remains live");
                    document
                        .debugger_execution_states
                        .insert(program, ChildDebuggerExecutionStatus::Pending);
                    document
                        .pending_debugger_executions
                        .push_back(PendingDebuggerExecution {
                            ordinal,
                            language: PageHostScriptLanguage::BlueTs,
                            kind: PageHostScriptKind::Module,
                            program: Some(program),
                            nested_safe_point: None,
                            linked_safe_point: None,
                            execution: DeferredChildExecution::BlueTsModule {
                                attachment,
                                root_safe_point: None,
                            },
                        });
                }
            }
        }
        if self.refresh_debugger_programs(tab_id).is_err() {
            return self.fail_debugger_execution_document(tab_id);
        }
        PageHostReply::Synchronized {
            tab_id,
            document_generation,
            already_current: false,
            reports,
        }
    }

    pub(super) fn enqueue_debugger_execution(
        &mut self,
        tab_id: u64,
        pending: PendingDebuggerExecution,
    ) {
        self.documents
            .get_mut(&tab_id)
            .expect("the deferred child document remains live while it is prepared")
            .pending_debugger_executions
            .push_back(pending);
    }

    pub(super) fn fail_debugger_execution_document(&mut self, tab_id: u64) -> PageHostReply {
        self.runtime.close_realm(tab_id);
        self.debug_registry
            .prune_invalid(self.runtime.program_registry());
        self.documents.remove(&tab_id);
        host_failure()
    }

    pub(super) fn close_realm(&mut self, tab_id: u64, document_generation: u64) -> PageHostReply {
        match self.documents.get(&tab_id) {
            None => unknown_realm(),
            Some(document) if document.generation != document_generation => stale_document(),
            Some(_) => {
                self.runtime.close_realm(tab_id);
                self.debug_registry
                    .prune_invalid(self.runtime.program_registry());
                self.documents.remove(&tab_id);
                PageHostReply::RealmClosed {
                    tab_id,
                    document_generation,
                }
            }
        }
    }

    pub(super) fn realm_stats(&self, tab_id: u64, document_generation: u64) -> PageHostReply {
        let Some(document) = self.documents.get(&tab_id) else {
            return unknown_realm();
        };
        if document.generation != document_generation {
            return stale_document();
        }
        match self.runtime.realm_stats(tab_id) {
            Ok(stats) => {
                let reply = PageHostRealmStats {
                    tab_id,
                    document_generation,
                    program_count: u32::try_from(stats.program_count).unwrap_or(u32::MAX),
                    bytecode_bytes: u64::try_from(stats.bytecode_bytes).unwrap_or(u64::MAX),
                    heap_bytes: u64::try_from(stats.heap.managed_bytes).unwrap_or(u64::MAX),
                };
                if !reply.is_well_formed() {
                    return host_failure();
                }
                PageHostReply::RealmStats(reply)
            }
            Err(_) => host_failure(),
        }
    }

    /// Recomputes actual VM-managed usage from the live realm table on each
    /// request. No cached predecessor generation or conservative reservation
    /// is included, and checked sums fail closed instead of wrapping.
    pub(super) fn child_stats(&self) -> PageHostReply {
        let Ok(realm_count) = u32::try_from(self.documents.len()) else {
            return host_failure();
        };
        let mut totals = PageHostChildStats {
            realm_count,
            program_count: 0,
            bytecode_bytes: 0,
            heap_bytes: 0,
        };
        for (&tab_id, document) in &self.documents {
            let PageHostReply::RealmStats(stats) = self.realm_stats(tab_id, document.generation)
            else {
                return host_failure();
            };
            let Some(program_count) = totals
                .program_count
                .checked_add(u64::from(stats.program_count))
            else {
                return host_failure();
            };
            let Some(bytecode_bytes) = totals.bytecode_bytes.checked_add(stats.bytecode_bytes)
            else {
                return host_failure();
            };
            let Some(heap_bytes) = totals.heap_bytes.checked_add(stats.heap_bytes) else {
                return host_failure();
            };
            totals.program_count = program_count;
            totals.bytecode_bytes = bytecode_bytes;
            totals.heap_bytes = heap_bytes;
        }
        if !totals.is_well_formed()
            || usize::try_from(totals.program_count)
                .ok()
                .is_none_or(|count| count > self.limits.max_reserved_programs)
            || usize::try_from(totals.bytecode_bytes)
                .ok()
                .is_none_or(|bytes| bytes > self.limits.max_reserved_bytecode_bytes)
            || usize::try_from(totals.heap_bytes)
                .ok()
                .is_none_or(|bytes| bytes > self.limits.max_reserved_heap_bytes)
        {
            return host_failure();
        }
        PageHostReply::ChildStats(totals)
    }

    /// Lists only the child-minted private IDs for one exact realm. The core
    /// intentionally remaps these again before public debugger IPC sees them.
    pub(super) fn debugger_programs(&self, tab_id: u64, document_generation: u64) -> PageHostReply {
        let document = match self.exact_document(tab_id, document_generation) {
            Ok(document) => document,
            Err(reply) => return reply,
        };
        PageHostReply::DebuggerPrograms {
            tab_id,
            document_generation,
            programs: document
                .debugger_programs
                .iter()
                .map(|(&program_handle, record)| PageHostDebuggerProgram {
                    program_handle,
                    program_generation: record.program_generation,
                })
                .collect(),
        }
    }
}
