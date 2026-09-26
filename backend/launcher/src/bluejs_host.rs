// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Launcher supervision and execution for the first isolated BlueJS page
//! host.
//!
//! [`SpawnedBlueJsHost`] owns a separate `blueice-bluejs-host` child, its
//! private Unix socket, and its one-time connection capability. The child
//! owns every [`BlueJsPageRuntime`] VM and program registry; no `Vm`, source
//! text, program handle, or DOM object crosses back into the launcher. Only
//! an exact paused-slot, bounded plain-data debugger preview may now carry a
//! runtime value. A future core page-loader adapter is responsible for deriving
//! [`PageHostDocument`] from an already-authorized navigation. This module
//! deliberately does not let a page or a frontend connect to the child.

use blueice_bluejs::{
    parse, parse_module, BlueJsPageDebuggerExecutionState, BlueJsPageDebuggerFrame,
    BlueJsPageDebuggerLinkedExecutionState, BlueJsPageDebuggerLinkedFrame,
    BlueJsPageDebuggerNestedExecutionState, BlueJsPageDebuggerValueTarget, BlueJsPageOrigin,
    BlueJsPageRuntime, BlueJsPageRuntimeConfig, BlueJsPageRuntimeError, BlueJsProgramHandle,
    BlueJsProgramV1, BlueJsSourceIdentity, CompileError, HeapConfig, HostFunctionError,
    HostObjectFamily, HostObjectKey, HostValue, Module, ParseError, RuntimeError, Value, Vm,
    VmConfig, VmDebuggerScopeEntry, VmDebuggerValuePreview, VM_DEBUGGER_MAX_SCOPE_ENTRIES,
    VM_DEBUGGER_MAX_STACK_FRAMES,
};
use blueice_bluets::{
    AuthorizedModule, AuthorizedModuleLoader, AuthorizedModuleResolution, CompilerOptions,
    Contract, ContractPlan, ContractValue, RuntimePolicy, ValidationLimits,
};
use blueice_bluets_bluejs::page_host_typings::{
    page_host_document_runtime_bindings_v1, page_host_dom_event_runtime_bindings_v1,
    page_host_dom_mutation_runtime_bindings_v1, page_host_dom_text_runtime_bindings_v1,
    PageHostDocumentTypingsV1,
};
use blueice_bluets_bluejs::{
    compile_direct_module_graph, compile_direct_script, BridgeError, DirectDebugRegistry,
    DirectModuleGraph, DirectPageModuleGraphAttachment, DirectRootSymbolSlot,
    DirectSafePointBinding, DirectScript, RetainedDirectDebugInfo,
};
use blueice_ipc::compiler::CompilerContractValue;
use blueice_ipc::debugger::{
    DebuggerSourceCoordinates, DebuggerStaticMetadataContractRootKind,
    DebuggerStaticMetadataSymbolKind, DEBUGGER_STATIC_METADATA_CONTRACT_DISPLAY_MAX_BYTES,
    DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_COLLECTION_ENTRIES,
    DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_DEPTH,
    DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_NODES,
    DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_STRING_BYTES,
    DEBUGGER_STATIC_METADATA_MAX_CONTRACTS, DEBUGGER_STATIC_METADATA_MAX_SOURCES,
    DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES, DEBUGGER_STATIC_METADATA_MAX_SYMBOLS,
    DEBUGGER_STATIC_METADATA_MAX_TYPES, DEBUGGER_STATIC_METADATA_SYMBOL_DISPLAY_MAX_BYTES,
    DEBUGGER_STATIC_METADATA_TYPE_DISPLAY_MAX_BYTES,
};
use blueice_ipc::page_host::{
    self, PageHostChildStats, PageHostDebuggerBlueTsExceptionLocation,
    PageHostDebuggerBlueTsMetadataContractDisplay, PageHostDebuggerBlueTsMetadataContractId,
    PageHostDebuggerBlueTsMetadataContractLocation,
    PageHostDebuggerBlueTsMetadataContractValidation,
    PageHostDebuggerBlueTsMetadataLoweringSummary, PageHostDebuggerBlueTsMetadataSourceId,
    PageHostDebuggerBlueTsMetadataSourceProvenance, PageHostDebuggerBlueTsMetadataSummary,
    PageHostDebuggerBlueTsMetadataSymbolContract, PageHostDebuggerBlueTsMetadataSymbolDisplay,
    PageHostDebuggerBlueTsMetadataSymbolId, PageHostDebuggerBlueTsMetadataSymbolLocation,
    PageHostDebuggerBlueTsMetadataSymbolType, PageHostDebuggerBlueTsMetadataTypeDisplay,
    PageHostDebuggerBlueTsMetadataTypeId, PageHostDebuggerBlueTsSafePointSpan,
    PageHostDebuggerExecutionState, PageHostDebuggerFrame, PageHostDebuggerLinkedExecutionState,
    PageHostDebuggerLinkedFrame, PageHostDebuggerLinkedSource, PageHostDebuggerLinkedStackFrame,
    PageHostDebuggerLinkedStackSnapshot, PageHostDebuggerMetadataHandle, PageHostDebuggerProgram,
    PageHostDebuggerSafePoint, PageHostDebuggerScopeEntry, PageHostDebuggerStackFrame,
    PageHostDebuggerStackSnapshot, PageHostDebuggerStaticScopeRelation,
    PageHostDebuggerStaticScopeTarget, PageHostDebuggerValuePreview, PageHostDebuggerValueSnapshot,
    PageHostDebuggerValueTarget, PageHostDocument, PageHostDocumentSnapshot, PageHostErrorCode,
    PageHostModuleGraph, PageHostRealmStats, PageHostReply, PageHostRequest, PageHostScript,
    PageHostScriptKind, PageHostScriptLanguage, PageHostScriptOutcome, PageHostScriptReport,
    PageHostSource, PageHostStaticResolution, PAGE_HOST_DEBUGGER_MAX_BREAKPOINTS_PER_REALM,
    PAGE_HOST_DEBUGGER_MAX_SAFE_POINTS_PER_PROGRAM, PAGE_HOST_DEBUGGER_MAX_SCOPE_ENTRIES,
    PAGE_HOST_DEBUGGER_MAX_STACK_FRAMES, PAGE_HOST_DOCUMENT_ORIGIN_MAX_BYTES,
    PAGE_HOST_DOCUMENT_TEXT_MAX_BYTES,
};
use blueice_ipc::script::{
    self, ScriptDocumentTarget, ScriptReply, ScriptRequest, SCRIPT_MAX_NAME_BYTES,
    SCRIPT_MAX_TEXT_BYTES,
};
use blueice_net::canonical_http_origin;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

/// Execution/source limits independently enforced by the child. The caller
/// cannot widen them by serializing a larger graph over its private socket.
const MAX_SCRIPTS_PER_DOCUMENT: usize = 256;
const MAX_MODULES_PER_GRAPH: usize = 8;
const MAX_SOURCE_BYTES_PER_MODULE: usize = 1024 * 1024;
const MAX_SOURCE_BYTES_PER_DOCUMENT: usize = 8 * 1024 * 1024;
// A freshly rebuilt child may spend several seconds in macOS's first-launch
// executable validation before it reaches its socket bind. Keep the deadline
// finite, but do not misclassify that cold start as a dead child.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(15);
/// A child DOM callback must fail before the core's 60-second nested wait.
const SCRIPT_DOM_CALL_WAIT: Duration = Duration::from_secs(30);

/// Static metadata inventory IDs are private to the child but intentionally
/// start in a separate range from child debugger-program IDs. The type-level
/// distinction remains the primary boundary; this disjoint start additionally
/// prevents a plausible-looking numeric program ID from being replayed as a
/// metadata handle by a buggy core adapter.
const CHILD_DEBUGGER_METADATA_ID_NAMESPACE_START: u64 = 1 << 63;

/// Immutable launcher-owner limits for one isolated page-host child.
///
/// Per-realm envelopes and conservative child-wide reservations. Each live
/// realm reserves its full program, root-bytecode, and VM-managed-heap budget
/// before admission. This is not an RSS or fleet memory limit: VM accounting
/// excludes allocator, Rust, registry, source, and operating-system overhead.
/// Only the launcher may select these values before starting a core generation;
/// page, frontend, and page-host IPC cannot inspect or modify them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlueJsHostRuntimeLimits {
    pub max_realms: usize,
    pub max_programs_per_realm: usize,
    pub max_bytecode_bytes_per_realm: usize,
    pub max_heap_bytes_per_realm: usize,
    pub max_reserved_programs: usize,
    pub max_reserved_bytecode_bytes: usize,
    pub max_reserved_heap_bytes: usize,
}

impl Default for BlueJsHostRuntimeLimits {
    fn default() -> Self {
        Self::from_runtime_config(BlueJsPageRuntimeConfig::default())
    }
}

impl BlueJsHostRuntimeLimits {
    fn from_runtime_config(runtime: BlueJsPageRuntimeConfig) -> Self {
        Self {
            max_realms: runtime.max_realms,
            max_programs_per_realm: runtime.max_programs_per_realm,
            max_bytecode_bytes_per_realm: runtime.max_bytecode_bytes_per_realm,
            max_heap_bytes_per_realm: runtime.vm.heap.max_heap_bytes,
            max_reserved_programs: runtime
                .max_realms
                .saturating_mul(runtime.max_programs_per_realm),
            max_reserved_bytecode_bytes: runtime
                .max_realms
                .saturating_mul(runtime.max_bytecode_bytes_per_realm),
            max_reserved_heap_bytes: runtime
                .max_realms
                .saturating_mul(runtime.vm.heap.max_heap_bytes),
        }
    }

    /// Rebuilds the one narrow policy surface into the full BlueJS runtime
    /// configuration. All non-resource VM configuration remains the child
    /// default rather than becoming an embedding/deployment API.
    pub fn runtime_config(self) -> Result<BlueJsPageRuntimeConfig, &'static str> {
        if self.max_realms == 0
            || self.max_programs_per_realm == 0
            || self.max_bytecode_bytes_per_realm == 0
            || self.max_heap_bytes_per_realm == 0
            || self.max_reserved_programs == 0
            || self.max_reserved_bytecode_bytes == 0
            || self.max_reserved_heap_bytes == 0
        {
            return Err("BlueJS page-host runtime limits must be non-zero");
        }
        if self.max_reserved_programs < self.max_programs_per_realm
            || self.max_reserved_bytecode_bytes < self.max_bytecode_bytes_per_realm
            || self.max_reserved_heap_bytes < self.max_heap_bytes_per_realm
        {
            return Err("BlueJS child-wide reservations must cover one full realm");
        }
        let defaults = BlueJsPageRuntimeConfig::default();
        let heap = HeapConfig {
            max_heap_bytes: self.max_heap_bytes_per_realm,
            major_threshold_bytes: defaults
                .vm
                .heap
                .major_threshold_bytes
                .min(self.max_heap_bytes_per_realm),
            ..defaults.vm.heap
        };
        let runtime = BlueJsPageRuntimeConfig {
            vm: VmConfig {
                heap,
                ..defaults.vm
            },
            max_realms: self.max_realms,
            max_programs_per_realm: self.max_programs_per_realm,
            max_bytecode_bytes_per_realm: self.max_bytecode_bytes_per_realm,
        };
        // PageRuntime validates its own count and byte bounds, while a VM
        // would otherwise defer malformed heap tuning until the first realm.
        Vm::new(runtime.vm).map_err(|_| "BlueJS page-host heap limits are invalid")?;
        BlueJsPageRuntime::new(runtime)
            .map(|_| runtime)
            .map_err(|_| "BlueJS page-host runtime limits are invalid")
    }
}

struct LiveDocument {
    generation: u64,
    origin: BlueJsPageOrigin,
    click_event_family: Option<HostObjectFamily>,
    pending_click_tasks: VecDeque<PendingClickTask>,
    debugger_execution_control: bool,
    debugger_programs: BTreeMap<u64, ChildDebuggerProgram>,
    /// Exact child-private breakpoint configuration records. These are not a
    /// VM interruption hook; replacing or closing the realm drops them.
    debugger_breakpoints: BTreeSet<PageHostDebuggerSafePoint>,
    pending_debugger_executions: VecDeque<PendingDebuggerExecution>,
    debugger_execution_states: BTreeMap<PageHostDebuggerProgram, ChildDebuggerExecutionStatus>,
}

/// A core-originated click is bound to its original document, never to a
/// successor realm or a page-supplied target.
#[derive(Clone, Copy)]
struct PendingClickTask {
    generation: u64,
    node_id: u64,
}

// Page-host requests are sequential today: the child must complete the
// current click task and its checkpoint before replying to core. Retain an
// explicit, one-slot per-realm queue so future scheduling cannot silently
// turn this into an unbounded collection of pending page work.
const MAX_PENDING_CLICK_TASKS_PER_REALM: usize = 1;

/// A source-span step can consume no more than this many root instructions
/// before yielding the still-live continuation at an exact root boundary.
/// The count is child-fixed and cannot be widened by a page or debugger peer.
const MAX_BLUETS_SOURCE_STEP_ROOT_INSTRUCTIONS: u16 = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BlueTsSourceSpanKey {
    source_id: u32,
    start_byte: u32,
    end_byte: u32,
}

/// Private child-only association between a child-minted opaque debugger
/// identity and its BlueJS registry handle. The registry handle never leaves
/// this process; the core separately mints its public debugger identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ChildDebuggerProgram {
    program_generation: u64,
    runtime_handle: BlueJsProgramHandle,
    /// Created only on the first authenticated metadata-inventory request
    /// while the matching BlueTS registry attachment is still live. A plain
    /// JavaScript program never receives one.
    metadata: Option<PageHostDebuggerMetadataHandle>,
    /// Snapshotted before another realm execution can replace the VM sidecar.
    /// The entire record remains private to this document and child program.
    exception_location: Option<ChildDebuggerExceptionLocation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChildRootSlotLookupError {
    UnknownDocument,
    StaleDocument,
    InvalidProgram,
    InvalidMetadata,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ChildDebuggerExceptionLocation {
    safe_point: PageHostDebuggerSafePoint,
    span: PageHostDebuggerBlueTsSafePointSpan,
}

/// Child-private execution state for one retained classic or module root.
/// It has no serializable VM frame, source, bytecode, or completion value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChildDebuggerExecutionStatus {
    Pending,
    Paused(PageHostDebuggerSafePoint),
    NestedPaused {
        frame: BlueJsPageDebuggerFrame,
        safe_point: PageHostDebuggerSafePoint,
    },
    NestedStepRequested {
        frame: BlueJsPageDebuggerFrame,
        safe_point: PageHostDebuggerSafePoint,
    },
    NestedResumeRequested {
        frame: BlueJsPageDebuggerFrame,
        safe_point: PageHostDebuggerSafePoint,
    },
    LinkedPaused {
        frame: BlueJsPageDebuggerLinkedFrame,
        safe_point: PageHostDebuggerSafePoint,
    },
    LinkedResumeRequested {
        frame: BlueJsPageDebuggerLinkedFrame,
        safe_point: PageHostDebuggerSafePoint,
    },
    StepRequested,
    BlueTsSourceStepRequested {
        origin: BlueTsSourceSpanKey,
        remaining: u16,
    },
    SourceStepLimitReached(PageHostDebuggerSafePoint),
    ResumeRequested,
    Completed,
}

/// A child-local cross-program stack; the v39 same-program wire never sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ChildLinkedStackFrame {
    safe_point: PageHostDebuggerSafePoint,
    scope_entries: Vec<PageHostDebuggerScopeEntry>,
    scope_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChildLinkedStackSnapshot {
    frames: [ChildLinkedStackFrame; 2],
    stack_truncated: bool,
    max_scope_entries: u32,
}

impl ChildLinkedStackSnapshot {
    fn to_wire(&self) -> PageHostDebuggerLinkedStackSnapshot {
        PageHostDebuggerLinkedStackSnapshot {
            frames: self
                .frames
                .clone()
                .map(|frame| PageHostDebuggerLinkedStackFrame {
                    safe_point: frame.safe_point,
                    scope_entries: frame.scope_entries,
                    scope_truncated: frame.scope_truncated,
                }),
            stack_truncated: self.stack_truncated,
            max_scope_entries: self.max_scope_entries,
        }
    }

    fn from_wire(snapshot: &PageHostDebuggerLinkedStackSnapshot) -> Self {
        Self {
            frames: snapshot.frames.clone().map(|frame| ChildLinkedStackFrame {
                safe_point: frame.safe_point,
                scope_entries: frame.scope_entries,
                scope_truncated: frame.scope_truncated,
            }),
            stack_truncated: snapshot.stack_truncated,
            max_scope_entries: snapshot.max_scope_entries,
        }
    }
}

/// A document-order declaration retained by the child in an explicitly
/// core-selected debugger-execution document. Classic programs are admitted
/// before the first advance so core can discover an opaque identity; a BlueTS
/// ESM entry is also admitted with its graph. All source-bearing data remains
/// in this child-only queue.
enum DeferredChildExecution {
    JavaScriptClassic {
        handle: BlueJsProgramHandle,
        root_safe_point: Option<PageHostDebuggerSafePoint>,
    },
    JavaScriptModule {
        graph: PageHostModuleGraph,
        programs: BTreeMap<String, BlueJsProgramV1>,
    },
    BlueTsClassic {
        handle: BlueJsProgramHandle,
        root_safe_point: Option<PageHostDebuggerSafePoint>,
    },
    BlueTsModule {
        attachment: DirectPageModuleGraphAttachment,
        root_safe_point: Option<PageHostDebuggerSafePoint>,
    },
}

struct PendingDebuggerExecution {
    ordinal: u32,
    language: PageHostScriptLanguage,
    kind: PageHostScriptKind,
    program: Option<PageHostDebuggerProgram>,
    nested_safe_point: Option<PageHostDebuggerSafePoint>,
    linked_safe_point: Option<PageHostDebuggerSafePoint>,
    execution: DeferredChildExecution,
}

#[derive(Clone)]
struct ScriptDomCapability {
    socket_path: PathBuf,
    session_token: String,
    enable_lookup_probe: bool,
    enable_dom_text_profile: bool,
    enable_dom_mutation_profile: bool,
    enable_dom_event_profile: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PageDomProfile {
    Snapshot,
    Text,
    Mutation,
    Event,
}

impl ScriptDomCapability {
    fn profile(&self) -> PageDomProfile {
        if self.enable_dom_event_profile {
            PageDomProfile::Event
        } else if self.enable_dom_mutation_profile {
            PageDomProfile::Mutation
        } else if self.enable_dom_text_profile {
            PageDomProfile::Text
        } else {
            PageDomProfile::Snapshot
        }
    }
}

/// Child-only script IPC. The core still owns every DOM node and validates
/// the exact target; this client never exposes a raw node ID to page code.
struct ScriptDomClient {
    capability: ScriptDomCapability,
    stream: Option<UnixStream>,
    next_call_id: u64,
}

impl ScriptDomClient {
    fn new(capability: ScriptDomCapability) -> Self {
        Self {
            capability,
            stream: None,
            next_call_id: 1,
        }
    }

    fn connect(&self) -> io::Result<UnixStream> {
        let mut stream = UnixStream::connect(&self.capability.socket_path)?;
        stream.set_read_timeout(Some(SCRIPT_DOM_CALL_WAIT))?;
        stream.set_write_timeout(Some(SCRIPT_DOM_CALL_WAIT))?;
        script::write_script_request(
            &mut stream,
            &ScriptRequest::Hello {
                protocol_version: script::SCRIPT_PROTOCOL_VERSION,
                session_token: self.capability.session_token.clone(),
            },
        )?;
        match script::read_script_reply(&mut stream)? {
            ScriptReply::HelloAck {
                protocol_version: script::SCRIPT_PROTOCOL_VERSION,
            } => Ok(stream),
            _ => Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "core rejected child script capability",
            )),
        }
    }

    fn has_element_by_id(&mut self, target: ScriptDocumentTarget, id: String) -> io::Result<bool> {
        self.get_element_by_id(target, id)
            .map(|node| node.is_some())
    }

    fn get_element_by_id(
        &mut self,
        target: ScriptDocumentTarget,
        id: String,
    ) -> io::Result<Option<u64>> {
        if id.len() > SCRIPT_MAX_NAME_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "script DOM lookup name exceeds its fixed limit",
            ));
        }
        match self.call(target, ScriptRequest::GetElementById { target, id })? {
            ScriptReply::Node { node } => Ok(node),
            ScriptReply::Error { .. } => {
                self.stream = None;
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "core denied child DOM lookup",
                ))
            }
            _ => {
                self.stream = None;
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "core returned an invalid child DOM lookup result",
                ))
            }
        }
    }

    fn validate_node(&mut self, target: ScriptDocumentTarget, node: u64) -> io::Result<()> {
        match self.call(target, ScriptRequest::ValidateNode { target, node })? {
            ScriptReply::Ack => Ok(()),
            ScriptReply::Error { .. } => {
                self.stream = None;
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "core denied stale child DOM node",
                ))
            }
            _ => {
                self.stream = None;
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "core returned an invalid child DOM validation result",
                ))
            }
        }
    }

    fn create_element(
        &mut self,
        target: ScriptDocumentTarget,
        tag_name: String,
    ) -> io::Result<u64> {
        if tag_name.len() > SCRIPT_MAX_NAME_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "script DOM tag name exceeds its fixed limit",
            ));
        }
        self.created_node(target, ScriptRequest::CreateElement { target, tag_name })
    }

    fn create_text_node(&mut self, target: ScriptDocumentTarget, data: String) -> io::Result<u64> {
        if data.len() > SCRIPT_MAX_TEXT_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "script DOM text exceeds its fixed limit",
            ));
        }
        self.created_node(target, ScriptRequest::CreateTextNode { target, data })
    }

    fn created_node(
        &mut self,
        target: ScriptDocumentTarget,
        request: ScriptRequest,
    ) -> io::Result<u64> {
        match self.call(target, request)? {
            ScriptReply::NodeCreated { node } => Ok(node),
            ScriptReply::Error { .. } => {
                self.stream = None;
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "core denied child DOM node creation",
                ))
            }
            _ => {
                self.stream = None;
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "core returned an invalid child DOM creation result",
                ))
            }
        }
    }

    fn append_child(
        &mut self,
        target: ScriptDocumentTarget,
        parent: u64,
        child: u64,
    ) -> io::Result<()> {
        match self.call(
            target,
            ScriptRequest::AppendChild {
                target,
                parent,
                child,
            },
        )? {
            ScriptReply::Ack => Ok(()),
            ScriptReply::Error { .. } => {
                self.stream = None;
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "core denied child DOM append",
                ))
            }
            _ => {
                self.stream = None;
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "core returned an invalid child DOM append result",
                ))
            }
        }
    }

    fn get_text_content(&mut self, target: ScriptDocumentTarget, node: u64) -> io::Result<String> {
        match self.call(target, ScriptRequest::GetTextContent { target, node })? {
            ScriptReply::Text { value } if value.len() <= SCRIPT_MAX_TEXT_BYTES => Ok(value),
            ScriptReply::Error { .. } => {
                self.stream = None;
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "core denied child DOM text read",
                ))
            }
            _ => {
                self.stream = None;
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "core returned an invalid child DOM text result",
                ))
            }
        }
    }

    fn set_text_content(
        &mut self,
        target: ScriptDocumentTarget,
        node: u64,
        value: String,
    ) -> io::Result<()> {
        if value.len() > SCRIPT_MAX_TEXT_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "script DOM text exceeds its fixed limit",
            ));
        }
        match self.call(
            target,
            ScriptRequest::SetTextContent {
                target,
                node,
                value,
            },
        )? {
            ScriptReply::Ack => Ok(()),
            ScriptReply::Error { .. } => {
                self.stream = None;
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "core denied child DOM text write",
                ))
            }
            _ => {
                self.stream = None;
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "core returned an invalid child DOM text mutation result",
                ))
            }
        }
    }

    fn call(
        &mut self,
        target: ScriptDocumentTarget,
        request: ScriptRequest,
    ) -> io::Result<ScriptReply> {
        if self.stream.is_none() {
            self.stream = Some(self.connect()?);
            self.next_call_id = 1;
        }
        let request_id = self.next_call_id;
        let result = (|| {
            let stream = self
                .stream
                .as_mut()
                .expect("script stream was just connected");
            script::write_script_request(
                stream,
                &ScriptRequest::Call {
                    request_id,
                    request: Box::new(request),
                },
            )?;
            match script::read_script_reply(stream)? {
                ScriptReply::CallResult {
                    request_id: reply_id,
                    target: reply_target,
                    reply,
                } if reply_id == request_id && reply_target == target => Ok(*reply),
                _ => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "core returned a reply for a different script call or document",
                )),
            }
        })();
        if result.is_err() {
            self.stream = None;
        } else {
            let Some(next_call_id) = self.next_call_id.checked_add(1) else {
                self.stream = None;
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "script call ID space exhausted",
                ));
            };
            self.next_call_id = next_call_id;
        }
        result
    }
}

/// The actual state machine running in the child process.
///
/// It accepts only fully selected source records. It has no filesystem or
/// network resolver and gives the parent no VM/program handle. Ordinary
/// realms install only the two fixed core-validated string snapshots; an
/// explicit owner-only proof profile can additionally make boolean or
/// opaque-wrapper DOM lookups, live text reads/writes, or bounded creation
/// and append through the private script socket without exposing numeric
/// node IDs to page code.
pub struct BlueJsChildHost {
    runtime: BlueJsPageRuntime,
    limits: BlueJsHostRuntimeLimits,
    /// Static BlueTS metadata paired with the exact child-local BlueJS
    /// generation that direct lowering admitted. This remains entirely in the
    /// child: the page-host protocol has no source, symbol, type, contract,
    /// or runtime-value inspection operation.
    debug_registry: DirectDebugRegistry,
    documents: BTreeMap<u64, LiveDocument>,
    next_debugger_program_handle: u64,
    next_debugger_program_generation: u64,
    next_debugger_metadata_handle: u64,
    next_debugger_metadata_generation: u64,
    script_dom_capability: Option<ScriptDomCapability>,
}

mod execution_drive;

mod debugger_inspection;

mod debugger_control;

mod bluets_metadata_details;

mod bluets_metadata_catalog;

mod host_lifecycle;

mod host_supervisor;
pub use host_supervisor::{BlueJsHostCoreConfig, SpawnedBlueJsHost};

mod debugger_mapping;
use debugger_mapping::*;

mod script_execution;
use script_execution::*;

impl BlueJsChildHost {
    fn exact_document(
        &self,
        tab_id: u64,
        document_generation: u64,
    ) -> Result<&LiveDocument, PageHostReply> {
        match self.documents.get(&tab_id) {
            None => Err(unknown_realm()),
            Some(document) if document.generation != document_generation => Err(stale_document()),
            Some(document) => Ok(document),
        }
    }

    fn refresh_debugger_programs(&mut self, tab_id: u64) -> Result<(), ()> {
        let runtime_handles = self.runtime.program_handles(tab_id).map_err(|_| ())?;
        for runtime_handle in runtime_handles {
            self.register_debugger_program(tab_id, runtime_handle)?;
        }
        Ok(())
    }

    /// Returns a stable child-private identity for one currently admitted
    /// runtime program. Refreshes never remint an existing handle: core's
    /// public mapping therefore remains bound to the exact private tuple.
    fn register_debugger_program(
        &mut self,
        tab_id: u64,
        runtime_handle: BlueJsProgramHandle,
    ) -> Result<PageHostDebuggerProgram, ()> {
        if let Some((program_handle, record)) = self
            .documents
            .get(&tab_id)
            .ok_or(())?
            .debugger_programs
            .iter()
            .find(|(_, record)| record.runtime_handle == runtime_handle)
        {
            return Ok(PageHostDebuggerProgram {
                program_handle: *program_handle,
                program_generation: record.program_generation,
            });
        }
        let program_handle = self.next_debugger_program_handle;
        let program_generation = self.next_debugger_program_generation;
        // Keep the numerical ranges disjoint for the process lifetime as a
        // defense in depth beyond the distinct wire types. A pathological
        // child that exhausts the lower program-ID space fails closed rather
        // than minting a value that could resemble a metadata handle.
        if program_handle >= CHILD_DEBUGGER_METADATA_ID_NAMESPACE_START
            || program_generation >= CHILD_DEBUGGER_METADATA_ID_NAMESPACE_START
        {
            return Err(());
        }
        self.next_debugger_program_handle = program_handle.checked_add(1).ok_or(())?;
        self.next_debugger_program_generation = program_generation.checked_add(1).ok_or(())?;
        self.documents
            .get_mut(&tab_id)
            .ok_or(())?
            .debugger_programs
            .insert(
                program_handle,
                ChildDebuggerProgram {
                    program_generation,
                    runtime_handle,
                    metadata: None,
                    exception_location: None,
                },
            );
        Ok(PageHostDebuggerProgram {
            program_handle,
            program_generation,
        })
    }

    /// Mints an inventory-only child metadata identity. Callers must first
    /// prove the matching registry attachment remains live; this helper never
    /// receives or derives compiler metadata.
    fn mint_debugger_metadata_handle(&mut self) -> Result<PageHostDebuggerMetadataHandle, ()> {
        let metadata_handle = self.next_debugger_metadata_handle;
        let metadata_generation = self.next_debugger_metadata_generation;
        self.next_debugger_metadata_handle = metadata_handle.checked_add(1).ok_or(())?;
        self.next_debugger_metadata_generation = metadata_generation.checked_add(1).ok_or(())?;
        Ok(PageHostDebuggerMetadataHandle {
            metadata_handle,
            metadata_generation,
        })
    }
}

impl Default for BlueJsChildHost {
    fn default() -> Self {
        Self::new().expect("the default BlueJS child-host configuration is valid")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DocumentSnapshotError {
    Invalid,
    ResourceLimit,
}

/// Repeats the fixed core-side snapshot checks before a child VM observes a
/// value. Parsing this already-serialized string does not give the child a
/// URL object, resolver, or network operation: `canonical_http_origin` is a
/// pure syntax/canonicalization check and this host never calls `fetch`.
fn validated_document_origin(
    snapshot: &PageHostDocumentSnapshot,
) -> Result<BlueJsPageOrigin, DocumentSnapshotError> {
    if snapshot.document_text.len() > PAGE_HOST_DOCUMENT_TEXT_MAX_BYTES
        || snapshot.document_origin.len() > PAGE_HOST_DOCUMENT_ORIGIN_MAX_BYTES
    {
        return Err(DocumentSnapshotError::ResourceLimit);
    }
    let canonical = canonical_http_origin(&snapshot.document_origin)
        .map_err(|_| DocumentSnapshotError::Invalid)?;
    if canonical != snapshot.document_origin {
        return Err(DocumentSnapshotError::Invalid);
    }
    BlueJsPageOrigin::new(canonical).map_err(|_| DocumentSnapshotError::Invalid)
}

/// Installs the immutable snapshots and, only under an owner-selected
/// capability, an exact live DOM inventory compiled into BlueTS.
/// Neither raw node IDs nor the socket capability reach page code.
fn install_document_snapshot_bindings(
    runtime: &mut BlueJsPageRuntime,
    tab_id: u64,
    document_generation: u64,
    snapshot: &PageHostDocumentSnapshot,
    script_dom_capability: Option<ScriptDomCapability>,
) -> Result<Option<HostObjectFamily>, BlueJsPageRuntimeError> {
    let page_dom_profile = script_dom_capability
        .as_ref()
        .map_or(PageDomProfile::Snapshot, ScriptDomCapability::profile);
    let artifact = match page_dom_profile {
        PageDomProfile::Snapshot => PageHostDocumentTypingsV1::generate(),
        PageDomProfile::Text => PageHostDocumentTypingsV1::generate_dom_text(),
        PageDomProfile::Mutation => PageHostDocumentTypingsV1::generate_dom_mutation(),
        PageDomProfile::Event => PageHostDocumentTypingsV1::generate_dom_event(),
    };
    let expected_bindings = match page_dom_profile {
        PageDomProfile::Snapshot => page_host_document_runtime_bindings_v1().to_vec(),
        PageDomProfile::Text => page_host_dom_text_runtime_bindings_v1().to_vec(),
        PageDomProfile::Mutation => page_host_dom_mutation_runtime_bindings_v1().to_vec(),
        PageDomProfile::Event => page_host_dom_event_runtime_bindings_v1().to_vec(),
    };
    artifact
        .verify_runtime_bindings(&expected_bindings)
        .map_err(|_| BlueJsPageRuntimeError::InvalidConfiguration)?;
    let binding_inventory = page_host_document_runtime_bindings_v1();
    let document_text = snapshot.document_text.clone();
    let document_origin = snapshot.document_origin.clone();
    let click_event_family = Rc::new(RefCell::new(None));
    let installed_click_event_family = Rc::clone(&click_event_family);
    runtime.configure_realm_bindings(tab_id, move |bindings| {
        let mut installed_bindings = Vec::from(binding_inventory);
        for binding in binding_inventory {
            match (binding.stable_id, binding.runtime_binding_id) {
                ("dom.document-origin", "global.blueiceDocumentOrigin") => {
                    let document_origin = document_origin.clone();
                    bindings.install_global_function(
                        "blueiceDocumentOrigin",
                        0,
                        move |arguments: &[HostValue]| {
                            require_no_arguments(arguments, "blueiceDocumentOrigin")?;
                            Ok(HostValue::String(document_origin.clone().into()))
                        },
                    )?;
                }
                ("dom.document-text", "global.blueiceDocumentText") => {
                    let document_text = document_text.clone();
                    bindings.install_global_function(
                        "blueiceDocumentText",
                        0,
                        move |arguments: &[HostValue]| {
                            require_no_arguments(arguments, "blueiceDocumentText")?;
                            Ok(HostValue::String(document_text.clone().into()))
                        },
                    )?;
                }
                _ => {
                    return Err(RuntimeError::Unsupported(
                        "page-host typing inventory has no callback installer",
                    ));
                }
            }
        }
        if let Some(capability) = script_dom_capability
            .clone()
            .filter(|capability| capability.enable_lookup_probe)
        {
            let client = Rc::new(RefCell::new(ScriptDomClient::new(capability)));
            let target = ScriptDocumentTarget {
                tab_id,
                document_generation,
            };
            let boolean_client = Rc::clone(&client);
            bindings.install_global_function(
                "blueiceTestHasElementById",
                1,
                move |arguments: &[HostValue]| {
                    let id = dom_lookup_id(arguments, "blueiceTestHasElementById")?;
                    boolean_client
                        .borrow_mut()
                        .has_element_by_id(target, id)
                        .map(HostValue::Bool)
                        .map_err(|_| HostFunctionError::new("child DOM lookup unavailable"))
                },
            )?;
            let family = bindings.create_host_object_family()?;
            let validation_client = Rc::clone(&client);
            bindings.install_host_object_method(
                family,
                "blueiceTestRequireLive",
                0,
                move |key: HostObjectKey, arguments: &[HostValue]| {
                    require_no_arguments(arguments, "blueiceTestRequireLive")?;
                    if !key.matches_owner(tab_id, document_generation) {
                        return Err(HostFunctionError::new(
                            "child DOM node belongs to another document",
                        ));
                    }
                    validation_client
                        .borrow_mut()
                        .validate_node(target, key.object())
                        .map_err(|_| HostFunctionError::new("child DOM node is no longer live"))?;
                    Ok(HostValue::Bool(true))
                },
            )?;
            bindings.install_global_object_factory(
                "blueiceTestGetElementById",
                1,
                family,
                move |arguments: &[HostValue]| {
                    let id = dom_lookup_id(arguments, "blueiceTestGetElementById")?;
                    client
                        .borrow_mut()
                        .get_element_by_id(target, id)
                        .map(|node| {
                            node.map(|node| HostObjectKey::new(tab_id, document_generation, node))
                        })
                        .map_err(|_| HostFunctionError::new("child DOM lookup unavailable"))
                },
            )?;
        }
        if let Some(capability) = script_dom_capability
            .filter(|capability| capability.profile() != PageDomProfile::Snapshot)
        {
            let target = ScriptDocumentTarget {
                tab_id,
                document_generation,
            };
            let client = Rc::new(RefCell::new(ScriptDomClient::new(capability)));
            let document = bindings.install_global_object("document")?;
            let family = bindings.create_host_object_family()?;
            let lookup_client = Rc::clone(&client);
            bindings.install_host_object_factory_method(
                document,
                "getElementById",
                1,
                family,
                move |arguments: &[HostValue]| {
                    let id = dom_lookup_id(arguments, "document.getElementById")?;
                    lookup_client
                        .borrow_mut()
                        .get_element_by_id(target, id)
                        .map(|node| {
                            node.map(|node| HostObjectKey::new(tab_id, document_generation, node))
                        })
                        .map_err(|_| HostFunctionError::new("child DOM lookup unavailable"))
                },
            )?;
            let read_client = Rc::clone(&client);
            let write_client = Rc::clone(&client);
            bindings.install_host_object_accessor(
                family,
                "textContent",
                move |key: HostObjectKey, arguments: &[HostValue]| {
                    require_no_arguments(arguments, "node.textContent getter")?;
                    if !key.matches_owner(tab_id, document_generation) {
                        return Err(HostFunctionError::new(
                            "child DOM node belongs to another document",
                        ));
                    }
                    read_client
                        .borrow_mut()
                        .get_text_content(target, key.object())
                        .map(|text| HostValue::String(text.into()))
                        .map_err(|_| HostFunctionError::new("child DOM text read unavailable"))
                },
                move |key: HostObjectKey, arguments: &[HostValue]| {
                    let [HostValue::String(value)] = arguments else {
                        return Err(HostFunctionError::new("node.textContent requires a string"));
                    };
                    if !key.matches_owner(tab_id, document_generation) {
                        return Err(HostFunctionError::new(
                            "child DOM node belongs to another document",
                        ));
                    }
                    let value = value.to_utf8().map_err(|_| {
                        HostFunctionError::new("node.textContent requires valid UTF-16")
                    })?;
                    write_client
                        .borrow_mut()
                        .set_text_content(target, key.object(), value)
                        .map_err(|_| HostFunctionError::new("child DOM text write unavailable"))?;
                    Ok(HostValue::Undefined)
                },
            )?;
            if matches!(
                page_dom_profile,
                PageDomProfile::Mutation | PageDomProfile::Event
            ) {
                let element_client = Rc::clone(&client);
                bindings.install_host_object_factory_method(
                    document,
                    "createElement",
                    1,
                    family,
                    move |arguments: &[HostValue]| {
                        let tag_name = dom_string_argument(arguments, "document.createElement")?;
                        element_client
                            .borrow_mut()
                            .create_element(target, tag_name)
                            .map(|node| Some(HostObjectKey::new(tab_id, document_generation, node)))
                            .map_err(|_| {
                                HostFunctionError::new("child DOM element creation unavailable")
                            })
                    },
                )?;
                let text_client = Rc::clone(&client);
                bindings.install_host_object_factory_method(
                    document,
                    "createTextNode",
                    1,
                    family,
                    move |arguments: &[HostValue]| {
                        let data = dom_string_argument(arguments, "document.createTextNode")?;
                        text_client
                            .borrow_mut()
                            .create_text_node(target, data)
                            .map(|node| Some(HostObjectKey::new(tab_id, document_generation, node)))
                            .map_err(|_| {
                                HostFunctionError::new("child DOM text-node creation unavailable")
                            })
                    },
                )?;
                bindings.install_host_object_pair_method(
                    family,
                    "appendChild",
                    1,
                    move |parent: HostObjectKey, child: HostObjectKey| {
                        if !parent.matches_owner(tab_id, document_generation)
                            || !child.matches_owner(tab_id, document_generation)
                        {
                            return Err(HostFunctionError::new(
                                "child DOM node belongs to another document",
                            ));
                        }
                        client
                            .borrow_mut()
                            .append_child(target, parent.object(), child.object())
                            .map_err(|_| HostFunctionError::new("child DOM append unavailable"))
                    },
                )?;
                installed_bindings
                    .extend_from_slice(&page_host_dom_mutation_runtime_bindings_v1()[2..]);
                if page_dom_profile == PageDomProfile::Event {
                    bindings.install_host_click_event_methods(family)?;
                    *installed_click_event_family.borrow_mut() = Some(family);
                    let event_bindings = page_host_dom_event_runtime_bindings_v1();
                    installed_bindings.extend([event_bindings[6], event_bindings[8]]);
                }
            } else {
                installed_bindings
                    .extend_from_slice(&page_host_dom_text_runtime_bindings_v1()[2..]);
            }
        }
        artifact
            .verify_runtime_bindings(&installed_bindings)
            .map_err(|_| RuntimeError::TypeError("host binding inventory mismatch".into()))?;
        Ok(())
    })?;
    let family = *click_event_family.borrow();
    Ok(family)
}

fn dom_lookup_id(arguments: &[HostValue], function: &str) -> Result<String, HostFunctionError> {
    let [HostValue::String(id)] = arguments else {
        return Err(HostFunctionError::new(format!(
            "{function} requires one string ID"
        )));
    };
    id.to_utf8()
        .map_err(|_| HostFunctionError::new("DOM lookup ID must be valid UTF-16"))
}

fn dom_string_argument(
    arguments: &[HostValue],
    function: &str,
) -> Result<String, HostFunctionError> {
    let [HostValue::String(value)] = arguments else {
        return Err(HostFunctionError::new(format!(
            "{function} requires one string argument"
        )));
    };
    value
        .to_utf8()
        .map_err(|_| HostFunctionError::new("DOM string argument must be valid UTF-16"))
}

fn require_no_arguments(arguments: &[HostValue], function: &str) -> Result<(), HostFunctionError> {
    if arguments.is_empty() {
        Ok(())
    } else {
        Err(HostFunctionError::new(format!(
            "{function} requires no arguments"
        )))
    }
}

fn invalid_request() -> PageHostReply {
    PageHostReply::Error {
        code: PageHostErrorCode::InvalidRequest,
        message: "BlueJS page-host request is invalid".to_string(),
    }
}

fn invalid_debugger_state() -> PageHostReply {
    PageHostReply::Error {
        code: PageHostErrorCode::InvalidDebuggerState,
        message: "BlueJS page-host debugger execution state is invalid".to_string(),
    }
}

fn stale_document() -> PageHostReply {
    PageHostReply::Error {
        code: PageHostErrorCode::StaleDocument,
        message: "BlueJS page-host document generation is stale".to_string(),
    }
}

fn unknown_realm() -> PageHostReply {
    PageHostReply::Error {
        code: PageHostErrorCode::UnknownRealm,
        message: "BlueJS page-host realm is unavailable".to_string(),
    }
}

fn resource_limit() -> PageHostReply {
    PageHostReply::Error {
        code: PageHostErrorCode::ResourceLimit,
        message: "BlueJS page-host request exceeds configured policy".to_string(),
    }
}

fn host_failure() -> PageHostReply {
    PageHostReply::Error {
        code: PageHostErrorCode::HostFailure,
        message: "BlueJS page-host could not activate the requested realm".to_string(),
    }
}

/// Serves one launcher-owned connection. The first decoded request must prove
/// possession of the per-spawn session capability before it can affect a
/// realm. A disconnect ends only this peer; the launcher owns restart policy.
pub fn serve_bluejs_host_connection(
    mut stream: UnixStream,
    session_token: &str,
    host: &mut BlueJsChildHost,
) -> io::Result<bool> {
    let first = page_host::read_page_host_request(&mut stream)?;
    let accepted = matches!(
        page_host::negotiate(&first, session_token),
        PageHostReply::HelloAck { .. }
    );
    let reply = page_host::negotiate(&first, session_token);
    page_host::write_page_host_reply(&mut stream, &reply)?;
    if !accepted {
        return Ok(false);
    }

    loop {
        let request = match page_host::read_page_host_request(&mut stream) {
            Ok(request) => request,
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(false),
            Err(error) => return Err(error),
        };
        let shutting_down = matches!(request, PageHostRequest::Shutdown);
        let reply = host.handle_request(request);
        page_host::write_page_host_reply(&mut stream, &reply)?;
        if shutting_down {
            return Ok(true);
        }
    }
}

/// Runs the child listener until its authenticated launcher asks it to shut
/// down. The listener is bound only at a launcher-created `0600` path and an
/// attacker cannot use a competing same-user connection without the session
/// token. A rejected peer cannot terminate the child or consume a realm.
pub fn serve_bluejs_host_listener(
    listener: UnixListener,
    session_token: String,
    host: &mut BlueJsChildHost,
) -> io::Result<()> {
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => match serve_bluejs_host_connection(stream, &session_token, host) {
                Ok(true) => return Ok(()),
                Ok(false) => continue,
                Err(_) => continue,
            },
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// Binds the child socket with owner-only filesystem permissions. The secret
/// session token remains mandatory because pathname permission alone does not
/// distinguish another process of the same user.
pub fn bind_bluejs_host_socket(path: &Path) -> io::Result<UnixListener> {
    let _ = fs::remove_file(path);
    let listener = UnixListener::bind(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

#[cfg(test)]
mod tests;
