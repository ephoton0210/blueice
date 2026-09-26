// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Versioned native debugger IPC vocabulary.
//!
//! This channel is intentionally separate from page-script DOM calls and the
//! public automation protocol. It gives `core` and an out-of-process BlueJS
//! host one typed way to agree on a page realm, its generation, and executable
//! program locations. It establishes framing, handshake, capability discovery,
//! bounded opaque program-location operations, exact breakpoint configuration,
//! and an opt-in root-code-unit pause/resume seam. Version forty adds a complete
//! linked dependency/entry module pause family: two separately reminted
//! program frames, a fixed complete stack, exact resume, and all-or-nothing
//! original coordinates under the existing span grant and two independent
//! same-stream metadata/source receipts. Version thirty-nine adds an
//! independently granted, source-receipted terminal BlueTS exception location
//! with an exact core-reminted safe point. Version thirty-seven adds
//! independently owner/client-gated, same-stream Scopes-receipted bounded
//! paused values. Version thirty-six adds atomic stack-coordinate batches.
//! Version thirty-five adds
//! separately gated bounded stack-location and active-scope snapshots for an
//! exact paused target, without values or BlueTS source coordinates. Version thirty-four binds
//! each active frame handle to its core instance across supervised cutover.
//! Version thirty-three adds
//! an exact active-frame resume command and distinct resuming state. Version
//! thirty-two adds a
//! separately gated, core-reminted active nested-frame identity and exact
//! nested arm/state/step commands. Its frame handle
//! is not the child invocation serial, a static code-unit point, or a value.
//! Version thirty adds an
//! independently authorized BlueTS source-span step for one exact paused
//! root safe point, with a distinct bounded-limit stop reason. Version twenty-nine adds
//! an atomic source-position arm request: one session turn must first pass
//! the independently default-denied metadata grant and same-stream opaque
//! source receipt, then bind an exact root safe point and arm only a pending
//! classic script. An unbound position never starts execution. Version
//! twenty-eight adds
//! a separately default-denied, bounded original-BlueTS byte-position binding
//! under one same-stream source-ID receipt. It reports one core-revalidated
//! safe point or explicit unbound result, without installing or executing a
//! breakpoint or granting source text. The metadata capability manifest is v3.
//! Version twenty-seven adds
//! a separately default-denied original-BlueTS safe-point span under exact
//! program, metadata, and same-stream source-ID receipts. It returns no source
//! text, module identity, or nearest guess.
//! Version twenty-six adds a source-free, exact-program single-root-instruction
//! step request and its observable one-turn `Stepping` state for the installed
//! in-process route; it does not imply nested-frame or out-of-process stepping.
//! Version twenty-five adds
//! bounded original-source UTF-16 coordinates to the existing separately
//! authorized symbol/contract location replies, with no arbitrary offset
//! query. Version twenty-four adds
//! the compiler's export classification to the existing opt-in, receipt-bound
//! symbol display without adding a target or a new disclosure grant. Version
//! twenty-three adds a fixed root-shape classification to the already default-denied, receipt-
//! bound contract display; it exposes no contract-plan edge or field name.
//! Version twenty-two adds a
//! separately default-denied contract declaration range under exact
//! same-stream contract and source receipts; no plan or source text crosses.
//! Version twenty-one adds a
//! compiler-minted declaration kind to the already receipt-bound, opt-in
//! symbol display. It carries no additional target or source-read authority.
//! Version twenty adds an
//! independently default-denied symbol-to-contract relation that requires
//! exact symbol and contract receipts on one debugger stream. Version nineteen adds an
//! independently default-denied symbol-to-static-type relation that requires
//! exact symbol and type receipts on one debugger stream. Version eighteen adds the
//! independently default-deny symbol-location operation for prior opaque
//! symbol and source receipts. Version seventeen added the independently
//! default-deny lowering-map summary operation for a prior
//! opaque metadata handle. Version sixteen added the default-deny data-only
//! contract-validation operation for a prior opaque
//! contract ID
//! handle: `Hello` grants only the canonical intersection of a requested
//! manifest and the core policy, and a metadata operation may be dispatched
//! only after the exact target's capability report also grants its specific
//! metadata capability. A host must report every operation as
//! [`DebuggerCapabilityState::Available`] only after it implements the native
//! behavior; a configured breakpoint is not evidence that pause, stack,
//! scope, value inspection, or static metadata access already exists.

use crate::compiler::CompilerContractValue;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet};
use std::io::{self, Read, Write};
use std::sync::{Arc, Mutex};

/// Independent protocol version for the private core-to-BlueJS debugger
/// channel. It does not share `crate::PROTOCOL_VERSION`, whose lifecycle is
/// the frontend control-plane protocol.
pub const DEBUGGER_PROTOCOL_VERSION: u32 = 41;

pub const DEBUGGER_MAX_STACK_FRAMES: u32 = 64;
pub const DEBUGGER_MAX_SCOPE_ENTRIES: u32 = 256;
pub const DEBUGGER_MAX_VALUE_DEPTH: usize = 4;
pub const DEBUGGER_MAX_VALUE_CONTAINER_LENGTH: usize = 32;
pub const DEBUGGER_MAX_VALUE_NODES: usize = 256;
pub const DEBUGGER_MAX_VALUE_PAYLOAD_BYTES: usize = 4_096;

mod shapes;
pub use shapes::*;

mod capabilities;
pub use capabilities::*;

mod authorization;
pub use authorization::*;

/// Source-free state of one native-debugger controlled declaration. A paused
/// state identifies only an already-validated opaque instruction boundary;
/// it never carries source text, bytecode, a stack, a scope, or a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DebuggerExecutionState {
    Pending,
    Paused {
        safe_point: DebuggerSafePoint,
    },
    NestedPaused {
        frame: DebuggerFrame,
        safe_point: DebuggerSafePoint,
    },
    /// A source-span step stopped at its fixed instruction budget; the
    /// continuation remains paused at this verified root safe point.
    SourceStepLimitReached {
        safe_point: DebuggerSafePoint,
    },
    Stepping,
    NestedStepping {
        frame: DebuggerFrame,
    },
    NestedResuming {
        frame: DebuggerFrame,
    },
    Resuming,
    Completed,
}

/// Versioned requests for the core debugger route and the isolated BlueJS host.
///
/// Positive command families are added only with real native behavior; a
/// denial-only source-text probe is explicitly non-dereferenceable.
/// The first non-discovery family resolves opaque programs and exact
/// compiler-verified instruction boundaries. The second is exact, bounded
/// breakpoint configuration. The root-entry arm/resume operations are
/// separately named because normal configuration must not be mistaken for a
/// VM interruption hook.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DebuggerRequest {
    Hello {
        protocol_version: u32,
        /// The exact canonical static-metadata capabilities this debugger
        /// client asks the core to consider for this transport session.
        /// Requesting one grants nothing; the core replies with only its
        /// policy intersection in [`DebuggerReply::HelloAck`].
        requested_metadata_capabilities: DebuggerMetadataCapabilityManifest,
        /// Independent opt-in to one bounded active-scope value read.
        requested_bounded_values: bool,
    },
    /// Lists bounded, currently loaded page realm identities. The reply carries
    /// no URL, source, program, bytecode, or runtime object; clients use the
    /// returned generation only to make a subsequent target-bound discovery
    /// request.
    ListPageRealms,
    DescribeCapabilities {
        realm: DebuggerPageRealm,
    },
    /// Lists opaque program identities currently retained by one exact page
    /// realm. It never returns a source identity, bytecode, VM object, or
    /// completion value.
    ListPrograms {
        realm: DebuggerPageRealm,
    },
    /// Lists a bounded source-free inventory of core-minted static-metadata
    /// handles for one exact live program. It exposes neither metadata nor a
    /// way to dereference a handle. Dispatch requires both the session grant
    /// negotiated in `Hello` and the exact realm's advertised capability.
    ListStaticMetadata {
        program: DebuggerProgram,
    },
    /// Returns a bounded source-free summary for one exact opaque handle
    /// obtained from [`Self::ListStaticMetadata`]. This is not a metadata
    /// record read or dereference operation.
    DescribeStaticMetadata {
        metadata: DebuggerStaticMetadataHandle,
    },
    /// Returns one separately authorized, aggregate summary of the verified
    /// direct BlueTS-to-BlueJS lowering map for an exact inventory handle.
    /// It never exposes source spans, map entries, AST nodes, code-unit IDs,
    /// bytecode offsets, or a generic map-read operation.
    DescribeStaticMetadataLoweringSummary {
        metadata: DebuggerStaticMetadataHandle,
    },
    /// Lists bounded compiler-minted source-record identities for one exact
    /// metadata attachment. It is not a source/provenance read operation.
    ListStaticMetadataSources {
        metadata: DebuggerStaticMetadataHandle,
    },
    /// Lists only compiler-minted type-record identities for one exact
    /// metadata attachment. It is not a type display or metadata-record read.
    ListStaticMetadataTypes {
        metadata: DebuggerStaticMetadataHandle,
    },
    /// Describes one type ID previously returned by
    /// [`Self::ListStaticMetadataTypes`]. This separately authorized
    /// operation returns only a bounded compiler-produced type display.
    DescribeStaticMetadataType {
        static_type: DebuggerStaticMetadataTypeId,
    },
    /// Lists only compiler-minted symbol-record identities for one exact
    /// metadata attachment. It is not a symbol name/span/type or record read.
    ListStaticMetadataSymbols {
        metadata: DebuggerStaticMetadataHandle,
    },
    /// Describes one symbol ID previously returned by
    /// [`Self::ListStaticMetadataSymbols`]. This separately authorized
    /// operation returns only a bounded compiler-produced symbol display.
    DescribeStaticMetadataSymbol {
        symbol: DebuggerStaticMetadataSymbolId,
    },
    /// Describes one half-open byte range for a symbol previously returned by
    /// [`Self::ListStaticMetadataSymbols`]. The reply requires a separately
    /// receipted source ID and carries neither text nor module identity.
    DescribeStaticMetadataSymbolLocation {
        target: DebuggerStaticMetadataSymbolLocationTarget,
    },
    /// Returns one exact original BlueTS byte span only under a separately
    /// granted metadata capability and a prior same-stream source-ID receipt.
    DescribeStaticMetadataSafePointSpan {
        target: DebuggerStaticMetadataSafePointSpanTarget,
    },
    /// Reports only the terminal uncaught exception's exact BlueTS source
    /// position under the separate safe-point-span grant and source receipt.
    DescribeExceptionLocation {
        source: DebuggerStaticMetadataSourceId,
    },
    /// Resolves one bounded original BlueTS byte position in a previously
    /// inventoried source ID to a compiler-verified safe point or explicit
    /// unbound result. This separately granted operation neither reads source
    /// text nor stores, arms, or executes a breakpoint.
    ResolveStaticMetadataSourceBreakpoint {
        target: DebuggerStaticMetadataSourceBreakpointTarget,
    },
    /// Atomically resolves one separately authorized original BlueTS byte
    /// position and arms only its verified classic or module root-code-unit
    /// safe point. Both the
    /// same-stream metadata/source receipts and execution-control capability
    /// must be live; an unbound position or non-root instruction fails before
    /// the pending declaration starts. The reply is the ordinary source-free
    /// `RootSafePointBreakpointArmed` with the exact core-reminted safe point.
    ArmStaticMetadataSourceBreakpoint {
        target: DebuggerStaticMetadataSourceBreakpointTarget,
    },
    /// Describes one contract declaration range only after separate exact
    /// contract and source inventory receipts on this debugger stream.
    DescribeStaticMetadataContractLocation {
        target: DebuggerStaticMetadataContractLocationTarget,
    },
    /// Verifies one compiler-produced symbol-to-type relation. Both IDs must
    /// have crossed this debugger stream's separate opaque inventories.
    DescribeStaticMetadataSymbolType {
        target: DebuggerStaticMetadataSymbolType,
    },
    /// Verifies one reifiable symbol's compiler-produced contract relation.
    /// Both IDs must have crossed this stream's separate opaque inventories.
    DescribeStaticMetadataSymbolContract {
        target: DebuggerStaticMetadataSymbolContract,
    },
    /// Lists only compiler-minted contract identities for one exact metadata
    /// attachment. It is not a contract name/span/plan/validation read.
    ListStaticMetadataContracts {
        metadata: DebuggerStaticMetadataHandle,
    },
    /// Describes one contract ID previously returned by
    /// [`Self::ListStaticMetadataContracts`]. This separately authorized
    /// operation returns only a bounded compiler-produced contract display.
    DescribeStaticMetadataContract {
        contract: DebuggerStaticMetadataContractId,
    },
    /// Validates a caller-provided data-only value against one contract ID
    /// previously returned by [`Self::ListStaticMetadataContracts`]. The
    /// independently authorized reply contains only a boolean; it never
    /// echoes input or exposes contract plan/failure details.
    ValidateStaticMetadataContract {
        contract: DebuggerStaticMetadataContractId,
        value: CompilerContractValue,
    },
    /// Describes one source ID previously returned by
    /// [`Self::ListStaticMetadataSources`]. This separately authorized
    /// operation returns compiler-canonical module identity and a labeled
    /// SHA-256 digest only; it cannot read source text.
    DescribeStaticMetadataSource {
        source: DebuggerStaticMetadataSourceId,
    },
    /// Lists bounded, compiler-verified instruction boundaries for one exact
    /// live program generation. A caller must not infer or substitute offsets.
    ListSafePoints {
        program: DebuggerProgram,
    },
    /// Checks one supplied location against the exact current BlueJS program
    /// generation. This operation does not execute or pause the program.
    ValidateSafePoint {
        safe_point: DebuggerSafePoint,
    },
    /// Stores one exact, already compiler-verified breakpoint record for the
    /// current program generation. It does not execute or interrupt a VM.
    SetBreakpoint {
        safe_point: DebuggerSafePoint,
    },
    /// Arms one compiler-verified root-code-unit entry boundary for a program
    /// that has been admitted but has not entered BlueJS execution. The host
    /// rejects any non-entry safe point or program that is no longer pending.
    ArmEntryBreakpoint {
        safe_point: DebuggerSafePoint,
    },
    /// Arms one exact root-code-unit safe point for a pending classic script.
    /// Unlike `SetBreakpoint`, this starts the script and retains a real
    /// BlueJS continuation when the requested non-entry boundary is reached.
    /// It rejects modules and child code units rather than claiming generic
    /// interpreter interruption.
    ArmRootSafePointBreakpoint {
        safe_point: DebuggerSafePoint,
    },
    /// Arms one exact child-code-unit safe point on a still-pending classic
    /// or BlueTS entry module. The eventual pause mints an active frame;
    /// this static target alone never authorizes an instruction step.
    ArmNestedSafePointBreakpoint {
        safe_point: DebuggerSafePoint,
    },
    /// Arms one pending entry module at an exact safe point in its directly
    /// linked dependency. Both installed generations are independently bound.
    ArmLinkedNestedSafePointBreakpoint {
        target: DebuggerLinkedArmTarget,
    },
    /// Lists only exact breakpoint records currently retained by one live
    /// realm. The records carry no source, bytecode, VM object, or value.
    ListBreakpoints {
        realm: DebuggerPageRealm,
    },
    /// Removes one exact current breakpoint record. Removal is idempotent,
    /// but its target must still name a live compiler-verified boundary.
    ClearBreakpoint {
        safe_point: DebuggerSafePoint,
    },
    /// Reads the source-free pending/paused/stepping/resuming/completed state
    /// for one exact program generation in the opt-in debugger scheduler.
    GetExecutionState {
        program: DebuggerProgram,
    },
    /// Resumes a currently paused classic program on its retained BlueJS
    /// continuation. It cannot resume a non-paused program or inject a value.
    ResumeExecution {
        program: DebuggerProgram,
    },
    /// Schedules one root-code-unit instruction from an exactly paused
    /// classic program. The eventual state is another verified root safe
    /// point or completion; no stack, operand, source, or value is returned.
    StepRootInstruction {
        program: DebuggerProgram,
    },
    /// Steps only the exact core-reminted, still-active nested invocation.
    StepNestedInstruction {
        frame: DebuggerFrame,
    },
    /// Runs only this exact paused nested invocation to its return. The
    /// original root remains paused, with its own separate resume command.
    ResumeNestedExecution {
        frame: DebuggerFrame,
    },
    GetLinkedExecutionState {
        entry: DebuggerProgram,
    },
    GetLinkedStack {
        top_frame: DebuggerLinkedFrame,
    },
    ResumeLinkedNestedExecution {
        top_frame: DebuggerLinkedFrame,
    },
    /// Exact two-source read under the existing independent span grant and
    /// each frame's metadata/source receipts on this debugger stream.
    GetLinkedStackCoordinates {
        target: DebuggerLinkedStackCoordinatesTarget,
    },
    /// Reads only paused frame locations. Scopes require their own gate.
    GetStack {
        program: DebuggerProgram,
        frame: Option<DebuggerFrame>,
        max_frames: u32,
    },
    /// Resolves original BlueTS coordinates for an exact previously returned
    /// Stack snapshot. Requires same-stream metadata/source receipts and the
    /// independent safe-point-span grant for every frame.
    GetStackCoordinates {
        target: DebuggerStackCoordinatesTarget,
    },
    /// Reads one frame's active lexical slots only when its exact currently
    /// paused safe point still equals the caller's expected location.
    GetScopes {
        program: DebuggerProgram,
        frame: Option<DebuggerFrame>,
        frame_index: u32,
        expected_safe_point: DebuggerSafePoint,
        max_scope_entries: u32,
    },
    /// Reads only the entry-root lexical slots of one complete linked pause.
    /// An incomplete reply does not mint a static-scope receipt.
    GetLinkedScopes {
        expected_stack: DebuggerLinkedStackSnapshot,
        max_scope_entries: u32,
    },
    /// Returns a compiler-only relation for an exact same-stream paused slot
    /// after the independent static grant and all three inventories.
    GetStaticScopeRelation {
        target: DebuggerStaticScopeTarget,
    },
    /// Reads only a Scopes slot receipted on this debugger stream during the
    /// same core-owned pause incarnation and independently granted in Hello.
    GetValue {
        target: DebuggerValueTarget,
    },
    /// Denial-only compatibility probe. Core never resolves this target or
    /// reads source text; the reply is always a typed capability refusal.
    GetSourceText {
        program: DebuggerProgram,
    },
    /// Steps from one exact paused BlueTS root safe point to the next distinct
    /// compiler-bound source span, completion, or a bounded-limit stop.
    /// Requires a separate metadata grant and same-stream source receipt.
    StepStaticMetadataSourceSpan {
        target: DebuggerStaticMetadataSafePointSpanTarget,
    },
    /// Catch-all for a newer request variant. Like the frontend protocol, a
    /// receiver preserves framing for an unknown unit command and replies
    /// with a typed capability refusal rather than running another command.
    #[serde(other)]
    Unknown,
}

/// BlueJS host replies on the debugger channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DebuggerReply {
    HelloAck {
        protocol_version: u32,
        /// The canonical intersection of the client's requested metadata
        /// capabilities and the core-owned allow policy for this one stream.
        /// An empty set is a successful handshake with no metadata authority.
        granted_metadata_capabilities: DebuggerMetadataCapabilityManifest,
        /// Granted only if both the owner and this client selected values.
        granted_bounded_values: bool,
    },
    /// Reply to [`DebuggerRequest::ListPageRealms`].
    PageRealms(Vec<DebuggerPageRealm>),
    Capabilities(DebuggerCapabilities),
    Programs(Vec<DebuggerProgram>),
    LinkedNestedSafePointBreakpointArmed {
        target: DebuggerLinkedArmTarget,
    },
    LinkedExecutionState {
        entry: DebuggerProgram,
        state: Box<DebuggerLinkedExecutionState>,
    },
    LinkedStack(Box<DebuggerLinkedStackSnapshot>),
    LinkedNestedResumeRequested {
        top_frame: DebuggerLinkedFrame,
    },
    LinkedStackCoordinates(Box<DebuggerLinkedStackCoordinates>),
    /// Reply to [`DebuggerRequest::ListStaticMetadata`]. Every handle is
    /// opaque and bound to the exact program generation supplied by the
    /// request; this is not a metadata payload or a read capability.
    StaticMetadata(Vec<DebuggerStaticMetadataHandle>),
    /// Reply to [`DebuggerRequest::DescribeStaticMetadata`]. The summary is
    /// bounded and source-free; individual metadata records remain private.
    StaticMetadataSummary(DebuggerStaticMetadataSummary),
    /// Reply to [`DebuggerRequest::DescribeStaticMetadataLoweringSummary`].
    /// The result is parent-bound and contains only fixed ABI/fingerprint
    /// evidence and an aggregate verified-entry count.
    StaticMetadataLoweringSummary(Box<DebuggerStaticMetadataLoweringSummary>),
    /// Reply to [`DebuggerRequest::ListStaticMetadataSources`]. IDs are
    /// parent-handle-bound and contain no source/provenance payload.
    StaticMetadataSources(Vec<DebuggerStaticMetadataSourceId>),
    /// Reply to [`DebuggerRequest::ListStaticMetadataTypes`]. IDs remain
    /// parent-handle-bound and contain no type display or record payload.
    StaticMetadataTypes(Vec<DebuggerStaticMetadataTypeId>),
    /// Reply to [`DebuggerRequest::DescribeStaticMetadataType`]. The type
    /// display remains parent-bound and source-text-free.
    StaticMetadataType(DebuggerStaticMetadataTypeDisplay),
    /// Reply to [`DebuggerRequest::ListStaticMetadataSymbols`]. IDs remain
    /// parent-bound and contain no symbol name, span, type, or record payload.
    StaticMetadataSymbols(Vec<DebuggerStaticMetadataSymbolId>),
    /// Reply to [`DebuggerRequest::DescribeStaticMetadataSymbol`]. The symbol
    /// display remains parent-bound, receipted, and source-text-free.
    StaticMetadataSymbol(DebuggerStaticMetadataSymbolDisplay),
    /// Reply to [`DebuggerRequest::DescribeStaticMetadataSymbolLocation`].
    /// The location is parent-bound, receipted, source-text-free, and does
    /// not include a module identity or a bytecode/source-map translation.
    StaticMetadataSymbolLocation(DebuggerStaticMetadataSymbolLocation),
    /// Reply to [`DebuggerRequest::DescribeStaticMetadataSafePointSpan`].
    /// The exact safe point and previously receipted source ID are echoed;
    /// no source text or module identity is included.
    StaticMetadataSafePointSpan(DebuggerStaticMetadataSafePointSpan),
    /// Distinct terminal exception location, never an arbitrary source-map
    /// lookup or a source-text/error-value read.
    ExceptionLocation(DebuggerExceptionLocation),
    /// Reply to [`DebuggerRequest::ResolveStaticMetadataSourceBreakpoint`].
    /// It repeats only the requested source/position and a core-reminted
    /// verified safe point, or an explicit unbound result.
    StaticMetadataSourceBreakpoint(DebuggerStaticMetadataSourceBreakpoint),
    /// Reply to [`DebuggerRequest::DescribeStaticMetadataContractLocation`].
    /// It contains only receipted IDs and a bounded byte range.
    StaticMetadataContractLocation(DebuggerStaticMetadataContractLocation),
    /// Reply to [`DebuggerRequest::DescribeStaticMetadataSymbolType`]. It
    /// repeats only the two previously receipted opaque IDs.
    StaticMetadataSymbolType(DebuggerStaticMetadataSymbolType),
    /// Reply to [`DebuggerRequest::DescribeStaticMetadataSymbolContract`].
    /// It repeats only the two previously receipted opaque IDs.
    StaticMetadataSymbolContract(DebuggerStaticMetadataSymbolContract),
    /// Reply to [`DebuggerRequest::ListStaticMetadataContracts`]. IDs remain
    /// parent-bound and contain no contract name, span, plan, or validation
    /// payload.
    StaticMetadataContracts(Vec<DebuggerStaticMetadataContractId>),
    /// Reply to [`DebuggerRequest::DescribeStaticMetadataContract`]. The
    /// contract display remains parent-bound, receipted, and source-text-free.
    StaticMetadataContract(DebuggerStaticMetadataContractDisplay),
    /// Reply to [`DebuggerRequest::ValidateStaticMetadataContract`]. The
    /// result remains parent-bound and receipted, and contains no caller data
    /// or contract-plan/failure detail.
    StaticMetadataContractValidation(DebuggerStaticMetadataContractValidation),
    /// Reply to [`DebuggerRequest::DescribeStaticMetadataSource`]. This is a
    /// bounded owner-authorized provenance disclosure, never source text.
    StaticMetadataSourceProvenance(DebuggerStaticMetadataSourceProvenance),
    SafePoints(Vec<DebuggerSafePoint>),
    SafePointValidated {
        safe_point: DebuggerSafePoint,
    },
    BreakpointSet {
        safe_point: DebuggerSafePoint,
    },
    BreakpointArmed {
        safe_point: DebuggerSafePoint,
    },
    RootSafePointBreakpointArmed {
        safe_point: DebuggerSafePoint,
    },
    NestedSafePointBreakpointArmed {
        safe_point: DebuggerSafePoint,
    },
    Breakpoints(Vec<DebuggerSafePoint>),
    BreakpointCleared {
        safe_point: DebuggerSafePoint,
        was_present: bool,
    },
    ExecutionState {
        program: DebuggerProgram,
        state: DebuggerExecutionState,
    },
    ExecutionResumed {
        program: DebuggerProgram,
    },
    ExecutionStepRequested {
        program: DebuggerProgram,
    },
    NestedStepRequested {
        frame: DebuggerFrame,
    },
    NestedResumeRequested {
        frame: DebuggerFrame,
    },
    Stack(DebuggerStackSnapshot),
    StackCoordinates(DebuggerStackCoordinates),
    Scopes(DebuggerScopeSnapshot),
    LinkedScopes(Box<DebuggerLinkedScopeSnapshot>),
    StaticScopeRelation(Box<DebuggerStaticScopeRelation>),
    Value(Box<DebuggerValueSnapshot>),
    ExecutionSourceSpanStepRequested {
        safe_point: DebuggerSafePoint,
    },
    Unsupported {
        operation: String,
        reason: String,
    },
    Error {
        code: DebuggerErrorCode,
        message: String,
    },
}

/// Stable error categories for native debugger implementations and their
/// adapters. `message` remains diagnostic prose; clients branch on this code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DebuggerErrorCode {
    ProtocolVersion,
    InvalidCapabilityManifest,
    InvalidTarget,
    StaleRealm,
    StaleProgram,
    InvalidSafePoint,
    CapabilityUnavailable,
    InvalidExecutionState,
    ResourceLimit,
}

/// Builds the only valid reply to the connection's first request. A caller
/// must still reject any non-`Hello` first request before it dispatches the
/// connection to a realm owner.
pub fn negotiate(
    request: &DebuggerRequest,
    allowed_metadata_capabilities: &DebuggerMetadataCapabilityManifest,
) -> DebuggerReply {
    negotiate_with_values(request, allowed_metadata_capabilities, false)
}

/// Negotiates the independent owner/client value policy alongside the
/// unchanged canonical static-metadata manifest.
pub fn negotiate_with_values(
    request: &DebuggerRequest,
    allowed_metadata_capabilities: &DebuggerMetadataCapabilityManifest,
    allowed_bounded_values: bool,
) -> DebuggerReply {
    match request {
        DebuggerRequest::Hello {
            protocol_version,
            requested_metadata_capabilities,
            requested_bounded_values,
        } if *protocol_version == DEBUGGER_PROTOCOL_VERSION
            && requested_metadata_capabilities.is_well_formed()
            && allowed_metadata_capabilities.is_well_formed() =>
        {
            DebuggerReply::HelloAck {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                granted_metadata_capabilities: allowed_metadata_capabilities
                    .intersection(requested_metadata_capabilities),
                granted_bounded_values: allowed_bounded_values && *requested_bounded_values,
            }
        }
        DebuggerRequest::Hello {
            protocol_version, ..
        } if *protocol_version == DEBUGGER_PROTOCOL_VERSION => DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidCapabilityManifest,
            message: "debugger metadata capability manifest is malformed".to_string(),
        },
        DebuggerRequest::Hello { .. } => DebuggerReply::Error {
            code: DebuggerErrorCode::ProtocolVersion,
            message: "unsupported debugger protocol version".to_string(),
        },
        DebuggerRequest::ListPageRealms
        | DebuggerRequest::DescribeCapabilities { .. }
        | DebuggerRequest::ListPrograms { .. }
        | DebuggerRequest::ListStaticMetadata { .. }
        | DebuggerRequest::DescribeStaticMetadata { .. }
        | DebuggerRequest::DescribeStaticMetadataLoweringSummary { .. }
        | DebuggerRequest::ListStaticMetadataSources { .. }
        | DebuggerRequest::ListStaticMetadataTypes { .. }
        | DebuggerRequest::DescribeStaticMetadataType { .. }
        | DebuggerRequest::ListStaticMetadataSymbols { .. }
        | DebuggerRequest::DescribeStaticMetadataSymbol { .. }
        | DebuggerRequest::DescribeStaticMetadataSymbolLocation { .. }
        | DebuggerRequest::DescribeStaticMetadataSafePointSpan { .. }
        | DebuggerRequest::DescribeExceptionLocation { .. }
        | DebuggerRequest::ResolveStaticMetadataSourceBreakpoint { .. }
        | DebuggerRequest::ArmStaticMetadataSourceBreakpoint { .. }
        | DebuggerRequest::DescribeStaticMetadataContractLocation { .. }
        | DebuggerRequest::DescribeStaticMetadataSymbolType { .. }
        | DebuggerRequest::DescribeStaticMetadataSymbolContract { .. }
        | DebuggerRequest::ListStaticMetadataContracts { .. }
        | DebuggerRequest::DescribeStaticMetadataContract { .. }
        | DebuggerRequest::ValidateStaticMetadataContract { .. }
        | DebuggerRequest::DescribeStaticMetadataSource { .. }
        | DebuggerRequest::ListSafePoints { .. }
        | DebuggerRequest::ValidateSafePoint { .. }
        | DebuggerRequest::SetBreakpoint { .. }
        | DebuggerRequest::ArmEntryBreakpoint { .. }
        | DebuggerRequest::ArmRootSafePointBreakpoint { .. }
        | DebuggerRequest::ArmNestedSafePointBreakpoint { .. }
        | DebuggerRequest::ArmLinkedNestedSafePointBreakpoint { .. }
        | DebuggerRequest::ListBreakpoints { .. }
        | DebuggerRequest::ClearBreakpoint { .. }
        | DebuggerRequest::GetExecutionState { .. }
        | DebuggerRequest::ResumeExecution { .. }
        | DebuggerRequest::StepRootInstruction { .. }
        | DebuggerRequest::StepNestedInstruction { .. }
        | DebuggerRequest::ResumeNestedExecution { .. }
        | DebuggerRequest::GetLinkedExecutionState { .. }
        | DebuggerRequest::GetLinkedStack { .. }
        | DebuggerRequest::ResumeLinkedNestedExecution { .. }
        | DebuggerRequest::GetLinkedStackCoordinates { .. }
        | DebuggerRequest::GetStack { .. }
        | DebuggerRequest::GetStackCoordinates { .. }
        | DebuggerRequest::GetScopes { .. }
        | DebuggerRequest::GetLinkedScopes { .. }
        | DebuggerRequest::GetStaticScopeRelation { .. }
        | DebuggerRequest::GetValue { .. }
        | DebuggerRequest::GetSourceText { .. }
        | DebuggerRequest::StepStaticMetadataSourceSpan { .. }
        | DebuggerRequest::Unknown => DebuggerReply::Error {
            code: DebuggerErrorCode::ProtocolVersion,
            message: "debugger protocol requires Hello as its first request".to_string(),
        },
    }
}

pub fn write_debugger_request<W: Write>(
    writer: &mut W,
    request: &DebuggerRequest,
) -> io::Result<()> {
    crate::write_framed(writer, request)
}

pub fn read_debugger_request<R: Read>(reader: &mut R) -> io::Result<DebuggerRequest> {
    let bytes = crate::read_frame_bytes(reader)?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

pub fn write_debugger_reply<W: Write>(writer: &mut W, reply: &DebuggerReply) -> io::Result<()> {
    crate::write_framed(writer, reply)
}

pub fn read_debugger_reply<R: Read>(reader: &mut R) -> io::Result<DebuggerReply> {
    let bytes = crate::read_frame_bytes(reader)?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

#[cfg(all(test, unix))]
mod tests;
