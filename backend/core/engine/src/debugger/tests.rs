// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use blueice_ipc::debugger::DebuggerSourceCoordinates;
mod linked_coordinates;
mod metadata_grants;
mod metadata_locations;
mod metadata_relations;
mod native_execution;
mod value_scopes;

struct MetadataLocations {
    malformed_summary: bool,
    malformed_provenance: bool,
    malformed_lowering_summary: bool,
    mismatched_symbol_display: bool,
    mismatched_contract_display: bool,
    mismatched_contract_validation: bool,
}

impl PageJavaScriptDebuggerLocations for MetadataLocations {
    fn debugger_has_live_realm(&mut self, _tab_id: TabId, _document_generation: u64) -> bool {
        true
    }

    fn max_debugger_safe_points_per_program(&self) -> usize {
        1
    }

    fn debugger_static_metadata_inventory_available(&self) -> bool {
        true
    }

    fn debugger_static_metadata_summary_available(&self) -> bool {
        true
    }

    fn debugger_static_metadata_lowering_summary_available(&self) -> bool {
        true
    }

    fn debugger_static_metadata_source_inventory_available(&self) -> bool {
        true
    }

    fn debugger_static_metadata_source_provenance_available(&self) -> bool {
        true
    }

    fn debugger_static_metadata_symbol_inventory_available(&self) -> bool {
        true
    }

    fn debugger_static_metadata_symbol_display_available(&self) -> bool {
        true
    }

    fn debugger_static_metadata_symbol_location_available(&self) -> bool {
        true
    }

    fn debugger_static_metadata_safe_point_span_available(&self) -> bool {
        true
    }

    fn debugger_exception_location_available(&self) -> bool {
        true
    }

    fn debugger_static_metadata_source_breakpoint_available(&self) -> bool {
        true
    }

    fn debugger_static_metadata_contract_location_available(&self) -> bool {
        true
    }

    fn debugger_static_metadata_type_inventory_available(&self) -> bool {
        true
    }

    fn debugger_static_metadata_symbol_type_available(&self) -> bool {
        true
    }

    fn debugger_static_metadata_symbol_contract_available(&self) -> bool {
        true
    }

    fn debugger_static_metadata_contract_inventory_available(&self) -> bool {
        true
    }

    fn debugger_static_metadata_contract_display_available(&self) -> bool {
        true
    }

    fn debugger_static_metadata_contract_validation_available(&self) -> bool {
        true
    }

    fn debugger_programs(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
    ) -> Result<
        Vec<crate::script::javascript::JavaScriptPageDebuggerProgram>,
        JavaScriptPageDebuggerError,
    > {
        Ok(Vec::new())
    }

    fn debugger_static_metadata(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _program_handle: u64,
        _program_generation: u64,
    ) -> Result<
        Vec<crate::script::javascript::JavaScriptPageDebuggerStaticMetadata>,
        JavaScriptPageDebuggerError,
    > {
        Ok(vec![
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadata {
                metadata_handle: 41,
                metadata_generation: 9,
            },
        ])
    }

    fn debugger_static_metadata_summary(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _program_handle: u64,
        _program_generation: u64,
        metadata_handle: u64,
        metadata_generation: u64,
    ) -> Result<
        crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSummary,
        JavaScriptPageDebuggerError,
    > {
        if metadata_handle != 41 || metadata_generation != 9 {
            return Err(JavaScriptPageDebuggerError::UnknownProgram);
        }
        Ok(
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSummary {
                language_version: if self.malformed_summary {
                    "x".repeat(
                        blueice_ipc::debugger::DEBUGGER_STATIC_METADATA_LANGUAGE_VERSION_MAX_BYTES
                            + 1,
                    )
                } else {
                    "blue-ts-0.1".to_string()
                },
                compiler_options_hash: "0123456789abcdef".to_string(),
                source_count: 1,
                type_count: 2,
                symbol_count: 3,
                contract_count: 4,
            },
        )
    }

    fn debugger_static_metadata_lowering_summary(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _program_handle: u64,
        _program_generation: u64,
        metadata_handle: u64,
        metadata_generation: u64,
    ) -> Result<
        crate::script::javascript::JavaScriptPageDebuggerStaticMetadataLoweringSummary,
        JavaScriptPageDebuggerError,
    > {
        if metadata_handle != 41 || metadata_generation != 9 {
            return Err(JavaScriptPageDebuggerError::UnknownProgram);
        }
        Ok(
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataLoweringSummary {
                safe_point_map_abi: if self.malformed_lowering_summary {
                    "unexpected-child-label".to_string()
                } else {
                    blueice_ipc::debugger::DEBUGGER_STATIC_METADATA_SAFE_POINT_MAP_ABI_V1
                        .to_string()
                },
                program_abi: blueice_ipc::debugger::DEBUGGER_STATIC_METADATA_PROGRAM_ABI_V1
                    .to_string(),
                source_set_hash: "bts-source-set-0123456789abcdef".to_string(),
                bound_safe_point_count: 1,
            },
        )
    }

    fn debugger_static_metadata_sources(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _program_handle: u64,
        _program_generation: u64,
        metadata_handle: u64,
        metadata_generation: u64,
    ) -> Result<
        Vec<crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSourceId>,
        JavaScriptPageDebuggerError,
    > {
        if metadata_handle != 41 || metadata_generation != 9 {
            return Err(JavaScriptPageDebuggerError::UnknownProgram);
        }
        Ok(vec![
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSourceId {
                source_id: 0,
            },
        ])
    }

    fn debugger_static_metadata_source_provenance(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        target: crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSourceTarget,
    ) -> Result<
        crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSourceProvenance,
        JavaScriptPageDebuggerError,
    > {
        if target.metadata_handle != 41 || target.metadata_generation != 9 || target.source_id != 0
        {
            return Err(JavaScriptPageDebuggerError::UnknownProgram);
        }
        Ok(
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSourceProvenance {
                source_id: target.source_id,
                module: if self.malformed_provenance {
                    "file:///private/main.ts".to_string()
                } else {
                    "page:///main.ts".to_string()
                },
                content_hash:
                    "bts-sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
                        .to_string(),
            },
        )
    }

    fn debugger_static_metadata_symbols(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _program_handle: u64,
        _program_generation: u64,
        metadata_handle: u64,
        metadata_generation: u64,
    ) -> Result<
        Vec<crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSymbolId>,
        JavaScriptPageDebuggerError,
    > {
        if metadata_handle != 41 || metadata_generation != 9 {
            return Err(JavaScriptPageDebuggerError::UnknownProgram);
        }
        Ok(vec![
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSymbolId {
                symbol_id: 0,
            },
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSymbolId {
                symbol_id: 1,
            },
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSymbolId {
                symbol_id: 2,
            },
        ])
    }

    fn debugger_static_metadata_types(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _program_handle: u64,
        _program_generation: u64,
        metadata_handle: u64,
        metadata_generation: u64,
    ) -> Result<
        Vec<crate::script::javascript::JavaScriptPageDebuggerStaticMetadataTypeId>,
        JavaScriptPageDebuggerError,
    > {
        if metadata_handle != 41 || metadata_generation != 9 {
            return Err(JavaScriptPageDebuggerError::UnknownProgram);
        }
        Ok(vec![
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataTypeId { type_id: 0 },
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataTypeId { type_id: 1 },
        ])
    }

    fn debugger_static_metadata_symbol_display(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        target: crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSymbolTarget,
    ) -> Result<
        crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSymbolDisplay,
        JavaScriptPageDebuggerError,
    > {
        if target.metadata_handle != 41 || target.metadata_generation != 9 || target.symbol_id != 0
        {
            return Err(JavaScriptPageDebuggerError::UnknownProgram);
        }
        Ok(
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSymbolDisplay {
                symbol_id: if self.mismatched_symbol_display {
                    target.symbol_id + 1
                } else {
                    target.symbol_id
                },
                display: "ProjectControlledName".to_string(),
                kind: blueice_ipc::debugger::DebuggerStaticMetadataSymbolKind::Interface,
                exported: true,
            },
        )
    }

    fn debugger_static_metadata_symbol_location(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        target: crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSymbolLocationTarget,
    ) -> Result<
        crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSymbolLocation,
        JavaScriptPageDebuggerError,
    > {
        if target.metadata_handle != 41
            || target.metadata_generation != 9
            || target.symbol_id != 0
            || target.source_id != 0
        {
            return Err(JavaScriptPageDebuggerError::UnknownProgram);
        }
        Ok(
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSymbolLocation {
                symbol_id: target.symbol_id,
                source_id: target.source_id,
                start_byte: 6,
                end_byte: 31,
                coordinates: DebuggerSourceCoordinates {
                    start_line: 0,
                    start_column_utf16: 6,
                    end_line: 0,
                    end_column_utf16: 31,
                },
            },
        )
    }

    fn debugger_static_metadata_safe_point_span(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        target: crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget,
    ) -> Result<
        crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSafePointSpan,
        JavaScriptPageDebuggerError,
    > {
        if target.program_handle != 7
            || target.program_generation != 3
            || target.metadata_handle != 41
            || target.metadata_generation != 9
            || target.source_id != 0
            || !matches!(target.code_unit_ordinal, 0 | 1)
            || target.bytecode_offset != 4
        {
            return Err(JavaScriptPageDebuggerError::UnknownProgram);
        }
        Ok(
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSafePointSpan {
                source_id: 0,
                start_byte: 6,
                end_byte: 31,
                coordinates: blueice_ipc::debugger::DebuggerSourceCoordinates {
                    start_line: 0,
                    start_column_utf16: 6,
                    end_line: 0,
                    end_column_utf16: 31,
                },
            },
        )
    }

    fn debugger_exception_location(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        target: JavaScriptPageDebuggerExceptionLocationTarget,
    ) -> Result<
        crate::script::javascript::JavaScriptPageDebuggerExceptionLocation,
        JavaScriptPageDebuggerError,
    > {
        if target.source_id == 1 {
            return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
        }
        if target.program_handle != 7
            || target.program_generation != 3
            || target.metadata_handle != 41
            || target.metadata_generation != 9
            || target.source_id != 0
        {
            return Err(JavaScriptPageDebuggerError::UnknownProgram);
        }
        Ok(
            crate::script::javascript::JavaScriptPageDebuggerExceptionLocation {
                source_id: 0,
                code_unit_ordinal: 1,
                bytecode_offset: 4,
                start_byte: 6,
                end_byte: 31,
                coordinates: DebuggerSourceCoordinates {
                    start_line: 0,
                    start_column_utf16: 6,
                    end_line: 0,
                    end_column_utf16: 31,
                },
            },
        )
    }

    fn debugger_static_metadata_source_breakpoint(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        target: crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSourceBreakpointTarget,
    ) -> Result<
        Option<crate::script::javascript::JavaScriptPageDebuggerSafePoint>,
        JavaScriptPageDebuggerError,
    > {
        if target.program_handle != 7
            || target.program_generation != 3
            || target.metadata_handle != 41
            || target.metadata_generation != 9
            || target.source_id != 0
        {
            return Err(JavaScriptPageDebuggerError::UnknownProgram);
        }
        Ok((target.source_byte < 31).then_some(
            crate::script::javascript::JavaScriptPageDebuggerSafePoint {
                code_unit_ordinal: 0,
                bytecode_offset: if target.source_byte == 7 { 5 } else { 4 },
            },
        ))
    }

    fn debugger_static_metadata_contract_location(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        target: crate::script::javascript::JavaScriptPageDebuggerStaticMetadataContractLocationTarget,
    ) -> Result<
        crate::script::javascript::JavaScriptPageDebuggerStaticMetadataContractLocation,
        JavaScriptPageDebuggerError,
    > {
        if target.metadata_handle != 41
            || target.metadata_generation != 9
            || target.contract_id != 0
            || target.source_id != 0
        {
            return Err(JavaScriptPageDebuggerError::UnknownProgram);
        }
        Ok(
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataContractLocation {
                contract_id: target.contract_id,
                source_id: target.source_id,
                start_byte: 6,
                end_byte: 31,
                coordinates: DebuggerSourceCoordinates {
                    start_line: 0,
                    start_column_utf16: 6,
                    end_line: 0,
                    end_column_utf16: 31,
                },
            },
        )
    }

    fn debugger_static_metadata_symbol_type(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        target: crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSymbolTypeTarget,
    ) -> Result<
        crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSymbolType,
        JavaScriptPageDebuggerError,
    > {
        if target.metadata_handle != 41
            || target.metadata_generation != 9
            || target.symbol_id != 0
            || target.type_id != 1
        {
            return Err(JavaScriptPageDebuggerError::UnknownProgram);
        }
        Ok(
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSymbolType {
                symbol_id: target.symbol_id,
                type_id: target.type_id,
            },
        )
    }

    fn debugger_static_metadata_symbol_contract(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        target: crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSymbolContractTarget,
    ) -> Result<
        crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSymbolContract,
        JavaScriptPageDebuggerError,
    > {
        if target.metadata_handle != 41
            || target.metadata_generation != 9
            || target.symbol_id != 0
            || target.contract_id != 1
        {
            return Err(JavaScriptPageDebuggerError::UnknownProgram);
        }
        Ok(
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSymbolContract {
                symbol_id: target.symbol_id,
                contract_id: target.contract_id,
            },
        )
    }

    fn debugger_static_metadata_contracts(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _program_handle: u64,
        _program_generation: u64,
        metadata_handle: u64,
        metadata_generation: u64,
    ) -> Result<
        Vec<crate::script::javascript::JavaScriptPageDebuggerStaticMetadataContractId>,
        JavaScriptPageDebuggerError,
    > {
        if metadata_handle != 41 || metadata_generation != 9 {
            return Err(JavaScriptPageDebuggerError::UnknownProgram);
        }
        Ok(vec![
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataContractId {
                contract_id: 0,
            },
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataContractId {
                contract_id: 1,
            },
        ])
    }

    fn debugger_static_metadata_contract_display(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        target: crate::script::javascript::JavaScriptPageDebuggerStaticMetadataContractTarget,
    ) -> Result<
        crate::script::javascript::JavaScriptPageDebuggerStaticMetadataContractDisplay,
        JavaScriptPageDebuggerError,
    > {
        if target.metadata_handle != 41
            || target.metadata_generation != 9
            || target.contract_id != 0
        {
            return Err(JavaScriptPageDebuggerError::UnknownProgram);
        }
        Ok(
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataContractDisplay {
                contract_id: if self.mismatched_contract_display {
                    target.contract_id + 1
                } else {
                    target.contract_id
                },
                display: "ProjectControlledContract".to_string(),
                root_kind: blueice_ipc::debugger::DebuggerStaticMetadataContractRootKind::Record,
            },
        )
    }

    fn debugger_static_metadata_contract_validation(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        target: crate::script::javascript::JavaScriptPageDebuggerStaticMetadataContractTarget,
        value: CompilerContractValue,
    ) -> Result<
        crate::script::javascript::JavaScriptPageDebuggerStaticMetadataContractValidation,
        JavaScriptPageDebuggerError,
    > {
        if target.metadata_handle != 41
            || target.metadata_generation != 9
            || target.contract_id != 0
        {
            return Err(JavaScriptPageDebuggerError::UnknownProgram);
        }
        Ok(
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataContractValidation {
                contract_id: if self.mismatched_contract_validation {
                    target.contract_id + 1
                } else {
                    target.contract_id
                },
                valid: matches!(value, CompilerContractValue::Boolean(true)),
            },
        )
    }

    fn debugger_safe_points(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _program_handle: u64,
        _program_generation: u64,
    ) -> Result<
        Vec<crate::script::javascript::JavaScriptPageDebuggerSafePoint>,
        JavaScriptPageDebuggerError,
    > {
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    }

    fn validate_debugger_safe_point(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        program_handle: u64,
        program_generation: u64,
        code_unit_ordinal: u32,
        bytecode_offset: u32,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        if (
            program_handle,
            program_generation,
            code_unit_ordinal,
            bytecode_offset,
        ) == (7, 3, 0, 4)
            || (
                program_handle,
                program_generation,
                code_unit_ordinal,
                bytecode_offset,
            ) == (7, 3, 1, 4)
        {
            Ok(())
        } else {
            Err(JavaScriptPageDebuggerError::NoLiveRealm)
        }
    }
}

fn loaded_tabs() -> (TabManager, DebuggerPageRealm) {
    let mut tabs = TabManager::new(320.0, 200.0);
    let tab_id = tabs.default_tab();
    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<main>debugger target</main>",
        Some("https://example.test/".to_string()),
    );
    (
        tabs,
        DebuggerPageRealm {
            browser_context_id: DEFAULT_BROWSER_CONTEXT_ID,
            tab_id: tab_id.as_u64(),
            realm_generation: 1,
        },
    )
}

struct StackCoordinateLocations {
    moved_root_offset: u32,
    unbound_root: bool,
    span_calls: usize,
    value_preview: Option<JavaScriptPageDebuggerValuePreview>,
    value_calls: usize,
}

impl PageJavaScriptDebuggerLocations for StackCoordinateLocations {
    fn debugger_has_live_realm(&mut self, _tab_id: TabId, _generation: u64) -> bool {
        true
    }

    fn max_debugger_safe_points_per_program(&self) -> usize {
        2
    }

    fn debugger_programs(
        &mut self,
        _tab_id: TabId,
        _generation: u64,
    ) -> Result<Vec<JavaScriptPageDebuggerProgram>, JavaScriptPageDebuggerError> {
        Ok(vec![JavaScriptPageDebuggerProgram {
            program_handle: 7,
            program_generation: 3,
        }])
    }

    fn debugger_safe_points(
        &mut self,
        _tab_id: TabId,
        _generation: u64,
        _program_handle: u64,
        _program_generation: u64,
    ) -> Result<
        Vec<crate::script::javascript::JavaScriptPageDebuggerSafePoint>,
        JavaScriptPageDebuggerError,
    > {
        Ok(Vec::new())
    }

    fn validate_debugger_safe_point(
        &mut self,
        _tab_id: TabId,
        _generation: u64,
        _program_handle: u64,
        _program_generation: u64,
        _code_unit_ordinal: u32,
        _bytecode_offset: u32,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        Ok(())
    }

    fn debugger_execution_control_available(&self) -> bool {
        true
    }

    fn debugger_nested_frames_available(&self) -> bool {
        true
    }

    fn debugger_stack_available(&self) -> bool {
        true
    }

    fn debugger_scopes_available(&self) -> bool {
        true
    }

    fn debugger_values_available(&self) -> bool {
        true
    }

    fn debugger_static_metadata_inventory_available(&self) -> bool {
        true
    }

    fn debugger_static_metadata_source_inventory_available(&self) -> bool {
        true
    }

    fn debugger_static_metadata_safe_point_span_available(&self) -> bool {
        true
    }

    fn debugger_static_metadata(
        &mut self,
        _tab_id: TabId,
        _generation: u64,
        _program_handle: u64,
        _program_generation: u64,
    ) -> Result<
        Vec<crate::script::javascript::JavaScriptPageDebuggerStaticMetadata>,
        JavaScriptPageDebuggerError,
    > {
        Ok(vec![
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadata {
                metadata_handle: 41,
                metadata_generation: 9,
            },
        ])
    }

    fn debugger_static_metadata_sources(
        &mut self,
        _tab_id: TabId,
        _generation: u64,
        _program_handle: u64,
        _program_generation: u64,
        metadata_handle: u64,
        metadata_generation: u64,
    ) -> Result<
        Vec<crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSourceId>,
        JavaScriptPageDebuggerError,
    > {
        if (metadata_handle, metadata_generation) != (41, 9) {
            return Err(JavaScriptPageDebuggerError::UnknownProgram);
        }
        Ok(vec![
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSourceId {
                source_id: 0,
            },
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSourceId {
                source_id: 1,
            },
        ])
    }

    fn debugger_stack_snapshot(
        &mut self,
        _tab_id: TabId,
        _generation: u64,
        _program: JavaScriptPageDebuggerProgram,
        frame: Option<JavaScriptPageDebuggerFrame>,
        max_frames: u32,
        _max_scope_entries: u32,
    ) -> Result<
        crate::script::javascript::JavaScriptPageDebuggerStackSnapshot,
        JavaScriptPageDebuggerError,
    > {
        if frame.is_none_or(|frame| frame.core_instance != [7; 16] || frame.frame_handle != 19) {
            return Err(JavaScriptPageDebuggerError::UnknownProgram);
        }
        let frames = [(1, 0), (0, self.moved_root_offset)]
            .into_iter()
            .take(max_frames as usize)
            .map(|(code_unit_ordinal, bytecode_offset)| {
                crate::script::javascript::JavaScriptPageDebuggerStackFrame {
                    code_unit_ordinal,
                    bytecode_offset,
                    scope_entries: vec![JavaScriptPageDebuggerScopeEntry {
                        slot_ordinal: 3,
                        scope_depth: 0,
                    }],
                    scope_truncated: false,
                }
            })
            .collect();
        Ok(
            crate::script::javascript::JavaScriptPageDebuggerStackSnapshot {
                frames,
                stack_truncated: max_frames < 2,
            },
        )
    }

    fn debugger_value_snapshot(
        &mut self,
        _tab_id: TabId,
        _generation: u64,
        target: JavaScriptPageDebuggerValueTarget,
    ) -> Result<JavaScriptPageDebuggerValuePreview, JavaScriptPageDebuggerError> {
        self.value_calls += 1;
        if target.program.program_handle != 7
            || target.program.program_generation != 3
            || target.frame_index != 1
            || target.safe_point.code_unit_ordinal != 0
            || target.safe_point.bytecode_offset != self.moved_root_offset
            || target.scope_entry.slot_ordinal != 3
            || target
                .frame
                .is_none_or(|frame| frame.core_instance != [7; 16] || frame.frame_handle != 19)
        {
            return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
        }
        self.value_preview
            .clone()
            .ok_or(JavaScriptPageDebuggerError::ExecutionControlUnavailable)
    }

    fn debugger_static_metadata_safe_point_span(
        &mut self,
        _tab_id: TabId,
        _generation: u64,
        target: JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget,
    ) -> Result<
        crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSafePointSpan,
        JavaScriptPageDebuggerError,
    > {
        self.span_calls += 1;
        let (start_byte, end_byte) = match (
            target.code_unit_ordinal,
            target.bytecode_offset,
            target.source_id,
        ) {
            (1, 0, 0) => (8, 40),
            (0, 41, 1) if !self.unbound_root => (42, 60),
            _ => return Err(JavaScriptPageDebuggerError::UnknownProgram),
        };
        Ok(
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSafePointSpan {
                source_id: target.source_id,
                start_byte,
                end_byte,
                coordinates: DebuggerSourceCoordinates {
                    start_line: 0,
                    start_column_utf16: start_byte,
                    end_line: 0,
                    end_column_utf16: end_byte,
                },
            },
        )
    }
}

struct LinkedLocations {
    moved_caller: bool,
    bad_second_source: bool,
    arm_calls: usize,
    span_calls: usize,
    resume_calls: usize,
}

impl LinkedLocations {
    fn stack(&self, tab_id: TabId, generation: u64) -> JavaScriptPageDebuggerLinkedStackSnapshot {
        JavaScriptPageDebuggerLinkedStackSnapshot {
            frames: [
                (7, 3, 1, 0, 19),
                (8, 4, 0, if self.moved_caller { 9 } else { 8 }, 29),
            ]
            .map(
                |(
                    program_handle,
                    program_generation,
                    code_unit_ordinal,
                    bytecode_offset,
                    frame_handle,
                )| {
                    JavaScriptPageDebuggerLinkedStackFrame {
                        frame: JavaScriptPageDebuggerFrame {
                            tab_id,
                            document_generation: generation,
                            program_handle,
                            program_generation,
                            code_unit_ordinal,
                            core_instance: [7; 16],
                            frame_handle,
                        },
                        safe_point: JavaScriptPageDebuggerSafePoint {
                            code_unit_ordinal,
                            bytecode_offset,
                        },
                    }
                },
            ),
        }
    }
}

impl PageJavaScriptDebuggerLocations for LinkedLocations {
    fn debugger_has_live_realm(&mut self, _: TabId, _: u64) -> bool {
        true
    }

    fn max_debugger_safe_points_per_program(&self) -> usize {
        2
    }

    fn debugger_programs(
        &mut self,
        _: TabId,
        _: u64,
    ) -> Result<Vec<JavaScriptPageDebuggerProgram>, JavaScriptPageDebuggerError> {
        Ok([(7, 3), (8, 4)]
            .map(
                |(program_handle, program_generation)| JavaScriptPageDebuggerProgram {
                    program_handle,
                    program_generation,
                },
            )
            .to_vec())
    }

    fn debugger_safe_points(
        &mut self,
        _: TabId,
        _: u64,
        _: u64,
        _: u64,
    ) -> Result<Vec<JavaScriptPageDebuggerSafePoint>, JavaScriptPageDebuggerError> {
        Ok(vec![])
    }

    fn validate_debugger_safe_point(
        &mut self,
        _: TabId,
        _: u64,
        program_handle: u64,
        program_generation: u64,
        code_unit_ordinal: u32,
        bytecode_offset: u32,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        if (
            program_handle,
            program_generation,
            code_unit_ordinal,
            bytecode_offset,
        ) == (7, 3, 1, 0)
        {
            Ok(())
        } else {
            Err(JavaScriptPageDebuggerError::InvalidSafePoint)
        }
    }

    fn debugger_execution_control_available(&self) -> bool {
        true
    }

    fn debugger_linked_frames_available(&self) -> bool {
        true
    }

    fn arm_debugger_linked_nested_safe_point_breakpoint(
        &mut self,
        _: TabId,
        _: u64,
        entry: JavaScriptPageDebuggerProgram,
        dependency: JavaScriptPageDebuggerProgram,
        point: JavaScriptPageDebuggerSafePoint,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        self.arm_calls += 1;
        if (
            entry.program_handle,
            dependency.program_handle,
            point.code_unit_ordinal,
        ) == (8, 7, 1)
        {
            Ok(())
        } else {
            Err(JavaScriptPageDebuggerError::InvalidSafePoint)
        }
    }

    fn debugger_linked_execution_state(
        &mut self,
        tab_id: TabId,
        generation: u64,
        _: JavaScriptPageDebuggerProgram,
    ) -> Result<JavaScriptPageDebuggerLinkedExecutionState, JavaScriptPageDebuggerError> {
        Ok(JavaScriptPageDebuggerLinkedExecutionState::Paused {
            stack: self.stack(tab_id, generation),
        })
    }

    fn debugger_linked_stack_snapshot(
        &mut self,
        top: JavaScriptPageDebuggerFrame,
        _: u32,
    ) -> Result<JavaScriptPageDebuggerLinkedStackSnapshot, JavaScriptPageDebuggerError> {
        if top.frame_handle != 19 {
            return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
        }
        Ok(self.stack(top.tab_id, top.document_generation))
    }

    fn resume_debugger_linked_nested_execution(
        &mut self,
        top: JavaScriptPageDebuggerFrame,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        self.resume_calls += 1;
        if top.frame_handle == 19 {
            Ok(())
        } else {
            Err(JavaScriptPageDebuggerError::InvalidExecutionState)
        }
    }

    fn debugger_static_metadata_inventory_available(&self) -> bool {
        true
    }

    fn debugger_static_metadata_source_inventory_available(&self) -> bool {
        true
    }

    fn debugger_static_metadata_safe_point_span_available(&self) -> bool {
        true
    }

    fn debugger_static_metadata(
        &mut self,
        _: TabId,
        _: u64,
        program_handle: u64,
        _: u64,
    ) -> Result<
        Vec<crate::script::javascript::JavaScriptPageDebuggerStaticMetadata>,
        JavaScriptPageDebuggerError,
    > {
        Ok(vec![
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadata {
                metadata_handle: program_handle + 40,
                metadata_generation: 9,
            },
        ])
    }

    fn debugger_static_metadata_sources(
        &mut self,
        _: TabId,
        _: u64,
        _: u64,
        _: u64,
        _: u64,
        _: u64,
    ) -> Result<
        Vec<crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSourceId>,
        JavaScriptPageDebuggerError,
    > {
        Ok(vec![
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSourceId {
                source_id: 0,
            },
        ])
    }

    fn debugger_linked_stack_spans(
        &mut self,
        expected: JavaScriptPageDebuggerLinkedStackSnapshot,
        access: JavaScriptPageDebuggerLinkedSpanAccess,
    ) -> Result<
        [crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSafePointSpan; 2],
        JavaScriptPageDebuggerError,
    > {
        self.span_calls += 1;
        if expected.frames
            != self
                .stack(
                    expected.frames[0].frame.tab_id,
                    expected.frames[0].frame.document_generation,
                )
                .frames
            || !access.granted
            || access.metadata_receipted != [true; 2]
            || access.source_receipted != [true; 2]
            || access.targets[0].metadata_handle != 47
            || access.targets[1].metadata_handle != 48
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        Ok([8, 20].map(|start_byte| {
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSafePointSpan {
                source_id: if start_byte == 20 && self.bad_second_source {
                    1
                } else {
                    0
                },
                start_byte,
                end_byte: start_byte + 2,
                coordinates: blueice_ipc::debugger::DebuggerSourceCoordinates {
                    start_line: 0,
                    start_column_utf16: start_byte,
                    end_line: 0,
                    end_column_utf16: start_byte + 2,
                },
            }
        }))
    }
}
