// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The launcher-internal control protocol -- distinct from the external
//! rendezvous socket [`blueice_ipc::ClientMessage`]/
//! [`blueice_ipc::ServerMessage`] speak, and deliberately not folded
//! into `blueice-ipc` itself: an operator/test tool talking to an
//! already-running launcher is never something `core` itself needs to
//! understand, so this stays entirely local to `blueice-launcher`, per
//! `phase-8-live-core-hotswap/PLAN.md`'s "Trigger mechanism for this
//! minimal slice."
//!
//! Wire format mirrors `blueice-ipc`'s own choice (length-prefixed
//! JSON) for the same "fastest to validate the boundary" reason, kept
//! as its own small implementation here rather than depending on
//! `blueice-ipc` for it -- this protocol carries exactly one
//! request/reply pair today, not worth sharing infrastructure for.

use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
use std::path::PathBuf;

/// Sent by an operator/test tool to an already-running `blueice-
/// launcher`, over [`default_control_socket_path`] (or an override the
/// launcher was started with).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ControlRequest {
    /// Spawn a fresh `core` (v2), replay v1's currently-open tabs into
    /// it, health-check it, and -- only if that all succeeds -- cut
    /// every already-connected external client's traffic over to it,
    /// then tear v1 down. No fields for this minimal slice: v2 is
    /// spawned with the same width/height the launcher itself was
    /// started with, and a fresh frame directory derived from v1's own.
    Cutover,
}

/// The launcher's reply to a [`ControlRequest`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ControlReply {
    /// The cutover completed: every already-connected external client
    /// now has its traffic flowing to v2, and v1 has been torn down.
    /// `tabs_migrated` is how many of v1's tabs were captured and
    /// replayed.
    CutoverDone { tabs_migrated: usize },
    /// The cutover was abandoned before touching v1's serving state --
    /// v1 keeps serving every already-connected client exactly as
    /// before. `reason` is a human-readable diagnostic, not meant to be
    /// pattern-matched on.
    CutoverFailed { reason: String },
}

/// The control socket path, mirroring
/// [`crate::default_rendezvous_socket_path`]'s exact per-user
/// directory convention -- a distinct filename in the same directory,
/// so the two sockets never collide.
pub fn default_control_socket_path() -> PathBuf {
    crate::rendezvous_socket_dir().join("control.sock")
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

pub fn write_control_request<W: Write>(w: &mut W, req: &ControlRequest) -> io::Result<()> {
    write_framed(w, req)
}

pub fn read_control_request<R: Read>(r: &mut R) -> io::Result<ControlRequest> {
    read_framed(r)
}

pub fn write_control_reply<W: Write>(w: &mut W, reply: &ControlReply) -> io::Result<()> {
    write_framed(w, reply)
}

pub fn read_control_reply<R: Read>(r: &mut R) -> io::Result<ControlReply> {
    read_framed(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::os::unix::net::UnixStream;

    #[test]
    fn control_request_round_trips() {
        let mut buf = Vec::new();
        write_control_request(&mut buf, &ControlRequest::Cutover).unwrap();
        let mut cursor = Cursor::new(buf);
        assert_eq!(
            read_control_request(&mut cursor).unwrap(),
            ControlRequest::Cutover
        );
    }

    #[test]
    fn control_reply_round_trips_both_variants() {
        for reply in [
            ControlReply::CutoverDone { tabs_migrated: 3 },
            ControlReply::CutoverFailed {
                reason: "boom".to_string(),
            },
        ] {
            let mut buf = Vec::new();
            write_control_reply(&mut buf, &reply).unwrap();
            let mut cursor = Cursor::new(buf);
            assert_eq!(read_control_reply(&mut cursor).unwrap(), reply);
        }
    }

    #[test]
    fn default_control_socket_path_differs_from_the_rendezvous_socket_path() {
        assert_ne!(
            default_control_socket_path(),
            crate::default_rendezvous_socket_path()
        );
    }

    #[test]
    fn default_control_socket_path_ends_in_control_sock() {
        assert_eq!(
            default_control_socket_path().file_name().unwrap(),
            "control.sock"
        );
    }

    #[test]
    fn control_protocol_round_trips_over_a_real_unix_socket() {
        let (mut a, mut b) = UnixStream::pair().unwrap();
        write_control_request(&mut a, &ControlRequest::Cutover).unwrap();
        assert_eq!(
            read_control_request(&mut b).unwrap(),
            ControlRequest::Cutover
        );
        write_control_reply(&mut b, &ControlReply::CutoverDone { tabs_migrated: 1 }).unwrap();
        assert_eq!(
            read_control_reply(&mut a).unwrap(),
            ControlReply::CutoverDone { tabs_migrated: 1 }
        );
    }

    #[test]
    fn reading_a_truncated_control_request_is_an_error_not_a_panic() {
        let mut buf = Vec::new();
        write_control_request(&mut buf, &ControlRequest::Cutover).unwrap();
        buf.truncate(buf.len() - 1);
        let mut cursor = Cursor::new(buf);
        assert!(read_control_request(&mut cursor).is_err());
    }
}
