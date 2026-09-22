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
//! second source loader. It installs only fixed copied JavaScript
//! document-text/origin callbacks; it installs no document object, DOM/event
//! object, IPC, storage, network, URL, resolver, or page-selected binding in
//! the child realm.

use super::{
    contracts::{core_script_binding_contract, CoreScriptBindingContractLimits},
    direct_page::DirectPageScriptKind,
    BlueJsPageScriptKind, CombinedPageScriptDeclaration, CombinedPageScriptLanguage,
};
use crate::script::javascript::{
    AuthorizedJavaScriptModuleGraph, BlueTsPageExecutionReport, JavaScriptPageDebuggerBreakpoint,
    JavaScriptPageDebuggerError, JavaScriptPageDebuggerExecutionState,
    JavaScriptPageDebuggerProgram, JavaScriptPageDebuggerSafePoint, JavaScriptPageExecutionReport,
    PageJavaScriptDebuggerLocations, PageJavaScriptExecutor,
};
use crate::script::page_source_authorizer::AuthorizedPageScriptGraph;
pub use crate::script::page_source_authorizer::{
    AuthorizedOutOfProcessPageScriptGraph, OutOfProcessPageScriptSourceAuthorizationError,
    OutOfProcessPageScriptSourceAuthorizer, OutOfProcessPageScriptSourceRequest,
};
use crate::{Page, TabId, TabManager};
use blueice_ipc::page_host::{
    self, PageHostDebuggerExecutionState, PageHostDebuggerProgram, PageHostDebuggerSafePoint,
    PageHostDocument, PageHostDocumentSnapshot, PageHostErrorCode, PageHostModuleGraph,
    PageHostReply, PageHostRequest, PageHostScript, PageHostScriptKind, PageHostScriptLanguage,
    PageHostScriptOutcome, PageHostSource, PageHostStaticResolution,
    PAGE_HOST_DEBUGGER_MAX_BREAKPOINTS_PER_REALM, PAGE_HOST_DEBUGGER_MAX_SAFE_POINTS_PER_PROGRAM,
};
use blueice_net::canonical_http_origin;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io;
use std::os::unix::net::UnixStream;
use std::path::Path;

/// The maximum number of source-free child-host results retained by core for
/// the existing tab-addressed control-plane drain.
const MAX_EXECUTION_REPORTS: usize = 128;

/// The fixed core-owned resolver identity for a one-source inline document
/// graph. It is not a URL resolver and cannot be selected by page content.
const INLINE_CHILD_RESOLVER_FINGERPRINT: &str = "core-inline-page-host-v1";

/// Core-owned namespace for public debugger program IDs that proxy the child.
/// Keeping it disjoint from the child counter makes it mechanically apparent
/// that a private child identifier cannot become a public protocol identity.
const CORE_CHILD_DEBUGGER_ID_NAMESPACE_START: u64 = 1 << 63;

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
    /// v1 capability handshake. The caller must obtain both values from its
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
}

/// The small transport surface the lifecycle adapter needs. Keeping this
/// separate from a VM/registry prevents the core from bypassing the child's
/// process boundary and lets focused tests use a recording child peer.
pub trait PageHostClient {
    fn synchronize_document(&mut self, document: PageHostDocument) -> io::Result<PageHostReply>;
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
}

impl PageHostClient for PageHostConnection {
    fn synchronize_document(&mut self, document: PageHostDocument) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::SynchronizeDocument { document })
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LiveDocument {
    document_generation: u64,
    origin: String,
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
    /// Core-minted public debugger identities keyed by the child-private
    /// program IDs they represent. Child IDs are transport keys only and can
    /// never accidentally become public protocol IDs.
    debugger_programs: BTreeMap<TabId, BTreeMap<PageHostDebuggerProgram, CoreDebuggerProgram>>,
    next_debugger_program_handle: u64,
    next_debugger_program_generation: u64,
    reports: VecDeque<JavaScriptPageExecutionReport>,
    blue_ts_reports: VecDeque<BlueTsPageExecutionReport>,
}

#[derive(Debug, Clone, Copy)]
struct CoreDebuggerProgram {
    program_handle: u64,
    program_generation: u64,
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
            debugger_programs: BTreeMap::new(),
            next_debugger_program_handle: CORE_CHILD_DEBUGGER_ID_NAMESPACE_START,
            next_debugger_program_generation: CORE_CHILD_DEBUGGER_ID_NAMESPACE_START,
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
            debugger_programs: BTreeMap::new(),
            next_debugger_program_handle: CORE_CHILD_DEBUGGER_ID_NAMESPACE_START,
            next_debugger_program_generation: CORE_CHILD_DEBUGGER_ID_NAMESPACE_START,
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
}

impl<C: PageHostClient> OutOfProcessJavaScriptPageExecutor<C> {
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
            self.advance_debugger_executions();
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
        self.debugger_programs.remove(&tab_id);
    }

    fn synchronize_document(&mut self, tab_id: TabId, page: &Page, identity: LiveDocument) {
        let declarations = page.combined_page_script_declarations();
        let snapshot = match core_document_snapshot(page, &identity) {
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
                self.live_documents.insert(tab_id, identity);
                return;
            }
        };
        let document_url = page
            .url()
            .expect("a page with a live child identity always has a URL");
        let (document, mut local_reports, mut local_blue_ts_reports) = authorized_document(
            tab_id,
            &identity,
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
        let result = self.child.synchronize_document(document);
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
                // runnable under a replaced core document. Best-effort close
                // uses the old generation, which the child rejects if a
                // successor did in fact activate.
                self.close_page(tab_id);
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
                self.close_page(tab_id);
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
        self.live_documents.insert(tab_id, identity);
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

    fn advance_debugger_executions(&mut self) {
        let documents: Vec<_> = self
            .live_documents
            .iter()
            .map(|(&tab_id, document)| (tab_id, document.document_generation))
            .collect();
        for (tab_id, document_generation) in documents {
            let reply = self
                .child
                .advance_debugger_execution(tab_id.as_u64(), document_generation);
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
        matches!(
            self.child
                .debugger_realm_stats(tab_id.as_u64(), document_generation),
            Ok(PageHostReply::RealmStats(stats))
                if stats.tab_id == tab_id.as_u64()
                    && stats.document_generation == document_generation
        )
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
        self.debugger_programs.insert(tab_id, current);
        Ok(programs)
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
            PageHostDebuggerExecutionState::Paused { .. } => {
                Err(JavaScriptPageDebuggerError::NoLiveRealm)
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

    fn unique_socket_path(label: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let count = COUNTER.fetch_add(1, Ordering::Relaxed);
        PathBuf::from("/private/tmp").join(format!(
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
