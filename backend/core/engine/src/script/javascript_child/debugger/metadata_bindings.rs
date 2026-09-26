// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl<C: PageHostClient> OutOfProcessJavaScriptPageExecutor<C> {
    pub(super) fn core_debugger_static_metadata_symbol_location(
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

    pub(super) fn core_debugger_static_metadata_safe_point_span(
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
            return Err(child_static_metadata_relation_reply_error(&reply));
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

    pub(super) fn core_debugger_exception_location(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        target: JavaScriptPageDebuggerExceptionLocationTarget,
    ) -> Result<JavaScriptPageDebuggerExceptionLocation, JavaScriptPageDebuggerError> {
        if !self.debugger_exception_location_available() {
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
            .debugger_bluets_exception_location(
                tab_id.as_u64(),
                document_generation,
                child_program,
                child_metadata,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerBlueTsExceptionLocation {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program,
            metadata,
            location,
        } = reply
        else {
            return Err(child_static_metadata_relation_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || program != child_program
            || metadata != child_metadata
            || !location.safe_point.is_well_formed()
            || location.safe_point.program != child_program
            || location.span.source_id != target.source_id
            || !location
                .span
                .coordinates
                .is_well_formed_for_range(location.span.start_byte, location.span.end_byte)
        {
            return Err(JavaScriptPageDebuggerError::UnknownProgram);
        }
        let point_reply = self
            .child
            .validate_debugger_safe_point(tab_id.as_u64(), document_generation, location.safe_point)
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        if !matches!(
            point_reply,
            PageHostReply::DebuggerSafePointValidated {
                tab_id: echoed_tab_id,
                document_generation: echoed_generation,
                safe_point,
            } if echoed_tab_id == tab_id.as_u64()
                && echoed_generation == document_generation
                && safe_point == location.safe_point
        ) {
            return Err(JavaScriptPageDebuggerError::UnknownProgram);
        }
        let span_reply = self
            .child
            .debugger_bluets_safe_point_span(
                tab_id.as_u64(),
                document_generation,
                child_metadata,
                location.safe_point,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        if !matches!(
            span_reply,
            PageHostReply::DebuggerBlueTsSafePointSpan {
                tab_id: echoed_tab_id,
                document_generation: echoed_generation,
                metadata: echoed_metadata,
                safe_point,
                span,
            } if echoed_tab_id == tab_id.as_u64()
                && echoed_generation == document_generation
                && echoed_metadata == child_metadata
                && safe_point == location.safe_point
                && span == location.span
        ) {
            return Err(JavaScriptPageDebuggerError::UnknownProgram);
        }
        Ok(JavaScriptPageDebuggerExceptionLocation {
            source_id: location.span.source_id,
            code_unit_ordinal: location.safe_point.code_unit_ordinal,
            bytecode_offset: location.safe_point.bytecode_offset,
            start_byte: location.span.start_byte,
            end_byte: location.span.end_byte,
            coordinates: location.span.coordinates,
        })
    }

    pub(super) fn core_debugger_static_metadata_source_breakpoint(
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

    pub(super) fn core_debugger_static_metadata_contract_location(
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

    pub(super) fn core_debugger_static_metadata_symbol_type(
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

    pub(super) fn core_debugger_static_metadata_symbol_contract(
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
}
