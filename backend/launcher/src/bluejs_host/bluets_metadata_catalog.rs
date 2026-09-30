// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl BlueJsChildHost {
    /// Resolves only child-retained, exact-live static root-slot IDs behind a
    /// previously minted metadata handle. This is an internal relation seed,
    /// not a page-host request or a runtime value/type assertion.
    pub(super) fn live_bluets_root_symbol_slots(
        &self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    ) -> Result<&[DirectRootSymbolSlot], ChildRootSlotLookupError> {
        if !program.is_well_formed() {
            return Err(ChildRootSlotLookupError::InvalidProgram);
        }
        if !metadata.is_well_formed() {
            return Err(ChildRootSlotLookupError::InvalidMetadata);
        }
        let document = self
            .documents
            .get(&tab_id)
            .ok_or(ChildRootSlotLookupError::UnknownDocument)?;
        if document.generation != document_generation {
            return Err(ChildRootSlotLookupError::StaleDocument);
        }
        let record = document
            .debugger_programs
            .get(&program.program_handle)
            .filter(|record| record.program_generation == program.program_generation)
            .ok_or(ChildRootSlotLookupError::InvalidProgram)?;
        if record.metadata != Some(metadata) {
            return Err(ChildRootSlotLookupError::InvalidMetadata);
        }
        self.debug_registry
            .root_symbol_slots(self.runtime.program_registry(), record.runtime_handle)
            .map_err(|_| ChildRootSlotLookupError::Unavailable)
    }

    /// Enumerates the child-private static-BlueTS association for one exact
    /// currently live program. The association is intentionally minted lazily
    /// on this authenticated inventory request: program discovery itself does
    /// not imply static-metadata authority. The reply is a bounded handle list
    /// rather than a metadata payload, and it uses an identity namespace that
    /// is distinct from the child debugger program IDs.
    pub(super) fn debugger_bluets_metadata(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    ) -> PageHostReply {
        if !program.is_well_formed() {
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
            if record.program_generation != program.program_generation {
                return invalid_request();
            }
            record.runtime_handle
        };

        // A normal JavaScript program and a BlueTS program whose attachment
        // has been pruned both have no metadata inventory. Do not mint a
        // negative-result handle; an empty bounded list reveals no static
        // count or compiler detail beyond this program's ineligibility.
        if self
            .debug_registry
            .root_symbol_slots(self.runtime.program_registry(), runtime_handle)
            .is_err()
        {
            let document = self
                .documents
                .get_mut(&tab_id)
                .expect("the exact child document remains live after registry validation");
            document
                .debugger_programs
                .get_mut(&program.program_handle)
                .expect("the exact child program remains registered")
                .metadata = None;
            return PageHostReply::DebuggerBlueTsMetadata {
                tab_id,
                document_generation,
                program,
                metadata: Vec::new(),
            };
        }

        let metadata = {
            let existing = self
                .documents
                .get(&tab_id)
                .expect("the exact child document remains live after registry validation")
                .debugger_programs
                .get(&program.program_handle)
                .expect("the exact child program remains registered")
                .metadata;
            match existing {
                Some(metadata) => metadata,
                None => {
                    let metadata = match self.mint_debugger_metadata_handle() {
                        Ok(metadata) => metadata,
                        Err(()) => return host_failure(),
                    };
                    self.documents
                        .get_mut(&tab_id)
                        .expect("the exact child document remains live while metadata is minted")
                        .debugger_programs
                        .get_mut(&program.program_handle)
                        .expect(
                            "the exact child program remains registered while metadata is minted",
                        )
                        .metadata = Some(metadata);
                    metadata
                }
            }
        };
        PageHostReply::DebuggerBlueTsMetadata {
            tab_id,
            document_generation,
            program,
            metadata: vec![metadata],
        }
    }

    /// Returns the first deliberately narrow read surface for a previously
    /// inventoried BlueTS debug attachment. The handle must still be owned by
    /// this exact live program, so neither an arbitrary child-private ID nor
    /// a handle from a sibling program can probe the registry. The response
    /// contains fixed fingerprints and aggregate counts only; source text and
    /// identity, spans, names, type displays, symbols, contracts, bytecode,
    /// VM objects, and values remain inside this child.
    pub(super) fn debugger_bluets_metadata_summary(
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

        if self
            .live_bluets_root_symbol_slots(tab_id, document_generation, program, metadata)
            .is_err()
        {
            self.documents
                .get_mut(&tab_id)
                .expect("the exact child document remains live after slot validation")
                .debugger_programs
                .get_mut(&program.program_handle)
                .expect("the exact child program remains registered after slot validation")
                .metadata = None;
            return invalid_request();
        }

        let summary = match self
            .debug_registry
            .get(self.runtime.program_registry(), runtime_handle)
        {
            Ok(retained) => {
                let info = retained.static_info();
                let (Ok(source_count), Ok(type_count), Ok(symbol_count), Ok(contract_count)) = (
                    u32::try_from(info.sources.len()),
                    u32::try_from(info.types.len()),
                    u32::try_from(info.symbols.len()),
                    u32::try_from(info.contracts.len()),
                ) else {
                    return host_failure();
                };
                PageHostDebuggerBlueTsMetadataSummary {
                    language_version: info.language_version.clone(),
                    compiler_options_hash: info.compiler_options_hash.clone(),
                    source_count,
                    type_count,
                    symbol_count,
                    contract_count,
                }
            }
            Err(_) => {
                // A registry pruning race invalidates the private inventory
                // identity before reporting anything about the old record.
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
        PageHostReply::DebuggerBlueTsMetadataSummary {
            tab_id,
            document_generation,
            program,
            metadata,
            summary,
        }
    }

    /// Returns aggregate evidence for the exact child-retained direct-lowering
    /// map. Source spans, map entries, AST nodes, code-unit IDs, and bytecode
    /// offsets remain in the child; this operation is not a map dereference.
    pub(super) fn debugger_bluets_metadata_lowering_summary(
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
        let summary = match self
            .debug_registry
            .get(self.runtime.program_registry(), runtime_handle)
        {
            Ok(retained) => {
                let map = retained.safe_point_map();
                let Ok(bound_safe_point_count) = u32::try_from(map.entries.len()) else {
                    return host_failure();
                };
                if bound_safe_point_count > PAGE_HOST_DEBUGGER_MAX_SAFE_POINTS_PER_PROGRAM {
                    return resource_limit();
                }
                PageHostDebuggerBlueTsMetadataLoweringSummary {
                    safe_point_map_abi: map.format.to_string(),
                    program_abi: map.program_abi.to_string(),
                    source_set_hash: map.source_set_hash.clone(),
                    bound_safe_point_count,
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
        PageHostReply::DebuggerBlueTsMetadataLoweringSummary {
            tab_id,
            document_generation,
            program,
            metadata,
            summary: Box::new(summary),
        }
    }

    /// Lists only compiler-minted source-record IDs for an already inventoried
    /// metadata attachment. The enclosing opaque metadata handle remains the
    /// target boundary; an ID reveals no module identity, content hash, text,
    /// span, or record detail.
    pub(super) fn debugger_bluets_metadata_sources(
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
        let sources = match self
            .debug_registry
            .get(self.runtime.program_registry(), runtime_handle)
        {
            Ok(retained) => {
                let static_sources = &retained.static_info().sources;
                if static_sources.len()
                    > usize::try_from(DEBUGGER_STATIC_METADATA_MAX_SOURCES).unwrap()
                {
                    return resource_limit();
                }
                let mut identities = BTreeSet::new();
                let mut sources = Vec::with_capacity(static_sources.len());
                for source in static_sources {
                    if !identities.insert(source.id.0) {
                        return invalid_request();
                    }
                    sources.push(PageHostDebuggerBlueTsMetadataSourceId {
                        source_id: source.id.0,
                    });
                }
                sources
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
        PageHostReply::DebuggerBlueTsMetadataSources {
            tab_id,
            document_generation,
            program,
            metadata,
            sources,
        }
    }

    /// Lists only compiler-minted type-record IDs for an already inventoried
    /// metadata attachment. The IDs carry no type display, source identity,
    /// span, symbol, contract, bytecode, VM object, or value.
    pub(super) fn debugger_bluets_metadata_types(
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
        let types = match self
            .debug_registry
            .get(self.runtime.program_registry(), runtime_handle)
        {
            Ok(retained) => {
                let static_types = &retained.static_info().types;
                if static_types.len() > usize::try_from(DEBUGGER_STATIC_METADATA_MAX_TYPES).unwrap()
                {
                    return resource_limit();
                }
                let mut identities = BTreeSet::new();
                let mut types = Vec::with_capacity(static_types.len());
                for static_type in static_types {
                    if !identities.insert(static_type.id.0) {
                        return invalid_request();
                    }
                    types.push(PageHostDebuggerBlueTsMetadataTypeId {
                        type_id: static_type.id.0,
                    });
                }
                types
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
        PageHostReply::DebuggerBlueTsMetadataTypes {
            tab_id,
            document_generation,
            program,
            metadata,
            types,
        }
    }

    /// Returns one bounded compiler-produced type display after the caller
    /// supplies an exact child program and metadata attachment. This private
    /// endpoint never accepts a standalone numeric type target, and returns
    /// no source, span, symbol, contract, bytecode, VM object, or value.
    pub(super) fn debugger_bluets_metadata_type_display(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
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
        let static_type = match self
            .debug_registry
            .get(self.runtime.program_registry(), runtime_handle)
        {
            Ok(retained) => {
                let Some(static_type) = retained
                    .static_info()
                    .types
                    .iter()
                    .find(|static_type| static_type.id.0 == type_id)
                else {
                    return invalid_request();
                };
                if static_type.display.is_empty()
                    || static_type.display.len() > DEBUGGER_STATIC_METADATA_TYPE_DISPLAY_MAX_BYTES
                {
                    return resource_limit();
                }
                PageHostDebuggerBlueTsMetadataTypeDisplay {
                    type_id,
                    display: static_type.display.clone(),
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
        PageHostReply::DebuggerBlueTsMetadataType {
            tab_id,
            document_generation,
            program,
            metadata,
            static_type,
        }
    }

    /// Lists only compiler-minted symbol IDs after the caller supplies an
    /// exact child program and metadata attachment. Names, spans, declared
    /// types, contracts, bytecode, VM objects, and values remain private.
    pub(super) fn debugger_bluets_metadata_symbols(
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
        let symbols = match self
            .debug_registry
            .get(self.runtime.program_registry(), runtime_handle)
        {
            Ok(retained) => {
                let static_symbols = &retained.static_info().symbols;
                if static_symbols.len()
                    > usize::try_from(DEBUGGER_STATIC_METADATA_MAX_SYMBOLS).unwrap()
                {
                    return resource_limit();
                }
                let mut identities = BTreeSet::new();
                let mut symbols = Vec::with_capacity(static_symbols.len());
                for symbol in static_symbols {
                    if !identities.insert(symbol.id.0) {
                        return invalid_request();
                    }
                    symbols.push(PageHostDebuggerBlueTsMetadataSymbolId {
                        symbol_id: symbol.id.0,
                    });
                }
                symbols
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
        PageHostReply::DebuggerBlueTsMetadataSymbols {
            tab_id,
            document_generation,
            program,
            metadata,
            symbols,
        }
    }

    /// Returns one bounded compiler-produced symbol display after the caller
    /// supplies an exact child program and metadata attachment. This private
    /// endpoint never accepts a standalone numeric symbol target, and returns
    /// no source span, type, contract, bytecode, VM object, or value.
    pub(super) fn debugger_bluets_metadata_symbol_display(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        symbol_id: u32,
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
        let symbol = match self
            .debug_registry
            .get(self.runtime.program_registry(), runtime_handle)
        {
            Ok(retained) => {
                let Some(symbol) = retained
                    .static_info()
                    .symbols
                    .iter()
                    .find(|symbol| symbol.id.0 == symbol_id)
                else {
                    return invalid_request();
                };
                if symbol.name.is_empty()
                    || symbol.name.len() > DEBUGGER_STATIC_METADATA_SYMBOL_DISPLAY_MAX_BYTES
                {
                    return resource_limit();
                }
                PageHostDebuggerBlueTsMetadataSymbolDisplay {
                    symbol_id,
                    display: symbol.name.clone(),
                    exported: symbol.exported,
                    kind: match symbol.kind {
                        blueice_bluets::SymbolKind::Import => {
                            DebuggerStaticMetadataSymbolKind::Import
                        }
                        blueice_bluets::SymbolKind::TypeAlias => {
                            DebuggerStaticMetadataSymbolKind::TypeAlias
                        }
                        blueice_bluets::SymbolKind::Interface => {
                            DebuggerStaticMetadataSymbolKind::Interface
                        }
                        blueice_bluets::SymbolKind::Variable => {
                            DebuggerStaticMetadataSymbolKind::Variable
                        }
                        blueice_bluets::SymbolKind::Function => {
                            DebuggerStaticMetadataSymbolKind::Function
                        }
                    },
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
        PageHostReply::DebuggerBlueTsMetadataSymbol {
            tab_id,
            document_generation,
            program,
            metadata,
            symbol,
        }
    }

    /// Returns one source-text-free half-open declaration range for an exact
    /// compiler-minted symbol. The location contains only the symbol/source
    /// numeric identities and byte offsets; module identity, source contents,
    /// names, types, contracts, bytecode, values, and source-map translation
    /// remain private to the child.
    pub(super) fn debugger_bluets_metadata_symbol_location(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        symbol_id: u32,
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
                let Some(symbol) = retained
                    .static_info()
                    .symbols
                    .iter()
                    .find(|symbol| symbol.id.0 == symbol_id)
                else {
                    return invalid_request();
                };
                let Some(source) = retained
                    .static_info()
                    .sources
                    .iter()
                    .find(|source| source.id == symbol.source)
                else {
                    return invalid_request();
                };
                if symbol.span.module != source.module
                    || symbol.span.start >= symbol.span.end
                    || symbol.span.end
                        > usize::try_from(DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES).unwrap()
                {
                    return invalid_request();
                }
                let Ok(start_byte) = u32::try_from(symbol.span.start) else {
                    return invalid_request();
                };
                let Ok(end_byte) = u32::try_from(symbol.span.end) else {
                    return invalid_request();
                };
                let Some(coordinates) =
                    debugger_source_coordinates(symbol.location, start_byte, end_byte)
                else {
                    return invalid_request();
                };
                PageHostDebuggerBlueTsMetadataSymbolLocation {
                    symbol_id,
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
        PageHostReply::DebuggerBlueTsMetadataSymbolLocation {
            tab_id,
            document_generation,
            program,
            metadata,
            location,
        }
    }
}
