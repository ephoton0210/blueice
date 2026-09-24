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
//! for form-control writes and one connection-scoped exact initial-navigation
//! block rule, but no generic DOM mutation, request callback, redirect, or
//! header/body interception surface.
//!
//! **What's real, what's a placeholder** (mirrors `blueice-ai-
//! gatekeeper`'s own "mechanism real, content stub" scoping): the
//! handshake, the registry lookup, and the allow/deny decision are all
//! real and tested. The standalone binary retains a fixed
//! [`ExtensionReply::DomReadResult`] value, but
//! [`handle_extension_connection_with_actions`] lets `blueice-core`
//! provide core-owned representation reads plus explicit form-control writes
//! and v2/v3 declarative navigation-rule delegates without this crate taking an
//! engine dependency. Generic `DomWrite` and legacy v1 `NetworkIntercept`
//! still lack safe core operation shapes, so they are reported as unavailable
//! rather than acknowledged without an effect.
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

mod durable_storage;
mod manifest;
mod runtime;

pub use durable_storage::default_durable_storage_root;
pub use manifest::{
    load_installed_extension, registry_for_installed_extension, ExtensionManifest,
    InstalledExtension, ManifestCapabilities, ManifestError, MANIFEST_API_VERSION,
};
pub use runtime::{
    execute_installed_extension, execute_installed_extension_for_invocation, RuntimeInvocation,
};

use blueice_ipc::extension::{
    read_extension_request, write_extension_reply, ExtensionReply, ExtensionRequest,
    ExtensionRuntimeEvent, NetworkResponseInfo, NetworkTraceInfo, UnsupportedCapabilityVersion,
};
use blueice_ipc::gatekeeper::{
    default_gatekeeper_socket_path, read_gatekeeper_reply, write_gatekeeper_request,
    GatekeeperReply, GatekeeperRequest,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::{mpsc, Arc, Mutex, RwLock};
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

/// Read-only observation of the final response behind a committed page.
pub const CAPABILITY_NETWORK_OBSERVE: &str = "network:observe";

/// A bounded native-chrome surface, not an HTML/CSS injection privilege.
pub const CAPABILITY_UI_INJECT: &str = "ui:inject";

pub fn validate_toolbar_label(label: &str) -> Result<(), String> {
    if label.is_empty()
        || label.len() > blueice_ipc::extension::MAX_EXTENSION_TOOLBAR_LABEL_BYTES
        || label.trim() != label
        || !label.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b'-' | b'_')
        })
    {
        return Err("toolbar label must be 1–20 ASCII letters, digits, spaces, hyphens, or underscores without outer spaces".to_string());
    }
    Ok(())
}

pub fn validate_popup_text(title: &str, body: &str) -> Result<(), String> {
    validate_toolbar_label(title)?;
    if body.is_empty()
        || body.len() > blueice_ipc::extension::MAX_EXTENSION_POPUP_BODY_BYTES
        || body.trim() != body
        || !body.bytes().all(|byte| (0x20..=0x7e).contains(&byte))
    {
        return Err("popup body must be 1–120 printable ASCII bytes without outer spaces"
            .to_string());
    }
    Ok(())
}

/// A per-extension key-value capability owned by core, never by the guest's
/// ambient filesystem or process. Version 1 is process-lifetime; version 2
/// uses a separate durable bucket under a core-selected private data root.
pub const CAPABILITY_STORAGE: &str = "storage";

/// A storage bucket has a bounded number of key/value pairs even when its
/// values are small, so an extension cannot turn its declared capability into
/// unbounded core memory use.
pub const MAX_STORAGE_ENTRIES_PER_EXTENSION: usize = 128;

/// Includes UTF-8 key and value bytes across one extension identity's bucket.
pub const MAX_STORAGE_BYTES_PER_EXTENSION: usize = 256 * 1024;

/// Core-owned storage isolated by manifest-derived extension identity. V1 is
/// a shared in-memory map with mutex-protected quota checks; v2 is a separate
/// disk bucket with a per-identity OS lock. Neither accepts a guest path.
#[derive(Clone, Default)]
pub struct ExtensionStorage {
    buckets: Arc<Mutex<HashMap<String, BTreeMap<String, String>>>>,
    durable: Option<durable_storage::DurableExtensionStorage>,
}

impl ExtensionStorage {
    /// Adds a separate version-two durable namespace. Version-one operations
    /// keep using only the process-lifetime map above.
    pub fn with_durable_root(mut self, root: std::path::PathBuf) -> Self {
        self.durable = Some(durable_storage::DurableExtensionStorage::new(root));
        self
    }

    pub fn durable_get(&self, extension_id: &str, key: &str) -> Result<Option<String>, String> {
        self.durable
            .as_ref()
            .ok_or_else(|| "durable extension storage is not configured".to_string())?
            .get(extension_id, key)
    }

    pub fn durable_list_keys(&self, extension_id: &str) -> Result<Vec<String>, String> {
        self.durable
            .as_ref()
            .ok_or_else(|| "durable extension storage is not configured".to_string())?
            .list_keys(extension_id)
    }

    pub fn durable_set(&self, extension_id: &str, key: String, value: String) -> Result<(), String> {
        self.durable
            .as_ref()
            .ok_or_else(|| "durable extension storage is not configured".to_string())?
            .set(extension_id, key, value)
    }

    pub fn durable_remove(&self, extension_id: &str, key: &str) -> Result<bool, String> {
        self.durable
            .as_ref()
            .ok_or_else(|| "durable extension storage is not configured".to_string())?
            .remove(extension_id, key)
    }

    /// Reads a value from exactly `extension_id`'s bucket after validating the
    /// bounded identifier syntax shared by all storage operations.
    pub fn get(&self, extension_id: &str, key: &str) -> Result<Option<String>, String> {
        validate_storage_key(key)?;
        let buckets = self
            .buckets
            .lock()
            .map_err(|_| "extension storage state was poisoned".to_string())?;
        Ok(buckets
            .get(extension_id)
            .and_then(|bucket| bucket.get(key))
            .cloned())
    }

    /// Stores one bounded UTF-8 value in the caller's bucket. Replacement is
    /// atomic under the same lock as the aggregate byte/entry quotas.
    pub fn set(&self, extension_id: &str, key: String, value: String) -> Result<(), String> {
        // Preserve v1's no-allocation behavior for malformed inputs: an
        // invalid write must not create an empty identity bucket.
        validate_storage_key(&key)?;
        if value.len() > blueice_ipc::extension::MAX_STORAGE_VALUE_BYTES {
            return Err(format!(
                "storage values cannot exceed {} bytes",
                blueice_ipc::extension::MAX_STORAGE_VALUE_BYTES
            ));
        }
        let mut buckets = self
            .buckets
            .lock()
            .map_err(|_| "extension storage state was poisoned".to_string())?;
        let bucket = buckets.entry(extension_id.to_string()).or_default();
        set_bounded_storage_value(bucket, key, value)
    }

    /// Removes only the identified extension's own key, returning whether it
    /// existed. Empty buckets are discarded so a sequence of absent reads or
    /// removals cannot grow the outer map.
    pub fn remove(&self, extension_id: &str, key: &str) -> Result<bool, String> {
        validate_storage_key(key)?;
        let mut buckets = self
            .buckets
            .lock()
            .map_err(|_| "extension storage state was poisoned".to_string())?;
        let Some(bucket) = buckets.get_mut(extension_id) else {
            return Ok(false);
        };
        let removed = bucket.remove(key).is_some();
        if bucket.is_empty() {
            buckets.remove(extension_id);
        }
        Ok(removed)
    }
}

pub(crate) fn validate_storage_key(key: &str) -> Result<(), String> {
    if key.is_empty() {
        return Err("storage keys must not be empty".to_string());
    }
    if key.len() > blueice_ipc::extension::MAX_STORAGE_KEY_BYTES {
        return Err(format!(
            "storage keys cannot exceed {} bytes",
            blueice_ipc::extension::MAX_STORAGE_KEY_BYTES
        ));
    }
    if !key
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(
            "storage keys may contain only ASCII letters, digits, '.', '_' or '-'".to_string(),
        );
    }
    Ok(())
}

pub(crate) fn bucket_storage_bytes(bucket: &BTreeMap<String, String>) -> Result<usize, String> {
    bucket.iter().try_fold(0_usize, |total, (key, value)| {
        total
            .checked_add(key.len())
            .and_then(|bytes| bytes.checked_add(value.len()))
            .ok_or_else(|| "extension storage size overflowed".to_string())
    })
}

pub(crate) fn set_bounded_storage_value(
    bucket: &mut BTreeMap<String, String>,
    key: String,
    value: String,
) -> Result<(), String> {
    validate_storage_key(&key)?;
    if value.len() > blueice_ipc::extension::MAX_STORAGE_VALUE_BYTES {
        return Err(format!(
            "storage values cannot exceed {} bytes",
            blueice_ipc::extension::MAX_STORAGE_VALUE_BYTES
        ));
    }
    let new_key = !bucket.contains_key(&key);
    if new_key && bucket.len() >= MAX_STORAGE_ENTRIES_PER_EXTENSION {
        return Err(format!(
            "an extension may store at most {MAX_STORAGE_ENTRIES_PER_EXTENSION} keys"
        ));
    }
    let new_key_bytes = if new_key { key.len() } else { 0 };
    let existing_value_bytes = bucket.get(&key).map_or(0, String::len);
    let current_bytes = bucket_storage_bytes(bucket)?;
    let prospective_bytes = current_bytes
        .checked_sub(existing_value_bytes)
        .and_then(|bytes| bytes.checked_add(new_key_bytes))
        .and_then(|bytes| bytes.checked_add(value.len()))
        .ok_or_else(|| "extension storage size overflowed".to_string())?;
    if prospective_bytes > MAX_STORAGE_BYTES_PER_EXTENSION {
        return Err(format!(
            "an extension storage bucket cannot exceed {MAX_STORAGE_BYTES_PER_EXTENSION} bytes"
        ));
    }
    bucket.insert(key, value);
    Ok(())
}

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
    optional_declarations: HashMap<String, HashSet<String>>,
    optional_grants: RwLock<HashMap<String, HashSet<String>>>,
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
            optional_declarations: HashMap::new(),
            optional_grants: RwLock::new(HashMap::new()),
            supported_versions: HashMap::new(),
        }
    }

    /// Registers the complete set of capability API windows this Phase 9 host
    /// understands. Installation grants remain a separate operation, so an
    /// extension cannot turn host support into authority merely by declaring a
    /// capability in its manifest or handshake.
    pub fn with_supported_capabilities() -> Self {
        let mut registry = Self::new();
        let v1_to_v2 = CapabilityVersionWindow::new(1, 2).expect("literal version window is valid");
        let v1_to_v3 = CapabilityVersionWindow::new(1, 3).expect("literal version window is valid");
        let v1_to_v6 = CapabilityVersionWindow::new(1, 6).expect("literal version window is valid");
        let v1_to_v9 = CapabilityVersionWindow::new(1, 9).expect("literal version window is valid");
        registry.register_capability_version_window(CAPABILITY_DOM_READ, v1_to_v2);
        registry.register_capability_version_window(CAPABILITY_DOM_WRITE, v1_to_v9);
        registry.register_capability_version_window(CAPABILITY_NETWORK_INTERCEPT, v1_to_v6);
        registry.register_capability_version_window(CAPABILITY_NETWORK_OBSERVE, v1_to_v2);
        registry.register_capability_version_window(CAPABILITY_UI_INJECT, v1_to_v3);
        registry.register_capability_version_window(CAPABILITY_STORAGE, v1_to_v3);
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

    /// Retains an install-validated optional declaration without granting it.
    /// Only the installed-manifest loader should seed this table. No guest or
    /// ordinary client message can write to it.
    pub(crate) fn declare_optional(
        &mut self,
        extension_id: impl Into<String>,
        capability: impl Into<String>,
    ) {
        self.optional_declarations
            .entry(extension_id.into())
            .or_default()
            .insert(capability.into());
    }

    /// Internal grant transition for a future separately authenticated human
    /// approval channel. This deliberately has crate-only visibility until
    /// that authority exists; merely negotiating or requesting a declared
    /// optional capability cannot call it. Returns whether state changed.
    #[allow(dead_code)] // Wired only after a separately authenticated approval channel exists.
    pub(crate) fn grant_optional(&self, extension_id: &str, capability: &str) -> Result<bool, String> {
        if !self.optional_declarations.get(extension_id)
            .is_some_and(|caps| caps.contains(capability)) {
            return Err(format!("{capability} is not an installed optional declaration for {extension_id}"));
        }
        let mut grants = self.optional_grants.write()
            .map_err(|_| "optional grant state is unavailable".to_string())?;
        Ok(grants.entry(extension_id.to_string()).or_default().insert(capability.to_string()))
    }

    /// Revocation is live even for an already-negotiated connection: every
    /// later operation checks the registry again, not a handshake snapshot.
    #[allow(dead_code)] // Wired only after a separately authenticated approval channel exists.
    pub(crate) fn revoke_optional(&self, extension_id: &str, capability: &str) -> Result<bool, String> {
        if !self.optional_declarations.get(extension_id)
            .is_some_and(|caps| caps.contains(capability)) {
            return Err(format!("{capability} is not an installed optional declaration for {extension_id}"));
        }
        let mut grants = self.optional_grants.write()
            .map_err(|_| "optional grant state is unavailable".to_string())?;
        Ok(grants.get_mut(extension_id).is_some_and(|caps| caps.remove(capability)))
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
            || self.optional_grants.read().ok()
                .and_then(|grants| grants.get(extension_id)
                    .map(|caps| caps.contains(capability)))
                .unwrap_or(false)
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
    runtime_events: Option<Arc<Mutex<mpsc::Receiver<ExtensionRuntimeEvent>>>>,
}

/// Core-owned delegates for one authenticated extension connection. Bundling
/// the protocol effects keeps the long-lived connection handler's
/// authority surface explicit without growing its public argument list every
/// time a new, independently reviewed operation is added.
pub struct ExtensionActionDelegates<R, W, N, B, C> {
    read_dom: R,
    write_dom: W,
    register_network_intercept: N,
    register_network_block_url: B,
    register_network_block_host: Box<dyn FnMut(String) -> Result<(), String> + Send>,
    register_network_block_path_prefix: Box<dyn FnMut(String, String) -> Result<(), String> + Send>,
    register_network_redirect_url: Box<dyn FnMut(String, String) -> Result<(), String> + Send>,
    clear_network_block_urls: C,
    observe_network: Box<dyn FnMut(u64) -> Result<Option<NetworkResponseInfo>, String> + Send>,
    observe_network_trace: Box<dyn FnMut(u64) -> Result<Option<NetworkTraceInfo>, String> + Send>,
    set_toolbar_button: Box<dyn FnMut(String) -> Result<(), String> + Send>,
    clear_toolbar_button: Box<dyn FnMut() -> Result<(), String> + Send>,
    show_popup: Box<dyn FnMut(u64, String, String) -> Result<(), String> + Send>,
    show_popup_action: Box<dyn FnMut(u64, String, String, String) -> Result<(), String> + Send>,
    clear_popup: Box<dyn FnMut() -> Result<(), String> + Send>,
    storage: ExtensionStorage,
}

impl<R, W, N, B, C> ExtensionActionDelegates<R, W, N, B, C> {
    /// Creates the complete delegate bundle for one connection. Each closure
    /// is still invoked only after the handler's normal capability, version,
    /// and (where required) gatekeeper checks.
    pub fn new(
        read_dom: R,
        write_dom: W,
        register_network_intercept: N,
        register_network_block_url: B,
        clear_network_block_urls: C,
    ) -> Self {
        Self {
            read_dom,
            write_dom,
            register_network_intercept,
            register_network_block_url,
            register_network_block_host: Box::new(|_| {
                Err("network:intercept v4 needs a core-backed host rule store".to_string())
            }),
            register_network_block_path_prefix: Box::new(|_, _| {
                Err("network:intercept v5 needs a core-backed path-prefix rule store".to_string())
            }),
            register_network_redirect_url: Box::new(|_, _| {
                Err("network:intercept v6 needs a core-backed redirect rule store".to_string())
            }),
            clear_network_block_urls,
            observe_network: Box::new(|_| {
                Err("network:observe needs a core-backed response reader".to_string())
            }),
            observe_network_trace: Box::new(|_| {
                Err("network:observe v2 needs a core-backed trace reader".to_string())
            }),
            set_toolbar_button: Box::new(|_| {
                Err("ui:inject needs a core-backed native toolbar".to_string())
            }),
            clear_toolbar_button: Box::new(|| {
                Err("ui:inject needs a core-backed native toolbar".to_string())
            }),
            show_popup: Box::new(|_, _, _| {
                Err("ui:inject version 2 needs a core-backed native popup".to_string())
            }),
            show_popup_action: Box::new(|_, _, _, _| {
                Err("ui:inject version 3 needs a core-backed popup action".to_string())
            }),
            clear_popup: Box::new(|| {
                Err("ui:inject version 2 needs a core-backed native popup".to_string())
            }),
            storage: ExtensionStorage::default(),
        }
    }

    /// Replaces the default isolated bucket handle with the core-owned handle
    /// shared by every connection for one extension service. This preserves
    /// per-identity state across reconnects without giving the guest a path,
    /// process handle, or mutable reference to the map.
    pub fn with_storage(mut self, storage: ExtensionStorage) -> Self {
        self.storage = storage;
        self
    }


    /// Binds read-only response observation to core's live tab state.
    pub fn with_network_observer(
        mut self,
        observer: impl FnMut(u64) -> Result<Option<NetworkResponseInfo>, String> + Send + 'static,
    ) -> Self {
        self.observe_network = Box::new(observer);
        self
    }

    /// Binds v2 request/redirect observation to committed core-owned state.
    pub fn with_network_trace_observer(
        mut self,
        observer: impl FnMut(u64) -> Result<Option<NetworkTraceInfo>, String> + Send + 'static,
    ) -> Self {
        self.observe_network_trace = Box::new(observer);
        self
    }

    /// Binds v4 declarative host blocking to core's connection-owned rules.
    pub fn with_network_block_host(
        mut self,
        blocker: impl FnMut(String) -> Result<(), String> + Send + 'static,
    ) -> Self {
        self.register_network_block_host = Box::new(blocker);
        self
    }

    /// Binds v5 literal host/path-prefix blocking to core's owned rule set.
    pub fn with_network_block_path_prefix(
        mut self,
        blocker: impl FnMut(String, String) -> Result<(), String> + Send + 'static,
    ) -> Self {
        self.register_network_block_path_prefix = Box::new(blocker);
        self
    }

    /// Binds v6 exact same-origin navigation rewrites to core's rule store.
    pub fn with_network_redirect_url(
        mut self,
        redirector: impl FnMut(String, String) -> Result<(), String> + Send + 'static,
    ) -> Self {
        self.register_network_redirect_url = Box::new(redirector);
        self
    }

    pub fn with_toolbar_button(
        mut self,
        setter: impl FnMut(String) -> Result<(), String> + Send + 'static,
    ) -> Self {
        self.set_toolbar_button = Box::new(setter);
        self
    }

    /// Removes a connection-owned button when this socket renegotiates.
    pub fn with_toolbar_clearer(
        mut self,
        clearer: impl FnMut() -> Result<(), String> + Send + 'static,
    ) -> Self {
        self.clear_toolbar_button = Box::new(clearer);
        self
    }

    pub fn with_popup(
        mut self,
        show: impl FnMut(u64, String, String) -> Result<(), String> + Send + 'static,
        clear: impl FnMut() -> Result<(), String> + Send + 'static,
    ) -> Self {
        self.show_popup = Box::new(show);
        self.clear_popup = Box::new(clear);
        self
    }

    pub fn with_popup_action(
        mut self,
        show: impl FnMut(u64, String, String, String) -> Result<(), String> + Send + 'static,
    ) -> Self {
        self.show_popup_action = Box::new(show);
        self
    }
}

impl<'a> ExtensionConnectionAuthentication<'a> {
    /// Allows the documented bearer-claim development protocol.
    pub const fn unauthenticated() -> Self {
        Self {
            expected: None,
            authenticated_ready: None,
            runtime_start: None,
            runtime_events: None,
        }
    }

    /// Requires every hello to prove this core-generated credential.
    pub const fn required(expected: &'a str) -> Self {
        Self {
            expected: Some(expected),
            authenticated_ready: None,
            runtime_start: None,
            runtime_events: None,
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

    /// Installs the bounded, core-owned event stream for the authenticated
    /// child. Events are pulled one at a time only after `RuntimeStart`, so a
    /// fresh Wasm invocation has completed before another event can arrive.
    pub fn with_runtime_event_receiver(
        mut self,
        runtime_events: Arc<Mutex<mpsc::Receiver<ExtensionRuntimeEvent>>>,
    ) -> Self {
        self.runtime_events = Some(runtime_events);
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

    /// Returns `Ok(None)` when core has deliberately ended its lifecycle
    /// stream, which is a normal host-shutdown condition rather than a
    /// recoverable extension operation failure.
    fn wait_for_runtime_event(&self) -> Result<Option<ExtensionRuntimeEvent>, String> {
        let receiver = self.runtime_events.as_ref().ok_or_else(|| {
            "the core has no lifecycle event stream for this extension connection".to_string()
        })?;
        let receiver = receiver
            .lock()
            .map_err(|_| "the core lifecycle event stream was poisoned".to_string())?;
        Ok(receiver.recv().ok())
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
    handle_extension_connection_with_actions_and_authentication_and_network_rules(
        registry,
        gatekeeper_socket,
        stream,
        authentication,
        ExtensionActionDelegates::new(
            read_dom,
            write_dom,
            register_network_intercept,
            |_| {
                Err(
                    "network:intercept version 2 needs a core-backed declarative rule handler"
                        .to_string(),
                )
            },
            || {
                Err(
                    "network:intercept version 3 needs a core-backed rule-clear handler"
                        .to_string(),
                )
            },
        ),
    )
}

/// Like [`handle_extension_connection_with_actions_and_authentication`], with
/// separate delegates for the version-2 exact navigation-block registration
/// and version-3 caller-owned rule clear. Keeping both distinct from the
/// legacy v1 acknowledgement preserves its isolated protocol-test behavior
/// while making each core-backed effect explicit.
pub fn handle_extension_connection_with_actions_and_authentication_and_network_rules<
    S,
    R,
    W,
    N,
    B,
    C,
>(
    registry: &ExtensionRegistry,
    gatekeeper_socket: &Path,
    stream: &mut S,
    authentication: ExtensionConnectionAuthentication<'_>,
    delegates: ExtensionActionDelegates<R, W, N, B, C>,
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
    B: FnMut(String) -> Result<(), String>,
    C: FnMut() -> Result<(), String>,
{
    let ExtensionActionDelegates {
        mut read_dom,
        mut write_dom,
        mut register_network_intercept,
        mut register_network_block_url,
        mut register_network_block_host,
        mut register_network_block_path_prefix,
        mut register_network_redirect_url,
        mut clear_network_block_urls,
        mut observe_network,
        mut observe_network_trace,
        mut set_toolbar_button,
        mut clear_toolbar_button,
        mut show_popup,
        mut show_popup_action,
        mut clear_popup,
        storage,
    } = delegates;
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
    let mut toolbar_visible = false;
    let mut popup_visible = false;

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
                if popup_visible {
                    if clear_popup().is_err() {
                        return Ok(());
                    }
                    popup_visible = false;
                }
                if toolbar_visible {
                    if clear_toolbar_button().is_err() {
                        return Ok(()); // fail closed before acknowledging a changed grant
                    }
                    toolbar_visible = false;
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
                if popup_visible {
                    if clear_popup().is_err() {
                        return Ok(());
                    }
                    popup_visible = false;
                }
                if toolbar_visible {
                    if clear_toolbar_button().is_err() {
                        return Ok(());
                    }
                    toolbar_visible = false;
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
            ExtensionRequest::NextRuntimeEvent => {
                let result = if !runtime_started {
                    Err("the extension runtime has not started on this connection".to_string())
                } else if authentication.expected().is_none() {
                    Err(
                        "NextRuntimeEvent is reserved for a core-spawned authenticated host"
                            .to_string(),
                    )
                } else {
                    authentication.wait_for_runtime_event()
                };
                match result {
                    Ok(Some(event)) => {
                        write_extension_reply(stream, &ExtensionReply::RuntimeEvent(event))?
                    }
                    Ok(None) => {
                        write_extension_reply(stream, &ExtensionReply::RuntimeEventStreamClosed)?
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
            ExtensionRequest::ReadNetworkResponse { tab_id } => {
                if let Some(reason) =
                    capability_denial_reason(registry, &identity, CAPABILITY_NETWORK_OBSERVE, 1)
                {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_NETWORK_OBSERVE.to_string(),
                            reason,
                        },
                    )?;
                } else {
                    let reply = match observe_network(tab_id) {
                        Ok(response)
                            if response.as_ref().is_some_and(|value| {
                                serde_json::to_vec(value).is_ok_and(|bytes| {
                                    bytes.len()
                                        > blueice_ipc::extension::MAX_NETWORK_OBSERVATION_BYTES
                                })
                            }) => ExtensionReply::OperationUnavailable {
                            capability: CAPABILITY_NETWORK_OBSERVE.to_string(),
                            reason: "network response metadata exceeds the 4096-byte limit"
                                .to_string(),
                        },
                        Ok(response) => ExtensionReply::NetworkResponseResult { response },
                        Err(reason) => ExtensionReply::OperationUnavailable {
                            capability: CAPABILITY_NETWORK_OBSERVE.to_string(),
                            reason,
                        },
                    };
                    write_extension_reply(stream, &reply)?;
                }
            }
            ExtensionRequest::ReadNetworkTrace { tab_id } => {
                if let Some(reason) =
                    capability_denial_reason(registry, &identity, CAPABILITY_NETWORK_OBSERVE, 2)
                {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_NETWORK_OBSERVE.to_string(),
                            reason,
                        },
                    )?;
                } else {
                    let reply = match observe_network_trace(tab_id) {
                        Ok(trace) if trace.as_ref().is_some_and(|value| {
                            serde_json::to_vec(value).is_ok_and(|bytes| {
                                bytes.len() > blueice_ipc::extension::MAX_NETWORK_TRACE_BYTES
                            })
                        }) => ExtensionReply::OperationUnavailable {
                            capability: CAPABILITY_NETWORK_OBSERVE.to_string(),
                            reason: "network trace metadata exceeds the 32768-byte limit".to_string(),
                        },
                        Ok(trace) => ExtensionReply::NetworkTraceResult { trace },
                        Err(reason) => ExtensionReply::OperationUnavailable {
                            capability: CAPABILITY_NETWORK_OBSERVE.to_string(),
                            reason,
                        },
                    };
                    write_extension_reply(stream, &reply)?;
                }
            }
            ExtensionRequest::SetToolbarButton { label } => {
                if let Some(reason) =
                    capability_denial_reason(registry, &identity, CAPABILITY_UI_INJECT, 1)
                {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_UI_INJECT.to_string(),
                            reason,
                        },
                    )?;
                } else {
                    let result = validate_toolbar_label(&label)
                        .and_then(|()| set_toolbar_button(label));
                    let reply = match result {
                        Ok(()) => {
                            toolbar_visible = true;
                            ExtensionReply::UiInjectAck
                        }
                        Err(reason) => ExtensionReply::OperationUnavailable {
                            capability: CAPABILITY_UI_INJECT.to_string(),
                            reason,
                        },
                    };
                    write_extension_reply(stream, &reply)?;
                }
            }
            ExtensionRequest::ClearToolbarButton => {
                if let Some(reason) =
                    capability_denial_reason(registry, &identity, CAPABILITY_UI_INJECT, 1)
                {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_UI_INJECT.to_string(),
                            reason,
                        },
                    )?;
                } else {
                    let reply = match clear_toolbar_button() {
                        Ok(()) => {
                            toolbar_visible = false;
                            popup_visible = false;
                            ExtensionReply::UiInjectAck
                        }
                        Err(reason) => ExtensionReply::OperationUnavailable {
                            capability: CAPABILITY_UI_INJECT.to_string(),
                            reason,
                        },
                    };
                    write_extension_reply(stream, &reply)?;
                }
            }
            ExtensionRequest::ShowPopup { tab_id, title, body } => {
                if let Some(reason) =
                    capability_denial_reason(registry, &identity, CAPABILITY_UI_INJECT, 2)
                {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_UI_INJECT.to_string(),
                            reason,
                        },
                    )?;
                    continue;
                }
                if let Err(reason) = validate_popup_text(&title, &body) {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::OperationUnavailable {
                            capability: CAPABILITY_UI_INJECT.to_string(),
                            reason,
                        },
                    )?;
                    continue;
                }
                if !toolbar_visible {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::OperationUnavailable {
                            capability: CAPABILITY_UI_INJECT.to_string(),
                            reason: "a native popup requires this connection's toolbar button"
                                .to_string(),
                        },
                    )?;
                    continue;
                }
                // Unlike the short toolbar label, popup prose may influence
                // a human. Review the bounded, validated text itself before
                // publishing; no guest-selected HTML or link target crosses.
                let detail = format!("action=show-native-popup; title={title:?}; body={body:?}");
                let reply = match check_extension_action(
                    gatekeeper_socket,
                    &identity.extension_id,
                    CAPABILITY_UI_INJECT,
                    detail,
                ) {
                    Ok(GatekeeperReply::Cleared) => match show_popup(tab_id, title, body) {
                        Ok(()) => {
                            popup_visible = true;
                            ExtensionReply::UiInjectAck
                        }
                        Err(reason) => ExtensionReply::OperationUnavailable {
                            capability: CAPABILITY_UI_INJECT.to_string(),
                            reason,
                        },
                    },
                    Ok(GatekeeperReply::Rejected { reason, category }) => {
                        ExtensionReply::GatekeeperBlocked {
                            capability: CAPABILITY_UI_INJECT.to_string(),
                            reason,
                            category,
                        }
                    }
                    Err(reason) => ExtensionReply::GatekeeperBlocked {
                        capability: CAPABILITY_UI_INJECT.to_string(),
                        reason,
                        category: "gatekeeper-unavailable".to_string(),
                    },
                };
                write_extension_reply(stream, &reply)?;
            }
            ExtensionRequest::ShowPopupAction { tab_id, title, body, action_label } => {
                if let Some(reason) = capability_denial_reason(registry, &identity, CAPABILITY_UI_INJECT, 3) {
                    write_extension_reply(stream, &ExtensionReply::CapabilityDenied {
                        capability: CAPABILITY_UI_INJECT.to_string(), reason,
                    })?;
                    continue;
                }
                if let Err(reason) = validate_popup_text(&title, &body)
                    .and_then(|()| validate_toolbar_label(&action_label))
                {
                    write_extension_reply(stream, &ExtensionReply::OperationUnavailable {
                        capability: CAPABILITY_UI_INJECT.to_string(), reason,
                    })?;
                    continue;
                }
                if !toolbar_visible {
                    write_extension_reply(stream, &ExtensionReply::OperationUnavailable {
                        capability: CAPABILITY_UI_INJECT.to_string(),
                        reason: "a native popup action requires this connection's toolbar button".to_string(),
                    })?;
                    continue;
                }
                // Every guest-visible word on the interactive surface reaches
                // policy review before a button can appear in browser chrome.
                let detail = format!(
                    "action=show-native-popup; title={title:?}; body={body:?}; action_label={action_label:?}"
                );
                let reply = match check_extension_action(
                    gatekeeper_socket,
                    &identity.extension_id,
                    CAPABILITY_UI_INJECT,
                    detail,
                ) {
                    Ok(GatekeeperReply::Cleared) => {
                        match show_popup_action(tab_id, title, body, action_label) {
                            Ok(()) => {
                                popup_visible = true;
                                ExtensionReply::UiInjectAck
                            }
                            Err(reason) => ExtensionReply::OperationUnavailable {
                                capability: CAPABILITY_UI_INJECT.to_string(), reason,
                            },
                        }
                    }
                    Ok(GatekeeperReply::Rejected { reason, category }) => ExtensionReply::GatekeeperBlocked {
                        capability: CAPABILITY_UI_INJECT.to_string(), reason, category,
                    },
                    Err(reason) => ExtensionReply::GatekeeperBlocked {
                        capability: CAPABILITY_UI_INJECT.to_string(),
                        reason,
                        category: "gatekeeper-unavailable".to_string(),
                    },
                };
                write_extension_reply(stream, &reply)?;
            }
            ExtensionRequest::ClearPopup => {
                if let Some(reason) =
                    capability_denial_reason(registry, &identity, CAPABILITY_UI_INJECT, 2)
                {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_UI_INJECT.to_string(),
                            reason,
                        },
                    )?;
                } else {
                    let reply = match clear_popup() {
                        Ok(()) => {
                            popup_visible = false;
                            ExtensionReply::UiInjectAck
                        }
                        Err(reason) => ExtensionReply::OperationUnavailable {
                            capability: CAPABILITY_UI_INJECT.to_string(),
                            reason,
                        },
                    };
                    write_extension_reply(stream, &reply)?;
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
                } else if matches!(
                    target,
                    blueice_ipc::extension::DomWriteTarget::VisibleTextLeaf
                        | blueice_ipc::extension::DomWriteTarget::VisibleTextContent
                ) {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::OperationUnavailable {
                            capability: CAPABILITY_DOM_WRITE.to_string(),
                            reason: "visible text requires its explicit versioned request and live node ID"
                                .to_string(),
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
                if value.len() > blueice_ipc::extension::MAX_TEXT_WRITE_BYTES {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::OperationUnavailable {
                            capability: CAPABILITY_DOM_WRITE.to_string(),
                            reason: format!(
                                "text-control values cannot exceed {} bytes",
                                blueice_ipc::extension::MAX_TEXT_WRITE_BYTES
                            ),
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
            ExtensionRequest::SetTextareaValue {
                tab_id,
                node_id,
                value,
            } => {
                if let Some(reason) =
                    capability_denial_reason(registry, &identity, CAPABILITY_DOM_WRITE, 4)
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
                if value.len() > blueice_ipc::extension::MAX_TEXT_WRITE_BYTES {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::OperationUnavailable {
                            capability: CAPABILITY_DOM_WRITE.to_string(),
                            reason: format!(
                                "text-control values cannot exceed {} bytes",
                                blueice_ipc::extension::MAX_TEXT_WRITE_BYTES
                            ),
                        },
                    )?;
                    continue;
                }
                // The v4 operation is one reviewed, core-defined textarea
                // write. The extension never controls the action label or
                // target class passed into policy review.
                match check_extension_action(
                    gatekeeper_socket,
                    &identity.extension_id,
                    CAPABILITY_DOM_WRITE,
                    "action=set-textarea-value".to_string(),
                ) {
                    Ok(GatekeeperReply::Cleared) => {
                        let target = blueice_ipc::extension::DomWriteTarget::FormInput {
                            input_type: "textarea".to_string(),
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
            ExtensionRequest::SetRangeInputValue {
                tab_id,
                node_id,
                value,
            } => {
                if let Some(reason) =
                    capability_denial_reason(registry, &identity, CAPABILITY_DOM_WRITE, 7)
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
                // The v7 operation carries only a signed integer. Core owns
                // the target's live type, disabled state, min/max/step, and
                // resulting value; the extension cannot provide a numeric
                // constraint or arbitrary attribute to weaken that boundary.
                match check_extension_action(
                    gatekeeper_socket,
                    &identity.extension_id,
                    CAPABILITY_DOM_WRITE,
                    "action=set-range-input-value".to_string(),
                ) {
                    Ok(GatekeeperReply::Cleared) => {
                        let target = blueice_ipc::extension::DomWriteTarget::FormInput {
                            input_type: "range".to_string(),
                        };
                        match write_dom(Some((tab_id, node_id)), value.to_string(), &target) {
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
            request @ (ExtensionRequest::SetVisibleLeafText { .. }
            | ExtensionRequest::SetVisibleTextContent { .. }) => {
                let (tab_id, node_id, value, required_version, action, target) = match request {
                    ExtensionRequest::SetVisibleLeafText { tab_id, node_id, value } => (
                        tab_id,
                        node_id,
                        value,
                        8,
                        "set-visible-leaf-text",
                        blueice_ipc::extension::DomWriteTarget::VisibleTextLeaf,
                    ),
                    ExtensionRequest::SetVisibleTextContent { tab_id, node_id, value } => (
                        tab_id,
                        node_id,
                        value,
                        9,
                        "set-visible-text-content",
                        blueice_ipc::extension::DomWriteTarget::VisibleTextContent,
                    ),
                    _ => unreachable!("the matched request is a visible-text write"),
                };
                if let Some(reason) = capability_denial_reason(registry, &identity, CAPABILITY_DOM_WRITE, required_version) {
                    write_extension_reply(stream, &ExtensionReply::CapabilityDenied {
                        capability: CAPABILITY_DOM_WRITE.to_string(), reason,
                    })?;
                    continue;
                }
                if value.trim().is_empty()
                    || value.len() > blueice_ipc::extension::MAX_VISIBLE_LEAF_TEXT_BYTES
                    || value.chars().any(|ch| ch.is_control() && ch != '\n' && ch != '\t')
                {
                    write_extension_reply(stream, &ExtensionReply::OperationUnavailable {
                        capability: CAPABILITY_DOM_WRITE.to_string(),
                        reason: "visible text must be nonempty, within its limit, and free of control characters".to_string(),
                    })?;
                    continue;
                }
                // Unlike form values, visible replacement text is itself a
                // public-facing effect. Review the exact bounded payload;
                // core separately verifies the live target and URL.
                let detail = format!("action={action}; text={value}");
                match check_extension_action(gatekeeper_socket, &identity.extension_id, CAPABILITY_DOM_WRITE, detail) {
                    Ok(GatekeeperReply::Cleared) => {
                        match write_dom(Some((tab_id, node_id)), value, &target) {
                            Ok(()) => write_extension_reply(stream, &ExtensionReply::DomWriteAck)?,
                            Err(reason) => write_extension_reply(stream, &ExtensionReply::OperationUnavailable {
                                capability: CAPABILITY_DOM_WRITE.to_string(), reason,
                            })?,
                        }
                    }
                    Ok(GatekeeperReply::Rejected { reason, category }) => {
                        write_extension_reply(stream, &ExtensionReply::GatekeeperBlocked {
                            capability: CAPABILITY_DOM_WRITE.to_string(), reason, category,
                        })?;
                    }
                    Err(reason) => {
                        write_extension_reply(stream, &ExtensionReply::GatekeeperBlocked {
                            capability: CAPABILITY_DOM_WRITE.to_string(), reason,
                            category: "gatekeeper-unavailable".to_string(),
                        })?;
                    }
                }
            }
            ExtensionRequest::SetRadioChecked { tab_id, node_id } => {
                if let Some(reason) =
                    capability_denial_reason(registry, &identity, CAPABILITY_DOM_WRITE, 5)
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
                // Version 5 deliberately represents only a radio selection:
                // group identity and the corresponding unchecks are derived
                // by core from the live document, never from extension input.
                match check_extension_action(
                    gatekeeper_socket,
                    &identity.extension_id,
                    CAPABILITY_DOM_WRITE,
                    "action=set-radio-checked".to_string(),
                ) {
                    Ok(GatekeeperReply::Cleared) => {
                        let target = blueice_ipc::extension::DomWriteTarget::FormInput {
                            input_type: "radio".to_string(),
                        };
                        match write_dom(Some((tab_id, node_id)), "true".to_string(), &target) {
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
            ExtensionRequest::SelectOption { tab_id, node_id } => {
                if let Some(reason) =
                    capability_denial_reason(registry, &identity, CAPABILITY_DOM_WRITE, 6)
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
                // Version 6 deliberately represents only selection of one
                // option. The extension cannot nominate an owning select,
                // clear a particular peer, or classify its own target for
                // review; core derives all of that from live DOM state.
                match check_extension_action(
                    gatekeeper_socket,
                    &identity.extension_id,
                    CAPABILITY_DOM_WRITE,
                    "action=select-option".to_string(),
                ) {
                    Ok(GatekeeperReply::Cleared) => {
                        let target = blueice_ipc::extension::DomWriteTarget::FormInput {
                            input_type: "select".to_string(),
                        };
                        match write_dom(Some((tab_id, node_id)), "true".to_string(), &target) {
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
            ExtensionRequest::RegisterNetworkBlockUrl { url } => {
                if let Some(reason) =
                    capability_denial_reason(registry, &identity, CAPABILITY_NETWORK_INTERCEPT, 2)
                {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                            reason,
                        },
                    )?;
                    continue;
                }
                if url.len() > blueice_ipc::extension::MAX_NETWORK_BLOCK_URL_BYTES {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::OperationUnavailable {
                            capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                            reason: format!(
                                "exact navigation-block URLs cannot exceed {} bytes",
                                blueice_ipc::extension::MAX_NETWORK_BLOCK_URL_BYTES
                            ),
                        },
                    )?;
                    continue;
                }
                // The reviewer receives a fixed operation class rather than
                // the extension-controlled URL. The URL is parsed and
                // canonicalized only in the core-owned rule store.
                match check_extension_action(
                    gatekeeper_socket,
                    &identity.extension_id,
                    CAPABILITY_NETWORK_INTERCEPT,
                    "action=register-exact-navigation-block".to_string(),
                ) {
                    Ok(GatekeeperReply::Cleared) => match register_network_block_url(url) {
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
            ExtensionRequest::RegisterNetworkBlockHost { host } => {
                if let Some(reason) =
                    capability_denial_reason(registry, &identity, CAPABILITY_NETWORK_INTERCEPT, 4)
                {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                            reason,
                        },
                    )?;
                    continue;
                }
                if host.len() > blueice_ipc::extension::MAX_NETWORK_BLOCK_HOST_BYTES {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::OperationUnavailable {
                            capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                            reason: format!(
                                "navigation-block hosts cannot exceed {} bytes",
                                blueice_ipc::extension::MAX_NETWORK_BLOCK_HOST_BYTES
                            ),
                        },
                    )?;
                    continue;
                }
                // The reviewer sees the operation class, never a guest-
                // controlled host. Core independently validates and stores
                // the ASCII host only after the review clears.
                match check_extension_action(
                    gatekeeper_socket,
                    &identity.extension_id,
                    CAPABILITY_NETWORK_INTERCEPT,
                    "action=register-host-navigation-block".to_string(),
                ) {
                    Ok(GatekeeperReply::Cleared) => match register_network_block_host(host) {
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
            ExtensionRequest::RegisterNetworkBlockPathPrefix { host, path_prefix } => {
                if let Some(reason) =
                    capability_denial_reason(registry, &identity, CAPABILITY_NETWORK_INTERCEPT, 5)
                {
                    write_extension_reply(stream, &ExtensionReply::CapabilityDenied {
                        capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                        reason,
                    })?;
                    continue;
                }
                if host.len() > blueice_ipc::extension::MAX_NETWORK_BLOCK_HOST_BYTES
                    || path_prefix.len() > blueice_ipc::extension::MAX_NETWORK_BLOCK_PATH_BYTES
                {
                    write_extension_reply(stream, &ExtensionReply::OperationUnavailable {
                        capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                        reason: "navigation-block host or path prefix exceeds its protocol bound".to_string(),
                    })?;
                    continue;
                }
                // Only the fixed action class reaches the gatekeeper. The
                // guest-controlled host/path are validated again by core,
                // after review and before a rule can become active.
                match check_extension_action(
                    gatekeeper_socket,
                    &identity.extension_id,
                    CAPABILITY_NETWORK_INTERCEPT,
                    "action=register-path-prefix-navigation-block".to_string(),
                ) {
                    Ok(GatekeeperReply::Cleared) => {
                        match register_network_block_path_prefix(host, path_prefix) {
                            Ok(()) => write_extension_reply(stream, &ExtensionReply::NetworkInterceptAck)?,
                            Err(reason) => write_extension_reply(stream, &ExtensionReply::OperationUnavailable {
                                capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                                reason,
                            })?,
                        }
                    }
                    Ok(GatekeeperReply::Rejected { reason, category }) => {
                        write_extension_reply(stream, &ExtensionReply::GatekeeperBlocked {
                            capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                            reason,
                            category,
                        })?;
                    }
                    Err(reason) => {
                        write_extension_reply(stream, &ExtensionReply::GatekeeperBlocked {
                            capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                            reason,
                            category: "gatekeeper-unavailable".to_string(),
                        })?;
                    }
                }
            }
            ExtensionRequest::RegisterNetworkRedirectUrl { source_url, target_url } => {
                if let Some(reason) = capability_denial_reason(
                    registry, &identity, CAPABILITY_NETWORK_INTERCEPT, 6,
                ) {
                    write_extension_reply(stream, &ExtensionReply::CapabilityDenied {
                        capability: CAPABILITY_NETWORK_INTERCEPT.to_string(), reason,
                    })?;
                    continue;
                }
                if source_url.len() > blueice_ipc::extension::MAX_NETWORK_BLOCK_URL_BYTES
                    || target_url.len() > blueice_ipc::extension::MAX_NETWORK_BLOCK_URL_BYTES
                {
                    write_extension_reply(stream, &ExtensionReply::OperationUnavailable {
                        capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                        reason: "navigation redirect URLs exceed the protocol bound".to_string(),
                    })?;
                    continue;
                }
                // Only a fixed action label reaches the reviewer. Core later
                // validates both exact URLs and their shared origin; every
                // actual target still receives normal CheckUrl review.
                match check_extension_action(
                    gatekeeper_socket,
                    &identity.extension_id,
                    CAPABILITY_NETWORK_INTERCEPT,
                    "action=register-same-origin-navigation-redirect".to_string(),
                ) {
                    Ok(GatekeeperReply::Cleared) => {
                        match register_network_redirect_url(source_url, target_url) {
                            Ok(()) => write_extension_reply(stream, &ExtensionReply::NetworkInterceptAck)?,
                            Err(reason) => write_extension_reply(stream, &ExtensionReply::OperationUnavailable {
                                capability: CAPABILITY_NETWORK_INTERCEPT.to_string(), reason,
                            })?,
                        }
                    }
                    Ok(GatekeeperReply::Rejected { reason, category }) => {
                        write_extension_reply(stream, &ExtensionReply::GatekeeperBlocked {
                            capability: CAPABILITY_NETWORK_INTERCEPT.to_string(), reason, category,
                        })?;
                    }
                    Err(reason) => {
                        write_extension_reply(stream, &ExtensionReply::GatekeeperBlocked {
                            capability: CAPABILITY_NETWORK_INTERCEPT.to_string(), reason,
                            category: "gatekeeper-unavailable".to_string(),
                        })?;
                    }
                }
            }
            ExtensionRequest::ClearNetworkBlockUrls => {
                if let Some(reason) =
                    capability_denial_reason(registry, &identity, CAPABILITY_NETWORK_INTERCEPT, 3)
                {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                            reason,
                        },
                    )?;
                    continue;
                }
                // A connection may only clear its own opaque rule bucket.
                // This carries no URL/body/header payload; any later
                // navigation, including one no longer rewritten, still
                // receives the ordinary mandatory URL/content reviews.
                match clear_network_block_urls() {
                    Ok(()) => write_extension_reply(stream, &ExtensionReply::NetworkInterceptAck)?,
                    Err(reason) => write_extension_reply(
                        stream,
                        &ExtensionReply::OperationUnavailable {
                            capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                            reason,
                        },
                    )?,
                }
            }
            ExtensionRequest::StorageGet { key } => {
                if let Some(reason) =
                    capability_denial_reason(registry, &identity, CAPABILITY_STORAGE, 1)
                {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_STORAGE.to_string(),
                            reason,
                        },
                    )?;
                    continue;
                }
                match storage.get(&identity.extension_id, &key) {
                    Ok(value) => {
                        write_extension_reply(stream, &ExtensionReply::StorageGetResult { value })?
                    }
                    Err(reason) => write_extension_reply(
                        stream,
                        &ExtensionReply::OperationUnavailable {
                            capability: CAPABILITY_STORAGE.to_string(),
                            reason,
                        },
                    )?,
                }
            }
            ExtensionRequest::StorageSet { key, value } => {
                if let Some(reason) =
                    capability_denial_reason(registry, &identity, CAPABILITY_STORAGE, 1)
                {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_STORAGE.to_string(),
                            reason,
                        },
                    )?;
                    continue;
                }
                match storage.set(&identity.extension_id, key, value) {
                    Ok(()) => write_extension_reply(stream, &ExtensionReply::StorageSetAck)?,
                    Err(reason) => write_extension_reply(
                        stream,
                        &ExtensionReply::OperationUnavailable {
                            capability: CAPABILITY_STORAGE.to_string(),
                            reason,
                        },
                    )?,
                }
            }
            ExtensionRequest::StorageRemove { key } => {
                if let Some(reason) =
                    capability_denial_reason(registry, &identity, CAPABILITY_STORAGE, 1)
                {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_STORAGE.to_string(),
                            reason,
                        },
                    )?;
                    continue;
                }
                match storage.remove(&identity.extension_id, &key) {
                    Ok(removed) => write_extension_reply(
                        stream,
                        &ExtensionReply::StorageRemoveAck { removed },
                    )?,
                    Err(reason) => write_extension_reply(
                        stream,
                        &ExtensionReply::OperationUnavailable {
                            capability: CAPABILITY_STORAGE.to_string(),
                            reason,
                        },
                    )?,
                }
            }
            ExtensionRequest::DurableStorageGet { key } => {
                if let Some(reason) = capability_denial_reason(registry, &identity, CAPABILITY_STORAGE, 2) {
                    write_extension_reply(stream, &ExtensionReply::CapabilityDenied {
                        capability: CAPABILITY_STORAGE.to_string(), reason,
                    })?;
                    continue;
                }
                match storage.durable_get(&identity.extension_id, &key) {
                    Ok(value) => write_extension_reply(stream, &ExtensionReply::StorageGetResult { value })?,
                    Err(reason) => write_extension_reply(stream, &ExtensionReply::OperationUnavailable {
                        capability: CAPABILITY_STORAGE.to_string(), reason,
                    })?,
                }
            }
            ExtensionRequest::DurableStorageSet { key, value } => {
                if let Some(reason) = capability_denial_reason(registry, &identity, CAPABILITY_STORAGE, 2) {
                    write_extension_reply(stream, &ExtensionReply::CapabilityDenied {
                        capability: CAPABILITY_STORAGE.to_string(), reason,
                    })?;
                    continue;
                }
                match storage.durable_set(&identity.extension_id, key, value) {
                    Ok(()) => write_extension_reply(stream, &ExtensionReply::StorageSetAck)?,
                    Err(reason) => write_extension_reply(stream, &ExtensionReply::OperationUnavailable {
                        capability: CAPABILITY_STORAGE.to_string(), reason,
                    })?,
                }
            }
            ExtensionRequest::DurableStorageRemove { key } => {
                if let Some(reason) = capability_denial_reason(registry, &identity, CAPABILITY_STORAGE, 2) {
                    write_extension_reply(stream, &ExtensionReply::CapabilityDenied {
                        capability: CAPABILITY_STORAGE.to_string(), reason,
                    })?;
                    continue;
                }
                match storage.durable_remove(&identity.extension_id, &key) {
                    Ok(removed) => write_extension_reply(stream, &ExtensionReply::StorageRemoveAck { removed })?,
                    Err(reason) => write_extension_reply(stream, &ExtensionReply::OperationUnavailable {
                        capability: CAPABILITY_STORAGE.to_string(), reason,
                    })?,
                }
            }
            ExtensionRequest::DurableStorageListKeys => {
                if let Some(reason) = capability_denial_reason(registry, &identity, CAPABILITY_STORAGE, 3) {
                    write_extension_reply(stream, &ExtensionReply::CapabilityDenied {
                        capability: CAPABILITY_STORAGE.to_string(), reason,
                    })?;
                    continue;
                }
                match storage.durable_list_keys(&identity.extension_id) {
                    Ok(keys) => write_extension_reply(stream, &ExtensionReply::StorageKeysResult { keys })?,
                    Err(reason) => write_extension_reply(stream, &ExtensionReply::OperationUnavailable {
                        capability: CAPABILITY_STORAGE.to_string(), reason,
                    })?,
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
                    // Legacy v1 deliberately has no extension-defined rule.
                    // Fixed metadata proves its review boundary without
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

    fn registry_with_storage_granted() -> ExtensionRegistry {
        let mut registry = ExtensionRegistry::minimal_slice();
        registry.grant(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_STORAGE);
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
    fn network_observation_requires_its_own_grant_and_negotiated_version() {
        let mut registry = ExtensionRegistry::minimal_slice();
        registry.grant(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_NETWORK_OBSERVE);
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || {
            handle_extension_connection_with_actions_and_authentication_and_network_rules(
                &registry,
                Path::new("/not-used-for-read-only-observation"),
                &mut server,
                ExtensionConnectionAuthentication::unauthenticated(),
                ExtensionActionDelegates::new(
                    |_| Ok(String::new()),
                    unused_write_delegate,
                    || Ok(()),
                    |_| Ok(()),
                    || Ok(()),
                )
                .with_network_observer(|tab_id| {
                    assert_eq!(tab_id, 7);
                    Ok(Some(NetworkResponseInfo {
                        method: "GET".to_string(),
                        final_url: "https://example.test/final".to_string(),
                        status: 200,
                        content_type: Some("text/html".to_string()),
                    }))
                })
                .with_network_trace_observer(|tab_id| {
                    assert_eq!(tab_id, 7);
                    Ok(Some(NetworkTraceInfo {
                        request_url: "https://example.test/start".to_string(),
                        redirects: vec![blueice_ipc::extension::NetworkRedirectInfo {
                            request_url: "https://example.test/start".to_string(),
                            status: 302,
                            target_url: "https://example.test/final".to_string(),
                        }],
                        response: NetworkResponseInfo {
                            method: "GET".to_string(),
                            final_url: "https://example.test/final".to_string(),
                            status: 200,
                            content_type: Some("text/html".to_string()),
                        },
                    }))
                }),
            )
        });

        write_extension_request(
            &mut client,
            &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_NETWORK_OBSERVE, 1)]),
        )
        .unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), empty_hello_ack());
        write_extension_request(&mut client, &ExtensionRequest::ReadNetworkResponse { tab_id: 7 })
            .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::NetworkResponseResult {
                response: Some(NetworkResponseInfo {
                    method: "GET".to_string(),
                    final_url: "https://example.test/final".to_string(),
                    status: 200,
                    content_type: Some("text/html".to_string()),
                })
            }
        );
        write_extension_request(&mut client, &ExtensionRequest::ReadNetworkTrace { tab_id: 7 })
            .unwrap();
        assert!(matches!(read_extension_reply(&mut client).unwrap(),
            ExtensionReply::CapabilityDenied { capability, .. } if capability == CAPABILITY_NETWORK_OBSERVE));

        write_extension_request(
            &mut client,
            &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_READ, 1)]),
        )
        .unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), empty_hello_ack());
        write_extension_request(&mut client, &ExtensionRequest::ReadNetworkResponse { tab_id: 7 })
            .unwrap();
        assert!(matches!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::CapabilityDenied { capability, .. } if capability == CAPABILITY_NETWORK_OBSERVE
        ));
        write_extension_request(
            &mut client,
            &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_NETWORK_OBSERVE, 2)]),
        )
        .unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), empty_hello_ack());
        write_extension_request(&mut client, &ExtensionRequest::ReadNetworkTrace { tab_id: 7 })
            .unwrap();
        assert!(matches!(read_extension_reply(&mut client).unwrap(),
            ExtensionReply::NetworkTraceResult { trace: Some(trace) }
                if trace.request_url == "https://example.test/start"
                    && trace.redirects.len() == 1
                    && trace.redirects[0].status == 302));
        write_extension_request(
            &mut client,
            &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_NETWORK_OBSERVE, 3)]),
        )
        .unwrap();
        assert!(matches!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::HelloAck { unsupported_capabilities }
                if matches!(
                    unsupported_capabilities.get(CAPABILITY_NETWORK_OBSERVE),
                    Some(UnsupportedCapabilityVersion::OutsideSupportedRange {
                        min_inclusive: 1,
                        max_inclusive: 2,
                    })
                )
        ));
        write_extension_request(&mut client, &ExtensionRequest::ReadNetworkTrace { tab_id: 7 })
            .unwrap();
        assert!(matches!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::CapabilityDenied { capability, .. } if capability == CAPABILITY_NETWORK_OBSERVE
        ));
        drop(client);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn native_toolbar_requires_ui_grant_and_validates_before_delegation() {
        let mut registry = ExtensionRegistry::minimal_slice();
        registry.grant(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_UI_INJECT);
        let (seen_tx, seen_rx) = mpsc::channel();
        let (clear_tx, clear_rx) = mpsc::channel();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || {
            handle_extension_connection_with_actions_and_authentication_and_network_rules(
                &registry,
                Path::new("/not-used-for-native-ui"),
                &mut server,
                ExtensionConnectionAuthentication::unauthenticated(),
                ExtensionActionDelegates::new(
                    |_| Ok(String::new()),
                    unused_write_delegate,
                    || Ok(()),
                    |_| Ok(()),
                    || Ok(()),
                )
                .with_toolbar_button(move |label| {
                    seen_tx.send(label).unwrap();
                    Ok(())
                })
                .with_toolbar_clearer(move || {
                    clear_tx.send(()).unwrap();
                    Ok(())
                }),
            )
        });
        write_extension_request(
            &mut client,
            &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_UI_INJECT, 1)]),
        )
        .unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), empty_hello_ack());
        write_extension_request(
            &mut client,
            &ExtensionRequest::SetToolbarButton {
                label: "Bad\nLabel".to_string(),
            },
        )
        .unwrap();
        assert!(matches!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::OperationUnavailable { capability, .. } if capability == CAPABILITY_UI_INJECT
        ));
        assert!(seen_rx.try_recv().is_err());
        write_extension_request(
            &mut client,
            &ExtensionRequest::SetToolbarButton {
                label: "Notes".to_string(),
            },
        )
        .unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), ExtensionReply::UiInjectAck);
        assert_eq!(seen_rx.recv_timeout(Duration::from_secs(1)).unwrap(), "Notes");
        write_extension_request(&mut client, &ExtensionRequest::ClearToolbarButton).unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), ExtensionReply::UiInjectAck);
        clear_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        write_extension_request(
            &mut client,
            &ExtensionRequest::SetToolbarButton {
                label: "Notes".to_string(),
            },
        )
        .unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), ExtensionReply::UiInjectAck);
        assert_eq!(seen_rx.recv_timeout(Duration::from_secs(1)).unwrap(), "Notes");
        write_extension_request(
            &mut client,
            &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_READ, 1)]),
        )
        .unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), empty_hello_ack());
        clear_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        write_extension_request(
            &mut client,
            &ExtensionRequest::SetToolbarButton {
                label: "Denied".to_string(),
            },
        )
        .unwrap();
        assert!(matches!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::CapabilityDenied { capability, .. } if capability == CAPABILITY_UI_INJECT
        ));
        assert!(seen_rx.try_recv().is_err());
        write_extension_request(
            &mut client,
            &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_UI_INJECT, 4)]),
        )
        .unwrap();
        assert!(matches!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::HelloAck { unsupported_capabilities }
                if matches!(
                    unsupported_capabilities.get(CAPABILITY_UI_INJECT),
                    Some(UnsupportedCapabilityVersion::OutsideSupportedRange {
                        min_inclusive: 1,
                        max_inclusive: 3,
                    })
                )
        ));
        assert!(clear_rx.try_recv().is_err());
        write_extension_request(
            &mut client,
            &ExtensionRequest::SetToolbarButton {
                label: "Denied".to_string(),
            },
        )
        .unwrap();
        assert!(matches!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::CapabilityDenied { capability, .. } if capability == CAPABILITY_UI_INJECT
        ));
        drop(client);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn native_popup_requires_v2_toolbar_and_review_of_its_actual_text() {
        let mut registry = ExtensionRegistry::minimal_slice();
        registry.grant(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_UI_INJECT);
        let (gatekeeper, reviewed) = start_gatekeeper("popup-clear", GatekeeperReply::Cleared);
        let (shown_tx, shown_rx) = mpsc::channel();
        let (cleared_tx, cleared_rx) = mpsc::channel();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let gatekeeper_for_host = gatekeeper.clone();
        let handle = thread::spawn(move || {
            handle_extension_connection_with_actions_and_authentication_and_network_rules(
                &registry,
                &gatekeeper_for_host,
                &mut server,
                ExtensionConnectionAuthentication::unauthenticated(),
                ExtensionActionDelegates::new(
                    |_| Ok(String::new()),
                    unused_write_delegate,
                    || Ok(()),
                    |_| Ok(()),
                    || Ok(()),
                )
                .with_toolbar_button(|_| Ok(()))
                .with_toolbar_clearer(|| Ok(()))
                .with_popup(
                    move |tab_id, title, body| {
                        shown_tx.send((tab_id, title, body)).unwrap();
                        Ok(())
                    },
                    move || {
                        cleared_tx.send(()).unwrap();
                        Ok(())
                    },
                ),
            )
        });
        write_extension_request(
            &mut client,
            &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_UI_INJECT, 1)]),
        ).unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), empty_hello_ack());
        write_extension_request(&mut client, &ExtensionRequest::ShowPopup {
            tab_id: 1, title: "Notes".to_string(), body: "Saved locally".to_string(),
        }).unwrap();
        assert!(matches!(read_extension_reply(&mut client).unwrap(), ExtensionReply::CapabilityDenied { .. }));
        write_extension_request(
            &mut client,
            &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_UI_INJECT, 2)]),
        ).unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), empty_hello_ack());
        write_extension_request(&mut client, &ExtensionRequest::ShowPopup {
            tab_id: 1, title: "Notes".to_string(), body: "Saved locally".to_string(),
        }).unwrap();
        assert!(matches!(read_extension_reply(&mut client).unwrap(), ExtensionReply::OperationUnavailable { .. }));
        write_extension_request(&mut client, &ExtensionRequest::SetToolbarButton { label: "Notes".to_string() }).unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), ExtensionReply::UiInjectAck);
        write_extension_request(&mut client, &ExtensionRequest::ShowPopup {
            tab_id: 1, title: "Notes".to_string(), body: "bad\ntext".to_string(),
        }).unwrap();
        assert!(matches!(read_extension_reply(&mut client).unwrap(), ExtensionReply::OperationUnavailable { .. }));
        write_extension_request(&mut client, &ExtensionRequest::ShowPopup {
            tab_id: 1, title: "Notes".to_string(), body: "Saved locally".to_string(),
        }).unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), ExtensionReply::UiInjectAck);
        assert_eq!(shown_rx.recv_timeout(Duration::from_secs(1)).unwrap(), (1, "Notes".to_string(), "Saved locally".to_string()));
        let request = reviewed.join().unwrap();
        assert!(matches!(request, GatekeeperRequest::CheckExtensionAction { capability, detail, .. }
            if capability == CAPABILITY_UI_INJECT && detail.contains("Saved locally")));
        write_extension_request(&mut client, &ExtensionRequest::ClearPopup).unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), ExtensionReply::UiInjectAck);
        cleared_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        drop(client);
        handle.join().unwrap().unwrap();
        let _ = std::fs::remove_file(gatekeeper);
    }

    #[test]
    fn popup_action_requires_v3_and_reviews_the_button_label_before_publication() {
        let mut registry = ExtensionRegistry::minimal_slice();
        registry.grant(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_UI_INJECT);
        let (gatekeeper, reviewed) = start_gatekeeper("popup-action-clear", GatekeeperReply::Cleared);
        let (shown_tx, shown_rx) = mpsc::channel();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let gatekeeper_for_host = gatekeeper.clone();
        let handle = thread::spawn(move || {
            handle_extension_connection_with_actions_and_authentication_and_network_rules(
                &registry,
                &gatekeeper_for_host,
                &mut server,
                ExtensionConnectionAuthentication::unauthenticated(),
                ExtensionActionDelegates::new(
                    |_| Ok(String::new()),
                    unused_write_delegate,
                    || Ok(()),
                    |_| Ok(()),
                    || Ok(()),
                )
                .with_toolbar_button(|_| Ok(()))
                .with_toolbar_clearer(|| Ok(()))
                .with_popup_action(move |tab_id, title, body, label| {
                    shown_tx.send((tab_id, title, body, label)).unwrap();
                    Ok(())
                }),
            )
        });
        write_extension_request(
            &mut client,
            &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_UI_INJECT, 2)]),
        ).unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), empty_hello_ack());
        write_extension_request(&mut client, &ExtensionRequest::SetToolbarButton {
            label: "Notes".to_string(),
        }).unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), ExtensionReply::UiInjectAck);
        let action = || ExtensionRequest::ShowPopupAction {
            tab_id: 1,
            title: "Notes".to_string(),
            body: "Saved locally".to_string(),
            action_label: "Open notes".to_string(),
        };
        write_extension_request(&mut client, &action()).unwrap();
        assert!(matches!(read_extension_reply(&mut client).unwrap(), ExtensionReply::CapabilityDenied { .. }));
        assert!(shown_rx.try_recv().is_err());
        write_extension_request(
            &mut client,
            &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_UI_INJECT, 3)]),
        ).unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), empty_hello_ack());
        write_extension_request(&mut client, &action()).unwrap();
        assert!(matches!(read_extension_reply(&mut client).unwrap(), ExtensionReply::OperationUnavailable { .. }));
        write_extension_request(&mut client, &ExtensionRequest::SetToolbarButton {
            label: "Notes".to_string(),
        }).unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), ExtensionReply::UiInjectAck);
        write_extension_request(&mut client, &ExtensionRequest::ShowPopupAction {
            tab_id: 1,
            title: "Notes".to_string(),
            body: "Saved locally".to_string(),
            action_label: "Bad\nLabel".to_string(),
        }).unwrap();
        assert!(matches!(read_extension_reply(&mut client).unwrap(), ExtensionReply::OperationUnavailable { .. }));
        assert!(shown_rx.try_recv().is_err());
        write_extension_request(&mut client, &action()).unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), ExtensionReply::UiInjectAck);
        assert_eq!(shown_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            (1, "Notes".to_string(), "Saved locally".to_string(), "Open notes".to_string()));
        let request = reviewed.join().unwrap();
        assert!(matches!(request, GatekeeperRequest::CheckExtensionAction { capability, detail, .. }
            if capability == CAPABILITY_UI_INJECT && detail.contains("action=show-native-popup")
                && detail.contains("body=\"Saved locally\"")
                && detail.contains("action_label=\"Open notes\"")));
        drop(client);
        handle.join().unwrap().unwrap();
        let _ = std::fs::remove_file(gatekeeper);
    }

    #[test]
    fn native_popup_rejection_never_calls_the_core_delegate() {
        let mut registry = ExtensionRegistry::minimal_slice();
        registry.grant(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_UI_INJECT);
        let (gatekeeper, reviewed) = start_gatekeeper(
            "popup-rejected",
            GatekeeperReply::Rejected {
                reason: "unsafe popup".to_string(),
                category: "extension-popup-social-engineering".to_string(),
            },
        );
        let (shown_tx, shown_rx) = mpsc::channel();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let gatekeeper_for_host = gatekeeper.clone();
        let handle = thread::spawn(move || {
            handle_extension_connection_with_actions_and_authentication_and_network_rules(
                &registry,
                &gatekeeper_for_host,
                &mut server,
                ExtensionConnectionAuthentication::unauthenticated(),
                ExtensionActionDelegates::new(
                    |_| Ok(String::new()), unused_write_delegate, || Ok(()), |_| Ok(()), || Ok(()),
                )
                .with_toolbar_button(|_| Ok(()))
                .with_popup(
                    move |_, _, _| { shown_tx.send(()).unwrap(); Ok(()) },
                    || Ok(()),
                ),
            )
        });
        write_extension_request(&mut client, &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_UI_INJECT, 2)])).unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), empty_hello_ack());
        write_extension_request(&mut client, &ExtensionRequest::SetToolbarButton { label: "Notes".to_string() }).unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), ExtensionReply::UiInjectAck);
        write_extension_request(&mut client, &ExtensionRequest::ShowPopup { tab_id: 1, title: "Notes".to_string(), body: "Enter your password".to_string() }).unwrap();
        assert!(matches!(read_extension_reply(&mut client).unwrap(), ExtensionReply::GatekeeperBlocked { category, .. } if category == "extension-popup-social-engineering"));
        assert!(shown_rx.try_recv().is_err());
        assert!(matches!(reviewed.join().unwrap(), GatekeeperRequest::CheckExtensionAction { detail, .. } if detail.contains("Enter your password")));
        drop(client);
        handle.join().unwrap().unwrap();
        let _ = std::fs::remove_file(gatekeeper);
    }

    fn unused_write_delegate(
        _: Option<(u64, u64)>,
        _: String,
        _: &DomWriteTarget,
    ) -> Result<(), String> {
        Ok(())
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
    fn optional_grant_and_revocation_take_effect_on_an_existing_negotiated_connection() {
        let mut registry = ExtensionRegistry::with_supported_capabilities();
        registry.declare_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ);
        assert!(registry.grant_optional("other-extension", CAPABILITY_DOM_READ).is_err());
        assert!(registry.grant_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_WRITE).is_err());
        let registry = Arc::new(registry);
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handler_registry = Arc::clone(&registry);
        let handle = thread::spawn(move || {
            handle_extension_connection(&handler_registry, &mut server)
        });
        write_extension_request(&mut client, &hello_with_capabilities(
            MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_READ, 1)],
        )).unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), empty_hello_ack());

        write_extension_request(&mut client, &ExtensionRequest::DomRead).unwrap();
        assert!(matches!(read_extension_reply(&mut client).unwrap(),
            ExtensionReply::CapabilityDenied { capability, .. } if capability == CAPABILITY_DOM_READ));
        assert!(registry.grant_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ).unwrap());
        assert!(!registry.grant_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ).unwrap());
        write_extension_request(&mut client, &ExtensionRequest::DomRead).unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), ExtensionReply::DomReadResult {
            value: PLACEHOLDER_DOM_READ_VALUE.to_string(),
        });

        assert!(registry.revoke_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ).unwrap());
        assert!(!registry.revoke_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ).unwrap());
        write_extension_request(&mut client, &ExtensionRequest::DomRead).unwrap();
        assert!(matches!(read_extension_reply(&mut client).unwrap(),
            ExtensionReply::CapabilityDenied { capability, .. } if capability == CAPABILITY_DOM_READ));
        drop(client);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn extension_storage_is_bounded_and_isolated_by_extension_identity() {
        let storage = ExtensionStorage::default();
        storage
            .set(
                "sha256:first",
                "task-state".to_string(),
                "complete".to_string(),
            )
            .unwrap();
        assert_eq!(
            storage.get("sha256:first", "task-state").unwrap(),
            Some("complete".to_string())
        );
        assert_eq!(storage.get("sha256:second", "task-state").unwrap(), None);
        assert!(storage
            .set(
                "sha256:first",
                "not a valid key".to_string(),
                "x".to_string()
            )
            .is_err());
        assert!(storage
            .set(
                "sha256:first",
                "oversized".to_string(),
                "x".repeat(blueice_ipc::extension::MAX_STORAGE_VALUE_BYTES + 1),
            )
            .is_err());
        assert!(storage.remove("sha256:first", "task-state").unwrap());
        assert!(!storage.remove("sha256:first", "task-state").unwrap());
        assert_eq!(storage.get("sha256:first", "task-state").unwrap(), None);

        for index in 0..MAX_STORAGE_ENTRIES_PER_EXTENSION {
            storage
                .set("sha256:count", format!("key-{index}"), "x".to_string())
                .unwrap();
        }
        assert!(storage
            .set("sha256:count", "one-too-many".to_string(), "x".to_string())
            .is_err());

        let quota_storage = ExtensionStorage::default();
        for index in 0..15 {
            quota_storage
                .set(
                    "sha256:quota",
                    format!("value-{index}"),
                    "x".repeat(blueice_ipc::extension::MAX_STORAGE_VALUE_BYTES),
                )
                .unwrap();
        }
        assert!(quota_storage
            .set(
                "sha256:quota",
                "exceeds-total".to_string(),
                "x".repeat(blueice_ipc::extension::MAX_STORAGE_VALUE_BYTES),
            )
            .is_err());
    }

    #[test]
    fn extension_storage_aggregate_quota_counts_every_new_key_byte() {
        let storage = ExtensionStorage::default();
        let identity = "sha256:key-byte-quota";
        for index in 0..15 {
            storage
                .set(
                    identity,
                    format!("k{index}"),
                    "x".repeat(blueice_ipc::extension::MAX_STORAGE_VALUE_BYTES),
                )
                .unwrap();
        }
        let current_bytes = {
            let buckets = storage.buckets.lock().unwrap();
            bucket_storage_bytes(buckets.get(identity).unwrap()).unwrap()
        };
        let remaining = MAX_STORAGE_BYTES_PER_EXTENSION - current_bytes;
        let long_key = "k".repeat(blueice_ipc::extension::MAX_STORAGE_KEY_BYTES);
        assert!(remaining <= blueice_ipc::extension::MAX_STORAGE_VALUE_BYTES);
        assert!(storage
            .set(identity, long_key.clone(), "x".repeat(remaining))
            .is_err());
        assert_eq!(storage.get(identity, &long_key).unwrap(), None);

        storage
            .set(identity, long_key.clone(), "x".repeat(remaining - long_key.len()))
            .unwrap();
        let buckets = storage.buckets.lock().unwrap();
        assert_eq!(
            bucket_storage_bytes(buckets.get(identity).unwrap()).unwrap(),
            MAX_STORAGE_BYTES_PER_EXTENSION
        );
        drop(buckets);

        let full_value_len = remaining - long_key.len();
        storage
            .set(identity, long_key.clone(), "y".repeat(full_value_len))
            .unwrap();
        assert!(storage
            .set(identity, long_key.clone(), "z".repeat(full_value_len + 1))
            .is_err());
        assert_eq!(
            storage.get(identity, &long_key).unwrap(),
            Some("y".repeat(full_value_len))
        );
    }

    #[test]
    fn granted_storage_v1_sets_reads_and_removes_only_its_handshake_bucket() {
        let registry = registry_with_storage_granted();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || {
            handle_extension_connection_with_gatekeeper(
                &registry,
                Path::new("/not-reached-for-storage-only-operation.sock"),
                &mut server,
            )
        });

        write_extension_request(
            &mut client,
            &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_STORAGE, 1)]),
        )
        .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            empty_hello_ack()
        );
        write_extension_request(
            &mut client,
            &ExtensionRequest::StorageSet {
                key: "task-state".to_string(),
                value: "complete".to_string(),
            },
        )
        .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::StorageSetAck
        );
        write_extension_request(
            &mut client,
            &ExtensionRequest::StorageGet {
                key: "task-state".to_string(),
            },
        )
        .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::StorageGetResult {
                value: Some("complete".to_string()),
            }
        );
        write_extension_request(
            &mut client,
            &ExtensionRequest::StorageRemove {
                key: "task-state".to_string(),
            },
        )
        .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::StorageRemoveAck { removed: true }
        );
        write_extension_request(
            &mut client,
            &ExtensionRequest::StorageGet {
                key: "task-state".to_string(),
            },
        )
        .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::StorageGetResult { value: None }
        );

        drop(client);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn durable_storage_v2_and_v3_require_grants_and_survive_a_new_service() {
        use std::sync::atomic::{AtomicU64, Ordering};

        static NEXT_ROOT: AtomicU64 = AtomicU64::new(1);
        const ID: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        const OTHER_ID: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let root = std::env::temp_dir().join(format!(
            "blueice-storage-v2-test-{}-{}",
            std::process::id(),
            NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
        ));
        let spawn = |storage: ExtensionStorage| {
            let mut registry = ExtensionRegistry::with_supported_capabilities();
            registry.grant(ID, CAPABILITY_STORAGE);
            let (client, mut server) = UnixStream::pair().unwrap();
            let worker = thread::spawn(move || {
                handle_extension_connection_with_actions_and_authentication_and_network_rules(
                    &registry,
                    Path::new("/not-reached-for-storage-only-operation.sock"),
                    &mut server,
                    ExtensionConnectionAuthentication::unauthenticated(),
                    ExtensionActionDelegates::new(
                        |_| Ok(String::new()),
                        unused_write_delegate,
                        || Ok(()),
                        |_| Ok(()),
                        || Ok(()),
                    )
                    .with_storage(storage),
                )
            });
            (client, worker)
        };
        let exchange = |client: &mut UnixStream, request: ExtensionRequest| {
            write_extension_request(client, &request).unwrap();
            read_extension_reply(client).unwrap()
        };

        let (mut client, worker) = spawn(ExtensionStorage::default().with_durable_root(root.clone()));
        assert_eq!(exchange(&mut client, hello_with_capabilities(ID, [(CAPABILITY_STORAGE, 1)])), empty_hello_ack());
        assert!(matches!(
            exchange(&mut client, ExtensionRequest::DurableStorageSet {
                key: "task".into(), value: "persistent".into(),
            }),
            ExtensionReply::CapabilityDenied { capability, .. } if capability == CAPABILITY_STORAGE
        ));
        assert_eq!(exchange(&mut client, ExtensionRequest::StorageSet {
            key: "task".into(), value: "ephemeral".into(),
        }), ExtensionReply::StorageSetAck);
        assert_eq!(exchange(&mut client, hello_with_capabilities(ID, [(CAPABILITY_STORAGE, 2)])), empty_hello_ack());
        assert!(matches!(exchange(&mut client, ExtensionRequest::DurableStorageListKeys),
            ExtensionReply::CapabilityDenied { capability, .. } if capability == CAPABILITY_STORAGE));
        assert_eq!(exchange(&mut client, ExtensionRequest::DurableStorageSet {
            key: "task".into(), value: "persistent".into(),
        }), ExtensionReply::StorageSetAck);
        assert_eq!(exchange(&mut client, ExtensionRequest::DurableStorageSet {
            key: "alpha".into(), value: "other".into(),
        }), ExtensionReply::StorageSetAck);
        assert_eq!(exchange(&mut client, ExtensionRequest::DurableStorageGet { key: "task".into() }),
            ExtensionReply::StorageGetResult { value: Some("persistent".into()) });
        assert_eq!(exchange(&mut client, ExtensionRequest::StorageGet { key: "task".into() }),
            ExtensionReply::StorageGetResult { value: Some("ephemeral".into()) });
        assert_eq!(exchange(&mut client, hello_with_capabilities(ID, [(CAPABILITY_STORAGE, 3)])), empty_hello_ack());
        assert_eq!(exchange(&mut client, ExtensionRequest::DurableStorageListKeys),
            ExtensionReply::StorageKeysResult { keys: vec!["alpha".into(), "task".into()] });
        assert_eq!(exchange(&mut client, hello_with_capabilities(OTHER_ID, [(CAPABILITY_STORAGE, 3)])), empty_hello_ack());
        assert!(matches!(
            exchange(&mut client, ExtensionRequest::DurableStorageListKeys),
            ExtensionReply::CapabilityDenied { capability, .. } if capability == CAPABILITY_STORAGE
        ));
        drop(client);
        worker.join().unwrap().unwrap();

        let (mut restarted, worker) = spawn(ExtensionStorage::default().with_durable_root(root.clone()));
        assert_eq!(exchange(&mut restarted, hello_with_capabilities(ID, [(CAPABILITY_STORAGE, 3)])), empty_hello_ack());
        assert_eq!(exchange(&mut restarted, ExtensionRequest::DurableStorageListKeys),
            ExtensionReply::StorageKeysResult { keys: vec!["alpha".into(), "task".into()] });
        assert_eq!(exchange(&mut restarted, ExtensionRequest::DurableStorageGet { key: "task".into() }),
            ExtensionReply::StorageGetResult { value: Some("persistent".into()) });
        assert_eq!(exchange(&mut restarted, ExtensionRequest::StorageGet { key: "task".into() }),
            ExtensionReply::StorageGetResult { value: None });
        assert_eq!(exchange(&mut restarted, ExtensionRequest::DurableStorageRemove { key: "task".into() }),
            ExtensionReply::StorageRemoveAck { removed: true });
        assert_eq!(exchange(&mut restarted, ExtensionRequest::DurableStorageListKeys),
            ExtensionReply::StorageKeysResult { keys: vec!["alpha".into()] });
        drop(restarted);
        worker.join().unwrap().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ungranted_storage_never_reaches_the_core_owned_bucket() {
        let registry = ExtensionRegistry::minimal_slice();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || handle_extension_connection(&registry, &mut server));

        write_extension_request(
            &mut client,
            &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_STORAGE, 1)]),
        )
        .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            empty_hello_ack()
        );
        write_extension_request(
            &mut client,
            &ExtensionRequest::StorageSet {
                key: "task-state".to_string(),
                value: "attacker-controlled".to_string(),
            },
        )
        .unwrap();
        match read_extension_reply(&mut client).unwrap() {
            ExtensionReply::CapabilityDenied { capability, .. } => {
                assert_eq!(capability, CAPABILITY_STORAGE)
            }
            other => panic!("expected storage capability denial, got {other:?}"),
        }

        drop(client);
        handle.join().unwrap().unwrap();
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
    fn v5_radio_selection_is_reviewed_and_delegated_without_group_metadata() {
        let registry = registry_with_dom_write_granted();
        let (gatekeeper_socket, gatekeeper) =
            start_gatekeeper("clear-v5-radio", GatekeeperReply::Cleared);
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
            &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_WRITE, 5)]),
        )
        .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            empty_hello_ack()
        );
        write_extension_request(
            &mut client,
            &ExtensionRequest::SetRadioChecked {
                tab_id: 7,
                node_id: 15,
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
                Some((7, 15)),
                "true".to_string(),
                DomWriteTarget::FormInput {
                    input_type: "radio".to_string()
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
                detail: "action=set-radio-checked".to_string(),
            }
        );
        let _ = std::fs::remove_file(gatekeeper_socket);
    }

    #[test]
    fn v7_range_write_is_reviewed_and_delegated_without_range_metadata() {
        let registry = registry_with_dom_write_granted();
        let (gatekeeper_socket, gatekeeper) =
            start_gatekeeper("clear-v7-range", GatekeeperReply::Cleared);
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
            &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_WRITE, 7)]),
        )
        .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            empty_hello_ack()
        );
        write_extension_request(
            &mut client,
            &ExtensionRequest::SetRangeInputValue {
                tab_id: 7,
                node_id: 17,
                value: -3,
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
                Some((7, 17)),
                "-3".to_string(),
                DomWriteTarget::FormInput {
                    input_type: "range".to_string()
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
                detail: "action=set-range-input-value".to_string(),
            }
        );
        let _ = std::fs::remove_file(gatekeeper_socket);
    }

    #[test]
    fn v8_visible_leaf_text_requires_its_version_and_reviews_the_exact_payload() {
        let registry = registry_with_dom_write_granted();
        let (gatekeeper_socket, gatekeeper) = start_gatekeeper("clear-v8-visible-leaf", GatekeeperReply::Cleared);
        let (seen_tx, seen_rx) = std::sync::mpsc::channel();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let socket_for_handler = gatekeeper_socket.clone();
        let handle = thread::spawn(move || {
            handle_extension_connection_with_actions(
                &registry, &socket_for_handler, &mut server,
                |_| Ok("unused".to_string()),
                move |target, value, kind| { seen_tx.send((target, value, kind.clone())).unwrap(); Ok(()) },
                || Ok(()),
            )
        });
        write_extension_request(&mut client, &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_WRITE, 8)])).unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), empty_hello_ack());
        write_extension_request(&mut client, &ExtensionRequest::SetVisibleLeafText { tab_id: 7, node_id: 19, value: "Updated heading".to_string() }).unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), ExtensionReply::DomWriteAck);
        assert_eq!(seen_rx.recv_timeout(Duration::from_secs(1)).unwrap(), (Some((7, 19)), "Updated heading".to_string(), DomWriteTarget::VisibleTextLeaf));
        write_extension_request(&mut client, &ExtensionRequest::SetVisibleLeafText { tab_id: 7, node_id: 19, value: " \n ".to_string() }).unwrap();
        assert!(matches!(read_extension_reply(&mut client).unwrap(), ExtensionReply::OperationUnavailable { capability, .. } if capability == CAPABILITY_DOM_WRITE));
        assert!(seen_rx.try_recv().is_err());
        drop(client);
        handle.join().unwrap().unwrap();
        assert_eq!(gatekeeper.join().unwrap(), GatekeeperRequest::CheckExtensionAction {
            extension_id: MINIMAL_SLICE_EXTENSION_ID.to_string(), capability: CAPABILITY_DOM_WRITE.to_string(),
            detail: "action=set-visible-leaf-text; text=Updated heading".to_string(),
        });
        let _ = std::fs::remove_file(gatekeeper_socket);

        let registry = registry_with_dom_write_granted();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || handle_extension_connection_with_actions(
            &registry, Path::new("/not-reached-for-v7-visible-leaf.sock"), &mut server,
            |_| Ok("unused".to_string()),
            |_, _, _| panic!("v7 cannot delegate a v8 request"),
            || Ok(()),
        ));
        write_extension_request(&mut client, &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_WRITE, 7)])).unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), empty_hello_ack());
        write_extension_request(&mut client, &ExtensionRequest::SetVisibleLeafText { tab_id: 7, node_id: 19, value: "denied".to_string() }).unwrap();
        assert!(matches!(read_extension_reply(&mut client).unwrap(), ExtensionReply::CapabilityDenied { capability, reason } if capability == CAPABILITY_DOM_WRITE && reason.contains("requires version 8")));
        drop(client);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn v9_text_content_requires_its_version_and_reviews_the_exact_payload() {
        let registry = registry_with_dom_write_granted();
        let (gatekeeper_socket, gatekeeper) =
            start_gatekeeper("clear-v9-text-content", GatekeeperReply::Cleared);
        let (seen_tx, seen_rx) = std::sync::mpsc::channel();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let socket_for_handler = gatekeeper_socket.clone();
        let handle = thread::spawn(move || {
            handle_extension_connection_with_actions(
                &registry,
                &socket_for_handler,
                &mut server,
                |_| Ok("unused".to_string()),
                move |target, value, kind| {
                    seen_tx.send((target, value, kind.clone())).unwrap();
                    Ok(())
                },
                || Ok(()),
            )
        });
        write_extension_request(
            &mut client,
            &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_WRITE, 9)]),
        )
        .unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), empty_hello_ack());
        write_extension_request(
            &mut client,
            &ExtensionRequest::SetVisibleTextContent {
                tab_id: 7,
                node_id: 19,
                value: "Updated formatted text".to_string(),
            },
        )
        .unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), ExtensionReply::DomWriteAck);
        assert_eq!(
            seen_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            (
                Some((7, 19)),
                "Updated formatted text".to_string(),
                DomWriteTarget::VisibleTextContent,
            )
        );
        drop(client);
        handle.join().unwrap().unwrap();
        assert_eq!(
            gatekeeper.join().unwrap(),
            GatekeeperRequest::CheckExtensionAction {
                extension_id: MINIMAL_SLICE_EXTENSION_ID.to_string(),
                capability: CAPABILITY_DOM_WRITE.to_string(),
                detail: "action=set-visible-text-content; text=Updated formatted text".to_string(),
            }
        );
        let _ = std::fs::remove_file(gatekeeper_socket);

        let registry = registry_with_dom_write_granted();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || {
            handle_extension_connection_with_actions(
                &registry,
                Path::new("/not-reached-for-v8-text-content.sock"),
                &mut server,
                |_| Ok("unused".to_string()),
                |_, _, _| panic!("v8 cannot delegate a v9 request"),
                || Ok(()),
            )
        });
        write_extension_request(
            &mut client,
            &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_WRITE, 8)]),
        )
        .unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), empty_hello_ack());
        write_extension_request(
            &mut client,
            &ExtensionRequest::SetVisibleTextContent {
                tab_id: 7,
                node_id: 19,
                value: "denied".to_string(),
            },
        )
        .unwrap();
        assert!(matches!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::CapabilityDenied { capability, reason }
                if capability == CAPABILITY_DOM_WRITE && reason.contains("requires version 9")
        ));
        drop(client);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn legacy_dom_write_cannot_alias_the_versioned_visible_text_operations() {
        let registry = registry_with_dom_write_granted();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || {
            handle_extension_connection_with_actions(
                &registry,
                Path::new("/not-reached-for-legacy-visible-text.sock"),
                &mut server,
                |_| Ok("unused".to_string()),
                |_, _, _| panic!("legacy mutation must not reach a visible text delegate"),
                || Ok(()),
            )
        });
        write_extension_request(
            &mut client,
            &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_WRITE, 9)]),
        )
        .unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), empty_hello_ack());
        for target in [DomWriteTarget::VisibleTextLeaf, DomWriteTarget::VisibleTextContent] {
            write_extension_request(
                &mut client,
                &ExtensionRequest::DomWrite {
                    value: "would bypass the explicit node ID".to_string(),
                    target,
                },
            )
            .unwrap();
            assert!(matches!(
                read_extension_reply(&mut client).unwrap(),
                ExtensionReply::OperationUnavailable { capability, reason }
                    if capability == CAPABILITY_DOM_WRITE && reason.contains("explicit versioned")
            ));
        }
        drop(client);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn v8_visible_leaf_text_rejection_never_reaches_core() {
        let registry = registry_with_dom_write_granted();
        let (gatekeeper_socket, gatekeeper) = start_gatekeeper("block-v8-visible-leaf", GatekeeperReply::Rejected {
            reason: "unsafe text".to_string(), category: "extension-visible-text-social-engineering".to_string(),
        });
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let socket_for_handler = gatekeeper_socket.clone();
        let handle = thread::spawn(move || handle_extension_connection_with_actions(
            &registry, &socket_for_handler, &mut server,
            |_| Ok("unused".to_string()),
            |_, _, _| panic!("a gatekeeper rejection must not mutate core"),
            || Ok(()),
        ));
        write_extension_request(&mut client, &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_WRITE, 8)])).unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), empty_hello_ack());
        write_extension_request(&mut client, &ExtensionRequest::SetVisibleLeafText {
            tab_id: 7, node_id: 19, value: "Enter your password".to_string(),
        }).unwrap();
        assert!(matches!(read_extension_reply(&mut client).unwrap(), ExtensionReply::GatekeeperBlocked { category, .. } if category == "extension-visible-text-social-engineering"));
        drop(client);
        handle.join().unwrap().unwrap();
        assert!(matches!(gatekeeper.join().unwrap(), GatekeeperRequest::CheckExtensionAction { detail, .. } if detail.contains("Enter your password")));
        let _ = std::fs::remove_file(gatekeeper_socket);
    }

    #[test]
    fn v7_range_write_is_denied_after_a_v6_handshake_before_review_or_delegate() {
        let registry = registry_with_dom_write_granted();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || {
            handle_extension_connection_with_actions(
                &registry,
                Path::new("/not-reached-for-v6-range-version-denial.sock"),
                &mut server,
                |_| Ok("unused in this test".to_string()),
                |_, _, _| panic!("a v6 connection must not delegate a v7 range request"),
                || Ok(()),
            )
        });

        write_extension_request(
            &mut client,
            &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_WRITE, 6)]),
        )
        .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            empty_hello_ack()
        );
        write_extension_request(
            &mut client,
            &ExtensionRequest::SetRangeInputValue {
                tab_id: 1,
                node_id: 2,
                value: 50,
            },
        )
        .unwrap();
        match read_extension_reply(&mut client).unwrap() {
            ExtensionReply::CapabilityDenied { capability, reason } => {
                assert_eq!(capability, CAPABILITY_DOM_WRITE);
                assert!(reason.contains("requires version 7"));
            }
            other => panic!("expected a v7 version denial, got {other:?}"),
        }

        drop(client);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn v6_select_option_is_reviewed_and_delegated_without_select_metadata() {
        let registry = registry_with_dom_write_granted();
        let (gatekeeper_socket, gatekeeper) =
            start_gatekeeper("clear-v6-select-option", GatekeeperReply::Cleared);
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
            &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_WRITE, 6)]),
        )
        .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            empty_hello_ack()
        );
        write_extension_request(
            &mut client,
            &ExtensionRequest::SelectOption {
                tab_id: 7,
                node_id: 16,
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
                Some((7, 16)),
                "true".to_string(),
                DomWriteTarget::FormInput {
                    input_type: "select".to_string()
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
                detail: "action=select-option".to_string(),
            }
        );
        let _ = std::fs::remove_file(gatekeeper_socket);
    }

    #[test]
    fn v6_select_option_is_denied_after_a_v5_handshake_before_review_or_delegate() {
        let registry = registry_with_dom_write_granted();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || {
            handle_extension_connection_with_actions(
                &registry,
                Path::new("/not-reached-for-v5-select-version-denial.sock"),
                &mut server,
                |_| Ok("unused in this test".to_string()),
                |_, _, _| panic!("a v5 connection must not delegate a v6 select request"),
                || Ok(()),
            )
        });

        write_extension_request(
            &mut client,
            &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_WRITE, 5)]),
        )
        .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            empty_hello_ack()
        );
        write_extension_request(
            &mut client,
            &ExtensionRequest::SelectOption {
                tab_id: 1,
                node_id: 2,
            },
        )
        .unwrap();
        match read_extension_reply(&mut client).unwrap() {
            ExtensionReply::CapabilityDenied { capability, reason } => {
                assert_eq!(capability, CAPABILITY_DOM_WRITE);
                assert!(reason.contains("requires version 6"));
            }
            other => panic!("expected a v6 version denial, got {other:?}"),
        }

        drop(client);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn v5_radio_selection_is_denied_after_a_v4_handshake_before_review_or_delegate() {
        let registry = registry_with_dom_write_granted();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || {
            handle_extension_connection_with_actions(
                &registry,
                Path::new("/not-reached-for-v4-radio-version-denial.sock"),
                &mut server,
                |_| Ok("unused in this test".to_string()),
                |_, _, _| panic!("a v4 connection must not delegate a v5 radio request"),
                || Ok(()),
            )
        });

        write_extension_request(
            &mut client,
            &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_WRITE, 4)]),
        )
        .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            empty_hello_ack()
        );
        write_extension_request(
            &mut client,
            &ExtensionRequest::SetRadioChecked {
                tab_id: 1,
                node_id: 2,
            },
        )
        .unwrap();
        match read_extension_reply(&mut client).unwrap() {
            ExtensionReply::CapabilityDenied { capability, reason } => {
                assert_eq!(capability, CAPABILITY_DOM_WRITE);
                assert!(reason.contains("requires version 5"));
            }
            other => panic!("expected a v5 version denial, got {other:?}"),
        }

        drop(client);
        handle.join().unwrap().unwrap();
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
    fn v4_textarea_write_is_reviewed_and_delegated_with_explicit_ids() {
        let registry = registry_with_dom_write_granted();
        let (gatekeeper_socket, gatekeeper) =
            start_gatekeeper("clear-v4-textarea", GatekeeperReply::Cleared);
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
            &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_WRITE, 4)]),
        )
        .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            empty_hello_ack()
        );
        write_extension_request(
            &mut client,
            &ExtensionRequest::SetTextareaValue {
                tab_id: 7,
                node_id: 14,
                value: "core-owned\nnotes".to_string(),
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
                Some((7, 14)),
                "core-owned\nnotes".to_string(),
                DomWriteTarget::FormInput {
                    input_type: "textarea".to_string()
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
                detail: "action=set-textarea-value".to_string(),
            }
        );
        let _ = std::fs::remove_file(gatekeeper_socket);
    }

    #[test]
    fn oversized_v2_and_v4_text_writes_are_rejected_before_review_or_delegation() {
        let registry = registry_with_dom_write_granted();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || {
            handle_extension_connection_with_actions(
                &registry,
                Path::new("/not-reached-for-oversized-text-writes.sock"),
                &mut server,
                |_| Ok("unused in this test".to_string()),
                |_, _, _| panic!("an oversized write must not reach core"),
                || Ok(()),
            )
        });

        write_extension_request(
            &mut client,
            &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_WRITE, 4)]),
        )
        .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            empty_hello_ack()
        );
        let oversized = "x".repeat(blueice_ipc::extension::MAX_TEXT_WRITE_BYTES + 1);
        for request in [
            ExtensionRequest::SetTextInputValue {
                tab_id: 1,
                node_id: 2,
                value: oversized.clone(),
            },
            ExtensionRequest::SetTextareaValue {
                tab_id: 1,
                node_id: 3,
                value: oversized.clone(),
            },
        ] {
            write_extension_request(&mut client, &request).unwrap();
            match read_extension_reply(&mut client).unwrap() {
                ExtensionReply::OperationUnavailable { capability, reason } => {
                    assert_eq!(capability, CAPABILITY_DOM_WRITE);
                    assert!(reason.contains("4096 bytes"));
                }
                other => panic!("expected oversized value rejection, got {other:?}"),
            }
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
    fn v2_network_block_url_is_reviewed_then_delegated_without_exposing_the_url_to_review() {
        let registry = registry_with_network_intercept_granted();
        let (gatekeeper_socket, gatekeeper) =
            start_gatekeeper("clear-exact-navigation-block", GatekeeperReply::Cleared);
        let (seen_tx, seen_rx) = std::sync::mpsc::channel();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let socket_for_handler = gatekeeper_socket.clone();
        let handle = thread::spawn(move || {
            handle_extension_connection_with_actions_and_authentication_and_network_rules(
                &registry,
                &socket_for_handler,
                &mut server,
                ExtensionConnectionAuthentication::unauthenticated(),
                ExtensionActionDelegates::new(
                    |_| Ok("unused in this test".to_string()),
                    unused_write_delegate,
                    || Ok(()),
                    move |url| {
                        seen_tx.send(url).unwrap();
                        Ok(())
                    },
                    || panic!("a v2 request must not clear v3 rules"),
                ),
            )
        });

        write_extension_request(
            &mut client,
            &hello_with_capabilities(
                MINIMAL_SLICE_EXTENSION_ID,
                [(CAPABILITY_NETWORK_INTERCEPT, 2)],
            ),
        )
        .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            empty_hello_ack()
        );
        write_extension_request(
            &mut client,
            &ExtensionRequest::RegisterNetworkBlockUrl {
                url: "https://example.test/private".to_string(),
            },
        )
        .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::NetworkInterceptAck
        );
        assert_eq!(
            seen_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            "https://example.test/private"
        );

        drop(client);
        handle.join().unwrap().unwrap();
        assert_eq!(
            gatekeeper.join().unwrap(),
            GatekeeperRequest::CheckExtensionAction {
                extension_id: MINIMAL_SLICE_EXTENSION_ID.to_string(),
                capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                detail: "action=register-exact-navigation-block".to_string(),
            }
        );
        let _ = std::fs::remove_file(gatekeeper_socket);
    }

    #[test]
    fn v4_network_block_host_is_reviewed_then_delegated_without_exposing_the_host() {
        let registry = registry_with_network_intercept_granted();
        let (gatekeeper_socket, gatekeeper) =
            start_gatekeeper("clear-host-navigation-block", GatekeeperReply::Cleared);
        let (seen_tx, seen_rx) = std::sync::mpsc::channel();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let socket_for_handler = gatekeeper_socket.clone();
        let handle = thread::spawn(move || {
            handle_extension_connection_with_actions_and_authentication_and_network_rules(
                &registry,
                &socket_for_handler,
                &mut server,
                ExtensionConnectionAuthentication::unauthenticated(),
                ExtensionActionDelegates::new(
                    |_| Ok("unused in this test".to_string()),
                    unused_write_delegate,
                    || Ok(()),
                    |_| panic!("a host request must not register an exact URL"),
                    || Ok(()),
                ).with_network_block_host(move |host| {
                    seen_tx.send(host).unwrap();
                    Ok(())
                }),
            )
        });

        write_extension_request(
            &mut client,
            &hello_with_capabilities(
                MINIMAL_SLICE_EXTENSION_ID,
                [(CAPABILITY_NETWORK_INTERCEPT, 4)],
            ),
        ).unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), empty_hello_ack());
        write_extension_request(
            &mut client,
            &ExtensionRequest::RegisterNetworkBlockHost { host: "Example.test".to_string() },
        ).unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), ExtensionReply::NetworkInterceptAck);
        assert_eq!(seen_rx.recv_timeout(Duration::from_secs(1)).unwrap(), "Example.test");

        drop(client);
        handle.join().unwrap().unwrap();
        assert_eq!(gatekeeper.join().unwrap(), GatekeeperRequest::CheckExtensionAction {
            extension_id: MINIMAL_SLICE_EXTENSION_ID.to_string(),
            capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
            detail: "action=register-host-navigation-block".to_string(),
        });
        let _ = std::fs::remove_file(gatekeeper_socket);
    }

    #[test]
    fn v4_host_registration_is_denied_after_a_v3_handshake() {
        let registry = registry_with_network_intercept_granted();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || {
            handle_extension_connection_with_actions_and_authentication_and_network_rules(
                &registry,
                Path::new("/not-reached-for-v3-host-rule-version-denial.sock"),
                &mut server,
                ExtensionConnectionAuthentication::unauthenticated(),
                ExtensionActionDelegates::new(
                    |_| Ok("unused in this test".to_string()),
                    unused_write_delegate,
                    || Ok(()),
                    |_| Ok(()),
                    || Ok(()),
                ).with_network_block_host(|_| panic!("a v3 connection must not reach core")),
            )
        });
        write_extension_request(
            &mut client,
            &hello_with_capabilities(
                MINIMAL_SLICE_EXTENSION_ID,
                [(CAPABILITY_NETWORK_INTERCEPT, 3)],
            ),
        ).unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), empty_hello_ack());
        write_extension_request(
            &mut client,
            &ExtensionRequest::RegisterNetworkBlockHost { host: "example.test".to_string() },
        ).unwrap();
        match read_extension_reply(&mut client).unwrap() {
            ExtensionReply::CapabilityDenied { capability, reason } => {
                assert_eq!(capability, CAPABILITY_NETWORK_INTERCEPT);
                assert!(reason.contains("requires version 4"));
            }
            other => panic!("expected a v4 version denial, got {other:?}"),
        }
        drop(client);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn v5_path_prefix_is_reviewed_then_delegated_without_exposing_guest_fields() {
        let registry = registry_with_network_intercept_granted();
        let (gatekeeper_socket, gatekeeper) =
            start_gatekeeper("clear-path-prefix-block", GatekeeperReply::Cleared);
        let (seen_tx, seen_rx) = std::sync::mpsc::channel();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let socket_for_handler = gatekeeper_socket.clone();
        let handle = thread::spawn(move || {
            handle_extension_connection_with_actions_and_authentication_and_network_rules(
                &registry,
                &socket_for_handler,
                &mut server,
                ExtensionConnectionAuthentication::unauthenticated(),
                ExtensionActionDelegates::new(
                    |_| Ok("unused".into()), unused_write_delegate, || Ok(()),
                    |_| panic!("a path request must not register an exact URL"), || Ok(()),
                ).with_network_block_path_prefix(move |host, path_prefix| {
                    seen_tx.send((host, path_prefix)).unwrap();
                    Ok(())
                }),
            )
        });
        write_extension_request(&mut client, &hello_with_capabilities(
            MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_NETWORK_INTERCEPT, 5)],
        )).unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), empty_hello_ack());
        write_extension_request(&mut client, &ExtensionRequest::RegisterNetworkBlockPathPrefix {
            host: "Example.test".into(), path_prefix: "/private".into(),
        }).unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), ExtensionReply::NetworkInterceptAck);
        assert_eq!(seen_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            ("Example.test".to_string(), "/private".to_string()));
        drop(client);
        handle.join().unwrap().unwrap();
        assert_eq!(gatekeeper.join().unwrap(), GatekeeperRequest::CheckExtensionAction {
            extension_id: MINIMAL_SLICE_EXTENSION_ID.to_string(),
            capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
            detail: "action=register-path-prefix-navigation-block".to_string(),
        });
        let _ = std::fs::remove_file(gatekeeper_socket);
    }

    #[test]
    fn v5_path_prefix_is_denied_after_a_v4_handshake_before_gatekeeper_or_core() {
        let registry = registry_with_network_intercept_granted();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || {
            handle_extension_connection_with_actions_and_authentication_and_network_rules(
                &registry,
                Path::new("/not-reached-for-v4-path-rule-denial.sock"),
                &mut server,
                ExtensionConnectionAuthentication::unauthenticated(),
                ExtensionActionDelegates::new(
                    |_| Ok("unused".into()), unused_write_delegate, || Ok(()),
                    |_| Ok(()), || Ok(()),
                ).with_network_block_path_prefix(|_, _| panic!("v4 must not reach core")),
            )
        });
        write_extension_request(&mut client, &hello_with_capabilities(
            MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_NETWORK_INTERCEPT, 4)],
        )).unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), empty_hello_ack());
        write_extension_request(&mut client, &ExtensionRequest::RegisterNetworkBlockPathPrefix {
            host: "example.test".into(), path_prefix: "/private".into(),
        }).unwrap();
        assert!(matches!(read_extension_reply(&mut client).unwrap(),
            ExtensionReply::CapabilityDenied { capability, reason }
                if capability == CAPABILITY_NETWORK_INTERCEPT && reason.contains("requires version 5")));
        drop(client);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn v6_same_origin_redirect_is_reviewed_with_fixed_metadata_before_core() {
        let registry = registry_with_network_intercept_granted();
        let (gatekeeper_socket, gatekeeper) =
            start_gatekeeper("clear-navigation-redirect", GatekeeperReply::Cleared);
        let (seen_tx, seen_rx) = std::sync::mpsc::channel();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let socket_for_handler = gatekeeper_socket.clone();
        let handle = thread::spawn(move || {
            handle_extension_connection_with_actions_and_authentication_and_network_rules(
                &registry, &socket_for_handler, &mut server,
                ExtensionConnectionAuthentication::unauthenticated(),
                ExtensionActionDelegates::new(
                    |_| Ok("unused".into()), unused_write_delegate, || Ok(()),
                    |_| Ok(()), || Ok(()),
                ).with_network_redirect_url(move |source_url, target_url| {
                    seen_tx.send((source_url, target_url)).unwrap();
                    Ok(())
                }),
            )
        });
        write_extension_request(&mut client, &hello_with_capabilities(
            MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_NETWORK_INTERCEPT, 6)],
        )).unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), empty_hello_ack());
        write_extension_request(&mut client, &ExtensionRequest::RegisterNetworkRedirectUrl {
            source_url: "https://example.test/old".into(),
            target_url: "https://example.test/new".into(),
        }).unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), ExtensionReply::NetworkInterceptAck);
        assert_eq!(seen_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            ("https://example.test/old".to_string(), "https://example.test/new".to_string()));
        drop(client);
        handle.join().unwrap().unwrap();
        assert_eq!(gatekeeper.join().unwrap(), GatekeeperRequest::CheckExtensionAction {
            extension_id: MINIMAL_SLICE_EXTENSION_ID.to_string(),
            capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
            detail: "action=register-same-origin-navigation-redirect".to_string(),
        });
        let _ = std::fs::remove_file(gatekeeper_socket);
    }

    #[test]
    fn v6_redirect_is_denied_after_a_v5_handshake() {
        let registry = registry_with_network_intercept_granted();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || {
            handle_extension_connection_with_actions_and_authentication_and_network_rules(
                &registry, Path::new("/not-reached-for-v5-redirect-denial.sock"), &mut server,
                ExtensionConnectionAuthentication::unauthenticated(),
                ExtensionActionDelegates::new(
                    |_| Ok("unused".into()), unused_write_delegate, || Ok(()),
                    |_| Ok(()), || Ok(()),
                ).with_network_redirect_url(|_, _| panic!("v5 must not reach core")),
            )
        });
        write_extension_request(&mut client, &hello_with_capabilities(
            MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_NETWORK_INTERCEPT, 5)],
        )).unwrap();
        assert_eq!(read_extension_reply(&mut client).unwrap(), empty_hello_ack());
        write_extension_request(&mut client, &ExtensionRequest::RegisterNetworkRedirectUrl {
            source_url: "https://example.test/old".into(),
            target_url: "https://example.test/new".into(),
        }).unwrap();
        assert!(matches!(read_extension_reply(&mut client).unwrap(),
            ExtensionReply::CapabilityDenied { capability, reason }
                if capability == CAPABILITY_NETWORK_INTERCEPT && reason.contains("requires version 6")));
        drop(client);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn v3_network_rule_clear_is_delegated_without_gatekeeper_review() {
        let registry = registry_with_network_intercept_granted();
        let (cleared_tx, cleared_rx) = std::sync::mpsc::channel();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || {
            handle_extension_connection_with_actions_and_authentication_and_network_rules(
                &registry,
                Path::new("/not-reached-for-safe-network-rule-clear.sock"),
                &mut server,
                ExtensionConnectionAuthentication::unauthenticated(),
                ExtensionActionDelegates::new(
                    |_| Ok("unused in this test".to_string()),
                    unused_write_delegate,
                    || Ok(()),
                    |_| panic!("a v3 clear must not register a rule"),
                    move || {
                        cleared_tx.send(()).unwrap();
                        Ok(())
                    },
                ),
            )
        });

        write_extension_request(
            &mut client,
            &hello_with_capabilities(
                MINIMAL_SLICE_EXTENSION_ID,
                [(CAPABILITY_NETWORK_INTERCEPT, 3)],
            ),
        )
        .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            empty_hello_ack()
        );
        write_extension_request(&mut client, &ExtensionRequest::ClearNetworkBlockUrls).unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::NetworkInterceptAck
        );
        cleared_rx.recv_timeout(Duration::from_secs(1)).unwrap();

        drop(client);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn v3_network_rule_clear_requires_a_v3_handshake_before_its_delegate() {
        let registry = registry_with_network_intercept_granted();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || {
            handle_extension_connection_with_actions_and_authentication_and_network_rules(
                &registry,
                Path::new("/not-reached-for-v2-network-rule-clear-version-denial.sock"),
                &mut server,
                ExtensionConnectionAuthentication::unauthenticated(),
                ExtensionActionDelegates::new(
                    |_| Ok("unused in this test".to_string()),
                    unused_write_delegate,
                    || Ok(()),
                    |_| panic!("a v2 connection must not register a v2 rule in this test"),
                    || panic!("a v2 connection must not clear v3 rules"),
                ),
            )
        });

        write_extension_request(
            &mut client,
            &hello_with_capabilities(
                MINIMAL_SLICE_EXTENSION_ID,
                [(CAPABILITY_NETWORK_INTERCEPT, 2)],
            ),
        )
        .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            empty_hello_ack()
        );
        write_extension_request(&mut client, &ExtensionRequest::ClearNetworkBlockUrls).unwrap();
        match read_extension_reply(&mut client).unwrap() {
            ExtensionReply::CapabilityDenied { capability, reason } => {
                assert_eq!(capability, CAPABILITY_NETWORK_INTERCEPT);
                assert!(reason.contains("requires version 3"));
            }
            other => panic!("expected a v3 version denial, got {other:?}"),
        }

        drop(client);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn v2_network_block_url_requires_a_v2_handshake_before_review_or_delegate() {
        let registry = registry_with_network_intercept_granted();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || {
            handle_extension_connection_with_actions_and_authentication_and_network_rules(
                &registry,
                Path::new("/not-reached-for-v1-network-block-version-denial.sock"),
                &mut server,
                ExtensionConnectionAuthentication::unauthenticated(),
                ExtensionActionDelegates::new(
                    |_| Ok("unused in this test".to_string()),
                    unused_write_delegate,
                    || Ok(()),
                    |_| panic!("a v1 connection must not install a v2 network rule"),
                    || panic!("a v1 connection must not clear v3 rules"),
                ),
            )
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
        write_extension_request(
            &mut client,
            &ExtensionRequest::RegisterNetworkBlockUrl {
                url: "https://example.test/private".to_string(),
            },
        )
        .unwrap();
        match read_extension_reply(&mut client).unwrap() {
            ExtensionReply::CapabilityDenied { capability, reason } => {
                assert_eq!(capability, CAPABILITY_NETWORK_INTERCEPT);
                assert!(reason.contains("requires version 2"));
            }
            other => panic!("expected a v2 version denial, got {other:?}"),
        }

        drop(client);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn oversized_v2_network_block_url_is_rejected_before_review_or_delegate() {
        let registry = registry_with_network_intercept_granted();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || {
            handle_extension_connection_with_actions_and_authentication_and_network_rules(
                &registry,
                Path::new("/not-reached-for-oversized-network-rule.sock"),
                &mut server,
                ExtensionConnectionAuthentication::unauthenticated(),
                ExtensionActionDelegates::new(
                    |_| Ok("unused in this test".to_string()),
                    unused_write_delegate,
                    || Ok(()),
                    |_| panic!("an oversized network rule must not reach core"),
                    || panic!("an oversized network rule must not clear v3 rules"),
                ),
            )
        });

        write_extension_request(
            &mut client,
            &hello_with_capabilities(
                MINIMAL_SLICE_EXTENSION_ID,
                [(CAPABILITY_NETWORK_INTERCEPT, 2)],
            ),
        )
        .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            empty_hello_ack()
        );
        write_extension_request(
            &mut client,
            &ExtensionRequest::RegisterNetworkBlockUrl {
                url: "x".repeat(blueice_ipc::extension::MAX_NETWORK_BLOCK_URL_BYTES + 1),
            },
        )
        .unwrap();
        match read_extension_reply(&mut client).unwrap() {
            ExtensionReply::OperationUnavailable { capability, reason } => {
                assert_eq!(capability, CAPABILITY_NETWORK_INTERCEPT);
                assert!(reason.contains("2048 bytes"));
            }
            other => panic!("expected an oversized-rule rejection, got {other:?}"),
        }

        drop(client);
        handle.join().unwrap().unwrap();
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
        let (runtime_event_tx, runtime_event_rx) = std::sync::mpsc::channel();
        let runtime_event_rx = Arc::new(Mutex::new(runtime_event_rx));
        let handle = thread::spawn(move || {
            handle_extension_connection_with_actions_and_authentication(
                &registry,
                Path::new("/not-used-before-a-dom-action.sock"),
                &mut server,
                ExtensionConnectionAuthentication::required(&expected)
                    .with_runtime_start_receiver(runtime_start_rx)
                    .with_runtime_event_receiver(runtime_event_rx),
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

        write_extension_request(&mut client, &ExtensionRequest::NextRuntimeEvent).unwrap();
        runtime_event_tx
            .send(ExtensionRuntimeEvent::NavigationCommitted { tab_id: 17 })
            .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::RuntimeEvent(ExtensionRuntimeEvent::NavigationCommitted { tab_id: 17 })
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
