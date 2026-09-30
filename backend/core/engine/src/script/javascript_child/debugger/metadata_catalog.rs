// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl<C: PageHostClient> OutOfProcessJavaScriptPageExecutor<C> {
    pub(super) fn core_debugger_static_metadata(
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

    pub(super) fn core_debugger_static_metadata_summary(
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

    pub(super) fn core_debugger_static_metadata_lowering_summary(
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

    pub(super) fn core_debugger_static_metadata_sources(
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

    pub(super) fn core_debugger_static_metadata_source_provenance(
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

    pub(super) fn core_debugger_static_metadata_types(
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

    pub(super) fn core_debugger_static_metadata_type_display(
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

    pub(super) fn core_debugger_static_metadata_symbols(
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

    pub(super) fn core_debugger_static_metadata_contracts(
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

    pub(super) fn core_debugger_static_metadata_contract_display(
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

    pub(super) fn core_debugger_static_metadata_contract_validation(
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

    pub(super) fn core_debugger_static_metadata_symbol_display(
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
}
