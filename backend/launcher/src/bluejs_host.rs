// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Launcher supervision and execution for the first isolated BlueJS page
//! host.
//!
//! [`SpawnedBlueJsHost`] owns a separate `blueice-bluejs-host` child, its
//! private Unix socket, and its one-time connection capability. The child
//! owns every [`BlueJsPageRuntime`] VM and program registry; no `Vm`, source
//! text, program handle, DOM object, or runtime value crosses back into the
//! launcher. A future core page-loader adapter is responsible for deriving
//! [`PageHostDocument`] from an already-authorized navigation. This module
//! deliberately does not let a page or a frontend connect to the child.

use blueice_bluejs::{
    parse, parse_module, BlueJsPageDebuggerExecutionState, BlueJsPageOrigin, BlueJsPageRuntime,
    BlueJsPageRuntimeConfig, BlueJsPageRuntimeError, BlueJsProgramHandle, BlueJsProgramV1,
    BlueJsSourceIdentity, CompileError, HeapConfig, HostFunctionError, HostValue, Module,
    ParseError, RuntimeError, Value, Vm, VmConfig,
};
use blueice_bluets::{
    AuthorizedModule, AuthorizedModuleLoader, AuthorizedModuleResolution, CompilerOptions,
    RuntimePolicy,
};
use blueice_bluets_bluejs::page_host_typings::{
    page_host_document_runtime_bindings_v1, PageHostDocumentTypingsV1,
};
use blueice_bluets_bluejs::{
    compile_direct_module_graph, compile_direct_script, BridgeError, DirectDebugRegistry,
    DirectModuleGraph, DirectScript,
};
use blueice_ipc::page_host::{
    self, PageHostDebuggerBlueTsMetadataSummary, PageHostDebuggerExecutionState,
    PageHostDebuggerMetadataHandle, PageHostDebuggerProgram, PageHostDebuggerSafePoint,
    PageHostDocument, PageHostDocumentSnapshot, PageHostErrorCode, PageHostModuleGraph,
    PageHostRealmStats, PageHostReply, PageHostRequest, PageHostScript, PageHostScriptKind,
    PageHostScriptLanguage, PageHostScriptOutcome, PageHostScriptReport, PageHostSource,
    PageHostStaticResolution, PAGE_HOST_DEBUGGER_MAX_BREAKPOINTS_PER_REALM,
    PAGE_HOST_DEBUGGER_MAX_SAFE_POINTS_PER_PROGRAM, PAGE_HOST_DOCUMENT_ORIGIN_MAX_BYTES,
    PAGE_HOST_DOCUMENT_TEXT_MAX_BYTES,
};
use blueice_net::canonical_http_origin;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

/// Execution/source limits independently enforced by the child. The caller
/// cannot widen them by serializing a larger graph over its private socket.
const MAX_SCRIPTS_PER_DOCUMENT: usize = 256;
const MAX_MODULES_PER_GRAPH: usize = 8;
const MAX_SOURCE_BYTES_PER_MODULE: usize = 1024 * 1024;
const MAX_SOURCE_BYTES_PER_DOCUMENT: usize = 8 * 1024 * 1024;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(5);

/// Static metadata inventory IDs are private to the child but intentionally
/// start in a separate range from child debugger-program IDs. The type-level
/// distinction remains the primary boundary; this disjoint start additionally
/// prevents a plausible-looking numeric program ID from being replayed as a
/// metadata handle by a buggy core adapter.
const CHILD_DEBUGGER_METADATA_ID_NAMESPACE_START: u64 = 1 << 63;

/// Immutable launcher-owner limits for one isolated page-host child.
///
/// These are per-realm envelopes, not a child-process RSS or aggregate memory
/// limit: VM managed-heap accounting deliberately excludes allocator, Rust,
/// registry, source, and operating-system overhead. The launcher may select
/// this value only while starting a core generation; page, frontend, and
/// page-host IPC contain no operation that can inspect or modify it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlueJsHostRuntimeLimits {
    pub max_realms: usize,
    pub max_programs_per_realm: usize,
    pub max_bytecode_bytes_per_realm: usize,
    pub max_heap_bytes_per_realm: usize,
}

impl Default for BlueJsHostRuntimeLimits {
    fn default() -> Self {
        let runtime = BlueJsPageRuntimeConfig::default();
        Self {
            max_realms: runtime.max_realms,
            max_programs_per_realm: runtime.max_programs_per_realm,
            max_bytecode_bytes_per_realm: runtime.max_bytecode_bytes_per_realm,
            max_heap_bytes_per_realm: runtime.vm.heap.max_heap_bytes,
        }
    }
}

impl BlueJsHostRuntimeLimits {
    /// Rebuilds the one narrow policy surface into the full BlueJS runtime
    /// configuration. All non-resource VM configuration remains the child
    /// default rather than becoming an embedding/deployment API.
    pub fn runtime_config(self) -> Result<BlueJsPageRuntimeConfig, &'static str> {
        if self.max_realms == 0
            || self.max_programs_per_realm == 0
            || self.max_bytecode_bytes_per_realm == 0
            || self.max_heap_bytes_per_realm == 0
        {
            return Err("BlueJS page-host runtime limits must be non-zero");
        }
        let defaults = BlueJsPageRuntimeConfig::default();
        let heap = HeapConfig {
            max_heap_bytes: self.max_heap_bytes_per_realm,
            major_threshold_bytes: defaults
                .vm
                .heap
                .major_threshold_bytes
                .min(self.max_heap_bytes_per_realm),
            ..defaults.vm.heap
        };
        let runtime = BlueJsPageRuntimeConfig {
            vm: VmConfig {
                heap,
                ..defaults.vm
            },
            max_realms: self.max_realms,
            max_programs_per_realm: self.max_programs_per_realm,
            max_bytecode_bytes_per_realm: self.max_bytecode_bytes_per_realm,
        };
        // PageRuntime validates its own count and byte bounds, while a VM
        // would otherwise defer malformed heap tuning until the first realm.
        Vm::new(runtime.vm).map_err(|_| "BlueJS page-host heap limits are invalid")?;
        BlueJsPageRuntime::new(runtime)
            .map(|_| runtime)
            .map_err(|_| "BlueJS page-host runtime limits are invalid")
    }
}

struct LiveDocument {
    generation: u64,
    origin: BlueJsPageOrigin,
    debugger_execution_control: bool,
    debugger_programs: BTreeMap<u64, ChildDebuggerProgram>,
    /// Exact child-private breakpoint configuration records. These are not a
    /// VM interruption hook; replacing or closing the realm drops them.
    debugger_breakpoints: BTreeSet<PageHostDebuggerSafePoint>,
    pending_debugger_executions: VecDeque<PendingDebuggerExecution>,
    debugger_execution_states: BTreeMap<PageHostDebuggerProgram, ChildDebuggerExecutionStatus>,
}

/// Private child-only association between a child-minted opaque debugger
/// identity and its BlueJS registry handle. The registry handle never leaves
/// this process; the core separately mints its public debugger identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ChildDebuggerProgram {
    program_generation: u64,
    runtime_handle: BlueJsProgramHandle,
    /// Created only on the first authenticated metadata-inventory request
    /// while the matching BlueTS registry attachment is still live. A plain
    /// JavaScript program never receives one.
    metadata: Option<PageHostDebuggerMetadataHandle>,
}

/// Child-private execution state for the narrow root-classic continuation.
/// It has no serializable VM frame, source, bytecode, or completion value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChildDebuggerExecutionStatus {
    Pending,
    Paused(PageHostDebuggerSafePoint),
    ResumeRequested,
    Completed,
}

/// A document-order declaration retained by the child in an explicitly
/// core-selected debugger-execution document. Classic programs are admitted
/// before the first advance so core can discover an opaque identity; all
/// source-bearing data remains in this child-only queue.
enum DeferredChildExecution {
    JavaScriptClassic {
        handle: BlueJsProgramHandle,
        root_safe_point: Option<PageHostDebuggerSafePoint>,
    },
    JavaScriptModule {
        graph: PageHostModuleGraph,
        programs: BTreeMap<String, BlueJsProgramV1>,
    },
    BlueTsClassic {
        script: Box<DirectScript>,
    },
    BlueTsModule {
        graph: Box<DirectModuleGraph>,
    },
}

struct PendingDebuggerExecution {
    ordinal: u32,
    language: PageHostScriptLanguage,
    kind: PageHostScriptKind,
    program: Option<PageHostDebuggerProgram>,
    execution: DeferredChildExecution,
}

/// The actual state machine running in the child process.
///
/// It accepts only fully selected source records. In particular, this type
/// has no filesystem/network resolver, no DOM/IPC callback path, and no API
/// that gives the parent a VM or a program handle. Its only host callbacks
/// are the two fixed core-validated JavaScript string snapshots installed
/// during document admission.
pub struct BlueJsChildHost {
    runtime: BlueJsPageRuntime,
    /// Static BlueTS metadata paired with the exact child-local BlueJS
    /// generation that direct lowering admitted. This remains entirely in the
    /// child: the page-host protocol has no source, symbol, type, contract,
    /// or runtime-value inspection operation.
    debug_registry: DirectDebugRegistry,
    documents: BTreeMap<u64, LiveDocument>,
    next_debugger_program_handle: u64,
    next_debugger_program_generation: u64,
    next_debugger_metadata_handle: u64,
    next_debugger_metadata_generation: u64,
}

impl BlueJsChildHost {
    /// Creates an empty, isolated page host with the public BlueJS runtime's
    /// default fixed realm/program/bytecode bounds.
    pub fn new() -> Result<Self, BlueJsPageRuntimeError> {
        Self::with_runtime_config(BlueJsPageRuntimeConfig::default())
    }

    /// Creates a host with launcher-selected runtime limits. This is exposed
    /// for deterministic tests and future launcher policy wiring; the page
    /// protocol contains no operation for a caller to modify it.
    pub fn with_runtime_config(
        config: BlueJsPageRuntimeConfig,
    ) -> Result<Self, BlueJsPageRuntimeError> {
        Ok(Self {
            runtime: BlueJsPageRuntime::new(config)?,
            debug_registry: DirectDebugRegistry::default(),
            documents: BTreeMap::new(),
            next_debugger_program_handle: 1,
            next_debugger_program_generation: 1,
            next_debugger_metadata_handle: CHILD_DEBUGGER_METADATA_ID_NAMESPACE_START,
            next_debugger_metadata_generation: CHILD_DEBUGGER_METADATA_ID_NAMESPACE_START,
        })
    }

    /// Handles one post-handshake request. The connection boundary performs
    /// `Hello` authentication first, so a later `Hello` is rejected rather
    /// than accidentally resetting host state.
    pub fn handle_request(&mut self, request: PageHostRequest) -> PageHostReply {
        match request {
            PageHostRequest::SynchronizeDocument { document } => self.synchronize(document),
            PageHostRequest::CloseRealm {
                tab_id,
                document_generation,
            } => self.close_realm(tab_id, document_generation),
            PageHostRequest::GetRealmStats {
                tab_id,
                document_generation,
            } => self.realm_stats(tab_id, document_generation),
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
            PageHostRequest::AdvanceDebuggerExecution {
                tab_id,
                document_generation,
            } => self.advance_debugger_execution(tab_id, document_generation),
            PageHostRequest::Shutdown => PageHostReply::ShutdownAck,
            PageHostRequest::Hello { .. } | PageHostRequest::Unknown => invalid_request(),
        }
    }

    fn synchronize(&mut self, document: PageHostDocument) -> PageHostReply {
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

        // Parse/compile every independent declaration first. A graph that
        // cannot be structurally admitted becomes one source-free rejection,
        // while a later declaration remains eligible exactly as browser
        // document-order execution requires. No candidate program enters a
        // new realm until source/graph preflight has completed.
        let prepared: Vec<_> = document.scripts.into_iter().map(prepare_script).collect();

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
        if install_document_snapshot_bindings(
            &mut self.runtime,
            document.tab_id,
            &document.snapshot,
        )
        .is_err()
        {
            // A replacement document whose fixed bindings cannot be installed
            // must not leave a partially initialized successor realm. Closing
            // this fresh VM also drops every copied snapshot immediately.
            self.runtime.close_realm(document.tab_id);
            self.debug_registry
                .prune_invalid(self.runtime.program_registry());
            self.documents.remove(&document.tab_id);
            return host_failure();
        }
        self.documents.insert(
            document.tab_id,
            LiveDocument {
                generation: document.document_generation,
                origin: origin.clone(),
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

    /// Retains an explicitly core-selected document in child-owned document
    /// order. Only already-authorized JavaScript classic programs are admitted
    /// before the first advance, because core needs their opaque identity and
    /// exact safe-point inventory before it can arm the single root
    /// continuation. A leading direct BlueTS declaration has no installed
    /// root-continuation pause surface, so it executes immediately while the
    /// document has no earlier deferred work; that preserves document order
    /// and lets its separately gated static-metadata inventory be discovered.
    fn defer_debugger_execution_document(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        origin: &BlueJsPageOrigin,
        prepared: Vec<PreparedScript>,
    ) -> PageHostReply {
        let mut reports = Vec::new();
        let mut may_execute_bluets_immediately = true;
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
                    may_execute_bluets_immediately = false;
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
                    may_execute_bluets_immediately = false;
                    self.enqueue_debugger_execution(
                        tab_id,
                        PendingDebuggerExecution {
                            ordinal,
                            language: PageHostScriptLanguage::JavaScript,
                            kind: PageHostScriptKind::Module,
                            program: None,
                            execution: DeferredChildExecution::JavaScriptModule { graph, programs },
                        },
                    );
                }
                PreparedScript::BlueTsClassic { ordinal, script } => {
                    if may_execute_bluets_immediately {
                        reports.push(script_report(
                            tab_id,
                            document_generation,
                            ordinal,
                            PageHostScriptLanguage::BlueTs,
                            PageHostScriptKind::Classic,
                            execute_bluets_classic(
                                &mut self.runtime,
                                &mut self.debug_registry,
                                tab_id,
                                origin,
                                &script,
                            ),
                        ));
                    } else {
                        self.enqueue_debugger_execution(
                            tab_id,
                            PendingDebuggerExecution {
                                ordinal,
                                language: PageHostScriptLanguage::BlueTs,
                                kind: PageHostScriptKind::Classic,
                                program: None,
                                execution: DeferredChildExecution::BlueTsClassic { script },
                            },
                        );
                    }
                }
                PreparedScript::BlueTsModule { ordinal, graph } => {
                    if may_execute_bluets_immediately {
                        reports.push(script_report(
                            tab_id,
                            document_generation,
                            ordinal,
                            PageHostScriptLanguage::BlueTs,
                            PageHostScriptKind::Module,
                            execute_bluets_module_graph(
                                &mut self.runtime,
                                &mut self.debug_registry,
                                tab_id,
                                origin,
                                &graph,
                            ),
                        ));
                    } else {
                        self.enqueue_debugger_execution(
                            tab_id,
                            PendingDebuggerExecution {
                                ordinal,
                                language: PageHostScriptLanguage::BlueTs,
                                kind: PageHostScriptKind::Module,
                                program: None,
                                execution: DeferredChildExecution::BlueTsModule { graph },
                            },
                        );
                    }
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

    fn enqueue_debugger_execution(&mut self, tab_id: u64, pending: PendingDebuggerExecution) {
        self.documents
            .get_mut(&tab_id)
            .expect("the deferred child document remains live while it is prepared")
            .pending_debugger_executions
            .push_back(pending);
    }

    fn fail_debugger_execution_document(&mut self, tab_id: u64) -> PageHostReply {
        self.runtime.close_realm(tab_id);
        self.debug_registry
            .prune_invalid(self.runtime.program_registry());
        self.documents.remove(&tab_id);
        host_failure()
    }

    fn close_realm(&mut self, tab_id: u64, document_generation: u64) -> PageHostReply {
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

    fn realm_stats(&self, tab_id: u64, document_generation: u64) -> PageHostReply {
        let Some(document) = self.documents.get(&tab_id) else {
            return unknown_realm();
        };
        if document.generation != document_generation {
            return stale_document();
        }
        match self.runtime.realm_stats(tab_id) {
            Ok(stats) => PageHostReply::RealmStats(PageHostRealmStats {
                tab_id,
                document_generation,
                program_count: u32::try_from(stats.program_count).unwrap_or(u32::MAX),
                bytecode_bytes: u64::try_from(stats.bytecode_bytes).unwrap_or(u64::MAX),
                heap_bytes: u64::try_from(stats.heap.managed_bytes).unwrap_or(u64::MAX),
            }),
            Err(_) => host_failure(),
        }
    }

    /// Lists only the child-minted private IDs for one exact realm. The core
    /// intentionally remaps these again before public debugger IPC sees them.
    fn debugger_programs(&self, tab_id: u64, document_generation: u64) -> PageHostReply {
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

    /// Enumerates the child-private static-BlueTS association for one exact
    /// currently live program. The association is intentionally minted lazily
    /// on this authenticated inventory request: program discovery itself does
    /// not imply static-metadata authority. The reply is a bounded handle list
    /// rather than a metadata payload, and it uses an identity namespace that
    /// is distinct from the child debugger program IDs.
    fn debugger_bluets_metadata(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    ) -> PageHostReply {
        if !program.is_well_formed() {
            return invalid_request();
        }
        let runtime_handle = {
            let document = match self.exact_document(tab_id, document_generation) {
                Ok(document) => document,
                Err(reply) => return reply,
            };
            let Some(record) = document.debugger_programs.get(&program.program_handle) else {
                return invalid_request();
            };
            if record.program_generation != program.program_generation {
                return invalid_request();
            }
            record.runtime_handle
        };

        // A normal JavaScript program and a BlueTS program whose attachment
        // has been pruned both have no metadata inventory. Do not mint a
        // negative-result handle; an empty bounded list reveals no static
        // count or compiler detail beyond this program's ineligibility.
        if self
            .debug_registry
            .get(self.runtime.program_registry(), runtime_handle)
            .is_err()
        {
            let document = self
                .documents
                .get_mut(&tab_id)
                .expect("the exact child document remains live after registry validation");
            document
                .debugger_programs
                .get_mut(&program.program_handle)
                .expect("the exact child program remains registered")
                .metadata = None;
            return PageHostReply::DebuggerBlueTsMetadata {
                tab_id,
                document_generation,
                program,
                metadata: Vec::new(),
            };
        }

        let metadata = {
            let existing = self
                .documents
                .get(&tab_id)
                .expect("the exact child document remains live after registry validation")
                .debugger_programs
                .get(&program.program_handle)
                .expect("the exact child program remains registered")
                .metadata;
            match existing {
                Some(metadata) => metadata,
                None => {
                    let metadata = match self.mint_debugger_metadata_handle() {
                        Ok(metadata) => metadata,
                        Err(()) => return host_failure(),
                    };
                    self.documents
                        .get_mut(&tab_id)
                        .expect("the exact child document remains live while metadata is minted")
                        .debugger_programs
                        .get_mut(&program.program_handle)
                        .expect(
                            "the exact child program remains registered while metadata is minted",
                        )
                        .metadata = Some(metadata);
                    metadata
                }
            }
        };
        PageHostReply::DebuggerBlueTsMetadata {
            tab_id,
            document_generation,
            program,
            metadata: vec![metadata],
        }
    }

    /// Returns the first deliberately narrow read surface for a previously
    /// inventoried BlueTS debug attachment. The handle must still be owned by
    /// this exact live program, so neither an arbitrary child-private ID nor
    /// a handle from a sibling program can probe the registry. The response
    /// contains fixed fingerprints and aggregate counts only; source text and
    /// identity, spans, names, type displays, symbols, contracts, bytecode,
    /// VM objects, and values remain inside this child.
    fn debugger_bluets_metadata_summary(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    ) -> PageHostReply {
        if !program.is_well_formed() || !metadata.is_well_formed() {
            return invalid_request();
        }
        let runtime_handle = {
            let document = match self.exact_document(tab_id, document_generation) {
                Ok(document) => document,
                Err(reply) => return reply,
            };
            let Some(record) = document.debugger_programs.get(&program.program_handle) else {
                return invalid_request();
            };
            if record.program_generation != program.program_generation
                || record.metadata != Some(metadata)
            {
                return invalid_request();
            }
            record.runtime_handle
        };

        let summary = match self
            .debug_registry
            .get(self.runtime.program_registry(), runtime_handle)
        {
            Ok(retained) => {
                let info = retained.static_info();
                let (Ok(source_count), Ok(type_count), Ok(symbol_count), Ok(contract_count)) = (
                    u32::try_from(info.sources.len()),
                    u32::try_from(info.types.len()),
                    u32::try_from(info.symbols.len()),
                    u32::try_from(info.contracts.len()),
                ) else {
                    return host_failure();
                };
                PageHostDebuggerBlueTsMetadataSummary {
                    language_version: info.language_version.clone(),
                    compiler_options_hash: info.compiler_options_hash.clone(),
                    source_count,
                    type_count,
                    symbol_count,
                    contract_count,
                }
            }
            Err(_) => {
                // A registry pruning race invalidates the private inventory
                // identity before reporting anything about the old record.
                self.documents
                    .get_mut(&tab_id)
                    .expect("the exact child document remains live after registry validation")
                    .debugger_programs
                    .get_mut(&program.program_handle)
                    .expect("the exact child program remains registered after registry validation")
                    .metadata = None;
                return invalid_request();
            }
        };
        PageHostReply::DebuggerBlueTsMetadataSummary {
            tab_id,
            document_generation,
            program,
            metadata,
            summary,
        }
    }

    fn debugger_safe_points(
        &self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    ) -> PageHostReply {
        if !program.is_well_formed() {
            return invalid_request();
        }
        let document = match self.exact_document(tab_id, document_generation) {
            Ok(document) => document,
            Err(reply) => return reply,
        };
        let Some(record) = document.debugger_programs.get(&program.program_handle) else {
            return invalid_request();
        };
        if record.program_generation != program.program_generation {
            return invalid_request();
        }
        let safe_points = match self.runtime.safe_points(
            tab_id,
            record.runtime_handle,
            usize::try_from(PAGE_HOST_DEBUGGER_MAX_SAFE_POINTS_PER_PROGRAM)
                .expect("page-host debugger safe-point cap fits usize"),
        ) {
            Ok(safe_points) => safe_points,
            Err(BlueJsPageRuntimeError::SafePointLimit { .. }) => return resource_limit(),
            Err(_) => return invalid_request(),
        };
        PageHostReply::DebuggerSafePoints {
            tab_id,
            document_generation,
            program,
            safe_points: safe_points
                .into_iter()
                .map(|safe_point| PageHostDebuggerSafePoint {
                    program,
                    code_unit_ordinal: safe_point.code_unit.ordinal(),
                    bytecode_offset: safe_point.bytecode_offset,
                })
                .collect(),
        }
    }

    fn validate_debugger_safe_point(
        &self,
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    ) -> PageHostReply {
        match self.exact_debugger_safe_point(tab_id, document_generation, safe_point) {
            Ok(()) => PageHostReply::DebuggerSafePointValidated {
                tab_id,
                document_generation,
                safe_point,
            },
            Err(reply) => reply,
        }
    }

    /// Revalidates an exact child-private safe-point tuple without returning
    /// a runtime handle, source, bytecode, or VM object to the caller.
    fn exact_debugger_safe_point(
        &self,
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    ) -> Result<(), PageHostReply> {
        if !safe_point.is_well_formed() {
            return Err(invalid_request());
        }
        let document = self.exact_document(tab_id, document_generation)?;
        let Some(record) = document
            .debugger_programs
            .get(&safe_point.program.program_handle)
        else {
            return Err(invalid_request());
        };
        if record.program_generation != safe_point.program.program_generation {
            return Err(invalid_request());
        }
        // Constructing a BlueJS safe point is intentionally not exposed by
        // its public runtime API. Find the exact compiler-recorded boundary
        // first, then ask the runtime to revalidate that authentic tuple.
        let found = match self.runtime.safe_points(
            tab_id,
            record.runtime_handle,
            usize::try_from(PAGE_HOST_DEBUGGER_MAX_SAFE_POINTS_PER_PROGRAM)
                .expect("page-host debugger safe-point cap fits usize"),
        ) {
            Ok(safe_points) => safe_points.into_iter().find(|candidate| {
                candidate.code_unit.ordinal() == safe_point.code_unit_ordinal
                    && candidate.bytecode_offset == safe_point.bytecode_offset
            }),
            Err(BlueJsPageRuntimeError::SafePointLimit { .. }) => return Err(resource_limit()),
            Err(_) => return Err(invalid_request()),
        };
        let Some(found) = found else {
            return Err(invalid_request());
        };
        if self
            .runtime
            .validate_safe_point(tab_id, record.runtime_handle, found)
            .is_err()
        {
            return Err(invalid_request());
        }
        Ok(())
    }

    fn set_debugger_breakpoint(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    ) -> PageHostReply {
        if let Err(reply) = self.exact_debugger_safe_point(tab_id, document_generation, safe_point)
        {
            return reply;
        }
        let document = self
            .documents
            .get_mut(&tab_id)
            .expect("the exact child document remains live after validation");
        let max_breakpoints = usize::try_from(PAGE_HOST_DEBUGGER_MAX_BREAKPOINTS_PER_REALM)
            .expect("page-host debugger breakpoint cap fits usize");
        if !document.debugger_breakpoints.contains(&safe_point)
            && document.debugger_breakpoints.len() == max_breakpoints
        {
            return resource_limit();
        }
        document.debugger_breakpoints.insert(safe_point);
        PageHostReply::DebuggerBreakpointSet {
            tab_id,
            document_generation,
            safe_point,
        }
    }

    fn debugger_breakpoints(&self, tab_id: u64, document_generation: u64) -> PageHostReply {
        let document = match self.exact_document(tab_id, document_generation) {
            Ok(document) => document,
            Err(reply) => return reply,
        };
        let max_breakpoints = usize::try_from(PAGE_HOST_DEBUGGER_MAX_BREAKPOINTS_PER_REALM)
            .expect("page-host debugger breakpoint cap fits usize");
        if document.debugger_breakpoints.len() > max_breakpoints {
            return resource_limit();
        }
        PageHostReply::DebuggerBreakpoints {
            tab_id,
            document_generation,
            safe_points: document.debugger_breakpoints.iter().copied().collect(),
        }
    }

    fn clear_debugger_breakpoint(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    ) -> PageHostReply {
        if let Err(reply) = self.exact_debugger_safe_point(tab_id, document_generation, safe_point)
        {
            return reply;
        }
        let was_present = self
            .documents
            .get_mut(&tab_id)
            .expect("the exact child document remains live after validation")
            .debugger_breakpoints
            .remove(&safe_point);
        PageHostReply::DebuggerBreakpointCleared {
            tab_id,
            document_generation,
            safe_point,
            was_present,
        }
    }

    fn arm_debugger_root_safe_point_breakpoint(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    ) -> PageHostReply {
        if safe_point.code_unit_ordinal != 0 {
            return invalid_debugger_state();
        }
        if let Err(reply) = self.exact_debugger_safe_point(tab_id, document_generation, safe_point)
        {
            return reply;
        }
        let Some(document) = self.documents.get_mut(&tab_id) else {
            return unknown_realm();
        };
        if !document.debugger_execution_control {
            return invalid_debugger_state();
        }
        if document.debugger_execution_states.get(&safe_point.program)
            != Some(&ChildDebuggerExecutionStatus::Pending)
        {
            return invalid_debugger_state();
        }
        let Some(pending) = document
            .pending_debugger_executions
            .iter_mut()
            .find(|pending| pending.program == Some(safe_point.program))
        else {
            return invalid_debugger_state();
        };
        let DeferredChildExecution::JavaScriptClassic {
            root_safe_point, ..
        } = &mut pending.execution
        else {
            return invalid_debugger_state();
        };
        if root_safe_point.is_some() {
            return invalid_debugger_state();
        }
        let max = usize::try_from(PAGE_HOST_DEBUGGER_MAX_BREAKPOINTS_PER_REALM)
            .expect("page-host debugger breakpoint cap fits usize");
        if !document.debugger_breakpoints.contains(&safe_point)
            && document.debugger_breakpoints.len() == max
        {
            return resource_limit();
        }
        document.debugger_breakpoints.insert(safe_point);
        *root_safe_point = Some(safe_point);
        PageHostReply::DebuggerRootSafePointBreakpointArmed {
            tab_id,
            document_generation,
            safe_point,
        }
    }

    fn debugger_execution_state(
        &self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    ) -> PageHostReply {
        let document = match self.exact_document(tab_id, document_generation) {
            Ok(document) => document,
            Err(reply) => return reply,
        };
        if !document.debugger_execution_control || !program.is_well_formed() {
            return invalid_debugger_state();
        }
        let Some(status) = document.debugger_execution_states.get(&program).copied() else {
            return invalid_debugger_state();
        };
        PageHostReply::DebuggerExecutionState {
            tab_id,
            document_generation,
            program,
            state: child_debugger_execution_state(status),
        }
    }

    fn resume_debugger_execution(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    ) -> PageHostReply {
        let document = match self.documents.get_mut(&tab_id) {
            Some(document) if document.generation == document_generation => document,
            Some(_) => return stale_document(),
            None => return unknown_realm(),
        };
        if !document.debugger_execution_control || !program.is_well_formed() {
            return invalid_debugger_state();
        }
        let Some(status) = document.debugger_execution_states.get_mut(&program) else {
            return invalid_debugger_state();
        };
        if !matches!(status, ChildDebuggerExecutionStatus::Paused(_)) {
            return invalid_debugger_state();
        }
        *status = ChildDebuggerExecutionStatus::ResumeRequested;
        PageHostReply::DebuggerExecutionResumed {
            tab_id,
            document_generation,
            program,
        }
    }

    fn advance_debugger_execution(
        &mut self,
        tab_id: u64,
        document_generation: u64,
    ) -> PageHostReply {
        let origin = match self.exact_document(tab_id, document_generation) {
            Ok(document) if document.debugger_execution_control => document.origin.clone(),
            Ok(_) => return invalid_debugger_state(),
            Err(reply) => return reply,
        };
        let mut reports = Vec::new();
        while let Some(mut pending) = self
            .documents
            .get_mut(&tab_id)
            .expect("the exact child document remains live while advancing")
            .pending_debugger_executions
            .pop_front()
        {
            let mut paused = false;
            let outcome = match &mut pending.execution {
                DeferredChildExecution::JavaScriptClassic {
                    handle,
                    root_safe_point,
                } => {
                    let program = pending
                        .program
                        .expect("every deferred classic has a private debugger program");
                    let status = self
                        .documents
                        .get(&tab_id)
                        .and_then(|document| document.debugger_execution_states.get(&program))
                        .copied()
                        .expect("every deferred classic has scheduler state");
                    match status {
                        ChildDebuggerExecutionStatus::Paused(_) => {
                            paused = true;
                            PageHostScriptOutcome::Executed
                        }
                        ChildDebuggerExecutionStatus::Pending
                        | ChildDebuggerExecutionStatus::ResumeRequested => {
                            let result = match (*root_safe_point, status) {
                                (Some(target), ChildDebuggerExecutionStatus::Pending) => self
                                    .runtime
                                    .execute_program_until_debugger_pause_at_root_offset(
                                        tab_id,
                                        *handle,
                                        target.bytecode_offset,
                                    )
                                    .map(|state| (state, true)),
                                (Some(_), ChildDebuggerExecutionStatus::ResumeRequested) => self
                                    .runtime
                                    .resume_debugger_execution(tab_id)
                                    .map(|state| (state, false)),
                                (None, _) => self
                                    .runtime
                                    .execute_program(tab_id, *handle)
                                    .map(|_| (BlueJsPageDebuggerExecutionState::Completed, false)),
                                _ => {
                                    unreachable!("only pending or resuming states reach execution")
                                }
                            };
                            match result {
                                Ok((BlueJsPageDebuggerExecutionState::Paused { .. }, true)) => {
                                    let target = root_safe_point
                                        .expect("a child pause has an armed exact root safe point");
                                    self.documents
                                        .get_mut(&tab_id)
                                        .expect("the advancing document remains live")
                                        .debugger_execution_states
                                        .insert(
                                            program,
                                            ChildDebuggerExecutionStatus::Paused(target),
                                        );
                                    paused = true;
                                    PageHostScriptOutcome::Executed
                                }
                                Ok((BlueJsPageDebuggerExecutionState::Completed, _)) => {
                                    self.documents
                                        .get_mut(&tab_id)
                                        .expect("the advancing document remains live")
                                        .debugger_execution_states
                                        .insert(program, ChildDebuggerExecutionStatus::Completed);
                                    PageHostScriptOutcome::Executed
                                }
                                Ok((BlueJsPageDebuggerExecutionState::Paused { .. }, false)) => {
                                    self.documents
                                        .get_mut(&tab_id)
                                        .expect("the advancing document remains live")
                                        .debugger_execution_states
                                        .insert(program, ChildDebuggerExecutionStatus::Completed);
                                    rejected("BlueJS debugger continuation did not complete after resume")
                                }
                                Err(error) => {
                                    self.documents
                                        .get_mut(&tab_id)
                                        .expect("the advancing document remains live")
                                        .debugger_execution_states
                                        .insert(program, ChildDebuggerExecutionStatus::Completed);
                                    rejected(page_runtime_category(error))
                                }
                            }
                        }
                        ChildDebuggerExecutionStatus::Completed => {
                            rejected("child debugger execution state was inconsistent")
                        }
                    }
                }
                DeferredChildExecution::JavaScriptModule { graph, programs } => {
                    execute_module_graph(
                        &mut self.runtime,
                        tab_id,
                        &origin,
                        graph.clone(),
                        programs.clone(),
                    )
                }
                DeferredChildExecution::BlueTsClassic { script } => execute_bluets_classic(
                    &mut self.runtime,
                    &mut self.debug_registry,
                    tab_id,
                    &origin,
                    script,
                ),
                DeferredChildExecution::BlueTsModule { graph } => execute_bluets_module_graph(
                    &mut self.runtime,
                    &mut self.debug_registry,
                    tab_id,
                    &origin,
                    graph,
                ),
            };
            if paused {
                self.documents
                    .get_mut(&tab_id)
                    .expect("the paused document remains live")
                    .pending_debugger_executions
                    .push_front(pending);
                break;
            }
            reports.push(script_report(
                tab_id,
                document_generation,
                pending.ordinal,
                pending.language,
                pending.kind,
                outcome,
            ));
        }
        if self.refresh_debugger_programs(tab_id).is_err() {
            return self.fail_debugger_execution_document(tab_id);
        }
        PageHostReply::DebuggerExecutionAdvanced {
            tab_id,
            document_generation,
            reports,
        }
    }

    fn exact_document(
        &self,
        tab_id: u64,
        document_generation: u64,
    ) -> Result<&LiveDocument, PageHostReply> {
        match self.documents.get(&tab_id) {
            None => Err(unknown_realm()),
            Some(document) if document.generation != document_generation => Err(stale_document()),
            Some(document) => Ok(document),
        }
    }

    fn refresh_debugger_programs(&mut self, tab_id: u64) -> Result<(), ()> {
        let runtime_handles = self.runtime.program_handles(tab_id).map_err(|_| ())?;
        for runtime_handle in runtime_handles {
            self.register_debugger_program(tab_id, runtime_handle)?;
        }
        Ok(())
    }

    /// Returns a stable child-private identity for one currently admitted
    /// runtime program. Refreshes never remint an existing handle: core's
    /// public mapping therefore remains bound to the exact private tuple.
    fn register_debugger_program(
        &mut self,
        tab_id: u64,
        runtime_handle: BlueJsProgramHandle,
    ) -> Result<PageHostDebuggerProgram, ()> {
        if let Some((program_handle, record)) = self
            .documents
            .get(&tab_id)
            .ok_or(())?
            .debugger_programs
            .iter()
            .find(|(_, record)| record.runtime_handle == runtime_handle)
        {
            return Ok(PageHostDebuggerProgram {
                program_handle: *program_handle,
                program_generation: record.program_generation,
            });
        }
        let program_handle = self.next_debugger_program_handle;
        let program_generation = self.next_debugger_program_generation;
        // Keep the numerical ranges disjoint for the process lifetime as a
        // defense in depth beyond the distinct wire types. A pathological
        // child that exhausts the lower program-ID space fails closed rather
        // than minting a value that could resemble a metadata handle.
        if program_handle >= CHILD_DEBUGGER_METADATA_ID_NAMESPACE_START
            || program_generation >= CHILD_DEBUGGER_METADATA_ID_NAMESPACE_START
        {
            return Err(());
        }
        self.next_debugger_program_handle = program_handle.checked_add(1).ok_or(())?;
        self.next_debugger_program_generation = program_generation.checked_add(1).ok_or(())?;
        self.documents
            .get_mut(&tab_id)
            .ok_or(())?
            .debugger_programs
            .insert(
                program_handle,
                ChildDebuggerProgram {
                    program_generation,
                    runtime_handle,
                    metadata: None,
                },
            );
        Ok(PageHostDebuggerProgram {
            program_handle,
            program_generation,
        })
    }

    /// Mints an inventory-only child metadata identity. Callers must first
    /// prove the matching registry attachment remains live; this helper never
    /// receives or derives compiler metadata.
    fn mint_debugger_metadata_handle(&mut self) -> Result<PageHostDebuggerMetadataHandle, ()> {
        let metadata_handle = self.next_debugger_metadata_handle;
        let metadata_generation = self.next_debugger_metadata_generation;
        self.next_debugger_metadata_handle = metadata_handle.checked_add(1).ok_or(())?;
        self.next_debugger_metadata_generation = metadata_generation.checked_add(1).ok_or(())?;
        Ok(PageHostDebuggerMetadataHandle {
            metadata_handle,
            metadata_generation,
        })
    }
}

impl Default for BlueJsChildHost {
    fn default() -> Self {
        Self::new().expect("the default BlueJS child-host configuration is valid")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DocumentSnapshotError {
    Invalid,
    ResourceLimit,
}

/// Repeats the fixed core-side snapshot checks before a child VM observes a
/// value. Parsing this already-serialized string does not give the child a
/// URL object, resolver, or network operation: `canonical_http_origin` is a
/// pure syntax/canonicalization check and this host never calls `fetch`.
fn validated_document_origin(
    snapshot: &PageHostDocumentSnapshot,
) -> Result<BlueJsPageOrigin, DocumentSnapshotError> {
    if snapshot.document_text.len() > PAGE_HOST_DOCUMENT_TEXT_MAX_BYTES
        || snapshot.document_origin.len() > PAGE_HOST_DOCUMENT_ORIGIN_MAX_BYTES
    {
        return Err(DocumentSnapshotError::ResourceLimit);
    }
    let canonical = canonical_http_origin(&snapshot.document_origin)
        .map_err(|_| DocumentSnapshotError::Invalid)?;
    if canonical != snapshot.document_origin {
        return Err(DocumentSnapshotError::Invalid);
    }
    BlueJsPageOrigin::new(canonical).map_err(|_| DocumentSnapshotError::Invalid)
}

/// Installs exactly the two core-selected immutable JavaScript callbacks. The
/// shared generated BlueTS artifact verifies this same inventory before it can
/// become an ambient module for a child compilation. The registrar deliberately
/// exposes no VM operation, DOM node, resolver, URL, fetch, IPC, or object
/// handle. Captured Rust strings are copied snapshots and page arguments are
/// rejected before a callback can return either one.
fn install_document_snapshot_bindings(
    runtime: &mut BlueJsPageRuntime,
    tab_id: u64,
    snapshot: &PageHostDocumentSnapshot,
) -> Result<(), BlueJsPageRuntimeError> {
    let artifact = PageHostDocumentTypingsV1::generate();
    let binding_inventory = page_host_document_runtime_bindings_v1();
    artifact
        .verify_runtime_bindings(&binding_inventory)
        .map_err(|_| BlueJsPageRuntimeError::InvalidConfiguration)?;
    let document_text = snapshot.document_text.clone();
    let document_origin = snapshot.document_origin.clone();
    runtime.configure_realm_bindings(tab_id, move |bindings| {
        for binding in binding_inventory {
            match (binding.stable_id, binding.runtime_binding_id) {
                ("dom.document-origin", "global.blueiceDocumentOrigin") => {
                    let document_origin = document_origin.clone();
                    bindings.install_global_function(
                        "blueiceDocumentOrigin",
                        0,
                        move |arguments: &[HostValue]| {
                            require_no_arguments(arguments, "blueiceDocumentOrigin")?;
                            Ok(HostValue::String(document_origin.clone().into()))
                        },
                    )?;
                }
                ("dom.document-text", "global.blueiceDocumentText") => {
                    let document_text = document_text.clone();
                    bindings.install_global_function(
                        "blueiceDocumentText",
                        0,
                        move |arguments: &[HostValue]| {
                            require_no_arguments(arguments, "blueiceDocumentText")?;
                            Ok(HostValue::String(document_text.clone().into()))
                        },
                    )?;
                }
                _ => {
                    return Err(RuntimeError::Unsupported(
                        "page-host typing inventory has no callback installer",
                    ));
                }
            }
        }
        Ok(())
    })
}

fn require_no_arguments(arguments: &[HostValue], function: &str) -> Result<(), HostFunctionError> {
    if arguments.is_empty() {
        Ok(())
    } else {
        Err(HostFunctionError::new(format!(
            "{function} requires no arguments"
        )))
    }
}

enum PreparedScript {
    Rejected {
        ordinal: u32,
        language: PageHostScriptLanguage,
        kind: PageHostScriptKind,
        category: &'static str,
    },
    JavaScriptClassic {
        ordinal: u32,
        source: PageHostSource,
        program: BlueJsProgramV1,
    },
    JavaScriptModule {
        ordinal: u32,
        graph: PageHostModuleGraph,
        programs: BTreeMap<String, BlueJsProgramV1>,
    },
    BlueTsClassic {
        ordinal: u32,
        script: Box<DirectScript>,
    },
    BlueTsModule {
        ordinal: u32,
        graph: Box<DirectModuleGraph>,
    },
}

fn prepare_script(script: PageHostScript) -> PreparedScript {
    let ordinal = script.ordinal;
    let language = script.language;
    let kind = script.kind;
    let prepared = match (language, kind) {
        (PageHostScriptLanguage::JavaScript, PageHostScriptKind::Classic) => {
            prepare_classic(script.graph).map(|(source, program)| {
                PreparedScript::JavaScriptClassic {
                    ordinal,
                    source,
                    program,
                }
            })
        }
        (PageHostScriptLanguage::JavaScript, PageHostScriptKind::Module) => {
            prepare_module_graph(script.graph).map(|(graph, programs)| {
                PreparedScript::JavaScriptModule {
                    ordinal,
                    graph,
                    programs,
                }
            })
        }
        (PageHostScriptLanguage::BlueTs, PageHostScriptKind::Classic) => {
            prepare_bluets_classic(script.graph).map(|script| PreparedScript::BlueTsClassic {
                ordinal,
                script: Box::new(script),
            })
        }
        (PageHostScriptLanguage::BlueTs, PageHostScriptKind::Module) => {
            prepare_bluets_module_graph(script.graph).map(|graph| PreparedScript::BlueTsModule {
                ordinal,
                graph: Box::new(graph),
            })
        }
    };
    prepared.unwrap_or_else(|category| PreparedScript::Rejected {
        ordinal,
        language,
        kind,
        category,
    })
}

fn prepare_classic(
    graph: PageHostModuleGraph,
) -> Result<(PageHostSource, BlueJsProgramV1), &'static str> {
    let modules = validate_graph(&graph)?;
    if modules.len() != 1 || !graph.resolutions.is_empty() {
        return Err("classic JavaScript source graph is not closed");
    }
    let source = modules
        .get(&graph.entry)
        .cloned()
        .ok_or("authorized JavaScript graph has no entry")?;
    let program = BlueJsProgramV1::Script(parse(&source.source).map_err(parse_category)?);
    program.compile().map_err(compile_category)?;
    Ok((source, program))
}

fn prepare_module_graph(
    graph: PageHostModuleGraph,
) -> Result<(PageHostModuleGraph, BTreeMap<String, BlueJsProgramV1>), &'static str> {
    let modules = validate_graph(&graph)?;
    let resolutions = validate_resolutions(&graph, &modules)?;
    let mut programs = BTreeMap::new();
    for (module_id, source) in &modules {
        let mut module = parse_module(&source.source).map_err(parse_category)?;
        rewrite_static_module_requests(module_id, &mut module, &resolutions)?;
        let program = BlueJsProgramV1::Module(module);
        // Preflight the full graph before the first program is admitted.
        program.compile().map_err(compile_category)?;
        programs.insert(module_id.clone(), program);
    }
    Ok((graph, programs))
}

/// Prepares an explicit BlueTS classic declaration through the same closed
/// caller-authorized graph validation used for JavaScript. The compiler sees
/// no filesystem, URL, import-map, page-selected profile, or callback
/// authority. Its only ambient declaration comes from the shared generated
/// artifact for the two child-fixed copied snapshot callbacks.
fn prepare_bluets_classic(graph: PageHostModuleGraph) -> Result<DirectScript, &'static str> {
    let modules = validate_graph(&graph)?;
    if modules.len() != 1 || !graph.resolutions.is_empty() {
        return Err("classic BlueTS source graph is not closed");
    }
    let loader = bluets_loader(&graph, modules)?;
    compile_direct_script(&graph.entry, &loader, bluets_compiler_options(&graph)?)
        .map_err(bluets_bridge_category)
}

/// Prepares a complete explicit BlueTS module graph without giving BlueTS a
/// resolver beyond the exact static edges serialized by its caller.
fn prepare_bluets_module_graph(
    graph: PageHostModuleGraph,
) -> Result<DirectModuleGraph, &'static str> {
    let modules = validate_graph(&graph)?;
    validate_resolutions(&graph, &modules)?;
    let loader = bluets_loader(&graph, modules)?;
    compile_direct_module_graph(&graph.entry, &loader, bluets_compiler_options(&graph)?)
        .map_err(bluets_bridge_category)
}

fn bluets_loader(
    graph: &PageHostModuleGraph,
    modules: BTreeMap<String, PageHostSource>,
) -> Result<AuthorizedModuleLoader, &'static str> {
    let resolutions = graph.resolutions.iter().map(|resolution| {
        AuthorizedModuleResolution::new(
            resolution.from_module.clone(),
            resolution.specifier.clone(),
            resolution.canonical_target.clone(),
        )
    });
    AuthorizedModuleLoader::new(
        modules
            .into_values()
            .map(|source| AuthorizedModule::new(source.canonical_module_id, source.source)),
        resolutions,
    )
    .map_err(|_| "authorized BlueTS graph is invalid")
}

fn bluets_compiler_options(graph: &PageHostModuleGraph) -> Result<CompilerOptions, &'static str> {
    let artifact = PageHostDocumentTypingsV1::generate();
    let ambient_declaration = artifact
        .verified_ambient_module(&page_host_document_runtime_bindings_v1())
        .map_err(|_| "verified page-host BlueTS typings are unavailable")?;
    let mut options = CompilerOptions {
        runtime_policy: RuntimePolicy::Checked,
        resolver_fingerprint: graph.resolver_fingerprint.clone(),
        require_declared_global_calls: true,
        ambient_declaration_modules: vec![ambient_declaration],
        ..CompilerOptions::default()
    };
    // The transport and BlueJS child already use the smaller page-host source
    // limits. Carry them into BlueTS too so a direct compilation cannot do
    // substantially more work than the closed graph the child admitted.
    options.limits.max_modules = MAX_MODULES_PER_GRAPH;
    options.limits.max_total_source_bytes = MAX_SOURCE_BYTES_PER_DOCUMENT;
    Ok(options)
}

fn execute_bluets_classic(
    runtime: &mut BlueJsPageRuntime,
    debug_registry: &mut DirectDebugRegistry,
    tab_id: u64,
    origin: &BlueJsPageOrigin,
    script: &DirectScript,
) -> PageHostScriptOutcome {
    let attachment =
        match script.attach_debug_in_page_realm(runtime, tab_id, origin, debug_registry) {
            Ok(attachment) => attachment,
            Err(error) => return rejected(bluets_bridge_category(error)),
        };
    match runtime
        .execute_program(tab_id, attachment.handle)
        .map(|_: Value| ())
    {
        Ok(()) => PageHostScriptOutcome::Executed,
        Err(error) => rejected(page_runtime_category(error)),
    }
}

fn execute_bluets_module_graph(
    runtime: &mut BlueJsPageRuntime,
    debug_registry: &mut DirectDebugRegistry,
    tab_id: u64,
    origin: &BlueJsPageOrigin,
    graph: &DirectModuleGraph,
) -> PageHostScriptOutcome {
    let attachment = match graph.attach_debug_in_page_realm(runtime, tab_id, origin, debug_registry)
    {
        Ok(attachment) => attachment,
        Err(error) => return rejected(bluets_bridge_category(error)),
    };
    match runtime.execute_module_graph(
        tab_id,
        attachment.entry.handle,
        attachment.modules.values().map(|module| module.handle),
    ) {
        Ok(_) => PageHostScriptOutcome::Executed,
        Err(error) => rejected(page_runtime_category(error)),
    }
}

fn validate_graph(
    graph: &PageHostModuleGraph,
) -> Result<BTreeMap<String, PageHostSource>, &'static str> {
    if graph.entry.is_empty()
        || graph.entry.contains('\0')
        || graph.resolver_fingerprint.trim().is_empty()
        || graph.resolver_fingerprint.contains('\0')
    {
        return Err("authorized JavaScript graph is invalid");
    }
    if graph.modules.len() > MAX_MODULES_PER_GRAPH {
        return Err("JavaScript module graph exceeds configured policy");
    }
    let mut modules = BTreeMap::new();
    for source in &graph.modules {
        if source.canonical_module_id.is_empty()
            || source.canonical_module_id.contains('\0')
            || source.source.len() > MAX_SOURCE_BYTES_PER_MODULE
            || source.source_hash != page_host::source_hash(&source.source)
        {
            return Err("authorized JavaScript source record is invalid");
        }
        if modules
            .insert(source.canonical_module_id.clone(), source.clone())
            .is_some()
        {
            return Err("authorized JavaScript graph has duplicate modules");
        }
    }
    if !modules.contains_key(&graph.entry) {
        return Err("authorized JavaScript graph has no entry");
    }
    Ok(modules)
}

fn validate_resolutions(
    graph: &PageHostModuleGraph,
    modules: &BTreeMap<String, PageHostSource>,
) -> Result<BTreeMap<(String, String), String>, &'static str> {
    let mut resolutions = BTreeMap::new();
    for PageHostStaticResolution {
        from_module,
        specifier,
        canonical_target,
    } in &graph.resolutions
    {
        if from_module.is_empty()
            || specifier.is_empty()
            || canonical_target.is_empty()
            || from_module.contains('\0')
            || specifier.contains('\0')
            || canonical_target.contains('\0')
            || !modules.contains_key(from_module)
            || !modules.contains_key(canonical_target)
        {
            return Err("authorized JavaScript resolution record is invalid");
        }
        if resolutions
            .insert(
                (from_module.clone(), specifier.clone()),
                canonical_target.clone(),
            )
            .is_some()
        {
            return Err("authorized JavaScript graph has duplicate static resolutions");
        }
    }
    Ok(resolutions)
}

fn rewrite_static_module_requests(
    module_id: &str,
    module: &mut Module,
    resolutions: &BTreeMap<(String, String), String>,
) -> Result<(), &'static str> {
    let resolve = |specifier: &str| {
        resolutions
            .get(&(module_id.to_string(), specifier.to_string()))
            .cloned()
            .ok_or("authorized JavaScript graph is missing a static resolution")
    };
    for import in &mut module.imports {
        import.module_request = resolve(&import.module_request)?;
    }
    for export in &mut module.exports {
        match export {
            blueice_bluejs::ExportEntry::Indirect { module_request, .. }
            | blueice_bluejs::ExportEntry::Star { module_request, .. }
            | blueice_bluejs::ExportEntry::Namespace { module_request, .. } => {
                *module_request = resolve(module_request)?;
            }
            blueice_bluejs::ExportEntry::Local { .. } => {}
        }
    }
    for request in &mut module.requests {
        request.specifier = resolve(&request.specifier)?;
    }
    Ok(())
}

fn execute_classic(
    runtime: &mut BlueJsPageRuntime,
    tab_id: u64,
    origin: &BlueJsPageOrigin,
    source: PageHostSource,
    program: BlueJsProgramV1,
) -> PageHostScriptOutcome {
    let source = match source_identity(&source) {
        Ok(source) => source,
        Err(category) => return rejected(category),
    };
    let handle = match runtime.install_program(tab_id, origin, source, &program) {
        Ok(handle) => handle,
        Err(error) => return rejected(page_runtime_category(error)),
    };
    match runtime.execute_program(tab_id, handle).map(|_: Value| ()) {
        Ok(()) => PageHostScriptOutcome::Executed,
        Err(error) => rejected(page_runtime_category(error)),
    }
}

fn execute_module_graph(
    runtime: &mut BlueJsPageRuntime,
    tab_id: u64,
    origin: &BlueJsPageOrigin,
    graph: PageHostModuleGraph,
    programs: BTreeMap<String, BlueJsProgramV1>,
) -> PageHostScriptOutcome {
    let modules = match validate_graph(&graph) {
        Ok(modules) => modules,
        Err(category) => return rejected(category),
    };
    let mut installed = Vec::with_capacity(programs.len());
    for (module_id, program) in &programs {
        let source = modules
            .get(module_id)
            .expect("prepared module programs derive from exactly this graph");
        let identity = match source_identity(source) {
            Ok(identity) => identity,
            Err(category) => {
                discard_programs(runtime, tab_id, &installed);
                return rejected(category);
            }
        };
        match runtime.install_program(tab_id, origin, identity, program) {
            Ok(handle) => installed.push(handle),
            Err(error) => {
                discard_programs(runtime, tab_id, &installed);
                return rejected(page_runtime_category(error));
            }
        }
    }
    let entry = programs
        .keys()
        .position(|module_id| module_id == &graph.entry)
        .and_then(|index| installed.get(index).copied())
        .expect("prepared graph has an installed entry module");
    match runtime.execute_module_graph(tab_id, entry, installed) {
        Ok(_) => PageHostScriptOutcome::Executed,
        Err(error) => rejected(page_runtime_category(error)),
    }
}

fn source_identity(source: &PageHostSource) -> Result<BlueJsSourceIdentity, &'static str> {
    BlueJsSourceIdentity::new(&source.canonical_module_id, &source.source_hash)
        .map_err(|_| "authorized JavaScript source record is invalid")
}

fn discard_programs(runtime: &mut BlueJsPageRuntime, tab_id: u64, handles: &[BlueJsProgramHandle]) {
    for handle in handles.iter().rev().copied() {
        let _ = runtime.discard_program(tab_id, handle);
    }
}

fn parse_category(_: ParseError) -> &'static str {
    "JavaScript parsing rejected the page script"
}

fn compile_category(_: CompileError) -> &'static str {
    "BlueJS compilation rejected the page script"
}

fn bluets_bridge_category(error: BridgeError) -> &'static str {
    match error {
        BridgeError::BlueTs(_) => "BlueTS compilation rejected the page script",
        BridgeError::PageRuntime(error) => page_runtime_category(error),
        BridgeError::BlueJs(_) | BridgeError::BlueJsDebug(_) => {
            "BlueJS compilation rejected the direct BlueTS page script"
        }
        BridgeError::UnsupportedRuntimeTarget { .. }
        | BridgeError::InvalidSourceIdentity(_)
        | BridgeError::ProvenanceAttachment(_)
        | BridgeError::DebugAttachment(_) => "BlueTS direct lowering rejected the page script",
    }
}

fn page_runtime_category(error: BlueJsPageRuntimeError) -> &'static str {
    match error {
        BlueJsPageRuntimeError::BytecodeLimit { .. }
        | BlueJsPageRuntimeError::ProgramLimit { .. }
        | BlueJsPageRuntimeError::RealmLimit { .. } => {
            "JavaScript page resource policy rejected the page script"
        }
        BlueJsPageRuntimeError::Runtime(RuntimeError::ModuleResolution(_)) => {
            "authorized JavaScript graph rejected the page script"
        }
        BlueJsPageRuntimeError::Runtime(_) => "BlueJS page execution failed",
        _ => "BlueJS page host rejected the page script",
    }
}

fn rejected(category: &'static str) -> PageHostScriptOutcome {
    PageHostScriptOutcome::Rejected {
        category: category.to_string(),
    }
}

fn script_report(
    tab_id: u64,
    document_generation: u64,
    ordinal: u32,
    language: PageHostScriptLanguage,
    kind: PageHostScriptKind,
    outcome: PageHostScriptOutcome,
) -> PageHostScriptReport {
    PageHostScriptReport {
        tab_id,
        document_generation,
        ordinal,
        language,
        kind,
        outcome,
    }
}

fn child_debugger_execution_state(
    status: ChildDebuggerExecutionStatus,
) -> PageHostDebuggerExecutionState {
    match status {
        ChildDebuggerExecutionStatus::Pending => PageHostDebuggerExecutionState::Pending,
        ChildDebuggerExecutionStatus::Paused(safe_point) => {
            PageHostDebuggerExecutionState::Paused { safe_point }
        }
        ChildDebuggerExecutionStatus::ResumeRequested => PageHostDebuggerExecutionState::Resuming,
        ChildDebuggerExecutionStatus::Completed => PageHostDebuggerExecutionState::Completed,
    }
}

fn invalid_request() -> PageHostReply {
    PageHostReply::Error {
        code: PageHostErrorCode::InvalidRequest,
        message: "BlueJS page-host request is invalid".to_string(),
    }
}

fn invalid_debugger_state() -> PageHostReply {
    PageHostReply::Error {
        code: PageHostErrorCode::InvalidDebuggerState,
        message: "BlueJS page-host debugger execution state is invalid".to_string(),
    }
}

fn stale_document() -> PageHostReply {
    PageHostReply::Error {
        code: PageHostErrorCode::StaleDocument,
        message: "BlueJS page-host document generation is stale".to_string(),
    }
}

fn unknown_realm() -> PageHostReply {
    PageHostReply::Error {
        code: PageHostErrorCode::UnknownRealm,
        message: "BlueJS page-host realm is unavailable".to_string(),
    }
}

fn resource_limit() -> PageHostReply {
    PageHostReply::Error {
        code: PageHostErrorCode::ResourceLimit,
        message: "BlueJS page-host request exceeds configured policy".to_string(),
    }
}

fn host_failure() -> PageHostReply {
    PageHostReply::Error {
        code: PageHostErrorCode::HostFailure,
        message: "BlueJS page-host could not activate the requested realm".to_string(),
    }
}

/// Serves one launcher-owned connection. The first decoded request must prove
/// possession of the per-spawn session capability before it can affect a
/// realm. A disconnect ends only this peer; the launcher owns restart policy.
pub fn serve_bluejs_host_connection(
    mut stream: UnixStream,
    session_token: &str,
    host: &mut BlueJsChildHost,
) -> io::Result<bool> {
    let first = page_host::read_page_host_request(&mut stream)?;
    let accepted = matches!(
        page_host::negotiate(&first, session_token),
        PageHostReply::HelloAck { .. }
    );
    let reply = page_host::negotiate(&first, session_token);
    page_host::write_page_host_reply(&mut stream, &reply)?;
    if !accepted {
        return Ok(false);
    }

    loop {
        let request = match page_host::read_page_host_request(&mut stream) {
            Ok(request) => request,
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(false),
            Err(error) => return Err(error),
        };
        let shutting_down = matches!(request, PageHostRequest::Shutdown);
        let reply = host.handle_request(request);
        page_host::write_page_host_reply(&mut stream, &reply)?;
        if shutting_down {
            return Ok(true);
        }
    }
}

/// Runs the child listener until its authenticated launcher asks it to shut
/// down. The listener is bound only at a launcher-created `0600` path and an
/// attacker cannot use a competing same-user connection without the session
/// token. A rejected peer cannot terminate the child or consume a realm.
pub fn serve_bluejs_host_listener(
    listener: UnixListener,
    session_token: String,
    host: &mut BlueJsChildHost,
) -> io::Result<()> {
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => match serve_bluejs_host_connection(stream, &session_token, host) {
                Ok(true) => return Ok(()),
                Ok(false) => continue,
                Err(_) => continue,
            },
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// Binds the child socket with owner-only filesystem permissions. The secret
/// session token remains mandatory because pathname permission alone does not
/// distinguish another process of the same user.
pub fn bind_bluejs_host_socket(path: &Path) -> io::Result<UnixListener> {
    let _ = fs::remove_file(path);
    let listener = UnixListener::bind(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

/// Private child-connection material a launcher may hand only to the core it
/// is supervising.
///
/// This is deliberately not a frontend setting or a page-visible capability.
/// A caller using [`SpawnedBlueJsHost::spawn_for_core`] must pass it to a
/// trusted core startup boundary and keep the returned supervisor alive for
/// at least as long as that core. It has no fetch, URL, DOM, or VM authority.
pub struct BlueJsHostCoreConfig {
    socket_path: PathBuf,
    session_token: String,
}

impl BlueJsHostCoreConfig {
    /// The owner-only socket created for this one child.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// The per-spawn capability needed by the trusted core's v1 handshake.
    /// Callers must not forward it to frontend/page code or log it.
    pub fn session_token(&self) -> &str {
        &self.session_token
    }
}

/// A launcher-owned child and, for the legacy direct-launcher path, its one
/// authenticated private connection. Dropping this handle kills/reaps the
/// process and removes the socket, matching [`crate::SpawnedCore`]'s
/// ownership discipline. [`Self::spawn_for_core`] instead delegates the sole
/// connection to a separately spawned trusted core while retaining process
/// supervision here.
pub struct SpawnedBlueJsHost {
    child: Child,
    socket_path: PathBuf,
    session_token: String,
    stream: Option<UnixStream>,
}

impl SpawnedBlueJsHost {
    /// Spawns the sibling `blueice-bluejs-host` binary, waits for its private
    /// socket, then performs the authenticated v1 handshake before returning
    /// a usable handle. A startup failure always reaps the child and removes
    /// the private socket.
    pub fn spawn() -> io::Result<Self> {
        Self::spawn_with_runtime_limits(BlueJsHostRuntimeLimits::default())
    }

    /// Spawns an isolated child under one owner-selected immutable per-realm
    /// envelope. This is an embedding/launcher construction API, not a
    /// page-host protocol capability and not a child-wide RSS limit.
    pub fn spawn_with_runtime_limits(limits: BlueJsHostRuntimeLimits) -> io::Result<Self> {
        let mut host = Self::spawn_unconnected(limits)?;
        if let Err(error) = host.connect_as_launcher() {
            host.reap_after_shutdown();
            return Err(error);
        }
        Ok(host)
    }

    /// Spawns and supervises a child whose sole authenticated connection will
    /// belong to a trusted core. The returned configuration contains the
    /// one-time capability and must be conveyed through a launcher-owned
    /// startup boundary, never from a page or frontend request.
    ///
    /// This does not alter the normal `blueice-launcher` command path. It is
    /// the narrow lifecycle hand-off used by the explicitly opted-in core
    /// adapter; the caller retains this supervisor until core exits.
    pub fn spawn_for_core() -> io::Result<(Self, BlueJsHostCoreConfig)> {
        Self::spawn_for_core_with_runtime_limits(BlueJsHostRuntimeLimits::default())
    }

    /// Equivalent to [`Self::spawn_for_core`], with an immutable launcher
    /// owner-selected per-realm envelope supplied to this one child before it
    /// binds its private socket. The delegated core receives neither the
    /// limits nor an operation to change them.
    pub fn spawn_for_core_with_runtime_limits(
        limits: BlueJsHostRuntimeLimits,
    ) -> io::Result<(Self, BlueJsHostCoreConfig)> {
        let host = Self::spawn_unconnected(limits)?;
        let config = BlueJsHostCoreConfig {
            socket_path: host.socket_path.clone(),
            session_token: host.session_token.clone(),
        };
        Ok((host, config))
    }

    fn spawn_unconnected(limits: BlueJsHostRuntimeLimits) -> io::Result<Self> {
        limits
            .runtime_config()
            .map_err(|message| io::Error::new(io::ErrorKind::InvalidInput, message))?;
        let this_exe = std::env::current_exe()?;
        let binary = sibling_bluejs_host_binary(&this_exe);
        let socket_path = unique_bluejs_host_socket_path();
        let token = secure_session_token()?;
        let _ = fs::remove_file(&socket_path);
        let mut child = Command::new(&binary)
            .arg("--socket")
            .arg(&socket_path)
            .arg("--session-token")
            .arg(&token)
            .arg("--max-realms")
            .arg(limits.max_realms.to_string())
            .arg("--max-programs-per-realm")
            .arg(limits.max_programs_per_realm.to_string())
            .arg("--max-bytecode-bytes-per-realm")
            .arg(limits.max_bytecode_bytes_per_realm.to_string())
            .arg("--max-heap-bytes-per-realm")
            .arg(limits.max_heap_bytes_per_realm.to_string())
            .spawn()?;

        let started = wait_for_child_socket(&mut child, &socket_path, STARTUP_TIMEOUT);
        if let Err(error) = started {
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_file(&socket_path);
            return Err(error);
        }
        Ok(Self {
            child,
            socket_path,
            session_token: token,
            stream: None,
        })
    }

    fn connect_as_launcher(&mut self) -> io::Result<()> {
        let mut stream = UnixStream::connect(&self.socket_path)?;
        let hello = PageHostRequest::Hello {
            protocol_version: page_host::PAGE_HOST_PROTOCOL_VERSION,
            session_token: self.session_token.clone(),
        };
        let handshake = (|| -> io::Result<PageHostReply> {
            page_host::write_page_host_request(&mut stream, &hello)?;
            page_host::read_page_host_reply(&mut stream)
        })();
        match handshake {
            Ok(PageHostReply::HelloAck {
                protocol_version: page_host::PAGE_HOST_PROTOCOL_VERSION,
            }) => {
                self.stream = Some(stream);
                Ok(())
            }
            Ok(reply) => Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("BlueJS page host rejected launcher handshake: {reply:?}"),
            )),
            Err(error) => Err(error),
        }
    }

    /// Sends one post-handshake request across the private child connection.
    /// This intentionally exposes only typed, source-free protocol values,
    /// never the child VM or its program registry.
    pub fn request(&mut self, request: PageHostRequest) -> io::Result<PageHostReply> {
        if matches!(request, PageHostRequest::Hello { .. }) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "BlueJS page-host handshake is already complete",
            ));
        }
        let stream = self.stream.as_mut().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "BlueJS page-host connection is delegated to the trusted core",
            )
        })?;
        page_host::write_page_host_request(stream, &request)?;
        page_host::read_page_host_reply(stream)
    }

    /// Applies one caller-authorized document to the isolated child.
    pub fn synchronize_document(
        &mut self,
        document: PageHostDocument,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::SynchronizeDocument { document })
    }

    /// Requests clean child shutdown, then reaps the child. `Drop` provides a
    /// hard-kill fallback when a process is hung or the transport is broken.
    pub fn shutdown(&mut self) -> io::Result<()> {
        let reply = self.request(PageHostRequest::Shutdown)?;
        if reply != PageHostReply::ShutdownAck {
            return Err(io::Error::other("BlueJS page host rejected shutdown"));
        }
        self.reap_after_shutdown();
        Ok(())
    }

    /// The private path is observable only for lifecycle tests and launcher
    /// cleanup diagnostics; callers must not use it as a second connection.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    fn reap_after_shutdown(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                Ok(None) | Err(_) => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    break;
                }
            }
        }
        let _ = fs::remove_file(&self.socket_path);
    }
}

impl Drop for SpawnedBlueJsHost {
    fn drop(&mut self) {
        self.reap_after_shutdown();
    }
}

fn sibling_bluejs_host_binary(this_exe: &Path) -> PathBuf {
    let name = if cfg!(windows) {
        "blueice-bluejs-host.exe"
    } else {
        "blueice-bluejs-host"
    };
    let directory = this_exe.parent().unwrap_or_else(|| Path::new("."));
    let directory = if directory.file_name().is_some_and(|name| name == "deps") {
        directory.parent().unwrap_or(directory)
    } else {
        directory
    };
    directory.join(name)
}

fn unique_bluejs_host_socket_path() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "blueice-launcher-bluejs-host-{}-{count}.sock",
        std::process::id()
    ))
}

fn secure_session_token() -> io::Result<String> {
    use std::io::Read;

    let mut bytes = [0u8; 32];
    fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    let mut token = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(token, "{byte:02x}").expect("writing to a String cannot fail");
    }
    Ok(token)
}

fn wait_for_child_socket(child: &mut Child, path: &Path, timeout: Duration) -> io::Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        if path.exists() {
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!(
                "BlueJS page-host child exited before binding its socket: {status}"
            )));
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "BlueJS page-host child never created its socket at {}",
                    path.display()
                ),
            ));
        }
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(entry: &str, modules: Vec<PageHostSource>) -> PageHostModuleGraph {
        PageHostModuleGraph {
            entry: entry.to_string(),
            modules,
            resolutions: Vec::new(),
            resolver_fingerprint: "core-page-loader-v1".to_string(),
        }
    }

    fn document(generation: u64, scripts: Vec<PageHostScript>) -> PageHostDocument {
        document_with_snapshot(
            generation,
            "test document snapshot".to_string(),
            "https://example.test".to_string(),
            scripts,
        )
    }

    fn document_with_snapshot(
        generation: u64,
        document_text: String,
        document_origin: String,
        scripts: Vec<PageHostScript>,
    ) -> PageHostDocument {
        PageHostDocument {
            tab_id: 7,
            document_generation: generation,
            snapshot: PageHostDocumentSnapshot {
                document_text,
                document_origin,
            },
            debugger_execution_control: false,
            scripts,
        }
    }

    fn debugger_document(generation: u64, scripts: Vec<PageHostScript>) -> PageHostDocument {
        let mut document = document(generation, scripts);
        document.debugger_execution_control = true;
        document
    }

    fn classic(ordinal: u32, source: &str) -> PageHostScript {
        let id = format!("blueice://page/inline-{ordinal}.js");
        PageHostScript {
            ordinal,
            language: PageHostScriptLanguage::JavaScript,
            kind: PageHostScriptKind::Classic,
            graph: graph(&id, vec![PageHostSource::new(id.clone(), source)]),
        }
    }

    fn blue_ts_classic(ordinal: u32, source: &str) -> PageHostScript {
        let id = format!("blueice://page/inline-{ordinal}.ts");
        PageHostScript {
            ordinal,
            language: PageHostScriptLanguage::BlueTs,
            kind: PageHostScriptKind::Classic,
            graph: graph(&id, vec![PageHostSource::new(id.clone(), source)]),
        }
    }

    #[test]
    fn executes_authorized_classic_and_rejects_a_later_parse_failure_independently() {
        let mut host = BlueJsChildHost::default();
        let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(
                1,
                vec![
                    classic(0, "globalThis.answer = 42;"),
                    classic(1, "const = ;"),
                ],
            ),
        });
        let PageHostReply::Synchronized { reports, .. } = reply else {
            panic!("expected synchronized reply");
        };
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].outcome, PageHostScriptOutcome::Executed);
        assert!(matches!(
            reports[1].outcome,
            PageHostScriptOutcome::Rejected { .. }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::GetRealmStats {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::RealmStats(PageHostRealmStats {
                program_count: 1,
                ..
            })
        ));
    }

    #[test]
    fn private_debugger_locations_are_generation_and_tab_bound_without_runtime_leaks() {
        let mut host = BlueJsChildHost::default();
        let first = document(1, vec![classic(0, "let answer = 42;")]);
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument { document: first }),
            PageHostReply::Synchronized { .. }
        ));
        let programs = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
            tab_id: 7,
            document_generation: 1,
        }) {
            PageHostReply::DebuggerPrograms { programs, .. } => programs,
            reply => panic!("expected source-free child debugger programs, got {reply:?}"),
        };
        assert_eq!(programs.len(), 1);
        let program = programs[0];
        let safe_points = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
            tab_id: 7,
            document_generation: 1,
            program,
        }) {
            PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points,
            reply => panic!("expected source-free child debugger safe points, got {reply:?}"),
        };
        let safe_point = *safe_points
            .first()
            .expect("a retained classic program has a root safe point");
        assert_eq!(
            host.handle_request(PageHostRequest::ValidateDebuggerSafePoint {
                tab_id: 7,
                document_generation: 1,
                safe_point,
            }),
            PageHostReply::DebuggerSafePointValidated {
                tab_id: 7,
                document_generation: 1,
                safe_point,
            }
        );
        assert_eq!(
            host.handle_request(PageHostRequest::SetDebuggerBreakpoint {
                tab_id: 7,
                document_generation: 1,
                safe_point,
            }),
            PageHostReply::DebuggerBreakpointSet {
                tab_id: 7,
                document_generation: 1,
                safe_point,
            }
        );
        // Retries are idempotent and cannot consume a second bounded record.
        assert_eq!(
            host.handle_request(PageHostRequest::SetDebuggerBreakpoint {
                tab_id: 7,
                document_generation: 1,
                safe_point,
            }),
            PageHostReply::DebuggerBreakpointSet {
                tab_id: 7,
                document_generation: 1,
                safe_point,
            }
        );
        assert_eq!(
            host.handle_request(PageHostRequest::ListDebuggerBreakpoints {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::DebuggerBreakpoints {
                tab_id: 7,
                document_generation: 1,
                safe_points: vec![safe_point],
            }
        );
        assert_eq!(
            host.handle_request(PageHostRequest::ClearDebuggerBreakpoint {
                tab_id: 7,
                document_generation: 1,
                safe_point,
            }),
            PageHostReply::DebuggerBreakpointCleared {
                tab_id: 7,
                document_generation: 1,
                safe_point,
                was_present: true,
            }
        );
        assert_eq!(
            host.handle_request(PageHostRequest::ClearDebuggerBreakpoint {
                tab_id: 7,
                document_generation: 1,
                safe_point,
            }),
            PageHostReply::DebuggerBreakpointCleared {
                tab_id: 7,
                document_generation: 1,
                safe_point,
                was_present: false,
            }
        );

        let mut other = document(1, vec![classic(0, "let other = 7;")]);
        other.tab_id = 9;
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument { document: other }),
            PageHostReply::Synchronized { .. }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::ListDebuggerSafePoints {
                tab_id: 9,
                document_generation: 1,
                program,
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::InvalidRequest,
                ..
            }
        ));

        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: document(2, vec![classic(0, "let successor = 1;")]),
            }),
            PageHostReply::Synchronized { .. }
        ));
        assert_eq!(
            host.handle_request(PageHostRequest::ListDebuggerBreakpoints {
                tab_id: 7,
                document_generation: 2,
            }),
            PageHostReply::DebuggerBreakpoints {
                tab_id: 7,
                document_generation: 2,
                safe_points: Vec::new(),
            }
        );
        assert!(matches!(
            host.handle_request(PageHostRequest::ListDebuggerPrograms {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::StaleDocument,
                ..
            }
        ));
        let reply = format!(
            "{:?}",
            host.handle_request(PageHostRequest::ListDebuggerSafePoints {
                tab_id: 7,
                document_generation: 2,
                program,
            })
        );
        assert!(
            !reply.contains("answer") && !reply.contains("bytecode") && !reply.contains("Value"),
            "private debugger errors must remain source/value-free"
        );
    }

    #[test]
    fn root_classic_breakpoint_pauses_and_resumes_without_vm_disclosure() {
        let mut host = BlueJsChildHost::default();
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: debugger_document(
                    1,
                    vec![classic(0, "let first = 1; first += 1; globalThis.answer = first;")],
                ),
            }),
            PageHostReply::Synchronized { reports, .. } if reports.is_empty()
        ));
        let program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
            tab_id: 7,
            document_generation: 1,
        }) {
            PageHostReply::DebuggerPrograms { programs, .. } => programs[0],
            reply => panic!("expected private classic program, got {reply:?}"),
        };
        let target = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
            tab_id: 7,
            document_generation: 1,
            program,
        }) {
            PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points
                .into_iter()
                .find(|safe_point| {
                    safe_point.code_unit_ordinal == 0 && safe_point.bytecode_offset != 0
                })
                .expect("fixture must have a non-entry root safe point"),
            reply => panic!("expected source-free safe points, got {reply:?}"),
        };
        assert_eq!(
            host.handle_request(PageHostRequest::GetDebuggerExecutionState {
                tab_id: 7,
                document_generation: 1,
                program,
            }),
            PageHostReply::DebuggerExecutionState {
                tab_id: 7,
                document_generation: 1,
                program,
                state: PageHostDebuggerExecutionState::Pending,
            }
        );
        assert!(matches!(
            host.handle_request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
                tab_id: 7,
                document_generation: 1,
                safe_point: target,
            }),
            PageHostReply::DebuggerRootSafePointBreakpointArmed { safe_point, .. }
                if safe_point == target
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
        ));
        assert_eq!(
            host.handle_request(PageHostRequest::GetDebuggerExecutionState {
                tab_id: 7,
                document_generation: 1,
                program,
            }),
            PageHostReply::DebuggerExecutionState {
                tab_id: 7,
                document_generation: 1,
                program,
                state: PageHostDebuggerExecutionState::Paused { safe_point: target },
            }
        );
        assert!(matches!(
            host.handle_request(PageHostRequest::ResumeDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
                program,
            }),
            PageHostReply::DebuggerExecutionResumed { .. }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::DebuggerExecutionAdvanced { reports, .. }
                if reports == vec![script_report(
                    7,
                    1,
                    0,
                    PageHostScriptLanguage::JavaScript,
                    PageHostScriptKind::Classic,
                    PageHostScriptOutcome::Executed,
                )]
        ));
        let reply = format!(
            "{:?}",
            host.handle_request(PageHostRequest::GetDebuggerExecutionState {
                tab_id: 7,
                document_generation: 1,
                program,
            })
        );
        assert!(reply.contains("Completed"));
        assert!(
            !reply.contains("answer") && !reply.contains("Value") && !reply.contains("Vm"),
            "execution state remains source/value/VM-free"
        );
    }

    #[test]
    fn private_debugger_breakpoint_configuration_is_idempotent_and_bounded() {
        let mut host = BlueJsChildHost::default();
        let source = (0..=PAGE_HOST_DEBUGGER_MAX_BREAKPOINTS_PER_REALM)
            .map(|index| format!("let breakpoint_{index} = {index};"))
            .collect::<String>();
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: document(1, vec![classic(0, &source)]),
            }),
            PageHostReply::Synchronized { .. }
        ));
        let program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
            tab_id: 7,
            document_generation: 1,
        }) {
            PageHostReply::DebuggerPrograms { programs, .. } => programs[0],
            reply => panic!("expected child debugger program, got {reply:?}"),
        };
        let safe_points = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
            tab_id: 7,
            document_generation: 1,
            program,
        }) {
            PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points,
            reply => panic!("expected child debugger safe points, got {reply:?}"),
        };
        let max = usize::try_from(PAGE_HOST_DEBUGGER_MAX_BREAKPOINTS_PER_REALM)
            .expect("page-host breakpoint cap fits usize");
        assert!(
            safe_points.len() > max,
            "fixture needs one point over the cap"
        );
        for safe_point in safe_points.iter().copied().take(max) {
            assert!(matches!(
                host.handle_request(PageHostRequest::SetDebuggerBreakpoint {
                    tab_id: 7,
                    document_generation: 1,
                    safe_point,
                }),
                PageHostReply::DebuggerBreakpointSet { .. }
            ));
        }
        let overflow = safe_points[max];
        assert!(matches!(
            host.handle_request(PageHostRequest::SetDebuggerBreakpoint {
                tab_id: 7,
                document_generation: 1,
                safe_point: overflow,
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::ResourceLimit,
                ..
            }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::ListDebuggerBreakpoints {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::DebuggerBreakpoints { safe_points, .. } if safe_points.len() == max
        ));
    }

    #[test]
    fn direct_bluets_and_javascript_execute_in_document_order_in_one_realm() {
        let mut host = BlueJsChildHost::default();
        let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(
                1,
                vec![
                    classic(0, "globalThis.beforeBlueTs = true;"),
                    // The TypeScript annotation makes this invalid JavaScript
                    // source. Success therefore proves the child used direct
                    // BlueTS lowering rather than emitted-JavaScript reparse.
                    blue_ts_classic(1, "const sharedAnswer: number = 42;"),
                    classic(
                        2,
                        "if (!globalThis.beforeBlueTs || sharedAnswer !== 42) throw 'realm/order failed';",
                    ),
                ],
            ),
        });
        let PageHostReply::Synchronized { reports, .. } = reply else {
            panic!("expected synchronized reply");
        };
        assert_eq!(
            reports
                .iter()
                .map(|report| (report.ordinal, report.language, &report.outcome))
                .collect::<Vec<_>>(),
            vec![
                (
                    0,
                    PageHostScriptLanguage::JavaScript,
                    &PageHostScriptOutcome::Executed
                ),
                (
                    1,
                    PageHostScriptLanguage::BlueTs,
                    &PageHostScriptOutcome::Executed
                ),
                (
                    2,
                    PageHostScriptLanguage::JavaScript,
                    &PageHostScriptOutcome::Executed
                ),
            ]
        );
        assert!(matches!(
            host.handle_request(PageHostRequest::GetRealmStats {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::RealmStats(PageHostRealmStats {
                program_count: 3,
                ..
            })
        ));
    }

    #[test]
    fn child_bluets_debug_metadata_is_bound_to_its_live_realm_generation() {
        let mut host = BlueJsChildHost::default();
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: document(1, vec![blue_ts_classic(0, "const answer: number = 42;")]),
            }),
            PageHostReply::Synchronized { reports, .. }
                if reports == vec![script_report(
                    7,
                    1,
                    0,
                    PageHostScriptLanguage::BlueTs,
                    PageHostScriptKind::Classic,
                    PageHostScriptOutcome::Executed,
                )]
        ));
        assert_eq!(host.debug_registry.len(), 1);
        let first_handle = host
            .documents
            .get(&7)
            .expect("the first child realm remains live")
            .debugger_programs
            .values()
            .map(|record| record.runtime_handle)
            .find(|handle| {
                host.debug_registry
                    .get(host.runtime.program_registry(), *handle)
                    .is_ok()
            })
            .expect("the BlueTS program retains static metadata");
        let first_static_info = host
            .debug_registry
            .get(host.runtime.program_registry(), first_handle)
            .expect("the exact live generation resolves its metadata")
            .static_info();
        assert!(first_static_info
            .sources
            .iter()
            .any(|source| source.module == "blueice://page/inline-0.ts"));
        assert!(!first_static_info.types.is_empty());

        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: document(
                    2,
                    vec![blue_ts_classic(0, "const answer: string = 'next';")]
                ),
            }),
            PageHostReply::Synchronized { .. }
        ));
        assert_eq!(host.debug_registry.len(), 1);
        assert!(host
            .debug_registry
            .get(host.runtime.program_registry(), first_handle)
            .is_err());

        assert!(matches!(
            host.handle_request(PageHostRequest::CloseRealm {
                tab_id: 7,
                document_generation: 2,
            }),
            PageHostReply::RealmClosed {
                tab_id: 7,
                document_generation: 2,
            }
        ));
        assert!(host.debug_registry.is_empty());
    }

    #[test]
    fn child_bluets_metadata_inventory_mints_only_opaque_live_attachment_handles() {
        let mut host = BlueJsChildHost::default();
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: document(
                    1,
                    vec![
                        classic(0, "globalThis.javaScriptOnly = true;"),
                        blue_ts_classic(1, "const typedAnswer: number = 42;"),
                    ],
                ),
            }),
            PageHostReply::Synchronized { reports, .. }
                if reports.iter().all(|report| report.outcome == PageHostScriptOutcome::Executed)
        ));
        let programs = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
            tab_id: 7,
            document_generation: 1,
        }) {
            PageHostReply::DebuggerPrograms { programs, .. } => programs,
            reply => panic!("expected private program inventory, got {reply:?}"),
        };
        assert_eq!(programs.len(), 2);

        let mut blue_ts = None;
        for program in programs {
            let reply = host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
                tab_id: 7,
                document_generation: 1,
                program,
            });
            let PageHostReply::DebuggerBlueTsMetadata { metadata, .. } = &reply else {
                panic!("expected private BlueTS metadata inventory, got {reply:?}");
            };
            // The child exposes no source/module/name/type/span/contract data:
            // only the bounded list's length and its opaque values are visible.
            assert!(!format!("{reply:?}").contains("typedAnswer"));
            assert!(!format!("{reply:?}").contains("inline-1.ts"));
            assert!(!format!("{reply:?}").contains("number"));
            if let [metadata] = metadata.as_slice() {
                blue_ts = Some((program, *metadata));
            } else {
                assert!(
                    metadata.is_empty(),
                    "only the JavaScript program is ineligible"
                );
            }
        }
        let (typed_program, first_metadata) = blue_ts.expect("the live BlueTS attachment exists");
        assert!(first_metadata.is_well_formed());
        assert_ne!(
            first_metadata.metadata_handle, typed_program.program_handle,
            "metadata IDs must not reuse the child program namespace"
        );
        assert!(
            first_metadata.metadata_handle >= CHILD_DEBUGGER_METADATA_ID_NAMESPACE_START,
            "metadata IDs have a child-private namespace separate from program IDs"
        );
        let summary_reply = host.handle_request(PageHostRequest::DescribeDebuggerBlueTsMetadata {
            tab_id: 7,
            document_generation: 1,
            program: typed_program,
            metadata: first_metadata,
        });
        let PageHostReply::DebuggerBlueTsMetadataSummary {
            metadata, summary, ..
        } = summary_reply
        else {
            panic!("expected bounded private BlueTS metadata summary")
        };
        assert_eq!(metadata, first_metadata);
        assert_eq!(summary.language_version, "blue-ts-0.1");
        assert!(summary.source_count > 0);
        assert!(summary.type_count > 0);
        assert!(summary.symbol_count > 0);
        // The summary intentionally reveals no source/module/name/type/span/
        // contract record. A separate future capability would be needed for
        // every individual record family.
        assert!(!format!("{summary:?}").contains("typedAnswer"));
        assert!(!format!("{summary:?}").contains("inline-1.ts"));
        assert!(!format!("{summary:?}").contains("number"));
        assert!(matches!(
            host.handle_request(PageHostRequest::DescribeDebuggerBlueTsMetadata {
                tab_id: 7,
                document_generation: 1,
                program: typed_program,
                metadata: PageHostDebuggerMetadataHandle {
                    metadata_handle: first_metadata.metadata_handle,
                    metadata_generation: first_metadata.metadata_generation + 1,
                },
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::InvalidRequest,
                ..
            }
        ));

        assert!(matches!(
            host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
                tab_id: 7,
                document_generation: 1,
                program: typed_program,
            }),
            PageHostReply::DebuggerBlueTsMetadata { metadata, .. }
                if metadata == vec![first_metadata]
        ));

        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: document(
                    2,
                    vec![blue_ts_classic(0, "const replacement: string = 'next';")]
                ),
            }),
            PageHostReply::Synchronized { .. }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
                tab_id: 7,
                document_generation: 1,
                program: typed_program,
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::StaleDocument,
                ..
            }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::DescribeDebuggerBlueTsMetadata {
                tab_id: 7,
                document_generation: 1,
                program: typed_program,
                metadata: first_metadata,
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::StaleDocument,
                ..
            }
        ));
        let replacement_program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
            tab_id: 7,
            document_generation: 2,
        }) {
            PageHostReply::DebuggerPrograms { programs, .. } => *programs
                .first()
                .expect("the replacement BlueTS program remains live"),
            reply => panic!("expected replacement private program inventory, got {reply:?}"),
        };
        let replacement_metadata =
            match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
                tab_id: 7,
                document_generation: 2,
                program: replacement_program,
            }) {
                PageHostReply::DebuggerBlueTsMetadata { metadata, .. } => *metadata
                    .first()
                    .expect("the replacement live BlueTS attachment remains eligible"),
                reply => panic!("expected replacement metadata inventory, got {reply:?}"),
            };
        assert_ne!(replacement_metadata, first_metadata);

        assert!(matches!(
            host.handle_request(PageHostRequest::CloseRealm {
                tab_id: 7,
                document_generation: 2,
            }),
            PageHostReply::RealmClosed { .. }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
                tab_id: 7,
                document_generation: 2,
                program: replacement_program,
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::UnknownRealm,
                ..
            }
        ));
    }

    #[test]
    fn child_installs_only_fixed_core_snapshot_callbacks_for_javascript() {
        let mut host = BlueJsChildHost::default();
        let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document_with_snapshot(
                1,
                "private document snapshot".to_string(),
                "https://example.test".to_string(),
                vec![
                    classic(
                        0,
                        concat!(
                            "if (blueiceDocumentText() !== 'private document snapshot') throw 'text';",
                            "if (blueiceDocumentOrigin() !== 'https://example.test') throw 'origin';",
                            "if (typeof document !== 'undefined' || typeof fetch !== 'undefined') throw 'ambient';"
                        ),
                    ),
                    classic(1, "blueiceDocumentText(1);"),
                ],
            ),
        });
        let PageHostReply::Synchronized { reports, .. } = reply else {
            panic!("expected synchronized reply");
        };
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].outcome, PageHostScriptOutcome::Executed);
        assert!(matches!(
            reports[1].outcome,
            PageHostScriptOutcome::Rejected { .. }
        ));
        // Result records remain source/value-free even when a callback
        // returned a private snapshot inside the child VM.
        assert!(!format!("{:?}", reports).contains("private document snapshot"));
    }

    #[test]
    fn snapshot_validation_happens_before_realm_creation_or_replacement() {
        let mut host = BlueJsChildHost::default();
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: document_with_snapshot(
                    1,
                    "snapshot".to_string(),
                    "HTTP://EXAMPLE.test".to_string(),
                    vec![classic(0, "globalThis.answer = 42;")],
                ),
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::InvalidRequest,
                ..
            }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::GetRealmStats {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::UnknownRealm,
                ..
            }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: document_with_snapshot(
                    1,
                    "x".repeat(PAGE_HOST_DOCUMENT_TEXT_MAX_BYTES + 1),
                    "https://example.test".to_string(),
                    vec![classic(0, "globalThis.answer = 42;")],
                ),
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::ResourceLimit,
                ..
            }
        ));
    }

    #[test]
    fn bluets_uses_the_verified_snapshot_profile_and_rejects_untyped_globals() {
        let mut host = BlueJsChildHost::default();
        let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(
                1,
                vec![
                    // The annotation is invalid JavaScript syntax, so this
                    // success also preserves direct BlueTS-to-BlueJS lowering.
                    blue_ts_classic(
                        0,
                        concat!(
                            "const snapshotText: string = blueiceDocumentText();",
                            "const snapshotOrigin: string = blueiceDocumentOrigin();"
                        ),
                    ),
                    classic(
                        1,
                        concat!(
                            "if (snapshotText !== 'test document snapshot') throw 'text';",
                            "if (snapshotOrigin !== 'https://example.test') throw 'origin';"
                        ),
                    ),
                    // The fixed declaration contains neither fetch nor a
                    // general document object. Both compile attempts fail
                    // before a program is admitted to the child realm.
                    blue_ts_classic(2, "fetch('https://example.test/');"),
                    blue_ts_classic(3, "blueiceDocumentText(1);"),
                ],
            ),
        });
        assert!(matches!(
            reply,
            PageHostReply::Synchronized {
                reports,
                ..
            } if reports.len() == 4
                && reports[0].language == PageHostScriptLanguage::BlueTs
                && reports[0].outcome == PageHostScriptOutcome::Executed
                && reports[1].language == PageHostScriptLanguage::JavaScript
                && reports[1].outcome == PageHostScriptOutcome::Executed
                && matches!(reports[2].outcome, PageHostScriptOutcome::Rejected { .. })
                && matches!(reports[3].outcome, PageHostScriptOutcome::Rejected { .. })
        ));
    }

    #[test]
    fn module_graph_uses_only_the_explicit_static_resolution_records() {
        let entry = "blueice://page/entry.js";
        let dependency = "blueice://page/dependency.js";
        let mut module = PageHostScript {
            ordinal: 0,
            language: PageHostScriptLanguage::JavaScript,
            kind: PageHostScriptKind::Module,
            graph: graph(
                entry,
                vec![
                    PageHostSource::new(
                        entry,
                        "import { answer } from './dependency.js'; export const result = answer;",
                    ),
                    PageHostSource::new(dependency, "export const answer = 42;"),
                ],
            ),
        };
        module.graph.resolutions.push(PageHostStaticResolution {
            from_module: entry.to_string(),
            specifier: "./dependency.js".to_string(),
            canonical_target: dependency.to_string(),
        });
        let mut host = BlueJsChildHost::default();
        let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(1, vec![module]),
        });
        assert!(matches!(
            reply,
            PageHostReply::Synchronized {
                reports,
                ..
            } if reports[0].outcome == PageHostScriptOutcome::Executed
        ));
    }

    #[test]
    fn direct_bluets_module_graph_uses_only_the_explicit_static_resolution_records() {
        let entry = "blueice://page/entry.ts";
        let dependency = "blueice://page/dependency.ts";
        let mut module = PageHostScript {
            ordinal: 0,
            language: PageHostScriptLanguage::BlueTs,
            kind: PageHostScriptKind::Module,
            graph: graph(
                entry,
                vec![
                    PageHostSource::new(
                        entry,
                        "import { answer } from './dependency.ts'; export const result: number = answer;",
                    ),
                    PageHostSource::new(dependency, "export const answer: number = 42;"),
                ],
            ),
        };
        module.graph.resolutions.push(PageHostStaticResolution {
            from_module: entry.to_string(),
            specifier: "./dependency.ts".to_string(),
            canonical_target: dependency.to_string(),
        });
        let mut host = BlueJsChildHost::default();
        let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(1, vec![module]),
        });
        assert!(matches!(
            reply,
            PageHostReply::Synchronized {
                reports,
                ..
            } if matches!(
                reports.as_slice(),
                [PageHostScriptReport {
                    language: PageHostScriptLanguage::BlueTs,
                    kind: PageHostScriptKind::Module,
                    outcome: PageHostScriptOutcome::Executed,
                    ..
                }]
            )
        ));
        assert_eq!(host.debug_registry.len(), 2);
        let mut retained_modules: Vec<_> = host
            .documents
            .get(&7)
            .expect("the child realm remains live")
            .debugger_programs
            .values()
            .filter_map(|record| {
                host.debug_registry
                    .get(host.runtime.program_registry(), record.runtime_handle)
                    .ok()
            })
            .map(|metadata| {
                let sources = &metadata.static_info().sources;
                sources
                    .iter()
                    .find(|source| source.module == entry || source.module == dependency)
                    .expect("each module keeps its own static source metadata")
                    .module
                    .clone()
            })
            .collect();
        retained_modules.sort();
        assert_eq!(
            retained_modules,
            vec![dependency.to_string(), entry.to_string()]
        );
    }

    #[test]
    fn leading_bluets_attaches_static_metadata_without_an_unavailable_pause_surface() {
        let mut host = BlueJsChildHost::default();
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: debugger_document(
                    1,
                    vec![blue_ts_classic(0, "const deferredAnswer: number = 42;")],
                ),
            }),
            PageHostReply::Synchronized { reports, .. }
                if reports == vec![script_report(
                    7,
                    1,
                    0,
                    PageHostScriptLanguage::BlueTs,
                    PageHostScriptKind::Classic,
                    PageHostScriptOutcome::Executed,
                )]
        ));
        assert_eq!(host.debug_registry.len(), 1);
        assert!(matches!(
            host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::DebuggerExecutionAdvanced { reports, .. }
                if reports.is_empty()
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::CloseRealm {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::RealmClosed { .. }
        ));
        assert!(host.debug_registry.is_empty());
    }

    #[test]
    fn a_missing_module_resolution_admits_no_partial_graph_programs() {
        let entry = "blueice://page/entry.js";
        let dependency = "blueice://page/dependency.js";
        let script = PageHostScript {
            ordinal: 0,
            language: PageHostScriptLanguage::JavaScript,
            kind: PageHostScriptKind::Module,
            graph: graph(
                entry,
                vec![
                    PageHostSource::new(entry, "import './dependency.js';"),
                    PageHostSource::new(dependency, "export const answer = 42;"),
                ],
            ),
        };
        let mut host = BlueJsChildHost::default();
        let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(1, vec![script]),
        });
        assert!(matches!(
            reply,
            PageHostReply::Synchronized {
                reports,
                ..
            } if matches!(reports[0].outcome, PageHostScriptOutcome::Rejected { .. })
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::GetRealmStats {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::RealmStats(PageHostRealmStats {
                program_count: 0,
                bytecode_bytes: 0,
                ..
            })
        ));
    }

    #[test]
    fn document_generations_are_idempotent_and_cannot_close_a_successor() {
        let mut host = BlueJsChildHost::default();
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: document(1, vec![classic(0, "globalThis.first = true;")]),
            }),
            PageHostReply::Synchronized {
                already_current: false,
                ..
            }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: document(1, vec![classic(0, "throw new Error('must not replay');")]),
            }),
            PageHostReply::Synchronized {
                already_current: true,
                reports,
                ..
            } if reports.is_empty()
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: document(2, vec![classic(0, "globalThis.second = true;")]),
            }),
            PageHostReply::Synchronized {
                already_current: false,
                ..
            }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::CloseRealm {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::StaleDocument,
                ..
            }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::GetRealmStats {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::StaleDocument,
                ..
            }
        ));
    }

    #[test]
    fn source_hash_tampering_is_rejected_before_realm_program_admission() {
        let mut source = PageHostSource::new("blueice://page/main.js", "globalThis.answer = 42;");
        source.source_hash = "fnv1a64:0000000000000000".to_string();
        let script = PageHostScript {
            ordinal: 0,
            language: PageHostScriptLanguage::JavaScript,
            kind: PageHostScriptKind::Classic,
            graph: graph("blueice://page/main.js", vec![source]),
        };
        let mut host = BlueJsChildHost::default();
        let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(1, vec![script]),
        });
        assert!(matches!(
            reply,
            PageHostReply::Synchronized {
                reports,
                ..
            } if matches!(reports[0].outcome, PageHostScriptOutcome::Rejected { .. })
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::GetRealmStats {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::RealmStats(PageHostRealmStats {
                program_count: 0,
                ..
            })
        ));
    }

    #[test]
    fn document_source_budget_rejects_before_realm_replacement() {
        let source = PageHostSource::new(
            "blueice://page/too-large.js",
            "x".repeat(MAX_SOURCE_BYTES_PER_DOCUMENT + 1),
        );
        let script = PageHostScript {
            ordinal: 0,
            language: PageHostScriptLanguage::JavaScript,
            kind: PageHostScriptKind::Classic,
            graph: graph("blueice://page/too-large.js", vec![source]),
        };
        let mut host = BlueJsChildHost::default();
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: document(1, vec![script]),
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::ResourceLimit,
                ..
            }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::GetRealmStats {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::UnknownRealm,
                ..
            }
        ));
    }

    #[test]
    fn duplicate_script_ordinals_are_rejected_before_realm_replacement() {
        let mut host = BlueJsChildHost::default();
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: document(
                    1,
                    vec![
                        classic(0, "globalThis.first = true;"),
                        classic(0, "globalThis.second = true;"),
                    ],
                ),
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::InvalidRequest,
                ..
            }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::GetRealmStats {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::UnknownRealm,
                ..
            }
        ));
    }

    #[test]
    fn private_socket_is_owner_only() {
        let path = PathBuf::from("/private/tmp").join(format!(
            "blueice-launcher-owner-only-test-{}.sock",
            std::process::id()
        ));
        let listener = bind_bluejs_host_socket(&path).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        drop(listener);
        let _ = fs::remove_file(path);
    }
}
