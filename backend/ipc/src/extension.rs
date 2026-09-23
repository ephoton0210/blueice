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

/// Maximum UTF-8 URL accepted by the first declarative network rule. Keeping
/// a distinct, small bound means a rule cannot turn the extension socket into
/// a route for large arbitrary payloads. Core also parses and canonicalizes it
/// before it becomes active.
pub const MAX_NETWORK_BLOCK_URL_BYTES: usize = 2 * 1024;

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
    /// Version 3 of `network:intercept`: remove every exact navigation-block
    /// rule owned by this connection. This cannot affect rules installed by a
    /// different extension connection and has no extension-controlled payload,
    /// so it needs no further gatekeeper action review. The same cleanup also
    /// runs automatically when the connection ends.
    ClearNetworkBlockUrls,
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
    /// Causes a network-facing effect, such as submitting a form. The
    /// short action label is metadata for gatekeeper review.
    NetworkCausing { action: String },
}

impl DomWriteTarget {
    /// Whether this target is in Phase 9's resolved gatekeeper trigger
    /// list.
    pub const fn requires_gatekeeper_review(&self) -> bool {
        matches!(self, Self::FormInput { .. } | Self::NetworkCausing { .. })
    }

    /// A bounded, structured diagnostic passed to the gatekeeper. The
    /// extension-provided write value itself is intentionally excluded:
    /// the reviewer needs the action class, not arbitrary untrusted text
    /// that could become a second prompt-injection surface.
    pub fn gatekeeper_detail(&self) -> Option<String> {
        match self {
            Self::Document => None,
            Self::FormInput { input_type } => Some(format!(
                "target=form-input; input_type={}",
                safe_metadata_label(input_type)
            )),
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
    /// Reply to a granted [`ExtensionRequest::DomWrite`].
    DomWriteAck,
    /// Reply to a granted `network:intercept` operation. Rule registration is
    /// gatekeeper-cleared; clearing the caller's own connection-scoped rules
    /// needs only ordinary capability/version authorization.
    NetworkInterceptAck,
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
/// invocation. This protocol intentionally starts with just an after-commit
/// navigation event; it does not expose arbitrary frontend or network hooks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExtensionRuntimeEvent {
    /// A navigation has cleared gatekeeper review, committed to the live tab,
    /// and published its frame. `tab_id` addresses that resulting live tab.
    NavigationCommitted { tab_id: u64 },
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
            ExtensionRequest::ClearNetworkBlockUrls,
            ExtensionRequest::NetworkIntercept,
        ] {
            let (mut a, mut b) = UnixStream::pair().unwrap();
            write_extension_request(&mut a, &req).unwrap();
            assert_eq!(read_extension_request(&mut b).unwrap(), req);
        }
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
            ExtensionReply::DomWriteAck,
            ExtensionReply::NetworkInterceptAck,
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
    fn only_form_input_and_network_causing_writes_require_a_gatekeeper_review() {
        assert!(!DomWriteTarget::Document.requires_gatekeeper_review());
        assert!(DomWriteTarget::FormInput {
            input_type: "email".to_string()
        }
        .requires_gatekeeper_review());
        assert!(DomWriteTarget::NetworkCausing {
            action: "form-submit".to_string()
        }
        .requires_gatekeeper_review());
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
