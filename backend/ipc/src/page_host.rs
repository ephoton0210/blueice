// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Private, versioned transport between `blueice-launcher` and its isolated
//! BlueJS page host child.
//!
//! This is intentionally neither the public frontend protocol nor the narrow
//! script-to-DOM channel. A launcher creates one private Unix socket and a
//! one-time capability token for one child, then a core-owned page loader may
//! supply already-authorized source records through that connection. The
//! child never receives a filesystem path, URL to fetch, DOM handle, network
//! authority, or a resolver callback. It receives only complete source graphs
//! selected by its caller and reports only bounded, source-free outcomes.
//!
//! Version 11 retains the two fixed, core-derived document snapshots consumed
//! by the child-owned JavaScript bindings, the location-only debugger
//! inventory, a bounded exact-breakpoint configuration table, and an opt-in
//! root-classic continuation seam. The
//! transport has no profile or capability
//! selector: every accepted document contains exactly the immutable text and
//! canonical-origin copies selected by core.
//! BlueTS stays a child-fixed, direct-lowering profile with no ambient host
//! typings, compiler option, resolver, or emitted JavaScript crossing this
//! channel. Apart from the two fixed JavaScript primitive snapshot callbacks,
//! version 10 exposes only a core-proxied, source-free debugger location
//! inventory and configuration records. A core-selected document may opt in
//! to the one-shot root-classic arm/state/resume lifecycle; the child admits
//! no generic interruption, stepping, nested continuation, stack, scope,
//! bytecode, source, runtime value transport, general host callback,
//! fetch/cache, or client-facing API. Version 7 additionally lets that
//! authenticated core enumerate one separately minted opaque static-metadata
//! handle for an exact BlueTS program. The handle discloses neither static
//! metadata nor a child program identity and is unusable after its realm is
//! replaced or closed. Version 8 adds only an explicitly requested,
//! handle-bound static summary: fixed compiler fingerprints and aggregate
//! counts, never a source identity/text, span, name, type display, symbol,
//! contract, bytecode, runtime value, or dereferenceable metadata record.
//! Version 9 adds only bounded compiler-minted source-record IDs for that
//! same handle; IDs carry no source identity, hash, text, or record detail.
//! Version 10 adds the separately authorized source-provenance reply for one
//! of those IDs: canonical module identity and a labeled SHA-256 digest only,
//! never source text or a general static-record read.
//! Version 11 adds only a parent-handle-bound compiler type-ID inventory.
//! Version 12 adds one exact type-ID display lookup; it remains an explicit
//! core-proxied operation, not a general static-record read or source access.
//! Version 13 adds a payload-free parent-handle-bound symbol-ID inventory.

use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

/// Independent version for the private launcher-to-BlueJS-host channel.
pub const PAGE_HOST_PROTOCOL_VERSION: u32 = 13;

/// Maximum private page-host request/reply frame. The child rejects a length
/// above this cap before allocating a payload buffer or deserializing source.
/// This is separate from per-document source budgets enforced by the child.
pub const PAGE_HOST_MAX_FRAME_BYTES: usize = 12 * 1024 * 1024;

/// The exact string budgets for the two fixed core-to-child document
/// snapshots. `blueice-engine` uses the same values in its pure host-binding
/// contract inventory before it serializes a document; the child repeats the
/// byte checks before it replaces a realm. There is deliberately no generic
/// binding-value transport with caller-controlled limits.
pub const PAGE_HOST_DOCUMENT_TEXT_MAX_BYTES: usize = 1_048_576;
pub const PAGE_HOST_DOCUMENT_ORIGIN_MAX_BYTES: usize = 4 * 1_024;

/// Maximum exact instruction boundaries returned for one child-owned program
/// by the private debugger-location inventory. It is an immutable child
/// policy, not a client-provided request limit.
pub const PAGE_HOST_DEBUGGER_MAX_SAFE_POINTS_PER_PROGRAM: u32 = 4_096;

/// Maximum exact breakpoint configuration records retained for one child
/// realm. The cap is fixed by this private protocol; neither the public
/// debugger nor page code can grow the child table without bound.
pub const PAGE_HOST_DEBUGGER_MAX_BREAKPOINTS_PER_REALM: u32 = 256;

/// A document accepts at most 256 declarations and each declaration can
/// carry at most eight closed graph modules. The accounting reply cannot name
/// more live BlueJS programs than that fixed document envelope permits.
pub const PAGE_HOST_REALM_STATS_MAX_PROGRAMS: u32 = 2_048;

/// An opaque debugger program identity minted by the isolated child. It is
/// valid only with the exact tab/document generation supplied by the request;
/// it deliberately contains no source identity, BlueJS registry handle, or
/// bytecode data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PageHostDebuggerProgram {
    pub program_handle: u64,
    pub program_generation: u64,
}

impl PageHostDebuggerProgram {
    /// Private debugger identities never use zero placeholders.
    pub fn is_well_formed(self) -> bool {
        self.program_handle != 0 && self.program_generation != 0
    }
}

/// A child-minted opaque association with static BlueTS metadata for one
/// exact live direct-program generation. It is deliberately a different
/// identity namespace from [`PageHostDebuggerProgram`]: callers cannot reuse
/// a program ID as a metadata ID, nor derive source/module/type/span/contract
/// information from either field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PageHostDebuggerMetadataHandle {
    pub metadata_handle: u64,
    pub metadata_generation: u64,
}

impl PageHostDebuggerMetadataHandle {
    /// Private metadata identities never use zero placeholders.
    pub fn is_well_formed(self) -> bool {
        self.metadata_handle != 0 && self.metadata_generation != 0
    }
}

/// A bounded source-free description of a live BlueTS debug attachment.
/// This private transport structure intentionally has no child program or
/// metadata handle: the enclosing request/reply supplies those opaque
/// identities and core verifies every component before reminting the public
/// summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostDebuggerBlueTsMetadataSummary {
    pub language_version: String,
    pub compiler_options_hash: String,
    pub source_count: u32,
    pub type_count: u32,
    pub symbol_count: u32,
    pub contract_count: u32,
}

/// One compiler-minted source-record ID for an exact private BlueTS metadata
/// attachment. It deliberately carries no module identity, source text,
/// content hash, span, symbol, type, contract, bytecode, VM object, or value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PageHostDebuggerBlueTsMetadataSourceId {
    pub source_id: u32,
}

/// One compiler-minted type-record ID for an exact private BlueTS metadata
/// attachment. It deliberately carries no display string, source identity,
/// span, symbol, contract, bytecode, VM object, or value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PageHostDebuggerBlueTsMetadataTypeId {
    pub type_id: u32,
}

/// One compiler-minted symbol-record ID for an exact private BlueTS metadata
/// attachment. It deliberately carries no name, source span, declared type,
/// contract, bytecode, VM object, or value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PageHostDebuggerBlueTsMetadataSymbolId {
    pub symbol_id: u32,
}

/// One child-local compiler-produced type display for an exact type ID under
/// an opaque metadata attachment. The enclosing request/reply carries the
/// child program and metadata identities; this value never grants a generic
/// static-record read or source access.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostDebuggerBlueTsMetadataTypeDisplay {
    pub type_id: u32,
    pub display: String,
}

/// One child-local, source-text-free provenance description for a source ID
/// under an exact private metadata attachment. Core validates and remints the
/// enclosing public identities; this value never grants source access.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostDebuggerBlueTsMetadataSourceProvenance {
    pub source_id: u32,
    pub module: String,
    pub content_hash: String,
}

/// One exact compiler-verified instruction boundary returned without source
/// text or bytecode. The child validates this complete tuple; it never maps a
/// caller-supplied nearest offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PageHostDebuggerSafePoint {
    pub program: PageHostDebuggerProgram,
    pub code_unit_ordinal: u32,
    pub bytecode_offset: u32,
}

/// Source-free lifecycle state for the one-shot root-classic continuation
/// seam. A `Paused` location is always an exact child-validated root safe
/// point; no frame, scope, runtime value, source, or bytecode is serialized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PageHostDebuggerExecutionState {
    Pending,
    Paused {
        safe_point: PageHostDebuggerSafePoint,
    },
    Resuming,
    Completed,
}

impl PageHostDebuggerSafePoint {
    /// The nested opaque program identity is required for every location.
    pub fn is_well_formed(self) -> bool {
        self.program.is_well_formed()
    }
}

/// A complete source record selected and fingerprinted by the caller-owned
/// page loader. `source_hash` is verified by the child against `source`; it
/// is never a client-supplied assertion that the child blindly trusts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostSource {
    pub canonical_module_id: String,
    pub source: String,
    pub source_hash: String,
}

impl PageHostSource {
    /// Creates a source record with the v1 deterministic FNV-1a content
    /// fingerprint. This detects accidental record mismatches and gives the
    /// program registry a stable identity; it is not cryptographic integrity
    /// and does not replace the caller's integrity, CSP, cache, or origin
    /// policy.
    pub fn new(canonical_module_id: impl Into<String>, source: impl Into<String>) -> Self {
        let source = source.into();
        Self {
            canonical_module_id: canonical_module_id.into(),
            source_hash: source_hash(&source),
            source,
        }
    }
}

/// One caller-authorized static import/export resolution. The child validates
/// this exact record against every static request; it never performs relative
/// URL, import-map, filesystem, package-manager, or network resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostStaticResolution {
    pub from_module: String,
    pub specifier: String,
    pub canonical_target: String,
}

/// A closed, caller-authorized source graph for one page declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostModuleGraph {
    pub entry: String,
    pub modules: Vec<PageHostSource>,
    pub resolutions: Vec<PageHostStaticResolution>,
    /// Caller-owned identity for the origin/integrity/cache resolver policy.
    /// It is recorded only as an opaque non-empty fingerprint in this v1
    /// transport; the child does not interpret it or gain the resolver.
    pub resolver_fingerprint: String,
}

/// Script grammar selected by the trusted page pipeline, never by a filename
/// suffix or the child host's own MIME sniffing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PageHostScriptKind {
    Classic,
    Module,
}

/// The source language selected by the trusted page pipeline. An ordinary
/// JavaScript declaration never becomes BlueTS merely because its source is
/// syntactically accepted by the compiler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PageHostScriptLanguage {
    JavaScript,
    BlueTs,
}

/// One declaration in document order. Its graph is fully supplied before the
/// child parses or executes anything.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostScript {
    pub ordinal: u32,
    pub language: PageHostScriptLanguage,
    pub kind: PageHostScriptKind,
    pub graph: PageHostModuleGraph,
}

/// The only core-to-child values made available to ordinary JavaScript page
/// code. They are copied primitive snapshots, not DOM handles, URL objects,
/// resolver capabilities, or a page-selected binding profile.
///
/// `document_origin` must be the canonical HTTP(S) tuple origin. The child
/// checks its canonical spelling before using it as either realm identity or
/// callback result; a missing/empty or over-budget snapshot fails closed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostDocumentSnapshot {
    pub document_text: String,
    pub document_origin: String,
}

/// One caller-authorized document snapshot. `snapshot` is supplied only by
/// the core-owned page lifecycle adapter after it has validated the live DOM
/// text and canonical origin. A page cannot add, remove, rename, or widen
/// these bindings through this private protocol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostDocument {
    pub tab_id: u64,
    pub document_generation: u64,
    pub snapshot: PageHostDocumentSnapshot,
    /// Set only by the authenticated core lifecycle owner. When true, the
    /// child holds document-order declarations until its next explicit
    /// advance turn so core can inspect and arm an exact root-classic safe
    /// point. Page content and frontend IPC cannot select this mode.
    pub debugger_execution_control: bool,
    pub scripts: Vec<PageHostScript>,
}

/// Source-free outcome for one attempted declaration. These records are safe
/// to return to the launcher/core but are not a runtime-value or diagnostic
/// transport.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostScriptReport {
    pub tab_id: u64,
    pub document_generation: u64,
    pub ordinal: u32,
    pub language: PageHostScriptLanguage,
    pub kind: PageHostScriptKind,
    pub outcome: PageHostScriptOutcome,
}

/// Bounded outcome category. The child owns these fixed labels; it never
/// forwards parser/compiler/runtime error text or page source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PageHostScriptOutcome {
    Executed,
    Rejected { category: String },
}

/// Safe, aggregate accounting for one live child-owned realm. Heap object
/// identities and bytecode/source remain private to the child.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostRealmStats {
    pub tab_id: u64,
    pub document_generation: u64,
    pub program_count: u32,
    pub bytecode_bytes: u64,
    pub heap_bytes: u64,
}

impl PageHostRealmStats {
    /// Checks the fixed, source-free accounting envelope before core caches a
    /// child report. `u32::MAX`/`u64::MAX` are conversion-failure sentinels in
    /// the child adapter, never credible live accounting values. This does
    /// not disclose the launcher's private resource limits.
    pub fn is_well_formed(&self) -> bool {
        self.tab_id != 0
            && self.document_generation != 0
            && self.program_count <= PAGE_HOST_REALM_STATS_MAX_PROGRAMS
            && self.bytecode_bytes != u64::MAX
            && self.heap_bytes != u64::MAX
    }
}

/// Launcher/core requests to the private host. `Hello` carries the per-spawn
/// secret capability so a same-user process that guesses a socket pathname
/// cannot claim the child before its launcher does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PageHostRequest {
    Hello {
        protocol_version: u32,
        session_token: String,
    },
    /// Applies a new document only if its generation succeeds the tab's
    /// current child-owned generation. Repeating the current generation is
    /// idempotent and never re-runs page code.
    SynchronizeDocument { document: PageHostDocument },
    /// Releases one exact live realm. A stale generation cannot close its
    /// successor after navigation.
    CloseRealm {
        tab_id: u64,
        document_generation: u64,
    },
    /// Returns only bounded, aggregate accounting for one exact live realm.
    GetRealmStats {
        tab_id: u64,
        document_generation: u64,
    },
    /// Lists only opaque program identities retained by one exact live child
    /// realm. This discovery operation cannot pause, resume, inspect, or
    /// mutate that realm.
    ListDebuggerPrograms {
        tab_id: u64,
        document_generation: u64,
    },
    /// Lists separately minted opaque static-metadata handles for one exact
    /// private program. A JavaScript program, a BlueTS program without a live
    /// registry attachment, or an invalidated attachment returns no handles.
    /// The reply contains no static metadata, source/module/name/type/span,
    /// contract, aggregate count, runtime handle, or VM object.
    ListDebuggerBlueTsMetadata {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    },
    /// Describes one previously minted private BlueTS metadata handle. The
    /// request repeats its owning program so the child can reject a handle
    /// from another live program without probing its registry. It returns
    /// only a bounded fingerprint/count summary, never a metadata record.
    DescribeDebuggerBlueTsMetadata {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    },
    /// Lists compiler-minted source-record IDs for a prior exact private
    /// metadata handle. This is not a source/provenance record read.
    ListDebuggerBlueTsMetadataSources {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    },
    /// Lists compiler-minted type-record IDs for a prior exact private
    /// metadata handle. This is not a type display or static-record read.
    ListDebuggerBlueTsMetadataTypes {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    },
    /// Describes one compiler-minted type ID under a prior exact private
    /// metadata handle. It returns one bounded display only; source text,
    /// spans, symbols, contracts, bytecode, VM objects, and values remain in
    /// the child.
    DescribeDebuggerBlueTsMetadataType {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        type_id: u32,
    },
    /// Lists compiler-minted symbol-record IDs for a prior exact private
    /// metadata handle. This is not a symbol-name, span, type, or record read.
    ListDebuggerBlueTsMetadataSymbols {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    },
    /// Describes exactly one prior compiler-minted source ID. This private
    /// request returns module identity and a digest only, never source text.
    DescribeDebuggerBlueTsMetadataSource {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        source_id: u32,
    },
    /// Lists the child's bounded compiler-verified safe points for one exact
    /// opaque program. No source, bytecode, VM, or value crosses this channel.
    ListDebuggerSafePoints {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    },
    /// Revalidates one exact source-free child safe-point tuple. It does not
    /// execute, pause, or otherwise alter the child VM.
    ValidateDebuggerSafePoint {
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    },
    /// Stores one exact compiler-verified safe point in the child-owned
    /// configuration table. It neither executes nor interrupts a realm.
    SetDebuggerBreakpoint {
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    },
    /// Lists the exact child-private breakpoint records for one live realm.
    /// The core re-mints every program identity before returning it publicly.
    ListDebuggerBreakpoints {
        tab_id: u64,
        document_generation: u64,
    },
    /// Removes one exact compiler-verified child-private breakpoint record.
    /// Removal is idempotent, but the tuple must remain valid for the current
    /// realm rather than naming a successor or nearest instruction boundary.
    ClearDebuggerBreakpoint {
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    },
    /// Arms one exact root-code-unit location for a classic declaration that
    /// remains pending in a core-selected execution-control document. This is
    /// distinct from breakpoint configuration: it starts execution only on a
    /// later explicit advance turn and can retain one root-frame continuation.
    ArmDebuggerRootSafePointBreakpoint {
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    },
    /// Reads only source-free lifecycle state for one exact child-private
    /// program generation in the opt-in root-classic continuation seam.
    GetDebuggerExecutionState {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    },
    /// Marks one paused root-classic continuation for execution on the next
    /// core-owned advance turn. It cannot inject a value or exception.
    ResumeDebuggerExecution {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    },
    /// Advances one exact opted-in realm in document order. It returns only
    /// fixed execution categories and never exposes a completion value.
    AdvanceDebuggerExecution {
        tab_id: u64,
        document_generation: u64,
    },
    /// Ends the child process after its acknowledgement.
    Shutdown,
    /// A newer request must not be interpreted as an existing operation.
    #[serde(other)]
    Unknown,
}

/// Child replies for [`PageHostRequest`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PageHostReply {
    HelloAck {
        protocol_version: u32,
    },
    Synchronized {
        tab_id: u64,
        document_generation: u64,
        /// `true` when the child already owned this exact document and did
        /// not execute any declaration again.
        already_current: bool,
        reports: Vec<PageHostScriptReport>,
    },
    RealmClosed {
        tab_id: u64,
        document_generation: u64,
    },
    RealmStats(PageHostRealmStats),
    DebuggerPrograms {
        tab_id: u64,
        document_generation: u64,
        programs: Vec<PageHostDebuggerProgram>,
    },
    /// A bounded inventory of child-minted opaque static BlueTS metadata
    /// associations for one exact private program. This is a handle-discovery
    /// operation only; the handle itself carries no metadata payload.
    DebuggerBlueTsMetadata {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: Vec<PageHostDebuggerMetadataHandle>,
    },
    /// Bounded source-free summary for one exact private metadata handle.
    /// It is valid only while the matching child realm, program, and retained
    /// BlueTS registry attachment remain live.
    DebuggerBlueTsMetadataSummary {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        summary: PageHostDebuggerBlueTsMetadataSummary,
    },
    DebuggerBlueTsMetadataSources {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        sources: Vec<PageHostDebuggerBlueTsMetadataSourceId>,
    },
    /// Bounded compiler-minted static type identities. The IDs are local to
    /// the exact metadata attachment and carry no type display or record
    /// payload.
    DebuggerBlueTsMetadataTypes {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        types: Vec<PageHostDebuggerBlueTsMetadataTypeId>,
    },
    /// One bounded child-local compiler-produced type display under the exact
    /// opaque metadata attachment.
    DebuggerBlueTsMetadataType {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        static_type: PageHostDebuggerBlueTsMetadataTypeDisplay,
    },
    /// Bounded compiler-minted symbol identities. The IDs are local to the
    /// exact metadata attachment and carry no symbol-record payload.
    DebuggerBlueTsMetadataSymbols {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        symbols: Vec<PageHostDebuggerBlueTsMetadataSymbolId>,
    },
    DebuggerBlueTsMetadataSourceProvenance {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        provenance: PageHostDebuggerBlueTsMetadataSourceProvenance,
    },
    DebuggerSafePoints {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        safe_points: Vec<PageHostDebuggerSafePoint>,
    },
    DebuggerSafePointValidated {
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    },
    DebuggerBreakpointSet {
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    },
    DebuggerBreakpoints {
        tab_id: u64,
        document_generation: u64,
        safe_points: Vec<PageHostDebuggerSafePoint>,
    },
    DebuggerBreakpointCleared {
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
        was_present: bool,
    },
    DebuggerRootSafePointBreakpointArmed {
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    },
    DebuggerExecutionState {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        state: PageHostDebuggerExecutionState,
    },
    DebuggerExecutionResumed {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    },
    DebuggerExecutionAdvanced {
        tab_id: u64,
        document_generation: u64,
        reports: Vec<PageHostScriptReport>,
    },
    ShutdownAck,
    Error {
        code: PageHostErrorCode,
        /// Fixed host-owned prose only; callers branch on `code`.
        message: String,
    },
}

/// Stable transport/lifecycle failure categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PageHostErrorCode {
    ProtocolVersion,
    Authentication,
    InvalidRequest,
    StaleDocument,
    UnknownRealm,
    ResourceLimit,
    HostFailure,
    /// The exact request is structurally valid but not eligible for the
    /// bounded root-classic lifecycle in this document/program state.
    InvalidDebuggerState,
}

/// Builds the only valid first reply. The caller must check this before it
/// dispatches a request to a live realm owner.
pub fn negotiate(request: &PageHostRequest, expected_session_token: &str) -> PageHostReply {
    match request {
        PageHostRequest::Hello {
            protocol_version,
            session_token,
        } if *protocol_version != PAGE_HOST_PROTOCOL_VERSION => PageHostReply::Error {
            code: PageHostErrorCode::ProtocolVersion,
            message: "unsupported BlueJS page-host protocol version".to_string(),
        },
        PageHostRequest::Hello { session_token, .. } if session_token != expected_session_token => {
            PageHostReply::Error {
                code: PageHostErrorCode::Authentication,
                message: "BlueJS page-host session capability was rejected".to_string(),
            }
        }
        PageHostRequest::Hello { .. } => PageHostReply::HelloAck {
            protocol_version: PAGE_HOST_PROTOCOL_VERSION,
        },
        _ => PageHostReply::Error {
            code: PageHostErrorCode::ProtocolVersion,
            message: "BlueJS page-host protocol requires Hello as its first request".to_string(),
        },
    }
}

/// Writes one length-prefixed request frame.
pub fn write_page_host_request<W: Write>(
    writer: &mut W,
    request: &PageHostRequest,
) -> io::Result<()> {
    crate::write_framed(writer, request)
}

/// Reads one length-prefixed request frame.
pub fn read_page_host_request<R: Read>(reader: &mut R) -> io::Result<PageHostRequest> {
    let bytes = crate::read_frame_bytes_with_limit(reader, PAGE_HOST_MAX_FRAME_BYTES)?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

/// Writes one length-prefixed child reply frame.
pub fn write_page_host_reply<W: Write>(writer: &mut W, reply: &PageHostReply) -> io::Result<()> {
    crate::write_framed(writer, reply)
}

/// Reads one length-prefixed child reply frame.
pub fn read_page_host_reply<R: Read>(reader: &mut R) -> io::Result<PageHostReply> {
    let bytes = crate::read_frame_bytes_with_limit(reader, PAGE_HOST_MAX_FRAME_BYTES)?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

/// The deterministic v1 source-content fingerprint shared by loader and
/// child. It detects accidental transport substitution but is not a
/// cryptographic integrity check; the caller's loader must enforce any
/// cryptographic integrity policy before it authorizes a record.
pub fn source_hash(source: &str) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in source.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("fnv1a64:{hash:016x}")
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;

    fn source() -> PageHostSource {
        PageHostSource::new("blueice://page/main.js", "globalThis.answer = 42;")
    }

    fn document() -> PageHostDocument {
        PageHostDocument {
            tab_id: 7,
            document_generation: 3,
            snapshot: PageHostDocumentSnapshot {
                document_text: "snapshot text".to_string(),
                document_origin: "https://example.test".to_string(),
            },
            debugger_execution_control: true,
            scripts: vec![PageHostScript {
                ordinal: 0,
                language: PageHostScriptLanguage::JavaScript,
                kind: PageHostScriptKind::Classic,
                graph: PageHostModuleGraph {
                    entry: "blueice://page/main.js".to_string(),
                    modules: vec![source()],
                    resolutions: vec![],
                    resolver_fingerprint: "core-loader-v1".to_string(),
                },
            }],
        }
    }

    #[test]
    fn requests_and_replies_round_trip_over_a_real_socket() {
        let requests = [
            PageHostRequest::Hello {
                protocol_version: PAGE_HOST_PROTOCOL_VERSION,
                session_token: "not-a-real-token".to_string(),
            },
            PageHostRequest::SynchronizeDocument {
                document: document(),
            },
            PageHostRequest::CloseRealm {
                tab_id: 7,
                document_generation: 3,
            },
            PageHostRequest::GetRealmStats {
                tab_id: 7,
                document_generation: 3,
            },
            PageHostRequest::ListDebuggerPrograms {
                tab_id: 7,
                document_generation: 3,
            },
            PageHostRequest::ListDebuggerBlueTsMetadata {
                tab_id: 7,
                document_generation: 3,
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
            },
            PageHostRequest::DescribeDebuggerBlueTsMetadata {
                tab_id: 7,
                document_generation: 3,
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
                metadata: PageHostDebuggerMetadataHandle {
                    metadata_handle: 17,
                    metadata_generation: 19,
                },
            },
            PageHostRequest::ListDebuggerBlueTsMetadataSources {
                tab_id: 7,
                document_generation: 3,
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
                metadata: PageHostDebuggerMetadataHandle {
                    metadata_handle: 17,
                    metadata_generation: 19,
                },
            },
            PageHostRequest::ListDebuggerBlueTsMetadataTypes {
                tab_id: 7,
                document_generation: 3,
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
                metadata: PageHostDebuggerMetadataHandle {
                    metadata_handle: 17,
                    metadata_generation: 19,
                },
            },
            PageHostRequest::DescribeDebuggerBlueTsMetadataType {
                tab_id: 7,
                document_generation: 3,
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
                metadata: PageHostDebuggerMetadataHandle {
                    metadata_handle: 17,
                    metadata_generation: 19,
                },
                type_id: 0,
            },
            PageHostRequest::ListDebuggerBlueTsMetadataSymbols {
                tab_id: 7,
                document_generation: 3,
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
                metadata: PageHostDebuggerMetadataHandle {
                    metadata_handle: 17,
                    metadata_generation: 19,
                },
            },
            PageHostRequest::ListDebuggerSafePoints {
                tab_id: 7,
                document_generation: 3,
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
            },
            PageHostRequest::ValidateDebuggerSafePoint {
                tab_id: 7,
                document_generation: 3,
                safe_point: PageHostDebuggerSafePoint {
                    program: PageHostDebuggerProgram {
                        program_handle: 11,
                        program_generation: 13,
                    },
                    code_unit_ordinal: 0,
                    bytecode_offset: 4,
                },
            },
            PageHostRequest::SetDebuggerBreakpoint {
                tab_id: 7,
                document_generation: 3,
                safe_point: PageHostDebuggerSafePoint {
                    program: PageHostDebuggerProgram {
                        program_handle: 11,
                        program_generation: 13,
                    },
                    code_unit_ordinal: 0,
                    bytecode_offset: 4,
                },
            },
            PageHostRequest::ListDebuggerBreakpoints {
                tab_id: 7,
                document_generation: 3,
            },
            PageHostRequest::ClearDebuggerBreakpoint {
                tab_id: 7,
                document_generation: 3,
                safe_point: PageHostDebuggerSafePoint {
                    program: PageHostDebuggerProgram {
                        program_handle: 11,
                        program_generation: 13,
                    },
                    code_unit_ordinal: 0,
                    bytecode_offset: 4,
                },
            },
            PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
                tab_id: 7,
                document_generation: 3,
                safe_point: PageHostDebuggerSafePoint {
                    program: PageHostDebuggerProgram {
                        program_handle: 11,
                        program_generation: 13,
                    },
                    code_unit_ordinal: 0,
                    bytecode_offset: 4,
                },
            },
            PageHostRequest::GetDebuggerExecutionState {
                tab_id: 7,
                document_generation: 3,
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
            },
            PageHostRequest::ResumeDebuggerExecution {
                tab_id: 7,
                document_generation: 3,
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
            },
            PageHostRequest::AdvanceDebuggerExecution {
                tab_id: 7,
                document_generation: 3,
            },
            PageHostRequest::Shutdown,
            PageHostRequest::Unknown,
        ];
        for request in requests {
            let (mut writer, mut reader) = UnixStream::pair().unwrap();
            write_page_host_request(&mut writer, &request).unwrap();
            assert_eq!(read_page_host_request(&mut reader).unwrap(), request);
        }

        let reply = PageHostReply::Synchronized {
            tab_id: 7,
            document_generation: 3,
            already_current: false,
            reports: vec![PageHostScriptReport {
                tab_id: 7,
                document_generation: 3,
                ordinal: 0,
                language: PageHostScriptLanguage::JavaScript,
                kind: PageHostScriptKind::Classic,
                outcome: PageHostScriptOutcome::Executed,
            }],
        };
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_reply(&mut writer, &reply).unwrap();
        assert_eq!(read_page_host_reply(&mut reader).unwrap(), reply);

        let debugger_reply = PageHostReply::DebuggerBreakpointCleared {
            tab_id: 7,
            document_generation: 3,
            safe_point: PageHostDebuggerSafePoint {
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
                code_unit_ordinal: 0,
                bytecode_offset: 4,
            },
            was_present: true,
        };
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_reply(&mut writer, &debugger_reply).unwrap();
        assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

        let debugger_reply = PageHostReply::DebuggerBlueTsMetadata {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            metadata: vec![PageHostDebuggerMetadataHandle {
                metadata_handle: 1 << 63,
                metadata_generation: 1 << 63,
            }],
        };
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_reply(&mut writer, &debugger_reply).unwrap();
        assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

        let debugger_reply = PageHostReply::DebuggerBlueTsMetadataSummary {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: 17,
                metadata_generation: 19,
            },
            summary: PageHostDebuggerBlueTsMetadataSummary {
                language_version: "blue-ts-0.1".to_string(),
                compiler_options_hash: "0123456789abcdef".to_string(),
                source_count: 1,
                type_count: 2,
                symbol_count: 3,
                contract_count: 4,
            },
        };
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_reply(&mut writer, &debugger_reply).unwrap();
        assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

        let debugger_reply = PageHostReply::DebuggerBlueTsMetadataSources {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: 17,
                metadata_generation: 19,
            },
            sources: vec![PageHostDebuggerBlueTsMetadataSourceId { source_id: 0 }],
        };
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_reply(&mut writer, &debugger_reply).unwrap();
        assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

        let debugger_reply = PageHostReply::DebuggerBlueTsMetadataTypes {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: 17,
                metadata_generation: 19,
            },
            types: vec![PageHostDebuggerBlueTsMetadataTypeId { type_id: 0 }],
        };
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_reply(&mut writer, &debugger_reply).unwrap();
        assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

        let debugger_reply = PageHostReply::DebuggerBlueTsMetadataSymbols {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: 17,
                metadata_generation: 19,
            },
            symbols: vec![PageHostDebuggerBlueTsMetadataSymbolId { symbol_id: 0 }],
        };
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_reply(&mut writer, &debugger_reply).unwrap();
        assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

        let debugger_reply = PageHostReply::DebuggerExecutionState {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            state: PageHostDebuggerExecutionState::Paused {
                safe_point: PageHostDebuggerSafePoint {
                    program: PageHostDebuggerProgram {
                        program_handle: 11,
                        program_generation: 13,
                    },
                    code_unit_ordinal: 0,
                    bytecode_offset: 4,
                },
            },
        };
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_reply(&mut writer, &debugger_reply).unwrap();
        assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);
    }

    #[test]
    fn handshake_requires_the_exact_version_and_capability() {
        let token = "launcher-secret";
        assert_eq!(
            negotiate(
                &PageHostRequest::Hello {
                    protocol_version: PAGE_HOST_PROTOCOL_VERSION,
                    session_token: token.to_string(),
                },
                token,
            ),
            PageHostReply::HelloAck {
                protocol_version: PAGE_HOST_PROTOCOL_VERSION,
            }
        );
        assert!(matches!(
            negotiate(
                &PageHostRequest::Hello {
                    protocol_version: PAGE_HOST_PROTOCOL_VERSION - 1,
                    session_token: token.to_string(),
                },
                token,
            ),
            PageHostReply::Error {
                code: PageHostErrorCode::ProtocolVersion,
                ..
            }
        ));
        assert!(matches!(
            negotiate(
                &PageHostRequest::Hello {
                    protocol_version: PAGE_HOST_PROTOCOL_VERSION + 1,
                    session_token: token.to_string(),
                },
                token,
            ),
            PageHostReply::Error {
                code: PageHostErrorCode::ProtocolVersion,
                ..
            }
        ));
        assert!(matches!(
            negotiate(
                &PageHostRequest::Hello {
                    protocol_version: PAGE_HOST_PROTOCOL_VERSION,
                    session_token: "wrong".to_string(),
                },
                token,
            ),
            PageHostReply::Error {
                code: PageHostErrorCode::Authentication,
                ..
            }
        ));
        assert!(matches!(
            negotiate(
                &PageHostRequest::SynchronizeDocument {
                    document: document(),
                },
                token,
            ),
            PageHostReply::Error {
                code: PageHostErrorCode::ProtocolVersion,
                ..
            }
        ));
    }

    #[test]
    fn source_constructor_fingerprints_exact_bytes() {
        let source = PageHostSource::new("blueice://page/main.js", "let answer = 42;");
        assert_eq!(source.source_hash, source_hash(&source.source));
        assert_ne!(source.source_hash, source_hash("let answer = 43;"));
    }

    #[test]
    fn realm_stats_reject_conversion_sentinels_and_program_overflow() {
        let stats = PageHostRealmStats {
            tab_id: 7,
            document_generation: 3,
            program_count: 2,
            bytecode_bytes: 64,
            heap_bytes: 128,
        };
        assert!(stats.is_well_formed());
        assert!(!PageHostRealmStats {
            tab_id: 0,
            ..stats.clone()
        }
        .is_well_formed());
        assert!(!PageHostRealmStats {
            program_count: PAGE_HOST_REALM_STATS_MAX_PROGRAMS + 1,
            ..stats.clone()
        }
        .is_well_formed());
        assert!(!PageHostRealmStats {
            bytecode_bytes: u64::MAX,
            ..stats.clone()
        }
        .is_well_formed());
        assert!(!PageHostRealmStats {
            heap_bytes: u64::MAX,
            ..stats
        }
        .is_well_formed());
    }

    #[test]
    fn page_host_rejects_an_oversized_frame_before_payload_allocation() {
        let oversized = u32::try_from(PAGE_HOST_MAX_FRAME_BYTES + 1).unwrap();
        let mut bytes = oversized.to_le_bytes().to_vec();
        assert!(read_page_host_request(&mut std::io::Cursor::new(&mut bytes)).is_err());
    }

    #[test]
    fn version_five_document_requires_the_fixed_core_snapshot() {
        let mut value = serde_json::to_value(PageHostRequest::SynchronizeDocument {
            document: document(),
        })
        .unwrap();
        value["SynchronizeDocument"]["document"]
            .as_object_mut()
            .unwrap()
            .remove("snapshot");
        assert!(serde_json::from_value::<PageHostRequest>(value).is_err());
    }
}
