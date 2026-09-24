// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The internal wire protocol between `core` (or, for this minimal
//! slice, `blueice-extension-host` standing in for the capability-
//! enforcing side -- see that crate's own module docs for why) and a
//! connected extension process (`phase-9-extension-protocol/PLAN.md`'s
//! "Wiring design (resolved 2026-09-08)"). Lives inside `blueice-ipc`
//! for the same reason [`crate::gatekeeper`] does: to reuse this
//! crate's private length-prefixed-JSON framing primitives
//! ([`crate::write_framed`]/[`crate::read_frame_bytes`]) without
//! changing their visibility -- `extension` is a descendant module of
//! the crate root, so those `fn`s (private to their defining module and
//! its descendants) are already reachable here.
//!
//! A new, separate module rather than new variants on
//! [`crate::ClientMessage`]/[`crate::ServerMessage`]: folding extension
//! messages into the external client protocol would couple extension
//! capability versioning to [`crate::PROTOCOL_VERSION`]'s own
//! all-or-nothing bumps, and force `frontend`/`mcp-server` to carry
//! match arms for a vocabulary they never speak.
//!
//! One real divergence from [`crate::gatekeeper`]'s own shape: that
//! module is deliberately handshake-less (one-shot, short-lived, built
//! in lockstep with `core`), but an extension connection is long-lived
//! like the external client protocol's, so it needs a `Hello`-handshake
//! shape layered onto `gatekeeper`'s framing style -- a genuine hybrid
//! of the two existing precedents, not a mechanical copy of either.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::path::PathBuf;

/// Maximum UTF-8 payload for a versioned native text-control write. The bound
/// is part of the protocol rather than only the WASM ABI: a manually connected
/// development client is subject to it too, and core repeats the check before
/// changing a live page.
pub const MAX_TEXT_WRITE_BYTES: usize = 4 * 1024;
/// One visible, semantic leaf's replacement text. Keep the payload smaller
/// than form values because it is reviewed as untrusted text by the model.
pub const MAX_VISIBLE_LEAF_TEXT_BYTES: usize = 1024;

/// Maximum UTF-8 URL accepted by the first declarative network rule. Keeping
/// a distinct, small bound means a rule cannot turn the extension socket into
/// a route for large arbitrary payloads. Core also parses and canonicalizes it
/// before it becomes active.
pub const MAX_NETWORK_BLOCK_URL_BYTES: usize = 2 * 1024;
/// A version-4 declarative host rule accepts one bounded ASCII host, never
/// a URL, wildcard expression, or arbitrary routing script.
pub const MAX_NETWORK_BLOCK_HOST_BYTES: usize = 253;
/// A path-prefix rule has a deliberately small, literal ASCII path rather
/// than an arbitrary URL pattern, regular expression, or request callback.
pub const MAX_NETWORK_BLOCK_PATH_BYTES: usize = 512;

/// Maximum serialized response metadata returned to an extension guest.
pub const MAX_NETWORK_OBSERVATION_BYTES: usize = 4 * 1024;
/// A redirect chain can contain up to ten URL pairs; keep the v2 trace
/// independently bounded without changing v1's smaller response limit.
pub const MAX_NETWORK_TRACE_BYTES: usize = 32 * 1024;

/// Native chrome accepts one short label, never extension HTML or a guest
/// selected coordinate. The host and core both enforce this bound.
pub const MAX_EXTENSION_TOOLBAR_LABEL_BYTES: usize = 20;

/// A popup is native, non-interactive text. Both strings remain bounded before
/// they cross the extension host and core session boundaries.
pub const MAX_EXTENSION_POPUP_TITLE_BYTES: usize = 20;
pub const MAX_EXTENSION_POPUP_BODY_BYTES: usize = 120;

/// Metadata for the final HTTP response of one committed navigation. Response
/// bodies and sensitive headers are intentionally excluded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkResponseInfo {
    pub method: String,
    pub final_url: String,
    pub status: u16,
    pub content_type: Option<String>,
}

/// One response that redirected an already-reviewed navigation request.
/// No arbitrary headers, body, or cookies are included (the v1 final response
/// carries only its validated Content-Type). URL strings can themselves carry
/// sensitive path/query data, so the capability needs an install-time grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkRedirectInfo {
    pub request_url: String,
    pub status: u16,
    pub target_url: String,
}

/// Version 2 of `network:observe`: committed GET request and redirect-hop
/// metadata plus the unchanged v1 final response. An incomplete or rejected
/// navigation must never publish this record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkTraceInfo {
    pub request_url: String,
    pub redirects: Vec<NetworkRedirectInfo>,
    pub response: NetworkResponseInfo,
}

/// Maximum UTF-8 key for the first bounded extension storage API. Keys use a
/// small identifier grammar in the host, while this wire-level size limit also
/// protects manually connected development clients before a request reaches
/// core-owned state.
pub const MAX_STORAGE_KEY_BYTES: usize = 256;

/// Maximum UTF-8 value for one bounded extension storage entry. The aggregate
/// per-extension quota is enforced by the core-owned store as well.
pub const MAX_STORAGE_VALUE_BYTES: usize = 16 * 1024;
/// A JSON array of at most 128 validated 256-byte ASCII keys fits below
/// this guest-copy bound, including quotes, commas, and brackets.
pub const MAX_STORAGE_KEYS_JSON_BYTES: usize = 40 * 1024;

/// One message an extension process sends to the capability-enforcing
/// side of this protocol.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ExtensionRequest {
    /// Sent once, first, on a fresh connection -- unlike
    /// [`crate::gatekeeper`]'s handshake-less one-shot protocol, this
    /// one is long-lived, so it needs an explicit first-message
    /// handshake the same way [`crate::ClientMessage::Hello`] is for
    /// the external client protocol. `blueice-core`'s opt-in installed
    /// extension path derives `extension_id` from the exact validated
    /// manifest and WASM bytes; the standalone reference host retains a
    /// fixed test identity. This string establishes a registry lookup, not
    /// process authentication: core's optional production host-spawned mode
    /// therefore requires [`Self::HelloAuthenticated`] instead. The standalone
    /// and explicitly manual development modes still use this bearer form.
    ///
    /// `capability_versions` declares only the capabilities this
    /// extension actually uses, per-capability, e.g. `{"dom:read": 1}`
    /// -- the two-layer versioning design's second layer. The host
    /// checks every declaration against its per-capability supported
    /// window and reports only incompatible entries in
    /// [`ExtensionReply::HelloAck`]; compatible entries remain usable
    /// on this connection.
    Hello {
        extension_id: String,
        capability_versions: BTreeMap<String, u32>,
    },
    /// The authenticated form of [`Self::Hello`]. A core process creates a
    /// fresh, high-entropy token for one extension-host child, passes it only
    /// through that child's environment, and requires this first request when
    /// it launched the host itself. The token is connection credentials, not
    /// an extension-controlled capability or package identity; it is never
    /// persisted in a manifest or returned in a reply.
    ///
    /// The unauthenticated [`Self::Hello`] is retained for the standalone
    /// protocol test server and manually-connected development clients. A
    /// core that enabled host-spawned mode rejects that older form before it
    /// acknowledges anything, so compatibility cannot silently weaken the
    /// new boundary.
    HelloAuthenticated {
        extension_id: String,
        capability_versions: BTreeMap<String, u32>,
        authentication: String,
    },
    /// Internal lifecycle barrier for a core-spawned, authenticated host.
    /// After `HelloAck`, the host sends this before executing its WASM reactor;
    /// core replies [`ExtensionReply::RuntimeStart`] only after it has accepted
    /// a frontend and initialized the session that owns live page state. This
    /// is not an extension capability and does not grant any new authority.
    RuntimeReady,
    /// Waits for the next core-defined lifecycle event after an authenticated
    /// runtime has started. This is a pull rather than an unsolicited message:
    /// the host finishes one bounded Wasm invocation before it can receive and
    /// instantiate the next one, so the extension socket has one reader and
    /// one request/reply turn at all times. It is not a capability grant.
    NextRuntimeEvent,
    /// Query the AI-facing representation, read-only -- requires the
    /// `dom:read` capability. The standalone reference host returns a
    /// placeholder. An installed extension served by `blueice-core` receives
    /// JSON for the core-owned default tab's [`crate::AiSnapshot`]. No tab
    /// field exists in this v1 request, so multi-tab targeting remains an
    /// explicit future protocol extension rather than an implicit "active
    /// tab" convention.
    DomRead,
    /// Version 2 of `dom:read`: returns the AI-facing representation of the
    /// explicit core tab. An extension that negotiated only `dom:read` v1
    /// must keep using [`Self::DomRead`], preserving the original default-tab
    /// behavior rather than gaining a new addressing convention silently.
    DomReadTab { tab_id: u64 },
    /// Version 1 of `network:observe`: read only the final response metadata
    /// associated with the currently committed page in an explicit tab.
    /// `None` means this page did not come from an HTTP fetch.
    ReadNetworkResponse { tab_id: u64 },
    /// Version 2 of `network:observe`: the committed navigation's initial
    /// request, redirect hops, and final response, without headers or bodies.
    ReadNetworkTrace { tab_id: u64 },
    /// Version 1 of `ui:inject`: show one core-owned native toolbar button.
    /// The label is validated as short printable ASCII before publication.
    SetToolbarButton { label: String },
    /// Remove only this extension connection's native toolbar button.
    ClearToolbarButton,
    /// Version 2 of `ui:inject`: publish one bounded native text popup for a
    /// live tab. This action is reviewed by the gatekeeper before core sees it.
    ShowPopup {
        tab_id: u64,
        title: String,
        body: String,
    },
    /// Version 3 of `ui:inject`: publish a native popup with one bounded
    /// action button. The title, body, and label all receive gatekeeper
    /// review before core exposes them. Activation conveys no human-gesture
    /// authority and cannot grant an optional capability.
    ShowPopupAction {
        tab_id: u64,
        title: String,
        body: String,
        action_label: String,
    },
    /// Hide the caller connection's popup, if any.
    ClearPopup,
    /// Mutate something DOM-shaped -- requires the `dom:write`
    /// capability, deliberately not granted to this minimal slice's one
    /// hardcoded extension, so this is the request that proves
    /// server-side denial. `value` is a trivial stand-in for whatever a
    /// real write payload eventually looks like. Only writes with a
    /// [`DomWriteTarget::FormInput`] or
    /// [`DomWriteTarget::NetworkCausing`] target require a gatekeeper
    /// action review; a generic document mutation does not.
    DomWrite {
        value: String,
        #[serde(default)]
        target: DomWriteTarget,
    },
    /// Version 2 of `dom:write`: changes the value of one explicit, supported
    /// native text input in one explicit tab. This is intentionally not a
    /// generic DOM-mutation language: core validates both IDs and the input
    /// type before it changes its own document. Every invocation receives
    /// gatekeeper review; no extension-provided field-type label is trusted
    /// to decide whether the operation is high-risk.
    SetTextInputValue {
        tab_id: u64,
        node_id: u64,
        value: String,
    },
    /// Version 3 of `dom:write`: changes the checked state of one explicit,
    /// enabled native checkbox in one explicit tab. This is intentionally not
    /// a generic attribute mutation: core verifies the live element/tag/type,
    /// rejects radios and disabled controls, performs gatekeeper review, and
    /// only then changes its own document state.
    SetCheckboxChecked {
        tab_id: u64,
        node_id: u64,
        checked: bool,
    },
    /// Version 4 of `dom:write`: changes the text content of one explicit,
    /// enabled native textarea in one explicit tab. This is deliberately a
    /// separate operation from text inputs: core verifies the live tag and
    /// enabled state before it changes its own document, then exposes the
    /// result through the same representation/frame path as every other
    /// core-owned form update.
    SetTextareaValue {
        tab_id: u64,
        node_id: u64,
        value: String,
    },
    /// Version 7 of `dom:write`: sets one integer value on an enabled native
    /// range input. Core derives and validates the live min/max/step
    /// constraints; no guest-supplied attribute name or range metadata is
    /// accepted.
    SetRangeInputValue {
        tab_id: u64,
        node_id: u64,
        value: i64,
    },
    /// Version 8 of `dom:write`: replace only the text of a live, rendered
    /// heading, paragraph, or list item with exactly one text child. Core
    /// rejects built-in pages, nested elements, and hidden targets; the host
    /// reviews the proposed text before asking core to mutate the page.
    SetVisibleLeafText {
        tab_id: u64,
        node_id: u64,
        value: String,
    },
    /// Version 9 of `dom:write`: replace the visible textContent of a
    /// rendered heading, paragraph, or list item whose descendants contain
    /// only ordinary inline formatting. Core rejects links, controls,
    /// inline event-handler attributes, hidden targets, and built-in pages
    /// before removing children.
    SetVisibleTextContent {
        tab_id: u64,
        node_id: u64,
        value: String,
    },
    /// Version 5 of `dom:write`: selects one enabled native radio input.
    /// This is intentionally selection-only, never a generic `checked`
    /// setter: core identifies the radio's local group and clears its other
    /// members with native-radio semantics. The request cannot supply a group
    /// name, form owner, or arbitrary attribute mutation.
    SetRadioChecked { tab_id: u64, node_id: u64 },
    /// Version 6 of `dom:write`: selects one enabled option belonging to an
    /// enabled native single-select. The extension supplies only the live
    /// option ID: core derives its owning select and clears the select's other
    /// options. Multiple-select controls and disabled options/groups are not
    /// part of this deliberately narrow first operation.
    SelectOption { tab_id: u64, node_id: u64 },
    /// Version 2 of `network:intercept`: install one declarative rule that
    /// blocks a navigation only when an HTTP(S) URL at its initial request or
    /// a later redirect hop canonically equals `url`. The rule is
    /// connection-scoped in core, so it disappears for future navigations when
    /// the extension disconnects. This is deliberately not a callback,
    /// redirector, header editor, or arbitrary request scripting API.
    RegisterNetworkBlockUrl { url: String },
    /// Version 4 of `network:intercept`: block a canonical ASCII host and
    /// its subdomains at the initial navigation or any redirect target.
    /// Core validates the host independently before installing the rule.
    RegisterNetworkBlockHost { host: String },
    /// Version 5 of `network:intercept`: block navigation to `host` or a
    /// dot-boundary subdomain only when its canonical URL path equals the
    /// literal prefix or begins at the next `/` segment boundary. Query and
    /// fragment do not affect the match. Core validates both fields; there
    /// is no guest-supplied pattern language or request callback.
    RegisterNetworkBlockPathPrefix { host: String, path_prefix: String },
    /// Version 3 of `network:intercept`: remove every exact-URL or host
    /// navigation-block rule owned by this connection. This cannot affect rules installed by a
    /// different extension connection and has no extension-controlled payload,
    /// so it needs no further gatekeeper action review. The same cleanup also
    /// runs automatically when the connection ends.
    ClearNetworkBlockUrls,
    /// Version 1 of `storage`: read one value from the caller extension's
    /// core-owned, per-core-lifetime key-value bucket. The extension identity
    /// comes only from its already-negotiated handshake; it cannot supply a
    /// different bucket ID in this request.
    StorageGet { key: String },
    /// Version 1 of `storage`: set one value in the caller extension's
    /// bounded bucket. This is not an ambient filesystem API and cannot name
    /// a host path, another extension, or a persistence target.
    StorageSet { key: String, value: String },
    /// Version 1 of `storage`: remove one value from the caller extension's
    /// bucket. The reply says whether the key existed, making removal
    /// idempotent without a preceding read.
    StorageRemove { key: String },
    /// Version 2 of `storage`: the same bounded point read, but from a
    /// separate durable namespace keyed only by the handshake identity.
    /// Version-one operations retain their process-lifetime semantics.
    DurableStorageGet { key: String },
    /// Version 2 of `storage`: atomically persist a bounded key/value pair
    /// under a core-selected private directory. No guest path is accepted.
    DurableStorageSet { key: String, value: String },
    /// Version 2 of `storage`: remove one durable value; this does not touch
    /// the version-one process-lifetime bucket.
    DurableStorageRemove { key: String },
    /// Version 3 of `storage`: enumerate only this installed extension's
    /// durable keys, in deterministic lexical order. Values and the separate
    /// process-lifetime v1 namespace are not exposed.
    DurableStorageListKeys,
    /// Registers a network interception rule -- requires the
    /// `network:intercept` capability. Registering interception at all
    /// is high-risk, so the host always routes this request through the
    /// gatekeeper after capability authorization and before any rule is
    /// installed. The eventual declarative rule representation is out
    /// of scope for this minimal protocol slice; the boundary is proved
    /// here with an explicit registration operation rather than an
    /// extension-controlled free-form rule payload.
    NetworkIntercept,
}

/// The risk-relevant target of a [`ExtensionRequest::DomWrite`]. The
/// default preserves wire compatibility with the original, generic
/// `DomWrite { value }` request: an older extension has not represented
/// either a form-input or network-causing action, so it remains a
/// non-triggering document mutation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DomWriteTarget {
    /// A generic document mutation with no form or network effect.
    #[default]
    Document,
    /// Writes a form/input control. `input_type` is structured metadata
    /// (for example `text`, `email`, or `password`), not the field value.
    FormInput { input_type: String },
    /// One reviewed, visible semantic text leaf. The actual proposed text is
    /// carried only by the explicit v8 request, never by legacy DomWrite.
    VisibleTextLeaf,
    /// A reviewed v9 textContent replacement of noninteractive inline markup.
    VisibleTextContent,
    /// Causes a network-facing effect, such as submitting a form. The
    /// short action label is metadata for gatekeeper review.
    NetworkCausing { action: String },
}

impl DomWriteTarget {
    /// Whether this target is in Phase 9's resolved gatekeeper trigger
    /// list.
    pub const fn requires_gatekeeper_review(&self) -> bool {
        matches!(
            self,
            Self::FormInput { .. }
                | Self::NetworkCausing { .. }
                | Self::VisibleTextLeaf
                | Self::VisibleTextContent
        )
    }

    /// A bounded, structured diagnostic for legacy `DomWrite` review. That
    /// generic request excludes its value. The explicit v8 visible-text
    /// v8/v9 visible-text operations instead send their bounded proposed text
    /// through separate fixed action labels before any core mutation.
    pub fn gatekeeper_detail(&self) -> Option<String> {
        match self {
            Self::Document => None,
            Self::FormInput { input_type } => Some(format!(
                "target=form-input; input_type={}",
                safe_metadata_label(input_type)
            )),
            Self::VisibleTextLeaf => Some("target=visible-text-leaf".to_string()),
            Self::VisibleTextContent => Some("target=visible-text-content".to_string()),
            Self::NetworkCausing { action } => Some(format!(
                "target=network-causing; action={}",
                safe_metadata_label(action)
            )),
        }
    }
}

/// Limits extension-controlled action metadata before it crosses into
/// the gatekeeper process. It is intentionally a small ASCII label,
/// not a free-form text channel to an eventual model reviewer.
fn safe_metadata_label(value: &str) -> String {
    let label: String = value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
        .take(64)
        .collect();
    if label.is_empty() {
        "unknown".to_string()
    } else {
        label
    }
}

/// The capability-enforcing side's reply to one [`ExtensionRequest`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ExtensionReply {
    /// Reply to [`ExtensionRequest::Hello`].
    ///
    /// A version mismatch deliberately does not terminate the whole
    /// connection: third-party extensions can use a subset of the API,
    /// so an unsupported `network:intercept` declaration must not stop
    /// an otherwise compatible `dom:read` extension. Each omitted
    /// capability was accepted at its declared version. Later requests
    /// for an entry in `unsupported_capabilities` are denied by the
    /// host, before checking its manifest grant.
    HelloAck {
        unsupported_capabilities: BTreeMap<String, UnsupportedCapabilityVersion>,
    },
    /// Core's response to an authenticated host's
    /// [`ExtensionRequest::RuntimeReady`]. It means the core session now owns
    /// a live `TabManager`, so bounded extension requests can reach it rather
    /// than timing out during pre-frontend process startup.
    RuntimeStart,
    /// One event dequeued in reply to [`ExtensionRequest::NextRuntimeEvent`].
    /// The event payload is core-defined and deliberately contains only an
    /// opaque live tab ID; an extension must use its separately granted
    /// `dom:read` operation to inspect page content.
    RuntimeEvent(ExtensionRuntimeEvent),
    /// Core is ending the private event stream, normally because its frontend
    /// session ended. The host should exit cleanly rather than retrying or
    /// inventing a background lifecycle of its own.
    RuntimeEventStreamClosed,
    /// Reply to a granted [`ExtensionRequest::DomRead`].
    DomReadResult { value: String },
    /// Reply to a granted [`ExtensionRequest::ReadNetworkResponse`].
    NetworkResponseResult { response: Option<NetworkResponseInfo> },
    /// Reply to a granted [`ExtensionRequest::ReadNetworkTrace`].
    NetworkTraceResult { trace: Option<NetworkTraceInfo> },
    /// A bounded native toolbar update or clear was applied by core.
    UiInjectAck,
    /// Reply to a granted [`ExtensionRequest::DomWrite`].
    DomWriteAck,
    /// Reply to a granted `network:intercept` operation. Rule registration is
    /// gatekeeper-cleared; clearing the caller's own connection-scoped rules
    /// needs only ordinary capability/version authorization.
    NetworkInterceptAck,
    /// Reply to a granted [`ExtensionRequest::StorageGet`]. `None` means the
    /// caller's isolated bucket has no such key; it is not an authorization
    /// failure and reveals nothing about another extension's state.
    StorageGetResult { value: Option<String> },
    /// Reply to a granted [`ExtensionRequest::StorageSet`].
    StorageSetAck,
    /// Reply to a granted [`ExtensionRequest::StorageRemove`].
    StorageRemoveAck { removed: bool },
    /// Reply to a granted [`ExtensionRequest::DurableStorageListKeys`].
    StorageKeysResult { keys: Vec<String> },
    /// Authorization (and, where applicable, gatekeeper review) succeeded,
    /// but the host has no concrete implementation for this operation. This
    /// is deliberately distinct from an acknowledgement: the standalone
    /// reference host can use placeholder effects, while a core-backed host
    /// must not claim that a DOM write or interception rule ran when the
    /// current wire shape cannot represent one safely.
    OperationUnavailable { capability: String, reason: String },
    /// A high-risk, otherwise-authorized extension action was rejected
    /// by the Phase 7 gatekeeper, or the gatekeeper was unavailable and
    /// the host therefore failed closed. Kept distinct from
    /// [`Self::CapabilityDenied`]: authorization succeeded, but this
    /// concrete invocation did not clear safety review.
    GatekeeperBlocked {
        capability: String,
        reason: String,
        category: String,
    },
    /// Reply to any capability-checked request the sending extension
    /// isn't granted -- a structured block, not a bare/generic error,
    /// mirroring [`crate::gatekeeper::GatekeeperReply::Rejected`]'s
    /// "structured, not a silent no-op" precedent.
    CapabilityDenied { capability: String, reason: String },
}

/// A core-owned lifecycle notification for one fresh, bounded Wasm reactor
/// invocation. Events carry only opaque tab IDs and do not expose arbitrary
/// frontend state or network hooks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExtensionRuntimeEvent {
    /// A navigation has cleared gatekeeper review, committed to the live tab,
    /// and published its frame. `tab_id` addresses that resulting live tab.
    NavigationCommitted { tab_id: u64 },
    /// A client activated the displayed native extension button for a live
    /// tab. This is not an authenticated human gesture and grants no ambient
    /// or ephemeral authority by itself.
    ToolbarActivated { tab_id: u64 },
    /// Core accepted the current popup's one action button for a live tab.
    /// Like `ToolbarActivated`, this is not an authenticated human gesture.
    PopupActionActivated { tab_id: u64 },
}

/// A capability declaration the host could not negotiate during an
/// [`ExtensionRequest::Hello`] handshake. Kept in the protocol reply so
/// an independently released extension can make an informed fallback
/// choice rather than infer it from a generic connection failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnsupportedCapabilityVersion {
    /// The host does not implement this capability name at all.
    UnknownCapability,
    /// The host implements the capability, but not the declared API
    /// version. Both bounds are inclusive.
    OutsideSupportedRange {
        min_inclusive: u32,
        max_inclusive: u32,
    },
}

pub fn write_extension_request<W: Write>(w: &mut W, msg: &ExtensionRequest) -> io::Result<()> {
    crate::write_framed(w, msg)
}

pub fn read_extension_request<R: Read>(r: &mut R) -> io::Result<ExtensionRequest> {
    let buf = crate::read_frame_bytes(r)?;
    serde_json::from_slice(&buf).map_err(io::Error::other)
}

pub fn write_extension_reply<W: Write>(w: &mut W, msg: &ExtensionReply) -> io::Result<()> {
    crate::write_framed(w, msg)
}

pub fn read_extension_reply<R: Read>(r: &mut R) -> io::Result<ExtensionReply> {
    let buf = crate::read_frame_bytes(r)?;
    serde_json::from_slice(&buf).map_err(io::Error::other)
}

/// Where the capability-enforcing side of this protocol listens, and
/// where an extension process connects by default in production.
/// Mirrors [`crate::gatekeeper::default_gatekeeper_socket_path`]'s exact
/// style (same per-user temp dir convention, so two different users --
/// or two independent BlueIce sessions -- never collide), but a
/// distinct filename: a different process/protocol from either the
/// rendezvous socket or the gatekeeper's own. Production code is the
/// only caller that uses this default directly -- tests thread an
/// explicit socket path through instead, for the same reason
/// `default_gatekeeper_socket_path`'s docs give.
pub fn default_extension_socket_path() -> PathBuf {
    crate::local_socket::default_socket_dir().join("extension-host.sock")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;

    fn sample_capability_versions() -> BTreeMap<String, u32> {
        let mut versions = BTreeMap::new();
        versions.insert("dom:read".to_string(), 1);
        versions
    }

    #[test]
    fn extension_request_round_trips_over_a_real_socket() {
        for req in [
            ExtensionRequest::Hello {
                extension_id: "minimal-slice-extension".to_string(),
                capability_versions: sample_capability_versions(),
            },
            ExtensionRequest::Hello {
                extension_id: "some-other-extension".to_string(),
                capability_versions: BTreeMap::new(),
            },
            ExtensionRequest::HelloAuthenticated {
                extension_id: "core-spawned-extension".to_string(),
                capability_versions: sample_capability_versions(),
                authentication: "not-a-real-secret-in-this-round-trip-test".to_string(),
            },
            ExtensionRequest::RuntimeReady,
            ExtensionRequest::NextRuntimeEvent,
            ExtensionRequest::DomRead,
            ExtensionRequest::DomReadTab { tab_id: 42 },
            ExtensionRequest::ReadNetworkResponse { tab_id: 42 },
            ExtensionRequest::ReadNetworkTrace { tab_id: 42 },
            ExtensionRequest::ShowPopupAction {
                tab_id: 42,
                title: "Tasks".into(),
                body: "Saved locally".into(),
                action_label: "Open".into(),
            },
            ExtensionRequest::DomWrite {
                value: "new content".to_string(),
                target: DomWriteTarget::Document,
            },
            ExtensionRequest::DomWrite {
                value: "secret".to_string(),
                target: DomWriteTarget::FormInput {
                    input_type: "password".to_string(),
                },
            },
            ExtensionRequest::SetTextInputValue {
                tab_id: 42,
                node_id: 99,
                value: "shared value".to_string(),
            },
            ExtensionRequest::SetCheckboxChecked {
                tab_id: 42,
                node_id: 100,
                checked: true,
            },
            ExtensionRequest::SetTextareaValue {
                tab_id: 42,
                node_id: 101,
                value: "multi-line shared value".to_string(),
            },
            ExtensionRequest::SetRangeInputValue {
                tab_id: 42,
                node_id: 104,
                value: 50,
            },
            ExtensionRequest::SetVisibleLeafText {
                tab_id: 42,
                node_id: 105,
                value: "Updated heading".to_string(),
            },
            ExtensionRequest::SetVisibleTextContent {
                tab_id: 42,
                node_id: 106,
                value: "Updated formatted paragraph".to_string(),
            },
            ExtensionRequest::SetRadioChecked {
                tab_id: 42,
                node_id: 102,
            },
            ExtensionRequest::SelectOption {
                tab_id: 42,
                node_id: 103,
            },
            ExtensionRequest::RegisterNetworkBlockUrl {
                url: "https://example.test/private".to_string(),
            },
            ExtensionRequest::RegisterNetworkBlockHost {
                host: "example.test".to_string(),
            },
            ExtensionRequest::RegisterNetworkBlockPathPrefix {
                host: "example.test".to_string(),
                path_prefix: "/private".to_string(),
            },
            ExtensionRequest::ClearNetworkBlockUrls,
            ExtensionRequest::StorageGet {
                key: "task-state".to_string(),
            },
            ExtensionRequest::StorageSet {
                key: "task-state".to_string(),
                value: "complete".to_string(),
            },
            ExtensionRequest::StorageRemove {
                key: "task-state".to_string(),
            },
            ExtensionRequest::DurableStorageGet {
                key: "task-state".to_string(),
            },
            ExtensionRequest::DurableStorageSet {
                key: "task-state".to_string(),
                value: "complete".to_string(),
            },
            ExtensionRequest::DurableStorageRemove {
                key: "task-state".to_string(),
            },
            ExtensionRequest::DurableStorageListKeys,
            ExtensionRequest::NetworkIntercept,
        ] {
            let (mut a, mut b) = UnixStream::pair().unwrap();
            write_extension_request(&mut a, &req).unwrap();
            assert_eq!(read_extension_request(&mut b).unwrap(), req);
        }
    }

    #[test]
    fn full_durable_key_bucket_fits_the_guest_json_copy_bound() {
        let keys: Vec<String> = (0..128)
            .map(|index| format!("{index:03}{}", "a".repeat(MAX_STORAGE_KEY_BYTES - 3)))
            .collect();
        assert!(keys.iter().all(|key| key.len() == MAX_STORAGE_KEY_BYTES));
        assert!(serde_json::to_vec(&keys).unwrap().len() <= MAX_STORAGE_KEYS_JSON_BYTES);
    }

    #[test]
    fn extension_reply_round_trips_over_a_real_socket() {
        for reply in [
            ExtensionReply::HelloAck {
                unsupported_capabilities: BTreeMap::new(),
            },
            ExtensionReply::HelloAck {
                unsupported_capabilities: BTreeMap::from([
                    (
                        "dom:read".to_string(),
                        UnsupportedCapabilityVersion::OutsideSupportedRange {
                            min_inclusive: 1,
                            max_inclusive: 2,
                        },
                    ),
                    (
                        "future:capability".to_string(),
                        UnsupportedCapabilityVersion::UnknownCapability,
                    ),
                ]),
            },
            ExtensionReply::RuntimeStart,
            ExtensionReply::RuntimeEvent(ExtensionRuntimeEvent::NavigationCommitted { tab_id: 42 }),
            ExtensionReply::RuntimeEventStreamClosed,
            ExtensionReply::DomReadResult {
                value: "placeholder".to_string(),
            },
            ExtensionReply::NetworkResponseResult {
                response: Some(NetworkResponseInfo {
                    method: "GET".to_string(),
                    final_url: "https://example.test/final".to_string(),
                    status: 200,
                    content_type: Some("text/html".to_string()),
                }),
            },
            ExtensionReply::NetworkTraceResult {
                trace: Some(NetworkTraceInfo {
                    request_url: "https://example.test/start".to_string(),
                    redirects: vec![NetworkRedirectInfo {
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
                }),
            },
            ExtensionReply::NetworkTraceResult { trace: None },
            ExtensionReply::DomWriteAck,
            ExtensionReply::NetworkInterceptAck,
            ExtensionReply::StorageGetResult {
                value: Some("complete".to_string()),
            },
            ExtensionReply::StorageGetResult { value: None },
            ExtensionReply::StorageKeysResult {
                keys: vec!["alpha".to_string(), "beta".to_string()],
            },
            ExtensionReply::StorageSetAck,
            ExtensionReply::StorageRemoveAck { removed: true },
            ExtensionReply::OperationUnavailable {
                capability: "network:intercept".to_string(),
                reason: "no declarative rule format is available".to_string(),
            },
            ExtensionReply::GatekeeperBlocked {
                capability: "dom:write".to_string(),
                reason: "review rejected the form action".to_string(),
                category: "sensitive-extension-action".to_string(),
            },
            ExtensionReply::CapabilityDenied {
                capability: "dom:write".to_string(),
                reason: "not granted".to_string(),
            },
        ] {
            let (mut a, mut b) = UnixStream::pair().unwrap();
            write_extension_reply(&mut a, &reply).unwrap();
            assert_eq!(read_extension_reply(&mut b).unwrap(), reply);
        }
    }

    #[test]
    fn multiple_requests_can_be_written_and_read_in_sequence_on_one_stream() {
        let mut buf = Vec::new();
        write_extension_request(&mut buf, &ExtensionRequest::DomRead).unwrap();
        write_extension_request(
            &mut buf,
            &ExtensionRequest::DomWrite {
                value: "x".to_string(),
                target: DomWriteTarget::Document,
            },
        )
        .unwrap();
        let mut cursor = std::io::Cursor::new(buf);
        assert_eq!(
            read_extension_request(&mut cursor).unwrap(),
            ExtensionRequest::DomRead
        );
        assert_eq!(
            read_extension_request(&mut cursor).unwrap(),
            ExtensionRequest::DomWrite {
                value: "x".to_string(),
                target: DomWriteTarget::Document,
            }
        );
    }

    #[test]
    fn an_old_dom_write_without_target_defaults_to_a_non_triggering_document_target() {
        let old_json = br#"{"DomWrite":{"value":"legacy"}}"#;
        let mut framed = Vec::new();
        framed.extend_from_slice(&(old_json.len() as u32).to_le_bytes());
        framed.extend_from_slice(old_json);
        assert_eq!(
            read_extension_request(&mut std::io::Cursor::new(framed)).unwrap(),
            ExtensionRequest::DomWrite {
                value: "legacy".to_string(),
                target: DomWriteTarget::Document,
            }
        );
    }

    #[test]
    fn form_network_and_visible_text_writes_require_a_gatekeeper_review() {
        assert!(!DomWriteTarget::Document.requires_gatekeeper_review());
        assert!(DomWriteTarget::FormInput {
            input_type: "email".to_string()
        }
        .requires_gatekeeper_review());
        assert!(DomWriteTarget::NetworkCausing {
            action: "form-submit".to_string()
        }
        .requires_gatekeeper_review());
        assert!(DomWriteTarget::VisibleTextLeaf.requires_gatekeeper_review());
        assert!(DomWriteTarget::VisibleTextContent.requires_gatekeeper_review());
        assert_eq!(DomWriteTarget::Document.gatekeeper_detail(), None);
        assert_eq!(
            DomWriteTarget::FormInput {
                input_type: "password".to_string()
            }
            .gatekeeper_detail(),
            Some("target=form-input; input_type=password".to_string())
        );
        assert_eq!(
            DomWriteTarget::NetworkCausing {
                action: "submit\u{200b}; steal everything".to_string()
            }
            .gatekeeper_detail(),
            Some("target=network-causing; action=submitstealeverything".to_string())
        );
        assert_eq!(DomWriteTarget::VisibleTextLeaf.gatekeeper_detail(), Some("target=visible-text-leaf".to_string()));
        assert_eq!(DomWriteTarget::VisibleTextContent.gatekeeper_detail(), Some("target=visible-text-content".to_string()));
    }

    #[test]
    fn default_extension_socket_path_is_distinct_from_the_other_well_known_socket_paths() {
        // Same per-user-temp-dir convention `blueice-launcher` and
        // `crate::gatekeeper` use for their own sockets, but a distinct
        // filename -- must never resolve to the same path either of
        // those protocols' clients connect to.
        let path = default_extension_socket_path();
        assert_eq!(path.file_name().unwrap(), "extension-host.sock");
        assert_ne!(path.file_name().unwrap(), "core.sock");
        assert_ne!(path.file_name().unwrap(), "ai-gatekeeper.sock");
    }

    #[test]
    fn reading_malformed_json_is_an_error_not_a_panic() {
        let mut buf = Vec::new();
        let bad_payload = b"not json";
        buf.extend_from_slice(&(bad_payload.len() as u32).to_le_bytes());
        buf.extend_from_slice(bad_payload);
        let mut cursor = std::io::Cursor::new(buf);
        assert!(read_extension_request(&mut cursor).is_err());
    }

    #[test]
    fn reading_a_truncated_frame_is_an_error_not_a_panic() {
        let mut buf = Vec::new();
        write_extension_reply(
            &mut buf,
            &ExtensionReply::HelloAck {
                unsupported_capabilities: BTreeMap::new(),
            },
        )
        .unwrap();
        buf.truncate(buf.len() - 1);
        let mut cursor = std::io::Cursor::new(buf);
        assert!(read_extension_reply(&mut cursor).is_err());
    }
}
