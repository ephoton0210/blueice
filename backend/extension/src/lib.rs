// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `blueice-extension-host`: the minimal-slice implementation of
//! `phase-9-extension-protocol/PLAN.md`'s incremental reference host.
//! It retains a hardcoded protocol-only fallback, and can now install one
//! strict JSON manifest plus WASM module at startup to derive its registry
//! identity and persistent grants before serving any connection. Its
//! core-spawned mode now runs a bounded, no-WASI WebAssembly reactor after
//! authentication.
//!
//! **Where this logic lives, and why.** The plan doc's "Wiring design"
//! frames the enforcing side as living inside `core` itself (an
//! `ExtensionRegistry` "a peer to `TabManager`, not a member of it").
//! The standalone `blueice-extension-host` binary remains a protocol-only
//! reference server. `blueice-core` can now opt in with an installed manifest
//! and private extension socket: it owns the registry and calls
//! [`handle_extension_connection_with_actions`] from a connection worker,
//! delegating bounded real operations back to its session thread.
//! This crate still does not depend on `blueice-engine`; that preserves the
//! protocol/engine boundary and keeps `Page`/`TabManager` single-thread-owned.
//! The bridge remains deliberately narrow: it has explicit tab/node targets
//! for text and checkbox writes, but no generic DOM mutation or declarative
//! interception-rule representation.
//!
//! **What's real, what's a placeholder** (mirrors `blueice-ai-
//! gatekeeper`'s own "mechanism real, content stub" scoping): the
//! handshake, the registry lookup, and the allow/deny decision are all
//! real and tested. The standalone binary retains a fixed
//! [`ExtensionReply::DomReadResult`] value, but
//! [`handle_extension_connection_with_actions`] lets `blueice-core`
//! provide core-owned representation reads plus explicit text/checkbox writes
//! without this crate taking an engine dependency. Generic `DomWrite` and
//! `NetworkIntercept` still lack safe core operation shapes, so they are
//! reported as unavailable rather than acknowledged without an effect.
//!
//! **Identity derivation versus peer authentication.**
//! [`load_installed_extension`] derives a `sha256:` ID from exact manifest and
//! WASM bytes, so package authors cannot assign an arbitrary friendly ID and a
//! changed installed artifact receives a different registry identity. The
//! core-owned production path now starts a host child and requires its fresh,
//! environment-only credential in
//! [`blueice_ipc::extension::ExtensionRequest::HelloAuthenticated`] before it
//! accepts that ID. The standalone server and an explicit manual core socket
//! deliberately retain the bearer [`blueice_ipc::extension::ExtensionRequest::Hello`]
//! form for protocol development; a derived identity alone is not credentials.

mod manifest;
mod runtime;

pub use manifest::{
    load_installed_extension, registry_for_installed_extension, ExtensionManifest,
    InstalledExtension, ManifestCapabilities, ManifestError, MANIFEST_API_VERSION,
};
pub use runtime::execute_installed_extension;

use blueice_ipc::extension::{
    read_extension_request, write_extension_reply, ExtensionReply, ExtensionRequest,
    UnsupportedCapabilityVersion,
};
use blueice_ipc::gatekeeper::{
    default_gatekeeper_socket_path, read_gatekeeper_reply, write_gatekeeper_request,
    GatekeeperReply, GatekeeperRequest,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

/// The one hardcoded identity used only when no installed manifest is supplied.
pub const MINIMAL_SLICE_EXTENSION_ID: &str = "minimal-slice-extension";

/// The capability this slice's one hardcoded extension is granted.
pub const CAPABILITY_DOM_READ: &str = "dom:read";

/// The capability this slice's one hardcoded extension is deliberately
/// *not* granted -- the request that proves server-side denial.
pub const CAPABILITY_DOM_WRITE: &str = "dom:write";

/// Registering network interception is high-risk and therefore always
/// needs a cleared gatekeeper review after ordinary capability checks.
pub const CAPABILITY_NETWORK_INTERCEPT: &str = "network:intercept";

/// An inclusive API-version interval a host supports for one capability.
/// A capability grant and a supported version are deliberately separate:
/// an extension may use a known API version without being authorized to
/// invoke that capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapabilityVersionWindow {
    min_inclusive: u32,
    max_inclusive: u32,
}

impl CapabilityVersionWindow {
    /// Creates a valid inclusive version window, or returns `None` when
    /// its bounds are inverted.
    pub const fn new(min_inclusive: u32, max_inclusive: u32) -> Option<Self> {
        if min_inclusive <= max_inclusive {
            Some(Self {
                min_inclusive,
                max_inclusive,
            })
        } else {
            None
        }
    }

    /// Whether `version` is supported by this interval.
    pub const fn contains(self, version: u32) -> bool {
        self.min_inclusive <= version && version <= self.max_inclusive
    }

    /// The lower inclusive API-version bound.
    pub const fn min_inclusive(self) -> u32 {
        self.min_inclusive
    }

    /// The upper inclusive API-version bound.
    pub const fn max_inclusive(self) -> u32 {
        self.max_inclusive
    }
}

/// [`ExtensionReply::DomReadResult`]'s value for a granted `DomRead` in
/// this minimal slice -- a fixed placeholder, not real `Page` state; see
/// this crate's own module docs for why.
const PLACEHOLDER_DOM_READ_VALUE: &str =
    "<blueice-extension-host: no real Page is wired into this minimal slice>";

const GATEKEEPER_CHECK_TIMEOUT: Duration = Duration::from_secs(10);

/// Which capabilities each connected extension has been granted. The registry
/// is keyed by an installed package's derived ID when used by
/// `blueice-core`/`registry_for_installed_extension`; the hardcoded
/// [`ExtensionRegistry::minimal_slice`] remains only a protocol-test fallback.
/// A derived ID establishes exact package membership and grants, but it is not
/// connection credentials. Core's optional host-spawned path adds that separate
/// boundary; standalone/manual protocol development intentionally does not.
pub struct ExtensionRegistry {
    grants: HashMap<String, HashSet<String>>,
    supported_versions: HashMap<String, CapabilityVersionWindow>,
}

impl Default for ExtensionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ExtensionRegistry {
    /// An empty registry: no extension_id is granted anything.
    pub fn new() -> Self {
        Self {
            grants: HashMap::new(),
            supported_versions: HashMap::new(),
        }
    }

    /// Registers the complete set of capability API windows this Phase 9 host
    /// understands. Installation grants remain a separate operation, so an
    /// extension cannot turn host support into authority merely by declaring a
    /// capability in its manifest or handshake.
    pub fn with_supported_capabilities() -> Self {
        let mut registry = Self::new();
        let v1 = CapabilityVersionWindow::new(1, 1).expect("literal version window is valid");
        let v1_to_v2 = CapabilityVersionWindow::new(1, 2).expect("literal version window is valid");
        let v1_to_v3 = CapabilityVersionWindow::new(1, 3).expect("literal version window is valid");
        registry.register_capability_version_window(CAPABILITY_DOM_READ, v1_to_v2);
        registry.register_capability_version_window(CAPABILITY_DOM_WRITE, v1_to_v3);
        registry.register_capability_version_window(CAPABILITY_NETWORK_INTERCEPT, v1);
        registry
    }

    /// Registers the API-version window this host implements for a
    /// capability. Registering a capability does not grant it to any
    /// extension; use [`Self::grant`] for that separate decision.
    pub fn register_capability_version_window(
        &mut self,
        capability: impl Into<String>,
        window: CapabilityVersionWindow,
    ) {
        self.supported_versions.insert(capability.into(), window);
    }

    /// Grants `capability` to `extension_id`, in addition to whatever
    /// it already holds.
    pub fn grant(&mut self, extension_id: impl Into<String>, capability: impl Into<String>) {
        self.grants
            .entry(extension_id.into())
            .or_default()
            .insert(capability.into());
    }

    /// The actual enforcement point: does `extension_id` currently hold
    /// `capability`? An unrecognized `extension_id` (never granted
    /// anything) simply has no capabilities -- see this crate's module
    /// docs for why an unknown identity isn't rejected outright at
    /// handshake time.
    pub fn has_capability(&self, extension_id: &str, capability: &str) -> bool {
        self.grants
            .get(extension_id)
            .is_some_and(|caps| caps.contains(capability))
    }

    /// Returns why a declared API version is unavailable, if it cannot
    /// be negotiated with this host.
    pub fn unsupported_capability_version(
        &self,
        capability: &str,
        version: u32,
    ) -> Option<UnsupportedCapabilityVersion> {
        match self.supported_versions.get(capability).copied() {
            None => Some(UnsupportedCapabilityVersion::UnknownCapability),
            Some(window) if !window.contains(version) => {
                Some(UnsupportedCapabilityVersion::OutsideSupportedRange {
                    min_inclusive: window.min_inclusive(),
                    max_inclusive: window.max_inclusive(),
                })
            }
            Some(_) => None,
        }
    }

    /// Validates all declarations in one `Hello`, retaining every
    /// independently unsupported entry for the structured handshake
    /// reply. This never rejects a compatible declaration merely because
    /// another capability on the same connection is unavailable.
    pub fn unsupported_capability_versions(
        &self,
        declared_versions: &BTreeMap<String, u32>,
    ) -> BTreeMap<String, UnsupportedCapabilityVersion> {
        declared_versions
            .iter()
            .filter_map(|(capability, version)| {
                self.unsupported_capability_version(capability, *version)
                    .map(|problem| (capability.clone(), problem))
            })
            .collect()
    }

    /// Seeds the hardcoded single-extension grant this minimal slice
    /// ships: [`MINIMAL_SLICE_EXTENSION_ID`] gets [`CAPABILITY_DOM_READ`]
    /// only -- deliberately *not* [`CAPABILITY_DOM_WRITE`] or
    /// [`CAPABILITY_NETWORK_INTERCEPT`], so a `DomWrite` or
    /// `NetworkIntercept` attempt is a concrete proof of server-side
    /// denial.
    pub fn minimal_slice() -> Self {
        let mut registry = Self::with_supported_capabilities();
        registry.grant(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ);
        registry
    }
}

/// Connection-scoped identity plus the subset of capability declarations
/// whose API version was successfully negotiated. The grant lookup stays
/// in [`ExtensionRegistry`], so the extension cannot manufacture either
/// half of an authorization decision in a later request.
struct ConnectionIdentity {
    extension_id: String,
    negotiated_capabilities: BTreeMap<String, u32>,
}

fn negotiate_hello(
    registry: &ExtensionRegistry,
    extension_id: String,
    capability_versions: BTreeMap<String, u32>,
) -> (ConnectionIdentity, ExtensionReply) {
    let unsupported_capabilities = registry.unsupported_capability_versions(&capability_versions);
    let negotiated_capabilities = capability_versions
        .into_iter()
        .filter(|(capability, _)| !unsupported_capabilities.contains_key(capability))
        .collect();

    (
        ConnectionIdentity {
            extension_id,
            negotiated_capabilities,
        },
        ExtensionReply::HelloAck {
            unsupported_capabilities,
        },
    )
}

fn capability_denial_reason(
    registry: &ExtensionRegistry,
    identity: &ConnectionIdentity,
    capability: &str,
    minimum_version: u32,
) -> Option<String> {
    let Some(version) = identity.negotiated_capabilities.get(capability) else {
        return Some(format!(
            "{} did not negotiate a supported version of {capability}",
            identity.extension_id
        ));
    };
    if *version < minimum_version {
        return Some(format!(
            "{} negotiated {capability} version {version}, but this request requires version {minimum_version}",
            identity.extension_id
        ));
    }
    if !registry.has_capability(&identity.extension_id, capability) {
        return Some(format!(
            "{} is not granted {capability}",
            identity.extension_id
        ));
    }
    None
}

fn check_extension_action(
    gatekeeper_socket: &Path,
    extension_id: &str,
    capability: &str,
    detail: String,
) -> Result<GatekeeperReply, String> {
    let mut stream = UnixStream::connect(gatekeeper_socket)
        .map_err(|error| format!("could not connect to the gatekeeper: {error}"))?;
    stream
        .set_read_timeout(Some(GATEKEEPER_CHECK_TIMEOUT))
        .map_err(|error| format!("could not configure the gatekeeper read deadline: {error}"))?;
    stream
        .set_write_timeout(Some(GATEKEEPER_CHECK_TIMEOUT))
        .map_err(|error| format!("could not configure the gatekeeper write deadline: {error}"))?;
    write_gatekeeper_request(
        &mut stream,
        &GatekeeperRequest::CheckExtensionAction {
            extension_id: extension_id.to_string(),
            capability: capability.to_string(),
            detail,
        },
    )
    .map_err(|error| format!("could not send the extension action for review: {error}"))?;
    read_gatekeeper_reply(&mut stream)
        .map_err(|error| format!("could not read the gatekeeper decision: {error}"))
}

/// Serves one extension connection until it disconnects (or sends
/// something this minimal slice can't make sense of -- see below):
/// requires [`ExtensionRequest::Hello`] as the very first message
/// (rejecting/ending the connection otherwise, mirroring how
/// `blueice_engine::session`'s `perform_handshake` rejects a non-`Hello`
/// first message on the external client protocol), replies
/// [`ExtensionReply::HelloAck`] (including any individually unsupported
/// capability versions), then loops handling `DomRead`/
/// `DomWrite`/`NetworkIntercept` requests -- checking `registry` before executing each,
/// replying [`ExtensionReply::CapabilityDenied`] for an unauthorized
/// request rather than a silent no-op or a bare/generic error.
///
/// **A later `Hello`** (once past the initial handshake) is accepted,
/// renegotiates the independently versioned capability set, and answers
/// with another `HelloAck`, updating which `extension_id` subsequent
/// requests on this connection are checked against -- the same
/// "answered again, not re-gating the whole connection" discipline
/// `run_session`'s own docs describe for a repeat `Hello` on the
/// external client protocol, applied here to a long-lived extension
/// connection.
///
/// **Any read failure** (a clean disconnect, or bytes that don't parse
/// as a well-formed [`ExtensionRequest`]) ends the connection by
/// returning `Ok(())`, never propagating a "malformed input" case as a
/// distinguishable error and never panicking -- mirrors `run_session`'s
/// own "any non-timeout read error means disconnect" handling of the
/// external client protocol's main loop. The wire-level parsing
/// functions this calls (`blueice_ipc::extension::read_extension_
/// request`) do still surface a malformed frame as a real `io::Error`
/// to *their own* callers/tests -- this fn just chooses, deliberately,
/// to treat that the same as an ordinary disconnect rather than
/// escalate it, the same choice `run_session` already made for the
/// analogous case on the external protocol.
pub fn handle_extension_connection<S: Read + Write>(
    registry: &ExtensionRegistry,
    stream: &mut S,
) -> io::Result<()> {
    let gatekeeper_socket = default_gatekeeper_socket_path();
    handle_extension_connection_with_gatekeeper(registry, &gatekeeper_socket, stream)
}

/// Like [`handle_extension_connection`], but routes high-risk extension
/// actions through an explicit gatekeeper socket. The launcher and tests
/// pass an isolated path; the public convenience function above retains
/// the conventional standalone-development default.
pub fn handle_extension_connection_with_gatekeeper<S: Read + Write>(
    registry: &ExtensionRegistry,
    gatekeeper_socket: &Path,
    stream: &mut S,
) -> io::Result<()> {
    handle_extension_connection_with_actions(
        registry,
        gatekeeper_socket,
        stream,
        |_| Ok(PLACEHOLDER_DOM_READ_VALUE.to_string()),
        |_, _, _| Ok(()),
        || Ok(()),
    )
}

/// Like [`handle_extension_connection_with_gatekeeper`], but delegates a
/// capability-approved operation to the process that owns the actual page
/// state. This preserves the authorization and, where required, gatekeeper
/// checks in this host before any effect is requested. A delegate error is a
/// structured [`ExtensionReply::OperationUnavailable`] response, not a false
/// acknowledgement.
///
/// The standalone host supplies placeholder delegates through
/// [`handle_extension_connection_with_gatekeeper`]. `blueice-core` supplies
/// a synchronous channel-backed read delegate so its session thread remains
/// the sole mutable owner of `TabManager`/`Page` state.
pub fn handle_extension_connection_with_actions<S, R, W, N>(
    registry: &ExtensionRegistry,
    gatekeeper_socket: &Path,
    stream: &mut S,
    read_dom: R,
    write_dom: W,
    register_network_intercept: N,
) -> io::Result<()>
where
    S: Read + Write,
    R: FnMut(Option<u64>) -> Result<String, String>,
    W: FnMut(
        Option<(u64, u64)>,
        String,
        &blueice_ipc::extension::DomWriteTarget,
    ) -> Result<(), String>,
    N: FnMut() -> Result<(), String>,
{
    handle_extension_connection_with_actions_and_authentication(
        registry,
        gatekeeper_socket,
        stream,
        ExtensionConnectionAuthentication::unauthenticated(),
        read_dom,
        write_dom,
        register_network_intercept,
    )
}

/// Connection authentication for
/// [`handle_extension_connection_with_actions_and_authentication`].
///
/// The standalone protocol server uses [`Self::unauthenticated`]. Core's
/// host-spawned mode uses [`Self::required`] and can attach a one-shot
/// readiness sender that fires only after the first valid handshake.
pub struct ExtensionConnectionAuthentication<'a> {
    expected: Option<&'a str>,
    authenticated_ready: Option<mpsc::Sender<()>>,
    runtime_start: Option<Arc<Mutex<mpsc::Receiver<()>>>>,
}

impl<'a> ExtensionConnectionAuthentication<'a> {
    /// Allows the documented bearer-claim development protocol.
    pub const fn unauthenticated() -> Self {
        Self {
            expected: None,
            authenticated_ready: None,
            runtime_start: None,
        }
    }

    /// Requires every hello to prove this core-generated credential.
    pub const fn required(expected: &'a str) -> Self {
        Self {
            expected: Some(expected),
            authenticated_ready: None,
            runtime_start: None,
        }
    }

    /// Signals core after the first valid handshake, before any operation can
    /// be handled on the connection.
    pub fn with_ready_notification(mut self, ready: mpsc::Sender<()>) -> Self {
        self.authenticated_ready = Some(ready);
        self
    }

    /// Installs core's one-shot session-start barrier for the authenticated
    /// child. The receiver is shared only because the listener accepts peers
    /// concurrently; only a peer that already proved `expected` can consume
    /// it through `RuntimeReady`.
    pub fn with_runtime_start_receiver(
        mut self,
        runtime_start: Arc<Mutex<mpsc::Receiver<()>>>,
    ) -> Self {
        self.runtime_start = Some(runtime_start);
        self
    }

    fn expected(&self) -> Option<&str> {
        self.expected
    }

    fn signal_ready(&self) {
        if let Some(ready) = &self.authenticated_ready {
            let _ = ready.send(());
        }
    }

    fn wait_for_runtime_start(&self) -> Result<(), String> {
        let receiver = self.runtime_start.as_ref().ok_or_else(|| {
            "the core has no runtime-start barrier for this extension connection".to_string()
        })?;
        receiver
            .lock()
            .map_err(|_| "the core runtime-start barrier was poisoned".to_string())?
            .recv()
            .map_err(|_| "the core ended before its extension runtime could start".to_string())
    }
}

/// Like [`handle_extension_connection_with_actions`], but applies the supplied
/// [`ExtensionConnectionAuthentication`] before it acknowledges a handshake.
/// `blueice-core` uses a required credential for a host it spawned itself; the
/// standalone server deliberately uses the unauthenticated development mode.
pub fn handle_extension_connection_with_actions_and_authentication<S, R, W, N>(
    registry: &ExtensionRegistry,
    gatekeeper_socket: &Path,
    stream: &mut S,
    authentication: ExtensionConnectionAuthentication<'_>,
    mut read_dom: R,
    mut write_dom: W,
    mut register_network_intercept: N,
) -> io::Result<()>
where
    S: Read + Write,
    R: FnMut(Option<u64>) -> Result<String, String>,
    W: FnMut(
        Option<(u64, u64)>,
        String,
        &blueice_ipc::extension::DomWriteTarget,
    ) -> Result<(), String>,
    N: FnMut() -> Result<(), String>,
{
    let mut identity = match read_extension_request(stream) {
        Ok(request) => match authenticated_hello(authentication.expected(), request) {
            Some((extension_id, capability_versions)) => {
                let (identity, reply) =
                    negotiate_hello(registry, extension_id, capability_versions);
                write_extension_reply(stream, &reply)?;
                identity
            }
            None => return Ok(()), // not an allowed first handshake: reject without an acknowledgement
        },
        Err(_) => return Ok(()), // disconnected, or sent something unparseable, before ever completing the handshake
    };
    authentication.signal_ready();
    let mut runtime_started = false;

    loop {
        let request = match read_extension_request(stream) {
            Ok(request) => request,
            Err(_) => return Ok(()),
        };
        match request {
            ExtensionRequest::Hello {
                extension_id,
                capability_versions,
            } => {
                if authentication.expected().is_some() {
                    return Ok(());
                }
                let (new_identity, reply) =
                    negotiate_hello(registry, extension_id, capability_versions);
                write_extension_reply(stream, &reply)?;
                identity = new_identity;
            }
            ExtensionRequest::HelloAuthenticated {
                extension_id,
                capability_versions,
                authentication: provided_authentication,
            } => {
                if authentication.expected().is_some_and(|expected| {
                    !constant_time_authentication_matches(expected, &provided_authentication)
                }) {
                    return Ok(());
                }
                let (new_identity, reply) =
                    negotiate_hello(registry, extension_id, capability_versions);
                write_extension_reply(stream, &reply)?;
                identity = new_identity;
            }
            ExtensionRequest::RuntimeReady => {
                let result = if runtime_started {
                    Err("the extension runtime has already started on this connection".to_string())
                } else if authentication.expected().is_none() {
                    Err(
                        "RuntimeReady is reserved for a core-spawned authenticated host"
                            .to_string(),
                    )
                } else {
                    authentication.wait_for_runtime_start()
                };
                match result {
                    Ok(()) => {
                        runtime_started = true;
                        write_extension_reply(stream, &ExtensionReply::RuntimeStart)?;
                    }
                    Err(reason) => write_extension_reply(
                        stream,
                        &ExtensionReply::OperationUnavailable {
                            capability: "runtime".to_string(),
                            reason,
                        },
                    )?,
                }
            }
            ExtensionRequest::DomRead => {
                if let Some(reason) =
                    capability_denial_reason(registry, &identity, CAPABILITY_DOM_READ, 1)
                {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_DOM_READ.to_string(),
                            reason,
                        },
                    )?;
                } else {
                    match read_dom(None) {
                        Ok(value) => {
                            write_extension_reply(stream, &ExtensionReply::DomReadResult { value })?
                        }
                        Err(reason) => write_extension_reply(
                            stream,
                            &ExtensionReply::OperationUnavailable {
                                capability: CAPABILITY_DOM_READ.to_string(),
                                reason,
                            },
                        )?,
                    }
                }
            }
            ExtensionRequest::DomReadTab { tab_id } => {
                if let Some(reason) =
                    capability_denial_reason(registry, &identity, CAPABILITY_DOM_READ, 2)
                {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_DOM_READ.to_string(),
                            reason,
                        },
                    )?;
                } else {
                    match read_dom(Some(tab_id)) {
                        Ok(value) => {
                            write_extension_reply(stream, &ExtensionReply::DomReadResult { value })?
                        }
                        Err(reason) => write_extension_reply(
                            stream,
                            &ExtensionReply::OperationUnavailable {
                                capability: CAPABILITY_DOM_READ.to_string(),
                                reason,
                            },
                        )?,
                    }
                }
            }
            ExtensionRequest::DomWrite { value, target } => {
                if let Some(reason) =
                    capability_denial_reason(registry, &identity, CAPABILITY_DOM_WRITE, 1)
                {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_DOM_WRITE.to_string(),
                            reason,
                        },
                    )?;
                } else if target.requires_gatekeeper_review() {
                    match check_extension_action(
                        gatekeeper_socket,
                        &identity.extension_id,
                        CAPABILITY_DOM_WRITE,
                        target.gatekeeper_detail().expect(
                            "only gatekeeper-triggering targets reach extension action review",
                        ),
                    ) {
                        Ok(GatekeeperReply::Cleared) => match write_dom(None, value, &target) {
                            Ok(()) => write_extension_reply(stream, &ExtensionReply::DomWriteAck)?,
                            Err(reason) => write_extension_reply(
                                stream,
                                &ExtensionReply::OperationUnavailable {
                                    capability: CAPABILITY_DOM_WRITE.to_string(),
                                    reason,
                                },
                            )?,
                        },
                        Ok(GatekeeperReply::Rejected { reason, category }) => {
                            write_extension_reply(
                                stream,
                                &ExtensionReply::GatekeeperBlocked {
                                    capability: CAPABILITY_DOM_WRITE.to_string(),
                                    reason,
                                    category,
                                },
                            )?;
                        }
                        Err(reason) => {
                            write_extension_reply(
                                stream,
                                &ExtensionReply::GatekeeperBlocked {
                                    capability: CAPABILITY_DOM_WRITE.to_string(),
                                    reason,
                                    category: "gatekeeper-unavailable".to_string(),
                                },
                            )?;
                        }
                    }
                } else {
                    match write_dom(None, value, &target) {
                        Ok(()) => write_extension_reply(stream, &ExtensionReply::DomWriteAck)?,
                        Err(reason) => write_extension_reply(
                            stream,
                            &ExtensionReply::OperationUnavailable {
                                capability: CAPABILITY_DOM_WRITE.to_string(),
                                reason,
                            },
                        )?,
                    }
                }
            }
            ExtensionRequest::SetTextInputValue {
                tab_id,
                node_id,
                value,
            } => {
                if let Some(reason) =
                    capability_denial_reason(registry, &identity, CAPABILITY_DOM_WRITE, 2)
                {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_DOM_WRITE.to_string(),
                            reason,
                        },
                    )?;
                    continue;
                }
                // The v2 operation has one safe semantic shape: set a
                // text-input value. Review is unconditional, so an extension
                // cannot under-classify a sensitive target through metadata.
                match check_extension_action(
                    gatekeeper_socket,
                    &identity.extension_id,
                    CAPABILITY_DOM_WRITE,
                    "action=set-text-input-value".to_string(),
                ) {
                    Ok(GatekeeperReply::Cleared) => {
                        let target = blueice_ipc::extension::DomWriteTarget::FormInput {
                            input_type: "text".to_string(),
                        };
                        match write_dom(Some((tab_id, node_id)), value, &target) {
                            Ok(()) => write_extension_reply(stream, &ExtensionReply::DomWriteAck)?,
                            Err(reason) => write_extension_reply(
                                stream,
                                &ExtensionReply::OperationUnavailable {
                                    capability: CAPABILITY_DOM_WRITE.to_string(),
                                    reason,
                                },
                            )?,
                        }
                    }
                    Ok(GatekeeperReply::Rejected { reason, category }) => {
                        write_extension_reply(
                            stream,
                            &ExtensionReply::GatekeeperBlocked {
                                capability: CAPABILITY_DOM_WRITE.to_string(),
                                reason,
                                category,
                            },
                        )?;
                    }
                    Err(reason) => {
                        write_extension_reply(
                            stream,
                            &ExtensionReply::GatekeeperBlocked {
                                capability: CAPABILITY_DOM_WRITE.to_string(),
                                reason,
                                category: "gatekeeper-unavailable".to_string(),
                            },
                        )?;
                    }
                }
            }
            ExtensionRequest::SetCheckboxChecked {
                tab_id,
                node_id,
                checked,
            } => {
                if let Some(reason) =
                    capability_denial_reason(registry, &identity, CAPABILITY_DOM_WRITE, 3)
                {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_DOM_WRITE.to_string(),
                            reason,
                        },
                    )?;
                    continue;
                }
                // The v3 operation is a deliberately bounded checkbox state
                // change. The action label and form-control class are owned
                // here, never selected by the extension, so review cannot be
                // weakened by untrusted metadata.
                match check_extension_action(
                    gatekeeper_socket,
                    &identity.extension_id,
                    CAPABILITY_DOM_WRITE,
                    "action=set-checkbox-checked".to_string(),
                ) {
                    Ok(GatekeeperReply::Cleared) => {
                        let target = blueice_ipc::extension::DomWriteTarget::FormInput {
                            input_type: "checkbox".to_string(),
                        };
                        let checked = if checked { "true" } else { "false" }.to_string();
                        match write_dom(Some((tab_id, node_id)), checked, &target) {
                            Ok(()) => write_extension_reply(stream, &ExtensionReply::DomWriteAck)?,
                            Err(reason) => write_extension_reply(
                                stream,
                                &ExtensionReply::OperationUnavailable {
                                    capability: CAPABILITY_DOM_WRITE.to_string(),
                                    reason,
                                },
                            )?,
                        }
                    }
                    Ok(GatekeeperReply::Rejected { reason, category }) => {
                        write_extension_reply(
                            stream,
                            &ExtensionReply::GatekeeperBlocked {
                                capability: CAPABILITY_DOM_WRITE.to_string(),
                                reason,
                                category,
                            },
                        )?;
                    }
                    Err(reason) => {
                        write_extension_reply(
                            stream,
                            &ExtensionReply::GatekeeperBlocked {
                                capability: CAPABILITY_DOM_WRITE.to_string(),
                                reason,
                                category: "gatekeeper-unavailable".to_string(),
                            },
                        )?;
                    }
                }
            }
            ExtensionRequest::NetworkIntercept => {
                if let Some(reason) =
                    capability_denial_reason(registry, &identity, CAPABILITY_NETWORK_INTERCEPT, 1)
                {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                            reason,
                        },
                    )?;
                } else {
                    // This protocol's minimal slice deliberately does not
                    // carry an extension-defined interception rule yet.
                    // Fixed metadata proves the review boundary without
                    // opening an unbounded extension-to-reviewer text path.
                    match check_extension_action(
                        gatekeeper_socket,
                        &identity.extension_id,
                        CAPABILITY_NETWORK_INTERCEPT,
                        "action=register-intercept".to_string(),
                    ) {
                        Ok(GatekeeperReply::Cleared) => match register_network_intercept() {
                            Ok(()) => {
                                write_extension_reply(stream, &ExtensionReply::NetworkInterceptAck)?
                            }
                            Err(reason) => write_extension_reply(
                                stream,
                                &ExtensionReply::OperationUnavailable {
                                    capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                                    reason,
                                },
                            )?,
                        },
                        Ok(GatekeeperReply::Rejected { reason, category }) => {
                            write_extension_reply(
                                stream,
                                &ExtensionReply::GatekeeperBlocked {
                                    capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                                    reason,
                                    category,
                                },
                            )?;
                        }
                        Err(reason) => {
                            write_extension_reply(
                                stream,
                                &ExtensionReply::GatekeeperBlocked {
                                    capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                                    reason,
                                    category: "gatekeeper-unavailable".to_string(),
                                },
                            )?;
                        }
                    }
                }
            }
        }
    }
}

/// Turns one wire handshake into its server-side identity claim only if it is
/// acceptable for this connection mode. The plain hello remains valid for an
/// explicitly unauthenticated development server, while a core-spawned host
/// must prove the secret on every identity-changing hello.
fn authenticated_hello(
    expected_authentication: Option<&str>,
    request: ExtensionRequest,
) -> Option<(String, BTreeMap<String, u32>)> {
    match request {
        ExtensionRequest::Hello {
            extension_id,
            capability_versions,
        } if expected_authentication.is_none() => Some((extension_id, capability_versions)),
        ExtensionRequest::HelloAuthenticated {
            extension_id,
            capability_versions,
            authentication,
        } if expected_authentication.is_none_or(|expected| {
            constant_time_authentication_matches(expected, &authentication)
        }) =>
        {
            Some((extension_id, capability_versions))
        }
        _ => None,
    }
}

/// Compares a supplied credential without exiting early on its contents. The
/// generated core credential has a fixed ASCII length, and this helper also
/// mixes a length mismatch into the result instead of indexing beyond the
/// supplied buffer.
fn constant_time_authentication_matches(expected: &str, supplied: &str) -> bool {
    let expected = expected.as_bytes();
    let supplied = supplied.as_bytes();
    let mut difference = u8::from(expected.len() != supplied.len());
    for (index, expected_byte) in expected.iter().enumerate() {
        difference |= expected_byte ^ supplied.get(index).copied().unwrap_or_default();
    }
    difference == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_ipc::extension::{read_extension_reply, write_extension_request, DomWriteTarget};
    use blueice_ipc::gatekeeper::{read_gatekeeper_request, write_gatekeeper_reply};
    use std::collections::BTreeMap;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::thread;

    fn unique_gatekeeper_socket(_label: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};

        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        // The temp-dir prefix can already be long on Darwin, whose
        // Unix-domain socket path limit is small. The counter and pid
        // make this concise leaf sufficient for independent tests.
        blueice_ipc::local_socket::default_socket_dir()
            .join(format!("extg-{}-{n}", std::process::id()))
    }

    fn start_gatekeeper(
        label: &str,
        reply: GatekeeperReply,
    ) -> (PathBuf, thread::JoinHandle<GatekeeperRequest>) {
        let socket = unique_gatekeeper_socket(label);
        let _ = std::fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket).unwrap();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_gatekeeper_request(&mut stream).unwrap();
            write_gatekeeper_reply(&mut stream, &reply).unwrap();
            request
        });
        (socket, handle)
    }

    fn registry_with_dom_write_granted() -> ExtensionRegistry {
        let mut registry = ExtensionRegistry::minimal_slice();
        registry.grant(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_WRITE);
        registry
    }

    fn registry_with_network_intercept_granted() -> ExtensionRegistry {
        let mut registry = ExtensionRegistry::minimal_slice();
        registry.grant(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_NETWORK_INTERCEPT);
        registry
    }

    fn hello(extension_id: &str) -> ExtensionRequest {
        hello_with_capabilities(
            extension_id,
            [(CAPABILITY_DOM_READ, 1), (CAPABILITY_DOM_WRITE, 1)],
        )
    }

    fn hello_with_capabilities(
        extension_id: &str,
        capabilities: impl IntoIterator<Item = (&'static str, u32)>,
    ) -> ExtensionRequest {
        ExtensionRequest::Hello {
            extension_id: extension_id.to_string(),
            capability_versions: capabilities
                .into_iter()
                .map(|(capability, version)| (capability.to_string(), version))
                .collect(),
        }
    }

    fn empty_hello_ack() -> ExtensionReply {
        ExtensionReply::HelloAck {
            unsupported_capabilities: BTreeMap::new(),
        }
    }

    #[test]
    fn extension_registry_minimal_slice_grants_dom_read_but_not_dom_write() {
        let registry = ExtensionRegistry::minimal_slice();
        assert!(registry.has_capability(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ));
        assert!(!registry.has_capability(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_WRITE));
        assert!(!registry.has_capability(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_NETWORK_INTERCEPT));
    }

    #[test]
    fn extension_registry_reports_no_capabilities_for_an_unknown_extension_id() {
        let registry = ExtensionRegistry::minimal_slice();
        assert!(!registry.has_capability("some-other-extension", CAPABILITY_DOM_READ));
        assert!(!registry.has_capability("some-other-extension", CAPABILITY_DOM_WRITE));
    }

    #[test]
    fn extension_registry_new_grants_nothing_until_granted() {
        let mut registry = ExtensionRegistry::new();
        assert!(!registry.has_capability(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ));
        registry.grant(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ);
        assert!(registry.has_capability(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ));
    }

    #[test]
    fn capability_version_window_has_inclusive_validated_bounds() {
        let window = CapabilityVersionWindow::new(1, 2).unwrap();
        assert!(window.contains(1));
        assert!(window.contains(2));
        assert!(!window.contains(0));
        assert!(!window.contains(3));
        assert!(CapabilityVersionWindow::new(2, 1).is_none());
    }

    #[test]
    fn a_hello_reports_only_unsupported_capabilities_and_keeps_compatible_ones_usable() {
        let registry = ExtensionRegistry::minimal_slice();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || handle_extension_connection(&registry, &mut server));

        write_extension_request(
            &mut client,
            &hello_with_capabilities(
                MINIMAL_SLICE_EXTENSION_ID,
                [(CAPABILITY_DOM_READ, 1), ("future:capability", 1)],
            ),
        )
        .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::HelloAck {
                unsupported_capabilities: BTreeMap::from([(
                    "future:capability".to_string(),
                    UnsupportedCapabilityVersion::UnknownCapability,
                )]),
            }
        );

        // An independently unsupported future capability must not end a
        // connection that successfully negotiated dom:read.
        write_extension_request(&mut client, &ExtensionRequest::DomRead).unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::DomReadResult {
                value: PLACEHOLDER_DOM_READ_VALUE.to_string()
            }
        );

        drop(client);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn an_incompatible_version_is_reported_and_cannot_be_used_after_handshake() {
        let registry = ExtensionRegistry::minimal_slice();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || handle_extension_connection(&registry, &mut server));

        write_extension_request(
            &mut client,
            &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_READ, 3)]),
        )
        .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::HelloAck {
                unsupported_capabilities: BTreeMap::from([(
                    CAPABILITY_DOM_READ.to_string(),
                    UnsupportedCapabilityVersion::OutsideSupportedRange {
                        min_inclusive: 1,
                        max_inclusive: 2
                    },
                )]),
            }
        );

        write_extension_request(&mut client, &ExtensionRequest::DomRead).unwrap();
        match read_extension_reply(&mut client).unwrap() {
            ExtensionReply::CapabilityDenied { capability, reason } => {
                assert_eq!(capability, CAPABILITY_DOM_READ);
                assert!(reason.contains("did not negotiate a supported version"));
            }
            other => panic!("expected CapabilityDenied, got {other:?}"),
        }

        drop(client);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn v2_tab_reads_are_denied_after_a_v1_handshake_but_v1_reads_keep_working() {
        let registry = ExtensionRegistry::minimal_slice();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || handle_extension_connection(&registry, &mut server));

        write_extension_request(
            &mut client,
            &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_READ, 1)]),
        )
        .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            empty_hello_ack()
        );
        write_extension_request(&mut client, &ExtensionRequest::DomReadTab { tab_id: 2 }).unwrap();
        match read_extension_reply(&mut client).unwrap() {
            ExtensionReply::CapabilityDenied { capability, reason } => {
                assert_eq!(capability, CAPABILITY_DOM_READ);
                assert!(reason.contains("requires version 2"));
            }
            other => panic!("expected a v2 version denial, got {other:?}"),
        }
        write_extension_request(&mut client, &ExtensionRequest::DomRead).unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::DomReadResult {
                value: PLACEHOLDER_DOM_READ_VALUE.to_string()
            }
        );

        drop(client);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn v2_text_input_write_is_reviewed_and_delegated_with_its_explicit_ids() {
        let registry = registry_with_dom_write_granted();
        let (gatekeeper_socket, gatekeeper) =
            start_gatekeeper("clear-v2-text-input", GatekeeperReply::Cleared);
        let (seen_tx, seen_rx) = std::sync::mpsc::channel();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let socket_for_handler = gatekeeper_socket.clone();
        let handle = thread::spawn(move || {
            handle_extension_connection_with_actions(
                &registry,
                &socket_for_handler,
                &mut server,
                |_| Ok("unused in this test".to_string()),
                move |target, value, _| {
                    seen_tx.send((target, value)).unwrap();
                    Ok(())
                },
                || Ok(()),
            )
        });

        write_extension_request(
            &mut client,
            &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_WRITE, 2)]),
        )
        .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            empty_hello_ack()
        );
        write_extension_request(
            &mut client,
            &ExtensionRequest::SetTextInputValue {
                tab_id: 7,
                node_id: 11,
                value: "core-owned value".to_string(),
            },
        )
        .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::DomWriteAck
        );
        assert_eq!(
            seen_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            (Some((7, 11)), "core-owned value".to_string())
        );

        drop(client);
        handle.join().unwrap().unwrap();
        assert_eq!(
            gatekeeper.join().unwrap(),
            GatekeeperRequest::CheckExtensionAction {
                extension_id: MINIMAL_SLICE_EXTENSION_ID.to_string(),
                capability: CAPABILITY_DOM_WRITE.to_string(),
                detail: "action=set-text-input-value".to_string(),
            }
        );
        let _ = std::fs::remove_file(gatekeeper_socket);
    }

    #[test]
    fn v2_text_input_write_a_gatekeeper_rejects_never_reaches_the_delegate() {
        let registry = registry_with_dom_write_granted();
        let (gatekeeper_socket, gatekeeper) = start_gatekeeper(
            "reject-v2-text-input",
            GatekeeperReply::Rejected {
                reason: "external input mutation needs confirmation".to_string(),
                category: "sensitive-extension-action".to_string(),
            },
        );
        let delegated = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let delegated_for_handler = std::sync::Arc::clone(&delegated);
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let socket_for_handler = gatekeeper_socket.clone();
        let handle = thread::spawn(move || {
            handle_extension_connection_with_actions(
                &registry,
                &socket_for_handler,
                &mut server,
                |_| Ok("unused in this test".to_string()),
                move |_, _, _| {
                    delegated_for_handler.store(true, std::sync::atomic::Ordering::SeqCst);
                    Ok(())
                },
                || Ok(()),
            )
        });

        write_extension_request(
            &mut client,
            &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_WRITE, 2)]),
        )
        .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            empty_hello_ack()
        );
        write_extension_request(
            &mut client,
            &ExtensionRequest::SetTextInputValue {
                tab_id: 1,
                node_id: 2,
                value: "must not reach core".to_string(),
            },
        )
        .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::GatekeeperBlocked {
                capability: CAPABILITY_DOM_WRITE.to_string(),
                reason: "external input mutation needs confirmation".to_string(),
                category: "sensitive-extension-action".to_string(),
            }
        );
        assert!(!delegated.load(std::sync::atomic::Ordering::SeqCst));

        drop(client);
        handle.join().unwrap().unwrap();
        assert_eq!(
            gatekeeper.join().unwrap(),
            GatekeeperRequest::CheckExtensionAction {
                extension_id: MINIMAL_SLICE_EXTENSION_ID.to_string(),
                capability: CAPABILITY_DOM_WRITE.to_string(),
                detail: "action=set-text-input-value".to_string(),
            }
        );
        let _ = std::fs::remove_file(gatekeeper_socket);
    }

    #[test]
    fn v3_checkbox_write_is_reviewed_and_delegated_with_explicit_ids() {
        let registry = registry_with_dom_write_granted();
        let (gatekeeper_socket, gatekeeper) =
            start_gatekeeper("clear-v3-checkbox", GatekeeperReply::Cleared);
        let (seen_tx, seen_rx) = std::sync::mpsc::channel();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let socket_for_handler = gatekeeper_socket.clone();
        let handle = thread::spawn(move || {
            handle_extension_connection_with_actions(
                &registry,
                &socket_for_handler,
                &mut server,
                |_| Ok("unused in this test".to_string()),
                move |target, value, write_target| {
                    seen_tx.send((target, value, write_target.clone())).unwrap();
                    Ok(())
                },
                || Ok(()),
            )
        });

        write_extension_request(
            &mut client,
            &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_WRITE, 3)]),
        )
        .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            empty_hello_ack()
        );
        write_extension_request(
            &mut client,
            &ExtensionRequest::SetCheckboxChecked {
                tab_id: 7,
                node_id: 12,
                checked: true,
            },
        )
        .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::DomWriteAck
        );
        assert_eq!(
            seen_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            (
                Some((7, 12)),
                "true".to_string(),
                DomWriteTarget::FormInput {
                    input_type: "checkbox".to_string()
                }
            )
        );

        drop(client);
        handle.join().unwrap().unwrap();
        assert_eq!(
            gatekeeper.join().unwrap(),
            GatekeeperRequest::CheckExtensionAction {
                extension_id: MINIMAL_SLICE_EXTENSION_ID.to_string(),
                capability: CAPABILITY_DOM_WRITE.to_string(),
                detail: "action=set-checkbox-checked".to_string(),
            }
        );
        let _ = std::fs::remove_file(gatekeeper_socket);
    }

    #[test]
    fn v3_checkbox_write_is_denied_after_a_v2_handshake_before_review_or_delegate() {
        let registry = registry_with_dom_write_granted();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || {
            handle_extension_connection_with_actions(
                &registry,
                Path::new("/not-reached-for-v2-checkbox-version-denial.sock"),
                &mut server,
                |_| Ok("unused in this test".to_string()),
                |_, _, _| panic!("a v2 connection must not delegate a v3 request"),
                || Ok(()),
            )
        });

        write_extension_request(
            &mut client,
            &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_WRITE, 2)]),
        )
        .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            empty_hello_ack()
        );
        write_extension_request(
            &mut client,
            &ExtensionRequest::SetCheckboxChecked {
                tab_id: 1,
                node_id: 2,
                checked: false,
            },
        )
        .unwrap();
        match read_extension_reply(&mut client).unwrap() {
            ExtensionReply::CapabilityDenied { capability, reason } => {
                assert_eq!(capability, CAPABILITY_DOM_WRITE);
                assert!(reason.contains("requires version 3"));
            }
            other => panic!("expected a v3 version denial, got {other:?}"),
        }

        drop(client);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn a_granted_capability_must_still_be_declared_and_version_negotiated() {
        let registry = ExtensionRegistry::minimal_slice();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || handle_extension_connection(&registry, &mut server));

        write_extension_request(
            &mut client,
            &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_WRITE, 1)]),
        )
        .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            empty_hello_ack()
        );

        write_extension_request(&mut client, &ExtensionRequest::DomRead).unwrap();
        match read_extension_reply(&mut client).unwrap() {
            ExtensionReply::CapabilityDenied { capability, reason } => {
                assert_eq!(capability, CAPABILITY_DOM_READ);
                assert!(reason.contains("did not negotiate a supported version"));
            }
            other => panic!("expected CapabilityDenied, got {other:?}"),
        }

        drop(client);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn a_granted_dom_read_succeeds_after_handshake() {
        let registry = ExtensionRegistry::minimal_slice();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || handle_extension_connection(&registry, &mut server));

        write_extension_request(&mut client, &hello(MINIMAL_SLICE_EXTENSION_ID)).unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            empty_hello_ack()
        );

        write_extension_request(&mut client, &ExtensionRequest::DomRead).unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::DomReadResult {
                value: PLACEHOLDER_DOM_READ_VALUE.to_string()
            }
        );

        drop(client);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn a_core_action_delegate_failure_is_reported_instead_of_acknowledged() {
        let registry = ExtensionRegistry::minimal_slice();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || {
            handle_extension_connection_with_actions(
                &registry,
                Path::new("/not-used-by-dom-read"),
                &mut server,
                |_| Err("the core session has ended".to_string()),
                |_, _, _| Ok(()),
                || Ok(()),
            )
        });

        write_extension_request(&mut client, &hello(MINIMAL_SLICE_EXTENSION_ID)).unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            empty_hello_ack()
        );
        write_extension_request(&mut client, &ExtensionRequest::DomRead).unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::OperationUnavailable {
                capability: CAPABILITY_DOM_READ.to_string(),
                reason: "the core session has ended".to_string(),
            }
        );

        drop(client);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn an_ungranted_dom_write_is_denied_with_the_capability_and_a_reason() {
        let registry = ExtensionRegistry::minimal_slice();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || handle_extension_connection(&registry, &mut server));

        write_extension_request(&mut client, &hello(MINIMAL_SLICE_EXTENSION_ID)).unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            empty_hello_ack()
        );

        write_extension_request(
            &mut client,
            &ExtensionRequest::DomWrite {
                value: "hijacked".to_string(),
                target: DomWriteTarget::Document,
            },
        )
        .unwrap();
        match read_extension_reply(&mut client).unwrap() {
            ExtensionReply::CapabilityDenied { capability, reason } => {
                assert_eq!(capability, CAPABILITY_DOM_WRITE);
                assert!(!reason.is_empty());
            }
            other => panic!("expected CapabilityDenied, got {other:?}"),
        }

        drop(client);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn a_form_input_write_is_reviewed_and_a_gatekeeper_rejection_blocks_it() {
        let registry = registry_with_dom_write_granted();
        let (gatekeeper_socket, gatekeeper) = start_gatekeeper(
            "reject-form-input",
            GatekeeperReply::Rejected {
                reason: "credential-shaped field requires confirmation".to_string(),
                category: "sensitive-extension-action".to_string(),
            },
        );
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let socket_for_handler = gatekeeper_socket.clone();
        let handle = thread::spawn(move || {
            handle_extension_connection_with_gatekeeper(&registry, &socket_for_handler, &mut server)
        });

        write_extension_request(&mut client, &hello(MINIMAL_SLICE_EXTENSION_ID)).unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            empty_hello_ack()
        );
        write_extension_request(
            &mut client,
            &ExtensionRequest::DomWrite {
                value: "a secret the host must not inspect".to_string(),
                target: DomWriteTarget::FormInput {
                    input_type: "password".to_string(),
                },
            },
        )
        .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::GatekeeperBlocked {
                capability: CAPABILITY_DOM_WRITE.to_string(),
                reason: "credential-shaped field requires confirmation".to_string(),
                category: "sensitive-extension-action".to_string(),
            }
        );

        drop(client);
        handle.join().unwrap().unwrap();
        assert_eq!(
            gatekeeper.join().unwrap(),
            GatekeeperRequest::CheckExtensionAction {
                extension_id: MINIMAL_SLICE_EXTENSION_ID.to_string(),
                capability: CAPABILITY_DOM_WRITE.to_string(),
                detail: "target=form-input; input_type=password".to_string(),
            }
        );
        let _ = std::fs::remove_file(gatekeeper_socket);
    }

    #[test]
    fn a_generic_dom_write_skips_review_but_a_missing_reviewer_blocks_network_causing_writes() {
        let registry = registry_with_dom_write_granted();
        let unavailable_socket = unique_gatekeeper_socket("unavailable");
        let _ = std::fs::remove_file(&unavailable_socket);
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let socket_for_handler = unavailable_socket.clone();
        let handle = thread::spawn(move || {
            handle_extension_connection_with_gatekeeper(&registry, &socket_for_handler, &mut server)
        });

        write_extension_request(&mut client, &hello(MINIMAL_SLICE_EXTENSION_ID)).unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            empty_hello_ack()
        );

        // A non-triggering document write stays local and does not need
        // a live gatekeeper connection.
        write_extension_request(
            &mut client,
            &ExtensionRequest::DomWrite {
                value: "cosmetic text".to_string(),
                target: DomWriteTarget::Document,
            },
        )
        .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::DomWriteAck
        );

        write_extension_request(
            &mut client,
            &ExtensionRequest::DomWrite {
                value: "submit".to_string(),
                target: DomWriteTarget::NetworkCausing {
                    action: "form-submit".to_string(),
                },
            },
        )
        .unwrap();
        match read_extension_reply(&mut client).unwrap() {
            ExtensionReply::GatekeeperBlocked {
                capability,
                reason,
                category,
            } => {
                assert_eq!(capability, CAPABILITY_DOM_WRITE);
                assert_eq!(category, "gatekeeper-unavailable");
                assert!(reason.contains("could not connect"));
            }
            other => panic!("expected fail-closed GatekeeperBlocked, got {other:?}"),
        }

        drop(client);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn a_network_intercept_registration_is_reviewed_before_it_is_acknowledged() {
        let registry = registry_with_network_intercept_granted();
        let (gatekeeper_socket, gatekeeper) =
            start_gatekeeper("clear-network-intercept", GatekeeperReply::Cleared);
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let socket_for_handler = gatekeeper_socket.clone();
        let handle = thread::spawn(move || {
            handle_extension_connection_with_gatekeeper(&registry, &socket_for_handler, &mut server)
        });

        write_extension_request(
            &mut client,
            &hello_with_capabilities(
                MINIMAL_SLICE_EXTENSION_ID,
                [(CAPABILITY_NETWORK_INTERCEPT, 1)],
            ),
        )
        .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            empty_hello_ack()
        );

        write_extension_request(&mut client, &ExtensionRequest::NetworkIntercept).unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::NetworkInterceptAck
        );

        drop(client);
        handle.join().unwrap().unwrap();
        assert_eq!(
            gatekeeper.join().unwrap(),
            GatekeeperRequest::CheckExtensionAction {
                extension_id: MINIMAL_SLICE_EXTENSION_ID.to_string(),
                capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                detail: "action=register-intercept".to_string(),
            }
        );
        let _ = std::fs::remove_file(gatekeeper_socket);
    }

    #[test]
    fn an_ungranted_network_intercept_never_reaches_an_unavailable_gatekeeper() {
        let registry = ExtensionRegistry::minimal_slice();
        let unavailable_socket = unique_gatekeeper_socket("ungranted-network-intercept");
        let _ = std::fs::remove_file(&unavailable_socket);
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let socket_for_handler = unavailable_socket.clone();
        let handle = thread::spawn(move || {
            handle_extension_connection_with_gatekeeper(&registry, &socket_for_handler, &mut server)
        });

        write_extension_request(
            &mut client,
            &hello_with_capabilities(
                MINIMAL_SLICE_EXTENSION_ID,
                [(CAPABILITY_NETWORK_INTERCEPT, 1)],
            ),
        )
        .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            empty_hello_ack()
        );
        write_extension_request(&mut client, &ExtensionRequest::NetworkIntercept).unwrap();
        match read_extension_reply(&mut client).unwrap() {
            ExtensionReply::CapabilityDenied { capability, reason } => {
                assert_eq!(capability, CAPABILITY_NETWORK_INTERCEPT);
                assert!(reason.contains("not granted"));
            }
            other => panic!("expected CapabilityDenied before gatekeeper review, got {other:?}"),
        }

        drop(client);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn a_denied_request_does_not_end_the_connection_a_later_granted_request_still_works() {
        // Proves denial is a per-request check, not a connection-ending
        // fault -- and, since this minimal slice has no real `Page`
        // state for `DomWrite` to touch, this is the closest available
        // proof that a rejected attempt has no side effect on later
        // requests either.
        let registry = ExtensionRegistry::minimal_slice();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || handle_extension_connection(&registry, &mut server));

        write_extension_request(&mut client, &hello(MINIMAL_SLICE_EXTENSION_ID)).unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            empty_hello_ack()
        );

        write_extension_request(
            &mut client,
            &ExtensionRequest::DomWrite {
                value: "x".to_string(),
                target: DomWriteTarget::Document,
            },
        )
        .unwrap();
        assert!(matches!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::CapabilityDenied { .. }
        ));

        write_extension_request(&mut client, &ExtensionRequest::DomRead).unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::DomReadResult {
                value: PLACEHOLDER_DOM_READ_VALUE.to_string()
            }
        );

        drop(client);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn an_unrecognized_extension_id_gets_capability_denied_not_a_crash_or_a_free_pass() {
        let registry = ExtensionRegistry::minimal_slice();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || handle_extension_connection(&registry, &mut server));

        write_extension_request(&mut client, &hello("never-registered-extension")).unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            empty_hello_ack()
        );

        write_extension_request(&mut client, &ExtensionRequest::DomRead).unwrap();
        match read_extension_reply(&mut client).unwrap() {
            ExtensionReply::CapabilityDenied { capability, .. } => {
                assert_eq!(capability, CAPABILITY_DOM_READ)
            }
            other => panic!("expected CapabilityDenied, got {other:?}"),
        }

        drop(client);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn a_non_hello_first_message_ends_the_connection_without_a_reply() {
        let registry = ExtensionRegistry::minimal_slice();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || handle_extension_connection(&registry, &mut server));

        write_extension_request(&mut client, &ExtensionRequest::DomRead).unwrap();

        handle.join().unwrap().unwrap();
        // No reply was ever written -- reading now must fail (EOF, since
        // the connection-handling thread already exited and dropped its
        // end of the socket), not hang or return a stray `HelloAck`.
        assert!(read_extension_reply(&mut client).is_err());
    }

    #[test]
    fn core_spawned_mode_requires_the_one_time_credential_before_acknowledging() {
        let registry = ExtensionRegistry::minimal_slice();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let expected = "a-core-generated-credential".to_string();
        let handle = thread::spawn(move || {
            handle_extension_connection_with_actions_and_authentication(
                &registry,
                Path::new("/not-used-before-a-dom-action.sock"),
                &mut server,
                ExtensionConnectionAuthentication::required(&expected),
                |_| Ok(PLACEHOLDER_DOM_READ_VALUE.to_string()),
                |_, _, _| Ok(()),
                || Ok(()),
            )
        });

        // A package-derived identity by itself is intentionally not
        // credentials in host-spawned mode.
        write_extension_request(&mut client, &hello(MINIMAL_SLICE_EXTENSION_ID)).unwrap();
        handle.join().unwrap().unwrap();
        assert!(read_extension_reply(&mut client).is_err());
    }

    #[test]
    fn core_spawned_mode_accepts_only_the_authenticated_hello_and_signals_readiness() {
        let registry = ExtensionRegistry::minimal_slice();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let expected = "a-core-generated-credential".to_string();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let handle = thread::spawn(move || {
            handle_extension_connection_with_actions_and_authentication(
                &registry,
                Path::new("/not-used-before-a-dom-action.sock"),
                &mut server,
                ExtensionConnectionAuthentication::required(&expected)
                    .with_ready_notification(ready_tx),
                |_| Ok(PLACEHOLDER_DOM_READ_VALUE.to_string()),
                |_, _, _| Ok(()),
                || Ok(()),
            )
        });

        write_extension_request(
            &mut client,
            &ExtensionRequest::HelloAuthenticated {
                extension_id: MINIMAL_SLICE_EXTENSION_ID.to_string(),
                capability_versions: BTreeMap::from([(CAPABILITY_DOM_READ.to_string(), 1)]),
                authentication: "a-core-generated-credential".to_string(),
            },
        )
        .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            empty_hello_ack()
        );
        ready_rx.recv_timeout(Duration::from_secs(1)).unwrap();

        // A later identity change must also prove the connection credential;
        // accepting a plain repeat Hello would let a previously authenticated
        // stream silently fall back to the bearer-claim protocol.
        write_extension_request(&mut client, &hello(MINIMAL_SLICE_EXTENSION_ID)).unwrap();
        handle.join().unwrap().unwrap();
        assert!(read_extension_reply(&mut client).is_err());
    }

    #[test]
    fn authenticated_runtime_waits_for_core_session_start_before_it_can_run() {
        let registry = ExtensionRegistry::minimal_slice();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let expected = "a-core-generated-runtime-credential".to_string();
        let (runtime_start_tx, runtime_start_rx) = std::sync::mpsc::channel();
        let runtime_start_rx = Arc::new(Mutex::new(runtime_start_rx));
        let handle = thread::spawn(move || {
            handle_extension_connection_with_actions_and_authentication(
                &registry,
                Path::new("/not-used-before-a-dom-action.sock"),
                &mut server,
                ExtensionConnectionAuthentication::required(&expected)
                    .with_runtime_start_receiver(runtime_start_rx),
                |_| Ok(PLACEHOLDER_DOM_READ_VALUE.to_string()),
                |_, _, _| Ok(()),
                || Ok(()),
            )
        });

        write_extension_request(
            &mut client,
            &ExtensionRequest::HelloAuthenticated {
                extension_id: MINIMAL_SLICE_EXTENSION_ID.to_string(),
                capability_versions: BTreeMap::from([(CAPABILITY_DOM_READ.to_string(), 1)]),
                authentication: "a-core-generated-runtime-credential".to_string(),
            },
        )
        .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            empty_hello_ack()
        );
        write_extension_request(&mut client, &ExtensionRequest::RuntimeReady).unwrap();

        // The handler is waiting at this point; only the core session's
        // explicit sender can release the host to execute guest code.
        runtime_start_tx.send(()).unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::RuntimeStart
        );

        drop(client);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn disconnecting_before_sending_anything_ends_the_connection_cleanly() {
        let registry = ExtensionRegistry::minimal_slice();
        let (client, mut server) = UnixStream::pair().unwrap();
        drop(client);
        assert!(handle_extension_connection(&registry, &mut server).is_ok());
    }

    #[test]
    fn a_second_hello_is_answered_again_and_updates_which_identity_is_checked() {
        // Mirrors `run_session`'s own "a repeat Hello is just answered
        // again, not re-gated" handling on the external client protocol
        // -- applied here to a long-lived extension connection that
        // re-identifies mid-connection.
        let registry = ExtensionRegistry::minimal_slice();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || handle_extension_connection(&registry, &mut server));

        write_extension_request(&mut client, &hello("some-other-extension")).unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            empty_hello_ack()
        );

        // Not yet granted anything under this first identity.
        write_extension_request(&mut client, &ExtensionRequest::DomRead).unwrap();
        assert!(matches!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::CapabilityDenied { .. }
        ));

        // Re-identify as the granted extension on the same connection.
        write_extension_request(&mut client, &hello(MINIMAL_SLICE_EXTENSION_ID)).unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            empty_hello_ack()
        );

        write_extension_request(&mut client, &ExtensionRequest::DomRead).unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::DomReadResult {
                value: PLACEHOLDER_DOM_READ_VALUE.to_string()
            }
        );

        drop(client);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn a_malformed_frame_after_handshake_ends_the_connection_rather_than_panicking() {
        let registry = ExtensionRegistry::minimal_slice();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || handle_extension_connection(&registry, &mut server));

        write_extension_request(&mut client, &hello(MINIMAL_SLICE_EXTENSION_ID)).unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            empty_hello_ack()
        );

        // A truncated frame: a length prefix promising more bytes than
        // are ever sent.
        client.write_all(&100u32.to_le_bytes()).unwrap();
        client.write_all(b"short").unwrap();
        drop(client);

        // Must return cleanly (not panic, not hang) -- see this fn's own
        // docs for why a malformed frame is treated the same as a plain
        // disconnect rather than propagated as a distinguishable error.
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn handles_two_connections_in_sequence() {
        // Proves the accept-loop-friendly shape (one call handles
        // exactly one connection, start to end, and returns) that
        // `src/main.rs`'s sequential accept loop depends on.
        for _ in 0..2 {
            let registry = ExtensionRegistry::minimal_slice();
            let (mut client, mut server) = UnixStream::pair().unwrap();
            let handle = thread::spawn(move || handle_extension_connection(&registry, &mut server));
            write_extension_request(&mut client, &hello(MINIMAL_SLICE_EXTENSION_ID)).unwrap();
            assert_eq!(
                read_extension_reply(&mut client).unwrap(),
                empty_hello_ack()
            );
            drop(client);
            handle.join().unwrap().unwrap();
        }
    }
}
