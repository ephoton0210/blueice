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
        JavaScriptPageDebuggerError, JavaScriptPageDebuggerExecutionState,
        JavaScriptPageDebuggerStaticMetadataContractLocationTarget,
        JavaScriptPageDebuggerStaticMetadataContractTarget,
        JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget,
        JavaScriptPageDebuggerStaticMetadataSourceTarget,
        JavaScriptPageDebuggerStaticMetadataSymbolContractTarget,
        JavaScriptPageDebuggerStaticMetadataSymbolLocationTarget,
        JavaScriptPageDebuggerStaticMetadataSymbolTarget,
        JavaScriptPageDebuggerStaticMetadataSymbolTypeTarget,
        JavaScriptPageDebuggerStaticMetadataTypeTarget, JavaScriptPageExecutor,
        PageJavaScriptDebuggerLocations, PageJavaScriptExecutor,
    },
    TabId, TabManager,
};
use blueice_ipc::compiler::CompilerContractValue;
use blueice_ipc::debugger::{
    DebuggerCapabilities, DebuggerCapability, DebuggerCapabilityReport, DebuggerCapabilityState,
    DebuggerErrorCode, DebuggerExecutionState, DebuggerMetadataCapability,
    DebuggerMetadataSessionAuthorization, DebuggerPageRealm, DebuggerProgram, DebuggerReply,
    DebuggerRequest, DebuggerSafePoint, DebuggerStaticMetadataContractDisplay,
    DebuggerStaticMetadataContractId, DebuggerStaticMetadataContractLocation,
    DebuggerStaticMetadataContractLocationTarget, DebuggerStaticMetadataContractValidation,
    DebuggerStaticMetadataHandle, DebuggerStaticMetadataLoweringSummary,
    DebuggerStaticMetadataSafePointSpan, DebuggerStaticMetadataSafePointSpanTarget,
    DebuggerStaticMetadataSourceId, DebuggerStaticMetadataSourceProvenance,
    DebuggerStaticMetadataSummary, DebuggerStaticMetadataSymbolContract,
    DebuggerStaticMetadataSymbolDisplay, DebuggerStaticMetadataSymbolId,
    DebuggerStaticMetadataSymbolLocation, DebuggerStaticMetadataSymbolLocationTarget,
    DebuggerStaticMetadataSymbolType, DebuggerStaticMetadataTypeDisplay,
    DebuggerStaticMetadataTypeId, DEBUGGER_PROTOCOL_VERSION,
    DEBUGGER_STATIC_METADATA_MAX_CONTRACTS, DEBUGGER_STATIC_METADATA_MAX_SOURCES,
    DEBUGGER_STATIC_METADATA_MAX_SYMBOLS, DEBUGGER_STATIC_METADATA_MAX_TYPES,
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

/// A direct BlueTS program has one compiler debug record at most. Keep the
/// public socket reply bounded independently of the child transport.
const MAX_STATIC_METADATA_HANDLES_PER_PROGRAM: usize = 1;

/// Sender owned by a debugger-socket worker. It forwards one decoded request
/// to the session thread and waits for that thread's target-checked reply.
#[derive(Clone)]
pub struct DebuggerRequestSender(mpsc::Sender<DebuggerRequestEnvelope>);

/// Receiver owned exclusively by the core session thread.
pub struct DebuggerRequestReceiver(mpsc::Receiver<DebuggerRequestEnvelope>);

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
            let Ok(envelope) = self.0.try_recv() else {
                break;
            };
            let requests_execution_transition = matches!(
                &envelope.request,
                DebuggerRequest::ArmEntryBreakpoint { .. }
                    | DebuggerRequest::ArmRootSafePointBreakpoint { .. }
                    | DebuggerRequest::ResumeExecution { .. }
                    | DebuggerRequest::StepRootInstruction { .. }
            );
            let reply = handle_debugger_request_with_page_javascript_executor_and_metadata_session(
                tabs,
                javascript_executor.as_deref_mut(),
                envelope.metadata_session.as_ref(),
                envelope.request,
            );
            // A rejected arm/resume request must not consume the pending
            // declaration's only discovery turn. Otherwise an invalid child
            // or module target could race the valid root target by causing
            // ordinary execution before the caller receives its error.
            let executes_or_releases_entry = requests_execution_transition
                && matches!(
                    &reply,
                    DebuggerReply::BreakpointArmed { .. }
                        | DebuggerReply::RootSafePointBreakpointArmed { .. }
                        | DebuggerReply::ExecutionResumed { .. }
                        | DebuggerReply::ExecutionStepRequested { .. }
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
                        | DebuggerExecutionState::Stepping
                        | DebuggerExecutionState::Resuming
                        | DebuggerExecutionState::Completed,
                    ..
                }
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
    handle_debugger_request_with_child_locations(tabs, locations, metadata_session, request)
}

fn handle_debugger_request_with_child_locations(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
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
        DebuggerRequest::GetExecutionState { program } => {
            child_execution_state(tabs, locations, program)
        }
        DebuggerRequest::ResumeExecution { program } => {
            resume_child_execution(tabs, locations, program)
        }
        DebuggerRequest::StepRootInstruction { program } => {
            step_child_root_instruction(tabs, locations, program)
        }
        // `ArmEntryBreakpoint` remains an in-process compatibility operation.
        // The isolated route deliberately exposes only its separately named
        // root-classic continuation seam, never generic interruption,
        // nested stepping, stacks, scopes, source, bytecode, or values.
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

fn describe_child_location_capabilities(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    realm: DebuggerPageRealm,
) -> DebuggerReply {
    if let Err(reply) = resolve_live_realm(tabs, realm) {
        return *reply;
    }
    // `Available` is stronger than "core has a similarly numbered page":
    // the authenticated child must acknowledge this exact live realm on its
    // own private socket at this session boundary.
    let locations_available =
        locations.debugger_has_live_realm(TabId::from_u64(realm.tab_id), realm.realm_generation);
    let max_safe_points_per_program = if locations_available {
        locations.max_debugger_safe_points_per_program()
    } else {
        DEFAULT_MAX_SAFE_POINTS_PER_PROGRAM
    };
    let breakpoint_configuration_available =
        locations_available && locations.debugger_breakpoint_configuration_available();
    let execution_control_available =
        locations_available && locations.debugger_execution_control_available();
    // Do not advertise an installed private inventory to a peer that has no
    // negotiated grant. This makes the public capability view fail closed as
    // well as the eventual operation; a successful `Hello` still needs this
    // exact live realm to supply the inventory.
    let static_metadata_inventory_available = locations_available
        && metadata_session
            .is_some_and(|session| session.permits(DebuggerMetadataCapability::OpaqueInventory))
        && locations.debugger_static_metadata_inventory_available();
    // A summary has no target without a successful inventory request, so it
    // additionally requires that same session grant. Do not publish either
    // availability bit to an ungranted peer.
    let static_metadata_summary_available = static_metadata_inventory_available
        && metadata_session
            .is_some_and(|session| session.permits(DebuggerMetadataCapability::OpaqueSummary))
        && locations.debugger_static_metadata_summary_available();
    let static_metadata_lowering_summary_available = static_metadata_inventory_available
        && metadata_session.is_some_and(|session| {
            session.permits(DebuggerMetadataCapability::OpaqueLoweringSummary)
        })
        && locations.debugger_static_metadata_lowering_summary_available();
    let static_metadata_source_inventory_available = static_metadata_inventory_available
        && metadata_session.is_some_and(|session| {
            session.permits(DebuggerMetadataCapability::OpaqueSourceInventory)
        })
        && locations.debugger_static_metadata_source_inventory_available();
    let static_metadata_source_provenance_available = static_metadata_source_inventory_available
        && metadata_session.is_some_and(|session| {
            session.permits(DebuggerMetadataCapability::OpaqueSourceProvenance)
        })
        && locations.debugger_static_metadata_source_provenance_available();
    let static_metadata_type_inventory_available = static_metadata_inventory_available
        && metadata_session.is_some_and(|session| {
            session.permits(DebuggerMetadataCapability::OpaqueTypeInventory)
        })
        && locations.debugger_static_metadata_type_inventory_available();
    let static_metadata_type_display_available = static_metadata_type_inventory_available
        && metadata_session
            .is_some_and(|session| session.permits(DebuggerMetadataCapability::OpaqueTypeDisplay))
        && locations.debugger_static_metadata_type_display_available();
    let static_metadata_symbol_inventory_available = static_metadata_inventory_available
        && metadata_session.is_some_and(|session| {
            session.permits(DebuggerMetadataCapability::OpaqueSymbolInventory)
        })
        && locations.debugger_static_metadata_symbol_inventory_available();
    let static_metadata_contract_inventory_available = static_metadata_inventory_available
        && metadata_session.is_some_and(|session| {
            session.permits(DebuggerMetadataCapability::OpaqueContractInventory)
        })
        && locations.debugger_static_metadata_contract_inventory_available();
    let static_metadata_contract_display_available = static_metadata_contract_inventory_available
        && metadata_session.is_some_and(|session| {
            session.permits(DebuggerMetadataCapability::OpaqueContractDisplay)
        })
        && locations.debugger_static_metadata_contract_display_available();
    let static_metadata_contract_validation_available = static_metadata_contract_inventory_available
        && metadata_session.is_some_and(|session| {
            session.permits(DebuggerMetadataCapability::OpaqueContractValidation)
        })
        && locations.debugger_static_metadata_contract_validation_available();
    let static_metadata_symbol_display_available = static_metadata_symbol_inventory_available
        && metadata_session.is_some_and(|session| {
            session.permits(DebuggerMetadataCapability::OpaqueSymbolDisplay)
        })
        && locations.debugger_static_metadata_symbol_display_available();
    let static_metadata_symbol_location_available = static_metadata_source_inventory_available
        && static_metadata_symbol_inventory_available
        && metadata_session.is_some_and(|session| {
            session.permits(DebuggerMetadataCapability::OpaqueSymbolLocation)
        })
        && locations.debugger_static_metadata_symbol_location_available();
    let static_metadata_safe_point_span_available = static_metadata_source_inventory_available
        && metadata_session.is_some_and(|session| {
            session.permits(DebuggerMetadataCapability::OpaqueSafePointSpan)
        })
        && locations.debugger_static_metadata_safe_point_span_available();
    let static_metadata_contract_location_available = static_metadata_source_inventory_available
        && static_metadata_contract_inventory_available
        && metadata_session.is_some_and(|session| {
            session.permits(DebuggerMetadataCapability::OpaqueContractLocation)
        })
        && locations.debugger_static_metadata_contract_location_available();
    let static_metadata_symbol_type_available = static_metadata_type_inventory_available
        && static_metadata_symbol_inventory_available
        && metadata_session
            .is_some_and(|session| session.permits(DebuggerMetadataCapability::OpaqueSymbolType))
        && locations.debugger_static_metadata_symbol_type_available();
    let static_metadata_symbol_contract_available = static_metadata_contract_inventory_available
        && static_metadata_symbol_inventory_available
        && metadata_session.is_some_and(|session| {
            session.permits(DebuggerMetadataCapability::OpaqueSymbolContract)
        })
        && locations.debugger_static_metadata_symbol_contract_available();
    let max_breakpoints_per_realm = if breakpoint_configuration_available {
        locations.max_debugger_breakpoints_per_realm()
    } else {
        DEFAULT_MAX_BREAKPOINTS_PER_REALM
    };
    DebuggerReply::Capabilities(DebuggerCapabilities {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        realm,
        reports: capability_reports(DebuggerCapabilityAvailability {
            program_locations_available: locations_available,
            breakpoint_configuration_available,
            entry_execution_control_available: execution_control_available,
            stepping_available: execution_control_available
                && locations.debugger_stepping_available(),
            static_metadata_inventory_available,
            static_metadata_summary_available,
            static_metadata_lowering_summary_available,
            static_metadata_source_inventory_available,
            static_metadata_source_provenance_available,
            static_metadata_type_inventory_available,
            static_metadata_type_display_available,
            static_metadata_symbol_inventory_available,
            static_metadata_contract_inventory_available,
            static_metadata_contract_display_available,
            static_metadata_contract_validation_available,
            static_metadata_symbol_display_available,
            static_metadata_symbol_location_available,
            static_metadata_safe_point_span_available,
            static_metadata_contract_location_available,
            static_metadata_symbol_type_available,
            static_metadata_symbol_contract_available,
        }),
        max_stack_frames: MAX_STACK_FRAMES,
        max_scope_bindings: MAX_SCOPE_BINDINGS,
        max_value_preview_bytes: MAX_VALUE_PREVIEW_BYTES,
        max_safe_points_per_program: u32::try_from(max_safe_points_per_program)
            .expect("native debugger safe-point reply cap fits the wire type"),
        max_breakpoints_per_realm: u32::try_from(max_breakpoints_per_realm)
            .expect("child debugger breakpoint reply cap fits the wire type"),
    })
}

fn list_child_static_metadata(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    program: DebuggerProgram,
) -> DebuggerReply {
    if !program.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger program target".to_string(),
        };
    }
    let Some(metadata_session) = metadata_session else {
        return unavailable_static_metadata_inventory();
    };
    let capabilities = describe_child_location_capabilities(
        tabs,
        locations,
        Some(metadata_session),
        program.realm,
    );
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        return capabilities;
    };
    let Some(authorization) = capabilities.authorize_metadata(
        metadata_session,
        DebuggerMetadataCapability::OpaqueInventory,
    ) else {
        return unavailable_static_metadata_inventory();
    };
    if !authorization.permits(program.realm, DebuggerMetadataCapability::OpaqueInventory) {
        return unavailable_static_metadata_inventory();
    }

    let tab_id = match resolve_live_realm(tabs, program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    match locations.debugger_static_metadata(
        tab_id,
        program.realm.realm_generation,
        program.program_handle,
        program.program_generation,
    ) {
        Ok(metadata) if metadata.len() <= MAX_STATIC_METADATA_HANDLES_PER_PROGRAM => {
            let mut identities = std::collections::BTreeSet::new();
            let mut handles = Vec::with_capacity(metadata.len());
            for metadata in metadata {
                if metadata.metadata_handle == 0
                    || metadata.metadata_generation == 0
                    || !identities.insert((metadata.metadata_handle, metadata.metadata_generation))
                {
                    return DebuggerReply::Error {
                        code: DebuggerErrorCode::InvalidTarget,
                        message: "invalid opaque debugger static metadata inventory".to_string(),
                    };
                }
                handles.push(DebuggerStaticMetadataHandle {
                    program,
                    metadata_handle: metadata.metadata_handle,
                    metadata_generation: metadata.metadata_generation,
                });
            }
            // The inventory is the only operation that can mint a parent
            // handle into this stream's local receipt ledger. Every dependent
            // metadata capability must have this receipt first; none may turn
            // a guessed numeric handle into a child query target.
            if (metadata_session.permits(DebuggerMetadataCapability::OpaqueSummary)
                || metadata_session.permits(DebuggerMetadataCapability::OpaqueSourceInventory)
                || metadata_session.permits(DebuggerMetadataCapability::OpaqueTypeInventory)
                || metadata_session.permits(DebuggerMetadataCapability::OpaqueSymbolInventory)
                || metadata_session.permits(DebuggerMetadataCapability::OpaqueContractInventory)
                || metadata_session.permits(DebuggerMetadataCapability::OpaqueLoweringSummary))
                && !metadata_session.observe_metadata(&handles)
            {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::ResourceLimit,
                    message: "debugger static metadata receipt budget is exhausted".to_string(),
                };
            }
            DebuggerReply::StaticMetadata(handles)
        }
        Ok(_) => DebuggerReply::Error {
            code: DebuggerErrorCode::ResourceLimit,
            message: "debugger static metadata inventory exceeds its fixed limit".to_string(),
        },
        Err(error) => debugger_program_error(error),
    }
}

fn describe_child_static_metadata(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    metadata: DebuggerStaticMetadataHandle,
) -> DebuggerReply {
    if !metadata.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger static metadata target".to_string(),
        };
    }
    let Some(metadata_session) = metadata_session else {
        return unavailable_static_metadata_summary();
    };
    // A summary can only be associated with a handle minted by inventory on
    // this same stream. A malformed/partial session or guessed numeric handle
    // can never turn the dependent summary grant into a target-probing
    // capability.
    if !metadata_session.permits(DebuggerMetadataCapability::OpaqueInventory)
        || !metadata_session.observed_metadata(metadata)
    {
        return unavailable_static_metadata_summary();
    }
    let capabilities = describe_child_location_capabilities(
        tabs,
        locations,
        Some(metadata_session),
        metadata.program.realm,
    );
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        return capabilities;
    };
    let Some(authorization) = capabilities
        .authorize_metadata(metadata_session, DebuggerMetadataCapability::OpaqueSummary)
    else {
        return unavailable_static_metadata_summary();
    };
    if !authorization.permits(
        metadata.program.realm,
        DebuggerMetadataCapability::OpaqueSummary,
    ) {
        return unavailable_static_metadata_summary();
    }

    let tab_id = match resolve_live_realm(tabs, metadata.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    match locations.debugger_static_metadata_summary(
        tab_id,
        metadata.program.realm.realm_generation,
        metadata.program.program_handle,
        metadata.program.program_generation,
        metadata.metadata_handle,
        metadata.metadata_generation,
    ) {
        Ok(summary) => {
            let summary = DebuggerStaticMetadataSummary {
                metadata,
                language_version: summary.language_version,
                compiler_options_hash: summary.compiler_options_hash,
                source_count: summary.source_count,
                type_count: summary.type_count,
                symbol_count: summary.symbol_count,
                contract_count: summary.contract_count,
            };
            if !summary.is_well_formed() {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget,
                    message: "invalid bounded debugger static metadata summary".to_string(),
                };
            }
            DebuggerReply::StaticMetadataSummary(summary)
        }
        Err(error) => debugger_program_error(error),
    }
}

/// Returns only aggregate evidence for an exact prior opaque metadata receipt.
/// This path intentionally has no per-entry source-map, source-span, AST, or
/// bytecode lookup operation; it exposes only the verified direct-map ABI and
/// aggregate count after the complete live tuple has been revalidated.
fn describe_child_static_metadata_lowering_summary(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    metadata: DebuggerStaticMetadataHandle,
) -> DebuggerReply {
    if !metadata.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger static metadata lowering summary target".to_string(),
        };
    }
    let Some(metadata_session) = metadata_session else {
        return unavailable_static_metadata_lowering_summary();
    };
    if !metadata_session.permits(DebuggerMetadataCapability::OpaqueInventory)
        || !metadata_session.observed_metadata(metadata)
    {
        return unavailable_static_metadata_lowering_summary();
    }
    let capabilities = describe_child_location_capabilities(
        tabs,
        locations,
        Some(metadata_session),
        metadata.program.realm,
    );
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        return capabilities;
    };
    let Some(authorization) = capabilities.authorize_metadata(
        metadata_session,
        DebuggerMetadataCapability::OpaqueLoweringSummary,
    ) else {
        return unavailable_static_metadata_lowering_summary();
    };
    if !authorization.permits(
        metadata.program.realm,
        DebuggerMetadataCapability::OpaqueLoweringSummary,
    ) {
        return unavailable_static_metadata_lowering_summary();
    }
    let tab_id = match resolve_live_realm(tabs, metadata.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    match locations.debugger_static_metadata_lowering_summary(
        tab_id,
        metadata.program.realm.realm_generation,
        metadata.program.program_handle,
        metadata.program.program_generation,
        metadata.metadata_handle,
        metadata.metadata_generation,
    ) {
        Ok(summary) => {
            let summary = DebuggerStaticMetadataLoweringSummary {
                metadata,
                safe_point_map_abi: summary.safe_point_map_abi,
                program_abi: summary.program_abi,
                source_set_hash: summary.source_set_hash,
                bound_safe_point_count: summary.bound_safe_point_count,
            };
            if !summary.is_well_formed() {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget,
                    message: "invalid debugger static metadata lowering summary".to_string(),
                };
            }
            DebuggerReply::StaticMetadataLoweringSummary(Box::new(summary))
        }
        Err(error) => debugger_program_error(error),
    }
}

fn list_child_static_metadata_sources(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    metadata: DebuggerStaticMetadataHandle,
) -> DebuggerReply {
    if !metadata.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger static metadata source inventory target".to_string(),
        };
    }
    let Some(metadata_session) = metadata_session else {
        return unavailable_static_metadata_source_inventory();
    };
    // Source inventory has the same parent receipt boundary as summary: it
    // does not admit a caller-invented metadata handle as a source-ID oracle.
    if !metadata_session.permits(DebuggerMetadataCapability::OpaqueInventory)
        || !metadata_session.observed_metadata(metadata)
    {
        return unavailable_static_metadata_source_inventory();
    }
    let capabilities = describe_child_location_capabilities(
        tabs,
        locations,
        Some(metadata_session),
        metadata.program.realm,
    );
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        // Preserve the same stale/invalid realm outcome as the parent
        // inventory and sibling summary paths. This still occurs before any
        // child source-record access or source-ID disclosure.
        return capabilities;
    };
    let Some(authorization) = capabilities.authorize_metadata(
        metadata_session,
        DebuggerMetadataCapability::OpaqueSourceInventory,
    ) else {
        return unavailable_static_metadata_source_inventory();
    };
    if !authorization.permits(
        metadata.program.realm,
        DebuggerMetadataCapability::OpaqueSourceInventory,
    ) {
        return unavailable_static_metadata_source_inventory();
    }
    let tab_id = match resolve_live_realm(tabs, metadata.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    match locations.debugger_static_metadata_sources(
        tab_id,
        metadata.program.realm.realm_generation,
        metadata.program.program_handle,
        metadata.program.program_generation,
        metadata.metadata_handle,
        metadata.metadata_generation,
    ) {
        Ok(sources)
            if sources.len() <= usize::try_from(DEBUGGER_STATIC_METADATA_MAX_SOURCES).unwrap() =>
        {
            let mut seen = std::collections::BTreeSet::new();
            let mut result = Vec::with_capacity(sources.len());
            for source in sources {
                if !seen.insert(source.source_id) {
                    return DebuggerReply::Error {
                        code: DebuggerErrorCode::InvalidTarget,
                        message: "duplicate debugger static metadata source identity".to_string(),
                    };
                }
                result.push(DebuggerStaticMetadataSourceId {
                    metadata,
                    source_id: source.source_id,
                });
            }
            // Keep only a bounded local receipt set when an enabled dependent
            // operation can consume source IDs. This preserves default deny
            // for inventory-only sessions while letting symbol-location use
            // the exact same stream-local identity boundary as provenance.
            if (metadata_session.permits(DebuggerMetadataCapability::OpaqueSourceProvenance)
                || metadata_session.permits(DebuggerMetadataCapability::OpaqueSymbolLocation)
                || metadata_session.permits(DebuggerMetadataCapability::OpaqueContractLocation)
                || metadata_session.permits(DebuggerMetadataCapability::OpaqueSafePointSpan))
                && !metadata_session.observe_sources(&result)
            {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::ResourceLimit,
                    message: "debugger static metadata source receipt budget is exhausted"
                        .to_string(),
                };
            }
            DebuggerReply::StaticMetadataSources(result)
        }
        Ok(_) => DebuggerReply::Error {
            code: DebuggerErrorCode::ResourceLimit,
            message: "debugger static metadata source inventory exceeds its fixed limit"
                .to_string(),
        },
        Err(error) => debugger_program_error(error),
    }
}

/// Lists compiler-minted type IDs under one opaque metadata parent. The
/// inventory is default-deny and payload-free: it is deliberately not a type
/// display or a static-record dereference operation.
fn list_child_static_metadata_types(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    metadata: DebuggerStaticMetadataHandle,
) -> DebuggerReply {
    if !metadata.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger static metadata type inventory target".to_string(),
        };
    }
    let Some(metadata_session) = metadata_session else {
        return unavailable_static_metadata_type_inventory();
    };
    // No guessed parent may query a child type table. The public handle must
    // have crossed this exact stream's inventory receipt boundary first.
    if !metadata_session.permits(DebuggerMetadataCapability::OpaqueInventory)
        || !metadata_session.observed_metadata(metadata)
    {
        return unavailable_static_metadata_type_inventory();
    }
    let capabilities = describe_child_location_capabilities(
        tabs,
        locations,
        Some(metadata_session),
        metadata.program.realm,
    );
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        return capabilities;
    };
    let Some(authorization) = capabilities.authorize_metadata(
        metadata_session,
        DebuggerMetadataCapability::OpaqueTypeInventory,
    ) else {
        return unavailable_static_metadata_type_inventory();
    };
    if !authorization.permits(
        metadata.program.realm,
        DebuggerMetadataCapability::OpaqueTypeInventory,
    ) {
        return unavailable_static_metadata_type_inventory();
    }
    let tab_id = match resolve_live_realm(tabs, metadata.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    match locations.debugger_static_metadata_types(
        tab_id,
        metadata.program.realm.realm_generation,
        metadata.program.program_handle,
        metadata.program.program_generation,
        metadata.metadata_handle,
        metadata.metadata_generation,
    ) {
        Ok(types)
            if types.len() <= usize::try_from(DEBUGGER_STATIC_METADATA_MAX_TYPES).unwrap() =>
        {
            let mut seen = std::collections::BTreeSet::new();
            let mut result = Vec::with_capacity(types.len());
            for static_type in types {
                if !seen.insert(static_type.type_id) {
                    return DebuggerReply::Error {
                        code: DebuggerErrorCode::InvalidTarget,
                        message: "duplicate debugger static metadata type identity".to_string(),
                    };
                }
                result.push(DebuggerStaticMetadataTypeId {
                    metadata,
                    type_id: static_type.type_id,
                });
            }
            if !metadata_session.observe_types(&result) {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::ResourceLimit,
                    message: "debugger static metadata type receipt budget is exhausted"
                        .to_string(),
                };
            }
            DebuggerReply::StaticMetadataTypes(result)
        }
        Ok(_) => DebuggerReply::Error {
            code: DebuggerErrorCode::ResourceLimit,
            message: "debugger static metadata type inventory exceeds its fixed limit".to_string(),
        },
        Err(error) => debugger_program_error(error),
    }
}

/// Lists compiler-minted symbol IDs under one opaque metadata parent. This
/// default-deny operation is payload-free and records a same-stream receipt
/// now, so a later symbol detail operation cannot accept a guessed ID.
fn list_child_static_metadata_symbols(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    metadata: DebuggerStaticMetadataHandle,
) -> DebuggerReply {
    if !metadata.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger static metadata symbol inventory target".to_string(),
        };
    }
    let Some(metadata_session) = metadata_session else {
        return unavailable_static_metadata_symbol_inventory();
    };
    if !metadata_session.permits(DebuggerMetadataCapability::OpaqueInventory)
        || !metadata_session.observed_metadata(metadata)
    {
        return unavailable_static_metadata_symbol_inventory();
    }
    let capabilities = describe_child_location_capabilities(
        tabs,
        locations,
        Some(metadata_session),
        metadata.program.realm,
    );
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        return capabilities;
    };
    let Some(authorization) = capabilities.authorize_metadata(
        metadata_session,
        DebuggerMetadataCapability::OpaqueSymbolInventory,
    ) else {
        return unavailable_static_metadata_symbol_inventory();
    };
    if !authorization.permits(
        metadata.program.realm,
        DebuggerMetadataCapability::OpaqueSymbolInventory,
    ) {
        return unavailable_static_metadata_symbol_inventory();
    }
    let tab_id = match resolve_live_realm(tabs, metadata.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    match locations.debugger_static_metadata_symbols(
        tab_id,
        metadata.program.realm.realm_generation,
        metadata.program.program_handle,
        metadata.program.program_generation,
        metadata.metadata_handle,
        metadata.metadata_generation,
    ) {
        Ok(symbols)
            if symbols.len() <= usize::try_from(DEBUGGER_STATIC_METADATA_MAX_SYMBOLS).unwrap() =>
        {
            let mut seen = std::collections::BTreeSet::new();
            let mut result = Vec::with_capacity(symbols.len());
            for symbol in symbols {
                if !seen.insert(symbol.symbol_id) {
                    return DebuggerReply::Error {
                        code: DebuggerErrorCode::InvalidTarget,
                        message: "duplicate debugger static metadata symbol identity".to_string(),
                    };
                }
                result.push(DebuggerStaticMetadataSymbolId {
                    metadata,
                    symbol_id: symbol.symbol_id,
                });
            }
            if !metadata_session.observe_symbols(&result) {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::ResourceLimit,
                    message: "debugger static metadata symbol receipt budget is exhausted"
                        .to_string(),
                };
            }
            DebuggerReply::StaticMetadataSymbols(result)
        }
        Ok(_) => DebuggerReply::Error {
            code: DebuggerErrorCode::ResourceLimit,
            message: "debugger static metadata symbol inventory exceeds its fixed limit"
                .to_string(),
        },
        Err(error) => debugger_program_error(error),
    }
}

/// Lists compiler-minted contract IDs under one opaque metadata parent. This
/// default-deny operation is payload-free and records a same-stream receipt
/// now, so a later plan or validation operation cannot accept a guessed ID.
fn list_child_static_metadata_contracts(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    metadata: DebuggerStaticMetadataHandle,
) -> DebuggerReply {
    if !metadata.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger static metadata contract inventory target".to_string(),
        };
    }
    let Some(metadata_session) = metadata_session else {
        return unavailable_static_metadata_contract_inventory();
    };
    if !metadata_session.permits(DebuggerMetadataCapability::OpaqueInventory)
        || !metadata_session.observed_metadata(metadata)
    {
        return unavailable_static_metadata_contract_inventory();
    }
    let capabilities = describe_child_location_capabilities(
        tabs,
        locations,
        Some(metadata_session),
        metadata.program.realm,
    );
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        return capabilities;
    };
    let Some(authorization) = capabilities.authorize_metadata(
        metadata_session,
        DebuggerMetadataCapability::OpaqueContractInventory,
    ) else {
        return unavailable_static_metadata_contract_inventory();
    };
    if !authorization.permits(
        metadata.program.realm,
        DebuggerMetadataCapability::OpaqueContractInventory,
    ) {
        return unavailable_static_metadata_contract_inventory();
    }
    let tab_id = match resolve_live_realm(tabs, metadata.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    match locations.debugger_static_metadata_contracts(
        tab_id,
        metadata.program.realm.realm_generation,
        metadata.program.program_handle,
        metadata.program.program_generation,
        metadata.metadata_handle,
        metadata.metadata_generation,
    ) {
        Ok(contracts)
            if contracts.len()
                <= usize::try_from(DEBUGGER_STATIC_METADATA_MAX_CONTRACTS).unwrap() =>
        {
            let mut seen = std::collections::BTreeSet::new();
            let mut result = Vec::with_capacity(contracts.len());
            for contract in contracts {
                if !seen.insert(contract.contract_id) {
                    return DebuggerReply::Error {
                        code: DebuggerErrorCode::InvalidTarget,
                        message: "duplicate debugger static metadata contract identity".to_string(),
                    };
                }
                result.push(DebuggerStaticMetadataContractId {
                    metadata,
                    contract_id: contract.contract_id,
                });
            }
            if !metadata_session.observe_contracts(&result) {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::ResourceLimit,
                    message: "debugger static metadata contract receipt budget is exhausted"
                        .to_string(),
                };
            }
            DebuggerReply::StaticMetadataContracts(result)
        }
        Ok(_) => DebuggerReply::Error {
            code: DebuggerErrorCode::ResourceLimit,
            message: "debugger static metadata contract inventory exceeds its fixed limit"
                .to_string(),
        },
        Err(error) => debugger_program_error(error),
    }
}

/// Discloses one bounded compiler-produced contract display only after the
/// exact contract ID crossed this stream's contract-inventory receipt boundary.
/// The target keeps its opaque parent and all generations, so a caller-supplied
/// number cannot probe a child contract table by itself.
fn describe_child_static_metadata_contract(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    contract: DebuggerStaticMetadataContractId,
) -> DebuggerReply {
    if !contract.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger static metadata contract display target".to_string(),
        };
    }
    let Some(metadata_session) = metadata_session else {
        return unavailable_static_metadata_contract_display();
    };
    if !metadata_session.permits(DebuggerMetadataCapability::OpaqueInventory)
        || !metadata_session.permits(DebuggerMetadataCapability::OpaqueContractInventory)
        || !metadata_session.observed_contract(contract)
    {
        return unavailable_static_metadata_contract_display();
    }
    let capabilities = describe_child_location_capabilities(
        tabs,
        locations,
        Some(metadata_session),
        contract.metadata.program.realm,
    );
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        return capabilities;
    };
    let Some(authorization) = capabilities.authorize_metadata(
        metadata_session,
        DebuggerMetadataCapability::OpaqueContractDisplay,
    ) else {
        return unavailable_static_metadata_contract_display();
    };
    if !authorization.permits(
        contract.metadata.program.realm,
        DebuggerMetadataCapability::OpaqueContractDisplay,
    ) {
        return unavailable_static_metadata_contract_display();
    }
    let tab_id = match resolve_live_realm(tabs, contract.metadata.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    match locations.debugger_static_metadata_contract_display(
        tab_id,
        contract.metadata.program.realm.realm_generation,
        JavaScriptPageDebuggerStaticMetadataContractTarget {
            program_handle: contract.metadata.program.program_handle,
            program_generation: contract.metadata.program.program_generation,
            metadata_handle: contract.metadata.metadata_handle,
            metadata_generation: contract.metadata.metadata_generation,
            contract_id: contract.contract_id,
        },
    ) {
        Ok(contract_display) => {
            if contract_display.contract_id != contract.contract_id {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget,
                    message: "mismatched debugger static metadata contract display identity"
                        .to_string(),
                };
            }
            let contract_display = DebuggerStaticMetadataContractDisplay {
                contract,
                display: contract_display.display,
                root_kind: contract_display.root_kind,
            };
            if !contract_display.is_well_formed() {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget,
                    message: "invalid debugger static metadata contract display".to_string(),
                };
            }
            DebuggerReply::StaticMetadataContract(contract_display)
        }
        Err(error) => debugger_program_error(error),
    }
}

/// Validates a bounded data-only snapshot only after its exact contract ID
/// crossed this stream's contract-inventory receipt boundary. The public reply
/// intentionally carries just a boolean: plan and structural failure detail
/// remain private to the supervised child.
fn validate_child_static_metadata_contract(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    contract: DebuggerStaticMetadataContractId,
    value: CompilerContractValue,
) -> DebuggerReply {
    if !contract.is_well_formed() || !debugger_contract_value_is_within_fixed_limits(&value) {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger static metadata contract validation target".to_string(),
        };
    }
    let Some(metadata_session) = metadata_session else {
        return unavailable_static_metadata_contract_validation();
    };
    if !metadata_session.permits(DebuggerMetadataCapability::OpaqueInventory)
        || !metadata_session.permits(DebuggerMetadataCapability::OpaqueContractInventory)
        || !metadata_session.observed_contract(contract)
    {
        return unavailable_static_metadata_contract_validation();
    }
    let capabilities = describe_child_location_capabilities(
        tabs,
        locations,
        Some(metadata_session),
        contract.metadata.program.realm,
    );
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        return capabilities;
    };
    let Some(authorization) = capabilities.authorize_metadata(
        metadata_session,
        DebuggerMetadataCapability::OpaqueContractValidation,
    ) else {
        return unavailable_static_metadata_contract_validation();
    };
    if !authorization.permits(
        contract.metadata.program.realm,
        DebuggerMetadataCapability::OpaqueContractValidation,
    ) {
        return unavailable_static_metadata_contract_validation();
    }
    let tab_id = match resolve_live_realm(tabs, contract.metadata.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    match locations.debugger_static_metadata_contract_validation(
        tab_id,
        contract.metadata.program.realm.realm_generation,
        JavaScriptPageDebuggerStaticMetadataContractTarget {
            program_handle: contract.metadata.program.program_handle,
            program_generation: contract.metadata.program.program_generation,
            metadata_handle: contract.metadata.metadata_handle,
            metadata_generation: contract.metadata.metadata_generation,
            contract_id: contract.contract_id,
        },
        value,
    ) {
        Ok(validation) => {
            if validation.contract_id != contract.contract_id {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget,
                    message: "mismatched debugger static metadata contract validation identity"
                        .to_string(),
                };
            }
            let validation = DebuggerStaticMetadataContractValidation {
                contract,
                valid: validation.valid,
            };
            if !validation.is_well_formed() {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget,
                    message: "invalid debugger static metadata contract validation".to_string(),
                };
            }
            DebuggerReply::StaticMetadataContractValidation(validation)
        }
        Err(error) => debugger_program_error(error),
    }
}

/// Checks the exact data-only wire envelope before core forwards it to the
/// child. This iterative check avoids a recursive pre-validation walk and
/// enforces the same immutable limits again in the child before plan use.
fn debugger_contract_value_is_within_fixed_limits(value: &CompilerContractValue) -> bool {
    use blueice_ipc::debugger::{
        DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_COLLECTION_ENTRIES,
        DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_DEPTH,
        DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_NODES,
        DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_STRING_BYTES,
    };

    let mut nodes = 0usize;
    let mut pending = vec![(value, 0usize)];
    while let Some((value, depth)) = pending.pop() {
        if depth > DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_DEPTH
            || nodes >= DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_NODES
        {
            return false;
        }
        nodes += 1;
        match value {
            CompilerContractValue::Null
            | CompilerContractValue::Undefined
            | CompilerContractValue::Boolean(_) => {}
            CompilerContractValue::Number(value) => {
                if value
                    .parse::<f64>()
                    .ok()
                    .is_none_or(|value| !value.is_finite())
                {
                    return false;
                }
            }
            CompilerContractValue::String(value) => {
                if value.len() > DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_STRING_BYTES {
                    return false;
                }
            }
            CompilerContractValue::Array(values) => {
                if values.len()
                    > DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_COLLECTION_ENTRIES
                {
                    return false;
                }
                pending.extend(values.iter().map(|value| (value, depth + 1)));
            }
            CompilerContractValue::Object(values) => {
                if values.len()
                    > DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_COLLECTION_ENTRIES
                    || values.keys().any(|key| {
                        key.len() > DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_STRING_BYTES
                    })
                {
                    return false;
                }
                pending.extend(values.values().map(|value| (value, depth + 1)));
            }
        }
    }
    true
}

/// Discloses one bounded compiler-produced display only after the exact type
/// ID crossed this stream's type-inventory receipt boundary. The target keeps
/// the opaque parent and all generations, so a caller-supplied number cannot
/// probe a child type table by itself.
fn describe_child_static_metadata_type(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    static_type: DebuggerStaticMetadataTypeId,
) -> DebuggerReply {
    if !static_type.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger static metadata type display target".to_string(),
        };
    }
    let Some(metadata_session) = metadata_session else {
        return unavailable_static_metadata_type_display();
    };
    if !metadata_session.permits(DebuggerMetadataCapability::OpaqueInventory)
        || !metadata_session.permits(DebuggerMetadataCapability::OpaqueTypeInventory)
        || !metadata_session.observed_type(static_type)
    {
        return unavailable_static_metadata_type_display();
    }
    let capabilities = describe_child_location_capabilities(
        tabs,
        locations,
        Some(metadata_session),
        static_type.metadata.program.realm,
    );
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        return capabilities;
    };
    let Some(authorization) = capabilities.authorize_metadata(
        metadata_session,
        DebuggerMetadataCapability::OpaqueTypeDisplay,
    ) else {
        return unavailable_static_metadata_type_display();
    };
    if !authorization.permits(
        static_type.metadata.program.realm,
        DebuggerMetadataCapability::OpaqueTypeDisplay,
    ) {
        return unavailable_static_metadata_type_display();
    }
    let tab_id = match resolve_live_realm(tabs, static_type.metadata.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    match locations.debugger_static_metadata_type_display(
        tab_id,
        static_type.metadata.program.realm.realm_generation,
        JavaScriptPageDebuggerStaticMetadataTypeTarget {
            program_handle: static_type.metadata.program.program_handle,
            program_generation: static_type.metadata.program.program_generation,
            metadata_handle: static_type.metadata.metadata_handle,
            metadata_generation: static_type.metadata.metadata_generation,
            type_id: static_type.type_id,
        },
    ) {
        Ok(type_display) => {
            let type_display = DebuggerStaticMetadataTypeDisplay {
                static_type,
                display: type_display.display,
            };
            if !type_display.is_well_formed() {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget,
                    message: "invalid debugger static metadata type display".to_string(),
                };
            }
            DebuggerReply::StaticMetadataType(type_display)
        }
        Err(error) => debugger_program_error(error),
    }
}

/// Discloses one bounded compiler-produced symbol display only after the
/// exact symbol ID crossed this stream's symbol-inventory receipt boundary.
/// The target keeps its opaque parent and all generations, so a caller-supplied
/// number cannot probe a child symbol table by itself.
fn describe_child_static_metadata_symbol(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    symbol: DebuggerStaticMetadataSymbolId,
) -> DebuggerReply {
    if !symbol.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger static metadata symbol display target".to_string(),
        };
    }
    let Some(metadata_session) = metadata_session else {
        return unavailable_static_metadata_symbol_display();
    };
    if !metadata_session.permits(DebuggerMetadataCapability::OpaqueInventory)
        || !metadata_session.permits(DebuggerMetadataCapability::OpaqueSymbolInventory)
        || !metadata_session.observed_symbol(symbol)
    {
        return unavailable_static_metadata_symbol_display();
    }
    let capabilities = describe_child_location_capabilities(
        tabs,
        locations,
        Some(metadata_session),
        symbol.metadata.program.realm,
    );
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        return capabilities;
    };
    let Some(authorization) = capabilities.authorize_metadata(
        metadata_session,
        DebuggerMetadataCapability::OpaqueSymbolDisplay,
    ) else {
        return unavailable_static_metadata_symbol_display();
    };
    if !authorization.permits(
        symbol.metadata.program.realm,
        DebuggerMetadataCapability::OpaqueSymbolDisplay,
    ) {
        return unavailable_static_metadata_symbol_display();
    }
    let tab_id = match resolve_live_realm(tabs, symbol.metadata.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    match locations.debugger_static_metadata_symbol_display(
        tab_id,
        symbol.metadata.program.realm.realm_generation,
        JavaScriptPageDebuggerStaticMetadataSymbolTarget {
            program_handle: symbol.metadata.program.program_handle,
            program_generation: symbol.metadata.program.program_generation,
            metadata_handle: symbol.metadata.metadata_handle,
            metadata_generation: symbol.metadata.metadata_generation,
            symbol_id: symbol.symbol_id,
        },
    ) {
        Ok(symbol_display) => {
            if symbol_display.symbol_id != symbol.symbol_id {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget,
                    message: "mismatched debugger static metadata symbol display identity"
                        .to_string(),
                };
            }
            let symbol_display = DebuggerStaticMetadataSymbolDisplay {
                symbol,
                display: symbol_display.display,
                kind: symbol_display.kind,
                exported: symbol_display.exported,
            };
            if !symbol_display.is_well_formed() {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget,
                    message: "invalid debugger static metadata symbol display".to_string(),
                };
            }
            DebuggerReply::StaticMetadataSymbol(symbol_display)
        }
        Err(error) => debugger_program_error(error),
    }
}

/// Discloses one half-open source byte range only after the exact stream has
/// independently inventoried both its symbol and source IDs. The caller never
/// supplies an offset, and the reply contains no source/module/name/type/
/// contract/bytecode data or source-map translation.
fn describe_child_static_metadata_symbol_location(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    target: DebuggerStaticMetadataSymbolLocationTarget,
) -> DebuggerReply {
    if !target.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger static metadata symbol location target".to_string(),
        };
    }
    let Some(metadata_session) = metadata_session else {
        return unavailable_static_metadata_symbol_location();
    };
    if !metadata_session.permits(DebuggerMetadataCapability::OpaqueInventory)
        || !metadata_session.permits(DebuggerMetadataCapability::OpaqueSourceInventory)
        || !metadata_session.permits(DebuggerMetadataCapability::OpaqueSymbolInventory)
        || !metadata_session.observed_symbol(target.symbol)
        || !metadata_session.observed_source(target.source)
    {
        return unavailable_static_metadata_symbol_location();
    }
    let capabilities = describe_child_location_capabilities(
        tabs,
        locations,
        Some(metadata_session),
        target.symbol.metadata.program.realm,
    );
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        return capabilities;
    };
    let Some(authorization) = capabilities.authorize_metadata(
        metadata_session,
        DebuggerMetadataCapability::OpaqueSymbolLocation,
    ) else {
        return unavailable_static_metadata_symbol_location();
    };
    if !authorization.permits(
        target.symbol.metadata.program.realm,
        DebuggerMetadataCapability::OpaqueSymbolLocation,
    ) {
        return unavailable_static_metadata_symbol_location();
    }
    let tab_id = match resolve_live_realm(tabs, target.symbol.metadata.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    match locations.debugger_static_metadata_symbol_location(
        tab_id,
        target.symbol.metadata.program.realm.realm_generation,
        JavaScriptPageDebuggerStaticMetadataSymbolLocationTarget {
            program_handle: target.symbol.metadata.program.program_handle,
            program_generation: target.symbol.metadata.program.program_generation,
            metadata_handle: target.symbol.metadata.metadata_handle,
            metadata_generation: target.symbol.metadata.metadata_generation,
            symbol_id: target.symbol.symbol_id,
            source_id: target.source.source_id,
        },
    ) {
        Ok(location) => {
            if location.symbol_id != target.symbol.symbol_id
                || location.source_id != target.source.source_id
            {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget,
                    message: "mismatched debugger static metadata symbol location identity"
                        .to_string(),
                };
            }
            let location = DebuggerStaticMetadataSymbolLocation {
                symbol: target.symbol,
                source: target.source,
                start_byte: location.start_byte,
                end_byte: location.end_byte,
                coordinates: location.coordinates,
            };
            if !location.is_well_formed() {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget,
                    message: "invalid debugger static metadata symbol location".to_string(),
                };
            }
            DebuggerReply::StaticMetadataSymbolLocation(location)
        }
        Err(error) => debugger_program_error(error),
    }
}

/// Discloses only a compiler-verified original byte span for one exact safe
/// point, after the stream independently received its opaque metadata parent
/// and source ID. A guessed source ID or unbound instruction cannot become a
/// nearest-position source-map query.
fn describe_child_static_metadata_safe_point_span(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    target: DebuggerStaticMetadataSafePointSpanTarget,
) -> DebuggerReply {
    if !target.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger static metadata safe-point span target".to_string(),
        };
    }
    let Some(metadata_session) = metadata_session else {
        return unavailable_static_metadata_safe_point_span();
    };
    if !metadata_session.permits(DebuggerMetadataCapability::OpaqueInventory)
        || !metadata_session.permits(DebuggerMetadataCapability::OpaqueSourceInventory)
        || !metadata_session.observed_metadata(target.source.metadata)
        || !metadata_session.observed_source(target.source)
    {
        return unavailable_static_metadata_safe_point_span();
    }
    let realm = target.safe_point.program.realm;
    let capabilities =
        describe_child_location_capabilities(tabs, locations, Some(metadata_session), realm);
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        return capabilities;
    };
    let Some(authorization) = capabilities.authorize_metadata(
        metadata_session,
        DebuggerMetadataCapability::OpaqueSafePointSpan,
    ) else {
        return unavailable_static_metadata_safe_point_span();
    };
    if !authorization.permits(realm, DebuggerMetadataCapability::OpaqueSafePointSpan) {
        return unavailable_static_metadata_safe_point_span();
    }
    let tab_id = match resolve_live_realm(tabs, realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    match locations.debugger_static_metadata_safe_point_span(
        tab_id,
        realm.realm_generation,
        JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget {
            program_handle: target.safe_point.program.program_handle,
            program_generation: target.safe_point.program.program_generation,
            metadata_handle: target.source.metadata.metadata_handle,
            metadata_generation: target.source.metadata.metadata_generation,
            source_id: target.source.source_id,
            code_unit_ordinal: target.safe_point.code_unit_ordinal,
            bytecode_offset: target.safe_point.bytecode_offset,
        },
    ) {
        Ok(span) if span.source_id == target.source.source_id => {
            let result = DebuggerStaticMetadataSafePointSpan {
                safe_point: target.safe_point,
                source: target.source,
                start_byte: span.start_byte,
                end_byte: span.end_byte,
            };
            if !result.is_well_formed() {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget,
                    message: "invalid debugger static metadata safe-point span".to_string(),
                };
            }
            DebuggerReply::StaticMetadataSafePointSpan(result)
        }
        Ok(_) => DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "mismatched debugger static metadata safe-point source identity".to_string(),
        },
        Err(error) => debugger_program_error(error),
    }
}

/// A contract location is resolved only after this stream received the exact
/// parent, contract ID, and source ID and the live child reports the distinct
/// location capability. No source identity, text, plan, or value crosses it.
fn describe_child_static_metadata_contract_location(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    target: DebuggerStaticMetadataContractLocationTarget,
) -> DebuggerReply {
    if !target.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger static metadata contract location target".to_string(),
        };
    }
    let Some(metadata_session) = metadata_session else {
        return unavailable_static_metadata_contract_location();
    };
    if !metadata_session.permits(DebuggerMetadataCapability::OpaqueInventory)
        || !metadata_session.permits(DebuggerMetadataCapability::OpaqueSourceInventory)
        || !metadata_session.permits(DebuggerMetadataCapability::OpaqueContractInventory)
        || !metadata_session.observed_contract(target.contract)
        || !metadata_session.observed_source(target.source)
    {
        return unavailable_static_metadata_contract_location();
    }
    let capabilities = describe_child_location_capabilities(
        tabs,
        locations,
        Some(metadata_session),
        target.contract.metadata.program.realm,
    );
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        return capabilities;
    };
    let Some(authorization) = capabilities.authorize_metadata(
        metadata_session,
        DebuggerMetadataCapability::OpaqueContractLocation,
    ) else {
        return unavailable_static_metadata_contract_location();
    };
    if !authorization.permits(
        target.contract.metadata.program.realm,
        DebuggerMetadataCapability::OpaqueContractLocation,
    ) {
        return unavailable_static_metadata_contract_location();
    }
    let tab_id = match resolve_live_realm(tabs, target.contract.metadata.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    match locations.debugger_static_metadata_contract_location(
        tab_id,
        target.contract.metadata.program.realm.realm_generation,
        JavaScriptPageDebuggerStaticMetadataContractLocationTarget {
            program_handle: target.contract.metadata.program.program_handle,
            program_generation: target.contract.metadata.program.program_generation,
            metadata_handle: target.contract.metadata.metadata_handle,
            metadata_generation: target.contract.metadata.metadata_generation,
            contract_id: target.contract.contract_id,
            source_id: target.source.source_id,
        },
    ) {
        Ok(location) => {
            if location.contract_id != target.contract.contract_id
                || location.source_id != target.source.source_id
            {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget,
                    message: "mismatched debugger static metadata contract location identity"
                        .to_string(),
                };
            }
            let location = DebuggerStaticMetadataContractLocation {
                contract: target.contract,
                source: target.source,
                start_byte: location.start_byte,
                end_byte: location.end_byte,
                coordinates: location.coordinates,
            };
            if !location.is_well_formed() {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget,
                    message: "invalid debugger static metadata contract location".to_string(),
                };
            }
            DebuggerReply::StaticMetadataContractLocation(location)
        }
        Err(error) => debugger_program_error(error),
    }
}

/// Verifies a compiler-recorded symbol/type relation after both opaque IDs
/// crossed this stream's separate inventories. The requested pair and the
/// child reply must agree exactly; no unrequested type or display is emitted.
fn describe_child_static_metadata_symbol_type(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    target: DebuggerStaticMetadataSymbolType,
) -> DebuggerReply {
    if !target.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger static metadata symbol type target".to_string(),
        };
    }
    let Some(metadata_session) = metadata_session else {
        return unavailable_static_metadata_symbol_type();
    };
    if !metadata_session.permits(DebuggerMetadataCapability::OpaqueInventory)
        || !metadata_session.permits(DebuggerMetadataCapability::OpaqueTypeInventory)
        || !metadata_session.permits(DebuggerMetadataCapability::OpaqueSymbolInventory)
        || !metadata_session.observed_symbol(target.symbol)
        || !metadata_session.observed_type(target.static_type)
    {
        return unavailable_static_metadata_symbol_type();
    }
    let capabilities = describe_child_location_capabilities(
        tabs,
        locations,
        Some(metadata_session),
        target.symbol.metadata.program.realm,
    );
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        return capabilities;
    };
    let Some(authorization) = capabilities.authorize_metadata(
        metadata_session,
        DebuggerMetadataCapability::OpaqueSymbolType,
    ) else {
        return unavailable_static_metadata_symbol_type();
    };
    if !authorization.permits(
        target.symbol.metadata.program.realm,
        DebuggerMetadataCapability::OpaqueSymbolType,
    ) {
        return unavailable_static_metadata_symbol_type();
    }
    let tab_id = match resolve_live_realm(tabs, target.symbol.metadata.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    match locations.debugger_static_metadata_symbol_type(
        tab_id,
        target.symbol.metadata.program.realm.realm_generation,
        JavaScriptPageDebuggerStaticMetadataSymbolTypeTarget {
            program_handle: target.symbol.metadata.program.program_handle,
            program_generation: target.symbol.metadata.program.program_generation,
            metadata_handle: target.symbol.metadata.metadata_handle,
            metadata_generation: target.symbol.metadata.metadata_generation,
            symbol_id: target.symbol.symbol_id,
            type_id: target.static_type.type_id,
        },
    ) {
        Ok(symbol_type)
            if symbol_type.symbol_id == target.symbol.symbol_id
                && symbol_type.type_id == target.static_type.type_id =>
        {
            DebuggerReply::StaticMetadataSymbolType(target)
        }
        Ok(_) => DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "mismatched debugger static metadata symbol type identity".to_string(),
        },
        Err(error) => debugger_program_error(error),
    }
}

/// Verifies one reifiable symbol/contract relation after both opaque IDs
/// crossed this stream's separate inventories. The child may only confirm
/// the exact requested pair; it cannot introduce a contract ID or plan.
fn describe_child_static_metadata_symbol_contract(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    target: DebuggerStaticMetadataSymbolContract,
) -> DebuggerReply {
    if !target.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger static metadata symbol contract target".to_string(),
        };
    }
    let Some(metadata_session) = metadata_session else {
        return unavailable_static_metadata_symbol_contract();
    };
    if !metadata_session.permits(DebuggerMetadataCapability::OpaqueInventory)
        || !metadata_session.permits(DebuggerMetadataCapability::OpaqueSymbolInventory)
        || !metadata_session.permits(DebuggerMetadataCapability::OpaqueContractInventory)
        || !metadata_session.observed_symbol(target.symbol)
        || !metadata_session.observed_contract(target.contract)
    {
        return unavailable_static_metadata_symbol_contract();
    }
    let capabilities = describe_child_location_capabilities(
        tabs,
        locations,
        Some(metadata_session),
        target.symbol.metadata.program.realm,
    );
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        return capabilities;
    };
    let Some(authorization) = capabilities.authorize_metadata(
        metadata_session,
        DebuggerMetadataCapability::OpaqueSymbolContract,
    ) else {
        return unavailable_static_metadata_symbol_contract();
    };
    if !authorization.permits(
        target.symbol.metadata.program.realm,
        DebuggerMetadataCapability::OpaqueSymbolContract,
    ) {
        return unavailable_static_metadata_symbol_contract();
    }
    let tab_id = match resolve_live_realm(tabs, target.symbol.metadata.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    match locations.debugger_static_metadata_symbol_contract(
        tab_id,
        target.symbol.metadata.program.realm.realm_generation,
        JavaScriptPageDebuggerStaticMetadataSymbolContractTarget {
            program_handle: target.symbol.metadata.program.program_handle,
            program_generation: target.symbol.metadata.program.program_generation,
            metadata_handle: target.symbol.metadata.metadata_handle,
            metadata_generation: target.symbol.metadata.metadata_generation,
            symbol_id: target.symbol.symbol_id,
            contract_id: target.contract.contract_id,
        },
    ) {
        Ok(symbol_contract)
            if symbol_contract.symbol_id == target.symbol.symbol_id
                && symbol_contract.contract_id == target.contract.contract_id =>
        {
            DebuggerReply::StaticMetadataSymbolContract(target)
        }
        Ok(_) => DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "mismatched debugger static metadata symbol contract identity".to_string(),
        },
        Err(error) => debugger_program_error(error),
    }
}

fn describe_child_static_metadata_source_provenance(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    source: DebuggerStaticMetadataSourceId,
) -> DebuggerReply {
    if !source.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger static metadata source provenance target".to_string(),
        };
    }
    let Some(metadata_session) = metadata_session else {
        return unavailable_static_metadata_source_provenance();
    };
    // Provenance is deliberately dependent on source inventory: callers must
    // present an exact source ID under an opaque parent, not invent a source
    // lookup key or obtain a standalone content oracle.
    if !metadata_session.permits(DebuggerMetadataCapability::OpaqueInventory)
        || !metadata_session.permits(DebuggerMetadataCapability::OpaqueSourceInventory)
        || !metadata_session.observed_source(source)
    {
        return unavailable_static_metadata_source_provenance();
    }
    let capabilities = describe_child_location_capabilities(
        tabs,
        locations,
        Some(metadata_session),
        source.metadata.program.realm,
    );
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        return capabilities;
    };
    let Some(authorization) = capabilities.authorize_metadata(
        metadata_session,
        DebuggerMetadataCapability::OpaqueSourceProvenance,
    ) else {
        return unavailable_static_metadata_source_provenance();
    };
    if !authorization.permits(
        source.metadata.program.realm,
        DebuggerMetadataCapability::OpaqueSourceProvenance,
    ) {
        return unavailable_static_metadata_source_provenance();
    }
    let tab_id = match resolve_live_realm(tabs, source.metadata.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    match locations.debugger_static_metadata_source_provenance(
        tab_id,
        source.metadata.program.realm.realm_generation,
        JavaScriptPageDebuggerStaticMetadataSourceTarget {
            program_handle: source.metadata.program.program_handle,
            program_generation: source.metadata.program.program_generation,
            metadata_handle: source.metadata.metadata_handle,
            metadata_generation: source.metadata.metadata_generation,
            source_id: source.source_id,
        },
    ) {
        Ok(provenance) => {
            let provenance = DebuggerStaticMetadataSourceProvenance {
                source,
                module: provenance.module,
                content_hash: provenance.content_hash,
            };
            if !provenance.is_well_formed() {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget,
                    message: "invalid debugger static metadata source provenance".to_string(),
                };
            }
            DebuggerReply::StaticMetadataSourceProvenance(provenance)
        }
        Err(error) => debugger_program_error(error),
    }
}

fn list_child_programs(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    realm: DebuggerPageRealm,
) -> DebuggerReply {
    let tab_id = match resolve_live_realm(tabs, realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    if !locations.debugger_has_live_realm(tab_id, realm.realm_generation) {
        return unavailable_program_locations();
    }
    match locations.debugger_programs(tab_id, realm.realm_generation) {
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

fn list_child_safe_points(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
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
        Err(reply) => return *reply,
    };
    if !locations.debugger_has_live_realm(tab_id, program.realm.realm_generation) {
        return unavailable_program_locations();
    }
    match locations.debugger_safe_points(
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

fn validate_child_safe_point(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
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
        Err(reply) => return *reply,
    };
    if !locations.debugger_has_live_realm(tab_id, safe_point.program.realm.realm_generation) {
        return unavailable_program_locations();
    }
    match locations.validate_debugger_safe_point(
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

fn set_child_breakpoint(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    safe_point: DebuggerSafePoint,
) -> DebuggerReply {
    if !safe_point.is_well_formed() {
        return invalid_safe_point_target();
    }
    let tab_id = match resolve_live_realm(tabs, safe_point.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    if !locations.debugger_has_live_realm(tab_id, safe_point.program.realm.realm_generation)
        || !locations.debugger_breakpoint_configuration_available()
    {
        return unavailable_breakpoint_configuration();
    }
    match locations.set_debugger_breakpoint(
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

fn list_child_breakpoints(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    realm: DebuggerPageRealm,
) -> DebuggerReply {
    let tab_id = match resolve_live_realm(tabs, realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    if !locations.debugger_has_live_realm(tab_id, realm.realm_generation)
        || !locations.debugger_breakpoint_configuration_available()
    {
        return unavailable_breakpoint_configuration();
    }
    match locations.debugger_breakpoints(tab_id, realm.realm_generation) {
        Ok(breakpoints) => {
            if breakpoints.len() > locations.max_debugger_breakpoints_per_realm() {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::ResourceLimit,
                    message: "too many native breakpoint records for one page realm".to_string(),
                };
            }
            DebuggerReply::Breakpoints(
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
            )
        }
        Err(error) => debugger_program_error(error),
    }
}

fn clear_child_breakpoint(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    safe_point: DebuggerSafePoint,
) -> DebuggerReply {
    if !safe_point.is_well_formed() {
        return invalid_safe_point_target();
    }
    let tab_id = match resolve_live_realm(tabs, safe_point.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    if !locations.debugger_has_live_realm(tab_id, safe_point.program.realm.realm_generation)
        || !locations.debugger_breakpoint_configuration_available()
    {
        return unavailable_breakpoint_configuration();
    }
    match locations.clear_debugger_breakpoint(
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

/// Routes only the one-shot child root-classic continuation arm. The public
/// tuple is resolved by core before the child-private mapping can be used;
/// child code units and every generic VM interruption path stay unavailable.
fn arm_child_root_safe_point_breakpoint(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    safe_point: DebuggerSafePoint,
) -> DebuggerReply {
    if !safe_point.is_well_formed() || safe_point.code_unit_ordinal != 0 {
        return invalid_safe_point_target();
    }
    let tab_id = match resolve_live_realm(tabs, safe_point.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    if !locations.debugger_has_live_realm(tab_id, safe_point.program.realm.realm_generation)
        || !locations.debugger_execution_control_available()
    {
        return unavailable_execution_control();
    }
    match locations.arm_debugger_root_safe_point_breakpoint(
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

fn child_execution_state(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
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
        Err(reply) => return *reply,
    };
    if !locations.debugger_has_live_realm(tab_id, program.realm.realm_generation)
        || !locations.debugger_execution_control_available()
    {
        return unavailable_execution_control();
    }
    match locations.debugger_execution_state(
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

fn resume_child_execution(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
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
        Err(reply) => return *reply,
    };
    if !locations.debugger_has_live_realm(tab_id, program.realm.realm_generation)
        || !locations.debugger_execution_control_available()
    {
        return unavailable_execution_control();
    }
    match locations.resume_debugger_execution(
        tab_id,
        program.realm.realm_generation,
        program.program_handle,
        program.program_generation,
    ) {
        Ok(()) => DebuggerReply::ExecutionResumed { program },
        Err(error) => debugger_program_error(error),
    }
}

fn step_child_root_instruction(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
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
        Err(reply) => return *reply,
    };
    if !locations.debugger_has_live_realm(tab_id, program.realm.realm_generation)
        || !locations.debugger_execution_control_available()
        || !locations.debugger_stepping_available()
    {
        return unavailable_stepping();
    }
    match locations.step_debugger_root_instruction(
        tab_id,
        program.realm.realm_generation,
        program.program_handle,
        program.program_generation,
    ) {
        Ok(()) => DebuggerReply::ExecutionStepRequested { program },
        Err(error) => debugger_program_error(error),
    }
}

fn describe_capabilities(
    tabs: &TabManager,
    javascript_executor: Option<&JavaScriptPageExecutor>,
    realm: DebuggerPageRealm,
) -> DebuggerReply {
    if let Err(reply) = resolve_live_realm(tabs, realm) {
        return *reply;
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
        reports: capability_reports(DebuggerCapabilityAvailability {
            program_locations_available,
            breakpoint_configuration_available: program_locations_available,
            entry_execution_control_available,
            stepping_available: entry_execution_control_available,
            static_metadata_inventory_available: false,
            static_metadata_summary_available: false,
            static_metadata_lowering_summary_available: false,
            static_metadata_source_inventory_available: false,
            static_metadata_source_provenance_available: false,
            static_metadata_type_inventory_available: false,
            static_metadata_type_display_available: false,
            static_metadata_symbol_inventory_available: false,
            static_metadata_contract_inventory_available: false,
            static_metadata_contract_display_available: false,
            static_metadata_contract_validation_available: false,
            static_metadata_symbol_display_available: false,
            static_metadata_symbol_location_available: false,
            static_metadata_safe_point_span_available: false,
            static_metadata_contract_location_available: false,
            static_metadata_symbol_type_available: false,
            static_metadata_symbol_contract_available: false,
        }),
        max_stack_frames: MAX_STACK_FRAMES,
        max_scope_bindings: MAX_SCOPE_BINDINGS,
        max_value_preview_bytes: MAX_VALUE_PREVIEW_BYTES,
        max_safe_points_per_program: u32::try_from(max_safe_points_per_program)
            .expect("native debugger safe-point reply cap fits the wire type"),
        max_breakpoints_per_realm: u32::try_from(max_breakpoints_per_realm)
            .expect("native debugger breakpoint reply cap fits the wire type"),
    })
}

/// Resolves a realm for one debugger operation without copying a large public
/// wire reply through every local `Result` error path.
fn resolve_live_realm(
    tabs: &TabManager,
    realm: DebuggerPageRealm,
) -> Result<TabId, Box<DebuggerReply>> {
    if !realm.is_well_formed() || realm.browser_context_id != DEFAULT_BROWSER_CONTEXT_ID {
        return Err(Box::new(DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger realm target".to_string(),
        }));
    }
    let tab_id = TabId::from_u64(realm.tab_id);
    let Some(page) = tabs.get(tab_id) else {
        return Err(Box::new(DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "unknown debugger tab".to_string(),
        }));
    };
    if page.document_generation() != realm.realm_generation {
        return Err(Box::new(DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm,
            message: "stale debugger realm generation".to_string(),
        }));
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
        Err(reply) => return *reply,
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
        Err(reply) => return *reply,
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
        Err(reply) => return *reply,
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
        Err(reply) => return *reply,
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
        Err(reply) => return *reply,
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
        Err(reply) => return *reply,
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
        Err(reply) => return *reply,
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
        Err(reply) => return *reply,
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
        Err(reply) => return *reply,
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
        Err(reply) => return *reply,
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

fn step_root_instruction(
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
        Err(reply) => return *reply,
    };
    let Some(executor) = javascript_executor else {
        return unavailable_stepping();
    };
    if !executor.debugger_has_live_realm(tab_id, program.realm.realm_generation)
        || !executor.debugger_execution_control_available()
    {
        return unavailable_stepping();
    }
    match executor.step_debugger_root_instruction(
        tab_id,
        program.realm.realm_generation,
        program.program_handle,
        program.program_generation,
    ) {
        Ok(()) => DebuggerReply::ExecutionStepRequested { program },
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

fn unavailable_static_metadata_inventory() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "opaque debugger static metadata inventory is not authorized for this session and live realm"
            .to_string(),
    }
}

fn unavailable_static_metadata_summary() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "bounded debugger static metadata summaries are not authorized for this session and live realm"
            .to_string(),
    }
}

fn unavailable_static_metadata_source_inventory() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "opaque debugger static metadata source inventory is not authorized for this session and live realm"
            .to_string(),
    }
}

fn unavailable_static_metadata_type_inventory() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "opaque debugger static metadata type inventory is not authorized for this session and live realm"
            .to_string(),
    }
}

fn unavailable_static_metadata_type_display() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "opaque debugger static metadata type display is not authorized for this session and live realm"
            .to_string(),
    }
}

fn unavailable_static_metadata_symbol_inventory() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "opaque debugger static metadata symbol inventory is not authorized for this session and live realm"
            .to_string(),
    }
}

fn unavailable_static_metadata_contract_inventory() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "opaque debugger static metadata contract inventory is not authorized for this session and live realm"
            .to_string(),
    }
}

fn unavailable_static_metadata_contract_display() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "opaque debugger static metadata contract display is not authorized for this session and live realm"
            .to_string(),
    }
}

fn unavailable_static_metadata_contract_validation() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "opaque debugger static metadata contract validation is not authorized for this session and live realm"
            .to_string(),
    }
}

fn unavailable_static_metadata_lowering_summary() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "opaque debugger static metadata lowering summary is not authorized for this session and live realm"
            .to_string(),
    }
}

fn unavailable_static_metadata_symbol_display() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "opaque debugger static metadata symbol display is not authorized for this session and live realm"
            .to_string(),
    }
}

fn unavailable_static_metadata_symbol_location() -> DebuggerReply {
    DebuggerReply::Unsupported {
        operation: "describe static metadata symbol location".to_string(),
        reason: "static metadata symbol locations require an explicitly negotiated owner grant, prior symbol/source receipts, and a live BlueTS child program".to_string(),
    }
}

fn unavailable_static_metadata_safe_point_span() -> DebuggerReply {
    DebuggerReply::Unsupported {
        operation: "describe static metadata safe-point span".to_string(),
        reason: "exact BlueTS safe-point spans require a separate owner/client grant, a prior same-stream source-ID receipt, and a live child attachment".to_string(),
    }
}

fn unavailable_static_metadata_contract_location() -> DebuggerReply {
    DebuggerReply::Unsupported {
        operation: "describe static metadata contract location".to_string(),
        reason: "static metadata contract locations require an explicitly negotiated owner grant, prior contract/source receipts, and a live BlueTS child program".to_string(),
    }
}

fn unavailable_static_metadata_symbol_type() -> DebuggerReply {
    DebuggerReply::Unsupported {
        operation: "describe static metadata symbol type".to_string(),
        reason: "static metadata symbol types require an explicitly negotiated owner grant, prior symbol/type receipts, and a live BlueTS child program".to_string(),
    }
}

fn unavailable_static_metadata_symbol_contract() -> DebuggerReply {
    DebuggerReply::Unsupported {
        operation: "describe static metadata symbol contract".to_string(),
        reason: "static metadata symbol contracts require an explicitly negotiated owner grant, prior symbol/contract receipts, and a live BlueTS child program".to_string(),
    }
}

fn unavailable_static_metadata_source_provenance() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "debugger static metadata source provenance is not authorized for this session and live realm"
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

fn unavailable_stepping() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "native debugger root stepping is not installed for this page-host route"
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
        JavaScriptPageDebuggerExecutionState::Stepping => DebuggerExecutionState::Stepping,
        JavaScriptPageDebuggerExecutionState::Resuming => DebuggerExecutionState::Resuming,
        JavaScriptPageDebuggerExecutionState::Completed => DebuggerExecutionState::Completed,
    }
}

struct DebuggerCapabilityAvailability {
    program_locations_available: bool,
    breakpoint_configuration_available: bool,
    entry_execution_control_available: bool,
    stepping_available: bool,
    static_metadata_inventory_available: bool,
    static_metadata_summary_available: bool,
    static_metadata_lowering_summary_available: bool,
    static_metadata_source_inventory_available: bool,
    static_metadata_source_provenance_available: bool,
    static_metadata_type_inventory_available: bool,
    static_metadata_type_display_available: bool,
    static_metadata_symbol_inventory_available: bool,
    static_metadata_contract_inventory_available: bool,
    static_metadata_contract_display_available: bool,
    static_metadata_contract_validation_available: bool,
    static_metadata_symbol_display_available: bool,
    static_metadata_symbol_location_available: bool,
    static_metadata_safe_point_span_available: bool,
    static_metadata_contract_location_available: bool,
    static_metadata_symbol_type_available: bool,
    static_metadata_symbol_contract_available: bool,
}

fn capability_reports(
    DebuggerCapabilityAvailability {
        program_locations_available,
        breakpoint_configuration_available,
        entry_execution_control_available,
        stepping_available,
        static_metadata_inventory_available,
        static_metadata_summary_available,
        static_metadata_lowering_summary_available,
        static_metadata_source_inventory_available,
        static_metadata_source_provenance_available,
        static_metadata_type_inventory_available,
        static_metadata_type_display_available,
        static_metadata_symbol_inventory_available,
        static_metadata_contract_inventory_available,
        static_metadata_contract_display_available,
        static_metadata_contract_validation_available,
        static_metadata_symbol_display_available,
        static_metadata_symbol_location_available,
        static_metadata_safe_point_span_available,
        static_metadata_contract_location_available,
        static_metadata_symbol_type_available,
        static_metadata_symbol_contract_available,
    }: DebuggerCapabilityAvailability,
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
            if breakpoint_configuration_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if breakpoint_configuration_available {
                "bounded exact breakpoint configuration is installed; it does not interrupt execution"
            } else {
                "native breakpoint configuration is not installed for this page-host route"
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
            if stepping_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if stepping_available {
                "one classic-root instruction step retains the same BlueJS continuation; nested frames and modules remain unavailable"
            } else {
                "native stepping is not installed for this page-host route"
            },
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
        (
            DebuggerCapability::StaticMetadataInventory,
            if static_metadata_inventory_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if static_metadata_inventory_available {
                "bounded opaque static-metadata handle inventory is installed; metadata remains unreadable"
            } else {
                "static metadata inventory requires an explicitly negotiated session grant and a live BlueTS child program"
            },
        ),
        (
            DebuggerCapability::StaticMetadataSummary,
            if static_metadata_summary_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if static_metadata_summary_available {
                "bounded source-free static-metadata summaries are installed; records remain unreadable"
            } else {
                "static metadata summaries require explicit inventory and summary session grants plus a live BlueTS child program"
            },
        ),
        (
            DebuggerCapability::StaticMetadataLoweringSummary,
            if static_metadata_lowering_summary_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if static_metadata_lowering_summary_available {
                "verified direct-lowering-map ABI and aggregate evidence is installed; map entries remain unreadable"
            } else {
                "static metadata lowering summaries require explicit inventory and lowering-summary session grants plus a live BlueTS child program"
            },
        ),
        (
            DebuggerCapability::StaticMetadataSourceInventory,
            if static_metadata_source_inventory_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if static_metadata_source_inventory_available {
                "bounded opaque static-metadata source identities are installed; source details remain unreadable"
            } else {
                "static metadata source identities require explicit inventory and source-inventory session grants plus a live BlueTS child program"
            },
        ),
        (
            DebuggerCapability::StaticMetadataSourceProvenance,
            if static_metadata_source_provenance_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if static_metadata_source_provenance_available {
                "owner-authorized source-free module identity and SHA-256 provenance are installed"
            } else {
                "source provenance requires explicit inventory, source-inventory, and provenance session grants plus a live BlueTS child program"
            },
        ),
        (
            DebuggerCapability::StaticMetadataTypeInventory,
            if static_metadata_type_inventory_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if static_metadata_type_inventory_available {
                "bounded opaque static-metadata type identities are installed; type displays remain unreadable"
            } else {
                "static metadata type identities require explicit inventory and type-inventory session grants plus a live BlueTS child program"
            },
        ),
        (
            DebuggerCapability::StaticMetadataTypeDisplay,
            if static_metadata_type_display_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if static_metadata_type_display_available {
                "bounded compiler-produced static type displays are installed for prior type-ID receipts"
            } else {
                "static metadata type displays require explicit inventory, type-inventory, and type-display session grants plus a live BlueTS child program"
            },
        ),
        (
            DebuggerCapability::StaticMetadataSymbolInventory,
            if static_metadata_symbol_inventory_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if static_metadata_symbol_inventory_available {
                "bounded opaque static-metadata symbol identities are installed; symbol records remain unreadable"
            } else {
                "static metadata symbol identities require explicit inventory and symbol-inventory session grants plus a live BlueTS child program"
            },
        ),
        (
            DebuggerCapability::StaticMetadataContractInventory,
            if static_metadata_contract_inventory_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if static_metadata_contract_inventory_available {
                "bounded opaque static-metadata contract identities are installed; contract records remain unreadable"
            } else {
                "static metadata contract identities require explicit inventory and contract-inventory session grants plus a live BlueTS child program"
            },
        ),
        (
            DebuggerCapability::StaticMetadataContractDisplay,
            if static_metadata_contract_display_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if static_metadata_contract_display_available {
                "bounded compiler-produced static contract displays are installed for prior contract-ID receipts"
            } else {
                "static metadata contract displays require explicit inventory, contract-inventory, and contract-display session grants plus a live BlueTS child program"
            },
        ),
        (
            DebuggerCapability::StaticMetadataContractValidation,
            if static_metadata_contract_validation_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if static_metadata_contract_validation_available {
                "bounded data-only static contract validation is installed for prior contract-ID receipts; it returns only a boolean"
            } else {
                "static metadata contract validation requires explicit inventory, contract-inventory, and contract-validation session grants plus a live BlueTS child program"
            },
        ),
        (
            DebuggerCapability::StaticMetadataSymbolDisplay,
            if static_metadata_symbol_display_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if static_metadata_symbol_display_available {
                "bounded compiler-produced static symbol displays are installed for prior symbol-ID receipts"
            } else {
                "static metadata symbol displays require explicit inventory, symbol-inventory, and symbol-display session grants plus a live BlueTS child program"
            },
        ),
        (
            DebuggerCapability::StaticMetadataSymbolLocation,
            if static_metadata_symbol_location_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if static_metadata_symbol_location_available {
                "bounded source-text-free static symbol locations are installed for prior symbol and source-ID receipts"
            } else {
                "static metadata symbol locations require explicit inventory, source-inventory, symbol-inventory, and symbol-location session grants plus a live BlueTS child program"
            },
        ),
        (
            DebuggerCapability::StaticMetadataSafePointSpan,
            if static_metadata_safe_point_span_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if static_metadata_safe_point_span_available {
                "exact BlueTS safe-point spans are installed for prior metadata and source-ID receipts"
            } else {
                "exact BlueTS safe-point spans require separate inventory, source-inventory, and span grants plus a live child attachment"
            },
        ),
        (
            DebuggerCapability::StaticMetadataContractLocation,
            if static_metadata_contract_location_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if static_metadata_contract_location_available {
                "bounded source-text-free static contract locations are installed for prior contract and source-ID receipts"
            } else {
                "static metadata contract locations require explicit inventory, source-inventory, contract-inventory, and contract-location session grants plus a live BlueTS child program"
            },
        ),
        (
            DebuggerCapability::StaticMetadataSymbolType,
            if static_metadata_symbol_type_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if static_metadata_symbol_type_available {
                "compiler-verified symbol-to-type relations are installed for prior symbol and type-ID receipts"
            } else {
                "static metadata symbol types require explicit inventory, symbol-inventory, type-inventory, and symbol-type session grants plus a live BlueTS child program"
            },
        ),
        (
            DebuggerCapability::StaticMetadataSymbolContract,
            if static_metadata_symbol_contract_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if static_metadata_symbol_contract_available {
                "compiler-verified symbol-to-contract relations are installed for prior symbol and contract-ID receipts"
            } else {
                "static metadata symbol contracts require explicit inventory, symbol-inventory, contract-inventory, and symbol-contract session grants plus a live BlueTS child program"
            },
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
    use blueice_ipc::debugger::DebuggerSourceCoordinates;

    struct MetadataLocations {
        malformed_summary: bool,
        malformed_provenance: bool,
        malformed_lowering_summary: bool,
        mismatched_symbol_display: bool,
        mismatched_contract_display: bool,
        mismatched_contract_validation: bool,
    }

    impl PageJavaScriptDebuggerLocations for MetadataLocations {
        fn debugger_has_live_realm(&mut self, _tab_id: TabId, _document_generation: u64) -> bool {
            true
        }

        fn max_debugger_safe_points_per_program(&self) -> usize {
            1
        }

        fn debugger_static_metadata_inventory_available(&self) -> bool {
            true
        }

        fn debugger_static_metadata_summary_available(&self) -> bool {
            true
        }

        fn debugger_static_metadata_lowering_summary_available(&self) -> bool {
            true
        }

        fn debugger_static_metadata_source_inventory_available(&self) -> bool {
            true
        }

        fn debugger_static_metadata_source_provenance_available(&self) -> bool {
            true
        }

        fn debugger_static_metadata_symbol_inventory_available(&self) -> bool {
            true
        }

        fn debugger_static_metadata_symbol_display_available(&self) -> bool {
            true
        }

        fn debugger_static_metadata_symbol_location_available(&self) -> bool {
            true
        }

        fn debugger_static_metadata_safe_point_span_available(&self) -> bool {
            true
        }

        fn debugger_static_metadata_contract_location_available(&self) -> bool {
            true
        }

        fn debugger_static_metadata_type_inventory_available(&self) -> bool {
            true
        }

        fn debugger_static_metadata_symbol_type_available(&self) -> bool {
            true
        }

        fn debugger_static_metadata_symbol_contract_available(&self) -> bool {
            true
        }

        fn debugger_static_metadata_contract_inventory_available(&self) -> bool {
            true
        }

        fn debugger_static_metadata_contract_display_available(&self) -> bool {
            true
        }

        fn debugger_static_metadata_contract_validation_available(&self) -> bool {
            true
        }

        fn debugger_programs(
            &mut self,
            _tab_id: TabId,
            _document_generation: u64,
        ) -> Result<
            Vec<crate::script::javascript::JavaScriptPageDebuggerProgram>,
            JavaScriptPageDebuggerError,
        > {
            Ok(Vec::new())
        }

        fn debugger_static_metadata(
            &mut self,
            _tab_id: TabId,
            _document_generation: u64,
            _program_handle: u64,
            _program_generation: u64,
        ) -> Result<
            Vec<crate::script::javascript::JavaScriptPageDebuggerStaticMetadata>,
            JavaScriptPageDebuggerError,
        > {
            Ok(vec![
                crate::script::javascript::JavaScriptPageDebuggerStaticMetadata {
                    metadata_handle: 41,
                    metadata_generation: 9,
                },
            ])
        }

        fn debugger_static_metadata_summary(
            &mut self,
            _tab_id: TabId,
            _document_generation: u64,
            _program_handle: u64,
            _program_generation: u64,
            metadata_handle: u64,
            metadata_generation: u64,
        ) -> Result<
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSummary,
            JavaScriptPageDebuggerError,
        > {
            if metadata_handle != 41 || metadata_generation != 9 {
                return Err(JavaScriptPageDebuggerError::UnknownProgram);
            }
            Ok(
                crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSummary {
                    language_version: if self.malformed_summary {
                        "x".repeat(
                            blueice_ipc::debugger::DEBUGGER_STATIC_METADATA_LANGUAGE_VERSION_MAX_BYTES
                                + 1,
                        )
                    } else {
                        "blue-ts-0.1".to_string()
                    },
                    compiler_options_hash: "0123456789abcdef".to_string(),
                    source_count: 1,
                    type_count: 2,
                    symbol_count: 3,
                    contract_count: 4,
                },
            )
        }

        fn debugger_static_metadata_lowering_summary(
            &mut self,
            _tab_id: TabId,
            _document_generation: u64,
            _program_handle: u64,
            _program_generation: u64,
            metadata_handle: u64,
            metadata_generation: u64,
        ) -> Result<
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataLoweringSummary,
            JavaScriptPageDebuggerError,
        > {
            if metadata_handle != 41 || metadata_generation != 9 {
                return Err(JavaScriptPageDebuggerError::UnknownProgram);
            }
            Ok(
                crate::script::javascript::JavaScriptPageDebuggerStaticMetadataLoweringSummary {
                    safe_point_map_abi: if self.malformed_lowering_summary {
                        "unexpected-child-label".to_string()
                    } else {
                        blueice_ipc::debugger::DEBUGGER_STATIC_METADATA_SAFE_POINT_MAP_ABI_V1
                            .to_string()
                    },
                    program_abi: blueice_ipc::debugger::DEBUGGER_STATIC_METADATA_PROGRAM_ABI_V1
                        .to_string(),
                    source_set_hash: "bts-source-set-0123456789abcdef".to_string(),
                    bound_safe_point_count: 1,
                },
            )
        }

        fn debugger_static_metadata_sources(
            &mut self,
            _tab_id: TabId,
            _document_generation: u64,
            _program_handle: u64,
            _program_generation: u64,
            metadata_handle: u64,
            metadata_generation: u64,
        ) -> Result<
            Vec<crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSourceId>,
            JavaScriptPageDebuggerError,
        > {
            if metadata_handle != 41 || metadata_generation != 9 {
                return Err(JavaScriptPageDebuggerError::UnknownProgram);
            }
            Ok(vec![
                crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSourceId {
                    source_id: 0,
                },
            ])
        }

        fn debugger_static_metadata_source_provenance(
            &mut self,
            _tab_id: TabId,
            _document_generation: u64,
            target: crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSourceTarget,
        ) -> Result<
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSourceProvenance,
            JavaScriptPageDebuggerError,
        > {
            if target.metadata_handle != 41
                || target.metadata_generation != 9
                || target.source_id != 0
            {
                return Err(JavaScriptPageDebuggerError::UnknownProgram);
            }
            Ok(
                crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSourceProvenance {
                    source_id: target.source_id,
                    module: if self.malformed_provenance {
                        "file:///private/main.ts".to_string()
                    } else {
                        "page:///main.ts".to_string()
                    },
                    content_hash: "bts-sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".to_string(),
                },
            )
        }

        fn debugger_static_metadata_symbols(
            &mut self,
            _tab_id: TabId,
            _document_generation: u64,
            _program_handle: u64,
            _program_generation: u64,
            metadata_handle: u64,
            metadata_generation: u64,
        ) -> Result<
            Vec<crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSymbolId>,
            JavaScriptPageDebuggerError,
        > {
            if metadata_handle != 41 || metadata_generation != 9 {
                return Err(JavaScriptPageDebuggerError::UnknownProgram);
            }
            Ok(vec![
                crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSymbolId {
                    symbol_id: 0,
                },
                crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSymbolId {
                    symbol_id: 1,
                },
                crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSymbolId {
                    symbol_id: 2,
                },
            ])
        }

        fn debugger_static_metadata_types(
            &mut self,
            _tab_id: TabId,
            _document_generation: u64,
            _program_handle: u64,
            _program_generation: u64,
            metadata_handle: u64,
            metadata_generation: u64,
        ) -> Result<
            Vec<crate::script::javascript::JavaScriptPageDebuggerStaticMetadataTypeId>,
            JavaScriptPageDebuggerError,
        > {
            if metadata_handle != 41 || metadata_generation != 9 {
                return Err(JavaScriptPageDebuggerError::UnknownProgram);
            }
            Ok(vec![
                crate::script::javascript::JavaScriptPageDebuggerStaticMetadataTypeId {
                    type_id: 0,
                },
                crate::script::javascript::JavaScriptPageDebuggerStaticMetadataTypeId {
                    type_id: 1,
                },
            ])
        }

        fn debugger_static_metadata_symbol_display(
            &mut self,
            _tab_id: TabId,
            _document_generation: u64,
            target: crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSymbolTarget,
        ) -> Result<
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSymbolDisplay,
            JavaScriptPageDebuggerError,
        > {
            if target.metadata_handle != 41
                || target.metadata_generation != 9
                || target.symbol_id != 0
            {
                return Err(JavaScriptPageDebuggerError::UnknownProgram);
            }
            Ok(
                crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSymbolDisplay {
                    symbol_id: if self.mismatched_symbol_display {
                        target.symbol_id + 1
                    } else {
                        target.symbol_id
                    },
                    display: "ProjectControlledName".to_string(),
                    kind: blueice_ipc::debugger::DebuggerStaticMetadataSymbolKind::Interface,
                    exported: true,
                },
            )
        }

        fn debugger_static_metadata_symbol_location(
            &mut self,
            _tab_id: TabId,
            _document_generation: u64,
            target: crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSymbolLocationTarget,
        ) -> Result<
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSymbolLocation,
            JavaScriptPageDebuggerError,
        > {
            if target.metadata_handle != 41
                || target.metadata_generation != 9
                || target.symbol_id != 0
                || target.source_id != 0
            {
                return Err(JavaScriptPageDebuggerError::UnknownProgram);
            }
            Ok(
                crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSymbolLocation {
                    symbol_id: target.symbol_id,
                    source_id: target.source_id,
                    start_byte: 6,
                    end_byte: 31,
                    coordinates: DebuggerSourceCoordinates {
                        start_line: 0,
                        start_column_utf16: 6,
                        end_line: 0,
                        end_column_utf16: 31,
                    },
                },
            )
        }

        fn debugger_static_metadata_safe_point_span(
            &mut self,
            _tab_id: TabId,
            _document_generation: u64,
            target: crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget,
        ) -> Result<
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSafePointSpan,
            JavaScriptPageDebuggerError,
        > {
            if target.program_handle != 7
                || target.program_generation != 3
                || target.metadata_handle != 41
                || target.metadata_generation != 9
                || target.source_id != 0
                || target.code_unit_ordinal != 0
                || target.bytecode_offset != 4
            {
                return Err(JavaScriptPageDebuggerError::UnknownProgram);
            }
            Ok(
                crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSafePointSpan {
                    source_id: 0,
                    start_byte: 6,
                    end_byte: 31,
                },
            )
        }

        fn debugger_static_metadata_contract_location(
            &mut self,
            _tab_id: TabId,
            _document_generation: u64,
            target: crate::script::javascript::JavaScriptPageDebuggerStaticMetadataContractLocationTarget,
        ) -> Result<
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataContractLocation,
            JavaScriptPageDebuggerError,
        > {
            if target.metadata_handle != 41
                || target.metadata_generation != 9
                || target.contract_id != 0
                || target.source_id != 0
            {
                return Err(JavaScriptPageDebuggerError::UnknownProgram);
            }
            Ok(
                crate::script::javascript::JavaScriptPageDebuggerStaticMetadataContractLocation {
                    contract_id: target.contract_id,
                    source_id: target.source_id,
                    start_byte: 6,
                    end_byte: 31,
                    coordinates: DebuggerSourceCoordinates {
                        start_line: 0,
                        start_column_utf16: 6,
                        end_line: 0,
                        end_column_utf16: 31,
                    },
                },
            )
        }

        fn debugger_static_metadata_symbol_type(
            &mut self,
            _tab_id: TabId,
            _document_generation: u64,
            target: crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSymbolTypeTarget,
        ) -> Result<
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSymbolType,
            JavaScriptPageDebuggerError,
        > {
            if target.metadata_handle != 41
                || target.metadata_generation != 9
                || target.symbol_id != 0
                || target.type_id != 1
            {
                return Err(JavaScriptPageDebuggerError::UnknownProgram);
            }
            Ok(
                crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSymbolType {
                    symbol_id: target.symbol_id,
                    type_id: target.type_id,
                },
            )
        }

        fn debugger_static_metadata_symbol_contract(
            &mut self,
            _tab_id: TabId,
            _document_generation: u64,
            target: crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSymbolContractTarget,
        ) -> Result<
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSymbolContract,
            JavaScriptPageDebuggerError,
        > {
            if target.metadata_handle != 41
                || target.metadata_generation != 9
                || target.symbol_id != 0
                || target.contract_id != 1
            {
                return Err(JavaScriptPageDebuggerError::UnknownProgram);
            }
            Ok(
                crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSymbolContract {
                    symbol_id: target.symbol_id,
                    contract_id: target.contract_id,
                },
            )
        }

        fn debugger_static_metadata_contracts(
            &mut self,
            _tab_id: TabId,
            _document_generation: u64,
            _program_handle: u64,
            _program_generation: u64,
            metadata_handle: u64,
            metadata_generation: u64,
        ) -> Result<
            Vec<crate::script::javascript::JavaScriptPageDebuggerStaticMetadataContractId>,
            JavaScriptPageDebuggerError,
        > {
            if metadata_handle != 41 || metadata_generation != 9 {
                return Err(JavaScriptPageDebuggerError::UnknownProgram);
            }
            Ok(vec![
                crate::script::javascript::JavaScriptPageDebuggerStaticMetadataContractId {
                    contract_id: 0,
                },
                crate::script::javascript::JavaScriptPageDebuggerStaticMetadataContractId {
                    contract_id: 1,
                },
            ])
        }

        fn debugger_static_metadata_contract_display(
            &mut self,
            _tab_id: TabId,
            _document_generation: u64,
            target: crate::script::javascript::JavaScriptPageDebuggerStaticMetadataContractTarget,
        ) -> Result<
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataContractDisplay,
            JavaScriptPageDebuggerError,
        > {
            if target.metadata_handle != 41
                || target.metadata_generation != 9
                || target.contract_id != 0
            {
                return Err(JavaScriptPageDebuggerError::UnknownProgram);
            }
            Ok(
                crate::script::javascript::JavaScriptPageDebuggerStaticMetadataContractDisplay {
                    contract_id: if self.mismatched_contract_display {
                        target.contract_id + 1
                    } else {
                        target.contract_id
                    },
                    display: "ProjectControlledContract".to_string(),
                    root_kind:
                        blueice_ipc::debugger::DebuggerStaticMetadataContractRootKind::Record,
                },
            )
        }

        fn debugger_static_metadata_contract_validation(
            &mut self,
            _tab_id: TabId,
            _document_generation: u64,
            target: crate::script::javascript::JavaScriptPageDebuggerStaticMetadataContractTarget,
            value: CompilerContractValue,
        ) -> Result<
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataContractValidation,
            JavaScriptPageDebuggerError,
        > {
            if target.metadata_handle != 41
                || target.metadata_generation != 9
                || target.contract_id != 0
            {
                return Err(JavaScriptPageDebuggerError::UnknownProgram);
            }
            Ok(
                crate::script::javascript::JavaScriptPageDebuggerStaticMetadataContractValidation {
                    contract_id: if self.mismatched_contract_validation {
                        target.contract_id + 1
                    } else {
                        target.contract_id
                    },
                    valid: matches!(value, CompilerContractValue::Boolean(true)),
                },
            )
        }

        fn debugger_safe_points(
            &mut self,
            _tab_id: TabId,
            _document_generation: u64,
            _program_handle: u64,
            _program_generation: u64,
        ) -> Result<
            Vec<crate::script::javascript::JavaScriptPageDebuggerSafePoint>,
            JavaScriptPageDebuggerError,
        > {
            Err(JavaScriptPageDebuggerError::NoLiveRealm)
        }

        fn validate_debugger_safe_point(
            &mut self,
            _tab_id: TabId,
            _document_generation: u64,
            _program_handle: u64,
            _program_generation: u64,
            _code_unit_ordinal: u32,
            _bytecode_offset: u32,
        ) -> Result<(), JavaScriptPageDebuggerError> {
            Err(JavaScriptPageDebuggerError::NoLiveRealm)
        }
    }

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
    fn static_metadata_summary_requires_a_dependent_session_grant_and_exact_handle() {
        let (tabs, realm) = loaded_tabs();
        let program = DebuggerProgram {
            realm,
            program_handle: 7,
            program_generation: 3,
        };
        let mut locations = MetadataLocations {
            malformed_summary: false,
            malformed_provenance: false,
            malformed_lowering_summary: false,
            mismatched_symbol_display: false,
            mismatched_contract_display: false,
            mismatched_contract_validation: false,
        };

        let denied_capabilities = handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            None,
            DebuggerRequest::DescribeCapabilities { realm },
        );
        let DebuggerReply::Capabilities(denied_capabilities) = denied_capabilities else {
            panic!("live realm capability discovery must succeed")
        };
        assert!(denied_capabilities.reports.iter().any(|report| {
            report.capability == DebuggerCapability::StaticMetadataInventory
                && report.state == DebuggerCapabilityState::Planned
        }));
        assert!(denied_capabilities.reports.iter().any(|report| {
            report.capability == DebuggerCapability::StaticMetadataSummary
                && report.state == DebuggerCapabilityState::Planned
        }));
        assert!(denied_capabilities.reports.iter().any(|report| {
            report.capability == DebuggerCapability::StaticMetadataSymbolInventory
                && report.state == DebuggerCapabilityState::Planned
        }));
        assert!(denied_capabilities.reports.iter().any(|report| {
            report.capability == DebuggerCapability::StaticMetadataContractInventory
                && report.state == DebuggerCapabilityState::Planned
        }));
        assert!(denied_capabilities.reports.iter().any(|report| {
            report.capability == DebuggerCapability::StaticMetadataContractDisplay
                && report.state == DebuggerCapabilityState::Planned
        }));
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                None,
                DebuggerRequest::ListStaticMetadata { program },
            ),
            unavailable_static_metadata_inventory()
        );
        let metadata = DebuggerStaticMetadataHandle {
            program,
            metadata_handle: 41,
            metadata_generation: 9,
        };
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                None,
                DebuggerRequest::DescribeStaticMetadata { metadata },
            ),
            unavailable_static_metadata_summary()
        );

        let hello = DebuggerRequest::Hello {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            requested_metadata_capabilities:
                blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_inventory(),
        };
        let hello_reply = blueice_ipc::debugger::negotiate(
            &hello,
            &blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_inventory(),
        );
        let metadata_session =
            blueice_ipc::debugger::metadata_session_authorization(&hello, &hello_reply)
                .expect("matching core policy must create a core-local session authorization");

        let allowed_capabilities = handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&metadata_session),
            DebuggerRequest::DescribeCapabilities { realm },
        );
        let DebuggerReply::Capabilities(allowed_capabilities) = allowed_capabilities else {
            panic!("live realm capability discovery must succeed")
        };
        assert!(allowed_capabilities.reports.iter().any(|report| {
            report.capability == DebuggerCapability::StaticMetadataInventory
                && report.state == DebuggerCapabilityState::Available
        }));
        assert!(allowed_capabilities.reports.iter().any(|report| {
            report.capability == DebuggerCapability::StaticMetadataSummary
                && report.state == DebuggerCapabilityState::Planned
        }));
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&metadata_session),
                DebuggerRequest::ListStaticMetadata { program },
            ),
            DebuggerReply::StaticMetadata(vec![DebuggerStaticMetadataHandle {
                program,
                metadata_handle: 41,
                metadata_generation: 9,
            }])
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&metadata_session),
                DebuggerRequest::DescribeStaticMetadata { metadata },
            ),
            unavailable_static_metadata_summary()
        );

        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&metadata_session),
                DebuggerRequest::ListStaticMetadataSources { metadata },
            ),
            unavailable_static_metadata_source_inventory()
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&metadata_session),
                DebuggerRequest::ListStaticMetadataSymbols { metadata },
            ),
            unavailable_static_metadata_symbol_inventory(),
            "symbol inventory remains default-denied under a parent-only grant"
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&metadata_session),
                DebuggerRequest::DescribeStaticMetadataSymbol {
                    symbol: DebuggerStaticMetadataSymbolId {
                        metadata,
                        symbol_id: 0,
                    },
                },
            ),
            unavailable_static_metadata_symbol_display(),
            "symbol display remains default-denied under a parent-only grant"
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&metadata_session),
                DebuggerRequest::ListStaticMetadataContracts { metadata },
            ),
            unavailable_static_metadata_contract_inventory(),
            "contract inventory remains default-denied under a parent-only grant"
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&metadata_session),
                DebuggerRequest::DescribeStaticMetadataContract {
                    contract: DebuggerStaticMetadataContractId {
                        metadata,
                        contract_id: 0,
                    },
                },
            ),
            unavailable_static_metadata_contract_display(),
            "contract display remains default-denied under a parent-only grant"
        );

        let symbol_inventory_hello = DebuggerRequest::Hello {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            requested_metadata_capabilities:
                blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_symbol_inventory(),
        };
        let symbol_inventory_hello_reply = blueice_ipc::debugger::negotiate(
            &symbol_inventory_hello,
            &blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_symbol_inventory(),
        );
        let symbol_inventory_session = blueice_ipc::debugger::metadata_session_authorization(
            &symbol_inventory_hello,
            &symbol_inventory_hello_reply,
        )
        .expect("dependent symbol inventory policy must create a core-local session authorization");
        let symbol_inventory_capabilities = handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&symbol_inventory_session),
            DebuggerRequest::DescribeCapabilities { realm },
        );
        let DebuggerReply::Capabilities(symbol_inventory_capabilities) =
            symbol_inventory_capabilities
        else {
            panic!("live realm symbol inventory capability discovery must succeed")
        };
        assert!(symbol_inventory_capabilities.reports.iter().any(|report| {
            report.capability == DebuggerCapability::StaticMetadataSymbolInventory
                && report.state == DebuggerCapabilityState::Available
        }));
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&symbol_inventory_session),
                DebuggerRequest::ListStaticMetadataSymbols { metadata },
            ),
            unavailable_static_metadata_symbol_inventory(),
            "symbol inventory cannot dereference a parent handle guessed before inventory"
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&symbol_inventory_session),
                DebuggerRequest::ListStaticMetadata { program },
            ),
            DebuggerReply::StaticMetadata(vec![metadata])
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&symbol_inventory_session),
                DebuggerRequest::ListStaticMetadataSymbols { metadata },
            ),
            DebuggerReply::StaticMetadataSymbols(vec![
                DebuggerStaticMetadataSymbolId {
                    metadata,
                    symbol_id: 0,
                },
                DebuggerStaticMetadataSymbolId {
                    metadata,
                    symbol_id: 1,
                },
                DebuggerStaticMetadataSymbolId {
                    metadata,
                    symbol_id: 2,
                },
            ])
        );

        let symbol_display_hello = DebuggerRequest::Hello {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            requested_metadata_capabilities:
                blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_symbol_display(),
        };
        let symbol_display_hello_reply = blueice_ipc::debugger::negotiate(
            &symbol_display_hello,
            &blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_symbol_display(),
        );
        let symbol_display_session = blueice_ipc::debugger::metadata_session_authorization(
            &symbol_display_hello,
            &symbol_display_hello_reply,
        )
        .expect("dependent symbol display policy must create a core-local session authorization");
        let symbol_display_capabilities = handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&symbol_display_session),
            DebuggerRequest::DescribeCapabilities { realm },
        );
        let DebuggerReply::Capabilities(symbol_display_capabilities) = symbol_display_capabilities
        else {
            panic!("live realm symbol display capability discovery must succeed")
        };
        assert!(symbol_display_capabilities.reports.iter().any(|report| {
            report.capability == DebuggerCapability::StaticMetadataSymbolDisplay
                && report.state == DebuggerCapabilityState::Available
        }));
        let displayed_symbol = DebuggerStaticMetadataSymbolId {
            metadata,
            symbol_id: 0,
        };
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&symbol_display_session),
                DebuggerRequest::DescribeStaticMetadataSymbol {
                    symbol: displayed_symbol,
                },
            ),
            unavailable_static_metadata_symbol_display(),
            "symbol display cannot dereference an ID guessed before its inventory receipt"
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&symbol_display_session),
                DebuggerRequest::ListStaticMetadata { program },
            ),
            DebuggerReply::StaticMetadata(vec![metadata])
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&symbol_display_session),
                DebuggerRequest::ListStaticMetadataSymbols { metadata },
            ),
            DebuggerReply::StaticMetadataSymbols(vec![
                displayed_symbol,
                DebuggerStaticMetadataSymbolId {
                    metadata,
                    symbol_id: 1,
                },
                DebuggerStaticMetadataSymbolId {
                    metadata,
                    symbol_id: 2,
                },
            ])
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&symbol_display_session),
                DebuggerRequest::DescribeStaticMetadataSymbol {
                    symbol: displayed_symbol,
                },
            ),
            DebuggerReply::StaticMetadataSymbol(DebuggerStaticMetadataSymbolDisplay {
                symbol: displayed_symbol,
                display: "ProjectControlledName".to_string(),
                kind: blueice_ipc::debugger::DebuggerStaticMetadataSymbolKind::Interface,
                exported: true,
            })
        );

        let contract_inventory_hello = DebuggerRequest::Hello {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            requested_metadata_capabilities:
                blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_contract_inventory(
                ),
        };
        let contract_inventory_hello_reply = blueice_ipc::debugger::negotiate(
            &contract_inventory_hello,
            &blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_contract_inventory(),
        );
        let contract_inventory_session = blueice_ipc::debugger::metadata_session_authorization(
            &contract_inventory_hello,
            &contract_inventory_hello_reply,
        )
        .expect(
            "dependent contract inventory policy must create a core-local session authorization",
        );
        let contract_inventory_capabilities = handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&contract_inventory_session),
            DebuggerRequest::DescribeCapabilities { realm },
        );
        let DebuggerReply::Capabilities(contract_inventory_capabilities) =
            contract_inventory_capabilities
        else {
            panic!("live realm contract inventory capability discovery must succeed")
        };
        assert!(contract_inventory_capabilities
            .reports
            .iter()
            .any(|report| {
                report.capability == DebuggerCapability::StaticMetadataContractInventory
                    && report.state == DebuggerCapabilityState::Available
            }));
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&contract_inventory_session),
                DebuggerRequest::ListStaticMetadataContracts { metadata },
            ),
            unavailable_static_metadata_contract_inventory(),
            "contract inventory cannot dereference a parent handle guessed before inventory"
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&contract_inventory_session),
                DebuggerRequest::ListStaticMetadata { program },
            ),
            DebuggerReply::StaticMetadata(vec![metadata])
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&contract_inventory_session),
                DebuggerRequest::ListStaticMetadataContracts { metadata },
            ),
            DebuggerReply::StaticMetadataContracts(vec![
                DebuggerStaticMetadataContractId {
                    metadata,
                    contract_id: 0,
                },
                DebuggerStaticMetadataContractId {
                    metadata,
                    contract_id: 1,
                },
            ])
        );

        let contract_display_hello = DebuggerRequest::Hello {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            requested_metadata_capabilities:
                blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_contract_display(),
        };
        let contract_display_hello_reply = blueice_ipc::debugger::negotiate(
            &contract_display_hello,
            &blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_contract_display(),
        );
        let contract_display_session = blueice_ipc::debugger::metadata_session_authorization(
            &contract_display_hello,
            &contract_display_hello_reply,
        )
        .expect("dependent contract display policy must create a core-local session authorization");
        let contract_display_capabilities = handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&contract_display_session),
            DebuggerRequest::DescribeCapabilities { realm },
        );
        let DebuggerReply::Capabilities(contract_display_capabilities) =
            contract_display_capabilities
        else {
            panic!("live realm contract display capability discovery must succeed")
        };
        assert!(contract_display_capabilities.reports.iter().any(|report| {
            report.capability == DebuggerCapability::StaticMetadataContractDisplay
                && report.state == DebuggerCapabilityState::Available
        }));
        let displayed_contract = DebuggerStaticMetadataContractId {
            metadata,
            contract_id: 0,
        };
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&contract_display_session),
                DebuggerRequest::DescribeStaticMetadataContract {
                    contract: displayed_contract,
                },
            ),
            unavailable_static_metadata_contract_display(),
            "contract display cannot dereference an ID guessed before its inventory receipt"
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&contract_display_session),
                DebuggerRequest::ListStaticMetadata { program },
            ),
            DebuggerReply::StaticMetadata(vec![metadata])
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&contract_display_session),
                DebuggerRequest::ListStaticMetadataContracts { metadata },
            ),
            DebuggerReply::StaticMetadataContracts(vec![
                displayed_contract,
                DebuggerStaticMetadataContractId {
                    metadata,
                    contract_id: 1,
                },
            ])
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&contract_display_session),
                DebuggerRequest::DescribeStaticMetadataContract {
                    contract: displayed_contract,
                },
            ),
            DebuggerReply::StaticMetadataContract(DebuggerStaticMetadataContractDisplay {
                contract: displayed_contract,
                display: "ProjectControlledContract".to_string(),
                root_kind: blueice_ipc::debugger::DebuggerStaticMetadataContractRootKind::Record,
            })
        );

        let source_inventory_hello = DebuggerRequest::Hello {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            requested_metadata_capabilities:
                blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_source_inventory(),
        };
        let source_inventory_hello_reply = blueice_ipc::debugger::negotiate(
            &source_inventory_hello,
            &blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_source_inventory(),
        );
        let source_inventory_session = blueice_ipc::debugger::metadata_session_authorization(
            &source_inventory_hello,
            &source_inventory_hello_reply,
        )
        .expect("dependent source inventory policy must create a core-local session authorization");
        let source_inventory_capabilities = handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&source_inventory_session),
            DebuggerRequest::DescribeCapabilities { realm },
        );
        let DebuggerReply::Capabilities(source_inventory_capabilities) =
            source_inventory_capabilities
        else {
            panic!("live realm source inventory capability discovery must succeed")
        };
        assert!(source_inventory_capabilities.reports.iter().any(|report| {
            report.capability == DebuggerCapability::StaticMetadataSourceInventory
                && report.state == DebuggerCapabilityState::Available
        }));
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&source_inventory_session),
                DebuggerRequest::ListStaticMetadataSources { metadata },
            ),
            unavailable_static_metadata_source_inventory(),
            "source inventory requires a parent handle emitted to this stream"
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&source_inventory_session),
                DebuggerRequest::ListStaticMetadata { program },
            ),
            DebuggerReply::StaticMetadata(vec![metadata])
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&source_inventory_session),
                DebuggerRequest::ListStaticMetadataSources { metadata },
            ),
            DebuggerReply::StaticMetadataSources(vec![DebuggerStaticMetadataSourceId {
                metadata,
                source_id: 0,
            }])
        );

        let summary_hello = DebuggerRequest::Hello {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            requested_metadata_capabilities:
                blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_summary(),
        };
        let summary_hello_reply = blueice_ipc::debugger::negotiate(
            &summary_hello,
            &blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_summary(),
        );
        let summary_session = blueice_ipc::debugger::metadata_session_authorization(
            &summary_hello,
            &summary_hello_reply,
        )
        .expect("dependent summary policy must create a core-local session authorization");
        let summary_capabilities = handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&summary_session),
            DebuggerRequest::DescribeCapabilities { realm },
        );
        let DebuggerReply::Capabilities(summary_capabilities) = summary_capabilities else {
            panic!("live realm summary capability discovery must succeed")
        };
        assert!(summary_capabilities.reports.iter().any(|report| {
            report.capability == DebuggerCapability::StaticMetadataSummary
                && report.state == DebuggerCapabilityState::Available
        }));
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&summary_session),
                DebuggerRequest::DescribeStaticMetadata { metadata },
            ),
            unavailable_static_metadata_summary(),
            "summary requires a parent handle emitted to this stream"
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&summary_session),
                DebuggerRequest::ListStaticMetadata { program },
            ),
            DebuggerReply::StaticMetadata(vec![metadata])
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&summary_session),
                DebuggerRequest::DescribeStaticMetadata { metadata },
            ),
            DebuggerReply::StaticMetadataSummary(DebuggerStaticMetadataSummary {
                metadata,
                language_version: "blue-ts-0.1".to_string(),
                compiler_options_hash: "0123456789abcdef".to_string(),
                source_count: 1,
                type_count: 2,
                symbol_count: 3,
                contract_count: 4,
            })
        );
    }

    #[test]
    fn static_metadata_summary_rejects_an_over_budget_child_reply() {
        let (tabs, realm) = loaded_tabs();
        let metadata = DebuggerStaticMetadataHandle {
            program: DebuggerProgram {
                realm,
                program_handle: 7,
                program_generation: 3,
            },
            metadata_handle: 41,
            metadata_generation: 9,
        };
        let hello = DebuggerRequest::Hello {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            requested_metadata_capabilities:
                blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_summary(),
        };
        let hello_reply = blueice_ipc::debugger::negotiate(
            &hello,
            &blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_summary(),
        );
        let session = blueice_ipc::debugger::metadata_session_authorization(&hello, &hello_reply)
            .expect("dependent summary policy must create a core-local session authorization");
        let mut locations = MetadataLocations {
            malformed_summary: true,
            malformed_provenance: false,
            malformed_lowering_summary: false,
            mismatched_symbol_display: false,
            mismatched_contract_display: false,
            mismatched_contract_validation: false,
        };
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::ListStaticMetadata {
                    program: metadata.program,
                },
            ),
            DebuggerReply::StaticMetadata(vec![metadata])
        );
        assert!(matches!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::DescribeStaticMetadata { metadata },
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidTarget,
                ..
            }
        ));
    }

    #[test]
    fn static_metadata_symbol_location_requires_exact_symbol_and_source_receipts() {
        let (tabs, realm) = loaded_tabs();
        let program = DebuggerProgram {
            realm,
            program_handle: 7,
            program_generation: 3,
        };
        let metadata = DebuggerStaticMetadataHandle {
            program,
            metadata_handle: 41,
            metadata_generation: 9,
        };
        let symbol = DebuggerStaticMetadataSymbolId {
            metadata,
            symbol_id: 0,
        };
        let source = DebuggerStaticMetadataSourceId {
            metadata,
            source_id: 0,
        };
        let target = DebuggerStaticMetadataSymbolLocationTarget { symbol, source };
        let hello = DebuggerRequest::Hello {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            requested_metadata_capabilities:
                blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_symbol_location(),
        };
        let hello_reply = blueice_ipc::debugger::negotiate(
            &hello,
            &blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_symbol_location(),
        );
        let session = blueice_ipc::debugger::metadata_session_authorization(&hello, &hello_reply)
            .expect(
                "dependent symbol-location policy must create a core-local session authorization",
            );
        let mut locations = MetadataLocations {
            malformed_summary: false,
            malformed_provenance: false,
            malformed_lowering_summary: false,
            mismatched_symbol_display: false,
            mismatched_contract_display: false,
            mismatched_contract_validation: false,
        };
        let DebuggerReply::Capabilities(capabilities) =
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::DescribeCapabilities { realm },
            )
        else {
            panic!("live realm symbol-location capability discovery must succeed")
        };
        assert!(capabilities.reports.iter().any(|report| {
            report.capability == DebuggerCapability::StaticMetadataSymbolLocation
                && report.state == DebuggerCapabilityState::Available
        }));
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::DescribeStaticMetadataSymbolLocation { target },
            ),
            unavailable_static_metadata_symbol_location(),
            "a caller cannot probe a child location before both IDs crossed this stream"
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::ListStaticMetadata { program },
            ),
            DebuggerReply::StaticMetadata(vec![metadata])
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::ListStaticMetadataSources { metadata },
            ),
            DebuggerReply::StaticMetadataSources(vec![source])
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::ListStaticMetadataSymbols { metadata },
            ),
            DebuggerReply::StaticMetadataSymbols(vec![
                symbol,
                DebuggerStaticMetadataSymbolId {
                    metadata,
                    symbol_id: 1,
                },
                DebuggerStaticMetadataSymbolId {
                    metadata,
                    symbol_id: 2,
                },
            ])
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::DescribeStaticMetadataSymbolLocation { target },
            ),
            DebuggerReply::StaticMetadataSymbolLocation(DebuggerStaticMetadataSymbolLocation {
                symbol,
                source,
                start_byte: 6,
                end_byte: 31,
                coordinates: DebuggerSourceCoordinates {
                    start_line: 0,
                    start_column_utf16: 6,
                    end_line: 0,
                    end_column_utf16: 31,
                },
            })
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::DescribeStaticMetadataSymbolLocation {
                    target: DebuggerStaticMetadataSymbolLocationTarget {
                        source: DebuggerStaticMetadataSourceId {
                            source_id: 1,
                            ..source
                        },
                        ..target
                    },
                },
            ),
            unavailable_static_metadata_symbol_location(),
            "a guessed source ID cannot be paired with an observed symbol"
        );
    }

    #[test]
    fn static_metadata_safe_point_span_requires_its_own_grant_and_source_receipt() {
        let (tabs, realm) = loaded_tabs();
        let program = DebuggerProgram {
            realm,
            program_handle: 7,
            program_generation: 3,
        };
        let metadata = DebuggerStaticMetadataHandle {
            program,
            metadata_handle: 41,
            metadata_generation: 9,
        };
        let source = DebuggerStaticMetadataSourceId {
            metadata,
            source_id: 0,
        };
        let safe_point = DebuggerSafePoint {
            program,
            code_unit_ordinal: 0,
            bytecode_offset: 4,
        };
        let target = DebuggerStaticMetadataSafePointSpanTarget { safe_point, source };
        let manifest =
            blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_safe_point_span();
        let hello = DebuggerRequest::Hello {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            requested_metadata_capabilities: manifest.clone(),
        };
        let reply = blueice_ipc::debugger::negotiate(&hello, &manifest);
        let session = blueice_ipc::debugger::metadata_session_authorization(&hello, &reply)
            .expect("explicit safe-point span grant must create a session");
        let mut locations = MetadataLocations {
            malformed_summary: false,
            malformed_provenance: false,
            malformed_lowering_summary: false,
            mismatched_symbol_display: false,
            mismatched_contract_display: false,
            mismatched_contract_validation: false,
        };
        let DebuggerReply::Capabilities(capabilities) =
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::DescribeCapabilities { realm },
            )
        else {
            panic!("live child must report metadata capabilities");
        };
        assert!(capabilities.reports.iter().any(|report| {
            report.capability == DebuggerCapability::StaticMetadataSafePointSpan
                && report.state == DebuggerCapabilityState::Available
        }));
        let request = DebuggerRequest::DescribeStaticMetadataSafePointSpan { target };
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                request.clone(),
            ),
            unavailable_static_metadata_safe_point_span()
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::ListStaticMetadata { program },
            ),
            DebuggerReply::StaticMetadata(vec![metadata])
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                request.clone(),
            ),
            unavailable_static_metadata_safe_point_span()
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::ListStaticMetadataSources { metadata },
            ),
            DebuggerReply::StaticMetadataSources(vec![source])
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                request.clone(),
            ),
            DebuggerReply::StaticMetadataSafePointSpan(DebuggerStaticMetadataSafePointSpan {
                safe_point,
                source,
                start_byte: 6,
                end_byte: 31,
            })
        );
        let separate_session =
            blueice_ipc::debugger::metadata_session_authorization(&hello, &reply)
                .expect("a second stream must negotiate independently");
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&separate_session),
                request.clone(),
            ),
            unavailable_static_metadata_safe_point_span()
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::DescribeStaticMetadataSafePointSpan {
                    target: DebuggerStaticMetadataSafePointSpanTarget {
                        source: DebuggerStaticMetadataSourceId {
                            source_id: 1,
                            ..source
                        },
                        ..target
                    },
                },
            ),
            unavailable_static_metadata_safe_point_span()
        );
        assert!(matches!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::DescribeStaticMetadataSafePointSpan {
                    target: DebuggerStaticMetadataSafePointSpanTarget {
                        safe_point: DebuggerSafePoint {
                            bytecode_offset: 5,
                            ..safe_point
                        },
                        ..target
                    },
                },
            ),
            DebuggerReply::Error { .. }
        ));
        let inventory_only =
            blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_source_inventory();
        let denied_reply = blueice_ipc::debugger::negotiate(&hello, &inventory_only);
        let denied_session =
            blueice_ipc::debugger::metadata_session_authorization(&hello, &denied_reply).unwrap();
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&denied_session),
                request,
            ),
            unavailable_static_metadata_safe_point_span()
        );
    }

    #[test]
    fn static_metadata_contract_location_requires_independent_same_stream_receipts() {
        let (tabs, realm) = loaded_tabs();
        let program = DebuggerProgram {
            realm,
            program_handle: 7,
            program_generation: 3,
        };
        let metadata = DebuggerStaticMetadataHandle {
            program,
            metadata_handle: 41,
            metadata_generation: 9,
        };
        let contract = DebuggerStaticMetadataContractId {
            metadata,
            contract_id: 0,
        };
        let source = DebuggerStaticMetadataSourceId {
            metadata,
            source_id: 0,
        };
        let target = DebuggerStaticMetadataContractLocationTarget { contract, source };
        let manifest =
            blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_contract_location();
        let hello = DebuggerRequest::Hello {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            requested_metadata_capabilities: manifest.clone(),
        };
        let hello_reply = blueice_ipc::debugger::negotiate(&hello, &manifest);
        let session = blueice_ipc::debugger::metadata_session_authorization(&hello, &hello_reply)
            .expect("dependent contract-location grant must create a session");
        let mut locations = MetadataLocations {
            malformed_summary: false,
            malformed_provenance: false,
            malformed_lowering_summary: false,
            mismatched_symbol_display: false,
            mismatched_contract_display: false,
            mismatched_contract_validation: false,
        };
        let DebuggerReply::Capabilities(capabilities) =
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::DescribeCapabilities { realm },
            )
        else {
            panic!("live contract-location capability discovery must succeed")
        };
        assert!(capabilities.reports.iter().any(|report| {
            report.capability == DebuggerCapability::StaticMetadataContractLocation
                && report.state == DebuggerCapabilityState::Available
        }));
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::DescribeStaticMetadataContractLocation { target },
            ),
            unavailable_static_metadata_contract_location(),
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::ListStaticMetadata { program },
            ),
            DebuggerReply::StaticMetadata(vec![metadata]),
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::ListStaticMetadataSources { metadata },
            ),
            DebuggerReply::StaticMetadataSources(vec![source]),
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::ListStaticMetadataContracts { metadata },
            ),
            DebuggerReply::StaticMetadataContracts(vec![
                contract,
                DebuggerStaticMetadataContractId {
                    contract_id: 1,
                    ..contract
                },
            ]),
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::DescribeStaticMetadataContractLocation { target },
            ),
            DebuggerReply::StaticMetadataContractLocation(DebuggerStaticMetadataContractLocation {
                contract,
                source,
                start_byte: 6,
                end_byte: 31,
                coordinates: DebuggerSourceCoordinates {
                    start_line: 0,
                    start_column_utf16: 6,
                    end_line: 0,
                    end_column_utf16: 31,
                },
            }),
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::DescribeStaticMetadataContractLocation {
                    target: DebuggerStaticMetadataContractLocationTarget {
                        source: DebuggerStaticMetadataSourceId {
                            source_id: 1,
                            ..source
                        },
                        ..target
                    },
                },
            ),
            unavailable_static_metadata_contract_location(),
        );
        let separate = blueice_ipc::debugger::metadata_session_authorization(&hello, &hello_reply)
            .expect("a separate stream has its own receipt ledger");
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&separate),
                DebuggerRequest::DescribeStaticMetadataContractLocation { target },
            ),
            unavailable_static_metadata_contract_location(),
        );
    }

    #[test]
    fn static_metadata_symbol_type_requires_both_exact_receipts() {
        let (tabs, realm) = loaded_tabs();
        let program = DebuggerProgram {
            realm,
            program_handle: 7,
            program_generation: 3,
        };
        let metadata = DebuggerStaticMetadataHandle {
            program,
            metadata_handle: 41,
            metadata_generation: 9,
        };
        let target = DebuggerStaticMetadataSymbolType {
            symbol: DebuggerStaticMetadataSymbolId {
                metadata,
                symbol_id: 0,
            },
            static_type: DebuggerStaticMetadataTypeId {
                metadata,
                type_id: 1,
            },
        };
        let manifest =
            blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_symbol_type();
        let hello = DebuggerRequest::Hello {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            requested_metadata_capabilities: manifest.clone(),
        };
        let hello_reply = blueice_ipc::debugger::negotiate(&hello, &manifest);
        let session = blueice_ipc::debugger::metadata_session_authorization(&hello, &hello_reply)
            .expect("the exact dependent symbol/type grant creates a local session");
        let mut locations = MetadataLocations {
            malformed_summary: false,
            malformed_provenance: false,
            malformed_lowering_summary: false,
            mismatched_symbol_display: false,
            mismatched_contract_display: false,
            mismatched_contract_validation: false,
        };
        let DebuggerReply::Capabilities(capabilities) =
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::DescribeCapabilities { realm },
            )
        else {
            panic!("the live child must report its symbol/type capability")
        };
        assert!(capabilities.reports.iter().any(|report| {
            report.capability == DebuggerCapability::StaticMetadataSymbolType
                && report.state == DebuggerCapabilityState::Available
        }));
        let request = DebuggerRequest::DescribeStaticMetadataSymbolType { target };
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                None,
                request.clone(),
            ),
            unavailable_static_metadata_symbol_type(),
            "the public relation is default-denied without an owner/Hello session"
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                request.clone(),
            ),
            unavailable_static_metadata_symbol_type(),
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::ListStaticMetadata { program },
            ),
            DebuggerReply::StaticMetadata(vec![metadata]),
        );
        assert!(matches!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::ListStaticMetadataSymbols { metadata },
            ),
            DebuggerReply::StaticMetadataSymbols(_)
        ));
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                request.clone(),
            ),
            unavailable_static_metadata_symbol_type(),
            "symbol inventory cannot stand in for the separate type receipt"
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::ListStaticMetadataTypes { metadata },
            ),
            DebuggerReply::StaticMetadataTypes(vec![
                DebuggerStaticMetadataTypeId {
                    metadata,
                    type_id: 0
                },
                target.static_type,
            ]),
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                request,
            ),
            DebuggerReply::StaticMetadataSymbolType(target),
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::DescribeStaticMetadataSymbolType {
                    target: DebuggerStaticMetadataSymbolType {
                        static_type: DebuggerStaticMetadataTypeId {
                            type_id: 999,
                            ..target.static_type
                        },
                        ..target
                    },
                },
            ),
            unavailable_static_metadata_symbol_type(),
            "a guessed type ID cannot reach the child"
        );
        assert!(matches!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::DescribeStaticMetadataSymbolType {
                    target: DebuggerStaticMetadataSymbolType {
                        static_type: DebuggerStaticMetadataTypeId {
                            type_id: 0,
                            ..target.static_type
                        },
                        ..target
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
    fn static_metadata_symbol_contract_requires_both_exact_receipts() {
        let (tabs, realm) = loaded_tabs();
        let program = DebuggerProgram {
            realm,
            program_handle: 7,
            program_generation: 3,
        };
        let metadata = DebuggerStaticMetadataHandle {
            program,
            metadata_handle: 41,
            metadata_generation: 9,
        };
        let target = DebuggerStaticMetadataSymbolContract {
            symbol: DebuggerStaticMetadataSymbolId {
                metadata,
                symbol_id: 0,
            },
            contract: DebuggerStaticMetadataContractId {
                metadata,
                contract_id: 1,
            },
        };
        let manifest =
            blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_symbol_contract();
        let hello = DebuggerRequest::Hello {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            requested_metadata_capabilities: manifest.clone(),
        };
        let hello_reply = blueice_ipc::debugger::negotiate(&hello, &manifest);
        let session = blueice_ipc::debugger::metadata_session_authorization(&hello, &hello_reply)
            .expect("the exact dependent symbol/contract grant creates a local session");
        let mut locations = MetadataLocations {
            malformed_summary: false,
            malformed_provenance: false,
            malformed_lowering_summary: false,
            mismatched_symbol_display: false,
            mismatched_contract_display: false,
            mismatched_contract_validation: false,
        };
        let DebuggerReply::Capabilities(capabilities) =
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::DescribeCapabilities { realm },
            )
        else {
            panic!("the live child must report its symbol/contract capability")
        };
        assert!(capabilities.reports.iter().any(|report| {
            report.capability == DebuggerCapability::StaticMetadataSymbolContract
                && report.state == DebuggerCapabilityState::Available
        }));
        let request = DebuggerRequest::DescribeStaticMetadataSymbolContract { target };
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                None,
                request.clone(),
            ),
            unavailable_static_metadata_symbol_contract(),
            "the public relation is default-denied without an owner/Hello session"
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                request.clone(),
            ),
            unavailable_static_metadata_symbol_contract(),
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::ListStaticMetadata { program },
            ),
            DebuggerReply::StaticMetadata(vec![metadata]),
        );
        assert!(matches!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::ListStaticMetadataSymbols { metadata },
            ),
            DebuggerReply::StaticMetadataSymbols(_)
        ));
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                request.clone(),
            ),
            unavailable_static_metadata_symbol_contract(),
            "symbol inventory cannot stand in for the separate contract receipt"
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::ListStaticMetadataContracts { metadata },
            ),
            DebuggerReply::StaticMetadataContracts(vec![
                DebuggerStaticMetadataContractId {
                    metadata,
                    contract_id: 0
                },
                target.contract,
            ]),
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                request,
            ),
            DebuggerReply::StaticMetadataSymbolContract(target),
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::DescribeStaticMetadataSymbolContract {
                    target: DebuggerStaticMetadataSymbolContract {
                        contract: DebuggerStaticMetadataContractId {
                            contract_id: 999,
                            ..target.contract
                        },
                        ..target
                    },
                },
            ),
            unavailable_static_metadata_symbol_contract(),
            "a guessed contract ID cannot reach the child"
        );
        assert!(matches!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::DescribeStaticMetadataSymbolContract {
                    target: DebuggerStaticMetadataSymbolContract {
                        contract: DebuggerStaticMetadataContractId {
                            contract_id: 0,
                            ..target.contract
                        },
                        ..target
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
    fn static_metadata_lowering_summary_requires_a_receipt_and_rejects_noncanonical_child_data() {
        let (tabs, realm) = loaded_tabs();
        let program = DebuggerProgram {
            realm,
            program_handle: 7,
            program_generation: 3,
        };
        let metadata = DebuggerStaticMetadataHandle {
            program,
            metadata_handle: 41,
            metadata_generation: 9,
        };
        let hello = DebuggerRequest::Hello {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            requested_metadata_capabilities:
                blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_lowering_summary(),
        };
        let hello_reply = blueice_ipc::debugger::negotiate(
            &hello,
            &blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_lowering_summary(),
        );
        let session = blueice_ipc::debugger::metadata_session_authorization(&hello, &hello_reply)
            .expect(
                "dependent lowering-summary policy must create a core-local session authorization",
            );
        let mut locations = MetadataLocations {
            malformed_summary: false,
            malformed_provenance: false,
            malformed_lowering_summary: false,
            mismatched_symbol_display: false,
            mismatched_contract_display: false,
            mismatched_contract_validation: false,
        };
        let capabilities = handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::DescribeCapabilities { realm },
        );
        let DebuggerReply::Capabilities(capabilities) = capabilities else {
            panic!("live realm lowering-summary capability discovery must succeed")
        };
        assert!(capabilities.reports.iter().any(|report| {
            report.capability == DebuggerCapability::StaticMetadataLoweringSummary
                && report.state == DebuggerCapabilityState::Available
        }));
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::DescribeStaticMetadataLoweringSummary { metadata },
            ),
            unavailable_static_metadata_lowering_summary(),
            "a guessed metadata handle must fail before core reaches the child"
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::ListStaticMetadata { program },
            ),
            DebuggerReply::StaticMetadata(vec![metadata])
        );
        let reply = handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::DescribeStaticMetadataLoweringSummary { metadata },
        );
        let DebuggerReply::StaticMetadataLoweringSummary(summary) = reply else {
            panic!("an inventoried metadata handle must expose its bounded lowering summary")
        };
        assert_eq!(summary.metadata, metadata);
        assert_eq!(
            summary.safe_point_map_abi,
            blueice_ipc::debugger::DEBUGGER_STATIC_METADATA_SAFE_POINT_MAP_ABI_V1
        );
        assert_eq!(
            summary.program_abi,
            blueice_ipc::debugger::DEBUGGER_STATIC_METADATA_PROGRAM_ABI_V1
        );
        assert_eq!(summary.source_set_hash, "bts-source-set-0123456789abcdef");
        assert_eq!(summary.bound_safe_point_count, 1);
        let disclosure = format!("{summary:?}");
        assert!(
            !disclosure.contains("page://")
                && !disclosure.contains("main.ts")
                && !disclosure.contains("bytecode")
                && !disclosure.contains("privateBlueTsMetadata"),
            "lowering summary must exclude source identities, map entries, offsets, and static-record payloads"
        );

        locations.malformed_lowering_summary = true;
        assert!(matches!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::DescribeStaticMetadataLoweringSummary { metadata },
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidTarget,
                ..
            }
        ));
    }

    #[test]
    fn static_metadata_symbol_display_rejects_a_mismatched_child_identity() {
        let (tabs, realm) = loaded_tabs();
        let metadata = DebuggerStaticMetadataHandle {
            program: DebuggerProgram {
                realm,
                program_handle: 7,
                program_generation: 3,
            },
            metadata_handle: 41,
            metadata_generation: 9,
        };
        let symbol = DebuggerStaticMetadataSymbolId {
            metadata,
            symbol_id: 0,
        };
        let hello = DebuggerRequest::Hello {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            requested_metadata_capabilities:
                blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_symbol_display(),
        };
        let hello_reply = blueice_ipc::debugger::negotiate(
            &hello,
            &blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_symbol_display(),
        );
        let session = blueice_ipc::debugger::metadata_session_authorization(&hello, &hello_reply)
            .expect(
                "dependent symbol display policy must create a core-local session authorization",
            );
        let mut locations = MetadataLocations {
            malformed_summary: false,
            malformed_provenance: false,
            malformed_lowering_summary: false,
            mismatched_symbol_display: true,
            mismatched_contract_display: false,
            mismatched_contract_validation: false,
        };
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::ListStaticMetadata {
                    program: metadata.program,
                },
            ),
            DebuggerReply::StaticMetadata(vec![metadata])
        );
        assert!(matches!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::ListStaticMetadataSymbols { metadata },
            ),
            DebuggerReply::StaticMetadataSymbols(_)
        ));
        assert!(matches!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::DescribeStaticMetadataSymbol { symbol },
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidTarget,
                ..
            }
        ));
    }

    #[test]
    fn static_metadata_contract_display_rejects_a_mismatched_child_identity() {
        let (tabs, realm) = loaded_tabs();
        let metadata = DebuggerStaticMetadataHandle {
            program: DebuggerProgram {
                realm,
                program_handle: 7,
                program_generation: 3,
            },
            metadata_handle: 41,
            metadata_generation: 9,
        };
        let contract = DebuggerStaticMetadataContractId {
            metadata,
            contract_id: 0,
        };
        let hello = DebuggerRequest::Hello {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            requested_metadata_capabilities:
                blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_contract_display(),
        };
        let hello_reply = blueice_ipc::debugger::negotiate(
            &hello,
            &blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_contract_display(),
        );
        let session = blueice_ipc::debugger::metadata_session_authorization(&hello, &hello_reply)
            .expect(
                "dependent contract display policy must create a core-local session authorization",
            );
        let mut locations = MetadataLocations {
            malformed_summary: false,
            malformed_provenance: false,
            malformed_lowering_summary: false,
            mismatched_symbol_display: false,
            mismatched_contract_display: true,
            mismatched_contract_validation: false,
        };
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::ListStaticMetadata {
                    program: metadata.program,
                },
            ),
            DebuggerReply::StaticMetadata(vec![metadata])
        );
        assert!(matches!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::ListStaticMetadataContracts { metadata },
            ),
            DebuggerReply::StaticMetadataContracts(_)
        ));
        assert!(matches!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::DescribeStaticMetadataContract { contract },
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidTarget,
                ..
            }
        ));
    }

    #[test]
    fn static_metadata_contract_validation_requires_a_receipt_and_hides_failure_detail() {
        let (tabs, realm) = loaded_tabs();
        let program = DebuggerProgram {
            realm,
            program_handle: 7,
            program_generation: 3,
        };
        let metadata = DebuggerStaticMetadataHandle {
            program,
            metadata_handle: 41,
            metadata_generation: 9,
        };
        let contract = DebuggerStaticMetadataContractId {
            metadata,
            contract_id: 0,
        };
        let hello = DebuggerRequest::Hello {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            requested_metadata_capabilities:
                blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_contract_validation(),
        };
        let hello_reply = blueice_ipc::debugger::negotiate(
            &hello,
            &blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_contract_validation(
            ),
        );
        let session = blueice_ipc::debugger::metadata_session_authorization(&hello, &hello_reply)
            .expect(
            "dependent contract-validation policy must create a core-local session authorization",
        );
        let mut locations = MetadataLocations {
            malformed_summary: false,
            malformed_provenance: false,
            malformed_lowering_summary: false,
            mismatched_symbol_display: false,
            mismatched_contract_display: false,
            mismatched_contract_validation: false,
        };
        let capabilities = handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::DescribeCapabilities { realm },
        );
        let DebuggerReply::Capabilities(capabilities) = capabilities else {
            panic!("live realm contract-validation capability discovery must succeed")
        };
        assert!(capabilities.reports.iter().any(|report| {
            report.capability == DebuggerCapability::StaticMetadataContractValidation
                && report.state == DebuggerCapabilityState::Available
        }));
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::ValidateStaticMetadataContract {
                    contract,
                    value: CompilerContractValue::Boolean(true),
                },
            ),
            unavailable_static_metadata_contract_validation(),
            "a guessed contract ID must fail before core reaches the child"
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::ListStaticMetadata { program },
            ),
            DebuggerReply::StaticMetadata(vec![metadata])
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::ListStaticMetadataContracts { metadata },
            ),
            DebuggerReply::StaticMetadataContracts(vec![
                contract,
                DebuggerStaticMetadataContractId {
                    metadata,
                    contract_id: 1,
                },
            ])
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::ValidateStaticMetadataContract {
                    contract,
                    value: CompilerContractValue::Boolean(true),
                },
            ),
            DebuggerReply::StaticMetadataContractValidation(
                DebuggerStaticMetadataContractValidation {
                    contract,
                    valid: true,
                },
            )
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::ValidateStaticMetadataContract {
                    contract,
                    value: CompilerContractValue::Boolean(false),
                },
            ),
            DebuggerReply::StaticMetadataContractValidation(
                DebuggerStaticMetadataContractValidation {
                    contract,
                    valid: false,
                },
            )
        );
        assert!(matches!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::ValidateStaticMetadataContract {
                    contract,
                    value: CompilerContractValue::String(
                        "x".repeat(
                            blueice_ipc::debugger::DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_STRING_BYTES
                                + 1,
                        ),
                    ),
                },
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidTarget,
                ..
            }
        ));
        locations.mismatched_contract_validation = true;
        assert!(matches!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::ValidateStaticMetadataContract {
                    contract,
                    value: CompilerContractValue::Boolean(true),
                },
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidTarget,
                ..
            }
        ));
    }

    #[test]
    fn source_provenance_requires_its_own_dependent_grant_and_exact_source_id() {
        let (tabs, realm) = loaded_tabs();
        let metadata = DebuggerStaticMetadataHandle {
            program: DebuggerProgram {
                realm,
                program_handle: 7,
                program_generation: 3,
            },
            metadata_handle: 41,
            metadata_generation: 9,
        };
        let source = DebuggerStaticMetadataSourceId {
            metadata,
            source_id: 0,
        };
        let mut locations = MetadataLocations {
            malformed_summary: false,
            malformed_provenance: false,
            malformed_lowering_summary: false,
            mismatched_symbol_display: false,
            mismatched_contract_display: false,
            mismatched_contract_validation: false,
        };

        let source_inventory_hello = DebuggerRequest::Hello {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            requested_metadata_capabilities:
                blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_source_inventory(),
        };
        let source_inventory_reply = blueice_ipc::debugger::negotiate(
            &source_inventory_hello,
            &blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_source_inventory(),
        );
        let source_inventory_session = blueice_ipc::debugger::metadata_session_authorization(
            &source_inventory_hello,
            &source_inventory_reply,
        )
        .expect("source inventory policy must create a core-local session authorization");
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&source_inventory_session),
                DebuggerRequest::DescribeStaticMetadataSource { source },
            ),
            unavailable_static_metadata_source_provenance()
        );

        let provenance_hello = DebuggerRequest::Hello {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            requested_metadata_capabilities:
                blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_source_provenance(
                ),
        };
        let provenance_hello_reply = blueice_ipc::debugger::negotiate(
            &provenance_hello,
            &blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_source_provenance(),
        );
        let provenance_session = blueice_ipc::debugger::metadata_session_authorization(
            &provenance_hello,
            &provenance_hello_reply,
        )
        .expect("source provenance policy must create a core-local session authorization");
        let capabilities = handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&provenance_session),
            DebuggerRequest::DescribeCapabilities { realm },
        );
        let DebuggerReply::Capabilities(capabilities) = capabilities else {
            panic!("live realm provenance capability discovery must succeed")
        };
        assert!(capabilities.reports.iter().any(|report| {
            report.capability == DebuggerCapability::StaticMetadataSourceProvenance
                && report.state == DebuggerCapabilityState::Available
        }));
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&provenance_session),
                DebuggerRequest::DescribeStaticMetadataSource { source },
            ),
            unavailable_static_metadata_source_provenance(),
            "a provenance target must have been emitted by this stream's source inventory"
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&provenance_session),
                DebuggerRequest::ListStaticMetadataSources { metadata },
            ),
            unavailable_static_metadata_source_inventory(),
            "source inventory cannot dereference a parent handle guessed before inventory"
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&provenance_session),
                DebuggerRequest::ListStaticMetadata {
                    program: metadata.program,
                },
            ),
            DebuggerReply::StaticMetadata(vec![metadata])
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&provenance_session),
                DebuggerRequest::ListStaticMetadataSources { metadata },
            ),
            DebuggerReply::StaticMetadataSources(vec![DebuggerStaticMetadataSourceId {
                metadata,
                source_id: 0,
            }])
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&provenance_session),
                DebuggerRequest::DescribeStaticMetadataSource { source },
            ),
            DebuggerReply::StaticMetadataSourceProvenance(DebuggerStaticMetadataSourceProvenance {
                source,
                module: "page:///main.ts".to_string(),
                content_hash:
                    "bts-sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
                        .to_string(),
            })
        );
        locations.malformed_provenance = true;
        assert!(matches!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&provenance_session),
                DebuggerRequest::DescribeStaticMetadataSource { source },
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidTarget,
                ..
            }
        ));
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
    fn native_debugger_steps_an_exact_classic_program_through_the_public_route() {
        let mut tabs = TabManager::new(320.0, 200.0);
        let tab_id = tabs.default_tab();
        tabs.get_mut(tab_id).unwrap().load_html_str(
            "<script>let value = 1; globalThis.answer = value + 2;</script>",
            Some("https://example.test/native-step.html".to_string()),
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

        let DebuggerReply::Capabilities(capabilities) =
            handle_debugger_request_with_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::DescribeCapabilities { realm },
            )
        else {
            panic!("the live realm must describe debugger capabilities")
        };
        assert!(capabilities.reports.iter().any(|report| {
            report.capability == DebuggerCapability::Stepping
                && report.state == DebuggerCapabilityState::Available
        }));
        let DebuggerReply::Programs(programs) = handle_debugger_request_with_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ListPrograms { realm },
        ) else {
            panic!("the admitted declaration must have an opaque program")
        };
        let program = programs[0];
        let DebuggerReply::SafePoints(safe_points) =
            handle_debugger_request_with_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::ListSafePoints { program },
            )
        else {
            panic!("the admitted declaration must have safe points")
        };
        let entry = *safe_points
            .iter()
            .find(|point| point.code_unit_ordinal == 0 && point.bytecode_offset == 0)
            .unwrap();
        assert_eq!(
            handle_debugger_request_with_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::ArmEntryBreakpoint { safe_point: entry },
            ),
            DebuggerReply::BreakpointArmed { safe_point: entry }
        );
        executor.synchronize_and_execute(&tabs).unwrap();
        let step = DebuggerRequest::StepRootInstruction { program };
        assert_eq!(
            handle_debugger_request_with_javascript_executor(
                &tabs,
                Some(&mut executor),
                step.clone(),
            ),
            DebuggerReply::ExecutionStepRequested { program }
        );
        assert_eq!(
            handle_debugger_request_with_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::GetExecutionState { program },
            ),
            DebuggerReply::ExecutionState {
                program,
                state: DebuggerExecutionState::Stepping,
            }
        );
        assert!(matches!(
            handle_debugger_request_with_javascript_executor(
                &tabs,
                Some(&mut executor),
                step.clone(),
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidExecutionState,
                ..
            }
        ));
        executor.synchronize_and_execute(&tabs).unwrap();
        let DebuggerReply::ExecutionState {
            state:
                DebuggerExecutionState::Paused {
                    safe_point: advanced,
                },
            ..
        } = handle_debugger_request_with_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::GetExecutionState { program },
        )
        else {
            panic!("one root instruction must return to an exact paused boundary")
        };
        assert_ne!(advanced, entry);
        assert!(safe_points.contains(&advanced));
        assert!(executor.drain_reports_for_tab(tab_id).is_empty());
        assert!(matches!(
            handle_debugger_request_with_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::StepRootInstruction {
                    program: DebuggerProgram {
                        program_generation: program.program_generation + 1,
                        ..program
                    },
                },
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::StaleProgram,
                ..
            }
        ));
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
        assert!(matches!(
            handle_debugger_request_with_javascript_executor(&tabs, Some(&mut executor), step),
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidExecutionState,
                ..
            }
        ));
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
                metadata_session: None,
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
