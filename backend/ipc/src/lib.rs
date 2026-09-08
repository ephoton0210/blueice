// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Control-plane IPC protocol shared by `core`'s out-of-process clients
//! (`extension`, `frontend`, and eventually the Phase 5 AI-facing API),
//! per `BROWSER_CORE_PLAN.md` §1 and `research/frontend-ipc.md`.
//!
//! Two logically separate channels, matching what both Chromium
//! (`WidgetHost`/`WidgetInputHandler`/`FrameWidget` vs.
//! `CompositorFrameSink`) and Gecko (`PCompositorBridge` vs.
//! `PWebRenderBridge`) settled on independently, per the research:
//! **control-plane** (this crate) -- small, low-frequency structured
//! messages (navigation, input, lifecycle) -- and **frame-plane** --
//! the actual pixel data, which this crate deliberately never carries
//! (see [`ServerMessage::FrameReady`]: a shared-memory path plus
//! metadata, not bytes). Wire format is length-prefixed JSON: not the
//! most compact framing, but `serde_json` is mature/correct and this is
//! explicitly the "fastest to validate the boundary" reference
//! implementation (`phase-4-human-rendering-path/PLAN.md`) -- swapping
//! in a smaller binary encoding later doesn't change any type in this
//! module, only [`write_message`]/[`read_message`]'s internals.
//!
//! Like `ureq`/`fontdue`/`winit` elsewhere in Phase 4, this crate uses
//! `serde`/`serde_json` rather than hand-rolling encoding -- this is
//! infrastructure (a solved problem), not the parsing/cascade/layout
//! domain this project is written from scratch for.

use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

pub mod ai;
pub mod shm;

pub use ai::{AiNode, AiSnapshot, Bounds, NameFrom, NodeAction, NodeState, Role};

/// The whole-protocol version this build of `blueice-ipc` speaks, per
/// `phase-1-ai-representation-layer/PLAN.md` §3's versioning decision:
/// one coarse version, bumped only on a breaking change (a variant
/// removed/renamed, a field's meaning changed) -- adding a new variant
/// or a new `#[serde(default)]` field does not bump it.
pub const PROTOCOL_VERSION: u32 = 1;

/// Browser-chrome control actions -- distinct from page-content
/// messages (`Navigate`, `ActOn`, ...) per
/// `phase-1-ai-representation-layer/PLAN.md`'s API-shape decision:
/// these operate on the window/`core` instance itself, not on the
/// current page's content, so they get their own nested group rather
/// than sitting as a same-level `ClientMessage` variant indistinguishable
/// from page actions.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ChromeCommand {
    /// Per plan §1: the human-facing window's visibility is a property
    /// of the windowing layer, not of whether `core` exists -- this
    /// message exists so that requirement is something the *protocol*
    /// carries (and an AI-facing client can drive too), not just an
    /// implementation detail private to one frontend's window object.
    SetVisible(bool),
}

/// Sent by a client (`frontend` today; `extension`/AI later) to `core`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ClientMessage {
    /// The protocol_version handshake (`phase-1-ai-representation-
    /// layer/PLAN.md` §3): every independent client sends this
    /// immediately after connecting, before anything else -- `core`
    /// rejects a fresh connection whose first message isn't this, or
    /// whose declared version it doesn't support, with a
    /// [`ServerMessage::Error`] before processing anything else. Once a
    /// connection has passed that initial gate, a later `Hello` (e.g.
    /// one `blueice-launcher`'s broker forwards from a second external
    /// client sharing the same underlying `core` connection) is just
    /// answered again rather than re-gating the whole session, since
    /// tearing down a shared connection over one client's handshake
    /// would end every other client's session too.
    Hello { protocol_version: u32 },
    /// Load a new URL, replacing the current page.
    Navigate { url: String },
    /// The viewport size changed; `core` re-lays-out at the new width.
    Resize { width: u32, height: u32 },
    /// A click at a point in viewport coordinates (post-scroll, i.e.
    /// `(0,0)` is always the top-left of what's currently visible).
    Click { x: f64, y: f64 },
    /// The pointer moved to this viewport point (same coordinate space
    /// as `Click`) -- `core` resolves it to a node the same way `Click`
    /// already does, becoming the single source of truth for "what's
    /// hovered" so both a future `:hover` visual effect and the AI-
    /// facing `NodeState::hovered` field read from the same state
    /// rather than two independently-tracked copies
    /// (`phase-1-ai-representation-layer/PLAN.md` §4).
    Hover { x: f64, y: f64 },
    /// Scroll the viewport by this many CSS pixels (positive = down).
    Scroll { delta_y: f64 },
    /// Requests a fresh [`AiSnapshot`] of the current page, replied to
    /// with [`ServerMessage::Representation`].
    GetRepresentation,
    /// Act on a specific, stably-addressed element -- see
    /// [`NodeAction`]'s own docs for why this is ID-addressed rather
    /// than coordinate-based.
    ActOn { id: u64, action: NodeAction },
    /// Highlights `id` (drawn as an outline derived fresh from that
    /// node's current bounds on every paint) or clears the highlight
    /// (`None`) -- the AI-to-human sync direction plan §1 asks for:
    /// keyed by ID, so it automatically tracks the node through any
    /// layout change instead of a caller having to recompute a screen
    /// rectangle itself.
    Highlight { id: Option<u64> },
    /// Requests a full DOM tree dump (`blueice_dom::dump`'s canonical
    /// text format -- the same one `blueice-testing`'s fixture corpus
    /// checks `blueice-html` against), replied to with
    /// [`ServerMessage::Dom`]. Deliberately separate from
    /// [`ClientMessage::GetRepresentation`]: the AI-facing snapshot
    /// intentionally excludes purely-decorative/non-semantic nodes
    /// (`phase-1-ai-representation-layer/spike.md`), which is exactly
    /// what a structural comparison against a real browser's DOM (the
    /// Chromium differential-testing harness, `TEST_PLAN.md`) needs to
    /// *not* have filtered out.
    GetDom,
    Chrome(ChromeCommand),
    Shutdown,
    /// Catch-all for a variant this build doesn't recognize (e.g. sent
    /// by a newer client than this `core`, or vice versa) -- per plan
    /// §3's versioning decision, an unrecognized *variant* fails soft
    /// (ignored) rather than erroring out the whole connection the way
    /// `serde_json`'s default unrecognized-enum-variant behavior would;
    /// only an unsupported *protocol_version* in [`ClientMessage::Hello`]
    /// is treated as fatal.
    #[serde(other)]
    Unknown,
}

/// Sent by `core` to a client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ServerMessage {
    /// Reply to [`ClientMessage::Hello`]: `protocol_version` is this
    /// `core`'s own, echoed so the client can also self-check
    /// compatibility, not just rely on not having received an
    /// [`ServerMessage::Error`].
    Hello { protocol_version: u32 },
    /// A new frame is available. `shm_path` names a shared-memory-
    /// backed file the client maps read-only; `generation` increases on
    /// every frame so a client can detect and drop stale
    /// notifications (playing the role Chromium's `SyncToken`/Gecko's
    /// fence do for producer/consumer synchronization, per
    /// `research/frontend-ipc.md` §4) without needing its own clock.
    FrameReady { shm_path: String, width: u32, height: u32, generation: u64 },
    /// Navigation finished (or failed) -- `url` is the final URL after
    /// following any redirects.
    Navigated { url: String },
    /// Reply to [`ClientMessage::GetRepresentation`].
    Representation(AiSnapshot),
    /// Reply to [`ClientMessage::GetDom`].
    Dom(String),
    Error { message: String },
    /// See [`ClientMessage::Unknown`] -- the same forward-compatibility
    /// fallback, in the other direction.
    #[serde(other)]
    Unknown,
}

fn write_framed<W: Write, T: Serialize>(w: &mut W, msg: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec(msg).map_err(io::Error::other)?;
    let len = u32::try_from(bytes.len()).map_err(io::Error::other)?;
    w.write_all(&len.to_le_bytes())?;
    w.write_all(&bytes)?;
    w.flush()
}

fn read_frame_bytes<R: Read>(r: &mut R) -> io::Result<Vec<u8>> {
    let mut len_bytes = [0u8; 4];
    r.read_exact(&mut len_bytes)?;
    let len = u32::from_le_bytes(len_bytes) as usize;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

/// Reads one frame and interprets it as a [`ClientEnvelope`], falling
/// back to [`ClientMessage::Unknown`] (preserving `request_id` if it
/// can still be pulled out of the raw JSON) when the bytes are
/// syntactically valid JSON but don't match a known message -- an
/// unrecognized variant tag, carrying data or not, or a recognized tag
/// with fields this build doesn't know about. `#[serde(other)]` alone
/// only covers a *unit*-shaped unrecognized tag (the default
/// externally-tagged JSON representation serializes those as a bare
/// string), since it can't know in advance what shape a genuinely new,
/// data-carrying variant's content should be parsed as. This is
/// `phase-1-ai-representation-layer/PLAN.md` §3's actual fail-soft
/// guarantee: syntactically malformed JSON (truncated, not JSON at
/// all) still surfaces as a real `io::Error` -- only a
/// well-formed-but-unrecognized *message* is swallowed.
fn read_client_envelope<R: Read>(r: &mut R) -> io::Result<ClientEnvelope> {
    let buf = read_frame_bytes(r)?;
    let value: serde_json::Value = serde_json::from_slice(&buf).map_err(io::Error::other)?;
    if let Ok(envelope) = serde_json::from_value::<ClientEnvelope>(value.clone()) {
        return Ok(envelope);
    }
    let request_id = value.get("request_id").and_then(serde_json::Value::as_u64);
    Ok(ClientEnvelope { request_id, message: ClientMessage::Unknown })
}

/// The [`ServerMessage`] counterpart to [`read_client_envelope`].
fn read_server_envelope<R: Read>(r: &mut R) -> io::Result<ServerEnvelope> {
    let buf = read_frame_bytes(r)?;
    let value: serde_json::Value = serde_json::from_slice(&buf).map_err(io::Error::other)?;
    if let Ok(envelope) = serde_json::from_value::<ServerEnvelope>(value.clone()) {
        return Ok(envelope);
    }
    let request_id = value.get("request_id").and_then(serde_json::Value::as_u64);
    Ok(ServerEnvelope { request_id, message: ServerMessage::Unknown })
}

/// A client-generated correlation id, echoed back verbatim on the
/// [`ServerMessage`] reply a given [`ClientMessage`] produces --
/// closes `phase-8-live-core-hotswap/PLAN.md`'s flagged broadcast-
/// misattribution gap: `blueice-launcher`'s broker broadcasts every
/// `ServerMessage` to every connected external client, so a caller
/// multiplexing several in-flight requests over one connection (or
/// simply sharing a connection with other clients) needs a way to
/// tell "the reply to *my* request" apart from "some other client's
/// concurrent traffic." `None` when a caller doesn't need this (every
/// single-request-at-a-time caller, and every existing test).
// Deliberately *not* `#[serde(flatten)]`: several variants of both
// enums are unit (`GetDom`, `Shutdown`, `Unknown`, ...) and serialize
// as a bare JSON string under the default externally-tagged
// representation, which `flatten` can't merge as object keys into the
// parent envelope. A plain nested `message` field has no such
// restriction on the inner value's JSON shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct ClientEnvelope {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    request_id: Option<u64>,
    message: ClientMessage,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct ServerEnvelope {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    request_id: Option<u64>,
    message: ServerMessage,
}

pub fn write_client_message<W: Write>(w: &mut W, msg: &ClientMessage) -> io::Result<()> {
    write_client_message_with_id(w, None, msg)
}

pub fn write_client_message_with_id<W: Write>(w: &mut W, request_id: Option<u64>, msg: &ClientMessage) -> io::Result<()> {
    write_framed(w, &ClientEnvelope { request_id, message: msg.clone() })
}

pub fn read_client_message<R: Read>(r: &mut R) -> io::Result<ClientMessage> {
    Ok(read_client_message_with_id(r)?.1)
}

pub fn read_client_message_with_id<R: Read>(r: &mut R) -> io::Result<(Option<u64>, ClientMessage)> {
    let envelope = read_client_envelope(r)?;
    Ok((envelope.request_id, envelope.message))
}

pub fn write_server_message<W: Write>(w: &mut W, msg: &ServerMessage) -> io::Result<()> {
    write_server_message_with_id(w, None, msg)
}

pub fn write_server_message_with_id<W: Write>(w: &mut W, request_id: Option<u64>, msg: &ServerMessage) -> io::Result<()> {
    write_framed(w, &ServerEnvelope { request_id, message: msg.clone() })
}

pub fn read_server_message<R: Read>(r: &mut R) -> io::Result<ServerMessage> {
    Ok(read_server_message_with_id(r)?.1)
}

pub fn read_server_message_with_id<R: Read>(r: &mut R) -> io::Result<(Option<u64>, ServerMessage)> {
    let envelope = read_server_envelope(r)?;
    Ok((envelope.request_id, envelope.message))
}

/// The client side of the `protocol_version` handshake (`phase-1-ai-
/// representation-layer/PLAN.md` §3): sends [`ClientMessage::Hello`]
/// declaring [`PROTOCOL_VERSION`], then blocks for `core`'s reply.
/// Every independent client -- `frontend`, `blueice-mcp-server`, and
/// `blueice-launcher`'s own connection to the `core` it spawns -- calls
/// this immediately after connecting and before sending anything else,
/// since `core` rejects a fresh connection whose first message isn't
/// `Hello`.
pub fn client_handshake<S: Read + Write>(stream: &mut S) -> io::Result<()> {
    write_client_message(stream, &ClientMessage::Hello { protocol_version: PROTOCOL_VERSION })?;
    match read_server_message(stream)? {
        ServerMessage::Hello { protocol_version } if protocol_version == PROTOCOL_VERSION => Ok(()),
        ServerMessage::Error { message } => Err(io::Error::other(message)),
        other => Err(io::Error::other(format!("expected a Hello handshake reply, got {other:?}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn client_message_round_trips_through_the_wire_format() {
        for msg in [
            ClientMessage::Hello { protocol_version: PROTOCOL_VERSION },
            ClientMessage::Navigate { url: "https://example.com".to_string() },
            ClientMessage::Resize { width: 800, height: 600 },
            ClientMessage::Click { x: 12.5, y: 30.0 },
            ClientMessage::Scroll { delta_y: -40.0 },
            ClientMessage::Chrome(ChromeCommand::SetVisible(false)),
            ClientMessage::Hover { x: 5.0, y: 6.0 },
            ClientMessage::GetRepresentation,
            ClientMessage::ActOn { id: 7, action: NodeAction::Click },
            ClientMessage::Highlight { id: Some(7) },
            ClientMessage::Highlight { id: None },
            ClientMessage::GetDom,
            ClientMessage::Shutdown,
            ClientMessage::Unknown,
        ] {
            let mut buf = Vec::new();
            write_client_message(&mut buf, &msg).unwrap();
            let mut cursor = Cursor::new(buf);
            assert_eq!(read_client_message(&mut cursor).unwrap(), msg);
        }
    }

    #[test]
    fn server_message_round_trips_through_the_wire_format() {
        for msg in [
            ServerMessage::Hello { protocol_version: PROTOCOL_VERSION },
            ServerMessage::FrameReady { shm_path: "/dev/shm/blueice-1".to_string(), width: 800, height: 600, generation: 42 },
            ServerMessage::Navigated { url: "https://example.com/".to_string() },
            ServerMessage::Representation(AiSnapshot {
                generation: 42,
                url: Some("https://example.com/".to_string()),
                scroll_y: 10.0,
                nodes: vec![AiNode {
                    id: 3,
                    parent: None,
                    children: vec![],
                    role: Role::Link,
                    name: Some("Example".to_string()),
                    name_from: Some(NameFrom::Contents),
                    state: NodeState { hovered: true, ..Default::default() },
                    bounds: Bounds { x: 0.0, y: 0.0, width: 10.0, height: 5.0 },
                    opacity: 1.0,
                    occluded: false,
                    occluded_by: None,
                    occluded_fraction: 0.0,
                }],
            }),
            ServerMessage::Dom("| <html>\n".to_string()),
            ServerMessage::Error { message: "oops".to_string() },
            ServerMessage::Unknown,
        ] {
            let mut buf = Vec::new();
            write_server_message(&mut buf, &msg).unwrap();
            let mut cursor = Cursor::new(buf);
            assert_eq!(read_server_message(&mut cursor).unwrap(), msg);
        }
    }

    #[test]
    fn multiple_messages_can_be_written_and_read_in_sequence_on_one_stream() {
        let mut buf = Vec::new();
        write_client_message(&mut buf, &ClientMessage::Resize { width: 1, height: 2 }).unwrap();
        write_client_message(&mut buf, &ClientMessage::Shutdown).unwrap();
        let mut cursor = Cursor::new(buf);
        assert_eq!(read_client_message(&mut cursor).unwrap(), ClientMessage::Resize { width: 1, height: 2 });
        assert_eq!(read_client_message(&mut cursor).unwrap(), ClientMessage::Shutdown);
    }

    #[test]
    fn reading_past_a_truncated_stream_is_an_io_error_not_a_panic() {
        let mut buf = Vec::new();
        write_client_message(&mut buf, &ClientMessage::Shutdown).unwrap();
        buf.truncate(buf.len() - 1); // cut off the last byte of the payload
        let mut cursor = Cursor::new(buf);
        assert!(read_client_message::<_>(&mut cursor).is_err());
    }

    #[test]
    fn reading_from_an_empty_stream_is_an_error() {
        let mut cursor = Cursor::new(Vec::<u8>::new());
        let result: io::Result<ClientMessage> = read_client_message(&mut cursor);
        assert!(result.is_err());
    }

    #[test]
    fn reading_malformed_json_payload_is_an_error() {
        let mut buf = Vec::new();
        let bad_payload = b"not json";
        buf.extend_from_slice(&(bad_payload.len() as u32).to_le_bytes());
        buf.extend_from_slice(bad_payload);
        let mut cursor = Cursor::new(buf);
        let result: io::Result<ClientMessage> = read_client_message(&mut cursor);
        assert!(result.is_err());
    }

    #[test]
    fn real_unix_domain_socket_round_trip() {
        // TEST_PLAN.md's "UI testing strategy": core<->frontend IPC-
        // boundary tests should drive the real protocol with a test
        // client, not just an in-memory buffer -- this is that test,
        // now actionable since Phase 4 is underway.
        use std::os::unix::net::UnixStream;
        let (mut a, mut b) = UnixStream::pair().unwrap();

        let sent = ClientMessage::Navigate { url: "https://example.com".to_string() };
        write_client_message(&mut a, &sent).unwrap();
        let received = read_client_message(&mut b).unwrap();
        assert_eq!(received, sent);

        let sent = ServerMessage::FrameReady { shm_path: "/tmp/x".to_string(), width: 10, height: 20, generation: 1 };
        write_server_message(&mut b, &sent).unwrap();
        let received = read_server_message(&mut a).unwrap();
        assert_eq!(received, sent);
    }

    #[test]
    fn a_message_written_without_a_request_id_reads_back_as_none() {
        let mut buf = Vec::new();
        write_client_message(&mut buf, &ClientMessage::Shutdown).unwrap();
        let mut cursor = Cursor::new(buf);
        assert_eq!(read_client_message_with_id(&mut cursor).unwrap(), (None, ClientMessage::Shutdown));
    }

    #[test]
    fn a_request_id_round_trips_for_both_a_unit_and_a_struct_variant() {
        // Regression coverage for the envelope shape specifically: unit
        // variants (`Shutdown`) serialize as a bare JSON string under
        // the default externally-tagged representation, unlike struct
        // variants (`Navigate`) -- both must still carry a request_id
        // through the same envelope.
        for msg in [ClientMessage::Shutdown, ClientMessage::Navigate { url: "https://example.com".to_string() }] {
            let mut buf = Vec::new();
            write_client_message_with_id(&mut buf, Some(42), &msg).unwrap();
            let mut cursor = Cursor::new(buf);
            assert_eq!(read_client_message_with_id(&mut cursor).unwrap(), (Some(42), msg));
        }

        let mut buf = Vec::new();
        let reply = ServerMessage::Representation(AiSnapshot { generation: 1, url: None, scroll_y: 0.0, nodes: vec![] });
        write_server_message_with_id(&mut buf, Some(7), &reply).unwrap();
        let mut cursor = Cursor::new(buf);
        assert_eq!(read_server_message_with_id(&mut cursor).unwrap(), (Some(7), reply));
    }

    #[test]
    fn an_unrecognized_client_variant_deserializes_as_unknown_rather_than_erroring() {
        let mut buf = Vec::new();
        let bad_payload = br#"{"message":"SomeFutureVariant"}"#;
        buf.extend_from_slice(&(bad_payload.len() as u32).to_le_bytes());
        buf.extend_from_slice(bad_payload);
        let mut cursor = Cursor::new(buf);
        assert_eq!(read_client_message(&mut cursor).unwrap(), ClientMessage::Unknown);
    }

    #[test]
    fn an_unrecognized_server_variant_deserializes_as_unknown_rather_than_erroring() {
        let mut buf = Vec::new();
        let bad_payload = br#"{"message":{"SomeFutureVariant":{"x":1}}}"#;
        buf.extend_from_slice(&(bad_payload.len() as u32).to_le_bytes());
        buf.extend_from_slice(bad_payload);
        let mut cursor = Cursor::new(buf);
        assert_eq!(read_server_message(&mut cursor).unwrap(), ServerMessage::Unknown);
    }

    #[test]
    fn client_handshake_succeeds_against_a_matching_hello_reply() {
        use std::os::unix::net::UnixStream;
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let responder = std::thread::spawn(move || {
            assert_eq!(read_client_message(&mut server).unwrap(), ClientMessage::Hello { protocol_version: PROTOCOL_VERSION });
            write_server_message(&mut server, &ServerMessage::Hello { protocol_version: PROTOCOL_VERSION }).unwrap();
        });
        client_handshake(&mut client).unwrap();
        responder.join().unwrap();
    }

    #[test]
    fn client_handshake_surfaces_an_error_reply_as_an_io_error() {
        use std::os::unix::net::UnixStream;
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let responder = std::thread::spawn(move || {
            let _ = read_client_message(&mut server).unwrap();
            write_server_message(&mut server, &ServerMessage::Error { message: "unsupported protocol_version".to_string() }).unwrap();
        });
        let err = client_handshake(&mut client).unwrap_err();
        assert!(err.to_string().contains("unsupported protocol_version"));
        responder.join().unwrap();
    }

    #[test]
    fn client_handshake_rejects_an_unexpected_reply_type() {
        use std::os::unix::net::UnixStream;
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let responder = std::thread::spawn(move || {
            let _ = read_client_message(&mut server).unwrap();
            write_server_message(&mut server, &ServerMessage::Navigated { url: "about:blank".to_string() }).unwrap();
        });
        assert!(client_handshake(&mut client).is_err());
        responder.join().unwrap();
    }
}
