// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
mod linked_frames;
mod metadata_bindings;
mod metadata_catalog;
mod nested_scopes;

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
        self.core_debugger_static_metadata(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )
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
        self.core_debugger_static_metadata_summary(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
            metadata_handle,
            metadata_generation,
        )
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
        self.core_debugger_static_metadata_lowering_summary(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
            metadata_handle,
            metadata_generation,
        )
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
        self.core_debugger_static_metadata_sources(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
            metadata_handle,
            metadata_generation,
        )
    }

    fn debugger_static_metadata_source_provenance(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        target: JavaScriptPageDebuggerStaticMetadataSourceTarget,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataSourceProvenance, JavaScriptPageDebuggerError>
    {
        self.core_debugger_static_metadata_source_provenance(tab_id, document_generation, target)
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
        self.core_debugger_static_metadata_types(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
            metadata_handle,
            metadata_generation,
        )
    }

    fn debugger_static_metadata_type_display(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        target: JavaScriptPageDebuggerStaticMetadataTypeTarget,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataTypeDisplay, JavaScriptPageDebuggerError> {
        self.core_debugger_static_metadata_type_display(tab_id, document_generation, target)
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
        self.core_debugger_static_metadata_symbols(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
            metadata_handle,
            metadata_generation,
        )
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
        self.core_debugger_static_metadata_contracts(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
            metadata_handle,
            metadata_generation,
        )
    }

    fn debugger_static_metadata_contract_display(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        target: JavaScriptPageDebuggerStaticMetadataContractTarget,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataContractDisplay, JavaScriptPageDebuggerError>
    {
        self.core_debugger_static_metadata_contract_display(tab_id, document_generation, target)
    }

    fn debugger_static_metadata_contract_validation(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        target: JavaScriptPageDebuggerStaticMetadataContractTarget,
        value: CompilerContractValue,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataContractValidation, JavaScriptPageDebuggerError>
    {
        self.core_debugger_static_metadata_contract_validation(
            tab_id,
            document_generation,
            target,
            value,
        )
    }

    fn debugger_static_metadata_symbol_display(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        target: JavaScriptPageDebuggerStaticMetadataSymbolTarget,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataSymbolDisplay, JavaScriptPageDebuggerError>
    {
        self.core_debugger_static_metadata_symbol_display(tab_id, document_generation, target)
    }

    fn debugger_static_metadata_symbol_location(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        target: JavaScriptPageDebuggerStaticMetadataSymbolLocationTarget,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataSymbolLocation, JavaScriptPageDebuggerError>
    {
        self.core_debugger_static_metadata_symbol_location(tab_id, document_generation, target)
    }

    fn debugger_static_metadata_safe_point_span(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        target: JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataSafePointSpan, JavaScriptPageDebuggerError>
    {
        self.core_debugger_static_metadata_safe_point_span(tab_id, document_generation, target)
    }

    fn debugger_exception_location(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        target: JavaScriptPageDebuggerExceptionLocationTarget,
    ) -> Result<JavaScriptPageDebuggerExceptionLocation, JavaScriptPageDebuggerError> {
        self.core_debugger_exception_location(tab_id, document_generation, target)
    }

    fn debugger_static_metadata_source_breakpoint(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        target: JavaScriptPageDebuggerStaticMetadataSourceBreakpointTarget,
    ) -> Result<Option<JavaScriptPageDebuggerSafePoint>, JavaScriptPageDebuggerError> {
        self.core_debugger_static_metadata_source_breakpoint(tab_id, document_generation, target)
    }

    fn debugger_static_metadata_contract_location(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        target: JavaScriptPageDebuggerStaticMetadataContractLocationTarget,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataContractLocation, JavaScriptPageDebuggerError>
    {
        self.core_debugger_static_metadata_contract_location(tab_id, document_generation, target)
    }

    fn debugger_static_metadata_symbol_type(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        target: JavaScriptPageDebuggerStaticMetadataSymbolTypeTarget,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataSymbolType, JavaScriptPageDebuggerError> {
        self.core_debugger_static_metadata_symbol_type(tab_id, document_generation, target)
    }

    fn debugger_static_metadata_symbol_contract(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        target: JavaScriptPageDebuggerStaticMetadataSymbolContractTarget,
    ) -> Result<JavaScriptPageDebuggerStaticMetadataSymbolContract, JavaScriptPageDebuggerError>
    {
        self.core_debugger_static_metadata_symbol_contract(tab_id, document_generation, target)
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
        self.core_arm_debugger_linked_nested_safe_point_breakpoint(
            tab_id,
            document_generation,
            entry,
            dependency,
            safe_point,
        )
    }

    fn debugger_linked_execution_state(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        entry: JavaScriptPageDebuggerProgram,
    ) -> Result<JavaScriptPageDebuggerLinkedExecutionState, JavaScriptPageDebuggerError> {
        self.core_debugger_linked_execution_state(tab_id, document_generation, entry)
    }

    fn debugger_linked_stack_snapshot(
        &mut self,
        top_frame: JavaScriptPageDebuggerFrame,
        max_scope_entries: u32,
    ) -> Result<JavaScriptPageDebuggerLinkedStackSnapshot, JavaScriptPageDebuggerError> {
        self.core_debugger_linked_stack_snapshot(top_frame, max_scope_entries)
    }

    fn debugger_linked_scope_snapshot(
        &mut self,
        expected_stack: JavaScriptPageDebuggerLinkedStackSnapshot,
    ) -> Result<JavaScriptPageDebuggerLinkedScopeSnapshot, JavaScriptPageDebuggerError> {
        self.core_debugger_linked_scope_snapshot(expected_stack)
    }

    fn resume_debugger_linked_nested_execution(
        &mut self,
        top_frame: JavaScriptPageDebuggerFrame,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        self.core_resume_debugger_linked_nested_execution(top_frame)
    }

    fn debugger_linked_stack_spans(
        &mut self,
        expected_stack: JavaScriptPageDebuggerLinkedStackSnapshot,
        access: JavaScriptPageDebuggerLinkedSpanAccess,
    ) -> Result<[JavaScriptPageDebuggerStaticMetadataSafePointSpan; 2], JavaScriptPageDebuggerError>
    {
        self.core_debugger_linked_stack_spans(expected_stack, access)
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
        self.core_arm_debugger_nested_safe_point_breakpoint(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
            code_unit_ordinal,
            bytecode_offset,
        )
    }

    fn debugger_nested_execution_state(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
    ) -> Result<Option<JavaScriptPageDebuggerNestedExecutionState>, JavaScriptPageDebuggerError>
    {
        self.core_debugger_nested_execution_state(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )
    }

    fn step_debugger_nested_instruction(
        &mut self,
        frame: JavaScriptPageDebuggerFrame,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        self.core_step_debugger_nested_instruction(frame)
    }

    fn resume_debugger_nested_execution(
        &mut self,
        frame: JavaScriptPageDebuggerFrame,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        self.core_resume_debugger_nested_execution(frame)
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
        self.core_debugger_stack_snapshot(
            tab_id,
            document_generation,
            core_program,
            frame,
            max_frames,
            max_scope_entries,
        )
    }

    fn debugger_static_scope_relation(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        target: JavaScriptPageDebuggerStaticScopeTarget,
    ) -> Result<JavaScriptPageDebuggerStaticScopeRelation, JavaScriptPageDebuggerError> {
        self.core_debugger_static_scope_relation(tab_id, document_generation, target)
    }

    fn debugger_value_snapshot(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        target: JavaScriptPageDebuggerValueTarget,
    ) -> Result<JavaScriptPageDebuggerValuePreview, JavaScriptPageDebuggerError> {
        self.core_debugger_value_snapshot(tab_id, document_generation, target)
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
