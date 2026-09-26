// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

/// The same numeric handle may be minted by a successor core. Bind it to a
/// distinct core instance as well: overlapping processes have different PIDs,
/// and 96 random bits prevent a later PID reuse from recreating this identity.
/// If OS randomness is unavailable, nested-frame minting fails closed.
static CORE_DEBUGGER_INSTANCE: OnceLock<Option<[u8; 16]>> = OnceLock::new();

pub(super) fn core_debugger_instance() -> Result<[u8; 16], JavaScriptPageDebuggerError> {
    (*CORE_DEBUGGER_INSTANCE.get_or_init(|| {
        let mut identity = [0_u8; 16];
        identity[..4].copy_from_slice(&std::process::id().to_le_bytes());
        getrandom::fill(&mut identity[4..]).ok()?;
        Some(identity)
    }))
    .ok_or(JavaScriptPageDebuggerError::ResourceLimit)
}

impl<C> OutOfProcessJavaScriptPageExecutor<C> {
    pub(super) fn has_core_live_document(&self, tab_id: TabId, document_generation: u64) -> bool {
        self.live_documents
            .get(&tab_id)
            .is_some_and(|document| document.document_generation == document_generation)
    }

    pub(super) fn mint_core_debugger_program(
        &mut self,
    ) -> Result<CoreDebuggerProgram, JavaScriptPageDebuggerError> {
        let program_handle = self.next_debugger_program_handle;
        let program_generation = self.next_debugger_program_generation;
        self.next_debugger_program_handle = program_handle
            .checked_add(1)
            .ok_or(JavaScriptPageDebuggerError::ResourceLimit)?;
        self.next_debugger_program_generation = program_generation
            .checked_add(1)
            .ok_or(JavaScriptPageDebuggerError::ResourceLimit)?;
        Ok(CoreDebuggerProgram {
            program_handle,
            program_generation,
        })
    }

    pub(super) fn mint_core_debugger_static_metadata(
        &mut self,
        program: PageHostDebuggerProgram,
    ) -> Result<CoreDebuggerStaticMetadata, JavaScriptPageDebuggerError> {
        let metadata_handle = self.next_debugger_metadata_handle;
        let metadata_generation = self.next_debugger_metadata_generation;
        // Never allow an exhausted metadata counter to cross into the public
        // program namespace; a wrap must fail closed instead of aliasing an
        // unrelated public identifier.
        if metadata_handle >= CORE_CHILD_DEBUGGER_ID_NAMESPACE_START
            || metadata_generation >= CORE_CHILD_DEBUGGER_ID_NAMESPACE_START
        {
            return Err(JavaScriptPageDebuggerError::ResourceLimit);
        }
        self.next_debugger_metadata_handle = metadata_handle
            .checked_add(1)
            .ok_or(JavaScriptPageDebuggerError::ResourceLimit)?;
        self.next_debugger_metadata_generation = metadata_generation
            .checked_add(1)
            .ok_or(JavaScriptPageDebuggerError::ResourceLimit)?;
        Ok(CoreDebuggerStaticMetadata {
            program,
            metadata_handle,
            metadata_generation,
        })
    }

    pub(super) fn child_program_for_core(
        &self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
    ) -> Result<PageHostDebuggerProgram, JavaScriptPageDebuggerError> {
        if !self.has_core_live_document(tab_id, document_generation) {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        self.debugger_programs
            .get(&tab_id)
            .and_then(|programs| {
                programs.iter().find_map(|(child, public)| {
                    (public.program_handle == program_handle
                        && public.program_generation == program_generation)
                        .then_some(*child)
                })
            })
            .ok_or(JavaScriptPageDebuggerError::UnknownProgram)
    }

    pub(super) fn remint_core_debugger_frame(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
        child: PageHostDebuggerFrame,
    ) -> Result<JavaScriptPageDebuggerFrame, JavaScriptPageDebuggerError> {
        if let Some(active) = self.debugger_nested_frames.get(&tab_id) {
            if active.child == child
                && active.public.document_generation == document_generation
                && active.public.program_handle == program_handle
                && active.public.program_generation == program_generation
            {
                return Ok(active.public);
            }
        }
        let frame_handle = NEXT_CORE_DEBUGGER_FRAME_HANDLE
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .map_err(|_| JavaScriptPageDebuggerError::ResourceLimit)?;
        let core_instance = core_debugger_instance()?;
        let public = JavaScriptPageDebuggerFrame {
            tab_id,
            document_generation,
            program_handle,
            program_generation,
            code_unit_ordinal: child.code_unit_ordinal,
            core_instance,
            frame_handle,
        };
        self.debugger_nested_frames
            .insert(tab_id, ActiveDebuggerFrame { public, child });
        Ok(public)
    }

    pub(super) fn child_static_metadata_for_core(
        &self,
        tab_id: TabId,
        document_generation: u64,
        child_program: PageHostDebuggerProgram,
        metadata_handle: u64,
        metadata_generation: u64,
    ) -> Result<PageHostDebuggerMetadataHandle, JavaScriptPageDebuggerError> {
        if !self.has_core_live_document(tab_id, document_generation) {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        self.debugger_static_metadata
            .get(&tab_id)
            .and_then(|metadata| {
                metadata.iter().find_map(|(child_metadata, public)| {
                    (public.program == child_program
                        && public.metadata_handle == metadata_handle
                        && public.metadata_generation == metadata_generation)
                        .then_some(*child_metadata)
                })
            })
            .ok_or(JavaScriptPageDebuggerError::UnknownProgram)
    }

    pub(super) fn core_program_for_child(
        &self,
        tab_id: TabId,
        document_generation: u64,
        child_program: PageHostDebuggerProgram,
    ) -> Result<CoreDebuggerProgram, JavaScriptPageDebuggerError> {
        if !child_program.is_well_formed()
            || !self.has_core_live_document(tab_id, document_generation)
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        self.debugger_programs
            .get(&tab_id)
            .and_then(|programs| programs.get(&child_program))
            .copied()
            .ok_or(JavaScriptPageDebuggerError::UnknownProgram)
    }
}
