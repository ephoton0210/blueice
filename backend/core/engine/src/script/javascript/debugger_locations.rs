// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

/// Source-free debugger location operations owned by an explicitly selected
/// page executor. The in-process runtime implements its richer debugger path
/// directly; the launcher-supervised child implements only this narrow
/// discovery/validation surface through its authenticated private transport.
///
/// The launcher-supervised child supports a bounded, exact breakpoint
/// configuration table in addition to location discovery. The table neither
/// pauses nor executes its VM unless an explicitly selected child execution
/// controller implements the separate root-classic methods below. Nested
/// stepping and bounded stack/scope snapshots require separate exact-target
/// methods and capability gates; source, bytecode, and values remain excluded.
pub trait PageJavaScriptDebuggerLocations {
    /// Whether this owner currently has the exact live tab/document realm.
    fn debugger_has_live_realm(&mut self, tab_id: TabId, document_generation: u64) -> bool;

    /// Immutable upper bound for one source-free safe-point inventory reply.
    fn max_debugger_safe_points_per_program(&self) -> usize;

    /// Whether this route implements the narrow exact-breakpoint
    /// configuration table. This is separate from location discovery so a
    /// transport double cannot cause the public capability report to promise
    /// an operation it does not implement.
    fn debugger_breakpoint_configuration_available(&self) -> bool {
        false
    }

    /// Immutable upper bound for one source-free breakpoint configuration
    /// list reply.
    fn max_debugger_breakpoints_per_realm(&self) -> usize {
        0
    }

    /// Whether this selected route can list source-free static BlueTS metadata
    /// handles. A capability report and the per-stream core authorization must
    /// both grant the public operation before this inventory is called.
    fn debugger_static_metadata_inventory_available(&self) -> bool {
        false
    }

    /// Whether this selected route can describe one existing static-metadata
    /// handle with a bounded source-free summary. This remains a distinct
    /// capability from handle inventory and is default-deny for every route.
    fn debugger_static_metadata_summary_available(&self) -> bool {
        false
    }

    /// Whether this route can return a source-free aggregate summary of the
    /// verified direct lowering map for an exact opaque metadata attachment.
    /// Per-entry source spans and bytecode positions remain unavailable.
    fn debugger_static_metadata_lowering_summary_available(&self) -> bool {
        false
    }

    /// Whether this route can list only compiler-minted source-record IDs for
    /// an exact opaque metadata attachment. No source/provenance detail is
    /// implied by this separate default-deny capability.
    fn debugger_static_metadata_source_inventory_available(&self) -> bool {
        false
    }

    /// Whether this route can disclose one source-free module identity and
    /// labeled SHA-256 digest for an ID returned by the distinct inventory.
    /// Source text and all other metadata records remain unavailable.
    fn debugger_static_metadata_source_provenance_available(&self) -> bool {
        false
    }

    /// Whether this route can list only compiler-minted type-record IDs for
    /// one exact opaque metadata attachment. Type displays and static-record
    /// reads remain separately default-denied.
    fn debugger_static_metadata_type_inventory_available(&self) -> bool {
        false
    }

    /// Whether this route can disclose one bounded compiler-produced display
    /// for a prior type-ID inventory receipt. This remains separately
    /// default-denied because displays can contain project-authored names.
    fn debugger_static_metadata_type_display_available(&self) -> bool {
        false
    }

    /// Whether this route can list payload-free compiler-minted symbol IDs
    /// under an exact opaque metadata parent. Symbol record reads remain
    /// separately default-denied.
    fn debugger_static_metadata_symbol_inventory_available(&self) -> bool {
        false
    }

    /// Whether this route can disclose a source-text-free half-open byte
    /// range for a prior exact symbol and independently inventoried source.
    /// This is separate from symbol names and source provenance because it
    /// exposes source structure.
    fn debugger_static_metadata_symbol_location_available(&self) -> bool {
        false
    }

    /// Whether this route can bind an exact verified BlueTS safe point to a
    /// separately receipted source ID under one opaque metadata attachment.
    /// This is not a public grant by itself.
    fn debugger_static_metadata_safe_point_span_available(&self) -> bool {
        false
    }

    /// Whether this executor can inspect only a terminal, exact BlueTS
    /// exception location. Public owner/client authorization is separate.
    fn debugger_exception_location_available(&self) -> bool {
        false
    }

    /// Whether this route can resolve a bounded original BlueTS position
    /// under one exact metadata/source tuple. A separate public owner and
    /// session capability must authorize this source-position oracle.
    fn debugger_static_metadata_source_breakpoint_available(&self) -> bool {
        false
    }

    /// Whether this route can disclose a separately authorized contract
    /// declaration range under receipted contract and source IDs.
    fn debugger_static_metadata_contract_location_available(&self) -> bool {
        false
    }

    /// Whether this route can verify a relation between separately
    /// inventoried opaque symbol and type IDs under one static attachment.
    fn debugger_static_metadata_symbol_type_available(&self) -> bool {
        false
    }

    /// Whether this route can verify a relation between separately
    /// inventoried opaque symbol and contract IDs under one attachment.
    fn debugger_static_metadata_symbol_contract_available(&self) -> bool {
        false
    }

    /// Whether this route can list payload-free compiler-minted contract IDs
    /// under an exact opaque metadata parent. Contract record reads remain
    /// separately default-denied.
    fn debugger_static_metadata_contract_inventory_available(&self) -> bool {
        false
    }

    /// Whether this route can describe a bounded display for a prior exact
    /// contract ID. Contract names remain an independently default-denied
    /// surface; plans and validation behavior do not cross this boundary.
    fn debugger_static_metadata_contract_display_available(&self) -> bool {
        false
    }

    /// Whether this route can validate one data-only snapshot against a prior
    /// exact contract-ID receipt. The boolean outcome is independently
    /// default-denied; plan and structural failure detail stay unavailable.
    fn debugger_static_metadata_contract_validation_available(&self) -> bool {
        false
    }

    /// Whether this route can describe a bounded display for a prior exact
    /// symbol ID. Symbol names remain an independently default-denied surface.
    fn debugger_static_metadata_symbol_display_available(&self) -> bool {
        false
    }

    /// Lists only opaque public-facing program IDs for one live realm.
    fn debugger_programs(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
    ) -> Result<Vec<JavaScriptPageDebuggerProgram>, JavaScriptPageDebuggerError>;

    /// Lists only core-minted opaque metadata identities for one exact public
    /// program generation. It exposes no static metadata payload; source,
    /// module identity, symbol/type/span/contract, bytecode, VM object, and
    /// runtime value all remain unavailable.
    fn debugger_static_metadata(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _program_handle: u64,
        _program_generation: u64,
    ) -> Result<Vec<JavaScriptPageDebuggerStaticMetadata>, JavaScriptPageDebuggerError> {
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    }

    /// Describes one already inventoried opaque metadata identity. The owner
    /// must verify the exact program and metadata generations before exposing
    /// the bounded summary; it must never use these numbers to synthesize a
    /// source/type/symbol/contract record read.
    fn debugger_static_metadata_summary(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _program_handle: u64,
        _program_generation: u64,
        _metadata_handle: u64,
        _metadata_generation: u64,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataSummary, JavaScriptPageDebuggerError> {
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    }

    /// Returns one verified direct-lowering-map aggregate only after the
    /// caller presented the exact opaque parent metadata attachment. This is
    /// not a source-map/bytecode entry lookup or a general static-record read.
    fn debugger_static_metadata_lowering_summary(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _program_handle: u64,
        _program_generation: u64,
        _metadata_handle: u64,
        _metadata_generation: u64,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataLoweringSummary, JavaScriptPageDebuggerError>
    {
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    }

    /// Lists source-record identities only after the caller presented an
    /// exact metadata attachment. Implementations must not synthesize source
    /// details from the numeric IDs.
    fn debugger_static_metadata_sources(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _program_handle: u64,
        _program_generation: u64,
        _metadata_handle: u64,
        _metadata_generation: u64,
    ) -> Result<Vec<JavaScriptPageDebuggerStaticMetadataSourceId>, JavaScriptPageDebuggerError>
    {
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    }

    /// Describes one inventoried source ID under the same exact opaque parent
    /// metadata attachment. Implementations must never turn this into source
    /// text or an arbitrary-record read surface.
    fn debugger_static_metadata_source_provenance(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _target: JavaScriptPageDebuggerStaticMetadataSourceTarget,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataSourceProvenance, JavaScriptPageDebuggerError>
    {
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    }

    /// Lists type-record identities only after the caller presented an exact
    /// metadata attachment. Implementations must not expose type displays or
    /// synthesize static records from the numeric IDs.
    fn debugger_static_metadata_types(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _program_handle: u64,
        _program_generation: u64,
        _metadata_handle: u64,
        _metadata_generation: u64,
    ) -> Result<Vec<JavaScriptPageDebuggerStaticMetadataTypeId>, JavaScriptPageDebuggerError> {
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    }

    /// Describes one previously inventoried type ID under the same exact
    /// metadata attachment. Implementations must not turn this into a
    /// source/span/symbol/contract or arbitrary-record read surface.
    fn debugger_static_metadata_type_display(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _target: JavaScriptPageDebuggerStaticMetadataTypeTarget,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataTypeDisplay, JavaScriptPageDebuggerError> {
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    }

    /// Lists compiler-minted symbol IDs only after the caller presented an
    /// exact metadata attachment. Implementations must not expose symbol
    /// names, spans, declared types, or synthesize static records from IDs.
    fn debugger_static_metadata_symbols(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _program_handle: u64,
        _program_generation: u64,
        _metadata_handle: u64,
        _metadata_generation: u64,
    ) -> Result<Vec<JavaScriptPageDebuggerStaticMetadataSymbolId>, JavaScriptPageDebuggerError>
    {
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    }

    /// Lists compiler-minted contract IDs only after the caller presented an
    /// exact metadata attachment. Implementations must not expose contract
    /// names, spans, plans, or validate caller data from this inventory route.
    fn debugger_static_metadata_contracts(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _program_handle: u64,
        _program_generation: u64,
        _metadata_handle: u64,
        _metadata_generation: u64,
    ) -> Result<Vec<JavaScriptPageDebuggerStaticMetadataContractId>, JavaScriptPageDebuggerError>
    {
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    }

    /// Describes one compiler-minted contract ID only after the caller
    /// presented that exact metadata attachment and ID. Implementations must
    /// not expose source spans, plans, validation behavior, or static records.
    fn debugger_static_metadata_contract_display(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _target: JavaScriptPageDebuggerStaticMetadataContractTarget,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataContractDisplay, JavaScriptPageDebuggerError>
    {
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    }

    /// Validates one data-only snapshot against an exact contract target. An
    /// implementation must enforce its fixed value limits before touching a
    /// retained plan and return only a boolean, never the input or plan/error
    /// detail.
    fn debugger_static_metadata_contract_validation(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _target: JavaScriptPageDebuggerStaticMetadataContractTarget,
        _value: CompilerContractValue,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataContractValidation, JavaScriptPageDebuggerError>
    {
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    }

    /// Describes one compiler-minted symbol ID only after the caller presented
    /// that exact metadata attachment and ID. Implementations must not expose
    /// source spans, types, contracts, or synthesize static records.
    fn debugger_static_metadata_symbol_display(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _target: JavaScriptPageDebuggerStaticMetadataSymbolTarget,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataSymbolDisplay, JavaScriptPageDebuggerError>
    {
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    }

    /// Describes one source-text-free declaration range for an exact prior
    /// symbol. Implementations must not turn it into source/module/name/type/
    /// contract/bytecode or arbitrary static-record access.
    fn debugger_static_metadata_symbol_location(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _target: JavaScriptPageDebuggerStaticMetadataSymbolLocationTarget,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataSymbolLocation, JavaScriptPageDebuggerError>
    {
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    }

    /// Resolves only the exact safe point and source ID presented under the
    /// live, core-reminted metadata identity. An unbound instruction has no
    /// nearest-source fallback.
    fn debugger_static_metadata_safe_point_span(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _target: JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataSafePointSpan, JavaScriptPageDebuggerError>
    {
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    }

    fn debugger_exception_location(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _target: JavaScriptPageDebuggerExceptionLocationTarget,
    ) -> Result<JavaScriptPageDebuggerExceptionLocation, JavaScriptPageDebuggerError> {
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    }

    fn debugger_static_metadata_source_breakpoint(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _target: JavaScriptPageDebuggerStaticMetadataSourceBreakpointTarget,
    ) -> Result<Option<JavaScriptPageDebuggerSafePoint>, JavaScriptPageDebuggerError> {
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    }

    /// Describes only the source ID and bounded declaration range for one
    /// exact contract. No name, plan, source text, or value may be inferred.
    fn debugger_static_metadata_contract_location(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _target: JavaScriptPageDebuggerStaticMetadataContractLocationTarget,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataContractLocation, JavaScriptPageDebuggerError>
    {
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    }

    /// Verifies one exact symbol/type relation without returning unrequested
    /// IDs or compiler-produced displays.
    fn debugger_static_metadata_symbol_type(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _target: JavaScriptPageDebuggerStaticMetadataSymbolTypeTarget,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataSymbolType, JavaScriptPageDebuggerError> {
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    }

    /// Verifies one exact symbol/contract relation without returning an
    /// unrequested ID, contract plan, or validation result.
    fn debugger_static_metadata_symbol_contract(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _target: JavaScriptPageDebuggerStaticMetadataSymbolContractTarget,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataSymbolContract, JavaScriptPageDebuggerError>
    {
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    }

    /// Lists exact compiler-recorded instruction boundaries for one program.
    fn debugger_safe_points(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
    ) -> Result<Vec<JavaScriptPageDebuggerSafePoint>, JavaScriptPageDebuggerError>;

    /// Revalidates one exact caller-supplied instruction boundary.
    fn validate_debugger_safe_point(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
        code_unit_ordinal: u32,
        bytecode_offset: u32,
    ) -> Result<(), JavaScriptPageDebuggerError>;

    /// Stores one exact previously-discovered safe point. The operation is
    /// idempotent and does not imply an interruption or execution transition.
    fn set_debugger_breakpoint(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _program_handle: u64,
        _program_generation: u64,
        _code_unit_ordinal: u32,
        _bytecode_offset: u32,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    }

    /// Lists exact current breakpoint records without source, bytecode, VM,
    /// or runtime-value data.
    fn debugger_breakpoints(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
    ) -> Result<Vec<JavaScriptPageDebuggerBreakpoint>, JavaScriptPageDebuggerError> {
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    }

    /// Removes one exact current breakpoint record after revalidating the
    /// full program-generation and safe-point tuple.
    fn clear_debugger_breakpoint(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _program_handle: u64,
        _program_generation: u64,
        _code_unit_ordinal: u32,
        _bytecode_offset: u32,
    ) -> Result<bool, JavaScriptPageDebuggerError> {
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    }

    /// Whether this selected child route has the bounded root-classic
    /// execution-control lifecycle installed. This is distinct from ordinary
    /// breakpoint configuration and is false by default.
    fn debugger_execution_control_available(&self) -> bool {
        false
    }

    /// Nested-frame control is a distinct default-deny capability. It never
    /// follows automatically from root execution control or safe-point lists.
    fn debugger_nested_frames_available(&self) -> bool {
        false
    }

    /// Linked-module control is a separate owner-selected capability. It
    /// never follows automatically from same-program nested-frame support.
    fn debugger_linked_frames_available(&self) -> bool {
        false
    }

    /// Arms an exact dependency safe point under one separately identified
    /// entry module. The child validates their installed graph relation before
    /// acknowledging; no public route is opened by this core-facing method.
    fn arm_debugger_linked_nested_safe_point_breakpoint(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _entry: JavaScriptPageDebuggerProgram,
        _dependency: JavaScriptPageDebuggerProgram,
        _safe_point: JavaScriptPageDebuggerSafePoint,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable)
    }

    fn debugger_linked_execution_state(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _entry: JavaScriptPageDebuggerProgram,
    ) -> Result<JavaScriptPageDebuggerLinkedExecutionState, JavaScriptPageDebuggerError> {
        Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable)
    }

    fn debugger_linked_stack_snapshot(
        &mut self,
        _top_frame: JavaScriptPageDebuggerFrame,
        _max_scope_entries: u32,
    ) -> Result<JavaScriptPageDebuggerLinkedStackSnapshot, JavaScriptPageDebuggerError> {
        Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable)
    }

    /// Private complete entry-root lexical slots under one exact linked
    /// dependency/entry pause. It never grants public Scopes or Value access.
    fn debugger_linked_scope_snapshot(
        &mut self,
        _expected_stack: JavaScriptPageDebuggerLinkedStackSnapshot,
    ) -> Result<JavaScriptPageDebuggerLinkedScopeSnapshot, JavaScriptPageDebuggerError> {
        Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable)
    }

    fn resume_debugger_linked_nested_execution(
        &mut self,
        _top_frame: JavaScriptPageDebuggerFrame,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable)
    }

    /// Reads two coordinates atomically only after the debugger stream has
    /// separately authorized the grant and both metadata/source receipts.
    fn debugger_linked_stack_spans(
        &mut self,
        _expected_stack: JavaScriptPageDebuggerLinkedStackSnapshot,
        _access: JavaScriptPageDebuggerLinkedSpanAccess,
    ) -> Result<[JavaScriptPageDebuggerStaticMetadataSafePointSpan; 2], JavaScriptPageDebuggerError>
    {
        Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable)
    }

    fn arm_debugger_nested_safe_point_breakpoint(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _program_handle: u64,
        _program_generation: u64,
        _code_unit_ordinal: u32,
        _bytecode_offset: u32,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable)
    }

    /// `None` means this live program is not currently in a nested frame;
    /// ordinary root/pending/completed state remains separately queryable.
    fn debugger_nested_execution_state(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _program_handle: u64,
        _program_generation: u64,
    ) -> Result<Option<JavaScriptPageDebuggerNestedExecutionState>, JavaScriptPageDebuggerError>
    {
        Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable)
    }

    fn step_debugger_nested_instruction(
        &mut self,
        _frame: JavaScriptPageDebuggerFrame,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable)
    }

    fn resume_debugger_nested_execution(
        &mut self,
        _frame: JavaScriptPageDebuggerFrame,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable)
    }

    /// Private core-facing bounded inspection. Public stack and scope reads
    /// still require their own separately advertised capability gates.
    fn debugger_stack_snapshot(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _program: JavaScriptPageDebuggerProgram,
        _frame: Option<JavaScriptPageDebuggerFrame>,
        _max_frames: u32,
        _max_scope_entries: u32,
    ) -> Result<JavaScriptPageDebuggerStackSnapshot, JavaScriptPageDebuggerError> {
        Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable)
    }

    /// Private core-facing exact active-slot read. A public owner receipt and
    /// grant are separate work; this method advertises no public capability.
    fn debugger_value_snapshot(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _target: JavaScriptPageDebuggerValueTarget,
    ) -> Result<JavaScriptPageDebuggerValuePreview, JavaScriptPageDebuggerError> {
        Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable)
    }

    /// Private static-only paused-slot relation. The public debugger has no
    /// request or grant for it until its independent receipt work is complete.
    fn debugger_static_scope_relation(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _target: JavaScriptPageDebuggerStaticScopeTarget,
    ) -> Result<JavaScriptPageDebuggerStaticScopeRelation, JavaScriptPageDebuggerError> {
        Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable)
    }

    /// Whether this page-host route can perform the private bounded value
    /// read. A public owner/client grant and same-stream receipt remain
    /// separate requirements.
    fn debugger_values_available(&self) -> bool {
        false
    }

    /// Stack locations and active lexical scopes are separately granted on
    /// the owner-selected debugger route; neither follows from stepping.
    fn debugger_stack_available(&self) -> bool {
        false
    }

    fn debugger_scopes_available(&self) -> bool {
        false
    }

    /// Arms one pending classic program at an already validated root safe
    /// point. No generic interruption or nested-function continuation exists.
    fn arm_debugger_root_safe_point_breakpoint(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _program_handle: u64,
        _program_generation: u64,
        _code_unit_ordinal: u32,
        _bytecode_offset: u32,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable)
    }

    /// Reads source-free lifecycle state for one root-classic program.
    fn debugger_execution_state(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _program_handle: u64,
        _program_generation: u64,
    ) -> Result<JavaScriptPageDebuggerExecutionState, JavaScriptPageDebuggerError> {
        Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable)
    }

    /// Marks a paused root-classic continuation for the next child advance.
    fn resume_debugger_execution(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _program_handle: u64,
        _program_generation: u64,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable)
    }

    /// Whether this route can advance one retained classic-root instruction.
    fn debugger_stepping_available(&self) -> bool {
        false
    }

    /// Exact, compiler-bound BlueTS source-span stepping is separate from
    /// ordinary source-free single-instruction stepping.
    fn debugger_source_span_stepping_available(&self) -> bool {
        false
    }

    fn step_debugger_bluets_source_span(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _target: JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable)
    }

    fn step_debugger_root_instruction(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _program_handle: u64,
        _program_generation: u64,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable)
    }
}
