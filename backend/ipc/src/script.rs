// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The internal wire protocol between `core` and an out-of-process
//! `bluejs` script host, per `phase-13-bluejs-engine/PLAN.md`'s
//! "Wiring design (resolved 2026-09-08)": BlueJS is isolated the same
//! way `extension`/`ai-gatekeeper` are (a script crash/hang must never
//! be able to take `core` down), so every DOM operation script performs
//! crosses this wire rather than touching `core`'s state in-process.
//!
//! Lives inside `blueice-ipc` for the same reason [`crate::extension`]
//! and [`crate::gatekeeper`] do: to reuse this crate's private
//! length-prefixed-JSON framing primitives ([`crate::write_framed`]/
//! [`crate::read_frame_bytes`]) without changing their visibility.
//!
//! A new, separate module rather than new variants on
//! [`crate::extension`]'s `ExtensionRequest`/`ExtensionReply`: folding
//! script's DOM-call traffic in there would force every extension-side
//! match arm to also cover BlueJS's much higher-frequency,
//! language-runtime-shaped requests (property get/set, node creation),
//! which have nothing to do with capability grants.
//!
//! Like [`crate::extension`] (and unlike [`crate::gatekeeper`]'s
//! one-shot connections), a `bluejs` connection is long-lived and
//! stateful, so it gets the same `Hello`-handshake-then-long-lived-
//! connection shape.
//!
//! **Scope of this minimal slice.** [`ScriptRequest`] covers only
//! enough DOM operations to prove the mechanism end to end and to
//! satisfy `phase-2-mvp-scope/PLAN.md`'s interactive-JS acceptance bar
//! (element lookup, node creation, tree mutation, text-content
//! get/set) -- the same "mechanism real, a deliberately small starting
//! vocabulary" scoping [`crate::extension`]'s own minimal slice used
//! (`DomRead`/`DomWrite` only, out of Phase 9's full six-capability
//! list). The rest of Phase 2's MVP DOM-binding surface (attributes,
//! `classList`, inline `style`, event listeners, `value`/`checked`,
//! `setTimeout`) is added incrementally as BlueJS's own implementation
//! actually needs each one, not enumerated speculatively here.
//! Likewise, the design doc's batching (`Batch(Vec<DomOp>)`) and
//! shared-memory read fast path are explicitly additive follow-ups on
//! top of this plain request/reply shape, not part of this slice.

use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
use std::path::PathBuf;

/// One message the `bluejs` script host sends to `core` over a
/// long-lived connection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ScriptRequest {
    /// Sent once, first, on a fresh connection -- the same
    /// `Hello`-handshake shape [`crate::extension::ExtensionRequest::Hello`]
    /// uses, layered onto `gatekeeper`'s framing style, per this
    /// module's own docs above.
    Hello,
    /// `document.getElementById(id)`, scoped to one tab.
    GetElementById { tab_id: u64, id: String },
    /// `document.createElement(tag_name)`, scoped to one tab. Creates
    /// the node but does not attach it anywhere -- a script must still
    /// place it with [`ScriptRequest::AppendChild`] or the like.
    CreateElement { tab_id: u64, tag_name: String },
    /// `document.createTextNode(data)`, scoped to one tab.
    CreateTextNode { tab_id: u64, data: String },
    /// `parent.appendChild(child)`, scoped to one tab. Node identity
    /// (`parent`/`child`) is `blueice_dom::NodeId`'s raw value, the
    /// same convention [`crate::ai::AiNode::id`] already uses for the
    /// same reason: this crate doesn't depend on `blueice-dom` for a
    /// plain numeric handle.
    AppendChild { tab_id: u64, parent: u64, child: u64 },
    /// `node.textContent` getter, scoped to one tab.
    GetTextContent { tab_id: u64, node: u64 },
    /// `node.textContent` setter, scoped to one tab.
    SetTextContent { tab_id: u64, node: u64, value: String },
}

/// `core`'s reply to one [`ScriptRequest`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ScriptReply {
    /// Reply to [`ScriptRequest::Hello`].
    HelloAck,
    /// Reply to [`ScriptRequest::GetElementById`] -- `None` if no
    /// element with that ID exists in the addressed tab, matching
    /// `document.getElementById`'s own `null`-on-miss behavior rather
    /// than treating a miss as an error.
    Node { node: Option<u64> },
    /// Reply to [`ScriptRequest::CreateElement`]/
    /// [`ScriptRequest::CreateTextNode`] -- creation is infallible
    /// given a valid tab, so this carries the new node's ID directly
    /// rather than an `Option`.
    NodeCreated { node: u64 },
    /// Reply to [`ScriptRequest::AppendChild`]/
    /// [`ScriptRequest::SetTextContent`] -- both are plain mutations
    /// with no meaningful success payload.
    Ack,
    /// Reply to [`ScriptRequest::GetTextContent`].
    Text { value: String },
    /// Reply to any request naming a tab or node that doesn't exist --
    /// a structured error, mirroring
    /// [`crate::extension::ExtensionReply::CapabilityDenied`]'s
    /// "structured, not a silent no-op" precedent.
    Error { message: String },
}

pub fn write_script_request<W: Write>(w: &mut W, msg: &ScriptRequest) -> io::Result<()> {
    crate::write_framed(w, msg)
}

pub fn read_script_request<R: Read>(r: &mut R) -> io::Result<ScriptRequest> {
    let buf = crate::read_frame_bytes(r)?;
    serde_json::from_slice(&buf).map_err(io::Error::other)
}

pub fn write_script_reply<W: Write>(w: &mut W, msg: &ScriptReply) -> io::Result<()> {
    crate::write_framed(w, msg)
}

pub fn read_script_reply<R: Read>(r: &mut R) -> io::Result<ScriptReply> {
    let buf = crate::read_frame_bytes(r)?;
    serde_json::from_slice(&buf).map_err(io::Error::other)
}

/// Where `core` listens for a `bluejs` script host connection, and
/// where `bluejs` connects by default in production. Mirrors
/// [`crate::extension::default_extension_socket_path`]'s exact style
/// (same per-user temp dir convention, so two different users -- or two
/// independent BlueIce sessions -- never collide), but a distinct
/// filename. Production code is the only caller that uses this default
/// directly -- tests thread an explicit socket path through instead,
/// for the same reason the other `default_*_socket_path` functions'
/// docs give.
pub fn default_script_socket_path() -> PathBuf {
    let dir = match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) => PathBuf::from(dir).join("blueice"),
        None => std::env::temp_dir().join(format!("blueice-{}", unsafe { libc_getuid() })),
    };
    dir.join("bluejs.sock")
}

// Duplicated from `blueice-launcher`'s (and `crate::gatekeeper`'s,
// `crate::extension`'s) identical helper rather than shared via a
// common dependency -- see those crates' own docs for why (~10 lines,
// and each socket-path helper is otherwise independent enough that
// sharing would be more indirection than the duplication costs).
unsafe fn libc_getuid() -> u32 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| status.lines().find_map(|line| line.strip_prefix("Uid:")).and_then(|rest| rest.split_whitespace().next()).and_then(|s| s.parse().ok()))
        .unwrap_or_else(std::process::id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;

    #[test]
    fn script_request_round_trips_over_a_real_socket() {
        for req in [
            ScriptRequest::Hello,
            ScriptRequest::GetElementById { tab_id: 1, id: "widget".to_string() },
            ScriptRequest::CreateElement { tab_id: 1, tag_name: "li".to_string() },
            ScriptRequest::CreateTextNode { tab_id: 1, data: "hello".to_string() },
            ScriptRequest::AppendChild { tab_id: 1, parent: 10, child: 11 },
            ScriptRequest::GetTextContent { tab_id: 1, node: 10 },
            ScriptRequest::SetTextContent { tab_id: 1, node: 10, value: "updated".to_string() },
        ] {
            let (mut a, mut b) = UnixStream::pair().unwrap();
            write_script_request(&mut a, &req).unwrap();
            assert_eq!(read_script_request(&mut b).unwrap(), req);
        }
    }

    #[test]
    fn script_reply_round_trips_over_a_real_socket() {
        for reply in [
            ScriptReply::HelloAck,
            ScriptReply::Node { node: Some(42) },
            ScriptReply::Node { node: None },
            ScriptReply::NodeCreated { node: 43 },
            ScriptReply::Ack,
            ScriptReply::Text { value: "hello".to_string() },
            ScriptReply::Error { message: "no such tab".to_string() },
        ] {
            let (mut a, mut b) = UnixStream::pair().unwrap();
            write_script_reply(&mut a, &reply).unwrap();
            assert_eq!(read_script_reply(&mut b).unwrap(), reply);
        }
    }

    #[test]
    fn multiple_requests_can_be_written_and_read_in_sequence_on_one_stream() {
        let mut buf = Vec::new();
        write_script_request(&mut buf, &ScriptRequest::GetElementById { tab_id: 1, id: "a".to_string() }).unwrap();
        write_script_request(&mut buf, &ScriptRequest::GetElementById { tab_id: 1, id: "b".to_string() }).unwrap();
        let mut cursor = std::io::Cursor::new(buf);
        assert_eq!(read_script_request(&mut cursor).unwrap(), ScriptRequest::GetElementById { tab_id: 1, id: "a".to_string() });
        assert_eq!(read_script_request(&mut cursor).unwrap(), ScriptRequest::GetElementById { tab_id: 1, id: "b".to_string() });
    }

    #[test]
    fn default_script_socket_path_is_distinct_from_the_other_well_known_socket_paths() {
        // Same per-user-temp-dir convention the other `default_*_socket_path`
        // functions use, but a distinct filename -- must never resolve to
        // the same path any other protocol's clients connect to.
        let path = default_script_socket_path();
        assert_eq!(path.file_name().unwrap(), "bluejs.sock");
        assert_ne!(path.file_name().unwrap(), "core.sock");
        assert_ne!(path.file_name().unwrap(), "ai-gatekeeper.sock");
        assert_ne!(path.file_name().unwrap(), "extension-host.sock");
    }

    #[test]
    fn reading_malformed_json_is_an_error_not_a_panic() {
        let mut buf = Vec::new();
        let bad_payload = b"not json";
        buf.extend_from_slice(&(bad_payload.len() as u32).to_le_bytes());
        buf.extend_from_slice(bad_payload);
        let mut cursor = std::io::Cursor::new(buf);
        assert!(read_script_request(&mut cursor).is_err());
    }

    #[test]
    fn reading_a_truncated_frame_is_an_error_not_a_panic() {
        let mut buf = Vec::new();
        write_script_reply(&mut buf, &ScriptReply::HelloAck).unwrap();
        buf.truncate(buf.len() - 1);
        let mut cursor = std::io::Cursor::new(buf);
        assert!(read_script_reply(&mut cursor).is_err());
    }
}
