// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `blueice-extension-host`: the minimal-slice implementation of
//! `phase-9-extension-protocol/PLAN.md`'s "Minimal first slice" -- a
//! hardcoded single extension (no WASM runtime, no manifest file
//! parser yet), proving server-side capability enforcement end to end
//! over a real process boundary before any of that fuller machinery
//! exists.
//!
//! **Where this logic lives, and why.** The plan doc's "Wiring design"
//! frames the enforcing side as living inside `core` itself (an
//! `ExtensionRegistry` "a peer to `TabManager`, not a member of it").
//! This minimal slice deliberately does *not* wire that into the real
//! `blueice-core` binary/`blueice_engine::session::run_session` loop --
//! see that fn's own docs, untouched by this crate -- because extension
//! messages are a wholly separate wire protocol on a separate
//! connection, and multiplexing a second listener into `core`'s
//! existing single-client session loop is real future work, not this
//! slice's. Instead, `blueice-extension-host` -- a crate literally
//! named for the "host" role a WASM extension runtime eventually plays
//! -- serves that protocol standalone: it owns [`ExtensionRegistry`]
//! and [`handle_extension_connection`] itself, rather than depending on
//! `blueice-engine` (which is `core`-specific `Page`/`TabManager`
//! dispatch logic this crate has no business reaching into for a
//! placeholder DOM value). When capability enforcement is eventually
//! wired into the real `core` process, this module is what moves (or
//! gets called from) there -- the mechanism doesn't change, only which
//! process runs it.
//!
//! **What's real, what's a placeholder** (mirrors `blueice-ai-
//! gatekeeper`'s own "mechanism real, content stub" scoping): the
//! handshake, the registry lookup, and the allow/deny decision are all
//! real and tested. [`ExtensionReply::DomReadResult`]'s value is a
//! fixed placeholder string, not real `Page` state -- wiring a real DOM
//! snapshot through is explicitly out of scope for this slice (it would
//! require this crate to depend on `blueice-engine` and share a `Page`
//! across a process boundary that doesn't exist yet); the point of this
//! slice is proving the *authorization* mechanism, not the DOM-read
//! capability's real payload.
//!
//! **Non-spoofable identity is still open work.** A real `extension_id`
//! is meant to be derived from a hash of the extension's manifest +
//! WASM module (per the plan doc), so it can't be spoofed at connect
//! time. This slice's [`blueice_ipc::extension::ExtensionRequest::Hello`]
//! still carries a plain author-chosen `extension_id` string -- the
//! registry enforces *whatever* identity is declared, which is the
//! actual mechanism under test here, but nothing yet stops a connecting
//! process from simply declaring a different extension's id. Tracked as
//! still-open in `phase-9-extension-protocol/PLAN.md`, not solved here.

use blueice_ipc::extension::{
    read_extension_request, write_extension_reply, ExtensionReply, ExtensionRequest,
};
use std::collections::{HashMap, HashSet};
use std::io::{self, Read, Write};

/// The one hardcoded extension identity this minimal slice recognizes.
/// A real, non-spoofable, manifest-derived identity scheme is still-open
/// future work -- see this crate's own module docs.
pub const MINIMAL_SLICE_EXTENSION_ID: &str = "minimal-slice-extension";

/// The capability this slice's one hardcoded extension is granted.
pub const CAPABILITY_DOM_READ: &str = "dom:read";

/// The capability this slice's one hardcoded extension is deliberately
/// *not* granted -- the request that proves server-side denial.
pub const CAPABILITY_DOM_WRITE: &str = "dom:write";

/// [`ExtensionReply::DomReadResult`]'s value for a granted `DomRead` in
/// this minimal slice -- a fixed placeholder, not real `Page` state; see
/// this crate's own module docs for why.
const PLACEHOLDER_DOM_READ_VALUE: &str =
    "<blueice-extension-host: no real Page is wired into this minimal slice>";

/// Which capabilities each connected extension has been granted --
/// `phase-9-extension-protocol/PLAN.md`'s "Wiring design" describes this
/// as an in-memory `ExtensionRegistry` keyed by a *derived*
/// (non-spoofable) `extension_id`; for this minimal slice it's a plain
/// `extension_id -> granted capability names` map, seeded with one
/// hardcoded entry ([`ExtensionRegistry::minimal_slice`]) rather than
/// built from install-time manifest persistence -- both explicitly
/// still-open future work per the plan doc, not solved here.
pub struct ExtensionRegistry {
    grants: HashMap<String, HashSet<String>>,
}

impl Default for ExtensionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ExtensionRegistry {
    /// An empty registry: no extension_id is granted anything.
    pub fn new() -> Self {
        Self {
            grants: HashMap::new(),
        }
    }

    /// Grants `capability` to `extension_id`, in addition to whatever
    /// it already holds.
    pub fn grant(&mut self, extension_id: impl Into<String>, capability: impl Into<String>) {
        self.grants
            .entry(extension_id.into())
            .or_default()
            .insert(capability.into());
    }

    /// The actual enforcement point: does `extension_id` currently hold
    /// `capability`? An unrecognized `extension_id` (never granted
    /// anything) simply has no capabilities -- see this crate's module
    /// docs for why an unknown identity isn't rejected outright at
    /// handshake time.
    pub fn has_capability(&self, extension_id: &str, capability: &str) -> bool {
        self.grants
            .get(extension_id)
            .is_some_and(|caps| caps.contains(capability))
    }

    /// Seeds the hardcoded single-extension grant this minimal slice
    /// ships: [`MINIMAL_SLICE_EXTENSION_ID`] gets [`CAPABILITY_DOM_READ`]
    /// only -- deliberately *not* [`CAPABILITY_DOM_WRITE`], so a
    /// `DomWrite` attempt is the concrete proof of server-side denial.
    pub fn minimal_slice() -> Self {
        let mut registry = Self::new();
        registry.grant(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ);
        registry
    }
}

/// Serves one extension connection until it disconnects (or sends
/// something this minimal slice can't make sense of -- see below):
/// requires [`ExtensionRequest::Hello`] as the very first message
/// (rejecting/ending the connection otherwise, mirroring how
/// `blueice_engine::session`'s `perform_handshake` rejects a non-`Hello`
/// first message on the external client protocol), replies
/// [`ExtensionReply::HelloAck`], then loops handling `DomRead`/
/// `DomWrite` requests -- checking `registry` before executing each,
/// replying [`ExtensionReply::CapabilityDenied`] for an unauthorized
/// request rather than a silent no-op or a bare/generic error.
///
/// **A later `Hello`** (once past the initial handshake) is accepted
/// and just answered with another `HelloAck`, updating which
/// `extension_id` subsequent requests on this connection are checked
/// against -- the same "answered again, not re-gating the whole
/// connection" discipline `run_session`'s own docs describe for a
/// repeat `Hello` on the external client protocol, applied here to a
/// long-lived extension connection.
///
/// **Any read failure** (a clean disconnect, or bytes that don't parse
/// as a well-formed [`ExtensionRequest`]) ends the connection by
/// returning `Ok(())`, never propagating a "malformed input" case as a
/// distinguishable error and never panicking -- mirrors `run_session`'s
/// own "any non-timeout read error means disconnect" handling of the
/// external client protocol's main loop. The wire-level parsing
/// functions this calls (`blueice_ipc::extension::read_extension_
/// request`) do still surface a malformed frame as a real `io::Error`
/// to *their own* callers/tests -- this fn just chooses, deliberately,
/// to treat that the same as an ordinary disconnect rather than
/// escalate it, the same choice `run_session` already made for the
/// analogous case on the external protocol.
pub fn handle_extension_connection<S: Read + Write>(
    registry: &ExtensionRegistry,
    stream: &mut S,
) -> io::Result<()> {
    let mut extension_id = match read_extension_request(stream) {
        Ok(ExtensionRequest::Hello { extension_id, .. }) => {
            write_extension_reply(stream, &ExtensionReply::HelloAck)?;
            extension_id
        }
        Ok(_) => return Ok(()), // first message wasn't Hello: reject by ending the connection
        Err(_) => return Ok(()), // disconnected, or sent something unparseable, before ever completing the handshake
    };

    loop {
        let request = match read_extension_request(stream) {
            Ok(request) => request,
            Err(_) => return Ok(()),
        };
        match request {
            ExtensionRequest::Hello {
                extension_id: new_id,
                ..
            } => {
                extension_id = new_id;
                write_extension_reply(stream, &ExtensionReply::HelloAck)?;
            }
            ExtensionRequest::DomRead => {
                if registry.has_capability(&extension_id, CAPABILITY_DOM_READ) {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::DomReadResult {
                            value: PLACEHOLDER_DOM_READ_VALUE.to_string(),
                        },
                    )?;
                } else {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_DOM_READ.to_string(),
                            reason: format!("{extension_id} is not granted {CAPABILITY_DOM_READ}"),
                        },
                    )?;
                }
            }
            ExtensionRequest::DomWrite { value: _ } => {
                if registry.has_capability(&extension_id, CAPABILITY_DOM_WRITE) {
                    write_extension_reply(stream, &ExtensionReply::DomWriteAck)?;
                } else {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_DOM_WRITE.to_string(),
                            reason: format!("{extension_id} is not granted {CAPABILITY_DOM_WRITE}"),
                        },
                    )?;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_ipc::extension::{read_extension_reply, write_extension_request};
    use std::collections::BTreeMap;
    use std::os::unix::net::UnixStream;
    use std::thread;

    fn hello(extension_id: &str) -> ExtensionRequest {
        ExtensionRequest::Hello {
            extension_id: extension_id.to_string(),
            capability_versions: BTreeMap::new(),
        }
    }

    #[test]
    fn extension_registry_minimal_slice_grants_dom_read_but_not_dom_write() {
        let registry = ExtensionRegistry::minimal_slice();
        assert!(registry.has_capability(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ));
        assert!(!registry.has_capability(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_WRITE));
    }

    #[test]
    fn extension_registry_reports_no_capabilities_for_an_unknown_extension_id() {
        let registry = ExtensionRegistry::minimal_slice();
        assert!(!registry.has_capability("some-other-extension", CAPABILITY_DOM_READ));
        assert!(!registry.has_capability("some-other-extension", CAPABILITY_DOM_WRITE));
    }

    #[test]
    fn extension_registry_new_grants_nothing_until_granted() {
        let mut registry = ExtensionRegistry::new();
        assert!(!registry.has_capability(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ));
        registry.grant(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ);
        assert!(registry.has_capability(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ));
    }

    #[test]
    fn a_granted_dom_read_succeeds_after_handshake() {
        let registry = ExtensionRegistry::minimal_slice();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || handle_extension_connection(&registry, &mut server));

        write_extension_request(&mut client, &hello(MINIMAL_SLICE_EXTENSION_ID)).unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::HelloAck
        );

        write_extension_request(&mut client, &ExtensionRequest::DomRead).unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::DomReadResult {
                value: PLACEHOLDER_DOM_READ_VALUE.to_string()
            }
        );

        drop(client);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn an_ungranted_dom_write_is_denied_with_the_capability_and_a_reason() {
        let registry = ExtensionRegistry::minimal_slice();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || handle_extension_connection(&registry, &mut server));

        write_extension_request(&mut client, &hello(MINIMAL_SLICE_EXTENSION_ID)).unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::HelloAck
        );

        write_extension_request(
            &mut client,
            &ExtensionRequest::DomWrite {
                value: "hijacked".to_string(),
            },
        )
        .unwrap();
        match read_extension_reply(&mut client).unwrap() {
            ExtensionReply::CapabilityDenied { capability, reason } => {
                assert_eq!(capability, CAPABILITY_DOM_WRITE);
                assert!(!reason.is_empty());
            }
            other => panic!("expected CapabilityDenied, got {other:?}"),
        }

        drop(client);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn a_denied_request_does_not_end_the_connection_a_later_granted_request_still_works() {
        // Proves denial is a per-request check, not a connection-ending
        // fault -- and, since this minimal slice has no real `Page`
        // state for `DomWrite` to touch, this is the closest available
        // proof that a rejected attempt has no side effect on later
        // requests either.
        let registry = ExtensionRegistry::minimal_slice();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || handle_extension_connection(&registry, &mut server));

        write_extension_request(&mut client, &hello(MINIMAL_SLICE_EXTENSION_ID)).unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::HelloAck
        );

        write_extension_request(
            &mut client,
            &ExtensionRequest::DomWrite {
                value: "x".to_string(),
            },
        )
        .unwrap();
        assert!(matches!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::CapabilityDenied { .. }
        ));

        write_extension_request(&mut client, &ExtensionRequest::DomRead).unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::DomReadResult {
                value: PLACEHOLDER_DOM_READ_VALUE.to_string()
            }
        );

        drop(client);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn an_unrecognized_extension_id_gets_capability_denied_not_a_crash_or_a_free_pass() {
        let registry = ExtensionRegistry::minimal_slice();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || handle_extension_connection(&registry, &mut server));

        write_extension_request(&mut client, &hello("never-registered-extension")).unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::HelloAck
        );

        write_extension_request(&mut client, &ExtensionRequest::DomRead).unwrap();
        match read_extension_reply(&mut client).unwrap() {
            ExtensionReply::CapabilityDenied { capability, .. } => {
                assert_eq!(capability, CAPABILITY_DOM_READ)
            }
            other => panic!("expected CapabilityDenied, got {other:?}"),
        }

        drop(client);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn a_non_hello_first_message_ends_the_connection_without_a_reply() {
        let registry = ExtensionRegistry::minimal_slice();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || handle_extension_connection(&registry, &mut server));

        write_extension_request(&mut client, &ExtensionRequest::DomRead).unwrap();

        handle.join().unwrap().unwrap();
        // No reply was ever written -- reading now must fail (EOF, since
        // the connection-handling thread already exited and dropped its
        // end of the socket), not hang or return a stray `HelloAck`.
        assert!(read_extension_reply(&mut client).is_err());
    }

    #[test]
    fn disconnecting_before_sending_anything_ends_the_connection_cleanly() {
        let registry = ExtensionRegistry::minimal_slice();
        let (client, mut server) = UnixStream::pair().unwrap();
        drop(client);
        assert!(handle_extension_connection(&registry, &mut server).is_ok());
    }

    #[test]
    fn a_second_hello_is_answered_again_and_updates_which_identity_is_checked() {
        // Mirrors `run_session`'s own "a repeat Hello is just answered
        // again, not re-gated" handling on the external client protocol
        // -- applied here to a long-lived extension connection that
        // re-identifies mid-connection.
        let registry = ExtensionRegistry::minimal_slice();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || handle_extension_connection(&registry, &mut server));

        write_extension_request(&mut client, &hello("some-other-extension")).unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::HelloAck
        );

        // Not yet granted anything under this first identity.
        write_extension_request(&mut client, &ExtensionRequest::DomRead).unwrap();
        assert!(matches!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::CapabilityDenied { .. }
        ));

        // Re-identify as the granted extension on the same connection.
        write_extension_request(&mut client, &hello(MINIMAL_SLICE_EXTENSION_ID)).unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::HelloAck
        );

        write_extension_request(&mut client, &ExtensionRequest::DomRead).unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::DomReadResult {
                value: PLACEHOLDER_DOM_READ_VALUE.to_string()
            }
        );

        drop(client);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn a_malformed_frame_after_handshake_ends_the_connection_rather_than_panicking() {
        let registry = ExtensionRegistry::minimal_slice();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || handle_extension_connection(&registry, &mut server));

        write_extension_request(&mut client, &hello(MINIMAL_SLICE_EXTENSION_ID)).unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::HelloAck
        );

        // A truncated frame: a length prefix promising more bytes than
        // are ever sent.
        client.write_all(&100u32.to_le_bytes()).unwrap();
        client.write_all(b"short").unwrap();
        drop(client);

        // Must return cleanly (not panic, not hang) -- see this fn's own
        // docs for why a malformed frame is treated the same as a plain
        // disconnect rather than propagated as a distinguishable error.
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn handles_two_connections_in_sequence() {
        // Proves the accept-loop-friendly shape (one call handles
        // exactly one connection, start to end, and returns) that
        // `src/main.rs`'s sequential accept loop depends on.
        for _ in 0..2 {
            let registry = ExtensionRegistry::minimal_slice();
            let (mut client, mut server) = UnixStream::pair().unwrap();
            let handle = thread::spawn(move || handle_extension_connection(&registry, &mut server));
            write_extension_request(&mut client, &hello(MINIMAL_SLICE_EXTENSION_ID)).unwrap();
            assert_eq!(
                read_extension_reply(&mut client).unwrap(),
                ExtensionReply::HelloAck
            );
            drop(client);
            handle.join().unwrap().unwrap();
        }
    }
}
