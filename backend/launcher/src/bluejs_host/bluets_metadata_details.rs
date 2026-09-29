// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl BlueJsChildHost {
    /// Resolves only one exact compiler-bound safe point to its retained
    /// original BlueTS byte span. The private child first revalidates the live
    /// instruction and opaque metadata attachment; an ordinary BlueJS safe
    /// point with no direct BlueTS lowering record never receives a guessed
    /// source position.
    pub(super) fn debugger_bluets_safe_point_span(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        metadata: PageHostDebuggerMetadataHandle,
        safe_point: PageHostDebuggerSafePoint,
    ) -> PageHostReply {
        if !metadata.is_well_formed() || !safe_point.is_well_formed() {
            return invalid_request();
        }
        if let Err(reply) = self.exact_debugger_safe_point(tab_id, document_generation, safe_point)
        {
            return reply;
        }
        let program = safe_point.program;
        let runtime_handle = {
            let document = match self.exact_document(tab_id, document_generation) {
                Ok(document) => document,
                Err(reply) => return reply,
            };
            let Some(record) = document.debugger_programs.get(&program.program_handle) else {
                return invalid_request();
            };
            if record.program_generation != program.program_generation
                || record.metadata != Some(metadata)
            {
                return invalid_request();
            }
            record.runtime_handle
        };
        let span = match self
            .debug_registry
            .get(self.runtime.program_registry(), runtime_handle)
        {
            Ok(retained) => match exact_bluets_span_for_site(retained, safe_point) {
                Some(span) => span,
                None => return invalid_request(),
            },
            Err(_) => {
                self.documents
                    .get_mut(&tab_id)
                    .expect("the exact child document remains live after registry validation")
                    .debugger_programs
                    .get_mut(&program.program_handle)
                    .expect("the exact child program remains registered after registry validation")
                    .metadata = None;
                return invalid_request();
            }
        };
        PageHostReply::DebuggerBlueTsSafePointSpan {
            tab_id,
            document_generation,
            metadata,
            safe_point,
            span,
        }
    }

    pub(super) fn debugger_bluets_exception_location(
        &self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    ) -> PageHostReply {
        let document = match self.exact_document(tab_id, document_generation) {
            Ok(document) => document,
            Err(reply) => return reply,
        };
        if !document.debugger_execution_control
            || !program.is_well_formed()
            || !metadata.is_well_formed()
        {
            return invalid_request();
        }
        let Some(record) = document.debugger_programs.get(&program.program_handle) else {
            return invalid_request();
        };
        if record.program_generation != program.program_generation
            || record.metadata != Some(metadata)
        {
            return invalid_request();
        }
        let Some(location) = record.exception_location else {
            return invalid_debugger_state();
        };
        if location.safe_point.program != program
            || self
                .exact_debugger_safe_point(tab_id, document_generation, location.safe_point)
                .is_err()
            || self
                .debug_registry
                .get(self.runtime.program_registry(), record.runtime_handle)
                .ok()
                .and_then(|retained| exact_bluets_span_for_site(retained, location.safe_point))
                != Some(location.span)
        {
            return invalid_request();
        }
        PageHostReply::DebuggerBlueTsExceptionLocation {
            tab_id,
            document_generation,
            program,
            metadata,
            location: PageHostDebuggerBlueTsExceptionLocation {
                safe_point: location.safe_point,
                span: location.span,
            },
        }
    }

    /// Resolves a bounded original TypeScript position only inside the
    /// already-authorized private core/child channel. A retained unbound span
    /// stays unbound; it is never silently skipped in favor of a later bound
    /// instruction. This operation does not install or arm a breakpoint.
    pub(super) fn debugger_bluets_source_breakpoint(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        source_id: u32,
        source_byte: u32,
    ) -> PageHostReply {
        if !program.is_well_formed()
            || !metadata.is_well_formed()
            || source_byte > DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES
        {
            return invalid_request();
        }
        let runtime_handle = {
            let document = match self.exact_document(tab_id, document_generation) {
                Ok(document) => document,
                Err(reply) => return reply,
            };
            let Some(record) = document.debugger_programs.get(&program.program_handle) else {
                return invalid_request();
            };
            if record.program_generation != program.program_generation
                || record.metadata != Some(metadata)
            {
                return invalid_request();
            }
            record.runtime_handle
        };
        let safe_point = match self
            .debug_registry
            .get(self.runtime.program_registry(), runtime_handle)
        {
            Ok(retained) => {
                let mut matching_sources = retained
                    .static_info()
                    .sources
                    .iter()
                    .filter(|source| source.id.0 == source_id);
                let Some(source) = matching_sources.next() else {
                    return invalid_request();
                };
                if matching_sources.next().is_some() {
                    return invalid_request();
                }
                match retained.breakpoint_at_or_after(&source.module, source_byte as usize) {
                    DirectSafePointBinding::Bound(bound) => Some(PageHostDebuggerSafePoint {
                        program,
                        code_unit_ordinal: bound.code_unit.ordinal(),
                        bytecode_offset: bound.bytecode_offset,
                    }),
                    DirectSafePointBinding::Unbound => None,
                }
            }
            Err(_) => {
                self.documents
                    .get_mut(&tab_id)
                    .expect("the exact child document remains live after registry validation")
                    .debugger_programs
                    .get_mut(&program.program_handle)
                    .expect("the exact child program remains registered after registry validation")
                    .metadata = None;
                return invalid_request();
            }
        };
        if let Some(safe_point) = safe_point {
            if let Err(reply) =
                self.exact_debugger_safe_point(tab_id, document_generation, safe_point)
            {
                return reply;
            }
        }
        PageHostReply::DebuggerBlueTsSourceBreakpoint {
            tab_id,
            document_generation,
            program,
            metadata,
            source_id,
            source_byte,
            safe_point,
        }
    }

    /// Returns only a retained contract declaration's source ID and bounded
    /// byte range under one exact child-local metadata attachment.
    pub(super) fn debugger_bluets_metadata_contract_location(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        contract_id: u32,
    ) -> PageHostReply {
        if !program.is_well_formed() || !metadata.is_well_formed() {
            return invalid_request();
        }
        let runtime_handle = {
            let document = match self.exact_document(tab_id, document_generation) {
                Ok(document) => document,
                Err(reply) => return reply,
            };
            let Some(record) = document.debugger_programs.get(&program.program_handle) else {
                return invalid_request();
            };
            if record.program_generation != program.program_generation
                || record.metadata != Some(metadata)
            {
                return invalid_request();
            }
            record.runtime_handle
        };
        let location = match self
            .debug_registry
            .get(self.runtime.program_registry(), runtime_handle)
        {
            Ok(retained) => {
                let Some(contract) = retained
                    .static_info()
                    .contracts
                    .iter()
                    .find(|contract| contract.id.0 == contract_id)
                else {
                    return invalid_request();
                };
                let Some(source) = retained
                    .static_info()
                    .sources
                    .iter()
                    .find(|source| source.id == contract.source)
                else {
                    return invalid_request();
                };
                if contract.span.module != source.module
                    || contract.span.start >= contract.span.end
                    || contract.span.end
                        > usize::try_from(DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES).unwrap()
                {
                    return invalid_request();
                }
                let Ok(start_byte) = u32::try_from(contract.span.start) else {
                    return invalid_request();
                };
                let Ok(end_byte) = u32::try_from(contract.span.end) else {
                    return invalid_request();
                };
                let Some(coordinates) =
                    debugger_source_coordinates(contract.location, start_byte, end_byte)
                else {
                    return invalid_request();
                };
                PageHostDebuggerBlueTsMetadataContractLocation {
                    contract_id,
                    source_id: source.id.0,
                    start_byte,
                    end_byte,
                    coordinates,
                }
            }
            Err(_) => {
                self.documents
                    .get_mut(&tab_id)
                    .expect("the exact child document remains live after registry validation")
                    .debugger_programs
                    .get_mut(&program.program_handle)
                    .expect("the exact child program remains registered after registry validation")
                    .metadata = None;
                return invalid_request();
            }
        };
        PageHostReply::DebuggerBlueTsMetadataContractLocation {
            tab_id,
            document_generation,
            program,
            metadata,
            location,
        }
    }

    /// Verifies an exact compiler-recorded symbol/type relation without
    /// returning an unrequested ID or a static record. Core has already
    /// required independent same-stream receipts for the two numeric IDs.
    pub(super) fn debugger_bluets_metadata_symbol_type(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        symbol_id: u32,
        type_id: u32,
    ) -> PageHostReply {
        if !program.is_well_formed() || !metadata.is_well_formed() {
            return invalid_request();
        }
        let runtime_handle = {
            let document = match self.exact_document(tab_id, document_generation) {
                Ok(document) => document,
                Err(reply) => return reply,
            };
            let Some(record) = document.debugger_programs.get(&program.program_handle) else {
                return invalid_request();
            };
            if record.program_generation != program.program_generation
                || record.metadata != Some(metadata)
            {
                return invalid_request();
            }
            record.runtime_handle
        };
        let symbol_type = match self
            .debug_registry
            .get(self.runtime.program_registry(), runtime_handle)
        {
            Ok(retained) => {
                let static_info = retained.static_info();
                let Some(symbol) = static_info
                    .symbols
                    .iter()
                    .find(|symbol| symbol.id.0 == symbol_id)
                else {
                    return invalid_request();
                };
                if symbol
                    .static_type
                    .is_none_or(|static_type| static_type.0 != type_id)
                    || !static_info
                        .types
                        .iter()
                        .any(|static_type| static_type.id.0 == type_id)
                {
                    return invalid_request();
                }
                PageHostDebuggerBlueTsMetadataSymbolType { symbol_id, type_id }
            }
            Err(_) => {
                self.documents
                    .get_mut(&tab_id)
                    .expect("the exact child document remains live after registry validation")
                    .debugger_programs
                    .get_mut(&program.program_handle)
                    .expect("the exact child program remains registered after registry validation")
                    .metadata = None;
                return invalid_request();
            }
        };
        PageHostReply::DebuggerBlueTsMetadataSymbolType {
            tab_id,
            document_generation,
            program,
            metadata,
            symbol_type,
        }
    }

    /// Verifies an exact compiler-recorded symbol/contract relation without
    /// returning an unrequested ID, a contract plan, or a validation result.
    /// Core has already required separate same-stream receipts for both IDs.
    pub(super) fn debugger_bluets_metadata_symbol_contract(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        symbol_id: u32,
        contract_id: u32,
    ) -> PageHostReply {
        if !program.is_well_formed() || !metadata.is_well_formed() {
            return invalid_request();
        }
        let runtime_handle = {
            let document = match self.exact_document(tab_id, document_generation) {
                Ok(document) => document,
                Err(reply) => return reply,
            };
            let Some(record) = document.debugger_programs.get(&program.program_handle) else {
                return invalid_request();
            };
            if record.program_generation != program.program_generation
                || record.metadata != Some(metadata)
            {
                return invalid_request();
            }
            record.runtime_handle
        };
        let symbol_contract = match self
            .debug_registry
            .get(self.runtime.program_registry(), runtime_handle)
        {
            Ok(retained) => {
                let static_info = retained.static_info();
                let Some(symbol) = static_info
                    .symbols
                    .iter()
                    .find(|symbol| symbol.id.0 == symbol_id)
                else {
                    return invalid_request();
                };
                if symbol
                    .contract
                    .is_none_or(|contract| contract.0 != contract_id)
                    || !static_info
                        .contracts
                        .iter()
                        .any(|contract| contract.id.0 == contract_id)
                {
                    return invalid_request();
                }
                PageHostDebuggerBlueTsMetadataSymbolContract {
                    symbol_id,
                    contract_id,
                }
            }
            Err(_) => {
                self.documents
                    .get_mut(&tab_id)
                    .expect("the exact child document remains live after registry validation")
                    .debugger_programs
                    .get_mut(&program.program_handle)
                    .expect("the exact child program remains registered after registry validation")
                    .metadata = None;
                return invalid_request();
            }
        };
        PageHostReply::DebuggerBlueTsMetadataSymbolContract {
            tab_id,
            document_generation,
            program,
            metadata,
            symbol_contract,
        }
    }

    /// Lists only compiler-minted contract IDs after the caller supplies an
    /// exact child program and metadata attachment. Names, spans, plans,
    /// validation, bytecode, VM objects, and values remain private.
    pub(super) fn debugger_bluets_metadata_contracts(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    ) -> PageHostReply {
        if !program.is_well_formed() || !metadata.is_well_formed() {
            return invalid_request();
        }
        let runtime_handle = {
            let document = match self.exact_document(tab_id, document_generation) {
                Ok(document) => document,
                Err(reply) => return reply,
            };
            let Some(record) = document.debugger_programs.get(&program.program_handle) else {
                return invalid_request();
            };
            if record.program_generation != program.program_generation
                || record.metadata != Some(metadata)
            {
                return invalid_request();
            }
            record.runtime_handle
        };
        let contracts = match self
            .debug_registry
            .get(self.runtime.program_registry(), runtime_handle)
        {
            Ok(retained) => {
                let static_contracts = &retained.static_info().contracts;
                if static_contracts.len()
                    > usize::try_from(DEBUGGER_STATIC_METADATA_MAX_CONTRACTS).unwrap()
                {
                    return resource_limit();
                }
                let mut identities = BTreeSet::new();
                let mut contracts = Vec::with_capacity(static_contracts.len());
                for contract in static_contracts {
                    if !identities.insert(contract.id.0) {
                        return invalid_request();
                    }
                    contracts.push(PageHostDebuggerBlueTsMetadataContractId {
                        contract_id: contract.id.0,
                    });
                }
                contracts
            }
            Err(_) => {
                self.documents
                    .get_mut(&tab_id)
                    .expect("the exact child document remains live after registry validation")
                    .debugger_programs
                    .get_mut(&program.program_handle)
                    .expect("the exact child program remains registered after registry validation")
                    .metadata = None;
                return invalid_request();
            }
        };
        PageHostReply::DebuggerBlueTsMetadataContracts {
            tab_id,
            document_generation,
            program,
            metadata,
            contracts,
        }
    }

    /// Returns one bounded compiler-produced contract display after the caller
    /// supplies an exact child program and metadata attachment. This private
    /// endpoint never accepts a standalone numeric contract target, and returns
    /// no source span, plan, validation behavior, bytecode, VM object, or value.
    pub(super) fn debugger_bluets_metadata_contract_display(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        contract_id: u32,
    ) -> PageHostReply {
        if !program.is_well_formed() || !metadata.is_well_formed() {
            return invalid_request();
        }
        let runtime_handle = {
            let document = match self.exact_document(tab_id, document_generation) {
                Ok(document) => document,
                Err(reply) => return reply,
            };
            let Some(record) = document.debugger_programs.get(&program.program_handle) else {
                return invalid_request();
            };
            if record.program_generation != program.program_generation
                || record.metadata != Some(metadata)
            {
                return invalid_request();
            }
            record.runtime_handle
        };
        let contract = match self
            .debug_registry
            .get(self.runtime.program_registry(), runtime_handle)
        {
            Ok(retained) => {
                let Some(contract) = retained
                    .static_info()
                    .contracts
                    .iter()
                    .find(|contract| contract.id.0 == contract_id)
                else {
                    return invalid_request();
                };
                if contract.name.is_empty()
                    || contract.name.len() > DEBUGGER_STATIC_METADATA_CONTRACT_DISPLAY_MAX_BYTES
                {
                    return resource_limit();
                }
                PageHostDebuggerBlueTsMetadataContractDisplay {
                    contract_id,
                    display: contract.name.clone(),
                    root_kind: debugger_contract_root_kind(&contract.plan),
                }
            }
            Err(_) => {
                self.documents
                    .get_mut(&tab_id)
                    .expect("the exact child document remains live after registry validation")
                    .debugger_programs
                    .get_mut(&program.program_handle)
                    .expect("the exact child program remains registered after registry validation")
                    .metadata = None;
                return invalid_request();
            }
        };
        PageHostReply::DebuggerBlueTsMetadataContract {
            tab_id,
            document_generation,
            program,
            metadata,
            contract,
        }
    }

    /// Validates a data-only snapshot under immutable child-selected limits
    /// against one exact private contract target. The reply never echoes the
    /// input or exposes a contract plan, path, expected type, observed type,
    /// bytecode, VM object, or runtime value.
    pub(super) fn debugger_bluets_metadata_contract_validation(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        contract_id: u32,
        value: CompilerContractValue,
    ) -> PageHostReply {
        if !program.is_well_formed() || !metadata.is_well_formed() {
            return invalid_request();
        }
        let value = match debugger_contract_value(value) {
            Ok(value) => value,
            Err(()) => return invalid_request(),
        };
        let runtime_handle = {
            let document = match self.exact_document(tab_id, document_generation) {
                Ok(document) => document,
                Err(reply) => return reply,
            };
            let Some(record) = document.debugger_programs.get(&program.program_handle) else {
                return invalid_request();
            };
            if record.program_generation != program.program_generation
                || record.metadata != Some(metadata)
            {
                return invalid_request();
            }
            record.runtime_handle
        };
        let valid = match self
            .debug_registry
            .get(self.runtime.program_registry(), runtime_handle)
        {
            Ok(retained) => {
                let Some(contract) = retained
                    .static_info()
                    .contracts
                    .iter()
                    .find(|contract| contract.id.0 == contract_id)
                else {
                    return invalid_request();
                };
                contract
                    .plan
                    .validate_with_limits(&value, debugger_contract_validation_limits())
                    .is_ok()
            }
            Err(_) => {
                self.documents
                    .get_mut(&tab_id)
                    .expect("the exact child document remains live after registry validation")
                    .debugger_programs
                    .get_mut(&program.program_handle)
                    .expect("the exact child program remains registered after registry validation")
                    .metadata = None;
                return invalid_request();
            }
        };
        PageHostReply::DebuggerBlueTsMetadataContractValidation {
            tab_id,
            document_generation,
            program,
            metadata,
            validation: PageHostDebuggerBlueTsMetadataContractValidation { contract_id, valid },
        }
    }

    /// Returns the explicitly authorized, source-text-free provenance for one
    /// source ID that remains owned by this exact child program and metadata
    /// attachment. A failed registry lookup destroys the child-private handle
    /// rather than letting a stale identity probe a successor attachment.
    pub(super) fn debugger_bluets_metadata_source_provenance(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        source_id: u32,
    ) -> PageHostReply {
        if !program.is_well_formed() || !metadata.is_well_formed() {
            return invalid_request();
        }
        let runtime_handle = {
            let document = match self.exact_document(tab_id, document_generation) {
                Ok(document) => document,
                Err(reply) => return reply,
            };
            let Some(record) = document.debugger_programs.get(&program.program_handle) else {
                return invalid_request();
            };
            if record.program_generation != program.program_generation
                || record.metadata != Some(metadata)
            {
                return invalid_request();
            }
            record.runtime_handle
        };
        let provenance = match self
            .debug_registry
            .get(self.runtime.program_registry(), runtime_handle)
        {
            Ok(retained) => {
                let Some(source) = retained
                    .static_info()
                    .sources
                    .iter()
                    .find(|source| source.id.0 == source_id)
                else {
                    return invalid_request();
                };
                PageHostDebuggerBlueTsMetadataSourceProvenance {
                    source_id,
                    module: source.module.clone(),
                    content_hash: source.content_hash.clone(),
                }
            }
            Err(_) => {
                self.documents
                    .get_mut(&tab_id)
                    .expect("the exact child document remains live after registry validation")
                    .debugger_programs
                    .get_mut(&program.program_handle)
                    .expect("the exact child program remains registered after registry validation")
                    .metadata = None;
                return invalid_request();
            }
        };
        PageHostReply::DebuggerBlueTsMetadataSourceProvenance {
            tab_id,
            document_generation,
            program,
            metadata,
            provenance,
        }
    }
}
