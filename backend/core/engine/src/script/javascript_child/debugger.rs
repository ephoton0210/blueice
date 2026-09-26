// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl<C: PageHostClient> PageJavaScriptDebuggerLocations for OutOfProcessJavaScriptPageExecutor<C> {
    fn debugger_has_live_realm(&mut self, tab_id: TabId, document_generation: u64) -> bool {
        if !self.has_core_live_document(tab_id, document_generation) {
            return false;
        }
        let child_has_live_realm = matches!(
            self.child
                .debugger_realm_stats(tab_id.as_u64(), document_generation),
            Ok(PageHostReply::RealmStats(stats))
                if stats.tab_id == tab_id.as_u64()
                    && stats.document_generation == document_generation
                    && stats.is_well_formed()
        );
        if !child_has_live_realm {
            // A later transport/error response is not proof that the cached
            // accounting still names a child-owned realm. Drop it immediately;
            // the ordinary lifecycle owner will close the realm if it gets a
            // later synchronization or execution-control failure.
            self.realm_stats.remove(&tab_id);
        }
        child_has_live_realm
    }

    fn max_debugger_safe_points_per_program(&self) -> usize {
        usize::try_from(PAGE_HOST_DEBUGGER_MAX_SAFE_POINTS_PER_PROGRAM)
            .expect("page-host debugger safe-point cap fits usize")
    }

    fn debugger_breakpoint_configuration_available(&self) -> bool {
        self.child.debugger_breakpoint_configuration_available()
    }

    fn max_debugger_breakpoints_per_realm(&self) -> usize {
        usize::try_from(PAGE_HOST_DEBUGGER_MAX_BREAKPOINTS_PER_REALM)
            .expect("page-host debugger breakpoint cap fits usize")
    }

    fn debugger_static_metadata_inventory_available(&self) -> bool {
        self.child.debugger_bluets_metadata_available()
    }

    fn debugger_static_metadata_summary_available(&self) -> bool {
        self.child.debugger_bluets_metadata_available()
            && self.child.debugger_bluets_metadata_summary_available()
    }

    fn debugger_static_metadata_lowering_summary_available(&self) -> bool {
        self.child.debugger_bluets_metadata_available()
            && self
                .child
                .debugger_bluets_metadata_lowering_summary_available()
    }

    fn debugger_static_metadata_source_inventory_available(&self) -> bool {
        self.child.debugger_bluets_metadata_available()
            && self.child.debugger_bluets_metadata_sources_available()
    }

    fn debugger_static_metadata_source_provenance_available(&self) -> bool {
        self.child.debugger_bluets_metadata_available()
            && self.child.debugger_bluets_metadata_sources_available()
            && self
                .child
                .debugger_bluets_metadata_source_provenance_available()
    }

    fn debugger_static_metadata_type_inventory_available(&self) -> bool {
        self.child.debugger_bluets_metadata_available()
            && self.child.debugger_bluets_metadata_types_available()
    }

    fn debugger_static_metadata_type_display_available(&self) -> bool {
        self.child.debugger_bluets_metadata_available()
            && self.child.debugger_bluets_metadata_types_available()
            && self.child.debugger_bluets_metadata_type_display_available()
    }

    fn debugger_static_metadata_symbol_inventory_available(&self) -> bool {
        self.child.debugger_bluets_metadata_available()
            && self.child.debugger_bluets_metadata_symbols_available()
    }

    fn debugger_static_metadata_symbol_location_available(&self) -> bool {
        self.child.debugger_bluets_metadata_available()
            && self.child.debugger_bluets_metadata_sources_available()
            && self.child.debugger_bluets_metadata_symbols_available()
            && self
                .child
                .debugger_bluets_metadata_symbol_location_available()
    }

    fn debugger_static_metadata_safe_point_span_available(&self) -> bool {
        self.child.debugger_bluets_metadata_available()
            && self.child.debugger_bluets_metadata_sources_available()
            && self.child.debugger_bluets_safe_point_span_available()
    }

    fn debugger_exception_location_available(&self) -> bool {
        self.child.debugger_bluets_metadata_available()
            && self.child.debugger_bluets_metadata_sources_available()
            && self.child.debugger_bluets_safe_point_span_available()
            && self.child.debugger_bluets_exception_location_available()
    }

    fn debugger_static_metadata_source_breakpoint_available(&self) -> bool {
        self.child.debugger_bluets_metadata_available()
            && self.child.debugger_bluets_metadata_sources_available()
            && self.child.debugger_bluets_source_breakpoint_available()
    }

    fn debugger_static_metadata_contract_location_available(&self) -> bool {
        self.child.debugger_bluets_metadata_available()
            && self.child.debugger_bluets_metadata_sources_available()
            && self.child.debugger_bluets_metadata_contracts_available()
            && self
                .child
                .debugger_bluets_metadata_contract_location_available()
    }

    fn debugger_static_metadata_symbol_type_available(&self) -> bool {
        self.child.debugger_bluets_metadata_available()
            && self.child.debugger_bluets_metadata_symbols_available()
            && self.child.debugger_bluets_metadata_types_available()
            && self.child.debugger_bluets_metadata_symbol_type_available()
    }

    fn debugger_static_metadata_symbol_contract_available(&self) -> bool {
        self.child.debugger_bluets_metadata_available()
            && self.child.debugger_bluets_metadata_symbols_available()
            && self.child.debugger_bluets_metadata_contracts_available()
            && self
                .child
                .debugger_bluets_metadata_symbol_contract_available()
    }

    fn debugger_static_metadata_contract_inventory_available(&self) -> bool {
        self.child.debugger_bluets_metadata_available()
            && self.child.debugger_bluets_metadata_contracts_available()
    }

    fn debugger_static_metadata_contract_display_available(&self) -> bool {
        self.child.debugger_bluets_metadata_available()
            && self.child.debugger_bluets_metadata_contracts_available()
            && self
                .child
                .debugger_bluets_metadata_contract_display_available()
    }

    fn debugger_static_metadata_contract_validation_available(&self) -> bool {
        self.child.debugger_bluets_metadata_available()
            && self.child.debugger_bluets_metadata_contracts_available()
            && self
                .child
                .debugger_bluets_metadata_contract_validation_available()
    }

    fn debugger_static_metadata_symbol_display_available(&self) -> bool {
        self.child.debugger_bluets_metadata_available()
            && self.child.debugger_bluets_metadata_symbols_available()
            && self
                .child
                .debugger_bluets_metadata_symbol_display_available()
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
        // Metadata identities are only valid together with the exact current
        // child-to-core program mapping. A fresh program discovery invalidates
        // all prior inventory IDs before a subsequent source-free lookup can
        // remint them, even when the child happened to retain a numeric ID.
        self.debugger_static_metadata.remove(&tab_id);
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
        if self
            .debugger_nested_frames
            .get(&tab_id)
            .is_some_and(|active| {
                !current.iter().any(|(child, public)| {
                    *child == active.child.program
                        && public.program_handle == active.public.program_handle
                        && public.program_generation == active.public.program_generation
                })
            })
        {
            self.debugger_nested_frames.remove(&tab_id);
        }
        if self
            .debugger_linked_frames
            .get(&tab_id)
            .is_some_and(|active| {
                active.frames.iter().enumerate().any(|(index, frame)| {
                    let child = if index == 0 {
                        active.child.dependency_program
                    } else {
                        active.child.entry_program
                    };
                    current.get(&child).is_none_or(|public| {
                        public.program_handle != frame.frame.program_handle
                            || public.program_generation != frame.frame.program_generation
                    })
                })
            })
        {
            self.debugger_linked_frames.remove(&tab_id);
        }
        self.debugger_programs.insert(tab_id, current);
        Ok(programs)
    }

    fn debugger_static_metadata(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
    ) -> Result<Vec<JavaScriptPageDebuggerStaticMetadata>, JavaScriptPageDebuggerError> {
        if !self.debugger_static_metadata_inventory_available() {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let child_program = self.child_program_for_core(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        let reply = self
            .child
            .debugger_bluets_metadata(tab_id.as_u64(), document_generation, child_program)
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerBlueTsMetadata {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program,
            metadata,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || program != child_program
            || metadata.len() > MAX_CHILD_DEBUGGER_STATIC_METADATA_PER_PROGRAM
            || metadata.iter().any(|metadata| !metadata.is_well_formed())
            || has_duplicate_child_static_metadata(&metadata)
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }

        let previous = self
            .debugger_static_metadata
            .remove(&tab_id)
            .unwrap_or_default();
        // Inventory refresh replaces only this program's attachments. A
        // linked dependency and its entry have separate compiler records;
        // discovering one must not erase the other's already receipted core
        // mapping within the same live document.
        let mut current: BTreeMap<_, _> = previous
            .iter()
            .filter(|(_, public)| public.program != child_program)
            .map(|(child, public)| (*child, *public))
            .collect();
        let mut public_metadata = Vec::with_capacity(metadata.len());
        for child_metadata in metadata {
            let public = match previous.get(&child_metadata).copied() {
                Some(public) if public.program == child_program => public,
                _ => self.mint_core_debugger_static_metadata(child_program)?,
            };
            current.insert(child_metadata, public);
            public_metadata.push(JavaScriptPageDebuggerStaticMetadata {
                metadata_handle: public.metadata_handle,
                metadata_generation: public.metadata_generation,
            });
        }
        self.debugger_static_metadata.insert(tab_id, current);
        Ok(public_metadata)
    }

    fn debugger_static_metadata_summary(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
        metadata_handle: u64,
        metadata_generation: u64,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataSummary, JavaScriptPageDebuggerError> {
        if !self.debugger_static_metadata_summary_available() {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let child_program = self.child_program_for_core(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        let child_metadata = self.child_static_metadata_for_core(
            tab_id,
            document_generation,
            child_program,
            metadata_handle,
            metadata_generation,
        )?;
        let reply = self
            .child
            .debugger_bluets_metadata_summary(
                tab_id.as_u64(),
                document_generation,
                child_program,
                child_metadata,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerBlueTsMetadataSummary {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program,
            metadata,
            summary,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || program != child_program
            || metadata != child_metadata
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        Ok(JavaScriptPageDebuggerStaticMetadataSummary {
            language_version: summary.language_version,
            compiler_options_hash: summary.compiler_options_hash,
            source_count: summary.source_count,
            type_count: summary.type_count,
            symbol_count: summary.symbol_count,
            contract_count: summary.contract_count,
        })
    }

    fn debugger_static_metadata_lowering_summary(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
        metadata_handle: u64,
        metadata_generation: u64,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataLoweringSummary, JavaScriptPageDebuggerError>
    {
        if !self.debugger_static_metadata_lowering_summary_available() {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let child_program = self.child_program_for_core(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        let child_metadata = self.child_static_metadata_for_core(
            tab_id,
            document_generation,
            child_program,
            metadata_handle,
            metadata_generation,
        )?;
        let reply = self
            .child
            .debugger_bluets_metadata_lowering_summary(
                tab_id.as_u64(),
                document_generation,
                child_program,
                child_metadata,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerBlueTsMetadataLoweringSummary {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program,
            metadata,
            summary,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || program != child_program
            || metadata != child_metadata
            || summary.safe_point_map_abi.is_empty()
            || summary.program_abi.is_empty()
            || summary.source_set_hash.is_empty()
            || summary.safe_point_map_abi.len()
                > blueice_ipc::debugger::DEBUGGER_STATIC_METADATA_LOWERING_ABI_MAX_BYTES
            || summary.program_abi.len()
                > blueice_ipc::debugger::DEBUGGER_STATIC_METADATA_LOWERING_ABI_MAX_BYTES
            || summary.source_set_hash.len()
                > blueice_ipc::debugger::DEBUGGER_STATIC_METADATA_LOWERING_SOURCE_SET_HASH_MAX_BYTES
            || summary.bound_safe_point_count
                > blueice_ipc::debugger::DEBUGGER_STATIC_METADATA_MAX_BOUND_SAFE_POINTS
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        Ok(JavaScriptPageDebuggerStaticMetadataLoweringSummary {
            safe_point_map_abi: summary.safe_point_map_abi,
            program_abi: summary.program_abi,
            source_set_hash: summary.source_set_hash,
            bound_safe_point_count: summary.bound_safe_point_count,
        })
    }

    fn debugger_static_metadata_sources(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
        metadata_handle: u64,
        metadata_generation: u64,
    ) -> Result<Vec<JavaScriptPageDebuggerStaticMetadataSourceId>, JavaScriptPageDebuggerError>
    {
        if !self.debugger_static_metadata_source_inventory_available() {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let child_program = self.child_program_for_core(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        let child_metadata = self.child_static_metadata_for_core(
            tab_id,
            document_generation,
            child_program,
            metadata_handle,
            metadata_generation,
        )?;
        let reply = self
            .child
            .debugger_bluets_metadata_sources(
                tab_id.as_u64(),
                document_generation,
                child_program,
                child_metadata,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerBlueTsMetadataSources {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program,
            metadata,
            sources,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || program != child_program
            || metadata != child_metadata
            || sources.len() > usize::try_from(DEBUGGER_STATIC_METADATA_MAX_SOURCES).unwrap()
            || has_duplicate_child_static_metadata_source_ids(&sources)
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        Ok(sources
            .into_iter()
            .map(|source| JavaScriptPageDebuggerStaticMetadataSourceId {
                source_id: source.source_id,
            })
            .collect())
    }

    fn debugger_static_metadata_source_provenance(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        target: JavaScriptPageDebuggerStaticMetadataSourceTarget,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataSourceProvenance, JavaScriptPageDebuggerError>
    {
        if !self.debugger_static_metadata_source_provenance_available() {
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
            .debugger_bluets_metadata_source_provenance(
                tab_id.as_u64(),
                document_generation,
                child_program,
                child_metadata,
                target.source_id,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerBlueTsMetadataSourceProvenance {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program,
            metadata,
            provenance,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || program != child_program
            || metadata != child_metadata
            || provenance.source_id != target.source_id
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        Ok(JavaScriptPageDebuggerStaticMetadataSourceProvenance {
            source_id: provenance.source_id,
            module: provenance.module,
            content_hash: provenance.content_hash,
        })
    }

    fn debugger_static_metadata_types(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
        metadata_handle: u64,
        metadata_generation: u64,
    ) -> Result<Vec<JavaScriptPageDebuggerStaticMetadataTypeId>, JavaScriptPageDebuggerError> {
        if !self.debugger_static_metadata_type_inventory_available() {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let child_program = self.child_program_for_core(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        let child_metadata = self.child_static_metadata_for_core(
            tab_id,
            document_generation,
            child_program,
            metadata_handle,
            metadata_generation,
        )?;
        let reply = self
            .child
            .debugger_bluets_metadata_types(
                tab_id.as_u64(),
                document_generation,
                child_program,
                child_metadata,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerBlueTsMetadataTypes {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program,
            metadata,
            types,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || program != child_program
            || metadata != child_metadata
            || types.len() > usize::try_from(DEBUGGER_STATIC_METADATA_MAX_TYPES).unwrap()
            || has_duplicate_child_static_metadata_type_ids(&types)
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        Ok(types
            .into_iter()
            .map(|static_type| JavaScriptPageDebuggerStaticMetadataTypeId {
                type_id: static_type.type_id,
            })
            .collect())
    }

    fn debugger_static_metadata_type_display(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        target: JavaScriptPageDebuggerStaticMetadataTypeTarget,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataTypeDisplay, JavaScriptPageDebuggerError> {
        if !self.debugger_static_metadata_type_display_available() {
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
            .debugger_bluets_metadata_type_display(
                tab_id.as_u64(),
                document_generation,
                child_program,
                child_metadata,
                target.type_id,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerBlueTsMetadataType {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program,
            metadata,
            static_type,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || program != child_program
            || metadata != child_metadata
            || static_type.type_id != target.type_id
            || static_type.display.is_empty()
            || static_type.display.len() > DEBUGGER_STATIC_METADATA_TYPE_DISPLAY_MAX_BYTES
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        Ok(JavaScriptPageDebuggerStaticMetadataTypeDisplay {
            type_id: static_type.type_id,
            display: static_type.display,
        })
    }

    fn debugger_static_metadata_symbols(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
        metadata_handle: u64,
        metadata_generation: u64,
    ) -> Result<Vec<JavaScriptPageDebuggerStaticMetadataSymbolId>, JavaScriptPageDebuggerError>
    {
        if !self.debugger_static_metadata_symbol_inventory_available() {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let child_program = self.child_program_for_core(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        let child_metadata = self.child_static_metadata_for_core(
            tab_id,
            document_generation,
            child_program,
            metadata_handle,
            metadata_generation,
        )?;
        let reply = self
            .child
            .debugger_bluets_metadata_symbols(
                tab_id.as_u64(),
                document_generation,
                child_program,
                child_metadata,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerBlueTsMetadataSymbols {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program,
            metadata,
            symbols,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || program != child_program
            || metadata != child_metadata
            || symbols.len() > usize::try_from(DEBUGGER_STATIC_METADATA_MAX_SYMBOLS).unwrap()
            || has_duplicate_child_static_metadata_symbol_ids(&symbols)
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        Ok(symbols
            .into_iter()
            .map(|symbol| JavaScriptPageDebuggerStaticMetadataSymbolId {
                symbol_id: symbol.symbol_id,
            })
            .collect())
    }

    fn debugger_static_metadata_contracts(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
        metadata_handle: u64,
        metadata_generation: u64,
    ) -> Result<Vec<JavaScriptPageDebuggerStaticMetadataContractId>, JavaScriptPageDebuggerError>
    {
        if !self.debugger_static_metadata_contract_inventory_available() {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let child_program = self.child_program_for_core(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        let child_metadata = self.child_static_metadata_for_core(
            tab_id,
            document_generation,
            child_program,
            metadata_handle,
            metadata_generation,
        )?;
        let reply = self
            .child
            .debugger_bluets_metadata_contracts(
                tab_id.as_u64(),
                document_generation,
                child_program,
                child_metadata,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerBlueTsMetadataContracts {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program,
            metadata,
            contracts,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || program != child_program
            || metadata != child_metadata
            || contracts.len() > usize::try_from(DEBUGGER_STATIC_METADATA_MAX_CONTRACTS).unwrap()
            || has_duplicate_child_static_metadata_contract_ids(&contracts)
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        Ok(contracts
            .into_iter()
            .map(|contract| JavaScriptPageDebuggerStaticMetadataContractId {
                contract_id: contract.contract_id,
            })
            .collect())
    }

    fn debugger_static_metadata_contract_display(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        target: JavaScriptPageDebuggerStaticMetadataContractTarget,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataContractDisplay, JavaScriptPageDebuggerError>
    {
        if !self.debugger_static_metadata_contract_display_available() {
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
            .debugger_bluets_metadata_contract_display(
                tab_id.as_u64(),
                document_generation,
                child_program,
                child_metadata,
                target.contract_id,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerBlueTsMetadataContract {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program,
            metadata,
            contract,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || program != child_program
            || metadata != child_metadata
            || contract.contract_id != target.contract_id
            || contract.display.is_empty()
            || contract.display.len() > DEBUGGER_STATIC_METADATA_CONTRACT_DISPLAY_MAX_BYTES
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        Ok(JavaScriptPageDebuggerStaticMetadataContractDisplay {
            contract_id: contract.contract_id,
            display: contract.display,
            root_kind: contract.root_kind,
        })
    }

    fn debugger_static_metadata_contract_validation(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        target: JavaScriptPageDebuggerStaticMetadataContractTarget,
        value: CompilerContractValue,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataContractValidation, JavaScriptPageDebuggerError>
    {
        if !self.debugger_static_metadata_contract_validation_available() {
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
            .debugger_bluets_metadata_contract_validation(
                tab_id.as_u64(),
                document_generation,
                child_program,
                child_metadata,
                target.contract_id,
                value,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerBlueTsMetadataContractValidation {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program,
            metadata,
            validation,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || program != child_program
            || metadata != child_metadata
            || validation.contract_id != target.contract_id
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        Ok(JavaScriptPageDebuggerStaticMetadataContractValidation {
            contract_id: validation.contract_id,
            valid: validation.valid,
        })
    }

    fn debugger_static_metadata_symbol_display(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        target: JavaScriptPageDebuggerStaticMetadataSymbolTarget,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataSymbolDisplay, JavaScriptPageDebuggerError>
    {
        if !self.debugger_static_metadata_symbol_display_available() {
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
            .debugger_bluets_metadata_symbol_display(
                tab_id.as_u64(),
                document_generation,
                child_program,
                child_metadata,
                target.symbol_id,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerBlueTsMetadataSymbol {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program,
            metadata,
            symbol,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || program != child_program
            || metadata != child_metadata
            || symbol.symbol_id != target.symbol_id
            || symbol.display.is_empty()
            || symbol.display.len() > DEBUGGER_STATIC_METADATA_SYMBOL_DISPLAY_MAX_BYTES
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        Ok(JavaScriptPageDebuggerStaticMetadataSymbolDisplay {
            symbol_id: symbol.symbol_id,
            display: symbol.display,
            kind: symbol.kind,
            exported: symbol.exported,
        })
    }

    fn debugger_static_metadata_symbol_location(
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

    fn debugger_static_metadata_safe_point_span(
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

    fn debugger_exception_location(
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

    fn debugger_static_metadata_source_breakpoint(
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

    fn debugger_static_metadata_contract_location(
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

    fn debugger_static_metadata_symbol_type(
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

    fn debugger_static_metadata_symbol_contract(
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

    fn set_debugger_breakpoint(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
        code_unit_ordinal: u32,
        bytecode_offset: u32,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        let safe_point = PageHostDebuggerSafePoint {
            program: self.child_program_for_core(
                tab_id,
                document_generation,
                program_handle,
                program_generation,
            )?,
            code_unit_ordinal,
            bytecode_offset,
        };
        // The public target was mapped to a child-private program identity,
        // but a program match alone is insufficient: make the child prove the
        // exact instruction boundary before it can mutate its bounded table.
        validate_child_safe_point_reply(self, tab_id, document_generation, safe_point)?;
        let reply = self
            .child
            .set_debugger_breakpoint(tab_id.as_u64(), document_generation, safe_point)
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        match reply {
            PageHostReply::DebuggerBreakpointSet {
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

    fn debugger_breakpoints(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
    ) -> Result<Vec<JavaScriptPageDebuggerBreakpoint>, JavaScriptPageDebuggerError> {
        if !self.has_core_live_document(tab_id, document_generation) {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let reply = self
            .child
            .debugger_breakpoints(tab_id.as_u64(), document_generation)
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerBreakpoints {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            safe_points,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || safe_points.len() > self.max_debugger_breakpoints_per_realm()
            || has_duplicate_child_safe_points(&safe_points)
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }

        let mut breakpoints = Vec::with_capacity(safe_points.len());
        for safe_point in safe_points {
            let public =
                self.core_program_for_child(tab_id, document_generation, safe_point.program)?;
            // The stored private table must not become a way for a compromised
            // child to fabricate arbitrary offsets under an otherwise known
            // private program ID. Require a fresh exact validation echo for
            // every listed tuple before re-minting the public response.
            validate_child_safe_point_reply(self, tab_id, document_generation, safe_point)?;
            breakpoints.push(JavaScriptPageDebuggerBreakpoint {
                program_handle: public.program_handle,
                program_generation: public.program_generation,
                code_unit_ordinal: safe_point.code_unit_ordinal,
                bytecode_offset: safe_point.bytecode_offset,
            });
        }
        Ok(breakpoints)
    }

    fn clear_debugger_breakpoint(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
        code_unit_ordinal: u32,
        bytecode_offset: u32,
    ) -> Result<bool, JavaScriptPageDebuggerError> {
        let safe_point = PageHostDebuggerSafePoint {
            program: self.child_program_for_core(
                tab_id,
                document_generation,
                program_handle,
                program_generation,
            )?,
            code_unit_ordinal,
            bytecode_offset,
        };
        // Clearing an absent valid record is idempotent, but clearing a
        // malformed/stale location is never a no-op that can target a
        // successor. Revalidate first just as the in-process route does.
        validate_child_safe_point_reply(self, tab_id, document_generation, safe_point)?;
        let reply = self
            .child
            .clear_debugger_breakpoint(tab_id.as_u64(), document_generation, safe_point)
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        match reply {
            PageHostReply::DebuggerBreakpointCleared {
                tab_id: reply_tab_id,
                document_generation: reply_generation,
                safe_point: reply_safe_point,
                was_present,
            } if reply_tab_id == tab_id.as_u64()
                && reply_generation == document_generation
                && reply_safe_point == safe_point =>
            {
                Ok(was_present)
            }
            PageHostReply::Error { .. } => Err(child_debugger_reply_error(&reply)),
            _ => Err(JavaScriptPageDebuggerError::NoLiveRealm),
        }
    }

    fn debugger_execution_control_available(&self) -> bool {
        self.native_debugger_execution_control && self.child.debugger_execution_control_available()
    }

    fn debugger_nested_frames_available(&self) -> bool {
        self.debugger_execution_control_available() && self.child.debugger_nested_frames_available()
    }

    fn debugger_linked_frames_available(&self) -> bool {
        self.debugger_execution_control_available() && self.child.debugger_linked_frames_available()
    }

    fn arm_debugger_linked_nested_safe_point_breakpoint(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        entry: JavaScriptPageDebuggerProgram,
        dependency: JavaScriptPageDebuggerProgram,
        safe_point: JavaScriptPageDebuggerSafePoint,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        if !self.debugger_linked_frames_available() || safe_point.code_unit_ordinal == 0 {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        let child_entry = self.child_program_for_core(
            tab_id,
            document_generation,
            entry.program_handle,
            entry.program_generation,
        )?;
        let child_dependency = self.child_program_for_core(
            tab_id,
            document_generation,
            dependency.program_handle,
            dependency.program_generation,
        )?;
        if child_entry == child_dependency {
            return Err(JavaScriptPageDebuggerError::InvalidSafePoint);
        }
        let child_point = PageHostDebuggerSafePoint {
            program: child_dependency,
            code_unit_ordinal: safe_point.code_unit_ordinal,
            bytecode_offset: safe_point.bytecode_offset,
        };
        validate_child_safe_point_reply(self, tab_id, document_generation, child_point)?;
        ChildLinkedDebuggerAdapter {
            child: &mut self.child,
        }
        .arm(
            tab_id.as_u64(),
            document_generation,
            child_entry,
            child_point,
        )
    }

    fn debugger_linked_execution_state(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        entry: JavaScriptPageDebuggerProgram,
    ) -> Result<JavaScriptPageDebuggerLinkedExecutionState, JavaScriptPageDebuggerError> {
        if !self.debugger_linked_frames_available() {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        let child_entry = self.child_program_for_core(
            tab_id,
            document_generation,
            entry.program_handle,
            entry.program_generation,
        )?;
        let observed = (ChildLinkedDebuggerAdapter {
            child: &mut self.child,
        })
        .lifecycle_state(tab_id.as_u64(), document_generation, child_entry, None);
        match observed {
            Ok(ChildLinkedDebuggerStatus::Pending) => {
                self.debugger_linked_frames.remove(&tab_id);
                Ok(JavaScriptPageDebuggerLinkedExecutionState::Pending)
            }
            Ok(ChildLinkedDebuggerStatus::Completed) => {
                self.debugger_linked_frames.remove(&tab_id);
                Ok(JavaScriptPageDebuggerLinkedExecutionState::Completed)
            }
            Ok(ChildLinkedDebuggerStatus::Paused { frame, safe_point }) => {
                let frames = self.capture_core_linked_pause_observed(
                    tab_id,
                    document_generation,
                    frame,
                    safe_point,
                    1,
                )?;
                Ok(JavaScriptPageDebuggerLinkedExecutionState::Paused {
                    stack: JavaScriptPageDebuggerLinkedStackSnapshot { frames },
                })
            }
            Ok(ChildLinkedDebuggerStatus::Resuming { frame }) => {
                let active = self
                    .debugger_linked_frames
                    .get(&tab_id)
                    .filter(|active| active.child == frame)
                    .ok_or(JavaScriptPageDebuggerError::InvalidExecutionState)?;
                Ok(JavaScriptPageDebuggerLinkedExecutionState::Resuming {
                    frame: active.frames[0].frame,
                })
            }
            Err(error) => {
                self.debugger_linked_frames.remove(&tab_id);
                Err(error)
            }
        }
    }

    fn debugger_linked_stack_snapshot(
        &mut self,
        top_frame: JavaScriptPageDebuggerFrame,
        max_scope_entries: u32,
    ) -> Result<JavaScriptPageDebuggerLinkedStackSnapshot, JavaScriptPageDebuggerError> {
        if !self.debugger_linked_frames_available()
            || !(1..=PAGE_HOST_DEBUGGER_MAX_SCOPE_ENTRIES).contains(&max_scope_entries)
        {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        let active = self
            .debugger_linked_frames
            .get(&top_frame.tab_id)
            .filter(|active| active.frames[0].frame == top_frame)
            .ok_or(JavaScriptPageDebuggerError::InvalidExecutionState)?;
        let entry = JavaScriptPageDebuggerProgram {
            program_handle: active.frames[1].frame.program_handle,
            program_generation: active.frames[1].frame.program_generation,
        };
        let frames = self.capture_core_linked_pause(
            top_frame.tab_id,
            top_frame.document_generation,
            entry,
            max_scope_entries,
        )?;
        if frames[0].frame != top_frame {
            return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
        }
        Ok(JavaScriptPageDebuggerLinkedStackSnapshot { frames })
    }

    fn debugger_linked_scope_snapshot(
        &mut self,
        expected_stack: JavaScriptPageDebuggerLinkedStackSnapshot,
    ) -> Result<JavaScriptPageDebuggerLinkedScopeSnapshot, JavaScriptPageDebuggerError> {
        if !self.debugger_linked_frames_available() {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        let top = expected_stack.frames[0].frame;
        let active_before = self
            .debugger_linked_frames
            .get(&top.tab_id)
            .filter(|active| active.frames == expected_stack.frames)
            .cloned()
            .ok_or(JavaScriptPageDebuggerError::InvalidExecutionState)?;
        if !self.has_core_live_document(top.tab_id, top.document_generation)
            || active_before.frames.iter().any(|frame| {
                frame.frame.tab_id != top.tab_id
                    || frame.frame.document_generation != top.document_generation
            })
            || !active_before
                .child_stack
                .is_well_formed(active_before.child)
        {
            return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
        }
        let observed =
            self.debugger_linked_stack_snapshot(top, PAGE_HOST_DEBUGGER_MAX_SCOPE_ENTRIES)?;
        let active_after = self
            .debugger_linked_frames
            .get(&top.tab_id)
            .filter(|active| {
                observed == expected_stack
                    && active.child == active_before.child
                    && active.frames == expected_stack.frames
                    && active.child_stack.is_well_formed(active.child)
                    && active.child_stack.max_scope_entries == PAGE_HOST_DEBUGGER_MAX_SCOPE_ENTRIES
                    && active
                        .child_stack
                        .frames
                        .iter()
                        .all(|frame| !frame.scope_truncated)
                    && (active_before
                        .child_stack
                        .frames
                        .iter()
                        .any(|frame| frame.scope_truncated)
                        || active_before.child_stack.frames == active.child_stack.frames)
            })
            .ok_or(JavaScriptPageDebuggerError::InvalidExecutionState)?;
        let scope_entries = active_after.child_stack.frames[1]
            .scope_entries
            .iter()
            .map(|entry| JavaScriptPageDebuggerScopeEntry {
                slot_ordinal: entry.slot_ordinal,
                scope_depth: entry.scope_depth,
            })
            .collect::<Vec<_>>();
        let mut seen = BTreeSet::new();
        if !scope_entries
            .iter()
            .all(|entry| seen.insert(entry.slot_ordinal))
        {
            return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
        }
        Ok(JavaScriptPageDebuggerLinkedScopeSnapshot {
            stack: expected_stack,
            scope_entries,
        })
    }

    fn resume_debugger_linked_nested_execution(
        &mut self,
        top_frame: JavaScriptPageDebuggerFrame,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        if !self.debugger_linked_frames_available() {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        let active = self
            .debugger_linked_frames
            .get(&top_frame.tab_id)
            .filter(|active| active.frames[0].frame == top_frame)
            .cloned()
            .ok_or(JavaScriptPageDebuggerError::InvalidExecutionState)?;
        for (index, frame) in active.frames.iter().enumerate() {
            let child = match self.child_program_for_core(
                top_frame.tab_id,
                top_frame.document_generation,
                frame.frame.program_handle,
                frame.frame.program_generation,
            ) {
                Ok(child) => child,
                Err(error) => {
                    self.debugger_linked_frames.remove(&top_frame.tab_id);
                    return Err(error);
                }
            };
            let expected = if index == 0 {
                active.child.dependency_program
            } else {
                active.child.entry_program
            };
            if child != expected {
                self.debugger_linked_frames.remove(&top_frame.tab_id);
                return Err(JavaScriptPageDebuggerError::NoLiveRealm);
            }
        }
        let result = (ChildLinkedDebuggerAdapter {
            child: &mut self.child,
        })
        .resume(active.child);
        if result.is_err() {
            self.debugger_linked_frames.remove(&top_frame.tab_id);
        }
        result
    }

    fn debugger_linked_stack_spans(
        &mut self,
        expected_stack: JavaScriptPageDebuggerLinkedStackSnapshot,
        access: JavaScriptPageDebuggerLinkedSpanAccess,
    ) -> Result<[JavaScriptPageDebuggerStaticMetadataSafePointSpan; 2], JavaScriptPageDebuggerError>
    {
        if !self.debugger_linked_frames_available() {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        self.core_linked_stack_spans(expected_stack, access)
    }

    fn arm_debugger_nested_safe_point_breakpoint(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
        code_unit_ordinal: u32,
        bytecode_offset: u32,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        if !self.debugger_nested_frames_available() || code_unit_ordinal == 0 {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        let safe_point = PageHostDebuggerSafePoint {
            program: self.child_program_for_core(
                tab_id,
                document_generation,
                program_handle,
                program_generation,
            )?,
            code_unit_ordinal,
            bytecode_offset,
        };
        validate_child_safe_point_reply(self, tab_id, document_generation, safe_point)?;
        let reply = self
            .child
            .arm_debugger_nested_safe_point_breakpoint(
                tab_id.as_u64(),
                document_generation,
                safe_point,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        match reply {
            PageHostReply::DebuggerNestedSafePointBreakpointArmed {
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

    fn debugger_nested_execution_state(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
    ) -> Result<Option<JavaScriptPageDebuggerNestedExecutionState>, JavaScriptPageDebuggerError>
    {
        if !self.debugger_nested_frames_available() {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        let program = self.child_program_for_core(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        let reply = self
            .child
            .debugger_execution_state(tab_id.as_u64(), document_generation, program)
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerExecutionState {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program: reply_program,
            state,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || reply_program != program
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        match state {
            PageHostDebuggerExecutionState::NestedPaused { frame, safe_point }
                if frame.tab_id == tab_id.as_u64()
                    && frame.document_generation == document_generation
                    && frame.program == program
                    && frame.matches_safe_point(safe_point) =>
            {
                validate_child_safe_point_reply(self, tab_id, document_generation, safe_point)?;
                Ok(Some(JavaScriptPageDebuggerNestedExecutionState::Paused {
                    frame: self.remint_core_debugger_frame(
                        tab_id,
                        document_generation,
                        program_handle,
                        program_generation,
                        frame,
                    )?,
                    bytecode_offset: safe_point.bytecode_offset,
                }))
            }
            PageHostDebuggerExecutionState::NestedStepping { frame }
            | PageHostDebuggerExecutionState::NestedResuming { frame }
                if frame.is_well_formed()
                    && frame.tab_id == tab_id.as_u64()
                    && frame.document_generation == document_generation
                    && frame.program == program =>
            {
                let active = self
                    .debugger_nested_frames
                    .get(&tab_id)
                    .filter(|active| {
                        active.child == frame
                            && active.public.document_generation == document_generation
                            && active.public.program_handle == program_handle
                            && active.public.program_generation == program_generation
                    })
                    .ok_or(JavaScriptPageDebuggerError::InvalidExecutionState)?;
                Ok(Some(match state {
                    PageHostDebuggerExecutionState::NestedStepping { .. } => {
                        JavaScriptPageDebuggerNestedExecutionState::Stepping {
                            frame: active.public,
                        }
                    }
                    PageHostDebuggerExecutionState::NestedResuming { .. } => {
                        JavaScriptPageDebuggerNestedExecutionState::Resuming {
                            frame: active.public,
                        }
                    }
                    _ => unreachable!("only nested advance states pass this guard"),
                }))
            }
            PageHostDebuggerExecutionState::NestedPaused { .. }
            | PageHostDebuggerExecutionState::NestedStepping { .. }
            | PageHostDebuggerExecutionState::NestedResuming { .. } => {
                Err(JavaScriptPageDebuggerError::NoLiveRealm)
            }
            _ => {
                if self
                    .debugger_nested_frames
                    .get(&tab_id)
                    .is_some_and(|active| {
                        active.public.document_generation == document_generation
                            && active.public.program_handle == program_handle
                            && active.public.program_generation == program_generation
                    })
                {
                    self.debugger_nested_frames.remove(&tab_id);
                }
                Ok(None)
            }
        }
    }

    fn step_debugger_nested_instruction(
        &mut self,
        frame: JavaScriptPageDebuggerFrame,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        if !self.debugger_nested_frames_available()
            || frame.code_unit_ordinal == 0
            || frame.frame_handle == 0
        {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        let active = self
            .debugger_nested_frames
            .get(&frame.tab_id)
            .copied()
            .filter(|active| active.public == frame)
            .ok_or(JavaScriptPageDebuggerError::InvalidExecutionState)?;
        let program = self.child_program_for_core(
            frame.tab_id,
            frame.document_generation,
            frame.program_handle,
            frame.program_generation,
        )?;
        if active.child.program != program {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let child_frame = active.child;
        let reply = self
            .child
            .step_debugger_nested_instruction(child_frame)
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        match reply {
            PageHostReply::DebuggerNestedStepRequested { frame: reply_frame }
                if reply_frame == child_frame =>
            {
                Ok(())
            }
            PageHostReply::Error { .. } => {
                self.debugger_nested_frames.remove(&frame.tab_id);
                Err(child_debugger_reply_error(&reply))
            }
            _ => {
                self.debugger_nested_frames.remove(&frame.tab_id);
                Err(JavaScriptPageDebuggerError::NoLiveRealm)
            }
        }
    }

    fn resume_debugger_nested_execution(
        &mut self,
        frame: JavaScriptPageDebuggerFrame,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        if !self.debugger_nested_frames_available()
            || frame.code_unit_ordinal == 0
            || frame.frame_handle == 0
        {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        let active = self
            .debugger_nested_frames
            .get(&frame.tab_id)
            .copied()
            .filter(|active| active.public == frame)
            .ok_or(JavaScriptPageDebuggerError::InvalidExecutionState)?;
        let program = self.child_program_for_core(
            frame.tab_id,
            frame.document_generation,
            frame.program_handle,
            frame.program_generation,
        )?;
        if active.child.program != program {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let child_frame = active.child;
        let reply = self
            .child
            .resume_debugger_nested_execution(child_frame)
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        match reply {
            PageHostReply::DebuggerNestedResumeRequested { frame: reply_frame }
                if reply_frame == child_frame =>
            {
                Ok(())
            }
            PageHostReply::Error { .. } => {
                self.debugger_nested_frames.remove(&frame.tab_id);
                Err(child_debugger_reply_error(&reply))
            }
            _ => {
                self.debugger_nested_frames.remove(&frame.tab_id);
                Err(JavaScriptPageDebuggerError::NoLiveRealm)
            }
        }
    }

    fn debugger_stack_snapshot(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        core_program: JavaScriptPageDebuggerProgram,
        frame: Option<JavaScriptPageDebuggerFrame>,
        max_frames: u32,
        max_scope_entries: u32,
    ) -> Result<JavaScriptPageDebuggerStackSnapshot, JavaScriptPageDebuggerError> {
        let JavaScriptPageDebuggerProgram {
            program_handle,
            program_generation,
        } = core_program;
        if !self.debugger_execution_control_available()
            || !self.child.debugger_stack_snapshot_available()
        {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        if !(1..=PAGE_HOST_DEBUGGER_MAX_STACK_FRAMES).contains(&max_frames)
            || !(1..=PAGE_HOST_DEBUGGER_MAX_SCOPE_ENTRIES).contains(&max_scope_entries)
        {
            return Err(JavaScriptPageDebuggerError::ResourceLimit);
        }
        let program = self.child_program_for_core(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        let (child_frame, top_code_unit, top_offset) = if let Some(frame) = frame {
            if frame.tab_id != tab_id
                || frame.document_generation != document_generation
                || frame.program_handle != program_handle
                || frame.program_generation != program_generation
                || frame.code_unit_ordinal == 0
                || frame.frame_handle == 0
            {
                return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
            }
            let active = self
                .debugger_nested_frames
                .get(&tab_id)
                .copied()
                .filter(|active| active.public == frame && active.child.program == program)
                .ok_or(JavaScriptPageDebuggerError::InvalidExecutionState)?;
            let Some(JavaScriptPageDebuggerNestedExecutionState::Paused {
                frame: still_paused,
                bytecode_offset,
            }) = self.debugger_nested_execution_state(
                tab_id,
                document_generation,
                program_handle,
                program_generation,
            )?
            else {
                return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
            };
            if still_paused != frame {
                return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
            }
            (Some(active.child), frame.code_unit_ordinal, bytecode_offset)
        } else {
            let state = self.debugger_execution_state(
                tab_id,
                document_generation,
                program_handle,
                program_generation,
            );
            let bytecode_offset = match state {
                Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable) => {
                    return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
                }
                Err(error) => return Err(error),
                Ok(state) => match state {
                    JavaScriptPageDebuggerExecutionState::Paused {
                        code_unit_ordinal: 0,
                        bytecode_offset,
                    }
                    | JavaScriptPageDebuggerExecutionState::SourceStepLimitReached {
                        code_unit_ordinal: 0,
                        bytecode_offset,
                    } => bytecode_offset,
                    _ => return Err(JavaScriptPageDebuggerError::InvalidExecutionState),
                },
            };
            (None, 0, bytecode_offset)
        };
        let reply = self
            .child
            .debugger_stack_snapshot(
                tab_id.as_u64(),
                document_generation,
                program,
                child_frame,
                max_frames,
                max_scope_entries,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerStackSnapshot {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program: reply_program,
            frame: reply_frame,
            snapshot,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || reply_program != program
            || reply_frame != child_frame
            || !valid_child_debugger_stack_snapshot(
                &snapshot,
                child_frame,
                top_code_unit,
                top_offset,
                max_frames,
                max_scope_entries,
            )
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        for stack_frame in &snapshot.frames {
            validate_child_safe_point_reply(
                self,
                tab_id,
                document_generation,
                PageHostDebuggerSafePoint {
                    program,
                    code_unit_ordinal: stack_frame.code_unit_ordinal,
                    bytecode_offset: stack_frame.bytecode_offset,
                },
            )?;
        }
        Ok(JavaScriptPageDebuggerStackSnapshot {
            frames: snapshot
                .frames
                .into_iter()
                .map(|frame| JavaScriptPageDebuggerStackFrame {
                    code_unit_ordinal: frame.code_unit_ordinal,
                    bytecode_offset: frame.bytecode_offset,
                    scope_entries: frame
                        .scope_entries
                        .into_iter()
                        .map(|entry| JavaScriptPageDebuggerScopeEntry {
                            slot_ordinal: entry.slot_ordinal,
                            scope_depth: entry.scope_depth,
                        })
                        .collect(),
                    scope_truncated: frame.scope_truncated,
                })
                .collect(),
            stack_truncated: snapshot.stack_truncated,
        })
    }

    fn debugger_static_scope_relation(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        target: JavaScriptPageDebuggerStaticScopeTarget,
    ) -> Result<JavaScriptPageDebuggerStaticScopeRelation, JavaScriptPageDebuggerError> {
        let (metadata, slot) = match target {
            JavaScriptPageDebuggerStaticScopeTarget::Ordinary { metadata, target } => {
                (metadata, target)
            }
            JavaScriptPageDebuggerStaticScopeTarget::Linked { .. } => {
                return self.core_linked_static_scope_relation(tab_id, document_generation, target);
            }
        };
        if !self.debugger_execution_control_available()
            || !self.child.debugger_stack_snapshot_available()
        {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        let frame_count = match (slot.frame, slot.frame_index) {
            (None, 0) => 1,
            (Some(_), 1) => 2,
            _ => return Err(JavaScriptPageDebuggerError::InvalidExecutionState),
        };
        if slot.safe_point.code_unit_ordinal != 0 {
            return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
        }
        let stack = self.debugger_stack_snapshot(
            tab_id,
            document_generation,
            slot.program,
            slot.frame,
            frame_count,
            PAGE_HOST_DEBUGGER_MAX_SCOPE_ENTRIES,
        )?;
        if stack.stack_truncated
            || stack.frames.len() != frame_count as usize
            || stack.frames.iter().any(|frame| frame.scope_truncated)
        {
            return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
        }
        let selected = &stack.frames[slot.frame_index as usize];
        if selected.code_unit_ordinal != 0
            || selected.bytecode_offset != slot.safe_point.bytecode_offset
            || selected
                .scope_entries
                .iter()
                .filter(|entry| entry.slot_ordinal == slot.scope_entry.slot_ordinal)
                .count()
                != 1
            || !selected.scope_entries.contains(&slot.scope_entry)
        {
            return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
        }
        let child_program = self.child_program_for_core(
            tab_id,
            document_generation,
            slot.program.program_handle,
            slot.program.program_generation,
        )?;
        let child_metadata = self.child_static_metadata_for_core(
            tab_id,
            document_generation,
            child_program,
            metadata.metadata_handle,
            metadata.metadata_generation,
        )?;
        let child_frame = slot
            .frame
            .map(|frame| {
                self.debugger_nested_frames
                    .get(&tab_id)
                    .filter(|active| {
                        active.public == frame && active.child.program == child_program
                    })
                    .map(|active| active.child)
                    .ok_or(JavaScriptPageDebuggerError::InvalidExecutionState)
            })
            .transpose()?;
        let child_target = PageHostDebuggerStaticScopeTarget::Ordinary {
            metadata: child_metadata,
            target: PageHostDebuggerValueTarget {
                tab_id: tab_id.as_u64(),
                document_generation,
                program: child_program,
                frame: child_frame,
                frame_index: slot.frame_index,
                safe_point: PageHostDebuggerSafePoint {
                    program: child_program,
                    code_unit_ordinal: 0,
                    bytecode_offset: slot.safe_point.bytecode_offset,
                },
                scope_entry: PageHostDebuggerScopeEntry {
                    slot_ordinal: slot.scope_entry.slot_ordinal,
                    scope_depth: slot.scope_entry.scope_depth,
                },
            },
        };
        let symbol_type = (ChildStaticScopeAdapter {
            child: &mut self.child,
        })
        .describe(child_target)?;
        Ok(JavaScriptPageDebuggerStaticScopeRelation {
            target,
            symbol_type: JavaScriptPageDebuggerStaticMetadataSymbolType {
                symbol_id: symbol_type.symbol_id,
                type_id: symbol_type.type_id,
            },
        })
    }

    fn debugger_value_snapshot(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        target: JavaScriptPageDebuggerValueTarget,
    ) -> Result<JavaScriptPageDebuggerValuePreview, JavaScriptPageDebuggerError> {
        if !self.debugger_execution_control_available()
            || !self.child.debugger_value_snapshot_available()
        {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        let frame_count = match (target.frame, target.frame_index) {
            (None, 0) => 1,
            (Some(_), 0 | 1) => 2,
            _ => return Err(JavaScriptPageDebuggerError::InvalidExecutionState),
        };
        let stack = self.debugger_stack_snapshot(
            tab_id,
            document_generation,
            target.program,
            target.frame,
            frame_count,
            PAGE_HOST_DEBUGGER_MAX_SCOPE_ENTRIES,
        )?;
        if stack.stack_truncated || stack.frames.len() != frame_count as usize {
            return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
        }
        let selected = &stack.frames[target.frame_index as usize];
        if selected.code_unit_ordinal != target.safe_point.code_unit_ordinal
            || selected.bytecode_offset != target.safe_point.bytecode_offset
            || !selected.scope_entries.contains(&target.scope_entry)
        {
            return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
        }
        let program = self.child_program_for_core(
            tab_id,
            document_generation,
            target.program.program_handle,
            target.program.program_generation,
        )?;
        let frame = target
            .frame
            .map(|public| {
                self.debugger_nested_frames
                    .get(&tab_id)
                    .filter(|active| active.public == public && active.child.program == program)
                    .map(|active| active.child)
                    .ok_or(JavaScriptPageDebuggerError::InvalidExecutionState)
            })
            .transpose()?;
        let child_target = PageHostDebuggerValueTarget {
            tab_id: tab_id.as_u64(),
            document_generation,
            program,
            frame,
            frame_index: target.frame_index,
            safe_point: PageHostDebuggerSafePoint {
                program,
                code_unit_ordinal: target.safe_point.code_unit_ordinal,
                bytecode_offset: target.safe_point.bytecode_offset,
            },
            scope_entry: PageHostDebuggerScopeEntry {
                slot_ordinal: target.scope_entry.slot_ordinal,
                scope_depth: target.scope_entry.scope_depth,
            },
        };
        if !child_target.is_well_formed() {
            return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
        }
        let reply = self
            .child
            .debugger_value_snapshot(child_target)
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerValueSnapshot(snapshot) = reply else {
            return Err(child_debugger_reply_error(&reply));
        };
        if snapshot.target != child_target || !snapshot.is_well_formed() {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        Ok(remint_child_debugger_value(snapshot.preview))
    }

    fn debugger_stack_available(&self) -> bool {
        self.debugger_execution_control_available()
            && self.child.debugger_stack_snapshot_available()
    }

    fn debugger_scopes_available(&self) -> bool {
        self.debugger_execution_control_available()
            && self.child.debugger_stack_snapshot_available()
    }

    fn debugger_values_available(&self) -> bool {
        self.debugger_scopes_available() && self.child.debugger_value_snapshot_available()
    }

    fn debugger_stepping_available(&self) -> bool {
        self.debugger_execution_control_available() && self.child.debugger_stepping_available()
    }

    fn debugger_source_span_stepping_available(&self) -> bool {
        self.debugger_stepping_available()
            && self.debugger_static_metadata_safe_point_span_available()
            && self.child.debugger_bluets_source_span_step_available()
    }

    fn arm_debugger_root_safe_point_breakpoint(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
        code_unit_ordinal: u32,
        bytecode_offset: u32,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        if !self.debugger_execution_control_available() || code_unit_ordinal != 0 {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        let safe_point = PageHostDebuggerSafePoint {
            program: self.child_program_for_core(
                tab_id,
                document_generation,
                program_handle,
                program_generation,
            )?,
            code_unit_ordinal,
            bytecode_offset,
        };
        validate_child_safe_point_reply(self, tab_id, document_generation, safe_point)?;
        let reply = self
            .child
            .arm_debugger_root_safe_point_breakpoint(
                tab_id.as_u64(),
                document_generation,
                safe_point,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        match reply {
            PageHostReply::DebuggerRootSafePointBreakpointArmed {
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

    fn debugger_execution_state(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
    ) -> Result<JavaScriptPageDebuggerExecutionState, JavaScriptPageDebuggerError> {
        if !self.debugger_execution_control_available() {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        let program = self.child_program_for_core(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        let reply = self
            .child
            .debugger_execution_state(tab_id.as_u64(), document_generation, program)
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerExecutionState {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program: reply_program,
            state,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || reply_program != program
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        match state {
            PageHostDebuggerExecutionState::Pending => {
                Ok(JavaScriptPageDebuggerExecutionState::Pending)
            }
            PageHostDebuggerExecutionState::Stepping => {
                Ok(JavaScriptPageDebuggerExecutionState::Stepping)
            }
            PageHostDebuggerExecutionState::Resuming => {
                Ok(JavaScriptPageDebuggerExecutionState::Resuming)
            }
            PageHostDebuggerExecutionState::Completed => {
                Ok(JavaScriptPageDebuggerExecutionState::Completed)
            }
            PageHostDebuggerExecutionState::Paused { safe_point }
                if safe_point.program == program && safe_point.code_unit_ordinal == 0 =>
            {
                validate_child_safe_point_reply(self, tab_id, document_generation, safe_point)?;
                Ok(JavaScriptPageDebuggerExecutionState::Paused {
                    code_unit_ordinal: safe_point.code_unit_ordinal,
                    bytecode_offset: safe_point.bytecode_offset,
                })
            }
            PageHostDebuggerExecutionState::SourceStepLimitReached { safe_point }
                if safe_point.program == program && safe_point.code_unit_ordinal == 0 =>
            {
                validate_child_safe_point_reply(self, tab_id, document_generation, safe_point)?;
                Ok(
                    JavaScriptPageDebuggerExecutionState::SourceStepLimitReached {
                        code_unit_ordinal: safe_point.code_unit_ordinal,
                        bytecode_offset: safe_point.bytecode_offset,
                    },
                )
            }
            PageHostDebuggerExecutionState::Paused { .. }
            | PageHostDebuggerExecutionState::SourceStepLimitReached { .. } => {
                Err(JavaScriptPageDebuggerError::NoLiveRealm)
            }
            PageHostDebuggerExecutionState::NestedPaused { .. }
            | PageHostDebuggerExecutionState::NestedStepping { .. }
            | PageHostDebuggerExecutionState::NestedResuming { .. } => {
                // Root-only state cannot alias a live nested frame.
                Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable)
            }
        }
    }

    fn resume_debugger_execution(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        if !self.debugger_execution_control_available() {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        let program = self.child_program_for_core(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        let reply = self
            .child
            .resume_debugger_execution(tab_id.as_u64(), document_generation, program)
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        match reply {
            PageHostReply::DebuggerExecutionResumed {
                tab_id: reply_tab_id,
                document_generation: reply_generation,
                program: reply_program,
            } if reply_tab_id == tab_id.as_u64()
                && reply_generation == document_generation
                && reply_program == program =>
            {
                Ok(())
            }
            PageHostReply::Error { .. } => Err(child_debugger_reply_error(&reply)),
            _ => Err(JavaScriptPageDebuggerError::NoLiveRealm),
        }
    }

    fn step_debugger_root_instruction(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        if !self.debugger_stepping_available() {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        let program = self.child_program_for_core(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        let reply = self
            .child
            .step_debugger_root_instruction(tab_id.as_u64(), document_generation, program)
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        match reply {
            PageHostReply::DebuggerExecutionStepRequested {
                tab_id: reply_tab_id,
                document_generation: reply_generation,
                program: reply_program,
            } if reply_tab_id == tab_id.as_u64()
                && reply_generation == document_generation
                && reply_program == program =>
            {
                Ok(())
            }
            PageHostReply::Error { .. } => Err(child_debugger_reply_error(&reply)),
            _ => Err(JavaScriptPageDebuggerError::NoLiveRealm),
        }
    }

    fn step_debugger_bluets_source_span(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        target: JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        // Compiler-minted source IDs are zero-based. The stream receipt and
        // child attachment, not a nonzero test, establish their authority.
        if !self.debugger_source_span_stepping_available() {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
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
            .step_debugger_bluets_source_span(
                tab_id.as_u64(),
                document_generation,
                child_metadata,
                target.source_id,
                safe_point,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        match reply {
            PageHostReply::DebuggerBlueTsSourceStepRequested {
                tab_id: reply_tab_id,
                document_generation: reply_generation,
                metadata,
                source_id,
                safe_point: reply_safe_point,
            } if reply_tab_id == tab_id.as_u64()
                && reply_generation == document_generation
                && metadata == child_metadata
                && source_id == target.source_id
                && reply_safe_point == safe_point =>
            {
                Ok(())
            }
            PageHostReply::Error { .. } => Err(child_debugger_reply_error(&reply)),
            _ => Err(JavaScriptPageDebuggerError::NoLiveRealm),
        }
    }
}
