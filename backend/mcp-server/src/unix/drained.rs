// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A connection that is read all the time, not only while a tool call waits.
//!
//! The launcher's broker sends every client every message the core produces,
//! and gives up on a client that stops taking them: after a few seconds of an
//! unread, full socket it drops the client, and the client's end stays open
//! and silent. An agent pauses between tool calls for far longer than that, and
//! a live page repaints all the while, so a session read only during calls was
//! dropped as soon as the agent thought for a moment, and its next request was
//! never answered.
//!
//! [`DrainedStream`] reads the socket on a thread of its own, parses what
//! arrives and queues it for the tool call that comes next. An unsolicited
//! frame notice (no request id) supersedes the one before it for the same tab,
//! so a long pause leaves the latest picture of each tab and not thousands of
//! stale ones.

use blueice_ipc::ServerMessage;
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Condvar, Mutex};

struct Frame {
    /// Which request this answers, if any (a test waits on it).
    #[cfg_attr(not(test), allow(dead_code))]
    request_id: Option<u64>,
    /// The message as it goes back out to a reader: length-prefixed, exactly as
    /// the wire has it.
    bytes: Vec<u8>,
    /// For an unsolicited frame notice, the tab it is for: a newer one for the
    /// same tab replaces it while it waits.
    supersedable_for: Option<Option<u64>>,
}

#[derive(Default)]
struct State {
    frames: VecDeque<Frame>,
    /// The frame being read out, and how far.
    current: Vec<u8>,
    position: usize,
    closed: bool,
}

#[derive(Default)]
struct Inbox {
    state: Mutex<State>,
    ready: Condvar,
}

/// A [`UnixStream`] that is always being read.
pub struct DrainedStream {
    writer: UnixStream,
    inbox: Arc<Inbox>,
}

impl DrainedStream {
    pub fn new(stream: UnixStream) -> io::Result<Self> {
        let mut reader = stream.try_clone()?;
        let inbox = Arc::new(Inbox::default());
        let theirs = Arc::clone(&inbox);
        std::thread::Builder::new()
            .name("mcp-drain".into())
            .spawn(move || {
                while let Ok((tab_id, request_id, message)) =
                    blueice_ipc::read_server_message_with_ids(&mut reader)
                {
                    let supersedable_for = (request_id.is_none()
                        && matches!(message, ServerMessage::FrameReady { .. }))
                    .then_some(tab_id);
                    let mut bytes = Vec::new();
                    if blueice_ipc::write_server_message_with_ids(
                        &mut bytes,
                        tab_id,
                        request_id,
                        &message,
                    )
                    .is_err()
                    {
                        break;
                    }
                    let mut state = theirs
                        .state
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    if let Some(tab) = supersedable_for {
                        state.frames.retain(|f| f.supersedable_for != Some(tab));
                    }
                    state.frames.push_back(Frame {
                        request_id,
                        bytes,
                        supersedable_for,
                    });
                    theirs.ready.notify_all();
                }
                let mut state = theirs
                    .state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                state.closed = true;
                theirs.ready.notify_all();
            })?;
        Ok(DrainedStream {
            writer: stream,
            inbox,
        })
    }
}

impl DrainedStream {
    /// Waits until a queued message answers `request_id`: everything that
    /// arrived before it has then been taken in.
    #[cfg(test)]
    fn wait_until_queued(&self, request_id: u64) {
        let started = std::time::Instant::now();
        loop {
            let queued = self
                .inbox
                .state
                .lock()
                .unwrap()
                .frames
                .iter()
                .any(|f| f.request_id == Some(request_id));
            if queued {
                return;
            }
            assert!(started.elapsed() < std::time::Duration::from_secs(20), "never queued");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
}

impl Read for DrainedStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut state = self
            .inbox
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        loop {
            if state.position < state.current.len() {
                let n = buf.len().min(state.current.len() - state.position);
                buf[..n].copy_from_slice(&state.current[state.position..state.position + n]);
                state.position += n;
                return Ok(n);
            }
            if let Some(frame) = state.frames.pop_front() {
                state.current = frame.bytes;
                state.position = 0;
                continue;
            }
            if state.closed {
                return Ok(0);
            }
            state = self
                .inbox
                .ready
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }
}

impl Write for DrainedStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.writer.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

impl Drop for DrainedStream {
    fn drop(&mut self) {
        // Ends the reading thread, which holds the other half.
        let _ = self.writer.shutdown(std::net::Shutdown::Both);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn frame(generation: u64) -> ServerMessage {
        ServerMessage::FrameReady {
            shm_path: "/tmp/frame".to_string(),
            width: 10,
            height: 10,
            generation,
        }
    }

    fn read_one(stream: &mut DrainedStream) -> (Option<u64>, Option<u64>, ServerMessage) {
        blueice_ipc::read_server_message_with_ids(stream).unwrap()
    }

    #[test]
    fn a_peer_can_write_far_more_than_a_socket_buffer_holds_while_nobody_reads() {
        let (client, mut broker) = UnixStream::pair().unwrap();
        let mut drained = DrainedStream::new(client).unwrap();
        let writing = std::thread::spawn(move || {
            for generation in 0..5000 {
                blueice_ipc::write_server_message_with_ids(
                    &mut broker,
                    Some(1),
                    None,
                    &frame(generation),
                )
                .unwrap();
            }
            blueice_ipc::write_server_message_with_ids(
                &mut broker,
                Some(1),
                Some(7),
                &ServerMessage::Error {
                    message: "the reply".to_string(),
                },
            )
            .unwrap();
            broker
        });
        let started = Instant::now();
        while !writing.is_finished() {
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "the writer is blocked: nobody drained the socket"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        let _broker = writing.join().unwrap();
        drained.wait_until_queued(7);
        // What waited is the latest picture and then the reply, nothing stale.
        let (tab, request, message) = read_one(&mut drained);
        assert_eq!((tab, request), (Some(1), None));
        assert_eq!(message, frame(4999));
        let (_, request, message) = read_one(&mut drained);
        assert_eq!(request, Some(7));
        assert_eq!(
            message,
            ServerMessage::Error {
                message: "the reply".to_string()
            }
        );
    }

    #[test]
    fn a_frame_notice_only_replaces_the_one_for_its_own_tab_and_never_a_reply() {
        let (client, mut broker) = UnixStream::pair().unwrap();
        let mut drained = DrainedStream::new(client).unwrap();
        for (tab, request, generation) in [
            (1, None, 1),
            (2, None, 2),
            (1, Some(9), 3),
            (1, None, 4),
            (2, None, 5),
        ] {
            blueice_ipc::write_server_message_with_ids(
                &mut broker,
                Some(tab),
                request,
                &frame(generation),
            )
            .unwrap();
        }
        blueice_ipc::write_server_message_with_ids(&mut broker, None, Some(99), &frame(0)).unwrap();
        drained.wait_until_queued(99);
        // A reply to a request arrives in order and is kept even though it is a
        // frame notice; the unsolicited ones collapse to the latest per tab.
        let mut seen = Vec::new();
        for _ in 0..3 {
            let (tab, request, message) = read_one(&mut drained);
            let ServerMessage::FrameReady { generation, .. } = message else {
                panic!("{message:?}")
            };
            seen.push((tab, request, generation));
        }
        seen.sort();
        assert_eq!(
            seen,
            vec![(Some(1), None, 4), (Some(1), Some(9), 3), (Some(2), None, 5)]
        );
    }

    #[test]
    fn what_is_written_reaches_the_peer_and_a_closed_peer_ends_reading() {
        let (client, mut broker) = UnixStream::pair().unwrap();
        let mut drained = DrainedStream::new(client).unwrap();
        blueice_ipc::write_client_message_with_ids(
            &mut drained,
            Some(3),
            Some(11),
            &blueice_ipc::ClientMessage::GetRepresentation,
        )
        .unwrap();
        let (tab, request, message) = blueice_ipc::read_client_message_with_ids(&mut broker).unwrap();
        assert_eq!((tab, request), (Some(3), Some(11)));
        assert_eq!(message, blueice_ipc::ClientMessage::GetRepresentation);

        blueice_ipc::write_server_message_with_ids(&mut broker, None, Some(11), &frame(1)).unwrap();
        drop(broker);
        let (_, request, _) = read_one(&mut drained);
        assert_eq!(request, Some(11));
        let error = blueice_ipc::read_server_message_with_ids(&mut drained).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn dropping_the_stream_closes_the_connection_for_the_peer() {
        let (client, mut broker) = UnixStream::pair().unwrap();
        let drained = DrainedStream::new(client).unwrap();
        drop(drained);
        broker
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut byte = [0u8; 1];
        assert_eq!(broker.read(&mut byte).unwrap(), 0, "the peer sees the end");
    }
}
