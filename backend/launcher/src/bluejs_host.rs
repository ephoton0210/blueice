// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Launcher supervision and execution for the first isolated BlueJS page
//! host.
//!
//! [`SpawnedBlueJsHost`] owns a separate `blueice-bluejs-host` child, its
//! private Unix socket, and its one-time connection capability. The child
//! owns every [`BlueJsPageRuntime`] VM and program registry; no `Vm`, source
//! text, program handle, DOM object, or runtime value crosses back into the
//! launcher. A future core page-loader adapter is responsible for deriving
//! [`PageHostDocument`] from an already-authorized navigation. This module
//! deliberately does not let a page or a frontend connect to the child.

use blueice_bluejs::{
    parse, parse_module, BlueJsPageDebuggerExecutionState, BlueJsPageOrigin, BlueJsPageRuntime,
    BlueJsPageRuntimeConfig, BlueJsPageRuntimeError, BlueJsProgramHandle, BlueJsProgramV1,
    BlueJsSourceIdentity, CompileError, HeapConfig, HostFunctionError, HostObjectKey, HostValue,
    Module, ParseError, RuntimeError, Value, Vm, VmConfig,
};
use blueice_bluets::{
    AuthorizedModule, AuthorizedModuleLoader, AuthorizedModuleResolution, CompilerOptions,
    Contract, ContractPlan, ContractValue, RuntimePolicy, ValidationLimits,
};
use blueice_bluets_bluejs::page_host_typings::{
    page_host_document_runtime_bindings_v1, page_host_dom_mutation_runtime_bindings_v1,
    page_host_dom_text_runtime_bindings_v1, PageHostDocumentTypingsV1,
};
use blueice_bluets_bluejs::{
    compile_direct_module_graph, compile_direct_script, BridgeError, DirectDebugRegistry,
    DirectModuleGraph, DirectSafePointBinding, DirectScript,
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
    self, PageHostChildStats, PageHostDebuggerBlueTsMetadataContractDisplay,
    PageHostDebuggerBlueTsMetadataContractId, PageHostDebuggerBlueTsMetadataContractLocation,
    PageHostDebuggerBlueTsMetadataContractValidation,
    PageHostDebuggerBlueTsMetadataLoweringSummary, PageHostDebuggerBlueTsMetadataSourceId,
    PageHostDebuggerBlueTsMetadataSourceProvenance, PageHostDebuggerBlueTsMetadataSummary,
    PageHostDebuggerBlueTsMetadataSymbolContract, PageHostDebuggerBlueTsMetadataSymbolDisplay,
    PageHostDebuggerBlueTsMetadataSymbolId, PageHostDebuggerBlueTsMetadataSymbolLocation,
    PageHostDebuggerBlueTsMetadataSymbolType, PageHostDebuggerBlueTsMetadataTypeDisplay,
    PageHostDebuggerBlueTsMetadataTypeId, PageHostDebuggerBlueTsSafePointSpan,
    PageHostDebuggerExecutionState, PageHostDebuggerMetadataHandle, PageHostDebuggerProgram,
    PageHostDebuggerSafePoint, PageHostDocument, PageHostDocumentSnapshot, PageHostErrorCode,
    PageHostModuleGraph, PageHostRealmStats, PageHostReply, PageHostRequest, PageHostScript,
    PageHostScriptKind, PageHostScriptLanguage, PageHostScriptOutcome, PageHostScriptReport,
    PageHostSource, PageHostStaticResolution, PAGE_HOST_DEBUGGER_MAX_BREAKPOINTS_PER_REALM,
    PAGE_HOST_DEBUGGER_MAX_SAFE_POINTS_PER_PROGRAM, PAGE_HOST_DOCUMENT_ORIGIN_MAX_BYTES,
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
    debugger_execution_control: bool,
    debugger_programs: BTreeMap<u64, ChildDebuggerProgram>,
    /// Exact child-private breakpoint configuration records. These are not a
    /// VM interruption hook; replacing or closing the realm drops them.
    debugger_breakpoints: BTreeSet<PageHostDebuggerSafePoint>,
    pending_debugger_executions: VecDeque<PendingDebuggerExecution>,
    debugger_execution_states: BTreeMap<PageHostDebuggerProgram, ChildDebuggerExecutionStatus>,
}

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
}

/// Child-private execution state for the narrow root-classic continuation.
/// It has no serializable VM frame, source, bytecode, or completion value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChildDebuggerExecutionStatus {
    Pending,
    Paused(PageHostDebuggerSafePoint),
    StepRequested,
    BlueTsSourceStepRequested {
        origin: BlueTsSourceSpanKey,
        remaining: u16,
    },
    SourceStepLimitReached(PageHostDebuggerSafePoint),
    ResumeRequested,
    Completed,
}

/// A document-order declaration retained by the child in an explicitly
/// core-selected debugger-execution document. Classic programs are admitted
/// before the first advance so core can discover an opaque identity; all
/// source-bearing data remains in this child-only queue.
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
        graph: Box<DirectModuleGraph>,
    },
}

struct PendingDebuggerExecution {
    ordinal: u32,
    language: PageHostScriptLanguage,
    kind: PageHostScriptKind,
    program: Option<PageHostDebuggerProgram>,
    execution: DeferredChildExecution,
}

#[derive(Clone)]
struct ScriptDomCapability {
    socket_path: PathBuf,
    session_token: String,
    enable_lookup_probe: bool,
    enable_dom_text_profile: bool,
    enable_dom_mutation_profile: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PageDomProfile {
    Snapshot,
    Text,
    Mutation,
}

impl ScriptDomCapability {
    fn profile(&self) -> PageDomProfile {
        if self.enable_dom_mutation_profile {
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

impl BlueJsChildHost {
    /// Creates an empty, isolated page host with the public BlueJS runtime's
    /// default fixed realm/program/bytecode bounds.
    pub fn new() -> Result<Self, BlueJsPageRuntimeError> {
        Self::with_runtime_config(BlueJsPageRuntimeConfig::default())
    }

    /// Creates a host with a runtime configuration and enough child-wide
    /// capacity to admit every permitted realm. The launcher-selected
    /// aggregate envelope uses `with_runtime_limits` instead.
    pub fn with_runtime_config(
        config: BlueJsPageRuntimeConfig,
    ) -> Result<Self, BlueJsPageRuntimeError> {
        let limits = BlueJsHostRuntimeLimits::from_runtime_config(config);
        Ok(Self {
            runtime: BlueJsPageRuntime::new(config)?,
            limits,
            debug_registry: DirectDebugRegistry::default(),
            documents: BTreeMap::new(),
            next_debugger_program_handle: 1,
            next_debugger_program_generation: 1,
            next_debugger_metadata_handle: CHILD_DEBUGGER_METADATA_ID_NAMESPACE_START,
            next_debugger_metadata_generation: CHILD_DEBUGGER_METADATA_ID_NAMESPACE_START,
            script_dom_capability: None,
        })
    }

    /// Installs only a launcher-originated, generation-private script socket.
    /// The mutually exclusive page-visible proof profiles are owner-only;
    /// none grants the general typed DOM/event profile.
    pub fn configure_script_dom_capability(
        &mut self,
        socket_path: PathBuf,
        session_token: String,
        enable_lookup_probe: bool,
        enable_dom_text_profile: bool,
        enable_dom_mutation_profile: bool,
    ) -> io::Result<()> {
        if !self.documents.is_empty()
            || self.script_dom_capability.is_some()
            || !socket_path.is_absolute()
            || !script::valid_script_session_token(&session_token)
            || [
                enable_lookup_probe,
                enable_dom_text_profile,
                enable_dom_mutation_profile,
            ]
            .into_iter()
            .filter(|enabled| *enabled)
            .count()
                > 1
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "child script capability must be configured once before document admission",
            ));
        }
        self.script_dom_capability = Some(ScriptDomCapability {
            socket_path,
            session_token,
            enable_lookup_probe,
            enable_dom_text_profile,
            enable_dom_mutation_profile,
        });
        Ok(())
    }

    /// Creates a child using the complete, immutable launcher-owner envelope.
    pub fn with_runtime_limits(limits: BlueJsHostRuntimeLimits) -> Result<Self, &'static str> {
        let config = limits.runtime_config()?;
        let mut host = Self::with_runtime_config(config)
            .map_err(|_| "BlueJS page-host runtime limits are invalid")?;
        host.limits = limits;
        Ok(host)
    }

    /// Handles one post-handshake request. The connection boundary performs
    /// `Hello` authentication first, so a later `Hello` is rejected rather
    /// than accidentally resetting host state.
    pub fn handle_request(&mut self, request: PageHostRequest) -> PageHostReply {
        match request {
            PageHostRequest::SynchronizeDocument { document } => self.synchronize(document),
            PageHostRequest::CloseRealm {
                tab_id,
                document_generation,
            } => self.close_realm(tab_id, document_generation),
            PageHostRequest::GetRealmStats {
                tab_id,
                document_generation,
            } => self.realm_stats(tab_id, document_generation),
            PageHostRequest::GetChildStats => self.child_stats(),
            PageHostRequest::ListDebuggerPrograms {
                tab_id,
                document_generation,
            } => self.debugger_programs(tab_id, document_generation),
            PageHostRequest::ListDebuggerBlueTsMetadata {
                tab_id,
                document_generation,
                program,
            } => self.debugger_bluets_metadata(tab_id, document_generation, program),
            PageHostRequest::DescribeDebuggerBlueTsMetadata {
                tab_id,
                document_generation,
                program,
                metadata,
            } => self.debugger_bluets_metadata_summary(
                tab_id,
                document_generation,
                program,
                metadata,
            ),
            PageHostRequest::DescribeDebuggerBlueTsMetadataLoweringSummary {
                tab_id,
                document_generation,
                program,
                metadata,
            } => self.debugger_bluets_metadata_lowering_summary(
                tab_id,
                document_generation,
                program,
                metadata,
            ),
            PageHostRequest::ListDebuggerBlueTsMetadataSources {
                tab_id,
                document_generation,
                program,
                metadata,
            } => self.debugger_bluets_metadata_sources(
                tab_id,
                document_generation,
                program,
                metadata,
            ),
            PageHostRequest::ListDebuggerBlueTsMetadataTypes {
                tab_id,
                document_generation,
                program,
                metadata,
            } => {
                self.debugger_bluets_metadata_types(tab_id, document_generation, program, metadata)
            }
            PageHostRequest::DescribeDebuggerBlueTsMetadataType {
                tab_id,
                document_generation,
                program,
                metadata,
                type_id,
            } => self.debugger_bluets_metadata_type_display(
                tab_id,
                document_generation,
                program,
                metadata,
                type_id,
            ),
            PageHostRequest::ListDebuggerBlueTsMetadataSymbols {
                tab_id,
                document_generation,
                program,
                metadata,
            } => self.debugger_bluets_metadata_symbols(
                tab_id,
                document_generation,
                program,
                metadata,
            ),
            PageHostRequest::ListDebuggerBlueTsMetadataContracts {
                tab_id,
                document_generation,
                program,
                metadata,
            } => self.debugger_bluets_metadata_contracts(
                tab_id,
                document_generation,
                program,
                metadata,
            ),
            PageHostRequest::DescribeDebuggerBlueTsMetadataContract {
                tab_id,
                document_generation,
                program,
                metadata,
                contract_id,
            } => self.debugger_bluets_metadata_contract_display(
                tab_id,
                document_generation,
                program,
                metadata,
                contract_id,
            ),
            PageHostRequest::ValidateDebuggerBlueTsMetadataContract {
                tab_id,
                document_generation,
                program,
                metadata,
                contract_id,
                value,
            } => self.debugger_bluets_metadata_contract_validation(
                tab_id,
                document_generation,
                program,
                metadata,
                contract_id,
                value,
            ),
            PageHostRequest::DescribeDebuggerBlueTsMetadataSymbol {
                tab_id,
                document_generation,
                program,
                metadata,
                symbol_id,
            } => self.debugger_bluets_metadata_symbol_display(
                tab_id,
                document_generation,
                program,
                metadata,
                symbol_id,
            ),
            PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolLocation {
                tab_id,
                document_generation,
                program,
                metadata,
                symbol_id,
            } => self.debugger_bluets_metadata_symbol_location(
                tab_id,
                document_generation,
                program,
                metadata,
                symbol_id,
            ),
            PageHostRequest::DescribeDebuggerBlueTsMetadataContractLocation {
                tab_id,
                document_generation,
                program,
                metadata,
                contract_id,
            } => self.debugger_bluets_metadata_contract_location(
                tab_id,
                document_generation,
                program,
                metadata,
                contract_id,
            ),
            PageHostRequest::DescribeDebuggerBlueTsSafePointSpan {
                tab_id,
                document_generation,
                metadata,
                safe_point,
            } => self.debugger_bluets_safe_point_span(
                tab_id,
                document_generation,
                metadata,
                safe_point,
            ),
            PageHostRequest::ResolveDebuggerBlueTsSourceBreakpoint {
                tab_id,
                document_generation,
                program,
                metadata,
                source_id,
                source_byte,
            } => self.debugger_bluets_source_breakpoint(
                tab_id,
                document_generation,
                program,
                metadata,
                source_id,
                source_byte,
            ),
            PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolType {
                tab_id,
                document_generation,
                program,
                metadata,
                symbol_id,
                type_id,
            } => self.debugger_bluets_metadata_symbol_type(
                tab_id,
                document_generation,
                program,
                metadata,
                symbol_id,
                type_id,
            ),
            PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolContract {
                tab_id,
                document_generation,
                program,
                metadata,
                symbol_id,
                contract_id,
            } => self.debugger_bluets_metadata_symbol_contract(
                tab_id,
                document_generation,
                program,
                metadata,
                symbol_id,
                contract_id,
            ),
            PageHostRequest::DescribeDebuggerBlueTsMetadataSource {
                tab_id,
                document_generation,
                program,
                metadata,
                source_id,
            } => self.debugger_bluets_metadata_source_provenance(
                tab_id,
                document_generation,
                program,
                metadata,
                source_id,
            ),
            PageHostRequest::ListDebuggerSafePoints {
                tab_id,
                document_generation,
                program,
            } => self.debugger_safe_points(tab_id, document_generation, program),
            PageHostRequest::ValidateDebuggerSafePoint {
                tab_id,
                document_generation,
                safe_point,
            } => self.validate_debugger_safe_point(tab_id, document_generation, safe_point),
            PageHostRequest::SetDebuggerBreakpoint {
                tab_id,
                document_generation,
                safe_point,
            } => self.set_debugger_breakpoint(tab_id, document_generation, safe_point),
            PageHostRequest::ListDebuggerBreakpoints {
                tab_id,
                document_generation,
            } => self.debugger_breakpoints(tab_id, document_generation),
            PageHostRequest::ClearDebuggerBreakpoint {
                tab_id,
                document_generation,
                safe_point,
            } => self.clear_debugger_breakpoint(tab_id, document_generation, safe_point),
            PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
                tab_id,
                document_generation,
                safe_point,
            } => self.arm_debugger_root_safe_point_breakpoint(
                tab_id,
                document_generation,
                safe_point,
            ),
            PageHostRequest::GetDebuggerExecutionState {
                tab_id,
                document_generation,
                program,
            } => self.debugger_execution_state(tab_id, document_generation, program),
            PageHostRequest::ResumeDebuggerExecution {
                tab_id,
                document_generation,
                program,
            } => self.resume_debugger_execution(tab_id, document_generation, program),
            PageHostRequest::StepDebuggerRootInstruction {
                tab_id,
                document_generation,
                program,
            } => self.step_debugger_root_instruction(tab_id, document_generation, program),
            PageHostRequest::StepDebuggerBlueTsSourceSpan {
                tab_id,
                document_generation,
                metadata,
                source_id,
                safe_point,
            } => self.step_debugger_bluets_source_span(
                tab_id,
                document_generation,
                metadata,
                source_id,
                safe_point,
            ),
            PageHostRequest::AdvanceDebuggerExecution {
                tab_id,
                document_generation,
            } => self.advance_debugger_execution(tab_id, document_generation),
            PageHostRequest::Shutdown => PageHostReply::ShutdownAck,
            PageHostRequest::Hello { .. } | PageHostRequest::Unknown => invalid_request(),
        }
    }

    fn synchronize(&mut self, document: PageHostDocument) -> PageHostReply {
        if document.tab_id == 0 || document.document_generation == 0 {
            return invalid_request();
        }
        if document.scripts.len() > MAX_SCRIPTS_PER_DOCUMENT {
            return resource_limit();
        }
        let mut ordinals = BTreeSet::new();
        if document
            .scripts
            .iter()
            .any(|script| !ordinals.insert(script.ordinal))
        {
            return invalid_request();
        }
        let source_bytes = document
            .scripts
            .iter()
            .flat_map(|script| script.graph.modules.iter())
            .try_fold(0usize, |total, source| {
                total.checked_add(source.source.len())
            });
        if !matches!(source_bytes, Some(total) if total <= MAX_SOURCE_BYTES_PER_DOCUMENT) {
            return resource_limit();
        }
        let origin = match validated_document_origin(&document.snapshot) {
            Ok(origin) => origin,
            Err(DocumentSnapshotError::ResourceLimit) => return resource_limit(),
            Err(DocumentSnapshotError::Invalid) => return invalid_request(),
        };
        if let Some(current) = self.documents.get(&document.tab_id) {
            if document.document_generation < current.generation {
                return stale_document();
            }
            if document.document_generation == current.generation {
                return PageHostReply::Synchronized {
                    tab_id: document.tab_id,
                    document_generation: document.document_generation,
                    already_current: true,
                    reports: Vec::new(),
                };
            }
        }

        if !self.documents.contains_key(&document.tab_id) && !self.can_reserve_new_realm() {
            return resource_limit();
        }

        // Parse/compile every independent declaration first. A graph that
        // cannot be structurally admitted becomes one source-free rejection,
        // while a later declaration remains eligible exactly as browser
        // document-order execution requires. No candidate program enters a
        // new realm until source/graph preflight has completed.
        let page_dom_profile = self
            .script_dom_capability
            .as_ref()
            .map_or(PageDomProfile::Snapshot, ScriptDomCapability::profile);
        let prepared: Vec<_> = document
            .scripts
            .into_iter()
            .map(|script| prepare_script(script, page_dom_profile))
            .collect();

        let lifecycle = if self.documents.contains_key(&document.tab_id) {
            self.runtime.navigate(document.tab_id, origin.clone())
        } else {
            self.runtime.open_realm(document.tab_id, origin.clone())
        };
        if lifecycle.is_err() {
            return host_failure();
        }
        // Realm replacement invalidates every prior program generation for
        // this tab. Prune before the successor is exposed so a stale static
        // record cannot survive the navigation window in the child.
        self.debug_registry
            .prune_invalid(self.runtime.program_registry());
        if install_document_snapshot_bindings(
            &mut self.runtime,
            document.tab_id,
            document.document_generation,
            &document.snapshot,
            self.script_dom_capability.clone(),
        )
        .is_err()
        {
            // A replacement document whose fixed bindings cannot be installed
            // must not leave a partially initialized successor realm. Closing
            // this fresh VM also drops every copied snapshot immediately.
            self.runtime.close_realm(document.tab_id);
            self.debug_registry
                .prune_invalid(self.runtime.program_registry());
            self.documents.remove(&document.tab_id);
            return host_failure();
        }
        self.documents.insert(
            document.tab_id,
            LiveDocument {
                generation: document.document_generation,
                origin: origin.clone(),
                debugger_execution_control: document.debugger_execution_control,
                debugger_programs: BTreeMap::new(),
                debugger_breakpoints: BTreeSet::new(),
                pending_debugger_executions: VecDeque::new(),
                debugger_execution_states: BTreeMap::new(),
            },
        );

        if document.debugger_execution_control {
            return self.defer_debugger_execution_document(
                document.tab_id,
                document.document_generation,
                &origin,
                prepared,
            );
        }

        let mut reports = Vec::with_capacity(prepared.len());
        for prepared in prepared {
            let (ordinal, language, kind, outcome) = match prepared {
                PreparedScript::Rejected {
                    ordinal,
                    language,
                    kind,
                    category,
                } => (
                    ordinal,
                    language,
                    kind,
                    PageHostScriptOutcome::Rejected {
                        category: category.to_string(),
                    },
                ),
                PreparedScript::JavaScriptClassic {
                    ordinal,
                    source,
                    program,
                } => {
                    let outcome = execute_classic(
                        &mut self.runtime,
                        document.tab_id,
                        &origin,
                        source,
                        program,
                    );
                    (
                        ordinal,
                        PageHostScriptLanguage::JavaScript,
                        PageHostScriptKind::Classic,
                        outcome,
                    )
                }
                PreparedScript::JavaScriptModule {
                    ordinal,
                    graph,
                    programs,
                } => {
                    let outcome = execute_module_graph(
                        &mut self.runtime,
                        document.tab_id,
                        &origin,
                        graph,
                        programs,
                    );
                    (
                        ordinal,
                        PageHostScriptLanguage::JavaScript,
                        PageHostScriptKind::Module,
                        outcome,
                    )
                }
                PreparedScript::BlueTsClassic { ordinal, script } => {
                    let outcome = execute_bluets_classic(
                        &mut self.runtime,
                        &mut self.debug_registry,
                        document.tab_id,
                        &origin,
                        &script,
                    );
                    (
                        ordinal,
                        PageHostScriptLanguage::BlueTs,
                        PageHostScriptKind::Classic,
                        outcome,
                    )
                }
                PreparedScript::BlueTsModule { ordinal, graph } => {
                    let outcome = execute_bluets_module_graph(
                        &mut self.runtime,
                        &mut self.debug_registry,
                        document.tab_id,
                        &origin,
                        &graph,
                    );
                    (
                        ordinal,
                        PageHostScriptLanguage::BlueTs,
                        PageHostScriptKind::Module,
                        outcome,
                    )
                }
            };
            reports.push(PageHostScriptReport {
                tab_id: document.tab_id,
                document_generation: document.document_generation,
                ordinal,
                language,
                kind,
                outcome,
            });
        }
        if self.refresh_debugger_programs(document.tab_id).is_err() {
            // A debugger-location record is part of the live-realm contract.
            // Do not report a runnable successor if the child could not mint
            // a bounded opaque inventory for every retained program.
            self.runtime.close_realm(document.tab_id);
            self.debug_registry
                .prune_invalid(self.runtime.program_registry());
            self.documents.remove(&document.tab_id);
            return host_failure();
        }
        PageHostReply::Synchronized {
            tab_id: document.tab_id,
            document_generation: document.document_generation,
            already_current: false,
            reports,
        }
    }

    fn can_reserve_new_realm(&self) -> bool {
        let Some(realm_count) = self.documents.len().checked_add(1) else {
            return false;
        };
        if realm_count > self.limits.max_realms {
            return false;
        }
        [
            (
                self.limits.max_programs_per_realm,
                self.limits.max_reserved_programs,
            ),
            (
                self.limits.max_bytecode_bytes_per_realm,
                self.limits.max_reserved_bytecode_bytes,
            ),
            (
                self.limits.max_heap_bytes_per_realm,
                self.limits.max_reserved_heap_bytes,
            ),
        ]
        .into_iter()
        .all(|(per_realm, reserved)| {
            realm_count
                .checked_mul(per_realm)
                .is_some_and(|needed| needed <= reserved)
        })
    }

    /// Retains an explicitly core-selected document in child-owned document
    /// order. Only already-authorized JavaScript classic programs are admitted
    /// before the first advance, because core needs their opaque identity and
    /// exact safe-point inventory before it can arm the single root
    /// continuation. A leading direct BlueTS declaration has no installed
    /// root-continuation pause surface, so it executes immediately while the
    /// document has no earlier deferred work; that preserves document order
    /// and lets its separately gated static-metadata inventory be discovered.
    fn defer_debugger_execution_document(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        origin: &BlueJsPageOrigin,
        prepared: Vec<PreparedScript>,
    ) -> PageHostReply {
        let mut reports = Vec::new();
        let mut may_execute_bluets_immediately = true;
        for prepared in prepared {
            match prepared {
                PreparedScript::Rejected {
                    ordinal,
                    language,
                    kind,
                    category,
                } => reports.push(script_report(
                    tab_id,
                    document_generation,
                    ordinal,
                    language,
                    kind,
                    rejected(category),
                )),
                PreparedScript::JavaScriptClassic {
                    ordinal,
                    source,
                    program,
                } => {
                    let source = match source_identity(&source) {
                        Ok(source) => source,
                        Err(category) => {
                            reports.push(script_report(
                                tab_id,
                                document_generation,
                                ordinal,
                                PageHostScriptLanguage::JavaScript,
                                PageHostScriptKind::Classic,
                                rejected(category),
                            ));
                            continue;
                        }
                    };
                    let handle = match self
                        .runtime
                        .install_program(tab_id, origin, source, &program)
                    {
                        Ok(handle) => handle,
                        Err(error) => {
                            reports.push(script_report(
                                tab_id,
                                document_generation,
                                ordinal,
                                PageHostScriptLanguage::JavaScript,
                                PageHostScriptKind::Classic,
                                rejected(page_runtime_category(error)),
                            ));
                            continue;
                        }
                    };
                    let program = match self.register_debugger_program(tab_id, handle) {
                        Ok(program) => program,
                        Err(()) => return self.fail_debugger_execution_document(tab_id),
                    };
                    let Some(document) = self.documents.get_mut(&tab_id) else {
                        return self.fail_debugger_execution_document(tab_id);
                    };
                    may_execute_bluets_immediately = false;
                    document
                        .debugger_execution_states
                        .insert(program, ChildDebuggerExecutionStatus::Pending);
                    document
                        .pending_debugger_executions
                        .push_back(PendingDebuggerExecution {
                            ordinal,
                            language: PageHostScriptLanguage::JavaScript,
                            kind: PageHostScriptKind::Classic,
                            program: Some(program),
                            execution: DeferredChildExecution::JavaScriptClassic {
                                handle,
                                root_safe_point: None,
                            },
                        });
                }
                PreparedScript::JavaScriptModule {
                    ordinal,
                    graph,
                    programs,
                } => {
                    may_execute_bluets_immediately = false;
                    self.enqueue_debugger_execution(
                        tab_id,
                        PendingDebuggerExecution {
                            ordinal,
                            language: PageHostScriptLanguage::JavaScript,
                            kind: PageHostScriptKind::Module,
                            program: None,
                            execution: DeferredChildExecution::JavaScriptModule { graph, programs },
                        },
                    );
                }
                PreparedScript::BlueTsClassic { ordinal, script } => {
                    let attachment = match script.attach_debug_in_page_realm(
                        &mut self.runtime,
                        tab_id,
                        origin,
                        &mut self.debug_registry,
                    ) {
                        Ok(attachment) => attachment,
                        Err(error) => {
                            reports.push(script_report(
                                tab_id,
                                document_generation,
                                ordinal,
                                PageHostScriptLanguage::BlueTs,
                                PageHostScriptKind::Classic,
                                rejected(bluets_bridge_category(error)),
                            ));
                            continue;
                        }
                    };
                    let program = match self.register_debugger_program(tab_id, attachment.handle) {
                        Ok(program) => program,
                        Err(()) => return self.fail_debugger_execution_document(tab_id),
                    };
                    may_execute_bluets_immediately = false;
                    let Some(document) = self.documents.get_mut(&tab_id) else {
                        return self.fail_debugger_execution_document(tab_id);
                    };
                    document
                        .debugger_execution_states
                        .insert(program, ChildDebuggerExecutionStatus::Pending);
                    document
                        .pending_debugger_executions
                        .push_back(PendingDebuggerExecution {
                            ordinal,
                            language: PageHostScriptLanguage::BlueTs,
                            kind: PageHostScriptKind::Classic,
                            program: Some(program),
                            execution: DeferredChildExecution::BlueTsClassic {
                                handle: attachment.handle,
                                root_safe_point: None,
                            },
                        });
                }
                PreparedScript::BlueTsModule { ordinal, graph } => {
                    if may_execute_bluets_immediately {
                        reports.push(script_report(
                            tab_id,
                            document_generation,
                            ordinal,
                            PageHostScriptLanguage::BlueTs,
                            PageHostScriptKind::Module,
                            execute_bluets_module_graph(
                                &mut self.runtime,
                                &mut self.debug_registry,
                                tab_id,
                                origin,
                                &graph,
                            ),
                        ));
                    } else {
                        self.enqueue_debugger_execution(
                            tab_id,
                            PendingDebuggerExecution {
                                ordinal,
                                language: PageHostScriptLanguage::BlueTs,
                                kind: PageHostScriptKind::Module,
                                program: None,
                                execution: DeferredChildExecution::BlueTsModule { graph },
                            },
                        );
                    }
                }
            }
        }
        if self.refresh_debugger_programs(tab_id).is_err() {
            return self.fail_debugger_execution_document(tab_id);
        }
        PageHostReply::Synchronized {
            tab_id,
            document_generation,
            already_current: false,
            reports,
        }
    }

    fn enqueue_debugger_execution(&mut self, tab_id: u64, pending: PendingDebuggerExecution) {
        self.documents
            .get_mut(&tab_id)
            .expect("the deferred child document remains live while it is prepared")
            .pending_debugger_executions
            .push_back(pending);
    }

    fn fail_debugger_execution_document(&mut self, tab_id: u64) -> PageHostReply {
        self.runtime.close_realm(tab_id);
        self.debug_registry
            .prune_invalid(self.runtime.program_registry());
        self.documents.remove(&tab_id);
        host_failure()
    }

    fn close_realm(&mut self, tab_id: u64, document_generation: u64) -> PageHostReply {
        match self.documents.get(&tab_id) {
            None => unknown_realm(),
            Some(document) if document.generation != document_generation => stale_document(),
            Some(_) => {
                self.runtime.close_realm(tab_id);
                self.debug_registry
                    .prune_invalid(self.runtime.program_registry());
                self.documents.remove(&tab_id);
                PageHostReply::RealmClosed {
                    tab_id,
                    document_generation,
                }
            }
        }
    }

    fn realm_stats(&self, tab_id: u64, document_generation: u64) -> PageHostReply {
        let Some(document) = self.documents.get(&tab_id) else {
            return unknown_realm();
        };
        if document.generation != document_generation {
            return stale_document();
        }
        match self.runtime.realm_stats(tab_id) {
            Ok(stats) => {
                let reply = PageHostRealmStats {
                    tab_id,
                    document_generation,
                    program_count: u32::try_from(stats.program_count).unwrap_or(u32::MAX),
                    bytecode_bytes: u64::try_from(stats.bytecode_bytes).unwrap_or(u64::MAX),
                    heap_bytes: u64::try_from(stats.heap.managed_bytes).unwrap_or(u64::MAX),
                };
                if !reply.is_well_formed() {
                    return host_failure();
                }
                PageHostReply::RealmStats(reply)
            }
            Err(_) => host_failure(),
        }
    }

    /// Recomputes actual VM-managed usage from the live realm table on each
    /// request. No cached predecessor generation or conservative reservation
    /// is included, and checked sums fail closed instead of wrapping.
    fn child_stats(&self) -> PageHostReply {
        let Ok(realm_count) = u32::try_from(self.documents.len()) else {
            return host_failure();
        };
        let mut totals = PageHostChildStats {
            realm_count,
            program_count: 0,
            bytecode_bytes: 0,
            heap_bytes: 0,
        };
        for (&tab_id, document) in &self.documents {
            let PageHostReply::RealmStats(stats) = self.realm_stats(tab_id, document.generation)
            else {
                return host_failure();
            };
            let Some(program_count) = totals
                .program_count
                .checked_add(u64::from(stats.program_count))
            else {
                return host_failure();
            };
            let Some(bytecode_bytes) = totals.bytecode_bytes.checked_add(stats.bytecode_bytes)
            else {
                return host_failure();
            };
            let Some(heap_bytes) = totals.heap_bytes.checked_add(stats.heap_bytes) else {
                return host_failure();
            };
            totals.program_count = program_count;
            totals.bytecode_bytes = bytecode_bytes;
            totals.heap_bytes = heap_bytes;
        }
        if !totals.is_well_formed()
            || usize::try_from(totals.program_count)
                .ok()
                .is_none_or(|count| count > self.limits.max_reserved_programs)
            || usize::try_from(totals.bytecode_bytes)
                .ok()
                .is_none_or(|bytes| bytes > self.limits.max_reserved_bytecode_bytes)
            || usize::try_from(totals.heap_bytes)
                .ok()
                .is_none_or(|bytes| bytes > self.limits.max_reserved_heap_bytes)
        {
            return host_failure();
        }
        PageHostReply::ChildStats(totals)
    }

    /// Lists only the child-minted private IDs for one exact realm. The core
    /// intentionally remaps these again before public debugger IPC sees them.
    fn debugger_programs(&self, tab_id: u64, document_generation: u64) -> PageHostReply {
        let document = match self.exact_document(tab_id, document_generation) {
            Ok(document) => document,
            Err(reply) => return reply,
        };
        PageHostReply::DebuggerPrograms {
            tab_id,
            document_generation,
            programs: document
                .debugger_programs
                .iter()
                .map(|(&program_handle, record)| PageHostDebuggerProgram {
                    program_handle,
                    program_generation: record.program_generation,
                })
                .collect(),
        }
    }

    /// Enumerates the child-private static-BlueTS association for one exact
    /// currently live program. The association is intentionally minted lazily
    /// on this authenticated inventory request: program discovery itself does
    /// not imply static-metadata authority. The reply is a bounded handle list
    /// rather than a metadata payload, and it uses an identity namespace that
    /// is distinct from the child debugger program IDs.
    fn debugger_bluets_metadata(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    ) -> PageHostReply {
        if !program.is_well_formed() {
            return invalid_request();
        }
        let runtime_handle = {
            let document = match self.exact_document(tab_id, document_generation) {
                Ok(document) => document,
                Err(reply) => return reply,
            };
            let Some(record) = document.debugger_programs.get(&program.program_handle) else {
                return invalid_request();
            };
            if record.program_generation != program.program_generation {
                return invalid_request();
            }
            record.runtime_handle
        };

        // A normal JavaScript program and a BlueTS program whose attachment
        // has been pruned both have no metadata inventory. Do not mint a
        // negative-result handle; an empty bounded list reveals no static
        // count or compiler detail beyond this program's ineligibility.
        if self
            .debug_registry
            .get(self.runtime.program_registry(), runtime_handle)
            .is_err()
        {
            let document = self
                .documents
                .get_mut(&tab_id)
                .expect("the exact child document remains live after registry validation");
            document
                .debugger_programs
                .get_mut(&program.program_handle)
                .expect("the exact child program remains registered")
                .metadata = None;
            return PageHostReply::DebuggerBlueTsMetadata {
                tab_id,
                document_generation,
                program,
                metadata: Vec::new(),
            };
        }

        let metadata = {
            let existing = self
                .documents
                .get(&tab_id)
                .expect("the exact child document remains live after registry validation")
                .debugger_programs
                .get(&program.program_handle)
                .expect("the exact child program remains registered")
                .metadata;
            match existing {
                Some(metadata) => metadata,
                None => {
                    let metadata = match self.mint_debugger_metadata_handle() {
                        Ok(metadata) => metadata,
                        Err(()) => return host_failure(),
                    };
                    self.documents
                        .get_mut(&tab_id)
                        .expect("the exact child document remains live while metadata is minted")
                        .debugger_programs
                        .get_mut(&program.program_handle)
                        .expect(
                            "the exact child program remains registered while metadata is minted",
                        )
                        .metadata = Some(metadata);
                    metadata
                }
            }
        };
        PageHostReply::DebuggerBlueTsMetadata {
            tab_id,
            document_generation,
            program,
            metadata: vec![metadata],
        }
    }

    /// Returns the first deliberately narrow read surface for a previously
    /// inventoried BlueTS debug attachment. The handle must still be owned by
    /// this exact live program, so neither an arbitrary child-private ID nor
    /// a handle from a sibling program can probe the registry. The response
    /// contains fixed fingerprints and aggregate counts only; source text and
    /// identity, spans, names, type displays, symbols, contracts, bytecode,
    /// VM objects, and values remain inside this child.
    fn debugger_bluets_metadata_summary(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    ) -> PageHostReply {
        if !program.is_well_formed() || !metadata.is_well_formed() {
            return invalid_request();
        }
        let runtime_handle = {
            let document = match self.exact_document(tab_id, document_generation) {
                Ok(document) => document,
                Err(reply) => return reply,
            };
            let Some(record) = document.debugger_programs.get(&program.program_handle) else {
                return invalid_request();
            };
            if record.program_generation != program.program_generation
                || record.metadata != Some(metadata)
            {
                return invalid_request();
            }
            record.runtime_handle
        };

        let summary = match self
            .debug_registry
            .get(self.runtime.program_registry(), runtime_handle)
        {
            Ok(retained) => {
                let info = retained.static_info();
                let (Ok(source_count), Ok(type_count), Ok(symbol_count), Ok(contract_count)) = (
                    u32::try_from(info.sources.len()),
                    u32::try_from(info.types.len()),
                    u32::try_from(info.symbols.len()),
                    u32::try_from(info.contracts.len()),
                ) else {
                    return host_failure();
                };
                PageHostDebuggerBlueTsMetadataSummary {
                    language_version: info.language_version.clone(),
                    compiler_options_hash: info.compiler_options_hash.clone(),
                    source_count,
                    type_count,
                    symbol_count,
                    contract_count,
                }
            }
            Err(_) => {
                // A registry pruning race invalidates the private inventory
                // identity before reporting anything about the old record.
                self.documents
                    .get_mut(&tab_id)
                    .expect("the exact child document remains live after registry validation")
                    .debugger_programs
                    .get_mut(&program.program_handle)
                    .expect("the exact child program remains registered after registry validation")
                    .metadata = None;
                return invalid_request();
            }
        };
        PageHostReply::DebuggerBlueTsMetadataSummary {
            tab_id,
            document_generation,
            program,
            metadata,
            summary,
        }
    }

    /// Returns aggregate evidence for the exact child-retained direct-lowering
    /// map. Source spans, map entries, AST nodes, code-unit IDs, and bytecode
    /// offsets remain in the child; this operation is not a map dereference.
    fn debugger_bluets_metadata_lowering_summary(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    ) -> PageHostReply {
        if !program.is_well_formed() || !metadata.is_well_formed() {
            return invalid_request();
        }
        let runtime_handle = {
            let document = match self.exact_document(tab_id, document_generation) {
                Ok(document) => document,
                Err(reply) => return reply,
            };
            let Some(record) = document.debugger_programs.get(&program.program_handle) else {
                return invalid_request();
            };
            if record.program_generation != program.program_generation
                || record.metadata != Some(metadata)
            {
                return invalid_request();
            }
            record.runtime_handle
        };
        let summary = match self
            .debug_registry
            .get(self.runtime.program_registry(), runtime_handle)
        {
            Ok(retained) => {
                let map = retained.safe_point_map();
                let Ok(bound_safe_point_count) = u32::try_from(map.entries.len()) else {
                    return host_failure();
                };
                if bound_safe_point_count > PAGE_HOST_DEBUGGER_MAX_SAFE_POINTS_PER_PROGRAM {
                    return resource_limit();
                }
                PageHostDebuggerBlueTsMetadataLoweringSummary {
                    safe_point_map_abi: map.format.to_string(),
                    program_abi: map.program_abi.to_string(),
                    source_set_hash: map.source_set_hash.clone(),
                    bound_safe_point_count,
                }
            }
            Err(_) => {
                self.documents
                    .get_mut(&tab_id)
                    .expect("the exact child document remains live after registry validation")
                    .debugger_programs
                    .get_mut(&program.program_handle)
                    .expect("the exact child program remains registered after registry validation")
                    .metadata = None;
                return invalid_request();
            }
        };
        PageHostReply::DebuggerBlueTsMetadataLoweringSummary {
            tab_id,
            document_generation,
            program,
            metadata,
            summary: Box::new(summary),
        }
    }

    /// Lists only compiler-minted source-record IDs for an already inventoried
    /// metadata attachment. The enclosing opaque metadata handle remains the
    /// target boundary; an ID reveals no module identity, content hash, text,
    /// span, or record detail.
    fn debugger_bluets_metadata_sources(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    ) -> PageHostReply {
        if !program.is_well_formed() || !metadata.is_well_formed() {
            return invalid_request();
        }
        let runtime_handle = {
            let document = match self.exact_document(tab_id, document_generation) {
                Ok(document) => document,
                Err(reply) => return reply,
            };
            let Some(record) = document.debugger_programs.get(&program.program_handle) else {
                return invalid_request();
            };
            if record.program_generation != program.program_generation
                || record.metadata != Some(metadata)
            {
                return invalid_request();
            }
            record.runtime_handle
        };
        let sources = match self
            .debug_registry
            .get(self.runtime.program_registry(), runtime_handle)
        {
            Ok(retained) => {
                let static_sources = &retained.static_info().sources;
                if static_sources.len()
                    > usize::try_from(DEBUGGER_STATIC_METADATA_MAX_SOURCES).unwrap()
                {
                    return resource_limit();
                }
                let mut identities = BTreeSet::new();
                let mut sources = Vec::with_capacity(static_sources.len());
                for source in static_sources {
                    if !identities.insert(source.id.0) {
                        return invalid_request();
                    }
                    sources.push(PageHostDebuggerBlueTsMetadataSourceId {
                        source_id: source.id.0,
                    });
                }
                sources
            }
            Err(_) => {
                self.documents
                    .get_mut(&tab_id)
                    .expect("the exact child document remains live after registry validation")
                    .debugger_programs
                    .get_mut(&program.program_handle)
                    .expect("the exact child program remains registered after registry validation")
                    .metadata = None;
                return invalid_request();
            }
        };
        PageHostReply::DebuggerBlueTsMetadataSources {
            tab_id,
            document_generation,
            program,
            metadata,
            sources,
        }
    }

    /// Lists only compiler-minted type-record IDs for an already inventoried
    /// metadata attachment. The IDs carry no type display, source identity,
    /// span, symbol, contract, bytecode, VM object, or value.
    fn debugger_bluets_metadata_types(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    ) -> PageHostReply {
        if !program.is_well_formed() || !metadata.is_well_formed() {
            return invalid_request();
        }
        let runtime_handle = {
            let document = match self.exact_document(tab_id, document_generation) {
                Ok(document) => document,
                Err(reply) => return reply,
            };
            let Some(record) = document.debugger_programs.get(&program.program_handle) else {
                return invalid_request();
            };
            if record.program_generation != program.program_generation
                || record.metadata != Some(metadata)
            {
                return invalid_request();
            }
            record.runtime_handle
        };
        let types = match self
            .debug_registry
            .get(self.runtime.program_registry(), runtime_handle)
        {
            Ok(retained) => {
                let static_types = &retained.static_info().types;
                if static_types.len() > usize::try_from(DEBUGGER_STATIC_METADATA_MAX_TYPES).unwrap()
                {
                    return resource_limit();
                }
                let mut identities = BTreeSet::new();
                let mut types = Vec::with_capacity(static_types.len());
                for static_type in static_types {
                    if !identities.insert(static_type.id.0) {
                        return invalid_request();
                    }
                    types.push(PageHostDebuggerBlueTsMetadataTypeId {
                        type_id: static_type.id.0,
                    });
                }
                types
            }
            Err(_) => {
                self.documents
                    .get_mut(&tab_id)
                    .expect("the exact child document remains live after registry validation")
                    .debugger_programs
                    .get_mut(&program.program_handle)
                    .expect("the exact child program remains registered after registry validation")
                    .metadata = None;
                return invalid_request();
            }
        };
        PageHostReply::DebuggerBlueTsMetadataTypes {
            tab_id,
            document_generation,
            program,
            metadata,
            types,
        }
    }

    /// Returns one bounded compiler-produced type display after the caller
    /// supplies an exact child program and metadata attachment. This private
    /// endpoint never accepts a standalone numeric type target, and returns
    /// no source, span, symbol, contract, bytecode, VM object, or value.
    fn debugger_bluets_metadata_type_display(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        type_id: u32,
    ) -> PageHostReply {
        if !program.is_well_formed() || !metadata.is_well_formed() {
            return invalid_request();
        }
        let runtime_handle = {
            let document = match self.exact_document(tab_id, document_generation) {
                Ok(document) => document,
                Err(reply) => return reply,
            };
            let Some(record) = document.debugger_programs.get(&program.program_handle) else {
                return invalid_request();
            };
            if record.program_generation != program.program_generation
                || record.metadata != Some(metadata)
            {
                return invalid_request();
            }
            record.runtime_handle
        };
        let static_type = match self
            .debug_registry
            .get(self.runtime.program_registry(), runtime_handle)
        {
            Ok(retained) => {
                let Some(static_type) = retained
                    .static_info()
                    .types
                    .iter()
                    .find(|static_type| static_type.id.0 == type_id)
                else {
                    return invalid_request();
                };
                if static_type.display.is_empty()
                    || static_type.display.len() > DEBUGGER_STATIC_METADATA_TYPE_DISPLAY_MAX_BYTES
                {
                    return resource_limit();
                }
                PageHostDebuggerBlueTsMetadataTypeDisplay {
                    type_id,
                    display: static_type.display.clone(),
                }
            }
            Err(_) => {
                self.documents
                    .get_mut(&tab_id)
                    .expect("the exact child document remains live after registry validation")
                    .debugger_programs
                    .get_mut(&program.program_handle)
                    .expect("the exact child program remains registered after registry validation")
                    .metadata = None;
                return invalid_request();
            }
        };
        PageHostReply::DebuggerBlueTsMetadataType {
            tab_id,
            document_generation,
            program,
            metadata,
            static_type,
        }
    }

    /// Lists only compiler-minted symbol IDs after the caller supplies an
    /// exact child program and metadata attachment. Names, spans, declared
    /// types, contracts, bytecode, VM objects, and values remain private.
    fn debugger_bluets_metadata_symbols(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    ) -> PageHostReply {
        if !program.is_well_formed() || !metadata.is_well_formed() {
            return invalid_request();
        }
        let runtime_handle = {
            let document = match self.exact_document(tab_id, document_generation) {
                Ok(document) => document,
                Err(reply) => return reply,
            };
            let Some(record) = document.debugger_programs.get(&program.program_handle) else {
                return invalid_request();
            };
            if record.program_generation != program.program_generation
                || record.metadata != Some(metadata)
            {
                return invalid_request();
            }
            record.runtime_handle
        };
        let symbols = match self
            .debug_registry
            .get(self.runtime.program_registry(), runtime_handle)
        {
            Ok(retained) => {
                let static_symbols = &retained.static_info().symbols;
                if static_symbols.len()
                    > usize::try_from(DEBUGGER_STATIC_METADATA_MAX_SYMBOLS).unwrap()
                {
                    return resource_limit();
                }
                let mut identities = BTreeSet::new();
                let mut symbols = Vec::with_capacity(static_symbols.len());
                for symbol in static_symbols {
                    if !identities.insert(symbol.id.0) {
                        return invalid_request();
                    }
                    symbols.push(PageHostDebuggerBlueTsMetadataSymbolId {
                        symbol_id: symbol.id.0,
                    });
                }
                symbols
            }
            Err(_) => {
                self.documents
                    .get_mut(&tab_id)
                    .expect("the exact child document remains live after registry validation")
                    .debugger_programs
                    .get_mut(&program.program_handle)
                    .expect("the exact child program remains registered after registry validation")
                    .metadata = None;
                return invalid_request();
            }
        };
        PageHostReply::DebuggerBlueTsMetadataSymbols {
            tab_id,
            document_generation,
            program,
            metadata,
            symbols,
        }
    }

    /// Returns one bounded compiler-produced symbol display after the caller
    /// supplies an exact child program and metadata attachment. This private
    /// endpoint never accepts a standalone numeric symbol target, and returns
    /// no source span, type, contract, bytecode, VM object, or value.
    fn debugger_bluets_metadata_symbol_display(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        symbol_id: u32,
    ) -> PageHostReply {
        if !program.is_well_formed() || !metadata.is_well_formed() {
            return invalid_request();
        }
        let runtime_handle = {
            let document = match self.exact_document(tab_id, document_generation) {
                Ok(document) => document,
                Err(reply) => return reply,
            };
            let Some(record) = document.debugger_programs.get(&program.program_handle) else {
                return invalid_request();
            };
            if record.program_generation != program.program_generation
                || record.metadata != Some(metadata)
            {
                return invalid_request();
            }
            record.runtime_handle
        };
        let symbol = match self
            .debug_registry
            .get(self.runtime.program_registry(), runtime_handle)
        {
            Ok(retained) => {
                let Some(symbol) = retained
                    .static_info()
                    .symbols
                    .iter()
                    .find(|symbol| symbol.id.0 == symbol_id)
                else {
                    return invalid_request();
                };
                if symbol.name.is_empty()
                    || symbol.name.len() > DEBUGGER_STATIC_METADATA_SYMBOL_DISPLAY_MAX_BYTES
                {
                    return resource_limit();
                }
                PageHostDebuggerBlueTsMetadataSymbolDisplay {
                    symbol_id,
                    display: symbol.name.clone(),
                    exported: symbol.exported,
                    kind: match symbol.kind {
                        blueice_bluets::SymbolKind::Import => {
                            DebuggerStaticMetadataSymbolKind::Import
                        }
                        blueice_bluets::SymbolKind::TypeAlias => {
                            DebuggerStaticMetadataSymbolKind::TypeAlias
                        }
                        blueice_bluets::SymbolKind::Interface => {
                            DebuggerStaticMetadataSymbolKind::Interface
                        }
                        blueice_bluets::SymbolKind::Variable => {
                            DebuggerStaticMetadataSymbolKind::Variable
                        }
                        blueice_bluets::SymbolKind::Function => {
                            DebuggerStaticMetadataSymbolKind::Function
                        }
                    },
                }
            }
            Err(_) => {
                self.documents
                    .get_mut(&tab_id)
                    .expect("the exact child document remains live after registry validation")
                    .debugger_programs
                    .get_mut(&program.program_handle)
                    .expect("the exact child program remains registered after registry validation")
                    .metadata = None;
                return invalid_request();
            }
        };
        PageHostReply::DebuggerBlueTsMetadataSymbol {
            tab_id,
            document_generation,
            program,
            metadata,
            symbol,
        }
    }

    /// Returns one source-text-free half-open declaration range for an exact
    /// compiler-minted symbol. The location contains only the symbol/source
    /// numeric identities and byte offsets; module identity, source contents,
    /// names, types, contracts, bytecode, values, and source-map translation
    /// remain private to the child.
    fn debugger_bluets_metadata_symbol_location(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        symbol_id: u32,
    ) -> PageHostReply {
        if !program.is_well_formed() || !metadata.is_well_formed() {
            return invalid_request();
        }
        let runtime_handle = {
            let document = match self.exact_document(tab_id, document_generation) {
                Ok(document) => document,
                Err(reply) => return reply,
            };
            let Some(record) = document.debugger_programs.get(&program.program_handle) else {
                return invalid_request();
            };
            if record.program_generation != program.program_generation
                || record.metadata != Some(metadata)
            {
                return invalid_request();
            }
            record.runtime_handle
        };
        let location = match self
            .debug_registry
            .get(self.runtime.program_registry(), runtime_handle)
        {
            Ok(retained) => {
                let Some(symbol) = retained
                    .static_info()
                    .symbols
                    .iter()
                    .find(|symbol| symbol.id.0 == symbol_id)
                else {
                    return invalid_request();
                };
                let Some(source) = retained
                    .static_info()
                    .sources
                    .iter()
                    .find(|source| source.id == symbol.source)
                else {
                    return invalid_request();
                };
                if symbol.span.module != source.module
                    || symbol.span.start >= symbol.span.end
                    || symbol.span.end
                        > usize::try_from(DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES).unwrap()
                {
                    return invalid_request();
                }
                let Ok(start_byte) = u32::try_from(symbol.span.start) else {
                    return invalid_request();
                };
                let Ok(end_byte) = u32::try_from(symbol.span.end) else {
                    return invalid_request();
                };
                let Some(coordinates) =
                    debugger_source_coordinates(symbol.location, start_byte, end_byte)
                else {
                    return invalid_request();
                };
                PageHostDebuggerBlueTsMetadataSymbolLocation {
                    symbol_id,
                    source_id: source.id.0,
                    start_byte,
                    end_byte,
                    coordinates,
                }
            }
            Err(_) => {
                self.documents
                    .get_mut(&tab_id)
                    .expect("the exact child document remains live after registry validation")
                    .debugger_programs
                    .get_mut(&program.program_handle)
                    .expect("the exact child program remains registered after registry validation")
                    .metadata = None;
                return invalid_request();
            }
        };
        PageHostReply::DebuggerBlueTsMetadataSymbolLocation {
            tab_id,
            document_generation,
            program,
            metadata,
            location,
        }
    }

    /// Resolves only one exact compiler-bound safe point to its retained
    /// original BlueTS byte span. The private child first revalidates the live
    /// instruction and opaque metadata attachment; an ordinary BlueJS safe
    /// point with no direct BlueTS lowering record never receives a guessed
    /// source position.
    fn debugger_bluets_safe_point_span(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        metadata: PageHostDebuggerMetadataHandle,
        safe_point: PageHostDebuggerSafePoint,
    ) -> PageHostReply {
        if !metadata.is_well_formed() || !safe_point.is_well_formed() {
            return invalid_request();
        }
        if let Err(reply) = self.exact_debugger_safe_point(tab_id, document_generation, safe_point)
        {
            return reply;
        }
        let program = safe_point.program;
        let runtime_handle = {
            let document = match self.exact_document(tab_id, document_generation) {
                Ok(document) => document,
                Err(reply) => return reply,
            };
            let Some(record) = document.debugger_programs.get(&program.program_handle) else {
                return invalid_request();
            };
            if record.program_generation != program.program_generation
                || record.metadata != Some(metadata)
            {
                return invalid_request();
            }
            record.runtime_handle
        };
        let span = match self
            .debug_registry
            .get(self.runtime.program_registry(), runtime_handle)
        {
            Ok(retained) => {
                let Some(entry) = retained.safe_point_map().source_span_for_safe_point(
                    safe_point.code_unit_ordinal,
                    safe_point.bytecode_offset,
                ) else {
                    return invalid_request();
                };
                let mut matching_sources = retained
                    .static_info()
                    .sources
                    .iter()
                    .filter(|source| source.module == entry.source);
                let Some(source) = matching_sources.next() else {
                    return invalid_request();
                };
                if matching_sources.next().is_some()
                    || entry.start_byte >= entry.end_byte
                    || entry.end_byte
                        > usize::try_from(DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES).unwrap()
                {
                    return invalid_request();
                }
                let (Ok(start_byte), Ok(end_byte)) = (
                    u32::try_from(entry.start_byte),
                    u32::try_from(entry.end_byte),
                ) else {
                    return invalid_request();
                };
                PageHostDebuggerBlueTsSafePointSpan {
                    source_id: source.id.0,
                    start_byte,
                    end_byte,
                }
            }
            Err(_) => {
                self.documents
                    .get_mut(&tab_id)
                    .expect("the exact child document remains live after registry validation")
                    .debugger_programs
                    .get_mut(&program.program_handle)
                    .expect("the exact child program remains registered after registry validation")
                    .metadata = None;
                return invalid_request();
            }
        };
        PageHostReply::DebuggerBlueTsSafePointSpan {
            tab_id,
            document_generation,
            metadata,
            safe_point,
            span,
        }
    }

    /// Resolves a bounded original TypeScript position only inside the
    /// already-authorized private core/child channel. A retained unbound span
    /// stays unbound; it is never silently skipped in favor of a later bound
    /// instruction. This operation does not install or arm a breakpoint.
    fn debugger_bluets_source_breakpoint(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        source_id: u32,
        source_byte: u32,
    ) -> PageHostReply {
        if !program.is_well_formed()
            || !metadata.is_well_formed()
            || source_byte > DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES
        {
            return invalid_request();
        }
        let runtime_handle = {
            let document = match self.exact_document(tab_id, document_generation) {
                Ok(document) => document,
                Err(reply) => return reply,
            };
            let Some(record) = document.debugger_programs.get(&program.program_handle) else {
                return invalid_request();
            };
            if record.program_generation != program.program_generation
                || record.metadata != Some(metadata)
            {
                return invalid_request();
            }
            record.runtime_handle
        };
        let safe_point = match self
            .debug_registry
            .get(self.runtime.program_registry(), runtime_handle)
        {
            Ok(retained) => {
                let mut matching_sources = retained
                    .static_info()
                    .sources
                    .iter()
                    .filter(|source| source.id.0 == source_id);
                let Some(source) = matching_sources.next() else {
                    return invalid_request();
                };
                if matching_sources.next().is_some() {
                    return invalid_request();
                }
                match retained.breakpoint_at_or_after(&source.module, source_byte as usize) {
                    DirectSafePointBinding::Bound(bound) => Some(PageHostDebuggerSafePoint {
                        program,
                        code_unit_ordinal: bound.code_unit.ordinal(),
                        bytecode_offset: bound.bytecode_offset,
                    }),
                    DirectSafePointBinding::Unbound => None,
                }
            }
            Err(_) => {
                self.documents
                    .get_mut(&tab_id)
                    .expect("the exact child document remains live after registry validation")
                    .debugger_programs
                    .get_mut(&program.program_handle)
                    .expect("the exact child program remains registered after registry validation")
                    .metadata = None;
                return invalid_request();
            }
        };
        if let Some(safe_point) = safe_point {
            if let Err(reply) =
                self.exact_debugger_safe_point(tab_id, document_generation, safe_point)
            {
                return reply;
            }
        }
        PageHostReply::DebuggerBlueTsSourceBreakpoint {
            tab_id,
            document_generation,
            program,
            metadata,
            source_id,
            source_byte,
            safe_point,
        }
    }

    /// Returns only a retained contract declaration's source ID and bounded
    /// byte range under one exact child-local metadata attachment.
    fn debugger_bluets_metadata_contract_location(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        contract_id: u32,
    ) -> PageHostReply {
        if !program.is_well_formed() || !metadata.is_well_formed() {
            return invalid_request();
        }
        let runtime_handle = {
            let document = match self.exact_document(tab_id, document_generation) {
                Ok(document) => document,
                Err(reply) => return reply,
            };
            let Some(record) = document.debugger_programs.get(&program.program_handle) else {
                return invalid_request();
            };
            if record.program_generation != program.program_generation
                || record.metadata != Some(metadata)
            {
                return invalid_request();
            }
            record.runtime_handle
        };
        let location = match self
            .debug_registry
            .get(self.runtime.program_registry(), runtime_handle)
        {
            Ok(retained) => {
                let Some(contract) = retained
                    .static_info()
                    .contracts
                    .iter()
                    .find(|contract| contract.id.0 == contract_id)
                else {
                    return invalid_request();
                };
                let Some(source) = retained
                    .static_info()
                    .sources
                    .iter()
                    .find(|source| source.id == contract.source)
                else {
                    return invalid_request();
                };
                if contract.span.module != source.module
                    || contract.span.start >= contract.span.end
                    || contract.span.end
                        > usize::try_from(DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES).unwrap()
                {
                    return invalid_request();
                }
                let Ok(start_byte) = u32::try_from(contract.span.start) else {
                    return invalid_request();
                };
                let Ok(end_byte) = u32::try_from(contract.span.end) else {
                    return invalid_request();
                };
                let Some(coordinates) =
                    debugger_source_coordinates(contract.location, start_byte, end_byte)
                else {
                    return invalid_request();
                };
                PageHostDebuggerBlueTsMetadataContractLocation {
                    contract_id,
                    source_id: source.id.0,
                    start_byte,
                    end_byte,
                    coordinates,
                }
            }
            Err(_) => {
                self.documents
                    .get_mut(&tab_id)
                    .expect("the exact child document remains live after registry validation")
                    .debugger_programs
                    .get_mut(&program.program_handle)
                    .expect("the exact child program remains registered after registry validation")
                    .metadata = None;
                return invalid_request();
            }
        };
        PageHostReply::DebuggerBlueTsMetadataContractLocation {
            tab_id,
            document_generation,
            program,
            metadata,
            location,
        }
    }

    /// Verifies an exact compiler-recorded symbol/type relation without
    /// returning an unrequested ID or a static record. Core has already
    /// required independent same-stream receipts for the two numeric IDs.
    fn debugger_bluets_metadata_symbol_type(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        symbol_id: u32,
        type_id: u32,
    ) -> PageHostReply {
        if !program.is_well_formed() || !metadata.is_well_formed() {
            return invalid_request();
        }
        let runtime_handle = {
            let document = match self.exact_document(tab_id, document_generation) {
                Ok(document) => document,
                Err(reply) => return reply,
            };
            let Some(record) = document.debugger_programs.get(&program.program_handle) else {
                return invalid_request();
            };
            if record.program_generation != program.program_generation
                || record.metadata != Some(metadata)
            {
                return invalid_request();
            }
            record.runtime_handle
        };
        let symbol_type = match self
            .debug_registry
            .get(self.runtime.program_registry(), runtime_handle)
        {
            Ok(retained) => {
                let static_info = retained.static_info();
                let Some(symbol) = static_info
                    .symbols
                    .iter()
                    .find(|symbol| symbol.id.0 == symbol_id)
                else {
                    return invalid_request();
                };
                if symbol
                    .static_type
                    .is_none_or(|static_type| static_type.0 != type_id)
                    || !static_info
                        .types
                        .iter()
                        .any(|static_type| static_type.id.0 == type_id)
                {
                    return invalid_request();
                }
                PageHostDebuggerBlueTsMetadataSymbolType { symbol_id, type_id }
            }
            Err(_) => {
                self.documents
                    .get_mut(&tab_id)
                    .expect("the exact child document remains live after registry validation")
                    .debugger_programs
                    .get_mut(&program.program_handle)
                    .expect("the exact child program remains registered after registry validation")
                    .metadata = None;
                return invalid_request();
            }
        };
        PageHostReply::DebuggerBlueTsMetadataSymbolType {
            tab_id,
            document_generation,
            program,
            metadata,
            symbol_type,
        }
    }

    /// Verifies an exact compiler-recorded symbol/contract relation without
    /// returning an unrequested ID, a contract plan, or a validation result.
    /// Core has already required separate same-stream receipts for both IDs.
    fn debugger_bluets_metadata_symbol_contract(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        symbol_id: u32,
        contract_id: u32,
    ) -> PageHostReply {
        if !program.is_well_formed() || !metadata.is_well_formed() {
            return invalid_request();
        }
        let runtime_handle = {
            let document = match self.exact_document(tab_id, document_generation) {
                Ok(document) => document,
                Err(reply) => return reply,
            };
            let Some(record) = document.debugger_programs.get(&program.program_handle) else {
                return invalid_request();
            };
            if record.program_generation != program.program_generation
                || record.metadata != Some(metadata)
            {
                return invalid_request();
            }
            record.runtime_handle
        };
        let symbol_contract = match self
            .debug_registry
            .get(self.runtime.program_registry(), runtime_handle)
        {
            Ok(retained) => {
                let static_info = retained.static_info();
                let Some(symbol) = static_info
                    .symbols
                    .iter()
                    .find(|symbol| symbol.id.0 == symbol_id)
                else {
                    return invalid_request();
                };
                if symbol
                    .contract
                    .is_none_or(|contract| contract.0 != contract_id)
                    || !static_info
                        .contracts
                        .iter()
                        .any(|contract| contract.id.0 == contract_id)
                {
                    return invalid_request();
                }
                PageHostDebuggerBlueTsMetadataSymbolContract {
                    symbol_id,
                    contract_id,
                }
            }
            Err(_) => {
                self.documents
                    .get_mut(&tab_id)
                    .expect("the exact child document remains live after registry validation")
                    .debugger_programs
                    .get_mut(&program.program_handle)
                    .expect("the exact child program remains registered after registry validation")
                    .metadata = None;
                return invalid_request();
            }
        };
        PageHostReply::DebuggerBlueTsMetadataSymbolContract {
            tab_id,
            document_generation,
            program,
            metadata,
            symbol_contract,
        }
    }

    /// Lists only compiler-minted contract IDs after the caller supplies an
    /// exact child program and metadata attachment. Names, spans, plans,
    /// validation, bytecode, VM objects, and values remain private.
    fn debugger_bluets_metadata_contracts(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    ) -> PageHostReply {
        if !program.is_well_formed() || !metadata.is_well_formed() {
            return invalid_request();
        }
        let runtime_handle = {
            let document = match self.exact_document(tab_id, document_generation) {
                Ok(document) => document,
                Err(reply) => return reply,
            };
            let Some(record) = document.debugger_programs.get(&program.program_handle) else {
                return invalid_request();
            };
            if record.program_generation != program.program_generation
                || record.metadata != Some(metadata)
            {
                return invalid_request();
            }
            record.runtime_handle
        };
        let contracts = match self
            .debug_registry
            .get(self.runtime.program_registry(), runtime_handle)
        {
            Ok(retained) => {
                let static_contracts = &retained.static_info().contracts;
                if static_contracts.len()
                    > usize::try_from(DEBUGGER_STATIC_METADATA_MAX_CONTRACTS).unwrap()
                {
                    return resource_limit();
                }
                let mut identities = BTreeSet::new();
                let mut contracts = Vec::with_capacity(static_contracts.len());
                for contract in static_contracts {
                    if !identities.insert(contract.id.0) {
                        return invalid_request();
                    }
                    contracts.push(PageHostDebuggerBlueTsMetadataContractId {
                        contract_id: contract.id.0,
                    });
                }
                contracts
            }
            Err(_) => {
                self.documents
                    .get_mut(&tab_id)
                    .expect("the exact child document remains live after registry validation")
                    .debugger_programs
                    .get_mut(&program.program_handle)
                    .expect("the exact child program remains registered after registry validation")
                    .metadata = None;
                return invalid_request();
            }
        };
        PageHostReply::DebuggerBlueTsMetadataContracts {
            tab_id,
            document_generation,
            program,
            metadata,
            contracts,
        }
    }

    /// Returns one bounded compiler-produced contract display after the caller
    /// supplies an exact child program and metadata attachment. This private
    /// endpoint never accepts a standalone numeric contract target, and returns
    /// no source span, plan, validation behavior, bytecode, VM object, or value.
    fn debugger_bluets_metadata_contract_display(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        contract_id: u32,
    ) -> PageHostReply {
        if !program.is_well_formed() || !metadata.is_well_formed() {
            return invalid_request();
        }
        let runtime_handle = {
            let document = match self.exact_document(tab_id, document_generation) {
                Ok(document) => document,
                Err(reply) => return reply,
            };
            let Some(record) = document.debugger_programs.get(&program.program_handle) else {
                return invalid_request();
            };
            if record.program_generation != program.program_generation
                || record.metadata != Some(metadata)
            {
                return invalid_request();
            }
            record.runtime_handle
        };
        let contract = match self
            .debug_registry
            .get(self.runtime.program_registry(), runtime_handle)
        {
            Ok(retained) => {
                let Some(contract) = retained
                    .static_info()
                    .contracts
                    .iter()
                    .find(|contract| contract.id.0 == contract_id)
                else {
                    return invalid_request();
                };
                if contract.name.is_empty()
                    || contract.name.len() > DEBUGGER_STATIC_METADATA_CONTRACT_DISPLAY_MAX_BYTES
                {
                    return resource_limit();
                }
                PageHostDebuggerBlueTsMetadataContractDisplay {
                    contract_id,
                    display: contract.name.clone(),
                    root_kind: debugger_contract_root_kind(&contract.plan),
                }
            }
            Err(_) => {
                self.documents
                    .get_mut(&tab_id)
                    .expect("the exact child document remains live after registry validation")
                    .debugger_programs
                    .get_mut(&program.program_handle)
                    .expect("the exact child program remains registered after registry validation")
                    .metadata = None;
                return invalid_request();
            }
        };
        PageHostReply::DebuggerBlueTsMetadataContract {
            tab_id,
            document_generation,
            program,
            metadata,
            contract,
        }
    }

    /// Validates a data-only snapshot under immutable child-selected limits
    /// against one exact private contract target. The reply never echoes the
    /// input or exposes a contract plan, path, expected type, observed type,
    /// bytecode, VM object, or runtime value.
    fn debugger_bluets_metadata_contract_validation(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        contract_id: u32,
        value: CompilerContractValue,
    ) -> PageHostReply {
        if !program.is_well_formed() || !metadata.is_well_formed() {
            return invalid_request();
        }
        let value = match debugger_contract_value(value) {
            Ok(value) => value,
            Err(()) => return invalid_request(),
        };
        let runtime_handle = {
            let document = match self.exact_document(tab_id, document_generation) {
                Ok(document) => document,
                Err(reply) => return reply,
            };
            let Some(record) = document.debugger_programs.get(&program.program_handle) else {
                return invalid_request();
            };
            if record.program_generation != program.program_generation
                || record.metadata != Some(metadata)
            {
                return invalid_request();
            }
            record.runtime_handle
        };
        let valid = match self
            .debug_registry
            .get(self.runtime.program_registry(), runtime_handle)
        {
            Ok(retained) => {
                let Some(contract) = retained
                    .static_info()
                    .contracts
                    .iter()
                    .find(|contract| contract.id.0 == contract_id)
                else {
                    return invalid_request();
                };
                contract
                    .plan
                    .validate_with_limits(&value, debugger_contract_validation_limits())
                    .is_ok()
            }
            Err(_) => {
                self.documents
                    .get_mut(&tab_id)
                    .expect("the exact child document remains live after registry validation")
                    .debugger_programs
                    .get_mut(&program.program_handle)
                    .expect("the exact child program remains registered after registry validation")
                    .metadata = None;
                return invalid_request();
            }
        };
        PageHostReply::DebuggerBlueTsMetadataContractValidation {
            tab_id,
            document_generation,
            program,
            metadata,
            validation: PageHostDebuggerBlueTsMetadataContractValidation { contract_id, valid },
        }
    }

    /// Returns the explicitly authorized, source-text-free provenance for one
    /// source ID that remains owned by this exact child program and metadata
    /// attachment. A failed registry lookup destroys the child-private handle
    /// rather than letting a stale identity probe a successor attachment.
    fn debugger_bluets_metadata_source_provenance(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        source_id: u32,
    ) -> PageHostReply {
        if !program.is_well_formed() || !metadata.is_well_formed() {
            return invalid_request();
        }
        let runtime_handle = {
            let document = match self.exact_document(tab_id, document_generation) {
                Ok(document) => document,
                Err(reply) => return reply,
            };
            let Some(record) = document.debugger_programs.get(&program.program_handle) else {
                return invalid_request();
            };
            if record.program_generation != program.program_generation
                || record.metadata != Some(metadata)
            {
                return invalid_request();
            }
            record.runtime_handle
        };
        let provenance = match self
            .debug_registry
            .get(self.runtime.program_registry(), runtime_handle)
        {
            Ok(retained) => {
                let Some(source) = retained
                    .static_info()
                    .sources
                    .iter()
                    .find(|source| source.id.0 == source_id)
                else {
                    return invalid_request();
                };
                PageHostDebuggerBlueTsMetadataSourceProvenance {
                    source_id,
                    module: source.module.clone(),
                    content_hash: source.content_hash.clone(),
                }
            }
            Err(_) => {
                self.documents
                    .get_mut(&tab_id)
                    .expect("the exact child document remains live after registry validation")
                    .debugger_programs
                    .get_mut(&program.program_handle)
                    .expect("the exact child program remains registered after registry validation")
                    .metadata = None;
                return invalid_request();
            }
        };
        PageHostReply::DebuggerBlueTsMetadataSourceProvenance {
            tab_id,
            document_generation,
            program,
            metadata,
            provenance,
        }
    }

    fn debugger_safe_points(
        &self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    ) -> PageHostReply {
        if !program.is_well_formed() {
            return invalid_request();
        }
        let document = match self.exact_document(tab_id, document_generation) {
            Ok(document) => document,
            Err(reply) => return reply,
        };
        let Some(record) = document.debugger_programs.get(&program.program_handle) else {
            return invalid_request();
        };
        if record.program_generation != program.program_generation {
            return invalid_request();
        }
        let safe_points = match self.runtime.safe_points(
            tab_id,
            record.runtime_handle,
            usize::try_from(PAGE_HOST_DEBUGGER_MAX_SAFE_POINTS_PER_PROGRAM)
                .expect("page-host debugger safe-point cap fits usize"),
        ) {
            Ok(safe_points) => safe_points,
            Err(BlueJsPageRuntimeError::SafePointLimit { .. }) => return resource_limit(),
            Err(_) => return invalid_request(),
        };
        PageHostReply::DebuggerSafePoints {
            tab_id,
            document_generation,
            program,
            safe_points: safe_points
                .into_iter()
                .map(|safe_point| PageHostDebuggerSafePoint {
                    program,
                    code_unit_ordinal: safe_point.code_unit.ordinal(),
                    bytecode_offset: safe_point.bytecode_offset,
                })
                .collect(),
        }
    }

    fn validate_debugger_safe_point(
        &self,
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    ) -> PageHostReply {
        match self.exact_debugger_safe_point(tab_id, document_generation, safe_point) {
            Ok(()) => PageHostReply::DebuggerSafePointValidated {
                tab_id,
                document_generation,
                safe_point,
            },
            Err(reply) => reply,
        }
    }

    /// Revalidates an exact child-private safe-point tuple without returning
    /// a runtime handle, source, bytecode, or VM object to the caller.
    fn exact_debugger_safe_point(
        &self,
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    ) -> Result<(), PageHostReply> {
        if !safe_point.is_well_formed() {
            return Err(invalid_request());
        }
        let document = self.exact_document(tab_id, document_generation)?;
        let Some(record) = document
            .debugger_programs
            .get(&safe_point.program.program_handle)
        else {
            return Err(invalid_request());
        };
        if record.program_generation != safe_point.program.program_generation {
            return Err(invalid_request());
        }
        // Constructing a BlueJS safe point is intentionally not exposed by
        // its public runtime API. Find the exact compiler-recorded boundary
        // first, then ask the runtime to revalidate that authentic tuple.
        let found = match self.runtime.safe_points(
            tab_id,
            record.runtime_handle,
            usize::try_from(PAGE_HOST_DEBUGGER_MAX_SAFE_POINTS_PER_PROGRAM)
                .expect("page-host debugger safe-point cap fits usize"),
        ) {
            Ok(safe_points) => safe_points.into_iter().find(|candidate| {
                candidate.code_unit.ordinal() == safe_point.code_unit_ordinal
                    && candidate.bytecode_offset == safe_point.bytecode_offset
            }),
            Err(BlueJsPageRuntimeError::SafePointLimit { .. }) => return Err(resource_limit()),
            Err(_) => return Err(invalid_request()),
        };
        let Some(found) = found else {
            return Err(invalid_request());
        };
        if self
            .runtime
            .validate_safe_point(tab_id, record.runtime_handle, found)
            .is_err()
        {
            return Err(invalid_request());
        }
        Ok(())
    }

    fn set_debugger_breakpoint(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    ) -> PageHostReply {
        if let Err(reply) = self.exact_debugger_safe_point(tab_id, document_generation, safe_point)
        {
            return reply;
        }
        let document = self
            .documents
            .get_mut(&tab_id)
            .expect("the exact child document remains live after validation");
        let max_breakpoints = usize::try_from(PAGE_HOST_DEBUGGER_MAX_BREAKPOINTS_PER_REALM)
            .expect("page-host debugger breakpoint cap fits usize");
        if !document.debugger_breakpoints.contains(&safe_point)
            && document.debugger_breakpoints.len() == max_breakpoints
        {
            return resource_limit();
        }
        document.debugger_breakpoints.insert(safe_point);
        PageHostReply::DebuggerBreakpointSet {
            tab_id,
            document_generation,
            safe_point,
        }
    }

    fn debugger_breakpoints(&self, tab_id: u64, document_generation: u64) -> PageHostReply {
        let document = match self.exact_document(tab_id, document_generation) {
            Ok(document) => document,
            Err(reply) => return reply,
        };
        let max_breakpoints = usize::try_from(PAGE_HOST_DEBUGGER_MAX_BREAKPOINTS_PER_REALM)
            .expect("page-host debugger breakpoint cap fits usize");
        if document.debugger_breakpoints.len() > max_breakpoints {
            return resource_limit();
        }
        PageHostReply::DebuggerBreakpoints {
            tab_id,
            document_generation,
            safe_points: document.debugger_breakpoints.iter().copied().collect(),
        }
    }

    fn clear_debugger_breakpoint(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    ) -> PageHostReply {
        if let Err(reply) = self.exact_debugger_safe_point(tab_id, document_generation, safe_point)
        {
            return reply;
        }
        let was_present = self
            .documents
            .get_mut(&tab_id)
            .expect("the exact child document remains live after validation")
            .debugger_breakpoints
            .remove(&safe_point);
        PageHostReply::DebuggerBreakpointCleared {
            tab_id,
            document_generation,
            safe_point,
            was_present,
        }
    }

    fn arm_debugger_root_safe_point_breakpoint(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    ) -> PageHostReply {
        if safe_point.code_unit_ordinal != 0 {
            return invalid_debugger_state();
        }
        if let Err(reply) = self.exact_debugger_safe_point(tab_id, document_generation, safe_point)
        {
            return reply;
        }
        let Some(document) = self.documents.get_mut(&tab_id) else {
            return unknown_realm();
        };
        if !document.debugger_execution_control {
            return invalid_debugger_state();
        }
        if document.debugger_execution_states.get(&safe_point.program)
            != Some(&ChildDebuggerExecutionStatus::Pending)
        {
            return invalid_debugger_state();
        }
        let Some(pending) = document
            .pending_debugger_executions
            .iter_mut()
            .find(|pending| pending.program == Some(safe_point.program))
        else {
            return invalid_debugger_state();
        };
        let (DeferredChildExecution::JavaScriptClassic {
            root_safe_point, ..
        }
        | DeferredChildExecution::BlueTsClassic {
            root_safe_point, ..
        }) = &mut pending.execution
        else {
            return invalid_debugger_state();
        };
        if root_safe_point.is_some() {
            return invalid_debugger_state();
        }
        let max = usize::try_from(PAGE_HOST_DEBUGGER_MAX_BREAKPOINTS_PER_REALM)
            .expect("page-host debugger breakpoint cap fits usize");
        if !document.debugger_breakpoints.contains(&safe_point)
            && document.debugger_breakpoints.len() == max
        {
            return resource_limit();
        }
        document.debugger_breakpoints.insert(safe_point);
        *root_safe_point = Some(safe_point);
        PageHostReply::DebuggerRootSafePointBreakpointArmed {
            tab_id,
            document_generation,
            safe_point,
        }
    }

    fn debugger_execution_state(
        &self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    ) -> PageHostReply {
        let document = match self.exact_document(tab_id, document_generation) {
            Ok(document) => document,
            Err(reply) => return reply,
        };
        if !document.debugger_execution_control || !program.is_well_formed() {
            return invalid_debugger_state();
        }
        let Some(status) = document.debugger_execution_states.get(&program).copied() else {
            return invalid_debugger_state();
        };
        PageHostReply::DebuggerExecutionState {
            tab_id,
            document_generation,
            program,
            state: child_debugger_execution_state(status),
        }
    }

    fn resume_debugger_execution(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    ) -> PageHostReply {
        let document = match self.documents.get_mut(&tab_id) {
            Some(document) if document.generation == document_generation => document,
            Some(_) => return stale_document(),
            None => return unknown_realm(),
        };
        if !document.debugger_execution_control || !program.is_well_formed() {
            return invalid_debugger_state();
        }
        let Some(status) = document.debugger_execution_states.get_mut(&program) else {
            return invalid_debugger_state();
        };
        if !matches!(
            status,
            ChildDebuggerExecutionStatus::Paused(_)
                | ChildDebuggerExecutionStatus::SourceStepLimitReached(_)
        ) {
            return invalid_debugger_state();
        }
        *status = ChildDebuggerExecutionStatus::ResumeRequested;
        PageHostReply::DebuggerExecutionResumed {
            tab_id,
            document_generation,
            program,
        }
    }

    fn step_debugger_root_instruction(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    ) -> PageHostReply {
        let document = match self.documents.get_mut(&tab_id) {
            Some(document) if document.generation == document_generation => document,
            Some(_) => return stale_document(),
            None => return unknown_realm(),
        };
        if !document.debugger_execution_control || !program.is_well_formed() {
            return invalid_debugger_state();
        }
        let Some(pending) = document.pending_debugger_executions.front() else {
            return invalid_debugger_state();
        };
        if pending.program != Some(program)
            || !matches!(
                &pending.execution,
                DeferredChildExecution::JavaScriptClassic {
                    root_safe_point: Some(_),
                    ..
                } | DeferredChildExecution::BlueTsClassic {
                    root_safe_point: Some(_),
                    ..
                }
            )
        {
            return invalid_debugger_state();
        }
        let Some(status) = document.debugger_execution_states.get_mut(&program) else {
            return invalid_debugger_state();
        };
        if !matches!(
            status,
            ChildDebuggerExecutionStatus::Paused(_)
                | ChildDebuggerExecutionStatus::SourceStepLimitReached(_)
        ) {
            return invalid_debugger_state();
        }
        *status = ChildDebuggerExecutionStatus::StepRequested;
        PageHostReply::DebuggerExecutionStepRequested {
            tab_id,
            document_generation,
            program,
        }
    }

    fn debugger_bluets_source_span_key(
        &self,
        tab_id: u64,
        document_generation: u64,
        metadata: PageHostDebuggerMetadataHandle,
        safe_point: PageHostDebuggerSafePoint,
    ) -> Result<Option<BlueTsSourceSpanKey>, PageHostReply> {
        let document = self.exact_document(tab_id, document_generation)?;
        let program = safe_point.program;
        let record = document
            .debugger_programs
            .get(&program.program_handle)
            .filter(|record| {
                record.program_generation == program.program_generation
                    && record.metadata == Some(metadata)
            })
            .ok_or_else(invalid_request)?;
        let retained = self
            .debug_registry
            .get(self.runtime.program_registry(), record.runtime_handle)
            .map_err(|_| invalid_request())?;
        let Some(entry) = retained
            .safe_point_map()
            .source_span_for_safe_point(safe_point.code_unit_ordinal, safe_point.bytecode_offset)
        else {
            return Ok(None);
        };
        let mut sources = retained
            .static_info()
            .sources
            .iter()
            .filter(|source| source.module == entry.source);
        let source = sources.next().ok_or_else(invalid_request)?;
        if sources.next().is_some()
            || entry.start_byte >= entry.end_byte
            || entry.end_byte
                > usize::try_from(DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES).unwrap()
        {
            return Err(invalid_request());
        }
        Ok(Some(BlueTsSourceSpanKey {
            source_id: source.id.0,
            start_byte: u32::try_from(entry.start_byte).map_err(|_| invalid_request())?,
            end_byte: u32::try_from(entry.end_byte).map_err(|_| invalid_request())?,
        }))
    }

    fn step_debugger_bluets_source_span(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        metadata: PageHostDebuggerMetadataHandle,
        source_id: u32,
        safe_point: PageHostDebuggerSafePoint,
    ) -> PageHostReply {
        if !metadata.is_well_formed() || !safe_point.is_well_formed() {
            return invalid_request();
        }
        if let Err(reply) = self.exact_debugger_safe_point(tab_id, document_generation, safe_point)
        {
            return reply;
        }
        let program = safe_point.program;
        {
            let document = match self.exact_document(tab_id, document_generation) {
                Ok(document) => document,
                Err(reply) => return reply,
            };
            if !document.debugger_execution_control
                || !document
                    .pending_debugger_executions
                    .front()
                    .is_some_and(|pending| {
                        pending.program == Some(program)
                            && matches!(
                                pending.execution,
                                DeferredChildExecution::BlueTsClassic { .. }
                            )
                    })
                || !matches!(
                    document.debugger_execution_states.get(&program),
                    Some(ChildDebuggerExecutionStatus::Paused(point)
                        | ChildDebuggerExecutionStatus::SourceStepLimitReached(point))
                        if *point == safe_point
                )
            {
                return invalid_debugger_state();
            }
        }
        let origin = match self.debugger_bluets_source_span_key(
            tab_id,
            document_generation,
            metadata,
            safe_point,
        ) {
            Ok(Some(span)) if span.source_id == source_id => span,
            Ok(_) => return invalid_request(),
            Err(reply) => return reply,
        };
        self.documents
            .get_mut(&tab_id)
            .expect("the exact source-step document remains live")
            .debugger_execution_states
            .insert(
                program,
                ChildDebuggerExecutionStatus::BlueTsSourceStepRequested {
                    origin,
                    remaining: MAX_BLUETS_SOURCE_STEP_ROOT_INSTRUCTIONS,
                },
            );
        PageHostReply::DebuggerBlueTsSourceStepRequested {
            tab_id,
            document_generation,
            metadata,
            source_id,
            safe_point,
        }
    }

    fn advance_debugger_execution(
        &mut self,
        tab_id: u64,
        document_generation: u64,
    ) -> PageHostReply {
        let origin = match self.exact_document(tab_id, document_generation) {
            Ok(document) if document.debugger_execution_control => document.origin.clone(),
            Ok(_) => return invalid_debugger_state(),
            Err(reply) => return reply,
        };
        let mut reports = Vec::new();
        while let Some(mut pending) = self
            .documents
            .get_mut(&tab_id)
            .expect("the exact child document remains live while advancing")
            .pending_debugger_executions
            .pop_front()
        {
            let mut paused = false;
            let outcome = match &mut pending.execution {
                DeferredChildExecution::JavaScriptClassic {
                    handle,
                    root_safe_point,
                }
                | DeferredChildExecution::BlueTsClassic {
                    handle,
                    root_safe_point,
                } => {
                    let program = pending
                        .program
                        .expect("every deferred classic has a private debugger program");
                    let status = self
                        .documents
                        .get(&tab_id)
                        .and_then(|document| document.debugger_execution_states.get(&program))
                        .copied()
                        .expect("every deferred classic has scheduler state");
                    match status {
                        ChildDebuggerExecutionStatus::Paused(_)
                        | ChildDebuggerExecutionStatus::SourceStepLimitReached(_) => {
                            paused = true;
                            PageHostScriptOutcome::Executed
                        }
                        ChildDebuggerExecutionStatus::StepRequested => {
                            match self.runtime.step_debugger_root_instruction(tab_id) {
                                Ok(BlueJsPageDebuggerExecutionState::Paused {
                                    bytecode_offset,
                                }) => {
                                    let original = root_safe_point
                                        .expect("a child step requires an armed root safe point");
                                    let successor = PageHostDebuggerSafePoint {
                                        bytecode_offset,
                                        ..original
                                    };
                                    if self
                                        .exact_debugger_safe_point(
                                            tab_id,
                                            document_generation,
                                            successor,
                                        )
                                        .is_ok()
                                    {
                                        self.documents
                                            .get_mut(&tab_id)
                                            .expect("the stepping document remains live")
                                            .debugger_execution_states
                                            .insert(
                                                program,
                                                ChildDebuggerExecutionStatus::Paused(successor),
                                            );
                                        paused = true;
                                        PageHostScriptOutcome::Executed
                                    } else {
                                        self.documents
                                            .get_mut(&tab_id)
                                            .expect("the stepping document remains live")
                                            .debugger_execution_states
                                            .insert(
                                                program,
                                                ChildDebuggerExecutionStatus::Completed,
                                            );
                                        rejected("BlueJS debugger step returned an invalid root boundary")
                                    }
                                }
                                Ok(BlueJsPageDebuggerExecutionState::Completed) => {
                                    self.documents
                                        .get_mut(&tab_id)
                                        .expect("the stepping document remains live")
                                        .debugger_execution_states
                                        .insert(program, ChildDebuggerExecutionStatus::Completed);
                                    PageHostScriptOutcome::Executed
                                }
                                Err(error) => {
                                    self.documents
                                        .get_mut(&tab_id)
                                        .expect("the stepping document remains live")
                                        .debugger_execution_states
                                        .insert(program, ChildDebuggerExecutionStatus::Completed);
                                    rejected(page_runtime_category(error))
                                }
                            }
                        }
                        ChildDebuggerExecutionStatus::BlueTsSourceStepRequested {
                            origin,
                            remaining,
                        } => {
                            if pending.language != PageHostScriptLanguage::BlueTs || remaining == 0
                            {
                                self.documents
                                    .get_mut(&tab_id)
                                    .expect("the source-step document remains live")
                                    .debugger_execution_states
                                    .insert(program, ChildDebuggerExecutionStatus::Completed);
                                rejected("BlueTS source step state was inconsistent")
                            } else {
                                match self.runtime.step_debugger_root_instruction(tab_id) {
                                    Ok(BlueJsPageDebuggerExecutionState::Paused {
                                        bytecode_offset,
                                    }) => {
                                        let original = root_safe_point.expect(
                                            "a BlueTS source step requires an armed root safe point",
                                        );
                                        let successor = PageHostDebuggerSafePoint {
                                            bytecode_offset,
                                            ..original
                                        };
                                        let metadata = self
                                            .documents
                                            .get(&tab_id)
                                            .and_then(|document| {
                                                document
                                                    .debugger_programs
                                                    .get(&program.program_handle)
                                            })
                                            .and_then(|record| record.metadata);
                                        let next_span = if self
                                            .exact_debugger_safe_point(
                                                tab_id,
                                                document_generation,
                                                successor,
                                            )
                                            .is_ok()
                                        {
                                            metadata.ok_or_else(invalid_request).and_then(
                                                |metadata| {
                                                    self.debugger_bluets_source_span_key(
                                                        tab_id,
                                                        document_generation,
                                                        metadata,
                                                        successor,
                                                    )
                                                },
                                            )
                                        } else {
                                            Err(invalid_request())
                                        };
                                        if let Ok(next_span) = next_span {
                                            let next_status = match next_span {
                                                Some(span) if span != origin => {
                                                    ChildDebuggerExecutionStatus::Paused(successor)
                                                }
                                                _ if remaining > 1 => {
                                                    ChildDebuggerExecutionStatus::BlueTsSourceStepRequested {
                                                        origin,
                                                        remaining: remaining - 1,
                                                    }
                                                }
                                                _ => ChildDebuggerExecutionStatus::SourceStepLimitReached(successor),
                                            };
                                            self.documents
                                                .get_mut(&tab_id)
                                                .expect("the source-step document remains live")
                                                .debugger_execution_states
                                                .insert(program, next_status);
                                            paused = true;
                                            PageHostScriptOutcome::Executed
                                        } else {
                                            self.documents
                                                .get_mut(&tab_id)
                                                .expect("the source-step document remains live")
                                                .debugger_execution_states
                                                .insert(
                                                    program,
                                                    ChildDebuggerExecutionStatus::Completed,
                                                );
                                            rejected(
                                                "BlueTS source step lost its verified boundary",
                                            )
                                        }
                                    }
                                    Ok(BlueJsPageDebuggerExecutionState::Completed) => {
                                        self.documents
                                            .get_mut(&tab_id)
                                            .expect("the source-step document remains live")
                                            .debugger_execution_states
                                            .insert(
                                                program,
                                                ChildDebuggerExecutionStatus::Completed,
                                            );
                                        PageHostScriptOutcome::Executed
                                    }
                                    Err(error) => {
                                        self.documents
                                            .get_mut(&tab_id)
                                            .expect("the source-step document remains live")
                                            .debugger_execution_states
                                            .insert(
                                                program,
                                                ChildDebuggerExecutionStatus::Completed,
                                            );
                                        rejected(page_runtime_category(error))
                                    }
                                }
                            }
                        }
                        ChildDebuggerExecutionStatus::Pending
                        | ChildDebuggerExecutionStatus::ResumeRequested => {
                            let result = match (*root_safe_point, status) {
                                (Some(target), ChildDebuggerExecutionStatus::Pending) => self
                                    .runtime
                                    .execute_program_until_debugger_pause_at_root_offset(
                                        tab_id,
                                        *handle,
                                        target.bytecode_offset,
                                    )
                                    .map(|state| (state, true)),
                                (Some(_), ChildDebuggerExecutionStatus::ResumeRequested) => self
                                    .runtime
                                    .resume_debugger_execution(tab_id)
                                    .map(|state| (state, false)),
                                (None, _) => self
                                    .runtime
                                    .execute_program(tab_id, *handle)
                                    .map(|_| (BlueJsPageDebuggerExecutionState::Completed, false)),
                                _ => {
                                    unreachable!("only pending or resuming states reach execution")
                                }
                            };
                            match result {
                                Ok((BlueJsPageDebuggerExecutionState::Paused { .. }, true)) => {
                                    let target = root_safe_point
                                        .expect("a child pause has an armed exact root safe point");
                                    self.documents
                                        .get_mut(&tab_id)
                                        .expect("the advancing document remains live")
                                        .debugger_execution_states
                                        .insert(
                                            program,
                                            ChildDebuggerExecutionStatus::Paused(target),
                                        );
                                    paused = true;
                                    PageHostScriptOutcome::Executed
                                }
                                Ok((BlueJsPageDebuggerExecutionState::Completed, _)) => {
                                    self.documents
                                        .get_mut(&tab_id)
                                        .expect("the advancing document remains live")
                                        .debugger_execution_states
                                        .insert(program, ChildDebuggerExecutionStatus::Completed);
                                    PageHostScriptOutcome::Executed
                                }
                                Ok((BlueJsPageDebuggerExecutionState::Paused { .. }, false)) => {
                                    self.documents
                                        .get_mut(&tab_id)
                                        .expect("the advancing document remains live")
                                        .debugger_execution_states
                                        .insert(program, ChildDebuggerExecutionStatus::Completed);
                                    rejected("BlueJS debugger continuation did not complete after resume")
                                }
                                Err(error) => {
                                    self.documents
                                        .get_mut(&tab_id)
                                        .expect("the advancing document remains live")
                                        .debugger_execution_states
                                        .insert(program, ChildDebuggerExecutionStatus::Completed);
                                    rejected(page_runtime_category(error))
                                }
                            }
                        }
                        ChildDebuggerExecutionStatus::Completed => {
                            rejected("child debugger execution state was inconsistent")
                        }
                    }
                }
                DeferredChildExecution::JavaScriptModule { graph, programs } => {
                    execute_module_graph(
                        &mut self.runtime,
                        tab_id,
                        &origin,
                        graph.clone(),
                        programs.clone(),
                    )
                }
                DeferredChildExecution::BlueTsModule { graph } => execute_bluets_module_graph(
                    &mut self.runtime,
                    &mut self.debug_registry,
                    tab_id,
                    &origin,
                    graph,
                ),
            };
            if paused {
                self.documents
                    .get_mut(&tab_id)
                    .expect("the paused document remains live")
                    .pending_debugger_executions
                    .push_front(pending);
                break;
            }
            reports.push(script_report(
                tab_id,
                document_generation,
                pending.ordinal,
                pending.language,
                pending.kind,
                outcome,
            ));
        }
        if self.refresh_debugger_programs(tab_id).is_err() {
            return self.fail_debugger_execution_document(tab_id);
        }
        PageHostReply::DebuggerExecutionAdvanced {
            tab_id,
            document_generation,
            reports,
        }
    }

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
) -> Result<(), BlueJsPageRuntimeError> {
    let page_dom_profile = script_dom_capability
        .as_ref()
        .map_or(PageDomProfile::Snapshot, ScriptDomCapability::profile);
    let artifact = match page_dom_profile {
        PageDomProfile::Snapshot => PageHostDocumentTypingsV1::generate(),
        PageDomProfile::Text => PageHostDocumentTypingsV1::generate_dom_text(),
        PageDomProfile::Mutation => PageHostDocumentTypingsV1::generate_dom_mutation(),
    };
    let expected_bindings = match page_dom_profile {
        PageDomProfile::Snapshot => page_host_document_runtime_bindings_v1().to_vec(),
        PageDomProfile::Text => page_host_dom_text_runtime_bindings_v1().to_vec(),
        PageDomProfile::Mutation => page_host_dom_mutation_runtime_bindings_v1().to_vec(),
    };
    artifact
        .verify_runtime_bindings(&expected_bindings)
        .map_err(|_| BlueJsPageRuntimeError::InvalidConfiguration)?;
    let binding_inventory = page_host_document_runtime_bindings_v1();
    let document_text = snapshot.document_text.clone();
    let document_origin = snapshot.document_origin.clone();
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
            if page_dom_profile == PageDomProfile::Mutation {
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
            } else {
                installed_bindings
                    .extend_from_slice(&page_host_dom_text_runtime_bindings_v1()[2..]);
            }
        }
        artifact
            .verify_runtime_bindings(&installed_bindings)
            .map_err(|_| RuntimeError::TypeError("host binding inventory mismatch".into()))?;
        Ok(())
    })
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

enum PreparedScript {
    Rejected {
        ordinal: u32,
        language: PageHostScriptLanguage,
        kind: PageHostScriptKind,
        category: &'static str,
    },
    JavaScriptClassic {
        ordinal: u32,
        source: PageHostSource,
        program: BlueJsProgramV1,
    },
    JavaScriptModule {
        ordinal: u32,
        graph: PageHostModuleGraph,
        programs: BTreeMap<String, BlueJsProgramV1>,
    },
    BlueTsClassic {
        ordinal: u32,
        script: Box<DirectScript>,
    },
    BlueTsModule {
        ordinal: u32,
        graph: Box<DirectModuleGraph>,
    },
}

fn prepare_script(script: PageHostScript, page_dom_profile: PageDomProfile) -> PreparedScript {
    let ordinal = script.ordinal;
    let language = script.language;
    let kind = script.kind;
    let prepared = match (language, kind) {
        (PageHostScriptLanguage::JavaScript, PageHostScriptKind::Classic) => {
            prepare_classic(script.graph).map(|(source, program)| {
                PreparedScript::JavaScriptClassic {
                    ordinal,
                    source,
                    program,
                }
            })
        }
        (PageHostScriptLanguage::JavaScript, PageHostScriptKind::Module) => {
            prepare_module_graph(script.graph).map(|(graph, programs)| {
                PreparedScript::JavaScriptModule {
                    ordinal,
                    graph,
                    programs,
                }
            })
        }
        (PageHostScriptLanguage::BlueTs, PageHostScriptKind::Classic) => {
            prepare_bluets_classic(script.graph, page_dom_profile).map(|script| {
                PreparedScript::BlueTsClassic {
                    ordinal,
                    script: Box::new(script),
                }
            })
        }
        (PageHostScriptLanguage::BlueTs, PageHostScriptKind::Module) => {
            prepare_bluets_module_graph(script.graph, page_dom_profile).map(|graph| {
                PreparedScript::BlueTsModule {
                    ordinal,
                    graph: Box::new(graph),
                }
            })
        }
    };
    prepared.unwrap_or_else(|category| PreparedScript::Rejected {
        ordinal,
        language,
        kind,
        category,
    })
}

fn prepare_classic(
    graph: PageHostModuleGraph,
) -> Result<(PageHostSource, BlueJsProgramV1), &'static str> {
    let modules = validate_graph(&graph)?;
    if modules.len() != 1 || !graph.resolutions.is_empty() {
        return Err("classic JavaScript source graph is not closed");
    }
    let source = modules
        .get(&graph.entry)
        .cloned()
        .ok_or("authorized JavaScript graph has no entry")?;
    let program = BlueJsProgramV1::Script(parse(&source.source).map_err(parse_category)?);
    program.compile().map_err(compile_category)?;
    Ok((source, program))
}

fn prepare_module_graph(
    graph: PageHostModuleGraph,
) -> Result<(PageHostModuleGraph, BTreeMap<String, BlueJsProgramV1>), &'static str> {
    let modules = validate_graph(&graph)?;
    let resolutions = validate_resolutions(&graph, &modules)?;
    let mut programs = BTreeMap::new();
    for (module_id, source) in &modules {
        let mut module = parse_module(&source.source).map_err(parse_category)?;
        rewrite_static_module_requests(module_id, &mut module, &resolutions)?;
        let program = BlueJsProgramV1::Module(module);
        // Preflight the full graph before the first program is admitted.
        program.compile().map_err(compile_category)?;
        programs.insert(module_id.clone(), program);
    }
    Ok((graph, programs))
}

/// Prepares an explicit BlueTS classic declaration through the same closed
/// caller-authorized graph validation used for JavaScript. The compiler sees
/// no filesystem, URL, import-map, page-selected profile, or callback
/// authority. Its only ambient declaration comes from the exact generated
/// owner-selected profile: copied snapshots by default, or bounded live DOM
/// text/mutation when the launcher granted that child capability.
fn prepare_bluets_classic(
    graph: PageHostModuleGraph,
    page_dom_profile: PageDomProfile,
) -> Result<DirectScript, &'static str> {
    let modules = validate_graph(&graph)?;
    if modules.len() != 1 || !graph.resolutions.is_empty() {
        return Err("classic BlueTS source graph is not closed");
    }
    let loader = bluets_loader(&graph, modules)?;
    compile_direct_script(
        &graph.entry,
        &loader,
        bluets_compiler_options(&graph, page_dom_profile)?,
    )
    .map_err(bluets_bridge_category)
}

/// Prepares a complete explicit BlueTS module graph without giving BlueTS a
/// resolver beyond the exact static edges serialized by its caller.
fn prepare_bluets_module_graph(
    graph: PageHostModuleGraph,
    page_dom_profile: PageDomProfile,
) -> Result<DirectModuleGraph, &'static str> {
    let modules = validate_graph(&graph)?;
    validate_resolutions(&graph, &modules)?;
    let loader = bluets_loader(&graph, modules)?;
    compile_direct_module_graph(
        &graph.entry,
        &loader,
        bluets_compiler_options(&graph, page_dom_profile)?,
    )
    .map_err(bluets_bridge_category)
}

fn bluets_loader(
    graph: &PageHostModuleGraph,
    modules: BTreeMap<String, PageHostSource>,
) -> Result<AuthorizedModuleLoader, &'static str> {
    let resolutions = graph.resolutions.iter().map(|resolution| {
        AuthorizedModuleResolution::new(
            resolution.from_module.clone(),
            resolution.specifier.clone(),
            resolution.canonical_target.clone(),
        )
    });
    AuthorizedModuleLoader::new(
        modules
            .into_values()
            .map(|source| AuthorizedModule::new(source.canonical_module_id, source.source)),
        resolutions,
    )
    .map_err(|_| "authorized BlueTS graph is invalid")
}

fn bluets_compiler_options(
    graph: &PageHostModuleGraph,
    page_dom_profile: PageDomProfile,
) -> Result<CompilerOptions, &'static str> {
    let ambient_declaration = match page_dom_profile {
        PageDomProfile::Snapshot => PageHostDocumentTypingsV1::generate()
            .verified_ambient_module(&page_host_document_runtime_bindings_v1()),
        PageDomProfile::Text => PageHostDocumentTypingsV1::generate_dom_text()
            .verified_dom_text_ambient_module(&page_host_dom_text_runtime_bindings_v1()),
        PageDomProfile::Mutation => PageHostDocumentTypingsV1::generate_dom_mutation()
            .verified_dom_mutation_ambient_module(&page_host_dom_mutation_runtime_bindings_v1()),
    }
    .map_err(|_| "verified page-host BlueTS typings are unavailable")?;
    let mut options = CompilerOptions {
        runtime_policy: RuntimePolicy::Checked,
        resolver_fingerprint: graph.resolver_fingerprint.clone(),
        require_declared_global_calls: true,
        ambient_declaration_modules: vec![ambient_declaration],
        ..CompilerOptions::default()
    };
    // The transport and BlueJS child already use the smaller page-host source
    // limits. Carry them into BlueTS too so a direct compilation cannot do
    // substantially more work than the closed graph the child admitted.
    options.limits.max_modules = MAX_MODULES_PER_GRAPH;
    options.limits.max_total_source_bytes = MAX_SOURCE_BYTES_PER_DOCUMENT;
    Ok(options)
}

fn execute_bluets_classic(
    runtime: &mut BlueJsPageRuntime,
    debug_registry: &mut DirectDebugRegistry,
    tab_id: u64,
    origin: &BlueJsPageOrigin,
    script: &DirectScript,
) -> PageHostScriptOutcome {
    let attachment =
        match script.attach_debug_in_page_realm(runtime, tab_id, origin, debug_registry) {
            Ok(attachment) => attachment,
            Err(error) => return rejected(bluets_bridge_category(error)),
        };
    match runtime
        .execute_program(tab_id, attachment.handle)
        .map(|_: Value| ())
    {
        Ok(()) => PageHostScriptOutcome::Executed,
        Err(error) => rejected(page_runtime_category(error)),
    }
}

fn execute_bluets_module_graph(
    runtime: &mut BlueJsPageRuntime,
    debug_registry: &mut DirectDebugRegistry,
    tab_id: u64,
    origin: &BlueJsPageOrigin,
    graph: &DirectModuleGraph,
) -> PageHostScriptOutcome {
    let attachment = match graph.attach_debug_in_page_realm(runtime, tab_id, origin, debug_registry)
    {
        Ok(attachment) => attachment,
        Err(error) => return rejected(bluets_bridge_category(error)),
    };
    match runtime.execute_module_graph(
        tab_id,
        attachment.entry.handle,
        attachment.modules.values().map(|module| module.handle),
    ) {
        Ok(_) => PageHostScriptOutcome::Executed,
        Err(error) => rejected(page_runtime_category(error)),
    }
}

fn validate_graph(
    graph: &PageHostModuleGraph,
) -> Result<BTreeMap<String, PageHostSource>, &'static str> {
    if graph.entry.is_empty()
        || graph.entry.contains('\0')
        || graph.resolver_fingerprint.trim().is_empty()
        || graph.resolver_fingerprint.contains('\0')
    {
        return Err("authorized JavaScript graph is invalid");
    }
    if graph.modules.len() > MAX_MODULES_PER_GRAPH {
        return Err("JavaScript module graph exceeds configured policy");
    }
    let mut modules = BTreeMap::new();
    for source in &graph.modules {
        if source.canonical_module_id.is_empty()
            || source.canonical_module_id.contains('\0')
            || source.source.len() > MAX_SOURCE_BYTES_PER_MODULE
            || source.source_hash != page_host::source_hash(&source.source)
        {
            return Err("authorized JavaScript source record is invalid");
        }
        if modules
            .insert(source.canonical_module_id.clone(), source.clone())
            .is_some()
        {
            return Err("authorized JavaScript graph has duplicate modules");
        }
    }
    if !modules.contains_key(&graph.entry) {
        return Err("authorized JavaScript graph has no entry");
    }
    Ok(modules)
}

fn validate_resolutions(
    graph: &PageHostModuleGraph,
    modules: &BTreeMap<String, PageHostSource>,
) -> Result<BTreeMap<(String, String), String>, &'static str> {
    let mut resolutions = BTreeMap::new();
    for PageHostStaticResolution {
        from_module,
        specifier,
        canonical_target,
    } in &graph.resolutions
    {
        if from_module.is_empty()
            || specifier.is_empty()
            || canonical_target.is_empty()
            || from_module.contains('\0')
            || specifier.contains('\0')
            || canonical_target.contains('\0')
            || !modules.contains_key(from_module)
            || !modules.contains_key(canonical_target)
        {
            return Err("authorized JavaScript resolution record is invalid");
        }
        if resolutions
            .insert(
                (from_module.clone(), specifier.clone()),
                canonical_target.clone(),
            )
            .is_some()
        {
            return Err("authorized JavaScript graph has duplicate static resolutions");
        }
    }
    Ok(resolutions)
}

fn rewrite_static_module_requests(
    module_id: &str,
    module: &mut Module,
    resolutions: &BTreeMap<(String, String), String>,
) -> Result<(), &'static str> {
    let resolve = |specifier: &str| {
        resolutions
            .get(&(module_id.to_string(), specifier.to_string()))
            .cloned()
            .ok_or("authorized JavaScript graph is missing a static resolution")
    };
    for import in &mut module.imports {
        import.module_request = resolve(&import.module_request)?;
    }
    for export in &mut module.exports {
        match export {
            blueice_bluejs::ExportEntry::Indirect { module_request, .. }
            | blueice_bluejs::ExportEntry::Star { module_request, .. }
            | blueice_bluejs::ExportEntry::Namespace { module_request, .. } => {
                *module_request = resolve(module_request)?;
            }
            blueice_bluejs::ExportEntry::Local { .. } => {}
        }
    }
    for request in &mut module.requests {
        request.specifier = resolve(&request.specifier)?;
    }
    Ok(())
}

fn execute_classic(
    runtime: &mut BlueJsPageRuntime,
    tab_id: u64,
    origin: &BlueJsPageOrigin,
    source: PageHostSource,
    program: BlueJsProgramV1,
) -> PageHostScriptOutcome {
    let source = match source_identity(&source) {
        Ok(source) => source,
        Err(category) => return rejected(category),
    };
    let handle = match runtime.install_program(tab_id, origin, source, &program) {
        Ok(handle) => handle,
        Err(error) => return rejected(page_runtime_category(error)),
    };
    match runtime.execute_program(tab_id, handle).map(|_: Value| ()) {
        Ok(()) => PageHostScriptOutcome::Executed,
        Err(error) => rejected(page_runtime_category(error)),
    }
}

fn execute_module_graph(
    runtime: &mut BlueJsPageRuntime,
    tab_id: u64,
    origin: &BlueJsPageOrigin,
    graph: PageHostModuleGraph,
    programs: BTreeMap<String, BlueJsProgramV1>,
) -> PageHostScriptOutcome {
    let modules = match validate_graph(&graph) {
        Ok(modules) => modules,
        Err(category) => return rejected(category),
    };
    let mut installed = Vec::with_capacity(programs.len());
    for (module_id, program) in &programs {
        let source = modules
            .get(module_id)
            .expect("prepared module programs derive from exactly this graph");
        let identity = match source_identity(source) {
            Ok(identity) => identity,
            Err(category) => {
                discard_programs(runtime, tab_id, &installed);
                return rejected(category);
            }
        };
        match runtime.install_program(tab_id, origin, identity, program) {
            Ok(handle) => installed.push(handle),
            Err(error) => {
                discard_programs(runtime, tab_id, &installed);
                return rejected(page_runtime_category(error));
            }
        }
    }
    let entry = programs
        .keys()
        .position(|module_id| module_id == &graph.entry)
        .and_then(|index| installed.get(index).copied())
        .expect("prepared graph has an installed entry module");
    match runtime.execute_module_graph(tab_id, entry, installed) {
        Ok(_) => PageHostScriptOutcome::Executed,
        Err(error) => rejected(page_runtime_category(error)),
    }
}

fn source_identity(source: &PageHostSource) -> Result<BlueJsSourceIdentity, &'static str> {
    BlueJsSourceIdentity::new(&source.canonical_module_id, &source.source_hash)
        .map_err(|_| "authorized JavaScript source record is invalid")
}

fn discard_programs(runtime: &mut BlueJsPageRuntime, tab_id: u64, handles: &[BlueJsProgramHandle]) {
    for handle in handles.iter().rev().copied() {
        let _ = runtime.discard_program(tab_id, handle);
    }
}

fn parse_category(_: ParseError) -> &'static str {
    "JavaScript parsing rejected the page script"
}

fn compile_category(_: CompileError) -> &'static str {
    "BlueJS compilation rejected the page script"
}

fn debugger_source_coordinates(
    location: blueice_bluets::DebugSourceLocation,
    start_byte: u32,
    end_byte: u32,
) -> Option<DebuggerSourceCoordinates> {
    let coordinates = DebuggerSourceCoordinates {
        start_line: u32::try_from(location.start.line).ok()?,
        start_column_utf16: u32::try_from(location.start.column_utf16).ok()?,
        end_line: u32::try_from(location.end.line).ok()?,
        end_column_utf16: u32::try_from(location.end.column_utf16).ok()?,
    };
    coordinates
        .is_well_formed_for_range(start_byte, end_byte)
        .then_some(coordinates)
}

fn bluets_bridge_category(error: BridgeError) -> &'static str {
    match error {
        BridgeError::BlueTs(_) => "BlueTS compilation rejected the page script",
        BridgeError::PageRuntime(error) => page_runtime_category(error),
        BridgeError::BlueJs(_) | BridgeError::BlueJsDebug(_) => {
            "BlueJS compilation rejected the direct BlueTS page script"
        }
        BridgeError::UnsupportedRuntimeTarget { .. }
        | BridgeError::InvalidSourceIdentity(_)
        | BridgeError::ProvenanceAttachment(_)
        | BridgeError::DebugAttachment(_) => "BlueTS direct lowering rejected the page script",
    }
}

fn page_runtime_category(error: BlueJsPageRuntimeError) -> &'static str {
    match error {
        BlueJsPageRuntimeError::BytecodeLimit { .. }
        | BlueJsPageRuntimeError::ProgramLimit { .. }
        | BlueJsPageRuntimeError::RealmLimit { .. } => {
            "JavaScript page resource policy rejected the page script"
        }
        BlueJsPageRuntimeError::Runtime(RuntimeError::ModuleResolution(_)) => {
            "authorized JavaScript graph rejected the page script"
        }
        BlueJsPageRuntimeError::Runtime(_) => "BlueJS page execution failed",
        _ => "BlueJS page host rejected the page script",
    }
}

fn rejected(category: &'static str) -> PageHostScriptOutcome {
    PageHostScriptOutcome::Rejected {
        category: category.to_string(),
    }
}

fn script_report(
    tab_id: u64,
    document_generation: u64,
    ordinal: u32,
    language: PageHostScriptLanguage,
    kind: PageHostScriptKind,
    outcome: PageHostScriptOutcome,
) -> PageHostScriptReport {
    PageHostScriptReport {
        tab_id,
        document_generation,
        ordinal,
        language,
        kind,
        outcome,
    }
}

fn child_debugger_execution_state(
    status: ChildDebuggerExecutionStatus,
) -> PageHostDebuggerExecutionState {
    match status {
        ChildDebuggerExecutionStatus::Pending => PageHostDebuggerExecutionState::Pending,
        ChildDebuggerExecutionStatus::Paused(safe_point) => {
            PageHostDebuggerExecutionState::Paused { safe_point }
        }
        ChildDebuggerExecutionStatus::StepRequested
        | ChildDebuggerExecutionStatus::BlueTsSourceStepRequested { .. } => {
            PageHostDebuggerExecutionState::Stepping
        }
        ChildDebuggerExecutionStatus::SourceStepLimitReached(safe_point) => {
            PageHostDebuggerExecutionState::SourceStepLimitReached { safe_point }
        }
        ChildDebuggerExecutionStatus::ResumeRequested => PageHostDebuggerExecutionState::Resuming,
        ChildDebuggerExecutionStatus::Completed => PageHostDebuggerExecutionState::Completed,
    }
}

/// The immutable data-only envelope for debugger contract validation. It is
/// intentionally independent from document/profile limits and cannot be
/// configured over a public or private request.
fn debugger_contract_validation_limits() -> ValidationLimits {
    ValidationLimits {
        max_depth: DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_DEPTH,
        max_collection_entries: DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_COLLECTION_ENTRIES,
        max_nodes: DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_NODES,
        max_string_bytes: DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_STRING_BYTES,
    }
}

/// Converts the shared data-only IPC value to the pure BlueTS validation value
/// while applying the fixed debugger limits before a second recursive tree is
/// retained. It never accepts a JavaScript object, function, getter, proxy,
/// host handle, source graph, or compiler configuration.
fn debugger_contract_root_kind(plan: &ContractPlan) -> DebuggerStaticMetadataContractRootKind {
    let mut root = &plan.root;
    // At most one more hop than the retained definition count is attempted.
    // A missing or cyclic reference stays `Reference`, not a plan disclosure.
    for _ in 0..=plan.definitions.len() {
        match root {
            Contract::Reference(name) => {
                let Some(next) = plan.definitions.get(name) else {
                    return DebuggerStaticMetadataContractRootKind::Reference;
                };
                root = next;
            }
            _ => break,
        }
    }
    match root {
        Contract::Null => DebuggerStaticMetadataContractRootKind::Null,
        Contract::Undefined => DebuggerStaticMetadataContractRootKind::Undefined,
        Contract::Boolean => DebuggerStaticMetadataContractRootKind::Boolean,
        Contract::Number => DebuggerStaticMetadataContractRootKind::Number,
        Contract::String => DebuggerStaticMetadataContractRootKind::String,
        Contract::Literal(_) => DebuggerStaticMetadataContractRootKind::Literal,
        Contract::Array(_) => DebuggerStaticMetadataContractRootKind::Array,
        Contract::Tuple(_) => DebuggerStaticMetadataContractRootKind::Tuple,
        Contract::Record(_) => DebuggerStaticMetadataContractRootKind::Record,
        Contract::Union(_) => DebuggerStaticMetadataContractRootKind::Union,
        Contract::Intersection(_) => DebuggerStaticMetadataContractRootKind::Intersection,
        Contract::Reference(_) => DebuggerStaticMetadataContractRootKind::Reference,
    }
}

fn debugger_contract_value(value: CompilerContractValue) -> Result<ContractValue, ()> {
    fn convert(
        value: CompilerContractValue,
        limits: ValidationLimits,
        depth: usize,
        nodes: &mut usize,
    ) -> Result<ContractValue, ()> {
        if depth > limits.max_depth || *nodes >= limits.max_nodes {
            return Err(());
        }
        *nodes += 1;
        match value {
            CompilerContractValue::Null => Ok(ContractValue::Null),
            CompilerContractValue::Undefined => Ok(ContractValue::Undefined),
            CompilerContractValue::Boolean(value) => Ok(ContractValue::Boolean(value)),
            CompilerContractValue::Number(value) => value
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())
                .map(ContractValue::Number)
                .ok_or(()),
            CompilerContractValue::String(value) => (value.len() <= limits.max_string_bytes)
                .then_some(ContractValue::String(value))
                .ok_or(()),
            CompilerContractValue::Array(values) => {
                if values.len() > limits.max_collection_entries {
                    return Err(());
                }
                values
                    .into_iter()
                    .map(|value| convert(value, limits, depth + 1, nodes))
                    .collect::<Result<Vec<_>, _>>()
                    .map(ContractValue::Array)
            }
            CompilerContractValue::Object(values) => {
                if values.len() > limits.max_collection_entries
                    || values.keys().any(|key| key.len() > limits.max_string_bytes)
                {
                    return Err(());
                }
                values
                    .into_iter()
                    .map(|(key, value)| {
                        convert(value, limits, depth + 1, nodes).map(|value| (key, value))
                    })
                    .collect::<Result<BTreeMap<_, _>, _>>()
                    .map(ContractValue::Object)
            }
        }
    }

    let mut nodes = 0;
    convert(value, debugger_contract_validation_limits(), 0, &mut nodes)
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

/// Private child-connection material a launcher may hand only to the core it
/// is supervising.
///
/// This is deliberately not a frontend setting or a page-visible capability.
/// A caller using [`SpawnedBlueJsHost::spawn_for_core`] must pass it to a
/// trusted core startup boundary and keep the returned supervisor alive for
/// at least as long as that core. The same secret authenticates the child's
/// generation-private script DOM connection when the launcher supplied that
/// socket; it must therefore never be disclosed to frontend or page code.
pub struct BlueJsHostCoreConfig {
    socket_path: PathBuf,
    session_token: String,
}

impl BlueJsHostCoreConfig {
    /// The owner-only socket created for this one child.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// The per-spawn capability for the trusted core's page-host handshake
    /// and the supervised child's matching private script socket.
    /// Callers must not forward it to frontend/page code or log it.
    pub fn session_token(&self) -> &str {
        &self.session_token
    }
}

/// A launcher-owned child and, for the legacy direct-launcher path, its one
/// authenticated private connection. Dropping this handle kills/reaps the
/// process and removes the socket, matching [`crate::SpawnedCore`]'s
/// ownership discipline. [`Self::spawn_for_core`] instead delegates the sole
/// connection to a separately spawned trusted core while retaining process
/// supervision here.
pub struct SpawnedBlueJsHost {
    child: Child,
    socket_path: PathBuf,
    session_token: String,
    stream: Option<UnixStream>,
}

impl SpawnedBlueJsHost {
    /// Spawns the sibling `blueice-bluejs-host` binary, waits for its private
    /// socket, then performs the authenticated v1 handshake before returning
    /// a usable handle. A startup failure always reaps the child and removes
    /// the private socket.
    pub fn spawn() -> io::Result<Self> {
        Self::spawn_with_runtime_limits(BlueJsHostRuntimeLimits::default())
    }

    /// Spawns an isolated child under one owner-selected immutable per-realm
    /// envelope. This is an embedding/launcher construction API, not a
    /// page-host protocol capability and not a child-wide RSS limit.
    pub fn spawn_with_runtime_limits(limits: BlueJsHostRuntimeLimits) -> io::Result<Self> {
        let mut host = Self::spawn_unconnected(limits, None)?;
        if let Err(error) = host.connect_as_launcher() {
            host.reap_after_shutdown();
            return Err(error);
        }
        Ok(host)
    }

    /// Spawns and supervises a child whose sole authenticated connection will
    /// belong to a trusted core. The returned configuration contains the
    /// one-time capability and must be conveyed through a launcher-owned
    /// startup boundary, never from a page or frontend request.
    ///
    /// This does not alter the normal `blueice-launcher` command path. It is
    /// the narrow lifecycle hand-off used by the explicitly opted-in core
    /// adapter; the caller retains this supervisor until core exits.
    pub fn spawn_for_core() -> io::Result<(Self, BlueJsHostCoreConfig)> {
        Self::spawn_for_core_with_runtime_limits(BlueJsHostRuntimeLimits::default())
    }

    /// Equivalent to [`Self::spawn_for_core`], with an immutable launcher
    /// owner-selected per-realm envelope supplied to this one child before it
    /// binds its private socket. The delegated core receives neither the
    /// limits nor an operation to change them.
    pub fn spawn_for_core_with_runtime_limits(
        limits: BlueJsHostRuntimeLimits,
    ) -> io::Result<(Self, BlueJsHostCoreConfig)> {
        let host = Self::spawn_unconnected(limits, None)?;
        let config = BlueJsHostCoreConfig {
            socket_path: host.socket_path.clone(),
            session_token: host.session_token.clone(),
        };
        Ok((host, config))
    }

    /// Gives the supervised child only this generation's launcher-selected
    /// core script socket and its existing per-child capability. The probe is
    /// enabled solely by the explicit DOM lookup proof profile; ordinary
    /// child realms receive no page-visible DOM callback yet.
    pub(crate) fn spawn_for_core_with_script_socket_and_runtime_limits(
        script_socket: &Path,
        limits: BlueJsHostRuntimeLimits,
        enable_dom_lookup_probe: bool,
        enable_dom_text_profile: bool,
        enable_dom_mutation_profile: bool,
    ) -> io::Result<(Self, BlueJsHostCoreConfig)> {
        if !script_socket.is_absolute()
            || [
                enable_dom_lookup_probe,
                enable_dom_text_profile,
                enable_dom_mutation_profile,
            ]
            .into_iter()
            .filter(|enabled| *enabled)
            .count()
                > 1
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "child script socket must be absolute and DOM profiles exclusive",
            ));
        }
        let host = Self::spawn_unconnected(
            limits,
            Some((
                script_socket,
                enable_dom_lookup_probe,
                enable_dom_text_profile,
                enable_dom_mutation_profile,
            )),
        )?;
        let config = BlueJsHostCoreConfig {
            socket_path: host.socket_path.clone(),
            session_token: host.session_token.clone(),
        };
        Ok((host, config))
    }

    fn spawn_unconnected(
        limits: BlueJsHostRuntimeLimits,
        script_socket: Option<(&Path, bool, bool, bool)>,
    ) -> io::Result<Self> {
        limits
            .runtime_config()
            .map_err(|message| io::Error::new(io::ErrorKind::InvalidInput, message))?;
        let this_exe = std::env::current_exe()?;
        let binary = sibling_bluejs_host_binary(&this_exe);
        let socket_path = unique_bluejs_host_socket_path();
        let token = secure_session_token()?;
        let _ = fs::remove_file(&socket_path);
        let mut command = Command::new(&binary);
        command
            .arg("--socket")
            .arg(&socket_path)
            .arg("--session-token")
            .arg(&token)
            .arg("--max-realms")
            .arg(limits.max_realms.to_string())
            .arg("--max-programs-per-realm")
            .arg(limits.max_programs_per_realm.to_string())
            .arg("--max-bytecode-bytes-per-realm")
            .arg(limits.max_bytecode_bytes_per_realm.to_string())
            .arg("--max-heap-bytes-per-realm")
            .arg(limits.max_heap_bytes_per_realm.to_string())
            .arg("--max-reserved-programs")
            .arg(limits.max_reserved_programs.to_string())
            .arg("--max-reserved-bytecode-bytes")
            .arg(limits.max_reserved_bytecode_bytes.to_string())
            .arg("--max-reserved-heap-bytes")
            .arg(limits.max_reserved_heap_bytes.to_string());
        if let Some((
            script_socket,
            enable_dom_lookup_probe,
            enable_dom_text_profile,
            enable_dom_mutation_profile,
        )) = script_socket
        {
            command.arg("--script-socket").arg(script_socket);
            if enable_dom_lookup_probe {
                command.arg("--enable-dom-lookup-probe");
            }
            if enable_dom_text_profile {
                command.arg("--enable-dom-text-profile");
            }
            if enable_dom_mutation_profile {
                command.arg("--enable-dom-mutation-profile");
            }
        }
        let mut child = command.spawn()?;

        let started = wait_for_child_socket(&mut child, &socket_path, STARTUP_TIMEOUT);
        if let Err(error) = started {
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_file(&socket_path);
            return Err(error);
        }
        Ok(Self {
            child,
            socket_path,
            session_token: token,
            stream: None,
        })
    }

    fn connect_as_launcher(&mut self) -> io::Result<()> {
        // A Unix socket pathname becomes visible at bind(2), before listen(2)
        // has completed. Under load, the launcher can observe the path in
        // `spawn_unconnected` and race that small interval. Only retry those
        // transient startup errors; never retry a rejected handshake.
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        let mut stream = loop {
            match UnixStream::connect(&self.socket_path) {
                Ok(stream) => break stream,
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
                    ) =>
                {
                    if let Some(status) = self.child.try_wait()? {
                        return Err(io::Error::other(format!(
                            "BlueJS page-host child exited before accepting its connection: {status}"
                        )));
                    }
                    if Instant::now() >= deadline {
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "BlueJS page-host child never accepted its private connection",
                        ));
                    }
                    thread::sleep(Duration::from_millis(20));
                }
                Err(error) => return Err(error),
            }
        };
        let hello = PageHostRequest::Hello {
            protocol_version: page_host::PAGE_HOST_PROTOCOL_VERSION,
            session_token: self.session_token.clone(),
        };
        let handshake = (|| -> io::Result<PageHostReply> {
            page_host::write_page_host_request(&mut stream, &hello)?;
            page_host::read_page_host_reply(&mut stream)
        })();
        match handshake {
            Ok(PageHostReply::HelloAck {
                protocol_version: page_host::PAGE_HOST_PROTOCOL_VERSION,
            }) => {
                self.stream = Some(stream);
                Ok(())
            }
            Ok(reply) => Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("BlueJS page host rejected launcher handshake: {reply:?}"),
            )),
            Err(error) => Err(error),
        }
    }

    /// Sends one post-handshake request across the private child connection.
    /// This intentionally exposes only typed, source-free protocol values,
    /// never the child VM or its program registry.
    pub fn request(&mut self, request: PageHostRequest) -> io::Result<PageHostReply> {
        if matches!(request, PageHostRequest::Hello { .. }) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "BlueJS page-host handshake is already complete",
            ));
        }
        let stream = self.stream.as_mut().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "BlueJS page-host connection is delegated to the trusted core",
            )
        })?;
        page_host::write_page_host_request(stream, &request)?;
        page_host::read_page_host_reply(stream)
    }

    /// Applies one caller-authorized document to the isolated child.
    pub fn synchronize_document(
        &mut self,
        document: PageHostDocument,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::SynchronizeDocument { document })
    }

    /// Requests clean child shutdown, then reaps the child. `Drop` provides a
    /// hard-kill fallback when a process is hung or the transport is broken.
    pub fn shutdown(&mut self) -> io::Result<()> {
        let reply = self.request(PageHostRequest::Shutdown)?;
        if reply != PageHostReply::ShutdownAck {
            return Err(io::Error::other("BlueJS page host rejected shutdown"));
        }
        self.reap_after_shutdown();
        Ok(())
    }

    /// The private path is observable only for lifecycle tests and launcher
    /// cleanup diagnostics; callers must not use it as a second connection.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    fn reap_after_shutdown(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                Ok(None) | Err(_) => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    break;
                }
            }
        }
        let _ = fs::remove_file(&self.socket_path);
    }
}

impl Drop for SpawnedBlueJsHost {
    fn drop(&mut self) {
        self.reap_after_shutdown();
    }
}

fn sibling_bluejs_host_binary(this_exe: &Path) -> PathBuf {
    let name = if cfg!(windows) {
        "blueice-bluejs-host.exe"
    } else {
        "blueice-bluejs-host"
    };
    let directory = this_exe.parent().unwrap_or_else(|| Path::new("."));
    let directory = if directory.file_name().is_some_and(|name| name == "deps") {
        directory.parent().unwrap_or(directory)
    } else {
        directory
    };
    directory.join(name)
}

fn unique_bluejs_host_socket_path() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "blueice-launcher-bluejs-host-{}-{count}.sock",
        std::process::id()
    ))
}

fn secure_session_token() -> io::Result<String> {
    use std::io::Read;

    let mut bytes = [0u8; 32];
    fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    let mut token = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(token, "{byte:02x}").expect("writing to a String cannot fail");
    }
    Ok(token)
}

fn wait_for_child_socket(child: &mut Child, path: &Path, timeout: Duration) -> io::Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        if path.exists() {
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!(
                "BlueJS page-host child exited before binding its socket: {status}"
            )));
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "BlueJS page-host child never created its socket at {}",
                    path.display()
                ),
            ));
        }
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_wrapper_rechecks_core_liveness_and_does_not_survive_realm_replacement() {
        let socket_path = std::env::temp_dir().join(format!(
            "bi-dom-wrapper-lifetime-{}.sock",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&socket_path);
        let listener = UnixListener::bind(&socket_path).unwrap();
        let capability = "a".repeat(script::SCRIPT_SESSION_TOKEN_HEX_BYTES);
        let server_capability = capability.clone();
        let server = std::thread::spawn(move || {
            let (mut peer, _) = listener.accept().unwrap();
            peer.set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            assert_eq!(
                script::read_script_request(&mut peer).unwrap(),
                ScriptRequest::Hello {
                    protocol_version: script::SCRIPT_PROTOCOL_VERSION,
                    session_token: server_capability,
                }
            );
            script::write_script_reply(
                &mut peer,
                &ScriptReply::HelloAck {
                    protocol_version: script::SCRIPT_PROTOCOL_VERSION,
                },
            )
            .unwrap();
            let target = ScriptDocumentTarget {
                tab_id: 7,
                document_generation: 1,
            };
            for (request_id, operation, reply) in [
                (
                    1,
                    ScriptRequest::GetElementById {
                        target,
                        id: "present".to_string(),
                    },
                    ScriptReply::Node { node: Some(11) },
                ),
                (
                    2,
                    ScriptRequest::ValidateNode { target, node: 11 },
                    ScriptReply::Ack,
                ),
                (
                    3,
                    ScriptRequest::ValidateNode { target, node: 11 },
                    ScriptReply::Error {
                        message: "unknown node 11".to_string(),
                    },
                ),
            ] {
                assert_eq!(
                    script::read_script_request(&mut peer).unwrap(),
                    ScriptRequest::Call {
                        request_id,
                        request: Box::new(operation),
                    }
                );
                script::write_script_reply(
                    &mut peer,
                    &ScriptReply::CallResult {
                        request_id,
                        target,
                        reply: Box::new(reply),
                    },
                )
                .unwrap();
            }
            assert_eq!(
                script::read_script_request(&mut peer).unwrap_err().kind(),
                io::ErrorKind::UnexpectedEof
            );
        });

        let mut host = BlueJsChildHost::default();
        host.configure_script_dom_capability(socket_path.clone(), capability, true, false, false)
            .unwrap();
        let outcomes = |reply: PageHostReply| match reply {
            PageHostReply::Synchronized { reports, .. } => reports
                .into_iter()
                .map(|report| report.outcome)
                .collect::<Vec<_>>(),
            other => panic!("expected a synchronized child document, got {other:?}"),
        };
        let first = outcomes(host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(
                1,
                vec![
                    classic(
                        0,
                        "globalThis.saved = blueiceTestGetElementById('present'); if (!saved.blueiceTestRequireLive()) throw 'not live';",
                    ),
                    classic(1, "saved.blueiceTestRequireLive();"),
                ],
            ),
        }));
        assert!(matches!(
            first.as_slice(),
            [
                PageHostScriptOutcome::Executed,
                PageHostScriptOutcome::Rejected { .. }
            ]
        ));
        let second = outcomes(host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(
                2,
                vec![classic(
                    0,
                    "if (typeof saved !== 'undefined') throw 'old wrapper';",
                )],
            ),
        }));
        assert_eq!(second, vec![PageHostScriptOutcome::Executed]);
        assert!(matches!(
            host.handle_request(PageHostRequest::CloseRealm {
                tab_id: 7,
                document_generation: 2,
            }),
            PageHostReply::RealmClosed { .. }
        ));
        let third = outcomes(host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(
                3,
                vec![classic(
                    0,
                    "if (typeof saved !== 'undefined') throw 'closed wrapper';",
                )],
            ),
        }));
        assert_eq!(third, vec![PageHostScriptOutcome::Executed]);
        server.join().unwrap();
        std::fs::remove_file(socket_path).unwrap();
    }

    #[test]
    fn mutation_profile_uses_exact_core_creation_and_append_calls() {
        let socket_path =
            std::env::temp_dir().join(format!("bi-dom-mutation-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&socket_path);
        let listener = UnixListener::bind(&socket_path).unwrap();
        let capability = "b".repeat(script::SCRIPT_SESSION_TOKEN_HEX_BYTES);
        let server_capability = capability.clone();
        let server = std::thread::spawn(move || {
            let (mut peer, _) = listener.accept().unwrap();
            peer.set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            assert_eq!(
                script::read_script_request(&mut peer).unwrap(),
                ScriptRequest::Hello {
                    protocol_version: script::SCRIPT_PROTOCOL_VERSION,
                    session_token: server_capability,
                }
            );
            script::write_script_reply(
                &mut peer,
                &ScriptReply::HelloAck {
                    protocol_version: script::SCRIPT_PROTOCOL_VERSION,
                },
            )
            .unwrap();
            let target = ScriptDocumentTarget {
                tab_id: 7,
                document_generation: 1,
            };
            for (request_id, operation, reply) in [
                (
                    1,
                    ScriptRequest::GetElementById {
                        target,
                        id: "target".to_string(),
                    },
                    ScriptReply::Node { node: Some(11) },
                ),
                (
                    2,
                    ScriptRequest::CreateElement {
                        target,
                        tag_name: "span".to_string(),
                    },
                    ScriptReply::NodeCreated { node: 12 },
                ),
                (
                    3,
                    ScriptRequest::CreateTextNode {
                        target,
                        data: "new text".to_string(),
                    },
                    ScriptReply::NodeCreated { node: 13 },
                ),
                (
                    4,
                    ScriptRequest::AppendChild {
                        target,
                        parent: 12,
                        child: 13,
                    },
                    ScriptReply::Ack,
                ),
                (
                    5,
                    ScriptRequest::AppendChild {
                        target,
                        parent: 11,
                        child: 12,
                    },
                    ScriptReply::Ack,
                ),
                (
                    6,
                    ScriptRequest::GetTextContent { target, node: 11 },
                    ScriptReply::Text {
                        value: "new text".to_string(),
                    },
                ),
            ] {
                assert_eq!(
                    script::read_script_request(&mut peer).unwrap(),
                    ScriptRequest::Call {
                        request_id,
                        request: Box::new(operation),
                    }
                );
                script::write_script_reply(
                    &mut peer,
                    &ScriptReply::CallResult {
                        request_id,
                        target,
                        reply: Box::new(reply),
                    },
                )
                .unwrap();
            }
            assert_eq!(
                script::read_script_request(&mut peer).unwrap_err().kind(),
                io::ErrorKind::UnexpectedEof
            );
        });

        let mut host = BlueJsChildHost::default();
        host.configure_script_dom_capability(socket_path.clone(), capability, false, false, true)
            .unwrap();
        let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(
                1,
                vec![classic(
                    0,
                    "let parent = document.getElementById('target'); \
                     let child = document.createElement('span'); \
                     let text = document.createTextNode('new text'); \
                     if (child.appendChild(text) !== text) throw 'text identity'; \
                     if (parent.appendChild(child) !== child) throw 'child identity'; \
                     if (parent.textContent !== 'new text') throw 'mutation'; \
                     try { parent.appendChild({}); throw 'forged accepted'; } \
                     catch (error) { if (!(error instanceof TypeError)) throw error; }",
                )],
            ),
        });
        let PageHostReply::Synchronized { reports, .. } = reply else {
            panic!("expected synchronized document");
        };
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].outcome, PageHostScriptOutcome::Executed);
        drop(host);
        server.join().unwrap();
        std::fs::remove_file(socket_path).unwrap();
    }

    #[test]
    fn child_revokes_dom_streams_on_navigation_and_close_and_rejects_old_replies() {
        let socket_path =
            std::env::temp_dir().join(format!("bi-dom-revoke-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&socket_path);
        let listener = UnixListener::bind(&socket_path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let capability = "a".repeat(script::SCRIPT_SESSION_TOKEN_HEX_BYTES);
        let server_capability = capability.clone();
        let server = std::thread::spawn(move || {
            // Both successors reuse call ID 1 on a fresh stream. First give
            // each an old-document result, then a valid result after the
            // child fails closed and reconnects.
            for (request_generation, reply_generation) in [(1, 1), (2, 1), (2, 2), (3, 2), (3, 3)] {
                let deadline = Instant::now() + Duration::from_secs(10);
                let mut peer = loop {
                    match listener.accept() {
                        Ok((peer, _)) => break peer,
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "child did not open DOM stream");
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("failed to accept child DOM stream: {error}"),
                    }
                };
                peer.set_nonblocking(false).unwrap();
                peer.set_read_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                assert_eq!(
                    script::read_script_request(&mut peer).unwrap(),
                    ScriptRequest::Hello {
                        protocol_version: script::SCRIPT_PROTOCOL_VERSION,
                        session_token: server_capability.clone(),
                    }
                );
                script::write_script_reply(
                    &mut peer,
                    &ScriptReply::HelloAck {
                        protocol_version: script::SCRIPT_PROTOCOL_VERSION,
                    },
                )
                .unwrap();
                assert_eq!(
                    script::read_script_request(&mut peer).unwrap(),
                    ScriptRequest::Call {
                        request_id: 1,
                        request: Box::new(ScriptRequest::GetElementById {
                            target: ScriptDocumentTarget {
                                tab_id: 7,
                                document_generation: request_generation,
                            },
                            id: "present".to_string(),
                        }),
                    }
                );
                script::write_script_reply(
                    &mut peer,
                    &ScriptReply::CallResult {
                        request_id: 1,
                        target: ScriptDocumentTarget {
                            tab_id: 7,
                            document_generation: reply_generation,
                        },
                        reply: Box::new(ScriptReply::Node { node: Some(11) }),
                    },
                )
                .unwrap();
                assert_eq!(
                    script::read_script_request(&mut peer).unwrap_err().kind(),
                    io::ErrorKind::UnexpectedEof,
                    "navigation, tab close, or a stale reply must close the old DOM stream"
                );
            }
        });

        let mut host = BlueJsChildHost::default();
        host.configure_script_dom_capability(socket_path.clone(), capability, true, false, false)
            .unwrap();
        let script = |ordinal| {
            classic(
                ordinal,
                "if (!blueiceTestHasElementById('present')) throw 'missing';",
            )
        };
        let outcomes = |reply: PageHostReply| match reply {
            PageHostReply::Synchronized { reports, .. } => reports
                .into_iter()
                .map(|report| report.outcome)
                .collect::<Vec<_>>(),
            other => panic!("expected a synchronized child document, got {other:?}"),
        };
        assert_eq!(
            outcomes(host.handle_request(PageHostRequest::SynchronizeDocument {
                document: document(1, vec![script(0)]),
            })),
            vec![PageHostScriptOutcome::Executed]
        );
        let second_outcomes = outcomes(host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(2, vec![script(0), script(1)]),
        }));
        assert!(matches!(
            second_outcomes.as_slice(),
            [
                PageHostScriptOutcome::Rejected { .. },
                PageHostScriptOutcome::Executed
            ]
        ));
        assert_eq!(
            host.handle_request(PageHostRequest::CloseRealm {
                tab_id: 7,
                document_generation: 2,
            }),
            PageHostReply::RealmClosed {
                tab_id: 7,
                document_generation: 2,
            }
        );
        let third_outcomes = outcomes(host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(3, vec![script(0), script(1)]),
        }));
        assert!(matches!(
            third_outcomes.as_slice(),
            [
                PageHostScriptOutcome::Rejected { .. },
                PageHostScriptOutcome::Executed
            ]
        ));
        assert_eq!(
            host.handle_request(PageHostRequest::CloseRealm {
                tab_id: 7,
                document_generation: 3,
            }),
            PageHostReply::RealmClosed {
                tab_id: 7,
                document_generation: 3,
            }
        );
        server.join().unwrap();
        std::fs::remove_file(socket_path).unwrap();
    }

    #[test]
    fn child_rejects_unbound_or_mismatched_dom_replies_and_closes_the_stream() {
        let target = ScriptDocumentTarget {
            tab_id: 7,
            document_generation: 9,
        };
        for reply in [
            ScriptReply::CallResult {
                request_id: 2,
                target,
                reply: Box::new(ScriptReply::Node { node: Some(11) }),
            },
            ScriptReply::CallResult {
                request_id: 1,
                target: ScriptDocumentTarget {
                    document_generation: 10,
                    ..target
                },
                reply: Box::new(ScriptReply::Node { node: Some(11) }),
            },
            ScriptReply::Node { node: Some(11) },
            ScriptReply::CallResult {
                request_id: 1,
                target,
                reply: Box::new(ScriptReply::CallResult {
                    request_id: 1,
                    target,
                    reply: Box::new(ScriptReply::Node { node: Some(11) }),
                }),
            },
        ] {
            let (client_stream, mut fake_core) = UnixStream::pair().unwrap();
            let mut client = ScriptDomClient {
                capability: ScriptDomCapability {
                    socket_path: PathBuf::new(),
                    session_token: String::new(),
                    enable_lookup_probe: true,
                    enable_dom_text_profile: false,
                    enable_dom_mutation_profile: false,
                },
                stream: Some(client_stream),
                next_call_id: 1,
            };
            let fake = std::thread::spawn(move || {
                assert_eq!(
                    script::read_script_request(&mut fake_core).unwrap(),
                    ScriptRequest::Call {
                        request_id: 1,
                        request: Box::new(ScriptRequest::GetElementById {
                            target,
                            id: "target".to_string(),
                        }),
                    }
                );
                script::write_script_reply(&mut fake_core, &reply).unwrap();
            });
            assert_eq!(
                client
                    .has_element_by_id(target, "target".to_string())
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::InvalidData
            );
            assert!(client.stream.is_none());
            fake.join().unwrap();
        }
    }

    fn graph(entry: &str, modules: Vec<PageHostSource>) -> PageHostModuleGraph {
        PageHostModuleGraph {
            entry: entry.to_string(),
            modules,
            resolutions: Vec::new(),
            resolver_fingerprint: "core-page-loader-v1".to_string(),
        }
    }

    fn document(generation: u64, scripts: Vec<PageHostScript>) -> PageHostDocument {
        document_with_snapshot(
            generation,
            "test document snapshot".to_string(),
            "https://example.test".to_string(),
            scripts,
        )
    }

    fn document_with_snapshot(
        generation: u64,
        document_text: String,
        document_origin: String,
        scripts: Vec<PageHostScript>,
    ) -> PageHostDocument {
        PageHostDocument {
            tab_id: 7,
            document_generation: generation,
            snapshot: PageHostDocumentSnapshot {
                document_text,
                document_origin,
            },
            debugger_execution_control: false,
            scripts,
        }
    }

    fn debugger_document(generation: u64, scripts: Vec<PageHostScript>) -> PageHostDocument {
        let mut document = document(generation, scripts);
        document.debugger_execution_control = true;
        document
    }

    fn classic(ordinal: u32, source: &str) -> PageHostScript {
        let id = format!("blueice://page/inline-{ordinal}.js");
        PageHostScript {
            ordinal,
            language: PageHostScriptLanguage::JavaScript,
            kind: PageHostScriptKind::Classic,
            graph: graph(&id, vec![PageHostSource::new(id.clone(), source)]),
        }
    }

    fn blue_ts_classic(ordinal: u32, source: &str) -> PageHostScript {
        let id = format!("blueice://page/inline-{ordinal}.ts");
        PageHostScript {
            ordinal,
            language: PageHostScriptLanguage::BlueTs,
            kind: PageHostScriptKind::Classic,
            graph: graph(&id, vec![PageHostSource::new(id.clone(), source)]),
        }
    }

    #[test]
    fn child_wide_reservations_reject_new_tabs_but_allow_replacement_and_release() {
        for constrained_resource in ["programs", "bytecode", "heap"] {
            let heap_per_realm = BlueJsHostRuntimeLimits::default().max_heap_bytes_per_realm;
            let mut limits = BlueJsHostRuntimeLimits {
                max_realms: 2,
                max_programs_per_realm: 2,
                max_bytecode_bytes_per_realm: 4096,
                max_heap_bytes_per_realm: heap_per_realm,
                max_reserved_programs: 4,
                max_reserved_bytecode_bytes: 8192,
                max_reserved_heap_bytes: heap_per_realm.saturating_mul(2),
            };
            match constrained_resource {
                "programs" => limits.max_reserved_programs = limits.max_programs_per_realm,
                "bytecode" => {
                    limits.max_reserved_bytecode_bytes = limits.max_bytecode_bytes_per_realm;
                }
                "heap" => limits.max_reserved_heap_bytes = limits.max_heap_bytes_per_realm,
                _ => unreachable!(),
            }
            let mut host = BlueJsChildHost::with_runtime_limits(limits).unwrap();
            let first = document(1, vec![classic(0, "globalThis.answer = 42;")]);
            assert!(matches!(
                host.handle_request(PageHostRequest::SynchronizeDocument { document: first }),
                PageHostReply::Synchronized { reports, .. }
                    if matches!(reports.as_slice(), [PageHostScriptReport {
                        outcome: PageHostScriptOutcome::Executed,
                        ..
                    }])
            ));

            let mut second = document(1, vec![classic(0, "globalThis.other = 7;")]);
            second.tab_id = 8;
            assert!(
                matches!(
                    host.handle_request(PageHostRequest::SynchronizeDocument {
                        document: second.clone()
                    }),
                    PageHostReply::Error {
                        code: PageHostErrorCode::ResourceLimit,
                        ..
                    }
                ),
                "{constrained_resource}"
            );
            assert!(
                matches!(
                    host.handle_request(PageHostRequest::SynchronizeDocument {
                        document: document(2, vec![classic(0, "globalThis.answer = 43;")])
                    }),
                    PageHostReply::Synchronized { reports, .. }
                        if matches!(reports.as_slice(), [PageHostScriptReport {
                            outcome: PageHostScriptOutcome::Executed,
                            ..
                        }])
                ),
                "{constrained_resource}"
            );
            assert!(matches!(
                host.handle_request(PageHostRequest::CloseRealm {
                    tab_id: 7,
                    document_generation: 2,
                }),
                PageHostReply::RealmClosed { .. }
            ));
            assert!(
                matches!(
                    host.handle_request(PageHostRequest::SynchronizeDocument { document: second }),
                    PageHostReply::Synchronized { reports, .. }
                        if matches!(reports.as_slice(), [PageHostScriptReport {
                            outcome: PageHostScriptOutcome::Executed,
                            ..
                        }])
                ),
                "{constrained_resource}"
            );
        }
    }

    #[test]
    fn child_wide_reservation_must_cover_one_full_realm() {
        let mut limits = BlueJsHostRuntimeLimits::default();
        limits.max_reserved_programs = limits.max_programs_per_realm - 1;
        assert!(limits.runtime_config().is_err());
        limits = BlueJsHostRuntimeLimits::default();
        limits.max_reserved_bytecode_bytes = limits.max_bytecode_bytes_per_realm - 1;
        assert!(limits.runtime_config().is_err());
        limits = BlueJsHostRuntimeLimits::default();
        limits.max_reserved_heap_bytes = limits.max_heap_bytes_per_realm - 1;
        assert!(limits.runtime_config().is_err());
    }

    #[test]
    fn child_wide_actual_usage_tracks_live_realms_not_predecessors_or_reservations() {
        let mut host = BlueJsChildHost::default();
        assert_eq!(
            host.handle_request(PageHostRequest::GetChildStats),
            PageHostReply::ChildStats(PageHostChildStats {
                realm_count: 0,
                program_count: 0,
                bytecode_bytes: 0,
                heap_bytes: 0,
            })
        );
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: document(1, vec![classic(0, "globalThis.first = 1;")]),
            }),
            PageHostReply::Synchronized { .. }
        ));
        let mut second = document(1, vec![classic(0, "globalThis.second = 2;")]);
        second.tab_id = 8;
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument { document: second }),
            PageHostReply::Synchronized { .. }
        ));
        let PageHostReply::RealmStats(first) =
            host.handle_request(PageHostRequest::GetRealmStats {
                tab_id: 7,
                document_generation: 1,
            })
        else {
            panic!("first child realm must remain live");
        };
        let PageHostReply::RealmStats(second) =
            host.handle_request(PageHostRequest::GetRealmStats {
                tab_id: 8,
                document_generation: 1,
            })
        else {
            panic!("second child realm must remain live");
        };
        assert_eq!(
            host.handle_request(PageHostRequest::GetChildStats),
            PageHostReply::ChildStats(PageHostChildStats {
                realm_count: 2,
                program_count: u64::from(first.program_count + second.program_count),
                bytecode_bytes: first.bytecode_bytes + second.bytecode_bytes,
                heap_bytes: first.heap_bytes + second.heap_bytes,
            })
        );
        assert!(first.bytecode_bytes > 0 && second.bytecode_bytes > 0);
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: document(2, Vec::new()),
            }),
            PageHostReply::Synchronized { .. }
        ));
        let PageHostReply::RealmStats(replaced) =
            host.handle_request(PageHostRequest::GetRealmStats {
                tab_id: 7,
                document_generation: 2,
            })
        else {
            panic!("replacement child realm must be live");
        };
        assert_eq!(replaced.program_count, 0);
        assert_eq!(
            host.handle_request(PageHostRequest::GetChildStats),
            PageHostReply::ChildStats(PageHostChildStats {
                realm_count: 2,
                program_count: u64::from(second.program_count),
                bytecode_bytes: second.bytecode_bytes,
                heap_bytes: replaced.heap_bytes + second.heap_bytes,
            })
        );
        assert!(matches!(
            host.handle_request(PageHostRequest::CloseRealm {
                tab_id: 8,
                document_generation: 1,
            }),
            PageHostReply::RealmClosed { .. }
        ));
        assert_eq!(
            host.handle_request(PageHostRequest::GetChildStats),
            PageHostReply::ChildStats(PageHostChildStats {
                realm_count: 1,
                program_count: 0,
                bytecode_bytes: 0,
                heap_bytes: replaced.heap_bytes,
            })
        );
    }

    #[test]
    fn contract_root_kind_resolves_local_definitions_without_exposing_a_plan() {
        let plan = ContractPlan {
            id: "private".to_string(),
            root: Contract::Reference("Alias".to_string()),
            definitions: BTreeMap::from([
                (
                    "Alias".to_string(),
                    Contract::Reference("Shape".to_string()),
                ),
                ("Shape".to_string(), Contract::Record(Vec::new())),
            ]),
            fingerprint: "private".to_string(),
        };
        assert_eq!(
            debugger_contract_root_kind(&plan),
            DebuggerStaticMetadataContractRootKind::Record
        );
        let cyclic = ContractPlan {
            definitions: BTreeMap::from([(
                "Alias".to_string(),
                Contract::Reference("Alias".to_string()),
            )]),
            ..plan
        };
        assert_eq!(
            debugger_contract_root_kind(&cyclic),
            DebuggerStaticMetadataContractRootKind::Reference
        );
    }

    #[test]
    fn executes_authorized_classic_and_rejects_a_later_parse_failure_independently() {
        let mut host = BlueJsChildHost::default();
        let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(
                1,
                vec![
                    classic(0, "globalThis.answer = 42;"),
                    classic(1, "const = ;"),
                ],
            ),
        });
        let PageHostReply::Synchronized { reports, .. } = reply else {
            panic!("expected synchronized reply");
        };
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].outcome, PageHostScriptOutcome::Executed);
        assert!(matches!(
            reports[1].outcome,
            PageHostScriptOutcome::Rejected { .. }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::GetRealmStats {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::RealmStats(PageHostRealmStats {
                program_count: 1,
                ..
            })
        ));
    }

    #[test]
    fn private_debugger_locations_are_generation_and_tab_bound_without_runtime_leaks() {
        let mut host = BlueJsChildHost::default();
        let first = document(1, vec![classic(0, "let answer = 42;")]);
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument { document: first }),
            PageHostReply::Synchronized { .. }
        ));
        let programs = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
            tab_id: 7,
            document_generation: 1,
        }) {
            PageHostReply::DebuggerPrograms { programs, .. } => programs,
            reply => panic!("expected source-free child debugger programs, got {reply:?}"),
        };
        assert_eq!(programs.len(), 1);
        let program = programs[0];
        let safe_points = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
            tab_id: 7,
            document_generation: 1,
            program,
        }) {
            PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points,
            reply => panic!("expected source-free child debugger safe points, got {reply:?}"),
        };
        let safe_point = *safe_points
            .first()
            .expect("a retained classic program has a root safe point");
        assert_eq!(
            host.handle_request(PageHostRequest::ValidateDebuggerSafePoint {
                tab_id: 7,
                document_generation: 1,
                safe_point,
            }),
            PageHostReply::DebuggerSafePointValidated {
                tab_id: 7,
                document_generation: 1,
                safe_point,
            }
        );
        assert_eq!(
            host.handle_request(PageHostRequest::SetDebuggerBreakpoint {
                tab_id: 7,
                document_generation: 1,
                safe_point,
            }),
            PageHostReply::DebuggerBreakpointSet {
                tab_id: 7,
                document_generation: 1,
                safe_point,
            }
        );
        // Retries are idempotent and cannot consume a second bounded record.
        assert_eq!(
            host.handle_request(PageHostRequest::SetDebuggerBreakpoint {
                tab_id: 7,
                document_generation: 1,
                safe_point,
            }),
            PageHostReply::DebuggerBreakpointSet {
                tab_id: 7,
                document_generation: 1,
                safe_point,
            }
        );
        assert_eq!(
            host.handle_request(PageHostRequest::ListDebuggerBreakpoints {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::DebuggerBreakpoints {
                tab_id: 7,
                document_generation: 1,
                safe_points: vec![safe_point],
            }
        );
        assert_eq!(
            host.handle_request(PageHostRequest::ClearDebuggerBreakpoint {
                tab_id: 7,
                document_generation: 1,
                safe_point,
            }),
            PageHostReply::DebuggerBreakpointCleared {
                tab_id: 7,
                document_generation: 1,
                safe_point,
                was_present: true,
            }
        );
        assert_eq!(
            host.handle_request(PageHostRequest::ClearDebuggerBreakpoint {
                tab_id: 7,
                document_generation: 1,
                safe_point,
            }),
            PageHostReply::DebuggerBreakpointCleared {
                tab_id: 7,
                document_generation: 1,
                safe_point,
                was_present: false,
            }
        );

        let mut other = document(1, vec![classic(0, "let other = 7;")]);
        other.tab_id = 9;
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument { document: other }),
            PageHostReply::Synchronized { .. }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::ListDebuggerSafePoints {
                tab_id: 9,
                document_generation: 1,
                program,
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::InvalidRequest,
                ..
            }
        ));

        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: document(2, vec![classic(0, "let successor = 1;")]),
            }),
            PageHostReply::Synchronized { .. }
        ));
        assert_eq!(
            host.handle_request(PageHostRequest::ListDebuggerBreakpoints {
                tab_id: 7,
                document_generation: 2,
            }),
            PageHostReply::DebuggerBreakpoints {
                tab_id: 7,
                document_generation: 2,
                safe_points: Vec::new(),
            }
        );
        assert!(matches!(
            host.handle_request(PageHostRequest::ListDebuggerPrograms {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::StaleDocument,
                ..
            }
        ));
        let reply = format!(
            "{:?}",
            host.handle_request(PageHostRequest::ListDebuggerSafePoints {
                tab_id: 7,
                document_generation: 2,
                program,
            })
        );
        assert!(
            !reply.contains("answer") && !reply.contains("bytecode") && !reply.contains("Value"),
            "private debugger errors must remain source/value-free"
        );
    }

    #[test]
    fn root_classic_breakpoint_pauses_and_resumes_without_vm_disclosure() {
        let mut host = BlueJsChildHost::default();
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: debugger_document(
                    1,
                    vec![classic(0, "let first = 1; first += 1; globalThis.answer = first;")],
                ),
            }),
            PageHostReply::Synchronized { reports, .. } if reports.is_empty()
        ));
        let program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
            tab_id: 7,
            document_generation: 1,
        }) {
            PageHostReply::DebuggerPrograms { programs, .. } => programs[0],
            reply => panic!("expected private classic program, got {reply:?}"),
        };
        let target = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
            tab_id: 7,
            document_generation: 1,
            program,
        }) {
            PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points
                .into_iter()
                .find(|safe_point| {
                    safe_point.code_unit_ordinal == 0 && safe_point.bytecode_offset != 0
                })
                .expect("fixture must have a non-entry root safe point"),
            reply => panic!("expected source-free safe points, got {reply:?}"),
        };
        assert_eq!(
            host.handle_request(PageHostRequest::GetDebuggerExecutionState {
                tab_id: 7,
                document_generation: 1,
                program,
            }),
            PageHostReply::DebuggerExecutionState {
                tab_id: 7,
                document_generation: 1,
                program,
                state: PageHostDebuggerExecutionState::Pending,
            }
        );
        assert!(matches!(
            host.handle_request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
                tab_id: 7,
                document_generation: 1,
                safe_point: target,
            }),
            PageHostReply::DebuggerRootSafePointBreakpointArmed { safe_point, .. }
                if safe_point == target
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
        ));
        assert_eq!(
            host.handle_request(PageHostRequest::GetDebuggerExecutionState {
                tab_id: 7,
                document_generation: 1,
                program,
            }),
            PageHostReply::DebuggerExecutionState {
                tab_id: 7,
                document_generation: 1,
                program,
                state: PageHostDebuggerExecutionState::Paused { safe_point: target },
            }
        );
        assert_eq!(
            host.handle_request(PageHostRequest::StepDebuggerRootInstruction {
                tab_id: 7,
                document_generation: 1,
                program,
            }),
            PageHostReply::DebuggerExecutionStepRequested {
                tab_id: 7,
                document_generation: 1,
                program,
            }
        );
        assert_eq!(
            host.handle_request(PageHostRequest::GetDebuggerExecutionState {
                tab_id: 7,
                document_generation: 1,
                program,
            }),
            PageHostReply::DebuggerExecutionState {
                tab_id: 7,
                document_generation: 1,
                program,
                state: PageHostDebuggerExecutionState::Stepping,
            }
        );
        assert!(matches!(
            host.handle_request(PageHostRequest::StepDebuggerRootInstruction {
                tab_id: 7,
                document_generation: 1,
                program,
            }),
            PageHostReply::Error { .. }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
        ));
        let successor = match host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
        }) {
            PageHostReply::DebuggerExecutionState {
                state: PageHostDebuggerExecutionState::Paused { safe_point },
                ..
            } => safe_point,
            reply => panic!("one child root step must pause at its successor: {reply:?}"),
        };
        assert_ne!(successor, target);
        assert_eq!(successor.program, program);
        assert!(matches!(
            host.handle_request(PageHostRequest::ResumeDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
                program,
            }),
            PageHostReply::DebuggerExecutionResumed { .. }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::DebuggerExecutionAdvanced { reports, .. }
                if reports == vec![script_report(
                    7,
                    1,
                    0,
                    PageHostScriptLanguage::JavaScript,
                    PageHostScriptKind::Classic,
                    PageHostScriptOutcome::Executed,
                )]
        ));
        let reply = format!(
            "{:?}",
            host.handle_request(PageHostRequest::GetDebuggerExecutionState {
                tab_id: 7,
                document_generation: 1,
                program,
            })
        );
        assert!(reply.contains("Completed"));
        assert!(
            !reply.contains("answer") && !reply.contains("Value") && !reply.contains("Vm"),
            "execution state remains source/value/VM-free"
        );
    }

    #[test]
    fn root_classic_steps_revisit_loop_boundaries_without_releasing_the_queue() {
        let mut host = BlueJsChildHost::default();
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: debugger_document(
                    1,
                    vec![classic(
                        0,
                        "let index = 0; while (index < 2) { index++; } globalThis.done = index;",
                    )],
                ),
            }),
            PageHostReply::Synchronized { reports, .. } if reports.is_empty()
        ));
        let program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
            tab_id: 7,
            document_generation: 1,
        }) {
            PageHostReply::DebuggerPrograms { programs, .. } => programs[0],
            reply => panic!("expected queued classic program: {reply:?}"),
        };
        let safe_points = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
            tab_id: 7,
            document_generation: 1,
            program,
        }) {
            PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points,
            reply => panic!("expected verified root safe points: {reply:?}"),
        };
        let target = *safe_points
            .iter()
            .find(|point| point.code_unit_ordinal == 0 && point.bytecode_offset != 0)
            .unwrap();
        assert!(matches!(
            host.handle_request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
                tab_id: 7,
                document_generation: 1,
                safe_point: target,
            }),
            PageHostReply::DebuggerRootSafePointBreakpointArmed { .. }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
        ));
        let mut seen_offsets = Vec::new();
        let mut completion_reports = None;
        for _ in 0..256 {
            assert!(matches!(
                host.handle_request(PageHostRequest::StepDebuggerRootInstruction {
                    tab_id: 7,
                    document_generation: 1,
                    program,
                }),
                PageHostReply::DebuggerExecutionStepRequested { .. }
            ));
            assert!(matches!(
                host.handle_request(PageHostRequest::GetDebuggerExecutionState {
                    tab_id: 7,
                    document_generation: 1,
                    program,
                }),
                PageHostReply::DebuggerExecutionState {
                    state: PageHostDebuggerExecutionState::Stepping,
                    ..
                }
            ));
            let reports = match host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
            }) {
                PageHostReply::DebuggerExecutionAdvanced { reports, .. } => reports,
                reply => panic!("expected one bounded child advance: {reply:?}"),
            };
            match host.handle_request(PageHostRequest::GetDebuggerExecutionState {
                tab_id: 7,
                document_generation: 1,
                program,
            }) {
                PageHostReply::DebuggerExecutionState {
                    state: PageHostDebuggerExecutionState::Paused { safe_point },
                    ..
                } => {
                    assert!(safe_points.contains(&safe_point));
                    seen_offsets.push(safe_point.bytecode_offset);
                    assert!(reports.is_empty());
                }
                PageHostReply::DebuggerExecutionState {
                    state: PageHostDebuggerExecutionState::Completed,
                    ..
                } => {
                    completion_reports = Some(reports);
                    break;
                }
                reply => panic!("unexpected child step state: {reply:?}"),
            }
        }
        assert_eq!(
            completion_reports,
            Some(vec![script_report(
                7,
                1,
                0,
                PageHostScriptLanguage::JavaScript,
                PageHostScriptKind::Classic,
                PageHostScriptOutcome::Executed,
            )])
        );
        assert!(
            seen_offsets
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                < seen_offsets.len(),
            "the loop must revisit a real root boundary"
        );
    }

    #[test]
    fn private_debugger_breakpoint_configuration_is_idempotent_and_bounded() {
        let mut host = BlueJsChildHost::default();
        let source = (0..=PAGE_HOST_DEBUGGER_MAX_BREAKPOINTS_PER_REALM)
            .map(|index| format!("let breakpoint_{index} = {index};"))
            .collect::<String>();
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: document(1, vec![classic(0, &source)]),
            }),
            PageHostReply::Synchronized { .. }
        ));
        let program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
            tab_id: 7,
            document_generation: 1,
        }) {
            PageHostReply::DebuggerPrograms { programs, .. } => programs[0],
            reply => panic!("expected child debugger program, got {reply:?}"),
        };
        let safe_points = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
            tab_id: 7,
            document_generation: 1,
            program,
        }) {
            PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points,
            reply => panic!("expected child debugger safe points, got {reply:?}"),
        };
        let max = usize::try_from(PAGE_HOST_DEBUGGER_MAX_BREAKPOINTS_PER_REALM)
            .expect("page-host breakpoint cap fits usize");
        assert!(
            safe_points.len() > max,
            "fixture needs one point over the cap"
        );
        for safe_point in safe_points.iter().copied().take(max) {
            assert!(matches!(
                host.handle_request(PageHostRequest::SetDebuggerBreakpoint {
                    tab_id: 7,
                    document_generation: 1,
                    safe_point,
                }),
                PageHostReply::DebuggerBreakpointSet { .. }
            ));
        }
        let overflow = safe_points[max];
        assert!(matches!(
            host.handle_request(PageHostRequest::SetDebuggerBreakpoint {
                tab_id: 7,
                document_generation: 1,
                safe_point: overflow,
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::ResourceLimit,
                ..
            }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::ListDebuggerBreakpoints {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::DebuggerBreakpoints { safe_points, .. } if safe_points.len() == max
        ));
    }

    #[test]
    fn direct_bluets_and_javascript_execute_in_document_order_in_one_realm() {
        let mut host = BlueJsChildHost::default();
        let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(
                1,
                vec![
                    classic(0, "globalThis.beforeBlueTs = true;"),
                    // The TypeScript annotation makes this invalid JavaScript
                    // source. Success therefore proves the child used direct
                    // BlueTS lowering rather than emitted-JavaScript reparse.
                    blue_ts_classic(1, "const sharedAnswer: number = 42;"),
                    classic(
                        2,
                        "if (!globalThis.beforeBlueTs || sharedAnswer !== 42) throw 'realm/order failed';",
                    ),
                ],
            ),
        });
        let PageHostReply::Synchronized { reports, .. } = reply else {
            panic!("expected synchronized reply");
        };
        assert_eq!(
            reports
                .iter()
                .map(|report| (report.ordinal, report.language, &report.outcome))
                .collect::<Vec<_>>(),
            vec![
                (
                    0,
                    PageHostScriptLanguage::JavaScript,
                    &PageHostScriptOutcome::Executed
                ),
                (
                    1,
                    PageHostScriptLanguage::BlueTs,
                    &PageHostScriptOutcome::Executed
                ),
                (
                    2,
                    PageHostScriptLanguage::JavaScript,
                    &PageHostScriptOutcome::Executed
                ),
            ]
        );
        assert!(matches!(
            host.handle_request(PageHostRequest::GetRealmStats {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::RealmStats(PageHostRealmStats {
                program_count: 3,
                ..
            })
        ));
    }

    #[test]
    fn child_bluets_debug_metadata_is_bound_to_its_live_realm_generation() {
        let mut host = BlueJsChildHost::default();
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: document(1, vec![blue_ts_classic(0, "const answer: number = 42;")]),
            }),
            PageHostReply::Synchronized { reports, .. }
                if reports == vec![script_report(
                    7,
                    1,
                    0,
                    PageHostScriptLanguage::BlueTs,
                    PageHostScriptKind::Classic,
                    PageHostScriptOutcome::Executed,
                )]
        ));
        assert_eq!(host.debug_registry.len(), 1);
        let first_handle = host
            .documents
            .get(&7)
            .expect("the first child realm remains live")
            .debugger_programs
            .values()
            .map(|record| record.runtime_handle)
            .find(|handle| {
                host.debug_registry
                    .get(host.runtime.program_registry(), *handle)
                    .is_ok()
            })
            .expect("the BlueTS program retains static metadata");
        let first_static_info = host
            .debug_registry
            .get(host.runtime.program_registry(), first_handle)
            .expect("the exact live generation resolves its metadata")
            .static_info();
        assert!(first_static_info
            .sources
            .iter()
            .any(|source| source.module == "blueice://page/inline-0.ts"));
        assert!(!first_static_info.types.is_empty());

        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: document(
                    2,
                    vec![blue_ts_classic(0, "const answer: string = 'next';")]
                ),
            }),
            PageHostReply::Synchronized { .. }
        ));
        assert_eq!(host.debug_registry.len(), 1);
        assert!(host
            .debug_registry
            .get(host.runtime.program_registry(), first_handle)
            .is_err());

        assert!(matches!(
            host.handle_request(PageHostRequest::CloseRealm {
                tab_id: 7,
                document_generation: 2,
            }),
            PageHostReply::RealmClosed {
                tab_id: 7,
                document_generation: 2,
            }
        ));
        assert!(host.debug_registry.is_empty());
    }

    #[test]
    fn child_bluets_metadata_inventory_mints_only_opaque_live_attachment_handles() {
        let mut host = BlueJsChildHost::default();
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: document(
                    1,
                    vec![
                        classic(0, "globalThis.javaScriptOnly = true;"),
                        blue_ts_classic(1, "const typedAnswer: number = 42;"),
                    ],
                ),
            }),
            PageHostReply::Synchronized { reports, .. }
                if reports.iter().all(|report| report.outcome == PageHostScriptOutcome::Executed)
        ));
        let programs = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
            tab_id: 7,
            document_generation: 1,
        }) {
            PageHostReply::DebuggerPrograms { programs, .. } => programs,
            reply => panic!("expected private program inventory, got {reply:?}"),
        };
        assert_eq!(programs.len(), 2);

        let mut blue_ts = None;
        for program in programs {
            let reply = host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
                tab_id: 7,
                document_generation: 1,
                program,
            });
            let PageHostReply::DebuggerBlueTsMetadata { metadata, .. } = &reply else {
                panic!("expected private BlueTS metadata inventory, got {reply:?}");
            };
            // The child exposes no source/module/name/type/span/contract data:
            // only the bounded list's length and its opaque values are visible.
            assert!(!format!("{reply:?}").contains("typedAnswer"));
            assert!(!format!("{reply:?}").contains("inline-1.ts"));
            assert!(!format!("{reply:?}").contains("number"));
            if let [metadata] = metadata.as_slice() {
                blue_ts = Some((program, *metadata));
            } else {
                assert!(
                    metadata.is_empty(),
                    "only the JavaScript program is ineligible"
                );
            }
        }
        let (typed_program, first_metadata) = blue_ts.expect("the live BlueTS attachment exists");
        assert!(first_metadata.is_well_formed());
        assert_ne!(
            first_metadata.metadata_handle, typed_program.program_handle,
            "metadata IDs must not reuse the child program namespace"
        );
        assert!(
            first_metadata.metadata_handle >= CHILD_DEBUGGER_METADATA_ID_NAMESPACE_START,
            "metadata IDs have a child-private namespace separate from program IDs"
        );
        let summary_reply = host.handle_request(PageHostRequest::DescribeDebuggerBlueTsMetadata {
            tab_id: 7,
            document_generation: 1,
            program: typed_program,
            metadata: first_metadata,
        });
        let PageHostReply::DebuggerBlueTsMetadataSummary {
            metadata, summary, ..
        } = summary_reply
        else {
            panic!("expected bounded private BlueTS metadata summary")
        };
        assert_eq!(metadata, first_metadata);
        assert_eq!(summary.language_version, "blue-ts-0.1");
        assert!(summary.source_count > 0);
        assert!(summary.type_count > 0);
        assert!(summary.symbol_count > 0);
        // The summary intentionally reveals no source/module/name/type/span/
        // contract record. A separate future capability would be needed for
        // every individual record family.
        assert!(!format!("{summary:?}").contains("typedAnswer"));
        assert!(!format!("{summary:?}").contains("inline-1.ts"));
        assert!(!format!("{summary:?}").contains("number"));
        let source_inventory_reply =
            host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadataSources {
                tab_id: 7,
                document_generation: 1,
                program: typed_program,
                metadata: first_metadata,
            });
        let PageHostReply::DebuggerBlueTsMetadataSources {
            program,
            metadata,
            sources,
            ..
        } = source_inventory_reply
        else {
            panic!("expected bounded private BlueTS source-record identity inventory")
        };
        assert_eq!(program, typed_program);
        assert_eq!(metadata, first_metadata);
        assert_eq!(
            sources.len(),
            usize::try_from(summary.source_count).unwrap()
        );
        assert_eq!(
            sources
                .iter()
                .map(|source| source.source_id)
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            sources.len(),
            "one metadata attachment cannot repeat a compiler source-record ID"
        );
        // The opaque inventory conveys source-record cardinality and IDs only;
        // no module, source, hash, span, or compiler-record field crosses it.
        assert!(!format!("{sources:?}").contains("typedAnswer"));
        assert!(!format!("{sources:?}").contains("inline-1.ts"));
        assert!(!format!("{sources:?}").contains("number"));
        let type_inventory_reply =
            host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadataTypes {
                tab_id: 7,
                document_generation: 1,
                program: typed_program,
                metadata: first_metadata,
            });
        let PageHostReply::DebuggerBlueTsMetadataTypes {
            program,
            metadata,
            types,
            ..
        } = type_inventory_reply
        else {
            panic!("expected bounded private BlueTS type-record identity inventory")
        };
        assert_eq!(program, typed_program);
        assert_eq!(metadata, first_metadata);
        assert_eq!(types.len(), usize::try_from(summary.type_count).unwrap());
        assert_eq!(
            types
                .iter()
                .map(|static_type| static_type.type_id)
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            types.len(),
            "one metadata attachment cannot repeat a compiler type-record ID"
        );
        assert!(!format!("{types:?}").contains("typedAnswer"));
        assert!(!format!("{types:?}").contains("inline-1.ts"));
        assert!(!format!("{types:?}").contains("number"));
        assert!(matches!(
            host.handle_request(PageHostRequest::DescribeDebuggerBlueTsMetadata {
                tab_id: 7,
                document_generation: 1,
                program: typed_program,
                metadata: PageHostDebuggerMetadataHandle {
                    metadata_handle: first_metadata.metadata_handle,
                    metadata_generation: first_metadata.metadata_generation + 1,
                },
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::InvalidRequest,
                ..
            }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadataSources {
                tab_id: 7,
                document_generation: 1,
                program: typed_program,
                metadata: PageHostDebuggerMetadataHandle {
                    metadata_handle: first_metadata.metadata_handle,
                    metadata_generation: first_metadata.metadata_generation + 1,
                },
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::InvalidRequest,
                ..
            }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadataTypes {
                tab_id: 7,
                document_generation: 1,
                program: typed_program,
                metadata: PageHostDebuggerMetadataHandle {
                    metadata_handle: first_metadata.metadata_handle,
                    metadata_generation: first_metadata.metadata_generation + 1,
                },
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::InvalidRequest,
                ..
            }
        ));

        assert!(matches!(
            host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
                tab_id: 7,
                document_generation: 1,
                program: typed_program,
            }),
            PageHostReply::DebuggerBlueTsMetadata { metadata, .. }
                if metadata == vec![first_metadata]
        ));

        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: document(
                    2,
                    vec![blue_ts_classic(0, "const replacement: string = 'next';")]
                ),
            }),
            PageHostReply::Synchronized { .. }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
                tab_id: 7,
                document_generation: 1,
                program: typed_program,
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::StaleDocument,
                ..
            }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::DescribeDebuggerBlueTsMetadata {
                tab_id: 7,
                document_generation: 1,
                program: typed_program,
                metadata: first_metadata,
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::StaleDocument,
                ..
            }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadataSources {
                tab_id: 7,
                document_generation: 1,
                program: typed_program,
                metadata: first_metadata,
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::StaleDocument,
                ..
            }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadataTypes {
                tab_id: 7,
                document_generation: 1,
                program: typed_program,
                metadata: first_metadata,
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::StaleDocument,
                ..
            }
        ));
        let replacement_program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
            tab_id: 7,
            document_generation: 2,
        }) {
            PageHostReply::DebuggerPrograms { programs, .. } => *programs
                .first()
                .expect("the replacement BlueTS program remains live"),
            reply => panic!("expected replacement private program inventory, got {reply:?}"),
        };
        let replacement_metadata =
            match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
                tab_id: 7,
                document_generation: 2,
                program: replacement_program,
            }) {
                PageHostReply::DebuggerBlueTsMetadata { metadata, .. } => *metadata
                    .first()
                    .expect("the replacement live BlueTS attachment remains eligible"),
                reply => panic!("expected replacement metadata inventory, got {reply:?}"),
            };
        assert_ne!(replacement_metadata, first_metadata);

        assert!(matches!(
            host.handle_request(PageHostRequest::CloseRealm {
                tab_id: 7,
                document_generation: 2,
            }),
            PageHostReply::RealmClosed { .. }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
                tab_id: 7,
                document_generation: 2,
                program: replacement_program,
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::UnknownRealm,
                ..
            }
        ));
    }

    #[test]
    fn child_bluets_symbol_contract_verifies_only_one_live_reifiable_pair() {
        let mut host = BlueJsChildHost::default();
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: document(
                    1,
                    vec![blue_ts_classic(
                        0,
                        "interface PrivateContract { enabled: boolean; } const typedAnswer: number = 42;",
                    )],
                ),
            }),
            PageHostReply::Synchronized { reports, .. }
                if reports.iter().all(|report| report.outcome == PageHostScriptOutcome::Executed)
        ));
        let program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
            tab_id: 7,
            document_generation: 1,
        }) {
            PageHostReply::DebuggerPrograms { programs, .. } => programs[0],
            reply => panic!("expected live child program, got {reply:?}"),
        };
        let metadata = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
            tab_id: 7,
            document_generation: 1,
            program,
        }) {
            PageHostReply::DebuggerBlueTsMetadata { metadata, .. } => metadata[0],
            reply => panic!("expected live child metadata, got {reply:?}"),
        };
        let symbols =
            match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadataSymbols {
                tab_id: 7,
                document_generation: 1,
                program,
                metadata,
            }) {
                PageHostReply::DebuggerBlueTsMetadataSymbols { symbols, .. } => symbols,
                reply => panic!("expected child symbol IDs, got {reply:?}"),
            };
        let contracts =
            match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadataContracts {
                tab_id: 7,
                document_generation: 1,
                program,
                metadata,
            }) {
                PageHostReply::DebuggerBlueTsMetadataContracts { contracts, .. } => contracts,
                reply => panic!("expected child contract IDs, got {reply:?}"),
            };
        assert!(!symbols.is_empty() && !contracts.is_empty());
        let mut verified = None;
        for symbol in &symbols {
            for contract in &contracts {
                let reply = host.handle_request(
                    PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolContract {
                        tab_id: 7,
                        document_generation: 1,
                        program,
                        metadata,
                        symbol_id: symbol.symbol_id,
                        contract_id: contract.contract_id,
                    },
                );
                match reply {
                    PageHostReply::DebuggerBlueTsMetadataSymbolContract {
                        symbol_contract, ..
                    } => {
                        assert_eq!(symbol_contract.symbol_id, symbol.symbol_id);
                        assert_eq!(symbol_contract.contract_id, contract.contract_id);
                        verified = Some(symbol_contract);
                        break;
                    }
                    PageHostReply::Error {
                        code: PageHostErrorCode::InvalidRequest,
                        ..
                    } => {}
                    reply => panic!("unexpected private relation reply: {reply:?}"),
                }
            }
            if verified.is_some() {
                break;
            }
        }
        let verified = verified.expect("the interface has one reifiable contract relation");
        assert!(!format!("{verified:?}").contains("PrivateContract"));
        assert!(!format!("{verified:?}").contains("enabled"));
        let sources =
            match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadataSources {
                tab_id: 7,
                document_generation: 1,
                program,
                metadata,
            }) {
                PageHostReply::DebuggerBlueTsMetadataSources { sources, .. } => sources,
                reply => panic!("expected child source IDs, got {reply:?}"),
            };
        let location = match host.handle_request(
            PageHostRequest::DescribeDebuggerBlueTsMetadataContractLocation {
                tab_id: 7,
                document_generation: 1,
                program,
                metadata,
                contract_id: verified.contract_id,
            },
        ) {
            PageHostReply::DebuggerBlueTsMetadataContractLocation { location, .. } => location,
            reply => panic!("expected child contract location, got {reply:?}"),
        };
        assert_eq!(location.contract_id, verified.contract_id);
        assert!(sources
            .iter()
            .any(|source| source.source_id == location.source_id));
        assert!(location.start_byte < location.end_byte);
        assert!(location.end_byte <= DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES);
        assert_eq!(location.coordinates.start_line, 0);
        assert_eq!(location.coordinates.end_line, 0);
        assert!(!format!("{location:?}").contains("PrivateContract"));
        assert!(!format!("{location:?}").contains("enabled"));
        assert!(matches!(
            host.handle_request(
                PageHostRequest::DescribeDebuggerBlueTsMetadataContractLocation {
                    tab_id: 7,
                    document_generation: 1,
                    program,
                    metadata,
                    contract_id: u32::MAX,
                }
            ),
            PageHostReply::Error {
                code: PageHostErrorCode::InvalidRequest,
                ..
            }
        ));
        assert!(matches!(
            host.handle_request(
                PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolContract {
                    tab_id: 7,
                    document_generation: 1,
                    program,
                    metadata,
                    symbol_id: verified.symbol_id,
                    contract_id: u32::MAX,
                }
            ),
            PageHostReply::Error {
                code: PageHostErrorCode::InvalidRequest,
                ..
            }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: document(2, vec![blue_ts_classic(0, "const successor: number = 1;")]),
            }),
            PageHostReply::Synchronized { .. }
        ));
        assert!(matches!(
            host.handle_request(
                PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolContract {
                    tab_id: 7,
                    document_generation: 1,
                    program,
                    metadata,
                    symbol_id: verified.symbol_id,
                    contract_id: verified.contract_id,
                }
            ),
            PageHostReply::Error {
                code: PageHostErrorCode::StaleDocument,
                ..
            }
        ));
        assert!(matches!(
            host.handle_request(
                PageHostRequest::DescribeDebuggerBlueTsMetadataContractLocation {
                    tab_id: 7,
                    document_generation: 1,
                    program,
                    metadata,
                    contract_id: verified.contract_id,
                }
            ),
            PageHostReply::Error {
                code: PageHostErrorCode::StaleDocument,
                ..
            }
        ));
    }

    #[test]
    fn child_bluets_symbol_location_is_live_bound_and_source_text_free() {
        let mut host = BlueJsChildHost::default();
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: document(1, vec![blue_ts_classic(0, "const typedAnswer: number = 42;")]),
            }),
            PageHostReply::Synchronized { reports, .. }
                if reports.iter().all(|report| report.outcome == PageHostScriptOutcome::Executed)
        ));
        let program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
            tab_id: 7,
            document_generation: 1,
        }) {
            PageHostReply::DebuggerPrograms { programs, .. } => *programs
                .first()
                .expect("the one BlueTS program remains live"),
            reply => panic!("expected private debugger program inventory, got {reply:?}"),
        };
        let metadata = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
            tab_id: 7,
            document_generation: 1,
            program,
        }) {
            PageHostReply::DebuggerBlueTsMetadata { metadata, .. } => *metadata
                .first()
                .expect("the BlueTS program retains one static attachment"),
            reply => panic!("expected private BlueTS metadata inventory, got {reply:?}"),
        };
        let sources =
            match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadataSources {
                tab_id: 7,
                document_generation: 1,
                program,
                metadata,
            }) {
                PageHostReply::DebuggerBlueTsMetadataSources { sources, .. } => sources,
                reply => panic!("expected private source-ID inventory, got {reply:?}"),
            };
        let symbols =
            match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadataSymbols {
                tab_id: 7,
                document_generation: 1,
                program,
                metadata,
            }) {
                PageHostReply::DebuggerBlueTsMetadataSymbols { symbols, .. } => symbols,
                reply => panic!("expected private symbol-ID inventory, got {reply:?}"),
            };
        let symbol = symbols
            .into_iter()
            .find(|candidate| {
                matches!(
                    host.handle_request(PageHostRequest::DescribeDebuggerBlueTsMetadataSymbol {
                        tab_id: 7,
                        document_generation: 1,
                        program,
                        metadata,
                        symbol_id: candidate.symbol_id,
                    }),
                    PageHostReply::DebuggerBlueTsMetadataSymbol { symbol, .. }
                        if symbol.display == "typedAnswer"
                )
            })
            .expect("the page declaration must retain its own symbol ID");
        let reply = host.handle_request(
            PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolLocation {
                tab_id: 7,
                document_generation: 1,
                program,
                metadata,
                symbol_id: symbol.symbol_id,
            },
        );
        let PageHostReply::DebuggerBlueTsMetadataSymbolLocation { location, .. } = reply else {
            panic!("expected bounded private symbol location")
        };
        assert_eq!(location.symbol_id, symbol.symbol_id);
        assert!(sources
            .iter()
            .any(|source| source.source_id == location.source_id));
        assert!(location.start_byte < location.end_byte);
        assert!(location.end_byte <= DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES);
        assert_eq!(location.coordinates.start_line, 0);
        assert_eq!(location.coordinates.end_line, 0);
        // The result is deliberately only ID/range/coordinate structure, even inside the
        // private bridge: no source text, module identity, name, or type leaks.
        assert!(!format!("{location:?}").contains("typedAnswer"));
        assert!(!format!("{location:?}").contains("inline-0.ts"));
        assert!(!format!("{location:?}").contains("number"));
        let types = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadataTypes {
            tab_id: 7,
            document_generation: 1,
            program,
            metadata,
        }) {
            PageHostReply::DebuggerBlueTsMetadataTypes { types, .. } => types,
            reply => panic!("expected private type-ID inventory, got {reply:?}"),
        };
        assert!(!types.is_empty());
        let matching = types
            .iter()
            .filter_map(|static_type| {
                let reply = host.handle_request(
                    PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolType {
                        tab_id: 7,
                        document_generation: 1,
                        program,
                        metadata,
                        symbol_id: symbol.symbol_id,
                        type_id: static_type.type_id,
                    },
                );
                match reply {
                    PageHostReply::DebuggerBlueTsMetadataSymbolType { symbol_type, .. } => {
                        Some(symbol_type)
                    }
                    _ => None,
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(matching.len(), 1);
        assert_eq!(matching[0].symbol_id, symbol.symbol_id);
        assert!(types
            .iter()
            .any(|static_type| static_type.type_id == matching[0].type_id));
        assert!(!format!("{:?}", matching[0]).contains("typedAnswer"));
        assert!(!format!("{:?}", matching[0]).contains("number"));
        assert!(matches!(
            host.handle_request(PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolType {
                tab_id: 7,
                document_generation: 1,
                program,
                metadata,
                symbol_id: symbol.symbol_id,
                type_id: u32::MAX,
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::InvalidRequest,
                ..
            }
        ));
        assert!(matches!(
            host.handle_request(
                PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolLocation {
                    tab_id: 7,
                    document_generation: 1,
                    program,
                    metadata,
                    symbol_id: u32::MAX,
                }
            ),
            PageHostReply::Error {
                code: PageHostErrorCode::InvalidRequest,
                ..
            }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: document(
                    2,
                    vec![blue_ts_classic(0, "const replacement: number = 1;")]
                ),
            }),
            PageHostReply::Synchronized { .. }
        ));
        assert!(matches!(
            host.handle_request(
                PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolLocation {
                    tab_id: 7,
                    document_generation: 1,
                    program,
                    metadata,
                    symbol_id: symbol.symbol_id,
                }
            ),
            PageHostReply::Error {
                code: PageHostErrorCode::StaleDocument,
                ..
            }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolType {
                tab_id: 7,
                document_generation: 1,
                program,
                metadata,
                symbol_id: symbol.symbol_id,
                type_id: matching[0].type_id,
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::StaleDocument,
                ..
            }
        ));
    }

    #[test]
    fn child_bluets_safe_point_span_requires_an_exact_live_metadata_attachment() {
        let mut host = BlueJsChildHost::default();
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: document(
                    1,
                    vec![
                        blue_ts_classic(0, "const first: number = 1;"),
                        blue_ts_classic(1, "const second: number = 2;"),
                    ],
                ),
            }),
            PageHostReply::Synchronized { reports, .. }
                if reports.iter().all(|report| report.outcome == PageHostScriptOutcome::Executed)
        ));
        let programs = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
            tab_id: 7,
            document_generation: 1,
        }) {
            PageHostReply::DebuggerPrograms { programs, .. } => programs,
            reply => panic!("expected two private BlueTS programs, got {reply:?}"),
        };
        assert_eq!(programs.len(), 2);
        let program = programs[0];
        let other_program = programs[1];
        let metadata = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
            tab_id: 7,
            document_generation: 1,
            program,
        }) {
            PageHostReply::DebuggerBlueTsMetadata { metadata, .. } => metadata[0],
            reply => panic!("expected exact private metadata handle, got {reply:?}"),
        };
        let entry = {
            let handle =
                host.documents[&7].debugger_programs[&program.program_handle].runtime_handle;
            host.debug_registry
                .get(host.runtime.program_registry(), handle)
                .unwrap()
                .safe_point_map()
                .entries[0]
                .clone()
        };
        let safe_point = PageHostDebuggerSafePoint {
            program,
            code_unit_ordinal: entry.code_unit.ordinal(),
            bytecode_offset: entry.bytecode_offset,
        };
        let request = PageHostRequest::DescribeDebuggerBlueTsSafePointSpan {
            tab_id: 7,
            document_generation: 1,
            metadata,
            safe_point,
        };
        let PageHostReply::DebuggerBlueTsSafePointSpan {
            span,
            safe_point: echoed,
            ..
        } = host.handle_request(request.clone())
        else {
            panic!("an exact retained safe point must have its original BlueTS span")
        };
        assert_eq!(echoed, safe_point);
        assert_eq!(
            (span.start_byte, span.end_byte),
            (
                u32::try_from(entry.start_byte).unwrap(),
                u32::try_from(entry.end_byte).unwrap()
            )
        );
        assert!(span.start_byte < span.end_byte);
        assert!(span.end_byte <= DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES);
        assert!(!format!("{span:?}").contains("const first"));
        assert!(!format!("{span:?}").contains("inline-0.ts"));

        for (metadata, safe_point) in [
            (
                metadata,
                PageHostDebuggerSafePoint {
                    program: other_program,
                    ..safe_point
                },
            ),
            (
                PageHostDebuggerMetadataHandle {
                    metadata_generation: metadata.metadata_generation + 1,
                    ..metadata
                },
                safe_point,
            ),
            (
                metadata,
                PageHostDebuggerSafePoint {
                    bytecode_offset: u32::MAX,
                    ..safe_point
                },
            ),
        ] {
            assert!(matches!(
                host.handle_request(PageHostRequest::DescribeDebuggerBlueTsSafePointSpan {
                    tab_id: 7,
                    document_generation: 1,
                    metadata,
                    safe_point,
                }),
                PageHostReply::Error {
                    code: PageHostErrorCode::InvalidRequest,
                    ..
                }
            ));
        }
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: document(2, vec![blue_ts_classic(0, "const successor = 3;")]),
            }),
            PageHostReply::Synchronized { .. }
        ));
        assert!(matches!(
            host.handle_request(request),
            PageHostReply::Error {
                code: PageHostErrorCode::StaleDocument,
                ..
            }
        ));
    }

    #[test]
    fn child_bluets_source_breakpoint_is_bound_to_a_live_source_and_generation() {
        let mut host = BlueJsChildHost::default();
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: document(
                    1,
                    vec![
                        blue_ts_classic(
                            0,
                            "const first: number = 1; const second: number = 2; second;"
                        ),
                        blue_ts_classic(1, "const other: number = 3;"),
                    ],
                ),
            }),
            PageHostReply::Synchronized { .. }
        ));
        let programs = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
            tab_id: 7,
            document_generation: 1,
        }) {
            PageHostReply::DebuggerPrograms { programs, .. } => programs,
            reply => panic!("expected private BlueTS programs, got {reply:?}"),
        };
        let program = programs[0];
        let metadata = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
            tab_id: 7,
            document_generation: 1,
            program,
        }) {
            PageHostReply::DebuggerBlueTsMetadata { metadata, .. } => metadata[0],
            reply => panic!("expected private BlueTS metadata, got {reply:?}"),
        };
        let (source_id, first, second) = {
            let handle =
                host.documents[&7].debugger_programs[&program.program_handle].runtime_handle;
            let retained = host
                .debug_registry
                .get(host.runtime.program_registry(), handle)
                .unwrap();
            let mut entries = retained.safe_point_map().entries.iter().collect::<Vec<_>>();
            entries.sort_by_key(|entry| entry.start_byte);
            (
                retained
                    .static_info()
                    .sources
                    .iter()
                    .find(|source| source.module == entries[0].source)
                    .expect("the lowered source must have a compiler source ID")
                    .id
                    .0,
                entries[0].clone(),
                entries[1].clone(),
            )
        };
        let request = |source_byte| PageHostRequest::ResolveDebuggerBlueTsSourceBreakpoint {
            tab_id: 7,
            document_generation: 1,
            program,
            metadata,
            source_id,
            source_byte,
        };
        for (source_byte, entry) in [
            (u32::try_from(first.start_byte).unwrap(), &first),
            (u32::try_from(first.end_byte).unwrap(), &second),
        ] {
            assert_eq!(
                host.handle_request(request(source_byte)),
                PageHostReply::DebuggerBlueTsSourceBreakpoint {
                    tab_id: 7,
                    document_generation: 1,
                    program,
                    metadata,
                    source_id,
                    source_byte,
                    safe_point: Some(PageHostDebuggerSafePoint {
                        program,
                        code_unit_ordinal: entry.code_unit.ordinal(),
                        bytecode_offset: entry.bytecode_offset,
                    }),
                }
            );
        }
        assert_eq!(
            host.handle_request(request(u32::try_from(second.end_byte).unwrap() + 10)),
            PageHostReply::DebuggerBlueTsSourceBreakpoint {
                tab_id: 7,
                document_generation: 1,
                program,
                metadata,
                source_id,
                source_byte: u32::try_from(second.end_byte).unwrap() + 10,
                safe_point: None,
            }
        );
        for forged in [
            PageHostRequest::ResolveDebuggerBlueTsSourceBreakpoint {
                tab_id: 7,
                document_generation: 1,
                program: programs[1],
                metadata,
                source_id,
                source_byte: 0,
            },
            PageHostRequest::ResolveDebuggerBlueTsSourceBreakpoint {
                tab_id: 7,
                document_generation: 1,
                program,
                metadata,
                source_id: u32::MAX,
                source_byte: 0,
            },
            PageHostRequest::ResolveDebuggerBlueTsSourceBreakpoint {
                tab_id: 7,
                document_generation: 1,
                program,
                metadata: PageHostDebuggerMetadataHandle {
                    metadata_generation: metadata.metadata_generation + 1,
                    ..metadata
                },
                source_id,
                source_byte: 0,
            },
            request(DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES + 1),
        ] {
            assert!(matches!(
                host.handle_request(forged),
                PageHostReply::Error {
                    code: PageHostErrorCode::InvalidRequest,
                    ..
                }
            ));
        }
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: document(2, vec![blue_ts_classic(0, "const successor = 4;")]),
            }),
            PageHostReply::Synchronized { .. }
        ));
        assert!(matches!(
            host.handle_request(request(0)),
            PageHostReply::Error {
                code: PageHostErrorCode::StaleDocument,
                ..
            }
        ));
    }

    #[test]
    fn child_installs_only_fixed_core_snapshot_callbacks_for_javascript() {
        let mut host = BlueJsChildHost::default();
        let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document_with_snapshot(
                1,
                "private document snapshot".to_string(),
                "https://example.test".to_string(),
                vec![
                    classic(
                        0,
                        concat!(
                            "if (blueiceDocumentText() !== 'private document snapshot') throw 'text';",
                            "if (blueiceDocumentOrigin() !== 'https://example.test') throw 'origin';",
                            "if (typeof document !== 'undefined' || typeof fetch !== 'undefined' || typeof blueiceTestHasElementById !== 'undefined' || typeof blueiceTestGetElementById !== 'undefined') throw 'ambient';"
                        ),
                    ),
                    classic(1, "blueiceDocumentText(1);"),
                ],
            ),
        });
        let PageHostReply::Synchronized { reports, .. } = reply else {
            panic!("expected synchronized reply");
        };
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].outcome, PageHostScriptOutcome::Executed);
        assert!(matches!(
            reports[1].outcome,
            PageHostScriptOutcome::Rejected { .. }
        ));
        // Result records remain source/value-free even when a callback
        // returned a private snapshot inside the child VM.
        assert!(!format!("{:?}", reports).contains("private document snapshot"));
    }

    #[test]
    fn snapshot_validation_happens_before_realm_creation_or_replacement() {
        let mut host = BlueJsChildHost::default();
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: document_with_snapshot(
                    1,
                    "snapshot".to_string(),
                    "HTTP://EXAMPLE.test".to_string(),
                    vec![classic(0, "globalThis.answer = 42;")],
                ),
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::InvalidRequest,
                ..
            }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::GetRealmStats {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::UnknownRealm,
                ..
            }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: document_with_snapshot(
                    1,
                    "x".repeat(PAGE_HOST_DOCUMENT_TEXT_MAX_BYTES + 1),
                    "https://example.test".to_string(),
                    vec![classic(0, "globalThis.answer = 42;")],
                ),
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::ResourceLimit,
                ..
            }
        ));
    }

    #[test]
    fn bluets_uses_the_verified_snapshot_profile_and_rejects_untyped_globals() {
        let mut host = BlueJsChildHost::default();
        let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(
                1,
                vec![
                    // The annotation is invalid JavaScript syntax, so this
                    // success also preserves direct BlueTS-to-BlueJS lowering.
                    blue_ts_classic(
                        0,
                        concat!(
                            "const snapshotText: string = blueiceDocumentText();",
                            "const snapshotOrigin: string = blueiceDocumentOrigin();"
                        ),
                    ),
                    classic(
                        1,
                        concat!(
                            "if (snapshotText !== 'test document snapshot') throw 'text';",
                            "if (snapshotOrigin !== 'https://example.test') throw 'origin';"
                        ),
                    ),
                    // The fixed declaration contains neither fetch nor a
                    // general document object. All three compile attempts fail
                    // before a program is admitted to the child realm.
                    blue_ts_classic(2, "fetch('https://example.test/');"),
                    blue_ts_classic(3, "blueiceDocumentText(1);"),
                    blue_ts_classic(4, "document.getElementById('target');"),
                ],
            ),
        });
        assert!(matches!(
            reply,
            PageHostReply::Synchronized {
                reports,
                ..
            } if reports.len() == 5
                && reports[0].language == PageHostScriptLanguage::BlueTs
                && reports[0].outcome == PageHostScriptOutcome::Executed
                && reports[1].language == PageHostScriptLanguage::JavaScript
                && reports[1].outcome == PageHostScriptOutcome::Executed
                && matches!(reports[2].outcome, PageHostScriptOutcome::Rejected { .. })
                && matches!(reports[3].outcome, PageHostScriptOutcome::Rejected { .. })
                && matches!(reports[4].outcome, PageHostScriptOutcome::Rejected { .. })
        ));
    }

    #[test]
    fn module_graph_uses_only_the_explicit_static_resolution_records() {
        let entry = "blueice://page/entry.js";
        let dependency = "blueice://page/dependency.js";
        let mut module = PageHostScript {
            ordinal: 0,
            language: PageHostScriptLanguage::JavaScript,
            kind: PageHostScriptKind::Module,
            graph: graph(
                entry,
                vec![
                    PageHostSource::new(
                        entry,
                        "import { answer } from './dependency.js'; export const result = answer;",
                    ),
                    PageHostSource::new(dependency, "export const answer = 42;"),
                ],
            ),
        };
        module.graph.resolutions.push(PageHostStaticResolution {
            from_module: entry.to_string(),
            specifier: "./dependency.js".to_string(),
            canonical_target: dependency.to_string(),
        });
        let mut host = BlueJsChildHost::default();
        let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(1, vec![module]),
        });
        assert!(matches!(
            reply,
            PageHostReply::Synchronized {
                reports,
                ..
            } if reports[0].outcome == PageHostScriptOutcome::Executed
        ));
    }

    #[test]
    fn direct_bluets_module_graph_uses_only_the_explicit_static_resolution_records() {
        let entry = "blueice://page/entry.ts";
        let dependency = "blueice://page/dependency.ts";
        let mut module = PageHostScript {
            ordinal: 0,
            language: PageHostScriptLanguage::BlueTs,
            kind: PageHostScriptKind::Module,
            graph: graph(
                entry,
                vec![
                    PageHostSource::new(
                        entry,
                        "import { answer } from './dependency.ts'; export const result: number = answer;",
                    ),
                    PageHostSource::new(dependency, "export const answer: number = 42;"),
                ],
            ),
        };
        module.graph.resolutions.push(PageHostStaticResolution {
            from_module: entry.to_string(),
            specifier: "./dependency.ts".to_string(),
            canonical_target: dependency.to_string(),
        });
        let mut host = BlueJsChildHost::default();
        let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(1, vec![module]),
        });
        assert!(matches!(
            reply,
            PageHostReply::Synchronized {
                reports,
                ..
            } if matches!(
                reports.as_slice(),
                [PageHostScriptReport {
                    language: PageHostScriptLanguage::BlueTs,
                    kind: PageHostScriptKind::Module,
                    outcome: PageHostScriptOutcome::Executed,
                    ..
                }]
            )
        ));
        assert_eq!(host.debug_registry.len(), 2);
        let mut retained_modules: Vec<_> = host
            .documents
            .get(&7)
            .expect("the child realm remains live")
            .debugger_programs
            .values()
            .filter_map(|record| {
                host.debug_registry
                    .get(host.runtime.program_registry(), record.runtime_handle)
                    .ok()
            })
            .map(|metadata| {
                let sources = &metadata.static_info().sources;
                sources
                    .iter()
                    .find(|source| source.module == entry || source.module == dependency)
                    .expect("each module keeps its own static source metadata")
                    .module
                    .clone()
            })
            .collect();
        retained_modules.sort();
        assert_eq!(
            retained_modules,
            vec![dependency.to_string(), entry.to_string()]
        );
    }

    #[test]
    fn leading_bluets_classic_attaches_metadata_and_pauses_at_a_root_safe_point() {
        let mut host = BlueJsChildHost::default();
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: debugger_document(
                    1,
                    vec![
                        blue_ts_classic(
                            0,
                            "let first: number = 1; let deferredAnswer: number = first + 41;",
                        ),
                        classic(1, "globalThis.afterBlueTs = 2;"),
                    ],
                ),
            }),
            PageHostReply::Synchronized { reports, .. } if reports.is_empty()
        ));
        assert_eq!(host.debug_registry.len(), 1);
        let program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
            tab_id: 7,
            document_generation: 1,
        }) {
            PageHostReply::DebuggerPrograms { programs, .. } => {
                assert_eq!(programs.len(), 2);
                programs[0]
            }
            reply => panic!("expected attached BlueTS program, got {reply:?}"),
        };
        let target = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
            tab_id: 7,
            document_generation: 1,
            program,
        }) {
            PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points
                .into_iter()
                .find(|point| point.code_unit_ordinal == 0 && point.bytecode_offset != 0)
                .expect("typed classic fixture needs a non-entry root safe point"),
            reply => panic!("expected BlueTS root safe points, got {reply:?}"),
        };
        assert_eq!(
            host.handle_request(PageHostRequest::GetDebuggerExecutionState {
                tab_id: 7,
                document_generation: 1,
                program,
            }),
            PageHostReply::DebuggerExecutionState {
                tab_id: 7,
                document_generation: 1,
                program,
                state: PageHostDebuggerExecutionState::Pending,
            }
        );
        assert!(matches!(
            host.handle_request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
                tab_id: 7,
                document_generation: 1,
                safe_point: target,
            }),
            PageHostReply::DebuggerRootSafePointBreakpointArmed { safe_point, .. }
                if safe_point == target
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::DebuggerExecutionAdvanced { reports, .. }
                if reports.is_empty()
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::GetDebuggerExecutionState {
                tab_id: 7,
                document_generation: 1,
                program,
            }),
            PageHostReply::DebuggerExecutionState {
                state: PageHostDebuggerExecutionState::Paused { safe_point },
                ..
            } if safe_point == target
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::StepDebuggerRootInstruction {
                tab_id: 7,
                document_generation: 1,
                program,
            }),
            PageHostReply::DebuggerExecutionStepRequested { .. }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::GetDebuggerExecutionState {
                tab_id: 7,
                document_generation: 1,
                program,
            }),
            PageHostReply::DebuggerExecutionState {
                state: PageHostDebuggerExecutionState::Paused { safe_point },
                ..
            } if safe_point.program == program && safe_point != target
        ));
        assert_eq!(host.debug_registry.len(), 1);
        assert!(matches!(
            host.handle_request(PageHostRequest::ResumeDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
                program,
            }),
            PageHostReply::DebuggerExecutionResumed { .. }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::DebuggerExecutionAdvanced { reports, .. }
                if reports == vec![
                    script_report(
                        7,
                        1,
                        0,
                        PageHostScriptLanguage::BlueTs,
                        PageHostScriptKind::Classic,
                        PageHostScriptOutcome::Executed,
                    ),
                    script_report(
                        7,
                        1,
                        1,
                        PageHostScriptLanguage::JavaScript,
                        PageHostScriptKind::Classic,
                        PageHostScriptOutcome::Executed,
                    ),
                ]
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::CloseRealm {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::RealmClosed { .. }
        ));
        assert!(host.debug_registry.is_empty());
    }

    #[test]
    fn child_source_span_step_stops_at_the_next_bound_bluets_statement() {
        let mut host = BlueJsChildHost::default();
        let source = concat!(
            "let first: number = 1; ",
            "let middle: number = first + 1; ",
            "let last: number = middle + 1;"
        );
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: debugger_document(
                    1,
                    vec![blue_ts_classic(0, source), classic(1, "globalThis.afterStep = 1;")],
                ),
            }),
            PageHostReply::Synchronized { reports, .. } if reports.is_empty()
        ));
        let program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
            tab_id: 7,
            document_generation: 1,
        }) {
            PageHostReply::DebuggerPrograms { programs, .. } => programs[0],
            reply => panic!("expected a BlueTS program, got {reply:?}"),
        };
        let metadata = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
            tab_id: 7,
            document_generation: 1,
            program,
        }) {
            PageHostReply::DebuggerBlueTsMetadata { metadata, .. } => metadata[0],
            reply => panic!("expected retained BlueTS metadata, got {reply:?}"),
        };
        let (entries, source_id) = {
            let handle =
                host.documents[&7].debugger_programs[&program.program_handle].runtime_handle;
            let retained = host
                .debug_registry
                .get(host.runtime.program_registry(), handle)
                .unwrap();
            let entries = retained
                .safe_point_map()
                .entries
                .iter()
                .filter(|entry| entry.code_unit.ordinal() == 0)
                .cloned()
                .collect::<Vec<_>>();
            let source_id = retained
                .static_info()
                .sources
                .iter()
                .find(|source| source.module == entries[1].source)
                .unwrap()
                .id
                .0;
            (entries, source_id)
        };
        assert!(entries.len() >= 3, "fixture needs three bound root spans");
        let target_entry = &entries[1];
        let target = PageHostDebuggerSafePoint {
            program,
            code_unit_ordinal: 0,
            bytecode_offset: target_entry.bytecode_offset,
        };
        assert_ne!(target.bytecode_offset, 0);
        assert!(matches!(
            host.handle_request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
                tab_id: 7,
                document_generation: 1,
                safe_point: target,
            }),
            PageHostReply::DebuggerRootSafePointBreakpointArmed { .. }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
        ));
        let request = PageHostRequest::StepDebuggerBlueTsSourceSpan {
            tab_id: 7,
            document_generation: 1,
            metadata,
            source_id,
            safe_point: target,
        };
        let wrong_source_reply =
            host.handle_request(PageHostRequest::StepDebuggerBlueTsSourceSpan {
                tab_id: 7,
                document_generation: 1,
                metadata,
                source_id: source_id + 1,
                safe_point: target,
            });
        assert!(
            matches!(
                wrong_source_reply,
                PageHostReply::Error {
                    code: PageHostErrorCode::InvalidRequest,
                    ..
                }
            ),
            "{wrong_source_reply:?}"
        );
        assert!(matches!(
            host.handle_request(PageHostRequest::StepDebuggerBlueTsSourceSpan {
                tab_id: 7,
                document_generation: 1,
                metadata: PageHostDebuggerMetadataHandle {
                    metadata_generation: metadata.metadata_generation + 1,
                    ..metadata
                },
                source_id,
                safe_point: target,
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::InvalidRequest,
                ..
            }
        ));
        assert_eq!(
            host.handle_request(request.clone()),
            PageHostReply::DebuggerBlueTsSourceStepRequested {
                tab_id: 7,
                document_generation: 1,
                metadata,
                source_id,
                safe_point: target,
            }
        );
        assert!(matches!(
            host.handle_request(request),
            PageHostReply::Error {
                code: PageHostErrorCode::InvalidDebuggerState,
                ..
            }
        ));
        let mut successor = None;
        for _ in 0..MAX_BLUETS_SOURCE_STEP_ROOT_INSTRUCTIONS {
            assert!(matches!(
                host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
                    tab_id: 7,
                    document_generation: 1,
                }),
                PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
            ));
            match host.handle_request(PageHostRequest::GetDebuggerExecutionState {
                tab_id: 7,
                document_generation: 1,
                program,
            }) {
                PageHostReply::DebuggerExecutionState {
                    state: PageHostDebuggerExecutionState::Stepping,
                    ..
                } => {}
                PageHostReply::DebuggerExecutionState {
                    state: PageHostDebuggerExecutionState::Paused { safe_point },
                    ..
                } => {
                    successor = Some(safe_point);
                    break;
                }
                reply => panic!("source step did not stop at a new bound span: {reply:?}"),
            }
        }
        let successor = successor.expect("source step must reach the third statement");
        assert_eq!(successor.bytecode_offset, entries[2].bytecode_offset);
        assert_eq!(host.debug_registry.len(), 1);
        assert!(matches!(
            host.handle_request(PageHostRequest::ResumeDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
                program,
            }),
            PageHostReply::DebuggerExecutionResumed { .. }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.len() == 2
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: debugger_document(2, Vec::new()),
            }),
            PageHostReply::Synchronized { .. }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::StepDebuggerBlueTsSourceSpan {
                tab_id: 7,
                document_generation: 1,
                metadata,
                source_id,
                safe_point: target,
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::StaleDocument,
                ..
            }
        ));
    }

    #[test]
    fn child_source_span_step_yields_at_its_fixed_root_instruction_limit() {
        let mut host = BlueJsChildHost::default();
        let expression = std::iter::repeat_n("first", 320)
            .collect::<Vec<_>>()
            .join(" + ");
        let source = format!(
            "let first: number = 1; let slow: number = {expression}; let last: number = slow + 1;"
        );
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: debugger_document(1, vec![blue_ts_classic(0, &source)]),
            }),
            PageHostReply::Synchronized { reports, .. } if reports.is_empty()
        ));
        let program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
            tab_id: 7,
            document_generation: 1,
        }) {
            PageHostReply::DebuggerPrograms { programs, .. } => programs[0],
            reply => panic!("expected a BlueTS program, got {reply:?}"),
        };
        let metadata = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
            tab_id: 7,
            document_generation: 1,
            program,
        }) {
            PageHostReply::DebuggerBlueTsMetadata { metadata, .. } => metadata[0],
            reply => panic!("expected retained BlueTS metadata, got {reply:?}"),
        };
        let (target, source_id) = {
            let handle =
                host.documents[&7].debugger_programs[&program.program_handle].runtime_handle;
            let retained = host
                .debug_registry
                .get(host.runtime.program_registry(), handle)
                .unwrap();
            let entry = retained
                .safe_point_map()
                .entries
                .iter()
                .filter(|entry| entry.code_unit.ordinal() == 0)
                .nth(1)
                .expect("the long second statement must have a bound root entry");
            let source_id = retained
                .static_info()
                .sources
                .iter()
                .find(|source| source.module == entry.source)
                .unwrap()
                .id
                .0;
            (
                PageHostDebuggerSafePoint {
                    program,
                    code_unit_ordinal: 0,
                    bytecode_offset: entry.bytecode_offset,
                },
                source_id,
            )
        };
        assert!(matches!(
            host.handle_request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
                tab_id: 7,
                document_generation: 1,
                safe_point: target,
            }),
            PageHostReply::DebuggerRootSafePointBreakpointArmed { .. }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::StepDebuggerBlueTsSourceSpan {
                tab_id: 7,
                document_generation: 1,
                metadata,
                source_id,
                safe_point: target,
            }),
            PageHostReply::DebuggerBlueTsSourceStepRequested { .. }
        ));
        for turn in 1..=MAX_BLUETS_SOURCE_STEP_ROOT_INSTRUCTIONS {
            assert!(matches!(
                host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
                    tab_id: 7,
                    document_generation: 1,
                }),
                PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
            ));
            let state = host.handle_request(PageHostRequest::GetDebuggerExecutionState {
                tab_id: 7,
                document_generation: 1,
                program,
            });
            if turn < MAX_BLUETS_SOURCE_STEP_ROOT_INSTRUCTIONS {
                assert!(
                    matches!(
                        state,
                        PageHostReply::DebuggerExecutionState {
                            state: PageHostDebuggerExecutionState::Stepping,
                            ..
                        }
                    ),
                    "unexpected pre-limit state on turn {turn}: {state:?}"
                );
            } else {
                assert!(
                    matches!(
                        state,
                        PageHostReply::DebuggerExecutionState {
                            state: PageHostDebuggerExecutionState::SourceStepLimitReached { safe_point },
                            ..
                        } if safe_point.program == program && safe_point != target
                    ),
                    "source step must yield at its exact budget: {state:?}"
                );
            }
        }
        assert!(matches!(
            host.handle_request(PageHostRequest::ResumeDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
                program,
            }),
            PageHostReply::DebuggerExecutionResumed { .. }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.len() == 1
        ));
    }

    #[test]
    fn a_missing_module_resolution_admits_no_partial_graph_programs() {
        let entry = "blueice://page/entry.js";
        let dependency = "blueice://page/dependency.js";
        let script = PageHostScript {
            ordinal: 0,
            language: PageHostScriptLanguage::JavaScript,
            kind: PageHostScriptKind::Module,
            graph: graph(
                entry,
                vec![
                    PageHostSource::new(entry, "import './dependency.js';"),
                    PageHostSource::new(dependency, "export const answer = 42;"),
                ],
            ),
        };
        let mut host = BlueJsChildHost::default();
        let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(1, vec![script]),
        });
        assert!(matches!(
            reply,
            PageHostReply::Synchronized {
                reports,
                ..
            } if matches!(reports[0].outcome, PageHostScriptOutcome::Rejected { .. })
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::GetRealmStats {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::RealmStats(PageHostRealmStats {
                program_count: 0,
                bytecode_bytes: 0,
                ..
            })
        ));
    }

    #[test]
    fn document_generations_are_idempotent_and_cannot_close_a_successor() {
        let mut host = BlueJsChildHost::default();
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: document(1, vec![classic(0, "globalThis.first = true;")]),
            }),
            PageHostReply::Synchronized {
                already_current: false,
                ..
            }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: document(1, vec![classic(0, "throw new Error('must not replay');")]),
            }),
            PageHostReply::Synchronized {
                already_current: true,
                reports,
                ..
            } if reports.is_empty()
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: document(2, vec![classic(0, "globalThis.second = true;")]),
            }),
            PageHostReply::Synchronized {
                already_current: false,
                ..
            }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::CloseRealm {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::StaleDocument,
                ..
            }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::GetRealmStats {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::StaleDocument,
                ..
            }
        ));
    }

    #[test]
    fn source_hash_tampering_is_rejected_before_realm_program_admission() {
        let mut source = PageHostSource::new("blueice://page/main.js", "globalThis.answer = 42;");
        source.source_hash = "fnv1a64:0000000000000000".to_string();
        let script = PageHostScript {
            ordinal: 0,
            language: PageHostScriptLanguage::JavaScript,
            kind: PageHostScriptKind::Classic,
            graph: graph("blueice://page/main.js", vec![source]),
        };
        let mut host = BlueJsChildHost::default();
        let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(1, vec![script]),
        });
        assert!(matches!(
            reply,
            PageHostReply::Synchronized {
                reports,
                ..
            } if matches!(reports[0].outcome, PageHostScriptOutcome::Rejected { .. })
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::GetRealmStats {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::RealmStats(PageHostRealmStats {
                program_count: 0,
                ..
            })
        ));
    }

    #[test]
    fn document_source_budget_rejects_before_realm_replacement() {
        let source = PageHostSource::new(
            "blueice://page/too-large.js",
            "x".repeat(MAX_SOURCE_BYTES_PER_DOCUMENT + 1),
        );
        let script = PageHostScript {
            ordinal: 0,
            language: PageHostScriptLanguage::JavaScript,
            kind: PageHostScriptKind::Classic,
            graph: graph("blueice://page/too-large.js", vec![source]),
        };
        let mut host = BlueJsChildHost::default();
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: document(1, vec![script]),
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::ResourceLimit,
                ..
            }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::GetRealmStats {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::UnknownRealm,
                ..
            }
        ));
    }

    #[test]
    fn duplicate_script_ordinals_are_rejected_before_realm_replacement() {
        let mut host = BlueJsChildHost::default();
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: document(
                    1,
                    vec![
                        classic(0, "globalThis.first = true;"),
                        classic(0, "globalThis.second = true;"),
                    ],
                ),
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::InvalidRequest,
                ..
            }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::GetRealmStats {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::UnknownRealm,
                ..
            }
        ));
    }

    #[test]
    fn private_socket_is_owner_only() {
        let path = PathBuf::from("/private/tmp").join(format!(
            "blueice-launcher-owner-only-test-{}.sock",
            std::process::id()
        ));
        let listener = bind_bluejs_host_socket(&path).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        drop(listener);
        let _ = fs::remove_file(path);
    }
}
