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

pub mod shm;

/// Sent by a client (`frontend` today; `extension`/AI later) to `core`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ClientMessage {
    /// Load a new URL, replacing the current page.
    Navigate { url: String },
    /// The viewport size changed; `core` re-lays-out at the new width.
    Resize { width: u32, height: u32 },
    /// A click at a point in viewport coordinates (post-scroll, i.e.
    /// `(0,0)` is always the top-left of what's currently visible).
    Click { x: f64, y: f64 },
    /// Scroll the viewport by this many CSS pixels (positive = down).
    Scroll { delta_y: f64 },
    /// Per plan §1: the human-facing window's visibility is a property
    /// of the windowing layer, not of whether `core` exists -- this
    /// message exists so that requirement is something the *protocol*
    /// carries (and an AI-facing client could drive later), not just an
    /// implementation detail private to one frontend's window object.
    SetVisible(bool),
    Shutdown,
}

/// Sent by `core` to a client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ServerMessage {
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
    Error { message: String },
}

fn write_framed<W: Write, T: Serialize>(w: &mut W, msg: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec(msg).map_err(io::Error::other)?;
    let len = u32::try_from(bytes.len()).map_err(io::Error::other)?;
    w.write_all(&len.to_le_bytes())?;
    w.write_all(&bytes)?;
    w.flush()
}

fn read_framed<R: Read, T: for<'de> Deserialize<'de>>(r: &mut R) -> io::Result<T> {
    let mut len_bytes = [0u8; 4];
    r.read_exact(&mut len_bytes)?;
    let len = u32::from_le_bytes(len_bytes) as usize;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    serde_json::from_slice(&buf).map_err(io::Error::other)
}

pub fn write_client_message<W: Write>(w: &mut W, msg: &ClientMessage) -> io::Result<()> {
    write_framed(w, msg)
}

pub fn read_client_message<R: Read>(r: &mut R) -> io::Result<ClientMessage> {
    read_framed(r)
}

pub fn write_server_message<W: Write>(w: &mut W, msg: &ServerMessage) -> io::Result<()> {
    write_framed(w, msg)
}

pub fn read_server_message<R: Read>(r: &mut R) -> io::Result<ServerMessage> {
    read_framed(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn client_message_round_trips_through_the_wire_format() {
        for msg in [
            ClientMessage::Navigate { url: "https://example.com".to_string() },
            ClientMessage::Resize { width: 800, height: 600 },
            ClientMessage::Click { x: 12.5, y: 30.0 },
            ClientMessage::Scroll { delta_y: -40.0 },
            ClientMessage::SetVisible(false),
            ClientMessage::Shutdown,
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
            ServerMessage::FrameReady { shm_path: "/dev/shm/blueice-1".to_string(), width: 800, height: 600, generation: 42 },
            ServerMessage::Navigated { url: "https://example.com/".to_string() },
            ServerMessage::Error { message: "oops".to_string() },
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
}
