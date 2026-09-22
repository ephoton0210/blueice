// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `blueice-ai-gatekeeper`: the deterministic rule-base component of
//! `phase-7-local-ai/PLAN.md`'s safety-gatekeeper process. It remains
//! deliberately independent of any future AI model: `rules` has no prompt or
//! model input and produces a stable decision from one request alone. The
//! process/IPC/concurrency/fail-closed mechanism remains owned by
//! `blueice-engine`'s `gatekeeper_client`/`session` modules.
//!
//! `core` opens a short-lived, per-check connection (connect -> request
//! -> reply -> disconnect) rather than multiplexing many checks over
//! one shared connection. The binary serves each accepted connection on a
//! separate bounded-time worker, so concurrent checks from different tabs
//! remain independent all the way through the gatekeeper.

use blueice_ipc::gatekeeper::{read_gatekeeper_request, write_gatekeeper_reply};
use std::io::{self, Read, Write};

mod rules;

pub use rules::{review, RULESET_VERSION};

/// Reads one [`blueice_ipc::gatekeeper::GatekeeperRequest`] from
/// `stream` and replies with the independent deterministic rule-base
/// decision. A future model review is a separate second layer; it must not
/// replace or be able to modify this one.
pub fn handle_one_check<S: Read + Write>(stream: &mut S) -> io::Result<()> {
    let request = read_gatekeeper_request(stream)?;
    write_gatekeeper_reply(stream, &review(&request))
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_ipc::gatekeeper::{
        read_gatekeeper_reply, write_gatekeeper_request, GatekeeperReply, GatekeeperRequest,
    };
    use std::os::unix::net::UnixStream;
    use std::thread;

    #[test]
    fn clears_a_safe_check_url_request() {
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
    fn clears_a_safe_check_content_request() {
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
    fn rejects_an_executable_check_download_request() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || handle_one_check(&mut server));

        write_gatekeeper_request(
            &mut client,
            &GatekeeperRequest::CheckDownload {
                url: "https://example.com/setup.exe".to_string(),
                file_name: "setup.exe".to_string(),
                content_type: Some("application/x-msdownload".to_string()),
                total_bytes: Some(4096),
            },
        )
        .unwrap();
        assert!(matches!(
            read_gatekeeper_reply(&mut client).unwrap(),
            GatekeeperReply::Rejected { category, .. } if category == "dangerous-file-type"
        ));

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
