// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Private native-debugger support for the bounded JavaScript page executor.
//!
//! This module deliberately owns only opaque identity translation and exact
//! safe-point validation. Page lifecycle, source authorization, binding
//! installation, and execution remain in the parent executor.

use super::*;

/// One opaque live-program identity available to the private native debugger
/// path. It deliberately contains neither source identity nor bytecode: those
/// remain inside the core-owned page executor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JavaScriptPageDebuggerProgram {
    pub program_handle: u64,
    pub program_generation: u64,
}

/// One compiler-verified instruction boundary represented without source or
/// bytecode contents for the native debugger path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JavaScriptPageDebuggerSafePoint {
    pub code_unit_ordinal: u32,
    pub bytecode_offset: u32,
}

/// Fixed failure categories for debugger requests resolved by the current
/// bounded JavaScript page host. None carries page-controlled source or a VM
/// value across the debugger boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JavaScriptPageDebuggerError {
    NoLiveRealm,
    UnknownProgram,
    StaleProgram,
    InvalidSafePoint,
    ResourceLimit,
}

/// Internal association between a core-minted debugger identity and one live
/// BlueJS generation. It is discarded with the page realm; the raw BlueJS
/// handle never crosses the executor's debugger methods.
#[derive(Debug, Clone, Copy)]
pub(super) struct DebuggerProgramRecord {
    program_handle: u64,
    program_generation: u64,
    bluejs_handle: BlueJsProgramHandle,
}

impl JavaScriptPageExecutor {
    /// Whether this executor owns the exact currently-live JavaScript realm
    /// for a tab/document target. Callers use this to avoid advertising a
    /// native program-location operation for an ordinary page with no opted-in
    /// JavaScript runtime.
    pub fn debugger_has_live_realm(&self, tab_id: TabId, document_generation: u64) -> bool {
        self.live_documents
            .get(&tab_id)
            .is_some_and(|identity| identity.document_generation == document_generation)
    }

    /// The fixed maximum number of locations that one private debugger query
    /// may receive for a program in this executor.
    pub fn max_debugger_safe_points_per_program(&self) -> usize {
        self.config.max_debugger_safe_points_per_program
    }

    /// Returns only opaque debugger program identities for one exact live
    /// tab/document realm. The result has no source, canonical module ID,
    /// bytecode, completion value, or VM object identity.
    pub fn debugger_programs(
        &self,
        tab_id: TabId,
        document_generation: u64,
    ) -> Result<Vec<JavaScriptPageDebuggerProgram>, JavaScriptPageDebuggerError> {
        self.require_live_debugger_realm(tab_id, document_generation)?;
        Ok(self
            .debugger_programs
            .get(&tab_id)
            .into_iter()
            .flatten()
            .map(|record| JavaScriptPageDebuggerProgram {
                program_handle: record.program_handle,
                program_generation: record.program_generation,
            })
            .collect())
    }

    /// Enumerates only the exact compiler-verified instruction boundaries for
    /// one opaque live debugger program. It is an inventory operation, not a
    /// request to execute, pause, inspect, or remap source in the VM.
    pub fn debugger_safe_points(
        &self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
    ) -> Result<Vec<JavaScriptPageDebuggerSafePoint>, JavaScriptPageDebuggerError> {
        let record = self.debugger_program_record(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        self.runtime
            .safe_points(
                tab_id.as_u64(),
                record.bluejs_handle,
                self.config.max_debugger_safe_points_per_program,
            )
            .map(|safe_points| {
                safe_points
                    .into_iter()
                    .map(|safe_point| JavaScriptPageDebuggerSafePoint {
                        code_unit_ordinal: safe_point.code_unit.ordinal(),
                        bytecode_offset: safe_point.bytecode_offset,
                    })
                    .collect()
            })
            .map_err(debugger_page_runtime_error)
    }

    /// Revalidates one caller-supplied location against the exact currently
    /// retained BlueJS program generation. The caller cannot construct a
    /// BlueJS handle or safe point through this API, and there is no nearest
    /// source/offset fallback.
    pub fn validate_debugger_safe_point(
        &self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
        code_unit_ordinal: u32,
        bytecode_offset: u32,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        let record = self.debugger_program_record(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        let safe_point = self
            .runtime
            .safe_points(
                tab_id.as_u64(),
                record.bluejs_handle,
                self.config.max_debugger_safe_points_per_program,
            )
            .map_err(debugger_page_runtime_error)?
            .into_iter()
            .find(|safe_point| {
                safe_point.code_unit.ordinal() == code_unit_ordinal
                    && safe_point.bytecode_offset == bytecode_offset
            })
            .ok_or(JavaScriptPageDebuggerError::InvalidSafePoint)?;
        self.runtime
            .validate_safe_point(tab_id.as_u64(), record.bluejs_handle, safe_point)
            .map_err(debugger_page_runtime_error)
    }

    pub(super) fn register_debugger_programs(
        &mut self,
        tab_id: TabId,
        handles: &[BlueJsProgramHandle],
    ) -> Result<(), &'static str> {
        let count = u64::try_from(handles.len())
            .map_err(|_| "native debugger program identities are exhausted")?;
        let next_after = self
            .next_debugger_program_handle
            .checked_add(count)
            .ok_or("native debugger program identities are exhausted")?;
        let mut records = Vec::with_capacity(handles.len());
        for (offset, &bluejs_handle) in handles.iter().enumerate() {
            let offset = u64::try_from(offset)
                .expect("a Vec length convertible to u64 has every index convertible to u64");
            let program_handle = self
                .next_debugger_program_handle
                .checked_add(offset)
                .expect("the preflight checked the complete debugger handle range");
            records.push(DebuggerProgramRecord {
                program_handle,
                program_generation: bluejs_handle.generation().as_u64(),
                bluejs_handle,
            });
        }
        self.next_debugger_program_handle = next_after;
        self.debugger_programs
            .entry(tab_id)
            .or_default()
            .extend(records);
        Ok(())
    }

    fn require_live_debugger_realm(
        &self,
        tab_id: TabId,
        document_generation: u64,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        self.debugger_has_live_realm(tab_id, document_generation)
            .then_some(())
            .ok_or(JavaScriptPageDebuggerError::NoLiveRealm)
    }

    fn debugger_program_record(
        &self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
    ) -> Result<&DebuggerProgramRecord, JavaScriptPageDebuggerError> {
        self.require_live_debugger_realm(tab_id, document_generation)?;
        let record = self
            .debugger_programs
            .get(&tab_id)
            .into_iter()
            .flatten()
            .find(|record| record.program_handle == program_handle)
            .ok_or(JavaScriptPageDebuggerError::UnknownProgram)?;
        (record.program_generation == program_generation)
            .then_some(record)
            .ok_or(JavaScriptPageDebuggerError::StaleProgram)
    }
}

fn debugger_page_runtime_error(error: BlueJsPageRuntimeError) -> JavaScriptPageDebuggerError {
    match error {
        BlueJsPageRuntimeError::SafePointLimit { .. } => JavaScriptPageDebuggerError::ResourceLimit,
        _ => JavaScriptPageDebuggerError::StaleProgram,
    }
}
