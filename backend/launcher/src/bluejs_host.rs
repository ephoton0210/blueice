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
    parse, parse_module, BlueJsPageOrigin, BlueJsPageRuntime, BlueJsPageRuntimeConfig,
    BlueJsPageRuntimeError, BlueJsProgramHandle, BlueJsProgramV1, BlueJsSourceIdentity,
    CompileError, HostFunctionError, HostValue, Module, ParseError, RuntimeError, Value,
};
use blueice_bluets::{
    AuthorizedModule, AuthorizedModuleLoader, AuthorizedModuleResolution, CompilerOptions,
    RuntimePolicy,
};
use blueice_bluets_bluejs::page_host_typings::{
    page_host_document_runtime_bindings_v1, PageHostDocumentTypingsV1,
};
use blueice_bluets_bluejs::{
    compile_direct_module_graph, compile_direct_script, BridgeError, DirectModuleGraph,
    DirectScript,
};
use blueice_ipc::page_host::{
    self, PageHostDebuggerProgram, PageHostDebuggerSafePoint, PageHostDocument,
    PageHostDocumentSnapshot, PageHostErrorCode, PageHostModuleGraph, PageHostRealmStats,
    PageHostReply, PageHostRequest, PageHostScript, PageHostScriptKind, PageHostScriptLanguage,
    PageHostScriptOutcome, PageHostScriptReport, PageHostSource, PageHostStaticResolution,
    PAGE_HOST_DEBUGGER_MAX_BREAKPOINTS_PER_REALM, PAGE_HOST_DEBUGGER_MAX_SAFE_POINTS_PER_PROGRAM,
    PAGE_HOST_DOCUMENT_ORIGIN_MAX_BYTES, PAGE_HOST_DOCUMENT_TEXT_MAX_BYTES,
};
use blueice_net::canonical_http_origin;
use std::collections::{BTreeMap, BTreeSet};
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct LiveDocument {
    generation: u64,
    debugger_programs: BTreeMap<u64, ChildDebuggerProgram>,
    /// Exact child-private breakpoint configuration records. These are not a
    /// VM interruption hook; replacing or closing the realm drops them.
    debugger_breakpoints: BTreeSet<PageHostDebuggerSafePoint>,
}

/// Private child-only association between a child-minted opaque debugger
/// identity and its BlueJS registry handle. The registry handle never leaves
/// this process; the core separately mints its public debugger identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ChildDebuggerProgram {
    program_generation: u64,
    runtime_handle: BlueJsProgramHandle,
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
    documents: BTreeMap<u64, LiveDocument>,
    next_debugger_program_handle: u64,
    next_debugger_program_generation: u64,
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
            documents: BTreeMap::new(),
            next_debugger_program_handle: 1,
            next_debugger_program_generation: 1,
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
            self.documents.remove(&document.tab_id);
            return host_failure();
        }
        self.documents.insert(
            document.tab_id,
            LiveDocument {
                generation: document.document_generation,
                debugger_programs: BTreeMap::new(),
                debugger_breakpoints: BTreeSet::new(),
            },
        );

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

    fn close_realm(&mut self, tab_id: u64, document_generation: u64) -> PageHostReply {
        match self.documents.get(&tab_id) {
            None => unknown_realm(),
            Some(document) if document.generation != document_generation => stale_document(),
            Some(_) => {
                self.runtime.close_realm(tab_id);
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
        let mut programs = BTreeMap::new();
        for runtime_handle in runtime_handles {
            let program_handle = self.next_debugger_program_handle;
            let program_generation = self.next_debugger_program_generation;
            self.next_debugger_program_handle = program_handle.checked_add(1).ok_or(())?;
            self.next_debugger_program_generation = program_generation.checked_add(1).ok_or(())?;
            programs.insert(
                program_handle,
                ChildDebuggerProgram {
                    program_generation,
                    runtime_handle,
                },
            );
        }
        self.documents.get_mut(&tab_id).ok_or(())?.debugger_programs = programs;
        Ok(())
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
    tab_id: u64,
    origin: &BlueJsPageOrigin,
    script: &DirectScript,
) -> PageHostScriptOutcome {
    let attachment = match script.attach_in_page_realm(runtime, tab_id, origin) {
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
    tab_id: u64,
    origin: &BlueJsPageOrigin,
    graph: &DirectModuleGraph,
) -> PageHostScriptOutcome {
    let attachment = match graph.attach_in_page_realm(runtime, tab_id, origin) {
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

fn invalid_request() -> PageHostReply {
    PageHostReply::Error {
        code: PageHostErrorCode::InvalidRequest,
        message: "BlueJS page-host request is invalid".to_string(),
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
        let mut host = Self::spawn_unconnected()?;
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
        let host = Self::spawn_unconnected()?;
        let config = BlueJsHostCoreConfig {
            socket_path: host.socket_path.clone(),
            session_token: host.session_token.clone(),
        };
        Ok((host, config))
    }

    fn spawn_unconnected() -> io::Result<Self> {
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
            scripts,
        }
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
