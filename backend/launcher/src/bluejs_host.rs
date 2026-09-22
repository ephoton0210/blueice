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
    parse, parse_module, BlueJsPageOrigin, BlueJsPageRuntime, BlueJsPageRuntimeConfig,
    BlueJsPageRuntimeError, BlueJsProgramHandle, BlueJsProgramV1, BlueJsSourceIdentity,
    CompileError, Module, ParseError, RuntimeError, Value,
};
use blueice_ipc::page_host::{
    self, PageHostDocument, PageHostErrorCode, PageHostModuleGraph, PageHostRealmStats,
    PageHostReply, PageHostRequest, PageHostScript, PageHostScriptKind, PageHostScriptOutcome,
    PageHostScriptReport, PageHostSource, PageHostStaticResolution,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

/// Execution/source limits independently enforced by the child. The caller
/// cannot widen them by serializing a larger graph over its private socket.
const MAX_SCRIPTS_PER_DOCUMENT: usize = 256;
const MAX_MODULES_PER_GRAPH: usize = 8;
const MAX_SOURCE_BYTES_PER_MODULE: usize = 1024 * 1024;
const MAX_SOURCE_BYTES_PER_DOCUMENT: usize = 8 * 1024 * 1024;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
struct LiveDocument {
    generation: u64,
}

/// The actual state machine running in the child process.
///
/// It accepts only fully selected source records. In particular, this type
/// has no filesystem/network resolver, no DOM/IPC callback path, and no API
/// that gives the parent a VM or a program handle.
pub struct BlueJsChildHost {
    runtime: BlueJsPageRuntime,
    documents: BTreeMap<u64, LiveDocument>,
}

impl BlueJsChildHost {
    /// Creates an empty, isolated page host with the public BlueJS runtime's
    /// default fixed realm/program/bytecode bounds.
    pub fn new() -> Result<Self, BlueJsPageRuntimeError> {
        Self::with_runtime_config(BlueJsPageRuntimeConfig::default())
    }

    /// Creates a host with launcher-selected runtime limits. This is exposed
    /// for deterministic tests and future launcher policy wiring; the page
    /// protocol contains no operation for a caller to modify it.
    pub fn with_runtime_config(
        config: BlueJsPageRuntimeConfig,
    ) -> Result<Self, BlueJsPageRuntimeError> {
        Ok(Self {
            runtime: BlueJsPageRuntime::new(config)?,
            documents: BTreeMap::new(),
        })
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
        let origin = match BlueJsPageOrigin::new(document.origin) {
            Ok(origin) => origin,
            Err(_) => return invalid_request(),
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

        // Parse/compile every independent declaration first. A graph that
        // cannot be structurally admitted becomes one source-free rejection,
        // while a later declaration remains eligible exactly as browser
        // document-order execution requires. No candidate program enters a
        // new realm until source/graph preflight has completed.
        let prepared: Vec<_> = document.scripts.into_iter().map(prepare_script).collect();

        let lifecycle = if self.documents.contains_key(&document.tab_id) {
            self.runtime.navigate(document.tab_id, origin.clone())
        } else {
            self.runtime.open_realm(document.tab_id, origin.clone())
        };
        if lifecycle.is_err() {
            return host_failure();
        }
        self.documents.insert(
            document.tab_id,
            LiveDocument {
                generation: document.document_generation,
            },
        );

        let mut reports = Vec::with_capacity(prepared.len());
        for prepared in prepared {
            let (ordinal, kind, outcome) = match prepared {
                PreparedScript::Rejected {
                    ordinal,
                    kind,
                    category,
                } => (
                    ordinal,
                    kind,
                    PageHostScriptOutcome::Rejected {
                        category: category.to_string(),
                    },
                ),
                PreparedScript::Classic {
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
                    (ordinal, PageHostScriptKind::Classic, outcome)
                }
                PreparedScript::Module {
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
                    (ordinal, PageHostScriptKind::Module, outcome)
                }
            };
            reports.push(PageHostScriptReport {
                tab_id: document.tab_id,
                document_generation: document.document_generation,
                ordinal,
                kind,
                outcome,
            });
        }
        PageHostReply::Synchronized {
            tab_id: document.tab_id,
            document_generation: document.document_generation,
            already_current: false,
            reports,
        }
    }

    fn close_realm(&mut self, tab_id: u64, document_generation: u64) -> PageHostReply {
        match self.documents.get(&tab_id) {
            None => unknown_realm(),
            Some(document) if document.generation != document_generation => stale_document(),
            Some(_) => {
                self.runtime.close_realm(tab_id);
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
            Ok(stats) => PageHostReply::RealmStats(PageHostRealmStats {
                tab_id,
                document_generation,
                program_count: u32::try_from(stats.program_count).unwrap_or(u32::MAX),
                bytecode_bytes: u64::try_from(stats.bytecode_bytes).unwrap_or(u64::MAX),
                heap_bytes: u64::try_from(stats.heap.managed_bytes).unwrap_or(u64::MAX),
            }),
            Err(_) => host_failure(),
        }
    }
}

impl Default for BlueJsChildHost {
    fn default() -> Self {
        Self::new().expect("the default BlueJS child-host configuration is valid")
    }
}

enum PreparedScript {
    Rejected {
        ordinal: u32,
        kind: PageHostScriptKind,
        category: &'static str,
    },
    Classic {
        ordinal: u32,
        source: PageHostSource,
        program: BlueJsProgramV1,
    },
    Module {
        ordinal: u32,
        graph: PageHostModuleGraph,
        programs: BTreeMap<String, BlueJsProgramV1>,
    },
}

fn prepare_script(script: PageHostScript) -> PreparedScript {
    let ordinal = script.ordinal;
    let kind = script.kind;
    let prepared = match kind {
        PageHostScriptKind::Classic => {
            prepare_classic(script.graph).map(|(source, program)| PreparedScript::Classic {
                ordinal,
                source,
                program,
            })
        }
        PageHostScriptKind::Module => {
            prepare_module_graph(script.graph).map(|(graph, programs)| PreparedScript::Module {
                ordinal,
                graph,
                programs,
            })
        }
    };
    prepared.unwrap_or_else(|category| PreparedScript::Rejected {
        ordinal,
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

fn invalid_request() -> PageHostReply {
    PageHostReply::Error {
        code: PageHostErrorCode::InvalidRequest,
        message: "BlueJS page-host request is invalid".to_string(),
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

/// A launcher-owned child and the single authenticated private connection to
/// it. Dropping this handle kills/reaps the process and removes the socket,
/// matching [`crate::SpawnedCore`]'s ownership discipline.
pub struct SpawnedBlueJsHost {
    child: Child,
    socket_path: PathBuf,
    stream: UnixStream,
}

impl SpawnedBlueJsHost {
    /// Spawns the sibling `blueice-bluejs-host` binary, waits for its private
    /// socket, then performs the authenticated v1 handshake before returning
    /// a usable handle. A startup failure always reaps the child and removes
    /// the private socket.
    pub fn spawn() -> io::Result<Self> {
        let this_exe = std::env::current_exe()?;
        let binary = sibling_bluejs_host_binary(&this_exe);
        let socket_path = unique_bluejs_host_socket_path();
        let token = secure_session_token()?;
        let _ = fs::remove_file(&socket_path);
        let mut child = Command::new(&binary)
            .arg("--socket")
            .arg(&socket_path)
            .arg("--session-token")
            .arg(&token)
            .spawn()?;

        let started = wait_for_child_socket(&mut child, &socket_path, STARTUP_TIMEOUT);
        if let Err(error) = started {
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_file(&socket_path);
            return Err(error);
        }
        let mut stream = match UnixStream::connect(&socket_path) {
            Ok(stream) => stream,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = fs::remove_file(&socket_path);
                return Err(error);
            }
        };
        let hello = PageHostRequest::Hello {
            protocol_version: page_host::PAGE_HOST_PROTOCOL_VERSION,
            session_token: token,
        };
        let handshake = (|| -> io::Result<PageHostReply> {
            page_host::write_page_host_request(&mut stream, &hello)?;
            page_host::read_page_host_reply(&mut stream)
        })();
        match handshake {
            Ok(PageHostReply::HelloAck {
                protocol_version: page_host::PAGE_HOST_PROTOCOL_VERSION,
            }) => Ok(Self {
                child,
                socket_path,
                stream,
            }),
            Ok(reply) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = fs::remove_file(&socket_path);
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("BlueJS page host rejected launcher handshake: {reply:?}"),
                ))
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = fs::remove_file(&socket_path);
                Err(error)
            }
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
        page_host::write_page_host_request(&mut self.stream, &request)?;
        page_host::read_page_host_reply(&mut self.stream)
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

    fn graph(entry: &str, modules: Vec<PageHostSource>) -> PageHostModuleGraph {
        PageHostModuleGraph {
            entry: entry.to_string(),
            modules,
            resolutions: Vec::new(),
            resolver_fingerprint: "core-page-loader-v1".to_string(),
        }
    }

    fn document(generation: u64, scripts: Vec<PageHostScript>) -> PageHostDocument {
        PageHostDocument {
            tab_id: 7,
            document_generation: generation,
            origin: "https://example.test".to_string(),
            scripts,
        }
    }

    fn classic(ordinal: u32, source: &str) -> PageHostScript {
        let id = format!("blueice://page/inline-{ordinal}.js");
        PageHostScript {
            ordinal,
            kind: PageHostScriptKind::Classic,
            graph: graph(&id, vec![PageHostSource::new(id.clone(), source)]),
        }
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
    fn module_graph_uses_only_the_explicit_static_resolution_records() {
        let entry = "blueice://page/entry.js";
        let dependency = "blueice://page/dependency.js";
        let mut module = PageHostScript {
            ordinal: 0,
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
    fn a_missing_module_resolution_admits_no_partial_graph_programs() {
        let entry = "blueice://page/entry.js";
        let dependency = "blueice://page/dependency.js";
        let script = PageHostScript {
            ordinal: 0,
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
