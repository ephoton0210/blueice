// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(unix)]

//! Exercises the actual compiled `blueice-automation` binary as a real
//! subprocess, proxying to a real, separately-spawned `blueice-core`
//! subprocess -- `main`'s own argument parsing, token issuance, socket
//! bind/accept and proxy wiring, none of which `src/bin/blueice-
//! automation.rs`'s unit tests touch (those cover `parse_args` in
//! isolation; `blueice_automation`'s own `lib.rs` tests cover
//! `serve_connection`'s logic against a fake "core" over an in-process
//! `UnixStream` pair). This is the "e2e test through the real public
//! interface" the project's Definition of Done asks for, matching
//! `blueice-engine`'s own `tests/core_binary.rs` -- except this file's
//! subject is *two* real subprocesses wired together, not one.

use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

// Short: a Unix socket path is capped at roughly 100 bytes total
// (`SUN_LEN`), same reasoning `blueice-engine`'s own
// `tests/core_binary.rs` documents at its `clearing_gatekeeper`
// helper -- so this deliberately doesn't spell out
// "blueice-automation-binary-test" in every path.
fn unique_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("bic-auto-{label}-{}.sock", std::process::id()))
}

fn wait_for(path: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    false
}

/// Connects to a socket a child process is still setting up -- see
/// `blueice-engine`'s own `tests/core_binary.rs` `connect` for why the
/// retry (bind vs. listen race) is needed.
fn connect(path: &Path) -> std::io::Result<UnixStream> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match UnixStream::connect(path) {
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
                ) && Instant::now() < deadline =>
            {
                thread::sleep(Duration::from_millis(10));
            }
            result => return result,
        }
    }
}

/// `blueice-core` is a sibling workspace binary, not this crate's own
/// -- `CARGO_BIN_EXE_*` is only set for binaries in the *current*
/// package, so it has to be located relative to this test binary's
/// own path instead, exactly like `blueice-mcp-server`'s
/// `sibling_core_binary` (`backend/mcp-server/src/lib.rs`) already
/// does for the same reason.
fn sibling_core_binary() -> PathBuf {
    let this_exe = std::env::current_exe().expect("failed to resolve the test binary's own path");
    let dir = this_exe.parent().unwrap_or_else(|| Path::new("."));
    let dir = if dir.file_name().is_some_and(|n| n == "deps") {
        dir.parent().unwrap_or(dir)
    } else {
        dir
    };
    dir.join(if cfg!(windows) {
        "blueice-core.exe"
    } else {
        "blueice-core"
    })
}

struct Spawned {
    core: Child,
    adapter: Child,
    frame_dir: PathBuf,
}

impl Drop for Spawned {
    fn drop(&mut self) {
        let _ = self.core.kill();
        let _ = self.core.wait();
        let _ = self.adapter.kill();
        let _ = self.adapter.wait();
        let _ = std::fs::remove_dir_all(&self.frame_dir);
    }
}

/// Spawns a real `blueice-core` (with its own `--automation-socket`)
/// and a real `blueice-automation` pointed at it, and waits for both
/// sockets and the adapter's token file to exist. Returns the
/// frontend/core socket, the adapter's own socket, and the token it
/// wrote -- the core socket is only needed so the test can complete
/// the frontend handshake that unblocks `core`'s dispatch loop (see
/// `blueice-engine`'s own `core_binary.rs` automation test for why
/// that handshake has to happen at all).
fn spawn_core_and_adapter(label: &str) -> (Spawned, PathBuf, PathBuf, PathBuf, String) {
    let core_socket = unique_path(&format!("{label}-c"));
    let core_automation_socket = unique_path(&format!("{label}-ca"));
    let adapter_socket = unique_path(&format!("{label}-a"));
    let token_path = unique_path(&format!("{label}-t"));
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-automation-binary-test-frames-{label}-{}",
        std::process::id()
    ));
    for path in [
        &core_socket,
        &core_automation_socket,
        &adapter_socket,
        &token_path,
    ] {
        let _ = std::fs::remove_file(path);
    }
    let _ = std::fs::remove_dir_all(&frame_dir);

    let mut core = Command::new(sibling_core_binary())
        .args([
            "--socket",
            core_socket.to_str().unwrap(),
            "--automation-socket",
            core_automation_socket.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
        ])
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn blueice-core");

    let adapter = Command::new(env!("CARGO_BIN_EXE_blueice-automation"))
        .args([
            "--socket",
            adapter_socket.to_str().unwrap(),
            "--core-socket",
            core_automation_socket.to_str().unwrap(),
            "--token-file",
            token_path.to_str().unwrap(),
        ])
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn blueice-automation");

    if !wait_for(&core_socket, Duration::from_secs(5)) {
        if let Ok(Some(status)) = core.try_wait() {
            let mut stderr = String::new();
            if let Some(mut s) = core.stderr.take() {
                use std::io::Read as _;
                let _ = s.read_to_string(&mut stderr);
            }
            panic!("blueice-core exited early with {status:?}, stderr: {stderr}");
        }
        panic!("blueice-core never created its frontend socket (still running)");
    }
    assert!(
        wait_for(&core_automation_socket, Duration::from_secs(5)),
        "blueice-core never created its automation socket"
    );
    assert!(
        wait_for(&adapter_socket, Duration::from_secs(5)),
        "blueice-automation never created its socket"
    );
    assert!(
        wait_for(&token_path, Duration::from_secs(5)),
        "blueice-automation never wrote its token file"
    );
    let token = std::fs::read_to_string(&token_path).expect("failed to read the token file");

    (
        Spawned {
            core,
            adapter,
            frame_dir,
        },
        core_socket,
        core_automation_socket,
        adapter_socket,
        token,
    )
}

fn write_frame(w: &mut impl std::io::Write, bytes: &[u8]) -> std::io::Result<()> {
    let len = u32::try_from(bytes.len()).unwrap();
    w.write_all(&len.to_le_bytes())?;
    w.write_all(bytes)?;
    w.flush()
}

fn read_frame(r: &mut impl std::io::Read) -> std::io::Result<Vec<u8>> {
    let mut len_bytes = [0u8; 4];
    r.read_exact(&mut len_bytes)?;
    let len = u32::from_le_bytes(len_bytes) as usize;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

#[test]
fn a_correctly_tokened_real_client_reaches_the_real_core_through_the_real_adapter() {
    let (spawned, core_socket, _core_automation_socket, adapter_socket, token) =
        spawn_core_and_adapter("ok");

    // `core`'s dispatch loop (and so its automation-request draining)
    // doesn't start until a frontend completes the handshake -- see
    // `blueice-engine`'s `tests/core_binary.rs` automation test for
    // the same requirement.
    let mut frontend = connect(&core_socket).expect("failed to connect to core's frontend socket");
    blueice_ipc::client_handshake(&mut frontend)
        .expect("the real core subprocess must complete the frontend handshake");

    let mut client = connect(&adapter_socket).expect("failed to connect to the real adapter");
    write_frame(&mut client, format!(r#"{{"token":"{token}"}}"#).as_bytes()).unwrap();

    blueice_ipc::automation::write_automation_request(
        &mut client,
        &blueice_ipc::automation::AutomationRequest::Hello {
            client_name: "adapter-e2e-test".to_string(),
            requested_capabilities: vec![blueice_ipc::automation::Capability::Inspection],
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::automation::read_automation_reply(&mut client).unwrap(),
        blueice_ipc::automation::AutomationReply::HelloAck { .. }
    ));

    drop(spawned);
}

#[test]
fn a_wrongly_tokened_real_client_is_rejected_before_reaching_core() {
    let (spawned, core_socket, _core_automation_socket, adapter_socket, _token) =
        spawn_core_and_adapter("bad");

    let mut frontend = connect(&core_socket).expect("failed to connect to core's frontend socket");
    blueice_ipc::client_handshake(&mut frontend)
        .expect("the real core subprocess must complete the frontend handshake");

    let mut client = connect(&adapter_socket).expect("failed to connect to the real adapter");
    write_frame(&mut client, br#"{"token":"definitely-not-it"}"#).unwrap();

    // The adapter must close the connection outright rather than
    // relaying to core -- reading any further frame either errors
    // (EOF) or, if it somehow succeeded, must never be a real
    // `AutomationReply`.
    let outcome = read_frame(&mut client);
    assert!(
        outcome.is_err(),
        "a wrong token must close the connection, not relay a reply from core"
    );

    drop(spawned);
}
