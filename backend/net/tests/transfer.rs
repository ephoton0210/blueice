// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The transfer engine through its public API, against the local test
//! server: segmentation and dynamic re-splitting, retries and failure
//! classification, pause/resume/cancel, resume validation, atomic
//! completion, and the stall watchdog.

mod common;

use blueice_ipc::downloads::{SegmentState, SingleStreamReason, TransferMode, TransferState};
use blueice_net::download::clearance::{DownloadClearance, Reviewer};
use blueice_net::download::probe::{Probe, probe};
use blueice_net::download::sidecar::{Sidecar, part_path, sidecar_path};
use blueice_net::download::transfer::{DownloadSpec, Snapshot, Transfer};
use blueice_net::download::{DownloadError, DownloadOptions};
use common::{FakeGatekeeper, Resource, TempDir, TestServer, body};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const KIB: usize = 1024;
const MIB: usize = 1024 * 1024;

/// A server, a gatekeeper that clears everything, and a scratch directory.
struct Rig {
    server: TestServer,
    gate: FakeGatekeeper,
    dir: TempDir,
}

impl Rig {
    fn new() -> Self {
        Rig {
            server: TestServer::start(),
            gate: FakeGatekeeper::clear_all(),
            dir: TempDir::new(),
        }
    }

    /// Small, fast settings so a test finishes in well under a second
    /// while still splitting a 1 MiB file into several segments.
    fn options() -> DownloadOptions {
        DownloadOptions {
            max_connections: 4,
            min_split_bytes: 64 * KIB as u64,
            max_retries: 3,
            retry_backoff_base: Duration::from_millis(10),
            retry_backoff_max: Duration::from_millis(40),
            stall_timeout: Duration::from_secs(20),
            buffer_bytes: 16 * KIB,
            checkpoint_interval: Duration::from_millis(30),
            tick: Duration::from_millis(15),
            ..DownloadOptions::default()
        }
    }

    /// Runs the review/probe/review sequence a real caller does.
    fn review(
        &self,
        path: &str,
        dest: &Path,
        options: &DownloadOptions,
    ) -> (Probe, DownloadClearance) {
        let reviewer = Reviewer::new(&self.gate.socket);
        let cleared = reviewer.review_url(&self.server.url(path)).unwrap();
        let probed = probe(&cleared, options).unwrap();
        let name = dest.file_name().unwrap().to_str().unwrap();
        let clearance = reviewer.review_download(cleared, &probed, name).unwrap();
        (probed, clearance)
    }

    fn try_begin(
        &self,
        path: &str,
        dest: &Path,
        options: DownloadOptions,
    ) -> Result<Transfer, DownloadError> {
        let (probed, clearance) = self.review(path, dest, &options);
        Transfer::begin(
            DownloadSpec {
                dest: dest.to_path_buf(),
                options,
                on_update: None,
            },
            probed,
            clearance,
        )
    }

    fn begin(&self, path: &str, dest: &Path, options: DownloadOptions) -> Transfer {
        self.try_begin(path, dest, options).expect("begin")
    }

    fn dest(&self, name: &str) -> std::path::PathBuf {
        self.dir.join(name)
    }
}

fn finish(transfer: &Transfer) -> Snapshot {
    transfer
        .wait_timeout(Duration::from_secs(30))
        .expect("the transfer did not settle in time")
}

fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn has_event(snapshot: &Snapshot, needle: &str) -> bool {
    snapshot.events.iter().any(|e| e.message.contains(needle))
}

fn slow_resource(len: usize, delay_ms: u64) -> Resource {
    Resource {
        chunk: 16 * KIB,
        delay_per_chunk: Duration::from_millis(delay_ms),
        ..Resource::new(body(len))
    }
}

fn ranged_requests(server: &TestServer) -> Vec<(u64, u64)> {
    server
        .requests()
        .iter()
        .filter(|r| r.status == 206)
        .filter_map(|r| r.range.as_deref())
        .filter_map(|r| r.strip_prefix("bytes="))
        .filter_map(|r| r.split_once('-'))
        .map(|(a, b)| (a.parse().unwrap(), b.parse().unwrap()))
        .collect()
}

// ---- happy paths --------------------------------------------------------

#[test]
fn a_file_is_downloaded_in_parallel_segments_and_arrives_byte_for_byte() {
    let rig = Rig::new();
    rig.server.serve("/big.bin", slow_resource(MIB, 10));
    let dest = rig.dest("big.bin");

    let transfer = rig.begin("/big.bin", &dest, Rig::options());
    let done = finish(&transfer);

    assert_eq!(done.state, TransferState::Completed, "{done:?}");
    assert_eq!(std::fs::read(&dest).unwrap(), body(MIB));
    assert_eq!(done.total_bytes, Some(MIB as u64));
    assert_eq!(done.completed_bytes, MIB as u64);
    assert_eq!(done.mode, TransferMode::Segmented);
    assert!(
        done.segments
            .iter()
            .all(|s| s.state == SegmentState::Done && s.completed == s.end - s.start)
    );
    assert!(
        rig.server.peak_concurrency() >= 2,
        "the segments must have been fetched concurrently, peak was {}",
        rig.server.peak_concurrency()
    );
    // The temporary files are gone: only the finished file is left.
    assert!(!part_path(&dest).exists() && !sidecar_path(&dest).exists());
    assert!(has_event(&done, "completed"), "{:?}", done.events);
}

#[test]
fn segments_cover_the_file_with_ranged_requests_that_carry_the_validator() {
    let rig = Rig::new();
    rig.server.serve("/f.bin", Resource::new(body(MIB)));
    let dest = rig.dest("f.bin");
    let transfer = rig.begin("/f.bin", &dest, Rig::options());
    finish(&transfer);

    let log = rig.server.requests();
    // The first request is the probe; every later ranged one must be an
    // If-Range request with the strong ETag the probe saw.
    let segment_requests: Vec<_> = log.iter().skip(1).filter(|r| r.range.is_some()).collect();
    assert!(segment_requests.len() >= 4, "{log:?}");
    assert!(
        segment_requests
            .iter()
            .all(|r| r.if_range.as_deref() == Some("\"v1\"")),
        "{segment_requests:?}"
    );
    assert!(
        segment_requests
            .iter()
            .all(|r| r.accept_encoding.as_deref() == Some("identity"))
    );
    // Together the segment requests cover the whole file with no gap. They
    // may *overlap*: when an idle connection splits a running segment, the
    // original request still names its old end and its worker simply stops
    // reading at the new one, so what matters is the union.
    let mut ranges: Vec<(u64, u64)> = ranged_requests(&rig.server).into_iter().skip(1).collect();
    ranges.sort();
    assert_eq!(ranges.first().unwrap().0, 0);
    let mut covered_to = 0u64;
    for (start, end) in &ranges {
        assert!(
            *start <= covered_to,
            "a gap before byte {start}: covered only to {covered_to} in {ranges:?}"
        );
        covered_to = covered_to.max(end + 1);
    }
    assert_eq!(covered_to, MIB as u64, "{ranges:?}");
}

#[test]
fn an_idle_connection_splits_the_slowest_segment_instead_of_finishing_early() {
    let rig = Rig::new();
    // Two connections; the segment starting at byte 0 is made slow, so the
    // other finishes first and must take over part of it.
    let slow: Arc<dyn Fn(Option<u64>) -> Duration + Send + Sync> = Arc::new(|start| {
        if start == Some(0) {
            Duration::from_millis(25)
        } else {
            Duration::ZERO
        }
    });
    rig.server.serve(
        "/f.bin",
        Resource {
            chunk: 16 * KIB,
            slow: Some(slow),
            ..Resource::new(body(MIB))
        },
    );
    let dest = rig.dest("f.bin");
    let options = DownloadOptions {
        max_connections: 2,
        min_split_bytes: 32 * KIB as u64,
        ..Rig::options()
    };

    let transfer = rig.begin("/f.bin", &dest, options);
    let done = finish(&transfer);

    assert_eq!(done.state, TransferState::Completed);
    assert_eq!(
        std::fs::read(&dest).unwrap(),
        body(MIB),
        "splitting must not corrupt the file"
    );
    let first_half = 0..(MIB as u64 / 2);
    let split_inside_slow_half = ranged_requests(&rig.server)
        .into_iter()
        .skip(1)
        .any(|(start, _)| start > 0 && first_half.contains(&start));
    assert!(
        split_inside_slow_half,
        "expected a request starting inside the slow first half: {:?}",
        ranged_requests(&rig.server)
    );
    assert!(has_event(&done, "split"), "{:?}", done.events);
}

#[test]
fn a_file_smaller_than_the_minimum_split_uses_one_segment() {
    let rig = Rig::new();
    rig.server.serve("/tiny.bin", Resource::new(body(10_000)));
    let dest = rig.dest("tiny.bin");
    let done = finish(&rig.begin("/tiny.bin", &dest, Rig::options()));
    assert_eq!(done.state, TransferState::Completed);
    assert_eq!(done.segments.len(), 1);
    assert_eq!(std::fs::read(&dest).unwrap(), body(10_000));
}

#[test]
fn a_server_that_ignores_ranges_is_downloaded_as_one_plain_stream() {
    let rig = Rig::new();
    rig.server.serve(
        "/f.bin",
        Resource {
            honor_ranges: false,
            ..Resource::new(body(300 * KIB))
        },
    );
    let dest = rig.dest("f.bin");
    let transfer = rig.begin("/f.bin", &dest, Rig::options());
    let snapshot = transfer.snapshot();
    assert_eq!(
        snapshot.mode,
        TransferMode::SingleStream {
            reason: SingleStreamReason::ServerIgnoresRange
        }
    );
    let done = finish(&transfer);

    assert_eq!(done.state, TransferState::Completed);
    assert_eq!(std::fs::read(&dest).unwrap(), body(300 * KIB));
    let log = rig.server.requests();
    assert_eq!(
        log.len(),
        2,
        "the probe plus exactly one transfer request: {log:?}"
    );
    assert_eq!(log[1].range, None, "a single stream is a plain GET");
}

#[test]
fn a_response_of_unknown_length_completes_when_the_stream_ends() {
    let rig = Rig::new();
    rig.server.serve(
        "/stream",
        Resource {
            honor_ranges: false,
            send_content_length: false,
            ..Resource::new(body(200 * KIB))
        },
    );
    let dest = rig.dest("stream.bin");
    let transfer = rig.begin("/stream", &dest, Rig::options());
    assert_eq!(
        transfer.snapshot().mode,
        TransferMode::SingleStream {
            reason: SingleStreamReason::ServerIgnoresRange
        }
    );
    let done = finish(&transfer);
    assert_eq!(done.state, TransferState::Completed);
    assert_eq!(done.completed_bytes, 200 * KIB as u64);
    assert_eq!(std::fs::read(&dest).unwrap(), body(200 * KIB));
}

#[test]
fn a_server_claiming_more_than_the_configured_limit_is_refused_before_a_part_file_is_created() {
    let rig = Rig::new();
    rig.server.serve("/f.bin", Resource::new(body(10_000)));
    let dest = rig.dest("f.bin");
    let options = DownloadOptions {
        max_total_bytes: Some(1_000),
        min_free_space_bytes: 0,
        ..Rig::options()
    };
    let (probed, clearance) = rig.review("/f.bin", &dest, &options);

    let result = Transfer::begin(
        DownloadSpec {
            dest: dest.clone(),
            options,
            on_update: None,
        },
        probed,
        clearance,
    );
    assert!(
        matches!(
            result,
            Err(DownloadError::SizeLimit {
                requested: 10_000,
                limit: 1_000
            })
        ),
        "{result:?}"
    );
    assert!(!part_path(&dest).exists() && !sidecar_path(&dest).exists());
}

#[test]
fn an_unknown_length_stream_cannot_exceed_the_configured_limit() {
    let rig = Rig::new();
    rig.server.serve(
        "/stream",
        Resource {
            send_content_length: false,
            honor_ranges: false,
            ..Resource::new(body(10_000))
        },
    );
    let dest = rig.dest("stream.bin");
    let options = DownloadOptions {
        max_total_bytes: Some(1_000),
        min_free_space_bytes: 0,
        ..Rig::options()
    };
    let (probed, clearance) = rig.review("/stream", &dest, &options);
    assert_eq!(probed.total_bytes(), None);

    let done = finish(
        &Transfer::begin(
            DownloadSpec {
                dest: dest.clone(),
                options,
                on_update: None,
            },
            probed,
            clearance,
        )
        .unwrap(),
    );
    assert_eq!(done.state, TransferState::Failed, "{done:?}");
    assert!(
        done.last_error
            .as_deref()
            .unwrap_or_default()
            .contains("limit"),
        "{done:?}"
    );
    assert!(!part_path(&dest).exists() && !sidecar_path(&dest).exists());
}

#[test]
fn an_empty_file_completes_at_once_as_an_empty_destination() {
    let rig = Rig::new();
    rig.server.serve("/empty", Resource::new(Vec::new()));
    let dest = rig.dest("empty.bin");
    let transfer = rig.begin("/empty", &dest, Rig::options());
    let done = finish(&transfer);
    assert_eq!(done.state, TransferState::Completed);
    assert_eq!(std::fs::read(&dest).unwrap(), Vec::<u8>::new());
    assert_eq!(
        rig.server.requests().len(),
        1,
        "nothing to fetch beyond the probe"
    );
}

// ---- failures and retries ----------------------------------------------

#[test]
fn transient_server_errors_are_retried_with_backoff_and_the_download_still_completes() {
    let rig = Rig::new();
    rig.server.serve("/f.bin", Resource::new(body(MIB)));
    let dest = rig.dest("f.bin");
    let options = Rig::options();
    let (probed, clearance) = rig.review("/f.bin", &dest, &options);
    // Fail the first two *transfer* requests (the probe is already done).
    rig.server
        .update("/f.bin", |r| r.fail_statuses = vec![503, 503]);

    let transfer = Transfer::begin(
        DownloadSpec {
            dest: dest.clone(),
            options,
            on_update: None,
        },
        probed,
        clearance,
    )
    .unwrap();
    let done = finish(&transfer);

    assert_eq!(done.state, TransferState::Completed, "{done:?}");
    assert_eq!(std::fs::read(&dest).unwrap(), body(MIB));
    assert!(done.retries >= 2, "retries was {}", done.retries);
    assert!(has_event(&done, "retrying"), "{:?}", done.events);
}

#[test]
fn a_connection_cut_mid_body_is_retried_from_where_it_stopped() {
    let rig = Rig::new();
    rig.server.serve("/f.bin", Resource::new(body(MIB)));
    let dest = rig.dest("f.bin");
    let options = Rig::options();
    let (probed, clearance) = rig.review("/f.bin", &dest, &options);
    rig.server.update("/f.bin", |r| {
        r.cut_body_after = Some(100 * KIB);
        r.cut_times = 3;
    });
    let transfer = Transfer::begin(
        DownloadSpec {
            dest: dest.clone(),
            options,
            on_update: None,
        },
        probed,
        clearance,
    )
    .unwrap();
    let done = finish(&transfer);

    assert_eq!(done.state, TransferState::Completed, "{done:?}");
    assert_eq!(std::fs::read(&dest).unwrap(), body(MIB));
    // A cut is only consumed by a body longer than the cut point, so how many
    // of the three happen depends on the request sizes; the first request
    // (a 256 KiB segment) always consumes one.
    assert!(done.retries >= 1, "retries was {}", done.retries);
    // The retry asks only for what is missing, not the whole segment again:
    // some request starts at a position that is not a segment boundary.
    let starts: Vec<u64> = ranged_requests(&rig.server)
        .into_iter()
        .skip(1)
        .map(|(s, _)| s)
        .collect();
    assert!(
        starts.iter().any(|s| s % (MIB as u64 / 4) != 0),
        "every request began on a segment boundary, so a cut segment was refetched from its start: {starts:?}"
    );
}

#[test]
fn progress_between_failures_resets_the_retry_budget() {
    // One segment (60 KiB is under the 64 KiB minimum split), cut three times
    // in a row, each time after fresh progress. With a budget of a single
    // retry this only completes if "consecutive failures" really means
    // failures *without progress in between*.
    let rig = Rig::new();
    rig.server.serve("/f.bin", Resource::new(body(60 * KIB)));
    let dest = rig.dest("f.bin");
    let options = DownloadOptions {
        max_retries: 1,
        ..Rig::options()
    };
    let (probed, clearance) = rig.review("/f.bin", &dest, &options);
    rig.server.update("/f.bin", |r| {
        r.cut_body_after = Some(10 * KIB);
        r.cut_times = 3;
    });

    let done = finish(
        &Transfer::begin(
            DownloadSpec {
                dest: dest.clone(),
                options,
                on_update: None,
            },
            probed,
            clearance,
        )
        .unwrap(),
    );
    assert_eq!(done.state, TransferState::Completed, "{done:?}");
    assert_eq!(done.segments.len(), 1);
    assert_eq!(
        done.retries, 3,
        "three cuts, three retries, none of them exhausting a budget of one"
    );
    assert_eq!(std::fs::read(&dest).unwrap(), body(60 * KIB));
}

#[test]
fn too_many_consecutive_failures_fail_the_transfer_but_keep_the_partial_download() {
    let rig = Rig::new();
    rig.server.serve("/f.bin", Resource::new(body(MIB)));
    let dest = rig.dest("f.bin");
    let options = DownloadOptions {
        max_retries: 2,
        ..Rig::options()
    };
    let (probed, clearance) = rig.review("/f.bin", &dest, &options);
    rig.server
        .update("/f.bin", |r| r.fail_statuses = vec![503; 500]);

    let transfer = Transfer::begin(
        DownloadSpec {
            dest: dest.clone(),
            options,
            on_update: None,
        },
        probed,
        clearance,
    )
    .unwrap();
    let done = finish(&transfer);

    assert_eq!(done.state, TransferState::Failed);
    assert!(
        done.last_error.as_deref().unwrap_or("").contains("503"),
        "{:?}",
        done.last_error
    );
    assert!(
        !dest.exists(),
        "a failed transfer must not leave a file at the destination"
    );
    assert!(
        sidecar_path(&dest).exists(),
        "the progress is kept so the download can be resumed"
    );
}

#[test]
fn a_client_error_on_a_segment_is_fatal_and_not_retried() {
    let rig = Rig::new();
    rig.server.serve("/f.bin", Resource::new(body(MIB)));
    let dest = rig.dest("f.bin");
    let options = Rig::options();
    let (probed, clearance) = rig.review("/f.bin", &dest, &options);
    rig.server.update("/f.bin", |r| r.fail_statuses = vec![404]);

    let transfer = Transfer::begin(
        DownloadSpec {
            dest,
            options,
            on_update: None,
        },
        probed,
        clearance,
    )
    .unwrap();
    let done = finish(&transfer);
    assert_eq!(done.state, TransferState::Failed);
    assert_eq!(done.retries, 0, "a 404 won't fix itself");
    assert!(done.last_error.as_deref().unwrap_or("").contains("404"));
}

#[test]
fn a_file_that_changes_between_probe_and_transfer_fails_instead_of_being_spliced() {
    let rig = Rig::new();
    rig.server.serve("/f.bin", Resource::new(body(MIB)));
    let dest = rig.dest("f.bin");
    let options = Rig::options();
    let (probed, clearance) = rig.review("/f.bin", &dest, &options);
    // The remote file is replaced after the probe recorded ETag "v1".
    rig.server
        .update("/f.bin", |r| r.etag = Some("\"v2\"".to_string()));

    let transfer = Transfer::begin(
        DownloadSpec {
            dest: dest.clone(),
            options,
            on_update: None,
        },
        probed,
        clearance,
    )
    .unwrap();
    let done = finish(&transfer);

    assert_eq!(done.state, TransferState::Failed);
    assert!(
        done.last_error.as_deref().unwrap_or("").contains("changed"),
        "{:?}",
        done.last_error
    );
    assert!(!dest.exists());
}

#[test]
fn a_server_whose_content_range_disagrees_with_the_request_is_rejected() {
    let rig = Rig::new();
    rig.server.serve("/f.bin", Resource::new(body(MIB)));
    let dest = rig.dest("f.bin");
    let options = Rig::options();
    let (probed, clearance) = rig.review("/f.bin", &dest, &options);
    rig.server.update("/f.bin", |r| {
        r.content_range_override = Some(format!("bytes 7-7/{MIB}"))
    });

    let done = finish(
        &Transfer::begin(
            DownloadSpec {
                dest,
                options,
                on_update: None,
            },
            probed,
            clearance,
        )
        .unwrap(),
    );
    assert_eq!(done.state, TransferState::Failed);
    assert_eq!(done.retries, 0, "a lying server is not a transient fault");
}

#[test]
fn a_stalled_connection_is_revoked_and_its_segment_retried() {
    let rig = Rig::new();
    rig.server.serve("/f.bin", Resource::new(body(MIB)));
    let dest = rig.dest("f.bin");
    let options = DownloadOptions {
        stall_timeout: Duration::from_millis(300),
        ..Rig::options()
    };
    let (probed, clearance) = rig.review("/f.bin", &dest, &options);
    // One connection goes silent after 50 KiB and stays that way for 8 s.
    rig.server.update("/f.bin", |r| {
        r.stall_after = Some(50 * KIB);
        r.stall_times = 1;
        r.stall_hold = Duration::from_secs(8);
    });

    let started = Instant::now();
    let transfer = Transfer::begin(
        DownloadSpec {
            dest: dest.clone(),
            options,
            on_update: None,
        },
        probed,
        clearance,
    )
    .unwrap();
    let done = finish(&transfer);

    assert_eq!(done.state, TransferState::Completed, "{done:?}");
    assert!(
        started.elapsed() < Duration::from_secs(6),
        "the watchdog must not wait out the server's 8 s stall, took {:?}",
        started.elapsed()
    );
    assert_eq!(std::fs::read(&dest).unwrap(), body(MIB));
    assert!(has_event(&done, "stalled"), "{:?}", done.events);
    assert!(done.retries >= 1);
}

#[test]
fn every_stalled_connection_is_replaced_without_waiting_for_its_read_to_return() {
    // All four initial workers enter a body read that the server holds for
    // eight seconds.  Counting physical threads rather than current segment
    // owners would make the watchdog revoke them but leave no logical slot
    // for the retry workers, permanently wedging the transfer.
    let rig = Rig::new();
    rig.server.serve("/f.bin", Resource::new(body(4 * MIB)));
    let dest = rig.dest("f.bin");
    let options = DownloadOptions {
        max_connections: 4,
        min_split_bytes: 64 * KIB as u64,
        stall_timeout: Duration::from_millis(200),
        max_retries: 3,
        retry_backoff_base: Duration::from_millis(10),
        retry_backoff_max: Duration::from_millis(20),
        tick: Duration::from_millis(15),
        ..Rig::options()
    };
    let (probed, clearance) = rig.review("/f.bin", &dest, &options);
    rig.server.update("/f.bin", |r| {
        r.stall_after = Some(50 * KIB);
        r.stall_times = 4;
        r.stall_hold = Duration::from_secs(8);
    });

    let transfer = Transfer::begin(
        DownloadSpec {
            dest: dest.clone(),
            options,
            on_update: None,
        },
        probed,
        clearance,
    )
    .unwrap();
    let done = transfer
        .wait_timeout(Duration::from_secs(4))
        .expect("all revoked connections must leave capacity for replacement workers");

    assert_eq!(done.state, TransferState::Completed, "{done:?}");
    assert_eq!(std::fs::read(&dest).unwrap(), body(4 * MIB));
    assert!(
        rig.server.requests().len() >= 9,
        "one probe, four stalled requests, and replacement requests: {:?}",
        rig.server.requests()
    );
    assert!(
        done.retries >= 4,
        "each stalled connection is accounted for: {done:?}"
    );
}

#[test]
fn repeatedly_stalled_reads_are_bounded_instead_of_accumulating_threads_forever() {
    let rig = Rig::new();
    let dest = rig.dest("f.bin");
    let options = DownloadOptions {
        max_connections: 1,
        stall_timeout: Duration::from_millis(100),
        max_retries: 100,
        retry_backoff_base: Duration::from_millis(1),
        retry_backoff_max: Duration::from_millis(2),
        tick: Duration::from_millis(10),
        // Three physical reads will be stuck in the test server by the time
        // this transfer fails. A replacement remains possible for the first
        // two, but a fourth cannot be permitted to make uninterruptible
        // ureq reads accumulate without bound.
        max_abandoned_workers: 2,
        ..Rig::options()
    };
    rig.server.serve("/f.bin", Resource::new(body(4 * MIB)));
    let (probed, clearance) = rig.review("/f.bin", &dest, &options);
    rig.server.update("/f.bin", |resource| {
        resource.stall_after = Some(50 * KIB);
        resource.stall_times = 3;
        resource.stall_hold = Duration::from_secs(8);
    });

    let transfer = Transfer::begin(
        DownloadSpec {
            dest,
            options,
            on_update: None,
        },
        probed,
        clearance,
    )
    .unwrap();
    let failed = transfer
        .wait_timeout(Duration::from_secs(2))
        .expect("the abandoned-worker limit must fail rather than wait for stuck reads");

    assert_eq!(failed.state, TransferState::Failed, "{failed:?}");
    assert!(
        failed
            .last_error
            .as_deref()
            .is_some_and(|error| error.contains("abandoned-worker limit")),
        "{failed:?}"
    );
    assert!(
        rig.server.requests().len() >= 4,
        "one probe plus the three stalled worker requests: {:?}",
        rig.server.requests()
    );
}

// ---- pause / resume / cancel --------------------------------------------

/// Starts a slow 4 MiB download and pauses it once real progress exists.
fn start_and_pause(rig: &Rig, name: &str) -> (std::path::PathBuf, Snapshot) {
    rig.server
        .serve(&format!("/{name}"), slow_resource(4 * MIB, 10));
    let dest = rig.dest(name);
    let transfer = rig.begin(&format!("/{name}"), &dest, Rig::options());
    wait_until("some progress", || {
        transfer.snapshot().completed_bytes > 300 * KIB as u64
    });
    transfer.pause();
    let paused = transfer.snapshot();
    assert_eq!(paused.state, TransferState::Paused);
    drop(transfer);
    (dest, paused)
}

#[test]
fn pausing_saves_progress_and_a_new_begin_resumes_from_it() {
    let rig = Rig::new();
    let (dest, paused) = start_and_pause(&rig, "big.bin");
    assert!(paused.completed_bytes > 0 && paused.completed_bytes < 4 * MIB as u64);
    assert!(paused.resume_safe);
    assert!(part_path(&dest).exists() && sidecar_path(&dest).exists());
    assert!(!dest.exists(), "the destination only appears on completion");

    let requests_before = rig.server.requests().len();
    let resumed = rig.begin("/big.bin", &dest, Rig::options());
    let first = resumed.snapshot();
    assert!(has_event(&first, "resumed"), "{:?}", first.events);
    assert!(
        first.completed_bytes >= paused.completed_bytes,
        "resuming must keep the saved progress: {} < {}",
        first.completed_bytes,
        paused.completed_bytes
    );
    let done = finish(&resumed);

    assert_eq!(done.state, TransferState::Completed);
    assert_eq!(std::fs::read(&dest).unwrap(), body(4 * MIB));
    // The second run fetched only what was missing.
    let fetched: u64 = ranged_requests(&rig.server)
        .into_iter()
        .skip(requests_before + 1)
        .map(|(a, b)| b - a + 1)
        .sum();
    assert!(
        fetched < 4 * MIB as u64,
        "resumed run refetched {fetched} bytes of a 4 MiB file"
    );
}

#[test]
fn pausing_a_server_with_nothing_to_validate_against_discards_the_progress() {
    let rig = Rig::new();
    rig.server.serve(
        "/f.bin",
        Resource {
            etag: None,
            last_modified: None,
            chunk: 16 * KIB,
            delay_per_chunk: Duration::from_millis(10),
            ..Resource::new(body(2 * MIB))
        },
    );
    let dest = rig.dest("f.bin");
    let transfer = rig.begin("/f.bin", &dest, Rig::options());
    assert!(
        !transfer.snapshot().resume_safe,
        "no ETag or Last-Modified: a pause can't be resumed safely"
    );
    assert!(
        has_event(&transfer.snapshot(), "restart"),
        "the user is told up front: {:?}",
        transfer.snapshot().events
    );
    wait_until("some progress", || {
        transfer.snapshot().completed_bytes > 100 * KIB as u64
    });

    transfer.pause();
    let paused = transfer.snapshot();
    assert_eq!(paused.state, TransferState::Paused);
    assert_eq!(
        paused.completed_bytes, 0,
        "progress is discarded, and the snapshot says so"
    );
    assert!(has_event(&paused, "discarded"), "{:?}", paused.events);
    assert!(!part_path(&dest).exists() && !sidecar_path(&dest).exists());
    drop(transfer);

    let done = finish(&rig.begin("/f.bin", &dest, Rig::options()));
    assert_eq!(done.state, TransferState::Completed);
    assert_eq!(std::fs::read(&dest).unwrap(), body(2 * MIB));
}

#[test]
fn cancelling_a_running_transfer_removes_every_partial_file() {
    let rig = Rig::new();
    rig.server.serve("/f.bin", slow_resource(4 * MIB, 10));
    let dest = rig.dest("f.bin");
    let transfer = rig.begin("/f.bin", &dest, Rig::options());
    wait_until("some progress", || {
        transfer.snapshot().completed_bytes > 100 * KIB as u64
    });

    transfer.cancel();
    assert_eq!(transfer.snapshot().state, TransferState::Cancelled);
    assert!(!dest.exists() && !part_path(&dest).exists() && !sidecar_path(&dest).exists());
}

#[test]
fn cancelling_a_paused_transfer_also_cleans_up() {
    let rig = Rig::new();
    rig.server.serve("/f.bin", slow_resource(4 * MIB, 10));
    let dest = rig.dest("f.bin");
    let transfer = rig.begin("/f.bin", &dest, Rig::options());
    wait_until("some progress", || {
        transfer.snapshot().completed_bytes > 100 * KIB as u64
    });
    transfer.pause();
    assert!(sidecar_path(&dest).exists());

    transfer.cancel();
    assert_eq!(transfer.snapshot().state, TransferState::Cancelled);
    assert!(!part_path(&dest).exists() && !sidecar_path(&dest).exists());
}

#[test]
fn pause_and_cancel_on_a_finished_transfer_change_nothing() {
    let rig = Rig::new();
    rig.server.serve("/f.bin", Resource::new(body(10_000)));
    let dest = rig.dest("f.bin");
    let transfer = rig.begin("/f.bin", &dest, Rig::options());
    assert_eq!(finish(&transfer).state, TransferState::Completed);
    transfer.pause();
    transfer.cancel();
    assert_eq!(transfer.snapshot().state, TransferState::Completed);
    assert_eq!(
        std::fs::read(&dest).unwrap(),
        body(10_000),
        "cancelling a completed download must not delete it"
    );
}

#[test]
fn dropping_a_running_transfer_leaves_it_resumable() {
    let rig = Rig::new();
    rig.server.serve("/f.bin", slow_resource(4 * MIB, 10));
    let dest = rig.dest("f.bin");
    let transfer = rig.begin("/f.bin", &dest, Rig::options());
    wait_until("some progress", || {
        transfer.snapshot().completed_bytes > 300 * KIB as u64
    });
    drop(transfer); // no explicit pause

    let resumed = rig.begin("/f.bin", &dest, Rig::options());
    assert!(
        has_event(&resumed.snapshot(), "resumed"),
        "{:?}",
        resumed.snapshot().events
    );
    assert_eq!(finish(&resumed).state, TransferState::Completed);
    assert_eq!(std::fs::read(&dest).unwrap(), body(4 * MIB));
}

// ---- resume validation --------------------------------------------------

#[test]
fn a_remote_file_that_changed_since_the_pause_restarts_from_zero_with_the_new_content() {
    let rig = Rig::new();
    let (dest, _) = start_and_pause(&rig, "big.bin");

    // The server now has a different file of the same length, new ETag.
    let replacement: Vec<u8> = body(4 * MIB).iter().map(|b| b.wrapping_add(1)).collect();
    rig.server.update("/big.bin", |r| {
        r.body = Arc::new(replacement.clone());
        r.etag = Some("\"v2\"".to_string());
        r.delay_per_chunk = Duration::ZERO;
    });

    let transfer = rig.begin("/big.bin", &dest, Rig::options());
    let first = transfer.snapshot();
    assert!(
        has_event(&first, "restarting from the beginning"),
        "{:?}",
        first.events
    );
    assert!(
        has_event(&first, "changed"),
        "the reason is given: {:?}",
        first.events
    );
    assert!(!has_event(&first, "resumed"));
    let done = finish(&transfer);
    assert_eq!(done.state, TransferState::Completed);
    assert_eq!(
        std::fs::read(&dest).unwrap(),
        replacement,
        "not one byte of the old file may survive in the result"
    );
}

#[test]
fn a_truncated_partial_file_restarts_from_zero() {
    let rig = Rig::new();
    let (dest, _) = start_and_pause(&rig, "big.bin");
    let part = std::fs::OpenOptions::new()
        .write(true)
        .open(part_path(&dest))
        .unwrap();
    part.set_len(1234).unwrap();
    drop(part);

    rig.server
        .update("/big.bin", |r| r.delay_per_chunk = Duration::ZERO);
    let transfer = rig.begin("/big.bin", &dest, Rig::options());
    assert!(
        has_event(&transfer.snapshot(), "restarting from the beginning"),
        "{:?}",
        transfer.snapshot().events
    );
    assert_eq!(finish(&transfer).state, TransferState::Completed);
    assert_eq!(std::fs::read(&dest).unwrap(), body(4 * MIB));
}

#[test]
fn a_corrupt_sidecar_is_ignored_and_the_download_starts_over() {
    let rig = Rig::new();
    let (dest, _) = start_and_pause(&rig, "big.bin");
    std::fs::write(sidecar_path(&dest), b"{ this is not json").unwrap();

    rig.server
        .update("/big.bin", |r| r.delay_per_chunk = Duration::ZERO);
    let transfer = rig.begin("/big.bin", &dest, Rig::options());
    assert!(!has_event(&transfer.snapshot(), "resumed"));
    assert_eq!(finish(&transfer).state, TransferState::Completed);
    assert_eq!(std::fs::read(&dest).unwrap(), body(4 * MIB));
}

#[test]
fn a_sidecar_for_a_different_url_is_not_resumed_from() {
    let rig = Rig::new();
    let (dest, _) = start_and_pause(&rig, "big.bin");
    // Same destination, but now asking for a different URL with the same length.
    rig.server.serve(
        "/other.bin",
        Resource::new(body(4 * MIB).iter().map(|b| b ^ 0xFF).collect()),
    );
    let transfer = rig.begin("/other.bin", &dest, Rig::options());
    assert!(
        has_event(&transfer.snapshot(), "restarting from the beginning"),
        "{:?}",
        transfer.snapshot().events
    );
    assert_eq!(finish(&transfer).state, TransferState::Completed);
    let expected: Vec<u8> = body(4 * MIB).iter().map(|b| b ^ 0xFF).collect();
    assert_eq!(std::fs::read(&dest).unwrap(), expected);
}

#[test]
fn files_captured_at_an_arbitrary_moment_are_resumable_like_after_a_crash() {
    // A crash leaves whatever the last checkpoint wrote. Copy the sidecar
    // and then the data file out from under a *running* transfer (in that
    // order: data only ever grows, so it is at least as far along as the
    // sidecar claims) and resume from the copy.
    let rig = Rig::new();
    rig.server.serve("/f.bin", slow_resource(4 * MIB, 10));
    let dest = rig.dest("f.bin");
    let transfer = rig.begin("/f.bin", &dest, Rig::options());
    wait_until("a checkpoint with progress", || {
        Sidecar::load(&dest).is_some_and(|s| {
            s.segments
                .iter()
                .any(|seg| seg.pos > seg.start + 100 * KIB as u64)
        })
    });

    let crash_dir = TempDir::new();
    let crashed = crash_dir.join("f.bin");
    std::fs::copy(sidecar_path(&dest), sidecar_path(&crashed)).unwrap();
    std::fs::copy(part_path(&dest), part_path(&crashed)).unwrap();
    transfer.cancel();

    rig.server
        .update("/f.bin", |r| r.delay_per_chunk = Duration::ZERO);
    let resumed = rig.begin("/f.bin", &crashed, Rig::options());
    assert!(
        has_event(&resumed.snapshot(), "resumed"),
        "{:?}",
        resumed.snapshot().events
    );
    assert_eq!(finish(&resumed).state, TransferState::Completed);
    assert_eq!(
        std::fs::read(&crashed).unwrap(),
        body(4 * MIB),
        "a mid-flight checkpoint must resume to a correct file"
    );
}

// ---- server variations found in the test review -------------------------

#[test]
fn last_modified_alone_is_enough_to_pause_resume_and_to_detect_a_change() {
    let rig = Rig::new();
    rig.server.serve(
        "/f.bin",
        Resource {
            etag: None,
            chunk: 16 * KIB,
            delay_per_chunk: Duration::from_millis(10),
            ..Resource::new(body(4 * MIB))
        },
    );
    let dest = rig.dest("f.bin");
    let transfer = rig.begin("/f.bin", &dest, Rig::options());
    assert!(
        transfer.snapshot().resume_safe,
        "Last-Modified is a usable validator"
    );
    wait_until("some progress", || {
        transfer.snapshot().completed_bytes > 300 * KIB as u64
    });
    transfer.pause();
    drop(transfer);

    rig.server
        .update("/f.bin", |r| r.delay_per_chunk = Duration::ZERO);
    let resumed = rig.begin("/f.bin", &dest, Rig::options());
    assert!(
        has_event(&resumed.snapshot(), "resumed"),
        "{:?}",
        resumed.snapshot().events
    );
    assert_eq!(finish(&resumed).state, TransferState::Completed);
    assert_eq!(std::fs::read(&dest).unwrap(), body(4 * MIB));

    // ...and a Last-Modified that moves after the probe is caught like an ETag.
    let other = rig.dest("g.bin");
    rig.server.serve(
        "/g.bin",
        Resource {
            etag: None,
            ..Resource::new(body(MIB))
        },
    );
    let options = Rig::options();
    let (probed, clearance) = rig.review("/g.bin", &other, &options);
    rig.server.update("/g.bin", |r| {
        r.last_modified = Some("Thu, 22 Oct 2015 07:28:00 GMT".to_string())
    });
    let failed = finish(
        &Transfer::begin(
            DownloadSpec {
                dest: other,
                options,
                on_update: None,
            },
            probed,
            clearance,
        )
        .unwrap(),
    );
    assert_eq!(failed.state, TransferState::Failed);
    assert!(
        failed
            .last_error
            .as_deref()
            .unwrap_or("")
            .contains("changed"),
        "{:?}",
        failed.last_error
    );
}

#[test]
fn a_server_that_hides_the_total_length_is_streamed_and_says_why() {
    let rig = Rig::new();
    // Ranges work, but the answer to the probe says `bytes 0-0/*`.
    rig.server.serve(
        "/f.bin",
        Resource {
            content_range_override: Some("bytes 0-0/*".to_string()),
            ..Resource::new(body(200 * KIB))
        },
    );
    let dest = rig.dest("f.bin");
    let transfer = rig.begin("/f.bin", &dest, Rig::options());
    assert_eq!(
        transfer.snapshot().mode,
        TransferMode::SingleStream {
            reason: SingleStreamReason::UnknownLength
        }
    );
    let done = finish(&transfer);
    assert_eq!(done.state, TransferState::Completed);
    assert_eq!(std::fs::read(&dest).unwrap(), body(200 * KIB));
}

#[test]
fn segments_are_fetched_from_the_final_url_not_through_the_redirect_again() {
    let rig = Rig::new();
    rig.server.serve(
        "/old",
        Resource {
            redirect_to: Some("/new".to_string()),
            ..Resource::new(Vec::new())
        },
    );
    rig.server.serve("/new", Resource::new(body(MIB)));
    let dest = rig.dest("f.bin");
    let done = finish(&rig.begin("/old", &dest, Rig::options()));
    assert_eq!(done.state, TransferState::Completed);
    assert_eq!(done.final_url, rig.server.url("/new"));
    assert_eq!(std::fs::read(&dest).unwrap(), body(MIB));
    // After the probe (which followed the redirect once), no request goes to /old.
    let log = rig.server.requests();
    let probe_end = log
        .iter()
        .position(|r| r.path == "/new")
        .expect("the probe reached /new");
    assert!(
        log[probe_end + 1..].iter().all(|r| r.path == "/new"),
        "{log:?}"
    );
}

#[test]
fn a_plain_stream_cut_mid_body_starts_over_and_still_completes() {
    let rig = Rig::new();
    rig.server.serve(
        "/f.bin",
        Resource {
            honor_ranges: false,
            ..Resource::new(body(300 * KIB))
        },
    );
    let dest = rig.dest("f.bin");
    let options = Rig::options();
    let (probed, clearance) = rig.review("/f.bin", &dest, &options);
    rig.server.update("/f.bin", |r| {
        r.cut_body_after = Some(100 * KIB);
        r.cut_times = 1;
    });
    let done = finish(
        &Transfer::begin(
            DownloadSpec {
                dest: dest.clone(),
                options,
                on_update: None,
            },
            probed,
            clearance,
        )
        .unwrap(),
    );

    assert_eq!(done.state, TransferState::Completed, "{done:?}");
    assert_eq!(std::fs::read(&dest).unwrap(), body(300 * KIB));
    assert!(done.retries >= 1);
    let transfer_requests = rig
        .server
        .requests()
        .iter()
        .skip(1)
        .filter(|r| r.range.is_none())
        .count();
    assert_eq!(
        transfer_requests, 2,
        "the cut attempt and the full retry from byte 0"
    );
}

#[test]
fn a_destination_whose_parent_cannot_be_created_is_a_file_error() {
    let rig = Rig::new();
    rig.server.serve("/f.bin", Resource::new(body(1_000)));
    let blocker = rig.dest("a-file");
    std::fs::write(&blocker, b"in the way").unwrap();
    let dest = blocker.join("child").join("f.bin");
    let result = rig.try_begin("/f.bin", &dest, Rig::options());
    assert!(matches!(result, Err(DownloadError::Io(_))), "{result:?}");
}

#[test]
fn the_event_log_is_bounded_and_drops_the_oldest_entries_first() {
    let rig = Rig::new();
    rig.server.serve("/f.bin", Resource::new(body(MIB)));
    let dest = rig.dest("f.bin");
    let options = DownloadOptions {
        max_retries: 100,
        retry_backoff_base: Duration::from_millis(1),
        retry_backoff_max: Duration::from_millis(2),
        ..Rig::options()
    };
    let (probed, clearance) = rig.review("/f.bin", &dest, &options);
    rig.server
        .update("/f.bin", |r| r.fail_statuses = vec![503; 60]);

    let done = finish(
        &Transfer::begin(
            DownloadSpec {
                dest,
                options,
                on_update: None,
            },
            probed,
            clearance,
        )
        .unwrap(),
    );
    assert_eq!(done.state, TransferState::Completed, "{done:?}");
    assert!(
        done.retries >= 40,
        "enough failures to overflow the log: {}",
        done.retries
    );
    assert_eq!(
        done.events.len(),
        32,
        "the log keeps only the most recent 32 events"
    );
    assert!(
        !has_event(&done, "byte ranges"),
        "the oldest event (the probe's finding) was dropped"
    );
    assert!(has_event(&done, "completed"), "the newest event is kept");
}

#[test]
fn wait_state_and_debug_describe_a_transfer_from_start_to_finish() {
    let rig = Rig::new();
    rig.server.serve("/f.bin", slow_resource(MIB, 5));
    let dest = rig.dest("f.bin");
    let transfer = rig.begin("/f.bin", &dest, Rig::options());
    assert_eq!(transfer.state(), TransferState::Active);
    assert!(format!("{transfer:?}").contains("Active"));

    let done = transfer.wait(); // blocks until the transfer settles
    assert_eq!(done.state, TransferState::Completed);
    assert_eq!(transfer.state(), TransferState::Completed);
    assert!(format!("{transfer:?}").contains("Completed"));
}

#[test]
fn a_plain_stream_that_gets_a_server_error_is_retried() {
    let rig = Rig::new();
    rig.server.serve(
        "/f.bin",
        Resource {
            honor_ranges: false,
            ..Resource::new(body(200 * KIB))
        },
    );
    let dest = rig.dest("f.bin");
    let options = Rig::options();
    let (probed, clearance) = rig.review("/f.bin", &dest, &options);
    rig.server.update("/f.bin", |r| r.fail_statuses = vec![503]);
    let done = finish(
        &Transfer::begin(
            DownloadSpec {
                dest: dest.clone(),
                options,
                on_update: None,
            },
            probed,
            clearance,
        )
        .unwrap(),
    );
    assert_eq!(done.state, TransferState::Completed);
    assert!(done.retries >= 1);
    assert_eq!(std::fs::read(&dest).unwrap(), body(200 * KIB));
}

#[test]
fn a_file_that_shrinks_under_an_unchanged_etag_is_caught_by_the_ranges_that_no_longer_fit() {
    let rig = Rig::new();
    rig.server.serve("/f.bin", Resource::new(body(MIB)));
    let dest = rig.dest("f.bin");
    let options = Rig::options();
    let (probed, clearance) = rig.review("/f.bin", &dest, &options);
    // Same ETag, but half the file: some segments now ask past the end (416),
    // the rest get a Content-Range with a different total.
    rig.server
        .update("/f.bin", |r| r.body = Arc::new(body(MIB / 2)));
    let done = finish(
        &Transfer::begin(
            DownloadSpec {
                dest: dest.clone(),
                options,
                on_update: None,
            },
            probed,
            clearance,
        )
        .unwrap(),
    );
    assert_eq!(done.state, TransferState::Failed);
    assert!(
        done.last_error.as_deref().unwrap_or("").contains("changed"),
        "{:?}",
        done.last_error
    );
    assert!(!dest.exists());
}

#[test]
fn a_range_answer_carrying_a_different_etag_or_last_modified_is_rejected() {
    // Defense in depth: even if a server (or a CDN in front of it) honors
    // If-Range, a `206` whose own validator differs from the probe's must
    // not be spliced in.
    for (what, change) in [
        (
            "ETag",
            (|r: &mut Resource| r.range_etag_override = Some("\"other\"".to_string()))
                as fn(&mut Resource),
        ),
        ("Last-Modified", |r: &mut Resource| {
            r.range_last_modified_override = Some("Fri, 01 Jan 2100 00:00:00 GMT".to_string())
        }),
    ] {
        let rig = Rig::new();
        // Only Last-Modified is a validator for the second case.
        let etag = if what == "ETag" {
            Some("\"v1\"".to_string())
        } else {
            None
        };
        rig.server.serve(
            "/f.bin",
            Resource {
                etag,
                ..Resource::new(body(MIB))
            },
        );
        let dest = rig.dest("f.bin");
        let options = Rig::options();
        let (probed, clearance) = rig.review("/f.bin", &dest, &options);
        rig.server.update("/f.bin", change);
        let done = finish(
            &Transfer::begin(
                DownloadSpec {
                    dest,
                    options,
                    on_update: None,
                },
                probed,
                clearance,
            )
            .unwrap(),
        );
        assert_eq!(done.state, TransferState::Failed, "{what}");
        assert_eq!(
            done.retries, 0,
            "{what}: a version mismatch is not transient"
        );
        assert!(
            done.last_error.as_deref().unwrap_or("").contains(what),
            "{what}: {:?}",
            done.last_error
        );
    }
}

#[test]
fn a_failed_transfer_that_cannot_be_resumed_leaves_no_partial_files() {
    let rig = Rig::new();
    rig.server.serve(
        "/f.bin",
        Resource {
            etag: None,
            last_modified: None,
            ..Resource::new(body(MIB))
        },
    );
    let dest = rig.dest("f.bin");
    let options = DownloadOptions {
        max_retries: 1,
        ..Rig::options()
    };
    let (probed, clearance) = rig.review("/f.bin", &dest, &options);
    rig.server
        .update("/f.bin", |r| r.fail_statuses = vec![503; 500]);
    let done = finish(
        &Transfer::begin(
            DownloadSpec {
                dest: dest.clone(),
                options,
                on_update: None,
            },
            probed,
            clearance,
        )
        .unwrap(),
    );
    assert_eq!(done.state, TransferState::Failed);
    assert!(
        !part_path(&dest).exists() && !sidecar_path(&dest).exists(),
        "with nothing to validate a resume against, keeping the bytes would only mislead"
    );
}

// ---- destination and clearance -----------------------------------------

#[test]
fn an_existing_destination_is_refused_unless_overwriting_is_allowed() {
    let rig = Rig::new();
    rig.server.serve("/f.bin", Resource::new(body(10_000)));
    let dest = rig.dest("f.bin");
    std::fs::write(&dest, b"precious").unwrap();

    let refused = rig.try_begin("/f.bin", &dest, Rig::options());
    assert!(
        matches!(refused, Err(DownloadError::DestinationExists(_))),
        "{refused:?}"
    );
    assert_eq!(
        std::fs::read(&dest).unwrap(),
        b"precious",
        "a refused download must not touch the existing file"
    );

    let allowed = rig.begin(
        "/f.bin",
        &dest,
        DownloadOptions {
            overwrite: true,
            ..Rig::options()
        },
    );
    assert_eq!(finish(&allowed).state, TransferState::Completed);
    assert_eq!(std::fs::read(&dest).unwrap(), body(10_000));
}

#[test]
fn a_dangling_destination_symlink_is_never_followed_even_for_an_empty_file() {
    let rig = Rig::new();
    rig.server.serve("/empty.bin", Resource::new(Vec::new()));
    let dest = rig.dest("empty.bin");
    let outside = rig.dir.join("outside.bin");
    std::os::unix::fs::symlink(&outside, &dest).unwrap();

    let refused = rig.try_begin("/empty.bin", &dest, Rig::options());
    assert!(
        matches!(refused, Err(DownloadError::DestinationExists(ref path)) if path == &dest),
        "{refused:?}"
    );
    assert!(
        !outside.exists(),
        "opening a dangling destination link must not create its target"
    );
    assert!(
        std::fs::symlink_metadata(&dest)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn a_parent_directory_symlink_is_never_followed_while_creating_a_part_file() {
    let rig = Rig::new();
    rig.server.serve("/f.bin", Resource::new(body(1_000)));
    let outside = TempDir::new();
    std::os::unix::fs::symlink(outside.path(), rig.dir.path().join("link")).unwrap();
    let dest = rig.dir.path().join("link").join("f.bin");

    let result = rig.try_begin("/f.bin", &dest, Rig::options());
    assert!(matches!(result, Err(DownloadError::Io(_))), "{result:?}");
    assert!(!outside.join("f.bin").exists() && !outside.join("f.bin.blueice-part").exists());
}

#[test]
fn a_destination_that_appears_during_the_transfer_is_not_clobbered() {
    let rig = Rig::new();
    rig.server.serve("/f.bin", slow_resource(MIB, 10));
    let dest = rig.dest("f.bin");
    let transfer = rig.begin("/f.bin", &dest, Rig::options());
    std::fs::write(&dest, b"appeared meanwhile").unwrap();

    let done = finish(&transfer);
    assert_eq!(done.state, TransferState::Failed);
    assert!(
        done.last_error
            .as_deref()
            .unwrap_or("")
            .contains("already exists"),
        "{:?}",
        done.last_error
    );
    assert_eq!(std::fs::read(&dest).unwrap(), b"appeared meanwhile");
}

#[test]
fn missing_parent_directories_are_created() {
    let rig = Rig::new();
    rig.server.serve("/f.bin", Resource::new(body(10_000)));
    let dest = rig.dir.path().join("a").join("b").join("f.bin");
    let done = finish(&rig.begin("/f.bin", &dest, Rig::options()));
    assert_eq!(done.state, TransferState::Completed);
    assert_eq!(std::fs::read(&dest).unwrap(), body(10_000));
}

#[test]
fn a_clearance_cannot_be_spent_on_a_different_file_name() {
    let rig = Rig::new();
    rig.server.serve("/f.bin", Resource::new(body(10_000)));
    let reviewed = rig.dest("reviewed.bin");
    let options = Rig::options();
    let (probed, clearance) = rig.review("/f.bin", &reviewed, &options);
    // Reviewed as "reviewed.bin", but asked to write "sneaky.exe".
    let result = Transfer::begin(
        DownloadSpec {
            dest: rig.dest("sneaky.exe"),
            options,
            on_update: None,
        },
        probed,
        clearance,
    );
    assert!(
        matches!(result, Err(DownloadError::ClearanceMismatch(_))),
        "{result:?}"
    );
    assert!(!rig.dest("sneaky.exe").exists() && !part_path(&rig.dest("sneaky.exe")).exists());
}

// ---- observability ------------------------------------------------------

#[test]
fn a_snapshot_reports_speed_eta_segments_and_connections_while_running() {
    let rig = Rig::new();
    rig.server.serve("/f.bin", slow_resource(4 * MIB, 10));
    let dest = rig.dest("f.bin");
    let transfer = rig.begin("/f.bin", &dest, Rig::options());

    let mut seen_speed = false;
    let mut seen_eta = false;
    let mut seen_active_segment = false;
    wait_until("speed, ETA and an active segment to show up", || {
        let s = transfer.snapshot();
        seen_speed |= s.speed_bps > 0;
        seen_eta |= s.eta.is_some();
        seen_active_segment |= s
            .segments
            .iter()
            .any(|seg| seg.state == SegmentState::Active)
            && s.connections >= 1;
        seen_speed && seen_eta && seen_active_segment
    });
    let s = transfer.snapshot();
    assert_eq!(s.state, TransferState::Active);
    assert_eq!(s.total_bytes, Some(4 * MIB as u64));
    assert_eq!(s.mode, TransferMode::Segmented);
    assert!(s.resume_safe);
    assert_eq!(s.final_url, rig.server.url("/f.bin"));
    assert_eq!(s.content_type.as_deref(), Some("application/octet-stream"));
    assert!(s.segments.len() >= 4);
    assert!(
        has_event(&s, "byte ranges"),
        "the probe's finding is in the event log: {:?}",
        s.events
    );
    transfer.cancel();
}

#[test]
fn the_update_callback_sees_progress_and_a_final_completed_snapshot() {
    let rig = Rig::new();
    rig.server.serve("/f.bin", slow_resource(MIB, 10));
    let dest = rig.dest("f.bin");
    let seen: Arc<Mutex<Vec<Snapshot>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    let options = Rig::options();
    let (probed, clearance) = rig.review("/f.bin", &dest, &options);
    let spec = DownloadSpec {
        dest: dest.clone(),
        options,
        on_update: Some(Arc::new(move |snapshot: &Snapshot| {
            sink.lock().unwrap().push(snapshot.clone())
        })),
    };

    let transfer = Transfer::begin(spec, probed, clearance).unwrap();
    finish(&transfer);

    let seen = seen.lock().unwrap();
    assert!(
        seen.len() >= 3,
        "expected several updates, got {}",
        seen.len()
    );
    assert!(
        seen.iter().any(|s| s.state == TransferState::Active
            && s.completed_bytes > 0
            && s.completed_bytes < MIB as u64),
        "an in-progress update"
    );
    let last = seen.last().unwrap();
    assert_eq!(last.state, TransferState::Completed);
    assert_eq!(last.completed_bytes, MIB as u64);
}

#[test]
fn wait_timeout_gives_up_when_the_transfer_has_not_settled() {
    let rig = Rig::new();
    rig.server.serve("/f.bin", slow_resource(4 * MIB, 10));
    let dest = rig.dest("f.bin");
    let transfer = rig.begin("/f.bin", &dest, Rig::options());
    assert!(transfer.wait_timeout(Duration::from_millis(50)).is_none());
    transfer.cancel();
    assert_eq!(
        transfer.wait_timeout(Duration::from_secs(5)).unwrap().state,
        TransferState::Cancelled
    );
}
