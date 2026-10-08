// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

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

    /// Private child-only terminal exception location. Public debugger policy
    /// and reminting remain separate from this transport capability.
    fn debugger_bluets_exception_location_available(&self) -> bool {
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

    fn debugger_bluets_exception_location(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _program: PageHostDebuggerProgram,
        _metadata: PageHostDebuggerMetadataHandle,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement BlueTS exception locations",
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

    fn debugger_linked_frames_available(&self) -> bool {
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

    fn debugger_value_snapshot_available(&self) -> bool {
        false
    }

    fn debugger_value_snapshot(
        &mut self,
        _target: PageHostDebuggerValueTarget,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement debugger value inspection",
        ))
    }

    fn debugger_linked_value_snapshot(
        &mut self,
        _target: PageHostDebuggerLinkedValueTarget,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement linked debugger value inspection",
        ))
    }

    /// Private static compiler relation at one exact paused slot. Test
    /// transports remain default-denied, and this is not a public grant.
    fn debugger_static_scope_relation(
        &mut self,
        _target: PageHostDebuggerStaticScopeTarget,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement static scope relations",
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

    fn arm_debugger_linked_nested_safe_point_breakpoint(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _entry_program: PageHostDebuggerProgram,
        _safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement linked debugger control",
        ))
    }

    fn debugger_linked_stack_snapshot(
        &mut self,
        _frame: PageHostDebuggerLinkedFrame,
        _max_scope_entries: u32,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement linked debugger stack inspection",
        ))
    }

    fn debugger_linked_stack_spans(
        &mut self,
        _frame: PageHostDebuggerLinkedFrame,
        _expected_stack: PageHostDebuggerLinkedStackSnapshot,
        _sources: [PageHostDebuggerLinkedSource; 2],
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement linked debugger source spans",
        ))
    }

    fn resume_debugger_linked_nested_execution(
        &mut self,
        _frame: PageHostDebuggerLinkedFrame,
    ) -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "page-host child does not implement linked debugger resume",
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

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_ipc::page_host::PageHostDebuggerLinkedStackFrame;

    /// Implements only the two required methods, leaving every optional
    /// method at its trait default — the "test doubles must opt in" pattern
    /// this trait's own doc comments describe, deliberately inverted from
    /// `RecordingChild` (which opts a few methods in) so every default body
    /// in this file is actually reachable from a test.
    struct MinimalClient;

    impl PageHostClient for MinimalClient {
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
    }

    fn document() -> PageHostDocument {
        PageHostDocument {
            tab_id: 1,
            document_generation: 1,
            snapshot: PageHostDocumentSnapshot {
                document_text: String::new(),
                document_origin: String::new(),
            },
            debugger_execution_control: false,
            scripts: Vec::new(),
        }
    }

    fn program() -> PageHostDebuggerProgram {
        PageHostDebuggerProgram {
            program_handle: 0,
            program_generation: 0,
        }
    }

    fn metadata() -> PageHostDebuggerMetadataHandle {
        PageHostDebuggerMetadataHandle {
            metadata_handle: 0,
            metadata_generation: 0,
        }
    }

    fn safe_point() -> PageHostDebuggerSafePoint {
        PageHostDebuggerSafePoint {
            program: program(),
            code_unit_ordinal: 0,
            bytecode_offset: 0,
        }
    }

    fn frame() -> PageHostDebuggerFrame {
        PageHostDebuggerFrame {
            tab_id: 0,
            document_generation: 0,
            program: program(),
            code_unit_ordinal: 0,
            invocation_serial: 0,
        }
    }

    fn linked_frame() -> PageHostDebuggerLinkedFrame {
        PageHostDebuggerLinkedFrame {
            tab_id: 0,
            document_generation: 0,
            entry_program: program(),
            dependency_program: program(),
            code_unit_ordinal: 0,
            invocation_serial: 0,
        }
    }

    fn linked_stack_snapshot() -> PageHostDebuggerLinkedStackSnapshot {
        let stack_frame = PageHostDebuggerLinkedStackFrame {
            safe_point: safe_point(),
            scope_entries: Vec::new(),
            scope_truncated: false,
        };
        PageHostDebuggerLinkedStackSnapshot {
            frames: [stack_frame.clone(), stack_frame],
            stack_truncated: false,
            max_scope_entries: 0,
        }
    }

    fn linked_source() -> PageHostDebuggerLinkedSource {
        PageHostDebuggerLinkedSource {
            metadata: metadata(),
            source_id: 0,
        }
    }

    fn scope_entry() -> PageHostDebuggerScopeEntry {
        PageHostDebuggerScopeEntry {
            slot_ordinal: 0,
            scope_depth: 0,
        }
    }

    fn value_target() -> PageHostDebuggerValueTarget {
        PageHostDebuggerValueTarget {
            tab_id: 0,
            document_generation: 0,
            program: program(),
            frame: None,
            frame_index: 0,
            safe_point: safe_point(),
            scope_entry: scope_entry(),
        }
    }

    fn linked_value_target() -> PageHostDebuggerLinkedValueTarget {
        PageHostDebuggerLinkedValueTarget {
            frame: linked_frame(),
            expected_stack: Box::new(linked_stack_snapshot()),
            frame_index: 0,
            scope_entry: scope_entry(),
        }
    }

    fn static_scope_target() -> PageHostDebuggerStaticScopeTarget {
        PageHostDebuggerStaticScopeTarget::Ordinary {
            metadata: metadata(),
            target: value_target(),
        }
    }

    #[track_caller]
    fn assert_unsupported(result: io::Result<PageHostReply>, message: &str) {
        let error = result.expect_err("default implementation must return an error");
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
        assert_eq!(error.to_string(), message);
    }

    #[test]
    fn default_capability_flags_are_all_false() {
        let client = MinimalClient;
        assert!(!client.debugger_breakpoint_configuration_available());
        assert!(!client.debugger_bluets_metadata_available());
        assert!(!client.debugger_bluets_metadata_summary_available());
        assert!(!client.debugger_bluets_metadata_lowering_summary_available());
        assert!(!client.debugger_bluets_metadata_sources_available());
        assert!(!client.debugger_bluets_metadata_source_provenance_available());
        assert!(!client.debugger_bluets_metadata_types_available());
        assert!(!client.debugger_bluets_metadata_type_display_available());
        assert!(!client.debugger_bluets_metadata_symbols_available());
        assert!(!client.debugger_bluets_metadata_contracts_available());
        assert!(!client.debugger_bluets_metadata_contract_display_available());
        assert!(!client.debugger_bluets_metadata_contract_validation_available());
        assert!(!client.debugger_bluets_metadata_symbol_display_available());
        assert!(!client.debugger_bluets_metadata_symbol_location_available());
        assert!(!client.debugger_bluets_metadata_contract_location_available());
        assert!(!client.debugger_bluets_safe_point_span_available());
        assert!(!client.debugger_bluets_exception_location_available());
        assert!(!client.debugger_bluets_source_breakpoint_available());
        assert!(!client.debugger_bluets_source_span_step_available());
        assert!(!client.debugger_bluets_metadata_symbol_type_available());
        assert!(!client.debugger_bluets_metadata_symbol_contract_available());
        assert!(!client.debugger_execution_control_available());
        assert!(!client.debugger_nested_frames_available());
        assert!(!client.debugger_linked_frames_available());
        assert!(!client.debugger_stack_snapshot_available());
        assert!(!client.debugger_value_snapshot_available());
        assert!(!client.debugger_stepping_available());
    }

    #[test]
    fn default_dispatch_click_with_script_pump_is_unsupported() {
        let mut client = MinimalClient;
        let mut pump_called = false;
        let mut pump = || {
            pump_called = true;
            Ok(())
        };
        assert_unsupported(
            client.dispatch_click_with_script_pump(1, 1, 1, &mut pump),
            "page-host child does not implement click dispatch",
        );
        assert!(!pump_called, "default must not invoke the pump");
    }

    #[test]
    fn default_debugger_realm_stats_is_unsupported() {
        let mut client = MinimalClient;
        assert_unsupported(
            client.debugger_realm_stats(1, 1),
            "page-host child does not implement debugger locations",
        );
    }

    #[test]
    fn default_child_stats_is_unsupported() {
        let mut client = MinimalClient;
        assert_unsupported(
            client.child_stats(),
            "page-host child does not implement child-wide accounting",
        );
    }

    #[test]
    fn default_debugger_programs_is_unsupported() {
        let mut client = MinimalClient;
        assert_unsupported(
            client.debugger_programs(1, 1),
            "page-host child does not implement debugger locations",
        );
    }

    #[test]
    fn default_bluets_metadata_operations_are_unsupported() {
        let mut client = MinimalClient;
        assert_unsupported(
            client.debugger_bluets_metadata(1, 1, program()),
            "page-host child does not implement BlueTS debugger metadata inventory",
        );
        assert_unsupported(
            client.debugger_bluets_metadata_summary(1, 1, program(), metadata()),
            "page-host child does not implement BlueTS debugger metadata summaries",
        );
        assert_unsupported(
            client.debugger_bluets_metadata_lowering_summary(1, 1, program(), metadata()),
            "page-host child does not implement BlueTS debugger lowering summaries",
        );
        assert_unsupported(
            client.debugger_bluets_metadata_sources(1, 1, program(), metadata()),
            "page-host child does not implement BlueTS debugger source inventories",
        );
        assert_unsupported(
            client.debugger_bluets_metadata_source_provenance(1, 1, program(), metadata(), 0),
            "page-host child does not implement BlueTS debugger source provenance",
        );
        assert_unsupported(
            client.debugger_bluets_metadata_types(1, 1, program(), metadata()),
            "page-host child does not implement BlueTS debugger type inventories",
        );
        assert_unsupported(
            client.debugger_bluets_metadata_type_display(1, 1, program(), metadata(), 0),
            "page-host child does not implement BlueTS debugger type displays",
        );
        assert_unsupported(
            client.debugger_bluets_metadata_symbols(1, 1, program(), metadata()),
            "page-host child does not implement BlueTS debugger symbol inventories",
        );
        assert_unsupported(
            client.debugger_bluets_metadata_contracts(1, 1, program(), metadata()),
            "page-host child does not implement BlueTS debugger contract inventories",
        );
        assert_unsupported(
            client.debugger_bluets_metadata_contract_display(1, 1, program(), metadata(), 0),
            "page-host child does not implement BlueTS debugger contract displays",
        );
        assert_unsupported(
            client.debugger_bluets_metadata_contract_validation(
                1,
                1,
                program(),
                metadata(),
                0,
                CompilerContractValue::Null,
            ),
            "page-host child does not implement BlueTS debugger contract validation",
        );
        assert_unsupported(
            client.debugger_bluets_metadata_symbol_display(1, 1, program(), metadata(), 0),
            "page-host child does not implement BlueTS debugger symbol displays",
        );
        assert_unsupported(
            client.debugger_bluets_metadata_symbol_location(1, 1, program(), metadata(), 0),
            "page-host child does not implement BlueTS debugger symbol locations",
        );
        assert_unsupported(
            client.debugger_bluets_metadata_contract_location(1, 1, program(), metadata(), 0),
            "page-host child does not implement BlueTS debugger contract locations",
        );
        assert_unsupported(
            client.debugger_bluets_safe_point_span(1, 1, metadata(), safe_point()),
            "page-host child does not implement exact BlueTS safe-point spans",
        );
        assert_unsupported(
            client.debugger_bluets_exception_location(1, 1, program(), metadata()),
            "page-host child does not implement BlueTS exception locations",
        );
        assert_unsupported(
            client.debugger_bluets_source_breakpoint(1, 1, program(), metadata(), 0, 0),
            "page-host child does not implement BlueTS source breakpoints",
        );
        assert_unsupported(
            client.debugger_bluets_metadata_symbol_type(1, 1, program(), metadata(), 0, 0),
            "page-host child does not implement BlueTS debugger symbol types",
        );
        assert_unsupported(
            client.debugger_bluets_metadata_symbol_contract(1, 1, program(), metadata(), 0, 0),
            "page-host child does not implement BlueTS debugger symbol contracts",
        );
    }

    #[test]
    fn default_breakpoint_operations_are_unsupported() {
        let mut client = MinimalClient;
        assert_unsupported(
            client.debugger_safe_points(1, 1, program()),
            "page-host child does not implement debugger locations",
        );
        assert_unsupported(
            client.validate_debugger_safe_point(1, 1, safe_point()),
            "page-host child does not implement debugger locations",
        );
        assert_unsupported(
            client.set_debugger_breakpoint(1, 1, safe_point()),
            "page-host child does not implement debugger breakpoint configuration",
        );
        assert_unsupported(
            client.debugger_breakpoints(1, 1),
            "page-host child does not implement debugger breakpoint configuration",
        );
        assert_unsupported(
            client.clear_debugger_breakpoint(1, 1, safe_point()),
            "page-host child does not implement debugger breakpoint configuration",
        );
    }

    #[test]
    fn default_stack_and_value_inspection_are_unsupported() {
        let mut client = MinimalClient;
        assert_unsupported(
            client.debugger_stack_snapshot(1, 1, program(), None, 0, 0),
            "page-host child does not implement debugger stack inspection",
        );
        assert_unsupported(
            client.debugger_value_snapshot(value_target()),
            "page-host child does not implement debugger value inspection",
        );
        assert_unsupported(
            client.debugger_linked_value_snapshot(linked_value_target()),
            "page-host child does not implement linked debugger value inspection",
        );
        assert_unsupported(
            client.debugger_static_scope_relation(static_scope_target()),
            "page-host child does not implement static scope relations",
        );
    }

    #[test]
    fn default_nested_and_linked_control_are_unsupported() {
        let mut client = MinimalClient;
        assert_unsupported(
            client.arm_debugger_nested_safe_point_breakpoint(1, 1, safe_point()),
            "page-host child does not implement nested debugger control",
        );
        assert_unsupported(
            client.step_debugger_nested_instruction(frame()),
            "page-host child does not implement nested debugger control",
        );
        assert_unsupported(
            client.resume_debugger_nested_execution(frame()),
            "page-host child does not implement nested debugger resume",
        );
        assert_unsupported(
            client
                .arm_debugger_linked_nested_safe_point_breakpoint(1, 1, program(), safe_point()),
            "page-host child does not implement linked debugger control",
        );
        assert_unsupported(
            client.debugger_linked_stack_snapshot(linked_frame(), 0),
            "page-host child does not implement linked debugger stack inspection",
        );
        assert_unsupported(
            client.debugger_linked_stack_spans(
                linked_frame(),
                linked_stack_snapshot(),
                [linked_source(), linked_source()],
            ),
            "page-host child does not implement linked debugger source spans",
        );
        assert_unsupported(
            client.resume_debugger_linked_nested_execution(linked_frame()),
            "page-host child does not implement linked debugger resume",
        );
    }

    #[test]
    fn default_root_execution_control_is_unsupported() {
        let mut client = MinimalClient;
        assert_unsupported(
            client.arm_debugger_root_safe_point_breakpoint(1, 1, safe_point()),
            "page-host child does not implement debugger execution control",
        );
        assert_unsupported(
            client.debugger_execution_state(1, 1, program()),
            "page-host child does not implement debugger execution control",
        );
        assert_unsupported(
            client.resume_debugger_execution(1, 1, program()),
            "page-host child does not implement debugger execution control",
        );
        assert_unsupported(
            client.step_debugger_root_instruction(1, 1, program()),
            "page-host child does not implement debugger root stepping",
        );
        assert_unsupported(
            client.step_debugger_bluets_source_span(1, 1, metadata(), 0, safe_point()),
            "page-host child does not implement BlueTS source-span stepping",
        );
        assert_unsupported(
            client.advance_debugger_execution(1, 1),
            "page-host child does not implement debugger execution control",
        );
    }

    #[test]
    fn required_methods_are_reachable_through_the_trait() {
        let mut client = MinimalClient;
        assert_eq!(
            client.synchronize_document(document()).unwrap(),
            PageHostReply::Synchronized {
                tab_id: 1,
                document_generation: 1,
                already_current: false,
                reports: Vec::new(),
            }
        );
        assert_eq!(
            client.close_realm(1, 1).unwrap(),
            PageHostReply::RealmClosed {
                tab_id: 1,
                document_generation: 1,
            }
        );
    }

    #[test]
    fn synchronize_document_with_script_pump_delegates_without_invoking_the_pump() {
        let mut client = MinimalClient;
        let mut pump_called = false;
        let mut pump = || {
            pump_called = true;
            Ok(())
        };
        let reply = client
            .synchronize_document_with_script_pump(document(), &mut pump)
            .unwrap();
        assert_eq!(
            reply,
            PageHostReply::Synchronized {
                tab_id: 1,
                document_generation: 1,
                already_current: false,
                reports: Vec::new(),
            }
        );
        assert!(!pump_called, "default delegate must not invoke the pump");
    }

    #[test]
    fn realm_stats_delegates_to_debugger_realm_stats() {
        let mut client = MinimalClient;
        assert_unsupported(
            client.realm_stats(1, 1),
            "page-host child does not implement debugger locations",
        );
    }

    #[test]
    fn advance_debugger_execution_with_script_pump_delegates_without_invoking_the_pump() {
        let mut client = MinimalClient;
        let mut pump_called = false;
        let mut pump = || {
            pump_called = true;
            Ok(())
        };
        assert_unsupported(
            client.advance_debugger_execution_with_script_pump(1, 1, &mut pump),
            "page-host child does not implement debugger execution control",
        );
        assert!(!pump_called, "default delegate must not invoke the pump");
    }
}
