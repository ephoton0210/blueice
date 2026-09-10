// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `blueice-ai-gatekeeper`: the minimal-slice stub for
//! `phase-7-local-ai/PLAN.md`'s safety-gatekeeper process. Per that
//! plan's "Minimal first slice" (in the "Wiring design (resolved
//! 2026-09-08)" section): this process is a trivial stub that always
//! clears both review stages -- no model, no rule-base yet. What's real
//! and tested in this slice is the *mechanism* around it: the process
//! itself, the `blueice_ipc::gatekeeper` wire protocol, `core`'s
//! non-blocking two-phase dispatch, and fail-closed behavior when this
//! process is unreachable (all owned by `blueice-engine`'s own
//! `gatekeeper_client`/`session` modules, not by this crate).
//!
//! `core` opens a short-lived, per-check connection (connect -> request
//! -> reply -> disconnect) rather than multiplexing many checks over
//! one shared connection, so [`handle_one_check`] handling connections
//! sequentially (see `src/bin/blueice-ai-gatekeeper.rs`) is deliberate,
//! not a scalability shortcut: concurrent checks from different tabs
//! are already independent OS-level connections/threads on `core`'s
//! side, each served by its own accepted connection here.

use blueice_ipc::gatekeeper::{read_gatekeeper_request, write_gatekeeper_reply, GatekeeperReply};
use std::io::{self, Read, Write};

/// Reads one [`blueice_ipc::gatekeeper::GatekeeperRequest`] from
/// `stream` and always replies [`GatekeeperReply::Cleared`] -- the
/// entire "review" this minimal slice performs. A real rule-base/AI
/// review is explicit future work per the plan doc, deliberately not
/// built here.
pub fn handle_one_check<S: Read + Write>(stream: &mut S) -> io::Result<()> {
    let _request = read_gatekeeper_request(stream)?;
    write_gatekeeper_reply(stream, &GatekeeperReply::Cleared)
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_ipc::gatekeeper::{
        read_gatekeeper_reply, write_gatekeeper_request, GatekeeperRequest,
    };
    use std::os::unix::net::UnixStream;
    use std::thread;

    #[test]
    fn always_clears_a_check_url_request() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || handle_one_check(&mut server));

        write_gatekeeper_request(
            &mut client,
            &GatekeeperRequest::CheckUrl {
                url: "https://example.com".to_string(),
            },
        )
        .unwrap();
        assert_eq!(
            read_gatekeeper_reply(&mut client).unwrap(),
            GatekeeperReply::Cleared
        );

        handle.join().unwrap().unwrap();
    }

    #[test]
    fn always_clears_a_check_content_request() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || handle_one_check(&mut server));

        write_gatekeeper_request(
            &mut client,
            &GatekeeperRequest::CheckContent {
                url: "https://example.com".to_string(),
                html: "<p>hi</p>".to_string(),
            },
        )
        .unwrap();
        assert_eq!(
            read_gatekeeper_reply(&mut client).unwrap(),
            GatekeeperReply::Cleared
        );

        handle.join().unwrap().unwrap();
    }

    #[test]
    fn a_malformed_request_is_an_io_error_not_a_panic() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || handle_one_check(&mut server));

        // A truncated frame: a length prefix promising more bytes than
        // are ever sent.
        client.write_all(&100u32.to_le_bytes()).unwrap();
        client.write_all(b"short").unwrap();
        drop(client);

        assert!(handle.join().unwrap().is_err());
    }

    #[test]
    fn handles_two_connections_in_sequence() {
        // Proves the accept-loop-friendly shape (`handle_one_check`
        // handles exactly one connection's one request/reply and
        // returns, never blocking on a second) that `src/bin/blueice-
        // ai-gatekeeper.rs`'s sequential accept loop depends on.
        for _ in 0..2 {
            let (mut client, mut server) = UnixStream::pair().unwrap();
            let handle = thread::spawn(move || handle_one_check(&mut server));
            write_gatekeeper_request(
                &mut client,
                &GatekeeperRequest::CheckUrl {
                    url: "https://example.com".to_string(),
                },
            )
            .unwrap();
            assert_eq!(
                read_gatekeeper_reply(&mut client).unwrap(),
                GatekeeperReply::Cleared
            );
            handle.join().unwrap().unwrap();
        }
    }
}
