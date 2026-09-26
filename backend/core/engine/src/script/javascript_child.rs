// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Explicit core adapter for a launcher-supervised, out-of-process BlueJS
//! page host.
//!
//! The core connects only when its operator supplied the private child socket
//! and one-time session capability. For each loaded HTTP(S) document it
//! mints a canonical identity, a one-module closed graph, and the fixed
//! resolver fingerprint for inline declarations. An external declaration
//! remains a source-free rejection unless the immutable core-owned authorizer
//! selected at executor startup supplies its complete closed graph. Core copies
//! that graph into the private protocol; neither this adapter nor the child
//! fetches, resolves a URL/import map, reads a filesystem, or falls back to a
//! second source loader. Ordinary realms install only fixed copied JavaScript
//! document-text/origin callbacks, with no document object, DOM/event object,
//! storage, network, URL, resolver, or page-selected binding. A separate
//! launcher-owner proof profile can install one boolean child-to-core DOM
//! lookup callback; it exposes no core node ID or raw IPC to page code.

use super::{
    contracts::{core_script_binding_contract, CoreScriptBindingContractLimits},
    direct_page::DirectPageScriptKind,
    BlueJsPageScriptKind, CombinedPageScriptDeclaration, CombinedPageScriptLanguage,
};
use crate::script::javascript::{
    AuthorizedJavaScriptModuleGraph, BlueTsPageExecutionReport, JavaScriptPageDebuggerBreakpoint,
    JavaScriptPageDebuggerError, JavaScriptPageDebuggerExceptionLocation,
    JavaScriptPageDebuggerExceptionLocationTarget, JavaScriptPageDebuggerExecutionState,
    JavaScriptPageDebuggerFrame, JavaScriptPageDebuggerLinkedExecutionState,
    JavaScriptPageDebuggerLinkedScopeSnapshot, JavaScriptPageDebuggerLinkedSpanAccess,
    JavaScriptPageDebuggerLinkedStackFrame, JavaScriptPageDebuggerLinkedStackSnapshot,
    JavaScriptPageDebuggerNestedExecutionState, JavaScriptPageDebuggerProgram,
    JavaScriptPageDebuggerSafePoint, JavaScriptPageDebuggerScopeEntry,
    JavaScriptPageDebuggerStackFrame, JavaScriptPageDebuggerStackSnapshot,
    JavaScriptPageDebuggerStaticMetadata, JavaScriptPageDebuggerStaticMetadataContractDisplay,
    JavaScriptPageDebuggerStaticMetadataContractId,
    JavaScriptPageDebuggerStaticMetadataContractLocation,
    JavaScriptPageDebuggerStaticMetadataContractLocationTarget,
    JavaScriptPageDebuggerStaticMetadataContractTarget,
    JavaScriptPageDebuggerStaticMetadataContractValidation,
    JavaScriptPageDebuggerStaticMetadataLoweringSummary,
    JavaScriptPageDebuggerStaticMetadataSafePointSpan,
    JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget,
    JavaScriptPageDebuggerStaticMetadataSourceBreakpointTarget,
    JavaScriptPageDebuggerStaticMetadataSourceId,
    JavaScriptPageDebuggerStaticMetadataSourceProvenance,
    JavaScriptPageDebuggerStaticMetadataSourceTarget, JavaScriptPageDebuggerStaticMetadataSummary,
    JavaScriptPageDebuggerStaticMetadataSymbolContract,
    JavaScriptPageDebuggerStaticMetadataSymbolContractTarget,
    JavaScriptPageDebuggerStaticMetadataSymbolDisplay,
    JavaScriptPageDebuggerStaticMetadataSymbolId,
    JavaScriptPageDebuggerStaticMetadataSymbolLocation,
    JavaScriptPageDebuggerStaticMetadataSymbolLocationTarget,
    JavaScriptPageDebuggerStaticMetadataSymbolTarget,
    JavaScriptPageDebuggerStaticMetadataSymbolType,
    JavaScriptPageDebuggerStaticMetadataSymbolTypeTarget,
    JavaScriptPageDebuggerStaticMetadataTypeDisplay, JavaScriptPageDebuggerStaticMetadataTypeId,
    JavaScriptPageDebuggerStaticMetadataTypeTarget, JavaScriptPageDebuggerStaticScopeRelation,
    JavaScriptPageDebuggerStaticScopeTarget, JavaScriptPageDebuggerValuePreview,
    JavaScriptPageDebuggerValueTarget, JavaScriptPageExecutionReport,
    PageJavaScriptDebuggerLocations, PageJavaScriptExecutor,
};
use crate::script::page_source_authorizer::AuthorizedPageScriptGraph;
pub use crate::script::page_source_authorizer::{
    AuthorizedOutOfProcessPageScriptGraph, OutOfProcessPageScriptSourceAuthorizationError,
    OutOfProcessPageScriptSourceAuthorizer, OutOfProcessPageScriptSourceRequest,
};
use crate::script::ScriptRequestReceiver;
use crate::{Page, TabId, TabManager};
use blueice_ipc::compiler::CompilerContractValue;
use blueice_ipc::debugger::{
    DEBUGGER_STATIC_METADATA_CONTRACT_DISPLAY_MAX_BYTES, DEBUGGER_STATIC_METADATA_MAX_CONTRACTS,
    DEBUGGER_STATIC_METADATA_MAX_SOURCES, DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES,
    DEBUGGER_STATIC_METADATA_MAX_SYMBOLS, DEBUGGER_STATIC_METADATA_MAX_TYPES,
    DEBUGGER_STATIC_METADATA_SYMBOL_DISPLAY_MAX_BYTES,
    DEBUGGER_STATIC_METADATA_TYPE_DISPLAY_MAX_BYTES,
};
use blueice_ipc::page_host::{
    self, PageHostChildStats, PageHostDebuggerBlueTsMetadataSymbolContract,
    PageHostDebuggerBlueTsMetadataSymbolLocation, PageHostDebuggerBlueTsMetadataSymbolType,
    PageHostDebuggerExecutionState, PageHostDebuggerFrame, PageHostDebuggerLinkedExecutionState,
    PageHostDebuggerLinkedFrame, PageHostDebuggerLinkedSource, PageHostDebuggerLinkedStackSnapshot,
    PageHostDebuggerMetadataHandle, PageHostDebuggerProgram, PageHostDebuggerSafePoint,
    PageHostDebuggerScopeEntry, PageHostDebuggerStackSnapshot, PageHostDebuggerStaticScopeTarget,
    PageHostDebuggerValuePreview, PageHostDebuggerValueTarget, PageHostDocument,
    PageHostDocumentSnapshot, PageHostErrorCode, PageHostModuleGraph, PageHostRealmStats,
    PageHostReply, PageHostRequest, PageHostScript, PageHostScriptKind, PageHostScriptLanguage,
    PageHostScriptOutcome, PageHostSource, PageHostStaticResolution,
    PAGE_HOST_DEBUGGER_MAX_BREAKPOINTS_PER_REALM, PAGE_HOST_DEBUGGER_MAX_SAFE_POINTS_PER_PROGRAM,
    PAGE_HOST_DEBUGGER_MAX_SCOPE_ENTRIES, PAGE_HOST_DEBUGGER_MAX_STACK_FRAMES,
};
use blueice_ipc::script::ScriptDocumentTarget;
use blueice_net::canonical_http_origin;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

/// The maximum number of source-free child-host results retained by core for
/// the existing tab-addressed control-plane drain.
const MAX_EXECUTION_REPORTS: usize = 128;
/// One page-host execution receives a fixed total DOM-call allowance. The
/// session still limits each polling turn independently in `script.rs`.
const MAX_NESTED_SCRIPT_REQUESTS_PER_WAIT: usize = 1_024;
/// The request write and reply read share this one deadline.
const CHILD_DOCUMENT_REPLY_WAIT: Duration = Duration::from_secs(60);
const CHILD_DOCUMENT_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Do not restart the public frame namespace when a child/executor is
/// replaced in this core process. Exhaustion fails closed rather than aliasing
/// a predecessor invocation.
static NEXT_CORE_DEBUGGER_FRAME_HANDLE: AtomicU64 = AtomicU64::new(1);

fn pump_script_requests_during_child_wait(
    script_requests: &ScriptRequestReceiver,
    tabs: &mut TabManager,
    target: ScriptDocumentTarget,
    remaining: &mut usize,
) -> io::Result<()> {
    if *remaining == 0 {
        if script_requests.reject_one_pending_for_exhausted_wait() {
            return Err(io::Error::other(
                "page-host script DOM request budget exhausted",
            ));
        }
        return Ok(());
    }
    let dispatched = script_requests.dispatch_pending_for_document(tabs, target, *remaining);
    *remaining -= dispatched;
    Ok(())
}

/// The fixed core-owned resolver identity for a one-source inline document
/// graph. It is not a URL resolver and cannot be selected by page content.
const INLINE_CHILD_RESOLVER_FINGERPRINT: &str = "core-inline-page-host-v1";

/// Core-owned namespace for public debugger program IDs that proxy the child.
/// Keeping it disjoint from the child counter makes it mechanically apparent
/// that a private child identifier cannot become a public protocol identity.
const CORE_CHILD_DEBUGGER_ID_NAMESPACE_START: u64 = 1 << 63;

/// Core-reminted public static-metadata identities remain numerically disjoint
/// from public debugger program IDs and every child-private namespace. The
/// distinct Rust types enforce the boundary; this range is defense in depth.
const CORE_CHILD_DEBUGGER_METADATA_ID_NAMESPACE_START: u64 = 1 << 62;

/// A direct BlueTS program has at most one attached compiler debug record.
/// Keep the public inventory bounded even if an untrusted child returns a
/// malformed larger reply.
const MAX_CHILD_DEBUGGER_STATIC_METADATA_PER_PROGRAM: usize = 1;

/// A socket peer may need several session turns to discover a newly admitted
/// program and arm its one root safe point, but it cannot turn that discovery
/// protocol into an unbounded page-execution lease. This is intentionally a
/// core-owned fixed budget, not a public debugger parameter.
const MAX_OOP_DEBUGGER_EXECUTION_DEFERRALS_PER_DOCUMENT: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
struct LiveDocument {
    document_generation: u64,
    origin: String,
}

/// Everything derived from a borrowed `Page` before the core enters the
/// reentrant child wait. Keeping this owned makes it possible to release the
/// immutable page borrow while the session thread mutates that same document
/// in response to an authenticated child DOM call.
struct PreparedDocumentSync {
    document: PageHostDocument,
    outcome: PendingDocumentOutcome,
}

struct PendingDocumentOutcome {
    local_reports: Vec<JavaScriptPageExecutionReport>,
    local_blue_ts_reports: Vec<BlueTsPageExecutionReport>,
    inline_scripts: Vec<(u32, PageHostScriptLanguage, PageHostScriptKind)>,
}

/// Explicit core-owned lifecycle owner for one authenticated child host.
///
/// This is intentionally not constructed by the default session. It makes a
/// document executable only after the core process selected the child channel
/// at startup. The default constructor admits inline scripts only. Its
/// separately named opt-in constructor accepts one immutable core-owned
/// external graph authorizer; this type itself never gains URL resolution or
/// source-loading authority.
pub struct OutOfProcessJavaScriptPageExecutor<C> {
    child: C,
    external_source_authorizer: Option<Box<dyn OutOfProcessPageScriptSourceAuthorizer>>,
    /// This changes document scheduling, so it is selected only by the core
    /// constructor and defaults to false for the existing page-host path.
    native_debugger_execution_control: bool,
    hold_pending_debugger_execution_once: bool,
    /// Remaining one-turn discovery/configuration deferrals keyed by the
    /// document that received them. Realm replacement and close discard the
    /// budget with every other OOP debugger lifetime record.
    debugger_execution_deferrals: BTreeMap<TabId, DebuggerExecutionDeferral>,
    live_documents: BTreeMap<TabId, LiveDocument>,
    /// Source-free child accounting retained only by the core for the exact
    /// live realm. A page, frontend, debugger, and MCP client receive neither
    /// this record nor a handle that could request it.
    realm_stats: BTreeMap<TabId, PageHostRealmStats>,
    /// Core-minted public debugger identities keyed by the child-private
    /// program IDs they represent. Child IDs are transport keys only and can
    /// never accidentally become public protocol IDs.
    debugger_programs: BTreeMap<TabId, BTreeMap<PageHostDebuggerProgram, CoreDebuggerProgram>>,
    debugger_nested_frames: BTreeMap<TabId, ActiveDebuggerFrame>,
    debugger_linked_frames: BTreeMap<TabId, ActiveLinkedDebuggerPause>,
    next_debugger_program_handle: u64,
    next_debugger_program_generation: u64,
    /// Public inventory identities keyed by child-private metadata handles.
    /// The map is discarded with every document replacement or close and a
    /// public debugger peer never receives the child key.
    debugger_static_metadata:
        BTreeMap<TabId, BTreeMap<PageHostDebuggerMetadataHandle, CoreDebuggerStaticMetadata>>,
    next_debugger_metadata_handle: u64,
    next_debugger_metadata_generation: u64,
    reports: VecDeque<JavaScriptPageExecutionReport>,
    blue_ts_reports: VecDeque<BlueTsPageExecutionReport>,
}

#[derive(Debug, Clone, Copy)]
struct CoreDebuggerProgram {
    program_handle: u64,
    program_generation: u64,
}

#[derive(Debug, Clone, Copy)]
struct ActiveDebuggerFrame {
    public: JavaScriptPageDebuggerFrame,
    child: PageHostDebuggerFrame,
}

/// Both identities belong to one complete child-first linked stack. Neither
/// child program nor invocation serial is exposed through these core frames.
#[derive(Debug, Clone)]
struct ActiveLinkedDebuggerPause {
    child: PageHostDebuggerLinkedFrame,
    child_stack: PageHostDebuggerLinkedStackSnapshot,
    frames: [JavaScriptPageDebuggerLinkedStackFrame; 2],
}

#[derive(Debug, Clone, Copy)]
struct CoreDebuggerStaticMetadata {
    program: PageHostDebuggerProgram,
    metadata_handle: u64,
    metadata_generation: u64,
}

#[derive(Debug, Clone, Copy)]
struct DebuggerExecutionDeferral {
    document_generation: u64,
    remaining: usize,
}

impl OutOfProcessJavaScriptPageExecutor<PageHostConnection> {
    /// Connects this explicit executor to an already launcher-supervised
    /// child. It does not spawn the child, open a URL, or enable itself from
    /// page content.
    pub fn connect(socket_path: &Path, session_token: &str) -> io::Result<Self> {
        Ok(Self::new(PageHostConnection::connect(
            socket_path,
            session_token,
        )?))
    }

    /// Connects an explicitly configured child route with one immutable
    /// core-owned external source authority. The authorizer is fixed for this
    /// executor's lifetime; no document, script, or frontend request can
    /// select or replace it.
    pub fn connect_with_external_source_authorizer(
        socket_path: &Path,
        session_token: &str,
        authorizer: impl OutOfProcessPageScriptSourceAuthorizer + 'static,
    ) -> io::Result<Self> {
        Ok(Self::with_external_source_authorizer(
            PageHostConnection::connect(socket_path, session_token)?,
            authorizer,
        ))
    }

    /// Connects an explicitly selected child route with the bounded
    /// root-classic debugger lifecycle. Normal OOP page execution remains the
    /// default; a page and frontend cannot enable this constructor.
    pub fn connect_with_debugger_execution_control(
        socket_path: &Path,
        session_token: &str,
    ) -> io::Result<Self> {
        Ok(Self::new_with_debugger_execution_control(
            PageHostConnection::connect(socket_path, session_token)?,
        ))
    }
}

#[allow(dead_code)] // The public linked-module wire connects these staged core operations.
impl<C: PageHostClient> OutOfProcessJavaScriptPageExecutor<C> {
    fn capture_core_linked_pause(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        entry: JavaScriptPageDebuggerProgram,
        max_scope_entries: u32,
    ) -> Result<[JavaScriptPageDebuggerLinkedStackFrame; 2], JavaScriptPageDebuggerError> {
        let child_entry = self.child_program_for_core(
            tab_id,
            document_generation,
            entry.program_handle,
            entry.program_generation,
        )?;
        let mut adapter = ChildLinkedDebuggerAdapter {
            child: &mut self.child,
        };
        let (child_frame, state) =
            match adapter.state(tab_id.as_u64(), document_generation, child_entry, None) {
                Ok(state) => state,
                Err(error) => {
                    self.debugger_linked_frames.remove(&tab_id);
                    return Err(error);
                }
            };
        let PageHostDebuggerLinkedExecutionState::Paused { safe_point } = state else {
            self.debugger_linked_frames.remove(&tab_id);
            return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
        };
        self.capture_core_linked_pause_observed(
            tab_id,
            document_generation,
            child_frame,
            safe_point,
            max_scope_entries,
        )
    }

    fn capture_core_linked_pause_observed(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        child_frame: PageHostDebuggerLinkedFrame,
        safe_point: PageHostDebuggerSafePoint,
        max_scope_entries: u32,
    ) -> Result<[JavaScriptPageDebuggerLinkedStackFrame; 2], JavaScriptPageDebuggerError> {
        let child_stack = match (ChildLinkedDebuggerAdapter {
            child: &mut self.child,
        })
        .stack(child_frame, max_scope_entries)
        {
            Ok(stack) => stack,
            Err(error) => {
                self.debugger_linked_frames.remove(&tab_id);
                return Err(error);
            }
        };
        if child_stack.frames[0].safe_point != safe_point {
            self.debugger_linked_frames.remove(&tab_id);
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let child_programs = [child_frame.dependency_program, child_frame.entry_program];
        let programs = child_programs
            .map(|program| self.core_program_for_child(tab_id, document_generation, program));
        let [dependency, caller] = programs;
        let programs = match (dependency, caller) {
            (Ok(dependency), Ok(caller)) => [dependency, caller],
            (Err(error), _) | (_, Err(error)) => {
                self.debugger_linked_frames.remove(&tab_id);
                return Err(error);
            }
        };
        if let Some(active) = self.debugger_linked_frames.get_mut(&tab_id) {
            if active.child == child_frame
                && active.frames.iter().enumerate().all(|(index, frame)| {
                    frame.frame.program_handle == programs[index].program_handle
                        && frame.frame.program_generation == programs[index].program_generation
                        && frame.safe_point.code_unit_ordinal
                            == child_stack.frames[index].safe_point.code_unit_ordinal
                        && frame.safe_point.bytecode_offset
                            == child_stack.frames[index].safe_point.bytecode_offset
                })
            {
                active.child_stack = child_stack;
                return Ok(active.frames);
            }
        }
        let core_instance = core_debugger_instance()?;
        let mint_frame = |index: usize| {
            let frame_handle = NEXT_CORE_DEBUGGER_FRAME_HANDLE
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                    current.checked_add(1)
                })
                .map_err(|_| JavaScriptPageDebuggerError::ResourceLimit)?;
            let point = child_stack.frames[index].safe_point;
            Ok(JavaScriptPageDebuggerLinkedStackFrame {
                frame: JavaScriptPageDebuggerFrame {
                    tab_id,
                    document_generation,
                    program_handle: programs[index].program_handle,
                    program_generation: programs[index].program_generation,
                    code_unit_ordinal: point.code_unit_ordinal,
                    core_instance,
                    frame_handle,
                },
                safe_point: JavaScriptPageDebuggerSafePoint {
                    code_unit_ordinal: point.code_unit_ordinal,
                    bytecode_offset: point.bytecode_offset,
                },
            })
        };
        let frames = [mint_frame(0)?, mint_frame(1)?];
        self.debugger_linked_frames.insert(
            tab_id,
            ActiveLinkedDebuggerPause {
                child: child_frame,
                child_stack,
                frames,
            },
        );
        Ok(frames)
    }

    fn core_linked_stack_spans(
        &mut self,
        expected_stack: JavaScriptPageDebuggerLinkedStackSnapshot,
        access: JavaScriptPageDebuggerLinkedSpanAccess,
    ) -> Result<[JavaScriptPageDebuggerStaticMetadataSafePointSpan; 2], JavaScriptPageDebuggerError>
    {
        if !access.granted
            || !access.metadata_receipted.into_iter().all(|receipt| receipt)
            || !access.source_receipted.into_iter().all(|receipt| receipt)
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let top_frame = expected_stack.frames[0].frame;
        let active = self
            .debugger_linked_frames
            .get(&top_frame.tab_id)
            .filter(|active| active.frames == expected_stack.frames)
            .cloned()
            .ok_or(JavaScriptPageDebuggerError::InvalidExecutionState)?;
        if !self.has_core_live_document(top_frame.tab_id, top_frame.document_generation) {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let mut sources = [PageHostDebuggerLinkedSource {
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: 0,
                metadata_generation: 0,
            },
            source_id: 0,
        }; 2];
        for (index, target) in access.targets.iter().enumerate() {
            let frame = active.frames[index];
            if target.program_handle != frame.frame.program_handle
                || target.program_generation != frame.frame.program_generation
                || target.code_unit_ordinal != frame.safe_point.code_unit_ordinal
                || target.bytecode_offset != frame.safe_point.bytecode_offset
            {
                return Err(JavaScriptPageDebuggerError::InvalidSafePoint);
            }
            let child_program = if index == 0 {
                active.child.dependency_program
            } else {
                active.child.entry_program
            };
            sources[index] = PageHostDebuggerLinkedSource {
                metadata: self.child_static_metadata_for_core(
                    top_frame.tab_id,
                    top_frame.document_generation,
                    child_program,
                    target.metadata_handle,
                    target.metadata_generation,
                )?,
                source_id: target.source_id,
            };
        }
        let spans = ChildLinkedDebuggerAdapter {
            child: &mut self.child,
        }
        .spans(active.child, active.child_stack, sources)?;
        Ok(
            spans.map(|span| JavaScriptPageDebuggerStaticMetadataSafePointSpan {
                source_id: span.source_id,
                start_byte: span.start_byte,
                end_byte: span.end_byte,
                coordinates: span.coordinates,
            }),
        )
    }

    fn core_linked_static_scope_relation(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        target: JavaScriptPageDebuggerStaticScopeTarget,
    ) -> Result<JavaScriptPageDebuggerStaticScopeRelation, JavaScriptPageDebuggerError> {
        if !self.debugger_linked_frames_available() {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        let JavaScriptPageDebuggerStaticScopeTarget::Linked {
            metadata,
            expected_stack,
            frame_index: 1,
            scope_entry,
        } = target
        else {
            return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
        };
        let active_before = self
            .debugger_linked_frames
            .get(&tab_id)
            .filter(|active| active.frames == expected_stack.frames)
            .cloned()
            .ok_or(JavaScriptPageDebuggerError::InvalidExecutionState)?;
        if !self.has_core_live_document(tab_id, document_generation)
            || active_before.frames.iter().any(|frame| {
                frame.frame.tab_id != tab_id
                    || frame.frame.document_generation != document_generation
            })
            || !active_before
                .child_stack
                .is_well_formed(active_before.child)
            || active_before
                .child_stack
                .frames
                .iter()
                .any(|frame| frame.scope_truncated)
        {
            return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
        }
        for index in 0..2 {
            let frame = active_before.frames[index];
            let child_program = self.child_program_for_core(
                tab_id,
                document_generation,
                frame.frame.program_handle,
                frame.frame.program_generation,
            )?;
            let expected_program = if index == 0 {
                active_before.child.dependency_program
            } else {
                active_before.child.entry_program
            };
            if child_program != expected_program {
                return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
            }
        }
        let root = &active_before.child_stack.frames[1];
        let child_scope_entry = PageHostDebuggerScopeEntry {
            slot_ordinal: scope_entry.slot_ordinal,
            scope_depth: scope_entry.scope_depth,
        };
        if root
            .scope_entries
            .iter()
            .filter(|entry| entry.slot_ordinal == child_scope_entry.slot_ordinal)
            .count()
            != 1
            || !root.scope_entries.contains(&child_scope_entry)
        {
            return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
        }
        let entry = JavaScriptPageDebuggerProgram {
            program_handle: active_before.frames[1].frame.program_handle,
            program_generation: active_before.frames[1].frame.program_generation,
        };
        let observed = self.capture_core_linked_pause(
            tab_id,
            document_generation,
            entry,
            active_before.child_stack.max_scope_entries,
        )?;
        let active_after = self
            .debugger_linked_frames
            .get(&tab_id)
            .filter(|active| {
                observed == expected_stack.frames
                    && active.child == active_before.child
                    && active.child_stack == active_before.child_stack
            })
            .ok_or(JavaScriptPageDebuggerError::InvalidExecutionState)?;
        let child_metadata = self.child_static_metadata_for_core(
            tab_id,
            document_generation,
            active_after.child.entry_program,
            metadata.metadata_handle,
            metadata.metadata_generation,
        )?;
        let child_target = PageHostDebuggerStaticScopeTarget::Linked {
            frame: active_after.child,
            expected_stack: Box::new(active_after.child_stack.clone()),
            frame_index: 1,
            metadata: child_metadata,
            scope_entry: child_scope_entry,
        };
        let symbol_type = (ChildStaticScopeAdapter {
            child: &mut self.child,
        })
        .describe(child_target)?;
        Ok(JavaScriptPageDebuggerStaticScopeRelation {
            target,
            symbol_type: JavaScriptPageDebuggerStaticMetadataSymbolType {
                symbol_id: symbol_type.symbol_id,
                type_id: symbol_type.type_id,
            },
        })
    }
}

impl<C> OutOfProcessJavaScriptPageExecutor<C> {
    /// Creates an executor around a caller-owned private child connection.
    /// Tests may supply a transport double; production uses
    /// [`PageHostConnection`].
    pub fn new(child: C) -> Self {
        Self {
            child,
            external_source_authorizer: None,
            native_debugger_execution_control: false,
            hold_pending_debugger_execution_once: false,
            debugger_execution_deferrals: BTreeMap::new(),
            live_documents: BTreeMap::new(),
            realm_stats: BTreeMap::new(),
            debugger_programs: BTreeMap::new(),
            debugger_nested_frames: BTreeMap::new(),
            debugger_linked_frames: BTreeMap::new(),
            next_debugger_program_handle: CORE_CHILD_DEBUGGER_ID_NAMESPACE_START,
            next_debugger_program_generation: CORE_CHILD_DEBUGGER_ID_NAMESPACE_START,
            debugger_static_metadata: BTreeMap::new(),
            next_debugger_metadata_handle: CORE_CHILD_DEBUGGER_METADATA_ID_NAMESPACE_START,
            next_debugger_metadata_generation: CORE_CHILD_DEBUGGER_METADATA_ID_NAMESPACE_START,
            reports: VecDeque::new(),
            blue_ts_reports: VecDeque::new(),
        }
    }

    /// Creates a test or core-selected child route that defers newly admitted
    /// documents by one lifecycle turn for exact root-classic debugger arms.
    pub fn new_with_debugger_execution_control(child: C) -> Self {
        let mut executor = Self::new(child);
        executor.enable_debugger_execution_control();
        executor
    }

    /// Enables the bounded root-classic debugger lifecycle on an executor
    /// that was already constructed by a trusted core startup path. This is
    /// intentionally a host-construction choice: callers still need the
    /// separate private debugger transport, and page/front-end traffic never
    /// receives this executor or a capability selector for it.
    ///
    /// Keeping this as a construction-time transformation lets the one fixed
    /// external-source authorizer and the one fixed debugger lifecycle compose
    /// without adding a page-controlled profile or a second child connection.
    pub fn enable_debugger_execution_control(&mut self) {
        self.native_debugger_execution_control = true;
    }

    /// Wraps a caller-owned child connection and the only external-source
    /// authority it may use. The authorizer is intentionally installed only
    /// here and exposed only through immutable authorization calls.
    pub fn with_external_source_authorizer(
        child: C,
        authorizer: impl OutOfProcessPageScriptSourceAuthorizer + 'static,
    ) -> Self {
        Self {
            child,
            external_source_authorizer: Some(Box::new(authorizer)),
            native_debugger_execution_control: false,
            hold_pending_debugger_execution_once: false,
            debugger_execution_deferrals: BTreeMap::new(),
            live_documents: BTreeMap::new(),
            realm_stats: BTreeMap::new(),
            debugger_programs: BTreeMap::new(),
            debugger_nested_frames: BTreeMap::new(),
            debugger_linked_frames: BTreeMap::new(),
            next_debugger_program_handle: CORE_CHILD_DEBUGGER_ID_NAMESPACE_START,
            next_debugger_program_generation: CORE_CHILD_DEBUGGER_ID_NAMESPACE_START,
            debugger_static_metadata: BTreeMap::new(),
            next_debugger_metadata_handle: CORE_CHILD_DEBUGGER_METADATA_ID_NAMESPACE_START,
            next_debugger_metadata_generation: CORE_CHILD_DEBUGGER_METADATA_ID_NAMESPACE_START,
            reports: VecDeque::new(),
            blue_ts_reports: VecDeque::new(),
        }
    }

    /// Drains only one tab's source-free results, preserving other tabs'
    /// records just as the in-process opt-in executor does.
    pub fn drain_reports_for_tab(&mut self, tab_id: TabId) -> Vec<JavaScriptPageExecutionReport> {
        let mut reports = Vec::new();
        let mut remaining = VecDeque::with_capacity(self.reports.len());
        while let Some(report) = self.reports.pop_front() {
            if report_tab_id(&report) == tab_id.as_u64() {
                reports.push(report);
            } else {
                remaining.push_back(report);
            }
        }
        self.reports = remaining;
        reports
    }

    /// Drains only this tab's explicit BlueTS outcomes. The private host
    /// executes them in the same BlueJS realm as JavaScript, but the public
    /// report lane remains language-specific and source-free.
    pub fn drain_blue_ts_reports_for_tab(
        &mut self,
        tab_id: TabId,
    ) -> Vec<BlueTsPageExecutionReport> {
        let mut reports = Vec::new();
        let mut remaining = VecDeque::with_capacity(self.blue_ts_reports.len());
        while let Some(report) = self.blue_ts_reports.pop_front() {
            if blue_ts_report_tab_id(&report) == tab_id.as_u64() {
                reports.push(report);
            } else {
                remaining.push_back(report);
            }
        }
        self.blue_ts_reports = remaining;
        reports
    }

    /// Returns the wrapped private child client for setup/teardown code that
    /// owns the transport lifecycle. It does not expose a BlueJS VM or source
    /// stored in the child.
    pub fn into_child(self) -> C {
        self.child
    }

    /// Returns the aggregate accounting cached for this tab's exact current
    /// child realm. The record is core-only and is discarded before a caller
    /// can observe it after document replacement, tab close, or a malformed
    /// child response.
    pub fn realm_stats(&self, tab_id: TabId) -> Option<&PageHostRealmStats> {
        let live_document = self.live_documents.get(&tab_id)?;
        let stats = self.realm_stats.get(&tab_id)?;
        (stats.document_generation == live_document.document_generation).then_some(stats)
    }
}

impl<C: PageHostClient> OutOfProcessJavaScriptPageExecutor<C> {
    /// Reads the authenticated child's actual live-realm usage as one
    /// core-only snapshot. Only realms with a validated per-realm accounting
    /// receipt count as live: the document table also retains failed attempts
    /// to prevent retries. This does not replace the owner's conservative
    /// reservation policy or expose accounting through public IPC.
    pub fn child_stats(&mut self) -> io::Result<PageHostChildStats> {
        let reply = self.child.child_stats()?;
        let PageHostReply::ChildStats(stats) = reply else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "page-host child returned the wrong accounting reply",
            ));
        };
        if !stats.is_well_formed()
            || usize::try_from(stats.realm_count).ok() != Some(self.realm_stats.len())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "page-host child returned invalid aggregate accounting",
            ));
        }
        Ok(stats)
    }

    /// Synchronizes loaded tabs to the child. A page failure becomes only a
    /// bounded source-free execution record; a transport failure also leaves
    /// the core render/session loop alive and is reported without exposing a
    /// child error string. A future launcher restart protocol is distinct work.
    pub fn synchronize_and_execute(&mut self, tabs: &TabManager) -> io::Result<()> {
        self.close_removed_tabs(tabs);
        // Existing documents advance before newly observed documents are
        // admitted. This gives the debugger request dispatcher one full
        // session boundary to discover and arm an exact child root point.
        if self.native_debugger_execution_control
            && !std::mem::take(&mut self.hold_pending_debugger_execution_once)
        {
            self.advance_debugger_executions(|child, tab_id, generation| {
                child.advance_debugger_execution(tab_id.as_u64(), generation)
            });
        }
        for tab_id in tabs.ids() {
            let Some(page) = tabs.get(tab_id) else {
                continue;
            };
            let Some(identity) = live_page_identity(page) else {
                self.close_page(tab_id);
                continue;
            };
            if self.live_documents.get(&tab_id) == Some(&identity) {
                continue;
            }
            self.synchronize_document(tab_id, page, identity);
        }
        Ok(())
    }

    /// Core-session variant for the supervised child. Prepare each immutable
    /// source graph while borrowing the page, release that borrow, then keep
    /// the same session thread available for exact-document DOM requests
    /// until the child returns its page-host acknowledgement.
    pub fn synchronize_and_execute_serving_script(
        &mut self,
        tabs: &mut TabManager,
        script_requests: &ScriptRequestReceiver,
    ) -> io::Result<()> {
        self.close_removed_tabs(tabs);
        if self.native_debugger_execution_control
            && !std::mem::take(&mut self.hold_pending_debugger_execution_once)
        {
            self.advance_debugger_executions(|child, tab_id, generation| {
                let target = ScriptDocumentTarget {
                    tab_id: tab_id.as_u64(),
                    document_generation: generation,
                };
                let mut remaining = MAX_NESTED_SCRIPT_REQUESTS_PER_WAIT;
                let mut pump = || {
                    pump_script_requests_during_child_wait(
                        script_requests,
                        tabs,
                        target,
                        &mut remaining,
                    )
                };
                child.advance_debugger_execution_with_script_pump(
                    tab_id.as_u64(),
                    generation,
                    &mut pump,
                )
            });
        }
        let tab_ids: Vec<_> = tabs.ids().collect();
        for tab_id in tab_ids {
            let Some(page) = tabs.get(tab_id) else {
                continue;
            };
            let Some(identity) = live_page_identity(page) else {
                self.close_page(tab_id);
                continue;
            };
            if self.live_documents.get(&tab_id) == Some(&identity) {
                continue;
            }
            let Some(prepared) = self.prepare_document(tab_id, page, &identity) else {
                continue;
            };
            let target = ScriptDocumentTarget {
                tab_id: tab_id.as_u64(),
                document_generation: identity.document_generation,
            };
            let mut remaining = MAX_NESTED_SCRIPT_REQUESTS_PER_WAIT;
            let mut pump = || {
                pump_script_requests_during_child_wait(
                    script_requests,
                    tabs,
                    target,
                    &mut remaining,
                )
            };
            let result = self
                .child
                .synchronize_document_with_script_pump(prepared.document, &mut pump);
            self.finish_document(tab_id, identity, prepared.outcome, result);
        }
        Ok(())
    }

    fn close_removed_tabs(&mut self, tabs: &TabManager) {
        let removed: Vec<_> = self
            .live_documents
            .keys()
            .copied()
            .filter(|tab_id| tabs.get(*tab_id).is_none())
            .collect();
        for tab_id in removed {
            self.close_page(tab_id);
        }
    }

    fn close_page(&mut self, tab_id: TabId) {
        if let Some(document) = self.live_documents.remove(&tab_id) {
            let _ = self
                .child
                .close_realm(tab_id.as_u64(), document.document_generation);
        }
        self.debugger_execution_deferrals.remove(&tab_id);
        self.realm_stats.remove(&tab_id);
        self.debugger_programs.remove(&tab_id);
        self.debugger_nested_frames.remove(&tab_id);
        self.debugger_linked_frames.remove(&tab_id);
        self.debugger_static_metadata.remove(&tab_id);
    }

    /// A failed synchronization may still have installed the candidate realm
    /// before the child sent a malformed acknowledgement. Close both the
    /// previously trusted generation and the attempted successor: a child
    /// with exact-generation close semantics will reject the stale request
    /// and discard whichever generation is actually live.
    fn close_failed_document(&mut self, tab_id: TabId, candidate_generation: u64) {
        let predecessor_generation = self
            .live_documents
            .get(&tab_id)
            .map(|document| document.document_generation);
        self.close_page(tab_id);
        if predecessor_generation != Some(candidate_generation) {
            let _ = self
                .child
                .close_realm(tab_id.as_u64(), candidate_generation);
        }
    }

    fn synchronize_document(&mut self, tab_id: TabId, page: &Page, identity: LiveDocument) {
        let Some(prepared) = self.prepare_document(tab_id, page, &identity) else {
            return;
        };
        let result = self.child.synchronize_document(prepared.document);
        self.finish_document(tab_id, identity, prepared.outcome, result);
    }

    fn prepare_document(
        &mut self,
        tab_id: TabId,
        page: &Page,
        identity: &LiveDocument,
    ) -> Option<PreparedDocumentSync> {
        // A successor can never inherit accounting from its predecessor while
        // its child acknowledgement is still in flight.
        self.realm_stats.remove(&tab_id);
        self.debugger_nested_frames.remove(&tab_id);
        self.debugger_linked_frames.remove(&tab_id);
        self.debugger_static_metadata.remove(&tab_id);
        let declarations = page.combined_page_script_declarations();
        let snapshot = match core_document_snapshot(page, identity) {
            Ok(snapshot) => snapshot,
            Err(()) => {
                // Do not leave the prior generation runnable after the core
                // rejected the successor's snapshots. Treat this just like a
                // child admission failure: it is bounded, source-free, and
                // cannot become a retry loop for one immutable document.
                self.close_page(tab_id);
                let (reports, blue_ts_reports) =
                    binding_contract_rejections(tab_id, identity.document_generation, declarations);
                for report in reports {
                    self.push_report(report);
                }
                for report in blue_ts_reports {
                    self.push_blue_ts_report(report);
                }
                self.debugger_programs.remove(&tab_id);
                self.debugger_nested_frames.remove(&tab_id);
                self.debugger_linked_frames.remove(&tab_id);
                self.debugger_static_metadata.remove(&tab_id);
                self.live_documents.insert(tab_id, identity.clone());
                return None;
            }
        };
        let document_url = page
            .url()
            .expect("a page with a live child identity always has a URL");
        let (document, local_reports, local_blue_ts_reports) = authorized_document(
            tab_id,
            identity,
            snapshot,
            document_url,
            declarations,
            self.external_source_authorizer.as_deref(),
            self.native_debugger_execution_control,
        );
        let inline_scripts: Vec<_> = document
            .scripts
            .iter()
            .map(|script| (script.ordinal, script.language, script.kind))
            .collect();
        Some(PreparedDocumentSync {
            document,
            outcome: PendingDocumentOutcome {
                local_reports,
                local_blue_ts_reports,
                inline_scripts,
            },
        })
    }

    fn finish_document(
        &mut self,
        tab_id: TabId,
        identity: LiveDocument,
        outcome: PendingDocumentOutcome,
        result: io::Result<PageHostReply>,
    ) {
        let PendingDocumentOutcome {
            mut local_reports,
            mut local_blue_ts_reports,
            inline_scripts,
        } = outcome;
        let mut child_synchronized = false;
        match result {
            Ok(PageHostReply::Synchronized {
                tab_id: reply_tab_id,
                document_generation: reply_generation,
                reports,
                ..
            }) if reply_tab_id == tab_id.as_u64()
                && reply_generation == identity.document_generation =>
            {
                child_synchronized = true;
                for report in reports {
                    match child_report(report) {
                        ChildExecutionReport::JavaScript(report) => local_reports.push(report),
                        ChildExecutionReport::BlueTs(report) => local_blue_ts_reports.push(report),
                    }
                }
            }
            Ok(PageHostReply::Error { code, .. }) => {
                // A child-side request rejection must not leave an old realm
                // runnable under a replaced core document. The child may
                // also have activated the successor before returning an
                // error, so both exact generations must be attempted.
                self.close_failed_document(tab_id, identity.document_generation);
                let category = child_error_category(code);
                for (ordinal, language, kind) in inline_scripts {
                    push_child_failure_report(
                        (&mut local_reports, &mut local_blue_ts_reports),
                        tab_id,
                        identity.document_generation,
                        ordinal,
                        language,
                        kind,
                        category,
                    );
                }
            }
            // A same-shape acknowledgement for a different realm is not a
            // synchronization success. In particular, it cannot seed a core
            // debugger deferral budget or retain a public/private program
            // mapping that could later be mistaken for this replacement.
            Ok(_) | Err(_) => {
                self.close_failed_document(tab_id, identity.document_generation);
                for (ordinal, language, kind) in inline_scripts {
                    push_child_failure_report(
                        (&mut local_reports, &mut local_blue_ts_reports),
                        tab_id,
                        identity.document_generation,
                        ordinal,
                        language,
                        kind,
                        "out-of-process JavaScript host is unavailable",
                    );
                }
            }
        }
        if child_synchronized && !self.cache_realm_stats(tab_id, &identity) {
            // A successful document acknowledgement without a matching
            // aggregate record is not a core-owned live realm. Best-effort
            // close names the newly acknowledged tuple, never its predecessor.
            let _ = self
                .child
                .close_realm(tab_id.as_u64(), identity.document_generation);
            child_synchronized = false;
        }
        local_reports.sort_by_key(report_ordinal);
        local_blue_ts_reports.sort_by_key(blue_ts_report_ordinal);
        for report in local_reports {
            self.push_report(report);
        }
        for report in local_blue_ts_reports {
            self.push_blue_ts_report(report);
        }
        if self.native_debugger_execution_control && child_synchronized {
            self.debugger_execution_deferrals.insert(
                tab_id,
                DebuggerExecutionDeferral {
                    document_generation: identity.document_generation,
                    remaining: MAX_OOP_DEBUGGER_EXECUTION_DEFERRALS_PER_DOCUMENT,
                },
            );
        } else {
            self.debugger_execution_deferrals.remove(&tab_id);
        }
        self.debugger_programs.remove(&tab_id);
        self.debugger_nested_frames.remove(&tab_id);
        self.debugger_linked_frames.remove(&tab_id);
        self.debugger_static_metadata.remove(&tab_id);
        self.live_documents.insert(tab_id, identity);
    }

    /// Queries and validates only the aggregate record for the exact realm
    /// just acknowledged by the child. A test transport that does not opt in
    /// to stats remains usable, but any production reply/error other than that
    /// explicit unsupported default clears the cache and causes the new realm
    /// to be closed rather than retaining unverified accounting.
    fn cache_realm_stats(&mut self, tab_id: TabId, identity: &LiveDocument) -> bool {
        match self
            .child
            .realm_stats(tab_id.as_u64(), identity.document_generation)
        {
            Ok(PageHostReply::RealmStats(stats))
                if stats.tab_id == tab_id.as_u64()
                    && stats.document_generation == identity.document_generation
                    && stats.is_well_formed() =>
            {
                self.realm_stats.insert(tab_id, stats);
                true
            }
            Err(error) if error.kind() == io::ErrorKind::Unsupported => true,
            Ok(_) | Err(_) => {
                self.realm_stats.remove(&tab_id);
                false
            }
        }
    }

    fn push_report(&mut self, report: JavaScriptPageExecutionReport) {
        if self.reports.len() == MAX_EXECUTION_REPORTS {
            self.reports.pop_front();
        }
        self.reports.push_back(report);
    }

    fn push_blue_ts_report(&mut self, report: BlueTsPageExecutionReport) {
        if self.blue_ts_reports.len() == MAX_EXECUTION_REPORTS {
            self.blue_ts_reports.pop_front();
        }
        self.blue_ts_reports.push_back(report);
    }

    fn advance_debugger_executions(
        &mut self,
        mut advance: impl FnMut(&mut C, TabId, u64) -> io::Result<PageHostReply>,
    ) {
        let documents: Vec<_> = self
            .live_documents
            .iter()
            .map(|(&tab_id, document)| (tab_id, document.document_generation))
            .collect();
        for (tab_id, document_generation) in documents {
            let reply = advance(&mut self.child, tab_id, document_generation);
            let Ok(PageHostReply::DebuggerExecutionAdvanced {
                tab_id: reply_tab_id,
                document_generation: reply_generation,
                reports,
            }) = reply
            else {
                // Child execution control is an all-or-nothing realm owner.
                // A malformed/lost reply cannot leave a core-visible document
                // that might be assumed to be safely paused.
                self.close_page(tab_id);
                continue;
            };
            if reply_tab_id != tab_id.as_u64() || reply_generation != document_generation {
                self.close_page(tab_id);
                continue;
            }
            for report in reports {
                match child_report(report) {
                    ChildExecutionReport::JavaScript(report) => self.push_report(report),
                    ChildExecutionReport::BlueTs(report) => self.push_blue_ts_report(report),
                }
            }
        }
    }
}

impl<C: PageHostClient> PageJavaScriptExecutor for OutOfProcessJavaScriptPageExecutor<C> {
    fn synchronize_and_execute(&mut self, tabs: &TabManager) -> io::Result<()> {
        Self::synchronize_and_execute(self, tabs)
    }

    fn synchronize_and_execute_serving_script(
        &mut self,
        tabs: &mut TabManager,
        script_requests: Option<&ScriptRequestReceiver>,
    ) -> io::Result<()> {
        match script_requests {
            Some(script_requests) => {
                Self::synchronize_and_execute_serving_script(self, tabs, script_requests)
            }
            None => Self::synchronize_and_execute(self, tabs),
        }
    }

    fn dispatch_click_serving_script(
        &mut self,
        tabs: &mut TabManager,
        tab_id: TabId,
        node_id: u64,
        script_requests: Option<&ScriptRequestReceiver>,
    ) -> io::Result<Option<bool>> {
        let Some(page) = tabs.get(tab_id) else {
            return Ok(None);
        };
        let Some(identity) = live_page_identity(page) else {
            return Ok(None);
        };
        if self.live_documents.get(&tab_id) != Some(&identity) {
            return Ok(None);
        }
        let Some(script_requests) = script_requests else {
            return Ok(None);
        };
        // The hit tester uses core DOM NodeIds. The child listener registry
        // uses document-bound script handles; convert before crossing the
        // page-host channel so equal NodeIds in separate tabs cannot alias.
        let Some(node_handle) = tabs
            .get_mut(tab_id)
            .and_then(|page| page.script_handle_for_raw_node(node_id))
        else {
            return Ok(None);
        };
        let target = ScriptDocumentTarget {
            tab_id: tab_id.as_u64(),
            document_generation: identity.document_generation,
        };
        let mut remaining = MAX_NESTED_SCRIPT_REQUESTS_PER_WAIT;
        let mut pump = || {
            pump_script_requests_during_child_wait(script_requests, tabs, target, &mut remaining)
        };
        match self.child.dispatch_click_with_script_pump(
            target.tab_id,
            target.document_generation,
            node_handle,
            &mut pump,
        )? {
            PageHostReply::ClickDispatched {
                tab_id: reply_tab_id,
                document_generation: reply_generation,
                default_prevented,
            } if reply_tab_id == target.tab_id
                && reply_generation == target.document_generation =>
            {
                Ok(Some(default_prevented))
            }
            _ => Err(io::Error::other("page-host click response invalid")),
        }
    }

    fn drain_reports_for_tab(&mut self, tab_id: TabId) -> Vec<JavaScriptPageExecutionReport> {
        Self::drain_reports_for_tab(self, tab_id)
    }

    fn supports_blue_ts_page_execution(&self) -> bool {
        true
    }

    fn drain_blue_ts_reports_for_tab(&mut self, tab_id: TabId) -> Vec<BlueTsPageExecutionReport> {
        Self::drain_blue_ts_reports_for_tab(self, tab_id)
    }

    fn debugger_locations(&mut self) -> Option<&mut (dyn PageJavaScriptDebuggerLocations + '_)> {
        Some(self)
    }

    fn hold_pending_debugger_execution_once(&mut self) {
        if self.native_debugger_execution_control {
            let mut preserved_a_live_document = false;
            for (tab_id, document) in &self.live_documents {
                let Some(deferral) = self.debugger_execution_deferrals.get_mut(tab_id) else {
                    continue;
                };
                if deferral.document_generation != document.document_generation
                    || deferral.remaining == 0
                {
                    continue;
                }
                deferral.remaining -= 1;
                preserved_a_live_document = true;
            }
            self.hold_pending_debugger_execution_once = preserved_a_live_document;
        }
    }
}

mod client;
pub use client::PageHostClient;
mod connection;
pub use connection::PageHostConnection;
mod child_reply_validation;
use child_reply_validation::*;
mod document_admission;
use document_admission::*;
mod debugger;
mod debugger_state;
use debugger_state::*;
mod private_debugger_adapter;
mod report_mapping;
use private_debugger_adapter::{
    ChildLinkedDebuggerAdapter, ChildLinkedDebuggerStatus, ChildStaticScopeAdapter,
};
use report_mapping::*;

#[cfg(test)]
mod tests;
