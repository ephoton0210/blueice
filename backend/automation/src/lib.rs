// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `blueice-automation`: the local, token-authenticated adapter process
//! `phase-17-automation-devtools-and-ajax/PLAN.md`'s Slice 1 item 3
//! asks for, alongside `core`'s own internal automation service
//! ([`blueice_engine::automation_service`], not a dependency of this
//! crate -- the two only ever talk over the real
//! [`blueice_ipc::automation`] socket, never a Rust API boundary). A
//! `blueice_ipc::automation` client (the eventual
//! `@blueice/automation` TypeScript library, a DevTools panel, or any
//! other automation tool) doesn't connect to `core`'s automation
//! socket directly -- it connects here, presents a locally-issued
//! token as its very first message, and only once that token matches
//! does this process transparently proxy the rest of the connection's
//! bytes to `core`'s real automation socket, unmodified in both
//! directions.
//!
//! This is a minimal first slice in the same deliberate sense Phases
//! 7/8/9 used it: what's real is the token issuance/verification and
//! the bidirectional byte-for-byte proxy; not yet done is anything
//! resembling a real multi-user credential store, token rotation/
//! expiry, or per-capability scoping at this layer (`core`'s own
//! automation service already enforces per-connection capability
//! grants -- see `blueice_engine::automation_service` -- this process
//! adds *authentication*, a separate concern, layered on top of that).
//!
//! The token itself is a fresh 32-byte value read from `/dev/urandom`
//! each time this process starts (never reused across runs) and
//! written to a per-user file at [`default_token_path`] with
//! owner-only permissions -- a legitimate local client reads the
//! token from that file itself, the same "prove you can read a file
//! only this user's own processes can read" trust model
//! `blueice_ipc`'s other per-user socket paths already rely on for
//! *connecting* at all.

use std::io::{self, Read};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::thread;

/// A fresh, random, 64-hex-character token. Generated with real
/// entropy (`/dev/urandom`, not `std::time`/pid-derived pseudo-
/// randomness -- this value is a bearer credential, not just an ID)
/// read directly, since the standard library has no built-in CSPRNG
/// and this crate deliberately doesn't add a new dependency just for
/// this one call -- the same "a direct POSIX call beats a crate for
/// one small need" reasoning `blueice_ipc::automation` and three
/// other modules already apply to `extern "C" fn getuid()`.
pub fn generate_token() -> io::Result<String> {
    let mut buf = [0u8; 32];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut buf)?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

// `extern "C"` declaration of the real POSIX `getuid()` -- see
// `blueice_ipc::automation`'s own copy of this exact pattern for why
// this is preferred over the Linux-only `/proc/self/status` parse
// `blueice_ipc::{gatekeeper, extension, script}` each had before
// `feature/css-wpt-conformance`'s fix.
extern "C" {
    fn getuid() -> u32;
}

unsafe fn libc_getuid() -> u32 {
    getuid()
}

/// Where this process writes its current token, per user (the same
/// `extern "C" fn getuid()`-derived-path pattern every other
/// `blueice_ipc` per-user socket path already uses, so two different
/// local users' adapters/tokens can never collide or read each
/// other's).
pub fn default_token_path() -> PathBuf {
    std::env::temp_dir().join(format!("blueice-automation-token-{}", unsafe {
        libc_getuid()
    }))
}

/// Writes `token` to `path` with owner-only (`0600`) permissions --
/// this file is a bearer credential, so it must never be
/// group/world-readable, unlike an ordinary socket path.
pub fn write_token_file(path: &Path, token: &str) -> io::Result<()> {
    std::fs::write(path, token)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

/// Constant-time byte comparison -- a token check must not leak how
/// many leading bytes matched through response-timing differences,
/// the ordinary bearer-token-comparison discipline regardless of how
/// small a purely-local socket's attack surface already is.
pub fn tokens_match(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

/// Parses the connection preamble -- the one frame a client must send
/// before anything else, `{"token": "..."}"` -- returning `None` for
/// anything that isn't that exact shape (including a token field of
/// the wrong type or a body that isn't even JSON) rather than ever
/// panicking on attacker-controlled bytes.
pub fn parse_preamble_token(bytes: &[u8]) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct Preamble {
        token: String,
    }
    serde_json::from_slice::<Preamble>(bytes)
        .ok()
        .map(|p| p.token)
}

/// This crate's own copy of `blueice_ipc`'s length-prefixed-JSON frame
/// shape (a `u32` little-endian byte length, then that many raw
/// bytes) -- `blueice_ipc::{read_frame_bytes, write_framed}` exist but
/// are crate-private, and this process deliberately never depends on
/// `blueice_ipc::automation`'s actual request/reply *types* at all
/// (everything past the preamble is opaque bytes it proxies, not a
/// message it decodes), so re-declaring just the byte-framing shape
/// here -- rather than making those two functions `pub` for a single
/// external caller -- keeps `blueice_ipc`'s own public surface
/// unchanged.
fn read_frame(r: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut len_bytes = [0u8; 4];
    r.read_exact(&mut len_bytes)?;
    let len = u32::from_le_bytes(len_bytes) as usize;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

/// Serves one already-accepted adapter connection: reads exactly one
/// preamble frame and checks it against `expected_token`. Only on a
/// match does this connect to `core_socket` and proxy bytes
/// bidirectionally, raw and unmodified, until either side closes -- a
/// wrong or malformed token closes the connection immediately without
/// `core_socket` ever being touched, so an unauthenticated peer can't
/// even learn whether a real `core` is listening on the far end.
pub fn serve_connection(
    mut client: UnixStream,
    expected_token: &str,
    core_socket: &Path,
) -> io::Result<()> {
    let preamble = read_frame(&mut client)?;
    let presented = parse_preamble_token(&preamble).unwrap_or_default();
    if !tokens_match(&presented, expected_token) {
        return Ok(());
    }
    let core = UnixStream::connect(core_socket)?;
    proxy_bidirectionally(client, core)
}

/// Relays bytes both ways between `client` and `core` until either
/// side reaches EOF, using a second thread for one direction since a
/// `UnixStream` half-duplex `io::copy` in each direction would
/// otherwise have to alternate reads instead of running concurrently.
/// `try_clone` shares the same underlying fd rather than opening a
/// second connection, matching the standard pattern for full-duplex
/// proxying over one socket.
fn proxy_bidirectionally(client: UnixStream, core: UnixStream) -> io::Result<()> {
    let mut client_read = client.try_clone()?;
    let mut core_write = core.try_clone()?;
    let forward = thread::spawn(move || {
        let _ = io::copy(&mut client_read, &mut core_write);
        let _ = core_write.shutdown(std::net::Shutdown::Write);
    });
    let mut core_read = core;
    let mut client_write = client;
    let _ = io::copy(&mut core_read, &mut client_write);
    let _ = client_write.shutdown(std::net::Shutdown::Write);
    let _ = forward.join();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::net::UnixListener;

    fn unique_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "blueice-automation-lib-test-{label}-{}",
            std::process::id()
        ))
    }

    #[test]
    fn generate_token_produces_64_hex_characters_and_never_repeats() {
        let a = generate_token().unwrap();
        let b = generate_token().unwrap();
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b, "two calls must not produce the same token");
    }

    #[test]
    fn tokens_match_requires_exact_equality() {
        assert!(tokens_match("abc", "abc"));
        assert!(!tokens_match("abc", "abd"));
        assert!(!tokens_match("abc", "ab"));
        assert!(tokens_match("", ""));
        assert!(tokens_match("nonempty", "nonempty"));
    }

    #[test]
    fn parse_preamble_token_accepts_only_the_exact_shape() {
        assert_eq!(
            parse_preamble_token(br#"{"token":"abc123"}"#),
            Some("abc123".to_string())
        );
        assert_eq!(parse_preamble_token(b"not json"), None);
        assert_eq!(parse_preamble_token(br#"{"nope":"abc"}"#), None);
        assert_eq!(parse_preamble_token(br#"{"token":42}"#), None);
    }

    #[test]
    fn write_token_file_writes_the_token_with_owner_only_permissions() {
        let path = unique_path("token-perms");
        let _ = std::fs::remove_file(&path);
        write_token_file(&path, "the-token").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "the-token");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let _ = std::fs::remove_file(&path);
    }

    /// A fake "core": accepts one connection, reads whatever frame(s)
    /// arrive, and echoes each one straight back -- enough to prove
    /// `serve_connection` really does relay bytes to and from whatever
    /// is listening at `core_socket`, without needing the real
    /// `blueice-core` binary in this crate's own unit tests (the real
    /// binary is exercised in `tests/automation_binary.rs` instead).
    fn spawn_echoing_fake_core(path: &Path) {
        let _ = std::fs::remove_file(path);
        let listener = UnixListener::bind(path).unwrap();
        thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            loop {
                let mut len_bytes = [0u8; 4];
                if stream.read_exact(&mut len_bytes).is_err() {
                    return;
                }
                let len = u32::from_le_bytes(len_bytes) as usize;
                let mut buf = vec![0u8; len];
                if stream.read_exact(&mut buf).is_err() {
                    return;
                }
                if stream.write_all(&len_bytes).is_err() || stream.write_all(&buf).is_err() {
                    return;
                }
            }
        });
    }

    fn write_frame(w: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
        let len = u32::try_from(bytes.len()).unwrap();
        w.write_all(&len.to_le_bytes())?;
        w.write_all(bytes)?;
        w.flush()
    }

    #[test]
    fn a_correct_token_gets_proxied_bytes_relayed_to_and_from_the_real_core_socket() {
        let core_path = unique_path("fake-core-correct");
        spawn_echoing_fake_core(&core_path);

        let (mut test_side, server_side) = UnixStream::pair().unwrap();
        let core_path_clone = core_path.clone();
        let worker = thread::spawn(move || {
            serve_connection(server_side, "the-right-token", &core_path_clone)
        });

        write_frame(&mut test_side, br#"{"token":"the-right-token"}"#).unwrap();
        write_frame(&mut test_side, b"hello core").unwrap();
        let echoed = read_frame(&mut test_side).unwrap();
        assert_eq!(echoed, b"hello core");

        drop(test_side);
        worker.join().unwrap().unwrap();
        let _ = std::fs::remove_file(&core_path);
    }

    #[test]
    fn a_wrong_token_is_rejected_and_core_socket_is_never_touched() {
        let core_path = unique_path("fake-core-wrong");
        // Deliberately never bind a listener at `core_path`: if
        // `serve_connection` tried to connect anyway, that connect
        // itself would fail loudly (`ConnectionRefused`) rather than
        // silently succeeding, so a passing test here really does
        // prove `core_socket` was never touched.
        let _ = std::fs::remove_file(&core_path);

        let (mut test_side, server_side) = UnixStream::pair().unwrap();
        let core_path_clone = core_path.clone();
        let worker =
            thread::spawn(move || serve_connection(server_side, "expected", &core_path_clone));

        write_frame(&mut test_side, br#"{"token":"wrong"}"#).unwrap();

        // The connection must be closed rather than proxied -- no
        // further bytes ever arrive, and the worker returns `Ok(())`
        // (a deliberate, silent rejection, not a propagated error).
        let mut buf = [0u8; 1];
        assert_eq!(
            test_side.read(&mut buf).unwrap(),
            0,
            "a wrong token must close the connection, not relay anything"
        );
        worker.join().unwrap().unwrap();
    }
}
