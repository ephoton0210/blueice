// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Core-owned debugger identity-map payload for one exact live document.

use super::*;

impl<C: PageHostClient> OutOfProcessJavaScriptPageExecutor<C> {
    /// Counts logical map-entry payload for core-minted program and metadata
    /// identities plus the execution-deferral record. Child metadata and
    /// active frame state are distinct owners and are not counted here.
    pub fn retained_debugger_identity_map_bytes(
        &self,
        tab_id: TabId,
        document_generation: u64,
    ) -> Option<usize> {
        let document = self.live_documents.get(&tab_id)?;
        if document.document_generation != document_generation {
            return None;
        }
        let mut bytes = 0usize;
        if let Some(deferral) = self.debugger_execution_deferrals.get(&tab_id) {
            if deferral.document_generation != document_generation {
                return None;
            }
            bytes = bytes.checked_add(std::mem::size_of::<(TabId, DebuggerExecutionDeferral)>())?;
        }
        if let Some(programs) = self.debugger_programs.get(&tab_id) {
            bytes = bytes
                .checked_add(std::mem::size_of::<(
                    TabId,
                    BTreeMap<PageHostDebuggerProgram, CoreDebuggerProgram>,
                )>())?
                .checked_add(programs.len().checked_mul(std::mem::size_of::<(
                    PageHostDebuggerProgram,
                    CoreDebuggerProgram,
                )>())?)?;
        }
        if let Some(metadata) = self.debugger_static_metadata.get(&tab_id) {
            bytes = bytes
                .checked_add(std::mem::size_of::<(
                    TabId,
                    BTreeMap<PageHostDebuggerMetadataHandle, CoreDebuggerStaticMetadata>,
                )>())?
                .checked_add(metadata.len().checked_mul(std::mem::size_of::<(
                    PageHostDebuggerMetadataHandle,
                    CoreDebuggerStaticMetadata,
                )>())?)?;
        }
        Some(bytes)
    }

    /// Counts core-owned active nested/linked frame map entries and the two
    /// retained linked scope vectors for one exact live document.
    pub fn retained_debugger_active_frame_bytes(
        &self,
        tab_id: TabId,
        document_generation: u64,
    ) -> Option<usize> {
        let document = self.live_documents.get(&tab_id)?;
        if document.document_generation != document_generation {
            return None;
        }
        let mut bytes = 0usize;
        if let Some(frame) = self.debugger_nested_frames.get(&tab_id) {
            if frame.public.tab_id != tab_id
                || frame.public.document_generation != document_generation
                || frame.child.tab_id != tab_id.as_u64()
                || frame.child.document_generation != document_generation
            {
                return None;
            }
            bytes = bytes.checked_add(std::mem::size_of::<(TabId, ActiveDebuggerFrame)>())?;
        }
        if let Some(pause) = self.debugger_linked_frames.get(&tab_id) {
            if pause.child.tab_id != tab_id.as_u64()
                || pause.child.document_generation != document_generation
                || pause.frames.iter().any(|frame| {
                    frame.frame.tab_id != tab_id
                        || frame.frame.document_generation != document_generation
                })
            {
                return None;
            }
            bytes = bytes.checked_add(std::mem::size_of::<(TabId, ActiveLinkedDebuggerPause)>())?;
            for frame in &pause.child_stack.frames {
                bytes = bytes.checked_add(
                    frame
                        .scope_entries
                        .capacity()
                        .checked_mul(std::mem::size_of::<PageHostDebuggerScopeEntry>())?,
                )?;
            }
        }
        Some(bytes)
    }

    /// Complete core-owned debugger-map payload for one live generation.
    pub fn retained_debugger_payload_bytes(
        &self,
        tab_id: TabId,
        document_generation: u64,
    ) -> Option<usize> {
        self.retained_debugger_identity_map_bytes(tab_id, document_generation)?
            .checked_add(self.retained_debugger_active_frame_bytes(tab_id, document_generation)?)
    }
}
