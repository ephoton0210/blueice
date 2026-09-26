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
        JavaScriptPageDebuggerError, JavaScriptPageDebuggerExceptionLocationTarget,
        JavaScriptPageDebuggerExecutionState, JavaScriptPageDebuggerFrame,
        JavaScriptPageDebuggerLinkedExecutionState, JavaScriptPageDebuggerLinkedSpanAccess,
        JavaScriptPageDebuggerLinkedStackFrame, JavaScriptPageDebuggerLinkedStackSnapshot,
        JavaScriptPageDebuggerNestedExecutionState, JavaScriptPageDebuggerProgram,
        JavaScriptPageDebuggerSafePoint, JavaScriptPageDebuggerScopeEntry,
        JavaScriptPageDebuggerStaticMetadata,
        JavaScriptPageDebuggerStaticMetadataContractLocationTarget,
        JavaScriptPageDebuggerStaticMetadataContractTarget,
        JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget,
        JavaScriptPageDebuggerStaticMetadataSourceBreakpointTarget,
        JavaScriptPageDebuggerStaticMetadataSourceTarget,
        JavaScriptPageDebuggerStaticMetadataSymbolContractTarget,
        JavaScriptPageDebuggerStaticMetadataSymbolLocationTarget,
        JavaScriptPageDebuggerStaticMetadataSymbolTarget,
        JavaScriptPageDebuggerStaticMetadataSymbolTypeTarget,
        JavaScriptPageDebuggerStaticMetadataTypeTarget, JavaScriptPageDebuggerStaticScopeRelation,
        JavaScriptPageDebuggerStaticScopeTarget, JavaScriptPageDebuggerValuePreview,
        JavaScriptPageDebuggerValueTarget, JavaScriptPageExecutor, PageJavaScriptDebuggerLocations,
        PageJavaScriptExecutor,
    },
    TabId, TabManager,
};
use blueice_ipc::compiler::CompilerContractValue;
use blueice_ipc::debugger::{
    DebuggerCapabilities, DebuggerCapability, DebuggerCapabilityReport, DebuggerCapabilityState,
    DebuggerErrorCode, DebuggerExceptionLocation, DebuggerExecutionState, DebuggerFrame,
    DebuggerLinkedArmTarget, DebuggerLinkedExecutionState, DebuggerLinkedFrame,
    DebuggerLinkedScopeSnapshot, DebuggerLinkedStackCoordinates,
    DebuggerLinkedStackCoordinatesTarget, DebuggerLinkedStackFrame, DebuggerLinkedStackSnapshot,
    DebuggerMetadataCapability, DebuggerMetadataSessionAuthorization, DebuggerPageRealm,
    DebuggerProgram, DebuggerReply, DebuggerRequest, DebuggerSafePoint, DebuggerScopeEntry,
    DebuggerScopeSnapshot, DebuggerStackCoordinates, DebuggerStackCoordinatesTarget,
    DebuggerStackSnapshot, DebuggerStaticMetadataContractDisplay, DebuggerStaticMetadataContractId,
    DebuggerStaticMetadataContractLocation, DebuggerStaticMetadataContractLocationTarget,
    DebuggerStaticMetadataContractValidation, DebuggerStaticMetadataHandle,
    DebuggerStaticMetadataLoweringSummary, DebuggerStaticMetadataSafePointSpan,
    DebuggerStaticMetadataSafePointSpanTarget, DebuggerStaticMetadataSourceBreakpoint,
    DebuggerStaticMetadataSourceBreakpointTarget, DebuggerStaticMetadataSourceId,
    DebuggerStaticMetadataSourceProvenance, DebuggerStaticMetadataSummary,
    DebuggerStaticMetadataSymbolContract, DebuggerStaticMetadataSymbolDisplay,
    DebuggerStaticMetadataSymbolId, DebuggerStaticMetadataSymbolLocation,
    DebuggerStaticMetadataSymbolLocationTarget, DebuggerStaticMetadataSymbolType,
    DebuggerStaticMetadataTypeDisplay, DebuggerStaticMetadataTypeId, DebuggerStaticScopeRelation,
    DebuggerStaticScopeTarget, DebuggerValuePreview, DebuggerValueSnapshot, DebuggerValueTarget,
    DEBUGGER_MAX_SCOPE_ENTRIES, DEBUGGER_MAX_STACK_FRAMES, DEBUGGER_MAX_VALUE_CONTAINER_LENGTH,
    DEBUGGER_MAX_VALUE_DEPTH, DEBUGGER_MAX_VALUE_NODES, DEBUGGER_MAX_VALUE_PAYLOAD_BYTES,
    DEBUGGER_PROTOCOL_VERSION, DEBUGGER_STATIC_METADATA_MAX_CONTRACTS,
    DEBUGGER_STATIC_METADATA_MAX_SOURCES, DEBUGGER_STATIC_METADATA_MAX_SYMBOLS,
    DEBUGGER_STATIC_METADATA_MAX_TYPES,
};
use std::cell::Cell;
use std::collections::HashSet;
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
const MAX_STACK_FRAMES: u32 = DEBUGGER_MAX_STACK_FRAMES;
const MAX_SCOPE_BINDINGS: u32 = DEBUGGER_MAX_SCOPE_ENTRIES;
const MAX_VALUE_PREVIEW_BYTES: u32 = 4_096;

/// A bounded native location reply prevents an instrumented program from
/// turning debugger discovery into an unbounded bytecode inventory channel.
const DEFAULT_MAX_SAFE_POINTS_PER_PROGRAM: usize = 4_096;

/// A debugger peer cannot turn a realm into an unbounded persistent
/// breakpoint-record store. This fallback applies only when no JavaScript
/// executor is present to report its core-selected limit.
const DEFAULT_MAX_BREAKPOINTS_PER_REALM: usize = 256;

/// A direct BlueTS program has one compiler debug record at most. Keep the
/// public socket reply bounded independently of the child transport.
const MAX_STATIC_METADATA_HANDLES_PER_PROGRAM: usize = 1;

/// Sender owned by a debugger-socket worker. It forwards one decoded request
/// to the session thread and waits for that thread's target-checked reply.
#[derive(Clone)]
pub struct DebuggerRequestSender(mpsc::Sender<DebuggerRequestEnvelope>);

/// Receiver owned exclusively by the core session thread.
pub struct DebuggerRequestReceiver {
    requests: mpsc::Receiver<DebuggerRequestEnvelope>,
    /// Shared by every debugger socket routed through this core session.
    /// Zero is terminal exhaustion, never a valid pause incarnation.
    pause_incarnation: Cell<u64>,
}

struct DebuggerRequestEnvelope {
    request: DebuggerRequest,
    metadata_session: Option<DebuggerMetadataSessionAuthorization>,
    reply: mpsc::SyncSender<DebuggerReply>,
}

/// Creates the worker-to-session hand-off for debugger requests. The worker
/// never borrows a tab, page, realm, VM, or BlueJS object.
pub fn debugger_request_channel() -> (DebuggerRequestSender, DebuggerRequestReceiver) {
    let (sender, receiver) = mpsc::channel();
    (
        DebuggerRequestSender(sender),
        DebuggerRequestReceiver {
            requests: receiver,
            pause_incarnation: Cell::new(1),
        },
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
                metadata_session: None,
                reply: reply_sender,
            })
            .map_err(|_| {
                io::Error::new(io::ErrorKind::BrokenPipe, "core debugger session ended")
            })?;
        reply_receiver.recv().map_err(|_| {
            io::Error::new(io::ErrorKind::BrokenPipe, "core debugger reply unavailable")
        })
    }

    /// Routes a request carrying only the core-local authorization recreated
    /// from this socket stream's successful `Hello` exchange. Callers cannot
    /// serialize or forge this value; a missing value is an explicit deny.
    pub fn request_with_metadata_session_authorization(
        &self,
        request: DebuggerRequest,
        metadata_session: DebuggerMetadataSessionAuthorization,
    ) -> io::Result<DebuggerReply> {
        let (reply_sender, reply_receiver) = mpsc::sync_channel(1);
        self.0
            .send(DebuggerRequestEnvelope {
                request,
                metadata_session: Some(metadata_session),
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
    fn invalidate_pause_receipts_for(&self, reply: &DebuggerReply) {
        if matches!(
            reply,
            DebuggerReply::BreakpointArmed { .. }
                | DebuggerReply::RootSafePointBreakpointArmed { .. }
                | DebuggerReply::NestedSafePointBreakpointArmed { .. }
                | DebuggerReply::LinkedNestedSafePointBreakpointArmed { .. }
                | DebuggerReply::ExecutionResumed { .. }
                | DebuggerReply::ExecutionStepRequested { .. }
                | DebuggerReply::NestedStepRequested { .. }
                | DebuggerReply::NestedResumeRequested { .. }
                | DebuggerReply::LinkedNestedResumeRequested { .. }
                | DebuggerReply::ExecutionSourceSpanStepRequested { .. }
        ) {
            let current = self.pause_incarnation.get();
            self.pause_incarnation.set(if current == 0 {
                0
            } else {
                current.checked_add(1).unwrap_or(0)
            });
        }
    }

    /// Resolves a bounded number of worker requests against the current core
    /// state. Replies are best effort: a disconnected debugger client cannot
    /// interrupt rendering or a frontend session.
    pub fn dispatch_pending(
        &self,
        tabs: &TabManager,
        mut javascript_executor: Option<&mut (dyn PageJavaScriptExecutor + '_)>,
    ) -> usize {
        let mut dispatched = 0;
        let mut preserve_pending_entry = false;
        let mut advance_pending_entry = false;
        let mut preserve_execution_transition = false;
        while dispatched < MAX_DEBUGGER_REQUESTS_PER_SESSION_TICK {
            let Ok(envelope) = self.requests.try_recv() else {
                break;
            };
            let requests_execution_transition = matches!(
                &envelope.request,
                DebuggerRequest::ArmEntryBreakpoint { .. }
                    | DebuggerRequest::ArmRootSafePointBreakpoint { .. }
                    | DebuggerRequest::ArmNestedSafePointBreakpoint { .. }
                    | DebuggerRequest::ArmLinkedNestedSafePointBreakpoint { .. }
                    | DebuggerRequest::ArmStaticMetadataSourceBreakpoint { .. }
                    | DebuggerRequest::ResumeExecution { .. }
                    | DebuggerRequest::StepRootInstruction { .. }
                    | DebuggerRequest::StepNestedInstruction { .. }
                    | DebuggerRequest::ResumeNestedExecution { .. }
                    | DebuggerRequest::ResumeLinkedNestedExecution { .. }
                    | DebuggerRequest::StepStaticMetadataSourceSpan { .. }
            );
            let reply = handle_debugger_request_with_page_javascript_executor_and_metadata_session(
                tabs,
                javascript_executor.as_deref_mut(),
                envelope.metadata_session.as_ref(),
                self.pause_incarnation.get(),
                envelope.request,
            );
            self.invalidate_pause_receipts_for(&reply);
            // A rejected arm/resume request must not consume the pending
            // declaration's only discovery turn. Otherwise an invalid child
            // or module target could race the valid root target by causing
            // ordinary execution before the caller receives its error.
            let executes_or_releases_entry = requests_execution_transition
                && matches!(
                    &reply,
                    DebuggerReply::BreakpointArmed { .. }
                        | DebuggerReply::RootSafePointBreakpointArmed { .. }
                        | DebuggerReply::NestedSafePointBreakpointArmed { .. }
                        | DebuggerReply::LinkedNestedSafePointBreakpointArmed { .. }
                        | DebuggerReply::ExecutionResumed { .. }
                        | DebuggerReply::ExecutionStepRequested { .. }
                        | DebuggerReply::NestedStepRequested { .. }
                        | DebuggerReply::NestedResumeRequested { .. }
                        | DebuggerReply::LinkedNestedResumeRequested { .. }
                        | DebuggerReply::ExecutionSourceSpanStepRequested { .. }
                );
            // A state observation after the bounded lifecycle has already
            // left `Pending` must not renew the scheduler hold. In particular
            // a peer may observe the promised one-turn `Resuming` state, but
            // must not keep that root frame suspended by polling it. Location
            // discovery/configuration replies retain their existing admission
            // turn behavior; OOP hosts additionally cap those turns locally.
            let reply_keeps_pending_entry = !matches!(
                &reply,
                DebuggerReply::ExecutionState {
                    state: DebuggerExecutionState::Paused { .. }
                        | DebuggerExecutionState::NestedPaused { .. }
                        | DebuggerExecutionState::SourceStepLimitReached { .. }
                        | DebuggerExecutionState::Stepping
                        | DebuggerExecutionState::NestedStepping { .. }
                        | DebuggerExecutionState::NestedResuming { .. }
                        | DebuggerExecutionState::Resuming
                        | DebuggerExecutionState::Completed,
                    ..
                }
            ) && !matches!(
                &reply,
                DebuggerReply::LinkedExecutionState { state, .. }
                    if !matches!(state.as_ref(), DebuggerLinkedExecutionState::Pending)
            );
            // `Resuming` is a public, source-free state rather than a reply
            // synonym. Preserve it for one owner-session turn after the
            // successful response so a socket peer can observe the exact
            // transition before the scheduler resumes the retained frame.
            // An idle turn still completes it normally, so this never turns
            // the debugger connection into an execution lease.
            preserve_execution_transition |= matches!(
                &reply,
                DebuggerReply::ExecutionResumed { .. }
                    | DebuggerReply::ExecutionStepRequested { .. }
                    | DebuggerReply::NestedStepRequested { .. }
                    | DebuggerReply::NestedResumeRequested { .. }
                    | DebuggerReply::LinkedNestedResumeRequested { .. }
                    | DebuggerReply::ExecutionSourceSpanStepRequested { .. }
            );
            let _ = envelope.reply.send(reply);
            if executes_or_releases_entry {
                advance_pending_entry = true;
            } else if reply_keeps_pending_entry {
                preserve_pending_entry = true;
            }
            dispatched += 1;
        }
        // A page declaration is admitted before a remote peer can learn its
        // opaque program identity. Give each discovery/configuration reply one
        // more session boundary to send the next bounded request; otherwise a
        // normal idle synchronization begins it. Successful arms consume that
        // boundary so their requested state transition happens. A successful
        // resume or step preserves its public transition state for one turn;
        // a following idle turn advances the same private VM frame.
        if (preserve_pending_entry && !advance_pending_entry) || preserve_execution_transition {
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
        DebuggerRequest::ListStaticMetadata { .. } => unavailable_static_metadata_inventory(),
        DebuggerRequest::DescribeStaticMetadata { .. } => unavailable_static_metadata_summary(),
        DebuggerRequest::DescribeStaticMetadataLoweringSummary { .. } => {
            unavailable_static_metadata_lowering_summary()
        }
        DebuggerRequest::ListStaticMetadataSources { .. } => {
            unavailable_static_metadata_source_inventory()
        }
        DebuggerRequest::ListStaticMetadataTypes { .. } => {
            unavailable_static_metadata_type_inventory()
        }
        DebuggerRequest::DescribeStaticMetadataType { .. } => {
            unavailable_static_metadata_type_display()
        }
        DebuggerRequest::ListStaticMetadataSymbols { .. } => {
            unavailable_static_metadata_symbol_inventory()
        }
        DebuggerRequest::ListStaticMetadataContracts { .. } => {
            unavailable_static_metadata_contract_inventory()
        }
        DebuggerRequest::DescribeStaticMetadataContract { .. } => {
            unavailable_static_metadata_contract_display()
        }
        DebuggerRequest::ValidateStaticMetadataContract { .. } => {
            unavailable_static_metadata_contract_validation()
        }
        DebuggerRequest::DescribeStaticMetadataSymbol { .. } => {
            unavailable_static_metadata_symbol_display()
        }
        DebuggerRequest::DescribeStaticMetadataSymbolLocation { .. } => {
            unavailable_static_metadata_symbol_location()
        }
        DebuggerRequest::DescribeStaticMetadataSafePointSpan { .. } => {
            unavailable_static_metadata_safe_point_span()
        }
        DebuggerRequest::DescribeExceptionLocation { .. } => unavailable_exception_location(),
        DebuggerRequest::ResolveStaticMetadataSourceBreakpoint { .. } => {
            unavailable_static_metadata_source_breakpoint()
        }
        DebuggerRequest::ArmStaticMetadataSourceBreakpoint { .. } => {
            unavailable_static_metadata_source_breakpoint_arm()
        }
        DebuggerRequest::DescribeStaticMetadataContractLocation { .. } => {
            unavailable_static_metadata_contract_location()
        }
        DebuggerRequest::DescribeStaticMetadataSymbolType { .. } => {
            unavailable_static_metadata_symbol_type()
        }
        DebuggerRequest::DescribeStaticMetadataSymbolContract { .. } => {
            unavailable_static_metadata_symbol_contract()
        }
        DebuggerRequest::DescribeStaticMetadataSource { .. } => {
            unavailable_static_metadata_source_provenance()
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
        DebuggerRequest::ArmNestedSafePointBreakpoint { .. }
        | DebuggerRequest::StepNestedInstruction { .. }
        | DebuggerRequest::ResumeNestedExecution { .. } => unavailable_nested_frames(),
        DebuggerRequest::ArmLinkedNestedSafePointBreakpoint { .. }
        | DebuggerRequest::GetLinkedExecutionState { .. }
        | DebuggerRequest::GetLinkedStack { .. }
        | DebuggerRequest::ResumeLinkedNestedExecution { .. }
        | DebuggerRequest::GetLinkedStackCoordinates { .. } => unavailable_linked_modules(),
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
        DebuggerRequest::StepRootInstruction { program } => {
            step_root_instruction(tabs, javascript_executor, program)
        }
        DebuggerRequest::StepStaticMetadataSourceSpan { .. } => {
            unavailable_static_metadata_source_span_step()
        }
        DebuggerRequest::GetStack { .. } => unavailable_stack(),
        DebuggerRequest::GetStackCoordinates { .. } => {
            unavailable_static_metadata_safe_point_span()
        }
        DebuggerRequest::GetScopes { .. } => unavailable_scopes(),
        DebuggerRequest::GetLinkedScopes { .. } => unavailable_scopes(),
        DebuggerRequest::GetStaticScopeRelation { .. } => unavailable_static_scope_relation(),
        DebuggerRequest::GetValue { .. } => unavailable_values(),
        DebuggerRequest::Hello { .. } => DebuggerReply::Error {
            code: DebuggerErrorCode::ProtocolVersion,
            message: "debugger Hello is valid only as the first request".to_string(),
        },
        DebuggerRequest::GetSourceText { .. } | DebuggerRequest::Unknown => DebuggerReply::Error {
            code: DebuggerErrorCode::CapabilityUnavailable,
            message: "debugger operation is unavailable".to_string(),
        },
    }
}

/// Resolves debugger requests through the selected session page executor.
///
/// The existing in-process executor continues to own its complete debugger
/// control path unchanged. A launcher-supervised child can opt into only the
/// small source-free program-location adapter; every other operation remains
/// unavailable on that route rather than gaining an accidental proxy to child
/// VM state or execution control.
pub fn handle_debugger_request_with_page_javascript_executor(
    tabs: &TabManager,
    javascript_executor: Option<&mut (dyn PageJavaScriptExecutor + '_)>,
    request: DebuggerRequest,
) -> DebuggerReply {
    handle_debugger_request_with_page_javascript_executor_and_metadata_session(
        tabs,
        javascript_executor,
        None,
        0,
        request,
    )
}

/// Resolves one request with a transport-scoped metadata authorization. This
/// stays private to the core request queue so ordinary callers cannot route a
/// manually constructed session into the metadata operation; absent
/// authorization is always denied.
fn handle_debugger_request_with_page_javascript_executor_and_metadata_session(
    tabs: &TabManager,
    javascript_executor: Option<&mut (dyn PageJavaScriptExecutor + '_)>,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    pause_incarnation: u64,
    request: DebuggerRequest,
) -> DebuggerReply {
    let Some(executor) = javascript_executor else {
        return handle_debugger_request_with_javascript_executor(tabs, None, request);
    };
    if let Some(in_process) = executor.debugger_executor() {
        return handle_debugger_request_with_javascript_executor(tabs, Some(in_process), request);
    }
    let Some(locations) = executor.debugger_locations() else {
        return handle_debugger_request_with_javascript_executor(tabs, None, request);
    };
    handle_debugger_request_with_child_locations_and_pause(
        tabs,
        locations,
        metadata_session,
        pause_incarnation,
        request,
    )
}

#[cfg(test)]
fn handle_debugger_request_with_child_locations(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    request: DebuggerRequest,
) -> DebuggerReply {
    handle_debugger_request_with_child_locations_and_pause(
        tabs,
        locations,
        metadata_session,
        0,
        request,
    )
}

fn handle_debugger_request_with_child_locations_and_pause(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    pause_incarnation: u64,
    request: DebuggerRequest,
) -> DebuggerReply {
    match request {
        DebuggerRequest::ListPageRealms => list_page_realms(tabs),
        DebuggerRequest::DescribeCapabilities { realm } => {
            describe_child_location_capabilities(tabs, locations, metadata_session, realm)
        }
        DebuggerRequest::ListPrograms { realm } => list_child_programs(tabs, locations, realm),
        DebuggerRequest::ListStaticMetadata { program } => {
            list_child_static_metadata(tabs, locations, metadata_session, program)
        }
        DebuggerRequest::DescribeStaticMetadata { metadata } => {
            describe_child_static_metadata(tabs, locations, metadata_session, metadata)
        }
        DebuggerRequest::DescribeStaticMetadataLoweringSummary { metadata } => {
            describe_child_static_metadata_lowering_summary(
                tabs,
                locations,
                metadata_session,
                metadata,
            )
        }
        DebuggerRequest::ListStaticMetadataSources { metadata } => {
            list_child_static_metadata_sources(tabs, locations, metadata_session, metadata)
        }
        DebuggerRequest::ListStaticMetadataTypes { metadata } => {
            list_child_static_metadata_types(tabs, locations, metadata_session, metadata)
        }
        DebuggerRequest::DescribeStaticMetadataType { static_type } => {
            describe_child_static_metadata_type(tabs, locations, metadata_session, static_type)
        }
        DebuggerRequest::ListStaticMetadataSymbols { metadata } => {
            list_child_static_metadata_symbols(tabs, locations, metadata_session, metadata)
        }
        DebuggerRequest::ListStaticMetadataContracts { metadata } => {
            list_child_static_metadata_contracts(tabs, locations, metadata_session, metadata)
        }
        DebuggerRequest::DescribeStaticMetadataContract { contract } => {
            describe_child_static_metadata_contract(tabs, locations, metadata_session, contract)
        }
        DebuggerRequest::ValidateStaticMetadataContract { contract, value } => {
            validate_child_static_metadata_contract(
                tabs,
                locations,
                metadata_session,
                contract,
                value,
            )
        }
        DebuggerRequest::DescribeStaticMetadataSymbol { symbol } => {
            describe_child_static_metadata_symbol(tabs, locations, metadata_session, symbol)
        }
        DebuggerRequest::DescribeStaticMetadataSymbolLocation { target } => {
            describe_child_static_metadata_symbol_location(
                tabs,
                locations,
                metadata_session,
                target,
            )
        }
        DebuggerRequest::DescribeStaticMetadataSafePointSpan { target } => {
            describe_child_static_metadata_safe_point_span(
                tabs,
                locations,
                metadata_session,
                target,
            )
        }
        DebuggerRequest::DescribeExceptionLocation { source } => {
            describe_child_exception_location(tabs, locations, metadata_session, source)
        }
        DebuggerRequest::ResolveStaticMetadataSourceBreakpoint { target } => {
            resolve_child_static_metadata_source_breakpoint(
                tabs,
                locations,
                metadata_session,
                target,
            )
        }
        DebuggerRequest::ArmStaticMetadataSourceBreakpoint { target } => {
            arm_child_static_metadata_source_breakpoint(tabs, locations, metadata_session, target)
        }
        DebuggerRequest::DescribeStaticMetadataContractLocation { target } => {
            describe_child_static_metadata_contract_location(
                tabs,
                locations,
                metadata_session,
                target,
            )
        }
        DebuggerRequest::DescribeStaticMetadataSymbolType { target } => {
            describe_child_static_metadata_symbol_type(tabs, locations, metadata_session, target)
        }
        DebuggerRequest::DescribeStaticMetadataSymbolContract { target } => {
            describe_child_static_metadata_symbol_contract(
                tabs,
                locations,
                metadata_session,
                target,
            )
        }
        DebuggerRequest::DescribeStaticMetadataSource { source } => {
            describe_child_static_metadata_source_provenance(
                tabs,
                locations,
                metadata_session,
                source,
            )
        }
        DebuggerRequest::ListSafePoints { program } => {
            list_child_safe_points(tabs, locations, program)
        }
        DebuggerRequest::ValidateSafePoint { safe_point } => {
            validate_child_safe_point(tabs, locations, safe_point)
        }
        DebuggerRequest::SetBreakpoint { safe_point } => {
            set_child_breakpoint(tabs, locations, safe_point)
        }
        DebuggerRequest::ListBreakpoints { realm } => {
            list_child_breakpoints(tabs, locations, realm)
        }
        DebuggerRequest::ClearBreakpoint { safe_point } => {
            clear_child_breakpoint(tabs, locations, safe_point)
        }
        DebuggerRequest::ArmRootSafePointBreakpoint { safe_point } => {
            arm_child_root_safe_point_breakpoint(tabs, locations, safe_point)
        }
        DebuggerRequest::ArmNestedSafePointBreakpoint { safe_point } => {
            arm_child_nested_safe_point_breakpoint(tabs, locations, safe_point)
        }
        DebuggerRequest::ArmLinkedNestedSafePointBreakpoint { target } => {
            arm_child_linked_nested_safe_point_breakpoint(tabs, locations, target)
        }
        DebuggerRequest::GetExecutionState { program } => {
            child_execution_state(tabs, locations, program)
        }
        DebuggerRequest::ResumeExecution { program } => {
            resume_child_execution(tabs, locations, program)
        }
        DebuggerRequest::StepRootInstruction { program } => {
            step_child_root_instruction(tabs, locations, program)
        }
        DebuggerRequest::StepNestedInstruction { frame } => {
            step_child_nested_instruction(tabs, locations, frame)
        }
        DebuggerRequest::ResumeNestedExecution { frame } => {
            resume_child_nested_execution(tabs, locations, frame)
        }
        DebuggerRequest::GetLinkedExecutionState { entry } => {
            child_linked_execution_state(tabs, locations, entry)
        }
        DebuggerRequest::GetLinkedStack { top_frame } => {
            child_linked_stack(tabs, locations, top_frame)
        }
        DebuggerRequest::ResumeLinkedNestedExecution { top_frame } => {
            resume_child_linked_nested_execution(tabs, locations, top_frame)
        }
        DebuggerRequest::GetLinkedStackCoordinates { target } => {
            child_linked_stack_coordinates(tabs, locations, metadata_session, target)
        }
        DebuggerRequest::GetStack {
            program,
            frame,
            max_frames,
        } => child_stack(tabs, locations, program, frame, max_frames),
        DebuggerRequest::GetStackCoordinates { target } => {
            child_stack_coordinates(tabs, locations, metadata_session, target)
        }
        DebuggerRequest::GetScopes {
            program,
            frame,
            frame_index,
            expected_safe_point,
            max_scope_entries,
        } => {
            let reply = child_scopes(
                tabs,
                locations,
                program,
                frame,
                frame_index,
                expected_safe_point,
                max_scope_entries,
            );
            if let DebuggerReply::Scopes(snapshot) = &reply {
                if metadata_session.is_some_and(|session| {
                    session.permits_bounded_values()
                        || session.permits(DebuggerMetadataCapability::OpaqueStaticScopeRelation)
                }) && !metadata_session
                    .is_some_and(|session| session.observe_scopes(snapshot, pause_incarnation))
                {
                    return DebuggerReply::Error {
                        code: DebuggerErrorCode::ResourceLimit,
                        message: "debugger scope receipt budget is exhausted".to_string(),
                    };
                }
            }
            reply
        }
        DebuggerRequest::GetLinkedScopes {
            expected_stack,
            max_scope_entries,
        } => match staged_child_linked_scopes(tabs, locations, expected_stack, max_scope_entries) {
            Ok(snapshot) => {
                if !snapshot.scope_truncated
                    && metadata_session.is_some_and(|session| {
                        session.permits(DebuggerMetadataCapability::OpaqueStaticScopeRelation)
                    })
                    && !metadata_session.is_some_and(|session| {
                        session.observe_linked_scopes(&snapshot, pause_incarnation)
                    })
                {
                    DebuggerReply::Error {
                        code: DebuggerErrorCode::ResourceLimit,
                        message: "debugger linked scope receipt budget is exhausted".to_string(),
                    }
                } else {
                    DebuggerReply::LinkedScopes(Box::new(snapshot))
                }
            }
            Err(reply) => *reply,
        },
        DebuggerRequest::GetStaticScopeRelation { target } => child_static_scope_relation(
            tabs,
            locations,
            metadata_session,
            pause_incarnation,
            target,
        ),
        DebuggerRequest::GetValue { target } => match child_value_snapshot(
            tabs,
            locations,
            metadata_session,
            metadata_session.is_some_and(|session| session.permits_bounded_values()),
            pause_incarnation,
            target,
        ) {
            Ok(snapshot) => DebuggerReply::Value(Box::new(snapshot)),
            Err(reply) => *reply,
        },
        DebuggerRequest::StepStaticMetadataSourceSpan { target } => {
            step_child_static_metadata_source_span(tabs, locations, metadata_session, target)
        }
        // `ArmEntryBreakpoint` remains an in-process compatibility operation.
        // The isolated route deliberately exposes only its separately named
        // root-classic continuation seam. Nested control, bounded stack and
        // scope inspection use their own exact-target routes; generic
        // interruption, source, bytecode, and values remain unavailable.
        other => handle_debugger_request_with_javascript_executor(tabs, None, other),
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

mod child_metadata_catalog;
use child_metadata_catalog::*;
mod child_metadata_details;
use child_metadata_details::*;
mod child_control;
use child_control::*;
mod child_inspection;
use child_inspection::*;
mod generic_routes;
use generic_routes::*;

#[cfg(test)]
mod tests;
