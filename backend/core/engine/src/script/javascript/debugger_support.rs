// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Private native-debugger support for the bounded JavaScript page executor.
//!
//! This module owns opaque identity translation, exact safe-point validation,
//! bounded breakpoint configuration, and the deliberately narrow root-code
//! unit continuation seam. Page lifecycle, source authorization, binding
//! installation, and ordinary execution remain in sibling modules.

use super::execution_support::DeferredJavaScriptExecution;
use super::*;

/// One opaque live-program identity available to the private native debugger
/// path. It deliberately contains neither source identity nor bytecode: those
/// remain inside the core-owned page executor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JavaScriptPageDebuggerProgram {
    pub program_handle: u64,
    pub program_generation: u64,
}

/// One source-free static-metadata inventory identity for an exact live
/// program. The core remints it from a child-private handle, so neither the
/// child handle nor any compiler source/type/symbol/span/contract data crosses
/// the page-executor boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JavaScriptPageDebuggerStaticMetadata {
    pub metadata_handle: u64,
    pub metadata_generation: u64,
}

/// A bounded source-free summary for one exact static-metadata inventory
/// identity. It deliberately contains no source identity/text, span, name,
/// type display, symbol, contract, bytecode, VM object, or runtime value.
/// The debugger dispatcher binds it back to the public opaque handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaScriptPageDebuggerStaticMetadataSummary {
    pub language_version: String,
    pub compiler_options_hash: String,
    pub source_count: u32,
    pub type_count: u32,
    pub symbol_count: u32,
    pub contract_count: u32,
}

/// One compiler-minted source-record identity for an exact static metadata
/// attachment. It has no module, hash, text, span, or record payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JavaScriptPageDebuggerStaticMetadataSourceId {
    pub source_id: u32,
}

/// One compiler-minted type-record identity for an exact static metadata
/// attachment. It contains no type display or static-record payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JavaScriptPageDebuggerStaticMetadataTypeId {
    pub type_id: u32,
}

/// One compiler-minted symbol-record identity for an exact static metadata
/// attachment. It contains no name, source span, declared type, contract, or
/// static-record payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JavaScriptPageDebuggerStaticMetadataSymbolId {
    pub symbol_id: u32,
}

/// One compiler-minted contract identity for an exact static metadata
/// attachment. It contains no contract name, source span, plan, validation,
/// or static-record payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JavaScriptPageDebuggerStaticMetadataContractId {
    pub contract_id: u32,
}

/// One child-validated compiler-produced display for an exact symbol identity.
/// It carries no source span, static type, contract, bytecode, VM object,
/// value, or arbitrary metadata record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaScriptPageDebuggerStaticMetadataSymbolDisplay {
    pub symbol_id: u32,
    pub display: String,
}

/// One exact opaque parent and compiler-minted symbol-ID target for the
/// separately authorized symbol-display operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JavaScriptPageDebuggerStaticMetadataSymbolTarget {
    pub program_handle: u64,
    pub program_generation: u64,
    pub metadata_handle: u64,
    pub metadata_generation: u64,
    pub symbol_id: u32,
}

/// One child-validated compiler-produced display for an exact static type
/// identity. It carries no source text, span, symbol, contract, bytecode, VM
/// object, value, or arbitrary metadata record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaScriptPageDebuggerStaticMetadataTypeDisplay {
    pub type_id: u32,
    pub display: String,
}

/// One child-validated source-text-free provenance description. The caller
/// supplies the parent metadata handle and compiler-minted source ID; this
/// internal transport value never carries a source read capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaScriptPageDebuggerStaticMetadataSourceProvenance {
    pub source_id: u32,
    pub module: String,
    pub content_hash: String,
}

/// One exact opaque parent and compiler-minted source-ID target for the
/// separately authorized source-provenance operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JavaScriptPageDebuggerStaticMetadataSourceTarget {
    pub program_handle: u64,
    pub program_generation: u64,
    pub metadata_handle: u64,
    pub metadata_generation: u64,
    pub source_id: u32,
}

/// One exact opaque parent and compiler-minted type-ID target for the
/// separately authorized type-display operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JavaScriptPageDebuggerStaticMetadataTypeTarget {
    pub program_handle: u64,
    pub program_generation: u64,
    pub metadata_handle: u64,
    pub metadata_generation: u64,
    pub type_id: u32,
}

/// One compiler-verified instruction boundary represented without source or
/// bytecode contents for the native debugger path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JavaScriptPageDebuggerSafePoint {
    pub code_unit_ordinal: u32,
    pub bytecode_offset: u32,
}

/// One source-free, exact breakpoint record retained for a live program. A
/// configured record does not imply that the synchronous page runtime has
/// paused or can resume at this location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JavaScriptPageDebuggerBreakpoint {
    pub program_handle: u64,
    pub program_generation: u64,
    pub code_unit_ordinal: u32,
    pub bytecode_offset: u32,
}

/// Source-free execution state for the opt-in native debugger root-frame
/// continuation seam. `Paused` can name a non-entry root instruction, but
/// this intentionally does not imply arbitrary interpreter continuation,
/// stack inspection, nested-function pause, or stepping support.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JavaScriptPageDebuggerExecutionState {
    Pending,
    Paused {
        code_unit_ordinal: u32,
        bytecode_offset: u32,
    },
    Resuming,
    Completed,
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
    BreakpointLimit,
    ExecutionControlUnavailable,
    NotExecutableEntry,
    NotResumableRootSafePoint,
    InvalidExecutionState,
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

/// Internal orderable form of a source-free breakpoint record. It excludes
/// BlueJS handles, source identities, bytecode, VM state, and result values.
/// The parent drops the complete set before a realm successor becomes live.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct DebuggerBreakpointRecord {
    program_handle: u64,
    program_generation: u64,
    code_unit_ordinal: u32,
    bytecode_offset: u32,
}

/// The identity portion of a debugger program record. It is kept separately
/// from a source-free breakpoint because completed entry-pause state does not
/// need to retain an instruction location.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct DebuggerProgramKey {
    program_handle: u64,
    program_generation: u64,
}

/// Session-thread-only execution state. A pending state becomes paused at an
/// armed root-code-unit boundary. `ResumeRequested` is transient: the same
/// session tick either resumes the retained BlueJS frame or invokes ordinary
/// execution for the legacy entry boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DebuggerExecutionStatus {
    Pending,
    Paused(DebuggerBreakpointRecord),
    ResumeRequested,
    Completed,
}

/// One admitted declaration waiting for the native-debugger scheduler. It
/// never owns source text, bytecode, a VM, a completion value, or a page
/// object: the runtime retains those behind tab-local handles.
#[derive(Debug, Clone)]
pub(super) struct PendingDebuggerExecution {
    program: DebuggerProgramKey,
    /// The current arm target. It starts at root entry for compatibility and
    /// is replaced only by the explicit root-safe-point arm operation while
    /// this declaration is still pending.
    entry_breakpoint: DebuggerBreakpointRecord,
    /// A root-safe-point continuation is deliberately one-shot. Keeping this
    /// state with the pending declaration prevents a second debugger peer (or
    /// a pipelined request) from moving the execution target before the
    /// scheduler gets its next turn. Ordinary breakpoint configuration and
    /// the v4 root-entry compatibility arm do not set this flag.
    root_safe_point_armed: bool,
    document_generation: u64,
    ordinal: u32,
    kind: BlueJsPageScriptKind,
    execution: DeferredJavaScriptExecution,
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

    /// The maximum number of exact breakpoint configuration records the core
    /// retains for one live realm. This is a storage limit, not an execution
    /// or pause limit.
    pub fn max_debugger_breakpoints_per_realm(&self) -> usize {
        self.config.max_debugger_breakpoints_per_realm
    }

    /// Whether this explicit executor configuration has the native entry-pause
    /// scheduler installed. A debugger socket alone is insufficient: normal
    /// inline JavaScript execution intentionally retains its historic
    /// immediate scheduling behavior.
    pub fn debugger_execution_control_available(&self) -> bool {
        self.config.native_debugger_execution_control
    }

    /// Preserves one pending root-entry declaration across the next lifecycle
    /// synchronization after a handshaken debugger discovery request. The
    /// request receiver uses this bounded one-turn hold so a peer can obtain
    /// the opaque program and verified entry location before ordinary idle
    /// scheduling starts it. An arm or resume request deliberately does not
    /// take this hold: its following lifecycle turn must pause or execute.
    pub(crate) fn hold_pending_debugger_execution_once(&mut self) {
        if self.debugger_execution_control_available()
            && self
                .pending_debugger_executions
                .values()
                .any(|pending| !pending.is_empty())
        {
            self.hold_pending_debugger_execution_once = true;
        }
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

    /// Stores one exact compiler-verified breakpoint record for a current
    /// opaque program generation. This is intentionally idempotent: retrying
    /// a request cannot consume additional bounded realm storage. It neither
    /// executes page code nor changes the synchronous VM's control flow.
    pub fn set_debugger_breakpoint(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
        code_unit_ordinal: u32,
        bytecode_offset: u32,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        self.validate_debugger_safe_point(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
            code_unit_ordinal,
            bytecode_offset,
        )?;
        self.insert_debugger_breakpoint(
            tab_id,
            DebuggerBreakpointRecord {
                program_handle,
                program_generation,
                code_unit_ordinal,
                bytecode_offset,
            },
        )
    }

    /// Arms a compiler-verified root-entry breakpoint for a declaration that
    /// is admitted but has not entered the BlueJS VM. This is intentionally
    /// narrower than generic breakpoint configuration: the root entry has a
    /// zero-execution continuation, so resume can safely invoke the ordinary
    /// VM entry point without claiming an arbitrary interpreter continuation.
    pub fn arm_debugger_entry_breakpoint(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
        code_unit_ordinal: u32,
        bytecode_offset: u32,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        if !self.debugger_execution_control_available() {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        self.validate_debugger_safe_point(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
            code_unit_ordinal,
            bytecode_offset,
        )?;
        let key = DebuggerProgramKey {
            program_handle,
            program_generation,
        };
        let entry_breakpoint = self.debugger_entry_breakpoint(tab_id, key)?;
        if entry_breakpoint.code_unit_ordinal != code_unit_ordinal
            || entry_breakpoint.bytecode_offset != bytecode_offset
        {
            return Err(JavaScriptPageDebuggerError::NotExecutableEntry);
        }
        let status = self
            .debugger_execution_states
            .get(&tab_id)
            .and_then(|states| states.get(&key))
            .copied()
            .ok_or(JavaScriptPageDebuggerError::NotExecutableEntry)?;
        if status != DebuggerExecutionStatus::Pending {
            return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
        }
        self.insert_debugger_breakpoint(tab_id, entry_breakpoint)?;
        Ok(())
    }

    /// Arms one verified root-code-unit location for a still-pending classic
    /// script. A nonzero offset is reached by the BlueJS continuation API,
    /// which preserves the root interpreter frame before returning control to
    /// this session thread. Modules and child code units are rejected because
    /// their continuation state is not represented by this scheduler.
    #[allow(clippy::too_many_arguments)]
    pub fn arm_debugger_root_safe_point_breakpoint(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
        code_unit_ordinal: u32,
        bytecode_offset: u32,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        if !self.debugger_execution_control_available() {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        self.validate_debugger_safe_point(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
            code_unit_ordinal,
            bytecode_offset,
        )?;
        if code_unit_ordinal != 0 {
            return Err(JavaScriptPageDebuggerError::NotResumableRootSafePoint);
        }
        let key = DebuggerProgramKey {
            program_handle,
            program_generation,
        };
        let status = self
            .debugger_execution_states
            .get(&tab_id)
            .and_then(|states| states.get(&key))
            .copied()
            .ok_or(JavaScriptPageDebuggerError::NotResumableRootSafePoint)?;
        if status != DebuggerExecutionStatus::Pending {
            return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
        }
        let target = DebuggerBreakpointRecord {
            program_handle,
            program_generation,
            code_unit_ordinal,
            bytecode_offset,
        };
        let pending = self
            .pending_debugger_executions
            .get(&tab_id)
            .into_iter()
            .flatten()
            .find(|pending| pending.program == key)
            .ok_or(JavaScriptPageDebuggerError::NotResumableRootSafePoint)?;
        if pending.root_safe_point_armed {
            return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
        }
        if !matches!(
            pending.execution,
            DeferredJavaScriptExecution::Classic { .. }
        ) {
            return Err(JavaScriptPageDebuggerError::NotResumableRootSafePoint);
        }
        self.insert_debugger_breakpoint(tab_id, target)?;
        let pending = self
            .pending_debugger_executions
            .get_mut(&tab_id)
            .expect("the inspected pending debugger queue remains live")
            .iter_mut()
            .find(|pending| pending.program == key)
            .expect("the inspected pending debugger declaration remains queued");
        pending.entry_breakpoint = target;
        pending.root_safe_point_armed = true;
        Ok(())
    }

    /// Reads only source-free scheduling state for one pending, paused, or
    /// completed root-entry declaration. Programs admitted in the ordinary
    /// immediate-scheduling mode never expose this operation.
    pub fn debugger_execution_state(
        &self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
    ) -> Result<JavaScriptPageDebuggerExecutionState, JavaScriptPageDebuggerError> {
        if !self.debugger_execution_control_available() {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        self.debugger_program_record(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        let status = self
            .debugger_execution_states
            .get(&tab_id)
            .and_then(|states| {
                states.get(&DebuggerProgramKey {
                    program_handle,
                    program_generation,
                })
            })
            .copied()
            .ok_or(JavaScriptPageDebuggerError::NotExecutableEntry)?;
        Ok(public_execution_state(status))
    }

    /// Authorizes one paused root-entry declaration to pass through the
    /// session-owned scheduler. The reply does not carry a completion value;
    /// callers observe only a later source-free completion state.
    pub fn resume_debugger_execution(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        if !self.debugger_execution_control_available() {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        self.debugger_program_record(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        let Some(status) = self
            .debugger_execution_states
            .get_mut(&tab_id)
            .and_then(|states| {
                states.get_mut(&DebuggerProgramKey {
                    program_handle,
                    program_generation,
                })
            })
        else {
            return Err(JavaScriptPageDebuggerError::NotExecutableEntry);
        };
        if !matches!(status, DebuggerExecutionStatus::Paused(_)) {
            return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
        }
        *status = DebuggerExecutionStatus::ResumeRequested;
        Ok(())
    }

    /// Returns the exact, source-free breakpoint configuration retained for a
    /// current realm. The records are sorted by opaque identity and compiler
    /// boundary, never by source text or a VM address.
    pub fn debugger_breakpoints(
        &self,
        tab_id: TabId,
        document_generation: u64,
    ) -> Result<Vec<JavaScriptPageDebuggerBreakpoint>, JavaScriptPageDebuggerError> {
        self.require_live_debugger_realm(tab_id, document_generation)?;
        Ok(self
            .debugger_breakpoints
            .get(&tab_id)
            .into_iter()
            .flatten()
            .map(|breakpoint| JavaScriptPageDebuggerBreakpoint {
                program_handle: breakpoint.program_handle,
                program_generation: breakpoint.program_generation,
                code_unit_ordinal: breakpoint.code_unit_ordinal,
                bytecode_offset: breakpoint.bytecode_offset,
            })
            .collect())
    }

    /// Removes one current exact breakpoint record after revalidating its
    /// complete program-generation and compiler-boundary tuple. An already
    /// absent exact record returns `false`; a stale/malformed target remains
    /// an error rather than silently affecting a successor program.
    pub fn clear_debugger_breakpoint(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
        code_unit_ordinal: u32,
        bytecode_offset: u32,
    ) -> Result<bool, JavaScriptPageDebuggerError> {
        self.validate_debugger_safe_point(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
            code_unit_ordinal,
            bytecode_offset,
        )?;
        Ok(self
            .debugger_breakpoints
            .get_mut(&tab_id)
            .is_some_and(|breakpoints| {
                breakpoints.remove(&DebuggerBreakpointRecord {
                    program_handle,
                    program_generation,
                    code_unit_ordinal,
                    bytecode_offset,
                })
            }))
    }

    /// Registers the execution record only after BlueJS admission and opaque
    /// debugger-program registration both succeeded. The first scheduler turn
    /// after admission is deliberately deferred, giving the remote owner one
    /// session turn to inspect exact locations and arm root entry.
    pub(super) fn defer_debugger_execution(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        ordinal: u32,
        kind: BlueJsPageScriptKind,
        execution: DeferredJavaScriptExecution,
    ) -> Result<(), &'static str> {
        debug_assert!(self.debugger_execution_control_available());
        let bluejs_handle = match &execution {
            DeferredJavaScriptExecution::Classic { handle }
            | DeferredJavaScriptExecution::ModuleGraph { entry: handle, .. } => *handle,
        };
        let record = self
            .debugger_program_record_for_bluejs(tab_id, bluejs_handle)
            .ok_or("native debugger entry scheduling lost its program identity")?;
        let program = DebuggerProgramKey {
            program_handle: record.program_handle,
            program_generation: record.program_generation,
        };
        let entry_breakpoint = self
            .debugger_entry_breakpoint(tab_id, program)
            .map_err(|_| "BlueJS program has no compiler-verified root entry boundary")?;
        let states = self.debugger_execution_states.entry(tab_id).or_default();
        if states
            .insert(program, DebuggerExecutionStatus::Pending)
            .is_some()
        {
            return Err("native debugger entry scheduling duplicated a program identity");
        }
        self.pending_debugger_executions
            .entry(tab_id)
            .or_default()
            .push_back(PendingDebuggerExecution {
                program,
                entry_breakpoint,
                root_safe_point_armed: false,
                document_generation,
                ordinal,
                kind,
                execution,
            });
        Ok(())
    }

    /// Advances at most the currently runnable declaration sequence for every
    /// tab. A paused declaration blocks later same-tab declarations, retaining
    /// classic-script order. This runs only on the core session thread and
    /// never waits for a debugger socket.
    pub(super) fn drive_debugger_executions(&mut self) {
        let tab_ids: Vec<_> = self.pending_debugger_executions.keys().copied().collect();
        for tab_id in tab_ids {
            while let Some(next) = self
                .pending_debugger_executions
                .get(&tab_id)
                .and_then(|pending| pending.front())
                .cloned()
            {
                let status = self
                    .debugger_execution_states
                    .get(&tab_id)
                    .and_then(|states| states.get(&next.program))
                    .copied();
                let Some(status) = status else {
                    // This cannot happen without an internal lifecycle bug;
                    // fail closed by dropping the orphaned work rather than
                    // executing an untracked declaration.
                    let _ = self
                        .pending_debugger_executions
                        .get_mut(&tab_id)
                        .and_then(VecDeque::pop_front);
                    self.push_deferred_rejection(
                        tab_id,
                        &next,
                        "native debugger execution state was lost",
                    );
                    continue;
                };
                match status {
                    DebuggerExecutionStatus::Paused(_) => break,
                    DebuggerExecutionStatus::Pending
                        if self
                            .debugger_breakpoints
                            .get(&tab_id)
                            .is_some_and(|breakpoints| {
                                breakpoints.contains(&next.entry_breakpoint)
                            }) =>
                    {
                        if next.entry_breakpoint.bytecode_offset == 0 {
                            self.debugger_execution_states
                                .get_mut(&tab_id)
                                .expect("a queued debugger execution has state")
                                .insert(
                                    next.program,
                                    DebuggerExecutionStatus::Paused(next.entry_breakpoint),
                                );
                            break;
                        }
                        // A non-entry root safe point has to execute real
                        // bytecode before it can pause. The page runtime owns
                        // the preserved operand/handler/iterator state; this
                        // scheduler retains only the source-free identity.
                        let result = match &next.execution {
                            DeferredJavaScriptExecution::Classic { handle } => self
                                .runtime
                                .execute_program_until_debugger_pause_at_root_offset(
                                    tab_id.as_u64(),
                                    *handle,
                                    next.entry_breakpoint.bytecode_offset,
                                )
                                .map_err(page_runtime_category),
                            DeferredJavaScriptExecution::ModuleGraph { .. } => Err(
                                "native debugger root continuation supports classic scripts only",
                            ),
                        };
                        match result {
                            Ok(BlueJsPageDebuggerExecutionState::Paused { .. }) => {
                                self.debugger_execution_states
                                    .get_mut(&tab_id)
                                    .expect("a queued debugger execution has state")
                                    .insert(
                                        next.program,
                                        DebuggerExecutionStatus::Paused(next.entry_breakpoint),
                                    );
                                break;
                            }
                            Ok(BlueJsPageDebuggerExecutionState::Completed) => {
                                // The exact compiler boundary was validated
                                // before start, so this defensive branch is a
                                // terminal completion rather than a silent
                                // claim that a pause occurred.
                                let pending = self
                                    .pending_debugger_executions
                                    .get_mut(&tab_id)
                                    .and_then(VecDeque::pop_front)
                                    .expect(
                                        "the inspected pending debugger execution remains queued",
                                    );
                                self.debugger_execution_states
                                    .get_mut(&tab_id)
                                    .expect("a queued debugger execution has state")
                                    .insert(pending.program, DebuggerExecutionStatus::Completed);
                                self.push_report(JavaScriptPageExecutionReport::Executed {
                                    tab_id: tab_id.as_u64(),
                                    document_generation: pending.document_generation,
                                    ordinal: pending.ordinal,
                                    kind: pending.kind,
                                });
                            }
                            Err(category) => {
                                let pending = self
                                    .pending_debugger_executions
                                    .get_mut(&tab_id)
                                    .and_then(VecDeque::pop_front)
                                    .expect(
                                        "the inspected pending debugger execution remains queued",
                                    );
                                self.debugger_execution_states
                                    .get_mut(&tab_id)
                                    .expect("a queued debugger execution has state")
                                    .insert(pending.program, DebuggerExecutionStatus::Completed);
                                self.push_deferred_rejection(tab_id, &pending, category);
                            }
                        }
                    }
                    DebuggerExecutionStatus::ResumeRequested
                        if next.entry_breakpoint.bytecode_offset != 0 =>
                    {
                        let result = self
                            .runtime
                            .resume_debugger_execution(tab_id.as_u64())
                            .map(|_| ())
                            .map_err(page_runtime_category);
                        let pending = self
                            .pending_debugger_executions
                            .get_mut(&tab_id)
                            .and_then(VecDeque::pop_front)
                            .expect("the inspected pending debugger execution remains queued");
                        self.debugger_execution_states
                            .get_mut(&tab_id)
                            .expect("a queued debugger execution has state")
                            .insert(pending.program, DebuggerExecutionStatus::Completed);
                        match result {
                            Ok(()) => self.push_report(JavaScriptPageExecutionReport::Executed {
                                tab_id: tab_id.as_u64(),
                                document_generation: pending.document_generation,
                                ordinal: pending.ordinal,
                                kind: pending.kind,
                            }),
                            Err(category) => {
                                self.push_deferred_rejection(tab_id, &pending, category)
                            }
                        }
                    }
                    DebuggerExecutionStatus::Pending | DebuggerExecutionStatus::ResumeRequested => {
                        let pending = self
                            .pending_debugger_executions
                            .get_mut(&tab_id)
                            .and_then(VecDeque::pop_front)
                            .expect("the inspected pending debugger execution remains queued");
                        let result = match &pending.execution {
                            DeferredJavaScriptExecution::Classic { handle } => self
                                .runtime
                                .execute_program(tab_id.as_u64(), *handle)
                                .map(|_: Value| ())
                                .map_err(page_runtime_category),
                            DeferredJavaScriptExecution::ModuleGraph { entry, installed } => self
                                .runtime
                                .execute_module_graph(tab_id.as_u64(), *entry, installed.clone())
                                .map(|_: Value| ())
                                .map_err(page_runtime_category),
                        };
                        self.debugger_execution_states
                            .get_mut(&tab_id)
                            .expect("a queued debugger execution has state")
                            .insert(pending.program, DebuggerExecutionStatus::Completed);
                        match result {
                            Ok(()) => self.push_report(JavaScriptPageExecutionReport::Executed {
                                tab_id: tab_id.as_u64(),
                                document_generation: pending.document_generation,
                                ordinal: pending.ordinal,
                                kind: pending.kind,
                            }),
                            Err(category) => {
                                self.push_deferred_rejection(tab_id, &pending, category)
                            }
                        }
                    }
                    DebuggerExecutionStatus::Completed => {
                        let _ = self
                            .pending_debugger_executions
                            .get_mut(&tab_id)
                            .and_then(VecDeque::pop_front);
                        self.push_deferred_rejection(
                            tab_id,
                            &next,
                            "native debugger execution was already completed",
                        );
                    }
                }
            }
            if self
                .pending_debugger_executions
                .get(&tab_id)
                .is_some_and(VecDeque::is_empty)
            {
                self.pending_debugger_executions.remove(&tab_id);
            }
        }
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

    fn insert_debugger_breakpoint(
        &mut self,
        tab_id: TabId,
        breakpoint: DebuggerBreakpointRecord,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        let max_breakpoints = self.config.max_debugger_breakpoints_per_realm;
        let breakpoints = self.debugger_breakpoints.entry(tab_id).or_default();
        if !breakpoints.contains(&breakpoint) && breakpoints.len() == max_breakpoints {
            return Err(JavaScriptPageDebuggerError::BreakpointLimit);
        }
        breakpoints.insert(breakpoint);
        Ok(())
    }

    fn debugger_program_record_for_bluejs(
        &self,
        tab_id: TabId,
        bluejs_handle: BlueJsProgramHandle,
    ) -> Option<DebuggerProgramRecord> {
        self.debugger_programs
            .get(&tab_id)
            .into_iter()
            .flatten()
            .find(|record| record.bluejs_handle == bluejs_handle)
            .copied()
    }

    fn debugger_entry_breakpoint(
        &self,
        tab_id: TabId,
        program: DebuggerProgramKey,
    ) -> Result<DebuggerBreakpointRecord, JavaScriptPageDebuggerError> {
        let record = self
            .debugger_programs
            .get(&tab_id)
            .into_iter()
            .flatten()
            .find(|record| {
                record.program_handle == program.program_handle
                    && record.program_generation == program.program_generation
            })
            .ok_or(JavaScriptPageDebuggerError::UnknownProgram)?;
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
                safe_point.code_unit.ordinal() == 0 && safe_point.bytecode_offset == 0
            })
            .ok_or(JavaScriptPageDebuggerError::NotExecutableEntry)?;
        self.runtime
            .validate_safe_point(tab_id.as_u64(), record.bluejs_handle, safe_point)
            .map_err(debugger_page_runtime_error)?;
        Ok(DebuggerBreakpointRecord {
            program_handle: program.program_handle,
            program_generation: program.program_generation,
            code_unit_ordinal: safe_point.code_unit.ordinal(),
            bytecode_offset: safe_point.bytecode_offset,
        })
    }

    fn push_deferred_rejection(
        &mut self,
        tab_id: TabId,
        pending: &PendingDebuggerExecution,
        category: &'static str,
    ) {
        self.push_report(JavaScriptPageExecutionReport::Rejected {
            tab_id: tab_id.as_u64(),
            document_generation: pending.document_generation,
            ordinal: pending.ordinal,
            kind: pending.kind,
            category,
        });
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

fn public_execution_state(status: DebuggerExecutionStatus) -> JavaScriptPageDebuggerExecutionState {
    match status {
        DebuggerExecutionStatus::Pending => JavaScriptPageDebuggerExecutionState::Pending,
        DebuggerExecutionStatus::Paused(breakpoint) => {
            JavaScriptPageDebuggerExecutionState::Paused {
                code_unit_ordinal: breakpoint.code_unit_ordinal,
                bytecode_offset: breakpoint.bytecode_offset,
            }
        }
        DebuggerExecutionStatus::ResumeRequested => JavaScriptPageDebuggerExecutionState::Resuming,
        DebuggerExecutionStatus::Completed => JavaScriptPageDebuggerExecutionState::Completed,
    }
}

fn debugger_page_runtime_error(error: BlueJsPageRuntimeError) -> JavaScriptPageDebuggerError {
    match error {
        BlueJsPageRuntimeError::SafePointLimit { .. } => JavaScriptPageDebuggerError::ResourceLimit,
        _ => JavaScriptPageDebuggerError::StaleProgram,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded_tabs(html: &str, url: &str) -> (TabManager, TabId) {
        let mut tabs = TabManager::new(320.0, 200.0);
        let tab_id = tabs.default_tab();
        tabs.get_mut(tab_id)
            .unwrap()
            .load_html_str(html, Some(url.to_string()));
        (tabs, tab_id)
    }

    #[test]
    fn breakpoint_configuration_is_exact_bounded_idempotent_and_realm_scoped() {
        let (mut tabs, tab_id) = loaded_tabs(
            "<script>const first = 1; const second = 2;</script>",
            "https://example.test/breakpoint-configuration.html",
        );
        let mut executor = JavaScriptPageExecutor::with_config(JavaScriptPageExecutorConfig {
            max_debugger_breakpoints_per_realm: 1,
            ..JavaScriptPageExecutorConfig::default()
        })
        .unwrap();

        executor.synchronize_and_execute(&tabs).unwrap();
        let program = executor.debugger_programs(tab_id, 1).unwrap()[0];
        let safe_points = executor
            .debugger_safe_points(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap();
        let first = *safe_points
            .first()
            .expect("the compiled declaration has a first safe point");
        let second = *safe_points
            .iter()
            .find(|safe_point| *safe_point != &first)
            .expect("the two declarations provide distinct safe points");

        executor
            .set_debugger_breakpoint(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
                first.code_unit_ordinal,
                first.bytecode_offset,
            )
            .unwrap();
        // A socket retry cannot consume an additional bounded record.
        executor
            .set_debugger_breakpoint(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
                first.code_unit_ordinal,
                first.bytecode_offset,
            )
            .unwrap();
        assert_eq!(
            executor.debugger_breakpoints(tab_id, 1).unwrap(),
            vec![JavaScriptPageDebuggerBreakpoint {
                program_handle: program.program_handle,
                program_generation: program.program_generation,
                code_unit_ordinal: first.code_unit_ordinal,
                bytecode_offset: first.bytecode_offset,
            }]
        );
        assert_eq!(
            executor.set_debugger_breakpoint(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
                second.code_unit_ordinal,
                second.bytecode_offset,
            ),
            Err(JavaScriptPageDebuggerError::BreakpointLimit)
        );
        assert_eq!(
            executor.clear_debugger_breakpoint(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
                first.code_unit_ordinal,
                u32::MAX,
            ),
            Err(JavaScriptPageDebuggerError::InvalidSafePoint)
        );
        assert!(executor
            .clear_debugger_breakpoint(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
                first.code_unit_ordinal,
                first.bytecode_offset,
            )
            .unwrap());
        assert!(!executor
            .clear_debugger_breakpoint(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
                first.code_unit_ordinal,
                first.bytecode_offset,
            )
            .unwrap());
        executor
            .set_debugger_breakpoint(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
                second.code_unit_ordinal,
                second.bytecode_offset,
            )
            .unwrap();

        tabs.get_mut(tab_id).unwrap().load_html_str(
            "<script>const successor = 3;</script>",
            Some("https://example.test/breakpoint-successor.html".to_string()),
        );
        executor.synchronize_and_execute(&tabs).unwrap();
        assert_eq!(
            executor.debugger_breakpoints(tab_id, 1),
            Err(JavaScriptPageDebuggerError::NoLiveRealm)
        );
        assert!(executor.debugger_breakpoints(tab_id, 2).unwrap().is_empty());
    }

    #[test]
    fn root_entry_breakpoint_pauses_before_vm_execution_and_resumes_once() {
        let (mut tabs, tab_id) = loaded_tabs(
            "<script>throw 1;</script>",
            "https://example.test/debugger-entry-pause.html",
        );
        let mut executor = JavaScriptPageExecutor::with_config(JavaScriptPageExecutorConfig {
            native_debugger_execution_control: true,
            ..JavaScriptPageExecutorConfig::default()
        })
        .unwrap();

        // Admission has compiled and registered the program, but execution is
        // deliberately deferred for one owner-session turn.
        executor.synchronize_and_execute(&tabs).unwrap();
        let program = executor.debugger_programs(tab_id, 1).unwrap()[0];
        assert_eq!(
            executor
                .debugger_execution_state(
                    tab_id,
                    1,
                    program.program_handle,
                    program.program_generation
                )
                .unwrap(),
            JavaScriptPageDebuggerExecutionState::Pending
        );
        assert!(executor.drain_reports_for_tab(tab_id).is_empty());
        let entry = executor
            .debugger_safe_points(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap()
            .into_iter()
            .find(|safe_point| safe_point.code_unit_ordinal == 0 && safe_point.bytecode_offset == 0)
            .expect("every admitted BlueJS root has a verified instruction-zero boundary");
        executor
            .arm_debugger_entry_breakpoint(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
                entry.code_unit_ordinal,
                entry.bytecode_offset,
            )
            .unwrap();
        // v4 root-entry arming retains its idempotent configuration behavior;
        // only the v5 non-entry continuation arm is one-shot.
        executor
            .arm_debugger_entry_breakpoint(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
                entry.code_unit_ordinal,
                entry.bytecode_offset,
            )
            .unwrap();

        // The second lifecycle turn observes the armed exact boundary before
        // passing anything to `Vm::execute_script`; the throwing bytecode has
        // not run, so there is neither execution report nor runtime error.
        executor.synchronize_and_execute(&tabs).unwrap();
        assert_eq!(
            executor
                .debugger_execution_state(
                    tab_id,
                    1,
                    program.program_handle,
                    program.program_generation
                )
                .unwrap(),
            JavaScriptPageDebuggerExecutionState::Paused {
                code_unit_ordinal: entry.code_unit_ordinal,
                bytecode_offset: entry.bytecode_offset,
            }
        );
        assert!(executor.drain_reports_for_tab(tab_id).is_empty());

        executor
            .resume_debugger_execution(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap();
        assert_eq!(
            executor
                .debugger_execution_state(
                    tab_id,
                    1,
                    program.program_handle,
                    program.program_generation
                )
                .unwrap(),
            JavaScriptPageDebuggerExecutionState::Resuming
        );
        executor.synchronize_and_execute(&tabs).unwrap();
        assert_eq!(
            executor
                .debugger_execution_state(
                    tab_id,
                    1,
                    program.program_handle,
                    program.program_generation
                )
                .unwrap(),
            JavaScriptPageDebuggerExecutionState::Completed
        );
        assert_eq!(
            executor.drain_reports_for_tab(tab_id),
            vec![JavaScriptPageExecutionReport::Rejected {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 0,
                kind: BlueJsPageScriptKind::Classic,
                category: "BlueJS page execution failed",
            }]
        );

        // Replacement clears both paused/completed scheduler records and
        // opaque program identities before admitting a successor realm.
        tabs.get_mut(tab_id).unwrap().load_html_str(
            "<script>const successor = 2;</script>",
            Some("https://example.test/debugger-entry-successor.html".to_string()),
        );
        executor.synchronize_and_execute(&tabs).unwrap();
        assert_eq!(
            executor.debugger_execution_state(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            ),
            Err(JavaScriptPageDebuggerError::NoLiveRealm)
        );
    }

    #[test]
    fn root_safe_point_breakpoint_pauses_after_real_execution_and_resumes_same_frame() {
        let (tabs, tab_id) = loaded_tabs(
            "<script>globalThis.before = 1; throw 2;</script>",
            "https://example.test/debugger-root-safe-point.html",
        );
        let mut executor = JavaScriptPageExecutor::with_config(JavaScriptPageExecutorConfig {
            native_debugger_execution_control: true,
            ..JavaScriptPageExecutorConfig::default()
        })
        .unwrap();

        executor.synchronize_and_execute(&tabs).unwrap();
        let program = executor.debugger_programs(tab_id, 1).unwrap()[0];
        let safe_point = executor
            .debugger_safe_points(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap()
            .into_iter()
            .find(|safe_point| safe_point.code_unit_ordinal == 0 && safe_point.bytecode_offset != 0)
            .expect("fixture has a non-entry root instruction boundary");
        executor
            .arm_debugger_root_safe_point_breakpoint(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
                safe_point.code_unit_ordinal,
                safe_point.bytecode_offset,
            )
            .unwrap();
        // The continuation target is committed while the declaration is
        // still pending. A second request cannot retarget it before the
        // scheduler reaches the first boundary.
        assert_eq!(
            executor.arm_debugger_root_safe_point_breakpoint(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
                safe_point.code_unit_ordinal,
                safe_point.bytecode_offset,
            ),
            Err(JavaScriptPageDebuggerError::InvalidExecutionState)
        );

        executor.synchronize_and_execute(&tabs).unwrap();
        assert_eq!(
            executor
                .debugger_execution_state(
                    tab_id,
                    1,
                    program.program_handle,
                    program.program_generation,
                )
                .unwrap(),
            JavaScriptPageDebuggerExecutionState::Paused {
                code_unit_ordinal: safe_point.code_unit_ordinal,
                bytecode_offset: safe_point.bytecode_offset,
            }
        );
        assert!(executor.drain_reports_for_tab(tab_id).is_empty());

        executor
            .resume_debugger_execution(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap();
        executor.synchronize_and_execute(&tabs).unwrap();
        assert_eq!(
            executor
                .debugger_execution_state(
                    tab_id,
                    1,
                    program.program_handle,
                    program.program_generation,
                )
                .unwrap(),
            JavaScriptPageDebuggerExecutionState::Completed
        );
        assert_eq!(
            executor.drain_reports_for_tab(tab_id),
            vec![JavaScriptPageExecutionReport::Rejected {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 0,
                kind: BlueJsPageScriptKind::Classic,
                category: "BlueJS page execution failed",
            }]
        );
    }
}
