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
    JavaScriptPageDebuggerError, JavaScriptPageDebuggerExecutionState, JavaScriptPageDebuggerFrame,
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
    JavaScriptPageDebuggerStaticMetadataTypeTarget, JavaScriptPageExecutionReport,
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
    PageHostDebuggerExecutionState, PageHostDebuggerFrame, PageHostDebuggerMetadataHandle,
    PageHostDebuggerProgram, PageHostDebuggerSafePoint, PageHostDebuggerStackSnapshot,
    PageHostDocument, PageHostDocumentSnapshot, PageHostErrorCode, PageHostModuleGraph,
    PageHostRealmStats, PageHostReply, PageHostRequest, PageHostScript, PageHostScriptKind,
    PageHostScriptLanguage, PageHostScriptOutcome, PageHostSource, PageHostStaticResolution,
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
/// The same numeric handle may be minted by a successor core. Bind it to a
/// distinct core instance as well: overlapping processes have different PIDs,
/// and 96 random bits prevent a later PID reuse from recreating this identity.
/// If OS randomness is unavailable, nested-frame minting fails closed.
static CORE_DEBUGGER_INSTANCE: OnceLock<Option<[u8; 16]>> = OnceLock::new();

fn core_debugger_instance() -> Result<[u8; 16], JavaScriptPageDebuggerError> {
    (*CORE_DEBUGGER_INSTANCE.get_or_init(|| {
        let mut identity = [0_u8; 16];
        identity[..4].copy_from_slice(&std::process::id().to_le_bytes());
        getrandom::fill(&mut identity[4..]).ok()?;
        Some(identity)
    }))
    .ok_or(JavaScriptPageDebuggerError::ResourceLimit)
}

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

/// One connected, authenticated private page-host transport. It intentionally
/// owns no child process: `blueice-launcher` remains the supervisor and must
/// reap the child after core disconnects or exits.
pub struct PageHostConnection {
    stream: UnixStream,
}

impl PageHostConnection {
    /// Connects to a launcher-created private child socket and completes the
    /// versioned capability handshake. The caller must obtain both values from its
    /// launcher owner; a page, frontend client, or script never receives this
    /// configuration.
    pub fn connect(socket_path: &Path, session_token: &str) -> io::Result<Self> {
        let mut stream = UnixStream::connect(socket_path)?;
        page_host::write_page_host_request(
            &mut stream,
            &PageHostRequest::Hello {
                protocol_version: page_host::PAGE_HOST_PROTOCOL_VERSION,
                session_token: session_token.to_string(),
            },
        )?;
        match page_host::read_page_host_reply(&mut stream)? {
            PageHostReply::HelloAck {
                protocol_version: page_host::PAGE_HOST_PROTOCOL_VERSION,
            } => Ok(Self { stream }),
            reply => Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("BlueJS child host rejected core handshake: {reply:?}"),
            )),
        }
    }

    fn request(&mut self, request: PageHostRequest) -> io::Result<PageHostReply> {
        page_host::write_page_host_request(&mut self.stream, &request)?;
        page_host::read_page_host_reply(&mut self.stream)
    }

    /// Reads the child result on a transport-only worker while the calling
    /// core session thread remains free to serve this document's DOM calls.
    /// No page, VM, or `TabManager` reference crosses to the reader worker.
    fn request_while_pumping_script(
        &mut self,
        request: PageHostRequest,
        pump: &mut dyn FnMut() -> io::Result<()>,
    ) -> io::Result<PageHostReply> {
        self.request_while_pumping_script_with_timeout(request, pump, CHILD_DOCUMENT_REPLY_WAIT)
    }

    fn request_while_pumping_script_with_timeout(
        &mut self,
        request: PageHostRequest,
        pump: &mut dyn FnMut() -> io::Result<()>,
        wait: Duration,
    ) -> io::Result<PageHostReply> {
        if wait.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "page-host wait must be nonzero",
            ));
        }
        let deadline = Instant::now() + wait;
        self.stream.set_write_timeout(Some(wait)).map_err(|error| {
            io::Error::new(error.kind(), format!("page-host write timeout: {error}"))
        })?;
        if let Err(error) = page_host::write_page_host_request(&mut self.stream, &request) {
            let _ = self.stream.shutdown(Shutdown::Both);
            return Err(error);
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            let _ = self.stream.shutdown(Shutdown::Both);
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "page-host request write exceeded its fixed wait",
            ));
        }
        let mut reader = match self.stream.try_clone() {
            Ok(reader) => reader,
            Err(error) => {
                let _ = self.stream.shutdown(Shutdown::Both);
                return Err(error);
            }
        };
        if let Err(error) = reader.set_read_timeout(Some(remaining)) {
            let _ = self.stream.shutdown(Shutdown::Both);
            return Err(io::Error::new(
                error.kind(),
                format!("page-host read timeout: {error}"),
            ));
        }
        let (reply_sender, reply_receiver) = mpsc::sync_channel(1);
        let reader_task = thread::spawn(move || {
            let _ = reply_sender.send(page_host::read_page_host_reply(&mut reader));
        });
        let result = loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "page-host reply exceeded its fixed wait",
                ));
            }
            match reply_receiver.recv_timeout(remaining.min(CHILD_DOCUMENT_POLL_INTERVAL)) {
                Ok(result) if Instant::now() <= deadline => break result,
                Ok(_) => {
                    break Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "page-host reply exceeded its fixed wait",
                    ));
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    break Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "page-host reply reader disconnected",
                    ));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if let Err(error) = pump() {
                break Err(error);
            }
        };
        if result.is_err() {
            let _ = self.stream.shutdown(Shutdown::Both);
        }
        if reader_task.join().is_err() {
            let _ = self.stream.shutdown(Shutdown::Both);
            return Err(io::Error::other("page-host reply reader panicked"));
        }
        // The socket retains fixed read/write bounds for subsequent control
        // operations. Some Unix platforms reject clearing SO_RCVTIMEO after
        // the peer has closed immediately following a valid reply.
        result
    }
}

/// The small transport surface the lifecycle adapter needs. Keeping this
/// separate from a VM/registry prevents the core from bypassing the child's
/// process boundary and lets focused tests use a recording child peer.
pub trait PageHostClient {
    fn synchronize_document(&mut self, document: PageHostDocument) -> io::Result<PageHostReply>;
    fn dispatch_click_with_script_pump(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _node_id: u64,
        _pump: &mut dyn FnMut() -> io::Result<()>,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement click dispatch",
        ))
    }
    /// Test transports can keep a simple request/reply path. The real socket
    /// transport overrides this to release the session thread for DOM calls.
    fn synchronize_document_with_script_pump(
        &mut self,
        document: PageHostDocument,
        _pump: &mut dyn FnMut() -> io::Result<()>,
    ) -> io::Result<PageHostReply> {
        self.synchronize_document(document)
    }
    fn close_realm(&mut self, tab_id: u64, document_generation: u64) -> io::Result<PageHostReply>;

    /// Whether this transport peer implements the v5 exact breakpoint
    /// configuration operations. Test doubles must opt in; location discovery
    /// alone must not make the public capability report promise configuration.
    fn debugger_breakpoint_configuration_available(&self) -> bool {
        false
    }

    /// Returns a source-free child realm acknowledgement for the exact core
    /// tab/document tuple. A transport double must opt in explicitly; the
    /// default keeps debugger locations unavailable rather than fabricating a
    /// remote realm.
    fn debugger_realm_stats(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement debugger locations",
        ))
    }

    /// Returns one source-free aggregate accounting record for the exact
    /// child-owned realm. The default reuses the legacy debugger liveness
    /// query so focused transport doubles do not accidentally gain a new
    /// capability; production's authenticated child always implements it.
    fn realm_stats(&mut self, tab_id: u64, document_generation: u64) -> io::Result<PageHostReply> {
        self.debugger_realm_stats(tab_id, document_generation)
    }

    /// An optional private child-wide actual-usage query. Test doubles deny
    /// it by default; a page or debugger cannot invoke this transport.
    fn child_stats(&mut self) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement child-wide accounting",
        ))
    }

    /// Lists private child program IDs for one exact realm.
    fn debugger_programs(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement debugger locations",
        ))
    }

    /// Whether this authenticated private peer implements the v7 BlueTS
    /// static-metadata handle inventory. This says only that the child can
    /// return source-free opaque handles; public debugger policy must still
    /// separately authorize and re-mint any metadata-facing capability.
    fn debugger_bluets_metadata_available(&self) -> bool {
        false
    }

    /// Whether this authenticated private peer supports a bounded summary for
    /// an existing BlueTS metadata handle. It is deliberately separate from
    /// the inventory signal so a transport double cannot gain a read surface
    /// merely by implementing handle enumeration.
    fn debugger_bluets_metadata_summary_available(&self) -> bool {
        false
    }

    /// Whether this peer supports the opaque-handle-bound aggregate summary
    /// of a verified direct-lowering map. Per-entry source and bytecode data
    /// remain private to the child.
    fn debugger_bluets_metadata_lowering_summary_available(&self) -> bool {
        false
    }

    /// Whether this peer supports the metadata-handle-bound inventory of
    /// compiler-minted source IDs. The inventory has no source detail.
    fn debugger_bluets_metadata_sources_available(&self) -> bool {
        false
    }

    /// Whether this peer implements the separately authorized source-free
    /// module-identity and SHA-256 provenance operation.
    fn debugger_bluets_metadata_source_provenance_available(&self) -> bool {
        false
    }

    /// Whether this peer supports the metadata-handle-bound inventory of
    /// compiler-minted type IDs. Type display remains a later operation.
    fn debugger_bluets_metadata_types_available(&self) -> bool {
        false
    }

    /// Whether this peer implements a bounded display lookup for one exact
    /// type ID. The core must still apply the independent public policy and
    /// same-stream type-ID receipt before it may call this private operation.
    fn debugger_bluets_metadata_type_display_available(&self) -> bool {
        false
    }

    /// Whether this peer supports the metadata-handle-bound inventory of
    /// compiler-minted symbol IDs. Symbol record detail remains unavailable.
    fn debugger_bluets_metadata_symbols_available(&self) -> bool {
        false
    }

    /// Whether this peer supports the metadata-handle-bound inventory of
    /// compiler-minted contract IDs. Contract record detail remains unavailable.
    fn debugger_bluets_metadata_contracts_available(&self) -> bool {
        false
    }

    /// Whether this peer implements a bounded display lookup for one exact
    /// contract ID. The core must still apply independent public policy and
    /// the same-stream contract-ID receipt before this private operation.
    fn debugger_bluets_metadata_contract_display_available(&self) -> bool {
        false
    }

    /// Whether this peer implements a data-only validation against one exact
    /// prior contract ID. The boolean result remains separately default-denied
    /// by core and must not expose plans or structural failure detail.
    fn debugger_bluets_metadata_contract_validation_available(&self) -> bool {
        false
    }

    /// Whether this peer implements a bounded display lookup for one exact
    /// symbol ID. The core must still apply independent public policy and the
    /// same-stream symbol-ID receipt before this private operation is called.
    fn debugger_bluets_metadata_symbol_display_available(&self) -> bool {
        false
    }

    /// Whether this peer implements an exact source-text-free location lookup
    /// for a prior symbol ID. The core separately requires parent, symbol, and
    /// source receipts before it can call this private operation.
    fn debugger_bluets_metadata_symbol_location_available(&self) -> bool {
        false
    }

    fn debugger_bluets_metadata_contract_location_available(&self) -> bool {
        false
    }

    /// Whether this authenticated child can resolve one exact verified safe
    /// point to a private BlueTS byte span. This does not advertise or grant a
    /// public debugger source-map operation.
    fn debugger_bluets_safe_point_span_available(&self) -> bool {
        false
    }

    fn debugger_bluets_source_breakpoint_available(&self) -> bool {
        false
    }

    fn debugger_bluets_source_span_step_available(&self) -> bool {
        false
    }

    /// Whether this private peer can verify an exact symbol/type pair.
    fn debugger_bluets_metadata_symbol_type_available(&self) -> bool {
        false
    }

    /// Whether this private peer can verify an exact symbol/contract pair.
    fn debugger_bluets_metadata_symbol_contract_available(&self) -> bool {
        false
    }

    /// Lists newly child-minted opaque handles only for a live direct-BlueTS
    /// attachment associated with one exact private program. The result has
    /// no source/module/name/type/span/contract payload, and a transport
    /// double must opt in rather than accidentally fabricating that inventory.
    fn debugger_bluets_metadata(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _program: PageHostDebuggerProgram,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement BlueTS debugger metadata inventory",
        ))
    }

    /// Describes one already issued child-private metadata handle. The child
    /// must bind it to the exact program supplied here and return no record
    /// contents, source identity/text, span, symbol, type, contract, VM
    /// object, or value.
    fn debugger_bluets_metadata_summary(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _program: PageHostDebuggerProgram,
        _metadata: PageHostDebuggerMetadataHandle,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement BlueTS debugger metadata summaries",
        ))
    }

    fn debugger_bluets_metadata_lowering_summary(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _program: PageHostDebuggerProgram,
        _metadata: PageHostDebuggerMetadataHandle,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement BlueTS debugger lowering summaries",
        ))
    }

    fn debugger_bluets_metadata_sources(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _program: PageHostDebuggerProgram,
        _metadata: PageHostDebuggerMetadataHandle,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement BlueTS debugger source inventories",
        ))
    }

    fn debugger_bluets_metadata_source_provenance(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _program: PageHostDebuggerProgram,
        _metadata: PageHostDebuggerMetadataHandle,
        _source_id: u32,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement BlueTS debugger source provenance",
        ))
    }

    fn debugger_bluets_metadata_types(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _program: PageHostDebuggerProgram,
        _metadata: PageHostDebuggerMetadataHandle,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement BlueTS debugger type inventories",
        ))
    }

    fn debugger_bluets_metadata_type_display(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _program: PageHostDebuggerProgram,
        _metadata: PageHostDebuggerMetadataHandle,
        _type_id: u32,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement BlueTS debugger type displays",
        ))
    }

    fn debugger_bluets_metadata_symbols(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _program: PageHostDebuggerProgram,
        _metadata: PageHostDebuggerMetadataHandle,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement BlueTS debugger symbol inventories",
        ))
    }

    fn debugger_bluets_metadata_contracts(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _program: PageHostDebuggerProgram,
        _metadata: PageHostDebuggerMetadataHandle,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement BlueTS debugger contract inventories",
        ))
    }

    fn debugger_bluets_metadata_contract_display(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _program: PageHostDebuggerProgram,
        _metadata: PageHostDebuggerMetadataHandle,
        _contract_id: u32,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement BlueTS debugger contract displays",
        ))
    }

    fn debugger_bluets_metadata_contract_validation(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _program: PageHostDebuggerProgram,
        _metadata: PageHostDebuggerMetadataHandle,
        _contract_id: u32,
        _value: CompilerContractValue,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement BlueTS debugger contract validation",
        ))
    }

    fn debugger_bluets_metadata_symbol_display(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _program: PageHostDebuggerProgram,
        _metadata: PageHostDebuggerMetadataHandle,
        _symbol_id: u32,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement BlueTS debugger symbol displays",
        ))
    }

    fn debugger_bluets_metadata_symbol_location(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _program: PageHostDebuggerProgram,
        _metadata: PageHostDebuggerMetadataHandle,
        _symbol_id: u32,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement BlueTS debugger symbol locations",
        ))
    }

    fn debugger_bluets_metadata_contract_location(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _program: PageHostDebuggerProgram,
        _metadata: PageHostDebuggerMetadataHandle,
        _contract_id: u32,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement BlueTS debugger contract locations",
        ))
    }

    fn debugger_bluets_safe_point_span(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _metadata: PageHostDebuggerMetadataHandle,
        _safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement exact BlueTS safe-point spans",
        ))
    }

    fn debugger_bluets_source_breakpoint(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _program: PageHostDebuggerProgram,
        _metadata: PageHostDebuggerMetadataHandle,
        _source_id: u32,
        _source_byte: u32,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement BlueTS source breakpoints",
        ))
    }

    fn debugger_bluets_metadata_symbol_type(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _program: PageHostDebuggerProgram,
        _metadata: PageHostDebuggerMetadataHandle,
        _symbol_id: u32,
        _type_id: u32,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement BlueTS debugger symbol types",
        ))
    }

    fn debugger_bluets_metadata_symbol_contract(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _program: PageHostDebuggerProgram,
        _metadata: PageHostDebuggerMetadataHandle,
        _symbol_id: u32,
        _contract_id: u32,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement BlueTS debugger symbol contracts",
        ))
    }

    /// Lists private child safe points for one exact private program ID.
    fn debugger_safe_points(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _program: PageHostDebuggerProgram,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement debugger locations",
        ))
    }

    /// Revalidates one exact private child safe point.
    fn validate_debugger_safe_point(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement debugger locations",
        ))
    }

    /// Stores an exact child-private breakpoint configuration record. This is
    /// deliberately separate from any child VM interruption capability.
    fn set_debugger_breakpoint(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement debugger breakpoint configuration",
        ))
    }

    /// Lists exact child-private breakpoint records for one realm.
    fn debugger_breakpoints(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement debugger breakpoint configuration",
        ))
    }

    /// Clears one exact child-private breakpoint record.
    fn clear_debugger_breakpoint(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement debugger breakpoint configuration",
        ))
    }

    /// Whether this private peer implements the v6 root-classic lifecycle.
    /// A transport double must opt in explicitly; configuration alone never
    /// advertises pause/resume to the public debugger.
    fn debugger_execution_control_available(&self) -> bool {
        false
    }

    fn debugger_nested_frames_available(&self) -> bool {
        false
    }

    fn debugger_stack_snapshot_available(&self) -> bool {
        false
    }

    fn debugger_stack_snapshot(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _program: PageHostDebuggerProgram,
        _frame: Option<PageHostDebuggerFrame>,
        _max_frames: u32,
        _max_scope_entries: u32,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement debugger stack inspection",
        ))
    }

    fn arm_debugger_nested_safe_point_breakpoint(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement nested debugger control",
        ))
    }

    fn step_debugger_nested_instruction(
        &mut self,
        _frame: PageHostDebuggerFrame,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement nested debugger control",
        ))
    }

    fn resume_debugger_nested_execution(
        &mut self,
        _frame: PageHostDebuggerFrame,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement nested debugger resume",
        ))
    }

    fn arm_debugger_root_safe_point_breakpoint(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement debugger execution control",
        ))
    }

    fn debugger_execution_state(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _program: PageHostDebuggerProgram,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement debugger execution control",
        ))
    }

    fn resume_debugger_execution(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _program: PageHostDebuggerProgram,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement debugger execution control",
        ))
    }

    fn debugger_stepping_available(&self) -> bool {
        false
    }

    fn step_debugger_root_instruction(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _program: PageHostDebuggerProgram,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement debugger root stepping",
        ))
    }

    fn step_debugger_bluets_source_span(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _metadata: PageHostDebuggerMetadataHandle,
        _source_id: u32,
        _safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement BlueTS source-span stepping",
        ))
    }

    fn advance_debugger_execution(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement debugger execution control",
        ))
    }

    fn advance_debugger_execution_with_script_pump(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        _pump: &mut dyn FnMut() -> io::Result<()>,
    ) -> io::Result<PageHostReply> {
        self.advance_debugger_execution(tab_id, document_generation)
    }
}

impl PageHostClient for PageHostConnection {
    fn synchronize_document(&mut self, document: PageHostDocument) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::SynchronizeDocument { document })
    }

    fn synchronize_document_with_script_pump(
        &mut self,
        document: PageHostDocument,
        pump: &mut dyn FnMut() -> io::Result<()>,
    ) -> io::Result<PageHostReply> {
        self.request_while_pumping_script(PageHostRequest::SynchronizeDocument { document }, pump)
    }

    fn dispatch_click_with_script_pump(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        node_id: u64,
        pump: &mut dyn FnMut() -> io::Result<()>,
    ) -> io::Result<PageHostReply> {
        self.request_while_pumping_script(
            PageHostRequest::DispatchClick {
                tab_id,
                document_generation,
                node_id,
            },
            pump,
        )
    }

    fn close_realm(&mut self, tab_id: u64, document_generation: u64) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::CloseRealm {
            tab_id,
            document_generation,
        })
    }

    fn debugger_breakpoint_configuration_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_summary_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_lowering_summary_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_sources_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_source_provenance_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_types_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_type_display_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_symbols_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_contracts_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_contract_display_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_contract_validation_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_symbol_display_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_symbol_location_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_contract_location_available(&self) -> bool {
        true
    }

    fn debugger_bluets_safe_point_span_available(&self) -> bool {
        true
    }

    fn debugger_bluets_source_breakpoint_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_symbol_type_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_symbol_contract_available(&self) -> bool {
        true
    }

    fn debugger_realm_stats(
        &mut self,
        tab_id: u64,
        document_generation: u64,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::GetRealmStats {
            tab_id,
            document_generation,
        })
    }

    fn child_stats(&mut self) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::GetChildStats)
    }

    fn debugger_programs(
        &mut self,
        tab_id: u64,
        document_generation: u64,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::ListDebuggerPrograms {
            tab_id,
            document_generation,
        })
    }

    fn debugger_bluets_metadata(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::ListDebuggerBlueTsMetadata {
            tab_id,
            document_generation,
            program,
        })
    }

    fn debugger_bluets_metadata_summary(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::DescribeDebuggerBlueTsMetadata {
            tab_id,
            document_generation,
            program,
            metadata,
        })
    }

    fn debugger_bluets_metadata_lowering_summary(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    ) -> io::Result<PageHostReply> {
        self.request(
            PageHostRequest::DescribeDebuggerBlueTsMetadataLoweringSummary {
                tab_id,
                document_generation,
                program,
                metadata,
            },
        )
    }

    fn debugger_bluets_metadata_sources(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::ListDebuggerBlueTsMetadataSources {
            tab_id,
            document_generation,
            program,
            metadata,
        })
    }

    fn debugger_bluets_metadata_source_provenance(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        source_id: u32,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::DescribeDebuggerBlueTsMetadataSource {
            tab_id,
            document_generation,
            program,
            metadata,
            source_id,
        })
    }

    fn debugger_bluets_metadata_types(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::ListDebuggerBlueTsMetadataTypes {
            tab_id,
            document_generation,
            program,
            metadata,
        })
    }

    fn debugger_bluets_metadata_type_display(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        type_id: u32,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::DescribeDebuggerBlueTsMetadataType {
            tab_id,
            document_generation,
            program,
            metadata,
            type_id,
        })
    }

    fn debugger_bluets_metadata_symbols(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::ListDebuggerBlueTsMetadataSymbols {
            tab_id,
            document_generation,
            program,
            metadata,
        })
    }

    fn debugger_bluets_metadata_contracts(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::ListDebuggerBlueTsMetadataContracts {
            tab_id,
            document_generation,
            program,
            metadata,
        })
    }

    fn debugger_bluets_metadata_contract_display(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        contract_id: u32,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::DescribeDebuggerBlueTsMetadataContract {
            tab_id,
            document_generation,
            program,
            metadata,
            contract_id,
        })
    }

    fn debugger_bluets_metadata_contract_validation(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        contract_id: u32,
        value: CompilerContractValue,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::ValidateDebuggerBlueTsMetadataContract {
            tab_id,
            document_generation,
            program,
            metadata,
            contract_id,
            value,
        })
    }

    fn debugger_bluets_metadata_symbol_display(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        symbol_id: u32,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::DescribeDebuggerBlueTsMetadataSymbol {
            tab_id,
            document_generation,
            program,
            metadata,
            symbol_id,
        })
    }

    fn debugger_bluets_metadata_symbol_location(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        symbol_id: u32,
    ) -> io::Result<PageHostReply> {
        self.request(
            PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolLocation {
                tab_id,
                document_generation,
                program,
                metadata,
                symbol_id,
            },
        )
    }

    fn debugger_bluets_metadata_contract_location(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        contract_id: u32,
    ) -> io::Result<PageHostReply> {
        self.request(
            PageHostRequest::DescribeDebuggerBlueTsMetadataContractLocation {
                tab_id,
                document_generation,
                program,
                metadata,
                contract_id,
            },
        )
    }

    fn debugger_bluets_safe_point_span(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        metadata: PageHostDebuggerMetadataHandle,
        safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::DescribeDebuggerBlueTsSafePointSpan {
            tab_id,
            document_generation,
            metadata,
            safe_point,
        })
    }

    fn debugger_bluets_source_breakpoint(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        source_id: u32,
        source_byte: u32,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::ResolveDebuggerBlueTsSourceBreakpoint {
            tab_id,
            document_generation,
            program,
            metadata,
            source_id,
            source_byte,
        })
    }

    fn debugger_bluets_metadata_symbol_type(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        symbol_id: u32,
        type_id: u32,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolType {
            tab_id,
            document_generation,
            program,
            metadata,
            symbol_id,
            type_id,
        })
    }

    fn debugger_bluets_metadata_symbol_contract(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        symbol_id: u32,
        contract_id: u32,
    ) -> io::Result<PageHostReply> {
        self.request(
            PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolContract {
                tab_id,
                document_generation,
                program,
                metadata,
                symbol_id,
                contract_id,
            },
        )
    }

    fn debugger_safe_points(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::ListDebuggerSafePoints {
            tab_id,
            document_generation,
            program,
        })
    }

    fn validate_debugger_safe_point(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::ValidateDebuggerSafePoint {
            tab_id,
            document_generation,
            safe_point,
        })
    }

    fn set_debugger_breakpoint(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::SetDebuggerBreakpoint {
            tab_id,
            document_generation,
            safe_point,
        })
    }

    fn debugger_breakpoints(
        &mut self,
        tab_id: u64,
        document_generation: u64,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::ListDebuggerBreakpoints {
            tab_id,
            document_generation,
        })
    }

    fn clear_debugger_breakpoint(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::ClearDebuggerBreakpoint {
            tab_id,
            document_generation,
            safe_point,
        })
    }

    fn debugger_execution_control_available(&self) -> bool {
        true
    }

    fn debugger_nested_frames_available(&self) -> bool {
        true
    }

    fn debugger_stack_snapshot_available(&self) -> bool {
        true
    }

    fn debugger_stack_snapshot(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        frame: Option<PageHostDebuggerFrame>,
        max_frames: u32,
        max_scope_entries: u32,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::GetDebuggerStackSnapshot {
            tab_id,
            document_generation,
            program,
            frame,
            max_frames,
            max_scope_entries,
        })
    }

    fn arm_debugger_nested_safe_point_breakpoint(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::ArmDebuggerNestedSafePointBreakpoint {
            tab_id,
            document_generation,
            safe_point,
        })
    }

    fn step_debugger_nested_instruction(
        &mut self,
        frame: PageHostDebuggerFrame,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::StepDebuggerNestedInstruction { frame })
    }

    fn resume_debugger_nested_execution(
        &mut self,
        frame: PageHostDebuggerFrame,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::ResumeDebuggerNestedExecution { frame })
    }

    fn arm_debugger_root_safe_point_breakpoint(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
            tab_id,
            document_generation,
            safe_point,
        })
    }

    fn debugger_execution_state(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::GetDebuggerExecutionState {
            tab_id,
            document_generation,
            program,
        })
    }

    fn resume_debugger_execution(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::ResumeDebuggerExecution {
            tab_id,
            document_generation,
            program,
        })
    }

    fn debugger_stepping_available(&self) -> bool {
        true
    }

    fn debugger_bluets_source_span_step_available(&self) -> bool {
        true
    }

    fn step_debugger_root_instruction(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::StepDebuggerRootInstruction {
            tab_id,
            document_generation,
            program,
        })
    }

    fn step_debugger_bluets_source_span(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        metadata: PageHostDebuggerMetadataHandle,
        source_id: u32,
        safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::StepDebuggerBlueTsSourceSpan {
            tab_id,
            document_generation,
            metadata,
            source_id,
            safe_point,
        })
    }

    fn advance_debugger_execution(
        &mut self,
        tab_id: u64,
        document_generation: u64,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id,
            document_generation,
        })
    }

    fn advance_debugger_execution_with_script_pump(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        pump: &mut dyn FnMut() -> io::Result<()>,
    ) -> io::Result<PageHostReply> {
        self.request_while_pumping_script(
            PageHostRequest::AdvanceDebuggerExecution {
                tab_id,
                document_generation,
            },
            pump,
        )
    }
}

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

impl<C: PageHostClient> PageJavaScriptDebuggerLocations for OutOfProcessJavaScriptPageExecutor<C> {
    fn debugger_has_live_realm(&mut self, tab_id: TabId, document_generation: u64) -> bool {
        if !self.has_core_live_document(tab_id, document_generation) {
            return false;
        }
        let child_has_live_realm = matches!(
            self.child
                .debugger_realm_stats(tab_id.as_u64(), document_generation),
            Ok(PageHostReply::RealmStats(stats))
                if stats.tab_id == tab_id.as_u64()
                    && stats.document_generation == document_generation
                    && stats.is_well_formed()
        );
        if !child_has_live_realm {
            // A later transport/error response is not proof that the cached
            // accounting still names a child-owned realm. Drop it immediately;
            // the ordinary lifecycle owner will close the realm if it gets a
            // later synchronization or execution-control failure.
            self.realm_stats.remove(&tab_id);
        }
        child_has_live_realm
    }

    fn max_debugger_safe_points_per_program(&self) -> usize {
        usize::try_from(PAGE_HOST_DEBUGGER_MAX_SAFE_POINTS_PER_PROGRAM)
            .expect("page-host debugger safe-point cap fits usize")
    }

    fn debugger_breakpoint_configuration_available(&self) -> bool {
        self.child.debugger_breakpoint_configuration_available()
    }

    fn max_debugger_breakpoints_per_realm(&self) -> usize {
        usize::try_from(PAGE_HOST_DEBUGGER_MAX_BREAKPOINTS_PER_REALM)
            .expect("page-host debugger breakpoint cap fits usize")
    }

    fn debugger_static_metadata_inventory_available(&self) -> bool {
        self.child.debugger_bluets_metadata_available()
    }

    fn debugger_static_metadata_summary_available(&self) -> bool {
        self.child.debugger_bluets_metadata_available()
            && self.child.debugger_bluets_metadata_summary_available()
    }

    fn debugger_static_metadata_lowering_summary_available(&self) -> bool {
        self.child.debugger_bluets_metadata_available()
            && self
                .child
                .debugger_bluets_metadata_lowering_summary_available()
    }

    fn debugger_static_metadata_source_inventory_available(&self) -> bool {
        self.child.debugger_bluets_metadata_available()
            && self.child.debugger_bluets_metadata_sources_available()
    }

    fn debugger_static_metadata_source_provenance_available(&self) -> bool {
        self.child.debugger_bluets_metadata_available()
            && self.child.debugger_bluets_metadata_sources_available()
            && self
                .child
                .debugger_bluets_metadata_source_provenance_available()
    }

    fn debugger_static_metadata_type_inventory_available(&self) -> bool {
        self.child.debugger_bluets_metadata_available()
            && self.child.debugger_bluets_metadata_types_available()
    }

    fn debugger_static_metadata_type_display_available(&self) -> bool {
        self.child.debugger_bluets_metadata_available()
            && self.child.debugger_bluets_metadata_types_available()
            && self.child.debugger_bluets_metadata_type_display_available()
    }

    fn debugger_static_metadata_symbol_inventory_available(&self) -> bool {
        self.child.debugger_bluets_metadata_available()
            && self.child.debugger_bluets_metadata_symbols_available()
    }

    fn debugger_static_metadata_symbol_location_available(&self) -> bool {
        self.child.debugger_bluets_metadata_available()
            && self.child.debugger_bluets_metadata_sources_available()
            && self.child.debugger_bluets_metadata_symbols_available()
            && self
                .child
                .debugger_bluets_metadata_symbol_location_available()
    }

    fn debugger_static_metadata_safe_point_span_available(&self) -> bool {
        self.child.debugger_bluets_metadata_available()
            && self.child.debugger_bluets_metadata_sources_available()
            && self.child.debugger_bluets_safe_point_span_available()
    }

    fn debugger_static_metadata_source_breakpoint_available(&self) -> bool {
        self.child.debugger_bluets_metadata_available()
            && self.child.debugger_bluets_metadata_sources_available()
            && self.child.debugger_bluets_source_breakpoint_available()
    }

    fn debugger_static_metadata_contract_location_available(&self) -> bool {
        self.child.debugger_bluets_metadata_available()
            && self.child.debugger_bluets_metadata_sources_available()
            && self.child.debugger_bluets_metadata_contracts_available()
            && self
                .child
                .debugger_bluets_metadata_contract_location_available()
    }

    fn debugger_static_metadata_symbol_type_available(&self) -> bool {
        self.child.debugger_bluets_metadata_available()
            && self.child.debugger_bluets_metadata_symbols_available()
            && self.child.debugger_bluets_metadata_types_available()
            && self.child.debugger_bluets_metadata_symbol_type_available()
    }

    fn debugger_static_metadata_symbol_contract_available(&self) -> bool {
        self.child.debugger_bluets_metadata_available()
            && self.child.debugger_bluets_metadata_symbols_available()
            && self.child.debugger_bluets_metadata_contracts_available()
            && self
                .child
                .debugger_bluets_metadata_symbol_contract_available()
    }

    fn debugger_static_metadata_contract_inventory_available(&self) -> bool {
        self.child.debugger_bluets_metadata_available()
            && self.child.debugger_bluets_metadata_contracts_available()
    }

    fn debugger_static_metadata_contract_display_available(&self) -> bool {
        self.child.debugger_bluets_metadata_available()
            && self.child.debugger_bluets_metadata_contracts_available()
            && self
                .child
                .debugger_bluets_metadata_contract_display_available()
    }

    fn debugger_static_metadata_contract_validation_available(&self) -> bool {
        self.child.debugger_bluets_metadata_available()
            && self.child.debugger_bluets_metadata_contracts_available()
            && self
                .child
                .debugger_bluets_metadata_contract_validation_available()
    }

    fn debugger_static_metadata_symbol_display_available(&self) -> bool {
        self.child.debugger_bluets_metadata_available()
            && self.child.debugger_bluets_metadata_symbols_available()
            && self
                .child
                .debugger_bluets_metadata_symbol_display_available()
    }

    fn debugger_programs(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
    ) -> Result<Vec<JavaScriptPageDebuggerProgram>, JavaScriptPageDebuggerError> {
        if !self.has_core_live_document(tab_id, document_generation) {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let reply = self
            .child
            .debugger_programs(tab_id.as_u64(), document_generation)
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerPrograms {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            programs,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || programs.iter().any(|program| !program.is_well_formed())
            || has_duplicate_child_programs(&programs)
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }

        let previous = self.debugger_programs.remove(&tab_id).unwrap_or_default();
        // Metadata identities are only valid together with the exact current
        // child-to-core program mapping. A fresh program discovery invalidates
        // all prior inventory IDs before a subsequent source-free lookup can
        // remint them, even when the child happened to retain a numeric ID.
        self.debugger_static_metadata.remove(&tab_id);
        let mut current = BTreeMap::new();
        for child_program in programs {
            let public = match previous.get(&child_program).copied() {
                Some(public) => public,
                None => self.mint_core_debugger_program()?,
            };
            current.insert(child_program, public);
        }
        let programs: Vec<_> = current
            .values()
            .map(|public| JavaScriptPageDebuggerProgram {
                program_handle: public.program_handle,
                program_generation: public.program_generation,
            })
            .collect();
        if self
            .debugger_nested_frames
            .get(&tab_id)
            .is_some_and(|active| {
                !current.iter().any(|(child, public)| {
                    *child == active.child.program
                        && public.program_handle == active.public.program_handle
                        && public.program_generation == active.public.program_generation
                })
            })
        {
            self.debugger_nested_frames.remove(&tab_id);
        }
        self.debugger_programs.insert(tab_id, current);
        Ok(programs)
    }

    fn debugger_static_metadata(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
    ) -> Result<Vec<JavaScriptPageDebuggerStaticMetadata>, JavaScriptPageDebuggerError> {
        if !self.debugger_static_metadata_inventory_available() {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let child_program = self.child_program_for_core(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        let reply = self
            .child
            .debugger_bluets_metadata(tab_id.as_u64(), document_generation, child_program)
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerBlueTsMetadata {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program,
            metadata,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || program != child_program
            || metadata.len() > MAX_CHILD_DEBUGGER_STATIC_METADATA_PER_PROGRAM
            || metadata.iter().any(|metadata| !metadata.is_well_formed())
            || has_duplicate_child_static_metadata(&metadata)
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }

        let previous = self
            .debugger_static_metadata
            .remove(&tab_id)
            .unwrap_or_default();
        let mut current = BTreeMap::new();
        let mut public_metadata = Vec::with_capacity(metadata.len());
        for child_metadata in metadata {
            let public = match previous.get(&child_metadata).copied() {
                Some(public) if public.program == child_program => public,
                _ => self.mint_core_debugger_static_metadata(child_program)?,
            };
            current.insert(child_metadata, public);
            public_metadata.push(JavaScriptPageDebuggerStaticMetadata {
                metadata_handle: public.metadata_handle,
                metadata_generation: public.metadata_generation,
            });
        }
        self.debugger_static_metadata.insert(tab_id, current);
        Ok(public_metadata)
    }

    fn debugger_static_metadata_summary(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
        metadata_handle: u64,
        metadata_generation: u64,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataSummary, JavaScriptPageDebuggerError> {
        if !self.debugger_static_metadata_summary_available() {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let child_program = self.child_program_for_core(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        let child_metadata = self.child_static_metadata_for_core(
            tab_id,
            document_generation,
            child_program,
            metadata_handle,
            metadata_generation,
        )?;
        let reply = self
            .child
            .debugger_bluets_metadata_summary(
                tab_id.as_u64(),
                document_generation,
                child_program,
                child_metadata,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerBlueTsMetadataSummary {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program,
            metadata,
            summary,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || program != child_program
            || metadata != child_metadata
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        Ok(JavaScriptPageDebuggerStaticMetadataSummary {
            language_version: summary.language_version,
            compiler_options_hash: summary.compiler_options_hash,
            source_count: summary.source_count,
            type_count: summary.type_count,
            symbol_count: summary.symbol_count,
            contract_count: summary.contract_count,
        })
    }

    fn debugger_static_metadata_lowering_summary(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
        metadata_handle: u64,
        metadata_generation: u64,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataLoweringSummary, JavaScriptPageDebuggerError>
    {
        if !self.debugger_static_metadata_lowering_summary_available() {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let child_program = self.child_program_for_core(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        let child_metadata = self.child_static_metadata_for_core(
            tab_id,
            document_generation,
            child_program,
            metadata_handle,
            metadata_generation,
        )?;
        let reply = self
            .child
            .debugger_bluets_metadata_lowering_summary(
                tab_id.as_u64(),
                document_generation,
                child_program,
                child_metadata,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerBlueTsMetadataLoweringSummary {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program,
            metadata,
            summary,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || program != child_program
            || metadata != child_metadata
            || summary.safe_point_map_abi.is_empty()
            || summary.program_abi.is_empty()
            || summary.source_set_hash.is_empty()
            || summary.safe_point_map_abi.len()
                > blueice_ipc::debugger::DEBUGGER_STATIC_METADATA_LOWERING_ABI_MAX_BYTES
            || summary.program_abi.len()
                > blueice_ipc::debugger::DEBUGGER_STATIC_METADATA_LOWERING_ABI_MAX_BYTES
            || summary.source_set_hash.len()
                > blueice_ipc::debugger::DEBUGGER_STATIC_METADATA_LOWERING_SOURCE_SET_HASH_MAX_BYTES
            || summary.bound_safe_point_count
                > blueice_ipc::debugger::DEBUGGER_STATIC_METADATA_MAX_BOUND_SAFE_POINTS
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        Ok(JavaScriptPageDebuggerStaticMetadataLoweringSummary {
            safe_point_map_abi: summary.safe_point_map_abi,
            program_abi: summary.program_abi,
            source_set_hash: summary.source_set_hash,
            bound_safe_point_count: summary.bound_safe_point_count,
        })
    }

    fn debugger_static_metadata_sources(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
        metadata_handle: u64,
        metadata_generation: u64,
    ) -> Result<Vec<JavaScriptPageDebuggerStaticMetadataSourceId>, JavaScriptPageDebuggerError>
    {
        if !self.debugger_static_metadata_source_inventory_available() {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let child_program = self.child_program_for_core(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        let child_metadata = self.child_static_metadata_for_core(
            tab_id,
            document_generation,
            child_program,
            metadata_handle,
            metadata_generation,
        )?;
        let reply = self
            .child
            .debugger_bluets_metadata_sources(
                tab_id.as_u64(),
                document_generation,
                child_program,
                child_metadata,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerBlueTsMetadataSources {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program,
            metadata,
            sources,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || program != child_program
            || metadata != child_metadata
            || sources.len() > usize::try_from(DEBUGGER_STATIC_METADATA_MAX_SOURCES).unwrap()
            || has_duplicate_child_static_metadata_source_ids(&sources)
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        Ok(sources
            .into_iter()
            .map(|source| JavaScriptPageDebuggerStaticMetadataSourceId {
                source_id: source.source_id,
            })
            .collect())
    }

    fn debugger_static_metadata_source_provenance(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        target: JavaScriptPageDebuggerStaticMetadataSourceTarget,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataSourceProvenance, JavaScriptPageDebuggerError>
    {
        if !self.debugger_static_metadata_source_provenance_available() {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let child_program = self.child_program_for_core(
            tab_id,
            document_generation,
            target.program_handle,
            target.program_generation,
        )?;
        let child_metadata = self.child_static_metadata_for_core(
            tab_id,
            document_generation,
            child_program,
            target.metadata_handle,
            target.metadata_generation,
        )?;
        let reply = self
            .child
            .debugger_bluets_metadata_source_provenance(
                tab_id.as_u64(),
                document_generation,
                child_program,
                child_metadata,
                target.source_id,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerBlueTsMetadataSourceProvenance {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program,
            metadata,
            provenance,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || program != child_program
            || metadata != child_metadata
            || provenance.source_id != target.source_id
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        Ok(JavaScriptPageDebuggerStaticMetadataSourceProvenance {
            source_id: provenance.source_id,
            module: provenance.module,
            content_hash: provenance.content_hash,
        })
    }

    fn debugger_static_metadata_types(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
        metadata_handle: u64,
        metadata_generation: u64,
    ) -> Result<Vec<JavaScriptPageDebuggerStaticMetadataTypeId>, JavaScriptPageDebuggerError> {
        if !self.debugger_static_metadata_type_inventory_available() {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let child_program = self.child_program_for_core(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        let child_metadata = self.child_static_metadata_for_core(
            tab_id,
            document_generation,
            child_program,
            metadata_handle,
            metadata_generation,
        )?;
        let reply = self
            .child
            .debugger_bluets_metadata_types(
                tab_id.as_u64(),
                document_generation,
                child_program,
                child_metadata,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerBlueTsMetadataTypes {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program,
            metadata,
            types,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || program != child_program
            || metadata != child_metadata
            || types.len() > usize::try_from(DEBUGGER_STATIC_METADATA_MAX_TYPES).unwrap()
            || has_duplicate_child_static_metadata_type_ids(&types)
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        Ok(types
            .into_iter()
            .map(|static_type| JavaScriptPageDebuggerStaticMetadataTypeId {
                type_id: static_type.type_id,
            })
            .collect())
    }

    fn debugger_static_metadata_type_display(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        target: JavaScriptPageDebuggerStaticMetadataTypeTarget,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataTypeDisplay, JavaScriptPageDebuggerError> {
        if !self.debugger_static_metadata_type_display_available() {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let child_program = self.child_program_for_core(
            tab_id,
            document_generation,
            target.program_handle,
            target.program_generation,
        )?;
        let child_metadata = self.child_static_metadata_for_core(
            tab_id,
            document_generation,
            child_program,
            target.metadata_handle,
            target.metadata_generation,
        )?;
        let reply = self
            .child
            .debugger_bluets_metadata_type_display(
                tab_id.as_u64(),
                document_generation,
                child_program,
                child_metadata,
                target.type_id,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerBlueTsMetadataType {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program,
            metadata,
            static_type,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || program != child_program
            || metadata != child_metadata
            || static_type.type_id != target.type_id
            || static_type.display.is_empty()
            || static_type.display.len() > DEBUGGER_STATIC_METADATA_TYPE_DISPLAY_MAX_BYTES
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        Ok(JavaScriptPageDebuggerStaticMetadataTypeDisplay {
            type_id: static_type.type_id,
            display: static_type.display,
        })
    }

    fn debugger_static_metadata_symbols(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
        metadata_handle: u64,
        metadata_generation: u64,
    ) -> Result<Vec<JavaScriptPageDebuggerStaticMetadataSymbolId>, JavaScriptPageDebuggerError>
    {
        if !self.debugger_static_metadata_symbol_inventory_available() {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let child_program = self.child_program_for_core(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        let child_metadata = self.child_static_metadata_for_core(
            tab_id,
            document_generation,
            child_program,
            metadata_handle,
            metadata_generation,
        )?;
        let reply = self
            .child
            .debugger_bluets_metadata_symbols(
                tab_id.as_u64(),
                document_generation,
                child_program,
                child_metadata,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerBlueTsMetadataSymbols {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program,
            metadata,
            symbols,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || program != child_program
            || metadata != child_metadata
            || symbols.len() > usize::try_from(DEBUGGER_STATIC_METADATA_MAX_SYMBOLS).unwrap()
            || has_duplicate_child_static_metadata_symbol_ids(&symbols)
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        Ok(symbols
            .into_iter()
            .map(|symbol| JavaScriptPageDebuggerStaticMetadataSymbolId {
                symbol_id: symbol.symbol_id,
            })
            .collect())
    }

    fn debugger_static_metadata_contracts(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
        metadata_handle: u64,
        metadata_generation: u64,
    ) -> Result<Vec<JavaScriptPageDebuggerStaticMetadataContractId>, JavaScriptPageDebuggerError>
    {
        if !self.debugger_static_metadata_contract_inventory_available() {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let child_program = self.child_program_for_core(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        let child_metadata = self.child_static_metadata_for_core(
            tab_id,
            document_generation,
            child_program,
            metadata_handle,
            metadata_generation,
        )?;
        let reply = self
            .child
            .debugger_bluets_metadata_contracts(
                tab_id.as_u64(),
                document_generation,
                child_program,
                child_metadata,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerBlueTsMetadataContracts {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program,
            metadata,
            contracts,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || program != child_program
            || metadata != child_metadata
            || contracts.len() > usize::try_from(DEBUGGER_STATIC_METADATA_MAX_CONTRACTS).unwrap()
            || has_duplicate_child_static_metadata_contract_ids(&contracts)
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        Ok(contracts
            .into_iter()
            .map(|contract| JavaScriptPageDebuggerStaticMetadataContractId {
                contract_id: contract.contract_id,
            })
            .collect())
    }

    fn debugger_static_metadata_contract_display(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        target: JavaScriptPageDebuggerStaticMetadataContractTarget,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataContractDisplay, JavaScriptPageDebuggerError>
    {
        if !self.debugger_static_metadata_contract_display_available() {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let child_program = self.child_program_for_core(
            tab_id,
            document_generation,
            target.program_handle,
            target.program_generation,
        )?;
        let child_metadata = self.child_static_metadata_for_core(
            tab_id,
            document_generation,
            child_program,
            target.metadata_handle,
            target.metadata_generation,
        )?;
        let reply = self
            .child
            .debugger_bluets_metadata_contract_display(
                tab_id.as_u64(),
                document_generation,
                child_program,
                child_metadata,
                target.contract_id,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerBlueTsMetadataContract {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program,
            metadata,
            contract,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || program != child_program
            || metadata != child_metadata
            || contract.contract_id != target.contract_id
            || contract.display.is_empty()
            || contract.display.len() > DEBUGGER_STATIC_METADATA_CONTRACT_DISPLAY_MAX_BYTES
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        Ok(JavaScriptPageDebuggerStaticMetadataContractDisplay {
            contract_id: contract.contract_id,
            display: contract.display,
            root_kind: contract.root_kind,
        })
    }

    fn debugger_static_metadata_contract_validation(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        target: JavaScriptPageDebuggerStaticMetadataContractTarget,
        value: CompilerContractValue,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataContractValidation, JavaScriptPageDebuggerError>
    {
        if !self.debugger_static_metadata_contract_validation_available() {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let child_program = self.child_program_for_core(
            tab_id,
            document_generation,
            target.program_handle,
            target.program_generation,
        )?;
        let child_metadata = self.child_static_metadata_for_core(
            tab_id,
            document_generation,
            child_program,
            target.metadata_handle,
            target.metadata_generation,
        )?;
        let reply = self
            .child
            .debugger_bluets_metadata_contract_validation(
                tab_id.as_u64(),
                document_generation,
                child_program,
                child_metadata,
                target.contract_id,
                value,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerBlueTsMetadataContractValidation {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program,
            metadata,
            validation,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || program != child_program
            || metadata != child_metadata
            || validation.contract_id != target.contract_id
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        Ok(JavaScriptPageDebuggerStaticMetadataContractValidation {
            contract_id: validation.contract_id,
            valid: validation.valid,
        })
    }

    fn debugger_static_metadata_symbol_display(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        target: JavaScriptPageDebuggerStaticMetadataSymbolTarget,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataSymbolDisplay, JavaScriptPageDebuggerError>
    {
        if !self.debugger_static_metadata_symbol_display_available() {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let child_program = self.child_program_for_core(
            tab_id,
            document_generation,
            target.program_handle,
            target.program_generation,
        )?;
        let child_metadata = self.child_static_metadata_for_core(
            tab_id,
            document_generation,
            child_program,
            target.metadata_handle,
            target.metadata_generation,
        )?;
        let reply = self
            .child
            .debugger_bluets_metadata_symbol_display(
                tab_id.as_u64(),
                document_generation,
                child_program,
                child_metadata,
                target.symbol_id,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerBlueTsMetadataSymbol {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program,
            metadata,
            symbol,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || program != child_program
            || metadata != child_metadata
            || symbol.symbol_id != target.symbol_id
            || symbol.display.is_empty()
            || symbol.display.len() > DEBUGGER_STATIC_METADATA_SYMBOL_DISPLAY_MAX_BYTES
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        Ok(JavaScriptPageDebuggerStaticMetadataSymbolDisplay {
            symbol_id: symbol.symbol_id,
            display: symbol.display,
            kind: symbol.kind,
            exported: symbol.exported,
        })
    }

    fn debugger_static_metadata_symbol_location(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        target: JavaScriptPageDebuggerStaticMetadataSymbolLocationTarget,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataSymbolLocation, JavaScriptPageDebuggerError>
    {
        if !self.debugger_static_metadata_symbol_location_available() {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let child_program = self.child_program_for_core(
            tab_id,
            document_generation,
            target.program_handle,
            target.program_generation,
        )?;
        let child_metadata = self.child_static_metadata_for_core(
            tab_id,
            document_generation,
            child_program,
            target.metadata_handle,
            target.metadata_generation,
        )?;
        let reply = self
            .child
            .debugger_bluets_metadata_symbol_location(
                tab_id.as_u64(),
                document_generation,
                child_program,
                child_metadata,
                target.symbol_id,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerBlueTsMetadataSymbolLocation {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program,
            metadata,
            location,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || program != child_program
            || metadata != child_metadata
            || !valid_child_static_metadata_symbol_location(
                location,
                target.symbol_id,
                target.source_id,
            )
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        Ok(JavaScriptPageDebuggerStaticMetadataSymbolLocation {
            symbol_id: location.symbol_id,
            source_id: location.source_id,
            start_byte: location.start_byte,
            end_byte: location.end_byte,
            coordinates: location.coordinates,
        })
    }

    fn debugger_static_metadata_safe_point_span(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        target: JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataSafePointSpan, JavaScriptPageDebuggerError>
    {
        if !self.debugger_static_metadata_safe_point_span_available() {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let child_program = self.child_program_for_core(
            tab_id,
            document_generation,
            target.program_handle,
            target.program_generation,
        )?;
        let child_metadata = self.child_static_metadata_for_core(
            tab_id,
            document_generation,
            child_program,
            target.metadata_handle,
            target.metadata_generation,
        )?;
        let safe_point = PageHostDebuggerSafePoint {
            program: child_program,
            code_unit_ordinal: target.code_unit_ordinal,
            bytecode_offset: target.bytecode_offset,
        };
        let reply = self
            .child
            .debugger_bluets_safe_point_span(
                tab_id.as_u64(),
                document_generation,
                child_metadata,
                safe_point,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerBlueTsSafePointSpan {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            metadata,
            safe_point: echoed_safe_point,
            span,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || metadata != child_metadata
            || echoed_safe_point != safe_point
            || !span
                .coordinates
                .is_well_formed_for_range(span.start_byte, span.end_byte)
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        if span.source_id != target.source_id {
            return Err(JavaScriptPageDebuggerError::UnknownProgram);
        }
        Ok(JavaScriptPageDebuggerStaticMetadataSafePointSpan {
            source_id: span.source_id,
            start_byte: span.start_byte,
            end_byte: span.end_byte,
            coordinates: span.coordinates,
        })
    }

    fn debugger_static_metadata_source_breakpoint(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        target: JavaScriptPageDebuggerStaticMetadataSourceBreakpointTarget,
    ) -> Result<Option<JavaScriptPageDebuggerSafePoint>, JavaScriptPageDebuggerError> {
        if !self.debugger_static_metadata_source_breakpoint_available()
            || target.source_byte > DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let child_program = self.child_program_for_core(
            tab_id,
            document_generation,
            target.program_handle,
            target.program_generation,
        )?;
        let child_metadata = self.child_static_metadata_for_core(
            tab_id,
            document_generation,
            child_program,
            target.metadata_handle,
            target.metadata_generation,
        )?;
        let reply = self
            .child
            .debugger_bluets_source_breakpoint(
                tab_id.as_u64(),
                document_generation,
                child_program,
                child_metadata,
                target.source_id,
                target.source_byte,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerBlueTsSourceBreakpoint {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program,
            metadata,
            source_id,
            source_byte,
            safe_point,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || program != child_program
            || metadata != child_metadata
            || source_id != target.source_id
            || source_byte != target.source_byte
            || safe_point.is_some_and(|safe_point| {
                !safe_point.is_well_formed() || safe_point.program != child_program
            })
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        Ok(
            safe_point.map(|safe_point| JavaScriptPageDebuggerSafePoint {
                code_unit_ordinal: safe_point.code_unit_ordinal,
                bytecode_offset: safe_point.bytecode_offset,
            }),
        )
    }

    fn debugger_static_metadata_contract_location(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        target: JavaScriptPageDebuggerStaticMetadataContractLocationTarget,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataContractLocation, JavaScriptPageDebuggerError>
    {
        if !self.debugger_static_metadata_contract_location_available() {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let child_program = self.child_program_for_core(
            tab_id,
            document_generation,
            target.program_handle,
            target.program_generation,
        )?;
        let child_metadata = self.child_static_metadata_for_core(
            tab_id,
            document_generation,
            child_program,
            target.metadata_handle,
            target.metadata_generation,
        )?;
        let reply = self
            .child
            .debugger_bluets_metadata_contract_location(
                tab_id.as_u64(),
                document_generation,
                child_program,
                child_metadata,
                target.contract_id,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerBlueTsMetadataContractLocation {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program,
            metadata,
            location,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || program != child_program
            || metadata != child_metadata
            || location.start_byte >= location.end_byte
            || location.end_byte > DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES
            || !location
                .coordinates
                .is_well_formed_for_range(location.start_byte, location.end_byte)
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        if location.contract_id != target.contract_id || location.source_id != target.source_id {
            return Err(JavaScriptPageDebuggerError::UnknownProgram);
        }
        Ok(JavaScriptPageDebuggerStaticMetadataContractLocation {
            contract_id: location.contract_id,
            source_id: location.source_id,
            start_byte: location.start_byte,
            end_byte: location.end_byte,
            coordinates: location.coordinates,
        })
    }

    fn debugger_static_metadata_symbol_type(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        target: JavaScriptPageDebuggerStaticMetadataSymbolTypeTarget,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataSymbolType, JavaScriptPageDebuggerError> {
        if !self.debugger_static_metadata_symbol_type_available() {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let child_program = self.child_program_for_core(
            tab_id,
            document_generation,
            target.program_handle,
            target.program_generation,
        )?;
        let child_metadata = self.child_static_metadata_for_core(
            tab_id,
            document_generation,
            child_program,
            target.metadata_handle,
            target.metadata_generation,
        )?;
        let reply = self
            .child
            .debugger_bluets_metadata_symbol_type(
                tab_id.as_u64(),
                document_generation,
                child_program,
                child_metadata,
                target.symbol_id,
                target.type_id,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerBlueTsMetadataSymbolType {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program,
            metadata,
            symbol_type,
        } = reply
        else {
            return Err(child_static_metadata_relation_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || program != child_program
            || metadata != child_metadata
            || !valid_child_static_metadata_symbol_type(
                symbol_type,
                target.symbol_id,
                target.type_id,
            )
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        Ok(JavaScriptPageDebuggerStaticMetadataSymbolType {
            symbol_id: symbol_type.symbol_id,
            type_id: symbol_type.type_id,
        })
    }

    fn debugger_static_metadata_symbol_contract(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        target: JavaScriptPageDebuggerStaticMetadataSymbolContractTarget,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataSymbolContract, JavaScriptPageDebuggerError>
    {
        if !self.debugger_static_metadata_symbol_contract_available() {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let child_program = self.child_program_for_core(
            tab_id,
            document_generation,
            target.program_handle,
            target.program_generation,
        )?;
        let child_metadata = self.child_static_metadata_for_core(
            tab_id,
            document_generation,
            child_program,
            target.metadata_handle,
            target.metadata_generation,
        )?;
        let reply = self
            .child
            .debugger_bluets_metadata_symbol_contract(
                tab_id.as_u64(),
                document_generation,
                child_program,
                child_metadata,
                target.symbol_id,
                target.contract_id,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerBlueTsMetadataSymbolContract {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program,
            metadata,
            symbol_contract,
        } = reply
        else {
            return Err(child_static_metadata_relation_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || program != child_program
            || metadata != child_metadata
            || !valid_child_static_metadata_symbol_contract(
                symbol_contract,
                target.symbol_id,
                target.contract_id,
            )
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        Ok(JavaScriptPageDebuggerStaticMetadataSymbolContract {
            symbol_id: symbol_contract.symbol_id,
            contract_id: symbol_contract.contract_id,
        })
    }

    fn debugger_safe_points(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
    ) -> Result<Vec<JavaScriptPageDebuggerSafePoint>, JavaScriptPageDebuggerError> {
        let child_program = self.child_program_for_core(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        let reply = self
            .child
            .debugger_safe_points(tab_id.as_u64(), document_generation, child_program)
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerSafePoints {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program,
            safe_points,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || program != child_program
            || safe_points.len() > self.max_debugger_safe_points_per_program()
            || safe_points.iter().any(|safe_point| {
                !safe_point.is_well_formed() || safe_point.program != child_program
            })
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        Ok(safe_points
            .into_iter()
            .map(|safe_point| JavaScriptPageDebuggerSafePoint {
                code_unit_ordinal: safe_point.code_unit_ordinal,
                bytecode_offset: safe_point.bytecode_offset,
            })
            .collect())
    }

    fn validate_debugger_safe_point(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
        code_unit_ordinal: u32,
        bytecode_offset: u32,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        let program = self.child_program_for_core(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        let safe_point = PageHostDebuggerSafePoint {
            program,
            code_unit_ordinal,
            bytecode_offset,
        };
        let reply = self
            .child
            .validate_debugger_safe_point(tab_id.as_u64(), document_generation, safe_point)
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        match reply {
            PageHostReply::DebuggerSafePointValidated {
                tab_id: reply_tab_id,
                document_generation: reply_generation,
                safe_point: reply_safe_point,
            } if reply_tab_id == tab_id.as_u64()
                && reply_generation == document_generation
                && reply_safe_point == safe_point =>
            {
                Ok(())
            }
            PageHostReply::Error { .. } => Err(child_debugger_reply_error(&reply)),
            _ => Err(JavaScriptPageDebuggerError::NoLiveRealm),
        }
    }

    fn set_debugger_breakpoint(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
        code_unit_ordinal: u32,
        bytecode_offset: u32,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        let safe_point = PageHostDebuggerSafePoint {
            program: self.child_program_for_core(
                tab_id,
                document_generation,
                program_handle,
                program_generation,
            )?,
            code_unit_ordinal,
            bytecode_offset,
        };
        // The public target was mapped to a child-private program identity,
        // but a program match alone is insufficient: make the child prove the
        // exact instruction boundary before it can mutate its bounded table.
        validate_child_safe_point_reply(self, tab_id, document_generation, safe_point)?;
        let reply = self
            .child
            .set_debugger_breakpoint(tab_id.as_u64(), document_generation, safe_point)
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        match reply {
            PageHostReply::DebuggerBreakpointSet {
                tab_id: reply_tab_id,
                document_generation: reply_generation,
                safe_point: reply_safe_point,
            } if reply_tab_id == tab_id.as_u64()
                && reply_generation == document_generation
                && reply_safe_point == safe_point =>
            {
                Ok(())
            }
            PageHostReply::Error { .. } => Err(child_debugger_reply_error(&reply)),
            _ => Err(JavaScriptPageDebuggerError::NoLiveRealm),
        }
    }

    fn debugger_breakpoints(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
    ) -> Result<Vec<JavaScriptPageDebuggerBreakpoint>, JavaScriptPageDebuggerError> {
        if !self.has_core_live_document(tab_id, document_generation) {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let reply = self
            .child
            .debugger_breakpoints(tab_id.as_u64(), document_generation)
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerBreakpoints {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            safe_points,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || safe_points.len() > self.max_debugger_breakpoints_per_realm()
            || has_duplicate_child_safe_points(&safe_points)
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }

        let mut breakpoints = Vec::with_capacity(safe_points.len());
        for safe_point in safe_points {
            let public =
                self.core_program_for_child(tab_id, document_generation, safe_point.program)?;
            // The stored private table must not become a way for a compromised
            // child to fabricate arbitrary offsets under an otherwise known
            // private program ID. Require a fresh exact validation echo for
            // every listed tuple before re-minting the public response.
            validate_child_safe_point_reply(self, tab_id, document_generation, safe_point)?;
            breakpoints.push(JavaScriptPageDebuggerBreakpoint {
                program_handle: public.program_handle,
                program_generation: public.program_generation,
                code_unit_ordinal: safe_point.code_unit_ordinal,
                bytecode_offset: safe_point.bytecode_offset,
            });
        }
        Ok(breakpoints)
    }

    fn clear_debugger_breakpoint(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
        code_unit_ordinal: u32,
        bytecode_offset: u32,
    ) -> Result<bool, JavaScriptPageDebuggerError> {
        let safe_point = PageHostDebuggerSafePoint {
            program: self.child_program_for_core(
                tab_id,
                document_generation,
                program_handle,
                program_generation,
            )?,
            code_unit_ordinal,
            bytecode_offset,
        };
        // Clearing an absent valid record is idempotent, but clearing a
        // malformed/stale location is never a no-op that can target a
        // successor. Revalidate first just as the in-process route does.
        validate_child_safe_point_reply(self, tab_id, document_generation, safe_point)?;
        let reply = self
            .child
            .clear_debugger_breakpoint(tab_id.as_u64(), document_generation, safe_point)
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        match reply {
            PageHostReply::DebuggerBreakpointCleared {
                tab_id: reply_tab_id,
                document_generation: reply_generation,
                safe_point: reply_safe_point,
                was_present,
            } if reply_tab_id == tab_id.as_u64()
                && reply_generation == document_generation
                && reply_safe_point == safe_point =>
            {
                Ok(was_present)
            }
            PageHostReply::Error { .. } => Err(child_debugger_reply_error(&reply)),
            _ => Err(JavaScriptPageDebuggerError::NoLiveRealm),
        }
    }

    fn debugger_execution_control_available(&self) -> bool {
        self.native_debugger_execution_control && self.child.debugger_execution_control_available()
    }

    fn debugger_nested_frames_available(&self) -> bool {
        self.debugger_execution_control_available() && self.child.debugger_nested_frames_available()
    }

    fn arm_debugger_nested_safe_point_breakpoint(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
        code_unit_ordinal: u32,
        bytecode_offset: u32,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        if !self.debugger_nested_frames_available() || code_unit_ordinal == 0 {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        let safe_point = PageHostDebuggerSafePoint {
            program: self.child_program_for_core(
                tab_id,
                document_generation,
                program_handle,
                program_generation,
            )?,
            code_unit_ordinal,
            bytecode_offset,
        };
        validate_child_safe_point_reply(self, tab_id, document_generation, safe_point)?;
        let reply = self
            .child
            .arm_debugger_nested_safe_point_breakpoint(
                tab_id.as_u64(),
                document_generation,
                safe_point,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        match reply {
            PageHostReply::DebuggerNestedSafePointBreakpointArmed {
                tab_id: reply_tab_id,
                document_generation: reply_generation,
                safe_point: reply_safe_point,
            } if reply_tab_id == tab_id.as_u64()
                && reply_generation == document_generation
                && reply_safe_point == safe_point =>
            {
                Ok(())
            }
            PageHostReply::Error { .. } => Err(child_debugger_reply_error(&reply)),
            _ => Err(JavaScriptPageDebuggerError::NoLiveRealm),
        }
    }

    fn debugger_nested_execution_state(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
    ) -> Result<Option<JavaScriptPageDebuggerNestedExecutionState>, JavaScriptPageDebuggerError>
    {
        if !self.debugger_nested_frames_available() {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        let program = self.child_program_for_core(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        let reply = self
            .child
            .debugger_execution_state(tab_id.as_u64(), document_generation, program)
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerExecutionState {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program: reply_program,
            state,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || reply_program != program
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        match state {
            PageHostDebuggerExecutionState::NestedPaused { frame, safe_point }
                if frame.tab_id == tab_id.as_u64()
                    && frame.document_generation == document_generation
                    && frame.program == program
                    && frame.matches_safe_point(safe_point) =>
            {
                validate_child_safe_point_reply(self, tab_id, document_generation, safe_point)?;
                Ok(Some(JavaScriptPageDebuggerNestedExecutionState::Paused {
                    frame: self.remint_core_debugger_frame(
                        tab_id,
                        document_generation,
                        program_handle,
                        program_generation,
                        frame,
                    )?,
                    bytecode_offset: safe_point.bytecode_offset,
                }))
            }
            PageHostDebuggerExecutionState::NestedStepping { frame }
            | PageHostDebuggerExecutionState::NestedResuming { frame }
                if frame.is_well_formed()
                    && frame.tab_id == tab_id.as_u64()
                    && frame.document_generation == document_generation
                    && frame.program == program =>
            {
                let active = self
                    .debugger_nested_frames
                    .get(&tab_id)
                    .filter(|active| {
                        active.child == frame
                            && active.public.document_generation == document_generation
                            && active.public.program_handle == program_handle
                            && active.public.program_generation == program_generation
                    })
                    .ok_or(JavaScriptPageDebuggerError::InvalidExecutionState)?;
                Ok(Some(match state {
                    PageHostDebuggerExecutionState::NestedStepping { .. } => {
                        JavaScriptPageDebuggerNestedExecutionState::Stepping {
                            frame: active.public,
                        }
                    }
                    PageHostDebuggerExecutionState::NestedResuming { .. } => {
                        JavaScriptPageDebuggerNestedExecutionState::Resuming {
                            frame: active.public,
                        }
                    }
                    _ => unreachable!("only nested advance states pass this guard"),
                }))
            }
            PageHostDebuggerExecutionState::NestedPaused { .. }
            | PageHostDebuggerExecutionState::NestedStepping { .. }
            | PageHostDebuggerExecutionState::NestedResuming { .. } => {
                Err(JavaScriptPageDebuggerError::NoLiveRealm)
            }
            _ => {
                if self
                    .debugger_nested_frames
                    .get(&tab_id)
                    .is_some_and(|active| {
                        active.public.document_generation == document_generation
                            && active.public.program_handle == program_handle
                            && active.public.program_generation == program_generation
                    })
                {
                    self.debugger_nested_frames.remove(&tab_id);
                }
                Ok(None)
            }
        }
    }

    fn step_debugger_nested_instruction(
        &mut self,
        frame: JavaScriptPageDebuggerFrame,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        if !self.debugger_nested_frames_available()
            || frame.code_unit_ordinal == 0
            || frame.frame_handle == 0
        {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        let active = self
            .debugger_nested_frames
            .get(&frame.tab_id)
            .copied()
            .filter(|active| active.public == frame)
            .ok_or(JavaScriptPageDebuggerError::InvalidExecutionState)?;
        let program = self.child_program_for_core(
            frame.tab_id,
            frame.document_generation,
            frame.program_handle,
            frame.program_generation,
        )?;
        if active.child.program != program {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let child_frame = active.child;
        let reply = self
            .child
            .step_debugger_nested_instruction(child_frame)
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        match reply {
            PageHostReply::DebuggerNestedStepRequested { frame: reply_frame }
                if reply_frame == child_frame =>
            {
                Ok(())
            }
            PageHostReply::Error { .. } => {
                self.debugger_nested_frames.remove(&frame.tab_id);
                Err(child_debugger_reply_error(&reply))
            }
            _ => {
                self.debugger_nested_frames.remove(&frame.tab_id);
                Err(JavaScriptPageDebuggerError::NoLiveRealm)
            }
        }
    }

    fn resume_debugger_nested_execution(
        &mut self,
        frame: JavaScriptPageDebuggerFrame,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        if !self.debugger_nested_frames_available()
            || frame.code_unit_ordinal == 0
            || frame.frame_handle == 0
        {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        let active = self
            .debugger_nested_frames
            .get(&frame.tab_id)
            .copied()
            .filter(|active| active.public == frame)
            .ok_or(JavaScriptPageDebuggerError::InvalidExecutionState)?;
        let program = self.child_program_for_core(
            frame.tab_id,
            frame.document_generation,
            frame.program_handle,
            frame.program_generation,
        )?;
        if active.child.program != program {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let child_frame = active.child;
        let reply = self
            .child
            .resume_debugger_nested_execution(child_frame)
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        match reply {
            PageHostReply::DebuggerNestedResumeRequested { frame: reply_frame }
                if reply_frame == child_frame =>
            {
                Ok(())
            }
            PageHostReply::Error { .. } => {
                self.debugger_nested_frames.remove(&frame.tab_id);
                Err(child_debugger_reply_error(&reply))
            }
            _ => {
                self.debugger_nested_frames.remove(&frame.tab_id);
                Err(JavaScriptPageDebuggerError::NoLiveRealm)
            }
        }
    }

    fn debugger_stack_snapshot(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        core_program: JavaScriptPageDebuggerProgram,
        frame: Option<JavaScriptPageDebuggerFrame>,
        max_frames: u32,
        max_scope_entries: u32,
    ) -> Result<JavaScriptPageDebuggerStackSnapshot, JavaScriptPageDebuggerError> {
        let JavaScriptPageDebuggerProgram {
            program_handle,
            program_generation,
        } = core_program;
        if !self.debugger_execution_control_available()
            || !self.child.debugger_stack_snapshot_available()
        {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        if !(1..=PAGE_HOST_DEBUGGER_MAX_STACK_FRAMES).contains(&max_frames)
            || !(1..=PAGE_HOST_DEBUGGER_MAX_SCOPE_ENTRIES).contains(&max_scope_entries)
        {
            return Err(JavaScriptPageDebuggerError::ResourceLimit);
        }
        let program = self.child_program_for_core(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        let (child_frame, top_code_unit, top_offset) = if let Some(frame) = frame {
            if frame.tab_id != tab_id
                || frame.document_generation != document_generation
                || frame.program_handle != program_handle
                || frame.program_generation != program_generation
                || frame.code_unit_ordinal == 0
                || frame.frame_handle == 0
            {
                return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
            }
            let active = self
                .debugger_nested_frames
                .get(&tab_id)
                .copied()
                .filter(|active| active.public == frame && active.child.program == program)
                .ok_or(JavaScriptPageDebuggerError::InvalidExecutionState)?;
            let Some(JavaScriptPageDebuggerNestedExecutionState::Paused {
                frame: still_paused,
                bytecode_offset,
            }) = self.debugger_nested_execution_state(
                tab_id,
                document_generation,
                program_handle,
                program_generation,
            )?
            else {
                return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
            };
            if still_paused != frame {
                return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
            }
            (Some(active.child), frame.code_unit_ordinal, bytecode_offset)
        } else {
            let state = self.debugger_execution_state(
                tab_id,
                document_generation,
                program_handle,
                program_generation,
            );
            let bytecode_offset = match state {
                Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable) => {
                    return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
                }
                Err(error) => return Err(error),
                Ok(state) => match state {
                    JavaScriptPageDebuggerExecutionState::Paused {
                        code_unit_ordinal: 0,
                        bytecode_offset,
                    }
                    | JavaScriptPageDebuggerExecutionState::SourceStepLimitReached {
                        code_unit_ordinal: 0,
                        bytecode_offset,
                    } => bytecode_offset,
                    _ => return Err(JavaScriptPageDebuggerError::InvalidExecutionState),
                },
            };
            (None, 0, bytecode_offset)
        };
        let reply = self
            .child
            .debugger_stack_snapshot(
                tab_id.as_u64(),
                document_generation,
                program,
                child_frame,
                max_frames,
                max_scope_entries,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerStackSnapshot {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program: reply_program,
            frame: reply_frame,
            snapshot,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || reply_program != program
            || reply_frame != child_frame
            || !valid_child_debugger_stack_snapshot(
                &snapshot,
                child_frame,
                top_code_unit,
                top_offset,
                max_frames,
                max_scope_entries,
            )
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        for stack_frame in &snapshot.frames {
            validate_child_safe_point_reply(
                self,
                tab_id,
                document_generation,
                PageHostDebuggerSafePoint {
                    program,
                    code_unit_ordinal: stack_frame.code_unit_ordinal,
                    bytecode_offset: stack_frame.bytecode_offset,
                },
            )?;
        }
        Ok(JavaScriptPageDebuggerStackSnapshot {
            frames: snapshot
                .frames
                .into_iter()
                .map(|frame| JavaScriptPageDebuggerStackFrame {
                    code_unit_ordinal: frame.code_unit_ordinal,
                    bytecode_offset: frame.bytecode_offset,
                    scope_entries: frame
                        .scope_entries
                        .into_iter()
                        .map(|entry| JavaScriptPageDebuggerScopeEntry {
                            slot_ordinal: entry.slot_ordinal,
                            scope_depth: entry.scope_depth,
                        })
                        .collect(),
                    scope_truncated: frame.scope_truncated,
                })
                .collect(),
            stack_truncated: snapshot.stack_truncated,
        })
    }

    fn debugger_stepping_available(&self) -> bool {
        self.debugger_execution_control_available() && self.child.debugger_stepping_available()
    }

    fn debugger_source_span_stepping_available(&self) -> bool {
        self.debugger_stepping_available()
            && self.debugger_static_metadata_safe_point_span_available()
            && self.child.debugger_bluets_source_span_step_available()
    }

    fn arm_debugger_root_safe_point_breakpoint(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
        code_unit_ordinal: u32,
        bytecode_offset: u32,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        if !self.debugger_execution_control_available() || code_unit_ordinal != 0 {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        let safe_point = PageHostDebuggerSafePoint {
            program: self.child_program_for_core(
                tab_id,
                document_generation,
                program_handle,
                program_generation,
            )?,
            code_unit_ordinal,
            bytecode_offset,
        };
        validate_child_safe_point_reply(self, tab_id, document_generation, safe_point)?;
        let reply = self
            .child
            .arm_debugger_root_safe_point_breakpoint(
                tab_id.as_u64(),
                document_generation,
                safe_point,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        match reply {
            PageHostReply::DebuggerRootSafePointBreakpointArmed {
                tab_id: reply_tab_id,
                document_generation: reply_generation,
                safe_point: reply_safe_point,
            } if reply_tab_id == tab_id.as_u64()
                && reply_generation == document_generation
                && reply_safe_point == safe_point =>
            {
                Ok(())
            }
            PageHostReply::Error { .. } => Err(child_debugger_reply_error(&reply)),
            _ => Err(JavaScriptPageDebuggerError::NoLiveRealm),
        }
    }

    fn debugger_execution_state(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
    ) -> Result<JavaScriptPageDebuggerExecutionState, JavaScriptPageDebuggerError> {
        if !self.debugger_execution_control_available() {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        let program = self.child_program_for_core(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        let reply = self
            .child
            .debugger_execution_state(tab_id.as_u64(), document_generation, program)
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerExecutionState {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program: reply_program,
            state,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || reply_program != program
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        match state {
            PageHostDebuggerExecutionState::Pending => {
                Ok(JavaScriptPageDebuggerExecutionState::Pending)
            }
            PageHostDebuggerExecutionState::Stepping => {
                Ok(JavaScriptPageDebuggerExecutionState::Stepping)
            }
            PageHostDebuggerExecutionState::Resuming => {
                Ok(JavaScriptPageDebuggerExecutionState::Resuming)
            }
            PageHostDebuggerExecutionState::Completed => {
                Ok(JavaScriptPageDebuggerExecutionState::Completed)
            }
            PageHostDebuggerExecutionState::Paused { safe_point }
                if safe_point.program == program && safe_point.code_unit_ordinal == 0 =>
            {
                validate_child_safe_point_reply(self, tab_id, document_generation, safe_point)?;
                Ok(JavaScriptPageDebuggerExecutionState::Paused {
                    code_unit_ordinal: safe_point.code_unit_ordinal,
                    bytecode_offset: safe_point.bytecode_offset,
                })
            }
            PageHostDebuggerExecutionState::SourceStepLimitReached { safe_point }
                if safe_point.program == program && safe_point.code_unit_ordinal == 0 =>
            {
                validate_child_safe_point_reply(self, tab_id, document_generation, safe_point)?;
                Ok(
                    JavaScriptPageDebuggerExecutionState::SourceStepLimitReached {
                        code_unit_ordinal: safe_point.code_unit_ordinal,
                        bytecode_offset: safe_point.bytecode_offset,
                    },
                )
            }
            PageHostDebuggerExecutionState::Paused { .. }
            | PageHostDebuggerExecutionState::SourceStepLimitReached { .. } => {
                Err(JavaScriptPageDebuggerError::NoLiveRealm)
            }
            PageHostDebuggerExecutionState::NestedPaused { .. }
            | PageHostDebuggerExecutionState::NestedStepping { .. }
            | PageHostDebuggerExecutionState::NestedResuming { .. } => {
                // Root-only state cannot alias a live nested frame.
                Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable)
            }
        }
    }

    fn resume_debugger_execution(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        if !self.debugger_execution_control_available() {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        let program = self.child_program_for_core(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        let reply = self
            .child
            .resume_debugger_execution(tab_id.as_u64(), document_generation, program)
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        match reply {
            PageHostReply::DebuggerExecutionResumed {
                tab_id: reply_tab_id,
                document_generation: reply_generation,
                program: reply_program,
            } if reply_tab_id == tab_id.as_u64()
                && reply_generation == document_generation
                && reply_program == program =>
            {
                Ok(())
            }
            PageHostReply::Error { .. } => Err(child_debugger_reply_error(&reply)),
            _ => Err(JavaScriptPageDebuggerError::NoLiveRealm),
        }
    }

    fn step_debugger_root_instruction(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        if !self.debugger_stepping_available() {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        let program = self.child_program_for_core(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        let reply = self
            .child
            .step_debugger_root_instruction(tab_id.as_u64(), document_generation, program)
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        match reply {
            PageHostReply::DebuggerExecutionStepRequested {
                tab_id: reply_tab_id,
                document_generation: reply_generation,
                program: reply_program,
            } if reply_tab_id == tab_id.as_u64()
                && reply_generation == document_generation
                && reply_program == program =>
            {
                Ok(())
            }
            PageHostReply::Error { .. } => Err(child_debugger_reply_error(&reply)),
            _ => Err(JavaScriptPageDebuggerError::NoLiveRealm),
        }
    }

    fn step_debugger_bluets_source_span(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        target: JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        // Compiler-minted source IDs are zero-based. The stream receipt and
        // child attachment, not a nonzero test, establish their authority.
        if !self.debugger_source_span_stepping_available() {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        let child_program = self.child_program_for_core(
            tab_id,
            document_generation,
            target.program_handle,
            target.program_generation,
        )?;
        let child_metadata = self.child_static_metadata_for_core(
            tab_id,
            document_generation,
            child_program,
            target.metadata_handle,
            target.metadata_generation,
        )?;
        let safe_point = PageHostDebuggerSafePoint {
            program: child_program,
            code_unit_ordinal: target.code_unit_ordinal,
            bytecode_offset: target.bytecode_offset,
        };
        let reply = self
            .child
            .step_debugger_bluets_source_span(
                tab_id.as_u64(),
                document_generation,
                child_metadata,
                target.source_id,
                safe_point,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        match reply {
            PageHostReply::DebuggerBlueTsSourceStepRequested {
                tab_id: reply_tab_id,
                document_generation: reply_generation,
                metadata,
                source_id,
                safe_point: reply_safe_point,
            } if reply_tab_id == tab_id.as_u64()
                && reply_generation == document_generation
                && metadata == child_metadata
                && source_id == target.source_id
                && reply_safe_point == safe_point =>
            {
                Ok(())
            }
            PageHostReply::Error { .. } => Err(child_debugger_reply_error(&reply)),
            _ => Err(JavaScriptPageDebuggerError::NoLiveRealm),
        }
    }
}

impl<C> OutOfProcessJavaScriptPageExecutor<C> {
    fn has_core_live_document(&self, tab_id: TabId, document_generation: u64) -> bool {
        self.live_documents
            .get(&tab_id)
            .is_some_and(|document| document.document_generation == document_generation)
    }

    fn mint_core_debugger_program(
        &mut self,
    ) -> Result<CoreDebuggerProgram, JavaScriptPageDebuggerError> {
        let program_handle = self.next_debugger_program_handle;
        let program_generation = self.next_debugger_program_generation;
        self.next_debugger_program_handle = program_handle
            .checked_add(1)
            .ok_or(JavaScriptPageDebuggerError::ResourceLimit)?;
        self.next_debugger_program_generation = program_generation
            .checked_add(1)
            .ok_or(JavaScriptPageDebuggerError::ResourceLimit)?;
        Ok(CoreDebuggerProgram {
            program_handle,
            program_generation,
        })
    }

    fn mint_core_debugger_static_metadata(
        &mut self,
        program: PageHostDebuggerProgram,
    ) -> Result<CoreDebuggerStaticMetadata, JavaScriptPageDebuggerError> {
        let metadata_handle = self.next_debugger_metadata_handle;
        let metadata_generation = self.next_debugger_metadata_generation;
        // Never allow an exhausted metadata counter to cross into the public
        // program namespace; a wrap must fail closed instead of aliasing an
        // unrelated public identifier.
        if metadata_handle >= CORE_CHILD_DEBUGGER_ID_NAMESPACE_START
            || metadata_generation >= CORE_CHILD_DEBUGGER_ID_NAMESPACE_START
        {
            return Err(JavaScriptPageDebuggerError::ResourceLimit);
        }
        self.next_debugger_metadata_handle = metadata_handle
            .checked_add(1)
            .ok_or(JavaScriptPageDebuggerError::ResourceLimit)?;
        self.next_debugger_metadata_generation = metadata_generation
            .checked_add(1)
            .ok_or(JavaScriptPageDebuggerError::ResourceLimit)?;
        Ok(CoreDebuggerStaticMetadata {
            program,
            metadata_handle,
            metadata_generation,
        })
    }

    fn child_program_for_core(
        &self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
    ) -> Result<PageHostDebuggerProgram, JavaScriptPageDebuggerError> {
        if !self.has_core_live_document(tab_id, document_generation) {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        self.debugger_programs
            .get(&tab_id)
            .and_then(|programs| {
                programs.iter().find_map(|(child, public)| {
                    (public.program_handle == program_handle
                        && public.program_generation == program_generation)
                        .then_some(*child)
                })
            })
            .ok_or(JavaScriptPageDebuggerError::UnknownProgram)
    }

    fn remint_core_debugger_frame(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
        child: PageHostDebuggerFrame,
    ) -> Result<JavaScriptPageDebuggerFrame, JavaScriptPageDebuggerError> {
        if let Some(active) = self.debugger_nested_frames.get(&tab_id) {
            if active.child == child
                && active.public.document_generation == document_generation
                && active.public.program_handle == program_handle
                && active.public.program_generation == program_generation
            {
                return Ok(active.public);
            }
        }
        let frame_handle = NEXT_CORE_DEBUGGER_FRAME_HANDLE
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .map_err(|_| JavaScriptPageDebuggerError::ResourceLimit)?;
        let core_instance = core_debugger_instance()?;
        let public = JavaScriptPageDebuggerFrame {
            tab_id,
            document_generation,
            program_handle,
            program_generation,
            code_unit_ordinal: child.code_unit_ordinal,
            core_instance,
            frame_handle,
        };
        self.debugger_nested_frames
            .insert(tab_id, ActiveDebuggerFrame { public, child });
        Ok(public)
    }

    fn child_static_metadata_for_core(
        &self,
        tab_id: TabId,
        document_generation: u64,
        child_program: PageHostDebuggerProgram,
        metadata_handle: u64,
        metadata_generation: u64,
    ) -> Result<PageHostDebuggerMetadataHandle, JavaScriptPageDebuggerError> {
        if !self.has_core_live_document(tab_id, document_generation) {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        self.debugger_static_metadata
            .get(&tab_id)
            .and_then(|metadata| {
                metadata.iter().find_map(|(child_metadata, public)| {
                    (public.program == child_program
                        && public.metadata_handle == metadata_handle
                        && public.metadata_generation == metadata_generation)
                        .then_some(*child_metadata)
                })
            })
            .ok_or(JavaScriptPageDebuggerError::UnknownProgram)
    }

    fn core_program_for_child(
        &self,
        tab_id: TabId,
        document_generation: u64,
        child_program: PageHostDebuggerProgram,
    ) -> Result<CoreDebuggerProgram, JavaScriptPageDebuggerError> {
        if !child_program.is_well_formed()
            || !self.has_core_live_document(tab_id, document_generation)
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        self.debugger_programs
            .get(&tab_id)
            .and_then(|programs| programs.get(&child_program))
            .copied()
            .ok_or(JavaScriptPageDebuggerError::UnknownProgram)
    }
}

fn has_duplicate_child_programs(programs: &[PageHostDebuggerProgram]) -> bool {
    let mut seen = BTreeSet::new();
    programs.iter().any(|program| !seen.insert(*program))
}

fn has_duplicate_child_safe_points(safe_points: &[PageHostDebuggerSafePoint]) -> bool {
    let mut seen = BTreeSet::new();
    safe_points
        .iter()
        .any(|safe_point| !safe_point.is_well_formed() || !seen.insert(*safe_point))
}

fn has_duplicate_child_static_metadata(metadata: &[PageHostDebuggerMetadataHandle]) -> bool {
    let mut seen = BTreeSet::new();
    metadata.iter().any(|metadata| !seen.insert(*metadata))
}

fn has_duplicate_child_static_metadata_source_ids(
    sources: &[blueice_ipc::page_host::PageHostDebuggerBlueTsMetadataSourceId],
) -> bool {
    let mut seen = BTreeSet::new();
    sources.iter().any(|source| !seen.insert(source.source_id))
}

fn has_duplicate_child_static_metadata_type_ids(
    types: &[blueice_ipc::page_host::PageHostDebuggerBlueTsMetadataTypeId],
) -> bool {
    let mut seen = BTreeSet::new();
    types
        .iter()
        .any(|static_type| !seen.insert(static_type.type_id))
}

fn has_duplicate_child_static_metadata_symbol_ids(
    symbols: &[blueice_ipc::page_host::PageHostDebuggerBlueTsMetadataSymbolId],
) -> bool {
    let mut seen = BTreeSet::new();
    symbols.iter().any(|symbol| !seen.insert(symbol.symbol_id))
}

fn valid_child_static_metadata_symbol_location(
    location: PageHostDebuggerBlueTsMetadataSymbolLocation,
    symbol_id: u32,
    source_id: u32,
) -> bool {
    location.symbol_id == symbol_id
        && location.source_id == source_id
        && location
            .coordinates
            .is_well_formed_for_range(location.start_byte, location.end_byte)
}

fn valid_child_static_metadata_symbol_type(
    symbol_type: PageHostDebuggerBlueTsMetadataSymbolType,
    symbol_id: u32,
    type_id: u32,
) -> bool {
    symbol_type.symbol_id == symbol_id && symbol_type.type_id == type_id
}

fn valid_child_static_metadata_symbol_contract(
    symbol_contract: PageHostDebuggerBlueTsMetadataSymbolContract,
    symbol_id: u32,
    contract_id: u32,
) -> bool {
    symbol_contract.symbol_id == symbol_id && symbol_contract.contract_id == contract_id
}

fn has_duplicate_child_static_metadata_contract_ids(
    contracts: &[blueice_ipc::page_host::PageHostDebuggerBlueTsMetadataContractId],
) -> bool {
    let mut seen = BTreeSet::new();
    contracts
        .iter()
        .any(|contract| !seen.insert(contract.contract_id))
}

fn valid_child_debugger_stack_snapshot(
    snapshot: &PageHostDebuggerStackSnapshot,
    child_frame: Option<PageHostDebuggerFrame>,
    top_code_unit: u32,
    top_offset: u32,
    max_frames: u32,
    max_scope_entries: u32,
) -> bool {
    let nested = child_frame.is_some();
    let expected_frames = if nested && max_frames > 1 { 2 } else { 1 };
    if snapshot.frames.len() != expected_frames
        || snapshot.stack_truncated != (nested && max_frames == 1)
        || snapshot.frames[0].code_unit_ordinal != top_code_unit
        || snapshot.frames[0].bytecode_offset != top_offset
        || (expected_frames == 2 && snapshot.frames[1].code_unit_ordinal != 0)
    {
        return false;
    }
    snapshot.frames.iter().all(|frame| {
        frame.scope_entries.len() <= max_scope_entries as usize
            && (!frame.scope_truncated || frame.scope_entries.len() == max_scope_entries as usize)
            && frame
                .scope_entries
                .windows(2)
                .all(|pair| pair[0].scope_depth <= pair[1].scope_depth)
    })
}

fn validate_child_safe_point_reply<C: PageHostClient>(
    executor: &mut OutOfProcessJavaScriptPageExecutor<C>,
    tab_id: TabId,
    document_generation: u64,
    safe_point: PageHostDebuggerSafePoint,
) -> Result<(), JavaScriptPageDebuggerError> {
    let reply = executor
        .child
        .validate_debugger_safe_point(tab_id.as_u64(), document_generation, safe_point)
        .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
    match reply {
        PageHostReply::DebuggerSafePointValidated {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            safe_point: reply_safe_point,
        } if reply_tab_id == tab_id.as_u64()
            && reply_generation == document_generation
            && reply_safe_point == safe_point =>
        {
            Ok(())
        }
        PageHostReply::Error { .. } => Err(child_debugger_reply_error(&reply)),
        _ => Err(JavaScriptPageDebuggerError::NoLiveRealm),
    }
}

fn child_debugger_reply_error(reply: &PageHostReply) -> JavaScriptPageDebuggerError {
    match reply {
        PageHostReply::Error {
            code: PageHostErrorCode::ResourceLimit,
            ..
        } => JavaScriptPageDebuggerError::ResourceLimit,
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidDebuggerState,
            ..
        } => JavaScriptPageDebuggerError::InvalidExecutionState,
        // Treat every other unexpected/private-child response as a lost live
        // realm. The public debugger must not infer a child registry state,
        // source identity, VM result, or a usable fallback target from it.
        _ => JavaScriptPageDebuggerError::NoLiveRealm,
    }
}

/// An exact, already-receipted relation can be false without losing the live
/// realm. The child intentionally uses a generic private InvalidRequest for
/// either no relation or an unknown pair; expose only public InvalidTarget.
fn child_static_metadata_relation_reply_error(
    reply: &PageHostReply,
) -> JavaScriptPageDebuggerError {
    match reply {
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        } => JavaScriptPageDebuggerError::UnknownProgram,
        _ => child_debugger_reply_error(reply),
    }
}

fn authorized_document(
    tab_id: TabId,
    identity: &LiveDocument,
    snapshot: PageHostDocumentSnapshot,
    document_url: &str,
    declarations: Vec<CombinedPageScriptDeclaration>,
    external_source_authorizer: Option<&dyn OutOfProcessPageScriptSourceAuthorizer>,
    debugger_execution_control: bool,
) -> (
    PageHostDocument,
    Vec<JavaScriptPageExecutionReport>,
    Vec<BlueTsPageExecutionReport>,
) {
    let mut scripts = Vec::new();
    let mut reports = Vec::new();
    let mut blue_ts_reports = Vec::new();
    for declaration in declarations {
        match declaration {
            CombinedPageScriptDeclaration::Inline {
                ordinal,
                language,
                source,
            } => {
                let module_id =
                    inline_module_id(tab_id, identity.document_generation, ordinal, language);
                scripts.push(PageHostScript {
                    ordinal,
                    language: child_language(language),
                    kind: child_kind(language),
                    graph: PageHostModuleGraph {
                        entry: module_id.clone(),
                        modules: vec![PageHostSource::new(module_id, source)],
                        resolutions: Vec::new(),
                        resolver_fingerprint: INLINE_CHILD_RESOLVER_FINGERPRINT.to_string(),
                    },
                });
            }
            CombinedPageScriptDeclaration::External {
                ordinal,
                language,
                src,
            } => match authorize_external_graph(
                tab_id,
                identity.document_generation,
                ordinal,
                language,
                document_url,
                src,
                external_source_authorizer,
            ) {
                Ok(graph) => scripts.push(PageHostScript {
                    ordinal,
                    language: child_language(language),
                    kind: child_kind(language),
                    graph,
                }),
                Err(category) => match language {
                    CombinedPageScriptLanguage::JavaScript(kind) => reports.push(rejected_report(
                        tab_id,
                        identity.document_generation,
                        ordinal,
                        kind,
                        category,
                    )),
                    CombinedPageScriptLanguage::BlueTs(kind) => {
                        blue_ts_reports.push(rejected_blue_ts_report(
                            tab_id,
                            identity.document_generation,
                            ordinal,
                            kind,
                            category,
                        ))
                    }
                },
            },
        }
    }
    (
        PageHostDocument {
            tab_id: tab_id.as_u64(),
            document_generation: identity.document_generation,
            snapshot,
            debugger_execution_control,
            scripts,
        },
        reports,
        blue_ts_reports,
    )
}

/// Obtains a graph only through the startup-selected core authorizer and
/// converts the typed result into the private child wire record. This adapter
/// deliberately has no fallback graph, URL normalization, import-map lookup,
/// cache lookup, network operation, or filesystem operation of its own.
#[allow(clippy::too_many_arguments)]
fn authorize_external_graph(
    tab_id: TabId,
    document_generation: u64,
    ordinal: u32,
    language: CombinedPageScriptLanguage,
    document_url: &str,
    declared_src: String,
    authorizer: Option<&dyn OutOfProcessPageScriptSourceAuthorizer>,
) -> Result<PageHostModuleGraph, &'static str> {
    let Some(authorizer) = authorizer else {
        return Err(external_loader_required_category(language));
    };
    let graph = authorizer
        .authorize(&OutOfProcessPageScriptSourceRequest {
            tab_id,
            document_generation,
            ordinal,
            language,
            document_url: document_url.to_string(),
            declared_src,
        })
        .map_err(|_| external_authorization_rejected_category(language))?;
    let graph = match (language, graph) {
        (
            CombinedPageScriptLanguage::JavaScript(_),
            AuthorizedOutOfProcessPageScriptGraph::JavaScript(graph),
        ) => page_host_graph_from_javascript(graph),
        (
            CombinedPageScriptLanguage::BlueTs(_),
            AuthorizedOutOfProcessPageScriptGraph::BlueTs(graph),
        ) => page_host_graph_from_bluets(graph),
        _ => return Err(external_authorization_rejected_category(language)),
    };
    if !valid_page_host_graph(&graph) {
        return Err(external_authorization_rejected_category(language));
    }
    Ok(graph)
}

fn external_loader_required_category(language: CombinedPageScriptLanguage) -> &'static str {
    match language {
        CombinedPageScriptLanguage::JavaScript(_) => {
            "external JavaScript declarations require an authorized loader"
        }
        CombinedPageScriptLanguage::BlueTs(_) => {
            "external BlueTS declarations require an authorized loader"
        }
    }
}

fn external_authorization_rejected_category(language: CombinedPageScriptLanguage) -> &'static str {
    match language {
        CombinedPageScriptLanguage::JavaScript(_) => {
            "external JavaScript source authorization rejected the page script"
        }
        CombinedPageScriptLanguage::BlueTs(_) => {
            "external BlueTS source authorization rejected the page script"
        }
    }
}

/// Copies the existing typed JavaScript graph without changing any selected
/// canonical module ID, static edge, or resolver-policy fingerprint.
fn page_host_graph_from_javascript(graph: AuthorizedJavaScriptModuleGraph) -> PageHostModuleGraph {
    PageHostModuleGraph {
        entry: graph.entry().to_string(),
        modules: graph
            .authorized_modules()
            .map(|module| PageHostSource::new(module.canonical_module_id(), module.source()))
            .collect(),
        resolutions: graph
            .authorized_resolutions()
            .map(|resolution| PageHostStaticResolution {
                from_module: resolution.from_module,
                specifier: resolution.specifier,
                canonical_target: resolution.canonical_target,
            })
            .collect(),
        resolver_fingerprint: graph.resolver_fingerprint().to_string(),
    }
}

/// Copies the existing typed BlueTS graph without exposing its loader as an
/// ambient resolver to either the child or the page.
fn page_host_graph_from_bluets(graph: AuthorizedPageScriptGraph) -> PageHostModuleGraph {
    let AuthorizedPageScriptGraph {
        entry,
        loader,
        resolver_fingerprint,
    } = graph;
    PageHostModuleGraph {
        entry,
        modules: loader
            .authorized_modules()
            .map(|(module_id, source)| PageHostSource::new(module_id, source))
            .collect(),
        resolutions: loader
            .authorized_resolutions()
            .map(
                |(from_module, specifier, canonical_target)| PageHostStaticResolution {
                    from_module: from_module.to_string(),
                    specifier: specifier.to_string(),
                    canonical_target: canonical_target.to_string(),
                },
            )
            .collect(),
        resolver_fingerprint,
    }
}

/// Rejects malformed typed-graph conversions before the core sends a document
/// to the child. The child repeats independent graph, hash, budget, syntax,
/// and static-edge validation after the protocol boundary, so a malformed or
/// tampered wire record cannot acquire an implicit fallback resolver.
fn valid_page_host_graph(graph: &PageHostModuleGraph) -> bool {
    if graph.entry.is_empty()
        || graph.entry.contains('\0')
        || graph.resolver_fingerprint.trim().is_empty()
        || graph.resolver_fingerprint.contains('\0')
    {
        return false;
    }
    let mut modules = BTreeSet::new();
    for source in &graph.modules {
        if source.canonical_module_id.is_empty()
            || source.canonical_module_id.contains('\0')
            || source.source_hash != page_host::source_hash(&source.source)
            || !modules.insert(source.canonical_module_id.as_str())
        {
            return false;
        }
    }
    if !modules.contains(graph.entry.as_str()) {
        return false;
    }
    let mut resolutions = BTreeSet::new();
    graph.resolutions.iter().all(|resolution| {
        !resolution.from_module.is_empty()
            && !resolution.specifier.is_empty()
            && !resolution.canonical_target.is_empty()
            && !resolution.from_module.contains('\0')
            && !resolution.specifier.contains('\0')
            && !resolution.canonical_target.contains('\0')
            && modules.contains(resolution.from_module.as_str())
            && modules.contains(resolution.canonical_target.as_str())
            && resolutions.insert((
                resolution.from_module.as_str(),
                resolution.specifier.as_str(),
            ))
    })
}

/// Builds the only document values that the core may serialize as child
/// bindings. The page neither selects their names/profile nor provides a
/// capability token. The pure contracts match the child protocol's fixed
/// byte budgets, while `live_page_identity` already derives the tuple origin
/// from an admitted HTTP(S) document rather than page script input.
fn core_document_snapshot(
    page: &Page,
    identity: &LiveDocument,
) -> Result<PageHostDocumentSnapshot, ()> {
    let document_text = page.script_document_text_content();
    let document_origin = identity.origin.clone();
    let limits = CoreScriptBindingContractLimits::default();
    core_script_binding_contract("dom.document-text")
        .expect("the fixed document-text binding has a contract inventory entry")
        .validate_string(&document_text, limits.document_text)
        .map_err(|_| ())?;
    core_script_binding_contract("dom.document-origin")
        .expect("the fixed document-origin binding has a contract inventory entry")
        .validate_string(&document_origin, limits.document_origin)
        .map_err(|_| ())?;
    if canonical_http_origin(&document_origin).ok().as_deref() != Some(document_origin.as_str()) {
        return Err(());
    }
    Ok(PageHostDocumentSnapshot {
        document_text,
        document_origin,
    })
}

fn binding_contract_rejections(
    tab_id: TabId,
    document_generation: u64,
    declarations: Vec<CombinedPageScriptDeclaration>,
) -> (
    Vec<JavaScriptPageExecutionReport>,
    Vec<BlueTsPageExecutionReport>,
) {
    let mut java_script_reports = Vec::new();
    let mut blue_ts_reports = Vec::new();
    for declaration in declarations {
        match declaration {
            CombinedPageScriptDeclaration::Inline {
                ordinal, language, ..
            }
            | CombinedPageScriptDeclaration::External {
                ordinal, language, ..
            } => match language {
                CombinedPageScriptLanguage::JavaScript(kind) => {
                    java_script_reports.push(rejected_report(
                        tab_id,
                        document_generation,
                        ordinal,
                        kind,
                        "host binding contract rejected the page script",
                    ))
                }
                CombinedPageScriptLanguage::BlueTs(kind) => {
                    blue_ts_reports.push(rejected_blue_ts_report(
                        tab_id,
                        document_generation,
                        ordinal,
                        kind,
                        "host binding contract rejected the page script",
                    ))
                }
            },
        }
    }
    (java_script_reports, blue_ts_reports)
}

fn live_page_identity(page: &Page) -> Option<LiveDocument> {
    let origin = canonical_http_origin(page.url()?).ok()?;
    Some(LiveDocument {
        document_generation: page.document_generation(),
        origin,
    })
}

fn inline_module_id(
    tab_id: TabId,
    document_generation: u64,
    ordinal: u32,
    language: CombinedPageScriptLanguage,
) -> String {
    let extension = match language {
        CombinedPageScriptLanguage::JavaScript(_) => "js",
        CombinedPageScriptLanguage::BlueTs(_) => "ts",
    };
    format!(
        "blueice://page/tab-{}/document-{document_generation}/inline-{ordinal}.{extension}",
        tab_id.as_u64()
    )
}

enum ChildExecutionReport {
    JavaScript(JavaScriptPageExecutionReport),
    BlueTs(BlueTsPageExecutionReport),
}

fn child_report(report: page_host::PageHostScriptReport) -> ChildExecutionReport {
    match report.language {
        PageHostScriptLanguage::JavaScript => match report.outcome {
            PageHostScriptOutcome::Executed => {
                ChildExecutionReport::JavaScript(JavaScriptPageExecutionReport::Executed {
                    tab_id: report.tab_id,
                    document_generation: report.document_generation,
                    ordinal: report.ordinal,
                    kind: core_js_kind(report.kind),
                })
            }
            PageHostScriptOutcome::Rejected { category } => {
                ChildExecutionReport::JavaScript(JavaScriptPageExecutionReport::Rejected {
                    tab_id: report.tab_id,
                    document_generation: report.document_generation,
                    ordinal: report.ordinal,
                    kind: core_js_kind(report.kind),
                    category: leak_category(category),
                })
            }
        },
        PageHostScriptLanguage::BlueTs => match report.outcome {
            PageHostScriptOutcome::Executed => {
                ChildExecutionReport::BlueTs(BlueTsPageExecutionReport::Executed {
                    tab_id: report.tab_id,
                    document_generation: report.document_generation,
                    ordinal: report.ordinal,
                    kind: core_blue_ts_kind(report.kind),
                })
            }
            PageHostScriptOutcome::Rejected { category } => {
                ChildExecutionReport::BlueTs(BlueTsPageExecutionReport::Rejected {
                    tab_id: report.tab_id,
                    document_generation: report.document_generation,
                    ordinal: report.ordinal,
                    kind: core_blue_ts_kind(report.kind),
                    category: leak_blue_ts_category(category),
                })
            }
        },
    }
}

// `JavaScriptPageExecutionReport` predates the child transport and stores
// fixed categories as `&'static str`. The child receives only categories this
// core itself recognizes; normalize unknown/new labels to one fixed value
// rather than retaining peer-owned allocation beyond a request.
fn leak_category(category: String) -> &'static str {
    match category.as_str() {
        "JavaScript parsing rejected the page script" => {
            "JavaScript parsing rejected the page script"
        }
        "BlueJS compilation rejected the page script" => {
            "BlueJS compilation rejected the page script"
        }
        "JavaScript page resource policy rejected the page script" => {
            "JavaScript page resource policy rejected the page script"
        }
        "authorized JavaScript graph rejected the page script" => {
            "authorized JavaScript graph rejected the page script"
        }
        "BlueJS page execution failed" => "BlueJS page execution failed",
        "BlueJS page host rejected the page script" => "BlueJS page host rejected the page script",
        "authorized JavaScript source record is invalid" => {
            "authorized JavaScript source record is invalid"
        }
        "authorized JavaScript graph is missing a static resolution" => {
            "authorized JavaScript graph is missing a static resolution"
        }
        _ => "out-of-process JavaScript host rejected the page script",
    }
}

fn leak_blue_ts_category(category: String) -> &'static str {
    match category.as_str() {
        "BlueTS compilation rejected the page script" => {
            "BlueTS compilation rejected the page script"
        }
        "BlueTS direct lowering rejected the page script" => {
            "BlueTS direct lowering rejected the page script"
        }
        "BlueJS compilation rejected the direct BlueTS page script" => {
            "BlueJS compilation rejected the direct BlueTS page script"
        }
        "JavaScript page resource policy rejected the page script" => {
            "JavaScript page resource policy rejected the page script"
        }
        "authorized JavaScript graph rejected the page script" => {
            "authorized JavaScript graph rejected the page script"
        }
        "BlueJS page execution failed" => "BlueJS page execution failed",
        "BlueJS page host rejected the page script" => "BlueJS page host rejected the page script",
        "authorized BlueTS graph is invalid" => "authorized BlueTS graph is invalid",
        _ => "out-of-process BlueTS host rejected the page script",
    }
}

fn child_error_category(code: PageHostErrorCode) -> &'static str {
    match code {
        PageHostErrorCode::ResourceLimit => {
            "out-of-process JavaScript host resource policy rejected the document"
        }
        PageHostErrorCode::StaleDocument => {
            "out-of-process JavaScript host rejected a stale document"
        }
        PageHostErrorCode::Authentication
        | PageHostErrorCode::ProtocolVersion
        | PageHostErrorCode::InvalidRequest
        | PageHostErrorCode::InvalidDebuggerState
        | PageHostErrorCode::UnknownRealm
        | PageHostErrorCode::HostFailure => "out-of-process JavaScript host rejected the document",
    }
}

fn rejected_report(
    tab_id: TabId,
    document_generation: u64,
    ordinal: u32,
    kind: BlueJsPageScriptKind,
    category: &'static str,
) -> JavaScriptPageExecutionReport {
    JavaScriptPageExecutionReport::Rejected {
        tab_id: tab_id.as_u64(),
        document_generation,
        ordinal,
        kind,
        category,
    }
}

fn rejected_blue_ts_report(
    tab_id: TabId,
    document_generation: u64,
    ordinal: u32,
    kind: DirectPageScriptKind,
    category: &'static str,
) -> BlueTsPageExecutionReport {
    BlueTsPageExecutionReport::Rejected {
        tab_id: tab_id.as_u64(),
        document_generation,
        ordinal,
        kind,
        category,
    }
}

fn push_child_failure_report(
    (java_script_reports, blue_ts_reports): (
        &mut Vec<JavaScriptPageExecutionReport>,
        &mut Vec<BlueTsPageExecutionReport>,
    ),
    tab_id: TabId,
    document_generation: u64,
    ordinal: u32,
    language: PageHostScriptLanguage,
    kind: PageHostScriptKind,
    category: &'static str,
) {
    match language {
        PageHostScriptLanguage::JavaScript => java_script_reports.push(rejected_report(
            tab_id,
            document_generation,
            ordinal,
            core_js_kind(kind),
            category,
        )),
        PageHostScriptLanguage::BlueTs => blue_ts_reports.push(rejected_blue_ts_report(
            tab_id,
            document_generation,
            ordinal,
            core_blue_ts_kind(kind),
            category,
        )),
    }
}

fn child_language(language: CombinedPageScriptLanguage) -> PageHostScriptLanguage {
    match language {
        CombinedPageScriptLanguage::JavaScript(_) => PageHostScriptLanguage::JavaScript,
        CombinedPageScriptLanguage::BlueTs(_) => PageHostScriptLanguage::BlueTs,
    }
}

fn child_kind(language: CombinedPageScriptLanguage) -> PageHostScriptKind {
    match language {
        CombinedPageScriptLanguage::JavaScript(BlueJsPageScriptKind::Classic)
        | CombinedPageScriptLanguage::BlueTs(DirectPageScriptKind::Classic) => {
            PageHostScriptKind::Classic
        }
        CombinedPageScriptLanguage::JavaScript(BlueJsPageScriptKind::Module)
        | CombinedPageScriptLanguage::BlueTs(DirectPageScriptKind::Module) => {
            PageHostScriptKind::Module
        }
    }
}

fn core_js_kind(kind: PageHostScriptKind) -> BlueJsPageScriptKind {
    match kind {
        PageHostScriptKind::Classic => BlueJsPageScriptKind::Classic,
        PageHostScriptKind::Module => BlueJsPageScriptKind::Module,
    }
}

fn core_blue_ts_kind(kind: PageHostScriptKind) -> DirectPageScriptKind {
    match kind {
        PageHostScriptKind::Classic => DirectPageScriptKind::Classic,
        PageHostScriptKind::Module => DirectPageScriptKind::Module,
    }
}

fn report_tab_id(report: &JavaScriptPageExecutionReport) -> u64 {
    match report {
        JavaScriptPageExecutionReport::Executed { tab_id, .. }
        | JavaScriptPageExecutionReport::Rejected { tab_id, .. } => *tab_id,
    }
}

fn report_ordinal(report: &JavaScriptPageExecutionReport) -> u32 {
    match report {
        JavaScriptPageExecutionReport::Executed { ordinal, .. }
        | JavaScriptPageExecutionReport::Rejected { ordinal, .. } => *ordinal,
    }
}

fn blue_ts_report_tab_id(report: &BlueTsPageExecutionReport) -> u64 {
    match report {
        BlueTsPageExecutionReport::Executed { tab_id, .. }
        | BlueTsPageExecutionReport::Rejected { tab_id, .. } => *tab_id,
    }
}

fn blue_ts_report_ordinal(report: &BlueTsPageExecutionReport) -> u32 {
    match report {
        BlueTsPageExecutionReport::Executed { ordinal, .. }
        | BlueTsPageExecutionReport::Rejected { ordinal, .. } => *ordinal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::http_resource_authorizer::{
        sha256_integrity, HttpOutOfProcessPageScriptSourceAuthorizer, HttpScriptIntegrityManifest,
        HttpScriptResourceLimits, HttpScriptResourceOriginRule, HttpScriptResourcePolicy,
    };
    use crate::script::javascript::{AuthorizedJavaScriptModule, AuthorizedJavaScriptResolution};
    use blueice_bluets::{AuthorizedModule, AuthorizedModuleLoader, AuthorizedModuleResolution};
    use blueice_ipc::script::ScriptReply;
    use blueice_launcher::bluejs_host::{
        bind_bluejs_host_socket, serve_bluejs_host_listener, BlueJsChildHost,
    };
    use std::collections::BTreeMap;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;

    #[test]
    fn core_rejects_malformed_child_stack_shapes_and_scope_budgets() {
        use blueice_ipc::page_host::{PageHostDebuggerScopeEntry, PageHostDebuggerStackFrame};

        let child = PageHostDebuggerFrame {
            tab_id: 7,
            document_generation: 1,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            code_unit_ordinal: 1,
            invocation_serial: 17,
        };
        let entry = PageHostDebuggerScopeEntry {
            slot_ordinal: 2,
            scope_depth: 0,
        };
        let top = PageHostDebuggerStackFrame {
            code_unit_ordinal: 1,
            bytecode_offset: 4,
            scope_entries: vec![entry],
            scope_truncated: true,
        };
        let snapshot = PageHostDebuggerStackSnapshot {
            frames: vec![top.clone()],
            stack_truncated: true,
        };
        assert!(valid_child_debugger_stack_snapshot(
            &snapshot,
            Some(child),
            1,
            4,
            1,
            1
        ));
        let mut malformed = snapshot.clone();
        malformed.frames[0].bytecode_offset = 5;
        assert!(!valid_child_debugger_stack_snapshot(
            &malformed,
            Some(child),
            1,
            4,
            1,
            1
        ));
        malformed = snapshot.clone();
        malformed.stack_truncated = false;
        assert!(!valid_child_debugger_stack_snapshot(
            &malformed,
            Some(child),
            1,
            4,
            1,
            1
        ));
        malformed = snapshot.clone();
        malformed.frames[0].scope_entries.push(entry);
        assert!(!valid_child_debugger_stack_snapshot(
            &malformed,
            Some(child),
            1,
            4,
            1,
            1
        ));
        malformed = snapshot.clone();
        malformed.frames[0].scope_entries.clear();
        assert!(!valid_child_debugger_stack_snapshot(
            &malformed,
            Some(child),
            1,
            4,
            1,
            1
        ));
        malformed = snapshot.clone();
        malformed.frames[0]
            .scope_entries
            .push(PageHostDebuggerScopeEntry {
                slot_ordinal: 3,
                scope_depth: 0,
            });
        malformed.frames[0].scope_entries[0].scope_depth = 1;
        assert!(!valid_child_debugger_stack_snapshot(
            &malformed,
            Some(child),
            1,
            4,
            1,
            2
        ));
        let root = PageHostDebuggerStackFrame {
            code_unit_ordinal: 0,
            bytecode_offset: 8,
            scope_entries: Vec::new(),
            scope_truncated: false,
        };
        let full = PageHostDebuggerStackSnapshot {
            frames: vec![top, root],
            stack_truncated: false,
        };
        assert!(valid_child_debugger_stack_snapshot(
            &full,
            Some(child),
            1,
            4,
            2,
            1
        ));
        assert!(!valid_child_debugger_stack_snapshot(
            &full, None, 0, 8, 2, 1
        ));
        malformed = full.clone();
        malformed.frames[1].code_unit_ordinal = 1;
        assert!(!valid_child_debugger_stack_snapshot(
            &malformed,
            Some(child),
            1,
            4,
            2,
            1
        ));
    }

    struct SnapshotReplyChild {
        reply: PageHostReply,
    }

    impl PageHostClient for SnapshotReplyChild {
        fn synchronize_document(&mut self, _: PageHostDocument) -> io::Result<PageHostReply> {
            unreachable!("the test installs a core-owned identity directly")
        }

        fn close_realm(&mut self, _: u64, _: u64) -> io::Result<PageHostReply> {
            unreachable!("the test does not close the test realm")
        }

        fn debugger_execution_control_available(&self) -> bool {
            true
        }

        fn debugger_stack_snapshot_available(&self) -> bool {
            true
        }

        fn debugger_execution_state(
            &mut self,
            tab_id: u64,
            document_generation: u64,
            program: PageHostDebuggerProgram,
        ) -> io::Result<PageHostReply> {
            Ok(PageHostReply::DebuggerExecutionState {
                tab_id,
                document_generation,
                program,
                state: PageHostDebuggerExecutionState::Paused {
                    safe_point: PageHostDebuggerSafePoint {
                        program,
                        code_unit_ordinal: 0,
                        bytecode_offset: 4,
                    },
                },
            })
        }

        fn validate_debugger_safe_point(
            &mut self,
            tab_id: u64,
            document_generation: u64,
            safe_point: PageHostDebuggerSafePoint,
        ) -> io::Result<PageHostReply> {
            Ok(PageHostReply::DebuggerSafePointValidated {
                tab_id,
                document_generation,
                safe_point,
            })
        }

        fn debugger_stack_snapshot(
            &mut self,
            _: u64,
            _: u64,
            _: PageHostDebuggerProgram,
            _: Option<PageHostDebuggerFrame>,
            _: u32,
            _: u32,
        ) -> io::Result<PageHostReply> {
            Ok(self.reply.clone())
        }
    }

    #[test]
    fn core_rejects_private_snapshot_reply_identity_mismatches() {
        use blueice_ipc::page_host::PageHostDebuggerStackFrame;

        let tab_id = TabId::from_u64(7);
        let child_program = PageHostDebuggerProgram {
            program_handle: 11,
            program_generation: 13,
        };
        let reply = PageHostReply::DebuggerStackSnapshot {
            tab_id: 7,
            document_generation: 1,
            program: child_program,
            frame: None,
            snapshot: PageHostDebuggerStackSnapshot {
                frames: vec![PageHostDebuggerStackFrame {
                    code_unit_ordinal: 0,
                    bytecode_offset: 4,
                    scope_entries: Vec::new(),
                    scope_truncated: false,
                }],
                stack_truncated: false,
            },
        };
        let mut executor = OutOfProcessJavaScriptPageExecutor::new(SnapshotReplyChild {
            reply: reply.clone(),
        });
        executor.enable_debugger_execution_control();
        executor.live_documents.insert(
            tab_id,
            LiveDocument {
                document_generation: 1,
                origin: "https://example.test".into(),
            },
        );
        executor.debugger_programs.insert(
            tab_id,
            BTreeMap::from([(
                child_program,
                CoreDebuggerProgram {
                    program_handle: 101,
                    program_generation: 103,
                },
            )]),
        );
        let request = |executor: &mut OutOfProcessJavaScriptPageExecutor<SnapshotReplyChild>| {
            executor.debugger_stack_snapshot(
                tab_id,
                1,
                JavaScriptPageDebuggerProgram {
                    program_handle: 101,
                    program_generation: 103,
                },
                None,
                2,
                2,
            )
        };
        assert_eq!(request(&mut executor).unwrap().frames.len(), 1);
        let PageHostReply::DebuggerStackSnapshot { snapshot, .. } = &reply else {
            unreachable!();
        };
        let snapshot = snapshot.clone();
        for malformed in [
            PageHostReply::DebuggerStackSnapshot {
                tab_id: 8,
                document_generation: 1,
                program: child_program,
                frame: None,
                snapshot: snapshot.clone(),
            },
            PageHostReply::DebuggerStackSnapshot {
                tab_id: 7,
                document_generation: 2,
                program: child_program,
                frame: None,
                snapshot: snapshot.clone(),
            },
            PageHostReply::DebuggerStackSnapshot {
                tab_id: 7,
                document_generation: 1,
                program: PageHostDebuggerProgram {
                    program_generation: 14,
                    ..child_program
                },
                frame: None,
                snapshot: snapshot.clone(),
            },
            PageHostReply::DebuggerStackSnapshot {
                tab_id: 7,
                document_generation: 1,
                program: child_program,
                frame: Some(PageHostDebuggerFrame {
                    tab_id: 7,
                    document_generation: 1,
                    program: child_program,
                    code_unit_ordinal: 1,
                    invocation_serial: 17,
                }),
                snapshot,
            },
        ] {
            executor.child.reply = malformed;
            assert_eq!(
                request(&mut executor),
                Err(JavaScriptPageDebuggerError::NoLiveRealm)
            );
        }
    }

    #[test]
    fn child_symbol_location_rejects_malformed_original_coordinates() {
        let location = PageHostDebuggerBlueTsMetadataSymbolLocation {
            symbol_id: 3,
            source_id: 5,
            start_byte: 8,
            end_byte: 31,
            coordinates: blueice_ipc::debugger::DebuggerSourceCoordinates {
                start_line: 0,
                start_column_utf16: 8,
                end_line: 1,
                end_column_utf16: 9,
            },
        };
        assert!(valid_child_static_metadata_symbol_location(location, 3, 5));
        assert!(!valid_child_static_metadata_symbol_location(
            PageHostDebuggerBlueTsMetadataSymbolLocation {
                coordinates: blueice_ipc::debugger::DebuggerSourceCoordinates {
                    end_line: 0,
                    end_column_utf16: 8,
                    ..location.coordinates
                },
                ..location
            },
            3,
            5,
        ));
        assert!(!valid_child_static_metadata_symbol_location(
            PageHostDebuggerBlueTsMetadataSymbolLocation {
                coordinates: blueice_ipc::debugger::DebuggerSourceCoordinates {
                    end_line: 32,
                    ..location.coordinates
                },
                ..location
            },
            3,
            5,
        ));
    }

    fn unique_socket_path(label: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let count = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "blueice-engine-child-host-{label}-{}-{count}.sock",
            std::process::id()
        ))
    }

    fn spawn_child() -> (PathBuf, String, thread::JoinHandle<()>) {
        let path = unique_socket_path("lifecycle");
        let token = "0123456789abcdef0123456789abcdef".to_string();
        let listener = bind_bluejs_host_socket(&path).unwrap();
        let child_token = token.clone();
        let handle = thread::spawn(move || {
            let mut host = BlueJsChildHost::default();
            serve_bluejs_host_listener(listener, child_token, &mut host).unwrap();
        });
        (path, token, handle)
    }

    fn loaded_tabs(html: &str, url: &str) -> (TabManager, TabId) {
        let mut tabs = TabManager::new(320.0, 200.0);
        let tab_id = tabs.default_tab();
        tabs.get_mut(tab_id)
            .unwrap()
            .load_html_str(html, Some(url.to_string()));
        (tabs, tab_id)
    }

    #[test]
    fn page_host_transport_pumps_before_the_child_replies() {
        let (core, mut child) = UnixStream::pair().unwrap();
        let (resume_sender, resume_receiver) = mpsc::sync_channel(1);
        let child_task = thread::spawn(move || {
            assert_eq!(
                page_host::read_page_host_request(&mut child).unwrap(),
                PageHostRequest::Shutdown
            );
            resume_receiver
                .recv_timeout(Duration::from_secs(1))
                .unwrap();
            page_host::write_page_host_reply(&mut child, &PageHostReply::ShutdownAck).unwrap();
        });
        let session_thread = thread::current().id();
        let mut pump_calls = 0;
        let reply = PageHostConnection { stream: core }
            .request_while_pumping_script(PageHostRequest::Shutdown, &mut || {
                assert_eq!(thread::current().id(), session_thread);
                pump_calls += 1;
                if pump_calls == 1 {
                    resume_sender.send(()).unwrap();
                }
                Ok(())
            })
            .unwrap();
        child_task.join().unwrap();
        assert_eq!(reply, PageHostReply::ShutdownAck);
        assert!(pump_calls > 0);
    }

    #[test]
    fn page_host_transport_times_out_and_poison_closes_a_stalled_child() {
        let (core, mut child) = UnixStream::pair().unwrap();
        let (release_sender, release_receiver) = mpsc::sync_channel(1);
        let child_task = thread::spawn(move || {
            assert_eq!(
                page_host::read_page_host_request(&mut child).unwrap(),
                PageHostRequest::Shutdown
            );
            release_receiver
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
        });
        let mut connection = PageHostConnection { stream: core };
        let started = Instant::now();
        let error = connection
            .request_while_pumping_script_with_timeout(
                PageHostRequest::Shutdown,
                &mut || Ok(()),
                Duration::from_millis(100),
            )
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(connection.request(PageHostRequest::Shutdown).is_err());
        release_sender.send(()).unwrap();
        child_task.join().unwrap();
    }

    #[test]
    fn page_host_transport_fails_promptly_when_the_child_disconnects() {
        let (core, mut child) = UnixStream::pair().unwrap();
        let child_task = thread::spawn(move || {
            assert_eq!(
                page_host::read_page_host_request(&mut child).unwrap(),
                PageHostRequest::Shutdown
            );
        });
        let started = Instant::now();
        let error = PageHostConnection { stream: core }
            .request_while_pumping_script_with_timeout(
                PageHostRequest::Shutdown,
                &mut || Ok(()),
                Duration::from_secs(3),
            )
            .unwrap_err();
        child_task.join().unwrap();
        assert_ne!(error.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn nested_child_wait_rejects_calls_after_its_total_budget() {
        let (mut tabs, tab_id) = loaded_tabs(
            "<div id='target'>before</div>",
            "https://example.test/nested-budget.html",
        );
        let target = ScriptDocumentTarget {
            tab_id: tab_id.as_u64(),
            document_generation: tabs.get(tab_id).unwrap().document_generation(),
        };
        let (sender, receiver) = crate::script::script_request_channel();
        let first = thread::spawn({
            let sender = sender.clone();
            move || {
                sender.request(blueice_ipc::script::ScriptRequest::GetElementById {
                    target,
                    id: "target".to_string(),
                })
            }
        });
        let mut remaining = 1;
        let deadline = Instant::now() + Duration::from_secs(1);
        while remaining > 0 {
            pump_script_requests_during_child_wait(&receiver, &mut tabs, target, &mut remaining)
                .unwrap();
            assert!(
                Instant::now() < deadline,
                "first nested request did not arrive"
            );
            thread::yield_now();
        }
        let ScriptReply::Node { node: Some(node) } = first.join().unwrap().unwrap() else {
            panic!("the first request must resolve the target node");
        };
        pump_script_requests_during_child_wait(&receiver, &mut tabs, target, &mut remaining)
            .unwrap();
        let before = tabs.get(tab_id).unwrap().dom_dump();
        let excess = thread::spawn(move || {
            sender.request(blueice_ipc::script::ScriptRequest::SetTextContent {
                target,
                node,
                value: "over budget".to_string(),
            })
        });
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            if pump_script_requests_during_child_wait(&receiver, &mut tabs, target, &mut remaining)
                .is_err()
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "excess nested request did not arrive"
            );
            thread::yield_now();
        }
        assert!(matches!(
            excess.join().unwrap().unwrap(),
            ScriptReply::Error { .. }
        ));
        assert_eq!(tabs.get(tab_id).unwrap().dom_dump(), before);
    }

    struct ReentrantScriptChild {
        script_sender: crate::script::ScriptRequestSender,
    }

    fn nested_dom_write(
        sender: crate::script::ScriptRequestSender,
        target: ScriptDocumentTarget,
        value: &'static str,
        pump: &mut dyn FnMut() -> io::Result<()>,
    ) -> io::Result<()> {
        let (done_sender, done_receiver) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            let result = (|| {
                let ScriptReply::Node { node: Some(node) } =
                    sender.request(blueice_ipc::script::ScriptRequest::GetElementById {
                        target,
                        id: "target".to_string(),
                    })?
                else {
                    return Err(io::Error::other("target was not found"));
                };
                sender.request(blueice_ipc::script::ScriptRequest::SetTextContent {
                    target,
                    node,
                    value: value.to_string(),
                })
            })();
            done_sender.send(result).unwrap();
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        let script_reply = loop {
            pump()?;
            match done_receiver.recv_timeout(Duration::from_millis(10)) {
                Ok(result) => break result?,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(io::Error::other("script worker disconnected"));
                }
                Err(mpsc::RecvTimeoutError::Timeout) if Instant::now() < deadline => {}
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "nested DOM call stalled",
                    ));
                }
            }
        };
        worker.join().unwrap();
        assert_eq!(script_reply, ScriptReply::Ack);
        Ok(())
    }

    impl PageHostClient for ReentrantScriptChild {
        fn synchronize_document(
            &mut self,
            _document: PageHostDocument,
        ) -> io::Result<PageHostReply> {
            panic!("a session-owned child sync must use the nested script pump");
        }

        fn synchronize_document_with_script_pump(
            &mut self,
            document: PageHostDocument,
            pump: &mut dyn FnMut() -> io::Result<()>,
        ) -> io::Result<PageHostReply> {
            let target = ScriptDocumentTarget {
                tab_id: document.tab_id,
                document_generation: document.document_generation,
            };
            nested_dom_write(
                self.script_sender.clone(),
                target,
                "from nested script",
                pump,
            )?;
            Ok(PageHostReply::Synchronized {
                tab_id: document.tab_id,
                document_generation: document.document_generation,
                already_current: false,
                reports: Vec::new(),
            })
        }

        fn close_realm(
            &mut self,
            tab_id: u64,
            document_generation: u64,
        ) -> io::Result<PageHostReply> {
            Ok(PageHostReply::RealmClosed {
                tab_id,
                document_generation,
            })
        }

        fn advance_debugger_execution_with_script_pump(
            &mut self,
            tab_id: u64,
            document_generation: u64,
            pump: &mut dyn FnMut() -> io::Result<()>,
        ) -> io::Result<PageHostReply> {
            nested_dom_write(
                self.script_sender.clone(),
                ScriptDocumentTarget {
                    tab_id,
                    document_generation,
                },
                "from resumed script",
                pump,
            )?;
            Ok(PageHostReply::DebuggerExecutionAdvanced {
                tab_id,
                document_generation,
                reports: Vec::new(),
            })
        }
    }

    #[test]
    fn child_document_sync_serves_dom_calls_on_the_owning_session_thread() {
        let (mut tabs, tab_id) = loaded_tabs(
            "<div id='target'>before</div><script>let page = 1;</script>",
            "https://example.test/nested-dom.html",
        );
        let (script_sender, script_receiver) = crate::script::script_request_channel();
        let mut executor =
            OutOfProcessJavaScriptPageExecutor::new(ReentrantScriptChild { script_sender });
        executor
            .synchronize_and_execute_serving_script(&mut tabs, &script_receiver)
            .unwrap();
        assert!(tabs
            .get(tab_id)
            .unwrap()
            .dom_dump()
            .contains("from nested script"));
        assert_eq!(executor.live_documents.len(), 1);
    }

    #[test]
    fn debugger_resume_serves_dom_calls_on_the_owning_session_thread() {
        let (mut tabs, tab_id) = loaded_tabs(
            "<div id='target'>before</div><script>let page = 1;</script>",
            "https://example.test/nested-debugger-dom.html",
        );
        let (script_sender, script_receiver) = crate::script::script_request_channel();
        let mut executor = OutOfProcessJavaScriptPageExecutor::new_with_debugger_execution_control(
            ReentrantScriptChild { script_sender },
        );
        executor
            .synchronize_and_execute_serving_script(&mut tabs, &script_receiver)
            .unwrap();
        executor
            .synchronize_and_execute_serving_script(&mut tabs, &script_receiver)
            .unwrap();
        assert!(tabs
            .get(tab_id)
            .unwrap()
            .dom_dump()
            .contains("from resumed script"));
        assert_eq!(executor.live_documents.len(), 1);
    }

    fn shutdown_child(path: &Path, token: &str) {
        let mut stream = UnixStream::connect(path).unwrap();
        page_host::write_page_host_request(
            &mut stream,
            &PageHostRequest::Hello {
                protocol_version: page_host::PAGE_HOST_PROTOCOL_VERSION,
                session_token: token.to_string(),
            },
        )
        .unwrap();
        assert!(matches!(
            page_host::read_page_host_reply(&mut stream).unwrap(),
            PageHostReply::HelloAck { .. }
        ));
        page_host::write_page_host_request(&mut stream, &PageHostRequest::Shutdown).unwrap();
        assert_eq!(
            page_host::read_page_host_reply(&mut stream).unwrap(),
            PageHostReply::ShutdownAck
        );
    }

    #[derive(Clone)]
    struct HttpTestResponse {
        status: &'static str,
        content_type: Option<&'static str>,
        extra_headers: Vec<(&'static str, &'static str)>,
        body: String,
    }

    impl HttpTestResponse {
        fn script(content_type: &'static str, body: impl Into<String>) -> Self {
            Self {
                status: "200 OK",
                content_type: Some(content_type),
                extra_headers: Vec::new(),
                body: body.into(),
            }
        }

        fn redirect(location: &'static str) -> Self {
            Self {
                status: "302 Found",
                content_type: None,
                extra_headers: vec![("Location", location)],
                body: String::new(),
            }
        }

        fn write_to(&self, stream: &mut std::net::TcpStream) {
            let mut response = format!(
                "HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n",
                self.status,
                self.body.len()
            );
            if let Some(content_type) = self.content_type {
                response.push_str(&format!("Content-Type: {content_type}\r\n"));
            }
            for (name, value) in &self.extra_headers {
                response.push_str(&format!("{name}: {value}\r\n"));
            }
            response.push_str("\r\n");
            stream.write_all(response.as_bytes()).unwrap();
            stream.write_all(self.body.as_bytes()).unwrap();
        }
    }

    fn spawn_local_resource_server(
        responses: BTreeMap<String, HttpTestResponse>,
        expected_requests: usize,
    ) -> (String, Arc<Mutex<Vec<String>>>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let requested = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&requested);
        let thread = thread::spawn(move || {
            for _ in 0..expected_requests {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buffer = [0u8; 4096];
                let bytes = stream.read(&mut buffer).unwrap();
                let request = std::str::from_utf8(&buffer[..bytes]).unwrap();
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap()
                    .to_string();
                observed.lock().unwrap().push(path.clone());
                responses
                    .get(&path)
                    .unwrap_or(&HttpTestResponse {
                        status: "404 Not Found",
                        content_type: None,
                        extra_headers: Vec::new(),
                        body: String::new(),
                    })
                    .write_to(&mut stream);
            }
        });
        (origin, requested, thread)
    }

    fn external_javascript_graph() -> AuthorizedJavaScriptModuleGraph {
        AuthorizedJavaScriptModuleGraph::new(
            "https://cdn.example.test/assets/external.js",
            [AuthorizedJavaScriptModule::new(
                "https://cdn.example.test/assets/external.js",
                "globalThis.externalJavaScript = 41;",
            )
            .unwrap()],
            [],
            "core-external-javascript-policy-v1",
        )
        .unwrap()
    }

    fn external_bluets_graph() -> AuthorizedPageScriptGraph {
        let entry = "https://cdn.example.test/assets/external.ts";
        let dependency = "https://cdn.example.test/assets/answer.ts";
        let loader = AuthorizedModuleLoader::new(
            [
                AuthorizedModule::new(
                    entry,
                    "import { answer } from './answer.ts'; export const result: number = answer;",
                ),
                AuthorizedModule::new(dependency, "export const answer: number = 41;"),
            ],
            [AuthorizedModuleResolution::new(
                entry,
                "./answer.ts",
                dependency,
            )],
        )
        .unwrap();
        AuthorizedPageScriptGraph::new(entry, loader, "core-external-bluets-policy-v1").unwrap()
    }

    struct ExternalGraphsAuthorizer {
        requests: Arc<Mutex<Vec<OutOfProcessPageScriptSourceRequest>>>,
    }

    impl OutOfProcessPageScriptSourceAuthorizer for ExternalGraphsAuthorizer {
        fn authorize(
            &self,
            request: &OutOfProcessPageScriptSourceRequest,
        ) -> Result<
            AuthorizedOutOfProcessPageScriptGraph,
            OutOfProcessPageScriptSourceAuthorizationError,
        > {
            self.requests.lock().unwrap().push(request.clone());
            match (
                request.document_generation,
                request.ordinal,
                request.language,
                request.document_url.as_str(),
                request.declared_src.as_str(),
            ) {
                (
                    1,
                    0,
                    CombinedPageScriptLanguage::JavaScript(BlueJsPageScriptKind::Classic),
                    "https://example.test/app/index.html",
                    "/assets/external.js",
                ) => Ok(AuthorizedOutOfProcessPageScriptGraph::JavaScript(
                    external_javascript_graph(),
                )),
                (
                    1,
                    1,
                    CombinedPageScriptLanguage::BlueTs(DirectPageScriptKind::Module),
                    "https://example.test/app/index.html",
                    "/assets/external.ts",
                ) => Ok(AuthorizedOutOfProcessPageScriptGraph::BlueTs(
                    external_bluets_graph(),
                )),
                _ => Err(OutOfProcessPageScriptSourceAuthorizationError::new(
                    "unexpected private test authorization request",
                )),
            }
        }
    }

    struct DeniedAndInvalidGraphsAuthorizer;

    impl OutOfProcessPageScriptSourceAuthorizer for DeniedAndInvalidGraphsAuthorizer {
        fn authorize(
            &self,
            request: &OutOfProcessPageScriptSourceRequest,
        ) -> Result<
            AuthorizedOutOfProcessPageScriptGraph,
            OutOfProcessPageScriptSourceAuthorizationError,
        > {
            match request.language {
                CombinedPageScriptLanguage::JavaScript(_) => {
                    Err(OutOfProcessPageScriptSourceAuthorizationError::new(
                        "private policy denied https://secret.example.test/denied.js",
                    ))
                }
                CombinedPageScriptLanguage::BlueTs(_) => {
                    Ok(AuthorizedOutOfProcessPageScriptGraph::BlueTs(
                        AuthorizedPageScriptGraph::new(
                            "https://secret.example.test/invalid.ts",
                            AuthorizedModuleLoader::new([], []).unwrap(),
                            "invalid-test-policy-v1",
                        )
                        .unwrap(),
                    ))
                }
            }
        }
    }

    struct MissingStaticEdgeAuthorizer;

    impl OutOfProcessPageScriptSourceAuthorizer for MissingStaticEdgeAuthorizer {
        fn authorize(
            &self,
            request: &OutOfProcessPageScriptSourceRequest,
        ) -> Result<
            AuthorizedOutOfProcessPageScriptGraph,
            OutOfProcessPageScriptSourceAuthorizationError,
        > {
            if request.language
                != CombinedPageScriptLanguage::JavaScript(BlueJsPageScriptKind::Module)
            {
                return Err(OutOfProcessPageScriptSourceAuthorizationError::new(
                    "unexpected private test authorization request",
                ));
            }
            Ok(AuthorizedOutOfProcessPageScriptGraph::JavaScript(
                AuthorizedJavaScriptModuleGraph::new(
                    "https://cdn.example.test/assets/missing-edge.js",
                    [AuthorizedJavaScriptModule::new(
                        "https://cdn.example.test/assets/missing-edge.js",
                        "import './not-authorized.js';",
                    )
                    .unwrap()],
                    [],
                    "core-no-fallback-policy-v1",
                )
                .unwrap(),
            ))
        }
    }

    struct NavigationAuthorizer {
        requests: Arc<Mutex<Vec<OutOfProcessPageScriptSourceRequest>>>,
    }

    impl OutOfProcessPageScriptSourceAuthorizer for NavigationAuthorizer {
        fn authorize(
            &self,
            request: &OutOfProcessPageScriptSourceRequest,
        ) -> Result<
            AuthorizedOutOfProcessPageScriptGraph,
            OutOfProcessPageScriptSourceAuthorizationError,
        > {
            self.requests.lock().unwrap().push(request.clone());
            let (document_url, entry, source, fingerprint) = match request.document_generation {
                1 => (
                    "https://first.example.test/one",
                    "https://cdn.example.test/assets/first.js",
                    "globalThis.firstExternalGeneration = 1;",
                    "core-navigation-policy-one-v1",
                ),
                2 => (
                    "https://second.example.test/two",
                    "https://cdn.example.test/assets/second.js",
                    concat!(
                        "if (typeof globalThis.firstExternalGeneration !== 'undefined') ",
                        "throw new Error('stale realm');",
                        "globalThis.secondExternalGeneration = 2;"
                    ),
                    "core-navigation-policy-two-v1",
                ),
                _ => {
                    return Err(OutOfProcessPageScriptSourceAuthorizationError::new(
                        "unexpected private test document generation",
                    ));
                }
            };
            if request.ordinal != 0
                || request.language
                    != CombinedPageScriptLanguage::JavaScript(BlueJsPageScriptKind::Classic)
                || request.document_url != document_url
                || request.declared_src != "/assets/navigation.js"
            {
                return Err(OutOfProcessPageScriptSourceAuthorizationError::new(
                    "unexpected private test authorization request",
                ));
            }
            Ok(AuthorizedOutOfProcessPageScriptGraph::JavaScript(
                AuthorizedJavaScriptModuleGraph::new(
                    entry,
                    [AuthorizedJavaScriptModule::new(entry, source).unwrap()],
                    Vec::<AuthorizedJavaScriptResolution>::new(),
                    fingerprint,
                )
                .unwrap(),
            ))
        }
    }

    #[derive(Default)]
    struct RecordingChild {
        documents: Vec<PageHostDocument>,
        closes: Vec<(u64, u64)>,
        child_stats_reply: Option<PageHostReply>,
    }

    impl PageHostClient for RecordingChild {
        fn synchronize_document(
            &mut self,
            document: PageHostDocument,
        ) -> io::Result<PageHostReply> {
            let reply = PageHostReply::Synchronized {
                tab_id: document.tab_id,
                document_generation: document.document_generation,
                already_current: false,
                reports: Vec::new(),
            };
            self.documents.push(document);
            Ok(reply)
        }

        fn close_realm(
            &mut self,
            tab_id: u64,
            document_generation: u64,
        ) -> io::Result<PageHostReply> {
            self.closes.push((tab_id, document_generation));
            Ok(PageHostReply::RealmClosed {
                tab_id,
                document_generation,
            })
        }

        fn debugger_realm_stats(
            &mut self,
            tab_id: u64,
            document_generation: u64,
        ) -> io::Result<PageHostReply> {
            Ok(PageHostReply::RealmStats(page_host::PageHostRealmStats {
                tab_id,
                document_generation,
                program_count: 1,
                bytecode_bytes: 64,
                heap_bytes: 128,
            }))
        }

        fn child_stats(&mut self) -> io::Result<PageHostReply> {
            self.child_stats_reply.clone().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::Unsupported,
                    "child-wide accounting unavailable",
                )
            })
        }
    }

    #[test]
    fn core_accepts_only_well_formed_child_wide_usage_for_its_live_realm_count() {
        let (tabs, _tab_id) = loaded_tabs(
            "<script>let accounting = 1;</script>",
            "https://example.test/aggregate-accounting.html",
        );
        let valid = PageHostChildStats {
            realm_count: 1,
            program_count: 1,
            bytecode_bytes: 64,
            heap_bytes: 128,
        };
        let mut executor = OutOfProcessJavaScriptPageExecutor::new(RecordingChild {
            child_stats_reply: Some(PageHostReply::ChildStats(valid)),
            ..RecordingChild::default()
        });
        executor.synchronize_and_execute(&tabs).unwrap();
        assert_eq!(executor.child_stats().unwrap(), valid);
        executor.child.child_stats_reply = Some(PageHostReply::ChildStats(PageHostChildStats {
            realm_count: 2,
            ..valid
        }));
        assert_eq!(
            executor.child_stats().unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        executor.child.child_stats_reply = Some(PageHostReply::ChildStats(PageHostChildStats {
            program_count: u64::MAX,
            ..valid
        }));
        assert_eq!(
            executor.child_stats().unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        executor.child.child_stats_reply = Some(PageHostReply::RealmStats(PageHostRealmStats {
            tab_id: 1,
            document_generation: 1,
            program_count: 1,
            bytecode_bytes: 64,
            heap_bytes: 128,
        }));
        assert_eq!(
            executor.child_stats().unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn core_caches_child_realm_accounting_only_for_the_live_generation() {
        let (mut tabs, tab_id) = loaded_tabs(
            "<script>let accounting = 1;</script>",
            "https://example.test/accounting-first.html",
        );
        let mut executor = OutOfProcessJavaScriptPageExecutor::new(RecordingChild::default());
        executor.synchronize_and_execute(&tabs).unwrap();
        assert_eq!(
            executor.realm_stats(tab_id),
            Some(&PageHostRealmStats {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                program_count: 1,
                bytecode_bytes: 64,
                heap_bytes: 128,
            })
        );

        tabs.get_mut(tab_id).unwrap().load_html_str(
            "<script>let accounting = 2;</script>",
            Some("https://example.test/accounting-successor.html".to_string()),
        );
        executor.synchronize_and_execute(&tabs).unwrap();
        assert_eq!(
            executor
                .realm_stats(tab_id)
                .map(|stats| stats.document_generation),
            Some(2),
            "a replacement must not retain the predecessor accounting record"
        );

        assert!(tabs.close_tab(tab_id));
        executor.synchronize_and_execute(&tabs).unwrap();
        assert_eq!(executor.realm_stats(tab_id), None);
        assert_eq!(executor.into_child().closes, vec![(tab_id.as_u64(), 2)]);
    }

    #[derive(Clone, Copy, Default)]
    enum InvalidRealmStats {
        #[default]
        MismatchedTuple,
        ExcessPrograms,
        SaturatedBytecode,
        SaturatedHeap,
    }

    #[derive(Default)]
    struct InvalidStatsChild {
        closes: Vec<(u64, u64)>,
        invalid_stats: InvalidRealmStats,
    }

    impl PageHostClient for InvalidStatsChild {
        fn synchronize_document(
            &mut self,
            document: PageHostDocument,
        ) -> io::Result<PageHostReply> {
            Ok(PageHostReply::Synchronized {
                tab_id: document.tab_id,
                document_generation: document.document_generation,
                already_current: false,
                reports: Vec::new(),
            })
        }

        fn close_realm(
            &mut self,
            tab_id: u64,
            document_generation: u64,
        ) -> io::Result<PageHostReply> {
            self.closes.push((tab_id, document_generation));
            Ok(PageHostReply::RealmClosed {
                tab_id,
                document_generation,
            })
        }

        fn debugger_realm_stats(
            &mut self,
            tab_id: u64,
            document_generation: u64,
        ) -> io::Result<PageHostReply> {
            let stats = match self.invalid_stats {
                InvalidRealmStats::MismatchedTuple => page_host::PageHostRealmStats {
                    tab_id: tab_id.saturating_add(1),
                    document_generation,
                    program_count: 1,
                    bytecode_bytes: 64,
                    heap_bytes: 128,
                },
                InvalidRealmStats::ExcessPrograms => page_host::PageHostRealmStats {
                    tab_id,
                    document_generation,
                    program_count: page_host::PAGE_HOST_REALM_STATS_MAX_PROGRAMS + 1,
                    bytecode_bytes: 64,
                    heap_bytes: 128,
                },
                InvalidRealmStats::SaturatedBytecode => page_host::PageHostRealmStats {
                    tab_id,
                    document_generation,
                    program_count: 1,
                    bytecode_bytes: u64::MAX,
                    heap_bytes: 128,
                },
                InvalidRealmStats::SaturatedHeap => page_host::PageHostRealmStats {
                    tab_id,
                    document_generation,
                    program_count: 1,
                    bytecode_bytes: 64,
                    heap_bytes: u64::MAX,
                },
            };
            Ok(PageHostReply::RealmStats(stats))
        }
    }

    #[test]
    fn malformed_child_realm_accounting_is_never_cached() {
        let (tabs, tab_id) = loaded_tabs(
            "<script>let untrustedAccounting = 1;</script>",
            "https://example.test/mismatched-accounting.html",
        );
        for invalid_stats in [
            InvalidRealmStats::MismatchedTuple,
            InvalidRealmStats::ExcessPrograms,
            InvalidRealmStats::SaturatedBytecode,
            InvalidRealmStats::SaturatedHeap,
        ] {
            let mut executor = OutOfProcessJavaScriptPageExecutor::new(InvalidStatsChild {
                invalid_stats,
                ..InvalidStatsChild::default()
            });
            executor.synchronize_and_execute(&tabs).unwrap();

            assert_eq!(executor.realm_stats(tab_id), None);
            assert!(
                !executor.debugger_has_live_realm(tab_id, 1),
                "malformed accounting must not be accepted as debugger liveness"
            );
            assert_eq!(
                executor.into_child().closes,
                vec![(tab_id.as_u64(), 1)],
                "an untrustworthy accounting record must close the newly acknowledged realm"
            );
        }
    }

    #[derive(Default)]
    struct WrongSuccessorAckChild {
        active_generation: Option<u64>,
        closes: Vec<(u64, u64)>,
        return_error: bool,
    }

    impl PageHostClient for WrongSuccessorAckChild {
        fn synchronize_document(
            &mut self,
            document: PageHostDocument,
        ) -> io::Result<PageHostReply> {
            self.active_generation = Some(document.document_generation);
            if document.document_generation == 2 && self.return_error {
                return Ok(PageHostReply::Error {
                    code: PageHostErrorCode::HostFailure,
                    message: "child failed after admission".to_string(),
                });
            }
            Ok(PageHostReply::Synchronized {
                tab_id: document.tab_id + u64::from(document.document_generation == 2),
                document_generation: document.document_generation,
                already_current: false,
                reports: Vec::new(),
            })
        }

        fn close_realm(
            &mut self,
            tab_id: u64,
            document_generation: u64,
        ) -> io::Result<PageHostReply> {
            self.closes.push((tab_id, document_generation));
            if self.active_generation == Some(document_generation) {
                self.active_generation = None;
                Ok(PageHostReply::RealmClosed {
                    tab_id,
                    document_generation,
                })
            } else {
                Ok(PageHostReply::Error {
                    code: PageHostErrorCode::StaleDocument,
                    message: "stale document".to_string(),
                })
            }
        }

        fn debugger_realm_stats(
            &mut self,
            tab_id: u64,
            document_generation: u64,
        ) -> io::Result<PageHostReply> {
            if self.active_generation != Some(document_generation) {
                return Ok(PageHostReply::Error {
                    code: PageHostErrorCode::StaleDocument,
                    message: "stale document".to_string(),
                });
            }
            Ok(PageHostReply::RealmStats(PageHostRealmStats {
                tab_id,
                document_generation,
                program_count: 1,
                bytecode_bytes: 64,
                heap_bytes: 128,
            }))
        }

        fn child_stats(&mut self) -> io::Result<PageHostReply> {
            let has_realm = self.active_generation.is_some();
            Ok(PageHostReply::ChildStats(PageHostChildStats {
                realm_count: u32::from(has_realm),
                program_count: u64::from(has_realm),
                bytecode_bytes: if has_realm { 64 } else { 0 },
                heap_bytes: if has_realm { 128 } else { 0 },
            }))
        }
    }

    #[test]
    fn untrusted_successor_ack_closes_the_generation_the_child_may_have_admitted() {
        for return_error in [false, true] {
            let (mut tabs, tab_id) = loaded_tabs(
                "<script>let previous = 1;</script>",
                "https://example.test/previous.html",
            );
            let mut executor = OutOfProcessJavaScriptPageExecutor::new(WrongSuccessorAckChild {
                return_error,
                ..WrongSuccessorAckChild::default()
            });
            executor.synchronize_and_execute(&tabs).unwrap();
            assert_eq!(executor.child.active_generation, Some(1));
            assert_eq!(executor.child_stats().unwrap().realm_count, 1);

            tabs.get_mut(tab_id).unwrap().load_html_str(
                "<script>let successor = 2;</script>",
                Some("https://example.test/successor.html".to_string()),
            );
            executor.synchronize_and_execute(&tabs).unwrap();
            assert_eq!(executor.child_stats().unwrap().realm_count, 0);
            let child = executor.into_child();
            assert_eq!(child.active_generation, None);
            assert_eq!(
                child.closes,
                vec![(tab_id.as_u64(), 1), (tab_id.as_u64(), 2)]
            );
        }
    }

    /// Records only the lifecycle advance requests needed to prove that a
    /// debugger discovery peer receives a finite grace budget rather than an
    /// execution lease. It deliberately implements no debugger inspection
    /// operation, so no VM/source/bytecode surface is added by this test.
    #[derive(Default)]
    struct DeferralBudgetChild {
        advances: Vec<(u64, u64)>,
    }

    impl PageHostClient for DeferralBudgetChild {
        fn synchronize_document(
            &mut self,
            document: PageHostDocument,
        ) -> io::Result<PageHostReply> {
            Ok(PageHostReply::Synchronized {
                tab_id: document.tab_id,
                document_generation: document.document_generation,
                already_current: false,
                reports: Vec::new(),
            })
        }

        fn close_realm(
            &mut self,
            tab_id: u64,
            document_generation: u64,
        ) -> io::Result<PageHostReply> {
            Ok(PageHostReply::RealmClosed {
                tab_id,
                document_generation,
            })
        }

        fn debugger_execution_control_available(&self) -> bool {
            true
        }

        fn advance_debugger_execution(
            &mut self,
            tab_id: u64,
            document_generation: u64,
        ) -> io::Result<PageHostReply> {
            self.advances.push((tab_id, document_generation));
            Ok(PageHostReply::DebuggerExecutionAdvanced {
                tab_id,
                document_generation,
                reports: Vec::new(),
            })
        }
    }

    #[test]
    fn oop_debugger_discovery_deferrals_are_finite_per_document() {
        let (tabs, tab_id) = loaded_tabs(
            "<script>let pendingDebuggerAdmission = true;</script>",
            "https://example.test/pending-debugger.html",
        );
        let mut executor = OutOfProcessJavaScriptPageExecutor::new_with_debugger_execution_control(
            DeferralBudgetChild::default(),
        );
        executor.synchronize_and_execute(&tabs).unwrap();

        // One hold is accepted for each of the fixed number of session turns,
        // matching a debugger peer that asks one discovery/configuration
        // question per core tick. The next request cannot keep the document
        // pending; core advances it through the child lifecycle instead.
        for _ in 0..MAX_OOP_DEBUGGER_EXECUTION_DEFERRALS_PER_DOCUMENT {
            executor.hold_pending_debugger_execution_once();
            executor.synchronize_and_execute(&tabs).unwrap();
        }
        assert!(executor.child.advances.is_empty());

        executor.hold_pending_debugger_execution_once();
        executor.synchronize_and_execute(&tabs).unwrap();
        assert_eq!(executor.child.advances, vec![(tab_id.as_u64(), 1)]);
    }

    #[test]
    fn immutable_core_authorizer_routes_external_javascript_and_bluets_graphs_to_real_child() {
        let (path, token, child) = spawn_child();
        let (tabs, tab_id) = loaded_tabs(
            concat!(
                "<script src=\"/assets/external.js\"></script>",
                "<script type=\"application/x-blueice-typescript-module\" src=\"/assets/external.ts\"></script>",
                "<script>if (globalThis.externalJavaScript !== 41) throw new Error('order');</script>"
            ),
            "https://example.test/app/index.html",
        );
        let requests = Arc::new(Mutex::new(Vec::new()));
        let mut executor =
            OutOfProcessJavaScriptPageExecutor::connect_with_external_source_authorizer(
                &path,
                &token,
                ExternalGraphsAuthorizer {
                    requests: Arc::clone(&requests),
                },
            )
            .unwrap();

        executor.synchronize_and_execute(&tabs).unwrap();

        assert_eq!(
            executor.drain_reports_for_tab(tab_id),
            vec![
                JavaScriptPageExecutionReport::Executed {
                    tab_id: tab_id.as_u64(),
                    document_generation: 1,
                    ordinal: 0,
                    kind: BlueJsPageScriptKind::Classic,
                },
                JavaScriptPageExecutionReport::Executed {
                    tab_id: tab_id.as_u64(),
                    document_generation: 1,
                    ordinal: 2,
                    kind: BlueJsPageScriptKind::Classic,
                },
            ]
        );
        assert_eq!(
            executor.drain_blue_ts_reports_for_tab(tab_id),
            vec![BlueTsPageExecutionReport::Executed {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 1,
                kind: DirectPageScriptKind::Module,
            }]
        );
        assert_eq!(
            requests.lock().unwrap().as_slice(),
            [
                OutOfProcessPageScriptSourceRequest {
                    tab_id,
                    document_generation: 1,
                    ordinal: 0,
                    language: CombinedPageScriptLanguage::JavaScript(BlueJsPageScriptKind::Classic,),
                    document_url: "https://example.test/app/index.html".to_string(),
                    declared_src: "/assets/external.js".to_string(),
                },
                OutOfProcessPageScriptSourceRequest {
                    tab_id,
                    document_generation: 1,
                    ordinal: 1,
                    language: CombinedPageScriptLanguage::BlueTs(DirectPageScriptKind::Module),
                    document_url: "https://example.test/app/index.html".to_string(),
                    declared_src: "/assets/external.ts".to_string(),
                },
            ]
        );
        drop(executor);
        shutdown_child(&path, &token);
        child.join().unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn startup_http_authorizer_builds_closed_classic_module_and_bluets_graphs_source_free() {
        let classic = "globalThis.externalClassic = (globalThis.externalClassic || 40) + 1;";
        let javascript_entry =
            "import { answer } from './answer.js'; globalThis.externalModule = answer;";
        let javascript_dependency = "export const answer = 42;";
        let bluets_entry =
            "import { answer } from './answer.ts'; export const result: number = answer;";
        let bluets_dependency = "export const answer: number = 41;";
        let integrity_mismatch = "globalThis.redactedIntegrityMarker = 'private bytes';";
        let bad_mime = "globalThis.redactedMimeMarker = 'private bytes';";
        let mut responses = BTreeMap::new();
        responses.insert(
            "/assets/classic.js".to_string(),
            HttpTestResponse::script("text/javascript; charset=utf-8", classic),
        );
        responses.insert(
            "/assets/main.js".to_string(),
            HttpTestResponse::script("application/javascript", javascript_entry),
        );
        responses.insert(
            "/assets/answer.js".to_string(),
            HttpTestResponse::script("application/javascript", javascript_dependency),
        );
        responses.insert(
            "/assets/main.ts".to_string(),
            HttpTestResponse::script("text/typescript", bluets_entry),
        );
        responses.insert(
            "/assets/answer.ts".to_string(),
            HttpTestResponse::script("application/typescript", bluets_dependency),
        );
        responses.insert(
            "/assets/bad-integrity.js".to_string(),
            HttpTestResponse::script("application/javascript", integrity_mismatch),
        );
        responses.insert(
            "/assets/bad-mime.js".to_string(),
            HttpTestResponse::script("text/plain", bad_mime),
        );
        responses.insert(
            "/assets/redirect.js".to_string(),
            HttpTestResponse::redirect("/assets/classic.js"),
        );
        // Eight requests: the repeated classic declaration uses the private
        // `(URL, integrity, MIME lane)` cache key, and the cross-origin URL
        // is rejected before any network I/O.
        let (origin, requested, server) = spawn_local_resource_server(responses, 8);
        let resource = |path: &str| format!("{origin}{path}");
        let manifest = HttpScriptIntegrityManifest::new([
            (
                resource("/assets/classic.js"),
                sha256_integrity(classic.as_bytes()),
            ),
            (
                resource("/assets/main.js"),
                sha256_integrity(javascript_entry.as_bytes()),
            ),
            (
                resource("/assets/answer.js"),
                sha256_integrity(javascript_dependency.as_bytes()),
            ),
            (
                resource("/assets/main.ts"),
                sha256_integrity(bluets_entry.as_bytes()),
            ),
            (
                resource("/assets/answer.ts"),
                sha256_integrity(bluets_dependency.as_bytes()),
            ),
            (
                resource("/assets/bad-integrity.js"),
                sha256_integrity(b"owner-selected different bytes"),
            ),
            (
                resource("/assets/bad-mime.js"),
                sha256_integrity(bad_mime.as_bytes()),
            ),
            (
                resource("/assets/redirect.js"),
                sha256_integrity(b"redirect body is not admitted"),
            ),
            (
                "http://127.0.0.1:1/cross-origin.js".to_string(),
                sha256_integrity(b"not fetched"),
            ),
        ])
        .unwrap();
        let policy = HttpScriptResourcePolicy::new(
            HttpScriptResourceOriginRule::same_document_origin(),
            manifest,
            HttpScriptResourceLimits::default(),
        )
        .unwrap();
        let authorizer = HttpOutOfProcessPageScriptSourceAuthorizer::new(policy);
        let (path, token, child) = spawn_child();
        let (tabs, tab_id) = loaded_tabs(
            concat!(
                "<script src=\"/assets/classic.js\"></script>",
                "<script type=\"module\" src=\"/assets/main.js\"></script>",
                "<script type=\"application/x-blueice-typescript-module\" src=\"/assets/main.ts\"></script>",
                "<script src=\"/assets/classic.js\"></script>",
                "<script src=\"/assets/bad-integrity.js\"></script>",
                "<script type=\"module\" src=\"/assets/bad-mime.js\"></script>",
                "<script src=\"/assets/redirect.js\"></script>",
                "<script src=\"http://127.0.0.1:1/cross-origin.js\"></script>",
                "<script>if (globalThis.externalClassic !== 42 || globalThis.externalModule !== 42) throw 'graph';</script>"
            ),
            &format!("{origin}/app/index.html"),
        );
        let mut executor =
            OutOfProcessJavaScriptPageExecutor::connect_with_external_source_authorizer(
                &path, &token, authorizer,
            )
            .unwrap();

        executor.synchronize_and_execute(&tabs).unwrap();

        let reports = executor.drain_reports_for_tab(tab_id);
        assert_eq!(
            reports,
            vec![
                JavaScriptPageExecutionReport::Executed {
                    tab_id: tab_id.as_u64(),
                    document_generation: 1,
                    ordinal: 0,
                    kind: BlueJsPageScriptKind::Classic,
                },
                JavaScriptPageExecutionReport::Executed {
                    tab_id: tab_id.as_u64(),
                    document_generation: 1,
                    ordinal: 1,
                    kind: BlueJsPageScriptKind::Module,
                },
                JavaScriptPageExecutionReport::Executed {
                    tab_id: tab_id.as_u64(),
                    document_generation: 1,
                    ordinal: 3,
                    kind: BlueJsPageScriptKind::Classic,
                },
                JavaScriptPageExecutionReport::Rejected {
                    tab_id: tab_id.as_u64(),
                    document_generation: 1,
                    ordinal: 4,
                    kind: BlueJsPageScriptKind::Classic,
                    category: "external JavaScript source authorization rejected the page script",
                },
                JavaScriptPageExecutionReport::Rejected {
                    tab_id: tab_id.as_u64(),
                    document_generation: 1,
                    ordinal: 5,
                    kind: BlueJsPageScriptKind::Module,
                    category: "external JavaScript source authorization rejected the page script",
                },
                JavaScriptPageExecutionReport::Rejected {
                    tab_id: tab_id.as_u64(),
                    document_generation: 1,
                    ordinal: 6,
                    kind: BlueJsPageScriptKind::Classic,
                    category: "external JavaScript source authorization rejected the page script",
                },
                JavaScriptPageExecutionReport::Rejected {
                    tab_id: tab_id.as_u64(),
                    document_generation: 1,
                    ordinal: 7,
                    kind: BlueJsPageScriptKind::Classic,
                    category: "external JavaScript source authorization rejected the page script",
                },
                JavaScriptPageExecutionReport::Executed {
                    tab_id: tab_id.as_u64(),
                    document_generation: 1,
                    ordinal: 8,
                    kind: BlueJsPageScriptKind::Classic,
                },
            ]
        );
        assert_eq!(
            executor.drain_blue_ts_reports_for_tab(tab_id),
            vec![BlueTsPageExecutionReport::Executed {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 2,
                kind: DirectPageScriptKind::Module,
            }]
        );
        let source_free = format!("{reports:?}");
        for protected in [
            "redactedIntegrityMarker",
            "redactedMimeMarker",
            "bad-integrity.js",
            "bad-mime.js",
            "redirect.js",
            "cross-origin.js",
        ] {
            assert!(!source_free.contains(protected));
        }
        drop(executor);
        shutdown_child(&path, &token);
        child.join().unwrap();
        let _ = std::fs::remove_file(path);
        server.join().unwrap();
        let requested = requested.lock().unwrap();
        assert_eq!(requested.len(), 8);
        assert_eq!(
            requested
                .iter()
                .filter(|path| path.as_str() == "/assets/classic.js")
                .count(),
            1
        );
        assert!(!requested.iter().any(|path| path.contains("cross-origin")));
    }

    #[test]
    fn external_authorizer_denial_and_invalid_graph_are_source_free_in_real_child_route() {
        let (path, token, child) = spawn_child();
        let (tabs, tab_id) = loaded_tabs(
            concat!(
                "<script src=\"https://secret.example.test/denied.js\"></script>",
                "<script type=\"application/x-blueice-typescript\" src=\"https://secret.example.test/invalid.ts\"></script>"
            ),
            "https://example.test/app/index.html",
        );
        let mut executor =
            OutOfProcessJavaScriptPageExecutor::connect_with_external_source_authorizer(
                &path,
                &token,
                DeniedAndInvalidGraphsAuthorizer,
            )
            .unwrap();

        executor.synchronize_and_execute(&tabs).unwrap();

        let reports = executor.drain_reports_for_tab(tab_id);
        let blue_ts_reports = executor.drain_blue_ts_reports_for_tab(tab_id);
        assert!(matches!(
            reports.as_slice(),
            [JavaScriptPageExecutionReport::Rejected {
                category: "external JavaScript source authorization rejected the page script",
                ..
            }]
        ));
        assert!(matches!(
            blue_ts_reports.as_slice(),
            [BlueTsPageExecutionReport::Rejected {
                category: "external BlueTS source authorization rejected the page script",
                ..
            }]
        ));
        let observed = format!("{reports:?}{blue_ts_reports:?}");
        assert!(!observed.contains("secret.example.test"));
        assert!(!observed.contains("invalid.ts"));
        assert!(!observed.contains("private policy"));
        drop(executor);
        shutdown_child(&path, &token);
        child.join().unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn missing_authorized_static_edge_has_no_child_resolver_fallback() {
        let (path, token, child) = spawn_child();
        let (tabs, tab_id) = loaded_tabs(
            concat!(
                "<script type=\"module\" src=\"/assets/missing-edge.js\"></script>",
                "<script>globalThis.afterMissingEdge = true;</script>"
            ),
            "https://example.test/app/index.html",
        );
        let mut executor =
            OutOfProcessJavaScriptPageExecutor::connect_with_external_source_authorizer(
                &path,
                &token,
                MissingStaticEdgeAuthorizer,
            )
            .unwrap();

        executor.synchronize_and_execute(&tabs).unwrap();

        assert_eq!(
            executor.drain_reports_for_tab(tab_id),
            vec![
                JavaScriptPageExecutionReport::Rejected {
                    tab_id: tab_id.as_u64(),
                    document_generation: 1,
                    ordinal: 0,
                    kind: BlueJsPageScriptKind::Module,
                    category: "authorized JavaScript graph is missing a static resolution",
                },
                JavaScriptPageExecutionReport::Executed {
                    tab_id: tab_id.as_u64(),
                    document_generation: 1,
                    ordinal: 1,
                    kind: BlueJsPageScriptKind::Classic,
                },
            ]
        );
        drop(executor);
        shutdown_child(&path, &token);
        child.join().unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn external_authorization_is_rechecked_and_old_child_realm_is_invalidated_on_navigation() {
        let (path, token, child) = spawn_child();
        let (mut tabs, tab_id) = loaded_tabs(
            "<script src=\"/assets/navigation.js\"></script>",
            "https://first.example.test/one",
        );
        let requests = Arc::new(Mutex::new(Vec::new()));
        let mut executor =
            OutOfProcessJavaScriptPageExecutor::connect_with_external_source_authorizer(
                &path,
                &token,
                NavigationAuthorizer {
                    requests: Arc::clone(&requests),
                },
            )
            .unwrap();

        executor.synchronize_and_execute(&tabs).unwrap();
        assert!(matches!(
            executor.drain_reports_for_tab(tab_id).as_slice(),
            [JavaScriptPageExecutionReport::Executed {
                document_generation: 1,
                ordinal: 0,
                ..
            }]
        ));
        tabs.get_mut(tab_id).unwrap().load_html_str(
            "<script src=\"/assets/navigation.js\"></script>",
            Some("https://second.example.test/two".to_string()),
        );
        executor.synchronize_and_execute(&tabs).unwrap();
        assert!(matches!(
            executor.drain_reports_for_tab(tab_id).as_slice(),
            [JavaScriptPageExecutionReport::Executed {
                document_generation: 2,
                ordinal: 0,
                ..
            }]
        ));
        assert_eq!(
            requests
                .lock()
                .unwrap()
                .iter()
                .map(|request| (
                    request.document_generation,
                    request.ordinal,
                    request.document_url.as_str(),
                    request.declared_src.as_str(),
                ))
                .collect::<Vec<_>>(),
            vec![
                (
                    1,
                    0,
                    "https://first.example.test/one",
                    "/assets/navigation.js"
                ),
                (
                    2,
                    0,
                    "https://second.example.test/two",
                    "/assets/navigation.js"
                ),
            ]
        );
        drop(executor);
        shutdown_child(&path, &token);
        child.join().unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn core_routes_explicit_inline_documents_to_the_real_child_and_rejects_external_src() {
        let (path, token, child) = spawn_child();
        let (tabs, tab_id) = loaded_tabs(
            concat!(
                "<script>globalThis.answer = 42;</script>",
                "<script type=\"module\">export const moduleAnswer = 43;</script>",
                "<script src=\"untrusted.js\"></script>"
            ),
            "https://example.test/app/index.html",
        );
        let mut executor = OutOfProcessJavaScriptPageExecutor::connect(&path, &token).unwrap();
        executor.synchronize_and_execute(&tabs).unwrap();
        let stats = executor
            .realm_stats(tab_id)
            .expect("the real child must supply core-owned realm accounting");
        assert_eq!(stats.tab_id, tab_id.as_u64());
        assert_eq!(stats.document_generation, 1);
        assert_eq!(stats.program_count, 2);
        assert!(
            stats.bytecode_bytes > 0,
            "the aggregate record must charge the admitted classic and module programs"
        );
        assert_eq!(
            executor.drain_reports_for_tab(tab_id),
            vec![
                JavaScriptPageExecutionReport::Executed {
                    tab_id: tab_id.as_u64(),
                    document_generation: 1,
                    ordinal: 0,
                    kind: BlueJsPageScriptKind::Classic,
                },
                JavaScriptPageExecutionReport::Executed {
                    tab_id: tab_id.as_u64(),
                    document_generation: 1,
                    ordinal: 1,
                    kind: BlueJsPageScriptKind::Module,
                },
                JavaScriptPageExecutionReport::Rejected {
                    tab_id: tab_id.as_u64(),
                    document_generation: 1,
                    ordinal: 2,
                    kind: BlueJsPageScriptKind::Classic,
                    category: "external JavaScript declarations require an authorized loader",
                },
            ]
        );
        drop(executor);
        shutdown_child(&path, &token);
        child.join().unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn core_proxies_real_child_debugger_locations_with_core_ids_and_rejects_stale_cross_tab_targets(
    ) {
        use crate::debugger::handle_debugger_request_with_page_javascript_executor;
        use blueice_ipc::debugger::{
            DebuggerCapability, DebuggerCapabilityState, DebuggerErrorCode, DebuggerPageRealm,
            DebuggerReply, DebuggerRequest,
        };

        let (path, token, child) = spawn_child();
        let (mut tabs, first_tab) = loaded_tabs(
            "<script>let first = 1; first += 1;</script>",
            "https://example.test/first.html",
        );
        let second_tab = tabs.open_tab();
        tabs.get_mut(second_tab).unwrap().load_html_str(
            "<script>let second = 2;</script>",
            Some("https://example.test/second.html".to_string()),
        );
        let mut executor = OutOfProcessJavaScriptPageExecutor::connect(&path, &token).unwrap();
        executor.synchronize_and_execute(&tabs).unwrap();

        let first_realm = DebuggerPageRealm {
            browser_context_id: crate::debugger::DEFAULT_BROWSER_CONTEXT_ID,
            tab_id: first_tab.as_u64(),
            realm_generation: 1,
        };
        let second_realm = DebuggerPageRealm {
            browser_context_id: crate::debugger::DEFAULT_BROWSER_CONTEXT_ID,
            tab_id: second_tab.as_u64(),
            realm_generation: 1,
        };
        let capabilities = handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::DescribeCapabilities { realm: first_realm },
        );
        let DebuggerReply::Capabilities(capabilities) = capabilities else {
            panic!("expected child debugger capabilities");
        };
        assert_eq!(
            capabilities
                .reports
                .iter()
                .find(|report| report.capability == DebuggerCapability::ProgramLocations)
                .map(|report| report.state),
            Some(DebuggerCapabilityState::Available)
        );
        assert_eq!(
            capabilities
                .reports
                .iter()
                .find(|report| report.capability == DebuggerCapability::BreakpointConfiguration)
                .map(|report| report.state),
            Some(DebuggerCapabilityState::Available)
        );
        assert!(
            capabilities
                .reports
                .iter()
                .all(|report| !report.detail.contains("first")
                    && !report.detail.contains("bytecode")),
            "capability replies must remain source/bytecode-free"
        );

        let programs = match handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ListPrograms { realm: first_realm },
        ) {
            DebuggerReply::Programs(programs) => programs,
            reply => panic!("expected public program inventory, got {reply:?}"),
        };
        let program = *programs.first().expect("first child page has one program");
        assert!(
            program.program_handle >= CORE_CHILD_DEBUGGER_ID_NAMESPACE_START
                && program.program_generation >= CORE_CHILD_DEBUGGER_ID_NAMESPACE_START,
            "core must mint a public namespace instead of forwarding child IDs"
        );
        let safe_points = match handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ListSafePoints { program },
        ) {
            DebuggerReply::SafePoints(safe_points) => safe_points,
            reply => panic!("expected source-free public safe points, got {reply:?}"),
        };
        let safe_point = *safe_points.first().expect("program exposes one safe point");
        assert_eq!(
            handle_debugger_request_with_page_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::ValidateSafePoint { safe_point },
            ),
            DebuggerReply::SafePointValidated { safe_point }
        );
        assert_eq!(
            handle_debugger_request_with_page_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::SetBreakpoint { safe_point },
            ),
            DebuggerReply::BreakpointSet { safe_point }
        );
        // Configuration is idempotent and does not create another public or
        // child-private record on a debugger socket retry.
        assert_eq!(
            handle_debugger_request_with_page_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::SetBreakpoint { safe_point },
            ),
            DebuggerReply::BreakpointSet { safe_point }
        );
        assert_eq!(
            handle_debugger_request_with_page_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::ListBreakpoints { realm: first_realm },
            ),
            DebuggerReply::Breakpoints(vec![safe_point])
        );
        assert_eq!(
            handle_debugger_request_with_page_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::ClearBreakpoint { safe_point },
            ),
            DebuggerReply::BreakpointCleared {
                safe_point,
                was_present: true,
            }
        );
        assert_eq!(
            handle_debugger_request_with_page_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::ClearBreakpoint { safe_point },
            ),
            DebuggerReply::BreakpointCleared {
                safe_point,
                was_present: false,
            }
        );
        assert_eq!(
            handle_debugger_request_with_page_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::SetBreakpoint { safe_point },
            ),
            DebuggerReply::BreakpointSet { safe_point }
        );

        let cross_tab = blueice_ipc::debugger::DebuggerProgram {
            realm: second_realm,
            ..program
        };
        assert!(matches!(
            handle_debugger_request_with_page_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::ListSafePoints { program: cross_tab },
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidTarget,
                ..
            }
        ));
        let cross_tab_safe_point = blueice_ipc::debugger::DebuggerSafePoint {
            program: cross_tab,
            ..safe_point
        };
        assert!(matches!(
            handle_debugger_request_with_page_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::SetBreakpoint {
                    safe_point: cross_tab_safe_point,
                },
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidTarget,
                ..
            }
        ));

        // Reconfiguration before navigation proves that the successor's
        // empty child table cannot retain prior private IDs or public tuples.
        tabs.get_mut(first_tab).unwrap().load_html_str(
            "<script>let successor = 4;</script>",
            Some("https://example.test/successor-again.html".to_string()),
        );
        executor.synchronize_and_execute(&tabs).unwrap();
        let successor_realm = DebuggerPageRealm {
            realm_generation: 2,
            ..first_realm
        };
        assert_eq!(
            handle_debugger_request_with_page_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::ListBreakpoints {
                    realm: successor_realm,
                },
            ),
            DebuggerReply::Breakpoints(Vec::new())
        );

        tabs.get_mut(first_tab).unwrap().load_html_str(
            "<script>let successor = 3;</script>",
            Some("https://example.test/successor.html".to_string()),
        );
        assert!(matches!(
            handle_debugger_request_with_page_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::ListSafePoints { program },
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::StaleRealm,
                ..
            }
        ));

        drop(executor);
        shutdown_child(&path, &token);
        child.join().unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn core_remints_real_child_bluets_metadata_handles_and_discards_them_on_navigation() {
        let (path, token, child) = spawn_child();
        let (mut tabs, tab_id) = loaded_tabs(
            concat!(
                "<script>globalThis.javaScriptOnly = true;</script>",
                "<script type=\"application/x-blueice-typescript\">",
                "const opaqueCompilerMetadata: number = 42;",
                "</script>"
            ),
            "https://example.test/opaque-metadata.html",
        );
        let mut executor = OutOfProcessJavaScriptPageExecutor::connect(&path, &token).unwrap();
        executor.synchronize_and_execute(&tabs).unwrap();

        let programs = executor.debugger_programs(tab_id, 1).unwrap();
        assert_eq!(programs.len(), 2);
        let mut metadata = None;
        for program in programs {
            let handles = executor
                .debugger_static_metadata(
                    tab_id,
                    1,
                    program.program_handle,
                    program.program_generation,
                )
                .unwrap();
            if let [handle] = handles.as_slice() {
                assert!(
                    handle.metadata_handle >= CORE_CHILD_DEBUGGER_METADATA_ID_NAMESPACE_START
                        && handle.metadata_handle < CORE_CHILD_DEBUGGER_ID_NAMESPACE_START
                        && handle.metadata_generation
                            >= CORE_CHILD_DEBUGGER_METADATA_ID_NAMESPACE_START
                        && handle.metadata_generation < CORE_CHILD_DEBUGGER_ID_NAMESPACE_START,
                    "core must remint metadata IDs outside both child and public program namespaces"
                );
                metadata = Some((program, *handle));
            } else {
                assert!(handles.is_empty(), "the JavaScript program is ineligible");
            }
        }
        let (typed_program, metadata) = metadata.expect("direct BlueTS has one private attachment");
        assert!(
            !format!("{metadata:?}").contains("opaqueCompilerMetadata"),
            "the core-facing handle must contain no compiler metadata payload"
        );
        let summary = executor
            .debugger_static_metadata_summary(
                tab_id,
                1,
                typed_program.program_handle,
                typed_program.program_generation,
                metadata.metadata_handle,
                metadata.metadata_generation,
            )
            .expect("the exact core-reminted metadata identity resolves a bounded summary");
        assert_eq!(summary.language_version, "blue-ts-0.1");
        assert!(summary.source_count > 0);
        assert!(summary.type_count > 0);
        assert!(summary.symbol_count > 0);
        assert!(
            !format!("{summary:?}").contains("opaqueCompilerMetadata"),
            "the core-facing summary must contain no compiler record payload"
        );
        let sources = executor
            .debugger_static_metadata_sources(
                tab_id,
                1,
                typed_program.program_handle,
                typed_program.program_generation,
                metadata.metadata_handle,
                metadata.metadata_generation,
            )
            .expect("the exact core-reminted metadata identity resolves source-record IDs");
        assert_eq!(
            sources.len(),
            usize::try_from(summary.source_count).unwrap()
        );
        assert_eq!(
            sources
                .iter()
                .map(|source| source.source_id)
                .collect::<BTreeSet<_>>()
                .len(),
            sources.len(),
            "the child must not repeat compiler source-record IDs"
        );
        assert!(
            !format!("{sources:?}").contains("opaqueCompilerMetadata"),
            "source-record identities must not carry compiler record payloads"
        );
        assert!(matches!(
            executor.debugger_static_metadata_sources(
                tab_id,
                1,
                typed_program.program_handle,
                typed_program.program_generation,
                metadata.metadata_handle,
                metadata.metadata_generation + 1,
            ),
            Err(JavaScriptPageDebuggerError::UnknownProgram)
        ));
        assert!(matches!(
            executor.debugger_static_metadata_summary(
                tab_id,
                1,
                typed_program.program_handle,
                typed_program.program_generation,
                metadata.metadata_handle,
                metadata.metadata_generation + 1,
            ),
            Err(JavaScriptPageDebuggerError::UnknownProgram)
        ));

        tabs.get_mut(tab_id).unwrap().load_html_str(
            "<script type=\"application/x-blueice-typescript\">const successor: number = 1;</script>",
            Some("https://example.test/opaque-metadata-successor.html".to_string()),
        );
        executor.synchronize_and_execute(&tabs).unwrap();
        assert!(matches!(
            executor.debugger_static_metadata(
                tab_id,
                1,
                typed_program.program_handle,
                typed_program.program_generation,
            ),
            Err(JavaScriptPageDebuggerError::NoLiveRealm)
        ));
        assert!(matches!(
            executor.debugger_static_metadata_summary(
                tab_id,
                1,
                typed_program.program_handle,
                typed_program.program_generation,
                metadata.metadata_handle,
                metadata.metadata_generation,
            ),
            Err(JavaScriptPageDebuggerError::NoLiveRealm)
        ));
        assert!(matches!(
            executor.debugger_static_metadata_sources(
                tab_id,
                1,
                typed_program.program_handle,
                typed_program.program_generation,
                metadata.metadata_handle,
                metadata.metadata_generation,
            ),
            Err(JavaScriptPageDebuggerError::NoLiveRealm)
        ));

        drop(executor);
        shutdown_child(&path, &token);
        child.join().unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn core_child_transport_resolves_only_live_exact_bluets_safe_point_spans() {
        let (path, token, child) = spawn_child();
        let (mut tabs, tab_id) = loaded_tabs(
            "<script type=\"application/x-blueice-typescript\">const mapped: number = 42;</script>",
            "https://example.test/mapped.html",
        );
        let mut executor = OutOfProcessJavaScriptPageExecutor::connect(&path, &token).unwrap();
        executor.synchronize_and_execute(&tabs).unwrap();
        let child_stats = executor.child_stats().unwrap();
        assert_eq!(child_stats.realm_count, 1);
        assert!(child_stats.is_well_formed());
        assert!(executor.child.debugger_bluets_safe_point_span_available());

        let PageHostReply::DebuggerPrograms { programs, .. } = executor
            .child
            .debugger_programs(tab_id.as_u64(), 1)
            .unwrap()
        else {
            panic!("expected a child-private BlueTS program");
        };
        let program = programs[0];
        let PageHostReply::DebuggerBlueTsMetadata { metadata, .. } = executor
            .child
            .debugger_bluets_metadata(tab_id.as_u64(), 1, program)
            .unwrap()
        else {
            panic!("expected a child-private metadata attachment");
        };
        let metadata = metadata[0];
        let PageHostReply::DebuggerSafePoints { safe_points, .. } = executor
            .child
            .debugger_safe_points(tab_id.as_u64(), 1, program)
            .unwrap()
        else {
            panic!("expected child-private safe points");
        };
        let (safe_point, span) = safe_points
            .into_iter()
            .find_map(|safe_point| {
                match executor
                    .child
                    .debugger_bluets_safe_point_span(tab_id.as_u64(), 1, metadata, safe_point)
                    .unwrap()
                {
                    PageHostReply::DebuggerBlueTsSafePointSpan {
                        safe_point: echoed,
                        span,
                        ..
                    } if echoed == safe_point => Some((safe_point, span)),
                    PageHostReply::Error {
                        code: page_host::PageHostErrorCode::InvalidRequest,
                        ..
                    } => None,
                    reply => panic!("unexpected exact-span reply: {reply:?}"),
                }
            })
            .expect("one verified safe point must have a retained BlueTS span");
        assert!(span.start_byte < span.end_byte);
        assert!(!format!("{span:?}").contains("mapped"));

        let public_program = executor.debugger_programs(tab_id, 1).unwrap()[0];
        let public_metadata = executor
            .debugger_static_metadata(
                tab_id,
                1,
                public_program.program_handle,
                public_program.program_generation,
            )
            .unwrap()[0];
        let public_sources = executor
            .debugger_static_metadata_sources(
                tab_id,
                1,
                public_program.program_handle,
                public_program.program_generation,
                public_metadata.metadata_handle,
                public_metadata.metadata_generation,
            )
            .unwrap();
        assert!(public_sources
            .iter()
            .any(|source| source.source_id == span.source_id));
        let target = JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget {
            program_handle: public_program.program_handle,
            program_generation: public_program.program_generation,
            metadata_handle: public_metadata.metadata_handle,
            metadata_generation: public_metadata.metadata_generation,
            source_id: span.source_id,
            code_unit_ordinal: safe_point.code_unit_ordinal,
            bytecode_offset: safe_point.bytecode_offset,
        };
        assert_eq!(
            executor
                .debugger_static_metadata_safe_point_span(tab_id, 1, target)
                .unwrap(),
            JavaScriptPageDebuggerStaticMetadataSafePointSpan {
                source_id: span.source_id,
                start_byte: span.start_byte,
                end_byte: span.end_byte,
                coordinates: span.coordinates,
            }
        );
        assert!(executor.debugger_static_metadata_source_breakpoint_available());
        let breakpoint_target = JavaScriptPageDebuggerStaticMetadataSourceBreakpointTarget {
            program_handle: public_program.program_handle,
            program_generation: public_program.program_generation,
            metadata_handle: public_metadata.metadata_handle,
            metadata_generation: public_metadata.metadata_generation,
            source_id: span.source_id,
            source_byte: span.start_byte,
        };
        assert_eq!(
            executor
                .debugger_static_metadata_source_breakpoint(tab_id, 1, breakpoint_target)
                .unwrap(),
            Some(JavaScriptPageDebuggerSafePoint {
                code_unit_ordinal: safe_point.code_unit_ordinal,
                bytecode_offset: safe_point.bytecode_offset,
            })
        );
        assert_eq!(
            executor
                .debugger_static_metadata_source_breakpoint(
                    tab_id,
                    1,
                    JavaScriptPageDebuggerStaticMetadataSourceBreakpointTarget {
                        source_byte: span.end_byte,
                        ..breakpoint_target
                    },
                )
                .unwrap(),
            None,
        );
        assert_eq!(
            executor.debugger_static_metadata_source_breakpoint(
                tab_id,
                1,
                JavaScriptPageDebuggerStaticMetadataSourceBreakpointTarget {
                    metadata_generation: breakpoint_target.metadata_generation + 1,
                    ..breakpoint_target
                },
            ),
            Err(JavaScriptPageDebuggerError::UnknownProgram)
        );
        assert_eq!(
            executor.debugger_static_metadata_safe_point_span(
                tab_id,
                1,
                JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget {
                    source_id: u32::MAX,
                    ..target
                },
            ),
            Err(JavaScriptPageDebuggerError::UnknownProgram)
        );
        assert_eq!(
            executor.debugger_static_metadata_safe_point_span(
                tab_id,
                1,
                JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget {
                    metadata_generation: target.metadata_generation + 1,
                    ..target
                },
            ),
            Err(JavaScriptPageDebuggerError::UnknownProgram)
        );
        assert_eq!(
            executor.debugger_static_metadata_safe_point_span(
                tab_id,
                1,
                JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget {
                    bytecode_offset: u32::MAX,
                    ..target
                },
            ),
            Err(JavaScriptPageDebuggerError::NoLiveRealm)
        );

        tabs.get_mut(tab_id).unwrap().load_html_str(
            "<script>const successor = true;</script>",
            Some("https://example.test/successor.html".to_string()),
        );
        executor.synchronize_and_execute(&tabs).unwrap();
        assert_eq!(
            executor.debugger_static_metadata_safe_point_span(tab_id, 1, target),
            Err(JavaScriptPageDebuggerError::NoLiveRealm)
        );
        assert_eq!(
            executor.debugger_static_metadata_source_breakpoint(tab_id, 1, breakpoint_target),
            Err(JavaScriptPageDebuggerError::NoLiveRealm)
        );
        assert!(matches!(
            executor
                .child
                .debugger_bluets_safe_point_span(tab_id.as_u64(), 1, metadata, safe_point)
                .unwrap(),
            PageHostReply::Error {
                code: page_host::PageHostErrorCode::StaleDocument,
                ..
            }
        ));

        drop(executor);
        shutdown_child(&path, &token);
        child.join().unwrap();
        let _ = std::fs::remove_file(path);
    }

    struct MalformedSafePointSpanChild {
        lifecycle: RecordingChild,
        reply: PageHostReply,
    }

    impl PageHostClient for MalformedSafePointSpanChild {
        fn synchronize_document(
            &mut self,
            document: PageHostDocument,
        ) -> io::Result<PageHostReply> {
            self.lifecycle.synchronize_document(document)
        }

        fn close_realm(
            &mut self,
            tab_id: u64,
            document_generation: u64,
        ) -> io::Result<PageHostReply> {
            self.lifecycle.close_realm(tab_id, document_generation)
        }

        fn debugger_realm_stats(
            &mut self,
            tab_id: u64,
            document_generation: u64,
        ) -> io::Result<PageHostReply> {
            self.lifecycle
                .debugger_realm_stats(tab_id, document_generation)
        }

        fn debugger_bluets_metadata_available(&self) -> bool {
            true
        }

        fn debugger_bluets_metadata_sources_available(&self) -> bool {
            true
        }

        fn debugger_bluets_safe_point_span_available(&self) -> bool {
            true
        }

        fn debugger_bluets_source_breakpoint_available(&self) -> bool {
            true
        }

        fn debugger_execution_control_available(&self) -> bool {
            true
        }

        fn debugger_stepping_available(&self) -> bool {
            true
        }

        fn debugger_bluets_source_span_step_available(&self) -> bool {
            true
        }

        fn step_debugger_bluets_source_span(
            &mut self,
            _tab_id: u64,
            _document_generation: u64,
            _metadata: PageHostDebuggerMetadataHandle,
            _source_id: u32,
            _safe_point: PageHostDebuggerSafePoint,
        ) -> io::Result<PageHostReply> {
            Ok(self.reply.clone())
        }

        fn debugger_bluets_safe_point_span(
            &mut self,
            _tab_id: u64,
            _document_generation: u64,
            _metadata: PageHostDebuggerMetadataHandle,
            _safe_point: PageHostDebuggerSafePoint,
        ) -> io::Result<PageHostReply> {
            Ok(self.reply.clone())
        }

        fn debugger_bluets_source_breakpoint(
            &mut self,
            _tab_id: u64,
            _document_generation: u64,
            _program: PageHostDebuggerProgram,
            _metadata: PageHostDebuggerMetadataHandle,
            _source_id: u32,
            _source_byte: u32,
        ) -> io::Result<PageHostReply> {
            Ok(self.reply.clone())
        }
    }

    #[test]
    fn core_forwards_zero_based_bluets_source_id_but_rejects_mismatched_step_echo() {
        let (tabs, tab_id) = loaded_tabs(
            "<script type=\"application/x-blueice-typescript\">const zero: number = 1;</script>",
            "https://example.test/zero-source.html",
        );
        let child_program = PageHostDebuggerProgram {
            program_handle: 11,
            program_generation: 13,
        };
        let child_metadata = PageHostDebuggerMetadataHandle {
            metadata_handle: 17,
            metadata_generation: 19,
        };
        let child_safe_point = PageHostDebuggerSafePoint {
            program: child_program,
            code_unit_ordinal: 0,
            bytecode_offset: 4,
        };
        let reply = PageHostReply::DebuggerBlueTsSourceStepRequested {
            tab_id: tab_id.as_u64(),
            document_generation: 1,
            metadata: child_metadata,
            source_id: 0,
            safe_point: child_safe_point,
        };
        let mut executor = OutOfProcessJavaScriptPageExecutor::new_with_debugger_execution_control(
            MalformedSafePointSpanChild {
                lifecycle: RecordingChild::default(),
                reply: reply.clone(),
            },
        );
        executor.synchronize_and_execute(&tabs).unwrap();
        executor.debugger_programs.insert(
            tab_id,
            BTreeMap::from([(
                child_program,
                CoreDebuggerProgram {
                    program_handle: 101,
                    program_generation: 103,
                },
            )]),
        );
        executor.debugger_static_metadata.insert(
            tab_id,
            BTreeMap::from([(
                child_metadata,
                CoreDebuggerStaticMetadata {
                    program: child_program,
                    metadata_handle: 107,
                    metadata_generation: 109,
                },
            )]),
        );
        let target = JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget {
            program_handle: 101,
            program_generation: 103,
            metadata_handle: 107,
            metadata_generation: 109,
            source_id: 0,
            code_unit_ordinal: 0,
            bytecode_offset: 4,
        };
        assert!(executor.debugger_source_span_stepping_available());
        assert_eq!(
            executor.step_debugger_bluets_source_span(tab_id, 1, target),
            Ok(())
        );
        executor.child.reply = PageHostReply::DebuggerBlueTsSourceStepRequested {
            tab_id: tab_id.as_u64(),
            document_generation: 1,
            metadata: child_metadata,
            source_id: 1,
            safe_point: child_safe_point,
        };
        assert_eq!(
            executor.step_debugger_bluets_source_span(tab_id, 1, target),
            Err(JavaScriptPageDebuggerError::NoLiveRealm)
        );
    }

    #[test]
    fn core_rejects_mismatched_child_safe_point_span_envelopes() {
        let (tabs, tab_id) = loaded_tabs(
            "<script type=\"application/x-blueice-typescript\">const mapped: number = 42;</script>",
            "https://example.test/forged-span.html",
        );
        let child_program = PageHostDebuggerProgram {
            program_handle: 11,
            program_generation: 13,
        };
        let child_metadata = PageHostDebuggerMetadataHandle {
            metadata_handle: 17,
            metadata_generation: 19,
        };
        let child_safe_point = PageHostDebuggerSafePoint {
            program: child_program,
            code_unit_ordinal: 0,
            bytecode_offset: 4,
        };
        let span = page_host::PageHostDebuggerBlueTsSafePointSpan {
            source_id: 3,
            start_byte: 0,
            end_byte: 5,
            coordinates: blueice_ipc::debugger::DebuggerSourceCoordinates {
                start_line: 0,
                start_column_utf16: 0,
                end_line: 0,
                end_column_utf16: 5,
            },
        };
        let span_reply = |reply_tab_id, reply_generation, metadata, safe_point, span| {
            PageHostReply::DebuggerBlueTsSafePointSpan {
                tab_id: reply_tab_id,
                document_generation: reply_generation,
                metadata,
                safe_point,
                span,
            }
        };
        let reply = span_reply(tab_id.as_u64(), 1, child_metadata, child_safe_point, span);
        let mut executor = OutOfProcessJavaScriptPageExecutor::new(MalformedSafePointSpanChild {
            lifecycle: RecordingChild::default(),
            reply: reply.clone(),
        });
        executor.synchronize_and_execute(&tabs).unwrap();
        executor.debugger_programs.insert(
            tab_id,
            BTreeMap::from([(
                child_program,
                CoreDebuggerProgram {
                    program_handle: 101,
                    program_generation: 103,
                },
            )]),
        );
        executor.debugger_static_metadata.insert(
            tab_id,
            BTreeMap::from([(
                child_metadata,
                CoreDebuggerStaticMetadata {
                    program: child_program,
                    metadata_handle: 107,
                    metadata_generation: 109,
                },
            )]),
        );
        let target = JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget {
            program_handle: 101,
            program_generation: 103,
            metadata_handle: 107,
            metadata_generation: 109,
            source_id: 3,
            code_unit_ordinal: 0,
            bytecode_offset: 4,
        };
        assert_eq!(
            executor
                .debugger_static_metadata_safe_point_span(tab_id, 1, target)
                .unwrap(),
            JavaScriptPageDebuggerStaticMetadataSafePointSpan {
                source_id: 3,
                start_byte: 0,
                end_byte: 5,
                coordinates: span.coordinates,
            }
        );
        for malformed in [
            span_reply(
                tab_id.as_u64() + 1,
                1,
                child_metadata,
                child_safe_point,
                span,
            ),
            span_reply(tab_id.as_u64(), 2, child_metadata, child_safe_point, span),
            span_reply(
                tab_id.as_u64(),
                1,
                PageHostDebuggerMetadataHandle {
                    metadata_generation: 20,
                    ..child_metadata
                },
                child_safe_point,
                span,
            ),
            span_reply(
                tab_id.as_u64(),
                1,
                child_metadata,
                PageHostDebuggerSafePoint {
                    bytecode_offset: 5,
                    ..child_safe_point
                },
                span,
            ),
            span_reply(
                tab_id.as_u64(),
                1,
                child_metadata,
                child_safe_point,
                page_host::PageHostDebuggerBlueTsSafePointSpan {
                    end_byte: span.start_byte,
                    ..span
                },
            ),
            span_reply(
                tab_id.as_u64(),
                1,
                child_metadata,
                child_safe_point,
                page_host::PageHostDebuggerBlueTsSafePointSpan {
                    end_byte: DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES + 1,
                    ..span
                },
            ),
            span_reply(
                tab_id.as_u64(),
                1,
                child_metadata,
                child_safe_point,
                page_host::PageHostDebuggerBlueTsSafePointSpan {
                    coordinates: blueice_ipc::debugger::DebuggerSourceCoordinates {
                        start_column_utf16: span.end_byte + 1,
                        ..span.coordinates
                    },
                    ..span
                },
            ),
        ] {
            executor.child.reply = malformed;
            assert_eq!(
                executor.debugger_static_metadata_safe_point_span(tab_id, 1, target),
                Err(JavaScriptPageDebuggerError::NoLiveRealm)
            );
        }
        executor.child.reply = span_reply(
            tab_id.as_u64(),
            1,
            child_metadata,
            child_safe_point,
            page_host::PageHostDebuggerBlueTsSafePointSpan {
                source_id: 4,
                ..span
            },
        );
        assert_eq!(
            executor.debugger_static_metadata_safe_point_span(tab_id, 1, target),
            Err(JavaScriptPageDebuggerError::UnknownProgram)
        );

        let breakpoint_target = JavaScriptPageDebuggerStaticMetadataSourceBreakpointTarget {
            program_handle: 101,
            program_generation: 103,
            metadata_handle: 107,
            metadata_generation: 109,
            source_id: 3,
            source_byte: 2,
        };
        let breakpoint_reply = |reply_tab_id,
                                reply_generation,
                                program,
                                metadata,
                                source_id,
                                source_byte,
                                safe_point| {
            PageHostReply::DebuggerBlueTsSourceBreakpoint {
                tab_id: reply_tab_id,
                document_generation: reply_generation,
                program,
                metadata,
                source_id,
                source_byte,
                safe_point,
            }
        };
        executor.child.reply = breakpoint_reply(
            tab_id.as_u64(),
            1,
            child_program,
            child_metadata,
            3,
            2,
            Some(child_safe_point),
        );
        assert_eq!(
            executor
                .debugger_static_metadata_source_breakpoint(tab_id, 1, breakpoint_target)
                .unwrap(),
            Some(JavaScriptPageDebuggerSafePoint {
                code_unit_ordinal: 0,
                bytecode_offset: 4,
            })
        );
        executor.child.reply = breakpoint_reply(
            tab_id.as_u64(),
            1,
            child_program,
            child_metadata,
            3,
            2,
            None,
        );
        assert_eq!(
            executor
                .debugger_static_metadata_source_breakpoint(tab_id, 1, breakpoint_target)
                .unwrap(),
            None
        );
        for malformed in [
            breakpoint_reply(8, 1, child_program, child_metadata, 3, 2, None),
            breakpoint_reply(
                tab_id.as_u64(),
                2,
                child_program,
                child_metadata,
                3,
                2,
                None,
            ),
            breakpoint_reply(
                tab_id.as_u64(),
                1,
                PageHostDebuggerProgram {
                    program_generation: 14,
                    ..child_program
                },
                child_metadata,
                3,
                2,
                None,
            ),
            breakpoint_reply(
                tab_id.as_u64(),
                1,
                child_program,
                PageHostDebuggerMetadataHandle {
                    metadata_generation: 20,
                    ..child_metadata
                },
                3,
                2,
                None,
            ),
            breakpoint_reply(
                tab_id.as_u64(),
                1,
                child_program,
                child_metadata,
                4,
                2,
                None,
            ),
            breakpoint_reply(
                tab_id.as_u64(),
                1,
                child_program,
                child_metadata,
                3,
                3,
                None,
            ),
            breakpoint_reply(
                tab_id.as_u64(),
                1,
                child_program,
                child_metadata,
                3,
                2,
                Some(PageHostDebuggerSafePoint {
                    program: PageHostDebuggerProgram {
                        program_generation: 14,
                        ..child_program
                    },
                    ..child_safe_point
                }),
            ),
        ] {
            executor.child.reply = malformed;
            assert_eq!(
                executor.debugger_static_metadata_source_breakpoint(tab_id, 1, breakpoint_target),
                Err(JavaScriptPageDebuggerError::NoLiveRealm)
            );
        }
    }

    #[test]
    fn core_proxies_real_child_root_safe_point_pause_and_resume_without_private_ids() {
        use crate::debugger::handle_debugger_request_with_page_javascript_executor;
        use blueice_ipc::debugger::{
            DebuggerCapability, DebuggerCapabilityState, DebuggerPageRealm, DebuggerReply,
            DebuggerRequest,
        };

        let (path, token, child) = spawn_child();
        let (tabs, tab_id) = loaded_tabs(
            "<script>let first = 1; first += 1; globalThis.answer = first;</script>",
            "https://example.test/root-safe-point.html",
        );
        let mut executor =
            OutOfProcessJavaScriptPageExecutor::connect_with_debugger_execution_control(
                &path, &token,
            )
            .unwrap();
        executor.synchronize_and_execute(&tabs).unwrap();
        let realm = DebuggerPageRealm {
            browser_context_id: crate::debugger::DEFAULT_BROWSER_CONTEXT_ID,
            tab_id: tab_id.as_u64(),
            realm_generation: 1,
        };
        let DebuggerReply::Capabilities(capabilities) =
            handle_debugger_request_with_page_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::DescribeCapabilities { realm },
            )
        else {
            panic!("expected OOP debugger capabilities");
        };
        assert!(capabilities.reports.iter().any(|report| {
            report.capability == DebuggerCapability::PauseResume
                && report.state == DebuggerCapabilityState::Available
        }));
        let program = match handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ListPrograms { realm },
        ) {
            DebuggerReply::Programs(programs) => programs[0],
            reply => panic!("expected public OOP debugger program, got {reply:?}"),
        };
        assert!(
            program.program_handle >= CORE_CHILD_DEBUGGER_ID_NAMESPACE_START,
            "core must not disclose child-private program IDs"
        );
        let safe_point = match handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ListSafePoints { program },
        ) {
            DebuggerReply::SafePoints(safe_points) => safe_points
                .into_iter()
                .find(|safe_point| {
                    safe_point.code_unit_ordinal == 0 && safe_point.bytecode_offset != 0
                })
                .expect("fixture has a resumable root safe point"),
            reply => panic!("expected public OOP safe points, got {reply:?}"),
        };
        assert_eq!(
            handle_debugger_request_with_page_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::ArmRootSafePointBreakpoint { safe_point },
            ),
            DebuggerReply::RootSafePointBreakpointArmed { safe_point }
        );
        executor.synchronize_and_execute(&tabs).unwrap();
        assert_eq!(
            handle_debugger_request_with_page_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::GetExecutionState { program },
            ),
            DebuggerReply::ExecutionState {
                program,
                state: blueice_ipc::debugger::DebuggerExecutionState::Paused { safe_point },
            }
        );
        assert_eq!(
            handle_debugger_request_with_page_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::ResumeExecution { program },
            ),
            DebuggerReply::ExecutionResumed { program }
        );
        executor.synchronize_and_execute(&tabs).unwrap();
        assert_eq!(
            handle_debugger_request_with_page_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::GetExecutionState { program },
            ),
            DebuggerReply::ExecutionState {
                program,
                state: blueice_ipc::debugger::DebuggerExecutionState::Completed,
            }
        );
        assert!(matches!(
            executor.drain_reports_for_tab(tab_id).as_slice(),
            [JavaScriptPageExecutionReport::Executed { ordinal: 0, .. }]
        ));
        drop(executor);
        shutdown_child(&path, &token);
        child.join().unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn core_proxies_real_child_nested_frame_without_exposing_private_program_ids() {
        let (path, token, child) = spawn_child();
        let (tabs, tab_id) = loaded_tabs(
            "<script>function inner() { return 4; } globalThis.answer = inner() + 1;</script>",
            "https://example.test/nested-frame.html",
        );
        let mut executor =
            OutOfProcessJavaScriptPageExecutor::connect_with_debugger_execution_control(
                &path, &token,
            )
            .unwrap();
        executor.synchronize_and_execute(&tabs).unwrap();
        assert!(executor.debugger_nested_frames_available());
        let program = executor.debugger_programs(tab_id, 1).unwrap()[0];
        assert!(program.program_handle >= CORE_CHILD_DEBUGGER_ID_NAMESPACE_START);
        let target = executor
            .debugger_safe_points(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap()
            .into_iter()
            .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
            .unwrap();
        executor
            .arm_debugger_nested_safe_point_breakpoint(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
                target.code_unit_ordinal,
                target.bytecode_offset,
            )
            .unwrap();
        executor.synchronize_and_execute(&tabs).unwrap();
        let Some(JavaScriptPageDebuggerNestedExecutionState::Paused {
            frame,
            bytecode_offset,
        }) = executor
            .debugger_nested_execution_state(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap()
        else {
            panic!("the exact nested invocation must be visible to core");
        };
        assert_eq!(bytecode_offset, target.bytecode_offset);
        assert_eq!(frame.tab_id, tab_id);
        assert_eq!(frame.document_generation, 1);
        assert_eq!(frame.program_handle, program.program_handle);
        assert_eq!(frame.program_generation, program.program_generation);
        assert_eq!(frame.code_unit_ordinal, 1);
        assert_ne!(frame.frame_handle, 0);
        let stack = executor
            .debugger_stack_snapshot(tab_id, 1, program, Some(frame), 1, 1)
            .unwrap();
        assert_eq!(stack.frames.len(), 1);
        assert_eq!(stack.frames[0].code_unit_ordinal, 1);
        assert_eq!(stack.frames[0].bytecode_offset, bytecode_offset);
        assert!(stack.stack_truncated);
        assert_eq!(
            executor.debugger_stack_snapshot(
                tab_id,
                1,
                program,
                Some(JavaScriptPageDebuggerFrame {
                    frame_handle: frame.frame_handle + 1,
                    ..frame
                }),
                2,
                256,
            ),
            Err(JavaScriptPageDebuggerError::InvalidExecutionState)
        );
        assert_eq!(
            executor.debugger_stack_snapshot(tab_id, 1, program, None, 2, 256,),
            Err(JavaScriptPageDebuggerError::InvalidExecutionState)
        );
        assert_eq!(
            executor
                .debugger_nested_execution_state(
                    tab_id,
                    1,
                    program.program_handle,
                    program.program_generation,
                )
                .unwrap(),
            Some(JavaScriptPageDebuggerNestedExecutionState::Paused {
                frame,
                bytecode_offset,
            })
        );
        assert_eq!(
            executor.step_debugger_nested_instruction(JavaScriptPageDebuggerFrame {
                frame_handle: frame.frame_handle + 1,
                ..frame
            }),
            Err(JavaScriptPageDebuggerError::InvalidExecutionState)
        );
        let mut returned = false;
        for _ in 0..96 {
            executor.step_debugger_nested_instruction(frame).unwrap();
            assert_eq!(
                executor
                    .debugger_nested_execution_state(
                        tab_id,
                        1,
                        program.program_handle,
                        program.program_generation,
                    )
                    .unwrap(),
                Some(JavaScriptPageDebuggerNestedExecutionState::Stepping { frame })
            );
            executor.synchronize_and_execute(&tabs).unwrap();
            match executor
                .debugger_nested_execution_state(
                    tab_id,
                    1,
                    program.program_handle,
                    program.program_generation,
                )
                .unwrap()
            {
                Some(JavaScriptPageDebuggerNestedExecutionState::Paused {
                    frame: same_frame,
                    ..
                }) => assert_eq!(same_frame, frame),
                None => {
                    returned = true;
                    break;
                }
                state => panic!("unexpected child frame state: {state:?}"),
            }
        }
        assert!(returned);
        assert_eq!(
            executor.step_debugger_nested_instruction(frame),
            Err(JavaScriptPageDebuggerError::InvalidExecutionState)
        );
        assert!(matches!(
            executor
                .debugger_execution_state(
                    tab_id,
                    1,
                    program.program_handle,
                    program.program_generation,
                )
                .unwrap(),
            JavaScriptPageDebuggerExecutionState::Paused {
                code_unit_ordinal: 0,
                ..
            }
        ));
        let root_stack = executor
            .debugger_stack_snapshot(tab_id, 1, program, None, 2, 256)
            .unwrap();
        assert_eq!(root_stack.frames.len(), 1);
        assert_eq!(root_stack.frames[0].code_unit_ordinal, 0);
        assert!(!root_stack.stack_truncated);
        executor
            .resume_debugger_execution(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap();
        executor.synchronize_and_execute(&tabs).unwrap();
        assert_eq!(
            executor
                .debugger_execution_state(
                    tab_id,
                    1,
                    program.program_handle,
                    program.program_generation,
                )
                .unwrap(),
            JavaScriptPageDebuggerExecutionState::Completed
        );
        drop(executor);
        shutdown_child(&path, &token);
        child.join().unwrap();
        let _ = std::fs::remove_file(path);

        // A replacement executor starts with the same core program counter,
        // but its process-unique frame handle must not alias the predecessor.
        let (next_path, next_token, next_child) = spawn_child();
        let mut replacement =
            OutOfProcessJavaScriptPageExecutor::connect_with_debugger_execution_control(
                &next_path,
                &next_token,
            )
            .unwrap();
        replacement.synchronize_and_execute(&tabs).unwrap();
        let next_program = replacement.debugger_programs(tab_id, 1).unwrap()[0];
        let next_target = replacement
            .debugger_safe_points(
                tab_id,
                1,
                next_program.program_handle,
                next_program.program_generation,
            )
            .unwrap()
            .into_iter()
            .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
            .unwrap();
        replacement
            .arm_debugger_nested_safe_point_breakpoint(
                tab_id,
                1,
                next_program.program_handle,
                next_program.program_generation,
                next_target.code_unit_ordinal,
                next_target.bytecode_offset,
            )
            .unwrap();
        replacement.synchronize_and_execute(&tabs).unwrap();
        let Some(JavaScriptPageDebuggerNestedExecutionState::Paused {
            frame: next_frame, ..
        }) = replacement
            .debugger_nested_execution_state(
                tab_id,
                1,
                next_program.program_handle,
                next_program.program_generation,
            )
            .unwrap()
        else {
            panic!("the replacement child must pause its own frame");
        };
        assert_eq!(next_frame.program_handle, frame.program_handle);
        assert_ne!(next_frame.frame_handle, frame.frame_handle);
        assert_eq!(
            replacement.step_debugger_nested_instruction(frame),
            Err(JavaScriptPageDebuggerError::InvalidExecutionState)
        );
        replacement.close_page(tab_id);
        assert_eq!(
            replacement.step_debugger_nested_instruction(next_frame),
            Err(JavaScriptPageDebuggerError::InvalidExecutionState)
        );
        drop(replacement);
        shutdown_child(&next_path, &next_token);
        next_child.join().unwrap();
        let _ = std::fs::remove_file(next_path);
    }

    #[test]
    fn core_proxies_exact_nested_resume_to_real_bluets_child_and_revokes_handle() {
        let (path, token, child) = spawn_child();
        let (tabs, tab_id) = loaded_tabs(
            "<script type=\"application/x-blueice-typescript\">function inner(): number { return 4; } globalThis.answer = inner() + 1;</script>",
            "https://example.test/private-nested-resume.html",
        );
        let mut executor =
            OutOfProcessJavaScriptPageExecutor::connect_with_debugger_execution_control(
                &path, &token,
            )
            .unwrap();
        executor.synchronize_and_execute(&tabs).unwrap();
        let program = executor.debugger_programs(tab_id, 1).unwrap()[0];
        let target = executor
            .debugger_safe_points(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap()
            .into_iter()
            .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
            .unwrap();
        executor
            .arm_debugger_nested_safe_point_breakpoint(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
                target.code_unit_ordinal,
                target.bytecode_offset,
            )
            .unwrap();
        executor.synchronize_and_execute(&tabs).unwrap();
        let Some(JavaScriptPageDebuggerNestedExecutionState::Paused { frame, .. }) = executor
            .debugger_nested_execution_state(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap()
        else {
            panic!("BlueTS child must pause under a core-owned handle");
        };
        assert_eq!(
            executor.resume_debugger_nested_execution(JavaScriptPageDebuggerFrame {
                frame_handle: frame.frame_handle + 1,
                ..frame
            }),
            Err(JavaScriptPageDebuggerError::InvalidExecutionState)
        );
        executor.resume_debugger_nested_execution(frame).unwrap();
        assert_eq!(
            executor
                .debugger_nested_execution_state(
                    tab_id,
                    1,
                    program.program_handle,
                    program.program_generation,
                )
                .unwrap(),
            Some(JavaScriptPageDebuggerNestedExecutionState::Resuming { frame })
        );
        executor.synchronize_and_execute(&tabs).unwrap();
        assert_eq!(
            executor
                .debugger_nested_execution_state(
                    tab_id,
                    1,
                    program.program_handle,
                    program.program_generation,
                )
                .unwrap(),
            None
        );
        assert_eq!(
            executor.resume_debugger_nested_execution(frame),
            Err(JavaScriptPageDebuggerError::InvalidExecutionState)
        );
        assert!(matches!(
            executor
                .debugger_execution_state(
                    tab_id,
                    1,
                    program.program_handle,
                    program.program_generation,
                )
                .unwrap(),
            JavaScriptPageDebuggerExecutionState::Paused {
                code_unit_ordinal: 0,
                ..
            }
        ));
        executor
            .resume_debugger_execution(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap();
        executor.synchronize_and_execute(&tabs).unwrap();
        assert_eq!(
            executor
                .debugger_execution_state(
                    tab_id,
                    1,
                    program.program_handle,
                    program.program_generation,
                )
                .unwrap(),
            JavaScriptPageDebuggerExecutionState::Completed
        );
        drop(executor);
        shutdown_child(&path, &token);
        child.join().unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn real_bluets_child_private_stack_is_bounded_and_publicly_disabled() {
        use blueice_ipc::debugger::{
            DebuggerCapability, DebuggerCapabilityState, DebuggerPageRealm, DebuggerReply,
            DebuggerRequest,
        };

        let (path, token, child) = spawn_child();
        let (mut tabs, tab_id) = loaded_tabs(
            "<script type=\"application/x-blueice-typescript\">function inner(a: number, b: number): number { let first: number = a; let second: number = b; return first + second; } globalThis.answer = inner(1, 2);</script>",
            "https://example.test/private-stack.html",
        );
        let mut executor =
            OutOfProcessJavaScriptPageExecutor::connect_with_debugger_execution_control(
                &path, &token,
            )
            .unwrap();
        executor.synchronize_and_execute(&tabs).unwrap();
        let realm = DebuggerPageRealm {
            browser_context_id: crate::debugger::DEFAULT_BROWSER_CONTEXT_ID,
            tab_id: tab_id.as_u64(),
            realm_generation: 1,
        };
        let DebuggerReply::Capabilities(capabilities) =
            crate::debugger::handle_debugger_request_with_page_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::DescribeCapabilities { realm },
            )
        else {
            panic!("the debugger must describe its actual capability boundary");
        };
        for capability in [DebuggerCapability::Stack, DebuggerCapability::Scopes] {
            assert!(capabilities.reports.iter().any(|report| {
                report.capability == capability && report.state == DebuggerCapabilityState::Planned
            }));
        }
        let program = executor.debugger_programs(tab_id, 1).unwrap()[0];
        let target = executor
            .debugger_safe_points(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap()
            .into_iter()
            .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
            .unwrap();
        executor
            .arm_debugger_nested_safe_point_breakpoint(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
                1,
                0,
            )
            .unwrap();
        executor.synchronize_and_execute(&tabs).unwrap();
        let Some(JavaScriptPageDebuggerNestedExecutionState::Paused { frame, .. }) = executor
            .debugger_nested_execution_state(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap()
        else {
            panic!("the BlueTS child must pause before its first instruction");
        };
        assert_eq!(frame.code_unit_ordinal, target.code_unit_ordinal);
        let mut full = None;
        for _ in 0..96 {
            let snapshot = executor
                .debugger_stack_snapshot(tab_id, 1, program, Some(frame), 2, 256)
                .unwrap();
            if snapshot.frames[0].scope_entries.len() >= 2 {
                full = Some(snapshot);
                break;
            }
            executor.step_debugger_nested_instruction(frame).unwrap();
            executor.synchronize_and_execute(&tabs).unwrap();
            assert!(matches!(
                executor
                    .debugger_nested_execution_state(
                        tab_id,
                        1,
                        program.program_handle,
                        program.program_generation,
                    )
                    .unwrap(),
                Some(JavaScriptPageDebuggerNestedExecutionState::Paused { frame: same, .. })
                    if same == frame
            ));
        }
        let full = full.expect("the child must enter a scope with two active slots");
        assert_eq!(full.frames.len(), 2);
        assert_eq!(full.frames[0].code_unit_ordinal, 1);
        assert_eq!(full.frames[1].code_unit_ordinal, 0);
        assert!(!full.stack_truncated);
        assert!(!full.frames[0].scope_truncated);
        let limited = executor
            .debugger_stack_snapshot(tab_id, 1, program, Some(frame), 1, 1)
            .unwrap();
        assert_eq!(limited.frames.len(), 1);
        assert_eq!(limited.frames[0].scope_entries.len(), 1);
        assert!(limited.stack_truncated);
        assert!(limited.frames[0].scope_truncated);
        assert_eq!(
            executor
                .debugger_stack_snapshot(tab_id, 1, program, Some(frame), 2, 256)
                .unwrap(),
            full
        );
        assert_eq!(
            executor.debugger_stack_snapshot(tab_id, 1, program, Some(frame), 0, 1),
            Err(JavaScriptPageDebuggerError::ResourceLimit)
        );
        assert_eq!(
            executor.debugger_stack_snapshot(tab_id, 1, program, Some(frame), 2, 257),
            Err(JavaScriptPageDebuggerError::ResourceLimit)
        );
        assert_eq!(
            executor.debugger_stack_snapshot(
                tab_id,
                1,
                program,
                Some(JavaScriptPageDebuggerFrame {
                    frame_handle: frame.frame_handle + 1,
                    ..frame
                }),
                2,
                256,
            ),
            Err(JavaScriptPageDebuggerError::InvalidExecutionState)
        );
        assert_eq!(
            executor.debugger_stack_snapshot(
                tab_id,
                1,
                JavaScriptPageDebuggerProgram {
                    program_generation: program.program_generation + 1,
                    ..program
                },
                Some(frame),
                2,
                256,
            ),
            Err(JavaScriptPageDebuggerError::UnknownProgram)
        );
        tabs.get_mut(tab_id).unwrap().load_html_str(
            "<script>globalThis.successor = true;</script>",
            Some("https://example.test/successor-stack.html".to_string()),
        );
        executor.synchronize_and_execute(&tabs).unwrap();
        assert_eq!(
            executor.debugger_stack_snapshot(tab_id, 1, program, Some(frame), 2, 256),
            Err(JavaScriptPageDebuggerError::NoLiveRealm)
        );
        drop(executor);
        shutdown_child(&path, &token);
        child.join().unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn public_core_route_steps_one_real_child_nested_frame_without_root_aliasing() {
        use crate::debugger::handle_debugger_request_with_page_javascript_executor;
        use blueice_ipc::debugger::{
            DebuggerCapability, DebuggerCapabilityState, DebuggerExecutionState, DebuggerPageRealm,
            DebuggerReply, DebuggerRequest,
        };

        let (path, token, child) = spawn_child();
        let (tabs, tab_id) = loaded_tabs(
            "<script>function inner() { return 4; } globalThis.answer = inner() + 1;</script>",
            "https://example.test/public-nested.html",
        );
        let mut executor =
            OutOfProcessJavaScriptPageExecutor::connect_with_debugger_execution_control(
                &path, &token,
            )
            .unwrap();
        executor.synchronize_and_execute(&tabs).unwrap();
        let realm = DebuggerPageRealm {
            browser_context_id: crate::debugger::DEFAULT_BROWSER_CONTEXT_ID,
            tab_id: tab_id.as_u64(),
            realm_generation: 1,
        };
        let DebuggerReply::Capabilities(capabilities) =
            handle_debugger_request_with_page_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::DescribeCapabilities { realm },
            )
        else {
            panic!("expected live debugger capabilities");
        };
        assert!(capabilities.reports.iter().any(|report| {
            report.capability == DebuggerCapability::NestedFrames
                && report.state == DebuggerCapabilityState::Available
        }));
        let program = match handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ListPrograms { realm },
        ) {
            DebuggerReply::Programs(programs) => programs[0],
            reply => panic!("expected one program: {reply:?}"),
        };
        let target = match handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ListSafePoints { program },
        ) {
            DebuggerReply::SafePoints(points) => points
                .into_iter()
                .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
                .unwrap(),
            reply => panic!("expected nested safe points: {reply:?}"),
        };
        assert_eq!(
            handle_debugger_request_with_page_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::ArmNestedSafePointBreakpoint { safe_point: target },
            ),
            DebuggerReply::NestedSafePointBreakpointArmed { safe_point: target }
        );
        executor.synchronize_and_execute(&tabs).unwrap();
        let frame = match handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::GetExecutionState { program },
        ) {
            DebuggerReply::ExecutionState {
                state: DebuggerExecutionState::NestedPaused { frame, safe_point },
                ..
            } => {
                assert_eq!(safe_point, target);
                frame
            }
            reply => panic!("expected a public active frame: {reply:?}"),
        };
        assert_eq!(frame.program, program);
        assert_ne!(frame.frame_handle, 0);
        assert!(matches!(
            handle_debugger_request_with_page_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::StepRootInstruction { program },
            ),
            DebuggerReply::Error { .. }
        ));
        let mut wrong = frame;
        wrong.frame_handle += 1;
        assert!(matches!(
            handle_debugger_request_with_page_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::StepNestedInstruction { frame: wrong },
            ),
            DebuggerReply::Error { .. }
        ));
        let mut returned = false;
        for _ in 0..96 {
            assert_eq!(
                handle_debugger_request_with_page_javascript_executor(
                    &tabs,
                    Some(&mut executor),
                    DebuggerRequest::StepNestedInstruction { frame },
                ),
                DebuggerReply::NestedStepRequested { frame }
            );
            assert_eq!(
                handle_debugger_request_with_page_javascript_executor(
                    &tabs,
                    Some(&mut executor),
                    DebuggerRequest::GetExecutionState { program },
                ),
                DebuggerReply::ExecutionState {
                    program,
                    state: DebuggerExecutionState::NestedStepping { frame },
                }
            );
            executor.synchronize_and_execute(&tabs).unwrap();
            match handle_debugger_request_with_page_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::GetExecutionState { program },
            ) {
                DebuggerReply::ExecutionState {
                    state:
                        DebuggerExecutionState::NestedPaused {
                            frame: same_frame, ..
                        },
                    ..
                } => assert_eq!(same_frame, frame),
                DebuggerReply::ExecutionState {
                    state: DebuggerExecutionState::Paused { safe_point },
                    ..
                } => {
                    assert_eq!(safe_point.code_unit_ordinal, 0);
                    returned = true;
                    break;
                }
                reply => panic!("unexpected nested successor: {reply:?}"),
            }
        }
        assert!(returned);
        assert!(matches!(
            handle_debugger_request_with_page_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::StepNestedInstruction { frame },
            ),
            DebuggerReply::Error { .. }
        ));
        assert_eq!(
            handle_debugger_request_with_page_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::ResumeExecution { program },
            ),
            DebuggerReply::ExecutionResumed { program }
        );
        executor.synchronize_and_execute(&tabs).unwrap();
        assert_eq!(
            handle_debugger_request_with_page_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::GetExecutionState { program },
            ),
            DebuggerReply::ExecutionState {
                program,
                state: DebuggerExecutionState::Completed,
            }
        );
        drop(executor);
        shutdown_child(&path, &token);
        child.join().unwrap();
        let _ = std::fs::remove_file(path);
    }

    /// A hostile authenticated child still cannot acknowledge a different
    /// realm and make core treat the requested document as eligible for any
    /// debugger lifecycle record.
    struct MismatchedSynchronizationChild;

    impl PageHostClient for MismatchedSynchronizationChild {
        fn synchronize_document(
            &mut self,
            document: PageHostDocument,
        ) -> io::Result<PageHostReply> {
            Ok(PageHostReply::Synchronized {
                tab_id: document.tab_id.saturating_add(1),
                document_generation: document.document_generation,
                already_current: false,
                reports: Vec::new(),
            })
        }

        fn close_realm(
            &mut self,
            tab_id: u64,
            document_generation: u64,
        ) -> io::Result<PageHostReply> {
            Ok(PageHostReply::RealmClosed {
                tab_id,
                document_generation,
            })
        }

        fn debugger_execution_control_available(&self) -> bool {
            true
        }
    }

    #[test]
    fn mismatched_child_synchronization_cannot_seed_oop_debugger_lifecycle() {
        let (tabs, tab_id) = loaded_tabs(
            "<script>let childLifecycleSecret = 1;</script>",
            "https://example.test/hostile-child.html",
        );
        let mut executor = OutOfProcessJavaScriptPageExecutor::new_with_debugger_execution_control(
            MismatchedSynchronizationChild,
        );
        executor.synchronize_and_execute(&tabs).unwrap();

        assert!(!executor.debugger_has_live_realm(tab_id, 1));
        assert!(executor.debugger_execution_deferrals.is_empty());
        assert_eq!(
            executor.drain_reports_for_tab(tab_id),
            vec![JavaScriptPageExecutionReport::Rejected {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 0,
                kind: BlueJsPageScriptKind::Classic,
                category: "out-of-process JavaScript host is unavailable",
            }]
        );
    }

    struct MismatchedDebuggerChild;

    impl PageHostClient for MismatchedDebuggerChild {
        fn synchronize_document(
            &mut self,
            document: PageHostDocument,
        ) -> io::Result<PageHostReply> {
            Ok(PageHostReply::Synchronized {
                tab_id: document.tab_id,
                document_generation: document.document_generation,
                already_current: false,
                reports: Vec::new(),
            })
        }

        fn close_realm(
            &mut self,
            tab_id: u64,
            document_generation: u64,
        ) -> io::Result<PageHostReply> {
            Ok(PageHostReply::RealmClosed {
                tab_id,
                document_generation,
            })
        }

        fn debugger_realm_stats(
            &mut self,
            tab_id: u64,
            document_generation: u64,
        ) -> io::Result<PageHostReply> {
            Ok(PageHostReply::RealmStats(page_host::PageHostRealmStats {
                tab_id,
                document_generation,
                program_count: 1,
                bytecode_bytes: 0,
                heap_bytes: 0,
            }))
        }

        fn debugger_programs(
            &mut self,
            _tab_id: u64,
            document_generation: u64,
        ) -> io::Result<PageHostReply> {
            Ok(PageHostReply::DebuggerPrograms {
                // A peer must never be able to retarget core's tab merely by
                // returning a plausible private record under another tuple.
                tab_id: 99,
                document_generation,
                programs: vec![PageHostDebuggerProgram {
                    program_handle: 1,
                    program_generation: 1,
                }],
            })
        }
    }

    #[test]
    fn core_rejects_a_child_debugger_reply_with_a_mismatched_realm_tuple() {
        let (tabs, tab_id) = loaded_tabs(
            "<script>let noChildSourceLeak = true;</script>",
            "https://example.test/app.html",
        );
        let mut executor = OutOfProcessJavaScriptPageExecutor::new(MismatchedDebuggerChild);
        executor.synchronize_and_execute(&tabs).unwrap();
        assert!(executor.debugger_has_live_realm(tab_id, 1));
        let realm = blueice_ipc::debugger::DebuggerPageRealm {
            browser_context_id: crate::debugger::DEFAULT_BROWSER_CONTEXT_ID,
            tab_id: tab_id.as_u64(),
            realm_generation: 1,
        };
        let capabilities = crate::debugger::handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            blueice_ipc::debugger::DebuggerRequest::DescribeCapabilities { realm },
        );
        let blueice_ipc::debugger::DebuggerReply::Capabilities(capabilities) = capabilities else {
            panic!("expected child debugger capability report");
        };
        assert!(capabilities.reports.iter().any(|report| {
            report.capability == blueice_ipc::debugger::DebuggerCapability::ProgramLocations
                && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
        }));
        assert!(capabilities.reports.iter().any(|report| {
            report.capability == blueice_ipc::debugger::DebuggerCapability::BreakpointConfiguration
                && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Planned
        }));
        assert_eq!(
            executor.debugger_programs(tab_id, 1),
            Err(JavaScriptPageDebuggerError::NoLiveRealm)
        );
    }

    /// A hostile private peer can know its own protocol shape, so core must
    /// reject malformed breakpoint acknowledgements rather than treating an
    /// authenticated socket as a source of public debugger identities.
    struct MalformedBreakpointChild;

    impl PageHostClient for MalformedBreakpointChild {
        fn synchronize_document(
            &mut self,
            document: PageHostDocument,
        ) -> io::Result<PageHostReply> {
            Ok(PageHostReply::Synchronized {
                tab_id: document.tab_id,
                document_generation: document.document_generation,
                already_current: false,
                reports: Vec::new(),
            })
        }

        fn close_realm(
            &mut self,
            tab_id: u64,
            document_generation: u64,
        ) -> io::Result<PageHostReply> {
            Ok(PageHostReply::RealmClosed {
                tab_id,
                document_generation,
            })
        }

        fn debugger_realm_stats(
            &mut self,
            tab_id: u64,
            document_generation: u64,
        ) -> io::Result<PageHostReply> {
            Ok(PageHostReply::RealmStats(page_host::PageHostRealmStats {
                tab_id,
                document_generation,
                program_count: 1,
                bytecode_bytes: 0,
                heap_bytes: 0,
            }))
        }

        fn debugger_programs(
            &mut self,
            tab_id: u64,
            document_generation: u64,
        ) -> io::Result<PageHostReply> {
            Ok(PageHostReply::DebuggerPrograms {
                tab_id,
                document_generation,
                programs: vec![PageHostDebuggerProgram {
                    program_handle: 1,
                    program_generation: 1,
                }],
            })
        }

        fn validate_debugger_safe_point(
            &mut self,
            tab_id: u64,
            document_generation: u64,
            safe_point: PageHostDebuggerSafePoint,
        ) -> io::Result<PageHostReply> {
            Ok(PageHostReply::DebuggerSafePointValidated {
                tab_id,
                document_generation,
                safe_point,
            })
        }

        fn set_debugger_breakpoint(
            &mut self,
            tab_id: u64,
            document_generation: u64,
            safe_point: PageHostDebuggerSafePoint,
        ) -> io::Result<PageHostReply> {
            Ok(PageHostReply::DebuggerBreakpointSet {
                // The reply is otherwise plausible, but a core must bind it
                // to the original tab and never silently retarget a request.
                tab_id: tab_id + 1,
                document_generation,
                safe_point,
            })
        }

        fn debugger_breakpoints(
            &mut self,
            tab_id: u64,
            document_generation: u64,
        ) -> io::Result<PageHostReply> {
            Ok(PageHostReply::DebuggerBreakpoints {
                tab_id,
                // A generation mismatch must not reveal or remint a record.
                document_generation: document_generation + 1,
                safe_points: Vec::new(),
            })
        }

        fn clear_debugger_breakpoint(
            &mut self,
            tab_id: u64,
            document_generation: u64,
            safe_point: PageHostDebuggerSafePoint,
        ) -> io::Result<PageHostReply> {
            Ok(PageHostReply::DebuggerBreakpointCleared {
                tab_id,
                document_generation,
                // A response must echo the exact child-private safe point,
                // not merely one with a valid private program ID.
                safe_point: PageHostDebuggerSafePoint {
                    bytecode_offset: safe_point.bytecode_offset.saturating_add(1),
                    ..safe_point
                },
                was_present: true,
            })
        }
    }

    #[test]
    fn core_fails_closed_on_malformed_child_breakpoint_replies() {
        let (tabs, tab_id) = loaded_tabs(
            "<script>let childBreakpointSecret = 1;</script>",
            "https://example.test/app.html",
        );
        let mut executor = OutOfProcessJavaScriptPageExecutor::new(MalformedBreakpointChild);
        executor.synchronize_and_execute(&tabs).unwrap();
        let program = executor.debugger_programs(tab_id, 1).unwrap()[0];
        let result = executor.set_debugger_breakpoint(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
            0,
            0,
        );
        assert_eq!(result, Err(JavaScriptPageDebuggerError::NoLiveRealm));
        assert_eq!(
            executor.debugger_breakpoints(tab_id, 1),
            Err(JavaScriptPageDebuggerError::NoLiveRealm)
        );
        assert_eq!(
            executor.clear_debugger_breakpoint(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
                0,
                0,
            ),
            Err(JavaScriptPageDebuggerError::NoLiveRealm)
        );
    }

    #[test]
    fn core_validates_and_installs_source_free_document_snapshots_in_the_real_child() {
        let (path, token, child) = spawn_child();
        let (tabs, tab_id) = loaded_tabs(
            concat!(
                "<p>core snapshot marker</p>",
                "<script>",
                "if (blueiceDocumentOrigin() !== 'https://example.test') throw 'origin';",
                "if (blueiceDocumentText() === '') throw 'text';",
                "if (typeof document !== 'undefined' || typeof fetch !== 'undefined') throw 'ambient';",
                "</script>"
            ),
            "https://example.test/app/index.html",
        );
        let mut executor = OutOfProcessJavaScriptPageExecutor::connect(&path, &token).unwrap();
        executor.synchronize_and_execute(&tabs).unwrap();
        assert_eq!(
            executor.drain_reports_for_tab(tab_id),
            vec![JavaScriptPageExecutionReport::Executed {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 0,
                kind: BlueJsPageScriptKind::Classic,
            }]
        );
        drop(executor);
        shutdown_child(&path, &token);
        child.join().unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn over_budget_core_snapshot_is_never_sent_to_the_child() {
        let oversized = "x".repeat(blueice_ipc::page_host::PAGE_HOST_DOCUMENT_TEXT_MAX_BYTES + 1);
        let html = format!("<p>{oversized}</p><script>blueiceDocumentText();</script>");
        let (tabs, tab_id) = loaded_tabs(&html, "https://example.test/app/index.html");
        let mut executor = OutOfProcessJavaScriptPageExecutor::new(RecordingChild::default());
        executor.synchronize_and_execute(&tabs).unwrap();
        assert_eq!(
            executor.drain_reports_for_tab(tab_id),
            vec![JavaScriptPageExecutionReport::Rejected {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 0,
                kind: BlueJsPageScriptKind::Classic,
                category: "host binding contract rejected the page script",
            }]
        );
        let child = executor.into_child();
        assert!(child.documents.is_empty());
        assert!(child.closes.is_empty());
    }

    #[test]
    fn core_routes_interleaved_bluets_and_javascript_to_one_child_realm() {
        let (path, token, child) = spawn_child();
        let (tabs, tab_id) = loaded_tabs(
            concat!(
                "<script>globalThis.beforeBlueTs = true;</script>",
                "<script type=\"application/x-blueice-typescript\">const sharedAnswer: number = 42;</script>",
                "<script>if (!globalThis.beforeBlueTs || sharedAnswer !== 42) throw 'shared realm failed';</script>",
                "<script type=\"application/x-blueice-typescript\">blueiceDocumentText();</script>",
                "<script type=\"application/x-blueice-typescript\" src=\"untrusted.ts\"></script>"
            ),
            "https://example.test/app/index.html",
        );
        let mut executor = OutOfProcessJavaScriptPageExecutor::connect(&path, &token).unwrap();
        executor.synchronize_and_execute(&tabs).unwrap();
        assert_eq!(
            executor.drain_reports_for_tab(tab_id),
            vec![
                JavaScriptPageExecutionReport::Executed {
                    tab_id: tab_id.as_u64(),
                    document_generation: 1,
                    ordinal: 0,
                    kind: BlueJsPageScriptKind::Classic,
                },
                JavaScriptPageExecutionReport::Executed {
                    tab_id: tab_id.as_u64(),
                    document_generation: 1,
                    ordinal: 2,
                    kind: BlueJsPageScriptKind::Classic,
                },
            ]
        );
        assert_eq!(
            executor.drain_blue_ts_reports_for_tab(tab_id),
            vec![
                BlueTsPageExecutionReport::Executed {
                    tab_id: tab_id.as_u64(),
                    document_generation: 1,
                    ordinal: 1,
                    kind: DirectPageScriptKind::Classic,
                },
                BlueTsPageExecutionReport::Executed {
                    tab_id: tab_id.as_u64(),
                    document_generation: 1,
                    ordinal: 3,
                    kind: DirectPageScriptKind::Classic,
                },
                BlueTsPageExecutionReport::Rejected {
                    tab_id: tab_id.as_u64(),
                    document_generation: 1,
                    ordinal: 4,
                    kind: DirectPageScriptKind::Classic,
                    category: "external BlueTS declarations require an authorized loader",
                },
            ]
        );
        drop(executor);
        shutdown_child(&path, &token);
        child.join().unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn navigation_replaces_the_child_document_and_close_releases_its_realm() {
        let (path, token, child) = spawn_child();
        let (mut tabs, tab_id) = loaded_tabs(
            "<script>globalThis.firstSnapshot = blueiceDocumentText();</script>",
            "https://example.test/first",
        );
        let mut executor = OutOfProcessJavaScriptPageExecutor::connect(&path, &token).unwrap();
        executor.synchronize_and_execute(&tabs).unwrap();
        assert!(matches!(
            executor.drain_reports_for_tab(tab_id).as_slice(),
            [JavaScriptPageExecutionReport::Executed {
                document_generation: 1,
                ..
            }]
        ));
        tabs.get_mut(tab_id).unwrap().load_html_str(
            concat!(
                "<script>",
                "if (typeof globalThis.firstSnapshot !== 'undefined') throw 'stale realm';",
                "if (blueiceDocumentOrigin() !== 'https://second.example.test') throw 'origin';",
                "globalThis.second = true;",
                "</script>"
            ),
            Some("https://second.example.test/second".to_string()),
        );
        executor.synchronize_and_execute(&tabs).unwrap();
        assert!(matches!(
            executor.drain_reports_for_tab(tab_id).as_slice(),
            [JavaScriptPageExecutionReport::Executed {
                document_generation: 2,
                ..
            }]
        ));
        assert!(tabs.close_tab(tab_id));
        executor.synchronize_and_execute(&tabs).unwrap();
        drop(executor);
        shutdown_child(&path, &token);
        child.join().unwrap();
        let _ = std::fs::remove_file(path);
    }
}
