// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed in-memory framing cases; no socket, process or resource-limit probes.

use super::*;
use std::cell::Cell;

#[test]
fn framing_storage_matches_existing_unicode_wire_bytes() {
    let message = vec!["kept", "☃", "\"quoted\""];
    let expected = serde_json::to_vec(&message).unwrap();
    let mut frame = Vec::new();
    write_framed_with_limit(&mut frame, &message, MAX_FRAME_BYTES).unwrap();
    assert_eq!(&frame[..4], &(expected.len() as u32).to_le_bytes());
    assert_eq!(&frame[4..], expected);
}

#[test]
fn framing_storage_page_reply_encodes_into_a_supplied_buffer() {
    let reply = page_host::PageHostReply::ShutdownAck;
    let length = page_host::page_host_reply_frame_len(&reply).unwrap();
    let mut frame = vec![0; length];
    page_host::encode_page_host_reply_into(&reply, &mut frame).unwrap();
    assert_eq!(
        page_host::read_page_host_reply(&mut std::io::Cursor::new(frame)).unwrap(),
        reply
    );
}

struct Refused;
impl Serialize for Refused {
    fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom("fixed serializer refusal"))
    }
}
#[test]
fn framing_storage_serializer_refusal_publishes_no_prefix() {
    let mut wire = Vec::new();
    let error = write_framed_with_limit(&mut wire, &Refused, MAX_FRAME_BYTES).unwrap_err();
    assert!(error.to_string().contains("fixed serializer refusal"));
    assert!(wire.is_empty());
}

struct Changes(Cell<bool>);
impl Serialize for Changes {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(if self.0.replace(true) { "x" } else { "fixed" })
    }
}
#[test]
fn framing_storage_changed_serialization_publishes_no_prefix() {
    let mut wire = Vec::new();
    assert!(
        write_framed_with_limit(&mut wire, &Changes(Cell::new(false)), MAX_FRAME_BYTES).is_err()
    );
    assert!(wire.is_empty());
}

struct Broken {
    bytes: Vec<u8>,
    flushed: bool,
}
impl Write for Broken {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.bytes.is_empty() {
            self.bytes.extend_from_slice(&bytes[..2]);
            Ok(2)
        } else {
            Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "fixed sink refusal",
            ))
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        self.flushed = true;
        Ok(())
    }
}
#[test]
fn framing_storage_write_failure_never_flushes() {
    let mut writer = Broken {
        bytes: Vec::new(),
        flushed: false,
    };
    assert_eq!(
        write_framed_with_limit(&mut writer, &true, MAX_FRAME_BYTES)
            .unwrap_err()
            .kind(),
        io::ErrorKind::BrokenPipe
    );
    assert_eq!(writer.bytes.len(), 2);
    assert!(!writer.flushed);
}

struct FlushRefused(Vec<u8>);
impl Write for FlushRefused {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::other("fixed flush refusal"))
    }
}
#[test]
fn framing_storage_flush_failure_keeps_a_complete_encoded_frame() {
    let mut writer = FlushRefused(Vec::new());
    assert!(write_framed_with_limit(&mut writer, &true, MAX_FRAME_BYTES).is_err());
    assert_eq!(&writer.0[..4], &4u32.to_le_bytes());
    assert_eq!(&writer.0[4..], b"true");
}
