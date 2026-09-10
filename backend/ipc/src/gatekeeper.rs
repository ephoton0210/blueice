// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The internal wire protocol between `core` and `ai-gatekeeper`
//! (`phase-7-local-ai/PLAN.md`'s "Wiring design" section) -- distinct
//! from, and never spoken by, an external [`crate::ClientMessage`]/
//! [`crate::ServerMessage`] client. Lives inside `blueice-ipc` (rather
//! than a separate crate) purely to reuse this crate's private
//! length-prefixed-JSON framing primitives ([`crate::write_framed`]/
//! [`crate::read_frame_bytes`]) without changing their visibility --
//! `gatekeeper` is a descendant module of the crate root the same way
//! every other module here is, so those `fn`s (private to their
//! defining module and its descendants) are already reachable from
//! this file with no `pub(crate)` promotion needed anywhere.
//!
//! Deliberately **no handshake**, unlike the client-facing protocol's
//! `Hello`/`protocol_version` negotiation: a gatekeeper check is one
//! request, one reply, over one short-lived connection (connect ->
//! request -> reply -> disconnect -- see `blueice-engine`'s own
//! `gatekeeper_client::check_and_fetch`), built and versioned in
//! lockstep with `core` itself. There's no independent third-party
//! client of this protocol to stay forward-compatible with, so none of
//! [`crate::ClientMessage`]/[`crate::ServerMessage`]'s
//! `#[serde(other)]` fail-soft machinery is warranted here.

use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
use std::path::PathBuf;

/// One gatekeeper review request -- `core` sends exactly one of these
/// per short-lived connection to `ai-gatekeeper`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GatekeeperRequest {
    /// The URL stage, sent before any network fetch: catches known-bad
    /// domains cheaply, before spending a fetch on them.
    CheckUrl { url: String },
    /// The content stage, sent after fetch but before parse/cascade/
    /// layout -- the stage that actually addresses this phase's named
    /// primary threat (hidden/adversarial content aimed at an AI
    /// reader), which URL blocklisting alone can't catch.
    CheckContent { url: String, html: String },
}

/// `ai-gatekeeper`'s reply to one [`GatekeeperRequest`]. Either stage
/// returning `Rejected` ends the navigation; a connection/IO failure
/// talking to the gatekeeper is treated the same way by `core`'s own
/// client (fail-closed) but is *not* itself a value of this type -- see
/// `blueice-engine`'s `gatekeeper_client` module.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GatekeeperReply {
    Cleared,
    Rejected { reason: String, category: String },
}

pub fn write_gatekeeper_request<W: Write>(w: &mut W, msg: &GatekeeperRequest) -> io::Result<()> {
    crate::write_framed(w, msg)
}

pub fn read_gatekeeper_request<R: Read>(r: &mut R) -> io::Result<GatekeeperRequest> {
    let buf = crate::read_frame_bytes(r)?;
    serde_json::from_slice(&buf).map_err(io::Error::other)
}

pub fn write_gatekeeper_reply<W: Write>(w: &mut W, msg: &GatekeeperReply) -> io::Result<()> {
    crate::write_framed(w, msg)
}

pub fn read_gatekeeper_reply<R: Read>(r: &mut R) -> io::Result<GatekeeperReply> {
    let buf = crate::read_frame_bytes(r)?;
    serde_json::from_slice(&buf).map_err(io::Error::other)
}

/// Where `ai-gatekeeper` listens, and where `core`'s own gatekeeper-
/// check background threads connect by default in production.
/// Mirrors `blueice-launcher`'s `default_rendezvous_socket_path`
/// location/convention (a per-user temp dir, never a single
/// system-wide path, so two different users -- or two independent
/// BlueIce sessions -- never collide) but a distinct filename: this is
/// a different process with a different socket, not the same
/// rendezvous point `core` and its external clients share. Production
/// code is the only caller that uses this default directly -- tests
/// thread an explicit socket path through instead (see
/// `blueice_engine::session::run_session`'s own `gatekeeper_socket`
/// parameter), since many gatekeeper-behavior tests need their own
/// independent fake listener running concurrently in the same test
/// binary process.
pub fn default_gatekeeper_socket_path() -> PathBuf {
    let dir = match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) => PathBuf::from(dir).join("blueice"),
        None => std::env::temp_dir().join(format!("blueice-{}", unsafe { libc_getuid() })),
    };
    dir.join("ai-gatekeeper.sock")
}

// Duplicated from `blueice-launcher`'s identical helper rather than
// shared via a common dependency -- see that crate's own docs for why
// (~10 lines, and the two crates' socket-path helpers are otherwise
// independent enough that sharing them would be more indirection than
// the duplication costs).
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

    #[test]
    fn gatekeeper_request_round_trips_over_a_real_socket() {
        for req in [
            GatekeeperRequest::CheckUrl {
                url: "https://example.com".to_string(),
            },
            GatekeeperRequest::CheckContent {
                url: "https://example.com".to_string(),
                html: "<p>hi</p>".to_string(),
            },
        ] {
            let (mut a, mut b) = UnixStream::pair().unwrap();
            write_gatekeeper_request(&mut a, &req).unwrap();
            assert_eq!(read_gatekeeper_request(&mut b).unwrap(), req);
        }
    }

    #[test]
    fn gatekeeper_reply_round_trips_over_a_real_socket() {
        for reply in [
            GatekeeperReply::Cleared,
            GatekeeperReply::Rejected {
                reason: "phishing-shaped domain".to_string(),
                category: "known-bad-domain".to_string(),
            },
        ] {
            let (mut a, mut b) = UnixStream::pair().unwrap();
            write_gatekeeper_reply(&mut a, &reply).unwrap();
            assert_eq!(read_gatekeeper_reply(&mut b).unwrap(), reply);
        }
    }

    #[test]
    fn multiple_requests_can_be_written_and_read_in_sequence_on_one_stream() {
        let mut buf = Vec::new();
        write_gatekeeper_request(
            &mut buf,
            &GatekeeperRequest::CheckUrl {
                url: "https://a.example".to_string(),
            },
        )
        .unwrap();
        write_gatekeeper_request(
            &mut buf,
            &GatekeeperRequest::CheckUrl {
                url: "https://b.example".to_string(),
            },
        )
        .unwrap();
        let mut cursor = std::io::Cursor::new(buf);
        assert_eq!(
            read_gatekeeper_request(&mut cursor).unwrap(),
            GatekeeperRequest::CheckUrl {
                url: "https://a.example".to_string()
            }
        );
        assert_eq!(
            read_gatekeeper_request(&mut cursor).unwrap(),
            GatekeeperRequest::CheckUrl {
                url: "https://b.example".to_string()
            }
        );
    }

    #[test]
    fn default_gatekeeper_socket_path_is_distinct_from_the_rendezvous_socket_path() {
        // Same per-user-temp-dir convention `blueice-launcher` uses for
        // its own rendezvous socket, but a different filename -- must
        // never resolve to the same path `core`'s external clients
        // connect to.
        let path = default_gatekeeper_socket_path();
        assert_eq!(path.file_name().unwrap(), "ai-gatekeeper.sock");
        assert_ne!(path.file_name().unwrap(), "core.sock");
    }

    #[test]
    fn reading_malformed_json_is_an_error_not_a_panic() {
        let mut buf = Vec::new();
        let bad_payload = b"not json";
        buf.extend_from_slice(&(bad_payload.len() as u32).to_le_bytes());
        buf.extend_from_slice(bad_payload);
        let mut cursor = std::io::Cursor::new(buf);
        assert!(read_gatekeeper_request(&mut cursor).is_err());
    }
}
