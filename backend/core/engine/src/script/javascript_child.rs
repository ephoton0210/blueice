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
    AuthorizedJavaScriptModuleGraph, BlueTsPageExecutionReport, JavaScriptPageDebuggerError,
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
    self, PageHostDebuggerProgram, PageHostDebuggerSafePoint, PageHostDocument,
    PageHostDocumentSnapshot, PageHostErrorCode, PageHostModuleGraph, PageHostReply,
    PageHostRequest, PageHostScript, PageHostScriptKind, PageHostScriptLanguage,
    PageHostScriptOutcome, PageHostSource, PageHostStaticResolution,
    PAGE_HOST_DEBUGGER_MAX_SAFE_POINTS_PER_PROGRAM,
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
}

impl<C> OutOfProcessJavaScriptPageExecutor<C> {
    /// Creates an executor around a caller-owned private child connection.
    /// Tests may supply a transport double; production uses
    /// [`PageHostConnection`].
    pub fn new(child: C) -> Self {
        Self {
            child,
            external_source_authorizer: None,
            live_documents: BTreeMap::new(),
            debugger_programs: BTreeMap::new(),
            next_debugger_program_handle: CORE_CHILD_DEBUGGER_ID_NAMESPACE_START,
            next_debugger_program_generation: CORE_CHILD_DEBUGGER_ID_NAMESPACE_START,
            reports: VecDeque::new(),
            blue_ts_reports: VecDeque::new(),
        }
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
        );
        let inline_scripts: Vec<_> = document
            .scripts
            .iter()
            .map(|script| (script.ordinal, script.language, script.kind))
            .collect();
        let result = self.child.synchronize_document(document);
        match result {
            Ok(PageHostReply::Synchronized { reports, .. }) => {
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
}

fn has_duplicate_child_programs(programs: &[PageHostDebuggerProgram]) -> bool {
    let mut seen = BTreeSet::new();
    programs.iter().any(|program| !seen.insert(*program))
}

fn child_debugger_reply_error(reply: &PageHostReply) -> JavaScriptPageDebuggerError {
    match reply {
        PageHostReply::Error {
            code: PageHostErrorCode::ResourceLimit,
            ..
        } => JavaScriptPageDebuggerError::ResourceLimit,
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
    use crate::script::javascript::{AuthorizedJavaScriptModule, AuthorizedJavaScriptResolution};
    use blueice_bluets::{AuthorizedModule, AuthorizedModuleLoader, AuthorizedModuleResolution};
    use blueice_launcher::bluejs_host::{
        bind_bluejs_host_socket, serve_bluejs_host_listener, BlueJsChildHost,
    };
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
            Some(DebuggerCapabilityState::Planned)
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
        assert_eq!(
            executor.debugger_programs(tab_id, 1),
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
