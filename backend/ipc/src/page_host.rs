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
//! Version 41 adds a child-private, static-only paused scope-symbol/type
//! relation target for ordinary and linked stacks. It does not add a public
//! debugger grant or a runtime value read. Version 40 adds the linked-module
//! debugger family. Version 39 adds a source-free, exact terminal BlueTS
//! exception-location route after version 38's paused-slot, bounded plain-data value preview.
//! It is child-private and does not itself grant a public debugger client
//! value access. Version 37 adds a bounded source-free stack/scope snapshot from an exact
//! paused root or nested frame. It remains private and does not enable a
//! public debugger capability. Version 36 adds exact active nested-frame resume without reusing root
//! resume or single-instruction stepping. Version 35 adds nested-frame arm, state, and single-instruction
//! step controls for the private core/child route. The frame identity binds
//! tab, document, program, code unit, and invocation; it grants no source or
//! value inspection and is distinct from a static safe-point location.
//! Version 31 adds an authenticated, source-free child-wide actual-usage
//! snapshot for all live realms. It is private to the core/child connection,
//! separate from the launcher's conservative reservations and from public
//! debugger, frontend, and MCP protocols. Version 30 adds a child-private
//! BlueTS source-span step on a retained classic-root continuation. It
//! advances one verified root instruction per
//! owner turn, stops at a different compiler-bound source span, completion,
//! or an explicit fixed instruction-budget yield. It reveals no source text,
//! runtime value, stack, or VM frame. Public debugger v30 separately gates its
//! use with owner/client capability grants and same-stream source receipts.
//! Version 29 adds a child-private, bounded BlueTS byte-position-to-safe-point
//! binding under an exact live metadata attachment. It preserves explicitly
//! unbound lowering spans and does not itself grant a public debugger client
//! an arbitrary source-position query or execution control.
//! Version 28 adds a child-private exact safe-point-to-BlueTS byte-span lookup
//! under one live opaque metadata attachment. It returns only a compiler
//! source ID and bounded original byte range, never source text, module
//! identity, a guessed nearest span, or a public debugger capability.
//! Version 27 adds an exact-program, source-free single-root-instruction
//! debugger step and a transient `Stepping` state for the isolated child.
//! Version 26 adds bounded compiler-produced original-source UTF-16
//! coordinates to the existing child-private symbol and contract location
//! replies. Version 25 adds the compiler's export classification to the existing
//! child-private symbol display. Version 24 adds a fixed root-shape
//! classification to the existing contract-display reply for an exact
//! child-private metadata attachment.
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
//! to the root-classic arm/state/resume lifecycle and, since v27, one
//! root-instruction step per explicit request; the child admits no generic
//! interruption, nested-frame stepping, stack, scope,
//! bytecode, source-text, runtime-value transport, general host callback,
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
//! Version 14 adds a payload-free parent-handle-bound contract-ID inventory.
//! Version 15 adds one exact symbol-ID display lookup; it remains an explicit
//! core-proxied operation, not a general static-record read or source access.
//! Version 16 adds one exact contract-ID display lookup; it remains an explicit
//! core-proxied operation, not a general static-record read or source access.
//! Version 23 adds an exact contract/source declaration range without a
//! contract plan, source text, module identity, or runtime value.
//! Version 22 adds the compiler declaration kind to the exact child-local
//! symbol display. It does not add a target, source read, or runtime access.
//! Version 21 adds an exact symbol-to-reifiable-contract verification under
//! one private metadata attachment. It repeats only two caller-supplied
//! opaque IDs and does not disclose the contract plan or static record.
//! Version 20 adds an exact symbol-to-static-type verification under one
//! private metadata attachment. It repeats only two caller-supplied opaque
//! IDs and does not disclose type displays or static records. Version 19 adds
//! a separately requested, opaque-handle-bound symbol
//! location. It carries only a receipted source ID and bounded half-open byte
//! range, never source text, module identity, line/column data, name, type,
//! contract, bytecode, VM object, or value. Version 18 adds a separately
//! requested, opaque-handle-bound summary of the
//! verified BlueTS-to-BlueJS lowering map. It carries only fixed ABI labels,
//! a source-set fingerprint, and an aggregate bound-entry count; source
//! identities/spans, map entries, AST nodes, and bytecode offsets remain
//! private. Version 17 adds a separately requested, data-only validation against one
//! exact prior contract ID. Its reply is only a boolean, never the input,
//! contract plan, or structural failure detail.

use crate::compiler::CompilerContractValue;
use crate::debugger::{
    DebuggerSourceCoordinates, DebuggerStaticMetadataContractRootKind,
    DebuggerStaticMetadataSymbolKind,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::{self, Read, Write};

/// Independent version for the private launcher-to-BlueJS-host channel.
/// V41 adds the private static scope-symbol/type relation wire. V40 adds the
/// complete linked-module private frame, stack, source-span, arm, and resume
/// family. The public debugger wire remains independently versioned.
pub const PAGE_HOST_PROTOCOL_VERSION: u32 = 41;

pub const PAGE_HOST_DEBUGGER_MAX_STACK_FRAMES: u32 = 64;
pub const PAGE_HOST_DEBUGGER_MAX_SCOPE_ENTRIES: u32 = 256;
pub const PAGE_HOST_DEBUGGER_MAX_VALUE_DEPTH: u32 = 4;
pub const PAGE_HOST_DEBUGGER_MAX_VALUE_CONTAINER_LENGTH: usize = 32;
pub const PAGE_HOST_DEBUGGER_MAX_VALUE_NODES: usize = 256;
pub const PAGE_HOST_DEBUGGER_MAX_VALUE_PAYLOAD_BYTES: usize = 4_096;

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

mod debugger_shapes;
pub use debugger_shapes::*;

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

/// Actual source-free usage across every currently live child-owned realm.
/// This excludes Rust allocation, source/cache copies, registry overhead,
/// process RSS, and other children. It conveys no tab or program identity and
/// is never a public debugger/frontend/MCP reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostChildStats {
    pub realm_count: u32,
    pub program_count: u64,
    pub bytecode_bytes: u64,
    pub heap_bytes: u64,
}

impl PageHostChildStats {
    /// Rejects conversion sentinels, impossible program counts, and a
    /// nonempty charge for an empty child before core trusts the snapshot.
    pub fn is_well_formed(self) -> bool {
        self.realm_count != u32::MAX
            && self.program_count != u64::MAX
            && self.bytecode_bytes != u64::MAX
            && self.heap_bytes != u64::MAX
            && self.program_count
                <= u64::from(self.realm_count) * u64::from(PAGE_HOST_REALM_STATS_MAX_PROGRAMS)
            && (self.program_count != 0 || self.bytecode_bytes == 0)
            && (self.realm_count != 0
                || (self.program_count == 0 && self.bytecode_bytes == 0 && self.heap_bytes == 0))
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
    SynchronizeDocument {
        document: PageHostDocument,
    },
    /// Delivers one core-hit-tested node to the exact live child document.
    /// `node_id` is the child-private script handle for that hit, not its raw
    /// core DOM NodeId. The child returns only whether a listener canceled
    /// default navigation.
    DispatchClick {
        tab_id: u64,
        document_generation: u64,
        node_id: u64,
    },
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
    /// Returns one authenticated child-wide actual-usage snapshot. It has no
    /// caller-selected realm, source, or runtime-object target.
    GetChildStats,
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
    /// Returns only aggregate ABI/fingerprint evidence for the verified direct
    /// lowering map paired with one exact opaque metadata handle. This is not
    /// an entry, source span, AST-node, or bytecode inspection operation.
    DescribeDebuggerBlueTsMetadataLoweringSummary {
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
    /// Lists compiler-minted contract IDs for a prior exact private metadata
    /// handle. This is not a contract-name, span, plan, or validation read.
    ListDebuggerBlueTsMetadataContracts {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    },
    /// Describes a compiler-minted contract ID under a prior exact private
    /// metadata handle. This is not a contract span, plan, validation, or
    /// record read.
    DescribeDebuggerBlueTsMetadataContract {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        contract_id: u32,
    },
    /// Validates a core-forwarded data-only snapshot against one exact prior
    /// contract ID. The child applies fixed limits and returns only a boolean.
    ValidateDebuggerBlueTsMetadataContract {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        contract_id: u32,
        value: CompilerContractValue,
    },
    /// Describes a compiler-minted symbol ID under a prior exact private
    /// metadata handle. This is not a symbol span/type/contract or record read.
    DescribeDebuggerBlueTsMetadataSymbol {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        symbol_id: u32,
    },
    /// Describes one bounded half-open source byte range for a prior exact
    /// symbol ID. It has no source/module/name/type/contract/bytecode payload.
    DescribeDebuggerBlueTsMetadataSymbolLocation {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        symbol_id: u32,
    },
    /// Describes one bounded contract declaration range under the exact
    /// child-local program and metadata attachment; no plan or source bytes.
    DescribeDebuggerBlueTsMetadataContractLocation {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        contract_id: u32,
    },
    /// Resolves one already-verified child safe point to an exact retained
    /// BlueTS lowering span under its live private metadata attachment.
    /// There is no caller-selected source offset or nearest-match behavior.
    DescribeDebuggerBlueTsSafePointSpan {
        tab_id: u64,
        document_generation: u64,
        metadata: PageHostDebuggerMetadataHandle,
        safe_point: PageHostDebuggerSafePoint,
    },
    /// Reads only the terminal uncaught location retained for this exact
    /// private BlueTS program and metadata attachment, never a VM value.
    DescribeDebuggerBlueTsExceptionLocation {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    },
    /// Resolves one bounded original BlueTS byte position in a child-minted
    /// source record to the first lowering span at or after it. A bound reply
    /// names only a compiler-verified safe point; an unbound span or missing
    /// following span returns `None` instead of guessing another instruction.
    ResolveDebuggerBlueTsSourceBreakpoint {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        source_id: u32,
        source_byte: u32,
    },
    /// Verifies exactly one symbol-to-static-type pair under the same private
    /// attachment. The reply does not return an unrequested type ID.
    DescribeDebuggerBlueTsMetadataSymbolType {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        symbol_id: u32,
        type_id: u32,
    },
    /// Verifies exactly one symbol-to-contract pair under the same private
    /// attachment. The reply does not return an unrequested contract ID.
    DescribeDebuggerBlueTsMetadataSymbolContract {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        symbol_id: u32,
        contract_id: u32,
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
    /// Arms a verified child-code-unit target on one still-pending classic
    /// or BlueTS entry-module declaration. It does not itself mint a frame.
    ArmDebuggerNestedSafePointBreakpoint {
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    },
    /// Arms a dependency child under an exact pending BlueTS module entry.
    ArmDebuggerLinkedNestedSafePointBreakpoint {
        tab_id: u64,
        document_generation: u64,
        entry_program: PageHostDebuggerProgram,
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
    /// Schedules one root instruction on the retained classic continuation;
    /// the next advance yields a verified root boundary or completion.
    StepDebuggerRootInstruction {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    },
    /// Steps only the exact currently paused nested invocation on a later
    /// child advance turn; a static safe point is never sufficient.
    StepDebuggerNestedInstruction {
        frame: PageHostDebuggerFrame,
    },
    /// Resumes only the exact retained nested invocation on a later child
    /// advance turn; the waiting root remains separately paused on return.
    ResumeDebuggerNestedExecution {
        frame: PageHostDebuggerFrame,
    },
    ResumeDebuggerLinkedNestedExecution {
        frame: PageHostDebuggerLinkedFrame,
    },
    /// Inspects only the exact paused root (`None`) or the currently active
    /// nested invocation (`Some`). Zero or over-cap limits are invalid.
    GetDebuggerStackSnapshot {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        frame: Option<PageHostDebuggerFrame>,
        max_frames: u32,
        max_scope_entries: u32,
    },
    GetDebuggerLinkedStackSnapshot {
        frame: PageHostDebuggerLinkedFrame,
        max_scope_entries: u32,
    },
    /// Checks the whole expected linked stack and both independently bound
    /// metadata/source pairs before returning any original coordinate.
    DescribeDebuggerLinkedStackSpans {
        frame: PageHostDebuggerLinkedFrame,
        expected_stack: PageHostDebuggerLinkedStackSnapshot,
        sources: [PageHostDebuggerLinkedSource; 2],
    },
    /// Copies only one exact active lexical slot in a paused BlueTS frame.
    /// This private route does not accept an object handle or run JavaScript.
    GetDebuggerValueSnapshot {
        target: PageHostDebuggerValueTarget,
    },
    /// Asks only for a compiler symbol/type relation at one exact paused
    /// root slot. This private wire does not grant a public debugger client.
    DescribeDebuggerStaticScopeRelation {
        target: Box<PageHostDebuggerStaticScopeTarget>,
    },
    /// Requires an exact paused BlueTS classic safe point, live metadata,
    /// and its compiler-minted source ID. The child derives the current span
    /// from its retained map; no source text or caller-selected stop span is
    /// accepted. This private operation does not authorize a public client.
    StepDebuggerBlueTsSourceSpan {
        tab_id: u64,
        document_generation: u64,
        metadata: PageHostDebuggerMetadataHandle,
        source_id: u32,
        safe_point: PageHostDebuggerSafePoint,
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
    ClickDispatched {
        tab_id: u64,
        document_generation: u64,
        default_prevented: bool,
    },
    RealmClosed {
        tab_id: u64,
        document_generation: u64,
    },
    RealmStats(PageHostRealmStats),
    ChildStats(PageHostChildStats),
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
    /// Aggregate ABI/fingerprint evidence for the exact verified direct
    /// lowering map retained under this opaque metadata handle. It carries no
    /// map entries, source spans, AST nodes, or bytecode offsets.
    DebuggerBlueTsMetadataLoweringSummary {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        summary: Box<PageHostDebuggerBlueTsMetadataLoweringSummary>,
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
    /// Bounded compiler-minted contract identities. The IDs are local to the
    /// exact metadata attachment and carry no contract-record payload.
    DebuggerBlueTsMetadataContracts {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        contracts: Vec<PageHostDebuggerBlueTsMetadataContractId>,
    },
    /// One bounded compiler-produced contract display. The enclosing tuple
    /// keeps it bound to an exact child program and metadata attachment.
    DebuggerBlueTsMetadataContract {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        contract: PageHostDebuggerBlueTsMetadataContractDisplay,
    },
    /// A data-only exact-contract validation outcome. It contains no caller
    /// value, plan, failure path, expected shape, or runtime object.
    DebuggerBlueTsMetadataContractValidation {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        validation: PageHostDebuggerBlueTsMetadataContractValidation,
    },
    /// One bounded compiler-produced symbol display. The enclosing tuple keeps
    /// it bound to an exact child program and metadata attachment.
    DebuggerBlueTsMetadataSymbol {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        symbol: PageHostDebuggerBlueTsMetadataSymbolDisplay,
    },
    /// One bounded child-local source-text-free declaration range. The
    /// enclosing tuple retains the exact private program and metadata binding.
    DebuggerBlueTsMetadataSymbolLocation {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        location: PageHostDebuggerBlueTsMetadataSymbolLocation,
    },
    /// One exact child-local contract/source pair and bounded byte range.
    DebuggerBlueTsMetadataContractLocation {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        location: PageHostDebuggerBlueTsMetadataContractLocation,
    },
    /// Exact original BlueTS byte span for the echoed verified child safe
    /// point. This private reply is not exposed to a debugger socket client.
    DebuggerBlueTsSafePointSpan {
        tab_id: u64,
        document_generation: u64,
        metadata: PageHostDebuggerMetadataHandle,
        safe_point: PageHostDebuggerSafePoint,
        span: PageHostDebuggerBlueTsSafePointSpan,
    },
    /// Complete private location under the echoed document/program/metadata
    /// tuple. A missing, stale, or unbound site returns a typed error instead.
    DebuggerBlueTsExceptionLocation {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        location: PageHostDebuggerBlueTsExceptionLocation,
    },
    /// Repeats the exact private request tuple and returns only a verified
    /// child safe point or an explicit unbound result. Core must separately
    /// authorize and remint this before any public debugger disclosure.
    DebuggerBlueTsSourceBreakpoint {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        source_id: u32,
        source_byte: u32,
        safe_point: Option<PageHostDebuggerSafePoint>,
    },
    /// A verified relation that repeats only the two requested opaque IDs.
    DebuggerBlueTsMetadataSymbolType {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        symbol_type: PageHostDebuggerBlueTsMetadataSymbolType,
    },
    /// A verified relation that repeats only the two requested opaque IDs.
    DebuggerBlueTsMetadataSymbolContract {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        symbol_contract: PageHostDebuggerBlueTsMetadataSymbolContract,
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
    DebuggerNestedSafePointBreakpointArmed {
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    },
    DebuggerLinkedNestedSafePointBreakpointArmed {
        tab_id: u64,
        document_generation: u64,
        entry_program: PageHostDebuggerProgram,
        safe_point: PageHostDebuggerSafePoint,
    },
    DebuggerExecutionState {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        state: PageHostDebuggerExecutionState,
    },
    DebuggerLinkedExecutionState {
        frame: PageHostDebuggerLinkedFrame,
        state: PageHostDebuggerLinkedExecutionState,
    },
    DebuggerExecutionResumed {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    },
    DebuggerExecutionStepRequested {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    },
    DebuggerNestedStepRequested {
        frame: PageHostDebuggerFrame,
    },
    DebuggerNestedResumeRequested {
        frame: PageHostDebuggerFrame,
    },
    DebuggerLinkedNestedResumeRequested {
        frame: PageHostDebuggerLinkedFrame,
    },
    DebuggerStackSnapshot {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        frame: Option<PageHostDebuggerFrame>,
        snapshot: PageHostDebuggerStackSnapshot,
    },
    DebuggerLinkedStackSnapshot {
        frame: PageHostDebuggerLinkedFrame,
        snapshot: Box<PageHostDebuggerLinkedStackSnapshot>,
    },
    DebuggerLinkedStackSpans {
        frame: PageHostDebuggerLinkedFrame,
        snapshot: Box<PageHostDebuggerLinkedStackSnapshot>,
        spans: Box<[PageHostDebuggerBlueTsSafePointSpan; 2]>,
    },
    DebuggerValueSnapshot(Box<PageHostDebuggerValueSnapshot>),
    DebuggerStaticScopeRelation(Box<PageHostDebuggerStaticScopeRelation>),
    DebuggerBlueTsSourceStepRequested {
        tab_id: u64,
        document_generation: u64,
        metadata: PageHostDebuggerMetadataHandle,
        source_id: u32,
        safe_point: PageHostDebuggerSafePoint,
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
mod tests;
