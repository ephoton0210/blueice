// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A timeout after a frame prefix must not abandon the pending payload.

use blueice_ipc::{read_server_message_with_ids, write_server_message_with_ids, ServerMessage};
use std::io::{self, Cursor, Read};

struct TimeoutReader {
    bytes: Cursor<Vec<u8>>,
    boundary: u64,
    kind: io::ErrorKind,
    injected: bool,
}

impl Read for TimeoutReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let position = self.bytes.position();
        if !self.injected && position == self.boundary {
            self.injected = true;
            return Err(io::Error::from(self.kind));
        }
        let limit = if !self.injected && position < self.boundary {
            buffer.len().min((self.boundary - position) as usize)
        } else {
            buffer.len()
        };
        self.bytes.read(&mut buffer[..limit])
    }
}

fn reader(boundary: u64, kind: io::ErrorKind) -> TimeoutReader {
    let mut bytes = Vec::new();
    for (tab, request, text) in [(3, 11, "first"), (5, 13, "second")] {
        write_server_message_with_ids(
            &mut bytes,
            Some(tab),
            Some(request),
            &ServerMessage::Dom(text.to_string()),
        )
        .unwrap();
    }
    TimeoutReader {
        bytes: Cursor::new(bytes),
        boundary,
        kind,
        injected: false,
    }
}

fn decode_both(reader: &mut TimeoutReader) {
    for (tab, request, text) in [(3, 11, "first"), (5, 13, "second")] {
        let (actual_tab, actual_request, message) = read_server_message_with_ids(reader).unwrap();
        assert_eq!(actual_tab, Some(tab));
        assert_eq!(actual_request, Some(request));
        let ServerMessage::Dom(actual_text) = message else {
            panic!("expected the complete DOM message: {message:?}");
        };
        assert_eq!(actual_text, text);
    }
    assert!(reader.injected);
    assert_eq!(
        reader.bytes.position() as usize,
        reader.bytes.get_ref().len()
    );
}

#[test]
fn timeout_after_the_length_prefix_preserves_both_frames() {
    for kind in [io::ErrorKind::WouldBlock, io::ErrorKind::TimedOut] {
        decode_both(&mut reader(4, kind));
    }
}

#[test]
fn timeouts_inside_the_prefix_or_payload_preserve_both_frames() {
    for kind in [io::ErrorKind::WouldBlock, io::ErrorKind::TimedOut] {
        for boundary in [2, 7] {
            decode_both(&mut reader(boundary, kind));
        }
    }
}

#[test]
fn a_timeout_before_the_prefix_still_returns_control_to_the_poller() {
    for kind in [io::ErrorKind::WouldBlock, io::ErrorKind::TimedOut] {
        let mut reader = reader(0, kind);
        assert_eq!(
            read_server_message_with_ids(&mut reader)
                .unwrap_err()
                .kind(),
            kind
        );
        assert_eq!(reader.bytes.position(), 0);
        decode_both(&mut reader);
    }
}
