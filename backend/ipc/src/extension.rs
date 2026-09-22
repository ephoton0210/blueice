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

/// One message an extension process sends to the capability-enforcing
/// side of this protocol.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ExtensionRequest {
    /// Sent once, first, on a fresh connection -- unlike
    /// [`crate::gatekeeper`]'s handshake-less one-shot protocol, this
    /// one is long-lived, so it needs an explicit first-message
    /// handshake the same way [`crate::ClientMessage::Hello`] is for
    /// the external client protocol. `extension_id` is, for this
    /// minimal slice, an author-chosen string an extension declares
    /// itself -- `phase-9-extension-protocol/PLAN.md`'s own "Wiring
    /// design" flags that a real, non-spoofable identity (derived from
    /// a hash of the manifest + WASM module) is still-open future work,
    /// not solved here; see `blueice_extension_host::ExtensionRegistry`'s
    /// own docs.
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
    /// Query the (for this minimal slice, placeholder) AI-facing
    /// representation, read-only -- requires the `dom:read` capability.
    /// No fields: this minimal slice doesn't wire a real target/query
    /// shape yet (see [`ExtensionReply::DomReadResult`]'s docs for why
    /// its value is a placeholder, not real `Page` state).
    DomRead,
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
    /// Reply to a granted [`ExtensionRequest::DomRead`].
    DomReadResult { value: String },
    /// Reply to a granted [`ExtensionRequest::DomWrite`].
    DomWriteAck,
    /// Reply to a granted and gatekeeper-cleared
    /// [`ExtensionRequest::NetworkIntercept`] registration.
    NetworkInterceptAck,
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
            ExtensionRequest::DomRead,
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
            ExtensionReply::DomReadResult {
                value: "placeholder".to_string(),
            },
            ExtensionReply::DomWriteAck,
            ExtensionReply::NetworkInterceptAck,
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
