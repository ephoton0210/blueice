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
    /// -- the two-layer versioning design's second layer. Real
    /// per-capability `[min, max]` window enforcement against these
    /// declared versions is explicit future work (see the plan doc's
    /// "Wiring design" section); this minimal slice's handshake accepts
    /// any declared versions without checking them.
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
    /// real write payload eventually looks like.
    DomWrite { value: String },
}

/// The capability-enforcing side's reply to one [`ExtensionRequest`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ExtensionReply {
    /// Reply to [`ExtensionRequest::Hello`].
    HelloAck,
    /// Reply to a granted [`ExtensionRequest::DomRead`].
    DomReadResult { value: String },
    /// Reply to a granted [`ExtensionRequest::DomWrite`].
    DomWriteAck,
    /// Reply to any capability-checked request the sending extension
    /// isn't granted -- a structured block, not a bare/generic error,
    /// mirroring [`crate::gatekeeper::GatekeeperReply::Rejected`]'s
    /// "structured, not a silent no-op" precedent.
    CapabilityDenied { capability: String, reason: String },
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
    let dir = match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) => PathBuf::from(dir).join("blueice"),
        None => std::env::temp_dir().join(format!("blueice-{}", unsafe { libc_getuid() })),
    };
    dir.join("extension-host.sock")
}

// Duplicated from `blueice-launcher`'s (and `crate::gatekeeper`'s)
// identical helper rather than shared via a common dependency -- see
// those crates' own docs for why (~10 lines, and each socket-path
// helper is otherwise independent enough that sharing would be more
// indirection than the duplication costs).
unsafe fn libc_getuid() -> u32 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status
                .lines()
                .find_map(|line| line.strip_prefix("Uid:"))
                .and_then(|rest| rest.split_whitespace().next())
                .and_then(|s| s.parse().ok())
        })
        .unwrap_or_else(std::process::id)
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
            },
        ] {
            let (mut a, mut b) = UnixStream::pair().unwrap();
            write_extension_request(&mut a, &req).unwrap();
            assert_eq!(read_extension_request(&mut b).unwrap(), req);
        }
    }

    #[test]
    fn extension_reply_round_trips_over_a_real_socket() {
        for reply in [
            ExtensionReply::HelloAck,
            ExtensionReply::DomReadResult {
                value: "placeholder".to_string(),
            },
            ExtensionReply::DomWriteAck,
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
                value: "x".to_string()
            }
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
        write_extension_reply(&mut buf, &ExtensionReply::HelloAck).unwrap();
        buf.truncate(buf.len() - 1);
        let mut cursor = std::io::Cursor::new(buf);
        assert!(read_extension_reply(&mut cursor).is_err());
    }
}
