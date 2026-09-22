// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The downloads process through its wire protocol, in-process: a real
//! `TransferManager` behind a real Unix-socket server, driven by the
//! shared `DownloadsClient`, against `blueice-net`'s local HTTP test
//! server and a fake gatekeeper (`phase-10-download-manager/PLAN.md`).
//! The real *binary* is exercised separately in `downloads_binary.rs`.

// One copy of the test server and fake gatekeeper, shared with
// `blueice-net`'s own tests rather than duplicated.
#[path = "../../net/tests/common/mod.rs"]
mod common;

use blueice_downloads::manager::{ManagerConfig, SubscriptionUpdate, TransferManager};
use blueice_downloads::server::serve;
use blueice_ipc::downloads::{
    ClientError, DOWNLOADS_PROTOCOL_VERSION, DownloadsClient, DownloadsReply, DownloadsRequest,
    ErrorCode, TransferInfo, TransferState, read_downloads_reply, write_downloads_reply,
    write_downloads_request,
};
use blueice_net::download::sidecar::{part_path, sidecar_path};
use blueice_net::download::{DownloadOptions, MAX_TRANSFER_TEXT_BYTES};
use common::{FakeGatekeeper, GateReply, Resource, TempDir, TestServer, body};
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const KIB: usize = 1024;
const MIB: usize = 1024 * 1024;

fn fast_options() -> DownloadOptions {
    DownloadOptions {
        max_connections: 4,
        min_split_bytes: 64 * KIB as u64,
        max_retries: 3,
        retry_backoff_base: Duration::from_millis(10),
        retry_backoff_max: Duration::from_millis(40),
        buffer_bytes: 16 * KIB,
        checkpoint_interval: Duration::from_millis(30),
        tick: Duration::from_millis(15),
        ..DownloadOptions::default()
    }
}

/// Everything one test needs: a content server, a gatekeeper, the
/// downloads directory and data directory, and a running server.
struct Rig {
    server: TestServer,
    gate: FakeGatekeeper,
    download_dir: TempDir,
    data_dir: TempDir,
    socket_dir: TempDir,
    manager: Arc<TransferManager>,
    stop: Arc<AtomicBool>,
    serving: Option<JoinHandle<()>>,
}

impl Rig {
    fn new() -> Self {
        Rig::with(|_| {})
    }

    fn with(tweak: impl FnOnce(&mut ManagerConfig)) -> Self {
        let gate = FakeGatekeeper::clear_all();
        Rig::with_gate(gate, tweak)
    }

    fn with_gate(gate: FakeGatekeeper, tweak: impl FnOnce(&mut ManagerConfig)) -> Self {
        let download_dir = TempDir::new();
        let data_dir = TempDir::new();
        let mut config = ManagerConfig::new(download_dir.path(), data_dir.path(), &gate.socket);
        config.options = fast_options();
        config.review_timeout = Duration::from_secs(2);
        tweak(&mut config);
        let manager = TransferManager::open(config).expect("open the manager");
        Rig::around(gate, download_dir, data_dir, manager)
    }

    fn around(
        gate: FakeGatekeeper,
        download_dir: TempDir,
        data_dir: TempDir,
        manager: Arc<TransferManager>,
    ) -> Self {
        let socket_dir = TempDir::new();
        let listener = UnixListener::bind(socket_dir.join("d.sock")).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let (m, s) = (manager.clone(), stop.clone());
        let serving = Some(thread::spawn(move || serve(listener, m, s)));
        Rig {
            server: TestServer::start(),
            gate,
            download_dir,
            data_dir,
            socket_dir,
            manager,
            stop,
            serving,
        }
    }

    fn socket(&self) -> PathBuf {
        self.socket_dir.join("d.sock")
    }

    fn client(&self) -> DownloadsClient<UnixStream> {
        DownloadsClient::connect(UnixStream::connect(self.socket()).unwrap()).expect("handshake")
    }

    fn url(&self, path: &str) -> String {
        self.server.url(path)
    }

    fn in_dir(&self, name: &str) -> PathBuf {
        std::fs::canonicalize(self.download_dir.path())
            .unwrap()
            .join(name)
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.manager.shutdown();
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.serving.take() {
            let _ = handle.join();
        }
    }
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
            "timed out waiting for {what}; last state {info:?}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn wait_state(
    client: &mut DownloadsClient<UnixStream>,
    id: u64,
    state: TransferState,
) -> TransferInfo {
    wait_for(client, id, &format!("state {state}"), |i| i.state == state)
}

fn has_event(info: &TransferInfo, needle: &str) -> bool {
    info.events.iter().any(|e| e.message.contains(needle))
}

fn remote_code(result: Result<impl std::fmt::Debug, ClientError>) -> ErrorCode {
    match result {
        Err(ClientError::Remote { code, .. }) => code,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// Make the actual kernel receive window deliberately small, then report the
/// effective value (kernels may clamp or double the requested value). The
/// slow-subscriber regression below uses the value rather than guessing a
/// platform default, so it proves real socket backpressure rather than merely
/// running for a long time with an unread peer.
fn constrain_receive_buffer(stream: &UnixStream) -> usize {
    let requested: libc::c_int = 1_024;
    let result = unsafe {
        libc::setsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_RCVBUF,
            (&requested as *const libc::c_int).cast(),
            std::mem::size_of_val(&requested) as libc::socklen_t,
        )
    };
    assert_eq!(result, 0, "could not reduce the test socket receive window");

    let mut actual: libc::c_int = 0;
    let mut len = std::mem::size_of_val(&actual) as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_RCVBUF,
            (&mut actual as *mut libc::c_int).cast(),
            &mut len,
        )
    };
    assert_eq!(
        result, 0,
        "could not inspect the test socket receive window"
    );
    usize::try_from(actual).expect("the kernel returned a positive receive window")
}

/// Bytes the subscribed peer has deliberately left unread in its Unix socket.
/// This is an observation only; it never drains the socket or wakes the peer.
fn unread_socket_bytes(stream: &UnixStream) -> usize {
    let mut unread: libc::c_int = 0;
    let result = unsafe { libc::ioctl(stream.as_raw_fd(), libc::FIONREAD, &mut unread) };
    assert_eq!(result, 0, "could not inspect unread socket bytes");
    usize::try_from(unread).expect("the kernel returned a non-negative unread byte count")
}

fn slow(len: usize) -> Resource {
    Resource {
        chunk: 16 * KIB,
        delay_per_chunk: Duration::from_millis(10),
        ..Resource::new(body(len))
    }
}

// ---- the happy path and naming -------------------------------------------

#[test]
fn a_started_download_runs_to_completion_and_is_visible_over_the_protocol() {
    let rig = Rig::new();
    rig.server.serve("/big.bin", Resource::new(body(MIB)));
    let mut client = rig.client();

    let started = client.start(&rig.url("/big.bin"), None, false).unwrap();
    assert_eq!(started.id, 1);
    assert_eq!(started.state, TransferState::Queued);
    assert_eq!(started.url, rig.url("/big.bin"));

    let done = wait_state(&mut client, 1, TransferState::Completed);
    assert_eq!(done.dest_path, rig.in_dir("big.bin").to_string_lossy());
    assert_eq!(std::fs::read(rig.in_dir("big.bin")).unwrap(), body(MIB));
    assert_eq!(
        (done.total_bytes, done.completed_bytes),
        (Some(MIB as u64), MIB as u64)
    );
    assert!(done.finished_at_ms.is_some() && done.created_at_ms > 0);
    assert!(done.generation > started.generation);
    assert!(has_event(&done, "completed"), "{:?}", done.events);
    assert_eq!(client.list(None).unwrap().len(), 1);
}

#[test]
fn an_empty_download_reaches_completed_and_releases_the_manager_slot() {
    // `Transfer::begin` can finish an empty body synchronously, before it
    // starts its coordinator.  The manager must still apply that final
    // snapshot: otherwise the record remains `AwaitingClearance` forever.
    let rig = Rig::new();
    rig.server.serve("/empty.bin", Resource::new(Vec::new()));
    let mut client = rig.client();

    let id = client
        .start(&rig.url("/empty.bin"), None, false)
        .unwrap()
        .id;
    let done = wait_for(&mut client, id, "an empty download to complete", |info| {
        info.state.is_terminal()
    });

    assert_eq!(done.state, TransferState::Completed, "{done:?}");
    assert_eq!((done.total_bytes, done.completed_bytes), (Some(0), 0));
    assert!(done.finished_at_ms.is_some());
    assert!(
        rig.manager.is_idle(),
        "a completed empty download cannot retain a worker slot"
    );
    assert_eq!(
        std::fs::read(rig.in_dir("empty.bin")).unwrap(),
        Vec::<u8>::new()
    );
}

#[test]
fn a_file_name_from_the_server_is_used_and_sanitized() {
    let rig = Rig::new();
    rig.server.serve(
        "/x",
        Resource {
            content_disposition: Some("attachment; filename=\"../../evil.sh\"".to_string()),
            ..Resource::new(body(10_000))
        },
    );
    let mut client = rig.client();
    let id = client.start(&rig.url("/x"), None, false).unwrap().id;
    let done = wait_state(&mut client, id, TransferState::Completed);
    assert_eq!(
        done.dest_path,
        rig.in_dir("evil.sh").to_string_lossy(),
        "the download cannot leave the downloads directory through a file name"
    );
}

#[test]
fn a_taken_name_gets_a_number_even_when_two_downloads_race_for_it() {
    let rig = Rig::new();
    rig.server.serve("/f.bin", slow(300 * KIB));
    let mut client = rig.client();
    let a = client.start(&rig.url("/f.bin"), None, false).unwrap().id;
    let b = client.start(&rig.url("/f.bin"), None, false).unwrap().id;
    let (a, b) = (
        wait_state(&mut client, a, TransferState::Completed),
        wait_state(&mut client, b, TransferState::Completed),
    );
    let mut names: Vec<String> = [a.dest_path, b.dest_path]
        .iter()
        .map(|p| {
            PathBuf::from(p)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    assert_eq!(names, vec!["f (1).bin".to_string(), "f.bin".to_string()]);
    assert_eq!(std::fs::read(rig.in_dir("f.bin")).unwrap(), body(300 * KIB));
    assert_eq!(
        std::fs::read(rig.in_dir("f (1).bin")).unwrap(),
        body(300 * KIB)
    );
}

#[test]
fn a_requested_destination_is_used_inside_the_download_directory() {
    let rig = Rig::new();
    rig.server.serve("/f.bin", Resource::new(body(10_000)));
    let mut client = rig.client();
    let id = client
        .start(&rig.url("/f.bin"), Some("sub/dir/mine.bin"), false)
        .unwrap()
        .id;
    let done = wait_state(&mut client, id, TransferState::Completed);
    assert_eq!(
        done.dest_path,
        rig.in_dir("sub/dir/mine.bin").to_string_lossy()
    );
    assert_eq!(
        std::fs::read(rig.in_dir("sub/dir/mine.bin")).unwrap(),
        body(10_000)
    );
}

#[test]
fn unsafe_or_pointless_requests_are_refused_up_front_and_create_no_transfer() {
    let rig = Rig::new();
    rig.server.serve("/f.bin", Resource::new(body(100)));
    let mut client = rig.client();
    for dest in [
        "/etc/passwd",
        "../escape.bin",
        "a/../../escape.bin",
        "bad:name.bin",
        "",
    ] {
        assert_eq!(
            remote_code(client.start(&rig.url("/f.bin"), Some(dest), false)),
            ErrorCode::InvalidRequest,
            "{dest:?}"
        );
    }
    for url in [
        "ftp://alice@example.com/f",
        "not a url",
        "file:///etc/passwd",
        "",
        "https://alice:secret@example.com/f",
        "sftp://alice:secret@example.com/f",
        "ftps://alice:secret@example.com/f",
    ] {
        assert_eq!(
            remote_code(client.start(url, None, false)),
            ErrorCode::InvalidRequest,
            "{url:?}"
        );
    }
    assert!(
        client.list(None).unwrap().is_empty(),
        "a refused request must not leave a transfer behind"
    );
    assert!(rig.server.requests().is_empty());
}

#[test]
fn an_existing_requested_destination_is_refused_unless_overwriting() {
    let rig = Rig::new();
    rig.server.serve("/f.bin", Resource::new(body(10_000)));
    std::fs::write(rig.in_dir("taken.bin"), b"precious").unwrap();
    let mut client = rig.client();
    assert_eq!(
        remote_code(client.start(&rig.url("/f.bin"), Some("taken.bin"), false)),
        ErrorCode::InvalidRequest
    );
    assert_eq!(std::fs::read(rig.in_dir("taken.bin")).unwrap(), b"precious");

    let id = client
        .start(&rig.url("/f.bin"), Some("taken.bin"), true)
        .unwrap()
        .id;
    wait_state(&mut client, id, TransferState::Completed);
    assert_eq!(
        std::fs::read(rig.in_dir("taken.bin")).unwrap(),
        body(10_000)
    );
}

#[test]
fn two_transfers_may_not_target_the_same_requested_destination() {
    let rig = Rig::new();
    rig.server.serve("/f.bin", slow(MIB));
    let mut client = rig.client();
    client
        .start(&rig.url("/f.bin"), Some("same.bin"), false)
        .unwrap();
    assert_eq!(
        remote_code(client.start(&rig.url("/f.bin"), Some("same.bin"), true)),
        ErrorCode::InvalidRequest
    );
}

#[test]
fn case_only_destination_names_cannot_claim_the_same_file() {
    // macOS commonly has a case-insensitive volume.  Reject this universally
    // rather than letting platform-dependent path equality make two jobs
    // share one `.blueice-part` file.
    let rig = Rig::new();
    rig.server.serve("/f.bin", slow(MIB));
    let mut client = rig.client();
    client
        .start(&rig.url("/f.bin"), Some("Report.bin"), false)
        .unwrap();

    assert_eq!(
        remote_code(client.start(&rig.url("/f.bin"), Some("report.bin"), false)),
        ErrorCode::InvalidRequest
    );
}

#[test]
fn a_failed_transfer_cannot_resume_while_another_transfer_claims_its_destination() {
    let rig = Rig::new();
    rig.server.serve("/big.bin", slow(MIB));
    let mut client = rig.client();
    let failed = client
        .start(&rig.url("/missing.bin"), Some("same.bin"), false)
        .unwrap()
        .id;
    wait_state(&mut client, failed, TransferState::Failed);

    let current = client
        .start(&rig.url("/big.bin"), Some("same.bin"), false)
        .unwrap()
        .id;
    wait_for(
        &mut client,
        current,
        "the replacement transfer to claim its destination",
        |info| info.state == TransferState::Active,
    );

    assert_eq!(remote_code(client.resume(failed)), ErrorCode::InvalidState);
    assert_eq!(
        client.get(failed).unwrap().state,
        TransferState::Failed,
        "a refused resume leaves the old record untouched"
    );
}

#[test]
fn requested_destinations_cannot_name_download_manager_internal_files() {
    let rig = Rig::new();
    rig.server.serve("/f.bin", Resource::new(body(1_000)));
    let mut client = rig.client();

    for dest in ["f.bin.blueice-part", "f.bin.blueice-part.json", "f.bin.tmp"] {
        assert_eq!(
            remote_code(client.start(&rig.url("/f.bin"), Some(dest), false)),
            ErrorCode::InvalidRequest,
            "{dest}"
        );
    }
    assert!(client.list(None).unwrap().is_empty());
}

// ---- the gatekeeper ------------------------------------------------------

#[test]
fn a_url_the_gatekeeper_rejects_is_blocked_before_any_network_request() {
    let gate = FakeGatekeeper::start(|_| GateReply::Reject {
        reason: "known phishing domain".to_string(),
        category: "known-bad-domain".to_string(),
    });
    let rig = Rig::with_gate(gate, |_| {});
    rig.server.serve("/f.bin", Resource::new(body(1_000)));
    let mut client = rig.client();
    let id = client.start(&rig.url("/f.bin"), None, false).unwrap().id;

    let blocked = wait_state(&mut client, id, TransferState::Blocked);
    let why = blocked
        .blocked
        .clone()
        .expect("a blocked transfer says why");
    assert_eq!(
        (why.reason.as_str(), why.category.as_str()),
        ("known phishing domain", "known-bad-domain")
    );
    assert!(
        has_event(&blocked, "blocked by the gatekeeper"),
        "{:?}",
        blocked.events
    );
    assert!(
        rig.server.requests().is_empty(),
        "not one request may reach the server for a rejected URL"
    );
    assert_eq!(
        std::fs::read_dir(rig.download_dir.path()).unwrap().count(),
        0
    );
}

#[test]
fn a_download_the_gatekeeper_rejects_after_the_probe_transfers_nothing() {
    let gate = FakeGatekeeper::start(|req| match req {
        blueice_ipc::gatekeeper::GatekeeperRequest::CheckDownload { .. } => GateReply::Reject {
            reason: "executable from an untrusted origin".to_string(),
            category: "dangerous-file-type".to_string(),
        },
        _ => GateReply::Clear,
    });
    let rig = Rig::with_gate(gate, |_| {});
    rig.server.serve("/setup.exe", Resource::new(body(MIB)));
    let mut client = rig.client();
    let id = client
        .start(&rig.url("/setup.exe"), None, false)
        .unwrap()
        .id;

    let blocked = wait_state(&mut client, id, TransferState::Blocked);
    assert_eq!(blocked.blocked.unwrap().category, "dangerous-file-type");
    assert_eq!(
        rig.server.requests().len(),
        1,
        "only the probe went out: {:?}",
        rig.server.requests()
    );
    assert_eq!(
        std::fs::read_dir(rig.download_dir.path()).unwrap().count(),
        0,
        "nothing was created on disk"
    );
    // The download stage saw what the probe learned.
    assert!(rig.gate.requests().iter().any(|r| matches!(r, blueice_ipc::gatekeeper::GatekeeperRequest::CheckDownload { file_name, total_bytes: Some(t), .. } if file_name == "setup.exe" && *t == MIB as u64)));
}

#[test]
fn an_unreachable_gatekeeper_blocks_the_download_fail_closed() {
    let dead = TempDir::new();
    let dead_socket = dead.join("nobody.sock");
    let rig = Rig::with(move |c| c.gatekeeper_socket = dead_socket);
    rig.server.serve("/f.bin", Resource::new(body(1_000)));
    let mut client = rig.client();
    let id = client.start(&rig.url("/f.bin"), None, false).unwrap().id;
    let blocked = wait_state(&mut client, id, TransferState::Blocked);
    assert_eq!(blocked.blocked.unwrap().category, "gatekeeper-unavailable");
    assert!(rig.server.requests().is_empty());
}

// ---- failures ------------------------------------------------------------

#[test]
fn a_missing_file_fails_with_the_servers_status_and_is_not_a_block() {
    let rig = Rig::new();
    let mut client = rig.client();
    let id = client.start(&rig.url("/nope.bin"), None, false).unwrap().id;
    let failed = wait_state(&mut client, id, TransferState::Failed);
    assert!(
        failed.last_error.as_deref().unwrap_or("").contains("404"),
        "{:?}",
        failed.last_error
    );
    assert!(failed.blocked.is_none());
}

#[test]
fn a_transient_probe_failure_is_retried_before_giving_up() {
    let rig = Rig::new();
    // The very first request is the probe.
    rig.server.serve(
        "/f.bin",
        Resource {
            fail_statuses: vec![503],
            ..Resource::new(body(10_000))
        },
    );
    let mut client = rig.client();
    let id = client.start(&rig.url("/f.bin"), None, false).unwrap().id;
    let done = wait_state(&mut client, id, TransferState::Completed);
    assert!(has_event(&done, "retrying"), "{:?}", done.events);
    assert_eq!(std::fs::read(rig.in_dir("f.bin")).unwrap(), body(10_000));
}

// ---- pause / resume / cancel --------------------------------------------

#[test]
fn pause_then_resume_completes_the_download_and_reviews_it_again() {
    let rig = Rig::new();
    rig.server.serve("/big.bin", slow(4 * MIB));
    let mut client = rig.client();
    let id = client.start(&rig.url("/big.bin"), None, false).unwrap().id;
    wait_for(&mut client, id, "progress", |i| {
        i.state == TransferState::Active && i.completed_bytes > 300 * KIB as u64
    });

    let paused = client.pause(id).unwrap();
    assert_eq!(paused.state, TransferState::Paused);
    assert!(paused.completed_bytes > 0 && paused.completed_bytes < 4 * MIB as u64);
    let dest = PathBuf::from(&paused.dest_path);
    assert!(part_path(&dest).exists() && sidecar_path(&dest).exists() && !dest.exists());

    let resumed = client.resume(id).unwrap();
    assert!(
        matches!(
            resumed.state,
            TransferState::Queued | TransferState::AwaitingClearance | TransferState::Active
        ),
        "{:?}",
        resumed.state
    );
    let done = wait_state(&mut client, id, TransferState::Completed);
    assert_eq!(std::fs::read(&dest).unwrap(), body(4 * MIB));
    assert!(has_event(&done, "resumed"), "{:?}", done.events);
    let url_checks = rig
        .gate
        .requests()
        .iter()
        .filter(|r| {
            matches!(
                r,
                blueice_ipc::gatekeeper::GatekeeperRequest::CheckUrl { .. }
            )
        })
        .count();
    assert_eq!(
        url_checks, 2,
        "a resume goes through the gatekeeper again: a verdict can change between pause and resume"
    );
}

#[test]
fn cancelling_a_running_download_removes_its_partial_files() {
    let rig = Rig::new();
    rig.server.serve("/big.bin", slow(4 * MIB));
    let mut client = rig.client();
    let id = client.start(&rig.url("/big.bin"), None, false).unwrap().id;
    let active = wait_for(&mut client, id, "progress", |i| {
        i.state == TransferState::Active && i.completed_bytes > 100 * KIB as u64
    });
    let dest = PathBuf::from(&active.dest_path);

    assert_eq!(client.cancel(id).unwrap().state, TransferState::Cancelled);
    assert!(!dest.exists() && !part_path(&dest).exists() && !sidecar_path(&dest).exists());
    assert_eq!(
        client.get(id).unwrap().state,
        TransferState::Cancelled,
        "a cancelled transfer stays visible"
    );
}

#[test]
fn cancelling_a_paused_download_removes_its_partial_files_too() {
    let rig = Rig::new();
    rig.server.serve("/big.bin", slow(4 * MIB));
    let mut client = rig.client();
    let id = client.start(&rig.url("/big.bin"), None, false).unwrap().id;
    wait_for(&mut client, id, "progress", |i| {
        i.state == TransferState::Active && i.completed_bytes > 100 * KIB as u64
    });
    let dest = PathBuf::from(client.pause(id).unwrap().dest_path);
    assert!(sidecar_path(&dest).exists());
    assert_eq!(client.cancel(id).unwrap().state, TransferState::Cancelled);
    assert!(!part_path(&dest).exists() && !sidecar_path(&dest).exists());
}

#[test]
fn only_the_configured_number_of_transfers_run_at_once() {
    let rig = Rig::with(|c| c.max_concurrent = 1);
    rig.server.serve("/a.bin", slow(MIB));
    rig.server.serve("/b.bin", Resource::new(body(10_000)));
    let mut client = rig.client();
    let a = client.start(&rig.url("/a.bin"), None, false).unwrap().id;
    let b = client.start(&rig.url("/b.bin"), None, false).unwrap().id;

    wait_for(&mut client, a, "the first to be running", |i| {
        i.state == TransferState::Active
    });
    assert_eq!(
        client.get(b).unwrap().state,
        TransferState::Queued,
        "the second waits for a free slot"
    );
    wait_state(&mut client, a, TransferState::Completed);
    wait_state(&mut client, b, TransferState::Completed);
}

#[test]
fn pausing_a_queued_transfer_takes_it_out_of_the_queue() {
    let rig = Rig::with(|c| c.max_concurrent = 1);
    rig.server.serve("/a.bin", slow(MIB));
    rig.server.serve("/b.bin", Resource::new(body(10_000)));
    let mut client = rig.client();
    let a = client.start(&rig.url("/a.bin"), None, false).unwrap().id;
    let b = client.start(&rig.url("/b.bin"), None, false).unwrap().id;
    wait_for(&mut client, a, "the first to be running", |i| {
        i.state == TransferState::Active
    });

    assert_eq!(client.pause(b).unwrap().state, TransferState::Paused);
    wait_state(&mut client, a, TransferState::Completed);
    // `Completed` is published by the engine just before its job releases
    // the queue slot.  Wait for that release/scheduling boundary, rather
    // than hoping a fixed delay lets a wrongly restarted `b` show itself.
    wait_until(|| rig.manager.is_idle());
    assert_eq!(
        client.get(b).unwrap().state,
        TransferState::Paused,
        "a paused transfer must not start when a slot frees up"
    );
    client.resume(b).unwrap();
    wait_state(&mut client, b, TransferState::Completed);
}

#[test]
fn pausing_during_the_review_stops_before_anything_touches_the_network() {
    // The gatekeeper takes a while to answer the URL check.
    let gate = FakeGatekeeper::start(|_| GateReply::SlowClear(Duration::from_millis(400)));
    let rig = Rig::with_gate(gate, |c| c.review_timeout = Duration::from_secs(3));
    rig.server.serve("/f.bin", Resource::new(body(10_000)));
    let mut client = rig.client();
    let id = client.start(&rig.url("/f.bin"), None, false).unwrap().id;
    wait_for(&mut client, id, "the review to begin", |i| {
        i.state == TransferState::AwaitingClearance
    });

    let paused = client.pause(id).unwrap();
    assert_eq!(paused.state, TransferState::Paused);
    assert!(
        has_event(&paused, "paused before the transfer started"),
        "{:?}",
        paused.events
    );
    assert!(
        rig.server.requests().is_empty(),
        "the job stopped at its next checkpoint, before the probe"
    );
}

#[test]
fn cancelling_during_the_review_ends_it_without_creating_anything() {
    let gate = FakeGatekeeper::start(|_| GateReply::SlowClear(Duration::from_millis(400)));
    let rig = Rig::with_gate(gate, |c| c.review_timeout = Duration::from_secs(3));
    rig.server.serve("/f.bin", Resource::new(body(10_000)));
    let mut client = rig.client();
    let id = client.start(&rig.url("/f.bin"), None, false).unwrap().id;
    wait_for(&mut client, id, "the review to begin", |i| {
        i.state == TransferState::AwaitingClearance
    });

    assert_eq!(client.cancel(id).unwrap().state, TransferState::Cancelled);
    assert!(rig.server.requests().is_empty());
    assert_eq!(
        std::fs::read_dir(rig.download_dir.path()).unwrap().count(),
        0
    );
}

#[test]
fn a_failed_download_can_be_resumed_once_the_problem_is_fixed() {
    let rig = Rig::new();
    let mut client = rig.client();
    let id = client.start(&rig.url("/late.bin"), None, false).unwrap().id; // not there yet
    wait_state(&mut client, id, TransferState::Failed);

    rig.server.serve("/late.bin", Resource::new(body(20_000)));
    client.resume(id).unwrap();
    let done = wait_state(&mut client, id, TransferState::Completed);
    assert!(
        done.last_error.is_none(),
        "a completed transfer carries no stale error: {:?}",
        done.last_error
    );
    assert_eq!(std::fs::read(&done.dest_path).unwrap(), body(20_000));
}

#[test]
fn a_blocked_download_is_reviewed_again_on_resume_and_can_now_proceed() {
    // A verdict can change: the gatekeeper rejects at first, then clears.
    let allow = Arc::new(AtomicBool::new(false));
    let flag = allow.clone();
    let gate = FakeGatekeeper::start(move |_| {
        if flag.load(Ordering::SeqCst) {
            GateReply::Clear
        } else {
            GateReply::Reject {
                reason: "not yet".to_string(),
                category: "test".to_string(),
            }
        }
    });
    let rig = Rig::with_gate(gate, |_| {});
    rig.server.serve("/f.bin", Resource::new(body(5_000)));
    let mut client = rig.client();
    let id = client.start(&rig.url("/f.bin"), None, false).unwrap().id;
    let blocked = wait_state(&mut client, id, TransferState::Blocked);
    assert_eq!(blocked.blocked.unwrap().reason, "not yet");
    assert!(rig.server.requests().is_empty());

    // Resuming while the gatekeeper still says no is blocked again -- and again nothing is fetched.
    client.resume(id).unwrap();
    wait_for(&mut client, id, "a second block", |i| {
        i.state == TransferState::Blocked
            && i.events
                .iter()
                .filter(|e| e.message.contains("blocked by the gatekeeper"))
                .count()
                == 2
    });
    assert!(rig.server.requests().is_empty());

    allow.store(true, Ordering::SeqCst);
    client.resume(id).unwrap();
    let done = wait_state(&mut client, id, TransferState::Completed);
    assert!(
        done.blocked.is_none(),
        "the block is cleared once it proceeds"
    );
    assert_eq!(std::fs::read(&done.dest_path).unwrap(), body(5_000));
}

#[test]
fn the_manager_reports_when_it_is_idle_and_refuses_work_after_shutting_down() {
    let gate = FakeGatekeeper::clear_all();
    let download_dir = TempDir::new();
    let data_dir = TempDir::new();
    let server = TestServer::start();
    server.serve("/big.bin", slow(2 * MIB));
    let manager = TransferManager::open(config_for(&download_dir, &data_dir, &gate)).unwrap();

    assert!(manager.is_idle(), "nothing queued or running");
    let id = manager
        .start(&server.url("/big.bin"), None, false)
        .unwrap()
        .id;
    assert!(
        !manager.is_idle(),
        "a queued or running transfer is not idle"
    );
    wait_until(|| manager.get(id).unwrap().state == TransferState::Completed);
    assert!(
        manager.is_idle(),
        "a finished transfer leaves the process free to be torn down"
    );

    manager.shutdown();
    let refused = manager
        .start(&server.url("/big.bin"), None, false)
        .unwrap_err();
    assert_eq!(refused.code, ErrorCode::Internal);
    assert!(refused.to_string().contains("shutting down"), "{refused}");
}

#[test]
fn a_subscriber_that_falls_behind_skips_progress_but_always_sees_the_newest_state() {
    let gate = FakeGatekeeper::clear_all();
    let download_dir = TempDir::new();
    let data_dir = TempDir::new();
    let server = TestServer::start();
    server.serve("/big.bin", slow(2 * MIB));
    let mut config = config_for(&download_dir, &data_dir, &gate);
    config.options.tick = Duration::from_millis(2); // hundreds of updates per second
    let manager = TransferManager::open(config).unwrap();

    let never_drained = manager.subscribe();
    let id = manager
        .start(&server.url("/big.bin"), None, false)
        .unwrap()
        .id;
    wait_until(|| manager.get(id).unwrap().state == TransferState::Completed);
    // Anything `get` can see was pushed in the same critical section, so the
    // subscriber's record is at least this new. (It may be one newer: the job
    // thread records its own cleanup a moment after the engine reports the end.)
    let at_completion = manager.get(id).unwrap().generation;

    // Hundreds of changes happened, but this subscriber holds exactly one
    // record per transfer -- the latest -- so it cannot grow without bound
    // and cannot be left believing the download is still running.
    let pending = never_drained.drain();
    assert_eq!(
        pending.len(),
        1,
        "one record per transfer, not one per change: {pending:?}"
    );
    let SubscriptionUpdate::Updated(pending) = &pending[0] else {
        panic!("a completed transfer must still have an update: {pending:?}");
    };
    assert_eq!(pending.state, TransferState::Completed);
    assert!(
        pending.generation >= at_completion,
        "the newest state, not a stale one: {} < {at_completion}",
        pending.generation
    );
    assert!(never_drained.drain().is_empty(), "draining empties it");
    assert_eq!(
        manager.get(id).unwrap().completed_bytes,
        2 * MIB as u64,
        "the transfer itself was unaffected"
    );
}

#[test]
fn a_dropped_subscription_stops_receiving_and_is_forgotten() {
    let gate = FakeGatekeeper::clear_all();
    let download_dir = TempDir::new();
    let data_dir = TempDir::new();
    let server = TestServer::start();
    server.serve("/f.bin", Resource::new(body(10_000)));
    let manager = TransferManager::open(config_for(&download_dir, &data_dir, &gate)).unwrap();

    let subscription = manager.subscribe();
    assert!(
        subscription
            .recv_timeout(Duration::from_millis(50))
            .is_none(),
        "nothing has changed yet"
    );
    manager.start(&server.url("/f.bin"), None, false).unwrap();
    let first = subscription
        .recv_timeout(Duration::from_secs(5))
        .expect("an update for the new transfer");
    assert!(matches!(first, SubscriptionUpdate::Updated(info) if info.id == 1));
    drop(subscription);
    // Later changes must not fail or pile up anywhere.
    let second = manager
        .start(&server.url("/f.bin"), None, false)
        .unwrap()
        .id;
    wait_until(|| manager.get(second).unwrap().state == TransferState::Completed);
}

#[test]
fn a_store_captured_while_a_transfer_was_running_comes_back_paused_like_after_a_crash() {
    // A crash leaves whatever was last persisted -- here, `Active`. Copy the
    // store out from under a running manager and open a second manager over
    // the copy: that is exactly what a restart after a crash finds.
    let gate = FakeGatekeeper::clear_all();
    let download_dir = TempDir::new();
    let data_dir = TempDir::new();
    let server = TestServer::start();
    server.serve("/big.bin", slow(4 * MIB));
    let first = TransferManager::open(config_for(&download_dir, &data_dir, &gate)).unwrap();
    let id = first
        .start(&server.url("/big.bin"), None, false)
        .unwrap()
        .id;
    wait_until(|| {
        first
            .get(id)
            .map(|i| i.state == TransferState::Active && i.completed_bytes > 300 * KIB as u64)
            .unwrap_or(false)
    });
    let crashed_data = TempDir::new();
    wait_until(|| {
        std::fs::copy(
            data_dir.join("transfers.json"),
            crashed_data.join("transfers.json"),
        )
        .is_ok()
            && std::fs::read_to_string(crashed_data.join("transfers.json"))
                .map(|s| s.contains("\"active\""))
                .unwrap_or(false)
    });

    let second = TransferManager::open(config_for(&download_dir, &crashed_data, &gate)).unwrap();
    let found = second.get(id).unwrap();
    assert_eq!(found.state, TransferState::Paused, "{found:?}");
    assert_eq!(
        (found.speed_bps, found.connections, found.eta_secs),
        (0, 0, None),
        "nothing is running, so no live figures are shown"
    );
    assert!(has_event(&found, "restarted"), "{:?}", found.events);
    assert!(
        second.is_idle(),
        "a paused transfer does not keep the process from being torn down"
    );
    first.shutdown();
}

#[test]
fn pausing_an_already_paused_transfer_is_a_no_op() {
    let rig = Rig::new();
    rig.server.serve("/big.bin", slow(4 * MIB));
    let mut client = rig.client();
    let id = client.start(&rig.url("/big.bin"), None, false).unwrap().id;
    wait_for(&mut client, id, "progress", |i| {
        i.state == TransferState::Active && i.completed_bytes > 100 * KIB as u64
    });
    let first = client.pause(id).unwrap();
    let again = client.pause(id).unwrap();
    assert_eq!(
        (again.state, again.completed_bytes),
        (TransferState::Paused, first.completed_bytes)
    );
}

#[test]
fn shutting_down_puts_transfers_still_waiting_in_the_queue_back_to_paused() {
    let gate = FakeGatekeeper::clear_all();
    let download_dir = TempDir::new();
    let data_dir = TempDir::new();
    let server = TestServer::start();
    server.serve("/a.bin", slow(4 * MIB));
    server.serve("/b.bin", Resource::new(body(1_000)));
    let mut config = config_for(&download_dir, &data_dir, &gate);
    config.max_concurrent = 1;
    let manager = TransferManager::open(config).unwrap();
    let a = manager
        .start(&server.url("/a.bin"), None, false)
        .unwrap()
        .id;
    let b = manager
        .start(&server.url("/b.bin"), None, false)
        .unwrap()
        .id;
    wait_until(|| manager.get(a).unwrap().state == TransferState::Active);
    assert_eq!(manager.get(b).unwrap().state, TransferState::Queued);

    manager.shutdown();
    assert_eq!(manager.get(a).unwrap().state, TransferState::Paused);
    let queued = manager.get(b).unwrap();
    assert_eq!(
        queued.state,
        TransferState::Paused,
        "a queued transfer is not left to start on its own later"
    );
    assert!(has_event(&queued, "shut down"), "{:?}", queued.events);
    assert!(
        server.requests().iter().all(|r| r.path != "/b.bin"),
        "shutdown joined the only running job before it paused the queue, so the queued transfer was never started"
    );
}

#[test]
fn a_transfers_event_log_stays_bounded_across_many_pauses_and_resumes() {
    let rig = Rig::new();
    rig.server.serve("/big.bin", slow(6 * MIB));
    let mut client = rig.client();
    let id = client.start(&rig.url("/big.bin"), None, false).unwrap().id;
    for _ in 0..8 {
        wait_for(&mut client, id, "progress", |i| {
            i.state == TransferState::Active && i.completed_bytes > 0
        });
        client.pause(id).unwrap();
        client.resume(id).unwrap();
    }
    let done = wait_state(&mut client, id, TransferState::Completed);
    assert!(done.events.len() <= 32, "{} events", done.events.len());
    assert!(
        has_event(&done, "completed"),
        "the newest events are the ones kept: {:?}",
        done.events.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
    assert_eq!(std::fs::read(&done.dest_path).unwrap(), body(6 * MIB));
}

#[test]
fn queue_and_history_are_bounded_without_discarding_resumable_work() {
    let rig = Rig::with(|config| {
        config.max_concurrent = 1;
        config.max_queued = 1;
        config.max_history = 2;
    });
    rig.server.serve("/slow.bin", slow(2 * MIB));
    rig.server.serve("/one.bin", Resource::new(body(1_000)));
    rig.server.serve("/two.bin", Resource::new(body(1_000)));
    rig.server.serve("/three.bin", Resource::new(body(1_000)));
    let mut client = rig.client();

    let active = client.start(&rig.url("/slow.bin"), None, false).unwrap().id;
    wait_for(
        &mut client,
        active,
        "the first transfer to become active",
        |info| info.state == TransferState::Active,
    );
    let queued = client.start(&rig.url("/one.bin"), None, false).unwrap().id;
    assert_eq!(client.get(queued).unwrap().state, TransferState::Queued);
    assert_eq!(
        remote_code(client.start(&rig.url("/two.bin"), None, false)),
        ErrorCode::InvalidState,
        "a configured queue limit refuses new work rather than retaining it forever"
    );
    client.cancel(active).unwrap();
    wait_state(&mut client, queued, TransferState::Completed);

    assert_eq!(
        remote_code(client.start(&rig.url("/three.bin"), Some("reserved.blueice-part"), false)),
        ErrorCode::InvalidRequest,
        "an invalid request cannot evict useful history just because it arrives at the limit"
    );
    assert_eq!(
        client
            .list(None)
            .unwrap()
            .into_iter()
            .map(|info| info.id)
            .collect::<Vec<_>>(),
        vec![active, queued]
    );

    // The two existing terminal records fill the record limit. Starting a
    // third replaces the oldest terminal record, but never a live one.
    let third = client
        .start(&rig.url("/three.bin"), None, false)
        .unwrap()
        .id;
    wait_state(&mut client, third, TransferState::Completed);
    let retained: Vec<u64> = client
        .list(None)
        .unwrap()
        .into_iter()
        .map(|info| info.id)
        .collect();
    assert_eq!(retained, vec![queued, third]);
    assert_eq!(remote_code(client.get(active)), ErrorCode::NotFound);
}

#[test]
fn gatekeeper_text_is_bounded_before_it_reaches_transfer_records() {
    let oversized = "怪".repeat(3_000);
    let gate = FakeGatekeeper::start(move |_| GateReply::Reject {
        reason: oversized.clone(),
        category: "category".repeat(1_000),
    });
    let rig = Rig::with_gate(gate, |_| {});
    rig.server.serve("/f.bin", Resource::new(body(1_000)));
    let mut client = rig.client();
    let id = client.start(&rig.url("/f.bin"), None, false).unwrap().id;
    let blocked = wait_state(&mut client, id, TransferState::Blocked);
    let blocked = blocked
        .blocked
        .expect("the gatekeeper's reason is retained");
    assert!(blocked.reason.len() <= 4 * KIB);
    assert!(blocked.category.len() <= 4 * KIB);
    assert!(blocked.reason.ends_with("… [truncated]"));
    assert!(blocked.category.ends_with("… [truncated]"));
}

#[test]
fn cancelling_while_the_probe_is_backing_off_stops_promptly() {
    let rig = Rig::with(|c| {
        c.options.max_retries = 50;
        c.options.retry_backoff_base = Duration::from_secs(30);
        c.options.retry_backoff_max = Duration::from_secs(30);
    });
    rig.server.serve(
        "/f.bin",
        Resource {
            fail_statuses: vec![503; 100],
            ..Resource::new(body(1_000))
        },
    );
    let mut client = rig.client();
    let id = client.start(&rig.url("/f.bin"), None, false).unwrap().id;
    wait_for(&mut client, id, "the first probe retry", |i| {
        has_event(i, "retrying")
    });

    let started = Instant::now();
    assert_eq!(client.cancel(id).unwrap().state, TransferState::Cancelled);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "must not wait out a 30 s backoff, took {:?}",
        started.elapsed()
    );

    // ...and so does pausing.
    let again = client.start(&rig.url("/f.bin"), None, false).unwrap().id;
    wait_for(&mut client, again, "the first probe retry", |i| {
        has_event(i, "retrying")
    });
    let started = Instant::now();
    assert_eq!(client.pause(again).unwrap().state, TransferState::Paused);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "took {:?}",
        started.elapsed()
    );
}

#[test]
fn a_destination_that_appears_during_the_review_fails_instead_of_being_overwritten() {
    let gate = FakeGatekeeper::start(|_| GateReply::SlowClear(Duration::from_millis(300)));
    let rig = Rig::with_gate(gate, |c| c.review_timeout = Duration::from_secs(3));
    rig.server.serve("/f.bin", Resource::new(body(1_000)));
    let mut client = rig.client();
    let id = client
        .start(&rig.url("/f.bin"), Some("raced.bin"), false)
        .unwrap()
        .id;
    wait_for(&mut client, id, "the review to begin", |i| {
        i.state == TransferState::AwaitingClearance
    });
    std::fs::write(rig.in_dir("raced.bin"), b"appeared meanwhile").unwrap();

    let failed = wait_state(&mut client, id, TransferState::Failed);
    assert!(
        failed
            .last_error
            .as_deref()
            .unwrap_or("")
            .contains("already exists"),
        "{:?}",
        failed.last_error
    );
    assert_eq!(
        std::fs::read(rig.in_dir("raced.bin")).unwrap(),
        b"appeared meanwhile"
    );
}

// ---- refusals, history, filters ------------------------------------------

#[test]
fn operations_a_state_does_not_allow_are_refused_and_unknown_ids_are_not_found() {
    let rig = Rig::new();
    rig.server.serve("/done.bin", Resource::new(body(1_000)));
    rig.server.serve("/big.bin", slow(4 * MIB));
    let mut client = rig.client();
    let done = client.start(&rig.url("/done.bin"), None, false).unwrap().id;
    wait_state(&mut client, done, TransferState::Completed);
    let running = client.start(&rig.url("/big.bin"), None, false).unwrap().id;
    wait_for(&mut client, running, "running", |i| {
        i.state == TransferState::Active
    });

    assert_eq!(
        remote_code(client.pause(done)),
        ErrorCode::InvalidState,
        "pausing a completed transfer"
    );
    assert_eq!(
        remote_code(client.resume(done)),
        ErrorCode::InvalidState,
        "resuming a completed transfer"
    );
    assert_eq!(
        remote_code(client.resume(running)),
        ErrorCode::InvalidState,
        "resuming one that is running"
    );
    assert_eq!(
        remote_code(client.remove(running)),
        ErrorCode::InvalidState,
        "removing a running transfer"
    );
    assert_eq!(
        client.cancel(done).unwrap().state,
        TransferState::Completed,
        "cancelling a completed transfer is a no-op that keeps the file"
    );
    assert!(PathBuf::from(client.get(done).unwrap().dest_path).exists());

    for id in [999, 0] {
        assert_eq!(remote_code(client.get(id)), ErrorCode::NotFound);
        assert_eq!(remote_code(client.pause(id)), ErrorCode::NotFound);
        assert_eq!(remote_code(client.resume(id)), ErrorCode::NotFound);
        assert_eq!(remote_code(client.cancel(id)), ErrorCode::NotFound);
        assert_eq!(remote_code(client.remove(id)), ErrorCode::NotFound);
    }
}

#[test]
fn removing_a_finished_transfer_drops_it_from_history_and_from_disk_records() {
    let rig = Rig::new();
    rig.server.serve("/f.bin", Resource::new(body(1_000)));
    let mut client = rig.client();
    let id = client.start(&rig.url("/f.bin"), None, false).unwrap().id;
    wait_state(&mut client, id, TransferState::Completed);
    client.remove(id).unwrap();
    assert_eq!(remote_code(client.get(id)), ErrorCode::NotFound);
    assert!(client.list(None).unwrap().is_empty());
    assert!(
        rig.in_dir("f.bin").exists(),
        "removing history does not delete the downloaded file"
    );
    let stored = std::fs::read_to_string(rig.data_dir.join("transfers.json")).unwrap();
    assert!(
        !stored.contains("f.bin"),
        "the persisted record is gone too: {stored}"
    );
}

#[test]
fn removing_right_after_completion_never_races_the_job_cleanup() {
    // The engine reports the final state a moment before the job thread has
    // finished cleaning up; a client that removes the instant it sees
    // "completed" must still succeed. Repeated, because the window is tiny.
    let rig = Rig::new();
    rig.server.serve("/tiny.bin", Resource::new(body(500)));
    let mut client = rig.client();
    for round in 0..30 {
        let id = client
            .start(&rig.url("/tiny.bin"), Some(&format!("t{round}.bin")), false)
            .unwrap()
            .id;
        wait_state(&mut client, id, TransferState::Completed);
        client
            .remove(id)
            .unwrap_or_else(|e| panic!("round {round}: {e}"));
    }
    assert!(client.list(None).unwrap().is_empty());
}

#[test]
fn removing_a_failed_transfer_also_deletes_its_leftover_partial_files() {
    let rig = Rig::with(|c| c.options.max_retries = 1);
    rig.server.serve("/f.bin", Resource::new(body(MIB)));
    let mut client = rig.client();
    let id = client.start(&rig.url("/f.bin"), None, false).unwrap().id;
    // Let the probe through, then make every segment request fail.
    wait_for(&mut client, id, "the probe to have happened", |_| {
        !rig.server.requests().is_empty()
    });
    rig.server
        .update("/f.bin", |r| r.fail_statuses = vec![503; 500]);
    let failed = wait_state(&mut client, id, TransferState::Failed);
    let dest = PathBuf::from(&failed.dest_path);
    client.remove(id).unwrap();
    assert!(!part_path(&dest).exists() && !sidecar_path(&dest).exists());
}

#[test]
fn removing_a_blocked_resume_also_deletes_its_existing_partial_files() {
    let reject = Arc::new(AtomicBool::new(false));
    let for_gate = reject.clone();
    let gate = FakeGatekeeper::start(move |_| {
        if for_gate.load(Ordering::SeqCst) {
            GateReply::Reject {
                reason: "the verdict changed".to_string(),
                category: "test-block".to_string(),
            }
        } else {
            GateReply::Clear
        }
    });
    let rig = Rig::with_gate(gate, |_| {});
    rig.server.serve("/big.bin", slow(4 * MIB));
    let mut client = rig.client();
    let id = client.start(&rig.url("/big.bin"), None, false).unwrap().id;
    wait_for(&mut client, id, "progress", |info| {
        info.state == TransferState::Active && info.completed_bytes > 100 * KIB as u64
    });
    let dest = PathBuf::from(client.pause(id).unwrap().dest_path);
    assert!(part_path(&dest).exists() && sidecar_path(&dest).exists());

    reject.store(true, Ordering::SeqCst);
    client.resume(id).unwrap();
    assert_eq!(
        wait_state(&mut client, id, TransferState::Blocked).state,
        TransferState::Blocked
    );
    assert!(
        part_path(&dest).exists() && sidecar_path(&dest).exists(),
        "a review block must not silently discard resumable data"
    );

    client.remove(id).unwrap();
    assert!(!part_path(&dest).exists() && !sidecar_path(&dest).exists());
}

#[test]
fn list_can_be_filtered_by_state() {
    let rig = Rig::new();
    rig.server.serve("/ok.bin", Resource::new(body(1_000)));
    let mut client = rig.client();
    let ok = client.start(&rig.url("/ok.bin"), None, false).unwrap().id;
    let bad = client
        .start(&rig.url("/missing.bin"), None, false)
        .unwrap()
        .id;
    wait_state(&mut client, ok, TransferState::Completed);
    wait_state(&mut client, bad, TransferState::Failed);

    assert_eq!(client.list(None).unwrap().len(), 2);
    let completed: Vec<u64> = client
        .list(Some(TransferState::Completed))
        .unwrap()
        .iter()
        .map(|t| t.id)
        .collect();
    assert_eq!(completed, vec![ok]);
    let failed: Vec<u64> = client
        .list(Some(TransferState::Failed))
        .unwrap()
        .iter()
        .map(|t| t.id)
        .collect();
    assert_eq!(failed, vec![bad]);
    assert!(client.list(Some(TransferState::Paused)).unwrap().is_empty());
}

// ---- subscribers ---------------------------------------------------------

#[test]
fn a_subscriber_is_pushed_updates_with_rising_generations_ending_in_completion() {
    let rig = Rig::new();
    rig.server.serve("/f.bin", slow(MIB));
    let mut watcher = rig.client();
    watcher.subscribe().unwrap();
    let mut driver = rig.client();
    let id = driver.start(&rig.url("/f.bin"), None, false).unwrap().id;

    let mut seen: Vec<TransferInfo> = Vec::new();
    loop {
        let blueice_ipc::downloads::DownloadsUpdate::Updated(update) =
            watcher.next_update().unwrap()
        else {
            panic!("the transfer was not removed while it was running");
        };
        assert_eq!(update.id, id);
        let terminal = update.state.is_terminal();
        seen.push(*update);
        if terminal {
            break;
        }
    }
    assert!(
        seen.len() >= 3,
        "several updates while it ran, got {}",
        seen.len()
    );
    assert!(
        seen.windows(2).all(|w| w[0].generation < w[1].generation),
        "generations strictly increase: {:?}",
        seen.iter().map(|s| s.generation).collect::<Vec<_>>()
    );
    assert!(seen.iter().any(|s| s.state == TransferState::Active
        && s.completed_bytes > 0
        && s.completed_bytes < MIB as u64));
    assert_eq!(seen.last().unwrap().state, TransferState::Completed);
}

#[test]
fn a_subscriber_is_told_when_history_is_removed() {
    let rig = Rig::new();
    rig.server.serve("/f.bin", Resource::new(body(1_000)));
    let mut watcher = rig.client();
    watcher.subscribe().unwrap();
    let mut driver = rig.client();
    let id = driver.start(&rig.url("/f.bin"), None, false).unwrap().id;
    wait_state(&mut driver, id, TransferState::Completed);
    driver.remove(id).unwrap();

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match watcher.next_update().unwrap() {
            blueice_ipc::downloads::DownloadsUpdate::Updated(info) => {
                assert_eq!(info.id, id);
            }
            blueice_ipc::downloads::DownloadsUpdate::Removed { id: removed } => {
                assert_eq!(removed, id);
                break;
            }
        }
        assert!(
            Instant::now() < deadline,
            "the removal event was never pushed"
        );
    }
}

#[test]
fn a_subscriber_that_disconnects_does_not_disturb_anyone_else() {
    let rig = Rig::new();
    rig.server.serve("/f.bin", slow(MIB));
    let mut gone = rig.client();
    gone.subscribe().unwrap();
    drop(gone);
    let mut client = rig.client();
    let id = client.start(&rig.url("/f.bin"), None, false).unwrap().id;
    wait_state(&mut client, id, TransferState::Completed);
}

#[test]
fn a_subscriber_that_never_reads_cannot_stall_other_clients() {
    // Hold one transfer active so all the deliberately large updates below
    // remain queued. That makes the pusher write each distinct record in
    // order, rather than a completed transfer collapsing into a small final
    // update before the socket is pressured.
    let rig = Rig::with(|config| config.max_concurrent = 1);
    rig.server.serve("/active.bin", slow(16 * MIB));

    let stuck_stream = UnixStream::connect(rig.socket()).unwrap();
    let receive_window = constrain_receive_buffer(&stuck_stream);
    let unread_monitor = stuck_stream.try_clone().unwrap();
    let mut stuck = DownloadsClient::connect(stuck_stream).expect("handshake");
    stuck.subscribe().unwrap(); // and never reads a single pushed update

    let mut client = rig.client();
    let active = client
        .start(&rig.url("/active.bin"), None, false)
        .unwrap()
        .id;
    wait_for(&mut client, active, "the blocker to be active", |info| {
        info.state == TransferState::Active
    });

    let prefix = rig.url("/queued.bin?");
    // Every queued `TransferInfo` carries this almost-4 KiB URL. A framed
    // update is therefore larger than `MAX_TRANSFER_TEXT_BYTES`; generate
    // enough distinct ids to exceed the *actual* (not assumed) receive
    // window even when a platform clamps SO_RCVBUF upward.
    let suffix_width = 8;
    let fill = "x".repeat(MAX_TRANSFER_TEXT_BYTES - prefix.len() - suffix_width);
    let queued = receive_window / MAX_TRANSFER_TEXT_BYTES + 4;
    let mut update_bytes = 0;
    for n in 0..queued {
        let url = format!("{prefix}{fill}{n:0suffix_width$}");
        let info = client.start(&url, None, false).unwrap();
        assert_eq!(info.state, TransferState::Queued);
        let mut frame = Vec::new();
        write_downloads_reply(&mut frame, None, &DownloadsReply::Updated(info)).unwrap();
        update_bytes = update_bytes.max(frame.len());
    }
    assert!(
        update_bytes > MAX_TRANSFER_TEXT_BYTES,
        "the framing and transfer fields must make each queued update large enough to pressure the peer"
    );

    // Wait for bytes to arrive at the unread peer until one more known-sized
    // update cannot fit. There are still several distinct pending records,
    // so its pusher is now blocked in `write_all` on a full Unix socket. This
    // is the condition the old elapsed-time assertion never established.
    let threshold = receive_window.saturating_sub(update_bytes.saturating_sub(1));
    wait_until(|| unread_socket_bytes(&unread_monitor) >= threshold);
    assert!(
        unread_socket_bytes(&unread_monitor) + update_bytes > receive_window,
        "the next queued update cannot fit in the unread peer's {receive_window}-byte receive window"
    );

    // The blocked writer owns only this connection's writer mutex. A normal
    // client's request still completes; its own bounded socket deadline is
    // the protocol's deterministic failure mode if that isolation regresses.
    assert_eq!(client.get(active).unwrap().state, TransferState::Active);
    drop(stuck);
}

// ---- restarts and shutdown ----------------------------------------------

#[test]
fn a_new_process_sees_history_and_finds_interrupted_transfers_paused_never_auto_started() {
    let gate = FakeGatekeeper::clear_all();
    let download_dir = TempDir::new();
    let data_dir = TempDir::new();
    let server = TestServer::start();
    server.serve("/done.bin", Resource::new(body(1_000)));
    server.serve("/big.bin", slow(4 * MIB));

    let first = TransferManager::open(config_for(&download_dir, &data_dir, &gate)).unwrap();
    let done = first
        .start(&server.url("/done.bin"), None, false)
        .unwrap()
        .id;
    let big = first
        .start(&server.url("/big.bin"), None, false)
        .unwrap()
        .id;
    wait_until(|| first.get(done).unwrap().state == TransferState::Completed);
    wait_until(|| {
        first
            .get(big)
            .map(|i| i.state == TransferState::Active && i.completed_bytes > 300 * KIB as u64)
            .unwrap_or(false)
    });
    first.shutdown();
    drop(first);

    let second = TransferManager::open(config_for(&download_dir, &data_dir, &gate)).unwrap();
    assert_eq!(
        second.get(done).unwrap().state,
        TransferState::Completed,
        "history survives a restart"
    );
    let interrupted = second.get(big).unwrap();
    assert_eq!(
        interrupted.state,
        TransferState::Paused,
        "an interrupted transfer comes back paused"
    );
    assert!(
        interrupted.completed_bytes > 0,
        "and remembers how far it got"
    );
    assert!(
        second.is_idle(),
        "opening persisted history leaves the interrupted transfer paused with no job to restart it"
    );

    second.resume(big).unwrap();
    wait_until(|| second.get(big).unwrap().state == TransferState::Completed);
    assert_eq!(
        std::fs::read(second.get(big).unwrap().dest_path).unwrap(),
        body(4 * MIB)
    );
    let third = second.start(&server.url("/done.bin"), None, false).unwrap();
    assert!(third.id > big, "ids are never reused across restarts");
    second.shutdown();
}

#[test]
fn a_shutdown_request_pauses_running_transfers_and_stops_the_server() {
    let mut rig = Rig::new();
    rig.server.serve("/big.bin", slow(4 * MIB));
    let mut client = rig.client();
    let id = client.start(&rig.url("/big.bin"), None, false).unwrap().id;
    wait_for(&mut client, id, "progress", |i| {
        i.state == TransferState::Active && i.completed_bytes > 100 * KIB as u64
    });

    let mut raw = UnixStream::connect(rig.socket()).unwrap();
    blueice_ipc::downloads::write_downloads_request(
        &mut raw,
        Some(1),
        &DownloadsRequest::Hello {
            protocol_version: DOWNLOADS_PROTOCOL_VERSION,
        },
    )
    .unwrap();
    read_downloads_reply(&mut raw).unwrap();
    write_downloads_request(&mut raw, Some(2), &DownloadsRequest::Shutdown).unwrap();
    assert_eq!(
        read_downloads_reply(&mut raw).unwrap(),
        (Some(2), DownloadsReply::Ok)
    );

    rig.serving.take().unwrap().join().unwrap();
    assert_eq!(
        rig.manager.get(id).unwrap().state,
        TransferState::Paused,
        "shutting down checkpoints, it does not throw progress away"
    );
    assert!(
        UnixStream::connect(rig.socket())
            .map(|s| DownloadsClient::connect(s).is_err())
            .unwrap_or(true),
        "the server no longer answers"
    );
}

// ---- the handshake and fail-soft -----------------------------------------

#[test]
fn the_first_message_must_be_a_hello_of_a_supported_version() {
    let rig = Rig::new();

    let mut not_hello = UnixStream::connect(rig.socket()).unwrap();
    write_downloads_request(
        &mut not_hello,
        Some(1),
        &DownloadsRequest::List { state: None },
    )
    .unwrap();
    let (id, reply) = read_downloads_reply(&mut not_hello).unwrap();
    assert_eq!(id, Some(1));
    assert!(
        matches!(
            reply,
            DownloadsReply::Error {
                code: ErrorCode::InvalidRequest,
                ..
            }
        ),
        "{reply:?}"
    );
    assert!(
        read_downloads_reply(&mut not_hello).is_err(),
        "the connection is closed after a bad start"
    );

    let mut wrong_version = UnixStream::connect(rig.socket()).unwrap();
    write_downloads_request(
        &mut wrong_version,
        Some(1),
        &DownloadsRequest::Hello {
            protocol_version: DOWNLOADS_PROTOCOL_VERSION + 1,
        },
    )
    .unwrap();
    let (_, reply) = read_downloads_reply(&mut wrong_version).unwrap();
    assert!(
        matches!(
            reply,
            DownloadsReply::Error {
                code: ErrorCode::UnsupportedVersion,
                ..
            }
        ),
        "{reply:?}"
    );
    assert!(read_downloads_reply(&mut wrong_version).is_err());
}

#[test]
fn an_unrecognized_request_is_answered_with_an_error_and_the_connection_stays_usable() {
    let rig = Rig::new();
    let mut raw = UnixStream::connect(rig.socket()).unwrap();
    write_downloads_request(
        &mut raw,
        Some(1),
        &DownloadsRequest::Hello {
            protocol_version: DOWNLOADS_PROTOCOL_VERSION,
        },
    )
    .unwrap();
    read_downloads_reply(&mut raw).unwrap();

    write_downloads_request(&mut raw, Some(2), &DownloadsRequest::Unknown).unwrap();
    let (id, reply) = read_downloads_reply(&mut raw).unwrap();
    assert_eq!(id, Some(2));
    assert!(
        matches!(
            reply,
            DownloadsReply::Error {
                code: ErrorCode::InvalidRequest,
                ..
            }
        ),
        "{reply:?}"
    );

    write_downloads_request(
        &mut raw,
        Some(3),
        &DownloadsRequest::Hello {
            protocol_version: DOWNLOADS_PROTOCOL_VERSION,
        },
    )
    .unwrap();
    assert_eq!(
        read_downloads_reply(&mut raw).unwrap(),
        (
            Some(3),
            DownloadsReply::Hello {
                protocol_version: DOWNLOADS_PROTOCOL_VERSION
            }
        ),
        "a repeated Hello is just answered again"
    );
    write_downloads_request(&mut raw, Some(4), &DownloadsRequest::List { state: None }).unwrap();
    assert_eq!(
        read_downloads_reply(&mut raw).unwrap(),
        (Some(4), DownloadsReply::Transfers(Vec::new()))
    );
}

// ---- helpers for the in-process manager tests ----------------------------

fn config_for(download_dir: &TempDir, data_dir: &TempDir, gate: &FakeGatekeeper) -> ManagerConfig {
    let mut config = ManagerConfig::new(download_dir.path(), data_dir.path(), &gate.socket);
    config.options = fast_options();
    config.review_timeout = Duration::from_secs(2);
    config
}

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !condition() {
        assert!(Instant::now() < deadline, "timed out");
        thread::sleep(Duration::from_millis(10));
    }
}
