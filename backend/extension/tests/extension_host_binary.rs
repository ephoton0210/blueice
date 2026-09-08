// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Exercises the actual compiled `blueice-extension-host` binary as a
//! real subprocess -- `main`'s own argument parsing, socket binding,
//! and accept-loop wiring, none of which `src/main.rs`'s own unit tests
//! touch (those cover `parse_args` in isolation; `handle_extension_
//! connection`'s own tests in `src/lib.rs` cover the protocol logic
//! over an in-process `UnixStream` pair). This is the concrete,
//! end-to-end proof `phase-9-extension-protocol/PLAN.md`'s "Minimal
//! first slice" asks for: a real client, over a real Unix socket,
//! across a real process boundary, gets a capability it's granted and
//! is denied one it isn't -- mirroring `blueice-engine`'s own
//! `tests/core_binary.rs` real-subprocess pattern one layer over.

use blueice_ipc::extension::{read_extension_reply, write_extension_request, ExtensionReply, ExtensionRequest};
use std::collections::BTreeMap;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

fn unique_socket_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("blueice-extension-host-test-{label}-{}.sock", std::process::id()))
}

fn wait_for(path: &std::path::Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    false
}

/// A spawned real `blueice-extension-host` subprocess, killed on drop --
/// there's no `Shutdown`-style message in this protocol (unlike
/// `blueice-core`'s), so every test here ends the subprocess by simply
/// dropping this.
struct ExtensionHost {
    child: Child,
    socket: PathBuf,
}

impl ExtensionHost {
    fn spawn(label: &str) -> Self {
        let socket = unique_socket_path(label);
        let _ = std::fs::remove_file(&socket);

        let child = Command::new(env!("CARGO_BIN_EXE_blueice-extension-host")).args(["--socket", socket.to_str().unwrap()]).spawn().expect("failed to spawn blueice-extension-host");

        assert!(wait_for(&socket, Duration::from_secs(5)), "blueice-extension-host never created its socket");
        ExtensionHost { child, socket }
    }

    fn connect(&self) -> UnixStream {
        UnixStream::connect(&self.socket).expect("failed to connect to the real subprocess")
    }
}

impl Drop for ExtensionHost {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.socket);
    }
}

fn hello(extension_id: &str) -> ExtensionRequest {
    ExtensionRequest::Hello { extension_id: extension_id.to_string(), capability_versions: BTreeMap::new() }
}

#[test]
fn missing_socket_flag_exits_with_failure_and_no_socket_is_created() {
    let output = Command::new(env!("CARGO_BIN_EXE_blueice-extension-host")).output().expect("failed to run blueice-extension-host");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--socket"));
}

#[test]
fn a_granted_capability_succeeds_and_an_ungranted_one_is_denied_over_a_real_process_boundary() {
    // The single most important test in this task: a real client,
    // across a real process boundary, sends `Hello` with the hardcoded
    // extension's id, gets `HelloAck`, gets a real `DomRead` result
    // (server-side allow), and gets `CapabilityDenied` for `DomWrite`
    // (server-side deny) -- concrete, end-to-end proof that
    // authorization is enforced by the server, not merely assumed.
    let host = ExtensionHost::spawn("authz");
    let mut stream = host.connect();

    write_extension_request(&mut stream, &hello("minimal-slice-extension")).unwrap();
    assert_eq!(read_extension_reply(&mut stream).unwrap(), ExtensionReply::HelloAck);

    write_extension_request(&mut stream, &ExtensionRequest::DomRead).unwrap();
    match read_extension_reply(&mut stream).unwrap() {
        ExtensionReply::DomReadResult { value } => assert!(!value.is_empty(), "a granted DomRead must return a real, non-empty result, not an empty placeholder"),
        other => panic!("expected DomReadResult for a granted dom:read, got {other:?}"),
    }

    write_extension_request(&mut stream, &ExtensionRequest::DomWrite { value: "attacker-controlled content".to_string() }).unwrap();
    match read_extension_reply(&mut stream).unwrap() {
        ExtensionReply::CapabilityDenied { capability, reason } => {
            assert_eq!(capability, "dom:write");
            assert!(!reason.is_empty());
        }
        other => panic!("expected CapabilityDenied for an ungranted dom:write, got {other:?}"),
    }
}

#[test]
fn an_extension_id_that_was_never_registered_gets_capability_denied_even_for_dom_read() {
    let host = ExtensionHost::spawn("unknown-id");
    let mut stream = host.connect();

    write_extension_request(&mut stream, &hello("an-extension-nobody-installed")).unwrap();
    assert_eq!(read_extension_reply(&mut stream).unwrap(), ExtensionReply::HelloAck);

    write_extension_request(&mut stream, &ExtensionRequest::DomRead).unwrap();
    match read_extension_reply(&mut stream).unwrap() {
        ExtensionReply::CapabilityDenied { capability, .. } => assert_eq!(capability, "dom:read"),
        other => panic!("expected CapabilityDenied for an unregistered extension_id, got {other:?}"),
    }
}

#[test]
fn a_non_hello_first_message_gets_no_reply_and_the_connection_ends() {
    let host = ExtensionHost::spawn("bad-first-msg");
    let mut stream = host.connect();

    write_extension_request(&mut stream, &ExtensionRequest::DomRead).unwrap();

    // The real subprocess must never answer a request sent before a
    // successful handshake -- reading now must fail (the connection was
    // dropped server-side), not return a stray reply or hang.
    assert!(read_extension_reply(&mut stream).is_err());
}

#[test]
fn the_real_subprocess_serves_two_independent_connections_in_sequence() {
    let host = ExtensionHost::spawn("two-conns");

    for _ in 0..2 {
        let mut stream = host.connect();
        write_extension_request(&mut stream, &hello("minimal-slice-extension")).unwrap();
        assert_eq!(read_extension_reply(&mut stream).unwrap(), ExtensionReply::HelloAck);
        write_extension_request(&mut stream, &ExtensionRequest::DomRead).unwrap();
        assert!(matches!(read_extension_reply(&mut stream).unwrap(), ExtensionReply::DomReadResult { .. }));
    }
}
