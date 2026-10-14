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

#[cfg(unix)]
mod durable_storage;
#[cfg(not(unix))]
#[path = "durable_storage_unsupported.rs"]
mod durable_storage;
mod manifest;
#[cfg(unix)]
mod runtime;

pub use durable_storage::default_durable_storage_root;
pub use manifest::{
    load_installed_extension, registry_for_installed_extension, ExtensionManifest,
    InstalledExtension, ManifestCapabilities, ManifestError, MANIFEST_API_VERSION,
};
#[cfg(unix)]
pub use runtime::{
    execute_installed_extension, execute_installed_extension_for_invocation, RuntimeInvocation,
};

use blueice_ipc::extension::{
    read_extension_request, write_extension_reply, ExtensionReply, ExtensionRequest,
    ExtensionRuntimeEvent, NetworkResponseInfo, NetworkTraceInfo, UnsupportedCapabilityVersion,
};
use blueice_ipc::gatekeeper::{default_gatekeeper_socket_path, GatekeeperReply};
#[cfg(unix)]
use blueice_ipc::gatekeeper::{read_gatekeeper_reply, write_gatekeeper_request, GatekeeperRequest};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{self, Read, Write};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

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
        || !label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b'-' | b'_'))
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
        return Err(
            "popup body must be 1–120 printable ASCII bytes without outer spaces".to_string(),
        );
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

/// [`ExtensionReply::DomReadResult`]'s value for a granted `DomRead` in
/// this minimal slice -- a fixed placeholder, not real `Page` state; see
/// this crate's own module docs for why.
const PLACEHOLDER_DOM_READ_VALUE: &str =
    "<blueice-extension-host: no real Page is wired into this minimal slice>";

#[cfg(unix)]
const GATEKEEPER_CHECK_TIMEOUT: Duration = Duration::from_secs(10);
mod storage;
pub use storage::ExtensionStorage;
#[cfg(unix)]
use storage::*;
mod registry;
use registry::*;
pub use registry::{CapabilityVersionWindow, ExtensionRegistry};
mod connection_support;
use connection_support::*;
mod delegates;
pub use delegates::{ExtensionActionDelegates, ExtensionConnectionAuthentication};
mod connection;
pub use connection::*;

#[cfg(all(test, unix))]
mod tests;
