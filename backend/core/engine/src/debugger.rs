// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Core-owned routing for the versioned native debugger discovery channel.
//!
//! The listener side may live on a worker thread, but this module resolves a
//! debugger target only on the core session thread against the live
//! [`crate::TabManager`]. The first shipped operation resolves opaque programs
//! and verifies compiler-recorded instruction boundaries, without fabricating
//! a breakpoint, paused frame, runtime value, source disclosure, or VM handle.

use crate::{
    script::javascript::{
        JavaScriptPageDebuggerError, JavaScriptPageDebuggerExecutionState, JavaScriptPageExecutor,
    },
    TabId, TabManager,
};
use blueice_ipc::debugger::{
    DebuggerCapabilities, DebuggerCapability, DebuggerCapabilityReport, DebuggerCapabilityState,
    DebuggerErrorCode, DebuggerExecutionState, DebuggerPageRealm, DebuggerProgram, DebuggerReply,
    DebuggerRequest, DebuggerSafePoint, DEBUGGER_PROTOCOL_VERSION,
};
use std::io;
use std::sync::mpsc;

/// The only browser-context identity the reference core currently owns.
pub const DEFAULT_BROWSER_CONTEXT_ID: u64 = 1;

/// A bounded debugger batch prevents a busy discovery peer from starving the
/// frontend or navigation-completion processing in the owning session loop.
const MAX_DEBUGGER_REQUESTS_PER_SESSION_TICK: usize = 64;

/// The control reply remains bounded even if a future frontend opens many
/// tabs. A debugger client must not turn target discovery into an unbounded
/// process-state enumeration endpoint.
const MAX_DISCOVERABLE_PAGE_REALMS: usize = 128;

/// Fixed discovery bounds for features that are not installed yet. They are
/// part of the advertised future contract, not permission to inspect a stack
/// or runtime value today.
const MAX_STACK_FRAMES: u32 = 64;
const MAX_SCOPE_BINDINGS: u32 = 256;
const MAX_VALUE_PREVIEW_BYTES: u32 = 4_096;

/// A bounded native location reply prevents an instrumented program from
/// turning debugger discovery into an unbounded bytecode inventory channel.
const DEFAULT_MAX_SAFE_POINTS_PER_PROGRAM: usize = 4_096;

/// A debugger peer cannot turn a realm into an unbounded persistent
/// breakpoint-record store. This fallback applies only when no JavaScript
/// executor is present to report its core-selected limit.
const DEFAULT_MAX_BREAKPOINTS_PER_REALM: usize = 256;

/// Sender owned by a debugger-socket worker. It forwards one decoded request
/// to the session thread and waits for that thread's target-checked reply.
#[derive(Clone)]
pub struct DebuggerRequestSender(mpsc::Sender<DebuggerRequestEnvelope>);

/// Receiver owned exclusively by the core session thread.
pub struct DebuggerRequestReceiver(mpsc::Receiver<DebuggerRequestEnvelope>);

struct DebuggerRequestEnvelope {
    request: DebuggerRequest,
    reply: mpsc::SyncSender<DebuggerReply>,
}

/// Creates the worker-to-session hand-off for debugger requests. The worker
/// never borrows a tab, page, realm, VM, or BlueJS object.
pub fn debugger_request_channel() -> (DebuggerRequestSender, DebuggerRequestReceiver) {
    let (sender, receiver) = mpsc::channel();
    (
        DebuggerRequestSender(sender),
        DebuggerRequestReceiver(receiver),
    )
}

impl DebuggerRequestSender {
    /// Routes one request to the owning session. A stopped session is a
    /// transport failure rather than a synthetic debugger reply that a caller
    /// could mistake for a live target result.
    pub fn request(&self, request: DebuggerRequest) -> io::Result<DebuggerReply> {
        let (reply_sender, reply_receiver) = mpsc::sync_channel(1);
        self.0
            .send(DebuggerRequestEnvelope {
                request,
                reply: reply_sender,
            })
            .map_err(|_| {
                io::Error::new(io::ErrorKind::BrokenPipe, "core debugger session ended")
            })?;
        reply_receiver.recv().map_err(|_| {
            io::Error::new(io::ErrorKind::BrokenPipe, "core debugger reply unavailable")
        })
    }
}

impl DebuggerRequestReceiver {
    /// Resolves a bounded number of worker requests against the current core
    /// state. Replies are best effort: a disconnected debugger client cannot
    /// interrupt rendering or a frontend session.
    pub fn dispatch_pending(
        &self,
        tabs: &TabManager,
        mut javascript_executor: Option<&mut JavaScriptPageExecutor>,
    ) -> usize {
        let mut dispatched = 0;
        let mut preserve_pending_entry = false;
        let mut advance_pending_entry = false;
        while dispatched < MAX_DEBUGGER_REQUESTS_PER_SESSION_TICK {
            let Ok(envelope) = self.0.try_recv() else {
                break;
            };
            let executes_or_releases_entry = matches!(
                &envelope.request,
                DebuggerRequest::ArmEntryBreakpoint { .. }
                    | DebuggerRequest::ArmRootSafePointBreakpoint { .. }
                    | DebuggerRequest::ResumeExecution { .. }
            );
            let reply = handle_debugger_request_with_javascript_executor(
                tabs,
                javascript_executor.as_deref_mut(),
                envelope.request,
            );
            let _ = envelope.reply.send(reply);
            if executes_or_releases_entry {
                advance_pending_entry = true;
            } else {
                preserve_pending_entry = true;
            }
            dispatched += 1;
        }
        // A page declaration is admitted before a remote peer can learn its
        // opaque program identity. Give each discovery/configuration reply one
        // more session boundary to send the next bounded request; otherwise a
        // normal idle synchronization begins it. Arm and resume deliberately
        // consume that boundary so their requested state transition happens.
        if preserve_pending_entry && !advance_pending_entry {
            if let Some(executor) = javascript_executor {
                executor.hold_pending_debugger_execution_once();
            }
        }
        dispatched
    }
}

/// Handles one post-handshake debugger request on the core session thread.
/// A `Hello` here is rejected because the socket listener owns first-message
/// negotiation before it creates a request envelope.
pub fn handle_debugger_request(tabs: &TabManager, request: DebuggerRequest) -> DebuggerReply {
    handle_debugger_request_with_javascript_executor(tabs, None, request)
}

/// Resolves one post-handshake request against the session's exact live page
/// state and, when explicitly enabled, the core-owned JavaScript page runtime.
/// The optional executor is borrowed only on the session thread; a socket
/// worker never sees a VM, source, bytecode, or page object.
pub fn handle_debugger_request_with_javascript_executor(
    tabs: &TabManager,
    javascript_executor: Option<&mut JavaScriptPageExecutor>,
    request: DebuggerRequest,
) -> DebuggerReply {
    match request {
        DebuggerRequest::ListPageRealms => list_page_realms(tabs),
        DebuggerRequest::DescribeCapabilities { realm } => {
            describe_capabilities(tabs, javascript_executor.as_deref(), realm)
        }
        DebuggerRequest::ListPrograms { realm } => {
            list_programs(tabs, javascript_executor.as_deref(), realm)
        }
        DebuggerRequest::ListSafePoints { program } => {
            list_safe_points(tabs, javascript_executor.as_deref(), program)
        }
        DebuggerRequest::ValidateSafePoint { safe_point } => {
            validate_safe_point(tabs, javascript_executor.as_deref(), safe_point)
        }
        DebuggerRequest::SetBreakpoint { safe_point } => {
            set_breakpoint(tabs, javascript_executor, safe_point)
        }
        DebuggerRequest::ArmEntryBreakpoint { safe_point } => {
            arm_entry_breakpoint(tabs, javascript_executor, safe_point)
        }
        DebuggerRequest::ArmRootSafePointBreakpoint { safe_point } => {
            arm_root_safe_point_breakpoint(tabs, javascript_executor, safe_point)
        }
        DebuggerRequest::ListBreakpoints { realm } => {
            list_breakpoints(tabs, javascript_executor.as_deref(), realm)
        }
        DebuggerRequest::ClearBreakpoint { safe_point } => {
            clear_breakpoint(tabs, javascript_executor, safe_point)
        }
        DebuggerRequest::GetExecutionState { program } => {
            execution_state(tabs, javascript_executor.as_deref(), program)
        }
        DebuggerRequest::ResumeExecution { program } => {
            resume_execution(tabs, javascript_executor, program)
        }
        DebuggerRequest::Hello { .. } => DebuggerReply::Error {
            code: DebuggerErrorCode::ProtocolVersion,
            message: "debugger Hello is valid only as the first request".to_string(),
        },
        DebuggerRequest::Unknown => DebuggerReply::Unsupported {
            operation: "unknown debugger request".to_string(),
            reason: "this core build does not recognize the requested debugger operation"
                .to_string(),
        },
    }
}

fn list_page_realms(tabs: &TabManager) -> DebuggerReply {
    let realms: Vec<DebuggerPageRealm> = tabs
        .ids()
        .filter_map(|tab_id| {
            let page = tabs.get(tab_id)?;
            let realm_generation = page.document_generation();
            (realm_generation != 0 && page.url().is_some()).then_some(DebuggerPageRealm {
                browser_context_id: DEFAULT_BROWSER_CONTEXT_ID,
                tab_id: tab_id.as_u64(),
                realm_generation,
            })
        })
        .take(MAX_DISCOVERABLE_PAGE_REALMS + 1)
        .collect();
    if realms.len() > MAX_DISCOVERABLE_PAGE_REALMS {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::ResourceLimit,
            message: "too many live debugger page realms".to_string(),
        };
    }
    DebuggerReply::PageRealms(realms)
}

fn describe_capabilities(
    tabs: &TabManager,
    javascript_executor: Option<&JavaScriptPageExecutor>,
    realm: DebuggerPageRealm,
) -> DebuggerReply {
    if let Err(reply) = resolve_live_realm(tabs, realm) {
        return reply;
    }
    let program_locations_available = javascript_executor.is_some_and(|executor| {
        executor.debugger_has_live_realm(TabId::from_u64(realm.tab_id), realm.realm_generation)
    });
    let entry_execution_control_available = javascript_executor.is_some_and(|executor| {
        executor.debugger_execution_control_available()
            && executor
                .debugger_has_live_realm(TabId::from_u64(realm.tab_id), realm.realm_generation)
    });
    let max_safe_points_per_program = javascript_executor
        .map(JavaScriptPageExecutor::max_debugger_safe_points_per_program)
        .unwrap_or(DEFAULT_MAX_SAFE_POINTS_PER_PROGRAM);
    let max_breakpoints_per_realm = javascript_executor
        .map(JavaScriptPageExecutor::max_debugger_breakpoints_per_realm)
        .unwrap_or(DEFAULT_MAX_BREAKPOINTS_PER_REALM);

    DebuggerReply::Capabilities(DebuggerCapabilities {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        realm,
        reports: capability_reports(
            program_locations_available,
            entry_execution_control_available,
        ),
        max_stack_frames: MAX_STACK_FRAMES,
        max_scope_bindings: MAX_SCOPE_BINDINGS,
        max_value_preview_bytes: MAX_VALUE_PREVIEW_BYTES,
        max_safe_points_per_program: u32::try_from(max_safe_points_per_program)
            .expect("native debugger safe-point reply cap fits the wire type"),
        max_breakpoints_per_realm: u32::try_from(max_breakpoints_per_realm)
            .expect("native debugger breakpoint reply cap fits the wire type"),
    })
}

fn resolve_live_realm(tabs: &TabManager, realm: DebuggerPageRealm) -> Result<TabId, DebuggerReply> {
    if !realm.is_well_formed() || realm.browser_context_id != DEFAULT_BROWSER_CONTEXT_ID {
        return Err(DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger realm target".to_string(),
        });
    }
    let tab_id = TabId::from_u64(realm.tab_id);
    let Some(page) = tabs.get(tab_id) else {
        return Err(DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "unknown debugger tab".to_string(),
        });
    };
    if page.document_generation() != realm.realm_generation {
        return Err(DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm,
            message: "stale debugger realm generation".to_string(),
        });
    }
    Ok(tab_id)
}

fn list_programs(
    tabs: &TabManager,
    javascript_executor: Option<&JavaScriptPageExecutor>,
    realm: DebuggerPageRealm,
) -> DebuggerReply {
    let tab_id = match resolve_live_realm(tabs, realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return reply,
    };
    let Some(executor) = javascript_executor else {
        return unavailable_program_locations();
    };
    if !executor.debugger_has_live_realm(tab_id, realm.realm_generation) {
        return unavailable_program_locations();
    }
    match executor.debugger_programs(tab_id, realm.realm_generation) {
        Ok(programs) => DebuggerReply::Programs(
            programs
                .into_iter()
                .map(|program| DebuggerProgram {
                    realm,
                    program_handle: program.program_handle,
                    program_generation: program.program_generation,
                })
                .collect(),
        ),
        Err(error) => debugger_program_error(error),
    }
}

fn list_safe_points(
    tabs: &TabManager,
    javascript_executor: Option<&JavaScriptPageExecutor>,
    program: DebuggerProgram,
) -> DebuggerReply {
    if !program.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger program target".to_string(),
        };
    }
    let tab_id = match resolve_live_realm(tabs, program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return reply,
    };
    let Some(executor) = javascript_executor else {
        return unavailable_program_locations();
    };
    if !executor.debugger_has_live_realm(tab_id, program.realm.realm_generation) {
        return unavailable_program_locations();
    }
    match executor.debugger_safe_points(
        tab_id,
        program.realm.realm_generation,
        program.program_handle,
        program.program_generation,
    ) {
        Ok(safe_points) => DebuggerReply::SafePoints(
            safe_points
                .into_iter()
                .map(|safe_point| DebuggerSafePoint {
                    program,
                    code_unit_ordinal: safe_point.code_unit_ordinal,
                    bytecode_offset: safe_point.bytecode_offset,
                })
                .collect(),
        ),
        Err(error) => debugger_program_error(error),
    }
}

fn validate_safe_point(
    tabs: &TabManager,
    javascript_executor: Option<&JavaScriptPageExecutor>,
    safe_point: DebuggerSafePoint,
) -> DebuggerReply {
    if !safe_point.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger safe-point target".to_string(),
        };
    }
    let tab_id = match resolve_live_realm(tabs, safe_point.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return reply,
    };
    let Some(executor) = javascript_executor else {
        return unavailable_program_locations();
    };
    if !executor.debugger_has_live_realm(tab_id, safe_point.program.realm.realm_generation) {
        return unavailable_program_locations();
    }
    match executor.validate_debugger_safe_point(
        tab_id,
        safe_point.program.realm.realm_generation,
        safe_point.program.program_handle,
        safe_point.program.program_generation,
        safe_point.code_unit_ordinal,
        safe_point.bytecode_offset,
    ) {
        Ok(()) => DebuggerReply::SafePointValidated { safe_point },
        Err(error) => debugger_program_error(error),
    }
}

fn set_breakpoint(
    tabs: &TabManager,
    javascript_executor: Option<&mut JavaScriptPageExecutor>,
    safe_point: DebuggerSafePoint,
) -> DebuggerReply {
    if !safe_point.is_well_formed() {
        return invalid_safe_point_target();
    }
    let tab_id = match resolve_live_realm(tabs, safe_point.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return reply,
    };
    let Some(executor) = javascript_executor else {
        return unavailable_breakpoint_configuration();
    };
    if !executor.debugger_has_live_realm(tab_id, safe_point.program.realm.realm_generation) {
        return unavailable_breakpoint_configuration();
    }
    match executor.set_debugger_breakpoint(
        tab_id,
        safe_point.program.realm.realm_generation,
        safe_point.program.program_handle,
        safe_point.program.program_generation,
        safe_point.code_unit_ordinal,
        safe_point.bytecode_offset,
    ) {
        Ok(()) => DebuggerReply::BreakpointSet { safe_point },
        Err(error) => debugger_program_error(error),
    }
}

fn arm_entry_breakpoint(
    tabs: &TabManager,
    javascript_executor: Option<&mut JavaScriptPageExecutor>,
    safe_point: DebuggerSafePoint,
) -> DebuggerReply {
    if !safe_point.is_well_formed() {
        return invalid_safe_point_target();
    }
    let tab_id = match resolve_live_realm(tabs, safe_point.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return reply,
    };
    let Some(executor) = javascript_executor else {
        return unavailable_execution_control();
    };
    if !executor.debugger_has_live_realm(tab_id, safe_point.program.realm.realm_generation) {
        return unavailable_execution_control();
    }
    match executor.arm_debugger_entry_breakpoint(
        tab_id,
        safe_point.program.realm.realm_generation,
        safe_point.program.program_handle,
        safe_point.program.program_generation,
        safe_point.code_unit_ordinal,
        safe_point.bytecode_offset,
    ) {
        Ok(()) => DebuggerReply::BreakpointArmed { safe_point },
        Err(error) => debugger_program_error(error),
    }
}

/// Arms the bounded continuation seam at an exact root-code-unit boundary.
/// This deliberately has its own request/reply instead of making ordinary
/// breakpoint configuration appear to interrupt a synchronous VM.
fn arm_root_safe_point_breakpoint(
    tabs: &TabManager,
    javascript_executor: Option<&mut JavaScriptPageExecutor>,
    safe_point: DebuggerSafePoint,
) -> DebuggerReply {
    if !safe_point.is_well_formed() {
        return invalid_safe_point_target();
    }
    let tab_id = match resolve_live_realm(tabs, safe_point.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return reply,
    };
    let Some(executor) = javascript_executor else {
        return unavailable_execution_control();
    };
    if !executor.debugger_has_live_realm(tab_id, safe_point.program.realm.realm_generation) {
        return unavailable_execution_control();
    }
    match executor.arm_debugger_root_safe_point_breakpoint(
        tab_id,
        safe_point.program.realm.realm_generation,
        safe_point.program.program_handle,
        safe_point.program.program_generation,
        safe_point.code_unit_ordinal,
        safe_point.bytecode_offset,
    ) {
        Ok(()) => DebuggerReply::RootSafePointBreakpointArmed { safe_point },
        Err(error) => debugger_program_error(error),
    }
}

fn list_breakpoints(
    tabs: &TabManager,
    javascript_executor: Option<&JavaScriptPageExecutor>,
    realm: DebuggerPageRealm,
) -> DebuggerReply {
    let tab_id = match resolve_live_realm(tabs, realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return reply,
    };
    let Some(executor) = javascript_executor else {
        return unavailable_breakpoint_configuration();
    };
    if !executor.debugger_has_live_realm(tab_id, realm.realm_generation) {
        return unavailable_breakpoint_configuration();
    }
    match executor.debugger_breakpoints(tab_id, realm.realm_generation) {
        Ok(breakpoints) => DebuggerReply::Breakpoints(
            breakpoints
                .into_iter()
                .map(|breakpoint| DebuggerSafePoint {
                    program: DebuggerProgram {
                        realm,
                        program_handle: breakpoint.program_handle,
                        program_generation: breakpoint.program_generation,
                    },
                    code_unit_ordinal: breakpoint.code_unit_ordinal,
                    bytecode_offset: breakpoint.bytecode_offset,
                })
                .collect(),
        ),
        Err(error) => debugger_program_error(error),
    }
}

fn clear_breakpoint(
    tabs: &TabManager,
    javascript_executor: Option<&mut JavaScriptPageExecutor>,
    safe_point: DebuggerSafePoint,
) -> DebuggerReply {
    if !safe_point.is_well_formed() {
        return invalid_safe_point_target();
    }
    let tab_id = match resolve_live_realm(tabs, safe_point.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return reply,
    };
    let Some(executor) = javascript_executor else {
        return unavailable_breakpoint_configuration();
    };
    if !executor.debugger_has_live_realm(tab_id, safe_point.program.realm.realm_generation) {
        return unavailable_breakpoint_configuration();
    }
    match executor.clear_debugger_breakpoint(
        tab_id,
        safe_point.program.realm.realm_generation,
        safe_point.program.program_handle,
        safe_point.program.program_generation,
        safe_point.code_unit_ordinal,
        safe_point.bytecode_offset,
    ) {
        Ok(was_present) => DebuggerReply::BreakpointCleared {
            safe_point,
            was_present,
        },
        Err(error) => debugger_program_error(error),
    }
}

fn execution_state(
    tabs: &TabManager,
    javascript_executor: Option<&JavaScriptPageExecutor>,
    program: DebuggerProgram,
) -> DebuggerReply {
    if !program.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger program target".to_string(),
        };
    }
    let tab_id = match resolve_live_realm(tabs, program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return reply,
    };
    let Some(executor) = javascript_executor else {
        return unavailable_execution_control();
    };
    if !executor.debugger_has_live_realm(tab_id, program.realm.realm_generation) {
        return unavailable_execution_control();
    }
    match executor.debugger_execution_state(
        tab_id,
        program.realm.realm_generation,
        program.program_handle,
        program.program_generation,
    ) {
        Ok(state) => DebuggerReply::ExecutionState {
            program,
            state: debugger_execution_state(state, program),
        },
        Err(error) => debugger_program_error(error),
    }
}

fn resume_execution(
    tabs: &TabManager,
    javascript_executor: Option<&mut JavaScriptPageExecutor>,
    program: DebuggerProgram,
) -> DebuggerReply {
    if !program.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger program target".to_string(),
        };
    }
    let tab_id = match resolve_live_realm(tabs, program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return reply,
    };
    let Some(executor) = javascript_executor else {
        return unavailable_execution_control();
    };
    if !executor.debugger_has_live_realm(tab_id, program.realm.realm_generation) {
        return unavailable_execution_control();
    }
    match executor.resume_debugger_execution(
        tab_id,
        program.realm.realm_generation,
        program.program_handle,
        program.program_generation,
    ) {
        Ok(()) => DebuggerReply::ExecutionResumed { program },
        Err(error) => debugger_program_error(error),
    }
}

fn invalid_safe_point_target() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::InvalidTarget,
        message: "invalid debugger safe-point target".to_string(),
    }
}

fn unavailable_program_locations() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "native debugger program locations require an enabled live JavaScript page realm"
            .to_string(),
    }
}

fn unavailable_breakpoint_configuration() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "native breakpoint configuration requires an enabled live JavaScript page realm"
            .to_string(),
    }
}

fn unavailable_execution_control() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "native debugger entry pause/resume requires an explicitly enabled JavaScript page realm"
            .to_string(),
    }
}

fn debugger_program_error(error: JavaScriptPageDebuggerError) -> DebuggerReply {
    let (code, message) = match error {
        JavaScriptPageDebuggerError::NoLiveRealm => (
            DebuggerErrorCode::CapabilityUnavailable,
            "native debugger program locations require an enabled live JavaScript page realm",
        ),
        JavaScriptPageDebuggerError::UnknownProgram => {
            (DebuggerErrorCode::InvalidTarget, "unknown debugger program")
        }
        JavaScriptPageDebuggerError::StaleProgram => (
            DebuggerErrorCode::StaleProgram,
            "stale debugger program generation",
        ),
        JavaScriptPageDebuggerError::InvalidSafePoint => (
            DebuggerErrorCode::InvalidSafePoint,
            "invalid debugger instruction boundary",
        ),
        JavaScriptPageDebuggerError::ResourceLimit => (
            DebuggerErrorCode::ResourceLimit,
            "too many verified debugger safe points for one program",
        ),
        JavaScriptPageDebuggerError::BreakpointLimit => (
            DebuggerErrorCode::ResourceLimit,
            "too many native breakpoint records for one page realm",
        ),
        JavaScriptPageDebuggerError::ExecutionControlUnavailable => (
            DebuggerErrorCode::CapabilityUnavailable,
            "native debugger entry pause/resume is not enabled for this page realm",
        ),
        JavaScriptPageDebuggerError::NotExecutableEntry => (
            DebuggerErrorCode::InvalidSafePoint,
            "debugger entry pause accepts only a pending root instruction boundary",
        ),
        JavaScriptPageDebuggerError::NotResumableRootSafePoint => (
            DebuggerErrorCode::InvalidSafePoint,
            "debugger root continuation accepts only a pending classic-script root code-unit boundary",
        ),
        JavaScriptPageDebuggerError::InvalidExecutionState => (
            DebuggerErrorCode::InvalidExecutionState,
            "debugger operation is not valid for the program execution state",
        ),
    };
    DebuggerReply::Error {
        code,
        message: message.to_string(),
    }
}

fn debugger_execution_state(
    state: JavaScriptPageDebuggerExecutionState,
    program: DebuggerProgram,
) -> DebuggerExecutionState {
    match state {
        JavaScriptPageDebuggerExecutionState::Pending => DebuggerExecutionState::Pending,
        JavaScriptPageDebuggerExecutionState::Paused {
            code_unit_ordinal,
            bytecode_offset,
        } => DebuggerExecutionState::Paused {
            safe_point: DebuggerSafePoint {
                program,
                code_unit_ordinal,
                bytecode_offset,
            },
        },
        JavaScriptPageDebuggerExecutionState::Resuming => DebuggerExecutionState::Resuming,
        JavaScriptPageDebuggerExecutionState::Completed => DebuggerExecutionState::Completed,
    }
}

fn capability_reports(
    program_locations_available: bool,
    entry_execution_control_available: bool,
) -> Vec<DebuggerCapabilityReport> {
    [
        (
            DebuggerCapability::ProgramLocations,
            if program_locations_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if program_locations_available {
                "opaque program and exact safe-point validation are installed"
            } else {
                "exact program-location validation requires an enabled JavaScript page realm"
            },
        ),
        (
            DebuggerCapability::BreakpointConfiguration,
            if program_locations_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if program_locations_available {
                "bounded exact breakpoint configuration is installed; it does not interrupt execution"
            } else {
                "exact breakpoint configuration requires an enabled JavaScript page realm"
            },
        ),
        (
            DebuggerCapability::Breakpoints,
            if entry_execution_control_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if entry_execution_control_available {
                "compiler-verified root-code-unit breakpoints pause pending classic page scripts"
            } else {
                "native breakpoint interruption is not installed"
            },
        ),
        (
            DebuggerCapability::PauseResume,
            if entry_execution_control_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if entry_execution_control_available {
                "resume preserves one paused classic-script root frame; modules and child code units remain unavailable"
            } else {
                "native pause and resume are not installed"
            },
        ),
        (
            DebuggerCapability::Stepping,
            DebuggerCapabilityState::Planned,
            "native stepping is not installed",
        ),
        (
            DebuggerCapability::Stack,
            DebuggerCapabilityState::Planned,
            "native stack inspection is not installed",
        ),
        (
            DebuggerCapability::Scopes,
            DebuggerCapabilityState::Planned,
            "native scope inspection is not installed",
        ),
        (
            DebuggerCapability::ExceptionPolicy,
            DebuggerCapabilityState::Planned,
            "native exception policy is not installed",
        ),
        (
            DebuggerCapability::BoundedValues,
            DebuggerCapabilityState::Planned,
            "native value inspection is not installed",
        ),
    ]
    .into_iter()
    .map(|(capability, state, detail)| DebuggerCapabilityReport {
        capability,
        state,
        detail: detail.to_string(),
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded_tabs() -> (TabManager, DebuggerPageRealm) {
        let mut tabs = TabManager::new(320.0, 200.0);
        let tab_id = tabs.default_tab();
        tabs.get_mut(tab_id).unwrap().load_html_str(
            "<main>debugger target</main>",
            Some("https://example.test/".to_string()),
        );
        (
            tabs,
            DebuggerPageRealm {
                browser_context_id: DEFAULT_BROWSER_CONTEXT_ID,
                tab_id: tab_id.as_u64(),
                realm_generation: 1,
            },
        )
    }

    #[test]
    fn discovery_requires_the_live_tab_and_exact_document_generation() {
        let (mut tabs, realm) = loaded_tabs();
        assert_eq!(
            handle_debugger_request(&tabs, DebuggerRequest::ListPageRealms),
            DebuggerReply::PageRealms(vec![realm])
        );
        let reply = handle_debugger_request(&tabs, DebuggerRequest::DescribeCapabilities { realm });
        let DebuggerReply::Capabilities(capabilities) = reply else {
            panic!("the live realm must have a discovery reply")
        };
        assert_eq!(capabilities.realm, realm);
        assert_eq!(capabilities.protocol_version, DEBUGGER_PROTOCOL_VERSION);
        assert!(capabilities
            .reports
            .iter()
            .all(|report| report.state == DebuggerCapabilityState::Planned));

        tabs.get_mut(TabId::from_u64(realm.tab_id))
            .unwrap()
            .load_html_str(
                "<main>replacement</main>",
                Some("https://example.test/replacement".to_string()),
            );
        assert!(matches!(
            handle_debugger_request(&tabs, DebuggerRequest::DescribeCapabilities { realm }),
            DebuggerReply::Error {
                code: DebuggerErrorCode::StaleRealm,
                ..
            }
        ));
        assert_eq!(
            handle_debugger_request(&tabs, DebuggerRequest::ListPageRealms),
            DebuggerReply::PageRealms(vec![DebuggerPageRealm {
                realm_generation: 2,
                ..realm
            }])
        );
    }

    #[test]
    fn discovery_rejects_a_malformed_or_unknown_target() {
        let (tabs, realm) = loaded_tabs();
        assert!(matches!(
            handle_debugger_request(
                &tabs,
                DebuggerRequest::DescribeCapabilities {
                    realm: DebuggerPageRealm {
                        realm_generation: 0,
                        ..realm
                    },
                },
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidTarget,
                ..
            }
        ));
        assert!(matches!(
            handle_debugger_request(
                &tabs,
                DebuggerRequest::DescribeCapabilities {
                    realm: DebuggerPageRealm {
                        tab_id: 999,
                        ..realm
                    },
                },
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidTarget,
                ..
            }
        ));
    }

    #[test]
    fn enabled_javascript_realm_exposes_only_exact_opaque_program_locations() {
        let mut tabs = TabManager::new(320.0, 200.0);
        let first_tab = tabs.default_tab();
        tabs.get_mut(first_tab).unwrap().load_html_str(
            "<main>first</main><script>const answer = 40 + 2; answer;</script>",
            Some("https://example.test/first".to_string()),
        );
        let second_tab = tabs.open_tab();
        tabs.get_mut(second_tab).unwrap().load_html_str(
            "<main>second</main><script>const answer = 43;</script>",
            Some("https://example.test/second".to_string()),
        );
        let mut executor = JavaScriptPageExecutor::default();
        executor.synchronize_and_execute(&tabs).unwrap();
        let first_realm = DebuggerPageRealm {
            browser_context_id: DEFAULT_BROWSER_CONTEXT_ID,
            tab_id: first_tab.as_u64(),
            realm_generation: 1,
        };
        let second_realm = DebuggerPageRealm {
            browser_context_id: DEFAULT_BROWSER_CONTEXT_ID,
            tab_id: second_tab.as_u64(),
            realm_generation: 1,
        };

        let DebuggerReply::Capabilities(capabilities) =
            handle_debugger_request_with_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::DescribeCapabilities { realm: first_realm },
            )
        else {
            panic!("an enabled JavaScript realm must describe its live location capability")
        };
        assert!(capabilities.reports.iter().any(|report| {
            report.capability == DebuggerCapability::ProgramLocations
                && report.state == DebuggerCapabilityState::Available
        }));
        assert!(capabilities
            .reports
            .iter()
            .filter(|report| {
                !matches!(
                    report.capability,
                    DebuggerCapability::ProgramLocations
                        | DebuggerCapability::BreakpointConfiguration
                )
            })
            .all(|report| report.state == DebuggerCapabilityState::Planned));
        assert!(capabilities.reports.iter().any(|report| {
            report.capability == DebuggerCapability::BreakpointConfiguration
                && report.state == DebuggerCapabilityState::Available
                && report.detail.contains("does not interrupt execution")
        }));

        let DebuggerReply::Programs(first_programs) =
            handle_debugger_request_with_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::ListPrograms { realm: first_realm },
            )
        else {
            panic!("the current realm must expose opaque program identities")
        };
        assert_eq!(first_programs.len(), 1);
        let first_program = first_programs[0];
        assert_eq!(first_program.realm, first_realm);

        let DebuggerReply::SafePoints(safe_points) =
            handle_debugger_request_with_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::ListSafePoints {
                    program: first_program,
                },
            )
        else {
            panic!("a current program must expose compiler-verified boundaries")
        };
        let safe_point = *safe_points
            .first()
            .expect("a non-empty JavaScript program has an instruction boundary");
        assert_eq!(safe_point.program, first_program);
        assert_eq!(
            handle_debugger_request_with_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::ValidateSafePoint { safe_point },
            ),
            DebuggerReply::SafePointValidated { safe_point }
        );
        assert_eq!(
            handle_debugger_request_with_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::SetBreakpoint { safe_point },
            ),
            DebuggerReply::BreakpointSet { safe_point }
        );
        assert_eq!(
            handle_debugger_request_with_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::ListBreakpoints { realm: first_realm },
            ),
            DebuggerReply::Breakpoints(vec![safe_point])
        );
        assert_eq!(
            handle_debugger_request_with_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::ClearBreakpoint { safe_point },
            ),
            DebuggerReply::BreakpointCleared {
                safe_point,
                was_present: true,
            }
        );
        assert_eq!(
            handle_debugger_request_with_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::ClearBreakpoint { safe_point },
            ),
            DebuggerReply::BreakpointCleared {
                safe_point,
                was_present: false,
            }
        );

        assert!(matches!(
            handle_debugger_request_with_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::ValidateSafePoint {
                    safe_point: DebuggerSafePoint {
                        bytecode_offset: u32::MAX,
                        ..safe_point
                    },
                },
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidSafePoint,
                ..
            }
        ));
        assert!(matches!(
            handle_debugger_request_with_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::SetBreakpoint {
                    safe_point: DebuggerSafePoint {
                        bytecode_offset: u32::MAX,
                        ..safe_point
                    },
                },
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidSafePoint,
                ..
            }
        ));

        assert!(matches!(
            handle_debugger_request_with_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::ListSafePoints {
                    program: DebuggerProgram {
                        realm: second_realm,
                        ..first_program
                    },
                },
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidTarget,
                ..
            }
        ));
        assert!(matches!(
            handle_debugger_request_with_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::SetBreakpoint {
                    safe_point: DebuggerSafePoint {
                        program: DebuggerProgram {
                            realm: second_realm,
                            ..first_program
                        },
                        ..safe_point
                    },
                },
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidTarget,
                ..
            }
        ));

        tabs.get_mut(first_tab).unwrap().load_html_str(
            "<main>replacement</main><script>const successor = 44;</script>",
            Some("https://example.test/replacement".to_string()),
        );
        executor.synchronize_and_execute(&tabs).unwrap();
        assert!(matches!(
            handle_debugger_request_with_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::ListSafePoints {
                    program: first_program,
                },
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::StaleRealm,
                ..
            }
        ));
    }

    #[test]
    fn native_debugger_arms_observes_and_resumes_a_root_entry_pause() {
        let mut tabs = TabManager::new(320.0, 200.0);
        let tab_id = tabs.default_tab();
        tabs.get_mut(tab_id).unwrap().load_html_str(
            "<script>throw 1;</script>",
            Some("https://example.test/native-entry-pause.html".to_string()),
        );
        let realm = DebuggerPageRealm {
            browser_context_id: DEFAULT_BROWSER_CONTEXT_ID,
            tab_id: tab_id.as_u64(),
            realm_generation: 1,
        };
        let mut executor = JavaScriptPageExecutor::with_config(
            crate::script::javascript::JavaScriptPageExecutorConfig {
                native_debugger_execution_control: true,
                ..crate::script::javascript::JavaScriptPageExecutorConfig::default()
            },
        )
        .unwrap();

        // First lifecycle turn admits but does not execute the declaration.
        executor.synchronize_and_execute(&tabs).unwrap();
        let DebuggerReply::Capabilities(capabilities) =
            handle_debugger_request_with_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::DescribeCapabilities { realm },
            )
        else {
            panic!("the controlled live realm must describe native execution control")
        };
        for capability in [
            DebuggerCapability::Breakpoints,
            DebuggerCapability::PauseResume,
        ] {
            assert!(capabilities.reports.iter().any(|report| {
                report.capability == capability
                    && report.state == DebuggerCapabilityState::Available
            }));
        }
        let DebuggerReply::Programs(programs) = handle_debugger_request_with_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ListPrograms { realm },
        ) else {
            panic!("admission must expose an opaque program before execution")
        };
        let program = programs[0];
        let DebuggerReply::SafePoints(safe_points) =
            handle_debugger_request_with_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::ListSafePoints { program },
            )
        else {
            panic!("the opaque program must retain compiler-verified boundaries")
        };
        let entry = *safe_points
            .iter()
            .find(|safe_point| safe_point.code_unit_ordinal == 0 && safe_point.bytecode_offset == 0)
            .expect("BlueJS root bytecode has a verified entry boundary");
        assert_eq!(
            handle_debugger_request_with_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::ArmEntryBreakpoint { safe_point: entry },
            ),
            DebuggerReply::BreakpointArmed { safe_point: entry }
        );

        // The owner-controlled scheduler now stops before `throw 1` reaches
        // the VM. A state query returns only the opaque exact boundary.
        executor.synchronize_and_execute(&tabs).unwrap();
        assert_eq!(
            handle_debugger_request_with_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::GetExecutionState { program },
            ),
            DebuggerReply::ExecutionState {
                program,
                state: DebuggerExecutionState::Paused { safe_point: entry },
            }
        );
        assert!(executor.drain_reports_for_tab(tab_id).is_empty());
        assert_eq!(
            handle_debugger_request_with_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::ResumeExecution { program },
            ),
            DebuggerReply::ExecutionResumed { program }
        );

        executor.synchronize_and_execute(&tabs).unwrap();
        assert_eq!(
            handle_debugger_request_with_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::GetExecutionState { program },
            ),
            DebuggerReply::ExecutionState {
                program,
                state: DebuggerExecutionState::Completed,
            }
        );
        assert_eq!(
            executor.drain_reports_for_tab(tab_id),
            vec![
                crate::script::javascript::JavaScriptPageExecutionReport::Rejected {
                    tab_id: tab_id.as_u64(),
                    document_generation: 1,
                    ordinal: 0,
                    kind: crate::script::BlueJsPageScriptKind::Classic,
                    category: "BlueJS page execution failed",
                }
            ]
        );
    }

    #[test]
    fn native_debugger_routes_a_non_entry_root_safe_point_to_same_frame_resume() {
        let mut tabs = TabManager::new(320.0, 200.0);
        let tab_id = tabs.default_tab();
        tabs.get_mut(tab_id).unwrap().load_html_str(
            "<script>globalThis.before = 1; throw 2;</script>",
            Some("https://example.test/native-root-safe-point.html".to_string()),
        );
        let realm = DebuggerPageRealm {
            browser_context_id: DEFAULT_BROWSER_CONTEXT_ID,
            tab_id: tab_id.as_u64(),
            realm_generation: 1,
        };
        let mut executor = JavaScriptPageExecutor::with_config(
            crate::script::javascript::JavaScriptPageExecutorConfig {
                native_debugger_execution_control: true,
                ..crate::script::javascript::JavaScriptPageExecutorConfig::default()
            },
        )
        .unwrap();
        executor.synchronize_and_execute(&tabs).unwrap();
        let DebuggerReply::Programs(programs) = handle_debugger_request_with_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ListPrograms { realm },
        ) else {
            panic!("pending classic declaration must expose an opaque program")
        };
        let program = programs[0];
        let DebuggerReply::SafePoints(safe_points) =
            handle_debugger_request_with_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::ListSafePoints { program },
            )
        else {
            panic!("pending classic declaration must expose exact safe points")
        };
        let safe_point = *safe_points
            .iter()
            .find(|safe_point| safe_point.code_unit_ordinal == 0 && safe_point.bytecode_offset != 0)
            .expect("fixture has a non-entry root safe point");
        assert_eq!(
            handle_debugger_request_with_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::ArmRootSafePointBreakpoint { safe_point },
            ),
            DebuggerReply::RootSafePointBreakpointArmed { safe_point }
        );

        executor.synchronize_and_execute(&tabs).unwrap();
        assert_eq!(
            handle_debugger_request_with_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::GetExecutionState { program },
            ),
            DebuggerReply::ExecutionState {
                program,
                state: DebuggerExecutionState::Paused { safe_point },
            }
        );
        assert!(executor.drain_reports_for_tab(tab_id).is_empty());
        assert_eq!(
            handle_debugger_request_with_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::ResumeExecution { program },
            ),
            DebuggerReply::ExecutionResumed { program }
        );
        executor.synchronize_and_execute(&tabs).unwrap();
        assert_eq!(
            handle_debugger_request_with_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::GetExecutionState { program },
            ),
            DebuggerReply::ExecutionState {
                program,
                state: DebuggerExecutionState::Completed,
            }
        );
        assert_eq!(
            executor.drain_reports_for_tab(tab_id),
            vec![
                crate::script::javascript::JavaScriptPageExecutionReport::Rejected {
                    tab_id: tab_id.as_u64(),
                    document_generation: 1,
                    ordinal: 0,
                    kind: crate::script::BlueJsPageScriptKind::Classic,
                    category: "BlueJS page execution failed",
                }
            ]
        );
    }

    #[test]
    fn queued_requests_are_applied_only_by_the_session_owner() {
        let (tabs, realm) = loaded_tabs();
        let (sender, receiver) = debugger_request_channel();
        let (reply_sender, reply_receiver) = mpsc::sync_channel(1);
        sender
            .0
            .send(DebuggerRequestEnvelope {
                request: DebuggerRequest::DescribeCapabilities { realm },
                reply: reply_sender,
            })
            .unwrap();
        assert_eq!(receiver.dispatch_pending(&tabs, None), 1);
        assert!(matches!(
            reply_receiver.recv().unwrap(),
            DebuggerReply::Capabilities(_)
        ));
    }

    #[test]
    fn discovery_refuses_an_unbounded_page_realm_list() {
        let mut tabs = TabManager::new(320.0, 200.0);
        for index in 0..=MAX_DISCOVERABLE_PAGE_REALMS {
            let tab_id = if index == 0 {
                tabs.default_tab()
            } else {
                tabs.open_tab()
            };
            tabs.get_mut(tab_id).unwrap().load_html_str(
                "<main>debugger target</main>",
                Some(format!("https://example.test/{index}")),
            );
        }
        assert!(matches!(
            handle_debugger_request(&tabs, DebuggerRequest::ListPageRealms),
            DebuggerReply::Error {
                code: DebuggerErrorCode::ResourceLimit,
                ..
            }
        ));
    }
}
