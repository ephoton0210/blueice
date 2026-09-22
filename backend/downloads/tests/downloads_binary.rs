// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The compiled `blueice-downloads` binary as a real subprocess: argument
//! handling, the socket it binds, orderly shutdown, and -- the one thing
//! only a real process can prove -- what a `kill -9` in the middle of a
//! download leaves behind for the next process to pick up. In the same
//! spirit as `blueice-engine`'s `core_binary.rs`: a Unix-socket server has
//! no display dependency, so nothing here needs to be excluded from
//! coverage as untestable wiring.

#[path = "../../net/tests/common/mod.rs"]
mod common;

use blueice_ipc::downloads::{
    DOWNLOADS_PROTOCOL_VERSION, DownloadsClient, DownloadsReply, DownloadsRequest, TransferInfo,
    TransferState,
};
use common::{FakeGatekeeper, GateReply, Resource, TempDir, TestServer, body};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const KIB: usize = 1024;
const MIB: usize = 1024 * 1024;

/// A spawned downloads process, killed on drop so a failing test can't
/// leave one running.
struct Process {
    child: Child,
    socket: PathBuf,
}

impl Process {
    fn spawn(dirs: &Dirs, gatekeeper_socket: &std::path::Path) -> Process {
        let child = Command::new(env!("CARGO_BIN_EXE_blueice-downloads"))
            .arg("--socket")
            .arg(dirs.socket())
            .arg("--download-dir")
            .arg(dirs.downloads.path())
            .arg("--data-dir")
            .arg(dirs.data.path())
            .arg("--gatekeeper-socket")
            .arg(gatekeeper_socket)
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn blueice-downloads");
        let process = Process {
            child,
            socket: dirs.socket(),
        };
        process.wait_until_listening();
        process
    }

    fn wait_until_listening(&self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while UnixStream::connect(&self.socket).is_err() {
            assert!(
                Instant::now() < deadline,
                "the process never started listening on {}",
                self.socket.display()
            );
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn client(&self) -> DownloadsClient<UnixStream> {
        DownloadsClient::connect(UnixStream::connect(&self.socket).unwrap()).expect("handshake")
    }

    fn wait_exit(&mut self, within: Duration) -> std::process::ExitStatus {
        let deadline = Instant::now() + within;
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "the process did not exit in time"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        // Ask it to stop first: a process that exits on its own writes its
        // coverage profile, one that is killed does not. Only a process that
        // ignores the request is killed.
        if self.child.try_wait().ok().flatten().is_none() {
            if let Ok(mut raw) = UnixStream::connect(&self.socket) {
                let _ = blueice_ipc::downloads::write_downloads_request(
                    &mut raw,
                    Some(1),
                    &DownloadsRequest::Hello {
                        protocol_version: DOWNLOADS_PROTOCOL_VERSION,
                    },
                );
                let _ = blueice_ipc::downloads::read_downloads_reply(&mut raw);
                let _ = blueice_ipc::downloads::write_downloads_request(
                    &mut raw,
                    Some(2),
                    &DownloadsRequest::Shutdown,
                );
                let _ = blueice_ipc::downloads::read_downloads_reply(&mut raw);
            }
            let deadline = Instant::now() + Duration::from_secs(5);
            while self.child.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(20));
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Dirs {
    downloads: TempDir,
    data: TempDir,
    runtime: TempDir,
}

impl Dirs {
    fn new() -> Self {
        Dirs {
            downloads: TempDir::new(),
            data: TempDir::new(),
            runtime: TempDir::new(),
        }
    }

    fn socket(&self) -> PathBuf {
        self.runtime.join("downloads.sock")
    }
}

#[test]
fn the_downloads_socket_is_owner_only_before_it_can_receive_credentials() {
    let dirs = Dirs::new();
    let gate = FakeGatekeeper::clear_all();
    let process = Process::spawn(&dirs, &gate.socket);
    assert_eq!(
        std::fs::metadata(&process.socket)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

fn wait_for(
    client: &mut DownloadsClient<UnixStream>,
    id: u64,
    what: &str,
    mut condition: impl FnMut(&TransferInfo) -> bool,
) -> TransferInfo {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let info = client.get(id).unwrap();
        if condition(&info) {
            return info;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}; last {info:?}"
        );
        thread::sleep(Duration::from_millis(15));
    }
}

fn shutdown(process: &Process) {
    let mut raw = UnixStream::connect(&process.socket).unwrap();
    blueice_ipc::downloads::write_downloads_request(
        &mut raw,
        Some(1),
        &DownloadsRequest::Hello {
            protocol_version: DOWNLOADS_PROTOCOL_VERSION,
        },
    )
    .unwrap();
    blueice_ipc::downloads::read_downloads_reply(&mut raw).unwrap();
    blueice_ipc::downloads::write_downloads_request(&mut raw, Some(2), &DownloadsRequest::Shutdown)
        .unwrap();
    assert_eq!(
        blueice_ipc::downloads::read_downloads_reply(&mut raw).unwrap(),
        (Some(2), DownloadsReply::Ok)
    );
}

#[test]
fn the_binary_completes_a_download_over_its_socket_and_exits_cleanly_on_shutdown() {
    let dirs = Dirs::new();
    let gate = FakeGatekeeper::clear_all();
    let server = TestServer::start();
    server.serve("/big.bin", Resource::new(body(MIB)));
    let mut process = Process::spawn(&dirs, &gate.socket);

    let mut client = process.client();
    let id = client
        .start(&server.url("/big.bin"), None, false)
        .unwrap()
        .id;
    let done = wait_for(&mut client, id, "completion", |i| {
        i.state == TransferState::Completed
    });
    assert_eq!(std::fs::read(&done.dest_path).unwrap(), body(MIB));
    assert!(
        dirs.data.join("transfers.json").exists(),
        "history is persisted"
    );
    assert!(
        gate.requests().len() >= 2,
        "the process reviewed the download itself: {:?}",
        gate.requests()
    );

    drop(client);
    shutdown(&process);
    let status = process.wait_exit(Duration::from_secs(10));
    assert!(status.success(), "{status:?}");
    assert!(
        !dirs.socket().exists(),
        "an orderly exit removes its socket"
    );
}

#[test]
fn the_binary_blocks_what_the_gatekeeper_rejects() {
    let dirs = Dirs::new();
    let gate = FakeGatekeeper::start(|_| GateReply::Reject {
        reason: "untrusted".to_string(),
        category: "test".to_string(),
    });
    let server = TestServer::start();
    server.serve("/f.bin", Resource::new(body(1_000)));
    let process = Process::spawn(&dirs, &gate.socket);
    let mut client = process.client();
    let id = client.start(&server.url("/f.bin"), None, false).unwrap().id;
    let blocked = wait_for(&mut client, id, "a block", |i| {
        i.state == TransferState::Blocked
    });
    assert_eq!(blocked.blocked.unwrap().category, "test");
    assert!(server.requests().is_empty());
}

#[test]
fn a_process_killed_mid_download_leaves_it_paused_and_resumable_for_the_next_one() {
    let dirs = Dirs::new();
    let gate = FakeGatekeeper::clear_all();
    let server = TestServer::start();
    // 16 MiB over the default 8 connections at ~30 ms per 16 KiB takes several
    // seconds, so the default one-second checkpoint interval has time to record
    // real progress before the download could finish.
    server.serve(
        "/big.bin",
        Resource {
            chunk: 16 * KIB,
            delay_per_chunk: Duration::from_millis(30),
            ..Resource::new(body(16 * MIB))
        },
    );

    let mut first = Process::spawn(&dirs, &gate.socket);
    let mut client = first.client();
    let id = client
        .start(&server.url("/big.bin"), None, false)
        .unwrap()
        .id;
    let running = wait_for(&mut client, id, "real progress", |i| {
        i.state == TransferState::Active && i.completed_bytes > 400 * KIB as u64
    });
    let dest = PathBuf::from(&running.dest_path);
    // Wait for a checkpoint that has recorded progress, then die without warning.
    let deadline = Instant::now() + Duration::from_secs(10);
    while blueice_net::download::sidecar::Sidecar::load(&dest)
        .is_none_or(|s| s.segments.iter().all(|seg| seg.pos == seg.start))
    {
        assert!(
            Instant::now() < deadline,
            "no checkpoint with progress was ever written"
        );
        thread::sleep(Duration::from_millis(20));
    }
    first.child.kill().unwrap(); // SIGKILL: no chance to save anything more
    first.child.wait().unwrap();
    drop(client);
    assert!(
        dirs.socket().exists(),
        "a killed process leaves its socket file behind; the next one must cope"
    );

    let second = Process::spawn(&dirs, &gate.socket);
    let mut client = second.client();
    let found = client.get(id).unwrap();
    assert_eq!(
        found.state,
        TransferState::Paused,
        "an interrupted transfer comes back paused: {found:?}"
    );
    assert!(
        found.events.iter().any(|e| e.message.contains("restarted")),
        "and says why: {:?}",
        found.events
    );

    server.update("/big.bin", |r| r.delay_per_chunk = Duration::ZERO);
    let requests_before = server.requests().len();
    client.resume(id).unwrap();
    let done = wait_for(&mut client, id, "completion after the restart", |i| {
        i.state == TransferState::Completed
    });
    assert_eq!(
        std::fs::read(&done.dest_path).unwrap(),
        body(16 * MIB),
        "the file survives a SIGKILL intact"
    );
    assert!(
        done.events.iter().any(|e| e.message.contains("resumed")),
        "it resumed from the checkpoint rather than restarting: {:?}",
        done.events
    );
    let refetched: u64 = server
        .requests()
        .iter()
        .skip(requests_before)
        .filter(|r| r.status == 206)
        .filter_map(|r| r.range.as_deref())
        .filter_map(|r| r.strip_prefix("bytes="))
        .filter_map(|r| r.split_once('-'))
        .map(|(a, b)| b.parse::<u64>().unwrap() - a.parse::<u64>().unwrap() + 1)
        .sum();
    assert!(
        refetched < 16 * MIB as u64,
        "only what was missing was fetched again ({refetched} bytes)"
    );
}

#[test]
fn a_second_process_will_not_start_beside_a_live_one() {
    let dirs = Dirs::new();
    let gate = FakeGatekeeper::clear_all();
    let first = Process::spawn(&dirs, &gate.socket);
    let output = Command::new(env!("CARGO_BIN_EXE_blueice-downloads"))
        .arg("--socket")
        .arg(dirs.socket())
        .arg("--download-dir")
        .arg(dirs.downloads.path())
        .arg("--data-dir")
        .arg(dirs.data.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("already listening"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    // ...and the first is undisturbed.
    assert!(first.client().list(None).unwrap().is_empty());
}

#[test]
fn bad_arguments_exit_with_status_2_and_the_usage() {
    let output = Command::new(env!("CARGO_BIN_EXE_blueice-downloads"))
        .arg("--bogus")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("usage:"));
}
