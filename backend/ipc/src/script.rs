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
//! connection shape. This v2 document-generation and byte-bound update is
//! still a proof-of-mechanism channel, not an authenticated child DOM bridge:
//! the launcher does not supply a child-only token or service reentrant calls
//! during page-host execution yet. Do not expose it to a page VM as authority.
//!
//! Each DOM request names the exact core-owned document generation as well as
//! its tab. A request from a predecessor document fails before lookup or
//! mutation, including operations that create new nodes and carry no old
//! `NodeId`. The current socket's `Hello` is not yet child authentication.
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

/// The old tab-only DOM request shapes must not be decoded as current-page
/// authority after a document replacement.
pub const SCRIPT_PROTOCOL_VERSION: u32 = 2;
pub const SCRIPT_MAX_FRAME_BYTES: usize = 1_100_000;
pub const SCRIPT_MAX_NAME_BYTES: usize = 4_096;
pub const SCRIPT_MAX_TEXT_BYTES: usize = 1_048_576;

/// Core-owned document identity. A child must carry the generation it was
/// admitted under on every DOM call; a tab ID alone never names a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptDocumentTarget {
    pub tab_id: u64,
    pub document_generation: u64,
}

/// One message the `bluejs` script host sends to `core` over a
/// long-lived connection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ScriptRequest {
    /// Sent once, first, on a fresh connection -- the same
    /// `Hello`-handshake shape [`crate::extension::ExtensionRequest::Hello`]
    /// uses, layered onto `gatekeeper`'s framing style, per this
    /// module's own docs above.
    Hello { protocol_version: u32 },
    /// `document.getElementById(id)`, scoped to one tab.
    GetElementById {
        target: ScriptDocumentTarget,
        id: String,
    },
    /// `document.createElement(tag_name)`, scoped to one tab. Creates
    /// the node but does not attach it anywhere -- a script must still
    /// place it with [`ScriptRequest::AppendChild`] or the like.
    CreateElement {
        target: ScriptDocumentTarget,
        tag_name: String,
    },
    /// `document.createTextNode(data)`, scoped to one tab.
    CreateTextNode {
        target: ScriptDocumentTarget,
        data: String,
    },
    /// `parent.appendChild(child)`, scoped to one tab. Node identity
    /// (`parent`/`child`) is `blueice_dom::NodeId`'s raw value, the
    /// same convention [`crate::ai::AiNode::id`] already uses for the
    /// same reason: this crate doesn't depend on `blueice-dom` for a
    /// plain numeric handle.
    AppendChild {
        target: ScriptDocumentTarget,
        parent: u64,
        child: u64,
    },
    /// `node.textContent` getter, scoped to one tab.
    GetTextContent {
        target: ScriptDocumentTarget,
        node: u64,
    },
    /// `node.textContent` setter, scoped to one tab.
    SetTextContent {
        target: ScriptDocumentTarget,
        node: u64,
        value: String,
    },
}

/// `core`'s reply to one [`ScriptRequest`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ScriptReply {
    /// Reply to [`ScriptRequest::Hello`].
    HelloAck { protocol_version: u32 },
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
    crate::write_framed_with_limit(w, msg, SCRIPT_MAX_FRAME_BYTES)
}

pub fn read_script_request<R: Read>(r: &mut R) -> io::Result<ScriptRequest> {
    let buf = crate::read_frame_bytes_with_limit(r, SCRIPT_MAX_FRAME_BYTES)?;
    serde_json::from_slice(&buf).map_err(io::Error::other)
}

pub fn write_script_reply<W: Write>(w: &mut W, msg: &ScriptReply) -> io::Result<()> {
    crate::write_framed_with_limit(w, msg, SCRIPT_MAX_FRAME_BYTES)
}

pub fn read_script_reply<R: Read>(r: &mut R) -> io::Result<ScriptReply> {
    let buf = crate::read_frame_bytes_with_limit(r, SCRIPT_MAX_FRAME_BYTES)?;
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
        .and_then(|status| {
            status
                .lines()
                .find_map(|line| line.strip_prefix("Uid:"))
                .and_then(|rest| rest.split_whitespace().next())
                .and_then(|s| s.parse().ok())
        })
        .unwrap_or_else(std::process::id)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;

    #[test]
    fn script_request_round_trips_over_a_real_socket() {
        let target = ScriptDocumentTarget {
            tab_id: 1,
            document_generation: 2,
        };
        for req in [
            ScriptRequest::Hello {
                protocol_version: SCRIPT_PROTOCOL_VERSION,
            },
            ScriptRequest::GetElementById {
                target,
                id: "widget".to_string(),
            },
            ScriptRequest::CreateElement {
                target,
                tag_name: "li".to_string(),
            },
            ScriptRequest::CreateTextNode {
                target,
                data: "hello".to_string(),
            },
            ScriptRequest::AppendChild {
                target,
                parent: 10,
                child: 11,
            },
            ScriptRequest::GetTextContent { target, node: 10 },
            ScriptRequest::SetTextContent {
                target,
                node: 10,
                value: "updated".to_string(),
            },
        ] {
            let (mut a, mut b) = UnixStream::pair().unwrap();
            write_script_request(&mut a, &req).unwrap();
            assert_eq!(read_script_request(&mut b).unwrap(), req);
        }
    }

    #[test]
    fn script_reply_round_trips_over_a_real_socket() {
        for reply in [
            ScriptReply::HelloAck {
                protocol_version: SCRIPT_PROTOCOL_VERSION,
            },
            ScriptReply::Node { node: Some(42) },
            ScriptReply::Node { node: None },
            ScriptReply::NodeCreated { node: 43 },
            ScriptReply::Ack,
            ScriptReply::Text {
                value: "hello".to_string(),
            },
            ScriptReply::Error {
                message: "no such tab".to_string(),
            },
        ] {
            let (mut a, mut b) = UnixStream::pair().unwrap();
            write_script_reply(&mut a, &reply).unwrap();
            assert_eq!(read_script_reply(&mut b).unwrap(), reply);
        }
    }

    #[test]
    fn multiple_requests_can_be_written_and_read_in_sequence_on_one_stream() {
        let target = ScriptDocumentTarget {
            tab_id: 1,
            document_generation: 2,
        };
        let mut buf = Vec::new();
        write_script_request(
            &mut buf,
            &ScriptRequest::GetElementById {
                target,
                id: "a".to_string(),
            },
        )
        .unwrap();
        write_script_request(
            &mut buf,
            &ScriptRequest::GetElementById {
                target,
                id: "b".to_string(),
            },
        )
        .unwrap();
        let mut cursor = std::io::Cursor::new(buf);
        assert_eq!(
            read_script_request(&mut cursor).unwrap(),
            ScriptRequest::GetElementById {
                target,
                id: "a".to_string()
            }
        );
        assert_eq!(
            read_script_request(&mut cursor).unwrap(),
            ScriptRequest::GetElementById {
                target,
                id: "b".to_string()
            }
        );
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
    fn legacy_dom_request_without_document_generation_is_rejected() {
        let mut frame = Vec::new();
        crate::write_framed(
            &mut frame,
            &serde_json::json!({
                "CreateTextNode": { "tab_id": 1, "data": "stale" }
            }),
        )
        .unwrap();
        assert!(read_script_request(&mut std::io::Cursor::new(frame)).is_err());
    }

    #[test]
    fn reading_a_truncated_frame_is_an_error_not_a_panic() {
        let mut buf = Vec::new();
        write_script_reply(
            &mut buf,
            &ScriptReply::HelloAck {
                protocol_version: SCRIPT_PROTOCOL_VERSION,
            },
        )
        .unwrap();
        buf.truncate(buf.len() - 1);
        let mut cursor = std::io::Cursor::new(buf);
        assert!(read_script_reply(&mut cursor).is_err());
    }

    #[test]
    fn oversized_script_frame_is_rejected_before_payload_allocation() {
        let mut frame = std::io::Cursor::new(
            u32::try_from(SCRIPT_MAX_FRAME_BYTES + 1)
                .unwrap()
                .to_le_bytes()
                .to_vec(),
        );
        assert_eq!(
            read_script_request(&mut frame).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        let mut output = Vec::new();
        assert_eq!(
            write_script_request(
                &mut output,
                &ScriptRequest::CreateTextNode {
                    target: ScriptDocumentTarget {
                        tab_id: 1,
                        document_generation: 1,
                    },
                    data: "x".repeat(SCRIPT_MAX_FRAME_BYTES),
                },
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::InvalidInput
        );
        assert!(output.is_empty());
    }
}
